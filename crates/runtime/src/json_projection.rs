use std::{
    borrow::Cow,
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
};

use guion_model::JsonErrorValue;
use serde::de::{DeserializeSeed, Error as DeError, MapAccess, SeqAccess, Visitor};

use crate::{
    duplicate_json_key, escape_reserved_json_keys, restore_reserved_json_keys, source_position,
    UniqueJsonSeed,
};

const PRIVATE_NUMBER_KEY: &str = "$serde_json::private::Number";
const INLINE_OBJECT_KEYS: usize = 4;

struct JsonKey<'de>(Cow<'de, str>);

impl<'de> serde::Deserialize<'de> for JsonKey<'de> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct KeyVisitor;

        impl<'de> Visitor<'de> for KeyVisitor {
            type Value = JsonKey<'de>;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a JSON object key")
            }

            fn visit_borrowed_str<E: DeError>(self, value: &'de str) -> Result<Self::Value, E> {
                Ok(JsonKey(Cow::Borrowed(value)))
            }

            fn visit_str<E: DeError>(self, value: &str) -> Result<Self::Value, E> {
                Ok(JsonKey(Cow::Owned(value.to_owned())))
            }

            fn visit_string<E: DeError>(self, value: String) -> Result<Self::Value, E> {
                Ok(JsonKey(Cow::Owned(value)))
            }
        }

        deserializer.deserialize_str(KeyVisitor)
    }
}

struct SeenKeys<'de> {
    inline: [Option<Cow<'de, str>>; INLINE_OBJECT_KEYS],
    inline_len: usize,
    overflow: Option<HashSet<Cow<'de, str>>>,
}

impl<'de> SeenKeys<'de> {
    fn new() -> Self {
        Self {
            inline: std::array::from_fn(|_| None),
            inline_len: 0,
            overflow: None,
        }
    }

    fn insert(&mut self, key: Cow<'de, str>) -> bool {
        if let Some(overflow) = &mut self.overflow {
            return overflow.insert(key);
        }
        if self.inline[..self.inline_len]
            .iter()
            .flatten()
            .any(|existing| existing.as_ref() == key.as_ref())
        {
            return false;
        }
        if self.inline_len < INLINE_OBJECT_KEYS {
            self.inline[self.inline_len] = Some(key);
            self.inline_len += 1;
            return true;
        }

        let mut overflow = HashSet::with_capacity(INLINE_OBJECT_KEYS * 2);
        for existing in &mut self.inline {
            if let Some(existing) = existing.take() {
                overflow.insert(existing);
            }
        }
        let inserted = overflow.insert(key);
        self.overflow = Some(overflow);
        inserted
    }
}

#[derive(Clone)]
enum Mode {
    SearchPath(usize),
    Rows,
    Row,
    Ignore,
}

struct State {
    path: Vec<String>,
    fields: Vec<String>,
    field_indices: HashMap<String, usize>,
    columns: Vec<Vec<serde_json::Value>>,
    row_values: Vec<Option<serde_json::Value>>,
    found_rows: bool,
    items: usize,
    max_items: usize,
    max_depth: usize,
    max_integer_digits: usize,
    reserved_key: Option<String>,
    duplicate: Rc<RefCell<Option<String>>>,
    fault: Option<(String, String)>,
}

impl State {
    fn fail<E: DeError>(&mut self, code: &str, message: &str) -> E {
        self.fault = Some((code.to_owned(), message.to_owned()));
        E::custom(message.to_owned())
    }

    fn add_items<E: DeError>(&mut self, count: usize) -> Result<(), E> {
        self.items = self.items.saturating_add(count);
        if self.items > self.max_items {
            return Err(self.fail(
                "R_COLLECTION_LIMIT",
                "JSON collection exceeds the configured item limit",
            ));
        }
        Ok(())
    }

    fn check_depth<E: DeError>(&mut self, depth: usize) -> Result<(), E> {
        if depth > self.max_depth {
            return Err(self.fail("R_JSON_DEPTH", "JSON nesting exceeds 64 levels"));
        }
        Ok(())
    }

    fn check_integer<E: DeError>(&mut self, digits: usize) -> Result<(), E> {
        if digits > self.max_integer_digits {
            return Err(self.fail(
                "R_INTEGER_LIMIT",
                "JSON integer exceeds the configured digit limit",
            ));
        }
        Ok(())
    }
}

