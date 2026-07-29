use crate::{Function, Value};
use smallvec::SmallVec;
use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Arc;

pub const JIT_TIER1_BACKEDGE_THRESHOLD: u64 = 10_000;

pub(crate) struct InlineMap<K, V> {
    inline: SmallVec<[(K, V); 4]>,
    heap: Option<HashMap<K, V>>,
}

impl<K, V> Default for InlineMap<K, V> {
    fn default() -> Self {
        Self {
            inline: SmallVec::new(),
            heap: None,
        }
    }
}

impl<K: Eq + Hash, V> InlineMap<K, V> {
    pub(crate) fn get(&self, key: &K) -> Option<&V> {
        if let Some(heap) = &self.heap {
            return heap.get(key);
        }
        self.inline
            .iter()
            .find_map(|(candidate, value)| (candidate == key).then_some(value))
    }

    pub(crate) fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        if let Some(heap) = &mut self.heap {
            return heap.get_mut(key);
        }
        self.inline
            .iter_mut()
            .find_map(|(candidate, value)| (candidate == key).then_some(value))
    }

    pub(crate) fn clear(&mut self) {
        self.inline.clear();
        if let Some(heap) = &mut self.heap {
            heap.clear();
        }
    }

    pub(crate) fn retain(&mut self, mut keep: impl FnMut(&K, &V) -> bool) {
        if let Some(heap) = &mut self.heap {
            heap.retain(|key, value| keep(key, value));
        } else {
            self.inline.retain(|(key, value)| keep(key, value));
        }
    }
}

impl<K: Eq + Hash, V> InlineMap<K, V> {
    pub(crate) fn insert(&mut self, key: K, value: V) {
        if let Some(heap) = &mut self.heap {
            heap.insert(key, value);
            return;
        }
        if let Some(existing) = self.get_mut(&key) {
            *existing = value;
            return;
        }
        if self.inline.len() < self.inline.inline_size() {
            self.inline.push((key, value));
            return;
        }
        let mut heap = HashMap::with_capacity(self.inline.len().saturating_mul(2));
        heap.extend(std::mem::take(&mut self.inline));
        heap.insert(key, value);
        self.heap = Some(heap);
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct JitFunctionKey {
    module: u64,
    path: SmallVec<[u32; 4]>,
}

impl JitFunctionKey {
    pub fn root(module: u64) -> Self {
        Self {
            module,
            path: SmallVec::from_slice(&[0]),
        }
    }

    pub fn child(&self, index: u32) -> Self {
        let mut path = self.path.clone();
        path.push(index);
        Self {
            module: self.module,
            path,
        }
    }

    pub fn module(&self) -> u64 {
        self.module
    }

    pub fn path(&self) -> &[u32] {
        &self.path
    }

    pub fn shared_path(&self) -> Arc<[u32]> {
        Arc::from(self.path.as_slice())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum JitCallResult {
    Unsupported,
    Returned(Value),
    Deoptimized {
        bytecode_ip: u32,
        registers: Vec<(u16, JitDeoptValue)>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum JitDeoptValue {
    Value(Value),
    Argument(usize),
}

#[derive(Clone, Copy, Debug)]
pub enum JitArgument<'a> {
    Integer(i64),
    IntegerArray(&'a [i64]),
    IntegerVec(&'a [i64]),
}

pub trait JitExecutor: Send + Sync {
    fn should_execute(&self, key: &JitFunctionKey, calls: u64) -> bool;

    fn observe_backedge(&self, key: &JitFunctionKey, function: &Function, backedges: u64);

    fn try_execute(
        &self,
        key: &JitFunctionKey,
        function: &Function,
        arguments: &[JitArgument<'_>],
        calls: u64,
    ) -> JitCallResult;
}
