use super::super::VM;
use super::super::{AelysString, GcRef, ObjectKind, Value};
use aelys_common::error::RuntimeError;

impl VM {
    pub(super) fn try_concat_strings(
        &mut self,
        left: Value,
        right: Value,
    ) -> Result<Option<Value>, RuntimeError> {
        let (Some(left_ptr), Some(right_ptr)) = (left.as_ptr(), right.as_ptr()) else {
            return Ok(None);
        };
        let (left_ref, right_ref) = (GcRef::new(left_ptr), GcRef::new(right_ptr));
        let Some(charge) = self.concat_charge(left_ref, right_ref) else {
            return Ok(None);
        };
        self.maybe_collect_for(charge);
        let charge = self.concat_charge(left_ref, right_ref).unwrap_or(charge);
        self.ensure_heap_capacity(charge)?;
        Ok(self
            .heap
            .concat_strings(left_ref, right_ref)
            .map(|joined| Value::ptr(joined.index())))
    }

    fn concat_charge(&self, left: GcRef, right: GcRef) -> Option<u64> {
        match (&self.heap.get(left)?.kind, &self.heap.get(right)?.kind) {
            (ObjectKind::String(left), ObjectKind::String(right)) => {
                let bytes = std::mem::size_of::<AelysString>()
                    .saturating_add(AelysString::concat_charge(left, right));
                Some(u64::try_from(bytes).unwrap_or(u64::MAX))
            }
            _ => None,
        }
    }
}
