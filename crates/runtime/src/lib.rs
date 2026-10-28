use guion_model::{
    Artifact, BigInt, FileErrorValue, Float64, HttpErrorValue, JsonErrorValue, NativeResourceValue,
    Op, RuntimeError, SharedJson, Span, TaskErrorValue, Type, Value,
};
pub use guion_vm_core::CancellationToken;
use guion_vm_core::{
    charge_allocation, check_call_depth, cloned_value_allocation, encoded_json_string_len,
    finish_json_value, finish_shared_json, json_exact_integer, new_vm_frame,
    shared_json_push_local_allocation, shared_json_set_local_allocation, value_matches, verify,
    Budget, FrameStart, HostOpContext, OutputStream, RuntimeProfileCounters, VmFrame, VmHost,
    VmLimits,
};
use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    io::Write as IoWrite,
    mem::size_of,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        mpsc, Arc,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
mod json_projection;
mod native_backend;
#[cfg(feature = "python-interop")]
pub mod python_bridge;
pub mod stdlib_data;
pub mod stdlib_services;
use native_backend::{
    NativeBackend, NativeCode, NativeFloatList, NativeIntList, NativeRecord, NativeSymbols,
    NATIVE_INLINE_LIST_ITEMS,
};

struct JsonTextPairRows<'a> {
    groups: &'a BTreeMap<String, Value>,
    key_field: &'a str,
    value_field: &'a str,
    row_count: usize,
}

impl serde::Serialize for JsonTextPairRows<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeSeq;

        let mut sequence = serializer.serialize_seq(Some(self.row_count))?;
        for (key, values) in self.groups {
            let Value::List(values) = values else {
                return Err(serde::ser::Error::custom(
                    "json_stringify_pairs requires Map[Text, List[Text]]",
                ));
            };
            for value in values.iter() {
                let Value::Text(value) = value else {
                    return Err(serde::ser::Error::custom(
                        "json_stringify_pairs requires Map[Text, List[Text]]",
                    ));
                };
                sequence.serialize_element(&JsonTextPair {
                    key_field: self.key_field,
                    key,
                    value_field: self.value_field,
                    value: value.as_ref(),
                })?;
            }
        }
        sequence.end()
    }
}

struct JsonTextPair<'a> {
    key_field: &'a str,
    key: &'a str,
    value_field: &'a str,
    value: &'a str,
}

impl serde::Serialize for JsonTextPair<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;

        let mut object = serializer.serialize_map(Some(2))?;
        object.serialize_entry(self.key_field, self.key)?;
        object.serialize_entry(self.value_field, self.value)?;
        object.end()
    }
}

#[cfg(test)]
mod native_inline_arena_tests {
    use super::{NativeIntList, NativeListArena};

    #[test]
    fn inline_list_addresses_and_values_survive_arena_growth() {
        let mut arena = NativeListArena::new(std::ptr::null(), 10000, 10000, 0, None);
        let first = arena
            .allocate_inline_int(&[10, 20, 30], 1, 99)
            .cast::<NativeIntList>();
        for value in 0..1000 {
            let current = arena
                .allocate_inline_int(&[value, value + 1], 0, value + 2)
                .cast::<NativeIntList>();
            let descriptor = unsafe { &*current };
            assert_eq!(unsafe { *descriptor.data }, value + 2);
        }
        let descriptor = unsafe { &*first };
        assert_eq!(
            unsafe { std::slice::from_raw_parts(descriptor.data, descriptor.len) },
            &[10, 99, 30]
        );
    }
}

// JIT code keeps raw pointers to these descriptors; Box preserves each address when the Vec grows.
#[allow(clippy::vec_box)]
struct NativeListArena {
    float_lists: Vec<Box<NativeFloatList>>,
    float_storage: Vec<Vec<Float64>>,
    int_lists: Vec<Box<NativeIntList>>,
    inline_int_lists: Vec<Vec<NativeIntList>>,
    int_storage: Vec<Vec<i64>>,
    budget: *const Budget,
    profile: Option<Arc<RuntimeProfileCounters>>,
    max_collection_items: usize,
    charged_bytes: usize,
    output_limit_bytes: usize,
    initial_output_bytes: usize,
    pending_output: String,
    pending_print_bytes: Vec<usize>,
}

impl NativeListArena {
    fn new(
        budget: *const Budget,
        max_collection_items: usize,
        output_limit_bytes: usize,
        initial_output_bytes: usize,
        profile: Option<Arc<RuntimeProfileCounters>>,
    ) -> Self {
        Self {
            float_lists: Vec::new(),
            float_storage: Vec::new(),
            int_lists: Vec::new(),
            inline_int_lists: Vec::new(),
            int_storage: Vec::new(),
            budget,
            profile,
            max_collection_items,
            charged_bytes: 0,
            output_limit_bytes,
            initial_output_bytes,
            pending_output: String::new(),
            pending_print_bytes: Vec::new(),
        }
    }

    fn charge(&mut self, bytes: usize) -> bool {
        if self.budget.is_null() || !unsafe { (*self.budget).allocate(bytes) } {
            return false;
        }
        self.charged_bytes = self.charged_bytes.saturating_add(bytes);
        true
    }

    fn allocate_inline_int(
        &mut self,
        source: &[i64],
        index: usize,
        value: i64,
    ) -> *mut std::ffi::c_void {
        const CHUNK_SIZE: usize = 256;
        let mut inline = [0; NATIVE_INLINE_LIST_ITEMS];
        inline[..source.len()].copy_from_slice(source);
        inline[index] = value;
        if self
            .inline_int_lists
            .last()
            .is_none_or(|chunk| chunk.len() == chunk.capacity())
        {
            self.inline_int_lists.push(Vec::with_capacity(CHUNK_SIZE));
        }
        // Each chunk is filled only up to its fixed capacity. Moving its Vec
        // header never moves the backing allocation referenced by JIT code.
        let chunk = self.inline_int_lists.last_mut().unwrap();
        chunk.push(NativeIntList {
            data: std::ptr::null_mut(),
            len: source.len(),
            storage_id: usize::MAX,
            owned: true,
            inline_storage: true,
            inline,
        });
        let descriptor = chunk.last_mut().unwrap();
        descriptor.data = descriptor.inline.as_mut_ptr();
        (descriptor as *mut NativeIntList).cast()
    }

    fn record_list_set_operation(&self) {
        if let Some(profile) = &self.profile {
            profile.record_list_set_operation();
        }
    }

    fn record_list_set_in_place_update(&self) {
        if let Some(profile) = &self.profile {
            profile.record_list_set_in_place_update();
        }
    }

    fn record_list_set_copy(&self, elements: usize, bytes: usize) {
        if let Some(profile) = &self.profile {
            profile.record_list_set_copy(elements, bytes);
        }
    }

    fn record_record_charge(&self, bytes: usize) {
        if let Some(profile) = &self.profile {
            profile.record_record_charge(bytes);
        }
    }

    fn record_guest_conditional_branch(&self) {
        if let Some(profile) = &self.profile {
            profile.record_guest_conditional_branch();
        }
    }

    fn print(&mut self, value: i64, kind: i64) -> bool {
        let rendered = match kind {
            1 => value.to_string(),
            2 => {
                let Some(value) = Float64::new(f64::from_bits(value as u64)) else {
                    return false;
                };
                value.get().to_string()
            }
            3 if value == 0 => "false".to_owned(),
            3 if value == 1 => "true".to_owned(),
            _ => return false,
        };
        let bytes = rendered.len().saturating_add(1);
        if self
            .initial_output_bytes
            .saturating_add(self.pending_output.len())
            .saturating_add(bytes)
            > self.output_limit_bytes
            || !self.charge(bytes)
        {
            return false;
        }
        self.pending_output.push_str(&rendered);
        self.pending_output.push('\n');
        self.pending_print_bytes.push(bytes);
        true
    }
}

const MAX_EFFECT_REPORT_ENTRIES: usize = 1024;
const MAX_EFFECT_TARGET_CHARS: usize = 256;
const DEFAULT_ALLOCATION_LIMIT_BYTES: usize = 256 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectKind {
    ProgramOutput,
    ProgramErrorOutput,
    FileRead,
    FileWrite,
    HttpRequest,
    PythonCall,
    EnvironmentRead,
    ClockRead,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectOutcome {
    Started,
    Returned,
    ReturnedError,
    Interrupted,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectRecord {
    pub kind: EffectKind,
    pub target: String,
    pub target_truncated: bool,
    pub outcome: EffectOutcome,
}

#[derive(Clone, Debug)]
pub struct HostPolicy {
    pub instruction_limit: u64,
    /// Cumulative estimated guest-data allocations allowed during one invocation.
    /// Interpreter/JIT frames, operand stacks, and scalar arithmetic temporaries are excluded.
    pub allocation_limit_bytes: usize,
    pub call_depth_limit: usize,
    pub output_limit_bytes: usize,
    pub max_integer_digits: usize,
    pub max_text_bytes: usize,
    pub max_collection_items: usize,
    pub max_parallelism: usize,
    pub file_root: Option<PathBuf>,
    pub additional_file_roots: Vec<PathBuf>,
    /// Virtual path prefixes mapped to independently granted, root-confined file areas.
    pub file_root_aliases: HashMap<String, PathBuf>,
    pub allow_file_read: bool,
    pub allow_file_write: bool,
    pub max_file_bytes: usize,
    pub arguments: Vec<String>,
    pub allowed_environment_variables: HashSet<String>,
    /// Whether wall-clock reads are explicitly granted to the guest.
    pub allow_clock: bool,
    /// Optional fixed Unix-millisecond value for deterministic embedders and tests.
    pub clock_unix_time_ms_override: Option<i64>,
    #[cfg(feature = "python-interop")]
    pub python_interpreter: Option<PathBuf>,
    #[cfg(feature = "python-interop")]
    pub python_timeout: Duration,
    pub allowed_http_hosts: HashSet<String>,
    pub http_timeout: Duration,
    pub max_http_request_bytes: usize,
    pub max_http_response_bytes: usize,
    pub max_http_redirects: usize,
}
impl Default for HostPolicy {
    fn default() -> Self {
        Self {
            instruction_limit: 2_000_000,
            allocation_limit_bytes: DEFAULT_ALLOCATION_LIMIT_BYTES,
            call_depth_limit: 128,
            output_limit_bytes: 1_000_000,
            max_integer_digits: 4096,
            max_text_bytes: 16 * 1024 * 1024,
            max_collection_items: 1_000_000,
            max_parallelism: thread::available_parallelism()
                .map(usize::from)
                .unwrap_or(1)
                .min(64),
            file_root: None,
            additional_file_roots: Vec::new(),
            file_root_aliases: HashMap::new(),
            allow_file_read: false,
            allow_file_write: false,
            max_file_bytes: 16 * 1024 * 1024,
            arguments: Vec::new(),
            allowed_environment_variables: HashSet::new(),
            allow_clock: false,
            clock_unix_time_ms_override: None,
            #[cfg(feature = "python-interop")]
            python_interpreter: None,
            #[cfg(feature = "python-interop")]
            python_timeout: Duration::from_secs(15),
            allowed_http_hosts: HashSet::new(),
            http_timeout: Duration::from_secs(15),
            max_http_request_bytes: 1024 * 1024,
            max_http_response_bytes: 16 * 1024 * 1024,
            max_http_redirects: 5,
        }
    }
}
impl HostPolicy {
    pub fn with_file_access(mut self, root: impl Into<PathBuf>, read: bool, write: bool) -> Self {
        self.file_root = Some(root.into());
        self.allow_file_read = read;
        self.allow_file_write = write;
        self
    }
    /// Add a secondary relative-path root. Reads search the primary root first;
    /// writes and Python bridge relative paths use the last granted root.
    pub fn with_additional_file_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.additional_file_roots.push(root.into());
        self
    }
    /// Map a virtual relative-path prefix (for example `std` or `pkg:codec`)
    /// to its own granted root. Paths under that prefix never fall through to
    /// the project root or another additional root.
    pub fn with_file_root_alias(
        mut self,
        alias: impl Into<String>,
        root: impl Into<PathBuf>,
    ) -> Self {
        self.file_root_aliases.insert(alias.into(), root.into());
        self
    }
    pub fn with_arguments(mut self, arguments: Vec<String>) -> Self {
        self.arguments = arguments;
        self
    }
    pub fn with_clock_access(mut self) -> Self {
        self.allow_clock = true;
        self
    }
    /// Grant clock access and provide a fixed Unix-millisecond value.
    pub fn with_clock_unix_time_ms(mut self, unix_time_ms: i64) -> Self {
        self.allow_clock = true;
        self.clock_unix_time_ms_override = Some(unix_time_ms);
        self
    }
    #[cfg(feature = "python-interop")]
    pub fn with_python(mut self, interpreter: impl Into<PathBuf>, timeout: Duration) -> Self {
        self.python_interpreter = Some(interpreter.into());
        self.python_timeout = timeout;
        self
    }
    pub fn with_network_hosts<I, S>(
        mut self,
        hosts: I,
        timeout: Duration,
        max_response_bytes: usize,
    ) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.allowed_http_hosts = hosts
            .into_iter()
            .map(|host| host.as_ref().trim_end_matches('.').to_ascii_lowercase())
            .collect();
        self.http_timeout = timeout;
        self.max_http_response_bytes = max_response_bytes;
        self
    }
}

pub struct Runtime {
    native_enabled: bool,
    runtime_profile_enabled: bool,
}

fn current_unix_time_ms() -> Result<i64, String> {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => i64::try_from(duration.as_millis())
            .map_err(|_| "system clock is outside the supported millisecond range".into()),
        Err(error) => {
            let milliseconds = i64::try_from(error.duration().as_millis()).map_err(|_| {
                "system clock is outside the supported millisecond range".to_string()
            })?;
            Ok(-milliseconds)
        }
    }
}

#[cfg(unix)]
fn local_date_from_unix_time_ms(unix_time_ms: i64) -> Result<(i32, i32, i32), String> {
    let seconds = unix_time_ms.div_euclid(1_000);
    let timestamp = libc::time_t::try_from(seconds)
        .map_err(|_| "timestamp is outside the host local-time range".to_string())?;
    // localtime_r writes to the supplied tm and does not expose shared static storage.
    let mut local = unsafe { std::mem::zeroed::<libc::tm>() };
    if unsafe { libc::localtime_r(&timestamp, &mut local) }.is_null() {
        return Err("host could not convert the timestamp to local time".into());
    }
    let year = local
        .tm_year
        .checked_add(1_900)
        .ok_or_else(|| "local year is outside the supported range".to_string())?;
    let month = local
        .tm_mon
        .checked_add(1)
        .ok_or_else(|| "local month is outside the supported range".to_string())?;
    Ok((year, month, local.tm_mday))
}

#[cfg(not(unix))]
fn local_date_from_unix_time_ms(_unix_time_ms: i64) -> Result<(i32, i32, i32), String> {
    Err("local date lookup is supported on Unix hosts only".into())
}

fn share_artifact(artifact: Artifact) -> Arc<Artifact> {
    Arc::new(artifact)
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new()
    }
}
impl Runtime {
    pub fn new() -> Self {
        Self {
            native_enabled: true,
            runtime_profile_enabled: false,
        }
    }

    /// Create a runtime that always uses the verified bytecode VM.
    /// This is useful for reproducible backend comparisons and diagnosis.
    pub fn vm_only() -> Self {
        Self {
            native_enabled: false,
            runtime_profile_enabled: false,
        }
    }

    /// Enable diagnostic execution counters for the latest guest invocation.
    /// This adds instrumentation and should be used only for untimed profiles.
    pub fn with_runtime_profile(mut self) -> Self {
        self.runtime_profile_enabled = true;
        self
    }
    pub fn load(
        &self,
        artifact: Artifact,
        mut policy: HostPolicy,
    ) -> Result<Instance, RuntimeError> {
        verify(&artifact)?;
        if let Some(metadata) = &artifact.build_metadata {
            let limits = &metadata.resource_limits;
            policy.instruction_limit = policy.instruction_limit.min(limits.instruction_limit);
            policy.allocation_limit_bytes = policy
                .allocation_limit_bytes
                .min(limits.allocation_limit_bytes);
            policy.call_depth_limit = policy.call_depth_limit.min(limits.call_depth_limit);
            policy.output_limit_bytes = policy.output_limit_bytes.min(limits.output_limit_bytes);
            policy.max_integer_digits = policy.max_integer_digits.min(limits.max_integer_digits);
            policy.max_text_bytes = policy.max_text_bytes.min(limits.max_text_bytes);
            policy.max_collection_items =
                policy.max_collection_items.min(limits.max_collection_items);
            policy.max_parallelism = policy.max_parallelism.min(limits.max_parallelism);
            policy.max_file_bytes = policy.max_file_bytes.min(limits.max_file_bytes);
        }
        let mut database_roots = Vec::with_capacity(
            1 + policy.additional_file_roots.len() + policy.file_root_aliases.len(),
        );
        if let Some(root) = &policy.file_root {
            database_roots.push(root.clone());
        }
        database_roots.extend(policy.additional_file_roots.iter().cloned());
        database_roots.extend(policy.file_root_aliases.values().cloned());
        let artifact = share_artifact(artifact);
        Ok(Instance {
            artifact,
            native: None,
            native_code: None,
            native_enabled: self.native_enabled,
            runtime_profile_enabled: self.runtime_profile_enabled,
            runtime_profile_metrics: None,
            native_calls: Arc::new(AtomicUsize::new(0)),
            native_deoptimizations: Arc::new(AtomicUsize::new(0)),
            native_compilations: Arc::new(AtomicUsize::new(0)),
            readers: stdlib_data::FileReaderRegistry::new(
                policy.max_file_bytes,
                policy.max_file_bytes,
            ),
            databases: stdlib_services::DatabaseRegistry::with_roots(
                database_roots,
                policy.max_file_bytes,
                policy.max_collection_items,
                2_000,
            ),
            http: None,
            toml_documents: stdlib_services::TomlRegistry::new(policy.max_text_bytes),
            policy,
            output: String::new(),
            program_stderr: String::new(),
            closed: false,
            worker_threads: HashSet::new(),
            cancelled: None,
            #[cfg(feature = "python-interop")]
            python_bridge: None,
            host_stderr: String::new(),
            effect_report: Vec::new(),
            omitted_effects: 0,
            python_native_image_observation: "not_used".into(),
            python_native_images: Vec::new(),
        })
    }
}

#[derive(Debug)]
pub struct Instance {
    artifact: Arc<Artifact>,
    native: Option<NativeBackend>,
    native_code: Option<Arc<NativeCode>>,
    native_enabled: bool,
    runtime_profile_enabled: bool,
    runtime_profile_metrics: Option<Arc<RuntimeProfileCounters>>,
    native_calls: Arc<AtomicUsize>,
    native_deoptimizations: Arc<AtomicUsize>,
    native_compilations: Arc<AtomicUsize>,
    policy: HostPolicy,
    readers: stdlib_data::FileReaderRegistry,
    databases: stdlib_services::DatabaseRegistry,
    http: Option<Arc<HostHttpClient>>,
    toml_documents: stdlib_services::TomlRegistry,
    output: String,
    program_stderr: String,
    closed: bool,
    worker_threads: HashSet<String>,
    cancelled: Option<Arc<AtomicBool>>,
    #[cfg(feature = "python-interop")]
    python_bridge: Option<python_bridge::PythonBridge>,
    host_stderr: String,
    effect_report: Vec<EffectRecord>,
    omitted_effects: usize,
    python_native_image_observation: String,
    python_native_images: Vec<String>,
}

impl HostPolicy {
    fn vm_limits(&self) -> VmLimits {
        VmLimits {
            allocation_limit_bytes: self.allocation_limit_bytes,
            call_depth_limit: self.call_depth_limit,
            output_limit_bytes: self.output_limit_bytes,
            max_integer_digits: self.max_integer_digits,
            max_text_bytes: self.max_text_bytes,
            max_collection_items: self.max_collection_items,
            max_file_bytes: self.max_file_bytes,
        }
    }
}

fn value_limit_error(value: &Value, policy: &HostPolicy, span: Span) -> Option<RuntimeError> {
    guion_vm_core::value_limit_error(value, &policy.vm_limits(), span)
}

fn consume_sort_step(budget: &Budget, span: Span) -> Result<(), RuntimeError> {
    if budget.is_cancelled() {
        return Err(RuntimeError::new(
            "R_CANCELLED",
            "parallel task was cancelled after a sibling failed",
            span,
        ));
    }
    if !budget.consume() {
        return Err(RuntimeError::new(
            "R_STEP_LIMIT",
            "instruction budget exhausted",
            span,
        ));
    }
    Ok(())
}

fn value_limit_error_at_boundary(
    value: &Value,
    policy: &HostPolicy,
    span: Span,
) -> Option<RuntimeError> {
    guion_vm_core::value_limit_error_at_boundary(value, &policy.vm_limits(), span)
}

impl VmHost for Instance {
    fn artifact(&self) -> &Arc<Artifact> {
        &self.artifact
    }

    fn limits(&self) -> VmLimits {
        self.policy.vm_limits()
    }

