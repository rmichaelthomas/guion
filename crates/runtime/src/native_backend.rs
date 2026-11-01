//! Host-native execution for verified numeric functions.
//!
//! The first backend slice compiles verified integer bytecode to machine code.
//! Functions that exceed the machine integer range deopt to the VM, which keeps
//! Guion's arbitrary-precision integer behavior.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;

use super::{NativeListArena, NativeVmContinuationFrame};
use cranelift_codegen::{
    ir::{
        condcodes::{FloatCC, IntCC},
        types, AbiParam, BlockArg, FuncRef, InstBuilder, MemFlagsData, Signature, StackSlotData,
        StackSlotKind, UserFuncName,
    },
    Context,
};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{default_libcall_names, FuncId, Linkage, Module};
use guion_model::{Artifact, Function, Op, Type, Value};
use std::fmt;

pub(crate) const NATIVE_INLINE_LIST_ITEMS: usize = 8;
pub(crate) const NATIVE_BRIDGE_JSON_TRY_TEXT: i64 = 1;
pub(crate) const NATIVE_BRIDGE_JSON_TRY_GET: i64 = 3;
pub(crate) const NATIVE_BRIDGE_JSON_TRY_INT: i64 = 4;
pub(crate) const NATIVE_BRIDGE_JSON_PARSE: i64 = 5;
pub(crate) const NATIVE_BRIDGE_FILE_NEXT_LINE: i64 = 6;
pub(crate) const NATIVE_BRIDGE_ARGS: i64 = 7;
pub(crate) const NATIVE_BRIDGE_TEXT_EQUAL: i64 = 8;
pub(crate) const NATIVE_BRIDGE_PRINT_ERR: i64 = 9;
pub(crate) const NATIVE_BRIDGE_MAKE_TYPED_MAP: i64 = 10;
pub(crate) const NATIVE_BRIDGE_JSON_ARRAY: i64 = 11;
pub(crate) const NATIVE_BRIDGE_MAP_KEYS: i64 = 12;
pub(crate) const NATIVE_BRIDGE_JSON_INDEX: i64 = 13;
pub(crate) const NATIVE_BRIDGE_TEXT_CHARS: i64 = 17;
pub(crate) const NATIVE_BRIDGE_JSON_GET: i64 = 18;
pub(crate) const NATIVE_BRIDGE_JSON_STRINGIFY: i64 = 19;
pub(crate) const NATIVE_BRIDGE_TEXT_CONCAT: i64 = 20;
pub(crate) const NATIVE_BRIDGE_JSON_OBJECT: i64 = 21;
pub(crate) const NATIVE_BRIDGE_MAP_INDEX: i64 = 22;
pub(crate) const NATIVE_BRIDGE_MAP_SET: i64 = 23;
pub(crate) const NATIVE_BRIDGE_JSON_INT: i64 = 24;
pub(crate) const NATIVE_BRIDGE_MAP_HAS: i64 = 25;
pub(crate) const NATIVE_BRIDGE_JSON_TEXT: i64 = 26;
pub(crate) const NATIVE_BRIDGE_FILE_OPEN_LINES: i64 = 27;
pub(crate) const NATIVE_BRIDGE_FILE_CLOSE: i64 = 28;
pub(crate) const NATIVE_BRIDGE_TEXT_COLLAPSE_WHITESPACE: i64 = 29;
pub(crate) const NATIVE_BRIDGE_TIME_PARSE_TIMESTAMP: i64 = 30;
pub(crate) const NATIVE_BRIDGE_TIME_FORMAT_UTC: i64 = 31;
pub(crate) const NATIVE_BRIDGE_TEXT_SLICE: i64 = 32;
pub(crate) const NATIVE_BRIDGE_JSON_SET_LOCAL: i64 = 33;
pub(crate) const NATIVE_BRIDGE_JSON_PUSH_LOCAL: i64 = 34;
pub(crate) const NATIVE_BRIDGE_MAKE_RECORD: i64 = 14;
pub(crate) const NATIVE_BRIDGE_GET_FIELD: i64 = 15;

pub(crate) const NATIVE_DIRECT_OPERATION_NAMES: [&str; 31] = [
    "json_parse",
    "json_try_text",
    "json_try_get",
    "json_try_int",
    "file_next_line",
    "get_field",
    "args",
    "text_equal",
    "print_err",
    "make_typed_map",
    "json_array",
    "map_keys",
    "json_index",
    "text_chars",
    "json_get",
    "json_stringify",
    "text_concat",
    "json_object",
    "map_index",
    "map_set",
    "json_int",
    "map_has",
    "json_text",
    "file_open_lines",
    "file_close",
    "text_collapse_whitespace",
    "time_parse_timestamp",
    "time_format_utc",
    "text_slice",
    "json_set_local",
    "json_push_local",
];

const NATIVE_BRIDGE_TAG_INT: i64 = 1;
const NATIVE_BRIDGE_TAG_FLOAT: i64 = 2;
const NATIVE_BRIDGE_TAG_BOOL: i64 = 3;
const NATIVE_BRIDGE_TAG_HANDLE: i64 = 4;
const NATIVE_BRIDGE_TAG_NATIVE_RECORD_BASE: i64 = -1024;

pub(crate) fn native_record_bridge_tag(record_index: usize) -> Option<i64> {
    NATIVE_BRIDGE_TAG_NATIVE_RECORD_BASE.checked_sub(i64::try_from(record_index).ok()?)
}

pub(crate) fn native_record_bridge_index(tag: i64) -> Option<usize> {
    if tag > NATIVE_BRIDGE_TAG_NATIVE_RECORD_BASE {
        return None;
    }
    usize::try_from(NATIVE_BRIDGE_TAG_NATIVE_RECORD_BASE.checked_sub(tag)?).ok()
}

pub(crate) const NATIVE_TAGGED_RESULT_OK: i64 = 1;
pub(crate) const NATIVE_TAGGED_RESULT_ERR: i64 = 2;
pub(crate) const NATIVE_TAGGED_OPTION_SOME: i64 = 3;
pub(crate) const NATIVE_TAGGED_OPTION_NONE: i64 = 4;
pub(crate) const NATIVE_CONSTANT_HANDLE_BASE: i64 = 1_i64 << 60;

fn native_constant_handles(artifact: &Artifact) -> Vec<Vec<Option<i64>>> {
    let mut next = 0_i64;
    artifact
        .functions
        .iter()
        .map(|function| {
            function
                .code
                .iter()
                .map(|instruction| {
                    if matches!(&instruction.op, Op::Const(Value::Text(_) | Value::Bytes(_))) {
                        let handle = NATIVE_CONSTANT_HANDLE_BASE.checked_add(next)?;
                        next = next.checked_add(1)?;
                        Some(handle)
                    } else {
                        None
                    }
                })
                .collect()
        })
        .collect()
}

/// Stable, invocation-local layout for Result and Option values inside native code.
/// Handles are negative pointers into stable inline arena slots or overflow chunks;
/// ordinary arena handles remain positive indices. The VM value is materialized only
/// at a VM/native boundary.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct NativeTaggedValue {
    pub(crate) kind: i64,
    pub(crate) payload_tag: i64,
    pub(crate) payload: i64,
    pub(crate) clone_charge_bytes: u64,
}

pub(crate) fn native_bridge_operation(op: &Op) -> Option<i64> {
    match op {
        Op::JsonTryText => Some(NATIVE_BRIDGE_JSON_TRY_TEXT),
        Op::JsonTryGet => Some(NATIVE_BRIDGE_JSON_TRY_GET),
        Op::JsonTryInt => Some(NATIVE_BRIDGE_JSON_TRY_INT),
        Op::JsonParse => Some(NATIVE_BRIDGE_JSON_PARSE),
        Op::FileNextLine => Some(NATIVE_BRIDGE_FILE_NEXT_LINE),
        Op::Args => Some(NATIVE_BRIDGE_ARGS),
        Op::Equal | Op::NotEqual => Some(NATIVE_BRIDGE_TEXT_EQUAL),
        Op::PrintErr => Some(NATIVE_BRIDGE_PRINT_ERR),
        Op::MakeTypedMap(_) => Some(NATIVE_BRIDGE_MAKE_TYPED_MAP),
        Op::JsonArray => Some(NATIVE_BRIDGE_JSON_ARRAY),
        Op::MapKeys => Some(NATIVE_BRIDGE_MAP_KEYS),
        Op::JsonIndex => Some(NATIVE_BRIDGE_JSON_INDEX),
        Op::TextChars => Some(NATIVE_BRIDGE_TEXT_CHARS),
        Op::JsonGet => Some(NATIVE_BRIDGE_JSON_GET),
        Op::JsonStringify => Some(NATIVE_BRIDGE_JSON_STRINGIFY),
        Op::Add => Some(NATIVE_BRIDGE_TEXT_CONCAT),
        Op::JsonObject => Some(NATIVE_BRIDGE_JSON_OBJECT),
        Op::Index => Some(NATIVE_BRIDGE_MAP_INDEX),
        Op::MapSet => Some(NATIVE_BRIDGE_MAP_SET),
        Op::JsonInt => Some(NATIVE_BRIDGE_JSON_INT),
        Op::MapHas => Some(NATIVE_BRIDGE_MAP_HAS),
        Op::JsonText => Some(NATIVE_BRIDGE_JSON_TEXT),
        Op::FileOpenLines => Some(NATIVE_BRIDGE_FILE_OPEN_LINES),
        Op::FileClose => Some(NATIVE_BRIDGE_FILE_CLOSE),
        Op::TextCollapseWhitespace => Some(NATIVE_BRIDGE_TEXT_COLLAPSE_WHITESPACE),
        Op::TimeParseTimestamp => Some(NATIVE_BRIDGE_TIME_PARSE_TIMESTAMP),
        Op::TimeFormatUtc => Some(NATIVE_BRIDGE_TIME_FORMAT_UTC),
        Op::TextSlice => Some(NATIVE_BRIDGE_TEXT_SLICE),
        Op::JsonSetLocal(_) => Some(NATIVE_BRIDGE_JSON_SET_LOCAL),
        Op::JsonPushLocal(_) => Some(NATIVE_BRIDGE_JSON_PUSH_LOCAL),
        Op::MakeRecord(_, _) => Some(NATIVE_BRIDGE_MAKE_RECORD),
        Op::GetField(_, _) => Some(NATIVE_BRIDGE_GET_FIELD),
        _ => None,
    }
}

fn native_direct_operation_index(op: &Op) -> Option<usize> {
    match op {
        Op::JsonParse => Some(0),
        Op::JsonTryText => Some(1),
        Op::JsonTryGet => Some(2),
        Op::JsonTryInt => Some(3),
        Op::FileNextLine => Some(4),
        Op::GetField(_, _) => Some(5),
        Op::Args => Some(6),
        Op::Equal | Op::NotEqual => Some(7),
        Op::PrintErr => Some(8),
        Op::MakeTypedMap(_) => Some(9),
        Op::JsonArray => Some(10),
        Op::MapKeys => Some(11),
        Op::JsonIndex => Some(12),
        Op::TextChars => Some(13),
        Op::JsonGet => Some(14),
        Op::JsonStringify => Some(15),
        Op::Add => Some(16),
        Op::JsonObject => Some(17),
        Op::Index => Some(18),
        Op::MapSet => Some(19),
        Op::JsonInt => Some(20),
        Op::MapHas => Some(21),
        Op::JsonText => Some(22),
        Op::FileOpenLines => Some(23),
        Op::FileClose => Some(24),
        Op::TextCollapseWhitespace => Some(25),
        Op::TimeParseTimestamp => Some(26),
        Op::TimeFormatUtc => Some(27),
        Op::TextSlice => Some(28),
        Op::JsonSetLocal(_) => Some(29),
        Op::JsonPushLocal(_) => Some(30),
        _ => None,
    }
}

fn native_constant_integer_operand(
    function: &Function,
    ip: usize,
    leaders: &[Option<usize>],
) -> Option<i64> {
    if ip == 0 || leaders.get(ip).is_some_and(Option::is_some) {
        return None;
    }
    match &function.code.get(ip - 1)?.op {
        Op::Const(Value::Int(value)) => value.to_i64(),
        Op::Negate if ip >= 2 && leaders.get(ip - 1).is_some_and(Option::is_none) => {
            match &function.code.get(ip - 2)?.op {
                Op::Const(Value::Int(value)) => value.to_i64()?.checked_neg(),
                _ => None,
            }
        }
        _ => None,
    }
}

fn native_integer_overflow_operation_at(
    op: &Op,
    stack: &[Type],
    function: &Function,
    ip: usize,
    leaders: &[Option<usize>],
) -> bool {
    let can_overflow = match op {
        Op::Add | Op::Subtract | Op::Multiply | Op::Divide => {
            stack.len() >= 2 && stack[stack.len() - 2..] == [Type::Int, Type::Int]
        }
        Op::IntegerDivide | Op::FloorDivide => {
            stack.len() >= 2 && stack[stack.len() - 2..] == [Type::Int, Type::Int]
        }
        Op::Negate => stack.last() == Some(&Type::Int),
        _ => false,
    };
    if !can_overflow {
        return false;
    }
    match op {
        Op::Divide | Op::IntegerDivide | Op::FloorDivide
            if native_constant_integer_operand(function, ip, leaders)
                .is_some_and(|divisor| divisor != -1) =>
        {
            return false;
        }
        Op::Negate
            if native_constant_integer_operand(function, ip, leaders)
                .is_some_and(|value| value != i64::MIN) =>
        {
            return false;
        }
        _ => {}
    }
    true
}

fn native_direct_argument_count(op: &Op) -> Option<usize> {
    match op {
        Op::JsonTryGet => Some(2),
        Op::JsonTryText | Op::JsonTryInt | Op::JsonParse | Op::FileNextLine => Some(1),
        Op::Args => Some(0),
        Op::Equal | Op::NotEqual => Some(2),
        Op::PrintErr => Some(1),
        Op::MakeTypedMap(_) => Some(0),
        Op::JsonArray => Some(0),
        Op::MapKeys => Some(1),
        Op::JsonIndex => Some(2),
        Op::TextChars => Some(1),
        Op::JsonGet => Some(2),
        Op::JsonStringify => Some(1),
        Op::Add => Some(2),
        Op::JsonObject => Some(0),
        Op::Index => Some(2),
        Op::MapSet => Some(5),
        Op::JsonInt => Some(1),
        Op::MapHas => Some(2),
        Op::JsonText => Some(1),
        Op::FileOpenLines => Some(1),
        Op::FileClose => Some(1),
        Op::TextCollapseWhitespace => Some(1),
        Op::TimeParseTimestamp => Some(1),
        Op::TimeFormatUtc => Some(1),
        Op::TextSlice => Some(3),
        _ => None,
    }
}

fn native_bridge_tag(ty: &Type) -> i64 {
    match ty {
        Type::Int => NATIVE_BRIDGE_TAG_INT,
        Type::Float64 => NATIVE_BRIDGE_TAG_FLOAT,
        Type::Bool => NATIVE_BRIDGE_TAG_BOOL,
        _ => NATIVE_BRIDGE_TAG_HANDLE,
    }
}

pub(crate) fn native_record_has_native_layout(record: &guion_model::RecordDefinition) -> bool {
    let native_item = |ty: &Type| matches!(ty, Type::Int | Type::Float64 | Type::Bool);
    record.fields.iter().all(|field| {
        native_item(&field.ty) || matches!(&field.ty, Type::List(item) if native_item(item))
    })
}

fn may_contain_map_handle(ty: &Type, records: &[guion_model::RecordDefinition]) -> bool {
    fn visit(
        ty: &Type,
        records: &[guion_model::RecordDefinition],
        visiting_records: &mut std::collections::HashSet<String>,
    ) -> bool {
        match ty {
            Type::Map(_) => true,
            Type::List(item) | Type::Option(item) => visit(item, records, visiting_records),
            Type::Result(ok, err) => {
                visit(ok, records, visiting_records) || visit(err, records, visiting_records)
            }
            Type::Record(name) if visiting_records.insert(name.clone()) => {
                let contains = records
                    .iter()
                    .find(|record| record.name == *name)
                    .is_some_and(|record| {
                        record
                            .fields
                            .iter()
                            .any(|field| visit(&field.ty, records, visiting_records))
                    });
                visiting_records.remove(name);
                contains
            }
            _ => false,
        }
    }

    visit(ty, records, &mut std::collections::HashSet::new())
}

fn may_hide_json_handle_in_native_value(ty: &Type) -> bool {
    match ty {
        Type::Json => true,
        // Native List and tagged Option/Result values keep JSON references as
        // arena handles without cloning their Arc. Maps and records materialize
        // Value nodes, so their Arc counts already expose hidden JSON aliases.
        Type::List(item) | Type::Option(item) => may_hide_json_handle_in_native_value(item),
        Type::Result(ok, err) => {
            may_hide_json_handle_in_native_value(ok) || may_hide_json_handle_in_native_value(err)
        }
        _ => false,
    }
}

type NativeFunction = unsafe extern "C" fn(
    *const i64,
    *mut i64,
    *const std::ffi::c_void,
    i64,
    i64,
    *mut i64,
    *mut std::ffi::c_void,
) -> i32;

pub(crate) struct NativeSymbols {
    pub(crate) reserve_fuel: *const u8,
    pub(crate) release_fuel: *const u8,
    pub(crate) poll_cancellation: *const u8,
    pub(crate) make_list: *const u8,
    pub(crate) list_set_float_copy: *const u8,
    pub(crate) list_set_int_copy: *const u8,
    pub(crate) list_set_bool_copy: *const u8,
    pub(crate) list_set_float_local: *const u8,
    pub(crate) list_set_int_local: *const u8,
    pub(crate) list_set_bool_local: *const u8,
    pub(crate) charge_record: *const u8,
    pub(crate) charge_value_clone: *const u8,
    pub(crate) make_tagged_value: *const u8,
    pub(crate) print: *const u8,
    pub(crate) direct_operations: [*const u8; NATIVE_DIRECT_OPERATION_NAMES.len()],
    pub(crate) make_record: *const u8,
    pub(crate) capture_vm_continuation: *const u8,
    pub(crate) capture_vm_continuation_fixed: *const u8,
    pub(crate) capture_vm_continuation_int2: *const u8,
    pub(crate) count_guest_branch: *const u8,
    pub(crate) loop_safepoint: *const u8,
}

#[repr(C)]
pub(crate) struct NativeFloatList {
    pub(crate) data: *mut u64,
    pub(crate) len: usize,
    pub(crate) storage_id: usize,
    pub(crate) owned: bool,
    pub(crate) inline_storage: bool,
    pub(crate) inline: [u64; NATIVE_INLINE_LIST_ITEMS],
}

#[repr(C)]
pub(crate) struct NativeIntList {
    pub(crate) data: *mut i64,
    pub(crate) len: usize,
    pub(crate) storage_id: usize,
    pub(crate) owned: bool,
    pub(crate) inline_storage: bool,
    pub(crate) inline: [i64; NATIVE_INLINE_LIST_ITEMS],
}

#[repr(C)]
pub(crate) struct NativeHandleList {
    pub(crate) data: *mut i64,
    pub(crate) len: usize,
    pub(crate) storage_id: usize,
    pub(crate) owned: bool,
    pub(crate) inline_storage: bool,
    pub(crate) inline: [i64; NATIVE_INLINE_LIST_ITEMS],
}

#[repr(C)]
pub(crate) struct NativeRecord {
    pub(crate) fields: *const i64,
    pub(crate) len: usize,
}

pub(crate) struct NativeBackend {
    module: JITModule,
    reserve_fuel: FuncId,
    release_fuel: FuncId,
    poll_cancellation: FuncId,
    make_list: FuncId,
    list_set_float_copy: FuncId,
    list_set_int_copy: FuncId,
    list_set_bool_copy: FuncId,
    list_set_float_local: FuncId,
    list_set_int_local: FuncId,
    list_set_bool_local: FuncId,
    charge_record: FuncId,
    charge_value_clone: FuncId,
    make_tagged_value: FuncId,
    print: FuncId,
    direct_operations: Vec<FuncId>,
    make_record: FuncId,
    capture_vm_continuation: FuncId,
    capture_vm_continuation_fixed: FuncId,
    capture_vm_continuation_int2: FuncId,
    count_guest_branch: Option<FuncId>,
    loop_safepoint: FuncId,
    loop_safepoint_functions: Vec<bool>,
    reaches_arena_allocation: Vec<bool>,
    constant_handles: Vec<Vec<Option<i64>>>,
    functions: Vec<Option<*const u8>>,
    result_words: Vec<usize>,
    max_result_words: usize,
    vm_call_stays_in_vm: Vec<bool>,
    bytecode_instructions: usize,
    bytecode_blocks: usize,
    cranelift_ir_blocks: usize,
    cranelift_ir_instructions: usize,
    function_compile_profiles: Vec<NativeFunctionCompileProfile>,
    function_rejection_profiles: Vec<NativeFunctionRejectionProfile>,
    finalize_nanoseconds: u64,
}

#[derive(Debug)]
struct NativeFunctionCompileProfile {
    name: String,
    bytecode_instructions: usize,
    bytecode_blocks: usize,
    cranelift_ir_blocks: usize,
    cranelift_ir_instructions: usize,
    lowering_nanoseconds: u64,
    verification_nanoseconds: u64,
    define_nanoseconds: u64,
}

#[derive(Debug, Clone)]
struct NativeFunctionRejectionProfile {
    name: String,
    reason_code: String,
    reason: String,
    blocking_instruction: Option<String>,
    blocking_callee: Option<String>,
}

/// Immutable native entry points shared by parallel workers. The owning
/// `NativeBackend` stays alive on the parent instance until those workers join.
#[derive(Debug)]
pub(crate) struct NativeCode {
    functions: Vec<Option<usize>>,
    result_words: Vec<usize>,
    max_result_words: usize,
    vm_call_stays_in_vm: Vec<bool>,
}

pub(crate) struct NativeOutcome {
    pub status: i32,
    pub value: i64,
    pub result_values: Vec<i64>,
    pub steps: u64,
    pub fault_ip: usize,
    pub fault_function: usize,
    pub calls: usize,
    pub trace: Vec<(usize, usize)>,
    pub allocations: usize,
    pub continuations: Vec<NativeVmContinuationFrame>,
}

impl fmt::Debug for NativeBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NativeBackend")
            .field("compiled_functions", &self.function_count())
            .field("bytecode_instructions", &self.bytecode_instructions)
            .field("bytecode_blocks", &self.bytecode_blocks)
            .field("cranelift_ir_blocks", &self.cranelift_ir_blocks)
            .field("cranelift_ir_instructions", &self.cranelift_ir_instructions)
            .field("function_compile_profiles", &self.function_compile_profiles)
            .field("finalize_nanoseconds", &self.finalize_nanoseconds)
            .finish()
    }
}

