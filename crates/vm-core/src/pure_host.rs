//! Host operations that need no host capability, shared by the native runtime and the browser VM.
//!
//! Each runs the same code in both, so the two cannot drift: the data library (dates, versions,
//! command-line parsing, CSV encoding, numeric text), the `Option` lookups, the error accessors,
//! HTTP response decoding, Unicode case folding and normalization, HMAC-SHA-256, gzip and zlib
//! over bytes, YAML, and TOML documents.

use crate::json_decode::{parse_unique_shared_json, JsonDecodeError};
use crate::services::TomlRegistry;
use crate::{
    charge_allocation, encoded_json_string_len, finish_json_value, finish_shared_json,
    json_exact_integer, Budget, HostOpContext, VmLimits,
};
use guion_model::{
    Artifact, BigInt, FileErrorValue, Float64, HttpErrorValue, NativeResourceValue, Op,
    RuntimeError, Shared, SharedJson, SharedText, Span, TextMap, Value,
};
use std::collections::HashMap;
use std::mem::size_of;
use std::time::Instant;

/// What the pure operations need from their host beyond the frame: the artifact (for the `StdError`
/// record), the host's TOML documents, and the ceiling that no `max_output` may raise for an inflate.
pub struct PureServices<'a> {
    pub artifact: &'a Artifact,
    pub toml: &'a mut TomlRegistry,
    pub decompress_ceiling: u64,
}

