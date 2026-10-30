use super::CancellationToken;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use guion_model::{BigInt, Float64, SharedJson, Type, Value};
use serde::{
    ser::{SerializeMap, SerializeSeq},
    Serialize, Serializer,
};
use serde_json::Value as JsonValue;
use std::{
    cell::RefCell,
    collections::BTreeMap,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        mpsc::{self, Receiver},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

const MAX_FRAME: usize = 24 * 1024 * 1024;
const PROTOCOL_VERSION: u64 = 2;
const BINARY_FRAME_MAGIC: &[u8; 4] = b"GBIN";
const SIDECAR: &str = include_str!("../python/sidecar.py");

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PythonBridgeError {
    pub code: String,
    pub message: String,
    pub python_type: Option<String>,
    pub traceback: Option<Box<str>>,
    pub stdout: Box<str>,
    pub stderr: Box<str>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PythonCallOutput {
    pub value: Value,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug)]
pub struct PythonBridge {
    child: Child,
    input: ChildStdin,
    responses: Receiver<Result<ReaderFrame, String>>,
    request_id: u64,
    timeout: Duration,
    closed: bool,
    native_image_observation: String,
    native_images: Vec<String>,
    profile_path: Option<PathBuf>,
    profile: Option<BridgeProfile>,
}

#[derive(Debug)]
struct ReaderFrame {
    bytes: Vec<u8>,
    profile: Option<ReaderFrameProfile>,
}

#[derive(Debug)]
struct ReaderFrameProfile {
    first_byte_at: Instant,
    after_first_byte: Duration,
    queued_at: Instant,
}

#[derive(Debug, Default)]
struct BridgeProfile {
    calls: u64,
    first_call_ns: BTreeMap<String, u64>,
    repeated_calls_ns: BTreeMap<String, u64>,
    python_calls: u64,
    python_request_json_parser: Option<String>,
    python_response_json_encoder: Option<String>,
    python_first_call_ns: BTreeMap<String, u64>,
    python_repeated_calls_ns: BTreeMap<String, u64>,
    active_first_call: bool,
}

impl BridgeProfile {
    fn add(&mut self, phase: &str, elapsed: Duration) {
        let nanos = u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX);
        let phases = if self.active_first_call {
            &mut self.first_call_ns
        } else {
            &mut self.repeated_calls_ns
        };
        let total = phases.entry(phase.to_owned()).or_default();
        *total = total.saturating_add(nanos);
    }

    fn finish_call(&mut self) {
        self.calls = self.calls.saturating_add(1);
    }
}

struct BridgeRequest<'data, 'attachments> {
    id: u64,
    module: &'data str,
    function: &'data str,
    args: &'data [Value],
    kwargs: &'data [(String, Value)],
    json_result: bool,
    numpy_list_result: bool,
    attachments: &'attachments RefCell<Vec<&'data [u8]>>,
}

impl Serialize for BridgeRequest<'_, '_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut request = serializer.serialize_map(Some(9))?;
        request.serialize_entry("protocol", &PROTOCOL_VERSION)?;
        request.serialize_entry("id", &self.id)?;
        request.serialize_entry("kind", "call")?;
        request.serialize_entry("module", self.module)?;
        request.serialize_entry("function", self.function)?;
        request.serialize_entry(
            "args",
            &WireValues {
                values: self.args,
                attachments: self.attachments,
            },
        )?;
        request.serialize_entry(
            "kwargs",
            &WireKeywords {
                values: self.kwargs,
                attachments: self.attachments,
            },
        )?;
        request.serialize_entry("json_result", &self.json_result)?;
        request.serialize_entry("numpy_list_result", &self.numpy_list_result)?;
        request.end()
    }
}

struct WireValue<'data, 'attachments> {
    value: &'data Value,
    attachments: &'attachments RefCell<Vec<&'data [u8]>>,
}

