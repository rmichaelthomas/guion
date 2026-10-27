//! Executes real compiled artifacts (the `.gub` byte format) through the shared VM core.

use guion_compiler::compile;
use guion_model::{Artifact, BigInt, Value};
use guion_vm_core::{PortableHost, VmLimits};
use std::sync::Arc;

const HELLO: &str = include_str!("fixtures/hello.gu");

/// Compile, serialize to the `.gub` format, and decode again, exactly as a loader would.
fn artifact_from_source(name: &str, source: &str) -> Arc<Artifact> {
    let compiled = compile(name, source).expect("fixture compiles");
    let bytes = compiled.to_bytes().expect("artifact serializes");
    assert_eq!(compiled.format_version, Artifact::CURRENT_VERSION);
    Arc::new(Artifact::from_bytes(&bytes).expect("artifact decodes"))
}

fn int(value: i64) -> Value {
    Value::Int(BigInt::from_i64(value))
}

#[test]
fn hello_prints_and_returns_zero() {
    let artifact = artifact_from_source("hello.gu", HELLO);
    let main_len = artifact
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("main exists")
        .code
        .len();
    let mut host = PortableHost::new(artifact, VmLimits::default());

    let run = host.invoke("main", vec![], 32).expect("hello runs");

    assert_eq!(host.take_output(), "Hello, Guion!\n");
    assert_eq!(run.return_value, int(0));
    assert_eq!(
        run.steps as usize, main_len,
        "straight-line code runs each instruction once"
    );
}

#[test]
fn pure_computation_uses_loops_and_arithmetic() {
    let source = "fn main() -> Int:\n    var total = 0\n    var i = 1\n    while i <= 10:\n        set total = total + i\n        set i = i + 1\n    print(total)\n    return total\n";
    let mut host = PortableHost::new(artifact_from_source("sum.gu", source), VmLimits::default());

    let run = host.invoke("main", vec![], 10_000).expect("sum runs");

    assert_eq!(host.take_output(), "55\n");
    assert_eq!(run.return_value, int(55));
}

#[test]
fn local_resolver_signature_verifies_with_the_portable_vm_stack_types() {
    use std::{collections::HashMap, fs, path::PathBuf};

    let root = std::env::temp_dir().join(format!(
        "guion-vm-temporal-types-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    let entry = root.join("main.gu");
    fs::write(
        &entry,
        "import \"std:time\"\nfn main() -> Int:\n    let resolution = unwrap(resolve_local(LocalDateTime(2026, 11, 1, 1, 30, 0, 0), \"America/Los_Angeles\"))\n    return len(resolution.instants)\n",
    )
    .unwrap();
    let stdlib = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../language/stdlib");
    let artifact =
        guion_compiler::compile_project_with_roots(&entry, &root, Some(&stdlib), &HashMap::new())
            .expect("local resolver source compiles");
    guion_vm_core::verify(&artifact).expect("resolver stack types verify in portable VM");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn function_calls_recurse_and_return() {
    let source = "fn fib(n: Int) -> Int:\n    if n < 2:\n        return n\n    return fib(n - 1) + fib(n - 2)\n\nfn main() -> Int:\n    return fib(10)\n";
    let mut host = PortableHost::new(artifact_from_source("fib.gu", source), VmLimits::default());

    let run = host.invoke("main", vec![], 100_000).expect("fib runs");

    assert_eq!(run.return_value, int(55));
}

#[test]
fn call_depth_limit_is_enforced() {
    let source = "fn down(n: Int) -> Int:\n    return down(n + 1)\n\nfn main() -> Int:\n    return down(0)\n";
    let limits = VmLimits {
        call_depth_limit: 16,
        ..VmLimits::default()
    };
    let mut host = PortableHost::new(artifact_from_source("depth.gu", source), limits);

    let error = host
        .invoke("main", vec![], 1_000_000)
        .expect_err("recursion is bounded");

    assert_eq!(error.code, "R_CALL_DEPTH");
}

#[test]
fn instruction_budget_exhaustion_is_an_error() {
    let source =
        "fn main() -> Int:\n    var i = 0\n    while true:\n        set i = i + 1\n    return i\n";
    let mut host = PortableHost::new(artifact_from_source("spin.gu", source), VmLimits::default());

    let error = host
        .invoke("main", vec![], 32)
        .expect_err("budget is enforced");

    assert_eq!(error.code, "R_STEP_LIMIT");
    assert_eq!(host.steps_used(), 32);
}

#[test]
fn unsupported_host_operation_fails_explicitly() {
    let source = "fn main() -> Int:\n    let content = read_text(\"input.txt\")\n    print(is_ok(content))\n    return 0\n";
    let mut host = PortableHost::new(
        artifact_from_source("host-op.gu", source),
        VmLimits::default(),
    );

    let error = host
        .invoke("main", vec![], 1_000)
        .expect_err("read_text is not available");

    assert_eq!(error.code, "R_HOST_UNSUPPORTED");
    assert!(error.message.contains("ReadText"), "{}", error.message);
    assert_eq!(
        host.take_output(),
        "",
        "nothing runs past the rejected operation"
    );
}

#[test]
fn unknown_export_and_arity_are_rejected() {
    let artifact = artifact_from_source("hello.gu", HELLO);
    let mut host = PortableHost::new(artifact, VmLimits::default());

    assert_eq!(
        host.invoke("nope", vec![], 32)
            .expect_err("unknown export")
            .code,
        "R_EXPORT_UNKNOWN"
    );
    assert_eq!(
        host.invoke("main", vec![int(1)], 32)
            .expect_err("arity")
            .code,
        "R_CALL_ARITY"
    );
}
