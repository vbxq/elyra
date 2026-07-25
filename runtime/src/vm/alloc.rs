use super::{
    AelysFunction, GcObject, GcRef, Heap, NativeFn, NativeFunction, NativeFunctionImpl, ObjectKind,
    VM,
};
use aelys_bytecode::object::{AelysArray, AelysVec};
use aelys_common::error::{RuntimeError, RuntimeErrorKind};
use aelys_native::AelysNativeFn;

impl VM {
    fn ensure_heap_capacity(&self, additional: u64) -> Result<(), RuntimeError> {
        let heap_bytes = self.heap.bytes_allocated() as u64;
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
        let size = Heap::estimate_object_size(&object) as u64;
        self.ensure_heap_capacity(size)?;
        Ok(self.heap.alloc(object))
    }

    pub fn alloc_string(&mut self, s: &str) -> Result<GcRef, RuntimeError> {
        let size = Heap::estimate_string_size(s.len()) as u64;
        self.ensure_heap_capacity(size)?;
        Ok(self.heap.alloc_string(s))
    }

    pub fn intern_string(&mut self, s: &str) -> Result<GcRef, RuntimeError> {
        if let Some(existing) = self.heap.find_interned_string(s) {
            return Ok(existing);
        }
        let size = Heap::estimate_string_size(s.len()) as u64;
        self.ensure_heap_capacity(size)?;
        Ok(self.heap.intern_string(s))
    }

    pub fn alloc_function(&mut self, func: super::Function) -> Result<GcRef, RuntimeError> {
        let obj = GcObject::new(ObjectKind::Function(AelysFunction::new(func)));
        self.alloc_object(obj)
    }

    pub fn alloc_native(
        &mut self,
        name: &str,
        arity: u8,
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
        arity: u8,
        func: AelysNativeFn,
    ) -> Result<GcRef, RuntimeError> {
        self.native_registry
            .insert(name.to_string(), NativeFunctionImpl::Foreign(func));
        let obj = GcObject::new(ObjectKind::Native(NativeFunction::new(name, arity)));
        self.alloc_object(obj)
    }

    pub fn alloc_array(&mut self, array: AelysArray) -> Result<GcRef, RuntimeError> {
        let size = array.size_bytes() as u64;
        self.ensure_heap_capacity(size)?;
        let obj = GcObject::new(ObjectKind::Array(array));
        self.alloc_object(obj)
    }

    pub fn alloc_vec(&mut self, vec: AelysVec) -> Result<GcRef, RuntimeError> {
        let size = vec.size_bytes() as u64;
        self.ensure_heap_capacity(size)?;
        let obj = GcObject::new(ObjectKind::Vec(vec));
        self.alloc_object(obj)
    }

    pub fn heap(&self) -> &Heap {
        &self.heap
    }

    pub fn heap_mut(&mut self) -> &mut Heap {
        &mut self.heap
    }

    pub fn merge_heap(
        &mut self,
        compile_heap: &mut Heap,
    ) -> Result<std::collections::HashMap<usize, usize>, RuntimeError> {
        let added_bytes = compile_heap.bytes_allocated() as u64;
        self.ensure_heap_capacity(added_bytes)?;
        Ok(self.heap.merge(compile_heap))
    }
}
