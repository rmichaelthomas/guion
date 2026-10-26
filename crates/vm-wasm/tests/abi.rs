//! Native checks of the adapter logic using real `.gub` bytes. The u32-pointer ABI is only
//! meaningful on wasm32 and is exercised against the built module instead.

use guion_compiler::compile;
use guion_vm_wasm::*;

fn gub(source: &str) -> Vec<u8> {
    compile("fixture.gu", source)
        .expect("fixture compiles")
        .to_bytes()
        .expect("artifact serializes")
}

const HELLO: &str = include_str!("../../vm-core/tests/fixtures/hello.gu");

#[test]
fn hello_runs_through_the_shared_vm() {
    let result = run_artifact(&gub(HELLO), 32).expect("hello runs");
    assert_eq!(result.output, "Hello, Guion!\n");
    assert_eq!(result.return_value, 0);
    assert!(result.steps > 0 && result.steps <= 32);
}

#[test]
fn failures_report_clear_errors() {
    let spin = gub(
        "fn main() -> Int:\n    var i = 0\n    while true:\n        set i = i + 1\n    return i\n",
    );
    assert!(run_artifact(&spin, 32)
        .unwrap_err()
        .contains("R_STEP_LIMIT"));

    let host = gub("fn main() -> Int:\n    let content = read_text(\"x\")\n    return 0\n");
    let error = run_artifact(&host, 100).unwrap_err();
    assert!(
        error.contains("R_HOST_UNSUPPORTED") && error.contains("ReadText"),
        "{error}"
    );

    assert!(run_artifact(b"not an artifact", 32).is_err());
    assert!(run_artifact(&gub(HELLO), MAX_BROWSER_STEPS + 1).is_err());
    assert!(run_artifact(&vec![0; MAX_ARTIFACT_BYTES + 1], 32).is_err());
}