    fn externally_cancelled(&self) -> bool {
        self.cancelled
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Acquire))
    }

    fn program_output_bytes(&self) -> usize {
        self.output.len().saturating_add(self.program_stderr.len())
    }

    fn write_program_output(
        &mut self,
        stream: OutputStream,
        text: &str,
        _span: Span,
    ) -> Result<(), RuntimeError> {
        let (destination, kind) = match stream {
            OutputStream::Stdout => (&mut self.output, EffectKind::ProgramOutput),
            OutputStream::Stderr => (&mut self.program_stderr, EffectKind::ProgramErrorOutput),
        };
        destination.push_str(text);
        let effect = self.begin_effect(kind, format!("{} bytes", text.len()));
        self.finish_effect(effect, EffectOutcome::Returned);
        Ok(())
    }

    fn start_frame(
        &mut self,
        index: usize,
        args: Vec<Value>,
        depth: usize,
        budget: &Arc<Budget>,
        call_site: Option<Span>,
    ) -> Result<FrameStart, RuntimeError> {
        self.start_guest_frame(index, args, depth, budget.clone(), call_site)
    }

    fn host_op(&mut self, context: HostOpContext<'_>) -> Result<(), RuntimeError> {
        let HostOpContext {
            instruction,
            function: _,
            stack,
            locals,
            budget,
            depth,
        } = context;
        let budget = budget.clone();
        macro_rules! push_stack {
            ($value:expr) => {{
                let value = $value;
                if stack.len() == stack.capacity() {
                    let next_capacity = if stack.capacity() == 0 {
                        4
                    } else {
                        stack.capacity().saturating_mul(2)
                    };
                    stack.reserve_exact(next_capacity.saturating_sub(stack.len()));
                }
                stack.push(value);
            }};
        }
        macro_rules! pop {
            () => {
                stack.pop().ok_or_else(|| {
                    RuntimeError::new(
                        "R_STACK_INVALID",
                        "verified bytecode stack underflow",
                        instruction.span,
                    )
                })?
            };
        }
        match instruction.op {
            Op::JsonPushLocal(slot) => {
                let value = match pop!() {
                    Value::Json(value) => value,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_push value must be Json",
                            instruction.span,
                        ))
                    }
                };
                let local = locals.get_mut(slot).ok_or_else(|| {
                    RuntimeError::new(
                        "R_LOCAL_UNINITIALIZED",
                        "json_push assignment targets an uninitialized local",
                        instruction.span,
                    )
                })?;
                let Value::Json(array) = local else {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "json_push assignment target must be Json",
                        instruction.span,
                    ));
                };
                let Some(items) = array.as_array() else {
                    return Err(RuntimeError::new(
                        "R_JSON_TYPE",
                        "json_push requires a JSON array",
                        instruction.span,
                    ));
                };
                let len = items.len();
                if len >= self.policy.max_collection_items {
                    return Err(RuntimeError::new(
                        "R_COLLECTION_LIMIT",
                        "JSON array exceeds the configured item limit",
                        instruction.span,
                    ));
                }
                charge_allocation(
                    budget.as_ref(),
                    shared_json_push_local_allocation(array),
                    instruction.span,
                )?;
                Arc::make_mut(array)
                    .as_array_mut()
                    .expect("array type was checked before copy-on-write")
                    .push(value);
                if let Some(error) = value_limit_error(local, &self.policy, instruction.span) {
                    return Err(error);
                }
            }
            Op::JsonSetLocal(slot) => {
                let value = match pop!() {
                    Value::Json(value) => value,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_set value must be Json",
                            instruction.span,
                        ))
                    }
                };
                let key = match pop!() {
                    Value::Text(key) => key,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_set key must be Text",
                            instruction.span,
                        ))
                    }
                };
                if key.len() > self.policy.max_text_bytes {
                    return Err(RuntimeError::new(
                        "R_TEXT_LIMIT",
                        "JSON key exceeds the configured byte limit",
                        instruction.span,
                    ));
                }
                let local = locals.get_mut(slot).ok_or_else(|| {
                    RuntimeError::new(
                        "R_LOCAL_UNINITIALIZED",
                        "json_set assignment targets an uninitialized local",
                        instruction.span,
                    )
                })?;
                let Value::Json(object) = local else {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "json_set assignment target must be Json",
                        instruction.span,
                    ));
                };
                let Some(fields) = object.as_object() else {
                    return Err(RuntimeError::new(
                        "R_JSON_TYPE",
                        "json_set requires an object",
                        instruction.span,
                    ));
                };
                if !fields.contains_key(key.as_ref())
                    && fields.len() >= self.policy.max_collection_items
                {
                    return Err(RuntimeError::new(
                        "R_COLLECTION_LIMIT",
                        "JSON object exceeds the configured item limit",
                        instruction.span,
                    ));
                }
                charge_allocation(
                    budget.as_ref(),
                    shared_json_set_local_allocation(object, &key),
                    instruction.span,
                )?;
                Arc::make_mut(object)
                    .as_object_mut()
                    .expect("object type was checked before copy-on-write")
                    .insert(key.to_string(), value);
                if let Some(error) = value_limit_error(local, &self.policy, instruction.span) {
                    return Err(error);
                }
            }
            Op::ParallelMap(target) => {
                let limit = match pop!() {
                    Value::Int(n) => n.to_usize(),
                    _ => None,
                }
                .filter(|n| *n > 0)
                .ok_or_else(|| {
                    RuntimeError::new(
                        "R_PARALLEL_LIMIT",
                        "parallel_map worker limit must be a positive machine-sized integer",
                        instruction.span,
                    )
                })?;
                if limit > self.policy.max_parallelism {
                    return Err(RuntimeError::new(
                        "R_PARALLEL_LIMIT",
                        format!(
                            "worker limit {limit} exceeds host limit {}",
                            self.policy.max_parallelism
                        ),
                        instruction.span,
                    ));
                }
                let values = match pop!() {
                    Value::List(values) => values.as_ref().clone(),
                    Value::Float64List(values) => {
                        values.iter().copied().map(Value::Float64).collect()
                    }
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "parallel_map requires a List",
                            instruction.span,
                        ))
                    }
                };
                let input_copy_bytes = size_of::<Vec<Value>>()
                    .saturating_add(values.len().saturating_mul(size_of::<Value>()))
                    .saturating_add(
                        values
                            .iter()
                            .map(cloned_value_allocation)
                            .fold(0usize, usize::saturating_add),
                    );
                charge_allocation(budget.as_ref(), input_copy_bytes, instruction.span)?;
                match self.parallel_map(
                    target,
                    values,
                    limit,
                    depth + 1,
                    budget.clone(),
                    instruction.span,
                ) {
                    Ok(Ok(results)) => {
                        push_stack!(Value::ResultOk(Box::new(Value::from_list_items(results))))
                    }
                    Ok(Err(error)) => {
                        push_stack!(Value::ResultErr(Box::new(Value::TaskError(error))))
                    }
                    Err(mut error) => {
                        error.trace.push(instruction.span);
                        return Err(error);
                    }
                }
            }
            Op::SortBy(target) => {
                let mut values = match pop!() {
                    Value::List(values) => values.as_ref().clone(),
                    Value::Float64List(values) => {
                        values.iter().copied().map(Value::Float64).collect()
                    }
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "sort_by requires a List",
                            instruction.span,
                        ))
                    }
                };
                let allocation = size_of::<Vec<Value>>()
                    .saturating_add(values.len().saturating_mul(size_of::<Value>()))
                    .saturating_add(
                        values
                            .iter()
                            .map(cloned_value_allocation)
                            .fold(0usize, usize::saturating_add),
                    );
                charge_allocation(budget.as_ref(), allocation, instruction.span)?;
                if values.len() <= 32 {
                    for index in 1..values.len() {
                        let value = values[index].clone();
                        let mut position = index;
                        while position > 0 {
                            let ordering = match self.invoke_inner(
                                target,
                                vec![value.clone(), values[position - 1].clone()],
                                depth + 1,
                                budget.clone(),
                            ) {
                                Ok(Value::Int(ordering)) => ordering,
                                Ok(_) => {
                                    return Err(RuntimeError::new(
                                        "R_TYPE_INVALID",
                                        "sort comparator must return Int",
                                        instruction.span,
                                    ))
                                }
                                Err(mut error) => {
                                    error.trace.push(instruction.span);
                                    return Err(error);
                                }
                            };
                            if ordering.is_negative() {
                                values[position] = values[position - 1].clone();
                                position -= 1;
                            } else {
                                break;
                            }
                        }
                        values[position] = value;
                    }
                } else {
                    let length = values.len();
                    let index_storage = size_of::<Vec<usize>>()
                        .saturating_mul(2)
                        .saturating_add(size_of::<Vec<bool>>())
                        .saturating_add(length.saturating_mul(size_of::<usize>()).saturating_mul(2))
                        .saturating_add(length);
                    charge_allocation(budget.as_ref(), index_storage, instruction.span)?;

                    let mut order: Vec<usize> = (0..length).collect();
                    let mut merged = Vec::with_capacity(length);
                    let mut width = 1usize;
                    while width < length {
                        merged.clear();
                        let mut run_start = 0usize;
                        while run_start < length {
                            let middle = run_start.saturating_add(width).min(length);
                            let run_end = middle.saturating_add(width).min(length);
                            let (mut left, mut right) = (run_start, middle);
                            while left < middle && right < run_end {
                                let take_left = self.sort_by_left_before_or_equal(
                                    target,
                                    &values[order[left]],
                                    &values[order[right]],
                                    depth,
                                    budget.clone(),
                                    instruction.span,
                                )?;
                                if take_left {
                                    merged.push(order[left]);
                                    left += 1;
                                } else {
                                    merged.push(order[right]);
                                    right += 1;
                                }
                            }
                            merged.extend_from_slice(&order[left..middle]);
                            merged.extend_from_slice(&order[right..run_end]);
                            run_start = run_end;
                        }
                        std::mem::swap(&mut order, &mut merged);
                        width = width.saturating_mul(2);
                    }

                    let mut visited = vec![false; length];
                    for start in 0..length {
                        if visited[start] {
                            continue;
                        }
                        let mut current = start;
                        loop {
                            visited[current] = true;
                            let next = order[current];
                            if next == start {
                                break;
                            }
                            values.swap(current, next);
                            current = next;
                        }
                    }
                }
                push_stack!(Value::from_list_items(values));
            }
            Op::SortByIntKey(target) => {
                let mut values = match pop!() {
                    Value::List(values) => values.as_ref().clone(),
                    Value::Float64List(values) => {
                        values.iter().copied().map(Value::Float64).collect()
                    }
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "sort_by_int_key requires a List",
                            instruction.span,
                        ))
                    }
                };
                let length = values.len();
                let value_allocation = size_of::<Vec<Value>>()
                    .saturating_add(length.saturating_mul(size_of::<Value>()))
                    .saturating_add(
                        values
                            .iter()
                            .map(cloned_value_allocation)
                            .fold(0usize, usize::saturating_add),
                    );
                let sort_storage = size_of::<Vec<BigInt>>()
                    .saturating_add(size_of::<Vec<usize>>().saturating_mul(2))
                    .saturating_add(size_of::<Vec<bool>>())
                    .saturating_add(
                        length.saturating_mul(
                            size_of::<BigInt>()
                                .saturating_add(size_of::<usize>().saturating_mul(2))
                                .saturating_add(size_of::<bool>()),
                        ),
                    );
                charge_allocation(
                    budget.as_ref(),
                    value_allocation.saturating_add(sort_storage),
                    instruction.span,
                )?;

                let mut keys = Vec::with_capacity(length);
                for value in &values {
                    match self.invoke_inner(target, vec![value.clone()], depth + 1, budget.clone())
                    {
                        Ok(Value::Int(key)) => keys.push(key),
                        Ok(_) => {
                            return Err(RuntimeError::new(
                                "R_TYPE_INVALID",
                                "sort_by_int_key key function must return Int",
                                instruction.span,
                            ))
                        }
                        Err(mut error) => {
                            error.trace.push(instruction.span);
                            return Err(error);
                        }
                    }
                }

                let mut order: Vec<usize> = (0..length).collect();
                if length <= 32 {
                    for index in 1..length {
                        let value = order[index];
                        let mut position = index;
                        while position > 0 {
                            consume_sort_step(budget.as_ref(), instruction.span)?;
                            if keys[value] < keys[order[position - 1]] {
                                order[position] = order[position - 1];
                                position -= 1;
                            } else {
                                break;
                            }
                        }
                        order[position] = value;
                    }
                } else {
                    let mut merged = Vec::with_capacity(length);
                    let mut width = 1usize;
                    while width < length {
                        merged.clear();
                        let mut run_start = 0usize;
                        while run_start < length {
                            let middle = run_start.saturating_add(width).min(length);
                            let run_end = middle.saturating_add(width).min(length);
                            let (mut left, mut right) = (run_start, middle);
                            while left < middle && right < run_end {
                                consume_sort_step(budget.as_ref(), instruction.span)?;
                                if keys[order[left]] <= keys[order[right]] {
                                    merged.push(order[left]);
                                    left += 1;
                                } else {
                                    merged.push(order[right]);
                                    right += 1;
                                }
                            }
                            merged.extend_from_slice(&order[left..middle]);
                            merged.extend_from_slice(&order[right..run_end]);
                            run_start = run_end;
                        }
                        std::mem::swap(&mut order, &mut merged);
                        width = width.saturating_mul(2);
                    }
                }

                let mut visited = vec![false; length];
                for start in 0..length {
                    if visited[start] {
                        continue;
                    }
                    let mut current = start;
                    loop {
                        visited[current] = true;
                        let next = order[current];
                        if next == start {
                            break;
                        }
                        values.swap(current, next);
                        current = next;
                    }
                }
                push_stack!(Value::from_list_items(values));
            }
            Op::MergeSortedByIntKey(target) => {
                let input_feeds = match pop!() {
                    Value::List(feeds) => feeds.as_ref().clone(),
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "merge_sorted_by_int_key requires a List of Lists",
                            instruction.span,
                        ))
                    }
                };
                let mut feeds = Vec::with_capacity(input_feeds.len());
                let mut total = 0usize;
                for feed in input_feeds {
                    let values: Arc<Vec<Value>> = match feed {
                        Value::List(values) => values,
                        Value::Float64List(values) => {
                            Arc::new(values.iter().copied().map(Value::Float64).collect())
                        }
                        _ => {
                            return Err(RuntimeError::new(
                                "R_TYPE_INVALID",
                                "merge_sorted_by_int_key requires each feed to be a List",
                                instruction.span,
                            ))
                        }
                    };
                    total = total.saturating_add(values.len());
                    feeds.push(values);
                }
                if total > self.policy.max_collection_items {
                    return Err(RuntimeError::new(
                        "R_COLLECTION_LIMIT",
                        "merged list exceeds the configured collection limit",
                        instruction.span,
                    ));
                }
                let value_allocation = size_of::<Vec<Value>>()
                    .saturating_add(total.saturating_mul(size_of::<Value>()))
                    .saturating_add(
                        feeds
                            .iter()
                            .flat_map(|feed| feed.iter())
                            .map(cloned_value_allocation)
                            .fold(0usize, usize::saturating_add),
                    );
                let key_storage = size_of::<Vec<Arc<Vec<Value>>>>()
                    .saturating_add(feeds.len().saturating_mul(size_of::<Arc<Vec<Value>>>()))
                    .saturating_add(size_of::<Vec<Vec<BigInt>>>())
                    .saturating_add(feeds.len().saturating_mul(size_of::<Vec<BigInt>>()))
                    .saturating_add(total.saturating_mul(size_of::<BigInt>()))
                    .saturating_add(size_of::<Vec<usize>>())
                    .saturating_add(feeds.len().saturating_mul(size_of::<usize>()));
                charge_allocation(
                    budget.as_ref(),
                    value_allocation.saturating_add(key_storage),
                    instruction.span,
                )?;

                let mut keys = Vec::with_capacity(feeds.len());
                for feed in &feeds {
                    let mut feed_keys = Vec::with_capacity(feed.len());
                    for value in feed.iter() {
                        match self.invoke_inner(
                            target,
                            vec![value.clone()],
                            depth + 1,
                            budget.clone(),
                        ) {
                            Ok(Value::Int(key)) => feed_keys.push(key),
                            Ok(_) => {
                                return Err(RuntimeError::new(
                                    "R_TYPE_INVALID",
                                    "merge_sorted_by_int_key key function must return Int",
                                    instruction.span,
                                ))
                            }
                            Err(mut error) => {
                                error.trace.push(instruction.span);
                                return Err(error);
                            }
                        }
                    }
                    for index in 1..feed_keys.len() {
                        consume_sort_step(budget.as_ref(), instruction.span)?;
                        if feed_keys[index] < feed_keys[index - 1] {
                            return Err(RuntimeError::new(
                                "R_SORT_ORDER",
                                "merge_sorted_by_int_key feeds must already be sorted by key",
                                instruction.span,
                            ));
                        }
                    }
                    keys.push(feed_keys);
                }

                let mut positions = vec![0usize; feeds.len()];
                let mut output = Vec::with_capacity(total);
                while output.len() < total {
                    let mut selected: Option<usize> = None;
                    for feed_index in 0..feeds.len() {
                        let position = positions[feed_index];
                        if position >= feeds[feed_index].len() {
                            continue;
                        }
                        if let Some(best) = selected {
                            consume_sort_step(budget.as_ref(), instruction.span)?;
                            if keys[feed_index][position] < keys[best][positions[best]] {
                                selected = Some(feed_index);
                            }
                        } else {
                            selected = Some(feed_index);
                        }
                    }
                    let feed_index = selected.expect("remaining merge item must have a feed");
                    let position = positions[feed_index];
                    output.push(feeds[feed_index][position].clone());
                    positions[feed_index] = position + 1;
                }
                push_stack!(Value::from_list_items(output));
            }
            Op::HttpGet => {
                let url = match pop!() {
                    Value::Text(url) => url,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "http_get URL must be Text",
                            instruction.span,
                        ))
                    }
                };
                let http = self.shared_http_client(instruction.span)?;
                let effect =
                    self.begin_effect(EffectKind::HttpRequest, http_effect_target("GET", &url));
                match perform_http_get(&http, &self.policy, &url, budget.as_ref()) {
                    Ok(text) => {
                        self.finish_effect(effect, EffectOutcome::Returned);
                        charge_allocation(
                            budget.as_ref(),
                            size_of::<String>().saturating_add(text.len()),
                            instruction.span,
                        )?;
                        push_stack!(Value::ResultOk(Box::new(Value::Text(text.into()))));
                    }
                    Err(mut error) => {
                        if budget.is_cancelled() {
                            self.finish_effect(effect, EffectOutcome::Interrupted);
                            return Ok(());
                        }
                        self.finish_effect(effect, EffectOutcome::ReturnedError);
                        error.trace.push(instruction.span);
                        push_stack!(Value::ResultErr(Box::new(Value::HttpError(error))));
                    }
                }
            }
            Op::HttpRequest | Op::HttpRequestWithTimeout | Op::HttpRequestBytes => {
                let byte_response = matches!(&instruction.op, Op::HttpRequestBytes);
                let has_timeout = matches!(
                    &instruction.op,
                    Op::HttpRequestWithTimeout | Op::HttpRequestBytes
                );
                let request_timeout = if has_timeout {
                    match pop!() {
                        Value::Int(milliseconds) => match milliseconds
                            .to_usize()
                            .and_then(|value| u64::try_from(value).ok())
                        {
                            Some(value) if value > 0 => {
                                self.policy.http_timeout.min(Duration::from_millis(value))
                            }
                            _ => {
                                let mut error = http_error(
                                    "R_HTTP_TIMEOUT",
                                    "per-request timeout_ms must be a positive 64-bit integer",
                                );
                                error.trace.push(instruction.span);
                                push_stack!(Value::ResultErr(Box::new(Value::HttpError(error))));
                                return Ok(());
                            }
                        },
                        _ => {
                            return Err(RuntimeError::new(
                                "R_TYPE_INVALID",
                                "HTTP timeout_ms must be Int",
                                instruction.span,
                            ))
                        }
                    }
                } else {
                    self.policy.http_timeout
                };
                let body = match pop!() {
                    Value::Text(body) if !byte_response => body.as_bytes().to_vec(),
                    Value::Bytes(body) if byte_response => body.as_ref().clone(),
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            if byte_response {
                                "http_request_bytes body must be Bytes"
                            } else {
                                "http_request body must be Text"
                            },
                            instruction.span,
                        ))
                    }
                };
                let headers = match pop!() {
                    Value::Map(headers) => {
                        let mut decoded = BTreeMap::new();
                        for (name, value) in headers.iter() {
                            let Value::Text(value) = value else {
                                return Err(RuntimeError::new(
                                    "R_TYPE_INVALID",
                                    "http_request headers must contain Text values",
                                    instruction.span,
                                ));
                            };
                            decoded.insert(name.clone(), value.to_string());
                        }
                        decoded
                    }
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "http_request headers must be Map[Text]",
                            instruction.span,
                        ))
                    }
                };
                let url = match pop!() {
                    Value::Text(url) => url,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "http_request URL must be Text",
                            instruction.span,
                        ))
                    }
                };
                let method = match pop!() {
                    Value::Text(method) => method,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "http_request method must be Text",
                            instruction.span,
                        ))
                    }
                };
                let http = self.shared_http_client(instruction.span)?;
                let effect =
                    self.begin_effect(EffectKind::HttpRequest, http_effect_target(&method, &url));
                match perform_http_request_bytes(
                    &http,
                    &self.policy,
                    HttpRequestBytes {
                        method: &method,
                        input: &url,
                        headers: &headers,
                        body: &body,
                        timeout: request_timeout,
                    },
                    budget.as_ref(),
                ) {
                    Ok(response) => {
                        if byte_response {
                            self.finish_effect(effect, EffectOutcome::Returned);
                            let size = response.body.len().saturating_add(
                                response
                                    .headers
                                    .iter()
                                    .map(|(k, v)| k.len() + v.len())
                                    .sum::<usize>(),
                            );
                            charge_allocation(budget.as_ref(), size, instruction.span)?;
                            let response_headers = response
                                .headers
                                .into_iter()
                                .map(|(key, value)| (key, Value::Text(value.into())))
                                .collect();
                            push_stack!(Value::ResultOk(Box::new(Value::Record(
                                "HttpResponse".into(),
                                Arc::new(vec![
                                    Value::Int(BigInt::from_i64(response.status as i64)),
                                    Value::Map(Arc::new(response_headers)),
                                    Value::Bytes(Arc::new(response.body)),
                                ]),
                            ))));
                        } else {
                            match response.into_text_json(self.policy.max_text_bytes) {
                                Ok(response) => {
                                    self.finish_effect(effect, EffectOutcome::Returned);
                                    let value = finish_json_value(
                                        response,
                                        &self.policy.vm_limits(),
                                        budget.as_ref(),
                                        instruction.span,
                                    )?;
                                    push_stack!(Value::ResultOk(Box::new(value)));
                                }
                                Err(mut error) => {
                                    self.finish_effect(effect, EffectOutcome::ReturnedError);
                                    error.trace.push(instruction.span);
                                    push_stack!(Value::ResultErr(Box::new(Value::HttpError(
                                        error
                                    ))));
                                }
                            }
                        }
                    }
                    Err(mut error) => {
                        if budget.is_cancelled() {
                            self.finish_effect(effect, EffectOutcome::Interrupted);
                            return Ok(());
                        }
                        self.finish_effect(effect, EffectOutcome::ReturnedError);
                        error.trace.push(instruction.span);
                        push_stack!(Value::ResultErr(Box::new(Value::HttpError(error))));
                    }
                }
            }
            Op::FileOpenLines | Op::CsvOpen => {
                let path = match pop!() {
                    Value::Text(path) => path,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "file reader path must be Text",
                            instruction.span,
                        ))
                    }
                };
                let resolved = match host_file_path(&self.policy, &path, false, instruction.span) {
                    Ok(path) => path,
                    Err(error) => {
                        push_stack!(file_error_result(error));
                        return Ok(());
                    }
                };
                let effect = self.begin_effect(EffectKind::FileRead, path.as_ref());
                let opened = if instruction.op == Op::FileOpenLines {
                    self.readers.open_lines(resolved)
                } else {
                    self.readers.open_csv(resolved)
                };
                match opened {
                    Ok(handle) => {
                        self.finish_effect(effect, EffectOutcome::Returned);
                        let (owner, id) = handle.parts();
                        push_stack!(Value::ResultOk(Box::new(Value::NativeResource(
                            NativeResourceValue::new("FileReader", owner, id),
                        ))));
                    }
                    Err(error) => {
                        self.finish_effect(effect, EffectOutcome::ReturnedError);
                        let code = match error.kind {
                            stdlib_data::ReaderErrorKind::InvalidUtf8 => "R_FILE_UTF8",
                            stdlib_data::ReaderErrorKind::Limit => "R_FILE_LIMIT",
                            stdlib_data::ReaderErrorKind::ForeignOwner => "R_RESOURCE_OWNER",
                            stdlib_data::ReaderErrorKind::Closed => "R_RESOURCE_CLOSED",
                            stdlib_data::ReaderErrorKind::Stale => "R_RESOURCE_STALE",
                            stdlib_data::ReaderErrorKind::Mode => "R_RESOURCE_MODE",
                            stdlib_data::ReaderErrorKind::Io => "R_FILE_READ",
                        };
                        push_stack!(file_error_result(RuntimeError::new(
                            code,
                            error.message,
                            instruction.span
                        )));
                    }
                }
            }
            Op::FileNextLine | Op::CsvNext | Op::FileClose => {
                let value = pop!();
                let handle = runtime_reader_handle(&value, instruction.span)?;
                if instruction.op == Op::FileClose {
                    match self.readers.close(handle) {
                        Ok(()) => push_stack!(Value::ResultOk(Box::new(Value::Unit))),
                        Err(error) => {
                            let error = reader_runtime_error(
                                error.kind,
                                error.message,
                                error.line,
                                error.column,
                                instruction.span,
                            );
                            if error.code == "R_FILE_LIMIT" {
                                return Err(error);
                            }
                            push_stack!(file_error_result(error));
                        }
                    }
                } else if instruction.op == Op::FileNextLine {
                    match self.readers.next_line(handle) {
                        Ok(Some(line)) => push_stack!(Value::ResultOk(Box::new(
                            Value::OptionSome(Box::new(Value::Record(
                                "SourceLine".into(),
                                Arc::new(vec![
                                    Value::Int(BigInt::from_usize(line.number)),
                                    Value::Text(line.text.into())
                                ]),
                            )))
                        ))),
                        Ok(None) => push_stack!(Value::ResultOk(Box::new(Value::OptionNone))),
                        Err(error) => push_stack!(file_error_result(reader_runtime_error(
                            error.kind,
                            error.message,
                            error.line,
                            error.column,
                            instruction.span
                        ))),
                    }
                } else {
                    match self.readers.next_csv(handle) {
                        Ok(Some(row)) => push_stack!(Value::ResultOk(Box::new(Value::OptionSome(
                            Box::new(Value::Record(
                                "CsvRow".into(),
                                Arc::new(vec![
                                    Value::Int(BigInt::from_usize(row.start_line)),
                                    Value::from_list_items(
                                        row.fields
                                            .into_iter()
                                            .map(|field| Value::Text(field.into()))
                                            .collect()
                                    )
                                ]),
                            ))
                        )))),
                        Ok(None) => push_stack!(Value::ResultOk(Box::new(Value::OptionNone))),
                        Err(error) => {
                            let (code, line, column, message) = match error {
                                stdlib_data::CsvError::Reader(error) => {
                                    let error = reader_runtime_error(
                                        error.kind,
                                        error.message,
                                        error.line,
                                        error.column,
                                        instruction.span,
                                    );
                                    if error.code == "R_FILE_LIMIT" {
                                        return Err(error);
                                    }
                                    push_stack!(file_error_result(error));
                                    return Ok(());
                                }
                                stdlib_data::CsvError::Mode => (
                                    "R_RESOURCE_MODE",
                                    0,
                                    0,
                                    "CSV operation requires a CSV reader".into(),
                                ),
                                stdlib_data::CsvError::Malformed {
                                    line,
                                    column,
                                    message,
                                } => ("R_CSV_MALFORMED", line, column, message),
                            };
                            let mut error = RuntimeError::new(code, message, instruction.span);
                            if line > 0 {
                                error.trace.push(Span {
                                    line,
                                    column,
                                    ..instruction.span
                                });
                            }
                            push_stack!(file_error_result(error));
                        }
                    }
                }
            }
            Op::CsvEncode => {
                let rows = match pop!() {
                    Value::List(rows) => rows,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "csv_encode requires List[List[Text]]",
                            instruction.span,
                        ))
                    }
                };
                let mut encoded_rows = Vec::with_capacity(rows.len());
                for row in rows.iter() {
                    let Value::List(fields) = row else {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "csv_encode row requires List[Text]",
                            instruction.span,
                        ));
                    };
                    let fields = fields
                        .iter()
                        .map(|value| match value {
                            Value::Text(text) => Ok(text.to_string()),
                            _ => Err(RuntimeError::new(
                                "R_TYPE_INVALID",
                                "csv_encode field requires Text",
                                instruction.span,
                            )),
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    encoded_rows.push(fields);
                }
                let text = stdlib_data::csv_encode(&encoded_rows);
                if text.len() > self.policy.max_file_bytes
                    || text.len() > self.policy.max_text_bytes
                {
                    return Err(RuntimeError::new(
                        "R_TEXT_LIMIT",
                        "CSV output exceeds the configured text limit",
                        instruction.span,
                    ));
                }
                charge_allocation(
                    budget.as_ref(),
                    size_of::<String>().saturating_add(text.len()),
                    instruction.span,
                )?;
                push_stack!(Value::ResultOk(Box::new(Value::Text(text.into()))));
            }
            Op::TimeParseDate | Op::TimeParseTimestamp | Op::SemVerParse => {
                let text = match pop!() {
                    Value::Text(text) => text,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "data parser requires Text",
                            instruction.span,
                        ))
                    }
                };
                if text.len() > self.policy.max_text_bytes {
                    return Err(RuntimeError::new(
                        "R_TEXT_LIMIT",
                        "data parser input exceeds the configured text limit",
                        instruction.span,
                    ));
                }
                let parsed: Result<Value, stdlib_data::DataError> = match instruction.op {
                    Op::TimeParseDate => stdlib_data::Date::parse(&text).map(|date| {
                        Value::Record(
                            "Date".into(),
                            Arc::new(vec![
                                Value::Int(BigInt::from_i64(date.year as i64)),
                                Value::Int(BigInt::from_u64(date.month as u64)),
                                Value::Int(BigInt::from_u64(date.day as u64)),
                            ]),
                        )
                    }),
                    Op::TimeParseTimestamp => stdlib_data::parse_timestamp(&text).map(|micros| {
                        Value::Record(
                            "Instant".into(),
                            Arc::new(vec![Value::Int(BigInt::from_i64(micros))]),
                        )
                    }),
                    Op::SemVerParse => stdlib_data::parse_semver(&text).map(|version| {
                        Value::Record(
                            "SemVer".into(),
                            Arc::new(vec![
                                Value::Text(version.major.into()),
                                Value::Text(version.minor.into()),
                                Value::Text(version.patch.into()),
                                Value::from_list_items(
                                    version
                                        .prerelease
                                        .into_iter()
                                        .map(|value| Value::Text(value.into()))
                                        .collect(),
                                ),
                                Value::from_list_items(
                                    version
                                        .build
                                        .into_iter()
                                        .map(|value| Value::Text(value.into()))
                                        .collect(),
                                ),
                                Value::Text(version.original.into()),
                            ]),
                        )
                    }),
                    _ => unreachable!(),
                };
                match parsed {
                    Ok(value) => push_stack!(Value::ResultOk(Box::new(value))),
                    Err(error) => push_stack!(file_error_result(RuntimeError::new(
                        error.code,
                        error.message,
                        instruction.span
                    ))),
                }
            }
            Op::TimeResolveLocal => {
                let timezone = match pop!() {
                    Value::Text(timezone) => timezone,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "resolve_local timezone must be Text",
                            instruction.span,
                        ))
                    }
                };
                let fields = match pop!() {
                    Value::Record(name, fields) if name == "LocalDateTime" && fields.len() == 7 => {
                        fields
                    }
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "resolve_local requires a LocalDateTime record",
                            instruction.span,
                        ))
                    }
                };
                let integer_field = |index: usize, field: &str, code: &str| match &fields[index] {
                    Value::Int(value) => value.to_i64().ok_or_else(|| {
                        stdlib_services::ServiceError::new(
                            code,
                            format!("{field} is outside the supported integer range"),
                        )
                    }),
                    _ => Err(stdlib_services::ServiceError::new(
                        "E_TIME_INPUT",
                        format!("LocalDateTime.{field} is not an integer"),
                    )),
                };
                let local = (|| {
                    Ok(stdlib_services::LocalDateTime {
                        year: integer_field(0, "year", "E_DATE")?,
                        month: integer_field(1, "month", "E_DATE")?,
                        day: integer_field(2, "day", "E_DATE")?,
                        hour: integer_field(3, "hour", "E_TIME")?,
                        minute: integer_field(4, "minute", "E_TIME")?,
                        second: integer_field(5, "second", "E_TIME")?,
                        microsecond: integer_field(6, "microsecond", "E_TIME")?,
                    })
                })();
                let result =
                    local.and_then(|local| stdlib_services::resolve_local_time(local, &timezone));
                let value = temporal_resolution_result_value(result);
                charge_allocation(
                    budget.as_ref(),
                    cloned_value_allocation(&value),
                    instruction.span,
                )?;
                push_stack!(value);
            }
            Op::TimeZoneDataVersion => {
                let version = Value::Text(stdlib_services::IANA_TZDB_VERSION.to_owned().into());
                charge_allocation(
                    budget.as_ref(),
                    stdlib_services::IANA_TZDB_VERSION.len(),
                    instruction.span,
                )?;
                push_stack!(version);
            }
            Op::TimeFormatUtc => {
                let value = pop!();
                let Value::Record(name, fields) = value else {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "time_format_utc requires Instant",
                        instruction.span,
                    ));
                };
                if name != "Instant" || fields.len() != 1 {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "invalid Instant record",
                        instruction.span,
                    ));
                }
                let Value::Int(micros) = &fields[0] else {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "invalid Instant value",
                        instruction.span,
                    ));
                };
                let micros = micros.to_i64().ok_or_else(|| {
                    RuntimeError::new(
                        "R_TIME_RANGE",
                        "instant is outside supported range",
                        instruction.span,
                    )
                })?;
                let text = stdlib_data::format_utc(stdlib_data::Instant {
                    unix_microseconds: micros,
                });
                if text.len() > self.policy.max_text_bytes {
                    return Err(RuntimeError::new(
                        "R_TEXT_LIMIT",
                        "formatted timestamp exceeds configured text limit",
                        instruction.span,
                    ));
                }
                charge_allocation(
                    budget.as_ref(),
                    size_of::<String>().saturating_add(text.len()),
                    instruction.span,
                )?;
                push_stack!(Value::Text(text.into()));
            }
            Op::UnixTimeMs | Op::TimeTodayLocal => {
                if !self.policy.allow_clock {
                    return Err(RuntimeError::new(
                        "R_CLOCK_DENIED",
                        "wall-clock access is not granted",
                        instruction.span,
                    ));
                }
                let effect = self.begin_effect(EffectKind::ClockRead, "system-clock");
                let unix_time_ms = match self.policy.clock_unix_time_ms_override {
                    Some(value) => value,
                    None => match current_unix_time_ms() {
                        Ok(value) => value,
                        Err(message) => {
                            self.finish_effect(effect, EffectOutcome::ReturnedError);
                            return Err(RuntimeError::new(
                                "R_CLOCK_UNAVAILABLE",
                                message,
                                instruction.span,
                            ));
                        }
                    },
                };
                if matches!(instruction.op, Op::UnixTimeMs) {
                    self.finish_effect(effect, EffectOutcome::Returned);
                    push_stack!(Value::Int(BigInt::from_i64(unix_time_ms)));
                } else {
                    match local_date_from_unix_time_ms(unix_time_ms) {
                        Ok((year, month, day)) => {
                            self.finish_effect(effect, EffectOutcome::Returned);
                            push_stack!(Value::Record(
                                "Date".into(),
                                Arc::new(vec![
                                    Value::Int(BigInt::from_i64(year as i64)),
                                    Value::Int(BigInt::from_i64(month as i64)),
                                    Value::Int(BigInt::from_i64(day as i64)),
                                ]),
                            ));
                        }
                        Err(message) => {
                            self.finish_effect(effect, EffectOutcome::ReturnedError);
                            return Err(RuntimeError::new(
                                "R_CLOCK_RANGE",
                                message,
                                instruction.span,
                            ));
                        }
                    }
                }
            }
            Op::SemVerCompare => {
                let right = pop!();
                let left = pop!();
                let parse_record = |value: &Value| -> Result<stdlib_data::SemVer, RuntimeError> {
                    let Value::Record(name, fields) = value else {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "semver_compare requires SemVer records",
                            instruction.span,
                        ));
                    };
                    if name != "SemVer" || fields.len() != 6 {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "invalid SemVer record",
                            instruction.span,
                        ));
                    }
                    let Value::Text(original) = &fields[5] else {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "invalid SemVer source text",
                            instruction.span,
                        ));
                    };
                    stdlib_data::parse_semver(original).map_err(|error| {
                        RuntimeError::new("R_TYPE_INVALID", error.message, instruction.span)
                    })
                };
                let left = parse_record(&left)?;
                let right = parse_record(&right)?;
                let ordering = stdlib_data::compare_semver(&left, &right);
                push_stack!(Value::Int(BigInt::from_i64(match ordering {
                    std::cmp::Ordering::Less => -1,
                    std::cmp::Ordering::Equal => 0,
                    std::cmp::Ordering::Greater => 1,
                })));
            }
            Op::CliParse => {
                let options = match pop!() {
                    Value::List(values) => values,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "cli_parse options must be a list",
                            instruction.span,
                        ))
                    }
                };
                let arguments = match pop!() {
                    Value::List(values) => values,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "cli_parse arguments must be a list",
                            instruction.span,
                        ))
                    }
                };
                let arguments = arguments
                    .iter()
                    .map(|value| match value {
                        Value::Text(value) => Ok(value.to_string()),
                        _ => Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "cli_parse argument must be Text",
                            instruction.span,
                        )),
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let mut parsed_options = Vec::with_capacity(options.len());
                for option in options.iter() {
                    let Value::Record(name, fields) = option else {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "cli_parse option must be CliOption",
                            instruction.span,
                        ));
                    };
                    if name != "CliOption" || fields.len() != 4 {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "invalid CliOption record",
                            instruction.span,
                        ));
                    }
                    let (
                        Value::Text(name),
                        Value::Bool(takes_value),
                        Value::Bool(repeatable),
                        Value::Bool(required),
                    ) = (&fields[0], &fields[1], &fields[2], &fields[3])
                    else {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "invalid CliOption fields",
                            instruction.span,
                        ));
                    };
                    parsed_options.push(stdlib_data::CliOption::new(
                        name.to_string(),
                        *takes_value,
                        *repeatable,
                        *required,
                    ));
                }
                match stdlib_data::CliArguments::parse(&arguments, &parsed_options) {
                    Ok(parsed) => {
                        let values = parsed
                            .values
                            .into_iter()
                            .map(|(key, values)| {
                                (
                                    key,
                                    Value::from_list_items(
                                        values
                                            .into_iter()
                                            .map(|value| Value::Text(value.into()))
                                            .collect(),
                                    ),
                                )
                            })
                            .collect::<BTreeMap<_, _>>();
                        let result = Value::Record(
                            "CliArgs".into(),
                            Arc::new(vec![
                                Value::Map(Arc::new(values)),
                                Value::from_list_items(
                                    parsed
                                        .positionals
                                        .into_iter()
                                        .map(|value| Value::Text(value.into()))
                                        .collect(),
                                ),
                            ]),
                        );
                        push_stack!(Value::ResultOk(Box::new(result)));
                    }
                    Err(error) => push_stack!(file_error_result(RuntimeError::new(
                        error.code,
                        error.message,
                        instruction.span
                    ))),
                }
            }
            Op::FsWalk => {
                let path = match pop!() {
                    Value::Text(path) => path,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "fs_walk path must be Text",
                            instruction.span,
                        ))
                    }
                };
                let resolved = match host_file_path_no_follow(&self.policy, &path, instruction.span)
                {
                    Ok(path) => path,
                    Err(error) => {
                        push_stack!(file_error_result(error));
                        return Ok(());
                    }
                };
                match stdlib_data::fs_walk(&resolved, self.policy.max_collection_items) {
                    Ok(entries) => {
                        let allocated = entries
                            .iter()
                            .map(|entry| {
                                std::mem::size_of::<stdlib_data::DirectoryEntry>()
                                    .saturating_add(entry.relative_path.len())
                                    .saturating_add(entry.kind.len())
                            })
                            .sum::<usize>();
                        charge_allocation(budget.as_ref(), allocated, instruction.span)?;
                        let values = entries
                            .into_iter()
                            .map(|entry| {
                                Value::Record(
                                    "DirectoryEntry".into(),
                                    Arc::new(vec![
                                        Value::Text(entry.relative_path.into()),
                                        Value::Text(entry.kind.into()),
                                    ]),
                                )
                            })
                            .collect();
                        push_stack!(Value::ResultOk(Box::new(Value::from_list_items(values))));
                    }
                    Err(error) if error.code == "E_FS_WALK_LIMIT" => {
                        return Err(RuntimeError::new(
                            "R_COLLECTION_LIMIT",
                            error.message,
                            instruction.span,
                        ))
                    }
                    Err(error) => push_stack!(file_error_result(RuntimeError::new(
                        error.code,
                        error.message,
                        instruction.span
                    ))),
                }
            }
            Op::ListGetOption => {
                let index = match pop!() {
                    Value::Int(index) => index.to_usize(),
                    _ => None,
                };
                let list = pop!();
                let Some(index) = index else {
                    push_stack!(Value::OptionNone);
                    return Ok(());
                };
                let value = list.list_item(index);
                push_stack!(value
                    .map(|value| Value::OptionSome(Box::new(value)))
                    .unwrap_or(Value::OptionNone));
            }
            Op::MapGetOption => {
                let key = match pop!() {
                    Value::Text(key) => key,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "map_get key must be Text",
                            instruction.span,
                        ))
                    }
                };
                let map = match pop!() {
                    Value::Map(map) => map,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "map_get requires Map",
                            instruction.span,
                        ))
                    }
                };
                push_stack!(map
                    .get(key.as_ref())
                    .cloned()
                    .map(|value| Value::OptionSome(Box::new(value)))
                    .unwrap_or(Value::OptionNone));
            }
            Op::TextParseInt | Op::TextParseFloat => {
                let text = match pop!() {
                    Value::Text(text) => text,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "numeric parser requires Text",
                            instruction.span,
                        ))
                    }
                };
                let parsed: Result<Value, stdlib_data::DataError> =
                    if instruction.op == Op::TextParseInt {
                        stdlib_data::text_parse_int(&text).map(Value::Int)
                    } else {
                        stdlib_data::text_parse_float(&text).and_then(|value| {
                            Float64::new(value).map(Value::Float64).ok_or_else(|| {
                                stdlib_data::DataError {
                                    code: "E_FLOAT_PARSE",
                                    message: "floating-point value must be finite".into(),
                                }
                            })
                        })
                    };
                match parsed {
                    Ok(value) => {
                        if let Value::Int(integer) = &value {
                            if integer.digit_count() > self.policy.max_integer_digits {
                                return Err(RuntimeError::new(
                                    "R_INTEGER_LIMIT",
                                    "parsed integer exceeds the configured digit limit",
                                    instruction.span,
                                ));
                            }
                        }
                        push_stack!(Value::ResultOk(Box::new(value)));
                    }
                    Err(error) => push_stack!(file_error_result(RuntimeError::new(
                        error.code,
                        error.message,
                        instruction.span
                    ))),
                }
            }
            Op::TextCaseFold | Op::TextCollapseWhitespace => {
                let text = match pop!() {
                    Value::Text(text) => text,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "Unicode operation requires Text",
                            instruction.span,
                        ))
                    }
                };
                if instruction.op == Op::TextCaseFold {
                    let output = stdlib_services::text_casefold(&text);
                    if output.len() > self.policy.max_text_bytes {
                        return Err(RuntimeError::new(
                            "R_TEXT_LIMIT",
                            "Unicode result exceeds the configured text limit",
                            instruction.span,
                        ));
                    }
                    charge_allocation(budget.as_ref(), output.len(), instruction.span)?;
                    push_stack!(Value::Text(output.into()));
                } else if let Some(output) =
                    stdlib_services::text_collapse_whitespace_if_changed(&text)
                {
                    if output.len() > self.policy.max_text_bytes {
                        return Err(RuntimeError::new(
                            "R_TEXT_LIMIT",
                            "Unicode result exceeds the configured text limit",
                            instruction.span,
                        ));
                    }
                    charge_allocation(budget.as_ref(), output.len(), instruction.span)?;
                    push_stack!(Value::Text(output.into()));
                } else {
                    push_stack!(Value::Text(text));
                }
            }
            Op::TextNormalize => {
                let form = match pop!() {
                    Value::Text(value) => value,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "normalization form must be Text",
                            instruction.span,
                        ))
                    }
                };
                let text = match pop!() {
                    Value::Text(value) => value,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "normalization input must be Text",
                            instruction.span,
                        ))
                    }
                };
                match stdlib_services::text_normalize(&text, &form) {
                    Ok(output) if output.len() <= self.policy.max_text_bytes => {
                        charge_allocation(budget.as_ref(), output.len(), instruction.span)?;
                        push_stack!(Value::ResultOk(Box::new(Value::Text(output.into()))));
                    }
                    Ok(_) => {
                        return Err(RuntimeError::new(
                            "R_TEXT_LIMIT",
                            "normalized text exceeds the configured text limit",
                            instruction.span,
                        ))
                    }
                    Err(error) => {
                        push_stack!(Value::ResultErr(Box::new(service_error_value(error))))
                    }
                }
            }
            Op::CryptoHmacSha256 => {
                let body = match pop!() {
                    Value::Bytes(value) => value,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "HMAC body must be Bytes",
                            instruction.span,
                        ))
                    }
                };
                let key = match pop!() {
                    Value::Bytes(value) => value,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "HMAC key must be Bytes",
                            instruction.span,
                        ))
                    }
                };
                push_stack!(Value::Bytes(Arc::new(stdlib_services::crypto_hmac_sha256(
                    &key, &body
                ))));
            }
            Op::CryptoVerifyHmacSha256 => {
                let expected = match pop!() {
                    Value::Bytes(value) => value,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "HMAC digest must be Bytes",
                            instruction.span,
                        ))
                    }
                };
                let body = match pop!() {
                    Value::Bytes(value) => value,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "HMAC body must be Bytes",
                            instruction.span,
                        ))
                    }
                };
                let key = match pop!() {
                    Value::Bytes(value) => value,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "HMAC key must be Bytes",
                            instruction.span,
                        ))
                    }
                };
                push_stack!(Value::Bool(stdlib_services::crypto_verify_hmac_sha256(
                    &key, &body, &expected
                )));
            }
            Op::SqliteOpen => {
                let path = match pop!() {
                    Value::Text(path) => path,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "sqlite_open path must be Text",
                            instruction.span,
                        ))
                    }
                };
                let result = if !self.policy.allow_file_read || !self.policy.allow_file_write {
                    Err(stdlib_services::ServiceError::new(
                        "E_SQLITE_CAPABILITY",
                        "SQLite requires both file read and file write grants",
                    ))
                } else {
                    match host_file_path(&self.policy, &path, true, instruction.span) {
                        Ok(path) => self.databases.open(path).map(|handle| {
                            let (owner, id) = handle.parts();
                            Value::NativeResource(NativeResourceValue::new("Db", owner, id))
                        }),
                        Err(error) => Err(stdlib_services::ServiceError::new(
                            "E_SQLITE_PATH",
                            error.message,
                        )
                        .at_path(path.to_string())),
                    }
                };
                push_stack!(service_result_value(result));
            }
            Op::SqliteBegin => {
                let (owner, id) = runtime_service_resource(&pop!(), "Db", instruction.span)?;
                let result = self
                    .databases
                    .begin(stdlib_services::DbHandle::from_parts(owner, id))
                    .and_then(|handle| {
                        let cancellation = budget.cancellation.cancelled.clone();
                        self.databases.set_cancelled(handle, cancellation)?;
                        let (owner, id) = handle.parts();
                        Ok(Value::NativeResource(NativeResourceValue::new(
                            "Transaction",
                            owner,
                            id,
                        )))
                    });
                push_stack!(service_result_value(result));
            }
            Op::SqliteExecute | Op::SqliteQuery => {
                let values = runtime_sql_values(&pop!(), instruction.span)?;
                let sql = match pop!() {
                    Value::Text(sql) => sql,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "SQLite query must be Text",
                            instruction.span,
                        ))
                    }
                };
                let (owner, id) =
                    runtime_service_resource(&pop!(), "Transaction", instruction.span)?;
                let transaction = stdlib_services::TransactionHandle::from_parts(owner, id);
                if instruction.op == Op::SqliteExecute {
                    let result = self
                        .databases
                        .execute(transaction, &sql, &values)
                        .map(|affected| Value::Int(BigInt::from_i64(affected)));
                    push_stack!(service_result_value(result));
                } else {
                    let result =
                        self.databases
                            .query(transaction, &sql, &values)
                            .and_then(|rows| {
                                if rows.len() > self.policy.max_collection_items {
                                    return Err(stdlib_services::ServiceError::new(
                                        "E_SQLITE_RESULT_LIMIT",
                                        "SQLite query exceeded the configured collection limit",
                                    ));
                                }
                                let values = rows
                                    .into_iter()
                                    .map(|row| {
                                        Value::Record(
                                            "SqlRow".into(),
                                            Arc::new(vec![
                                                Value::from_list_items(
                                                    row.columns
                                                        .into_iter()
                                                        .map(|text| Value::Text(text.into()))
                                                        .collect(),
                                                ),
                                                Value::from_list_items(
                                                    row.values
                                                        .into_iter()
                                                        .map(service_sql_value)
                                                        .collect(),
                                                ),
                                            ]),
                                        )
                                    })
                                    .collect::<Vec<_>>();
                                let bytes = values
                                    .iter()
                                    .map(cloned_value_allocation)
                                    .fold(0usize, usize::saturating_add);
                                charge_allocation(budget.as_ref(), bytes, instruction.span)
                                    .map_err(|error| {
                                        stdlib_services::ServiceError::new(
                                            error.code.as_str(),
                                            error.message,
                                        )
                                    })?;
                                Ok(Value::from_list_items(values))
                            });
                    push_stack!(service_result_value(result));
                }
            }
            Op::SqliteCommit | Op::SqliteRollback => {
                let (owner, id) =
                    runtime_service_resource(&pop!(), "Transaction", instruction.span)?;
                let transaction = stdlib_services::TransactionHandle::from_parts(owner, id);
                let result = if instruction.op == Op::SqliteCommit {
                    self.databases.commit(transaction)
                } else {
                    self.databases.rollback(transaction)
                }
                .map(|()| Value::Unit);
                push_stack!(service_result_value(result));
            }
            Op::SqliteClose => {
                let (owner, id) = runtime_service_resource(&pop!(), "Db", instruction.span)?;
                let result = self
                    .databases
                    .close(stdlib_services::DbHandle::from_parts(owner, id))
                    .map(|()| Value::Unit);
                push_stack!(service_result_value(result));
            }
            Op::TomlParse => {
                let source = match pop!() {
                    Value::Text(source) => source,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "toml_parse input must be Text",
                            instruction.span,
                        ))
                    }
                };
                let result = self.toml_documents.parse(&source).map(|handle| {
                    let (owner, id) = handle.parts();
                    Value::NativeResource(NativeResourceValue::new("TomlDocument", owner, id))
                });
                push_stack!(service_result_value(result));
            }
            Op::TomlHas | Op::TomlGetText | Op::TomlGetInt | Op::TomlGetBool => {
                let path = runtime_text_path(&pop!(), instruction.span)?;
                let (owner, id) =
                    runtime_service_resource(&pop!(), "TomlDocument", instruction.span)?;
                let handle = stdlib_services::TomlHandle::from_parts(owner, id);
                let result = match instruction.op {
                    Op::TomlHas => self.toml_documents.has(handle, &path).map(Value::Bool),
                    Op::TomlGetText => self
                        .toml_documents
                        .get_text(handle, &path)
                        .map(|text| Value::Text(text.into())),
                    Op::TomlGetInt => self
                        .toml_documents
                        .get_integer(handle, &path)
                        .map(|value| Value::Int(BigInt::from_i64(value))),
                    Op::TomlGetBool => self.toml_documents.get_bool(handle, &path).map(Value::Bool),
                    _ => unreachable!(),
                };
                if instruction.op == Op::TomlHas {
                    let value = result.map_err(|error| {
                        RuntimeError::new(error.code, error.message, instruction.span)
                    })?;
                    push_stack!(value);
                } else {
                    push_stack!(service_result_value(result));
                }
            }
            Op::TomlPutText | Op::TomlPutInt | Op::TomlPutBool => {
                let replacement = pop!();
                let path = runtime_text_path(&pop!(), instruction.span)?;
                let (owner, id) =
                    runtime_service_resource(&pop!(), "TomlDocument", instruction.span)?;
                let handle = stdlib_services::TomlHandle::from_parts(owner, id);
                let result = match (instruction.op, replacement) {
                    (Op::TomlPutText, Value::Text(value)) => {
                        self.toml_documents.put_text(handle, &path, &value)
                    }
                    (Op::TomlPutInt, Value::Int(value)) => value
                        .to_i64()
                        .ok_or_else(|| {
                            stdlib_services::ServiceError::new(
                                "E_TOML_INTEGER_RANGE",
                                "TOML integers must fit signed 64-bit range",
                            )
                        })
                        .and_then(|value| self.toml_documents.put_integer(handle, &path, value)),
                    (Op::TomlPutBool, Value::Bool(value)) => {
                        self.toml_documents.put_bool(handle, &path, value)
                    }
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "TOML value type does not match the edit operation",
                            instruction.span,
                        ))
                    }
                }
                .map(|()| Value::Unit);
                push_stack!(service_result_value(result));
            }
            Op::TomlRender => {
                let (owner, id) =
                    runtime_service_resource(&pop!(), "TomlDocument", instruction.span)?;
                let text = self
                    .toml_documents
                    .render(stdlib_services::TomlHandle::from_parts(owner, id))
                    .map_err(|error| {
                        RuntimeError::new("R_TOML_RENDER", error.to_string(), instruction.span)
                    })?;
                if text.len() > self.policy.max_text_bytes {
                    return Err(RuntimeError::new(
                        "R_TEXT_LIMIT",
                        "rendered TOML exceeds the configured text limit",
                        instruction.span,
                    ));
                }
                charge_allocation(
                    budget.as_ref(),
                    size_of::<String>().saturating_add(text.len()),
                    instruction.span,
                )?;
                push_stack!(Value::Text(text.into()));
            }
            Op::CalendarParse => {
                let source = match pop!() {
                    Value::Text(source) => source,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "calendar_parse input must be Text",
                            instruction.span,
                        ))
                    }
                };
                match stdlib_services::calendar_parse(&source) {
                    Ok(events) if events.len() <= self.policy.max_collection_items => {
                        let values = events
                            .into_iter()
                            .map(service_calendar_event)
                            .collect::<Vec<_>>();
                        let bytes = values
                            .iter()
                            .map(cloned_value_allocation)
                            .fold(0usize, usize::saturating_add);
                        charge_allocation(budget.as_ref(), bytes, instruction.span)?;
                        push_stack!(Value::ResultOk(Box::new(Value::from_list_items(values))));
                    }
                    Ok(_) => push_stack!(Value::ResultErr(Box::new(service_error_value(
                        stdlib_services::ServiceError::new(
                            "E_ICAL_LIMIT",
                            "calendar event count exceeds the configured collection limit"
                        )
                    )))),
                    Err(error) => {
                        push_stack!(Value::ResultErr(Box::new(service_error_value(error))))
                    }
                }
            }
            Op::ReadText => {
                let path = match pop!() {
                    Value::Text(path) => path,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "read_text path must be Text",
                            instruction.span,
                        ))
                    }
                };
                let resolved = match host_file_path(&self.policy, &path, false, instruction.span) {
                    Ok(resolved) => resolved,
                    Err(error) => {
                        push_stack!(file_error_result(error));
                        return Ok(());
                    }
                };
                let metadata = match fs::metadata(&resolved) {
                    Ok(metadata) => metadata,
                    Err(error) => {
                        push_stack!(file_error_result(RuntimeError::new(
                            file_io_error_code(&error, "R_FILE_READ"),
                            format!("cannot inspect `{path}`: {error}"),
                            instruction.span,
                        )));
                        return Ok(());
                    }
                };
                if metadata.len() as usize > self.policy.max_file_bytes {
                    return Err(RuntimeError::new(
                        "R_FILE_LIMIT",
                        format!("file `{path}` exceeds the configured byte limit"),
                        instruction.span,
                    ));
                }
                let text = match fs::read_to_string(&resolved) {
                    Ok(text) => text,
                    Err(error) => {
                        push_stack!(file_error_result(RuntimeError::new(
                            file_io_error_code(&error, "R_FILE_READ"),
                            format!("cannot read `{path}` as UTF-8 text: {error}"),
                            instruction.span,
                        )));
                        return Ok(());
                    }
                };
                if text.len() > self.policy.max_text_bytes {
                    return Err(RuntimeError::new(
                        "R_TEXT_LIMIT",
                        "file content exceeds the configured text limit",
                        instruction.span,
                    ));
                }
                charge_allocation(
                    budget.as_ref(),
                    size_of::<String>().saturating_add(text.len()),
                    instruction.span,
                )?;
                let effect = self.begin_effect(EffectKind::FileRead, path);
                self.finish_effect(effect, EffectOutcome::Returned);
                push_stack!(Value::ResultOk(Box::new(Value::Text(text.into()))));
            }
            Op::WriteText => {
                let content = match pop!() {
                    Value::Text(content) => content,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "write_text content must be Text",
                            instruction.span,
                        ))
                    }
                };
                let path = match pop!() {
                    Value::Text(path) => path,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "write_text path must be Text",
                            instruction.span,
                        ))
                    }
                };
                if content.len() > self.policy.max_file_bytes {
                    return Err(RuntimeError::new(
                        "R_FILE_LIMIT",
                        "file content exceeds the configured byte limit",
                        instruction.span,
                    ));
                }
                let resolved = match host_file_path(&self.policy, &path, true, instruction.span) {
                    Ok(resolved) => resolved,
                    Err(error) => {
                        push_stack!(file_error_result(error));
                        return Ok(());
                    }
                };
                let effect = self.begin_effect(EffectKind::FileWrite, path.as_ref());
                if let Err(error) = fs::write(&resolved, content.as_bytes()) {
                    self.finish_effect(effect, EffectOutcome::ReturnedError);
                    push_stack!(file_error_result(RuntimeError::new(
                        "R_FILE_WRITE",
                        format!("cannot write `{path}`: {error}"),
                        instruction.span,
                    )));
                    return Ok(());
                }
                self.finish_effect(effect, EffectOutcome::Returned);
                push_stack!(Value::ResultOk(Box::new(Value::Unit)));
            }
            Op::WriteTextAtomic => {
                let content = match pop!() {
                    Value::Text(content) => content,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "write_text_atomic content must be Text",
                            instruction.span,
                        ))
                    }
                };
                let path = match pop!() {
                    Value::Text(path) => path,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "write_text_atomic path must be Text",
                            instruction.span,
                        ))
                    }
                };
                if content.len() > self.policy.max_file_bytes {
                    return Err(RuntimeError::new(
                        "R_FILE_LIMIT",
                        "file content exceeds the configured byte limit",
                        instruction.span,
                    ));
                }
                if content.len() > self.policy.max_text_bytes {
                    return Err(RuntimeError::new(
                        "R_TEXT_LIMIT",
                        "file content exceeds the configured text limit",
                        instruction.span,
                    ));
                }
                let resolved = match host_file_path(&self.policy, &path, true, instruction.span) {
                    Ok(resolved) => resolved,
                    Err(error) => {
                        push_stack!(file_error_result(error));
                        return Ok(());
                    }
                };
                let effect = self.begin_effect(EffectKind::FileWrite, path.as_ref());
                if let Err(error) = atomic_replace_text(&resolved, &content) {
                    self.finish_effect(effect, EffectOutcome::ReturnedError);
                    push_stack!(file_error_result(RuntimeError::new(
                        file_io_error_code(&error, "R_FILE_WRITE"),
                        format!("cannot atomically replace `{path}`: {error}"),
                        instruction.span,
                    )));
                    return Ok(());
                }
                self.finish_effect(effect, EffectOutcome::Returned);
                push_stack!(Value::ResultOk(Box::new(Value::Unit)));
            }
            Op::ReadBytes => {
                let path = match pop!() {
                    Value::Text(path) => path,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "read_bytes path must be Text",
                            instruction.span,
                        ))
                    }
                };
                let resolved = match host_file_path(&self.policy, &path, false, instruction.span) {
                    Ok(resolved) => resolved,
                    Err(error) => {
                        push_stack!(file_error_result(error));
                        return Ok(());
                    }
                };
                let metadata = match fs::metadata(&resolved) {
                    Ok(metadata) => metadata,
                    Err(error) => {
                        push_stack!(file_error_result(RuntimeError::new(
                            file_io_error_code(&error, "R_FILE_READ"),
                            format!("cannot inspect `{path}`: {error}"),
                            instruction.span
                        )));
                        return Ok(());
                    }
                };
                if metadata.len() > self.policy.max_file_bytes as u64 {
                    return Err(RuntimeError::new(
                        "R_FILE_LIMIT",
                        format!("file `{path}` exceeds the configured byte limit"),
                        instruction.span,
                    ));
                }
                let bytes = match fs::read(&resolved) {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        push_stack!(file_error_result(RuntimeError::new(
                            file_io_error_code(&error, "R_FILE_READ"),
                            format!("cannot read `{path}`: {error}"),
                            instruction.span
                        )));
                        return Ok(());
                    }
                };
                if bytes.len() > self.policy.max_file_bytes {
                    return Err(RuntimeError::new(
                        "R_FILE_LIMIT",
                        "file content exceeds the configured byte limit",
                        instruction.span,
                    ));
                }
                charge_allocation(
                    budget.as_ref(),
                    bytes.capacity().saturating_add(size_of::<Vec<u8>>()),
                    instruction.span,
                )?;
                let effect = self.begin_effect(EffectKind::FileRead, path.as_ref());
                self.finish_effect(effect, EffectOutcome::Returned);
                push_stack!(Value::ResultOk(Box::new(Value::Bytes(Arc::new(bytes)))));
            }
            Op::WriteBytes => {
                let content = match pop!() {
                    Value::Bytes(content) => content,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "write_bytes content must be Bytes",
                            instruction.span,
                        ))
                    }
                };
                let path = match pop!() {
                    Value::Text(path) => path,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "write_bytes path must be Text",
                            instruction.span,
                        ))
                    }
                };
                if content.len() > self.policy.max_file_bytes {
                    return Err(RuntimeError::new(
                        "R_FILE_LIMIT",
                        "file content exceeds the configured byte limit",
                        instruction.span,
                    ));
                }
                let resolved = match host_file_path(&self.policy, &path, true, instruction.span) {
                    Ok(resolved) => resolved,
                    Err(error) => {
                        push_stack!(file_error_result(error));
                        return Ok(());
                    }
                };
                let effect = self.begin_effect(EffectKind::FileWrite, path.as_ref());
                if let Err(error) = fs::write(&resolved, content.as_slice()) {
                    self.finish_effect(effect, EffectOutcome::ReturnedError);
                    push_stack!(file_error_result(RuntimeError::new(
                        "R_FILE_WRITE",
                        format!("cannot write `{path}`: {error}"),
                        instruction.span
                    )));
                    return Ok(());
                }
                self.finish_effect(effect, EffectOutcome::Returned);
                push_stack!(Value::ResultOk(Box::new(Value::Unit)));
            }
            Op::Args => {
                let allocation = self.policy.arguments.iter().fold(
                    size_of::<Vec<Value>>().saturating_add(
                        self.policy
                            .arguments
                            .len()
                            .saturating_mul(size_of::<Value>()),
                    ),
                    |total, value| total.saturating_add(size_of::<String>() + value.len()),
                );
                charge_allocation(budget.as_ref(), allocation, instruction.span)?;
                let values = self
                    .policy
                    .arguments
                    .iter()
                    .cloned()
                    .map(|value| Value::Text(value.into()))
                    .collect::<Vec<_>>();
                if values.len() > self.policy.max_collection_items {
                    return Err(RuntimeError::new(
                        "R_COLLECTION_LIMIT",
                        "process arguments exceed the configured item limit",
                        instruction.span,
                    ));
                }
                if let Some(error) = values
                    .iter()
                    .find_map(|value| value_limit_error(value, &self.policy, instruction.span))
                {
                    return Err(error);
                }
                push_stack!(Value::List(Arc::new(values)));
            }
            Op::ArgsFrom => {
                let start = match pop!() {
                    Value::Int(index) => index
                        .to_usize()
                        .filter(|index| *index <= self.policy.arguments.len())
                        .ok_or_else(|| {
                            RuntimeError::new(
                                "R_ARGUMENT_INDEX",
                                "args_from index must be between zero and the argument count",
                                instruction.span,
                            )
                        })?,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "args_from requires an Int index",
                            instruction.span,
                        ))
                    }
                };
                let values = self
                    .policy
                    .arguments
                    .iter()
                    .skip(start)
                    .cloned()
                    .map(|value| Value::Text(value.into()))
                    .collect::<Vec<_>>();
                let allocation = values.iter().fold(
                    size_of::<Vec<Value>>()
                        .saturating_add(values.len().saturating_mul(size_of::<Value>())),
                    |total, value| match value {
                        Value::Text(text) => total.saturating_add(size_of::<String>() + text.len()),
                        _ => total,
                    },
                );
                charge_allocation(budget.as_ref(), allocation, instruction.span)?;
                if values.len() > self.policy.max_collection_items {
                    return Err(RuntimeError::new(
                        "R_COLLECTION_LIMIT",
                        "process arguments exceed the configured item limit",
                        instruction.span,
                    ));
                }
                if let Some(error) = values
                    .iter()
                    .find_map(|value| value_limit_error(value, &self.policy, instruction.span))
                {
                    return Err(error);
                }
                push_stack!(Value::List(Arc::new(values)));
            }
            Op::Environment => {
                let name = match pop!() {
                    Value::Text(name) => name,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "env name must be Text",
                            instruction.span,
                        ))
                    }
                };
                if !self
                    .policy
                    .allowed_environment_variables
                    .contains(name.as_ref())
                {
                    return Err(RuntimeError::new(
                        "R_ENV_DENIED",
                        format!("environment variable `{name}` is not granted"),
                        instruction.span,
                    ));
                }
                let effect = self.begin_effect(EffectKind::EnvironmentRead, name.as_ref());
                match std::env::var(name.as_ref()) {
                    Ok(value) => {
                        self.finish_effect(effect, EffectOutcome::Returned);
                        push_stack!(Value::OptionSome(Box::new(Value::Text(value.into()))))
                    }
                    Err(std::env::VarError::NotPresent) => {
                        self.finish_effect(effect, EffectOutcome::Returned);
                        push_stack!(Value::OptionNone)
                    }
                    Err(std::env::VarError::NotUnicode(_)) => {
                        self.finish_effect(effect, EffectOutcome::ReturnedError);
                        return Err(RuntimeError::new(
                            "R_ENV_ENCODING",
                            "environment value is not valid Unicode",
                            instruction.span,
                        ));
                    }
                }
            }
            Op::JsonErrorMessage | Op::JsonErrorLine | Op::JsonErrorColumn | Op::JsonErrorCode => {
                let error = match pop!() {
                    Value::JsonError(error) => error,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "JSON error accessor requires JsonError",
                            instruction.span,
                        ))
                    }
                };
                push_stack!(match instruction.op {
                    Op::JsonErrorMessage => Value::Text(error.message.into()),
                    Op::JsonErrorLine => Value::Int(BigInt::from_usize(error.line)),
                    Op::JsonErrorColumn => Value::Int(BigInt::from_usize(error.column)),
                    Op::JsonErrorCode => Value::Text(error.code.into()),
                    _ => unreachable!(),
                });
            }
            Op::FileErrorCode | Op::FileErrorMessage | Op::FileErrorLine | Op::FileErrorColumn => {
                let error = match pop!() {
                    Value::FileError(error) => error,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "file error accessor requires FileError",
                            instruction.span,
                        ))
                    }
                };
                push_stack!(match instruction.op {
                    Op::FileErrorCode => Value::Text(error.code.into()),
                    Op::FileErrorMessage => Value::Text(error.message.into()),
                    Op::FileErrorLine => Value::Int(BigInt::from_usize(
                        error.trace.first().map(|span| span.line).unwrap_or(0),
                    )),
                    Op::FileErrorColumn => Value::Int(BigInt::from_usize(
                        error.trace.first().map(|span| span.column).unwrap_or(0),
                    )),
                    _ => unreachable!(),
                });
            }
            Op::HttpErrorCode | Op::HttpErrorMessage => {
                let error = match pop!() {
                    Value::HttpError(error) => error,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "HTTP error accessor requires HttpError",
                            instruction.span,
                        ))
                    }
                };
                push_stack!(Value::Text(
                    if matches!(instruction.op, Op::HttpErrorCode) {
                        error.code.into()
                    } else {
                        error.message.into()
                    }
                ));
            }
            Op::TaskIndex => match pop!() {
                Value::TaskError(error) => push_stack!(Value::Int(error.index)),
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "task_index requires TaskError",
                        instruction.span,
                    ))
                }
            },
            Op::TaskCode => match pop!() {
                Value::TaskError(error) => push_stack!(Value::Text(error.code.into())),
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "task_code requires TaskError",
                        instruction.span,
                    ))
                }
            },
            Op::TaskMessage => match pop!() {
                Value::TaskError(error) => push_stack!(Value::Text(error.message.into())),
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "task_message requires TaskError",
                        instruction.span,
                    ))
                }
            },
            Op::PythonCall(target, argc) => {
                let mut call_args = Vec::with_capacity(argc);
                for _ in 0..argc {
                    call_args.push(pop!());
                }
                call_args.reverse();
                let result =
                    self.call_python(target, &call_args, &[], budget.as_ref(), instruction.span)?;
                push_stack!(result);
            }
            Op::PythonCallKw(target, positional_count, keyword_names) => {
                let mut supplied = Vec::with_capacity(positional_count + keyword_names.len());
                for _ in 0..positional_count + keyword_names.len() {
                    supplied.push(pop!());
                }
                supplied.reverse();
                let keyword_values = supplied.split_off(positional_count);
                let call_kwargs = keyword_names
                    .into_iter()
                    .zip(keyword_values)
                    .collect::<Vec<_>>();
                let result = self.call_python(
                    target,
                    &supplied,
                    &call_kwargs,
                    budget.as_ref(),
                    instruction.span,
                )?;
                push_stack!(result);
            }
            Op::JsonNull => {
                push_stack!(finish_json_value(
                    serde_json::Value::Null,
                    &self.policy.vm_limits(),
                    budget.as_ref(),
                    instruction.span,
                )?);
            }
            Op::JsonBool => {
                let boolean = match pop!() {
                    Value::Bool(boolean) => boolean,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_bool requires Bool",
                            instruction.span,
                        ))
                    }
                };
                push_stack!(finish_json_value(
                    serde_json::Value::Bool(boolean),
                    &self.policy.vm_limits(),
                    budget.as_ref(),
                    instruction.span,
                )?);
            }
            Op::JsonInt => {
                let integer = match pop!() {
                    Value::Int(integer) => integer,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_int requires Int",
                            instruction.span,
                        ))
                    }
                };
                if integer.digit_count() > self.policy.max_integer_digits {
                    return Err(RuntimeError::new(
                        "R_INTEGER_LIMIT",
                        "JSON integer exceeds the configured digit limit",
                        instruction.span,
                    ));
                }
                let parsed = serde_json::from_str::<serde_json::Value>(&integer.to_string())
                    .map_err(|error| {
                        RuntimeError::new("R_JSON_ENCODE", error.to_string(), instruction.span)
                    })?;
                push_stack!(finish_json_value(
                    parsed,
                    &self.policy.vm_limits(),
                    budget.as_ref(),
                    instruction.span,
                )?);
            }
            Op::JsonFloat => {
                let float = match pop!() {
                    Value::Float64(float) => float.get(),
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_float requires Float64",
                            instruction.span,
                        ))
                    }
                };
                let number = serde_json::Number::from_f64(float).ok_or_else(|| {
                    RuntimeError::new(
                        "R_JSON_TYPE",
                        "JSON number must be finite",
                        instruction.span,
                    )
                })?;
                push_stack!(finish_json_value(
                    serde_json::Value::Number(number),
                    &self.policy.vm_limits(),
                    budget.as_ref(),
                    instruction.span,
                )?);
            }
            Op::JsonText => {
                let text = match pop!() {
                    Value::Text(text) => text,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_text requires Text",
                            instruction.span,
                        ))
                    }
                };
                if text.len() > self.policy.max_text_bytes {
                    return Err(RuntimeError::new(
                        "R_TEXT_LIMIT",
                        "JSON text exceeds the configured byte limit",
                        instruction.span,
                    ));
                }
                push_stack!(finish_json_value(
                    serde_json::Value::String(text.to_string()),
                    &self.policy.vm_limits(),
                    budget.as_ref(),
                    instruction.span,
                )?);
            }
            Op::JsonArray => {
                push_stack!(finish_json_value(
                    serde_json::Value::Array(Vec::new()),
                    &self.policy.vm_limits(),
                    budget.as_ref(),
                    instruction.span,
                )?);
            }
            Op::JsonObject => {
                push_stack!(finish_json_value(
                    serde_json::Value::Object(serde_json::Map::new()),
                    &self.policy.vm_limits(),
                    budget.as_ref(),
                    instruction.span,
                )?);
            }
            Op::JsonPush => {
                let child = match pop!() {
                    Value::Json(value) => value.as_ref().clone(),
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_push value must be Json",
                            instruction.span,
                        ))
                    }
                };
                let mut array = match pop!() {
                    Value::Json(value) => value.as_ref().clone(),
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_push requires a JSON array",
                            instruction.span,
                        ))
                    }
                };
                let Some(items) = array.as_array_mut() else {
                    return Err(RuntimeError::new(
                        "R_JSON_TYPE",
                        "json_push requires an array",
                        instruction.span,
                    ));
                };
                items.push(Arc::new(child));
                push_stack!(finish_shared_json(
                    array,
                    &self.policy.vm_limits(),
                    budget.as_ref(),
                    instruction.span,
                )?);
            }
            Op::JsonSet => {
                let child = match pop!() {
                    Value::Json(value) => value.as_ref().clone(),
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_set value must be Json",
                            instruction.span,
                        ))
                    }
                };
                let key = match pop!() {
                    Value::Text(key) => key,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_set key must be Text",
                            instruction.span,
                        ))
                    }
                };
                if key.len() > self.policy.max_text_bytes {
                    return Err(RuntimeError::new(
                        "R_TEXT_LIMIT",
                        "JSON key exceeds the configured byte limit",
                        instruction.span,
                    ));
                }
                let mut object = match pop!() {
                    Value::Json(value) => value.as_ref().clone(),
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_set requires a JSON object",
                            instruction.span,
                        ))
                    }
                };
                let Some(fields) = object.as_object_mut() else {
                    return Err(RuntimeError::new(
                        "R_JSON_TYPE",
                        "json_set requires an object",
                        instruction.span,
                    ));
                };
                fields.insert(key.to_string(), Arc::new(child));
                push_stack!(finish_shared_json(
                    object,
                    &self.policy.vm_limits(),
                    budget.as_ref(),
                    instruction.span,
                )?);
            }
            Op::JsonParse => {
                let source = match pop!() {
                    Value::Text(text) => text,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_parse requires Text",
                            instruction.span,
                        ))
                    }
                };
                if source.len() > self.policy.max_text_bytes {
                    return Err(RuntimeError::new(
                        "R_TEXT_LIMIT",
                        "JSON input exceeds the configured byte limit",
                        instruction.span,
                    ));
                }
                let value = match parse_unique_shared_json(&source) {
                    Ok(value) => value,
                    Err(JsonDecodeError::Parse(error)) => {
                        push_stack!(Value::ResultErr(Box::new(Value::JsonError(
                            JsonErrorValue {
                                code: "R_JSON_PARSE".into(),
                                message: error.to_string(),
                                line: error.line(),
                                column: error.column(),
                            },
                        ))));
                        return Ok(());
                    }
                    Err(JsonDecodeError::DuplicateKey { key, line, column }) => {
                        push_stack!(Value::ResultErr(Box::new(Value::JsonError(
                            JsonErrorValue {
                                code: "R_JSON_DUPLICATE_KEY".into(),
                                message: format!("JSON object contains duplicate key `{key}`"),
                                line,
                                column,
                            },
                        ))));
                        return Ok(());
                    }
                };
                let value = finish_shared_json(
                    value,
                    &self.policy.vm_limits(),
                    budget.as_ref(),
                    instruction.span,
                )?;
                push_stack!(Value::ResultOk(Box::new(value)));
            }
            Op::JsonStringify => {
                let value = match pop!() {
                    Value::Json(value) => value,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_stringify requires Json",
                            instruction.span,
                        ))
                    }
                };
                let text = serde_json::to_string(value.as_ref()).map_err(|error| {
                    RuntimeError::new("R_JSON_ENCODE", error.to_string(), instruction.span)
                })?;
                if text.len() > self.policy.max_text_bytes {
                    return Err(RuntimeError::new(
                        "R_TEXT_LIMIT",
                        "JSON output exceeds the configured byte limit",
                        instruction.span,
                    ));
                }
                charge_allocation(
                    budget.as_ref(),
                    size_of::<String>().saturating_add(text.len()),
                    instruction.span,
                )?;
                push_stack!(Value::Text(text.into()));
            }
            Op::JsonStringifyPairs => {
                let value_field = match pop!() {
                    Value::Text(value) => value,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_stringify_pairs value field must be Text",
                            instruction.span,
                        ))
                    }
                };
                let key_field = match pop!() {
                    Value::Text(value) => value,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_stringify_pairs key field must be Text",
                            instruction.span,
                        ))
                    }
                };
                let groups = match pop!() {
                    Value::Map(values) => values,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_stringify_pairs requires Map[Text, List[Text]]",
                            instruction.span,
                        ))
                    }
                };
                if key_field == value_field {
                    return Err(RuntimeError::new(
                        "R_JSON_ENCODE",
                        "json_stringify_pairs field names must be distinct",
                        instruction.span,
                    ));
                }
                if groups.len() > self.policy.max_collection_items {
                    return Err(RuntimeError::new(
                        "R_COLLECTION_LIMIT",
                        "json_stringify_pairs exceeds the configured group limit",
                        instruction.span,
                    ));
                }

                let encoded_key_field_len = encoded_json_string_len(&key_field);
                let encoded_value_field_len = encoded_json_string_len(&value_field);
                let mut row_count = 0usize;
                let mut output_length = 2usize;
                for (key, values) in groups.iter() {
                    let Value::List(values) = values else {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_stringify_pairs requires Map[Text, List[Text]]",
                            instruction.span,
                        ));
                    };
                    if values.len() > self.policy.max_collection_items {
                        return Err(RuntimeError::new(
                            "R_COLLECTION_LIMIT",
                            "json_stringify_pairs exceeds the configured group size limit",
                            instruction.span,
                        ));
                    }
                    let encoded_key_len = encoded_json_string_len(key);
                    for value in values.iter() {
                        let Value::Text(value) = value else {
                            return Err(RuntimeError::new(
                                "R_TYPE_INVALID",
                                "json_stringify_pairs requires Map[Text, List[Text]]",
                                instruction.span,
                            ));
                        };
                        if row_count >= self.policy.max_collection_items {
                            return Err(RuntimeError::new(
                                "R_COLLECTION_LIMIT",
                                "json_stringify_pairs exceeds the configured row limit",
                                instruction.span,
                            ));
                        }
                        if row_count > 0 {
                            output_length = output_length.saturating_add(1);
                        }
                        output_length = output_length
                            .saturating_add(5)
                            .saturating_add(encoded_key_field_len)
                            .saturating_add(encoded_key_len)
                            .saturating_add(encoded_value_field_len)
                            .saturating_add(encoded_json_string_len(value));
                        row_count += 1;
                        if output_length > self.policy.max_text_bytes {
                            return Err(RuntimeError::new(
                                "R_TEXT_LIMIT",
                                "JSON pair output exceeds the configured text limit",
                                instruction.span,
                            ));
                        }
                    }
                }
                if output_length > self.policy.max_text_bytes {
                    return Err(RuntimeError::new(
                        "R_TEXT_LIMIT",
                        "JSON pair output exceeds the configured text limit",
                        instruction.span,
                    ));
                }
                charge_allocation(
                    budget.as_ref(),
                    size_of::<String>().saturating_add(output_length),
                    instruction.span,
                )?;

                let mut output = Vec::with_capacity(output_length);
                let rows = JsonTextPairRows {
                    groups: groups.as_ref(),
                    key_field: &key_field,
                    value_field: &value_field,
                    row_count,
                };
                let mut serializer = serde_json::Serializer::new(&mut output);
                serde::Serialize::serialize(&rows, &mut serializer).map_err(|error| {
                    RuntimeError::new("R_JSON_ENCODE", error.to_string(), instruction.span)
                })?;
                if output.len() != output_length {
                    return Err(RuntimeError::new(
                        "R_JSON_ENCODE",
                        "JSON pair output length did not match its checked size",
                        instruction.span,
                    ));
                }
                let text = String::from_utf8(output).map_err(|error| {
                    RuntimeError::new("R_JSON_ENCODE", error.to_string(), instruction.span)
                })?;
                push_stack!(Value::Text(text.into()));
            }
            Op::JsonTryGet
            | Op::JsonTryIndex
            | Op::JsonTryText
            | Op::JsonTryInt
            | Op::JsonTryBool
            | Op::JsonTryFloat => {
                let outcome: Result<Value, JsonErrorValue> = match instruction.op {
                    Op::JsonTryGet => {
                        let key = match pop!() {
                            Value::Text(key) => key,
                            _ => {
                                return Err(RuntimeError::new(
                                    "R_TYPE_INVALID",
                                    "json_try_get key must be Text",
                                    instruction.span,
                                ))
                            }
                        };
                        if key.len() > self.policy.max_text_bytes {
                            return Err(RuntimeError::new(
                                "R_TEXT_LIMIT",
                                "JSON key exceeds the configured byte limit",
                                instruction.span,
                            ));
                        }
                        let value = match pop!() {
                            Value::Json(value) => value,
                            _ => {
                                return Err(RuntimeError::new(
                                    "R_TYPE_INVALID",
                                    "json_try_get requires Json",
                                    instruction.span,
                                ))
                            }
                        };
                        stdlib_data::json_try_get(&value, &key).map(Value::Json)
                    }
                    Op::JsonTryIndex => {
                        let index = match pop!() {
                            Value::Int(index) => index.to_i64().unwrap_or(-1),
                            _ => {
                                return Err(RuntimeError::new(
                                    "R_TYPE_INVALID",
                                    "json_try_index index must be Int",
                                    instruction.span,
                                ))
                            }
                        };
                        let value = match pop!() {
                            Value::Json(value) => value,
                            _ => {
                                return Err(RuntimeError::new(
                                    "R_TYPE_INVALID",
                                    "json_try_index requires Json",
                                    instruction.span,
                                ))
                            }
                        };
                        stdlib_data::json_try_index(&value, index).map(Value::Json)
                    }
                    Op::JsonTryText | Op::JsonTryInt | Op::JsonTryBool | Op::JsonTryFloat => {
                        let value = match pop!() {
                            Value::Json(value) => value,
                            _ => {
                                return Err(RuntimeError::new(
                                    "R_TYPE_INVALID",
                                    "JSON conversion requires Json",
                                    instruction.span,
                                ))
                            }
                        };
                        match instruction.op {
                            Op::JsonTryText => stdlib_data::json_try_text(value.as_ref())
                                .map(|text| Value::Text(text.into())),
                            Op::JsonTryInt => {
                                stdlib_data::json_try_int(value.as_ref()).map(Value::Int)
                            }
                            Op::JsonTryBool => {
                                stdlib_data::json_try_bool(value.as_ref()).map(Value::Bool)
                            }
                            Op::JsonTryFloat => stdlib_data::json_try_float(value.as_ref())
                                .and_then(|number| {
                                    Float64::new(number).map(Value::Float64).ok_or_else(|| {
                                        JsonErrorValue {
                                            code: "E_JSON_TYPE".into(),
                                            message: "JSON number is outside finite Float64 range"
                                                .into(),
                                            line: 0,
                                            column: 0,
                                        }
                                    })
                                }),
                            _ => unreachable!(),
                        }
                    }
                    _ => unreachable!(),
                };
                match outcome {
                    Ok(value) => {
                        if let Value::Int(integer) = &value {
                            if integer.digit_count() > self.policy.max_integer_digits {
                                return Err(RuntimeError::new(
                                    "R_INTEGER_LIMIT",
                                    "JSON integer exceeds the configured digit limit",
                                    instruction.span,
                                ));
                            }
                        }
                        if let Value::Text(text) = &value {
                            charge_allocation(
                                budget.as_ref(),
                                size_of::<String>().saturating_add(text.len()),
                                instruction.span,
                            )?;
                        }
                        push_stack!(Value::ResultOk(Box::new(value)));
                    }
                    Err(error) => {
                        push_stack!(Value::ResultErr(Box::new(Value::JsonError(error))))
                    }
                }
            }
            Op::JsonGet => {
                let key = match pop!() {
                    Value::Text(key) => key,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_get key must be Text",
                            instruction.span,
                        ))
                    }
                };
                let value = match pop!() {
                    Value::Json(value) => value,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_get requires Json",
                            instruction.span,
                        ))
                    }
                };
                let object = value.as_object().ok_or_else(|| {
                    RuntimeError::new(
                        "R_JSON_TYPE",
                        "json_get requires an object",
                        instruction.span,
                    )
                })?;
                let value = object.get(key.as_ref()).cloned().ok_or_else(|| {
                    RuntimeError::new(
                        "R_JSON_KEY",
                        format!("JSON object has no key `{key}`"),
                        instruction.span,
                    )
                })?;
                push_stack!(Value::Json(value));
            }
            Op::JsonIndex => {
                let index = match pop!() {
                    Value::Int(index) => index.to_usize(),
                    _ => None,
                }
                .ok_or_else(|| {
                    RuntimeError::new(
                        "R_JSON_INDEX",
                        "JSON array index must be a non-negative machine-sized Int",
                        instruction.span,
                    )
                })?;
                let value = match pop!() {
                    Value::Json(value) => value,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_index requires Json",
                            instruction.span,
                        ))
                    }
                };
                let array = value.as_array().ok_or_else(|| {
                    RuntimeError::new(
                        "R_JSON_TYPE",
                        "json_index requires an array",
                        instruction.span,
                    )
                })?;
                let value = array.get(index).cloned().ok_or_else(|| {
                    RuntimeError::new(
                        "R_JSON_INDEX",
                        format!(
                            "JSON array index {index} is out of range for length {}",
                            array.len()
                        ),
                        instruction.span,
                    )
                })?;
                push_stack!(Value::Json(value));
            }
            Op::JsonHas => {
                let key = match pop!() {
                    Value::Text(key) => key,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_has key must be Text",
                            instruction.span,
                        ))
                    }
                };
                let value = match pop!() {
                    Value::Json(value) => value,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_has requires Json",
                            instruction.span,
                        ))
                    }
                };
                let object = value.as_object().ok_or_else(|| {
                    RuntimeError::new(
                        "R_JSON_TYPE",
                        "json_has requires an object",
                        instruction.span,
                    )
                })?;
                push_stack!(Value::Bool(object.contains_key(key.as_ref())));
            }
            Op::JsonLength => {
                let value = match pop!() {
                    Value::Json(value) => value,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_len requires Json",
                            instruction.span,
                        ))
                    }
                };
                let length = value
                    .as_array()
                    .map(|items| items.len())
                    .or_else(|| value.as_object().map(|items| items.len()))
                    .ok_or_else(|| {
                        RuntimeError::new(
                            "R_JSON_TYPE",
                            "json_len requires an array or object",
                            instruction.span,
                        )
                    })?;
                push_stack!(Value::Int(BigInt::from_usize(length)));
            }
            Op::JsonIntColumn => {
                let field = match pop!() {
                    Value::Text(field) => field,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_int_column requires Text field",
                            instruction.span,
                        ))
                    }
                };
                let rows = match pop!() {
                    Value::Json(rows) => rows,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_int_column requires Json rows",
                            instruction.span,
                        ))
                    }
                };
                let rows = rows.as_array().ok_or_else(|| {
                    RuntimeError::new(
                        "R_JSON_TYPE",
                        "json_int_column requires an array of objects",
                        instruction.span,
                    )
                })?;
                if rows.len() > self.policy.max_collection_items {
                    return Err(RuntimeError::new(
                        "R_COLLECTION_LIMIT",
                        "JSON column exceeds the configured item limit",
                        instruction.span,
                    ));
                }
                let mut values = Vec::with_capacity(rows.len());
                let mut allocation = size_of::<Vec<Value>>()
                    .saturating_add(rows.len().saturating_mul(size_of::<Value>()));
                for row in rows {
                    let object = row.as_object().ok_or_else(|| {
                        RuntimeError::new(
                            "R_JSON_TYPE",
                            "json_int_column rows must be objects",
                            instruction.span,
                        )
                    })?;
                    let value = object.get(field.as_ref()).ok_or_else(|| {
                        RuntimeError::new(
                            "R_JSON_FIELD",
                            format!("JSON row has no field {field:?}"),
                            instruction.span,
                        )
                    })?;
                    let integer = json_exact_integer(value).ok_or_else(|| {
                        RuntimeError::new(
                            "R_JSON_TYPE",
                            format!("JSON field {field:?} must contain an exact integer"),
                            instruction.span,
                        )
                    })?;
                    if integer.digit_count() > self.policy.max_integer_digits {
                        return Err(RuntimeError::new(
                            "R_INTEGER_LIMIT",
                            "JSON integer exceeds the configured digit limit",
                            instruction.span,
                        ));
                    }
                    allocation = allocation
                        .saturating_add(size_of::<Value>())
                        .saturating_add(integer.digit_count());
                    values.push(Value::Int(integer));
                }
                charge_allocation(budget.as_ref(), allocation, instruction.span)?;
                push_stack!(Value::List(Arc::new(values)));
            }
            Op::JsonTextCategories => {
                let field = match pop!() {
                    Value::Text(field) => field,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_text_categories requires Text field",
                            instruction.span,
                        ))
                    }
                };
                let rows = match pop!() {
                    Value::Json(rows) => rows,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_text_categories requires Json rows",
                            instruction.span,
                        ))
                    }
                };
                let rows = rows.as_array().ok_or_else(|| {
                    RuntimeError::new(
                        "R_JSON_TYPE",
                        "json_text_categories requires an array of objects",
                        instruction.span,
                    )
                })?;
                if rows.len() > self.policy.max_collection_items {
                    return Err(RuntimeError::new(
                        "R_COLLECTION_LIMIT",
                        "JSON category column exceeds the configured item limit",
                        instruction.span,
                    ));
                }
                let mut codes = Vec::with_capacity(rows.len());
                let mut labels = Vec::<String>::new();
                let mut category_codes = HashMap::<String, usize>::new();
                for row in rows {
                    let object = row.as_object().ok_or_else(|| {
                        RuntimeError::new(
                            "R_JSON_TYPE",
                            "json_text_categories rows must be objects",
                            instruction.span,
                        )
                    })?;
                    let value = object.get(field.as_ref()).ok_or_else(|| {
                        RuntimeError::new(
                            "R_JSON_FIELD",
                            format!("JSON row has no field {field:?}"),
                            instruction.span,
                        )
                    })?;
                    let label = value.as_str().ok_or_else(|| {
                        RuntimeError::new(
                            "R_JSON_TYPE",
                            format!("JSON field {field:?} must contain Text"),
                            instruction.span,
                        )
                    })?;
                    let code = if let Some(code) = category_codes.get(label) {
                        *code
                    } else {
                        let code = labels.len();
                        if code >= self.policy.max_collection_items {
                            return Err(RuntimeError::new(
                                "R_COLLECTION_LIMIT",
                                "JSON categories exceed the configured item limit",
                                instruction.span,
                            ));
                        }
                        charge_allocation(
                            budget.as_ref(),
                            size_of::<(String, usize)>()
                                .saturating_add(label.len().saturating_mul(2)),
                            instruction.span,
                        )?;
                        let label = label.to_owned();
                        category_codes.insert(label.clone(), code);
                        labels.push(label);
                        code
                    };
                    codes.push(serde_json::Value::Number(serde_json::Number::from(
                        code as u64,
                    )));
                }
                let labels = labels
                    .into_iter()
                    .map(serde_json::Value::String)
                    .collect::<Vec<_>>();
                let value = serde_json::json!({ "codes": codes, "labels": labels });
                push_stack!(finish_json_value(
                    value,
                    &self.policy.vm_limits(),
                    budget.as_ref(),
                    instruction.span
                )?);
            }
            Op::JsonAsIntList => {
                let value = match pop!() {
                    Value::Json(value) => value,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_as_int_list requires Json",
                            instruction.span,
                        ))
                    }
                };
                let items = value.as_array().ok_or_else(|| {
                    RuntimeError::new(
                        "R_JSON_TYPE",
                        "json_as_int_list requires an array",
                        instruction.span,
                    )
                })?;
                if items.len() > self.policy.max_collection_items {
                    return Err(RuntimeError::new(
                        "R_COLLECTION_LIMIT",
                        "JSON integer list exceeds the configured item limit",
                        instruction.span,
                    ));
                }
                let mut values = Vec::with_capacity(items.len());
                let mut allocation = size_of::<Vec<Value>>()
                    .saturating_add(items.len().saturating_mul(size_of::<Value>()));
                for item in items {
                    let integer = json_exact_integer(item).ok_or_else(|| {
                        RuntimeError::new(
                            "R_JSON_TYPE",
                            "JSON array values must be exact integers",
                            instruction.span,
                        )
                    })?;
                    if integer.digit_count() > self.policy.max_integer_digits {
                        return Err(RuntimeError::new(
                            "R_INTEGER_LIMIT",
                            "JSON integer exceeds the configured digit limit",
                            instruction.span,
                        ));
                    }
                    allocation = allocation
                        .saturating_add(size_of::<Value>())
                        .saturating_add(integer.digit_count());
                    values.push(Value::Int(integer));
                }
                charge_allocation(budget.as_ref(), allocation, instruction.span)?;
                push_stack!(Value::List(Arc::new(values)));
            }
            Op::JsonProjectColumns => {
                let fields = match pop!() {
                    Value::List(fields) => fields
                        .iter()
                        .map(|field| match field {
                            Value::Text(field) => Ok(field.clone()),
                            _ => Err(RuntimeError::new(
                                "R_TYPE_INVALID",
                                "json_project_columns field names must be Text",
                                instruction.span,
                            )),
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_project_columns requires a List[Text] of fields",
                            instruction.span,
                        ))
                    }
                };
                let path = match pop!() {
                    Value::List(path) => path
                        .iter()
                        .map(|part| match part {
                            Value::Text(part) => Ok(part.clone()),
                            _ => Err(RuntimeError::new(
                                "R_TYPE_INVALID",
                                "json_project_columns path segments must be Text",
                                instruction.span,
                            )),
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_project_columns requires a List[Text] path",
                            instruction.span,
                        ))
                    }
                };
                let source = match pop!() {
                    Value::Text(source) => source,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_project_columns requires JSON Text",
                            instruction.span,
                        ))
                    }
                };
                if source.len() > self.policy.max_text_bytes {
                    return Err(RuntimeError::new(
                        "R_TEXT_LIMIT",
                        "JSON input exceeds the configured byte limit",
                        instruction.span,
                    ));
                }
                let path = path.iter().map(|part| part.to_string()).collect();
                let fields = fields.iter().map(|field| field.to_string()).collect();
                let result = json_projection::project_columns(
                    &source,
                    path,
                    fields,
                    self.policy.max_collection_items,
                    self.policy.max_integer_digits,
                );
                match result {
                    Ok(value) => push_stack!(Value::ResultOk(Box::new(finish_json_value(
                        value,
                        &self.policy.vm_limits(),
                        budget.as_ref(),
                        instruction.span,
                    )?))),
                    Err(error) => {
                        push_stack!(Value::ResultErr(Box::new(Value::JsonError(error,))))
                    }
                }
            }
            Op::JsonTextCategoriesValues => {
                let values = match pop!() {
                    Value::Json(values) => values,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "json_text_categories_values requires Json values",
                            instruction.span,
                        ))
                    }
                };
                let values = values.as_array().ok_or_else(|| {
                    RuntimeError::new(
                        "R_JSON_TYPE",
                        "json_text_categories_values requires an array",
                        instruction.span,
                    )
                })?;
                if values.len() > self.policy.max_collection_items {
                    return Err(RuntimeError::new(
                        "R_COLLECTION_LIMIT",
                        "JSON category column exceeds the configured item limit",
                        instruction.span,
                    ));
                }
                let mut codes = Vec::with_capacity(values.len());
                let mut labels = Vec::<String>::new();
                let mut category_codes = HashMap::<String, usize>::new();
                for value in values {
                    let label = value.as_str().ok_or_else(|| {
                        RuntimeError::new(
                            "R_JSON_TYPE",
                            "JSON category values must be Text",
                            instruction.span,
                        )
                    })?;
                    let code = if let Some(code) = category_codes.get(label) {
                        *code
                    } else {
                        let code = labels.len();
                        if code >= self.policy.max_collection_items {
                            return Err(RuntimeError::new(
                                "R_COLLECTION_LIMIT",
                                "JSON categories exceed the configured item limit",
                                instruction.span,
                            ));
                        }
                        charge_allocation(
                            budget.as_ref(),
                            size_of::<(String, usize)>()
                                .saturating_add(label.len().saturating_mul(2)),
                            instruction.span,
                        )?;
                        let label = label.to_owned();
                        category_codes.insert(label.clone(), code);
                        labels.push(label);
                        code
                    };
                    codes.push(serde_json::Value::Number(serde_json::Number::from(
                        code as u64,
                    )));
                }
                let labels = labels
                    .into_iter()
                    .map(serde_json::Value::String)
                    .collect::<Vec<_>>();
                let result = serde_json::json!({ "codes": codes, "labels": labels });
                push_stack!(finish_json_value(
                    result,
                    &self.policy.vm_limits(),
                    budget.as_ref(),
                    instruction.span
                )?);
            }
            Op::JsonAsText => match pop!() {
                Value::Json(value) => match value.as_ref() {
                    SharedJson::String(text) => {
                        charge_allocation(
                            budget.as_ref(),
                            size_of::<String>().saturating_add(text.len()),
                            instruction.span,
                        )?;
                        push_stack!(Value::Text(text.to_string().into()))
                    }
                    _ => {
                        return Err(RuntimeError::new(
                            "R_JSON_TYPE",
                            "JSON value is not Text",
                            instruction.span,
                        ))
                    }
                },
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "json_as_text requires Json",
                        instruction.span,
                    ))
                }
            },
            Op::JsonAsInt => match pop!() {
                Value::Json(value) if value.is_number() => {
                    let integer = json_exact_integer(value.as_ref()).ok_or_else(|| {
                        RuntimeError::new(
                            "R_JSON_TYPE",
                            "JSON number is not an exact integer",
                            instruction.span,
                        )
                    })?;
                    if integer.digit_count() > self.policy.max_integer_digits {
                        return Err(RuntimeError::new(
                            "R_INTEGER_LIMIT",
                            "JSON integer exceeds the configured digit limit",
                            instruction.span,
                        ));
                    }
                    charge_allocation(
                        budget.as_ref(),
                        size_of::<BigInt>().saturating_add(integer.digit_count()),
                        instruction.span,
                    )?;
                    push_stack!(Value::Int(integer));
                }
                Value::Json(_) => {
                    return Err(RuntimeError::new(
                        "R_JSON_TYPE",
                        "JSON value is not an integer",
                        instruction.span,
                    ))
                }
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "json_as_int requires Json",
                        instruction.span,
                    ))
                }
            },
            Op::JsonAsFloat => match pop!() {
                Value::Json(value) if value.is_number() => {
                    let number = value
                        .as_number()
                        .and_then(serde_json::Number::as_f64)
                        .ok_or_else(|| {
                            RuntimeError::new(
                                "R_JSON_TYPE",
                                "JSON number cannot be represented as Float64",
                                instruction.span,
                            )
                        })?;
                    let value = Float64::new(number).ok_or_else(|| {
                        RuntimeError::new(
                            "R_JSON_TYPE",
                            "JSON number is outside finite Float64 range",
                            instruction.span,
                        )
                    })?;
                    push_stack!(Value::Float64(value));
                }
                Value::Json(_) => {
                    return Err(RuntimeError::new(
                        "R_JSON_TYPE",
                        "JSON value is not a number",
                        instruction.span,
                    ))
                }
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "json_as_float requires Json",
                        instruction.span,
                    ))
                }
            },
            Op::JsonAsBool => match pop!() {
                Value::Json(value) if value.is_boolean() => {
                    push_stack!(Value::Bool(
                        value.as_bool().expect("boolean type was checked")
                    ))
                }
                Value::Json(_) => {
                    return Err(RuntimeError::new(
                        "R_JSON_TYPE",
                        "JSON value is not Bool",
                        instruction.span,
                    ))
                }
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "json_as_bool requires Json",
                        instruction.span,
                    ))
                }
            },
            Op::JsonIsNull => match pop!() {
                Value::Json(value) => push_stack!(Value::Bool(value.is_null())),
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "json_is_null requires Json",
                        instruction.span,
                    ))
                }
            },
            other => {
                return Err(RuntimeError::new(
                    "R_HOST_UNSUPPORTED",
                    format!("instruction {other:?} is not implemented by the runtime host"),
                    instruction.span,
                ))
            }
        }
        Ok(())
    }
}

