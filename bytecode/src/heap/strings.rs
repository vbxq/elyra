use super::Heap;
use crate::object::{AelysString, GcObject, GcRef, ObjectKind};

impl Heap {
    // lookup only, doesn't insert
    pub fn find_interned_string(&self, s: &str) -> Option<GcRef> {
        let r = *self.intern_table.get(&Self::fnv1a_hash(s.as_bytes()))?;
        match self.get(r)?.kind {
            ObjectKind::String(ref existing) if existing.as_str() == s => Some(r),
            _ => None,
        }
    }

    pub fn lookup_interned_string(&mut self, s: &str) -> Option<GcRef> {
        let r = self.find_interned_string(s)?;
        self.keep_interned_alive(r);
        Some(r)
    }

    // the intern table is weak: a string handed out again while a major is running must not be swept
    fn keep_interned_alive(&mut self, r: GcRef) {
        match self.phase {
            super::MajorPhase::Idle => {}
            super::MajorPhase::Mark => self.shade(r),
            super::MajorPhase::Sweep => {
                if r.slot_index() >= self.sweep_cursor
                    && let Some(object) = self.get_mut(r)
                {
                    object.marked = true;
                }
            }
        }
    }

    // intern or return existing
    pub fn intern_string(&mut self, s: &str) -> GcRef {
        let hash = Self::fnv1a_hash(s.as_bytes());

        // check if already interned
        if let Some(&r) = self.intern_table.get(&hash)
            && let Some(obj) = self.get(r)
            && let ObjectKind::String(ref existing) = obj.kind
            && existing.as_str() == s
        {
            self.keep_interned_alive(r);
            return r;
        }

        let string = AelysString::new(s);
        string.mark_interned();
        let r = self.alloc(GcObject::new(ObjectKind::String(string)));
        self.intern_table.insert(hash, r);
        self.intern_keys.insert(r, hash);
        r
    }

    pub fn concat_strings(&mut self, left: GcRef, right: GcRef) -> Option<GcRef> {
        let joined = match (&self.get(left)?.kind, &self.get(right)?.kind) {
            (ObjectKind::String(left), ObjectKind::String(right)) => {
                AelysString::concat(left, right)
            }
            _ => return None,
        };
        Some(self.alloc(GcObject::new(ObjectKind::String(joined))))
    }

    pub fn fnv1a_hash(bytes: &[u8]) -> u64 {
        let mut h = 0xcbf29ce484222325u64;
        for &b in bytes {
            h = (h ^ b as u64).wrapping_mul(0x100000001b3);
        }
        h
    }
}
