//! JSON interchange format used by a Guion-written compiler to hand verified
//! bytecode IR to the stage-zero artifact writer.
use std::{collections::BTreeMap, fs};

use guion_model::{
    Artifact, BigInt, ExternalFunction, Float64, Function, Instruction, Op, RecordDefinition,
    RecordField, Span, Type, Value,
};
use serde_json::Value as Json;

pub fn assemble_file(input: &str, output: &str) -> Result<(), String> {
    let bytes = fs::read(input).map_err(|error| format!("cannot read IR `{input}`: {error}"))?;
    let json: Json = serde_json::from_slice(&bytes)
        .map_err(|error| format!("invalid IR JSON `{input}`: {error}"))?;
    let artifact = assemble(&json)?;
    let encoded = artifact
        .to_bytes()
        .map_err(|error| format!("invalid bytecode IR: {}: {}", error.code, error.message))?;
    fs::write(output, encoded).map_err(|error| format!("cannot write artifact `{output}`: {error}"))
}

fn assemble(json: &Json) -> Result<Artifact, String> {
    let object = json.as_object().ok_or("IR root must be an object")?;
    if required_str(object.get("schema"), "schema")? != "guion-bytecode-ir/v1" {
        return Err("unsupported IR schema; expected `guion-bytecode-ir/v1`".into());
    }
    let source_name = optional_str(object.get("source_name"), "generated.gu")?;
    let entry = required_str(object.get("entry"), "entry")?.to_owned();
    let records = parse_records(object.get("records"))?;
    let externals = parse_externals(object.get("externals"))?;
    let raw_functions = object
        .get("functions")
        .and_then(Json::as_array)
        .ok_or("IR field `functions` must be an array")?;
    let names = raw_functions
        .iter()
        .enumerate()
        .map(|(index, raw)| {
            let function = raw
                .as_object()
                .ok_or_else(|| format!("function {index} must be an object"))?;
            required_str(function.get("name"), "function.name").map(str::to_owned)
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut functions = Vec::with_capacity(raw_functions.len());
    for (index, raw) in raw_functions.iter().enumerate() {
        let function = raw
            .as_object()
            .ok_or_else(|| format!("function {index} must be an object"))?;
        let name = names[index].clone();
        let parameters = string_array(function.get("parameters"), "function.parameters")?
            .iter()
            .map(|ty| parse_type(ty))
            .collect::<Result<Vec<_>, _>>()?;
        let result = parse_type(required_str(function.get("result"), "function.result")?)?;
        let local_count = required_usize(function.get("local_count"), "function.local_count")?;
        let raw_code = function
            .get("code")
            .and_then(Json::as_array)
            .ok_or_else(|| format!("function `{name}` field `code` must be an array"))?;
        let mut labels = BTreeMap::new();
        let mut executable_pc = 0usize;
        for (pc, instruction) in raw_code.iter().enumerate() {
            let object = instruction
                .as_object()
                .ok_or_else(|| format!("instruction {pc} in `{name}` must be an object"))?;
            let op = required_str(object.get("op"), "instruction.op")?;
            if op == "Label" {
                let label = required_usize(object.get("id"), "Label.id")?;
                if labels.insert(label, executable_pc).is_some() {
                    return Err(format!("duplicate label {label} in function `{name}`"));
                }
            } else {
                executable_pc = executable_pc.saturating_add(1);
            }
        }
        let mut code = Vec::with_capacity(executable_pc);
        for (pc, instruction) in raw_code.iter().enumerate() {
            let object = instruction
                .as_object()
                .expect("instruction object checked above");
            if required_str(object.get("op"), "instruction.op")? != "Label" {
                code.push(parse_instruction(
                    instruction,
                    pc,
                    &names,
                    &records,
                    &externals,
                    &labels,
                )?);
            }
        }
        functions.push(Function {
            name,
            parameters,
            result,
            local_count,
            code,
        });
    }
    let artifact = Artifact {
        format_version: Artifact::CURRENT_VERSION,
        build_metadata: None,
        compiled_source_sha256: None,
        source_name: source_name.clone(),
        sources: vec![source_name],
        entry,
        functions,
        externals,
        records,
    };
    // Artifact::to_bytes performs the authoritative control-flow, local,
    // operand, and function-reference verification before serialization.
    Ok(artifact)
}

fn parse_instruction(
    raw: &Json,
    pc: usize,
    functions: &[String],
    records: &[RecordDefinition],
    externals: &[ExternalFunction],
    labels: &BTreeMap<usize, usize>,
) -> Result<Instruction, String> {
    let object = raw
        .as_object()
        .ok_or_else(|| format!("instruction {pc} must be an object"))?;
    let op_name = required_str(object.get("op"), "instruction.op")?;
    let op = match op_name {
        "ConstInt" => Op::Const(Value::Int(
            BigInt::parse(required_str(object.get("value"), "ConstInt.value")?)
                .map_err(|e| format!("invalid integer at instruction {pc}: {e}"))?,
        )),
        "ConstBool" => Op::Const(Value::Bool(
            object
                .get("value")
                .and_then(Json::as_bool)
                .ok_or("ConstBool.value must be boolean")?,
        )),
        "ConstText" => Op::Const(Value::Text(
            required_str(object.get("value"), "ConstText.value")?
                .to_owned()
                .into(),
        )),
        "ConstFloat64" => {
            let value = required_str(object.get("value"), "ConstFloat64.value")?
                .parse::<f64>()
                .map_err(|error| format!("invalid Float64 at instruction {pc}: {error}"))?;
            Op::Const(Value::Float64(
                Float64::new(value).ok_or("Float64 IR constants must be finite")?,
            ))
        }
        "ConstUnit" => Op::Const(Value::Unit),
        "Load" => Op::Load(required_usize(object.get("local"), "Load.local")?),
        "Store" => Op::Store(required_usize(object.get("local"), "Store.local")?),
        "Pop" => Op::Pop,
        "Add" => Op::Add,
        "Subtract" => Op::Subtract,
        "Multiply" => Op::Multiply,
        "Divide" => Op::Divide,
        "IntegerDivide" => Op::IntegerDivide,
        "FloorDivide" => Op::FloorDivide,
        "Remainder" => Op::Remainder,
        "Negate" => Op::Negate,
        "Not" => Op::Not,
        "Equal" => Op::Equal,
        "NotEqual" => Op::NotEqual,
        "Less" => Op::Less,
        "LessEqual" => Op::LessEqual,
        "Greater" => Op::Greater,
        "GreaterEqual" => Op::GreaterEqual,
        "Jump" => Op::Jump(jump_target(object, "Jump", labels)?),
        "JumpIfFalse" => Op::JumpIfFalse(jump_target(object, "JumpIfFalse", labels)?),
        "Call" => {
            let target = required_str(object.get("function"), "Call.function")?;
            let index = functions
                .iter()
                .position(|name| name == target)
                .ok_or_else(|| format!("Call.function references unknown function `{target}`"))?;
            Op::Call(index, required_usize(object.get("argc"), "Call.argc")?)
        }
        "PythonCall" | "PythonCallKw" => {
            let name = required_str(object.get("function"), "PythonCall.function")?;
            let index = externals
                .iter()
                .position(|external| external.name == name)
                .ok_or_else(|| {
                    format!("{op_name}.function references unknown external `{name}`")
                })?;
            let argc = required_usize(object.get("argc"), "PythonCall.argc")?;
            if op_name == "PythonCall" {
                Op::PythonCall(index, argc)
            } else {
                Op::PythonCallKw(
                    index,
                    argc,
                    string_array(object.get("keyword_names"), "PythonCallKw.keyword_names")?,
                )
            }
        }
        "MakeRecord" => {
            let name = required_str(object.get("record"), "MakeRecord.record")?;
            let index = records
                .iter()
                .position(|record| record.name == name)
                .ok_or_else(|| format!("MakeRecord references unknown record `{name}`"))?;
            Op::MakeRecord(
                index,
                required_usize(object.get("count"), "MakeRecord.count")?,
            )
        }
        "GetField" => {
            let name = required_str(object.get("record"), "GetField.record")?;
            let record = records
                .iter()
                .position(|record| record.name == name)
                .ok_or_else(|| format!("GetField references unknown record `{name}`"))?;
            let field_name = required_str(object.get("field"), "GetField.field")?;
            let field = records[record]
                .fields
                .iter()
                .position(|field| field.name == field_name)
                .ok_or_else(|| {
                    format!("GetField references unknown field `{name}.{field_name}`")
                })?;
            Op::GetField(record, field)
        }
        "Print" => Op::Print,
        "PrintErr" => Op::PrintErr,
        "Return" => Op::Return,
        "Length" => Op::Length,
        "Index" => Op::Index,
        "MakeList" => Op::MakeList(required_usize(object.get("count"), "MakeList.count")?),
        "MakeMap" => Op::MakeMap(required_usize(object.get("count"), "MakeMap.count")?),
        "MakeTypedList" => Op::MakeTypedList(parse_type(required_str(
            object.get("item_type"),
            "MakeTypedList.item_type",
        )?)?),
        "MakeTypedMap" => Op::MakeTypedMap(parse_type(required_str(
            object.get("item_type"),
            "MakeTypedMap.item_type",
        )?)?),
        "MapKeys" => Op::MapKeys,
        "MapSet" => Op::MapSet,
        "MapHas" => Op::MapHas,
        "ListPush" => Op::ListPush,
        "ListPushLocal" => {
            Op::ListPushLocal(required_usize(object.get("local"), "ListPushLocal.local")?)
        }
        "ParallelMap" => {
            let name = required_str(object.get("function"), "ParallelMap.function")?;
            let index = functions
                .iter()
                .position(|function| function == name)
                .ok_or_else(|| {
                    format!("ParallelMap.function references unknown function `{name}`")
                })?;
            Op::ParallelMap(index)
        }
        "ListSet" => Op::ListSet,
        "ListSetLocal" => {
            Op::ListSetLocal(required_usize(object.get("local"), "ListSetLocal.local")?)
        }
        "FloatSqrt" => Op::FloatSqrt,
        "JsonParse" => Op::JsonParse,
        "JsonStringify" => Op::JsonStringify,
        "JsonNull" => Op::JsonNull,
        "JsonBool" => Op::JsonBool,
        "JsonInt" => Op::JsonInt,
        "JsonFloat" => Op::JsonFloat,
        "JsonText" => Op::JsonText,
        "JsonArray" => Op::JsonArray,
        "JsonObject" => Op::JsonObject,
        "JsonPush" => Op::JsonPush,
        "JsonPushLocal" => {
            Op::JsonPushLocal(required_usize(object.get("local"), "JsonPushLocal.local")?)
        }
        "JsonSet" => Op::JsonSet,
        "JsonGet" => Op::JsonGet,
        "JsonIndex" => Op::JsonIndex,
        "JsonHas" => Op::JsonHas,
        "JsonLength" => Op::JsonLength,
        "JsonAsText" => Op::JsonAsText,
        "JsonAsInt" => Op::JsonAsInt,
        "JsonAsFloat" => Op::JsonAsFloat,
        "JsonAsBool" => Op::JsonAsBool,
        "JsonIsNull" => Op::JsonIsNull,
        "TextChars" => Op::TextChars,
        "TextJoin" => Op::TextJoin,
        "BytesFromInts" => Op::BytesFromInts,
        "BytesFromText" => Op::BytesFromText,
        "BytesJoin" => Op::BytesJoin,
        "Float64Bits" => Op::Float64Bits,
        "Args" => Op::Args,
        "ArgsFrom" => Op::ArgsFrom,
        "ReadText" => Op::ReadText,
        "WriteText" => Op::WriteText,
        "WriteTextAtomic" => Op::WriteTextAtomic,
        "ReadBytes" => Op::ReadBytes,
        "WriteBytes" => Op::WriteBytes,
        "Environment" => Op::Environment,
        "MakeNone" => Op::MakeNone,
        "MakeSome" => Op::MakeSome,
        "IsSome" => Op::IsSome,
        "MakeOk" => Op::MakeOk,
        "MakeErr" => Op::MakeErr,
        "IsOk" => Op::IsOk,
        "Unwrap" => Op::Unwrap,
        "GetError" => Op::GetError,
        "TryResult" => Op::TryResult,
        "TaskIndex" => Op::TaskIndex,
        "TaskCode" => Op::TaskCode,
        "TaskMessage" => Op::TaskMessage,
        "FileErrorCode" => Op::FileErrorCode,
        "FileErrorMessage" => Op::FileErrorMessage,
        "FileErrorLine" => Op::FileErrorLine,
        "FileErrorColumn" => Op::FileErrorColumn,
        "HttpErrorCode" => Op::HttpErrorCode,
        "HttpErrorMessage" => Op::HttpErrorMessage,
        "HttpGet" => Op::HttpGet,
        "HttpRequest" => Op::HttpRequest,
        "HttpRequestWithTimeout" => Op::HttpRequestWithTimeout,
        "JsonErrorCode" => Op::JsonErrorCode,
        "JsonErrorMessage" => Op::JsonErrorMessage,
        "JsonErrorLine" => Op::JsonErrorLine,
        "JsonErrorColumn" => Op::JsonErrorColumn,
        "JsonIntColumn" => Op::JsonIntColumn,
        "JsonTextCategories" => Op::JsonTextCategories,
        "JsonAsIntList" => Op::JsonAsIntList,
        "JsonProjectColumns" => Op::JsonProjectColumns,
        "JsonTextCategoriesValues" => Op::JsonTextCategoriesValues,
        "Assert" => Op::Assert,
        _ => {
            return Err(format!(
                "unsupported IR opcode `{op_name}` at instruction {pc}"
            ))
        }
    };
    let span = match object.get("span") {
        Some(span) => {
            let span = span
                .as_object()
                .ok_or("instruction.span must be an object")?;
            Span {
                source_id: optional_usize(span.get("source_id"), 0, "span.source_id")? as u32,
                start: optional_usize(span.get("start"), 0, "span.start")?,
                end: optional_usize(span.get("end"), 0, "span.end")?,
                line: optional_usize(span.get("line"), 1, "span.line")?,
                column: optional_usize(span.get("column"), pc + 1, "span.column")?,
            }
        }
        None => Span {
            source_id: 0,
            start: 0,
            end: 0,
            line: 1,
            column: pc + 1,
        },
    };
    Ok(Instruction { op, span })
}

fn jump_target(
    instruction: &serde_json::Map<String, Json>,
    operation: &str,
    labels: &BTreeMap<usize, usize>,
) -> Result<usize, String> {
    if let Some(label) = instruction.get("label_id") {
        let label = required_usize(Some(label), &format!("{operation}.label_id"))?;
        return labels
            .get(&label)
            .copied()
            .ok_or_else(|| format!("{operation} references undefined label {label}"));
    }
    required_usize(instruction.get("target"), &format!("{operation}.target"))
}

fn parse_type(input: &str) -> Result<Type, String> {
    match input.trim() {
        "Int" => Ok(Type::Int),
        "Float64" => Ok(Type::Float64),
        "Bool" => Ok(Type::Bool),
        "Text" => Ok(Type::Text),
        "Bytes" => Ok(Type::Bytes),
        "Json" => Ok(Type::Json),
        "JsonError" => Ok(Type::JsonError),
        "FileError" => Ok(Type::FileError),
        "HttpError" => Ok(Type::HttpError),
        "TaskError" => Ok(Type::TaskError),
        "Unit" => Ok(Type::Unit),
        other if other.starts_with("List[") && other.ends_with(']') => Ok(Type::List(Box::new(
            parse_type(&other[5..other.len() - 1])?,
        ))),
        other if other.starts_with("Map[") && other.ends_with(']') => {
            let inner = &other[4..other.len() - 1];
            let value_type = if let Some(split) = top_level_type_comma(inner) {
                let key_type = inner[..split].trim();
                if key_type != "Text" {
                    return Err(format!("Map key type must be Text, got `{key_type}`"));
                }
                &inner[split + 1..]
            } else {
                inner
            };
            Ok(Type::Map(Box::new(parse_type(value_type)?)))
        }
        other if other.starts_with("Option[") && other.ends_with(']') => Ok(Type::Option(
            Box::new(parse_type(&other[7..other.len() - 1])?),
        )),
        other if other.starts_with("Result[") && other.ends_with(']') => {
            let inner = &other[7..other.len() - 1];
            let split = top_level_type_comma(inner)
                .ok_or_else(|| format!("Result type needs two arguments: `{other}`"))?;
            Ok(Type::Result(
                Box::new(parse_type(inner[..split].trim())?),
                Box::new(parse_type(inner[split + 1..].trim())?),
            ))
        }
        other if other.starts_with("Record[") && other.ends_with(']') => {
            Ok(Type::Record(other[7..other.len() - 1].to_owned()))
        }
        other => Err(format!("unsupported IR type `{other}`")),
    }
}

fn top_level_type_comma(input: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (index, byte) in input.bytes().enumerate() {
        match byte {
            b'[' => depth += 1,
            b']' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => return Some(index),
            _ => {}
        }
    }
    None
}

fn parse_records(value: Option<&Json>) -> Result<Vec<RecordDefinition>, String> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let items = value
        .as_array()
        .ok_or("IR field `records` must be an array")?;
    items
        .iter()
        .enumerate()
        .map(|(index, raw)| {
            let object = raw
                .as_object()
                .ok_or_else(|| format!("record {index} must be an object"))?;
            let name = required_str(object.get("name"), "record.name")?.to_owned();
            let fields = object
                .get("fields")
                .and_then(Json::as_array)
                .ok_or_else(|| format!("record `{name}` field `fields` must be an array"))?
                .iter()
                .map(|raw_field| {
                    let field = raw_field
                        .as_object()
                        .ok_or("record field must be an object")?;
                    Ok(RecordField {
                        name: required_str(field.get("name"), "record field.name")?.to_owned(),
                        ty: parse_type(required_str(field.get("type"), "record field.type")?)?,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok(RecordDefinition { name, fields })
        })
        .collect()
}

fn parse_externals(value: Option<&Json>) -> Result<Vec<ExternalFunction>, String> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let items = value
        .as_array()
        .ok_or("IR field `externals` must be an array")?;
    items
        .iter()
        .enumerate()
        .map(|(index, raw)| {
            let object = raw
                .as_object()
                .ok_or_else(|| format!("external {index} must be an object"))?;
            let name = required_str(object.get("name"), "external.name")?.to_owned();
            let parameters = string_array(object.get("parameters"), "external.parameters")?
                .iter()
                .map(|ty| parse_type(ty))
                .collect::<Result<Vec<_>, _>>()?;
            let parameter_names =
                string_array(object.get("parameter_names"), "external.parameter_names")?;
            if parameters.len() != parameter_names.len() {
                return Err(format!(
                    "external `{name}` parameter names and types have different lengths"
                ));
            }
            Ok(ExternalFunction {
                name,
                module: required_str(object.get("module"), "external.module")?.to_owned(),
                callable: required_str(object.get("callable"), "external.callable")?.to_owned(),
                parameter_names,
                parameters,
                result: parse_type(required_str(object.get("result"), "external.result")?)?,
            })
        })
        .collect()
}

fn required_str<'a>(value: Option<&'a Json>, field: &str) -> Result<&'a str, String> {
    value
        .and_then(Json::as_str)
        .ok_or_else(|| format!("IR field `{field}` must be a string"))
}
fn optional_str(value: Option<&Json>, default: &str) -> Result<String, String> {
    value
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or("IR field `source_name` must be a string".into())
        })
        .unwrap_or_else(|| Ok(default.to_owned()))
}
fn string_array(value: Option<&Json>, field: &str) -> Result<Vec<String>, String> {
    value
        .and_then(Json::as_array)
        .ok_or_else(|| format!("IR field `{field}` must be an array"))?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("IR field `{field}` items must be strings"))
        })
        .collect()
}
fn required_usize(value: Option<&Json>, field: &str) -> Result<usize, String> {
    value
        .and_then(Json::as_u64)
        .and_then(|number| usize::try_from(number).ok())
        .ok_or_else(|| format!("IR field `{field}` must be a non-negative integer"))
}
fn optional_usize(value: Option<&Json>, default: usize, field: &str) -> Result<usize, String> {
    value
        .map(|_| required_usize(value, field))
        .unwrap_or(Ok(default))
}