impl Serialize for WireValue<'_, '_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut value = serializer.serialize_map(Some(2))?;
        match self.value {
            Value::Unit => value.serialize_entry("t", "unit")?,
            Value::Bool(item) => {
                value.serialize_entry("t", "bool")?;
                value.serialize_entry("v", item)?;
            }
            Value::Int(item) => {
                value.serialize_entry("t", "int")?;
                value.serialize_entry("v", &item.to_string())?;
            }
            Value::Float64(item) => {
                value.serialize_entry("t", "f64")?;
                value.serialize_entry("v", &item.get().to_string())?;
            }
            Value::Text(item) => {
                value.serialize_entry("t", "text")?;
                value.serialize_entry("v", item.as_ref())?;
            }
            Value::Bytes(item) => {
                let index = {
                    let mut attachments = self.attachments.borrow_mut();
                    let index = attachments.len();
                    attachments.push(item.as_slice());
                    index
                };
                value.serialize_entry("t", "bytes")?;
                value.serialize_entry("v", &index)?;
            }
            Value::List(items) => {
                value.serialize_entry("t", "list")?;
                value.serialize_entry(
                    "v",
                    &WireValues {
                        values: items,
                        attachments: self.attachments,
                    },
                )?;
            }
            Value::Float64List(items) => {
                value.serialize_entry("t", "list")?;
                value.serialize_entry("v", &WireFloat64Values(items))?;
            }
            Value::Map(items) => {
                value.serialize_entry("t", "map")?;
                value.serialize_entry(
                    "v",
                    &WireMap {
                        values: items,
                        attachments: self.attachments,
                    },
                )?;
            }
            Value::Json(item) => {
                value.serialize_entry("t", "json")?;
                value.serialize_entry("v", item.as_ref())?;
            }
            Value::OptionNone
            | Value::OptionSome(_)
            | Value::ResultOk(_)
            | Value::ResultErr(_)
            | Value::TaskError(_)
            | Value::JsonError(_)
            | Value::FileError(_)
            | Value::HttpError(_)
            | Value::NativeResource(_)
            | Value::Record(_, _) => {
                return Err(serde::ser::Error::custom(
                    "unsupported Guion value in Python bridge request",
                ));
            }
        }
        value.end()
    }
}

struct WireValues<'data, 'attachments> {
    values: &'data [Value],
    attachments: &'attachments RefCell<Vec<&'data [u8]>>,
}

impl Serialize for WireValues<'_, '_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut sequence = serializer.serialize_seq(Some(self.values.len()))?;
        for value in self.values {
            sequence.serialize_element(&WireValue {
                value,
                attachments: self.attachments,
            })?;
        }
        sequence.end()
    }
}

struct WireFloat64Values<'a>(&'a [Float64]);

impl Serialize for WireFloat64Values<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for value in self.0 {
            sequence.serialize_element(&WireFloat64Value(*value))?;
        }
        sequence.end()
    }
}

struct WireFloat64Value(Float64);

impl Serialize for WireFloat64Value {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut value = serializer.serialize_map(Some(2))?;
        value.serialize_entry("t", "f64")?;
        value.serialize_entry("v", &self.0.get().to_string())?;
        value.end()
    }
}

struct WireMap<'data, 'attachments> {
    values: &'data std::collections::BTreeMap<String, Value>,
    attachments: &'attachments RefCell<Vec<&'data [u8]>>,
}

impl Serialize for WireMap<'_, '_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(self.values.len()))?;
        for (key, value) in self.values.iter() {
            map.serialize_entry(
                key,
                &WireValue {
                    value,
                    attachments: self.attachments,
                },
            )?;
        }
        map.end()
    }
}

struct WireKeywords<'data, 'attachments> {
    values: &'data [(String, Value)],
    attachments: &'attachments RefCell<Vec<&'data [u8]>>,
}

impl Serialize for WireKeywords<'_, '_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(self.values.len()))?;
        let mut entries = self.values.iter().collect::<Vec<_>>();
        entries.sort_unstable_by(|(left, _), (right, _)| left.cmp(right));
        for (key, value) in entries {
            map.serialize_entry(
                key,
                &WireValue {
                    value,
                    attachments: self.attachments,
                },
            )?;
        }
        map.end()
    }
}

impl PythonBridge {
    /// Launch an explicitly selected interpreter. This API never searches PATH.
    pub fn launch(
        interpreter: &Path,
        working_directory: &Path,
        timeout: Duration,
    ) -> Result<Self, PythonBridgeError> {
        Self::launch_with_project_root(interpreter, working_directory, working_directory, timeout)
    }

