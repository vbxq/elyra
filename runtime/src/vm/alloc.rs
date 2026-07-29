use super::{
    AelysFunction, GcObject, GcRef, Heap, NativeFn, NativeFunction, NativeFunctionImpl, ObjectKind,
    VM, Value,
};
use aelys_bytecode::object::{AelysArray, AelysVec};
use aelys_common::error::{RuntimeError, RuntimeErrorKind};
use aelys_native::AelysNativeFn;

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
        self.native_registry
            .insert(name.to_string(), NativeFunctionImpl::Foreign(func));
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

    pub fn heap(&self) -> &Heap {
        &self.heap
    }

    pub fn heap_mut(&mut self) -> &mut Heap {
        &mut self.heap
    }
}
