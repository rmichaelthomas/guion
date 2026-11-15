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

/// Public text that states the current release, its platforms or the library size. The site's
/// story section is left out on purpose: it is a dated history of earlier releases.
const PUBLIC_CLAIMS_OF_NOW: &[&str] = &[
    "README.md",
    "docs/getting-started.md",
    "site/src/sections/01-hero.html",
    "site/src/sections/01a-start.html",
    "site/src/sections/09-agents.html",
    "site/src/sections/11-footer.html",
    "site/src/instruments.json",
    "site/agents/site-brief.md",
    "site/tools/agents-surface.mjs",
];

fn read(relative: &str) -> String {
    std::fs::read_to_string(repository().join(relative))
        .unwrap_or_else(|error| panic!("read {relative}: {error}"))
}

fn is_version(token: &str) -> bool {
    let parts: Vec<&str> = token.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

#[test]
fn public_text_states_the_current_release_platforms_and_library_size() {
    let current = workspace_version(&read("Cargo.toml"));
    let current = core_version(&current).to_owned();
    let modules = std::fs::read_dir(repository().join("language/stdlib"))
        .expect("language/stdlib exists")
        .filter(|entry| {
            entry
                .as_ref()
                .is_ok_and(|entry| entry.path().extension().is_some_and(|ext| ext == "gu"))
        })
        .count();
    let mut problems = Vec::new();
    for file in PUBLIC_CLAIMS_OF_NOW {
        let text = read(file);
        // Release links point at the current release.
        for (index, _) in text.match_indices("releases/tag/v") {
            let version: String = text[index + "releases/tag/v".len()..]
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            if version != current {
                problems.push(format!(
                    "{file}: release link names v{version}, not v{current}"
                ));
            }
        }
        // "<something> X.Y.Z is out for ...": the version is current and every platform is named.
        for (index, _) in text.match_indices(" is out") {
            let before = text[..index].split_whitespace().last().unwrap_or("");
            let before = before.trim_matches(|c: char| !c.is_ascii_digit() && c != '.');
            if is_version(before) && before != current {
                problems.push(format!(
                    "{file}: says {before} is out, current is {current}"
                ));
            }
            if before == current {
                let sentence = text[index..].split(". ").next().unwrap_or("");
                for platform in ["Linux", "Intel", "Windows"] {
                    if !sentence.contains(platform) {
                        problems.push(format!(
                            "{file}: \"{current} is out\" sentence does not name {platform}: {}",
                            sentence.chars().take(120).collect::<String>()
                        ));
                    }
                }
            }
        }
        // "N standard-library modules" matches the shipped library.
        for (index, _) in text.match_indices(" standard-library modules") {
            let count = text[..index].split_whitespace().last().unwrap_or("");
            if !count.is_empty()
                && count.bytes().all(|b| b.is_ascii_digit())
                && count != modules.to_string()
            {
                problems.push(format!(
                    "{file}: says {count} standard-library modules, language/stdlib has {modules}"
                ));
            }
        }
    }
    let cli = read("site/src/content/cli.json");
    if !cli.contains(&format!("\"version\": \"guion {current} ")) {
        problems.push(format!(
            "site/src/content/cli.json: version is not guion {current}"
        ));
    }
    assert!(
        problems.is_empty(),
        "public text disagrees with the repository:\n{}",
        problems.join("\n")
    );
}
