//! WebAssembly adapter for Guion's production bytecode VM.
//!
//! The adapter decodes a `.gub` artifact with `Artifact::from_bytes` and runs it through the
//! same instruction loop as the native runtime (`guion-vm-core`). It has no filesystem,
//! network, database, clock, or process access: those operations fail with
//! `R_HOST_UNSUPPORTED` instead of being skipped.
//!
//! The exported ABI is the one the AllHereHTML loader already consumes.

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use guion_model::{Artifact, BigInt, Float64, RecordDefinition, SharedJson, Value};
use guion_vm_core::{PortableHost, VmLimits};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::{BTreeMap, BTreeSet};
use std::{cell::UnsafeCell, sync::Arc};

pub const MAX_ARTIFACT_BYTES: usize = 1024 * 1024;
pub const MAX_OUTPUT_BYTES: usize = 64 * 1024;
pub const MAX_ERROR_BYTES: usize = 1024;
pub const MAX_BROWSER_STEPS: u32 = 10_000;
pub const MAX_REQUEST_BYTES: usize = 256 * 1024;
pub const MAX_RESPONSE_BYTES: usize = 256 * 1024;
pub const MAX_WIRE_DEPTH: usize = 64;
pub const MAX_WIRE_NODES: usize = 4_096;
pub const MAX_WIRE_COLLECTION_ITEMS: usize = 4_096;
pub const MAX_WIRE_VALUE_BYTES: usize = 64 * 1024;
pub const MAX_WIRE_INTEGER_DIGITS: usize = 4_096;
const MAX_LIVE_INPUTS: usize = 16;
const MAX_LIVE_INPUT_BYTES: usize = MAX_ARTIFACT_BYTES * 2;
/// Cumulative guest allocation allowed in the browser, well below the native default.
const MAX_BROWSER_ALLOCATION_BYTES: usize = 16 * 1024 * 1024;

/// ABI version, packed as `(major << 16) | minor`. Version 1 adds typed request/response
/// exports while preserving the legacy pointer/i64 exports.
pub const WASM_ABI_VERSION: u32 = 0x0001_0000;

/// Return the packed WebAssembly ABI version implemented by this module.
///
/// A missing export means the module predates ABI version discovery. ABI 1.0 adds typed
/// calls and retains the legacy pointer-based functions for existing callers.
#[no_mangle]
pub extern "C" fn guion_wasm_abi_version() -> u32 {
    WASM_ABI_VERSION
}

#[derive(Debug, PartialEq, Eq)]
pub struct RunOutput {
    pub output: String,
    pub steps: u32,
    pub return_value: i64,
}

/// Decode and run `bytes`, returning printed output, step count, and the integer result of
/// the artifact's entry function.
pub fn run_artifact(bytes: &[u8], max_steps: u32) -> Result<RunOutput, String> {
    if bytes.len() > MAX_ARTIFACT_BYTES {
        return Err(format!(
            "artifact exceeds the {MAX_ARTIFACT_BYTES}-byte browser limit"
        ));
    }
    if max_steps > MAX_BROWSER_STEPS {
        return Err(format!(
            "max_steps exceeds the browser limit of {MAX_BROWSER_STEPS}"
        ));
    }
    let artifact = Artifact::from_bytes(bytes)
        .map_err(|error| format!("{}: {}", error.code, error.message))?;

    let mut limits = VmLimits::default();
    let mut step_budget = u64::from(max_steps);
    if let Some(metadata) = &artifact.build_metadata {
        let embedded = &metadata.resource_limits;
        step_budget = step_budget.min(embedded.instruction_limit);
        limits.allocation_limit_bytes = embedded.allocation_limit_bytes;
        limits.call_depth_limit = embedded.call_depth_limit;
        limits.output_limit_bytes = embedded.output_limit_bytes;
        limits.max_integer_digits = embedded.max_integer_digits;
        limits.max_text_bytes = embedded.max_text_bytes;
        limits.max_collection_items = embedded.max_collection_items;
        limits.max_file_bytes = embedded.max_file_bytes;
    }
    limits.output_limit_bytes = limits.output_limit_bytes.min(MAX_OUTPUT_BYTES);
    limits.allocation_limit_bytes = limits
        .allocation_limit_bytes
        .min(MAX_BROWSER_ALLOCATION_BYTES);

    let entry = artifact.entry().to_owned();
    let mut host = PortableHost::new(Arc::new(artifact), limits);
    let run = host.invoke(&entry, Vec::new(), step_budget);
    let output = host.take_output();
    match run {
        Ok(run) => {
            let return_value = match &run.return_value {
                Value::Int(value) => value.to_i64().ok_or_else(|| {
                    format!("`{entry}` returned an integer outside the 64-bit range")
                })?,
                other => {
                    return Err(format!(
                        "`{entry}` must return Int to run in the browser, got {other:?}"
                    ))
                }
            };
            Ok(RunOutput {
                output,
                steps: u32::try_from(run.steps).unwrap_or(u32::MAX),
                return_value,
            })
        }
        Err(error) => Err(format!("{}: {}", error.code, error.message)),
    }
}

