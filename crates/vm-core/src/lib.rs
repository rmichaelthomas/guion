//! Portable core of the Guion production bytecode VM.
//!
//! This crate owns the instruction loop, frame stack, value semantics, call-depth and
//! instruction budgets, and the artifact verifier. Everything that touches the outside
//! world is requested through [`VmHost`]; `guion-runtime` implements it with the native
//! filesystem, database, HTTP, calendar, and process services, and [`PortableHost`]
//! implements it for environments such as WebAssembly by rejecting those operations.

use guion_model::{
    Artifact, BigInt, FileErrorValue, Float64, HttpErrorValue, JsonErrorValue, NativeResourceValue,
    Op, RuntimeError, SharedJson, Span, TaskErrorValue, Type, Value,
};
use std::{
    collections::{BTreeMap, HashSet, VecDeque},
    fmt::Write as _,
    mem::size_of,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc, OnceLock,
    },
};

fn shared_text_scalar(scalar: char) -> Arc<str> {
    if !scalar.is_ascii() {
        return Arc::<str>::from(scalar.to_string());
    }

    static ASCII_SCALARS: OnceLock<[OnceLock<Arc<str>>; 128]> = OnceLock::new();
    let scalars = ASCII_SCALARS.get_or_init(|| std::array::from_fn(|_| OnceLock::new()));
    Arc::clone(scalars[scalar as usize].get_or_init(|| Arc::<str>::from(scalar.to_string())))
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
    pub instruction: guion_model::Instruction,
    pub function: &'a guion_model::Function,
    pub stack: &'a mut Vec<Value>,
    pub locals: &'a mut Vec<Value>,
    pub budget: &'a Arc<Budget>,
    pub depth: usize,
}

