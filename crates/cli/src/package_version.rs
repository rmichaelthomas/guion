//! Version numbers, version ranges and the dependency resolver.
//!
//! Versions are `major.minor.patch` (no pre-release tags yet). A bare `1.2.3` in a manifest means
//! exactly that version, as it always has. Ranges use explicit operators: `^1.2`, `~1.2.3`,
//! `>=1.0, <2.0`, `=1.4`, or `*`. The resolver picks the highest version that satisfies every
//! requirement, deterministically, and explains a failure in plain words.
//!
//! Nothing outside the tests calls the resolver until the registry client (A7) supplies the
//! candidate lists; the exact-version install path does not need it.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl Version {
    pub fn parse(text: &str) -> Result<Version, String> {
        let (version, parts) = parse_partial(text)?;
        if parts != 3 {
            return Err(format!("version must be major.minor.patch, got `{text}`"));
        }
        Ok(version)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Parses one to three numeric parts; missing parts are zero. Returns how many were written.
fn parse_partial(text: &str) -> Result<(Version, usize), String> {
    let pieces: Vec<&str> = text.trim().split('.').collect();
    if pieces.is_empty() || pieces.len() > 3 {
        return Err(format!("`{text}` is not a version"));
    }
    let mut numbers = [0u64; 3];
    for (slot, piece) in pieces.iter().enumerate() {
        if piece.is_empty() || !piece.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(format!("`{text}` is not a version"));
        }
        numbers[slot] = piece
            .parse()
            .map_err(|_| format!("`{text}` has a number that is too large"))?;
    }
    Ok((
        Version {
            major: numbers[0],
            minor: numbers[1],
            patch: numbers[2],
        },
        pieces.len(),
    ))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Bound {
    AtLeast(Version),
    Above(Version),
    AtMost(Version),
    Below(Version),
}

impl Bound {
    fn allows(self, version: Version) -> bool {
        match self {
            Bound::AtLeast(bound) => version >= bound,
            Bound::Above(bound) => version > bound,
            Bound::AtMost(bound) => version <= bound,
            Bound::Below(bound) => version < bound,
        }
    }
}

/// A set of version bounds that must all hold.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VersionReq {
    text: String,
    bounds: Vec<Bound>,
}

impl VersionReq {
    pub fn parse(text: &str) -> Result<VersionReq, String> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Err("version requirement is empty".into());
        }
        let mut bounds = Vec::new();
        for clause in trimmed.split(',') {
            parse_clause(clause.trim(), &mut bounds)
                .map_err(|problem| format!("invalid version requirement `{trimmed}`: {problem}"))?;
        }
        Ok(VersionReq {
            text: trimmed.to_owned(),
            bounds,
        })
    }

    pub fn exact(version: Version) -> VersionReq {
        VersionReq {
            text: version.to_string(),
            bounds: vec![Bound::AtLeast(version), Bound::AtMost(version)],
        }
    }

    pub fn matches(&self, version: Version) -> bool {
        self.bounds.iter().all(|bound| bound.allows(version))
    }
}

impl fmt::Display for VersionReq {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.text)
    }
}

fn next_major(version: Version) -> Version {
    Version {
        major: version.major + 1,
        minor: 0,
        patch: 0,
    }
}

fn next_minor(version: Version) -> Version {
    Version {
        major: version.major,
        minor: version.minor + 1,
        patch: 0,
    }
}

fn next_patch(version: Version) -> Version {
    Version {
        patch: version.patch + 1,
        ..version
    }
}

