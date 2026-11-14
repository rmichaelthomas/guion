//! Duplicate-key-aware JSON decoding for the native host.

use super::*;

#[derive(Debug)]
pub(crate) enum JsonDecodeError {
    Parse(serde_json::Error),
    DuplicateKey {
        key: String,
        line: usize,
        column: usize,
    },
}

#[cfg(test)]
pub(crate) fn parse_unique_json(source: &str) -> Result<serde_json::Value, JsonDecodeError> {
    use serde::de::DeserializeSeed;

    let (safe_source, reserved_key) = escape_reserved_json_keys(source);
    let mut duplicate = None;
    let mut deserializer = serde_json::Deserializer::from_str(&safe_source);
    let parsed = UniqueJsonSeed {
        duplicate: &mut duplicate,
    }
    .deserialize(&mut deserializer);
    match parsed {
        Ok(mut value) => {
            deserializer.end().map_err(JsonDecodeError::Parse)?;
            if let Some(reserved_key) = reserved_key.as_deref() {
                restore_reserved_json_keys(&mut value, reserved_key);
            }
            Ok(value)
        }
        Err(error) => {
            if let Some(key) = duplicate.take() {
                let offset = duplicate_json_key(source)
                    .map(|(_, offset)| offset)
                    .unwrap_or(source.len());
                let (line, column) = source_position(source, offset);
                let key = if reserved_key.as_deref() == Some(key.as_str()) {
                    "$serde_json::private::Number".to_owned()
                } else {
                    key
                };
                Err(JsonDecodeError::DuplicateKey { key, line, column })
            } else {
                Err(JsonDecodeError::Parse(error))
            }
        }
    }
}

pub(crate) fn parse_unique_shared_json(source: &str) -> Result<SharedJson, JsonDecodeError> {
    use serde::de::DeserializeSeed;

    let (safe_source, reserved_key) = escape_reserved_json_keys(source);
    let mut duplicate = None;
    let mut deserializer = serde_json::Deserializer::from_str(&safe_source);
    let parsed = SharedJsonSeed {
        duplicate: &mut duplicate,
    }
    .deserialize(&mut deserializer);
    match parsed {
        Ok(mut value) => {
            deserializer.end().map_err(JsonDecodeError::Parse)?;
            if let Some(reserved_key) = reserved_key.as_deref() {
                restore_reserved_shared_json_keys(&mut value, reserved_key);
            }
            Ok(value)
        }
        Err(error) => {
            if let Some(key) = duplicate.take() {
                let offset = duplicate_json_key(source)
                    .map(|(_, offset)| offset)
                    .unwrap_or(source.len());
                let (line, column) = source_position(source, offset);
                let key = if reserved_key.as_deref() == Some(key.as_str()) {
                    "$serde_json::private::Number".to_owned()
                } else {
                    key
                };
                Err(JsonDecodeError::DuplicateKey { key, line, column })
            } else {
                Err(JsonDecodeError::Parse(error))
            }
        }
    }
}

pub(crate) fn restore_reserved_shared_json_keys(value: &mut SharedJson, reserved_key: &str) {
    match value {
        SharedJson::Array(values) => {
            for value in Shared::make_mut(values) {
                restore_reserved_shared_json_keys(Shared::make_mut(value), reserved_key);
            }
        }
        SharedJson::Object(values) => {
            let values = Shared::make_mut(values);
            if let Some(mut value) = values.remove(reserved_key) {
                restore_reserved_shared_json_keys(Shared::make_mut(&mut value), reserved_key);
                values.insert("$serde_json::private::Number".to_owned(), value);
            }
            for value in values.values_mut() {
                restore_reserved_shared_json_keys(Shared::make_mut(value), reserved_key);
            }
        }
        _ => {}
    }
}

