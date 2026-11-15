//! The shipped browser page (`docs/browser.md`) is pinned to the code: its ABI version, each
//! limit in its table, and its host-operation statements are asserted here.

use guion_compiler::compile;
use guion_vm_wasm::*;
use std::fs;
use std::path::PathBuf;

fn repository() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn page() -> String {
    fs::read_to_string(repository().join("docs/browser.md")).expect("docs/browser.md exists")
}

#[test]
fn docs_browser_abi_version_matches_constant() {
    let page = page();
    let major = WASM_ABI_VERSION >> 16;
    let minor = WASM_ABI_VERSION & 0xffff;
    assert!(
        page.contains(&format!("ABI {major}.{minor}")),
        "page must name ABI {major}.{minor}"
    );
    assert!(
        page.contains(&format!("0x{major:04x}_{minor:04x}")),
        "page must show the packed value of guion_wasm_abi_version()"
    );
    assert!(page.contains("guion_wasm_abi_version"));
}

fn constants() -> Vec<(&'static str, usize)> {
    vec![
        ("MAX_ARTIFACT_BYTES", MAX_ARTIFACT_BYTES),
        ("MAX_REQUEST_BYTES", MAX_REQUEST_BYTES),
        ("MAX_RESPONSE_BYTES", MAX_RESPONSE_BYTES),
        ("MAX_BROWSER_STEPS", MAX_BROWSER_STEPS as usize),
        ("MAX_OUTPUT_BYTES", MAX_OUTPUT_BYTES),
        ("MAX_ERROR_BYTES", MAX_ERROR_BYTES),
        ("MAX_BROWSER_ALLOCATION_BYTES", MAX_BROWSER_ALLOCATION_BYTES),
        ("MAX_LIVE_INPUTS", MAX_LIVE_INPUTS),
        ("MAX_LIVE_INPUT_BYTES", MAX_LIVE_INPUT_BYTES),
        ("MAX_WIRE_DEPTH", MAX_WIRE_DEPTH),
        ("MAX_WIRE_NODES", MAX_WIRE_NODES),
        ("MAX_WIRE_COLLECTION_ITEMS", MAX_WIRE_COLLECTION_ITEMS),
        ("MAX_WIRE_VALUE_BYTES", MAX_WIRE_VALUE_BYTES),
        ("MAX_WIRE_INTEGER_DIGITS", MAX_WIRE_INTEGER_DIGITS),
    ]
}

#[test]
fn docs_browser_limits_match_constants() {
    let page = page();
    let mut seen = Vec::new();
    for line in page.lines().filter(|line| line.starts_with('|')) {
        let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
        let Some(constant) = cells.iter().find_map(|cell| {
            cell.strip_prefix('`')
                .and_then(|cell| cell.strip_suffix('`'))
                .filter(|name| name.starts_with("MAX_"))
        }) else {
            continue;
        };
        let value = cells
            .iter()
            .find_map(|cell| {
                cell.split_whitespace()
                    .next()
                    .map(|word| word.replace(',', ""))
                    .and_then(|word| word.parse::<usize>().ok())
            })
            .unwrap_or_else(|| panic!("no numeric value in row: {line}"));
        let (_, expected) = *constants()
            .iter()
            .find(|(name, _)| *name == constant)
            .unwrap_or_else(|| panic!("table row for unknown constant {constant}"));
        assert_eq!(value, expected, "{constant} in docs/browser.md");
        seen.push(constant.to_string());
    }
    for (name, _) in constants() {
        assert!(
            seen.iter().any(|seen| seen == name),
            "docs/browser.md has no table row for {name}"
        );
    }
    // Every error code the table names must be one the code can return.
    for code in [
        "E_ABI_LIMIT",
        "R_STEP_LIMIT",
        "R_OUTPUT_LIMIT",
        "R_ALLOCATION_LIMIT",
    ] {
        assert!(page.contains(code), "table names {code}");
    }
}

