use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    fmt::Write as _,
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
use veritas_model::{
    Artifact, BigInt, FileErrorValue, Float64, HttpErrorValue, JsonErrorValue, NativeResourceValue,
    Op, RuntimeError, SharedJson, Span, TaskErrorValue, Type, Value,
};
mod json_projection;
mod native_backend;
#[cfg(feature = "python-interop")]
pub mod python_bridge;
pub mod stdlib_data;
pub mod stdlib_services;
use native_backend::{
    NativeBackend, NativeCode, NativeFloatList, NativeIntList, NativeRecord, NativeSymbols,
};

// JIT code keeps raw pointers to these descriptors; Box preserves each address when the Vec grows.
#[allow(clippy::vec_box)]
struct NativeListArena {
    float_lists: Vec<Box<NativeFloatList>>,
    float_storage: Vec<Vec<Float64>>,
    int_lists: Vec<Box<NativeIntList>>,
    int_storage: Vec<Vec<i64>>,
    budget: *const Budget,
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
    ) -> Self {
        Self {
            float_lists: Vec::new(),
            float_storage: Vec::new(),
            int_lists: Vec::new(),
            int_storage: Vec::new(),
            budget,
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

/// A cloneable, one-invocation cancellation signal for [`Instance::invoke_with_cancellation`].
#[derive(Clone, Debug, Default)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

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
        }
    }

    /// Create a runtime that always uses the verified bytecode VM.
    /// This is useful for reproducible backend comparisons and diagnosis.
    pub fn vm_only() -> Self {
        Self {
            native_enabled: false,
        }
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

struct VmFrame {
    function_index: usize,
    depth: usize,
    call_site: Option<Span>,
    locals: Vec<Value>,
    stack: Vec<Value>,
    ip: usize,
}

enum FrameStart {
    Returned(Value),
    Vm(VmFrame),
}

enum VmStep {
    Call {
        frame: VmFrame,
        target: usize,
        args: Vec<Value>,
        span: Span,
    },
    Return(Value),
}

fn append_guest_call_trace(
    error: &mut RuntimeError,
    call_site: Option<Span>,
    ancestors: &[VmFrame],
) {
    if let Some(span) = call_site {
        error.trace.push(span);
    }
    for frame in ancestors.iter().rev() {
        if let Some(span) = frame.call_site {
            error.trace.push(span);
        }
    }
}

impl Instance {
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
        let budget = Arc::new(Budget::new(
            self.policy.instruction_limit,
            self.policy.allocation_limit_bytes,
            cancellation,
        ));
        if self.native_enabled && self.native.is_none() {
            self.native = NativeBackend::compile(
                &self.artifact,
                NativeSymbols {
                    reserve_fuel: native_reserve_fuel as *const () as *const u8,
                    release_fuel: native_release_fuel as *const () as *const u8,
                    poll_cancellation: native_poll_cancellation as *const () as *const u8,
                    make_list: native_make_list as *const () as *const u8,
                    list_set: native_list_set as *const () as *const u8,
                    charge_record: native_charge_record as *const () as *const u8,
                    print: native_print as *const () as *const u8,
                },
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
        if depth >= self.policy.call_depth_limit {
            return Err(RuntimeError::new(
                "R_CALL_DEPTH",
                format!("call depth exceeds {}", self.policy.call_depth_limit),
                f.code.first().map(|i| i.span).unwrap_or_default(),
            ));
        }
        let mut native_arena = NativeListArena::new(
            Arc::as_ptr(&budget),
            self.policy.max_collection_items,
            self.policy.output_limit_bytes,
            self.output.len().saturating_add(self.program_stderr.len()),
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
                    .unwrap_or(&f);
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
                                            let storage_id = unsafe { (*descriptor).storage_id };
                                            let Some(storage) =
                                                native_arena.float_storage.get_mut(storage_id)
                                            else {
                                                return Err(RuntimeError::new(
                                                    "R_NATIVE_RESULT",
                                                    "native record returned an invalid list storage handle",
                                                    f.code.last().map(|instruction| instruction.span).unwrap_or_default(),
                                                ));
                                            };
                                            let values = decoded_float_lists
                                                .entry(storage_id)
                                                .or_insert_with(|| {
                                                    Arc::new(std::mem::take(storage))
                                                })
                                                .clone();
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
                                            let storage_id = unsafe { (*descriptor).storage_id };
                                            let Some(storage) =
                                                native_arena.int_storage.get_mut(storage_id)
                                            else {
                                                return Err(RuntimeError::new(
                                                    "R_NATIVE_RESULT",
                                                    "native record returned an invalid integer-list storage handle",
                                                    f.code.last().map(|instruction| instruction.span).unwrap_or_default(),
                                                ));
                                            };
                                            let values = decoded_int_lists
                                                .entry(storage_id)
                                                .or_insert_with(|| {
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
                                                })
                                                .clone();
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
                                            let storage_id = unsafe { (*descriptor).storage_id };
                                            let Some(storage) =
                                                native_arena.int_storage.get_mut(storage_id)
                                            else {
                                                return Err(RuntimeError::new(
                                                    "R_NATIVE_RESULT",
                                                    "native record returned an invalid boolean-list storage handle",
                                                    f.code.last().map(|instruction| instruction.span).unwrap_or_default(),
                                                ));
                                            };
                                            let values = decoded_bool_lists
                                                .entry(storage_id)
                                                .or_insert_with(|| {
                                                    Arc::new(
                                                        std::mem::take(storage)
                                                            .into_iter()
                                                            .map(|value| Value::Bool(value != 0))
                                                            .collect(),
                                                    )
                                                })
                                                .clone();
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
        let mut locals = vec![Value::Unit; f.local_count];
        for (i, arg) in args.into_iter().enumerate() {
            locals[i] = arg;
        }
        Ok(FrameStart::Vm(VmFrame {
            function_index: index,
            depth,
            call_site,
            locals,
            stack: Vec::new(),
            ip: 0,
        }))
    }

    fn run_guest_frames(
        &mut self,
        initial: VmFrame,
        budget: Arc<Budget>,
    ) -> Result<Value, RuntimeError> {
        let mut frames = vec![initial];
        loop {
            let frame = frames.pop().expect("guest frame stack is non-empty");
            let current_call_site = frame.call_site;
            match self.execute_vm_frame(frame, budget.clone()) {
                Err(mut error) => {
                    append_guest_call_trace(&mut error, current_call_site, &frames);
                    return Err(error);
                }
                Ok(VmStep::Return(value)) => {
                    if let Some(mut parent) = frames.pop() {
                        parent.stack.push(value);
                        frames.push(parent);
                    } else {
                        return Ok(value);
                    }
                }
                Ok(VmStep::Call {
                    mut frame,
                    target,
                    args,
                    span,
                }) => match self.start_guest_frame(
                    target,
                    args,
                    frame.depth.saturating_add(1),
                    budget.clone(),
                    Some(span),
                ) {
                    Ok(FrameStart::Returned(value)) => {
                        frame.stack.push(value);
                        frames.push(frame);
                    }
                    Ok(FrameStart::Vm(child)) => {
                        frames.push(frame);
                        frames.push(child);
                    }
                    Err(mut error) => {
                        error.trace.push(span);
                        append_guest_call_trace(&mut error, frame.call_site, &frames);
                        return Err(error);
                    }
                },
            }
        }
    }

    fn execute_vm_frame(
        &mut self,
        frame: VmFrame,
        budget: Arc<Budget>,
    ) -> Result<VmStep, RuntimeError> {
        let VmFrame {
            function_index,
            depth,
            call_site,
            mut locals,
            mut stack,
            mut ip,
        } = frame;
        let artifact = Arc::clone(&self.artifact);
        let f = &artifact.functions[function_index];
        loop {
            if budget.is_cancelled()
                || self
                    .cancelled
                    .as_ref()
                    .is_some_and(|flag| flag.load(Ordering::Acquire))
            {
                return Err(RuntimeError::new(
                    "R_CANCELLED",
                    "parallel task was cancelled after a sibling failed",
                    f.code.first().map(|i| i.span).unwrap_or_default(),
                ));
            }
            if ip >= f.code.len() {
                return Err(RuntimeError::new(
                    "R_NO_RETURN",
                    format!("function `{}` reached the end without returning", f.name),
                    f.code.last().map(|i| i.span).unwrap_or_default(),
                ));
            }
            if !budget.consume() {
                return Err(RuntimeError::new(
                    "R_STEP_LIMIT",
                    "instruction budget exhausted",
                    f.code[ip].span,
                ));
            }
            let instruction = f.code[ip].clone();
            ip += 1;
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
                Op::Const(v) => {
                    if let Some(error) =
                        value_limit_error_at_boundary(&v, &self.policy, instruction.span)
                    {
                        return Err(error);
                    }
                    charge_allocation(
                        budget.as_ref(),
                        cloned_value_allocation(&v),
                        instruction.span,
                    )?;
                    push_stack!(v)
                }
                Op::Load(slot) => {
                    charge_allocation(
                        budget.as_ref(),
                        cloned_value_allocation(&locals[slot]),
                        instruction.span,
                    )?;
                    push_stack!(locals[slot].clone());
                }
                Op::Store(slot) => locals[slot] = pop!(),
                Op::Pop => {
                    pop!();
                }
                Op::Add => {
                    let b = pop!();
                    let a = pop!();
                    let v = match (a, b) {
                        (Value::Int(x), Value::Int(y)) => Value::Int(&x + &y),
                        (Value::Float64(x), Value::Float64(y)) => {
                            let value = Float64::new(x.get() + y.get()).ok_or_else(|| {
                                RuntimeError::new(
                                    "R_FLOAT_NONFINITE",
                                    "Float64 operation produced a non-finite value",
                                    instruction.span,
                                )
                            })?;
                            Value::Float64(value)
                        }
                        (Value::Text(x), Value::Text(y)) => {
                            if x.len().saturating_add(y.len()) > self.policy.max_text_bytes {
                                return Err(RuntimeError::new(
                                    "R_TEXT_LIMIT",
                                    "text value exceeds the configured byte limit",
                                    instruction.span,
                                ));
                            }
                            Value::Text(x + &y)
                        }
                        _ => {
                            return Err(RuntimeError::new(
                                "R_TYPE_INVALID",
                                "invalid operands for addition",
                                instruction.span,
                            ))
                        }
                    };
                    if let Some(error) = value_limit_error(&v, &self.policy, instruction.span) {
                        return Err(error);
                    }
                    charge_allocation(budget.as_ref(), new_value_allocation(&v), instruction.span)?;
                    push_stack!(v);
                }
                Op::Subtract => {
                    let b = pop!();
                    let a = pop!();
                    let v = match (a, b) {
                        (Value::Int(x), Value::Int(y)) => Value::Int(&x - &y),
                        (Value::Float64(x), Value::Float64(y)) => {
                            Value::Float64(Float64::new(x.get() - y.get()).ok_or_else(|| {
                                RuntimeError::new(
                                    "R_FLOAT_NONFINITE",
                                    "Float64 operation produced a non-finite value",
                                    instruction.span,
                                )
                            })?)
                        }
                        _ => {
                            return Err(RuntimeError::new(
                                "R_TYPE_INVALID",
                                "invalid operands for subtraction",
                                instruction.span,
                            ))
                        }
                    };
                    if let Some(error) = value_limit_error(&v, &self.policy, instruction.span) {
                        return Err(error);
                    }
                    charge_allocation(budget.as_ref(), new_value_allocation(&v), instruction.span)?;
                    push_stack!(v);
                }
                Op::Multiply => {
                    let b = pop!();
                    let a = pop!();
                    let v = match (a, b) {
                        (Value::Int(x), Value::Int(y)) => Value::Int(&x * &y),
                        (Value::Float64(x), Value::Float64(y)) => {
                            Value::Float64(Float64::new(x.get() * y.get()).ok_or_else(|| {
                                RuntimeError::new(
                                    "R_FLOAT_NONFINITE",
                                    "Float64 operation produced a non-finite value",
                                    instruction.span,
                                )
                            })?)
                        }
                        _ => {
                            return Err(RuntimeError::new(
                                "R_TYPE_INVALID",
                                "invalid operands for multiplication",
                                instruction.span,
                            ))
                        }
                    };
                    if let Some(error) = value_limit_error(&v, &self.policy, instruction.span) {
                        return Err(error);
                    }
                    charge_allocation(budget.as_ref(), new_value_allocation(&v), instruction.span)?;
                    push_stack!(v);
                }
                Op::Divide => {
                    let right = pop!();
                    let left = pop!();
                    let (Value::Float64(left), Value::Float64(right)) = (left, right) else {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "Float64 division requires Float64 operands",
                            instruction.span,
                        ));
                    };
                    if right.get() == 0.0 {
                        return Err(RuntimeError::new(
                            "R_DIV_ZERO",
                            "division by zero",
                            instruction.span,
                        ));
                    }
                    let value = Float64::new(left.get() / right.get()).ok_or_else(|| {
                        RuntimeError::new(
                            "R_FLOAT_NONFINITE",
                            "Float64 operation produced a non-finite value",
                            instruction.span,
                        )
                    })?;
                    push_stack!(Value::Float64(value));
                }
                Op::FloatSqrt => {
                    let Value::Float64(value) = pop!() else {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "sqrt requires a Float64 value",
                            instruction.span,
                        ));
                    };
                    if value.get() < 0.0 {
                        return Err(RuntimeError::new(
                            "R_MATH_DOMAIN",
                            "sqrt is undefined for negative values",
                            instruction.span,
                        ));
                    }
                    let result = Float64::new(value.get().sqrt()).ok_or_else(|| {
                        RuntimeError::new(
                            "R_FLOAT_NONFINITE",
                            "sqrt produced a non-finite Float64 value",
                            instruction.span,
                        )
                    })?;
                    push_stack!(Value::Float64(result));
                }
                Op::IntegerDivide | Op::FloorDivide | Op::Remainder => {
                    let right = pop!();
                    let left = pop!();
                    let (Value::Int(left), Value::Int(right)) = (left, right) else {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "integer division requires Int operands",
                            instruction.span,
                        ));
                    };
                    if right.is_zero() {
                        return Err(RuntimeError::new(
                            "R_DIV_ZERO",
                            "division by zero",
                            instruction.span,
                        ));
                    }
                    let (quotient, remainder) = if matches!(instruction.op, Op::IntegerDivide) {
                        left.trunc_div_rem(&right).expect("nonzero divisor checked")
                    } else {
                        left.floor_div_rem(&right).expect("nonzero divisor checked")
                    };
                    let value = if matches!(instruction.op, Op::Remainder) {
                        remainder
                    } else {
                        quotient
                    };
                    let value = Value::Int(value);
                    if let Some(error) = value_limit_error(&value, &self.policy, instruction.span) {
                        return Err(error);
                    }
                    charge_allocation(
                        budget.as_ref(),
                        new_value_allocation(&value),
                        instruction.span,
                    )?;
                    push_stack!(value);
                }
                Op::Negate => {
                    let value = match pop!() {
                        Value::Int(x) => Value::Int(&BigInt::zero() - &x),
                        Value::Float64(x) => {
                            Value::Float64(Float64::new(-x.get()).ok_or_else(|| {
                                RuntimeError::new(
                                    "R_FLOAT_NONFINITE",
                                    "Float64 operation produced a non-finite value",
                                    instruction.span,
                                )
                            })?)
                        }
                        _ => {
                            return Err(RuntimeError::new(
                                "R_TYPE_INVALID",
                                "invalid operand for negation",
                                instruction.span,
                            ))
                        }
                    };
                    charge_allocation(
                        budget.as_ref(),
                        new_value_allocation(&value),
                        instruction.span,
                    )?;
                    push_stack!(value);
                }
                Op::Not => {
                    if let Value::Bool(x) = pop!() {
                        push_stack!(Value::Bool(!x));
                    } else {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "invalid operand for not",
                            instruction.span,
                        ));
                    }
                }
                Op::MakeList(count) => {
                    if count > self.policy.max_collection_items {
                        return Err(RuntimeError::new(
                            "R_COLLECTION_LIMIT",
                            "list exceeds the configured item limit",
                            instruction.span,
                        ));
                    }
                    charge_allocation(
                        budget.as_ref(),
                        size_of::<Vec<Value>>()
                            .saturating_add(count.saturating_mul(size_of::<Value>())),
                        instruction.span,
                    )?;
                    let mut values = Vec::with_capacity(count);
                    for _ in 0..count {
                        values.push(pop!());
                    }
                    values.reverse();
                    push_stack!(Value::from_list_items(values));
                }
                Op::MakeTypedList(Type::Float64) => {
                    charge_allocation(
                        budget.as_ref(),
                        size_of::<Vec<Float64>>(),
                        instruction.span,
                    )?;
                    push_stack!(Value::Float64List(Arc::new(Vec::new())));
                }
                Op::MakeTypedList(_) => {
                    charge_allocation(budget.as_ref(), size_of::<Vec<Value>>(), instruction.span)?;
                    push_stack!(Value::List(Arc::new(Vec::new())));
                }
                Op::MakeMap(count) => {
                    if count > self.policy.max_collection_items {
                        return Err(RuntimeError::new(
                            "R_COLLECTION_LIMIT",
                            "map exceeds the configured item limit",
                            instruction.span,
                        ));
                    }
                    charge_allocation(
                        budget.as_ref(),
                        size_of::<BTreeMap<String, Value>>()
                            .saturating_add(count.saturating_mul(size_of::<(String, Value)>())),
                        instruction.span,
                    )?;
                    let mut values = BTreeMap::new();
                    let mut entries = Vec::with_capacity(count);
                    for _ in 0..count {
                        let value = pop!();
                        let key = match pop!() {
                            Value::Text(key) => key,
                            _ => {
                                return Err(RuntimeError::new(
                                    "R_TYPE_INVALID",
                                    "map key must be Text",
                                    instruction.span,
                                ))
                            }
                        };
                        entries.push((key, value));
                    }
                    for (key, value) in entries.into_iter().rev() {
                        charge_allocation(
                            budget.as_ref(),
                            key.len()
                                .saturating_add(size_of::<String>() + size_of::<Value>() * 2),
                            instruction.span,
                        )?;
                        if values.insert(key.clone(), value).is_some() {
                            return Err(RuntimeError::new(
                                "R_MAP_DUPLICATE_KEY",
                                format!("duplicate map key `{key}`"),
                                instruction.span,
                            ));
                        }
                    }
                    let map = Value::Map(Arc::new(values));
                    if let Some(error) = value_limit_error(&map, &self.policy, instruction.span) {
                        return Err(error);
                    }
                    push_stack!(map);
                }
                Op::MakeTypedMap(_) => {
                    charge_allocation(
                        budget.as_ref(),
                        size_of::<BTreeMap<String, Value>>(),
                        instruction.span,
                    )?;
                    push_stack!(Value::Map(Arc::new(BTreeMap::new())));
                }
                Op::MakeRecord(record_index, count) => {
                    let record = self.artifact.records.get(record_index).ok_or_else(|| {
                        RuntimeError::new(
                            "R_ARTIFACT_INVALID",
                            "record constructor references a missing definition",
                            instruction.span,
                        )
                    })?;
                    if count != record.fields.len() {
                        return Err(RuntimeError::new(
                            "R_ARTIFACT_INVALID",
                            "record constructor has an invalid field count",
                            instruction.span,
                        ));
                    }
                    charge_allocation(
                        budget.as_ref(),
                        size_of::<Vec<Value>>()
                            .saturating_add(count.saturating_mul(size_of::<Value>()))
                            .saturating_add(record.name.len()),
                        instruction.span,
                    )?;
                    let mut fields = Vec::with_capacity(count);
                    for _ in 0..count {
                        fields.push(pop!());
                    }
                    fields.reverse();
                    push_stack!(Value::Record(record.name.clone(), Arc::new(fields)));
                }
                Op::GetField(record_index, field_index) => {
                    let record = self.artifact.records.get(record_index).ok_or_else(|| {
                        RuntimeError::new(
                            "R_ARTIFACT_INVALID",
                            "field access references a missing record definition",
                            instruction.span,
                        )
                    })?;
                    let Some(expected_field) = record.fields.get(field_index) else {
                        return Err(RuntimeError::new(
                            "R_ARTIFACT_INVALID",
                            "field access references a missing record field",
                            instruction.span,
                        ));
                    };
                    let Value::Record(name, fields) = pop!() else {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "field access requires a record value",
                            instruction.span,
                        ));
                    };
                    if name != record.name {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "field access record type does not match",
                            instruction.span,
                        ));
                    }
                    let value = fields.get(field_index).cloned().ok_or_else(|| {
                        RuntimeError::new(
                            "R_TYPE_INVALID",
                            "record value has the wrong number of fields",
                            instruction.span,
                        )
                    })?;
                    if !value_matches(&value, &expected_field.ty, &self.artifact.records) {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "record field value has the wrong type",
                            instruction.span,
                        ));
                    }
                    charge_allocation(
                        budget.as_ref(),
                        cloned_value_allocation(&value),
                        instruction.span,
                    )?;
                    push_stack!(value);
                }
                Op::MapKeys => match pop!() {
                    Value::Map(values) => {
                        let allocation = values.keys().fold(
                            size_of::<Vec<Value>>()
                                .saturating_add(values.len().saturating_mul(size_of::<Value>())),
                            |total, key| total.saturating_add(size_of::<String>() + key.len()),
                        );
                        charge_allocation(budget.as_ref(), allocation, instruction.span)?;
                        let keys = values.keys().cloned().map(Value::Text).collect::<Vec<_>>();
                        push_stack!(Value::List(Arc::new(keys)));
                    }
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "map keys requires a Map",
                            instruction.span,
                        ))
                    }
                },
                Op::MapSet => {
                    let value = pop!();
                    let key = match pop!() {
                        Value::Text(key) => key,
                        _ => {
                            return Err(RuntimeError::new(
                                "R_TYPE_INVALID",
                                "map_set key must be Text",
                                instruction.span,
                            ))
                        }
                    };
                    let values = match pop!() {
                        Value::Map(values) => values,
                        _ => {
                            return Err(RuntimeError::new(
                                "R_TYPE_INVALID",
                                "map_set first argument must be a Map",
                                instruction.span,
                            ))
                        }
                    };
                    if !values.contains_key(&key)
                        && values.len() >= self.policy.max_collection_items
                    {
                        return Err(RuntimeError::new(
                            "R_COLLECTION_LIMIT",
                            "map exceeds the configured item limit",
                            instruction.span,
                        ));
                    }
                    if key.len() > self.policy.max_text_bytes {
                        return Err(RuntimeError::new(
                            "R_TEXT_LIMIT",
                            "map key exceeds the configured text limit",
                            instruction.span,
                        ));
                    }
                    let map_clone_bytes = map_clone_allocation(values.as_ref(), Some(&key), &value);
                    charge_allocation(budget.as_ref(), map_clone_bytes, instruction.span)?;
                    let mut updated = values.as_ref().clone();
                    updated.insert(key, value);
                    let map = Value::Map(Arc::new(updated));
                    if let Some(error) = value_limit_error(&map, &self.policy, instruction.span) {
                        return Err(error);
                    }
                    push_stack!(map);
                }
                Op::MapHas => {
                    let key = match pop!() {
                        Value::Text(key) => key,
                        _ => {
                            return Err(RuntimeError::new(
                                "R_TYPE_INVALID",
                                "map_has key must be Text",
                                instruction.span,
                            ))
                        }
                    };
                    let values = match pop!() {
                        Value::Map(values) => values,
                        _ => {
                            return Err(RuntimeError::new(
                                "R_TYPE_INVALID",
                                "map_has first argument must be a Map",
                                instruction.span,
                            ))
                        }
                    };
                    push_stack!(Value::Bool(values.contains_key(&key)));
                }
                Op::Length => {
                    let len = match pop!() {
                        Value::List(values) => values.len(),
                        Value::Float64List(values) => values.len(),
                        Value::Map(values) => values.len(),
                        Value::Bytes(values) => values.len(),
                        Value::Text(values) => values.chars().count(),
                        _ => {
                            return Err(RuntimeError::new(
                                "R_TYPE_INVALID",
                                "len requires a List, Map, Bytes, or Text",
                                instruction.span,
                            ))
                        }
                    };
                    push_stack!(Value::Int(BigInt::parse(&len.to_string()).unwrap()));
                }
                Op::TextChars => {
                    let text = match pop!() {
                        Value::Text(text) => text,
                        _ => {
                            return Err(RuntimeError::new(
                                "R_TYPE_INVALID",
                                "text_chars requires Text",
                                instruction.span,
                            ))
                        }
                    };
                    let count = text.chars().count();
                    if count > self.policy.max_collection_items {
                        return Err(RuntimeError::new(
                            "R_COLLECTION_LIMIT",
                            "text_chars result exceeds the configured item limit",
                            instruction.span,
                        ));
                    }
                    let allocation = text.chars().fold(
                        size_of::<Vec<Value>>()
                            .saturating_add(count.saturating_mul(size_of::<Value>())),
                        |total, scalar| {
                            total.saturating_add(size_of::<String>() + scalar.len_utf8())
                        },
                    );
                    charge_allocation(budget.as_ref(), allocation, instruction.span)?;
                    let values = text
                        .chars()
                        .map(|scalar| Value::Text(scalar.to_string()))
                        .collect::<Vec<_>>();
                    push_stack!(Value::from_list_items(values));
                }
                Op::BytesFromInts => {
                    let values = match pop!() {
                        Value::List(values) => values,
                        _ => {
                            return Err(RuntimeError::new(
                                "R_TYPE_INVALID",
                                "bytes_from_ints requires List[Int]",
                                instruction.span,
                            ))
                        }
                    };
                    if values.len() > self.policy.max_collection_items {
                        return Err(RuntimeError::new(
                            "R_COLLECTION_LIMIT",
                            "bytes_from_ints exceeds the configured item limit",
                            instruction.span,
                        ));
                    }
                    if values.len() > 64 * 1024 * 1024 {
                        return Err(RuntimeError::new(
                            "R_ALLOCATION_LIMIT",
                            "byte value exceeds the 64 MiB limit",
                            instruction.span,
                        ));
                    }
                    let mut bytes = Vec::with_capacity(values.len());
                    for value in values.iter() {
                        let Value::Int(value) = value else {
                            return Err(RuntimeError::new(
                                "R_TYPE_INVALID",
                                "bytes_from_ints requires List[Int]",
                                instruction.span,
                            ));
                        };
                        let Some(byte) = value.to_usize().filter(|byte| *byte <= 255) else {
                            return Err(RuntimeError::new(
                                "R_BYTE_RANGE",
                                "bytes_from_ints items must be between 0 and 255",
                                instruction.span,
                            ));
                        };
                        bytes.push(byte as u8);
                    }
                    charge_allocation(budget.as_ref(), bytes.capacity(), instruction.span)?;
                    push_stack!(Value::Bytes(Arc::new(bytes)));
                }
                Op::BytesFromText => {
                    let text = match pop!() {
                        Value::Text(text) => text,
                        _ => {
                            return Err(RuntimeError::new(
                                "R_TYPE_INVALID",
                                "bytes_from_text requires Text",
                                instruction.span,
                            ))
                        }
                    };
                    if text.len() > 64 * 1024 * 1024 {
                        return Err(RuntimeError::new(
                            "R_ALLOCATION_LIMIT",
                            "byte value exceeds the 64 MiB limit",
                            instruction.span,
                        ));
                    }
                    charge_allocation(budget.as_ref(), text.len(), instruction.span)?;
                    push_stack!(Value::Bytes(Arc::new(text.into_bytes())));
                }
                Op::BytesJoin => {
                    let values = match pop!() {
                        Value::List(values) => values,
                        _ => {
                            return Err(RuntimeError::new(
                                "R_TYPE_INVALID",
                                "bytes_join requires List[Bytes]",
                                instruction.span,
                            ))
                        }
                    };
                    if values.len() > self.policy.max_collection_items {
                        return Err(RuntimeError::new(
                            "R_COLLECTION_LIMIT",
                            "bytes_join exceeds the configured item limit",
                            instruction.span,
                        ));
                    }
                    let mut total = 0usize;
                    for value in values.iter() {
                        let Value::Bytes(bytes) = value else {
                            return Err(RuntimeError::new(
                                "R_TYPE_INVALID",
                                "bytes_join requires List[Bytes]",
                                instruction.span,
                            ));
                        };
                        total = total.checked_add(bytes.len()).ok_or_else(|| {
                            RuntimeError::new(
                                "R_ALLOCATION_LIMIT",
                                "joined byte value length overflow",
                                instruction.span,
                            )
                        })?;
                        if total > 64 * 1024 * 1024 {
                            return Err(RuntimeError::new(
                                "R_ALLOCATION_LIMIT",
                                "joined byte value exceeds the 64 MiB limit",
                                instruction.span,
                            ));
                        }
                    }
                    charge_allocation(budget.as_ref(), total, instruction.span)?;
                    let mut bytes = Vec::with_capacity(total);
                    for value in values.iter() {
                        if let Value::Bytes(part) = value {
                            bytes.extend_from_slice(part);
                        }
                    }
                    push_stack!(Value::Bytes(Arc::new(bytes)));
                }
                Op::Float64Bits => match pop!() {
                    Value::Float64(value) => {
                        push_stack!(Value::Int(BigInt::from_u64(value.get().to_bits())))
                    }
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "float64_bits requires Float64",
                            instruction.span,
                        ))
                    }
                },
                Op::Index => {
                    let index = pop!();
                    let values = pop!();
                    match (values, index) {
                        (Value::Float64List(items), Value::Int(index)) => {
                            let index = index.to_usize().ok_or_else(|| {
                                RuntimeError::new(
                                    "R_INDEX_INVALID",
                                    "list index must be a non-negative machine-sized integer",
                                    instruction.span,
                                )
                            })?;
                            let value =
                                items
                                    .get(index)
                                    .copied()
                                    .map(Value::Float64)
                                    .ok_or_else(|| {
                                        RuntimeError::new(
                                            "R_INDEX_BOUNDS",
                                            format!(
                                                "list index {index} is out of range for length {}",
                                                items.len()
                                            ),
                                            instruction.span,
                                        )
                                    })?;
                            charge_allocation(
                                budget.as_ref(),
                                cloned_value_allocation(&value),
                                instruction.span,
                            )?;
                            push_stack!(value);
                        }
                        (Value::List(items), Value::Int(index)) => {
                            let index = index.to_usize().ok_or_else(|| {
                                RuntimeError::new(
                                    "R_INDEX_INVALID",
                                    "list index must be a non-negative machine-sized integer",
                                    instruction.span,
                                )
                            })?;
                            let value = items.get(index).cloned().ok_or_else(|| {
                                RuntimeError::new(
                                    "R_INDEX_BOUNDS",
                                    format!(
                                        "list index {index} is out of range for length {}",
                                        items.len()
                                    ),
                                    instruction.span,
                                )
                            })?;
                            charge_allocation(
                                budget.as_ref(),
                                cloned_value_allocation(&value),
                                instruction.span,
                            )?;
                            push_stack!(value);
                        }
                        (Value::Map(items), Value::Text(key)) => {
                            let value = items.get(&key).cloned().ok_or_else(|| {
                                RuntimeError::new(
                                    "R_MAP_KEY_MISSING",
                                    format!("map has no key `{key}`"),
                                    instruction.span,
                                )
                            })?;
                            charge_allocation(
                                budget.as_ref(),
                                cloned_value_allocation(&value),
                                instruction.span,
                            )?;
                            push_stack!(value);
                        }
                        (Value::Text(text), Value::Int(index)) => {
                            let index = index.to_usize().ok_or_else(|| {
                                RuntimeError::new(
                                    "R_INDEX_INVALID",
                                    "text index must be a non-negative machine-sized integer",
                                    instruction.span,
                                )
                            })?;
                            let length = text.chars().count();
                            let value = text.chars().nth(index).map(|scalar| scalar.to_string()).ok_or_else(|| {
                                RuntimeError::new(
                                    "R_INDEX_BOUNDS",
                                    format!("text index {index} is out of range for length {length}"),
                                    instruction.span,
                                )
                            })?;
                            let value = Value::Text(value);
                            charge_allocation(
                                budget.as_ref(),
                                cloned_value_allocation(&value),
                                instruction.span,
                            )?;
                            push_stack!(value);
                        }
                        (Value::List(_), _) => {
                            return Err(RuntimeError::new(
                                "R_INDEX_INVALID",
                                "list index must be Int",
                                instruction.span,
                            ))
                        }
                        (Value::Float64List(_), _) => {
                            return Err(RuntimeError::new(
                                "R_INDEX_INVALID",
                                "list index must be Int",
                                instruction.span,
                            ))
                        }
                        (Value::Map(_), _) => {
                            return Err(RuntimeError::new(
                                "R_INDEX_INVALID",
                                "map index must be Text",
                                instruction.span,
                            ))
                        }
                        (Value::Text(_), _) => {
                            return Err(RuntimeError::new(
                                "R_INDEX_INVALID",
                                "text index must be Int",
                                instruction.span,
                            ))
                        }
                        _ => {
                            return Err(RuntimeError::new(
                                "R_TYPE_INVALID",
                                "index requires a List, Map, or Text",
                                instruction.span,
                            ))
                        }
                    }
                }
                Op::ListPush => {
                    let value = pop!();
                    let list_value = pop!();
                    let items = match &list_value {
                        Value::List(items) => items.len(),
                        Value::Float64List(items) => items.len(),
                        _ => {
                            return Err(RuntimeError::new(
                                "R_TYPE_INVALID",
                                "list_push first argument must be a List",
                                instruction.span,
                            ))
                        }
                    };
                    if items >= self.policy.max_collection_items {
                        return Err(RuntimeError::new(
                            "R_COLLECTION_LIMIT",
                            "list exceeds the configured item limit",
                            instruction.span,
                        ));
                    }
                    let copy_bytes = match &list_value {
                        Value::Float64List(_) => size_of::<Vec<Float64>>().saturating_add(
                            items.saturating_add(1).saturating_mul(size_of::<Float64>()),
                        ),
                        _ => size_of::<Vec<Value>>().saturating_add(
                            items.saturating_add(1).saturating_mul(size_of::<Value>()),
                        ),
                    };
                    charge_allocation(budget.as_ref(), copy_bytes, instruction.span)?;
                    let list = match list_value {
                        Value::List(items) => {
                            let mut updated = items.as_ref().clone();
                            updated.push(value);
                            Value::from_list_items(updated)
                        }
                        Value::Float64List(items) => {
                            let Value::Float64(value) = value else {
                                return Err(RuntimeError::new(
                                    "R_TYPE_INVALID",
                                    "list_push value must match the list item type",
                                    instruction.span,
                                ));
                            };
                            let mut updated = items.as_ref().clone();
                            updated.push(value);
                            Value::Float64List(Arc::new(updated))
                        }
                        _ => unreachable!("list type checked above"),
                    };
                    if let Some(error) = value_limit_error(&list, &self.policy, instruction.span) {
                        return Err(error);
                    }
                    push_stack!(list);
                }
                Op::ListPushLocal(slot) => {
                    let value = pop!();
                    let local = locals.get_mut(slot).ok_or_else(|| {
                        RuntimeError::new(
                            "R_LOCAL_UNINITIALIZED",
                            "list_push assignment targets an uninitialized local",
                            instruction.span,
                        )
                    })?;
                    match local {
                        Value::List(items) => {
                            let len = items.len();
                            if len >= self.policy.max_collection_items {
                                return Err(RuntimeError::new(
                                    "R_COLLECTION_LIMIT",
                                    "list exceeds the configured item limit",
                                    instruction.span,
                                ));
                            }
                            let shared = Arc::strong_count(items) > 1;
                            let capacity = items.capacity();
                            let allocation = if shared || capacity == len {
                                let grown_capacity =
                                    capacity.saturating_mul(2).max(len.saturating_add(1)).max(4);
                                size_of::<Vec<Value>>().saturating_add(
                                    grown_capacity.saturating_mul(size_of::<Value>()),
                                )
                            } else {
                                0
                            };
                            charge_allocation(budget.as_ref(), allocation, instruction.span)?;
                            Arc::make_mut(items).push(value);
                        }
                        Value::Float64List(items) => {
                            let Value::Float64(value) = value else {
                                return Err(RuntimeError::new(
                                    "R_TYPE_INVALID",
                                    "list_push value must match the List item type",
                                    instruction.span,
                                ));
                            };
                            let len = items.len();
                            if len >= self.policy.max_collection_items {
                                return Err(RuntimeError::new(
                                    "R_COLLECTION_LIMIT",
                                    "list exceeds the configured item limit",
                                    instruction.span,
                                ));
                            }
                            let shared = Arc::strong_count(items) > 1;
                            let capacity = items.capacity();
                            let allocation = if shared || capacity == len {
                                let grown_capacity =
                                    capacity.saturating_mul(2).max(len.saturating_add(1)).max(4);
                                size_of::<Vec<Float64>>().saturating_add(
                                    grown_capacity.saturating_mul(size_of::<Float64>()),
                                )
                            } else {
                                0
                            };
                            charge_allocation(budget.as_ref(), allocation, instruction.span)?;
                            Arc::make_mut(items).push(value);
                        }
                        _ => {
                            return Err(RuntimeError::new(
                                "R_TYPE_INVALID",
                                "list_push assignment target must be a List",
                                instruction.span,
                            ));
                        }
                    }
                    if let Some(error) = value_limit_error(local, &self.policy, instruction.span) {
                        return Err(error);
                    }
                }
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
                Op::ListSet => {
                    let value = pop!();
                    let index = match pop!() {
                        Value::Int(index) => index.to_usize().ok_or_else(|| {
                            RuntimeError::new(
                                "R_LIST_INDEX",
                                "list_set index must be a non-negative machine-sized integer",
                                instruction.span,
                            )
                        })?,
                        _ => {
                            return Err(RuntimeError::new(
                                "R_LIST_INDEX",
                                "list_set index must be Int",
                                instruction.span,
                            ))
                        }
                    };
                    let list_value = pop!();
                    let len = match &list_value {
                        Value::List(items) => items.len(),
                        Value::Float64List(items) => items.len(),
                        _ => {
                            return Err(RuntimeError::new(
                                "R_TYPE_INVALID",
                                "list_set first argument must be a List",
                                instruction.span,
                            ))
                        }
                    };
                    if index >= len {
                        return Err(RuntimeError::new(
                            "R_LIST_INDEX",
                            format!("list_set index {index} is out of range for length {}", len),
                            instruction.span,
                        ));
                    }
                    let copy_bytes = match &list_value {
                        Value::Float64List(_) => size_of::<Vec<Float64>>()
                            .saturating_add(len.saturating_mul(size_of::<Float64>())),
                        _ => size_of::<Vec<Value>>()
                            .saturating_add(len.saturating_mul(size_of::<Value>())),
                    };
                    charge_allocation(budget.as_ref(), copy_bytes, instruction.span)?;
                    let list = match list_value {
                        Value::List(items) => {
                            let mut updated = items.as_ref().clone();
                            updated[index] = value;
                            Value::from_list_items(updated)
                        }
                        Value::Float64List(items) => {
                            let Value::Float64(value) = value else {
                                return Err(RuntimeError::new(
                                    "R_TYPE_INVALID",
                                    "list_set value must match the list item type",
                                    instruction.span,
                                ));
                            };
                            let mut updated = items.as_ref().clone();
                            updated[index] = value;
                            Value::Float64List(Arc::new(updated))
                        }
                        _ => unreachable!("list type checked above"),
                    };
                    if let Some(error) = value_limit_error(&list, &self.policy, instruction.span) {
                        return Err(error);
                    }
                    push_stack!(list);
                }
                Op::ListSetLocal(slot) => {
                    let value = pop!();
                    let index = match pop!() {
                        Value::Int(index) => index.to_usize().ok_or_else(|| {
                            RuntimeError::new(
                                "R_LIST_INDEX",
                                "list_set index must be a non-negative machine-sized integer",
                                instruction.span,
                            )
                        })?,
                        _ => {
                            return Err(RuntimeError::new(
                                "R_LIST_INDEX",
                                "list_set index must be Int",
                                instruction.span,
                            ))
                        }
                    };
                    let local = locals.get_mut(slot).ok_or_else(|| {
                        RuntimeError::new(
                            "R_LOCAL_UNINITIALIZED",
                            "list_set assignment targets an uninitialized local",
                            instruction.span,
                        )
                    })?;
                    let len = match local {
                        Value::List(items) => items.len(),
                        Value::Float64List(items) => items.len(),
                        _ => {
                            return Err(RuntimeError::new(
                                "R_TYPE_INVALID",
                                "list_set assignment target must be a List",
                                instruction.span,
                            ))
                        }
                    };
                    if index >= len {
                        return Err(RuntimeError::new(
                            "R_LIST_INDEX",
                            format!("list_set index {index} is out of range for length {}", len),
                            instruction.span,
                        ));
                    }
                    match local {
                        Value::List(items) => {
                            if Arc::strong_count(items) > 1 {
                                let copy_bytes = size_of::<Vec<Value>>()
                                    .saturating_add(len.saturating_mul(size_of::<Value>()));
                                charge_allocation(budget.as_ref(), copy_bytes, instruction.span)?;
                            }
                            Arc::make_mut(items)[index] = value;
                        }
                        Value::Float64List(items) => {
                            let Value::Float64(value) = value else {
                                return Err(RuntimeError::new(
                                    "R_TYPE_INVALID",
                                    "list_set value must match the list item type",
                                    instruction.span,
                                ));
                            };
                            if Arc::strong_count(items) > 1 {
                                let copy_bytes = size_of::<Vec<Float64>>()
                                    .saturating_add(len.saturating_mul(size_of::<Float64>()));
                                charge_allocation(budget.as_ref(), copy_bytes, instruction.span)?;
                            }
                            Arc::make_mut(items)[index] = value;
                        }
                        _ => unreachable!("list type checked above"),
                    }
                    if let Some(error) = value_limit_error(local, &self.policy, instruction.span) {
                        return Err(error);
                    }
                }
                Op::Equal | Op::NotEqual => {
                    let b = pop!();
                    let a = pop!();
                    let eq = a == b;
                    push_stack!(Value::Bool(if matches!(instruction.op, Op::Equal) {
                        eq
                    } else {
                        !eq
                    }));
                }
                Op::Less | Op::LessEqual | Op::Greater | Op::GreaterEqual => {
                    let b = pop!();
                    let a = pop!();
                    let ord = match (&a, &b) {
                        (Value::Int(x), Value::Int(y)) => Some(x.cmp(y)),
                        (Value::Float64(x), Value::Float64(y)) => x.get().partial_cmp(&y.get()),
                        (Value::Text(x), Value::Text(y)) => Some(x.cmp(y)),
                        _ => None,
                    }
                    .ok_or_else(|| {
                        RuntimeError::new(
                            "R_TYPE_INVALID",
                            "invalid operands for comparison",
                            instruction.span,
                        )
                    })?;
                    let value = match instruction.op {
                        Op::Less => ord.is_lt(),
                        Op::LessEqual => !ord.is_gt(),
                        Op::Greater => ord.is_gt(),
                        Op::GreaterEqual => !ord.is_lt(),
                        _ => unreachable!(),
                    };
                    push_stack!(Value::Bool(value));
                }
                Op::Jump(target) => ip = target,
                Op::JumpIfFalse(target) => {
                    if let Value::Bool(c) = pop!() {
                        if !c {
                            ip = target;
                        }
                    } else {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "conditional jump requires Bool",
                            instruction.span,
                        ));
                    }
                }
                Op::Call(target, argc) => {
                    let mut call_args = Vec::with_capacity(argc);
                    for _ in 0..argc {
                        call_args.push(pop!());
                    }
                    call_args.reverse();
                    return Ok(VmStep::Call {
                        frame: VmFrame {
                            function_index,
                            depth,
                            call_site,
                            locals,
                            stack,
                            ip,
                        },
                        target,
                        args: call_args,
                        span: instruction.span,
                    });
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
                    push_stack!(Value::from_list_items(values));
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
                                size_of::<String>().saturating_add(text.capacity()),
                                instruction.span,
                            )?;
                            push_stack!(Value::ResultOk(Box::new(Value::Text(text))));
                        }
                        Err(mut error) => {
                            if budget.is_cancelled() {
                                self.finish_effect(effect, EffectOutcome::Interrupted);
                                continue;
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
                                    push_stack!(Value::ResultErr(Box::new(Value::HttpError(
                                        error
                                    ))));
                                    continue;
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
                        Value::Text(body) if !byte_response => body.into_bytes(),
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
                                decoded.insert(name.clone(), value.clone());
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
                    let effect = self
                        .begin_effect(EffectKind::HttpRequest, http_effect_target(&method, &url));
                    match perform_http_request_bytes(
                        &http,
                        &self.policy,
                        &method,
                        &url,
                        &headers,
                        &body,
                        request_timeout,
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
                                    .map(|(key, value)| (key, Value::Text(value)))
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
                                            &self.policy,
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
                                continue;
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
                    let resolved =
                        match host_file_path(&self.policy, &path, false, instruction.span) {
                            Ok(path) => path,
                            Err(error) => {
                                push_stack!(file_error_result(error));
                                continue;
                            }
                        };
                    let effect = self.begin_effect(EffectKind::FileRead, path.as_str());
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
                    let handle = match runtime_reader_handle(&value, instruction.span) {
                        Ok(handle) => handle,
                        Err(error) => return Err(error),
                    };
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
                                        Value::Text(line.text)
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
                            Ok(Some(row)) => push_stack!(Value::ResultOk(Box::new(
                                Value::OptionSome(Box::new(Value::Record(
                                    "CsvRow".into(),
                                    Arc::new(vec![
                                        Value::Int(BigInt::from_usize(row.start_line)),
                                        Value::from_list_items(
                                            row.fields.into_iter().map(Value::Text).collect()
                                        )
                                    ]),
                                )))
                            ))),
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
                                        continue;
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
                                Value::Text(text) => Ok(text.clone()),
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
                    charge_allocation(budget.as_ref(), text.len(), instruction.span)?;
                    push_stack!(Value::ResultOk(Box::new(Value::Text(text))));
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
                        Op::TimeParseTimestamp => {
                            stdlib_data::parse_timestamp(&text).map(|micros| {
                                Value::Record(
                                    "Instant".into(),
                                    Arc::new(vec![Value::Int(BigInt::from_i64(micros))]),
                                )
                            })
                        }
                        Op::SemVerParse => stdlib_data::parse_semver(&text).map(|version| {
                            Value::Record(
                                "SemVer".into(),
                                Arc::new(vec![
                                    Value::Text(version.major),
                                    Value::Text(version.minor),
                                    Value::Text(version.patch),
                                    Value::from_list_items(
                                        version.prerelease.into_iter().map(Value::Text).collect(),
                                    ),
                                    Value::from_list_items(
                                        version.build.into_iter().map(Value::Text).collect(),
                                    ),
                                    Value::Text(version.original),
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
                    charge_allocation(budget.as_ref(), text.len(), instruction.span)?;
                    push_stack!(Value::Text(text));
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
                    let parse_record =
                        |value: &Value| -> Result<stdlib_data::SemVer, RuntimeError> {
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
                            Value::Text(value) => Ok(value.clone()),
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
                            name.clone(),
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
                                            values.into_iter().map(Value::Text).collect(),
                                        ),
                                    )
                                })
                                .collect::<BTreeMap<_, _>>();
                            let result = Value::Record(
                                "CliArgs".into(),
                                Arc::new(vec![
                                    Value::Map(Arc::new(values)),
                                    Value::from_list_items(
                                        parsed.positionals.into_iter().map(Value::Text).collect(),
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
                    let resolved =
                        match host_file_path_no_follow(&self.policy, &path, instruction.span) {
                            Ok(path) => path,
                            Err(error) => {
                                push_stack!(file_error_result(error));
                                continue;
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
                                            Value::Text(entry.relative_path),
                                            Value::Text(entry.kind),
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
                        continue;
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
                        .get(&key)
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
                    let output = if instruction.op == Op::TextCaseFold {
                        stdlib_services::text_casefold(&text)
                    } else {
                        stdlib_services::text_collapse_whitespace(&text)
                    };
                    if output.len() > self.policy.max_text_bytes {
                        return Err(RuntimeError::new(
                            "R_TEXT_LIMIT",
                            "Unicode result exceeds the configured text limit",
                            instruction.span,
                        ));
                    }
                    charge_allocation(budget.as_ref(), output.len(), instruction.span)?;
                    push_stack!(Value::Text(output));
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
                            push_stack!(Value::ResultOk(Box::new(Value::Text(output))));
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
                            .at_path(path.clone())),
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
                                                            .map(Value::Text)
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
                        Op::TomlGetText => {
                            self.toml_documents.get_text(handle, &path).map(Value::Text)
                        }
                        Op::TomlGetInt => self
                            .toml_documents
                            .get_integer(handle, &path)
                            .map(|value| Value::Int(BigInt::from_i64(value))),
                        Op::TomlGetBool => {
                            self.toml_documents.get_bool(handle, &path).map(Value::Bool)
                        }
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
                            .and_then(|value| {
                                self.toml_documents.put_integer(handle, &path, value)
                            }),
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
                    charge_allocation(budget.as_ref(), text.len(), instruction.span)?;
                    push_stack!(Value::Text(text));
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
                    let resolved =
                        match host_file_path(&self.policy, &path, false, instruction.span) {
                            Ok(resolved) => resolved,
                            Err(error) => {
                                push_stack!(file_error_result(error));
                                continue;
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
                            continue;
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
                            continue;
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
                        size_of::<String>().saturating_add(text.capacity()),
                        instruction.span,
                    )?;
                    let effect = self.begin_effect(EffectKind::FileRead, path);
                    self.finish_effect(effect, EffectOutcome::Returned);
                    push_stack!(Value::ResultOk(Box::new(Value::Text(text))));
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
                    let resolved = match host_file_path(&self.policy, &path, true, instruction.span)
                    {
                        Ok(resolved) => resolved,
                        Err(error) => {
                            push_stack!(file_error_result(error));
                            continue;
                        }
                    };
                    let effect = self.begin_effect(EffectKind::FileWrite, path.as_str());
                    if let Err(error) = fs::write(&resolved, content) {
                        self.finish_effect(effect, EffectOutcome::ReturnedError);
                        push_stack!(file_error_result(RuntimeError::new(
                            "R_FILE_WRITE",
                            format!("cannot write `{path}`: {error}"),
                            instruction.span,
                        )));
                        continue;
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
                    let resolved = match host_file_path(&self.policy, &path, true, instruction.span)
                    {
                        Ok(resolved) => resolved,
                        Err(error) => {
                            push_stack!(file_error_result(error));
                            continue;
                        }
                    };
                    let effect = self.begin_effect(EffectKind::FileWrite, path.as_str());
                    if let Err(error) = atomic_replace_text(&resolved, &content) {
                        self.finish_effect(effect, EffectOutcome::ReturnedError);
                        push_stack!(file_error_result(RuntimeError::new(
                            file_io_error_code(&error, "R_FILE_WRITE"),
                            format!("cannot atomically replace `{path}`: {error}"),
                            instruction.span,
                        )));
                        continue;
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
                    let resolved =
                        match host_file_path(&self.policy, &path, false, instruction.span) {
                            Ok(resolved) => resolved,
                            Err(error) => {
                                push_stack!(file_error_result(error));
                                continue;
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
                            continue;
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
                            continue;
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
                    let effect = self.begin_effect(EffectKind::FileRead, path);
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
                    let resolved = match host_file_path(&self.policy, &path, true, instruction.span)
                    {
                        Ok(resolved) => resolved,
                        Err(error) => {
                            push_stack!(file_error_result(error));
                            continue;
                        }
                    };
                    let effect = self.begin_effect(EffectKind::FileWrite, path.as_str());
                    if let Err(error) = fs::write(&resolved, content.as_slice()) {
                        self.finish_effect(effect, EffectOutcome::ReturnedError);
                        push_stack!(file_error_result(RuntimeError::new(
                            "R_FILE_WRITE",
                            format!("cannot write `{path}`: {error}"),
                            instruction.span
                        )));
                        continue;
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
                        .map(Value::Text)
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
                        .map(Value::Text)
                        .collect::<Vec<_>>();
                    let allocation = values.iter().fold(
                        size_of::<Vec<Value>>()
                            .saturating_add(values.len().saturating_mul(size_of::<Value>())),
                        |total, value| match value {
                            Value::Text(text) => {
                                total.saturating_add(size_of::<String>() + text.len())
                            }
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
                    if !self.policy.allowed_environment_variables.contains(&name) {
                        return Err(RuntimeError::new(
                            "R_ENV_DENIED",
                            format!("environment variable `{name}` is not granted"),
                            instruction.span,
                        ));
                    }
                    let effect = self.begin_effect(EffectKind::EnvironmentRead, name.as_str());
                    match std::env::var(name) {
                        Ok(value) => {
                            self.finish_effect(effect, EffectOutcome::Returned);
                            push_stack!(Value::OptionSome(Box::new(Value::Text(value))))
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
                Op::Assert => match pop!() {
                    Value::Bool(true) => push_stack!(Value::Unit),
                    Value::Bool(false) => {
                        return Err(RuntimeError::new(
                            "R_ASSERTION",
                            "assertion failed",
                            instruction.span,
                        ))
                    }
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "assert requires Bool",
                            instruction.span,
                        ))
                    }
                },
                Op::IsOk => match pop!() {
                    Value::ResultOk(_) => push_stack!(Value::Bool(true)),
                    Value::ResultErr(_) => push_stack!(Value::Bool(false)),
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "is_ok requires Result",
                            instruction.span,
                        ))
                    }
                },
                Op::IsSome => match pop!() {
                    Value::OptionSome(_) => push_stack!(Value::Bool(true)),
                    Value::OptionNone => push_stack!(Value::Bool(false)),
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "is_some requires Option",
                            instruction.span,
                        ))
                    }
                },
                Op::Unwrap => match pop!() {
                    Value::ResultOk(value) => push_stack!(*value),
                    Value::OptionSome(value) => push_stack!(*value),
                    Value::ResultErr(_) => {
                        return Err(RuntimeError::new(
                            "R_RESULT_UNWRAP",
                            "cannot unwrap an Err result",
                            instruction.span,
                        ))
                    }
                    Value::OptionNone => {
                        return Err(RuntimeError::new(
                            "R_OPTION_UNWRAP",
                            "cannot unwrap None",
                            instruction.span,
                        ))
                    }
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "unwrap requires Result or Option",
                            instruction.span,
                        ))
                    }
                },
                Op::GetError => match pop!() {
                    Value::ResultErr(value) => push_stack!(*value),
                    Value::ResultOk(_) => {
                        return Err(RuntimeError::new(
                            "R_RESULT_ERROR",
                            "cannot get the error from an Ok result",
                            instruction.span,
                        ))
                    }
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "error requires Result",
                            instruction.span,
                        ))
                    }
                },
                Op::MakeOk => {
                    let value = pop!();
                    charge_allocation(budget.as_ref(), size_of::<Value>(), instruction.span)?;
                    push_stack!(Value::ResultOk(Box::new(value)));
                }
                Op::MakeErr => {
                    let value = pop!();
                    charge_allocation(budget.as_ref(), size_of::<Value>(), instruction.span)?;
                    push_stack!(Value::ResultErr(Box::new(value)));
                }
                Op::MakeNone => push_stack!(Value::OptionNone),
                Op::MakeSome => {
                    let value = pop!();
                    charge_allocation(budget.as_ref(), size_of::<Value>(), instruction.span)?;
                    push_stack!(Value::OptionSome(Box::new(value)));
                }
                Op::TryResult => match pop!() {
                    Value::ResultOk(value) => push_stack!(*value),
                    Value::ResultErr(error) => return Ok(VmStep::Return(Value::ResultErr(error))),
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "`?` requires Result",
                            instruction.span,
                        ))
                    }
                },
                Op::JsonErrorMessage
                | Op::JsonErrorLine
                | Op::JsonErrorColumn
                | Op::JsonErrorCode => {
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
                        Op::JsonErrorMessage => Value::Text(error.message),
                        Op::JsonErrorLine => Value::Int(BigInt::from_usize(error.line)),
                        Op::JsonErrorColumn => Value::Int(BigInt::from_usize(error.column)),
                        Op::JsonErrorCode => Value::Text(error.code),
                        _ => unreachable!(),
                    });
                }
                Op::FileErrorCode
                | Op::FileErrorMessage
                | Op::FileErrorLine
                | Op::FileErrorColumn => {
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
                        Op::FileErrorCode => Value::Text(error.code),
                        Op::FileErrorMessage => Value::Text(error.message),
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
                            error.code
                        } else {
                            error.message
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
                    Value::TaskError(error) => push_stack!(Value::Text(error.code)),
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "task_code requires TaskError",
                            instruction.span,
                        ))
                    }
                },
                Op::TaskMessage => match pop!() {
                    Value::TaskError(error) => push_stack!(Value::Text(error.message)),
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
                    let result = self.call_python(
                        target,
                        &call_args,
                        &[],
                        budget.as_ref(),
                        instruction.span,
                    )?;
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
                        &self.policy,
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
                        &self.policy,
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
                        &self.policy,
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
                        &self.policy,
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
                        serde_json::Value::String(text),
                        &self.policy,
                        budget.as_ref(),
                        instruction.span,
                    )?);
                }
                Op::JsonArray => {
                    push_stack!(finish_json_value(
                        serde_json::Value::Array(Vec::new()),
                        &self.policy,
                        budget.as_ref(),
                        instruction.span,
                    )?);
                }
                Op::JsonObject => {
                    push_stack!(finish_json_value(
                        serde_json::Value::Object(serde_json::Map::new()),
                        &self.policy,
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
                        &self.policy,
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
                    fields.insert(key, Arc::new(child));
                    push_stack!(finish_shared_json(
                        object,
                        &self.policy,
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
                    let value = match parse_unique_json(&source) {
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
                            continue;
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
                            continue;
                        }
                    };
                    let value =
                        finish_json_value(value, &self.policy, budget.as_ref(), instruction.span)?;
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
                    let text = serde_json::to_string(&value.to_serde()).map_err(|error| {
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
                        size_of::<String>().saturating_add(text.capacity()),
                        instruction.span,
                    )?;
                    push_stack!(Value::Text(text));
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
                                Op::JsonTryText => {
                                    stdlib_data::json_try_text(value.as_ref()).map(Value::Text)
                                }
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
                                                message:
                                                    "JSON number is outside finite Float64 range"
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
                    let value = object.get(&key).cloned().ok_or_else(|| {
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
                    push_stack!(Value::Bool(object.contains_key(&key)));
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
                        let value = object.get(field.as_str()).ok_or_else(|| {
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
                        let value = object.get(field.as_str()).ok_or_else(|| {
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
                        &self.policy,
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
                            &self.policy,
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
                        &self.policy,
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
                            push_stack!(Value::Text(text.to_string()))
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
                Op::Print | Op::PrintErr => {
                    let v = pop!();
                    let remaining_output = self.policy.output_limit_bytes.saturating_sub(
                        self.output.len().saturating_add(self.program_stderr.len()),
                    );
                    let mut rendered = LimitedString::new(remaining_output);
                    if writeln!(&mut rendered, "{v}").is_err() {
                        return Err(RuntimeError::new(
                            "R_OUTPUT_LIMIT",
                            "program output exceeds the configured limit",
                            instruction.span,
                        ));
                    }
                    charge_allocation(budget.as_ref(), rendered.value.len(), instruction.span)?;
                    let (destination, kind) = if matches!(&instruction.op, Op::PrintErr) {
                        (&mut self.program_stderr, EffectKind::ProgramErrorOutput)
                    } else {
                        (&mut self.output, EffectKind::ProgramOutput)
                    };
                    destination.push_str(&rendered.value);
                    let effect = self.begin_effect(kind, format!("{} bytes", rendered.value.len()));
                    self.finish_effect(effect, EffectOutcome::Returned);
                }
                Op::Return => {
                    let result = pop!();
                    if !value_matches(&result, &f.result, &self.artifact.records) {
                        return Err(RuntimeError::new(
                            "R_RETURN_INVALID",
                            "function returned a value with an invalid type",
                            instruction.span,
                        ));
                    }
                    if let Some(error) = value_limit_error(&result, &self.policy, instruction.span)
                    {
                        return Err(error);
                    }
                    return Ok(VmStep::Return(result));
                }
            }
        }
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
            return Err(RuntimeError::new(
                "R_PYTHON_UNAVAILABLE",
                "this external call requires the optional `python-interop` feature",
                span,
            ));
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

struct Budget {
    remaining: AtomicU64,
    allocation_remaining: Arc<AtomicUsize>,
    allocation_floor: usize,
    cancellation: CancellationToken,
    local_cancellation: Option<Arc<AtomicBool>>,
}

struct LimitedString {
    value: String,
    max_bytes: usize,
}

impl LimitedString {
    fn new(max_bytes: usize) -> Self {
        Self {
            value: String::new(),
            max_bytes,
        }
    }
}

impl std::fmt::Write for LimitedString {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        if text.len() > self.max_bytes.saturating_sub(self.value.len()) {
            return Err(std::fmt::Error);
        }
        self.value.push_str(text);
        Ok(())
    }
}

impl Budget {
    fn new(
        instruction_limit: u64,
        allocation_limit: usize,
        cancellation: CancellationToken,
    ) -> Self {
        Self {
            remaining: AtomicU64::new(instruction_limit),
            allocation_remaining: Arc::new(AtomicUsize::new(allocation_limit)),
            allocation_floor: 0,
            cancellation,
            local_cancellation: None,
        }
    }

    fn is_cancelled(&self) -> bool {
        self.cancellation.is_cancelled()
            || self
                .local_cancellation
                .as_ref()
                .is_some_and(|flag| flag.load(Ordering::Acquire))
    }
    fn consume(&self) -> bool {
        self.remaining
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |left| {
                if left > 0 {
                    Some(left - 1)
                } else {
                    None
                }
            })
            .is_ok()
    }

    fn reserve(&self, requested: u64) -> u64 {
        self.remaining
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |left| {
                let reserved = left.min(requested);
                Some(left - reserved)
            })
            .map(|left| left.min(requested))
            .unwrap_or(0)
    }

    fn refund(&self, count: u64) {
        self.remaining.fetch_add(count, Ordering::Relaxed);
    }
    fn reserve_parallel(&self) -> u64 {
        let mut remaining = self.remaining.load(Ordering::Relaxed);
        loop {
            let reserved = remaining.saturating_mul(3) / 4;
            match self.remaining.compare_exchange_weak(
                remaining,
                remaining - reserved,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return reserved,
                Err(actual) => remaining = actual,
            }
        }
    }

    fn allocate(&self, bytes: usize) -> bool {
        self.allocation_remaining
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |left| {
                if bytes <= left.saturating_sub(self.allocation_floor) {
                    Some(left - bytes)
                } else {
                    None
                }
            })
            .is_ok()
    }

    fn refund_allocation(&self, bytes: usize) {
        self.allocation_remaining
            .fetch_add(bytes, Ordering::Relaxed);
    }

    fn parallel_allocation_floor(&self) -> usize {
        let remaining = self.allocation_remaining.load(Ordering::Relaxed);
        let available = remaining.saturating_sub(self.allocation_floor);
        remaining.saturating_sub(available.saturating_mul(3) / 4)
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
        let Some(decoded) = encoded
            .iter()
            .map(|value| Float64::new(f64::from_bits(*value as u64)))
            .collect::<Option<Vec<_>>>()
        else {
            return std::ptr::null_mut();
        };
        let bytes = if typed_empty != 0 {
            size_of::<Vec<Float64>>()
        } else {
            size_of::<Vec<Value>>().saturating_add(count.saturating_mul(size_of::<Value>()))
        };
        if !arena.charge(bytes) {
            return std::ptr::null_mut();
        }
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
        let storage_id = arena.int_storage.len();
        arena.int_storage.push(encoded.to_vec());
        arena.int_lists.push(Box::new(NativeIntList {
            data: arena.int_storage.last_mut().unwrap().as_mut_ptr(),
            len: count,
            storage_id,
            owned: true,
        }));
        (&mut **arena.int_lists.last_mut().unwrap() as *mut NativeIntList).cast()
    } else {
        std::ptr::null_mut()
    }
}

unsafe extern "C" fn native_list_set(
    arena: *mut NativeListArena,
    list: *mut std::ffi::c_void,
    index: i64,
    value: i64,
    item_kind: i64,
    always_copy: i64,
) -> *mut std::ffi::c_void {
    if arena.is_null() || list.is_null() || index < 0 || !(1..=3).contains(&item_kind) {
        return std::ptr::null_mut();
    }
    let arena = unsafe { &mut *arena };
    let Ok(index) = usize::try_from(index) else {
        return std::ptr::null_mut();
    };
    if item_kind == NATIVE_LIST_FLOAT_KIND {
        let descriptor = unsafe { &mut *(list.cast::<NativeFloatList>()) };
        if index >= descriptor.len {
            return std::ptr::null_mut();
        }
        let Some(value) = Float64::new(f64::from_bits(value as u64)) else {
            return std::ptr::null_mut();
        };
        if always_copy == 0 && descriptor.owned {
            let Some(storage) = arena.float_storage.get_mut(descriptor.storage_id) else {
                return std::ptr::null_mut();
            };
            storage[index] = value;
            return list;
        }
        let storage_id = descriptor.storage_id;
        let list_len = descriptor.len;
        let bytes =
            size_of::<Vec<Float64>>().saturating_add(list_len.saturating_mul(size_of::<Float64>()));
        if !arena.charge(bytes) {
            return std::ptr::null_mut();
        }
        let Some(storage) = arena.float_storage.get(storage_id) else {
            return std::ptr::null_mut();
        };
        let mut copied = storage.clone();
        copied[index] = value;
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
        }));
        (&mut **arena.float_lists.last_mut().unwrap() as *mut NativeFloatList).cast()
    } else {
        if item_kind == NATIVE_LIST_BOOL_KIND && value != 0 && value != 1 {
            return std::ptr::null_mut();
        }
        let descriptor = unsafe { &mut *(list.cast::<NativeIntList>()) };
        if index >= descriptor.len {
            return std::ptr::null_mut();
        }
        if always_copy == 0 && descriptor.owned {
            let Some(storage) = arena.int_storage.get_mut(descriptor.storage_id) else {
                return std::ptr::null_mut();
            };
            storage[index] = value;
            return list;
        }
        let storage_id = descriptor.storage_id;
        let list_len = descriptor.len;
        let bytes =
            size_of::<Vec<Value>>().saturating_add(list_len.saturating_mul(size_of::<Value>()));
        if !arena.charge(bytes) {
            return std::ptr::null_mut();
        }
        let Some(storage) = arena.int_storage.get(storage_id) else {
            return std::ptr::null_mut();
        };
        let mut copied = storage.clone();
        copied[index] = value;
        let storage_id = arena.int_storage.len();
        arena.int_storage.push(copied);
        arena.int_lists.push(Box::new(NativeIntList {
            data: arena.int_storage.last_mut().unwrap().as_mut_ptr(),
            len: list_len,
            storage_id,
            owned: true,
        }));
        (&mut **arena.int_lists.last_mut().unwrap() as *mut NativeIntList).cast()
    }
}

unsafe extern "C" fn native_charge_record(arena: *mut NativeListArena, bytes: i64) -> i32 {
    if arena.is_null() || bytes < 0 {
        return 0;
    }
    let Ok(bytes) = usize::try_from(bytes) else {
        return 0;
    };
    let ok = unsafe { &mut *arena }.charge(bytes);
    i32::from(ok)
}

unsafe extern "C" fn native_print(arena: *mut NativeListArena, value: i64, kind: i64) -> i32 {
    if arena.is_null() {
        return 0;
    }
    i32::from(unsafe { &mut *arena }.print(value, kind))
}

fn charge_allocation(budget: &Budget, bytes: usize, span: Span) -> Result<(), RuntimeError> {
    if budget.allocate(bytes) {
        Ok(())
    } else {
        Err(RuntimeError::new(
            "R_ALLOCATION_LIMIT",
            "cumulative allocation budget exhausted during this invocation",
            span,
        ))
    }
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

fn new_value_allocation(value: &Value) -> usize {
    match value {
        // Integer size and growth are bounded separately by max_integer_digits.
        // Counting transient scalar values here made VM and JIT budgets differ.
        Value::Int(_) => 0,
        Value::Text(text) => size_of::<String>().saturating_add(text.capacity()),
        Value::Bytes(bytes) => size_of::<Vec<u8>>().saturating_add(bytes.capacity()),
        Value::Float64List(items) => size_of::<Vec<Float64>>()
            .saturating_add(items.len().saturating_mul(size_of::<Float64>())),
        // Records own a fresh field-vector, but nested Lists, Maps, and JSON
        // remain Arc-shared. Their payloads were charged when first created.
        Value::Record(name, fields) => size_of::<Vec<Value>>()
            .saturating_add(fields.len().saturating_mul(size_of::<Value>()))
            .saturating_add(name.capacity()),
        // JSON nodes are immutable and Arc-shared; cloning a value allocates no subtree.
        Value::Json(_) => 0,
        Value::TaskError(error) => size_of::<TaskErrorValue>()
            .saturating_add(error.code.capacity())
            .saturating_add(error.message.capacity())
            .saturating_add(error.trace.capacity().saturating_mul(size_of::<Span>())),
        Value::JsonError(error) => size_of::<JsonErrorValue>()
            .saturating_add(error.code.capacity())
            .saturating_add(error.message.capacity()),
        Value::FileError(error) => size_of::<FileErrorValue>()
            .saturating_add(error.code.capacity())
            .saturating_add(error.message.capacity())
            .saturating_add(error.trace.capacity().saturating_mul(size_of::<Span>())),
        Value::HttpError(error) => size_of::<HttpErrorValue>()
            .saturating_add(error.code.capacity())
            .saturating_add(error.message.capacity())
            .saturating_add(error.trace.capacity().saturating_mul(size_of::<Span>())),
        Value::OptionSome(value) | Value::ResultOk(value) | Value::ResultErr(value) => {
            size_of::<Value>().saturating_add(new_value_allocation(value))
        }
        _ => 0,
    }
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

fn cloned_value_allocation(value: &Value) -> usize {
    match value {
        Value::Int(_) => 0,
        Value::Text(text) => size_of::<String>().saturating_add(text.len()),
        Value::Record(name, _) => size_of::<String>().saturating_add(name.len()),
        Value::OptionSome(value) | Value::ResultOk(value) | Value::ResultErr(value) => {
            size_of::<Value>().saturating_add(cloned_value_allocation(value))
        }
        Value::TaskError(error) => size_of::<TaskErrorValue>()
            .saturating_add(error.code.len())
            .saturating_add(error.message.len())
            .saturating_add(error.trace.len().saturating_mul(size_of::<Span>())),
        Value::JsonError(error) => size_of::<JsonErrorValue>()
            .saturating_add(error.code.len())
            .saturating_add(error.message.len()),
        Value::FileError(error) => size_of::<FileErrorValue>()
            .saturating_add(error.code.len())
            .saturating_add(error.message.len())
            .saturating_add(error.trace.len().saturating_mul(size_of::<Span>())),
        Value::HttpError(error) => size_of::<HttpErrorValue>()
            .saturating_add(error.code.len())
            .saturating_add(error.message.len())
            .saturating_add(error.trace.len().saturating_mul(size_of::<Span>())),
        Value::NativeResource(resource) => {
            size_of::<NativeResourceValue>().saturating_add(resource.kind().len())
        }
        // Lists, maps, and JSON trees use shared immutable storage.
        Value::List(_) | Value::Float64List(_) | Value::Map(_) | Value::Json(_) => 0,
        _ => 0,
    }
}

fn map_clone_allocation(
    values: &BTreeMap<String, Value>,
    new_key: Option<&str>,
    new_value: &Value,
) -> usize {
    let mut total = size_of::<BTreeMap<String, Value>>();
    for (key, value) in values {
        total = total
            .saturating_add(size_of::<String>() + key.len())
            .saturating_add(size_of::<Value>() * 3)
            .saturating_add(cloned_value_allocation(value));
    }
    if let Some(key) = new_key {
        if !values.contains_key(key) {
            total = total
                .saturating_add(size_of::<String>() + key.len())
                .saturating_add(size_of::<Value>() * 3)
                .saturating_add(cloned_value_allocation(new_value));
        } else {
            total = total.saturating_add(cloned_value_allocation(new_value));
        }
    }
    total
}

struct JsonMetrics {
    depth: usize,
    item_count: usize,
    oversized_integer: bool,
    allocation_bytes: usize,
    encoded_len: usize,
}

fn encoded_json_string_len(value: &str) -> usize {
    value.bytes().fold(2usize, |size, byte| {
        size.saturating_add(match byte {
            b'"' | b'\\' | b'\x08' | b'\t' | b'\n' | b'\x0c' | b'\r' => 2,
            0..=0x1f => 6,
            _ => 1,
        })
    })
}

fn json_metrics(value: &serde_json::Value, max_integer_digits: usize) -> JsonMetrics {
    use serde_json::Value as JsonValue;
    match value {
        JsonValue::Null | JsonValue::Bool(_) => JsonMetrics {
            depth: 0,
            item_count: 0,
            oversized_integer: false,
            allocation_bytes: size_of::<JsonValue>(),
            encoded_len: match value {
                JsonValue::Null | JsonValue::Bool(true) => 4,
                JsonValue::Bool(false) => 5,
                _ => unreachable!(),
            },
        },
        JsonValue::Number(number) => {
            let text = number.to_string();
            let digits = text.strip_prefix('-').unwrap_or(&text);
            let is_integer = !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit());
            JsonMetrics {
                depth: 0,
                item_count: 0,
                oversized_integer: is_integer && digits.len() > max_integer_digits,
                allocation_bytes: size_of::<JsonValue>().saturating_add(text.len()),
                encoded_len: text.len(),
            }
        }
        JsonValue::String(text) => JsonMetrics {
            depth: 0,
            item_count: 0,
            oversized_integer: false,
            allocation_bytes: size_of::<JsonValue>()
                .saturating_add(size_of::<String>())
                .saturating_add(text.len()),
            encoded_len: encoded_json_string_len(text),
        },
        JsonValue::Array(items) => {
            let mut metrics = JsonMetrics {
                depth: 1,
                item_count: items.len(),
                oversized_integer: false,
                allocation_bytes: size_of::<JsonValue>()
                    .saturating_add(size_of::<Vec<JsonValue>>())
                    .saturating_add(items.capacity().saturating_mul(size_of::<JsonValue>())),
                encoded_len: 2usize.saturating_add(items.len().saturating_sub(1)),
            };
            for item in items {
                let child = json_metrics(item, max_integer_digits);
                metrics.depth = metrics.depth.max(child.depth.saturating_add(1));
                metrics.item_count = metrics.item_count.saturating_add(child.item_count);
                metrics.oversized_integer |= child.oversized_integer;
                metrics.allocation_bytes = metrics
                    .allocation_bytes
                    .saturating_add(child.allocation_bytes);
                metrics.encoded_len = metrics.encoded_len.saturating_add(child.encoded_len);
            }
            metrics
        }
        JsonValue::Object(items) => {
            let mut metrics = JsonMetrics {
                depth: 1,
                item_count: items.len(),
                oversized_integer: false,
                allocation_bytes: size_of::<JsonValue>()
                    .saturating_add(size_of::<serde_json::Map<String, JsonValue>>()),
                encoded_len: 2usize.saturating_add(items.len().saturating_sub(1)),
            };
            for (key, child_value) in items {
                let child = json_metrics(child_value, max_integer_digits);
                metrics.depth = metrics.depth.max(child.depth.saturating_add(1));
                metrics.item_count = metrics.item_count.saturating_add(child.item_count);
                metrics.oversized_integer |= child.oversized_integer;
                metrics.allocation_bytes = metrics
                    .allocation_bytes
                    .saturating_add(size_of::<String>())
                    .saturating_add(key.len())
                    .saturating_add(size_of::<JsonValue>() * 3)
                    .saturating_add(child.allocation_bytes);
                metrics.encoded_len = metrics
                    .encoded_len
                    .saturating_add(encoded_json_string_len(key))
                    .saturating_add(1)
                    .saturating_add(child.encoded_len);
            }
            metrics
        }
    }
}

fn json_exact_integer(value: &SharedJson) -> Option<BigInt> {
    let number = value.as_number()?;
    if let Some(value) = number.as_i64() {
        return Some(BigInt::from_i64(value));
    }
    if let Some(value) = number.as_u64() {
        return Some(BigInt::from_u64(value));
    }
    BigInt::parse(&number.to_string()).ok()
}

fn finish_json_value(
    value: serde_json::Value,
    policy: &HostPolicy,
    budget: &Budget,
    span: Span,
) -> Result<Value, RuntimeError> {
    let metrics = json_metrics(&value, policy.max_integer_digits);
    validate_json_metrics(&metrics, policy, span)?;
    charge_allocation(budget, metrics.allocation_bytes, span)?;
    Ok(Value::Json(SharedJson::from_serde(value)))
}

fn shared_json_metrics(value: &SharedJson, max_integer_digits: usize) -> JsonMetrics {
    match value {
        SharedJson::Null | SharedJson::Bool(_) => JsonMetrics {
            depth: 0,
            item_count: 0,
            oversized_integer: false,
            allocation_bytes: size_of::<SharedJson>(),
            encoded_len: match value {
                SharedJson::Null | SharedJson::Bool(true) => 4,
                SharedJson::Bool(false) => 5,
                _ => unreachable!(),
            },
        },
        SharedJson::Number(number) => {
            let text = number.to_string();
            let digits = text.strip_prefix('-').unwrap_or(&text);
            let is_integer = !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit());
            JsonMetrics {
                depth: 0,
                item_count: 0,
                oversized_integer: is_integer && digits.len() > max_integer_digits,
                allocation_bytes: size_of::<SharedJson>().saturating_add(text.len()),
                encoded_len: text.len(),
            }
        }
        SharedJson::String(text) => JsonMetrics {
            depth: 0,
            item_count: 0,
            oversized_integer: false,
            allocation_bytes: size_of::<SharedJson>().saturating_add(text.len()),
            encoded_len: encoded_json_string_len(text),
        },
        SharedJson::Array(items) => {
            let mut metrics = JsonMetrics {
                depth: 1,
                item_count: items.len(),
                oversized_integer: false,
                allocation_bytes: size_of::<SharedJson>()
                    .saturating_add(size_of::<Vec<Arc<SharedJson>>>())
                    .saturating_add(
                        items
                            .capacity()
                            .saturating_mul(size_of::<Arc<SharedJson>>()),
                    ),
                encoded_len: 2usize.saturating_add(items.len().saturating_sub(1)),
            };
            for item in items.iter() {
                let child = shared_json_metrics(item, max_integer_digits);
                metrics.depth = metrics.depth.max(child.depth.saturating_add(1));
                metrics.item_count = metrics.item_count.saturating_add(child.item_count);
                metrics.oversized_integer |= child.oversized_integer;
                metrics.encoded_len = metrics.encoded_len.saturating_add(child.encoded_len);
            }
            metrics
        }
        SharedJson::Object(items) => {
            let mut metrics = JsonMetrics {
                depth: 1,
                item_count: items.len(),
                oversized_integer: false,
                allocation_bytes: size_of::<SharedJson>()
                    .saturating_add(size_of::<BTreeMap<String, Arc<SharedJson>>>()),
                encoded_len: 2usize.saturating_add(items.len().saturating_sub(1)),
            };
            for (key, child_value) in items.iter() {
                let child = shared_json_metrics(child_value, max_integer_digits);
                metrics.depth = metrics.depth.max(child.depth.saturating_add(1));
                metrics.item_count = metrics.item_count.saturating_add(child.item_count);
                metrics.oversized_integer |= child.oversized_integer;
                metrics.allocation_bytes = metrics
                    .allocation_bytes
                    .saturating_add(size_of::<String>() + key.len())
                    .saturating_add(size_of::<Arc<SharedJson>>() * 3);
                metrics.encoded_len = metrics
                    .encoded_len
                    .saturating_add(encoded_json_string_len(key))
                    .saturating_add(1)
                    .saturating_add(child.encoded_len);
            }
            metrics
        }
    }
}

fn finish_shared_json(
    value: SharedJson,
    policy: &HostPolicy,
    budget: &Budget,
    span: Span,
) -> Result<Value, RuntimeError> {
    let metrics = shared_json_metrics(&value, policy.max_integer_digits);
    validate_json_metrics(&metrics, policy, span)?;
    charge_allocation(budget, metrics.allocation_bytes, span)?;
    Ok(Value::Json(Arc::new(value)))
}

fn validate_json_metrics(
    metrics: &JsonMetrics,
    policy: &HostPolicy,
    span: Span,
) -> Result<(), RuntimeError> {
    if metrics.depth > 64 {
        return Err(RuntimeError::new(
            "R_JSON_DEPTH",
            "JSON nesting exceeds 64 levels",
            span,
        ));
    }
    if metrics.encoded_len > policy.max_text_bytes {
        return Err(RuntimeError::new(
            "R_TEXT_LIMIT",
            "JSON value exceeds the configured byte limit",
            span,
        ));
    }
    if metrics.oversized_integer {
        return Err(RuntimeError::new(
            "R_INTEGER_LIMIT",
            "JSON integer exceeds the configured digit limit",
            span,
        ));
    }
    if metrics.item_count > policy.max_collection_items {
        return Err(RuntimeError::new(
            "R_COLLECTION_LIMIT",
            "JSON collection exceeds the configured item limit",
            span,
        ));
    }
    Ok(())
}

fn shared_json_push_local_allocation(value: &Arc<SharedJson>) -> usize {
    let SharedJson::Array(items) = value.as_ref() else {
        return 0;
    };
    let len = items.len();
    let mut allocation = if Arc::strong_count(value) > 1 {
        size_of::<SharedJson>()
    } else {
        0
    };
    if Arc::strong_count(items) > 1 {
        allocation = allocation.saturating_add(
            size_of::<Vec<Arc<SharedJson>>>()
                .saturating_add(len.saturating_mul(size_of::<Arc<SharedJson>>())),
        );
    } else if items.capacity() == len {
        allocation = allocation.saturating_add(
            size_of::<Vec<Arc<SharedJson>>>().saturating_add(
                items
                    .capacity()
                    .saturating_mul(2)
                    .max(len.saturating_add(1))
                    .max(4)
                    .saturating_mul(size_of::<Arc<SharedJson>>()),
            ),
        );
    }
    allocation
}

fn value_matches(value: &Value, ty: &Type, records: &[veritas_model::RecordDefinition]) -> bool {
    match (value, ty) {
        (Value::Int(_), Type::Int)
        | (Value::Float64(_), Type::Float64)
        | (Value::Bool(_), Type::Bool)
        | (Value::Text(_), Type::Text)
        | (Value::Bytes(_), Type::Bytes)
        | (Value::Unit, Type::Unit) => true,
        (Value::List(items), Type::List(item_ty)) => items
            .iter()
            .all(|item| value_matches(item, item_ty, records)),
        (Value::Float64List(_), Type::List(item_ty)) => **item_ty == Type::Float64,
        (Value::Map(items), Type::Map(item_ty)) => items
            .values()
            .all(|item| value_matches(item, item_ty, records)),
        (Value::OptionNone, Type::Option(_)) => true,
        (Value::OptionSome(value), Type::Option(item)) => value_matches(value, item, records),
        (Value::ResultOk(value), Type::Result(ok, _)) => value_matches(value, ok, records),
        (Value::ResultErr(value), Type::Result(_, err)) => value_matches(value, err, records),
        (Value::Record(name, fields), Type::Record(expected)) if name == expected => records
            .iter()
            .find(|record| record.name == *name)
            .is_some_and(|record| {
                fields.len() == record.fields.len()
                    && fields
                        .iter()
                        .zip(&record.fields)
                        .all(|(value, field)| value_matches(value, &field.ty, records))
            }),
        (Value::TaskError(_), Type::TaskError) => true,
        (Value::Json(_), Type::Json) => true,
        (Value::JsonError(_), Type::JsonError) => true,
        (Value::FileError(_), Type::FileError) => true,
        (Value::HttpError(_), Type::HttpError) => true,
        (Value::NativeResource(resource), Type::NativeResource(expected)) => {
            resource.kind() == expected
        }
        _ => false,
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
            Value::Text(text) => Ok(text.clone()),
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
            let decoded = match kind.as_str() {
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
                    stdlib_services::SqlValue::Text(text.clone())
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
            Value::Text(error.code),
            Value::Text(error.message),
            Value::Text(error.path),
            Value::Int(BigInt::from_usize(error.line)),
            Value::Int(BigInt::from_usize(error.column)),
        ]),
    )
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
            Value::Text(text),
            Value::Bytes(Arc::new(blob)),
        ]),
    )
}

