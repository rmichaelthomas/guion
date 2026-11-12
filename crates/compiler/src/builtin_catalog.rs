use guion_model::{
    ArrayFunction, CodecFunction, ElementType, FsFunction, MathFunction, NetFunction, Op,
    ProcessFunction, RegexFunction, TlsFunction, Type,
};
use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Clone, Debug)]
pub(crate) struct BuiltinSignature {
    pub parameters: Vec<String>,
    pub result: String,
    pub operation: String,
}

fn signatures() -> &'static HashMap<String, BuiltinSignature> {
    static SIGNATURES: OnceLock<HashMap<String, BuiltinSignature>> = OnceLock::new();
    SIGNATURES.get_or_init(|| {
        let value: serde_json::Value =
            serde_json::from_str(include_str!("../../../language/builtin-signatures.json"))
                .expect("checked-in builtin signature catalog must be valid JSON");
        value["entries"]
            .as_array()
            .expect("builtin catalog entries must be an array")
            .iter()
            .map(|entry| {
                let name = entry["name"]
                    .as_str()
                    .expect("builtin name must be text")
                    .to_string();
                let parameters = entry["parameters"]
                    .as_array()
                    .expect("builtin parameters must be an array")
                    .iter()
                    .map(|value| {
                        value
                            .as_str()
                            .expect("builtin parameter must be text")
                            .to_string()
                    })
                    .collect();
                let signature = BuiltinSignature {
                    parameters,
                    result: entry["result"]
                        .as_str()
                        .expect("builtin result must be text")
                        .to_string(),
                    operation: entry["operation"]
                        .as_str()
                        .expect("builtin operation must be text")
                        .to_string(),
                };
                (name, signature)
            })
            .collect()
    })
}

pub(crate) fn get(name: &str) -> Option<&'static BuiltinSignature> {
    signatures().get(name)
}

