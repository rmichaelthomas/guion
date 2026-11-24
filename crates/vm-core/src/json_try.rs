//! The non-raising JSON accessors (`json_try_*`), shared by the native and browser hosts.

use guion_model::{BigInt, JsonErrorValue, Shared, SharedJson};

pub type JsonTryError = JsonErrorValue;

pub fn json_try_get(
    value: &Shared<SharedJson>,
    key: &str,
) -> Result<Shared<SharedJson>, JsonTryError> {
    let Some(object) = value.as_object() else {
        return Err(json_error("E_JSON_SHAPE", "expected a JSON object"));
    };
    object
        .get(key)
        .cloned()
        .ok_or_else(|| json_error("E_JSON_MISSING", format!("JSON object has no key `{key}`")))
}

pub fn json_try_index(
    value: &Shared<SharedJson>,
    index: i64,
) -> Result<Shared<SharedJson>, JsonTryError> {
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