    /// Launch the bridge with a project import root and an independent process working directory.
    pub fn launch_with_project_root(
        interpreter: &Path,
        project_root: &Path,
        working_directory: &Path,
        timeout: Duration,
    ) -> Result<Self, PythonBridgeError> {
        let profile_path = std::env::var_os("GUION_PYTHON_BRIDGE_PROFILE_OUT").map(|value| {
            let path = PathBuf::from(value);
            if path.is_absolute() {
                path
            } else {
                std::env::current_dir()
                    .map(|directory| directory.join(&path))
                    .unwrap_or(path)
            }
        });
        if let Some(parent) = profile_path.as_ref().and_then(|path| path.parent()) {
            let _ = std::fs::create_dir_all(parent);
        }

        let mut command = Command::new(interpreter);
        command
            .arg("-I")
            .arg("-B")
            .arg("-u")
            .arg("-c")
            .arg("import sys; sys.path.insert(0, sys.argv.pop(1)); exec(sys.argv.pop(1))")
            .arg(project_root)
            .arg(SIDECAR)
            .current_dir(working_directory)
            .env("PYTHONIOENCODING", "utf-8")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if profile_path.is_some() {
            command.env("GUION_PYTHON_BRIDGE_PROFILE", "1");
        } else {
            command.env_remove("GUION_PYTHON_BRIDGE_PROFILE");
        }
        let mut child = command
            .spawn()
            .map_err(|error| bridge_error("R_PYTHON_START", error.to_string()))?;
        let input = child
            .stdin
            .take()
            .ok_or_else(|| bridge_error("R_PYTHON_START", "cannot open sidecar input"))?;
        let mut output = child
            .stdout
            .take()
            .ok_or_else(|| bridge_error("R_PYTHON_START", "cannot open sidecar output"))?;
        if let Some(mut stderr) = child.stderr.take() {
            thread::spawn(move || {
                let _ = io::copy(&mut stderr, &mut io::sink());
            });
        }
        let (sender, responses) = mpsc::channel();
        let profile_enabled = profile_path.is_some();
        thread::spawn(move || loop {
            match read_frame(&mut output, profile_enabled) {
                Ok(Some(frame)) => {
                    if sender.send(Ok(frame)).is_err() {
                        break;
                    }
                }
                Ok(None) => {
                    let _ = sender.send(Err("Python sidecar closed its protocol stream".into()));
                    break;
                }
                Err(error) => {
                    let _ = sender.send(Err(error));
                    break;
                }
            }
        });
        Ok(Self {
            child,
            input,
            responses,
            request_id: 0,
            timeout,
            closed: false,
            native_image_observation: "not_used".into(),
            native_images: Vec::new(),
            profile: profile_path.as_ref().map(|_| BridgeProfile::default()),
            profile_path,
        })
    }

    pub fn call(
        &mut self,
        module: &str,
        function: &str,
        args: &[Value],
        kwargs: &[(String, Value)],
        result_type: &Type,
    ) -> Result<PythonCallOutput, PythonBridgeError> {
        self.call_with_cancellation(
            module,
            function,
            args,
            kwargs,
            result_type,
            &CancellationToken::new(),
        )
    }

