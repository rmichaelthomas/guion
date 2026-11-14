//! `guion lsp`: a language server over standard input and output.
//!
//! It speaks the Language Server Protocol with full-document sync and offers diagnostics from
//! the compiler, hover, go to definition, completion and formatting. It reads and writes only
//! the protocol: it never runs the program being edited.

use serde_json::{json, Value};
use std::{
    collections::HashMap,
    io::{BufRead, Write},
    path::{Path, PathBuf},
};

const KEYWORDS: &[&str] = &[
    "fn", "let", "var", "set", "if", "else", "while", "for", "in", "return", "break", "continue",
    "match", "record", "enum", "pub", "priv", "import", "true", "false", "not", "and", "or",
    "range",
];

struct Builtin {
    name: String,
    parameters: Vec<String>,
    result: String,
}

fn builtins() -> &'static Vec<Builtin> {
    static CATALOG: std::sync::OnceLock<Vec<Builtin>> = std::sync::OnceLock::new();
    CATALOG.get_or_init(|| {
        let catalog: Value =
            serde_json::from_str(include_str!("../../../language/builtin-signatures.json"))
                .expect("checked-in builtin catalog is valid JSON");
        let mut found: Vec<Builtin> = catalog["entries"]
            .as_array()
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(|entry| {
                        Some(Builtin {
                            name: entry["name"].as_str()?.to_owned(),
                            parameters: entry["parameters"]
                                .as_array()?
                                .iter()
                                .filter_map(|value| value.as_str().map(str::to_owned))
                                .collect(),
                            result: entry["result"].as_str()?.to_owned(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        found.sort_by(|left, right| left.name.cmp(&right.name));
        found
    })
}

/// A name the buffer declares: where, what it is, and the line that declares it.
struct Symbol {
    name: String,
    kind: &'static str,
    line: usize,
    signature: String,
    doc: String,
}

fn is_name_char(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

fn declared_symbols(text: &str) -> Vec<Symbol> {
    let lines: Vec<&str> = text.lines().collect();
    let mut symbols = Vec::new();
    for (index, raw) in lines.iter().enumerate() {
        let trimmed = raw.trim_start();
        let indent = raw.len() - trimmed.len();
        let declaration = trimmed.strip_prefix("pub ").unwrap_or(trimmed);
        let doc = if index > 0 && lines[index - 1].trim_start().starts_with('#') {
            lines[index - 1]
                .trim_start()
                .trim_start_matches('#')
                .trim()
                .to_owned()
        } else {
            String::new()
        };
        let take_name = |rest: &str| -> String {
            rest.trim_start()
                .chars()
                .take_while(|c| is_name_char(*c))
                .collect()
        };
        if indent == 0 {
            if let Some(rest) = declaration.strip_prefix("fn ") {
                let name = take_name(rest);
                if !name.is_empty() {
                    symbols.push(Symbol {
                        name,
                        kind: "function",
                        line: index,
                        signature: declaration.trim_end_matches(':').trim().to_owned(),
                        doc,
                    });
                }
                continue;
            }
            for (keyword, kind) in [("record ", "record"), ("enum ", "enum")] {
                if let Some(rest) = declaration.strip_prefix(keyword) {
                    let name = take_name(rest);
                    if name.is_empty() {
                        continue;
                    }
                    let mut signature = format!("{keyword}{name}");
                    for member in lines.iter().skip(index + 1) {
                        if !member.starts_with(' ') && !member.trim().is_empty() {
                            break;
                        }
                        if member.trim().is_empty() {
                            continue;
                        }
                        signature.push_str("\n    ");
                        signature.push_str(member.trim());
                        if kind == "enum" {
                            let variant = take_name(member.trim());
                            if !variant.is_empty() {
                                symbols.push(Symbol {
                                    name: variant,
                                    kind: "variant",
                                    line: index + signature.lines().count() - 1,
                                    signature: format!(
                                        "{} variant of {name}: {}",
                                        "enum",
                                        member.trim()
                                    ),
                                    doc: String::new(),
                                });
                            }
                        }
                    }
                    symbols.push(Symbol {
                        name,
                        kind,
                        line: index,
                        signature,
                        doc,
                    });
                    break;
                }
            }
        } else if let Some(rest) = trimmed
            .strip_prefix("let ")
            .or_else(|| trimmed.strip_prefix("var "))
        {
            let name = take_name(rest);
            if !name.is_empty() {
                symbols.push(Symbol {
                    name,
                    kind: "variable",
                    line: index,
                    signature: trimmed.to_owned(),
                    doc: String::new(),
                });
            }
        }
    }
    symbols
}

fn word_at(text: &str, line: usize, character: usize) -> Option<String> {
    let line_text = text.lines().nth(line)?;
    let chars: Vec<char> = line_text.chars().collect();
    let mut at = character.min(chars.len());
    if at == chars.len() || !is_name_char(chars[at]) {
        if at > 0 && is_name_char(chars[at - 1]) {
            at -= 1;
        } else {
            return None;
        }
    }
    let mut start = at;
    while start > 0 && is_name_char(chars[start - 1]) {
        start -= 1;
    }
    let mut end = at;
    while end < chars.len() && is_name_char(chars[end]) {
        end += 1;
    }
    Some(chars[start..end].iter().collect())
}

fn builtin_signature(builtin: &Builtin) -> String {
    format!(
        "{}({}) -> {}",
        builtin.name,
        builtin.parameters.join(", "),
        builtin.result
    )
}

fn hover_text(text: &str, word: &str) -> Option<String> {
    if let Some(symbol) = declared_symbols(text)
        .into_iter()
        .find(|symbol| symbol.name == word)
    {
        let mut body = format!("```guion\n{}\n```", symbol.signature);
        if !symbol.doc.is_empty() {
            body.push_str("\n\n");
            body.push_str(&symbol.doc);
        }
        return Some(body);
    }
    if let Some(builtin) = builtins().iter().find(|builtin| builtin.name == word) {
        return Some(format!(
            "```guion\n{}\n```\n\nbuilt in",
            builtin_signature(builtin)
        ));
    }
    if KEYWORDS.contains(&word) {
        return Some(format!("`{word}` is a Guion keyword"));
    }
    None
}

fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let bytes = rest.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(value) = u8::from_str_radix(&rest[index + 1..index + 3], 16) {
                decoded.push(value);
                index += 3;
                continue;
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8(decoded).ok().map(PathBuf::from)
}

/// The compiler's complaint about this buffer, if any, as an LSP diagnostic.
fn diagnostics_for(uri: &str, text: &str) -> Vec<Value> {
    let result = if text.contains("import ") || text.contains("import\t") {
        // Relative and `std:` imports need a file on disk: compile a copy beside the original.
        let directory = uri_to_path(uri)
            .and_then(|path| path.parent().map(Path::to_path_buf))
            .unwrap_or_else(std::env::temp_dir);
        let scratch = directory.join(format!(".guion-lsp-{}.gu", std::process::id()));
        if std::fs::write(&scratch, text).is_err() {
            return Vec::new();
        }
        let compiled = crate::compiler_driver::compile_project_with_roots(
            &scratch,
            &directory,
            crate::compiler_driver::standard_library_root().as_deref(),
            &HashMap::new(),
        );
        let _ = std::fs::remove_file(&scratch);
        compiled.map(|_| ())
    } else {
        guion_compiler::compile("buffer.gu", text).map(|_| ())
    };
    match result {
        Ok(()) => Vec::new(),
        Err(error) => {
            if error.source_name.as_deref().is_some_and(|name| {
                !name.is_empty() && name != "buffer.gu" && !name.contains(".guion-lsp-")
            }) {
                // The problem is in another file; report it at the top of this one.
                return vec![json!({
                    "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}},
                    "severity": 1,
                    "code": error.code,
                    "source": "guion",
                    "message": format!("{} (in {})", error.message, error.source_name.as_deref().unwrap_or("an imported file")),
                })];
            }
            let line = error.span.line.saturating_sub(1);
            let column = error.span.column.saturating_sub(1);
            let width = text
                .lines()
                .nth(line)
                .map(|line_text| {
                    line_text
                        .chars()
                        .skip(column)
                        .take_while(|c| is_name_char(*c))
                        .count()
                        .max(1)
                })
                .unwrap_or(1);
            let mut message = error.message.clone();
            if !error.suggestions().is_empty() {
                message.push_str(&format!(
                    " (did you mean {}?)",
                    error.suggestions().join(", ")
                ));
            }
            vec![json!({
                "range": {
                    "start": {"line": line, "character": column},
                    "end": {"line": line, "character": column + width},
                },
                "severity": 1,
                "code": error.code,
                "source": "guion",
                "message": message,
            })]
        }
    }
}

fn completion_items(text: &str) -> Vec<Value> {
    let mut items = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for symbol in declared_symbols(text) {
        if seen.insert(symbol.name.clone()) {
            let kind = match symbol.kind {
                "function" => 3,
                "record" | "enum" => 22,
                "variant" => 20,
                _ => 6,
            };
            items.push(json!({"label": symbol.name, "kind": kind, "detail": symbol.signature.lines().next().unwrap_or("")}));
        }
    }
    for builtin in builtins() {
        if seen.insert(builtin.name.clone()) {
            items.push(
                json!({"label": builtin.name, "kind": 3, "detail": builtin_signature(builtin)}),
            );
        }
    }
    for keyword in KEYWORDS {
        if seen.insert((*keyword).to_owned()) {
            items.push(json!({"label": keyword, "kind": 14}));
        }
    }
    items
}

fn read_message(input: &mut impl BufRead) -> Option<Value> {
    let mut length = None;
    loop {
        let mut header = String::new();
        if input.read_line(&mut header).ok()? == 0 {
            return None;
        }
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some(value) = header.to_ascii_lowercase().strip_prefix("content-length:") {
            length = value.trim().parse::<usize>().ok();
        }
    }
    let length = length?;
    let mut body = vec![0u8; length];
    input.read_exact(&mut body).ok()?;
    serde_json::from_slice(&body).ok()
}

fn send(output: &mut dyn Write, message: &Value) {
    let body = message.to_string();
    let _ = write!(output, "Content-Length: {}\r\n\r\n{}", body.len(), body);
    let _ = output.flush();
}

pub fn serve(input: impl BufRead, mut output: impl Write) {
    let mut input = input;
    let mut documents: HashMap<String, String> = HashMap::new();
    let mut shutdown = false;
    while let Some(message) = read_message(&mut input) {
        let method = message["method"].as_str().unwrap_or("");
        let id = message.get("id").cloned();
        let params = &message["params"];
        let uri = params["textDocument"]["uri"]
            .as_str()
            .unwrap_or("")
            .to_owned();
        let reply = |output: &mut dyn Write, result: Value| {
            if let Some(id) = &id {
                send(
                    output,
                    &json!({"jsonrpc": "2.0", "id": id, "result": result}),
                );
            }
        };
        match method {
            "initialize" => reply(
                &mut output,
                json!({
                    "capabilities": {
                        "textDocumentSync": 1,
                        "hoverProvider": true,
                        "definitionProvider": true,
                        "documentFormattingProvider": true,
                        "completionProvider": {"triggerCharacters": []},
                    },
                    "serverInfo": {"name": "guion", "version": env!("CARGO_PKG_VERSION")},
                }),
            ),
            "shutdown" => {
                shutdown = true;
                reply(&mut output, Value::Null);
            }
            "exit" => {
                let _ = shutdown;
                return;
            }
            "textDocument/didOpen" | "textDocument/didChange" => {
                let text = if method == "textDocument/didOpen" {
                    params["textDocument"]["text"].as_str().map(str::to_owned)
                } else {
                    params["contentChanges"]
                        .as_array()
                        .and_then(|changes| changes.last())
                        .and_then(|change| change["text"].as_str())
                        .map(str::to_owned)
                };
                if let Some(text) = text {
                    let found = diagnostics_for(&uri, &text);
                    documents.insert(uri.clone(), text);
                    send(
                        &mut output,
                        &json!({
                            "jsonrpc": "2.0",
                            "method": "textDocument/publishDiagnostics",
                            "params": {"uri": uri, "diagnostics": found},
                        }),
                    );
                }
            }
            "textDocument/didClose" => {
                documents.remove(&uri);
                send(
                    &mut output,
                    &json!({
                        "jsonrpc": "2.0",
                        "method": "textDocument/publishDiagnostics",
                        "params": {"uri": uri, "diagnostics": []},
                    }),
                );
            }
            "textDocument/hover" => {
                let line = params["position"]["line"].as_u64().unwrap_or(0) as usize;
                let character = params["position"]["character"].as_u64().unwrap_or(0) as usize;
                let result = documents
                    .get(&uri)
                    .and_then(|text| {
                        let word = word_at(text, line, character)?;
                        hover_text(text, &word)
                    })
                    .map(|body| json!({"contents": {"kind": "markdown", "value": body}}))
                    .unwrap_or(Value::Null);
                reply(&mut output, result);
            }
            "textDocument/definition" => {
                let line = params["position"]["line"].as_u64().unwrap_or(0) as usize;
                let character = params["position"]["character"].as_u64().unwrap_or(0) as usize;
                let result = documents
                    .get(&uri)
                    .and_then(|text| {
                        let word = word_at(text, line, character)?;
                        let symbol = declared_symbols(text)
                            .into_iter()
                            .find(|symbol| symbol.name == word && symbol.kind != "variable")?;
                        let declaration = text.lines().nth(symbol.line)?;
                        let column = declaration.find(&word).unwrap_or(0);
                        Some(json!({
                            "uri": uri,
                            "range": {
                                "start": {"line": symbol.line, "character": column},
                                "end": {"line": symbol.line, "character": column + word.chars().count()},
                            },
                        }))
                    })
                    .unwrap_or(Value::Null);
                reply(&mut output, result);
            }
            "textDocument/completion" => {
                let items = documents
                    .get(&uri)
                    .map(|text| completion_items(text))
                    .unwrap_or_default();
                reply(&mut output, json!({"isIncomplete": false, "items": items}));
            }
            "textDocument/formatting" => {
                let result = documents
                    .get(&uri)
                    .map(|text| {
                        let formatted = crate::format_source(text);
                        if formatted == *text {
                            json!([])
                        } else {
                            let last_line = text.lines().count();
                            json!([{
                                "range": {
                                    "start": {"line": 0, "character": 0},
                                    "end": {"line": last_line + 1, "character": 0},
                                },
                                "newText": formatted,
                            }])
                        }
                    })
                    .unwrap_or(Value::Null);
                reply(&mut output, result);
            }
            _ => {
                // An unknown request gets "method not found"; an unknown notification is ignored.
                if let Some(id) = &id {
                    send(
                        &mut output,
                        &json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": format!("method not found: {method}")}}),
                    );
                }
            }
        }
    }
}

pub fn run_stdio() {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    serve(stdin.lock(), stdout.lock());
}
