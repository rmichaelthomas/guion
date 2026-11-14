//! Capability declarations for libraries.
//!
//! A library lists what it may touch in a `[capabilities]` table of its manifest:
//!
//! ```toml
//! [capabilities]
//! network = ["api.example.com"]   # hosts it contacts
//! files = ["read"]                # "read" and/or "write"
//! environment = ["HOME"]          # environment variables it reads
//! processes = ["git"]             # commands it runs
//! clock = true                    # reads the system clock
//! python = true                   # calls into Python
//! ```
//!
//! `guion check` compares the declaration with what static inspection finds in the library's own
//! modules: using something undeclared is an error, declaring something unused is a warning. The
//! lock records the declaration, and an update that widens it stops until it is approved. A project
//! may also keep an `[approved-capabilities.NAME]` table per package (the same shape); once the
//! table exists, installing or updating a package that asks for more than its entry is refused.

use std::collections::BTreeSet;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Capabilities {
    pub network: BTreeSet<String>,
    pub files: BTreeSet<String>,
    pub environment: BTreeSet<String>,
    pub processes: BTreeSet<String>,
    pub clock: bool,
    pub python: bool,
}

const KEYS: [&str; 6] = [
    "network",
    "files",
    "environment",
    "processes",
    "clock",
    "python",
];

impl Capabilities {
    pub fn is_empty(&self) -> bool {
        *self == Capabilities::default()
    }

    pub fn from_toml(value: Option<&toml::Value>) -> Result<Capabilities, String> {
        let Some(value) = value else {
            return Ok(Capabilities::default());
        };
        let table = value.as_table().ok_or("[capabilities] must be a table")?;
        if let Some(unknown) = table.keys().find(|key| !KEYS.contains(&key.as_str())) {
            return Err(format!(
                "[capabilities] has an unknown entry `{unknown}`; known entries are {}",
                KEYS.join(", ")
            ));
        }
        let list = |key: &str| -> Result<BTreeSet<String>, String> {
            let Some(entry) = table.get(key) else {
                return Ok(BTreeSet::new());
            };
            let items = entry
                .as_array()
                .ok_or_else(|| format!("[capabilities].{key} must be a list of text"))?;
            items
                .iter()
                .map(|item| {
                    item.as_str()
                        .filter(|text| !text.is_empty())
                        .map(str::to_owned)
                        .ok_or_else(|| {
                            format!("[capabilities].{key} must contain only non-empty text")
                        })
                })
                .collect()
        };
        let flag = |key: &str| -> Result<bool, String> {
            match table.get(key) {
                None => Ok(false),
                Some(entry) => entry
                    .as_bool()
                    .ok_or_else(|| format!("[capabilities].{key} must be true or false")),
            }
        };
        let capabilities = Capabilities {
            network: list("network")?,
            files: list("files")?,
            environment: list("environment")?,
            processes: list("processes")?,
            clock: flag("clock")?,
            python: flag("python")?,
        };
        if let Some(bad) = capabilities
            .files
            .iter()
            .find(|mode| !matches!(mode.as_str(), "read" | "write"))
        {
            return Err(format!(
                "[capabilities].files entries must be \"read\" or \"write\", got `{bad}`"
            ));
        }
        Ok(capabilities)
    }

    pub fn to_toml(&self) -> toml::Value {
        let mut table = toml::map::Map::new();
        let list = |items: &BTreeSet<String>| {
            toml::Value::Array(items.iter().cloned().map(toml::Value::String).collect())
        };
        if !self.network.is_empty() {
            table.insert("network".into(), list(&self.network));
        }
        if !self.files.is_empty() {
            table.insert("files".into(), list(&self.files));
        }
        if !self.environment.is_empty() {
            table.insert("environment".into(), list(&self.environment));
        }
        if !self.processes.is_empty() {
            table.insert("processes".into(), list(&self.processes));
        }
        if self.clock {
            table.insert("clock".into(), toml::Value::Boolean(true));
        }
        if self.python {
            table.insert("python".into(), toml::Value::Boolean(true));
        }
        toml::Value::Table(table)
    }

    /// What `self` asks for beyond `approved`, one line per kind of access, in the form
    /// `network: +hosts ["api.example.com"]`. Empty when nothing is beyond it.
    pub fn delta_beyond(&self, approved: &Capabilities) -> Vec<String> {
        let mut lines = Vec::new();
        let mut extend =
            |label: &str, noun: &str, now: &BTreeSet<String>, ok: &BTreeSet<String>| {
                let extra: Vec<String> =
                    now.difference(ok).map(|item| format!("{item:?}")).collect();
                if !extra.is_empty() {
                    lines.push(format!("{label}: +{noun} [{}]", extra.join(", ")));
                }
            };
        extend("network", "hosts", &self.network, &approved.network);
        extend("files", "modes", &self.files, &approved.files);
        extend(
            "environment",
            "variables",
            &self.environment,
            &approved.environment,
        );
        extend(
            "processes",
            "commands",
            &self.processes,
            &approved.processes,
        );
        if self.clock && !approved.clock {
            lines.push("clock: +true".into());
        }
        if self.python && !approved.python {
            lines.push("python: +true".into());
        }
        lines
    }

