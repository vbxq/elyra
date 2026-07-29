// gc heap for bytecode constants and runtime objects

mod access;
mod alloc;
mod gc;
mod strings;

use crate::object::{GcObject, GcRef};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeapGeneration {
    Young,
    Old,
}

struct HeapSlot {
    generation: u16,
    heap_generation: HeapGeneration,
    survival_count: u8,
    object: Option<GcObject>,
}

enum MajorCollection {
    Idle,
    Mark { worklist: Vec<GcRef> },
    Sweep { cursor: usize, freed: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MajorSliceResult {
    Idle,
    InProgress,
    Complete { freed: usize },
}

pub struct Heap {
    objects: Vec<HeapSlot>,
    free_list: Vec<u32>,
    bytes_allocated: usize,
    next_gc: usize,
    intern_table: HashMap<u64, GcRef>, // string interning
    remembered_set: HashSet<u32>,
    minor_collections: u64,
    major_collections: u64,
    major_collection: MajorCollection,
    allocation_count: u64,
}

impl Heap {
    pub const INITIAL_GC_THRESHOLD: usize = 1024 * 1024; // 1MB
    const GC_GROWTH_FACTOR: usize = 2;

    pub fn new() -> Self {
        Self {
            objects: Vec::new(),
            free_list: Vec::new(),
            bytes_allocated: 0,
            next_gc: Self::INITIAL_GC_THRESHOLD,
            intern_table: HashMap::new(),
            remembered_set: HashSet::new(),
            minor_collections: 0,
            major_collections: 0,
            major_collection: MajorCollection::Idle,
            allocation_count: 0,
        }
    }

    pub fn estimate_string_size(len: usize) -> usize {
        std::mem::size_of::<crate::object::AelysString>() + len
    }

    pub fn major_collection_active(&self) -> bool {
        !matches!(self.major_collection, MajorCollection::Idle)
    }

    pub fn begin_major_collection(&mut self, roots: Vec<GcRef>) -> bool {
        if self.major_collection_active() {
            return false;
        }
        self.major_collection = MajorCollection::Mark { worklist: roots };
        true
    }

    pub fn add_major_roots(&mut self, roots: impl IntoIterator<Item = GcRef>) {
        if let MajorCollection::Mark { worklist } = &mut self.major_collection {
            worklist.extend(roots);
        }
    }

    pub fn major_collection_slice(&mut self, budget: Duration) -> MajorSliceResult {
        if !self.major_collection_active() {
            return MajorSliceResult::Idle;
        }

        let started = Instant::now();
        loop {
            let root = match &mut self.major_collection {
                MajorCollection::Mark { worklist } => worklist.pop(),
                MajorCollection::Idle | MajorCollection::Sweep { .. } => None,
            };

            if let Some(root) = root {
                let children = self.mark_one(root);
                if let MajorCollection::Mark { worklist } = &mut self.major_collection {
                    worklist.extend(children);
                }
            } else if matches!(self.major_collection, MajorCollection::Mark { .. }) {
                self.major_collection = MajorCollection::Sweep {
                    cursor: 0,
                    freed: 0,
                };
            } else {
                let next = match self.major_collection {
                    MajorCollection::Sweep { cursor, .. } if cursor < self.objects.len() => {
                        Some(cursor)
                    }
                    MajorCollection::Sweep { freed, .. } => {
                        self.finish_major_collection();
                        return MajorSliceResult::Complete { freed };
                    }
                    MajorCollection::Idle | MajorCollection::Mark { .. } => None,
                };
                if let Some(index) = next {
                    let freed_slot = self.sweep_slot(index, false);
                    if let MajorCollection::Sweep { cursor, freed } = &mut self.major_collection {
                        *cursor += 1;
                        *freed += usize::from(freed_slot);
                    }
                }
            }

            if started.elapsed() >= budget {
                return MajorSliceResult::InProgress;
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
