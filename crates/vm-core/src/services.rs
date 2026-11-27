//! Pure services shared by the native runtime and the browser VM: the service error, HMAC-SHA-256,
//! Unicode case folding, whitespace collapsing and normalization, and TOML documents.

use std::{
    collections::{HashMap, HashSet},
    sync::atomic::{AtomicU64, Ordering},
};

use hmac::{Hmac, Mac};
use icu_casemap::CaseMapper;
use icu_normalizer::{ComposingNormalizerBorrowed, DecomposingNormalizerBorrowed};
use icu_properties::{props::WhiteSpace, CodePointSetData};
use sha2::Sha256;
use toml_edit::{Document, DocumentMut, Item, Table, Value};

pub const UNICODE_DATA_VERSION: &str = "18.0.0";
pub const IANA_TZDB_VERSION: &str = "2026c";

static NEXT_SERVICE_OWNER: AtomicU64 = AtomicU64::new(1);

/// A fresh owner id for a registry of handles; no two registries in a process share one.
pub fn next_service_owner() -> u64 {
    NEXT_SERVICE_OWNER.fetch_add(1, Ordering::Relaxed)
}

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceError {
    pub code: String,
    pub message: String,
    pub path: String,
    pub line: usize,
    pub column: usize,
}

impl ServiceError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_owned(),
            message: message.into(),
            path: String::new(),
            line: 0,
            column: 0,
        }
    }
    pub fn at_path(mut self, path: impl Into<String>) -> Self {
        self.path = path.into();
        self
    }
    pub fn at_position(mut self, source: &str, offset: usize) -> Self {
        let prefix = &source[..offset.min(source.len())];
        self.line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
        self.column = prefix
            .rsplit('\n')
            .next()
            .unwrap_or_default()
            .chars()
            .count()
            + 1;
        self
    }
}

impl std::fmt::Display for ServiceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ServiceError {}

pub fn crypto_hmac_sha256(key: &[u8], body: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts keys of every size");
    mac.update(body);
    mac.finalize().into_bytes().to_vec()
}

pub fn crypto_verify_hmac_sha256(key: &[u8], body: &[u8], expected: &[u8]) -> bool {
    if expected.len() != 32 {
        return false;
    }
    let Ok(mut mac) = HmacSha256::new_from_slice(key) else {
        return false;
    };
    mac.update(body);
    mac.verify_slice(expected).is_ok()
}

pub fn text_casefold(text: &str) -> String {
    CaseMapper::new().fold_string(text).into_owned()
}

pub fn text_collapse_whitespace(text: &str) -> String {
    let whitespace = CodePointSetData::new::<WhiteSpace>();
    let mut output = String::with_capacity(text.len());
    let mut needs_space = false;
    for scalar in text.chars() {
        if whitespace.contains(scalar) {
            needs_space = !output.is_empty();
        } else {
            if needs_space {
                output.push(' ');
            }
            needs_space = false;
            output.push(scalar);
        }
    }
    output
}

/// Normalize Unicode whitespace only when the input would actually change.
///
/// The common case for machine-readable text is already normalized. Returning
/// `None` lets the VM keep its owned text value without allocating and copying
/// an identical string.
pub fn text_collapse_whitespace_if_changed(text: &str) -> Option<String> {
    // JSON and other machine-readable text are commonly ASCII. Check that
    // case with a byte loop so it does not perform a Unicode-set lookup for
    // every scalar. If any non-ASCII byte appears, use the complete Unicode
    // path below.
    let mut first = true;
    let mut previous_was_whitespace = false;
    let mut all_ascii = true;
    for byte in text.bytes() {
        if !byte.is_ascii() {
            all_ascii = false;
            break;
        }
        let whitespace = matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c);
        if whitespace {
            if first || previous_was_whitespace || byte != b' ' {
                return Some(text_collapse_whitespace(text));
            }
            previous_was_whitespace = true;
        } else {
            previous_was_whitespace = false;
        }
        first = false;
    }
    if all_ascii {
        return previous_was_whitespace.then(|| text_collapse_whitespace(text));
    }

    let whitespace = CodePointSetData::new::<WhiteSpace>();
    first = true;
    previous_was_whitespace = false;

    for scalar in text.chars() {
        if whitespace.contains(scalar) {
            if first || previous_was_whitespace || scalar != ' ' {
                return Some(text_collapse_whitespace(text));
            }
            previous_was_whitespace = true;
        } else {
            previous_was_whitespace = false;
        }
        first = false;
    }

    if previous_was_whitespace {
        Some(text_collapse_whitespace(text))
    } else {
        None
    }
}