    pub fn call_with_cancellation(
        &mut self,
        module: &str,
        function: &str,
        args: &[Value],
        kwargs: &[(String, Value)],
        result_type: &Type,
        cancellation: &CancellationToken,
    ) -> Result<PythonCallOutput, PythonBridgeError> {
        if self.closed {
            return Err(bridge_error("R_PYTHON_CLOSED", "Python sidecar is closed"));
        }
        if cancellation.is_cancelled() {
            return Err(bridge_error("R_CANCELLED", "invocation was cancelled"));
        }
        self.request_id = self.request_id.checked_add(1).ok_or_else(|| {
            bridge_error("R_PYTHON_PROTOCOL", "Python bridge request ID exhausted")
        })?;
        let id = self.request_id;
        for value in args {
            validate_value(value, 0)?;
        }
        let mut keyword_names = std::collections::BTreeSet::new();
        for (name, value) in kwargs {
            validate_value(value, 0)?;
            if !keyword_names.insert(name) {
                return Err(bridge_error(
                    "R_PYTHON_ARGUMENT",
                    format!("duplicate Python keyword argument `{name}`"),
                ));
            }
        }
        if let Some(profile) = self.profile.as_mut() {
            profile.active_first_call = profile.calls == 0;
        }
        write_request_frame(
            &mut self.input,
            id,
            module,
            function,
            args,
            kwargs,
            matches!(result_type, Type::Json),
            matches!(result_type, Type::List(_)),
            self.profile.as_mut(),
        )?;
        let deadline = Instant::now() + self.timeout;
        let response_wait_started = self.profile.as_ref().map(|_| Instant::now());
        let response = loop {
            if cancellation.is_cancelled() {
                self.terminate();
                return Err(bridge_error("R_CANCELLED", "invocation was cancelled"));
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                self.terminate();
                return Err(bridge_error(
                    "R_PYTHON_TIMEOUT",
                    format!(
                        "Python call `{module}:{function}` exceeded {:?}",
                        self.timeout
                    ),
                ));
            }
            match self
                .responses
                .recv_timeout(remaining.min(Duration::from_millis(20)))
            {
                Ok(Ok(response)) => break response,
                Ok(Err(message)) => {
                    self.terminate();
                    return Err(bridge_error("R_PYTHON_PROTOCOL", message));
                }
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    self.terminate();
                    return Err(bridge_error(
                        "R_PYTHON_CRASH",
                        "Python sidecar response channel closed",
                    ));
                }
            }
        };
        if let (Some(profile), Some(reader_profile)) =
            (self.profile.as_mut(), response.profile.as_ref())
        {
            if let Some(started) = response_wait_started {
                profile.add(
                    "response_first_byte_wait",
                    reader_profile
                        .first_byte_at
                        .saturating_duration_since(started),
                );
            }
            profile.add(
                "reader_frame_after_first_byte",
                reader_profile.after_first_byte,
            );
            profile.add("channel_delivery", reader_profile.queued_at.elapsed());
        }
        if let (Some(profile), Some(started)) = (self.profile.as_mut(), response_wait_started) {
            profile.add("response_wait", started.elapsed());
        }
        let response_parse_started = self.profile.as_ref().map(|_| Instant::now());
        let response: JsonValue = serde_json::from_slice(&response.bytes).map_err(|error| {
            self.terminate();
            bridge_error(
                "R_PYTHON_PROTOCOL",
                format!("invalid sidecar response JSON: {error}"),
            )
        })?;
        if let (Some(profile), Some(started)) = (self.profile.as_mut(), response_parse_started) {
            profile.add("response_json_parse", started.elapsed());
        }
        if let Some(profile) = self.profile.as_mut() {
            if let Some(sidecar_profile) = response.get("_bridge_profile") {
                profile.python_calls = sidecar_profile
                    .get("calls")
                    .and_then(JsonValue::as_u64)
                    .unwrap_or(profile.python_calls);
                profile.python_response_json_encoder = sidecar_profile
                    .get("response_json_encoder")
                    .and_then(JsonValue::as_str)
                    .map(str::to_owned);
                profile.python_request_json_parser = sidecar_profile
                    .get("request_json_parser")
                    .and_then(JsonValue::as_str)
                    .map(str::to_owned);
                if let Some(phases) = sidecar_profile
                    .get("first_call_ns")
                    .and_then(JsonValue::as_object)
                {
                    profile.python_first_call_ns = phases
                        .iter()
                        .filter_map(|(name, duration)| {
                            duration.as_u64().map(|duration| (name.clone(), duration))
                        })
                        .collect();
                }
                if let Some(phases) = sidecar_profile
                    .get("repeated_calls_ns")
                    .and_then(JsonValue::as_object)
                {
                    profile.python_repeated_calls_ns = phases
                        .iter()
                        .filter_map(|(name, duration)| {
                            duration.as_u64().map(|duration| (name.clone(), duration))
                        })
                        .collect();
                }
            }
        }
        if response.get("protocol").and_then(JsonValue::as_u64) != Some(PROTOCOL_VERSION)
            || response.get("id").and_then(JsonValue::as_u64) != Some(id)
        {
            self.terminate();
            return Err(bridge_error(
                "R_PYTHON_PROTOCOL",
                "sidecar response version or request ID mismatch",
            ));
        }
        let stdout = response
            .get("stdout")
            .and_then(JsonValue::as_str)
            .unwrap_or("")
            .to_owned();
        let stderr = response
            .get("stderr")
            .and_then(JsonValue::as_str)
            .unwrap_or("")
            .to_owned();
        let native_image_observation = response
            .get("native_image_observation")
            .and_then(JsonValue::as_str)
            .unwrap_or("unsupported")
            .to_owned();
        let native_images = response
            .get("native_images")
            .and_then(JsonValue::as_array)
            .into_iter()
            .flatten()
            .filter_map(JsonValue::as_str)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        self.native_image_observation = native_image_observation;
        for image in native_images {
            if !self.native_images.contains(&image) {
                self.native_images.push(image);
            }
        }
        if response.get("ok").and_then(JsonValue::as_bool) != Some(true) {
            if let Some(profile) = self.profile.as_mut() {
                profile.finish_call();
            }
            self.write_profile();
            let error = response.get("error").unwrap_or(&JsonValue::Null);
            return Err(PythonBridgeError {
                code: "R_PYTHON_CALL".into(),
                message: error
                    .get("message")
                    .and_then(JsonValue::as_str)
                    .unwrap_or("Python call failed")
                    .to_owned(),
                python_type: error
                    .get("type")
                    .and_then(JsonValue::as_str)
                    .map(str::to_owned),
                traceback: error
                    .get("traceback")
                    .and_then(JsonValue::as_str)
                    .map(|text| text.to_owned().into_boxed_str()),
                stdout: stdout.into_boxed_str(),
                stderr: stderr.into_boxed_str(),
            });
        }
        let value = response
            .get("value")
            .ok_or_else(|| bridge_error("R_PYTHON_PROTOCOL", "successful response has no value"))?;
        let result_decode_started = self.profile.as_ref().map(|_| Instant::now());
        let value = decode_value(value, result_type)?;
        if let (Some(profile), Some(started)) = (self.profile.as_mut(), result_decode_started) {
            profile.add("result_decode", started.elapsed());
        }
        if let Some(profile) = self.profile.as_mut() {
            profile.finish_call();
        }
        self.write_profile();
        Ok(PythonCallOutput {
            value,
            stdout,
            stderr,
        })
    }

    pub fn close(&mut self) {
        self.terminate();
    }

    pub fn native_image_observation(&self) -> &str {
        &self.native_image_observation
    }

    pub fn native_images(&self) -> &[String] {
        &self.native_images
    }

    fn terminate(&mut self) {
        if !self.closed {
            let _ = self.child.kill();
            let _ = self.child.wait();
            self.closed = true;
        }
    }

    fn write_profile(&self) {
        let Some(path) = &self.profile_path else {
            return;
        };
        let Some(profile) = &self.profile else {
            return;
        };
        let report = serde_json::json!({
            "schema_version": 1,
            "kind": "guion-python-bridge-phase-profile",
            "calls": profile.calls,
            "rust_first_call_ns": profile.first_call_ns,
            "rust_repeated_calls_ns": profile.repeated_calls_ns,
            "python": {
                "calls": profile.python_calls,
                "request_json_parser": profile.python_request_json_parser,
                "response_json_encoder": profile.python_response_json_encoder,
                "first_call_ns": profile.python_first_call_ns,
                "repeated_calls_ns": profile.python_repeated_calls_ns,
            },
        });
        if let Ok(encoded) = serde_json::to_vec_pretty(&report) {
            let _ = std::fs::write(path, encoded);
        }
    }
}

