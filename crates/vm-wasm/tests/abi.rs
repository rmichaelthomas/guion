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
fn reports_the_typed_v1_abi_version() {
    assert_eq!(guion_wasm_abi_version(), 0x0001_0000);
}

#[test]
fn v1_accepts_typed_arguments_and_returns_a_typed_non_integer_value() {
    let artifact = gub("fn greet(name: Text) -> Text:\n    return \"Hello, \" + name\n\nfn main() -> Int:\n    return 0\n");
    let response = execute_artifact_request(
        &artifact,
        br#"{"abi":{"major":1,"minor":0},"entry":"greet","arguments":[{"type":"Text","value":"Ada"}],"capabilities":[],"limits":{"steps":1000,"outputBytes":1024}}"#,
    );
    let response: serde_json::Value = serde_json::from_slice(&response).unwrap();
    assert_eq!(response["abi"]["major"], 1);
    assert_eq!(response["ok"], true);
    assert_eq!(response["value"]["type"], "Text");
    assert_eq!(response["value"]["value"], "Hello, Ada");
    assert!(response["steps"].as_u64().unwrap() > 0);
}

#[test]
fn v1_preserves_arbitrary_precision_integer_arguments_and_results() {
    let artifact =
        gub("fn echo(value: Int) -> Int:\n    return value\n\nfn main() -> Int:\n    return 0\n");
    let response = execute_artifact_request(
        &artifact,
        br#"{"abi":{"major":1,"minor":0},"entry":"echo","arguments":[{"type":"Int","value":"1234567890123456789012345678901234567890"}],"capabilities":[]}"#,
    );
    let response: serde_json::Value = serde_json::from_slice(&response).unwrap();
    assert_eq!(response["ok"], true);
    assert_eq!(response["value"]["type"], "Int");
    assert_eq!(
        response["value"]["value"],
        "1234567890123456789012345678901234567890"
    );
}

#[test]
fn v1_fails_closed_for_capabilities_and_returns_stable_errors() {
    let artifact = gub("fn main() -> Int:\n    return 0\n");
    let response = execute_artifact_request(
        &artifact,
        br#"{"abi":{"major":1,"minor":0},"entry":"main","arguments":[],"capabilities":["document.read"]}"#,
    );
    let response: serde_json::Value = serde_json::from_slice(&response).unwrap();
    assert_eq!(response["ok"], false);
    assert_eq!(response["code"], "E_CAPABILITY_UNSUPPORTED");

    let malformed = execute_artifact_request(&artifact, b"{not json");
    let malformed: serde_json::Value = serde_json::from_slice(&malformed).unwrap();
    assert_eq!(malformed["ok"], false);
    assert_eq!(malformed["code"], "E_ABI_REQUEST");

    let mut invalid_utf8 = br#"{"abi":{"major":1,"minor":0},"entry":"main","arguments":[],"capabilities":[],"extra":"x"}"#.to_vec();
    *invalid_utf8.last_mut().unwrap() = 0xff;
    let invalid_utf8 = execute_artifact_request(&artifact, &invalid_utf8);
    let invalid_utf8: serde_json::Value = serde_json::from_slice(&invalid_utf8).unwrap();
    assert_eq!(invalid_utf8["ok"], false);
    assert_eq!(invalid_utf8["code"], "E_ABI_REQUEST");
}

#[test]
fn v1_rejects_noncanonical_values_and_excessive_limits() {
    let artifact =
        gub("fn echo(value: Int) -> Int:\n    return value\n\nfn main() -> Int:\n    return 0\n");
    let noncanonical = execute_artifact_request(
        &artifact,
        br#"{"abi":{"major":1,"minor":0},"entry":"echo","arguments":[{"type":"Int","value":"01"}],"capabilities":[]}"#,
    );
    let noncanonical: serde_json::Value = serde_json::from_slice(&noncanonical).unwrap();
    assert_eq!(noncanonical["ok"], false);
    assert_eq!(noncanonical["code"], "E_ABI_VALUE");

    let too_many_steps = execute_artifact_request(
        &artifact,
        br#"{"abi":{"major":1,"minor":0},"entry":"echo","arguments":[{"type":"Int","value":"1"}],"capabilities":[],"limits":{"steps":10001}}"#,
    );
    let too_many_steps: serde_json::Value = serde_json::from_slice(&too_many_steps).unwrap();
    assert_eq!(too_many_steps["ok"], false);
    assert_eq!(too_many_steps["code"], "E_ABI_LIMIT");
}

#[test]
fn v1_reports_typed_argument_mismatch_without_coercion() {
    let artifact =
        gub("fn echo(value: Text) -> Text:\n    return value\n\nfn main() -> Int:\n    return 0\n");
    let response = execute_artifact_request(
        &artifact,
        br#"{"abi":{"major":1,"minor":0},"entry":"echo","arguments":[{"type":"Int","value":"7"}],"capabilities":[]}"#,
    );
    let response: serde_json::Value = serde_json::from_slice(&response).unwrap();
    assert_eq!(response["ok"], false);
    assert_eq!(response["code"], "R_CALL_TYPE");
}

#[test]
fn v1_enforces_the_caller_output_budget_and_strict_request_fields() {
    let artifact = gub("fn main() -> Int:\n    print(\"hello\")\n    return 0\n");
    let output_limited = execute_artifact_request(
        &artifact,
        br#"{"abi":{"major":1,"minor":0},"entry":"main","arguments":[],"capabilities":[],"limits":{"outputBytes":1}}"#,
    );
    let output_limited: serde_json::Value = serde_json::from_slice(&output_limited).unwrap();
    assert_eq!(output_limited["ok"], false);
    assert_eq!(output_limited["code"], "R_OUTPUT_LIMIT");

    let unknown_field = execute_artifact_request(
        &artifact,
        br#"{"abi":{"major":1,"minor":0},"entry":"main","arguments":[],"capabilities":[],"dom":true}"#,
    );
    let unknown_field: serde_json::Value = serde_json::from_slice(&unknown_field).unwrap();
    assert_eq!(unknown_field["ok"], false);
    assert_eq!(unknown_field["code"], "E_ABI_REQUEST");
}

#[test]
fn v1_rejects_overdeep_values_before_vm_invocation() {
    let artifact = gub(
        "fn echo(value: Option[Option[Unit]]) -> Option[Option[Unit]]:\n    return value\n\nfn main() -> Int:\n    return 0\n",
    );
    let mut nested = r#"{"type":"Unit"}"#.to_owned();
    for _ in 0..=MAX_WIRE_DEPTH {
        nested = format!(r#"{{"type":"OptionSome","value":{nested}}}"#);
    }
    let request = format!(
        r#"{{"abi":{{"major":1,"minor":0}},"entry":"echo","arguments":[{nested}],"capabilities":[]}}"#
    );
    let response = execute_artifact_request(&artifact, request.as_bytes());
    let response: serde_json::Value = serde_json::from_slice(&response).unwrap();
    assert_eq!(response["ok"], false);
    assert_eq!(response["code"], "E_ABI_LIMIT");
}

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