impl NativeBackend {
    pub(crate) fn compile(
        artifact: &Artifact,
        symbols: NativeSymbols,
        profile_guest_branches: bool,
    ) -> Option<Self> {
        let (eligible, mut rejection_profiles) =
            native_eligibility_report(artifact, cfg!(feature = "native-value-bridge"));
        if !eligible.iter().any(|eligible| *eligible) {
            return None;
        }
        let mut builder = JITBuilder::new(default_libcall_names()).ok()?;
        builder.symbol("guion_reserve_fuel", symbols.reserve_fuel);
        builder.symbol("guion_release_fuel", symbols.release_fuel);
        builder.symbol("guion_poll_cancellation", symbols.poll_cancellation);
        builder.symbol("guion_make_list", symbols.make_list);
        builder.symbol("guion_list_set_float_copy", symbols.list_set_float_copy);
        builder.symbol("guion_list_set_int_copy", symbols.list_set_int_copy);
        builder.symbol("guion_list_set_bool_copy", symbols.list_set_bool_copy);
        builder.symbol("guion_list_set_float_local", symbols.list_set_float_local);
        builder.symbol("guion_list_set_int_local", symbols.list_set_int_local);
        builder.symbol("guion_list_set_bool_local", symbols.list_set_bool_local);
        builder.symbol("guion_charge_record", symbols.charge_record);
        builder.symbol("guion_charge_value_clone", symbols.charge_value_clone);
        builder.symbol("guion_make_tagged_value", symbols.make_tagged_value);
        builder.symbol("guion_print", symbols.print);
        for (name, address) in NATIVE_DIRECT_OPERATION_NAMES
            .into_iter()
            .zip(symbols.direct_operations)
        {
            builder.symbol(format!("guion_direct_{name}"), address);
        }
        builder.symbol("guion_make_record", symbols.make_record);
        builder.symbol(
            "guion_capture_vm_continuation",
            symbols.capture_vm_continuation,
        );
        builder.symbol(
            "guion_capture_vm_continuation_fixed",
            symbols.capture_vm_continuation_fixed,
        );
        builder.symbol(
            "guion_capture_vm_continuation_int2",
            symbols.capture_vm_continuation_int2,
        );
        if profile_guest_branches {
            builder.symbol("guion_count_guest_branch", symbols.count_guest_branch);
        }
        builder.symbol("guion_loop_safepoint", symbols.loop_safepoint);
        let mut module = JITModule::new(builder);
        let pointer_type = module.target_config().pointer_type();
        if pointer_type != types::I64 {
            return None;
        }
        let mut reserve_signature = Signature::new(module.target_config().default_call_conv);
        reserve_signature.params.push(AbiParam::new(pointer_type));
        reserve_signature.params.push(AbiParam::new(types::I64));
        reserve_signature.returns.push(AbiParam::new(types::I64));
        let reserve_fuel_id = module
            .declare_function("guion_reserve_fuel", Linkage::Import, &reserve_signature)
            .ok()?;
        let mut release_signature = Signature::new(module.target_config().default_call_conv);
        release_signature.params.push(AbiParam::new(pointer_type));
        release_signature.params.push(AbiParam::new(types::I64));
        let release_fuel_id = module
            .declare_function("guion_release_fuel", Linkage::Import, &release_signature)
            .ok()?;
        let mut cancel_signature = Signature::new(module.target_config().default_call_conv);
        cancel_signature.params.push(AbiParam::new(pointer_type));
        cancel_signature.returns.push(AbiParam::new(types::I32));
        let poll_cancellation_id = module
            .declare_function(
                "guion_poll_cancellation",
                Linkage::Import,
                &cancel_signature,
            )
            .ok()?;
        let mut make_list_signature = Signature::new(module.target_config().default_call_conv);
        make_list_signature.params.push(AbiParam::new(pointer_type));
        make_list_signature.params.push(AbiParam::new(pointer_type));
        make_list_signature.params.push(AbiParam::new(types::I64));
        make_list_signature.params.push(AbiParam::new(types::I64));
        make_list_signature.params.push(AbiParam::new(types::I64));
        make_list_signature
            .returns
            .push(AbiParam::new(pointer_type));
        let make_list_id = module
            .declare_function("guion_make_list", Linkage::Import, &make_list_signature)
            .ok()?;
        let mut list_set_signature = Signature::new(module.target_config().default_call_conv);
        list_set_signature.params.push(AbiParam::new(pointer_type));
        list_set_signature.params.push(AbiParam::new(pointer_type));
        list_set_signature.params.push(AbiParam::new(types::I64));
        list_set_signature.params.push(AbiParam::new(types::I64));
        list_set_signature.returns.push(AbiParam::new(pointer_type));
        let list_set_float_copy_id = module
            .declare_function(
                "guion_list_set_float_copy",
                Linkage::Import,
                &list_set_signature,
            )
            .ok()?;
        let list_set_int_copy_id = module
            .declare_function(
                "guion_list_set_int_copy",
                Linkage::Import,
                &list_set_signature,
            )
            .ok()?;
        let list_set_bool_copy_id = module
            .declare_function(
                "guion_list_set_bool_copy",
                Linkage::Import,
                &list_set_signature,
            )
            .ok()?;
        let list_set_float_local_id = module
            .declare_function(
                "guion_list_set_float_local",
                Linkage::Import,
                &list_set_signature,
            )
            .ok()?;
        let list_set_int_local_id = module
            .declare_function(
                "guion_list_set_int_local",
                Linkage::Import,
                &list_set_signature,
            )
            .ok()?;
        let list_set_bool_local_id = module
            .declare_function(
                "guion_list_set_bool_local",
                Linkage::Import,
                &list_set_signature,
            )
            .ok()?;
        let mut charge_record_signature = Signature::new(module.target_config().default_call_conv);
        charge_record_signature
            .params
            .push(AbiParam::new(pointer_type));
        charge_record_signature
            .params
            .push(AbiParam::new(types::I64));
        charge_record_signature
            .returns
            .push(AbiParam::new(types::I32));
        let charge_record_id = module
            .declare_function(
                "guion_charge_record",
                Linkage::Import,
                &charge_record_signature,
            )
            .ok()?;
        let charge_value_clone_id = module
            .declare_function(
                "guion_charge_value_clone",
                Linkage::Import,
                &charge_record_signature,
            )
            .ok()?;
        let mut make_tagged_value_signature =
            Signature::new(module.target_config().default_call_conv);
        make_tagged_value_signature
            .params
            .push(AbiParam::new(pointer_type));
        make_tagged_value_signature
            .params
            .push(AbiParam::new(types::I64));
        make_tagged_value_signature
            .params
            .push(AbiParam::new(types::I64));
        make_tagged_value_signature
            .params
            .push(AbiParam::new(types::I64));
        make_tagged_value_signature
            .returns
            .push(AbiParam::new(types::I64));
        let make_tagged_value_id = module
            .declare_function(
                "guion_make_tagged_value",
                Linkage::Import,
                &make_tagged_value_signature,
            )
            .ok()?;
        let mut print_signature = Signature::new(module.target_config().default_call_conv);
        print_signature.params.push(AbiParam::new(pointer_type));
        print_signature.params.push(AbiParam::new(types::I64));
        print_signature.params.push(AbiParam::new(types::I64));
        print_signature.returns.push(AbiParam::new(types::I32));
        let print_id = module
            .declare_function("guion_print", Linkage::Import, &print_signature)
            .ok()?;
        let mut direct_signature = Signature::new(module.target_config().default_call_conv);
        direct_signature.params.push(AbiParam::new(pointer_type));
        for _ in 0..14 {
            direct_signature.params.push(AbiParam::new(types::I64));
        }
        direct_signature.params.push(AbiParam::new(pointer_type));
        direct_signature.returns.push(AbiParam::new(types::I32));
        let direct_operations = NATIVE_DIRECT_OPERATION_NAMES
            .iter()
            .map(|name| {
                module
                    .declare_function(
                        &format!("guion_direct_{name}"),
                        Linkage::Import,
                        &direct_signature,
                    )
                    .ok()
            })
            .collect::<Option<Vec<_>>>()?;
        let mut make_record_signature = Signature::new(module.target_config().default_call_conv);
        make_record_signature
            .params
            .push(AbiParam::new(pointer_type));
        make_record_signature.params.push(AbiParam::new(types::I64));
        make_record_signature.params.push(AbiParam::new(types::I64));
        make_record_signature.params.push(AbiParam::new(types::I64));
        make_record_signature
            .params
            .push(AbiParam::new(pointer_type));
        make_record_signature
            .params
            .push(AbiParam::new(pointer_type));
        make_record_signature.params.push(AbiParam::new(types::I64));
        make_record_signature.params.push(AbiParam::new(types::I64));
        make_record_signature
            .params
            .push(AbiParam::new(pointer_type));
        make_record_signature
            .returns
            .push(AbiParam::new(types::I32));
        let make_record_id = module
            .declare_function("guion_make_record", Linkage::Import, &make_record_signature)
            .ok()?;
        let mut continuation_signature = Signature::new(module.target_config().default_call_conv);
        continuation_signature
            .params
            .push(AbiParam::new(pointer_type));
        continuation_signature
            .params
            .push(AbiParam::new(types::I64));
        continuation_signature
            .params
            .push(AbiParam::new(types::I64));
        continuation_signature
            .params
            .push(AbiParam::new(types::I64));
        continuation_signature
            .params
            .push(AbiParam::new(pointer_type));
        continuation_signature
            .params
            .push(AbiParam::new(types::I64));
        continuation_signature
            .params
            .push(AbiParam::new(pointer_type));
        continuation_signature
            .params
            .push(AbiParam::new(types::I64));
        continuation_signature
            .params
            .push(AbiParam::new(types::I64));
        continuation_signature
            .returns
            .push(AbiParam::new(types::I32));
        let capture_vm_continuation_id = module
            .declare_function(
                "guion_capture_vm_continuation",
                Linkage::Import,
                &continuation_signature,
            )
            .ok()?;
        let mut fixed_continuation_signature =
            Signature::new(module.target_config().default_call_conv);
        fixed_continuation_signature
            .params
            .push(AbiParam::new(pointer_type));
        for _ in 0..9 {
            fixed_continuation_signature
                .params
                .push(AbiParam::new(types::I64));
        }
        fixed_continuation_signature
            .returns
            .push(AbiParam::new(types::I32));
        let capture_vm_continuation_fixed_id = module
            .declare_function(
                "guion_capture_vm_continuation_fixed",
                Linkage::Import,
                &fixed_continuation_signature,
            )
            .ok()?;
        let mut int2_continuation_signature =
            Signature::new(module.target_config().default_call_conv);
        int2_continuation_signature
            .params
            .push(AbiParam::new(pointer_type));
        for _ in 0..6 {
            int2_continuation_signature
                .params
                .push(AbiParam::new(types::I64));
        }
        int2_continuation_signature
            .returns
            .push(AbiParam::new(types::I32));
        let capture_vm_continuation_int2_id = module
            .declare_function(
                "guion_capture_vm_continuation_int2",
                Linkage::Import,
                &int2_continuation_signature,
            )
            .ok()?;
        let mut loop_safepoint_signature = Signature::new(module.target_config().default_call_conv);
        loop_safepoint_signature
            .params
            .push(AbiParam::new(pointer_type));
        loop_safepoint_signature
            .params
            .push(AbiParam::new(pointer_type));
        loop_safepoint_signature
            .params
            .push(AbiParam::new(types::I64));
        loop_safepoint_signature
            .returns
            .push(AbiParam::new(types::I32));
        let loop_safepoint_id = module
            .declare_function(
                "guion_loop_safepoint",
                Linkage::Import,
                &loop_safepoint_signature,
            )
            .ok()?;
        let count_guest_branch_id = if profile_guest_branches {
            let mut signature = Signature::new(module.target_config().default_call_conv);
            signature.params.push(AbiParam::new(pointer_type));
            Some(
                module
                    .declare_function("guion_count_guest_branch", Linkage::Import, &signature)
                    .ok()?,
            )
        } else {
            None
        };
        let mut backend = Self {
            module,
            reserve_fuel: reserve_fuel_id,
            release_fuel: release_fuel_id,
            poll_cancellation: poll_cancellation_id,
            make_list: make_list_id,
            list_set_float_copy: list_set_float_copy_id,
            list_set_int_copy: list_set_int_copy_id,
            list_set_bool_copy: list_set_bool_copy_id,
            list_set_float_local: list_set_float_local_id,
            list_set_int_local: list_set_int_local_id,
            list_set_bool_local: list_set_bool_local_id,
            charge_record: charge_record_id,
            charge_value_clone: charge_value_clone_id,
            make_tagged_value: make_tagged_value_id,
            print: print_id,
            direct_operations,
            make_record: make_record_id,
            capture_vm_continuation: capture_vm_continuation_id,
            capture_vm_continuation_fixed: capture_vm_continuation_fixed_id,
            capture_vm_continuation_int2: capture_vm_continuation_int2_id,
            count_guest_branch: count_guest_branch_id,
            loop_safepoint: loop_safepoint_id,
            loop_safepoint_functions: loop_safepoint_functions(artifact),
            reaches_arena_allocation: functions_reaching_native_arena_allocations(artifact),
            constant_handles: native_constant_handles(artifact),
            functions: vec![None; artifact.functions.len()],
            result_words: artifact
                .functions
                .iter()
                .map(|function| {
                    native_result_types(&function.result, &artifact.records)
                        .map_or(1, |fields| fields.len())
                })
                .collect(),
            max_result_words: artifact
                .functions
                .iter()
                .filter_map(|function| native_result_types(&function.result, &artifact.records))
                .map(|fields| fields.len())
                .max()
                .unwrap_or(1)
                .max(1),
            vm_call_stays_in_vm: functions_reaching_native_host_callbacks(artifact),
            bytecode_instructions: 0,
            bytecode_blocks: 0,
            cranelift_ir_blocks: 0,
            cranelift_ir_instructions: 0,
            function_compile_profiles: Vec::new(),
            function_rejection_profiles: Vec::new(),
            finalize_nanoseconds: 0,
        };
        let pointer_type = backend.module.target_config().pointer_type();
        let mut native_signature = Signature::new(backend.module.target_config().default_call_conv);
        native_signature.params.push(AbiParam::new(pointer_type));
        native_signature.params.push(AbiParam::new(pointer_type));
        native_signature.params.push(AbiParam::new(pointer_type));
        native_signature.params.push(AbiParam::new(types::I64));
        native_signature.params.push(AbiParam::new(types::I64));
        native_signature.params.push(AbiParam::new(pointer_type));
        native_signature.params.push(AbiParam::new(pointer_type));
        native_signature.returns.push(AbiParam::new(types::I32));
        let mut function_ids = vec![None; artifact.functions.len()];
        for (index, _function) in artifact.functions.iter().enumerate() {
            if !eligible[index] {
                continue;
            }
            function_ids[index] = backend
                .module
                .declare_function(
                    &format!("guion_native_{index}"),
                    Linkage::Local,
                    &native_signature,
                )
                .ok();
            if function_ids[index].is_none() {
                rejection_profiles[index] = Some(NativeFunctionRejectionProfile {
                    name: artifact.functions[index].name.clone(),
                    reason_code: "native_declaration_failed".into(),
                    reason: "Cranelift could not declare the native function".into(),
                    blocking_instruction: None,
                    blocking_callee: None,
                });
            }
        }
        for (index, function) in artifact.functions.iter().enumerate() {
            if function_ids[index].is_some() {
                match backend.compile_function(
                    index,
                    function,
                    &function_ids,
                    &artifact.functions,
                    &artifact.records,
                    backend.max_result_words,
                ) {
                    Ok(profile) => {
                        backend.bytecode_instructions += profile.bytecode_instructions;
                        backend.bytecode_blocks += profile.bytecode_blocks;
                        backend.cranelift_ir_blocks += profile.cranelift_ir_blocks;
                        backend.cranelift_ir_instructions += profile.cranelift_ir_instructions;
                        backend.function_compile_profiles.push(profile);
                    }
                    Err(error) => {
                        function_ids[index] = None;
                        rejection_profiles[index] = Some(NativeFunctionRejectionProfile {
                            name: function.name.clone(),
                            reason_code: "native_lowering_failed".into(),
                            reason: error,
                            blocking_instruction: None,
                            blocking_callee: None,
                        });
                    }
                }
            }
        }
        let finalize_start = Instant::now();
        backend.module.finalize_definitions().ok()?;
        backend.finalize_nanoseconds =
            finalize_start.elapsed().as_nanos().min(u64::MAX as u128) as u64;
        for (index, function_id) in function_ids.into_iter().enumerate() {
            if let Some(function_id) = function_id {
                backend.functions[index] = Some(backend.module.get_finalized_function(function_id));
            }
        }
        if backend.functions.iter().any(Option::is_some) {
            backend.function_rejection_profiles =
                rejection_profiles.into_iter().flatten().collect();
            Some(backend)
        } else {
            None
        }
    }

    pub(crate) fn function_count(&self) -> usize {
        self.functions
            .iter()
            .filter(|function| function.is_some())
            .count()
    }

    pub(crate) fn bytecode_instruction_count(&self) -> usize {
        self.bytecode_instructions
    }

    pub(crate) fn bytecode_block_count(&self) -> usize {
        self.bytecode_blocks
    }

    pub(crate) fn cranelift_ir_block_count(&self) -> usize {
        self.cranelift_ir_blocks
    }

    pub(crate) fn cranelift_ir_instruction_count(&self) -> usize {
        self.cranelift_ir_instructions
    }

    pub(crate) fn compile_profile_json(&self) -> serde_json::Value {
        let functions: Vec<_> = self
            .function_compile_profiles
            .iter()
            .map(|profile| {
                serde_json::json!({
                    "name": profile.name,
                    "bytecode_instructions": profile.bytecode_instructions,
                    "bytecode_blocks": profile.bytecode_blocks,
                    "cranelift_ir_blocks": profile.cranelift_ir_blocks,
                    "cranelift_ir_instructions": profile.cranelift_ir_instructions,
                    "lowering_nanoseconds": profile.lowering_nanoseconds,
                    "verification_nanoseconds": profile.verification_nanoseconds,
                    "define_nanoseconds": profile.define_nanoseconds
                })
            })
            .collect();
        serde_json::json!({
            "functions": functions,
            "rejected_functions": self.function_rejection_profiles.iter().map(|profile| {
                serde_json::json!({
                    "name": profile.name,
                    "reason_code": profile.reason_code,
                    "reason": profile.reason,
                    "blocking_instruction": profile.blocking_instruction,
                    "blocking_callee": profile.blocking_callee
                })
            }).collect::<Vec<_>>(),
            "finalize_nanoseconds": self.finalize_nanoseconds
        })
    }

    pub(crate) fn eligibility_profile_json(artifact: &Artifact) -> serde_json::Value {
        let (_, rejected) =
            native_eligibility_report(artifact, cfg!(feature = "native-value-bridge"));
        serde_json::json!({
            "functions": [],
            "rejected_functions": rejected.into_iter().flatten().map(|profile| {
                serde_json::json!({
                    "name": profile.name,
                    "reason_code": profile.reason_code,
                    "reason": profile.reason,
                    "blocking_instruction": profile.blocking_instruction,
                    "blocking_callee": profile.blocking_callee
                })
            }).collect::<Vec<_>>(),
            "finalize_nanoseconds": 0
        })
    }

    pub(crate) fn compiled_function_names(&self, artifact: &Artifact) -> Vec<String> {
        self.functions
            .iter()
            .enumerate()
            .filter_map(|(index, function)| {
                function
                    .as_ref()
                    .map(|_| artifact.functions[index].name.clone())
            })
            .collect()
    }

    pub(crate) fn shared_code(&self) -> Arc<NativeCode> {
        Arc::new(NativeCode {
            functions: self
                .functions
                .iter()
                .map(|function| function.map(|pointer| pointer as usize))
                .collect(),
            result_words: self.result_words.clone(),
            max_result_words: self.max_result_words,
            vm_call_stays_in_vm: self.vm_call_stays_in_vm.clone(),
        })
    }
}

impl NativeCode {
    pub(crate) fn has_function(&self, function_index: usize) -> bool {
        self.functions
            .get(function_index)
            .is_some_and(Option::is_some)
    }

    pub(crate) fn vm_call_stays_in_vm(&self, function_index: usize) -> bool {
        self.vm_call_stays_in_vm
            .get(function_index)
            .copied()
            .unwrap_or(false)
    }

    pub(crate) fn invoke(
        &self,
        index: usize,
        args: &[i64],
        budget: *const std::ffi::c_void,
        depth: usize,
        depth_limit: usize,
        arena: &mut NativeListArena,
    ) -> Option<NativeOutcome> {
        let pointer = *self.functions.get(index)?.as_ref()?;
        // SAFETY: the parent Instance retains the JITModule that owns this address
        // until parallel_map has joined every worker. Generated functions are
        // reentrant: invocation state is passed through these arguments.
        let function: NativeFunction = unsafe { std::mem::transmute(pointer as *const u8) };
        let trace_words = depth_limit.checked_mul(2)?.checked_add(1)?;
        let mut trace_buffer = Vec::new();
        trace_buffer.try_reserve_exact(trace_words).ok()?;
        trace_buffer.resize(trace_words, -1_i64);
        *trace_buffer.last_mut()? = depth.min(i64::MAX as usize) as i64;
        let result_words = *self.result_words.get(index)?;
        let metadata_start = self.max_result_words;
        let mut output = vec![0_i64; metadata_start.checked_add(5)?];
        // `args` and `output` are live, properly aligned arrays for this call.
        let status = unsafe {
            function(
                args.as_ptr(),
                output.as_mut_ptr(),
                budget,
                depth.min(i64::MAX as usize) as i64,
                depth_limit.min(i64::MAX as usize) as i64,
                trace_buffer.as_mut_ptr(),
                arena as *mut NativeListArena as *mut std::ffi::c_void,
            )
        };
        let unused_fuel = output[metadata_start + 4].max(0) as u64;
        if unused_fuel > 0 && !budget.is_null() {
            // SAFETY: the caller passes a pointer to the live Budget for the whole call.
            unsafe { (*(budget as *const guion_vm_core::Budget)).refund(unused_fuel) };
        }
        let trace_depth = usize::try_from(*trace_buffer.last()?).ok()?;
        let trace = trace_buffer
            .as_slice()
            .as_chunks::<2>()
            .0
            .iter()
            .skip(depth)
            .take(trace_depth.saturating_sub(depth))
            .take_while(|frame| frame[0] >= 0 && frame[1] >= 0)
            .map(|frame| (frame[0] as usize, frame[1] as usize))
            .collect();
        Some(NativeOutcome {
            status,
            value: output[0],
            result_values: output[..result_words].to_vec(),
            steps: output[metadata_start].max(0) as u64,
            fault_ip: output[metadata_start + 1].max(0) as usize,
            fault_function: output[metadata_start + 2].max(0) as usize,
            calls: output[metadata_start + 3].max(0) as usize,
            trace,
            allocations: arena.charged_bytes,
            continuations: std::mem::take(&mut arena.native_vm_continuations),
        })
    }
}

