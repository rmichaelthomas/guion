//! Artifact verifier: checks structure and types of every function before it runs.

use super::*;

pub fn verify(artifact: &Artifact) -> Result<(), RuntimeError> {
    if artifact.uses_int_map() {
        return verify_erased(&artifact.without_int_maps());
    }
    verify_erased(artifact)
}

fn verify_erased(artifact: &Artifact) -> Result<(), RuntimeError> {
    let fail = |msg: String| RuntimeError::new("R_ARTIFACT_INVALID", msg, Span::default());
    if !(14..=Artifact::CURRENT_VERSION).contains(&artifact.format_version) {
        return Err(RuntimeError::new(
            "R_ARTIFACT_VERSION",
            format!(
                "artifact format {} is unsupported (runtime supports 14 through {})",
                artifact.format_version,
                Artifact::CURRENT_VERSION
            ),
            Span::default(),
        ));
    }
    if artifact.functions.is_empty() {
        return Err(fail("artifact contains no functions".into()));
    }
    let mut source_names = HashSet::new();
    if artifact.sources.is_empty()
        || artifact
            .sources
            .iter()
            .any(|source| source.is_empty() || !source_names.insert(source.as_str()))
        || !artifact.sources.contains(&artifact.source_name)
    {
        return Err(fail("artifact must contain named source files".into()));
    }
    let mut record_names = HashSet::new();
    for record in &artifact.records {
        if record.name.is_empty() || !record_names.insert(record.name.clone()) {
            return Err(fail("record names must be nonempty and unique".into()));
        }
        let mut field_names = HashSet::new();
        if record.fields.is_empty()
            || record
                .fields
                .iter()
                .any(|field| field.name.is_empty() || !field_names.insert(field.name.as_str()))
        {
            return Err(fail(format!("record `{}` has invalid fields", record.name)));
        }
    }
    let reserved_types = [
        "Int",
        "Float64",
        "Bool",
        "Text",
        "Unit",
        "Json",
        "JsonError",
        "FileError",
        "TaskError",
        "Bytes",
        "List",
        "Map",
        "Option",
        "Result",
    ];
    if artifact
        .records
        .iter()
        .any(|record| reserved_types.contains(&record.name.as_str()))
    {
        return Err(fail("record name conflicts with a built-in type".into()));
    }
    if artifact.records.iter().any(|record| {
        record
            .fields
            .iter()
            .any(|field| !artifact_type_known(&field.ty, &record_names))
    }) || artifact.functions.iter().any(|function| {
        !function
            .parameters
            .iter()
            .all(|ty| artifact_type_known(ty, &record_names))
            || !artifact_type_known(&function.result, &record_names)
    }) || artifact.externals.iter().any(|external| {
        !external
            .parameters
            .iter()
            .all(|ty| artifact_type_known(ty, &record_names))
            || !artifact_type_known(&external.result, &record_names)
    }) {
        return Err(fail("artifact references an unknown type".into()));
    }
    if !artifact.functions.iter().any(|f| f.name == artifact.entry) {
        return Err(fail("entry function does not exist".into()));
    }
    let mut names = record_names
        .iter()
        .map(|name| name.as_str())
        .collect::<HashSet<_>>();
    for function in &artifact.functions {
        if function.name.is_empty() || !names.insert(function.name.as_str()) {
            return Err(fail("function names must be nonempty and unique".into()));
        }
    }
    for external in &artifact.externals {
        if external.name.is_empty()
            || external.module.is_empty()
            || external.callable.is_empty()
            || external.parameter_names.len() != external.parameters.len()
            || !names.insert(external.name.as_str())
        {
            return Err(fail(
                "external declarations must have unique names and complete signatures".into(),
            ));
        }
        let mut parameter_names = HashSet::new();
        if external
            .parameter_names
            .iter()
            .any(|name| name.is_empty() || !parameter_names.insert(name.as_str()))
        {
            return Err(fail(format!(
                "external `{}` has invalid parameter names",
                external.name
            )));
        }
    }
    let entry = artifact
        .functions
        .iter()
        .find(|f| f.name == artifact.entry)
        .unwrap();
    if !entry.parameters.is_empty() || !matches!(entry.result, Type::Int | Type::Unit) {
        return Err(fail(
            "entry function must take no parameters and return Int or Unit".into(),
        ));
    }
    for f in &artifact.functions {
        if f.local_count < f.parameters.len() {
            return Err(fail(format!(
                "function `{}` has fewer locals than parameters",
                f.name
            )));
        }
        for i in &f.code {
            if i.span.source_id as usize >= artifact.sources.len() {
                return Err(fail(format!(
                    "function `{}` references a missing source file",
                    f.name
                )));
            }
            match i.op {
                Op::Load(s)
                | Op::Store(s)
                | Op::ListSetLocal(s)
                | Op::ArraySetLocal(s)
                | Op::ListPushLocal(s)
                | Op::JsonPushLocal(s)
                | Op::JsonSetLocal(s)
                    if s >= f.local_count =>
                {
                    return Err(fail(format!(
                        "function `{}` references missing local {s}",
                        f.name
                    )));
                }
                Op::Jump(t) | Op::JumpIfFalse(t) if t > f.code.len() => {
                    return Err(fail(format!(
                        "function `{}` jumps outside its code",
                        f.name
                    )));
                }
                Op::Call(target, argc)
                    if target >= artifact.functions.len()
                        || argc != artifact.functions[target].parameters.len() =>
                {
                    return Err(fail(format!(
                        "function `{}` contains an invalid call",
                        f.name
                    )));
                }
                Op::MakeClosure(target, captured)
                    if target >= artifact.functions.len()
                        || captured > artifact.functions[target].parameters.len() =>
                {
                    return Err(fail(format!(
                        "function `{}` forms a closure of a missing function",
                        f.name
                    )));
                }
                Op::MakeFn(target) if target >= artifact.functions.len() => {
                    return Err(fail(format!(
                        "function `{}` forms a value of a missing function",
                        f.name
                    )));
                }
                Op::ParallelMap(target)
                    if target >= artifact.functions.len()
                        || artifact.functions[target].parameters.len() != 1 =>
                {
                    return Err(fail(format!(
                        "function `{}` contains an invalid parallel worker",
                        f.name
                    )));
                }
                Op::SortBy(target)
                    if target >= artifact.functions.len()
                        || artifact.functions[target].parameters.len() != 2
                        || artifact.functions[target].parameters[0]
                            != artifact.functions[target].parameters[1]
                        || artifact.functions[target].result != Type::Int =>
                {
                    return Err(fail(format!(
                        "function `{}` contains an invalid sort comparator",
                        f.name
                    )));
                }
                Op::SortByKey(target)
                    if target >= artifact.functions.len()
                        || artifact.functions[target].parameters.len() != 1
                        || !is_sort_key_type(&artifact.functions[target].result) =>
                {
                    return Err(fail(format!(
                        "function `{}` contains an invalid sort key function",
                        f.name
                    )));
                }
                Op::SortByIntKey(target)
                    if target >= artifact.functions.len()
                        || artifact.functions[target].parameters.len() != 1
                        || artifact.functions[target].result != Type::Int =>
                {
                    return Err(fail(format!(
                        "function `{}` contains an invalid integer sort key function",
                        f.name
                    )));
                }
                Op::MergeSortedByIntKey(target)
                    if target >= artifact.functions.len()
                        || artifact.functions[target].parameters.len() != 1
                        || artifact.functions[target].result != Type::Int =>
                {
                    return Err(fail(format!(
                        "function `{}` contains an invalid sorted-feed key function",
                        f.name
                    )));
                }
                Op::PythonCall(target, argc)
                    if target >= artifact.externals.len()
                        || argc != artifact.externals[target].parameters.len() =>
                {
                    return Err(fail(format!(
                        "function `{}` contains an invalid Python call",
                        f.name
                    )));
                }
                Op::PythonCallKw(target, positional_count, ref names)
                    if target >= artifact.externals.len()
                        || positional_count > artifact.externals[target].parameters.len()
                        || positional_count + names.len()
                            > artifact.externals[target].parameters.len() =>
                {
                    return Err(fail(format!(
                        "function `{}` contains an invalid Python keyword call",
                        f.name
                    )));
                }
                Op::ReadText
                | Op::WriteText
                | Op::WriteTextAtomic
                | Op::ReadBytes
                | Op::WriteBytes
                | Op::Args
                | Op::ArgsFrom
                | Op::Assert => {}
                _ => {}
            }
        }
        verify_stack(f, artifact).map_err(|m| fail(format!("function `{}`: {m}", f.name)))?;
    }
    guion_model::validate_sort_comparator_effects(artifact).map_err(fail)?;
    Ok(())
}

fn artifact_type_known(ty: &Type, record_names: &HashSet<guion_model::RecordName>) -> bool {
    match ty {
        Type::Record(name) => record_names.contains(name.as_str()),
        Type::NativeResource(name) => matches!(
            name.as_str(),
            "FileReader" | "Db" | "Transaction" | "TomlDocument"
        ),
        Type::List(item) | Type::Map(item) | Type::IntMap(item) | Type::Option(item) => {
            artifact_type_known(item, record_names)
        }
        Type::Result(ok, err) => {
            artifact_type_known(ok, record_names) && artifact_type_known(err, record_names)
        }
        Type::Int
        | Type::Float64
        | Type::Bool
        | Type::Text
        | Type::Bytes
        | Type::TaskError
        | Type::Json
        | Type::JsonError
        | Type::FileError
        | Type::HttpError
        | Type::Array(_)
        | Type::Unit => true,
        Type::Channel(item) => artifact_type_known(item, record_names),
        Type::RecordApp(_, _) | Type::Task(_) => false,
        Type::Fn(parameters, result) => {
            parameters
                .iter()
                .all(|parameter| artifact_type_known(parameter, record_names))
                && artifact_type_known(result, record_names)
        }
    }
}

