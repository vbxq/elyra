use super::Heap;
use crate::Value;
use crate::object::{BoxedInt, GcObject, GcRef, ObjectKind};

impl Heap {
    #[inline]
    pub fn int_value(&self, value: Value) -> Option<i128> {
        match value.as_int() {
            Some(inline) => Some(i128::from(inline)),
            None => self.boxed_int(value),
        }
    }

    pub fn boxed_int(&self, value: Value) -> Option<i128> {
        let raw = value.as_ptr()?;
        match self.get(GcRef::new(raw))?.kind {
            ObjectKind::Int(boxed) => Some(boxed.value()),
            _ => None,
        }
    }

    #[inline]
    pub fn int64_value(&self, value: Value) -> Option<i64> {
        match value.as_int() {
            Some(inline) => Some(inline),
            None => self.boxed_int(value).and_then(|wide| i64::try_from(wide).ok()),
        }
    }

    pub fn is_int(&self, value: Value) -> bool {
        value.is_int() || self.boxed_int(value).is_some()
    }

    #[inline(always)]
    pub fn boxed_ints_seen(&self) -> bool {
        self.boxed_ints
    }

    pub fn alloc_boxed_int(&mut self, boxed: BoxedInt) -> GcRef {
        self.boxed_ints = true;
        self.alloc(GcObject::new(ObjectKind::Int(boxed)))
    }

    // allocates without collecting: only for load time, when no collection can run
    pub fn alloc_int(&mut self, value: i128) -> Option<Value> {
        if let Some(inline) = i64::try_from(value).ok().and_then(Value::inline_int) {
            return Some(inline);
        }
        let boxed = BoxedInt::new(value)?;
        let reference = self.alloc_boxed_int(boxed);
        Some(Value::ptr(reference.index()))
    }
}
