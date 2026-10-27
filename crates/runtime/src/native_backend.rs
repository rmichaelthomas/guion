//! Host-native execution for verified numeric functions.
//!
//! The first backend slice compiles verified integer bytecode to machine code.
//! Functions that exceed the machine integer range deopt to the VM, which keeps
//! Guion's arbitrary-precision integer behavior.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;

use super::NativeListArena;
use cranelift_codegen::{
    ir::{
        condcodes::{FloatCC, IntCC},
        types, AbiParam, BlockArg, InstBuilder, MemFlagsData, Signature, StackSlotData,
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
    pub(crate) list_set: *const u8,
    pub(crate) charge_record: *const u8,
    pub(crate) print: *const u8,
    pub(crate) count_guest_branch: *const u8,
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
    list_set: FuncId,
    charge_record: FuncId,
    print: FuncId,
    count_guest_branch: Option<FuncId>,
    functions: Vec<Option<*const u8>>,
    result_words: Vec<usize>,
    max_result_words: usize,
    bytecode_instructions: usize,
    bytecode_blocks: usize,
    cranelift_ir_blocks: usize,
    cranelift_ir_instructions: usize,
    function_compile_profiles: Vec<NativeFunctionCompileProfile>,
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

/// Immutable native entry points shared by parallel workers. The owning
/// `NativeBackend` stays alive on the parent instance until those workers join.
#[derive(Debug)]
pub(crate) struct NativeCode {
    functions: Vec<Option<usize>>,
    result_words: Vec<usize>,
    max_result_words: usize,
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
        let mut builder = JITBuilder::new(default_libcall_names()).ok()?;
        builder.symbol("guion_reserve_fuel", symbols.reserve_fuel);
        builder.symbol("guion_release_fuel", symbols.release_fuel);
        builder.symbol("guion_poll_cancellation", symbols.poll_cancellation);
        builder.symbol("guion_make_list", symbols.make_list);
        builder.symbol("guion_list_set", symbols.list_set);
        builder.symbol("guion_charge_record", symbols.charge_record);
        builder.symbol("guion_print", symbols.print);
        if profile_guest_branches {
            builder.symbol("guion_count_guest_branch", symbols.count_guest_branch);
        }
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
        list_set_signature.params.push(AbiParam::new(types::I64));
        list_set_signature.params.push(AbiParam::new(types::I64));
        list_set_signature.returns.push(AbiParam::new(pointer_type));
        let list_set_id = module
            .declare_function("guion_list_set", Linkage::Import, &list_set_signature)
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
        let mut print_signature = Signature::new(module.target_config().default_call_conv);
        print_signature.params.push(AbiParam::new(pointer_type));
        print_signature.params.push(AbiParam::new(types::I64));
        print_signature.params.push(AbiParam::new(types::I64));
        print_signature.returns.push(AbiParam::new(types::I32));
        let print_id = module
            .declare_function("guion_print", Linkage::Import, &print_signature)
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
            list_set: list_set_id,
            charge_record: charge_record_id,
            print: print_id,
            count_guest_branch: count_guest_branch_id,
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
            bytecode_instructions: 0,
            bytecode_blocks: 0,
            cranelift_ir_blocks: 0,
            cranelift_ir_instructions: 0,
            function_compile_profiles: Vec::new(),
            finalize_nanoseconds: 0,
        };
        let recursive = recursive_functions(&artifact.functions);
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
        let mut eligible: Vec<_> = artifact
            .functions
            .iter()
            .enumerate()
            .map(|(index, function)| {
                !recursive[index]
                    && infer_native_types(function, &artifact.functions, &artifact.records)
                        .is_some()
            })
            .collect();
        loop {
            let mut changed = false;
            for (index, function) in artifact.functions.iter().enumerate() {
                if eligible[index]
                    && function.code.iter().any(|instruction| {
                        matches!(&instruction.op, Op::Call(target, _) if !eligible.get(*target).copied().unwrap_or(false) || native_list_mutator.get(*target).copied().unwrap_or(false))
                    })
                {
                    eligible[index] = false;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
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
                    Err(_) => function_ids[index] = None,
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
            "finalize_nanoseconds": self.finalize_nanoseconds
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
        })
    }
}

impl NativeCode {
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
        let mut output = vec![0_i64; metadata_start.checked_add(4)?];
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
        let stack_types = infer_native_types(function, all_functions, records)
            .ok_or_else(|| "unsupported types or control flow".to_owned())?;
        let depths = stack_depths(function).ok_or_else(|| "invalid stack graph".to_owned())?;
        let (leader_indices, block_for_ip) = bytecode_block_layout(function, &depths)
            .ok_or_else(|| "invalid bytecode block graph".to_owned())?;
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
        let list_set = self
            .module
            .declare_func_in_func(self.list_set, &mut context.func);
        let charge_record = self
            .module
            .declare_func_in_func(self.charge_record, &mut context.func);
        let print = self
            .module
            .declare_func_in_func(self.print, &mut context.func);
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
            for (ip, block_index) in block_for_ip.iter().enumerate() {
                if depths[ip].is_some() {
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
                exit_params.push([steps, source_ip]);
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
            builder.def_var(fuel_var, zero_steps);
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
                let steps_before = builder.use_var(steps_var);
                let current_ip = builder.ins().iconst(types::I64, ip as i64);
                let exit_args = [BlockArg::Value(steps_before), BlockArg::Value(current_ip)];
                if polls_cancellation {
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
                let remaining_fuel = builder.use_var(fuel_var);
                let remaining_fuel = builder.ins().iadd_imm_s(remaining_fuel, -1);
                builder.def_var(fuel_var, remaining_fuel);
                let steps = builder.use_var(steps_var);
                let steps = builder.ins().iadd_imm_s(steps, 1);
                builder.def_var(steps_var, steps);

                match &instruction.op {
                    Op::Const(Value::Int(value)) => {
                        let Ok(value) = value.to_string().parse::<i64>() else {
                            let fault_ip = builder.ins().iconst(types::I64, ip as i64);
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
                        if clone_bytes > 0 {
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
                        let record = stack.pop().ok_or_else(|| "stack underflow".to_owned())?;
                        let descriptor = record;
                        let fields = builder.ins().load(
                            ptr,
                            MemFlagsData::new(),
                            descriptor,
                            std::mem::offset_of!(NativeRecord, fields) as i32,
                        );
                        let field_address = builder
                            .ins()
                            .iadd_imm_s(fields, (*field_index).saturating_mul(8) as i64);
                        let value =
                            builder
                                .ins()
                                .load(types::I64, MemFlagsData::new(), field_address, 0);
                        let field_type = records
                            .get(*record_index)
                            .and_then(|record| record.fields.get(*field_index))
                            .map(|field| &field.ty);
                        let clone_bytes = field_type.map(native_cloned_value_bytes).unwrap_or(0);
                        if clone_bytes > 0 {
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
                    Op::MakeRecord(record_index, field_count) => {
                        let definition = records
                            .get(*record_index)
                            .ok_or_else(|| "record definition is missing".to_owned())?;
                        if definition.fields.len() != *field_count {
                            return Err("record field count mismatch".into());
                        }
                        let charge_bytes = std::mem::size_of::<Vec<Value>>()
                            .saturating_add(
                                field_count.saturating_mul(std::mem::size_of::<Value>()),
                            )
                            .saturating_add(definition.name.len());
                        let charge_bytes = builder
                            .ins()
                            .iconst(types::I64, i64::try_from(charge_bytes).unwrap_or(i64::MAX));
                        let charged = builder
                            .ins()
                            .call(charge_record, &[arena_pointer, charge_bytes]);
                        let charged = builder.inst_results(charged)[0];
                        let zero = builder.ins().iconst(types::I32, 0);
                        let succeeded = builder.ins().icmp(IntCC::NotEqual, charged, zero);
                        let success = builder.create_block();
                        builder
                            .ins()
                            .brif(succeeded, success, &[], fallback, &exit_args);
                        builder.switch_to_block(success);
                        let mut fields = Vec::with_capacity(*field_count);
                        for _ in 0..*field_count {
                            fields.push(stack.pop().ok_or_else(|| "stack underflow".to_owned())?);
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
                        let descriptor_slot = builder.create_sized_stack_slot(StackSlotData::new(
                            StackSlotKind::ExplicitSlot,
                            16,
                            3,
                        ));
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
                        let index = stack.pop().ok_or_else(|| "stack underflow".to_owned())?;
                        let handle = stack.pop().ok_or_else(|| "stack underflow".to_owned())?;
                        let zero = builder.ins().iconst(types::I64, 0);
                        let negative = builder.ins().icmp(IntCC::SignedLessThan, index, zero);
                        let check_upper = builder.create_block();
                        let fault_ip = builder.ins().iconst(types::I64, ip as i64);
                        let fault_steps = builder.use_var(steps_var);
                        let fault_args = [BlockArg::Value(fault_steps), BlockArg::Value(fault_ip)];
                        builder
                            .ins()
                            .brif(negative, fallback, &fault_args, check_upper, &[]);
                        builder.switch_to_block(check_upper);
                        let list = handle;
                        let length = builder.ins().load(
                            types::I64,
                            MemFlagsData::new(),
                            list,
                            std::mem::offset_of!(NativeFloatList, len) as i32,
                        );
                        let in_bounds = builder.ins().icmp(IntCC::UnsignedLessThan, index, length);
                        let load_item = builder.create_block();
                        let fault_ip = builder.ins().iconst(types::I64, ip as i64);
                        let fault_steps = builder.use_var(steps_var);
                        let fault_args = [BlockArg::Value(fault_steps), BlockArg::Value(fault_ip)];
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
                        let value =
                            builder
                                .ins()
                                .load(types::I64, MemFlagsData::new(), item_address, 0);
                        stack.push(value);
                        builder.seal_block(check_upper);
                        builder.seal_block(load_item);
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
                        if stack_types[ip].as_ref().and_then(|types| types.last())
                            == Some(&Type::Float64)
                        {
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
                                steps,
                                ip,
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
                            let ok = builder.create_block();
                            let overflow_ip = builder.ins().iconst(types::I64, ip as i64);
                            let overflow_steps = builder.use_var(steps_var);
                            let exit_args = [
                                BlockArg::Value(overflow_steps),
                                BlockArg::Value(overflow_ip),
                            ];
                            builder.ins().brif(overflow, fallback, &exit_args, ok, &[]);
                            builder.switch_to_block(ok);
                            builder.seal_block(ok);
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
                            let (negated, overflow) = builder.ins().ssub_overflow(zero, value);
                            let ok = builder.create_block();
                            let overflow_ip = builder.ins().iconst(types::I64, ip as i64);
                            let overflow_steps = builder.use_var(steps_var);
                            let exit_args = [
                                BlockArg::Value(overflow_steps),
                                BlockArg::Value(overflow_ip),
                            ];
                            builder.ins().brif(overflow, fallback, &exit_args, ok, &[]);
                            builder.switch_to_block(ok);
                            builder.seal_block(ok);
                            stack.push(negated);
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
                        let float_operands = stack_types[ip].as_ref().is_some_and(|types| {
                            types.len() >= 2 && types[types.len() - 2] == Type::Float64
                        });
                        if float_operands {
                            let left = builder.ins().bitcast(types::F64, MemFlagsData::new(), left);
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
                    Op::Jump(target) => {
                        let block_args: Vec<_> =
                            stack.iter().copied().map(BlockArg::Value).collect();
                        builder
                            .ins()
                            .jump(blocks[block_for_ip[*target]], &block_args);
                        continue;
                    }
                    Op::Call(target, argc) => {
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
                            ((max_result_words + 4) * 8) as u32,
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
                        builder
                            .ins()
                            .call(release_fuel, &[budget_pointer, unused_fuel]);
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
                        let zero_status = builder.ins().iconst(types::I32, 0);
                        let succeeded = builder.ins().icmp(IntCC::Equal, status, zero_status);
                        let success = builder.create_block();
                        let failure = builder.create_block();
                        builder.ins().brif(succeeded, success, &[], failure, &[]);

                        builder.switch_to_block(failure);
                        let unused_fuel = builder.use_var(fuel_var);
                        builder
                            .ins()
                            .call(release_fuel, &[budget_pointer, unused_fuel]);
                        let child_steps =
                            builder
                                .ins()
                                .load(types::I64, MemFlagsData::new(), child_metadata, 8);
                        let parent_steps = builder.use_var(steps_var);
                        let total_steps = builder.ins().iadd(parent_steps, child_steps);
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
                        let parent_steps = builder.use_var(steps_var);
                        let total_steps = builder.ins().iadd(parent_steps, child_steps);
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
                    Op::ListSetLocal(slot) => {
                        let value = stack.pop().ok_or_else(|| "stack underflow".to_owned())?;
                        let list_index = stack.pop().ok_or_else(|| "stack underflow".to_owned())?;
                        let list_handle = builder.use_var(local_vars[*slot]);
                        let item_type = stack_types[ip]
                            .as_ref()
                            .and_then(|types| types.last())
                            .ok_or_else(|| "missing list update item type".to_owned())?;
                        let kind = builder.ins().iconst(
                            types::I64,
                            native_list_kind(item_type)
                                .ok_or_else(|| "unsupported native list item".to_owned())?,
                        );
                        let always_copy = builder.ins().iconst(types::I64, 0);
                        let result = builder.ins().call(
                            list_set,
                            &[
                                arena_pointer,
                                list_handle,
                                list_index,
                                value,
                                kind,
                                always_copy,
                            ],
                        );
                        let updated = builder.inst_results(result)[0];
                        let null = builder.ins().iconst(ptr, 0);
                        let succeeded = builder.ins().icmp(IntCC::NotEqual, updated, null);
                        let success = builder.create_block();
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
                        let kind = builder.ins().iconst(
                            types::I64,
                            native_list_kind(item_type)
                                .ok_or_else(|| "unsupported native list item".to_owned())?,
                        );
                        let always_copy = builder.ins().iconst(types::I64, 1);
                        let result = builder.ins().call(
                            list_set,
                            &[arena_pointer, list, list_index, value, kind, always_copy],
                        );
                        let updated = builder.inst_results(result)[0];
                        let null = builder.ins().iconst(ptr, 0);
                        let succeeded = builder.ins().icmp(IntCC::NotEqual, updated, null);
                        let success = builder.create_block();
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
                            let fault_ip = builder.ins().iconst(types::I64, ip as i64);
                            let fault_steps = builder.use_var(steps_var);
                            let exit_args =
                                [BlockArg::Value(fault_steps), BlockArg::Value(fault_ip)];
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
                                steps,
                                ip,
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
                            let fault_ip = builder.ins().iconst(types::I64, ip as i64);
                            let fault_steps = builder.use_var(steps_var);
                            let exit_args =
                                [BlockArg::Value(fault_steps), BlockArg::Value(fault_ip)];
                            builder.ins().brif(
                                is_zero,
                                divide_zero,
                                &exit_args,
                                continue_division,
                                &[],
                            );
                            builder.switch_to_block(continue_division);
                            builder.seal_block(continue_division);
                            let min = builder.ins().iconst(types::I64, i64::MIN);
                            let minus_one = builder.ins().iconst(types::I64, -1);
                            let is_min = builder.ins().icmp(IntCC::Equal, left, min);
                            let is_minus_one = builder.ins().icmp(IntCC::Equal, right, minus_one);
                            let overflow = builder.ins().band(is_min, is_minus_one);
                            let safe_division = builder.create_block();
                            let fault_ip = builder.ins().iconst(types::I64, ip as i64);
                            let fault_steps = builder.use_var(steps_var);
                            let exit_args =
                                [BlockArg::Value(fault_steps), BlockArg::Value(fault_ip)];
                            builder
                                .ins()
                                .brif(overflow, fallback, &exit_args, safe_division, &[]);
                            builder.switch_to_block(safe_division);
                            builder.seal_block(safe_division);
                            stack.push(builder.ins().sdiv(left, right));
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
                        let fault_ip = builder.ins().iconst(types::I64, ip as i64);
                        let fault_steps = builder.use_var(steps_var);
                        let exit_args = [BlockArg::Value(fault_steps), BlockArg::Value(fault_ip)];
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
                        emit_float_finite_guard(&mut builder, result, float_nonfinite, steps, ip);
                        stack.push(
                            builder
                                .ins()
                                .bitcast(types::I64, MemFlagsData::new(), result),
                        );
                    }
                    Op::IntegerDivide | Op::FloorDivide | Op::Remainder => {
                        let right = stack.pop().ok_or_else(|| "stack underflow".to_owned())?;
                        let left = stack.pop().ok_or_else(|| "stack underflow".to_owned())?;
                        let zero = builder.ins().iconst(types::I64, 0);
                        let is_zero = builder.ins().icmp(IntCC::Equal, right, zero);
                        let nonzero = builder.create_block();
                        let fault_ip = builder.ins().iconst(types::I64, ip as i64);
                        let fault_steps = builder.use_var(steps_var);
                        let exit_args = [BlockArg::Value(fault_steps), BlockArg::Value(fault_ip)];
                        builder
                            .ins()
                            .brif(is_zero, divide_zero, &exit_args, nonzero, &[]);
                        builder.switch_to_block(nonzero);
                        builder.seal_block(nonzero);
                        let min = builder.ins().iconst(types::I64, i64::MIN);
                        let minus_one = builder.ins().iconst(types::I64, -1);
                        let is_min = builder.ins().icmp(IntCC::Equal, left, min);
                        let is_minus_one = builder.ins().icmp(IntCC::Equal, right, minus_one);
                        let overflow = builder.ins().band(is_min, is_minus_one);
                        let one = builder.ins().iconst(types::I64, 1);
                        let safe_right = builder.ins().select(overflow, one, right);
                        if !matches!(instruction.op, Op::Remainder) {
                            let continue_division = builder.create_block();
                            let fault_steps = builder.use_var(steps_var);
                            let fault_ip = builder.ins().iconst(types::I64, ip as i64);
                            let exit_args =
                                [BlockArg::Value(fault_steps), BlockArg::Value(fault_ip)];
                            builder.ins().brif(
                                overflow,
                                fallback,
                                &exit_args,
                                continue_division,
                                &[],
                            );
                            builder.switch_to_block(continue_division);
                            builder.seal_block(continue_division);
                        }
                        let quotient = builder.ins().sdiv(left, safe_right);
                        let truncating = builder.ins().srem(left, safe_right);
                        let truncating = builder.ins().select(overflow, zero, truncating);
                        let left_negative = builder.ins().icmp(IntCC::SignedLessThan, left, zero);
                        let right_negative = builder.ins().icmp(IntCC::SignedLessThan, right, zero);
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
                        builder
                            .ins()
                            .brif(succeeded, success, &[], fallback, &exit_args);
                        builder.switch_to_block(success);
                        builder.seal_block(success);
                    }
                    Op::Return => {
                        let value = stack.pop().ok_or_else(|| "stack underflow".to_owned())?;
                        if let Type::Record(name) = &function.result {
                            let definition = records
                                .iter()
                                .find(|record| record.name == *name)
                                .ok_or_else(|| "record result definition is missing".to_owned())?;
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
                        emit_success(
                            &mut builder,
                            output_pointer,
                            steps,
                            release_fuel,
                            budget_pointer,
                            unused_fuel,
                            calls,
                        );
                        continue;
                    }
                    _ => return Err("unsupported instruction".into()),
                }
                if ip + 1 >= function.code.len() {
                    return Err("missing function terminator".into());
                }
                if let Some(next_block) = leader_indices[ip + 1] {
                    let block_args: Vec<_> = stack.iter().copied().map(BlockArg::Value).collect();
                    builder.ins().jump(blocks[next_block], &block_args);
                }
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
                let unused_fuel = builder.use_var(fuel_var);
                builder
                    .ins()
                    .call(release_fuel, &[budget_pointer, unused_fuel]);
                let calls = builder.use_var(native_calls_var);
                emit_exit(
                    &mut builder,
                    output_pointer,
                    status,
                    params[0],
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

fn emit_float_finite_guard(
    builder: &mut FunctionBuilder<'_>,
    value: cranelift_codegen::ir::Value,
    failure: cranelift_codegen::ir::Block,
    steps: cranelift_codegen::ir::Value,
    ip: usize,
) {
    let max = builder.ins().f64const(f64::MAX);
    let min = builder.ins().f64const(-f64::MAX);
    let too_large = builder
        .ins()
        .fcmp(FloatCC::UnorderedOrGreaterThan, value, max);
    let too_small = builder.ins().fcmp(FloatCC::UnorderedOrLessThan, value, min);
    let invalid = builder.ins().bor(too_large, too_small);
    let okay = builder.create_block();
    let fault_ip = builder.ins().iconst(types::I64, ip as i64);
    let exit_args = [BlockArg::Value(steps), BlockArg::Value(fault_ip)];
    builder.ins().brif(invalid, failure, &exit_args, okay, &[]);
    builder.switch_to_block(okay);
    builder.seal_block(okay);
}

fn emit_success(
    builder: &mut FunctionBuilder<'_>,
    output: cranelift_codegen::ir::Value,
    steps: cranelift_codegen::ir::Value,
    release_fuel: cranelift_codegen::ir::FuncRef,
    budget: cranelift_codegen::ir::Value,
    unused_fuel: cranelift_codegen::ir::Value,
    calls: cranelift_codegen::ir::Value,
) {
    builder.ins().call(release_fuel, &[budget, unused_fuel]);
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
) -> Option<Vec<Option<Vec<Type>>>> {
    if let Some(first_update) = function
        .code
        .iter()
        .position(|instruction| matches!(instruction.op, Op::ListSetLocal(_)))
    {
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
    let is_native_parameter = |ty: &Type| {
        is_native_item(ty)
            || matches!(ty, Type::List(item) if is_native_item(item))
            || matches!(ty, Type::Record(name) if records.iter().find(|record| record.name == *name).is_some_and(|record| record.fields.iter().all(|field| is_native_item(&field.ty) || matches!(&field.ty, Type::List(item) if is_native_item(item)))))
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
    let mut stack_types = vec![None; function.code.len()];
    states[0] = Some(initial);
    let mut pending = VecDeque::from([0]);
    while let Some(ip) = pending.pop_front() {
        let mut state = states[ip].clone()?;
        stack_types[ip] = Some(state.stack.clone());
        let op = &function.code[ip].op;
        match op {
            Op::Const(Value::Int(value)) => {
                value.to_string().parse::<i64>().ok()?;
                state.stack.push(Type::Int);
            }
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
                if left != right || !is_numeric(&left) {
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
            Op::Length => {
                if !matches!(state.stack.pop()?, Type::List(item) if is_native_item(&item)) {
                    return None;
                }
                state.stack.push(Type::Int);
            }
            Op::MakeList(count) => {
                let start = state.stack.len().checked_sub(*count)?;
                let item_type = state.stack.get(start).cloned()?;
                if !is_native_item(&item_type)
                    || state.stack[start..].iter().any(|ty| *ty != item_type)
                {
                    return None;
                }
                state.stack.truncate(start);
                state.stack.push(Type::List(Box::new(item_type)));
            }
            Op::MakeTypedList(item_type) => {
                if !is_native_item(item_type) {
                    return None;
                }
                state.stack.push(Type::List(Box::new(item_type.clone())));
            }
            Op::Index => {
                if state.stack.pop()? != Type::Int {
                    return None;
                }
                let Type::List(item) = state.stack.pop()? else {
                    return None;
                };
                if !is_native_item(&item) {
                    return None;
                }
                state.stack.push(*item);
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
                if left != right || !(is_numeric(&left) || left == Type::Bool) {
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
                if matches!(op, Op::PrintErr) {
                    return None;
                }
                if !matches!(state.stack.pop()?, Type::Int | Type::Float64 | Type::Bool) {
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
            Op::ListPushLocal(_) | Op::JsonPushLocal(_) => return None,
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
    Some(stack_types)
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
            definition
                .fields
                .iter()
                .all(|field| supported_field(&field.ty))
                .then(|| {
                    definition
                        .fields
                        .iter()
                        .map(|field| field.ty.clone())
                        .collect()
                })
        }
        _ => None,
    }
}

fn native_cloned_value_bytes(ty: &Type) -> usize {
    match ty {
        Type::Record(name) => std::mem::size_of::<String>().saturating_add(name.len()),
        _ => 0,
    }
}

fn native_list_kind(item_type: &Type) -> Option<i64> {
    match item_type {
        Type::Float64 => Some(1),
        Type::Int => Some(2),
        Type::Bool => Some(3),
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
            Op::Const(_) | Op::Load(_) => (0, 1),
            Op::Store(_) | Op::Pop | Op::JumpIfFalse(_) | Op::Print | Op::PrintErr | Op::Return => {
                (1, 0)
            }
            Op::Negate | Op::Not | Op::FloatSqrt | Op::TextChars => (1, 1),
            Op::Length | Op::GetField(_, _) => (1, 1),
            Op::Index => (2, 1),
            Op::MakeRecord(_, count) => (*count, 1),
            Op::MakeList(count) => (*count, 1),
            Op::MakeTypedList(_) => (0, 1),
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