#[test]
fn docs_name_r_host_unsupported_and_five_classes() {
    let page = page();
    assert!(page.contains("R_HOST_UNSUPPORTED"));
    assert!(page.contains("E_CAPABILITY_UNSUPPORTED"));
    for class in ["filesystem", "network", "database", "clock", "process"] {
        assert!(page.contains(class), "page names the {class} class");
    }
    // The statements are true: a host operation fails with the named code, and a granted
    // capability is refused.
    let artifact = compile(
        "fixture.gu",
        "fn main() -> Int:\n    let content = read_text(\"x\")\n    return 0\n",
    )
    .unwrap()
    .to_bytes()
    .unwrap();
    let response: serde_json::Value = serde_json::from_slice(&execute_artifact_request(
        &artifact,
        br#"{"abi":{"major":1,"minor":0},"entry":"main","arguments":[],"capabilities":[]}"#,
    ))
    .unwrap();
    assert_eq!(response["code"], "R_HOST_UNSUPPORTED");
    let response: serde_json::Value = serde_json::from_slice(&execute_artifact_request(
        &artifact,
        br#"{"abi":{"major":1,"minor":0},"entry":"main","arguments":[],"capabilities":["x"]}"#,
    ))
    .unwrap();
    assert_eq!(response["code"], "E_CAPABILITY_UNSUPPORTED");
}

#[test]
fn function_values_and_higher_order_builtins_run_in_the_browser_vm() {
    let page = page();
    assert!(page.contains("map_list"));
    let run = |source: &str| -> serde_json::Value {
        let artifact = compile("fixture.gu", source).unwrap().to_bytes().unwrap();
        serde_json::from_slice(&execute_artifact_request(
            &artifact,
            br#"{"abi":{"major":1,"minor":0},"entry":"main","arguments":[],"capabilities":[]}"#,
        ))
        .unwrap()
    };
    let direct = run(
        "fn double(x: Int) -> Int:\n    return x * 2\n\nfn main() -> Int:\n    let f = double\n    return f(21)\n",
    );
    assert_eq!(direct["ok"], true, "{direct}");
    assert_eq!(direct["value"]["value"], "42");
    let mapped = run(
        "fn main() -> Int:\n    let k = 10\n    let scaled = map_list([1, 2], fn(x: Int) -> Int: x * k)\n    return scaled[1]\n",
    );
    assert_eq!(mapped["ok"], true, "{mapped}");
    assert_eq!(mapped["value"]["value"], "20");
    let sorted = run("fn main() -> Int:\n    return sort([3, 1, 2])[0]\n");
    assert_eq!(sorted["ok"], true, "{sorted}");
    assert_eq!(sorted["value"]["value"], "1");
}

#[test]
fn agent_index_lists_browser_page() {
    let index: serde_json::Value =
        serde_json::from_slice(&fs::read(repository().join("docs/agents/index.json")).unwrap())
            .unwrap();
    assert_eq!(index["documents"]["browser"], "docs/browser.md");
    assert!(repository().join("docs/browser.md").is_file());
}

#[test]
fn parallel_map_is_the_one_higher_order_operation_the_browser_vm_lacks() {
    let artifact = compile(
        "fixture.gu",
        "fn k(x: Int) -> Int:\n    return 0 - x\n\nfn main() -> Int:\n    let s = parallel_map([1], k, 2)\n    return 0\n",
    )
    .unwrap()
    .to_bytes()
    .unwrap();
    let response: serde_json::Value = serde_json::from_slice(&execute_artifact_request(
        &artifact,
        br#"{"abi":{"major":1,"minor":0},"entry":"main","arguments":[],"capabilities":[]}"#,
    ))
    .unwrap();
    assert_eq!(response["code"], "R_HOST_UNSUPPORTED", "{response}");
    assert!(response["message"]
        .as_str()
        .unwrap()
        .contains("`ParallelMap`"));
    assert!(page().contains("ParallelMap"));
}

#[test]
fn output_past_the_ceiling_is_r_output_limit() {
    let artifact = compile(
        "fixture.gu",
        "fn main() -> Int:\n    var t = \"xxxxxxxxxxxxxxxx\"\n    var i = 0\n    while i < 13:\n        set t = t + t\n        set i = i + 1\n    print(t)\n    return 0\n",
    )
    .unwrap()
    .to_bytes()
    .unwrap();
    let response: serde_json::Value = serde_json::from_slice(&execute_artifact_request(
        &artifact,
        br#"{"abi":{"major":1,"minor":0},"entry":"main","arguments":[],"capabilities":[]}"#,
    ))
    .unwrap();
    assert_eq!(response["code"], "R_OUTPUT_LIMIT");
}