impl NativeBackend {
    fn compile_function(
        &mut self,
        index: usize,
        function: &Function,
        function_ids: &[Option<FuncId>],
        all_functions: &[Function],
        records: &[guion_model::RecordDefinition],
        max_result_words: usize,
    ) -> Result<NativeFunctionCompileProfile, String> {
        let count_guest_branch = self.count_guest_branch;
        let lowering_start = Instant::now();
        let mut blocking_ip = None;
        let type_states = infer_native_types(function, all_functions, records, &mut blocking_ip)
            .ok_or_else(|| "unsupported types or control flow".to_owned())?;
        let live_locals_at_ip = live_local_slots_at_ip(function);
        let depths = stack_depths(function).ok_or_else(|| "invalid stack graph".to_owned())?;
        let (leader_indices, block_for_ip) = bytecode_block_layout(function, &depths)
            .ok_or_else(|| "invalid bytecode block graph".to_owned())?;
        let stack_types = &type_states.stack_at_ip;
        let needs_int2_continuation = function.code.iter().enumerate().any(|(ip, instruction)| {
            let Some(stack) = stack_types.get(ip).and_then(Option::as_ref) else {
                return false;
            };
            native_integer_overflow_operation_at(
                &instruction.op,
                stack,
                function,
                ip,
                &leader_indices,
            ) && stack.len() == 2
                && live_locals_at_ip.get(ip).is_some_and(Vec::is_empty)
        });
        let needs_continuation = function.code.iter().enumerate().any(|(ip, instruction)| {
            let record_requires_direct_call = match &instruction.op {
                Op::GetField(record_index, _) | Op::MakeRecord(record_index, _) => records
                    .get(*record_index)
                    .is_some_and(|record| !native_record_has_native_layout(record)),
                _ => false,
            };
            let direct_operation_requires_capture = match &instruction.op {
                Op::GetField(_, _) | Op::MakeRecord(_, _) => false,
                Op::Add => stack_types
                    .get(ip)
                    .and_then(Option::as_ref)
                    .is_some_and(|stack| {
                        stack.len() >= 2 && stack[stack.len() - 2..] == [Type::Text, Type::Text]
                    }),
                Op::Index => stack_types
                    .get(ip)
                    .and_then(Option::as_ref)
                    .is_some_and(|stack| {
                        matches!(stack.get(stack.len().saturating_sub(2)), Some(Type::Map(_)))
                    }),
                _ => native_direct_operation_index(&instruction.op).is_some(),
            };
            let tagged_value_requires_capture = matches!(
                &instruction.op,
                Op::Unwrap | Op::GetError | Op::MakeOk | Op::MakeErr | Op::MakeSome | Op::MakeNone
            ) || matches!(&instruction.op, Op::WriteTextAtomic);
            direct_operation_requires_capture
                || tagged_value_requires_capture
                || record_requires_direct_call
                || stack_types
                    .get(ip)
                    .and_then(Option::as_ref)
                    .is_some_and(|stack| {
                        native_integer_overflow_operation_at(
                            &instruction.op,
                            stack,
                            function,
                            ip,
                            &leader_indices,
                        )
                    })
        });
        let target_config = self.module.target_config();
        let ptr = target_config.pointer_type();
        let mut signature = Signature::new(target_config.default_call_conv);
        signature.params.push(AbiParam::new(ptr)); // argument array
        signature.params.push(AbiParam::new(ptr)); // output array
        signature.params.push(AbiParam::new(ptr)); // shared instruction budget
        signature.params.push(AbiParam::new(types::I64)); // current call depth
        signature.params.push(AbiParam::new(types::I64)); // call depth limit
        signature.params.push(AbiParam::new(ptr)); // shared trace buffer
        signature.params.push(AbiParam::new(ptr)); // invocation-local list arena
        signature.returns.push(AbiParam::new(types::I32));
        let function_id = function_ids[index].ok_or_else(|| "function is not native".to_owned())?;

        let mut context = Context::new();
        context.func = cranelift_codegen::ir::Function::with_name_signature(
            UserFuncName::user(0, function_id.as_u32()),
            signature,
        );

        let reserve_fuel = self
            .module
            .declare_func_in_func(self.reserve_fuel, &mut context.func);
        let release_fuel = self
            .module
            .declare_func_in_func(self.release_fuel, &mut context.func);
        let poll_cancellation = self
            .module
            .declare_func_in_func(self.poll_cancellation, &mut context.func);
        let make_list = self
            .module
            .declare_func_in_func(self.make_list, &mut context.func);
        let list_set_functions = NativeListSetFunctions {
            float_copy: self
                .module
                .declare_func_in_func(self.list_set_float_copy, &mut context.func),
            int_copy: self
                .module
                .declare_func_in_func(self.list_set_int_copy, &mut context.func),
            bool_copy: self
                .module
                .declare_func_in_func(self.list_set_bool_copy, &mut context.func),
            float_local: self
                .module
                .declare_func_in_func(self.list_set_float_local, &mut context.func),
            int_local: self
                .module
                .declare_func_in_func(self.list_set_int_local, &mut context.func),
            bool_local: self
                .module
                .declare_func_in_func(self.list_set_bool_local, &mut context.func),
        };
        let charge_record = self
            .module
            .declare_func_in_func(self.charge_record, &mut context.func);
        let charge_value_clone = self
            .module
            .declare_func_in_func(self.charge_value_clone, &mut context.func);
        let make_tagged_value = self
            .module
            .declare_func_in_func(self.make_tagged_value, &mut context.func);
        let print = self
            .module
            .declare_func_in_func(self.print, &mut context.func);
        let direct_operations = self
            .direct_operations
            .iter()
            .map(|function_id| {
                self.module
                    .declare_func_in_func(*function_id, &mut context.func)
            })
            .collect::<Vec<_>>();
        let make_record = self
            .module
            .declare_func_in_func(self.make_record, &mut context.func);
        let capture_vm_continuation = self
            .module
            .declare_func_in_func(self.capture_vm_continuation, &mut context.func);
        let capture_vm_continuation_fixed = self
            .module
            .declare_func_in_func(self.capture_vm_continuation_fixed, &mut context.func);
        let capture_vm_continuation_int2 = self
            .module
            .declare_func_in_func(self.capture_vm_continuation_int2, &mut context.func);
        let loop_safepoint = self
            .module
            .declare_func_in_func(self.loop_safepoint, &mut context.func);
        let count_guest_branch = count_guest_branch.map(|function_id| {
            self.module
                .declare_func_in_func(function_id, &mut context.func)
        });
        let native_function_refs: Vec<_> = function_ids
            .iter()
            .map(|function_id| {
                function_id.map(|function_id| {
                    self.module
                        .declare_func_in_func(function_id, &mut context.func)
                })
            })
            .collect();
        let mut builder_context = FunctionBuilderContext::new();
        {
            let mut builder = FunctionBuilder::new(&mut context.func, &mut builder_context);
            let entry = builder.create_block();
            builder.append_block_params_for_function_params(entry);
            let params = builder.block_params(entry).to_vec();
            let args_pointer = params[0];
            let result_pointer = params[1];
            let budget_pointer = params[2];
            let depth_value = params[3];
            let depth_limit = params[4];
            let trace_pointer = params[5];
            let arena_pointer = params[6];

            let blocks: Vec<_> = leader_indices
                .iter()
                .filter(|leader| leader.is_some())
                .map(|_| builder.create_block())
                .collect();
            let mut stack_params = vec![Vec::new(); blocks.len()];
            for (ip, block_index) in leader_indices.iter().enumerate() {
                let Some(block_index) = block_index else {
                    continue;
                };
                let mut values = Vec::new();
                for _ in 0..depths[ip].unwrap_or(0) {
                    values.push(builder.append_block_param(blocks[*block_index], types::I64));
                }
                stack_params[*block_index] = values;
            }
            let mut block_instruction_counts = vec![0usize; blocks.len()];
            let mut block_instruction_offsets = vec![0usize; function.code.len()];
            for (ip, block_index) in block_for_ip.iter().enumerate() {
                if depths[ip].is_some() {
                    block_instruction_offsets[ip] = block_instruction_counts[*block_index];
                    block_instruction_counts[*block_index] += 1;
                }
            }
            let fuel_reservation_requirements = fuel_reservation_requirements(
                function,
                &leader_indices,
                &block_for_ip,
                &block_instruction_counts,
            )
            .ok_or_else(|| "invalid fuel reservation graph".to_owned())?;
            let fallback = builder.create_block();
            let divide_zero = builder.create_block();
            let float_nonfinite = builder.create_block();
            let math_domain = builder.create_block();
            let cancelled = builder.create_block();
            let continuation_exit_state = needs_continuation.then(|| {
                let block = builder.create_block();
                let fault_ip = builder.append_block_param(block, types::I64);
                let completed = builder.append_block_param(block, types::I64);
                (block, [fault_ip, completed])
            });
            let continuation_exit = continuation_exit_state
                .map(|(block, _)| block)
                .unwrap_or(fallback);
            let int2_continuation_state = needs_int2_continuation.then(|| {
                let block = builder.create_block();
                let fault_ip = builder.append_block_param(block, types::I64);
                let completed = builder.append_block_param(block, types::I64);
                let first = builder.append_block_param(block, types::I64);
                let second = builder.append_block_param(block, types::I64);
                (block, [fault_ip, completed, first, second])
            });
            let int2_continuation_exit = int2_continuation_state
                .map(|(block, _)| block)
                .unwrap_or(fallback);
            let mut exit_params = Vec::new();
            for block in [
                fallback,
                divide_zero,
                float_nonfinite,
                math_domain,
                cancelled,
            ] {
                let steps = builder.append_block_param(block, types::I64);
                let source_ip = builder.append_block_param(block, types::I64);
                let completed_instructions = builder.append_block_param(block, types::I64);
                exit_params.push([steps, source_ip, completed_instructions]);
            }

            builder.switch_to_block(entry);
            let output_pointer = builder.ins().iadd_imm_s(
                result_pointer,
                (max_result_words.saturating_sub(1) * 8) as i64,
            );
            let trace_length_offset = builder.ins().ishl_imm_s(depth_limit, 4);
            let trace_length_pointer = builder.ins().iadd(trace_pointer, trace_length_offset);
            let steps_var = builder.declare_var(types::I64);
            let fuel_var = builder.declare_var(types::I64);
            let native_calls_var = builder.declare_var(types::I64);
            let local_vars: Vec<Variable> = (0..function.local_count)
                .map(|_| builder.declare_var(types::I64))
                .collect();
            let continuation_codegen = NativeContinuationCodegen {
                pointer_type: ptr,
                capture: capture_vm_continuation,
                capture_fixed: capture_vm_continuation_fixed,
                capture_int2: capture_vm_continuation_int2,
                arena: arena_pointer,
                function_index: index,
                depth: depth_value,
                local_vars: &local_vars,
                live_locals_at_ip: &live_locals_at_ip,
                steps_var,
                fallback,
                continuation_exit,
                int2_continuation_exit,
                use_int2_continuation_exit: needs_int2_continuation,
            };
            // The local values must be declared once; seed the SSA variables above.
            for (local, variable) in local_vars.iter().enumerate() {
                let zero = builder.ins().iconst(types::I64, 0);
                builder.def_var(*variable, zero);
                if local < function.parameters.len() {
                    let address = builder.ins().iadd_imm_s(args_pointer, (local * 8) as i64);
                    let value = builder
                        .ins()
                        .load(types::I64, MemFlagsData::new(), address, 0);
                    builder.def_var(*variable, value);
                }
            }
            let zero_steps = builder.ins().iconst(types::I64, 0);
            builder.def_var(steps_var, zero_steps);
            let handed_fuel =
                builder
                    .ins()
                    .load(types::I64, MemFlagsData::new(), output_pointer, 5 * 8);
            builder.def_var(fuel_var, handed_fuel);
            let first_native_call = builder.ins().iconst(types::I64, 1);
            builder.def_var(native_calls_var, first_native_call);
            builder.ins().jump(blocks[block_for_ip[0]], &[]);

            let mut stack = Vec::new();
            for (ip, instruction) in function.code.iter().enumerate() {
                if let Some(leader_index) = leader_indices[ip] {
                    builder.switch_to_block(blocks[leader_index]);
                    stack = stack_params[leader_index].clone();
                }
                if depths[ip].is_none() {
                    let fault_ip = builder.ins().iconst(types::I64, ip as i64);
                    let calls = builder.use_var(native_calls_var);
                    emit_exit(
                        &mut builder,
                        output_pointer,
                        1,
                        zero_steps,
                        fault_ip,
                        index,
                        calls,
                    );
                    continue;
                }
                let polls_cancellation = ip == 0
                    || matches!(
                        &instruction.op,
                        Op::Jump(target) | Op::JumpIfFalse(target) if *target <= ip
                    );
                if polls_cancellation {
                    let exit_args =
                        step_exit_args(&mut builder, steps_var, ip, block_instruction_offsets[ip]);
                    let cancellation = builder.ins().call(poll_cancellation, &[budget_pointer]);
                    let cancellation = builder.inst_results(cancellation)[0];
                    let no_cancel = builder.ins().iconst(types::I32, 0);
                    let is_cancelled = builder.ins().icmp(IntCC::NotEqual, cancellation, no_cancel);
                    let check_fuel = builder.create_block();
                    builder
                        .ins()
                        .brif(is_cancelled, cancelled, &exit_args, check_fuel, &[]);
                    builder.switch_to_block(check_fuel);
                    builder.seal_block(check_fuel);
                }
                if let Some(required) = leader_indices[ip]
                    .map(|block_index| fuel_reservation_requirements[block_index])
                    .filter(|required| *required > 0)
                {
                    let exit_args =
                        step_exit_args(&mut builder, steps_var, ip, block_instruction_offsets[ip]);
                    let remaining_fuel = builder.use_var(fuel_var);
                    let required_fuel = builder.ins().iconst(types::I64, required as i64);
                    let enough_fuel = builder.ins().icmp(
                        IntCC::SignedGreaterThanOrEqual,
                        remaining_fuel,
                        required_fuel,
                    );
                    let execute = builder.create_block();
                    let refill = builder.create_block();
                    builder.ins().brif(enough_fuel, execute, &[], refill, &[]);
                    builder.switch_to_block(refill);
                    builder
                        .ins()
                        .call(release_fuel, &[budget_pointer, remaining_fuel]);
                    let zero_fuel = builder.ins().iconst(types::I64, 0);
                    builder.def_var(fuel_var, zero_fuel);
                    let chunk = builder.ins().iconst(types::I64, 256.max(required) as i64);
                    let reserve_call = builder.ins().call(reserve_fuel, &[budget_pointer, chunk]);
                    let granted = builder.inst_results(reserve_call)[0];
                    builder.def_var(fuel_var, granted);
                    let has_grant =
                        builder
                            .ins()
                            .icmp(IntCC::SignedGreaterThanOrEqual, granted, required_fuel);
                    let refilled = builder.create_block();
                    builder
                        .ins()
                        .brif(has_grant, refilled, &[], fallback, &exit_args);
                    builder.switch_to_block(refilled);
                    builder.ins().jump(execute, &[]);
                    builder.switch_to_block(execute);
                    builder.seal_block(execute);
                }
                let instruction_stack = stack.clone();
                match &instruction.op {
                    Op::Const(Value::Int(value)) => {
                        let Ok(value) = value.to_string().parse::<i64>() else {
                            let fault_ip = builder.ins().iconst(types::I64, ip as i64);
                            let unused_fuel = fuel_count_after(
                                &mut builder,
                                fuel_var,
                                block_instruction_offsets[ip] + 1,
                            );
                            store_output(&mut builder, output_pointer, 5, unused_fuel);
                            let steps = step_count_after(
                                &mut builder,
                                steps_var,
                                block_instruction_offsets[ip] + 1,
                            );
                            let calls = builder.use_var(native_calls_var);
                            emit_exit(
                                &mut builder,
                                output_pointer,
                                1,
                                steps,
                                fault_ip,
                                index,
                                calls,
                            );
                            continue;
                        };
                        stack.push(builder.ins().iconst(types::I64, value));
                    }
                    Op::Const(constant @ (Value::Text(_) | Value::Bytes(_))) => {
                        let clone_bytes = guion_vm_core::cloned_value_allocation(constant);
                        let handle = self
                            .constant_handles
                            .get(index)
                            .and_then(|handles| handles.get(ip))
                            .copied()
                            .flatten()
                            .ok_or_else(|| "native constant handle is missing".to_owned())?;
                        let handle_value = builder.ins().iconst(types::I64, handle);
                        let bytes = builder
                            .ins()
                            .iconst(types::I64, i64::try_from(clone_bytes).unwrap_or(i64::MAX));
                        let charged = builder.ins().call(charge_record, &[arena_pointer, bytes]);
                        let charged = builder.inst_results(charged)[0];
                        let zero = builder.ins().iconst(types::I32, 0);
                        let succeeded = builder.ins().icmp(IntCC::NotEqual, charged, zero);
                        let success = builder.create_block();
                        let exit_args = step_exit_args(
                            &mut builder,
                            steps_var,
                            ip,
                            block_instruction_offsets[ip] + 1,
                        );
                        builder
                            .ins()
                            .brif(succeeded, success, &[], fallback, &exit_args);
                        builder.switch_to_block(success);
                        stack.push(handle_value);
                        builder.seal_block(success);
                    }
                    Op::Const(Value::Bool(value)) => {
                        stack.push(builder.ins().iconst(types::I64, i64::from(*value)));
                    }
                    Op::Const(Value::Float64(value)) => {
                        stack.push(
                            builder
                                .ins()
                                .iconst(types::I64, value.get().to_bits() as i64),
                        );
                    }
                    Op::MakeList(count) => {
                        let type_stack = stack_types[ip]
                            .as_ref()
                            .ok_or_else(|| "missing list element types".to_owned())?;
                        let first = type_stack
                            .len()
                            .checked_sub(*count)
                            .and_then(|start| type_stack.get(start))
                            .ok_or_else(|| "unsupported empty untyped list".to_owned())?;
                        let item_kind = native_list_kind(first)
                            .ok_or_else(|| "unsupported native list item type".to_owned())?;
                        let mut values = Vec::with_capacity(*count);
                        for _ in 0..*count {
                            values.push(stack.pop().ok_or_else(|| "stack underflow".to_owned())?);
                        }
                        values.reverse();
                        let slot = builder.create_sized_stack_slot(StackSlotData::new(
                            StackSlotKind::ExplicitSlot,
                            ((*count).max(1).saturating_mul(8)) as u32,
                            3,
                        ));
                        let items_pointer = builder.ins().stack_addr(ptr, slot, 0);
                        for (item, value) in values.iter().enumerate() {
                            builder.ins().store(
                                MemFlagsData::new(),
                                *value,
                                items_pointer,
                                (item * 8) as i32,
                            );
                        }
                        let count_value = builder.ins().iconst(
                            types::I64,
                            i64::try_from(*count).map_err(|_| "list is too long".to_owned())?,
                        );
                        let kind_value = builder.ins().iconst(types::I64, item_kind);
                        let not_typed_empty = builder.ins().iconst(types::I64, 0);
                        let result = builder.ins().call(
                            make_list,
                            &[
                                arena_pointer,
                                items_pointer,
                                count_value,
                                kind_value,
                                not_typed_empty,
                            ],
                        );
                        let list = builder.inst_results(result)[0];
                        let null = builder.ins().iconst(ptr, 0);
                        let succeeded = builder.ins().icmp(IntCC::NotEqual, list, null);
                        let success = builder.create_block();
                        let exit_args = step_exit_args(
                            &mut builder,
                            steps_var,
                            ip,
                            block_instruction_offsets[ip] + 1,
                        );
                        builder
                            .ins()
                            .brif(succeeded, success, &[], fallback, &exit_args);
                        builder.switch_to_block(success);
                        stack.push(list);
                        builder.seal_block(success);
                    }
                    Op::MakeTypedList(item_type) => {
                        let item_kind = native_list_kind(item_type)
                            .ok_or_else(|| "unsupported native typed-list item".to_owned())?;
                        let count = builder.ins().iconst(types::I64, 0);
                        let kind = builder.ins().iconst(types::I64, item_kind);
                        let typed_empty = builder.ins().iconst(types::I64, 1);
                        let no_items = builder.ins().iconst(ptr, 0);
                        let result = builder.ins().call(
                            make_list,
                            &[arena_pointer, no_items, count, kind, typed_empty],
                        );
                        let list = builder.inst_results(result)[0];
                        let null = builder.ins().iconst(ptr, 0);
                        let succeeded = builder.ins().icmp(IntCC::NotEqual, list, null);
                        let success = builder.create_block();
                        let exit_args = step_exit_args(
                            &mut builder,
                            steps_var,
                            ip,
                            block_instruction_offsets[ip] + 1,
                        );
                        builder
                            .ins()
                            .brif(succeeded, success, &[], fallback, &exit_args);
                        builder.switch_to_block(success);
                        stack.push(list);
                        builder.seal_block(success);
                    }
                    Op::Load(slot) => {
                        let value = builder.use_var(local_vars[*slot]);
                        let loaded_type = stack_types
                            .get(ip + 1)
                            .and_then(Option::as_ref)
                            .and_then(|types| types.last());
                        let clone_bytes = loaded_type.map(native_cloned_value_bytes).unwrap_or(0);
                        if loaded_type.is_some_and(native_value_clone_uses_helper) {
                            let exit_args = step_exit_args(
                                &mut builder,
                                steps_var,
                                ip,
                                block_instruction_offsets[ip] + 1,
                            );
                            let charged = builder
                                .ins()
                                .call(charge_value_clone, &[arena_pointer, value]);
                            let charged = builder.inst_results(charged)[0];
                            let zero = builder.ins().iconst(types::I32, 0);
                            let succeeded = builder.ins().icmp(IntCC::NotEqual, charged, zero);
                            let success = builder.create_block();
                            builder
                                .ins()
                                .brif(succeeded, success, &[], fallback, &exit_args);
                            builder.switch_to_block(success);
                            builder.seal_block(success);
                        } else if clone_bytes > 0 {
                            let exit_args = step_exit_args(
                                &mut builder,
                                steps_var,
                                ip,
                                block_instruction_offsets[ip] + 1,
                            );
                            let bytes = builder
                                .ins()
                                .iconst(types::I64, i64::try_from(clone_bytes).unwrap_or(i64::MAX));
                            let charged =
                                builder.ins().call(charge_record, &[arena_pointer, bytes]);
                            let charged = builder.inst_results(charged)[0];
                            let zero = builder.ins().iconst(types::I32, 0);
                            let succeeded = builder.ins().icmp(IntCC::NotEqual, charged, zero);
                            let success = builder.create_block();
                            builder
                                .ins()
                                .brif(succeeded, success, &[], fallback, &exit_args);
                            builder.switch_to_block(success);
                            builder.seal_block(success);
                        }
                        stack.push(value);
                    }
                    Op::GetField(record_index, field_index) => {
                        let definition = records
                            .get(*record_index)
                            .ok_or_else(|| "record definition is missing".to_owned())?;
                        if native_record_has_native_layout(definition) {
                            let record = stack.pop().ok_or_else(|| "stack underflow".to_owned())?;
                            let fields = builder.ins().load(
                                ptr,
                                MemFlagsData::new(),
                                record,
                                std::mem::offset_of!(NativeRecord, fields) as i32,
                            );
                            let field_address = builder
                                .ins()
                                .iadd_imm_s(fields, (*field_index).saturating_mul(8) as i64);
                            let value = builder.ins().load(
                                types::I64,
                                MemFlagsData::new(),
                                field_address,
                                0,
                            );
                            let field_type =
                                definition.fields.get(*field_index).map(|field| &field.ty);
                            let clone_bytes =
                                field_type.map(native_cloned_value_bytes).unwrap_or(0);
                            if clone_bytes > 0 {
                                let exit_args = step_exit_args(
                                    &mut builder,
                                    steps_var,
                                    ip,
                                    block_instruction_offsets[ip] + 1,
                                );
                                let bytes = builder.ins().iconst(
                                    types::I64,
                                    i64::try_from(clone_bytes).unwrap_or(i64::MAX),
                                );
                                let charged =
                                    builder.ins().call(charge_record, &[arena_pointer, bytes]);
                                let charged = builder.inst_results(charged)[0];
                                let zero = builder.ins().iconst(types::I32, 0);
                                let succeeded = builder.ins().icmp(IntCC::NotEqual, charged, zero);
                                let success = builder.create_block();
                                builder
                                    .ins()
                                    .brif(succeeded, success, &[], fallback, &exit_args);
                                builder.switch_to_block(success);
                                builder.seal_block(success);
                            }
                            stack.push(value);
                        } else {
                            let input_types = stack_types[ip]
                                .as_ref()
                                .ok_or_else(|| "missing native bridge input types".to_owned())?;
                            let argument_start = input_types
                                .len()
                                .checked_sub(1)
                                .ok_or_else(|| "native bridge stack underflow".to_owned())?;
                            let arguments = stack.split_off(argument_start);
                            let result_type = stack_types
                                .get(ip + 1)
                                .and_then(Option::as_ref)
                                .and_then(|types| types.last())
                                .ok_or_else(|| "missing native bridge result type".to_owned())?;
                            let local_tags =
                                continuation_local_tags(function, &type_states, ip, records);
                            let stack_tags = continuation_stack_tags(
                                &type_states,
                                ip,
                                instruction_stack.len(),
                                records,
                            );
                            let value = emit_native_direct_operation_call(
                                &mut builder,
                                ptr,
                                direct_operations[5],
                                continuation_codegen,
                                ip,
                                block_instruction_offsets[ip],
                                &arguments,
                                &input_types[argument_start..],
                                result_type,
                                &local_tags,
                                &instruction_stack,
                                &stack_tags,
                            )?;
                            stack.push(value);
                        }
                    }
                    Op::MakeRecord(record_index, field_count) => {
                        let definition = records
                            .get(*record_index)
                            .ok_or_else(|| "record definition is missing".to_owned())?;
                        if definition.fields.len() != *field_count {
                            return Err("record field count mismatch".into());
                        }
                        if !native_record_has_native_layout(definition) {
                            let input_types = stack_types[ip]
                                .as_ref()
                                .ok_or_else(|| "missing native bridge input types".to_owned())?;
                            let argument_start = input_types
                                .len()
                                .checked_sub(*field_count)
                                .ok_or_else(|| "native bridge stack underflow".to_owned())?;
                            let arguments = stack.split_off(argument_start);
                            let result_type = stack_types
                                .get(ip + 1)
                                .and_then(Option::as_ref)
                                .and_then(|types| types.last())
                                .ok_or_else(|| "missing native bridge result type".to_owned())?;
                            let local_tags =
                                continuation_local_tags(function, &type_states, ip, records);
                            let stack_tags = continuation_stack_tags(
                                &type_states,
                                ip,
                                instruction_stack.len(),
                                records,
                            );
                            let value = emit_native_make_record_call(
                                &mut builder,
                                ptr,
                                make_record,
                                continuation_codegen,
                                ip,
                                block_instruction_offsets[ip],
                                &arguments,
                                &input_types[argument_start..],
                                result_type,
                                &local_tags,
                                &instruction_stack,
                                &stack_tags,
                            )?;
                            stack.push(value);
                        } else {
                            let charge_bytes = std::mem::size_of::<Vec<Value>>()
                                .saturating_add(
                                    field_count.saturating_mul(std::mem::size_of::<Value>()),
                                )
                                .saturating_add(definition.name.len());
                            let charge_bytes = builder.ins().iconst(
                                types::I64,
                                i64::try_from(charge_bytes).unwrap_or(i64::MAX),
                            );
                            let charged = builder
                                .ins()
                                .call(charge_record, &[arena_pointer, charge_bytes]);
                            let charged = builder.inst_results(charged)[0];
                            let zero = builder.ins().iconst(types::I32, 0);
                            let succeeded = builder.ins().icmp(IntCC::NotEqual, charged, zero);
                            let success = builder.create_block();
                            let exit_args = step_exit_args(
                                &mut builder,
                                steps_var,
                                ip,
                                block_instruction_offsets[ip] + 1,
                            );
                            builder
                                .ins()
                                .brif(succeeded, success, &[], fallback, &exit_args);
                            builder.switch_to_block(success);
                            let mut fields = Vec::with_capacity(*field_count);
                            for _ in 0..*field_count {
                                fields
                                    .push(stack.pop().ok_or_else(|| "stack underflow".to_owned())?);
                            }
                            fields.reverse();
                            let fields_slot = builder.create_sized_stack_slot(StackSlotData::new(
                                StackSlotKind::ExplicitSlot,
                                ((*field_count).max(1).saturating_mul(8)) as u32,
                                3,
                            ));
                            let fields_pointer = builder.ins().stack_addr(ptr, fields_slot, 0);
                            for (field, value) in fields.iter().enumerate() {
                                builder.ins().store(
                                    MemFlagsData::new(),
                                    *value,
                                    fields_pointer,
                                    (field * 8) as i32,
                                );
                            }
                            let descriptor_slot = builder.create_sized_stack_slot(
                                StackSlotData::new(StackSlotKind::ExplicitSlot, 16, 3),
                            );
                            let descriptor = builder.ins().stack_addr(ptr, descriptor_slot, 0);
                            builder
                                .ins()
                                .store(MemFlagsData::new(), fields_pointer, descriptor, 0);
                            let count = builder.ins().iconst(types::I64, *field_count as i64);
                            builder.ins().store(
                                MemFlagsData::new(),
                                count,
                                descriptor,
                                std::mem::offset_of!(NativeRecord, len) as i32,
                            );
                            stack.push(descriptor);
                        }
                    }
                    Op::Length => {
                        let handle = stack.pop().ok_or_else(|| "stack underflow".to_owned())?;
                        let list = handle;
                        let length = builder.ins().load(
                            types::I64,
                            MemFlagsData::new(),
                            list,
                            std::mem::offset_of!(NativeFloatList, len) as i32,
                        );
                        stack.push(length);
                    }
                    Op::Index => {
                        let map_lookup = stack_types[ip]
                            .as_ref()
                            .and_then(|types| types.get(types.len().saturating_sub(2)))
                            .is_some_and(|ty| matches!(ty, Type::Map(_)));
                        if map_lookup {
                            let key = stack.pop().ok_or_else(|| "stack underflow".to_owned())?;
                            let map = stack.pop().ok_or_else(|| "stack underflow".to_owned())?;
                            let input_types = stack_types[ip]
                                .as_ref()
                                .ok_or_else(|| "missing map index input types".to_owned())?;
                            let argument_start = input_types
                                .len()
                                .checked_sub(2)
                                .ok_or_else(|| "map index stack underflow".to_owned())?;
                            let result_type = stack_types
                                .get(ip + 1)
                                .and_then(Option::as_ref)
                                .and_then(|types| types.last())
                                .ok_or_else(|| "missing map index result type".to_owned())?;
                            let operation_index = native_direct_operation_index(&instruction.op)
                                .ok_or_else(|| "missing native map index operation".to_owned())?;
                            let local_tags =
                                continuation_local_tags(function, &type_states, ip, records);
                            let stack_tags = continuation_stack_tags(
                                &type_states,
                                ip,
                                instruction_stack.len(),
                                records,
                            );
                            let value = emit_native_direct_operation_call(
                                &mut builder,
                                ptr,
                                direct_operations[operation_index],
                                continuation_codegen,
                                ip,
                                block_instruction_offsets[ip],
                                &[map, key],
                                &input_types[argument_start..],
                                result_type,
                                &local_tags,
                                &instruction_stack,
                                &stack_tags,
                            )?;
                            stack.push(value);
                        } else {
                            let indexed_type = stack_types[ip]
                                .as_ref()
                                .and_then(|types| types.get(types.len().saturating_sub(2)))
                                .and_then(|collection_type| match collection_type {
                                    Type::List(item_type) => Some(item_type.as_ref()),
                                    _ => None,
                                });
                            let index = stack.pop().ok_or_else(|| "stack underflow".to_owned())?;
                            let handle = stack.pop().ok_or_else(|| "stack underflow".to_owned())?;
                            let list = handle;
                            let length = builder.ins().load(
                                types::I64,
                                MemFlagsData::new(),
                                list,
                                std::mem::offset_of!(NativeFloatList, len) as i32,
                            );
                            // Unsigned comparison also rejects negative signed indices, whose
                            // bit patterns sort above any valid list length.
                            let in_bounds =
                                builder.ins().icmp(IntCC::UnsignedLessThan, index, length);
                            let load_item = builder.create_block();
                            let fault_args = step_exit_args(
                                &mut builder,
                                steps_var,
                                ip,
                                block_instruction_offsets[ip] + 1,
                            );
                            builder
                                .ins()
                                .brif(in_bounds, load_item, &[], fallback, &fault_args);
                            builder.switch_to_block(load_item);
                            let data = builder.ins().load(
                                ptr,
                                MemFlagsData::new(),
                                list,
                                std::mem::offset_of!(NativeFloatList, data) as i32,
                            );
                            let byte_offset = builder.ins().ishl_imm_u(index, 3);
                            let item_address = builder.ins().iadd(data, byte_offset);
                            let value = builder.ins().load(
                                types::I64,
                                MemFlagsData::new(),
                                item_address,
                                0,
                            );
                            if indexed_type.is_some_and(native_value_clone_uses_helper) {
                                let exit_args = step_exit_args(
                                    &mut builder,
                                    steps_var,
                                    ip,
                                    block_instruction_offsets[ip] + 1,
                                );
                                let charged = builder
                                    .ins()
                                    .call(charge_value_clone, &[arena_pointer, value]);
                                let charged = builder.inst_results(charged)[0];
                                let zero = builder.ins().iconst(types::I32, 0);
                                let succeeded = builder.ins().icmp(IntCC::NotEqual, charged, zero);
                                let success = builder.create_block();
                                builder
                                    .ins()
                                    .brif(succeeded, success, &[], fallback, &exit_args);
                                builder.switch_to_block(success);
                                builder.seal_block(success);
                            } else if let Some(clone_bytes) =
                                indexed_type.map(native_cloned_value_bytes)
                            {
                                if clone_bytes > 0 {
                                    let exit_args = step_exit_args(
                                        &mut builder,
                                        steps_var,
                                        ip,
                                        block_instruction_offsets[ip] + 1,
                                    );
                                    let bytes = builder.ins().iconst(
                                        types::I64,
                                        i64::try_from(clone_bytes).unwrap_or(i64::MAX),
                                    );
                                    let charged =
                                        builder.ins().call(charge_record, &[arena_pointer, bytes]);
                                    let charged = builder.inst_results(charged)[0];
                                    let zero = builder.ins().iconst(types::I32, 0);
                                    let succeeded =
                                        builder.ins().icmp(IntCC::NotEqual, charged, zero);
                                    let success = builder.create_block();
                                    builder.ins().brif(
                                        succeeded,
                                        success,
                                        &[],
                                        fallback,
                                        &exit_args,
                                    );
                                    builder.switch_to_block(success);
                                    builder.seal_block(success);
                                }
                            }
                            stack.push(value);
                            builder.seal_block(load_item);
                        }
                    }
                    Op::Store(slot) => {
                        let Some(value) = stack.pop() else {
                            return Err("stack underflow".into());
                        };
                        builder.def_var(local_vars[*slot], value);
                    }
                    Op::Pop => {
                        stack.pop().ok_or_else(|| "stack underflow".to_owned())?;
                    }
                    Op::Add | Op::Subtract | Op::Multiply => {
                        let right = stack.pop().ok_or_else(|| "stack underflow".to_owned())?;
                        let left = stack.pop().ok_or_else(|| "stack underflow".to_owned())?;
                        let operand_types = stack_types[ip]
                            .as_ref()
                            .ok_or_else(|| "missing arithmetic operand types".to_owned())?;
                        let text_operands = operand_types.len() >= 2
                            && operand_types[operand_types.len() - 2..] == [Type::Text, Type::Text];
                        if text_operands {
                            let argument_start = operand_types.len() - 2;
                            let local_tags =
                                continuation_local_tags(function, &type_states, ip, records);
                            let stack_tags = continuation_stack_tags(
                                &type_states,
                                ip,
                                instruction_stack.len(),
                                records,
                            );
                            let operation_index = native_direct_operation_index(&instruction.op)
                                .ok_or_else(|| "missing text concatenation operation".to_owned())?;
                            let value = emit_native_direct_operation_call(
                                &mut builder,
                                ptr,
                                direct_operations[operation_index],
                                continuation_codegen,
                                ip,
                                block_instruction_offsets[ip],
                                &[left, right],
                                &operand_types[argument_start..],
                                &Type::Text,
                                &local_tags,
                                &instruction_stack,
                                &stack_tags,
                            )?;
                            stack.push(value);
                        } else if operand_types.last() == Some(&Type::Float64) {
                            let left = builder.ins().bitcast(types::F64, MemFlagsData::new(), left);
                            let right =
                                builder
                                    .ins()
                                    .bitcast(types::F64, MemFlagsData::new(), right);
                            let value = match &instruction.op {
                                Op::Add => builder.ins().fadd(left, right),
                                Op::Subtract => builder.ins().fsub(left, right),
                                Op::Multiply => builder.ins().fmul(left, right),
                                _ => unreachable!(),
                            };
                            emit_float_finite_guard(
                                &mut builder,
                                value,
                                float_nonfinite,
                                steps_var,
                                ip,
                                block_instruction_offsets[ip] + 1,
                            );
                            stack.push(builder.ins().bitcast(
                                types::I64,
                                MemFlagsData::new(),
                                value,
                            ));
                        } else {
                            let (value, overflow) = match &instruction.op {
                                Op::Add => builder.ins().sadd_overflow(left, right),
                                Op::Subtract => builder.ins().ssub_overflow(left, right),
                                Op::Multiply => builder.ins().smul_overflow(left, right),
                                _ => unreachable!(),
                            };
                            let local_tags =
                                continuation_local_tags(function, &type_states, ip, records);
                            let stack_tags = continuation_stack_tags(
                                &type_states,
                                ip,
                                instruction_stack.len(),
                                records,
                            );
                            emit_native_overflow_side_exit(
                                &mut builder,
                                overflow,
                                continuation_codegen,
                                ip,
                                block_instruction_offsets[ip],
                                &local_tags,
                                &instruction_stack,
                                &stack_tags,
                            );
                            stack.push(value);
                        }
                    }
                    Op::Negate => {
                        let value = stack.pop().ok_or_else(|| "stack underflow".to_owned())?;
                        if stack_types[ip].as_ref().and_then(|types| types.last())
                            == Some(&Type::Float64)
                        {
                            let sign_mask = builder.ins().iconst(types::I64, i64::MIN);
                            stack.push(builder.ins().bxor(value, sign_mask));
                        } else {
                            let zero = builder.ins().iconst(types::I64, 0);
                            let may_overflow = stack_types[ip].as_ref().is_some_and(|stack| {
                                native_integer_overflow_operation_at(
                                    &instruction.op,
                                    stack,
                                    function,
                                    ip,
                                    &leader_indices,
                                )
                            });
                            if may_overflow {
                                let (negated, overflow) = builder.ins().ssub_overflow(zero, value);
                                let local_tags =
                                    continuation_local_tags(function, &type_states, ip, records);
                                let stack_tags = continuation_stack_tags(
                                    &type_states,
                                    ip,
                                    instruction_stack.len(),
                                    records,
                                );
                                emit_native_overflow_side_exit(
                                    &mut builder,
                                    overflow,
                                    continuation_codegen,
                                    ip,
                                    block_instruction_offsets[ip],
                                    &local_tags,
                                    &instruction_stack,
                                    &stack_tags,
                                );
                                stack.push(negated);
                            } else {
                                stack.push(builder.ins().isub(zero, value));
                            }
                        }
                    }
                    Op::Not => {
                        let value = stack.pop().ok_or_else(|| "stack underflow".to_owned())?;
                        let zero = builder.ins().iconst(types::I64, 0);
                        let result = builder.ins().icmp(IntCC::Equal, value, zero);
                        stack.push(builder.ins().uextend(types::I64, result));
                    }
                    Op::Equal
                    | Op::NotEqual
                    | Op::Less
                    | Op::LessEqual
                    | Op::Greater
                    | Op::GreaterEqual => {
                        let right = stack.pop().ok_or_else(|| "stack underflow".to_owned())?;
                        let left = stack.pop().ok_or_else(|| "stack underflow".to_owned())?;
                        let operand_types = stack_types[ip]
                            .as_ref()
                            .ok_or_else(|| "missing comparison operand types".to_owned())?;
                        let text_operands = operand_types.len() >= 2
                            && operand_types[operand_types.len() - 2..] == [Type::Text, Type::Text];
                        if text_operands {
                            let argument_start = operand_types.len() - 2;
                            let result_type = Type::Bool;
                            let operation_index = native_direct_operation_index(&instruction.op)
                                .ok_or_else(|| "missing text equality operation".to_owned())?;
                            let local_tags =
                                continuation_local_tags(function, &type_states, ip, records);
                            let stack_tags = continuation_stack_tags(
                                &type_states,
                                ip,
                                instruction_stack.len(),
                                records,
                            );
                            let result = emit_native_direct_operation_call(
                                &mut builder,
                                ptr,
                                direct_operations[operation_index],
                                continuation_codegen,
                                ip,
                                block_instruction_offsets[ip],
                                &[left, right],
                                &operand_types[argument_start..],
                                &result_type,
                                &local_tags,
                                &instruction_stack,
                                &stack_tags,
                            )?;
                            stack.push(result);
                        } else {
                            let float_operands = operand_types.len() >= 2
                                && operand_types[operand_types.len() - 2] == Type::Float64;
                            if float_operands {
                                let left =
                                    builder.ins().bitcast(types::F64, MemFlagsData::new(), left);
                                let right =
                                    builder
                                        .ins()
                                        .bitcast(types::F64, MemFlagsData::new(), right);
                                let condition = match &instruction.op {
                                    Op::Equal => FloatCC::Equal,
                                    Op::NotEqual => FloatCC::NotEqual,
                                    Op::Less => FloatCC::LessThan,
                                    Op::LessEqual => FloatCC::LessThanOrEqual,
                                    Op::Greater => FloatCC::GreaterThan,
                                    Op::GreaterEqual => FloatCC::GreaterThanOrEqual,
                                    _ => unreachable!(),
                                };
                                let result = builder.ins().fcmp(condition, left, right);
                                stack.push(builder.ins().uextend(types::I64, result));
                            } else {
                                let condition = match &instruction.op {
                                    Op::Equal => IntCC::Equal,
                                    Op::NotEqual => IntCC::NotEqual,
                                    Op::Less => IntCC::SignedLessThan,
                                    Op::LessEqual => IntCC::SignedLessThanOrEqual,
                                    Op::Greater => IntCC::SignedGreaterThan,
                                    Op::GreaterEqual => IntCC::SignedGreaterThanOrEqual,
                                    _ => unreachable!(),
                                };
                                let result = builder.ins().icmp(condition, left, right);
                                stack.push(builder.ins().uextend(types::I64, result));
                            }
                        }
                    }
                    Op::Jump(target) => {
                        if *target <= ip
                            && self.loop_safepoint_functions[index]
                            && loop_backedge_allocates_arena_values(
                                function,
                                &self.reaches_arena_allocation,
                                ip,
                            )
                        {
                            let roots = loop_safepoint_roots(
                                function,
                                &type_states,
                                &live_locals_at_ip,
                                records,
                                ip,
                                *target,
                            )
                            .ok_or_else(|| "loop safepoint roots are unsupported".to_owned())?;
                            let mut tags =
                                continuation_local_tags(function, &type_states, *target, records);
                            if let Some(Some(locals)) = type_states.locals_at_ip.get(*target) {
                                for slot in &roots {
                                    if matches!(&locals[*slot], Some(Type::List(item)) if **item == Type::Text)
                                    {
                                        tags[*slot] = crate::NATIVE_SAFEPOINT_TEXT_LIST_TAG;
                                    }
                                }
                            }
                            let values: Vec<_> = roots
                                .iter()
                                .map(|slot| builder.use_var(local_vars[*slot]))
                                .collect();
                            let triples = emit_local_continuation_triples(
                                &mut builder,
                                ptr,
                                &roots,
                                &values,
                                &tags,
                            );
                            let count = builder.ins().iconst(types::I64, roots.len() as i64);
                            let call = builder
                                .ins()
                                .call(loop_safepoint, &[arena_pointer, triples, count]);
                            let failed = builder.inst_results(call)[0];
                            let zero = builder.ins().iconst(types::I32, 0);
                            let succeeded = builder.ins().icmp(IntCC::Equal, failed, zero);
                            let exit_args = step_exit_args(
                                &mut builder,
                                steps_var,
                                ip,
                                block_instruction_offsets[ip] + 1,
                            );
                            let resumed = builder.create_block();
                            builder
                                .ins()
                                .brif(succeeded, resumed, &[], fallback, &exit_args);
                            builder.switch_to_block(resumed);
                            builder.seal_block(resumed);
                            for (position, slot) in roots.iter().enumerate() {
                                let refreshed = builder.ins().load(
                                    types::I64,
                                    MemFlagsData::new(),
                                    triples,
                                    (position * 24 + 16) as i32,
                                );
                                builder.def_var(local_vars[*slot], refreshed);
                            }
                        }
                        let completed_instructions = block_instruction_counts[block_for_ip[ip]];
                        let block_steps =
                            step_count_after(&mut builder, steps_var, completed_instructions);
                        builder.def_var(steps_var, block_steps);
                        let remaining_fuel =
                            fuel_count_after(&mut builder, fuel_var, completed_instructions);
                        builder.def_var(fuel_var, remaining_fuel);
                        let block_args: Vec<_> =
                            stack.iter().copied().map(BlockArg::Value).collect();
                        builder
                            .ins()
                            .jump(blocks[block_for_ip[*target]], &block_args);
                        continue;
                    }
                    Op::Call(target, argc) => {
                        let completed_instructions = block_instruction_counts[block_for_ip[ip]];
                        let steps =
                            step_count_after(&mut builder, steps_var, completed_instructions);
                        let remaining_fuel =
                            fuel_count_after(&mut builder, fuel_var, completed_instructions);
                        builder.def_var(fuel_var, remaining_fuel);
                        let _function_id = function_ids
                            .get(*target)
                            .and_then(|function_id| *function_id)
                            .ok_or_else(|| "callee is not native".to_owned())?;
                        let mut call_args = Vec::with_capacity(*argc);
                        for _ in 0..*argc {
                            call_args
                                .push(stack.pop().ok_or_else(|| "stack underflow".to_owned())?);
                        }
                        call_args.reverse();
                        let args_slot = builder.create_sized_stack_slot(StackSlotData::new(
                            StackSlotKind::ExplicitSlot,
                            ((*argc).max(1).saturating_mul(8)) as u32,
                            3,
                        ));
                        let output_slot = builder.create_sized_stack_slot(StackSlotData::new(
                            StackSlotKind::ExplicitSlot,
                            ((max_result_words + 5) * 8) as u32,
                            3,
                        ));
                        let args_address = builder.ins().stack_addr(ptr, args_slot, 0);
                        for (argument_index, argument) in call_args.iter().enumerate() {
                            builder.ins().store(
                                MemFlagsData::new(),
                                *argument,
                                args_address,
                                (argument_index * 8) as i32,
                            );
                        }
                        let child_output = builder.ins().stack_addr(ptr, output_slot, 0);
                        let child_metadata = builder.ins().iadd_imm_s(
                            child_output,
                            (max_result_words.saturating_sub(1) * 8) as i64,
                        );
                        let trace_offset = builder.ins().ishl_imm_s(depth_value, 4);
                        let trace_frame = builder.ins().iadd(trace_pointer, trace_offset);
                        let current_function = builder
                            .ins()
                            .iconst(types::I64, index.min(i64::MAX as usize) as i64);
                        let call_ip = builder.ins().iconst(types::I64, ip as i64);
                        builder
                            .ins()
                            .store(MemFlagsData::new(), current_function, trace_frame, 0);
                        builder
                            .ins()
                            .store(MemFlagsData::new(), call_ip, trace_frame, 8);
                        let child_depth = builder.ins().iadd_imm_s(depth_value, 1);
                        builder.ins().store(
                            MemFlagsData::new(),
                            child_depth,
                            trace_length_pointer,
                            0,
                        );
                        let depth_exceeded = builder.ins().icmp(
                            IntCC::SignedGreaterThanOrEqual,
                            child_depth,
                            depth_limit,
                        );
                        let allowed_depth = builder.create_block();
                        let too_deep = builder.create_block();
                        builder
                            .ins()
                            .brif(depth_exceeded, too_deep, &[], allowed_depth, &[]);
                        builder.switch_to_block(too_deep);
                        let unused_fuel = builder.use_var(fuel_var);
                        store_output(&mut builder, output_pointer, 5, unused_fuel);
                        let target_span = builder.ins().iconst(types::I64, 0);
                        let calls = builder.use_var(native_calls_var);
                        emit_exit(
                            &mut builder,
                            output_pointer,
                            5,
                            steps,
                            target_span,
                            *target,
                            calls,
                        );
                        builder.switch_to_block(allowed_depth);
                        let callee = native_function_refs[*target]
                            .ok_or_else(|| "callee is not native".to_owned())?;
                        let handed_fuel = builder.use_var(fuel_var);
                        store_output(&mut builder, child_metadata, 5, handed_fuel);
                        let result = builder.ins().call(
                            callee,
                            &[
                                args_address,
                                child_output,
                                budget_pointer,
                                child_depth,
                                depth_limit,
                                trace_pointer,
                                arena_pointer,
                            ],
                        );
                        let status = builder.inst_results(result)[0];
                        let returned_fuel = builder.ins().load(
                            types::I64,
                            MemFlagsData::new(),
                            child_metadata,
                            5 * 8,
                        );
                        builder.def_var(fuel_var, returned_fuel);
                        let zero_status = builder.ins().iconst(types::I32, 0);
                        let succeeded = builder.ins().icmp(IntCC::Equal, status, zero_status);
                        let success = builder.create_block();
                        let failure = builder.create_block();
                        builder.ins().brif(succeeded, success, &[], failure, &[]);

                        builder.switch_to_block(failure);
                        let local_tags =
                            continuation_local_tags(function, &type_states, ip, records);
                        let stack_tags =
                            continuation_stack_tags(&type_states, ip, stack.len(), records);
                        let _ = emit_native_continuation_capture(
                            &mut builder,
                            ptr,
                            capture_vm_continuation,
                            arena_pointer,
                            index,
                            ip + 1,
                            depth_value,
                            &local_vars,
                            continuation_codegen.live_locals_at_ip,
                            capture_vm_continuation_int2,
                            capture_vm_continuation_fixed,
                            &local_tags,
                            &stack,
                            &stack_tags,
                            ip.min(i64::MAX as usize) as i64,
                        );
                        let unused_fuel = builder.use_var(fuel_var);
                        store_output(&mut builder, output_pointer, 5, unused_fuel);
                        let child_steps =
                            builder
                                .ins()
                                .load(types::I64, MemFlagsData::new(), child_metadata, 8);
                        let total_steps = builder.ins().iadd(steps, child_steps);
                        let fault_ip =
                            builder
                                .ins()
                                .load(types::I64, MemFlagsData::new(), child_metadata, 16);
                        let fault_function =
                            builder
                                .ins()
                                .load(types::I64, MemFlagsData::new(), child_metadata, 24);
                        store_output(&mut builder, output_pointer, 2, fault_ip);
                        store_output(&mut builder, output_pointer, 3, fault_function);
                        store_output(&mut builder, output_pointer, 1, total_steps);
                        let child_calls =
                            builder
                                .ins()
                                .load(types::I64, MemFlagsData::new(), child_metadata, 32);
                        let parent_calls = builder.use_var(native_calls_var);
                        let total_calls = builder.ins().iadd(parent_calls, child_calls);
                        store_output(&mut builder, output_pointer, 4, total_calls);
                        builder.ins().return_(&[status]);

                        builder.switch_to_block(success);
                        builder.ins().store(
                            MemFlagsData::new(),
                            depth_value,
                            trace_length_pointer,
                            0,
                        );
                        let child_steps =
                            builder
                                .ins()
                                .load(types::I64, MemFlagsData::new(), child_metadata, 8);
                        let total_steps = builder.ins().iadd(steps, child_steps);
                        builder.def_var(steps_var, total_steps);
                        let child_calls =
                            builder
                                .ins()
                                .load(types::I64, MemFlagsData::new(), child_metadata, 32);
                        let parent_calls = builder.use_var(native_calls_var);
                        let total_calls = builder.ins().iadd(parent_calls, child_calls);
                        builder.def_var(native_calls_var, total_calls);
                        let callee_result = &all_functions[*target].result;
                        let value = if let Type::Record(name) = callee_result {
                            let fields = records
                                .iter()
                                .find(|record| record.name == *name)
                                .ok_or_else(|| {
                                    "callee record result definition is missing".to_owned()
                                })?
                                .fields
                                .len();
                            let descriptor_slot = builder.create_sized_stack_slot(
                                StackSlotData::new(StackSlotKind::ExplicitSlot, 16, 3),
                            );
                            let descriptor = builder.ins().stack_addr(ptr, descriptor_slot, 0);
                            builder.ins().store(
                                MemFlagsData::new(),
                                child_output,
                                descriptor,
                                std::mem::offset_of!(NativeRecord, fields) as i32,
                            );
                            let field_count = builder.ins().iconst(types::I64, fields as i64);
                            builder.ins().store(
                                MemFlagsData::new(),
                                field_count,
                                descriptor,
                                std::mem::offset_of!(NativeRecord, len) as i32,
                            );
                            descriptor
                        } else {
                            builder
                                .ins()
                                .load(types::I64, MemFlagsData::new(), child_output, 0)
                        };
                        stack.push(value);
                        builder.seal_block(success);
                        builder.seal_block(failure);
                    }
                    Op::JumpIfFalse(target) => {
                        let completed_instructions = block_instruction_counts[block_for_ip[ip]];
                        let block_steps =
                            step_count_after(&mut builder, steps_var, completed_instructions);
                        builder.def_var(steps_var, block_steps);
                        let remaining_fuel =
                            fuel_count_after(&mut builder, fuel_var, completed_instructions);
                        builder.def_var(fuel_var, remaining_fuel);
                        let value = stack.pop().ok_or_else(|| "stack underflow".to_owned())?;
                        if let Some(counter) = count_guest_branch {
                            builder.ins().call(counter, &[arena_pointer]);
                        }
                        let zero = builder.ins().iconst(types::I64, 0);
                        let condition = builder.ins().icmp(IntCC::NotEqual, value, zero);
                        let next = ip + 1;
                        let branch_args: Vec<_> =
                            stack.iter().copied().map(BlockArg::Value).collect();
                        builder.ins().brif(
                            condition,
                            blocks[block_for_ip[next]],
                            &branch_args,
                            blocks[block_for_ip[*target]],
                            &branch_args,
                        );
                        continue;
                    }
                    Op::JsonSetLocal(slot) => {
                        let input_types = stack_types[ip]
                            .as_ref()
                            .ok_or_else(|| "missing JSON set local input types".to_owned())?;
                        let value = stack.pop().ok_or_else(|| "stack underflow".to_owned())?;
                        let key = stack.pop().ok_or_else(|| "stack underflow".to_owned())?;
                        let object = builder.use_var(local_vars[*slot]);
                        let locals_at_ip = type_states
                            .locals_at_ip
                            .get(ip)
                            .and_then(Option::as_ref)
                            .ok_or_else(|| "missing JSON set local types".to_owned())?;
                        let live_aliases = live_locals_at_ip
                            .get(ip)
                            .ok_or_else(|| "missing JSON set local liveness".to_owned())?;
                        let mut alias_count = builder.ins().iconst(types::I64, 0);
                        let mut safe_alias_analysis = true;
                        for (local, ty) in locals_at_ip.iter().enumerate() {
                            let Some(ty) = ty.as_ref() else {
                                continue;
                            };
                            let local_is_live = local == *slot || live_aliases.contains(&local);
                            if !local_is_live {
                                continue;
                            }
                            if may_hide_json_handle_in_native_value(ty) && *ty != Type::Json {
                                safe_alias_analysis = false;
                            }
                            if *ty == Type::Json {
                                let local_value = builder.use_var(local_vars[local]);
                                let aliases_object =
                                    builder.ins().icmp(IntCC::Equal, local_value, object);
                                let alias_bit = builder.ins().uextend(types::I64, aliases_object);
                                alias_count = builder.ins().iadd(alias_count, alias_bit);
                            }
                        }
                        for (stack_index, ty) in input_types.iter().enumerate() {
                            if may_hide_json_handle_in_native_value(ty) && *ty != Type::Json {
                                safe_alias_analysis = false;
                            }
                            if *ty == Type::Json {
                                let stack_value = instruction_stack[stack_index];
                                let aliases_object =
                                    builder.ins().icmp(IntCC::Equal, stack_value, object);
                                let alias_bit = builder.ins().uextend(types::I64, aliases_object);
                                alias_count = builder.ins().iadd(alias_count, alias_bit);
                            }
                        }
                        let one = builder.ins().iconst(types::I64, 1);
                        let exactly_one_reference =
                            builder.ins().icmp(IntCC::Equal, alias_count, one);
                        let unique_target = if safe_alias_analysis {
                            builder.ins().uextend(types::I64, exactly_one_reference)
                        } else {
                            builder.ins().iconst(types::I64, 0)
                        };
                        let arguments = [object, key, value, unique_target];
                        let argument_types = [Type::Json, Type::Text, Type::Json, Type::Bool];
                        let local_tags =
                            continuation_local_tags(function, &type_states, ip, records);
                        let stack_tags = continuation_stack_tags(
                            &type_states,
                            ip,
                            instruction_stack.len(),
                            records,
                        );
                        let operation_index = native_direct_operation_index(&instruction.op)
                            .ok_or_else(|| "missing native JSON set local operation".to_owned())?;
                        let updated = emit_native_direct_operation_call(
                            &mut builder,
                            ptr,
                            direct_operations[operation_index],
                            continuation_codegen,
                            ip,
                            block_instruction_offsets[ip],
                            &arguments,
                            &argument_types,
                            &Type::Json,
                            &local_tags,
                            &instruction_stack,
                            &stack_tags,
                        )?;
                        builder.def_var(local_vars[*slot], updated);
                    }
                    Op::JsonPushLocal(slot) => {
                        let input_types = stack_types[ip]
                            .as_ref()
                            .ok_or_else(|| "missing JSON push local input types".to_owned())?;
                        let value = stack.pop().ok_or_else(|| "stack underflow".to_owned())?;
                        let array = builder.use_var(local_vars[*slot]);
                        let locals_at_ip = type_states
                            .locals_at_ip
                            .get(ip)
                            .and_then(Option::as_ref)
                            .ok_or_else(|| "missing JSON push local types".to_owned())?;
                        let live_aliases = live_locals_at_ip
                            .get(ip)
                            .ok_or_else(|| "missing JSON push local liveness".to_owned())?;
                        let mut alias_count = builder.ins().iconst(types::I64, 0);
                        let mut safe_alias_analysis = true;
                        for (local, ty) in locals_at_ip.iter().enumerate() {
                            let Some(ty) = ty.as_ref() else {
                                continue;
                            };
                            let local_is_live = local == *slot || live_aliases.contains(&local);
                            if !local_is_live {
                                continue;
                            }
                            if may_hide_json_handle_in_native_value(ty) && *ty != Type::Json {
                                safe_alias_analysis = false;
                            }
                            if *ty == Type::Json {
                                let local_value = builder.use_var(local_vars[local]);
                                let aliases_array =
                                    builder.ins().icmp(IntCC::Equal, local_value, array);
                                let alias_bit = builder.ins().uextend(types::I64, aliases_array);
                                alias_count = builder.ins().iadd(alias_count, alias_bit);
                            }
                        }
                        for (stack_index, ty) in input_types.iter().enumerate() {
                            if may_hide_json_handle_in_native_value(ty) && *ty != Type::Json {
                                safe_alias_analysis = false;
                            }
                            if *ty == Type::Json {
                                let stack_value = instruction_stack[stack_index];
                                let aliases_array =
                                    builder.ins().icmp(IntCC::Equal, stack_value, array);
                                let alias_bit = builder.ins().uextend(types::I64, aliases_array);
                                alias_count = builder.ins().iadd(alias_count, alias_bit);
                            }
                        }
                        let one = builder.ins().iconst(types::I64, 1);
                        let exactly_one_reference =
                            builder.ins().icmp(IntCC::Equal, alias_count, one);
                        let unique_target = if safe_alias_analysis {
                            builder.ins().uextend(types::I64, exactly_one_reference)
                        } else {
                            builder.ins().iconst(types::I64, 0)
                        };
                        let arguments = [array, value, unique_target];
                        let argument_types = [Type::Json, Type::Json, Type::Bool];
                        let local_tags =
                            continuation_local_tags(function, &type_states, ip, records);
                        let stack_tags = continuation_stack_tags(
                            &type_states,
                            ip,
                            instruction_stack.len(),
                            records,
                        );
                        let operation_index = native_direct_operation_index(&instruction.op)
                            .ok_or_else(|| "missing native JSON push local operation".to_owned())?;
                        let updated = emit_native_direct_operation_call(
                            &mut builder,
                            ptr,
                            direct_operations[operation_index],
                            continuation_codegen,
                            ip,
                            block_instruction_offsets[ip],
                            &arguments,
                            &argument_types,
                            &Type::Json,
                            &local_tags,
                            &instruction_stack,
                            &stack_tags,
                        )?;
                        builder.def_var(local_vars[*slot], updated);
                    }
                    Op::WriteTextAtomic => {
                        let local_tags =
                            continuation_local_tags(function, &type_states, ip, records);
                        let stack_tags = continuation_stack_tags(
                            &type_states,
                            ip,
                            instruction_stack.len(),
                            records,
                        );
                        emit_native_capture_exit(
                            &mut builder,
                            continuation_codegen,
                            ip,
                            block_instruction_offsets[ip],
                            &local_tags,
                            &instruction_stack,
                            &stack_tags,
                        );
                        continue;
                    }
                    Op::MapSet => {
                        let input_types = stack_types[ip]
                            .as_ref()
                            .ok_or_else(|| "missing map set input types".to_owned())?;
                        let argument_start = input_types
                            .len()
                            .checked_sub(3)
                            .ok_or_else(|| "map set stack underflow".to_owned())?;
                        let map_type = input_types[argument_start].clone();
                        let key_type = input_types[argument_start + 1].clone();
                        let value_type = input_types[argument_start + 2].clone();
                        let value = stack.pop().ok_or_else(|| "stack underflow".to_owned())?;
                        let key = stack.pop().ok_or_else(|| "stack underflow".to_owned())?;
                        let map = stack.pop().ok_or_else(|| "stack underflow".to_owned())?;
                        let result_type = stack_types
                            .get(ip + 1)
                            .and_then(Option::as_ref)
                            .and_then(|types| types.last())
                            .ok_or_else(|| "missing map set result type".to_owned())?;

                        let store_slot = function.code.get(ip + 1).and_then(|next| match next.op {
                            Op::Store(slot) => Some(slot),
                            _ => None,
                        });
                        let target_handle = store_slot
                            .map(|slot| builder.use_var(local_vars[slot]))
                            .unwrap_or(map);
                        let candidate = if store_slot.is_some() {
                            let locals_at_ip = type_states
                                .locals_at_ip
                                .get(ip)
                                .and_then(Option::as_ref)
                                .ok_or_else(|| "missing map set local types".to_owned())?;
                            let mut alias_count = builder.ins().iconst(types::I64, 0);
                            let mut safe_alias_analysis = true;
                            for (local, ty) in locals_at_ip.iter().enumerate() {
                                let Some(ty) = ty.as_ref() else {
                                    continue;
                                };
                                if may_contain_map_handle(ty, records)
                                    && !matches!(ty, Type::Map(_))
                                {
                                    safe_alias_analysis = false;
                                }
                                if matches!(ty, Type::Map(_)) {
                                    let local_value = builder.use_var(local_vars[local]);
                                    let aliases_input =
                                        builder.ins().icmp(IntCC::Equal, local_value, map);
                                    let alias_bit =
                                        builder.ins().uextend(types::I64, aliases_input);
                                    alias_count = builder.ins().iadd(alias_count, alias_bit);
                                }
                            }
                            for (stack_index, ty) in
                                input_types[..argument_start].iter().enumerate()
                            {
                                if may_contain_map_handle(ty, records)
                                    && !matches!(ty, Type::Map(_))
                                {
                                    safe_alias_analysis = false;
                                }
                                if matches!(ty, Type::Map(_)) {
                                    let stack_value = instruction_stack[stack_index];
                                    let aliases_input =
                                        builder.ins().icmp(IntCC::Equal, stack_value, map);
                                    let alias_bit =
                                        builder.ins().uextend(types::I64, aliases_input);
                                    alias_count = builder.ins().iadd(alias_count, alias_bit);
                                }
                            }
                            let target_matches =
                                builder.ins().icmp(IntCC::Equal, target_handle, map);
                            let one = builder.ins().iconst(types::I64, 1);
                            let exactly_one_reference =
                                builder.ins().icmp(IntCC::Equal, alias_count, one);
                            let unique_target =
                                builder.ins().band(target_matches, exactly_one_reference);
                            if safe_alias_analysis {
                                builder.ins().uextend(types::I64, unique_target)
                            } else {
                                builder.ins().iconst(types::I64, 0)
                            }
                        } else {
                            builder.ins().iconst(types::I64, 0)
                        };
                        let arguments = [map, key, value, target_handle, candidate];
                        let argument_types = [
                            map_type,
                            key_type,
                            value_type.clone(),
                            input_types[argument_start].clone(),
                            Type::Bool,
                        ];
                        let local_tags =
                            continuation_local_tags(function, &type_states, ip, records);
                        let stack_tags = continuation_stack_tags(
                            &type_states,
                            ip,
                            instruction_stack.len(),
                            records,
                        );
                        let operation_index = native_direct_operation_index(&instruction.op)
                            .ok_or_else(|| "missing native map set operation".to_owned())?;
                        let mut argument_tag_overrides = vec![None; arguments.len()];
                        if let Type::Record(name) = &value_type {
                            if let Some(record_index) = records.iter().position(|record| {
                                record.name == *name && native_record_has_native_layout(record)
                            }) {
                                argument_tag_overrides[2] = Some(
                                    native_record_bridge_tag(record_index)
                                        .ok_or_else(|| "native record tag overflow".to_owned())?,
                                );
                            }
                        }
                        let result = emit_native_direct_operation_call_with_argument_tag_overrides(
                            &mut builder,
                            ptr,
                            direct_operations[operation_index],
                            continuation_codegen,
                            ip,
                            block_instruction_offsets[ip],
                            &arguments,
                            &argument_types,
                            &argument_tag_overrides,
                            result_type,
                            &local_tags,
                            &instruction_stack,
                            &stack_tags,
                        )?;
                        stack.push(result);
                    }
                    Op::ListSetLocal(slot) => {
                        let value = stack.pop().ok_or_else(|| "stack underflow".to_owned())?;
                        let list_index = stack.pop().ok_or_else(|| "stack underflow".to_owned())?;
                        let list_handle = builder.use_var(local_vars[*slot]);
                        let item_type = stack_types[ip]
                            .as_ref()
                            .and_then(|types| types.last())
                            .ok_or_else(|| "missing list update item type".to_owned())?;
                        let list_set =
                            native_list_set_function(item_type, false, list_set_functions)
                                .ok_or_else(|| "unsupported native list item".to_owned())?;
                        let result = builder
                            .ins()
                            .call(list_set, &[arena_pointer, list_handle, list_index, value]);
                        let updated = builder.inst_results(result)[0];
                        let null = builder.ins().iconst(ptr, 0);
                        let succeeded = builder.ins().icmp(IntCC::NotEqual, updated, null);
                        let success = builder.create_block();
                        let exit_args = step_exit_args(
                            &mut builder,
                            steps_var,
                            ip,
                            block_instruction_offsets[ip] + 1,
                        );
                        builder
                            .ins()
                            .brif(succeeded, success, &[], fallback, &exit_args);
                        builder.switch_to_block(success);
                        builder.def_var(local_vars[*slot], updated);
                        builder.seal_block(success);
                    }
                    Op::ListSet => {
                        let value = stack.pop().ok_or_else(|| "stack underflow".to_owned())?;
                        let list_index = stack.pop().ok_or_else(|| "stack underflow".to_owned())?;
                        let list = stack.pop().ok_or_else(|| "stack underflow".to_owned())?;
                        let type_stack = stack_types[ip]
                            .as_ref()
                            .ok_or_else(|| "missing list update types".to_owned())?;
                        let item_type = type_stack
                            .get(type_stack.len().checked_sub(1).ok_or("stack underflow")?)
                            .ok_or_else(|| "missing list item type".to_owned())?;
                        let list_set =
                            native_list_set_function(item_type, true, list_set_functions)
                                .ok_or_else(|| "unsupported native list item".to_owned())?;
                        let result = builder
                            .ins()
                            .call(list_set, &[arena_pointer, list, list_index, value]);
                        let updated = builder.inst_results(result)[0];
                        let null = builder.ins().iconst(ptr, 0);
                        let succeeded = builder.ins().icmp(IntCC::NotEqual, updated, null);
                        let success = builder.create_block();
                        let exit_args = step_exit_args(
                            &mut builder,
                            steps_var,
                            ip,
                            block_instruction_offsets[ip] + 1,
                        );
                        builder
                            .ins()
                            .brif(succeeded, success, &[], fallback, &exit_args);
                        builder.switch_to_block(success);
                        stack.push(updated);
                        builder.seal_block(success);
                    }
                    Op::Divide => {
                        let right = stack.pop().ok_or_else(|| "stack underflow".to_owned())?;
                        let left = stack.pop().ok_or_else(|| "stack underflow".to_owned())?;
                        if stack_types[ip].as_ref().and_then(|types| types.last())
                            == Some(&Type::Float64)
                        {
                            let left = builder.ins().bitcast(types::F64, MemFlagsData::new(), left);
                            let right =
                                builder
                                    .ins()
                                    .bitcast(types::F64, MemFlagsData::new(), right);
                            let zero = builder.ins().f64const(0.0);
                            let is_zero = builder.ins().fcmp(FloatCC::Equal, right, zero);
                            let continue_division = builder.create_block();
                            let exit_args = step_exit_args(
                                &mut builder,
                                steps_var,
                                ip,
                                block_instruction_offsets[ip] + 1,
                            );
                            builder.ins().brif(
                                is_zero,
                                divide_zero,
                                &exit_args,
                                continue_division,
                                &[],
                            );
                            builder.switch_to_block(continue_division);
                            builder.seal_block(continue_division);
                            let value = builder.ins().fdiv(left, right);
                            emit_float_finite_guard(
                                &mut builder,
                                value,
                                float_nonfinite,
                                steps_var,
                                ip,
                                block_instruction_offsets[ip] + 1,
                            );
                            stack.push(builder.ins().bitcast(
                                types::I64,
                                MemFlagsData::new(),
                                value,
                            ));
                        } else {
                            let zero = builder.ins().iconst(types::I64, 0);
                            let is_zero = builder.ins().icmp(IntCC::Equal, right, zero);
                            let continue_division = builder.create_block();
                            let exit_args = step_exit_args(
                                &mut builder,
                                steps_var,
                                ip,
                                block_instruction_offsets[ip] + 1,
                            );
                            builder.ins().brif(
                                is_zero,
                                divide_zero,
                                &exit_args,
                                continue_division,
                                &[],
                            );
                            builder.switch_to_block(continue_division);
                            builder.seal_block(continue_division);
                            let needs_overflow_exit =
                                stack_types[ip].as_ref().is_some_and(|stack| {
                                    native_integer_overflow_operation_at(
                                        &instruction.op,
                                        stack,
                                        function,
                                        ip,
                                        &leader_indices,
                                    )
                                });
                            if needs_overflow_exit {
                                let min = builder.ins().iconst(types::I64, i64::MIN);
                                let minus_one = builder.ins().iconst(types::I64, -1);
                                let is_min = builder.ins().icmp(IntCC::Equal, left, min);
                                let is_minus_one =
                                    builder.ins().icmp(IntCC::Equal, right, minus_one);
                                let overflow = builder.ins().band(is_min, is_minus_one);
                                let local_tags =
                                    continuation_local_tags(function, &type_states, ip, records);
                                let stack_tags = continuation_stack_tags(
                                    &type_states,
                                    ip,
                                    instruction_stack.len(),
                                    records,
                                );
                                emit_native_overflow_side_exit(
                                    &mut builder,
                                    overflow,
                                    continuation_codegen,
                                    ip,
                                    block_instruction_offsets[ip],
                                    &local_tags,
                                    &instruction_stack,
                                    &stack_tags,
                                );
                                stack.push(builder.ins().sdiv(left, right));
                            } else {
                                stack.push(builder.ins().sdiv(left, right));
                            }
                        }
                    }
                    Op::FloatSqrt => {
                        let value = stack.pop().ok_or_else(|| "stack underflow".to_owned())?;
                        let value = builder
                            .ins()
                            .bitcast(types::F64, MemFlagsData::new(), value);
                        let zero = builder.ins().f64const(0.0);
                        let is_negative = builder.ins().fcmp(FloatCC::LessThan, value, zero);
                        let continue_sqrt = builder.create_block();
                        let exit_args = step_exit_args(
                            &mut builder,
                            steps_var,
                            ip,
                            block_instruction_offsets[ip] + 1,
                        );
                        builder.ins().brif(
                            is_negative,
                            math_domain,
                            &exit_args,
                            continue_sqrt,
                            &[],
                        );
                        builder.switch_to_block(continue_sqrt);
                        builder.seal_block(continue_sqrt);
                        let result = builder.ins().sqrt(value);
                        emit_float_finite_guard(
                            &mut builder,
                            result,
                            float_nonfinite,
                            steps_var,
                            ip,
                            block_instruction_offsets[ip] + 1,
                        );
                        stack.push(
                            builder
                                .ins()
                                .bitcast(types::I64, MemFlagsData::new(), result),
                        );
                    }
                    Op::IntegerDivide | Op::FloorDivide | Op::Remainder => {
                        let right = stack.pop().ok_or_else(|| "stack underflow".to_owned())?;
                        let left = stack.pop().ok_or_else(|| "stack underflow".to_owned())?;
                        let constant_power_of_two_divisor = if ip > 0
                            && leader_indices[ip].is_none()
                        {
                            let decode_divisor = |value: i64, negated: bool| {
                                let negative = value < 0 || negated;
                                let magnitude = value.unsigned_abs();
                                (value != 0
                                    && !(negative && magnitude == 1)
                                    && magnitude.is_power_of_two()
                                    && magnitude <= (1_u64 << 62))
                                    .then_some((magnitude as i64, negative))
                            };
                            function
                                .code
                                .get(ip - 1)
                                .and_then(|previous| match &previous.op {
                                    Op::Const(Value::Int(value)) => value
                                        .to_i64()
                                        .and_then(|value| decode_divisor(value, false)),
                                    Op::Negate if ip >= 2 && leader_indices[ip - 1].is_none() => {
                                        function.code.get(ip - 2).and_then(|literal| {
                                            if let Op::Const(Value::Int(value)) = &literal.op {
                                                value.to_i64().and_then(|value| {
                                                    (value > 0)
                                                        .then(|| decode_divisor(value, true))
                                                        .flatten()
                                                })
                                            } else {
                                                None
                                            }
                                        })
                                    }
                                    _ => None,
                                })
                        } else {
                            None
                        };
                        if let Some((divisor, negative_divisor)) = constant_power_of_two_divisor
                            .filter(|_| {
                                matches!(
                                    instruction.op,
                                    Op::IntegerDivide | Op::FloorDivide | Op::Remainder
                                )
                            })
                        {
                            if negative_divisor {
                                let shift = (divisor as u64).trailing_zeros() as i64;
                                let zero = builder.ins().iconst(types::I64, 0);
                                if matches!(instruction.op, Op::FloorDivide) {
                                    let floor_quotient = builder.ins().sshr_imm_s(left, shift);
                                    let mask = builder.ins().iconst(types::I64, divisor - 1);
                                    let remainder = builder.ins().band(left, mask);
                                    let remainder_nonzero =
                                        builder.ins().icmp(IntCC::NotEqual, remainder, zero);
                                    let one = builder.ins().iconst(types::I64, 1);
                                    let adjustment =
                                        builder.ins().select(remainder_nonzero, one, zero);
                                    let ceiling = builder.ins().iadd(floor_quotient, adjustment);
                                    stack.push(builder.ins().isub(zero, ceiling));
                                } else if matches!(instruction.op, Op::IntegerDivide) {
                                    // Compute truncation for the positive magnitude, then flip the sign.
                                    let negative =
                                        builder.ins().icmp(IntCC::SignedLessThan, left, zero);
                                    let bias = builder.ins().iconst(types::I64, divisor - 1);
                                    let bias = builder.ins().select(negative, bias, zero);
                                    let adjusted = builder.ins().iadd(left, bias);
                                    let quotient = builder.ins().sshr_imm_s(adjusted, shift);
                                    stack.push(builder.ins().isub(zero, quotient));
                                } else {
                                    let mask = builder.ins().iconst(types::I64, divisor - 1);
                                    let remainder = builder.ins().band(left, mask);
                                    let remainder_nonzero =
                                        builder.ins().icmp(IntCC::NotEqual, remainder, zero);
                                    let divisor_value = builder.ins().iconst(types::I64, divisor);
                                    let adjusted = builder.ins().isub(remainder, divisor_value);
                                    stack.push(builder.ins().select(
                                        remainder_nonzero,
                                        adjusted,
                                        zero,
                                    ));
                                }
                            } else {
                                if matches!(instruction.op, Op::FloorDivide) {
                                    let shift = (divisor as u64).trailing_zeros() as i64;
                                    if shift == 0 {
                                        stack.push(left);
                                    } else {
                                        stack.push(builder.ins().sshr_imm_s(left, shift));
                                    }
                                } else if matches!(instruction.op, Op::IntegerDivide) {
                                    let shift = (divisor as u64).trailing_zeros() as i64;
                                    if shift == 0 {
                                        stack.push(left);
                                    } else {
                                        // Bias negative values so this keeps `/`'s truncation toward zero.
                                        let zero = builder.ins().iconst(types::I64, 0);
                                        let negative =
                                            builder.ins().icmp(IntCC::SignedLessThan, left, zero);
                                        let bias = builder.ins().iconst(types::I64, divisor - 1);
                                        let bias = builder.ins().select(negative, bias, zero);
                                        let adjusted = builder.ins().iadd(left, bias);
                                        stack.push(builder.ins().sshr_imm_s(adjusted, shift));
                                    }
                                } else if divisor == 1 {
                                    stack.push(builder.ins().iconst(types::I64, 0));
                                } else {
                                    let mask = builder.ins().iconst(types::I64, divisor - 1);
                                    stack.push(builder.ins().band(left, mask));
                                }
                            }
                        } else {
                            let zero = builder.ins().iconst(types::I64, 0);
                            let is_zero = builder.ins().icmp(IntCC::Equal, right, zero);
                            let nonzero = builder.create_block();
                            let exit_args = step_exit_args(
                                &mut builder,
                                steps_var,
                                ip,
                                block_instruction_offsets[ip] + 1,
                            );
                            builder
                                .ins()
                                .brif(is_zero, divide_zero, &exit_args, nonzero, &[]);
                            builder.switch_to_block(nonzero);
                            builder.seal_block(nonzero);
                            let one = builder.ins().iconst(types::I64, 1);
                            let may_side_exit = native_integer_overflow_operation_at(
                                &instruction.op,
                                stack_types[ip].as_deref().unwrap_or(&[]),
                                function,
                                ip,
                                &leader_indices,
                            );
                            let known_divisor =
                                native_constant_integer_operand(function, ip, &leader_indices);
                            let needs_overflow_safety = if matches!(instruction.op, Op::Remainder) {
                                known_divisor.is_none_or(|divisor| divisor == -1)
                            } else {
                                may_side_exit
                            };
                            let (quotient, truncating) = if needs_overflow_safety {
                                let min = builder.ins().iconst(types::I64, i64::MIN);
                                let minus_one = builder.ins().iconst(types::I64, -1);
                                let is_min = builder.ins().icmp(IntCC::Equal, left, min);
                                let is_minus_one =
                                    builder.ins().icmp(IntCC::Equal, right, minus_one);
                                let overflow = builder.ins().band(is_min, is_minus_one);
                                if may_side_exit {
                                    let local_tags = continuation_local_tags(
                                        function,
                                        &type_states,
                                        ip,
                                        records,
                                    );
                                    let stack_tags = continuation_stack_tags(
                                        &type_states,
                                        ip,
                                        instruction_stack.len(),
                                        records,
                                    );
                                    emit_native_overflow_side_exit(
                                        &mut builder,
                                        overflow,
                                        continuation_codegen,
                                        ip,
                                        block_instruction_offsets[ip],
                                        &local_tags,
                                        &instruction_stack,
                                        &stack_tags,
                                    );
                                }
                                let safe_right = builder.ins().select(overflow, one, right);
                                let quotient = builder.ins().sdiv(left, safe_right);
                                let truncating = builder.ins().srem(left, safe_right);
                                let truncating = builder.ins().select(overflow, zero, truncating);
                                (quotient, truncating)
                            } else {
                                (
                                    builder.ins().sdiv(left, right),
                                    builder.ins().srem(left, right),
                                )
                            };
                            let left_negative =
                                builder.ins().icmp(IntCC::SignedLessThan, left, zero);
                            let right_negative =
                                builder.ins().icmp(IntCC::SignedLessThan, right, zero);
                            let signs_differ =
                                builder
                                    .ins()
                                    .icmp(IntCC::NotEqual, left_negative, right_negative);
                            let remainder_nonzero =
                                builder.ins().icmp(IntCC::NotEqual, truncating, zero);
                            let adjust = builder.ins().band(signs_differ, remainder_nonzero);
                            if matches!(instruction.op, Op::Remainder) {
                                let adjusted = builder.ins().iadd(truncating, right);
                                stack.push(builder.ins().select(adjust, adjusted, truncating));
                            } else if matches!(instruction.op, Op::FloorDivide) {
                                let adjusted = builder.ins().isub(quotient, one);
                                stack.push(builder.ins().select(adjust, adjusted, quotient));
                            } else {
                                stack.push(quotient);
                            }
                        }
                    }
                    Op::Print => {
                        let value = stack.pop().ok_or_else(|| "stack underflow".to_owned())?;
                        let value_type = stack_types[ip]
                            .as_ref()
                            .and_then(|types| types.last())
                            .ok_or_else(|| "missing printed value type".to_owned())?;
                        let kind = builder.ins().iconst(
                            types::I64,
                            native_print_kind(value_type)
                                .ok_or_else(|| "unsupported native print value".to_owned())?,
                        );
                        let printed = builder.ins().call(print, &[arena_pointer, value, kind]);
                        let printed = builder.inst_results(printed)[0];
                        let zero = builder.ins().iconst(types::I32, 0);
                        let succeeded = builder.ins().icmp(IntCC::NotEqual, printed, zero);
                        let success = builder.create_block();
                        let exit_args = step_exit_args(
                            &mut builder,
                            steps_var,
                            ip,
                            block_instruction_offsets[ip] + 1,
                        );
                        builder
                            .ins()
                            .brif(succeeded, success, &[], fallback, &exit_args);
                        builder.switch_to_block(success);
                        builder.seal_block(success);
                    }
                    Op::IsOk | Op::IsSome => {
                        let handle = stack.pop().ok_or_else(|| "stack underflow".to_owned())?;
                        let pointer = native_tagged_value_pointer(&mut builder, ptr, handle);
                        let kind_offset =
                            i32::try_from(std::mem::offset_of!(NativeTaggedValue, kind))
                                .map_err(|_| "tagged value layout is invalid".to_owned())?;
                        let kind = builder.ins().load(
                            types::I64,
                            MemFlagsData::new(),
                            pointer,
                            kind_offset,
                        );
                        let expected_kind = if matches!(&instruction.op, Op::IsOk) {
                            NATIVE_TAGGED_RESULT_OK
                        } else {
                            NATIVE_TAGGED_OPTION_SOME
                        };
                        let expected = builder.ins().iconst(types::I64, expected_kind);
                        let matched = builder.ins().icmp(IntCC::Equal, kind, expected);
                        stack.push(builder.ins().uextend(types::I64, matched));
                    }
                    Op::Unwrap | Op::GetError => {
                        let input_type = stack_types[ip]
                            .as_ref()
                            .and_then(|types| types.last())
                            .ok_or_else(|| "missing tagged input type".to_owned())?;
                        let output_type = stack_types
                            .get(ip + 1)
                            .and_then(Option::as_ref)
                            .and_then(|types| types.last())
                            .ok_or_else(|| "missing tagged output type".to_owned())?;
                        let expected_kind = match (&instruction.op, input_type) {
                            (Op::Unwrap, Type::Result(_, _)) => NATIVE_TAGGED_RESULT_OK,
                            (Op::Unwrap, Type::Option(_)) => NATIVE_TAGGED_OPTION_SOME,
                            (Op::GetError, Type::Result(_, _)) => NATIVE_TAGGED_RESULT_ERR,
                            _ => return Err("invalid native tagged extraction".into()),
                        };
                        let handle = stack.pop().ok_or_else(|| "stack underflow".to_owned())?;
                        let local_tags =
                            continuation_local_tags(function, &type_states, ip, records);
                        let stack_tags = continuation_stack_tags(
                            &type_states,
                            ip,
                            instruction_stack.len(),
                            records,
                        );
                        let value = emit_native_tagged_value_extract(
                            &mut builder,
                            ptr,
                            handle,
                            expected_kind,
                            native_bridge_tag(output_type),
                            continuation_codegen,
                            ip,
                            block_instruction_offsets[ip],
                            &local_tags,
                            &instruction_stack,
                            &stack_tags,
                        )?;
                        stack.push(value);
                    }
                    Op::MakeOk | Op::MakeErr | Op::MakeSome | Op::MakeNone => {
                        let (kind, payload_type, payload) = match &instruction.op {
                            Op::MakeOk => (
                                NATIVE_TAGGED_RESULT_OK,
                                stack_types[ip]
                                    .as_ref()
                                    .and_then(|types| types.last())
                                    .cloned(),
                                stack.pop(),
                            ),
                            Op::MakeErr => (
                                NATIVE_TAGGED_RESULT_ERR,
                                stack_types[ip]
                                    .as_ref()
                                    .and_then(|types| types.last())
                                    .cloned(),
                                stack.pop(),
                            ),
                            Op::MakeSome => (
                                NATIVE_TAGGED_OPTION_SOME,
                                stack_types[ip]
                                    .as_ref()
                                    .and_then(|types| types.last())
                                    .cloned(),
                                stack.pop(),
                            ),
                            Op::MakeNone => (NATIVE_TAGGED_OPTION_NONE, None, None),
                            _ => unreachable!(),
                        };
                        let kind = builder.ins().iconst(types::I64, kind);
                        let payload_tag = builder.ins().iconst(
                            types::I64,
                            payload_type.as_ref().map_or(0, native_bridge_tag),
                        );
                        let payload =
                            payload.unwrap_or_else(|| builder.ins().iconst(types::I64, 0));
                        let tagged_value = builder.ins().call(
                            make_tagged_value,
                            &[arena_pointer, kind, payload_tag, payload],
                        );
                        let tagged_value = builder.inst_results(tagged_value)[0];
                        let zero = builder.ins().iconst(types::I64, 0);
                        let succeeded = builder.ins().icmp(IntCC::NotEqual, tagged_value, zero);
                        let success = builder.create_block();
                        let failure = builder.create_block();
                        builder.ins().brif(succeeded, success, &[], failure, &[]);
                        builder.switch_to_block(failure);
                        let local_tags =
                            continuation_local_tags(function, &type_states, ip, records);
                        let stack_tags = continuation_stack_tags(
                            &type_states,
                            ip,
                            instruction_stack.len(),
                            records,
                        );
                        emit_native_capture_exit(
                            &mut builder,
                            continuation_codegen,
                            ip,
                            block_instruction_offsets[ip],
                            &local_tags,
                            &instruction_stack,
                            &stack_tags,
                        );
                        builder.seal_block(failure);
                        builder.switch_to_block(success);
                        stack.push(tagged_value);
                        builder.seal_block(success);
                    }
                    Op::JsonTryText
                    | Op::JsonTryGet
                    | Op::JsonTryInt
                    | Op::JsonParse
                    | Op::FileNextLine
                    | Op::Args
                    | Op::MakeTypedMap(_)
                    | Op::JsonArray
                    | Op::MapKeys
                    | Op::JsonIndex
                    | Op::TextChars
                    | Op::TextSlice
                    | Op::JsonGet
                    | Op::JsonStringify
                    | Op::JsonObject
                    | Op::JsonInt
                    | Op::MapHas
                    | Op::JsonText
                    | Op::FileOpenLines
                    | Op::FileClose
                    | Op::TextCollapseWhitespace
                    | Op::TimeParseTimestamp
                    | Op::TimeFormatUtc => {
                        let argument_count = native_direct_argument_count(&instruction.op)
                            .ok_or_else(|| "missing direct native argument count".to_owned())?;
                        let input_types = stack_types[ip]
                            .as_ref()
                            .ok_or_else(|| "missing native bridge input types".to_owned())?;
                        let argument_start = input_types
                            .len()
                            .checked_sub(argument_count)
                            .ok_or_else(|| "native bridge stack underflow".to_owned())?;
                        let arguments = stack.split_off(argument_start);
                        let result_type = stack_types
                            .get(ip + 1)
                            .and_then(Option::as_ref)
                            .and_then(|types| types.last())
                            .ok_or_else(|| "missing direct native result type".to_owned())?;
                        let operation_index = native_direct_operation_index(&instruction.op)
                            .ok_or_else(|| "missing direct native operation".to_owned())?;
                        let local_tags =
                            continuation_local_tags(function, &type_states, ip, records);
                        let stack_tags = continuation_stack_tags(
                            &type_states,
                            ip,
                            instruction_stack.len(),
                            records,
                        );
                        let result = emit_native_direct_operation_call(
                            &mut builder,
                            ptr,
                            direct_operations[operation_index],
                            continuation_codegen,
                            ip,
                            block_instruction_offsets[ip],
                            &arguments,
                            &input_types[argument_start..],
                            result_type,
                            &local_tags,
                            &instruction_stack,
                            &stack_tags,
                        )?;
                        stack.push(result);
                    }
                    Op::PrintErr => {
                        let value = stack.pop().ok_or_else(|| "stack underflow".to_owned())?;
                        let input_type = stack_types[ip]
                            .as_ref()
                            .and_then(|types| types.last())
                            .ok_or_else(|| "missing stderr value type".to_owned())?;
                        if input_type != &Type::Text {
                            return Err("native stderr currently requires Text".into());
                        }
                        let operation_index = native_direct_operation_index(&instruction.op)
                            .ok_or_else(|| "missing direct native stderr operation".to_owned())?;
                        let local_tags =
                            continuation_local_tags(function, &type_states, ip, records);
                        let stack_tags = continuation_stack_tags(
                            &type_states,
                            ip,
                            instruction_stack.len(),
                            records,
                        );
                        let _ = emit_native_direct_operation_call(
                            &mut builder,
                            ptr,
                            direct_operations[operation_index],
                            continuation_codegen,
                            ip,
                            block_instruction_offsets[ip],
                            &[value],
                            std::slice::from_ref(input_type),
                            &Type::Unit,
                            &local_tags,
                            &instruction_stack,
                            &stack_tags,
                        )?;
                    }
                    Op::Return => {
                        let completed_instructions = block_instruction_counts[block_for_ip[ip]];
                        let steps =
                            step_count_after(&mut builder, steps_var, completed_instructions);
                        let remaining_fuel =
                            fuel_count_after(&mut builder, fuel_var, completed_instructions);
                        builder.def_var(fuel_var, remaining_fuel);
                        let value = stack.pop().ok_or_else(|| "stack underflow".to_owned())?;
                        let native_record_result = match &function.result {
                            Type::Record(name) => records
                                .iter()
                                .find(|record| record.name == *name)
                                .filter(|record| native_record_has_native_layout(record)),
                            _ => None,
                        };
                        if let Some(definition) = native_record_result {
                            let fields = builder.ins().load(
                                ptr,
                                MemFlagsData::new(),
                                value,
                                std::mem::offset_of!(NativeRecord, fields) as i32,
                            );
                            for (field_index, _) in definition.fields.iter().enumerate() {
                                let address =
                                    builder.ins().iadd_imm_s(fields, (field_index * 8) as i64);
                                let field =
                                    builder
                                        .ins()
                                        .load(types::I64, MemFlagsData::new(), address, 0);
                                store_output(
                                    &mut builder,
                                    result_pointer,
                                    field_index as i32,
                                    field,
                                );
                            }
                        } else {
                            store_output(&mut builder, result_pointer, 0, value);
                        }
                        let unused_fuel = builder.use_var(fuel_var);
                        let calls = builder.use_var(native_calls_var);
                        emit_success(&mut builder, output_pointer, steps, unused_fuel, calls);
                        continue;
                    }
                    _ => return Err("unsupported instruction".into()),
                }
                if ip + 1 >= function.code.len() {
                    return Err("missing function terminator".into());
                }
                if let Some(next_block) = leader_indices[ip + 1] {
                    if !matches!(instruction.op, Op::Call(_, _)) {
                        let completed_instructions = block_instruction_counts[block_for_ip[ip]];
                        let block_steps =
                            step_count_after(&mut builder, steps_var, completed_instructions);
                        builder.def_var(steps_var, block_steps);
                        let remaining_fuel =
                            fuel_count_after(&mut builder, fuel_var, completed_instructions);
                        builder.def_var(fuel_var, remaining_fuel);
                    }
                    let block_args: Vec<_> = stack.iter().copied().map(BlockArg::Value).collect();
                    builder.ins().jump(blocks[next_block], &block_args);
                }
            }

            if let Some((block, [fault_ip, completed, first, second])) = int2_continuation_state {
                builder.switch_to_block(block);
                let int2_function_index = builder
                    .ins()
                    .iconst(types::I64, index.min(i64::MAX as usize) as i64);
                let no_caller = builder.ins().iconst(types::I64, -1);
                let capture = builder.ins().call(
                    capture_vm_continuation_int2,
                    &[
                        arena_pointer,
                        int2_function_index,
                        fault_ip,
                        depth_value,
                        first,
                        second,
                        no_caller,
                    ],
                );
                let captured = builder.inst_results(capture)[0];
                let zero_status = builder.ins().iconst(types::I32, 0);
                let capture_succeeded = builder.ins().icmp(IntCC::NotEqual, captured, zero_status);
                let steps = builder.use_var(steps_var);
                let retried_instruction_count = builder.ins().iadd_imm_s(completed, 1);
                builder.ins().brif(
                    capture_succeeded,
                    continuation_exit,
                    &[BlockArg::Value(fault_ip), BlockArg::Value(completed)],
                    fallback,
                    &[
                        BlockArg::Value(steps),
                        BlockArg::Value(fault_ip),
                        BlockArg::Value(retried_instruction_count),
                    ],
                );
            }

            if let Some((block, [fault_ip, completed])) = continuation_exit_state {
                builder.switch_to_block(block);
                let reserved_fuel = builder.use_var(fuel_var);
                let unused_fuel = builder.ins().isub(reserved_fuel, completed);
                store_output(&mut builder, output_pointer, 5, unused_fuel);
                let completed_steps = builder.use_var(steps_var);
                let steps = builder.ins().iadd(completed_steps, completed);
                let calls = builder.use_var(native_calls_var);
                emit_exit(
                    &mut builder,
                    output_pointer,
                    1,
                    steps,
                    fault_ip,
                    index,
                    calls,
                );
            }

            for ((block, status), params) in [
                (fallback, 1),
                (divide_zero, 2),
                (float_nonfinite, 4),
                (math_domain, 7),
                (cancelled, 6),
            ]
            .into_iter()
            .zip(exit_params)
            {
                builder.switch_to_block(block);
                let reserved_fuel = builder.use_var(fuel_var);
                let completed_instructions = params[2];
                let unused_fuel = builder.ins().isub(reserved_fuel, completed_instructions);
                store_output(&mut builder, output_pointer, 5, unused_fuel);
                let calls = builder.use_var(native_calls_var);
                let steps = builder.ins().iadd(params[0], params[2]);
                emit_exit(
                    &mut builder,
                    output_pointer,
                    status,
                    steps,
                    params[1],
                    index,
                    calls,
                );
            }
            builder.seal_all_blocks();
            builder.finalize(target_config);
        }
        let ir_blocks = context.func.layout.blocks().count();
        let ir_instructions = context.func.dfg.num_insts();
        let lowering_nanoseconds = lowering_start.elapsed().as_nanos().min(u64::MAX as u128) as u64;
        let verification_start = Instant::now();
        if let Err(errors) =
            cranelift_codegen::verifier::verify_function(&context.func, self.module.isa())
        {
            return Err(format!(
                "native IR verification failed:\n{errors}\n{}",
                context.func
            ));
        }
        let verification_nanoseconds = verification_start
            .elapsed()
            .as_nanos()
            .min(u64::MAX as u128) as u64;
        let define_start = Instant::now();
        self.module
            .define_function(function_id, &mut context)
            .map_err(|error| error.to_string())?;
        let define_nanoseconds = define_start.elapsed().as_nanos().min(u64::MAX as u128) as u64;
        self.module.clear_context(&mut context);
        Ok(NativeFunctionCompileProfile {
            name: function.name.clone(),
            bytecode_instructions: function.code.len(),
            bytecode_blocks: leader_indices
                .iter()
                .filter(|leader| leader.is_some())
                .count(),
            cranelift_ir_blocks: ir_blocks,
            cranelift_ir_instructions: ir_instructions,
            lowering_nanoseconds,
            verification_nanoseconds,
            define_nanoseconds,
        })
    }
}

