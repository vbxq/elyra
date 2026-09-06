use super::{
    AelysFunction, GcObject, GcRef, Heap, HostRoot, NativeFn, NativeFunction, NativeFunctionImpl,
    ObjectKind, VM, Value,
};
use aelys_bytecode::object::{
    AelysArray, AelysEnum, AelysStruct, AelysSum, AelysVec, SumTag, TypeTag,
};
use aelys_bytecode::{SchemaId, StructSchema};
use aelys_common::error::{RuntimeError, RuntimeErrorKind};
use aelys_native::{AelysNativeFn, AelysNativeType};

impl VM {
    pub(crate) fn ensure_array_capacity(
        &mut self,
        type_tag: TypeTag,
        len: usize,
    ) -> Result<(), RuntimeError> {
        let size = AelysArray::size_bytes_for(type_tag, len)
            .and_then(|size| u64::try_from(size).ok())
            .ok_or_else(|| {
                self.runtime_error(RuntimeErrorKind::OutOfMemory {
                    requested: u64::MAX,
                    max: self.config.max_heap_bytes,
                })
            })?;
        self.maybe_collect_for(size);
        self.ensure_heap_capacity(size)
    }

    pub(crate) fn reserve_vec(
        &mut self,
        vector_ref: GcRef,
        additional: usize,
    ) -> Result<(), RuntimeError> {
        let (type_tag, len, previous_size) = match self.heap.get(vector_ref) {
            Some(object) => match &object.kind {
                ObjectKind::Vec(vector) => (
                    vector.type_tag(),
                    vector.len(),
                    vector.try_size_bytes().ok_or_else(|| {
                        self.runtime_error(RuntimeErrorKind::OutOfMemory {
                            requested: u64::MAX,
                            max: self.config.max_heap_bytes,
                        })
                    })?,
                ),
                _ => {
                    return Err(self.runtime_error(RuntimeErrorKind::TypeError {
                        operation: "vec reserve",
                        expected: "vec",
                        got: "non-vec object".to_string(),
                    }));
                }
            },
            None => return Err(self.runtime_error(RuntimeErrorKind::InvalidMemoryHandle)),
        };
        let required_len = len.checked_add(additional).ok_or_else(|| {
            self.runtime_error(RuntimeErrorKind::OutOfMemory {
                requested: u64::MAX,
                max: self.config.max_heap_bytes,
            })
        })?;
        let required_size = AelysVec::size_bytes_for(type_tag, required_len)
            .and_then(|size| u64::try_from(size).ok())
            .ok_or_else(|| {
                self.runtime_error(RuntimeErrorKind::OutOfMemory {
                    requested: u64::MAX,
                    max: self.config.max_heap_bytes,
                })
            })?;
        let previous_size_u64 = u64::try_from(previous_size).unwrap_or(u64::MAX);
        let minimum_growth = required_size.saturating_sub(previous_size_u64);
        self.ensure_heap_capacity(minimum_growth)?;
        let vector_root = HostRoot::new(&self.host_roots, vector_ref, self.id);

        let reserve_failed = match self.heap.get_mut(vector_ref) {
            Some(object) => match &mut object.kind {
                ObjectKind::Vec(vector) => !vector.try_reserve_exact(additional),
                _ => {
                    return Err(self.runtime_error(RuntimeErrorKind::TypeError {
                        operation: "vec reserve",
                        expected: "vec",
                        got: "non-vec object".to_string(),
                    }));
                }
            },
            None => return Err(self.runtime_error(RuntimeErrorKind::InvalidMemoryHandle)),
        };
        if reserve_failed {
            return Err(self.runtime_error(RuntimeErrorKind::OutOfMemory {
                requested: minimum_growth,
                max: self.config.max_heap_bytes,
            }));
        }

        self.maybe_collect_for(0);

        let current_size = match self.heap.get(vector_ref) {
            Some(object) => match &object.kind {
                ObjectKind::Vec(vector) => vector.try_size_bytes().ok_or_else(|| {
                    self.runtime_error(RuntimeErrorKind::OutOfMemory {
                        requested: u64::MAX,
                        max: self.config.max_heap_bytes,
                    })
                })?,
                _ => {
                    return Err(self.runtime_error(RuntimeErrorKind::TypeError {
                        operation: "vec reserve",
                        expected: "vec",
                        got: "non-vec object".to_string(),
                    }));
                }
            },
            None => return Err(self.runtime_error(RuntimeErrorKind::InvalidMemoryHandle)),
        };
        self.heap.account_reallocation(previous_size, current_size);
        drop(vector_root);
        Ok(())
    }

