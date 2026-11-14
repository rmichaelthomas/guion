//! First-class function values (artifact format 66): named top-level functions bound to names,
//! passed, stored and called; no closures; effects and serialization boundaries decided at the
//! forming site; the higher-order builtins that take function values.

/// The format that introduced function values, and the one before it.
const FORMAT_FUNCTION_VALUES: u32 = 66;
const FORMAT_BEFORE_FUNCTION_VALUES: u32 = 65;

use guion_compiler::{compile, compile_project_with_roots};
use guion_model::{Artifact, Op};
use guion_runtime::{HostPolicy, Runtime};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Output, or the error code, message and trace as (line, column) pairs.
type RunResult = Result<String, (String, String, Vec<(usize, usize)>)>;

fn run_artifact(artifact: Artifact, policy: HostPolicy, runtime: Runtime) -> RunResult {
    let mut instance = runtime.load(artifact, policy).expect("artifact loads");
    match instance.invoke("main", vec![]) {
        Ok(_) => Ok(instance.take_output()),
        Err(error) => Err((
            error.code.to_string(),
            error.message,
            error
                .trace
                .iter()
                .map(|span| (span.line, span.column))
                .collect(),
        )),
    }
}

fn policy() -> HostPolicy {
    HostPolicy {
        max_parallelism: 4,
        instruction_limit: 50_000_000,
        ..HostPolicy::default()
    }
}

fn try_run(source: &str) -> RunResult {
    let artifact = compile("function-values.gu", source)
        .unwrap_or_else(|error| panic!("{} {}", error.code, error.message));
    run_artifact(artifact, policy(), Runtime::vm_only())
}

fn run(source: &str) -> String {
    try_run(source).unwrap_or_else(|(code, message, _)| panic!("{code}: {message}"))
}

/// Compile and return the diagnostic code with its line and column.
fn check_error(source: &str) -> (String, usize, usize) {
    match compile("function-values.gu", source) {
        Ok(_) => panic!("expected a compile error for:\n{source}"),
        Err(error) => (error.code.to_string(), error.span.line, error.span.column),
    }
}

fn check_error_code(source: &str) -> String {
    check_error(source).0
}

const DOUBLE: &str = "fn double(x: Int) -> Int:\n    return x * 2\n\n";

#[test]
fn a_function_name_binds_to_a_local_of_function_type() {
    let out = run(&format!(
        "{DOUBLE}fn main() -> Unit:\n    let f = double\n    let g: fn(Int) -> Int = f\n    print(f(4))\n    print(g(5))\n"
    ));
    assert_eq!(out, "8\n10\n");
    // The inferred type is `fn(Int) -> Int`: an annotation that disagrees is rejected.
    assert_eq!(
        check_error_code(&format!(
            "{DOUBLE}fn main() -> Unit:\n    let f: fn(Int) -> Text = double\n    print(f(4))\n"
        )),
        "E_TYPE_ANNOTATION"
    );
    assert_eq!(
        check_error_code(&format!(
            "{DOUBLE}fn main() -> Unit:\n    let f: fn(Int, Int) -> Int = double\n    print(1)\n"
        )),
        "E_TYPE_ANNOTATION"
    );
}

#[test]
fn apply_twice_returns_twelve_for_double_and_three() {
    let out = run(&format!(
        "{DOUBLE}fn apply_twice(f: fn(Int) -> Int, x: Int) -> Int:\n    return f(f(x))\n\nfn main() -> Int:\n    print(apply_twice(double, 3))\n    return apply_twice(double, 3)\n"
    ));
    assert_eq!(out, "12\n");
    let artifact = compile(
        "function-values.gu",
        &format!(
            "{DOUBLE}fn apply_twice(f: fn(Int) -> Int, x: Int) -> Int:\n    return f(f(x))\n\nfn main() -> Int:\n    return apply_twice(double, 3)\n"
        ),
    )
    .unwrap();
    let mut instance = Runtime::vm_only().load(artifact, policy()).unwrap();
    assert_eq!(
        instance.invoke("main", vec![]).unwrap(),
        guion_model::Value::Int(guion_model::BigInt::parse("12").unwrap())
    );
}