// The operands are separate Cranelift values and a continuation snapshot; keeping them
// explicit makes the generated call ABI and side-exit state auditable at the call site.
#[allow(clippy::too_many_arguments)]
fn emit_native_direct_operation_call(
    builder: &mut FunctionBuilder<'_>,
    pointer_type: cranelift_codegen::ir::Type,
    direct_operation: FuncRef,
    codegen: NativeContinuationCodegen<'_>,
    ip: usize,
    completed_in_block: usize,
    arguments: &[cranelift_codegen::ir::Value],
    argument_types: &[Type],
    result_type: &Type,
    local_tags: &[i64],
    stack_values: &[cranelift_codegen::ir::Value],
    stack_tags: &[i64],
) -> Result<cranelift_codegen::ir::Value, String> {
    emit_native_direct_operation_call_with_argument_tag_overrides(
        builder,
        pointer_type,
        direct_operation,
        codegen,
        ip,
        completed_in_block,
        arguments,
        argument_types,
        &[],
        result_type,
        local_tags,
        stack_values,
        stack_tags,
    )
}

#[allow(clippy::too_many_arguments)]
fn emit_native_direct_operation_call_with_argument_tag_overrides(
    builder: &mut FunctionBuilder<'_>,
    pointer_type: cranelift_codegen::ir::Type,
    direct_operation: FuncRef,
    codegen: NativeContinuationCodegen<'_>,
    ip: usize,
    completed_in_block: usize,
    arguments: &[cranelift_codegen::ir::Value],
    argument_types: &[Type],
    argument_tag_overrides: &[Option<i64>],
    result_type: &Type,
    local_tags: &[i64],
    stack_values: &[cranelift_codegen::ir::Value],
    stack_tags: &[i64],
) -> Result<cranelift_codegen::ir::Value, String> {
    if arguments.len() != argument_types.len() || arguments.len() > 5 {
        return Err("direct native operation has an invalid argument layout".into());
    }
    let zero = builder.ins().iconst(types::I64, 0);
    let mut operation_arguments = Vec::with_capacity(10);
    for index in 0..5 {
        operation_arguments.push(arguments.get(index).copied().unwrap_or(zero));
        let tag = argument_tag_overrides
            .get(index)
            .copied()
            .flatten()
            .or_else(|| argument_types.get(index).map(native_bridge_tag))
            .unwrap_or(0);
        operation_arguments.push(builder.ins().iconst(types::I64, tag));
    }
    let function_index = builder.ins().iconst(
        types::I64,
        codegen.function_index.min(i64::MAX as usize) as i64,
    );
    let instruction_index = builder
        .ins()
        .iconst(types::I64, ip.min(i64::MAX as usize) as i64);
    let result_tag = builder
        .ins()
        .iconst(types::I64, native_bridge_tag(result_type));
    let result_slot =
        builder.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 8, 3));
    let result_pointer = builder.ins().stack_addr(pointer_type, result_slot, 0);
    let mut call_arguments = Vec::with_capacity(17);
    call_arguments.extend([
        codegen.arena,
        function_index,
        instruction_index,
        codegen.depth,
    ]);
    call_arguments.extend(operation_arguments);
    call_arguments.extend([result_tag, result_pointer]);
    let call = builder.ins().call(direct_operation, &call_arguments);
    let status = builder.inst_results(call)[0];
    let zero_status = builder.ins().iconst(types::I32, 0);
    let succeeded = builder.ins().icmp(IntCC::NotEqual, status, zero_status);
    let success = builder.create_block();
    let failure = builder.create_block();
    builder.ins().brif(succeeded, success, &[], failure, &[]);
    builder.switch_to_block(failure);
    emit_native_capture_exit(
        builder,
        codegen,
        ip,
        completed_in_block,
        local_tags,
        stack_values,
        stack_tags,
    );
    builder.seal_block(failure);
    builder.switch_to_block(success);
    let result = builder
        .ins()
        .load(types::I64, MemFlagsData::new(), result_pointer, 0);
    builder.seal_block(success);
    Ok(result)
}