pub fn text_normalize(text: &str, form: &str) -> Result<String, ServiceError> {
    let normalized = match form {
        "NFC" => ComposingNormalizerBorrowed::new_nfc().normalize(text),
        "NFD" => DecomposingNormalizerBorrowed::new_nfd().normalize(text),
        "NFKC" => ComposingNormalizerBorrowed::new_nfkc().normalize(text),
        "NFKD" => DecomposingNormalizerBorrowed::new_nfkd().normalize(text),
        _ => {
            return Err(ServiceError::new(
                "E_NORMALIZATION_FORM",
                "normalization form must be NFC, NFD, NFKC, or NFKD",
            ));
        }
    };
    Ok(normalized.into_owned())
}

#[derive(Debug)]
pub struct TomlDocument {
    document: DocumentMut,
    spanned: Document<String>,
    original: String,
    edited: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TomlHandle {
    owner: u64,
    id: u64,
}

impl TomlHandle {
    pub fn from_parts(owner: u64, id: u64) -> Self {
        Self { owner, id }
    }
    pub fn parts(self) -> (u64, u64) {
        (self.owner, self.id)
    }
}

/// Keeps mutable TOML documents instance-owned and bounds their input size.
#[derive(Debug)]
pub struct TomlRegistry {
    owner: u64,
    next_id: u64,
    max_bytes: usize,
    documents: HashMap<u64, TomlDocument>,
    closed: HashSet<u64>,
}

impl TomlRegistry {
    pub fn new(max_bytes: usize) -> Self {
        Self {
            owner: next_service_owner(),
            next_id: 1,
            max_bytes,
            documents: HashMap::new(),
            closed: HashSet::new(),
        }
    }
    pub fn parse(&mut self, source: &str) -> Result<TomlHandle, ServiceError> {
        if source.len() > self.max_bytes {
            return Err(ServiceError::new(
                "E_TOML_LIMIT",
                "TOML input exceeds the configured byte limit",
            ));
        }
        let document = TomlDocument::parse(source)?;
        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or_else(|| ServiceError::new("E_RESOURCE_LIMIT", "TOML handle space exhausted"))?;
        self.documents.insert(id, document);
        self.closed.remove(&id);
        Ok(TomlHandle {
            owner: self.owner,
            id,
        })
    }
    pub fn has(&self, handle: TomlHandle, path: &[String]) -> Result<bool, ServiceError> {
        let document = self.get(handle)?;
        let keys = path.iter().map(String::as_str).collect::<Vec<_>>();
        Ok(document.has(&keys))
    }
    pub fn get_text(&self, handle: TomlHandle, path: &[String]) -> Result<String, ServiceError> {
        let keys = path.iter().map(String::as_str).collect::<Vec<_>>();
        self.get(handle)?.get_text(&keys)
    }
    pub fn get_text_list(
        &self,
        handle: TomlHandle,
        path: &[String],
    ) -> Result<Vec<String>, ServiceError> {
        let keys = path.iter().map(String::as_str).collect::<Vec<_>>();
        self.get(handle)?.get_text_list(&keys)
    }
    pub fn keys(&self, handle: TomlHandle, path: &[String]) -> Result<Vec<String>, ServiceError> {
        let keys = path.iter().map(String::as_str).collect::<Vec<_>>();
        self.get(handle)?.keys(&keys)
    }
    pub fn get_integer(&self, handle: TomlHandle, path: &[String]) -> Result<i64, ServiceError> {
        let keys = path.iter().map(String::as_str).collect::<Vec<_>>();
        self.get(handle)?.get_integer(&keys)
    }
    pub fn get_bool(&self, handle: TomlHandle, path: &[String]) -> Result<bool, ServiceError> {
        let keys = path.iter().map(String::as_str).collect::<Vec<_>>();
        self.get(handle)?.get_bool(&keys)
    }
    pub fn put_text(
        &mut self,
        handle: TomlHandle,
        path: &[String],
        value: &str,
    ) -> Result<(), ServiceError> {
        let keys = path.iter().map(String::as_str).collect::<Vec<_>>();
        self.get_mut(handle)?.put_text(&keys, value)
    }
    pub fn put_integer(
        &mut self,
        handle: TomlHandle,
        path: &[String],
        value: i64,
    ) -> Result<(), ServiceError> {
        let keys = path.iter().map(String::as_str).collect::<Vec<_>>();
        self.get_mut(handle)?.put_integer(&keys, value)
    }
    pub fn put_bool(
        &mut self,
        handle: TomlHandle,
        path: &[String],
        value: bool,
    ) -> Result<(), ServiceError> {
        let keys = path.iter().map(String::as_str).collect::<Vec<_>>();
        self.get_mut(handle)?.put_bool(&keys, value)
    }
    pub fn render(&self, handle: TomlHandle) -> Result<String, ServiceError> {
        Ok(self.get(handle)?.render().to_owned())
    }
    pub fn open_count(&self) -> usize {
        self.documents.len()
    }
    pub fn close_all(&mut self) {
        self.documents.clear();
        self.closed.clear();
    }

