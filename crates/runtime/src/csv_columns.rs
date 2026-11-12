//! Reads a whole CSV file straight into typed columns (`csv_read_columns`).
//!
//! The file is streamed in buffered blocks and never held whole. Fields are read as byte
//! slices of the record and parsed into each column's storage without building a value per
//! field. A column whose kind is not fixed starts undecided and keeps parsed values only for
//! the kinds that can still hold everything seen so far; one that turns out to be text is read
//! again in a second pass, as text, because the text of its earlier values was not kept.

use super::resources::{io_error, read_csv_record, ReaderError, ReaderErrorKind, ReaderResource};
use std::{
    cell::Cell,
    collections::{BTreeMap, HashSet},
    fs::File,
    hash::BuildHasher,
    io::BufRead,
    path::Path,
};

/// Size of the read buffer.
const BUFFER_BYTES: usize = 64 * 1024;
/// Rows between checks of the allocation budget and the cancel flag.
const CHUNK_ROWS: usize = 4096;
/// Bytes of record text between those checks, for files with very long records.
const CHUNK_BYTES: usize = 1 << 20;
/// What a dictionary entry costs beyond its text: the `String` itself.
const ENTRY_OVERHEAD: usize = 24;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColumnKind {
    I64,
    F64,
    Bool,
    Text,
}

impl ColumnKind {
    pub fn name(self) -> &'static str {
        match self {
            ColumnKind::I64 => "i64",
            ColumnKind::F64 => "f64",
            ColumnKind::Bool => "bool",
            ColumnKind::Text => "text",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "i64" => Some(ColumnKind::I64),
            "f64" => Some(ColumnKind::F64),
            "bool" => Some(ColumnKind::Bool),
            "text" => Some(ColumnKind::Text),
            _ => None,
        }
    }
}

/// One column. Only the storage for `kind` has the column's length (for text, `codes` and
/// `dictionary`); the other vectors are empty. `valid` is true where a value is present.
#[derive(Clone, Debug, PartialEq)]
pub struct Column {
    pub name: String,
    pub kind: ColumnKind,
    pub ints: Vec<i64>,
    pub floats: Vec<f64>,
    pub flags: Vec<bool>,
    pub codes: Vec<i64>,
    pub dictionary: Vec<String>,
    pub valid: Vec<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ColumnsError {
    /// Opening or reading the file failed, or a record was over the limit.
    Reader(ReaderError),
    /// The file is not well-formed CSV, or a record has the wrong number of fields.
    Malformed {
        line: usize,
        column: usize,
        message: String,
    },
    /// A value does not fit a fixed kind, or `kinds` names something that is not there.
    Value {
        line: usize,
        column: usize,
        message: String,
    },
    /// The allocation budget refused this many more bytes.
    Allocation(usize),
    /// The invocation was cancelled while the file was being read.
    Cancelled,
}

/// What the read reports to and asks of its host.
pub struct Meter<'a> {
    /// Charges this many bytes to the allocation budget; false when it is exhausted.
    pub charge: &'a mut dyn FnMut(usize) -> bool,
    /// Gives back bytes that were charged for buffers since freed.
    pub release: &'a mut dyn FnMut(usize),
    pub cancelled: &'a dyn Fn() -> bool,
}

/// Where a field's text lies in the record text, and whether the field was quoted.
#[derive(Clone, Copy)]
struct Span {
    start: usize,
    end: usize,
    quoted: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// No value seen yet; the kind is still open.
    Unset,
    Int,
    Float,
    Bool,
    Text,
    /// An inferred column found to be text; its values are discarded until the second pass.
    Reread,
}

/// Dictionary of the distinct texts of a column, with codes in first-seen order.
struct Dictionary {
    values: Vec<String>,
    table: Vec<usize>,
    hasher: std::collections::hash_map::RandomState,
    text_bytes: usize,
}

const EMPTY_SLOT: usize = usize::MAX;

impl Dictionary {
    fn new() -> Self {
        Self {
            values: Vec::new(),
            table: Vec::new(),
            hasher: Default::default(),
            text_bytes: 0,
        }
    }

    fn code(&mut self, text: &str) -> usize {
        if self.table.is_empty() {
            self.table = vec![EMPTY_SLOT; 16];
        }
        let mask = self.table.len() - 1;
        let mut slot = (self.hasher.hash_one(text) as usize) & mask;
        loop {
            let entry = self.table[slot];
            if entry == EMPTY_SLOT {
                break;
            }
            if self.values[entry] == text {
                return entry;
            }
            slot = (slot + 1) & mask;
        }
        let code = self.values.len();
        self.values.push(text.to_owned());
        self.text_bytes += text.len();
        self.table[slot] = code;
        if self.values.len() * 2 > self.table.len() {
            self.grow();
        }
        code
    }

