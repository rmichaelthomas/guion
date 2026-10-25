use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

fn read(root: &Path, relative: &str) -> Result<String, String> {
    fs::read_to_string(root.join(relative)).map_err(|error| format!("read {relative}: {error}"))
}

fn enum_variants(source: &str, enum_name: &str) -> Result<Vec<String>, String> {
    let marker = format!("pub enum {enum_name} {{");
    let start = source
        .find(&marker)
        .ok_or_else(|| format!("could not find {marker}"))?;
    let body_start = start + marker.len();
    let mut depth = 1usize;
    let mut body_end = None;
    for (offset, character) in source[body_start..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    body_end = Some(body_start + offset);
                    break;
                }
            }
            _ => {}
        }
    }
    let body_end = body_end.ok_or_else(|| format!("unterminated {enum_name} enum"))?;
    let mut variants = Vec::new();
    let mut nested_depth = 0usize;
    let mut current = String::new();
    for character in source[body_start..body_end].chars() {
        match character {
            '{' | '(' | '[' => nested_depth += 1,
            '}' | ')' | ']' => nested_depth = nested_depth.saturating_sub(1),
            ',' if nested_depth == 0 => {
                let item = current.trim();
                if let Some(name) = item.split(['(', '{']).next().map(str::trim) {
                    if !name.is_empty() {
                        variants.push(name.to_string());
                    }
                }
                current.clear();
                continue;
            }
            _ => {}
        }
        current.push(character);
    }
    let item = current.trim();
    if let Some(name) = item.split(['(', '{']).next().map(str::trim) {
        if !name.is_empty() {
            variants.push(name.to_string());
        }
    }
    Ok(variants)
}

fn host_signatures(source: &str) -> BTreeMap<String, String> {
    let mut in_operations = false;
    let mut result = BTreeMap::new();
    for line in source.lines() {
        if line.trim() == "## Public operations" {
            in_operations = true;
            continue;
        }
        if in_operations && line.starts_with("## ") {
            break;
        }
        if !in_operations || !line.trim_start().starts_with('|') {
            continue;
        }
        let cells: Vec<_> = line
            .trim()
            .trim_matches('|')
            .split('|')
            .map(str::trim)
            .collect();
        if cells.len() >= 2 && cells[0] != "Operation" && !cells[0].starts_with('-') {
            result.insert(cells[0].to_string(), cells[1].to_string());
        }
    }
    result
}

fn entry_names<'a>(entries: &'a [Value], kind: &'a str) -> impl Iterator<Item = &'a Value> + 'a {
    entries
        .iter()
        .filter(move |entry| entry.get("kind").and_then(Value::as_str) == Some(kind))
}

