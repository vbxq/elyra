use super::ir::{FunctionIr, IntPredicate, IrError, IrInstructionKind, IrTerminator, IrType};
use cranelift_codegen::ir::{AbiParam, BlockArg, InstBuilder, MemFlagsData, UserFuncName, types};
use cranelift_codegen::isa::OwnedTargetIsa;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{Linkage, Module};
use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::sync::{Arc, Mutex};

pub(crate) const JIT_ABI_VERSION: u16 = 1;

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
    pub(crate) abi: u16,
    pub(crate) cpu_features: u64,
}

impl JitKey {
    #[cfg(test)]
    pub(crate) fn new(module: u64, function: u32, tier: JitTier) -> Self {
        Self::for_path(module, Arc::from([function]), tier)
    }

    pub(crate) fn for_path(module: u64, function_path: Arc<[u32]>, tier: JitTier) -> Self {
        Self {
            module,
            function_path,
            tier,
            abi: JIT_ABI_VERSION,
            cpu_features: cpu_feature_key(),
        }
    }
}

pub(crate) struct CompiledFunction {
    address: usize,
    arity: usize,
    deopt_register_count: usize,
    deopt_maps: HashMap<u32, Arc<[u16]>>,
    _module: Mutex<JITModule>,
}

#[repr(C)]
#[derive(Default)]
struct RawJitExit {
    kind: u64,
    bytecode_ip: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum JitExecution {
    Returned(i64),
    Deoptimized {
        bytecode_ip: u32,
        registers: Vec<(u16, i64)>,
    },
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

    #[cfg(test)]
    pub(crate) fn execute_i64(&self, arguments: &[i64]) -> Result<i64, JitError> {
        match self.execute(arguments)? {
            JitExecution::Returned(value) => Ok(value),
            JitExecution::Deoptimized { bytecode_ip, .. } => {
                Err(JitError::Deoptimized(bytecode_ip))
            }
        }
    }

    pub(crate) fn execute(&self, arguments: &[i64]) -> Result<JitExecution, JitError> {
        if arguments.len() != self.arity {
            return Err(JitError::Arity {
                expected: self.arity,
                actual: arguments.len(),
            });
        }
        type Entry = unsafe extern "C" fn(*const i64, *mut RawJitExit, *mut i64) -> i64;
        // SAFETY: addresses are obtained from finalized Cranelift functions with this exact ABI.
        let entry = unsafe { std::mem::transmute::<usize, Entry>(self.address) };
        let mut exit = RawJitExit::default();
        let mut deopt_registers = vec![0; self.deopt_register_count];
        // SAFETY: Cranelift receives valid argument, exit-state and deoptimization buffers for the compiled ABI.
        let result = unsafe { entry(arguments.as_ptr(), &mut exit, deopt_registers.as_mut_ptr()) };
        if exit.kind == 0 {
            return Ok(JitExecution::Returned(result));
        }
        if exit.kind != 1 {
            return Err(JitError::InvalidExitKind(exit.kind));
        }
        let bytecode_ip =
            u32::try_from(exit.bytecode_ip).map_err(|_| JitError::InvalidExitKind(exit.kind))?;
        let register_ids = self
            .deopt_maps
            .get(&bytecode_ip)
            .ok_or(JitError::UnknownDeoptExit(bytecode_ip))?;
        let registers = register_ids
            .iter()
            .map(|register| (*register, deopt_registers[usize::from(*register)]))
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
        if ir.return_type != IrType::I64 {
            return Err(JitError::UnsupportedIr("non-i64 return type"));
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
        let mut module = JITModule::new(JITBuilder::with_isa(
            Arc::clone(isa),
            cranelift_module::default_libcall_names(),
        ));
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
            .returns
            .push(AbiParam::new(types::I64));
        context.func.name = UserFuncName::user(0, u32::try_from(state.next_symbol).unwrap_or(0));

        let function_id = module
            .declare_function(&symbol, Linkage::Local, &context.func.signature)
            .map_err(|error| JitError::Module(error.to_string()))?;
        lower_function(ir, &mut context.func, frontend_config)?;
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
        let deopt_maps = ir
            .deopt_maps
            .iter()
            .map(|map| {
                (
                    map.bytecode_ip,
                    Arc::from(
                        map.registers
                            .iter()
                            .map(|(register, _)| *register)
                            .collect::<Vec<_>>(),
                    ),
                )
            })
            .collect();
        let entry = Arc::new(CompiledFunction {
            address,
            arity: ir.parameter_types.len(),
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

fn lower_function(
    ir: &FunctionIr,
    function: &mut cranelift_codegen::ir::Function,
    frontend_config: cranelift_codegen::isa::TargetFrontendConfig,
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
                let offset = i32::try_from(index.checked_mul(8).ok_or(JitError::OffsetOverflow)?)
                    .map_err(|_| JitError::OffsetOverflow)?;
                let loaded =
                    builder
                        .ins()
                        .load(lower_type(ty), MemFlagsData::trusted(), arguments, offset);
                values.insert(value, loaded);
            }
        }
        for instruction in &block.instructions {
            let result = match instruction.kind {
                IrInstructionKind::Iconst(value) => Some(builder.ins().iconst(types::I64, value)),
                IrInstructionKind::Bconst(value) => {
                    Some(builder.ins().iconst(types::I8, i64::from(value)))
                }
                IrInstructionKind::Iadd(left, right) => {
                    let result = builder.ins().iadd(values[&left], values[&right]);
                    Some(check_integer_result(
                        &mut builder,
                        result,
                        None,
                        integer_overflow,
                    ))
                }
                IrInstructionKind::Isub(left, right) => {
                    let result = builder.ins().isub(values[&left], values[&right]);
                    Some(check_integer_result(
                        &mut builder,
                        result,
                        None,
                        integer_overflow,
                    ))
                }
                IrInstructionKind::Imul(left, right) => {
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
                IrInstructionKind::Icmp {
                    predicate,
                    left,
                    right,
                } => Some(builder.ins().icmp(
                    lower_predicate(predicate),
                    values[&left],
                    values[&right],
                )),
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
                if ir.return_type == IrType::Bool {
                    value = builder.ins().uextend(types::I64, value);
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
        IrType::I64 => types::I64,
        IrType::Bool => types::I8,
    }
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
    Arity {
        expected: usize,
        actual: usize,
    },
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
            Self::Arity { expected, actual } => {
                write!(
                    formatter,
                    "JIT arity mismatch: expected {expected}, got {actual}"
                )
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