impl Instance {
    /// Return diagnostic counters for the latest invocation when profiling is enabled.
    pub fn runtime_profile(&self) -> Option<serde_json::Value> {
        self.runtime_profile_metrics
            .as_ref()
            .map(|profile| profile.to_json())
    }

    pub fn native_function_count(&self) -> usize {
        self.native
            .as_ref()
            .map(NativeBackend::function_count)
            .unwrap_or(0)
    }

    pub fn native_function_names(&self) -> Vec<String> {
        self.native
            .as_ref()
            .map(|native| native.compiled_function_names(&self.artifact))
            .unwrap_or_default()
    }

    pub fn native_calls(&self) -> usize {
        self.native_calls.load(Ordering::Relaxed)
    }

    pub fn native_deoptimizations(&self) -> usize {
        self.native_deoptimizations.load(Ordering::Relaxed)
    }

    pub fn native_compilations(&self) -> usize {
        self.native_compilations.load(Ordering::Relaxed)
    }

    /// Number of native file readers still owned by this instance.
    pub fn open_file_reader_count(&self) -> usize {
        self.readers.open_count()
    }

    /// Number of native filesystem, database, and TOML handles owned by this instance.
    pub fn open_native_resource_count(&self) -> usize {
        self.readers.open_count() + self.databases.open_count() + self.toml_documents.open_count()
    }