    fn get(&self, handle: TomlHandle) -> Result<&TomlDocument, ServiceError> {
        if handle.owner != self.owner {
            return Err(ServiceError::new(
                "E_RESOURCE_OWNER",
                "TOML document belongs to another runtime instance",
            ));
        }
        self.documents.get(&handle.id).ok_or_else(|| {
            ServiceError::new(
                if self.closed.contains(&handle.id) {
                    "E_RESOURCE_CLOSED"
                } else {
                    "E_RESOURCE_STALE"
                },
                "TOML document is closed or stale",
            )
        })
    }
    fn get_mut(&mut self, handle: TomlHandle) -> Result<&mut TomlDocument, ServiceError> {
        if handle.owner != self.owner {
            return Err(ServiceError::new(
                "E_RESOURCE_OWNER",
                "TOML document belongs to another runtime instance",
            ));
        }
        if !self.documents.contains_key(&handle.id) {
            return Err(ServiceError::new(
                if self.closed.contains(&handle.id) {
                    "E_RESOURCE_CLOSED"
                } else {
                    "E_RESOURCE_STALE"
                },
                "TOML document is closed or stale",
            ));
        }
        match self.documents.get_mut(&handle.id) {
            Some(document) => Ok(document),
            None => Err(ServiceError::new(
                "E_RESOURCE_STALE",
                "TOML document is stale",
            )),
        }
    }
}

impl Drop for TomlRegistry {
    fn drop(&mut self) {
        self.close_all();
    }
}

impl TomlDocument {
    pub fn parse(source: &str) -> Result<Self, ServiceError> {
        let spanned = source
            .to_owned()
            .parse::<Document<String>>()
            .map_err(|error| {
                let result = ServiceError::new("E_TOML_PARSE", error.to_string());
                error.span().map_or(result.clone(), |span| {
                    result.at_position(source, span.start)
                })
            })?;
        let document = spanned.clone().into_mut();
        Ok(Self {
            document,
            spanned,
            original: source.to_owned(),
            edited: None,
        })
    }

