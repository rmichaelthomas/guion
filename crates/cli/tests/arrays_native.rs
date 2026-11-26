//! Element reads from a numeric array inside a loop compiled to machine code.

use std::path::PathBuf;
use std::process::{Command, Output};

struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("guion-arrays-native-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn guion(scratch: &Scratch, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(args)
        .current_dir(&scratch.0)
        .env("GUION_NO_PROMPT", "1")
        .output()
        .expect("guion starts")
}

fn write(scratch: &Scratch, source: &str) {
    std::fs::write(scratch.0.join("main.gu"), source).unwrap();
}

fn loop_over(make: &str, bound: &str, zero: &str) -> String {
    format!(
        "fn main() -> Unit:\n    let n = 1000\n    let a = {make}\n    var total = {zero}\n    var i = 0\n    while i < {bound}:\n        set total = total + array_get(a, i)\n        set i = i + 1\n    print(total)\n"
    )
}

/// The native-backend section of `guion run --json`.
fn native_report(scratch: &Scratch) -> serde_json::Value {
    let output = guion(scratch, &["run", "--json", "--no-cache", "main.gu"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).expect("a JSON report");
    report["native_backend"].clone()
}

fn stdout_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn array_get_f64_loop_runs_native() {
    let scratch = Scratch::new("f64-native");
    write(
        &scratch,
        &loop_over("array_f64_linspace(0.0, 1.0, n)", "n", "0.0"),
    );
    let native = native_report(&scratch);
    let compiled: Vec<&str> = native["compiled_functions"]
        .as_array()
        .expect("a list")
        .iter()
        .filter_map(|entry| entry["name"].as_str().or_else(|| entry.as_str()))
        .collect();
    assert!(
        compiled.contains(&"main"),
        "main did not compile to machine code: {}",
        native["compile_profile"]["rejected_functions"]
    );
    assert_eq!(native["deoptimizations"], 0, "the loop left machine code");
    let output = guion(&scratch, &["run", "main.gu"]);
    assert_eq!(stdout_of(&output).trim(), "499.9999999999999");
    assert_eq!(stdout_of(&output), stdout_of(&vm_run(&scratch)));
}

/// The VM is the reference: a tape recording runs on it.
fn vm_run(scratch: &Scratch) -> Output {
    guion(scratch, &["run", "--record", "vm.tape", "main.gu"])
}

fn out_of_bounds_program(make: &str, start: &str) -> String {
    format!(
        "fn main() -> Unit:\n    let n = 10\n    let a = {make}\n    var total = 0.0\n    var i = {start}\n    while i <= n:\n        set total = total + array_get(a, i)\n        set i = i + 1\n    print(total)\n"
    )
}

#[test]
fn native_array_get_out_of_bounds_matches_vm() {
    for (name, start) in [("past-end", "0"), ("negative", "0 - 1")] {
        let scratch = Scratch::new(name);
        write(
            &scratch,
            &out_of_bounds_program("array_f64_linspace(0.0, 1.0, n)", start),
        );
        let native = guion(&scratch, &["run", "main.gu"]);
        let vm = vm_run(&scratch);
        assert!(!native.status.success(), "{name}: expected a runtime error");
        assert_eq!(native.status.code(), vm.status.code(), "{name}: exit code");
        let native_error = String::from_utf8_lossy(&native.stderr).into_owned();
        assert!(
            native_error.contains("R_ARRAY_INDEX"),
            "{name}: {native_error}"
        );
        assert_eq!(
            native_error,
            String::from_utf8_lossy(&vm.stderr),
            "{name}: message"
        );
        assert_eq!(stdout_of(&native), stdout_of(&vm), "{name}: output");
    }
}

#[test]
fn non_f64_array_get_guards_the_vm_path_and_error_parity() {
    // This guards the VM path and output/error parity, not native code: i64 reads share the f64
    // lowering, but no narrower or unsigned array (u8 included) reaches native code today, so
    // these programs run through the runtime. Every one must give the VM's answer.
    let cases = [
        ("array_i64_range(0, n, 1)", "0", "499500"),
        ("array_i32_zeros([n])", "0", "0"),
        ("array_u8_full([n], 200)", "0", "200000"),
        (
            "array_u64_full([n], 18446744073709551615)",
            "0",
            "18446744073709551615000",
        ),
        ("array_f32_full([n], 0.5)", "0.0", "500"),
        ("array_bool_full([n], true)", "0", ""),
    ];
    for (index, (make, zero, expected)) in cases.iter().enumerate() {
        if expected.is_empty() {
            continue; // a Bool sum is a type error; covered by the VM's own tests
        }
        let scratch = Scratch::new(&format!("other-{index}"));
        write(&scratch, &loop_over(make, "n", zero));
        let native = guion(&scratch, &["run", "main.gu"]);
        let vm = vm_run(&scratch);
        assert_eq!(stdout_of(&native), stdout_of(&vm), "{make}");
        assert_eq!(
            String::from_utf8_lossy(&native.stderr),
            String::from_utf8_lossy(&vm.stderr),
            "{make}"
        );
        assert_eq!(stdout_of(&native).trim(), *expected, "{make}");
    }
}

#[test]
fn two_dimensional_array_get_guards_the_vm_path_and_output_parity() {
    // This guards the VM path and output parity, not native code: no 2-D array reaches native
    // code today, so the program runs through the runtime and must match the VM's answer.
    let scratch = Scratch::new("two-d");
    write(
        &scratch,
        &loop_over("array_f64_full([10, 100], 0.25)", "n", "0.0"),
    );
    let native = guion(&scratch, &["run", "main.gu"]);
    let vm = vm_run(&scratch);
    assert_eq!(stdout_of(&native), stdout_of(&vm));
    assert_eq!(stdout_of(&native).trim(), "250");
}

#[test]
fn native_array_kernels_charge_the_same_budget_as_the_vm() {
    // A 1,000,000-element f64 array is 8 MB; with the limit at 4 MiB native code and the VM
    // must both stop with the same error.
    let scratch = Scratch::new("budget");
    std::fs::write(
        scratch.0.join("guion.toml"),
        "[package]\nname = \"limited\"\nversion = \"0.1.0\"\n\n[runtime]\nallocation_limit_bytes = 4194304\n",
    )
    .unwrap();
    write(
        &scratch,
        &loop_over("array_f64_linspace(0.0, 1.0, 1000000)", "n", "0.0"),
    );
    let native = guion(&scratch, &["run", "main.gu"]);
    let vm = vm_run(&scratch);
    assert!(!native.status.success());
    let native_error = String::from_utf8_lossy(&native.stderr).into_owned();
    assert!(
        native_error.contains("R_ALLOCATION_LIMIT"),
        "{native_error}"
    );
    let vm_error = String::from_utf8_lossy(&vm.stderr).into_owned();
    assert!(vm_error.contains("R_ALLOCATION_LIMIT"), "{vm_error}");
    // The "in use" figure differs between a recorded and a plain run; the request does not.
    assert!(
        native_error.contains("requested 8000064 more"),
        "{native_error}"
    );
    assert!(vm_error.contains("requested 8000064 more"), "{vm_error}");
}