pub(crate) fn project_columns(
    source: &str,
    path: Vec<String>,
    fields: Vec<String>,
    max_items: usize,
    max_integer_digits: usize,
) -> Result<serde_json::Value, JsonErrorValue> {
    let (safe_source, reserved_key) = escape_reserved_json_keys(source);
    let search_path = path
        .iter()
        .map(|part| {
            if part == PRIVATE_NUMBER_KEY {
                reserved_key.clone().unwrap_or_else(|| part.clone())
            } else {
                part.clone()
            }
        })
        .collect::<Vec<_>>();
    let mut field_indices = HashMap::with_capacity(fields.len());
    for (index, field) in fields.iter().enumerate() {
        let source_field = if field == PRIVATE_NUMBER_KEY {
            reserved_key.clone().unwrap_or_else(|| field.clone())
        } else {
            field.clone()
        };
        if field_indices.insert(source_field, index).is_some() {
            return Err(json_error(
                "R_JSON_FIELD",
                format!("JSON projection repeats field {field:?}"),
                1,
                1,
            ));
        }
    }
    let columns = (0..fields.len()).map(|_| Vec::new()).collect();
    let row_values = std::iter::repeat_with(|| None).take(fields.len()).collect();
    let duplicate = Rc::new(RefCell::new(None));
    let state = Rc::new(RefCell::new(State {
        path: search_path,
        fields,
        field_indices,
        columns,
        row_values,
        found_rows: false,
        items: 0,
        max_items,
        max_depth: 64,
        max_integer_digits,
        reserved_key: reserved_key.clone(),
        duplicate: Rc::clone(&duplicate),
        fault: None,
    }));
    let mut deserializer = serde_json::Deserializer::from_str(&safe_source);
    let mode = if path.is_empty() {
        Mode::Rows
    } else {
        Mode::SearchPath(0)
    };
    let parsed = ProjectSeed {
        state: Rc::clone(&state),
        mode,
        depth: 1,
    }
    .deserialize(&mut deserializer);

    if let Err(error) = parsed {
        if let Some(key) = duplicate.borrow_mut().take() {
            let offset = duplicate_json_key(source)
                .map(|(_, offset)| offset)
                .unwrap_or(source.len());
            let (line, column) = source_position(source, offset);
            let key = if reserved_key.as_deref() == Some(key.as_str()) {
                PRIVATE_NUMBER_KEY.to_owned()
            } else {
                key
            };
            return Err(json_error(
                "R_JSON_DUPLICATE_KEY",
                format!("JSON object contains duplicate key `{key}`"),
                line,
                column,
            ));
        }
        let (code, message) = state
            .borrow_mut()
            .fault
            .take()
            .unwrap_or_else(|| ("R_JSON_PARSE".to_owned(), error.to_string()));
        return Err(json_error(&code, message, error.line(), error.column()));
    }
    deserializer.end().map_err(|error| {
        json_error(
            "R_JSON_PARSE",
            error.to_string(),
            error.line(),
            error.column(),
        )
    })?;
    if let Some(offset) = oversized_integer_offset(source, max_integer_digits) {
        let (line, column) = source_position(source, offset);
        return Err(json_error(
            "R_INTEGER_LIMIT",
            "JSON integer exceeds the configured digit limit",
            line,
            column,
        ));
    }

    let mut state = state.borrow_mut();
    if !state.found_rows {
        return Err(json_error(
            "R_JSON_FIELD",
            format!("JSON path {:?} was not found", path),
            1,
            1,
        ));
    }
    let mut columns = serde_json::Map::new();
    let fields = state.fields.clone();
    let values_by_column = std::mem::take(&mut state.columns);
    for (field, values) in fields.into_iter().zip(values_by_column) {
        columns.insert(field, serde_json::Value::Array(values));
    }
    Ok(serde_json::Value::Object(columns))
}

fn json_error(
    code: &str,
    message: impl Into<String>,
    line: usize,
    column: usize,
) -> JsonErrorValue {
    JsonErrorValue {
        code: code.to_owned(),
        message: message.into(),
        line,
        column,
    }
}

#[derive(Clone)]
struct ProjectSeed {
    state: Rc<RefCell<State>>,
    mode: Mode,
    depth: usize,
}

