//! Record and replay of everything a program receives from outside.
//!
//! A Guion program reaches the world only through host operations, and the runtime sees every
//! one. A *tape* is the ordered list of those operations with the values each one left on the
//! stack. Recording writes the tape; replaying feeds the program from it without touching the
//! file system, network, clock, environment or anything else, and stops at the first
//! operation that does not match what was recorded.

use guion_model::{
    BigInt, FileErrorValue, Float64, HttpErrorValue, JsonErrorValue, NativeResourceValue,
    RuntimeError, Shared, SharedJson, Span, TaskErrorValue, TextMap, Value,
};
use serde_json::{json, Value as Json};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Mutex,
};

/// What `Runtime::with_tape` should do.
#[derive(Clone, Debug)]
pub enum TapeMode {
    /// Run normally and write the tape to this file when the run ends.
    Record(PathBuf),
    /// Run from this tape, with no access to the outside world.
    Replay(PathBuf),
    /// Replay a tape even though the program has changed, to find the first call that differs.
    ReplayChanged(PathBuf),
}

/// The first tape format: one ordered list of operations.
pub(crate) const TAPE_VERSION: u64 = 1;
/// Adds lanes: `parallel_map` items each keep their own ordered list, so any worker count
/// replays the same. A tape with no lanes is still written as version 1.
pub(crate) const TAPE_VERSION_LANES: u64 = 2;

/// Sort key of a lane name such as `0.2/1.5` (call 0 item 2, then call 1 item 5 inside it);
/// the main lane, the empty name, sorts first.
fn lane_key(lane: &str) -> Vec<(u64, u64)> {
    lane.split('/')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let (call, item) = part.split_once('.').unwrap_or((part, "0"));
            (call.parse().unwrap_or(0), item.parse().unwrap_or(0))
        })
        .collect()
}

fn lane_label(lane: &str) -> String {
    if lane.is_empty() {
        String::new()
    } else {
        format!(" (in parallel_map lane {lane})")
    }
}

/// Values a tape cannot hold: function values and typed arrays.
fn unsupported(kind: &str) -> String {
    format!("a {kind} cannot be written to a tape")
}

fn spans_to_json(trace: &[Span]) -> Json {
    Json::Array(
        trace
            .iter()
            .map(|span| json!([span.line, span.column]))
            .collect(),
    )
}

