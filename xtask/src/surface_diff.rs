//! Additions-only comparison of two `guion doc --all --json` surfaces (VERSIONING.md section 2:
//! a MINOR release adds, and removes or re-signs nothing).
//!
//! A function is re-signed when its parameter list (names and types), its result type or its
//! effects differ. A record is re-signed when a field it already had is removed or changes type.
//! Added modules, functions, records and record fields are reported as additions and pass.
//! Everything else in the document (the `deprecated` note, `source`, summaries) is not part of
//! the signature and is ignored.
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

fn read(path: &str) -> Result<Value, String> {
    let text =
        fs::read_to_string(Path::new(path)).map_err(|error| format!("read {path}: {error}"))?;
    let value: Value =
        serde_json::from_str(&text).map_err(|error| format!("parse {path}: {error}"))?;
    if value["kind"] != "guion_std_surface" {
        return Err(format!(
            "{path} is not a `guion doc --all --json` document (kind `guion_std_surface`)"
        ));
    }
    Ok(value)
}

fn by_name<'a>(items: &'a Value, key: &str) -> BTreeMap<&'a str, &'a Value> {
    items
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| Some((item[key].as_str()?, item)))
        .collect()
}

fn signature(function: &Value) -> String {
    let parameters = function["parameters"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|parameter| {
            format!(
                "{}: {}",
                parameter["name"].as_str().unwrap_or("?"),
                parameter["type"].as_str().unwrap_or("?")
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let effects = function["effects"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "({parameters}) -> {} effects [{effects}]",
        function["result"].as_str().unwrap_or("?")
    )
}

/// `Ok(additions)` when `to` only adds to `from`; `Err(problems)` listing every removal and
/// re-signing otherwise.
pub fn compare(from: &Value, to: &Value) -> Result<Vec<String>, Vec<String>> {
    let mut added = Vec::new();
    let mut problems = Vec::new();
    let from_modules = by_name(&from["modules"], "module");
    let to_modules = by_name(&to["modules"], "module");
    for (name, old) in &from_modules {
        let Some(new) = to_modules.get(name) else {
            problems.push(format!("removed module std:{name}"));
            continue;
        };
        let old_functions = by_name(&old["functions"], "name");
        let new_functions = by_name(&new["functions"], "name");
        for (function, old_function) in &old_functions {
            match new_functions.get(function) {
                None => problems.push(format!("removed function std:{name}.{function}")),
                Some(new_function) => {
                    let (before, after) = (signature(old_function), signature(new_function));
                    if before != after {
                        problems.push(format!(
                            "re-signed function std:{name}.{function}: {before} became {after}"
                        ));
                    }
                }
            }
        }
        for function in new_functions.keys() {
            if !old_functions.contains_key(function) {
                added.push(format!("added std:{name}.{function}"));
            }
        }
        let old_records = by_name(&old["records"], "name");
        let new_records = by_name(&new["records"], "name");
        for (record, old_record) in &old_records {
            let Some(new_record) = new_records.get(record) else {
                problems.push(format!("removed record std:{name}.{record}"));
                continue;
            };
            let old_fields = by_name(&old_record["fields"], "name");
            let new_fields = by_name(&new_record["fields"], "name");
            for (field, old_field) in &old_fields {
                match new_fields.get(field) {
                    None => problems.push(format!("removed field std:{name}.{record}.{field}")),
                    Some(new_field) if new_field["type"] != old_field["type"] => {
                        problems.push(format!(
                            "re-signed field std:{name}.{record}.{field}: {} became {}",
                            old_field["type"].as_str().unwrap_or("?"),
                            new_field["type"].as_str().unwrap_or("?")
                        ));
                    }
                    Some(_) => {}
                }
            }
            for field in new_fields.keys() {
                if !old_fields.contains_key(field) {
                    added.push(format!("added field std:{name}.{record}.{field}"));
                }
            }
        }
        for record in new_records.keys() {
            if !old_records.contains_key(record) {
                added.push(format!("added record std:{name}.{record}"));
            }
        }
    }
    for name in to_modules.keys() {
        if !from_modules.contains_key(name) {
            added.push(format!("added module std:{name}"));
        }
    }
    if problems.is_empty() {
        Ok(added)
    } else {
        Err(problems)
    }
}

/// `cargo xtask surface-diff --from FILE.json --to FILE.json`.
pub fn run(args: &[String]) -> i32 {
    let value_of = |flag: &str| {
        args.iter()
            .position(|arg| arg == flag)
            .and_then(|index| args.get(index + 1))
    };
    let (Some(from), Some(to)) = (value_of("--from"), value_of("--to")) else {
        eprintln!("usage: cargo xtask surface-diff --from FILE.json --to FILE.json");
        return 2;
    };
    let documents = read(from).and_then(|from| read(to).map(|to| (from, to)));
    let (from, to) = match documents {
        Ok(pair) => pair,
        Err(error) => {
            eprintln!("{error}");
            return 2;
        }
    };
    match compare(&from, &to) {
        Ok(added) => {
            for line in &added {
                println!("{line}");
            }
            println!("surface is additions only ({} added)", added.len());
            0
        }
        Err(problems) => {
            for line in &problems {
                eprintln!("{line}");
            }
            eprintln!(
                "surface changed beyond additions: {} problem(s)",
                problems.len()
            );
            1
        }
    }
}
