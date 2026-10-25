use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

const CATALOG: &str = "language/builtin-signatures.json";
const GENERATED: &str = "language/self_host/builtin_catalog.gu";

fn catalog_entries(root: &Path) -> Result<Vec<Value>, String> {
    let path = root.join(CATALOG);
    let contents =
        fs::read_to_string(&path).map_err(|error| format!("read {}: {error}", path.display()))?;
    let value: Value = serde_json::from_str(&contents)
        .map_err(|error| format!("parse {}: {error}", path.display()))?;
    if value.get("schema_version").and_then(Value::as_u64) != Some(1) {
        return Err(format!(
            "{} has an unsupported schema_version",
            path.display()
        ));
    }
    let entries = value
        .get("entries")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{} has no entries array", path.display()))?;
    let mut names = BTreeSet::new();
    for entry in entries {
        let name = entry
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| "builtin entry has no name".to_string())?;
        let parameters = entry
            .get("parameters")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("builtin `{name}` has no parameters array"))?;
        if parameters.iter().any(|item| item.as_str().is_none()) {
            return Err(format!(
                "builtin `{name}` has a non-text parameter signature"
            ));
        }
        for field in ["result", "operation"] {
            if entry.get(field).and_then(Value::as_str).is_none() {
                return Err(format!("builtin `{name}` has no text `{field}`"));
            }
        }
        if !names.insert(name.to_string()) {
            return Err(format!("duplicate builtin signature `{name}`"));
        }
    }
    Ok(entries.clone())
}

pub fn render_guion(root: &Path) -> Result<String, String> {
    let entries = catalog_entries(root)?;
    let mut output = String::from("# Generated from language/builtin-signatures.json by `cargo xtask catalog --write`.\npub fn builtin_catalog_signature(name: Text) -> Text:\n    if len(name) == 0:\n        return \"\"\n");
    let mut initial = String::new();
    for entry in &entries {
        let name = entry["name"].as_str().unwrap();
        let first = name.chars().next().unwrap().to_string();
        if first != initial {
            initial = first.clone();
            output.push_str(&format!("    if name[0] == {first:?}:\n"));
        }
        let arity = entry["parameters"].as_array().unwrap().len();
        let parameters = entry["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| {
                let parameter = value.as_str().unwrap();
                if parameter.contains('$')
                    || parameter.contains('|')
                    || parameter == "Any"
                    || parameter == "Sized"
                    || parameter.starts_with("NamedWorker[")
                {
                    ""
                } else {
                    parameter
                }
            })
            .collect::<Vec<_>>()
            .join(";");
        let signature = format!(
            "{arity}:{}:{}:{parameters}",
            entry["operation"].as_str().unwrap(),
            entry["result"].as_str().unwrap()
        );
        output.push_str(&format!(
            "        if name == {name:?}:\n            return {signature:?}\n"
        ));
    }
    output.push_str("    return \"\"\n\nfn builtin_catalog_part(value: Text, delimiter: Text, wanted: Int) -> Text:\n    var cursor = 0\n    var part = 0\n    var result = \"\"\n    while cursor <= len(value):\n        if cursor == len(value) or value[cursor] == delimiter:\n            if part == wanted:\n                return result\n            set part = part + 1\n            set result = \"\"\n        else:\n            if part == wanted:\n                set result = result + value[cursor]\n        set cursor = cursor + 1\n    return \"\"\n\npub fn builtin_catalog_has(name: Text) -> Bool:\n    return builtin_catalog_signature(name) != \"\"\n\npub fn builtin_catalog_arity(name: Text) -> Int:\n    let value = builtin_catalog_part(builtin_catalog_signature(name), \":\", 0)\n    if value == \"0\":\n        return 0\n    if value == \"1\":\n        return 1\n    if value == \"2\":\n        return 2\n    if value == \"3\":\n        return 3\n    if value == \"4\":\n        return 4\n    if value == \"5\":\n        return 5\n    return -1\n\npub fn builtin_catalog_operation(name: Text) -> Text:\n    return builtin_catalog_part(builtin_catalog_signature(name), \":\", 1)\n\npub fn builtin_catalog_result(name: Text) -> Text:\n    return builtin_catalog_part(builtin_catalog_signature(name), \":\", 2)\n\npub fn builtin_catalog_parameter_type(name: Text, index: Int) -> Text:\n    let parameters = builtin_catalog_part(builtin_catalog_signature(name), \":\", 3)\n    return builtin_catalog_part(parameters, \";\", index)\n");
    Ok(output)
}

pub fn validate(root: &Path) -> Result<(), String> {
    let expected = render_guion(root)?;
    let generated = root.join(GENERATED);
    let actual = fs::read_to_string(&generated).map_err(|error| {
        format!(
            "read {}: {error}; run `cargo xtask catalog --write`",
            generated.display()
        )
    })?;
    if actual != expected {
        return Err(format!(
            "{} is stale; regenerate it with `cargo xtask catalog --write`",
            generated.display()
        ));
    }
    Ok(())
}

pub fn write(root: &Path) -> Result<(), String> {
    let generated = root.join(GENERATED);
    fs::write(&generated, render_guion(root)?)
        .map_err(|error| format!("write {}: {error}", generated.display()))
}

#[cfg(test)]
mod tests {
    use super::{render_guion, validate};
    use std::fs;
    use std::path::PathBuf;

    fn fixture() -> PathBuf {
        let thread = std::thread::current()
            .name()
            .unwrap_or("unknown")
            .replace(':', "_");
        let root = std::env::temp_dir().join(format!(
            "guion-builtin-catalog-{}-{thread}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("language/self_host")).unwrap();
        fs::write(root.join("language/builtin-signatures.json"), r#"{"schema_version":1,"entries":[{"name":"test","parameters":["Text"],"result":"Int","operation":"Test"}]}"#).unwrap();
        root
    }

    #[test]
    fn renders_stable_guion_signatures_from_catalog() {
        let root = fixture();
        let rendered = render_guion(&root).unwrap();
        assert!(rendered.contains("builtin_catalog_arity(name: Text)"));
        assert!(rendered.contains("return \"1:Test:Int:Text\""));
        assert!(rendered.contains("builtin_catalog_parameter_type(name: Text, index: Int)"));
        fs::write(
            root.join("language/self_host/builtin_catalog.gu"),
            &rendered,
        )
        .unwrap();
        assert_eq!(validate(&root), Ok(()));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_duplicate_catalog_names() {
        let root = fixture();
        let path = root.join("language/builtin-signatures.json");
        fs::write(path, r#"{"schema_version":1,"entries":[{"name":"test","parameters":[],"result":"Int","operation":"Test"},{"name":"test","parameters":[],"result":"Int","operation":"Test"}]}"#).unwrap();
        assert!(render_guion(&root)
            .unwrap_err()
            .contains("duplicate builtin signature"));
        fs::remove_dir_all(root).unwrap();
    }
}
