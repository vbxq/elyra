use super::worker::FrameJit;
use aelys_runtime::{FrameRequest, FrameSubmission, JitExecutor};
use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) struct JitProvider {
    call_threshold: u64,
    deoptimizations: AtomicU64,
    osr_executions: AtomicU64,
    frames: FrameJit,
}

impl JitProvider {
    pub(crate) fn new(
        call_threshold: u64,
        asynchronous: bool,
        fault: Option<String>,
    ) -> Result<Self, String> {
        Ok(Self {
            call_threshold,
            deoptimizations: AtomicU64::new(0),
            osr_executions: AtomicU64::new(0),
            frames: FrameJit::new(asynchronous, fault)?,
        })
    }

    pub(crate) fn cache_len(&self) -> usize {
        self.frames.len()
    }

    pub(crate) fn deoptimizations(&self) -> u64 {
        self.deoptimizations.load(Ordering::Relaxed)
    }

    pub(crate) fn osr_executions(&self) -> u64 {
        self.osr_executions.load(Ordering::Relaxed)
    }
}

impl JitExecutor for JitProvider {
    fn submit_frame(&self, request: FrameRequest<'_>) -> FrameSubmission {
        self.frames.submit(request)
    }

    fn frame_call_threshold(&self) -> u64 {
        self.call_threshold
    }

    fn record_frame_osr_return(&self) {
        self.osr_executions.fetch_add(1, Ordering::Relaxed);
    }

    fn record_frame_exit(&self) {
        self.deoptimizations.fetch_add(1, Ordering::Relaxed);
    }
}
