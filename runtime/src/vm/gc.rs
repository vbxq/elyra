use super::{GcRef, VM};
use aelys_bytecode::MajorStep;
use std::fmt::Write as _;
use std::time::{Duration, Instant};

const PAUSE_BUCKET_NANOS: u64 = 10_000;
const PAUSE_BUCKETS: usize = 2048;
const HISTOGRAM_EDGES_US: [u64; 10] = [50, 100, 250, 500, 1000, 2000, 4000, 8000, 16000, 32000];

#[derive(Debug, Clone)]
struct PauseHistogram {
    max_nanos: u64,
    buckets: Box<[u64]>,
}

impl Default for PauseHistogram {
    fn default() -> Self {
        Self {
            max_nanos: 0,
            buckets: vec![0; PAUSE_BUCKETS + 1].into_boxed_slice(),
        }
    }
}

impl PauseHistogram {
    fn record(&mut self, nanos: u64) {
        self.max_nanos = self.max_nanos.max(nanos);
        let bucket = usize::try_from(nanos / PAUSE_BUCKET_NANOS)
            .unwrap_or(PAUSE_BUCKETS)
            .min(PAUSE_BUCKETS);
        self.buckets[bucket] = self.buckets[bucket].saturating_add(1);
    }

    fn percentile_nanos(&self, count: u64, percentile: f64) -> u64 {
        if count == 0 {
            return 0;
        }
        let wanted = ((count as f64) * percentile / 100.0).ceil().max(1.0) as u64;
        let mut seen = 0u64;
        for (bucket, hits) in self.buckets.iter().enumerate() {
            seen = seen.saturating_add(*hits);
            if seen >= wanted {
                if bucket == PAUSE_BUCKETS {
                    return self.max_nanos;
                }
                let upper = (bucket as u64 + 1) * PAUSE_BUCKET_NANOS;
                return upper.min(self.max_nanos);
            }
        }
        self.max_nanos
    }

    fn rows(&self) -> Vec<(String, u64)> {
        let mut rows = Vec::new();
        let mut lower = 0u64;
        for upper in HISTOGRAM_EDGES_US {
            rows.push((
                format!("[{lower},{upper})"),
                self.count_between_us(lower, upper),
            ));
            lower = upper;
        }
        rows.push((
            format!("[{lower},inf)"),
            self.count_between_us(lower, u64::MAX),
        ));
        rows
    }

    fn count_between_us(&self, lower_us: u64, upper_us: u64) -> u64 {
        let first = usize::try_from(lower_us.saturating_mul(1000) / PAUSE_BUCKET_NANOS)
            .unwrap_or(PAUSE_BUCKETS)
            .min(PAUSE_BUCKETS + 1);
        let last = usize::try_from(upper_us.saturating_mul(1000) / PAUSE_BUCKET_NANOS)
            .unwrap_or(PAUSE_BUCKETS + 1)
            .min(PAUSE_BUCKETS + 1);
        self.buckets[first..last].iter().sum()
    }
}

#[derive(Debug, Clone, Default)]
pub struct GcPauseStats {
    count: u64,
    total_nanos: u64,
    window_max_nanos: u64,
    wall: PauseHistogram,
    cpu: PauseHistogram,
}

impl GcPauseStats {
    fn record(&mut self, nanos: u64, cpu_nanos: u64) {
        self.count = self.count.saturating_add(1);
        self.total_nanos = self.total_nanos.saturating_add(nanos);
        self.window_max_nanos = self.window_max_nanos.max(nanos);
        self.wall.record(nanos);
        self.cpu.record(cpu_nanos);
    }

    pub fn count(&self) -> u64 {
        self.count
    }

    pub fn total_nanos(&self) -> u64 {
        self.total_nanos
    }

    pub fn max_nanos(&self) -> u64 {
        self.wall.max_nanos
    }

    pub fn max_cpu_nanos(&self) -> u64 {
        self.cpu.max_nanos
    }

    pub fn take_window_max_nanos(&mut self) -> u64 {
        std::mem::take(&mut self.window_max_nanos)
    }

    pub fn percentile_nanos(&self, percentile: f64) -> u64 {
        self.wall.percentile_nanos(self.count, percentile)
    }

    pub fn cpu_percentile_nanos(&self, percentile: f64) -> u64 {
        self.cpu.percentile_nanos(self.count, percentile)
    }

    pub fn histogram(&self) -> Vec<(String, u64)> {
        self.wall.rows()
    }

    pub fn cpu_histogram(&self) -> Vec<(String, u64)> {
        self.cpu.rows()
    }
}

fn thread_cpu_nanos() -> u64 {
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: clock_gettime only writes the timespec passed by pointer.
    let status = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut time) };
    if status != 0 {
        return 0;
    }
    u64::try_from(time.tv_sec)
        .unwrap_or(0)
        .saturating_mul(1_000_000_000)
        .saturating_add(u64::try_from(time.tv_nsec).unwrap_or(0))
}

struct PauseClock {
    wall: Instant,
    cpu: u64,
}

impl PauseClock {
    fn start() -> Self {
        Self {
            wall: Instant::now(),
            cpu: thread_cpu_nanos(),
        }
    }
}

impl VM {
    const GC_SLICE_DEADLINE: Duration = Duration::from_micros(1000);

    pub fn maybe_collect(&mut self) {
        self.maybe_collect_for(0);
    }