impl Drop for PythonBridge {
    fn drop(&mut self) {
        self.terminate();
    }
}

fn bridge_error(code: &str, message: impl Into<String>) -> PythonBridgeError {
    PythonBridgeError {
        code: code.into(),
        message: message.into(),
        python_type: None,
        traceback: None,
        stdout: String::new().into_boxed_str(),
        stderr: String::new().into_boxed_str(),
    }
}

fn validate_value(value: &Value, depth: usize) -> Result<(), PythonBridgeError> {
    if depth > 64 {
        return Err(bridge_error(
            "R_PYTHON_CONVERSION",
            "value nesting exceeds 64 levels",
        ));
    }
    match value {
        Value::Unit
        | Value::Bool(_)
        | Value::Int(_)
        | Value::Float64(_)
        | Value::Text(_)
        | Value::Bytes(_) => Ok(()),
        Value::List(values) => values
            .iter()
            .try_for_each(|item| validate_value(item, depth + 1)),
        Value::Float64List(values) => {
            if !values.is_empty() && depth + 1 > 64 {
                Err(bridge_error(
                    "R_PYTHON_CONVERSION",
                    "value nesting exceeds 64 levels",
                ))
            } else {
                Ok(())
            }
        }
        Value::Map(values) => values
            .values()
            .try_for_each(|item| validate_value(item, depth + 1)),
        Value::Json(value) => validate_shared_json_depth(value, depth + 1),
        Value::OptionNone
        | Value::OptionSome(_)
        | Value::ResultOk(_)
        | Value::ResultErr(_)
        | Value::TaskError(_)
        | Value::JsonError(_)
        | Value::FileError(_)
        | Value::HttpError(_)
        | Value::NativeResource(_)
        | Value::Record(_, _) => Err(bridge_error(
            "R_PYTHON_CONVERSION",
            "Option, Result, structured errors, and native resources cannot cross the current Python bridge",
        )),
    }
}

