use super::{
    AelysFunction, GcObject, GcRef, Heap, HostRoot, NativeFn, NativeFunction, NativeFunctionImpl,
    ObjectKind, VM, Value,
};
use aelys_bytecode::object::{AelysArray, AelysSum, AelysVec, SumTag};
use aelys_common::error::{RuntimeError, RuntimeErrorKind};
use aelys_native::{AelysNativeFn, AelysNativeType};

impl VM {
    fn ensure_heap_capacity(&self, additional: u64) -> Result<(), RuntimeError> {
        let heap_bytes = u64::try_from(self.heap.bytes_allocated()).unwrap_or(u64::MAX);
        let new_total = heap_bytes.checked_add(additional).ok_or_else(|| {
            self.runtime_error(RuntimeErrorKind::OutOfMemory {
                requested: additional,
                max: self.config.max_heap_bytes,
            })
        })?;
        if new_total > self.config.max_heap_bytes {
            return Err(self.runtime_error(RuntimeErrorKind::OutOfMemory {
                requested: additional,
                max: self.config.max_heap_bytes,
            }));
        }
        Ok(())
    }

    pub fn alloc_object(&mut self, object: GcObject) -> Result<GcRef, RuntimeError> {
        let size = u64::try_from(Heap::estimate_object_size(&object)).unwrap_or(u64::MAX);
        self.maybe_collect_for(size);
        self.alloc_object_without_collection(object)
    }

    fn alloc_object_without_collection(&mut self, object: GcObject) -> Result<GcRef, RuntimeError> {
        let size = u64::try_from(Heap::estimate_object_size(&object)).unwrap_or(u64::MAX);
        self.ensure_heap_capacity(size)?;
        Ok(self.heap.alloc(object))
    }

    pub fn alloc_string(&mut self, s: &str) -> Result<GcRef, RuntimeError> {
        let size = u64::try_from(Heap::estimate_string_size(s.len())).unwrap_or(u64::MAX);
        self.maybe_collect_for(size);
        self.ensure_heap_capacity(size)?;
        Ok(self.heap.alloc_string(s))
    }

    pub fn intern_string(&mut self, s: &str) -> Result<GcRef, RuntimeError> {
        if let Some(existing) = self.heap.find_interned_string(s) {
            return Ok(existing);
        }
        let size = u64::try_from(Heap::estimate_string_size(s.len())).unwrap_or(u64::MAX);
        self.maybe_collect_for(size);
        self.intern_string_without_collection(s)
    }

    fn intern_string_without_collection(&mut self, s: &str) -> Result<GcRef, RuntimeError> {
        if let Some(existing) = self.heap.find_interned_string(s) {
            return Ok(existing);
        }
        let size = u64::try_from(Heap::estimate_string_size(s.len())).unwrap_or(u64::MAX);
        self.ensure_heap_capacity(size)?;
        Ok(self.heap.intern_string(s))
    }

    pub fn alloc_function(&mut self, func: super::Function) -> Result<GcRef, RuntimeError> {
        let mut required = u64::try_from(Heap::estimate_function_size(&func)).unwrap_or(u64::MAX);
        for constant in &func.constants {
            if let aelys_bytecode::Constant::String(string) = constant {
                required = required.saturating_add(
                    u64::try_from(Heap::estimate_string_size(string.len())).unwrap_or(u64::MAX),
                );
            }
        }
        self.maybe_collect_for(required);
        let mut constants = Vec::with_capacity(func.constants.len());
        for constant in &func.constants {
            let value = if let aelys_bytecode::Constant::String(string) = constant {
                Value::ptr(self.intern_string_without_collection(string)?.index())
            } else {
                constant.materialize(&mut self.heap)
            };
            constants.push(value);
        }
        let obj = GcObject::new(ObjectKind::Function(AelysFunction::with_constants(
            func, constants,
        )));
        self.alloc_object_without_collection(obj)
    }

    pub fn alloc_native(
        &mut self,
        name: &str,
        arity: u16,
        func: NativeFn,
    ) -> Result<GcRef, RuntimeError> {
        self.native_registry
            .insert(name.to_string(), NativeFunctionImpl::Rust(func));
        let obj = GcObject::new(ObjectKind::Native(NativeFunction::new(name, arity)));
        self.alloc_object(obj)
    }

    pub fn alloc_foreign(
        &mut self,
        name: &str,
        arity: u16,
        func: AelysNativeFn,
    ) -> Result<GcRef, RuntimeError> {
        self.native_registry.insert(
            name.to_string(),
            NativeFunctionImpl::Foreign {
                function: func,
                result: super::ForeignReturnKind::Raw,
            },
        );
        let obj = GcObject::new(ObjectKind::Native(NativeFunction::new(name, arity)));
        self.alloc_object(obj)
    }

    pub fn alloc_foreign_with_result(
        &mut self,
        name: &str,
        arity: u16,
        func: AelysNativeFn,
        result_type: Option<AelysNativeType>,
    ) -> Result<GcRef, RuntimeError> {
        self.native_registry.insert(
            name.to_string(),
            NativeFunctionImpl::Foreign {
                function: func,
                result: super::ForeignReturnKind::from_native_type(result_type),
            },
        );
        let obj = GcObject::new(ObjectKind::Native(NativeFunction::new(name, arity)));
        self.alloc_object(obj)
    }

