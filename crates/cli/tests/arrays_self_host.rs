//! The self-hosted compiler accepts, rejects and compiles array programs exactly as the Rust
//! compiler does, and its programs print the same output.

use std::path::{Path, PathBuf};
use std::process::Command;

struct Project(PathBuf);

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn project(name: &str) -> Project {
    let dir = std::env::temp_dir().join(format!("guion-arrays-self-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let self_host = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../language/self_host");
    for file in [
        "guion.toml",
        "compiler.gu",
        "lexer.gu",
        "parser.gu",
        "builtin_catalog.gu",
        "artifact.gu",
    ] {
        std::fs::copy(self_host.join(file), dir.join(file)).unwrap();
    }
    Project(dir)
}

fn guion(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("guion starts")
}

/// Compiles `entry` with the self-hosted compiler and assembles the result; returns the path of
/// the artifact.
fn self_hosted_artifact(project: &Project, entry: &str) -> PathBuf {
    let compiled = guion(&project.0, &["run", "compiler.gu", entry]);
    assert!(
        compiled.status.success(),
        "self-hosted compile failed: {}{}",
        String::from_utf8_lossy(&compiled.stdout),
        String::from_utf8_lossy(&compiled.stderr)
    );
    let ir = project.0.join(format!("{entry}.ir.json"));
    std::fs::write(&ir, &compiled.stdout).unwrap();
    let artifact = project.0.join(format!("{entry}.gub"));
    let assembled = guion(
        &project.0,
        &[
            "assemble-ir",
            ir.to_str().unwrap(),
            "--output",
            artifact.to_str().unwrap(),
        ],
    );
    assert!(
        assembled.status.success(),
        "{}",
        String::from_utf8_lossy(&assembled.stderr)
    );
    artifact
}

/// Compiles `entry` with the self-hosted compiler and runs the result.
fn self_hosted_stdout(project: &Project, entry: &str) -> String {
    let artifact = self_hosted_artifact(project, entry);
    let ran = guion(&project.0, &["run", artifact.to_str().unwrap()]);
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    String::from_utf8_lossy(&ran.stdout).into_owned()
}

/// Every function's instructions, as text, in the order the compiler emitted them.
fn instructions(artifact: &guion_model::Artifact) -> Vec<(String, Vec<String>)> {
    artifact
        .functions
        .iter()
        .map(|function| {
            (
                function.name.clone(),
                function
                    .code
                    .iter()
                    .map(|instruction| format!("{:?}", instruction.op))
                    .collect(),
            )
        })
        .collect()
}

const PROGRAM: &str = r#"fn total(a: Array[i64]) -> Int:
    return array_sum(a)

fn main() -> Unit:
    let a = array_reshape(array_i32([1, 2, 3, 4, 5, 6]), [2, 3])
    let f = array_f64_linspace(0.0, 1.0, 5)
    var v = array_f64_zeros([3])
    set v = array_set(v, 1, 2.5)
    print(v)
    print(array_add(a, array_i32([10, 20, 30])))
    print(array_mul_scalar(f, 2.0))
    print(array_get(a, 4) + 1)
    print(array_get_at(a, [1, 2]))
    print(array_shape(array_transpose(a)))
    print(array_to_f32(array_i64([1, 2])))
    print(array_filter(array_to_i64(array_flatten(a)), array_gt_scalar(array_flatten(a), 3)))
    print(array_any(array_eq_scalar(a, 5)))
    print(array_mean(f))
    print(array_sqrt(array_f64([4.0])))
    print(array_matmul(a, array_i32([1, 0, 1])))
    print(array_concat([a, a]))
    print(array_to_list(array_u8([1, 2])))
    print(array_u16_from_bytes(array_to_bytes(array_u16([513]))))
    print(total(array_i64_range(0, 5, 1)))
    print(array_sort(array_f64([3.0, 1.0])))
    print(array_where(array_bool([true, false]), array_i8([1, 2]), array_i8([3, 4])))
    print(array_f64_random(3, [2]) == array_f64_random(3, [2]))
    let codes = array_factorize(array_i64([7, 3, 7]))
    print(array_take(array_f64([1.5, 2.5]), array_i64([1, 0, 1])))
    print(array_sum_by(array_i32([1, 2, 3]), codes, 2))
    print(array_mean_by(array_f64([1.0, 2.0, 4.0]), codes, 2))
    print(array_count_by(codes, 2))
    print(array_first_index(codes, 2))
    print(array_join_index(codes, array_i64([1, -1]), true))
"#;

#[test]
fn self_hosted_array_programs_match_the_rust_compiler() {
    let project = project("parity");
    std::fs::write(project.0.join("arrays.gu"), PROGRAM).unwrap();
    let rust = guion(&project.0, &["run", "arrays.gu"]);
    assert!(
        rust.status.success(),
        "{}",
        String::from_utf8_lossy(&rust.stderr)
    );
    let expected = String::from_utf8_lossy(&rust.stdout).into_owned();
    assert!(expected.contains("[0, 2.5, 0]"), "{expected}");
    assert_eq!(self_hosted_stdout(&project, "arrays.gu"), expected);
}

fn assert_both_reject(project: &Project, name: &str, body: &str, code: &str) {
    let entry = format!("{name}.gu");
    let source = format!("fn main() -> Unit:\n{body}");
    std::fs::write(project.0.join(&entry), &source).unwrap();
    let rust = guion_compiler::compile_project(&project.0.join(&entry), &project.0)
        .expect_err("the Rust compiler must reject the program");
    assert_eq!(rust.code, code, "{name}: {}", rust.message);
    let compiled = guion(&project.0, &["run", "compiler.gu", &entry]);
    let diagnostic = String::from_utf8_lossy(&compiled.stdout);
    assert!(
        diagnostic.contains(code),
        "{name}: expected {code}, got {diagnostic}"
    );
}

#[test]
fn self_hosted_compiler_rejects_what_the_rust_compiler_rejects() {
    let project = project("reject");
    assert_both_reject(
        &project,
        "numeric",
        "    print(array_add(array_bool([true]), array_bool([true])))\n",
        "E_TYPE_ARGUMENT",
    );
    assert_both_reject(
        &project,
        "float",
        "    print(array_sqrt(array_i32([4])))\n",
        "E_TYPE_ARGUMENT",
    );
    assert_both_reject(
        &project,
        "mismatch",
        "    print(array_add(array_f64([1.0]), array_f32([1.0])))\n",
        "E_TYPE_ARGUMENT",
    );
    assert_both_reject(
        &project,
        "elem",
        "    print(array_set(array_f64([1.0]), 0, 1))\n",
        "E_TYPE_ARGUMENT",
    );
    assert_both_reject(
        &project,
        "not-array",
        "    print(array_size([1, 2]))\n",
        "E_TYPE_ARGUMENT",
    );
    std::fs::write(
        project.0.join("tag.gu"),
        "fn f(x: Array[f16]) -> Int:\n    return 1\n\nfn main() -> Unit:\n    print(1)\n",
    )
    .unwrap();
    let rust = guion_compiler::compile_project(&project.0.join("tag.gu"), &project.0)
        .expect_err("unknown element type");
    let compiled = guion(&project.0, &["run", "compiler.gu", "tag.gu"]);
    assert!(
        String::from_utf8_lossy(&compiled.stdout).contains(&rust.code),
        "{} vs {}",
        rust.code,
        String::from_utf8_lossy(&compiled.stdout)
    );
}

const ELEMENT_LIST: &str =
    "Array element type must be one of i8, i16, i32, i64, u8, u16, u32, u64, f32, f64, bool";

fn assert_both_report_type_error(
    project: &Project,
    name: &str,
    source: &str,
    code: &str,
    message: &str,
) {
    let entry = format!("{name}.gu");
    std::fs::write(project.0.join(&entry), source).unwrap();
    let rust = guion_compiler::compile_project(&project.0.join(&entry), &project.0)
        .expect_err("the Rust compiler must reject the program");
    assert_eq!((rust.code.as_str(), rust.message.as_str()), (code, message));
    let compiled = guion(&project.0, &["run", "compiler.gu", &entry]);
    let diagnostic = String::from_utf8_lossy(&compiled.stdout);
    assert!(
        diagnostic.contains(&format!("\"code\":\"{code}\""))
            && diagnostic.contains(&format!("\"message\":\"{message}\"")),
        "{name}: expected {code} {message}, got {diagnostic}"
    );
}

#[test]
fn both_compilers_report_the_same_array_element_type_errors() {
    let project = project("element-types");
    let main = "fn main() -> Unit:\n    print(1)\n";
    assert_both_report_type_error(
        &project,
        "unknown",
        &format!("fn f(x: Array[i128]) -> Int:\n    return 1\n\n{main}"),
        "E_TYPE_UNKNOWN",
        "unknown type `i128`",
    );
    assert_both_report_type_error(
        &project,
        "nested",
        &format!("fn f(x: Array[Array[f64]]) -> Int:\n    return 1\n\n{main}"),
        "E_TYPE_ANNOTATION",
        ELEMENT_LIST,
    );
    assert_both_report_type_error(
        &project,
        "list-of-element",
        &format!("fn f(x: Array[List[f64]]) -> Int:\n    return 1\n\n{main}"),
        "E_TYPE_UNKNOWN",
        "unknown type `f64`; element types are only valid inside Array[...]",
    );
    assert_both_report_type_error(
        &project,
        "map-of-element",
        &format!("fn f(x: Array[Map[Text, i32]]) -> Int:\n    return 1\n\n{main}"),
        "E_TYPE_UNKNOWN",
        "unknown type `i32`; element types are only valid inside Array[...]",
    );
    assert_both_report_type_error(
        &project,
        "int",
        &format!("fn f(x: Array[Int]) -> Int:\n    return 1\n\n{main}"),
        "E_TYPE_ANNOTATION",
        ELEMENT_LIST,
    );
    assert_both_report_type_error(
        &project,
        "record",
        &format!(
            "record Point:\n    x: Int\n\nfn f(x: Array[Point]) -> Int:\n    return 1\n\n{main}"
        ),
        "E_TYPE_ANNOTATION",
        ELEMENT_LIST,
    );
    assert_both_report_type_error(
        &project,
        "bare-param",
        &format!("fn f(x: f64) -> Int:\n    return 1\n\n{main}"),
        "E_TYPE_UNKNOWN",
        "unknown type `f64`; element types are only valid inside Array[...]",
    );
    assert_both_report_type_error(
        &project,
        "bare-let",
        "fn main() -> Unit:\n    let x: f64 = 1.0\n    print(x)\n",
        "E_TYPE_UNKNOWN",
        "unknown type `f64`; element types are only valid inside Array[...]",
    );
    // A bare element type is reported wherever a type is written: a result, a record field,
    // an external signature and a type inside another type.
    assert_both_report_type_error(
        &project,
        "bare-result",
        &format!("fn f() -> f64:\n    return 1.0\n\n{main}"),
        "E_TYPE_UNKNOWN",
        "unknown type `f64`; element types are only valid inside Array[...]",
    );
    assert_both_report_type_error(
        &project,
        "bare-field",
        &format!("record Point:\n    x: f64\n\n{main}"),
        "E_TYPE_UNKNOWN",
        "unknown type `f64`; element types are only valid inside Array[...]",
    );
    assert_both_report_type_error(
        &project,
        "bare-external",
        &format!("extern python \"math:sqrt\" fn root(value: f64) -> Int\n\n{main}"),
        "E_TYPE_UNKNOWN",
        "unknown type `f64`; element types are only valid inside Array[...]",
    );
    assert_both_report_type_error(
        &project,
        "bare-in-list",
        &format!("fn f(x: List[f64]) -> Int:\n    return 1\n\n{main}"),
        "E_TYPE_UNKNOWN",
        "unknown type `f64`; element types are only valid inside Array[...]",
    );
    assert_both_report_type_error(
        &project,
        "array-field",
        &format!("record Point:\n    x: Array[i128]\n\n{main}"),
        "E_TYPE_UNKNOWN",
        "unknown type `i128`",
    );
    assert_both_report_type_error(
        &project,
        "array-result",
        &format!("fn f() -> Array[Int]:\n    return array_i32([1])\n\n{main}"),
        "E_TYPE_ANNOTATION",
        ELEMENT_LIST,
    );
    assert_both_report_type_error(
        &project,
        "let-unknown",
        "fn main() -> Unit:\n    let x: Array[i128] = array_i32([1])\n    print(1)\n",
        "E_TYPE_UNKNOWN",
        "unknown type `i128`",
    );
}

const CSV_PROGRAM: &str = r#"record CsvColumn:
    name: Text
    kind: Text
    ints: Array[i64]
    floats: Array[f64]
    flags: Array[bool]
    codes: Array[i64]
    dictionary: List[Text]
    valid: Array[bool]

fn main() -> Unit:
    let none: Map[Text, Text] = {}
    let columns = unwrap(csv_read_columns("data.csv", none))
    for column in columns:
        print(column.name + " " + column.kind)
        print(column.valid)
    let fixed: Map[Text, Text] = {"a": "f64"}
    print(array_sum(unwrap(csv_read_columns("data.csv", fixed))[0].floats))
"#;

#[test]
fn self_hosted_csv_read_columns_program_matches_the_rust_compiler() {
    let project = project("csv-columns");
    std::fs::write(
        project.0.join("guion.toml"),
        format!(
            "{}\n[capabilities]\nfiles = [\"read\"]\n",
            std::fs::read_to_string(project.0.join("guion.toml")).unwrap()
        ),
    )
    .unwrap();
    std::fs::write(project.0.join("data.csv"), "a,b\n1,x\n,y\n3,\n").unwrap();
    std::fs::write(project.0.join("cols.gu"), CSV_PROGRAM).unwrap();
    let rust = guion(&project.0, &["run", "cols.gu"]);
    assert!(
        rust.status.success(),
        "{}{}",
        String::from_utf8_lossy(&rust.stdout),
        String::from_utf8_lossy(&rust.stderr)
    );
    let expected = String::from_utf8_lossy(&rust.stdout).into_owned();
    assert!(expected.contains("a i64"), "{expected}");
    assert!(expected.contains("b text"), "{expected}");
    assert_eq!(self_hosted_stdout(&project, "cols.gu"), expected);
    // Not only the same output: the same instructions, so the new operation is encoded as the
    // Rust compiler encodes it.
    let rust_artifact = guion_compiler::compile_project(&project.0.join("cols.gu"), &project.0)
        .expect("the Rust compiler compiles the program");
    let self_hosted = guion_model::Artifact::from_bytes(
        &std::fs::read(self_hosted_artifact(&project, "cols.gu")).unwrap(),
    )
    .expect("the assembled artifact decodes");
    let rust_instructions = instructions(&rust_artifact);
    assert_eq!(
        rust_instructions
            .iter()
            .flat_map(|(_, ops)| ops)
            .filter(|op| op.starts_with("CsvReadColumns"))
            .count(),
        2
    );
    assert_eq!(instructions(&self_hosted), rust_instructions);
}

#[test]
fn self_hosted_compiler_emits_array_set_local_like_the_rust_compiler() {
    let project = project("set-local");
    let source = "fn main() -> Unit:\n    var v = array_f64_zeros([3])\n    set v = array_set(v, 1, 2.5)\n    var w = array_i32([1, 2])\n    set w = array_set(w, 0, 7)\n    print(v)\n    print(w)\n";
    std::fs::write(project.0.join("set.gu"), source).unwrap();
    let compiled = guion(&project.0, &["run", "compiler.gu", "set.gu"]);
    assert!(compiled.status.success());
    let ir = String::from_utf8_lossy(&compiled.stdout).into_owned();
    assert_eq!(ir.matches("ArraySetLocal").count(), 2, "{ir}");
    let rust_count = guion_compiler::compile_project(&project.0.join("set.gu"), &project.0)
        .expect("compiles")
        .functions
        .iter()
        .flat_map(|function| function.code.iter())
        .filter(|instruction| matches!(instruction.op, guion_model::Op::ArraySetLocal(_)))
        .count();
    assert_eq!(rust_count, 2);
    let rust = guion(&project.0, &["run", "set.gu"]);
    assert_eq!(
        self_hosted_stdout(&project, "set.gu"),
        String::from_utf8_lossy(&rust.stdout)
    );
}
