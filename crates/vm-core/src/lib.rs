//! Portable core of the Guion production bytecode VM.
//!
//! This crate owns the instruction loop, frame stack, value semantics, call-depth and
//! instruction budgets, and the artifact verifier. Everything that touches the outside
//! world is requested through [`VmHost`]; `guion-runtime` implements it with the native
//! filesystem, database, HTTP, calendar, and process services, and [`PortableHost`]
//! implements it for environments such as WebAssembly by rejecting those operations.

use guion_model::{
    Artifact, BigInt, CodecFunction, FileErrorValue, Float64, FsFunction, HttpErrorValue,
    JsonErrorValue, MathFunction, NativeResourceValue, Op, RegexFunction, RuntimeError,
    ShareAcrossThreads, Shared, SharedJson, SharedText, Span, TaskErrorValue, TextMap, Type, Value,
};
use std::{
    cell::Cell,
    collections::{BTreeMap, HashSet, VecDeque},
    fmt::Write as _,
    mem::size_of,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::Instant,
};

mod register;

pub use register::{build_register_programs, RegisterProgram, RegisterPrograms};

/// Byte offsets of every `TEXT_INDEX_STRIDE`th scalar of the text most recently searched,
/// so repeated `text_find` calls over one long text seek in constant time instead of
/// rescanning from the start. The cache holds the text alive, so its address cannot be reused.
struct TextScalarIndex {
    text: SharedText,
    ascii: bool,
    scalars: usize,
    checkpoints: Vec<usize>,
    /// The scalar and byte position of the last match, so a search that moves forward
    /// from it walks only the scalars in between.
    cursor: (usize, usize),
    /// The searcher for the needle most recently looked for in this text.
    finder: Option<memchr::memmem::Finder<'static>>,
}

const TEXT_INDEX_STRIDE: usize = 64;
/// Room reserved when appending to a text local first makes it growable.
const TEXT_APPEND_MIN_CAPACITY: usize = 32;
/// Texts shorter than this are scanned directly; indexing them would cost more than it saves.
const TEXT_INDEX_MIN_BYTES: usize = 4096;

thread_local! {
    static TEXT_INDEX_CACHE: std::cell::RefCell<Option<TextScalarIndex>> =
        const { std::cell::RefCell::new(None) };
}

impl TextScalarIndex {
    fn new(text: &SharedText) -> Self {
        let ascii = text.is_ascii();
        let mut checkpoints = Vec::new();
        let mut scalars = text.len();
        if !ascii {
            scalars = 0;
            for (byte_index, _) in text.char_indices() {
                if scalars.is_multiple_of(TEXT_INDEX_STRIDE) {
                    checkpoints.push(byte_index);
                }
                scalars += 1;
            }
        }
        Self {
            text: SharedText::clone(text),
            ascii,
            scalars,
            checkpoints,
            cursor: (0, 0),
            finder: None,
        }
    }

    fn byte_offset(&self, scalar: usize) -> Option<usize> {
        if scalar > self.scalars {
            return None;
        }
        if self.ascii || scalar == self.scalars {
            return Some(if self.ascii { scalar } else { self.text.len() });
        }
        let (mut base_scalar, mut base) = (
            scalar - scalar % TEXT_INDEX_STRIDE,
            self.checkpoints[scalar / TEXT_INDEX_STRIDE],
        );
        if (base_scalar..=scalar).contains(&self.cursor.0) {
            (base_scalar, base) = self.cursor;
        }
        self.text[base..]
            .char_indices()
            .nth(scalar - base_scalar)
            .map(|(offset, _)| base + offset)
    }

    fn find(&mut self, needle: &str, from: usize) -> Option<Option<usize>> {
        let byte_start = self.byte_offset(from)?;
        if self
            .finder
            .as_ref()
            .is_none_or(|finder| finder.needle() != needle.as_bytes())
        {
            self.finder = Some(memchr::memmem::Finder::new(needle).into_owned());
        }
        let finder = self.finder.as_ref().expect("finder was just stored");
        let Some(offset) = finder.find(&self.text.as_bytes()[byte_start..]) else {
            return Some(None);
        };
        let found = if self.ascii {
            from + offset
        } else {
            from + self.text[byte_start..byte_start + offset].chars().count()
        };
        self.cursor = (found, byte_start + offset);
        Some(Some(found))
    }
}

/// Finds `needle` in `text` at or after the Unicode scalar index `from`.
/// Returns `None` when `from` is past the end, `Some(None)` when there is no
/// match, and the scalar index of the first match otherwise.
pub fn text_find_scalar(text: &SharedText, needle: &str, from: usize) -> Option<Option<usize>> {
    if text.len() >= TEXT_INDEX_MIN_BYTES {
        return TEXT_INDEX_CACHE.with(|cache| {
            let mut cache = cache.borrow_mut();
            if !cache
                .as_ref()
                .is_some_and(|index| SharedText::ptr_eq(&index.text, text))
            {
                // The cache outlives the program on this thread and keeps a reference to the
                // text, which other threads may hold too, so it is counted atomically.
                text.share_across_threads();
                *cache = Some(TextScalarIndex::new(text));
            }
            cache
                .as_mut()
                .expect("text scalar index was just stored")
                .find(needle, from)
        });
    }
    let ascii = text.char_count() == text.len();
    let byte_start = if ascii || from == 0 {
        (from <= text.len()).then_some(from)?
    } else {
        match text.char_indices().nth(from) {
            Some((byte_index, _)) => byte_index,
            None if text.char_count() == from => text.len(),
            None => return None,
        }
    };
    Some(
        memchr::memmem::find(&text.as_bytes()[byte_start..], needle.as_bytes()).map(|offset| {
            if ascii {
                from + offset
            } else {
                from + text[byte_start..byte_start + offset].chars().count()
            }
        }),
    )
}

/// The Unicode scalar at `index` as its own text, or `None` past the end. ASCII text is
/// indexed by byte, and ASCII scalars share one immortal text each.
pub fn text_scalar_at(text: &SharedText, index: usize) -> Option<SharedText> {
    let scalar = if text.char_count() == text.len() {
        text.as_bytes().get(index).map(|&byte| char::from(byte))
    } else {
        text.chars().nth(index)
    };
    scalar.map(shared_text_scalar)
}

/// The byte range of the scalars `start..end` (with `start <= end`), or `None` when `end` is
/// past the end. ASCII text is indexed by byte.
pub fn text_scalar_range(text: &SharedText, start: usize, end: usize) -> Option<(usize, usize)> {
    let length = text.char_count();
    if end > length {
        return None;
    }
    if length == text.len() {
        return Some((start, end));
    }
    let byte_at = |scalar: usize| {
        text.char_indices()
            .nth(scalar)
            .map_or(text.len(), |(byte, _)| byte)
    };
    let byte_start = byte_at(start);
    let byte_end = byte_start
        + text[byte_start..]
            .char_indices()
            .nth(end - start)
            .map_or(text.len() - byte_start, |(byte, _)| byte);
    Some((byte_start, byte_end))
}

/// Byte offsets of the non-overlapping matches of a non-empty `needle`, left to right, or
/// `None` when there are more than `limit` of them.
pub fn text_match_offsets(text: &str, needle: &str, limit: usize) -> Option<Vec<usize>> {
    let mut offsets = Vec::new();
    for offset in memchr::memmem::find_iter(text.as_bytes(), needle.as_bytes()) {
        if offsets.len() == limit {
            return None;
        }
        offsets.push(offset);
    }
    Some(offsets)
}

/// The fields between the delimiter matches at `offsets`, as `text.split(delimiter)` would.
pub fn text_split_at(text: &str, offsets: &[usize], delimiter_len: usize) -> Vec<Value> {
    let mut fields = Vec::with_capacity(offsets.len() + 1);
    let mut start = 0;
    for &offset in offsets {
        fields.push(Value::Text(SharedText::from(&text[start..offset])));
        start = offset + delimiter_len;
    }
    fields.push(Value::Text(SharedText::from(&text[start..])));
    fields
}

/// The number of non-overlapping matches of a non-empty `needle`.
pub fn text_match_count(text: &str, needle: &str) -> usize {
    memchr::memmem::find_iter(text.as_bytes(), needle.as_bytes()).count()
}

/// `text` with every match of a non-empty `needle` replaced, as `text.replace(needle,
/// replacement)` would; `total` is the result length in bytes.
pub fn text_replace_all(text: &str, needle: &str, replacement: &str, total: usize) -> SharedText {
    let mut replaced = String::with_capacity(total);
    let mut start = 0;
    for offset in memchr::memmem::find_iter(text.as_bytes(), needle.as_bytes()) {
        replaced.push_str(&text[start..offset]);
        replaced.push_str(replacement);
        start = offset + needle.len();
    }
    replaced.push_str(&text[start..]);
    SharedText::from(replaced)
}

/// Joins a `List[Text]` value, or returns `None` when an item is not text or the result
/// would exceed the item or text-size ceilings. Callers report the specific error.
pub fn text_join_checked(
    values: &[Value],
    delimiter: &str,
    max_collection_items: usize,
    max_text_bytes: usize,
) -> Option<String> {
    if values.len() > max_collection_items {
        return None;
    }
    let mut total = delimiter
        .len()
        .checked_mul(values.len().saturating_sub(1))?;
    for value in values {
        let Value::Text(text) = value else {
            return None;
        };
        total = total.checked_add(text.len())?;
    }
    if total > max_text_bytes {
        return None;
    }
    let mut joined = String::with_capacity(total);
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            joined.push_str(delimiter);
        }
        if let Value::Text(text) = value {
            joined.push_str(text);
        }
    }
    Some(joined)
}

/// The guest allocation charged for a `List[Text]` of `count` items whose text totals
/// `text_bytes`. Native code charges the same amount when it stores such a list.
pub fn text_list_allocation(count: usize, text_bytes: usize) -> usize {
    size_of::<Vec<Value>>()
        .saturating_add(count.saturating_mul(size_of::<Value>() + size_of::<String>()))
        .saturating_add(text_bytes)
}

fn shared_text_scalar(scalar: char) -> SharedText {
    if !scalar.is_ascii() {
        return SharedText::from(scalar.to_string());
    }

    static ASCII_SCALARS: OnceLock<[OnceLock<SharedText>; 128]> = OnceLock::new();
    let scalars = ASCII_SCALARS.get_or_init(|| std::array::from_fn(|_| OnceLock::new()));
    SharedText::clone(
        scalars[scalar as usize]
            .get_or_init(|| SharedText::immortal(scalar.encode_utf8(&mut [0; 4]))),
    )
}

/// Size and depth limits the shared VM enforces while executing guest code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VmLimits {
    pub allocation_limit_bytes: usize,
    pub call_depth_limit: usize,
    pub output_limit_bytes: usize,
    pub max_integer_digits: usize,
    pub max_text_bytes: usize,
    pub max_collection_items: usize,
    pub max_file_bytes: usize,
}

impl Default for VmLimits {
    fn default() -> Self {
        Self {
            allocation_limit_bytes: DEFAULT_ALLOCATION_LIMIT_BYTES,
            call_depth_limit: 128,
            output_limit_bytes: 1_000_000,
            max_integer_digits: 4096,
            max_text_bytes: 1_000_000,
            max_collection_items: 100_000,
            max_file_bytes: 1_000_000,
        }
    }
}

pub const DEFAULT_ALLOCATION_LIMIT_BYTES: usize = 256 * 1024 * 1024;

/// Which program output stream a `print` instruction writes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputStream {
    Stdout,
    Stderr,
}

/// Mutable view of the executing frame handed to [`VmHost::host_op`].
pub struct HostOpContext<'a> {
    pub instruction: &'a guion_model::Instruction,
    pub function: &'a guion_model::Function,
    pub stack: &'a mut Vec<Value>,
    pub locals: &'a mut Vec<Value>,
    pub budget: &'a Arc<Budget>,
    pub depth: usize,
}