    fn shared_http_client(&mut self, span: Span) -> Result<Arc<HostHttpClient>, RuntimeError> {
        if self.http.is_none() {
            let client = HostHttpClient::new(self.policy.max_parallelism)
                .map_err(|error| RuntimeError::new("R_HTTP_CLIENT", error, span))?;
            self.http = Some(Arc::new(client));
        }
        self.http
            .as_ref()
            .cloned()
            .ok_or_else(|| RuntimeError::new("R_HTTP_CLIENT", "HTTP client is unavailable", span))
    }

    pub fn native_bytecode_instructions(&self) -> usize {
        self.native
            .as_ref()
            .map_or(0, NativeBackend::bytecode_instruction_count)
    }

    pub fn native_bytecode_blocks(&self) -> usize {
        self.native
            .as_ref()
            .map_or(0, NativeBackend::bytecode_block_count)
    }

    pub fn native_cranelift_ir_blocks(&self) -> usize {
        self.native
            .as_ref()
            .map_or(0, NativeBackend::cranelift_ir_block_count)
    }

    pub fn native_cranelift_ir_instructions(&self) -> usize {
        self.native
            .as_ref()
            .map_or(0, NativeBackend::cranelift_ir_instruction_count)
    }

    pub fn native_compile_profile(&self) -> serde_json::Value {
        self.native.as_ref().map_or_else(
            || serde_json::json!({"functions": [], "finalize_nanoseconds": 0}),
            NativeBackend::compile_profile_json,
        )
    }