    #[inline]
    pub(crate) fn maybe_collect_for(&mut self, additional: u64) {
        let projected = u64::try_from(self.heap.bytes_allocated())
            .unwrap_or(u64::MAX)
            .saturating_add(additional);
        if projected > self.config.max_heap_bytes {
            self.collect();
            return;
        }
        if self.heap.gc_work_due() {
            self.run_due_gc_work();
        }
    }

    #[cold]
    #[inline(never)]
    fn run_due_gc_work(&mut self) {
        let started = PauseClock::start();
        if self.heap.minor_due() {
            self.minor_collection();
            if self.heap.major_due() {
                let roots = self.root_refs();
                self.heap.begin_major_collection(roots);
            }
        } else if self.heap.major_slice_due() {
            self.heap.note_major_slice();
            let mut budget = self.heap.slice_work_budget();
            self.major_work(
                &mut budget,
                started.wall.checked_add(Self::GC_SLICE_DEADLINE),
            );
        }
        self.record_gc_pause(started);
    }

    pub fn collect(&mut self) {
        let started = PauseClock::start();
        self.minor_collection();
        if !self.heap.major_collection_active() {
            let roots = self.root_refs();
            self.heap.begin_major_collection(roots);
        }
        while self.heap.major_collection_active() {
            let mut budget = usize::MAX;
            self.major_work(&mut budget, None);
        }
        self.record_gc_pause(started);
    }

    pub fn collect_minor(&mut self) {
        let started = PauseClock::start();
        self.minor_collection();
        self.record_gc_pause(started);
    }

    fn minor_collection(&mut self) {
        let roots = self.root_refs();
        self.heap.mark_young(roots);
        self.heap.sweep_young();
        self.sweep_jit_metadata();
        self.globals_by_index_cache.clear();
        self.execution_stats.collections = self.execution_stats.collections.saturating_add(1);
        self.execution_stats.minor_collections =
            self.execution_stats.minor_collections.saturating_add(1);
    }

    fn major_work(&mut self, budget: &mut usize, deadline: Option<Instant>) {
        loop {
            match self.heap.major_step(budget, deadline) {
                MajorStep::Idle | MajorStep::BudgetExhausted => return,
                MajorStep::NeedsRoots => {
                    let roots = self.root_refs();
                    if !self.heap.add_major_roots(roots) {
                        self.heap.finish_marking();
                    }
                }
                MajorStep::Complete { .. } => {
                    self.sweep_jit_metadata();
                    self.globals_by_index_cache.clear();
                    self.execution_stats.collections =
                        self.execution_stats.collections.saturating_add(1);
                    self.execution_stats.major_collections =
                        self.execution_stats.major_collections.saturating_add(1);
                    return;
                }
            }
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
        roots.extend(self.host_roots.references());
        roots
    }

    fn record_gc_pause(&mut self, started: PauseClock) {
        let nanos = u64::try_from(started.wall.elapsed().as_nanos()).unwrap_or(u64::MAX);
        let cpu_nanos = thread_cpu_nanos().saturating_sub(started.cpu);
        let micros = nanos / 1000;
        self.execution_stats.gc_pause_micros =
            self.execution_stats.gc_pause_micros.saturating_add(micros);
        self.execution_stats.gc_max_pause_micros =
            self.execution_stats.gc_max_pause_micros.max(micros);
        self.execution_stats.gc_pause_nanos =
            self.execution_stats.gc_pause_nanos.saturating_add(nanos);
        self.execution_stats.gc_max_pause_nanos =
            self.execution_stats.gc_max_pause_nanos.max(nanos);
        self.gc_pauses.record(nanos, cpu_nanos);
    }

    pub fn gc_pause_stats(&self) -> &GcPauseStats {
        &self.gc_pauses
    }

    pub fn take_gc_window_max_pause_nanos(&mut self) -> u64 {
        self.gc_pauses.take_window_max_nanos()
    }

    pub fn gc_stats_report(&self) -> String {
        let pauses = &self.gc_pauses;
        let mut report = format!(
            "gc-stats: collections={} minor={} major={} pauses={} max_pause_us={} p99_pause_us={} max_pause_cpu_us={} p99_pause_cpu_us={} total_pause_us={} allocations={} allocated_bytes={} heap_bytes={} peak_heap_bytes={} live_objects={}",
            self.heap
                .minor_collection_count()
                .saturating_add(self.heap.major_collection_count()),
            self.heap.minor_collection_count(),
            self.heap.major_collection_count(),
            pauses.count(),
            pauses.max_nanos() / 1000,
            pauses.percentile_nanos(99.0) / 1000,
            pauses.max_cpu_nanos() / 1000,
            pauses.cpu_percentile_nanos(99.0) / 1000,
            pauses.total_nanos() / 1000,
            self.heap.allocation_count(),
            self.heap.total_allocated_bytes(),
            self.heap.bytes_allocated(),
            self.heap.peak_bytes(),
            self.heap.object_count(),
        );
        report.push_str("\ngc-stats: pause histogram (us)");
        for (label, count) in pauses.histogram() {
            let _ = write!(report, " {label}={count}");
        }
        report.push_str("\ngc-stats: pause cpu-time histogram (us)");
        for (label, count) in pauses.cpu_histogram() {
            let _ = write!(report, " {label}={count}");
        }
        report
    }
}

impl Drop for VM {
    fn drop(&mut self) {
        if self.config.gc_stats {
            eprintln!("{}", self.gc_stats_report());
        }
    }
}
