//! What a program needs versus what it has: the `inspect --grants` report, which also drives the
//! question `guion run` asks before a run. See `docs/design-scoped-profiles.md`.
//!
//! Targets come from literals the compiled code can see. Where a target is computed (a host read
//! from a file, say) the report says so instead of guessing.

use guion_model::{Artifact, GrantHint, Op, Value};
use guion_runtime::{GrantKind, HostPolicy};
use std::collections::BTreeSet;

/// One thing the entry-reachable code may do that needs a grant.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Need {
    pub kind: NeedKind,
    /// The literal target, or `None` when it is only known while running.
    pub target: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum NeedKind {
    Fetch,
    Run,
    Clock,
    Environment,
    Connect,
    Listen,
}

impl NeedKind {
    fn grant_kind(self) -> GrantKind {
        match self {
            NeedKind::Fetch => GrantKind::HttpHost,
            NeedKind::Run => GrantKind::Command,
            NeedKind::Clock => GrantKind::Clock,
            NeedKind::Environment => GrantKind::Environment,
            NeedKind::Connect => GrantKind::Connect,
            NeedKind::Listen => GrantKind::Bind,
        }
    }

    fn name(self) -> &'static str {
        self.grant_kind().name()
    }
}

impl Need {
    /// Plain words, as shown in the report and in the question.
    pub fn phrase(&self) -> String {
        match &self.target {
            Some(target) => self.kind.grant_kind().phrase(target),
            None => match self.kind {
                NeedKind::Fetch => "fetch from a host chosen while running".to_owned(),
                NeedKind::Run => "run a command chosen while running".to_owned(),
                NeedKind::Environment => {
                    "read an environment variable chosen while running".to_owned()
                }
                NeedKind::Connect => "connect to a host chosen while running".to_owned(),
                NeedKind::Listen => "listen on an address chosen while running".to_owned(),
                NeedKind::Clock => "read the clock".to_owned(),
            },
        }
    }

    /// The grant that would allow this need, when the target is known.
    pub fn hint(&self) -> Option<GrantHint> {
        let target = self.target.as_deref();
        Some(match (self.kind, target) {
            (NeedKind::Clock, _) => GrantHint::clock(),
            (NeedKind::Fetch, Some(host)) => GrantHint::network_host(host),
            (NeedKind::Run, Some(name)) => GrantHint::command(name),
            (NeedKind::Environment, Some(name)) => GrantHint::environment_variable(name),
            (NeedKind::Connect, Some(entry)) => GrantHint::socket_connect(entry),
            (NeedKind::Listen, Some(entry)) => GrantHint::socket_bind(entry),
            _ => return None,
        })
    }

    pub fn is_granted(&self, policy: &HostPolicy) -> bool {
        let Some(target) = self.target.as_deref() else {
            return false;
        };
        match self.kind {
            NeedKind::Clock => policy.allow_clock,
            NeedKind::Fetch => policy.allowed_http_hosts.contains(target),
            NeedKind::Run => policy.allowed_commands.contains(target),
            NeedKind::Environment => policy.allowed_environment_variables.contains(target),
            NeedKind::Connect => socket_granted(&policy.allowed_connect, target),
            NeedKind::Listen => socket_granted(&policy.allowed_bind, target),
        }
    }

    pub fn is_computed(&self) -> bool {
        self.target.is_none() && self.kind != NeedKind::Clock
    }
}

fn socket_granted(grants: &std::collections::HashSet<String>, entry: &str) -> bool {
    grants.contains(entry)
        || entry
            .rsplit_once(':')
            .is_some_and(|(host, _)| grants.contains(&format!("{host}:*")))
}

/// The pushes and pops of the few instructions that build the arguments of a call.
fn stack_effect(op: &Op) -> Option<(usize, usize)> {
    match op {
        Op::Const(_) | Op::Load(_) => Some((0, 1)),
        Op::MakeList(count) => Some((*count, 1)),
        _ => None,
    }
}

/// The literal text of each argument of the call ending at `call` (integers as digits), `None` where an argument is
/// not a single text or integer constant. `None` overall when the argument code is too complex to read.
fn literal_arguments(
    code: &[guion_model::Instruction],
    call: usize,
    argc: usize,
) -> Option<Vec<Option<String>>> {
    let mut end = call;
    let mut result = vec![None; argc];
    for slot in (0..argc).rev() {
        let mut outstanding = 1usize;
        let mut start = end;
        while outstanding > 0 {
            start = start.checked_sub(1)?;
            let (pops, pushes) = stack_effect(&code[start].op)?;
            outstanding = outstanding + pops - pushes;
        }
        if end - start == 1 {
            match &code[start].op {
                Op::Const(Value::Text(text)) => result[slot] = Some(text.as_ref().to_owned()),
                Op::Const(Value::Int(number)) => result[slot] = Some(number.to_string()),
                _ => {}
            }
        }
        end = start;
    }
    Some(result)
}

fn short_name(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or(name)
}