    pub fn invoke(&mut self, export: &str, args: Vec<Value>) -> Result<Value, RuntimeError> {
        self.invoke_with_cancellation(export, args, CancellationToken::new())
    }

    /// Invoke an export with a cancellation token that may be cloned and cancelled by the host.
    /// The token belongs to this invocation and should not be reused after cancellation.
    pub fn invoke_with_cancellation(
        &mut self,
        export: &str,
        args: Vec<Value>,
        cancellation: CancellationToken,
    ) -> Result<Value, RuntimeError> {
        if self.closed {
            return Err(RuntimeError::new(
                "R_INSTANCE_CLOSED",
                "runtime instance is closed",
                Span::default(),
            ));
        }
        let Some(index) = self
            .artifact
            .functions
            .iter()
            .position(|f| f.name == export)
        else {
            return Err(RuntimeError::new(
                "R_EXPORT_UNKNOWN",
                format!("unknown function `{export}`"),
                Span::default(),
            ));
        };
        self.output.clear();
        self.program_stderr.clear();
        self.host_stderr.clear();
        self.effect_report.clear();
        self.omitted_effects = 0;
        self.worker_threads.clear();
        self.readers.close_all();
        self.databases.close_all();
        self.toml_documents.close_all();
        if args.len() != self.artifact.functions[index].parameters.len() {
            return Err(RuntimeError::new(
                "R_CALL_ARITY",
                format!(
                    "function `{export}` expects {} arguments, got {}",
                    self.artifact.functions[index].parameters.len(),
                    args.len()
                ),
                Span::default(),
            ));
        }
        let runtime_profile = self
            .runtime_profile_enabled
            .then(|| Arc::new(RuntimeProfileCounters::default()));
        self.runtime_profile_metrics = runtime_profile.clone();
        let budget = Arc::new(Budget::new(
            self.policy.instruction_limit,
            self.policy.allocation_limit_bytes,
            cancellation,
            runtime_profile.clone(),
        ));
        if self.native_enabled && self.native.is_none() {
            self.native = NativeBackend::compile(
                &self.artifact,
                NativeSymbols {
                    reserve_fuel: native_reserve_fuel as *const () as *const u8,
                    release_fuel: native_release_fuel as *const () as *const u8,
                    poll_cancellation: native_poll_cancellation as *const () as *const u8,
                    make_list: native_make_list as *const () as *const u8,
                    list_set_float_copy: native_list_set_float_copy as *const () as *const u8,
                    list_set_int_copy: native_list_set_int_copy as *const () as *const u8,
                    list_set_bool_copy: native_list_set_bool_copy as *const () as *const u8,
                    list_set_float_local: native_list_set_float_local as *const () as *const u8,
                    list_set_int_local: native_list_set_int_local as *const () as *const u8,
                    list_set_bool_local: native_list_set_bool_local as *const () as *const u8,
                    charge_record: native_charge_record as *const () as *const u8,
                    print: native_print as *const () as *const u8,
                    count_guest_branch: native_count_guest_branch as *const () as *const u8,
                },
                runtime_profile.is_some(),
            );
            if let Some(native) = self.native.as_ref() {
                self.native_code = Some(native.shared_code());
                self.native_compilations.fetch_add(1, Ordering::Relaxed);
            }
        }
        for (value, ty) in args.iter().zip(&self.artifact.functions[index].parameters) {
            if !value_matches(value, ty, &self.artifact.records) {
                return Err(RuntimeError::new(
                    "R_CALL_TYPE",
                    format!("argument type mismatch: expected {ty:?}"),
                    Span::default(),
                ));
            }
            if let Some(error) = value_limit_error_at_boundary(value, &self.policy, Span::default())
            {
                return Err(error);
            }
            charge_allocation(
                budget.as_ref(),
                input_value_allocation(value),
                Span::default(),
            )?;
        }
        let result = self.invoke_inner(index, args, 0, budget);
        self.readers.close_all();
        self.databases.close_all();
        self.toml_documents.close_all();
        result
    }
    pub fn take_output(&mut self) -> String {
        std::mem::take(&mut self.output)
    }
    pub fn take_program_stderr(&mut self) -> String {
        std::mem::take(&mut self.program_stderr)
    }
    pub fn close(&mut self) {
        self.closed = true;
        if let Some(cancelled) = &self.cancelled {
            cancelled.store(true, Ordering::Relaxed);
        }
        self.readers.close_all();
        self.databases.close_all();
        self.toml_documents.close_all();
        self.http = None;
        self.output.clear();
        self.program_stderr.clear();
        self.host_stderr.clear();
        #[cfg(feature = "python-interop")]
        if let Some(bridge) = &mut self.python_bridge {
            bridge.close();
        }
    }
    pub fn take_host_stderr(&mut self) -> String {
        std::mem::take(&mut self.host_stderr)
    }
    pub fn python_native_image_observation(&self) -> &str {
        &self.python_native_image_observation
    }
    pub fn python_native_images(&self) -> &[String] {
        &self.python_native_images
    }
    pub fn parallel_threads_used(&self) -> usize {
        self.worker_threads.len()
    }

    /// Effect attempts from the most recent invocation and their last observed state.
    pub fn effect_report(&self) -> &[EffectRecord] {
        &self.effect_report
    }

    /// Number of additional effect attempts omitted after the report reached its cap.
    pub fn omitted_effects(&self) -> usize {
        self.omitted_effects
    }

    fn begin_effect(&mut self, kind: EffectKind, target: impl AsRef<str>) -> usize {
        if self.effect_report.len() >= MAX_EFFECT_REPORT_ENTRIES {
            self.omitted_effects = self.omitted_effects.saturating_add(1);
            return usize::MAX;
        }
        let source = target.as_ref();
        let mut target = source
            .chars()
            .take(MAX_EFFECT_TARGET_CHARS)
            .collect::<String>();
        let target_truncated = source.chars().nth(MAX_EFFECT_TARGET_CHARS).is_some();
        if target_truncated {
            target.push('…');
        }
        let index = self.effect_report.len();
        self.effect_report.push(EffectRecord {
            kind,
            target,
            target_truncated,
            outcome: EffectOutcome::Started,
        });
        index
    }

    fn finish_effect(&mut self, index: usize, outcome: EffectOutcome) {
        if index == usize::MAX {
            return;
        }
        if let Some(effect) = self.effect_report.get_mut(index) {
            effect.outcome = outcome;
        }
    }

    #[cfg(feature = "python-interop")]
    fn record_python_native_images(&mut self, observation: &str, images: &[String]) {
        self.python_native_image_observation = observation.to_owned();
        for image in images {
            if !self.python_native_images.contains(image) {
                self.python_native_images.push(image.clone());
            }
        }
    }

    fn invoke_inner(
        &mut self,
        index: usize,
        args: Vec<Value>,
        depth: usize,
        budget: Arc<Budget>,
    ) -> Result<Value, RuntimeError> {
        match self.start_guest_frame(index, args, depth, budget.clone(), None)? {
            FrameStart::Returned(value) => Ok(value),
            FrameStart::Vm(frame) => self.run_guest_frames(frame, budget),
        }
    }

    fn sort_by_left_before_or_equal(
        &mut self,
        comparator: usize,
        left: &Value,
        right: &Value,
        depth: usize,
        budget: Arc<Budget>,
        span: Span,
    ) -> Result<bool, RuntimeError> {
        let ordering = match self.invoke_inner(
            comparator,
            vec![left.clone(), right.clone()],
            depth + 1,
            budget,
        ) {
            Ok(Value::Int(ordering)) => ordering,
            Ok(_) => {
                return Err(RuntimeError::new(
                    "R_TYPE_INVALID",
                    "sort comparator must return Int",
                    span,
                ))
            }
            Err(mut error) => {
                error.trace.push(span);
                return Err(error);
            }
        };
        Ok(ordering.is_negative() || ordering.is_zero())
    }

    fn start_guest_frame(
        &mut self,
        index: usize,
        args: Vec<Value>,
        depth: usize,
        budget: Arc<Budget>,
        call_site: Option<Span>,
    ) -> Result<FrameStart, RuntimeError> {
        let artifact = Arc::clone(&self.artifact);
        let f = &artifact.functions[index];
        check_call_depth(&artifact, &self.policy.vm_limits(), index, depth)?;
        let mut native_arena = NativeListArena::new(
            Arc::as_ptr(&budget),
            self.policy.max_collection_items,
            self.policy.output_limit_bytes,
            self.output.len().saturating_add(self.program_stderr.len()),
            budget.runtime_profile.clone(),
        );
        let mut native_record_fields: Vec<Box<[i64]>> = Vec::new();
        let mut native_records: Vec<Box<NativeRecord>> = Vec::new();
        let native_args = args
            .iter()
            .zip(&f.parameters)
            .map(|(value, ty)| match (value, ty) {
                (Value::Int(integer), Type::Int) => integer.to_i64(),
                (Value::Float64(float), Type::Float64) => Some(float.get().to_bits() as i64),
                (Value::Float64List(items), Type::List(item)) if **item == Type::Float64 => {
                    let storage_id = native_arena.float_storage.len();
                    native_arena.float_storage.push(items.as_ref().clone());
                    native_arena.float_lists.push(Box::new(NativeFloatList {
                        data: native_arena
                            .float_storage
                            .last_mut()?
                            .as_mut_ptr()
                            .cast::<u64>(),
                        len: items.len(),
                        storage_id,
                        owned: false,
                        inline_storage: false,
                        inline: [0; NATIVE_INLINE_LIST_ITEMS],
                    }));
                    Some((&**native_arena.float_lists.last()? as *const NativeFloatList) as i64)
                }
                (Value::List(items), Type::List(item)) if **item == Type::Float64 => {
                    let values: Vec<f64> = items
                        .iter()
                        .map(|value| match value {
                            Value::Float64(value) => Some(value.get()),
                            _ => None,
                        })
                        .collect::<Option<_>>()?;
                    let storage_id = native_arena.float_storage.len();
                    native_arena.float_storage.push(
                        values
                            .into_iter()
                            .map(Float64::new)
                            .collect::<Option<Vec<_>>>()?,
                    );
                    native_arena.float_lists.push(Box::new(NativeFloatList {
                        data: native_arena
                            .float_storage
                            .last_mut()?
                            .as_mut_ptr()
                            .cast::<u64>(),
                        len: items.len(),
                        storage_id,
                        owned: false,
                        inline_storage: false,
                        inline: [0; NATIVE_INLINE_LIST_ITEMS],
                    }));
                    Some((&**native_arena.float_lists.last()? as *const NativeFloatList) as i64)
                }
                (Value::List(items), Type::List(item)) if **item == Type::Int => {
                    let values: Vec<i64> = items
                        .iter()
                        .map(|value| match value {
                            Value::Int(value) => value.to_i64(),
                            _ => None,
                        })
                        .collect::<Option<_>>()?;
                    let storage_id = native_arena.int_storage.len();
                    native_arena.int_storage.push(values);
                    native_arena.int_lists.push(Box::new(NativeIntList {
                        data: native_arena.int_storage.last_mut()?.as_mut_ptr(),
                        len: items.len(),
                        storage_id,
                        owned: false,
                        inline_storage: false,
                        inline: [0; NATIVE_INLINE_LIST_ITEMS],
                    }));
                    Some((&**native_arena.int_lists.last()? as *const NativeIntList) as i64)
                }
                (Value::List(items), Type::List(item)) if **item == Type::Bool => {
                    let values: Vec<i64> = items
                        .iter()
                        .map(|value| match value {
                            Value::Bool(value) => Some(i64::from(*value)),
                            _ => None,
                        })
                        .collect::<Option<_>>()?;
                    let storage_id = native_arena.int_storage.len();
                    native_arena.int_storage.push(values);
                    native_arena.int_lists.push(Box::new(NativeIntList {
                        data: native_arena.int_storage.last_mut()?.as_mut_ptr(),
                        len: items.len(),
                        storage_id,
                        owned: false,
                        inline_storage: false,
                        inline: [0; NATIVE_INLINE_LIST_ITEMS],
                    }));
                    Some((&**native_arena.int_lists.last()? as *const NativeIntList) as i64)
                }
                (Value::Record(name, fields), Type::Record(expected)) if name == expected => {
                    let definition = self
                        .artifact
                        .records
                        .iter()
                        .find(|record| record.name == *name)?;
                    if fields.len() != definition.fields.len() {
                        return None;
                    }
                    let mut encoded_fields = Vec::with_capacity(fields.len());
                    for (value, field) in fields.iter().zip(&definition.fields) {
                        let encoded = match (value, &field.ty) {
                            (Value::Int(integer), Type::Int) => integer.to_i64()?,
                            (Value::Float64(float), Type::Float64) => float.get().to_bits() as i64,
                            (Value::Bool(value), Type::Bool) => i64::from(*value),
                            (Value::Float64List(items), Type::List(item))
                                if **item == Type::Float64 =>
                            {
                                let storage_id = native_arena.float_storage.len();
                                native_arena.float_storage.push(items.as_ref().clone());
                                native_arena.float_lists.push(Box::new(NativeFloatList {
                                    data: native_arena
                                        .float_storage
                                        .last_mut()?
                                        .as_mut_ptr()
                                        .cast::<u64>(),
                                    len: items.len(),
                                    storage_id,
                                    owned: false,
                                    inline_storage: false,
                                    inline: [0; NATIVE_INLINE_LIST_ITEMS],
                                }));
                                (&**native_arena.float_lists.last()? as *const NativeFloatList)
                                    as i64
                            }
                            (Value::List(items), Type::List(item)) if **item == Type::Float64 => {
                                let values: Vec<f64> = items
                                    .iter()
                                    .map(|value| match value {
                                        Value::Float64(value) => Some(value.get()),
                                        _ => None,
                                    })
                                    .collect::<Option<_>>()?;
                                let storage_id = native_arena.float_storage.len();
                                native_arena.float_storage.push(
                                    values
                                        .into_iter()
                                        .map(Float64::new)
                                        .collect::<Option<Vec<_>>>()?,
                                );
                                native_arena.float_lists.push(Box::new(NativeFloatList {
                                    data: native_arena
                                        .float_storage
                                        .last_mut()?
                                        .as_mut_ptr()
                                        .cast::<u64>(),
                                    len: items.len(),
                                    storage_id,
                                    owned: false,
                                    inline_storage: false,
                                    inline: [0; NATIVE_INLINE_LIST_ITEMS],
                                }));
                                (&**native_arena.float_lists.last()? as *const NativeFloatList)
                                    as i64
                            }
                            (Value::List(items), Type::List(item)) if **item == Type::Int => {
                                let values: Vec<i64> = items
                                    .iter()
                                    .map(|value| match value {
                                        Value::Int(value) => value.to_i64(),
                                        _ => None,
                                    })
                                    .collect::<Option<_>>()?;
                                let storage_id = native_arena.int_storage.len();
                                native_arena.int_storage.push(values);
                                native_arena.int_lists.push(Box::new(NativeIntList {
                                    data: native_arena.int_storage.last_mut()?.as_mut_ptr(),
                                    len: items.len(),
                                    storage_id,
                                    owned: false,
                                    inline_storage: false,
                                    inline: [0; NATIVE_INLINE_LIST_ITEMS],
                                }));
                                (&**native_arena.int_lists.last()? as *const NativeIntList) as i64
                            }
                            (Value::List(items), Type::List(item)) if **item == Type::Bool => {
                                let values: Vec<i64> = items
                                    .iter()
                                    .map(|value| match value {
                                        Value::Bool(value) => Some(i64::from(*value)),
                                        _ => None,
                                    })
                                    .collect::<Option<_>>()?;
                                let storage_id = native_arena.int_storage.len();
                                native_arena.int_storage.push(values);
                                native_arena.int_lists.push(Box::new(NativeIntList {
                                    data: native_arena.int_storage.last_mut()?.as_mut_ptr(),
                                    len: items.len(),
                                    storage_id,
                                    owned: false,
                                    inline_storage: false,
                                    inline: [0; NATIVE_INLINE_LIST_ITEMS],
                                }));
                                (&**native_arena.int_lists.last()? as *const NativeIntList) as i64
                            }
                            _ => return None,
                        };
                        encoded_fields.push(encoded);
                    }
                    let fields = encoded_fields.into_boxed_slice();
                    let descriptor = Box::new(NativeRecord {
                        fields: fields.as_ptr(),
                        len: fields.len(),
                    });
                    native_record_fields.push(fields);
                    native_records.push(descriptor);
                    Some((&**native_records.last()? as *const NativeRecord) as i64)
                }
                _ => None,
            })
            .collect::<Option<Vec<_>>>();
        if let Some(native_args) = native_args {
            if let Some(outcome) = self.native_code.as_ref().and_then(|native| {
                native.invoke(
                    index,
                    &native_args,
                    Arc::as_ptr(&budget) as *const std::ffi::c_void,
                    depth,
                    self.policy.call_depth_limit,
                    &mut native_arena,
                )
            }) {
                self.native_calls
                    .fetch_add(outcome.calls, Ordering::Relaxed);
                let fault_frame = self
                    .artifact
                    .functions
                    .get(outcome.fault_function)
                    .unwrap_or(f);
                match outcome.status {
                    0 => {
                        let value = match &f.result {
                            Type::Int => {
                                Value::Int(BigInt::parse(&outcome.value.to_string()).unwrap())
                            }
                            Type::Float64 => Value::Float64(
                                Float64::new(f64::from_bits(outcome.value as u64)).unwrap(),
                            ),
                            Type::Bool => Value::Bool(outcome.value != 0),
                            Type::Record(name) => {
                                let definition = self
                                    .artifact
                                    .records
                                    .iter()
                                    .find(|record| record.name == *name)
                                    .expect("native record result has a verified definition");
                                let mut fields = Vec::with_capacity(definition.fields.len());
                                let mut decoded_float_lists =
                                    std::collections::HashMap::<usize, Arc<Vec<Float64>>>::new();
                                let mut decoded_int_lists =
                                    std::collections::HashMap::<usize, Arc<Vec<Value>>>::new();
                                let mut decoded_bool_lists =
                                    std::collections::HashMap::<usize, Arc<Vec<Value>>>::new();
                                for (field_type, encoded) in definition
                                    .fields
                                    .iter()
                                    .map(|field| &field.ty)
                                    .zip(&outcome.result_values)
                                {
                                    let field = match field_type {
                                        Type::Int => {
                                            Value::Int(BigInt::parse(&encoded.to_string()).unwrap())
                                        }
                                        Type::Float64 => Value::Float64(
                                            Float64::new(f64::from_bits(*encoded as u64)).unwrap(),
                                        ),
                                        Type::Bool => Value::Bool(*encoded != 0),
                                        Type::List(item) if **item == Type::Float64 => {
                                            let descriptor =
                                                *encoded as usize as *const NativeFloatList;
                                            if descriptor.is_null() {
                                                return Err(RuntimeError::new(
                                                    "R_NATIVE_RESULT",
                                                    "native record returned a null list descriptor",
                                                    f.code
                                                        .last()
                                                        .map(|instruction| instruction.span)
                                                        .unwrap_or_default(),
                                                ));
                                            }
                                            let list_key = descriptor as usize;
                                            let values: Arc<Vec<Float64>> = if let Some(values) =
                                                decoded_float_lists.get(&list_key)
                                            {
                                                values.clone()
                                            } else {
                                                let descriptor = unsafe { &*descriptor };
                                                let values: Arc<Vec<Float64>> = if descriptor
                                                    .inline_storage
                                                {
                                                    let encoded = unsafe {
                                                        std::slice::from_raw_parts(
                                                            descriptor.data,
                                                            descriptor.len,
                                                        )
                                                    };
                                                    Arc::new(
                                                        encoded
                                                            .iter()
                                                            .map(|bits| {
                                                                Float64::new(f64::from_bits(*bits))
                                                                    .unwrap()
                                                            })
                                                            .collect(),
                                                    )
                                                } else {
                                                    let Some(storage) = native_arena
                                                        .float_storage
                                                        .get_mut(descriptor.storage_id)
                                                    else {
                                                        return Err(RuntimeError::new(
                                                            "R_NATIVE_RESULT",
                                                            "native record returned an invalid list storage handle",
                                                            f.code.last().map(|instruction| instruction.span).unwrap_or_default(),
                                                        ));
                                                    };
                                                    Arc::new(std::mem::take(storage))
                                                };
                                                decoded_float_lists
                                                    .insert(list_key, values.clone());
                                                values
                                            };
                                            Value::Float64List(values)
                                        }
                                        Type::List(item) if **item == Type::Int => {
                                            let descriptor =
                                                *encoded as usize as *const NativeIntList;
                                            if descriptor.is_null() {
                                                return Err(RuntimeError::new(
                                                    "R_NATIVE_RESULT",
                                                    "native record returned a null integer-list descriptor",
                                                    f.code.last().map(|instruction| instruction.span).unwrap_or_default(),
                                                ));
                                            }
                                            let list_key = descriptor as usize;
                                            let values: Arc<Vec<Value>> = if let Some(values) =
                                                decoded_int_lists.get(&list_key)
                                            {
                                                values.clone()
                                            } else {
                                                let descriptor = unsafe { &*descriptor };
                                                let values: Arc<Vec<Value>> = if descriptor
                                                    .inline_storage
                                                {
                                                    let encoded = unsafe {
                                                        std::slice::from_raw_parts(
                                                            descriptor.data,
                                                            descriptor.len,
                                                        )
                                                    };
                                                    Arc::new(
                                                        encoded
                                                            .iter()
                                                            .map(|value| {
                                                                Value::Int(
                                                                    BigInt::parse(
                                                                        &value.to_string(),
                                                                    )
                                                                    .unwrap(),
                                                                )
                                                            })
                                                            .collect(),
                                                    )
                                                } else {
                                                    let Some(storage) = native_arena
                                                        .int_storage
                                                        .get_mut(descriptor.storage_id)
                                                    else {
                                                        return Err(RuntimeError::new(
                                                            "R_NATIVE_RESULT",
                                                            "native record returned an invalid integer-list storage handle",
                                                            f.code.last().map(|instruction| instruction.span).unwrap_or_default(),
                                                        ));
                                                    };
                                                    Arc::new(
                                                        std::mem::take(storage)
                                                            .into_iter()
                                                            .map(|value| {
                                                                Value::Int(
                                                                    BigInt::parse(
                                                                        &value.to_string(),
                                                                    )
                                                                    .unwrap(),
                                                                )
                                                            })
                                                            .collect(),
                                                    )
                                                };
                                                decoded_int_lists.insert(list_key, values.clone());
                                                values
                                            };
                                            Value::List(values)
                                        }
                                        Type::List(item) if **item == Type::Bool => {
                                            let descriptor =
                                                *encoded as usize as *const NativeIntList;
                                            if descriptor.is_null() {
                                                return Err(RuntimeError::new(
                                                    "R_NATIVE_RESULT",
                                                    "native record returned a null boolean-list descriptor",
                                                    f.code.last().map(|instruction| instruction.span).unwrap_or_default(),
                                                ));
                                            }
                                            let list_key = descriptor as usize;
                                            let values: Arc<Vec<Value>> = if let Some(values) =
                                                decoded_bool_lists.get(&list_key)
                                            {
                                                values.clone()
                                            } else {
                                                let descriptor = unsafe { &*descriptor };
                                                let values: Arc<Vec<Value>> = if descriptor
                                                    .inline_storage
                                                {
                                                    let encoded = unsafe {
                                                        std::slice::from_raw_parts(
                                                            descriptor.data,
                                                            descriptor.len,
                                                        )
                                                    };
                                                    Arc::new(
                                                        encoded
                                                            .iter()
                                                            .map(|value| Value::Bool(*value != 0))
                                                            .collect(),
                                                    )
                                                } else {
                                                    let Some(storage) = native_arena
                                                        .int_storage
                                                        .get_mut(descriptor.storage_id)
                                                    else {
                                                        return Err(RuntimeError::new(
                                                            "R_NATIVE_RESULT",
                                                            "native record returned an invalid boolean-list storage handle",
                                                            f.code.last().map(|instruction| instruction.span).unwrap_or_default(),
                                                        ));
                                                    };
                                                    Arc::new(
                                                        std::mem::take(storage)
                                                            .into_iter()
                                                            .map(|value| Value::Bool(value != 0))
                                                            .collect(),
                                                    )
                                                };
                                                decoded_bool_lists.insert(list_key, values.clone());
                                                values
                                            };
                                            Value::List(values)
                                        }
                                        _ => unreachable!(
                                            "native result fields are checked at compile time"
                                        ),
                                    };
                                    fields.push(field);
                                }
                                Value::Record(name.clone(), Arc::new(fields))
                            }
                            _ => unreachable!(
                                "native backend only compiles numeric and record results"
                            ),
                        };
                        self.output.push_str(&native_arena.pending_output);
                        for bytes in &native_arena.pending_print_bytes {
                            let effect = self
                                .begin_effect(EffectKind::ProgramOutput, format!("{bytes} bytes"));
                            self.finish_effect(effect, EffectOutcome::Returned);
                        }
                        return Ok(FrameStart::Returned(value));
                    }
                    1 => {
                        self.native_deoptimizations.fetch_add(1, Ordering::Relaxed);
                        budget.refund(outcome.steps);
                        budget.refund_allocation(outcome.allocations);
                    }
                    2 => {
                        let span = fault_frame
                            .code
                            .get(outcome.fault_ip)
                            .map(|i| i.span)
                            .unwrap_or_default();
                        return Err(native_runtime_error(
                            &self.artifact,
                            &outcome.trace,
                            "R_DIV_ZERO",
                            "integer division by zero",
                            span,
                        ));
                    }
                    3 => {
                        let span = fault_frame
                            .code
                            .get(outcome.fault_ip)
                            .map(|i| i.span)
                            .unwrap_or_default();
                        return Err(native_runtime_error(
                            &self.artifact,
                            &outcome.trace,
                            "R_STEP_LIMIT",
                            "instruction budget exhausted",
                            span,
                        ));
                    }
                    4 => {
                        let span = fault_frame
                            .code
                            .get(outcome.fault_ip)
                            .map(|i| i.span)
                            .unwrap_or_default();
                        return Err(native_runtime_error(
                            &self.artifact,
                            &outcome.trace,
                            "R_FLOAT_NONFINITE",
                            "Float64 operation produced a non-finite value",
                            span,
                        ));
                    }
                    5 => {
                        let span = fault_frame.code.first().map(|i| i.span).unwrap_or_default();
                        return Err(native_runtime_error(
                            &self.artifact,
                            &outcome.trace,
                            "R_CALL_DEPTH",
                            format!("call depth exceeds {}", self.policy.call_depth_limit),
                            span,
                        ));
                    }
                    6 => {
                        let span = fault_frame
                            .code
                            .get(outcome.fault_ip)
                            .map(|i| i.span)
                            .unwrap_or_default();
                        return Err(native_runtime_error(
                            &self.artifact,
                            &outcome.trace,
                            "R_CANCELLED",
                            "invocation was cancelled",
                            span,
                        ));
                    }
                    7 => {
                        let span = fault_frame
                            .code
                            .get(outcome.fault_ip)
                            .map(|i| i.span)
                            .unwrap_or_default();
                        return Err(native_runtime_error(
                            &self.artifact,
                            &outcome.trace,
                            "R_MATH_DOMAIN",
                            "sqrt requires a non-negative value",
                            span,
                        ));
                    }
                    _ => {
                        return Err(RuntimeError::new(
                            "R_NATIVE_INVALID",
                            "native function returned an invalid status",
                            f.code.first().map(|i| i.span).unwrap_or_default(),
                        ));
                    }
                }
            }
        }
        Ok(FrameStart::Vm(new_vm_frame(
            &artifact, index, args, depth, call_site,
        )))
    }