/// Runs the instruction in `context` if it is one of the pure operations above. `Ok(false)` means it
/// is not one and nothing was touched.
#[inline(never)]
pub fn run_pure_instruction(
    context: &mut HostOpContext<'_>,
    limits: &VmLimits,
    services: PureServices<'_>,
) -> Result<bool, RuntimeError> {
    let PureServices {
        artifact,
        toml,
        decompress_ceiling,
    } = services;
    let instruction = context.instruction;
    let budget = context.budget;
    let stack = &mut *context.stack;
    macro_rules! push_stack {
        ($value:expr) => {{
            let value = $value;
            stack.push(value);
        }};
    }
    macro_rules! pop {
        () => {
            stack.pop().ok_or_else(|| {
                RuntimeError::new(
                    "R_STACK_INVALID",
                    "verified bytecode stack underflow",
                    instruction.span,
                )
            })?
        };
    }
    match instruction.op {
        Op::HttpResponseText => {
            let response = pop!();
            push_stack!(http_response_text_value(
                response,
                budget.as_ref(),
                limits.max_text_bytes,
                instruction.span,
            )?);
        }
        Op::HttpResponseJson => {
            let response = pop!();
            push_stack!(http_response_json_value(
                response,
                artifact,
                budget.as_ref(),
                limits.max_text_bytes,
                limits,
                instruction.span,
            )?);
        }
        Op::CsvEncode => {
            let rows = match pop!() {
                Value::List(rows) => rows,
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "csv_encode requires List[List[Text]]",
                        instruction.span,
                    ));
                }
            };
            let mut encoded_rows = Vec::with_capacity(rows.len());
            for row in rows.iter() {
                let Value::List(fields) = row else {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "csv_encode row requires List[Text]",
                        instruction.span,
                    ));
                };
                let fields = fields
                    .iter()
                    .map(|value| match value {
                        Value::Text(text) => Ok(text.to_string()),
                        _ => Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "csv_encode field requires Text",
                            instruction.span,
                        )),
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                encoded_rows.push(fields);
            }
            let text = crate::data::csv_encode(&encoded_rows);
            if text.len() > limits.max_file_bytes || text.len() > limits.max_text_bytes {
                return Err(RuntimeError::new(
                    "R_TEXT_LIMIT",
                    "CSV output exceeds the configured text limit",
                    instruction.span,
                ));
            }
            charge_allocation(
                budget.as_ref(),
                size_of::<String>().saturating_add(text.len()),
                instruction.span,
            )?;
            push_stack!(Value::ResultOk(Box::new(Value::Text(text.into()))));
        }
        Op::TimeParseDate | Op::TimeParseTimestamp | Op::SemVerParse => {
            let text = match pop!() {
                Value::Text(text) => text,
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "data parser requires Text",
                        instruction.span,
                    ));
                }
            };
            if text.len() > limits.max_text_bytes {
                return Err(RuntimeError::new(
                    "R_TEXT_LIMIT",
                    "data parser input exceeds the configured text limit",
                    instruction.span,
                ));
            }
            let parsed: Result<Value, crate::data::DataError> = match instruction.op {
                Op::TimeParseDate => crate::data::Date::parse(&text).map(|date| {
                    Value::Record(
                        "Date".into(),
                        Shared::new(vec![
                            Value::Int(BigInt::from_i64(date.year as i64)),
                            Value::Int(BigInt::from_u64(date.month as u64)),
                            Value::Int(BigInt::from_u64(date.day as u64)),
                        ]),
                    )
                }),
                Op::TimeParseTimestamp => crate::data::parse_timestamp(&text).map(|micros| {
                    Value::Record(
                        "Instant".into(),
                        Shared::new(vec![Value::Int(BigInt::from_i64(micros))]),
                    )
                }),
                Op::SemVerParse => crate::data::parse_semver(&text).map(|version| {
                    Value::Record(
                        "SemVer".into(),
                        Shared::new(vec![
                            Value::Text(version.major.into()),
                            Value::Text(version.minor.into()),
                            Value::Text(version.patch.into()),
                            Value::from_list_items(
                                version
                                    .prerelease
                                    .into_iter()
                                    .map(|value| Value::Text(value.into()))
                                    .collect(),
                            ),
                            Value::from_list_items(
                                version
                                    .build
                                    .into_iter()
                                    .map(|value| Value::Text(value.into()))
                                    .collect(),
                            ),
                            Value::Text(version.original.into()),
                        ]),
                    )
                }),
                _ => unreachable!(),
            };
            match parsed {
                Ok(value) => push_stack!(Value::ResultOk(Box::new(value))),
                Err(error) => push_stack!(file_error_result(RuntimeError::new(
                    error.code,
                    error.message,
                    instruction.span
                ))),
            }
        }
        Op::TimeZoneDataVersion => {
            let version = Value::Text(crate::services::IANA_TZDB_VERSION.to_owned().into());
            charge_allocation(
                budget.as_ref(),
                crate::services::IANA_TZDB_VERSION.len(),
                instruction.span,
            )?;
            push_stack!(version);
        }
        Op::TimeFormatUtc => {
            let value = pop!();
            let Value::Record(name, fields) = value else {
                return Err(RuntimeError::new(
                    "R_TYPE_INVALID",
                    "time_format_utc requires Instant",
                    instruction.span,
                ));
            };
            if name != "Instant" || fields.len() != 1 {
                return Err(RuntimeError::new(
                    "R_TYPE_INVALID",
                    "invalid Instant record",
                    instruction.span,
                ));
            }
            let Value::Int(micros) = &fields[0] else {
                return Err(RuntimeError::new(
                    "R_TYPE_INVALID",
                    "invalid Instant value",
                    instruction.span,
                ));
            };
            let micros = micros.to_i64().ok_or_else(|| {
                RuntimeError::new(
                    "R_TIME_RANGE",
                    "instant is outside supported range",
                    instruction.span,
                )
            })?;
            let text = crate::data::format_utc(crate::data::Instant {
                unix_microseconds: micros,
            });
            if text.len() > limits.max_text_bytes {
                return Err(RuntimeError::new(
                    "R_TEXT_LIMIT",
                    "formatted timestamp exceeds configured text limit",
                    instruction.span,
                ));
            }
            charge_allocation(
                budget.as_ref(),
                size_of::<String>().saturating_add(text.len()),
                instruction.span,
            )?;
            push_stack!(Value::Text(text.into()));
        }
        Op::TimeFormatUtcDate => {
            let value = pop!();
            let Value::Record(name, fields) = value else {
                return Err(RuntimeError::new(
                    "R_TYPE_INVALID",
                    "time_format_utc_date requires Instant",
                    instruction.span,
                ));
            };
            if name != "Instant" || fields.len() != 1 {
                return Err(RuntimeError::new(
                    "R_TYPE_INVALID",
                    "invalid Instant record",
                    instruction.span,
                ));
            }
            let Value::Int(micros) = &fields[0] else {
                return Err(RuntimeError::new(
                    "R_TYPE_INVALID",
                    "invalid Instant value",
                    instruction.span,
                ));
            };
            let micros = micros.to_i64().ok_or_else(|| {
                RuntimeError::new(
                    "R_TIME_RANGE",
                    "instant is outside supported range",
                    instruction.span,
                )
            })?;
            let text = crate::data::format_utc_date(crate::data::Instant {
                unix_microseconds: micros,
            });
            if text.len() > limits.max_text_bytes {
                return Err(RuntimeError::new(
                    "R_TEXT_LIMIT",
                    "formatted UTC date exceeds configured text limit",
                    instruction.span,
                ));
            }
            charge_allocation(
                budget.as_ref(),
                size_of::<String>().saturating_add(text.len()),
                instruction.span,
            )?;
            push_stack!(Value::Text(text.into()));
        }
        Op::SemVerCompare => {
            let right = pop!();
            let left = pop!();
            let parse_record = |value: &Value| -> Result<crate::data::SemVer, RuntimeError> {
                let Value::Record(name, fields) = value else {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "semver_compare requires SemVer records",
                        instruction.span,
                    ));
                };
                if name != "SemVer" || fields.len() != 6 {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "invalid SemVer record",
                        instruction.span,
                    ));
                }
                let Value::Text(original) = &fields[5] else {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "invalid SemVer source text",
                        instruction.span,
                    ));
                };
                crate::data::parse_semver(original).map_err(|error| {
                    RuntimeError::new("R_TYPE_INVALID", error.message, instruction.span)
                })
            };
            let left = parse_record(&left)?;
            let right = parse_record(&right)?;
            let ordering = crate::data::compare_semver(&left, &right);
            push_stack!(Value::Int(BigInt::from_i64(match ordering {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            })));
        }
        Op::CliParse => {
            let options = match pop!() {
                Value::List(values) => values,
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "cli_parse options must be a list",
                        instruction.span,
                    ));
                }
            };
            let arguments = match pop!() {
                Value::List(values) => values,
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "cli_parse arguments must be a list",
                        instruction.span,
                    ));
                }
            };
            let arguments = arguments
                .iter()
                .map(|value| match value {
                    Value::Text(value) => Ok(value.to_string()),
                    _ => Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "cli_parse argument must be Text",
                        instruction.span,
                    )),
                })
                .collect::<Result<Vec<_>, _>>()?;
            let mut parsed_options = Vec::with_capacity(options.len());
            for option in options.iter() {
                let Value::Record(name, fields) = option else {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "cli_parse option must be CliOption",
                        instruction.span,
                    ));
                };
                if name != "CliOption" || fields.len() != 4 {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "invalid CliOption record",
                        instruction.span,
                    ));
                }
                let (
                    Value::Text(name),
                    Value::Bool(takes_value),
                    Value::Bool(repeatable),
                    Value::Bool(required),
                ) = (&fields[0], &fields[1], &fields[2], &fields[3])
                else {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "invalid CliOption fields",
                        instruction.span,
                    ));
                };
                parsed_options.push(crate::data::CliOption::new(
                    name.to_string(),
                    *takes_value,
                    *repeatable,
                    *required,
                ));
            }
            match crate::data::CliArguments::parse(&arguments, &parsed_options) {
                Ok(parsed) => {
                    let values = parsed
                        .values
                        .into_iter()
                        .map(|(key, values)| {
                            (
                                key,
                                Value::from_list_items(
                                    values
                                        .into_iter()
                                        .map(|value| Value::Text(value.into()))
                                        .collect(),
                                ),
                            )
                        })
                        .collect::<TextMap>();
                    let result = Value::Record(
                        "CliArgs".into(),
                        Shared::new(vec![
                            Value::Map(Shared::new(values)),
                            Value::from_list_items(
                                parsed
                                    .positionals
                                    .into_iter()
                                    .map(|value| Value::Text(value.into()))
                                    .collect(),
                            ),
                        ]),
                    );
                    push_stack!(Value::ResultOk(Box::new(result)));
                }
                Err(error) => push_stack!(file_error_result(RuntimeError::new(
                    error.code,
                    error.message,
                    instruction.span
                ))),
            }
        }
        Op::ListGetOption => {
            let index = match pop!() {
                Value::Int(index) => index.to_usize(),
                _ => None,
            };
            let list = pop!();
            let Some(index) = index else {
                push_stack!(Value::OptionNone);
                return Ok(true);
            };
            let value = list.list_item(index);
            push_stack!(value
                .map(|value| Value::OptionSome(Box::new(value)))
                .unwrap_or(Value::OptionNone));
        }
        Op::MapGetOption => {
            let key = match pop!() {
                Value::Text(key) => key,
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "map_get key must be Text",
                        instruction.span,
                    ));
                }
            };
            let map = match pop!() {
                Value::Map(map) => map,
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "map_get requires Map",
                        instruction.span,
                    ));
                }
            };
            push_stack!(map
                .get(key.as_ref())
                .cloned()
                .map(|value| Value::OptionSome(Box::new(value)))
                .unwrap_or(Value::OptionNone));
        }
        Op::TextParseInt | Op::TextParseFloat => {
            let text = match pop!() {
                Value::Text(text) => text,
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "numeric parser requires Text",
                        instruction.span,
                    ));
                }
            };
            let parsed: Result<Value, crate::data::DataError> =
                if instruction.op == Op::TextParseInt {
                    crate::data::text_parse_int(&text).map(Value::Int)
                } else {
                    crate::data::text_parse_float(&text).and_then(|value| {
                        Float64::new(value).map(Value::Float64).ok_or_else(|| {
                            crate::data::DataError {
                                code: "E_FLOAT_PARSE",
                                message: "floating-point value must be finite".into(),
                            }
                        })
                    })
                };
            match parsed {
                Ok(value) => {
                    if let Value::Int(integer) = &value {
                        if integer.digit_count() > limits.max_integer_digits {
                            return Err(RuntimeError::new(
                                "R_INTEGER_LIMIT",
                                "parsed integer exceeds the configured digit limit",
                                instruction.span,
                            ));
                        }
                    }
                    push_stack!(Value::ResultOk(Box::new(value)));
                }
                Err(error) => push_stack!(file_error_result(RuntimeError::new(
                    error.code,
                    error.message,
                    instruction.span
                ))),
            }
        }
        Op::TextCaseFold | Op::TextCollapseWhitespace => {
            let text = match pop!() {
                Value::Text(text) => text,
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "Unicode operation requires Text",
                        instruction.span,
                    ));
                }
            };
            if instruction.op == Op::TextCaseFold {
                let output = crate::services::text_casefold(&text);
                if output.len() > limits.max_text_bytes {
                    return Err(RuntimeError::new(
                        "R_TEXT_LIMIT",
                        "Unicode result exceeds the configured text limit",
                        instruction.span,
                    ));
                }
                charge_allocation(budget.as_ref(), output.len(), instruction.span)?;
                push_stack!(Value::Text(output.into()));
            } else if let Some(output) = crate::services::text_collapse_whitespace_if_changed(&text)
            {
                if output.len() > limits.max_text_bytes {
                    return Err(RuntimeError::new(
                        "R_TEXT_LIMIT",
                        "Unicode result exceeds the configured text limit",
                        instruction.span,
                    ));
                }
                charge_allocation(budget.as_ref(), output.len(), instruction.span)?;
                push_stack!(Value::Text(output.into()));
            } else {
                push_stack!(Value::Text(text));
            }
        }
        Op::TextNormalize => {
            let form = match pop!() {
                Value::Text(value) => value,
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "normalization form must be Text",
                        instruction.span,
                    ));
                }
            };
            let text = match pop!() {
                Value::Text(value) => value,
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "normalization input must be Text",
                        instruction.span,
                    ));
                }
            };
            match crate::services::text_normalize(&text, &form) {
                Ok(output) if output.len() <= limits.max_text_bytes => {
                    charge_allocation(budget.as_ref(), output.len(), instruction.span)?;
                    push_stack!(Value::ResultOk(Box::new(Value::Text(output.into()))));
                }
                Ok(_) => {
                    return Err(RuntimeError::new(
                        "R_TEXT_LIMIT",
                        "normalized text exceeds the configured text limit",
                        instruction.span,
                    ));
                }
                Err(error) => {
                    push_stack!(Value::ResultErr(Box::new(service_error_value(error))))
                }
            }
        }
        Op::CryptoHmacSha256 => {
            let body = match pop!() {
                Value::Bytes(value) => value,
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "HMAC body must be Bytes",
                        instruction.span,
                    ));
                }
            };
            let key = match pop!() {
                Value::Bytes(value) => value,
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "HMAC key must be Bytes",
                        instruction.span,
                    ));
                }
            };
            push_stack!(Value::Bytes(Shared::new(
                crate::services::crypto_hmac_sha256(&key, &body)
            )));
        }
        Op::CryptoVerifyHmacSha256 => {
            let expected = match pop!() {
                Value::Bytes(value) => value,
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "HMAC digest must be Bytes",
                        instruction.span,
                    ));
                }
            };
            let body = match pop!() {
                Value::Bytes(value) => value,
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "HMAC body must be Bytes",
                        instruction.span,
                    ));
                }
            };
            let key = match pop!() {
                Value::Bytes(value) => value,
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "HMAC key must be Bytes",
                        instruction.span,
                    ));
                }
            };
            push_stack!(Value::Bool(crate::services::crypto_verify_hmac_sha256(
                &key, &body, &expected
            )));
        }
        Op::GzipCompress | Op::ZlibCompress => {
            let data = match pop!() {
                Value::Bytes(value) => value,
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "compression input must be Bytes",
                        instruction.span,
                    ));
                }
            };
            let packed = if matches!(instruction.op, Op::GzipCompress) {
                crate::compress::gzip_compress(&data)
            } else {
                crate::compress::zlib_compress(&data)
            };
            charge_allocation(budget.as_ref(), packed.len(), instruction.span)?;
            push_stack!(Value::Bytes(Shared::new(packed)));
        }
        Op::GzipDecompress | Op::ZlibDecompress => {
            let max_output = match pop!() {
                Value::Int(value) => value,
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "max_output must be Int",
                        instruction.span,
                    ));
                }
            };
            let data = match pop!() {
                Value::Bytes(value) => value,
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "compressed input must be Bytes",
                        instruction.span,
                    ));
                }
            };
            let outcome = crate::compress::inflate_bytes(
                matches!(instruction.op, Op::GzipDecompress),
                &data,
                &max_output,
                decompress_ceiling,
            );
            match outcome {
                Ok(unpacked) => {
                    charge_allocation(budget.as_ref(), unpacked.len(), instruction.span)?;
                    push_stack!(Value::ResultOk(Box::new(Value::Bytes(Shared::new(
                        unpacked
                    )))));
                }
                Err(found) => push_stack!(Value::ResultErr(Box::new(entropy_error_value(
                    found.code,
                    found.message,
                )))),
            }
        }
        Op::YamlParse => {
            let source = match pop!() {
                Value::Text(text) => text,
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "yaml_parse requires Text",
                        instruction.span,
                    ));
                }
            };
            if source.len() > limits.max_text_bytes {
                return Err(RuntimeError::new(
                    "R_TEXT_LIMIT",
                    "YAML input exceeds the configured byte limit",
                    instruction.span,
                ));
            }
            match crate::yaml::parse(&source) {
                Ok(parsed) => {
                    let shared = SharedJson::from_serde(parsed);
                    let value = finish_shared_json(
                        shared.as_ref().clone(),
                        limits,
                        budget.as_ref(),
                        instruction.span,
                    )?;
                    push_stack!(Value::ResultOk(Box::new(value)));
                }
                Err(found) => {
                    push_stack!(Value::ResultErr(Box::new(service_error_value(
                        crate::services::ServiceError {
                            code: found.code.to_owned(),
                            message: found.message,
                            path: String::new(),
                            line: found.line,
                            column: found.column,
                        }
                    ))));
                }
            }
        }
        Op::TomlParse => {
            let source = match pop!() {
                Value::Text(source) => source,
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "toml_parse input must be Text",
                        instruction.span,
                    ));
                }
            };
            let result = toml.parse(&source).map(|handle| {
                let (owner, id) = handle.parts();
                Value::NativeResource(Box::new(NativeResourceValue::new(
                    "TomlDocument",
                    owner,
                    id,
                )))
            });
            push_stack!(service_result_value(result));
        }
        Op::TomlGetTextList | Op::TomlKeys => {
            let path = runtime_text_path(&pop!(), instruction.span)?;
            let (owner, id) = runtime_service_resource(&pop!(), "TomlDocument", instruction.span)?;
            let handle = crate::services::TomlHandle::from_parts(owner, id);
            let found = if instruction.op == Op::TomlKeys {
                toml.keys(handle, &path)
            } else {
                toml.get_text_list(handle, &path)
            };
            let result = match found {
                Ok(items) => {
                    if items.len() > limits.max_collection_items {
                        return Err(RuntimeError::new(
                            "R_COLLECTION_LIMIT",
                            "TOML list exceeds the configured item limit",
                            instruction.span,
                        ));
                    }
                    let text_bytes: usize = items.iter().map(String::len).sum();
                    charge_allocation(
                        budget.as_ref(),
                        crate::text_list_allocation(items.len(), text_bytes),
                        instruction.span,
                    )?;
                    Ok(Value::from_list_items(
                        items
                            .into_iter()
                            .map(|item| Value::Text(item.into()))
                            .collect(),
                    ))
                }
                Err(error) => Err(error),
            };
            push_stack!(service_result_value(result));
        }
        Op::TomlHas | Op::TomlGetText | Op::TomlGetInt | Op::TomlGetBool => {
            let path = runtime_text_path(&pop!(), instruction.span)?;
            let (owner, id) = runtime_service_resource(&pop!(), "TomlDocument", instruction.span)?;
            let handle = crate::services::TomlHandle::from_parts(owner, id);
            let result = match instruction.op {
                Op::TomlHas => toml.has(handle, &path).map(Value::Bool),
                Op::TomlGetText => toml
                    .get_text(handle, &path)
                    .map(|text| Value::Text(text.into())),
                Op::TomlGetInt => toml
                    .get_integer(handle, &path)
                    .map(|value| Value::Int(BigInt::from_i64(value))),
                Op::TomlGetBool => toml.get_bool(handle, &path).map(Value::Bool),
                _ => unreachable!(),
            };
            if instruction.op == Op::TomlHas {
                let value = result.map_err(|error| {
                    RuntimeError::new(error.code, error.message, instruction.span)
                })?;
                push_stack!(value);
            } else {
                push_stack!(service_result_value(result));
            }
        }
        Op::TomlPutText | Op::TomlPutInt | Op::TomlPutBool => {
            let replacement = pop!();
            let path = runtime_text_path(&pop!(), instruction.span)?;
            let (owner, id) = runtime_service_resource(&pop!(), "TomlDocument", instruction.span)?;
            let handle = crate::services::TomlHandle::from_parts(owner, id);
            let result = match (&instruction.op, replacement) {
                (Op::TomlPutText, Value::Text(value)) => toml.put_text(handle, &path, &value),
                (Op::TomlPutInt, Value::Int(value)) => value
                    .to_i64()
                    .ok_or_else(|| {
                        crate::services::ServiceError::new(
                            "E_TOML_INTEGER_RANGE",
                            "TOML integers must fit signed 64-bit range",
                        )
                    })
                    .and_then(|value| toml.put_integer(handle, &path, value)),
                (Op::TomlPutBool, Value::Bool(value)) => toml.put_bool(handle, &path, value),
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "TOML value type does not match the edit operation",
                        instruction.span,
                    ));
                }
            }
            .map(|()| Value::Unit);
            push_stack!(service_result_value(result));
        }
        Op::TomlRender => {
            let (owner, id) = runtime_service_resource(&pop!(), "TomlDocument", instruction.span)?;
            let text = toml
                .render(crate::services::TomlHandle::from_parts(owner, id))
                .map_err(|error| {
                    RuntimeError::new("R_TOML_RENDER", error.to_string(), instruction.span)
                })?;
            if text.len() > limits.max_text_bytes {
                return Err(RuntimeError::new(
                    "R_TEXT_LIMIT",
                    "rendered TOML exceeds the configured text limit",
                    instruction.span,
                ));
            }
            charge_allocation(
                budget.as_ref(),
                size_of::<String>().saturating_add(text.len()),
                instruction.span,
            )?;
            push_stack!(Value::Text(text.into()));
        }
        Op::FileErrorCode | Op::FileErrorMessage | Op::FileErrorLine | Op::FileErrorColumn => {
            let error = match pop!() {
                Value::FileError(error) => error,
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "file error accessor requires FileError",
                        instruction.span,
                    ));
                }
            };
            push_stack!(match instruction.op {
                Op::FileErrorCode => Value::Text(error.code.into()),
                Op::FileErrorMessage => Value::Text(error.message.into()),
                Op::FileErrorLine => Value::Int(BigInt::from_usize(
                    error.trace.first().map(|span| span.line).unwrap_or(0),
                )),
                Op::FileErrorColumn => Value::Int(BigInt::from_usize(
                    error.trace.first().map(|span| span.column).unwrap_or(0),
                )),
                _ => unreachable!(),
            });
        }
        Op::HttpErrorCode | Op::HttpErrorMessage => {
            let error = match pop!() {
                Value::HttpError(error) => error,
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "HTTP error accessor requires HttpError",
                        instruction.span,
                    ));
                }
            };
            push_stack!(Value::Text(
                if matches!(instruction.op, Op::HttpErrorCode) {
                    error.code.into()
                } else {
                    error.message.into()
                }
            ));
        }
        Op::JsonStringifyPairs => {
            let value_field = match pop!() {
                Value::Text(value) => value,
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "json_stringify_pairs value field must be Text",
                        instruction.span,
                    ));
                }
            };
            let key_field = match pop!() {
                Value::Text(value) => value,
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "json_stringify_pairs key field must be Text",
                        instruction.span,
                    ));
                }
            };
            let groups = match pop!() {
                Value::Map(values) => values,
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "json_stringify_pairs requires Map[Text, List[Text]]",
                        instruction.span,
                    ));
                }
            };
            if key_field == value_field {
                return Err(RuntimeError::new(
                    "R_JSON_ENCODE",
                    "json_stringify_pairs field names must be distinct",
                    instruction.span,
                ));
            }
            if groups.len() > limits.max_collection_items {
                return Err(RuntimeError::new(
                    "R_COLLECTION_LIMIT",
                    "json_stringify_pairs exceeds the configured group limit",
                    instruction.span,
                ));
            }

            let encoded_key_field_len = encoded_json_string_len(&key_field);
            let encoded_value_field_len = encoded_json_string_len(&value_field);
            let mut row_count = 0usize;
            let mut output_length = 2usize;
            for (key, values) in groups.iter() {
                let Value::List(values) = values else {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "json_stringify_pairs requires Map[Text, List[Text]]",
                        instruction.span,
                    ));
                };
                if values.len() > limits.max_collection_items {
                    return Err(RuntimeError::new(
                        "R_COLLECTION_LIMIT",
                        "json_stringify_pairs exceeds the configured group size limit",
                        instruction.span,
                    ));
                }
                let encoded_key_len = encoded_json_string_len(key);
                for value in values.iter() {
                    let Value::Text(value) = value else {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_stringify_pairs requires Map[Text, List[Text]]",
                            instruction.span,
                        ));
                    };
                    if row_count >= limits.max_collection_items {
                        return Err(RuntimeError::new(
                            "R_COLLECTION_LIMIT",
                            "json_stringify_pairs exceeds the configured row limit",
                            instruction.span,
                        ));
                    }
                    if row_count > 0 {
                        output_length = output_length.saturating_add(1);
                    }
                    output_length = output_length
                        .saturating_add(5)
                        .saturating_add(encoded_key_field_len)
                        .saturating_add(encoded_key_len)
                        .saturating_add(encoded_value_field_len)
                        .saturating_add(encoded_json_string_len(value));
                    row_count += 1;
                    if output_length > limits.max_text_bytes {
                        return Err(RuntimeError::new(
                            "R_TEXT_LIMIT",
                            "JSON pair output exceeds the configured text limit",
                            instruction.span,
                        ));
                    }
                }
            }
            if output_length > limits.max_text_bytes {
                return Err(RuntimeError::new(
                    "R_TEXT_LIMIT",
                    "JSON pair output exceeds the configured text limit",
                    instruction.span,
                ));
            }
            charge_allocation(
                budget.as_ref(),
                size_of::<String>().saturating_add(output_length),
                instruction.span,
            )?;

            let mut output = Vec::with_capacity(output_length);
            let rows = JsonTextPairRows {
                groups: groups.as_ref(),
                key_field: &key_field,
                value_field: &value_field,
                row_count,
            };
            let mut serializer = serde_json::Serializer::new(&mut output);
            serde::Serialize::serialize(&rows, &mut serializer).map_err(|error| {
                RuntimeError::new("R_JSON_ENCODE", error.to_string(), instruction.span)
            })?;
            if output.len() != output_length {
                return Err(RuntimeError::new(
                    "R_JSON_ENCODE",
                    "JSON pair output length did not match its checked size",
                    instruction.span,
                ));
            }
            let text = String::from_utf8(output).map_err(|error| {
                RuntimeError::new("R_JSON_ENCODE", error.to_string(), instruction.span)
            })?;
            push_stack!(Value::Text(text.into()));
        }
        Op::JsonIntColumn => {
            let field = match pop!() {
                Value::Text(field) => field,
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "json_int_column requires Text field",
                        instruction.span,
                    ));
                }
            };
            let rows = match pop!() {
                Value::Json(rows) => rows,
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "json_int_column requires Json rows",
                        instruction.span,
                    ));
                }
            };
            let rows = rows.as_array().ok_or_else(|| {
                RuntimeError::new(
                    "R_JSON_TYPE",
                    "json_int_column requires an array of objects",
                    instruction.span,
                )
            })?;
            if rows.len() > limits.max_collection_items {
                return Err(RuntimeError::new(
                    "R_COLLECTION_LIMIT",
                    "JSON column exceeds the configured item limit",
                    instruction.span,
                ));
            }
            let mut values = Vec::with_capacity(rows.len());
            let mut allocation = size_of::<Vec<Value>>()
                .saturating_add(rows.len().saturating_mul(size_of::<Value>()));
            for row in rows {
                let object = row.as_object().ok_or_else(|| {
                    RuntimeError::new(
                        "R_JSON_TYPE",
                        "json_int_column rows must be objects",
                        instruction.span,
                    )
                })?;
                let value = object.get(field.as_ref()).ok_or_else(|| {
                    RuntimeError::new(
                        "R_JSON_FIELD",
                        format!("JSON row has no field {field:?}"),
                        instruction.span,
                    )
                })?;
                let integer = json_exact_integer(value).ok_or_else(|| {
                    RuntimeError::new(
                        "R_JSON_TYPE",
                        format!("JSON field {field:?} must contain an exact integer"),
                        instruction.span,
                    )
                })?;
                if integer.digit_count() > limits.max_integer_digits {
                    return Err(RuntimeError::new(
                        "R_INTEGER_LIMIT",
                        "JSON integer exceeds the configured digit limit",
                        instruction.span,
                    ));
                }
                allocation = allocation
                    .saturating_add(size_of::<Value>())
                    .saturating_add(integer.digit_count());
                values.push(Value::Int(integer));
            }
            charge_allocation(budget.as_ref(), allocation, instruction.span)?;
            push_stack!(Value::List(Shared::new(values)));
        }
        Op::JsonTextCategories => {
            let field = match pop!() {
                Value::Text(field) => field,
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "json_text_categories requires Text field",
                        instruction.span,
                    ));
                }
            };
            let rows = match pop!() {
                Value::Json(rows) => rows,
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "json_text_categories requires Json rows",
                        instruction.span,
                    ));
                }
            };
            let rows = rows.as_array().ok_or_else(|| {
                RuntimeError::new(
                    "R_JSON_TYPE",
                    "json_text_categories requires an array of objects",
                    instruction.span,
                )
            })?;
            if rows.len() > limits.max_collection_items {
                return Err(RuntimeError::new(
                    "R_COLLECTION_LIMIT",
                    "JSON category column exceeds the configured item limit",
                    instruction.span,
                ));
            }
            let mut codes = Vec::with_capacity(rows.len());
            let mut labels = Vec::<String>::new();
            let mut category_codes = HashMap::<String, usize>::new();
            for row in rows {
                let object = row.as_object().ok_or_else(|| {
                    RuntimeError::new(
                        "R_JSON_TYPE",
                        "json_text_categories rows must be objects",
                        instruction.span,
                    )
                })?;
                let value = object.get(field.as_ref()).ok_or_else(|| {
                    RuntimeError::new(
                        "R_JSON_FIELD",
                        format!("JSON row has no field {field:?}"),
                        instruction.span,
                    )
                })?;
                let label = value.as_str().ok_or_else(|| {
                    RuntimeError::new(
                        "R_JSON_TYPE",
                        format!("JSON field {field:?} must contain Text"),
                        instruction.span,
                    )
                })?;
                let code = if let Some(code) = category_codes.get(label) {
                    *code
                } else {
                    let code = labels.len();
                    if code >= limits.max_collection_items {
                        return Err(RuntimeError::new(
                            "R_COLLECTION_LIMIT",
                            "JSON categories exceed the configured item limit",
                            instruction.span,
                        ));
                    }
                    charge_allocation(
                        budget.as_ref(),
                        size_of::<(String, usize)>().saturating_add(label.len().saturating_mul(2)),
                        instruction.span,
                    )?;
                    let label = label.to_owned();
                    category_codes.insert(label.clone(), code);
                    labels.push(label);
                    code
                };
                codes.push(serde_json::Value::Number(serde_json::Number::from(
                    code as u64,
                )));
            }
            let labels = labels
                .into_iter()
                .map(serde_json::Value::String)
                .collect::<Vec<_>>();
            let value = serde_json::json!({ "codes": codes, "labels": labels });
            push_stack!(finish_json_value(
                value,
                limits,
                budget.as_ref(),
                instruction.span
            )?);
        }
        Op::JsonAsIntList => {
            let value = match pop!() {
                Value::Json(value) => value,
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "json_as_int_list requires Json",
                        instruction.span,
                    ));
                }
            };
            let items = value.as_array().ok_or_else(|| {
                RuntimeError::new(
                    "R_JSON_TYPE",
                    "json_as_int_list requires an array",
                    instruction.span,
                )
            })?;
            if items.len() > limits.max_collection_items {
                return Err(RuntimeError::new(
                    "R_COLLECTION_LIMIT",
                    "JSON integer list exceeds the configured item limit",
                    instruction.span,
                ));
            }
            let mut values = Vec::with_capacity(items.len());
            let mut allocation = size_of::<Vec<Value>>()
                .saturating_add(items.len().saturating_mul(size_of::<Value>()));
            for item in items {
                let integer = json_exact_integer(item).ok_or_else(|| {
                    RuntimeError::new(
                        "R_JSON_TYPE",
                        "JSON array values must be exact integers",
                        instruction.span,
                    )
                })?;
                if integer.digit_count() > limits.max_integer_digits {
                    return Err(RuntimeError::new(
                        "R_INTEGER_LIMIT",
                        "JSON integer exceeds the configured digit limit",
                        instruction.span,
                    ));
                }
                allocation = allocation
                    .saturating_add(size_of::<Value>())
                    .saturating_add(integer.digit_count());
                values.push(Value::Int(integer));
            }
            charge_allocation(budget.as_ref(), allocation, instruction.span)?;
            push_stack!(Value::List(Shared::new(values)));
        }
        Op::JsonProjectColumns => {
            let fields = match pop!() {
                Value::List(fields) => fields
                    .iter()
                    .map(|field| match field {
                        Value::Text(field) => Ok(field.clone()),
                        _ => Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_project_columns field names must be Text",
                            instruction.span,
                        )),
                    })
                    .collect::<Result<Vec<_>, _>>()?,
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "json_project_columns requires a List[Text] of fields",
                        instruction.span,
                    ));
                }
            };
            let path = match pop!() {
                Value::List(path) => path
                    .iter()
                    .map(|part| match part {
                        Value::Text(part) => Ok(part.clone()),
                        _ => Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_project_columns path segments must be Text",
                            instruction.span,
                        )),
                    })
                    .collect::<Result<Vec<_>, _>>()?,
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "json_project_columns requires a List[Text] path",
                        instruction.span,
                    ));
                }
            };
            let source = match pop!() {
                Value::Text(source) => source,
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "json_project_columns requires JSON Text",
                        instruction.span,
                    ));
                }
            };
            if source.len() > limits.max_text_bytes {
                return Err(RuntimeError::new(
                    "R_TEXT_LIMIT",
                    "JSON input exceeds the configured byte limit",
                    instruction.span,
                ));
            }
            let path = path.iter().map(|part| part.to_string()).collect();
            let fields = fields.iter().map(|field| field.to_string()).collect();
            let result = crate::json_projection::project_columns(
                &source,
                path,
                fields,
                limits.max_collection_items,
                limits.max_integer_digits,
            );
            match result {
                Ok(value) => push_stack!(Value::ResultOk(Box::new(finish_json_value(
                    value,
                    limits,
                    budget.as_ref(),
                    instruction.span,
                )?))),
                Err(error) => {
                    push_stack!(Value::ResultErr(Box::new(Value::JsonError(Box::new(
                        error,
                    )))))
                }
            }
        }
        Op::JsonTextCategoriesValues => {
            let values = match pop!() {
                Value::Json(values) => values,
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "json_text_categories_values requires Json values",
                        instruction.span,
                    ));
                }
            };
            let values = values.as_array().ok_or_else(|| {
                RuntimeError::new(
                    "R_JSON_TYPE",
                    "json_text_categories_values requires an array",
                    instruction.span,
                )
            })?;
            if values.len() > limits.max_collection_items {
                return Err(RuntimeError::new(
                    "R_COLLECTION_LIMIT",
                    "JSON category column exceeds the configured item limit",
                    instruction.span,
                ));
            }
            let mut codes = Vec::with_capacity(values.len());
            let mut labels = Vec::<String>::new();
            let mut category_codes = HashMap::<String, usize>::new();
            for value in values {
                let label = value.as_str().ok_or_else(|| {
                    RuntimeError::new(
                        "R_JSON_TYPE",
                        "JSON category values must be Text",
                        instruction.span,
                    )
                })?;
                let code = if let Some(code) = category_codes.get(label) {
                    *code
                } else {
                    let code = labels.len();
                    if code >= limits.max_collection_items {
                        return Err(RuntimeError::new(
                            "R_COLLECTION_LIMIT",
                            "JSON categories exceed the configured item limit",
                            instruction.span,
                        ));
                    }
                    charge_allocation(
                        budget.as_ref(),
                        size_of::<(String, usize)>().saturating_add(label.len().saturating_mul(2)),
                        instruction.span,
                    )?;
                    let label = label.to_owned();
                    category_codes.insert(label.clone(), code);
                    labels.push(label);
                    code
                };
                codes.push(serde_json::Value::Number(serde_json::Number::from(
                    code as u64,
                )));
            }
            let labels = labels
                .into_iter()
                .map(serde_json::Value::String)
                .collect::<Vec<_>>();
            let result = serde_json::json!({ "codes": codes, "labels": labels });
            push_stack!(finish_json_value(
                result,
                limits,
                budget.as_ref(),
                instruction.span
            )?);
        }

        Op::TaskIndex => match pop!() {
            Value::TaskError(error) => push_stack!(Value::Int(error.index)),
            _ => {
                return Err(RuntimeError::new(
                    "R_TYPE_INVALID",
                    "task_index requires TaskError",
                    instruction.span,
                ));
            }
        },
        Op::TaskCode => match pop!() {
            Value::TaskError(error) => push_stack!(Value::Text(error.code.into())),
            _ => {
                return Err(RuntimeError::new(
                    "R_TYPE_INVALID",
                    "task_code requires TaskError",
                    instruction.span,
                ));
            }
        },
        Op::TaskMessage => match pop!() {
            Value::TaskError(error) => push_stack!(Value::Text(error.message.into())),
            _ => {
                return Err(RuntimeError::new(
                    "R_TYPE_INVALID",
                    "task_message requires TaskError",
                    instruction.span,
                ));
            }
        },

        _ => return Ok(false),
    }
    Ok(true)
}