fn service_calendar_event(event: stdlib_services::CalendarEvent) -> Value {
    let instant = |seconds: Option<i64>| {
        seconds
            .map(|seconds| {
                let microseconds = &BigInt::from_i64(seconds) * &BigInt::from_i64(1_000_000);
                Value::OptionSome(Box::new(Value::Record(
                    "Instant".into(),
                    Arc::new(vec![Value::Int(microseconds)]),
                )))
            })
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
        .map(|rule| Value::OptionSome(Box::new(Value::Text(rule))))
        .unwrap_or(Value::OptionNone);
    Value::Record(
        "CalendarEvent".into(),
        Arc::new(vec![
            Value::Text(event.uid),
            instant(event.start_unix_seconds),
            instant(event.end_unix_seconds),
            date(event.start_date),
            date(event.end_date),
            Value::Text(event.status),
            Value::Bool(event.all_day),
            recurrence,
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

fn perform_http_request_bytes(
    host: &HostHttpClient,
    policy: &HostPolicy,
    method: &str,
    input: &str,
    headers: &BTreeMap<String, String>,
    body: &[u8],
    request_timeout: Duration,
    budget: &Budget,
) -> Result<HttpBytesResponse, HttpErrorValue> {
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
        "GET",
        input,
        &BTreeMap::new(),
        &[],
        policy.http_timeout,
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

fn parse_unique_json(source: &str) -> Result<serde_json::Value, JsonDecodeError> {
    use serde::de::DeserializeSeed;

    let (safe_source, reserved_key) = escape_reserved_json_keys(source);
    let duplicate = std::rc::Rc::new(std::cell::RefCell::new(None));
    let mut deserializer = serde_json::Deserializer::from_str(&safe_source);
    let parsed = UniqueJsonSeed {
        duplicate: std::rc::Rc::clone(&duplicate),
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
            if let Some(key) = duplicate.borrow_mut().take() {
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
                "$veritas_private_{number:0width$}$",
                width = RESERVED.len() - 18
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

#[derive(Clone)]
struct UniqueJsonSeed {
    duplicate: std::rc::Rc<std::cell::RefCell<Option<String>>>,
}

impl<'de> serde::de::DeserializeSeed<'de> for UniqueJsonSeed {
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

struct UniqueJsonVisitor {
    duplicate: std::rc::Rc<std::cell::RefCell<Option<String>>>,
}

impl<'de> serde::de::Visitor<'de> for UniqueJsonVisitor {
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
            duplicate: std::rc::Rc::clone(&self.duplicate),
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
            duplicate: std::rc::Rc::clone(&self.duplicate),
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
                *self.duplicate.borrow_mut() = Some(current_key);
                return Err(serde::de::Error::custom("duplicate JSON object key"));
            }
            let value = map.next_value_seed(UniqueJsonSeed {
                duplicate: std::rc::Rc::clone(&self.duplicate),
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
    use super::{escape_reserved_json_keys, parse_unique_json, JsonDecodeError};

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
}

#[cfg(test)]
mod function_cache_tests {
    use super::share_artifact;
    use std::sync::Arc;
    use veritas_model::{Artifact, Function, Type};

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
        let temporary = parent.join(format!(".{name}.veritas-tmp-{}-{id}", std::process::id()));
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

fn value_limit_error(value: &Value, policy: &HostPolicy, span: Span) -> Option<RuntimeError> {
    value_limit_error_with_json(value, policy, span, false)
}

fn value_limit_error_at_boundary(
    value: &Value,
    policy: &HostPolicy,
    span: Span,
) -> Option<RuntimeError> {
    value_limit_error_with_json(value, policy, span, true)
}

fn value_limit_error_with_json(
    value: &Value,
    policy: &HostPolicy,
    span: Span,
    check_json: bool,
) -> Option<RuntimeError> {
    match value {
        Value::Record(_, fields) => fields
            .iter()
            .find_map(|field| value_limit_error_with_json(field, policy, span, check_json)),
        Value::Int(i) if i.digit_count() > policy.max_integer_digits => Some(RuntimeError::new(
            "R_INTEGER_LIMIT",
            format!(
                "integer has {} digits; configured limit is {}",
                i.digit_count(),
                policy.max_integer_digits
            ),
            span,
        )),
        Value::Text(s) if s.len() > policy.max_text_bytes => Some(RuntimeError::new(
            "R_TEXT_LIMIT",
            format!(
                "text has {} bytes; configured limit is {}",
                s.len(),
                policy.max_text_bytes
            ),
            span,
        )),
        Value::Bytes(bytes) if bytes.len() > policy.max_file_bytes => Some(RuntimeError::new(
            "R_FILE_LIMIT",
            format!(
                "byte value has {} bytes; configured file limit is {}",
                bytes.len(),
                policy.max_file_bytes
            ),
            span,
        )),
        Value::List(items) if items.len() > policy.max_collection_items => Some(RuntimeError::new(
            "R_COLLECTION_LIMIT",
            format!(
                "list has {} items; configured limit is {}",
                items.len(),
                policy.max_collection_items
            ),
            span,
        )),
        Value::List(items) => items
            .iter()
            .find_map(|item| value_limit_error_with_json(item, policy, span, check_json)),
        Value::Float64List(items) if items.len() > policy.max_collection_items => {
            Some(RuntimeError::new(
                "R_COLLECTION_LIMIT",
                format!(
                    "list has {} items; configured limit is {}",
                    items.len(),
                    policy.max_collection_items
                ),
                span,
            ))
        }
        Value::Float64List(_) => None,
        Value::Map(items) if items.len() > policy.max_collection_items => Some(RuntimeError::new(
            "R_COLLECTION_LIMIT",
            format!(
                "map has {} items; configured limit is {}",
                items.len(),
                policy.max_collection_items
            ),
            span,
        )),
        Value::Map(items) => items.iter().find_map(|(key, value)| {
            if key.len() > policy.max_text_bytes {
                Some(RuntimeError::new(
                    "R_TEXT_LIMIT",
                    "map key exceeds configured text limit",
                    span,
                ))
            } else {
                value_limit_error_with_json(value, policy, span, check_json)
            }
        }),
        Value::OptionSome(value) => value_limit_error_with_json(value, policy, span, check_json),
        Value::ResultOk(value) | Value::ResultErr(value) => {
            value_limit_error_with_json(value, policy, span, check_json)
        }
        Value::Json(value) if check_json => {
            let metrics = shared_json_metrics(value, policy.max_integer_digits);
            validate_json_metrics(&metrics, policy, span).err()
        }
        Value::Json(_) => None,
        Value::TaskError(error)
            if error.message.len() > policy.max_text_bytes
                || error.code.len() > policy.max_text_bytes =>
        {
            Some(RuntimeError::new(
                "R_TEXT_LIMIT",
                "task error text exceeds the configured byte limit",
                span,
            ))
        }
        _ => None,
    }
}

fn verify(artifact: &Artifact) -> Result<(), RuntimeError> {
    let fail = |msg: String| RuntimeError::new("R_ARTIFACT_INVALID", msg, Span::default());
    if !(14..=Artifact::CURRENT_VERSION).contains(&artifact.format_version) {
        return Err(RuntimeError::new(
            "R_ARTIFACT_VERSION",
            format!(
                "artifact format {} is unsupported (runtime supports 14 through {})",
                artifact.format_version,
                Artifact::CURRENT_VERSION
            ),
            Span::default(),
        ));
    }
    if artifact.functions.is_empty() {
        return Err(fail("artifact contains no functions".into()));
    }
    let mut source_names = HashSet::new();
    if artifact.sources.is_empty()
        || artifact
            .sources
            .iter()
            .any(|source| source.is_empty() || !source_names.insert(source.as_str()))
        || !artifact.sources.contains(&artifact.source_name)
    {
        return Err(fail("artifact must contain named source files".into()));
    }
    let mut record_names = HashSet::new();
    for record in &artifact.records {
        if record.name.is_empty() || !record_names.insert(record.name.clone()) {
            return Err(fail("record names must be nonempty and unique".into()));
        }
        let mut field_names = HashSet::new();
        if record.fields.is_empty()
            || record
                .fields
                .iter()
                .any(|field| field.name.is_empty() || !field_names.insert(field.name.as_str()))
        {
            return Err(fail(format!("record `{}` has invalid fields", record.name)));
        }
    }
    let reserved_types = [
        "Int",
        "Float64",
        "Bool",
        "Text",
        "Unit",
        "Json",
        "JsonError",
        "FileError",
        "TaskError",
        "Bytes",
        "List",
        "Map",
        "Option",
        "Result",
    ];
    if artifact
        .records
        .iter()
        .any(|record| reserved_types.contains(&record.name.as_str()))
    {
        return Err(fail("record name conflicts with a built-in type".into()));
    }
    if artifact.records.iter().any(|record| {
        record
            .fields
            .iter()
            .any(|field| !artifact_type_known(&field.ty, &record_names))
    }) || artifact.functions.iter().any(|function| {
        !function
            .parameters
            .iter()
            .all(|ty| artifact_type_known(ty, &record_names))
            || !artifact_type_known(&function.result, &record_names)
    }) || artifact.externals.iter().any(|external| {
        !external
            .parameters
            .iter()
            .all(|ty| artifact_type_known(ty, &record_names))
            || !artifact_type_known(&external.result, &record_names)
    }) {
        return Err(fail("artifact references an unknown type".into()));
    }
    if !artifact.functions.iter().any(|f| f.name == artifact.entry) {
        return Err(fail("entry function does not exist".into()));
    }
    let mut names = record_names
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    for function in &artifact.functions {
        if function.name.is_empty() || !names.insert(function.name.as_str()) {
            return Err(fail("function names must be nonempty and unique".into()));
        }
    }
    for external in &artifact.externals {
        if external.name.is_empty()
            || external.module.is_empty()
            || external.callable.is_empty()
            || external.parameter_names.len() != external.parameters.len()
            || !names.insert(external.name.as_str())
        {
            return Err(fail(
                "external declarations must have unique names and complete signatures".into(),
            ));
        }
        let mut parameter_names = HashSet::new();
        if external
            .parameter_names
            .iter()
            .any(|name| name.is_empty() || !parameter_names.insert(name.as_str()))
        {
            return Err(fail(format!(
                "external `{}` has invalid parameter names",
                external.name
            )));
        }
    }
    let entry = artifact
        .functions
        .iter()
        .find(|f| f.name == artifact.entry)
        .unwrap();
    if !entry.parameters.is_empty() || !matches!(entry.result, Type::Int | Type::Unit) {
        return Err(fail(
            "entry function must take no parameters and return Int or Unit".into(),
        ));
    }
    for f in &artifact.functions {
        if f.local_count < f.parameters.len() {
            return Err(fail(format!(
                "function `{}` has fewer locals than parameters",
                f.name
            )));
        }
        for i in &f.code {
            if i.span.source_id as usize >= artifact.sources.len() {
                return Err(fail(format!(
                    "function `{}` references a missing source file",
                    f.name
                )));
            }
            match i.op {
                Op::Load(s)
                | Op::Store(s)
                | Op::ListSetLocal(s)
                | Op::ListPushLocal(s)
                | Op::JsonPushLocal(s)
                    if s >= f.local_count =>
                {
                    return Err(fail(format!(
                        "function `{}` references missing local {s}",
                        f.name
                    )))
                }
                Op::Jump(t) | Op::JumpIfFalse(t) if t > f.code.len() => {
                    return Err(fail(format!(
                        "function `{}` jumps outside its code",
                        f.name
                    )))
                }
                Op::Call(target, argc)
                    if target >= artifact.functions.len()
                        || argc != artifact.functions[target].parameters.len() =>
                {
                    return Err(fail(format!(
                        "function `{}` contains an invalid call",
                        f.name
                    )))
                }
                Op::ParallelMap(target)
                    if target >= artifact.functions.len()
                        || artifact.functions[target].parameters.len() != 1 =>
                {
                    return Err(fail(format!(
                        "function `{}` contains an invalid parallel worker",
                        f.name
                    )))
                }
                Op::SortBy(target)
                    if target >= artifact.functions.len()
                        || artifact.functions[target].parameters.len() != 2
                        || artifact.functions[target].parameters[0]
                            != artifact.functions[target].parameters[1]
                        || artifact.functions[target].result != Type::Int =>
                {
                    return Err(fail(format!(
                        "function `{}` contains an invalid sort comparator",
                        f.name
                    )))
                }
                Op::PythonCall(target, argc)
                    if target >= artifact.externals.len()
                        || argc != artifact.externals[target].parameters.len() =>
                {
                    return Err(fail(format!(
                        "function `{}` contains an invalid Python call",
                        f.name
                    )))
                }
                Op::PythonCallKw(target, positional_count, ref names)
                    if target >= artifact.externals.len()
                        || positional_count > artifact.externals[target].parameters.len()
                        || positional_count + names.len()
                            > artifact.externals[target].parameters.len() =>
                {
                    return Err(fail(format!(
                        "function `{}` contains an invalid Python keyword call",
                        f.name
                    )))
                }
                Op::ReadText
                | Op::WriteText
                | Op::WriteTextAtomic
                | Op::ReadBytes
                | Op::WriteBytes
                | Op::Args
                | Op::ArgsFrom
                | Op::Assert => {}
                _ => {}
            }
        }
        verify_stack(f, artifact).map_err(|m| fail(format!("function `{}`: {m}", f.name)))?;
    }
    veritas_model::validate_sort_comparator_effects(artifact).map_err(fail)?;
    Ok(())
}

fn artifact_type_known(ty: &Type, record_names: &HashSet<String>) -> bool {
    match ty {
        Type::Record(name) => record_names.contains(name),
        Type::NativeResource(name) => matches!(
            name.as_str(),
            "FileReader" | "Db" | "Transaction" | "TomlDocument"
        ),
        Type::List(item) | Type::Map(item) | Type::Option(item) => {
            artifact_type_known(item, record_names)
        }
        Type::Result(ok, err) => {
            artifact_type_known(ok, record_names) && artifact_type_known(err, record_names)
        }
        Type::Int
        | Type::Float64
        | Type::Bool
        | Type::Text
        | Type::Bytes
        | Type::TaskError
        | Type::Json
        | Type::JsonError
        | Type::FileError
        | Type::HttpError
        | Type::Unit => true,
    }
}

fn type_contains_native_resource(ty: &Type) -> bool {
    match ty {
        Type::NativeResource(_) => true,
        Type::List(item) | Type::Map(item) | Type::Option(item) => {
            type_contains_native_resource(item)
        }
        Type::Result(ok, err) => {
            type_contains_native_resource(ok) || type_contains_native_resource(err)
        }
        _ => false,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FlowState {
    stack: Vec<Type>,
    locals: Vec<Option<Type>>,
}

fn pop_type(stack: &mut Vec<Type>, pc: usize) -> Result<Type, String> {
    stack
        .pop()
        .ok_or_else(|| format!("stack underflow at instruction {pc}"))
}

fn value_type(value: &Value) -> Option<Type> {
    Some(match value {
        Value::Int(_) => Type::Int,
        Value::Float64(_) => Type::Float64,
        Value::Record(name, _) => Type::Record(name.clone()),
        Value::NativeResource(resource) => Type::NativeResource(resource.kind().to_owned()),
        Value::Bool(_) => Type::Bool,
        Value::Text(_) => Type::Text,
        Value::Bytes(_) => Type::Bytes,
        Value::Unit => Type::Unit,
        Value::List(items) => {
            let first = items.first()?;
            let item_type = value_type(first)?;
            if items
                .iter()
                .skip(1)
                .any(|item| value_type(item).as_ref() != Some(&item_type))
            {
                return None;
            }
            Type::List(Box::new(item_type))
        }
        Value::Float64List(_) => Type::List(Box::new(Type::Float64)),
        Value::Map(items) => {
            let first = items.values().next()?;
            let item_type = value_type(first)?;
            if items
                .values()
                .skip(1)
                .any(|item| value_type(item).as_ref() != Some(&item_type))
            {
                return None;
            }
            Type::Map(Box::new(item_type))
        }
        Value::OptionNone => return None,
        Value::OptionSome(value) => Type::Option(Box::new(value_type(value)?)),
        Value::ResultOk(value) => {
            Type::Result(Box::new(value_type(value)?), Box::new(Type::TaskError))
        }
        Value::ResultErr(value) => Type::Result(Box::new(Type::Unit), Box::new(value_type(value)?)),
        Value::TaskError(_) => Type::TaskError,
        Value::Json(_) => Type::Json,
        Value::JsonError(_) => Type::JsonError,
        Value::FileError(_) => Type::FileError,
        Value::HttpError(_) => Type::HttpError,
    })
}

fn verify_stack(function: &veritas_model::Function, artifact: &Artifact) -> Result<(), String> {
    if function.code.is_empty() {
        return Err("function has no instructions".into());
    }
    let mut initial = FlowState {
        stack: Vec::new(),
        locals: vec![None; function.local_count],
    };
    for (slot, ty) in function.parameters.iter().enumerate() {
        initial.locals[slot] = Some(ty.clone());
    }
    let mut states = vec![None; function.code.len()];
    let mut pending = VecDeque::new();
    states[0] = Some(initial);
    pending.push_back(0usize);
    let mut has_return = false;
    while let Some(pc) = pending.pop_front() {
        let instruction = &function.code[pc];
        let mut state = states[pc].clone().unwrap();
        let mut successors = Vec::new();
        match &instruction.op {
            Op::Const(value) => {
                let ty = value_type(value).ok_or_else(|| {
                    format!("invalid or heterogeneous constant at instruction {pc}")
                })?;
                if !value_matches(value, &ty, &artifact.records) {
                    return Err(format!(
                        "constant violates its record schema at instruction {pc}"
                    ));
                }
                state.stack.push(ty);
            }
            Op::Load(slot) => state.stack.push(
                state
                    .locals
                    .get(*slot)
                    .and_then(Clone::clone)
                    .ok_or_else(|| format!("read of uninitialized local at instruction {pc}"))?,
            ),
            Op::Store(slot) => {
                let ty = pop_type(&mut state.stack, pc)?;
                state.locals[*slot] = Some(ty);
            }
            Op::ListPushLocal(slot) => {
                let value = pop_type(&mut state.stack, pc)?;
                let Type::List(item) = state
                    .locals
                    .get(*slot)
                    .and_then(Option::as_ref)
                    .ok_or_else(|| format!("list_push reads an uninitialized local at {pc}"))?
                else {
                    return Err(format!("list_push local is not a List at instruction {pc}"));
                };
                if item.as_ref() != &value {
                    return Err(format!("list_push item type mismatch at instruction {pc}"));
                }
            }
            Op::JsonPushLocal(slot) => {
                if pop_type(&mut state.stack, pc)? != Type::Json {
                    return Err(format!("json_push value must be Json at instruction {pc}"));
                }
                if state.locals.get(*slot).and_then(Option::as_ref) != Some(&Type::Json) {
                    return Err(format!("json_push local is not Json at instruction {pc}"));
                }
            }
            Op::ListSetLocal(slot) => {
                let value = pop_type(&mut state.stack, pc)?;
                if pop_type(&mut state.stack, pc)? != Type::Int {
                    return Err(format!("list_set index requires Int at instruction {pc}"));
                }
                let Type::List(item) = state
                    .locals
                    .get(*slot)
                    .and_then(Option::as_ref)
                    .ok_or_else(|| format!("list_set reads an uninitialized local at {pc}"))?
                else {
                    return Err(format!("list_set local is not a List at instruction {pc}"));
                };
                if item.as_ref() != &value {
                    return Err(format!("list_set item type mismatch at instruction {pc}"));
                }
            }
            Op::Pop | Op::Print | Op::PrintErr => {
                pop_type(&mut state.stack, pc)?;
            }
            Op::Assert => {
                if pop_type(&mut state.stack, pc)? != Type::Bool {
                    return Err(format!("assert requires Bool at instruction {pc}"));
                }
                state.stack.push(Type::Unit);
            }
            Op::IsOk | Op::GetError => {
                let Type::Result(ok, err) = pop_type(&mut state.stack, pc)? else {
                    return Err(format!(
                        "result operation requires Result at instruction {pc}"
                    ));
                };
                if matches!(instruction.op, Op::IsOk) {
                    state.stack.push(Type::Bool);
                } else {
                    state.stack.push(*err);
                }
                let _ = ok;
            }
            Op::IsSome => {
                if !matches!(pop_type(&mut state.stack, pc)?, Type::Option(_)) {
                    return Err(format!("is_some requires Option at instruction {pc}"));
                }
                state.stack.push(Type::Bool);
            }
            Op::Unwrap => match pop_type(&mut state.stack, pc)? {
                Type::Result(ok, _) | Type::Option(ok) => state.stack.push(*ok),
                _ => {
                    return Err(format!(
                        "unwrap requires Result or Option at instruction {pc}"
                    ))
                }
            },
            Op::MakeOk | Op::MakeErr => {
                let Type::Result(ok, err) = &function.result else {
                    return Err(format!(
                        "Result constructor outside Result-returning function at instruction {pc}"
                    ));
                };
                let expected = if matches!(instruction.op, Op::MakeOk) {
                    ok.as_ref()
                } else {
                    err.as_ref()
                };
                if pop_type(&mut state.stack, pc)? != *expected {
                    return Err(format!(
                        "Result constructor payload type mismatch at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Result(ok.clone(), err.clone()));
            }
            Op::MakeNone | Op::MakeSome => {
                let Type::Option(item) = &function.result else {
                    return Err(format!(
                        "Option constructor outside Option-returning function at instruction {pc}"
                    ));
                };
                if matches!(instruction.op, Op::MakeSome)
                    && pop_type(&mut state.stack, pc)? != **item
                {
                    return Err(format!(
                        "Option constructor payload mismatch at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Option(item.clone()));
            }
            Op::TryResult => {
                let Type::Result(ok, err) = pop_type(&mut state.stack, pc)? else {
                    return Err(format!("`?` requires Result at instruction {pc}"));
                };
                let Type::Result(_, function_error) = &function.result else {
                    return Err(format!(
                        "`?` outside Result-returning function at instruction {pc}"
                    ));
                };
                if err.as_ref() != function_error.as_ref() {
                    return Err(format!("`?` error type mismatch at instruction {pc}"));
                }
                state.stack.push(*ok);
            }
            Op::TaskIndex | Op::TaskCode | Op::TaskMessage => {
                if pop_type(&mut state.stack, pc)? != Type::TaskError {
                    return Err(format!(
                        "task error accessor requires TaskError at instruction {pc}"
                    ));
                }
                state.stack.push(if instruction.op == Op::TaskIndex {
                    Type::Int
                } else {
                    Type::Text
                });
            }
            Op::JsonNull | Op::JsonArray | Op::JsonObject => {
                state.stack.push(Type::Json);
            }
            Op::JsonBool => {
                if pop_type(&mut state.stack, pc)? != Type::Bool {
                    return Err(format!("json_bool requires Bool at instruction {pc}"));
                }
                state.stack.push(Type::Json);
            }
            Op::JsonInt => {
                if pop_type(&mut state.stack, pc)? != Type::Int {
                    return Err(format!("json_int requires Int at instruction {pc}"));
                }
                state.stack.push(Type::Json);
            }
            Op::JsonFloat => {
                if pop_type(&mut state.stack, pc)? != Type::Float64 {
                    return Err(format!("json_float requires Float64 at instruction {pc}"));
                }
                state.stack.push(Type::Json);
            }
            Op::JsonText => {
                if pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!("json_text requires Text at instruction {pc}"));
                }
                state.stack.push(Type::Json);
            }
            Op::JsonPush => {
                if pop_type(&mut state.stack, pc)? != Type::Json
                    || pop_type(&mut state.stack, pc)? != Type::Json
                {
                    return Err(format!(
                        "json_push requires Json and Json at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Json);
            }
            Op::JsonSet => {
                if pop_type(&mut state.stack, pc)? != Type::Json
                    || pop_type(&mut state.stack, pc)? != Type::Text
                    || pop_type(&mut state.stack, pc)? != Type::Json
                {
                    return Err(format!(
                        "json_set requires Json, Text and Json at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Json);
            }
            Op::JsonParse => {
                if pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!("json_parse requires Text at instruction {pc}"));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Json),
                    Box::new(Type::JsonError),
                ));
            }
            Op::JsonStringify => {
                if pop_type(&mut state.stack, pc)? != Type::Json {
                    return Err(format!("json_stringify requires Json at instruction {pc}"));
                }
                state.stack.push(Type::Text);
            }
            Op::JsonGet | Op::JsonHas => {
                if pop_type(&mut state.stack, pc)? != Type::Text
                    || pop_type(&mut state.stack, pc)? != Type::Json
                {
                    return Err(format!(
                        "JSON key operation requires Json and Text at instruction {pc}"
                    ));
                }
                state.stack.push(if matches!(instruction.op, Op::JsonHas) {
                    Type::Bool
                } else {
                    Type::Json
                });
            }
            Op::JsonIndex => {
                if pop_type(&mut state.stack, pc)? != Type::Int
                    || pop_type(&mut state.stack, pc)? != Type::Json
                {
                    return Err(format!(
                        "json_index requires Json and Int at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Json);
            }
            Op::JsonLength => {
                if pop_type(&mut state.stack, pc)? != Type::Json {
                    return Err(format!("json_len requires Json at instruction {pc}"));
                }
                state.stack.push(Type::Int);
            }
            Op::JsonIntColumn | Op::JsonTextCategories => {
                if pop_type(&mut state.stack, pc)? != Type::Text
                    || pop_type(&mut state.stack, pc)? != Type::Json
                {
                    return Err(format!(
                        "JSON column operation requires Json and Text at instruction {pc}"
                    ));
                }
                state
                    .stack
                    .push(if matches!(instruction.op, Op::JsonIntColumn) {
                        Type::List(Box::new(Type::Int))
                    } else {
                        Type::Json
                    });
            }
            Op::JsonAsIntList => {
                if pop_type(&mut state.stack, pc)? != Type::Json {
                    return Err(format!(
                        "json_as_int_list requires Json at instruction {pc}"
                    ));
                }
                state.stack.push(Type::List(Box::new(Type::Int)));
            }
            Op::JsonProjectColumns => {
                let text_list = Type::List(Box::new(Type::Text));
                if pop_type(&mut state.stack, pc)? != text_list
                    || pop_type(&mut state.stack, pc)? != Type::List(Box::new(Type::Text))
                    || pop_type(&mut state.stack, pc)? != Type::Text
                {
                    return Err(format!(
                        "json_project_columns requires Text, List[Text], and List[Text] at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Json),
                    Box::new(Type::JsonError),
                ));
            }
            Op::JsonTextCategoriesValues => {
                if pop_type(&mut state.stack, pc)? != Type::Json {
                    return Err(format!(
                        "json_text_categories_values requires Json at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Json);
            }
            Op::JsonTryGet | Op::JsonTryIndex => {
                let expected = if instruction.op == Op::JsonTryGet {
                    Type::Text
                } else {
                    Type::Int
                };
                if pop_type(&mut state.stack, pc)? != expected
                    || pop_type(&mut state.stack, pc)? != Type::Json
                {
                    return Err(format!(
                        "JSON accessor argument types do not match at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Json),
                    Box::new(Type::JsonError),
                ));
            }
            Op::JsonTryText | Op::JsonTryInt | Op::JsonTryBool | Op::JsonTryFloat => {
                if pop_type(&mut state.stack, pc)? != Type::Json {
                    return Err(format!("JSON conversion requires Json at instruction {pc}"));
                }
                let result = match instruction.op {
                    Op::JsonTryText => Type::Text,
                    Op::JsonTryInt => Type::Int,
                    Op::JsonTryBool => Type::Bool,
                    Op::JsonTryFloat => Type::Float64,
                    _ => unreachable!(),
                };
                state
                    .stack
                    .push(Type::Result(Box::new(result), Box::new(Type::JsonError)));
            }
            Op::JsonAsText | Op::JsonAsInt | Op::JsonAsFloat | Op::JsonAsBool | Op::JsonIsNull => {
                if pop_type(&mut state.stack, pc)? != Type::Json {
                    return Err(format!("JSON conversion requires Json at instruction {pc}"));
                }
                state.stack.push(match instruction.op {
                    Op::JsonAsText => Type::Text,
                    Op::JsonAsInt => Type::Int,
                    Op::JsonAsFloat => Type::Float64,
                    Op::JsonAsBool | Op::JsonIsNull => Type::Bool,
                    _ => unreachable!(),
                });
            }
            Op::JsonErrorMessage | Op::JsonErrorColumn | Op::JsonErrorLine | Op::JsonErrorCode => {
                if pop_type(&mut state.stack, pc)? != Type::JsonError {
                    return Err(format!(
                        "JSON error accessor requires JsonError at instruction {pc}"
                    ));
                }
                state.stack.push(match instruction.op {
                    Op::JsonErrorColumn | Op::JsonErrorLine => Type::Int,
                    _ => Type::Text,
                });
            }
            Op::FileErrorCode | Op::FileErrorMessage | Op::FileErrorLine | Op::FileErrorColumn => {
                if pop_type(&mut state.stack, pc)? != Type::FileError {
                    return Err(format!(
                        "file error accessor requires FileError at instruction {pc}"
                    ));
                }
                state.stack.push(
                    if matches!(instruction.op, Op::FileErrorLine | Op::FileErrorColumn) {
                        Type::Int
                    } else {
                        Type::Text
                    },
                );
            }
            Op::HttpErrorCode | Op::HttpErrorMessage => {
                if pop_type(&mut state.stack, pc)? != Type::HttpError {
                    return Err(format!(
                        "HTTP error accessor requires HttpError at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Text);
            }
            Op::Add => {
                let right = pop_type(&mut state.stack, pc)?;
                let left = pop_type(&mut state.stack, pc)?;
                if left != right || !matches!(left, Type::Int | Type::Text | Type::Float64) {
                    return Err(format!("invalid operands to add at instruction {pc}"));
                }
                state.stack.push(left);
            }
            Op::Subtract | Op::Multiply => {
                let right = pop_type(&mut state.stack, pc)?;
                let left = pop_type(&mut state.stack, pc)?;
                if left != right || !matches!(left, Type::Int | Type::Float64) {
                    return Err(format!(
                        "arithmetic requires matching numeric types at instruction {pc}"
                    ));
                }
                state.stack.push(left);
            }
            Op::Divide => {
                if pop_type(&mut state.stack, pc)? != Type::Float64
                    || pop_type(&mut state.stack, pc)? != Type::Float64
                {
                    return Err(format!(
                        "Float64 division requires Float64 at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Float64);
            }
            Op::FloatSqrt => {
                if pop_type(&mut state.stack, pc)? != Type::Float64 {
                    return Err(format!("sqrt requires Float64 at instruction {pc}"));
                }
                state.stack.push(Type::Float64);
            }
            Op::ListPush => {
                let value = pop_type(&mut state.stack, pc)?;
                match pop_type(&mut state.stack, pc)? {
                    Type::List(item) if *item == value => state.stack.push(Type::List(item)),
                    _ => {
                        return Err(format!(
                            "list_push requires a matching List item at instruction {pc}"
                        ))
                    }
                }
            }
            Op::ListSet => {
                let value = pop_type(&mut state.stack, pc)?;
                if pop_type(&mut state.stack, pc)? != Type::Int {
                    return Err(format!("list_set index requires Int at instruction {pc}"));
                }
                match pop_type(&mut state.stack, pc)? {
                    Type::List(item) if *item == value => state.stack.push(Type::List(item)),
                    _ => {
                        return Err(format!(
                            "list_set requires a matching List item at instruction {pc}"
                        ))
                    }
                }
            }
            Op::IntegerDivide | Op::FloorDivide | Op::Remainder => {
                if pop_type(&mut state.stack, pc)? != Type::Int
                    || pop_type(&mut state.stack, pc)? != Type::Int
                {
                    return Err(format!("integer division requires Int at instruction {pc}"));
                }
                state.stack.push(Type::Int);
            }
            Op::Negate => {
                let ty = pop_type(&mut state.stack, pc)?;
                if !matches!(ty, Type::Int | Type::Float64) {
                    return Err(format!("negation requires Int at instruction {pc}"));
                }
                state.stack.push(ty);
            }
            Op::Not => {
                if pop_type(&mut state.stack, pc)? != Type::Bool {
                    return Err(format!("not requires Bool at instruction {pc}"));
                }
                state.stack.push(Type::Bool);
            }
            Op::Equal | Op::NotEqual => {
                let right = pop_type(&mut state.stack, pc)?;
                let left = pop_type(&mut state.stack, pc)?;
                if left != right {
                    return Err(format!(
                        "equality operands have different types at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Bool);
            }
            Op::Less | Op::LessEqual | Op::Greater | Op::GreaterEqual => {
                let right = pop_type(&mut state.stack, pc)?;
                let left = pop_type(&mut state.stack, pc)?;
                if left != right || !matches!(left, Type::Int | Type::Text | Type::Float64) {
                    return Err(format!("invalid comparison operands at instruction {pc}"));
                }
                state.stack.push(Type::Bool);
            }
            Op::Length => {
                if !matches!(
                    pop_type(&mut state.stack, pc)?,
                    Type::List(_) | Type::Map(_) | Type::Bytes | Type::Text
                ) {
                    return Err(format!(
                        "len requires List, Map, Bytes, or Text at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Int);
            }
            Op::TextChars => {
                if pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!("text_chars requires Text at instruction {pc}"));
                }
                state.stack.push(Type::List(Box::new(Type::Text)));
            }
            Op::BytesFromInts => {
                if pop_type(&mut state.stack, pc)? != Type::List(Box::new(Type::Int)) {
                    return Err(format!(
                        "bytes_from_ints requires List[Int] at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Bytes);
            }
            Op::BytesFromText => {
                if pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!("bytes_from_text requires Text at instruction {pc}"));
                }
                state.stack.push(Type::Bytes);
            }
            Op::BytesJoin => {
                if pop_type(&mut state.stack, pc)? != Type::List(Box::new(Type::Bytes)) {
                    return Err(format!(
                        "bytes_join requires List[Bytes] at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Bytes);
            }
            Op::Float64Bits => {
                if pop_type(&mut state.stack, pc)? != Type::Float64 {
                    return Err(format!("float64_bits requires Float64 at instruction {pc}"));
                }
                state.stack.push(Type::Int);
            }
            Op::Index => {
                let index = pop_type(&mut state.stack, pc)?;
                match pop_type(&mut state.stack, pc)? {
                    Type::List(item) if index == Type::Int => state.stack.push(*item),
                    Type::Map(item) if index == Type::Text => state.stack.push(*item),
                    Type::Text if index == Type::Int => state.stack.push(Type::Text),
                    _ => {
                        return Err(format!(
                        "index requires List[Int], Map[Text, T], or Text[Int] at instruction {pc}"
                    ))
                    }
                }
            }
            Op::MakeList(count) => {
                let mut item_type = None;
                for _ in 0..*count {
                    let ty = pop_type(&mut state.stack, pc)?;
                    if item_type.as_ref().is_some_and(|expected| expected != &ty) {
                        return Err(format!(
                            "list items have different types at instruction {pc}"
                        ));
                    }
                    item_type = Some(ty);
                }
                state
                    .stack
                    .push(Type::List(Box::new(item_type.unwrap_or(Type::Unit))));
            }
            Op::MakeMap(count) => {
                let mut item_type = None;
                for _ in 0..*count {
                    let value_type = pop_type(&mut state.stack, pc)?;
                    if pop_type(&mut state.stack, pc)? != Type::Text {
                        return Err(format!("map key requires Text at instruction {pc}"));
                    }
                    if item_type
                        .as_ref()
                        .is_some_and(|expected| expected != &value_type)
                    {
                        return Err(format!(
                            "map values have different types at instruction {pc}"
                        ));
                    }
                    item_type = Some(value_type);
                }
                state
                    .stack
                    .push(Type::Map(Box::new(item_type.unwrap_or(Type::Unit))));
            }
            Op::MakeTypedList(item) => {
                state.stack.push(Type::List(Box::new(item.clone())));
            }
            Op::MakeTypedMap(item) => {
                state.stack.push(Type::Map(Box::new(item.clone())));
            }
            Op::MakeRecord(record_index, count) => {
                let record = artifact.records.get(*record_index).ok_or_else(|| {
                    format!("record constructor references missing record at instruction {pc}")
                })?;
                if count != &record.fields.len() {
                    return Err(format!(
                        "record constructor has invalid field count at instruction {pc}"
                    ));
                }
                for field in record.fields.iter().rev() {
                    if pop_type(&mut state.stack, pc)? != field.ty {
                        return Err(format!(
                            "record constructor field type mismatch at instruction {pc}"
                        ));
                    }
                }
                state.stack.push(Type::Record(record.name.clone()));
            }
            Op::GetField(record_index, field_index) => {
                let record = artifact.records.get(*record_index).ok_or_else(|| {
                    format!("field access references missing record at instruction {pc}")
                })?;
                let field = record.fields.get(*field_index).ok_or_else(|| {
                    format!("field access references missing field at instruction {pc}")
                })?;
                if pop_type(&mut state.stack, pc)? != Type::Record(record.name.clone()) {
                    return Err(format!(
                        "field access record type mismatch at instruction {pc}"
                    ));
                }
                state.stack.push(field.ty.clone());
            }
            Op::MapKeys => {
                if !matches!(pop_type(&mut state.stack, pc)?, Type::Map(_)) {
                    return Err(format!("map keys requires Map at instruction {pc}"));
                }
                state.stack.push(Type::List(Box::new(Type::Text)));
            }
            Op::MapSet => {
                let value_type = pop_type(&mut state.stack, pc)?;
                if pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!("map_set key requires Text at instruction {pc}"));
                }
                let Type::Map(item_type) = pop_type(&mut state.stack, pc)? else {
                    return Err(format!("map_set requires Map at instruction {pc}"));
                };
                if value_type != *item_type {
                    return Err(format!(
                        "map_set value has the wrong type at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Map(item_type));
            }
            Op::MapHas => {
                if pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!("map_has key requires Text at instruction {pc}"));
                }
                if !matches!(pop_type(&mut state.stack, pc)?, Type::Map(_)) {
                    return Err(format!("map_has requires Map at instruction {pc}"));
                }
                state.stack.push(Type::Bool);
            }
            Op::Call(target, argc) => {
                let callee = artifact.functions.get(*target).ok_or_else(|| {
                    format!("call references missing function at instruction {pc}")
                })?;
                if argc != &callee.parameters.len() {
                    return Err(format!("call argument count mismatch at instruction {pc}"));
                }
                for expected in callee.parameters.iter().rev() {
                    if &pop_type(&mut state.stack, pc)? != expected {
                        return Err(format!("call argument type mismatch at instruction {pc}"));
                    }
                }
                state.stack.push(callee.result.clone());
            }
            Op::PythonCall(target, argc) => {
                let external = artifact.externals.get(*target).ok_or_else(|| {
                    format!("Python call references missing declaration at instruction {pc}")
                })?;
                if argc != &external.parameters.len() {
                    return Err(format!(
                        "Python call argument count mismatch at instruction {pc}"
                    ));
                }
                for expected in external.parameters.iter().rev() {
                    if &pop_type(&mut state.stack, pc)? != expected {
                        return Err(format!(
                            "Python call argument type mismatch at instruction {pc}"
                        ));
                    }
                }
                state.stack.push(external.result.clone());
            }
            Op::PythonCallKw(target, positional_count, keyword_names) => {
                let external = artifact.externals.get(*target).ok_or_else(|| {
                    format!("Python call references missing declaration at instruction {pc}")
                })?;
                if *positional_count > external.parameters.len()
                    || positional_count + keyword_names.len() > external.parameters.len()
                {
                    return Err(format!(
                        "Python keyword call has too many arguments at instruction {pc}"
                    ));
                }
                let mut supplied = HashSet::new();
                for name in keyword_names {
                    let index = external
                        .parameter_names
                        .iter()
                        .position(|parameter| parameter == name)
                        .ok_or_else(|| {
                            format!("Python call uses an unknown keyword at instruction {pc}")
                        })?;
                    if index < *positional_count || !supplied.insert(index) {
                        return Err(format!(
                            "Python call has a duplicate keyword at instruction {pc}"
                        ));
                    }
                }
                for name in keyword_names.iter().rev() {
                    let index = external
                        .parameter_names
                        .iter()
                        .position(|parameter| parameter == name)
                        .unwrap();
                    if pop_type(&mut state.stack, pc)? != external.parameters[index] {
                        return Err(format!(
                            "Python keyword argument type mismatch at instruction {pc}"
                        ));
                    }
                }
                for expected in external.parameters[..*positional_count].iter().rev() {
                    if pop_type(&mut state.stack, pc)? != *expected {
                        return Err(format!(
                            "Python positional argument type mismatch at instruction {pc}"
                        ));
                    }
                }
                state.stack.push(external.result.clone());
            }
            Op::ParallelMap(target) => {
                let callee = artifact.functions.get(*target).ok_or_else(|| {
                    format!("parallel_map references missing function at instruction {pc}")
                })?;
                if callee.parameters.len() != 1 {
                    return Err(format!(
                        "parallel worker must take one argument at instruction {pc}"
                    ));
                }
                if type_contains_native_resource(&callee.parameters[0])
                    || type_contains_native_resource(&callee.result)
                {
                    return Err(format!("parallel workers cannot accept or return native resources at instruction {pc}"));
                }
                if pop_type(&mut state.stack, pc)? != Type::Int {
                    return Err(format!(
                        "parallel worker limit must be Int at instruction {pc}"
                    ));
                }
                let Type::List(item) = pop_type(&mut state.stack, pc)? else {
                    return Err(format!("parallel input must be List at instruction {pc}"));
                };
                if *item != callee.parameters[0] {
                    return Err(format!(
                        "parallel input type differs from worker parameter at instruction {pc}"
                    ));
                }
                if type_contains_native_resource(&item) {
                    return Err(format!(
                        "parallel inputs cannot contain native resources at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::List(Box::new(callee.result.clone()))),
                    Box::new(Type::TaskError),
                ));
            }
            Op::SortBy(target) => {
                let comparator = artifact.functions.get(*target).ok_or_else(|| {
                    format!("sort_by references missing comparator at instruction {pc}")
                })?;
                if comparator.parameters.len() != 2
                    || comparator.parameters[0] != comparator.parameters[1]
                    || comparator.result != Type::Int
                {
                    return Err(format!(
                        "sort comparator must have type (T, T) -> Int at instruction {pc}"
                    ));
                }
                let Type::List(item) = pop_type(&mut state.stack, pc)? else {
                    return Err(format!("sort_by input must be a List at instruction {pc}"));
                };
                if *item != comparator.parameters[0] {
                    return Err(format!(
                        "sort comparator parameter type differs from list item at instruction {pc}"
                    ));
                }
                state.stack.push(Type::List(item));
            }
            Op::TextCaseFold | Op::TextCollapseWhitespace => {
                if pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!(
                        "Unicode text operation requires Text at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Text);
            }
            Op::TextNormalize => {
                if pop_type(&mut state.stack, pc)? != Type::Text
                    || pop_type(&mut state.stack, pc)? != Type::Text
                {
                    return Err(format!(
                        "text_normalize requires Text value and form at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Text),
                    Box::new(Type::Record("StdError".into())),
                ));
            }
            Op::CryptoHmacSha256 => {
                if pop_type(&mut state.stack, pc)? != Type::Bytes
                    || pop_type(&mut state.stack, pc)? != Type::Bytes
                {
                    return Err(format!(
                        "crypto_hmac_sha256 requires key and body Bytes at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Bytes);
            }
            Op::CryptoVerifyHmacSha256 => {
                if pop_type(&mut state.stack, pc)? != Type::Bytes
                    || pop_type(&mut state.stack, pc)? != Type::Bytes
                    || pop_type(&mut state.stack, pc)? != Type::Bytes
                {
                    return Err(format!(
                        "crypto_verify_hmac_sha256 requires three Bytes values at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Bool);
            }
            Op::SqliteOpen => {
                if pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!(
                        "sqlite_open requires Text path at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::NativeResource("Db".into())),
                    Box::new(Type::Record("StdError".into())),
                ));
            }
            Op::SqliteBegin => {
                if pop_type(&mut state.stack, pc)? != Type::NativeResource("Db".into()) {
                    return Err(format!("sqlite_begin requires Db at instruction {pc}"));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::NativeResource("Transaction".into())),
                    Box::new(Type::Record("StdError".into())),
                ));
            }
            Op::SqliteExecute | Op::SqliteQuery => {
                if pop_type(&mut state.stack, pc)?
                    != Type::List(Box::new(Type::Record("SqlValue".into())))
                    || pop_type(&mut state.stack, pc)? != Type::Text
                    || pop_type(&mut state.stack, pc)? != Type::NativeResource("Transaction".into())
                {
                    return Err(format!("SQLite execution requires Transaction, Text SQL, and List[SqlValue] at instruction {pc}"));
                }
                let result = if instruction.op == Op::SqliteExecute {
                    Type::Int
                } else {
                    Type::List(Box::new(Type::Record("SqlRow".into())))
                };
                state.stack.push(Type::Result(
                    Box::new(result),
                    Box::new(Type::Record("StdError".into())),
                ));
            }
            Op::SqliteCommit | Op::SqliteRollback => {
                if pop_type(&mut state.stack, pc)? != Type::NativeResource("Transaction".into()) {
                    return Err(format!(
                        "SQLite transaction finalizer requires Transaction at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Unit),
                    Box::new(Type::Record("StdError".into())),
                ));
            }
            Op::SqliteClose => {
                if pop_type(&mut state.stack, pc)? != Type::NativeResource("Db".into()) {
                    return Err(format!("sqlite_close requires Db at instruction {pc}"));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Unit),
                    Box::new(Type::Record("StdError".into())),
                ));
            }
            Op::TomlParse => {
                if pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!("toml_parse requires Text at instruction {pc}"));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::NativeResource("TomlDocument".into())),
                    Box::new(Type::Record("StdError".into())),
                ));
            }
            Op::TomlHas | Op::TomlGetText | Op::TomlGetInt | Op::TomlGetBool => {
                if pop_type(&mut state.stack, pc)? != Type::List(Box::new(Type::Text))
                    || pop_type(&mut state.stack, pc)?
                        != Type::NativeResource("TomlDocument".into())
                {
                    return Err(format!(
                        "TOML lookup requires TomlDocument and List[Text] path at instruction {pc}"
                    ));
                }
                if instruction.op == Op::TomlHas {
                    state.stack.push(Type::Bool);
                } else {
                    let result = match instruction.op {
                        Op::TomlGetText => Type::Text,
                        Op::TomlGetInt => Type::Int,
                        Op::TomlGetBool => Type::Bool,
                        _ => unreachable!(),
                    };
                    state.stack.push(Type::Result(
                        Box::new(result),
                        Box::new(Type::Record("StdError".into())),
                    ));
                }
            }
            Op::TomlPutText | Op::TomlPutInt | Op::TomlPutBool => {
                let value_type = match instruction.op {
                    Op::TomlPutText => Type::Text,
                    Op::TomlPutInt => Type::Int,
                    Op::TomlPutBool => Type::Bool,
                    _ => unreachable!(),
                };
                if pop_type(&mut state.stack, pc)? != value_type
                    || pop_type(&mut state.stack, pc)? != Type::List(Box::new(Type::Text))
                    || pop_type(&mut state.stack, pc)?
                        != Type::NativeResource("TomlDocument".into())
                {
                    return Err(format!("TOML edit requires matching document, path, and value types at instruction {pc}"));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Unit),
                    Box::new(Type::Record("StdError".into())),
                ));
            }
            Op::TomlRender => {
                if pop_type(&mut state.stack, pc)? != Type::NativeResource("TomlDocument".into()) {
                    return Err(format!(
                        "toml_render requires TomlDocument at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Text);
            }
            Op::CalendarParse => {
                if pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!("calendar_parse requires Text at instruction {pc}"));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::List(Box::new(Type::Record("CalendarEvent".into())))),
                    Box::new(Type::Record("StdError".into())),
                ));
            }
            Op::ReadText => {
                if pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!("read_text path must be Text at instruction {pc}"));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Text),
                    Box::new(Type::FileError),
                ));
            }
            Op::HttpGet => {
                if pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!("http_get URL must be Text at instruction {pc}"));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Text),
                    Box::new(Type::HttpError),
                ));
            }
            Op::HttpRequest => {
                if pop_type(&mut state.stack, pc)? != Type::Text
                    || pop_type(&mut state.stack, pc)? != Type::Map(Box::new(Type::Text))
                    || pop_type(&mut state.stack, pc)? != Type::Text
                    || pop_type(&mut state.stack, pc)? != Type::Text
                {
                    return Err(format!("http_request requires Text method and URL, Map[Text] headers, and Text body at instruction {pc}"));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Json),
                    Box::new(Type::HttpError),
                ));
            }
            Op::HttpRequestWithTimeout => {
                if pop_type(&mut state.stack, pc)? != Type::Int
                    || pop_type(&mut state.stack, pc)? != Type::Text
                    || pop_type(&mut state.stack, pc)? != Type::Map(Box::new(Type::Text))
                    || pop_type(&mut state.stack, pc)? != Type::Text
                    || pop_type(&mut state.stack, pc)? != Type::Text
                {
                    return Err(format!("http_request_with_timeout requires Text method and URL, Map[Text] headers, Text body, and Int timeout_ms at instruction {pc}"));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Json),
                    Box::new(Type::HttpError),
                ));
            }
            Op::HttpRequestBytes => {
                if pop_type(&mut state.stack, pc)? != Type::Int
                    || pop_type(&mut state.stack, pc)? != Type::Bytes
                    || pop_type(&mut state.stack, pc)? != Type::Map(Box::new(Type::Text))
                    || pop_type(&mut state.stack, pc)? != Type::Text
                    || pop_type(&mut state.stack, pc)? != Type::Text
                {
                    return Err(format!("http_request_bytes requires Text method and URL, Map[Text] headers, Bytes body, and Int timeout_ms at instruction {pc}"));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Record("HttpResponse".into())),
                    Box::new(Type::HttpError),
                ));
            }
            Op::WriteText => {
                if pop_type(&mut state.stack, pc)? != Type::Text
                    || pop_type(&mut state.stack, pc)? != Type::Text
                {
                    return Err(format!(
                        "write_text requires Text path and content at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Unit),
                    Box::new(Type::FileError),
                ));
            }
            Op::WriteTextAtomic => {
                if pop_type(&mut state.stack, pc)? != Type::Text
                    || pop_type(&mut state.stack, pc)? != Type::Text
                {
                    return Err(format!(
                        "write_text_atomic requires Text path and content at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Unit),
                    Box::new(Type::FileError),
                ));
            }
            Op::ReadBytes => {
                if pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!("read_bytes path must be Text at instruction {pc}"));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Bytes),
                    Box::new(Type::FileError),
                ));
            }
            Op::FileOpenLines | Op::CsvOpen => {
                if pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!("file reader path must be Text at instruction {pc}"));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::NativeResource("FileReader".into())),
                    Box::new(Type::FileError),
                ));
            }
            Op::FileNextLine | Op::CsvNext | Op::FileClose => {
                if pop_type(&mut state.stack, pc)? != Type::NativeResource("FileReader".into()) {
                    return Err(format!(
                        "file reader operation requires FileReader at instruction {pc}"
                    ));
                }
                let result = match instruction.op {
                    Op::FileNextLine => Type::Option(Box::new(Type::Record("SourceLine".into()))),
                    Op::CsvNext => Type::Option(Box::new(Type::Record("CsvRow".into()))),
                    Op::FileClose => Type::Unit,
                    _ => unreachable!(),
                };
                state
                    .stack
                    .push(Type::Result(Box::new(result), Box::new(Type::FileError)));
            }
            Op::CsvEncode => {
                if pop_type(&mut state.stack, pc)?
                    != Type::List(Box::new(Type::List(Box::new(Type::Text))))
                {
                    return Err(format!(
                        "csv_encode requires List[List[Text]] at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Text),
                    Box::new(Type::FileError),
                ));
            }
            Op::TimeParseDate | Op::TimeParseTimestamp | Op::SemVerParse => {
                if pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!("data parser requires Text at instruction {pc}"));
                }
                let record = match instruction.op {
                    Op::TimeParseDate => "Date",
                    Op::TimeParseTimestamp => "Instant",
                    Op::SemVerParse => "SemVer",
                    _ => unreachable!(),
                };
                state.stack.push(Type::Result(
                    Box::new(Type::Record(record.into())),
                    Box::new(Type::FileError),
                ));
            }
            Op::TimeFormatUtc => {
                if pop_type(&mut state.stack, pc)? != Type::Record("Instant".into()) {
                    return Err(format!(
                        "time_format_utc requires Instant at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Text);
            }
            Op::UnixTimeMs => state.stack.push(Type::Int),
            Op::TimeTodayLocal => state.stack.push(Type::Record("Date".into())),
            Op::SemVerCompare => {
                if pop_type(&mut state.stack, pc)? != Type::Record("SemVer".into())
                    || pop_type(&mut state.stack, pc)? != Type::Record("SemVer".into())
                {
                    return Err(format!(
                        "semver_compare requires two SemVer records at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Int);
            }
            Op::CliParse => {
                if pop_type(&mut state.stack, pc)?
                    != Type::List(Box::new(Type::Record("CliOption".into())))
                    || pop_type(&mut state.stack, pc)? != Type::List(Box::new(Type::Text))
                {
                    return Err(format!(
                        "cli_parse requires List[Text] and List[CliOption] at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Record("CliArgs".into())),
                    Box::new(Type::FileError),
                ));
            }
            Op::FsWalk => {
                if pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!("fs_walk requires Text at instruction {pc}"));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::List(Box::new(Type::Record("DirectoryEntry".into())))),
                    Box::new(Type::FileError),
                ));
            }
            Op::ListGetOption => {
                if pop_type(&mut state.stack, pc)? != Type::Int {
                    return Err(format!("list_get index must be Int at instruction {pc}"));
                }
                let Type::List(item) = pop_type(&mut state.stack, pc)? else {
                    return Err(format!("list_get requires List at instruction {pc}"));
                };
                state.stack.push(Type::Option(item));
            }
            Op::MapGetOption => {
                if pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!("map_get key must be Text at instruction {pc}"));
                }
                let Type::Map(item) = pop_type(&mut state.stack, pc)? else {
                    return Err(format!("map_get requires Map at instruction {pc}"));
                };
                state.stack.push(Type::Option(item));
            }
            Op::TextParseInt | Op::TextParseFloat => {
                if pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!(
                        "text numeric parser requires Text at instruction {pc}"
                    ));
                }
                let parsed = if instruction.op == Op::TextParseInt {
                    Type::Int
                } else {
                    Type::Float64
                };
                state
                    .stack
                    .push(Type::Result(Box::new(parsed), Box::new(Type::FileError)));
            }
            Op::WriteBytes => {
                if pop_type(&mut state.stack, pc)? != Type::Bytes
                    || pop_type(&mut state.stack, pc)? != Type::Text
                {
                    return Err(format!(
                        "write_bytes requires Text path and Bytes content at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Unit),
                    Box::new(Type::FileError),
                ));
            }
            Op::Args => state.stack.push(Type::List(Box::new(Type::Text))),
            Op::ArgsFrom => {
                if pop_type(&mut state.stack, pc)? != Type::Int {
                    return Err(format!("args_from requires Int at instruction {pc}"));
                }
                state.stack.push(Type::List(Box::new(Type::Text)));
            }
            Op::Environment => {
                if pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!("env name must be Text at instruction {pc}"));
                }
                state.stack.push(Type::Option(Box::new(Type::Text)));
            }
            Op::Jump(target) => successors.push(*target),
            Op::JumpIfFalse(target) => {
                if pop_type(&mut state.stack, pc)? != Type::Bool {
                    return Err(format!(
                        "conditional jump requires Bool at instruction {pc}"
                    ));
                }
                successors.push(*target);
                successors.push(pc + 1);
            }
            Op::Return => {
                let result = pop_type(&mut state.stack, pc)?;
                if result != function.result || !state.stack.is_empty() {
                    return Err(format!(
                        "return type or stack height is invalid at instruction {pc}: expected {:?}, got {:?}, stack {:?}",
                        function.result,
                        result,
                        state.stack
                    ));
                }
                has_return = true;
            }
        }
        if !matches!(
            instruction.op,
            Op::Jump(_) | Op::JumpIfFalse(_) | Op::Return
        ) {
            successors.push(pc + 1);
        }
        for next in successors {
            if next >= function.code.len() {
                return Err(format!(
                    "control flow falls off the end at instruction {pc}"
                ));
            }
            match &mut states[next] {
                None => {
                    states[next] = Some(state.clone());
                    pending.push_back(next);
                }
                Some(previous) => {
                    if previous.stack != state.stack {
                        return Err(format!("inconsistent stack types at instruction {next}"));
                    }
                    let mut changed = false;
                    for (old, new) in previous.locals.iter_mut().zip(&state.locals) {
                        if old != new && old.is_some() {
                            *old = None;
                            changed = true;
                        }
                    }
                    if changed {
                        pending.push_back(next);
                    }
                }
            }
        }
    }
    if !has_return {
        return Err("no reachable return".into());
    }
    Ok(())
}
