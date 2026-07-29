use super::{GcRef, VM};
use aelys_bytecode::MajorSliceResult;
use std::time::{Duration, Instant};

impl VM {
    const GC_SLICE_BUDGET: Duration = Duration::from_micros(500);

    pub fn maybe_collect(&mut self) {
        self.maybe_collect_for(0);
    }

    pub(crate) fn maybe_collect_for(&mut self, additional: u64) {
        let projected = u64::try_from(self.heap.bytes_allocated())
            .unwrap_or(u64::MAX)
            .saturating_add(additional);
        if projected > self.config.max_heap_bytes {
            self.collect();
            return;
        }
        if self.heap.major_collection_active() {
            self.run_major_slice();
            return;
        }
        if !self.heap.should_collect() {
            return;
        }
        if self.heap.minor_collection_count() % 8 == 7 {
            let roots = self.root_refs();
            self.heap.begin_major_collection(roots);
            self.run_major_slice();
        } else {
            self.collect_minor();
        }
    }

    pub fn collect(&mut self) {
        let roots = self.root_refs();
        if !self.heap.begin_major_collection(roots.clone()) {
            self.heap.add_major_roots(roots);
        }
        while self.heap.major_collection_active() {
            self.run_major_slice();
        }
    }

    pub fn collect_minor(&mut self) {
        if self.heap.major_collection_active() {
            self.collect();
            return;
        }
        let started = Instant::now();
        self.heap.mark_young(self.root_refs());
        self.heap.sweep_young();
        self.sweep_jit_metadata();
        self.globals_by_index_cache.clear();
        self.record_gc_slice(started);
        self.execution_stats.collections = self.execution_stats.collections.saturating_add(1);
        self.execution_stats.minor_collections =
            self.execution_stats.minor_collections.saturating_add(1);
    }

    fn run_major_slice(&mut self) {
        let started = Instant::now();
        let result = self.heap.major_collection_slice(Self::GC_SLICE_BUDGET);
        self.record_gc_slice(started);
        if matches!(result, MajorSliceResult::Complete { .. }) {
            self.sweep_jit_metadata();
            self.globals_by_index_cache.clear();
            self.execution_stats.collections = self.execution_stats.collections.saturating_add(1);
            self.execution_stats.major_collections =
                self.execution_stats.major_collections.saturating_add(1);
        }
    }

    fn root_refs(&self) -> Vec<GcRef> {
        let mut roots = Vec::new();
        for frame in &self.frames {
            let base = frame.base;
            let count = usize::try_from(frame.num_registers).unwrap_or(usize::MAX);
            roots.extend(
                (0..count)
                    .filter_map(|offset| self.registers.get(base + offset))
                    .filter_map(|value| value.as_ptr().map(GcRef::new)),
            );
            roots.push(frame.function());
        }
        roots.extend(
            self.globals
                .values()
                .filter_map(|value| value.as_ptr().map(GcRef::new)),
        );
        roots.extend(
            self.globals_by_index
                .iter()
                .filter_map(|value| value.as_ptr().map(GcRef::new)),
        );
        roots.extend(self.open_upvalues.iter().copied());
        roots.extend(self.current_upvalues.iter().copied());
        roots
    }

    fn record_gc_slice(&mut self, started: Instant) {
        let micros = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);
        self.execution_stats.gc_pause_micros =
            self.execution_stats.gc_pause_micros.saturating_add(micros);
        self.execution_stats.gc_max_pause_micros =
            self.execution_stats.gc_max_pause_micros.max(micros);
    }
}