fn validate_shared_json_depth(value: &SharedJson, depth: usize) -> Result<(), PythonBridgeError> {
    if depth > 64 {
        return Err(bridge_error(
            "R_PYTHON_CONVERSION",
            "JSON nesting exceeds 64 levels",
        ));
    }
    match value {
        SharedJson::Array(items) => items
            .iter()
            .try_for_each(|item| validate_shared_json_depth(item, depth + 1)),
        SharedJson::Object(fields) => fields
            .values()
            .try_for_each(|item| validate_shared_json_depth(item, depth + 1)),
        SharedJson::Null | SharedJson::Bool(_) | SharedJson::Number(_) | SharedJson::String(_) => {
            Ok(())
        }
    }
}

fn decode_value(value: &JsonValue, expected: &Type) -> Result<Value, PythonBridgeError> {
    let tag = value.get("t").and_then(JsonValue::as_str).unwrap_or("");
    let payload = value.get("v").unwrap_or(&JsonValue::Null);
    let invalid = || {
        bridge_error(
            "R_PYTHON_RETURN_TYPE",
            format!("Python result does not match declared {expected:?}"),
        )
    };
    match (tag, expected) {
        ("unit", Type::Unit) => Ok(Value::Unit),
        ("json", Type::Json) => {
            validate_json_depth(payload, 0)?;
            Ok(Value::Json(guion_model::SharedJson::from_serde(
                payload.clone(),
            )))
        }
        ("bool", Type::Bool) => payload.as_bool().map(Value::Bool).ok_or_else(invalid),
        ("int", Type::Int) => payload
            .as_str()
            .and_then(|text| BigInt::parse(text).ok())
            .map(Value::Int)
            .ok_or_else(invalid),
        ("f64", Type::Float64) => payload
            .as_str()
            .and_then(|text| text.parse::<f64>().ok())
            .and_then(Float64::new)
            .map(Value::Float64)
            .ok_or_else(invalid),
        ("text", Type::Text) => payload
            .as_str()
            .map(|text| Value::Text(text.to_owned().into()))
            .ok_or_else(invalid),
        ("bytes", Type::Bytes) => payload
            .as_str()
            .and_then(|encoded| STANDARD.decode(encoded).ok())
            .map(|bytes| Value::Bytes(Arc::new(bytes)))
            .ok_or_else(invalid),
        ("list", Type::List(item_type)) => payload
            .as_array()
            .ok_or_else(invalid)?
            .iter()
            .map(|item| decode_value(item, item_type))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::from_list_items),
        ("map", Type::Map(item_type)) => {
            let object = payload.as_object().ok_or_else(invalid)?;
            let mut values = std::collections::BTreeMap::new();
            for (key, item) in object {
                values.insert(key.clone(), decode_value(item, item_type)?);
            }
            Ok(Value::Map(Arc::new(values)))
        }
        _ => Err(invalid()),
    }
}

