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
    io::{BufRead, Read},
    path::Path,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    },
};

/// Size of the read buffer.
const BUFFER_BYTES: usize = 64 * 1024;
/// Rows between checks of the allocation budget and the cancel flag.
const CHUNK_ROWS: usize = 4096;
/// Bytes of record text between those checks, for files with very long records.
const CHUNK_BYTES: usize = 1 << 20;
/// What a dictionary entry costs beyond its text: the `String` itself.
const ENTRY_OVERHEAD: usize = 24;
/// Files of at least this many bytes are read by several threads; smaller ones are read by
/// one, because starting threads and merging their columns costs more than it saves. Measured
/// on mixed-kind files from 1 to 36 MiB, the parallel read was faster at every size, so this
/// is the smallest size tried.
const PARALLEL_MIN_BYTES: usize = 1 << 20;
/// Bytes of file each thread parses in one round.
const PIECE_BYTES: usize = 4 << 20;
/// The most threads a read uses; more only shrinks each piece.
const MAX_THREADS: usize = 8;

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
    /// Seed of the hash, drawn once per dictionary from the process's random state, so the
    /// order of the table cannot be predicted from the text.
    seed: u64,
    text_bytes: usize,
}

const HASH_MULTIPLIER: u64 = 0x9E37_79B9_7F4A_7C15;

/// A multiply-fold hash of the bytes: eight bytes at a time, then mixed.
#[inline]
fn hash_bytes(seed: u64, bytes: &[u8]) -> u64 {
    let mut hash = seed ^ (bytes.len() as u64).wrapping_mul(HASH_MULTIPLIER);
    let mut chunks = bytes.chunks_exact(8);
    for chunk in &mut chunks {
        let word = u64::from_le_bytes(chunk.try_into().expect("a chunk of eight"));
        hash = (hash ^ word).wrapping_mul(HASH_MULTIPLIER);
        hash ^= hash >> 32;
    }
    let rest = chunks.remainder();
    if !rest.is_empty() {
        let mut tail = [0u8; 8];
        tail[..rest.len()].copy_from_slice(rest);
        hash = (hash ^ u64::from_le_bytes(tail)).wrapping_mul(HASH_MULTIPLIER);
    }
    hash ^= hash >> 29;
    hash = hash.wrapping_mul(HASH_MULTIPLIER);
    hash ^ (hash >> 32)
}

const EMPTY_SLOT: usize = usize::MAX;

impl Dictionary {
    fn new() -> Self {
        Self {
            values: Vec::new(),
            table: Vec::new(),
            seed: std::collections::hash_map::RandomState::new().hash_one(0u64),
            text_bytes: 0,
        }
    }

