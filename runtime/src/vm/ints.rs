use super::{GcObject, GcRef, ObjectKind, VM, Value};
use aelys_bytecode::{
    BoxedInt, IntFault, IntOp, SizedInt, checked_int_op, wrapping_sized_neg, wrapping_sized_not,
    wrapping_sized_op,
};
use aelys_common::error::{RuntimeError, RuntimeErrorKind};

impl VM {
    #[inline]
    pub fn int_value(&self, value: Value) -> Option<i128> {
        self.heap.int_value(value)
    }

    #[inline]
    pub fn int64_value(&self, value: Value) -> Option<i64> {
        self.heap.int64_value(value)
    }

    #[inline]
    pub fn is_int_value(&self, value: Value) -> bool {
        value.is_int() || self.heap.boxed_int(value).is_some()
    }

    #[inline]
    pub fn number_as_f64(&self, value: Value) -> Option<f64> {
        match value.as_float() {
            Some(float) => Some(float),
            None => self.int_value(value).map(|int| int as f64),
        }
    }

    // the two's complement pattern of an i64 or u64 value
    #[inline]
    pub fn int_bits(&self, value: Value) -> Option<i64> {
        match value.as_int() {
            Some(inline) => Some(inline),
            None => self.heap.boxed_int(value).map(|wide| wide as i64),
        }
    }

    #[inline]
    pub fn make_int(&mut self, value: i64) -> Result<Value, RuntimeError> {
        match Value::inline_int(value) {
            Some(inline) => Ok(inline),
            None => self.box_int(i128::from(value)),
        }
    }

    pub fn make_wide_int(&mut self, value: i128) -> Result<Value, RuntimeError> {
        match i64::try_from(value).ok().and_then(Value::inline_int) {
            Some(inline) => Ok(inline),
            None => self.box_int(value),
        }
    }

    #[cold]
    #[inline(never)]
    pub fn box_int(&mut self, value: i128) -> Result<Value, RuntimeError> {
        let Some(boxed) = BoxedInt::new(value) else {
            return Err(self.runtime_error(RuntimeErrorKind::IntegerOverflow));
        };
        let object = GcObject::new(ObjectKind::Int(boxed));
        let size =
            u64::try_from(aelys_bytecode::Heap::estimate_object_size(&object)).unwrap_or(u64::MAX);
        self.maybe_collect_for(size);
        self.ensure_heap_capacity(size)?;
        Ok(Value::ptr(self.heap.alloc_boxed_int(boxed).index()))
    }

    pub fn make_sized(&mut self, kind: SizedInt, bits: i64) -> Result<Value, RuntimeError> {
        match kind {
            SizedInt::U64 => self.make_wide_int(i128::from(bits as u64)),
            _ => self.make_int(bits),
        }
    }

    // packed int storage holds only inline values: a boxed int turns the container into objects
    #[inline]
    pub(crate) fn admit_element(&mut self, container: GcRef, value: Value) {
        if value.is_ptr() {
            if self.heap.boxed_ints_seen() {
                self.admit_boxed_int_slow(container, value);
            }
        } else if value.as_int().is_some_and(|int| u8::try_from(int).is_err()) {
            self.widen_byte_storage(container);
        }
    }

    #[cold]
    #[inline(never)]
    fn admit_boxed_int_slow(&mut self, container: GcRef, value: Value) {
        if self.heap.boxed_int(value).is_none() {
            return;
        }
        let resized = match self.heap.get_mut(container).map(|object| &mut object.kind) {
            Some(ObjectKind::Vec(vector)) => {
                let before = vector.size_bytes();
                vector.promote_ints_to_objects();
                Some((before, vector.size_bytes()))
            }
            Some(ObjectKind::Array(array)) => {
                array.promote_ints_to_objects();
                None
            }
            _ => None,
        };
        if let Some((before, after)) = resized {
            self.heap.account_reallocation(before, after);
        }
    }

    // packed bytes hold only 0..=255: a wider int turns the vector into ints
    #[cold]
    #[inline(never)]
    fn widen_byte_storage(&mut self, container: GcRef) {
        let resized = match self.heap.get_mut(container).map(|object| &mut object.kind) {
            Some(ObjectKind::Vec(vector)) if vector.data.as_bytes().is_some() => {
                let before = vector.size_bytes();
                vector.widen_bytes_to_ints();
                Some((before, vector.size_bytes()))
            }
            _ => None,
        };
        if let Some((before, after)) = resized {
            self.heap.account_reallocation(before, after);
        }
    }

    pub(crate) fn int_fault(&self, fault: IntFault) -> RuntimeError {
        match fault {
            IntFault::Overflow => self.runtime_error(RuntimeErrorKind::IntegerOverflow),
            IntFault::DivisionByZero => self.runtime_error(RuntimeErrorKind::DivisionByZero),
        }
    }

    pub(crate) fn int_type_error(&self, op: IntOp, left: Value, right: Value) -> RuntimeError {
        self.runtime_error(RuntimeErrorKind::TypeError {
            operation: int_op_symbol(op),
            expected: "integer",
            got: format!(
                "{} and {}",
                self.value_type_name(left),
                self.value_type_name(right)
            ),
        })
    }