fn native_tagged_value_pointer(
    builder: &mut FunctionBuilder<'_>,
    pointer_type: cranelift_codegen::ir::Type,
    handle: cranelift_codegen::ir::Value,
) -> cranelift_codegen::ir::Value {
    let address = builder.ins().ineg(handle);
    if pointer_type == types::I64 {
        address
    } else {
        builder.ins().ireduce(pointer_type, address)
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_native_tagged_value_extract(
    builder: &mut FunctionBuilder<'_>,
    pointer_type: cranelift_codegen::ir::Type,
    handle: cranelift_codegen::ir::Value,
    expected_kind: i64,
    expected_payload_tag: i64,
    codegen: NativeContinuationCodegen<'_>,
    ip: usize,
    completed_in_block: usize,
    local_tags: &[i64],
    stack_values: &[cranelift_codegen::ir::Value],
    stack_tags: &[i64],
) -> Result<cranelift_codegen::ir::Value, String> {
    let pointer = native_tagged_value_pointer(builder, pointer_type, handle);
    let kind_offset = i32::try_from(std::mem::offset_of!(NativeTaggedValue, kind))
        .map_err(|_| "tagged value layout is invalid".to_owned())?;
    let payload_tag_offset = i32::try_from(std::mem::offset_of!(NativeTaggedValue, payload_tag))
        .map_err(|_| "tagged value layout is invalid".to_owned())?;
    let payload_offset = i32::try_from(std::mem::offset_of!(NativeTaggedValue, payload))
        .map_err(|_| "tagged value layout is invalid".to_owned())?;
    let kind = builder
        .ins()
        .load(types::I64, MemFlagsData::new(), pointer, kind_offset);
    let expected_kind = builder.ins().iconst(types::I64, expected_kind);
    let kind_matches = builder.ins().icmp(IntCC::Equal, kind, expected_kind);
    let payload_tag =
        builder
            .ins()
            .load(types::I64, MemFlagsData::new(), pointer, payload_tag_offset);
    let expected_payload_tag = builder.ins().iconst(types::I64, expected_payload_tag);
    let payload_matches = builder
        .ins()
        .icmp(IntCC::Equal, payload_tag, expected_payload_tag);
    let can_extract = builder.ins().band(kind_matches, payload_matches);
    let success = builder.create_block();
    let side_exit = builder.create_block();
    builder
        .ins()
        .brif(can_extract, success, &[], side_exit, &[]);
    builder.switch_to_block(side_exit);
    emit_native_capture_exit(
        builder,
        codegen,
        ip,
        completed_in_block,
        local_tags,
        stack_values,
        stack_tags,
    );
    builder.seal_block(side_exit);
    builder.switch_to_block(success);
    let value = builder
        .ins()
        .load(types::I64, MemFlagsData::new(), pointer, payload_offset);
    builder.seal_block(success);
    Ok(value)
}

// The operands are separate Cranelift values and a continuation snapshot; keeping them
// explicit makes the generated call ABI and side-exit state auditable at the call site.
#[allow(clippy::too_many_arguments)]
fn emit_native_make_record_call(
    builder: &mut FunctionBuilder<'_>,
    pointer_type: cranelift_codegen::ir::Type,
    make_record: FuncRef,
    codegen: NativeContinuationCodegen<'_>,
    ip: usize,
    completed_in_block: usize,
    arguments: &[cranelift_codegen::ir::Value],
    argument_types: &[Type],
    result_type: &Type,
    local_tags: &[i64],
    stack_values: &[cranelift_codegen::ir::Value],
    stack_tags: &[i64],
) -> Result<cranelift_codegen::ir::Value, String> {
    if arguments.len() != argument_types.len() {
        return Err("native record arguments do not match their inferred types".into());
    }
    let array_bytes = u32::try_from(arguments.len().max(1).saturating_mul(8))
        .map_err(|_| "native record field frame is too large".to_owned())?;
    let values_slot = builder.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        array_bytes,
        3,
    ));
    let tags_slot = builder.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        array_bytes,
        3,
    ));
    for (index, (value, ty)) in arguments.iter().zip(argument_types).enumerate() {
        let offset = i32::try_from(index.saturating_mul(8))
            .map_err(|_| "native record field offset is too large".to_owned())?;
        let value_address = builder.ins().stack_addr(pointer_type, values_slot, offset);
        builder
            .ins()
            .store(MemFlagsData::new(), *value, value_address, 0);
        let tag_address = builder.ins().stack_addr(pointer_type, tags_slot, offset);
        let tag = builder.ins().iconst(types::I64, native_bridge_tag(ty));
        builder
            .ins()
            .store(MemFlagsData::new(), tag, tag_address, 0);
    }
    let values_pointer = builder.ins().stack_addr(pointer_type, values_slot, 0);
    let tags_pointer = builder.ins().stack_addr(pointer_type, tags_slot, 0);
    let function_index = builder.ins().iconst(
        types::I64,
        codegen.function_index.min(i64::MAX as usize) as i64,
    );
    let instruction_index = builder
        .ins()
        .iconst(types::I64, ip.min(i64::MAX as usize) as i64);
    let count = builder
        .ins()
        .iconst(types::I64, arguments.len().min(i64::MAX as usize) as i64);
    let result_tag = builder
        .ins()
        .iconst(types::I64, native_bridge_tag(result_type));
    let result_slot =
        builder.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 8, 3));
    let result_pointer = builder.ins().stack_addr(pointer_type, result_slot, 0);
    let call = builder.ins().call(
        make_record,
        &[
            codegen.arena,
            function_index,
            instruction_index,
            codegen.depth,
            values_pointer,
            tags_pointer,
            count,
            result_tag,
            result_pointer,
        ],
    );
    let status = builder.inst_results(call)[0];
    let zero_status = builder.ins().iconst(types::I32, 0);
    let succeeded = builder.ins().icmp(IntCC::NotEqual, status, zero_status);
    let success = builder.create_block();
    let failure = builder.create_block();
    builder.ins().brif(succeeded, success, &[], failure, &[]);
    builder.switch_to_block(failure);
    emit_native_capture_exit(
        builder,
        codegen,
        ip,
        completed_in_block,
        local_tags,
        stack_values,
        stack_tags,
    );
    builder.seal_block(failure);
    builder.switch_to_block(success);
    let result = builder
        .ins()
        .load(types::I64, MemFlagsData::new(), result_pointer, 0);
    builder.seal_block(success);
    Ok(result)
}

