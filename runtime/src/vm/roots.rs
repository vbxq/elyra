use super::GcRef;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// A reference held by embedding code and therefore outside the VM's normal
/// register/global root graph.
#[derive(Debug)]
pub struct HostRoot {
    set: Arc<HostRootSet>,
    reference: GcRef,
    owner_id: u64,
}

#[derive(Debug)]
pub(crate) struct HostRootSet {
    references: Mutex<HashMap<GcRef, usize>>,
}

impl HostRootSet {
    pub(crate) fn new() -> Self {
        Self {
            references: Mutex::new(HashMap::new()),
        }
    }

    fn pin(self: &Arc<Self>, reference: GcRef, owner_id: u64) -> HostRoot {
        let mut references = self.references.lock().expect("host root set poisoned");
        *references.entry(reference).or_default() += 1;
        HostRoot {
            set: Arc::clone(self),
            reference,
            owner_id,
        }
    }

    pub(crate) fn references(&self) -> Vec<GcRef> {
        self.references
            .lock()
            .expect("host root set poisoned")
            .keys()
            .copied()
            .collect()
    }
}

impl HostRoot {
    pub(crate) fn new(set: &Arc<HostRootSet>, reference: GcRef, owner_id: u64) -> Self {
        set.pin(reference, owner_id)
    }
}

impl Clone for HostRoot {
    fn clone(&self) -> Self {
        Self::new(&self.set, self.reference, self.owner_id)
    }
}

impl Drop for HostRoot {
    fn drop(&mut self) {
        let mut references = self.set.references.lock().expect("host root set poisoned");
        let Some(count) = references.get_mut(&self.reference) else {
            return;
        };
        *count = count.saturating_sub(1);
        if *count == 0 {
            references.remove(&self.reference);
        }
    }
}