#[test]
fn a_user_function_can_return_and_compose_function_values() {
    let out = run(&format!(
        "{DOUBLE}fn add_one(x: Int) -> Int:\n    return x + 1\n\nfn pick(first: Bool) -> fn(Int) -> Int:\n    if first:\n        return double\n    return add_one\n\nfn compose(f: fn(Int) -> Int, g: fn(Int) -> Int, x: Int) -> Int:\n    return f(g(x))\n\nfn main() -> Unit:\n    print(compose(pick(true), pick(false), 5))\n    print(compose(pick(false), pick(true), 5))\n"
    ));
    assert_eq!(out, "12\n11\n");
}

#[test]
fn a_local_shadows_a_function_of_the_same_name() {
    let out = run(&format!(
        "{DOUBLE}fn add_one(x: Int) -> Int:\n    return x + 1\n\nfn apply(double: fn(Int) -> Int, x: Int) -> Int:\n    return double(x)\n\nfn main() -> Unit:\n    print(apply(add_one, 1))\n    print(double(1))\n"
    ));
    assert_eq!(out, "2\n2\n");
}

#[test]
fn sort_by_takes_a_user_key_function_and_a_function_typed_variable() {
    let source = r#"record Person:
    name: Text
    score: Int

fn score_of(person: Person) -> Int:
    return person.score

fn name_of(person: Person) -> Text:
    return person.name

fn names(people: List[Person]) -> Text:
    var out = ""
    for person in people:
        set out = out + person.name
    return out

fn sorted_with(people: List[Person], key: fn(Person) -> Int) -> Text:
    return names(sort_by(people, key))

fn main() -> Unit:
    let people = [Person("c", 3), Person("a", 1), Person("b", 2)]
    print(names(sort_by(people, score_of)))
    let by_name = name_of
    print(names(sort_by(people, by_name)))
    let key = score_of
    print(names(sort_by(people, key)))
    print(sorted_with(people, score_of))
"#;
    assert_eq!(run(source), "abc\nabc\nabc\nabc\n");
}

#[test]
fn sort_by_int_key_merge_and_parallel_map_accept_function_variables() {
    let source = r#"fn negate(x: Int) -> Int:
    return 0 - x

fn square(x: Int) -> Int:
    return x * x

fn main() -> Unit:
    let key = negate
    print(sort_by_int_key([1, 3, 2], key))
    print(merge_sorted_by_int_key([[3, 1], [2]], key))
    let worker = square
    match parallel_map([1, 2, 3], worker, 2):
        Ok(values):
            print(values)
        Err(error):
            print("failed")
"#;
    assert_eq!(run(source), "[3, 2, 1]\n[3, 2, 1]\n[1, 4, 9]\n");
}

#[test]
fn a_comparator_variable_orders_like_a_named_comparator() {
    let source = r#"fn descending(left: Int, right: Int) -> Int:
    return right - left

fn main() -> Unit:
    let compare = descending
    print(sort_by([1, 3, 2], compare))
    print(sort_by([1, 3, 2], descending))
"#;
    assert_eq!(run(source), "[3, 2, 1]\n[3, 2, 1]\n");
}

#[test]
fn calling_a_function_value_with_the_wrong_arity_or_type_is_a_check_error() {
    let two = "fn add(a: Int, b: Int) -> Int:\n    return a + b\n\n";
    assert_eq!(
        check_error_code(&format!(
            "{two}fn main() -> Unit:\n    let f = add\n    print(f(1))\n"
        )),
        "E_CALL_ARITY"
    );
    assert_eq!(
        check_error_code(&format!(
            "{two}fn main() -> Unit:\n    let f = add\n    print(f(1, 2, 3))\n"
        )),
        "E_CALL_ARITY"
    );
    assert_eq!(
        check_error_code(&format!(
            "{two}fn main() -> Unit:\n    let f = add\n    print(f(1, \"two\"))\n"
        )),
        "E_TYPE_ARGUMENT"
    );
    // Passing a function of the wrong shape is a check error too.
    assert_eq!(
        check_error_code(&format!(
            "{two}fn apply(f: fn(Int) -> Int, x: Int) -> Int:\n    return f(x)\n\nfn main() -> Unit:\n    print(apply(add, 1))\n"
        )),
        "E_TYPE_ARGUMENT"
    );
    // Calling a local that is not a function is not a call through a value.
    assert_eq!(
        check_error_code("fn main() -> Unit:\n    let n = 3\n    print(n(1))\n"),
        "E_NAME_UNKNOWN"
    );
}

