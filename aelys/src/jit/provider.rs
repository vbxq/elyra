use super::engine::{CompiledFunction, JitDeoptValue, JitEngine, JitExecution, JitKey, JitTier};
use super::optimize::{optimize_integer_ir, specialize_integer_parameters};
use super::translate::{
    translate_controlled_integer_function, translate_controlled_integer_osr,
    translate_integer_function, translate_integer_osr, translate_optimized_integer_function,
};
use aelys_bytecode::Function;
use aelys_runtime::{
    JitArgument, JitCallResult, JitDeoptValue as RuntimeDeoptValue, JitExecutionContext,
    JitExecutor, JitFunctionKey, Value,
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy)]
enum ProfileValue {
    Constant(i64),
    Variable,
}

struct NumericProfile {
    values: Vec<ProfileValue>,
}

impl NumericProfile {
    fn observe(&mut self, arguments: &[i64]) {
        if self.values.is_empty() {
            self.values = arguments
                .iter()
                .copied()
                .map(ProfileValue::Constant)
                .collect();
            return;
        }
        if self.values.len() != arguments.len() {
            self.values.fill(ProfileValue::Variable);
            return;
        }
        for (profile, argument) in self.values.iter_mut().zip(arguments) {
            if matches!(profile, ProfileValue::Constant(value) if value != argument) {
                *profile = ProfileValue::Variable;
            }
        }
    }

    fn constants(&self) -> Vec<Option<i64>> {
        self.values
            .iter()
            .map(|value| match value {
                ProfileValue::Constant(value) => Some(*value),
                ProfileValue::Variable => None,
            })
            .collect()
    }
}

pub(crate) struct JitProvider {
    engine: JitEngine,
    call_threshold: u64,
    tier2_call_threshold: Option<u64>,
    has_compiled_code: AtomicBool,
    profiles: Mutex<HashMap<JitKey, NumericProfile>>,
    deoptimizations: AtomicU64,
    osr_executions: AtomicU64,
}

impl JitProvider {
    pub(crate) fn new(
        max_entries: usize,
        call_threshold: u64,
        tier2_call_threshold: Option<u64>,
    ) -> Result<Self, String> {
        let engine = JitEngine::new(max_entries).map_err(|error| error.to_string())?;
        Ok(Self {
            engine,
            call_threshold,
            tier2_call_threshold,
            has_compiled_code: AtomicBool::new(false),
            profiles: Mutex::new(HashMap::new()),
            deoptimizations: AtomicU64::new(0),
            osr_executions: AtomicU64::new(0),
        })
    }

    pub(crate) fn cache_len(&self) -> usize {
        self.engine.cache_len().unwrap_or(0)
    }

    pub(crate) fn deoptimizations(&self) -> u64 {
        self.deoptimizations.load(Ordering::Relaxed)
    }

    pub(crate) fn osr_executions(&self) -> u64 {
        self.osr_executions.load(Ordering::Relaxed)
    }

    fn observe_profile(&self, key: &JitFunctionKey, arguments: &[i64]) {
        let profile_key = JitKey::for_path(key.module(), key.shared_path(), JitTier::Baseline);
        if let Ok(mut profiles) = self.profiles.lock() {
            profiles
                .entry(profile_key)
                .or_insert_with(|| NumericProfile { values: Vec::new() })
                .observe(arguments);
        }
    }

    fn profile(&self, key: &JitFunctionKey) -> Option<Vec<Option<i64>>> {
        let profile_key = JitKey::for_path(key.module(), key.shared_path(), JitTier::Baseline);
        self.profiles
            .lock()
            .ok()?
            .get(&profile_key)
            .map(NumericProfile::constants)
    }