impl<'de> DeserializeSeed<'de> for ProjectSeed {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(ProjectVisitor {
            state: self.state,
            mode: self.mode,
            depth: self.depth,
        })
    }
}

struct ProjectVisitor {
    state: Rc<RefCell<State>>,
    mode: Mode,
    depth: usize,
}

impl<'de> Visitor<'de> for ProjectVisitor {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a JSON object containing the requested row array")
    }

    fn visit_bool<E: DeError>(self, _value: bool) -> Result<(), E> {
        self.accept_scalar("JSON projection expected an object or row array")
    }

    fn visit_i64<E: DeError>(self, value: i64) -> Result<(), E> {
        self.accept_integer(value.to_string().trim_start_matches('-').len())
    }

    fn visit_u64<E: DeError>(self, value: u64) -> Result<(), E> {
        self.accept_integer(value.to_string().len())
    }

    fn visit_i128<E: DeError>(self, value: i128) -> Result<(), E> {
        self.accept_integer(value.to_string().trim_start_matches('-').len())
    }

    fn visit_u128<E: DeError>(self, value: u128) -> Result<(), E> {
        self.accept_integer(value.to_string().len())
    }

    fn visit_f64<E: DeError>(self, _value: f64) -> Result<(), E> {
        self.accept_scalar("JSON projection expected an object or row array")
    }

    fn visit_str<E: DeError>(self, _value: &str) -> Result<(), E> {
        self.accept_scalar("JSON projection expected an object or row array")
    }

    fn visit_string<E: DeError>(self, _value: String) -> Result<(), E> {
        self.accept_scalar("JSON projection expected an object or row array")
    }

    fn visit_unit<E: DeError>(self) -> Result<(), E> {
        self.accept_scalar("JSON projection expected an object or row array")
    }

    fn visit_none<E: DeError>(self) -> Result<(), E> {
        self.accept_scalar("JSON projection expected an object or row array")
    }

    fn visit_some<D>(self, deserializer: D) -> Result<(), D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        ProjectSeed {
            state: self.state,
            mode: self.mode,
            depth: self.depth,
        }
        .deserialize(deserializer)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<(), A::Error>
    where
        A: SeqAccess<'de>,
    {
        {
            let mut state = self.state.borrow_mut();
            state.check_depth(self.depth)?;
            if !matches!(self.mode, Mode::Rows | Mode::Ignore) {
                return Err(state.fail(
                    "R_JSON_TYPE",
                    "JSON projection path and rows must have the declared object/array shape",
                ));
            }
            if matches!(self.mode, Mode::Rows) {
                state.found_rows = true;
            }
        }
        let rows = matches!(self.mode, Mode::Rows);
        while let Some(()) = sequence.next_element_seed(ProjectSeed {
            state: Rc::clone(&self.state),
            mode: if rows { Mode::Row } else { Mode::Ignore },
            depth: self.depth + 1,
        })? {
            self.state.borrow_mut().add_items::<A::Error>(1)?;
        }
        Ok(())
    }

    fn visit_map<A>(self, mut map: A) -> Result<(), A::Error>
    where
        A: MapAccess<'de>,
    {
        {
            let mut state = self.state.borrow_mut();
            state.check_depth(self.depth)?;
            if matches!(self.mode, Mode::Rows) {
                return Err(state.fail(
                    "R_JSON_TYPE",
                    "JSON projection path must identify an array of row objects",
                ));
            }
        }
        if matches!(self.mode, Mode::Row) {
            self.state.borrow_mut().row_values.fill(None);
        }
        let Some(first_key) = map.next_key::<JsonKey<'de>>()?.map(|key| key.0) else {
            return if matches!(self.mode, Mode::Row) {
                Err(self
                    .state
                    .borrow_mut()
                    .fail("R_JSON_FIELD", "JSON row is missing projected fields"))
            } else {
                Ok(())
            };
        };
        if first_key.as_ref() == PRIVATE_NUMBER_KEY && !matches!(self.mode, Mode::Ignore) {
            let number = map.next_value::<String>()?;
            let digits = number
                .strip_prefix('-')
                .unwrap_or(&number)
                .bytes()
                .take_while(u8::is_ascii_digit)
                .count();
            self.state.borrow_mut().check_integer::<A::Error>(digits)?;
            return self.accept_scalar("JSON projection expected an object or row array");
        }

        let mut seen = SeenKeys::new();
        let mut key = Some(first_key);
        while let Some(current_key) = key {
            {
                let mut state = self.state.borrow_mut();
                state.add_items::<A::Error>(1)?;
                if !seen.insert(current_key.clone()) {
                    *state.duplicate.borrow_mut() = Some(current_key.to_string());
                    return Err(A::Error::custom("duplicate JSON object key"));
                }
            }
            let child_mode = match self.mode {
                Mode::SearchPath(index) => {
                    let state = self.state.borrow();
                    if state
                        .path
                        .get(index)
                        .is_some_and(|part| part == current_key.as_ref())
                    {
                        if index + 1 == state.path.len() {
                            Mode::Rows
                        } else {
                            Mode::SearchPath(index + 1)
                        }
                    } else {
                        Mode::Ignore
                    }
                }
                Mode::Row => {
                    let selected = self
                        .state
                        .borrow()
                        .field_indices
                        .contains_key(current_key.as_ref());
                    if selected {
                        let duplicate = Rc::clone(&self.state.borrow().duplicate);
                        let mut value = map.next_value_seed(UniqueJsonSeed { duplicate })?;
                        if let Some(reserved_key) = self.state.borrow().reserved_key.clone() {
                            restore_reserved_json_keys(&mut value, &reserved_key);
                        }
                        let (index, depth, max_integer_digits) = {
                            let state = self.state.borrow();
                            (
                                state.field_indices[current_key.as_ref()],
                                self.depth,
                                state.max_integer_digits,
                            )
                        };
                        let value_metrics = metrics(&value);
                        if depth.saturating_add(value_metrics.depth) > 64 {
                            return Err(self
                                .state
                                .borrow_mut()
                                .fail("R_JSON_DEPTH", "JSON nesting exceeds 64 levels"));
                        }
                        if value_metrics
                            .oversized_integer_digits
                            .is_some_and(|digits| digits > max_integer_digits)
                        {
                            return Err(self.state.borrow_mut().fail(
                                "R_INTEGER_LIMIT",
                                "JSON integer exceeds the configured digit limit",
                            ));
                        }
                        self.state
                            .borrow_mut()
                            .add_items::<A::Error>(value_metrics.item_count)?;
                        self.state.borrow_mut().row_values[index] = Some(value);
                        key = map.next_key::<JsonKey<'de>>()?.map(|key| key.0);
                        continue;
                    }
                    Mode::Ignore
                }
                Mode::Ignore => Mode::Ignore,
                Mode::Rows => unreachable!(),
            };
            map.next_value_seed(ProjectSeed {
                state: Rc::clone(&self.state),
                mode: child_mode,
                depth: self.depth + 1,
            })?;
            key = map.next_key::<JsonKey<'de>>()?.map(|key| key.0);
        }
        if matches!(self.mode, Mode::Row) {
            let mut state = self.state.borrow_mut();
            if let Some(missing) = state.row_values.iter().position(Option::is_none) {
                let field = state.fields[missing].clone();
                return Err(state.fail("R_JSON_FIELD", &format!("JSON row has no field {field:?}")));
            }
            for index in 0..state.row_values.len() {
                let value = state.row_values[index].take().expect("validated row field");
                state.columns[index].push(value);
            }
        }
        Ok(())
    }
}