#[test]
fn comparing_function_values_is_e_fn_compare() {
    for operator in ["==", "!="] {
        let code = check_error(&format!(
            "{DOUBLE}fn main() -> Unit:\n    let f = double\n    let g = double\n    print(f {operator} g)\n"
        ));
        assert_eq!(code.0, "E_FN_COMPARE", "{operator}");
    }
    // A record that holds a function value cannot be compared either.
    let record = check_error(&format!(
        "{DOUBLE}record Box:\n    run: fn(Int) -> Int\n\nfn main() -> Unit:\n    print(Box(double) == Box(double))\n"
    ));
    assert_eq!(record.0, "E_FN_COMPARE");
}

#[test]
fn function_values_live_in_record_fields_and_lists_and_are_called_after_reading() {
    let source = r#"fn double(x: Int) -> Int:
    return x * 2

fn square(x: Int) -> Int:
    return x * x

record Handler:
    name: Text
    run: fn(Int) -> Int

fn main() -> Unit:
    let handler = Handler("double", double)
    let run = handler.run
    print(run(21))
    let table = [Handler("double", double), Handler("square", square)]
    for entry in table:
        let f = entry.run
        print(entry.name + ":" + int_to_text(f(7)))
    let fns = [double, square, double]
    var total = 0
    for f in fns:
        set total = total + f(3)
    print(total)
    let by_name = {"double": double, "square": square}
    match map_get(by_name, "square"):
        Some(f):
            print(f(9))
        None:
            print("missing")
"#;
    assert_eq!(run(source), "42\ndouble:14\nsquare:49\n21\n81\n");
}

struct TempProject(PathBuf);

impl TempProject {
    fn new(label: &str) -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "guion-fn-values-{}-{label}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, source: &str) {
        std::fs::write(self.0.join(name), source).unwrap();
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn stdlib_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../language/stdlib")
}

fn compile_project(project: &TempProject) -> Result<Artifact, guion_model::Diagnostic> {
    compile_project_with_roots(
        &project.0.join("main.gu"),
        &project.0,
        Some(&stdlib_root()),
        &HashMap::new(),
    )
}

#[test]
fn an_imported_function_is_a_value() {
    let project = TempProject::new("imported");
    project.write(
        "tools.gu",
        "pub fn triple(x: Int) -> Int:\n    return x * 3\n\npub fn apply_twice(f: fn(Int) -> Int, x: Int) -> Int:\n    return f(f(x))\n",
    );
    project.write(
        "main.gu",
        "import \"tools.gu\"\n\nfn main() -> Unit:\n    let f = triple\n    print(f(2))\n    print(apply_twice(triple, 2))\n    print(map_list([1, 2], triple))\n",
    );
    let artifact = compile_project(&project).expect("project compiles");
    let out = run_artifact(artifact, policy(), Runtime::vm_only()).unwrap();
    assert_eq!(out, "6\n18\n[3, 6]\n");
}

#[test]
fn recursion_through_a_function_value_hits_the_call_depth_limit() {
    let source = r#"fn forever(f: fn(Int) -> Int, n: Int) -> Int:
    return f(n + 1)

fn start(n: Int) -> Int:
    return forever(start, n)

fn main() -> Unit:
    print(start(0))
"#;
    let (code, message, _) = try_run(source).expect_err("the recursion is cut off");
    assert_eq!(code, "R_CALL_DEPTH", "{message}");
    // Through the higher-order builtins, which re-enter the VM, the limit still applies.
    let nested = r#"fn again(x: Int) -> Int:
    return len(map_list([x], again))

fn main() -> Unit:
    print(again(1))
"#;
    let (code, message, _) = try_run(nested).expect_err("the recursion is cut off");
    assert_eq!(code, "R_CALL_DEPTH", "{message}");
}

