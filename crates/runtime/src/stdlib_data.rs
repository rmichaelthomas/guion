//! Portable, deterministic pieces of the data standard library.
#[path = "resources.rs"]
pub mod resources;

use guion_model::{BigInt, JsonErrorValue, SharedJson};
use std::sync::Arc;
use std::{cmp::Ordering, collections::BTreeMap, path::Path};

pub use resources::{
    CsvError, CsvRow, FileReaderHandle, FileReaderRegistry, ReaderError, ReaderErrorKind,
    SourceLine,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataError {
    pub code: &'static str,
    pub message: String,
}

impl DataError {
    fn invalid(kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            code: kind,
            message: message.into(),
        }
    }
}

pub type JsonTryError = JsonErrorValue;

pub fn json_try_get(value: &Arc<SharedJson>, key: &str) -> Result<Arc<SharedJson>, JsonTryError> {
    let Some(object) = value.as_object() else {
        return Err(json_error("E_JSON_SHAPE", "expected a JSON object"));
    };
    object
        .get(key)
        .cloned()
        .ok_or_else(|| json_error("E_JSON_MISSING", format!("JSON object has no key `{key}`")))
}

pub fn json_try_index(
    value: &Arc<SharedJson>,
    index: i64,
) -> Result<Arc<SharedJson>, JsonTryError> {
    let Some(array) = value.as_array() else {
        return Err(json_error("E_JSON_SHAPE", "expected a JSON array"));
    };
    if index < 0 {
        return Err(json_error(
            "E_JSON_INDEX",
            "JSON array index must not be negative",
        ));
    }
    array
        .get(index as usize)
        .cloned()
        .ok_or_else(|| json_error("E_JSON_INDEX", "JSON array index is out of range"))
}

pub fn json_try_text(value: &SharedJson) -> Result<String, JsonTryError> {
    value
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| json_error("E_JSON_TYPE", "expected a JSON string"))
}

pub fn json_try_int(value: &SharedJson) -> Result<BigInt, JsonTryError> {
    let number = value
        .as_number()
        .ok_or_else(|| json_error("E_JSON_TYPE", "expected a JSON integer"))?;
    if let Some(value) = number.as_i64() {
        return Ok(BigInt::from_i64(value));
    }
    if let Some(value) = number.as_u64() {
        return Ok(BigInt::from_u64(value));
    }
    BigInt::parse(&number.to_string())
        .map_err(|_| json_error("E_JSON_TYPE", "JSON number is not an exact integer"))
}

pub fn json_try_bool(value: &SharedJson) -> Result<bool, JsonTryError> {
    value
        .as_bool()
        .ok_or_else(|| json_error("E_JSON_TYPE", "expected a JSON boolean"))
}

pub fn json_try_float(value: &SharedJson) -> Result<f64, JsonTryError> {
    let number = value
        .as_number()
        .ok_or_else(|| json_error("E_JSON_TYPE", "expected a JSON number"))?;
    number
        .as_f64()
        .filter(|value| value.is_finite())
        .ok_or_else(|| json_error("E_JSON_TYPE", "JSON number is outside finite Float64 range"))
}