/// Services the shared VM requests from whatever environment embeds it.
pub trait VmHost {
    fn artifact(&self) -> &Arc<Artifact>;
    /// Register form of the functions that run. Hosts that do not build it return `None`.
    fn register_programs(&self) -> Option<Arc<RegisterPrograms>> {
        None
    }
    fn limits(&self) -> VmLimits;
    /// Extra cancellation signal checked before every instruction.
    fn externally_cancelled(&self) -> bool {
        false
    }
    /// Bytes already written to program stdout and stderr in this invocation.
    fn program_output_bytes(&self) -> usize;
    fn write_program_output(
        &mut self,
        stream: OutputStream,
        text: &str,
        span: Span,
    ) -> Result<(), RuntimeError>;
    /// Begin a call. Hosts may run the callee natively and return its value directly.
    fn start_frame(
        &mut self,
        index: usize,
        args: Vec<Value>,
        depth: usize,
        budget: &Arc<Budget>,
        call_site: Option<Span>,
    ) -> Result<FrameStart, RuntimeError>;
    /// Execute an instruction the portable core does not implement itself. The host must
    /// either perform it (adjusting the stack as the instruction requires) or return an error.
    fn host_op(&mut self, context: HostOpContext<'_>) -> Result<(), RuntimeError>;
}

/// Build the interpreter frame for a call after the caller has checked depth limits.
#[inline]
pub fn new_vm_frame(
    artifact: &Artifact,
    index: usize,
    args: Vec<Value>,
    depth: usize,
    call_site: Option<Span>,
) -> VmFrame {
    let f = &artifact.functions[index];
    let mut locals = args;
    locals.resize_with(f.local_count, || Value::Unit);
    VmFrame {
        function_index: index,
        depth,
        call_site,
        locals,
        stack: Vec::new(),
        ip: 0,
    }
}

/// Reject a call that would exceed the configured depth.
#[inline]
pub fn check_call_depth(
    artifact: &Artifact,
    limits: &VmLimits,
    index: usize,
    depth: usize,
) -> Result<(), RuntimeError> {
    if depth >= limits.call_depth_limit {
        return Err(RuntimeError::new(
            "R_CALL_DEPTH",
            format!("call depth exceeds {}", limits.call_depth_limit),
            artifact.functions[index]
                .code
                .first()
                .map(|i| i.span)
                .unwrap_or_default(),
        ));
    }
    Ok(())
}

/// Drive a frame and all of its callees to completion.
pub fn run_frames<H: VmHost>(
    host: &mut H,
    initial: VmFrame,
    budget: Arc<Budget>,
) -> Result<Value, RuntimeError> {
    run_frames_with_stack(host, vec![initial], budget)
}

/// Drive an already active VM call stack to completion.
pub fn run_frames_with_stack<H: VmHost>(
    host: &mut H,
    frames: Vec<VmFrame>,
    budget: Arc<Budget>,
) -> Result<Value, RuntimeError> {
    if budget.runtime_profile.is_some() {
        run_frames_with_stack_profiled::<true, _>(host, frames, budget)
    } else {
        run_frames_with_stack_profiled::<false, _>(host, frames, budget)
    }
}

fn run_frames_with_stack_profiled<const PROFILED: bool, H: VmHost>(
    host: &mut H,
    mut frames: Vec<VmFrame>,
    budget: Arc<Budget>,
) -> Result<Value, RuntimeError> {
    assert!(!frames.is_empty(), "guest frame stack must not be empty");
    let fast_host_forwarders = host
        .artifact()
        .functions
        .iter()
        .map(fast_host_forwarder)
        .collect::<Vec<_>>();
    let artifact = Arc::clone(host.artifact());
    let register_programs = if PROFILED {
        None
    } else {
        host.register_programs()
    };
    let shared = VmRunShared {
        artifact: &artifact,
        limits: host.limits(),
        registers: register_programs.as_deref(),
        fast_host_forwarders: &fast_host_forwarders,
    };
    let mut pool = VmVecPool::default();
    loop {
        let frame = frames.pop().expect("guest frame stack is non-empty");
        let current_call_site = frame.call_site;
        match execute_vm_frame_with_forwarders::<PROFILED, _>(
            host, frame, &budget, &shared, &mut pool,
        ) {
            Err(mut error) => {
                append_guest_call_trace(&mut error, current_call_site, &frames);
                return Err(error);
            }
            Ok(VmStep::Return(value)) => {
                if let Some(parent) = frames.last_mut() {
                    parent.stack.push(value);
                } else {
                    return Ok(value);
                }
            }
            Ok(VmStep::Call {
                mut frame,
                target,
                args,
                span,
            }) => match host.start_frame(
                target,
                args,
                frame.depth.saturating_add(1),
                &budget,
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
                Ok(FrameStart::VmFrames(children)) => {
                    frames.push(frame);
                    frames.extend(children);
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

/// Run one frame until it returns or calls another function.
pub fn execute_vm_frame<H: VmHost>(
    host: &mut H,
    frame: VmFrame,
    budget: Arc<Budget>,
) -> Result<VmStep, RuntimeError> {
    let fast_host_forwarders = host
        .artifact()
        .functions
        .iter()
        .map(fast_host_forwarder)
        .collect::<Vec<_>>();
    let artifact = Arc::clone(host.artifact());
    let register_programs = host.register_programs();
    let profiled = budget.runtime_profile.is_some();
    let shared = VmRunShared {
        artifact: &artifact,
        limits: host.limits(),
        registers: if profiled {
            None
        } else {
            register_programs.as_deref()
        },
        fast_host_forwarders: &fast_host_forwarders,
    };
    let mut pool = VmVecPool::default();
    if profiled {
        execute_vm_frame_with_forwarders::<true, _>(host, frame, &budget, &shared, &mut pool)
    } else {
        execute_vm_frame_with_forwarders::<false, _>(host, frame, &budget, &shared, &mut pool)
    }
}

#[derive(Clone, Copy)]
enum LoadFastPath<'a> {
    Probe(bool),
    Unwrap(&'a Value),
    RecordField(&'a Value),
    ListIndex(&'a Value),
}

#[derive(Clone, Copy)]
struct TextCharsConcatFastPath {
    result_consumer_ip: usize,
    required_characters: usize,
}

fn text_chars_concat_fast_path(
    code: &[guion_model::Instruction],
    text_chars_ip: usize,
) -> Option<TextCharsConcatFastPath> {
    if !matches!(
        code.get(text_chars_ip).map(|instruction| &instruction.op),
        Some(Op::TextChars)
    ) {
        return None;
    }
    let Some(Op::Store(characters_slot)) = code
        .get(text_chars_ip + 1)
        .map(|instruction| &instruction.op)
    else {
        return None;
    };

    let mut cursor = text_chars_ip + 2;
    let mut required_characters = 0usize;
    loop {
        if matches!(
            code.get(cursor).map(|instruction| &instruction.op),
            Some(Op::Store(_) | Op::Print)
        ) {
            break;
        }
        if !matches!(
            code.get(cursor).map(|instruction| &instruction.op),
            Some(Op::Load(slot)) if slot == characters_slot
        ) {
            return None;
        }
        cursor += 1;
        if !matches!(
            code.get(cursor).map(|instruction| &instruction.op),
            Some(Op::Const(Value::Int(index)))
                if index.to_usize() == Some(required_characters)
        ) {
            return None;
        }
        cursor += 1;
        if !matches!(
            code.get(cursor).map(|instruction| &instruction.op),
            Some(Op::Index)
        ) {
            return None;
        }
        cursor += 1;
        if required_characters > 0 {
            if !matches!(
                code.get(cursor).map(|instruction| &instruction.op),
                Some(Op::Add)
            ) {
                return None;
            }
            cursor += 1;
        }
        required_characters += 1;
        if required_characters > 64 {
            return None;
        }
    }

    if required_characters < 2 || cursor >= code.len() {
        return None;
    }
    Some(TextCharsConcatFastPath {
        result_consumer_ip: cursor,
        required_characters,
    })
}

/// Per-run state that every frame entry reads, resolved once so calls and returns do
/// not touch reference counts or rebuild it.
struct VmRunShared<'r> {
    artifact: &'r Arc<Artifact>,
    limits: VmLimits,
    registers: Option<&'r RegisterPrograms>,
    fast_host_forwarders: &'r [Option<FastHostForwarder>],
}

/// Emptied frame vectors kept for the next call, so a call does not allocate.
#[derive(Default)]
struct VmVecPool {
    free: Vec<Vec<Value>>,
}

const VM_VEC_POOL_ENTRIES: usize = 64;
const VM_VEC_POOL_MAX_CAPACITY: usize = 256;

impl VmVecPool {
    #[inline]
    fn take(&mut self, capacity: usize) -> Vec<Value> {
        match self.free.pop() {
            Some(mut values) => {
                values.reserve(capacity);
                values
            }
            None => Vec::with_capacity(capacity),
        }
    }

    #[inline]
    fn give(&mut self, mut values: Vec<Value>) {
        if self.free.len() < VM_VEC_POOL_ENTRIES
            && values.capacity() > 0
            && values.capacity() <= VM_VEC_POOL_MAX_CAPACITY
        {
            values.clear();
            self.free.push(values);
        }
    }
}

fn execute_vm_frame_with_forwarders<const PROFILED: bool, H: VmHost>(
    host: &mut H,
    frame: VmFrame,
    budget: &Arc<Budget>,
    shared: &VmRunShared<'_>,
    pool: &mut VmVecPool,
) -> Result<VmStep, RuntimeError> {
    let VmFrame {
        function_index,
        depth,
        call_site,
        mut locals,
        mut stack,
        mut ip,
    } = frame;
    let artifact = shared.artifact;
    let limits = &shared.limits;
    let fast_host_forwarders = shared.fast_host_forwarders;
    let f = &artifact.functions[function_index];
    if stack.capacity() == 0 {
        stack = pool.take(0);
    }
    let profile_sample = if PROFILED {
        budget
            .runtime_profile
            .as_ref()
            .filter(|profile| profile.should_sample_vm_frame())
            .map(|profile| VmFrameProfileTimer::new(Arc::clone(profile), &f.name))
    } else {
        None
    };
    let mut instruction_fuel = InstructionFuel::new(budget);
    let mut previous_profiled_instruction: Option<(usize, &'static str)> = None;
    // Parallel workers charge fuel exactly, one atomic at a time, which this form does not; they
    // stay on the stack interpreter.
    let registers = shared
        .registers
        .filter(|_| instruction_fuel.reserve_locally)
        .and_then(|programs| programs.get(function_index, f))
        .filter(|program| program.usable_under(limits));
    // The stack interpreter takes at least one instruction after this form hands back.
    let mut stay_on_stack = false;
    // Cancellation is polled on frame entry, on every taken backward jump and after every host
    // operation: code between two polls is straight-line, so it stops soon after a cancellation.
    let mut poll_cancellation = true;
    loop {
        if poll_cancellation {
            poll_cancellation = false;
            if budget.is_cancelled() || host.externally_cancelled() {
                return Err(RuntimeError::new(
                    "R_CANCELLED",
                    "parallel task was cancelled after a sibling failed",
                    f.code.first().map(|i| i.span).unwrap_or_default(),
                ));
            }
        }
        if let Some(program) = registers {
            if !stay_on_stack {
                if let Some(pc) = program.entry(ip) {
                    instruction_fuel.top_up();
                    let exit = register::run_registers(
                        program,
                        pc as usize,
                        register::RegisterRun {
                            host: &mut *host,
                            artifact,
                            function: f,
                            locals: &mut locals,
                            stack: &mut stack,
                            fuel: &mut instruction_fuel,
                            budget,
                            limits,
                            depth,
                        },
                    )?;
                    ip = exit.ip;
                    poll_cancellation = exit.poll_cancellation;
                    stay_on_stack = true;
                    continue;
                }
            }
            stay_on_stack = false;
        }
        if ip >= f.code.len() {
            return Err(RuntimeError::new(
                "R_NO_RETURN",
                format!("function `{}` reached the end without returning", f.name),
                f.code.last().map(|i| i.span).unwrap_or_default(),
            ));
        }
        if !instruction_fuel.consume() {
            return Err(RuntimeError::new(
                "R_STEP_LIMIT",
                "instruction budget exhausted",
                f.code[ip].span,
            ));
        }
        if PROFILED {
            if let Some(profile) = &budget.runtime_profile {
                if profile.detailed_counters_enabled() {
                    let current_opcode = opcode_profile_name(&f.code[ip].op);
                    profile.record_opcode_execution(current_opcode);
                    if let Some((previous_ip, previous_opcode)) = previous_profiled_instruction {
                        if previous_ip.checked_add(1) == Some(ip) {
                            profile.record_opcode_transition(
                                &f.name,
                                previous_opcode,
                                current_opcode,
                            );
                        }
                    }
                    previous_profiled_instruction = Some((ip, current_opcode));
                    if let Op::Call(target, _) = &f.code[ip].op {
                        if let Some(callee) = artifact.functions.get(*target) {
                            profile.record_function_call(&callee.name);
                        }
                    }
                }
                if profile.detailed_counters_enabled() {
                    profile
                        .vm_interpreter_instructions
                        .fetch_add(1, Ordering::Relaxed);
                }
                if let Some(sample) = &profile_sample {
                    sample.record_instruction();
                }
            }
        }
        let instruction_ref = &f.code[ip];
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
                        instruction_ref.span,
                    )
                })?
            };
        }
        // Both operands inline: `$compute` yields the result, or None to take the general
        // path. Inline operands own nothing and an inline result needs no digit check
        // or allocation charge, so the slower handler is skipped.
        macro_rules! small_int_binary {
            ($compute:expr) => {
                if let [.., Value::Int(left), Value::Int(right)] = stack.as_slice() {
                    if let (Some(left), Some(right)) = (left.small(), right.small()) {
                        let compute: fn(i64, i64) -> Option<Value> = $compute;
                        if limits.max_integer_digits >= 19 {
                            if let Some(result) = compute(left, right) {
                                std::mem::forget(stack.pop());
                                std::mem::forget(stack.pop());
                                stack.push(result);
                                continue;
                            }
                        }
                    }
                }
            };
        }
        macro_rules! record_skipped_instruction {
            ($instruction_ip:expr, $skipped:expr) => {{
                if PROFILED {
                    if let Some(profile) = &budget.runtime_profile {
                        if profile.detailed_counters_enabled() {
                            let current_opcode = opcode_profile_name(&$skipped.op);
                            profile.record_opcode_execution(current_opcode);
                            if let Some((previous_ip, previous_opcode)) =
                                previous_profiled_instruction
                            {
                                if previous_ip.checked_add(1) == Some($instruction_ip) {
                                    profile.record_opcode_transition(
                                        &f.name,
                                        previous_opcode,
                                        current_opcode,
                                    );
                                }
                            }
                            previous_profiled_instruction = Some(($instruction_ip, current_opcode));
                            if let Op::Call(target, _) = &$skipped.op {
                                if let Some(callee) = artifact.functions.get(*target) {
                                    profile.record_function_call(&callee.name);
                                }
                            }
                            profile
                                .vm_interpreter_instructions
                                .fetch_add(1, Ordering::Relaxed);
                        }
                        if let Some(sample) = &profile_sample {
                            sample.record_instruction();
                        }
                    }
                }
            }};
        }
        let instruction = instruction_ref;
        match &instruction.op {
            Op::Load(slot) => {
                let value = &locals[*slot];
                let next_op = f.code.get(ip);
                let fast_path = next_op.and_then(|next| match (&next.op, value) {
                    (Op::IsOk, Value::ResultOk(_)) => Some(LoadFastPath::Probe(true)),
                    (Op::IsOk, Value::ResultErr(_)) => Some(LoadFastPath::Probe(false)),
                    (Op::IsSome, Value::OptionSome(_)) => Some(LoadFastPath::Probe(true)),
                    (Op::IsSome, Value::OptionNone) => Some(LoadFastPath::Probe(false)),
                    (Op::Unwrap, Value::ResultOk(payload) | Value::OptionSome(payload)) => {
                        Some(LoadFastPath::Unwrap(payload.as_ref()))
                    }
                    (Op::GetField(record_index, field_index), Value::Record(name, fields)) => {
                        let record = artifact.records.get(*record_index)?;
                        if name.as_str() != record.name.as_str() {
                            return None;
                        }
                        let expected_field = record.fields.get(*field_index)?;
                        let field = fields.get(*field_index)?;
                        value_matches(field, &expected_field.ty, &artifact.records)
                            .then_some(LoadFastPath::RecordField(field))
                    }
                    (Op::Const(Value::Int(index)), Value::List(items)) => {
                        if index.digit_count() > limits.max_integer_digits {
                            return None;
                        }
                        let index_ip = ip.checked_add(1)?;
                        if !matches!(&f.code.get(index_ip)?.op, Op::Index) {
                            return None;
                        }
                        let index = index.to_usize()?;
                        let field = items.get(index)?;
                        Some(LoadFastPath::ListIndex(field))
                    }
                    _ => None,
                });
                if let (Some(next), Some(fast_path)) = (next_op, fast_path) {
                    charge_allocation(
                        budget.as_ref(),
                        cloned_value_allocation(value),
                        instruction_ref.span,
                    )?;
                    if !instruction_fuel.consume() {
                        return Err(RuntimeError::new(
                            "R_STEP_LIMIT",
                            "instruction budget exhausted",
                            next.span,
                        ));
                    }
                    record_skipped_instruction!(ip, next);
                    if let LoadFastPath::ListIndex(field) = fast_path {
                        let index_ip = ip + 1;
                        let index_instruction = &f.code[index_ip];
                        if !instruction_fuel.consume() {
                            return Err(RuntimeError::new(
                                "R_STEP_LIMIT",
                                "instruction budget exhausted",
                                index_instruction.span,
                            ));
                        }
                        record_skipped_instruction!(index_ip, index_instruction);
                        charge_allocation(
                            budget.as_ref(),
                            cloned_value_allocation(field),
                            index_instruction.span,
                        )?;
                        if PROFILED {
                            if let Some(profile) = &budget.runtime_profile {
                                profile
                                    .vm_list_index_load_fast_paths
                                    .fetch_add(1, Ordering::Relaxed);
                            }
                        }
                        ip += 2;
                        push_stack!(field.clone());
                        continue;
                    }
                    if let LoadFastPath::RecordField(field) = fast_path {
                        // Preserve both logical clone charges while skipping the temporary record
                        // name clone and stack value for this adjacent Load -> GetField pair.
                        charge_allocation(
                            budget.as_ref(),
                            cloned_value_allocation(field),
                            next.span,
                        )?;
                    }
                    if PROFILED {
                        if let Some(profile) = &budget.runtime_profile {
                            match fast_path {
                                LoadFastPath::Probe(_) => {
                                    profile
                                        .vm_result_option_probe_fast_paths
                                        .fetch_add(1, Ordering::Relaxed);
                                }
                                LoadFastPath::Unwrap(_) => {
                                    profile
                                        .vm_result_option_unwrap_fast_paths
                                        .fetch_add(1, Ordering::Relaxed);
                                }
                                LoadFastPath::RecordField(_) => {
                                    profile
                                        .vm_record_field_load_fast_paths
                                        .fetch_add(1, Ordering::Relaxed);
                                }
                                LoadFastPath::ListIndex(_) => unreachable!(
                                    "list index fast path is handled before one-op paths"
                                ),
                            }
                        }
                    }
                    let result = match fast_path {
                        LoadFastPath::Probe(value) => Value::Bool(value),
                        LoadFastPath::Unwrap(value) => value.clone(),
                        LoadFastPath::RecordField(value) => value.clone(),
                        LoadFastPath::ListIndex(_) => {
                            unreachable!("list index fast path is handled before one-op paths")
                        }
                    };
                    ip += 1;
                    push_stack!(result);
                    continue;
                }
                charge_allocation(
                    budget.as_ref(),
                    cloned_value_allocation_fast(value),
                    instruction_ref.span,
                )?;
                push_stack!(clone_value_fast(value));
                continue;
            }
            Op::Store(slot) => {
                let value = stack.pop().ok_or_else(|| {
                    RuntimeError::new(
                        "R_STACK_INVALID",
                        "verified bytecode stack underflow",
                        instruction_ref.span,
                    )
                })?;
                drop_value_fast(std::mem::replace(&mut locals[*slot], value));
                continue;
            }
            Op::Const(v) => {
                if let Some(error) = value_limit_error_at_boundary(v, limits, instruction.span) {
                    return Err(error);
                }
                charge_allocation(
                    budget.as_ref(),
                    cloned_value_allocation_fast(v),
                    instruction.span,
                )?;
                push_stack!(clone_value_fast(v))
            }
            Op::Pop => {
                pop!();
            }
            Op::Add => {
                small_int_binary!(|a, b| a.checked_add(b).map(|v| Value::Int(BigInt::from_i64(v))));
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
                    (Value::Text(mut x), Value::Text(y)) => {
                        if x.len().saturating_add(y.len()) > limits.max_text_bytes {
                            return Err(RuntimeError::new(
                                "R_TEXT_LIMIT",
                                "text value exceeds the configured byte limit",
                                instruction.span,
                            ));
                        }
                        // `set s = s + piece` stores straight back into the local it read.
                        // Releasing the local's reference first lets the append extend the
                        // text in place, so building text this way stays linear. Other
                        // concatenations produce ordinary single-allocation text.
                        let appending = match f.code.get(ip).map(|next| &next.op) {
                            Some(Op::Store(slot)) => matches!(
                                &locals[*slot],
                                Value::Text(local) if SharedText::ptr_eq(local, &x)
                            )
                            .then_some(*slot),
                            _ => None,
                        };
                        if let Some(slot) = appending {
                            drop_value_fast(std::mem::replace(&mut locals[slot], Value::Unit));
                            // The budget counts the appended bytes, as a fresh concatenation
                            // counts its length, so it does not depend on how text grows.
                            if SharedText::try_append(&mut x, &y) {
                                charge_allocation(budget.as_ref(), y.len(), instruction.span)?;
                                push_stack!(Value::Text(x));
                                continue;
                            }
                        }
                        Value::Text(if appending.is_some() {
                            SharedText::concat(&x, &y, TEXT_APPEND_MIN_CAPACITY, true)
                        } else {
                            SharedText::concat(&x, &y, 0, false)
                        })
                    }
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "invalid operands for addition",
                            instruction.span,
                        ));
                    }
                };
                if let Some(error) = value_limit_error(&v, limits, instruction.span) {
                    return Err(error);
                }
                charge_allocation(budget.as_ref(), new_value_allocation(&v), instruction.span)?;
                push_stack!(v);
            }
            Op::Subtract => {
                small_int_binary!(|a, b| a.checked_sub(b).map(|v| Value::Int(BigInt::from_i64(v))));
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
                        ));
                    }
                };
                if let Some(error) = value_limit_error(&v, limits, instruction.span) {
                    return Err(error);
                }
                charge_allocation(budget.as_ref(), new_value_allocation(&v), instruction.span)?;
                push_stack!(v);
            }
            Op::Multiply => {
                small_int_binary!(|a, b| a.checked_mul(b).map(|v| Value::Int(BigInt::from_i64(v))));
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
                        ));
                    }
                };
                if let Some(error) = value_limit_error(&v, limits, instruction.span) {
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
                match instruction.op {
                    Op::IntegerDivide => small_int_binary!(|a, b| {
                        a.checked_div(b).map(|q| Value::Int(BigInt::from_i64(q)))
                    }),
                    Op::FloorDivide => small_int_binary!(|a, b| {
                        let (q, r) = (a.checked_div(b)?, a.checked_rem(b)?);
                        let adjust = r != 0 && (a < 0) != (b < 0);
                        Some(Value::Int(BigInt::from_i64(if adjust { q - 1 } else { q })))
                    }),
                    _ => small_int_binary!(|a, b| {
                        let r = a.checked_rem(b)?;
                        let adjust = r != 0 && (a < 0) != (b < 0);
                        Some(Value::Int(BigInt::from_i64(if adjust { r + b } else { r })))
                    }),
                }
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
                if let Some(error) = value_limit_error(&value, limits, instruction.span) {
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
                        ));
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
                if *count > limits.max_collection_items {
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
                let mut values = Vec::with_capacity(*count);
                for _ in 0..*count {
                    values.push(pop!());
                }
                values.reverse();
                push_stack!(Value::from_list_items(values));
            }
            Op::MakeTypedList(Type::Float64) => {
                charge_allocation(budget.as_ref(), size_of::<Vec<Float64>>(), instruction.span)?;
                push_stack!(Value::Float64List(Shared::new(Vec::new())));
            }
            Op::MakeTypedList(_) => {
                charge_allocation(budget.as_ref(), size_of::<Vec<Value>>(), instruction.span)?;
                push_stack!(Value::List(Shared::new(Vec::new())));
            }
            Op::MakeMap(count) => {
                if *count > limits.max_collection_items {
                    return Err(RuntimeError::new(
                        "R_COLLECTION_LIMIT",
                        "map exceeds the configured item limit",
                        instruction.span,
                    ));
                }
                charge_allocation(
                    budget.as_ref(),
                    TextMap::ACCOUNTED_HEADER_BYTES
                        .saturating_add(count.saturating_mul(size_of::<(String, Value)>())),
                    instruction.span,
                )?;
                let mut values = TextMap::new();
                let mut entries = Vec::with_capacity(*count);
                for _ in 0..*count {
                    let value = pop!();
                    let key = match pop!() {
                        Value::Text(key) => key,
                        _ => {
                            return Err(RuntimeError::new(
                                "R_TYPE_INVALID",
                                "map key must be Text",
                                instruction.span,
                            ));
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
                    if values.insert(SharedText::clone(&key), value).is_some() {
                        return Err(RuntimeError::new(
                            "R_MAP_DUPLICATE_KEY",
                            format!("duplicate map key `{key}`"),
                            instruction.span,
                        ));
                    }
                }
                let map = Value::Map(Shared::new(values));
                if let Some(error) = value_limit_error(&map, limits, instruction.span) {
                    return Err(error);
                }
                push_stack!(map);
            }
            Op::MakeTypedMap(_) => {
                charge_allocation(
                    budget.as_ref(),
                    TextMap::ACCOUNTED_HEADER_BYTES,
                    instruction.span,
                )?;
                push_stack!(Value::Map(Shared::new(TextMap::new())));
            }
            Op::MakeRecord(record_index, count) => {
                let record = host.artifact().records.get(*record_index).ok_or_else(|| {
                    RuntimeError::new(
                        "R_ARTIFACT_INVALID",
                        "record constructor references a missing definition",
                        instruction.span,
                    )
                })?;
                if *count != record.fields.len() {
                    return Err(RuntimeError::new(
                        "R_ARTIFACT_INVALID",
                        "record constructor has an invalid field count",
                        instruction.span,
                    ));
                }
                let record_bytes = size_of::<Vec<Value>>()
                    .saturating_add(count.saturating_mul(size_of::<Value>()))
                    .saturating_add(record.name.len());
                charge_allocation(budget.as_ref(), record_bytes, instruction.span)?;
                budget.record_record_charge(record_bytes);
                let mut fields = Vec::with_capacity(*count);
                for _ in 0..*count {
                    fields.push(pop!());
                }
                fields.reverse();
                push_stack!(Value::Record(record.name.clone(), Shared::new(fields)));
            }
            Op::GetField(record_index, field_index) => {
                let record = host.artifact().records.get(*record_index).ok_or_else(|| {
                    RuntimeError::new(
                        "R_ARTIFACT_INVALID",
                        "field access references a missing record definition",
                        instruction.span,
                    )
                })?;
                let Some(expected_field) = record.fields.get(*field_index) else {
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
                if name.as_str() != record.name.as_str() {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "field access record type does not match",
                        instruction.span,
                    ));
                }
                let value = fields.get(*field_index).cloned().ok_or_else(|| {
                    RuntimeError::new(
                        "R_TYPE_INVALID",
                        "record value has the wrong number of fields",
                        instruction.span,
                    )
                })?;
                if !value_matches(&value, &expected_field.ty, &host.artifact().records) {
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
                    let allocation = values.iter_unordered().fold(
                        size_of::<Vec<Value>>()
                            .saturating_add(values.len().saturating_mul(size_of::<Value>())),
                        |total, (key, _)| total.saturating_add(size_of::<String>() + key.len()),
                    );
                    charge_allocation(budget.as_ref(), allocation, instruction.span)?;
                    let keys = values
                        .keys()
                        .map(|key| Value::Text(SharedText::clone(key)))
                        .collect::<Vec<_>>();
                    push_stack!(Value::List(Shared::new(keys)));
                }
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "map keys requires a Map",
                        instruction.span,
                    ));
                }
            },
            Op::MapRecordGet(record_index, field_index, nested) => {
                let nested_key = if *nested {
                    Some(match pop!() {
                        Value::Text(key) => key,
                        _ => {
                            return Err(RuntimeError::new(
                                "R_TYPE_INVALID",
                                "map_record_get nested key must be Text",
                                instruction.span,
                            ));
                        }
                    })
                } else {
                    None
                };
                let key = match pop!() {
                    Value::Text(key) => key,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "map_record_get key must be Text",
                            instruction.span,
                        ));
                    }
                };
                let values = match pop!() {
                    Value::Map(values) => values,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "map_record_get first argument must be a Map",
                            instruction.span,
                        ));
                    }
                };
                let record = artifact.records.get(*record_index).ok_or_else(|| {
                    RuntimeError::new(
                        "R_TYPE_INVALID",
                        "map_record_get references a missing record type",
                        instruction.span,
                    )
                })?;
                let field = record.fields.get(*field_index).ok_or_else(|| {
                    RuntimeError::new(
                        "R_TYPE_INVALID",
                        "map_record_get references a missing record field",
                        instruction.span,
                    )
                })?;
                let entry = match (values.get(key.as_ref()), nested_key.as_deref()) {
                    (Some(Value::Record(name, fields)), None) => Some((name, fields)),
                    (Some(Value::Map(inner)), Some(inner_key)) => match inner.get(inner_key) {
                        Some(Value::Record(name, fields)) => Some((name, fields)),
                        _ => None,
                    },
                    _ => None,
                };
                let Some((record_name, fields)) = entry else {
                    return Err(RuntimeError::new(
                        "R_MAP_KEY",
                        format!("map_record_get key `{key}` is missing or is not a record"),
                        instruction.span,
                    ));
                };
                if record_name.as_str() != record.name.as_str() {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "map_record_get record type does not match the declared Map value",
                        instruction.span,
                    ));
                }
                let value = fields.get(*field_index).cloned().ok_or_else(|| {
                    RuntimeError::new(
                        "R_TYPE_INVALID",
                        "map_record_get record is missing a declared field",
                        instruction.span,
                    )
                })?;
                if !value_matches(&value, &field.ty, &artifact.records) {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "map_record_get field value does not match its declared type",
                        instruction.span,
                    ));
                }
                charge_allocation(
                    budget.as_ref(),
                    cloned_value_allocation(&value),
                    instruction.span,
                )?;
                if PROFILED {
                    if let Some(profile) = &budget.runtime_profile {
                        profile.record_map_record_get();
                    }
                }
                push_stack!(value);
            }
            Op::MapRecordSet(record_index, field_index, nested) => {
                let value = pop!();
                let nested_key = if *nested {
                    Some(match pop!() {
                        Value::Text(key) => key,
                        _ => {
                            return Err(RuntimeError::new(
                                "R_TYPE_INVALID",
                                "map_record_set nested key must be Text",
                                instruction.span,
                            ));
                        }
                    })
                } else {
                    None
                };
                let key = match pop!() {
                    Value::Text(key) => key,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "map_record_set key must be Text",
                            instruction.span,
                        ));
                    }
                };
                let values = match pop!() {
                    Value::Map(values) => values,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "map_record_set first argument must be a Map",
                            instruction.span,
                        ));
                    }
                };
                let record = artifact.records.get(*record_index).ok_or_else(|| {
                    RuntimeError::new(
                        "R_TYPE_INVALID",
                        "map_record_set references a missing record type",
                        instruction.span,
                    )
                })?;
                let field = record.fields.get(*field_index).ok_or_else(|| {
                    RuntimeError::new(
                        "R_TYPE_INVALID",
                        "map_record_set references a missing record field",
                        instruction.span,
                    )
                })?;
                if let Some(error) = value_limit_error(&value, limits, instruction.span) {
                    return Err(error);
                }
                let entry = match (values.get(key.as_ref()), nested_key.as_deref()) {
                    (Some(Value::Record(name, fields)), None) => Some((name, fields)),
                    (Some(Value::Map(inner)), Some(inner_key)) => match inner.get(inner_key) {
                        Some(Value::Record(name, fields)) => Some((name, fields)),
                        _ => None,
                    },
                    _ => None,
                };
                let Some((record_name, fields)) = entry else {
                    return Err(RuntimeError::new(
                        "R_MAP_KEY",
                        format!("map_record_set key `{key}` is missing or is not a record"),
                        instruction.span,
                    ));
                };
                if record_name.as_str() != record.name.as_str()
                    || fields.get(*field_index).is_none()
                {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "map_record_set record does not match its declared type",
                        instruction.span,
                    ));
                }
                if !value_matches(&value, &field.ty, &artifact.records) {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "map_record_set value does not match the record field type",
                        instruction.span,
                    ));
                }
                let local_store_slot = match f.code.get(ip).map(|next| &next.op) {
                    Some(Op::Store(slot))
                        if locals.get(*slot).is_some_and(|local| {
                            matches!(local, Value::Map(current) if Shared::ptr_eq(current, &values))
                        }) =>
                    {
                        Some(*slot)
                    }
                    _ => None,
                };
                if let Some(slot) = local_store_slot {
                    drop(values);
                    let local = locals.get_mut(slot).ok_or_else(|| {
                        RuntimeError::new(
                            "R_LOCAL_UNINITIALIZED",
                            "map_record_set assignment targets an uninitialized local",
                            instruction.span,
                        )
                    })?;
                    let Value::Map(items) = local else {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "map_record_set assignment target must be a Map",
                            instruction.span,
                        ));
                    };
                    let map_shared = Shared::strong_count(items) > 1;
                    let mut map_copy_bytes = if map_shared {
                        map_clone_allocation(items.as_ref(), None, &Value::Unit)
                    } else {
                        0
                    };
                    let (record_fields_shared, record_field_count) = if let Some(inner_key) =
                        nested_key.as_deref()
                    {
                        let Some(Value::Map(inner)) = items.get(key.as_ref()) else {
                            return Err(RuntimeError::new(
                                "R_MAP_KEY",
                                format!("map_record_set key `{key}` is missing or is not a Map"),
                                instruction.span,
                            ));
                        };
                        let Some(Value::Record(_, fields)) = inner.get(inner_key) else {
                            return Err(RuntimeError::new(
                                "R_MAP_KEY",
                                format!("map_record_set nested key `{inner_key}` is missing or is not a record"),
                                instruction.span,
                            ));
                        };
                        let inner_shared = Shared::strong_count(inner) > 1;
                        let fields_shared = Shared::strong_count(fields) > 1;
                        let inner_will_copy = map_shared || inner_shared;
                        if inner_will_copy {
                            map_copy_bytes = map_copy_bytes.saturating_add(map_clone_allocation(
                                inner.as_ref(),
                                None,
                                &Value::Unit,
                            ));
                        }
                        (map_shared || inner_will_copy || fields_shared, fields.len())
                    } else {
                        let Some(Value::Record(_, fields)) = items.get(key.as_ref()) else {
                            return Err(RuntimeError::new(
                                "R_MAP_KEY",
                                format!("map_record_set key `{key}` is missing or is not a record"),
                                instruction.span,
                            ));
                        };
                        (map_shared || Shared::strong_count(fields) > 1, fields.len())
                    };
                    let record_copy_bytes = if record_fields_shared {
                        size_of::<Vec<Value>>()
                            .saturating_add(record_field_count.saturating_mul(size_of::<Value>()))
                    } else {
                        0
                    };
                    charge_allocation(
                        budget.as_ref(),
                        map_copy_bytes.saturating_add(record_copy_bytes),
                        instruction.span,
                    )?;
                    let map = Shared::make_mut(items);
                    let fields = if let Some(inner_key) = nested_key.as_deref() {
                        let Some(Value::Map(inner)) = map.get_mut(key.as_ref()) else {
                            unreachable!("map_record_set outer map was checked before mutation")
                        };
                        let inner = Shared::make_mut(inner);
                        let Some(Value::Record(_, fields)) = inner.get_mut(inner_key) else {
                            unreachable!("map_record_set inner map was checked before mutation")
                        };
                        fields
                    } else {
                        let Some(Value::Record(_, fields)) = map.get_mut(key.as_ref()) else {
                            unreachable!("map_record_set target was checked before mutation")
                        };
                        fields
                    };
                    Shared::make_mut(fields)[*field_index] = value;
                    if PROFILED {
                        if let Some(profile) = &budget.runtime_profile {
                            profile.record_map_record_set(map_copy_bytes, record_copy_bytes);
                        }
                    }
                    let updated = Value::Map(Shared::clone(items));
                    push_stack!(updated);
                    continue;
                }
                let mut map_copy_bytes = map_clone_allocation(values.as_ref(), None, &Value::Unit);
                let mut record_fields = None;
                if let Some(inner_key) = nested_key.as_deref() {
                    let Some(Value::Map(inner)) = values.get(key.as_ref()) else {
                        unreachable!("map_record_set target was checked before mutation")
                    };
                    let Some(Value::Record(_, fields)) = inner.get(inner_key) else {
                        unreachable!("map_record_set target was checked before mutation")
                    };
                    map_copy_bytes = map_copy_bytes.saturating_add(map_clone_allocation(
                        inner.as_ref(),
                        None,
                        &Value::Unit,
                    ));
                    record_fields = Some(fields.len());
                } else if let Some(Value::Record(_, fields)) = values.get(key.as_ref()) {
                    record_fields = Some(fields.len());
                }
                let record_copy_bytes = record_fields.map_or(0, |count| {
                    size_of::<Vec<Value>>().saturating_add(count.saturating_mul(size_of::<Value>()))
                });
                charge_allocation(
                    budget.as_ref(),
                    map_copy_bytes.saturating_add(record_copy_bytes),
                    instruction.span,
                )?;
                let mut updated = values.as_ref().clone();
                let fields = if let Some(inner_key) = nested_key.as_deref() {
                    let Some(Value::Map(inner)) = updated.get_mut(key.as_ref()) else {
                        unreachable!("map_record_set cloned outer map must retain nested map")
                    };
                    let inner = Shared::make_mut(inner);
                    let Some(Value::Record(_, fields)) = inner.get_mut(inner_key) else {
                        unreachable!("map_record_set cloned inner map must retain record")
                    };
                    fields
                } else {
                    let Some(Value::Record(_, fields)) = updated.get_mut(key.as_ref()) else {
                        unreachable!("map_record_set cloned map must retain record")
                    };
                    fields
                };
                Shared::make_mut(fields)[*field_index] = value;
                let map = Value::Map(Shared::new(updated));
                if let Some(error) = value_limit_error(&map, limits, instruction.span) {
                    return Err(error);
                }
                if PROFILED {
                    if let Some(profile) = &budget.runtime_profile {
                        profile.record_map_record_set(map_copy_bytes, record_copy_bytes);
                    }
                }
                push_stack!(map);
            }
            Op::MapSet => {
                let value = pop!();
                let key = match pop!() {
                    Value::Text(key) => key,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "map_set key must be Text",
                            instruction.span,
                        ));
                    }
                };
                let values = match pop!() {
                    Value::Map(values) => values,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "map_set first argument must be a Map",
                            instruction.span,
                        ));
                    }
                };
                let local_store_slot = match f.code.get(ip).map(|next| &next.op) {
                        Some(Op::Store(slot))
                            if locals.get(*slot).is_some_and(|local| {
                                matches!(local, Value::Map(current) if Shared::ptr_eq(current, &values))
                            }) =>
                        {
                            Some(*slot)
                        }
                        _ => None,
                    };
                if let Some((child_slot, parent_slot, parent_key)) =
                    local_store_slot.and_then(|child_slot| {
                        unique_pending_nested_map_writeback_target(
                            &f.code, ip, child_slot, &locals, &values,
                        )
                        .map(|(parent_slot, parent_key)| (child_slot, parent_slot, parent_key))
                    })
                {
                    let contains_key = values.contains_key(key.as_ref());
                    if !contains_key && values.len() >= limits.max_collection_items {
                        return Err(RuntimeError::new(
                            "R_COLLECTION_LIMIT",
                            "map exceeds the configured item limit",
                            instruction.span,
                        ));
                    }
                    if key.len() > limits.max_text_bytes {
                        return Err(RuntimeError::new(
                            "R_TEXT_LIMIT",
                            "map key exceeds the configured text limit",
                            instruction.span,
                        ));
                    }
                    let allocation = if contains_key {
                        0
                    } else {
                        size_of::<(String, Value)>().saturating_add(key.len())
                    };
                    charge_allocation(budget.as_ref(), allocation, instruction.span)?;
                    if let Some(error) = value_limit_error(&value, limits, instruction.span) {
                        return Err(error);
                    }

                    // This child update is followed only by the proven parent writeback.
                    // With unique parent/child ownership, updating the nested map now is
                    // observationally equivalent to the two persistent map_set calls.
                    drop(values);
                    let old_child = std::mem::replace(
                        locals.get_mut(child_slot).ok_or_else(|| {
                            RuntimeError::new(
                                "R_LOCAL_UNINITIALIZED",
                                "nested map update targets an uninitialized local",
                                instruction.span,
                            )
                        })?,
                        Value::Unit,
                    );
                    drop(old_child);
                    let updated = {
                        let parent_local = locals.get_mut(parent_slot).ok_or_else(|| {
                            RuntimeError::new(
                                "R_LOCAL_UNINITIALIZED",
                                "nested map parent is uninitialized",
                                instruction.span,
                            )
                        })?;
                        let Value::Map(parent_arc) = parent_local else {
                            return Err(RuntimeError::new(
                                "R_TYPE_INVALID",
                                "nested map update parent must be a Map",
                                instruction.span,
                            ));
                        };
                        let parent_values = Shared::get_mut(parent_arc).expect(
                            "nested writeback parent uniqueness was checked before mutation",
                        );
                        let Some(Value::Map(inner_arc)) =
                            parent_values.get_mut(parent_key.as_ref())
                        else {
                            unreachable!("nested writeback child was checked before mutation")
                        };
                        let inner_values = Shared::get_mut(inner_arc).expect(
                            "nested writeback child uniqueness was checked before mutation",
                        );
                        replace_map_value(inner_values, &key, value);
                        Value::Map(Shared::clone(inner_arc))
                    };
                    locals[child_slot] = updated.clone();
                    stack.push(updated);
                    if PROFILED {
                        if let Some(profile) = &budget.runtime_profile {
                            profile.record_map_set(true, 0);
                        }
                    }
                    continue;
                }
                if let Some((parent_slot, parent_key)) =
                    unique_nested_map_update_target(&f.code, ip, &stack, &locals, &values)
                {
                    let contains_key = values.contains_key(key.as_ref());
                    if !contains_key && values.len() >= limits.max_collection_items {
                        return Err(RuntimeError::new(
                            "R_COLLECTION_LIMIT",
                            "map exceeds the configured item limit",
                            instruction.span,
                        ));
                    }
                    if key.len() > limits.max_text_bytes {
                        return Err(RuntimeError::new(
                            "R_TEXT_LIMIT",
                            "map key exceeds the configured text limit",
                            instruction.span,
                        ));
                    }
                    let allocation = if contains_key {
                        0
                    } else {
                        size_of::<(String, Value)>().saturating_add(key.len())
                    };
                    charge_allocation(budget.as_ref(), allocation, instruction.span)?;
                    if let Some(error) = value_limit_error(&value, limits, instruction.span) {
                        return Err(error);
                    }

                    // The parent map is unique apart from its local and stack handles, and
                    // this child map is unique apart from its parent entry and this value.
                    // The following MapSet/Store writes the child back to that same parent,
                    // so the intermediate child update can preserve value semantics without
                    // cloning the entire nested map.
                    drop(stack.pop()); // parent key; copied above
                    drop(stack.pop()); // parent map; its local is now the unique owner
                    drop(values); // only the parent entry owns the child after this point
                    let parent_local = locals.get_mut(parent_slot).ok_or_else(|| {
                        RuntimeError::new(
                            "R_LOCAL_UNINITIALIZED",
                            "nested map update targets an uninitialized local",
                            instruction.span,
                        )
                    })?;
                    let Value::Map(parent_arc) = parent_local else {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "nested map update parent must be a Map",
                            instruction.span,
                        ));
                    };
                    let nested_result = {
                        let parent_values = Shared::get_mut(parent_arc)
                            .expect("nested update parent uniqueness was checked before mutation");
                        let Some(Value::Map(inner_arc)) =
                            parent_values.get_mut(parent_key.as_ref())
                        else {
                            unreachable!("nested update target was checked before mutation")
                        };
                        let inner_values = Shared::get_mut(inner_arc)
                            .expect("nested update child uniqueness was checked before mutation");
                        replace_map_value(inner_values, &key, value);
                        Value::Map(Shared::clone(inner_arc))
                    };
                    let parent_result = Value::Map(Shared::clone(parent_arc));
                    stack.push(parent_result);
                    stack.push(Value::Text(parent_key));
                    stack.push(nested_result);
                    if PROFILED {
                        if let Some(profile) = &budget.runtime_profile {
                            profile.record_map_set(true, 0);
                        }
                    }
                    continue;
                }
                if let Some(slot) = local_store_slot {
                    // The compiler emits Store immediately after this expression when a
                    // mutable local is assigned its own map_set result. Drop the temporary
                    // load before copy-on-write so a unique local can update in place.
                    drop(values);
                    let local = locals.get_mut(slot).ok_or_else(|| {
                        RuntimeError::new(
                            "R_LOCAL_UNINITIALIZED",
                            "map_set assignment targets an uninitialized local",
                            instruction.span,
                        )
                    })?;
                    let Value::Map(items) = local else {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "map_set assignment target must be a Map",
                            instruction.span,
                        ));
                    };
                    // Updating an existing key of a unique map charges nothing: one lookup is enough.
                    if key.len() <= limits.max_text_bytes {
                        if let Some(existing) =
                            Shared::get_mut(items).and_then(|entries| entries.get_mut(key.as_ref()))
                        {
                            if let Some(error) = value_limit_error(&value, limits, instruction.span)
                            {
                                return Err(error);
                            }
                            *existing = value;
                            if PROFILED {
                                if let Some(profile) = &budget.runtime_profile {
                                    profile.record_map_set(true, 0);
                                }
                            }
                            push_stack!(Value::Map(Shared::clone(items)));
                            continue;
                        }
                    }
                    let contains_key = items.contains_key(key.as_ref());
                    if !contains_key && items.len() >= limits.max_collection_items {
                        return Err(RuntimeError::new(
                            "R_COLLECTION_LIMIT",
                            "map exceeds the configured item limit",
                            instruction.span,
                        ));
                    }
                    if key.len() > limits.max_text_bytes {
                        return Err(RuntimeError::new(
                            "R_TEXT_LIMIT",
                            "map key exceeds the configured text limit",
                            instruction.span,
                        ));
                    }
                    let shared = Shared::strong_count(items) > 1;
                    let allocation = if shared {
                        map_clone_allocation(items.as_ref(), Some(key.as_ref()), &value)
                    } else if contains_key {
                        0
                    } else {
                        size_of::<(String, Value)>().saturating_add(key.len())
                    };
                    charge_allocation(budget.as_ref(), allocation, instruction.span)?;
                    if let Some(error) = value_limit_error(&value, limits, instruction.span) {
                        return Err(error);
                    }
                    replace_map_value(Shared::make_mut(items), &key, value);
                    if PROFILED {
                        if let Some(profile) = &budget.runtime_profile {
                            profile.record_map_set(!shared, if shared { allocation } else { 0 });
                        }
                    }
                    push_stack!(Value::Map(Shared::clone(items)));
                    continue;
                }
                if !values.contains_key(key.as_ref()) && values.len() >= limits.max_collection_items
                {
                    return Err(RuntimeError::new(
                        "R_COLLECTION_LIMIT",
                        "map exceeds the configured item limit",
                        instruction.span,
                    ));
                }
                if key.len() > limits.max_text_bytes {
                    return Err(RuntimeError::new(
                        "R_TEXT_LIMIT",
                        "map key exceeds the configured text limit",
                        instruction.span,
                    ));
                }
                let map_clone_bytes =
                    map_clone_allocation(values.as_ref(), Some(key.as_ref()), &value);
                charge_allocation(budget.as_ref(), map_clone_bytes, instruction.span)?;
                let mut updated = values.as_ref().clone();
                replace_map_value(&mut updated, &key, value);
                if PROFILED {
                    if let Some(profile) = &budget.runtime_profile {
                        profile.record_map_set(false, map_clone_bytes);
                    }
                }
                let map = Value::Map(Shared::new(updated));
                if let Some(error) = value_limit_error(&map, limits, instruction.span) {
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
                        ));
                    }
                };
                let values = match pop!() {
                    Value::Map(values) => values,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "map_has first argument must be a Map",
                            instruction.span,
                        ));
                    }
                };
                push_stack!(Value::Bool(values.contains_key(key.as_ref())));
            }
            Op::Length => {
                let len = match pop!() {
                    Value::List(values) => values.len(),
                    Value::Float64List(values) => values.len(),
                    Value::Map(values) => values.len(),
                    Value::Bytes(values) => values.len(),
                    Value::Text(values) => values.char_count(),
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "len requires a List, Map, Bytes, or Text",
                            instruction.span,
                        ));
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
                        ));
                    }
                };
                let text_chars_ip = ip - 1;
                if let Some(fast_path) = text_chars_concat_fast_path(&f.code, text_chars_ip) {
                    let mut character_count = 0usize;
                    let mut scalar_byte_lengths = [0usize; 64];
                    let mut prefix_byte_lengths = [0usize; 64];
                    let mut prefix_bytes = 0usize;
                    for scalar in text
                        .chars()
                        .take(limits.max_collection_items.saturating_add(1))
                    {
                        if character_count < fast_path.required_characters {
                            let scalar_bytes = scalar.len_utf8();
                            scalar_byte_lengths[character_count] = scalar_bytes;
                            prefix_bytes = prefix_bytes.saturating_add(scalar_bytes);
                            prefix_byte_lengths[character_count] = prefix_bytes;
                        }
                        character_count = character_count.saturating_add(1);
                        if character_count > limits.max_collection_items {
                            return Err(RuntimeError::new(
                                "R_COLLECTION_LIMIT",
                                "text_chars result exceeds the configured item limit",
                                instruction.span,
                            ));
                        }
                    }
                    if character_count >= fast_path.required_characters {
                        let text_chars_allocation = size_of::<Vec<Value>>()
                            .saturating_add(character_count.saturating_mul(size_of::<Value>()))
                            .saturating_add(character_count.saturating_mul(size_of::<String>()))
                            .saturating_add(text.len());
                        charge_allocation(
                            budget.as_ref(),
                            text_chars_allocation,
                            instruction_ref.span,
                        )?;

                        let mut indexed_characters = 0usize;
                        let mut additions = 0usize;
                        for skipped_ip in text_chars_ip + 1..fast_path.result_consumer_ip {
                            if budget.is_cancelled() || host.externally_cancelled() {
                                return Err(RuntimeError::new(
                                    "R_CANCELLED",
                                    "parallel task was cancelled after a sibling failed",
                                    f.code.first().map(|i| i.span).unwrap_or_default(),
                                ));
                            }
                            let skipped_instruction = &f.code[skipped_ip];
                            if !instruction_fuel.consume() {
                                return Err(RuntimeError::new(
                                    "R_STEP_LIMIT",
                                    "instruction budget exhausted",
                                    skipped_instruction.span,
                                ));
                            }
                            record_skipped_instruction!(skipped_ip, skipped_instruction);
                            match &skipped_instruction.op {
                                Op::Load(_) => {
                                    // Cloning the immutable character list has a zero-byte
                                    // logical charge, but still records the same budget event.
                                    charge_allocation(budget.as_ref(), 0, skipped_instruction.span)?;
                                }
                                Op::Const(value) => {
                                    charge_allocation(
                                        budget.as_ref(),
                                        cloned_value_allocation(value),
                                        skipped_instruction.span,
                                    )?;
                                }
                                Op::Index => {
                                    let scalar_bytes = scalar_byte_lengths[indexed_characters];
                                    indexed_characters += 1;
                                    charge_allocation(
                                        budget.as_ref(),
                                        size_of::<String>().saturating_add(scalar_bytes),
                                        skipped_instruction.span,
                                    )?;
                                }
                                Op::Add => {
                                    additions += 1;
                                    let bytes = prefix_byte_lengths[additions];
                                    if bytes > limits.max_text_bytes {
                                        return Err(RuntimeError::new(
                                            "R_TEXT_LIMIT",
                                            "text value exceeds the configured byte limit",
                                            skipped_instruction.span,
                                        ));
                                    }
                                    charge_allocation(
                                        budget.as_ref(),
                                        size_of::<String>().saturating_add(bytes),
                                        skipped_instruction.span,
                                    )?;
                                }
                                Op::Store(_) => {}
                                _ => unreachable!(
                                    "text character concat fast path only skips its verified pattern"
                                ),
                            }
                        }

                        let mut joined = String::with_capacity(
                            prefix_byte_lengths[fast_path.required_characters - 1],
                        );
                        for scalar in text.chars().take(fast_path.required_characters) {
                            joined.push(scalar);
                        }
                        let value = Value::Text(SharedText::from(joined));
                        if PROFILED {
                            if let Some(profile) = &budget.runtime_profile {
                                profile
                                    .vm_text_chars_concat_fast_paths
                                    .fetch_add(1, Ordering::Relaxed);
                            }
                        }
                        ip = fast_path.result_consumer_ip;
                        push_stack!(value);
                        continue;
                    }
                }
                let count = text.char_count();
                if count > limits.max_collection_items {
                    return Err(RuntimeError::new(
                        "R_COLLECTION_LIMIT",
                        "text_chars result exceeds the configured item limit",
                        instruction.span,
                    ));
                }
                let allocation = text.chars().fold(
                    size_of::<Vec<Value>>()
                        .saturating_add(count.saturating_mul(size_of::<Value>())),
                    |total, scalar| total.saturating_add(size_of::<String>() + scalar.len_utf8()),
                );
                charge_allocation(budget.as_ref(), allocation, instruction.span)?;
                let values = text
                    .chars()
                    .map(|scalar| Value::Text(shared_text_scalar(scalar)))
                    .collect::<Vec<_>>();
                push_stack!(Value::from_list_items(values));
            }
            Op::TextSlice => {
                let end = match pop!() {
                    Value::Int(index) => index.to_usize().ok_or_else(|| {
                        RuntimeError::new(
                            "R_INDEX_INVALID",
                            "text slice bounds must be non-negative machine-sized integers",
                            instruction.span,
                        )
                    })?,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "text_slice end bound must be Int",
                            instruction.span,
                        ));
                    }
                };
                let start = match pop!() {
                    Value::Int(index) => index.to_usize().ok_or_else(|| {
                        RuntimeError::new(
                            "R_INDEX_INVALID",
                            "text slice bounds must be non-negative machine-sized integers",
                            instruction.span,
                        )
                    })?,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "text_slice start bound must be Int",
                            instruction.span,
                        ));
                    }
                };
                let text = match pop!() {
                    Value::Text(text) => text,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "text_slice requires Text",
                            instruction.span,
                        ));
                    }
                };
                if start > end {
                    return Err(RuntimeError::new(
                        "R_INDEX_BOUNDS",
                        format!("text slice start {start} exceeds end {end}"),
                        instruction.span,
                    ));
                }
                let Some((byte_start, byte_end)) = text_scalar_range(&text, start, end) else {
                    return Err(RuntimeError::new(
                        "R_INDEX_BOUNDS",
                        format!(
                            "text slice range {start}..{end} is out of bounds for length {}",
                            text.char_count()
                        ),
                        instruction.span,
                    ));
                };
                if byte_start == 0 && byte_end == text.len() {
                    push_stack!(Value::Text(text));
                    continue;
                }
                let sliced = &text[byte_start..byte_end];
                let value = Value::Text(SharedText::from(sliced));
                charge_allocation(
                    budget.as_ref(),
                    new_value_allocation(&value),
                    instruction.span,
                )?;
                push_stack!(value);
            }
            Op::TextJoin => {
                let delimiter = match pop!() {
                    Value::Text(text) => text,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "text_join_native requires Text delimiter",
                            instruction.span,
                        ));
                    }
                };
                let values = match pop!() {
                    Value::List(values) => values,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "text_join_native requires List[Text]",
                            instruction.span,
                        ));
                    }
                };
                if values.len() > limits.max_collection_items {
                    return Err(RuntimeError::new(
                        "R_COLLECTION_LIMIT",
                        "text_join_native exceeds the configured item limit",
                        instruction.span,
                    ));
                }
                let separator_bytes = delimiter
                    .len()
                    .checked_mul(values.len().saturating_sub(1))
                    .ok_or_else(|| {
                        RuntimeError::new(
                            "R_ALLOCATION_LIMIT",
                            "joined text length overflow",
                            instruction.span,
                        )
                    })?;
                let mut total = separator_bytes;
                for value in values.iter() {
                    let Value::Text(text) = value else {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "text_join_native requires List[Text]",
                            instruction.span,
                        ));
                    };
                    total = total.checked_add(text.len()).ok_or_else(|| {
                        RuntimeError::new(
                            "R_ALLOCATION_LIMIT",
                            "joined text length overflow",
                            instruction.span,
                        )
                    })?;
                    if total > limits.max_text_bytes {
                        return Err(RuntimeError::new(
                            "R_ALLOCATION_LIMIT",
                            "joined text exceeds the configured text limit",
                            instruction.span,
                        ));
                    }
                }
                charge_allocation(budget.as_ref(), total, instruction.span)?;
                let mut joined = String::with_capacity(total);
                for (index, value) in values.iter().enumerate() {
                    if index > 0 {
                        joined.push_str(&delimiter);
                    }
                    if let Value::Text(text) = value {
                        joined.push_str(text);
                    }
                }
                push_stack!(Value::Text(SharedText::from(joined)));
            }
            Op::TextFind => {
                let from = match pop!() {
                    Value::Int(index) => index.to_usize().ok_or_else(|| {
                        RuntimeError::new(
                            "R_INDEX_INVALID",
                            "text_find start must be a non-negative machine-sized integer",
                            instruction.span,
                        )
                    })?,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "text_find start must be Int",
                            instruction.span,
                        ));
                    }
                };
                let needle = match pop!() {
                    Value::Text(text) => text,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "text_find requires Text needle",
                            instruction.span,
                        ));
                    }
                };
                let text = match pop!() {
                    Value::Text(text) => text,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "text_find requires Text",
                            instruction.span,
                        ));
                    }
                };
                let found = text_find_scalar(&text, &needle, from).ok_or_else(|| {
                    RuntimeError::new(
                        "R_INDEX_BOUNDS",
                        format!("text_find start {from} is past the end of the text"),
                        instruction.span,
                    )
                })?;
                let found = match found {
                    Some(index) => BigInt::from_usize(index),
                    None => BigInt::from_i64(-1),
                };
                push_stack!(Value::Int(found));
            }
            Op::TextSplit => {
                let delimiter = match pop!() {
                    Value::Text(text) => text,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "text_split_native requires Text delimiter",
                            instruction.span,
                        ));
                    }
                };
                let text = match pop!() {
                    Value::Text(text) => text,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "text_split_native requires Text",
                            instruction.span,
                        ));
                    }
                };
                if delimiter.is_empty() {
                    charge_allocation(
                        budget.as_ref(),
                        text_list_allocation(1, text.len()),
                        instruction.span,
                    )?;
                    push_stack!(Value::from_list_items(vec![Value::Text(text)]));
                    continue;
                }
                let Some(offsets) = text_match_offsets(
                    &text,
                    &delimiter,
                    limits.max_collection_items.saturating_sub(1),
                ) else {
                    return Err(RuntimeError::new(
                        "R_COLLECTION_LIMIT",
                        "text_split result exceeds the configured item limit",
                        instruction.span,
                    ));
                };
                let count = offsets.len() + 1;
                let field_bytes = text.len() - (count - 1) * delimiter.len();
                charge_allocation(
                    budget.as_ref(),
                    text_list_allocation(count, field_bytes),
                    instruction.span,
                )?;
                push_stack!(Value::from_list_items(text_split_at(
                    &text,
                    &offsets,
                    delimiter.len()
                )));
            }
            Op::TextReplace => {
                let replacement = match pop!() {
                    Value::Text(text) => text,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "text_replace_native requires Text replacement",
                            instruction.span,
                        ));
                    }
                };
                let needle = match pop!() {
                    Value::Text(text) => text,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "text_replace_native requires Text needle",
                            instruction.span,
                        ));
                    }
                };
                let text = match pop!() {
                    Value::Text(text) => text,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "text_replace_native requires Text",
                            instruction.span,
                        ));
                    }
                };
                let count = if needle.is_empty() {
                    0
                } else {
                    text_match_count(&text, &needle)
                };
                if count == 0 {
                    push_stack!(Value::Text(text));
                    continue;
                }
                let total = (text.len() - count * needle.len())
                    .saturating_add(count.saturating_mul(replacement.len()));
                if total > limits.max_text_bytes {
                    return Err(RuntimeError::new(
                        "R_ALLOCATION_LIMIT",
                        "replaced text exceeds the configured text limit",
                        instruction.span,
                    ));
                }
                charge_allocation(budget.as_ref(), total, instruction.span)?;
                push_stack!(Value::Text(text_replace_all(
                    &text,
                    &needle,
                    &replacement,
                    total
                )));
            }
            Op::TextTrim => {
                let text = match pop!() {
                    Value::Text(text) => text,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "text_trim_native requires Text",
                            instruction.span,
                        ));
                    }
                };
                let trimmed = text.trim();
                if trimmed.len() == text.len() {
                    push_stack!(Value::Text(text));
                    continue;
                }
                let value = Value::Text(SharedText::from(trimmed));
                charge_allocation(
                    budget.as_ref(),
                    new_value_allocation(&value),
                    instruction.span,
                )?;
                push_stack!(value);
            }
            Op::Math(function) => {
                let function = *function;
                let right = if function.arity() == 2 {
                    match pop!() {
                        Value::Float64(value) => Some(value.get()),
                        _ => {
                            return Err(RuntimeError::new(
                                "R_TYPE_INVALID",
                                "math functions require Float64 values",
                                instruction.span,
                            ));
                        }
                    }
                } else {
                    None
                };
                let Value::Float64(left) = pop!() else {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "math functions require Float64 values",
                        instruction.span,
                    ));
                };
                let result = apply_math(function, left.get(), right.unwrap_or(0.0)).map_err(
                    |(code, message)| RuntimeError::new(code, message, instruction.span),
                )?;
                push_stack!(Value::Float64(result));
            }
            Op::Codec(function) => {
                let function = *function;
                let mut operands = Vec::with_capacity(function.arity());
                for _ in 0..function.arity() {
                    operands.push(pop!());
                }
                operands.reverse();
                let value = apply_codec(function, operands).map_err(|(code, message)| {
                    RuntimeError::new(code, message, instruction.span)
                })?;
                charge_allocation(
                    budget.as_ref(),
                    new_value_allocation(&value),
                    instruction.span,
                )?;
                push_stack!(value);
            }
            Op::Regex(function) => {
                let function = *function;
                let mut operands = Vec::with_capacity(function.arity());
                for _ in 0..function.arity() {
                    operands.push(pop!());
                }
                operands.reverse();
                let value = apply_regex(function, operands).map_err(|(code, message)| {
                    RuntimeError::new(code, message, instruction.span)
                })?;
                charge_allocation(
                    budget.as_ref(),
                    new_value_allocation(&value),
                    instruction.span,
                )?;
                push_stack!(value);
            }
            Op::IntToFloat => {
                let Value::Int(value) = pop!() else {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "int_to_float requires an Int value",
                        instruction.span,
                    ));
                };
                let converted = match value.small() {
                    Some(small) => Float64::new(small as f64),
                    None => value.to_string().parse::<f64>().ok().and_then(Float64::new),
                }
                .ok_or_else(|| {
                    RuntimeError::new(
                        "R_FLOAT_NONFINITE",
                        "Int is too large to convert to Float64",
                        instruction.span,
                    )
                })?;
                push_stack!(Value::Float64(converted));
            }
            Op::FloatToInt => {
                let Value::Float64(value) = pop!() else {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "float_to_int requires a Float64 value",
                        instruction.span,
                    ));
                };
                push_stack!(Value::Int(float_to_bigint(value.get())));
            }
            Op::NumberToText => {
                let text = match pop!() {
                    Value::Int(value) => value.to_string(),
                    Value::Float64(value) => float_to_text(value.get()),
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "number-to-text conversion requires an Int or Float64 value",
                            instruction.span,
                        ));
                    }
                };
                let value = Value::Text(SharedText::from(text.as_str()));
                charge_allocation(
                    budget.as_ref(),
                    new_value_allocation(&value),
                    instruction.span,
                )?;
                push_stack!(value);
            }
            Op::RandomMix => {
                let Value::Int(state) = pop!() else {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "random_mix requires an Int state",
                        instruction.span,
                    ));
                };
                push_stack!(Value::Int(BigInt::from_u64(splitmix64_mix(
                    bigint_low_u64(&state)
                ))));
            }
            Op::BytesFromInts => {
                let values = match pop!() {
                    Value::List(values) => values,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "bytes_from_ints requires List[Int]",
                            instruction.span,
                        ));
                    }
                };
                if values.len() > limits.max_collection_items {
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
                push_stack!(Value::Bytes(Shared::new(bytes)));
            }
            Op::BytesFromText => {
                let text = match pop!() {
                    Value::Text(text) => text,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "bytes_from_text requires Text",
                            instruction.span,
                        ));
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
                push_stack!(Value::Bytes(Shared::new(text.as_bytes().to_vec())));
            }
            Op::BytesJoin => {
                let values = match pop!() {
                    Value::List(values) => values,
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "bytes_join requires List[Bytes]",
                            instruction.span,
                        ));
                    }
                };
                if values.len() > limits.max_collection_items {
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
                push_stack!(Value::Bytes(Shared::new(bytes)));
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
                    ));
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
                        let value = items.get(key.as_ref()).cloned().ok_or_else(|| {
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
                        let value = text_scalar_at(&text, index).ok_or_else(|| {
                            RuntimeError::new(
                                "R_INDEX_BOUNDS",
                                format!(
                                    "text index {index} is out of range for length {}",
                                    text.char_count()
                                ),
                                instruction.span,
                            )
                        })?;
                        let value = Value::Text(value);
                        charge_allocation(
                            budget.as_ref(),
                            new_value_allocation(&value),
                            instruction.span,
                        )?;
                        push_stack!(value);
                    }
                    (Value::List(_), _) => {
                        return Err(RuntimeError::new(
                            "R_INDEX_INVALID",
                            "list index must be Int",
                            instruction.span,
                        ));
                    }
                    (Value::Float64List(_), _) => {
                        return Err(RuntimeError::new(
                            "R_INDEX_INVALID",
                            "list index must be Int",
                            instruction.span,
                        ));
                    }
                    (Value::Map(_), _) => {
                        return Err(RuntimeError::new(
                            "R_INDEX_INVALID",
                            "map index must be Text",
                            instruction.span,
                        ));
                    }
                    (Value::Text(_), _) => {
                        return Err(RuntimeError::new(
                            "R_INDEX_INVALID",
                            "text index must be Int",
                            instruction.span,
                        ));
                    }
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "index requires a List, Map, or Text",
                            instruction.span,
                        ));
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
                        ));
                    }
                };
                if items >= limits.max_collection_items {
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
                    _ => size_of::<Vec<Value>>()
                        .saturating_add(items.saturating_add(1).saturating_mul(size_of::<Value>())),
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
                        Value::Float64List(Shared::new(updated))
                    }
                    _ => unreachable!("list type checked above"),
                };
                if let Some(error) = value_limit_error(&list, limits, instruction.span) {
                    return Err(error);
                }
                push_stack!(list);
            }
            Op::ListPushLocal(slot) => {
                let value = pop!();
                // Items already in the list passed this check when they were created, so
                // checking the pushed value keeps each push constant-time.
                if let Some(error) = value_limit_error(&value, limits, instruction.span) {
                    return Err(error);
                }
                let local = locals.get_mut(*slot).ok_or_else(|| {
                    RuntimeError::new(
                        "R_LOCAL_UNINITIALIZED",
                        "list_push assignment targets an uninitialized local",
                        instruction.span,
                    )
                })?;
                match local {
                    Value::List(items) => {
                        let len = items.len();
                        if len >= limits.max_collection_items {
                            return Err(RuntimeError::new(
                                "R_COLLECTION_LIMIT",
                                "list exceeds the configured item limit",
                                instruction.span,
                            ));
                        }
                        let shared = Shared::strong_count(items) > 1;
                        let capacity = items.capacity();
                        let allocation = if shared || capacity == len {
                            let grown_capacity =
                                capacity.saturating_mul(2).max(len.saturating_add(1)).max(4);
                            size_of::<Vec<Value>>()
                                .saturating_add(grown_capacity.saturating_mul(size_of::<Value>()))
                        } else {
                            0
                        };
                        charge_allocation(budget.as_ref(), allocation, instruction.span)?;
                        Shared::make_mut(items).push(value);
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
                        if len >= limits.max_collection_items {
                            return Err(RuntimeError::new(
                                "R_COLLECTION_LIMIT",
                                "list exceeds the configured item limit",
                                instruction.span,
                            ));
                        }
                        let shared = Shared::strong_count(items) > 1;
                        let capacity = items.capacity();
                        let allocation = if shared || capacity == len {
                            let grown_capacity =
                                capacity.saturating_mul(2).max(len.saturating_add(1)).max(4);
                            size_of::<Vec<Float64>>()
                                .saturating_add(grown_capacity.saturating_mul(size_of::<Float64>()))
                        } else {
                            0
                        };
                        charge_allocation(budget.as_ref(), allocation, instruction.span)?;
                        Shared::make_mut(items).push(value);
                    }
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "list_push assignment target must be a List",
                            instruction.span,
                        ));
                    }
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
                        ));
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
                        ));
                    }
                };
                if index >= len {
                    return Err(RuntimeError::new(
                        "R_LIST_INDEX",
                        format!("list_set index {index} is out of range for length {}", len),
                        instruction.span,
                    ));
                }
                budget.record_list_set_operation();
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
                        Value::Float64List(Shared::new(updated))
                    }
                    _ => unreachable!("list type checked above"),
                };
                budget.record_list_set_copy(len, copy_bytes);
                if let Some(error) = value_limit_error(&list, limits, instruction.span) {
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
                        ));
                    }
                };
                // The other items passed this check when they were created, so checking the
                // stored value keeps each in-place update constant-time.
                if let Some(error) = value_limit_error(&value, limits, instruction.span) {
                    return Err(error);
                }
                let local = locals.get_mut(*slot).ok_or_else(|| {
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
                        ));
                    }
                };
                if index >= len {
                    return Err(RuntimeError::new(
                        "R_LIST_INDEX",
                        format!("list_set index {index} is out of range for length {}", len),
                        instruction.span,
                    ));
                }
                budget.record_list_set_operation();
                match local {
                    Value::List(items) => {
                        let needs_copy = Shared::strong_count(items) > 1;
                        let copy_bytes = size_of::<Vec<Value>>()
                            .saturating_add(len.saturating_mul(size_of::<Value>()));
                        if needs_copy {
                            charge_allocation(budget.as_ref(), copy_bytes, instruction.span)?;
                        }
                        Shared::make_mut(items)[index] = value;
                        if needs_copy {
                            budget.record_list_set_copy(len, copy_bytes);
                        } else {
                            budget.record_list_set_in_place_update();
                        }
                    }
                    Value::Float64List(items) => {
                        let Value::Float64(value) = value else {
                            return Err(RuntimeError::new(
                                "R_TYPE_INVALID",
                                "list_set value must match the list item type",
                                instruction.span,
                            ));
                        };
                        let needs_copy = Shared::strong_count(items) > 1;
                        let copy_bytes = size_of::<Vec<Float64>>()
                            .saturating_add(len.saturating_mul(size_of::<Float64>()));
                        if needs_copy {
                            charge_allocation(budget.as_ref(), copy_bytes, instruction.span)?;
                        }
                        Shared::make_mut(items)[index] = value;
                        if needs_copy {
                            budget.record_list_set_copy(len, copy_bytes);
                        } else {
                            budget.record_list_set_in_place_update();
                        }
                    }
                    _ => unreachable!("list type checked above"),
                }
            }
            Op::Equal | Op::NotEqual => {
                if matches!(instruction.op, Op::Equal) {
                    small_int_binary!(|a, b| Some(Value::Bool(a == b)));
                } else {
                    small_int_binary!(|a, b| Some(Value::Bool(a != b)));
                }
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
                match instruction.op {
                    Op::Less => small_int_binary!(|a, b| Some(Value::Bool(a < b))),
                    Op::LessEqual => small_int_binary!(|a, b| Some(Value::Bool(a <= b))),
                    Op::Greater => small_int_binary!(|a, b| Some(Value::Bool(a > b))),
                    _ => small_int_binary!(|a, b| Some(Value::Bool(a >= b))),
                }
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
            Op::Jump(target) => {
                if PROFILED {
                    if let Some(profile) = &budget.runtime_profile {
                        if *target < ip {
                            profile.record_function_loop_backedge(function_index);
                            if let Some(profile) = &profile_sample {
                                profile.record_loop_backedge();
                            }
                        }
                    }
                }
                poll_cancellation = *target < ip;
                ip = *target;
            }
            Op::JumpIfFalse(target) => {
                if let Value::Bool(c) = pop!() {
                    budget.record_guest_conditional_branch();
                    if !c {
                        if PROFILED {
                            if let Some(profile) = &budget.runtime_profile {
                                if *target < ip {
                                    profile.record_function_loop_backedge(function_index);
                                    if let Some(profile) = &profile_sample {
                                        profile.record_loop_backedge();
                                    }
                                }
                            }
                        }
                        poll_cancellation = *target < ip;
                        ip = *target;
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
                if *argc
                    == fast_host_forwarders
                        .get(*target)
                        .and_then(|forwarder| *forwarder)
                        .map(|forwarder| forwarder.argument_count)
                        .unwrap_or(usize::MAX)
                    && stack.len() >= *argc
                {
                    if let Some(forwarder) =
                        fast_host_forwarders.get(*target).and_then(|value| *value)
                    {
                        if let Err(mut error) =
                            check_call_depth(artifact, limits, *target, depth.saturating_add(1))
                        {
                            error.trace.push(instruction.span);
                            return Err(error);
                        }
                        let child_started = if PROFILED {
                            profile_sample.as_ref().map(|_| Instant::now())
                        } else {
                            None
                        };
                        let forwarder_result = execute_fast_host_forwarder::<PROFILED, _>(
                            FastHostForwarderExecution {
                                host,
                                artifact,
                                target: *target,
                                forwarder,
                                stack: &mut stack,
                                locals: &mut locals,
                                budget,
                                instruction_fuel: &mut instruction_fuel,
                                depth: depth.saturating_add(1),
                            },
                        );
                        if let (Some(sample), Some(started)) = (&profile_sample, child_started) {
                            sample.record_child_duration(started.elapsed());
                        }
                        match forwarder_result {
                            Ok(value) => {
                                push_stack!(value);
                                continue;
                            }
                            Err(mut error) => {
                                error.trace.push(instruction.span);
                                return Err(error);
                            }
                        }
                    }
                }
                let local_count = artifact
                    .functions
                    .get(*target)
                    .map_or(*argc, |callee| callee.local_count);
                let mut call_args = pool.take(local_count.max(*argc));
                for _ in 0..*argc {
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
                    target: *target,
                    args: call_args,
                    span: instruction.span,
                });
            }
            Op::Assert => match pop!() {
                Value::Bool(true) => push_stack!(Value::Unit),
                Value::Bool(false) => {
                    return Err(RuntimeError::new(
                        "R_ASSERTION",
                        "assertion failed",
                        instruction.span,
                    ));
                }
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "assert requires Bool",
                        instruction.span,
                    ));
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
                    ));
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
                    ));
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
                    ));
                }
                Value::OptionNone => {
                    return Err(RuntimeError::new(
                        "R_OPTION_UNWRAP",
                        "cannot unwrap None",
                        instruction.span,
                    ));
                }
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "unwrap requires Result or Option",
                        instruction.span,
                    ));
                }
            },
            Op::GetError => match pop!() {
                Value::ResultErr(value) => push_stack!(*value),
                Value::ResultOk(_) => {
                    return Err(RuntimeError::new(
                        "R_RESULT_ERROR",
                        "cannot get the error from an Ok result",
                        instruction.span,
                    ));
                }
                _ => {
                    return Err(RuntimeError::new(
                        "R_TYPE_INVALID",
                        "error requires Result",
                        instruction.span,
                    ));
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
                    ));
                }
            },
            Op::Print | Op::PrintErr => {
                let v = pop!();
                let remaining_output = limits
                    .output_limit_bytes
                    .saturating_sub(host.program_output_bytes());
                let mut rendered = LimitedString::new(remaining_output);
                if writeln!(&mut rendered, "{v}").is_err() {
                    return Err(RuntimeError::new(
                        "R_OUTPUT_LIMIT",
                        "program output exceeds the configured limit",
                        instruction.span,
                    ));
                }
                charge_allocation(budget.as_ref(), rendered.value.len(), instruction.span)?;
                let stream = if matches!(&instruction.op, Op::PrintErr) {
                    OutputStream::Stderr
                } else {
                    OutputStream::Stdout
                };
                let host_started = if PROFILED {
                    profile_sample.as_ref().map(|_| Instant::now())
                } else {
                    None
                };
                let output_result =
                    host.write_program_output(stream, &rendered.value, instruction.span);
                if let (Some(sample), Some(started)) = (&profile_sample, host_started) {
                    sample.record_host_duration(started.elapsed());
                }
                output_result?;
            }
            Op::Return => {
                let result = pop!();
                if !value_matches(&result, &f.result, &host.artifact().records) {
                    return Err(RuntimeError::new(
                        "R_RETURN_INVALID",
                        "function returned a value with an invalid type",
                        instruction.span,
                    ));
                }
                if let Some(error) = value_limit_error(&result, limits, instruction.span) {
                    return Err(error);
                }
                pool.give(locals);
                pool.give(stack);
                return Ok(VmStep::Return(result));
            }
            _ => {
                instruction_fuel.flush();
                let host_started = if PROFILED {
                    profile_sample.as_ref().map(|_| Instant::now())
                } else {
                    None
                };
                let host_result = host.host_op(HostOpContext {
                    instruction,
                    function: f,
                    stack: &mut stack,
                    locals: &mut locals,
                    budget,
                    depth,
                });
                if let (Some(sample), Some(started)) = (&profile_sample, host_started) {
                    sample.record_host_duration(started.elapsed());
                }
                host_result?;
                // A host operation can stop early on cancellation, leaving no result behind.
                poll_cancellation = true;
                if budget.is_cancelled() || host.externally_cancelled() {
                    continue;
                }
                if host_result_needs_limit_check(&instruction.op) {
                    if let Some(result) = stack.last() {
                        if let Some(error) = value_limit_error(result, limits, instruction.span) {
                            return Err(error);
                        }
                    }
                }
            }
        }
    }
}

