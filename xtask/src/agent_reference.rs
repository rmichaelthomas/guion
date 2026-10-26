//! Generate the compact, single-file programming reference for agents.
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

const OUTPUT: &str = "docs/agents/guion-reference.md";
const RECIPES: &[(&str, &str, &str)] = &[
    (
        "JSON file CLI",
        "cargo run --locked -p guion-cli -- run docs/agents/examples/file_data_cli.gu records.json",
        "docs/agents/examples/file_data_cli.gu",
    ),
    (
        "Granted HTTP request",
        "cargo run --locked -p guion-cli -- run --allow-http-host api.example.org docs/agents/examples/http_request.gu https://api.example.org/health",
        "docs/agents/examples/http_request.gu",
    ),
    (
        "TOML and SQLite",
        "cargo run --locked -p guion-cli -- run --file-root /tmp/guion-agent-data docs/agents/examples/sqlite_toml.gu",
        "docs/agents/examples/sqlite_toml.gu",
    ),
    (
        "Bounded concurrent work",
        "cargo run --locked -p guion-cli -- run docs/agents/examples/concurrent_task.gu",
        "docs/agents/examples/concurrent_task.gu",
    ),
    (
        "Failure paths",
        "See the four invocations listed with the example below.",
        "docs/agents/examples/failure_paths.gu",
    ),
];

fn read(root: &Path, path: &str) -> Result<String, String> {
    fs::read_to_string(root.join(path)).map_err(|error| format!("read {path}: {error}"))
}

fn markdown_section(document: &str, title: &str) -> Result<String, String> {
    let heading = format!("## {title}");
    let start = document
        .find(&heading)
        .ok_or_else(|| format!("missing markdown section {heading}"))?;
    let rest = &document[start..];
    let end = rest[heading.len()..]
        .find("\n## ")
        .map(|offset| heading.len() + offset + 1)
        .unwrap_or(rest.len());
    Ok(rest[..end].trim().to_string())
}

fn pub_signatures(source: &str) -> Vec<String> {
    source
        .lines()
        .filter_map(|line| {
            let declaration = line.trim();
            declaration
                .starts_with("pub fn ")
                .then(|| declaration.trim_end_matches(':').to_string())
        })
        .collect()
}

fn walk_files(directory: &Path, output: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(directory)
        .map_err(|error| format!("read directory {}: {error}", directory.display()))?
    {
        let path = entry
            .map_err(|error| format!("read directory entry: {error}"))?
            .path();
        if path.is_dir() {
            walk_files(&path, output)?;
        } else if path
            .extension()
            .is_some_and(|extension| extension == "rs" || extension == "gu")
        {
            output.push(path);
        }
    }
    Ok(())
}

fn error_codes(root: &Path) -> Result<Vec<String>, String> {
    let mut files = Vec::new();
    for directory in [root.join("crates"), root.join("language")] {
        walk_files(&directory, &mut files)?;
    }
    let mut codes = std::collections::BTreeSet::new();
    for path in files {
        let text = fs::read_to_string(&path)
            .map_err(|error| format!("read {}: {error}", path.display()))?;
        let bytes = text.as_bytes();
        let mut index = 0;
        while index + 2 < bytes.len() {
            if (bytes[index] == b'R' || bytes[index] == b'E') && bytes[index + 1] == b'_' {
                let mut end = index + 2;
                while end < bytes.len()
                    && (bytes[end].is_ascii_uppercase()
                        || bytes[end].is_ascii_digit()
                        || bytes[end] == b'_')
                {
                    end += 1;
                }
                if end > index + 2 {
                    codes.insert(String::from_utf8_lossy(&bytes[index..end]).into_owned());
                }
                index = end;
            } else {
                index += 1;
            }
        }
    }
    Ok(codes.into_iter().collect())
}

