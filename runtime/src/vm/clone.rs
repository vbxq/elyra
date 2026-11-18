use super::{GcObject, GcRef, HostRoot, ObjectKind, VM, Value};
use aelys_bytecode::object::{AelysArray, AelysVec, ArrayData, VecData};
use aelys_common::error::{RuntimeError, RuntimeErrorKind};
use std::collections::HashMap;

impl VM {
    pub fn deep_clone_value(&mut self, value: Value) -> Result<Value, RuntimeError> {
        let Some(raw) = value.as_ptr() else {
            return Ok(value);
        };
        let mut copies: HashMap<GcRef, GcRef> = HashMap::new();
        let mut pins: Vec<HostRoot> = Vec::new();
        let mut pending: Vec<GcRef> = Vec::new();
        let root = self.clone_shell(GcRef::new(raw), &mut copies, &mut pins, &mut pending)?;
        while let Some(shell) = pending.pop() {
            let children = self.aggregate_children(shell);
            for (slot, child) in children {
                let Some(child_raw) = child.as_ptr() else {
                    continue;
                };
                let copy =
                    self.clone_shell(GcRef::new(child_raw), &mut copies, &mut pins, &mut pending)?;
                let replacement = Value::ptr(copy.index());
                if replacement == child {
                    continue;
                }
                self.heap.write_barrier_value(shell, replacement);
                self.store_aggregate_child(shell, slot, replacement)?;
            }
        }
        drop(pins);
        Ok(Value::ptr(root.index()))
    }

    // a copy is allocated before its children are visited, so cycles and shared children map to one copy
    fn clone_shell(
        &mut self,
        original: GcRef,
        copies: &mut HashMap<GcRef, GcRef>,
        pins: &mut Vec<HostRoot>,
        pending: &mut Vec<GcRef>,
    ) -> Result<GcRef, RuntimeError> {
        if let Some(copy) = copies.get(&original) {
            return Ok(*copy);
        }
        let Some(object) = self.heap.get(original) else {
            return Err(self.runtime_error(RuntimeErrorKind::UseAfterFree));
        };
        let kind = match &object.kind {
            ObjectKind::Struct(structure) => ObjectKind::Struct(structure.clone()),
            ObjectKind::Enum(value) => ObjectKind::Enum(value.clone()),
            ObjectKind::Sum(value) => ObjectKind::Sum(*value),
            ObjectKind::Array(array) => ObjectKind::Array(AelysArray {
                data: array.data.clone(),
            }),
            ObjectKind::Vec(vector) => ObjectKind::Vec(match &vector.data {
                VecData::Ints(values) => AelysVec::from_ints(values.clone()),
                VecData::Floats(values) => AelysVec::from_floats(values.clone()),
                VecData::Bools(values) => {
                    AelysVec::from_bools(values.iter().map(|byte| *byte != 0).collect())
                }
                VecData::Objects(values) => AelysVec::from_objects(values.clone()),
            }),
            _ => return Ok(original),
        };
        let copy = self.alloc_object(GcObject::new(kind))?;
        pins.push(HostRoot::new(&self.host_roots, copy, self.id));
        copies.insert(original, copy);
        pending.push(copy);
        Ok(copy)
    }

    fn aggregate_children(&self, shell: GcRef) -> Vec<(usize, Value)> {
        let Some(object) = self.heap.get(shell) else {
            return Vec::new();
        };
        let values: &[Value] = match &object.kind {
            ObjectKind::Struct(structure) => &structure.slots,
            ObjectKind::Enum(value) => &value.slots,
            ObjectKind::Sum(value) => std::slice::from_ref(&value.payload),
            ObjectKind::Array(array) => match &array.data {
                ArrayData::Objects(values) => values,
                _ => &[],
            },
            ObjectKind::Vec(vector) => match &vector.data {
                VecData::Objects(values) => values,
                _ => &[],
            },
            _ => &[],
        };
        values
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, value): &(usize, Value)| value.as_ptr().is_some())
            .collect()
    }

    fn store_aggregate_child(
        &mut self,
        shell: GcRef,
        slot: usize,
        replacement: Value,
    ) -> Result<(), RuntimeError> {
        let Some(object) = self.heap.get_mut(shell) else {
            return Err(self.runtime_error(RuntimeErrorKind::UseAfterFree));
        };
        let target = match &mut object.kind {
            ObjectKind::Struct(structure) => structure.slots.get_mut(slot),
            ObjectKind::Enum(value) => value.slots.get_mut(slot),
            ObjectKind::Sum(value) => (slot == 0).then_some(&mut value.payload),
            ObjectKind::Array(array) => match &mut array.data {
                ArrayData::Objects(values) => values.get_mut(slot),
                _ => None,
            },
            ObjectKind::Vec(vector) => match &mut vector.data {
                VecData::Objects(values) => values.get_mut(slot),
                _ => None,
            },
            _ => None,
        };
        if let Some(target) = target {
            *target = replacement;
        }
        Ok(())
    }
}