pub fn http_response_text_value(
    response: Value,
    budget: &Budget,
    max_text_bytes: usize,
    span: Span,
) -> Result<Value, RuntimeError> {
    let fields = match response {
        Value::Record(name, fields) if name == "HttpResponse" && fields.len() == 3 => fields,
        _ => {
            return Err(RuntimeError::new(
                "R_TYPE_INVALID",
                "http_response_text argument must be HttpResponse",
                span,
            ));
        }
    };
    let Value::Bytes(body) = &fields[2] else {
        return Err(RuntimeError::new(
            "R_TYPE_INVALID",
            "HttpResponse body must be Bytes",
            span,
        ));
    };
    if body.len() > max_text_bytes {
        let mut error = http_error(
            "R_HTTP_LIMIT",
            "HTTP response exceeds the configured text byte limit",
        );
        error.trace.push(span);
        return Ok(Value::ResultErr(Box::new(Value::HttpError(Box::new(
            error,
        )))));
    }
    match std::str::from_utf8(body.as_ref()) {
        Ok(text) => {
            charge_allocation(budget, text.len(), span)?;
            Ok(Value::ResultOk(Box::new(Value::Text(text.into()))))
        }
        Err(_) => {
            let mut error = http_error("R_HTTP_ENCODING", "HTTP response body is not valid UTF-8");
            error.trace.push(span);
            Ok(Value::ResultErr(Box::new(Value::HttpError(Box::new(
                error,
            )))))
        }
    }
}