/// Services the shared VM requests from whatever environment embeds it.
pub trait VmHost {
    fn artifact(&self) -> &Arc<Artifact>;
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
    let mut locals = vec![Value::Unit; f.local_count];
    for (i, arg) in args.into_iter().enumerate() {
        locals[i] = arg;
    }
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
    let mut frames = vec![initial];
    loop {
        let frame = frames.pop().expect("guest frame stack is non-empty");
        let current_call_site = frame.call_site;
        match execute_vm_frame(host, frame, budget.clone()) {
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
    let VmFrame {
        function_index,
        depth,
        call_site,
        mut locals,
        mut stack,
        mut ip,
    } = frame;
    let artifact = Arc::clone(host.artifact());
    let limits = host.limits();
    let f = &artifact.functions[function_index];
    loop {
        if budget.is_cancelled() || host.externally_cancelled() {
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
                if let Some(error) = value_limit_error_at_boundary(&v, &limits, instruction.span) {
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
                        if x.len().saturating_add(y.len()) > limits.max_text_bytes {
                            return Err(RuntimeError::new(
                                "R_TEXT_LIMIT",
                                "text value exceeds the configured byte limit",
                                instruction.span,
                            ));
                        }
                        let mut text = String::with_capacity(x.len() + y.len());
                        text.push_str(&x);
                        text.push_str(&y);
                        Value::Text(text.into())
                    }
                    _ => {
                        return Err(RuntimeError::new(
                            "R_TYPE_INVALID",
                            "invalid operands for addition",
                            instruction.span,
                        ))
                    }
                };
                if let Some(error) = value_limit_error(&v, &limits, instruction.span) {
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
                if let Some(error) = value_limit_error(&v, &limits, instruction.span) {
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
                if let Some(error) = value_limit_error(&v, &limits, instruction.span) {
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
                if let Some(error) = value_limit_error(&value, &limits, instruction.span) {
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
                if count > limits.max_collection_items {
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
                charge_allocation(budget.as_ref(), size_of::<Vec<Float64>>(), instruction.span)?;
                push_stack!(Value::Float64List(Arc::new(Vec::new())));
            }
            Op::MakeTypedList(_) => {
                charge_allocation(budget.as_ref(), size_of::<Vec<Value>>(), instruction.span)?;
                push_stack!(Value::List(Arc::new(Vec::new())));
            }
            Op::MakeMap(count) => {
                if count > limits.max_collection_items {
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
                    if values.insert(key.to_string(), value).is_some() {
                        return Err(RuntimeError::new(
                            "R_MAP_DUPLICATE_KEY",
                            format!("duplicate map key `{key}`"),
                            instruction.span,
                        ));
                    }
                }
                let map = Value::Map(Arc::new(values));
                if let Some(error) = value_limit_error(&map, &limits, instruction.span) {
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
                let record = host.artifact().records.get(record_index).ok_or_else(|| {
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
                let record_bytes = size_of::<Vec<Value>>()
                    .saturating_add(count.saturating_mul(size_of::<Value>()))
                    .saturating_add(record.name.len());
                charge_allocation(budget.as_ref(), record_bytes, instruction.span)?;
                budget.record_record_charge(record_bytes);
                let mut fields = Vec::with_capacity(count);
                for _ in 0..count {
                    fields.push(pop!());
                }
                fields.reverse();
                push_stack!(Value::Record(record.name.clone(), Arc::new(fields)));
            }
            Op::GetField(record_index, field_index) => {
                let record = host.artifact().records.get(record_index).ok_or_else(|| {
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
                    let allocation = values.keys().fold(
                        size_of::<Vec<Value>>()
                            .saturating_add(values.len().saturating_mul(size_of::<Value>())),
                        |total, key| total.saturating_add(size_of::<String>() + key.len()),
                    );
                    charge_allocation(budget.as_ref(), allocation, instruction.span)?;
                    let keys = values
                        .keys()
                        .map(|key| Value::Text(Arc::<str>::from(key.as_str())))
                        .collect::<Vec<_>>();
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
                let local_store_slot = match f.code.get(ip).map(|next| &next.op) {
                        Some(Op::Store(slot))
                            if locals.get(*slot).is_some_and(|local| {
                                matches!(local, Value::Map(current) if Arc::ptr_eq(current, &values))
                            }) =>
                        {
                            Some(*slot)
                        }
                        _ => None,
                    };
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
                    let shared = Arc::strong_count(items) > 1;
                    let allocation = if shared {
                        map_clone_allocation(items.as_ref(), Some(key.as_ref()), &value)
                    } else if contains_key {
                        0
                    } else {
                        size_of::<(String, Value)>().saturating_add(key.len())
                    };
                    charge_allocation(budget.as_ref(), allocation, instruction.span)?;
                    if let Some(error) = value_limit_error(&value, &limits, instruction.span) {
                        return Err(error);
                    }
                    replace_map_value(Arc::make_mut(items), key.as_ref(), value);
                    push_stack!(Value::Map(Arc::clone(items)));
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
                replace_map_value(&mut updated, key.as_ref(), value);
                let map = Value::Map(Arc::new(updated));
                if let Some(error) = value_limit_error(&map, &limits, instruction.span) {
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
                push_stack!(Value::Bool(values.contains_key(key.as_ref())));
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
                push_stack!(Value::Bytes(Arc::new(text.as_bytes().to_vec())));
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
                        let length = text.chars().count();
                        let value = text
                            .chars()
                            .nth(index)
                            .map(|scalar| scalar.to_string())
                            .ok_or_else(|| {
                                RuntimeError::new(
                                    "R_INDEX_BOUNDS",
                                    format!(
                                        "text index {index} is out of range for length {length}"
                                    ),
                                    instruction.span,
                                )
                            })?;
                        let value = Value::Text(value.into());
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
                        Value::Float64List(Arc::new(updated))
                    }
                    _ => unreachable!("list type checked above"),
                };
                if let Some(error) = value_limit_error(&list, &limits, instruction.span) {
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
                        if len >= limits.max_collection_items {
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
                            size_of::<Vec<Value>>()
                                .saturating_add(grown_capacity.saturating_mul(size_of::<Value>()))
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
                        if len >= limits.max_collection_items {
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
                            size_of::<Vec<Float64>>()
                                .saturating_add(grown_capacity.saturating_mul(size_of::<Float64>()))
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
                if let Some(error) = value_limit_error(local, &limits, instruction.span) {
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
                        Value::Float64List(Arc::new(updated))
                    }
                    _ => unreachable!("list type checked above"),
                };
                budget.record_list_set_copy(len, copy_bytes);
                if let Some(error) = value_limit_error(&list, &limits, instruction.span) {
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
                budget.record_list_set_operation();
                match local {
                    Value::List(items) => {
                        let needs_copy = Arc::strong_count(items) > 1;
                        let copy_bytes = size_of::<Vec<Value>>()
                            .saturating_add(len.saturating_mul(size_of::<Value>()));
                        if needs_copy {
                            charge_allocation(budget.as_ref(), copy_bytes, instruction.span)?;
                        }
                        Arc::make_mut(items)[index] = value;
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
                        let needs_copy = Arc::strong_count(items) > 1;
                        let copy_bytes = size_of::<Vec<Float64>>()
                            .saturating_add(len.saturating_mul(size_of::<Float64>()));
                        if needs_copy {
                            charge_allocation(budget.as_ref(), copy_bytes, instruction.span)?;
                        }
                        Arc::make_mut(items)[index] = value;
                        if needs_copy {
                            budget.record_list_set_copy(len, copy_bytes);
                        } else {
                            budget.record_list_set_in_place_update();
                        }
                    }
                    _ => unreachable!("list type checked above"),
                }
                if let Some(error) = value_limit_error(local, &limits, instruction.span) {
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
                    budget.record_guest_conditional_branch();
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
                host.write_program_output(stream, &rendered.value, instruction.span)?;
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
                if let Some(error) = value_limit_error(&result, &limits, instruction.span) {
                    return Err(error);
                }
                return Ok(VmStep::Return(result));
            }
            _ => host.host_op(HostOpContext {
                instruction,
                function: f,
                stack: &mut stack,
                locals: &mut locals,
                budget: &budget,
                depth,
            })?,
        }
    }
}

#[derive(Debug, Default)]
pub struct CopySizeBucket {
    pub copies: AtomicUsize,
    pub estimated_copy_bytes: AtomicUsize,
}

#[derive(Debug, Default)]
pub struct RuntimeProfileCounters {
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
}

impl RuntimeProfileCounters {
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
        serde_json::json!({
            "scope": "latest guest invocation",
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
            "metric_limits": {
                "allocation": "estimated guest-data budget charges, not host allocator calls",
                "branches": "executed guest conditional branches, not hardware branch-misprediction counts",
            },
        })
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

pub struct Budget {
    pub remaining: AtomicU64,
    pub allocation_remaining: Arc<AtomicUsize>,
    pub allocation_floor: usize,
    pub cancellation: CancellationToken,
    pub local_cancellation: Option<Arc<AtomicBool>>,
    pub runtime_profile: Option<Arc<RuntimeProfileCounters>>,
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
            allocation_remaining: Arc::new(AtomicUsize::new(allocation_limit)),
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
        let allocated = self
            .allocation_remaining
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |left| {
                if bytes <= left.saturating_sub(self.allocation_floor) {
                    Some(left - bytes)
                } else {
                    None
                }
            })
            .is_ok();
        if allocated {
            if let Some(profile) = &self.runtime_profile {
                profile.record_allocation_charge(bytes);
            }
        }
        allocated
    }

    #[inline]
    pub fn refund_allocation(&self, bytes: usize) {
        self.allocation_remaining
            .fetch_add(bytes, Ordering::Relaxed);
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
        let remaining = self.allocation_remaining.load(Ordering::Relaxed);
        let available = remaining.saturating_sub(self.allocation_floor);
        remaining.saturating_sub(available.saturating_mul(3) / 4)
    }
}

#[inline]
pub fn charge_allocation(budget: &Budget, bytes: usize, span: Span) -> Result<(), RuntimeError> {
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

#[inline]
pub fn cloned_value_allocation(value: &Value) -> usize {
    match value {
        Value::Int(_) => 0,
        // Keep the existing logical copy quota even though the clone now
        // shares its immutable backing storage.
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

#[inline]
fn replace_map_value(values: &mut BTreeMap<String, Value>, key: &str, value: Value) {
    if let Some(existing_value) = values.get_mut(key) {
        *existing_value = value;
    } else {
        values.insert(key.to_owned(), value);
    }
}

#[inline]
pub fn map_clone_allocation(
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

pub fn finish_shared_json(
    value: SharedJson,
    limits: &VmLimits,
    budget: &Budget,
    span: Span,
) -> Result<Value, RuntimeError> {
    let metrics = shared_json_metrics(&value, limits.max_integer_digits);
    validate_json_metrics(&metrics, limits, span)?;
    charge_allocation(budget, metrics.allocation_bytes, span)?;
    Ok(Value::Json(Arc::new(value)))
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

pub fn shared_json_push_local_allocation(value: &Arc<SharedJson>) -> usize {
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

#[inline]
pub fn value_limit_error(value: &Value, limits: &VmLimits, span: Span) -> Option<RuntimeError> {
    value_limit_error_with_json(value, limits, span, false)
}

#[inline]
pub fn value_limit_error_at_boundary(
    value: &Value,
    limits: &VmLimits,
    span: Span,
) -> Option<RuntimeError> {
    value_limit_error_with_json(value, limits, span, true)
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
        Value::Int(i) if i.digit_count() > limits.max_integer_digits => Some(RuntimeError::new(
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
    guion_model::validate_sort_comparator_effects(artifact).map_err(fail)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_update_existing_key_preserves_owned_key_storage() {
        let mut values = BTreeMap::new();
        let key = String::from("service");
        let original_key_ptr = key.as_ptr();
        values.insert(key, Value::Bool(false));

        replace_map_value(&mut values, "service", Value::Bool(true));

        let (stored_key, stored_value) = values
            .get_key_value("service")
            .expect("the existing map key remains present");
        assert_eq!(stored_key.as_ptr(), original_key_ptr);
        assert_eq!(stored_value, &Value::Bool(true));
    }
}
