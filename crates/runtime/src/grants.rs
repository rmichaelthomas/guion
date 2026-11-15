//! Access grants that can be asked for while a program runs, and the record of what was denied.
//!
//! A `HostPolicy` still decides everything from its fixed sets. The only addition is an optional
//! [`GrantAsker`]: when an effect is missing a grant, the policy may put one plain question to a
//! person through it. The asker is a convenience for people and never the security boundary: it
//! is absent unless the embedder installs one, and nothing a running program does can install it.

use crate::HostPolicy;
use guion_model::GrantHint;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

/// What kind of access a question or a denial is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GrantKind {
    HttpHost,
    Command,
    Clock,
    Environment,
    Connect,
    Bind,
}

impl GrantKind {
    /// Plain words for the question shown to a person, never a flag name or effect kind.
    pub fn phrase(self, target: &str) -> String {
        match self {
            GrantKind::HttpHost => format!("fetch from {target}"),
            GrantKind::Command => format!("run {target}"),
            GrantKind::Clock => "read the clock".to_owned(),
            GrantKind::Environment => format!("read the environment variable {target}"),
            GrantKind::Connect => format!("connect to {target}"),
            GrantKind::Bind => format!("listen on {target}"),
        }
    }

    /// The stable name used in reports and tapes.
    pub fn name(self) -> &'static str {
        match self {
            GrantKind::HttpHost => "http_host",
            GrantKind::Command => "command",
            GrantKind::Clock => "clock",
            GrantKind::Environment => "environment",
            GrantKind::Connect => "connect",
            GrantKind::Bind => "bind",
        }
    }
}

/// One thing a program wants and does not have.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrantRequest {
    pub kind: GrantKind,
    pub target: String,
    pub grant: GrantHint,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrantAnswer {
    No,
    ThisRun,
    /// Allowed now; the asker has saved the grant where the project keeps its grants.
    Always,
}

/// Puts a question to a person. Implemented by the command line; there is no guest-code route to it.
pub trait GrantAsker: Send + Sync {
    fn ask(&self, request: &GrantRequest) -> GrantAnswer;
}

/// A grant a person gave at a question, kept so a recording can say where each grant came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrantAnswerRecord {
    pub kind: GrantKind,
    pub target: String,
    pub always: bool,
}

/// A denied effect, recorded whether the program saw it as a value or as an error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DenialRecord {
    pub code: String,
    pub message: String,
    pub grant: Option<GrantHint>,
}

#[derive(Default)]
struct BrokerState {
    granted: HashSet<(GrantKind, String)>,
    refused: HashSet<(GrantKind, String)>,
}

/// Serializes questions and remembers answers, so one target is asked about at most once per run
/// and workers running in parallel cannot interleave two questions.
pub struct GrantBroker {
    asker: Arc<dyn GrantAsker>,
    state: Mutex<BrokerState>,
    answers: Arc<Mutex<Vec<GrantAnswerRecord>>>,
}

impl std::fmt::Debug for GrantBroker {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("GrantBroker")
    }
}

impl GrantBroker {
    pub fn new(asker: Arc<dyn GrantAsker>, answers: Arc<Mutex<Vec<GrantAnswerRecord>>>) -> Self {
        Self {
            asker,
            state: Mutex::new(BrokerState::default()),
            answers,
        }
    }

    fn permits(&self, kind: GrantKind, target: &str, hint: &GrantHint) -> bool {
        // The lock is held across the question on purpose: it is what serializes questions.
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        };
        let key = (kind, target.to_owned());
        if state.granted.contains(&key) {
            return true;
        }
        if state.refused.contains(&key) {
            return false;
        }
        let request = GrantRequest {
            kind,
            target: target.to_owned(),
            grant: hint.clone(),
        };
        match self.asker.ask(&request) {
            GrantAnswer::No => {
                state.refused.insert(key);
                false
            }
            answer => {
                state.granted.insert(key);
                push_answer(
                    &self.answers,
                    GrantAnswerRecord {
                        kind,
                        target: target.to_owned(),
                        always: answer == GrantAnswer::Always,
                    },
                );
                true
            }
        }
    }
}

fn push_answer(answers: &Mutex<Vec<GrantAnswerRecord>>, record: GrantAnswerRecord) {
    match answers.lock() {
        Ok(mut answers) => answers.push(record),
        Err(poisoned) => poisoned.into_inner().push(record),
    }
}