pub fn std_error_record_value(
    artifact: &Artifact,
    budget: &Budget,
    code: &str,
    message: &str,
    span: Span,
) -> Result<Value, RuntimeError> {
    let record = artifact
        .records
        .iter()
        .find(|record| record.name == "StdError")
        .ok_or_else(|| {
            RuntimeError::new(
                "R_ARTIFACT_INVALID",
                "HTTP JSON response requires the StdError record definition",
                span,
            )
        })?;
    if record.fields.len() != 5 {
        return Err(RuntimeError::new(
            "R_ARTIFACT_INVALID",
            "StdError must contain code, message, path, line, and column fields",
            span,
        ));
    }
    let record_bytes = size_of::<Vec<Value>>()
        .saturating_add(record.fields.len().saturating_mul(size_of::<Value>()))
        .saturating_add(record.name.len());
    charge_allocation(budget, record_bytes, span)?;
    budget.record_record_charge(record_bytes);
    Ok(Value::Record(
        record.name.clone(),
        Shared::new(vec![
            Value::Text(SharedText::from(code)),
            Value::Text(SharedText::from(message)),
            Value::Text(SharedText::from("")),
            Value::Int(BigInt::zero()),
            Value::Int(BigInt::zero()),
        ]),
    ))
}

pub fn http_response_json_value(
    response: Value,
    artifact: &Artifact,
    budget: &Budget,
    max_text_bytes: usize,
    limits: &VmLimits,
    span: Span,
) -> Result<Value, RuntimeError> {
    let fields = match response {
        Value::Record(name, fields) if name == "HttpResponse" && fields.len() == 3 => fields,
        _ => {
            return Err(RuntimeError::new(
                "R_TYPE_INVALID",
                "http_response_json_bytes argument must be HttpResponse",
                span,
            ));
        }
    };
    let Value::Bytes(body) = &fields[2] else {
        return Err(RuntimeError::new(
            "R_TYPE_INVALID",
            "HttpResponse body must be Bytes",
            span,
        ));
    };
    if body.len() > max_text_bytes {
        let value = std_error_record_value(
            artifact,
            budget,
            "R_HTTP_LIMIT",
            "HTTP response exceeds the configured text byte limit",
            span,
        )?;
        return Ok(Value::ResultErr(Box::new(value)));
    }
    let text = match std::str::from_utf8(body.as_ref()) {
        Ok(text) => text,
        Err(_) => {
            let value = std_error_record_value(
                artifact,
                budget,
                "R_HTTP_ENCODING",
                "HTTP response body is not valid UTF-8",
                span,
            )?;
            return Ok(Value::ResultErr(Box::new(value)));
        }
    };

    // Match the old Text conversion's guest allocation charge while parsing the
    // response's existing bytes in place, without allocating an intermediate Text.
    charge_allocation(budget, body.len(), span)?;
    let parse_started = budget.runtime_profile.as_ref().map(|_| Instant::now());
    let parsed = parse_unique_shared_json(text);
    if let (Some(profile), Some(started)) = (&budget.runtime_profile, parse_started) {
        profile.record_json_parse(started.elapsed().as_nanos().min(u64::MAX as u128) as u64);
    }
    match parsed {
        Ok(value) => {
            let value = finish_shared_json(value, limits, budget, span)?;
            Ok(Value::ResultOk(Box::new(value)))
        }
        Err(JsonDecodeError::Parse(error)) => {
            let value =
                std_error_record_value(artifact, budget, "R_JSON_PARSE", &error.to_string(), span)?;
            Ok(Value::ResultErr(Box::new(value)))
        }
        Err(JsonDecodeError::DuplicateKey { key, .. }) => {
            let message = format!("JSON object contains duplicate key `{key}`");
            let value =
                std_error_record_value(artifact, budget, "R_JSON_DUPLICATE_KEY", &message, span)?;
            Ok(Value::ResultErr(Box::new(value)))
        }
    }
}