    /// Compares a declaration with the effects static inspection found in the library's own
    /// modules. Returns `(errors, warnings)`.
    pub fn compare_with_effects(&self, used: &BTreeSet<String>) -> (Vec<String>, Vec<String>) {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        let network_used = used.contains("network.http") || used.contains("network.socket");
        if network_used && self.network.is_empty() {
            errors.push("the library uses the network but declares no hosts; add `network = [\"host\"]` to [capabilities] in guion.toml".into());
        } else if !network_used && !self.network.is_empty() {
            warnings.push(
                "[capabilities] declares network hosts but the library never uses the network"
                    .into(),
            );
        }
        let mut require = |used_effect: &str, declared: bool, advice: &str| {
            if used.contains(used_effect) && !declared {
                errors.push(format!("the library uses `{used_effect}` but does not declare it; add {advice} to [capabilities] in guion.toml"));
            } else if !used.contains(used_effect) && declared {
                warnings.push(format!(
                    "[capabilities] declares {advice} but the library never uses `{used_effect}`"
                ));
            }
        };
        require(
            "filesystem.read",
            self.files.contains("read"),
            "`files = [\"read\"]`",
        );
        require(
            "filesystem.write",
            self.files.contains("write"),
            "`files = [\"write\"]`",
        );
        require(
            "process.environment",
            !self.environment.is_empty(),
            "`environment = [\"NAME\"]`",
        );
        require(
            "process.spawn",
            !self.processes.is_empty(),
            "`processes = [\"command\"]`",
        );
        require("clock.read", self.clock, "`clock = true`");
        require("python.opaque", self.python, "`python = true`");
        (errors, warnings)
    }
}

impl Capabilities {
    /// Compares declared network hosts with the hosts named by URL literals in the library's
    /// code. An undeclared literal host is an error; network use with no literal host at all is
    /// reported, because the declaration then cannot be confirmed from the code.
    pub fn compare_with_hosts(
        &self,
        used: &BTreeSet<String>,
        literal_hosts: &std::collections::BTreeMap<String, String>,
    ) -> (Vec<String>, Vec<String>) {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        if !(used.contains("network.http") || used.contains("network.socket")) {
            return (errors, warnings);
        }
        let declared: BTreeSet<String> = self
            .network
            .iter()
            .map(|host| host.to_ascii_lowercase())
            .collect();
        for (host, function) in literal_hosts {
            if !declared.contains(host) {
                errors.push(format!("`{function}` contacts `{host}`, which [capabilities] `network` does not list; add it to `network` in guion.toml"));
            }
        }
        if literal_hosts.is_empty() && !self.network.is_empty() {
            warnings.push("no host in [capabilities] `network` appears as a URL in the library's code, so the declaration cannot be confirmed; reviewers should check where requests go".into());
        }
        (errors, warnings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Capabilities {
        let value: toml::Value = toml::from_str(text).unwrap();
        Capabilities::from_toml(value.get("capabilities")).unwrap()
    }

    fn effects(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    #[test]
    fn declarations_round_trip_through_toml() {
        let declared =
            parse("[capabilities]\nnetwork = [\"a.example\"]\nfiles = [\"read\"]\nclock = true\n");
        assert!(declared.network.contains("a.example") && declared.clock);
        let again = Capabilities::from_toml(Some(&declared.to_toml())).unwrap();
        assert_eq!(declared, again);
        assert!(Capabilities::default().is_empty());
    }

    #[test]
    fn bad_declarations_are_refused() {
        for text in [
            "[capabilities]\nnetwork = \"x\"\n",
            "[capabilities]\nfiles = [\"execute\"]\n",
            "[capabilities]\nbogus = true\n",
            "[capabilities]\nclock = \"yes\"\n",
        ] {
            let value: toml::Value = toml::from_str(text).unwrap();
            assert!(
                Capabilities::from_toml(value.get("capabilities")).is_err(),
                "{text}"
            );
        }
    }

    #[test]
    fn undeclared_use_is_an_error_and_unused_declarations_warn() {
        let declared = parse("[capabilities]\nfiles = [\"read\"]\nclock = true\n");
        let (errors, warnings) = declared.compare_with_effects(&effects(&[
            "filesystem.read",
            "network.http",
            "process.spawn",
        ]));
        assert_eq!(errors.len(), 2, "{errors:?}");
        assert!(errors.iter().any(|e| e.contains("uses the network")));
        assert!(errors.iter().any(|e| e.contains("process.spawn")));
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("clock"));
    }

    #[test]
    fn widening_is_reported_as_a_delta() {
        let before = parse("[capabilities]\nnetwork = [\"a.example\"]\n");
        let after = parse(
            "[capabilities]\nnetwork = [\"a.example\", \"b.example\"]\nprocesses = [\"git\"]\n",
        );
        let added = after.delta_beyond(&before);
        assert_eq!(
            added,
            vec![
                "network: +hosts [\"b.example\"]",
                "processes: +commands [\"git\"]"
            ]
        );
        assert!(before.delta_beyond(&after).is_empty());
    }
}