    pub fn render(&self) -> &str {
        self.edited.as_deref().unwrap_or(&self.original)
    }

    pub fn has(&self, path: &[&str]) -> bool {
        get_item(self.document.as_item(), path).is_some()
    }

    pub fn get_text(&self, path: &[&str]) -> Result<String, ServiceError> {
        get_item(self.document.as_item(), path)
            .and_then(Item::as_str)
            .map(str::to_owned)
            .ok_or_else(|| wrong_toml_type("text", path))
    }
    /// The strings of the array at `path`, in order. Every element must be a string.
    pub fn get_text_list(&self, path: &[&str]) -> Result<Vec<String>, ServiceError> {
        let array = get_item(self.document.as_item(), path)
            .and_then(Item::as_array)
            .ok_or_else(|| wrong_toml_type("an array of text", path))?;
        array
            .iter()
            .enumerate()
            .map(|(index, element)| {
                element.as_str().map(str::to_owned).ok_or_else(|| {
                    ServiceError::new(
                        "E_TOML_TYPE",
                        format!("{}[{index}] must be text", path.join(".")),
                    )
                })
            })
            .collect()
    }
    /// The keys of the table at `path` (the root when empty), in document order.
    pub fn keys(&self, path: &[&str]) -> Result<Vec<String>, ServiceError> {
        let table = get_item(self.document.as_item(), path)
            .and_then(Item::as_table_like)
            .ok_or_else(|| wrong_toml_type("a table", path))?;
        Ok(table.iter().map(|(key, _)| key.to_owned()).collect())
    }
    pub fn get_integer(&self, path: &[&str]) -> Result<i64, ServiceError> {
        get_item(self.document.as_item(), path)
            .and_then(Item::as_integer)
            .ok_or_else(|| wrong_toml_type("integer", path))
    }
    pub fn get_bool(&self, path: &[&str]) -> Result<bool, ServiceError> {
        get_item(self.document.as_item(), path)
            .and_then(Item::as_bool)
            .ok_or_else(|| wrong_toml_type("boolean", path))
    }
    pub fn put_text(&mut self, path: &[&str], value: &str) -> Result<(), ServiceError> {
        self.put(path, Value::from(value), |document| {
            get_item(document.as_item(), path)
                .and_then(Item::as_str)
                .is_some_and(|actual| actual == value)
        })
    }
    pub fn put_integer(&mut self, path: &[&str], value: i64) -> Result<(), ServiceError> {
        self.put(path, Value::from(value), |document| {
            get_item(document.as_item(), path).and_then(Item::as_integer) == Some(value)
        })
    }
    pub fn put_bool(&mut self, path: &[&str], value: bool) -> Result<(), ServiceError> {
        self.put(path, Value::from(value), |document| {
            get_item(document.as_item(), path).and_then(Item::as_bool) == Some(value)
        })
    }