fn step_count_after(
    builder: &mut FunctionBuilder<'_>,
    steps_var: Variable,
    completed_instructions: usize,
) -> cranelift_codegen::ir::Value {
    let steps = builder.use_var(steps_var);
    builder
        .ins()
        .iadd_imm_s(steps, completed_instructions.min(i64::MAX as usize) as i64)
}

fn fuel_count_after(
    builder: &mut FunctionBuilder<'_>,
    fuel_var: Variable,
    completed_instructions: usize,
) -> cranelift_codegen::ir::Value {
    let remaining_fuel = builder.use_var(fuel_var);
    builder.ins().iadd_imm_s(
        remaining_fuel,
        -(completed_instructions.min(i64::MAX as usize) as i64),
    )
}

fn step_exit_args(
    builder: &mut FunctionBuilder<'_>,
    steps_var: Variable,
    ip: usize,
    completed_instructions: usize,
) -> [BlockArg; 3] {
    let steps = builder.use_var(steps_var);
    let fault_ip = builder.ins().iconst(types::I64, ip as i64);
    let completed_instructions = builder.ins().iconst(
        types::I64,
        completed_instructions.min(i64::MAX as usize) as i64,
    );
    [
        BlockArg::Value(steps),
        BlockArg::Value(fault_ip),
        BlockArg::Value(completed_instructions),
    ]
}

fn emit_continuation_pairs(
    builder: &mut FunctionBuilder<'_>,
    pointer_type: cranelift_codegen::ir::Type,
    values: &[cranelift_codegen::ir::Value],
    tags: &[i64],
) -> cranelift_codegen::ir::Value {
    if values.is_empty() {
        return builder.ins().iconst(pointer_type, 0);
    }
    let slot_size = values.len().saturating_mul(16).min(u32::MAX as usize) as u32;
    let slot = builder.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        slot_size,
        3,
    ));
    let address = builder.ins().stack_addr(pointer_type, slot, 0);
    for (index, value) in values.iter().enumerate() {
        let tag = builder
            .ins()
            .iconst(types::I64, tags.get(index).copied().unwrap_or(-1));
        let offset = (index.saturating_mul(16)).min(i32::MAX as usize) as i32;
        builder
            .ins()
            .store(MemFlagsData::new(), tag, address, offset);
        builder.ins().store(
            MemFlagsData::new(),
            *value,
            address,
            offset.saturating_add(8),
        );
    }
    address
}

fn emit_local_continuation_triples(
    builder: &mut FunctionBuilder<'_>,
    pointer_type: cranelift_codegen::ir::Type,
    slots: &[usize],
    values: &[cranelift_codegen::ir::Value],
    tags: &[i64],
) -> cranelift_codegen::ir::Value {
    if slots.is_empty() {
        return builder.ins().iconst(pointer_type, 0);
    }
    let slot_size = slots.len().saturating_mul(24).min(u32::MAX as usize) as u32;
    let slot = builder.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        slot_size,
        3,
    ));
    let address = builder.ins().stack_addr(pointer_type, slot, 0);
    for (index, (local, value)) in slots.iter().zip(values).enumerate() {
        let offset = index.saturating_mul(24).min(i32::MAX as usize) as i32;
        let local_slot = *local;
        let local_value = builder
            .ins()
            .iconst(types::I64, local_slot.min(i64::MAX as usize) as i64);
        let tag = builder
            .ins()
            .iconst(types::I64, tags.get(local_slot).copied().unwrap_or(-1));
        builder
            .ins()
            .store(MemFlagsData::new(), local_value, address, offset);
        builder
            .ins()
            .store(MemFlagsData::new(), tag, address, offset.saturating_add(8));
        builder.ins().store(
            MemFlagsData::new(),
            *value,
            address,
            offset.saturating_add(16),
        );
    }
    address
}

// This helper lowers three distinct callback ABIs (fixed int pairs, fixed values, and
// variable snapshots) from one compiler-state snapshot.
#[allow(clippy::too_many_arguments)]
fn emit_native_continuation_capture(
    builder: &mut FunctionBuilder<'_>,
    pointer_type: cranelift_codegen::ir::Type,
    capture: FuncRef,
    arena: cranelift_codegen::ir::Value,
    function_index: usize,
    resume_ip: usize,
    depth: cranelift_codegen::ir::Value,
    local_vars: &[Variable],
    live_locals_at_ip: &[Vec<usize>],
    capture_int2: FuncRef,
    capture_fixed: FuncRef,
    local_tags: &[i64],
    stack_values: &[cranelift_codegen::ir::Value],
    stack_tags: &[i64],
    caller_ip: i64,
) -> cranelift_codegen::ir::Value {
    let local_slots = live_locals_at_ip
        .get(resume_ip)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if local_slots.is_empty()
        && stack_values.len() == 2
        && stack_tags.first() == Some(&NATIVE_BRIDGE_TAG_INT)
        && stack_tags.get(1) == Some(&NATIVE_BRIDGE_TAG_INT)
    {
        let function_index = builder
            .ins()
            .iconst(types::I64, function_index.min(i64::MAX as usize) as i64);
        let resume_ip = builder
            .ins()
            .iconst(types::I64, resume_ip.min(i64::MAX as usize) as i64);
        let caller_ip = builder.ins().iconst(types::I64, caller_ip);
        let call = builder.ins().call(
            capture_int2,
            &[
                arena,
                function_index,
                resume_ip,
                depth,
                stack_values[0],
                stack_values[1],
                caller_ip,
            ],
        );
        return builder.inst_results(call)[0];
    }
    if local_slots.is_empty() && stack_values.len() <= 2 {
        let zero = builder.ins().iconst(types::I64, 0);
        let stack_count = builder
            .ins()
            .iconst(types::I64, stack_values.len().min(i64::MAX as usize) as i64);
        let first = stack_values.first().copied().unwrap_or(zero);
        let first_tag = builder
            .ins()
            .iconst(types::I64, stack_tags.first().copied().unwrap_or(-1));
        let second = stack_values.get(1).copied().unwrap_or(zero);
        let second_tag = builder
            .ins()
            .iconst(types::I64, stack_tags.get(1).copied().unwrap_or(-1));
        let function_index = builder
            .ins()
            .iconst(types::I64, function_index.min(i64::MAX as usize) as i64);
        let resume_ip = builder
            .ins()
            .iconst(types::I64, resume_ip.min(i64::MAX as usize) as i64);
        let caller_ip = builder.ins().iconst(types::I64, caller_ip);
        let call = builder.ins().call(
            capture_fixed,
            &[
                arena,
                function_index,
                resume_ip,
                depth,
                stack_count,
                first,
                first_tag,
                second,
                second_tag,
                caller_ip,
            ],
        );
        return builder.inst_results(call)[0];
    }
    let local_values: Vec<_> = local_slots
        .iter()
        .filter_map(|slot| {
            local_vars
                .get(*slot)
                .map(|variable| builder.use_var(*variable))
        })
        .collect();
    let locals_pointer = emit_local_continuation_triples(
        builder,
        pointer_type,
        local_slots,
        &local_values,
        local_tags,
    );
    let stack_pointer = emit_continuation_pairs(builder, pointer_type, stack_values, stack_tags);
    let function_index = builder
        .ins()
        .iconst(types::I64, function_index.min(i64::MAX as usize) as i64);
    let resume_ip = builder
        .ins()
        .iconst(types::I64, resume_ip.min(i64::MAX as usize) as i64);
    let local_count = builder
        .ins()
        .iconst(types::I64, local_values.len().min(i64::MAX as usize) as i64);
    let stack_count = builder
        .ins()
        .iconst(types::I64, stack_values.len().min(i64::MAX as usize) as i64);
    let caller_ip = builder.ins().iconst(types::I64, caller_ip);
    let call = builder.ins().call(
        capture,
        &[
            arena,
            function_index,
            resume_ip,
            depth,
            locals_pointer,
            local_count,
            stack_pointer,
            stack_count,
            caller_ip,
        ],
    );
    builder.inst_results(call)[0]
}