fn spans_from_json(value: &Json) -> Vec<Span> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|item| Span {
                    line: item[0].as_u64().unwrap_or(0) as usize,
                    column: item[1].as_u64().unwrap_or(0) as usize,
                    ..Span::default()
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn encode_value(value: &Value) -> Result<Json, String> {
    Ok(match value {
        Value::Int(number) => json!({"int": number.to_string()}),
        Value::Float64(number) => json!({"float": number.get().to_bits().to_string()}),
        Value::Bool(flag) => json!({"bool": flag}),
        Value::Text(text) => json!({"text": text.as_ref() as &str}),
        Value::Bytes(bytes) => {
            json!({"bytes": bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()})
        }
        Value::Unit => json!({"unit": true}),
        Value::OptionNone => json!({"none": true}),
        Value::OptionSome(inner) => json!({"some": encode_value(inner)?}),
        Value::ResultOk(inner) => json!({"ok": encode_value(inner)?}),
        Value::ResultErr(inner) => json!({"err": encode_value(inner)?}),
        Value::List(items) => {
            json!({"list": items.iter().map(encode_value).collect::<Result<Vec<_>, _>>()?})
        }
        Value::Float64List(items) => json!({
            "list": items.iter().map(|item| json!({"float": item.get().to_bits().to_string()})).collect::<Vec<_>>()
        }),
        Value::Record(name, fields) => json!({
            "record": name.to_string(),
            "fields": fields.iter().map(encode_value).collect::<Result<Vec<_>, _>>()?,
        }),
        Value::Map(entries) => {
            let mut pairs = Vec::new();
            for (key, item) in entries.iter() {
                pairs.push(json!([key.as_ref() as &str, encode_value(item)?]));
            }
            json!({"map": pairs})
        }
        Value::Json(document) => {
            json!({"json": serde_json::to_value(document.as_ref()).map_err(|e| e.to_string())?})
        }
        Value::TaskError(error) => json!({"task_error": {
            "index": error.index.to_string(), "code": error.code, "message": error.message,
            "trace": spans_to_json(&error.trace)}}),
        Value::JsonError(error) => json!({"json_error": {
            "code": error.code, "message": error.message, "line": error.line, "column": error.column}}),
        Value::FileError(error) => json!({"file_error": {
            "code": error.code, "message": error.message, "trace": spans_to_json(&error.trace)}}),
        Value::HttpError(error) => json!({"http_error": {
            "code": error.code, "message": error.message, "trace": spans_to_json(&error.trace)}}),
        Value::NativeResource(resource) => {
            let (kind, owner, id) = resource.parts();
            json!({"resource": {"kind": kind, "owner": owner.to_string(), "id": id.to_string()}})
        }
        Value::Array(_) => return Err(unsupported("typed array")),
        Value::Function(_) | Value::Closure(..) => return Err(unsupported("function value")),
        Value::Task(_) | Value::Channel(_) => return Err(unsupported("task or channel")),
    })
}

pub(crate) fn decode_value(value: &Json) -> Result<Value, String> {
    let bad = || format!("tape holds a value it cannot read: {value}");
    let object = value.as_object().ok_or_else(bad)?;
    // A record has two keys, and the map may list them in either order.
    let (tag, body) = match object.get("record") {
        Some(name) => ("record", name),
        None => {
            let (tag, body) = object.iter().next().ok_or_else(bad)?;
            (tag.as_str(), body)
        }
    };
    let decode_list = |body: &Json| -> Result<Vec<Value>, String> {
        body.as_array()
            .ok_or_else(bad)?
            .iter()
            .map(decode_value)
            .collect()
    };
    Ok(match tag {
        "int" => Value::Int(BigInt::parse(body.as_str().ok_or_else(bad)?).map_err(|_| bad())?),
        "float" => {
            let bits: u64 = body.as_str().ok_or_else(bad)?.parse().map_err(|_| bad())?;
            Value::Float64(Float64::new(f64::from_bits(bits)).ok_or_else(bad)?)
        }
        "bool" => Value::Bool(body.as_bool().ok_or_else(bad)?),
        "text" => Value::Text(body.as_str().ok_or_else(bad)?.into()),
        "bytes" => {
            let text = body.as_str().ok_or_else(bad)?;
            let mut bytes = Vec::with_capacity(text.len() / 2);
            for pair in text.as_bytes().chunks(2) {
                let pair = std::str::from_utf8(pair).map_err(|_| bad())?;
                bytes.push(u8::from_str_radix(pair, 16).map_err(|_| bad())?);
            }
            Value::Bytes(Shared::new(bytes))
        }
        "unit" => Value::Unit,
        "none" => Value::OptionNone,
        "some" => Value::OptionSome(Box::new(decode_value(body)?)),
        "ok" => Value::ResultOk(Box::new(decode_value(body)?)),
        "err" => Value::ResultErr(Box::new(decode_value(body)?)),
        "list" => Value::from_list_items(decode_list(body)?),
        "record" => {
            let fields = object.get("fields").ok_or_else(bad)?;
            Value::Record(
                body.as_str().ok_or_else(bad)?.into(),
                Shared::new(decode_list(fields)?),
            )
        }
        "map" => {
            let mut map = TextMap::new();
            for pair in body.as_array().ok_or_else(bad)? {
                map.insert(pair[0].as_str().ok_or_else(bad)?, decode_value(&pair[1])?);
            }
            Value::Map(Shared::new(map))
        }
        "json" => Value::Json(SharedJson::from_serde(body.clone())),
        "task_error" => Value::TaskError(Box::new(TaskErrorValue {
            index: BigInt::parse(body["index"].as_str().ok_or_else(bad)?).map_err(|_| bad())?,
            code: body["code"].as_str().ok_or_else(bad)?.to_owned(),
            message: body["message"].as_str().ok_or_else(bad)?.to_owned(),
            trace: spans_from_json(&body["trace"]),
        })),
        "json_error" => Value::JsonError(Box::new(JsonErrorValue {
            code: body["code"].as_str().ok_or_else(bad)?.to_owned(),
            message: body["message"].as_str().ok_or_else(bad)?.to_owned(),
            line: body["line"].as_u64().unwrap_or(0) as usize,
            column: body["column"].as_u64().unwrap_or(0) as usize,
        })),
        "file_error" => Value::FileError(Box::new(FileErrorValue {
            code: body["code"].as_str().ok_or_else(bad)?.to_owned(),
            message: body["message"].as_str().ok_or_else(bad)?.to_owned(),
            trace: spans_from_json(&body["trace"]),
        })),
        "http_error" => Value::HttpError(Box::new(HttpErrorValue {
            code: body["code"].as_str().ok_or_else(bad)?.to_owned(),
            message: body["message"].as_str().ok_or_else(bad)?.to_owned(),
            trace: spans_from_json(&body["trace"]),
        })),
        "resource" => Value::NativeResource(Box::new(NativeResourceValue::new(
            body["kind"].as_str().ok_or_else(bad)?,
            body["owner"]
                .as_str()
                .ok_or_else(bad)?
                .parse()
                .map_err(|_| bad())?,
            body["id"]
                .as_str()
                .ok_or_else(bad)?
                .parse()
                .map_err(|_| bad())?,
        ))),
        _ => return Err(bad()),
    })
}

/// Request headers that carry credentials. They are never written to a tape and never compared
/// on replay, so a rotated token does not make a replay diverge.
fn is_credential_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "authorization" | "cookie" | "proxy-authorization"
    )
}