    fn run_guest_frames(
        &mut self,
        initial: VmFrame,
        budget: Arc<Budget>,
    ) -> Result<Value, RuntimeError> {
        guion_vm_core::run_frames(self, initial, budget)
    }

    fn call_python(
        &mut self,
        target: usize,
        args: &[Value],
        kwargs: &[(String, Value)],
        budget: &Budget,
        span: Span,
    ) -> Result<Value, RuntimeError> {
        #[cfg(not(feature = "python-interop"))]
        {
            let _ = (target, args, kwargs, budget);
            Err(RuntimeError::new(
                "R_PYTHON_UNAVAILABLE",
                "this external call requires the optional `python-interop` feature",
                span,
            ))
        }
        #[cfg(feature = "python-interop")]
        {
            let external = self
                .artifact
                .externals
                .get(target)
                .cloned()
                .ok_or_else(|| {
                    RuntimeError::new(
                        "R_ARTIFACT_INVALID",
                        "Python call references a missing declaration",
                        span,
                    )
                })?;
            let interpreter = self.policy.python_interpreter.clone().ok_or_else(|| {
            RuntimeError::new(
                "R_PYTHON_UNCONFIGURED",
                "this project declares a Python call; configure an explicit interpreter or install the managed bridge",
                span,
            )
        })?;
            if self.python_bridge.is_none() {
                let project_root = self
                    .policy
                    .file_root
                    .as_deref()
                    .unwrap_or_else(|| std::path::Path::new("."));
                let working_directory = self
                    .policy
                    .additional_file_roots
                    .last()
                    .map(std::path::PathBuf::as_path)
                    .unwrap_or(project_root);
                self.python_bridge = Some(
                    python_bridge::PythonBridge::launch_with_project_root(
                        &interpreter,
                        project_root,
                        working_directory,
                        self.policy.python_timeout,
                    )
                    .map_err(|error| RuntimeError::new(error.code, error.message, span))?,
                );
            }
            let effect = self.begin_effect(
                EffectKind::PythonCall,
                format!("{}:{}", external.module, external.callable),
            );
            let call = self.python_bridge.as_mut().unwrap().call_with_cancellation(
                &external.module,
                &external.callable,
                args,
                kwargs,
                &external.result,
                &budget.cancellation,
            );
            self.finish_effect(
                effect,
                match &call {
                    Ok(_) => EffectOutcome::Returned,
                    Err(error) if error.code == "R_CANCELLED" => EffectOutcome::Interrupted,
                    Err(_) => EffectOutcome::ReturnedError,
                },
            );
            if let Some(bridge) = &self.python_bridge {
                let observation = bridge.native_image_observation().to_owned();
                let images = bridge.native_images().to_vec();
                self.record_python_native_images(&observation, &images);
            }
            match call {
                Ok(result) => {
                    charge_allocation(
                        budget,
                        input_value_allocation(&result.value)
                            .saturating_add(result.stdout.len())
                            .saturating_add(result.stderr.len()),
                        span,
                    )?;
                    if self.output.len().saturating_add(result.stdout.len())
                        > self.policy.output_limit_bytes
                    {
                        return Err(RuntimeError::new(
                            "R_OUTPUT_LIMIT",
                            "Python output exceeds the configured limit",
                            span,
                        ));
                    }
                    self.output.push_str(&result.stdout);
                    if self.host_stderr.len().saturating_add(result.stderr.len())
                        > self.policy.output_limit_bytes
                    {
                        return Err(RuntimeError::new(
                            "R_OUTPUT_LIMIT",
                            "Python stderr exceeds the configured limit",
                            span,
                        ));
                    }
                    self.host_stderr.push_str(&result.stderr);
                    if let Some(error) =
                        value_limit_error_at_boundary(&result.value, &self.policy, span)
                    {
                        return Err(error);
                    }
                    Ok(result.value)
                }
                Err(error) => {
                    if self.output.len().saturating_add(error.stdout.len())
                        <= self.policy.output_limit_bytes
                    {
                        self.output.push_str(&error.stdout);
                    }
                    if self.host_stderr.len().saturating_add(error.stderr.len())
                        <= self.policy.output_limit_bytes
                    {
                        self.host_stderr.push_str(&error.stderr);
                    }
                    let detail = error
                        .python_type
                        .map(|kind| format!("Python {kind}: "))
                        .unwrap_or_default();
                    let trace = error
                        .traceback
                        .map(|trace| format!("\n{trace}"))
                        .unwrap_or_default();
                    Err(RuntimeError::new(
                        error.code,
                        format!("{detail}{}{trace}", error.message),
                        span,
                    ))
                }
            }
        }
    }

    fn parallel_map(
        &mut self,
        function: usize,
        values: Vec<Value>,
        limit: usize,
        depth: usize,
        budget: Arc<Budget>,
        span: Span,
    ) -> Result<Result<Vec<Value>, TaskErrorValue>, RuntimeError> {
        if values.len() > self.policy.max_collection_items {
            return Err(RuntimeError::new(
                "R_COLLECTION_LIMIT",
                "parallel input exceeds the configured item limit",
                span,
            ));
        }
        if values.is_empty() {
            return Ok(Ok(Vec::new()));
        }
        if function >= self.artifact.functions.len() {
            return Err(RuntimeError::new(
                "R_ARTIFACT_INVALID",
                "parallel_map references a missing function",
                span,
            ));
        }
        let expected = self.artifact.functions[function]
            .parameters
            .first()
            .ok_or_else(|| {
                RuntimeError::new(
                    "R_ARTIFACT_INVALID",
                    "parallel worker must accept one argument",
                    span,
                )
            })?;
        if self.artifact.functions[function].parameters.len() != 1
            || values
                .iter()
                .any(|value| !value_matches(value, expected, &self.artifact.records))
        {
            return Err(RuntimeError::new(
                "R_CALL_TYPE",
                "parallel_map input does not match worker parameter type",
                span,
            ));
        }
        let worker_count = limit.min(values.len());
        let worker_input_copies = values
            .iter()
            .map(cloned_value_allocation)
            .fold(0usize, usize::saturating_add);
        charge_allocation(budget.as_ref(), worker_input_copies, span)?;
        charge_allocation(
            budget.as_ref(),
            size_of::<Vec<Value>>()
                .saturating_add(values.len().saturating_mul(size_of::<Value>()))
                .saturating_add(
                    values
                        .len()
                        .saturating_mul(size_of::<Option<(Result<Value, RuntimeError>, String)>>()),
                ),
            span,
        )?;
        let next = Arc::new(AtomicUsize::new(worker_count));
        let cancelled = Arc::new(AtomicBool::new(false));
        let artifact = Arc::clone(&self.artifact);
        let policy = self.policy.clone();
        let native_enabled = self.native_enabled;
        let native_code = self.native_code.clone();
        let values = Arc::new(values);
        let native_calls = self.native_calls.clone();
        let native_deoptimizations = self.native_deoptimizations.clone();
        let native_compilations = self.native_compilations.clone();
        // Reserve most of the remaining global instruction allowance for workers,
        // while keeping enough for the caller to inspect and handle the Result.
        // The workers share one bounded child budget, so the total remains bounded.
        let worker_budget = Arc::new(Budget {
            remaining: AtomicU64::new(budget.reserve_parallel()),
            allocation_remaining: budget.allocation_remaining.clone(),
            allocation_floor: budget.parallel_allocation_floor(),
            cancellation: budget.cancellation.clone(),
            local_cancellation: Some(cancelled.clone()),
            runtime_profile: budget.runtime_profile.clone(),
        });
        let (sender, receiver) = mpsc::channel();
        let mut handles = Vec::new();
        for initial_index in 0..worker_count {
            let next = next.clone();
            let cancelled = cancelled.clone();
            let artifact = artifact.clone();
            let policy = policy.clone();
            let values = values.clone();
            let budget = worker_budget.clone();
            let native_calls = native_calls.clone();
            let native_deoptimizations = native_deoptimizations.clone();
            let native_compilations = native_compilations.clone();
            let native_code = native_code.clone();
            let http = self.http.clone();
            let sender = sender.clone();
            handles.push(thread::spawn(move || {
                let mut assigned_index = Some(initial_index);
                loop {
                    if cancelled.load(Ordering::Relaxed) {
                        break;
                    }
                    let index = assigned_index
                        .take()
                        .unwrap_or_else(|| next.fetch_add(1, Ordering::Relaxed));
                    if index >= values.len() {
                        break;
                    }
                    let thread_id = format!("{:?}", thread::current().id());
                    let mut database_roots = Vec::with_capacity(
                        1 + policy.additional_file_roots.len() + policy.file_root_aliases.len(),
                    );
                    if let Some(root) = &policy.file_root {
                        database_roots.push(root.clone());
                    }
                    database_roots.extend(policy.additional_file_roots.iter().cloned());
                    database_roots.extend(policy.file_root_aliases.values().cloned());
                    let mut worker = Instance {
                        artifact: Arc::clone(&artifact),
                        native: None,
                        native_code: native_code.clone(),
                        native_enabled,
                        runtime_profile_enabled: false,
                        runtime_profile_metrics: None,
                        native_calls: native_calls.clone(),
                        native_deoptimizations: native_deoptimizations.clone(),
                        native_compilations: native_compilations.clone(),
                        policy: policy.clone(),
                        http: http.clone(),
                        readers: stdlib_data::FileReaderRegistry::new(
                            policy.max_file_bytes,
                            policy.max_file_bytes,
                        ),
                        databases: stdlib_services::DatabaseRegistry::with_roots(
                            database_roots,
                            policy.max_file_bytes,
                            policy.max_collection_items,
                            2_000,
                        ),
                        toml_documents: stdlib_services::TomlRegistry::new(policy.max_text_bytes),
                        output: String::new(),
                        program_stderr: String::new(),
                        closed: false,
                        worker_threads: HashSet::new(),
                        cancelled: Some(cancelled.clone()),
                        #[cfg(feature = "python-interop")]
                        python_bridge: None,
                        host_stderr: String::new(),
                        effect_report: Vec::new(),
                        omitted_effects: 0,
                        python_native_image_observation: "not_used".into(),
                        python_native_images: Vec::new(),
                    };
                    let result = worker.invoke_inner(
                        function,
                        vec![values[index].clone()],
                        depth,
                        budget.clone(),
                    );
                    if result.is_err() {
                        cancelled.store(true, Ordering::Relaxed);
                    }
                    let _ = sender.send((index, result, worker.output, thread_id));
                }
            }));
        }
        drop(sender);
        let mut completed: Vec<Option<(Result<Value, RuntimeError>, String)>> =
            (0..values.len()).map(|_| None).collect();
        for (index, result, output, thread_id) in receiver {
            self.worker_threads.insert(thread_id);
            completed[index] = Some((result, output));
        }
        let mut worker_panicked = false;
        for handle in handles {
            worker_panicked |= handle.join().is_err();
        }
        if budget.cancellation.is_cancelled() {
            return Err(RuntimeError::new(
                "R_CANCELLED",
                "invocation was cancelled while parallel workers were running",
                span,
            ));
        }
        let mut results = Vec::with_capacity(values.len());
        let mut first_error = None;
        for (index, item) in completed.into_iter().enumerate() {
            let Some((result, output)) = item else {
                continue;
            };
            if self.output.len().saturating_add(output.len()) > self.policy.output_limit_bytes {
                return Err(RuntimeError::new(
                    "R_OUTPUT_LIMIT",
                    "parallel worker output exceeds the configured limit",
                    span,
                ));
            }
            self.output.push_str(&output);
            match result {
                Ok(value) => results.push(value),
                Err(error) => {
                    if error.code != "R_CANCELLED" && first_error.is_none() {
                        first_error = Some((index, error));
                    }
                }
            }
        }
        if let Some((index, error)) = first_error {
            let mut trace = error.trace;
            trace.push(span);
            return Ok(Err(TaskErrorValue {
                index: BigInt::from_usize(index),
                code: error.code,
                message: error.message,
                trace,
            }));
        }
        if worker_panicked {
            return Err(RuntimeError::new(
                "R_WORKER_PANIC",
                "a parallel worker terminated unexpectedly",
                span,
            ));
        }
        if results.len() != values.len() {
            return Err(RuntimeError::new(
                "R_WORKER_CANCELLED",
                "parallel work ended before every input completed",
                span,
            ));
        }
        Ok(Ok(results))
    }
}

unsafe extern "C" fn native_reserve_fuel(budget: *const Budget, requested: u64) -> u64 {
    if budget.is_null() {
        return 0;
    }
    // The pointer is created from a live Arc<Budget> for the full JIT call.
    unsafe { (*budget).reserve(requested) }
}

unsafe extern "C" fn native_release_fuel(budget: *const Budget, count: u64) {
    if !budget.is_null() {
        // The pointer is created from a live Arc<Budget> for the full JIT call.
        unsafe { (*budget).refund(count) };
    }
}

unsafe extern "C" fn native_poll_cancellation(budget: *const Budget) -> i32 {
    if budget.is_null() {
        return 1;
    }
    // The pointer is created from a live Arc<Budget> for the full JIT call.
    i32::from(unsafe { (*budget).is_cancelled() })
}

const NATIVE_LIST_FLOAT_KIND: i64 = 1;
const NATIVE_LIST_INT_KIND: i64 = 2;
const NATIVE_LIST_BOOL_KIND: i64 = 3;

unsafe extern "C" fn native_make_list(
    arena: *mut NativeListArena,
    values: *const i64,
    count: i64,
    item_kind: i64,
    typed_empty: i64,
) -> *mut std::ffi::c_void {
    if arena.is_null() || count < 0 || !(1..=3).contains(&item_kind) {
        return std::ptr::null_mut();
    }
    let arena = unsafe { &mut *arena };
    let Ok(count) = usize::try_from(count) else {
        return std::ptr::null_mut();
    };
    if count > arena.max_collection_items || (count > 0 && values.is_null()) {
        return std::ptr::null_mut();
    }
    let encoded = if count == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(values, count) }
    };
    if item_kind == NATIVE_LIST_FLOAT_KIND {
        if encoded
            .iter()
            .any(|value| Float64::new(f64::from_bits(*value as u64)).is_none())
        {
            return std::ptr::null_mut();
        }
        let bytes = if typed_empty != 0 {
            size_of::<Vec<Float64>>()
        } else {
            size_of::<Vec<Value>>().saturating_add(count.saturating_mul(size_of::<Value>()))
        };
        if !arena.charge(bytes) {
            return std::ptr::null_mut();
        }
        if count <= NATIVE_INLINE_LIST_ITEMS {
            let mut descriptor = Box::new(NativeFloatList {
                data: std::ptr::null_mut(),
                len: count,
                storage_id: usize::MAX,
                owned: true,
                inline_storage: true,
                inline: [0; NATIVE_INLINE_LIST_ITEMS],
            });
            for (slot, value) in descriptor.inline.iter_mut().zip(encoded) {
                *slot = *value as u64;
            }
            descriptor.data = descriptor.inline.as_mut_ptr();
            arena.float_lists.push(descriptor);
            return (&mut **arena.float_lists.last_mut().unwrap() as *mut NativeFloatList).cast();
        }
        let decoded = encoded
            .iter()
            .map(|value| Float64::new(f64::from_bits(*value as u64)).unwrap())
            .collect::<Vec<_>>();
        let storage_id = arena.float_storage.len();
        arena.float_storage.push(decoded);
        arena.float_lists.push(Box::new(NativeFloatList {
            data: arena
                .float_storage
                .last_mut()
                .unwrap()
                .as_mut_ptr()
                .cast::<u64>(),
            len: count,
            storage_id,
            owned: true,
            inline_storage: false,
            inline: [0; NATIVE_INLINE_LIST_ITEMS],
        }));
        (&mut **arena.float_lists.last_mut().unwrap() as *mut NativeFloatList).cast()
    } else if item_kind == NATIVE_LIST_INT_KIND || item_kind == NATIVE_LIST_BOOL_KIND {
        if item_kind == NATIVE_LIST_BOOL_KIND
            && encoded.iter().any(|value| *value != 0 && *value != 1)
        {
            return std::ptr::null_mut();
        }
        let bytes =
            size_of::<Vec<Value>>().saturating_add(count.saturating_mul(size_of::<Value>()));
        if !arena.charge(bytes) {
            return std::ptr::null_mut();
        }
        if count <= NATIVE_INLINE_LIST_ITEMS {
            let mut descriptor = Box::new(NativeIntList {
                data: std::ptr::null_mut(),
                len: count,
                storage_id: usize::MAX,
                owned: true,
                inline_storage: true,
                inline: [0; NATIVE_INLINE_LIST_ITEMS],
            });
            descriptor.inline[..count].copy_from_slice(encoded);
            descriptor.data = descriptor.inline.as_mut_ptr();
            arena.int_lists.push(descriptor);
            return (&mut **arena.int_lists.last_mut().unwrap() as *mut NativeIntList).cast();
        }
        let storage_id = arena.int_storage.len();
        arena.int_storage.push(encoded.to_vec());
        arena.int_lists.push(Box::new(NativeIntList {
            data: arena.int_storage.last_mut().unwrap().as_mut_ptr(),
            len: count,
            storage_id,
            owned: true,
            inline_storage: false,
            inline: [0; NATIVE_INLINE_LIST_ITEMS],
        }));
        (&mut **arena.int_lists.last_mut().unwrap() as *mut NativeIntList).cast()
    } else {
        std::ptr::null_mut()
    }
}

#[inline(always)]
unsafe fn native_list_set_impl<const ITEM_KIND: i64, const ALWAYS_COPY: bool>(
    arena: *mut NativeListArena,
    list: *mut std::ffi::c_void,
    index: i64,
    value: i64,
) -> *mut std::ffi::c_void {
    if arena.is_null() || list.is_null() || index < 0 {
        return std::ptr::null_mut();
    }
    let arena = unsafe { &mut *arena };
    let Ok(index) = usize::try_from(index) else {
        return std::ptr::null_mut();
    };
    if ITEM_KIND == NATIVE_LIST_FLOAT_KIND {
        let descriptor = unsafe { &mut *(list.cast::<NativeFloatList>()) };
        if index >= descriptor.len {
            return std::ptr::null_mut();
        }
        arena.record_list_set_operation();
        let Some(value) = Float64::new(f64::from_bits(value as u64)) else {
            return std::ptr::null_mut();
        };
        if !ALWAYS_COPY && descriptor.owned {
            if descriptor.inline_storage {
                descriptor.inline[index] = value.get().to_bits();
            } else {
                let Some(storage) = arena.float_storage.get_mut(descriptor.storage_id) else {
                    return std::ptr::null_mut();
                };
                storage[index] = value;
            }
            arena.record_list_set_in_place_update();
            return list;
        }
        let list_len = descriptor.len;
        let bytes =
            size_of::<Vec<Float64>>().saturating_add(list_len.saturating_mul(size_of::<Float64>()));
        if !arena.charge(bytes) {
            return std::ptr::null_mut();
        }
        if list_len <= NATIVE_INLINE_LIST_ITEMS {
            let source = unsafe { std::slice::from_raw_parts(descriptor.data, list_len) };
            let mut copied = Box::new(NativeFloatList {
                data: std::ptr::null_mut(),
                len: list_len,
                storage_id: usize::MAX,
                owned: true,
                inline_storage: true,
                inline: [0; NATIVE_INLINE_LIST_ITEMS],
            });
            copied.inline[..list_len].copy_from_slice(source);
            copied.inline[index] = value.get().to_bits();
            copied.data = copied.inline.as_mut_ptr();
            arena.record_list_set_copy(list_len, bytes);
            arena.float_lists.push(copied);
            return (&mut **arena.float_lists.last_mut().unwrap() as *mut NativeFloatList).cast();
        }
        let Some(storage) = arena.float_storage.get(descriptor.storage_id) else {
            return std::ptr::null_mut();
        };
        let mut copied = storage.clone();
        copied[index] = value;
        arena.record_list_set_copy(list_len, bytes);
        let storage_id = arena.float_storage.len();
        arena.float_storage.push(copied);
        arena.float_lists.push(Box::new(NativeFloatList {
            data: arena
                .float_storage
                .last_mut()
                .unwrap()
                .as_mut_ptr()
                .cast::<u64>(),
            len: list_len,
            storage_id,
            owned: true,
            inline_storage: false,
            inline: [0; NATIVE_INLINE_LIST_ITEMS],
        }));
        (&mut **arena.float_lists.last_mut().unwrap() as *mut NativeFloatList).cast()
    } else {
        let descriptor = unsafe { &mut *(list.cast::<NativeIntList>()) };
        if index >= descriptor.len {
            return std::ptr::null_mut();
        }
        arena.record_list_set_operation();
        if ITEM_KIND == NATIVE_LIST_BOOL_KIND && value != 0 && value != 1 {
            return std::ptr::null_mut();
        }
        if !ALWAYS_COPY && descriptor.owned {
            if descriptor.inline_storage {
                descriptor.inline[index] = value;
            } else {
                let Some(storage) = arena.int_storage.get_mut(descriptor.storage_id) else {
                    return std::ptr::null_mut();
                };
                storage[index] = value;
            }
            arena.record_list_set_in_place_update();
            return list;
        }
        let list_len = descriptor.len;
        let bytes =
            size_of::<Vec<Value>>().saturating_add(list_len.saturating_mul(size_of::<Value>()));
        if !arena.charge(bytes) {
            return std::ptr::null_mut();
        }
        if list_len <= NATIVE_INLINE_LIST_ITEMS {
            let source = unsafe { std::slice::from_raw_parts(descriptor.data, list_len) };
            arena.record_list_set_copy(list_len, bytes);
            return arena.allocate_inline_int(source, index, value);
        }
        let Some(storage) = arena.int_storage.get(descriptor.storage_id) else {
            return std::ptr::null_mut();
        };
        let mut copied = storage.clone();
        copied[index] = value;
        arena.record_list_set_copy(list_len, bytes);
        let storage_id = arena.int_storage.len();
        arena.int_storage.push(copied);
        arena.int_lists.push(Box::new(NativeIntList {
            data: arena.int_storage.last_mut().unwrap().as_mut_ptr(),
            len: list_len,
            storage_id,
            owned: true,
            inline_storage: false,
            inline: [0; NATIVE_INLINE_LIST_ITEMS],
        }));
        (&mut **arena.int_lists.last_mut().unwrap() as *mut NativeIntList).cast()
    }
}

#[cfg(test)]
unsafe extern "C" fn native_list_set(
    arena: *mut NativeListArena,
    list: *mut std::ffi::c_void,
    index: i64,
    value: i64,
    item_kind: i64,
    always_copy: i64,
) -> *mut std::ffi::c_void {
    match (item_kind, always_copy == 0) {
        (NATIVE_LIST_FLOAT_KIND, true) => unsafe {
            native_list_set_impl::<NATIVE_LIST_FLOAT_KIND, false>(arena, list, index, value)
        },
        (NATIVE_LIST_INT_KIND, true) => unsafe {
            native_list_set_impl::<NATIVE_LIST_INT_KIND, false>(arena, list, index, value)
        },
        (NATIVE_LIST_BOOL_KIND, true) => unsafe {
            native_list_set_impl::<NATIVE_LIST_BOOL_KIND, false>(arena, list, index, value)
        },
        (NATIVE_LIST_FLOAT_KIND, false) => unsafe {
            native_list_set_impl::<NATIVE_LIST_FLOAT_KIND, true>(arena, list, index, value)
        },
        (NATIVE_LIST_INT_KIND, false) => unsafe {
            native_list_set_impl::<NATIVE_LIST_INT_KIND, true>(arena, list, index, value)
        },
        (NATIVE_LIST_BOOL_KIND, false) => unsafe {
            native_list_set_impl::<NATIVE_LIST_BOOL_KIND, true>(arena, list, index, value)
        },
        _ => std::ptr::null_mut(),
    }
}