pub(crate) fn escape_reserved_json_keys<'a>(source: &'a str) -> (Cow<'a, str>, Option<String>) {
    const RESERVED: &str = "$serde_json::private::Number";
    // The reserved key starts with `$` and can otherwise only appear through an escape.
    if memchr::memchr2(b'$', b'\\', source.as_bytes()).is_none() {
        return (Cow::Borrowed(source), None);
    }
    let bytes = source.as_bytes();
    let mut offset = 0;
    let mut has_reserved_key = false;
    while offset < bytes.len() {
        if bytes[offset] != b'"' {
            offset += 1;
            continue;
        }
        let start = offset;
        offset += 1;
        let mut escaped = false;
        while offset < bytes.len() {
            let byte = bytes[offset];
            offset += 1;
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                break;
            }
        }
        if offset > bytes.len() || bytes.get(offset - 1) != Some(&b'"') {
            break;
        }
        let mut after = offset;
        while bytes
            .get(after)
            .is_some_and(|byte| matches!(byte, b' ' | b'\n' | b'\r' | b'\t'))
        {
            after += 1;
        }
        let token = &source[start + 1..offset - 1];
        let is_reserved = if token.contains('\\') {
            serde_json::from_str::<String>(&source[start..offset])
                .is_ok_and(|decoded| decoded == RESERVED)
        } else {
            token == RESERVED
        };
        if bytes.get(after) == Some(&b':') && is_reserved {
            has_reserved_key = true;
            break;
        }
    }
    if !has_reserved_key {
        return (Cow::Borrowed(source), None);
    }

    let mut strings = Vec::<(usize, usize, String, bool)>::new();
    offset = 0;
    while offset < bytes.len() {
        if bytes[offset] != b'"' {
            offset += 1;
            continue;
        }
        let start = offset;
        offset += 1;
        let mut escaped = false;
        while offset < bytes.len() {
            let byte = bytes[offset];
            offset += 1;
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                break;
            }
        }
        if offset > bytes.len() || bytes.get(offset - 1) != Some(&b'"') {
            break;
        }
        let Ok(decoded) = serde_json::from_str::<String>(&source[start..offset]) else {
            continue;
        };
        let mut after = offset;
        while bytes
            .get(after)
            .is_some_and(|byte| matches!(byte, b' ' | b'\n' | b'\r' | b'\t'))
        {
            after += 1;
        }
        let is_key = bytes.get(after) == Some(&b':');
        strings.push((start, offset, decoded, is_key));
    }
    if !strings
        .iter()
        .any(|(_, _, value, is_key)| *is_key && value == RESERVED)
    {
        return (Cow::Borrowed(source), None);
    }
    let existing = strings
        .iter()
        .map(|(_, _, value, _)| value.as_str())
        .collect::<HashSet<_>>();
    let reserved_key = (0_u64..)
        .map(|number| {
            format!(
                "$guion_private_{number:0width$}$",
                width = RESERVED.len() - 16
            )
        })
        .find(|candidate| !existing.contains(candidate.as_str()))
        .expect("a unique private key can be generated");
    let mut rewritten = String::with_capacity(source.len());
    let mut copied_until = 0;
    for (start, end, decoded, is_key) in strings {
        if !is_key || decoded != RESERVED {
            continue;
        }
        let raw_content = &source[start + 1..end - 1];
        let escape_count = raw_content.len().saturating_sub(RESERVED.len()) / 5;
        let mut replacement = String::with_capacity(raw_content.len());
        for (index, character) in reserved_key.chars().enumerate() {
            if index < escape_count {
                replacement.push_str(&format!("\\u{:04x}", character as u32));
            } else {
                replacement.push(character);
            }
        }
        debug_assert_eq!(replacement.len(), raw_content.len());
        rewritten.push_str(&source[copied_until..start + 1]);
        rewritten.push_str(&replacement);
        copied_until = end - 1;
    }
    rewritten.push_str(&source[copied_until..]);
    (Cow::Owned(rewritten), Some(reserved_key))
}