    fn put(
        &mut self,
        path: &[&str],
        value: Value,
        verify: impl Fn(&DocumentMut) -> bool,
    ) -> Result<(), ServiceError> {
        if path.is_empty() || path.iter().any(|key| key.is_empty()) {
            return Err(ServiceError::new(
                "E_TOML_KEY",
                "TOML key path must contain non-empty segments",
            ));
        }
        if verify(&self.document) {
            return Ok(());
        }
        let source = self.render().to_owned();
        let mut candidate = self.document.clone();
        let replacement_span = get_item(self.spanned.as_item(), path)
            .and_then(Item::as_value)
            .and_then(Value::span);
        if get_item(candidate.as_item(), path).is_some() {
            let existing = get_item_mut(candidate.as_item_mut(), path)
                .expect("TOML path was found in the immutable candidate");
            let existing = existing
                .as_value_mut()
                .ok_or_else(|| wrong_toml_type("scalar", path))?;
            if existing.type_name() != value.type_name() {
                return Err(wrong_toml_type(value.type_name(), path));
            }
            *existing = value.clone();
        } else {
            let table = table_at_path(candidate.as_table_mut(), &path[..path.len() - 1])?;
            table.insert(path[path.len() - 1], Item::Value(value.clone()));
        }
        let rendered = if let Some(span) = replacement_span.filter(|span| {
            span.end <= source.len()
                && source.is_char_boundary(span.start)
                && source.is_char_boundary(span.end)
        }) {
            let mut output =
                String::with_capacity(source.len().saturating_add(value.to_string().len()));
            output.push_str(&source[..span.start]);
            output.push_str(&value.to_string());
            output.push_str(&source[span.end..]);
            output
        } else if !self.has(path) {
            insert_toml_scalar(&source, &self.spanned, path, &value)
                .unwrap_or_else(|| candidate.to_string())
        } else {
            candidate.to_string()
        };
        let spanned = rendered
            .to_owned()
            .parse::<Document<String>>()
            .map_err(|error| ServiceError::new("E_TOML_EDIT", error.to_string()))?;
        let reparsed = spanned.clone().into_mut();
        if !verify(&reparsed) {
            return Err(ServiceError::new(
                "E_TOML_EDIT",
                "edited TOML did not retain the requested value",
            ));
        }
        self.document = reparsed;
        self.spanned = spanned;
        self.edited = Some(rendered);
        Ok(())
    }
}

fn insert_toml_scalar(
    source: &str,
    document: &Document<String>,
    path: &[&str],
    value: &Value,
) -> Option<String> {
    let parent = get_item(document.as_item(), &path[..path.len().checked_sub(1)?])?.as_table()?;
    let insertion = parent
        .iter()
        .filter(|(_, item)| item.as_table().is_none() && item.as_array_of_tables().is_none())
        .filter_map(|(_, item)| item.span().map(|span| span.end))
        .max()
        .or_else(|| parent.span().map(|span| span.end))
        .unwrap_or(0);
    if insertion > source.len() || !source.is_char_boundary(insertion) {
        return None;
    }
    let offset = source[insertion..]
        .find('\n')
        .map_or(source.len(), |relative| insertion + relative + 1);
    let newline = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let key = toml_edit::Key::new(*path.last()?).to_string();
    let line = format!("{key} = {value}");
    let needs_leading_newline = offset == source.len() && offset > 0 && !source.ends_with('\n');
    let needs_trailing_newline = offset < source.len() || source.ends_with('\n');
    let mut output = String::with_capacity(source.len().saturating_add(line.len() + newline.len()));
    output.push_str(&source[..offset]);
    if needs_leading_newline {
        output.push_str(newline);
    }
    output.push_str(&line);
    if needs_trailing_newline {
        output.push_str(newline);
    }
    output.push_str(&source[offset..]);
    Some(output)
}

fn table_at_path<'a>(
    mut table: &'a mut Table,
    path: &[&str],
) -> Result<&'a mut Table, ServiceError> {
    for key in path {
        let item = table.entry(key).or_insert(Item::None);
        if item.is_none() {
            *item = Item::Table(Table::new());
        }
        if item.as_table_mut().is_none() {
            return Err(ServiceError::new(
                "E_TOML_TYPE",
                format!("{key} is not a table"),
            ));
        }
        table = item.as_table_mut().expect("table type checked above");
    }
    Ok(table)
}

fn get_item<'a>(mut item: &'a Item, path: &[&str]) -> Option<&'a Item> {
    for segment in path {
        item = item.get(segment)?;
    }
    Some(item)
}

fn get_item_mut<'a>(mut item: &'a mut Item, path: &[&str]) -> Option<&'a mut Item> {
    for segment in path {
        item = item.get_mut(segment)?;
    }
    Some(item)
}

fn wrong_toml_type(expected: &str, path: &[&str]) -> ServiceError {
    ServiceError::new(
        "E_TOML_TYPE",
        format!("{} must be {expected}", path.join(".")),
    )
}