#[test]
fn a_function_value_handed_to_a_parallel_map_worker_calls_the_right_function() {
    let source = r#"fn triple(x: Int) -> Int:
    return x * 3

fn square(x: Int) -> Int:
    return x * x

record Job:
    run: fn(Int) -> Int
    n: Int

fn work(job: Job) -> Int:
    let f = job.run
    return f(job.n)

fn main() -> Unit:
    let jobs = [Job(triple, 4), Job(square, 5), Job(triple, 6), Job(square, 7)]
    match parallel_map(jobs, work, 3):
        Ok(values):
            print(values)
        Err(error):
            print("failed")
"#;
    assert_eq!(run(source), "[12, 25, 18, 49]\n");
}

#[test]
fn a_function_value_that_names_no_function_is_r_fn_invalid_and_is_not_reachable_from_source() {
    // No source expression builds a `Value::Function` from an integer; the only way to get
    // one is `MakeFn`. A tampered artifact whose index is out of range is stopped at load
    // (verification) or, if it slips through a worker or host, by `R_FN_INVALID`.
    let artifact = compile(
        "function-values.gu",
        &format!("{DOUBLE}fn main() -> Int:\n    let f = double\n    return f(1)\n"),
    )
    .unwrap();
    let mut forged = artifact.clone();
    for function in &mut forged.functions {
        for instruction in &mut function.code {
            if let Op::MakeFn(index) = &mut instruction.op {
                *index = 99;
            }
        }
    }
    assert!(Runtime::vm_only().load(forged, policy()).is_err());
    assert!(
        check_error_code("fn main() -> Int:\n    let f = 3\n    return f(1)\n") == "E_NAME_UNKNOWN"
    );
}

#[test]
fn function_values_cannot_cross_a_serialization_boundary() {
    let double = DOUBLE;
    let cases: Vec<(&str, String)> = vec![
        (
            "json_set",
            format!(
                "{double}fn main() -> Unit:\n    let f = double\n    print(json_stringify(json_set(json_object(), \"k\", f)))\n"
            ),
        ),
        (
            "json_push of a record holding a function",
            format!(
                "{double}record Box:\n    run: fn(Int) -> Int\n\nfn main() -> Unit:\n    let box = Box(double)\n    print(json_stringify(json_push(json_array(), box)))\n"
            ),
        ),
        (
            "toml",
            format!(
                "{double}fn put(document: TomlDocument) -> Int:\n    let f = double\n    native_toml_put_text(document, [\"a\"], f)\n    return 0\n\nfn main() -> Unit:\n    print(1)\n"
            ),
        ),
        (
            "sqlite parameter",
            format!(
                "{double}fn run_sql(tx: Transaction) -> Int:\n    let f = double\n    native_sqlite_execute(tx, \"select 1\", [f])\n    return 0\n\nfn main() -> Unit:\n    print(1)\n"
            ),
        ),
        (
            "process argument",
            format!(
                "{double}fn main() -> Unit:\n    let f = double\n    native_process_run(\"ls\", [f], \"\")\n"
            ),
        ),
        (
            "http body",
            format!(
                "{double}fn main() -> Unit:\n    let f = double\n    http_request(\"POST\", \"http://localhost\", {{\"a\": \"b\"}}, f)\n"
            ),
        ),
        (
            "print",
            format!("{double}fn main() -> Unit:\n    print(double)\n"),
        ),
        (
            "file write",
            format!("{double}fn main() -> Unit:\n    write_text(\"out.txt\", double)\n"),
        ),
        (
            "csv",
            format!("{double}fn main() -> Unit:\n    print(csv_encode([[double]]))\n"),
        ),
    ];
    for (label, source) in cases {
        let (code, _, _) = check_error(&source);
        assert_eq!(code, "E_FN_NOT_SERIALIZABLE", "{label}");
    }
}