/// Every grant the entry-reachable code may need, deduplicated and ordered.
pub fn needs(artifact: &Artifact, inspected: &serde_json::Value) -> Vec<Need> {
    let mut found = BTreeSet::new();
    let mut literal_hosts = BTreeSet::new();
    let mut fetches = false;
    for function in &artifact.functions {
        let entry = &inspected["functions"][function.name.as_str()];
        if entry["entry_reachable"] != true {
            continue;
        }
        let direct = |effect: &str| {
            entry["direct_effects"]
                .as_array()
                .is_some_and(|effects| effects.iter().any(|value| value == effect))
        };
        if direct("clock.read") {
            found.insert(Need {
                kind: NeedKind::Clock,
                target: None,
            });
        }
        if direct("network.http") {
            fetches = true;
        }
        for (index, instruction) in function.code.iter().enumerate() {
            match &instruction.op {
                Op::Const(Value::Text(text)) => {
                    let text: &str = text.as_ref();
                    if ["http://", "https://"]
                        .iter()
                        .any(|scheme| text.starts_with(scheme))
                    {
                        if let Some(host) = reqwest::Url::parse(text).ok().and_then(|url| {
                            url.host_str()
                                .map(|host| host.trim_end_matches('.').to_ascii_lowercase())
                        }) {
                            literal_hosts.insert(host);
                        }
                    }
                }
                Op::Environment => {
                    let name = literal_arguments(&function.code, index, 1)
                        .and_then(|mut arguments| arguments.remove(0));
                    found.insert(Need {
                        kind: NeedKind::Environment,
                        target: name,
                    });
                }
                Op::Call(target, argc) => {
                    let Some(callee) = artifact.functions.get(*target) else {
                        continue;
                    };
                    let arguments = literal_arguments(&function.code, index, *argc);
                    let argument = |position: usize| {
                        arguments
                            .as_ref()
                            .and_then(|arguments| arguments.get(position).cloned().flatten())
                    };
                    match short_name(&callee.name) {
                        "process_run" => {
                            found.insert(Need {
                                kind: NeedKind::Run,
                                target: argument(0),
                            });
                        }
                        "tcp_connect" | "tls_connect" | "tls_connect_with_identity" => {
                            found.insert(socket_need(NeedKind::Connect, argument(0), argument(1)));
                        }
                        "udp_send_to" => {
                            found.insert(socket_need(NeedKind::Connect, argument(1), argument(2)));
                        }
                        "tcp_listen" | "udp_bind" | "http_server_listen" => {
                            found.insert(socket_need(NeedKind::Listen, argument(0), argument(1)));
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }
    if fetches {
        if literal_hosts.is_empty() {
            found.insert(Need {
                kind: NeedKind::Fetch,
                target: None,
            });
        }
        for host in literal_hosts {
            found.insert(Need {
                kind: NeedKind::Fetch,
                target: Some(host),
            });
        }
    }
    found.into_iter().collect()
}

fn socket_need(kind: NeedKind, host: Option<String>, port: Option<String>) -> Need {
    Need {
        kind,
        target: match (host, port) {
            (Some(host), Some(port)) => Some(format!("{host}:{port}")),
            _ => None,
        },
    }
}

/// The report as JSON: each need with whether the project grants it and the grant that would.
pub fn report_json(source: &str, needs: &[Need], policy: &HostPolicy) -> serde_json::Value {
    let entries = needs
        .iter()
        .map(|need| {
            let hint = need.hint();
            serde_json::json!({
                "effect": need.kind.name(),
                "description": need.phrase(),
                "target": need.target,
                "computed": need.is_computed(),
                "granted": need.is_granted(policy),
                "grant": hint.as_ref().map(crate::grant_json),
            })
        })
        .collect::<Vec<_>>();
    let missing = needs
        .iter()
        .filter(|need| !need.is_granted(policy))
        .filter_map(Need::hint)
        .map(|hint| crate::grant_json(&hint))
        .collect::<Vec<_>>();
    serde_json::json!({
        "schema_version": 1,
        "kind": "guion_grants_report",
        "source": source,
        "needs": entries,
        "missing": missing,
        "capability_claim": "static_bytecode_literals_not_sandbox",
    })
}

/// The report as the two-column text shown by `guion inspect --grants`.
pub fn report_text(needs: &[Need], policy: &HostPolicy) -> String {
    if needs.is_empty() {
        return "needs no grants: nothing it can reach needs clock, network, command, socket or environment access\n"
            .to_owned();
    }
    let width = needs
        .iter()
        .map(|need| need.phrase().len())
        .max()
        .unwrap_or(0)
        .max(28);
    let mut text = format!("{:<w$}granted?\n", "needs", w = width + 2);
    for need in needs {
        let phrase = need.phrase();
        let status = if need.is_granted(policy) {
            match need.hint().and_then(|hint| hint.table) {
                Some(table) => format!("yes  (guion.toml [{table}])"),
                None => "yes".to_owned(),
            }
        } else if let Some(hint) = need.hint() {
            let how = hint
                .manifest_line()
                .map(|line| format!("→ {line}"))
                .unwrap_or_else(|| format!("→ {}", hint.flag));
            format!("no   {how}")
        } else {
            "no   → known only while running".to_owned()
        };
        text.push_str(&format!("  {phrase:<width$}  {status}\n"));
    }
    text
}
