use guion_compiler::compile;
use std::{cell::UnsafeCell, str};

pub const MAX_SOURCE_BYTES: usize = 64 * 1024;
pub const MAX_ARTIFACT_BYTES: usize = 1024 * 1024;
pub const MAX_ERROR_BYTES: usize = 1024;

/// Compile one in-memory Guion source file into a portable `.gub` artifact.
/// Project imports are intentionally unsupported because this adapter has no filesystem.
pub fn compile_source_to_artifact(source_name: &str, source: &str) -> Result<Vec<u8>, String> {
    if source.len() > MAX_SOURCE_BYTES {
        return Err(format!(
            "source exceeds the {MAX_SOURCE_BYTES}-byte browser limit"
        ));
    }

    let artifact = compile(source_name, source).map_err(|error| {
        if error.span.line > 0 {
            format!(
                "{} (line {}, column {}): {}",
                error.code, error.span.line, error.span.column, error.message
            )
        } else {
            format!("{}: {}", error.code, error.message)
        }
    })?;
    let bytes = artifact
        .to_bytes()
        .map_err(|error| format!("{}: {}", error.code, error.message))?;
    if bytes.len() > MAX_ARTIFACT_BYTES {
        return Err(format!(
            "compiled artifact exceeds the {MAX_ARTIFACT_BYTES}-byte browser limit"
        ));
    }
    Ok(bytes)
}

struct CompileResult {
    artifact: [u8; MAX_ARTIFACT_BYTES],
    error: [u8; MAX_ERROR_BYTES],
    artifact_length: u32,
    error_length: u32,
}

struct SharedResult(UnsafeCell<CompileResult>);

// SAFETY: the browser module is single-threaded and calls these exports serially.
unsafe impl Sync for SharedResult {}

static RESULT: SharedResult = SharedResult(UnsafeCell::new(CompileResult {
    artifact: [0; MAX_ARTIFACT_BYTES],
    error: [0; MAX_ERROR_BYTES],
    artifact_length: 0,
    error_length: 0,
}));

fn result() -> &'static mut CompileResult {
    // SAFETY: the module is single-threaded; no result reference is held across calls.
    unsafe { &mut *RESULT.0.get() }
}

fn reset_result() {
    let result = result();
    result.artifact_length = 0;
    result.error_length = 0;
}

fn set_error(message: &str) {
    let result = result();
    let mut end = message.len().min(MAX_ERROR_BYTES);
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    result.error[..end].copy_from_slice(&message.as_bytes()[..end]);
    result.error_length = end as u32;
}

/// Allocate a source buffer for `compile_source`. Returns zero for invalid sizes.
#[no_mangle]
pub extern "C" fn allocate_source(length: u32) -> u32 {
    if length == 0 || length as usize > MAX_SOURCE_BYTES {
        return 0;
    }
    let buffer = vec![0_u8; length as usize].into_boxed_slice();
    Box::into_raw(buffer) as *mut u8 as usize as u32
}

/// Release a buffer returned by `allocate_source`.
#[no_mangle]
pub extern "C" fn release_source(pointer: u32, length: u32) {
    if pointer == 0 || length == 0 || length as usize > MAX_SOURCE_BYTES {
        return;
    }
    // SAFETY: callers return a buffer obtained from `allocate_source` with its original length.
    unsafe {
        let slice = std::ptr::slice_from_raw_parts_mut(pointer as *mut u8, length as usize);
        drop(Box::from_raw(slice));
    }
}

/// Compile the supplied UTF-8 source. Returns zero on success and one on failure.
/// On success, read `result_artifact_ptr` and `result_artifact_len`; otherwise read the error.
///
/// # Safety
/// `pointer` must be a live buffer returned by `allocate_source` with its original length.
#[no_mangle]
pub unsafe extern "C" fn compile_source(pointer: u32, length: u32) -> u32 {
    reset_result();
    if pointer == 0 || length == 0 {
        set_error("source input is empty");
        return 1;
    }
    if length as usize > MAX_SOURCE_BYTES {
        set_error("source exceeds the 65536-byte browser limit");
        return 1;
    }

    let bytes = std::slice::from_raw_parts(pointer as *const u8, length as usize);
    let source = match str::from_utf8(bytes) {
        Ok(source) => source,
        Err(_) => {
            set_error("source input is not valid UTF-8");
            return 1;
        }
    };
    match compile_source_to_artifact("playground.gu", source) {
        Ok(bytes) => {
            let result = result();
            result.artifact[..bytes.len()].copy_from_slice(&bytes);
            result.artifact_length = bytes.len() as u32;
            0
        }
        Err(message) => {
            set_error(&message);
            1
        }
    }
}

#[no_mangle]
pub extern "C" fn result_artifact_ptr() -> u32 {
    result().artifact.as_ptr() as usize as u32
}

#[no_mangle]
pub extern "C" fn result_artifact_len() -> u32 {
    result().artifact_length
}

#[no_mangle]
pub extern "C" fn result_error_ptr() -> u32 {
    result().error.as_ptr() as usize as u32
}

#[no_mangle]
pub extern "C" fn result_error_len() -> u32 {
    result().error_length
}

#[cfg(test)]
mod tests {
    use super::{compile_source_to_artifact, MAX_SOURCE_BYTES};
    use guion_model::Artifact;

    const HELLO_SOURCE: &str = "fn main() -> Int:\n    let greeting = \"Hello, Guion!\"\n    print(greeting)\n    return 0\n";

    #[test]
    fn compiles_single_file_source_into_a_guion_artifact() {
        let bytes = compile_source_to_artifact("playground.gu", HELLO_SOURCE).unwrap();
        let artifact = Artifact::from_bytes(&bytes).unwrap();

        assert_eq!(artifact.format_version, Artifact::CURRENT_VERSION);
        assert_eq!(artifact.entry(), "main");
    }

    #[test]
    fn rejects_source_over_the_browser_input_limit() {
        let source = " ".repeat(MAX_SOURCE_BYTES + 1);

        let error = compile_source_to_artifact("playground.gu", &source).unwrap_err();

        assert!(error.contains("source exceeds"));
    }

    #[test]
    fn compile_errors_report_the_line_and_column() {
        let source = "fn main() -> Int:\n    let total = 2 + \"three\"\n    return 0\n";

        let error = compile_source_to_artifact("playground.gu", source).unwrap_err();

        assert!(error.starts_with("E_TYPE_OPERATOR"), "{error}");
        assert!(error.contains("line 2"), "{error}");
        assert!(error.contains("column"), "{error}");
    }

    #[test]
    fn rejects_imports_that_need_project_files() {
        let source = "import \"tools.gu\"\nfn main() -> Int:\n    return 0\n";

        let error = compile_source_to_artifact("playground.gu", source).unwrap_err();

        assert!(error.contains("E_IMPORT_PROJECT_REQUIRED"));
    }
}
