//! The `Json` instructions for [`PortableHost`](crate::PortableHost).
//!
//! [`run_json_instruction`] is the one implementation of these operations: the native host
//! (`guion-runtime`) calls it as well, so a program that reads or builds JSON behaves identically
//! in the browser by construction. Parsing and the non-raising accessors come from the shared
//! [`json_decode`](crate::json_decode) and [`json_try`](crate::json_try) modules. The column and
//! projection instructions of the data library (`JsonIntColumn`, `JsonTextCategories`,
//! `JsonAsIntList`, `JsonProjectColumns`, `JsonTextCategoriesValues`, `JsonStringifyPairs`) are
//! not here and stay `R_HOST_UNSUPPORTED`.

use crate::json_decode::{parse_unique_shared_json, JsonDecodeError};
use crate::{
    charge_allocation, finish_json_value, finish_shared_json, json_exact_integer, json_try,
    shared_json_push_local_allocation, shared_json_set_local_allocation, value_limit_error, Budget,
    HostOpContext, PortableHost, VmLimits,
};
use guion_model::{
    BigInt, Float64, JsonErrorValue, Op, RuntimeError, Shared, SharedJson, Span, Value,
};
use std::mem::size_of;

fn invalid(message: &str, span: Span) -> RuntimeError {
    RuntimeError::new("R_TYPE_INVALID", message, span)
}

fn pop(stack: &mut Vec<Value>, span: Span) -> Result<Value, RuntimeError> {
    stack
        .pop()
        .ok_or_else(|| RuntimeError::new("R_STACK_UNDERFLOW", "operand stack is empty", span))
}

impl PortableHost {
    /// Runs one JSON instruction. `Ok(false)` means the instruction is not a JSON instruction
    /// this host implements and nothing was touched.
    pub(crate) fn portable_json(
        &mut self,
        context: &mut HostOpContext<'_>,
    ) -> Result<bool, RuntimeError> {
        let limits = self.limits;
        run_json_instruction(context, &limits)
    }
}