fn validate_json_depth(value: &JsonValue, depth: usize) -> Result<(), PythonBridgeError> {
    if depth > 64 {
        return Err(bridge_error(
            "R_PYTHON_CONVERSION",
            "JSON nesting exceeds 64 levels",
        ));
    }
    match value {
        JsonValue::Array(items) => {
            for item in items {
                validate_json_depth(item, depth + 1)?;
            }
        }
        JsonValue::Object(fields) => {
            for item in fields.values() {
                validate_json_depth(item, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn request_frame_size(
    metadata_size: usize,
    attachments: &[&[u8]],
) -> Result<usize, PythonBridgeError> {
    let initial_size = if attachments.is_empty() {
        Some(metadata_size)
    } else {
        12usize.checked_add(metadata_size)
    };
    let mut size = initial_size
        .ok_or_else(|| bridge_error("R_PYTHON_FRAME", "Python request frame size overflow"))?;
    for attachment in attachments {
        size = size
            .checked_add(4)
            .and_then(|size| size.checked_add(attachment.len()))
            .ok_or_else(|| bridge_error("R_PYTHON_FRAME", "Python request frame size overflow"))?;
    }
    if size > MAX_FRAME {
        return Err(bridge_error(
            "R_PYTHON_FRAME",
            "Python request exceeds the maximum frame size",
        ));
    }
    Ok(size)
}

#[allow(clippy::too_many_arguments)]
fn write_request_frame<'data>(
    writer: &mut impl Write,
    id: u64,
    module: &'data str,
    function: &'data str,
    args: &'data [Value],
    kwargs: &'data [(String, Value)],
    json_result: bool,
    numpy_list_result: bool,
    mut profile: Option<&mut BridgeProfile>,
) -> Result<(), PythonBridgeError> {
    let attachments = RefCell::new(Vec::new());
    let request = BridgeRequest {
        id,
        module,
        function,
        args,
        kwargs,
        json_result,
        numpy_list_result,
        attachments: &attachments,
    };
    let serialization_started = profile.as_ref().map(|_| Instant::now());
    let metadata = serde_json::to_vec(&request)
        .map_err(|error| bridge_error("R_PYTHON_PROTOCOL", error.to_string()))?;
    if let (Some(profile), Some(started)) = (profile.as_deref_mut(), serialization_started) {
        profile.add("request_serialize", started.elapsed());
    }
    let attachments = attachments.into_inner();
    let frame_size = request_frame_size(metadata.len(), &attachments)?;
    let metadata_size = u32::try_from(metadata.len())
        .map_err(|_| bridge_error("R_PYTHON_FRAME", "Python request metadata is too large"))?;
    let attachment_count = u32::try_from(attachments.len())
        .map_err(|_| bridge_error("R_PYTHON_FRAME", "too many Python request attachments"))?;

    let write_started = profile.as_ref().map(|_| Instant::now());
    writer
        .write_all(&(frame_size as u32).to_be_bytes())
        .map_err(|error| bridge_error("R_PYTHON_PROTOCOL", error.to_string()))?;
    if attachments.is_empty() {
        writer
            .write_all(&metadata)
            .map_err(|error| bridge_error("R_PYTHON_PROTOCOL", error.to_string()))?;
    } else {
        writer
            .write_all(BINARY_FRAME_MAGIC)
            .and_then(|_| writer.write_all(&metadata_size.to_be_bytes()))
            .and_then(|_| writer.write_all(&metadata))
            .and_then(|_| writer.write_all(&attachment_count.to_be_bytes()))
            .map_err(|error| bridge_error("R_PYTHON_PROTOCOL", error.to_string()))?;
        for attachment in attachments {
            let attachment_size = u32::try_from(attachment.len()).map_err(|_| {
                bridge_error("R_PYTHON_FRAME", "Python request attachment is too large")
            })?;
            writer
                .write_all(&attachment_size.to_be_bytes())
                .and_then(|_| writer.write_all(attachment))
                .map_err(|error| bridge_error("R_PYTHON_PROTOCOL", error.to_string()))?;
        }
    }
    writer
        .flush()
        .map_err(|error| bridge_error("R_PYTHON_PROTOCOL", error.to_string()))?;
    if let (Some(profile), Some(started)) = (profile.as_deref_mut(), write_started) {
        profile.add("request_write", started.elapsed());
    }
    Ok(())
}

fn read_frame(
    reader: &mut impl Read,
    profile_enabled: bool,
) -> Result<Option<ReaderFrame>, String> {
    let mut header = [0; 4];
    match reader.read(&mut header[..1]) {
        Ok(0) => return Ok(None),
        Ok(_) => {}
        Err(error) => return Err(error.to_string()),
    }
    let first_byte_at = profile_enabled.then(Instant::now);
    reader
        .read_exact(&mut header[1..])
        .map_err(|error| format!("truncated Python response header: {error}"))?;
    let size = u32::from_be_bytes(header) as usize;
    if size > MAX_FRAME {
        return Err(format!(
            "Python response frame of {size} bytes exceeds limit"
        ));
    }
    let mut frame = vec![0; size];
    reader
        .read_exact(&mut frame)
        .map_err(|error| format!("truncated Python response frame: {error}"))?;
    let profile = first_byte_at.map(|first_byte_at| ReaderFrameProfile {
        first_byte_at,
        after_first_byte: first_byte_at.elapsed(),
        queued_at: Instant::now(),
    });
    Ok(Some(ReaderFrame {
        bytes: frame,
        profile,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn request_frame_uses_ordered_raw_attachments_for_nested_bytes() {
        let top_level = Value::Bytes(Arc::new(vec![0, 0xff]));
        let list_and_map = Value::List(Arc::new(vec![
            Value::Bytes(Arc::new(vec![b'c', 0, 0xff])),
            Value::Map(Arc::new(BTreeMap::from([
                ("zeta".into(), Value::Bytes(Arc::new(vec![0]))),
                ("alpha".into(), Value::Bytes(Arc::new(vec![0x80]))),
            ]))),
        ]));
        let args = [top_level, list_and_map];
        let kwargs = [
            ("z_payload".into(), Value::Bytes(Arc::new(vec![b'z']))),
            ("a_payload".into(), Value::Bytes(Arc::new(vec![b'a']))),
        ];
        let mut frame = Vec::new();
        write_request_frame(
            &mut frame, 7, "payloads", "echo", &args, &kwargs, false, false, None,
        )
        .unwrap();

        let body_len = u32::from_be_bytes(frame[..4].try_into().unwrap()) as usize;
        assert_eq!(body_len, frame.len() - 4);
        let body = &frame[4..];
        assert_eq!(&body[..4], b"GBIN");
        let metadata_len = u32::from_be_bytes(body[4..8].try_into().unwrap()) as usize;
        let metadata_start = 8;
        let metadata_end = metadata_start + metadata_len;
        let metadata: JsonValue =
            serde_json::from_slice(&body[metadata_start..metadata_end]).unwrap();
        assert_eq!(metadata["protocol"], 2);
        assert_eq!(metadata["args"][0]["v"], 0);
        assert_eq!(metadata["args"][1]["v"][0]["v"], 1);
        assert_eq!(metadata["args"][1]["v"][1]["v"]["alpha"]["v"], 2);
        assert_eq!(metadata["args"][1]["v"][1]["v"]["zeta"]["v"], 3);
        assert_eq!(metadata["kwargs"]["a_payload"]["v"], 4);
        assert_eq!(metadata["kwargs"]["z_payload"]["v"], 5);

        let mut offset = metadata_end;
        let count = u32::from_be_bytes(body[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;
        assert_eq!(count, 6);
        let mut attachments = Vec::new();
        for _ in 0..count {
            let length = u32::from_be_bytes(body[offset..offset + 4].try_into().unwrap()) as usize;
            offset += 4;
            attachments.push(body[offset..offset + length].to_vec());
            offset += length;
        }
        assert_eq!(
            offset,
            body.len(),
            "request frame must not have trailing bytes"
        );
        assert_eq!(
            attachments,
            vec![
                vec![0, 0xff],
                vec![b'c', 0, 0xff],
                vec![0x80],
                vec![0],
                vec![b'a'],
                vec![b'z']
            ]
        );
    }

    #[test]
    fn request_frame_size_counts_attachment_headers_and_bytes() {
        let two_bytes = [1, 2];
        assert_eq!(request_frame_size(10, &[&two_bytes]).unwrap(), 28);
        assert_eq!(
            request_frame_size(MAX_FRAME - 16, &[&[]]).unwrap(),
            MAX_FRAME
        );
        assert!(request_frame_size(MAX_FRAME + 1, &[]).is_err());
        assert!(request_frame_size(MAX_FRAME - 15, &[&[]]).is_err());
    }
}
