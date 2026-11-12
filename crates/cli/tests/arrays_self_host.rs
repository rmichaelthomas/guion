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

/// Compiles `entry` with the self-hosted compiler and runs the result.
fn self_hosted_stdout(project: &Project, entry: &str) -> String {
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
    let ran = guion(&project.0, &["run", artifact.to_str().unwrap()]);
    assert!(ran.status.success(), "{}", String::from_utf8_lossy(&ran.stderr));
    String::from_utf8_lossy(&ran.stdout).into_owned()
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
"#;

#[test]
fn self_hosted_array_programs_match_the_rust_compiler() {
    let project = project("parity");
    std::fs::write(project.0.join("arrays.gu"), PROGRAM).unwrap();
    let rust = guion(&project.0, &["run", "arrays.gu"]);
    assert!(rust.status.success(), "{}", String::from_utf8_lossy(&rust.stderr));
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
    assert!(diagnostic.contains(code), "{name}: expected {code}, got {diagnostic}");
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
