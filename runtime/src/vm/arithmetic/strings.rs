use super::super::VM;
use super::super::{GcRef, Heap, ObjectKind, Value};
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
        let added = match (self.heap.get(left_ref), self.heap.get(right_ref)) {
            (Some(l), Some(r)) => match (&l.kind, &r.kind) {
                (ObjectKind::String(ls), ObjectKind::String(rs)) => ls.len() + rs.len(),
                _ => return Ok(None),
            },
            _ => return Ok(None),
        };
        let size = u64::try_from(Heap::estimate_string_size(added)).unwrap_or(u64::MAX);
        self.maybe_collect_for(size);
        self.ensure_heap_capacity(size)?;
        Ok(self
            .heap
            .concat_strings(left_ref, right_ref)
            .map(|joined| Value::ptr(joined.index())))
    }
}