fn parse_clause(clause: &str, bounds: &mut Vec<Bound>) -> Result<(), String> {
    if clause == "*" {
        return Ok(());
    }
    let (operator, rest) = ["^", "~", ">=", "<=", ">", "<", "="]
        .iter()
        .find_map(|operator| clause.strip_prefix(operator).map(|rest| (*operator, rest)))
        .unwrap_or(("", clause));
    let rest = rest.trim();
    let (version, parts) = parse_partial(rest)?;
    match operator {
        "" => {
            if parts != 3 {
                return Err(format!(
                    "`{rest}` needs an operator; write `^{rest}` for compatible versions or `={rest}.0` for one version"
                ));
            }
            bounds.push(Bound::AtLeast(version));
            bounds.push(Bound::AtMost(version));
        }
        "=" => {
            bounds.push(Bound::AtLeast(version));
            bounds.push(Bound::Below(match parts {
                1 => next_major(version),
                2 => next_minor(version),
                _ => next_patch(version),
            }));
        }
        "^" => {
            bounds.push(Bound::AtLeast(version));
            let upper = if version.major > 0 || parts == 1 {
                next_major(version)
            } else if version.minor > 0 || parts == 2 {
                next_minor(version)
            } else {
                next_patch(version)
            };
            bounds.push(Bound::Below(upper));
        }
        "~" => {
            bounds.push(Bound::AtLeast(version));
            bounds.push(Bound::Below(if parts == 1 {
                next_major(version)
            } else {
                next_minor(version)
            }));
        }
        ">=" => bounds.push(Bound::AtLeast(version)),
        ">" => bounds.push(Bound::Above(version)),
        "<=" => bounds.push(Bound::AtMost(version)),
        "<" => bounds.push(Bound::Below(version)),
        _ => unreachable!("operator list is closed"),
    }
    Ok(())
}

/// One published version of a package and what it needs.
#[derive(Clone, Debug)]
pub struct Candidate {
    pub version: Version,
    pub dependencies: Vec<(String, VersionReq)>,
    /// Yanked versions are skipped unless the lock already pins them.
    pub yanked: bool,
}

/// Where the resolver learns which versions exist.
pub trait PackageIndex {
    fn candidates(&self, name: &str) -> Result<Vec<Candidate>, String>;
}

const ROOT: &str = "this project";

struct Search<'a> {
    index: &'a dyn PackageIndex,
    pinned: &'a BTreeMap<String, Version>,
    chosen: BTreeMap<String, Version>,
    requirements: BTreeMap<String, Vec<(VersionReq, String)>>,
    failure: Option<String>,
    steps: usize,
}

const MAX_STEPS: usize = 100_000;

/// Resolves `roots` (the project's own requirements) to one version per package. `pinned` holds
/// versions the lock already uses: those stay eligible even if yanked.
pub fn resolve(
    roots: &[(String, VersionReq)],
    index: &dyn PackageIndex,
    pinned: &BTreeMap<String, Version>,
) -> Result<BTreeMap<String, Version>, String> {
    let mut search = Search {
        index,
        pinned,
        chosen: BTreeMap::new(),
        requirements: BTreeMap::new(),
        failure: None,
        steps: 0,
    };
    let mut pending = Vec::new();
    for (name, requirement) in roots {
        search
            .requirements
            .entry(name.clone())
            .or_default()
            .push((requirement.clone(), ROOT.to_owned()));
        if !pending.contains(name) {
            pending.push(name.clone());
        }
    }
    pending.sort();
    if search.solve(pending)? {
        Ok(search.chosen)
    } else {
        Err(search
            .failure
            .unwrap_or_else(|| "the requirements cannot be satisfied".into()))
    }
}

impl Search<'_> {
    fn solve(&mut self, mut pending: Vec<String>) -> Result<bool, String> {
        self.steps += 1;
        if self.steps > MAX_STEPS {
            return Err(
                "dependency resolution gave up: the requirements are too tangled to search".into(),
            );
        }
        let Some(position) = pending
            .iter()
            .position(|name| !self.chosen.contains_key(name))
        else {
            return Ok(true);
        };
        let name = pending.remove(position);
        let mut candidates = self.index.candidates(&name)?;
        candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.version));
        let constraints = self.requirements.get(&name).cloned().unwrap_or_default();
        let eligible: Vec<Candidate> = candidates
            .iter()
            .filter(|candidate| {
                (!candidate.yanked || self.pinned.get(&name) == Some(&candidate.version))
                    && constraints
                        .iter()
                        .all(|(requirement, _)| requirement.matches(candidate.version))
            })
            .cloned()
            .collect();
        if eligible.is_empty() {
            self.failure = Some(explain_no_match(&name, &constraints, &candidates));
            return Ok(false);
        }
        for candidate in eligible {
            let mut added: Vec<String> = Vec::new();
            let mut conflict = false;
            for (dependency, requirement) in &candidate.dependencies {
                let origin = format!("{name} {}", candidate.version);
                self.requirements
                    .entry(dependency.clone())
                    .or_default()
                    .push((requirement.clone(), origin));
                added.push(dependency.clone());
                if let Some(version) = self.chosen.get(dependency) {
                    if !requirement.matches(*version) {
                        let all = self.requirements[dependency].clone();
                        self.failure = Some(explain_chosen_conflict(dependency, *version, &all));
                        conflict = true;
                    }
                }
            }
            if !conflict {
                self.chosen.insert(name.clone(), candidate.version);
                let mut next = pending.clone();
                for dependency in &added {
                    if !next.contains(dependency) {
                        next.push(dependency.clone());
                    }
                }
                next.sort();
                if self.solve(next)? {
                    return Ok(true);
                }
                self.chosen.remove(&name);
            }
            for dependency in added.iter().rev() {
                if let Some(list) = self.requirements.get_mut(dependency) {
                    list.pop();
                    if list.is_empty() {
                        self.requirements.remove(dependency);
                    }
                }
            }
        }
        Ok(false)
    }
}