    // the checked `int` semantics on any encoding; bitwise ops also serve u64 through the exact value
    #[cold]
    #[inline(never)]
    pub(crate) fn int_binary(
        &mut self,
        op: IntOp,
        left: Value,
        right: Value,
    ) -> Result<Value, RuntimeError> {
        let (Some(l), Some(r)) = (self.int_value(left), self.int_value(right)) else {
            return Err(self.int_type_error(op, left, right));
        };
        if matches!(op, IntOp::And | IntOp::Or | IntOp::Xor) {
            let exact = match op {
                IntOp::And => l & r,
                IntOp::Or => l | r,
                _ => l ^ r,
            };
            return self.make_wide_int(exact);
        }
        let (Ok(l), Ok(r)) = (i64::try_from(l), i64::try_from(r)) else {
            return Err(self.runtime_error(RuntimeErrorKind::IntegerOverflow));
        };
        match checked_int_op(op, l, r) {
            Ok(result) => self.make_int(result),
            Err(fault) => Err(self.int_fault(fault)),
        }
    }

    #[cold]
    #[inline(never)]
    pub(crate) fn int_compare(
        &self,
        left: Value,
        right: Value,
        operation: &'static str,
    ) -> Result<std::cmp::Ordering, RuntimeError> {
        match (self.int_value(left), self.int_value(right)) {
            (Some(l), Some(r)) => Ok(l.cmp(&r)),
            _ => Err(self.runtime_error(RuntimeErrorKind::TypeError {
                operation,
                expected: "integer",
                got: format!(
                    "{} and {}",
                    self.value_type_name(left),
                    self.value_type_name(right)
                ),
            })),
        }
    }

    pub(crate) fn for_loop_next(
        &mut self,
        iter: Value,
        end: Value,
        step: Value,
        inclusive: bool,
    ) -> Result<Option<Value>, RuntimeError> {
        let (Some(iter), Some(end), Some(step)) = (
            self.int_value(iter),
            self.int_value(end),
            self.int_value(step),
        ) else {
            return Err(self.runtime_error(RuntimeErrorKind::TypeError {
                operation: "for loop",
                expected: "integer bounds",
                got: "non-integer value".to_string(),
            }));
        };
        let next = iter + step;
        let again = match (step > 0, inclusive) {
            (true, true) => next <= end,
            (true, false) => next < end,
            (false, true) => next >= end,
            (false, false) => next > end,
        };
        if again {
            self.make_wide_int(next).map(Some)
        } else {
            Ok(None)
        }
    }

    pub(crate) fn int_negate(&mut self, value: Value) -> Result<Value, RuntimeError> {
        let Some(n) = self.int64_value(value) else {
            return Err(self.runtime_error(RuntimeErrorKind::TypeError {
                operation: "negation",
                expected: "integer",
                got: self.value_type_name(value).to_string(),
            }));
        };
        match n.checked_neg() {
            Some(result) => self.make_int(result),
            None => Err(self.runtime_error(RuntimeErrorKind::IntegerOverflow)),
        }
    }

    pub(crate) fn int_not(&mut self, value: Value) -> Result<Value, RuntimeError> {
        match self.int_value(value) {
            Some(n) if i64::try_from(n).is_ok() => self.make_int(!(n as i64)),
            _ => Err(self.runtime_error(RuntimeErrorKind::TypeError {
                operation: "~",
                expected: "integer",
                got: self.value_type_name(value).to_string(),
            })),
        }
    }

    #[cold]
    #[inline(never)]
    pub(crate) fn sized_binary(
        &mut self,
        op: IntOp,
        kind: SizedInt,
        left: Value,
        right: Value,
    ) -> Result<Value, RuntimeError> {
        let (Some(l), Some(r)) = (self.int_bits(left), self.int_bits(right)) else {
            return Err(self.int_type_error(op, left, right));
        };
        match wrapping_sized_op(op, kind, l, r) {
            Ok(result) => self.make_sized(kind, result),
            Err(fault) => Err(self.int_fault(fault)),
        }
    }

    pub(crate) fn sized_unary(
        &mut self,
        negate: bool,
        kind: SizedInt,
        value: Value,
    ) -> Result<Value, RuntimeError> {
        let Some(bits) = self.int_bits(value) else {
            return Err(self.runtime_error(RuntimeErrorKind::TypeError {
                operation: if negate { "negation" } else { "~" },
                expected: "integer",
                got: self.value_type_name(value).to_string(),
            }));
        };
        let result = if negate {
            wrapping_sized_neg(kind, bits)
        } else {
            wrapping_sized_not(kind, bits)
        };
        self.make_sized(kind, result)
    }
}

pub(crate) fn int_op_symbol(op: IntOp) -> &'static str {
    match op {
        IntOp::Add => "+",
        IntOp::Sub => "-",
        IntOp::Mul => "*",
        IntOp::Div => "/",
        IntOp::Rem => "%",
        IntOp::Shl => "<<",
        IntOp::Shr => ">>",
        IntOp::And => "&",
        IntOp::Or => "|",
        IntOp::Xor => "^",
    }
}