macro_rules! native_list_set_entry {
    ($name:ident, $item_kind:ident, $always_copy:literal) => {
        unsafe extern "C" fn $name(
            arena: *mut NativeListArena,
            list: *mut std::ffi::c_void,
            index: i64,
            value: i64,
        ) -> *mut std::ffi::c_void {
            unsafe { native_list_set_impl::<$item_kind, $always_copy>(arena, list, index, value) }
        }
    };
}

native_list_set_entry!(native_list_set_float_copy, NATIVE_LIST_FLOAT_KIND, true);
native_list_set_entry!(native_list_set_int_copy, NATIVE_LIST_INT_KIND, true);
native_list_set_entry!(native_list_set_bool_copy, NATIVE_LIST_BOOL_KIND, true);
native_list_set_entry!(native_list_set_float_local, NATIVE_LIST_FLOAT_KIND, false);
native_list_set_entry!(native_list_set_int_local, NATIVE_LIST_INT_KIND, false);
native_list_set_entry!(native_list_set_bool_local, NATIVE_LIST_BOOL_KIND, false);

unsafe extern "C" fn native_charge_record(arena: *mut NativeListArena, bytes: i64) -> i32 {
    if arena.is_null() || bytes < 0 {
        return 0;
    }
    let Ok(bytes) = usize::try_from(bytes) else {
        return 0;
    };
    let arena = unsafe { &mut *arena };
    let ok = arena.charge(bytes);
    if ok {
        arena.record_record_charge(bytes);
    }
    i32::from(ok)
}

unsafe extern "C" fn native_count_guest_branch(arena: *mut NativeListArena) {
    if !arena.is_null() {
        unsafe { &*arena }.record_guest_conditional_branch();
    }
}

unsafe extern "C" fn native_print(arena: *mut NativeListArena, value: i64, kind: i64) -> i32 {
    if arena.is_null() {
        return 0;
    }
    i32::from(unsafe { &mut *arena }.print(value, kind))
}

fn http_effect_target(method: &str, url: &str) -> String {
    let host = reqwest::Url::parse(url)
        .ok()
        .and_then(|url| {
            let host = url.host_str()?;
            Some(match url.port() {
                Some(port) => format!("{host}:{port}"),
                None => host.to_owned(),
            })
        })
        .unwrap_or_else(|| "invalid-url".into());
    format!("{method} {host}")
}

fn native_runtime_error(
    artifact: &Artifact,
    native_trace: &[(usize, usize)],
    code: impl Into<String>,
    message: impl Into<String>,
    fault_span: Span,
) -> RuntimeError {
    let mut error = RuntimeError::new(code, message, fault_span);
    for (function_index, instruction_index) in native_trace.iter().rev() {
        if let Some(span) = artifact
            .functions
            .get(*function_index)
            .and_then(|function| function.code.get(*instruction_index))
            .map(|instruction| instruction.span)
        {
            error.trace.push(span);
        }
    }
    error
}

fn input_value_allocation(value: &Value) -> usize {
    match value {
        Value::Record(name, fields) => size_of::<Value>()
            .saturating_add(size_of::<String>() + name.len())
            .saturating_add(size_of::<Vec<Value>>())
            .saturating_add(fields.len().saturating_mul(size_of::<Value>()))
            .saturating_add(
                fields
                    .iter()
                    .map(input_value_allocation)
                    .fold(0usize, usize::saturating_add),
            ),
        Value::List(items) => size_of::<Value>()
            .saturating_add(size_of::<Vec<Value>>())
            .saturating_add(items.len().saturating_mul(size_of::<Value>()))
            .saturating_add(
                items
                    .iter()
                    .map(input_value_allocation)
                    .fold(0usize, usize::saturating_add),
            ),
        Value::Float64List(items) => size_of::<Value>()
            .saturating_add(size_of::<Vec<Float64>>())
            .saturating_add(items.len().saturating_mul(size_of::<Float64>())),
        Value::Map(items) => size_of::<Value>().saturating_add(
            items
                .iter()
                .map(|(key, value)| {
                    size_of::<String>()
                        .saturating_add(key.len())
                        .saturating_add(size_of::<Value>() * 3)
                        .saturating_add(input_value_allocation(value))
                })
                .fold(0usize, usize::saturating_add),
        ),
        Value::Json(_) => 0,
        Value::OptionSome(value) | Value::ResultOk(value) | Value::ResultErr(value) => {
            size_of::<Value>().saturating_add(input_value_allocation(value))
        }
        Value::Int(value) => size_of::<Value>() + size_of::<BigInt>() + value.digit_count(),
        Value::Text(value) => size_of::<Value>() + size_of::<String>() + value.len(),
        Value::Bytes(value) => size_of::<Value>() + size_of::<Vec<u8>>() + value.len(),
        Value::TaskError(error) => {
            size_of::<Value>()
                + size_of::<TaskErrorValue>()
                + error.code.len()
                + error.message.len()
                + error.trace.len().saturating_mul(size_of::<Span>())
        }
        Value::JsonError(error) => {
            size_of::<Value>()
                + size_of::<JsonErrorValue>()
                + error.code.len()
                + error.message.len()
        }
        Value::FileError(error) => {
            size_of::<Value>()
                + size_of::<FileErrorValue>()
                + error.code.len()
                + error.message.len()
                + error.trace.len().saturating_mul(size_of::<Span>())
        }
        Value::HttpError(error) => {
            size_of::<Value>()
                + size_of::<HttpErrorValue>()
                + error.code.len()
                + error.message.len()
                + error.trace.len().saturating_mul(size_of::<Span>())
        }
        Value::NativeResource(resource) => size_of::<Value>() + resource.kind().len(),
        Value::Float64(_) | Value::Bool(_) | Value::OptionNone | Value::Unit => size_of::<Value>(),
    }
}

fn file_error_result(error: RuntimeError) -> Value {
    Value::ResultErr(Box::new(Value::FileError(FileErrorValue {
        code: error.code,
        message: error.message,
        trace: error.trace,
    })))
}

fn runtime_reader_handle(
    value: &Value,
    span: Span,
) -> Result<stdlib_data::FileReaderHandle, RuntimeError> {
    let Value::NativeResource(resource) = value else {
        return Err(RuntimeError::new(
            "R_TYPE_INVALID",
            "file reader operation requires FileReader",
            span,
        ));
    };
    let (kind, owner, id) = resource.parts();
    if kind != "FileReader" {
        return Err(RuntimeError::new(
            "R_TYPE_INVALID",
            "resource is not a FileReader",
            span,
        ));
    }
    Ok(stdlib_data::FileReaderHandle::from_parts(owner, id))
}

fn runtime_service_resource(
    value: &Value,
    expected_kind: &str,
    span: Span,
) -> Result<(u64, u64), RuntimeError> {
    let Value::NativeResource(resource) = value else {
        return Err(RuntimeError::new(
            "R_TYPE_INVALID",
            format!("operation requires {expected_kind}"),
            span,
        ));
    };
    let (kind, owner, id) = resource.parts();
    if kind != expected_kind {
        return Err(RuntimeError::new(
            "R_TYPE_INVALID",
            format!("operation requires {expected_kind}"),
            span,
        ));
    }
    Ok((owner, id))
}

fn runtime_text_path(value: &Value, span: Span) -> Result<Vec<String>, RuntimeError> {
    let Value::List(items) = value else {
        return Err(RuntimeError::new(
            "R_TYPE_INVALID",
            "TOML path must be List[Text]",
            span,
        ));
    };
    items
        .iter()
        .map(|item| match item {
            Value::Text(text) => Ok(text.to_string()),
            _ => Err(RuntimeError::new(
                "R_TYPE_INVALID",
                "TOML path must contain only Text",
                span,
            )),
        })
        .collect()
}

fn runtime_sql_values(
    value: &Value,
    span: Span,
) -> Result<Vec<stdlib_services::SqlValue>, RuntimeError> {
    let Value::List(items) = value else {
        return Err(RuntimeError::new(
            "R_TYPE_INVALID",
            "SQLite parameters must be List[SqlValue]",
            span,
        ));
    };
    items
        .iter()
        .map(|item| {
            let Value::Record(name, fields) = item else {
                return Err(RuntimeError::new(
                    "R_TYPE_INVALID",
                    "SQLite parameter must be SqlValue",
                    span,
                ));
            };
            if name != "SqlValue" || fields.len() != 5 {
                return Err(RuntimeError::new(
                    "R_TYPE_INVALID",
                    "invalid SqlValue record",
                    span,
                ));
            }
            let (
                Value::Text(kind),
                Value::Int(integer),
                Value::Float64(real),
                Value::Text(text),
                Value::Bytes(blob),
            ) = (&fields[0], &fields[1], &fields[2], &fields[3], &fields[4])
            else {
                return Err(RuntimeError::new(
                    "R_TYPE_INVALID",
                    "invalid SqlValue payload",
                    span,
                ));
            };
            let integer_i64 = integer.to_i64();
            let zero_integer = integer == &BigInt::zero();
            let zero_real = real.get() == 0.0;
            let empty_text = text.is_empty();
            let empty_blob = blob.is_empty();
            let decoded = match kind.as_ref() {
                "null" if zero_integer && zero_real && empty_text && empty_blob => {
                    stdlib_services::SqlValue::Null
                }
                "integer" if integer_i64.is_some() && zero_real && empty_text && empty_blob => {
                    stdlib_services::SqlValue::Integer(integer_i64.unwrap())
                }
                "real" if zero_integer && real.get().is_finite() && empty_text && empty_blob => {
                    stdlib_services::SqlValue::Real(real.get())
                }
                "text" if zero_integer && zero_real && empty_blob => {
                    stdlib_services::SqlValue::Text(text.to_string())
                }
                "blob" if zero_integer && zero_real && empty_text => {
                    stdlib_services::SqlValue::Blob(blob.as_ref().clone())
                }
                _ => return Err(RuntimeError::new(
                    "R_SQLITE_VALUE",
                    "SqlValue kind and payload fields are inconsistent or outside SQLite's range",
                    span,
                )),
            };
            Ok(decoded)
        })
        .collect()
}

fn service_error_value(error: stdlib_services::ServiceError) -> Value {
    Value::Record(
        "StdError".into(),
        Arc::new(vec![
            Value::Text(error.code.into()),
            Value::Text(error.message.into()),
            Value::Text(error.path.into()),
            Value::Int(BigInt::from_usize(error.line)),
            Value::Int(BigInt::from_usize(error.column)),
        ]),
    )
}

fn temporal_resolution_result_value(
    result: Result<stdlib_services::TemporalResolution, stdlib_services::ServiceError>,
) -> Value {
    match result {
        Ok(resolution) => Value::ResultOk(Box::new(temporal_resolution_value(resolution))),
        Err(error) => Value::ResultErr(Box::new(Value::Record(
            "TimeError".into(),
            Arc::new(vec![
                Value::Text(error.code.into()),
                Value::Text(error.message.into()),
            ]),
        ))),
    }
}

fn service_result_value(result: Result<Value, stdlib_services::ServiceError>) -> Value {
    match result {
        Ok(value) => Value::ResultOk(Box::new(value)),
        Err(error) => Value::ResultErr(Box::new(service_error_value(error))),
    }
}

fn service_sql_value(value: stdlib_services::SqlValue) -> Value {
    let (kind, integer, real, text, blob) = match value {
        stdlib_services::SqlValue::Null => ("null", 0, 0.0, String::new(), Vec::new()),
        stdlib_services::SqlValue::Integer(value) => {
            ("integer", value, 0.0, String::new(), Vec::new())
        }
        stdlib_services::SqlValue::Real(value) => ("real", 0, value, String::new(), Vec::new()),
        stdlib_services::SqlValue::Text(value) => ("text", 0, 0.0, value, Vec::new()),
        stdlib_services::SqlValue::Blob(value) => ("blob", 0, 0.0, String::new(), value),
    };
    Value::Record(
        "SqlValue".into(),
        Arc::new(vec![
            Value::Text(kind.into()),
            Value::Int(BigInt::from_i64(integer)),
            Value::Float64(Float64::new(real).expect("SQLite service returns finite reals")),
            Value::Text(text.into()),
            Value::Bytes(Arc::new(blob)),
        ]),
    )
}

fn service_calendar_event(event: stdlib_services::CalendarEvent) -> Value {
    let resolution = |resolution: Option<stdlib_services::TemporalResolution>| {
        resolution
            .map(|resolution| Value::OptionSome(Box::new(temporal_resolution_value(resolution))))
            .unwrap_or(Value::OptionNone)
    };
    let date = |date: Option<(i32, u8, u8)>| {
        date.map(|(year, month, day)| {
            Value::OptionSome(Box::new(Value::Record(
                "Date".into(),
                Arc::new(vec![
                    Value::Int(BigInt::from_i64(i64::from(year))),
                    Value::Int(BigInt::from_i64(i64::from(month))),
                    Value::Int(BigInt::from_i64(i64::from(day))),
                ]),
            )))
        })
        .unwrap_or(Value::OptionNone)
    };
    let recurrence = event
        .recurrence_rule
        .map(|rule| Value::OptionSome(Box::new(Value::Text(rule.into()))))
        .unwrap_or(Value::OptionNone);
    Value::Record(
        "CalendarEvent".into(),
        Arc::new(vec![
            Value::Text(event.uid.into()),
            Value::Text(event.summary.into()),
            resolution(event.start_resolution),
            resolution(event.end_resolution),
            date(event.start_date),
            date(event.end_date),
            Value::Text(event.status.into()),
            Value::Bool(event.all_day),
            recurrence,
        ]),
    )
}

fn temporal_resolution_value(resolution: stdlib_services::TemporalResolution) -> Value {
    let local = resolution.local;
    let instants = resolution
        .unix_microseconds
        .into_iter()
        .map(|microseconds| {
            Value::Record(
                "Instant".into(),
                Arc::new(vec![Value::Int(BigInt::from_i64(microseconds))]),
            )
        })
        .collect();
    Value::Record(
        "TemporalResolution".into(),
        Arc::new(vec![
            Value::Text(resolution.kind.into()),
            Value::Record(
                "LocalDateTime".into(),
                Arc::new(vec![
                    Value::Int(BigInt::from_i64(local.year)),
                    Value::Int(BigInt::from_i64(local.month)),
                    Value::Int(BigInt::from_i64(local.day)),
                    Value::Int(BigInt::from_i64(local.hour)),
                    Value::Int(BigInt::from_i64(local.minute)),
                    Value::Int(BigInt::from_i64(local.second)),
                    Value::Int(BigInt::from_i64(local.microsecond)),
                ]),
            ),
            Value::Text(resolution.timezone.into()),
            Value::from_list_items(instants),
        ]),
    )
}

fn reader_runtime_error(
    kind: stdlib_data::ReaderErrorKind,
    message: String,
    line: usize,
    column: usize,
    span: Span,
) -> RuntimeError {
    let code = match kind {
        stdlib_data::ReaderErrorKind::Closed => "R_RESOURCE_CLOSED",
        stdlib_data::ReaderErrorKind::Stale => "R_RESOURCE_STALE",
        stdlib_data::ReaderErrorKind::ForeignOwner => "R_RESOURCE_OWNER",
        stdlib_data::ReaderErrorKind::Mode => "R_RESOURCE_MODE",
        stdlib_data::ReaderErrorKind::InvalidUtf8 => "R_FILE_UTF8",
        stdlib_data::ReaderErrorKind::Limit => "R_FILE_LIMIT",
        stdlib_data::ReaderErrorKind::Io => "R_FILE_READ",
    };
    let mut error = RuntimeError::new(code, message, span);
    if line > 0 {
        error.trace.push(Span {
            line,
            column,
            ..span
        });
    }
    error
}

fn http_error(code: &str, message: impl Into<String>) -> HttpErrorValue {
    HttpErrorValue {
        code: code.into(),
        message: message.into(),
        trace: Vec::new(),
    }
}

#[derive(Debug)]
struct HostHttpClient {
    client: reqwest::Client,
    executor: tokio::runtime::Runtime,
}

impl HostHttpClient {
    fn new(max_parallelism: usize) -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| error.to_string())?;
        let executor = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(max_parallelism.clamp(1, 8))
            .enable_all()
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Self { client, executor })
    }
}

#[derive(Debug)]
struct HttpBytesResponse {
    status: u16,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

impl HttpBytesResponse {
    fn into_text_json(self, max_text_bytes: usize) -> Result<serde_json::Value, HttpErrorValue> {
        if self.body.len() > max_text_bytes {
            return Err(http_error(
                "R_HTTP_LIMIT",
                "HTTP response exceeds the configured text byte limit",
            ));
        }
        let body = String::from_utf8(self.body)
            .map_err(|_| http_error("R_HTTP_ENCODING", "HTTP response body is not valid UTF-8"))?;
        Ok(serde_json::json!({
            "status": self.status,
            "headers": self.headers,
            "body": body,
        }))
    }
}

fn validate_http_url(url: &reqwest::Url, policy: &HostPolicy) -> Result<String, HttpErrorValue> {
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(http_error(
            "R_HTTP_URL",
            "only HTTP and HTTPS URLs without embedded credentials are allowed",
        ));
    }
    let host = url
        .host_str()
        .map(|host| host.trim_end_matches('.').to_ascii_lowercase())
        .ok_or_else(|| http_error("R_HTTP_URL", "URL has no host"))?;
    if policy.allowed_http_hosts.is_empty() || !policy.allowed_http_hosts.contains(&host) {
        return Err(http_error(
            "R_NETWORK_DENIED",
            "network access to this host is not granted",
        ));
    }
    Ok(host)
}

fn request_headers(
    headers: &BTreeMap<String, String>,
    policy: &HostPolicy,
) -> Result<reqwest::header::HeaderMap, HttpErrorValue> {
    let mut result = reqwest::header::HeaderMap::new();
    let mut header_bytes = 0usize;
    for (name, value) in headers {
        header_bytes = header_bytes
            .saturating_add(name.len())
            .saturating_add(value.len());
        if header_bytes > policy.max_http_request_bytes {
            return Err(http_error(
                "R_HTTP_LIMIT",
                "HTTP request headers exceed the configured byte limit",
            ));
        }
        let name = reqwest::header::HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| http_error("R_HTTP_HEADER", "request header name is invalid"))?;
        if matches!(
            name.as_str(),
            "host" | "content-length" | "connection" | "transfer-encoding"
        ) {
            return Err(http_error(
                "R_HTTP_HEADER",
                "request header is managed by the HTTP client",
            ));
        }
        let value = reqwest::header::HeaderValue::from_str(value)
            .map_err(|_| http_error("R_HTTP_HEADER", "request header value is invalid"))?;
        result.insert(name, value);
    }
    Ok(result)
}

async fn execute_http_bytes(
    client: &reqwest::Client,
    policy: &HostPolicy,
    method: &str,
    input: &str,
    headers: &BTreeMap<String, String>,
    body: &[u8],
    deadline: Instant,
) -> Result<HttpBytesResponse, HttpErrorValue> {
    if body.len() > policy.max_http_request_bytes {
        return Err(http_error(
            "R_HTTP_LIMIT",
            "HTTP request body exceeds the configured byte limit",
        ));
    }
    let mut method = reqwest::Method::from_bytes(method.as_bytes())
        .map_err(|_| http_error("R_HTTP_METHOD", "HTTP method is invalid"))?;
    if !matches!(
        method.as_str(),
        "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD" | "OPTIONS"
    ) {
        return Err(http_error("R_HTTP_METHOD", "HTTP method is not supported"));
    }
    let mut url =
        reqwest::Url::parse(input).map_err(|_| http_error("R_HTTP_URL", "URL is invalid"))?;
    let initial_host = validate_http_url(&url, policy)?;
    let initial_origin = url.origin().ascii_serialization();
    let mut headers = request_headers(headers, policy)?;
    let mut body = body.to_vec();

    for redirect_count in 0..=policy.max_http_redirects {
        let host = validate_http_url(&url, policy)?;
        if host != initial_host || url.origin().ascii_serialization() != initial_origin {
            return Err(http_error(
                "R_HTTP_REDIRECT",
                "redirect changed origin; credentials were not forwarded",
            ));
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(http_error("R_HTTP_TIMEOUT", "HTTP request timed out"));
        }
        let response = client
            .request(method.clone(), url.clone())
            .headers(headers.clone())
            .timeout(remaining)
            .body(body.clone())
            .send()
            .await
            .map_err(|error| {
                if error.is_timeout() {
                    http_error("R_HTTP_TIMEOUT", "HTTP request timed out")
                } else {
                    http_error("R_HTTP_REQUEST", "HTTP request failed")
                }
            })?;

        let status = response.status();
        if matches!(status.as_u16(), 301 | 302 | 303 | 307 | 308) {
            if let Some(location) = response.headers().get(reqwest::header::LOCATION) {
                if redirect_count == policy.max_http_redirects {
                    return Err(http_error(
                        "R_HTTP_REDIRECT",
                        "HTTP redirect limit exceeded",
                    ));
                }
                let location = location.to_str().map_err(|_| {
                    http_error("R_HTTP_REDIRECT", "HTTP redirect location is invalid")
                })?;
                let next_url = url
                    .join(location)
                    .map_err(|_| http_error("R_HTTP_REDIRECT", "HTTP redirect URL is invalid"))?;
                let next_host = validate_http_url(&next_url, policy)
                    .map_err(|_| http_error("R_HTTP_REDIRECT", "redirect host is not granted"))?;
                if next_host != initial_host
                    || next_url.origin().ascii_serialization() != initial_origin
                {
                    return Err(http_error(
                        "R_HTTP_REDIRECT",
                        "redirect changed origin; credentials were not forwarded",
                    ));
                }
                if (status.as_u16() == 303 && method != reqwest::Method::HEAD)
                    || (matches!(status.as_u16(), 301 | 302) && method == reqwest::Method::POST)
                {
                    method = reqwest::Method::GET;
                    body.clear();
                    for name in [
                        "content-type",
                        "content-encoding",
                        "content-language",
                        "content-location",
                    ] {
                        headers.remove(name);
                    }
                }
                url = next_url;
                continue;
            }
        }

        if response
            .content_length()
            .is_some_and(|length| length > policy.max_http_response_bytes as u64)
        {
            return Err(http_error(
                "R_HTTP_LIMIT",
                "HTTP response exceeds the configured byte limit",
            ));
        }
        let response_headers = response
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                value
                    .to_str()
                    .ok()
                    .map(|value| (name.as_str().to_ascii_lowercase(), value.to_owned()))
            })
            .fold(
                BTreeMap::<String, String>::new(),
                |mut result, (name, value)| {
                    result
                        .entry(name)
                        .and_modify(|existing| {
                            existing.push_str(", ");
                            existing.push_str(&value);
                        })
                        .or_insert(value);
                    result
                },
            );
        let mut response = response;
        let reserve = response
            .content_length()
            .unwrap_or(0)
            .min(policy.max_http_response_bytes as u64)
            .min(64 * 1024) as usize;
        let mut bytes = Vec::with_capacity(reserve);
        while let Some(chunk) = response.chunk().await.map_err(|error| {
            if error.is_timeout() {
                http_error("R_HTTP_TIMEOUT", "HTTP request timed out")
            } else {
                http_error("R_HTTP_REQUEST", "could not read HTTP response")
            }
        })? {
            if bytes.len().saturating_add(chunk.len()) > policy.max_http_response_bytes {
                return Err(http_error(
                    "R_HTTP_LIMIT",
                    "HTTP response exceeds the configured byte limit",
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        return Ok(HttpBytesResponse {
            status: status.as_u16(),
            headers: response_headers,
            body: bytes,
        });
    }
    Err(http_error(
        "R_HTTP_REDIRECT",
        "HTTP redirect limit exceeded",
    ))
}

struct HttpRequestBytes<'a> {
    method: &'a str,
    input: &'a str,
    headers: &'a BTreeMap<String, String>,
    body: &'a [u8],
    timeout: Duration,
}

fn perform_http_request_bytes(
    host: &HostHttpClient,
    policy: &HostPolicy,
    request: HttpRequestBytes<'_>,
    budget: &Budget,
) -> Result<HttpBytesResponse, HttpErrorValue> {
    let HttpRequestBytes {
        method,
        input,
        headers,
        body,
        timeout: request_timeout,
    } = request;
    let deadline = Instant::now().checked_add(request_timeout).ok_or_else(|| {
        http_error(
            "R_HTTP_TIMEOUT",
            "HTTP timeout is outside the supported range",
        )
    })?;
    host.executor.block_on(async {
        let request = execute_http_bytes(
            &host.client,
            policy,
            method,
            input,
            headers,
            body,
            deadline,
        );
        tokio::pin!(request);
        let cancellation = async {
            loop {
                if budget.is_cancelled() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        };
        tokio::pin!(cancellation);
        tokio::select! {
            biased;
            _ = &mut cancellation => Err(http_error("R_HTTP_CANCELLED", "HTTP request was cancelled")),
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => {
                Err(http_error("R_HTTP_TIMEOUT", "HTTP request timed out"))
            },
            result = &mut request => result,
        }
    })
}

fn perform_http_get(
    host: &HostHttpClient,
    policy: &HostPolicy,
    input: &str,
    budget: &Budget,
) -> Result<String, HttpErrorValue> {
    let response = perform_http_request_bytes(
        host,
        policy,
        HttpRequestBytes {
            method: "GET",
            input,
            headers: &BTreeMap::new(),
            body: &[],
            timeout: policy.http_timeout,
        },
        budget,
    )?;
    if !(200..300).contains(&response.status) {
        return Err(http_error(
            "R_HTTP_STATUS",
            format!("HTTP server returned status {}", response.status),
        ));
    }
    if response.body.len() > policy.max_text_bytes {
        return Err(http_error(
            "R_HTTP_LIMIT",
            "HTTP response exceeds the configured text byte limit",
        ));
    }
    String::from_utf8(response.body)
        .map_err(|_| http_error("R_HTTP_ENCODING", "HTTP response body is not valid UTF-8"))
}

#[derive(Debug)]
enum JsonDecodeError {
    Parse(serde_json::Error),
    DuplicateKey {
        key: String,
        line: usize,
        column: usize,
    },
}

#[cfg(test)]
fn parse_unique_json(source: &str) -> Result<serde_json::Value, JsonDecodeError> {
    use serde::de::DeserializeSeed;

    let (safe_source, reserved_key) = escape_reserved_json_keys(source);
    let mut duplicate = None;
    let mut deserializer = serde_json::Deserializer::from_str(&safe_source);
    let parsed = UniqueJsonSeed {
        duplicate: &mut duplicate,
    }
    .deserialize(&mut deserializer);
    match parsed {
        Ok(mut value) => {
            deserializer.end().map_err(JsonDecodeError::Parse)?;
            if let Some(reserved_key) = reserved_key.as_deref() {
                restore_reserved_json_keys(&mut value, reserved_key);
            }
            Ok(value)
        }
        Err(error) => {
            if let Some(key) = duplicate.take() {
                let offset = duplicate_json_key(source)
                    .map(|(_, offset)| offset)
                    .unwrap_or(source.len());
                let (line, column) = source_position(source, offset);
                let key = if reserved_key.as_deref() == Some(key.as_str()) {
                    "$serde_json::private::Number".to_owned()
                } else {
                    key
                };
                Err(JsonDecodeError::DuplicateKey { key, line, column })
            } else {
                Err(JsonDecodeError::Parse(error))
            }
        }
    }
}

fn parse_unique_shared_json(source: &str) -> Result<SharedJson, JsonDecodeError> {
    use serde::de::DeserializeSeed;

    let (safe_source, reserved_key) = escape_reserved_json_keys(source);
    let mut duplicate = None;
    let mut deserializer = serde_json::Deserializer::from_str(&safe_source);
    let parsed = SharedJsonSeed {
        duplicate: &mut duplicate,
    }
    .deserialize(&mut deserializer);
    match parsed {
        Ok(mut value) => {
            deserializer.end().map_err(JsonDecodeError::Parse)?;
            if let Some(reserved_key) = reserved_key.as_deref() {
                restore_reserved_shared_json_keys(&mut value, reserved_key);
            }
            Ok(value)
        }
        Err(error) => {
            if let Some(key) = duplicate.take() {
                let offset = duplicate_json_key(source)
                    .map(|(_, offset)| offset)
                    .unwrap_or(source.len());
                let (line, column) = source_position(source, offset);
                let key = if reserved_key.as_deref() == Some(key.as_str()) {
                    "$serde_json::private::Number".to_owned()
                } else {
                    key
                };
                Err(JsonDecodeError::DuplicateKey { key, line, column })
            } else {
                Err(JsonDecodeError::Parse(error))
            }
        }
    }
}

fn restore_reserved_shared_json_keys(value: &mut SharedJson, reserved_key: &str) {
    match value {
        SharedJson::Array(values) => {
            for value in Arc::make_mut(values) {
                restore_reserved_shared_json_keys(Arc::make_mut(value), reserved_key);
            }
        }
        SharedJson::Object(values) => {
            let values = Arc::make_mut(values);
            if let Some(mut value) = values.remove(reserved_key) {
                restore_reserved_shared_json_keys(Arc::make_mut(&mut value), reserved_key);
                values.insert("$serde_json::private::Number".to_owned(), value);
            }
            for value in values.values_mut() {
                restore_reserved_shared_json_keys(Arc::make_mut(value), reserved_key);
            }
        }
        _ => {}
    }
}

pub(crate) fn escape_reserved_json_keys<'a>(source: &'a str) -> (Cow<'a, str>, Option<String>) {
    const RESERVED: &str = "$serde_json::private::Number";
    let bytes = source.as_bytes();
    let mut offset = 0;
    let mut has_reserved_key = false;
    while offset < bytes.len() {
        if bytes[offset] != b'"' {
            offset += 1;
            continue;
        }
        let start = offset;
        offset += 1;
        let mut escaped = false;
        while offset < bytes.len() {
            let byte = bytes[offset];
            offset += 1;
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                break;
            }
        }
        if offset > bytes.len() || bytes.get(offset - 1) != Some(&b'"') {
            break;
        }
        let mut after = offset;
        while bytes
            .get(after)
            .is_some_and(|byte| matches!(byte, b' ' | b'\n' | b'\r' | b'\t'))
        {
            after += 1;
        }
        let token = &source[start + 1..offset - 1];
        let is_reserved = if token.contains('\\') {
            serde_json::from_str::<String>(&source[start..offset])
                .is_ok_and(|decoded| decoded == RESERVED)
        } else {
            token == RESERVED
        };
        if bytes.get(after) == Some(&b':') && is_reserved {
            has_reserved_key = true;
            break;
        }
    }
    if !has_reserved_key {
        return (Cow::Borrowed(source), None);
    }

    let mut strings = Vec::<(usize, usize, String, bool)>::new();
    offset = 0;
    while offset < bytes.len() {
        if bytes[offset] != b'"' {
            offset += 1;
            continue;
        }
        let start = offset;
        offset += 1;
        let mut escaped = false;
        while offset < bytes.len() {
            let byte = bytes[offset];
            offset += 1;
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                break;
            }
        }
        if offset > bytes.len() || bytes.get(offset - 1) != Some(&b'"') {
            break;
        }
        let Ok(decoded) = serde_json::from_str::<String>(&source[start..offset]) else {
            continue;
        };
        let mut after = offset;
        while bytes
            .get(after)
            .is_some_and(|byte| matches!(byte, b' ' | b'\n' | b'\r' | b'\t'))
        {
            after += 1;
        }
        let is_key = bytes.get(after) == Some(&b':');
        strings.push((start, offset, decoded, is_key));
    }
    if !strings
        .iter()
        .any(|(_, _, value, is_key)| *is_key && value == RESERVED)
    {
        return (Cow::Borrowed(source), None);
    }
    let existing = strings
        .iter()
        .map(|(_, _, value, _)| value.as_str())
        .collect::<HashSet<_>>();
    let reserved_key = (0_u64..)
        .map(|number| {
            format!(
                "$guion_private_{number:0width$}$",
                width = RESERVED.len() - 16
            )
        })
        .find(|candidate| !existing.contains(candidate.as_str()))
        .expect("a unique private key can be generated");
    let mut rewritten = String::with_capacity(source.len());
    let mut copied_until = 0;
    for (start, end, decoded, is_key) in strings {
        if !is_key || decoded != RESERVED {
            continue;
        }
        let raw_content = &source[start + 1..end - 1];
        let escape_count = raw_content.len().saturating_sub(RESERVED.len()) / 5;
        let mut replacement = String::with_capacity(raw_content.len());
        for (index, character) in reserved_key.chars().enumerate() {
            if index < escape_count {
                replacement.push_str(&format!("\\u{:04x}", character as u32));
            } else {
                replacement.push(character);
            }
        }
        debug_assert_eq!(replacement.len(), raw_content.len());
        rewritten.push_str(&source[copied_until..start + 1]);
        rewritten.push_str(&replacement);
        copied_until = end - 1;
    }
    rewritten.push_str(&source[copied_until..]);
    (Cow::Owned(rewritten), Some(reserved_key))
}