    pub fn alloc_array(&mut self, array: AelysArray) -> Result<GcRef, RuntimeError> {
        let size = u64::try_from(array.size_bytes()).unwrap_or(u64::MAX);
        self.maybe_collect_for(size);
        self.ensure_heap_capacity(size)?;
        let obj = GcObject::new(ObjectKind::Array(array));
        self.alloc_object_without_collection(obj)
    }

    pub fn alloc_vec(&mut self, vec: AelysVec) -> Result<GcRef, RuntimeError> {
        let size = u64::try_from(vec.size_bytes()).unwrap_or(u64::MAX);
        self.maybe_collect_for(size);
        self.ensure_heap_capacity(size)?;
        let obj = GcObject::new(ObjectKind::Vec(vec));
        self.alloc_object_without_collection(obj)
    }

    pub fn alloc_sum(&mut self, tag: SumTag, payload: Value) -> Result<GcRef, RuntimeError> {
        let payload_root = payload
            .as_ptr()
            .map(|raw| HostRoot::new(&self.host_roots, GcRef::new(raw), self.id));
        let obj = GcObject::new(ObjectKind::Sum(AelysSum::new(tag, payload)));
        let size = u64::try_from(Heap::estimate_object_size(&obj)).unwrap_or(u64::MAX);
        self.maybe_collect_for(size);
        self.ensure_heap_capacity(size)?;
        drop(payload_root);
        Ok(self.heap.alloc(obj))
    }

    pub fn heap(&self) -> &Heap {
        &self.heap
    }

    pub fn heap_mut(&mut self) -> &mut Heap {
        &mut self.heap
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aelys_bytecode::{Constant, Function, OpCode, SumTag};
    use aelys_syntax::Source;

    #[test]
    fn alloc_sum_roots_payload_across_its_collection() {
        let mut vm = VM::new(Source::new("alloc-sum-gc", "")).unwrap();
        let payload = vm.alloc_string("payload").unwrap();
        let host_root = vm.pin_host_ref(payload);
        vm.collect();
        drop(host_root);

        let sum = vm
            .alloc_sum(SumTag::ResultErr, Value::ptr(payload.index()))
            .unwrap();
        let sum_root = vm.pin_host_ref(sum);
        vm.collect();

        assert!(vm.heap().get(payload).is_some());
        assert_eq!(vm.heap().get_type_name(sum), "Sum");
        drop(sum_root);
    }

    #[test]
    fn bytecode_make_sum_keeps_a_native_payload_across_collection() {
        fn make_payload(vm: &mut VM, _args: &[Value]) -> Result<Value, RuntimeError> {
            let payload = vm.alloc_string("native payload")?;
            Ok(Value::ptr(payload.index()))
        }

        fn force_safepoint(vm: &mut VM, _args: &[Value]) -> Result<Value, RuntimeError> {
            vm.collect();
            Ok(Value::unit())
        }

        let mut vm = VM::new(Source::new("make-sum-gc", "")).unwrap();
        let native = vm.alloc_native("make_payload", 0, make_payload).unwrap();
        let safepoint = vm
            .alloc_native("force_safepoint", 0, force_safepoint)
            .unwrap();
        vm.set_global("make_payload".to_string(), Value::ptr(native.index()));
        vm.set_global("force_safepoint".to_string(), Value::ptr(safepoint.index()));
        let anchor = vm.alloc_string("anchor").unwrap();
        let anchor_root = vm.pin_host_ref(anchor);

        let mut function = Function::new(Some("make_sum".to_string()), 0);
        let make_name =
            function.add_structural_constant(Constant::String("make_payload".to_string()));
        let safepoint_name =
            function.add_structural_constant(Constant::String("force_safepoint".to_string()));
        function.emit_b(OpCode::GetGlobal, 0, i16::try_from(make_name).unwrap(), 1);
        function.emit_c(OpCode::Call, 1, 0, 0, 1);
        function.emit_b(
            OpCode::GetGlobal,
            0,
            i16::try_from(safepoint_name).unwrap(),
            1,
        );
        function.emit_c(OpCode::Call, 0, 0, 0, 1);
        function.emit_a(OpCode::MakeSum, 2, 1, SumTag::ResultErr as u8, 1);
        function.emit_a(OpCode::Return, 2, 0, 0, 1);
        function.finalize_bytecode();
        let function = vm.alloc_function(function).unwrap();
        let function_root = vm.pin_host_ref(function);

        let before = vm.execution_stats();
        let result = vm.execute(function).unwrap();
        let after = vm.execution_stats();
        assert!(after.collections > before.collections);

        let sum = GcRef::new(result.as_ptr().expect("make_sum must return a pointer"));
        let sum_root = vm.pin_host_ref(sum);
        vm.collect();
        let ObjectKind::Sum(sum_object) = &vm.heap().get(sum).unwrap().kind else {
            panic!("make_sum must return a sum");
        };
        let payload = GcRef::new(sum_object.payload.as_ptr().expect("sum payload"));
        assert!(vm.heap().get(payload).is_some());
        assert_eq!(vm.heap().get_type_name(payload), "String");
        drop(sum_root);
        drop(function_root);
        drop(anchor_root);
    }
}
