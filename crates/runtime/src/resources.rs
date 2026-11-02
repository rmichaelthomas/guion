//! Bounded, owner-checked streaming file readers shared by the standard library.
use std::{
    collections::{HashMap, VecDeque},
    fs::File,
    io::{self, BufRead, BufReader},
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);
const CLOSED_HANDLE_HISTORY: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReaderMode {
    Lines,
    Csv,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReaderErrorKind {
    Closed,
    Stale,
    ForeignOwner,
    Mode,
    InvalidUtf8,
    Limit,
    Io,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReaderError {
    pub kind: ReaderErrorKind,
    pub message: String,
    pub line: usize,
    pub column: usize,
}

impl ReaderError {
    fn new(kind: ReaderErrorKind, message: impl Into<String>, line: usize, column: usize) -> Self {
        Self {
            kind,
            message: message.into(),
            line,
            column,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FileReaderHandle {
    owner: u64,
    id: u64,
}

impl FileReaderHandle {
    pub fn from_parts(owner: u64, id: u64) -> Self {
        Self { owner, id }
    }
    pub fn parts(self) -> (u64, u64) {
        (self.owner, self.id)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceLine {
    pub number: usize,
    pub text: Arc<str>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CsvRow {
    pub start_line: usize,
    pub fields: Vec<Arc<str>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CsvError {
    Reader(ReaderError),
    Mode,
    Malformed {
        line: usize,
        column: usize,
        message: String,
    },
}

impl CsvError {
    pub fn line(&self) -> usize {
        match self {
            Self::Reader(error) => error.line,
            Self::Mode => 0,
            Self::Malformed { line, .. } => *line,
        }
    }
}

#[derive(Debug)]
struct ReaderResource {
    mode: ReaderMode,
    reader: BufReader<File>,
    bytes_read: usize,
    max_file_bytes: usize,
    max_record_bytes: usize,
    physical_line: usize,
    eof: bool,
    failed: bool,
    /// Holds the record being read, kept between reads so each one needs no new buffer.
    record: Vec<u8>,
}

/// Owns native readers. Handles are scoped to this registry and become stale when it closes.
#[derive(Debug)]
pub struct FileReaderRegistry {
    owner: u64,
    next_id: u64,
    resources: HashMap<u64, ReaderResource>,
    closed: VecDeque<u64>,
    max_file_bytes: usize,
    max_record_bytes: usize,
    buffer_bytes: usize,
}

impl FileReaderRegistry {
    pub fn new(max_file_bytes: usize, max_record_bytes: usize) -> Self {
        Self::with_buffer(max_file_bytes, max_record_bytes, 8192)
    }

    pub fn with_buffer(
        max_file_bytes: usize,
        max_record_bytes: usize,
        buffer_bytes: usize,
    ) -> Self {
        Self {
            owner: NEXT_OWNER.fetch_add(1, Ordering::Relaxed),
            next_id: 1,
            resources: HashMap::new(),
            closed: VecDeque::new(),
            max_file_bytes,
            max_record_bytes,
            buffer_bytes: buffer_bytes.max(1),
        }
    }

    fn open(&mut self, path: &Path, mode: ReaderMode) -> Result<FileReaderHandle, ReaderError> {
        let file = File::open(path).map_err(|error| {
            ReaderError::new(
                ReaderErrorKind::Io,
                format!("cannot open file: {error}"),
                0,
                0,
            )
        })?;
        let id = self.next_id;
        self.next_id = self.next_id.checked_add(1).ok_or_else(|| {
            ReaderError::new(
                ReaderErrorKind::Limit,
                "reader handle space exhausted",
                0,
                0,
            )
        })?;
        self.resources.insert(
            id,
            ReaderResource {
                mode,
                reader: BufReader::with_capacity(self.buffer_bytes, file),
                bytes_read: 0,
                max_file_bytes: self.max_file_bytes,
                max_record_bytes: self.max_record_bytes,
                physical_line: 1,
                eof: false,
                failed: false,
                record: Vec::new(),
            },
        );
        Ok(FileReaderHandle {
            owner: self.owner,
            id,
        })
    }

    pub fn open_lines(&mut self, path: impl AsRef<Path>) -> Result<FileReaderHandle, ReaderError> {
        self.open(path.as_ref(), ReaderMode::Lines)
    }

    pub fn open_csv(&mut self, path: impl AsRef<Path>) -> Result<FileReaderHandle, ReaderError> {
        self.open(path.as_ref(), ReaderMode::Csv)
    }

    fn get_mut(
        &mut self,
        handle: FileReaderHandle,
        mode: ReaderMode,
    ) -> Result<&mut ReaderResource, ReaderError> {
        if handle.owner != self.owner {
            return Err(ReaderError::new(
                ReaderErrorKind::ForeignOwner,
                "reader belongs to another runtime instance",
                0,
                0,
            ));
        }
        if let Some(resource) = self.resources.get_mut(&handle.id) {
            if resource.mode != mode {
                return Err(ReaderError::new(
                    ReaderErrorKind::Mode,
                    "reader operation does not match its open mode",
                    resource.physical_line,
                    0,
                ));
            }
            if resource.failed {
                return Err(ReaderError::new(
                    ReaderErrorKind::Stale,
                    "reader is unusable after a prior read error",
                    resource.physical_line,
                    0,
                ));
            }
            return Ok(resource);
        }
        let kind = if self.closed.contains(&handle.id) {
            ReaderErrorKind::Closed
        } else {
            ReaderErrorKind::Stale
        };
        Err(ReaderError::new(kind, "reader is closed or stale", 0, 0))
    }

    pub fn next_line(
        &mut self,
        handle: FileReaderHandle,
    ) -> Result<Option<SourceLine>, ReaderError> {
        let resource = self.get_mut(handle, ReaderMode::Lines)?;
        if resource.eof {
            return Ok(None);
        }
        let start_line = resource.physical_line;
        let mut bytes = std::mem::take(&mut resource.record);
        bytes.clear();
        read_bounded_record(resource, b'\n', &mut bytes)?;
        if bytes.is_empty() && resource.eof {
            return Ok(None);
        }
        resource.physical_line = resource
            .physical_line
            .saturating_add(bytes.iter().filter(|byte| **byte == b'\n').count());
        let mut content = bytes.as_slice();
        if let Some(rest) = content.strip_suffix(b"\n") {
            content = rest;
        }
        if let Some(rest) = content.strip_suffix(b"\r") {
            content = rest;
        }
        let text = std::str::from_utf8(content).map_err(|error| {
            resource.failed = true;
            ReaderError::new(
                ReaderErrorKind::InvalidUtf8,
                "line is not valid UTF-8",
                start_line,
                error.valid_up_to() + 1,
            )
        })?;
        let text = Arc::<str>::from(text);
        resource.record = bytes;
        Ok(Some(SourceLine {
            number: start_line,
            text,
        }))
    }

    pub fn next_csv(&mut self, handle: FileReaderHandle) -> Result<Option<CsvRow>, CsvError> {
        let resource = self.get_mut(handle, ReaderMode::Csv).map_err(|error| {
            if error.kind == ReaderErrorKind::Mode {
                CsvError::Mode
            } else {
                CsvError::Reader(error)
            }
        })?;
        if resource.eof {
            return Ok(None);
        }
        let start_line = resource.physical_line;
        if let Some(fields) = read_plain_csv_record(resource) {
            return Ok(Some(CsvRow { start_line, fields }));
        }
        let record = read_csv_record(resource).map_err(CsvError::Reader)?;
        let Some(record) = record else {
            return Ok(None);
        };
        let mut record = record;
        if record.last() == Some(&b'\n') {
            record.pop();
            if record.last() == Some(&b'\r') {
                record.pop();
            }
        }
        let parsed = parse_csv_record(&record, start_line).map_err(|error| {
            resource.failed = true;
            CsvError::Malformed {
                line: error.0,
                column: error.1,
                message: error.2,
            }
        })?;
        Ok(Some(CsvRow {
            start_line,
            fields: parsed.into_iter().map(Arc::from).collect(),
        }))
    }

    pub fn close(&mut self, handle: FileReaderHandle) -> Result<(), ReaderError> {
        if handle.owner != self.owner {
            return Err(ReaderError::new(
                ReaderErrorKind::ForeignOwner,
                "reader belongs to another runtime instance",
                0,
                0,
            ));
        }
        if let Some(resource) = self.resources.remove(&handle.id) {
            let _ = resource.mode;
            self.mark_closed(handle.id);
            return Ok(());
        }
        let kind = if self.closed.contains(&handle.id) {
            ReaderErrorKind::Closed
        } else {
            ReaderErrorKind::Stale
        };
        Err(ReaderError::new(kind, "reader is closed or stale", 0, 0))
    }

    pub fn close_all(&mut self) {
        let handles = self
            .resources
            .drain()
            .map(|(id, _resource)| id)
            .collect::<Vec<_>>();
        for id in handles {
            self.mark_closed(id);
        }
    }

    fn mark_closed(&mut self, id: u64) {
        self.closed.push_back(id);
        while self.closed.len() > CLOSED_HANDLE_HISTORY {
            self.closed.pop_front();
        }
    }

    pub fn open_count(&self) -> usize {
        self.resources.len()
    }
}

fn read_bounded_record(
    resource: &mut ReaderResource,
    delimiter: u8,
    output: &mut Vec<u8>,
) -> Result<(), ReaderError> {
    loop {
        let available = resource
            .reader
            .fill_buf()
            .map_err(|error| io_error(error, resource.physical_line))?;
        if available.is_empty() {
            resource.eof = true;
            break;
        }
        let take =
            memchr::memchr(delimiter, available).map_or(available.len(), |position| position + 1);
        let next_total = resource.bytes_read.saturating_add(take);
        let next_record = output.len().saturating_add(take);
        if next_total > resource.max_file_bytes || next_record > resource.max_record_bytes {
            resource.failed = true;
            return Err(ReaderError::new(
                ReaderErrorKind::Limit,
                "reader byte or record limit exceeded",
                resource.physical_line,
                0,
            ));
        }
        output.extend_from_slice(&available[..take]);
        let found_delimiter = available[take - 1] == delimiter;
        resource.reader.consume(take);
        resource.bytes_read = next_total;
        if found_delimiter {
            break;
        }
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CsvState {
    Start,
    Unquoted,
    Quoted,
    AfterQuote,
    AfterQuoteCr,
}

/// The common record: complete in the read buffer, valid UTF-8, within the limits, with no
/// quote or inner carriage return. Its fields are exactly the comma-separated pieces. Returns
/// None without consuming anything otherwise, and the general reader takes it from there.
fn read_plain_csv_record(resource: &mut ReaderResource) -> Option<Vec<Arc<str>>> {
    let available = resource.reader.fill_buf().ok()?;
    let newline = memchr::memchr(b'\n', available)?;
    let consumed = newline + 1;
    if resource.bytes_read.saturating_add(consumed) > resource.max_file_bytes
        || consumed > resource.max_record_bytes
    {
        return None;
    }
    let line = &available[..newline];
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    if memchr::memchr2(b'"', b'\r', line).is_some() {
        return None;
    }
    let line = std::str::from_utf8(line).ok()?;
    let fields = line.split(',').map(Arc::from).collect();
    resource.reader.consume(consumed);
    resource.bytes_read += consumed;
    resource.physical_line = resource.physical_line.saturating_add(1);
    Some(fields)
}

fn read_csv_record(resource: &mut ReaderResource) -> Result<Option<Vec<u8>>, ReaderError> {
    let mut output = Vec::new();
    let mut state = CsvState::Start;
    let mut newlines = 0usize;
    loop {
        let available = resource
            .reader
            .fill_buf()
            .map_err(|error| io_error(error, resource.physical_line))?;
        if available.is_empty() {
            resource.eof = true;
            if output.is_empty() {
                return Ok(None);
            }
            resource.physical_line = resource.physical_line.saturating_add(newlines);
            return Ok(Some(output));
        }
        let mut consumed = 0usize;
        let mut end_record = false;
        for byte in available.iter().copied() {
            consumed += 1;
            match state {
                CsvState::Start => match byte {
                    b'"' => state = CsvState::Quoted,
                    b',' => {}
                    b'\n' => {
                        end_record = true;
                        newlines += 1;
                    }
                    _ => state = CsvState::Unquoted,
                },
                CsvState::Unquoted => match byte {
                    b',' => state = CsvState::Start,
                    b'\n' => {
                        end_record = true;
                        newlines += 1;
                    }
                    _ => {}
                },
                CsvState::Quoted => {
                    if byte == b'"' {
                        state = CsvState::AfterQuote
                    } else if byte == b'\n' {
                        newlines += 1;
                    }
                }
                CsvState::AfterQuote => match byte {
                    b'"' => state = CsvState::Quoted,
                    b',' => state = CsvState::Start,
                    b'\r' => state = CsvState::AfterQuoteCr,
                    b'\n' => {
                        end_record = true;
                        newlines += 1;
                    }
                    _ => state = CsvState::Unquoted,
                },
                CsvState::AfterQuoteCr => {
                    if byte == b'\n' {
                        end_record = true;
                        newlines += 1;
                    } else {
                        state = CsvState::Unquoted
                    }
                }
            }
            if end_record {
                break;
            }
        }
        if resource.bytes_read.saturating_add(consumed) > resource.max_file_bytes
            || output.len().saturating_add(consumed) > resource.max_record_bytes
        {
            resource.failed = true;
            return Err(ReaderError::new(
                ReaderErrorKind::Limit,
                "CSV file or record limit exceeded",
                resource.physical_line + newlines,
                0,
            ));
        }
        output.extend_from_slice(&available[..consumed]);
        resource.reader.consume(consumed);
        resource.bytes_read += consumed;
        if end_record {
            resource.physical_line = resource.physical_line.saturating_add(newlines);
            return Ok(Some(output));
        }
    }
}

fn parse_csv_record(
    bytes: &[u8],
    start_line: usize,
) -> Result<Vec<String>, (usize, usize, String)> {
    let text = std::str::from_utf8(bytes).map_err(|error| {
        (
            start_line,
            error.valid_up_to() + 1,
            "CSV data is not valid UTF-8".to_owned(),
        )
    })?;
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut chars = text.char_indices().peekable();
    let mut state = CsvState::Start;
    while let Some((index, character)) = chars.next() {
        match state {
            CsvState::Start => match character {
                '"' => state = CsvState::Quoted,
                ',' => fields.push(String::new()),
                '\n' => break,
                '\r' if chars.peek().is_none() => break,
                _ => {
                    field.push(character);
                    state = CsvState::Unquoted;
                }
            },
            CsvState::Unquoted => match character {
                '"' => {
                    return Err((
                        start_line,
                        index + 1,
                        "quote inside unquoted CSV field".into(),
                    ));
                }
                ',' => {
                    fields.push(std::mem::take(&mut field));
                    state = CsvState::Start;
                }
                '\n' => break,
                _ => field.push(character),
            },
            CsvState::Quoted => match character {
                '"' if chars.peek().is_some_and(|(_, next)| *next == '"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => state = CsvState::AfterQuote,
                _ => field.push(character),
            },
            CsvState::AfterQuote => match character {
                ',' => {
                    fields.push(std::mem::take(&mut field));
                    state = CsvState::Start;
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
            CsvState::AfterQuoteCr => unreachable!(),
        }
    }
    if state == CsvState::Quoted {
        return Err((
            start_line,
            text.len() + 1,
            "unterminated quoted CSV field".into(),
        ));
    }
    fields.push(field);
    Ok(fields)
}

fn io_error(error: io::Error, line: usize) -> ReaderError {
    ReaderError::new(
        ReaderErrorKind::Io,
        format!("file read failed: {error}"),
        line,
        0,
    )
}