pub(crate) fn restore_reserved_json_keys(value: &mut serde_json::Value, reserved_key: &str) {
    match value {
        serde_json::Value::Array(values) => {
            for value in values {
                restore_reserved_json_keys(value, reserved_key);
            }
        }
        serde_json::Value::Object(object) => {
            let Some(mut value) = object.remove(reserved_key) else {
                for value in object.values_mut() {
                    restore_reserved_json_keys(value, reserved_key);
                }
                return;
            };
            restore_reserved_json_keys(&mut value, reserved_key);
            object.insert("$serde_json::private::Number".to_owned(), value);
            for value in object.values_mut() {
                restore_reserved_json_keys(value, reserved_key);
            }
        }
        _ => {}
    }
}

pub(crate) struct UniqueJsonSeed<'duplicate> {
    pub(crate) duplicate: &'duplicate mut Option<String>,
}

pub(crate) struct SharedJsonSeed<'duplicate> {
    duplicate: &'duplicate mut Option<String>,
}

impl<'de> serde::de::DeserializeSeed<'de> for SharedJsonSeed<'_> {
    type Value = SharedJson;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(SharedJsonVisitor {
            duplicate: self.duplicate,
        })
    }
}

pub(crate) struct SharedJsonVisitor<'duplicate> {
    duplicate: &'duplicate mut Option<String>,
}

impl<'de> serde::de::Visitor<'de> for SharedJsonVisitor<'_> {
    type Value = SharedJson;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(SharedJson::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(SharedJson::Number(value.into()))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(SharedJson::Number(value.into()))
    }

    fn visit_i128<E>(self, value: i128) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        serde_json::Number::from_i128(value)
            .map(SharedJson::Number)
            .ok_or_else(|| E::custom("JSON integer is out of range"))
    }

    fn visit_u128<E>(self, value: u128) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        serde_json::Number::from_u128(value)
            .map(SharedJson::Number)
            .ok_or_else(|| E::custom("JSON integer is out of range"))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        serde_json::Number::from_f64(value)
            .map(SharedJson::Number)
            .ok_or_else(|| E::custom("JSON number is out of range"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(SharedJson::String(SharedText::from(value)))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(SharedJson::String(SharedText::from(value)))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(SharedJson::Null)
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(SharedJson::Null)
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::DeserializeSeed;
        SharedJsonSeed {
            duplicate: &mut *self.duplicate,
        }
        .deserialize(deserializer)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element_seed(SharedJsonSeed {
            duplicate: &mut *self.duplicate,
        })? {
            values.push(Shared::new(value));
        }
        Ok(SharedJson::Array(Shared::new(values)))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        let mut object = BTreeMap::new();
        while let Some(key) = map.next_key::<String>()? {
            if object.contains_key(&key) {
                *self.duplicate = Some(key);
                return Err(serde::de::Error::custom("duplicate JSON object key"));
            }
            let value = map.next_value_seed(SharedJsonSeed {
                duplicate: &mut *self.duplicate,
            })?;
            object.insert(key, Shared::new(value));
        }
        if object.len() == 1 {
            if let Some(SharedJson::String(number)) = object
                .get("$serde_json::private::Number")
                .map(Shared::as_ref)
            {
                if let Ok(number) = number.parse::<serde_json::Number>() {
                    return Ok(SharedJson::Number(number));
                }
            }
        }
        Ok(SharedJson::Object(Shared::new(object)))
    }
}

impl<'de> serde::de::DeserializeSeed<'de> for UniqueJsonSeed<'_> {
    type Value = serde_json::Value;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(UniqueJsonVisitor {
            duplicate: self.duplicate,
        })
    }
}

pub(crate) struct UniqueJsonVisitor<'duplicate> {
    duplicate: &'duplicate mut Option<String>,
}