struct ResultSlot {
    output: [u8; MAX_OUTPUT_BYTES],
    error: [u8; MAX_ERROR_BYTES],
    response: [u8; MAX_RESPONSE_BYTES],
    output_length: u32,
    error_length: u32,
    response_length: u32,
    steps: u32,
    return_value: i64,
}

/// The module runs on a single thread, so plain interior mutability is sufficient.
struct SharedSlot(UnsafeCell<ResultSlot>);

// SAFETY: WebAssembly modules built from this crate are single-threaded.
unsafe impl Sync for SharedSlot {}

static RESULT: SharedSlot = SharedSlot(UnsafeCell::new(ResultSlot {
    output: [0; MAX_OUTPUT_BYTES],
    error: [0; MAX_ERROR_BYTES],
    response: [0; MAX_RESPONSE_BYTES],
    output_length: 0,
    error_length: 0,
    response_length: 0,
    steps: 0,
    return_value: 0,
}));

fn slot() -> &'static mut ResultSlot {
    // SAFETY: single-threaded access; no reference is held across calls.
    unsafe { &mut *RESULT.0.get() }
}

fn reset_result() {
    let slot = slot();
    slot.output_length = 0;
    slot.error_length = 0;
    slot.response_length = 0;
    slot.steps = 0;
    slot.return_value = 0;
}

fn set_error(message: &str) {
    let slot = slot();
    let mut end = message.len().min(MAX_ERROR_BYTES);
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    slot.error[..end].copy_from_slice(&message.as_bytes()[..end]);
    slot.error_length = end as u32;
}

fn set_output(output: &str) -> Result<(), String> {
    if output.len() > MAX_OUTPUT_BYTES {
        return Err(format!(
            "program output exceeds the {MAX_OUTPUT_BYTES}-byte browser limit"
        ));
    }
    let slot = slot();
    slot.output[..output.len()].copy_from_slice(output.as_bytes());
    slot.output_length = output.len() as u32;
    Ok(())
}

#[derive(Clone, Copy)]
struct InputAllocation {
    pointer: u32,
    length: u32,
}

const EMPTY_ALLOCATION: InputAllocation = InputAllocation {
    pointer: 0,
    length: 0,
};

struct InputAllocationState {
    entries: [InputAllocation; MAX_LIVE_INPUTS],
    total_bytes: usize,
}

struct SharedAllocations(UnsafeCell<InputAllocationState>);

// SAFETY: the Wasm adapter is single-threaded; its allocation registry is only used by ABI calls.
unsafe impl Sync for SharedAllocations {}

static INPUT_ALLOCATIONS: SharedAllocations =
    SharedAllocations(UnsafeCell::new(InputAllocationState {
        entries: [EMPTY_ALLOCATION; MAX_LIVE_INPUTS],
        total_bytes: 0,
    }));

fn input_allocations() -> &'static mut InputAllocationState {
    // SAFETY: Wasm ABI calls are single-threaded and no mutable reference crosses a call boundary.
    unsafe { &mut *INPUT_ALLOCATIONS.0.get() }
}

fn is_live_input(pointer: u32, length: u32) -> bool {
    input_allocations()
        .entries
        .iter()
        .any(|entry| entry.pointer == pointer && entry.length == length && pointer != 0)
}

#[no_mangle]
pub extern "C" fn allocate_input(length: u32) -> u32 {
    if length == 0 || length as usize > MAX_ARTIFACT_BYTES {
        return 0;
    }
    let state = input_allocations();
    let Some(index) = state.entries.iter().position(|entry| entry.pointer == 0) else {
        return 0;
    };
    if state.total_bytes.saturating_add(length as usize) > MAX_LIVE_INPUT_BYTES {
        return 0;
    }
    let buffer = vec![0_u8; length as usize].into_boxed_slice();
    let pointer = Box::into_raw(buffer) as *mut u8 as usize as u32;
    if pointer == 0 {
        return 0;
    }
    state.entries[index] = InputAllocation { pointer, length };
    state.total_bytes += length as usize;
    pointer
}

#[no_mangle]
pub extern "C" fn release_input(pointer: u32, length: u32) {
    if pointer == 0 || length == 0 || length as usize > MAX_ARTIFACT_BYTES {
        return;
    }
    let state = input_allocations();
    let Some(index) = state
        .entries
        .iter()
        .position(|entry| entry.pointer == pointer && entry.length == length)
    else {
        return;
    };
    state.entries[index] = EMPTY_ALLOCATION;
    state.total_bytes = state.total_bytes.saturating_sub(length as usize);
    // SAFETY: the caller returns a buffer obtained from `allocate_input` with its length.
    unsafe {
        let slice = std::ptr::slice_from_raw_parts_mut(pointer as *mut u8, length as usize);
        drop(Box::from_raw(slice));
    }
}

