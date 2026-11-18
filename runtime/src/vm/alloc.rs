use super::{
    AelysFunction, GcObject, GcRef, Heap, HostRoot, NativeFn, NativeFunction, NativeFunctionImpl,
    ObjectKind, VM, Value,
};
use aelys_bytecode::object::{
    AelysArray, AelysEnum, AelysStruct, AelysSum, AelysVec, SumTag, TypeTag,
};
use aelys_bytecode::{DefId, EnumSchema, SchemaId, StructSchema, TypeDescriptor};
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
        let vector_root = self
            .collection_pending(0)
            .then(|| HostRoot::new(&self.host_roots, vector_ref, self.id));

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
        let (length, spare) = match self.heap.get(vector_ref).map(|object| &object.kind) {
            Some(ObjectKind::Vec(vector)) => (vector.len(), vector.len() < vector.capacity()),
            _ => (0, false),
        };
        if !spare && self.reserve_vec(vector_ref, length.max(1)).is_err() {
            self.reserve_vec(vector_ref, 1)?;
        }
        match self.heap.get_mut(vector_ref) {
            Some(object) => match &mut object.kind {
                ObjectKind::Vec(vector) => {
                    if vector.push(value) {
                        let index = vector.len() - 1;
                        self.heap.write_barrier_element(vector_ref, index, value);
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
        if let Some(existing) = self.heap.lookup_interned_string(s) {
            return Ok(existing);
        }
        let size = u64::try_from(Heap::estimate_string_size(s.len())).unwrap_or(u64::MAX);
        self.maybe_collect_for(size);
        self.intern_string_without_collection(s)
    }

    fn intern_string_without_collection(&mut self, s: &str) -> Result<GcRef, RuntimeError> {
        if let Some(existing) = self.heap.lookup_interned_string(s) {
            return Ok(existing);
        }
        let size = u64::try_from(Heap::estimate_string_size(s.len())).unwrap_or(u64::MAX);
        self.ensure_heap_capacity(size)?;
        Ok(self.heap.intern_string(s))
    }

    pub fn alloc_function(&mut self, mut func: super::Function) -> Result<GcRef, RuntimeError> {
        self.materialize_schema_ids(&mut func)?;
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

    fn materialize_schema_ids(
        &mut self,
        function: &mut super::Function,
    ) -> Result<(), RuntimeError> {
        let mut ids = Vec::with_capacity(function.struct_schemas.len());
        for schema in &function.struct_schemas {
            let key = struct_identity(schema, function);
            ids.push(self.intern_schema(key, schema));
        }
        function.schema_ids = ids;
        let mut enum_ids = Vec::with_capacity(function.enum_schemas.len());
        for schema in &function.enum_schemas {
            let key = enum_identity(schema, function);
            let id = self.intern_enum_schema(key, schema).ok_or_else(|| {
                self.runtime_error(RuntimeErrorKind::InvalidBytecode(
                    "more distinct enum schemas than a u16 enum id can name".to_string(),
                ))
            })?;
            enum_ids.push(id);
        }
        function.enum_ids = enum_ids;
        for nested in &mut function.nested_functions {
            self.materialize_schema_ids(nested)?;
        }
        Ok(())
    }

    fn intern_schema(&mut self, key: StructIdentity, schema: &StructSchema) -> SchemaId {
        if let Some(id) = self.schema_index.get(&key) {
            return *id;
        }
        let mut id = SchemaId(self.next_schema_id);
        self.next_schema_id = self.next_schema_id.saturating_add(1).max(1);
        while self.schema_registry.contains_key(&id) {
            id = SchemaId(self.next_schema_id);
            self.next_schema_id = self.next_schema_id.saturating_add(1).max(1);
        }
        self.schema_registry.insert(id, schema.clone());
        self.schema_index.insert(key, id);
        id
    }

    fn intern_enum_schema(&mut self, key: EnumIdentity, schema: &EnumSchema) -> Option<u16> {
        if let Some(id) = self.enum_index.get(&key) {
            return Some(*id);
        }
        let id = u16::try_from(self.enum_registry.len()).ok()?;
        self.enum_registry.push(
            schema
                .variants
                .iter()
                .map(|variant| variant.fields.len())
                .collect(),
        );
        self.enum_index.insert(key, id);
        Some(id)
    }

    pub fn alloc_native(
        &mut self,
        name: &str,
        arity: u16,
        func: NativeFn,
    ) -> Result<GcRef, RuntimeError> {
        let slot = self.register_native_impl(name, NativeFunctionImpl::Rust(func));
        let obj = GcObject::new(ObjectKind::Native(NativeFunction::with_slot(
            name, arity, slot,
        )));
        self.alloc_object(obj)
    }

    pub fn alloc_foreign(
        &mut self,
        name: &str,
        arity: u16,
        func: AelysNativeFn,
    ) -> Result<GcRef, RuntimeError> {
        let slot = self.register_native_impl(
            name,
            NativeFunctionImpl::Foreign {
                function: func,
                result: super::ForeignReturnKind::Raw,
            },
        );
        let obj = GcObject::new(ObjectKind::Native(NativeFunction::with_slot(
            name, arity, slot,
        )));
        self.alloc_object(obj)
    }

    pub fn alloc_foreign_with_result(
        &mut self,
        name: &str,
        arity: u16,
        func: AelysNativeFn,
        result_type: Option<AelysNativeType>,
    ) -> Result<GcRef, RuntimeError> {
        let slot = self.register_native_impl(
            name,
            NativeFunctionImpl::Foreign {
                function: func,
                result: super::ForeignReturnKind::from_native_type(result_type),
            },
        );
        let obj = GcObject::new(ObjectKind::Native(NativeFunction::with_slot(
            name, arity, slot,
        )));
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

    pub(crate) fn unit_ok_value(&mut self) -> Result<Value, RuntimeError> {
        if let Some(reference) = self.unit_ok
            && let Some(ObjectKind::Sum(sum)) = self.heap.get(reference).map(|object| &object.kind)
            && sum.tag == SumTag::ResultOk
            && sum.payload.is_unit()
        {
            return Ok(Value::ptr(reference.index()));
        }
        let reference = self.alloc_sum(SumTag::ResultOk, Value::unit())?;
        self.unit_ok = Some(reference);
        Ok(Value::ptr(reference.index()))
    }

    pub fn alloc_sum(&mut self, tag: SumTag, payload: Value) -> Result<GcRef, RuntimeError> {
        let obj = GcObject::new(ObjectKind::Sum(AelysSum::new(tag, payload)));
        let size = u64::try_from(Heap::estimate_object_size(&obj)).unwrap_or(u64::MAX);
        let payload_root = self.pin_values_if_collecting(size, &[payload]);
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
        let object = GcObject::new(ObjectKind::Enum(AelysEnum::new(enum_id, variant_id, slots)));
        let size = u64::try_from(Heap::estimate_object_size(&object)).unwrap_or(u64::MAX);
        let slots_root = match &object.kind {
            ObjectKind::Enum(value) => self.pin_values_if_collecting(size, &value.slots),
            _ => Vec::new(),
        };
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
        let object = GcObject::new(ObjectKind::Struct(Box::new(AelysStruct::new(
            schema_id, slots,
        ))));
        let size = u64::try_from(Heap::estimate_object_size(&object)).unwrap_or(u64::MAX);
        let slots_root = match &object.kind {
            ObjectKind::Struct(value) => self.pin_values_if_collecting(size, &value.slots),
            _ => Vec::new(),
        };
        self.maybe_collect_for(size);
        self.ensure_heap_capacity(size)?;
        let reference = self.heap.alloc(object);
        drop(slots_root);
        Ok(reference)
    }

    fn pin_values_if_collecting(&self, additional: u64, values: &[Value]) -> Vec<HostRoot> {
        if !self.collection_pending(additional) {
            return Vec::new();
        }
        values
            .iter()
            .filter_map(|value| value.as_ptr())
            .map(|raw| HostRoot::new(&self.host_roots, GcRef::new(raw), self.id))
            .collect()
    }

    pub fn heap(&self) -> &Heap {
        &self.heap
    }

    pub fn heap_mut(&mut self) -> &mut Heap {
        &mut self.heap
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum CanonicalDescriptor {
    Plain(TypeDescriptor),
    Option(Box<CanonicalDescriptor>),
    Result(Box<CanonicalDescriptor>, Box<CanonicalDescriptor>),
    Array(Box<CanonicalDescriptor>),
    FixedArray(Box<CanonicalDescriptor>, u32),
    Vec(Box<CanonicalDescriptor>),
    Struct(NominalIdentity),
    Enum(NominalIdentity),
    Function {
        params: Box<[CanonicalDescriptor]>,
        ret: Box<CanonicalDescriptor>,
    },
    Unresolved,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct NominalIdentity {
    def: DefId,
    args: Box<[CanonicalDescriptor]>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct StructIdentity {
    nominal: NominalIdentity,
    fields: Box<[(String, CanonicalDescriptor)]>,
}

type VariantFields = Box<[(Option<String>, CanonicalDescriptor)]>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct EnumIdentity {
    nominal: NominalIdentity,
    name: String,
    variants: Box<[(String, VariantFields)]>,
}

const CANONICAL_DEPTH: usize = 64;

// a local schema index means nothing outside its function, so nesting keys on definition identity
fn canonical(
    descriptor: &TypeDescriptor,
    function: &super::Function,
    depth: usize,
) -> CanonicalDescriptor {
    if depth > CANONICAL_DEPTH {
        return CanonicalDescriptor::Unresolved;
    }
    let inner = |descriptor: &TypeDescriptor| Box::new(canonical(descriptor, function, depth + 1));
    match descriptor {
        TypeDescriptor::Option(item) => CanonicalDescriptor::Option(inner(item)),
        TypeDescriptor::Result(ok, err) => CanonicalDescriptor::Result(inner(ok), inner(err)),
        TypeDescriptor::Array(item) => CanonicalDescriptor::Array(inner(item)),
        TypeDescriptor::FixedArray(item, length) => {
            CanonicalDescriptor::FixedArray(inner(item), *length)
        }
        TypeDescriptor::Vec(item) => CanonicalDescriptor::Vec(inner(item)),
        TypeDescriptor::Struct(index) => usize::try_from(*index)
            .ok()
            .and_then(|index| function.struct_schemas.get(index))
            .map_or(CanonicalDescriptor::Unresolved, |schema| {
                CanonicalDescriptor::Struct(nominal(
                    &schema.ctor,
                    &schema.type_args,
                    function,
                    depth + 1,
                ))
            }),
        TypeDescriptor::Enum(index) => function.enum_schemas.get(usize::from(*index)).map_or(
            CanonicalDescriptor::Unresolved,
            |schema| {
                CanonicalDescriptor::Enum(nominal(
                    &schema.def_id,
                    &schema.type_args,
                    function,
                    depth + 1,
                ))
            },
        ),
        TypeDescriptor::Function { params, ret } => CanonicalDescriptor::Function {
            params: params
                .iter()
                .map(|param| canonical(param, function, depth + 1))
                .collect(),
            ret: inner(ret),
        },
        plain => CanonicalDescriptor::Plain(plain.clone()),
    }
}

fn nominal(
    def: &DefId,
    args: &[TypeDescriptor],
    function: &super::Function,
    depth: usize,
) -> NominalIdentity {
    NominalIdentity {
        def: def.clone(),
        args: args
            .iter()
            .map(|arg| canonical(arg, function, depth))
            .collect(),
    }
}

fn struct_identity(schema: &StructSchema, function: &super::Function) -> StructIdentity {
    StructIdentity {
        nominal: nominal(&schema.ctor, &schema.type_args, function, 0),
        fields: schema
            .fields
            .iter()
            .map(|field| (field.name.clone(), canonical(&field.ty, function, 0)))
            .collect(),
    }
}

fn enum_identity(schema: &EnumSchema, function: &super::Function) -> EnumIdentity {
    EnumIdentity {
        nominal: nominal(&schema.def_id, &schema.type_args, function, 0),
        name: schema.name.clone(),
        variants: schema
            .variants
            .iter()
            .map(|variant| {
                (
                    variant.name.clone(),
                    variant
                        .fields
                        .iter()
                        .map(|field| (field.name.clone(), canonical(&field.ty, function, 0)))
                        .collect(),
                )
            })
            .collect(),
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
