use super::engine::{CompiledFunction, JitEngine, JitExecution, JitKey, JitTier};
use super::optimize::optimize_integer_ir;
use super::translate::translate_integer_function;
use aelys_bytecode::Function;
use aelys_runtime::{JitCallResult, JitExecutor, JitFunctionKey, Value};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub(crate) struct JitProvider {
    engine: JitEngine,
    call_threshold: u64,
    tier2_call_threshold: Option<u64>,
    has_compiled_code: AtomicBool,
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
        })
    }

    pub(crate) fn cache_len(&self) -> usize {
        self.engine.cache_len().unwrap_or(0)
    }

    fn compiled(
        &self,
        key: &JitFunctionKey,
        function: &Function,
        calls: u64,
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
            let _ = self.compiled(key, function, self.call_threshold);
        }
    }

    fn try_execute(
        &self,
        key: &JitFunctionKey,
        function: &Function,
        arguments: &[Value],
        calls: u64,
    ) -> JitCallResult {
        let Some(compiled) = self.compiled(key, function, calls) else {
            return JitCallResult::Unsupported;
        };
        if compiled.arity() != arguments.len() {
            return JitCallResult::Unsupported;
        }
        let Some(arguments) = arguments
            .iter()
            .map(Value::as_int)
            .collect::<Option<Vec<_>>>()
        else {
            return JitCallResult::Unsupported;
        };
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