fn list_requirements(constraints: &[(VersionReq, String)]) -> String {
    constraints
        .iter()
        .map(|(requirement, origin)| format!("  - {requirement} (required by {origin})"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn explain_no_match(name: &str, constraints: &[(VersionReq, String)], all: &[Candidate]) -> String {
    let available = if all.is_empty() {
        "no versions are available".to_owned()
    } else {
        format!(
            "available versions: {}",
            all.iter()
                .map(|candidate| {
                    if candidate.yanked {
                        format!("{} (yanked)", candidate.version)
                    } else {
                        candidate.version.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    format!(
        "no version of `{name}` satisfies every requirement:\n{}\n{available}",
        list_requirements(constraints)
    )
}

fn explain_chosen_conflict(
    name: &str,
    chosen: Version,
    constraints: &[(VersionReq, String)],
) -> String {
    format!(
        "`{name}` {chosen} was selected but cannot satisfy every requirement:\n{}",
        list_requirements(constraints)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    fn r(text: &str) -> VersionReq {
        VersionReq::parse(text).unwrap()
    }

    #[test]
    fn versions_order_numerically() {
        assert!(v("1.10.0") > v("1.9.9"));
        assert!(Version::parse("1.2").is_err());
        assert!(Version::parse("1.2.x").is_err());
    }

    #[test]
    fn bare_versions_are_exact_and_partial_bare_versions_are_refused() {
        assert!(r("1.2.3").matches(v("1.2.3")));
        assert!(!r("1.2.3").matches(v("1.2.4")));
        assert!(VersionReq::parse("1.2")
            .unwrap_err()
            .contains("needs an operator"));
    }

    #[test]
    fn caret_tilde_and_comparison_ranges() {
        assert!(r("^1.2").matches(v("1.9.0")) && !r("^1.2").matches(v("2.0.0")));
        assert!(r("^0.2.3").matches(v("0.2.9")) && !r("^0.2.3").matches(v("0.3.0")));
        assert!(r("^0.0.3").matches(v("0.0.3")) && !r("^0.0.3").matches(v("0.0.4")));
        assert!(r("~1.2.3").matches(v("1.2.9")) && !r("~1.2.3").matches(v("1.3.0")));
        assert!(r(">=1.0, <2.0").matches(v("1.5.0")) && !r(">=1.0, <2.0").matches(v("2.0.0")));
        assert!(r("=1.4").matches(v("1.4.7")) && !r("=1.4").matches(v("1.5.0")));
        assert!(r("*").matches(v("9.9.9")));
        assert!(VersionReq::parse(">=").is_err() && VersionReq::parse("").is_err());
    }

    struct Fixed(BTreeMap<String, Vec<Candidate>>);

    impl PackageIndex for Fixed {
        fn candidates(&self, name: &str) -> Result<Vec<Candidate>, String> {
            Ok(self.0.get(name).cloned().unwrap_or_default())
        }
    }

    fn package(entries: &[(&str, &[(&str, &str)])]) -> Vec<Candidate> {
        entries
            .iter()
            .map(|(version, dependencies)| Candidate {
                version: v(version),
                dependencies: dependencies
                    .iter()
                    .map(|(name, requirement)| ((*name).to_owned(), r(requirement)))
                    .collect(),
                yanked: false,
            })
            .collect()
    }

    fn index(entries: Vec<(&str, Vec<Candidate>)>) -> Fixed {
        Fixed(
            entries
                .into_iter()
                .map(|(name, candidates)| (name.to_owned(), candidates))
                .collect(),
        )
    }

    fn roots(entries: &[(&str, &str)]) -> Vec<(String, VersionReq)> {
        entries
            .iter()
            .map(|(name, requirement)| ((*name).to_owned(), r(requirement)))
            .collect()
    }

    #[test]
    fn picks_the_highest_satisfying_version() {
        let index = index(vec![(
            "a",
            package(&[("1.0.0", &[]), ("1.4.0", &[]), ("2.0.0", &[])]),
        )]);
        let solved = resolve(&roots(&[("a", "^1.0")]), &index, &BTreeMap::new()).unwrap();
        assert_eq!(solved["a"], v("1.4.0"));
    }

    #[test]
    fn backtracks_to_an_older_version_when_the_newest_conflicts() {
        // a 2.0.0 needs c ^2, but the project also needs c ^1: a 1.0.0 is the only fit.
        let index = index(vec![
            (
                "a",
                package(&[("1.0.0", &[("c", "^1.0")]), ("2.0.0", &[("c", "^2.0")])]),
            ),
            ("c", package(&[("1.5.0", &[]), ("2.1.0", &[])])),
        ]);
        let solved = resolve(
            &roots(&[("a", ">=1.0"), ("c", "^1.0")]),
            &index,
            &BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(solved["a"], v("1.0.0"));
        assert_eq!(solved["c"], v("1.5.0"));
    }

    #[test]
    fn failure_names_the_requirements_and_who_imposed_them() {
        let index = index(vec![
            ("a", package(&[("1.0.0", &[("c", "^2.0")])])),
            ("c", package(&[("1.5.0", &[]), ("2.1.0", &[])])),
        ]);
        let message = resolve(
            &roots(&[("a", "^1.0"), ("c", "^1.0")]),
            &index,
            &BTreeMap::new(),
        )
        .unwrap_err();
        let expected = "no version of `c` satisfies every requirement:\n  - ^1.0 (required by this project)\n  - ^2.0 (required by a 1.0.0)\navailable versions: 2.1.0, 1.5.0";
        assert!(message.contains(expected), "{message}");
    }

    #[test]
    fn yanked_versions_are_skipped_unless_pinned() {
        let mut candidates = package(&[("1.0.0", &[]), ("1.1.0", &[])]);
        candidates[1].yanked = true;
        let index = index(vec![("a", candidates)]);
        let fresh = resolve(&roots(&[("a", "^1.0")]), &index, &BTreeMap::new()).unwrap();
        assert_eq!(fresh["a"], v("1.0.0"));
        let pinned = BTreeMap::from([("a".to_owned(), v("1.1.0"))]);
        let locked = resolve(&roots(&[("a", "^1.0")]), &index, &pinned).unwrap();
        assert_eq!(locked["a"], v("1.1.0"));
    }

    #[test]
    fn missing_packages_and_cycles_are_reported_or_tolerated() {
        let none = index(vec![]);
        let message = resolve(&roots(&[("ghost", "*")]), &none, &BTreeMap::new()).unwrap_err();
        assert!(message.contains("no versions are available"), "{message}");
        let cycle = index(vec![
            ("a", package(&[("1.0.0", &[("b", "*")])])),
            ("b", package(&[("1.0.0", &[("a", "*")])])),
        ]);
        let solved = resolve(&roots(&[("a", "*")]), &cycle, &BTreeMap::new()).unwrap();
        assert_eq!(solved.len(), 2);
    }

    #[test]
    fn resolution_is_deterministic() {
        let index = index(vec![
            (
                "a",
                package(&[("1.0.0", &[("c", "*")]), ("1.1.0", &[("c", "*")])]),
            ),
            ("b", package(&[("1.0.0", &[("c", "^1.0")])])),
            ("c", package(&[("1.0.0", &[]), ("1.2.0", &[])])),
        ]);
        let first = resolve(&roots(&[("b", "*"), ("a", "*")]), &index, &BTreeMap::new()).unwrap();
        let second = resolve(&roots(&[("a", "*"), ("b", "*")]), &index, &BTreeMap::new()).unwrap();
        assert_eq!(first, second);
        assert_eq!(first["c"], v("1.2.0"));
    }
}