    fn compiled(
        &self,
        key: &JitFunctionKey,
        function: &Function,
        calls: u64,
        profile: Option<&[Option<i64>]>,
        controlled: bool,
    ) -> Option<Arc<CompiledFunction>> {
        if function.jit_unsupported_struct {
            return None;
        }
        let tier = self
            .tier2_call_threshold
            .filter(|threshold| calls >= *threshold)
            .map_or(JitTier::Baseline, |_| JitTier::Optimized);
        let cache_key =
            JitKey::for_path_with_control(key.module(), key.shared_path(), tier, controlled);
        if let Some(compiled) = self.engine.cached(&cache_key).ok().flatten() {
            return Some(compiled);
        }
        if calls < self.call_threshold {
            return None;
        }
        let mut ir = if controlled {
            translate_controlled_integer_function(function)?
        } else if tier == JitTier::Optimized {
            translate_optimized_integer_function(function)?
        } else {
            translate_integer_function(function)?
        };
        if tier == JitTier::Optimized {
            optimize_integer_ir(&mut ir);
            if let Some(profile) = profile {
                specialize_integer_parameters(&mut ir, profile);
            }
            ir.verify().ok()?;
        }
        let compiled = match self.engine.compile(&cache_key, &ir) {
            Ok(compiled) => compiled,
            Err(_) if tier == JitTier::Optimized => {
                let baseline = JitKey::for_path_with_control(
                    key.module(),
                    Arc::clone(&cache_key.function_path),
                    JitTier::Baseline,
                    controlled,
                );
                self.engine.cached(&baseline).ok().flatten()?
            }
            Err(_) => return None,
        };
        self.has_compiled_code.store(true, Ordering::Release);
        Some(compiled)
    }

    fn key(key: &JitFunctionKey) -> JitKey {
        JitKey::for_path(key.module(), key.shared_path(), JitTier::Baseline)
    }
}

impl JitExecutor for JitProvider {
    fn should_execute(&self, key: &JitFunctionKey, calls: u64) -> bool {
        if calls >= self.call_threshold {
            return true;
        }
        self.has_compiled_code.load(Ordering::Acquire)
            && self.engine.cached(&Self::key(key)).ok().flatten().is_some()
    }

    fn observe_backedge(&self, key: &JitFunctionKey, function: &Function, backedges: u64) {
        if backedges >= aelys_runtime::JIT_TIER1_BACKEDGE_THRESHOLD {
            let _ = self.compiled(key, function, self.call_threshold, None, false);
        }
    }

