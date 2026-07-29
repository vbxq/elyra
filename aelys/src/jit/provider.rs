use super::engine::{CompiledFunction, JitEngine, JitExecution, JitKey, JitTier};
use super::optimize::{optimize_integer_ir, specialize_integer_parameters};
use super::translate::translate_integer_function;
use aelys_bytecode::Function;
use aelys_runtime::{JitCallResult, JitExecutor, JitFunctionKey, Value};
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
        })
    }

    pub(crate) fn cache_len(&self) -> usize {
        self.engine.cache_len().unwrap_or(0)
    }

    pub(crate) fn deoptimizations(&self) -> u64 {
        self.deoptimizations.load(Ordering::Relaxed)
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
    ) -> Option<Arc<CompiledFunction>> {
        let tier = self
            .tier2_call_threshold
            .filter(|threshold| calls >= *threshold)
            .map_or(JitTier::Baseline, |_| JitTier::Optimized);
        let cache_key = JitKey::for_path(key.module(), key.shared_path(), tier);
        if let Some(compiled) = self.engine.cached(&cache_key).ok().flatten() {
            return Some(compiled);
        }
        if calls < self.call_threshold {
            return None;
        }
        let mut ir = translate_integer_function(function)?;
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
                let baseline = JitKey::for_path(
                    key.module(),
                    Arc::clone(&cache_key.function_path),
                    JitTier::Baseline,
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
            let _ = self.compiled(key, function, self.call_threshold, None);
        }
    }

    fn try_execute(
        &self,
        key: &JitFunctionKey,
        function: &Function,
        arguments: &[Value],
        calls: u64,
    ) -> JitCallResult {
        let Some(arguments) = arguments
            .iter()
            .map(Value::as_int)
            .collect::<Option<Vec<_>>>()
        else {
            return JitCallResult::Unsupported;
        };
        if let Some(threshold) = self.tier2_call_threshold
            && calls <= threshold
        {
            self.observe_profile(key, &arguments);
        }
        let profile = self
            .tier2_call_threshold
            .filter(|threshold| calls >= *threshold)
            .and_then(|_| self.profile(key));
        let Some(compiled) = self.compiled(key, function, calls, profile.as_deref()) else {
            return JitCallResult::Unsupported;
        };
        if compiled.arity() != arguments.len() {
            return JitCallResult::Unsupported;
        }
        let Ok(result) = compiled.execute(&arguments) else {
            return JitCallResult::Unsupported;
        };
        match result {
            JitExecution::Returned(result) => match Value::int_checked(result) {
                Ok(value) => JitCallResult::Returned(value),
                Err(_) => JitCallResult::Unsupported,
            },
            JitExecution::Deoptimized {
                bytecode_ip,
                registers,
            } => {
                self.deoptimizations.fetch_add(1, Ordering::Relaxed);
                let Some(registers) = registers
                    .into_iter()
                    .map(|(register, value)| Some((register, Value::int_checked(value).ok()?)))
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