/// Returns 0 on success and 1 on failure; the details are in the `result_*` exports.
///
/// # Safety
/// `pointer` must be the live buffer returned by `allocate_input`, with its original length.
#[no_mangle]
pub unsafe extern "C" fn execute_artifact(pointer: u32, length: u32, max_steps: u32) -> u32 {
    reset_result();
    if pointer == 0 || length == 0 {
        set_error("artifact input is empty");
        return 1;
    }
    if length as usize > MAX_ARTIFACT_BYTES {
        set_error("artifact exceeds the 1048576-byte browser limit");
        return 1;
    }
    if !is_live_input(pointer, length) {
        set_error("artifact pointer and length must match a live allocated input buffer");
        return 1;
    }
    let bytes = std::slice::from_raw_parts(pointer as *const u8, length as usize);
    match run_artifact(bytes, max_steps)
        .and_then(|result| set_output(&result.output).map(|_| result))
    {
        Ok(result) => {
            let slot = slot();
            slot.steps = result.steps;
            slot.return_value = result.return_value;
            0
        }
        Err(message) => {
            set_error(&message);
            1
        }
    }
}

#[no_mangle]
pub extern "C" fn result_output_ptr() -> u32 {
    slot().output.as_ptr() as usize as u32
}

#[no_mangle]
pub extern "C" fn result_output_len() -> u32 {
    slot().output_length
}

#[no_mangle]
pub extern "C" fn result_error_ptr() -> u32 {
    slot().error.as_ptr() as usize as u32
}

#[no_mangle]
pub extern "C" fn result_error_len() -> u32 {
    slot().error_length
}

#[no_mangle]
pub extern "C" fn result_step_count() -> u32 {
    slot().steps
}