/// These operations were reached through forwarding wrappers whose return applied the
/// value limits; the check stays now that they are called directly. `CsvNext` checks its own
/// row, where it knows the shape.
fn host_result_needs_limit_check(op: &Op) -> bool {
    matches!(
        op,
        Op::JsonTryGet
            | Op::JsonTryIndex
            | Op::JsonTryGetText
            | Op::JsonTryGetInt
            | Op::JsonTryText
            | Op::JsonTryInt
            | Op::JsonTryBool
            | Op::JsonTryFloat
            | Op::FileNextLine
            | Op::TextParseInt
            | Op::TextParseFloat
            | Op::TextCollapseWhitespace
            | Op::TimeParseTimestamp
            | Op::TimeFormatUtc
            | Op::TimeFormatUtcDate
            | Op::HttpResponseJson
    )
}

#[derive(Clone, Copy)]
struct FastHostForwarder {
    argument_count: usize,
    host_operation_ip: usize,
}

struct FastHostForwarderExecution<'a, 'fuel, H: VmHost> {
    host: &'a mut H,
    artifact: &'a Artifact,
    target: usize,
    forwarder: FastHostForwarder,
    stack: &'a mut Vec<Value>,
    locals: &'a mut Vec<Value>,
    budget: &'a Arc<Budget>,
    instruction_fuel: &'a mut InstructionFuel<'fuel>,
    depth: usize,
}

