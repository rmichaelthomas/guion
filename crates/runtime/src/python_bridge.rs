use super::CancellationToken;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use guion_model::{BigInt, Float64, SharedJson, Type, Value};
use serde::{
    ser::{SerializeMap, SerializeSeq},
    Serialize, Serializer,
};
use serde_json::Value as JsonValue;
use std::{
    io::{self, Read, Write},
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        mpsc::{self, Receiver},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

const MAX_FRAME: usize = 24 * 1024 * 1024;
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
    responses: Receiver<Result<Vec<u8>, String>>,
    request_id: u64,
    timeout: Duration,
    closed: bool,
    native_image_observation: String,
    native_images: Vec<String>,
}

struct BridgeRequest<'a> {
    id: u64,
    module: &'a str,
    function: &'a str,
    args: &'a [Value],
    kwargs: &'a [(String, Value)],
    json_result: bool,
    numpy_list_result: bool,
}

impl Serialize for BridgeRequest<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut request = serializer.serialize_map(Some(9))?;
        request.serialize_entry("protocol", &1)?;
        request.serialize_entry("id", &self.id)?;
        request.serialize_entry("kind", "call")?;
        request.serialize_entry("module", self.module)?;
        request.serialize_entry("function", self.function)?;
        request.serialize_entry("args", &WireValues(self.args))?;
        request.serialize_entry("kwargs", &WireKeywords(self.kwargs))?;
        request.serialize_entry("json_result", &self.json_result)?;
        request.serialize_entry("numpy_list_result", &self.numpy_list_result)?;
        request.end()
    }
}

struct WireValue<'a>(&'a Value);

impl Serialize for WireValue<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut value = serializer.serialize_map(Some(2))?;
        match self.0 {
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
                value.serialize_entry("t", "bytes")?;
                value.serialize_entry("v", &STANDARD.encode(item.as_slice()))?;
            }
            Value::List(items) => {
                value.serialize_entry("t", "list")?;
                value.serialize_entry("v", &WireValues(items))?;
            }
            Value::Float64List(items) => {
                value.serialize_entry("t", "list")?;
                value.serialize_entry("v", &WireFloat64Values(items))?;
            }
            Value::Map(items) => {
                value.serialize_entry("t", "map")?;
                value.serialize_entry("v", &WireMap(items))?;
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

struct WireValues<'a>(&'a [Value]);

impl Serialize for WireValues<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for value in self.0 {
            sequence.serialize_element(&WireValue(value))?;
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
            let value = Value::Float64(*value);
            sequence.serialize_element(&WireValue(&value))?;
        }
        sequence.end()
    }
}

struct WireMap<'a>(&'a std::collections::BTreeMap<String, Value>);

impl Serialize for WireMap<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (key, value) in self.0.iter() {
            map.serialize_entry(key, &WireValue(value))?;
        }
        map.end()
    }
}

struct WireKeywords<'a>(&'a [(String, Value)]);

impl Serialize for WireKeywords<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        let mut entries = self.0.iter().collect::<Vec<_>>();
        entries.sort_unstable_by(|(left, _), (right, _)| left.cmp(right));
        for (key, value) in entries {
            map.serialize_entry(key, &WireValue(value))?;
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
        let mut child = Command::new(interpreter)
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
            .stderr(Stdio::piped())
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
        thread::spawn(move || loop {
            match read_frame(&mut output) {
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
        let request = BridgeRequest {
            id,
            module,
            function,
            args,
            kwargs,
            json_result: matches!(result_type, Type::Json),
            numpy_list_result: matches!(result_type, Type::List(_)),
        };
        write_frame(&mut self.input, &request)?;
        let deadline = Instant::now() + self.timeout;
        let bytes = loop {
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
                Ok(Ok(bytes)) => break bytes,
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
        let response: JsonValue = serde_json::from_slice(&bytes).map_err(|error| {
            self.terminate();
            bridge_error(
                "R_PYTHON_PROTOCOL",
                format!("invalid sidecar response JSON: {error}"),
            )
        })?;
        if response.get("protocol").and_then(JsonValue::as_u64) != Some(1)
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
        let value = decode_value(value, result_type)?;
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
        Value::Unit | Value::Bool(_) | Value::Int(_) | Value::Float64(_) | Value::Text(_) | Value::Bytes(_) => Ok(()),
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
        | Value::Record(_, _) => {
            Err(bridge_error(
                "R_PYTHON_CONVERSION",
                "Option, Result, structured errors, and native resources cannot cross the current Python bridge",
            ))
        }
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

fn write_frame(writer: &mut impl Write, value: &impl Serialize) -> Result<(), PythonBridgeError> {
    let bytes = serde_json::to_vec(value)
        .map_err(|error| bridge_error("R_PYTHON_PROTOCOL", error.to_string()))?;
    if bytes.len() > MAX_FRAME {
        return Err(bridge_error(
            "R_PYTHON_FRAME",
            "Python request exceeds the maximum frame size",
        ));
    }
    writer
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .and_then(|_| writer.write_all(&bytes))
        .and_then(|_| writer.flush())
        .map_err(|error| bridge_error("R_PYTHON_PROTOCOL", error.to_string()))
}

fn read_frame(reader: &mut impl Read) -> Result<Option<Vec<u8>>, String> {
    let mut header = [0; 4];
    match reader.read(&mut header[..1]) {
        Ok(0) => return Ok(None),
        Ok(_) => {}
        Err(error) => return Err(error.to_string()),
    }
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
    Ok(Some(frame))
}