pub(crate) fn restore_reserved_json_keys(value: &mut serde_json::Value, reserved_key: &str) {
    match value {
        serde_json::Value::Array(values) => {
            for value in values {
                restore_reserved_json_keys(value, reserved_key);
            }
        }
        serde_json::Value::Object(object) => {
            let Some(mut value) = object.remove(reserved_key) else {
                for value in object.values_mut() {
                    restore_reserved_json_keys(value, reserved_key);
                }
                return;
            };
            restore_reserved_json_keys(&mut value, reserved_key);
            object.insert("$serde_json::private::Number".to_owned(), value);
            for value in object.values_mut() {
                restore_reserved_json_keys(value, reserved_key);
            }
        }
        _ => {}
    }
}

struct UniqueJsonSeed<'duplicate> {
    duplicate: &'duplicate mut Option<String>,
}

struct SharedJsonSeed<'duplicate> {
    duplicate: &'duplicate mut Option<String>,
}

impl<'de> serde::de::DeserializeSeed<'de> for SharedJsonSeed<'_> {
    type Value = SharedJson;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(SharedJsonVisitor {
            duplicate: self.duplicate,
        })
    }
}

struct SharedJsonVisitor<'duplicate> {
    duplicate: &'duplicate mut Option<String>,
}

impl<'de> serde::de::Visitor<'de> for SharedJsonVisitor<'_> {
    type Value = SharedJson;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(SharedJson::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(SharedJson::Number(value.into()))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(SharedJson::Number(value.into()))
    }

    fn visit_i128<E>(self, value: i128) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        serde_json::Number::from_i128(value)
            .map(SharedJson::Number)
            .ok_or_else(|| E::custom("JSON integer is out of range"))
    }

    fn visit_u128<E>(self, value: u128) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        serde_json::Number::from_u128(value)
            .map(SharedJson::Number)
            .ok_or_else(|| E::custom("JSON integer is out of range"))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        serde_json::Number::from_f64(value)
            .map(SharedJson::Number)
            .ok_or_else(|| E::custom("JSON number is out of range"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(SharedJson::String(Arc::from(value)))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(SharedJson::String(Arc::from(value)))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(SharedJson::Null)
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(SharedJson::Null)
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::DeserializeSeed;
        SharedJsonSeed {
            duplicate: &mut *self.duplicate,
        }
        .deserialize(deserializer)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element_seed(SharedJsonSeed {
            duplicate: &mut *self.duplicate,
        })? {
            values.push(Arc::new(value));
        }
        Ok(SharedJson::Array(Arc::new(values)))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        let mut object = BTreeMap::new();
        while let Some(key) = map.next_key::<String>()? {
            if object.contains_key(&key) {
                *self.duplicate = Some(key);
                return Err(serde::de::Error::custom("duplicate JSON object key"));
            }
            let value = map.next_value_seed(SharedJsonSeed {
                duplicate: &mut *self.duplicate,
            })?;
            object.insert(key, Arc::new(value));
        }
        if object.len() == 1 {
            if let Some(SharedJson::String(number)) =
                object.get("$serde_json::private::Number").map(Arc::as_ref)
            {
                if let Ok(number) = number.parse::<serde_json::Number>() {
                    return Ok(SharedJson::Number(number));
                }
            }
        }
        Ok(SharedJson::Object(Arc::new(object)))
    }
}

impl<'de> serde::de::DeserializeSeed<'de> for UniqueJsonSeed<'_> {
    type Value = serde_json::Value;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(UniqueJsonVisitor {
            duplicate: self.duplicate,
        })
    }
}

struct UniqueJsonVisitor<'duplicate> {
    duplicate: &'duplicate mut Option<String>,
}

impl<'de> serde::de::Visitor<'de> for UniqueJsonVisitor<'_> {
    type Value = serde_json::Value;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(serde_json::Value::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(serde_json::Value::Number(value.into()))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(serde_json::Value::Number(value.into()))
    }

    fn visit_i128<E>(self, value: i128) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        serde_json::Number::from_i128(value)
            .map(serde_json::Value::Number)
            .ok_or_else(|| E::custom("JSON integer is out of range"))
    }

    fn visit_u128<E>(self, value: u128) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        serde_json::Number::from_u128(value)
            .map(serde_json::Value::Number)
            .ok_or_else(|| E::custom("JSON integer is out of range"))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        serde_json::Number::from_f64(value)
            .map(serde_json::Value::Number)
            .ok_or_else(|| E::custom("JSON number is out of range"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(serde_json::Value::String(value.to_owned()))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(serde_json::Value::String(value))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(serde_json::Value::Null)
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(serde_json::Value::Null)
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::DeserializeSeed;
        UniqueJsonSeed {
            duplicate: self.duplicate,
        }
        .deserialize(deserializer)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element_seed(UniqueJsonSeed {
            duplicate: &mut *self.duplicate,
        })? {
            values.push(value);
        }
        Ok(serde_json::Value::Array(values))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        let Some(first_key) = map.next_key::<String>()? else {
            return Ok(serde_json::Value::Object(serde_json::Map::new()));
        };

        // serde_json uses this reserved key when arbitrary-precision numbers
        // cannot be represented by its machine-number visitor methods.
        let mut keys = HashSet::new();
        let mut object = serde_json::Map::new();
        let first_value = map.next_value_seed(UniqueJsonSeed {
            duplicate: &mut *self.duplicate,
        })?;
        let next_key = map.next_key::<String>()?;
        if first_key == "$serde_json::private::Number" && next_key.is_none() {
            if let serde_json::Value::String(number) = &first_value {
                if let Ok(number) = number.parse::<serde_json::Number>() {
                    return Ok(serde_json::Value::Number(number));
                }
            }
        }
        keys.insert(first_key.clone());
        object.insert(first_key, first_value);
        let mut key = next_key;
        while let Some(current_key) = key {
            if !keys.insert(current_key.clone()) {
                *self.duplicate = Some(current_key);
                return Err(serde::de::Error::custom("duplicate JSON object key"));
            }
            let value = map.next_value_seed(UniqueJsonSeed {
                duplicate: &mut *self.duplicate,
            })?;
            object.insert(current_key, value);
            key = map.next_key::<String>()?;
        }
        Ok(serde_json::Value::Object(object))
    }
}

fn duplicate_json_key(source: &str) -> Option<(String, usize)> {
    #[derive(Default)]
    struct ObjectKeys {
        expecting_key: bool,
        keys: HashSet<String>,
    }
    let bytes = source.as_bytes();
    let mut stack: Vec<Option<ObjectKeys>> = Vec::new();
    let mut offset = 0;
    while offset < bytes.len() {
        match bytes[offset] {
            b' ' | b'\n' | b'\r' | b'\t' | b':' => offset += 1,
            b'{' => {
                stack.push(Some(ObjectKeys {
                    expecting_key: true,
                    ..ObjectKeys::default()
                }));
                offset += 1;
            }
            b'[' => {
                stack.push(None);
                offset += 1;
            }
            b'}' | b']' => {
                stack.pop();
                offset += 1;
            }
            b',' => {
                if let Some(Some(object)) = stack.last_mut() {
                    object.expecting_key = true;
                }
                offset += 1;
            }
            b'"' => {
                let start = offset;
                offset += 1;
                while offset < bytes.len() {
                    match bytes[offset] {
                        b'\\' => offset = (offset + 2).min(bytes.len()),
                        b'"' => {
                            offset += 1;
                            break;
                        }
                        _ => offset += 1,
                    }
                }
                if let Some(Some(object)) = stack.last_mut() {
                    if object.expecting_key {
                        let key = serde_json::from_str::<String>(&source[start..offset]).ok()?;
                        if !object.keys.insert(key.clone()) {
                            return Some((key, start));
                        }
                        object.expecting_key = false;
                    }
                }
            }
            _ => offset += 1,
        }
    }
    None
}

#[cfg(test)]
mod unique_json_parser_tests {
    use super::{
        escape_reserved_json_keys, parse_unique_json, parse_unique_shared_json, JsonDecodeError,
    };

    #[test]
    fn does_not_copy_input_when_no_reserved_key_needs_rewriting() {
        let source = r#"{"rows":[{"value":42,"label":"東京"}]}"#;
        let (safe_source, reserved_key) = escape_reserved_json_keys(source);

        assert!(reserved_key.is_none());
        assert_eq!(safe_source, source);
        assert!(std::ptr::eq(source.as_ptr(), safe_source.as_ptr()));
    }

    #[test]
    fn rejects_escaped_duplicate_keys_with_the_duplicate_location() {
        let source = "{\n \"rows\": [{\"name\": \"first\", \"\\u006eame\": \"second\"}]\n}";
        match parse_unique_json(source) {
            Err(JsonDecodeError::DuplicateKey { key, line, column }) => {
                assert_eq!(key, "name");
                assert_eq!((line, column), (2, 29));
            }
            other => panic!("expected duplicate-key error, got {other:?}"),
        }
    }

    #[test]
    fn retains_arbitrary_precision_numbers_and_nested_objects() {
        let source = r#"{"rows":[{"value":123456789012345678901234567890,"label":"東京"}]}"#;
        let value = parse_unique_json(source).expect("valid JSON should parse");
        assert_eq!(
            value["rows"][0]["value"].to_string(),
            "123456789012345678901234567890"
        );
        assert_eq!(value["rows"][0]["label"], "東京");
    }

    #[test]
    fn accepts_user_objects_with_serde_private_number_key() {
        let source = r#"{"rows":[{"value":{"$serde_json::private::Number":{"nested":1}}}]}"#;
        let value = parse_unique_json(source).expect("reserved-looking object keys are valid JSON");
        assert_eq!(
            value["rows"][0]["value"]["$serde_json::private::Number"]["nested"],
            1
        );

        let numeric_text = r#"{"value":{"\u0024serde_json::private::Number":"123"}}"#;
        let value = parse_unique_json(numeric_text).expect("numeric strings remain strings");
        assert_eq!(value["value"]["$serde_json::private::Number"], "123");
    }

    #[test]
    fn reports_malformed_json_as_a_parser_error() {
        assert!(matches!(
            parse_unique_json("{\"value\": [}"),
            Err(JsonDecodeError::Parse(_))
        ));
    }

    #[test]
    fn shared_parser_matches_generic_parser_for_supported_json_values() {
        for source in [
            "null",
            "true",
            "-17",
            "1.25e3",
            r#"{"rows":[{"value":42,"label":"東京"}]}"#,
            r#"{"rows":[{"value":123456789012345678901234567890,"label":"東京"}]}"#,
            r#"{"value":{"$serde_json::private::Number":{"nested":1}}}"#,
            r#"{"value":{"\u0024serde_json::private::Number":"123"}}"#,
        ] {
            let generic = parse_unique_json(source).expect("generic JSON parser accepts fixture");
            let shared =
                parse_unique_shared_json(source).expect("shared JSON parser accepts fixture");

            assert_eq!(shared.to_serde(), generic, "source: {source}");
        }
    }

    #[test]
    fn shared_parser_rejects_escaped_duplicate_keys_at_the_same_location() {
        let source = "{\n \"rows\": [{\"name\": \"first\", \"\\u006eame\": \"second\"}]\n}";

        match parse_unique_shared_json(source) {
            Err(JsonDecodeError::DuplicateKey { key, line, column }) => {
                assert_eq!(key, "name");
                assert_eq!((line, column), (2, 29));
            }
            other => panic!("expected duplicate-key error, got {other:?}"),
        }
    }

    #[test]
    fn shared_parser_reports_malformed_json_as_a_parser_error() {
        assert!(matches!(
            parse_unique_shared_json("{\"value\": [}"),
            Err(JsonDecodeError::Parse(_))
        ));
    }
}

#[cfg(test)]
mod function_cache_tests {
    use super::share_artifact;
    use guion_model::{Artifact, Function, Type};
    use std::sync::Arc;

    #[test]
    fn frame_and_worker_owners_share_immutable_artifact_storage() {
        let function = Function {
            name: "work".into(),
            parameters: vec![],
            result: Type::Int,
            local_count: 0,
            code: vec![],
        };
        let artifact = Artifact {
            format_version: Artifact::CURRENT_VERSION,
            build_metadata: None,
            compiled_source_sha256: None,
            source_name: "cache-test".into(),
            sources: vec![],
            entry: "work".into(),
            functions: vec![function],
            externals: vec![],
            records: vec![],
        };
        let shared = share_artifact(artifact);

        let first_frame = Arc::clone(&shared);
        let second_worker = Arc::clone(&shared);

        assert!(Arc::ptr_eq(&first_frame, &second_worker));
        assert_eq!(Arc::strong_count(&shared), 3);
    }
}

fn source_position(source: &str, offset: usize) -> (usize, usize) {
    let prefix = &source[..offset.min(source.len())];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = prefix.rsplit('\n').next().unwrap_or("").len() + 1;
    (line, column)
}

fn atomic_replace_text(path: &Path, content: &str) -> std::io::Result<()> {
    static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(1);
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_else(|| "state".into());
    let permissions = fs::metadata(path)
        .ok()
        .map(|metadata| metadata.permissions());

    for _ in 0..32 {
        let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(".{name}.guion-tmp-{}-{id}", std::process::id()));
        let mut file = match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };

        let replacement = (|| {
            file.write_all(content.as_bytes())?;
            if let Some(permissions) = &permissions {
                fs::set_permissions(&temporary, permissions.clone())?;
            }
            file.sync_all()?;
            drop(file);
            fs::rename(&temporary, path)
        })();
        if replacement.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        return replacement;
    }

    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "could not reserve a unique temporary file name",
    ))
}

fn file_io_error_code(error: &std::io::Error, fallback: &'static str) -> &'static str {
    match error.kind() {
        std::io::ErrorKind::NotFound => "R_FILE_NOT_FOUND",
        std::io::ErrorKind::PermissionDenied => "R_FILE_PERMISSION",
        _ => fallback,
    }
}

fn host_file_path(
    policy: &HostPolicy,
    requested: &str,
    write: bool,
    span: Span,
) -> Result<PathBuf, RuntimeError> {
    host_file_path_with_walk_root(policy, requested, write, span, false)
}

fn host_file_path_no_follow(
    policy: &HostPolicy,
    requested: &str,
    span: Span,
) -> Result<PathBuf, RuntimeError> {
    host_file_path_with_walk_root(policy, requested, false, span, true)
}

fn host_file_path_with_walk_root(
    policy: &HostPolicy,
    requested: &str,
    write: bool,
    span: Span,
    preserve_walk_root: bool,
) -> Result<PathBuf, RuntimeError> {
    let allowed = if write {
        policy.allow_file_write
    } else {
        policy.allow_file_read
    };
    if !allowed {
        return Err(RuntimeError::new(
            "R_HOST_DENIED",
            if write {
                "file writing is not granted by the host policy"
            } else {
                "file reading is not granted by the host policy"
            },
            span,
        ));
    }
    let alias = policy
        .file_root_aliases
        .iter()
        .filter_map(|(alias, root)| {
            requested
                .strip_prefix(alias)
                .and_then(|suffix| suffix.strip_prefix('/'))
                .map(|relative| (alias, root, relative))
        })
        .max_by_key(|(alias, _, _)| alias.len());
    let (requested_path, roots) = if let Some((_, root, relative)) = alias {
        (Path::new(relative), vec![root])
    } else {
        let mut roots = Vec::with_capacity(1 + policy.additional_file_roots.len());
        if let Some(root) = &policy.file_root {
            roots.push(root);
        }
        roots.extend(policy.additional_file_roots.iter());
        (Path::new(requested), roots)
    };
    if roots.is_empty() {
        return Err(RuntimeError::new(
            "R_HOST_DENIED",
            "file access has no configured project root",
            span,
        ));
    }
    let roots = roots
        .into_iter()
        .map(|root| {
            root.canonicalize().map_err(|error| {
                RuntimeError::new(
                    "R_HOST_DENIED",
                    format!("cannot resolve file capability root: {error}"),
                    span,
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if requested_path.is_absolute() {
        return Err(RuntimeError::new(
            "R_HOST_DENIED",
            "file paths must be relative to the granted project root",
            span,
        ));
    }
    if requested_path.components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir
                | std::path::Component::RootDir
                | std::path::Component::Prefix(_)
        )
    }) {
        return Err(RuntimeError::new(
            "R_HOST_DENIED",
            "file path escapes or is absolute relative to the granted project root",
            span,
        ));
    }

    if !write {
        let mut missing_error = None;
        for root in &roots {
            if preserve_walk_root {
                let candidate = root.join(requested_path);
                let components = requested_path
                    .components()
                    .filter(|component| matches!(component, std::path::Component::Normal(_)))
                    .collect::<Vec<_>>();
                let mut current = root.clone();
                let mut failure = None;
                let mut candidate_missing = false;
                for (index, component) in components.iter().enumerate() {
                    current.push(component);
                    match fs::symlink_metadata(&current) {
                        Ok(metadata)
                            if metadata.file_type().is_symlink()
                                && index + 1 < components.len() =>
                        {
                            failure = Some(RuntimeError::new(
                                "R_HOST_DENIED",
                                "fs_walk root cannot pass through a symlink directory",
                                span,
                            ));
                            break;
                        }
                        Ok(_) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                            missing_error = Some(error);
                            candidate_missing = true;
                            break;
                        }
                        Err(error) => {
                            failure = Some(RuntimeError::new(
                                file_io_error_code(&error, "R_FILE_READ"),
                                format!("cannot inspect `{requested}`: {error}"),
                                span,
                            ));
                            break;
                        }
                    }
                }
                if let Some(error) = failure {
                    return Err(error);
                }
                if candidate_missing {
                    continue;
                }
                return Ok(candidate);
            }
            match root.join(requested_path).canonicalize() {
                Ok(resolved) if resolved.starts_with(root) => return Ok(resolved),
                Ok(_) => {
                    return Err(RuntimeError::new(
                        "R_HOST_DENIED",
                        "file path escapes the granted project root",
                        span,
                    ));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    missing_error = Some(error);
                }
                Err(error) => {
                    return Err(RuntimeError::new(
                        file_io_error_code(&error, "R_FILE_READ"),
                        format!("cannot resolve `{requested:?}`: {error}"),
                        span,
                    ));
                }
            }
        }
        let error = missing_error.unwrap_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no granted root contains this path",
            )
        });
        return Err(RuntimeError::new(
            file_io_error_code(&error, "R_FILE_READ"),
            format!("cannot resolve `{requested:?}`: {error}"),
            span,
        ));
    }

    let root = roots.last().expect("at least one file root was checked");
    let candidate = root.join(requested_path);
    let resolved = if !candidate.exists() {
        let name = candidate.file_name().ok_or_else(|| {
            RuntimeError::new("R_HOST_DENIED", "file path must name a file", span)
        })?;
        let parent = candidate
            .parent()
            .unwrap_or(root)
            .canonicalize()
            .map_err(|error| {
                RuntimeError::new(
                    "R_FILE_WRITE",
                    format!("cannot resolve parent directory: {error}"),
                    span,
                )
            })?;
        if !parent.starts_with(root) {
            return Err(RuntimeError::new(
                "R_HOST_DENIED",
                "file path escapes the granted project root",
                span,
            ));
        }
        parent.join(name)
    } else {
        candidate.canonicalize().map_err(|error| {
            RuntimeError::new(
                file_io_error_code(&error, "R_FILE_WRITE"),
                format!("cannot resolve `{requested:?}`: {error}"),
                span,
            )
        })?
    };
    if !resolved.starts_with(root) {
        return Err(RuntimeError::new(
            "R_HOST_DENIED",
            "file path escapes the granted project root",
            span,
        ));
    }
    Ok(resolved)
}

#[cfg(test)]
mod native_small_list_tests {
    use super::{
        native_list_set, native_make_list, Budget, CancellationToken, NativeFloatList,
        NativeIntList, NativeListArena, RuntimeProfileCounters, NATIVE_LIST_FLOAT_KIND,
        NATIVE_LIST_INT_KIND,
    };
    use std::{slice, sync::Arc};

    fn arena() -> (Arc<Budget>, NativeListArena) {
        let budget = Arc::new(Budget::new(
            1_000_000,
            1_000_000,
            CancellationToken::new(),
            None,
        ));
        let arena = NativeListArena::new(Arc::as_ptr(&budget), 10_000, 10_000, 0, None);
        (budget, arena)
    }

    #[test]
    fn small_integer_list_copy_uses_inline_data_and_preserves_the_original() {
        let (_budget, mut arena) = arena();
        let values = [1_i64, 2, 3, 4, 5, 6, 7, 8];
        let original = unsafe {
            native_make_list(
                &mut arena,
                values.as_ptr(),
                values.len() as i64,
                NATIVE_LIST_INT_KIND,
                0,
            )
        };
        assert!(!original.is_null());
        assert!(
            arena.int_storage.is_empty(),
            "small list should use inline data"
        );
        let updated =
            unsafe { native_list_set(&mut arena, original, 0, 9, NATIVE_LIST_INT_KIND, 1) };
        assert!(!updated.is_null());
        let original = unsafe { &*original.cast::<NativeIntList>() };
        let updated = unsafe { &*updated.cast::<NativeIntList>() };
        assert_eq!(
            unsafe { slice::from_raw_parts(original.data, original.len) },
            &values
        );
        assert_eq!(
            unsafe { slice::from_raw_parts(updated.data, updated.len) },
            &[9, 2, 3, 4, 5, 6, 7, 8]
        );
        assert!(
            arena.int_storage.is_empty(),
            "small copies should stay inline"
        );
    }

    #[test]
    fn small_float_list_copy_uses_inline_data_and_preserves_the_original() {
        let (_budget, mut arena) = arena();
        let values = [1.5_f64.to_bits() as i64, 2.5_f64.to_bits() as i64];
        let original = unsafe {
            native_make_list(
                &mut arena,
                values.as_ptr(),
                values.len() as i64,
                NATIVE_LIST_FLOAT_KIND,
                0,
            )
        };
        assert!(!original.is_null());
        assert!(
            arena.float_storage.is_empty(),
            "small list should use inline data"
        );
        let replacement = 9.5_f64.to_bits() as i64;
        let updated = unsafe {
            native_list_set(
                &mut arena,
                original,
                1,
                replacement,
                NATIVE_LIST_FLOAT_KIND,
                1,
            )
        };
        assert!(!updated.is_null());
        let original = unsafe { &*original.cast::<NativeFloatList>() };
        let updated = unsafe { &*updated.cast::<NativeFloatList>() };
        assert_eq!(
            unsafe { slice::from_raw_parts(original.data, original.len) },
            &[values[0] as u64, values[1] as u64]
        );
        assert_eq!(
            unsafe { slice::from_raw_parts(updated.data, updated.len) },
            &[values[0] as u64, replacement as u64]
        );
        assert!(
            arena.float_storage.is_empty(),
            "small copies should stay inline"
        );
    }

    #[test]
    fn large_native_lists_keep_external_storage() {
        let (_budget, mut arena) = arena();
        let values = [1_i64, 2, 3, 4, 5, 6, 7, 8, 9];
        let list = unsafe {
            native_make_list(
                &mut arena,
                values.as_ptr(),
                values.len() as i64,
                NATIVE_LIST_INT_KIND,
                0,
            )
        };
        assert!(!list.is_null());
        assert_eq!(arena.int_storage.len(), 1);
    }

    #[test]
    fn invalid_native_list_set_attempts_do_not_increment_the_operation_counter() {
        let budget = Arc::new(Budget::new(
            1_000_000,
            1_000_000,
            CancellationToken::new(),
            None,
        ));
        let profile = Arc::new(RuntimeProfileCounters::default());
        let mut arena = NativeListArena::new(
            Arc::as_ptr(&budget),
            10_000,
            10_000,
            0,
            Some(profile.clone()),
        );
        let values = [1_i64, 2];
        let list = unsafe {
            native_make_list(
                &mut arena,
                values.as_ptr(),
                values.len() as i64,
                NATIVE_LIST_INT_KIND,
                0,
            )
        };

        let invalid = unsafe { native_list_set(&mut arena, list, 2, 9, NATIVE_LIST_INT_KIND, 1) };

        assert!(invalid.is_null());
        assert_eq!(profile.to_json()["list_set"]["operations"], 0);
    }
}

#[cfg(test)]
mod runtime_profile_histogram_tests {
    use super::RuntimeProfileCounters;

    #[test]
    fn copy_size_histogram_tracks_bucket_boundaries_and_sums_to_total() {
        let profile = RuntimeProfileCounters::default();
        for (elements, bytes) in [(0, 1), (8, 2), (9, 3), (32, 4), (33, 5), (128, 6), (129, 7)] {
            profile.record_list_set_copy(elements, bytes);
        }

        let list_set = profile.to_json()["list_set"].clone();
        let histogram = &list_set["copy_size_histogram"]["buckets"];
        assert_eq!(histogram["0-8"]["copies"], 2);
        assert_eq!(histogram["0-8"]["estimated_copy_bytes"], 3);
        assert_eq!(histogram["9-32"]["copies"], 2);
        assert_eq!(histogram["9-32"]["estimated_copy_bytes"], 7);
        assert_eq!(histogram["33-128"]["copies"], 2);
        assert_eq!(histogram["33-128"]["estimated_copy_bytes"], 11);
        assert_eq!(histogram["129+"]["copies"], 1);
        assert_eq!(histogram["129+"]["estimated_copy_bytes"], 7);

        let total: u64 = ["0-8", "9-32", "33-128", "129+"]
            .into_iter()
            .map(|bucket| histogram[bucket]["copies"].as_u64().unwrap())
            .sum();
        assert_eq!(total, list_set["copy_on_write_copies"].as_u64().unwrap());
    }
}