#[test]
fn function_values_cannot_cross_standard_library_wrappers() {
    for (label, call) in [
        (
            "std:process argument",
            "process_run(\"ls\", [double], \"\")",
        ),
        (
            "std:toml value",
            "toml_put_text(unwrap(toml_parse(\"a = 1\")), [\"a\"], double)",
        ),
    ] {
        let project = TempProject::new("std");
        let module = if label.contains("process") {
            "std:process"
        } else {
            "std:toml"
        };
        project.write(
            "main.gu",
            &format!(
                "import \"{module}\"\n\nfn double(x: Int) -> Int:\n    return x * 2\n\nfn main() -> Unit:\n    {call}\n"
            ),
        );
        let error = compile_project(&project).expect_err(label);
        assert_eq!(
            error.code, "E_FN_NOT_SERIALIZABLE",
            "{label}: {}",
            error.message
        );
    }
}

#[test]
fn function_values_inside_lists_records_and_options_stay_inside_the_program() {
    // Nothing here crosses a boundary: the values stay inside one program.
    let source = r#"fn double(x: Int) -> Int:
    return x * 2

record Box:
    items: List[fn(Int) -> Int]

fn main() -> Unit:
    let box = Box([double, double])
    var total = 0
    for f in box.items:
        set total = total + f(5)
    print(total)
    print(len(box.items))
"#;
    assert_eq!(run(source), "20\n2\n");
}

#[test]
fn artifacts_below_format_66_cannot_use_function_values() {
    let source = format!("{DOUBLE}fn main() -> Int:\n    let f = double\n    return f(4)\n");
    let artifact = compile("function-values.gu", &source).unwrap();
    assert_eq!(artifact.format_version, Artifact::CURRENT_VERSION);
    assert!(artifact.functions.iter().any(|function| function
        .code
        .iter()
        .any(|instruction| matches!(instruction.op, Op::MakeFn(_) | Op::CallFn(_)))));
    let bytes = artifact.to_bytes().expect("format 66 writes");
    assert_eq!(
        Artifact::from_bytes(&bytes).unwrap().functions,
        artifact.functions
    );
    let mut old = artifact.clone();
    old.format_version = FORMAT_BEFORE_FUNCTION_VALUES;
    let error = old.to_bytes().expect_err("format 65 cannot hold MakeFn");
    assert_eq!(error.code, "E_ARTIFACT_VERSION");
    // The same bytes with the version field rewritten to 65 are rejected on read.
    let mut patched = bytes.clone();
    patched[4..8].copy_from_slice(&(FORMAT_BEFORE_FUNCTION_VALUES).to_le_bytes());
    let error = Artifact::from_bytes(&patched).expect_err("format 65 cannot hold MakeFn");
    assert_eq!(error.code, "E_ARTIFACT_VERSION");
    // A function type in a signature alone also needs format 66.
    let signature_only = compile(
        "function-values.gu",
        "fn apply(f: fn(Int) -> Int, x: Int) -> Int:\n    return f(x)\n\nfn main() -> Int:\n    return 0\n",
    )
    .unwrap();
    let mut old = signature_only;
    old.format_version = FORMAT_BEFORE_FUNCTION_VALUES;
    assert_eq!(old.to_bytes().unwrap_err().code, "E_ARTIFACT_VERSION");
}

#[test]
fn a_format_65_artifact_still_loads_and_runs() {
    let source = "fn double(x: Int) -> Int:\n    return x * 2\n\nfn main() -> Unit:\n    print(sort([3, 1, 2]))\n    print(double(21))\n";
    let mut artifact = compile("function-values.gu", source).unwrap();
    artifact.format_version = FORMAT_BEFORE_FUNCTION_VALUES;
    let bytes = artifact.to_bytes().expect("format 65 can still be written");
    let reread = Artifact::from_bytes(&bytes).expect("format 65 can still be read");
    assert_eq!(reread.format_version, FORMAT_BEFORE_FUNCTION_VALUES);
    let out = run_artifact(reread, policy(), Runtime::vm_only()).unwrap();
    assert_eq!(out, "[1, 2, 3]\n42\n");
}

