//! `docs/basics.md` is a versioned list of the tasks a small program needs and what covers each.
//! The claim it supports is "covers the basics as of this list", so this test fails when a function
//! the list names is missing from the shipped standard library or the built-in functions.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
}

/// Every function a program can call: the `std:` modules from `guion doc --all --json` and the
/// built-in functions from `language/builtin-signatures.json`.
fn shipped_names() -> BTreeSet<String> {
    let doc = Command::new(env!("CARGO_BIN_EXE_guion"))
        .args(["doc", "--all", "--json"])
        .output()
        .expect("guion starts");
    assert!(
        doc.status.success(),
        "{}",
        String::from_utf8_lossy(&doc.stderr)
    );
    let surface: serde_json::Value = serde_json::from_slice(&doc.stdout).unwrap();
    let mut names = BTreeSet::new();
    for module in surface["modules"].as_array().unwrap() {
        for function in module["functions"].as_array().unwrap() {
            names.insert(function["name"].as_str().unwrap().to_owned());
        }
    }
    let builtins: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root().join("language/builtin-signatures.json")).unwrap(),
    )
    .unwrap();
    for entry in builtins["entries"].as_array().unwrap() {
        names.insert(entry["name"].as_str().unwrap().to_owned());
    }
    names
}

/// The rows of the "Covered" table: (basic, names in the "Covered by" column).
fn covered_rows(text: &str) -> Vec<(String, Vec<String>)> {
    let section = text
        .split("## Covered")
        .nth(1)
        .and_then(|rest| rest.split("## Deliberately not covered").next())
        .expect("docs/basics.md has a Covered section");
    section
        .lines()
        .filter(|line| {
            line.starts_with('|') && !line.starts_with("|---") && !line.contains("| Basic |")
        })
        .map(|line| {
            let cells: Vec<&str> = line.split('|').map(str::trim).collect();
            let names = cells[2]
                .split('`')
                .skip(1)
                .step_by(2)
                .map(str::to_owned)
                .collect();
            (cells[1].to_owned(), names)
        })
        .collect()
}

#[test]
fn every_function_the_basics_list_names_is_shipped() {
    let text = std::fs::read_to_string(root().join("docs/basics.md")).unwrap();
    let shipped = shipped_names();
    let rows = covered_rows(&text);
    assert!(rows.len() >= 25, "the list has {} rows", rows.len());
    let mut missing = Vec::new();
    for (basic, names) in &rows {
        assert!(!names.is_empty(), "`{basic}` names no function");
        for name in names {
            if !shipped.contains(name) {
                missing.push(format!("{basic}: {name}"));
            }
        }
    }
    assert!(missing.is_empty(), "named but not shipped: {missing:?}");
}

#[test]
fn the_list_is_versioned_names_the_release_and_lists_what_it_declines() {
    let text = std::fs::read_to_string(root().join("docs/basics.md")).unwrap();
    let version = env!("CARGO_PKG_VERSION");
    assert!(
        text.contains(&format!("as of Guion {version}")),
        "docs/basics.md must name release {version}"
    );
    assert!(text.contains("**Version 1"), "the list carries a version");
    assert!(text.contains("covers the basics as of this list"));
    assert!(text.contains("## Deliberately not covered"));
}