fn scrub_credentials(value: &Value) -> Value {
    match value {
        Value::Map(entries) => {
            let mut scrubbed = TextMap::new();
            for (key, item) in entries.iter() {
                if !is_credential_header(key.as_ref()) {
                    scrubbed.insert(key.clone(), scrub_credentials(item));
                }
            }
            Value::Map(Shared::new(scrubbed))
        }
        Value::List(items) => {
            Value::List(Shared::new(items.iter().map(scrub_credentials).collect()))
        }
        Value::Record(name, fields) => Value::Record(
            name.clone(),
            Shared::new(fields.iter().map(scrub_credentials).collect()),
        ),
        Value::OptionSome(inner) => Value::OptionSome(Box::new(scrub_credentials(inner))),
        other => other.clone(),
    }
}

/// A hash of the whole stack at a host call, and the top few values in readable form. The
/// stack holds the call's arguments, so two runs that call with different arguments differ here.
fn stack_fingerprint(stack: &[Value]) -> (String, String) {
    let stack: Vec<Value> = stack.iter().map(scrub_credentials).collect();
    let mut digest = Sha256::new();
    for value in &stack {
        match encode_value(value) {
            Ok(encoded) => digest.update(encoded.to_string().as_bytes()),
            Err(_) => digest.update(b"<opaque>"),
        }
        digest.update(b"\n");
    }
    let preview = stack
        .iter()
        .rev()
        .take(3)
        .rev()
        .map(|value| {
            let text = value.to_string();
            if text.chars().count() > 60 {
                format!("{}...", text.chars().take(60).collect::<String>())
            } else {
                text
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    (format!("{:x}", digest.finalize()), preview)
}

/// One recorded host operation.
struct Entry {
    /// The `parallel_map` item this operation ran in; empty for the main program.
    lane: String,
    arguments_sha256: String,
    arguments: String,
    operation: String,
    function: String,
    line: usize,
    column: usize,
    /// How many stack values below the operation's arguments the operation left alone.
    kept: usize,
    /// The stack height the operation started with.
    height: usize,
    outcome: Outcome,
}

enum Outcome {
    /// The call touched something the project asked to keep off tapes; only its place is kept.
    Redacted,
    Values(Vec<Json>),
    Error {
        code: String,
        message: String,
    },
}

enum TapeState {
    Recording {
        path: PathBuf,
        entries: Vec<Entry>,
        incomplete: Vec<String>,
        program_sha256: String,
        redact: Vec<String>,
    },
    Replaying {
        lanes: HashMap<String, Lane>,
        /// Lanes whose recorded run was cut short by a sibling's failure.
        incomplete: HashSet<String>,
        /// The first divergence inside a `parallel_map` item, which the program sees only as an
        /// `Err` result, kept so the replay still ends with it.
        diverged: Option<RuntimeError>,
    },
}

struct Lane {
    entries: Vec<Entry>,
    next: usize,
}

pub(crate) struct Tape {
    state: Mutex<TapeState>,
}

impl std::fmt::Debug for Tape {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Tape")
    }
}

/// Hash of the program a tape belongs to.
pub(crate) fn program_hash(artifact: &guion_model::Artifact) -> String {
    let mut stripped = artifact.clone();
    stripped.build_metadata = None;
    stripped.compiled_source_sha256 = None;
    let bytes = stripped.to_bytes().unwrap_or_default();
    format!("{:x}", Sha256::digest(&bytes))
}

impl Tape {
    pub(crate) fn open(
        mode: &TapeMode,
        artifact: &guion_model::Artifact,
        redact: &[String],
    ) -> Result<Tape, RuntimeError> {
        let fail = |message: String| RuntimeError::new("R_TAPE", message, Span::default());
        let hash = program_hash(artifact);
        let state = match mode {
            TapeMode::Record(path) => TapeState::Recording {
                path: path.clone(),
                entries: Vec::new(),
                incomplete: Vec::new(),
                program_sha256: hash,
                redact: redact.to_vec(),
            },
            TapeMode::Replay(path) | TapeMode::ReplayChanged(path) => {
                let text = std::fs::read_to_string(path).map_err(|error| {
                    fail(format!("cannot read tape {}: {error}", path.display()))
                })?;
                let mut lines = text.lines();
                let header: Json = serde_json::from_str(lines.next().unwrap_or(""))
                    .map_err(|error| fail(format!("tape header is not valid: {error}")))?;
                let version = header["guion_tape"].as_u64();
                if version != Some(TAPE_VERSION) && version != Some(TAPE_VERSION_LANES) {
                    return Err(fail(
                        "this tape was written by a different tape format".into(),
                    ));
                }
                if !matches!(mode, TapeMode::ReplayChanged(_))
                    && header["program_sha256"].as_str() != Some(hash.as_str())
                {
                    return Err(fail(
                        "the program has changed since this tape was recorded (program hash differs); record a new tape, or use --replay-changed to find the first call that differs".into(),
                    ));
                }
                let mut lanes: HashMap<String, Lane> = HashMap::new();
                let mut incomplete = HashSet::new();
                for line in lines.filter(|line| !line.trim().is_empty()) {
                    let entry: Json = serde_json::from_str(line)
                        .map_err(|error| fail(format!("tape entry is not valid: {error}")))?;
                    if entry.get("incomplete_lane").is_some() {
                        incomplete
                            .insert(entry["incomplete_lane"].as_str().unwrap_or("").to_owned());
                        continue;
                    }
                    let lane = entry["lane"].as_str().unwrap_or("").to_owned();
                    let outcome = if entry.get("redacted").is_some() {
                        Outcome::Redacted
                    } else if let Some(error) = entry.get("error") {
                        Outcome::Error {
                            code: error["code"].as_str().unwrap_or("R_TAPE").to_owned(),
                            message: error["message"].as_str().unwrap_or("").to_owned(),
                        }
                    } else {
                        Outcome::Values(entry["tail"].as_array().cloned().unwrap_or_default())
                    };
                    lanes
                        .entry(lane.clone())
                        .or_insert_with(|| Lane {
                            entries: Vec::new(),
                            next: 0,
                        })
                        .entries
                        .push(Entry {
                            lane,
                            arguments_sha256: entry["args_sha256"]
                                .as_str()
                                .unwrap_or("")
                                .to_owned(),
                            arguments: entry["args"].as_str().unwrap_or("").to_owned(),
                            operation: entry["op"].as_str().unwrap_or("").to_owned(),
                            function: entry["function"].as_str().unwrap_or("").to_owned(),
                            line: entry["line"].as_u64().unwrap_or(0) as usize,
                            column: entry["column"].as_u64().unwrap_or(0) as usize,
                            kept: entry["kept"].as_u64().unwrap_or(0) as usize,
                            height: entry["height"].as_u64().unwrap_or(0) as usize,
                            outcome,
                        });
                }
                TapeState::Replaying {
                    lanes,
                    incomplete,
                    diverged: None,
                }
            }
        };
        Ok(Tape {
            state: Mutex::new(state),
        })
    }

    pub(crate) fn is_replay(&self) -> bool {
        matches!(*self.state.lock().unwrap(), TapeState::Replaying { .. })
    }

    /// Records one operation: its name and place, the stack height it started with, how much of
    /// the stack it left alone, and what it left above that (or its error).
    pub(crate) fn record(
        &self,
        lane: &str,
        operation: &str,
        function: &str,
        span: Span,
        before: &[Value],
        after: &Result<&[Value], &RuntimeError>,
    ) -> Result<(), RuntimeError> {
        let mut state = self.state.lock().unwrap();
        let TapeState::Recording {
            entries, redact, ..
        } = &mut *state
        else {
            return Ok(());
        };
        // A call whose arguments mention a redacted host or path keeps its place on the tape
        // but not its result.
        let arguments_text: String = before
            .iter()
            .map(|value| value.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        let redacted = redact
            .iter()
            .any(|pattern| !pattern.is_empty() && arguments_text.contains(pattern.as_str()));
        let (kept, outcome) = if redacted {
            (before.len(), Outcome::Redacted)
        } else {
            match after {
                Ok(after) => {
                    let kept = before
                        .iter()
                        .zip(after.iter())
                        .take_while(|(a, b)| a == b)
                        .count();
                    let tail = after[kept..]
                        .iter()
                        .map(encode_value)
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|message| RuntimeError::new("R_TAPE", message, span))?;
                    (kept, Outcome::Values(tail))
                }
                Err(error) => (
                    before.len(),
                    Outcome::Error {
                        code: error.code.clone(),
                        message: error.message.clone(),
                    },
                ),
            }
        };
        let (arguments_sha256, arguments) = stack_fingerprint(before);
        entries.push(Entry {
            lane: lane.to_owned(),
            arguments_sha256,
            arguments,
            operation: operation.to_owned(),
            function: function.to_owned(),
            line: span.line,
            column: span.column,
            kept,
            height: before.len(),
            outcome,
        });
        Ok(())
    }

    /// Takes the next recorded operation, checking it is the one the program is making. Returns
    /// how many stack values to keep and the values to push, or the recorded error.
    pub(crate) fn replay(
        &self,
        lane: &str,
        operation: &str,
        function: &str,
        span: Span,
        stack: &[Value],
    ) -> Result<Result<(usize, Vec<Value>), RuntimeError>, RuntimeError> {
        let height = stack.len();
        let mut state = self.state.lock().unwrap();
        let TapeState::Replaying {
            lanes,
            incomplete,
            diverged: first_divergence,
        } = &mut *state
        else {
            return Err(RuntimeError::new(
                "R_TAPE",
                "tape is not in replay mode",
                span,
            ));
        };
        let place = lane_label(lane);
        let cut_short = incomplete.contains(lane);
        let mut diverged = |message: String| {
            let error = RuntimeError::new("R_TAPE_DIVERGED", message, span);
            if !lane.is_empty() && first_divergence.is_none() {
                *first_divergence = Some(error.clone());
            }
            error
        };
        let Some(recorded) = lanes.get_mut(lane) else {
            if cut_short {
                return Err(RuntimeError::new(
                    "R_CANCELLED",
                    "parallel task was cancelled after a sibling failed",
                    span,
                ));
            }
            return Err(diverged(format!(
                "replay diverged{place}: the program called {operation} in `{function}` at line {}, but the tape has no calls for this part of the program",
                span.line
            )));
        };
        let index = recorded.next;
        let Some(entry) = recorded.entries.get(index) else {
            if cut_short {
                // The recorded run of this item stopped here because a sibling failed; the
                // replayed one stops at the same place.
                return Err(RuntimeError::new(
                    "R_CANCELLED",
                    "parallel task was cancelled after a sibling failed",
                    span,
                ));
            }
            return Err(diverged(format!(
                "replay diverged{place} at host call {}: the program called {operation} in `{function}` at line {}, but the tape has no more calls",
                index + 1,
                span.line
            )));
        };
        if entry.operation != operation
            || entry.line != span.line
            || entry.column != span.column
            || entry.height != height
        {
            return Err(diverged(format!(
                "replay diverged{place} at host call {}: the program called {operation} in `{function}` at line {}, but the tape recorded {} in `{}` at line {}",
                index + 1,
                span.line,
                entry.operation,
                entry.function,
                entry.line
            )));
        }
        let (fingerprint, preview) = stack_fingerprint(stack);
        if entry.arguments_sha256 != fingerprint {
            return Err(diverged(format!(
                "replay diverged{place} at host call {}: the program called {operation} in `{function}` at line {} with ({preview}), but the tape recorded it with ({})",
                index + 1,
                span.line,
                entry.arguments
            )));
        }
        recorded.next += 1;
        let entry = &recorded.entries[index];
        Ok(match &entry.outcome {
            Outcome::Redacted => Err(RuntimeError::new(
                "R_TAPE_REDACTED",
                format!(
                    "the result of {operation} at line {} was redacted from this tape ([record] redact in guion.toml), so a replay cannot supply it",
                    span.line
                ),
                span,
            )),
            Outcome::Error { code, message } => Err(RuntimeError::new(code.clone(), message.clone(), span)),
            Outcome::Values(tail) => {
                let values = tail
                    .iter()
                    .map(decode_value)
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|message| RuntimeError::new("R_TAPE", message, span))?;
                Ok((entry.kept, values))
            }
        })
    }

    /// Notes that a `parallel_map` item did not run to its end (a sibling failed first), so a
    /// replay of that item may stop where the recording did.
    ///
    /// While replaying it excuses what is left of a lane the replay itself cut short: a failing
    /// sibling can stop an item before it starts or part-way, whatever the recorded timing was.
    /// While replaying: whether the recording stopped this lane early (a failing sibling, or a
    /// deadline). Always false when recording.
    pub(crate) fn lane_cut_short(&self, lane: &str) -> bool {
        match &*self.state.lock().unwrap() {
            TapeState::Replaying { incomplete, .. } => incomplete.contains(lane),
            _ => false,
        }
    }

    pub(crate) fn mark_lane_incomplete(&self, lane: &str) {
        match &mut *self.state.lock().unwrap() {
            TapeState::Recording { incomplete, .. } => incomplete.push(lane.to_owned()),
            TapeState::Replaying { incomplete, .. } => {
                incomplete.insert(lane.to_owned());
            }
        }
    }

    /// At the end of a replayed run: a divergence inside a `parallel_map` item that the program
    /// only saw as an `Err`, then calls on the tape the program never made.
    pub(crate) fn finish_replay(&self) -> Result<(), RuntimeError> {
        let state = self.state.lock().unwrap();
        if let TapeState::Replaying {
            lanes,
            incomplete,
            diverged,
        } = &*state
        {
            if let Some(error) = diverged {
                return Err(error.clone());
            }
            let mut names: Vec<&String> = lanes.keys().collect();
            names.sort_by_key(|name| lane_key(name));
            for name in names {
                let lane = &lanes[name];
                if lane.next < lane.entries.len() && !incomplete.contains(name) {
                    let entry = &lane.entries[lane.next];
                    return Err(RuntimeError::new(
                        "R_TAPE_DIVERGED",
                        format!(
                            "replay diverged{}: the program finished after {} host calls, but the tape also has {} in `{}` at line {}",
                            lane_label(name),
                            lane.next,
                            entry.operation,
                            entry.function,
                            entry.line
                        ),
                        Span::default(),
                    ));
                }
            }
        }
        Ok(())
    }

    /// Writes the tape file when recording; does nothing when replaying.
    pub(crate) fn finish_record(
        &self,
        answered: &[crate::GrantAnswerRecord],
    ) -> Result<(), String> {
        let state = self.state.lock().unwrap();
        let TapeState::Recording {
            path,
            entries,
            incomplete,
            program_sha256,
            ..
        } = &*state
        else {
            return Ok(());
        };
        let has_lanes = entries.iter().any(|entry| !entry.lane.is_empty());
        let version = if has_lanes {
            TAPE_VERSION_LANES
        } else {
            TAPE_VERSION
        };
        let mut header = json!({"guion_tape": version, "program_sha256": program_sha256});
        // Grants a person gave at a question, so a reader can tell "granted in guion.toml" from
        // "granted by a person at the prompt". A replay ignores this; it gets no access at all.
        if !answered.is_empty() {
            header["grants_from_question"] = Json::Array(
                answered
                    .iter()
                    .map(|answer| {
                        json!({
                            "effect": answer.kind.name(),
                            "target": answer.target,
                            "always": answer.always,
                        })
                    })
                    .collect(),
            );
        }
        let mut text = header.to_string();
        text.push('\n');
        // The main program first, then each parallel_map item in call and item order; within a
        // lane the order the operations ran in.
        let mut ordered: Vec<&Entry> = entries.iter().collect();
        ordered.sort_by_cached_key(|entry| lane_key(&entry.lane));
        let mut marks: Vec<&String> = incomplete.iter().collect();
        marks.sort_by_key(|name| lane_key(name));
        for name in marks {
            text.push_str(&json!({"incomplete_lane": name}).to_string());
            text.push('\n');
        }
        for entry in ordered {
            let mut line = json!({
                "op": entry.operation, "function": entry.function,
                "line": entry.line, "column": entry.column,
                "kept": entry.kept, "height": entry.height,
                "args_sha256": entry.arguments_sha256, "args": entry.arguments,
            });
            if !entry.lane.is_empty() {
                line["lane"] = json!(entry.lane);
            }
            match &entry.outcome {
                Outcome::Redacted => line["redacted"] = json!(true),
                Outcome::Values(tail) => line["tail"] = Json::Array(tail.clone()),
                Outcome::Error { code, message } => {
                    line["error"] = json!({"code": code, "message": message})
                }
            }
            text.push_str(&line.to_string());
            text.push('\n');
        }
        std::fs::write(path, text)
            .map_err(|error| format!("cannot write tape {}: {error}", path.display()))
    }
}

/// The operations that reach outside the program. Printing is output, not input, and is left
/// to run in both modes.
pub(crate) fn is_world_operation(op: &guion_model::Op) -> bool {
    use guion_model::Op;
    matches!(
        op,
        Op::ReadText
            | Op::ReadBytes
            | Op::WriteText
            | Op::WriteTextAtomic
            | Op::WriteBytes
            | Op::FsWalk
            | Op::FileOpenLines
            | Op::FileNextLine
            | Op::FileClose
            | Op::CsvOpen
            | Op::CsvNext
            | Op::CsvReadColumns
            | Op::Fs(_)
            | Op::Net(_)
            | Op::Tls(_)
            | Op::Process(_)
            | Op::TaskWaitWithin
            | Op::ChannelSend
            | Op::ChannelReceive
            | Op::FirstReady(_)
            | Op::FirstValue(_)
            | Op::Args
            | Op::ArgsFrom
            | Op::Environment
            | Op::SqliteOpen
            | Op::SqliteBegin
            | Op::SqliteBeginRead
            | Op::SqliteExecute
            | Op::SqliteQuery
            | Op::SqliteCommit
            | Op::SqliteRollback
            | Op::SqliteClose
            | Op::LogAppend
            | Op::UnixTimeMs
            | Op::TimeTodayLocal
            | Op::HttpGet
            | Op::HttpRequest
            | Op::HttpRequestWithTimeout
            | Op::HttpRequestBytes
            | Op::RandomBytes
    )
}
