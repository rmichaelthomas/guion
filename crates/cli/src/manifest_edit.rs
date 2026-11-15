//! Adding one grant to `guion.toml` without disturbing anything else in it.
//!
//! The file is edited with `toml_edit`, so comments, blank lines, key order and the formatting of
//! every other entry stay as they were. A value is added to the existing table and key; a table
//! is created only when it is missing.

use guion_model::GrantHint;
use std::path::Path;
use toml_edit::{value, Array, DocumentMut, Item, Table};

/// Add `grant` to the manifest at `path`, creating the file's table or key if needed. A grant
/// already present leaves the file unchanged (returns `Ok(false)`).
pub fn add_grant(path: &Path, grant: &GrantHint) -> Result<bool, String> {
    let (Some(table_name), Some(key)) = (grant.table.as_deref(), grant.key.as_deref()) else {
        return Err("this grant has no guion.toml entry".into());
    };
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let mut document = text
        .parse::<DocumentMut>()
        .map_err(|error| format!("{}: invalid TOML: {error}", path.display()))?;
    let changed = apply(&mut document, table_name, key, &grant.value)?;
    if changed {
        write_atomically(path, &document.to_string())?;
    }
    Ok(changed)
}

fn apply(
    document: &mut DocumentMut,
    table_name: &str,
    key: &str,
    grant_value: &str,
) -> Result<bool, String> {
    if !document.contains_key(table_name) {
        document[table_name] = Item::Table(Table::new());
    }
    let table = document[table_name]
        .as_table_like_mut()
        .ok_or_else(|| format!("guion.toml [{table_name}] must be a table"))?;
    if key == "enabled" {
        let already = table
            .get(key)
            .and_then(Item::as_bool)
            .is_some_and(|enabled| enabled);
        if already {
            return Ok(false);
        }
        table.insert(key, value(true));
        return Ok(true);
    }
    if table.get(key).is_none() {
        table.insert(key, value(Array::new()));
    }
    let array = table
        .get_mut(key)
        .and_then(Item::as_array_mut)
        .ok_or_else(|| format!("guion.toml [{table_name}].{key} must be an array"))?;
    if array
        .iter()
        .any(|entry| entry.as_str() == Some(grant_value))
    {
        return Ok(false);
    }
    array.push(grant_value);
    Ok(true)
}

fn write_atomically(path: &Path, contents: &str) -> Result<(), String> {
    let temporary = path.with_extension("toml.guion-tmp");
    std::fs::write(&temporary, contents)
        .map_err(|error| format!("cannot write {}: {error}", temporary.display()))?;
    std::fs::rename(&temporary, path).map_err(|error| {
        let _ = std::fs::remove_file(&temporary);
        format!("cannot replace {}: {error}", path.display())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edited(text: &str, table: &str, key: &str, grant_value: &str) -> (String, bool) {
        let mut document = text.parse::<DocumentMut>().unwrap();
        let changed = apply(&mut document, table, key, grant_value).unwrap();
        (document.to_string(), changed)
    }

    #[test]
    fn adds_to_an_existing_array_and_keeps_comments_and_order() {
        let before = "# my project\nname = \"x\"\n\n[network]\n# hosts I trust\nhosts = [\"a.example\"] # keep\ntimeout_ms = 500\n\n[log]\nlevel = \"info\"\n";
        let (after, changed) = edited(before, "network", "hosts", "b.example");
        assert!(changed);
        assert_eq!(
            after,
            "# my project\nname = \"x\"\n\n[network]\n# hosts I trust\nhosts = [\"a.example\", \"b.example\"] # keep\ntimeout_ms = 500\n\n[log]\nlevel = \"info\"\n"
        );
    }

    #[test]
    fn creates_the_table_only_when_it_is_missing() {
        let before = "name = \"x\"\n";
        let (after, changed) = edited(before, "process", "commands", "git");
        assert!(changed);
        assert!(after.starts_with("name = \"x\"\n"), "{after}");
        assert!(after.contains("[process]\ncommands = [\"git\"]"), "{after}");
    }

    #[test]
    fn a_grant_already_there_changes_nothing() {
        let before = "[network]\nhosts = [\"a.example\"]\n";
        let (after, changed) = edited(before, "network", "hosts", "a.example");
        assert!(!changed);
        assert_eq!(after, before);
    }

    #[test]
    fn the_clock_flips_from_false_to_true_in_place() {
        let (after, changed) = edited(
            "[clock]\nenabled = false # off\n",
            "clock",
            "enabled",
            "true",
        );
        assert!(changed);
        assert!(after.contains("enabled = true"), "{after}");
        let (_, again) = edited(&after, "clock", "enabled", "true");
        assert!(!again);
    }
}