fn split_types(input: &str) -> Option<Vec<&str>> {
    let mut pieces = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (index, character) in input.char_indices() {
        match character {
            '[' => depth += 1,
            ']' => depth = depth.checked_sub(1)?,
            ',' if depth == 0 => {
                pieces.push(input[start..index].trim());
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    if depth != 0 {
        return None;
    }
    pieces.push(input[start..].trim());
    Some(pieces)
}

pub(crate) fn concrete_type(spec: &str) -> Option<Type> {
    let simple = match spec {
        "Int" => Some(Type::Int),
        "Float64" => Some(Type::Float64),
        "Bytes" => Some(Type::Bytes),
        "Bool" => Some(Type::Bool),
        "Text" => Some(Type::Text),
        "TaskError" => Some(Type::TaskError),
        "Json" => Some(Type::Json),
        "JsonError" => Some(Type::JsonError),
        "FileError" => Some(Type::FileError),
        "HttpError" => Some(Type::HttpError),
        "FileReader" | "Db" | "Transaction" | "TomlDocument" => {
            Some(Type::NativeResource(spec.to_owned()))
        }
        "Unit" => Some(Type::Unit),
        _ => None,
    };
    if simple.is_some() {
        return simple;
    }
    if spec.starts_with("Map[") && spec.ends_with(']') {
        let parts = split_types(&spec[4..spec.len() - 1])?;
        let item = match parts.as_slice() {
            [item] => *item,
            ["Text", item] => *item,
            _ => return None,
        };
        return Some(Type::Map(Box::new(concrete_type(item)?)));
    }
    for (prefix, constructor) in [("List[", 0u8), ("Option[", 2), ("Record[", 3)] {
        if spec.starts_with(prefix) && spec.ends_with(']') {
            let inner = &spec[prefix.len()..spec.len() - 1];
            if constructor == 3 {
                return Some(Type::Record(inner.to_string()));
            }
            let item = concrete_type(inner)?;
            return Some(match constructor {
                0 => Type::List(Box::new(item)),
                1 => Type::Map(Box::new(item)),
                _ => Type::Option(Box::new(item)),
            });
        }
    }
    if let Some(tag) = spec
        .strip_prefix("Array[")
        .and_then(|rest| rest.strip_suffix(']'))
    {
        return ElementType::from_name(tag).map(Type::Array);
    }
    if spec.starts_with("Result[") && spec.ends_with(']') {
        let parts = split_types(&spec[7..spec.len() - 1])?;
        if parts.len() != 2 {
            return None;
        }
        return Some(Type::Result(
            Box::new(concrete_type(parts[0])?),
            Box::new(concrete_type(parts[1])?),
        ));
    }
    None
}

/// An array catalog operation: `ArrayAdd` (element bound at the call) or `ArrayFull.f64`.
pub(crate) fn array_operation(name: &str) -> Option<(ArrayFunction, Option<ElementType>)> {
    match name.split_once('.') {
        Some((function, element)) => Some((
            ArrayFunction::from_op_name(function)?,
            Some(ElementType::from_name(element)?),
        )),
        None => Some((ArrayFunction::from_op_name(name)?, None)),
    }
}

/// The element variables array signatures use, and the element types each accepts.
pub(crate) fn element_variable_accepts(variable: &str, element: ElementType) -> Option<bool> {
    match variable {
        "$A" => Some(true),
        "$N" => Some(element.is_numeric()),
        "$F" => Some(element.is_float()),
        _ => None,
    }
}

/// Replaces an element variable in a signature type: `Elem[$X]` becomes the element's scalar
/// type and `$X` the element's name.
pub(crate) fn substitute_element(spec: &str, variable: &str, element: ElementType) -> String {
    let scalar = match element.scalar_type() {
        Type::Float64 => "Float64",
        Type::Bool => "Bool",
        _ => "Int",
    };
    spec.replace(&format!("Elem[{variable}]"), scalar)
        .replace(variable, element.name())
}

pub(crate) fn operation(name: &str) -> Option<Op> {
    if name.starts_with("Array") {
        return match array_operation(name)? {
            (function, Some(element)) => Some(Op::Array(function, element)),
            (_, None) => None,
        };
    }
    Some(match name {
        "MakeSome" => Op::MakeSome,
        "MakeOk" => Op::MakeOk,
        "MakeErr" => Op::MakeErr,
        "Length" => Op::Length,
        "ListPush" => Op::ListPush,
        "ListSet" => Op::ListSet,
        "MapSet" => Op::MapSet,
        "MapHas" => Op::MapHas,
        "FloatSqrt" => Op::FloatSqrt,
        "ReadText" => Op::ReadText,
        "ReadBytes" => Op::ReadBytes,
        "WriteText" => Op::WriteText,
        "WriteTextAtomic" => Op::WriteTextAtomic,
        "WriteBytes" => Op::WriteBytes,
        "Unwrap" => Op::Unwrap,
        "GetError" => Op::GetError,
        "IsOk" => Op::IsOk,
        "IsSome" => Op::IsSome,
        "JsonNull" => Op::JsonNull,
        "JsonBool" => Op::JsonBool,
        "JsonInt" => Op::JsonInt,
        "JsonFloat" => Op::JsonFloat,
        "JsonText" => Op::JsonText,
        "JsonArray" => Op::JsonArray,
        "JsonObject" => Op::JsonObject,
        "JsonPush" => Op::JsonPush,
        "JsonSet" => Op::JsonSet,
        "JsonParse" => Op::JsonParse,
        "JsonStringify" => Op::JsonStringify,
        "JsonStringifyPairs" => Op::JsonStringifyPairs,
        "JsonGet" => Op::JsonGet,
        "JsonIndex" => Op::JsonIndex,
        "JsonHas" => Op::JsonHas,
        "JsonLength" => Op::JsonLength,
        "JsonAsText" => Op::JsonAsText,
        "JsonAsInt" => Op::JsonAsInt,
        "JsonAsBool" => Op::JsonAsBool,
        "JsonAsFloat" => Op::JsonAsFloat,
        "JsonAsIntList" => Op::JsonAsIntList,
        "JsonIntColumn" => Op::JsonIntColumn,
        "JsonTextCategories" => Op::JsonTextCategories,
        "JsonProjectColumns" => Op::JsonProjectColumns,
        "JsonTextCategoriesValues" => Op::JsonTextCategoriesValues,
        "JsonIsNull" => Op::JsonIsNull,
        "JsonErrorCode" => Op::JsonErrorCode,
        "JsonErrorMessage" => Op::JsonErrorMessage,
        "JsonErrorLine" => Op::JsonErrorLine,
        "JsonErrorColumn" => Op::JsonErrorColumn,
        "FileErrorCode" => Op::FileErrorCode,
        "FileErrorMessage" => Op::FileErrorMessage,
        "FileErrorLine" => Op::FileErrorLine,
        "FileErrorColumn" => Op::FileErrorColumn,
        "HttpErrorCode" => Op::HttpErrorCode,
        "HttpErrorMessage" => Op::HttpErrorMessage,
        "Args" => Op::Args,
        "ArgsFrom" => Op::ArgsFrom,
        "Environment" => Op::Environment,
        "TaskIndex" => Op::TaskIndex,
        "TaskCode" => Op::TaskCode,
        "TaskMessage" => Op::TaskMessage,
        "HttpGet" => Op::HttpGet,
        "HttpRequest" => Op::HttpRequest,
        "HttpRequestWithTimeout" => Op::HttpRequestWithTimeout,
        "HttpRequestBytes" => Op::HttpRequestBytes,
        "HttpResponseText" => Op::HttpResponseText,
        "HttpResponseJson" => Op::HttpResponseJson,
        "TextChars" => Op::TextChars,
        "TextSlice" => Op::TextSlice,
        "TextJoin" => Op::TextJoin,
        "TextFind" => Op::TextFind,
        "TextSplit" => Op::TextSplit,
        "TextReplace" => Op::TextReplace,
        "TextTrim" => Op::TextTrim,
        "IntToFloat" => Op::IntToFloat,
        "FloatToInt" => Op::FloatToInt,
        "NumberToText" => Op::NumberToText,
        "RandomMix" => Op::RandomMix,
        "RandomSeed" => Op::RandomSeed,
        "RandomAdvance" => Op::RandomAdvance,
        "RandomBits" => Op::RandomBits,
        "RandomUnitFloat" => Op::RandomUnitFloat,
        "RandomBelow" => Op::RandomBelow,
        "BytesFromInts" => Op::BytesFromInts,
        "BytesFromText" => Op::BytesFromText,
        "BytesJoin" => Op::BytesJoin,
        "Float64Bits" => Op::Float64Bits,
        "FileOpenLines" => Op::FileOpenLines,
        "FileNextLine" => Op::FileNextLine,
        "FileClose" => Op::FileClose,
        "CsvOpen" => Op::CsvOpen,
        "CsvNext" => Op::CsvNext,
        "CsvEncode" => Op::CsvEncode,
        "JsonTryGet" => Op::JsonTryGet,
        "JsonTryGetInt" => Op::JsonTryGetInt,
        "JsonTryGetText" => Op::JsonTryGetText,
        "JsonTryIndex" => Op::JsonTryIndex,
        "JsonTryText" => Op::JsonTryText,
        "JsonTryInt" => Op::JsonTryInt,
        "JsonTryBool" => Op::JsonTryBool,
        "JsonTryFloat" => Op::JsonTryFloat,
        "TimeParseDate" => Op::TimeParseDate,
        "TimeParseTimestamp" => Op::TimeParseTimestamp,
        "TimeFormatUtc" => Op::TimeFormatUtc,
        "TimeFormatUtcDate" => Op::TimeFormatUtcDate,
        "UnixTimeMs" => Op::UnixTimeMs,
        "TimeTodayLocal" => Op::TimeTodayLocal,
        "TimeResolveLocal" => Op::TimeResolveLocal,
        "TimeZoneDataVersion" => Op::TimeZoneDataVersion,
        "SemVerParse" => Op::SemVerParse,
        "SemVerCompare" => Op::SemVerCompare,
        "CliParse" => Op::CliParse,
        "FsWalk" => Op::FsWalk,
        "ListGetOption" => Op::ListGetOption,
        "MapGetOption" => Op::MapGetOption,
        "TextParseInt" => Op::TextParseInt,
        "TextParseFloat" => Op::TextParseFloat,
        "TextCaseFold" => Op::TextCaseFold,
        "TextCollapseWhitespace" => Op::TextCollapseWhitespace,
        "TextNormalize" => Op::TextNormalize,
        "CryptoHmacSha256" => Op::CryptoHmacSha256,
        "CryptoVerifyHmacSha256" => Op::CryptoVerifyHmacSha256,
        "SqliteOpen" => Op::SqliteOpen,
        "SqliteBegin" => Op::SqliteBegin,
        "SqliteExecute" => Op::SqliteExecute,
        "SqliteQuery" => Op::SqliteQuery,
        "SqliteCommit" => Op::SqliteCommit,
        "SqliteRollback" => Op::SqliteRollback,
        "SqliteClose" => Op::SqliteClose,
        "TomlParse" => Op::TomlParse,
        "TomlHas" => Op::TomlHas,
        "TomlGetText" => Op::TomlGetText,
        "TomlGetInt" => Op::TomlGetInt,
        "TomlGetBool" => Op::TomlGetBool,
        "TomlPutText" => Op::TomlPutText,
        "TomlPutInt" => Op::TomlPutInt,
        "TomlPutBool" => Op::TomlPutBool,
        "TomlRender" => Op::TomlRender,
        "CalendarParse" => Op::CalendarParse,
        "Assert" => Op::Assert,
        "Print" => Op::Print,
        "PrintErr" => Op::PrintErr,
        "ParallelMap" => return None,
        other => match MathFunction::from_op_name(other) {
            Some(function) => Op::Math(function),
            None => match CodecFunction::from_op_name(other) {
                Some(function) => Op::Codec(function),
                None => match FsFunction::from_op_name(other) {
                    Some(function) => Op::Fs(function),
                    None => match RegexFunction::from_op_name(other) {
                        Some(function) => Op::Regex(function),
                        None => match ProcessFunction::from_op_name(other) {
                            Some(function) => Op::Process(function),
                            None => match NetFunction::from_op_name(other) {
                                Some(function) => Op::Net(function),
                                None => Op::Tls(TlsFunction::from_op_name(other)?),
                            },
                        },
                    },
                },
            },
        },
    })
}

#[cfg(test)]
mod tests {
    use super::{concrete_type, get, operation, signatures};
    use guion_model::{Op, Type};

    #[test]
    fn signatures_are_loaded_from_the_declarative_catalog() {
        let signature = get("json_project_columns").unwrap();
        assert_eq!(signature.parameters, ["Text", "List[Text]", "List[Text]"]);
        assert_eq!(signature.result, "Result[Json, JsonError]");
        assert_eq!(
            operation(&signature.operation),
            Some(Op::JsonProjectColumns)
        );
    }

    #[test]
    fn array_catalog_matches_the_model_operand_table() {
        use guion_model::{ArrayOperand, ElementType};
        let mut checked = 0;
        for (name, signature) in signatures() {
            let Some((function, fixed)) = super::array_operation(&signature.operation) else {
                continue;
            };
            let specs: Vec<&String> = signature
                .parameters
                .iter()
                .chain([&signature.result])
                .collect();
            let variable = ["$A", "$N", "$F"]
                .into_iter()
                .find(|variable| specs.iter().any(|spec| spec.contains(variable)));
            assert!(
                variable.is_some() || fixed.is_some(),
                "{name}: an array builtin needs an element variable or a fixed element"
            );
            for element in ElementType::ALL {
                let accepted = match variable {
                    Some(variable) => super::element_variable_accepts(variable, element).unwrap(),
                    None => Some(element) == fixed,
                };
                let op_element = fixed.unwrap_or(element);
                if fixed.is_none() {
                    assert_eq!(
                        accepted,
                        function.element_class().accepts(element),
                        "{name}: element variable disagrees with the model for {element:?}"
                    );
                }
                if !accepted {
                    continue;
                }
                assert!(function.element_class().accepts(op_element), "{name}");
                let concrete = |spec: &str| {
                    let spec = match variable {
                        Some(variable) => super::substitute_element(spec, variable, element),
                        None => spec.to_string(),
                    };
                    concrete_type(&spec).unwrap_or_else(|| panic!("{name}: cannot parse {spec}"))
                };
                let (operands, result) = function.signature(op_element);
                assert_eq!(operands.len(), signature.parameters.len(), "{name}");
                for (operand, spec) in operands.iter().zip(&signature.parameters) {
                    let actual = concrete(spec);
                    match operand {
                        ArrayOperand::Exact(expected) => {
                            assert_eq!(&actual, expected, "{name} {element:?}")
                        }
                        ArrayOperand::AnyArray => {
                            assert!(matches!(actual, Type::Array(_)), "{name}")
                        }
                    }
                }
                assert_eq!(concrete(&signature.result), result, "{name} {element:?}");
                checked += 1;
            }
        }
        assert!(
            checked > 500,
            "only {checked} array signatures were checked"
        );
    }

    #[test]
    fn concrete_signature_types_parse_nested_generics() {
        assert_eq!(
            concrete_type("Result[List[Int],JsonError]"),
            Some(Type::Result(
                Box::new(Type::List(Box::new(Type::Int))),
                Box::new(Type::JsonError)
            ))
        );
        assert_eq!(
            concrete_type("Map[Text, Text]"),
            Some(Type::Map(Box::new(Type::Text)))
        );
    }

    #[test]
    fn catalog_operations_are_supported_by_the_bootstrap_compiler() {
        for (name, signature) in signatures() {
            assert!(
                operation(&signature.operation).is_some()
                    || super::array_operation(&signature.operation).is_some()
                    || signature.operation == "ParallelMap"
                    || signature.operation == "SortBy"
                    || signature.operation == "SortByIntKey"
                    || signature.operation == "MergeSortedByIntKey",
                "{name}: {}",
                signature.operation
            );
            for parameter in &signature.parameters {
                if !parameter.contains('$')
                    && !parameter.contains('|')
                    && parameter != "Any"
                    && parameter != "Sized"
                    && !parameter.starts_with("NamedWorker[")
                {
                    assert!(
                        concrete_type(parameter).is_some(),
                        "{name}: unknown parameter type {parameter}"
                    );
                }
            }
            if !signature.result.contains('$') && !signature.result.contains('|') {
                assert!(
                    concrete_type(&signature.result).is_some(),
                    "{name}: unknown result type {}",
                    signature.result
                );
            }
        }
    }
}