#[no_mangle]
pub extern "C" fn result_return_value() -> i64 {
    slot().return_value
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct AbiVersion {
    major: u16,
    minor: u16,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireRequest {
    abi: AbiVersion,
    entry: String,
    arguments: Vec<WireValue>,
    capabilities: Vec<String>,
    #[serde(default)]
    limits: Option<WireLimits>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WireLimits {
    steps: Option<u32>,
    output_bytes: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", content = "value", deny_unknown_fields)]
enum WireValue {
    Int(String),
    Float64(String),
    Bool(bool),
    Text(String),
    Bytes(String),
    Unit,
    List(Vec<WireValue>),
    Map(Vec<WireMapEntry>),
    Record(WireRecord),
    OptionNone,
    OptionSome(Box<WireValue>),
    ResultOk(Box<WireValue>),
    ResultErr(Box<WireValue>),
    Json(JsonValue),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireMapEntry {
    key: String,
    value: WireValue,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireRecord {
    name: String,
    fields: Vec<WireField>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireField {
    name: String,
    value: WireValue,
}

#[derive(Serialize)]
struct WireResponse {
    abi: AbiVersion,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<WireValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stdout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    steps: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

#[derive(Debug)]
struct AbiFailure {
    code: String,
    message: String,
}

impl AbiFailure {
    fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

const ABI_V1: AbiVersion = AbiVersion { major: 1, minor: 0 };

/// Execute one typed ABI request and return its JSON response envelope.
///
/// This host-callable helper is also used by the Wasm exports, keeping native contract tests
/// and browser execution on the same request validation and VM path.
pub fn execute_artifact_request(artifact_bytes: &[u8], request_json: &[u8]) -> Vec<u8> {
    execute_artifact_request_with_status(artifact_bytes, request_json).0
}

fn execute_artifact_request_with_status(
    artifact_bytes: &[u8],
    request_json: &[u8],
) -> (Vec<u8>, u32) {
    let result = execute_typed(artifact_bytes, request_json);
    match result {
        Ok(response) => serialize_response(response, 0),
        Err(failure) => serialize_response(
            WireResponse {
                abi: ABI_V1,
                ok: false,
                value: None,
                stdout: None,
                steps: None,
                code: Some(failure.code),
                message: Some(failure.message),
            },
            1,
        ),
    }
}

fn serialize_response(response: WireResponse, success_status: u32) -> (Vec<u8>, u32) {
    match serde_json::to_vec(&response) {
        Ok(bytes) if bytes.len() <= MAX_RESPONSE_BYTES => (bytes, success_status),
        Ok(_) => serialize_limit_error("serialized response exceeds the browser ABI limit"),
        Err(_) => serialize_limit_error("could not serialize the browser ABI response"),
    }
}

fn serialize_limit_error(message: &str) -> (Vec<u8>, u32) {
    let response = WireResponse {
        abi: ABI_V1,
        ok: false,
        value: None,
        stdout: None,
        steps: None,
        code: Some("E_ABI_LIMIT".into()),
        message: Some(message.into()),
    };
    // This fixed response is far below the response ceiling.
    (serde_json::to_vec(&response).unwrap_or_default(), 1)
}

fn execute_typed(artifact_bytes: &[u8], request_json: &[u8]) -> Result<WireResponse, AbiFailure> {
    if request_json.len() > MAX_REQUEST_BYTES {
        return Err(AbiFailure::new(
            "E_ABI_LIMIT",
            format!("request exceeds the {MAX_REQUEST_BYTES}-byte browser ABI limit"),
        ));
    }
    if artifact_bytes.is_empty() {
        return Err(AbiFailure::new("E_ABI_ARTIFACT", "artifact input is empty"));
    }
    if artifact_bytes.len() > MAX_ARTIFACT_BYTES {
        return Err(AbiFailure::new(
            "E_ABI_LIMIT",
            format!("artifact exceeds the {MAX_ARTIFACT_BYTES}-byte browser limit"),
        ));
    }
    let request: WireRequest = serde_json::from_slice(request_json)
        .map_err(|error| AbiFailure::new("E_ABI_REQUEST", error.to_string()))?;
    if request.abi != ABI_V1 {
        return Err(AbiFailure::new(
            "E_ABI_VERSION",
            format!(
                "unsupported ABI version {}.{}; this module implements 1.0",
                request.abi.major, request.abi.minor
            ),
        ));
    }
    if request.entry.is_empty() || request.entry.len() > 128 {
        return Err(AbiFailure::new(
            "E_ABI_ENTRY",
            "entry must contain between 1 and 128 UTF-8 bytes",
        ));
    }
    if !request.capabilities.is_empty() {
        return Err(AbiFailure::new(
            "E_CAPABILITY_UNSUPPORTED",
            "ABI 1.0 grants no host capabilities",
        ));
    }
    let requested_steps = request
        .limits
        .as_ref()
        .and_then(|limits| limits.steps)
        .unwrap_or(MAX_BROWSER_STEPS);
    let requested_output = request
        .limits
        .as_ref()
        .and_then(|limits| limits.output_bytes)
        .map(|bytes| bytes as usize)
        .unwrap_or(MAX_OUTPUT_BYTES);
    if requested_steps > MAX_BROWSER_STEPS || requested_output > MAX_OUTPUT_BYTES {
        return Err(AbiFailure::new(
            "E_ABI_LIMIT",
            "requested limits exceed this module's browser ceilings",
        ));
    }

    let artifact = Artifact::from_bytes(artifact_bytes)
        .map_err(|error| AbiFailure::new(error.code, error.message))?;
    let mut nodes = 0;
    let mut args = Vec::with_capacity(request.arguments.len());
    for wire in request.arguments {
        args.push(decode_wire_value(wire, &artifact.records, 0, &mut nodes)?);
    }

    let mut limits = VmLimits::default();
    let mut step_budget = u64::from(requested_steps);
    if let Some(metadata) = &artifact.build_metadata {
        let embedded = &metadata.resource_limits;
        step_budget = step_budget.min(embedded.instruction_limit);
        limits.allocation_limit_bytes = embedded.allocation_limit_bytes;
        limits.call_depth_limit = embedded.call_depth_limit;
        limits.output_limit_bytes = embedded.output_limit_bytes;
        limits.max_integer_digits = embedded.max_integer_digits;
        limits.max_text_bytes = embedded.max_text_bytes;
        limits.max_collection_items = embedded.max_collection_items;
        limits.max_file_bytes = embedded.max_file_bytes;
    }
    limits.output_limit_bytes = limits
        .output_limit_bytes
        .min(MAX_OUTPUT_BYTES)
        .min(requested_output);
    limits.allocation_limit_bytes = limits
        .allocation_limit_bytes
        .min(MAX_BROWSER_ALLOCATION_BYTES);

    let mut host = PortableHost::new(Arc::new(artifact.clone()), limits);
    let run = host.invoke(&request.entry, args, step_budget);
    let stdout = host.take_output();
    let steps = u32::try_from(host.steps_used()).unwrap_or(u32::MAX);
    match run {
        Ok(run) => {
            let value = encode_wire_value(&run.return_value, &artifact.records, 0, &mut nodes)?;
            Ok(WireResponse {
                abi: ABI_V1,
                ok: true,
                value: Some(value),
                stdout: Some(stdout),
                steps: Some(u32::try_from(run.steps).unwrap_or(u32::MAX)),
                code: None,
                message: None,
            })
        }
        Err(error) => Ok(WireResponse {
            abi: ABI_V1,
            ok: false,
            value: None,
            stdout: Some(stdout),
            steps: Some(steps),
            code: Some(error.code),
            message: Some(error.message),
        }),
    }
}

fn decode_wire_value(
    wire: WireValue,
    records: &[RecordDefinition],
    depth: usize,
    nodes: &mut usize,
) -> Result<Value, AbiFailure> {
    count_wire_node(depth, nodes)?;
    match wire {
        WireValue::Int(decimal) => {
            let digits = decimal.strip_prefix('-').unwrap_or(&decimal);
            if digits.len() > MAX_WIRE_INTEGER_DIGITS {
                return Err(AbiFailure::new("E_ABI_LIMIT", "integer value is too large"));
            }
            let parsed =
                BigInt::parse(&decimal).map_err(|error| AbiFailure::new("E_ABI_VALUE", error))?;
            if parsed.to_string() != decimal {
                return Err(AbiFailure::new(
                    "E_ABI_VALUE",
                    "integer values must use canonical signed decimal notation",
                ));
            }
            Ok(Value::Int(parsed))
        }
        WireValue::Float64(bits) => {
            if bits.len() != 16 || !bits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(AbiFailure::new(
                    "E_ABI_VALUE",
                    "Float64 values must contain exactly 16 hexadecimal bit digits",
                ));
            }
            let raw = u64::from_str_radix(&bits, 16)
                .map_err(|_| AbiFailure::new("E_ABI_VALUE", "invalid Float64 bit pattern"))?;
            let value = Float64::new(f64::from_bits(raw))
                .ok_or_else(|| AbiFailure::new("E_ABI_VALUE", "Float64 must be finite"))?;
            Ok(Value::Float64(value))
        }
        WireValue::Bool(value) => Ok(Value::Bool(value)),
        WireValue::Text(value) => {
            check_string(&value)?;
            Ok(Value::Text(value.into()))
        }
        WireValue::Bytes(value) => {
            if value.len() > MAX_WIRE_VALUE_BYTES.saturating_mul(4).div_ceil(3) + 4 {
                return Err(AbiFailure::new("E_ABI_LIMIT", "byte value is too large"));
            }
            let bytes = BASE64
                .decode(value)
                .map_err(|_| AbiFailure::new("E_ABI_VALUE", "Bytes value must be valid base64"))?;
            if bytes.len() > MAX_WIRE_VALUE_BYTES {
                return Err(AbiFailure::new("E_ABI_LIMIT", "byte value is too large"));
            }
            Ok(Value::Bytes(Arc::new(bytes)))
        }
        WireValue::Unit => Ok(Value::Unit),
        WireValue::List(items) => {
            if items.len() > MAX_WIRE_COLLECTION_ITEMS {
                return Err(AbiFailure::new(
                    "E_ABI_LIMIT",
                    "list contains too many values",
                ));
            }
            let values = items
                .into_iter()
                .map(|item| decode_wire_value(item, records, depth + 1, nodes))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Value::from_list_items(values))
        }
        WireValue::Map(entries) => {
            if entries.len() > MAX_WIRE_COLLECTION_ITEMS {
                return Err(AbiFailure::new(
                    "E_ABI_LIMIT",
                    "map contains too many values",
                ));
            }
            let mut values = BTreeMap::new();
            let mut previous: Option<String> = None;
            for entry in entries {
                check_string(&entry.key)?;
                if previous.as_ref().is_some_and(|key| key >= &entry.key) {
                    return Err(AbiFailure::new(
                        "E_ABI_VALUE",
                        "map entries must be uniquely sorted by key",
                    ));
                }
                previous = Some(entry.key.clone());
                let value = decode_wire_value(entry.value, records, depth + 1, nodes)?;
                values.insert(entry.key, value);
            }
            Ok(Value::Map(Arc::new(values)))
        }
        WireValue::Record(record) => {
            check_string(&record.name)?;
            if record.fields.len() > MAX_WIRE_COLLECTION_ITEMS {
                return Err(AbiFailure::new(
                    "E_ABI_LIMIT",
                    "record contains too many fields",
                ));
            }
            let definition = records
                .iter()
                .find(|definition| definition.name == record.name)
                .ok_or_else(|| AbiFailure::new("E_ABI_VALUE", "unknown record type"))?;
            if record.fields.len() != definition.fields.len() {
                return Err(AbiFailure::new(
                    "E_ABI_VALUE",
                    "record fields do not match the declared record shape",
                ));
            }
            let mut seen = BTreeSet::new();
            let mut values = Vec::with_capacity(record.fields.len());
            for (field, declared) in record.fields.into_iter().zip(&definition.fields) {
                check_string(&field.name)?;
                if !seen.insert(field.name.clone()) || field.name != declared.name {
                    return Err(AbiFailure::new(
                        "E_ABI_VALUE",
                        "record fields must match the declared order and names exactly",
                    ));
                }
                values.push(decode_wire_value(field.value, records, depth + 1, nodes)?);
            }
            Ok(Value::Record(record.name.into(), Arc::new(values)))
        }
        WireValue::OptionNone => Ok(Value::OptionNone),
        WireValue::OptionSome(value) => Ok(Value::OptionSome(Box::new(decode_wire_value(
            *value,
            records,
            depth + 1,
            nodes,
        )?))),
        WireValue::ResultOk(value) => Ok(Value::ResultOk(Box::new(decode_wire_value(
            *value,
            records,
            depth + 1,
            nodes,
        )?))),
        WireValue::ResultErr(value) => Ok(Value::ResultErr(Box::new(decode_wire_value(
            *value,
            records,
            depth + 1,
            nodes,
        )?))),
        WireValue::Json(value) => {
            check_json_bounds(&value, 0, nodes)?;
            Ok(Value::Json(SharedJson::from_serde(value)))
        }
    }
}

fn encode_wire_value(
    value: &Value,
    records: &[RecordDefinition],
    depth: usize,
    nodes: &mut usize,
) -> Result<WireValue, AbiFailure> {
    count_wire_node(depth, nodes)?;
    match value {
        Value::Int(value) => {
            let decimal = value.to_string();
            if decimal.strip_prefix('-').unwrap_or(&decimal).len() > MAX_WIRE_INTEGER_DIGITS {
                return Err(AbiFailure::new(
                    "E_ABI_LIMIT",
                    "integer result is too large",
                ));
            }
            Ok(WireValue::Int(decimal))
        }
        Value::Float64(value) => Ok(WireValue::Float64(format!(
            "{:016x}",
            value.get().to_bits()
        ))),
        Value::Bool(value) => Ok(WireValue::Bool(*value)),
        Value::Text(value) => {
            check_string(value)?;
            Ok(WireValue::Text(value.to_string()))
        }
        Value::Bytes(value) => {
            if value.len() > MAX_WIRE_VALUE_BYTES {
                return Err(AbiFailure::new("E_ABI_LIMIT", "byte result is too large"));
            }
            Ok(WireValue::Bytes(BASE64.encode(value.as_slice())))
        }
        Value::Unit => Ok(WireValue::Unit),
        Value::List(values) => {
            if values.len() > MAX_WIRE_COLLECTION_ITEMS {
                return Err(AbiFailure::new(
                    "E_ABI_LIMIT",
                    "list result contains too many values",
                ));
            }
            Ok(WireValue::List(
                values
                    .iter()
                    .map(|value| encode_wire_value(value, records, depth + 1, nodes))
                    .collect::<Result<Vec<_>, _>>()?,
            ))
        }
        Value::Float64List(values) => {
            if values.len() > MAX_WIRE_COLLECTION_ITEMS {
                return Err(AbiFailure::new(
                    "E_ABI_LIMIT",
                    "list result contains too many values",
                ));
            }
            Ok(WireValue::List(
                values
                    .iter()
                    .map(|value| {
                        encode_wire_value(&Value::Float64(*value), records, depth + 1, nodes)
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            ))
        }
        Value::Map(values) => {
            if values.len() > MAX_WIRE_COLLECTION_ITEMS {
                return Err(AbiFailure::new(
                    "E_ABI_LIMIT",
                    "map result contains too many values",
                ));
            }
            Ok(WireValue::Map(
                values
                    .iter()
                    .map(|(key, value)| {
                        check_string(key)?;
                        Ok(WireMapEntry {
                            key: key.clone(),
                            value: encode_wire_value(value, records, depth + 1, nodes)?,
                        })
                    })
                    .collect::<Result<Vec<_>, AbiFailure>>()?,
            ))
        }
        Value::Record(name, fields) => {
            let definition = records
                .iter()
                .find(|definition| definition.name == *name)
                .ok_or_else(|| AbiFailure::new("E_ABI_VALUE_UNSUPPORTED", "unknown record type"))?;
            if fields.len() != definition.fields.len() {
                return Err(AbiFailure::new(
                    "E_ABI_VALUE_UNSUPPORTED",
                    "record result does not match its declared shape",
                ));
            }
            let mut encoded = Vec::with_capacity(fields.len());
            for (value, field) in fields.iter().zip(&definition.fields) {
                encoded.push(WireField {
                    name: field.name.clone(),
                    value: encode_wire_value(value, records, depth + 1, nodes)?,
                });
            }
            Ok(WireValue::Record(WireRecord {
                name: name.to_string(),
                fields: encoded,
            }))
        }
        Value::OptionNone => Ok(WireValue::OptionNone),
        Value::OptionSome(value) => Ok(WireValue::OptionSome(Box::new(encode_wire_value(
            value,
            records,
            depth + 1,
            nodes,
        )?))),
        Value::ResultOk(value) => Ok(WireValue::ResultOk(Box::new(encode_wire_value(
            value,
            records,
            depth + 1,
            nodes,
        )?))),
        Value::ResultErr(value) => Ok(WireValue::ResultErr(Box::new(encode_wire_value(
            value,
            records,
            depth + 1,
            nodes,
        )?))),
        Value::Json(value) => {
            let json = value.to_serde();
            check_json_bounds(&json, depth + 1, nodes)?;
            Ok(WireValue::Json(json))
        }
        Value::TaskError(_)
        | Value::JsonError(_)
        | Value::FileError(_)
        | Value::HttpError(_)
        | Value::NativeResource(_) => Err(AbiFailure::new(
            "E_ABI_VALUE_UNSUPPORTED",
            "this runtime value cannot cross the browser ABI",
        )),
    }
}

fn count_wire_node(depth: usize, nodes: &mut usize) -> Result<(), AbiFailure> {
    if depth > MAX_WIRE_DEPTH {
        return Err(AbiFailure::new(
            "E_ABI_LIMIT",
            "value nesting exceeds the browser ABI limit",
        ));
    }
    *nodes = nodes.saturating_add(1);
    if *nodes > MAX_WIRE_NODES {
        return Err(AbiFailure::new(
            "E_ABI_LIMIT",
            "value contains too many nodes",
        ));
    }
    Ok(())
}

fn check_string(value: &str) -> Result<(), AbiFailure> {
    if value.len() > MAX_WIRE_VALUE_BYTES {
        return Err(AbiFailure::new("E_ABI_LIMIT", "string value is too large"));
    }
    Ok(())
}

fn check_json_bounds(value: &JsonValue, depth: usize, nodes: &mut usize) -> Result<(), AbiFailure> {
    count_wire_node(depth, nodes)?;
    match value {
        JsonValue::String(value) => check_string(value),
        JsonValue::Array(values) => {
            if values.len() > MAX_WIRE_COLLECTION_ITEMS {
                return Err(AbiFailure::new(
                    "E_ABI_LIMIT",
                    "JSON array contains too many values",
                ));
            }
            for value in values {
                check_json_bounds(value, depth + 1, nodes)?;
            }
            Ok(())
        }
        JsonValue::Object(values) => {
            if values.len() > MAX_WIRE_COLLECTION_ITEMS {
                return Err(AbiFailure::new(
                    "E_ABI_LIMIT",
                    "JSON object contains too many values",
                ));
            }
            for (key, value) in values {
                check_string(key)?;
                check_json_bounds(value, depth + 1, nodes)?;
            }
            Ok(())
        }
        JsonValue::Null | JsonValue::Bool(_) | JsonValue::Number(_) => Ok(()),
    }
}

/// Execute the typed ABI using two caller-allocated buffers.
///
/// The returned JSON envelope is available through `result_response_ptr/len`, including
/// structured failures. Return 0 for `ok: true`, 1 for a failure envelope.
///
/// # Safety
/// Both pointers must refer to live buffers returned by `allocate_input` with their original
/// lengths, and those buffers must not be released until this function returns.
#[no_mangle]
pub unsafe extern "C" fn execute_artifact_v1(
    artifact_pointer: u32,
    artifact_length: u32,
    request_pointer: u32,
    request_length: u32,
) -> u32 {
    reset_result();
    if artifact_pointer == 0 || artifact_length == 0 {
        let (response, status) = serialize_failure("E_ABI_ARTIFACT", "artifact input is empty");
        store_response(&response);
        return status;
    }
    if request_pointer == 0 || request_length == 0 {
        let (response, status) = serialize_failure("E_ABI_REQUEST", "request input is empty");
        store_response(&response);
        return status;
    }
    if artifact_length as usize > MAX_ARTIFACT_BYTES || request_length as usize > MAX_REQUEST_BYTES
    {
        let (response, status) =
            serialize_failure("E_ABI_LIMIT", "input exceeds the browser ABI limit");
        store_response(&response);
        return status;
    }
    if !is_live_input(artifact_pointer, artifact_length)
        || !is_live_input(request_pointer, request_length)
    {
        let (response, status) = serialize_failure(
            "E_ABI_POINTER",
            "pointer and length must match a live allocated input buffer",
        );
        store_response(&response);
        return status;
    }
    let artifact =
        std::slice::from_raw_parts(artifact_pointer as *const u8, artifact_length as usize);
    let request = std::slice::from_raw_parts(request_pointer as *const u8, request_length as usize);
    let (response, status) = execute_artifact_request_with_status(artifact, request);
    store_response(&response);
    status
}

fn serialize_failure(code: &str, message: &str) -> (Vec<u8>, u32) {
    serialize_response(
        WireResponse {
            abi: ABI_V1,
            ok: false,
            value: None,
            stdout: None,
            steps: None,
            code: Some(code.into()),
            message: Some(message.into()),
        },
        1,
    )
}

fn store_response(response: &[u8]) {
    let slot = slot();
    let length = response.len().min(MAX_RESPONSE_BYTES);
    slot.response[..length].copy_from_slice(&response[..length]);
    slot.response_length = length as u32;
}

#[no_mangle]
pub extern "C" fn result_response_ptr() -> u32 {
    slot().response.as_ptr() as usize as u32
}

#[no_mangle]
pub extern "C" fn result_response_len() -> u32 {
    slot().response_length
}

#[cfg(test)]
mod typed_abi_tests {
    use super::*;
    use guion_model::{RecordField, Type};

    fn records() -> Vec<RecordDefinition> {
        vec![RecordDefinition {
            name: "Card".into(),
            fields: vec![
                RecordField {
                    name: "title".into(),
                    ty: Type::Text,
                },
                RecordField {
                    name: "count".into(),
                    ty: Type::Int,
                },
            ],
        }]
    }

    #[test]
    fn wire_round_trips_every_supported_value_family() {
        let records = records();
        let mut map = BTreeMap::new();
        map.insert("alpha".into(), Value::Bool(true));
        map.insert("omega".into(), Value::Text("last".into()));
        let samples = vec![
            Value::Int(BigInt::parse("123456789012345678901234567890").unwrap()),
            Value::Float64(Float64::new(-123.5).unwrap()),
            Value::Bool(false),
            Value::Text("hello 🌱".into()),
            Value::Bytes(Arc::new(vec![0, 1, 127, 255])),
            Value::Unit,
            Value::from_list_items(vec![
                Value::Int(BigInt::from_i64(1)),
                Value::Int(BigInt::from_i64(2)),
            ]),
            Value::Map(Arc::new(map)),
            Value::Record(
                "Card".into(),
                Arc::new(vec![
                    Value::Text("A title".into()),
                    Value::Int(BigInt::from_i64(7)),
                ]),
            ),
            Value::OptionNone,
            Value::OptionSome(Box::new(Value::Text("present".into()))),
            Value::ResultOk(Box::new(Value::Int(BigInt::from_i64(3)))),
            Value::ResultErr(Box::new(Value::Text("problem".into()))),
            Value::Json(SharedJson::from_serde(
                serde_json::from_str(r#"{"count":12345678901234567890,"ok":true}"#).unwrap(),
            )),
        ];

        for value in samples {
            let mut nodes = 0;
            let encoded = encode_wire_value(&value, &records, 0, &mut nodes).unwrap();
            let json = serde_json::to_vec(&encoded).unwrap();
            let parsed: WireValue = serde_json::from_slice(&json).unwrap();
            let mut decoded_nodes = 0;
            let decoded = decode_wire_value(parsed, &records, 0, &mut decoded_nodes).unwrap();
            assert_eq!(decoded, value);
        }
    }

    #[test]
    fn wire_rejects_duplicate_record_fields_unsorted_maps_and_nonfinite_floats() {
        let records = records();
        let mut nodes = 0;
        let duplicate_record = WireValue::Record(WireRecord {
            name: "Card".into(),
            fields: vec![
                WireField {
                    name: "title".into(),
                    value: WireValue::Text("first".into()),
                },
                WireField {
                    name: "title".into(),
                    value: WireValue::Int("1".into()),
                },
            ],
        });
        assert_eq!(
            decode_wire_value(duplicate_record, &records, 0, &mut nodes)
                .unwrap_err()
                .code,
            "E_ABI_VALUE"
        );

        let mut nodes = 0;
        let unsorted_map = WireValue::Map(vec![
            WireMapEntry {
                key: "z".into(),
                value: WireValue::Unit,
            },
            WireMapEntry {
                key: "a".into(),
                value: WireValue::Unit,
            },
        ]);
        assert_eq!(
            decode_wire_value(unsorted_map, &records, 0, &mut nodes)
                .unwrap_err()
                .code,
            "E_ABI_VALUE"
        );

        let mut nodes = 0;
        assert_eq!(
            decode_wire_value(
                WireValue::Float64("7ff0000000000000".into()),
                &records,
                0,
                &mut nodes,
            )
            .unwrap_err()
            .code,
            "E_ABI_VALUE"
        );
    }

    #[test]
    fn float64_wire_keeps_signed_zero_and_finite_edge_bits_exact() {
        for bits in [0, 0x8000_0000_0000_0000, 1, 0x7fef_ffff_ffff_ffff] {
            let original = Value::Float64(Float64::new(f64::from_bits(bits)).unwrap());
            let mut nodes = 0;
            let encoded = encode_wire_value(&original, &[], 0, &mut nodes).unwrap();
            assert!(
                matches!(encoded, WireValue::Float64(ref hex) if hex == &format!("{bits:016x}"))
            );
            let json = serde_json::to_vec(&encoded).unwrap();
            let wire: WireValue = serde_json::from_slice(&json).unwrap();
            let mut nodes = 0;
            let decoded = decode_wire_value(wire, &[], 0, &mut nodes).unwrap();
            let Value::Float64(decoded) = decoded else {
                panic!("Float64 wire value changed type");
            };
            assert_eq!(decoded.get().to_bits(), bits);
        }
    }

    #[test]
    fn runtime_resource_values_are_not_transferable() {
        let mut nodes = 0;
        let error = encode_wire_value(
            &Value::NativeResource(Box::new(guion_model::NativeResourceValue::new(
                "test", 1, 1,
            ))),
            &[],
            0,
            &mut nodes,
        )
        .unwrap_err();
        assert_eq!(error.code, "E_ABI_VALUE_UNSUPPORTED");
    }
}