impl<'de> serde::de::Visitor<'de> for UniqueJsonVisitor<'_> {
    type Value = serde_json::Value;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(serde_json::Value::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(serde_json::Value::Number(value.into()))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(serde_json::Value::Number(value.into()))
    }

    fn visit_i128<E>(self, value: i128) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        serde_json::Number::from_i128(value)
            .map(serde_json::Value::Number)
            .ok_or_else(|| E::custom("JSON integer is out of range"))
    }

    fn visit_u128<E>(self, value: u128) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        serde_json::Number::from_u128(value)
            .map(serde_json::Value::Number)
            .ok_or_else(|| E::custom("JSON integer is out of range"))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        serde_json::Number::from_f64(value)
            .map(serde_json::Value::Number)
            .ok_or_else(|| E::custom("JSON number is out of range"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(serde_json::Value::String(value.to_owned()))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(serde_json::Value::String(value))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(serde_json::Value::Null)
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(serde_json::Value::Null)
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::DeserializeSeed;
        UniqueJsonSeed {
            duplicate: self.duplicate,
        }
        .deserialize(deserializer)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element_seed(UniqueJsonSeed {
            duplicate: &mut *self.duplicate,
        })? {
            values.push(value);
        }
        Ok(serde_json::Value::Array(values))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        let Some(first_key) = map.next_key::<String>()? else {
            return Ok(serde_json::Value::Object(serde_json::Map::new()));
        };

        // serde_json uses this reserved key when arbitrary-precision numbers
        // cannot be represented by its machine-number visitor methods.
        let mut keys = HashSet::new();
        let mut object = serde_json::Map::new();
        let first_value = map.next_value_seed(UniqueJsonSeed {
            duplicate: &mut *self.duplicate,
        })?;
        let next_key = map.next_key::<String>()?;
        if first_key == "$serde_json::private::Number" && next_key.is_none() {
            if let serde_json::Value::String(number) = &first_value {
                if let Ok(number) = number.parse::<serde_json::Number>() {
                    return Ok(serde_json::Value::Number(number));
                }
            }
        }
        keys.insert(first_key.clone());
        object.insert(first_key, first_value);
        let mut key = next_key;
        while let Some(current_key) = key {
            if !keys.insert(current_key.clone()) {
                *self.duplicate = Some(current_key);
                return Err(serde::de::Error::custom("duplicate JSON object key"));
            }
            let value = map.next_value_seed(UniqueJsonSeed {
                duplicate: &mut *self.duplicate,
            })?;
            object.insert(current_key, value);
            key = map.next_key::<String>()?;
        }
        Ok(serde_json::Value::Object(object))
    }
}

pub(crate) fn duplicate_json_key(source: &str) -> Option<(String, usize)> {
    #[derive(Default)]
    struct ObjectKeys {
        expecting_key: bool,
        keys: HashSet<String>,
    }
    let bytes = source.as_bytes();
    let mut stack: Vec<Option<ObjectKeys>> = Vec::new();
    let mut offset = 0;
    while offset < bytes.len() {
        match bytes[offset] {
            b' ' | b'\n' | b'\r' | b'\t' | b':' => offset += 1,
            b'{' => {
                stack.push(Some(ObjectKeys {
                    expecting_key: true,
                    ..ObjectKeys::default()
                }));
                offset += 1;
            }
            b'[' => {
                stack.push(None);
                offset += 1;
            }
            b'}' | b']' => {
                stack.pop();
                offset += 1;
            }
            b',' => {
                if let Some(Some(object)) = stack.last_mut() {
                    object.expecting_key = true;
                }
                offset += 1;
            }
            b'"' => {
                let start = offset;
                offset += 1;
                while offset < bytes.len() {
                    match bytes[offset] {
                        b'\\' => offset = (offset + 2).min(bytes.len()),
                        b'"' => {
                            offset += 1;
                            break;
                        }
                        _ => offset += 1,
                    }
                }
                if let Some(Some(object)) = stack.last_mut() {
                    if object.expecting_key {
                        let key = serde_json::from_str::<String>(&source[start..offset]).ok()?;
                        if !object.keys.insert(key.clone()) {
                            return Some((key, start));
                        }
                        object.expecting_key = false;
                    }
                }
            }
            _ => offset += 1,
        }
    }
    None
}