fn type_contains_native_resource(ty: &Type) -> bool {
    match ty {
        Type::NativeResource(_) => true,
        Type::List(item) | Type::Map(item) | Type::IntMap(item) | Type::Option(item) => {
            type_contains_native_resource(item)
        }
        Type::Result(ok, err) => {
            type_contains_native_resource(ok) || type_contains_native_resource(err)
        }
        _ => false,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlowState {
    pub stack: Vec<Type>,
    pub locals: Vec<Option<Type>>,
}

fn pop_type(stack: &mut Vec<Type>, pc: usize) -> Result<Type, String> {
    stack
        .pop()
        .ok_or_else(|| format!("stack underflow at instruction {pc}"))
}

fn value_type(value: &Value) -> Option<Type> {
    Some(match value {
        Value::Int(_) => Type::Int,
        Value::Float64(_) => Type::Float64,
        Value::Record(name, _) => Type::Record(name.to_string()),
        Value::NativeResource(resource) => Type::NativeResource(resource.kind().to_owned()),
        Value::Bool(_) => Type::Bool,
        Value::Text(_) => Type::Text,
        Value::Bytes(_) => Type::Bytes,
        Value::Array(array) => Type::Array(array.element()),
        Value::Unit => Type::Unit,
        Value::Task(_) | Value::Channel(_) => return None,
        Value::List(items) => {
            let first = items.first()?;
            let item_type = value_type(first)?;
            if items
                .iter()
                .skip(1)
                .any(|item| value_type(item).as_ref() != Some(&item_type))
            {
                return None;
            }
            Type::List(Box::new(item_type))
        }
        Value::Float64List(_) => Type::List(Box::new(Type::Float64)),
        Value::Map(items) => {
            let first = items.iter_unordered().next()?.1;
            let item_type = value_type(first)?;
            if items
                .iter_unordered()
                .any(|(_, item)| value_type(item).as_ref() != Some(&item_type))
            {
                return None;
            }
            Type::Map(Box::new(item_type))
        }
        Value::OptionNone => return None,
        Value::OptionSome(value) => Type::Option(Box::new(value_type(value)?)),
        Value::ResultOk(value) => {
            Type::Result(Box::new(value_type(value)?), Box::new(Type::TaskError))
        }
        Value::ResultErr(value) => Type::Result(Box::new(Type::Unit), Box::new(value_type(value)?)),
        Value::TaskError(_) => Type::TaskError,
        Value::Json(_) => Type::Json,
        Value::JsonError(_) => Type::JsonError,
        Value::FileError(_) => Type::FileError,
        Value::HttpError(_) => Type::HttpError,
        // A function value is never a constant; the verifier only learns its type from the
        // function table (`MakeFn`).
        Value::Function(_) | Value::Closure(..) => return None,
    })
}

fn verify_stack(function: &guion_model::Function, artifact: &Artifact) -> Result<(), String> {
    if function.code.is_empty() {
        return Err("function has no instructions".into());
    }
    let mut initial = FlowState {
        stack: Vec::new(),
        locals: vec![None; function.local_count],
    };
    for (slot, ty) in function.parameters.iter().enumerate() {
        initial.locals[slot] = Some(ty.clone());
    }
    let mut states = vec![None; function.code.len()];
    let mut pending = VecDeque::new();
    states[0] = Some(initial);
    pending.push_back(0usize);
    let mut has_return = false;
    while let Some(pc) = pending.pop_front() {
        let instruction = &function.code[pc];
        let mut state = states[pc].clone().unwrap();
        let mut successors = Vec::new();
        match &instruction.op {
            Op::Const(value) => {
                let ty = value_type(value).ok_or_else(|| {
                    format!("invalid or heterogeneous constant at instruction {pc}")
                })?;
                if !value_matches(value, &ty, &artifact.records) {
                    return Err(format!(
                        "constant violates its record schema at instruction {pc}"
                    ));
                }
                state.stack.push(ty);
            }
            Op::Load(slot) => state.stack.push(
                state
                    .locals
                    .get(*slot)
                    .and_then(Clone::clone)
                    .ok_or_else(|| format!("read of uninitialized local at instruction {pc}"))?,
            ),
            Op::Store(slot) => {
                let ty = pop_type(&mut state.stack, pc)?;
                state.locals[*slot] = Some(ty);
            }
            Op::ListPushLocal(slot) => {
                let value = pop_type(&mut state.stack, pc)?;
                let Type::List(item) = state
                    .locals
                    .get(*slot)
                    .and_then(Option::as_ref)
                    .ok_or_else(|| format!("list_push reads an uninitialized local at {pc}"))?
                else {
                    return Err(format!("list_push local is not a List at instruction {pc}"));
                };
                if item.as_ref() != &value {
                    return Err(format!("list_push item type mismatch at instruction {pc}"));
                }
            }
            Op::JsonPushLocal(slot) => {
                if pop_type(&mut state.stack, pc)? != Type::Json {
                    return Err(format!("json_push value must be Json at instruction {pc}"));
                }
                if state.locals.get(*slot).and_then(Option::as_ref) != Some(&Type::Json) {
                    return Err(format!("json_push local is not Json at instruction {pc}"));
                }
            }
            Op::JsonSetLocal(slot) => {
                if pop_type(&mut state.stack, pc)? != Type::Json {
                    return Err(format!("json_set value must be Json at instruction {pc}"));
                }
                if pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!("json_set key must be Text at instruction {pc}"));
                }
                if state.locals.get(*slot).and_then(Option::as_ref) != Some(&Type::Json) {
                    return Err(format!("json_set local is not Json at instruction {pc}"));
                }
            }
            Op::ListSetLocal(slot) => {
                let value = pop_type(&mut state.stack, pc)?;
                if pop_type(&mut state.stack, pc)? != Type::Int {
                    return Err(format!("list_set index requires Int at instruction {pc}"));
                }
                let Type::List(item) = state
                    .locals
                    .get(*slot)
                    .and_then(Option::as_ref)
                    .ok_or_else(|| format!("list_set reads an uninitialized local at {pc}"))?
                else {
                    return Err(format!("list_set local is not a List at instruction {pc}"));
                };
                if item.as_ref() != &value {
                    return Err(format!("list_set item type mismatch at instruction {pc}"));
                }
            }
            Op::ArraySetLocal(slot) => {
                let value = pop_type(&mut state.stack, pc)?;
                if pop_type(&mut state.stack, pc)? != Type::Int {
                    return Err(format!("array_set index requires Int at instruction {pc}"));
                }
                let Some(Type::Array(element)) = state.locals.get(*slot).and_then(Option::as_ref)
                else {
                    return Err(format!(
                        "array_set local is not an array at instruction {pc}"
                    ));
                };
                if element.scalar_type() != value {
                    return Err(format!("array_set value type mismatch at instruction {pc}"));
                }
            }
            Op::Pop | Op::Print | Op::PrintErr => {
                pop_type(&mut state.stack, pc)?;
            }
            Op::Assert => {
                if pop_type(&mut state.stack, pc)? != Type::Bool {
                    return Err(format!("assert requires Bool at instruction {pc}"));
                }
                state.stack.push(Type::Unit);
            }
            Op::IsOk | Op::GetError => {
                let Type::Result(ok, err) = pop_type(&mut state.stack, pc)? else {
                    return Err(format!(
                        "result operation requires Result at instruction {pc}"
                    ));
                };
                if matches!(instruction.op, Op::IsOk) {
                    state.stack.push(Type::Bool);
                } else {
                    state.stack.push(*err);
                }
                let _ = ok;
            }
            Op::IsSome => {
                if !matches!(pop_type(&mut state.stack, pc)?, Type::Option(_)) {
                    return Err(format!("is_some requires Option at instruction {pc}"));
                }
                state.stack.push(Type::Bool);
            }
            Op::Unwrap => match pop_type(&mut state.stack, pc)? {
                Type::Result(ok, _) | Type::Option(ok) => state.stack.push(*ok),
                _ => {
                    return Err(format!(
                        "unwrap requires Result or Option at instruction {pc}"
                    ));
                }
            },
            Op::MakeOk | Op::MakeErr => {
                let Type::Result(ok, err) = &function.result else {
                    return Err(format!(
                        "Result constructor outside Result-returning function at instruction {pc}"
                    ));
                };
                let expected = if matches!(instruction.op, Op::MakeOk) {
                    ok.as_ref()
                } else {
                    err.as_ref()
                };
                if pop_type(&mut state.stack, pc)? != *expected {
                    return Err(format!(
                        "Result constructor payload type mismatch at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Result(ok.clone(), err.clone()));
            }
            Op::MakeNone | Op::MakeSome => {
                let Type::Option(item) = &function.result else {
                    return Err(format!(
                        "Option constructor outside Option-returning function at instruction {pc}"
                    ));
                };
                if matches!(instruction.op, Op::MakeSome)
                    && pop_type(&mut state.stack, pc)? != **item
                {
                    return Err(format!(
                        "Option constructor payload mismatch at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Option(item.clone()));
            }
            Op::TryResult => {
                let Type::Result(ok, err) = pop_type(&mut state.stack, pc)? else {
                    return Err(format!("`?` requires Result at instruction {pc}"));
                };
                let Type::Result(_, function_error) = &function.result else {
                    return Err(format!(
                        "`?` outside Result-returning function at instruction {pc}"
                    ));
                };
                if err.as_ref() != function_error.as_ref() {
                    return Err(format!("`?` error type mismatch at instruction {pc}"));
                }
                state.stack.push(*ok);
            }
            Op::TaskIndex | Op::TaskCode | Op::TaskMessage => {
                if pop_type(&mut state.stack, pc)? != Type::TaskError {
                    return Err(format!(
                        "task error accessor requires TaskError at instruction {pc}"
                    ));
                }
                state.stack.push(if instruction.op == Op::TaskIndex {
                    Type::Int
                } else {
                    Type::Text
                });
            }
            Op::JsonNull | Op::JsonArray | Op::JsonObject => {
                state.stack.push(Type::Json);
            }
            Op::JsonBool => {
                if pop_type(&mut state.stack, pc)? != Type::Bool {
                    return Err(format!("json_bool requires Bool at instruction {pc}"));
                }
                state.stack.push(Type::Json);
            }
            Op::JsonInt => {
                if pop_type(&mut state.stack, pc)? != Type::Int {
                    return Err(format!("json_int requires Int at instruction {pc}"));
                }
                state.stack.push(Type::Json);
            }
            Op::JsonFloat => {
                if pop_type(&mut state.stack, pc)? != Type::Float64 {
                    return Err(format!("json_float requires Float64 at instruction {pc}"));
                }
                state.stack.push(Type::Json);
            }
            Op::JsonText => {
                if pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!("json_text requires Text at instruction {pc}"));
                }
                state.stack.push(Type::Json);
            }
            Op::JsonPush => {
                if pop_type(&mut state.stack, pc)? != Type::Json
                    || pop_type(&mut state.stack, pc)? != Type::Json
                {
                    return Err(format!(
                        "json_push requires Json and Json at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Json);
            }
            Op::JsonSet => {
                if pop_type(&mut state.stack, pc)? != Type::Json
                    || pop_type(&mut state.stack, pc)? != Type::Text
                    || pop_type(&mut state.stack, pc)? != Type::Json
                {
                    return Err(format!(
                        "json_set requires Json, Text and Json at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Json);
            }
            Op::JsonParse => {
                if pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!("json_parse requires Text at instruction {pc}"));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Json),
                    Box::new(Type::JsonError),
                ));
            }
            Op::JsonStringify => {
                if pop_type(&mut state.stack, pc)? != Type::Json {
                    return Err(format!("json_stringify requires Json at instruction {pc}"));
                }
                state.stack.push(Type::Text);
            }
            Op::JsonStringifyPairs => {
                if pop_type(&mut state.stack, pc)? != Type::Text
                    || pop_type(&mut state.stack, pc)? != Type::Text
                    || pop_type(&mut state.stack, pc)?
                        != Type::Map(Box::new(Type::List(Box::new(Type::Text))))
                {
                    return Err(format!(
                        "json_stringify_pairs requires Map[Text, List[Text]], Text and Text at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Text);
            }
            Op::JsonGet | Op::JsonHas => {
                if pop_type(&mut state.stack, pc)? != Type::Text
                    || pop_type(&mut state.stack, pc)? != Type::Json
                {
                    return Err(format!(
                        "JSON key operation requires Json and Text at instruction {pc}"
                    ));
                }
                state.stack.push(if matches!(instruction.op, Op::JsonHas) {
                    Type::Bool
                } else {
                    Type::Json
                });
            }
            Op::JsonIndex => {
                if pop_type(&mut state.stack, pc)? != Type::Int
                    || pop_type(&mut state.stack, pc)? != Type::Json
                {
                    return Err(format!(
                        "json_index requires Json and Int at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Json);
            }
            Op::JsonLength => {
                if pop_type(&mut state.stack, pc)? != Type::Json {
                    return Err(format!("json_len requires Json at instruction {pc}"));
                }
                state.stack.push(Type::Int);
            }
            Op::JsonIntColumn | Op::JsonTextCategories => {
                if pop_type(&mut state.stack, pc)? != Type::Text
                    || pop_type(&mut state.stack, pc)? != Type::Json
                {
                    return Err(format!(
                        "JSON column operation requires Json and Text at instruction {pc}"
                    ));
                }
                state
                    .stack
                    .push(if matches!(instruction.op, Op::JsonIntColumn) {
                        Type::List(Box::new(Type::Int))
                    } else {
                        Type::Json
                    });
            }
            Op::JsonAsIntList => {
                if pop_type(&mut state.stack, pc)? != Type::Json {
                    return Err(format!(
                        "json_as_int_list requires Json at instruction {pc}"
                    ));
                }
                state.stack.push(Type::List(Box::new(Type::Int)));
            }
            Op::JsonProjectColumns => {
                let text_list = Type::List(Box::new(Type::Text));
                if pop_type(&mut state.stack, pc)? != text_list
                    || pop_type(&mut state.stack, pc)? != Type::List(Box::new(Type::Text))
                    || pop_type(&mut state.stack, pc)? != Type::Text
                {
                    return Err(format!(
                        "json_project_columns requires Text, List[Text], and List[Text] at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Json),
                    Box::new(Type::JsonError),
                ));
            }
            Op::JsonTextCategoriesValues => {
                if pop_type(&mut state.stack, pc)? != Type::Json {
                    return Err(format!(
                        "json_text_categories_values requires Json at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Json);
            }
            Op::JsonTryGet | Op::JsonTryIndex => {
                let expected = if instruction.op == Op::JsonTryGet {
                    Type::Text
                } else {
                    Type::Int
                };
                if pop_type(&mut state.stack, pc)? != expected
                    || pop_type(&mut state.stack, pc)? != Type::Json
                {
                    return Err(format!(
                        "JSON accessor argument types do not match at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Json),
                    Box::new(Type::JsonError),
                ));
            }
            Op::JsonTryGetText | Op::JsonTryGetInt => {
                if pop_type(&mut state.stack, pc)? != Type::Text
                    || pop_type(&mut state.stack, pc)? != Type::Json
                {
                    return Err(format!(
                        "JSON accessor argument types do not match at instruction {pc}"
                    ));
                }
                let result = if instruction.op == Op::JsonTryGetText {
                    Type::Text
                } else {
                    Type::Int
                };
                state
                    .stack
                    .push(Type::Result(Box::new(result), Box::new(Type::JsonError)));
            }
            Op::JsonTryText | Op::JsonTryInt | Op::JsonTryBool | Op::JsonTryFloat => {
                if pop_type(&mut state.stack, pc)? != Type::Json {
                    return Err(format!("JSON conversion requires Json at instruction {pc}"));
                }
                let result = match instruction.op {
                    Op::JsonTryText => Type::Text,
                    Op::JsonTryInt => Type::Int,
                    Op::JsonTryBool => Type::Bool,
                    Op::JsonTryFloat => Type::Float64,
                    _ => unreachable!(),
                };
                state
                    .stack
                    .push(Type::Result(Box::new(result), Box::new(Type::JsonError)));
            }
            Op::JsonAsText | Op::JsonAsInt | Op::JsonAsFloat | Op::JsonAsBool | Op::JsonIsNull => {
                if pop_type(&mut state.stack, pc)? != Type::Json {
                    return Err(format!("JSON conversion requires Json at instruction {pc}"));
                }
                state.stack.push(match instruction.op {
                    Op::JsonAsText => Type::Text,
                    Op::JsonAsInt => Type::Int,
                    Op::JsonAsFloat => Type::Float64,
                    Op::JsonAsBool | Op::JsonIsNull => Type::Bool,
                    _ => unreachable!(),
                });
            }
            Op::JsonErrorMessage | Op::JsonErrorColumn | Op::JsonErrorLine | Op::JsonErrorCode => {
                if pop_type(&mut state.stack, pc)? != Type::JsonError {
                    return Err(format!(
                        "JSON error accessor requires JsonError at instruction {pc}"
                    ));
                }
                state.stack.push(match instruction.op {
                    Op::JsonErrorColumn | Op::JsonErrorLine => Type::Int,
                    _ => Type::Text,
                });
            }
            Op::FileErrorCode | Op::FileErrorMessage | Op::FileErrorLine | Op::FileErrorColumn => {
                if pop_type(&mut state.stack, pc)? != Type::FileError {
                    return Err(format!(
                        "file error accessor requires FileError at instruction {pc}"
                    ));
                }
                state.stack.push(
                    if matches!(instruction.op, Op::FileErrorLine | Op::FileErrorColumn) {
                        Type::Int
                    } else {
                        Type::Text
                    },
                );
            }
            Op::HttpErrorCode | Op::HttpErrorMessage => {
                if pop_type(&mut state.stack, pc)? != Type::HttpError {
                    return Err(format!(
                        "HTTP error accessor requires HttpError at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Text);
            }
            Op::Add => {
                let right = pop_type(&mut state.stack, pc)?;
                let left = pop_type(&mut state.stack, pc)?;
                if left != right || !matches!(left, Type::Int | Type::Text | Type::Float64) {
                    return Err(format!("invalid operands to add at instruction {pc}"));
                }
                state.stack.push(left);
            }
            Op::Subtract | Op::Multiply => {
                let right = pop_type(&mut state.stack, pc)?;
                let left = pop_type(&mut state.stack, pc)?;
                if left != right || !matches!(left, Type::Int | Type::Float64) {
                    return Err(format!(
                        "arithmetic requires matching numeric types at instruction {pc}"
                    ));
                }
                state.stack.push(left);
            }
            Op::Divide => {
                if pop_type(&mut state.stack, pc)? != Type::Float64
                    || pop_type(&mut state.stack, pc)? != Type::Float64
                {
                    return Err(format!(
                        "Float64 division requires Float64 at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Float64);
            }
            Op::FloatSqrt => {
                if pop_type(&mut state.stack, pc)? != Type::Float64 {
                    return Err(format!("sqrt requires Float64 at instruction {pc}"));
                }
                state.stack.push(Type::Float64);
            }
            Op::Array(function, element) => {
                if !function.element_class().accepts(*element) {
                    return Err(format!(
                        "{} does not accept {} arrays at instruction {pc}",
                        function.op_name(),
                        element.name()
                    ));
                }
                let (operands, result) = function.signature(*element);
                for operand in operands.iter().rev() {
                    let actual = pop_type(&mut state.stack, pc)?;
                    let matches = match operand {
                        guion_model::ArrayOperand::Exact(expected) => actual == *expected,
                        guion_model::ArrayOperand::AnyArray => matches!(actual, Type::Array(_)),
                    };
                    if !matches {
                        return Err(format!(
                            "{} expects {operand:?}, found {actual:?} at instruction {pc}",
                            function.op_name()
                        ));
                    }
                }
                state.stack.push(result);
            }
            Op::Math(function) => {
                for _ in 0..function.arity() {
                    if pop_type(&mut state.stack, pc)? != Type::Float64 {
                        return Err(format!(
                            "math functions require Float64 at instruction {pc}"
                        ));
                    }
                }
                state.stack.push(Type::Float64);
            }
            Op::Codec(function) => {
                let (parameters, result) = codec_signature(*function);
                for expected in parameters.iter().rev() {
                    if pop_type(&mut state.stack, pc)? != *expected {
                        return Err(format!(
                            "{} received an operand of the wrong type at instruction {pc}",
                            function.op_name()
                        ));
                    }
                }
                state.stack.push(result);
            }
            Op::Crypto(function) => {
                let (parameters, result) = crypto_signature(*function);
                for expected in parameters.iter().rev() {
                    if pop_type(&mut state.stack, pc)? != *expected {
                        return Err(format!(
                            "{} received an operand of the wrong type at instruction {pc}",
                            function.op_name()
                        ));
                    }
                }
                state.stack.push(result);
            }
            Op::Tls(function) => {
                let expected: &[Type] = match function {
                    guion_model::TlsFunction::Connect => {
                        &[Type::Text, Type::Int, Type::Int, Type::Text]
                    }
                    guion_model::TlsFunction::Accept => {
                        &[Type::Int, Type::Int, Type::Text, Type::Text]
                    }
                    guion_model::TlsFunction::ConnectIdentity => &[
                        Type::Text,
                        Type::Int,
                        Type::Int,
                        Type::Text,
                        Type::Text,
                        Type::Text,
                    ],
                    guion_model::TlsFunction::AcceptVerified => {
                        &[Type::Int, Type::Int, Type::Text, Type::Text, Type::Text]
                    }
                };
                for expected in expected.iter().rev() {
                    if pop_type(&mut state.stack, pc)? != *expected {
                        return Err(format!(
                            "{} received an operand of the wrong type at instruction {pc}",
                            function.op_name()
                        ));
                    }
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Int),
                    Box::new(Type::List(Box::new(Type::Text))),
                ));
            }
            Op::Net(function) => {
                let (parameters, ok) = net_signature(*function);
                for expected in parameters.iter().rev() {
                    if pop_type(&mut state.stack, pc)? != *expected {
                        return Err(format!(
                            "{} received an operand of the wrong type at instruction {pc}",
                            function.op_name()
                        ));
                    }
                }
                state.stack.push(Type::Result(
                    Box::new(ok),
                    Box::new(Type::List(Box::new(Type::Text))),
                ));
            }
            Op::Process(function) => {
                let texts = Type::List(Box::new(Type::Text));
                match function {
                    guion_model::ProcessFunction::Run => {
                        let stdin = pop_type(&mut state.stack, pc)?;
                        let arguments = pop_type(&mut state.stack, pc)?;
                        let command = pop_type(&mut state.stack, pc)?;
                        if stdin != Type::Text || arguments != texts || command != Type::Text {
                            return Err(format!(
                                "process_run requires Text, List[Text], Text operands at instruction {pc}"
                            ));
                        }
                        state
                            .stack
                            .push(Type::Result(Box::new(texts.clone()), Box::new(texts)));
                    }
                    guion_model::ProcessFunction::Pid | guion_model::ProcessFunction::CoreCount => {
                        state.stack.push(Type::Int)
                    }
                    guion_model::ProcessFunction::SignalPoll => state.stack.push(Type::Text),
                    guion_model::ProcessFunction::Start => {
                        let arguments = pop_type(&mut state.stack, pc)?;
                        let command = pop_type(&mut state.stack, pc)?;
                        if arguments != texts || command != Type::Text {
                            return Err(format!(
                                "start_process requires Text and List[Text] operands at instruction {pc}"
                            ));
                        }
                        state
                            .stack
                            .push(Type::Result(Box::new(Type::Int), Box::new(texts)));
                    }
                    guion_model::ProcessFunction::Line => {
                        if pop_type(&mut state.stack, pc)? != Type::Int {
                            return Err(format!(
                                "process_line requires an Int handle at instruction {pc}"
                            ));
                        }
                        state.stack.push(Type::Option(Box::new(Type::Text)));
                    }
                    guion_model::ProcessFunction::Write => {
                        let text = pop_type(&mut state.stack, pc)?;
                        let handle = pop_type(&mut state.stack, pc)?;
                        if text != Type::Text || handle != Type::Int {
                            return Err(format!(
                                "process_write requires an Int handle and Text at instruction {pc}"
                            ));
                        }
                        state
                            .stack
                            .push(Type::Result(Box::new(Type::Unit), Box::new(texts)));
                    }
                    guion_model::ProcessFunction::CloseInput => {
                        if pop_type(&mut state.stack, pc)? != Type::Int {
                            return Err(format!(
                                "process_close_input requires an Int handle at instruction {pc}"
                            ));
                        }
                        state
                            .stack
                            .push(Type::Result(Box::new(Type::Unit), Box::new(texts)));
                    }
                    guion_model::ProcessFunction::Wait => {
                        if pop_type(&mut state.stack, pc)? != Type::Int {
                            return Err(format!(
                                "process_wait requires an Int handle at instruction {pc}"
                            ));
                        }
                        state
                            .stack
                            .push(Type::Result(Box::new(texts.clone()), Box::new(texts)));
                    }
                }
            }
            Op::Regex(function) => {
                for _ in 0..function.arity() {
                    if pop_type(&mut state.stack, pc)? != Type::Text {
                        return Err(format!(
                            "{} requires Text operands at instruction {pc}",
                            function.op_name()
                        ));
                    }
                }
                state.stack.push(regex_result_type(*function));
            }
            Op::Fs(function) => {
                for _ in 0..function.arity() {
                    if pop_type(&mut state.stack, pc)? != Type::Text {
                        return Err(format!(
                            "{} requires Text paths at instruction {pc}",
                            function.op_name()
                        ));
                    }
                }
                state.stack.push(fs_result_type(*function));
            }
            Op::IntToFloat => {
                if pop_type(&mut state.stack, pc)? != Type::Int {
                    return Err(format!("int_to_float requires Int at instruction {pc}"));
                }
                state.stack.push(Type::Float64);
            }
            Op::FloatToInt => {
                if pop_type(&mut state.stack, pc)? != Type::Float64 {
                    return Err(format!("float_to_int requires Float64 at instruction {pc}"));
                }
                state.stack.push(Type::Int);
            }
            Op::NumberToText => {
                if !matches!(pop_type(&mut state.stack, pc)?, Type::Int | Type::Float64) {
                    return Err(format!(
                        "number-to-text conversion requires Int or Float64 at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Text);
            }
            Op::RandomMix | Op::RandomSeed | Op::RandomAdvance | Op::RandomBits => {
                if pop_type(&mut state.stack, pc)? != Type::Int {
                    return Err(format!("random state requires Int at instruction {pc}"));
                }
                state.stack.push(Type::Int);
            }
            Op::RandomUnitFloat => {
                if pop_type(&mut state.stack, pc)? != Type::Int {
                    return Err(format!("random state requires Int at instruction {pc}"));
                }
                state.stack.push(Type::Float64);
            }
            Op::RandomBelow => {
                if pop_type(&mut state.stack, pc)? != Type::Int
                    || pop_type(&mut state.stack, pc)? != Type::Int
                {
                    return Err(format!(
                        "random_below requires Int operands at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Int);
            }
            Op::ListPush => {
                let value = pop_type(&mut state.stack, pc)?;
                match pop_type(&mut state.stack, pc)? {
                    Type::List(item) if *item == value => state.stack.push(Type::List(item)),
                    _ => {
                        return Err(format!(
                            "list_push requires a matching List item at instruction {pc}"
                        ));
                    }
                }
            }
            Op::ListSet => {
                let value = pop_type(&mut state.stack, pc)?;
                if pop_type(&mut state.stack, pc)? != Type::Int {
                    return Err(format!("list_set index requires Int at instruction {pc}"));
                }
                match pop_type(&mut state.stack, pc)? {
                    Type::List(item) if *item == value => state.stack.push(Type::List(item)),
                    _ => {
                        return Err(format!(
                            "list_set requires a matching List item at instruction {pc}"
                        ));
                    }
                }
            }
            Op::IntegerDivide | Op::FloorDivide | Op::Remainder => {
                if pop_type(&mut state.stack, pc)? != Type::Int
                    || pop_type(&mut state.stack, pc)? != Type::Int
                {
                    return Err(format!("integer division requires Int at instruction {pc}"));
                }
                state.stack.push(Type::Int);
            }
            Op::Negate => {
                let ty = pop_type(&mut state.stack, pc)?;
                if !matches!(ty, Type::Int | Type::Float64) {
                    return Err(format!("negation requires Int at instruction {pc}"));
                }
                state.stack.push(ty);
            }
            Op::Not => {
                if pop_type(&mut state.stack, pc)? != Type::Bool {
                    return Err(format!("not requires Bool at instruction {pc}"));
                }
                state.stack.push(Type::Bool);
            }
            Op::Equal | Op::NotEqual => {
                let right = pop_type(&mut state.stack, pc)?;
                let left = pop_type(&mut state.stack, pc)?;
                if left != right {
                    return Err(format!(
                        "equality operands have different types at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Bool);
            }
            Op::Less | Op::LessEqual | Op::Greater | Op::GreaterEqual => {
                let right = pop_type(&mut state.stack, pc)?;
                let left = pop_type(&mut state.stack, pc)?;
                if left != right || !matches!(left, Type::Int | Type::Text | Type::Float64) {
                    return Err(format!("invalid comparison operands at instruction {pc}"));
                }
                state.stack.push(Type::Bool);
            }
            Op::Length => {
                if !matches!(
                    pop_type(&mut state.stack, pc)?,
                    Type::List(_) | Type::Map(_) | Type::Bytes | Type::Text
                ) {
                    return Err(format!(
                        "len requires List, Map, Bytes, or Text at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Int);
            }
            Op::TextChars => {
                if pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!("text_chars requires Text at instruction {pc}"));
                }
                state.stack.push(Type::List(Box::new(Type::Text)));
            }
            Op::TextSlice => {
                if pop_type(&mut state.stack, pc)? != Type::Int
                    || pop_type(&mut state.stack, pc)? != Type::Int
                    || pop_type(&mut state.stack, pc)? != Type::Text
                {
                    return Err(format!(
                        "text_slice requires Text and Int start/end bounds at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Text);
            }
            Op::TextJoin => {
                let delimiter = pop_type(&mut state.stack, pc)?;
                let values = pop_type(&mut state.stack, pc)?;
                if delimiter != Type::Text || values != Type::List(Box::new(Type::Text)) {
                    return Err(format!(
                        "text_join_native requires List[Text] and Text at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Text);
            }
            Op::TextFind => {
                if pop_type(&mut state.stack, pc)? != Type::Int
                    || pop_type(&mut state.stack, pc)? != Type::Text
                    || pop_type(&mut state.stack, pc)? != Type::Text
                {
                    return Err(format!(
                        "text_find requires Text, Text and an Int start at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Int);
            }
            Op::TextSplit => {
                if pop_type(&mut state.stack, pc)? != Type::Text
                    || pop_type(&mut state.stack, pc)? != Type::Text
                {
                    return Err(format!(
                        "text_split_native requires Text and Text at instruction {pc}"
                    ));
                }
                state.stack.push(Type::List(Box::new(Type::Text)));
            }
            Op::TextReplace => {
                if pop_type(&mut state.stack, pc)? != Type::Text
                    || pop_type(&mut state.stack, pc)? != Type::Text
                    || pop_type(&mut state.stack, pc)? != Type::Text
                {
                    return Err(format!(
                        "text_replace_native requires three Text arguments at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Text);
            }
            Op::TextTrim => {
                if pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!(
                        "text_trim_native requires Text at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Text);
            }
            Op::BytesFromInts => {
                if pop_type(&mut state.stack, pc)? != Type::List(Box::new(Type::Int)) {
                    return Err(format!(
                        "bytes_from_ints requires List[Int] at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Bytes);
            }
            Op::BytesFromText => {
                if pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!("bytes_from_text requires Text at instruction {pc}"));
                }
                state.stack.push(Type::Bytes);
            }
            Op::BytesJoin => {
                if pop_type(&mut state.stack, pc)? != Type::List(Box::new(Type::Bytes)) {
                    return Err(format!(
                        "bytes_join requires List[Bytes] at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Bytes);
            }
            Op::Float64Bits => {
                if pop_type(&mut state.stack, pc)? != Type::Float64 {
                    return Err(format!("float64_bits requires Float64 at instruction {pc}"));
                }
                state.stack.push(Type::Int);
            }
            Op::Index => {
                let index = pop_type(&mut state.stack, pc)?;
                match pop_type(&mut state.stack, pc)? {
                    Type::List(item) if index == Type::Int => state.stack.push(*item),
                    Type::Map(item) if index == Type::Text => state.stack.push(*item),
                    Type::Text if index == Type::Int => state.stack.push(Type::Text),
                    _ => {
                        return Err(format!(
                            "index requires List[Int], Map[Text, T], or Text[Int] at instruction {pc}"
                        ));
                    }
                }
            }
            Op::MakeList(count) => {
                let mut item_type = None;
                for _ in 0..*count {
                    let ty = pop_type(&mut state.stack, pc)?;
                    if item_type.as_ref().is_some_and(|expected| expected != &ty) {
                        return Err(format!(
                            "list items have different types at instruction {pc}"
                        ));
                    }
                    item_type = Some(ty);
                }
                state
                    .stack
                    .push(Type::List(Box::new(item_type.unwrap_or(Type::Unit))));
            }
            Op::MakeMap(count) => {
                let mut item_type = None;
                for _ in 0..*count {
                    let value_type = pop_type(&mut state.stack, pc)?;
                    if pop_type(&mut state.stack, pc)? != Type::Text {
                        return Err(format!("map key requires Text at instruction {pc}"));
                    }
                    if item_type
                        .as_ref()
                        .is_some_and(|expected| expected != &value_type)
                    {
                        return Err(format!(
                            "map values have different types at instruction {pc}"
                        ));
                    }
                    item_type = Some(value_type);
                }
                state
                    .stack
                    .push(Type::Map(Box::new(item_type.unwrap_or(Type::Unit))));
            }
            Op::MakeTypedList(item) => {
                state.stack.push(Type::List(Box::new(item.clone())));
            }
            Op::MakeTypedMap(item) => {
                state.stack.push(Type::Map(Box::new(item.clone())));
            }
            Op::MakeRecord(record_index, count) => {
                let record = artifact.records.get(*record_index).ok_or_else(|| {
                    format!("record constructor references missing record at instruction {pc}")
                })?;
                if count != &record.fields.len() {
                    return Err(format!(
                        "record constructor has invalid field count at instruction {pc}"
                    ));
                }
                for field in record.fields.iter().rev() {
                    if pop_type(&mut state.stack, pc)? != field.ty {
                        return Err(format!(
                            "record constructor field type mismatch at instruction {pc}"
                        ));
                    }
                }
                state.stack.push(Type::Record(record.name.to_string()));
            }
            Op::GetField(record_index, field_index) => {
                let record = artifact.records.get(*record_index).ok_or_else(|| {
                    format!("field access references missing record at instruction {pc}")
                })?;
                let field = record.fields.get(*field_index).ok_or_else(|| {
                    format!("field access references missing field at instruction {pc}")
                })?;
                if pop_type(&mut state.stack, pc)? != Type::Record(record.name.to_string()) {
                    return Err(format!(
                        "field access record type mismatch at instruction {pc}"
                    ));
                }
                state.stack.push(field.ty.clone());
            }
            Op::MapKeys => {
                if !matches!(pop_type(&mut state.stack, pc)?, Type::Map(_)) {
                    return Err(format!("map keys requires Map at instruction {pc}"));
                }
                state.stack.push(Type::List(Box::new(Type::Text)));
            }
            Op::MapSet => {
                let value_type = pop_type(&mut state.stack, pc)?;
                if pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!("map_set key requires Text at instruction {pc}"));
                }
                let Type::Map(item_type) = pop_type(&mut state.stack, pc)? else {
                    return Err(format!("map_set requires Map at instruction {pc}"));
                };
                if value_type != *item_type {
                    return Err(format!(
                        "map_set value has the wrong type at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Map(item_type));
            }
            Op::MapRecordGet(record_index, field_index, nested) => {
                if *nested && pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!(
                        "map_record_get nested key requires Text at instruction {pc}"
                    ));
                }
                if pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!(
                        "map_record_get key requires Text at instruction {pc}"
                    ));
                }
                let record = artifact.records.get(*record_index).ok_or_else(|| {
                    format!("map_record_get references missing record at instruction {pc}")
                })?;
                let field = record.fields.get(*field_index).ok_or_else(|| {
                    format!("map_record_get references missing field at instruction {pc}")
                })?;
                let Type::Map(item_type) = pop_type(&mut state.stack, pc)? else {
                    return Err(format!("map_record_get requires Map at instruction {pc}"));
                };
                let value_type = if *nested {
                    let Type::Map(record_type) = item_type.as_ref() else {
                        return Err(format!(
                            "nested map_record_get requires Map of Maps at instruction {pc}"
                        ));
                    };
                    record_type.as_ref()
                } else {
                    item_type.as_ref()
                };
                if *value_type != Type::Record(record.name.to_string()) {
                    return Err(format!(
                        "map_record_get value type does not match record at instruction {pc}"
                    ));
                }
                state.stack.push(field.ty.clone());
            }
            Op::MapRecordSet(record_index, field_index, nested) => {
                let value_type = pop_type(&mut state.stack, pc)?;
                if *nested && pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!(
                        "map_record_set nested key requires Text at instruction {pc}"
                    ));
                }
                if pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!(
                        "map_record_set key requires Text at instruction {pc}"
                    ));
                }
                let record = artifact.records.get(*record_index).ok_or_else(|| {
                    format!("map_record_set references missing record at instruction {pc}")
                })?;
                let field = record.fields.get(*field_index).ok_or_else(|| {
                    format!("map_record_set references missing field at instruction {pc}")
                })?;
                let Type::Map(item_type) = pop_type(&mut state.stack, pc)? else {
                    return Err(format!("map_record_set requires Map at instruction {pc}"));
                };
                let record_type = if *nested {
                    let Type::Map(record_type) = item_type.as_ref() else {
                        return Err(format!(
                            "nested map_record_set requires Map of Maps at instruction {pc}"
                        ));
                    };
                    record_type.as_ref()
                } else {
                    item_type.as_ref()
                };
                if *record_type != Type::Record(record.name.to_string()) || value_type != field.ty {
                    return Err(format!(
                        "map_record_set types do not match record field at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Map(item_type));
            }
            Op::MapHas => {
                if pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!("map_has key requires Text at instruction {pc}"));
                }
                if !matches!(pop_type(&mut state.stack, pc)?, Type::Map(_)) {
                    return Err(format!("map_has requires Map at instruction {pc}"));
                }
                state.stack.push(Type::Bool);
            }
            Op::Call(target, argc) => {
                let callee = artifact.functions.get(*target).ok_or_else(|| {
                    format!("call references missing function at instruction {pc}")
                })?;
                if argc != &callee.parameters.len() {
                    return Err(format!("call argument count mismatch at instruction {pc}"));
                }
                for expected in callee.parameters.iter().rev() {
                    if &pop_type(&mut state.stack, pc)? != expected {
                        return Err(format!("call argument type mismatch at instruction {pc}"));
                    }
                }
                state.stack.push(callee.result.clone());
            }
            Op::PythonCall(target, argc) => {
                let external = artifact.externals.get(*target).ok_or_else(|| {
                    format!("Python call references missing declaration at instruction {pc}")
                })?;
                if argc != &external.parameters.len() {
                    return Err(format!(
                        "Python call argument count mismatch at instruction {pc}"
                    ));
                }
                for expected in external.parameters.iter().rev() {
                    if &pop_type(&mut state.stack, pc)? != expected {
                        return Err(format!(
                            "Python call argument type mismatch at instruction {pc}"
                        ));
                    }
                }
                state.stack.push(external.result.clone());
            }
            Op::PythonCallKw(target, positional_count, keyword_names) => {
                let external = artifact.externals.get(*target).ok_or_else(|| {
                    format!("Python call references missing declaration at instruction {pc}")
                })?;
                if *positional_count > external.parameters.len()
                    || positional_count + keyword_names.len() > external.parameters.len()
                {
                    return Err(format!(
                        "Python keyword call has too many arguments at instruction {pc}"
                    ));
                }
                let mut supplied = HashSet::new();
                for name in keyword_names {
                    let index = external
                        .parameter_names
                        .iter()
                        .position(|parameter| parameter == name)
                        .ok_or_else(|| {
                            format!("Python call uses an unknown keyword at instruction {pc}")
                        })?;
                    if index < *positional_count || !supplied.insert(index) {
                        return Err(format!(
                            "Python call has a duplicate keyword at instruction {pc}"
                        ));
                    }
                }
                for name in keyword_names.iter().rev() {
                    let index = external
                        .parameter_names
                        .iter()
                        .position(|parameter| parameter == name)
                        .unwrap();
                    if pop_type(&mut state.stack, pc)? != external.parameters[index] {
                        return Err(format!(
                            "Python keyword argument type mismatch at instruction {pc}"
                        ));
                    }
                }
                for expected in external.parameters[..*positional_count].iter().rev() {
                    if pop_type(&mut state.stack, pc)? != *expected {
                        return Err(format!(
                            "Python positional argument type mismatch at instruction {pc}"
                        ));
                    }
                }
                state.stack.push(external.result.clone());
            }
            Op::TogetherEnter | Op::TogetherExit => {}
            Op::ChannelNew(item) => {
                if pop_type(&mut state.stack, pc)? != Type::Int {
                    return Err(format!("channel capacity must be Int at instruction {pc}"));
                }
                if item.contains_channel() {
                    return Err(format!(
                        "a channel cannot carry a channel at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Channel(Box::new(item.clone())));
            }
            Op::ChannelSend => {
                let value = pop_type(&mut state.stack, pc)?;
                let Type::Channel(item) = pop_type(&mut state.stack, pc)? else {
                    return Err(format!("send needs a channel at instruction {pc}"));
                };
                if value != *item {
                    return Err(format!("send value type mismatch at instruction {pc}"));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Unit),
                    Box::new(Type::TaskError),
                ));
            }
            Op::ChannelReceive => {
                let Type::Channel(item) = pop_type(&mut state.stack, pc)? else {
                    return Err(format!("receive needs a channel at instruction {pc}"));
                };
                state.stack.push(Type::Option(item));
            }
            Op::ChannelClose => {
                if !matches!(pop_type(&mut state.stack, pc)?, Type::Channel(_)) {
                    return Err(format!("close needs a channel at instruction {pc}"));
                }
                state.stack.push(Type::Unit);
            }
            Op::FirstReady(record_index) | Op::FirstValue(record_index) => {
                if pop_type(&mut state.stack, pc)? != Type::Int {
                    return Err(format!("time limit must be Int at instruction {pc}"));
                }
                let Type::List(item) = pop_type(&mut state.stack, pc)? else {
                    return Err(format!("a list is needed at instruction {pc}"));
                };
                let payload = match (&instruction.op, *item) {
                    (Op::FirstReady(_), Type::Task(result)) => {
                        Some(Type::Result(result, Box::new(Type::TaskError)))
                    }
                    (Op::FirstValue(_), Type::Channel(value)) => Some(Type::Option(value)),
                    _ => None,
                };
                let record = artifact.records.get(*record_index).ok_or_else(|| {
                    format!("first_ready or first_value references a missing record at instruction {pc}")
                })?;
                let shaped = record.fields.len() == 2
                    && record.fields[0].ty == Type::Int
                    && payload.as_ref() == Some(&record.fields[1].ty);
                if !shaped {
                    return Err(format!(
                        "first_ready needs tasks and first_value needs channels, and a Ready record of their result, at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Option(Box::new(Type::Record(
                    record.name.to_string(),
                ))));
            }
            Op::TaskWait => {
                let Type::Task(item) = pop_type(&mut state.stack, pc)? else {
                    return Err(format!("wait needs a task at instruction {pc}"));
                };
                state
                    .stack
                    .push(Type::Result(item, Box::new(Type::TaskError)));
            }
            Op::TaskWaitWithin => {
                if pop_type(&mut state.stack, pc)? != Type::Int {
                    return Err(format!(
                        "wait_within needs Int milliseconds at instruction {pc}"
                    ));
                }
                let Type::Task(item) = pop_type(&mut state.stack, pc)? else {
                    return Err(format!("wait_within needs a task at instruction {pc}"));
                };
                state
                    .stack
                    .push(Type::Result(item, Box::new(Type::TaskError)));
            }
            Op::MakeFn(target) => {
                let callee = artifact.functions.get(*target).ok_or_else(|| {
                    format!("function value references missing function at instruction {pc}")
                })?;
                state.stack.push(Type::Fn(
                    callee.parameters.clone(),
                    Box::new(callee.result.clone()),
                ));
            }
            Op::MakeClosure(target, captured) => {
                let callee = artifact.functions.get(*target).ok_or_else(|| {
                    format!("closure references missing function at instruction {pc}")
                })?;
                if *captured > callee.parameters.len() {
                    return Err(format!(
                        "closure captures more values than its function takes at instruction {pc}"
                    ));
                }
                for expected in callee.parameters[..*captured].iter().rev() {
                    if pop_type(&mut state.stack, pc)? != *expected {
                        return Err(format!(
                            "closure captured value type mismatch at instruction {pc}"
                        ));
                    }
                }
                state.stack.push(Type::Fn(
                    callee.parameters[*captured..].to_vec(),
                    Box::new(callee.result.clone()),
                ));
            }
            Op::CallFn(argc) => {
                let Type::Fn(parameters, result) = pop_type(&mut state.stack, pc)? else {
                    return Err(format!("call through a non-function at instruction {pc}"));
                };
                if parameters.len() != *argc {
                    return Err(format!(
                        "function value call argument count mismatch at instruction {pc}"
                    ));
                }
                for expected in parameters.iter().rev() {
                    if pop_type(&mut state.stack, pc)? != *expected {
                        return Err(format!(
                            "function value call argument type mismatch at instruction {pc}"
                        ));
                    }
                }
                state.stack.push(*result);
            }
            Op::HigherOrder(kind) => {
                use guion_model::HigherOrderKind as Kind;
                let name = kind.name();
                let mut worker_limit = false;
                if *kind == Kind::TryParallelMap {
                    if pop_type(&mut state.stack, pc)? != Type::Record("Workers".into()) {
                        return Err(format!(
                            "try_parallel_map limits must be Workers at instruction {pc}"
                        ));
                    }
                    worker_limit = true;
                }
                if *kind == Kind::ParallelMap {
                    if pop_type(&mut state.stack, pc)? != Type::Int {
                        return Err(format!(
                            "parallel worker limit must be Int at instruction {pc}"
                        ));
                    }
                    worker_limit = true;
                }
                let Type::Fn(parameters, result) = pop_type(&mut state.stack, pc)? else {
                    return Err(format!("{name} needs a function value at instruction {pc}"));
                };
                let result = *result;
                let initial = if *kind == Kind::FoldList {
                    Some(pop_type(&mut state.stack, pc)?)
                } else {
                    None
                };
                let input = pop_type(&mut state.stack, pc)?;
                let bad_shape =
                    || format!("{name} function has the wrong type at instruction {pc}");
                let output = match kind {
                    Kind::SortBy | Kind::SortByKey | Kind::SortByIntKey => {
                        let Type::List(item) = input else {
                            return Err(format!("{name} input must be a List at instruction {pc}"));
                        };
                        let fits = match kind {
                            Kind::SortBy => {
                                parameters.len() == 2
                                    && parameters[0] == parameters[1]
                                    && result == Type::Int
                            }
                            Kind::SortByKey => parameters.len() == 1 && is_sort_key_type(&result),
                            _ => parameters.len() == 1 && result == Type::Int,
                        };
                        if !fits || *item != parameters[0] {
                            return Err(bad_shape());
                        }
                        Type::List(item)
                    }
                    Kind::MergeSortedByIntKey => {
                        let Type::List(feed) = input else {
                            return Err(format!(
                                "{name} input must be a List of Lists at instruction {pc}"
                            ));
                        };
                        let Type::List(item) = *feed else {
                            return Err(format!(
                                "{name} input must be a List of Lists at instruction {pc}"
                            ));
                        };
                        if parameters.len() != 1 || result != Type::Int || *item != parameters[0] {
                            return Err(bad_shape());
                        }
                        Type::List(item)
                    }
                    Kind::ParallelMap => {
                        let Type::List(item) = input else {
                            return Err(format!("parallel input must be List at instruction {pc}"));
                        };
                        if parameters.len() != 1
                            || *item != parameters[0]
                            || !worker_limit
                            || type_contains_native_resource(&parameters[0])
                            || type_contains_native_resource(&result)
                        {
                            return Err(bad_shape());
                        }
                        Type::Result(
                            Box::new(Type::List(Box::new(result))),
                            Box::new(Type::TaskError),
                        )
                    }
                    Kind::TryParallelMap => {
                        let Type::List(item) = input else {
                            return Err(format!("parallel input must be List at instruction {pc}"));
                        };
                        if parameters.len() != 1
                            || *item != parameters[0]
                            || !worker_limit
                            || type_contains_native_resource(&parameters[0])
                            || type_contains_native_resource(&result)
                        {
                            return Err(bad_shape());
                        }
                        Type::List(Box::new(Type::Result(
                            Box::new(result),
                            Box::new(Type::TaskError),
                        )))
                    }
                    Kind::StartTask => {
                        if parameters.len() != 1
                            || input != parameters[0]
                            || type_contains_native_resource(&parameters[0])
                            || type_contains_native_resource(&result)
                        {
                            return Err(bad_shape());
                        }
                        Type::Task(Box::new(result))
                    }
                    Kind::MapList => {
                        let Type::List(item) = input else {
                            return Err(format!("{name} input must be a List at instruction {pc}"));
                        };
                        if parameters.len() != 1 || *item != parameters[0] {
                            return Err(bad_shape());
                        }
                        Type::List(Box::new(result))
                    }
                    Kind::TryMapList => {
                        let Type::List(item) = input else {
                            return Err(format!("{name} input must be a List at instruction {pc}"));
                        };
                        if parameters.len() != 1 || *item != parameters[0] {
                            return Err(bad_shape());
                        }
                        Type::List(Box::new(Type::Result(
                            Box::new(result),
                            Box::new(Type::TaskError),
                        )))
                    }
                    Kind::FilterList | Kind::AnyList | Kind::AllList => {
                        let Type::List(item) = input else {
                            return Err(format!("{name} input must be a List at instruction {pc}"));
                        };
                        if parameters.len() != 1 || *item != parameters[0] || result != Type::Bool {
                            return Err(bad_shape());
                        }
                        if *kind == Kind::FilterList {
                            Type::List(item)
                        } else {
                            Type::Bool
                        }
                    }
                    Kind::FoldList => {
                        let Type::List(item) = input else {
                            return Err(format!("{name} input must be a List at instruction {pc}"));
                        };
                        let initial = initial.expect("fold pops its initial value");
                        if parameters.len() != 2
                            || parameters[0] != initial
                            || parameters[1] != *item
                            || result != initial
                        {
                            return Err(bad_shape());
                        }
                        result
                    }
                };
                state.stack.push(output);
            }
            Op::ParallelMap(target) => {
                let callee = artifact.functions.get(*target).ok_or_else(|| {
                    format!("parallel_map references missing function at instruction {pc}")
                })?;
                if callee.parameters.len() != 1 {
                    return Err(format!(
                        "parallel worker must take one argument at instruction {pc}"
                    ));
                }
                if type_contains_native_resource(&callee.parameters[0])
                    || type_contains_native_resource(&callee.result)
                {
                    return Err(format!(
                        "parallel workers cannot accept or return native resources at instruction {pc}"
                    ));
                }
                if pop_type(&mut state.stack, pc)? != Type::Int {
                    return Err(format!(
                        "parallel worker limit must be Int at instruction {pc}"
                    ));
                }
                let Type::List(item) = pop_type(&mut state.stack, pc)? else {
                    return Err(format!("parallel input must be List at instruction {pc}"));
                };
                if *item != callee.parameters[0] {
                    return Err(format!(
                        "parallel input type differs from worker parameter at instruction {pc}"
                    ));
                }
                if type_contains_native_resource(&item) {
                    return Err(format!(
                        "parallel inputs cannot contain native resources at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::List(Box::new(callee.result.clone()))),
                    Box::new(Type::TaskError),
                ));
            }
            Op::SortBy(target) => {
                let comparator = artifact.functions.get(*target).ok_or_else(|| {
                    format!("sort_by references missing comparator at instruction {pc}")
                })?;
                if comparator.parameters.len() != 2
                    || comparator.parameters[0] != comparator.parameters[1]
                    || comparator.result != Type::Int
                {
                    return Err(format!(
                        "sort comparator must have type (T, T) -> Int at instruction {pc}"
                    ));
                }
                let Type::List(item) = pop_type(&mut state.stack, pc)? else {
                    return Err(format!("sort_by input must be a List at instruction {pc}"));
                };
                if *item != comparator.parameters[0] {
                    return Err(format!(
                        "sort comparator parameter type differs from list item at instruction {pc}"
                    ));
                }
                state.stack.push(Type::List(item));
            }
            Op::SortNatural => {
                let Type::List(item) = pop_type(&mut state.stack, pc)? else {
                    return Err(format!("sort input must be a List at instruction {pc}"));
                };
                if !is_sort_key_type(&item) {
                    return Err(format!(
                        "sort requires a List of Text, Int or Float64 at instruction {pc}"
                    ));
                }
                state.stack.push(Type::List(item));
            }
            Op::SortByKey(target) => {
                let key_function = artifact.functions.get(*target).ok_or_else(|| {
                    format!("sort_by references missing key function at instruction {pc}")
                })?;
                if key_function.parameters.len() != 1 || !is_sort_key_type(&key_function.result) {
                    return Err(format!(
                        "sort_by key function must have type (T) -> K with K in Text, Int, Float64 at instruction {pc}"
                    ));
                }
                let Type::List(item) = pop_type(&mut state.stack, pc)? else {
                    return Err(format!("sort_by input must be a List at instruction {pc}"));
                };
                if *item != key_function.parameters[0] {
                    return Err(format!(
                        "sort_by key function parameter type differs from list item at instruction {pc}"
                    ));
                }
                state.stack.push(Type::List(item));
            }
            Op::SortByIntKey(target) => {
                let key_function = artifact.functions.get(*target).ok_or_else(|| {
                    format!("sort_by_int_key references missing key function at instruction {pc}")
                })?;
                if key_function.parameters.len() != 1 || key_function.result != Type::Int {
                    return Err(format!(
                        "sort_by_int_key key function must have type (T) -> Int at instruction {pc}"
                    ));
                }
                let Type::List(item) = pop_type(&mut state.stack, pc)? else {
                    return Err(format!(
                        "sort_by_int_key input must be a List at instruction {pc}"
                    ));
                };
                if *item != key_function.parameters[0] {
                    return Err(format!(
                        "sort_by_int_key key function parameter type differs from list item at instruction {pc}"
                    ));
                }
                state.stack.push(Type::List(item));
            }
            Op::MergeSortedByIntKey(target) => {
                let key_function = artifact.functions.get(*target).ok_or_else(|| {
                    format!("merge_sorted_by_int_key references missing key function at instruction {pc}")
                })?;
                if key_function.parameters.len() != 1 || key_function.result != Type::Int {
                    return Err(format!(
                        "merge_sorted_by_int_key key function must have type (T) -> Int at instruction {pc}"
                    ));
                }
                let Type::List(feed_item) = pop_type(&mut state.stack, pc)? else {
                    return Err(format!(
                        "merge_sorted_by_int_key input must be a List of Lists at instruction {pc}"
                    ));
                };
                let Type::List(item) = *feed_item else {
                    return Err(format!(
                        "merge_sorted_by_int_key input must be a List of Lists at instruction {pc}"
                    ));
                };
                if *item != key_function.parameters[0] {
                    return Err(format!(
                        "merge_sorted_by_int_key key function parameter type differs from feed item at instruction {pc}"
                    ));
                }
                state.stack.push(Type::List(item));
            }
            Op::TextCaseFold | Op::TextCollapseWhitespace => {
                if pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!(
                        "Unicode text operation requires Text at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Text);
            }
            Op::TextNormalize => {
                if pop_type(&mut state.stack, pc)? != Type::Text
                    || pop_type(&mut state.stack, pc)? != Type::Text
                {
                    return Err(format!(
                        "text_normalize requires Text value and form at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Text),
                    Box::new(Type::Record("StdError".into())),
                ));
            }
            Op::Sleep => {
                if pop_type(&mut state.stack, pc)? != Type::Int {
                    return Err(format!(
                        "sleep_ms requires an Int count at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Unit);
            }
            Op::RandomBytes => {
                if pop_type(&mut state.stack, pc)? != Type::Int {
                    return Err(format!(
                        "crypto_random_bytes requires an Int count at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Bytes),
                    Box::new(Type::Record("StdError".into())),
                ));
            }
            Op::CryptoHmacSha256 => {
                if pop_type(&mut state.stack, pc)? != Type::Bytes
                    || pop_type(&mut state.stack, pc)? != Type::Bytes
                {
                    return Err(format!(
                        "crypto_hmac_sha256 requires key and body Bytes at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Bytes);
            }
            Op::CryptoVerifyHmacSha256 => {
                if pop_type(&mut state.stack, pc)? != Type::Bytes
                    || pop_type(&mut state.stack, pc)? != Type::Bytes
                    || pop_type(&mut state.stack, pc)? != Type::Bytes
                {
                    return Err(format!(
                        "crypto_verify_hmac_sha256 requires three Bytes values at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Bool);
            }
            Op::SqliteOpen => {
                if pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!(
                        "sqlite_open requires Text path at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::NativeResource("Db".into())),
                    Box::new(Type::Record("StdError".into())),
                ));
            }
            Op::SqliteBegin | Op::SqliteBeginRead => {
                if pop_type(&mut state.stack, pc)? != Type::NativeResource("Db".into()) {
                    return Err(format!("sqlite_begin requires Db at instruction {pc}"));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::NativeResource("Transaction".into())),
                    Box::new(Type::Record("StdError".into())),
                ));
            }
            Op::LogAppend => {
                if pop_type(&mut state.stack, pc)? != Type::Text
                    || pop_type(&mut state.stack, pc)? != Type::Text
                {
                    return Err(format!(
                        "log append requires Text path and line at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Unit),
                    Box::new(Type::FileError),
                ));
            }
            Op::LogDefaultLevel => state.stack.push(Type::Text),
            Op::StdinReadLine => {
                state.stack.push(Type::Result(
                    Box::new(Type::Option(Box::new(Type::Text))),
                    Box::new(Type::Record("StdError".into())),
                ));
            }
            Op::StdinReadAll => {
                state.stack.push(Type::Result(
                    Box::new(Type::Bytes),
                    Box::new(Type::Record("StdError".into())),
                ));
            }
            Op::GzipCompress | Op::ZlibCompress => {
                if pop_type(&mut state.stack, pc)? != Type::Bytes {
                    return Err(format!(
                        "compression requires Bytes input at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Bytes);
            }
            Op::GzipDecompress | Op::ZlibDecompress => {
                if pop_type(&mut state.stack, pc)? != Type::Int
                    || pop_type(&mut state.stack, pc)? != Type::Bytes
                {
                    return Err(format!(
                        "decompression requires Bytes input and an Int output ceiling at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Bytes),
                    Box::new(Type::Record("StdError".into())),
                ));
            }
            Op::YamlParse => {
                if pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!("yaml_parse requires Text at instruction {pc}"));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Json),
                    Box::new(Type::Record("StdError".into())),
                ));
            }
            Op::SqliteExecute | Op::SqliteQuery => {
                if pop_type(&mut state.stack, pc)?
                    != Type::List(Box::new(Type::Record("SqlValue".into())))
                    || pop_type(&mut state.stack, pc)? != Type::Text
                    || pop_type(&mut state.stack, pc)? != Type::NativeResource("Transaction".into())
                {
                    return Err(format!(
                        "SQLite execution requires Transaction, Text SQL, and List[SqlValue] at instruction {pc}"
                    ));
                }
                let result = if instruction.op == Op::SqliteExecute {
                    Type::Int
                } else {
                    Type::List(Box::new(Type::Record("SqlRow".into())))
                };
                state.stack.push(Type::Result(
                    Box::new(result),
                    Box::new(Type::Record("StdError".into())),
                ));
            }
            Op::SqliteCommit | Op::SqliteRollback => {
                if pop_type(&mut state.stack, pc)? != Type::NativeResource("Transaction".into()) {
                    return Err(format!(
                        "SQLite transaction finalizer requires Transaction at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Unit),
                    Box::new(Type::Record("StdError".into())),
                ));
            }
            Op::SqliteClose => {
                if pop_type(&mut state.stack, pc)? != Type::NativeResource("Db".into()) {
                    return Err(format!("sqlite_close requires Db at instruction {pc}"));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Unit),
                    Box::new(Type::Record("StdError".into())),
                ));
            }
            Op::TomlParse => {
                if pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!("toml_parse requires Text at instruction {pc}"));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::NativeResource("TomlDocument".into())),
                    Box::new(Type::Record("StdError".into())),
                ));
            }
            Op::TomlHas | Op::TomlGetText | Op::TomlGetInt | Op::TomlGetBool => {
                if pop_type(&mut state.stack, pc)? != Type::List(Box::new(Type::Text))
                    || pop_type(&mut state.stack, pc)?
                        != Type::NativeResource("TomlDocument".into())
                {
                    return Err(format!(
                        "TOML lookup requires TomlDocument and List[Text] path at instruction {pc}"
                    ));
                }
                if instruction.op == Op::TomlHas {
                    state.stack.push(Type::Bool);
                } else {
                    let result = match instruction.op {
                        Op::TomlGetText => Type::Text,
                        Op::TomlGetInt => Type::Int,
                        Op::TomlGetBool => Type::Bool,
                        _ => unreachable!(),
                    };
                    state.stack.push(Type::Result(
                        Box::new(result),
                        Box::new(Type::Record("StdError".into())),
                    ));
                }
            }
            Op::TomlPutText | Op::TomlPutInt | Op::TomlPutBool => {
                let value_type = match instruction.op {
                    Op::TomlPutText => Type::Text,
                    Op::TomlPutInt => Type::Int,
                    Op::TomlPutBool => Type::Bool,
                    _ => unreachable!(),
                };
                if pop_type(&mut state.stack, pc)? != value_type
                    || pop_type(&mut state.stack, pc)? != Type::List(Box::new(Type::Text))
                    || pop_type(&mut state.stack, pc)?
                        != Type::NativeResource("TomlDocument".into())
                {
                    return Err(format!(
                        "TOML edit requires matching document, path, and value types at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Unit),
                    Box::new(Type::Record("StdError".into())),
                ));
            }
            Op::TomlRender => {
                if pop_type(&mut state.stack, pc)? != Type::NativeResource("TomlDocument".into()) {
                    return Err(format!(
                        "toml_render requires TomlDocument at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Text);
            }
            Op::CalendarParse => {
                if pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!("calendar_parse requires Text at instruction {pc}"));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::List(Box::new(Type::Record("CalendarEvent".into())))),
                    Box::new(Type::Record("StdError".into())),
                ));
            }
            Op::ReadText => {
                if pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!("read_text path must be Text at instruction {pc}"));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Text),
                    Box::new(Type::FileError),
                ));
            }
            Op::HttpGet => {
                if pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!("http_get URL must be Text at instruction {pc}"));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Text),
                    Box::new(Type::HttpError),
                ));
            }
            Op::HttpRequest => {
                if pop_type(&mut state.stack, pc)? != Type::Text
                    || pop_type(&mut state.stack, pc)? != Type::Map(Box::new(Type::Text))
                    || pop_type(&mut state.stack, pc)? != Type::Text
                    || pop_type(&mut state.stack, pc)? != Type::Text
                {
                    return Err(format!(
                        "http_request requires Text method and URL, Map[Text] headers, and Text body at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Json),
                    Box::new(Type::HttpError),
                ));
            }
            Op::HttpRequestWithTimeout => {
                if pop_type(&mut state.stack, pc)? != Type::Int
                    || pop_type(&mut state.stack, pc)? != Type::Text
                    || pop_type(&mut state.stack, pc)? != Type::Map(Box::new(Type::Text))
                    || pop_type(&mut state.stack, pc)? != Type::Text
                    || pop_type(&mut state.stack, pc)? != Type::Text
                {
                    return Err(format!(
                        "http_request_with_timeout requires Text method and URL, Map[Text] headers, Text body, and Int timeout_ms at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Json),
                    Box::new(Type::HttpError),
                ));
            }
            Op::HttpRequestBytes => {
                if pop_type(&mut state.stack, pc)? != Type::Int
                    || pop_type(&mut state.stack, pc)? != Type::Bytes
                    || pop_type(&mut state.stack, pc)? != Type::Map(Box::new(Type::Text))
                    || pop_type(&mut state.stack, pc)? != Type::Text
                    || pop_type(&mut state.stack, pc)? != Type::Text
                {
                    return Err(format!(
                        "http_request_bytes requires Text method and URL, Map[Text] headers, Bytes body, and Int timeout_ms at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Record("HttpResponse".into())),
                    Box::new(Type::HttpError),
                ));
            }
            Op::HttpResponseText => {
                if pop_type(&mut state.stack, pc)? != Type::Record("HttpResponse".into()) {
                    return Err(format!(
                        "http_response_text requires HttpResponse at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Text),
                    Box::new(Type::HttpError),
                ));
            }
            Op::HttpResponseJson => {
                if pop_type(&mut state.stack, pc)? != Type::Record("HttpResponse".into()) {
                    return Err(format!(
                        "http_response_json_bytes requires HttpResponse at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Json),
                    Box::new(Type::Record("StdError".into())),
                ));
            }
            Op::WriteText => {
                if pop_type(&mut state.stack, pc)? != Type::Text
                    || pop_type(&mut state.stack, pc)? != Type::Text
                {
                    return Err(format!(
                        "write_text requires Text path and content at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Unit),
                    Box::new(Type::FileError),
                ));
            }
            Op::WriteTextAtomic => {
                if pop_type(&mut state.stack, pc)? != Type::Text
                    || pop_type(&mut state.stack, pc)? != Type::Text
                {
                    return Err(format!(
                        "write_text_atomic requires Text path and content at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Unit),
                    Box::new(Type::FileError),
                ));
            }
            Op::ReadBytes => {
                if pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!("read_bytes path must be Text at instruction {pc}"));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Bytes),
                    Box::new(Type::FileError),
                ));
            }
            Op::FileOpenLines | Op::CsvOpen => {
                if pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!("file reader path must be Text at instruction {pc}"));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::NativeResource("FileReader".into())),
                    Box::new(Type::FileError),
                ));
            }
            Op::FileNextLine | Op::CsvNext | Op::FileClose => {
                if pop_type(&mut state.stack, pc)? != Type::NativeResource("FileReader".into()) {
                    return Err(format!(
                        "file reader operation requires FileReader at instruction {pc}"
                    ));
                }
                let result = match instruction.op {
                    Op::FileNextLine => Type::Option(Box::new(Type::Record("SourceLine".into()))),
                    Op::CsvNext => Type::Option(Box::new(Type::Record("CsvRow".into()))),
                    Op::FileClose => Type::Unit,
                    _ => unreachable!(),
                };
                state
                    .stack
                    .push(Type::Result(Box::new(result), Box::new(Type::FileError)));
            }
            Op::CsvReadColumns => {
                if pop_type(&mut state.stack, pc)? != Type::Map(Box::new(Type::Text))
                    || pop_type(&mut state.stack, pc)? != Type::Text
                {
                    return Err(format!(
                        "csv_read_columns requires a Text path and Map[Text, Text] kinds at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::List(Box::new(Type::Record("CsvColumn".into())))),
                    Box::new(Type::FileError),
                ));
            }
            Op::CsvEncode => {
                if pop_type(&mut state.stack, pc)?
                    != Type::List(Box::new(Type::List(Box::new(Type::Text))))
                {
                    return Err(format!(
                        "csv_encode requires List[List[Text]] at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Text),
                    Box::new(Type::FileError),
                ));
            }
            Op::TimeParseDate | Op::TimeParseTimestamp | Op::SemVerParse => {
                if pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!("data parser requires Text at instruction {pc}"));
                }
                let record = match instruction.op {
                    Op::TimeParseDate => "Date",
                    Op::TimeParseTimestamp => "Instant",
                    Op::SemVerParse => "SemVer",
                    _ => unreachable!(),
                };
                state.stack.push(Type::Result(
                    Box::new(Type::Record(record.into())),
                    Box::new(Type::FileError),
                ));
            }
            Op::TimeFormatUtc => {
                if pop_type(&mut state.stack, pc)? != Type::Record("Instant".into()) {
                    return Err(format!(
                        "time_format_utc requires Instant at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Text);
            }
            Op::TimeFormatUtcDate => {
                if pop_type(&mut state.stack, pc)? != Type::Record("Instant".into()) {
                    return Err(format!(
                        "time_format_utc_date requires Instant at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Text);
            }
            Op::UnixTimeMs => state.stack.push(Type::Int),
            Op::TimeTodayLocal => state.stack.push(Type::Record("Date".into())),
            Op::TimeResolveLocal => {
                if pop_type(&mut state.stack, pc)? != Type::Text
                    || pop_type(&mut state.stack, pc)? != Type::Record("LocalDateTime".into())
                {
                    return Err(format!(
                        "resolve_local requires LocalDateTime and Text at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Record("TemporalResolution".into())),
                    Box::new(Type::Record("TimeError".into())),
                ));
            }
            Op::TimeZoneDataVersion => state.stack.push(Type::Text),
            Op::SemVerCompare => {
                if pop_type(&mut state.stack, pc)? != Type::Record("SemVer".into())
                    || pop_type(&mut state.stack, pc)? != Type::Record("SemVer".into())
                {
                    return Err(format!(
                        "semver_compare requires two SemVer records at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Int);
            }
            Op::CliParse => {
                if pop_type(&mut state.stack, pc)?
                    != Type::List(Box::new(Type::Record("CliOption".into())))
                    || pop_type(&mut state.stack, pc)? != Type::List(Box::new(Type::Text))
                {
                    return Err(format!(
                        "cli_parse requires List[Text] and List[CliOption] at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Record("CliArgs".into())),
                    Box::new(Type::FileError),
                ));
            }
            Op::FsWalk => {
                if pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!("fs_walk requires Text at instruction {pc}"));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::List(Box::new(Type::Record("DirectoryEntry".into())))),
                    Box::new(Type::FileError),
                ));
            }
            Op::ListGetOption => {
                if pop_type(&mut state.stack, pc)? != Type::Int {
                    return Err(format!("list_get index must be Int at instruction {pc}"));
                }
                let Type::List(item) = pop_type(&mut state.stack, pc)? else {
                    return Err(format!("list_get requires List at instruction {pc}"));
                };
                state.stack.push(Type::Option(item));
            }
            Op::MapGetOption => {
                if pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!("map_get key must be Text at instruction {pc}"));
                }
                let Type::Map(item) = pop_type(&mut state.stack, pc)? else {
                    return Err(format!("map_get requires Map at instruction {pc}"));
                };
                state.stack.push(Type::Option(item));
            }
            Op::TextParseInt | Op::TextParseFloat => {
                if pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!(
                        "text numeric parser requires Text at instruction {pc}"
                    ));
                }
                let parsed = if instruction.op == Op::TextParseInt {
                    Type::Int
                } else {
                    Type::Float64
                };
                state
                    .stack
                    .push(Type::Result(Box::new(parsed), Box::new(Type::FileError)));
            }
            Op::WriteBytes => {
                if pop_type(&mut state.stack, pc)? != Type::Bytes
                    || pop_type(&mut state.stack, pc)? != Type::Text
                {
                    return Err(format!(
                        "write_bytes requires Text path and Bytes content at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Unit),
                    Box::new(Type::FileError),
                ));
            }
            Op::Args => state.stack.push(Type::List(Box::new(Type::Text))),
            Op::ArgsFrom => {
                if pop_type(&mut state.stack, pc)? != Type::Int {
                    return Err(format!("args_from requires Int at instruction {pc}"));
                }
                state.stack.push(Type::List(Box::new(Type::Text)));
            }
            Op::Environment => {
                if pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!("env name must be Text at instruction {pc}"));
                }
                state.stack.push(Type::Option(Box::new(Type::Text)));
            }
            Op::Jump(target) => successors.push(*target),
            Op::JumpIfFalse(target) => {
                if pop_type(&mut state.stack, pc)? != Type::Bool {
                    return Err(format!(
                        "conditional jump requires Bool at instruction {pc}"
                    ));
                }
                successors.push(*target);
                successors.push(pc + 1);
            }
            Op::Return => {
                let result = pop_type(&mut state.stack, pc)?;
                if result != function.result || !state.stack.is_empty() {
                    return Err(format!(
                        "return type or stack height is invalid at instruction {pc}: expected {:?}, got {:?}, stack {:?}",
                        function.result, result, state.stack
                    ));
                }
                has_return = true;
            }
        }
        if !matches!(
            instruction.op,
            Op::Jump(_) | Op::JumpIfFalse(_) | Op::Return
        ) {
            successors.push(pc + 1);
        }
        for next in successors {
            if next >= function.code.len() {
                return Err(format!(
                    "control flow falls off the end at instruction {pc}"
                ));
            }
            match &mut states[next] {
                None => {
                    states[next] = Some(state.clone());
                    pending.push_back(next);
                }
                Some(previous) => {
                    if previous.stack != state.stack {
                        return Err(format!("inconsistent stack types at instruction {next}"));
                    }
                    let mut changed = false;
                    for (old, new) in previous.locals.iter_mut().zip(&state.locals) {
                        if old != new && old.is_some() {
                            *old = None;
                            changed = true;
                        }
                    }
                    if changed {
                        pending.push_back(next);
                    }
                }
            }
        }
    }
    if !has_return {
        return Err("no reachable return".into());
    }
    Ok(())
}