pub fn file_error_result(error: RuntimeError) -> Value {
    Value::ResultErr(Box::new(Value::FileError(Box::new(FileErrorValue {
        code: error.code,
        message: error.message,
        trace: error.trace,
    }))))
}

pub fn runtime_service_resource(
    value: &Value,
    expected_kind: &str,
    span: Span,
) -> Result<(u64, u64), RuntimeError> {
    let Value::NativeResource(resource) = value else {
        return Err(RuntimeError::new(
            "R_TYPE_INVALID",
            format!("operation requires {expected_kind}"),
            span,
        ));
    };
    let (kind, owner, id) = resource.parts();
    if kind != expected_kind {
        return Err(RuntimeError::new(
            "R_TYPE_INVALID",
            format!("operation requires {expected_kind}"),
            span,
        ));
    }
    Ok((owner, id))
}

pub fn runtime_text_path(value: &Value, span: Span) -> Result<Vec<String>, RuntimeError> {
    let Value::List(items) = value else {
        return Err(RuntimeError::new(
            "R_TYPE_INVALID",
            "TOML path must be List[Text]",
            span,
        ));
    };
    items
        .iter()
        .map(|item| match item {
            Value::Text(text) => Ok(text.to_string()),
            _ => Err(RuntimeError::new(
                "R_TYPE_INVALID",
                "TOML path must contain only Text",
                span,
            )),
        })
        .collect()
}