fn fast_host_forwarder(function: &guion_model::Function) -> Option<FastHostForwarder> {
    let argument_count = function.parameters.len();
    if function.local_count != argument_count || function.code.len() != argument_count + 2 {
        return None;
    }
    for (slot, instruction) in function.code.iter().take(argument_count).enumerate() {
        if !matches!(instruction.op, Op::Load(loaded) if loaded == slot) {
            return None;
        }
    }
    let host_operation_ip = argument_count;
    let host_operation = &function.code[host_operation_ip].op;
    let expected_arguments = match host_operation {
        Op::JsonTryGet | Op::JsonTryIndex | Op::JsonTryGetText | Op::JsonTryGetInt => 2,
        Op::JsonTryText
        | Op::JsonTryInt
        | Op::JsonTryBool
        | Op::JsonTryFloat
        | Op::FileNextLine
        | Op::CsvNext
        | Op::TextParseInt
        | Op::TextParseFloat
        | Op::TextCollapseWhitespace
        | Op::TimeParseTimestamp
        | Op::TimeFormatUtc
        | Op::TimeFormatUtcDate
        | Op::HttpResponseJson
        | Op::MakeSome => 1,
        Op::MakeNone => 0,
        _ => return None,
    };
    if expected_arguments != argument_count
        || !matches!(
            function.code.last().map(|instruction| &instruction.op),
            Some(Op::Return)
        )
    {
        return None;
    }
    Some(FastHostForwarder {
        argument_count,
        host_operation_ip,
    })
}

/// The host operation a tiny forwarding function applies to its own parameters, when
/// the function is exactly `Load 0..n; <host operation>; Return`.
pub fn forwarded_host_operation(function: &guion_model::Function) -> Option<Op> {
    fast_host_forwarder(function)
        .map(|forwarder| function.code[forwarder.host_operation_ip].op.clone())
}

fn execute_fast_host_forwarder<const PROFILED: bool, H: VmHost>(
    execution: FastHostForwarderExecution<'_, '_, H>,
) -> Result<Value, RuntimeError> {
    let FastHostForwarderExecution {
        host,
        artifact,
        target,
        forwarder,
        stack,
        locals,
        budget,
        instruction_fuel,
        depth,
    } = execution;
    let function = artifact.functions.get(target).ok_or_else(|| {
        RuntimeError::new(
            "R_CALL_TARGET",
            "function call target is outside the artifact",
            Span::default(),
        )
    })?;
    let argument_start = stack
        .len()
        .checked_sub(forwarder.argument_count)
        .ok_or_else(|| {
            RuntimeError::new(
                "R_STACK_UNDERFLOW",
                "function call has fewer stack values than arguments",
                function
                    .code
                    .first()
                    .map(|instruction| instruction.span)
                    .unwrap_or_default(),
            )
        })?;
    let profile_sample = if PROFILED {
        budget
            .runtime_profile
            .as_ref()
            .filter(|profile| profile.should_sample_vm_frame())
            .map(|profile| VmFrameProfileTimer::new(Arc::clone(profile), &function.name))
    } else {
        None
    };
    for (ip, instruction) in function.code.iter().enumerate() {
        if budget.is_cancelled() || host.externally_cancelled() {
            return Err(RuntimeError::new(
                "R_CANCELLED",
                "parallel task was cancelled after a sibling failed",
                instruction.span,
            ));
        }
        if !instruction_fuel.consume() {
            return Err(RuntimeError::new(
                "R_STEP_LIMIT",
                "instruction budget exhausted",
                instruction.span,
            ));
        }
        if PROFILED {
            if let Some(profile) = &budget.runtime_profile {
                if profile.detailed_counters_enabled() {
                    profile.record_opcode_execution(opcode_profile_name(&instruction.op));
                    profile
                        .vm_interpreter_instructions
                        .fetch_add(1, Ordering::Relaxed);
                }
                if let Some(sample) = &profile_sample {
                    sample.record_instruction();
                }
            }
        }
        if ip < forwarder.argument_count {
            if !matches!(instruction.op, Op::Load(_)) {
                unreachable!("forwarder descriptor only accepts parameter loads")
            }
            // The caller's arguments already occupy the stack positions used by the host
            // operation; avoid cloning and pushing another copy of each value.
        } else if ip == forwarder.host_operation_ip {
            match &instruction.op {
                Op::MakeNone => stack.push(Value::OptionNone),
                Op::MakeSome => {
                    let value = stack.pop().ok_or_else(|| {
                        RuntimeError::new(
                            "R_STACK_UNDERFLOW",
                            "option constructor is missing its value",
                            instruction.span,
                        )
                    })?;
                    charge_allocation(budget, size_of::<Value>(), instruction.span)?;
                    stack.push(Value::OptionSome(Box::new(value)));
                }
                _ => {
                    instruction_fuel.flush();
                    let host_started = if PROFILED {
                        profile_sample.as_ref().map(|_| Instant::now())
                    } else {
                        None
                    };
                    let host_result = host.host_op(HostOpContext {
                        instruction,
                        function,
                        stack,
                        locals,
                        budget,
                        depth,
                    });
                    if let (Some(sample), Some(started)) = (&profile_sample, host_started) {
                        sample.record_host_duration(started.elapsed());
                    }
                    host_result?;
                }
            }
        } else {
            let result = stack.pop().ok_or_else(|| {
                RuntimeError::new(
                    "R_STACK_UNDERFLOW",
                    "forwarding function returned without a value",
                    instruction.span,
                )
            })?;
            if !value_matches(&result, &function.result, &artifact.records) {
                return Err(RuntimeError::new(
                    "R_RETURN_INVALID",
                    "function returned a value with an invalid type",
                    instruction.span,
                ));
            }
            if let Some(error) = value_limit_error(&result, &host.limits(), instruction.span) {
                return Err(error);
            }
            stack.truncate(argument_start);
            return Ok(result);
        }
    }
    Err(RuntimeError::new(
        "R_NO_RETURN",
        format!(
            "function `{}` reached the end without a return",
            function.name
        ),
        function
            .code
            .last()
            .map(|instruction| instruction.span)
            .unwrap_or_default(),
    ))
}

fn unique_nested_map_update_target(
    code: &[guion_model::Instruction],
    next_ip: usize,
    stack: &[Value],
    locals: &[Value],
    inner: &Shared<TextMap>,
) -> Option<(usize, SharedText)> {
    let Some(Op::MapSet) = code.get(next_ip).map(|instruction| &instruction.op) else {
        return None;
    };
    let Some(Op::Store(parent_slot)) = code.get(next_ip + 1).map(|instruction| &instruction.op)
    else {
        return None;
    };
    let [.., Value::Map(parent), Value::Text(parent_key)] = stack else {
        return None;
    };
    let Some(Value::Map(local_parent)) = locals.get(*parent_slot) else {
        return None;
    };
    if !Shared::ptr_eq(parent, local_parent)
        || Shared::strong_count(parent) != 2
        || Shared::strong_count(inner) != 2
        || !matches!(parent.get(parent_key.as_ref()), Some(Value::Map(child)) if Shared::ptr_eq(child, inner))
    {
        return None;
    }
    Some((*parent_slot, SharedText::clone(parent_key)))
}

fn unique_pending_nested_map_writeback_target(
    code: &[guion_model::Instruction],
    child_store_ip: usize,
    child_slot: usize,
    locals: &[Value],
    inner: &Shared<TextMap>,
) -> Option<(usize, SharedText)> {
    let Some(Op::Store(stored_child)) = code.get(child_store_ip).map(|instruction| &instruction.op)
    else {
        return None;
    };
    if *stored_child != child_slot {
        return None;
    }

    // The only instructions allowed between storing the updated child and the
    // outer map_set are unconditional branch joins. Reject any expression that
    // could observe either local before the parent writeback.
    let mut ip = child_store_ip.checked_add(1)?;
    let mut writeback = None;
    for _ in 0..16 {
        match &code.get(ip)?.op {
            Op::Jump(target) => ip = *target,
            Op::Load(parent_slot) if *parent_slot != child_slot => {
                let parent_key = match &code.get(ip + 1)?.op {
                    Op::Load(key_slot) => match locals.get(*key_slot)? {
                        Value::Text(key) => SharedText::clone(key),
                        _ => return None,
                    },
                    Op::Const(Value::Text(key)) => SharedText::clone(key),
                    _ => return None,
                };
                if !matches!(code.get(ip + 2).map(|instruction| &instruction.op), Some(Op::Load(slot)) if *slot == child_slot)
                    || !matches!(
                        code.get(ip + 3).map(|instruction| &instruction.op),
                        Some(Op::MapSet)
                    )
                    || !matches!(code.get(ip + 4).map(|instruction| &instruction.op), Some(Op::Store(slot)) if *slot == *parent_slot)
                {
                    return None;
                }
                writeback = Some((*parent_slot, parent_key));
                break;
            }
            _ => return None,
        }
    }
    let (parent_slot, parent_key) = writeback?;

    let Some(Value::Map(local_child)) = locals.get(child_slot) else {
        return None;
    };
    let Some(Value::Map(parent)) = locals.get(parent_slot) else {
        return None;
    };
    if !Shared::ptr_eq(local_child, inner)
        || Shared::strong_count(parent) != 1
        || Shared::strong_count(inner) != 3
    {
        return None;
    }
    let mut matching_key = None;
    for (key, value) in parent.iter() {
        if matches!(value, Value::Map(child) if Shared::ptr_eq(child, inner))
            && matching_key.replace(&**key).is_some()
        {
            return None;
        }
    }
    if matching_key != Some(parent_key.as_ref()) {
        return None;
    }
    Some((parent_slot, parent_key))
}

#[derive(Debug, Default)]
pub struct CopySizeBucket {
    pub copies: AtomicUsize,
    pub estimated_copy_bytes: AtomicUsize,
}

#[derive(Debug, Default)]
pub struct RuntimeProfileCounters {
    sampled_mode: AtomicBool,
    host_operation_events: AtomicUsize,
    pub allocation_charge_events: AtomicUsize,
    pub estimated_allocation_gross_bytes: AtomicUsize,
    pub estimated_allocation_refunded_bytes: AtomicUsize,
    pub list_set_operations: AtomicUsize,
    pub list_set_in_place_updates: AtomicUsize,
    pub list_set_copy_on_write_copies: AtomicUsize,
    pub list_set_estimated_copy_bytes: AtomicUsize,
    pub list_set_copy_size_histogram: [CopySizeBucket; 4],
    pub record_charge_events: AtomicUsize,
    pub record_estimated_bytes: AtomicUsize,
    pub dynamic_guest_conditional_branches: AtomicUsize,
    pub vm_interpreter_instructions: AtomicUsize,
    pub vm_interpreter_frames: AtomicUsize,
    pub vm_sampled_frames: AtomicUsize,
    pub vm_sampled_instructions: AtomicUsize,
    pub vm_sampled_nanoseconds: AtomicU64,
    pub vm_result_option_probe_fast_paths: AtomicUsize,
    pub vm_result_option_unwrap_fast_paths: AtomicUsize,
    pub vm_record_field_load_fast_paths: AtomicUsize,
    pub vm_list_index_load_fast_paths: AtomicUsize,
    pub vm_text_chars_concat_fast_paths: AtomicUsize,
    pub json_parse_nanoseconds: AtomicU64,
    pub json_metrics_nanoseconds: AtomicU64,
    pub opcode_executions: Mutex<BTreeMap<&'static str, u64>>,
    pub opcode_transition_counts: Mutex<BTreeMap<String, u64>>,
    pub function_call_counts: Mutex<BTreeMap<String, u64>>,
    function_profile_names: OnceLock<Vec<String>>,
    function_loop_backedge_counts: OnceLock<Vec<AtomicU64>>,
    sampled_function_counts: Mutex<BTreeMap<String, SampledFunctionProfile>>,
    pub host_operation_counts: Mutex<BTreeMap<&'static str, u64>>,
    pub host_operation_nanoseconds: Mutex<BTreeMap<&'static str, u64>>,
    pub map_set_operations: AtomicUsize,
    pub map_set_in_place_updates: AtomicUsize,
    pub map_set_copy_on_write_copies: AtomicUsize,
    pub map_set_estimated_copy_bytes: AtomicUsize,
    pub map_record_get_operations: AtomicUsize,
    pub map_record_set_operations: AtomicUsize,
    pub map_record_set_in_place_updates: AtomicUsize,
    pub map_record_set_copy_on_write_copies: AtomicUsize,
    pub map_record_set_estimated_map_copy_bytes: AtomicUsize,
    pub map_record_set_estimated_record_copy_bytes: AtomicUsize,
    pub json_set_operations: AtomicUsize,
    pub json_set_copy_on_write_copies: AtomicUsize,
    pub json_set_estimated_copy_bytes: AtomicUsize,
}

#[derive(Debug, Default)]
struct SampledFunctionProfile {
    sampled_frames: u64,
    sampled_instructions: u64,
    sampled_loop_backedges: u64,
    sampled_wall_nanoseconds: u64,
    sampled_host_nanoseconds: u64,
    sampled_child_nanoseconds: u64,
    sampled_guest_nanoseconds: u64,
}

const VM_PROFILE_SAMPLE_RATE_FRAMES: u64 = 1024;