impl HostPolicy {
    /// Let `asker` put a question to a person when an effect lacks a grant. Used by `guion run`
    /// and `guion repl` only when a person is present; never by tests, replays or agents.
    pub fn with_grant_asker(mut self, asker: Arc<dyn GrantAsker>) -> Self {
        self.broker = Some(Arc::new(GrantBroker::new(asker, self.answers.clone())));
        self
    }

    /// Every denial so far, in order.
    pub fn denials(&self) -> Vec<DenialRecord> {
        match self.denials.lock() {
            Ok(denials) => denials.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    /// The grants a person gave at a question during this run.
    pub fn answered_grants(&self) -> Vec<GrantAnswerRecord> {
        match self.answers.lock() {
            Ok(answers) => answers.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    /// Note a grant a person gave before the run started (the question `guion run` asks first),
    /// so a recording can say where each grant came from.
    pub fn note_answered_grant(&self, kind: GrantKind, target: &str, always: bool) {
        push_answer(
            &self.answers,
            GrantAnswerRecord {
                kind,
                target: target.to_owned(),
                always,
            },
        );
    }

    pub(crate) fn record_denial(&self, code: &str, message: &str, grant: Option<&GrantHint>) {
        let record = DenialRecord {
            code: code.to_owned(),
            message: message.to_owned(),
            grant: grant.cloned(),
        };
        let mut denials = match self.denials.lock() {
            Ok(denials) => denials,
            Err(poisoned) => poisoned.into_inner(),
        };
        // A loop that retries a denied effect must not grow the record without bound.
        if denials.len() < 256 && !denials.contains(&record) {
            denials.push(record);
        }
    }

    /// Whether `target` may be used: granted by the fixed sets (`statically_granted`), already
    /// granted for this run, or allowed by a person who is asked now.
    pub(crate) fn authorize(
        &self,
        kind: GrantKind,
        target: &str,
        statically_granted: bool,
        hint: &GrantHint,
    ) -> bool {
        if statically_granted {
            return true;
        }
        self.broker
            .as_ref()
            .is_some_and(|broker| broker.permits(kind, target, hint))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Counting {
        asked: AtomicUsize,
        in_flight: AtomicUsize,
        overlapped: AtomicUsize,
        answer: GrantAnswer,
    }

    impl GrantAsker for Counting {
        fn ask(&self, _request: &GrantRequest) -> GrantAnswer {
            self.asked.fetch_add(1, Ordering::SeqCst);
            if self.in_flight.fetch_add(1, Ordering::SeqCst) != 0 {
                self.overlapped.fetch_add(1, Ordering::SeqCst);
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
            self.in_flight.fetch_sub(1, Ordering::SeqCst);
            self.answer
        }
    }

    fn counting(answer: GrantAnswer) -> Arc<Counting> {
        Arc::new(Counting {
            asked: AtomicUsize::new(0),
            in_flight: AtomicUsize::new(0),
            overlapped: AtomicUsize::new(0),
            answer,
        })
    }

    #[test]
    fn workers_never_interleave_questions_and_a_target_is_asked_once() {
        let asker = counting(GrantAnswer::ThisRun);
        let policy = HostPolicy::default().with_grant_asker(asker.clone());
        let handles = (0..8)
            .map(|index| {
                let policy = policy.clone();
                std::thread::spawn(move || {
                    let target = if index % 2 == 0 {
                        "a.example"
                    } else {
                        "b.example"
                    };
                    policy.authorize(
                        GrantKind::HttpHost,
                        target,
                        false,
                        &GrantHint::network_host(target),
                    )
                })
            })
            .collect::<Vec<_>>();
        for handle in handles {
            assert!(handle.join().unwrap());
        }
        assert_eq!(asker.asked.load(Ordering::SeqCst), 2, "once per target");
        assert_eq!(asker.overlapped.load(Ordering::SeqCst), 0, "serialized");
        assert_eq!(policy.answered_grants().len(), 2);
    }

    #[test]
    fn a_refusal_is_remembered_and_a_policy_without_an_asker_never_asks() {
        let asker = counting(GrantAnswer::No);
        let policy = HostPolicy::default().with_grant_asker(asker.clone());
        let hint = GrantHint::command("git");
        assert!(!policy.authorize(GrantKind::Command, "git", false, &hint));
        assert!(!policy.authorize(GrantKind::Command, "git", false, &hint));
        assert_eq!(asker.asked.load(Ordering::SeqCst), 1);
        assert!(policy.answered_grants().is_empty());

        let silent = HostPolicy::default();
        assert!(!silent.authorize(GrantKind::Command, "git", false, &hint));
        assert!(silent.authorize(GrantKind::Command, "git", true, &hint));
    }
}
