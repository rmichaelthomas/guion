//! Native services backing Guion' durable-state and interoperability APIs.
use std::{
    collections::{HashMap, HashSet},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::{Duration, Instant},
};

use hmac::{Hmac, Mac};
use icu_casemap::CaseMapper;
use icu_normalizer::{ComposingNormalizerBorrowed, DecomposingNormalizerBorrowed};
use icu_properties::{props::WhiteSpace, CodePointSetData};
use rusqlite::{
    params_from_iter,
    types::{Value as SqliteValue, ValueRef},
    Connection, OpenFlags,
};
use sha2::Sha256;
use toml_edit::{Document, DocumentMut, Item, Table, Value};

pub const UNICODE_DATA_VERSION: &str = "18.0.0";
pub const IANA_TZDB_VERSION: &str = "2026c";
const SQLITE_BUSY_DEADLINE_MS: u64 = 30_000;
const CLOSED_HANDLE_CACHE: usize = 4096;
static NEXT_SERVICE_OWNER: AtomicU64 = AtomicU64::new(1);

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
    pub(crate) fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_owned(),
            message: message.into(),
            path: String::new(),
            line: 0,
            column: 0,
        }
    }
    pub(crate) fn at_path(mut self, path: impl Into<String>) -> Self {
        self.path = path.into();
        self
    }
    fn at_position(mut self, source: &str, offset: usize) -> Self {
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
    let whitespace = CodePointSetData::new::<WhiteSpace>();
    let mut first = true;
    let mut previous_was_whitespace = false;

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
            ))
        }
    };
    Ok(normalized.into_owned())
}