pub fn render(root: &Path) -> Result<String, String> {
    let spec = read(root, "language/spec.md")?;
    let host_api = read(root, "language/host-api.md")?;
    let builtin_text = read(root, "language/builtin-signatures.json")?;
    let builtin_json: Value = serde_json::from_str(&builtin_text)
        .map_err(|error| format!("parse language/builtin-signatures.json: {error}"))?;
    let entries = builtin_json["entries"]
        .as_array()
        .ok_or("builtin signature catalog has no entries array")?;
    let mut builtins = Vec::new();
    for entry in entries {
        let name = entry["name"].as_str().ok_or("builtin without name")?;
        let parameters = entry["parameters"]
            .as_array()
            .ok_or("builtin without parameters")?
            .iter()
            .map(|value| value.as_str().ok_or("non-text builtin parameter"))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
        let result = entry["result"].as_str().ok_or("builtin without result")?;
        builtins.push(format!("{name}({parameters}) -> {result}"));
    }
    builtins.sort();

    let mut standard_library = Vec::new();
    let stdlib_dir = root.join("language/stdlib");
    let mut files: Vec<_> = fs::read_dir(&stdlib_dir)
        .map_err(|error| format!("read {}: {error}", stdlib_dir.display()))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<_, _>>()
        .map_err(|error| format!("read standard library entry: {error}"))?;
    files.sort();
    for path in files {
        if path.extension().is_some_and(|extension| extension == "gu") {
            let module = path
                .file_stem()
                .and_then(|name| name.to_str())
                .ok_or("non-UTF-8 standard library filename")?;
            let source = fs::read_to_string(&path)
                .map_err(|error| format!("read {}: {error}", path.display()))?;
            for signature in pub_signatures(&source) {
                standard_library.push(format!("std:{module} — {signature}"));
            }
        }
    }

    let mut output = String::from(
        "# Guion reference for agents\n\nGenerated from the current Guion language contract, builtin signature catalog, standard library, host API, diagnostics, and runnable examples. Use this file as a compact entry point; the linked contracts remain authoritative.\n\n",
    );
    output.push_str("## Syntax summary\n\n");
    for title in [
        "Source and declarations",
        "Statements and expressions",
        "Runtime rules",
    ] {
        output.push_str(&markdown_section(&spec, title)?);
        output.push_str("\n\n");
    }

    output.push_str("## Builtin signatures\n\n```text\n");
    for signature in builtins {
        output.push_str(&signature);
        output.push('\n');
    }
    output.push_str("```\n\n## Standard-library public signatures\n\n```text\n");
    for signature in standard_library {
        output.push_str(&signature);
        output.push('\n');
    }
    output.push_str("```\n\n## Effects and grants\n\n");
    output.push_str("Guest source cannot expand the host grants. File access is limited to granted roots and relative paths. Network access is denied by default and requires exact host grants. Environment variables and clock reads also require explicit grants. Resource limits are enforced by the host policy. This is a capability policy inside the runtime, not an operating-system sandbox.\n\n");
    for title in [
        "Capabilities and policy",
        "File behavior",
        "HTTP behavior",
        "Structured concurrency behavior",
        "Durable storage and interoperability services",
    ] {
        output.push_str(&markdown_section(&host_api, title)?);
        output.push_str("\n\n");
    }

    output.push_str("## Stable compiler and runtime error codes\n\n");
    output.push_str("Codes below are collected from compiler/runtime source and self-hosted language files. Host error codes are also documented beside their operations in the host API contract.\n\n```text\n");
    for code in error_codes(root)? {
        output.push_str(&code);
        output.push('\n');
    }
    output.push_str("```\n\n## Runnable recipes\n\n");
    for (title, command, path) in RECIPES {
        output.push_str(&format!("### {title}\n\nCommand: `{command}`\n\n"));
        if *title == "Failure paths" {
            output.push_str("Run each mode independently:\n\n```sh\n");
            for mode in [
                "missing-file",
                "denied-http",
                "sqlite-escape",
                "worker-failure",
            ] {
                output.push_str(&format!(
                    "cargo run --locked -p guion-cli -- run {path} {mode}\n"
                ));
            }
            output.push_str("```\n\n");
        }
        output.push_str("```guion\n");
        output.push_str(&read(root, path)?);
        if !output.ends_with('\n') {
            output.push('\n');
        }
        output.push_str("```\n\n");
    }
    output.push_str("## Authoritative sources\n\n- [Language contract](../../language/spec.md)\n- [Builtin signature catalog](../../language/builtin-signatures.json)\n- [Standard library](../../language/stdlib/)\n- [Host API and effects](../../language/host-api.md)\n- [Agent recipes](recipes.md)\n\n");
    let digest = Sha256::digest(output.as_bytes());
    output.push_str(&format!("<!-- Generated reference content digest: sha256:{digest:x}. Validated by `cargo xtask agent-reference --check`. -->\n"));
    let bytes = output.len();
    if bytes > 120 * 1024 {
        return Err(format!(
            "generated agent reference is {bytes} bytes; limit is 122880"
        ));
    }
    Ok(output)
}

pub fn check(root: &Path) -> Result<(), String> {
    let expected = render(root)?;
    let actual = read(root, OUTPUT)?;
    if actual != expected {
        return Err(format!(
            "{OUTPUT} is out of date; run `cargo xtask agent-reference --write`"
        ));
    }
    Ok(())
}

pub fn write(root: &Path) -> Result<(), String> {
    fs::write(root.join(OUTPUT), render(root)?).map_err(|error| format!("write {OUTPUT}: {error}"))
}