pub fn validate_inventory(root: &Path) -> Result<Vec<String>, String> {
    let manifest: Value = serde_json::from_str(&read(root, "language/conformance/manifest.json")?)
        .map_err(|error| format!("parse language/conformance/manifest.json: {error}"))?;
    if manifest.get("schema_version").and_then(Value::as_u64) != Some(1) {
        return Err("conformance manifest schema_version must be 1".into());
    }
    let entries = manifest
        .get("entries")
        .and_then(Value::as_array)
        .ok_or("conformance manifest entries must be an array")?;
    let mut missing = Vec::new();
    let mut ids = BTreeSet::new();
    for entry in entries {
        let id = entry.get("id").and_then(Value::as_str).unwrap_or_default();
        if id.is_empty() || !ids.insert(id.to_string()) {
            missing.push(format!("entry has missing or duplicate id: {id}"));
        }
        if entry.get("owner_task").and_then(Value::as_u64).is_none() {
            missing.push(format!("{id}: owner_task is missing or not an integer"));
        }
        for status_field in ["bootstrap", "self_hosted"] {
            let status = entry
                .get("compiler_status")
                .and_then(|value| value.get(status_field))
                .and_then(Value::as_str);
            if status.is_none_or(str::is_empty) {
                missing.push(format!("{id}: compiler_status.{status_field} is missing"));
            }
        }
        for class in ["positive", "negative", "interaction"] {
            let coverage = entry.get("coverage").and_then(|value| value.get(class));
            if coverage
                .and_then(|value| value.get("case_id"))
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
            {
                missing.push(format!("{id}: {class} case_id is missing"));
            }
        }
    }

    let model = read(root, "crates/model/src/lib.rs")?;
    for (enum_name, kind) in [("Type", "type"), ("Op", "opcode")] {
        let actual: BTreeSet<_> = enum_variants(&model, enum_name)?.into_iter().collect();
        let declared: BTreeSet<_> = entry_names(entries, kind)
            .filter_map(|entry| {
                entry
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .collect();
        for item in actual.difference(&declared) {
            missing.push(format!("{kind}:{item}"));
        }
        for item in declared.difference(&actual) {
            missing.push(format!("{kind}:{item} (stale inventory entry)"));
        }
    }

    let actual_host = host_signatures(&read(root, "language/host-api.md")?);
    let declared_host: BTreeMap<_, _> = entry_names(entries, "host_operation")
        .filter_map(|entry| {
            Some((
                entry.get("name")?.as_str()?.to_string(),
                entry.get("source_signature")?.as_str()?.to_string(),
            ))
        })
        .collect();
    for (name, signature) in &actual_host {
        match declared_host.get(name) {
            Some(declared) if declared == signature => {}
            Some(_) => missing.push(format!(
                "host_operation:{name} signature differs from language/host-api.md"
            )),
            None => missing.push(format!("host_operation:{name}")),
        }
    }
    for name in declared_host.keys() {
        if !actual_host.contains_key(name) {
            missing.push(format!("host_operation:{name} (stale inventory entry)"));
        }
    }

    let cli_source = read(root, "crates/cli/src/main.rs")?;
    let usage = cli_source
        .lines()
        .find(|line| line.contains("usage: veritas <"))
        .ok_or("could not find CLI command inventory in usage string")?;
    let usage = usage
        .split("usage: veritas <")
        .nth(1)
        .and_then(|part| part.split('>').next())
        .unwrap_or_default();
    let mut cli_commands: BTreeSet<String> = usage
        .split('|')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect();
    cli_commands.insert("--version".into());
    cli_commands.insert("version".into());
    let declared_cli: BTreeSet<String> = entry_names(entries, "cli_command")
        .filter_map(|entry| {
            entry
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect();
    for name in cli_commands.difference(&declared_cli) {
        missing.push(format!("cli_command:{name}"));
    }

    let apps = manifest
        .get("regression_applications")
        .and_then(Value::as_array)
        .ok_or("regression_apps must be an array")?;
    if apps.len() != 9 {
        missing.push(format!("expected 9 regression apps, found {}", apps.len()));
    }
    for app in apps {
        let id = app.get("id").and_then(Value::as_str).unwrap_or("unknown");
        let brief = app
            .get("brief_path")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if brief.is_empty() || !root.join(brief).is_file() {
            missing.push(format!("regression app {id}: missing brief {brief}"));
        } else if !read(root, brief)?.contains("Follow-up change:") {
            missing.push(format!("regression app {id}: brief lacks Follow-up change"));
        }
        if app
            .get("initial_and_follow_up_registered")
            .and_then(Value::as_bool)
            != Some(true)
        {
            missing.push(format!(
                "regression app {id}: follow-up registration is false"
            ));
        }
    }

    let checks = manifest
        .get("existing_checks")
        .and_then(Value::as_array)
        .ok_or("existing_checks must be an array")?;
    for check in checks {
        let id = check.get("id").and_then(Value::as_str).unwrap_or("unknown");
        if check
            .get("argv")
            .and_then(Value::as_array)
            .is_none_or(|argv| argv.is_empty())
        {
            missing.push(format!("existing check {id}: argv is missing"));
        }
        let source = check
            .get("source")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if source.is_empty() || !root.join(source).is_file() {
            missing.push(format!(
                "existing check {id}: source file {source} is missing"
            ));
        }
    }

    missing.sort();
    missing.dedup();
    Ok(missing)
}