#[derive(Clone, Copy)]
struct NativeContinuationCodegen<'a> {
    pointer_type: cranelift_codegen::ir::Type,
    capture: FuncRef,
    capture_int2: FuncRef,
    capture_fixed: FuncRef,
    arena: cranelift_codegen::ir::Value,
    function_index: usize,
    depth: cranelift_codegen::ir::Value,
    local_vars: &'a [Variable],
    live_locals_at_ip: &'a [Vec<usize>],
    steps_var: Variable,
    fallback: cranelift_codegen::ir::Block,
    continuation_exit: cranelift_codegen::ir::Block,
    int2_continuation_exit: cranelift_codegen::ir::Block,
    use_int2_continuation_exit: bool,
}

// The values describe the exact VM state to capture if generated arithmetic overflows.
#[allow(clippy::too_many_arguments)]
fn emit_native_overflow_side_exit(
    builder: &mut FunctionBuilder<'_>,
    overflow: cranelift_codegen::ir::Value,
    codegen: NativeContinuationCodegen<'_>,
    ip: usize,
    completed_in_block: usize,
    local_tags: &[i64],
    stack_values: &[cranelift_codegen::ir::Value],
    stack_tags: &[i64],
) {
    let safe = builder.create_block();
    if codegen.live_locals_at_ip.get(ip).is_some_and(Vec::is_empty)
        && codegen.use_int2_continuation_exit
        && stack_values.len() == 2
        && stack_tags.first() == Some(&NATIVE_BRIDGE_TAG_INT)
        && stack_tags.get(1) == Some(&NATIVE_BRIDGE_TAG_INT)
    {
        let fault_ip = builder.ins().iconst(types::I64, ip as i64);
        let completed = builder
            .ins()
            .iconst(types::I64, completed_in_block.min(i64::MAX as usize) as i64);
        builder.ins().brif(
            overflow,
            codegen.int2_continuation_exit,
            &[
                BlockArg::Value(fault_ip),
                BlockArg::Value(completed),
                BlockArg::Value(stack_values[0]),
                BlockArg::Value(stack_values[1]),
            ],
            safe,
            &[],
        );
        builder.switch_to_block(safe);
        builder.seal_block(safe);
        return;
    }
    let overflow_exit = builder.create_block();
    builder.ins().brif(overflow, overflow_exit, &[], safe, &[]);

    builder.switch_to_block(overflow_exit);
    emit_native_capture_exit(
        builder,
        codegen,
        ip,
        completed_in_block,
        local_tags,
        stack_values,
        stack_tags,
    );
    builder.seal_block(overflow_exit);
    builder.switch_to_block(safe);
    builder.seal_block(safe);
}

fn emit_native_capture_exit(
    builder: &mut FunctionBuilder<'_>,
    codegen: NativeContinuationCodegen<'_>,
    ip: usize,
    completed_in_block: usize,
    local_tags: &[i64],
    stack_values: &[cranelift_codegen::ir::Value],
    stack_tags: &[i64],
) {
    let captured = emit_native_continuation_capture(
        builder,
        codegen.pointer_type,
        codegen.capture,
        codegen.arena,
        codegen.function_index,
        ip,
        codegen.depth,
        codegen.local_vars,
        codegen.live_locals_at_ip,
        codegen.capture_int2,
        codegen.capture_fixed,
        local_tags,
        stack_values,
        stack_tags,
        -1,
    );
    let zero = builder.ins().iconst(types::I32, 0);
    let captured = builder.ins().icmp(IntCC::NotEqual, captured, zero);
    let fallback_args = step_exit_args(
        builder,
        codegen.steps_var,
        ip,
        completed_in_block.saturating_add(1),
    );
    let fault_ip = builder.ins().iconst(types::I64, ip as i64);
    let completed = builder
        .ins()
        .iconst(types::I64, completed_in_block.min(i64::MAX as usize) as i64);
    builder.ins().brif(
        captured,
        codegen.continuation_exit,
        &[BlockArg::Value(fault_ip), BlockArg::Value(completed)],
        codegen.fallback,
        &fallback_args,
    );
}

fn emit_float_finite_guard(
    builder: &mut FunctionBuilder<'_>,
    value: cranelift_codegen::ir::Value,
    failure: cranelift_codegen::ir::Block,
    steps_var: Variable,
    ip: usize,
    completed_instructions: usize,
) {
    let max = builder.ins().f64const(f64::MAX);
    let min = builder.ins().f64const(-f64::MAX);
    let too_large = builder
        .ins()
        .fcmp(FloatCC::UnorderedOrGreaterThan, value, max);
    let too_small = builder.ins().fcmp(FloatCC::UnorderedOrLessThan, value, min);
    let invalid = builder.ins().bor(too_large, too_small);
    let okay = builder.create_block();
    let exit_args = step_exit_args(builder, steps_var, ip, completed_instructions);
    builder.ins().brif(invalid, failure, &exit_args, okay, &[]);
    builder.switch_to_block(okay);
    builder.seal_block(okay);
}

fn emit_success(
    builder: &mut FunctionBuilder<'_>,
    output: cranelift_codegen::ir::Value,
    steps: cranelift_codegen::ir::Value,
    unused_fuel: cranelift_codegen::ir::Value,
    calls: cranelift_codegen::ir::Value,
) {
    store_output(builder, output, 5, unused_fuel);
    store_output(builder, output, 1, steps);
    let zero = builder.ins().iconst(types::I64, 0);
    store_output(builder, output, 2, zero);
    store_output(builder, output, 3, zero);
    store_output(builder, output, 4, calls);
    let success = builder.ins().iconst(types::I32, 0);
    builder.ins().return_(&[success]);
}

fn emit_exit(
    builder: &mut FunctionBuilder<'_>,
    output: cranelift_codegen::ir::Value,
    status: i32,
    steps: cranelift_codegen::ir::Value,
    fault_ip: cranelift_codegen::ir::Value,
    function_index: usize,
    calls: cranelift_codegen::ir::Value,
) {
    store_output(builder, output, 1, steps);
    store_output(builder, output, 2, fault_ip);
    let function_index = builder
        .ins()
        .iconst(types::I64, function_index.min(i64::MAX as usize) as i64);
    store_output(builder, output, 3, function_index);
    store_output(builder, output, 4, calls);
    let status = builder.ins().iconst(types::I32, i64::from(status));
    builder.ins().return_(&[status]);
}

fn store_output(
    builder: &mut FunctionBuilder<'_>,
    output: cranelift_codegen::ir::Value,
    slot: i32,
    value: cranelift_codegen::ir::Value,
) {
    let address = builder.ins().iadd_imm_s(output, i64::from(slot * 8));
    builder.ins().store(MemFlagsData::new(), value, address, 0);
}

#[derive(Clone, PartialEq, Eq)]
struct TypeState {
    stack: Vec<Type>,
    locals: Vec<Option<Type>>,
}

struct NativeTypeStates {
    stack_at_ip: Vec<Option<Vec<Type>>>,
    locals_at_ip: Vec<Option<Vec<Option<Type>>>>,
}

fn recursive_functions(functions: &[Function]) -> Vec<bool> {
    fn visit(
        index: usize,
        functions: &[Function],
        states: &mut [u8],
        path: &mut Vec<usize>,
        recursive: &mut [bool],
    ) {
        states[index] = 1;
        path.push(index);
        for target in functions[index].code.iter().filter_map(|instruction| {
            if let Op::Call(target, _) = &instruction.op {
                Some(*target)
            } else {
                None
            }
        }) {
            if target >= functions.len() {
                continue;
            }
            match states[target] {
                0 => visit(target, functions, states, path, recursive),
                1 => {
                    if let Some(start) = path.iter().position(|candidate| *candidate == target) {
                        for member in &path[start..] {
                            recursive[*member] = true;
                        }
                    }
                }
                _ => {}
            }
        }
        path.pop();
        states[index] = 2;
    }

    let mut states = vec![0; functions.len()];
    let mut recursive = vec![false; functions.len()];
    let mut path = Vec::new();
    for index in 0..functions.len() {
        if states[index] == 0 {
            visit(index, functions, &mut states, &mut path, &mut recursive);
        }
    }
    recursive
}

fn infer_native_types(
    function: &Function,
    functions: &[Function],
    records: &[guion_model::RecordDefinition],
    blocking_ip: &mut Option<usize>,
) -> Option<NativeTypeStates> {
    *blocking_ip = None;
    if let Some(first_update) = function
        .code
        .iter()
        .position(|instruction| matches!(instruction.op, Op::ListSetLocal(_)))
    {
        *blocking_ip = Some(first_update);
        // A list handle can be shared by a record created earlier in the function.
        // Until the verifier can prove uniqueness, don't compile local in-place
        // updates across that potential alias.
        if function.code[..first_update]
            .iter()
            .any(|instruction| matches!(instruction.op, Op::MakeRecord(_, _)))
        {
            return None;
        }
        if function.code.iter().any(|instruction| {
            matches!(instruction.op, Op::ListSetLocal(slot) if slot < function.parameters.len())
        }) {
            return None;
        }
        let mut read_fields = std::collections::HashSet::new();
        for instruction in &function.code[..first_update] {
            if let Op::GetField(record_index, field_index) = instruction.op {
                let field = records.get(record_index)?.fields.get(field_index)?;
                if matches!(&field.ty, Type::List(item) if matches!(**item, Type::Float64 | Type::Int | Type::Bool))
                    && !read_fields.insert((record_index, field_index))
                {
                    return None;
                }
            }
        }
    }
    let is_numeric = |ty: &Type| matches!(ty, Type::Int | Type::Float64);
    let is_native_item = |ty: &Type| is_numeric(ty) || *ty == Type::Bool;
    let is_native_list_item = |ty: &Type| {
        is_native_item(ty)
            || matches!(
                ty,
                Type::Text
                    | Type::Bytes
                    | Type::List(_)
                    | Type::Map(_)
                    | Type::Option(_)
                    | Type::Result(_, _)
                    | Type::TaskError
                    | Type::Json
                    | Type::JsonError
                    | Type::FileError
                    | Type::HttpError
                    | Type::NativeResource(_)
                    | Type::Record(_)
            )
    };
    let is_native_parameter = |ty: &Type| {
        is_native_item(ty)
            || matches!(
                ty,
                Type::Text
                    | Type::Bytes
                    | Type::List(_)
                    | Type::Map(_)
                    | Type::Option(_)
                    | Type::Result(_, _)
                    | Type::TaskError
                    | Type::Json
                    | Type::JsonError
                    | Type::FileError
                    | Type::HttpError
                    | Type::NativeResource(_)
            )
            || matches!(ty, Type::Record(name) if records.iter().any(|record| record.name == *name))
    };
    if function.code.is_empty()
        || native_result_types(&function.result, records).is_none()
        || !function.parameters.iter().all(is_native_parameter)
    {
        return None;
    }
    let mut initial = TypeState {
        stack: Vec::new(),
        locals: vec![None; function.local_count],
    };
    for (index, ty) in function.parameters.iter().enumerate() {
        initial.locals[index] = Some(ty.clone());
    }
    let mut states = vec![None; function.code.len()];
    let mut stack_at_ip = vec![None; function.code.len()];
    let mut locals_at_ip = vec![None; function.code.len()];
    states[0] = Some(initial);
    let mut pending = VecDeque::from([0]);
    while let Some(ip) = pending.pop_front() {
        *blocking_ip = Some(ip);
        let mut state = states[ip].clone()?;
        stack_at_ip[ip] = Some(state.stack.clone());
        locals_at_ip[ip] = Some(state.locals.clone());
        let op = &function.code[ip].op;
        match op {
            Op::Const(Value::Int(value)) => {
                value.to_string().parse::<i64>().ok()?;
                state.stack.push(Type::Int);
            }
            Op::Const(Value::Text(_)) => state.stack.push(Type::Text),
            Op::Args => state.stack.push(Type::List(Box::new(Type::Text))),
            Op::Const(Value::Bytes(_)) => state.stack.push(Type::Bytes),
            Op::Const(Value::Float64(_)) => state.stack.push(Type::Float64),
            Op::Const(Value::Bool(_)) => state.stack.push(Type::Bool),
            Op::Load(slot) => state.stack.push(state.locals.get(*slot)?.clone()?),
            Op::GetField(record_index, field_index) => {
                let definition = records.get(*record_index)?;
                if state.stack.pop()? != Type::Record(definition.name.clone()) {
                    return None;
                }
                state
                    .stack
                    .push(definition.fields.get(*field_index)?.ty.clone());
            }
            Op::MakeRecord(record_index, count) => {
                let definition = records.get(*record_index)?;
                if *count != definition.fields.len() {
                    return None;
                }
                let start = state.stack.len().checked_sub(*count)?;
                if state.stack[start..]
                    != definition
                        .fields
                        .iter()
                        .map(|field| field.ty.clone())
                        .collect::<Vec<_>>()
                {
                    return None;
                }
                state.stack.truncate(start);
                state.stack.push(Type::Record(definition.name.clone()));
            }
            Op::Store(slot) => {
                let ty = state.stack.pop()?;
                if matches!(&ty, Type::List(item) if is_native_item(item))
                    && !matches!(
                        function.code.get(ip.wrapping_sub(1))?.op,
                        Op::GetField(_, _) | Op::MakeList(_) | Op::MakeTypedList(_) | Op::ListSet
                    )
                {
                    return None;
                }
                let local = state.locals.get_mut(*slot)?;
                if local.as_ref().is_some_and(|existing| *existing != ty) {
                    return None;
                }
                *local = Some(ty);
            }
            Op::Pop => {
                state.stack.pop()?;
            }
            Op::Add | Op::Subtract | Op::Multiply | Op::Divide => {
                let right = state.stack.pop()?;
                let left = state.stack.pop()?;
                if left != right
                    || (!is_numeric(&left) && !(matches!(op, Op::Add) && left == Type::Text))
                {
                    return None;
                }
                state.stack.push(left);
            }
            Op::FloatSqrt => {
                if state.stack.pop()? != Type::Float64 {
                    return None;
                }
                state.stack.push(Type::Float64);
            }
            Op::Remainder => {
                if state.stack.pop()? != Type::Int || state.stack.pop()? != Type::Int {
                    return None;
                }
                state.stack.push(Type::Int);
            }
            Op::IntegerDivide | Op::FloorDivide => {
                if state.stack.pop()? != Type::Int || state.stack.pop()? != Type::Int {
                    return None;
                }
                state.stack.push(Type::Int);
            }
            Op::Negate => {
                let ty = state.stack.pop()?;
                if !is_numeric(&ty) {
                    return None;
                }
                state.stack.push(ty);
            }
            Op::Not => {
                if state.stack.pop()? != Type::Bool {
                    return None;
                }
                state.stack.push(Type::Bool);
            }
            Op::JsonTryText => {
                if state.stack.pop()? != Type::Json {
                    return None;
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Text),
                    Box::new(Type::JsonError),
                ));
            }
            Op::JsonTryGet => {
                if state.stack.pop()? != Type::Text || state.stack.pop()? != Type::Json {
                    return None;
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Json),
                    Box::new(Type::JsonError),
                ));
            }
            Op::JsonTryInt => {
                if state.stack.pop()? != Type::Json {
                    return None;
                }
                state
                    .stack
                    .push(Type::Result(Box::new(Type::Int), Box::new(Type::JsonError)));
            }
            Op::JsonParse => {
                if state.stack.pop()? != Type::Text {
                    return None;
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Json),
                    Box::new(Type::JsonError),
                ));
            }
            Op::FileNextLine => {
                if !matches!(state.stack.pop()?, Type::NativeResource(_)) {
                    return None;
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Option(Box::new(Type::Record("SourceLine".into())))),
                    Box::new(Type::FileError),
                ));
            }
            Op::IsOk => {
                if !matches!(state.stack.pop()?, Type::Result(_, _)) {
                    return None;
                }
                state.stack.push(Type::Bool);
            }
            Op::IsSome => {
                if !matches!(state.stack.pop()?, Type::Option(_)) {
                    return None;
                }
                state.stack.push(Type::Bool);
            }
            Op::Unwrap => {
                let ty = state.stack.pop()?;
                match ty {
                    Type::Result(ok, _) | Type::Option(ok) => state.stack.push(*ok),
                    _ => return None,
                }
            }
            Op::GetError => {
                let Type::Result(_, error) = state.stack.pop()? else {
                    return None;
                };
                state.stack.push(*error);
            }
            Op::MakeOk => {
                let Type::Result(ok, _) = &function.result else {
                    return None;
                };
                if state.stack.pop()? != **ok {
                    return None;
                }
                state.stack.push(function.result.clone());
            }
            Op::MakeErr => {
                let Type::Result(_, error) = &function.result else {
                    return None;
                };
                if state.stack.pop()? != **error {
                    return None;
                }
                state.stack.push(function.result.clone());
            }
            Op::MakeSome => {
                let value = state.stack.pop()?;
                state.stack.push(Type::Option(Box::new(value)));
            }
            Op::MakeNone => {
                if !matches!(function.result, Type::Option(_)) {
                    return None;
                }
                state.stack.push(function.result.clone());
            }
            Op::Length => {
                if !matches!(state.stack.pop()?, Type::List(item) if is_native_list_item(&item)) {
                    return None;
                }
                state.stack.push(Type::Int);
            }
            Op::MakeList(count) => {
                let start = state.stack.len().checked_sub(*count)?;
                let item_type = state.stack.get(start).cloned()?;
                if !is_native_list_item(&item_type)
                    || state.stack[start..].iter().any(|ty| *ty != item_type)
                {
                    return None;
                }
                state.stack.truncate(start);
                state.stack.push(Type::List(Box::new(item_type)));
            }
            Op::MakeTypedList(item_type) => {
                if !is_native_list_item(item_type) {
                    return None;
                }
                state.stack.push(Type::List(Box::new(item_type.clone())));
            }
            Op::MakeTypedMap(item_type) => {
                if !is_native_parameter(item_type) {
                    return None;
                }
                state.stack.push(Type::Map(Box::new(item_type.clone())));
            }
            Op::JsonArray => state.stack.push(Type::Json),
            Op::JsonObject => state.stack.push(Type::Json),
            Op::JsonInt => {
                if state.stack.pop()? != Type::Int {
                    return None;
                }
                state.stack.push(Type::Json);
            }
            Op::JsonText => {
                if state.stack.pop()? != Type::Text {
                    return None;
                }
                state.stack.push(Type::Json);
            }
            Op::FileOpenLines => {
                if state.stack.pop()? != Type::Text {
                    return None;
                }
                state.stack.push(Type::Result(
                    Box::new(Type::NativeResource("FileReader".into())),
                    Box::new(Type::FileError),
                ));
            }
            Op::FileClose => {
                if state.stack.pop()? != Type::NativeResource("FileReader".into()) {
                    return None;
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Unit),
                    Box::new(Type::FileError),
                ));
            }
            Op::TextCollapseWhitespace => {
                if state.stack.pop()? != Type::Text {
                    return None;
                }
                state.stack.push(Type::Text);
            }
            Op::TimeParseTimestamp => {
                if state.stack.pop()? != Type::Text {
                    return None;
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Record("Instant".into())),
                    Box::new(Type::FileError),
                ));
            }
            Op::TimeFormatUtc => {
                if state.stack.pop()? != Type::Record("Instant".into()) {
                    return None;
                }
                state.stack.push(Type::Text);
            }
            Op::TextSlice => {
                if state.stack.pop()? != Type::Int
                    || state.stack.pop()? != Type::Int
                    || state.stack.pop()? != Type::Text
                {
                    return None;
                }
                state.stack.push(Type::Text);
            }
            Op::MapHas => {
                if state.stack.pop()? != Type::Text {
                    return None;
                }
                if !matches!(state.stack.pop()?, Type::Map(_)) {
                    return None;
                }
                state.stack.push(Type::Bool);
            }
            Op::MapKeys => {
                if !matches!(state.stack.pop()?, Type::Map(_)) {
                    return None;
                }
                state.stack.push(Type::List(Box::new(Type::Text)));
            }
            Op::JsonIndex => {
                if state.stack.pop()? != Type::Int || state.stack.pop()? != Type::Json {
                    return None;
                }
                state.stack.push(Type::Json);
            }
            Op::JsonGet => {
                if state.stack.pop()? != Type::Text || state.stack.pop()? != Type::Json {
                    return None;
                }
                state.stack.push(Type::Json);
            }
            Op::JsonStringify => {
                if state.stack.pop()? != Type::Json {
                    return None;
                }
                state.stack.push(Type::Text);
            }
            Op::JsonSetLocal(slot) => {
                if state.stack.pop()? != Type::Json
                    || state.stack.pop()? != Type::Text
                    || state.locals.get(*slot).and_then(Option::as_ref) != Some(&Type::Json)
                {
                    return None;
                }
            }
            Op::JsonPushLocal(slot) => {
                if state.stack.pop()? != Type::Json
                    || state.locals.get(*slot).and_then(Option::as_ref) != Some(&Type::Json)
                {
                    return None;
                }
            }
            Op::WriteTextAtomic => {
                if state.stack.pop()? != Type::Text || state.stack.pop()? != Type::Text {
                    return None;
                }
                state.stack.push(Type::Result(
                    Box::new(Type::Unit),
                    Box::new(Type::FileError),
                ));
            }
            Op::MapSet => {
                let value_type = state.stack.pop()?;
                if state.stack.pop()? != Type::Text {
                    return None;
                }
                let Type::Map(item_type) = state.stack.pop()? else {
                    return None;
                };
                if value_type != *item_type || !is_native_parameter(&item_type) {
                    return None;
                }
                state.stack.push(Type::Map(item_type));
            }
            Op::TextChars => {
                if state.stack.pop()? != Type::Text {
                    return None;
                }
                state.stack.push(Type::List(Box::new(Type::Text)));
            }
            Op::Index => {
                let index_type = state.stack.pop()?;
                match state.stack.pop()? {
                    Type::List(item) if index_type == Type::Int && is_native_list_item(&item) => {
                        state.stack.push(*item);
                    }
                    Type::Map(item) if index_type == Type::Text && is_native_parameter(&item) => {
                        state.stack.push(*item);
                    }
                    _ => return None,
                }
            }
            Op::ListSet => {
                let value_type = state.stack.pop()?;
                if state.stack.pop()? != Type::Int {
                    return None;
                }
                let Type::List(item_type) = state.stack.pop()? else {
                    return None;
                };
                if *item_type != value_type || !is_native_item(&item_type) {
                    return None;
                }
                state.stack.push(Type::List(item_type));
            }
            Op::Equal | Op::NotEqual => {
                let right = state.stack.pop()?;
                let left = state.stack.pop()?;
                if left != right || !(is_numeric(&left) || left == Type::Bool || left == Type::Text)
                {
                    return None;
                }
                state.stack.push(Type::Bool);
            }
            Op::Less | Op::LessEqual | Op::Greater | Op::GreaterEqual => {
                let right = state.stack.pop()?;
                let left = state.stack.pop()?;
                if left != right || !is_numeric(&left) {
                    return None;
                }
                state.stack.push(Type::Bool);
            }
            Op::JumpIfFalse(_) => {
                if state.stack.pop()? != Type::Bool {
                    return None;
                }
            }
            Op::Jump(_) => {}
            Op::Call(target, argc) => {
                let callee = functions.get(*target)?;
                if *argc != callee.parameters.len()
                    || native_result_types(&callee.result, records).is_none()
                    || !callee.parameters.iter().all(is_native_parameter)
                {
                    return None;
                }
                let start = state.stack.len().checked_sub(*argc)?;
                if state.stack[start..] != callee.parameters {
                    return None;
                }
                state.stack.truncate(start);
                state.stack.push(callee.result.clone());
            }
            Op::Return => {
                if state.stack.pop()? != function.result || !state.stack.is_empty() {
                    return None;
                }
            }
            Op::Print | Op::PrintErr => {
                let printed_type = state.stack.pop()?;
                if matches!(op, Op::Print) {
                    if !matches!(printed_type, Type::Int | Type::Float64 | Type::Bool) {
                        return None;
                    }
                } else if printed_type != Type::Text {
                    return None;
                }
            }
            Op::ListSetLocal(slot) => {
                let value = state.stack.pop()?;
                if state.stack.pop()? != Type::Int {
                    return None;
                }
                let list_type = state.locals.get(*slot)?.as_ref()?;
                if !matches!(list_type, Type::List(item) if **item == value && is_native_item(item))
                {
                    return None;
                }
            }
            Op::ListPushLocal(_) => return None,
            _ => return None,
        }

        let mut successors = Vec::with_capacity(2);
        match op {
            Op::Return => {}
            Op::Jump(target) => successors.push(*target),
            Op::JumpIfFalse(target) => {
                successors.push(*target);
                successors.push(ip + 1);
            }
            _ => successors.push(ip + 1),
        }
        for successor in successors {
            if successor >= function.code.len() {
                return None;
            }
            match &mut states[successor] {
                None => {
                    states[successor] = Some(state.clone());
                    pending.push_back(successor);
                }
                Some(existing) => {
                    if existing.stack != state.stack {
                        *blocking_ip = Some(successor);
                        return None;
                    }
                    let mut changed = false;
                    for (old, new) in existing.locals.iter_mut().zip(&state.locals) {
                        if old != new && old.is_some() {
                            *old = None;
                            changed = true;
                        }
                    }
                    if changed {
                        pending.push_back(successor);
                    }
                }
            }
        }
    }
    *blocking_ip = None;
    Some(NativeTypeStates {
        stack_at_ip,
        locals_at_ip,
    })
}

fn continuation_tag(ty: &Type, records: &[guion_model::RecordDefinition]) -> i64 {
    match ty {
        Type::Unit => 0,
        Type::Int => NATIVE_BRIDGE_TAG_INT,
        Type::Float64 => NATIVE_BRIDGE_TAG_FLOAT,
        Type::Bool => NATIVE_BRIDGE_TAG_BOOL,
        Type::List(item) if matches!(**item, Type::Int | Type::Float64 | Type::Bool) => -1,
        Type::Record(name)
            if records
                .iter()
                .find(|record| record.name == *name)
                .is_some_and(native_record_has_native_layout) =>
        {
            -1
        }
        _ => NATIVE_BRIDGE_TAG_HANDLE,
    }
}