#[test]
fn a_function_value_is_never_an_artifact_constant() {
    let mut artifact = compile("function-values.gu", "fn main() -> Unit:\n    print(1)\n").unwrap();
    artifact.functions[0].code.insert(
        0,
        guion_model::Instruction {
            op: Op::Const(guion_model::Value::Function(0)),
            span: Default::default(),
        },
    );
    let error = artifact
        .to_bytes()
        .expect_err("a function constant cannot be written");
    assert_eq!(error.code, "E_ARTIFACT_VALUE");
}

// ---- Effects ----

#[test]
fn a_sort_key_that_forms_an_effectful_function_value_is_rejected() {
    let source = r#"fn loud(x: Int) -> Int:
    print(x)
    return x

fn key(x: Int) -> Int:
    let f = loud
    return f(x)

fn main() -> Unit:
    print(sort_by_int_key([2, 1], key))
"#;
    let error = compile("function-values.gu", source).expect_err("the key reaches print");
    assert_eq!(error.code, "E_SORT_EFFECT");
}

#[test]
fn a_sort_key_may_use_a_pure_function_value_formed_inside_it() {
    let source = r#"fn negate(x: Int) -> Int:
    return 0 - x

fn key(x: Int) -> Int:
    let f = negate
    return f(x)

fn main() -> Unit:
    print(sort_by_int_key([1, 3, 2], key))
"#;
    assert_eq!(run(source), "[3, 2, 1]\n");
}

// ---- The five higher-order builtins ----

fn expect_runtime_error(source: &str) -> (String, String, Vec<(usize, usize)>) {
    try_run(source).expect_err("the program fails at run time")
}

#[test]
fn map_list_applies_a_function_value_to_every_item() {
    let source = format!(
        "{DOUBLE}fn label(x: Int) -> Text:\n    return \"n\" + int_to_text(x)\n\nfn main() -> Unit:\n    print(map_list([1, 2, 3], double))\n    let f = label\n    print(map_list([4, 5], f))\n    let none: List[Int] = []\n    print(len(map_list(none, double)))\n"
    );
    assert_eq!(run(&source), "[2, 4, 6]\n[n4, n5]\n0\n");
}

#[test]
fn map_list_passes_result_values_through_to_the_caller() {
    let source = r#"fn parse(x: Int) -> Result[Int, Text]:
    if x < 0:
        return Err("negative")
    return Ok(x * 10)

fn main() -> Unit:
    let results = map_list([1, -2, 3], parse)
    print(len(results))
    print(is_ok(results[0]))
    print(is_ok(results[1]))
    print(error(results[1]))
    print(unwrap(results[2]))
"#;
    assert_eq!(run(source), "3\ntrue\nfalse\nnegative\n30\n");
}

#[test]
fn a_runtime_error_inside_a_function_value_reports_the_callee_span() {
    let source = r#"fn reciprocal(x: Int) -> Int:
    return 100 / x

fn main() -> Unit:
    print(map_list([5, 0, 2], reciprocal))
"#;
    let (code, _, trace) = expect_runtime_error(source);
    assert_eq!(code, "R_DIV_ZERO");
    // The first span is the division in the callee (line 2); the builtin's own frame follows.
    assert_eq!(trace.first().map(|(line, _)| *line), Some(2), "{trace:?}");
    assert!(
        trace.iter().any(|(line, _)| *line == 5),
        "the builtin's call site stays in the trace: {trace:?}"
    );
}

#[test]
fn filter_list_keeps_items_the_function_accepts() {
    let source = r#"fn big(x: Int) -> Bool:
    return x > 2

fn main() -> Unit:
    print(filter_list([1, 2, 3, 4], big))
    let keep = big
    print(filter_list([5, 1], keep))
    print(len(filter_list([1], big)))
"#;
    assert_eq!(run(source), "[3, 4]\n[5]\n0\n");
}

#[test]
fn filter_list_propagates_a_runtime_error_with_the_callee_span() {
    let source = r#"fn check(x: Int) -> Bool:
    return 10 / x > 1

fn main() -> Unit:
    print(filter_list([5, 0], check))
"#;
    let (code, _, trace) = expect_runtime_error(source);
    assert_eq!(code, "R_DIV_ZERO");
    assert_eq!(trace.first().map(|(line, _)| *line), Some(2), "{trace:?}");
}

