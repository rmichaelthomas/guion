//! WebAssembly adapter for Guion's production bytecode VM.
//!
//! The adapter decodes a `.gub` artifact with `Artifact::from_bytes` and runs it through the
//! same instruction loop as the native runtime (`guion-vm-core`). It has no filesystem,
//! network, database, clock, or process access: those operations fail with
//! `R_HOST_UNSUPPORTED` instead of being skipped.
//!
//! The exported ABI is the one the AllHereHTML loader already consumes.

use guion_model::{Artifact, Value};
use guion_vm_core::{PortableHost, VmLimits};
use std::{cell::UnsafeCell, sync::Arc};

pub const MAX_ARTIFACT_BYTES: usize = 1024 * 1024;
pub const MAX_OUTPUT_BYTES: usize = 64 * 1024;
pub const MAX_ERROR_BYTES: usize = 1024;
pub const MAX_BROWSER_STEPS: u32 = 10_000;
/// Cumulative guest allocation allowed in the browser, well below the native default.
const MAX_BROWSER_ALLOCATION_BYTES: usize = 16 * 1024 * 1024;

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
    output_length: u32,
    error_length: u32,
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
    output_length: 0,
    error_length: 0,
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

#[no_mangle]
pub extern "C" fn allocate_input(length: u32) -> u32 {
    if length == 0 || length as usize > MAX_ARTIFACT_BYTES {
        return 0;
    }
    let buffer = vec![0_u8; length as usize].into_boxed_slice();
    Box::into_raw(buffer) as *mut u8 as usize as u32
}

#[no_mangle]
pub extern "C" fn release_input(pointer: u32, length: u32) {
    if pointer == 0 || length == 0 || length as usize > MAX_ARTIFACT_BYTES {
        return;
    }
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
