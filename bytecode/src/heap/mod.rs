// gc heap for bytecode constants and runtime objects

mod access;
mod alloc;
mod gc;
mod ints;
mod reclaim;
mod strings;

use crate::object::{GcObject, GcRef};
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeapGeneration {
    Young,
    Old,
}

const YOUNG_MARK: u8 = 1;
const REMEMBERED: u8 = 2;
const WHOLE_OBJECT: u32 = u32::MAX;

struct HeapSlot {
    generation: u32,
    card_table: u32,
    heap_generation: HeapGeneration,
    flags: u8,
    object: Option<GcObject>,
}

#[derive(Debug, Clone, Copy)]
struct RememberedEntry {
    owner: GcRef,
    element: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MajorPhase {
    Idle,
    Mark,
    Sweep,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MajorSliceResult {
    Idle,
    InProgress,
    Complete { freed: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MajorStep {
    Idle,
    BudgetExhausted,
    NeedsRoots,
    Complete { freed: usize },
}

pub struct Heap {
    // frames cache pointers into function objects for a whole run, so growing the slot table must not move them
    #[allow(clippy::vec_box)]
    objects: Vec<Box<HeapSlot>>,
    free_list: Vec<u32>,
    young: Vec<u32>,
    remembered: Vec<RememberedEntry>,
    card_tables: Vec<Vec<u64>>,
    free_card_tables: Vec<u32>,
    gray: Vec<(GcRef, u32)>,
    young_stack: Vec<GcRef>,
    scratch: Vec<GcRef>,
    phase: MajorPhase,
    sweep_cursor: usize,
    sweep_freed: usize,
    bytes_allocated: usize,
    peak_bytes: usize,
    alloc_clock: u64,
    minor_at: u64,
    slice_at: u64,
    next_event: u64,
    nursery_bytes: u64,
    nursery_limit: u64,
    minor_cost_rate: u64,
    slice_interval: u64,
    max_heap: usize,
    verify: bool,
    next_gc: usize,
    slice_work: usize,
    live_objects: usize,
    stress: bool,
    intern_table: HashMap<u64, GcRef>, // string interning
    intern_keys: HashMap<GcRef, u64>,
    minor_collections: u64,
    major_collections: u64,
    allocation_count: u64,
    boxed_ints: bool,
    pause_work: u64,
}

impl Heap {
    pub const INITIAL_GC_THRESHOLD: usize = 4 * 1024 * 1024;
    pub const DEFAULT_NURSERY_BYTES: u64 = 256 * 1024;
    pub const MIN_NURSERY_BYTES: u64 = 1024;
    pub const ADAPTIVE_NURSERY_FLOOR: u64 = 16 * 1024;
    pub const MINOR_TARGET_NANOS: u64 = 1_000_000;
    pub const REMEMBERED_LIMIT: usize = 4096;
    const GC_GROWTH_FACTOR: usize = 2;
    const SLICE_BYTES: u64 = 32 * 1024;
    const MIN_SLICE_INTERVAL: u64 = 2 * 1024;
    const MIN_SLICE_WORK: usize = 4096;
    const STRESS_SLICE_WORK: usize = 16;
    const MAX_SLICE_WORK: usize = 1 << 20;
    const MARK_CHUNK: usize = 2048;
    const DEADLINE_STRIDE: usize = 512;
    const RECLAIM_BYTES: usize = 64 * 1024;
    pub const PRETENURE_BYTES: usize = 64 * 1024;

    pub fn new() -> Self {
        let mut heap = Self {
            objects: Vec::new(),
            free_list: Vec::new(),
            young: Vec::new(),
            remembered: Vec::new(),
            card_tables: vec![Vec::new()],
            free_card_tables: Vec::new(),
            gray: Vec::new(),
            young_stack: Vec::new(),
            scratch: Vec::new(),
            phase: MajorPhase::Idle,
            sweep_cursor: 0,
            sweep_freed: 0,
            bytes_allocated: 0,
            peak_bytes: 0,
            alloc_clock: 0,
            minor_at: Self::DEFAULT_NURSERY_BYTES,
            slice_at: u64::MAX,
            next_event: 0,
            nursery_bytes: Self::DEFAULT_NURSERY_BYTES,
            nursery_limit: Self::DEFAULT_NURSERY_BYTES,
            minor_cost_rate: 0,
            slice_interval: Self::SLICE_BYTES,
            max_heap: usize::MAX,
            verify: false,
            next_gc: Self::INITIAL_GC_THRESHOLD,
            slice_work: Self::MIN_SLICE_WORK,
            live_objects: 0,
            stress: false,
            intern_table: HashMap::new(),
            intern_keys: HashMap::new(),
            minor_collections: 0,
            major_collections: 0,
            allocation_count: 0,
            boxed_ints: false,
            pause_work: 0,
        };
        heap.update_next_event();
        reclaim::start();
        heap
    }

    pub fn estimate_string_size(len: usize) -> usize {
        std::mem::size_of::<crate::object::AelysString>() + len
    }

    pub fn set_nursery_bytes(&mut self, bytes: u64) {
        self.nursery_limit = bytes.max(Self::MIN_NURSERY_BYTES);
        self.nursery_bytes = (self.nursery_limit / 4).max(self.nursery_floor());
        self.minor_at = self.alloc_clock.saturating_add(self.nursery_bytes);
        self.update_next_event();
    }

    fn nursery_floor(&self) -> u64 {
        Self::ADAPTIVE_NURSERY_FLOOR.min(self.nursery_limit)
    }

    pub fn note_minor_cost(&mut self, nanos: u64) {
        if self.stress {
            return;
        }
        let measured = nanos.saturating_mul(1000) / self.nursery_bytes.max(1);
        self.minor_cost_rate = measured.max(self.minor_cost_rate - self.minor_cost_rate / 16);
        let next =
            (Self::MINOR_TARGET_NANOS / 2).saturating_mul(1000) / self.minor_cost_rate.max(1);
        self.nursery_bytes = next.clamp(self.nursery_floor(), self.nursery_limit);
        self.minor_at = self.alloc_clock.saturating_add(self.nursery_bytes);
        self.update_next_event();
    }

    pub fn nursery_limit(&self) -> u64 {
        self.nursery_limit
    }

    pub fn set_max_heap_bytes(&mut self, bytes: u64) {
        self.max_heap = usize::try_from(bytes).unwrap_or(usize::MAX);
        self.next_gc = self.next_gc.min(self.max_heap / 2);
    }

    pub fn set_verify(&mut self, verify: bool) {
        self.verify = verify;
    }

    pub fn set_stress(&mut self, stress: bool) {
        self.stress = stress;
        if stress {
            self.set_nursery_bytes(Self::MIN_NURSERY_BYTES);
        }
    }

    pub fn nursery_bytes(&self) -> u64 {
        self.nursery_bytes
    }

    pub fn major_collection_active(&self) -> bool {
        self.phase != MajorPhase::Idle
    }

    pub fn major_marking(&self) -> bool {
        self.phase == MajorPhase::Mark
    }

    #[inline]
    pub fn gc_work_due(&self) -> bool {
        self.alloc_clock >= self.next_event
    }

    pub fn minor_due(&self) -> bool {
        self.alloc_clock >= self.minor_at
    }

    pub fn major_slice_due(&self) -> bool {
        self.phase != MajorPhase::Idle && self.alloc_clock >= self.slice_at
    }

    pub fn major_due(&self) -> bool {
        self.phase == MajorPhase::Idle && (self.stress || self.bytes_allocated >= self.next_gc)
    }

    pub fn slice_work_budget(&self) -> usize {
        self.slice_work
    }

    pub fn note_major_slice(&mut self, kept_pace: bool) {
        if self.phase == MajorPhase::Idle {
            return;
        }
        self.slice_interval = if self.stress {
            1
        } else if kept_pace {
            self.slice_interval.saturating_mul(2).min(Self::SLICE_BYTES)
        } else {
            (self.slice_interval / 2).max(Self::MIN_SLICE_INTERVAL)
        };
        self.slice_at = self.alloc_clock.saturating_add(self.slice_interval);
        self.update_next_event();
    }

    fn update_next_event(&mut self) {
        self.next_event = if self.phase == MajorPhase::Idle {
            self.minor_at
        } else {
            self.minor_at.min(self.slice_at)
        };
    }

    pub fn begin_major_collection(&mut self, roots: Vec<GcRef>) -> bool {
        if self.major_collection_active() {
            return false;
        }
        self.phase = MajorPhase::Mark;
        self.gray.clear();
        let slots = self.objects.len();
        let estimated_work = slots.saturating_mul(2).saturating_add(1024);
        let headroom = u64::try_from(self.max_heap.saturating_sub(self.bytes_allocated) / 2)
            .unwrap_or(u64::MAX);
        let budget_bytes = u64::try_from(self.bytes_allocated)
            .unwrap_or(u64::MAX)
            .max(self.nursery_limit.saturating_mul(8))
            .min(headroom)
            .max(Self::SLICE_BYTES);
        self.slice_interval = Self::SLICE_BYTES;
        let slices = usize::try_from((budget_bytes / Self::SLICE_BYTES).max(1)).unwrap_or(1);
        self.slice_work = if self.stress {
            Self::STRESS_SLICE_WORK
        } else {
            (estimated_work / slices).clamp(Self::MIN_SLICE_WORK, Self::MAX_SLICE_WORK)
        };
        self.slice_at = self.alloc_clock;
        self.update_next_event();
        for root in roots {
            self.pause_work += 1;
            self.shade(root);
        }
        true
    }

    pub fn add_major_roots(&mut self, roots: impl IntoIterator<Item = GcRef>) -> bool {
        if self.phase != MajorPhase::Mark {
            return false;
        }
        let before = self.gray.len();
        for root in roots {
            self.pause_work += 1;
            self.shade(root);
        }
        self.gray.len() > before
    }

    pub fn take_pause_work(&mut self) -> u64 {
        std::mem::take(&mut self.pause_work)
    }

    pub fn major_worklist_len(&self) -> usize {
        self.gray.len()
    }

    pub fn finish_marking(&mut self) {
        if self.phase == MajorPhase::Mark && self.gray.is_empty() {
            self.phase = MajorPhase::Sweep;
            self.sweep_cursor = 0;
            self.sweep_freed = 0;
        }
    }

    pub fn major_step(&mut self, budget: &mut usize, deadline: Option<Instant>) -> MajorStep {
        let mut spent = 0usize;
        let mut next_check = Self::DEADLINE_STRIDE;
        let result = self.major_step_inner(*budget, deadline, &mut spent, &mut next_check);
        self.pause_work = self
            .pause_work
            .saturating_add(u64::try_from(spent).unwrap_or(u64::MAX));
        *budget = budget.saturating_sub(spent);
        result
    }

    fn major_step_inner(
        &mut self,
        budget: usize,
        deadline: Option<Instant>,
        spent: &mut usize,
        next_check: &mut usize,
    ) -> MajorStep {
        if budget == 0 {
            return MajorStep::BudgetExhausted;
        }
        loop {
            match self.phase {
                MajorPhase::Idle => return MajorStep::Idle,
                MajorPhase::Mark => {
                    let Some(entry) = self.gray.pop() else {
                        return MajorStep::NeedsRoots;
                    };
                    *spent = spent.saturating_add(self.scan_gray(entry));
                }
                MajorPhase::Sweep => {
                    if self.sweep_cursor >= self.objects.len() {
                        let freed = self.sweep_freed;
                        self.finish_major_collection();
                        return MajorStep::Complete { freed };
                    }
                    let index = self.sweep_cursor;
                    self.sweep_cursor += 1;
                    if self.sweep_major_slot(index) {
                        self.sweep_freed += 1;
                    }
                    *spent += 1;
                }
            }
            if *spent >= budget {
                return MajorStep::BudgetExhausted;
            }
            if *spent >= *next_check {
                *next_check = spent.saturating_add(Self::DEADLINE_STRIDE);
                if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                    return MajorStep::BudgetExhausted;
                }
            }
        }
    }

    pub fn major_collection_slice(&mut self, budget: Duration) -> MajorSliceResult {
        if !self.major_collection_active() {
            return MajorSliceResult::Idle;
        }
        let mut units = if budget.is_zero() { 1 } else { usize::MAX };
        let deadline = Instant::now().checked_add(budget);
        loop {
            match self.major_step(&mut units, deadline) {
                MajorStep::Idle => return MajorSliceResult::Idle,
                MajorStep::BudgetExhausted => return MajorSliceResult::InProgress,
                MajorStep::NeedsRoots => self.finish_marking(),
                MajorStep::Complete { freed } => return MajorSliceResult::Complete { freed },
            }
        }
    }
}

impl Default for Heap {
    fn default() -> Self {
        Self::new()
    }
}

// clone gives you a fresh heap, not a copy (objects aren't clonable)
impl Clone for Heap {
    fn clone(&self) -> Self {
        Self::new()
    }
}