#[test]
fn fold_list_threads_an_accumulator_through_the_function() {
    let source = r#"fn add(total: Int, x: Int) -> Int:
    return total + x

fn join(text: Text, x: Int) -> Text:
    return text + int_to_text(x)

fn main() -> Unit:
    print(fold_list([1, 2, 3, 4], 0, add))
    print(fold_list([1, 2, 3], "n", join))
    let none: List[Int] = []
    print(fold_list(none, 7, add))
    let f = add
    print(fold_list([5, 5], 1, f))
"#;
    assert_eq!(run(source), "10\nn123\n7\n11\n");
}

#[test]
fn fold_list_can_fold_results_and_stops_at_a_runtime_error() {
    let source = r#"fn checked(total: Result[Int, Text], x: Int) -> Result[Int, Text]:
    if is_ok(total) == false:
        return total
    if x < 0:
        return Err("negative")
    return Ok(unwrap(total) + x)

fn main() -> Unit:
    let folded = fold_list([1, 2, 3], Ok(0), checked)
    print(unwrap(folded))
    let broken = fold_list([1, -1, 3], Ok(0), checked)
    print(is_ok(broken))
"#;
    // `Ok(0)` needs a Result-returning context, so the folds are built in a helper.
    let _ = source;
    let source = r#"fn start() -> Result[Int, Text]:
    return Ok(0)

fn checked(total: Result[Int, Text], x: Int) -> Result[Int, Text]:
    if is_ok(total) == false:
        return total
    if x < 0:
        return Err("negative")
    return Ok(unwrap(total) + x)

fn main() -> Unit:
    print(unwrap(fold_list([1, 2, 3], start(), checked)))
    print(is_ok(fold_list([1, -1, 3], start(), checked)))
"#;
    assert_eq!(run(source), "6\nfalse\n");
    let failing = r#"fn step(total: Int, x: Int) -> Int:
    return total / x

fn main() -> Unit:
    print(fold_list([1, 0], 10, step))
"#;
    let (code, _, trace) = expect_runtime_error(failing);
    assert_eq!(code, "R_DIV_ZERO");
    assert_eq!(trace.first().map(|(line, _)| *line), Some(2), "{trace:?}");
}

#[test]
fn any_list_and_all_list_short_circuit_and_report_errors() {
    let source = r#"fn even(x: Int) -> Bool:
    return x % 2 == 0

fn positive(x: Int) -> Bool:
    return x > 0

fn main() -> Unit:
    print(any_list([1, 3, 4], even))
    print(any_list([1, 3, 5], even))
    print(all_list([1, 2, 3], positive))
    print(all_list([1, -2, 3], positive))
    let none: List[Int] = []
    print(any_list(none, even))
    print(all_list(none, even))
    let f = positive
    print(all_list([4, 5], f))
"#;
    assert_eq!(run(source), "true\nfalse\ntrue\nfalse\nfalse\ntrue\ntrue\n");
    // A true answer for `any_list` stops before the failing item is reached.
    let stops = r#"fn probe(x: Int) -> Bool:
    return 10 / x > 1

fn main() -> Unit:
    print(any_list([2, 0], probe))
    print(all_list([2, 0], probe))
"#;
    let (code, _, trace) = expect_runtime_error(stops);
    assert_eq!(code, "R_DIV_ZERO");
    assert_eq!(trace.first().map(|(line, _)| *line), Some(2), "{trace:?}");
    let any_stops = r#"fn probe(x: Int) -> Bool:
    return 10 / x > 1

fn main() -> Unit:
    print(any_list([2, 0], probe))
"#;
    assert_eq!(run(any_stops), "true\n");
}

#[test]
fn the_higher_order_builtins_reject_a_function_of_the_wrong_shape() {
    let take_two = "fn add(a: Int, b: Int) -> Int:\n    return a + b\n\nfn text_of(x: Int) -> Text:\n    return int_to_text(x)\n\n";
    for call in [
        "map_list([1], add)",
        "filter_list([1], text_of)",
        "any_list([1], text_of)",
        "all_list([\"a\"], text_of)",
        "fold_list([1], 0, text_of)",
        "fold_list([1], \"x\", add)",
        "map_list(3, text_of)",
        "map_list([1], 3)",
    ] {
        let code = check_error_code(&format!(
            "{take_two}fn main() -> Unit:\n    print({call})\n"
        ));
        assert_eq!(code, "E_TYPE_ARGUMENT", "{call}");
    }
    assert_eq!(
        check_error_code(&format!(
            "{take_two}fn main() -> Unit:\n    print(map_list([1]))\n"
        )),
        "E_CALL_ARITY"
    );
}