    fn grow(&mut self) {
        let size = self.table.len() * 2;
        let mut table = vec![EMPTY_SLOT; size];
        let mask = size - 1;
        for (code, value) in self.values.iter().enumerate() {
            let mut slot = (self.hasher.hash_one(value.as_str()) as usize) & mask;
            while table[slot] != EMPTY_SLOT {
                slot = (slot + 1) & mask;
            }
            table[slot] = code;
        }
        self.table = table;
    }

    fn accounted_bytes(&self) -> usize {
        self.text_bytes
            .saturating_add(self.values.capacity().saturating_mul(ENTRY_OVERHEAD))
            .saturating_add(self.table.capacity().saturating_mul(8))
    }
}

struct Builder {
    name: String,
    fixed: bool,
    mode: Mode,
    quoted_empty: bool,
    ints: Vec<i64>,
    floats: Vec<f64>,
    flags: Vec<bool>,
    codes: Vec<i64>,
    dictionary: Dictionary,
    valid: Vec<bool>,
}

/// Why a builder refused a field.
enum Refused {
    /// The value does not fit the column's fixed kind.
    DoesNotFit,
    /// The budget refused this many bytes for a transient copy.
    Budget(usize),
}

impl Builder {
    fn new(name: String, kind: Option<ColumnKind>) -> Self {
        let mode = match kind {
            None => Mode::Unset,
            Some(ColumnKind::I64) => Mode::Int,
            Some(ColumnKind::F64) => Mode::Float,
            Some(ColumnKind::Bool) => Mode::Bool,
            Some(ColumnKind::Text) => Mode::Text,
        };
        Self {
            name,
            fixed: kind.is_some(),
            mode,
            quoted_empty: false,
            ints: Vec::new(),
            floats: Vec::new(),
            flags: Vec::new(),
            codes: Vec::new(),
            dictionary: Dictionary::new(),
            valid: Vec::new(),
        }
    }

    fn fixed_kind_name(&self) -> &'static str {
        match self.mode {
            Mode::Int => "i64",
            Mode::Float => "f64",
            Mode::Bool => "bool",
            _ => "text",
        }
    }

    /// Whether this column must be read again as text, because its kind is text and the text
    /// of its values was not kept.
    fn needs_text_pass(&self) -> bool {
        self.mode == Mode::Reread || (self.mode == Mode::Unset && self.quoted_empty)
    }

    fn give_up_numbers(&mut self) {
        self.mode = Mode::Reread;
        self.ints = Vec::new();
        self.floats = Vec::new();
        self.flags = Vec::new();
        self.valid = Vec::new();
    }

    fn push_missing(&mut self) {
        match self.mode {
            Mode::Int => self.ints.push(0),
            Mode::Float => self.floats.push(0.0),
            Mode::Bool => self.flags.push(false),
            Mode::Text => self.codes.push(0),
            Mode::Unset | Mode::Reread => {}
        }
        self.valid.push(false);
    }

    /// Takes one field. An unquoted empty field is missing; a quoted empty one is missing too,
    /// except in a text column, where it is the empty text.
    fn ingest(&mut self, text: &str, quoted: bool, meter: &mut Meter<'_>) -> Result<(), Refused> {
        match self.mode {
            Mode::Reread => return Ok(()),
            Mode::Text => {
                if text.is_empty() && !quoted {
                    self.push_missing();
                } else {
                    let code = self.dictionary.code(text);
                    self.codes.push(code as i64);
                    self.valid.push(true);
                }
                return Ok(());
            }
            _ => {}
        }
        if text.is_empty() {
            self.quoted_empty |= quoted;
            self.push_missing();
            return Ok(());
        }
        match self.mode {
            Mode::Unset => {
                let rows = self.valid.len();
                if let Some(value) = parse_i64(text) {
                    self.ints.resize(rows, 0);
                    self.ints.push(value);
                    self.mode = Mode::Int;
                } else if let Some(value) = parse_f64(text) {
                    self.floats.resize(rows, 0.0);
                    self.floats.push(value);
                    self.mode = Mode::Float;
                } else if let Some(value) = parse_bool(text) {
                    self.flags.resize(rows, false);
                    self.flags.push(value);
                    self.mode = Mode::Bool;
                } else {
                    self.give_up_numbers();
                    return Ok(());
                }
            }
            Mode::Int => {
                if let Some(value) = parse_i64(text) {
                    self.ints.push(value);
                } else if self.fixed {
                    return Err(Refused::DoesNotFit);
                } else if let Some(value) = parse_f64(text) {
                    // The floats are built while the ints are still held: charge that copy
                    // before making it and give it back once the ints are freed. The floats
                    // themselves are charged with the builder at the next settle.
                    let copy = self.ints.len().saturating_mul(8);
                    if !(meter.charge)(copy) {
                        return Err(Refused::Budget(copy));
                    }
                    self.floats = self.ints.iter().map(|&int| int as f64).collect();
                    self.floats.push(value);
                    self.ints = Vec::new();
                    (meter.release)(copy);
                    self.mode = Mode::Float;
                } else {
                    self.give_up_numbers();
                    return Ok(());
                }
            }
            Mode::Float => {
                if let Some(value) = parse_f64(text) {
                    self.floats.push(value);
                } else if self.fixed {
                    return Err(Refused::DoesNotFit);
                } else {
                    self.give_up_numbers();
                    return Ok(());
                }
            }
            Mode::Bool => {
                if let Some(value) = parse_bool(text) {
                    self.flags.push(value);
                } else if self.fixed {
                    return Err(Refused::DoesNotFit);
                } else {
                    self.give_up_numbers();
                    return Ok(());
                }
            }
            Mode::Text | Mode::Reread => unreachable!("handled above"),
        }
        self.valid.push(true);
        Ok(())
    }

    fn accounted_bytes(&self) -> usize {
        self.ints
            .capacity()
            .saturating_mul(8)
            .saturating_add(self.floats.capacity().saturating_mul(8))
            .saturating_add(self.flags.capacity())
            .saturating_add(self.codes.capacity().saturating_mul(8))
            .saturating_add(self.valid.capacity())
            .saturating_add(self.dictionary.accounted_bytes())
    }

    fn finish(mut self) -> Column {
        let rows = self.valid.len();
        let kind = match self.mode {
            Mode::Int => ColumnKind::I64,
            Mode::Float => ColumnKind::F64,
            Mode::Bool => ColumnKind::Bool,
            Mode::Text | Mode::Unset | Mode::Reread => ColumnKind::Text,
        };
        if self.mode == Mode::Unset {
            // No present value: text, every row missing.
            self.codes = vec![0; rows];
        }
        Column {
            name: self.name,
            kind,
            ints: self.ints,
            floats: self.floats,
            flags: self.flags,
            codes: self.codes,
            dictionary: self.dictionary.values,
            valid: self.valid,
        }
    }
}