    pub(crate) fn push_vec_value(
        &mut self,
        vector_ref: GcRef,
        value: Value,
    ) -> Result<(), RuntimeError> {
        let accepts = match self.heap.get(vector_ref) {
            Some(object) => match &object.kind {
                ObjectKind::Vec(vector) => vector.accepts(value),
                _ => {
                    return Err(self.runtime_error(RuntimeErrorKind::TypeError {
                        operation: "vec push",
                        expected: "vec",
                        got: "non-vec object".to_string(),
                    }));
                }
            },
            None => return Err(self.runtime_error(RuntimeErrorKind::InvalidMemoryHandle)),
        };
        if !accepts {
            return Err(self.runtime_error(RuntimeErrorKind::TypeError {
                operation: "vec push",
                expected: "matching element type",
                got: "incompatible type".to_string(),
            }));
        }
        self.reserve_vec(vector_ref, 1)?;
        match self.heap.get_mut(vector_ref) {
            Some(object) => match &mut object.kind {
                ObjectKind::Vec(vector) => {
                    if vector.push(value) {
                        Ok(())
                    } else {
                        Err(self.runtime_error(RuntimeErrorKind::TypeError {
                            operation: "vec push",
                            expected: "matching element type",
                            got: "incompatible type".to_string(),
                        }))
                    }
                }
                _ => Err(self.runtime_error(RuntimeErrorKind::TypeError {
                    operation: "vec push",
                    expected: "vec",
                    got: "non-vec object".to_string(),
                })),
            },
            None => Err(self.runtime_error(RuntimeErrorKind::InvalidMemoryHandle)),
        }
    }

    pub(crate) fn ensure_heap_capacity(&self, additional: u64) -> Result<(), RuntimeError> {
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

    pub fn alloc_function(&mut self, mut func: super::Function) -> Result<GcRef, RuntimeError> {
        self.materialize_schema_ids(&mut func);
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
        let obj = GcObject::new(ObjectKind::Function(Box::new(
            AelysFunction::with_constants(func, constants),
        )));
        self.alloc_object_without_collection(obj)
    }

    fn materialize_schema_ids(&mut self, function: &mut super::Function) {
        let schemas = function.struct_schemas.clone();
        function.schema_ids = schemas
            .iter()
            .map(|schema| self.intern_schema(schema))
            .collect();
        for nested in &mut function.nested_functions {
            self.materialize_schema_ids(nested);
        }
    }

    fn intern_schema(&mut self, schema: &StructSchema) -> SchemaId {
        if let Some((id, _)) = self
            .schema_registry
            .iter()
            .find(|(_, existing)| same_schema(existing, schema))
        {
            return *id;
        }
        let mut id = SchemaId(self.next_schema_id);
        self.next_schema_id = self.next_schema_id.saturating_add(1).max(1);
        while self.schema_registry.contains_key(&id) {
            id = SchemaId(self.next_schema_id);
            self.next_schema_id = self.next_schema_id.saturating_add(1).max(1);
        }
        self.schema_registry.insert(id, schema.clone());
        id
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

    pub fn alloc_enum(
        &mut self,
        enum_id: u16,
        variant_id: u16,
        slots: Vec<Value>,
    ) -> Result<GcRef, RuntimeError> {
        if slots.len() > usize::from(u16::MAX) {
            return Err(self.runtime_error(RuntimeErrorKind::InvalidBytecode(
                "enum payload exceeds u16 slot limit".to_string(),
            )));
        }
        let slots_root = slots
            .iter()
            .filter_map(|value| value.as_ptr())
            .map(|raw| HostRoot::new(&self.host_roots, GcRef::new(raw), self.id))
            .collect::<Vec<_>>();
        let object = GcObject::new(ObjectKind::Enum(AelysEnum::new(enum_id, variant_id, slots)));
        let size = u64::try_from(Heap::estimate_object_size(&object)).unwrap_or(u64::MAX);
        self.maybe_collect_for(size);
        self.ensure_heap_capacity(size)?;
        let reference = self.heap.alloc(object);
        drop(slots_root);
        Ok(reference)
    }

    pub fn alloc_struct(
        &mut self,
        schema_id: aelys_bytecode::SchemaId,
        slots: Vec<Value>,
    ) -> Result<GcRef, RuntimeError> {
        let slots_root = slots
            .iter()
            .filter_map(|value| value.as_ptr())
            .map(|raw| HostRoot::new(&self.host_roots, GcRef::new(raw), self.id))
            .collect::<Vec<_>>();
        let object = GcObject::new(ObjectKind::Struct(Box::new(AelysStruct::new(
            schema_id, slots,
        ))));
        let size = u64::try_from(Heap::estimate_object_size(&object)).unwrap_or(u64::MAX);
        self.maybe_collect_for(size);
        self.ensure_heap_capacity(size)?;
        let reference = self.heap.alloc(object);
        drop(slots_root);
        Ok(reference)
    }

    pub fn heap(&self) -> &Heap {
        &self.heap
    }

    pub fn heap_mut(&mut self) -> &mut Heap {
        &mut self.heap
    }
}

fn same_schema(left: &StructSchema, right: &StructSchema) -> bool {
    left.ctor == right.ctor && left.type_args == right.type_args && left.fields == right.fields
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

    #[test]
    fn failed_vec_reserve_does_not_collect_before_admission() {
        let config =
            super::super::config::VmConfig::new(super::super::config::VmConfig::MIN_HEAP_BYTES)
                .unwrap();
        let mut vm = VM::with_config(Source::new("reserve-admission", ""), config).unwrap();
        let vector = vm.alloc_vec(AelysVec::new_ints()).unwrap();
        let vector_root = vm.pin_host_ref(vector);
        let garbage = vm.alloc_string(&"x".repeat(300_000)).unwrap();
        assert!(vm.heap().get(garbage).is_some());
        let before = vm.heap().bytes_allocated();

        let error = vm.reserve_vec(vector, 200_000).unwrap_err();

        assert!(matches!(error.kind, RuntimeErrorKind::OutOfMemory { .. }));
        assert_eq!(vm.heap().bytes_allocated(), before);
        drop(vector_root);
    }
}