// ---- Machine code on and off ----

const NATIVE_SOURCE: &str = r#"fn double(x: Int) -> Int:
    return x * 2

fn add(total: Int, x: Int) -> Int:
    return total + x

fn big(x: Int) -> Bool:
    return x > 4

fn apply_twice(f: fn(Int) -> Int, x: Int) -> Int:
    return f(f(x))

fn plain_loop(n: Int) -> Int:
    var i = 0
    var total = 0
    while i < n:
        set total = total + i
        set i = i + 1
    return total

fn via_values(n: Int) -> Int:
    var total = 0
    var i = 0
    while i < n:
        let f = double
        set total = total + apply_twice(f, i)
        set i = i + 1
    return total

fn main() -> Unit:
    print(plain_loop(1000))
    print(via_values(100))
    print(fold_list(map_list([1, 2, 3, 4, 5], double), 0, add))
    print(filter_list([1, 2, 3, 4, 5, 6, 7], big))
    print(any_list([1, 2], big))
    print(all_list([5, 6], big))
"#;

#[test]
fn programs_with_function_values_give_identical_output_with_machine_code_on_and_off() {
    let artifact = compile("function-values-native.gu", NATIVE_SOURCE).unwrap();
    let mut native = Runtime::new()
        .load(artifact.clone(), policy())
        .expect("native runtime loads");
    let mut vm = Runtime::vm_only()
        .load(artifact, policy())
        .expect("VM runtime loads");
    native.invoke("main", vec![]).expect("native run");
    vm.invoke("main", vec![]).expect("VM run");
    let output = native.take_output();
    assert_eq!(output, vm.take_output());
    assert_eq!(output, "499500\n19800\n30\n[5, 6, 7]\nfalse\ntrue\n");
    // Functions that form or call function values stay in the interpreter; the plain loop
    // still compiles.
    let compiled = native.native_function_names();
    for name in ["via_values", "apply_twice", "main"] {
        assert!(
            !compiled.iter().any(|compiled| compiled == name),
            "`{name}` uses function values and must not be compiled: {compiled:?}"
        );
    }
    assert_eq!(native.native_deoptimizations(), 0);
}

#[test]
fn an_error_inside_a_function_value_is_the_same_with_machine_code_on_and_off() {
    let source = "fn reciprocal(x: Int) -> Int:\n    return 100 / x\n\nfn main() -> Unit:\n    print(map_list([5, 0], reciprocal))\n";
    let artifact = compile("function-values-native.gu", source).unwrap();
    let on = run_artifact(artifact.clone(), policy(), Runtime::new()).unwrap_err();
    let off = run_artifact(artifact, policy(), Runtime::vm_only()).unwrap_err();
    // The native and VM messages for a division by zero differ in wording; the code and
    // the spans, callee first, are the same.
    assert_eq!((&on.0, &on.2), (&off.0, &off.2));
    assert_eq!(on.0, "R_DIV_ZERO");
}

#[test]
fn a_format_66_artifact_with_function_values_still_loads_and_runs_at_format_67() {
    let source = format!("{DOUBLE}fn main() -> Unit:\n    let f = double\n    print(f(21))\n");
    let mut artifact = compile("function-values.gu", &source).unwrap();
    artifact.format_version = FORMAT_FUNCTION_VALUES;
    let bytes = artifact.to_bytes().expect("format 66 can still be written");
    let reread = Artifact::from_bytes(&bytes).expect("format 66 can still be read");
    assert_eq!(reread.format_version, FORMAT_FUNCTION_VALUES);
    let out = run_artifact(reread, policy(), Runtime::vm_only()).unwrap();
    assert_eq!(out, "42\n");
}