    fn try_execute(
        &self,
        key: &JitFunctionKey,
        function: &Function,
        arguments: &[JitArgument<'_>],
        calls: u64,
    ) -> JitCallResult {
        self.try_execute_inner(key, function, arguments, calls, false, None)
    }

    fn try_execute_with_context(
        &self,
        key: &JitFunctionKey,
        function: &Function,
        arguments: &[JitArgument<'_>],
        calls: u64,
        context: Option<&JitExecutionContext>,
    ) -> JitCallResult {
        self.try_execute_inner(
            key,
            function,
            arguments,
            calls,
            context.is_some_and(|context| context.controlled),
            context,
        )
    }

    fn try_execute_osr(
        &self,
        key: &JitFunctionKey,
        function: &Function,
        bytecode_ip: u32,
        registers: &[JitArgument<'_>],
    ) -> JitCallResult {
        self.try_execute_osr_inner(key, function, bytecode_ip, registers, false, None)
    }

    fn try_execute_osr_with_context(
        &self,
        key: &JitFunctionKey,
        function: &Function,
        bytecode_ip: u32,
        registers: &[JitArgument<'_>],
        context: Option<&JitExecutionContext>,
    ) -> JitCallResult {
        self.try_execute_osr_inner(
            key,
            function,
            bytecode_ip,
            registers,
            context.is_some_and(|context| context.controlled),
            context,
        )
    }
}

impl JitProvider {
    fn try_execute_inner(
        &self,
        key: &JitFunctionKey,
        function: &Function,
        arguments: &[JitArgument<'_>],
        calls: u64,
        controlled: bool,
        context: Option<&JitExecutionContext>,
    ) -> JitCallResult {
        if function.jit_unsupported_struct {
            return JitCallResult::Unsupported;
        }
        let integer_arguments = arguments
            .iter()
            .map(|argument| match argument {
                JitArgument::Integer(value) => Some(*value),
                JitArgument::Float(_)
                | JitArgument::Boolean(_)
                | JitArgument::IntegerArray(_)
                | JitArgument::IntegerVec(_)
                | JitArgument::Unused => None,
            })
            .collect::<Option<Vec<_>>>();
        if let Some(threshold) = self.tier2_call_threshold
            && calls <= threshold
            && let Some(integer_arguments) = &integer_arguments
        {
            self.observe_profile(key, integer_arguments);
        }
        let profile = self
            .tier2_call_threshold
            .filter(|threshold| calls >= *threshold)
            .and_then(|_| self.profile(key));
        let Some(compiled) = self.compiled(key, function, calls, profile.as_deref(), controlled)
        else {
            return JitCallResult::Unsupported;
        };
        if compiled.arity() != arguments.len() {
            return JitCallResult::Unsupported;
        }
        let result = compiled.execute_arguments_with_context(arguments, context);
        let Ok(result) = result else {
            return JitCallResult::Unsupported;
        };
        match result {
            JitExecution::Returned(result) => match returned_value(&compiled, result) {
                Some(value) => JitCallResult::Returned(value),
                None => JitCallResult::Unsupported,
            },
            JitExecution::Aborted => JitCallResult::Aborted,
            JitExecution::Deoptimized {
                bytecode_ip,
                registers,
            } => {
                self.deoptimizations.fetch_add(1, Ordering::Relaxed);
                let Some(registers) = registers
                    .into_iter()
                    .map(|(register, value)| {
                        let value = match value {
                            JitDeoptValue::Integer(value) => {
                                RuntimeDeoptValue::Value(Value::int_checked(value).ok()?)
                            }
                            JitDeoptValue::Float(bits) => {
                                RuntimeDeoptValue::Value(Value::float(f64::from_bits(bits)))
                            }
                            JitDeoptValue::Boolean(value) => {
                                RuntimeDeoptValue::Value(Value::bool(value))
                            }
                            JitDeoptValue::Argument(index) => RuntimeDeoptValue::Argument(index),
                        };
                        Some((register, value))
                    })
                    .collect::<Option<Vec<_>>>()
                else {
                    return JitCallResult::Unsupported;
                };
                JitCallResult::Deoptimized {
                    bytecode_ip,
                    registers,
                }
            }
        }
    }
}

impl JitProvider {
    fn try_execute_osr_inner(
        &self,
        key: &JitFunctionKey,
        function: &Function,
        bytecode_ip: u32,
        registers: &[JitArgument<'_>],
        controlled: bool,
        context: Option<&JitExecutionContext>,
    ) -> JitCallResult {
        if function.jit_unsupported_struct {
            return JitCallResult::Unsupported;
        }
        let cache_key =
            JitKey::for_osr_with_control(key.module(), key.shared_path(), bytecode_ip, controlled);
        let compiled = self.engine.cached(&cache_key).ok().flatten().or_else(|| {
            let ir = if controlled {
                translate_controlled_integer_osr(function, bytecode_ip)?
            } else {
                translate_integer_osr(function, bytecode_ip)?
            };
            self.engine.compile(&cache_key, &ir).ok()
        });
        let Some(compiled) = compiled else {
            return JitCallResult::Unsupported;
        };
        let Ok(result) = compiled.execute_arguments_with_context(registers, context) else {
            return JitCallResult::Unsupported;
        };
        match result {
            JitExecution::Returned(value) => match returned_value(&compiled, value) {
                Some(value) => {
                    self.osr_executions.fetch_add(1, Ordering::Relaxed);
                    JitCallResult::Returned(value)
                }
                None => JitCallResult::Unsupported,
            },
            JitExecution::Aborted => JitCallResult::Aborted,
            JitExecution::Deoptimized { .. } => JitCallResult::Unsupported,
        }
    }
}

fn returned_value(compiled: &CompiledFunction, raw: i64) -> Option<Value> {
    match compiled.return_type() {
        super::ir::IrType::I64 => Value::int_checked(raw).ok(),
        super::ir::IrType::F64 => Some(Value::float(f64::from_bits(u64::from_ne_bytes(
            raw.to_ne_bytes(),
        )))),
        super::ir::IrType::Bool => Some(Value::bool(raw != 0)),
        super::ir::IrType::Uninitialized
        | super::ir::IrType::I64Array
        | super::ir::IrType::I64Vec => None,
    }
}