#[derive(Clone, Debug, PartialEq)]
pub enum SqlValue {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct SqlRow {
    pub columns: Vec<String>,
    pub values: Vec<SqlValue>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DbHandle {
    owner: u64,
    id: u64,
}

impl DbHandle {
    pub fn from_parts(owner: u64, id: u64) -> Self {
        Self { owner, id }
    }
    pub fn parts(self) -> (u64, u64) {
        (self.owner, self.id)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TransactionHandle {
    owner: u64,
    id: u64,
}

impl TransactionHandle {
    pub fn from_parts(owner: u64, id: u64) -> Self {
        Self { owner, id }
    }
    pub fn parts(self) -> (u64, u64) {
        (self.owner, self.id)
    }
}

#[derive(Debug)]
struct DatabaseResource {
    connection: Connection,
    active_transaction: Option<u64>,
}

#[derive(Debug)]
struct TransactionResource {
    database_id: u64,
    cancellation: std::sync::Arc<AtomicBool>,
}

/// Owns SQLite connections and transactions for one runtime instance.
#[derive(Debug)]
pub struct DatabaseRegistry {
    owner: u64,
    next_id: u64,
    databases: HashMap<u64, DatabaseResource>,
    transactions: HashMap<u64, TransactionResource>,
    allowed_roots: Vec<PathBuf>,
    max_result_bytes: usize,
    max_rows: usize,
    busy_deadline: Duration,
    closed_databases: HashSet<u64>,
    closed_transactions: HashSet<u64>,
}

impl DatabaseRegistry {
    pub fn new(max_result_bytes: usize, max_rows: usize, busy_deadline_ms: u64) -> Self {
        Self::with_roots(
            Vec::<PathBuf>::new(),
            max_result_bytes,
            max_rows,
            busy_deadline_ms,
        )
    }

    pub fn with_roots(
        roots: impl IntoIterator<Item = PathBuf>,
        max_result_bytes: usize,
        max_rows: usize,
        busy_deadline_ms: u64,
    ) -> Self {
        let allowed_roots = roots
            .into_iter()
            .filter_map(|root| root.canonicalize().ok())
            .collect();
        Self {
            owner: NEXT_SERVICE_OWNER.fetch_add(1, Ordering::Relaxed),
            next_id: 1,
            databases: HashMap::new(),
            transactions: HashMap::new(),
            allowed_roots,
            max_result_bytes,
            max_rows,
            busy_deadline: Duration::from_millis(
                busy_deadline_ms.clamp(1, SQLITE_BUSY_DEADLINE_MS),
            ),
            closed_databases: HashSet::new(),
            closed_transactions: HashSet::new(),
        }
    }

    pub fn open(&mut self, path: impl AsRef<Path>) -> Result<DbHandle, ServiceError> {
        let path = self.resolve_database_path(path.as_ref())?;
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE;
        let connection = Connection::open_with_flags(&path, flags).map_err(|error| {
            sqlite_error("E_SQLITE_OPEN", error).at_path(path.to_string_lossy())
        })?;
        connection
            .busy_timeout(self.busy_deadline)
            .map_err(|error| sqlite_error("E_SQLITE_OPEN", error))?;
        connection
            .pragma_update(None, "trusted_schema", false)
            .map_err(|error| sqlite_error("E_SQLITE_OPEN", error))?;
        let id = self.allocate_id()?;
        self.databases.insert(
            id,
            DatabaseResource {
                connection,
                active_transaction: None,
            },
        );
        Ok(DbHandle {
            owner: self.owner,
            id,
        })
    }

    fn resolve_database_path(&self, path: &Path) -> Result<PathBuf, ServiceError> {
        if path.as_os_str().is_empty() || !path.is_absolute() {
            return Err(ServiceError::new(
                "E_SQLITE_PATH",
                "database path must be an absolute path inside a granted root",
            ));
        }
        if path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(ServiceError::new(
                "E_SQLITE_PATH",
                "database path may not contain parent-directory traversal",
            ));
        }
        let parent = path.parent().ok_or_else(|| {
            ServiceError::new("E_SQLITE_PATH", "database path has no parent directory")
        })?;
        let canonical_parent = parent.canonicalize().map_err(|error| {
            ServiceError::new(
                "E_SQLITE_PATH",
                format!("database directory is unavailable: {error}"),
            )
        })?;
        let candidate =
            canonical_parent.join(path.file_name().ok_or_else(|| {
                ServiceError::new("E_SQLITE_PATH", "database path has no filename")
            })?);
        let resolved = if candidate.exists() {
            candidate.canonicalize().map_err(|error| {
                ServiceError::new(
                    "E_SQLITE_PATH",
                    format!("database path is unavailable: {error}"),
                )
            })?
        } else {
            candidate
        };
        if !self
            .allowed_roots
            .iter()
            .any(|root| resolved.starts_with(root))
        {
            return Err(ServiceError::new(
                "E_SQLITE_PATH",
                "database path is outside every granted root",
            ));
        }
        Ok(resolved)
    }

    pub fn begin(&mut self, database: DbHandle) -> Result<TransactionHandle, ServiceError> {
        self.check_db_owner(database)?;
        if !self.databases.contains_key(&database.id) {
            return Err(self.database_closed_error(database.id));
        }
        if self
            .databases
            .get(&database.id)
            .is_some_and(|resource| resource.active_transaction.is_some())
        {
            return Err(ServiceError::new(
                "E_SQLITE_NESTED_TRANSACTION",
                "nested SQLite transactions are not allowed",
            ));
        }
        let id = self.allocate_id()?;
        let resource = match self.databases.get_mut(&database.id) {
            Some(resource) => resource,
            None => {
                return Err(ServiceError::new(
                    "E_RESOURCE_STALE",
                    "database handle is stale",
                ))
            }
        };
        resource
            .connection
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(|error| sqlite_error("E_SQLITE_BEGIN", error))?;
        resource.active_transaction = Some(id);
        self.transactions.insert(
            id,
            TransactionResource {
                database_id: database.id,
                cancellation: Default::default(),
            },
        );
        Ok(TransactionHandle {
            owner: self.owner,
            id,
        })
    }

    pub fn set_cancelled(
        &mut self,
        transaction: TransactionHandle,
        cancelled: std::sync::Arc<AtomicBool>,
    ) -> Result<(), ServiceError> {
        let resource = self.transaction_mut(transaction)?;
        resource.cancellation = cancelled;
        Ok(())
    }

    pub fn execute(
        &mut self,
        transaction: TransactionHandle,
        sql: &str,
        values: &[SqlValue],
    ) -> Result<i64, ServiceError> {
        let (database_id, cancellation) = self.transaction_snapshot(transaction)?;
        validate_sql(sql)?;
        let deadline = Instant::now() + self.busy_deadline;
        let resource = self
            .databases
            .get_mut(&database_id)
            .ok_or_else(|| ServiceError::new("E_SQLITE_STALE", "database handle is stale"))?;
        install_progress_handler(&resource.connection, cancellation.clone(), deadline);
        let params = values.iter().map(sqlite_owned_value).collect::<Vec<_>>();
        let result = resource
            .connection
            .execute(sql, params_from_iter(params.iter()))
            .map(|count| i64::try_from(count).unwrap_or(i64::MAX))
            .map_err(|error| {
                sqlite_error(
                    if cancellation.load(Ordering::Acquire) {
                        "E_SQLITE_CANCELLED"
                    } else {
                        "E_SQLITE_EXECUTE"
                    },
                    error,
                )
            });
        clear_progress_handler(&resource.connection);
        result
    }

    pub fn query(
        &mut self,
        transaction: TransactionHandle,
        sql: &str,
        values: &[SqlValue],
    ) -> Result<Vec<SqlRow>, ServiceError> {
        let (database_id, cancellation) = self.transaction_snapshot(transaction)?;
        validate_sql(sql)?;
        let deadline = Instant::now() + self.busy_deadline;
        let max_rows = self.max_rows;
        let max_bytes = self.max_result_bytes;
        let resource = self
            .databases
            .get_mut(&database_id)
            .ok_or_else(|| ServiceError::new("E_SQLITE_STALE", "database handle is stale"))?;
        install_progress_handler(&resource.connection, cancellation.clone(), deadline);
        let params = values.iter().map(sqlite_owned_value).collect::<Vec<_>>();
        let result = (|| {
            let mut statement = resource
                .connection
                .prepare(sql)
                .map_err(|error| sqlite_error("E_SQLITE_QUERY", error))?;
            let columns = statement
                .column_names()
                .iter()
                .map(|name| (*name).to_owned())
                .collect::<Vec<_>>();
            let mut rows = statement
                .query(params_from_iter(params.iter()))
                .map_err(|error| sqlite_error("E_SQLITE_QUERY", error))?;
            let mut output = Vec::new();
            let mut used_bytes = columns.iter().map(String::len).sum::<usize>();
            while let Some(row) = rows
                .next()
                .map_err(|error| sqlite_error("E_SQLITE_QUERY", error))?
            {
                if cancellation.load(Ordering::Acquire) {
                    return Err(ServiceError::new(
                        "E_SQLITE_CANCELLED",
                        "SQLite query was cancelled",
                    ));
                }
                if output.len() >= max_rows {
                    return Err(ServiceError::new(
                        "E_SQLITE_RESULT_LIMIT",
                        "SQLite query exceeded the configured row limit",
                    ));
                }
                let mut values = Vec::with_capacity(columns.len());
                for index in 0..columns.len() {
                    let value = match row
                        .get_ref(index)
                        .map_err(|error| sqlite_error("E_SQLITE_QUERY", error))?
                    {
                        ValueRef::Null => SqlValue::Null,
                        ValueRef::Integer(value) => SqlValue::Integer(value),
                        ValueRef::Real(value) if value.is_finite() => SqlValue::Real(value),
                        ValueRef::Real(_) => {
                            return Err(ServiceError::new(
                                "E_SQLITE_VALUE",
                                "SQLite returned a non-finite real number",
                            ))
                        }
                        ValueRef::Text(bytes) => SqlValue::Text(
                            std::str::from_utf8(bytes)
                                .map_err(|_| {
                                    ServiceError::new(
                                        "E_SQLITE_UTF8",
                                        "SQLite returned invalid UTF-8 text",
                                    )
                                })?
                                .to_owned(),
                        ),
                        ValueRef::Blob(bytes) => SqlValue::Blob(bytes.to_vec()),
                    };
                    used_bytes = used_bytes.saturating_add(sql_value_size(&value));
                    if used_bytes > max_bytes {
                        return Err(ServiceError::new(
                            "E_SQLITE_RESULT_LIMIT",
                            "SQLite query exceeded the configured result-byte limit",
                        ));
                    }
                    values.push(value);
                }
                output.push(SqlRow {
                    columns: columns.clone(),
                    values,
                });
            }
            Ok(output)
        })();
        clear_progress_handler(&resource.connection);
        result
    }

    pub fn commit(&mut self, transaction: TransactionHandle) -> Result<(), ServiceError> {
        self.finish_transaction(transaction, "COMMIT", "E_SQLITE_COMMIT")
    }

    pub fn rollback(&mut self, transaction: TransactionHandle) -> Result<(), ServiceError> {
        self.finish_transaction(transaction, "ROLLBACK", "E_SQLITE_ROLLBACK")
    }

    fn finish_transaction(
        &mut self,
        transaction: TransactionHandle,
        sql: &str,
        code: &str,
    ) -> Result<(), ServiceError> {
        self.check_tx_owner(transaction)?;
        let tx = self
            .transactions
            .get(&transaction.id)
            .ok_or_else(|| self.transaction_closed_error(transaction.id))?;
        let database_id = tx.database_id;
        let connection = &self
            .databases
            .get(&database_id)
            .ok_or_else(|| ServiceError::new("E_SQLITE_STALE", "database handle is stale"))?
            .connection;
        if !(sql == "ROLLBACK" && connection.is_autocommit()) {
            connection
                .execute_batch(sql)
                .map_err(|error| sqlite_error(code, error))?;
        }
        if let Some(database) = self.databases.get_mut(&database_id) {
            database.active_transaction = None;
        }
        self.transactions.remove(&transaction.id);
        remember_closed_handle(&mut self.closed_transactions, transaction.id, self.next_id);
        Ok(())
    }

    pub fn close(&mut self, database: DbHandle) -> Result<(), ServiceError> {
        self.check_db_owner(database)?;
        let mut resource = self
            .databases
            .remove(&database.id)
            .ok_or_else(|| self.database_closed_error(database.id))?;
        if let Some(tx_id) = resource.active_transaction.take() {
            let _ = resource.connection.execute_batch("ROLLBACK");
            self.transactions.remove(&tx_id);
            remember_closed_handle(&mut self.closed_transactions, tx_id, self.next_id);
        }
        remember_closed_handle(&mut self.closed_databases, database.id, self.next_id);
        Ok(())
    }

    pub fn close_all(&mut self) {
        let next_id = self.next_id;
        for (id, database) in std::mem::take(&mut self.databases) {
            if let Some(transaction_id) = database.active_transaction {
                let _ = database.connection.execute_batch("ROLLBACK");
                remember_closed_handle(&mut self.closed_transactions, transaction_id, next_id);
            }
            remember_closed_handle(&mut self.closed_databases, id, next_id);
        }
        for id in self.transactions.keys().copied().collect::<Vec<_>>() {
            remember_closed_handle(&mut self.closed_transactions, id, next_id);
        }
        self.transactions.clear();
    }

    pub fn open_count(&self) -> usize {
        self.databases.len()
    }
    pub fn transaction_count(&self) -> usize {
        self.transactions.len()
    }

    fn check_db_owner(&self, handle: DbHandle) -> Result<(), ServiceError> {
        if handle.owner != self.owner {
            return Err(ServiceError::new(
                "E_RESOURCE_OWNER",
                "database handle belongs to another runtime instance",
            ));
        }
        Ok(())
    }
    fn check_tx_owner(&self, handle: TransactionHandle) -> Result<(), ServiceError> {
        if handle.owner != self.owner {
            return Err(ServiceError::new(
                "E_RESOURCE_OWNER",
                "transaction handle belongs to another runtime instance",
            ));
        }
        Ok(())
    }
    fn transaction_mut(
        &mut self,
        handle: TransactionHandle,
    ) -> Result<&mut TransactionResource, ServiceError> {
        self.check_tx_owner(handle)?;
        if !self.transactions.contains_key(&handle.id) {
            return Err(self.transaction_closed_error(handle.id));
        }
        match self.transactions.get_mut(&handle.id) {
            Some(resource) => Ok(resource),
            None => Err(ServiceError::new(
                "E_RESOURCE_STALE",
                "transaction handle is stale",
            )),
        }
    }
    fn transaction_snapshot(
        &self,
        handle: TransactionHandle,
    ) -> Result<(u64, std::sync::Arc<AtomicBool>), ServiceError> {
        self.check_tx_owner(handle)?;
        self.transactions
            .get(&handle.id)
            .map(|tx| (tx.database_id, tx.cancellation.clone()))
            .ok_or_else(|| self.transaction_closed_error(handle.id))
    }
    fn database_closed_error(&self, id: u64) -> ServiceError {
        ServiceError::new(
            if self.closed_databases.contains(&id) {
                "E_RESOURCE_CLOSED"
            } else {
                "E_RESOURCE_STALE"
            },
            "database handle is closed or stale",
        )
    }
    fn transaction_closed_error(&self, id: u64) -> ServiceError {
        ServiceError::new(
            if self.closed_transactions.contains(&id) {
                "E_RESOURCE_CLOSED"
            } else {
                "E_RESOURCE_STALE"
            },
            "transaction handle is closed or stale",
        )
    }
    fn allocate_id(&mut self) -> Result<u64, ServiceError> {
        let id = self.next_id;
        self.next_id = self.next_id.checked_add(1).ok_or_else(|| {
            ServiceError::new("E_RESOURCE_LIMIT", "native resource handle space exhausted")
        })?;
        Ok(id)
    }
}

fn remember_closed_handle(handles: &mut HashSet<u64>, id: u64, next_id: u64) {
    handles.insert(id);
    if handles.len() > CLOSED_HANDLE_CACHE {
        let oldest_kept = next_id.saturating_sub(CLOSED_HANDLE_CACHE as u64);
        handles.retain(|closed_id| *closed_id >= oldest_kept);
    }
}

impl Drop for DatabaseRegistry {
    fn drop(&mut self) {
        self.close_all();
    }
}

fn install_progress_handler(
    connection: &Connection,
    cancellation: std::sync::Arc<AtomicBool>,
    deadline: Instant,
) {
    connection.progress_handler(
        1000,
        Some(move || cancellation.load(Ordering::Acquire) || Instant::now() >= deadline),
    );
}

fn clear_progress_handler(connection: &Connection) {
    connection.progress_handler(0, None::<fn() -> bool>);
}

fn sqlite_owned_value(value: &SqlValue) -> SqliteValue {
    match value {
        SqlValue::Null => SqliteValue::Null,
        SqlValue::Integer(value) => SqliteValue::Integer(*value),
        SqlValue::Real(value) => SqliteValue::Real(*value),
        SqlValue::Text(value) => SqliteValue::Text(value.clone()),
        SqlValue::Blob(value) => SqliteValue::Blob(value.clone()),
    }
}

fn sql_value_size(value: &SqlValue) -> usize {
    match value {
        SqlValue::Null => 1,
        SqlValue::Integer(_) | SqlValue::Real(_) => 8,
        SqlValue::Text(value) => value.len(),
        SqlValue::Blob(value) => value.len(),
    }
}

fn sqlite_error(code: &str, error: impl std::fmt::Display) -> ServiceError {
    ServiceError::new(code, error.to_string())
}

fn validate_sql(sql: &str) -> Result<(), ServiceError> {
    let tokens = sql_tokens(sql)?;
    for forbidden in ["ATTACH", "DETACH", "VACUUM", "PRAGMA", "LOAD_EXTENSION"] {
        if tokens.iter().any(|token| token == forbidden) {
            return Err(ServiceError::new(
                "E_SQLITE_POLICY",
                format!("{forbidden} is disabled by the database policy"),
            ));
        }
    }
    Ok(())
}

fn sql_tokens(sql: &str) -> Result<Vec<String>, ServiceError> {
    let bytes = sql.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\'' | b'"' | b'`' | b'[' => {
                let delimiter = if bytes[index] == b'[' {
                    b']'
                } else {
                    bytes[index]
                };
                index += 1;
                while index < bytes.len() {
                    if bytes[index] == delimiter {
                        if delimiter != b']' && bytes.get(index + 1) == Some(&delimiter) {
                            index += 2;
                            continue;
                        }
                        index += 1;
                        break;
                    }
                    index += 1;
                }
            }
            b'-' if bytes.get(index + 1) == Some(&b'-') => {
                index += 2;
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index += 2;
                while index + 1 < bytes.len() && !(bytes[index] == b'*' && bytes[index + 1] == b'/')
                {
                    index += 1;
                }
                if index + 1 >= bytes.len() {
                    return Err(ServiceError::new(
                        "E_SQLITE_SYNTAX",
                        "unterminated SQL comment",
                    ));
                }
                index += 2;
            }
            byte if byte.is_ascii_alphabetic() || byte == b'_' => {
                let start = index;
                index += 1;
                while index < bytes.len()
                    && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
                {
                    index += 1;
                }
                tokens.push(sql[start..index].to_ascii_uppercase());
            }
            _ => index += 1,
        }
    }
    Ok(tokens)
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
            owner: NEXT_SERVICE_OWNER.fetch_add(1, Ordering::Relaxed),
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CalendarEvent {
    pub uid: String,
    pub summary: String,
    pub status: String,
    pub all_day: bool,
    pub start_unix_seconds: Option<i64>,
    pub end_unix_seconds: Option<i64>,
    pub start_date: Option<(i32, u8, u8)>,
    pub end_date: Option<(i32, u8, u8)>,
    pub recurrence_rule: Option<String>,
}

impl CalendarEvent {
    pub fn timed(uid: &str, summary: &str, start: i64, end: i64) -> Self {
        Self {
            uid: uid.to_owned(),
            summary: summary.to_owned(),
            status: "CONFIRMED".to_owned(),
            all_day: false,
            start_unix_seconds: Some(start),
            end_unix_seconds: Some(end),
            start_date: None,
            end_date: None,
            recurrence_rule: None,
        }
    }
}

unsafe extern "C" {
    fn guion_ical_parse(
        source: *const std::ffi::c_char,
        error: *mut std::ffi::c_char,
        error_capacity: usize,
        error_code: *mut i32,
    ) -> *mut std::ffi::c_void;
    fn guion_ical_event_count(calendar: *mut std::ffi::c_void) -> usize;
    fn guion_ical_event(
        calendar: *mut std::ffi::c_void,
        index: usize,
        uid: *mut *const std::ffi::c_char,
        summary: *mut *const std::ffi::c_char,
        status: *mut *const std::ffi::c_char,
        rrule: *mut *mut std::ffi::c_char,
        all_day: *mut i32,
        start_seconds: *mut i64,
        end_seconds: *mut i64,
        start_year: *mut i32,
        start_month: *mut i32,
        start_day: *mut i32,
        end_year: *mut i32,
        end_month: *mut i32,
        end_day: *mut i32,
        error: *mut std::ffi::c_char,
        error_capacity: usize,
    ) -> i32;
    fn guion_ical_free_text(text: *mut std::ffi::c_char);
    fn guion_ical_free(calendar: *mut std::ffi::c_void);
}

struct IcalCalendar(std::ptr::NonNull<std::ffi::c_void>);
impl Drop for IcalCalendar {
    fn drop(&mut self) {
        unsafe { guion_ical_free(self.0.as_ptr()) };
    }
}

struct IcalText(*mut std::ffi::c_char);
impl Drop for IcalText {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { guion_ical_free_text(self.0) };
        }
    }
}