/// Runs one `Json` instruction on the frame in `context` under `limits`. This is the one
/// implementation of these instructions: the browser VM's host and the native runtime's host both
/// call it, so the two cannot drift. `Ok(false)` means the instruction is not one it implements
/// (the column and projection instructions of the data library are not) and nothing was touched.
pub fn run_json_instruction(
    context: &mut HostOpContext<'_>,
    limits: &VmLimits,
) -> Result<bool, RuntimeError> {
    let span = context.instruction.span;
    let instruction = context.instruction;
    let limits = *limits;
    let budget: &Budget = context.budget.as_ref();
    let stack = &mut *context.stack;
    match &instruction.op {
        Op::JsonNull => {
            let value = finish_json_value(serde_json::Value::Null, &limits, budget, span)?;
            stack.push(value);
        }
        Op::JsonBool => {
            let Value::Bool(boolean) = pop(stack, span)? else {
                return Err(invalid("json_bool requires Bool", span));
            };
            let value = finish_json_value(serde_json::Value::Bool(boolean), &limits, budget, span)?;
            stack.push(value);
        }
        Op::JsonInt => {
            let Value::Int(integer) = pop(stack, span)? else {
                return Err(invalid("json_int requires Int", span));
            };
            if integer.digit_count() > limits.max_integer_digits {
                return Err(RuntimeError::new(
                    "R_INTEGER_LIMIT",
                    "JSON integer exceeds the configured digit limit",
                    span,
                ));
            }
            let parsed = serde_json::from_str::<serde_json::Value>(&integer.to_string())
                .map_err(|error| RuntimeError::new("R_JSON_ENCODE", error.to_string(), span))?;
            stack.push(finish_json_value(parsed, &limits, budget, span)?);
        }
        Op::JsonFloat => {
            let Value::Float64(float) = pop(stack, span)? else {
                return Err(invalid("json_float requires Float64", span));
            };
            let number = serde_json::Number::from_f64(float.get()).ok_or_else(|| {
                RuntimeError::new("R_JSON_TYPE", "JSON number must be finite", span)
            })?;
            stack.push(finish_json_value(
                serde_json::Value::Number(number),
                &limits,
                budget,
                span,
            )?);
        }
        Op::JsonText => {
            let Value::Text(text) = pop(stack, span)? else {
                return Err(invalid("json_text requires Text", span));
            };
            if text.len() > limits.max_text_bytes {
                return Err(RuntimeError::new(
                    "R_TEXT_LIMIT",
                    "JSON text exceeds the configured byte limit",
                    span,
                ));
            }
            stack.push(finish_json_value(
                serde_json::Value::String(text.to_string()),
                &limits,
                budget,
                span,
            )?);
        }
        Op::JsonArray => {
            stack.push(finish_json_value(
                serde_json::Value::Array(Vec::new()),
                &limits,
                budget,
                span,
            )?);
        }
        Op::JsonObject => {
            stack.push(finish_json_value(
                serde_json::Value::Object(serde_json::Map::new()),
                &limits,
                budget,
                span,
            )?);
        }
        Op::JsonPush => {
            let Value::Json(child) = pop(stack, span)? else {
                return Err(invalid("json_push value must be Json", span));
            };
            let child = child.as_ref().clone();
            let Value::Json(array) = pop(stack, span)? else {
                return Err(invalid("json_push requires a JSON array", span));
            };
            let mut array = array.as_ref().clone();
            let Some(items) = array.as_array_mut() else {
                return Err(RuntimeError::new(
                    "R_JSON_TYPE",
                    "json_push requires an array",
                    span,
                ));
            };
            items.push(Shared::new(child));
            stack.push(finish_shared_json(array, &limits, budget, span)?);
        }
        Op::JsonSet => {
            let Value::Json(child) = pop(stack, span)? else {
                return Err(invalid("json_set value must be Json", span));
            };
            let child = child.as_ref().clone();
            let Value::Text(key) = pop(stack, span)? else {
                return Err(invalid("json_set key must be Text", span));
            };
            if key.len() > limits.max_text_bytes {
                return Err(RuntimeError::new(
                    "R_TEXT_LIMIT",
                    "JSON key exceeds the configured byte limit",
                    span,
                ));
            }
            let Value::Json(object) = pop(stack, span)? else {
                return Err(invalid("json_set requires a JSON object", span));
            };
            let mut object = object.as_ref().clone();
            let Some(fields) = object.as_object_mut() else {
                return Err(RuntimeError::new(
                    "R_JSON_TYPE",
                    "json_set requires an object",
                    span,
                ));
            };
            fields.insert(key.to_string(), Shared::new(child));
            stack.push(finish_shared_json(object, &limits, budget, span)?);
        }
        Op::JsonPushLocal(slot) => {
            let Value::Json(value) = pop(stack, span)? else {
                return Err(invalid("json_push value must be Json", span));
            };
            let local = context.locals.get_mut(*slot).ok_or_else(|| {
                RuntimeError::new(
                    "R_LOCAL_UNINITIALIZED",
                    "json_push assignment targets an uninitialized local",
                    span,
                )
            })?;
            let Value::Json(array) = local else {
                return Err(invalid("json_push assignment target must be Json", span));
            };
            let Some(items) = array.as_array() else {
                return Err(RuntimeError::new(
                    "R_JSON_TYPE",
                    "json_push requires a JSON array",
                    span,
                ));
            };
            if items.len() >= limits.max_collection_items {
                return Err(RuntimeError::new(
                    "R_COLLECTION_LIMIT",
                    "JSON array exceeds the configured item limit",
                    span,
                ));
            }
            charge_allocation(budget, shared_json_push_local_allocation(array), span)?;
            Shared::make_mut(array)
                .as_array_mut()
                .ok_or_else(|| {
                    RuntimeError::new("R_JSON_TYPE", "json_push requires a JSON array", span)
                })?
                .push(value);
            if let Some(error) = value_limit_error(local, &limits, span) {
                return Err(error);
            }
        }
        Op::JsonSetLocal(slot) => {
            let Value::Json(value) = pop(stack, span)? else {
                return Err(invalid("json_set value must be Json", span));
            };
            let Value::Text(key) = pop(stack, span)? else {
                return Err(invalid("json_set key must be Text", span));
            };
            if key.len() > limits.max_text_bytes {
                return Err(RuntimeError::new(
                    "R_TEXT_LIMIT",
                    "JSON key exceeds the configured byte limit",
                    span,
                ));
            }
            let local = context.locals.get_mut(*slot).ok_or_else(|| {
                RuntimeError::new(
                    "R_LOCAL_UNINITIALIZED",
                    "json_set assignment targets an uninitialized local",
                    span,
                )
            })?;
            let Value::Json(object) = local else {
                return Err(invalid("json_set assignment target must be Json", span));
            };
            let Some(fields) = object.as_object() else {
                return Err(RuntimeError::new(
                    "R_JSON_TYPE",
                    "json_set requires an object",
                    span,
                ));
            };
            if !fields.contains_key(key.as_ref()) && fields.len() >= limits.max_collection_items {
                return Err(RuntimeError::new(
                    "R_COLLECTION_LIMIT",
                    "JSON object exceeds the configured item limit",
                    span,
                ));
            }
            let copied_bytes = shared_json_set_local_allocation(object, &key);
            charge_allocation(budget, copied_bytes, span)?;
            Shared::make_mut(object)
                .as_object_mut()
                .ok_or_else(|| {
                    RuntimeError::new("R_JSON_TYPE", "json_set requires an object", span)
                })?
                .insert(key.to_string(), value);
            if let Some(error) = value_limit_error(local, &limits, span) {
                return Err(error);
            }
        }
        Op::JsonParse => {
            let Value::Text(source) = pop(stack, span)? else {
                return Err(invalid("json_parse requires Text", span));
            };
            if source.len() > limits.max_text_bytes {
                return Err(RuntimeError::new(
                    "R_TEXT_LIMIT",
                    "JSON input exceeds the configured byte limit",
                    span,
                ));
            }
            let parse_started = budget
                .runtime_profile
                .as_ref()
                .map(|_| std::time::Instant::now());
            let parsed = parse_unique_shared_json(&source);
            if let (Some(profile), Some(started)) = (&budget.runtime_profile, parse_started) {
                profile
                    .record_json_parse(started.elapsed().as_nanos().min(u64::MAX as u128) as u64);
            }
            let value = match parsed {
                Ok(value) => value,
                Err(JsonDecodeError::Parse(error)) => {
                    stack.push(Value::ResultErr(Box::new(Value::JsonError(Box::new(
                        JsonErrorValue {
                            code: "R_JSON_PARSE".into(),
                            message: error.to_string(),
                            line: error.line(),
                            column: error.column(),
                        },
                    )))));
                    return Ok(true);
                }
                Err(JsonDecodeError::DuplicateKey { key, line, column }) => {
                    stack.push(Value::ResultErr(Box::new(Value::JsonError(Box::new(
                        JsonErrorValue {
                            code: "R_JSON_DUPLICATE_KEY".into(),
                            message: format!("JSON object contains duplicate key `{key}`"),
                            line,
                            column,
                        },
                    )))));
                    return Ok(true);
                }
            };
            let value = finish_shared_json(value, &limits, budget, span)?;
            stack.push(Value::ResultOk(Box::new(value)));
        }
        Op::JsonStringify => {
            let Value::Json(value) = pop(stack, span)? else {
                return Err(invalid("json_stringify requires Json", span));
            };
            let text = serde_json::to_string(value.as_ref())
                .map_err(|error| RuntimeError::new("R_JSON_ENCODE", error.to_string(), span))?;
            if text.len() > limits.max_text_bytes {
                return Err(RuntimeError::new(
                    "R_TEXT_LIMIT",
                    "JSON output exceeds the configured byte limit",
                    span,
                ));
            }
            charge_allocation(budget, size_of::<String>().saturating_add(text.len()), span)?;
            stack.push(Value::Text(text.into()));
        }
        Op::JsonKeys => {
            let Value::Json(value) = pop(stack, span)? else {
                return Err(invalid("json_keys requires Json", span));
            };
            let SharedJson::Object(object) = value.as_ref() else {
                let found = match value.as_ref() {
                    SharedJson::Null => "null",
                    SharedJson::Bool(_) => "boolean",
                    SharedJson::Number(_) => "number",
                    SharedJson::String(_) => "string",
                    SharedJson::Array(_) => "array",
                    SharedJson::Object(_) => "object",
                };
                stack.push(Value::ResultErr(Box::new(Value::JsonError(Box::new(
                    JsonErrorValue {
                        code: "R_JSON_TYPE".into(),
                        message: format!("json_keys expects a JSON object, found {found}"),
                        line: 0,
                        column: 0,
                    },
                )))));
                return Ok(true);
            };
            if object.len() > limits.max_collection_items {
                return Err(RuntimeError::new(
                    "R_COLLECTION_LIMIT",
                    "json_keys result exceeds the configured item limit",
                    span,
                ));
            }
            let key_bytes: usize = object.keys().map(String::len).sum();
            charge_allocation(
                budget,
                crate::text_list_allocation(object.len(), key_bytes),
                span,
            )?;
            let keys: Vec<Value> = object
                .keys()
                .map(|key| Value::Text(key.as_str().into()))
                .collect();
            stack.push(Value::ResultOk(Box::new(Value::from_list_items(keys))));
        }
        Op::JsonErrorMessage | Op::JsonErrorLine | Op::JsonErrorColumn | Op::JsonErrorCode => {
            let Value::JsonError(error) = pop(stack, span)? else {
                return Err(invalid("JSON error accessor requires JsonError", span));
            };
            stack.push(match &instruction.op {
                Op::JsonErrorMessage => Value::Text(error.message.into()),
                Op::JsonErrorLine => Value::Int(BigInt::from_usize(error.line)),
                Op::JsonErrorColumn => Value::Int(BigInt::from_usize(error.column)),
                _ => Value::Text(error.code.into()),
            });
        }
        Op::JsonTryGet
        | Op::JsonTryGetText
        | Op::JsonTryGetInt
        | Op::JsonTryIndex
        | Op::JsonTryText
        | Op::JsonTryInt
        | Op::JsonTryBool
        | Op::JsonTryFloat => {
            let outcome: Result<Value, JsonErrorValue> = match &instruction.op {
                Op::JsonTryGet | Op::JsonTryGetText | Op::JsonTryGetInt => {
                    let Value::Text(key) = pop(stack, span)? else {
                        return Err(invalid("json_try_get key must be Text", span));
                    };
                    if key.len() > limits.max_text_bytes {
                        return Err(RuntimeError::new(
                            "R_TEXT_LIMIT",
                            "JSON key exceeds the configured byte limit",
                            span,
                        ));
                    }
                    let Value::Json(value) = pop(stack, span)? else {
                        return Err(invalid("json_try_get requires Json", span));
                    };
                    match &instruction.op {
                        Op::JsonTryGet => json_try::json_try_get(&value, &key).map(Value::Json),
                        Op::JsonTryGetText => {
                            json_try::json_try_get(&value, &key).and_then(|child| {
                                json_try::json_try_text(child.as_ref())
                                    .map(|text| Value::Text(text.into()))
                            })
                        }
                        _ => json_try::json_try_get(&value, &key).and_then(|child| {
                            json_try::json_try_int(child.as_ref()).map(Value::Int)
                        }),
                    }
                }
                Op::JsonTryIndex => {
                    let Value::Int(index) = pop(stack, span)? else {
                        return Err(invalid("json_try_index index must be Int", span));
                    };
                    let index = index.to_i64().unwrap_or(-1);
                    let Value::Json(value) = pop(stack, span)? else {
                        return Err(invalid("json_try_index requires Json", span));
                    };
                    json_try::json_try_index(&value, index).map(Value::Json)
                }
                _ => {
                    let Value::Json(value) = pop(stack, span)? else {
                        return Err(invalid("JSON conversion requires Json", span));
                    };
                    match &instruction.op {
                        Op::JsonTryText => json_try::json_try_text(value.as_ref())
                            .map(|text| Value::Text(text.into())),
                        Op::JsonTryInt => json_try::json_try_int(value.as_ref()).map(Value::Int),
                        Op::JsonTryBool => json_try::json_try_bool(value.as_ref()).map(Value::Bool),
                        _ => json_try::json_try_float(value.as_ref()).and_then(|number| {
                            Float64::new(number)
                                .map(Value::Float64)
                                .ok_or_else(|| JsonErrorValue {
                                    code: "E_JSON_TYPE".into(),
                                    message: "JSON number is outside finite Float64 range".into(),
                                    line: 0,
                                    column: 0,
                                })
                        }),
                    }
                }
            };
            match outcome {
                Ok(value) => {
                    if let Value::Int(integer) = &value {
                        if integer.digit_count() > limits.max_integer_digits {
                            return Err(RuntimeError::new(
                                "R_INTEGER_LIMIT",
                                "JSON integer exceeds the configured digit limit",
                                span,
                            ));
                        }
                    }
                    if let Value::Text(text) = &value {
                        charge_allocation(
                            budget,
                            size_of::<String>().saturating_add(text.len()),
                            span,
                        )?;
                    }
                    stack.push(Value::ResultOk(Box::new(value)));
                }
                Err(error) => stack.push(Value::ResultErr(Box::new(Value::JsonError(Box::new(
                    error,
                ))))),
            }
        }
        Op::JsonGet => {
            let Value::Text(key) = pop(stack, span)? else {
                return Err(invalid("json_get key must be Text", span));
            };
            let Value::Json(value) = pop(stack, span)? else {
                return Err(invalid("json_get requires Json", span));
            };
            let object = value.as_object().ok_or_else(|| {
                RuntimeError::new("R_JSON_TYPE", "json_get requires an object", span)
            })?;
            let value = object.get(key.as_ref()).cloned().ok_or_else(|| {
                RuntimeError::new(
                    "R_JSON_KEY",
                    format!("JSON object has no key `{key}`"),
                    span,
                )
            })?;
            stack.push(Value::Json(value));
        }
        Op::JsonIndex => {
            let index = match pop(stack, span)? {
                Value::Int(index) => index.to_usize(),
                _ => None,
            }
            .ok_or_else(|| {
                RuntimeError::new(
                    "R_JSON_INDEX",
                    "JSON array index must be a non-negative machine-sized Int",
                    span,
                )
            })?;
            let Value::Json(value) = pop(stack, span)? else {
                return Err(invalid("json_index requires Json", span));
            };
            let array = value.as_array().ok_or_else(|| {
                RuntimeError::new("R_JSON_TYPE", "json_index requires an array", span)
            })?;
            let value = array.get(index).cloned().ok_or_else(|| {
                RuntimeError::new(
                    "R_JSON_INDEX",
                    format!(
                        "JSON array index {index} is out of range for length {}",
                        array.len()
                    ),
                    span,
                )
            })?;
            stack.push(Value::Json(value));
        }
        Op::JsonHas => {
            let Value::Text(key) = pop(stack, span)? else {
                return Err(invalid("json_has key must be Text", span));
            };
            let Value::Json(value) = pop(stack, span)? else {
                return Err(invalid("json_has requires Json", span));
            };
            let object = value.as_object().ok_or_else(|| {
                RuntimeError::new("R_JSON_TYPE", "json_has requires an object", span)
            })?;
            stack.push(Value::Bool(object.contains_key(key.as_ref())));
        }
        Op::JsonLength => {
            let Value::Json(value) = pop(stack, span)? else {
                return Err(invalid("json_len requires Json", span));
            };
            let length = value
                .as_array()
                .map(|items| items.len())
                .or_else(|| value.as_object().map(|items| items.len()))
                .ok_or_else(|| {
                    RuntimeError::new("R_JSON_TYPE", "json_len requires an array or object", span)
                })?;
            stack.push(Value::Int(BigInt::from_usize(length)));
        }
        Op::JsonAsText => match pop(stack, span)? {
            Value::Json(value) => match value.as_ref() {
                SharedJson::String(text) => {
                    charge_allocation(
                        budget,
                        size_of::<String>().saturating_add(text.len()),
                        span,
                    )?;
                    stack.push(Value::Text(text.to_string().into()));
                }
                _ => {
                    return Err(RuntimeError::new(
                        "R_JSON_TYPE",
                        "JSON value is not Text",
                        span,
                    ))
                }
            },
            _ => return Err(invalid("json_as_text requires Json", span)),
        },
        Op::JsonAsInt => match pop(stack, span)? {
            Value::Json(value) if value.is_number() => {
                let integer = json_exact_integer(value.as_ref()).ok_or_else(|| {
                    RuntimeError::new("R_JSON_TYPE", "JSON number is not an exact integer", span)
                })?;
                if integer.digit_count() > limits.max_integer_digits {
                    return Err(RuntimeError::new(
                        "R_INTEGER_LIMIT",
                        "JSON integer exceeds the configured digit limit",
                        span,
                    ));
                }
                charge_allocation(
                    budget,
                    size_of::<BigInt>().saturating_add(integer.digit_count()),
                    span,
                )?;
                stack.push(Value::Int(integer));
            }
            Value::Json(_) => {
                return Err(RuntimeError::new(
                    "R_JSON_TYPE",
                    "JSON value is not an integer",
                    span,
                ))
            }
            _ => return Err(invalid("json_as_int requires Json", span)),
        },
        Op::JsonAsFloat => match pop(stack, span)? {
            Value::Json(value) if value.is_number() => {
                let number = value
                    .as_number()
                    .and_then(serde_json::Number::as_f64)
                    .ok_or_else(|| {
                        RuntimeError::new(
                            "R_JSON_TYPE",
                            "JSON number cannot be represented as Float64",
                            span,
                        )
                    })?;
                let value = Float64::new(number).ok_or_else(|| {
                    RuntimeError::new(
                        "R_JSON_TYPE",
                        "JSON number is outside finite Float64 range",
                        span,
                    )
                })?;
                stack.push(Value::Float64(value));
            }
            Value::Json(_) => {
                return Err(RuntimeError::new(
                    "R_JSON_TYPE",
                    "JSON value is not a number",
                    span,
                ))
            }
            _ => return Err(invalid("json_as_float requires Json", span)),
        },
        Op::JsonAsBool => match pop(stack, span)? {
            Value::Json(value) => match value.as_bool() {
                Some(boolean) => stack.push(Value::Bool(boolean)),
                None => {
                    return Err(RuntimeError::new(
                        "R_JSON_TYPE",
                        "JSON value is not Bool",
                        span,
                    ))
                }
            },
            _ => return Err(invalid("json_as_bool requires Json", span)),
        },
        Op::JsonIsNull => match pop(stack, span)? {
            Value::Json(value) => stack.push(Value::Bool(value.is_null())),
            _ => return Err(invalid("json_is_null requires Json", span)),
        },
        _ => return Ok(false),
    }
    Ok(true)
}
