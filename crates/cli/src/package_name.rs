//! The one place package-name rules live. `guion add`, `guion publish`, `guion new --lib` and the
//! manifest parser all go through here.
//!
//! A name is 1 to 64 characters of lowercase ASCII letters, digits, `-` and `_`. It starts with a
//! letter and does not end with a separator. `-` and `_` are the same character for uniqueness
//! (`http_client` and `http-client` collide). Names that match a standard-library module, `std`,
//! or the `guion-` prefix are reserved. `@owner/name` is reserved syntax: it parses, then is
//! refused with a clear message.

pub const MAX_PACKAGE_NAME_BYTES: usize = 64;

/// File names under `language/stdlib/` (without `.gu`). A test keeps this list in step with the
/// directory, so a new standard-library module cannot be added without reserving its name.
const STDLIB_MODULE_NAMES: &[&str] = &[
    "calendar",
    "cli",
    "collections",
    "crypto",
    "csv",
    "encoding",
    "fs",
    "http",
    "http-server",
    "http-types",
    "json",
    "jsonl",
    "math",
    "net",
    "path",
    "process",
    "random",
    "regex",
    "semver",
    "sqlite",
    "text",
    "time",
    "tls",
    "toml",
    "types",
    "unicode",
    "url",
    "websocket",
];

/// Names reserved in addition to the standard-library modules.
const FIXED_RESERVED_NAMES: &[&str] = &["std", "guion", "pkg", "core"];

const RESERVED_PREFIX: &str = "guion-";

/// The form used to decide whether two names are the same package.
pub fn collision_key(name: &str) -> String {
    name.to_ascii_lowercase().replace('_', "-")
}

pub fn is_reserved(name: &str) -> bool {
    let key = collision_key(name);
    STDLIB_MODULE_NAMES.contains(&key.as_str())
        || FIXED_RESERVED_NAMES.contains(&key.as_str())
        || key.starts_with(RESERVED_PREFIX)
}

/// The rules every name must meet to be created, added or published.
pub fn validate_new_package_name(name: &str) -> Result<(), String> {
    if let Some(scoped) = name.strip_prefix('@') {
        return Err(format!(
            "`@{scoped}` uses the scoped `@owner/name` form, which is reserved and not available yet; \
             choose a plain name such as `{}`",
            scoped.rsplit('/').next().unwrap_or(scoped)
        ));
    }
    if name.is_empty() {
        return Err("package name is empty".into());
    }
    if name.len() > MAX_PACKAGE_NAME_BYTES {
        return Err(format!(
            "package name `{name}` is longer than {MAX_PACKAGE_NAME_BYTES} characters"
        ));
    }
    if name.bytes().any(|byte| byte.is_ascii_uppercase()) {
        return Err(format!(
            "package name `{name}` must be lowercase; use `{}`",
            collision_key(name)
        ));
    }
    if !name.bytes().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
    }) {
        return Err(format!(
            "package name `{name}` may only contain lowercase letters, digits, `-` and `_`"
        ));
    }
    if !name.as_bytes()[0].is_ascii_lowercase() {
        return Err(format!("package name `{name}` must start with a letter"));
    }
    if name.ends_with('-') || name.ends_with('_') {
        return Err(format!(
            "package name `{name}` must not end with `-` or `_`"
        ));
    }
    if is_reserved(name) {
        return Err(format!(
            "package name `{name}` is reserved (standard-library modules, `std` and the `guion-` prefix cannot be used)"
        ));
    }
    Ok(())
}

/// The older, looser rule, kept so existing manifests and locks keep loading.
pub fn validate_existing_package_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(format!("invalid native package name `{name}`"));
    }
    Ok(())
}

/// Reads a name that already exists in a lock or manifest: it must load, but one that breaks the
/// current rules gets a warning, because new publishes would refuse it.
pub fn check_existing_package_name(name: &str) -> Result<(), String> {
    if let Some(scoped) = name.strip_prefix('@') {
        return Err(format!(
            "`@{scoped}` uses the scoped `@owner/name` form, which is reserved and not available yet"
        ));
    }
    validate_existing_package_name(name)?;
    if let Err(problem) = validate_new_package_name(name) {
        eprintln!("warning: existing package name does not meet the current rules: {problem}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_plain_lowercase_names() {
        for name in ["a", "http-client", "my_lib2", "x".repeat(64).as_str()] {
            assert_eq!(validate_new_package_name(name), Ok(()), "{name}");
        }
    }

    #[test]
    fn rejects_malformed_names_with_a_reason() {
        let cases = [
            ("", "empty"),
            ("Http-Client", "lowercase"),
            ("1abc", "start with a letter"),
            ("abc-", "must not end"),
            ("a b", "may only contain"),
            ("dé", "may only contain"),
            ("../x", "may only contain"),
        ];
        for (name, expected) in cases {
            let message = validate_new_package_name(name).expect_err(name);
            assert!(message.contains(expected), "{name}: {message}");
        }
        let long = "x".repeat(65);
        assert!(validate_new_package_name(&long)
            .unwrap_err()
            .contains("longer than 64"));
    }

    #[test]
    fn reserved_names_are_refused_in_either_separator_form() {
        for name in [
            "std",
            "regex",
            "http_server",
            "http-server",
            "guion-tools",
            "guion_tools",
            "guion",
        ] {
            let message = validate_new_package_name(name).expect_err(name);
            assert!(message.contains("reserved"), "{name}: {message}");
        }
    }

    #[test]
    fn separator_variants_collide() {
        assert_eq!(collision_key("Http_Client"), collision_key("http-client"));
    }

    #[test]
    fn scoped_names_parse_and_are_reserved() {
        let message = validate_new_package_name("@acme/widgets").unwrap_err();
        assert!(
            message.contains("reserved") && message.contains("widgets"),
            "{message}"
        );
        assert!(check_existing_package_name("@acme/widgets").is_err());
    }

    #[test]
    fn legacy_names_still_load() {
        assert_eq!(check_existing_package_name("Legacy_Name"), Ok(()));
    }

    #[test]
    fn reserved_list_matches_the_standard_library_directory() {
        let directory =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../language/stdlib");
        let mut on_disk: Vec<String> = std::fs::read_dir(directory)
            .unwrap()
            .filter_map(|entry| {
                let name = entry.ok()?.file_name().into_string().ok()?;
                Some(name.strip_suffix(".gu")?.replace('_', "-"))
            })
            .collect();
        on_disk.sort();
        for name in &on_disk {
            assert!(
                STDLIB_MODULE_NAMES.contains(&name.as_str()),
                "standard-library module `{name}` is not reserved in package_name.rs"
            );
        }
    }
}