pub fn calendar_parse(source: &str) -> Result<Vec<CalendarEvent>, ServiceError> {
    if source.len() > 16 * 1024 * 1024 {
        return Err(ServiceError::new(
            "E_ICAL_LIMIT",
            "calendar exceeds the configured input limit",
        ));
    }
    let input = std::ffi::CString::new(source)
        .map_err(|_| ServiceError::new("E_ICAL_PARSE", "calendar contains a NUL byte"))?;
    let mut error = [0 as std::ffi::c_char; 256];
    let mut error_code = 0;
    let calendar = unsafe {
        guion_ical_parse(
            input.as_ptr(),
            error.as_mut_ptr(),
            error.len(),
            &mut error_code,
        )
    };
    let calendar = std::ptr::NonNull::new(calendar).ok_or_else(|| {
        let message = unsafe { std::ffi::CStr::from_ptr(error.as_ptr()) }.to_string_lossy();
        let code = match error_code {
            2 => "E_ICAL_LIMIT",
            _ => "E_ICAL_PARSE",
        };
        ServiceError::new(code, message.into_owned())
    })?;
    let calendar = IcalCalendar(calendar);
    let count = unsafe { guion_ical_event_count(calendar.0.as_ptr()) };
    let mut events = Vec::with_capacity(count);
    for index in 0..count {
        let mut uid = std::ptr::null();
        let mut summary = std::ptr::null();
        let mut status = std::ptr::null();
        let mut recurrence = std::ptr::null_mut();
        let mut all_day = 0;
        let mut start_seconds = 0i64;
        let mut end_seconds = 0i64;
        let mut start_year = 0;
        let mut start_month = 0;
        let mut start_day = 0;
        let mut end_year = 0;
        let mut end_month = 0;
        let mut end_day = 0;
        error.fill(0);
        let result = unsafe {
            guion_ical_event(
                calendar.0.as_ptr(),
                index,
                &mut uid,
                &mut summary,
                &mut status,
                &mut recurrence,
                &mut all_day,
                &mut start_seconds,
                &mut end_seconds,
                &mut start_year,
                &mut start_month,
                &mut start_day,
                &mut end_year,
                &mut end_month,
                &mut end_day,
                error.as_mut_ptr(),
                error.len(),
            )
        };
        let recurrence_guard = IcalText(recurrence);
        if result != 0 {
            let message = unsafe { std::ffi::CStr::from_ptr(error.as_ptr()) }
                .to_string_lossy()
                .into_owned();
            let code = if message.contains("UID") {
                "E_ICAL_UID"
            } else if message.contains("floating") {
                "E_ICAL_FLOATING"
            } else if message.contains("unknown time zone") {
                "E_ICAL_TZID"
            } else if message.contains("precedes") {
                "E_ICAL_RANGE"
            } else if message.contains("time") || message.contains("date") {
                "E_ICAL_DATE"
            } else {
                "E_ICAL_EVENT"
            };
            return Err(ServiceError::new(code, message));
        }
        let text = |value: *const std::ffi::c_char, field: &str| -> Result<String, ServiceError> {
            if value.is_null() {
                return Err(ServiceError::new(
                    "E_ICAL_EVENT",
                    format!("VEVENT {field} is unavailable"),
                ));
            }
            unsafe { std::ffi::CStr::from_ptr(value) }
                .to_str()
                .map(str::to_owned)
                .map_err(|_| {
                    ServiceError::new("E_ICAL_UTF8", format!("VEVENT {field} is not valid UTF-8"))
                })
        };
        let recurrence_rule = if recurrence_guard.0.is_null() {
            None
        } else {
            Some(
                unsafe { std::ffi::CStr::from_ptr(recurrence_guard.0) }
                    .to_str()
                    .map(str::to_owned)
                    .map_err(|_| {
                        ServiceError::new("E_ICAL_UTF8", "VEVENT RRULE is not valid UTF-8")
                    })?,
            )
        };
        let is_all_day = all_day != 0;
        let (start, end, start_date, end_date) = if is_all_day {
            (
                None,
                None,
                Some((start_year, start_month as u8, start_day as u8)),
                Some((end_year, end_month as u8, end_day as u8)),
            )
        } else {
            (Some(start_seconds), Some(end_seconds), None, None)
        };
        events.push(CalendarEvent {
            uid: text(uid, "UID")?,
            summary: text(summary, "SUMMARY")?,
            status: text(status, "STATUS")?,
            all_day: is_all_day,
            start_unix_seconds: start,
            end_unix_seconds: end,
            start_date,
            end_date,
            recurrence_rule,
        });
    }
    Ok(events)
}
