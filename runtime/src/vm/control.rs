use super::{GcRef, ObjectKind, VM};
use aelys_common::error::{RuntimeError, RuntimeErrorKind};
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
        self.execution_control.report
            || self.execution_control.max_instructions.is_some()
            || self.execution_control.deadline.is_some()
            || self.execution_control.interrupt.is_some()
    }

    pub fn configure_execution(&mut self, control: ExecutionControl) {
        self.execution_control = control;
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
