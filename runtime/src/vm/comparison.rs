
use super::VM;
use super::{GcRef, ObjectKind, Value};
use aelys_common::error::{RuntimeError, RuntimeErrorKind};
use std::collections::HashSet;

const EQUALITY_MEMO_THRESHOLD: usize = 4096;

impl VM {
    pub fn values_equal(&self, left: Value, right: Value) -> bool {
        if left == right {
            return true;
        }
        let (Some(left_ptr), Some(right_ptr)) = (left.as_ptr(), right.as_ptr()) else {
            return false;
        };
        let (Some(left_object), Some(right_object)) = (
            self.heap.get(GcRef::new(left_ptr)),
            self.heap.get(GcRef::new(right_ptr)),
        ) else {
            return false;
        };
        if let (ObjectKind::String(left_text), ObjectKind::String(right_text)) =
            (&left_object.kind, &right_object.kind)
        {
            return left_text == right_text;
        }
        self.values_equal_deep(left, right)
    }

    fn values_equal_deep(&self, left: Value, right: Value) -> bool {
        let mut pending = vec![(left, right)];
        let mut opened = 0usize;
        let mut seen: Option<HashSet<(usize, usize)>> = None;
        while let Some((left, right)) = pending.pop() {
            if left == right {
                continue;
            }
            let (Some(left_ptr), Some(right_ptr)) = (left.as_ptr(), right.as_ptr()) else {
                return false;
            };
            opened += 1;
            if opened > EQUALITY_MEMO_THRESHOLD
                && !seen
                    .get_or_insert_with(HashSet::new)
                    .insert((left_ptr, right_ptr))
            {
                continue;
            }
            let (Some(left_object), Some(right_object)) = (
                self.heap.get(GcRef::new(left_ptr)),
                self.heap.get(GcRef::new(right_ptr)),
            ) else {
                return false;
            };
            match (&left_object.kind, &right_object.kind) {
                (ObjectKind::String(left_text), ObjectKind::String(right_text)) => {
                    if left_text != right_text {
                        return false;
                    }
                }
                (ObjectKind::Enum(left_enum), ObjectKind::Enum(right_enum)) => {
                    if left_enum.enum_id != right_enum.enum_id
                        || left_enum.variant_id != right_enum.variant_id
                        || left_enum.slots.len() != right_enum.slots.len()
                    {
                        return false;
                    }
                    pending.extend(
                        left_enum
                            .slots
                            .iter()
                            .copied()
                            .zip(right_enum.slots.iter().copied()),
                    );
                }
                (ObjectKind::Struct(left_struct), ObjectKind::Struct(right_struct)) => {
                    if left_struct.schema_id != right_struct.schema_id
                        || left_struct.slots.len() != right_struct.slots.len()
                    {
                        return false;
                    }
                    pending.extend(
                        left_struct
                            .slots
                            .iter()
                            .copied()
                            .zip(right_struct.slots.iter().copied()),
                    );
                }
                (ObjectKind::Sum(left_sum), ObjectKind::Sum(right_sum)) => {
                    if left_sum.tag != right_sum.tag {
                        return false;
                    }
                    pending.push((left_sum.payload, right_sum.payload));
                }
                (ObjectKind::Array(left_array), ObjectKind::Array(right_array)) => {
                    if left_array.len() != right_array.len() {
                        return false;
                    }
                    for index in 0..left_array.len() {
                        match (left_array.get(index), right_array.get(index)) {
                            (Some(left_item), Some(right_item)) => {
                                pending.push((left_item, right_item))
                            }
                            _ => return false,
                        }
                    }
                }
                (ObjectKind::Vec(left_vec), ObjectKind::Vec(right_vec)) => {
                    if left_vec.len() != right_vec.len() {
                        return false;
                    }
                    for index in 0..left_vec.len() {
                        match (left_vec.get(index), right_vec.get(index)) {
                            (Some(left_item), Some(right_item)) => {
                                pending.push((left_item, right_item))
                            }
                            _ => return false,
                        }
                    }
                }
                (ObjectKind::Range(left_range), ObjectKind::Range(right_range)) => {
                    if left_range.start != right_range.start
                        || left_range.end != right_range.end
                        || left_range.inclusive != right_range.inclusive
                    {
                        return false;
                    }
                }
                _ => return false,
            }
        }
        true
    }

    pub fn compare_lt(&self, left: Value, right: Value) -> Result<bool, RuntimeError> {
        if let (Some(a), Some(b)) = (left.as_int(), right.as_int()) {
            return Ok(a < b);
        }

        if let (Some(a), Some(b)) = (left.as_float(), right.as_float()) {
            return Ok(a < b);
        }

        if let (Some(a), Some(b)) = (left.as_int(), right.as_float()) {
            return Ok((a as f64) < b);
        }
        if let (Some(a), Some(b)) = (left.as_float(), right.as_int()) {
            return Ok(a < (b as f64));
        }

        Err(self.runtime_error(RuntimeErrorKind::TypeError {
            operation: "comparison",
            expected: "number",
            got: format!(
                "{} and {}",
                self.value_type_name(left),
                self.value_type_name(right)
            ),
        }))
    }

    pub fn compare_le(&self, left: Value, right: Value) -> Result<bool, RuntimeError> {
        if let (Some(a), Some(b)) = (left.as_int(), right.as_int()) {
            return Ok(a <= b);
        }

        if let (Some(a), Some(b)) = (left.as_float(), right.as_float()) {
            return Ok(a <= b);
        }

        if let (Some(a), Some(b)) = (left.as_int(), right.as_float()) {
            return Ok((a as f64) <= b);
        }
        if let (Some(a), Some(b)) = (left.as_float(), right.as_int()) {
            return Ok(a <= (b as f64));
        }

        Err(self.runtime_error(RuntimeErrorKind::TypeError {
            operation: "comparison",
            expected: "number",
            got: format!(
                "{} and {}",
                self.value_type_name(left),
                self.value_type_name(right)
            ),
        }))
    }

    pub fn compare_gt(&self, left: Value, right: Value) -> Result<bool, RuntimeError> {
        if let (Some(a), Some(b)) = (left.as_int(), right.as_int()) {
            return Ok(a > b);
        }

        if let (Some(a), Some(b)) = (left.as_float(), right.as_float()) {
            return Ok(a > b);
        }

        if let (Some(a), Some(b)) = (left.as_int(), right.as_float()) {
            return Ok((a as f64) > b);
        }
        if let (Some(a), Some(b)) = (left.as_float(), right.as_int()) {
            return Ok(a > (b as f64));
        }

        Err(self.runtime_error(RuntimeErrorKind::TypeError {
            operation: "comparison",
            expected: "number",
            got: format!(
                "{} and {}",
                self.value_type_name(left),
                self.value_type_name(right)
            ),
        }))
    }

    pub fn compare_ge(&self, left: Value, right: Value) -> Result<bool, RuntimeError> {
        if let (Some(a), Some(b)) = (left.as_int(), right.as_int()) {
            return Ok(a >= b);
        }

        if let (Some(a), Some(b)) = (left.as_float(), right.as_float()) {
            return Ok(a >= b);
        }

        if let (Some(a), Some(b)) = (left.as_int(), right.as_float()) {
            return Ok((a as f64) >= b);
        }
        if let (Some(a), Some(b)) = (left.as_float(), right.as_int()) {
            return Ok(a >= (b as f64));
        }

        Err(self.runtime_error(RuntimeErrorKind::TypeError {
            operation: "comparison",
            expected: "number",
            got: format!(
                "{} and {}",
                self.value_type_name(left),
                self.value_type_name(right)
            ),
        }))
    }
}
