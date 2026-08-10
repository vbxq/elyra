use super::{GcRef, ObjectKind, VM, Value};
use crate::JitExecutionContext;
use aelys_common::error::{RuntimeError, RuntimeErrorKind};
use std::ffi::c_void;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

#[derive(Clone, Debug, Default)]
pub struct InterruptHandle(Arc<AtomicBool>);

impl InterruptHandle {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn interrupt(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn reset(&self) {
        self.0.store(false, Ordering::Release);
    }

    pub fn is_interrupted(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

#[derive(Clone, Debug)]
pub struct ExecutionControl {
    pub max_instructions: Option<u64>,
    pub deadline: Option<Instant>,
    pub interrupt: Option<InterruptHandle>,
    pub safepoint_interval: u32,
    pub report: bool,
}

impl Default for ExecutionControl {
    fn default() -> Self {
        Self {
            max_instructions: None,
            deadline: None,
            interrupt: None,
            safepoint_interval: 1_024,
            report: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ExecutionStats {
    pub instructions: u64,
    pub collections: u64,
    pub minor_collections: u64,
    pub major_collections: u64,
    pub gc_pause_micros: u64,
    pub gc_max_pause_micros: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub allocations: u64,
    pub last_function: Option<GcRef>,
    pub last_instruction_pointer: Option<usize>,
    pub(crate) allocation_start: u64,
}

impl VM {
    pub(crate) fn execution_control_enabled(&self) -> bool {
        self.execution_control.max_instructions.is_some()
            || self.execution_control.deadline.is_some()
            || self.execution_control.interrupt.is_some()
    }

    pub fn configure_execution(&mut self, control: ExecutionControl) {
        self.execution_control = control;
        self.jit_control_error = None;
        self.execution_stats = ExecutionStats {
            allocation_start: self.heap.allocation_count(),
            ..ExecutionStats::default()
        };
    }

    pub fn execution_stats(&self) -> ExecutionStats {
        let mut stats = self.execution_stats;
        stats.allocations = self
            .heap
            .allocation_count()
            .saturating_sub(stats.allocation_start);
        stats
    }

    /// Create the isolate-local context used by controlled JIT invocations.
    pub fn jit_execution_context(&mut self, controlled: bool) -> JitExecutionContext {
        JitExecutionContext {
            data: (self as *mut VM).cast::<c_void>(),
            poll: poll_jit_execution,
            controlled,
        }
    }

    /// Take an error raised by a controlled JIT poll, if any.
    pub fn take_jit_control_error(&mut self) -> Option<RuntimeError> {
        self.jit_control_error.take()
    }

    pub fn last_execution_function_name(&self) -> Option<String> {
        let reference = self.execution_stats.last_function?;
        let object = self.heap.get(reference)?;
        let ObjectKind::Function(function) = &object.kind else {
            return None;
        };
        Some(function.name().unwrap_or("<main>").to_string())
    }

    #[inline(always)]
    pub(crate) fn check_execution_control(&mut self) -> Result<(), RuntimeError> {
        if let Some(limit) = self.execution_control.max_instructions
            && self.execution_stats.instructions >= limit
        {
            return Err(self.runtime_error(RuntimeErrorKind::InstructionBudgetExceeded { limit }));
        }

        self.execution_stats.instructions += 1;
        let interval = u64::from(self.execution_control.safepoint_interval.max(1));
        let is_safepoint = self.execution_stats.instructions == 1
            || self.execution_stats.instructions.is_multiple_of(interval);
        if !is_safepoint {
            return Ok(());
        }

        if self
            .execution_control
            .interrupt
            .as_ref()
            .is_some_and(InterruptHandle::is_interrupted)
        {
            return Err(self.runtime_error(RuntimeErrorKind::Interrupted));
        }
        if self
            .execution_control
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            return Err(self.runtime_error(RuntimeErrorKind::DeadlineExceeded));
        }
        self.maybe_collect();
        Ok(())
    }
}

/// Callback invoked by controlled JIT code. The machine-code side only needs a
/// continue/abort bit; the VM retains the structured error for the caller.
unsafe extern "C" fn poll_jit_execution(data: *mut c_void) -> i64 {
    if data.is_null() {
        return 0;
    }
    // SAFETY: the context is created from the currently borrowed VM and the
    // JIT call is synchronous, so it remains valid until the machine code
    // returns.
    let vm = unsafe { &mut *data.cast::<VM>() };
    if vm.jit_control_error.is_some() {
        return 1;
    }
    match vm.check_execution_control() {
        Ok(()) => 0,
        Err(error) => {
            vm.jit_control_error = Some(error);
            1
        }
    }
}

/// Runtime callback used by JIT code for a statically addressed global call.
/// The arguments are unboxed machine values accompanied by two-bit type tags;
/// the callback converts them to VM values, invokes the global, and converts
/// the numeric result back for the compiled continuation.
#[doc = "# Safety"]
#[doc = "the jit supplies valid context, exit, and argument pointers for the call."]
pub unsafe extern "C" fn call_jit_global(
    context: *mut JitExecutionContext,
    exit_state: *mut u64,
    global_index: u64,
    arguments: *const i64,
    argument_count: u64,
    type_mask: u64,
    result_type: u64,
) -> i64 {
    if context.is_null() {
        return 0;
    }
    // SAFETY: the JIT receives a live context and exit buffer for the whole
    // synchronous invocation.
    let context = unsafe { &*context };
    // SAFETY: the context was created from a live VM by the runtime.
    let vm = unsafe { &mut *context.data.cast::<VM>() };
    let fail = |vm: &mut VM, error: RuntimeError| {
        vm.jit_control_error = Some(error);
        if !exit_state.is_null() {
            // Exit kind 3 is reserved for a callback/runtime error.
            unsafe { *exit_state = 3 };
        }
        0
    };

    let count = match usize::try_from(argument_count) {
        Ok(count) => count,
        Err(_) => {
            return fail(
                vm,
                vm.runtime_error(RuntimeErrorKind::ArgumentLimitExceeded {
                    count: usize::MAX,
                    max: u16::MAX,
                }),
            );
        }
    };
    if count > 32 || (count != 0 && arguments.is_null()) {
        return fail(
            vm,
            vm.runtime_error(RuntimeErrorKind::ArgumentLimitExceeded {
                count,
                max: u16::MAX,
            }),
        );
    }
    // SAFETY: the JIT builds this stack buffer immediately before the call and
    // keeps it alive until the callback returns.
    let arguments = unsafe { std::slice::from_raw_parts(arguments, count) };
    let mut values = Vec::with_capacity(count);
    for (index, raw) in arguments.iter().copied().enumerate() {
        let tag = (type_mask >> (index * 2)) & 0b11;
        let value = match tag {
            0 => match Value::int_checked(raw) {
                Ok(value) => value,
                Err(_) => {
                    return fail(vm, vm.runtime_error(RuntimeErrorKind::IntegerOverflow));
                }
            },
            1 => Value::float(f64::from_bits(u64::from_ne_bytes(raw.to_ne_bytes()))),
            2 => Value::bool(raw != 0),
            _ => {
                return fail(
                    vm,
                    vm.runtime_error(RuntimeErrorKind::InvalidBytecode(
                        "JIT native-call argument has an invalid type tag".to_string(),
                    )),
                );
            }
        };
        values.push(value);
    }

    let index = match usize::try_from(global_index) {
        Ok(index) => index,
        Err(_) => {
            return fail(
                vm,
                vm.runtime_error(RuntimeErrorKind::InvalidBytecode(
                    "JIT global index does not fit this target".to_string(),
                )),
            );
        }
    };
    let Some(function) = vm.globals_by_index.get(index).copied() else {
        return fail(
            vm,
            vm.runtime_error(RuntimeErrorKind::UndefinedVariable(format!(
                "global index {index}"
            ))),
        );
    };
    let value = match vm.call_value(function, &values) {
        Ok(value) => value,
        Err(error) => return fail(vm, error),
    };
    match result_type {
        0 => value.as_int().unwrap_or_else(|| {
            fail(
                vm,
                vm.runtime_error(RuntimeErrorKind::TypeError {
                    operation: "JIT native call",
                    expected: "int",
                    got: vm.value_type_name(value).to_string(),
                }),
            );
            0
        }),
        1 => value.as_float().map_or_else(
            || {
                fail(
                    vm,
                    vm.runtime_error(RuntimeErrorKind::TypeError {
                        operation: "JIT native call",
                        expected: "float",
                        got: vm.value_type_name(value).to_string(),
                    }),
                )
            },
            |value| i64::from_ne_bytes(value.to_bits().to_ne_bytes()),
        ),
        2 => value.as_bool().map_or_else(
            || {
                fail(
                    vm,
                    vm.runtime_error(RuntimeErrorKind::TypeError {
                        operation: "JIT native call",
                        expected: "bool",
                        got: vm.value_type_name(value).to_string(),
                    }),
                )
            },
            i64::from,
        ),
        _ => fail(
            vm,
            vm.runtime_error(RuntimeErrorKind::InvalidBytecode(
                "JIT native-call result has an invalid type tag".to_string(),
            )),
        ),
    }
}
