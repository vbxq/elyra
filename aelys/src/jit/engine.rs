use super::ir::{
    BlockId, FunctionIr, IntPredicate, IrBlock, IrError, IrInstructionKind, IrTerminator, IrType,
    ValueId,
};
use aelys_runtime::{JitArgument, JitExecutionContext};
use cranelift_codegen::ir::{
    AbiParam, BlockArg, InstBuilder, MemFlagsData, StackSlotData, StackSlotKind, UserFuncName,
    immediates::Ieee64, types,
};
use cranelift_codegen::isa::OwnedTargetIsa;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{Linkage, Module};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::sync::{Arc, Mutex};

pub(crate) const JIT_ABI_VERSION: u16 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub(crate) enum JitTier {
    Baseline,
    Optimized,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct JitKey {
    pub(crate) module: u64,
    pub(crate) function_path: Arc<[u32]>,
    pub(crate) tier: JitTier,
    pub(crate) osr_ip: Option<u32>,
    pub(crate) abi: u16,
    pub(crate) cpu_features: u64,
    pub(crate) controlled: bool,
}

impl JitKey {
    #[cfg(test)]
    pub(crate) fn new(module: u64, function: u32, tier: JitTier) -> Self {
        Self::for_path(module, Arc::from([function]), tier)
    }

    pub(crate) fn for_path(module: u64, function_path: Arc<[u32]>, tier: JitTier) -> Self {
        Self::for_path_with_control(module, function_path, tier, false)
    }

    pub(crate) fn for_path_with_control(
        module: u64,
        function_path: Arc<[u32]>,
        tier: JitTier,
        controlled: bool,
    ) -> Self {
        Self {
            module,
            function_path,
            tier,
            osr_ip: None,
            abi: JIT_ABI_VERSION,
            cpu_features: cpu_feature_key(),
            controlled,
        }
    }

    #[cfg(test)]
    pub(crate) fn for_osr(module: u64, function_path: Arc<[u32]>, bytecode_ip: u32) -> Self {
        Self::for_osr_with_control(module, function_path, bytecode_ip, false)
    }

    pub(crate) fn for_osr_with_control(
        module: u64,
        function_path: Arc<[u32]>,
        bytecode_ip: u32,
        controlled: bool,
    ) -> Self {
        let mut key =
            Self::for_path_with_control(module, function_path, JitTier::Baseline, controlled);
        key.osr_ip = Some(bytecode_ip);
        key
    }
}

pub(crate) struct CompiledFunction {
    address: usize,
    arity: usize,
    parameter_types: Arc<[IrType]>,
    return_type: IrType,
    deopt_register_count: usize,
    deopt_maps: HashMap<u32, Arc<[(u16, DeoptSource)]>>,
    _module: Mutex<JITModule>,
}

#[repr(C)]
#[derive(Default)]
struct RawJitExit {
    kind: u64,
    bytecode_ip: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct RawI64Collection {
    data: *const i64,
    length: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum JitExecution {
    Returned(i64),
    Aborted,
    Deoptimized {
        bytecode_ip: u32,
        registers: Vec<(u16, JitDeoptValue)>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum JitDeoptValue {
    Integer(i64),
    Float(u64),
    Boolean(bool),
    Argument(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DeoptSource {
    Machine(u16, IrType),
    Argument(usize),
}

impl fmt::Debug for CompiledFunction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CompiledFunction")
            .field("address", &self.address)
            .field("arity", &self.arity)
            .field("deopt_register_count", &self.deopt_register_count)
            .finish_non_exhaustive()
    }
}

impl CompiledFunction {
    pub(crate) fn arity(&self) -> usize {
        self.arity
    }

    pub(crate) fn return_type(&self) -> IrType {
        self.return_type
    }

    #[cfg(test)]
    pub(crate) fn execute_i64(&self, arguments: &[i64]) -> Result<i64, JitError> {
        match self.execute(arguments)? {
            JitExecution::Returned(value) => Ok(value),
            JitExecution::Aborted => Err(JitError::Aborted),
            JitExecution::Deoptimized { bytecode_ip, .. } => {
                Err(JitError::Deoptimized(bytecode_ip))
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn execute(&self, arguments: &[i64]) -> Result<JitExecution, JitError> {
        if arguments.len() != self.arity {
            return Err(JitError::Arity {
                expected: self.arity,
                actual: arguments.len(),
            });
        }
        if self.parameter_types.iter().any(|ty| *ty != IrType::I64) {
            return Err(JitError::ArgumentType(0));
        }
        self.execute_raw(arguments.as_ptr(), std::ptr::null(), std::ptr::null_mut())
    }

    #[cfg(test)]
    pub(crate) fn execute_arguments(
        &self,
        arguments: &[JitArgument<'_>],
    ) -> Result<JitExecution, JitError> {
        self.execute_arguments_with_context(arguments, None)
    }

    pub(crate) fn execute_arguments_with_context(
        &self,
        arguments: &[JitArgument<'_>],
        context: Option<&JitExecutionContext>,
    ) -> Result<JitExecution, JitError> {
        if arguments.len() != self.arity {
            return Err(JitError::Arity {
                expected: self.arity,
                actual: arguments.len(),
            });
        }
        let mut integers = Vec::with_capacity(arguments.len());
        let mut collections = Vec::with_capacity(arguments.len());
        for (index, (argument, expected)) in arguments
            .iter()
            .zip(self.parameter_types.iter())
            .enumerate()
        {
            match (argument, expected) {
                (JitArgument::Integer(value), IrType::I64) => {
                    integers.push(*value);
                    collections.push(RawI64Collection {
                        data: std::ptr::null(),
                        length: 0,
                    });
                }
                (JitArgument::Boolean(value), IrType::Bool) => {
                    integers.push(i64::from(*value));
                    collections.push(RawI64Collection {
                        data: std::ptr::null(),
                        length: 0,
                    });
                }
                (JitArgument::Float(value), IrType::F64) => {
                    integers.push(i64::from_ne_bytes(value.to_bits().to_ne_bytes()));
                    collections.push(RawI64Collection {
                        data: std::ptr::null(),
                        length: 0,
                    });
                }
                (JitArgument::IntegerArray(elements), IrType::I64Array) => {
                    integers.push(0);
                    collections.push(RawI64Collection {
                        data: elements.as_ptr(),
                        length: u64::try_from(elements.len())
                            .map_err(|_| JitError::OffsetOverflow)?,
                    });
                }
                (JitArgument::IntegerVec(elements), IrType::I64Vec) => {
                    integers.push(0);
                    collections.push(RawI64Collection {
                        data: elements.as_ptr(),
                        length: u64::try_from(elements.len())
                            .map_err(|_| JitError::OffsetOverflow)?,
                    });
                }
                (_, IrType::Uninitialized) => {
                    integers.push(0);
                    collections.push(RawI64Collection {
                        data: std::ptr::null(),
                        length: 0,
                    });
                }
                _ => return Err(JitError::ArgumentType(index)),
            }
        }
        let context = context
            .map(|context| (context as *const JitExecutionContext).cast_mut())
            .unwrap_or(std::ptr::null_mut());
        self.execute_raw(integers.as_ptr(), collections.as_ptr(), context)
    }

    fn execute_raw(
        &self,
        integers: *const i64,
        collections: *const RawI64Collection,
        context: *mut JitExecutionContext,
    ) -> Result<JitExecution, JitError> {
        type Entry = unsafe extern "C" fn(
            *const i64,
            *mut RawJitExit,
            *mut i64,
            *const RawI64Collection,
            *mut JitExecutionContext,
        ) -> i64;
        // safety: addresses are obtained from finalized Cranelift functions with this exact ABI.
        let entry = unsafe { std::mem::transmute::<usize, Entry>(self.address) };
        let mut exit = RawJitExit::default();
        let mut deopt_registers = vec![0; self.deopt_register_count];
        // safety: Cranelift receives valid argument, exit-state and deoptimization buffers for the compiled ABI.
        let result = unsafe {
            entry(
                integers,
                &mut exit,
                deopt_registers.as_mut_ptr(),
                collections,
                context,
            )
        };
        if exit.kind == 0 {
            return Ok(JitExecution::Returned(result));
        }
        if exit.kind == 2 || exit.kind == 3 {
            return Ok(JitExecution::Aborted);
        }
        if exit.kind != 1 {
            return Err(JitError::InvalidExitKind(exit.kind));
        }
        let bytecode_ip =
            u32::try_from(exit.bytecode_ip).map_err(|_| JitError::InvalidExitKind(exit.kind))?;
        let register_sources = self
            .deopt_maps
            .get(&bytecode_ip)
            .ok_or(JitError::UnknownDeoptExit(bytecode_ip))?;
        let registers = register_sources
            .iter()
            .map(|(register, source)| {
                let value = match source {
                    DeoptSource::Machine(slot, ty) => match ty {
                        IrType::F64 => JitDeoptValue::Float(u64::from_ne_bytes(
                            deopt_registers[usize::from(*slot)].to_ne_bytes(),
                        )),
                        IrType::Bool => {
                            JitDeoptValue::Boolean(deopt_registers[usize::from(*slot)] != 0)
                        }
                        _ => JitDeoptValue::Integer(deopt_registers[usize::from(*slot)]),
                    },
                    DeoptSource::Argument(index) => JitDeoptValue::Argument(*index),
                };
                (*register, value)
            })
            .collect();
        Ok(JitExecution::Deoptimized {
            bytecode_ip,
            registers,
        })
    }
}

pub(crate) struct JitEngine {
    baseline_isa: OwnedTargetIsa,
    optimized_isa: OwnedTargetIsa,
    state: Mutex<EngineState>,
}

struct EngineState {
    entries: HashMap<JitKey, Arc<CompiledFunction>>,
    lru: VecDeque<JitKey>,
    max_entries: usize,
    next_symbol: u64,
}

impl JitEngine {
    pub(crate) fn new(max_entries: usize) -> Result<Self, JitError> {
        if max_entries == 0 {
            return Err(JitError::InvalidCacheLimit);
        }
        let baseline_isa = native_isa("none")?;
        let optimized_isa = native_isa("speed")?;
        Ok(Self {
            baseline_isa,
            optimized_isa,
            state: Mutex::new(EngineState {
                entries: HashMap::new(),
                lru: VecDeque::new(),
                max_entries,
                next_symbol: 0,
            }),
        })
    }

    pub(crate) fn compile(
        &self,
        key: &JitKey,
        ir: &FunctionIr,
    ) -> Result<Arc<CompiledFunction>, JitError> {
        ir.verify().map_err(JitError::InvalidIr)?;
        if !matches!(ir.return_type, IrType::I64 | IrType::F64 | IrType::Bool) {
            return Err(JitError::UnsupportedIr("unsupported return type"));
        }
        let mut state = self.state.lock().map_err(|_| JitError::Poisoned)?;
        if let Some(entry) = state.entries.get(key).cloned() {
            touch_lru(&mut state.lru, key.clone());
            return Ok(entry);
        }

        let symbol = format!("aelys_jit_{}", state.next_symbol);
        state.next_symbol = state.next_symbol.wrapping_add(1);
        let isa = match key.tier {
            JitTier::Baseline => &self.baseline_isa,
            JitTier::Optimized => &self.optimized_isa,
        };
        let mut jit_builder =
            JITBuilder::with_isa(Arc::clone(isa), cranelift_module::default_libcall_names());
        jit_builder.symbol("aelys_jit_poll", jit_poll as *const u8);
        jit_builder.symbol(
            "aelys_jit_call_global",
            aelys_runtime::call_jit_global as *const u8,
        );
        let mut module = JITModule::new(jit_builder);
        let pointer_type = module.target_config().pointer_type();
        let frontend_config = module.target_config();
        let mut context = module.make_context();
        context
            .func
            .signature
            .params
            .push(AbiParam::new(pointer_type));
        context
            .func
            .signature
            .params
            .push(AbiParam::new(pointer_type));
        context
            .func
            .signature
            .params
            .push(AbiParam::new(pointer_type));
        context
            .func
            .signature
            .params
            .push(AbiParam::new(pointer_type));
        context
            .func
            .signature
            .params
            .push(AbiParam::new(pointer_type));
        context
            .func
            .signature
            .returns
            .push(AbiParam::new(types::I64));
        context.func.name = UserFuncName::user(0, u32::try_from(state.next_symbol).unwrap_or(0));

        let function_id = module
            .declare_function(&symbol, Linkage::Local, &context.func.signature)
            .map_err(|error| JitError::Module(error.to_string()))?;
        let poll_function = if key.controlled {
            let mut signature = module.make_signature();
            signature.params.push(AbiParam::new(pointer_type));
            signature.returns.push(AbiParam::new(types::I64));
            let poll_id = module
                .declare_function("aelys_jit_poll", Linkage::Import, &signature)
                .map_err(|error| JitError::Module(error.to_string()))?;
            Some(module.declare_func_in_func(poll_id, &mut context.func))
        } else {
            None
        };
        let native_call_function = if ir.blocks.iter().any(|block| {
            block
                .instructions
                .iter()
                .any(|instruction| matches!(instruction.kind, IrInstructionKind::NativeCall { .. }))
        }) {
            let mut signature = module.make_signature();
            signature.params.push(AbiParam::new(pointer_type));
            signature.params.push(AbiParam::new(pointer_type));
            signature.params.push(AbiParam::new(types::I64));
            signature.params.push(AbiParam::new(pointer_type));
            signature.params.push(AbiParam::new(types::I64));
            signature.params.push(AbiParam::new(types::I64));
            signature.params.push(AbiParam::new(types::I64));
            signature.returns.push(AbiParam::new(types::I64));
            let call_id = module
                .declare_function("aelys_jit_call_global", Linkage::Import, &signature)
                .map_err(|error| JitError::Module(error.to_string()))?;
            Some(module.declare_func_in_func(call_id, &mut context.func))
        } else {
            None
        };
        lower_function(
            ir,
            &mut context.func,
            frontend_config,
            poll_function,
            native_call_function,
        )?;
        module
            .define_function(function_id, &mut context)
            .map_err(|error| JitError::Module(format!("{error:?}")))?;
        module.clear_context(&mut context);
        module
            .finalize_definitions()
            .map_err(|error| JitError::Module(error.to_string()))?;
        let address = module.get_finalized_function(function_id) as usize;
        let deopt_register_count = ir
            .deopt_maps
            .iter()
            .flat_map(|map| {
                map.registers
                    .iter()
                    .map(|(register, _)| usize::from(*register))
            })
            .max()
            .map_or(0, |register| register.saturating_add(1));
        let entry = ir
            .blocks
            .iter()
            .find(|block| block.id == ir.entry)
            .ok_or(JitError::InvalidIr(IrError::InvalidBlockGraph))?;
        let array_parameters = collection_origins(ir, entry);
        let value_types = ir
            .blocks
            .iter()
            .flat_map(|block| {
                block.parameters.iter().copied().chain(
                    block
                        .instructions
                        .iter()
                        .filter_map(|instruction| instruction.result),
                )
            })
            .collect::<HashMap<_, _>>();
        let mut deopt_maps = HashMap::new();
        for map in &ir.deopt_maps {
            let mut sources = Vec::with_capacity(map.registers.len());
            for &(register, value) in &map.registers {
                let source = if value_types
                    .get(&value)
                    .is_some_and(|ty| is_collection_type(*ty))
                {
                    DeoptSource::Argument(array_parameters.get(&value).copied().ok_or(
                        JitError::UnsupportedIr(
                            "array deoptimization source is not an entry argument",
                        ),
                    )?)
                } else {
                    DeoptSource::Machine(
                        register,
                        value_types
                            .get(&value)
                            .copied()
                            .ok_or(JitError::InvalidIr(IrError::UnknownValue(value)))?,
                    )
                };
                sources.push((register, source));
            }
            deopt_maps.insert(map.bytecode_ip, Arc::from(sources));
        }
        let entry = Arc::new(CompiledFunction {
            address,
            arity: ir.parameter_types.len(),
            parameter_types: Arc::from(ir.parameter_types.clone()),
            return_type: ir.return_type,
            deopt_register_count,
            deopt_maps,
            _module: Mutex::new(module),
        });
        state.entries.insert(key.clone(), Arc::clone(&entry));
        touch_lru(&mut state.lru, key.clone());
        while state.entries.len() > state.max_entries {
            if let Some(evicted) = state.lru.pop_front() {
                state.entries.remove(&evicted);
            }
        }
        Ok(entry)
    }

    pub(crate) fn cached(&self, key: &JitKey) -> Result<Option<Arc<CompiledFunction>>, JitError> {
        let mut state = self.state.lock().map_err(|_| JitError::Poisoned)?;
        let entry = state.entries.get(key).cloned();
        if entry.is_some() {
            touch_lru(&mut state.lru, key.clone());
        }
        Ok(entry)
    }

    pub(crate) fn cache_len(&self) -> Result<usize, JitError> {
        self.state
            .lock()
            .map(|state| state.entries.len())
            .map_err(|_| JitError::Poisoned)
    }
}

fn native_isa(opt_level: &str) -> Result<OwnedTargetIsa, JitError> {
    let mut flags = settings::builder();
    flags
        .set("use_colocated_libcalls", "false")
        .map_err(|error| JitError::Configuration(error.to_string()))?;
    flags
        .set("is_pic", "false")
        .map_err(|error| JitError::Configuration(error.to_string()))?;
    flags
        .set("opt_level", opt_level)
        .map_err(|error| JitError::Configuration(error.to_string()))?;
    cranelift_native::builder()
        .map_err(|error| JitError::UnsupportedTarget(error.to_string()))?
        .finish(settings::Flags::new(flags))
        .map_err(|error| JitError::Configuration(error.to_string()))
}

extern "C" fn jit_poll(context: *mut JitExecutionContext) -> i64 {
    if context.is_null() {
        return 0;
    }
    // safety: controlled JIT calls pass a live context for the duration of the synchronous machine-code invocation.
    let context = unsafe { &*context };
    unsafe { (context.poll)(context.data) }
}

fn lower_function(
    ir: &FunctionIr,
    function: &mut cranelift_codegen::ir::Function,
    frontend_config: cranelift_codegen::isa::TargetFrontendConfig,
    poll_function: Option<cranelift_codegen::ir::FuncRef>,
    native_call_function: Option<cranelift_codegen::ir::FuncRef>,
) -> Result<(), JitError> {
    let mut builder_context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(function, &mut builder_context);
    let blocks = ir
        .blocks
        .iter()
        .map(|block| (block.id, builder.create_block()))
        .collect::<HashMap<_, _>>();
    let entry = blocks[&ir.entry];
    let integer_overflow = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    let exit_state = builder.block_params(entry)[1];
    let deopt_registers = builder.block_params(entry)[2];
    let collections = builder.block_params(entry)[3];
    let control = builder.block_params(entry)[4];
    let value_types = ir
        .blocks
        .iter()
        .flat_map(|block| {
            block.parameters.iter().copied().chain(
                block
                    .instructions
                    .iter()
                    .filter_map(|instruction| instruction.result),
            )
        })
        .collect::<HashMap<_, _>>();
    let mut values = HashMap::new();
    for block in &ir.blocks {
        let lowered = blocks[&block.id];
        if block.id != ir.entry {
            for &(value, ty) in &block.parameters {
                let lowered_value = builder.append_block_param(lowered, lower_type(ty));
                values.insert(value, lowered_value);
            }
        }
    }

    for block in &ir.blocks {
        let lowered = blocks[&block.id];
        builder.switch_to_block(lowered);
        if block.id == ir.entry {
            let arguments = builder.block_params(entry)[0];
            for (index, &(value, ty)) in block.parameters.iter().enumerate() {
                let (base, stride) = if is_collection_type(ty) {
                    (collections, 16usize)
                } else {
                    (arguments, 8usize)
                };
                let offset =
                    i32::try_from(index.checked_mul(stride).ok_or(JitError::OffsetOverflow)?)
                        .map_err(|_| JitError::OffsetOverflow)?;
                let loaded = if is_collection_type(ty) {
                    builder.ins().iadd_imm_s(base, i64::from(offset))
                } else {
                    builder
                        .ins()
                        .load(lower_type(ty), MemFlagsData::trusted(), base, offset)
                };
                values.insert(value, loaded);
            }
        }
        for instruction in &block.instructions {
            let result = match instruction.kind {
                IrInstructionKind::Iconst(value) => {
                    let ty = instruction
                        .result
                        .map(|(_, ty)| ty)
                        .ok_or(JitError::InvalidIr(IrError::UnexpectedResult))?;
                    Some(match ty {
                        IrType::F64 => builder
                            .ins()
                            .f64const(Ieee64::with_bits(u64::from_ne_bytes(value.to_ne_bytes()))),
                        _ => builder.ins().iconst(types::I64, value),
                    })
                }
                IrInstructionKind::Bconst(value) => {
                    Some(builder.ins().iconst(types::I8, i64::from(value)))
                }
                IrInstructionKind::Iadd(left, right) => {
                    if value_types.get(&left) == Some(&IrType::F64) {
                        Some(builder.ins().fadd(values[&left], values[&right]))
                    } else {
                        let result = builder.ins().iadd(values[&left], values[&right]);
                        Some(check_integer_result(
                            &mut builder,
                            result,
                            None,
                            integer_overflow,
                        ))
                    }
                }
                IrInstructionKind::Isub(left, right) => {
                    if value_types.get(&left) == Some(&IrType::F64) {
                        Some(builder.ins().fsub(values[&left], values[&right]))
                    } else {
                        let result = builder.ins().isub(values[&left], values[&right]);
                        Some(check_integer_result(
                            &mut builder,
                            result,
                            None,
                            integer_overflow,
                        ))
                    }
                }
                IrInstructionKind::Imul(left, right) => {
                    if value_types.get(&left) == Some(&IrType::F64) {
                        Some(builder.ins().fmul(values[&left], values[&right]))
                    } else {
                        let left = values[&left];
                        let right = values[&right];
                        let result = builder.ins().imul(left, right);
                        let high = builder.ins().smulhi(left, right);
                        let sign = builder.ins().sshr_imm_u(result, 63);
                        let overflow = builder.ins().icmp(
                            cranelift_codegen::ir::condcodes::IntCC::NotEqual,
                            high,
                            sign,
                        );
                        Some(check_integer_result(
                            &mut builder,
                            result,
                            Some(overflow),
                            integer_overflow,
                        ))
                    }
                }
                IrInstructionKind::Fdiv(left, right) => {
                    Some(builder.ins().fdiv(values[&left], values[&right]))
                }
                IrInstructionKind::Fneg(value) => Some(builder.ins().fneg(values[&value])),
                IrInstructionKind::Icmp {
                    predicate,
                    left,
                    right,
                } => {
                    if value_types.get(&left) == Some(&IrType::F64) {
                        Some(builder.ins().fcmp(
                            lower_float_predicate(predicate),
                            values[&left],
                            values[&right],
                        ))
                    } else {
                        Some(builder.ins().icmp(
                            lower_predicate(predicate),
                            values[&left],
                            values[&right],
                        ))
                    }
                }
                IrInstructionKind::Guard { condition, deopt } => {
                    let map = ir
                        .deopt_maps
                        .iter()
                        .find(|map| map.bytecode_ip == deopt)
                        .ok_or(JitError::InvalidIr(IrError::UnknownDeoptMap(deopt)))?;
                    lower_guard(
                        &mut builder,
                        values[&condition],
                        map,
                        &values,
                        exit_state,
                        deopt_registers,
                    )?;
                    None
                }
                IrInstructionKind::BoundsCheck {
                    index,
                    length,
                    deopt,
                } => {
                    use cranelift_codegen::ir::condcodes::IntCC;
                    let zero = builder.ins().iconst(types::I64, 0);
                    let non_negative =
                        builder
                            .ins()
                            .icmp(IntCC::SignedGreaterThanOrEqual, values[&index], zero);
                    let below_length =
                        builder
                            .ins()
                            .icmp(IntCC::SignedLessThan, values[&index], values[&length]);
                    let in_bounds = builder.ins().band(non_negative, below_length);
                    let map = ir
                        .deopt_maps
                        .iter()
                        .find(|map| map.bytecode_ip == deopt)
                        .ok_or(JitError::InvalidIr(IrError::UnknownDeoptMap(deopt)))?;
                    lower_guard(
                        &mut builder,
                        in_bounds,
                        map,
                        &values,
                        exit_state,
                        deopt_registers,
                    )?;
                    None
                }
                IrInstructionKind::ArrayLen(array) | IrInstructionKind::VecLen(array) => Some(
                    builder
                        .ins()
                        .load(types::I64, MemFlagsData::trusted(), values[&array], 8),
                ),
                IrInstructionKind::ArrayLoadI {
                    array,
                    index,
                    deopt,
                }
                | IrInstructionKind::VecLoadI {
                    vector: array,
                    index,
                    deopt,
                } => {
                    use cranelift_codegen::ir::condcodes::IntCC;
                    let descriptor = values[&array];
                    let index = values[&index];
                    let length =
                        builder
                            .ins()
                            .load(types::I64, MemFlagsData::trusted(), descriptor, 8);
                    let zero = builder.ins().iconst(types::I64, 0);
                    let non_negative =
                        builder
                            .ins()
                            .icmp(IntCC::SignedGreaterThanOrEqual, index, zero);
                    let below_length = builder.ins().icmp(IntCC::SignedLessThan, index, length);
                    let in_bounds = builder.ins().band(non_negative, below_length);
                    let map = ir
                        .deopt_maps
                        .iter()
                        .find(|map| map.bytecode_ip == deopt)
                        .ok_or(JitError::InvalidIr(IrError::UnknownDeoptMap(deopt)))?;
                    lower_guard(
                        &mut builder,
                        in_bounds,
                        map,
                        &values,
                        exit_state,
                        deopt_registers,
                    )?;
                    let data =
                        builder
                            .ins()
                            .load(types::I64, MemFlagsData::trusted(), descriptor, 0);
                    let byte_offset = builder.ins().ishl_imm_u(index, 3);
                    let address = builder.ins().iadd(data, byte_offset);
                    Some(
                        builder
                            .ins()
                            .load(types::I64, MemFlagsData::trusted(), address, 0),
                    )
                }
                IrInstructionKind::ArrayLoadIUnchecked { array, index }
                | IrInstructionKind::VecLoadIUnchecked {
                    vector: array,
                    index,
                } => {
                    let descriptor = values[&array];
                    let index = values[&index];
                    let data =
                        builder
                            .ins()
                            .load(types::I64, MemFlagsData::trusted(), descriptor, 0);
                    let byte_offset = builder.ins().ishl_imm_u(index, 3);
                    let address = builder.ins().iadd(data, byte_offset);
                    Some(
                        builder
                            .ins()
                            .load(types::I64, MemFlagsData::trusted(), address, 0),
                    )
                }
                IrInstructionKind::NativeCall {
                    global_index,
                    ref arguments,
                } => {
                    let native_call_function = native_call_function.ok_or(
                        JitError::UnsupportedIr("native-call IR is missing its runtime callback"),
                    )?;
                    let argument_bytes = arguments
                        .len()
                        .checked_mul(8)
                        .and_then(|size| u32::try_from(size).ok())
                        .ok_or(JitError::OffsetOverflow)?;
                    let slot = builder.create_sized_stack_slot(StackSlotData::new(
                        StackSlotKind::ExplicitSlot,
                        argument_bytes.max(1),
                        3,
                    ));
                    let argument_pointer =
                        builder
                            .ins()
                            .stack_addr(frontend_config.pointer_type(), slot, 0);
                    let mut type_mask = 0u64;
                    for (index, argument) in arguments.iter().copied().enumerate() {
                        let ty = value_types
                            .get(&argument)
                            .copied()
                            .ok_or(JitError::InvalidIr(IrError::UnknownValue(argument)))?;
                        let (raw, tag) = match ty {
                            IrType::I64 => (values[&argument], 0u64),
                            IrType::F64 => (
                                builder.ins().bitcast(
                                    types::I64,
                                    MemFlagsData::new(),
                                    values[&argument],
                                ),
                                1,
                            ),
                            IrType::Bool => {
                                (builder.ins().uextend(types::I64, values[&argument]), 2)
                            }
                            _ => {
                                return Err(JitError::UnsupportedIr(
                                    "native-call arguments must be numeric or boolean",
                                ));
                            }
                        };
                        let shift =
                            u32::try_from(index.checked_mul(2).ok_or(JitError::OffsetOverflow)?)
                                .map_err(|_| JitError::OffsetOverflow)?;
                        type_mask |= tag << shift;
                        let offset =
                            i32::try_from(index.checked_mul(8).ok_or(JitError::OffsetOverflow)?)
                                .map_err(|_| JitError::OffsetOverflow)?;
                        builder
                            .ins()
                            .store(MemFlagsData::trusted(), raw, argument_pointer, offset);
                    }
                    let result_type = match instruction.result.map(|(_, ty)| ty) {
                        Some(IrType::I64) => 0i64,
                        Some(IrType::F64) => 1,
                        Some(IrType::Bool) => 2,
                        _ => {
                            return Err(JitError::UnsupportedIr(
                                "native-call results must be numeric or boolean",
                            ));
                        }
                    };
                    let global_index = i64::from(global_index);
                    let argument_count =
                        i64::try_from(arguments.len()).map_err(|_| JitError::OffsetOverflow)?;
                    let context_value = control;
                    let exit_state_value = exit_state;
                    let global_value = builder.ins().iconst(types::I64, global_index);
                    let count_value = builder.ins().iconst(types::I64, argument_count);
                    let mask_value = builder.ins().iconst(
                        types::I64,
                        i64::try_from(type_mask).map_err(|_| JitError::OffsetOverflow)?,
                    );
                    let result_type_value = builder.ins().iconst(types::I64, result_type);
                    let call = builder.ins().call(
                        native_call_function,
                        &[
                            context_value,
                            exit_state_value,
                            global_value,
                            argument_pointer,
                            count_value,
                            mask_value,
                            result_type_value,
                        ],
                    );
                    lower_native_call_status(&mut builder, exit_state)?;
                    let raw = builder.inst_results(call)[0];
                    match instruction.result.map(|(_, ty)| ty) {
                        Some(IrType::F64) => {
                            Some(builder.ins().bitcast(types::F64, MemFlagsData::new(), raw))
                        }
                        Some(IrType::Bool) => Some(builder.ins().ireduce(types::I8, raw)),
                        Some(IrType::I64) => Some(raw),
                        _ => None,
                    }
                }
                IrInstructionKind::JitPoll => {
                    let poll_function = poll_function.ok_or(JitError::UnsupportedIr(
                        "controlled IR is missing its poll callback",
                    ))?;
                    lower_poll(&mut builder, poll_function, control, exit_state)?;
                    None
                }
                IrInstructionKind::Safepoint { .. } => None,
            };
            if let Some((result_id, _)) = instruction.result {
                let Some(result) = result else {
                    return Err(JitError::InvalidIr(IrError::UnexpectedResult));
                };
                values.insert(result_id, result);
            }
        }
        match &block.terminator {
            IrTerminator::Jump { target, arguments } => {
                let arguments = arguments
                    .iter()
                    .map(|value| BlockArg::from(values[value]))
                    .collect::<Vec<BlockArg>>();
                builder.ins().jump(blocks[target], &arguments);
            }
            IrTerminator::Branch {
                condition,
                then_target,
                then_arguments,
                else_target,
                else_arguments,
            } => {
                let then_arguments = then_arguments
                    .iter()
                    .map(|value| BlockArg::from(values[value]))
                    .collect::<Vec<BlockArg>>();
                let else_arguments = else_arguments
                    .iter()
                    .map(|value| BlockArg::from(values[value]))
                    .collect::<Vec<BlockArg>>();
                builder.ins().brif(
                    values[condition],
                    blocks[then_target],
                    &then_arguments,
                    blocks[else_target],
                    &else_arguments,
                );
            }
            IrTerminator::Return(value) => {
                let mut value = values[value];
                match ir.return_type {
                    IrType::F64 => {
                        value = builder
                            .ins()
                            .bitcast(types::I64, MemFlagsData::new(), value);
                    }
                    IrType::Bool => {
                        value = builder.ins().uextend(types::I64, value);
                    }
                    _ => {}
                }
                builder.ins().return_(&[value]);
            }
        }
    }
    builder.switch_to_block(integer_overflow);
    let overflow_sentinel = builder.ins().iconst(types::I64, i64::MIN);
    builder.ins().return_(&[overflow_sentinel]);
    builder.seal_all_blocks();
    builder.finalize(frontend_config);
    Ok(())
}

fn lower_poll(
    builder: &mut FunctionBuilder<'_>,
    poll_function: cranelift_codegen::ir::FuncRef,
    context: cranelift_codegen::ir::Value,
    exit_state: cranelift_codegen::ir::Value,
) -> Result<(), JitError> {
    let continuation = builder.create_block();
    let aborted = builder.create_block();
    let call = builder.ins().call(poll_function, &[context]);
    let status = builder.inst_results(call)[0];
    builder.ins().brif(status, aborted, &[], continuation, &[]);

    builder.switch_to_block(aborted);
    builder.seal_block(aborted);
    let kind = builder.ins().iconst(types::I64, 2);
    builder
        .ins()
        .store(MemFlagsData::trusted(), kind, exit_state, 0);
    let sentinel = builder.ins().iconst(types::I64, i64::MIN);
    builder.ins().return_(&[sentinel]);

    builder.switch_to_block(continuation);
    builder.seal_block(continuation);
    Ok(())
}

fn lower_native_call_status(
    builder: &mut FunctionBuilder<'_>,
    exit_state: cranelift_codegen::ir::Value,
) -> Result<(), JitError> {
    use cranelift_codegen::ir::condcodes::IntCC;
    let continuation = builder.create_block();
    let aborted = builder.create_block();
    let kind = builder
        .ins()
        .load(types::I64, MemFlagsData::trusted(), exit_state, 0);
    let zero = builder.ins().iconst(types::I64, 0);
    let ok = builder.ins().icmp(IntCC::Equal, kind, zero);
    builder.ins().brif(ok, continuation, &[], aborted, &[]);

    builder.switch_to_block(aborted);
    builder.seal_block(aborted);
    let sentinel = builder.ins().iconst(types::I64, i64::MIN);
    builder.ins().return_(&[sentinel]);

    builder.switch_to_block(continuation);
    builder.seal_block(continuation);
    Ok(())
}

fn lower_guard(
    builder: &mut FunctionBuilder<'_>,
    condition: cranelift_codegen::ir::Value,
    map: &super::ir::DeoptMap,
    values: &HashMap<super::ir::ValueId, cranelift_codegen::ir::Value>,
    exit_state: cranelift_codegen::ir::Value,
    deopt_registers: cranelift_codegen::ir::Value,
) -> Result<(), JitError> {
    let continuation = builder.create_block();
    let deopt = builder.create_block();
    builder.ins().brif(condition, continuation, &[], deopt, &[]);
    builder.switch_to_block(deopt);
    builder.seal_block(deopt);
    let kind = builder.ins().iconst(types::I64, 1);
    let bytecode_ip = builder.ins().iconst(types::I64, i64::from(map.bytecode_ip));
    builder
        .ins()
        .store(MemFlagsData::trusted(), kind, exit_state, 0);
    builder
        .ins()
        .store(MemFlagsData::trusted(), bytecode_ip, exit_state, 8);
    for &(register, value) in &map.registers {
        let offset = i32::try_from(
            usize::from(register)
                .checked_mul(8)
                .ok_or(JitError::OffsetOverflow)?,
        )
        .map_err(|_| JitError::OffsetOverflow)?;
        builder.ins().store(
            MemFlagsData::trusted(),
            values[&value],
            deopt_registers,
            offset,
        );
    }
    let sentinel = builder.ins().iconst(types::I64, i64::MIN);
    builder.ins().return_(&[sentinel]);
    builder.switch_to_block(continuation);
    builder.seal_block(continuation);
    Ok(())
}

fn check_integer_result(
    builder: &mut FunctionBuilder<'_>,
    result: cranelift_codegen::ir::Value,
    machine_overflow: Option<cranelift_codegen::ir::Value>,
    overflow_block: cranelift_codegen::ir::Block,
) -> cranelift_codegen::ir::Value {
    use cranelift_codegen::ir::condcodes::IntCC;
    let minimum = builder
        .ins()
        .iconst(types::I64, aelys_bytecode::Value::INT_MIN);
    let maximum = builder
        .ins()
        .iconst(types::I64, aelys_bytecode::Value::INT_MAX);
    let below = builder.ins().icmp(IntCC::SignedLessThan, result, minimum);
    let above = builder
        .ins()
        .icmp(IntCC::SignedGreaterThan, result, maximum);
    let mut overflow = builder.ins().bor(below, above);
    if let Some(machine_overflow) = machine_overflow {
        overflow = builder.ins().bor(overflow, machine_overflow);
    }
    let continuation = builder.create_block();
    builder
        .ins()
        .brif(overflow, overflow_block, &[], continuation, &[]);
    builder.switch_to_block(continuation);
    builder.seal_block(continuation);
    result
}

fn lower_type(ty: IrType) -> cranelift_codegen::ir::Type {
    match ty {
        IrType::Uninitialized => types::I64,
        IrType::I64 => types::I64,
        IrType::F64 => types::F64,
        IrType::Bool => types::I8,
        IrType::I64Array => types::I64,
        IrType::I64Vec => types::I64,
    }
}

/// which entry argument a collection value comes from, followed through the block arguments of every jump
fn collection_origins(ir: &FunctionIr, entry: &IrBlock) -> HashMap<ValueId, usize> {
    let mut origins: HashMap<ValueId, usize> = entry
        .parameters
        .iter()
        .enumerate()
        .filter_map(|(index, (value, ty))| is_collection_type(*ty).then_some((*value, index)))
        .collect();
    let mut conflicting: HashSet<ValueId> = HashSet::new();
    let blocks_by_id: HashMap<BlockId, &IrBlock> =
        ir.blocks.iter().map(|block| (block.id, block)).collect();
    let mut changed = true;
    while changed {
        changed = false;
        for block in &ir.blocks {
            let edges: Vec<(BlockId, &Vec<ValueId>)> = match &block.terminator {
                IrTerminator::Jump { target, arguments } => vec![(*target, arguments)],
                IrTerminator::Branch {
                    then_target,
                    then_arguments,
                    else_target,
                    else_arguments,
                    ..
                } => vec![
                    (*then_target, then_arguments),
                    (*else_target, else_arguments),
                ],
                IrTerminator::Return(_) => Vec::new(),
            };
            for (target, arguments) in edges {
                let Some(target_block) = blocks_by_id.get(&target) else {
                    continue;
                };
                for (position, argument) in arguments.iter().enumerate() {
                    let Some(&(parameter, ty)) = target_block.parameters.get(position) else {
                        continue;
                    };
                    if !is_collection_type(ty) || conflicting.contains(&parameter) {
                        continue;
                    }
                    let Some(&source) = origins.get(argument) else {
                        continue;
                    };
                    match origins.get(&parameter) {
                        Some(&existing) if existing == source => {}
                        Some(_) => {
                            origins.remove(&parameter);
                            conflicting.insert(parameter);
                            changed = true;
                        }
                        None => {
                            origins.insert(parameter, source);
                            changed = true;
                        }
                    }
                }
            }
        }
    }
    origins
}

fn is_collection_type(ty: IrType) -> bool {
    matches!(ty, IrType::I64Array | IrType::I64Vec)
}

fn lower_predicate(predicate: IntPredicate) -> cranelift_codegen::ir::condcodes::IntCC {
    use cranelift_codegen::ir::condcodes::IntCC;
    match predicate {
        IntPredicate::Equal => IntCC::Equal,
        IntPredicate::NotEqual => IntCC::NotEqual,
        IntPredicate::SignedLessThan => IntCC::SignedLessThan,
        IntPredicate::SignedLessThanOrEqual => IntCC::SignedLessThanOrEqual,
        IntPredicate::SignedGreaterThan => IntCC::SignedGreaterThan,
        IntPredicate::SignedGreaterThanOrEqual => IntCC::SignedGreaterThanOrEqual,
    }
}

fn lower_float_predicate(predicate: IntPredicate) -> cranelift_codegen::ir::condcodes::FloatCC {
    use cranelift_codegen::ir::condcodes::FloatCC;
    match predicate {
        IntPredicate::Equal => FloatCC::Equal,
        IntPredicate::NotEqual => FloatCC::NotEqual,
        IntPredicate::SignedLessThan => FloatCC::LessThan,
        IntPredicate::SignedLessThanOrEqual => FloatCC::LessThanOrEqual,
        IntPredicate::SignedGreaterThan => FloatCC::GreaterThan,
        IntPredicate::SignedGreaterThanOrEqual => FloatCC::GreaterThanOrEqual,
    }
}

fn touch_lru(lru: &mut VecDeque<JitKey>, key: JitKey) {
    if let Some(position) = lru.iter().position(|candidate| candidate == &key) {
        lru.remove(position);
    }
    lru.push_back(key);
}

fn cpu_feature_key() -> u64 {
    #[cfg(target_arch = "x86_64")]
    {
        u64::from(std::is_x86_feature_detected!("sse2"))
            | (u64::from(std::is_x86_feature_detected!("sse4.1")) << 1)
            | (u64::from(std::is_x86_feature_detected!("avx")) << 2)
            | (u64::from(std::is_x86_feature_detected!("avx2")) << 3)
            | (u64::from(std::is_x86_feature_detected!("bmi1")) << 4)
            | (u64::from(std::is_x86_feature_detected!("bmi2")) << 5)
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        0
    }
}

#[derive(Debug)]
pub(crate) enum JitError {
    UnsupportedTarget(String),
    Configuration(String),
    InvalidIr(IrError),
    UnsupportedIr(&'static str),
    Module(String),
    InvalidCacheLimit,
    Poisoned,
    OffsetOverflow,
    #[cfg(test)]
    Aborted,
    Arity {
        expected: usize,
        actual: usize,
    },
    ArgumentType(usize),
    #[cfg(test)]
    Deoptimized(u32),
    InvalidExitKind(u64),
    UnknownDeoptExit(u32),
}

impl fmt::Display for JitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedTarget(error) => write!(formatter, "unsupported JIT target: {error}"),
            Self::Configuration(error) => write!(formatter, "invalid JIT configuration: {error}"),
            Self::InvalidIr(error) => write!(formatter, "invalid Aelys IR: {error:?}"),
            Self::UnsupportedIr(feature) => write!(formatter, "unsupported Aelys IR: {feature}"),
            Self::Module(error) => write!(formatter, "Cranelift module error: {error}"),
            Self::InvalidCacheLimit => {
                formatter.write_str("JIT cache must contain at least one entry")
            }
            Self::Poisoned => formatter.write_str("JIT cache lock is poisoned"),
            Self::OffsetOverflow => {
                formatter.write_str("JIT argument offset exceeds the target ABI")
            }
            #[cfg(test)]
            Self::Aborted => formatter.write_str("JIT execution was aborted by its control poll"),
            Self::Arity { expected, actual } => {
                write!(
                    formatter,
                    "JIT arity mismatch: expected {expected}, got {actual}"
                )
            }
            Self::ArgumentType(index) => {
                write!(formatter, "JIT argument {index} has an incompatible type")
            }
            #[cfg(test)]
            Self::Deoptimized(bytecode_ip) => {
                write!(formatter, "JIT deoptimized at bytecode IP {bytecode_ip}")
            }
            Self::InvalidExitKind(kind) => write!(formatter, "invalid JIT exit kind {kind}"),
            Self::UnknownDeoptExit(bytecode_ip) => {
                write!(formatter, "unknown JIT deoptimization IP {bytecode_ip}")
            }
        }
    }
}

impl std::error::Error for JitError {}