fn live_local_slots_at_ip(function: &Function) -> Vec<Vec<usize>> {
    let instruction_count = function.code.len();
    let mut live_in = vec![vec![false; function.local_count]; instruction_count];
    loop {
        let mut changed = false;
        for ip in (0..instruction_count).rev() {
            let instruction = &function.code[ip];
            let mut live_out = vec![false; function.local_count];
            let mut merge_successor = |successor: usize| {
                if let Some(successor_live) = live_in.get(successor) {
                    for (out, live) in live_out.iter_mut().zip(successor_live) {
                        *out |= *live;
                    }
                }
            };
            match &instruction.op {
                Op::Return => {}
                Op::Jump(target) => merge_successor(*target),
                Op::JumpIfFalse(target) => {
                    merge_successor(*target);
                    merge_successor(ip + 1);
                }
                _ => merge_successor(ip + 1),
            }

            let mut uses = vec![false; function.local_count];
            let mut defines = vec![false; function.local_count];
            match instruction.op {
                Op::Load(slot) => uses[slot] = true,
                Op::Store(slot) => defines[slot] = true,
                Op::ListSetLocal(slot)
                | Op::ListPushLocal(slot)
                | Op::JsonPushLocal(slot)
                | Op::JsonSetLocal(slot) => {
                    uses[slot] = true;
                    defines[slot] = true;
                }
                _ => {}
            }
            for slot in 0..function.local_count {
                let next = uses[slot] || (live_out[slot] && !defines[slot]);
                if live_in[ip][slot] != next {
                    live_in[ip][slot] = next;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    live_in
        .into_iter()
        .map(|slots| {
            slots
                .into_iter()
                .enumerate()
                .filter_map(|(slot, live)| live.then_some(slot))
                .collect()
        })
        .collect()
}

fn continuation_local_tags(
    function: &Function,
    type_states: &NativeTypeStates,
    ip: usize,
    records: &[guion_model::RecordDefinition],
) -> Vec<i64> {
    let Some(locals) = type_states.locals_at_ip.get(ip).and_then(Option::as_ref) else {
        return vec![-1; function.local_count];
    };
    locals
        .iter()
        .enumerate()
        .map(|(slot, ty)| match ty {
            Some(ty) => continuation_tag(ty, records),
            None if slot >= function.parameters.len()
                && !function.code[..ip.min(function.code.len())].iter().any(
                    |instruction| matches!(instruction.op, Op::Store(stored) if stored == slot),
                ) =>
            {
                0
            }
            None => -1,
        })
        .collect()
}

fn continuation_stack_tags(
    type_states: &NativeTypeStates,
    ip: usize,
    stack_count: usize,
    records: &[guion_model::RecordDefinition],
) -> Vec<i64> {
    let Some(stack) = type_states.stack_at_ip.get(ip).and_then(Option::as_ref) else {
        return vec![-1; stack_count];
    };
    stack
        .iter()
        .take(stack_count)
        .map(|ty| continuation_tag(ty, records))
        .collect()
}

fn instruction_requires_native_value_bridge(instruction: &guion_model::Instruction) -> bool {
    match &instruction.op {
        // MakeRecord and GetField each have operation-specific callbacks that
        // preserve value cloning and guest allocation accounting for mixed records.
        Op::MakeRecord(_, _)
        | Op::GetField(_, _)
        | Op::Equal
        | Op::NotEqual
        | Op::PrintErr
        | Op::MakeTypedMap(_)
        | Op::JsonArray
        | Op::MapKeys
        | Op::JsonIndex
        | Op::TextChars
        | Op::JsonGet
        | Op::JsonStringify
        | Op::Add
        | Op::JsonObject
        | Op::Index
        | Op::MapSet
        | Op::JsonInt
        | Op::MapHas
        | Op::JsonText
        | Op::FileOpenLines
        | Op::FileClose
        | Op::TextCollapseWhitespace
        | Op::TimeParseTimestamp
        | Op::TimeFormatUtc
        | Op::TextSlice => false,
        Op::JsonSetLocal(_) => false,
        Op::JsonPushLocal(_) => false,
        Op::Args
        | Op::FileNextLine
        | Op::JsonParse
        | Op::JsonTryGet
        | Op::JsonTryText
        | Op::JsonTryInt => false,
        _ => native_bridge_operation(&instruction.op).is_some(),
    }
}

fn function_requires_native_value_bridge(function: &Function) -> bool {
    function
        .code
        .iter()
        .any(instruction_requires_native_value_bridge)
}

fn functions_reaching_native_host_callbacks(artifact: &Artifact) -> Vec<bool> {
    let mut reaches_callback = vec![false; artifact.functions.len()];
    let mut callers = vec![Vec::new(); artifact.functions.len()];
    let mut pending = VecDeque::new();
    for (caller, function) in artifact.functions.iter().enumerate() {
        for instruction in &function.code {
            if matches!(
                instruction.op,
                Op::JsonParse
                    | Op::JsonTryGet
                    | Op::JsonTryText
                    | Op::JsonTryInt
                    | Op::FileNextLine
            ) && !reaches_callback[caller]
            {
                reaches_callback[caller] = true;
                pending.push_back(caller);
            }
            if let Op::Call(target, _) = &instruction.op {
                if let Some(target_callers) = callers.get_mut(*target) {
                    target_callers.push(caller);
                }
            }
        }
    }

    while let Some(callee) = pending.pop_front() {
        for caller in &callers[callee] {
            if !reaches_callback[*caller] {
                reaches_callback[*caller] = true;
                pending.push_back(*caller);
            }
        }
    }
    reaches_callback
}

fn functions_reaching_native_arena_allocations(artifact: &Artifact) -> Vec<bool> {
    let mut reaches_allocation = vec![false; artifact.functions.len()];
    let mut callers = vec![Vec::new(); artifact.functions.len()];
    let mut pending = VecDeque::new();
    for (caller, function) in artifact.functions.iter().enumerate() {
        if function
            .code
            .iter()
            .any(|instruction| instruction_may_allocate_native_arena_value(&instruction.op))
        {
            reaches_allocation[caller] = true;
            pending.push_back(caller);
        }
        for instruction in &function.code {
            if let Op::Call(target, _) = &instruction.op {
                if let Some(target_callers) = callers.get_mut(*target) {
                    target_callers.push(caller);
                }
            }
        }
    }

    while let Some(callee) = pending.pop_front() {
        for caller in &callers[callee] {
            if !reaches_allocation[*caller] {
                reaches_allocation[*caller] = true;
                pending.push_back(*caller);
            }
        }
    }
    reaches_allocation
}

fn safepoint_type_ok(ty: &Type) -> bool {
    match ty {
        Type::Int
        | Type::Float64
        | Type::Bool
        | Type::Unit
        | Type::Text
        | Type::Bytes
        | Type::Json
        | Type::JsonError
        | Type::FileError
        | Type::HttpError
        | Type::NativeResource(_) => true,
        Type::Map(_) => true,
        Type::Option(inner) => safepoint_type_ok(inner),
        Type::Result(ok, error) => safepoint_type_ok(ok) && safepoint_type_ok(error),
        _ => false,
    }
}

/// Live handle-tagged locals that must survive an arena reset at this backedge.
/// `None` means a live value is not one the arena can evacuate and re-encode.
fn loop_safepoint_roots(
    function: &Function,
    type_states: &NativeTypeStates,
    live_locals_at_ip: &[Vec<usize>],
    records: &[guion_model::RecordDefinition],
    ip: usize,
    target: usize,
) -> Option<Vec<usize>> {
    if !type_states.stack_at_ip.get(ip)?.as_ref()?.is_empty() {
        return None;
    }
    let locals = type_states.locals_at_ip.get(target)?.as_ref()?;
    let mut roots = Vec::new();
    for slot in live_locals_at_ip.get(target)? {
        let ty = locals.get(*slot)?.as_ref()?;
        match ty {
            Type::Int | Type::Float64 | Type::Bool | Type::Unit => {}
            Type::List(item) if **item == Type::Text => roots.push(*slot),
            ty if safepoint_type_ok(ty)
                && continuation_tag(ty, records) == NATIVE_BRIDGE_TAG_HANDLE =>
            {
                roots.push(*slot)
            }
            _ => return None,
        }
    }
    let _ = function;
    Some(roots)
}

/// Functions whose allocating loops can retire arena handles at each backedge:
/// nothing else calls them, every backedge is an unconditional `Jump` with an empty
/// operand stack, and every live local can be evacuated.
fn loop_safepoint_functions(artifact: &Artifact) -> Vec<bool> {
    if !cfg!(feature = "native-value-bridge") {
        return vec![false; artifact.functions.len()];
    }
    let recursive = recursive_functions(&artifact.functions);
    let reaches_allocation = functions_reaching_native_arena_allocations(artifact);
    let mut has_caller = vec![false; artifact.functions.len()];
    for (caller, function) in artifact.functions.iter().enumerate() {
        for instruction in &function.code {
            if let Op::Call(target, _) = &instruction.op {
                if *target != caller {
                    if let Some(called) = has_caller.get_mut(*target) {
                        *called = true;
                    }
                }
            }
        }
    }
    artifact
        .functions
        .iter()
        .enumerate()
        .map(|(index, function)| {
            if recursive[index]
                || has_caller[index]
                || function_loop_backedge_with_arena_allocation(function, &reaches_allocation)
                    .is_none()
            {
                return false;
            }
            let mut blocking_ip = None;
            let Some(type_states) = infer_native_types(
                function,
                &artifact.functions,
                &artifact.records,
                &mut blocking_ip,
            ) else {
                return false;
            };
            let live = live_local_slots_at_ip(function);
            (0..function.code.len())
                .filter(|ip| {
                    loop_backedge_allocates_arena_values(function, &reaches_allocation, *ip)
                })
                .all(|ip| match &function.code[ip].op {
                    Op::Jump(target) => loop_safepoint_roots(
                        function,
                        &type_states,
                        &live,
                        &artifact.records,
                        ip,
                        *target,
                    )
                    .is_some(),
                    _ => false,
                })
        })
        .collect()
}

fn instruction_may_allocate_native_arena_value(op: &Op) -> bool {
    match op {
        Op::Args
        | Op::ArgsFrom
        | Op::FileOpenLines
        | Op::FileNextLine
        | Op::JsonParse
        | Op::JsonStringify
        | Op::JsonNull
        | Op::JsonBool
        | Op::JsonInt
        | Op::JsonFloat
        | Op::JsonText
        | Op::JsonArray
        | Op::JsonObject
        | Op::JsonPush
        | Op::JsonSet
        | Op::JsonGet
        | Op::JsonIndex
        | Op::JsonAsFloat
        | Op::JsonAsText
        | Op::JsonAsInt
        | Op::JsonAsBool
        | Op::JsonIntColumn
        | Op::JsonTextCategories
        | Op::JsonAsIntList
        | Op::JsonProjectColumns
        | Op::JsonTextCategoriesValues
        | Op::JsonPushLocal(_)
        | Op::JsonSetLocal(_)
        | Op::JsonErrorMessage
        | Op::JsonErrorLine
        | Op::JsonErrorColumn
        | Op::JsonErrorCode
        | Op::TextChars
        | Op::TextCollapseWhitespace
        | Op::TextSlice
        | Op::TimeParseTimestamp
        | Op::TimeFormatUtc
        | Op::JsonTryGet
        | Op::JsonTryText
        | Op::JsonTryInt => true,
        _ => false,
    }
}

fn native_eligibility_report(
    artifact: &Artifact,
    native_value_bridge_enabled: bool,
) -> (Vec<bool>, Vec<Option<NativeFunctionRejectionProfile>>) {
    let recursive = recursive_functions(&artifact.functions);
    let reaches_native_arena_allocation = functions_reaching_native_arena_allocations(artifact);
    let safepoint_functions = loop_safepoint_functions(artifact);
    let native_list_mutator: Vec<_> = artifact
        .functions
        .iter()
        .map(|function| {
            function.code.iter().any(|instruction| {
                matches!(
                    instruction.op,
                    Op::ListSetLocal(_) | Op::ListPushLocal(_) | Op::JsonPushLocal(_)
                )
            })
        })
        .collect();
    let mut rejected: Vec<Option<NativeFunctionRejectionProfile>> = artifact
        .functions
        .iter()
        .enumerate()
        .map(|(index, function)| {
            if recursive[index] {
                return Some(NativeFunctionRejectionProfile {
                    name: function.name.clone(),
                    reason_code: "recursive_function".into(),
                    reason: "recursive functions are not compiled by the native backend".into(),
                    blocking_instruction: None,
                    blocking_callee: None,
                });
            }
            if let Some(backedge_ip) = (!safepoint_functions[index])
                .then(|| {
                    function_loop_backedge_with_arena_allocation(
                        function,
                        &reaches_native_arena_allocation,
                    )
                })
                .flatten()
            {
                return Some(NativeFunctionRejectionProfile {
                    name: function.name.clone(),
                    reason_code: "native_loop_value_arena_retention".into(),
                    reason: "native arena handles are retained for the invocation, so loops that allocate JSON, file, or text values must stay in the VM until the native arena can retire dead handles".into(),
                    blocking_instruction: Some(format!(
                        "loop backedge at bytecode instruction {backedge_ip}"
                    )),
                    blocking_callee: None,
                });
            }
            if !native_value_bridge_enabled
                && function_requires_native_value_bridge(function)
            {
                let blocker = function.code.iter().find_map(|instruction| {
                    instruction_requires_native_value_bridge(instruction)
                        .then(|| format!("{:?}", instruction.op))
                });
                return Some(NativeFunctionRejectionProfile {
                    name: function.name.clone(),
                    reason_code: "mixed_value_bridge_disabled".into(),
                    reason: "function signature or bytecode requires the opt-in mixed-value native bridge".into(),
                    blocking_instruction: blocker,
                    blocking_callee: None,
                });
            }
            let mut blocking_ip = None;
            if infer_native_types(
                function,
                &artifact.functions,
                &artifact.records,
                &mut blocking_ip,
            )
            .is_none()
            {
                let blocking_instruction = blocking_ip.and_then(|ip| {
                    function
                        .code
                        .get(ip)
                        .map(|instruction| format!("bytecode instruction {ip}: {:?}", instruction.op))
                });
                return Some(NativeFunctionRejectionProfile {
                    name: function.name.clone(),
                    reason_code: "unsupported_types_or_control_flow".into(),
                    reason: "native type inference could not prove the function's stack and control-flow types".into(),
                    blocking_instruction,
                    blocking_callee: None,
                });
            }
            None
        })
        .collect();
    let mut eligible: Vec<_> = rejected.iter().map(Option::is_none).collect();

    loop {
        let previous_eligibility = eligible.clone();
        let mut changed = false;
        for (index, function) in artifact.functions.iter().enumerate() {
            if !previous_eligibility[index] {
                continue;
            }
            let blocking_call = function
                .code
                .iter()
                .enumerate()
                .find_map(|(ip, instruction)| {
                    let Op::Call(target, _) = &instruction.op else {
                        return None;
                    };
                    (!previous_eligibility.get(*target).copied().unwrap_or(false)
                        || native_list_mutator.get(*target).copied().unwrap_or(false))
                    .then_some((ip, *target))
                });
            if let Some((ip, target)) = blocking_call {
                eligible[index] = false;
                let callee_name = artifact
                    .functions
                    .get(target)
                    .map(|callee| callee.name.clone());
                rejected[index] = Some(NativeFunctionRejectionProfile {
                    name: function.name.clone(),
                    reason_code: "callee_requires_vm".into(),
                    reason: "function calls a callee that is not available in the native subset"
                        .into(),
                    blocking_instruction: Some(format!("Call at bytecode instruction {ip}")),
                    blocking_callee: callee_name,
                });
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    (eligible, rejected)
}

fn loop_backedge_allocates_arena_values(
    function: &Function,
    reaches_native_arena_allocation: &[bool],
    ip: usize,
) -> bool {
    let target = match function.code.get(ip).map(|instruction| &instruction.op) {
        Some(Op::Jump(target) | Op::JumpIfFalse(target)) if *target <= ip => *target,
        _ => return false,
    };
    function.code.get(target..=ip).is_some_and(|loop_body| {
        loop_body.iter().any(|instruction| match &instruction.op {
            Op::Call(callee, _) => reaches_native_arena_allocation
                .get(*callee)
                .copied()
                .unwrap_or(false),
            op => instruction_may_allocate_native_arena_value(op),
        })
    })
}

fn function_loop_backedge_with_arena_allocation(
    function: &Function,
    reaches_native_arena_allocation: &[bool],
) -> Option<usize> {
    (0..function.code.len()).find(|ip| {
        loop_backedge_allocates_arena_values(function, reaches_native_arena_allocation, *ip)
    })
}

fn native_result_types(
    result: &Type,
    records: &[guion_model::RecordDefinition],
) -> Option<Vec<Type>> {
    let is_numeric = |ty: &Type| matches!(ty, Type::Int | Type::Float64);
    let is_native_item = |ty: &Type| is_numeric(ty) || *ty == Type::Bool;
    let supported_field =
        |ty: &Type| is_native_item(ty) || matches!(ty, Type::List(item) if is_native_item(item));
    match result {
        Type::Int | Type::Float64 | Type::Bool => Some(vec![result.clone()]),
        Type::Record(name) => {
            let definition = records.iter().find(|record| record.name == *name)?;
            if definition
                .fields
                .iter()
                .all(|field| supported_field(&field.ty))
            {
                Some(
                    definition
                        .fields
                        .iter()
                        .map(|field| field.ty.clone())
                        .collect(),
                )
            } else {
                Some(vec![result.clone()])
            }
        }
        Type::Text
        | Type::Bytes
        | Type::Map(_)
        | Type::Option(_)
        | Type::Result(_, _)
        | Type::TaskError
        | Type::Json
        | Type::JsonError
        | Type::FileError
        | Type::HttpError
        | Type::NativeResource(_) => Some(vec![result.clone()]),
        _ => None,
    }
}

fn native_cloned_value_bytes(ty: &Type) -> usize {
    match ty {
        Type::Record(name) => std::mem::size_of::<String>().saturating_add(name.len()),
        _ => 0,
    }
}

fn native_value_clone_uses_helper(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Text
            | Type::Bytes
            | Type::Option(_)
            | Type::Result(_, _)
            | Type::TaskError
            | Type::JsonError
            | Type::FileError
            | Type::HttpError
            | Type::NativeResource(_)
    )
}

fn native_list_kind(item_type: &Type) -> Option<i64> {
    match item_type {
        Type::Float64 => Some(1),
        Type::Int => Some(2),
        Type::Bool => Some(3),
        Type::Text
        | Type::Bytes
        | Type::List(_)
        | Type::Map(_)
        | Type::Option(_)
        | Type::Result(_, _)
        | Type::TaskError
        | Type::Json
        | Type::JsonError
        | Type::FileError
        | Type::HttpError
        | Type::NativeResource(_)
        | Type::Record(_) => Some(4),
        _ => None,
    }
}

#[derive(Clone, Copy)]
struct NativeListSetFunctions {
    float_copy: FuncRef,
    int_copy: FuncRef,
    bool_copy: FuncRef,
    float_local: FuncRef,
    int_local: FuncRef,
    bool_local: FuncRef,
}

fn native_list_set_function(
    item_type: &Type,
    always_copy: bool,
    functions: NativeListSetFunctions,
) -> Option<FuncRef> {
    match item_type {
        Type::Float64 if always_copy => Some(functions.float_copy),
        Type::Int if always_copy => Some(functions.int_copy),
        Type::Bool if always_copy => Some(functions.bool_copy),
        Type::Float64 => Some(functions.float_local),
        Type::Int => Some(functions.int_local),
        Type::Bool => Some(functions.bool_local),
        _ => None,
    }
}

fn native_print_kind(value_type: &Type) -> Option<i64> {
    match value_type {
        Type::Int => Some(1),
        Type::Float64 => Some(2),
        Type::Bool => Some(3),
        _ => None,
    }
}

fn stack_depths(function: &Function) -> Option<Vec<Option<usize>>> {
    if function.code.is_empty() {
        return None;
    }
    let mut depths = vec![None; function.code.len()];
    depths[0] = Some(0);
    let mut pending = VecDeque::from([0]);
    while let Some(ip) = pending.pop_front() {
        let depth = depths[ip]?;
        let op = &function.code[ip].op;
        let (pops, pushes) = match op {
            Op::Const(_) | Op::Load(_) | Op::Args => (0, 1),
            Op::Store(_) | Op::Pop | Op::JumpIfFalse(_) | Op::Print | Op::PrintErr | Op::Return => {
                (1, 0)
            }
            Op::Negate
            | Op::Not
            | Op::FloatSqrt
            | Op::TextChars
            | Op::JsonTryText
            | Op::JsonTryInt
            | Op::JsonParse
            | Op::JsonStringify
            | Op::FileNextLine
            | Op::IsOk
            | Op::IsSome
            | Op::Unwrap
            | Op::GetError
            | Op::MakeOk
            | Op::MakeErr
            | Op::MakeSome => (1, 1),
            Op::JsonTryGet => (2, 1),
            Op::MakeNone => (0, 1),
            Op::TextSlice => (3, 1),
            Op::Length | Op::GetField(_, _) => (1, 1),
            Op::Index => (2, 1),
            Op::MakeRecord(_, count) => (*count, 1),
            Op::MakeList(count) => (*count, 1),
            Op::MakeTypedList(_) | Op::MakeTypedMap(_) | Op::JsonArray | Op::JsonObject => (0, 1),
            Op::JsonInt => (1, 1),
            Op::JsonText => (1, 1),
            Op::FileOpenLines | Op::FileClose => (1, 1),
            Op::TextCollapseWhitespace => (1, 1),
            Op::TimeParseTimestamp => (1, 1),
            Op::TimeFormatUtc => (1, 1),
            Op::MapKeys => (1, 1),
            Op::JsonIndex => (2, 1),
            Op::JsonGet => (2, 1),
            Op::JsonSetLocal(_) => (2, 0),
            Op::MapSet => (3, 1),
            Op::MapHas => (2, 1),
            Op::WriteTextAtomic => (2, 1),
            Op::ListSetLocal(_) => (2, 0),
            Op::ListPushLocal(_) | Op::JsonPushLocal(_) => (1, 0),
            Op::ListSet => (3, 1),
            Op::Add
            | Op::Subtract
            | Op::Multiply
            | Op::Equal
            | Op::NotEqual
            | Op::Less
            | Op::LessEqual
            | Op::Greater
            | Op::GreaterEqual
            | Op::Divide => (2, 1),
            Op::IntegerDivide | Op::FloorDivide | Op::Remainder => (2, 1),
            Op::Call(_, argc) => (*argc, 1),
            Op::Jump(_) => (0, 0),
            _ => return None,
        };
        if depth < pops {
            return None;
        }
        let next_depth = depth - pops + pushes;
        let mut successors = Vec::with_capacity(2);
        match op {
            Op::Return => {}
            Op::Jump(target) => successors.push(*target),
            Op::JumpIfFalse(target) => {
                successors.push(*target);
                successors.push(ip + 1);
            }
            _ => successors.push(ip + 1),
        }
        for successor in successors {
            if successor >= function.code.len() {
                return None;
            }
            match depths[successor] {
                Some(existing) if existing != next_depth => return None,
                Some(_) => {}
                None => {
                    depths[successor] = Some(next_depth);
                    pending.push_back(successor);
                }
            }
        }
    }
    Some(depths)
}

fn bytecode_block_layout(
    function: &Function,
    depths: &[Option<usize>],
) -> Option<(Vec<Option<usize>>, Vec<usize>)> {
    let instruction_count = function.code.len();
    if instruction_count == 0 || depths.len() != instruction_count {
        return None;
    }

    let mut leaders = vec![false; instruction_count];
    leaders[0] = true;
    for (ip, instruction) in function.code.iter().enumerate() {
        if depths[ip].is_none() {
            // Preserve the existing isolated exit for unreachable bytecode.
            leaders[ip] = true;
            continue;
        }
        match &instruction.op {
            Op::Jump(target) => {
                *leaders.get_mut(*target)? = true;
            }
            Op::JumpIfFalse(target) => {
                *leaders.get_mut(*target)? = true;
                *leaders.get_mut(ip.checked_add(1)?)? = true;
            }
            Op::Call(_, _) if ip + 1 < instruction_count && depths[ip + 1].is_some() => {
                leaders[ip + 1] = true;
            }
            Op::JsonSetLocal(_) if ip + 1 < instruction_count && depths[ip + 1].is_some() => {
                leaders[ip + 1] = true;
            }
            Op::JsonPushLocal(_) if ip + 1 < instruction_count && depths[ip + 1].is_some() => {
                leaders[ip + 1] = true;
            }
            Op::WriteTextAtomic if ip + 1 < instruction_count && depths[ip + 1].is_some() => {
                leaders[ip + 1] = true;
            }
            _ => {}
        }
    }

    let mut leader_indices = vec![None; instruction_count];
    let mut block_for_ip = Vec::with_capacity(instruction_count);
    let mut next_block = 0;
    let mut current_block = None;
    for (ip, is_leader) in leaders.into_iter().enumerate() {
        if is_leader {
            current_block = Some(next_block);
            leader_indices[ip] = current_block;
            next_block += 1;
        }
        block_for_ip.push(current_block?);
    }
    Some((leader_indices, block_for_ip))
}

// Reserve enough fuel at function entry and loop headers for the longest
// reachable path to the next header or return. Backedges make new regions.
fn fuel_reservation_requirements(
    function: &Function,
    leader_indices: &[Option<usize>],
    block_for_ip: &[usize],
    block_instruction_counts: &[usize],
) -> Option<Vec<usize>> {
    if leader_indices.len() != function.code.len()
        || block_for_ip.len() != function.code.len()
        || block_instruction_counts.is_empty()
    {
        return None;
    }

    let block_starts: Vec<_> = leader_indices
        .iter()
        .enumerate()
        .filter_map(|(ip, block)| block.map(|_| ip))
        .collect();
    if block_starts.len() != block_instruction_counts.len() {
        return None;
    }

    let mut region_starts = vec![false; block_instruction_counts.len()];
    region_starts[0] = true;
    let mut successors = vec![Vec::new(); block_instruction_counts.len()];

    for block in 0..block_starts.len() {
        if block_instruction_counts[block] == 0 {
            continue;
        }
        let start = block_starts[block];
        let end = block_starts
            .get(block + 1)
            .copied()
            .unwrap_or(function.code.len());
        let last_ip = (start..end)
            .rev()
            .find(|ip| block_for_ip.get(*ip).copied() == Some(block))?;
        let mut add_successor = |target_ip: usize| -> Option<()> {
            let target_block = *block_for_ip.get(target_ip)?;
            if !successors[block].contains(&target_block) {
                successors[block].push(target_block);
            }
            if target_ip <= last_ip {
                region_starts[target_block] = true;
            }
            Some(())
        };

        match &function.code[last_ip].op {
            Op::Jump(target) => add_successor(*target)?,
            Op::JumpIfFalse(target) => {
                add_successor(*target)?;
                add_successor(last_ip.checked_add(1)?)?;
            }
            Op::Call(_, _) => add_successor(last_ip.checked_add(1)?)?,
            Op::Return => {}
            _ if last_ip + 1 < function.code.len() => add_successor(last_ip + 1)?,
            _ => return None,
        }
    }

    let mut longest_region_path = vec![0usize; block_instruction_counts.len()];
    for block in (0..block_instruction_counts.len()).rev() {
        if block_instruction_counts[block] == 0 {
            continue;
        }
        let longest_tail = successors[block]
            .iter()
            .copied()
            .filter(|successor| !region_starts[*successor])
            .map(|successor| longest_region_path[successor])
            .max()
            .unwrap_or(0);
        longest_region_path[block] = block_instruction_counts[block].checked_add(longest_tail)?;
    }

    Some(
        longest_region_path
            .into_iter()
            .zip(region_starts)
            .map(|(required, region_start)| if region_start { required } else { 0 })
            .collect(),
    )
}