    fn code(&mut self, text: &str) -> usize {
        if self.table.is_empty() {
            self.table = vec![EMPTY_SLOT; 16];
        }
        let mask = self.table.len() - 1;
        let mut slot = (hash_bytes(self.seed, text.as_bytes()) as usize) & mask;
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
            let mut slot = (hash_bytes(self.seed, value.as_bytes()) as usize) & mask;
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
    /// An integer field read here was written `-0` (or `-00`...). It is `0` as an integer,
    /// but a float column would have read it as `-0.0`.
    negative_zero: bool,
    ints: Vec<i64>,
    floats: Vec<f64>,
    flags: Vec<bool>,
    codes: Vec<i64>,
    dictionary: Dictionary,
    valid: Vec<bool>,
}

/// Why a piece's column could not be appended to the columns before it.
enum Merge {
    /// The kinds do not combine without reading the file again.
    Reread,
    /// The budget refused this many bytes for a transient copy.
    Budget(usize),
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
            negative_zero: false,
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
                    self.negative_zero |= value == 0 && text.starts_with('-');
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
                } else if !self.quoted_empty {
                    // Text from its first value on. Every earlier field was an unquoted empty
                    // one, so each earlier row is missing and nothing needs to be read again.
                    self.codes.resize(rows, 0);
                    let code = self.dictionary.code(text);
                    self.codes.push(code as i64);
                    self.mode = Mode::Text;
                } else {
                    self.give_up_numbers();
                    return Ok(());
                }
            }
            Mode::Int => {
                if let Some(value) = parse_i64(text) {
                    self.negative_zero |= value == 0 && text.starts_with('-');
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

    /// Appends the rows `piece` read, which follow this column's rows in the file. The result is
    /// what reading both ranges in one go would have built, or `Err(Merge::Reread)` when that
    /// cannot be known without reading the file again (a kind change that makes the column
    /// text, a quoted empty), which sends the whole read down the sequential path.
    fn absorb(&mut self, piece: Builder, meter: &mut Meter<'_>) -> Result<(), Merge> {
        if piece.mode == Mode::Reread || piece.quoted_empty {
            return Err(Merge::Reread);
        }
        let rows = self.valid.len();
        if piece.mode == Mode::Unset {
            // Only unquoted empty fields: missing rows, of whatever kind the column is.
            let more = piece.valid.len();
            match self.mode {
                Mode::Int => self.ints.resize(rows + more, 0),
                Mode::Float => self.floats.resize(rows + more, 0.0),
                Mode::Bool => self.flags.resize(rows + more, false),
                Mode::Text => self.codes.resize(rows + more, 0),
                Mode::Unset => {}
                Mode::Reread => return Err(Merge::Reread),
            }
            self.valid.resize(rows + more, false);
            return Ok(());
        }
        // A `-0` read as an integer is `0.0` after a promotion, which is what a sequential
        // read gives only when the column was still integers. Once it is a float column a
        // sequential read would have parsed `-0` as `-0.0`.
        if self.mode == Mode::Float
            && piece.negative_zero
            && matches!(piece.mode, Mode::Int | Mode::Float)
        {
            return Err(Merge::Reread);
        }
        if self.mode == Mode::Unset {
            match piece.mode {
                Mode::Int => self.ints.resize(rows, 0),
                Mode::Float => self.floats.resize(rows, 0.0),
                Mode::Bool => self.flags.resize(rows, false),
                _ => self.codes.resize(rows, 0),
            }
            self.mode = piece.mode;
        }
        match (self.mode, piece.mode) {
            (Mode::Int, Mode::Int) => self.ints.extend_from_slice(&piece.ints),
            (Mode::Float, Mode::Float) => self.floats.extend_from_slice(&piece.floats),
            (Mode::Bool, Mode::Bool) => self.flags.extend_from_slice(&piece.flags),
            (Mode::Int, Mode::Float) => {
                // The same transient copy the sequential promotion charges.
                let copy = self.ints.len().saturating_mul(8);
                if !(meter.charge)(copy) {
                    return Err(Merge::Budget(copy));
                }
                self.floats = self.ints.iter().map(|&int| int as f64).collect();
                self.floats.extend_from_slice(&piece.floats);
                self.ints = Vec::new();
                (meter.release)(copy);
                self.mode = Mode::Float;
            }
            (Mode::Float, Mode::Int) => {
                self.floats.extend(piece.ints.iter().map(|&int| int as f64));
            }
            (Mode::Text, Mode::Text) => {
                if self.dictionary.values.is_empty() {
                    self.codes.extend_from_slice(&piece.codes);
                    self.dictionary = piece.dictionary;
                } else {
                    // The piece's texts in the order it first saw them, which is the order
                    // the file first shows them after the pieces before it.
                    let map: Vec<i64> = piece
                        .dictionary
                        .values
                        .iter()
                        .map(|text| self.dictionary.code(text) as i64)
                        .collect();
                    self.codes.extend(piece.codes.iter().zip(&piece.valid).map(
                        |(&code, &valid)| {
                            if valid {
                                map[code as usize]
                            } else {
                                0
                            }
                        },
                    ));
                }
            }
            _ => return Err(Merge::Reread),
        }
        self.negative_zero |= piece.negative_zero;
        self.valid.extend_from_slice(&piece.valid);
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
    if let Some(value) = fast_f64(text.as_bytes()) {
        return Some(value);
    }
    text.parse::<f64>().ok().filter(|value| value.is_finite())
}

/// The powers of ten that an `f64` holds exactly.
const POWERS_OF_TEN: [f64; 23] = [
    1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15, 1e16,
    1e17, 1e18, 1e19, 1e20, 1e21, 1e22,
];

/// The common decimal, read without the general parser: an optional sign, at most 15 digits
/// with at most one `.` among them, no exponent. Its digits make an integer below 10^15,
/// which an `f64` holds exactly, and it is divided by an exact power of ten; one division of
/// two exact values is correctly rounded, so the result is bit for bit what `str::parse`
/// gives. Anything else is `None` and goes to the general parser.
#[inline]
fn fast_f64(bytes: &[u8]) -> Option<f64> {
    let (negative, digits) = match bytes.first()? {
        b'-' => (true, &bytes[1..]),
        b'+' => (false, &bytes[1..]),
        _ => (false, bytes),
    };
    let mut mantissa = 0u64;
    let mut count = 0usize;
    let mut fraction = None;
    for &byte in digits {
        match byte {
            b'0'..=b'9' => {
                mantissa = mantissa * 10 + u64::from(byte - b'0');
                count += 1;
                if count > 15 {
                    return None;
                }
            }
            b'.' if fraction.is_none() => fraction = Some(count),
            _ => return None,
        }
    }
    if count == 0 {
        return None;
    }
    let places = fraction.map_or(0, |at| count - at);
    let value = mantissa as f64 / POWERS_OF_TEN[places];
    Some(if negative { -value } else { value })
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

/// The bytes as text, or `None` when they are not valid UTF-8. A record that is all ASCII (the
/// common case) skips the general validator, which costs more than the ASCII check on short records.
fn as_str(bytes: &[u8]) -> Option<&str> {
    if bytes.is_ascii() {
        // SAFETY: every byte is below 0x80, and ASCII bytes are valid UTF-8 on their own.
        Some(unsafe { std::str::from_utf8_unchecked(bytes) })
    } else {
        std::str::from_utf8(bytes).ok()
    }
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
                    if let Some(text) = as_str(text) {
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
        self.settle_live(live)
    }

    /// Charges the growth of `live`, the bytes held now, and checks for cancellation.
    fn settle_live(&mut self, live: usize) -> Result<(), ColumnsError> {
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

/// The name and fixed kind of each column.
type Specs = Vec<(String, Option<ColumnKind>)>;

/// The name and fixed kind of each column the header names, or the error for a `kinds` entry
/// that names an unknown kind or a column that is not there.
fn column_specs(names: &[&str], kinds: &BTreeMap<String, String>) -> Result<Specs, ColumnsError> {
    let mut specs = Vec::with_capacity(names.len());
    for name in names {
        let kind = match kinds.get(*name) {
            None => None,
            Some(kind) => Some(
                ColumnKind::from_name(kind).ok_or_else(|| ColumnsError::Value {
                    line: 1,
                    column: 0,
                    message: format!("unknown kind `{kind}`"),
                })?,
            ),
        };
        specs.push(((*name).to_owned(), kind));
    }
    let present: HashSet<&str> = names.iter().copied().collect();
    if let Some(missing) = kinds.keys().find(|name| !present.contains(name.as_str())) {
        return Err(ColumnsError::Value {
            line: 1,
            column: 0,
            message: format!("no column named `{missing}`"),
        });
    }
    Ok(specs)
}

/// Takes the fields of one data record into the builders.
fn ingest_record(
    builders: &mut [Builder],
    spans: &[Span],
    text: &str,
    line: usize,
    meter: &mut Meter<'_>,
) -> Result<(), ColumnsError> {
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
        match builder.ingest(field, span.quoted, meter) {
            Ok(()) => {}
            Err(Refused::DoesNotFit) => {
                return Err(bad_value(builder, field, line, index + 1));
            }
            Err(Refused::Budget(bytes)) => return Err(ColumnsError::Allocation(bytes)),
        }
    }
    Ok(())
}

/// Reads the CSV file at `path` into columns. `kinds` fixes the kind of columns by name.
/// Each record is held to `max_record_bytes`; the file as a whole is held to nothing.
pub fn read_csv_columns(
    path: &Path,
    kinds: &BTreeMap<String, String>,
    max_record_bytes: usize,
    meter: &mut Meter<'_>,
) -> Result<Vec<Column>, ColumnsError> {
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    read_csv_columns_with_threads(path, kinds, max_record_bytes, meter, threads)
}

/// `read_csv_columns` with at most `threads` threads (the result is the same for any number).
pub fn read_csv_columns_with_threads(
    path: &Path,
    kinds: &BTreeMap<String, String>,
    max_record_bytes: usize,
    meter: &mut Meter<'_>,
    threads: usize,
) -> Result<Vec<Column>, ColumnsError> {
    read_tracked(path, kinds, max_record_bytes, meter, threads, || {})
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
#[cfg(test)]
fn read_csv_columns_between(
    path: &Path,
    kinds: &BTreeMap<String, String>,
    max_record_bytes: usize,
    meter: &mut Meter<'_>,
    between_passes: impl FnOnce(),
) -> Result<Vec<Column>, ColumnsError> {
    read_tracked(path, kinds, max_record_bytes, meter, 1, between_passes)
}

fn read_tracked(
    path: &Path,
    kinds: &BTreeMap<String, String>,
    max_record_bytes: usize,
    meter: &mut Meter<'_>,
    threads: usize,
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
        let threads = threads.min(MAX_THREADS);
        let parallel = if threads > 1 {
            read_parallel(path, kinds, max_record_bytes, &mut tracked, threads)
        } else {
            Ok(None)
        };
        match parallel {
            Ok(Some(columns)) => {
                note_path("parallel");
                Ok(columns)
            }
            Err(error) => {
                note_path("parallel");
                Err(error)
            }
            Ok(None) => {
                note_path("sequential");
                read_passes(path, kinds, max_record_bytes, &mut tracked, between_passes)
            }
        }
    };
    if result.is_err() {
        (meter.release)(held.get());
    }
    result
}

#[cfg(test)]
thread_local! {
    static LAST_PATH: Cell<&'static str> = const { Cell::new("") };
    static NO_NEW_THREADS: Cell<bool> = const { Cell::new(false) };
}

/// Records which path a read took, for the tests.
fn note_path(_path: &'static str) {
    #[cfg(test)]
    LAST_PATH.with(|last| last.set(_path));
}

#[cfg(test)]
fn last_path() -> &'static str {
    LAST_PATH.with(|last| last.get())
}

/// Lets a test stand in for an operating system that refuses to start a thread.
#[cfg(test)]
fn refuse_new_threads(refuse: bool) {
    NO_NEW_THREADS.with(|flag| flag.set(refuse));
}

fn refuse_spawn() -> bool {
    #[cfg(test)]
    return NO_NEW_THREADS.with(|flag| flag.get());
    #[cfg(not(test))]
    false
}

/// Why a piece stopped before its end.
enum PieceStop {
    /// The piece holds something only the sequential reader handles.
    Sequential,
    /// A record failed; its line counts from the start of the piece.
    Error(ColumnsError),
}

/// What one piece of the file produced.
struct Piece {
    builders: Vec<Builder>,
    /// Physical lines the piece covered, blank ones included.
    lines: usize,
    /// The largest transient copy a promotion inside the piece would have charged.
    transient: usize,
    stop: Option<PieceStop>,
}

/// Parses `data`, whole records from the start of one. It takes only what the sequential
/// reader's fast path takes: records that are valid UTF-8, hold no quote and no carriage
/// return but one before the newline, and are under `max_record_bytes`. Anything else, or a
/// last record without a newline that is not plain, stops it with `Sequential`. `last` says
/// the piece may end in a record without a newline.
fn parse_piece(
    data: &[u8],
    last: bool,
    specs: &[(String, Option<ColumnKind>)],
    blank_is_value: bool,
    max_record_bytes: usize,
) -> Piece {
    let mut builders: Vec<Builder> = specs
        .iter()
        .map(|(name, kind)| Builder::new(name.clone(), *kind))
        .collect();
    let transient = Cell::new(0usize);
    let mut charge = |bytes: usize| {
        transient.set(transient.get().max(bytes));
        true
    };
    let mut release = |_: usize| {};
    let cancelled = || false;
    let mut meter = Meter {
        charge: &mut charge,
        release: &mut release,
        cancelled: &cancelled,
    };
    let mut spans: Vec<Span> = Vec::new();
    let mut position = 0usize;
    let mut line = 0usize;
    let stop = loop {
        if position >= data.len() {
            break None;
        }
        let rest = &data[position..];
        let (used, record) = match memchr::memchr(b'\n', rest) {
            Some(newline) => {
                let record = &rest[..newline];
                (newline + 1, record.strip_suffix(b"\r").unwrap_or(record))
            }
            None if last => (rest.len(), rest),
            None => break Some(PieceStop::Sequential),
        };
        // The sequential fast path takes a record when its bytes with the newline are within
        // the limit; one byte of margin keeps the two readers' limits from disagreeing.
        if used >= max_record_bytes || memchr::memchr2(b'"', b'\r', record).is_some() {
            break Some(PieceStop::Sequential);
        }
        let Some(text) = as_str(record) else {
            break Some(PieceStop::Sequential);
        };
        if !text.is_empty() || blank_is_value {
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
            if let Err(error) = ingest_record(&mut builders, &spans, text, line, &mut meter) {
                break Some(PieceStop::Error(error));
            }
        }
        position += used;
        line += 1;
    };
    Piece {
        builders,
        lines: line,
        transient: transient.get(),
        stop,
    }
}

/// Adds the lines before a piece to the line of the error it reported.
fn offset_error(error: ColumnsError, before: usize) -> ColumnsError {
    match error {
        ColumnsError::Malformed {
            line,
            column,
            message,
        } => ColumnsError::Malformed {
            line: line + before,
            column,
            message,
        },
        ColumnsError::Value {
            line,
            column,
            message,
        } => ColumnsError::Value {
            line: line + before,
            column,
            message,
        },
        other => other,
    }
}

/// Reads a large file with `threads` threads, or returns `Ok(None)` after giving back
/// everything it charged when the sequential reader has to read it: the file is small, or
/// holds a quote, a record over the limit, a byte that is not UTF-8, or columns whose kinds
/// cannot be joined without reading the file again.
///
/// Rounds: one block of `threads` pieces of `PIECE_BYTES` is read, cut at its last newline (the
/// rest starts the next block), split at newlines into `threads` pieces, and the pieces are
/// parsed on scoped threads, each into builders of its own. The coordinating thread then
/// joins the pieces onto the columns so far in file order. It alone charges the budget and
/// checks for cancellation, once per round.
fn read_parallel(
    path: &Path,
    kinds: &BTreeMap<String, String>,
    max_record_bytes: usize,
    meter: &mut Meter<'_>,
    threads: usize,
) -> Result<Option<Vec<Column>>, ColumnsError> {
    let Ok(mut file) = File::open(path) else {
        return Ok(None);
    };
    let size = file.metadata().map_or(0, |meta| meta.len());
    if size < PARALLEL_MIN_BYTES as u64 {
        return Ok(None);
    }
    let block = threads * PIECE_BYTES;
    let mut settler = Settler {
        meter,
        charged: 0,
        rows: 0,
        bytes: 0,
    };
    let mut buffer: Vec<u8> = Vec::new();
    let mut header: Option<(Specs, bool)> = None;
    let mut globals: Vec<Builder> = Vec::new();
    // Physical lines before the first byte of the current piece.
    let mut lines_before = 0usize;
    let mut start = 0usize;
    let mut first = true;
    // Gives back the charge and sends the read to the sequential path.
    macro_rules! sequential {
        ($settler:expr) => {{
            (settler.meter.release)($settler.charged);
            return Ok(None);
        }};
    }
    loop {
        // Charge the block before reading it, as the buffer grows to hold it.
        settler.settle_live(
            buffer
                .capacity()
                .max(buffer.len() + block)
                .saturating_add(globals.iter().map(Builder::accounted_bytes).sum::<usize>()),
        )?;
        let wanted = block;
        let got = match (&mut file).take(wanted as u64).read_to_end(&mut buffer) {
            Ok(got) => got,
            Err(_) => sequential!(settler),
        };
        let eof = got < wanted;
        if first {
            first = false;
            if buffer.starts_with(&[0xEF, 0xBB, 0xBF]) {
                start = 3;
            }
            // The header: the first line that is not blank. It must be plain.
            let mut at = start;
            lines_before = 1;
            let names = loop {
                let Some(newline) = memchr::memchr(b'\n', &buffer[at..]) else {
                    sequential!(settler);
                };
                let line = &buffer[at..at + newline];
                let line = line.strip_suffix(b"\r").unwrap_or(line);
                at += newline + 1;
                if line.is_empty() {
                    lines_before += 1;
                    continue;
                }
                if newline + 1 >= max_record_bytes || memchr::memchr2(b'"', b'\r', line).is_some() {
                    sequential!(settler);
                }
                let Some(text) = as_str(line) else {
                    sequential!(settler);
                };
                break text.split(',').collect::<Vec<&str>>();
            };
            let specs = column_specs(&names, kinds)?;
            let one_column = specs.len() == 1;
            globals = specs
                .iter()
                .map(|(name, kind)| Builder::new(name.clone(), *kind))
                .collect();
            header = Some((specs, one_column));
            lines_before += 1;
            start = at;
        }
        let (specs, one_column) = header.as_ref().expect("the header was read");
        // Cut the block after its last newline; the rest begins the next block.
        let end = if eof {
            buffer.len()
        } else {
            match memchr::memrchr(b'\n', &buffer[start..]) {
                Some(last) => start + last + 1,
                None => sequential!(settler),
            }
        };
        if end > start {
            let data = &buffer[start..end];
            if memchr::memchr(b'"', data).is_some() {
                sequential!(settler);
            }
            // Pieces of about equal size, each cut after a newline.
            let mut cuts = vec![0usize];
            for index in 1..threads {
                let target = (data.len() / threads * index).max(*cuts.last().expect("a cut"));
                let cut = match memchr::memchr(b'\n', &data[target..]) {
                    Some(newline) => target + newline + 1,
                    None => data.len(),
                };
                cuts.push(cut);
            }
            cuts.push(data.len());
            let count = cuts.len() - 1;
            let results: Vec<Mutex<Option<Piece>>> = (0..count).map(|_| Mutex::new(None)).collect();
            let next = AtomicUsize::new(0);
            let work = || loop {
                let index = next.fetch_add(1, Ordering::Relaxed);
                if index >= count {
                    return;
                }
                let piece = parse_piece(
                    &data[cuts[index]..cuts[index + 1]],
                    eof && index + 1 == count,
                    specs,
                    *one_column,
                    max_record_bytes,
                );
                *results[index].lock().unwrap_or_else(|e| e.into_inner()) = Some(piece);
            };
            // The workers pull pieces from one counter, so the calling thread works too and a
            // thread the operating system refuses to start only means fewer workers.
            std::thread::scope(|scope| {
                for _ in 1..count {
                    if refuse_spawn()
                        || std::thread::Builder::new()
                            .spawn_scoped(scope, work)
                            .is_err()
                    {
                        break;
                    }
                }
                work();
            });
            let pieces: Vec<Piece> = results
                .into_iter()
                .map(|slot| {
                    slot.into_inner()
                        .unwrap_or_else(|e| e.into_inner())
                        .expect("every piece was parsed")
                })
                .collect();
            if pieces
                .iter()
                .any(|piece| matches!(piece.stop, Some(PieceStop::Sequential)))
            {
                sequential!(settler);
            }
            let mut before = lines_before;
            let mut failure = None;
            for piece in &pieces {
                if let Some(PieceStop::Error(error)) = &piece.stop {
                    failure = Some(offset_error(error.clone(), before));
                    break;
                }
                before += piece.lines;
            }
            if let Some(error) = failure {
                return Err(error);
            }
            lines_before = before;
            let live = pieces
                .iter()
                .flat_map(|piece| &piece.builders)
                .chain(&globals)
                .map(Builder::accounted_bytes)
                .fold(buffer.capacity(), usize::saturating_add);
            settler.settle_live(live)?;
            for piece in &pieces {
                if piece.transient > 0 {
                    if !(settler.meter.charge)(piece.transient) {
                        return Err(ColumnsError::Allocation(piece.transient));
                    }
                    (settler.meter.release)(piece.transient);
                }
            }
            for piece in pieces {
                for (global, part) in globals.iter_mut().zip(piece.builders) {
                    match global.absorb(part, settler.meter) {
                        Ok(()) => {}
                        Err(Merge::Reread) => sequential!(settler),
                        Err(Merge::Budget(bytes)) => return Err(ColumnsError::Allocation(bytes)),
                    }
                }
            }
        }
        if eof {
            break;
        }
        buffer.drain(..end);
        start = 0;
    }
    if header.is_none() {
        sequential!(settler);
    }
    let live = globals
        .iter()
        .map(Builder::accounted_bytes)
        .fold(0usize, usize::saturating_add);
    settler.settle_live(live)?;
    if globals.iter().any(Builder::needs_text_pass) {
        sequential!(settler);
    }
    Ok(Some(globals.into_iter().map(Builder::finish).collect()))
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
            for (name, kind) in column_specs(&names, kinds)? {
                builders.push(Builder::new(name, kind));
            }
            return Ok(());
        }
        ingest_record(&mut builders, spans, text, line, settler.meter)?;
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
        // `s` starts as a number and turns out to be text, so it is read again in a second pass.
        std::fs::write(&path, "n,s\n1,7\n2,b\n").unwrap();
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
    fn a_column_that_is_text_from_its_first_value_needs_no_second_pass() {
        let path = std::env::temp_dir().join(format!(
            "guion-csv-columns-{}-one-pass.csv",
            std::process::id()
        ));
        std::fs::write(&path, "n,s\n1,\n2,x\n3,y\n4,x\n").unwrap();
        let mut passes = 0;
        let result = read_with_hook(&path, || passes += 1);
        let _ = std::fs::remove_file(&path);
        let columns = result.unwrap();
        assert_eq!(passes, 0, "the hook runs only when a second pass is needed");
        assert_eq!(columns[1].kind, ColumnKind::Text);
        assert_eq!(columns[1].dictionary, ["x", "y"]);
        assert_eq!(columns[1].codes, [0, 0, 1, 0]);
        assert_eq!(columns[1].valid, [false, true, true, true]);
    }

    #[test]
    fn a_column_with_a_quoted_empty_before_its_first_text_is_still_read_again() {
        let path = std::env::temp_dir().join(format!(
            "guion-csv-columns-{}-quoted-empty.csv",
            std::process::id()
        ));
        std::fs::write(&path, "s\n\"\"\nx\n").unwrap();
        let columns = read_with_hook(&path, || {}).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(columns[0].dictionary, ["", "x"]);
        assert_eq!(columns[0].valid, [true, true]);
    }

    #[test]
    fn an_unchanged_file_reads_both_passes() {
        let columns = changed_file("same", |_| {}).unwrap();
        assert_eq!(columns[1].dictionary, ["7", "b"]);
        assert_eq!(columns[1].codes.len(), columns[0].ints.len());
    }

    #[test]
    fn a_file_that_grows_between_the_passes_is_an_error() {
        assert_changed(changed_file("grows", |path| {
            std::fs::write(path, "n,s\n1,7\n2,b\n3,c\n").unwrap()
        }));
    }

    #[test]
    fn a_file_that_shrinks_between_the_passes_is_an_error() {
        assert_changed(changed_file("shrinks", |path| {
            std::fs::write(path, "n,s\n1,7\n").unwrap()
        }));
    }

    #[test]
    fn a_file_with_a_new_header_between_the_passes_is_an_error() {
        assert_changed(changed_file("header", |path| {
            std::fs::write(path, "n,t\n1,7\n2,b\n").unwrap()
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
        // A number first, so the column is read again as text in a second pass.
        let mut body = String::from("n,s\n-1,7\n");
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

    // ---- the parallel read ----

    /// Reads with a plain, unlimited meter and `threads` threads.
    fn read_threads(
        path: &Path,
        kinds: &BTreeMap<String, String>,
        threads: usize,
    ) -> Result<Vec<Column>, ColumnsError> {
        let mut charge = |_: usize| true;
        let mut release = |_: usize| {};
        let cancelled = || false;
        let mut meter = Meter {
            charge: &mut charge,
            release: &mut release,
            cancelled: &cancelled,
        };
        read_csv_columns_with_threads(path, kinds, 1 << 20, &mut meter, threads)
    }

    /// Reads `body` with one thread and with 2, 3 and 8, requires the same result each time,
    /// and says which path the threaded reads took.
    fn same_for_every_thread_count(
        name: &str,
        body: &str,
        kinds: &BTreeMap<String, String>,
        expect_parallel: bool,
    ) -> Result<Vec<Column>, ColumnsError> {
        assert!(body.len() >= PARALLEL_MIN_BYTES, "{name}: file too small");
        let path = temp_csv(name, body);
        let one = read_threads(&path, kinds, 1);
        assert_eq!(last_path(), "sequential");
        for threads in [2, 3, 8] {
            let many = read_threads(&path, kinds, threads);
            assert_eq!(
                last_path(),
                if expect_parallel {
                    "parallel"
                } else {
                    "sequential"
                },
                "{name}: {threads} threads took the wrong path"
            );
            assert_eq!(one, many, "{name}: {threads} threads differ from one");
        }
        let _ = std::fs::remove_file(&path);
        one
    }

    fn mixed_rows(rows: usize) -> String {
        let mut body = String::from("id,price,flag,name,note\n");
        for row in 0..rows {
            let price = if row % 17 == 3 {
                String::new()
            } else {
                format!("{}.{:02}", row % 997, row % 100)
            };
            let flag = if row % 5 == 0 { "true" } else { "false" };
            let name = if row % 11 == 4 {
                String::new()
            } else {
                format!("name{}", row % 313)
            };
            let note = if row % 7 == 0 {
                String::new()
            } else {
                format!("n{}", (row * 31) % 1009)
            };
            body.push_str(&format!("{row},{price},{flag},{name},{note}\n"));
        }
        body
    }

    #[test]
    fn parallel_read_equals_sequential_on_mixed_kinds() {
        let body = mixed_rows(300_000);
        let columns = same_for_every_thread_count("mixed", &body, &BTreeMap::new(), true).unwrap();
        assert_eq!(columns[0].kind, ColumnKind::I64);
        assert_eq!(columns[1].kind, ColumnKind::F64);
        assert_eq!(columns[2].kind, ColumnKind::Bool);
        assert_eq!(columns[3].kind, ColumnKind::Text);
        assert!(columns[1].valid.contains(&false) && columns[3].valid.contains(&false));
        assert_eq!(columns[0].ints.len(), 300_000);
    }

    #[test]
    fn parallel_read_int_then_float_late_in_the_file() {
        let rows = 600_000;
        let mut body = String::from("x,y\n");
        for row in 0..rows {
            if row >= rows - rows / 10 && row % 1000 == 0 {
                body.push_str(&format!("{row}.5,{row}\n"));
            } else {
                body.push_str(&format!("{row},{row}\n"));
            }
        }
        let columns =
            same_for_every_thread_count("int-float", &body, &BTreeMap::new(), true).unwrap();
        assert_eq!(columns[0].kind, ColumnKind::F64);
        assert_eq!(columns[1].kind, ColumnKind::I64);
        assert_eq!(columns[0].floats[5], 5.0);
    }

    #[test]
    fn parallel_read_numeric_looking_text() {
        let rows = 1_500_000;
        let mut body = String::from("code\n");
        for _ in 0..rows {
            body.push_str("007\n");
        }
        body.push_str("abc\n");
        let columns =
            same_for_every_thread_count("numeric-text", &body, &BTreeMap::new(), false).unwrap();
        assert_eq!(columns[0].kind, ColumnKind::Text);
        assert_eq!(columns[0].dictionary, ["007", "abc"]);
    }

    #[test]
    fn parallel_read_quoted_newline_falls_back() {
        let mut body = mixed_rows(150_000);
        body.push_str("999999,1.5,true,\"two\nlines\",x\n");
        body.push_str(&mixed_rows(10)["id,price,flag,name,note\n".len()..]);
        let columns =
            same_for_every_thread_count("quoted-newline", &body, &BTreeMap::new(), false).unwrap();
        assert!(columns[3].dictionary.contains(&"two\nlines".to_string()));
    }

    #[test]
    fn parallel_read_error_names_the_same_line_and_column() {
        // CRLF endings and blank lines, so the physical line differs from the record count.
        let rows = 500_000;
        let build = |bad: Option<(usize, &str)>| {
            let mut body = String::from("a,b,c\r\n");
            for row in 0..rows {
                if row % 1000 == 999 {
                    body.push_str("\r\n");
                }
                match bad {
                    Some((at, text)) if at == row => body.push_str(text),
                    _ => body.push_str(&format!("{row},{},w{}\r\n", row % 50, row % 9)),
                }
            }
            body
        };
        let late = rows - rows / 20;
        let fields = build(Some((late, "1,2\r\n")));
        let error = same_for_every_thread_count("error-fields", &fields, &BTreeMap::new(), true)
            .unwrap_err();
        assert!(matches!(error, ColumnsError::Malformed { line, .. } if line > late));
        let kinds = BTreeMap::from([("b".to_string(), "i64".to_string())]);
        let value = build(Some((late, "1,x,w\r\n")));
        let error = same_for_every_thread_count("error-value", &value, &kinds, true).unwrap_err();
        assert!(
            matches!(error, ColumnsError::Value { column: 2, .. }),
            "{error:?}"
        );
        // Two bad records: the first one, in file order, is the one reported.
        let mut two = build(Some((late, "1,x,w\r\n")));
        two.push_str("1,y,w\r\n");
        let error = same_for_every_thread_count("error-two", &two, &kinds, true).unwrap_err();
        assert!(matches!(error, ColumnsError::Value { .. }));
    }

    #[test]
    fn parallel_read_dictionary_codes_are_first_seen_order() {
        let mut body = String::from("s\n");
        let mut next = 0;
        let rows = 1_200_000;
        for row in 0..rows {
            // Every value after the first thousand or so is new only occasionally, so new
            // values first appear in different pieces.
            if row % 97 == 0 {
                body.push_str(&format!("v{next}\n"));
                next += 1;
            } else {
                body.push_str(&format!("v{}\n", (row * 7) % (next.max(1))));
            }
        }
        let columns =
            same_for_every_thread_count("dictionary", &body, &BTreeMap::new(), true).unwrap();
        for (code, value) in columns[0].dictionary.iter().enumerate() {
            assert_eq!(value, &format!("v{code}"), "first-seen order");
        }
    }

    #[test]
    fn parallel_read_keeps_a_negative_zero_a_float_column_would_read() {
        let mut body = String::from("x\n1.5\n");
        for row in 0..400_000 {
            body.push_str(if row % 1000 == 0 { "-0\n" } else { "2\n" });
        }
        let path = temp_csv("negative-zero", &body);
        let one = read_threads(&path, &BTreeMap::new(), 1).unwrap();
        for threads in [2, 3, 8] {
            let many = read_threads(&path, &BTreeMap::new(), threads).unwrap();
            let bits = |columns: &Vec<Column>| {
                columns[0]
                    .floats
                    .iter()
                    .map(|f| f.to_bits())
                    .collect::<Vec<_>>()
            };
            assert_eq!(bits(&one), bits(&many), "{threads} threads");
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn parallel_read_survives_a_refused_thread() {
        let body = mixed_rows(150_000);
        let path = temp_csv("refused-thread", &body);
        let one = read_threads(&path, &BTreeMap::new(), 1);
        refuse_new_threads(true);
        let many = read_threads(&path, &BTreeMap::new(), 8);
        refuse_new_threads(false);
        let _ = std::fs::remove_file(&path);
        assert_eq!(last_path(), "parallel");
        assert_eq!(one, many);
    }

    fn pooled_threads(
        pool: &Pool,
        path: &Path,
        threads: usize,
    ) -> Result<Vec<Column>, ColumnsError> {
        let mut charge = |bytes: usize| {
            if pool.used.get().saturating_add(bytes) > pool.limit {
                return false;
            }
            pool.used.set(pool.used.get() + bytes);
            true
        };
        let mut release = |bytes: usize| pool.used.set(pool.used.get() - bytes);
        let cancelled = || false;
        let mut meter = Meter {
            charge: &mut charge,
            release: &mut release,
            cancelled: &cancelled,
        };
        read_csv_columns_with_threads(path, &BTreeMap::new(), 1 << 20, &mut meter, threads)
    }

    #[test]
    fn parallel_read_charges_and_releases_like_sequential() {
        let body = mixed_rows(150_000);
        let path = temp_csv("pool-parallel", &body);
        // A refused budget: both paths refuse, and the meter is back to zero.
        for limit in [1usize << 10, 1 << 20, 3 << 20] {
            for threads in [1, 2, 8] {
                let pool = Pool {
                    limit,
                    ..Pool::default()
                };
                let result = pooled_threads(&pool, &path, threads);
                assert!(
                    matches!(result, Err(ColumnsError::Allocation(_))),
                    "limit {limit}, {threads} threads: {:?}",
                    result.map(|c| c.len())
                );
                assert_eq!(pool.used.get(), 0, "limit {limit}, {threads} threads");
            }
        }
        // A read that fits keeps a charge for what it returns, and gives the rest back.
        let pool = Pool {
            limit: 1 << 30,
            ..Pool::default()
        };
        let columns = pooled_threads(&pool, &path, 4).unwrap();
        assert_eq!(last_path(), "parallel");
        assert!(pool.used.get() > columns[0].ints.len() * 8);
        let _ = std::fs::remove_file(&path);
        // A read that falls back (a quote in the file) ends holding what a sequential one does.
        let mut quoted = mixed_rows(150_000);
        quoted.push_str("1,1.5,true,\"q\",x\n");
        let path = temp_csv("pool-fallback", &quoted);
        let sequential = Pool {
            limit: 1 << 30,
            ..Pool::default()
        };
        pooled_threads(&sequential, &path, 1).unwrap();
        let fallback = Pool {
            limit: 1 << 30,
            ..Pool::default()
        };
        pooled_threads(&fallback, &path, 8).unwrap();
        assert_eq!(last_path(), "sequential");
        assert_eq!(fallback.used.get(), sequential.used.get());
        // And a fallback that then fails leaves nothing held.
        quoted.push_str("1,2\n");
        std::fs::write(&path, &quoted).unwrap();
        let failing = Pool {
            limit: 1 << 30,
            ..Pool::default()
        };
        assert!(pooled_threads(&failing, &path, 8).is_err());
        assert_eq!(failing.used.get(), 0);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn float_fast_path_matches_parse() {
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let mut fast = 0;
        for _ in 0..100_000 {
            let digits = 1 + (next() % 19) as usize;
            let mut text = String::new();
            match next() % 4 {
                0 => text.push('-'),
                1 => text.push('+'),
                _ => {}
            }
            let dot = if next() % 5 == 0 {
                None
            } else {
                Some((next() % (digits as u64 + 1)) as usize)
            };
            for index in 0..digits {
                if dot == Some(index) {
                    text.push('.');
                }
                text.push((b'0' + (next() % 10) as u8) as char);
            }
            if dot == Some(digits) {
                text.push('.');
            }
            let expected = text.parse::<f64>().ok().filter(|v| v.is_finite());
            if let Some(value) = fast_f64(text.as_bytes()) {
                fast += 1;
                assert_eq!(Some(value.to_bits()), expected.map(f64::to_bits), "{text}");
            }
            assert_eq!(
                parse_f64(&text).map(f64::to_bits),
                expected.map(f64::to_bits),
                "{text}"
            );
        }
        assert!(fast > 50_000, "the fast path took only {fast} of 100000");
        for text in [
            "-0",
            "-0.0",
            "0.1",
            ".5",
            "5.",
            "-.5",
            "+.5",
            "1e5",
            "1.5e3",
            ".",
            "-",
            "+",
            "1.2.3",
            "0.30000000000000004",
        ] {
            let expected = text.parse::<f64>().ok().filter(|v| v.is_finite());
            assert_eq!(
                parse_f64(text).map(f64::to_bits),
                expected.map(f64::to_bits),
                "{text}"
            );
        }
    }
}