#[inline]
fn vm_profile_sample_hash(frame_index: u64) -> u64 {
    let mut value = frame_index.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

impl RuntimeProfileCounters {
    pub fn initialize_function_profiles(&self, function_names: Vec<String>) {
        let backedges = function_names.iter().map(|_| AtomicU64::new(0)).collect();
        let _ = self.function_profile_names.set(function_names);
        let _ = self.function_loop_backedge_counts.set(backedges);
    }

    pub fn set_sampled_mode(&self, sampled: bool) {
        self.sampled_mode.store(sampled, Ordering::Relaxed);
    }

    #[inline]
    pub fn detailed_counters_enabled(&self) -> bool {
        !self.sampled_mode.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn should_sample_host_operation(&self) -> bool {
        if self.detailed_counters_enabled() {
            return true;
        }
        let event_index = self.host_operation_events.fetch_add(1, Ordering::Relaxed) as u64;
        event_index == 0
            || vm_profile_sample_hash(event_index) & (VM_PROFILE_SAMPLE_RATE_FRAMES - 1) == 0
    }

    #[inline]
    pub fn record_opcode_execution(&self, name: &'static str) {
        if !self.detailed_counters_enabled() {
            return;
        }
        *self
            .opcode_executions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entry(name)
            .or_default() += 1;
    }

    #[inline]
    pub fn record_opcode_transition(
        &self,
        function_name: &str,
        previous: &'static str,
        current: &'static str,
    ) {
        if !self.detailed_counters_enabled() {
            return;
        }
        let key = format!("{function_name}::{previous}->{current}");
        *self
            .opcode_transition_counts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entry(key)
            .or_default() += 1;
    }

    #[inline]
    pub fn record_function_call(&self, name: &str) {
        if !self.detailed_counters_enabled() {
            return;
        }
        *self
            .function_call_counts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entry(name.to_owned())
            .or_default() += 1;
    }

    #[inline]
    pub fn should_sample_vm_frame(&self) -> bool {
        let frame_index = self.vm_interpreter_frames.fetch_add(1, Ordering::Relaxed) as u64;
        frame_index == 0
            || vm_profile_sample_hash(frame_index) & (VM_PROFILE_SAMPLE_RATE_FRAMES - 1) == 0
    }

    #[inline]
    pub fn record_vm_sample(
        &self,
        function_name: &str,
        instructions: usize,
        loop_backedges: usize,
        wall_nanoseconds: u64,
        host_nanoseconds: u64,
        child_nanoseconds: u64,
    ) {
        let host_nanoseconds = host_nanoseconds.min(wall_nanoseconds);
        let child_nanoseconds = child_nanoseconds.min(wall_nanoseconds - host_nanoseconds);
        let guest_nanoseconds = wall_nanoseconds
            .saturating_sub(host_nanoseconds)
            .saturating_sub(child_nanoseconds);
        self.vm_sampled_frames.fetch_add(1, Ordering::Relaxed);
        self.vm_sampled_instructions
            .fetch_add(instructions, Ordering::Relaxed);
        self.vm_sampled_nanoseconds
            .fetch_add(wall_nanoseconds, Ordering::Relaxed);
        let mut sampled_functions = self
            .sampled_function_counts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let function = sampled_functions
            .entry(function_name.to_owned())
            .or_default();
        function.sampled_frames += 1;
        function.sampled_instructions += instructions as u64;
        function.sampled_loop_backedges += loop_backedges as u64;
        function.sampled_wall_nanoseconds += wall_nanoseconds;
        function.sampled_host_nanoseconds += host_nanoseconds;
        function.sampled_child_nanoseconds += child_nanoseconds;
        function.sampled_guest_nanoseconds += guest_nanoseconds;
    }

    #[inline]
    pub fn record_function_loop_backedge(&self, function_index: usize) {
        if let Some(count) = self
            .function_loop_backedge_counts
            .get()
            .and_then(|counts| counts.get(function_index))
        {
            count.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn record_host_operation(&self, name: &'static str, nanoseconds: u64) {
        *self
            .host_operation_counts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entry(name)
            .or_default() += 1;
        *self
            .host_operation_nanoseconds
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entry(name)
            .or_default() += nanoseconds;
    }

    #[inline]
    pub fn record_json_parse(&self, nanoseconds: u64) {
        self.json_parse_nanoseconds
            .fetch_add(nanoseconds, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_json_metrics(&self, nanoseconds: u64) {
        self.json_metrics_nanoseconds
            .fetch_add(nanoseconds, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_map_set(&self, in_place: bool, copied_bytes: usize) {
        self.map_set_operations.fetch_add(1, Ordering::Relaxed);
        if in_place {
            self.map_set_in_place_updates
                .fetch_add(1, Ordering::Relaxed);
        } else {
            self.map_set_copy_on_write_copies
                .fetch_add(1, Ordering::Relaxed);
            self.map_set_estimated_copy_bytes
                .fetch_add(copied_bytes, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn record_map_record_get(&self) {
        self.map_record_get_operations
            .fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_map_record_set(&self, map_copy_bytes: usize, record_copy_bytes: usize) {
        self.map_record_set_operations
            .fetch_add(1, Ordering::Relaxed);
        if map_copy_bytes == 0 && record_copy_bytes == 0 {
            self.map_record_set_in_place_updates
                .fetch_add(1, Ordering::Relaxed);
        } else {
            self.map_record_set_copy_on_write_copies
                .fetch_add(1, Ordering::Relaxed);
            self.map_record_set_estimated_map_copy_bytes
                .fetch_add(map_copy_bytes, Ordering::Relaxed);
            self.map_record_set_estimated_record_copy_bytes
                .fetch_add(record_copy_bytes, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn record_json_set(&self, copied_bytes: usize) {
        self.json_set_operations.fetch_add(1, Ordering::Relaxed);
        if copied_bytes > 0 {
            self.json_set_copy_on_write_copies
                .fetch_add(1, Ordering::Relaxed);
            self.json_set_estimated_copy_bytes
                .fetch_add(copied_bytes, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn record_allocation_charge(&self, bytes: usize) {
        self.allocation_charge_events
            .fetch_add(1, Ordering::Relaxed);
        self.estimated_allocation_gross_bytes
            .fetch_add(bytes, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_allocation_refund(&self, bytes: usize) {
        self.estimated_allocation_refunded_bytes
            .fetch_add(bytes, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_list_set_operation(&self) {
        self.list_set_operations.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_list_set_in_place_update(&self) {
        self.list_set_in_place_updates
            .fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_list_set_copy(&self, elements: usize, bytes: usize) {
        self.list_set_copy_on_write_copies
            .fetch_add(1, Ordering::Relaxed);
        self.list_set_estimated_copy_bytes
            .fetch_add(bytes, Ordering::Relaxed);
        let bucket = match elements {
            0..=8 => &self.list_set_copy_size_histogram[0],
            9..=32 => &self.list_set_copy_size_histogram[1],
            33..=128 => &self.list_set_copy_size_histogram[2],
            _ => &self.list_set_copy_size_histogram[3],
        };
        bucket.copies.fetch_add(1, Ordering::Relaxed);
        bucket
            .estimated_copy_bytes
            .fetch_add(bytes, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_record_charge(&self, bytes: usize) {
        self.record_charge_events.fetch_add(1, Ordering::Relaxed);
        self.record_estimated_bytes
            .fetch_add(bytes, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_guest_conditional_branch(&self) {
        self.dynamic_guest_conditional_branches
            .fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn to_json(&self) -> serde_json::Value {
        let sampled_mode = self.sampled_mode.load(Ordering::Relaxed);
        let gross_bytes = self
            .estimated_allocation_gross_bytes
            .load(Ordering::Relaxed);
        let refunded_bytes = self
            .estimated_allocation_refunded_bytes
            .load(Ordering::Relaxed);
        let copy_size_bucket = |bucket: &CopySizeBucket| {
            serde_json::json!({
                "copies": bucket.copies.load(Ordering::Relaxed),
                "estimated_copy_bytes": bucket.estimated_copy_bytes.load(Ordering::Relaxed),
            })
        };
        let opcode_executions = self
            .opcode_executions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .map(|(name, count)| ((*name).to_owned(), serde_json::json!(count)))
            .collect::<serde_json::Map<_, _>>();
        let opcode_transition_counts = self
            .opcode_transition_counts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .map(|(name, count)| (name.clone(), serde_json::json!(count)))
            .collect::<serde_json::Map<_, _>>();
        let function_call_counts = self
            .function_call_counts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .map(|(name, count)| (name.clone(), serde_json::json!(count)))
            .collect::<serde_json::Map<_, _>>();
        let function_loop_backedge_counts = match (
            self.function_profile_names.get(),
            self.function_loop_backedge_counts.get(),
        ) {
            (Some(names), Some(counts)) => names
                .iter()
                .zip(counts)
                .filter_map(|(name, count)| {
                    let count = count.load(Ordering::Relaxed);
                    (count > 0).then(|| (name.clone(), serde_json::json!(count)))
                })
                .collect::<serde_json::Map<_, _>>(),
            _ => serde_json::Map::new(),
        };
        let sampled_functions = self
            .sampled_function_counts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .map(|(name, counts)| {
                (
                    name.clone(),
                    serde_json::json!({
                        "sampled_frames": counts.sampled_frames,
                        "sampled_instructions": counts.sampled_instructions,
                        "sampled_loop_backedges": counts.sampled_loop_backedges,
                        "sampled_wall_nanoseconds": counts.sampled_wall_nanoseconds,
                        "sampled_host_nanoseconds": counts.sampled_host_nanoseconds,
                        "sampled_child_nanoseconds": counts.sampled_child_nanoseconds,
                        "sampled_guest_nanoseconds": counts.sampled_guest_nanoseconds,
                    }),
                )
            })
            .collect::<serde_json::Map<_, _>>();
        let host_operation_counts = self
            .host_operation_counts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .map(|(name, count)| ((*name).to_owned(), serde_json::json!(count)))
            .collect::<serde_json::Map<_, _>>();
        let host_operation_nanoseconds = self
            .host_operation_nanoseconds
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .map(|(name, nanos)| ((*name).to_owned(), serde_json::json!(nanos)))
            .collect::<serde_json::Map<_, _>>();
        serde_json::json!({
            "scope": "latest guest invocation",
            "profile_detail": if sampled_mode { "sampled" } else { "detailed" },
            "estimated_guest_allocation_budget_charges": {
                "successful_events": self.allocation_charge_events.load(Ordering::Relaxed),
                "estimated_gross_bytes": gross_bytes,
                "estimated_refunded_bytes": refunded_bytes,
                "estimated_net_bytes": gross_bytes.saturating_sub(refunded_bytes),
            },
            "list_set": {
                "operations": self.list_set_operations.load(Ordering::Relaxed),
                "in_place_updates": self.list_set_in_place_updates.load(Ordering::Relaxed),
                "copy_on_write_copies": self.list_set_copy_on_write_copies.load(Ordering::Relaxed),
                "estimated_copy_bytes": self.list_set_estimated_copy_bytes.load(Ordering::Relaxed),
                "copy_size_histogram": {
                    "unit": "elements",
                    "buckets": {
                        "0-8": copy_size_bucket(&self.list_set_copy_size_histogram[0]),
                        "9-32": copy_size_bucket(&self.list_set_copy_size_histogram[1]),
                        "33-128": copy_size_bucket(&self.list_set_copy_size_histogram[2]),
                        "129+": copy_size_bucket(&self.list_set_copy_size_histogram[3]),
                    },
                },
            },
            "record_charges": {
                "events": self.record_charge_events.load(Ordering::Relaxed),
                "estimated_bytes": self.record_estimated_bytes.load(Ordering::Relaxed),
            },
            "dynamic_guest_conditional_branch_executions": self.dynamic_guest_conditional_branches.load(Ordering::Relaxed),
            "opcode_executions": opcode_executions,
            "opcode_transition_counts": opcode_transition_counts,
            "function_call_counts": function_call_counts,
            "function_loop_backedge_counts": function_loop_backedge_counts,
            "vm_interpreter": {
                "instructions": if sampled_mode {
                    serde_json::Value::Null
                } else {
                    serde_json::json!(self.vm_interpreter_instructions.load(Ordering::Relaxed))
                },
                "instructions_are_sampled": sampled_mode,
                "frames": self.vm_interpreter_frames.load(Ordering::Relaxed),
                "sampled_frames": self.vm_sampled_frames.load(Ordering::Relaxed),
                "sampled_instructions": self.vm_sampled_instructions.load(Ordering::Relaxed),
                "sampled_nanoseconds": self.vm_sampled_nanoseconds.load(Ordering::Relaxed),
                "sample_rate_frames": VM_PROFILE_SAMPLE_RATE_FRAMES,
                "result_option_probe_fast_paths": self
                    .vm_result_option_probe_fast_paths
                    .load(Ordering::Relaxed),
                "result_option_unwrap_fast_paths": self
                    .vm_result_option_unwrap_fast_paths
                    .load(Ordering::Relaxed),
                "record_field_load_fast_paths": self
                    .vm_record_field_load_fast_paths
                    .load(Ordering::Relaxed),
                "list_index_load_fast_paths": self
                    .vm_list_index_load_fast_paths
                    .load(Ordering::Relaxed),
                "text_chars_concat_fast_paths": self
                    .vm_text_chars_concat_fast_paths
                    .load(Ordering::Relaxed),
                "sampled_wall_time_is_inclusive_of_host_operations": true,
                "sampled_functions": sampled_functions,
            },
            "host_operations": {
                "timings_are_sampled": sampled_mode,
                "sample_rate_operations": if sampled_mode { VM_PROFILE_SAMPLE_RATE_FRAMES } else { 1 },
                "counts": host_operation_counts,
                "inclusive_nanoseconds": host_operation_nanoseconds,
            },
            "json_phase_nanoseconds": {
                "parse_and_reserved_key_handling": self.json_parse_nanoseconds.load(Ordering::Relaxed),
                "post_parse_metrics_traversal": self.json_metrics_nanoseconds.load(Ordering::Relaxed),
            },
            "map_set": {
                "operations": self.map_set_operations.load(Ordering::Relaxed),
                "in_place_updates": self.map_set_in_place_updates.load(Ordering::Relaxed),
                "copy_on_write_copies": self.map_set_copy_on_write_copies.load(Ordering::Relaxed),
                "estimated_copy_bytes": self.map_set_estimated_copy_bytes.load(Ordering::Relaxed),
            },
            "map_record_update": {
                "reads": self.map_record_get_operations.load(Ordering::Relaxed),
                "operations": self.map_record_set_operations.load(Ordering::Relaxed),
                "in_place_updates": self.map_record_set_in_place_updates.load(Ordering::Relaxed),
                "copy_on_write_copies": self.map_record_set_copy_on_write_copies.load(Ordering::Relaxed),
                "estimated_map_copy_bytes": self.map_record_set_estimated_map_copy_bytes.load(Ordering::Relaxed),
                "estimated_record_copy_bytes": self.map_record_set_estimated_record_copy_bytes.load(Ordering::Relaxed),
            },
            "json_set": {
                "operations": self.json_set_operations.load(Ordering::Relaxed),
                "copy_on_write_copies": self.json_set_copy_on_write_copies.load(Ordering::Relaxed),
                "estimated_copy_bytes": self.json_set_estimated_copy_bytes.load(Ordering::Relaxed),
            },
            "metric_limits": {
                "allocation": "estimated guest-data budget charges, not host allocator calls",
                "branches": "executed guest conditional branches, not hardware branch-misprediction counts",
                "vm_sampled_time": "sampled wall time includes host calls and inline fast-forwarded guest children; sampled_functions subtracts both to estimate exclusive guest time; deterministic hash sampling targets one in 1024 frames and always includes the first frame; attribution only, not final benchmark timing",
                "vm_instruction_counts": if sampled_mode { "exact instruction count omitted in sampled mode; sampled_instructions records selected frames" } else { "exact executed instruction count" },
                "opcode_counts": if sampled_mode { "exact opcode counts are omitted in sampled mode; use detailed mode" } else { "exact executed opcode counts" },
                "opcode_transitions": if sampled_mode { "opcode transitions are omitted in sampled mode; use detailed mode" } else { "exact adjacent opcode transitions attributed to the current function" },
                "function_calls": if sampled_mode { "exact function call counts are omitted in sampled mode; use detailed mode" } else { "exact function call counts" },
                "host_operation_time": if sampled_mode { "sampled inclusive time at the host-op boundary; counts and time are sampled together; nested parse/metrics timers overlap and must not be summed" } else { "inclusive time at host-op boundary; nested parse/metrics timers overlap and must not be summed" },
            },
        })
    }
}

struct VmFrameProfileTimer {
    profile: Arc<RuntimeProfileCounters>,
    function_name: String,
    started: Instant,
    instructions: Cell<usize>,
    loop_backedges: Cell<usize>,
    host_nanoseconds: Cell<u64>,
    child_nanoseconds: Cell<u64>,
}

impl VmFrameProfileTimer {
    fn new(profile: Arc<RuntimeProfileCounters>, function_name: &str) -> Self {
        Self {
            profile,
            function_name: function_name.to_owned(),
            started: Instant::now(),
            instructions: Cell::new(0),
            loop_backedges: Cell::new(0),
            host_nanoseconds: Cell::new(0),
            child_nanoseconds: Cell::new(0),
        }
    }

    #[inline]
    fn record_instruction(&self) {
        self.instructions.set(self.instructions.get() + 1);
    }

    #[inline]
    fn record_loop_backedge(&self) {
        self.loop_backedges.set(self.loop_backedges.get() + 1);
    }

    #[inline]
    fn record_host_duration(&self, duration: std::time::Duration) {
        let elapsed = duration.as_nanos().min(u64::MAX as u128) as u64;
        self.host_nanoseconds
            .set(self.host_nanoseconds.get().saturating_add(elapsed));
    }

    #[inline]
    fn record_child_duration(&self, duration: std::time::Duration) {
        let elapsed = duration.as_nanos().min(u64::MAX as u128) as u64;
        self.child_nanoseconds
            .set(self.child_nanoseconds.get().saturating_add(elapsed));
    }
}

impl Drop for VmFrameProfileTimer {
    fn drop(&mut self) {
        let loop_backedges = self.loop_backedges.get();
        self.profile.record_vm_sample(
            &self.function_name,
            self.instructions.get(),
            loop_backedges,
            self.started.elapsed().as_nanos().min(u64::MAX as u128) as u64,
            self.host_nanoseconds.get(),
            self.child_nanoseconds.get(),
        );
    }
}

pub fn opcode_profile_name(op: &Op) -> &'static str {
    match op {
        Op::Const(_) => "Const",
        Op::Load(_) => "Load",
        Op::Store(_) => "Store",
        Op::Pop => "Pop",
        Op::Add => "Add",
        Op::Subtract => "Subtract",
        Op::Multiply => "Multiply",
        Op::Negate => "Negate",
        Op::Not => "Not",
        Op::Equal => "Equal",
        Op::NotEqual => "NotEqual",
        Op::Less => "Less",
        Op::LessEqual => "LessEqual",
        Op::Greater => "Greater",
        Op::GreaterEqual => "GreaterEqual",
        Op::Jump(_) => "Jump",
        Op::JumpIfFalse(_) => "JumpIfFalse",
        Op::Call(_, _) => "Call",
        Op::Print => "Print",
        Op::PrintErr => "PrintErr",
        Op::Return => "Return",
        Op::Length => "Length",
        Op::Index => "Index",
        Op::MakeList(_) => "MakeList",
        Op::MakeMap(_) => "MakeMap",
        Op::MakeTypedList(_) => "MakeTypedList",
        Op::MakeTypedMap(_) => "MakeTypedMap",
        Op::MapKeys => "MapKeys",
        Op::MapSet => "MapSet",
        Op::MapRecordGet(_, _, _) => "MapRecordGet",
        Op::MapRecordSet(_, _, _) => "MapRecordSet",
        Op::MapHas => "MapHas",
        Op::ListPush => "ListPush",
        Op::ListPushLocal(_) => "ListPushLocal",
        Op::ListSet => "ListSet",
        Op::ListSetLocal(_) => "ListSetLocal",
        Op::Divide => "Divide",
        Op::FloatSqrt => "FloatSqrt",
        Op::JsonAsFloat => "JsonAsFloat",
        Op::IntegerDivide => "IntegerDivide",
        Op::FloorDivide => "FloorDivide",
        Op::Remainder => "Remainder",
        Op::MakeRecord(_, _) => "MakeRecord",
        Op::GetField(_, _) => "GetField",
        Op::ParallelMap(_) => "ParallelMap",
        Op::ReadText => "ReadText",
        Op::WriteText => "WriteText",
        Op::WriteTextAtomic => "WriteTextAtomic",
        Op::ReadBytes => "ReadBytes",
        Op::WriteBytes => "WriteBytes",
        Op::Args => "Args",
        Op::ArgsFrom => "ArgsFrom",
        Op::Assert => "Assert",
        Op::IsOk => "IsOk",
        Op::Unwrap => "Unwrap",
        Op::GetError => "GetError",
        Op::TaskIndex => "TaskIndex",
        Op::TaskCode => "TaskCode",
        Op::TaskMessage => "TaskMessage",
        Op::PythonCall(_, _) => "PythonCall",
        Op::PythonCallKw(_, _, _) => "PythonCallKw",
        Op::HttpGet => "HttpGet",
        Op::JsonParse => "JsonParse",
        Op::JsonStringify => "JsonStringify",
        Op::JsonStringifyPairs => "JsonStringifyPairs",
        Op::JsonNull => "JsonNull",
        Op::JsonBool => "JsonBool",
        Op::JsonInt => "JsonInt",
        Op::JsonFloat => "JsonFloat",
        Op::JsonText => "JsonText",
        Op::JsonArray => "JsonArray",
        Op::JsonObject => "JsonObject",
        Op::JsonPush => "JsonPush",
        Op::JsonSet => "JsonSet",
        Op::JsonGet => "JsonGet",
        Op::JsonIndex => "JsonIndex",
        Op::JsonHas => "JsonHas",
        Op::JsonLength => "JsonLength",
        Op::JsonAsText => "JsonAsText",
        Op::JsonAsInt => "JsonAsInt",
        Op::JsonAsBool => "JsonAsBool",
        Op::JsonIsNull => "JsonIsNull",
        Op::JsonIntColumn => "JsonIntColumn",
        Op::JsonTextCategories => "JsonTextCategories",
        Op::JsonAsIntList => "JsonAsIntList",
        Op::JsonProjectColumns => "JsonProjectColumns",
        Op::JsonTextCategoriesValues => "JsonTextCategoriesValues",
        Op::JsonPushLocal(_) => "JsonPushLocal",
        Op::JsonSetLocal(_) => "JsonSetLocal",
        Op::JsonErrorMessage => "JsonErrorMessage",
        Op::JsonErrorLine => "JsonErrorLine",
        Op::JsonErrorColumn => "JsonErrorColumn",
        Op::JsonErrorCode => "JsonErrorCode",
        Op::FileErrorCode => "FileErrorCode",
        Op::FileErrorMessage => "FileErrorMessage",
        Op::FileErrorLine => "FileErrorLine",
        Op::FileErrorColumn => "FileErrorColumn",
        Op::HttpErrorCode => "HttpErrorCode",
        Op::HttpErrorMessage => "HttpErrorMessage",
        Op::MakeNone => "MakeNone",
        Op::MakeSome => "MakeSome",
        Op::IsSome => "IsSome",
        Op::MakeOk => "MakeOk",
        Op::MakeErr => "MakeErr",
        Op::TryResult => "TryResult",
        Op::HttpRequest => "HttpRequest",
        Op::HttpRequestWithTimeout => "HttpRequestWithTimeout",
        Op::HttpRequestBytes => "HttpRequestBytes",
        Op::Environment => "Environment",
        Op::TextChars => "TextChars",
        Op::TextSlice => "TextSlice",
        Op::TextJoin => "TextJoin",
        Op::TextFind => "TextFind",
        Op::TextSplit => "TextSplit",
        Op::TextReplace => "TextReplace",
        Op::TextTrim => "TextTrim",
        Op::Math(function) => function.op_name(),
        Op::Codec(function) => function.op_name(),
        Op::Fs(function) => function.op_name(),
        Op::Regex(function) => function.op_name(),
        Op::Process(function) => function.op_name(),
        Op::Net(function) => function.op_name(),
        Op::Tls(function) => function.op_name(),
        Op::IntToFloat => "IntToFloat",
        Op::FloatToInt => "FloatToInt",
        Op::NumberToText => "NumberToText",
        Op::RandomMix => "RandomMix",
        Op::BytesFromInts => "BytesFromInts",
        Op::BytesFromText => "BytesFromText",
        Op::BytesJoin => "BytesJoin",
        Op::Float64Bits => "Float64Bits",
        Op::FileOpenLines => "FileOpenLines",
        Op::FileNextLine => "FileNextLine",
        Op::FileClose => "FileClose",
        Op::CsvOpen => "CsvOpen",
        Op::CsvNext => "CsvNext",
        Op::CsvEncode => "CsvEncode",
        Op::JsonTryGet => "JsonTryGet",
        Op::JsonTryIndex => "JsonTryIndex",
        Op::JsonTryText => "JsonTryText",
        Op::JsonTryInt => "JsonTryInt",
        Op::JsonTryBool => "JsonTryBool",
        Op::JsonTryFloat => "JsonTryFloat",
        Op::JsonTryGetText => "JsonTryGetText",
        Op::JsonTryGetInt => "JsonTryGetInt",
        Op::TimeParseDate => "TimeParseDate",
        Op::TimeParseTimestamp => "TimeParseTimestamp",
        Op::TimeFormatUtc => "TimeFormatUtc",
        Op::TimeFormatUtcDate => "TimeFormatUtcDate",
        Op::UnixTimeMs => "UnixTimeMs",
        Op::TimeTodayLocal => "TimeTodayLocal",
        Op::TimeResolveLocal => "TimeResolveLocal",
        Op::TimeZoneDataVersion => "TimeZoneDataVersion",
        Op::SemVerParse => "SemVerParse",
        Op::SemVerCompare => "SemVerCompare",
        Op::CliParse => "CliParse",
        Op::FsWalk => "FsWalk",
        Op::ListGetOption => "ListGetOption",
        Op::MapGetOption => "MapGetOption",
        Op::TextParseInt => "TextParseInt",
        Op::TextParseFloat => "TextParseFloat",
        Op::SortBy(_) => "SortBy",
        Op::SortByIntKey(_) => "SortByIntKey",
        Op::MergeSortedByIntKey(_) => "MergeSortedByIntKey",
        Op::TextCaseFold => "TextCaseFold",
        Op::TextCollapseWhitespace => "TextCollapseWhitespace",
        Op::TextNormalize => "TextNormalize",
        Op::CryptoHmacSha256 => "CryptoHmacSha256",
        Op::CryptoVerifyHmacSha256 => "CryptoVerifyHmacSha256",
        Op::SqliteOpen => "SqliteOpen",
        Op::SqliteBegin => "SqliteBegin",
        Op::SqliteExecute => "SqliteExecute",
        Op::SqliteQuery => "SqliteQuery",
        Op::SqliteCommit => "SqliteCommit",
        Op::SqliteRollback => "SqliteRollback",
        Op::SqliteClose => "SqliteClose",
        Op::TomlParse => "TomlParse",
        Op::TomlHas => "TomlHas",
        Op::TomlGetText => "TomlGetText",
        Op::TomlGetInt => "TomlGetInt",
        Op::TomlGetBool => "TomlGetBool",
        Op::TomlPutText => "TomlPutText",
        Op::TomlPutInt => "TomlPutInt",
        Op::TomlPutBool => "TomlPutBool",
        Op::TomlRender => "TomlRender",
        Op::CalendarParse => "CalendarParse",
        Op::HttpResponseText => "HttpResponseText",
        Op::HttpResponseJson => "HttpResponseJson",
    }
}

/// A cloneable, one-invocation cancellation signal for [`Instance::invoke_with_cancellation`].
#[derive(Clone, Debug, Default)]
pub struct CancellationToken {
    pub cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    #[inline]
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    #[inline]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

pub struct VmFrame {
    pub function_index: usize,
    pub depth: usize,
    pub call_site: Option<Span>,
    pub locals: Vec<Value>,
    pub stack: Vec<Value>,
    pub ip: usize,
}

pub enum FrameStart {
    Returned(Value),
    Vm(VmFrame),
    /// Resume a native call chain as a VM stack, ordered outermost to innermost.
    VmFrames(Vec<VmFrame>),
}

pub enum VmStep {
    Call {
        frame: VmFrame,
        target: usize,
        args: Vec<Value>,
        span: Span,
    },
    Return(Value),
}

pub fn append_guest_call_trace(
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

/// The allocation allowance one invocation and its parallel workers draw from. While no
/// workers share it, one thread updates it with plain loads and stores, as an atomic
/// read-modify-write on every allocation costs a locked instruction on x86_64.
#[derive(Debug)]
pub struct AllocationPool {
    remaining: AtomicUsize,
    /// Parallel operations currently sharing this pool with worker threads.
    sharers: AtomicUsize,
}

impl AllocationPool {
    pub fn new(limit: usize) -> Self {
        Self {
            remaining: AtomicUsize::new(limit),
            sharers: AtomicUsize::new(0),
        }
    }

    pub fn remaining(&self) -> usize {
        self.remaining.load(Ordering::Relaxed)
    }

    /// Mark the pool as shared with worker threads; call it before starting them.
    pub fn begin_sharing(&self) {
        self.sharers.fetch_add(1, Ordering::Relaxed);
    }

    /// Call it after every worker started since `begin_sharing` has been joined.
    pub fn end_sharing(&self) {
        self.sharers.fetch_sub(1, Ordering::Relaxed);
    }

    #[inline]
    fn take(&self, bytes: usize, floor: usize) -> bool {
        if self.sharers.load(Ordering::Relaxed) == 0 {
            let left = self.remaining.load(Ordering::Relaxed);
            if bytes > left.saturating_sub(floor) {
                return false;
            }
            self.remaining.store(left - bytes, Ordering::Relaxed);
            return true;
        }
        self.remaining
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |left| {
                (bytes <= left.saturating_sub(floor)).then(|| left - bytes)
            })
            .is_ok()
    }

    #[inline]
    fn give_back(&self, bytes: usize) {
        if self.sharers.load(Ordering::Relaxed) == 0 {
            let left = self.remaining.load(Ordering::Relaxed);
            self.remaining
                .store(left.saturating_add(bytes), Ordering::Relaxed);
        } else {
            self.remaining.fetch_add(bytes, Ordering::Relaxed);
        }
    }
}

pub struct Budget {
    pub remaining: AtomicU64,
    pub allocation: Arc<AllocationPool>,
    pub allocation_floor: usize,
    pub cancellation: CancellationToken,
    pub local_cancellation: Option<Arc<AtomicBool>>,
    pub runtime_profile: Option<Arc<RuntimeProfileCounters>>,
}

const INSTRUCTION_FUEL_RESERVATION: u64 = 4096;

struct InstructionFuel<'a> {
    budget: &'a Budget,
    remaining: u64,
    reserve_locally: bool,
}

impl<'a> InstructionFuel<'a> {
    fn new(budget: &'a Budget) -> Self {
        Self {
            budget,
            remaining: 0,
            // Parallel workers share a bounded child budget. Keep their exact atomic
            // charging so one worker cannot hide fuel from its siblings.
            reserve_locally: budget.local_cancellation.is_none(),
        }
    }

    #[inline]
    fn consume(&mut self) -> bool {
        // `remaining` stays zero under exact atomic charging, so one test covers both.
        if self.remaining != 0 {
            self.remaining -= 1;
            return true;
        }
        self.consume_slow()
    }

    #[cold]
    fn consume_slow(&mut self) -> bool {
        if !self.reserve_locally {
            return self.budget.consume();
        }
        self.remaining = self.budget.reserve(INSTRUCTION_FUEL_RESERVATION);
        if self.remaining == 0 {
            return false;
        }
        self.remaining -= 1;
        true
    }

    /// Reserve more fuel once the local reservation has run out.
    #[inline]
    fn top_up(&mut self) {
        if self.remaining == 0 && self.reserve_locally {
            self.remaining = self.budget.reserve(INSTRUCTION_FUEL_RESERVATION);
        }
    }

    #[inline]
    fn flush(&mut self) {
        if self.remaining != 0 {
            self.budget.refund(self.remaining);
            self.remaining = 0;
        }
    }
}

impl Drop for InstructionFuel<'_> {
    fn drop(&mut self) {
        self.flush();
    }
}

pub struct LimitedString {
    pub value: String,
    pub max_bytes: usize,
}

impl LimitedString {
    #[inline]
    pub fn new(max_bytes: usize) -> Self {
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
    #[inline]
    pub fn new(
        instruction_limit: u64,
        allocation_limit: usize,
        cancellation: CancellationToken,
        runtime_profile: Option<Arc<RuntimeProfileCounters>>,
    ) -> Self {
        Self {
            remaining: AtomicU64::new(instruction_limit),
            allocation: Arc::new(AllocationPool::new(allocation_limit)),
            allocation_floor: 0,
            cancellation,
            local_cancellation: None,
            runtime_profile,
        }
    }

    #[inline]
    pub fn is_cancelled(&self) -> bool {
        self.cancellation.is_cancelled()
            || self
                .local_cancellation
                .as_ref()
                .is_some_and(|flag| flag.load(Ordering::Acquire))
    }
    #[inline]
    pub fn consume(&self) -> bool {
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

    #[inline]
    pub fn reserve(&self, requested: u64) -> u64 {
        self.remaining
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |left| {
                let reserved = left.min(requested);
                Some(left - reserved)
            })
            .map(|left| left.min(requested))
            .unwrap_or(0)
    }

    #[inline]
    pub fn refund(&self, count: u64) {
        self.remaining.fetch_add(count, Ordering::Relaxed);
    }
    #[inline]
    pub fn reserve_parallel(&self) -> u64 {
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

    #[inline]
    pub fn allocate(&self, bytes: usize) -> bool {
        // A zero-byte charge never changes the remaining budget and always fits.
        if bytes == 0 {
            return true;
        }
        let allocated = self.allocation.take(bytes, self.allocation_floor);
        if allocated {
            if let Some(profile) = &self.runtime_profile {
                profile.record_allocation_charge(bytes);
            }
        }
        allocated
    }

    #[inline]
    pub fn refund_allocation(&self, bytes: usize) {
        self.allocation.give_back(bytes);
        if let Some(profile) = &self.runtime_profile {
            profile.record_allocation_refund(bytes);
        }
    }

    #[inline]
    pub fn record_list_set_operation(&self) {
        if let Some(profile) = &self.runtime_profile {
            profile.record_list_set_operation();
        }
    }

    #[inline]
    pub fn record_list_set_in_place_update(&self) {
        if let Some(profile) = &self.runtime_profile {
            profile.record_list_set_in_place_update();
        }
    }

    #[inline]
    pub fn record_list_set_copy(&self, elements: usize, bytes: usize) {
        if let Some(profile) = &self.runtime_profile {
            profile.record_list_set_copy(elements, bytes);
        }
    }

    #[inline]
    pub fn record_record_charge(&self, bytes: usize) {
        if let Some(profile) = &self.runtime_profile {
            profile.record_record_charge(bytes);
        }
    }

    #[inline]
    pub fn record_guest_conditional_branch(&self) {
        if let Some(profile) = &self.runtime_profile {
            profile.record_guest_conditional_branch();
        }
    }

    #[inline]
    pub fn parallel_allocation_floor(&self) -> usize {
        let remaining = self.allocation.remaining();
        let available = remaining.saturating_sub(self.allocation_floor);
        remaining.saturating_sub(available.saturating_mul(3) / 4)
    }
}

#[inline(always)]
pub fn charge_allocation(budget: &Budget, bytes: usize, span: Span) -> Result<(), RuntimeError> {
    if bytes == 0 || budget.allocate(bytes) {
        Ok(())
    } else {
        Err(allocation_limit_error(budget, bytes, span))
    }
}

#[cold]
fn allocation_limit_error(budget: &Budget, bytes: usize, span: Span) -> RuntimeError {
    let remaining = budget.allocation.remaining();
    RuntimeError::new(
        "R_ALLOCATION_LIMIT",
        format!(
            "cumulative allocation budget exhausted during this invocation (requested {bytes} bytes; {remaining} remain)"
        ),
        span,
    )
}

/// The clone charge for the common values, without a call.
#[inline(always)]
fn cloned_value_allocation_fast(value: &Value) -> usize {
    match value {
        Value::Int(_) | Value::Bool(_) | Value::Float64(_) | Value::Unit | Value::OptionNone => 0,
        Value::Json(_) | Value::Map(_) | Value::List(_) | Value::Text(_) => 0,
        _ => cloned_value_allocation(value),
    }
}

/// `Value::clone` for the common values, without a call.
#[inline(always)]
fn clone_value_fast(value: &Value) -> Value {
    match value {
        Value::Int(int) => match int.small() {
            Some(small) => Value::Int(BigInt::from_i64(small)),
            None => value.clone(),
        },
        Value::Bool(flag) => Value::Bool(*flag),
        Value::Float64(float) => Value::Float64(*float),
        Value::Text(text) => Value::Text(SharedText::clone(text)),
        Value::Json(json) => Value::Json(Shared::clone(json)),
        Value::Map(map) => Value::Map(Shared::clone(map)),
        Value::List(items) => Value::List(Shared::clone(items)),
        _ => value.clone(),
    }
}

/// Drop a value, skipping the drop call when it owns nothing.
#[inline(always)]
fn drop_value_fast(value: Value) {
    let owns_nothing = match &value {
        Value::Int(int) => int.small().is_some(),
        Value::Bool(_) | Value::Float64(_) | Value::Unit | Value::OptionNone => true,
        _ => false,
    };
    if owns_nothing {
        std::mem::forget(value);
    }
}

#[inline]
pub fn new_value_allocation(value: &Value) -> usize {
    match value {
        // Integer size and growth are bounded separately by max_integer_digits.
        // Counting transient scalar values here made VM and JIT budgets differ.
        Value::Int(_) => 0,
        Value::Text(text) => size_of::<String>().saturating_add(text.len()),
        Value::Bytes(bytes) => size_of::<Vec<u8>>().saturating_add(bytes.capacity()),
        Value::Float64List(items) => size_of::<Vec<Float64>>()
            .saturating_add(items.len().saturating_mul(size_of::<Float64>())),
        // Records own a fresh field-vector, but nested Lists, Maps, and JSON
        // remain shared. Their payloads were charged when first created.
        Value::Record(name, fields) => size_of::<Vec<Value>>()
            .saturating_add(fields.len().saturating_mul(size_of::<Value>()))
            .saturating_add(name.len()),
        // JSON nodes are immutable and shared by reference; cloning a value allocates no subtree.
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

#[inline]
pub fn cloned_value_allocation(value: &Value) -> usize {
    // Each Option or Result wrapper clones one boxed value around its payload.
    let mut wrappers = 0usize;
    let mut value = value;
    while let Value::OptionSome(inner) | Value::ResultOk(inner) | Value::ResultErr(inner) = value {
        wrappers += 1;
        value = inner;
    }
    wrappers
        .saturating_mul(size_of::<Value>())
        .saturating_add(cloned_payload_allocation(value))
}

#[inline]
fn cloned_payload_allocation(value: &Value) -> usize {
    match value {
        // A text clone shares its immutable backing storage, as Lists, Maps and JSON do,
        // so it allocates nothing. The text was charged once when it was created.
        Value::Int(_) | Value::Text(_) => 0,
        Value::Record(name, _) => size_of::<String>().saturating_add(name.len()),
        Value::OptionSome(_) | Value::ResultOk(_) | Value::ResultErr(_) => {
            unreachable!("wrappers are unwrapped by cloned_value_allocation")
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

#[inline]
fn replace_map_value(values: &mut TextMap, key: &SharedText, value: Value) {
    if let Some(existing_value) = values.get_mut(key) {
        *existing_value = value;
    } else {
        values.insert(SharedText::clone(key), value);
    }
}

#[inline]
pub fn map_clone_allocation(values: &TextMap, new_key: Option<&str>, new_value: &Value) -> usize {
    let mut total = TextMap::ACCOUNTED_HEADER_BYTES;
    for (key, value) in values.iter_unordered() {
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

pub struct JsonMetrics {
    pub depth: usize,
    pub item_count: usize,
    pub oversized_integer: bool,
    pub allocation_bytes: usize,
    pub encoded_len: usize,
}

pub fn encoded_json_string_len(value: &str) -> usize {
    value.bytes().fold(2usize, |size, byte| {
        size.saturating_add(match byte {
            b'"' | b'\\' | b'\x08' | b'\t' | b'\n' | b'\x0c' | b'\r' => 2,
            0..=0x1f => 6,
            _ => 1,
        })
    })
}

pub fn json_metrics(value: &serde_json::Value, max_integer_digits: usize) -> JsonMetrics {
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

#[inline]
pub fn json_exact_integer(value: &SharedJson) -> Option<BigInt> {
    let number = value.as_number()?;
    if let Some(value) = number.as_i64() {
        return Some(BigInt::from_i64(value));
    }
    if let Some(value) = number.as_u64() {
        return Some(BigInt::from_u64(value));
    }
    BigInt::parse(&number.to_string()).ok()
}

pub fn finish_json_value(
    value: serde_json::Value,
    limits: &VmLimits,
    budget: &Budget,
    span: Span,
) -> Result<Value, RuntimeError> {
    let metrics = json_metrics(&value, limits.max_integer_digits);
    validate_json_metrics(&metrics, limits, span)?;
    charge_allocation(budget, metrics.allocation_bytes, span)?;
    Ok(Value::Json(SharedJson::from_serde(value)))
}

pub fn shared_json_metrics(value: &SharedJson, max_integer_digits: usize) -> JsonMetrics {
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
            // Numbers keep their source text, so measuring one needs no formatting.
            let text = number.as_str();
            let digits = text.strip_prefix('-').unwrap_or(text);
            let is_integer = !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit());
            let (len, digits) = (text.len(), is_integer.then_some(digits.len()));
            JsonMetrics {
                depth: 0,
                item_count: 0,
                oversized_integer: digits.is_some_and(|digits| digits > max_integer_digits),
                allocation_bytes: size_of::<SharedJson>().saturating_add(len),
                encoded_len: len,
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
                    .saturating_add(size_of::<Vec<Shared<SharedJson>>>())
                    .saturating_add(
                        items
                            .capacity()
                            .saturating_mul(size_of::<Shared<SharedJson>>()),
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
                    .saturating_add(size_of::<BTreeMap<String, Shared<SharedJson>>>()),
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
                    .saturating_add(size_of::<Shared<SharedJson>>() * 3);
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

pub fn finish_shared_json(
    value: SharedJson,
    limits: &VmLimits,
    budget: &Budget,
    span: Span,
) -> Result<Value, RuntimeError> {
    let metrics_started = budget.runtime_profile.as_ref().map(|_| Instant::now());
    let metrics = shared_json_metrics(&value, limits.max_integer_digits);
    if let (Some(profile), Some(started)) = (&budget.runtime_profile, metrics_started) {
        profile.record_json_metrics(started.elapsed().as_nanos().min(u64::MAX as u128) as u64);
    }
    validate_json_metrics(&metrics, limits, span)?;
    charge_allocation(budget, metrics.allocation_bytes, span)?;
    Ok(Value::Json(Shared::new(value)))
}

pub fn validate_json_metrics(
    metrics: &JsonMetrics,
    limits: &VmLimits,
    span: Span,
) -> Result<(), RuntimeError> {
    if metrics.depth > 64 {
        return Err(RuntimeError::new(
            "R_JSON_DEPTH",
            "JSON nesting exceeds 64 levels",
            span,
        ));
    }
    if metrics.encoded_len > limits.max_text_bytes {
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
    if metrics.item_count > limits.max_collection_items {
        return Err(RuntimeError::new(
            "R_COLLECTION_LIMIT",
            "JSON collection exceeds the configured item limit",
            span,
        ));
    }
    Ok(())
}

pub fn shared_json_push_local_allocation(value: &Shared<SharedJson>) -> usize {
    let SharedJson::Array(items) = value.as_ref() else {
        return 0;
    };
    let len = items.len();
    let mut allocation = if Shared::strong_count(value) > 1 {
        size_of::<SharedJson>()
    } else {
        0
    };
    if Shared::strong_count(items) > 1 {
        allocation = allocation.saturating_add(
            size_of::<Vec<Shared<SharedJson>>>()
                .saturating_add(len.saturating_mul(size_of::<Shared<SharedJson>>())),
        );
    } else if items.capacity() == len {
        allocation = allocation.saturating_add(
            size_of::<Vec<Shared<SharedJson>>>().saturating_add(
                items
                    .capacity()
                    .saturating_mul(2)
                    .max(len.saturating_add(1))
                    .max(4)
                    .saturating_mul(size_of::<Shared<SharedJson>>()),
            ),
        );
    }
    allocation
}

pub fn shared_json_set_local_allocation(value: &Shared<SharedJson>, key: &str) -> usize {
    let SharedJson::Object(fields) = value.as_ref() else {
        return 0;
    };
    let outer_shared = Shared::strong_count(value) > 1;
    let mut allocation = if outer_shared {
        size_of::<SharedJson>()
    } else {
        0
    };
    if outer_shared || Shared::strong_count(fields) > 1 {
        allocation = allocation.saturating_add(size_of::<BTreeMap<String, Shared<SharedJson>>>());
        for existing_key in fields.keys() {
            allocation = allocation
                .saturating_add(size_of::<String>() + existing_key.len())
                .saturating_add(size_of::<Shared<SharedJson>>() * 3);
        }
    }
    allocation
        .saturating_add(size_of::<String>() + key.len())
        .saturating_add(size_of::<Shared<SharedJson>>() * 3)
}

#[inline]
pub fn value_matches(value: &Value, ty: &Type, records: &[guion_model::RecordDefinition]) -> bool {
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
            .iter_unordered()
            .all(|(_, item)| value_matches(item, item_ty, records)),
        (Value::OptionNone, Type::Option(_)) => true,
        (Value::OptionSome(value), Type::Option(item)) => value_matches(value, item, records),
        (Value::ResultOk(value), Type::Result(ok, _)) => value_matches(value, ok, records),
        (Value::ResultErr(value), Type::Result(_, err)) => value_matches(value, err, records),
        (Value::Record(name, fields), Type::Record(expected))
            if name.as_str() == expected.as_str() =>
        {
            records
                .iter()
                .find(|record| record.name.as_str() == name.as_str())
                .is_some_and(|record| {
                    fields.len() == record.fields.len()
                        && fields
                            .iter()
                            .zip(&record.fields)
                            .all(|(value, field)| value_matches(value, &field.ty, records))
                })
        }
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

#[inline(always)]
pub fn value_limit_error(value: &Value, limits: &VmLimits, span: Span) -> Option<RuntimeError> {
    // Scalars within their limits are the common case on arithmetic and comparison
    // results; answer them without entering the recursive walk.
    match value {
        // This check leaves JSON payloads to the boundary check.
        Value::Json(_) => None,
        Value::ResultOk(inner) | Value::OptionSome(inner) => match inner.as_ref() {
            Value::Int(i) if !i.exceeds_digits(limits.max_integer_digits) => None,
            Value::Text(s) if s.len() <= limits.max_text_bytes => None,
            Value::Json(_) | Value::Bool(_) | Value::Float64(_) => None,
            _ => value_limit_error_with_json(value, limits, span, false),
        },
        Value::Int(i) if !i.exceeds_digits(limits.max_integer_digits) => None,
        Value::Text(s) if s.len() <= limits.max_text_bytes => None,
        Value::Bool(_) | Value::Float64(_) | Value::Unit | Value::OptionNone => None,
        // Records of scalars, such as an aggregate row stored back into a map, are checked
        // field by field without entering the recursive walk.
        Value::Record(_, fields)
            if fields
                .iter()
                .all(|field| scalar_within_limits(field, limits)) =>
        {
            None
        }
        _ => value_limit_error_with_json(value, limits, span, false),
    }
}

/// Whether `value` is a scalar within its limits; JSON payloads are left to the boundary
/// check, as `value_limit_error` leaves them.
#[inline(always)]
fn scalar_within_limits(value: &Value, limits: &VmLimits) -> bool {
    match value {
        Value::Int(i) => !i.exceeds_digits(limits.max_integer_digits),
        Value::Text(s) => s.len() <= limits.max_text_bytes,
        Value::Bool(_) | Value::Float64(_) | Value::Unit | Value::OptionNone | Value::Json(_) => {
            true
        }
        _ => false,
    }
}

#[inline]
pub fn value_limit_error_at_boundary(
    value: &Value,
    limits: &VmLimits,
    span: Span,
) -> Option<RuntimeError> {
    match value {
        Value::Int(i) if !i.exceeds_digits(limits.max_integer_digits) => None,
        Value::Text(s) if s.len() <= limits.max_text_bytes => None,
        Value::Bool(_) | Value::Float64(_) | Value::Unit | Value::OptionNone => None,
        _ => value_limit_error_with_json(value, limits, span, true),
    }
}

fn value_limit_error_with_json(
    value: &Value,
    limits: &VmLimits,
    span: Span,
    check_json: bool,
) -> Option<RuntimeError> {
    match value {
        Value::Record(_, fields) => fields
            .iter()
            .find_map(|field| value_limit_error_with_json(field, limits, span, check_json)),
        Value::Int(i) if i.exceeds_digits(limits.max_integer_digits) => Some(RuntimeError::new(
            "R_INTEGER_LIMIT",
            format!(
                "integer has {} digits; configured limit is {}",
                i.digit_count(),
                limits.max_integer_digits
            ),
            span,
        )),
        Value::Text(s) if s.len() > limits.max_text_bytes => Some(RuntimeError::new(
            "R_TEXT_LIMIT",
            format!(
                "text has {} bytes; configured limit is {}",
                s.len(),
                limits.max_text_bytes
            ),
            span,
        )),
        Value::Bytes(bytes) if bytes.len() > limits.max_file_bytes => Some(RuntimeError::new(
            "R_FILE_LIMIT",
            format!(
                "byte value has {} bytes; configured file limit is {}",
                bytes.len(),
                limits.max_file_bytes
            ),
            span,
        )),
        Value::List(items) if items.len() > limits.max_collection_items => Some(RuntimeError::new(
            "R_COLLECTION_LIMIT",
            format!(
                "list has {} items; configured limit is {}",
                items.len(),
                limits.max_collection_items
            ),
            span,
        )),
        Value::List(items) => items
            .iter()
            .find_map(|item| value_limit_error_with_json(item, limits, span, check_json)),
        Value::Float64List(items) if items.len() > limits.max_collection_items => {
            Some(RuntimeError::new(
                "R_COLLECTION_LIMIT",
                format!(
                    "list has {} items; configured limit is {}",
                    items.len(),
                    limits.max_collection_items
                ),
                span,
            ))
        }
        Value::Float64List(_) => None,
        Value::Map(items) if items.len() > limits.max_collection_items => Some(RuntimeError::new(
            "R_COLLECTION_LIMIT",
            format!(
                "map has {} items; configured limit is {}",
                items.len(),
                limits.max_collection_items
            ),
            span,
        )),
        // Which violation is reported depends on key order, so only a violation pays for it.
        Value::Map(items)
            if items.iter_unordered().all(|(key, value)| {
                key.len() <= limits.max_text_bytes
                    && value_limit_error_with_json(value, limits, span, check_json).is_none()
            }) =>
        {
            None
        }
        Value::Map(items) => items.iter().find_map(|(key, value)| {
            if key.len() > limits.max_text_bytes {
                Some(RuntimeError::new(
                    "R_TEXT_LIMIT",
                    "map key exceeds configured text limit",
                    span,
                ))
            } else {
                value_limit_error_with_json(value, limits, span, check_json)
            }
        }),
        Value::OptionSome(value) => value_limit_error_with_json(value, limits, span, check_json),
        Value::ResultOk(value) | Value::ResultErr(value) => {
            value_limit_error_with_json(value, limits, span, check_json)
        }
        Value::Json(value) if check_json => {
            let metrics = shared_json_metrics(value, limits.max_integer_digits);
            validate_json_metrics(&metrics, limits, span).err()
        }
        Value::Json(_) => None,
        Value::TaskError(error)
            if error.message.len() > limits.max_text_bytes
                || error.code.len() > limits.max_text_bytes =>
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

pub fn verify(artifact: &Artifact) -> Result<(), RuntimeError> {
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
        .map(|name| name.as_str())
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
                | Op::JsonSetLocal(s)
                    if s >= f.local_count =>
                {
                    return Err(fail(format!(
                        "function `{}` references missing local {s}",
                        f.name
                    )));
                }
                Op::Jump(t) | Op::JumpIfFalse(t) if t > f.code.len() => {
                    return Err(fail(format!(
                        "function `{}` jumps outside its code",
                        f.name
                    )));
                }
                Op::Call(target, argc)
                    if target >= artifact.functions.len()
                        || argc != artifact.functions[target].parameters.len() =>
                {
                    return Err(fail(format!(
                        "function `{}` contains an invalid call",
                        f.name
                    )));
                }
                Op::ParallelMap(target)
                    if target >= artifact.functions.len()
                        || artifact.functions[target].parameters.len() != 1 =>
                {
                    return Err(fail(format!(
                        "function `{}` contains an invalid parallel worker",
                        f.name
                    )));
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
                    )));
                }
                Op::SortByIntKey(target)
                    if target >= artifact.functions.len()
                        || artifact.functions[target].parameters.len() != 1
                        || artifact.functions[target].result != Type::Int =>
                {
                    return Err(fail(format!(
                        "function `{}` contains an invalid integer sort key function",
                        f.name
                    )));
                }
                Op::MergeSortedByIntKey(target)
                    if target >= artifact.functions.len()
                        || artifact.functions[target].parameters.len() != 1
                        || artifact.functions[target].result != Type::Int =>
                {
                    return Err(fail(format!(
                        "function `{}` contains an invalid sorted-feed key function",
                        f.name
                    )));
                }
                Op::PythonCall(target, argc)
                    if target >= artifact.externals.len()
                        || argc != artifact.externals[target].parameters.len() =>
                {
                    return Err(fail(format!(
                        "function `{}` contains an invalid Python call",
                        f.name
                    )));
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
                    )));
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
    guion_model::validate_sort_comparator_effects(artifact).map_err(fail)?;
    Ok(())
}

fn artifact_type_known(ty: &Type, record_names: &HashSet<guion_model::RecordName>) -> bool {
    match ty {
        Type::Record(name) => record_names.contains(name.as_str()),
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
pub struct FlowState {
    pub stack: Vec<Type>,
    pub locals: Vec<Option<Type>>,
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
        Value::Record(name, _) => Type::Record(name.to_string()),
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
            let first = items.iter_unordered().next()?.1;
            let item_type = value_type(first)?;
            if items
                .iter_unordered()
                .any(|(_, item)| value_type(item).as_ref() != Some(&item_type))
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

fn verify_stack(function: &guion_model::Function, artifact: &Artifact) -> Result<(), String> {
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
            Op::JsonSetLocal(slot) => {
                if pop_type(&mut state.stack, pc)? != Type::Json {
                    return Err(format!("json_set value must be Json at instruction {pc}"));
                }
                if pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!("json_set key must be Text at instruction {pc}"));
                }
                if state.locals.get(*slot).and_then(Option::as_ref) != Some(&Type::Json) {
                    return Err(format!("json_set local is not Json at instruction {pc}"));
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
                    ));
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
            Op::JsonStringifyPairs => {
                if pop_type(&mut state.stack, pc)? != Type::Text
                    || pop_type(&mut state.stack, pc)? != Type::Text
                    || pop_type(&mut state.stack, pc)?
                        != Type::Map(Box::new(Type::List(Box::new(Type::Text))))
                {
                    return Err(format!(
                        "json_stringify_pairs requires Map[Text, List[Text]], Text and Text at instruction {pc}"
                    ));
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
            Op::JsonTryGetText | Op::JsonTryGetInt => {
                if pop_type(&mut state.stack, pc)? != Type::Text
                    || pop_type(&mut state.stack, pc)? != Type::Json
                {
                    return Err(format!(
                        "JSON accessor argument types do not match at instruction {pc}"
                    ));
                }
                let result = if instruction.op == Op::JsonTryGetText {
                    Type::Text
                } else {
                    Type::Int
                };
                state
                    .stack
                    .push(Type::Result(Box::new(result), Box::new(Type::JsonError)));
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
            Op::Math(function) => {
                for _ in 0..function.arity() {
                    if pop_type(&mut state.stack, pc)? != Type::Float64 {
                        return Err(format!(
                            "math functions require Float64 at instruction {pc}"
                        ));
                    }
                }
                state.stack.push(Type::Float64);
            }
            Op::Codec(function) => {
                let (parameters, result) = codec_signature(*function);
                for expected in parameters.iter().rev() {
                    if pop_type(&mut state.stack, pc)? != *expected {
                        return Err(format!(
                            "{} received an operand of the wrong type at instruction {pc}",
                            function.op_name()
                        ));
                    }
                }
                state.stack.push(result);
            }
            Op::Tls(function) => {
                let expected = match function {
                    guion_model::TlsFunction::Connect => {
                        [Type::Text, Type::Int, Type::Int, Type::Text]
                    }
                    guion_model::TlsFunction::Accept => {
                        [Type::Int, Type::Int, Type::Text, Type::Text]
                    }
                };
                for expected in expected.iter().rev() {
                    if pop_type(&mut state.stack, pc)? != *expected {
                        return Err(format!(
                            "{} received an operand of the wrong type at instruction {pc}",
                            function.op_name()
                        ));
                    }
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Int),
                    Box::new(Type::List(Box::new(Type::Text))),
                ));
            }
            Op::Net(function) => {
                let (parameters, ok) = net_signature(*function);
                for expected in parameters.iter().rev() {
                    if pop_type(&mut state.stack, pc)? != *expected {
                        return Err(format!(
                            "{} received an operand of the wrong type at instruction {pc}",
                            function.op_name()
                        ));
                    }
                }
                state.stack.push(Type::Result(
                    Box::new(ok),
                    Box::new(Type::List(Box::new(Type::Text))),
                ));
            }
            Op::Process(function) => {
                let texts = Type::List(Box::new(Type::Text));
                match function {
                    guion_model::ProcessFunction::Run => {
                        let stdin = pop_type(&mut state.stack, pc)?;
                        let arguments = pop_type(&mut state.stack, pc)?;
                        let command = pop_type(&mut state.stack, pc)?;
                        if stdin != Type::Text || arguments != texts || command != Type::Text {
                            return Err(format!(
                                "process_run requires Text, List[Text], Text operands at instruction {pc}"
                            ));
                        }
                        state
                            .stack
                            .push(Type::Result(Box::new(texts.clone()), Box::new(texts)));
                    }
                    guion_model::ProcessFunction::Pid => state.stack.push(Type::Int),
                    guion_model::ProcessFunction::SignalPoll => state.stack.push(Type::Text),
                }
            }
            Op::Regex(function) => {
                for _ in 0..function.arity() {
                    if pop_type(&mut state.stack, pc)? != Type::Text {
                        return Err(format!(
                            "{} requires Text operands at instruction {pc}",
                            function.op_name()
                        ));
                    }
                }
                state.stack.push(regex_result_type(*function));
            }
            Op::Fs(function) => {
                for _ in 0..function.arity() {
                    if pop_type(&mut state.stack, pc)? != Type::Text {
                        return Err(format!(
                            "{} requires Text paths at instruction {pc}",
                            function.op_name()
                        ));
                    }
                }
                state.stack.push(fs_result_type(*function));
            }
            Op::IntToFloat => {
                if pop_type(&mut state.stack, pc)? != Type::Int {
                    return Err(format!("int_to_float requires Int at instruction {pc}"));
                }
                state.stack.push(Type::Float64);
            }
            Op::FloatToInt => {
                if pop_type(&mut state.stack, pc)? != Type::Float64 {
                    return Err(format!("float_to_int requires Float64 at instruction {pc}"));
                }
                state.stack.push(Type::Int);
            }
            Op::NumberToText => {
                if !matches!(pop_type(&mut state.stack, pc)?, Type::Int | Type::Float64) {
                    return Err(format!(
                        "number-to-text conversion requires Int or Float64 at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Text);
            }
            Op::RandomMix => {
                if pop_type(&mut state.stack, pc)? != Type::Int {
                    return Err(format!("random_mix requires Int at instruction {pc}"));
                }
                state.stack.push(Type::Int);
            }
            Op::ListPush => {
                let value = pop_type(&mut state.stack, pc)?;
                match pop_type(&mut state.stack, pc)? {
                    Type::List(item) if *item == value => state.stack.push(Type::List(item)),
                    _ => {
                        return Err(format!(
                            "list_push requires a matching List item at instruction {pc}"
                        ));
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
                        ));
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
            Op::TextSlice => {
                if pop_type(&mut state.stack, pc)? != Type::Int
                    || pop_type(&mut state.stack, pc)? != Type::Int
                    || pop_type(&mut state.stack, pc)? != Type::Text
                {
                    return Err(format!(
                        "text_slice requires Text and Int start/end bounds at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Text);
            }
            Op::TextJoin => {
                let delimiter = pop_type(&mut state.stack, pc)?;
                let values = pop_type(&mut state.stack, pc)?;
                if delimiter != Type::Text || values != Type::List(Box::new(Type::Text)) {
                    return Err(format!(
                        "text_join_native requires List[Text] and Text at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Text);
            }
            Op::TextFind => {
                if pop_type(&mut state.stack, pc)? != Type::Int
                    || pop_type(&mut state.stack, pc)? != Type::Text
                    || pop_type(&mut state.stack, pc)? != Type::Text
                {
                    return Err(format!(
                        "text_find requires Text, Text and an Int start at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Int);
            }
            Op::TextSplit => {
                if pop_type(&mut state.stack, pc)? != Type::Text
                    || pop_type(&mut state.stack, pc)? != Type::Text
                {
                    return Err(format!(
                        "text_split_native requires Text and Text at instruction {pc}"
                    ));
                }
                state.stack.push(Type::List(Box::new(Type::Text)));
            }
            Op::TextReplace => {
                if pop_type(&mut state.stack, pc)? != Type::Text
                    || pop_type(&mut state.stack, pc)? != Type::Text
                    || pop_type(&mut state.stack, pc)? != Type::Text
                {
                    return Err(format!(
                        "text_replace_native requires three Text arguments at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Text);
            }
            Op::TextTrim => {
                if pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!(
                        "text_trim_native requires Text at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Text);
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
                        ));
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
                state.stack.push(Type::Record(record.name.to_string()));
            }
            Op::GetField(record_index, field_index) => {
                let record = artifact.records.get(*record_index).ok_or_else(|| {
                    format!("field access references missing record at instruction {pc}")
                })?;
                let field = record.fields.get(*field_index).ok_or_else(|| {
                    format!("field access references missing field at instruction {pc}")
                })?;
                if pop_type(&mut state.stack, pc)? != Type::Record(record.name.to_string()) {
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
            Op::MapRecordGet(record_index, field_index, nested) => {
                if *nested && pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!(
                        "map_record_get nested key requires Text at instruction {pc}"
                    ));
                }
                if pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!(
                        "map_record_get key requires Text at instruction {pc}"
                    ));
                }
                let record = artifact.records.get(*record_index).ok_or_else(|| {
                    format!("map_record_get references missing record at instruction {pc}")
                })?;
                let field = record.fields.get(*field_index).ok_or_else(|| {
                    format!("map_record_get references missing field at instruction {pc}")
                })?;
                let Type::Map(item_type) = pop_type(&mut state.stack, pc)? else {
                    return Err(format!("map_record_get requires Map at instruction {pc}"));
                };
                let value_type = if *nested {
                    let Type::Map(record_type) = item_type.as_ref() else {
                        return Err(format!(
                            "nested map_record_get requires Map of Maps at instruction {pc}"
                        ));
                    };
                    record_type.as_ref()
                } else {
                    item_type.as_ref()
                };
                if *value_type != Type::Record(record.name.to_string()) {
                    return Err(format!(
                        "map_record_get value type does not match record at instruction {pc}"
                    ));
                }
                state.stack.push(field.ty.clone());
            }
            Op::MapRecordSet(record_index, field_index, nested) => {
                let value_type = pop_type(&mut state.stack, pc)?;
                if *nested && pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!(
                        "map_record_set nested key requires Text at instruction {pc}"
                    ));
                }
                if pop_type(&mut state.stack, pc)? != Type::Text {
                    return Err(format!(
                        "map_record_set key requires Text at instruction {pc}"
                    ));
                }
                let record = artifact.records.get(*record_index).ok_or_else(|| {
                    format!("map_record_set references missing record at instruction {pc}")
                })?;
                let field = record.fields.get(*field_index).ok_or_else(|| {
                    format!("map_record_set references missing field at instruction {pc}")
                })?;
                let Type::Map(item_type) = pop_type(&mut state.stack, pc)? else {
                    return Err(format!("map_record_set requires Map at instruction {pc}"));
                };
                let record_type = if *nested {
                    let Type::Map(record_type) = item_type.as_ref() else {
                        return Err(format!(
                            "nested map_record_set requires Map of Maps at instruction {pc}"
                        ));
                    };
                    record_type.as_ref()
                } else {
                    item_type.as_ref()
                };
                if *record_type != Type::Record(record.name.to_string()) || value_type != field.ty {
                    return Err(format!(
                        "map_record_set types do not match record field at instruction {pc}"
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
                    return Err(format!(
                        "parallel workers cannot accept or return native resources at instruction {pc}"
                    ));
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
            Op::SortByIntKey(target) => {
                let key_function = artifact.functions.get(*target).ok_or_else(|| {
                    format!("sort_by_int_key references missing key function at instruction {pc}")
                })?;
                if key_function.parameters.len() != 1 || key_function.result != Type::Int {
                    return Err(format!(
                        "sort_by_int_key key function must have type (T) -> Int at instruction {pc}"
                    ));
                }
                let Type::List(item) = pop_type(&mut state.stack, pc)? else {
                    return Err(format!(
                        "sort_by_int_key input must be a List at instruction {pc}"
                    ));
                };
                if *item != key_function.parameters[0] {
                    return Err(format!(
                        "sort_by_int_key key function parameter type differs from list item at instruction {pc}"
                    ));
                }
                state.stack.push(Type::List(item));
            }
            Op::MergeSortedByIntKey(target) => {
                let key_function = artifact.functions.get(*target).ok_or_else(|| {
                    format!("merge_sorted_by_int_key references missing key function at instruction {pc}")
                })?;
                if key_function.parameters.len() != 1 || key_function.result != Type::Int {
                    return Err(format!(
                        "merge_sorted_by_int_key key function must have type (T) -> Int at instruction {pc}"
                    ));
                }
                let Type::List(feed_item) = pop_type(&mut state.stack, pc)? else {
                    return Err(format!(
                        "merge_sorted_by_int_key input must be a List of Lists at instruction {pc}"
                    ));
                };
                let Type::List(item) = *feed_item else {
                    return Err(format!(
                        "merge_sorted_by_int_key input must be a List of Lists at instruction {pc}"
                    ));
                };
                if *item != key_function.parameters[0] {
                    return Err(format!(
                        "merge_sorted_by_int_key key function parameter type differs from feed item at instruction {pc}"
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
                    return Err(format!(
                        "SQLite execution requires Transaction, Text SQL, and List[SqlValue] at instruction {pc}"
                    ));
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
                    return Err(format!(
                        "TOML edit requires matching document, path, and value types at instruction {pc}"
                    ));
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
                    return Err(format!(
                        "http_request requires Text method and URL, Map[Text] headers, and Text body at instruction {pc}"
                    ));
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
                    return Err(format!(
                        "http_request_with_timeout requires Text method and URL, Map[Text] headers, Text body, and Int timeout_ms at instruction {pc}"
                    ));
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
                    return Err(format!(
                        "http_request_bytes requires Text method and URL, Map[Text] headers, Bytes body, and Int timeout_ms at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Record("HttpResponse".into())),
                    Box::new(Type::HttpError),
                ));
            }
            Op::HttpResponseText => {
                if pop_type(&mut state.stack, pc)? != Type::Record("HttpResponse".into()) {
                    return Err(format!(
                        "http_response_text requires HttpResponse at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Text),
                    Box::new(Type::HttpError),
                ));
            }
            Op::HttpResponseJson => {
                if pop_type(&mut state.stack, pc)? != Type::Record("HttpResponse".into()) {
                    return Err(format!(
                        "http_response_json_bytes requires HttpResponse at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Json),
                    Box::new(Type::Record("StdError".into())),
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
            Op::TimeFormatUtcDate => {
                if pop_type(&mut state.stack, pc)? != Type::Record("Instant".into()) {
                    return Err(format!(
                        "time_format_utc_date requires Instant at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Text);
            }
            Op::UnixTimeMs => state.stack.push(Type::Int),
            Op::TimeTodayLocal => state.stack.push(Type::Record("Date".into())),
            Op::TimeResolveLocal => {
                if pop_type(&mut state.stack, pc)? != Type::Text
                    || pop_type(&mut state.stack, pc)? != Type::Record("LocalDateTime".into())
                {
                    return Err(format!(
                        "resolve_local requires LocalDateTime and Text at instruction {pc}"
                    ));
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Record("TemporalResolution".into())),
                    Box::new(Type::Record("TimeError".into())),
                ));
            }
            Op::TimeZoneDataVersion => state.stack.push(Type::Text),
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
                        function.result, result, state.stack
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

/// Result of a successful [`PortableHost::invoke`].
#[derive(Clone, Debug, PartialEq)]
pub struct PortableRun {
    pub return_value: Value,
    /// Instructions executed, as charged against the caller's budget.
    pub steps: u64,
}

/// A [`VmHost`] with no filesystem, network, database, clock, or process access.
///
/// It runs the same instruction loop as the native runtime. Pure computation, calls, and
/// standard-output printing work; every other host operation fails with `R_HOST_UNSUPPORTED`.
#[derive(Debug)]
pub struct PortableHost {
    artifact: Arc<Artifact>,
    limits: VmLimits,
    output: String,
    steps_used: u64,
    verified: Option<Result<(), RuntimeError>>,
}

impl PortableHost {
    pub fn new(artifact: Arc<Artifact>, limits: VmLimits) -> Self {
        Self {
            artifact,
            limits,
            output: String::new(),
            steps_used: 0,
            verified: None,
        }
    }

    /// Instructions consumed by the latest invocation, including one that failed.
    pub fn steps_used(&self) -> u64 {
        self.steps_used
    }

    pub fn take_output(&mut self) -> String {
        std::mem::take(&mut self.output)
    }

    /// Run an exported function with at most `max_steps` instructions.
    pub fn invoke(
        &mut self,
        export: &str,
        args: Vec<Value>,
        max_steps: u64,
    ) -> Result<PortableRun, RuntimeError> {
        self.output.clear();
        self.steps_used = 0;
        let artifact = Arc::clone(&self.artifact);
        self.verified
            .get_or_insert_with(|| verify(&artifact))
            .clone()?;
        let Some(index) = artifact.functions.iter().position(|f| f.name == export) else {
            return Err(RuntimeError::new(
                "R_EXPORT_UNKNOWN",
                format!("unknown function `{export}`"),
                Span::default(),
            ));
        };
        let function = &artifact.functions[index];
        if args.len() != function.parameters.len() {
            return Err(RuntimeError::new(
                "R_CALL_ARITY",
                format!(
                    "function `{export}` expects {} arguments, got {}",
                    function.parameters.len(),
                    args.len()
                ),
                Span::default(),
            ));
        }
        let budget = Arc::new(Budget::new(
            max_steps,
            self.limits.allocation_limit_bytes,
            CancellationToken::new(),
            None,
        ));
        for (value, ty) in args.iter().zip(&function.parameters) {
            if !value_matches(value, ty, &artifact.records) {
                return Err(RuntimeError::new(
                    "R_CALL_TYPE",
                    format!("argument type mismatch: expected {ty:?}"),
                    Span::default(),
                ));
            }
            if let Some(error) = value_limit_error_at_boundary(value, &self.limits, Span::default())
            {
                return Err(error);
            }
        }
        let result = match self.start_frame(index, args, 0, &budget, None) {
            Ok(FrameStart::Returned(value)) => Ok(value),
            Ok(FrameStart::Vm(frame)) => run_frames(self, frame, Arc::clone(&budget)),
            Ok(FrameStart::VmFrames(frames)) => {
                run_frames_with_stack(self, frames, Arc::clone(&budget))
            }
            Err(error) => Err(error),
        };
        self.steps_used = max_steps.saturating_sub(budget.remaining.load(Ordering::Relaxed));
        result.map(|return_value| PortableRun {
            return_value,
            steps: self.steps_used,
        })
    }
}

impl VmHost for PortableHost {
    fn artifact(&self) -> &Arc<Artifact> {
        &self.artifact
    }

    fn limits(&self) -> VmLimits {
        self.limits
    }

    fn program_output_bytes(&self) -> usize {
        self.output.len()
    }

    fn write_program_output(
        &mut self,
        stream: OutputStream,
        text: &str,
        span: Span,
    ) -> Result<(), RuntimeError> {
        match stream {
            OutputStream::Stdout => {
                self.output.push_str(text);
                Ok(())
            }
            OutputStream::Stderr => Err(RuntimeError::new(
                "R_HOST_UNSUPPORTED",
                "standard error output is not available in this environment",
                span,
            )),
        }
    }

    fn start_frame(
        &mut self,
        index: usize,
        args: Vec<Value>,
        depth: usize,
        _budget: &Arc<Budget>,
        call_site: Option<Span>,
    ) -> Result<FrameStart, RuntimeError> {
        check_call_depth(&self.artifact, &self.limits, index, depth)?;
        Ok(FrameStart::Vm(new_vm_frame(
            &self.artifact,
            index,
            args,
            depth,
            call_site,
        )))
    }

    fn host_op(&mut self, context: HostOpContext<'_>) -> Result<(), RuntimeError> {
        let debug = format!("{:?}", context.instruction.op);
        let name = debug.split(['(', ' ']).next().unwrap_or(&debug);
        Err(RuntimeError::new(
            "R_HOST_UNSUPPORTED",
            format!("host operation `{name}` is not available in this environment"),
            context.instruction.span,
        ))
    }
}

/// Applies a pure math function. Domain errors and non-finite results are reported, never returned
/// as NaN or infinity, matching `sqrt`.
fn apply_math(
    function: MathFunction,
    a: f64,
    b: f64,
) -> Result<Float64, (&'static str, &'static str)> {
    const DOMAIN: &str = "R_MATH_DOMAIN";
    let value = match function {
        MathFunction::Sin => a.sin(),
        MathFunction::Cos => a.cos(),
        MathFunction::Tan => a.tan(),
        MathFunction::Asin if (-1.0..=1.0).contains(&a) => a.asin(),
        MathFunction::Acos if (-1.0..=1.0).contains(&a) => a.acos(),
        MathFunction::Asin | MathFunction::Acos => {
            return Err((DOMAIN, "asin and acos require a value between -1 and 1"));
        }
        MathFunction::Atan => a.atan(),
        MathFunction::Sinh => a.sinh(),
        MathFunction::Cosh => a.cosh(),
        MathFunction::Tanh => a.tanh(),
        MathFunction::Exp => a.exp(),
        MathFunction::Ln | MathFunction::Log10 | MathFunction::Log2 if a <= 0.0 => {
            return Err((DOMAIN, "logarithms require a positive value"));
        }
        MathFunction::Ln => a.ln(),
        MathFunction::Log10 => a.log10(),
        MathFunction::Log2 => a.log2(),
        MathFunction::Floor => a.floor(),
        MathFunction::Ceil => a.ceil(),
        MathFunction::Round => a.round(),
        MathFunction::Abs => a.abs(),
        MathFunction::Pow => {
            if a == 0.0 && b < 0.0 {
                return Err(("R_DIV_ZERO", "pow of zero to a negative power"));
            }
            if a < 0.0 && b.fract() != 0.0 {
                return Err((
                    DOMAIN,
                    "pow of a negative base requires an integer exponent",
                ));
            }
            a.powf(b)
        }
        MathFunction::Atan2 => a.atan2(b),
        MathFunction::Hypot => a.hypot(b),
    };
    Float64::new(value).ok_or((
        "R_FLOAT_NONFINITE",
        "math function produced a non-finite Float64 value",
    ))
}

/// Truncates toward zero. `Float64` values are always finite, so the result is exact.
fn float_to_bigint(value: f64) -> BigInt {
    let truncated = value.trunc();
    if truncated.abs() < 9.0e18 {
        BigInt::from_i64(truncated as i64)
    } else {
        BigInt::parse(&format!("{truncated:.0}")).unwrap_or_else(|_| BigInt::zero())
    }
}

/// The same shortest round-trip text that `print` shows for a `Float64`.
fn float_to_text(value: f64) -> String {
    format!("{value}")
}

/// The low 64 bits of an `Int`, in two's complement for negative values.
fn bigint_low_u64(value: &BigInt) -> u64 {
    if let Some(small) = value.small() {
        return small as u64;
    }
    let modulus = BigInt::parse("18446744073709551616").expect("2^64 parses");
    match value.floor_div_rem(&modulus) {
        Some((_, remainder)) => remainder.to_string().parse::<u64>().unwrap_or(0),
        None => 0,
    }
}

/// The SplitMix64 output function.
fn splitmix64_mix(state: u64) -> u64 {
    let mut z = state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The `Result[T, FileError]` type a filesystem function pushes.
fn fs_result_type(function: FsFunction) -> Type {
    let ok = match function {
        FsFunction::Exists => Type::Bool,
        FsFunction::Kind | FsFunction::TempDir | FsFunction::TempFile => Type::Text,
        FsFunction::Size | FsFunction::ModifiedMs | FsFunction::Copy => Type::Int,
        FsFunction::ListDir => Type::List(Box::new(Type::Text)),
        FsFunction::MakeDir | FsFunction::Remove | FsFunction::RemoveAll | FsFunction::Rename => {
            Type::Unit
        }
    };
    Type::Result(Box::new(ok), Box::new(Type::FileError))
}

/// Operand types (push order) and success type of a socket function; failures are `[code, message]`.
fn net_signature(function: guion_model::NetFunction) -> (Vec<Type>, Type) {
    use guion_model::NetFunction as N;
    match function {
        N::TcpConnect => (vec![Type::Text, Type::Int, Type::Int], Type::Int),
        N::TcpListen | N::UdpBind => (vec![Type::Text, Type::Int], Type::Int),
        N::TcpAccept => (vec![Type::Int, Type::Int], Type::Int),
        N::LocalPort => (vec![Type::Int], Type::Int),
        N::Send => (vec![Type::Int, Type::Bytes], Type::Int),
        N::Recv => (vec![Type::Int, Type::Int, Type::Int], Type::Bytes),
        N::Close => (vec![Type::Int], Type::Unit),
        N::UdpSendTo => (
            vec![Type::Int, Type::Text, Type::Int, Type::Bytes],
            Type::Int,
        ),
        N::UdpRecvFrom => (
            vec![Type::Int, Type::Int, Type::Int],
            Type::List(Box::new(Type::Bytes)),
        ),
    }
}

/// The type a regex function pushes: `Result[T, Text]` (the error is the pattern's compile
/// message), except `escape`, which cannot fail.
fn regex_result_type(function: RegexFunction) -> Type {
    let ok = match function {
        RegexFunction::IsMatch => Type::Bool,
        RegexFunction::Find => Type::Option(Box::new(Type::List(Box::new(Type::Text)))),
        RegexFunction::FindAll | RegexFunction::Split => Type::List(Box::new(Type::Text)),
        RegexFunction::Replace | RegexFunction::ReplaceFirst => Type::Text,
        RegexFunction::Escape => return Type::Text,
    };
    Type::Result(Box::new(ok), Box::new(Type::Text))
}

const REGEX_MAX_PATTERN_BYTES: usize = 4096;
const REGEX_SIZE_LIMIT: usize = 1 << 20;
const REGEX_CACHE_ENTRIES: usize = 64;

thread_local! {
    static REGEX_CACHE: std::cell::RefCell<std::collections::HashMap<String, regex::Regex>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Compiles `pattern` with bounded program size, reusing a small per-thread cache.
fn compile_regex(pattern: &str) -> Result<regex::Regex, String> {
    if pattern.len() > REGEX_MAX_PATTERN_BYTES {
        return Err(format!(
            "pattern is longer than {REGEX_MAX_PATTERN_BYTES} bytes"
        ));
    }
    REGEX_CACHE.with(|cache| {
        if let Some(found) = cache.borrow().get(pattern) {
            return Ok(found.clone());
        }
        let compiled = regex::RegexBuilder::new(pattern)
            .size_limit(REGEX_SIZE_LIMIT)
            .dfa_size_limit(REGEX_SIZE_LIMIT)
            .build()
            .map_err(|error| error.to_string())?;
        let mut cache = cache.borrow_mut();
        if cache.len() >= REGEX_CACHE_ENTRIES {
            cache.clear();
        }
        cache.insert(pattern.to_string(), compiled.clone());
        Ok(compiled)
    })
}

fn apply_regex(
    function: RegexFunction,
    mut operands: Vec<Value>,
) -> Result<Value, (&'static str, &'static str)> {
    const TYPE: (&str, &str) = (
        "R_TYPE_INVALID",
        "regex function received an invalid operand",
    );
    let text_value = |text: &str| Value::Text(SharedText::from(text));
    let mut texts = Vec::with_capacity(operands.len());
    for operand in operands.drain(..) {
        let Value::Text(text) = operand else {
            return Err(TYPE);
        };
        texts.push(text.to_string());
    }
    if function == RegexFunction::Escape {
        return Ok(text_value(&regex::escape(&texts[0])));
    }
    let compiled = match compile_regex(&texts[0]) {
        Ok(compiled) => compiled,
        Err(message) => return Ok(Value::ResultErr(Box::new(text_value(&message)))),
    };
    let subject = texts[1].as_str();
    let text_list = |items: Vec<&str>| {
        Value::List(Shared::new(
            items.into_iter().map(text_value).collect::<Vec<_>>(),
        ))
    };
    let ok = match function {
        RegexFunction::IsMatch => Value::Bool(compiled.is_match(subject)),
        RegexFunction::Find => match compiled.captures(subject) {
            Some(captures) => Value::OptionSome(Box::new(text_list(
                captures
                    .iter()
                    .map(|group| group.map_or("", |matched| matched.as_str()))
                    .collect(),
            ))),
            None => Value::OptionNone,
        },
        RegexFunction::FindAll => {
            text_list(compiled.find_iter(subject).map(|m| m.as_str()).collect())
        }
        RegexFunction::Replace => text_value(&compiled.replace_all(subject, texts[2].as_str())),
        RegexFunction::ReplaceFirst => text_value(&compiled.replace(subject, texts[2].as_str())),
        RegexFunction::Split => text_list(compiled.split(subject).collect()),
        RegexFunction::Escape => unreachable!("handled above"),
    };
    Ok(Value::ResultOk(Box::new(ok)))
}

/// Operand and result types of a codec function; operands are listed in push order.
fn codec_signature(function: CodecFunction) -> (Vec<Type>, Type) {
    let option = |inner: Type| Type::Option(Box::new(inner));
    match function {
        CodecFunction::Base64Encode
        | CodecFunction::Base64UrlEncode
        | CodecFunction::HexEncode
        | CodecFunction::TextFromBytesLossy => (vec![Type::Bytes], Type::Text),
        CodecFunction::Base64Decode | CodecFunction::Base64UrlDecode | CodecFunction::HexDecode => {
            (vec![Type::Text], option(Type::Bytes))
        }
        CodecFunction::BytesToInts => (vec![Type::Bytes], Type::List(Box::new(Type::Int))),
        CodecFunction::BytesSlice => (vec![Type::Bytes, Type::Int, Type::Int], Type::Bytes),
        CodecFunction::TextFromBytes => (vec![Type::Bytes], option(Type::Text)),
        CodecFunction::PercentEncode => (vec![Type::Text], Type::Text),
        CodecFunction::PercentDecode => (vec![Type::Text], option(Type::Text)),
    }
}

const BASE64_STANDARD: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const BASE64_URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn base64_encode(bytes: &[u8], alphabet: &[u8; 64]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let block = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(alphabet[(block >> 18) as usize & 63] as char);
        out.push(alphabet[(block >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            alphabet[(block >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            alphabet[block as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// Strict decoding: the alphabet must match, the length must be a multiple of four, and padding
/// is required. Trailing bits of the final group are ignored, as in Python's `b64decode`.
fn base64_decode(text: &str, alphabet: &[u8; 64]) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    let mut lookup = [255u8; 256];
    for (index, symbol) in alphabet.iter().enumerate() {
        lookup[usize::from(*symbol)] = index as u8;
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    let groups = bytes.len() / 4;
    for (group, chunk) in bytes.chunks(4).enumerate() {
        let last = group + 1 == groups;
        let padding = chunk.iter().rev().take_while(|byte| **byte == b'=').count();
        if padding > 2 || (padding > 0 && !last) {
            return None;
        }
        let mut block = 0u32;
        for byte in &chunk[..4 - padding] {
            let value = lookup[usize::from(*byte)];
            if value == 255 {
                return None;
            }
            block = (block << 6) | u32::from(value);
        }
        block <<= 6 * padding as u32;
        out.push((block >> 16) as u8);
        if padding < 2 {
            out.push((block >> 8) as u8);
        }
        if padding < 1 {
            out.push(block as u8);
        }
    }
    Some(out)
}

fn hex_decode(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(2) {
        return None;
    }
    let nibble = |byte: u8| match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    };
    bytes
        .chunks(2)
        .map(|pair| Some(nibble(pair[0])? << 4 | nibble(pair[1])?))
        .collect()
}

/// RFC 3986 component encoding: everything except unreserved characters is percent-encoded.
fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let pair = bytes.get(index + 1..index + 3)?;
            let digits = std::str::from_utf8(pair).ok()?;
            out.push(
                u8::from_str_radix(digits, 16)
                    .ok()
                    .filter(|_| pair.iter().all(u8::is_ascii_hexdigit))?,
            );
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn apply_codec(
    function: CodecFunction,
    mut operands: Vec<Value>,
) -> Result<Value, (&'static str, &'static str)> {
    const TYPE: (&str, &str) = (
        "R_TYPE_INVALID",
        "codec function received an invalid operand",
    );
    let text_value = |text: &str| Value::Text(SharedText::from(text));
    let option_of = |value: Option<Value>| match value {
        Some(value) => Value::OptionSome(Box::new(value)),
        None => Value::OptionNone,
    };
    if function == CodecFunction::BytesSlice {
        let (Value::Int(end), Value::Int(start), Value::Bytes(bytes)) = (
            operands.pop().ok_or(TYPE)?,
            operands.pop().ok_or(TYPE)?,
            operands.pop().ok_or(TYPE)?,
        ) else {
            return Err(TYPE);
        };
        let bounds = (start.to_usize(), end.to_usize());
        return match bounds {
            (Some(start), Some(end)) if start <= end && end <= bytes.len() => {
                Ok(Value::Bytes(Shared::new(bytes[start..end].to_vec())))
            }
            _ => Err((
                "R_INDEX_BOUNDS",
                "bytes_slice requires 0 <= start <= end <= len(bytes)",
            )),
        };
    }
    let operand = operands.pop().ok_or(TYPE)?;
    match (function, operand) {
        (CodecFunction::Base64Encode, Value::Bytes(bytes)) => {
            Ok(text_value(&base64_encode(&bytes, BASE64_STANDARD)))
        }
        (CodecFunction::Base64UrlEncode, Value::Bytes(bytes)) => {
            Ok(text_value(&base64_encode(&bytes, BASE64_URL)))
        }
        (CodecFunction::HexEncode, Value::Bytes(bytes)) => {
            let mut out = String::with_capacity(bytes.len() * 2);
            for byte in bytes.iter() {
                out.push_str(&format!("{byte:02x}"));
            }
            Ok(text_value(&out))
        }
        (CodecFunction::Base64Decode, Value::Text(text)) => Ok(option_of(
            base64_decode(&text, BASE64_STANDARD).map(|bytes| Value::Bytes(Shared::new(bytes))),
        )),
        (CodecFunction::Base64UrlDecode, Value::Text(text)) => Ok(option_of(
            base64_decode(&text, BASE64_URL).map(|bytes| Value::Bytes(Shared::new(bytes))),
        )),
        (CodecFunction::HexDecode, Value::Text(text)) => Ok(option_of(
            hex_decode(&text).map(|bytes| Value::Bytes(Shared::new(bytes))),
        )),
        (CodecFunction::BytesToInts, Value::Bytes(bytes)) => Ok(Value::List(Shared::new(
            bytes
                .iter()
                .map(|byte| Value::Int(BigInt::from_i64(i64::from(*byte))))
                .collect(),
        ))),
        (CodecFunction::TextFromBytes, Value::Bytes(bytes)) => {
            Ok(option_of(std::str::from_utf8(&bytes).ok().map(text_value)))
        }
        (CodecFunction::TextFromBytesLossy, Value::Bytes(bytes)) => {
            Ok(text_value(&String::from_utf8_lossy(&bytes)))
        }
        (CodecFunction::PercentEncode, Value::Text(text)) => Ok(text_value(&percent_encode(&text))),
        (CodecFunction::PercentDecode, Value::Text(text)) => Ok(option_of(
            percent_decode(&text).map(|decoded| text_value(&decoded)),
        )),
        _ => Err(TYPE),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use guion_model::{Function, Instruction};

    #[test]
    fn text_search_helpers_match_the_standard_library() {
        let cases = [
            ("a,b,,c", ","),
            ("aaaa", "aa"),
            ("aaa", "aa"),
            ("α::🙂::", "::"),
            ("café|é|", "é"),
            ("no match", "xyz"),
            ("", ","),
            ("{{x}}{{x}}", "{{x}}"),
        ];
        for (text, needle) in cases {
            let offsets = text_match_offsets(text, needle, usize::MAX).unwrap();
            let fields: Vec<String> = text_split_at(text, &offsets, needle.len())
                .into_iter()
                .map(|value| match value {
                    Value::Text(field) => field.to_string(),
                    _ => unreachable!(),
                })
                .collect();
            assert_eq!(fields, text.split(needle).collect::<Vec<_>>(), "{text:?}");
            let count = text_match_count(text, needle);
            assert_eq!(count, text.matches(needle).count(), "{text:?}");
            let total = text.len() - count * needle.len() + count * 2;
            assert_eq!(
                &*text_replace_all(text, needle, "§", total),
                text.replace(needle, "§"),
                "{text:?}"
            );
        }
        assert_eq!(text_match_offsets("a,b,c", ",", 1), None);
        assert_eq!(text_match_offsets("a,b,c", ",", 2), Some(vec![1, 3]));
        let long: SharedText = SharedText::from("é".repeat(5000) + "tea" + &"x".repeat(10) + "tea");
        assert_eq!(text_find_scalar(&long, "tea", 0), Some(Some(5000)));
        assert_eq!(text_find_scalar(&long, "tea", 5001), Some(Some(5013)));
        assert_eq!(text_find_scalar(&long, "x", 5001), Some(Some(5003)));
        assert_eq!(text_find_scalar(&long, "zz", 0), Some(None));
    }

    #[test]
    fn instruction_fuel_refunds_unused_reservation_and_keeps_exact_exhaustion() {
        let budget = Budget::new(5, usize::MAX, CancellationToken::new(), None);
        let mut fuel = InstructionFuel::new(&budget);

        assert!(fuel.consume());
        assert!(fuel.consume());
        assert_eq!(budget.remaining.load(Ordering::Relaxed), 0);
        fuel.flush();
        assert_eq!(budget.remaining.load(Ordering::Relaxed), 3);

        assert!(fuel.consume());
        assert!(fuel.consume());
        assert!(fuel.consume());
        assert!(!fuel.consume());
        drop(fuel);
        assert_eq!(budget.remaining.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn allocation_pool_charges_the_same_whether_or_not_it_is_shared() {
        let pool = Arc::new(AllocationPool::new(100));
        assert!(pool.take(60, 0));
        assert!(!pool.take(41, 0));
        assert!(!pool.take(31, 10));
        pool.give_back(20);
        assert_eq!(pool.remaining(), 60);

        pool.begin_sharing();
        let workers: Vec<_> = (0..4)
            .map(|_| {
                let pool = Arc::clone(&pool);
                std::thread::spawn(move || (0..20).filter(|_| pool.take(1, 0)).count())
            })
            .collect();
        let taken: usize = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .sum();
        pool.end_sharing();
        assert_eq!(taken, 60);
        assert_eq!(pool.remaining(), 0);
        assert!(!pool.take(1, 0));
    }

    #[test]
    fn parallel_worker_instruction_fuel_remains_directly_atomic() {
        let parent = Budget::new(10, usize::MAX, CancellationToken::new(), None);
        let worker = Budget {
            remaining: AtomicU64::new(3),
            allocation: parent.allocation.clone(),
            allocation_floor: 0,
            cancellation: parent.cancellation.clone(),
            local_cancellation: Some(Arc::new(AtomicBool::new(false))),
            runtime_profile: None,
        };
        let mut fuel = InstructionFuel::new(&worker);

        assert!(fuel.consume());
        assert_eq!(worker.remaining.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn vm_profile_sampling_is_deterministic_without_a_fixed_frame_stride() {
        let collect_samples = || {
            let profile = RuntimeProfileCounters::default();
            (0..65_536)
                .filter(|_| profile.should_sample_vm_frame())
                .collect::<Vec<_>>()
        };

        let first = collect_samples();
        let second = collect_samples();
        assert_eq!(first, second);
        assert_eq!(first.first(), Some(&0));
        assert!((40..=88).contains(&first.len()));
        assert!(
            first.iter().any(|frame| frame % 1024 != 0),
            "sampling should not lock onto the same fixed frame stride"
        );
    }

    #[test]
    fn vm_dispatch_profiles_exact_const_and_return_instructions() {
        let artifact = Arc::new(Artifact {
            format_version: Artifact::CURRENT_VERSION,
            build_metadata: None,
            compiled_source_sha256: None,
            source_name: "dispatch-clone-test.gu".into(),
            sources: vec![],
            entry: "main".into(),
            functions: vec![Function {
                name: "main".into(),
                parameters: vec![],
                result: Type::Int,
                local_count: 0,
                code: vec![
                    Instruction {
                        op: Op::Const(Value::Int(BigInt::one())),
                        span: Span::default(),
                    },
                    Instruction {
                        op: Op::Return,
                        span: Span::default(),
                    },
                ],
            }],
            externals: vec![],
            records: vec![],
        });
        let profile = Arc::new(RuntimeProfileCounters::default());
        let mut host = PortableHost::new(Arc::clone(&artifact), VmLimits::default());
        let budget = Arc::new(Budget::new(
            10,
            VmLimits::default().allocation_limit_bytes,
            CancellationToken::new(),
            Some(Arc::clone(&profile)),
        ));

        let value = run_frames(
            &mut host,
            new_vm_frame(&artifact, 0, vec![], 0, None),
            budget,
        )
        .expect("tiny verified program returns normally");

        assert_eq!(value, Value::Int(BigInt::one()));
        assert_eq!(
            profile.vm_interpreter_instructions.load(Ordering::Relaxed),
            2
        );
        let opcodes = profile
            .opcode_executions
            .lock()
            .expect("profile opcode lock is not poisoned");
        assert_eq!(opcodes.get("Const"), Some(&1));
        assert_eq!(opcodes.get("Return"), Some(&1));
    }

    #[test]
    fn vm_frame_reuses_the_call_argument_buffer_for_locals() {
        let artifact = Artifact {
            format_version: Artifact::CURRENT_VERSION,
            build_metadata: None,
            compiled_source_sha256: None,
            source_name: "frame-buffer-test.gu".into(),
            sources: vec![],
            entry: "callee".into(),
            functions: vec![Function {
                name: "callee".into(),
                parameters: vec![Type::Int, Type::Text],
                result: Type::Unit,
                local_count: 2,
                code: vec![],
            }],
            externals: vec![],
            records: vec![],
        };
        let mut args = Vec::with_capacity(8);
        args.push(Value::Int(BigInt::one()));
        args.push(Value::Text(SharedText::from("kept")));

        let frame = new_vm_frame(&artifact, 0, args, 1, None);

        assert_eq!(frame.locals[0], Value::Int(BigInt::one()));
        assert_eq!(frame.locals[1], Value::Text(SharedText::from("kept")));
        assert!(frame.locals.capacity() >= 8);
    }

    #[test]
    fn text_collapse_single_operation_wrapper_is_forwardable() {
        let function = Function {
            name: "unicode_collapse_whitespace".into(),
            parameters: vec![Type::Text],
            result: Type::Text,
            local_count: 1,
            code: vec![
                Instruction {
                    op: Op::Load(0),
                    span: Span::default(),
                },
                Instruction {
                    op: Op::TextCollapseWhitespace,
                    span: Span::default(),
                },
                Instruction {
                    op: Op::Return,
                    span: Span::default(),
                },
            ],
        };

        assert!(fast_host_forwarder(&function).is_some());
    }

    #[test]
    fn fast_host_forwarder_uses_caller_arguments_without_duplicate_stack_values() {
        let function = Function {
            name: "collapse".into(),
            parameters: vec![Type::Text],
            result: Type::Text,
            local_count: 1,
            code: vec![
                Instruction {
                    op: Op::Load(0),
                    span: Span::default(),
                },
                Instruction {
                    op: Op::TextCollapseWhitespace,
                    span: Span::default(),
                },
                Instruction {
                    op: Op::Return,
                    span: Span::default(),
                },
            ],
        };
        let artifact = Arc::new(Artifact {
            format_version: Artifact::CURRENT_VERSION,
            build_metadata: None,
            compiled_source_sha256: None,
            source_name: "forwarder-test.gu".into(),
            sources: vec![],
            entry: "collapse".into(),
            functions: vec![function],
            externals: vec![],
            records: vec![],
        });
        let function = &artifact.functions[0];
        let forwarder = fast_host_forwarder(function).expect("one-op wrapper is forwardable");
        let mut host = ForwarderTestHost {
            artifact: Arc::clone(&artifact),
            observed_stack_len: None,
        };
        let mut stack = vec![Value::Bool(true), Value::Text(SharedText::from("example"))];
        let mut locals = Vec::new();
        let budget = Arc::new(Budget::new(3, 1024, CancellationToken::new(), None));
        let mut instruction_fuel = InstructionFuel::new(&budget);

        let result = execute_fast_host_forwarder::<false, _>(FastHostForwarderExecution {
            host: &mut host,
            artifact: &artifact,
            target: 0,
            forwarder,
            stack: &mut stack,
            locals: &mut locals,
            budget: &budget,
            instruction_fuel: &mut instruction_fuel,
            depth: 1,
        })
        .expect("forwarder returns its host operation result");

        assert_eq!(host.observed_stack_len, Some(2));
        assert_eq!(stack, vec![Value::Bool(true)]);
        assert_eq!(result, Value::Text(SharedText::from("example")));
        drop(instruction_fuel);
        assert_eq!(budget.remaining.load(Ordering::Relaxed), 0);
    }

    struct ForwarderTestHost {
        artifact: Arc<Artifact>,
        observed_stack_len: Option<usize>,
    }

    impl VmHost for ForwarderTestHost {
        fn artifact(&self) -> &Arc<Artifact> {
            &self.artifact
        }

        fn limits(&self) -> VmLimits {
            VmLimits::default()
        }

        fn program_output_bytes(&self) -> usize {
            0
        }

        fn write_program_output(
            &mut self,
            _stream: OutputStream,
            _text: &str,
            _span: Span,
        ) -> Result<(), RuntimeError> {
            Ok(())
        }

        fn start_frame(
            &mut self,
            _index: usize,
            _args: Vec<Value>,
            _depth: usize,
            _budget: &Arc<Budget>,
            _call_site: Option<Span>,
        ) -> Result<FrameStart, RuntimeError> {
            unreachable!("the forwarder test does not call another function")
        }

        fn host_op(&mut self, context: HostOpContext<'_>) -> Result<(), RuntimeError> {
            self.observed_stack_len = Some(context.stack.len());
            let value = context.stack.pop().expect("wrapper input is on the stack");
            context.stack.push(value);
            Ok(())
        }
    }

    struct RegisterTestHost {
        artifact: Arc<Artifact>,
        registers: Option<Arc<RegisterPrograms>>,
    }

    impl VmHost for RegisterTestHost {
        fn artifact(&self) -> &Arc<Artifact> {
            &self.artifact
        }

        fn register_programs(&self) -> Option<Arc<RegisterPrograms>> {
            self.registers.clone()
        }

        fn limits(&self) -> VmLimits {
            VmLimits::default()
        }

        fn program_output_bytes(&self) -> usize {
            0
        }

        fn write_program_output(
            &mut self,
            _stream: OutputStream,
            _text: &str,
            _span: Span,
        ) -> Result<(), RuntimeError> {
            Ok(())
        }

        fn start_frame(
            &mut self,
            _index: usize,
            _args: Vec<Value>,
            _depth: usize,
            _budget: &Arc<Budget>,
            _call_site: Option<Span>,
        ) -> Result<FrameStart, RuntimeError> {
            unreachable!("the fusion test does not call another function")
        }

        fn host_op(&mut self, _context: HostOpContext<'_>) -> Result<(), RuntimeError> {
            unreachable!("the fusion test uses no host operations")
        }
    }

    fn register_test_artifact() -> Arc<Artifact> {
        let ops = vec![
            Op::Const(Value::Int(BigInt::from_i64(7))),
            Op::MakeOk,
            Op::Store(1),
            // probe branch with Not, taken: Ok -> IsOk true -> Not false -> jump
            Op::Load(1),
            Op::IsOk,
            Op::Not,
            Op::JumpIfFalse(9),
            Op::Const(Value::Int(BigInt::from_i64(-1))),
            Op::Store(2),
            // probe branch, not taken
            Op::Load(1),
            Op::IsOk,
            Op::JumpIfFalse(16),
            // unwrap-store
            Op::Load(1),
            Op::Unwrap,
            Op::Store(3),
            Op::Jump(16),
            // Not;JumpIfFalse with a Bool already on the stack
            Op::Const(Value::Bool(true)),
            Op::Not,
            Op::JumpIfFalse(21),
            Op::Const(Value::Int(BigInt::from_i64(-2))),
            Op::Store(2),
            Op::Load(3),
            Op::Return,
        ];
        let code = ops
            .into_iter()
            .enumerate()
            .map(|(index, op)| Instruction {
                op,
                span: Span {
                    source_id: 0,
                    start: index,
                    end: index + 1,
                    line: index + 1,
                    column: 1,
                },
            })
            .collect();
        Arc::new(Artifact {
            format_version: Artifact::CURRENT_VERSION,
            build_metadata: None,
            compiled_source_sha256: None,
            source_name: "register-test.gu".into(),
            sources: vec![],
            entry: "main".into(),
            functions: vec![guion_model::Function {
                name: "main".into(),
                parameters: vec![],
                result: Type::Int,
                local_count: 4,
                code,
            }],
            externals: vec![],
            records: vec![],
        })
    }

    #[test]
    fn register_form_matches_plain_execution_at_every_instruction_limit() {
        let artifact = register_test_artifact();
        let tables = Arc::new(build_register_programs(&artifact));
        assert!(
            tables
                .get(0, &artifact.functions[0])
                .unwrap()
                .covers_instruction_ranges(),
            "the test program must contain register instructions"
        );
        let run = |registers: Option<Arc<RegisterPrograms>>, limit: u64| {
            let mut host = RegisterTestHost {
                artifact: Arc::clone(&artifact),
                registers,
            };
            let budget = Arc::new(Budget::new(limit, 1 << 20, CancellationToken::new(), None));
            let frame = new_vm_frame(&artifact, 0, vec![], 1, None);
            let result = run_frames(&mut host, frame, Arc::clone(&budget));
            let outcome = match result {
                Ok(value) => Ok(value),
                Err(error) => Err((error.code, error.trace)),
            };
            (outcome, budget.remaining.load(Ordering::Relaxed))
        };
        let mut succeeded = false;
        for limit in 0..40 {
            let plain = run(None, limit);
            let registers = run(Some(Arc::clone(&tables)), limit);
            assert_eq!(plain, registers, "instruction limit {limit}");
            succeeded |= plain.0.is_ok();
        }
        assert!(succeeded, "a large enough limit completes the program");
        let (outcome, _) = run(Some(tables), 1000);
        assert_eq!(outcome, Ok(Value::Int(BigInt::from_i64(7))));
    }

    #[test]
    fn register_reads_and_constructor_calls_match_plain_execution_at_every_limit() {
        let span = |index: usize| Span {
            source_id: 0,
            start: index,
            end: index + 1,
            line: index + 1,
            column: 1,
        };
        let function =
            |name: &str, parameters, result, local_count, ops: Vec<Op>| guion_model::Function {
                name: name.into(),
                parameters,
                result,
                local_count,
                code: ops
                    .into_iter()
                    .enumerate()
                    .map(|(index, op)| Instruction {
                        op,
                        span: span(index),
                    })
                    .collect(),
            };
        let int = |value| Value::Int(BigInt::from_i64(value));
        let mut entries = TextMap::new();
        entries.insert("key".to_owned(), Value::Text("value".into()));
        let main = function(
            "main",
            vec![],
            Type::Int,
            4,
            vec![
                Op::Const(Value::Map(Shared::new(entries))),
                Op::Store(0),
                Op::Const(Value::Text("key".into())),
                Op::Store(1),
                Op::Const(Value::List(Shared::new(vec![int(5)]))),
                Op::Store(2),
                Op::Const(int(0)),
                Op::Store(3),
                Op::Load(0),
                Op::Load(1),
                Op::MapHas,
                Op::Pop,
                Op::Load(0),
                Op::Load(1),
                Op::Index,
                Op::Pop,
                Op::Load(2),
                Op::Load(3),
                Op::Index,
                Op::Pop,
                Op::Const(int(1)),
                Op::Call(1, 1),
                Op::Pop,
                Op::Call(2, 0),
                Op::Pop,
                Op::Const(int(7)),
                Op::Return,
            ],
        );
        let some = function(
            "some",
            vec![Type::Int],
            Type::Option(Box::new(Type::Int)),
            1,
            vec![Op::Load(0), Op::MakeSome, Op::Return],
        );
        let none = function(
            "none",
            vec![],
            Type::Option(Box::new(Type::Int)),
            0,
            vec![Op::MakeNone, Op::Return],
        );
        let artifact = Arc::new(Artifact {
            format_version: Artifact::CURRENT_VERSION,
            build_metadata: None,
            compiled_source_sha256: None,
            source_name: "register-reads-test.gu".into(),
            sources: vec![],
            entry: "main".into(),
            functions: vec![main, some, none],
            externals: vec![],
            records: vec![],
        });
        let tables = Arc::new(build_register_programs(&artifact));
        for kind in ["Index", "MapHas", "CallSome", "CallNone"] {
            assert!(
                tables
                    .get(0, &artifact.functions[0])
                    .unwrap()
                    .has_kind(kind),
                "the test program must reach {kind}"
            );
        }
        let run = |registers: Option<Arc<RegisterPrograms>>, instructions: u64, bytes: usize| {
            let mut host = RegisterTestHost {
                artifact: Arc::clone(&artifact),
                registers,
            };
            let budget = Arc::new(Budget::new(
                instructions,
                bytes,
                CancellationToken::new(),
                None,
            ));
            let frame = new_vm_frame(&artifact, 0, vec![], 1, None);
            let outcome = match run_frames(&mut host, frame, Arc::clone(&budget)) {
                Ok(value) => Ok(value),
                Err(error) => Err((error.code, error.trace)),
            };
            (
                outcome,
                budget.remaining.load(Ordering::Relaxed),
                budget.allocation.remaining(),
            )
        };
        for limit in 0..50 {
            let plain = run(None, limit, 1 << 20);
            let registers = run(Some(Arc::clone(&tables)), limit, 1 << 20);
            assert_eq!(plain, registers, "instruction limit {limit}");
        }
        let mut exhausted = false;
        for bytes in 0..200 {
            let plain = run(None, 1000, bytes);
            let registers = run(Some(Arc::clone(&tables)), 1000, bytes);
            // The failing program stops here, so the fuel left over is not observable.
            assert_eq!(
                (&plain.0, plain.2),
                (&registers.0, registers.2),
                "allocation limit {bytes}"
            );
            exhausted |= plain.0.is_err();
        }
        assert!(exhausted, "a small allocation budget must be exhausted");
        assert_eq!(run(Some(tables), 1000, 1 << 20).0, Ok(int(7)));
    }

    #[test]
    fn register_text_operations_match_plain_execution_at_every_limit() {
        let int = |value| Value::Int(BigInt::from_i64(value));
        let text = |value: &str| Value::Text(SharedText::from(value));
        let ops = vec![
            // lines = []; line = "  a" + "," + "b  "; push it twice, once from the local
            Op::Const(Value::List(Shared::new(vec![]))),
            Op::Store(0),
            Op::Const(text("  a")),
            Op::Const(text(",")),
            Op::Add,
            Op::Const(text("b  ")),
            Op::Add,
            Op::Store(1),
            Op::Load(1),
            Op::ListPushLocal(0),
            Op::Load(1),
            Op::Const(text("!")),
            Op::Add,
            Op::ListPushLocal(0),
            // fields = text_split(lines[1], ","); total = len(trim(fields[0])) + len(trim(fields[1]))
            Op::Load(0),
            Op::Const(int(1)),
            Op::Index,
            Op::Const(text(",")),
            Op::TextSplit,
            Op::Store(2),
            Op::Load(2),
            Op::Const(int(0)),
            Op::Index,
            Op::TextTrim,
            Op::Length,
            Op::Load(2),
            Op::Const(int(1)),
            Op::Index,
            Op::TextTrim,
            Op::Length,
            Op::Add,
            Op::Store(3),
            // set s = s + "x" keeps appending in place through the stack interpreter
            Op::Load(1),
            Op::Const(text("x")),
            Op::Add,
            Op::Store(1),
            // total = total + text_find(s, "b", 0) + text_find(s, "zz", 1) + len(lines)
            Op::Load(3),
            Op::Load(1),
            Op::Const(text("b")),
            Op::Const(int(0)),
            Op::TextFind,
            Op::Add,
            Op::Load(1),
            Op::Const(text("zz")),
            Op::Const(int(1)),
            Op::TextFind,
            Op::Add,
            Op::Load(0),
            Op::Length,
            Op::Add,
            // slices and scalars of ASCII and non-ASCII text, including a whole-text slice
            Op::Load(1),
            Op::Const(int(2)),
            Op::Const(int(5)),
            Op::TextSlice,
            Op::Length,
            Op::Add,
            Op::Const(text("héllo")),
            Op::Const(int(1)),
            Op::Const(int(3)),
            Op::TextSlice,
            Op::Length,
            Op::Add,
            Op::Const(text("héllo")),
            Op::Const(int(0)),
            Op::Const(int(5)),
            Op::TextSlice,
            Op::Length,
            Op::Add,
            Op::Const(text("héllo")),
            Op::Const(int(1)),
            Op::Index,
            Op::Length,
            Op::Add,
            Op::Load(1),
            Op::Const(int(2)),
            Op::Index,
            Op::Length,
            Op::Add,
            Op::Return,
        ];
        let artifact = Arc::new(Artifact {
            format_version: Artifact::CURRENT_VERSION,
            build_metadata: None,
            compiled_source_sha256: None,
            source_name: "register-text-test.gu".into(),
            sources: vec![],
            entry: "main".into(),
            functions: vec![guion_model::Function {
                name: "main".into(),
                parameters: vec![],
                result: Type::Int,
                local_count: 4,
                code: ops
                    .into_iter()
                    .enumerate()
                    .map(|(index, op)| Instruction {
                        op,
                        span: Span {
                            source_id: 0,
                            start: index,
                            end: index + 1,
                            line: index + 1,
                            column: 1,
                        },
                    })
                    .collect(),
            }],
            externals: vec![],
            records: vec![],
        });
        let tables = Arc::new(build_register_programs(&artifact));
        for kind in [
            "Add",
            "Index",
            "TextSplit",
            "TextSlice",
            "TextTrim",
            "TextFind",
            "ListPush",
        ] {
            assert!(
                tables
                    .get(0, &artifact.functions[0])
                    .unwrap()
                    .has_kind(kind),
                "the test program must reach {kind}"
            );
        }
        let run = |registers: Option<Arc<RegisterPrograms>>, instructions: u64, bytes: usize| {
            let mut host = RegisterTestHost {
                artifact: Arc::clone(&artifact),
                registers,
            };
            let budget = Arc::new(Budget::new(
                instructions,
                bytes,
                CancellationToken::new(),
                None,
            ));
            let frame = new_vm_frame(&artifact, 0, vec![], 1, None);
            let outcome = match run_frames(&mut host, frame, Arc::clone(&budget)) {
                Ok(value) => Ok(value),
                Err(error) => Err((error.code, error.trace)),
            };
            (
                outcome,
                budget.remaining.load(Ordering::Relaxed),
                budget.allocation.remaining(),
            )
        };
        for limit in 0..110 {
            let plain = run(None, limit, 1 << 20);
            let registers = run(Some(Arc::clone(&tables)), limit, 1 << 20);
            assert_eq!(plain, registers, "instruction limit {limit}");
        }
        let mut exhausted = false;
        for bytes in 0..800 {
            let plain = run(None, 1000, bytes);
            let registers = run(Some(Arc::clone(&tables)), 1000, bytes);
            assert_eq!(
                (&plain.0, plain.2),
                (&registers.0, registers.2),
                "allocation limit {bytes}"
            );
            exhausted |= plain.0.is_err();
        }
        assert!(exhausted, "a small allocation budget must be exhausted");
        // "a" + "b  !" = 5, "  a,b  x" has "b" at 4, "zz" is absent, two lines; then the
        // slices "a,b", "él" and "héllo" and the scalars "é" and "a".
        assert_eq!(
            run(Some(tables), 1000, 1 << 20).0,
            Ok(int(5 + 4 - 1 + 2 + 3 + 2 + 5 + 1 + 1))
        );
    }

    #[test]
    fn register_map_updates_and_threaded_tests_match_plain_execution_at_every_limit() {
        let int = |value| Value::Int(BigInt::from_i64(value));
        let text = |value: &str| Value::Text(SharedText::from(value));
        enum Item {
            Op(Op),
            Jump(&'static str),
            JumpIfFalse(&'static str),
            Label(&'static str),
        }
        use Item::{Jump, JumpIfFalse, Label, Op as O};
        // set m = map_set(m, k, m[k] + addend), with `addend` already on the stack
        let update = |map: usize, key: usize, addend: Vec<Item>| {
            let mut items = vec![O(Op::Load(map)), O(Op::Load(key))];
            items.extend([O(Op::Load(map)), O(Op::Load(key)), O(Op::Index)]);
            items.extend(addend);
            items.extend([O(Op::Add), O(Op::MapSet), O(Op::Store(map))]);
            items
        };
        let add_to_total = |amount| {
            vec![
                O(Op::Load(3)),
                O(Op::Const(int(amount))),
                O(Op::Add),
                O(Op::Store(3)),
            ]
        };
        let compare = |op: Op, right| vec![O(Op::Load(3)), O(Op::Const(int(right))), O(op)];
        let mut items = vec![
            // m = {"a": 1, "s": "x"}, k = "a", r = Ok(5), s = "s"
            O(Op::MakeTypedMap(Type::Int)),
            O(Op::Store(0)),
            O(Op::Const(text("a"))),
            O(Op::Store(1)),
            O(Op::Const(int(5))),
            O(Op::MakeOk),
            O(Op::Store(2)),
            O(Op::Const(text("s"))),
            O(Op::Store(4)),
            O(Op::Load(0)),
            O(Op::Load(1)),
            O(Op::Const(int(1))),
            O(Op::MapSet),
            O(Op::Store(0)),
            O(Op::Load(0)),
            O(Op::Load(4)),
            O(Op::Const(text("x"))),
            O(Op::MapSet),
            O(Op::Store(0)),
        ];
        // in place; then shared with local 5, so copied; then by an unwrapped addend
        items.extend(update(0, 1, vec![O(Op::Const(int(1)))]));
        items.extend([O(Op::Load(0)), O(Op::Store(5))]);
        items.extend(update(0, 1, vec![O(Op::Const(int(1)))]));
        items.extend(update(0, 1, vec![O(Op::Load(2)), O(Op::Unwrap)]));
        // a text entry, which the fused form leaves to the stack interpreter
        items.extend(update(0, 4, vec![O(Op::Const(text("y")))]));
        items.extend([
            O(Op::Load(0)),
            O(Op::Const(text("a"))),
            O(Op::Index),
            O(Op::Store(3)),
        ]);
        // if total == 0 or total > 7, with the right side deciding
        items.extend(compare(Op::Equal, 0));
        items.extend([
            JumpIfFalse("or1-right"),
            O(Op::Const(Value::Bool(true))),
            Jump("or1-end"),
        ]);
        items.push(Label("or1-right"));
        items.extend(compare(Op::Greater, 7));
        items.extend([Label("or1-end"), JumpIfFalse("if1-end")]);
        items.extend(add_to_total(10));
        items.extend([Jump("if1-end"), Label("if1-end")]);
        // if total > 0 or total == 99, with the left side deciding
        items.extend(compare(Op::Greater, 0));
        items.extend([
            JumpIfFalse("or2-right"),
            O(Op::Const(Value::Bool(true))),
            Jump("or2-end"),
        ]);
        items.push(Label("or2-right"));
        items.extend(compare(Op::Equal, 99));
        items.extend([Label("or2-end"), JumpIfFalse("if2-end")]);
        items.extend(add_to_total(100));
        items.extend([Jump("if2-end"), Label("if2-end")]);
        // if total > 0 and total < 1000
        items.extend(compare(Op::Greater, 0));
        items.push(JumpIfFalse("and1-false"));
        items.extend(compare(Op::Less, 1000));
        items.extend([
            Jump("and1-end"),
            Label("and1-false"),
            O(Op::Const(Value::Bool(false))),
        ]);
        items.extend([Label("and1-end"), JumpIfFalse("if3-end")]);
        items.extend(add_to_total(20));
        items.extend([Jump("if3-end"), Label("if3-end")]);
        // if total < 0 and total > 0, false on the left
        items.extend(compare(Op::Less, 0));
        items.push(JumpIfFalse("and2-false"));
        items.extend(compare(Op::Greater, 0));
        items.extend([
            Jump("and2-end"),
            Label("and2-false"),
            O(Op::Const(Value::Bool(false))),
        ]);
        items.extend([Label("and2-end"), JumpIfFalse("if4-end")]);
        items.extend(add_to_total(10_000));
        items.extend([Jump("if4-end"), Label("if4-end")]);
        // if total > 0 and not total == 5
        items.extend(compare(Op::Greater, 0));
        items.push(JumpIfFalse("and3-false"));
        items.extend(compare(Op::Equal, 5));
        items.push(O(Op::Not));
        items.extend([
            Jump("and3-end"),
            Label("and3-false"),
            O(Op::Const(Value::Bool(false))),
        ]);
        items.extend([Label("and3-end"), JumpIfFalse("if5-end")]);
        items.extend(add_to_total(1000));
        items.extend([Jump("if5-end"), Label("if5-end")]);
        // total + len(m["s"]) + the copy's m["a"]
        items.extend([
            O(Op::Load(3)),
            O(Op::Load(0)),
            O(Op::Const(text("s"))),
            O(Op::Index),
        ]);
        items.extend([O(Op::Length), O(Op::Add)]);
        items.extend([
            O(Op::Load(5)),
            O(Op::Const(text("a"))),
            O(Op::Index),
            O(Op::Add),
        ]);
        items.push(O(Op::Return));

        let mut labels = std::collections::HashMap::new();
        let mut position = 0;
        for item in &items {
            match item {
                Label(name) => {
                    labels.insert(*name, position);
                }
                _ => position += 1,
            }
        }
        let ops: Vec<Op> = items
            .into_iter()
            .filter_map(|item| match item {
                O(op) => Some(op),
                Jump(name) => Some(Op::Jump(labels[name])),
                JumpIfFalse(name) => Some(Op::JumpIfFalse(labels[name])),
                Label(_) => None,
            })
            .collect();
        let artifact = Arc::new(Artifact {
            format_version: Artifact::CURRENT_VERSION,
            build_metadata: None,
            compiled_source_sha256: None,
            source_name: "register-map-test.gu".into(),
            sources: vec![],
            entry: "main".into(),
            functions: vec![guion_model::Function {
                name: "main".into(),
                parameters: vec![],
                result: Type::Int,
                local_count: 6,
                code: ops
                    .into_iter()
                    .enumerate()
                    .map(|(index, op)| Instruction {
                        op,
                        span: Span {
                            source_id: 0,
                            start: index,
                            end: index + 1,
                            line: index + 1,
                            column: 1,
                        },
                    })
                    .collect(),
            }],
            externals: vec![],
            records: vec![],
        });
        let tables = Arc::new(build_register_programs(&artifact));
        let program = tables.get(0, &artifact.functions[0]).unwrap();
        for kind in [
            "MapAddLocal { unwrap: false }",
            "MapAddLocal { unwrap: true }",
        ] {
            assert!(program.has_kind(kind), "the test program must reach {kind}");
        }
        // Every `or` and `and` test branches directly, so no `Truth` of a joined value is left.
        assert!(!program.has_kind("Truth"), "a test was not threaded");
        let run = |registers: Option<Arc<RegisterPrograms>>, instructions: u64, bytes: usize| {
            let mut host = RegisterTestHost {
                artifact: Arc::clone(&artifact),
                registers,
            };
            let budget = Arc::new(Budget::new(
                instructions,
                bytes,
                CancellationToken::new(),
                None,
            ));
            let frame = new_vm_frame(&artifact, 0, vec![], 1, None);
            let outcome = match run_frames(&mut host, frame, Arc::clone(&budget)) {
                Ok(value) => Ok(value),
                Err(error) => Err((error.code, error.trace)),
            };
            (
                outcome,
                budget.remaining.load(Ordering::Relaxed),
                budget.allocation.remaining(),
            )
        };
        for limit in 0..200 {
            let plain = run(None, limit, 1 << 20);
            let registers = run(Some(Arc::clone(&tables)), limit, 1 << 20);
            assert_eq!(plain, registers, "instruction limit {limit}");
        }
        assert!(
            run(None, 199, 1 << 20).0.is_ok(),
            "the sweep must reach the end"
        );
        let mut exhausted = false;
        for bytes in 0..1200 {
            let plain = run(None, 1000, bytes);
            let registers = run(Some(Arc::clone(&tables)), 1000, bytes);
            assert_eq!(
                (&plain.0, plain.2),
                (&registers.0, registers.2),
                "allocation limit {bytes}"
            );
            exhausted |= plain.0.is_err();
        }
        assert!(exhausted, "a small allocation budget must be exhausted");
        // m["a"] = 1 + 1 + 1 + 5 = 8, then 8 + 10 + 100 + 20 + 1000; "xy" has 2 scalars, and
        // the copy taken before the last two updates holds 2.
        assert_eq!(
            run(Some(tables), 1000, 1 << 20).0,
            Ok(int(8 + 10 + 100 + 20 + 1000 + 2 + 2))
        );
    }

    #[test]
    fn text_scalar_range_agrees_with_a_scalar_walk() {
        for sample in [
            "",
            "a",
            "plain ascii",
            "héllo wörld",
            "日本語テキスト",
            "a😀b",
        ] {
            let text = SharedText::from(sample);
            let scalars: Vec<usize> = sample
                .char_indices()
                .map(|(byte, _)| byte)
                .chain([sample.len()])
                .collect();
            let length = scalars.len() - 1;
            for start in 0..=length + 1 {
                for end in start..=length + 2 {
                    let expected = (end <= length).then(|| (scalars[start], scalars[end]));
                    assert_eq!(
                        text_scalar_range(&text, start, end),
                        expected,
                        "{sample:?} {start}..{end}"
                    );
                }
            }
            for index in 0..=length {
                assert_eq!(
                    text_scalar_at(&text, index).map(|scalar| scalar.to_string()),
                    sample.chars().nth(index).map(String::from),
                    "{sample:?} [{index}]"
                );
            }
        }
    }

    #[test]
    fn map_update_existing_key_preserves_owned_key_storage() {
        let mut values = TextMap::new();
        let key = SharedText::from("service");
        let original_key_ptr = key.as_ptr();
        values.insert(key, Value::Bool(false));

        replace_map_value(&mut values, &SharedText::from("service"), Value::Bool(true));

        let (stored_key, stored_value) = values
            .get_key_value("service")
            .expect("the existing map key remains present");
        assert_eq!(stored_key.as_ptr(), original_key_ptr);
        assert_eq!(stored_value, &Value::Bool(true));
    }

    #[test]
    fn record_limit_fast_path_agrees_with_the_full_walk() {
        let limits = VmLimits {
            max_integer_digits: 3,
            max_text_bytes: 4,
            max_collection_items: 2,
            ..VmLimits::default()
        };
        let record = |fields: Vec<Value>| Value::Record("Row".into(), Shared::new(fields));
        let text = |text: &str| Value::Text(SharedText::from(text));
        let int = |value: i64| Value::Int(BigInt::from_i64(value));
        let cases = [
            record(vec![]),
            record(vec![int(123), text("ok"), Value::Bool(true), Value::Unit]),
            record(vec![int(1234), text("ok")]),
            record(vec![int(1), text("too long")]),
            record(vec![
                Value::OptionNone,
                Value::Json(Shared::new(SharedJson::Null)),
            ]),
            record(vec![record(vec![text("too long")])]),
            record(vec![Value::OptionSome(Box::new(int(12345)))]),
            record(vec![Value::List(Shared::new(vec![int(1), int(2), int(3)]))]),
        ];
        for value in cases {
            let code = |error: Option<RuntimeError>| error.map(|error| error.code);
            assert_eq!(
                code(value_limit_error(&value, &limits, Span::default())),
                code(value_limit_error_with_json(
                    &value,
                    &limits,
                    Span::default(),
                    false
                )),
                "{value:?}"
            );
        }
    }

    #[test]
    fn cloned_allocation_counts_every_option_and_result_wrapper() {
        let text = Value::Text(SharedText::from("payload"));
        let wrapped = Value::OptionSome(Box::new(Value::ResultOk(Box::new(Value::ResultErr(
            Box::new(text.clone()),
        )))));
        assert_eq!(cloned_value_allocation(&text), 0);
        assert_eq!(cloned_value_allocation(&wrapped), 3 * size_of::<Value>());
        let record = Value::Record("Row".into(), Shared::new(vec![]));
        assert_eq!(
            cloned_value_allocation(&Value::ResultOk(Box::new(record.clone()))),
            size_of::<Value>() + cloned_value_allocation(&record)
        );
    }

    #[test]
    fn json_number_metrics_measure_the_number_as_written() {
        // The parser stores a number's text (normalising exponents), and both `as_str` and
        // `Display` return that stored text.
        for (source, digits_over_three) in [
            ("0", false),
            ("-123", false),
            ("1234", true),
            ("-12345678901234567890123", true),
            ("1.5", false),
            ("-2.50e10", false),
            ("1E400", false),
        ] {
            let number: serde_json::Number = serde_json::from_str(source).unwrap();
            let written = number.to_string();
            assert_eq!(number.as_str(), written);
            let metrics = shared_json_metrics(&SharedJson::Number(number), 3);
            assert_eq!(metrics.encoded_len, written.len(), "{source}");
            assert_eq!(metrics.oversized_integer, digits_over_three, "{source}");
            assert_eq!(
                metrics.allocation_bytes,
                size_of::<SharedJson>() + written.len(),
                "{source}"
            );
        }
    }
}