/// An integer in the i64 range: an optional sign and at least one digit, nothing else.
fn parse_i64(text: &str) -> Option<i64> {
    let bytes = text.as_bytes();
    let (negative, digits) = match bytes.first()? {
        b'-' => (true, &bytes[1..]),
        b'+' => (false, &bytes[1..]),
        _ => (false, bytes),
    };
    if digits.is_empty() {
        return None;
    }
    // Up to 18 digits cannot overflow, which covers nearly every field.
    if digits.len() <= 18 {
        let mut value = 0i64;
        for &byte in digits {
            if !byte.is_ascii_digit() {
                return None;
            }
            value = value * 10 + i64::from(byte - b'0');
        }
        return Some(if negative { -value } else { value });
    }
    if !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    text.parse::<i64>().ok()
}

/// What `text_parse_float` accepts: a finite decimal floating-point number.
fn parse_f64(text: &str) -> Option<f64> {
    text.parse::<f64>().ok().filter(|value| value.is_finite())
}

fn parse_bool(text: &str) -> Option<bool> {
    match text {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// Splits the text of one record into fields, the way `csv_next` does: the text of an
/// unquoted field as is, a quoted field without its quotes and with `""` as `"`. Unescaped
/// text is appended to `scratch` and the spans point into it.
fn split_general(
    record: &str,
    start_line: usize,
    scratch: &mut String,
    spans: &mut Vec<Span>,
) -> Result<(), (usize, usize, String)> {
    #[derive(Clone, Copy, PartialEq)]
    enum State {
        Start,
        Unquoted,
        Quoted,
        AfterQuote,
    }
    scratch.clear();
    spans.clear();
    let mut state = State::Start;
    let mut field_start = 0usize;
    let mut quoted = false;
    let mut chars = record.char_indices().peekable();
    while let Some((index, character)) = chars.next() {
        match state {
            State::Start => match character {
                '"' => {
                    state = State::Quoted;
                    quoted = true;
                }
                ',' => {
                    spans.push(Span {
                        start: field_start,
                        end: scratch.len(),
                        quoted,
                    });
                    field_start = scratch.len();
                    quoted = false;
                }
                '\n' => break,
                '\r' if chars.peek().is_none() => break,
                _ => {
                    scratch.push(character);
                    state = State::Unquoted;
                }
            },
            State::Unquoted => match character {
                '"' => {
                    return Err((
                        start_line,
                        index + 1,
                        "quote inside unquoted CSV field".into(),
                    ));
                }
                ',' => {
                    spans.push(Span {
                        start: field_start,
                        end: scratch.len(),
                        quoted,
                    });
                    field_start = scratch.len();
                    quoted = false;
                    state = State::Start;
                }
                '\n' => break,
                _ => scratch.push(character),
            },
            State::Quoted => match character {
                '"' if chars.peek().is_some_and(|(_, next)| *next == '"') => {
                    chars.next();
                    scratch.push('"');
                }
                '"' => state = State::AfterQuote,
                _ => scratch.push(character),
            },
            State::AfterQuote => match character {
                ',' => {
                    spans.push(Span {
                        start: field_start,
                        end: scratch.len(),
                        quoted,
                    });
                    field_start = scratch.len();
                    quoted = false;
                    state = State::Start;
                }
                '\n' => break,
                '\r' if chars.peek().is_none() => break,
                _ => {
                    return Err((
                        start_line,
                        index + 1,
                        "unexpected character after closing CSV quote".into(),
                    ));
                }
            },
        }
    }
    if state == State::Quoted {
        return Err((
            start_line,
            record.len() + 1,
            "unterminated quoted CSV field".into(),
        ));
    }
    spans.push(Span {
        start: field_start,
        end: scratch.len(),
        quoted,
    });
    Ok(())
}

fn open(path: &Path, max_record_bytes: usize) -> Result<ReaderResource, ColumnsError> {
    let file = File::open(path).map_err(|error| {
        ColumnsError::Reader(ReaderError::new(
            ReaderErrorKind::Io,
            format!("cannot open file: {error}"),
            0,
            0,
        ))
    })?;
    let mut resource = ReaderResource::standalone(file, BUFFER_BYTES, max_record_bytes);
    // A UTF-8 byte-order mark is not part of the first name.
    let available = resource
        .reader
        .fill_buf()
        .map_err(|error| ColumnsError::Reader(io_error(error, 1)))?;
    if available.starts_with(&[0xEF, 0xBB, 0xBF]) {
        resource.reader.consume(3);
        resource.bytes_read = 3;
    }
    Ok(resource)
}

/// Calls `each` with the line, the text and the fields of every record that is not a blank
/// line, in order. While `blank_is_value` is set (the header has one column), a blank line is
/// the single empty field of a record instead of being skipped.
fn for_each_record(
    resource: &mut ReaderResource,
    blank_is_value: &Cell<bool>,
    mut each: impl FnMut(usize, &str, &[Span]) -> Result<(), ColumnsError>,
) -> Result<(), ColumnsError> {
    let mut spans: Vec<Span> = Vec::new();
    let mut scratch = String::new();
    loop {
        if resource.eof {
            return Ok(());
        }
        let line = resource.physical_line;
        let available = resource
            .reader
            .fill_buf()
            .map_err(|error| ColumnsError::Reader(io_error(error, line)))?;
        if available.is_empty() {
            resource.eof = true;
            return Ok(());
        }
        // The common record: whole in the buffer, valid UTF-8, no quote or stray carriage
        // return. Its fields are the pieces between commas, and nothing is copied.
        if let Some(newline) = memchr::memchr(b'\n', available) {
            let consumed = newline + 1;
            if consumed <= resource.max_record_bytes {
                let text = &available[..newline];
                let text = text.strip_suffix(b"\r").unwrap_or(text);
                if memchr::memchr2(b'"', b'\r', text).is_none() {
                    if let Ok(text) = std::str::from_utf8(text) {
                        if !text.is_empty() || blank_is_value.get() {
                            spans.clear();
                            let mut start = 0;
                            for comma in memchr::memchr_iter(b',', text.as_bytes()) {
                                spans.push(Span {
                                    start,
                                    end: comma,
                                    quoted: false,
                                });
                                start = comma + 1;
                            }
                            spans.push(Span {
                                start,
                                end: text.len(),
                                quoted: false,
                            });
                            each(line, text, &spans)?;
                        }
                        resource.reader.consume(consumed);
                        resource.bytes_read += consumed;
                        resource.physical_line += 1;
                        continue;
                    }
                }
            }
        }
        let Some(mut record) = read_csv_record(resource).map_err(ColumnsError::Reader)? else {
            return Ok(());
        };
        if record.last() == Some(&b'\n') {
            record.pop();
            if record.last() == Some(&b'\r') {
                record.pop();
            }
        }
        if record.is_empty() {
            if blank_is_value.get() {
                spans.clear();
                spans.push(Span {
                    start: 0,
                    end: 0,
                    quoted: false,
                });
                each(line, "", &spans)?;
            }
            continue;
        }
        let text = std::str::from_utf8(&record).map_err(|error| ColumnsError::Malformed {
            line,
            column: error.valid_up_to() + 1,
            message: "CSV data is not valid UTF-8".into(),
        })?;
        split_general(text, line, &mut scratch, &mut spans).map_err(
            |(line, column, message)| ColumnsError::Malformed {
                line,
                column,
                message,
            },
        )?;
        each(line, &scratch, &spans)?;
    }
}

/// Charges what the builders have grown by and checks for cancellation.
struct Settler<'a, 'm> {
    meter: &'a mut Meter<'m>,
    charged: usize,
    rows: usize,
    bytes: usize,
}

impl Settler<'_, '_> {
    fn note(&mut self, record_bytes: usize) -> bool {
        self.rows += 1;
        self.bytes += record_bytes;
        self.rows >= CHUNK_ROWS || self.bytes >= CHUNK_BYTES
    }

    fn settle(&mut self, builders: &[Builder], buffers: usize) -> Result<(), ColumnsError> {
        self.rows = 0;
        self.bytes = 0;
        let live = builders
            .iter()
            .map(Builder::accounted_bytes)
            .fold(buffers, usize::saturating_add);
        if live > self.charged {
            let more = live - self.charged;
            if !(self.meter.charge)(more) {
                return Err(ColumnsError::Allocation(more));
            }
            self.charged = live;
        }
        if (self.meter.cancelled)() {
            return Err(ColumnsError::Cancelled);
        }
        Ok(())
    }
}

fn bad_value(builder: &Builder, text: &str, line: usize, column: usize) -> ColumnsError {
    ColumnsError::Value {
        line,
        column,
        message: format!(
            "column `{}`: `{}` is not {}",
            builder.name,
            text,
            builder.fixed_kind_name()
        ),
    }
}

/// Reads the CSV file at `path` into columns. `kinds` fixes the kind of columns by name.
/// Each record is held to `max_record_bytes`; the file as a whole is held to nothing.
pub fn read_csv_columns(
    path: &Path,
    kinds: &BTreeMap<String, String>,
    max_record_bytes: usize,
    meter: &mut Meter<'_>,
) -> Result<Vec<Column>, ColumnsError> {
    read_csv_columns_between(path, kinds, max_record_bytes, meter, || {})
}

/// What the result of a read holds beyond the arrays and the dictionary text, both of which
/// were charged as the builders grew (a dictionary's text is charged there and only there).
pub fn result_overhead(columns: &[Column]) -> usize {
    columns
        .iter()
        .map(|column| column.name.len().saturating_add(256))
        .sum()
}

fn changed_while_read() -> ColumnsError {
    ColumnsError::Malformed {
        line: 0,
        column: 0,
        message: "the file changed while it was being read".into(),
    }
}

/// `read_csv_columns` with a hook that runs between the first and second pass (a seam for
/// tests; the production hook is empty and is compiled away).
fn read_csv_columns_between(
    path: &Path,
    kinds: &BTreeMap<String, String>,
    max_record_bytes: usize,
    meter: &mut Meter<'_>,
    between_passes: impl FnOnce(),
) -> Result<Vec<Column>, ColumnsError> {
    // What this read holds on the budget: whatever it charged and has not released. A read
    // that fails frees every builder, so it hands all of that back; a read that succeeds
    // keeps the charge for the columns it returns.
    let held = std::cell::Cell::new(0usize);
    let result = {
        let mut charge = |bytes: usize| {
            let granted = (meter.charge)(bytes);
            if granted {
                held.set(held.get().saturating_add(bytes));
            }
            granted
        };
        let mut release = |bytes: usize| {
            (meter.release)(bytes);
            held.set(held.get().saturating_sub(bytes));
        };
        let mut tracked = Meter {
            charge: &mut charge,
            release: &mut release,
            cancelled: meter.cancelled,
        };
        read_passes(path, kinds, max_record_bytes, &mut tracked, between_passes)
    };
    if result.is_err() {
        (meter.release)(held.get());
    }
    result
}

fn read_passes(
    path: &Path,
    kinds: &BTreeMap<String, String>,
    max_record_bytes: usize,
    meter: &mut Meter<'_>,
    between_passes: impl FnOnce(),
) -> Result<Vec<Column>, ColumnsError> {
    let mut resource = open(path, max_record_bytes)?;
    let mut records = 0usize;
    let mut builders: Vec<Builder> = Vec::new();
    let mut header_seen = false;
    let mut settler = Settler {
        meter,
        charged: 0,
        rows: 0,
        bytes: 0,
    };
    // The read buffer, a record buffer of the largest size seen, and the field scratch space.
    let mut buffers = BUFFER_BYTES;
    settler.settle(&builders, buffers)?;
    let one_column = Cell::new(false);
    for_each_record(&mut resource, &one_column, |line, text, spans| {
        if !header_seen {
            header_seen = true;
            one_column.set(spans.len() == 1);
            let names: Vec<&str> = spans
                .iter()
                .map(|span| &text[span.start..span.end])
                .collect();
            for name in &names {
                let kind =
                    match kinds.get(*name) {
                        None => None,
                        Some(kind) => Some(ColumnKind::from_name(kind).ok_or_else(|| {
                            ColumnsError::Value {
                                line: 1,
                                column: 0,
                                message: format!("unknown kind `{kind}`"),
                            }
                        })?),
                    };
                builders.push(Builder::new((*name).to_owned(), kind));
            }
            let present: HashSet<&str> = names.iter().copied().collect();
            if let Some(missing) = kinds.keys().find(|name| !present.contains(name.as_str())) {
                return Err(ColumnsError::Value {
                    line: 1,
                    column: 0,
                    message: format!("no column named `{missing}`"),
                });
            }
            return Ok(());
        }
        if spans.len() != builders.len() {
            return Err(ColumnsError::Malformed {
                line,
                column: 0,
                message: format!(
                    "CSV record has {} fields, expected {}",
                    spans.len(),
                    builders.len()
                ),
            });
        }
        for (index, (builder, span)) in builders.iter_mut().zip(spans).enumerate() {
            let field = &text[span.start..span.end];
            match builder.ingest(field, span.quoted, settler.meter) {
                Ok(()) => {}
                Err(Refused::DoesNotFit) => {
                    return Err(bad_value(builder, field, line, index + 1));
                }
                Err(Refused::Budget(bytes)) => return Err(ColumnsError::Allocation(bytes)),
            }
        }
        records += 1;
        buffers = buffers.max(BUFFER_BYTES + text.len() * 2);
        if settler.note(text.len()) {
            settler.settle(&builders, buffers)?;
        }
        Ok(())
    })?;
    settler.settle(&builders, buffers)?;
    if !header_seen {
        return Err(ColumnsError::Malformed {
            line: 1,
            column: 0,
            message: "CSV file has no header row".into(),
        });
    }
    let again: Vec<usize> = builders
        .iter()
        .enumerate()
        .filter(|(_, builder)| builder.needs_text_pass())
        .map(|(index, _)| index)
        .collect();
    let mut columns: Vec<Option<Column>> = Vec::new();
    let names: Vec<String> = builders
        .iter()
        .map(|builder| builder.name.clone())
        .collect();
    // The first pass's buffers for the columns that are read again are freed below; what stays
    // charged is what the kept columns hold, and the second pass is charged on top of it.
    let retained = builders
        .iter()
        .enumerate()
        .filter(|(index, _)| !again.contains(index))
        .map(|(_, builder)| builder.accounted_bytes())
        .fold(0usize, usize::saturating_add);
    (settler.meter.release)(settler.charged.saturating_sub(retained));
    for (index, builder) in builders.into_iter().enumerate() {
        columns.push(if again.contains(&index) {
            None
        } else {
            Some(builder.finish())
        });
    }
    if !again.is_empty() {
        // The columns that turned out to be text, read again as text. The second pass must see
        // the file the first one saw: the same header and as many records.
        between_passes();
        let mut resource = open(path, max_record_bytes)?;
        let mut texts: Vec<Builder> = again
            .iter()
            .map(|&index| Builder::new(names[index].clone(), Some(ColumnKind::Text)))
            .collect();
        let mut header_seen = false;
        let mut buffers = BUFFER_BYTES;
        let mut settler = Settler {
            meter: settler.meter,
            charged: 0,
            rows: 0,
            bytes: 0,
        };
        let mut again_records = 0usize;
        let one_column = Cell::new(false);
        for_each_record(&mut resource, &one_column, |line, text, spans| {
            if !header_seen {
                header_seen = true;
                one_column.set(spans.len() == 1);
                let same = spans.len() == names.len()
                    && spans
                        .iter()
                        .zip(&names)
                        .all(|(span, name)| &text[span.start..span.end] == name);
                return if same {
                    Ok(())
                } else {
                    Err(changed_while_read())
                };
            }
            if again_records == records {
                // The file grew since the first pass.
                return Err(changed_while_read());
            }
            again_records += 1;
            if spans.len() != names.len() {
                return Err(ColumnsError::Malformed {
                    line,
                    column: 0,
                    message: format!(
                        "CSV record has {} fields, expected {}",
                        spans.len(),
                        names.len()
                    ),
                });
            }
            for (builder, &index) in texts.iter_mut().zip(&again) {
                let span = spans[index];
                // A text column accepts every field and promotes nothing.
                let _ = builder.ingest(&text[span.start..span.end], span.quoted, settler.meter);
            }
            buffers = buffers.max(BUFFER_BYTES + text.len() * 2);
            if settler.note(text.len()) {
                settler.settle(&texts, buffers)?;
            }
            Ok(())
        })?;
        if !header_seen || again_records != records {
            return Err(changed_while_read());
        }
        settler.settle(&texts, buffers)?;
        for (builder, &index) in texts.into_iter().zip(&again) {
            columns[index] = Some(builder.finish());
        }
    }
    Ok(columns
        .into_iter()
        .map(|column| column.expect("every column was built"))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integers_parse_like_text_parse_int_within_the_i64_range() {
        assert_eq!(parse_i64("0"), Some(0));
        assert_eq!(parse_i64("-0"), Some(0));
        assert_eq!(parse_i64("+7"), Some(7));
        assert_eq!(parse_i64("007"), Some(7));
        assert_eq!(parse_i64("9223372036854775807"), Some(i64::MAX));
        assert_eq!(parse_i64("-9223372036854775808"), Some(i64::MIN));
        assert_eq!(parse_i64("9223372036854775808"), None);
        assert_eq!(parse_i64("-9223372036854775809"), None);
        assert_eq!(parse_i64(""), None);
        assert_eq!(parse_i64("-"), None);
        assert_eq!(parse_i64("1.0"), None);
        assert_eq!(parse_i64("1e5"), None);
        assert_eq!(parse_i64(" 1"), None);
        assert_eq!(parse_i64("123456789012345678901234567890"), None);
    }

    #[test]
    fn floats_must_be_finite_decimals() {
        assert_eq!(parse_f64("1e5"), Some(100000.0));
        assert_eq!(parse_f64(".5"), Some(0.5));
        assert_eq!(parse_f64("inf"), None);
        assert_eq!(parse_f64("nan"), None);
        assert_eq!(parse_f64("1e999"), None);
        assert_eq!(parse_f64("true"), None);
    }

    fn read_with_hook(path: &Path, between: impl FnOnce()) -> Result<Vec<Column>, ColumnsError> {
        let mut charge = |_: usize| true;
        let mut release = |_: usize| {};
        let cancelled = || false;
        let mut meter = Meter {
            charge: &mut charge,
            release: &mut release,
            cancelled: &cancelled,
        };
        read_csv_columns_between(path, &BTreeMap::new(), 1 << 20, &mut meter, between)
    }

    fn changed_file(name: &str, change: impl FnOnce(&Path)) -> Result<Vec<Column>, ColumnsError> {
        let path = std::env::temp_dir().join(format!(
            "guion-csv-columns-{}-{name}.csv",
            std::process::id()
        ));
        std::fs::write(&path, "n,s\n1,a\n2,b\n").unwrap();
        let result = read_with_hook(&path, || change(&path));
        let _ = std::fs::remove_file(&path);
        result
    }

    fn assert_changed(result: Result<Vec<Column>, ColumnsError>) {
        match result {
            Err(ColumnsError::Malformed { message, .. }) => {
                assert_eq!(message, "the file changed while it was being read")
            }
            other => panic!(
                "expected a changed-file error, got {:?}",
                other.map(|c| c.len())
            ),
        }
    }

    #[test]
    fn an_unchanged_file_reads_both_passes() {
        let columns = changed_file("same", |_| {}).unwrap();
        assert_eq!(columns[1].dictionary, ["a", "b"]);
        assert_eq!(columns[1].codes.len(), columns[0].ints.len());
    }

    #[test]
    fn a_file_that_grows_between_the_passes_is_an_error() {
        assert_changed(changed_file("grows", |path| {
            std::fs::write(path, "n,s\n1,a\n2,b\n3,c\n").unwrap()
        }));
    }

    #[test]
    fn a_file_that_shrinks_between_the_passes_is_an_error() {
        assert_changed(changed_file("shrinks", |path| {
            std::fs::write(path, "n,s\n1,a\n").unwrap()
        }));
    }

    #[test]
    fn a_file_with_a_new_header_between_the_passes_is_an_error() {
        assert_changed(changed_file("header", |path| {
            std::fs::write(path, "n,t\n1,a\n2,b\n").unwrap()
        }));
    }

    /// A simulated budget: what is charged and not yet released, and each charge and release.
    #[derive(Default)]
    struct Pool {
        limit: usize,
        used: std::cell::Cell<usize>,
        charges: std::cell::RefCell<Vec<usize>>,
        releases: std::cell::RefCell<Vec<usize>>,
    }

    fn pooled(
        pool: &Pool,
        path: &Path,
        kinds: &BTreeMap<String, String>,
        between: impl FnOnce(),
    ) -> Result<Vec<Column>, ColumnsError> {
        let mut charge = |bytes: usize| {
            if pool.used.get().saturating_add(bytes) > pool.limit {
                return false;
            }
            pool.used.set(pool.used.get() + bytes);
            pool.charges.borrow_mut().push(bytes);
            true
        };
        let mut release = |bytes: usize| {
            pool.used.set(pool.used.get() - bytes);
            pool.releases.borrow_mut().push(bytes);
        };
        let cancelled = || false;
        let mut meter = Meter {
            charge: &mut charge,
            release: &mut release,
            cancelled: &cancelled,
        };
        read_csv_columns_between(path, kinds, 1 << 20, &mut meter, between)
    }

    fn temp_csv(name: &str, body: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "guion-csv-columns-pool-{}-{name}.csv",
            std::process::id()
        ));
        std::fs::write(&path, body).unwrap();
        path
    }

    fn text_rows(rows: usize) -> String {
        let mut body = String::from("n,s\n");
        for row in 0..rows {
            body.push_str(&format!("{row},word{row}\n"));
        }
        body
    }

    #[test]
    fn every_failing_read_releases_everything_it_charged() {
        let rows = text_rows(300);
        let mut ragged = rows.clone();
        ragged.push_str("1,2,3\n");
        type Case = (&'static str, String, BTreeMap<String, String>, usize, bool);
        let cases: Vec<Case> = vec![
            // The second pass finds a changed file after pass 1's columns are retained.
            ("changed", rows.clone(), BTreeMap::new(), 1 << 30, true),
            ("ragged", ragged, BTreeMap::new(), 1 << 30, false),
            (
                "value",
                rows.clone(),
                BTreeMap::from([("s".to_string(), "i64".to_string())]),
                1 << 30,
                false,
            ),
            // The budget runs out partway through.
            ("budget", rows.clone(), BTreeMap::new(), 66_000, false),
        ];
        for (name, body, kinds, limit, change) in cases {
            let path = temp_csv(name, &body);
            let pool = Pool {
                limit,
                ..Pool::default()
            };
            for _ in 0..1000 {
                let result = pooled(&pool, &path, &kinds, || {
                    if change {
                        std::fs::write(&path, "n,s\n1,a\n").unwrap();
                    }
                });
                assert!(result.is_err(), "{name} unexpectedly read");
                if change {
                    std::fs::write(&path, &body).unwrap();
                }
                assert_eq!(pool.used.get(), 0, "{name}: a failed read kept its charges");
            }
            let _ = std::fs::remove_file(&path);
        }
    }

    #[test]
    fn promoting_an_int_column_to_float_charges_its_transient_copy() {
        let mut body = String::from("x\n");
        for row in 0..1000 {
            body.push_str(&format!("{row}\n"));
        }
        body.push_str("1.5\n");
        let path = temp_csv("promote", &body);
        let pool = Pool {
            limit: 1 << 30,
            ..Pool::default()
        };
        let columns = pooled(&pool, &path, &BTreeMap::new(), || {}).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(columns[0].floats.len(), 1001);
        // The copy of 1,000 values (8,000 bytes) is charged first and released afterwards.
        assert!(pool.charges.borrow().contains(&8000), "{:?}", pool.charges);
        assert!(
            pool.releases.borrow().contains(&8000),
            "{:?}",
            pool.releases
        );
    }

    /// A dictionary's text is charged where it is built and nowhere else: what the budget
    /// holds after a read is the text plus a bounded cost per entry, not the text twice.
    #[test]
    fn dictionary_text_is_charged_once() {
        let distinct = 300;
        let width = 4000;
        let mut body = String::from("s\n");
        for row in 0..distinct {
            body.push_str(&format!("w{row:0>width$}\n", width = width - 1));
        }
        let path = temp_csv("dictionary-once", &body);
        let pool = Pool {
            limit: 1 << 30,
            ..Pool::default()
        };
        let columns = pooled(&pool, &path, &BTreeMap::new(), || {}).unwrap();
        let _ = std::fs::remove_file(&path);
        let text = distinct * width;
        let held = pool.used.get();
        assert!(held >= text, "the text must be charged: {held} < {text}");
        // Per entry: the list slot, the hash table, a code and a mask byte; and the fixed
        // buffers. Nothing near a second copy of the text.
        let per_entry = ENTRY_OVERHEAD + 8 * 4 + 16;
        assert!(
            held <= text + distinct * per_entry + 2 * BUFFER_BYTES,
            "the text is charged more than once: {held} for {text} bytes of text"
        );
        // The result's own charge adds names and records, never the dictionary text.
        assert_eq!(result_overhead(&columns), "s".len() + 256);
    }

    /// An Int column that turns out to hold a float is copied; a budget that holds the ints but
    /// not their copy refuses the copy and gives back everything the read charged.
    #[test]
    fn a_budget_that_cannot_hold_the_promotion_copy_refuses_it() {
        let mut body = String::from("x\n");
        for row in 0..1000 {
            body.push_str(&format!("{row}\n"));
        }
        body.push_str("1.5\n");
        let path = temp_csv("promote-refused", &body);
        // Room for the fixed buffers but not the 8,000-byte copy of the ints.
        let pool = Pool {
            limit: BUFFER_BYTES + 4000,
            ..Pool::default()
        };
        let result = pooled(&pool, &path, &BTreeMap::new(), || {});
        let _ = std::fs::remove_file(&path);
        assert!(
            matches!(result, Err(ColumnsError::Allocation(8000))),
            "expected the copy to be refused"
        );
        assert_eq!(pool.used.get(), 0, "a refused read keeps no charge");
    }

    #[test]
    fn the_dictionary_numbers_texts_in_first_seen_order_across_growth() {
        let mut dictionary = Dictionary::new();
        for round in 0..3 {
            for value in 0..1000 {
                assert_eq!(dictionary.code(&format!("v{value}")), value, "{round}");
            }
        }
        assert_eq!(dictionary.values.len(), 1000);
    }
}
