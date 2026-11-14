//! Checks that keep shipped text honest about the release it ships with.
use std::path::PathBuf;

fn repository() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// `X.Y.Z` of a version, dropping any `-pre.release` or `+build` suffix.
fn core_version(version: &str) -> &str {
    version.split(['-', '+']).next().unwrap_or(version)
}

/// The `[workspace.package]` version of the root `Cargo.toml`.
fn workspace_version(cargo_toml: &str) -> String {
    let manifest: toml::Value = toml::from_str(cargo_toml).expect("Cargo.toml parses");
    manifest["workspace"]["package"]["version"]
        .as_str()
        .expect("[workspace.package] has a version")
        .to_owned()
}

/// The version of the changelog heading marked `(unreleased)`, if there is one.
fn unreleased_version(changelog: &str) -> Option<&str> {
    changelog.lines().find_map(|line| {
        let rest = line.strip_prefix("## ")?;
        let version = rest.strip_suffix("(unreleased)")?.trim();
        Some(version)
    })
}

/// Every `X.Y.Z` token in a page, with any `v` prefix removed. A token is three runs of digits
/// joined by dots; a pre-release suffix is part of it so `0.3.0-rc.1` is one token.
fn version_tokens(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if !bytes[index].is_ascii_digit()
            || (index > 0 && bytes[index - 1].is_ascii_alphanumeric() && bytes[index - 1] != b'v')
        {
            index += 1;
            continue;
        }
        let start = index;
        while index < bytes.len() && (bytes[index].is_ascii_digit() || bytes[index] == b'.') {
            index += 1;
        }
        let candidate = text[start..index].trim_end_matches('.');
        if candidate.split('.').count() == 3 && candidate.split('.').all(|part| !part.is_empty()) {
            let mut end = start + candidate.len();
            if bytes.get(end) == Some(&b'-') {
                let suffix = text[end..]
                    .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '.'))
                    .unwrap_or(text.len() - end);
                end += suffix;
            }
            tokens.push(text[start..end].trim_end_matches('.').to_owned());
        }
    }
    tokens
}

/// The rule: every version the getting-started page names must be the core of the workspace
/// version (so `0.3.0-rc.1` is documented as `0.3.0`), or the version of the changelog's
/// `(unreleased)` section, which is how a page is prepared ahead of the version bump.
fn check_page(page: &str, cargo_toml: &str, changelog: &str) -> Result<(), String> {
    let cargo = workspace_version(cargo_toml);
    let allowed_core = core_version(&cargo).to_owned();
    let unreleased = unreleased_version(changelog).map(core_version);
    let tokens = version_tokens(page);
    if tokens.is_empty() {
        return Err("the page names no version".into());
    }
    for token in &tokens {
        let token_core = core_version(token);
        if token_core != allowed_core && Some(token_core) != unreleased {
            return Err(format!(
                "the page names {token}, but the workspace version is {cargo} (documented as {allowed_core}){}",
                unreleased
                    .map(|version| format!(" and the changelog's unreleased section is {version}"))
                    .unwrap_or_default()
            ));
        }
    }
    Ok(())
}

#[test]
fn shipped_getting_started_names_this_release() {
    let root = repository();
    let read = |relative: &str| {
        std::fs::read_to_string(root.join(relative))
            .unwrap_or_else(|error| panic!("{relative}: {error}"))
    };
    if let Err(error) = check_page(
        &read("docs/getting-started.md"),
        &read("Cargo.toml"),
        &read("CHANGELOG.md"),
    ) {
        panic!("docs/getting-started.md: {error}");
    }
}

#[test]
fn the_release_rule_accepts_the_core_of_a_pre_release_and_the_unreleased_section() {
    let cargo = |version: &str| format!("[workspace.package]\nversion = \"{version}\"\n");
    let page = "Guion 0.3.0 ... releases/tag/v0.3.0 ... v0.3.0 release";
    let changelog = "## 0.3.0 (unreleased)\n\n## 0.2.0 - 2026-11-12\n";
    assert_eq!(check_page(page, &cargo("0.3.0-rc.1"), changelog), Ok(()));
    assert_eq!(
        check_page(page, &cargo("0.3.0"), "## 0.3.0 - 2026-12-01\n"),
        Ok(())
    );
    // The page may run ahead of the bump while the changelog marks the release unreleased.
    assert_eq!(check_page(page, &cargo("0.2.0"), changelog), Ok(()));
}

#[test]
fn the_release_rule_rejects_a_stale_page() {
    let cargo = |version: &str| format!("[workspace.package]\nversion = \"{version}\"\n");
    let stale = "Guion 0.2.0 is downloadable ... v0.2.0";
    assert!(check_page(stale, &cargo("0.3.0-rc.1"), "## 0.3.0 (unreleased)\n").is_err());
    assert!(check_page(stale, &cargo("0.3.0"), "## 0.3.0 - 2026-12-01\n").is_err());
    assert!(check_page("no number here", &cargo("0.3.0"), "").is_err());
    assert!(check_page("Guion 0.4.0", &cargo("0.3.0"), "## 0.3.0 (unreleased)\n").is_err());
}