pub fn entropy_error_value(code: &str, message: String) -> Value {
    service_error_value(crate::services::ServiceError {
        code: code.to_owned(),
        message,
        path: String::new(),
        line: 0,
        column: 0,
    })
}

pub fn service_error_value(error: crate::services::ServiceError) -> Value {
    Value::Record(
        "StdError".into(),
        Shared::new(vec![
            Value::Text(error.code.into()),
            Value::Text(error.message.into()),
            Value::Text(error.path.into()),
            Value::Int(BigInt::from_usize(error.line)),
            Value::Int(BigInt::from_usize(error.column)),
        ]),
    )
}

pub fn service_result_value(result: Result<Value, crate::services::ServiceError>) -> Value {
    match result {
        Ok(value) => Value::ResultOk(Box::new(value)),
        Err(error) => Value::ResultErr(Box::new(service_error_value(error))),
    }
}

pub fn http_error(code: &str, message: impl Into<String>) -> HttpErrorValue {
    HttpErrorValue {
        code: code.into(),
        message: message.into(),
        trace: Vec::new(),
    }
}

struct JsonTextPairRows<'a> {
    groups: &'a TextMap,
    key_field: &'a str,
    value_field: &'a str,
    row_count: usize,
}

impl serde::Serialize for JsonTextPairRows<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeSeq;

        let mut sequence = serializer.serialize_seq(Some(self.row_count))?;
        for (key, values) in self.groups {
            let Value::List(values) = values else {
                return Err(serde::ser::Error::custom(
                    "json_stringify_pairs requires Map[Text, List[Text]]",
                ));
            };
            for value in values.iter() {
                let Value::Text(value) = value else {
                    return Err(serde::ser::Error::custom(
                        "json_stringify_pairs requires Map[Text, List[Text]]",
                    ));
                };
                sequence.serialize_element(&JsonTextPair {
                    key_field: self.key_field,
                    key,
                    value_field: self.value_field,
                    value: value.as_ref(),
                })?;
            }
        }
        sequence.end()
    }
}

struct JsonTextPair<'a> {
    key_field: &'a str,
    key: &'a str,
    value_field: &'a str,
    value: &'a str,
}

impl serde::Serialize for JsonTextPair<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;

        let mut object = serializer.serialize_map(Some(2))?;
        object.serialize_entry(self.key_field, self.key)?;
        object.serialize_entry(self.value_field, self.value)?;
        object.end()
    }
}
