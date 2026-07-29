use super::core::{InlineCacheKey, InlineCallCacheEntry};
use super::{GcRef, VM};

impl VM {
    #[inline(never)]
    pub(crate) fn probe_inline_call_cache(
        &mut self,
        key: InlineCacheKey,
        global_index: usize,
        global_generation: u64,
        target: GcRef,
    ) -> bool {
        let target_alive = self.heap.get(target).is_some();
        let valid = |entry: &InlineCallCacheEntry| {
            target_alive
                && entry.global_index == global_index
                && entry.global_generation == global_generation
                && entry.target == target
        };
        let hit = self
            .last_inline_call_cache
            .as_ref()
            .is_some_and(|(cached_key, entry)| *cached_key == key && valid(entry))
            || self.inline_call_cache.get(&key).is_some_and(valid);
        let entry = InlineCallCacheEntry {
            global_index,
            global_generation,
            target,
        };
        if !hit {
            self.inline_call_cache.insert(key, entry);
        }
        self.last_inline_call_cache = Some((key, entry));
        hit
    }
}