fn json_error(code: &str, message: impl Into<String>) -> JsonTryError {
    JsonTryError {
        code: code.into(),
        message: message.into(),
        line: 0,
        column: 0,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Date {
    pub year: i32,
    pub month: u8,
    pub day: u8,
}

impl Date {
    pub fn parse(text: &str) -> Result<Self, DataError> {
        if text.len() != 10
            || !text.is_ascii()
            || text.as_bytes().get(4) != Some(&b'-')
            || text.as_bytes().get(7) != Some(&b'-')
        {
            return Err(DataError::invalid(
                "E_DATE_FORMAT",
                "date must use YYYY-MM-DD",
            ));
        }
        let year = decimal(&text[0..4])
            .ok_or_else(|| DataError::invalid("E_DATE_FORMAT", "year must contain four digits"))?
            as i32;
        let month = decimal(&text[5..7])
            .ok_or_else(|| DataError::invalid("E_DATE_FORMAT", "month must contain two digits"))?
            as u8;
        let day = decimal(&text[8..10])
            .ok_or_else(|| DataError::invalid("E_DATE_FORMAT", "day must contain two digits"))?
            as u8;
        let maximum = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if is_leap_year(year) => 29,
            2 => 28,
            _ => 0,
        };
        if day == 0 || day > maximum {
            return Err(DataError::invalid(
                "E_DATE_RANGE",
                "date is not a valid Gregorian calendar day",
            ));
        }
        Ok(Self { year, month, day })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Instant {
    pub unix_microseconds: i64,
}

pub fn parse_timestamp(text: &str) -> Result<i64, DataError> {
    if text.len() < 20 || text.as_bytes().get(10) != Some(&b'T') {
        return Err(DataError::invalid(
            "E_TIMESTAMP_FORMAT",
            "timestamp must use YYYY-MM-DDTHH:MM:SS with an explicit offset",
        ));
    }
    let date = Date::parse(&text[..10])?;
    let bytes = text.as_bytes();
    if bytes.get(13) != Some(&b':') || bytes.get(16) != Some(&b':') {
        return Err(DataError::invalid(
            "E_TIMESTAMP_FORMAT",
            "timestamp time must use HH:MM:SS",
        ));
    }
    let hour = decimal(&text[11..13])
        .ok_or_else(|| DataError::invalid("E_TIMESTAMP_FORMAT", "hour must contain two digits"))?;
    let minute = decimal(&text[14..16]).ok_or_else(|| {
        DataError::invalid("E_TIMESTAMP_FORMAT", "minute must contain two digits")
    })?;
    let second = decimal(&text[17..19]).ok_or_else(|| {
        DataError::invalid("E_TIMESTAMP_FORMAT", "second must contain two digits")
    })?;
    if hour > 23 || minute > 59 || second > 59 {
        return Err(DataError::invalid(
            "E_TIMESTAMP_RANGE",
            "time is outside the supported clock range",
        ));
    }
    let mut cursor = 19usize;
    let mut micros = 0i64;
    if bytes.get(cursor) == Some(&b'.') {
        cursor += 1;
        let start = cursor;
        while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
            cursor += 1;
        }
        let digits = cursor - start;
        if !(1..=6).contains(&digits) {
            return Err(DataError::invalid(
                "E_TIMESTAMP_PRECISION",
                "fraction must contain one to six digits",
            ));
        }
        let value = decimal(&text[start..cursor]).unwrap() as i64;
        micros = value * 10i64.pow((6 - digits) as u32);
    }
    let offset_seconds: i64 = match bytes.get(cursor) {
        Some(b'Z') if cursor + 1 == bytes.len() => 0,
        Some(sign @ (b'+' | b'-'))
            if cursor + 6 == bytes.len() && bytes.get(cursor + 3) == Some(&b':') =>
        {
            let hours = decimal(&text[cursor + 1..cursor + 3]).ok_or_else(|| {
                DataError::invalid("E_TIMESTAMP_OFFSET", "offset hour must be two digits")
            })?;
            let minutes = decimal(&text[cursor + 4..cursor + 6]).ok_or_else(|| {
                DataError::invalid("E_TIMESTAMP_OFFSET", "offset minute must be two digits")
            })?;
            if hours > 23 || minutes > 59 {
                return Err(DataError::invalid(
                    "E_TIMESTAMP_OFFSET",
                    "offset is outside its valid range",
                ));
            }
            let magnitude = (hours * 60 + minutes) as i64 * 60;
            if *sign == b'+' {
                magnitude
            } else {
                -magnitude
            }
        }
        _ => {
            return Err(DataError::invalid(
                "E_TIMESTAMP_OFFSET",
                "timestamp must end in Z or a signed HH:MM offset",
            ))
        }
    };
    let days = days_from_civil(date.year, date.month as u32, date.day as u32);
    let local_seconds = days * 86_400 + (hour as i64) * 3600 + (minute as i64) * 60 + second as i64;
    Ok((local_seconds - offset_seconds) * 1_000_000 + micros)
}

pub fn format_utc(instant: Instant) -> String {
    let seconds = instant.unix_microseconds.div_euclid(1_000_000);
    let micros = instant.unix_microseconds.rem_euclid(1_000_000);
    let days = seconds.div_euclid(86_400);
    let day_seconds = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = day_seconds / 3600;
    let minute = day_seconds % 3600 / 60;
    let second = day_seconds % 60;
    let fraction = if micros == 0 {
        String::new()
    } else {
        let mut digits = format!("{micros:06}");
        while digits.ends_with('0') {
            digits.pop();
        }
        format!(".{digits}")
    };
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}{fraction}Z")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemVer {
    pub major: String,
    pub minor: String,
    pub patch: String,
    pub prerelease: Vec<String>,
    pub build: Vec<String>,
    pub original: String,
}

pub fn parse_semver(text: &str) -> Result<SemVer, DataError> {
    if text.len() > 1024 * 1024 {
        return Err(DataError::invalid(
            "E_SEMVER_LIMIT",
            "version text is too long",
        ));
    }
    let original = text.to_owned();
    let (core_pre, build) = match text.split_once('+') {
        Some((before, after)) if !after.is_empty() && !after.contains('+') => {
            (before, split_identifiers(after, "build")?)
        }
        Some(_) => {
            return Err(DataError::invalid(
                "E_SEMVER_BUILD",
                "build metadata must contain valid identifiers",
            ))
        }
        None => (text, Vec::new()),
    };
    let (core, prerelease) = match core_pre.split_once('-') {
        Some((before, after)) if !after.is_empty() => {
            (before, split_identifiers(after, "prerelease")?)
        }
        Some(_) => {
            return Err(DataError::invalid(
                "E_SEMVER_PRERELEASE",
                "prerelease must contain valid identifiers",
            ))
        }
        None => (core_pre, Vec::new()),
    };
    let pieces = core.split('.').collect::<Vec<_>>();
    if pieces.len() != 3 || pieces.iter().any(|piece| !valid_numeric_component(piece)) {
        return Err(DataError::invalid(
            "E_SEMVER_CORE",
            "version core must contain three canonical nonnegative integers",
        ));
    }
    for identifier in &prerelease {
        if identifier.bytes().all(|byte| byte.is_ascii_digit())
            && identifier.len() > 1
            && identifier.starts_with('0')
        {
            return Err(DataError::invalid(
                "E_SEMVER_PRERELEASE",
                "numeric prerelease identifiers cannot have leading zeroes",
            ));
        }
    }
    Ok(SemVer {
        major: pieces[0].to_owned(),
        minor: pieces[1].to_owned(),
        patch: pieces[2].to_owned(),
        prerelease,
        build,
        original,
    })
}

fn split_identifiers(value: &str, part: &str) -> Result<Vec<String>, DataError> {
    let identifiers = value.split('.').collect::<Vec<_>>();
    if identifiers.is_empty()
        || identifiers.iter().any(|identifier| {
            identifier.is_empty()
                || !identifier
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
    {
        return Err(DataError::invalid(
            "E_SEMVER_IDENTIFIER",
            format!("{part} identifiers must be nonempty ASCII alphanumeric or hyphen"),
        ));
    }
    Ok(identifiers.into_iter().map(str::to_owned).collect())
}

fn valid_numeric_component(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && (value == "0" || !value.starts_with('0'))
}

pub fn compare_semver(left: &SemVer, right: &SemVer) -> Ordering {
    compare_numeric(&left.major, &right.major)
        .then_with(|| compare_numeric(&left.minor, &right.minor))
        .then_with(|| compare_numeric(&left.patch, &right.patch))
        .then_with(|| compare_prerelease(&left.prerelease, &right.prerelease))
}

fn compare_prerelease(left: &[String], right: &[String]) -> Ordering {
    match (left.is_empty(), right.is_empty()) {
        (true, true) => return Ordering::Equal,
        (true, false) => return Ordering::Greater,
        (false, true) => return Ordering::Less,
        _ => {}
    }
    for (left, right) in left.iter().zip(right) {
        let left_numeric = left.bytes().all(|byte| byte.is_ascii_digit());
        let right_numeric = right.bytes().all(|byte| byte.is_ascii_digit());
        let order = match (left_numeric, right_numeric) {
            (true, true) => compare_numeric(left, right),
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            (false, false) => left.cmp(right),
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    left.len().cmp(&right.len())
}

fn compare_numeric(left: &str, right: &str) -> Ordering {
    left.len().cmp(&right.len()).then_with(|| left.cmp(right))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CliOption {
    pub name: String,
    pub takes_value: bool,
    pub repeatable: bool,
    pub required: bool,
}

impl CliOption {
    pub fn new(
        name: impl Into<String>,
        takes_value: bool,
        repeatable: bool,
        required: bool,
    ) -> Self {
        Self {
            name: name.into(),
            takes_value,
            repeatable,
            required,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CliArguments {
    pub values: BTreeMap<String, Vec<String>>,
    pub positionals: Vec<String>,
}
pub type TextOptionMap = CliArguments;

impl CliArguments {
    pub fn parse(arguments: &[String], options: &[CliOption]) -> Result<Self, DataError> {
        let mut definitions = BTreeMap::new();
        for option in options {
            if option.name.is_empty()
                || option.name.starts_with('-')
                || definitions.insert(option.name.as_str(), option).is_some()
            {
                return Err(DataError::invalid(
                    "E_CLI_SCHEMA",
                    "CLI option names must be nonempty and unique",
                ));
            }
        }
        let mut values: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut positionals = Vec::new();
        let mut parse_options = true;
        let mut cursor = 0usize;
        while cursor < arguments.len() {
            let argument = &arguments[cursor];
            if parse_options && argument == "--" {
                parse_options = false;
                cursor += 1;
                continue;
            }
            if parse_options && argument.starts_with("--") {
                let option_text = &argument[2..];
                let (name, inline_value) = option_text
                    .split_once('=')
                    .map_or((option_text, None), |(name, value)| (name, Some(value)));
                let definition = definitions.get(name).ok_or_else(|| {
                    DataError::invalid("E_CLI_UNKNOWN", format!("unknown option `--{name}`"))
                })?;
                if values.contains_key(name) && !definition.repeatable {
                    return Err(DataError::invalid(
                        "E_CLI_DUPLICATE",
                        format!("option `--{name}` may only be supplied once"),
                    ));
                }
                let value = if definition.takes_value {
                    if let Some(value) = inline_value {
                        value.to_owned()
                    } else {
                        cursor += 1;
                        let Some(value) = arguments.get(cursor) else {
                            return Err(DataError::invalid(
                                "E_CLI_VALUE",
                                format!("option `--{name}` requires a value"),
                            ));
                        };
                        if value == "--" || (value.starts_with('-') && value != "-") {
                            return Err(DataError::invalid(
                                "E_CLI_VALUE",
                                format!("option `--{name}` requires a value"),
                            ));
                        }
                        value.clone()
                    }
                } else {
                    if inline_value.is_some() {
                        return Err(DataError::invalid(
                            "E_CLI_VALUE",
                            format!("option `--{name}` does not take a value"),
                        ));
                    }
                    "true".to_owned()
                };
                values.entry(name.to_owned()).or_default().push(value);
            } else {
                positionals.push(argument.clone());
            }
            cursor += 1;
        }
        for option in options {
            if option.required && !values.contains_key(&option.name) {
                return Err(DataError::invalid(
                    "E_CLI_REQUIRED",
                    format!("required option `--{}` is missing", option.name),
                ));
            }
        }
        Ok(Self {
            values,
            positionals,
        })
    }
}

pub fn text_compare_scalar(left: &str, right: &str) -> Ordering {
    left.chars().cmp(right.chars())
}
pub fn text_compare_bytes(left: &str, right: &str) -> Ordering {
    left.as_bytes().cmp(right.as_bytes())
}
pub fn text_split(value: &str, delimiter: &str) -> Result<Vec<String>, DataError> {
    if delimiter.is_empty() {
        return Err(DataError::invalid(
            "E_TEXT_DELIMITER",
            "split delimiter must not be empty",
        ));
    }
    Ok(value.split(delimiter).map(str::to_owned).collect())
}
pub fn text_join(values: &[String], delimiter: &str) -> String {
    values.join(delimiter)
}
pub fn text_replace(value: &str, from: &str, to: &str) -> String {
    value.replace(from, to)
}
pub fn text_trim(value: &str) -> String {
    value.trim().to_owned()
}
pub fn text_parse_int(value: &str) -> Result<BigInt, DataError> {
    let digits = value
        .strip_prefix('-')
        .or_else(|| value.strip_prefix('+'))
        .unwrap_or(value);
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(DataError::invalid(
            "E_INT_PARSE",
            "expected a signed base-10 integer",
        ));
    }
    BigInt::parse(value).map_err(|_| {
        DataError::invalid(
            "E_INT_PARSE",
            "integer is outside the supported representation",
        )
    })
}
pub fn text_parse_float(value: &str) -> Result<f64, DataError> {
    let parsed = value.parse::<f64>().map_err(|_| {
        DataError::invalid("E_FLOAT_PARSE", "expected a decimal floating-point number")
    })?;
    if parsed.is_finite() {
        Ok(parsed)
    } else {
        Err(DataError::invalid(
            "E_FLOAT_PARSE",
            "floating-point value must be finite",
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectoryEntry {
    pub relative_path: String,
    pub kind: String,
}

pub fn fs_walk(root: &Path, max_entries: usize) -> Result<Vec<DirectoryEntry>, DataError> {
    let root_metadata = std::fs::symlink_metadata(root).map_err(|error| {
        DataError::invalid("E_FS_WALK", format!("cannot inspect walk root: {error}"))
    })?;
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return Err(DataError::invalid(
            "E_FS_WALK_ROOT",
            "walk root must be a directory and cannot be a symlink",
        ));
    }
    let mut directories = vec![(root.to_path_buf(), String::new())];
    let mut entries = Vec::new();
    while let Some((directory, prefix)) = directories.pop() {
        let children = std::fs::read_dir(&directory).map_err(|error| {
            DataError::invalid(
                "E_FS_WALK",
                format!("cannot read directory `{prefix}`: {error}"),
            )
        })?;
        let mut children = children.collect::<Result<Vec<_>, _>>().map_err(|error| {
            DataError::invalid(
                "E_FS_WALK",
                format!("cannot read a directory entry: {error}"),
            )
        })?;
        children.sort_by(|left, right| left.file_name().cmp(&right.file_name()));
        for child in children {
            let name = child.file_name().into_string().map_err(|_| {
                DataError::invalid("E_FS_WALK_UTF8", "walk path is not valid UTF-8")
            })?;
            let relative_path = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            let metadata = std::fs::symlink_metadata(child.path()).map_err(|error| {
                DataError::invalid(
                    "E_FS_WALK",
                    format!("cannot inspect `{relative_path}`: {error}"),
                )
            })?;
            let kind = if metadata.file_type().is_symlink() {
                "symlink"
            } else if metadata.is_dir() {
                "directory"
            } else if metadata.is_file() {
                "file"
            } else {
                "other"
            };
            if entries.len() >= max_entries {
                return Err(DataError::invalid(
                    "E_FS_WALK_LIMIT",
                    "directory traversal exceeds the configured entry limit",
                ));
            }
            entries.push(DirectoryEntry {
                relative_path: relative_path.clone(),
                kind: kind.into(),
            });
            if kind == "directory" {
                directories.push((child.path(), relative_path));
            }
        }
    }
    entries.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(entries)
}

pub fn csv_encode(rows: &[Vec<String>]) -> String {
    let mut output = String::new();
    for (row_index, row) in rows.iter().enumerate() {
        if row_index > 0 {
            output.push('\n');
        }
        for (field_index, field) in row.iter().enumerate() {
            if field_index > 0 {
                output.push(',');
            }
            if field.contains([',', '"', '\r', '\n']) {
                output.push('"');
                for character in field.chars() {
                    output.push(character);
                    if character == '"' {
                        output.push('"');
                    }
                }
                output.push('"');
            } else {
                output.push_str(field);
            }
        }
    }
    output
}

fn decimal(text: &str) -> Option<u32> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.bytes().try_fold(0u32, |value, byte| {
        value.checked_mul(10)?.checked_add((byte - b'0') as u32)
    })
}
fn is_leap_year(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}
fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = year as i64 - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let adjusted_month = month as i64 + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * adjusted_month + 2) / 5 + day as i64 - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month, day)
}