fn oversized_integer_offset(source: &str, max_digits: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut offset = 0;
    let mut in_string = false;
    let mut escaped = false;
    while offset < bytes.len() {
        let byte = bytes[offset];
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            offset += 1;
            continue;
        }
        if byte == b'"' {
            in_string = true;
            offset += 1;
            continue;
        }
        if byte == b'-' || byte.is_ascii_digit() {
            let start = offset;
            if byte == b'-' {
                offset += 1;
            }
            let digit_start = offset;
            while offset < bytes.len() && bytes[offset].is_ascii_digit() {
                offset += 1;
            }
            let integer_end = offset;
            while offset < bytes.len()
                && !matches!(
                    bytes[offset],
                    b',' | b']' | b'}' | b' ' | b'\n' | b'\r' | b'\t'
                )
            {
                offset += 1;
            }
            let token = &bytes[start..offset];
            if !token.iter().any(|byte| matches!(byte, b'.' | b'e' | b'E'))
                && integer_end.saturating_sub(digit_start) > max_digits
            {
                return Some(start);
            }
            continue;
        }
        offset += 1;
    }
    None
}

impl ProjectVisitor {
    fn accept_scalar<E: DeError>(self, message: &str) -> Result<(), E> {
        if matches!(self.mode, Mode::Ignore) {
            Ok(())
        } else {
            self.wrong_shape(message)
        }
    }

    fn accept_integer<E: DeError>(self, digits: usize) -> Result<(), E> {
        self.state.borrow_mut().check_integer::<E>(digits)?;
        self.accept_scalar("JSON projection expected an object or row array")
    }

    fn wrong_shape<E: DeError>(self, message: &str) -> Result<(), E> {
        Err(self.state.borrow_mut().fail("R_JSON_TYPE", message))
    }
}

struct Metrics {
    depth: usize,
    item_count: usize,
    oversized_integer_digits: Option<usize>,
}

fn metrics(value: &serde_json::Value) -> Metrics {
    use serde_json::Value as V;
    match value {
        V::Null | V::Bool(_) | V::String(_) => Metrics {
            depth: 0,
            item_count: 0,
            oversized_integer_digits: None,
        },
        V::Number(number) => {
            let text = number.to_string();
            let digits = text.strip_prefix('-').unwrap_or(&text);
            let integer = !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit());
            Metrics {
                depth: 0,
                item_count: 0,
                oversized_integer_digits: integer.then_some(digits.len()),
            }
        }
        V::Array(values) => {
            let mut result = Metrics {
                depth: 1,
                item_count: values.len(),
                oversized_integer_digits: None,
            };
            for value in values {
                let child = metrics(value);
                result.depth = result.depth.max(child.depth + 1);
                result.item_count = result.item_count.saturating_add(child.item_count);
                result.oversized_integer_digits = result
                    .oversized_integer_digits
                    .max(child.oversized_integer_digits);
            }
            result
        }
        V::Object(values) => {
            let mut result = Metrics {
                depth: 1,
                item_count: values.len(),
                oversized_integer_digits: None,
            };
            for value in values.values() {
                let child = metrics(value);
                result.depth = result.depth.max(child.depth + 1);
                result.item_count = result.item_count.saturating_add(child.item_count);
                result.oversized_integer_digits = result
                    .oversized_integer_digits
                    .max(child.oversized_integer_digits);
            }
            result
        }
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::{project_columns, JsonKey, SeenKeys};

    #[test]
    fn plain_object_keys_borrow_the_source_and_escaped_keys_remain_exact() {
        let plain: JsonKey<'_> = serde_json::from_str("\"team\"").unwrap();
        assert!(matches!(plain.0, Cow::Borrowed("team")));

        let escaped: JsonKey<'_> = serde_json::from_str("\"te\\u0061m\"").unwrap();
        assert_eq!(escaped.0, "team");
    }

    #[test]
    fn inline_and_spilled_object_key_sets_reject_duplicates() {
        let mut keys = SeenKeys::new();
        for key in ["id", "team", "priority", "status"] {
            assert!(keys.insert(Cow::Borrowed(key)));
        }
        assert!(!keys.insert(Cow::Borrowed("team")));

        assert!(keys.insert(Cow::Borrowed("created_at")));
        assert!(keys.insert(Cow::Borrowed("metadata")));
        assert!(!keys.insert(Cow::Borrowed("metadata")));
    }

    #[test]
    fn reused_row_buffer_keeps_column_values_aligned_when_keys_are_reordered() {
        let columns = project_columns(
            r#"{"rows":[{"n":1,"kind":"first"},{"kind":"second","n":2}]}"#,
            vec!["rows".to_owned()],
            vec!["n".to_owned(), "kind".to_owned()],
            100,
            100,
        )
        .unwrap();

        assert_eq!(
            columns,
            serde_json::json!({"n": [1, 2], "kind": ["first", "second"]})
        );
    }

    #[test]
    fn reused_row_buffer_does_not_hide_a_missing_field_in_a_later_row() {
        let error = project_columns(
            r#"{"rows":[{"n":1,"kind":"first"},{"kind":"second"}]}"#,
            vec!["rows".to_owned()],
            vec!["n".to_owned(), "kind".to_owned()],
            100,
            100,
        )
        .unwrap_err();

        assert_eq!(error.code, "R_JSON_FIELD");
    }
}
