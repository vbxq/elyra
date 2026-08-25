use super::super::state::{DispatchControl, DispatchState};
use crate::vm::{GcRef, ObjectKind, VM, Value};
use aelys_bytecode::object::SumTag;
use aelys_bytecode::{EnumSchema, IntWidth, OpCode, SchemaId, StructSchema, TypeDescriptor};
use aelys_common::error::{RuntimeError, RuntimeErrorKind};

struct SchemaTables<'a> {
    structs: &'a [StructSchema],
    runtime_ids: &'a [SchemaId],
    enums: &'a [EnumSchema],
}

impl VM {
    #[inline(always)]
    pub(in crate::vm::dispatch) fn execute_enum(
        &mut self,
        state: &DispatchState,
        instruction_pointer: &mut usize,
        func_ref: GcRef,
        bytecode_ptr: *const u32,
        opcode_byte: u8,
        instr: u32,
    ) -> Result<DispatchControl, RuntimeError> {
        let ip = *instruction_pointer;
        let schema_index = u16::try_from(instr & 0xffff).expect("enum schema index fits");
        let first = unsafe { *bytecode_ptr.add(ip) };
        let second = unsafe { *bytecode_ptr.add(ip + 1) };
        *instruction_pointer = ip + 2;
        if instr & 0x00ff_0000 != 0
            || (opcode_byte == u8::from(OpCode::EnumTest) && second & 0xffff != 0)
        {
            return Err(self.runtime_error(RuntimeErrorKind::InvalidBytecode(
                "enum instruction has non-zero reserved bits".to_string(),
            )));
        }
        let a = u16::try_from(first >> 16).expect("enum destination fits");
        let b = u16::try_from(first & 0xffff).expect("enum source fits");
        let c = u16::try_from(second >> 16).expect("enum variant or field fits");
        let count_or_zero = u16::try_from(second & 0xffff).expect("enum count fits");
        let enum_schema = self.enum_schema(func_ref, usize::from(schema_index))?;
        if enum_schema.schema_id != schema_index {
            return Err(self.runtime_error(RuntimeErrorKind::InvalidBytecode(
                "enum schema id does not match its registry slot".to_string(),
            )));
        }
        let schemas = self.schemas(func_ref)?;
        let runtime_ids = self.runtime_schema_ids(func_ref)?;
        let enum_schemas = self.enum_schemas(func_ref)?;
        let tables = SchemaTables {
            structs: &schemas,
            runtime_ids: &runtime_ids,
            enums: &enum_schemas,
        };
        let base = state.base;

        match opcode_byte {
            x if x == u8::from(OpCode::EnumNew) => {
                let variant = enum_schema.variants.get(usize::from(c)).ok_or_else(|| {
                    self.runtime_error(RuntimeErrorKind::InvalidBytecode(
                        "enum variant index is out of bounds".to_string(),
                    ))
                })?;
                if variant.variant_id != c {
                    return Err(self.runtime_error(RuntimeErrorKind::InvalidBytecode(
                        "enum variant id does not match its registry slot".to_string(),
                    )));
                }
                if usize::from(count_or_zero) != variant.fields.len() {
                    return Err(self.runtime_error(RuntimeErrorKind::InvalidBytecode(
                        "enum field count does not match variant schema".to_string(),
                    )));
                }
                let mut slots = Vec::with_capacity(variant.fields.len());
                for (offset, field) in variant.fields.iter().enumerate() {
                    let register = b
                        .checked_add(u16::try_from(offset).map_err(|_| {
                            self.runtime_error(RuntimeErrorKind::InvalidRegister {
                                reg: usize::from(b) + offset,
                                max: state.registers_len,
                            })
                        })?)
                        .ok_or_else(|| {
                            self.runtime_error(RuntimeErrorKind::InvalidRegister {
                                reg: usize::from(b) + offset,
                                max: state.registers_len,
                            })
                        })?;
                    let value = state.read_register(self, base + usize::from(register), ip)?;
                    if !self.value_satisfies_schema(value, &field.ty, &tables, 0) {
                        return Err(self.runtime_error(RuntimeErrorKind::InvalidBytecode(
                            "enum value does not satisfy schema field type".to_string(),
                        )));
                    }
                    slots.push(value);
                }
                let reference = self.alloc_enum(schema_index, c, slots)?;
                state.write_register(
                    self,
                    base + usize::from(a),
                    Value::ptr(reference.index()),
                    ip,
                )?;
            }
            x if x == u8::from(OpCode::EnumTest) => {
                if count_or_zero != 0 {
                    return Err(self.runtime_error(RuntimeErrorKind::InvalidBytecode(
                        "EnumTest has non-zero reserved bits".to_string(),
                    )));
                }
                let source_value = state.read_register(self, base + usize::from(b), ip)?;
                let matches = if let Some(pointer) = source_value.as_ptr()
                    && let Some(object) = self.heap.get(GcRef::new(pointer))
                    && let ObjectKind::Enum(value) = &object.kind
                {
                    let actual_schema =
                        enum_schemas
                            .get(usize::from(value.enum_id))
                            .ok_or_else(|| {
                                self.runtime_error(RuntimeErrorKind::InvalidBytecode(
                                    "enum object has invalid schema index".to_string(),
                                ))
                            })?;
                    let actual_variant = actual_schema
                        .variants
                        .get(usize::from(value.variant_id))
                        .ok_or_else(|| {
                            self.runtime_error(RuntimeErrorKind::InvalidBytecode(
                                "enum object has invalid variant index".to_string(),
                            ))
                        })?;
                    if value.slot_count != u16::try_from(value.slots.len()).unwrap_or(u16::MAX)
                        || value.slots.len() != actual_variant.fields.len()
                        || actual_schema.schema_id != value.enum_id
                        || actual_variant.variant_id != value.variant_id
                    {
                        return Err(self.runtime_error(RuntimeErrorKind::InvalidBytecode(
                            "enum object slot count does not match schema".to_string(),
                        )));
                    }
                    value.enum_id == schema_index && value.variant_id == c
                } else {
                    false
                };
                state.write_register(self, base + usize::from(a), Value::bool(matches), ip)?;
            }
            x if x == u8::from(OpCode::EnumLoad) => {
                let value = state.read_register(self, base + usize::from(b), ip)?;
                let pointer = value.as_ptr().ok_or_else(|| {
                    self.runtime_error(RuntimeErrorKind::TypeError {
                        operation: "enum field read",
                        expected: "enum",
                        got: value.type_name().to_string(),
                    })
                })?;
                let object = self
                    .heap
                    .get(GcRef::new(pointer))
                    .ok_or_else(|| self.runtime_error(RuntimeErrorKind::UseAfterFree))?;
                let ObjectKind::Enum(value) = &object.kind else {
                    return Err(self.runtime_error(RuntimeErrorKind::TypeError {
                        operation: "enum field read",
                        expected: "enum",
                        got: value.type_name().to_string(),
                    }));
                };
                let actual_schema =
                    enum_schemas
                        .get(usize::from(value.enum_id))
                        .ok_or_else(|| {
                            self.runtime_error(RuntimeErrorKind::InvalidBytecode(
                                "enum object has invalid schema index".to_string(),
                            ))
                        })?;
                let actual_variant = actual_schema
                    .variants
                    .get(usize::from(value.variant_id))
                    .ok_or_else(|| {
                        self.runtime_error(RuntimeErrorKind::InvalidBytecode(
                            "enum object has invalid variant index".to_string(),
                        ))
                    })?;
                if value.slot_count != u16::try_from(value.slots.len()).unwrap_or(u16::MAX)
                    || value.slots.len() != actual_variant.fields.len()
                    || actual_schema.schema_id != value.enum_id
                    || actual_variant.variant_id != value.variant_id
                {
                    return Err(self.runtime_error(RuntimeErrorKind::InvalidBytecode(
                        "enum object slot count does not match schema".to_string(),
                    )));
                }
                if value.enum_id != schema_index {
                    return Err(self.runtime_error(RuntimeErrorKind::InvalidEnumField {
                        expected_schema: schema_index,
                        actual_schema: value.enum_id,
                        expected_variant: c,
                        actual_variant: value.variant_id,
                        field_offset: count_or_zero,
                    }));
                }
                if value.variant_id != c {
                    return Err(self.runtime_error(RuntimeErrorKind::InvalidEnumField {
                        expected_schema: schema_index,
                        actual_schema: value.enum_id,
                        expected_variant: c,
                        actual_variant: value.variant_id,
                        field_offset: count_or_zero,
                    }));
                }
                let variant = enum_schema
                    .variants
                    .get(usize::from(value.variant_id))
                    .ok_or_else(|| {
                        self.runtime_error(RuntimeErrorKind::InvalidBytecode(
                            "enum object has invalid variant index".to_string(),
                        ))
                    })?;
                if variant.variant_id != value.variant_id {
                    return Err(self.runtime_error(RuntimeErrorKind::InvalidBytecode(
                        "enum variant id does not match its registry slot".to_string(),
                    )));
                }
                if usize::from(count_or_zero) >= variant.fields.len() {
                    return Err(self.runtime_error(RuntimeErrorKind::InvalidEnumField {
                        expected_schema: schema_index,
                        actual_schema: value.enum_id,
                        expected_variant: c,
                        actual_variant: value.variant_id,
                        field_offset: count_or_zero,
                    }));
                }
                if variant.fields[usize::from(count_or_zero)].offset != count_or_zero {
                    return Err(self.runtime_error(RuntimeErrorKind::InvalidEnumField {
                        expected_schema: schema_index,
                        actual_schema: value.enum_id,
                        expected_variant: c,
                        actual_variant: value.variant_id,
                        field_offset: count_or_zero,
                    }));
                }
                let field = value
                    .slots
                    .get(usize::from(count_or_zero))
                    .copied()
                    .ok_or_else(|| {
                        self.runtime_error(RuntimeErrorKind::InvalidBytecode(
                            "enum object has fewer slots than its schema".to_string(),
                        ))
                    })?;
                state.write_register(self, base + usize::from(a), field, ip)?;
            }
            _ => {
                return Err(self.runtime_error(RuntimeErrorKind::InvalidOpcode {
                    opcode: opcode_byte,
                }));
            }
        }

        Ok(DispatchControl::Continue)
    }

    #[inline(always)]
    pub(in crate::vm::dispatch) fn execute_struct(
        &mut self,
        state: &DispatchState,
        instruction_pointer: &mut usize,
        func_ref: GcRef,
        bytecode_ptr: *const u32,
        opcode_byte: u8,
        instr: u32,
    ) -> Result<DispatchControl, RuntimeError> {
        let ip = *instruction_pointer;
        let schema_index = usize::from(u16::try_from(instr & 0xffff).expect("schema index fits"));
        let first = unsafe { *bytecode_ptr.add(ip) };
        let second = unsafe { *bytecode_ptr.add(ip + 1) };
        *instruction_pointer = ip + 2;
        if instr & 0x00ff_0000 != 0 || second & 0xffff != 0 {
            return Err(self.runtime_error(RuntimeErrorKind::InvalidBytecode(
                "struct instruction has non-zero reserved bits".to_string(),
            )));
        }
        let a = u16::try_from(first >> 16).expect("struct destination fits");
        let b = u16::try_from(first & 0xffff).expect("struct source fits");
        let c = u16::try_from(second >> 16).expect("struct field count fits");
        let schema = self.schema(func_ref, schema_index)?;
        if schema.schema_id != u32::try_from(schema_index).unwrap_or(u32::MAX) {
            return Err(self.runtime_error(RuntimeErrorKind::InvalidBytecode(
                "struct schema id does not match its registry slot".to_string(),
            )));
        }
        let schemas = self.schemas(func_ref)?;
        let enum_schemas = self.enum_schemas(func_ref)?;
        let runtime_ids = self.runtime_schema_ids(func_ref)?;
        let schema_id = *runtime_ids.get(schema_index).ok_or_else(|| {
            self.runtime_error(RuntimeErrorKind::InvalidBytecode(
                "struct schema index has no runtime id".to_string(),
            ))
        })?;
        let tables = SchemaTables {
            structs: &schemas,
            runtime_ids: &runtime_ids,
            enums: &enum_schemas,
        };
        let base = state.base;

        match opcode_byte {
            196 => {
                if usize::from(c) != schema.fields.len() {
                    return Err(self.runtime_error(RuntimeErrorKind::InvalidBytecode(
                        "struct field count does not match schema".to_string(),
                    )));
                }
                let mut slots = Vec::with_capacity(usize::from(c));
                for offset in 0..c {
                    let register = b.checked_add(offset).ok_or_else(|| {
                        self.runtime_error(RuntimeErrorKind::InvalidRegister {
                            reg: usize::from(b) + usize::from(offset),
                            max: state.registers_len,
                        })
                    })?;
                    let value = state.read_register(self, base + usize::from(register), ip)?;
                    let field = &schema.fields[usize::from(offset)];
                    if !self.value_satisfies_schema(value, &field.ty, &tables, 0) {
                        return Err(self.runtime_error(RuntimeErrorKind::InvalidBytecode(
                            "struct value does not satisfy schema field type".to_string(),
                        )));
                    }
                    slots.push(value);
                }
                let reference = self.alloc_struct(schema_id, slots)?;
                state.write_register(
                    self,
                    base + usize::from(a),
                    Value::ptr(reference.index()),
                    ip,
                )?;
            }
            197 => {
                let object = state.read_register(self, base + usize::from(b), ip)?;
                let value = self.read_struct_slot(object, schema_id, c)?;
                state.write_register(self, base + usize::from(a), value, ip)?;
            }
            198 => {
                let object = state.read_register(self, base + usize::from(a), ip)?;
                let value = state.read_register(self, base + usize::from(b), ip)?;
                let field = schema.fields.get(usize::from(c)).ok_or_else(|| {
                    self.runtime_error(RuntimeErrorKind::InvalidBytecode(
                        "struct field offset is out of bounds".to_string(),
                    ))
                })?;
                if !self.value_satisfies_schema(value, &field.ty, &tables, 0) {
                    return Err(self.runtime_error(RuntimeErrorKind::InvalidBytecode(
                        "struct value does not satisfy schema field type".to_string(),
                    )));
                }
                self.write_struct_slot(object, schema_id, c, value)?;
            }
            _ => {
                return Err(self.runtime_error(RuntimeErrorKind::InvalidOpcode {
                    opcode: opcode_byte,
                }));
            }
        }

        Ok(DispatchControl::Continue)
    }

    fn schemas(&self, func_ref: GcRef) -> Result<Vec<StructSchema>, RuntimeError> {
        let function = match self.heap.get(func_ref).map(|object| &object.kind) {
            Some(ObjectKind::Function(function)) => &function.function,
            Some(ObjectKind::Closure(closure)) => match self.heap.get(closure.function) {
                Some(object) => match &object.kind {
                    ObjectKind::Function(function) => &function.function,
                    _ => {
                        return Err(self.runtime_error(RuntimeErrorKind::InvalidBytecode(
                            "struct instruction has no function schema table".to_string(),
                        )));
                    }
                },
                None => {
                    return Err(self.runtime_error(RuntimeErrorKind::InvalidBytecode(
                        "struct instruction has no function schema table".to_string(),
                    )));
                }
            },
            _ => {
                return Err(self.runtime_error(RuntimeErrorKind::InvalidBytecode(
                    "struct instruction has no function schema table".to_string(),
                )));
            }
        };
        Ok(function.struct_schemas.clone())
    }

    fn runtime_schema_ids(&self, func_ref: GcRef) -> Result<Vec<SchemaId>, RuntimeError> {
        let function = match self.heap.get(func_ref).map(|object| &object.kind) {
            Some(ObjectKind::Function(function)) => &function.function,
            Some(ObjectKind::Closure(closure)) => match self.heap.get(closure.function) {
                Some(object) => match &object.kind {
                    ObjectKind::Function(function) => &function.function,
                    _ => {
                        return Err(self.runtime_error(RuntimeErrorKind::InvalidBytecode(
                            "struct instruction has no function schema table".to_string(),
                        )));
                    }
                },
                None => {
                    return Err(self.runtime_error(RuntimeErrorKind::InvalidBytecode(
                        "struct instruction has no function schema table".to_string(),
                    )));
                }
            },
            _ => {
                return Err(self.runtime_error(RuntimeErrorKind::InvalidBytecode(
                    "struct instruction has no function schema table".to_string(),
                )));
            }
        };
        if function.schema_ids.len() != function.struct_schemas.len() {
            return Err(self.runtime_error(RuntimeErrorKind::InvalidBytecode(
                "struct schema table has no materialized runtime ids".to_string(),
            )));
        }
        Ok(function.schema_ids.clone())
    }

    fn enum_schemas(&self, func_ref: GcRef) -> Result<Vec<EnumSchema>, RuntimeError> {
        let function = match self.heap.get(func_ref).map(|object| &object.kind) {
            Some(ObjectKind::Function(function)) => &function.function,
            Some(ObjectKind::Closure(closure)) => match self.heap.get(closure.function) {
                Some(object) => match &object.kind {
                    ObjectKind::Function(function) => &function.function,
                    _ => {
                        return Err(self.runtime_error(RuntimeErrorKind::InvalidBytecode(
                            "enum instruction has no function schema table".to_string(),
                        )));
                    }
                },
                None => {
                    return Err(self.runtime_error(RuntimeErrorKind::InvalidBytecode(
                        "enum instruction has no function schema table".to_string(),
                    )));
                }
            },
            _ => {
                return Err(self.runtime_error(RuntimeErrorKind::InvalidBytecode(
                    "enum instruction has no function schema table".to_string(),
                )));
            }
        };
        Ok(function.enum_schemas.clone())
    }

    fn enum_schema(&self, func_ref: GcRef, index: usize) -> Result<EnumSchema, RuntimeError> {
        self.enum_schemas(func_ref)?
            .into_iter()
            .nth(index)
            .ok_or_else(|| {
                self.runtime_error(RuntimeErrorKind::InvalidBytecode(format!(
                    "enum schema index {index} is out of bounds"
                )))
            })
    }

    fn schema(&self, func_ref: GcRef, index: usize) -> Result<StructSchema, RuntimeError> {
        self.schemas(func_ref)?
            .into_iter()
            .nth(index)
            .ok_or_else(|| {
                self.runtime_error(RuntimeErrorKind::InvalidBytecode(format!(
                    "struct schema index {index} is out of bounds"
                )))
            })
    }

    fn value_satisfies_schema(
        &self,
        value: Value,
        descriptor: &TypeDescriptor,
        tables: &SchemaTables<'_>,
        depth: usize,
    ) -> bool {
        if depth > 64 {
            return false;
        }
        match descriptor {
            TypeDescriptor::Unit => value.is_unit(),
            TypeDescriptor::Bool => value.as_bool().is_some(),
            TypeDescriptor::Int(width) => value
                .as_int()
                .is_some_and(|value| integer_fits(*width, value)),
            TypeDescriptor::Float(_) => value.as_float().is_some(),
            TypeDescriptor::String => {
                self.is_object_kind(value, |kind| matches!(kind, ObjectKind::String(_)))
            }
            TypeDescriptor::Option(inner) => {
                if value.is_none() {
                    true
                } else {
                    self.sum_payload_if(value, SumTag::OptionSome)
                        .is_some_and(|payload| {
                            self.value_satisfies_schema(payload, inner, tables, depth + 1)
                        })
                }
            }
            TypeDescriptor::Result(ok, err) => {
                if let Some(payload) = self.sum_payload_if(value, SumTag::ResultOk) {
                    self.value_satisfies_schema(payload, ok, tables, depth + 1)
                } else if let Some(payload) = self.sum_payload_if(value, SumTag::ResultErr) {
                    self.value_satisfies_schema(payload, err, tables, depth + 1)
                } else {
                    false
                }
            }
            TypeDescriptor::Array(inner) => {
                self.collection_satisfies(value, inner, tables, depth, |kind| {
                    matches!(kind, ObjectKind::Array(_))
                })
            }
            TypeDescriptor::FixedArray(inner, length) => {
                let Some(reference) = value.as_ptr().map(GcRef::new) else {
                    return false;
                };
                let Some(object) = self.heap.get(reference) else {
                    return false;
                };
                let ObjectKind::Array(array) = &object.kind else {
                    return false;
                };
                array.len() == usize::try_from(*length).unwrap_or(usize::MAX)
                    && (0..array.len()).all(|index| {
                        array.get(index).is_some_and(|item| {
                            self.value_satisfies_schema(item, inner, tables, depth + 1)
                        })
                    })
            }
            TypeDescriptor::Vec(inner) => {
                self.collection_satisfies(value, inner, tables, depth, |kind| {
                    matches!(kind, ObjectKind::Vec(_))
                })
            }
            TypeDescriptor::Struct(schema_id) => {
                let Ok(index) = usize::try_from(*schema_id) else {
                    return false;
                };
                let Some(expected) = tables.structs.get(index) else {
                    return false;
                };
                if expected.schema_id != *schema_id {
                    return false;
                }
                let Some(expected_id) = tables.runtime_ids.get(index) else {
                    return false;
                };
                let Some(reference) = value.as_ptr().map(GcRef::new) else {
                    return false;
                };
                self.heap.get(reference).is_some_and(|object| {
                    matches!(&object.kind, ObjectKind::Struct(structure) if structure.schema_id == *expected_id)
                })
            }
            TypeDescriptor::Enum(expected) => {
                let Some(reference) = value.as_ptr().map(GcRef::new) else {
                    return false;
                };
                let Some(object) = self.heap.get(reference) else {
                    return false;
                };
                let ObjectKind::Enum(value) = &object.kind else {
                    return false;
                };
                if value.enum_id != *expected {
                    return false;
                }
                let Some(schema) = tables.enums.get(usize::from(*expected)) else {
                    return false;
                };
                let Some(variant) = schema.variants.get(usize::from(value.variant_id)) else {
                    return false;
                };
                if schema.schema_id != *expected
                    || variant.variant_id != value.variant_id
                    || value.slot_count != u16::try_from(value.slots.len()).unwrap_or(u16::MAX)
                    || value.slot_count != u16::try_from(variant.fields.len()).unwrap_or(u16::MAX)
                {
                    return false;
                }
                variant
                    .fields
                    .iter()
                    .zip(value.slots.iter())
                    .all(|(field, value)| {
                        self.value_satisfies_schema(*value, &field.ty, tables, depth + 1)
                    })
            }
            TypeDescriptor::Any => !value.is_null() && value.as_nested_fn_marker().is_none(),
            TypeDescriptor::Error => self.sum_payload_if(value, SumTag::ErrorMessage).is_some(),
            TypeDescriptor::Never => false,
        }
    }

    fn collection_satisfies(
        &self,
        value: Value,
        inner: &TypeDescriptor,
        tables: &SchemaTables<'_>,
        depth: usize,
        kind_matches: impl Fn(&ObjectKind) -> bool,
    ) -> bool {
        let Some(reference) = value.as_ptr().map(GcRef::new) else {
            return false;
        };
        let Some(object) = self.heap.get(reference) else {
            return false;
        };
        if !kind_matches(&object.kind) {
            return false;
        }
        match &object.kind {
            ObjectKind::Array(array) => (0..array.len()).all(|index| {
                array
                    .get(index)
                    .is_some_and(|item| self.value_satisfies_schema(item, inner, tables, depth + 1))
            }),
            ObjectKind::Vec(vector) => (0..vector.len()).all(|index| {
                vector
                    .get(index)
                    .is_some_and(|item| self.value_satisfies_schema(item, inner, tables, depth + 1))
            }),
            _ => false,
        }
    }

    fn is_object_kind(&self, value: Value, predicate: impl Fn(&ObjectKind) -> bool) -> bool {
        value
            .as_ptr()
            .map(GcRef::new)
            .and_then(|reference| self.heap.get(reference))
            .is_some_and(|object| predicate(&object.kind))
    }

    fn sum_payload_if(&self, value: Value, expected: SumTag) -> Option<Value> {
        let reference = value.as_ptr().map(GcRef::new)?;
        let object = self.heap.get(reference)?;
        match &object.kind {
            ObjectKind::Sum(sum) if sum.tag == expected => Some(sum.payload),
            _ => None,
        }
    }

    fn read_struct_slot(
        &self,
        value: Value,
        schema_id: SchemaId,
        offset: u16,
    ) -> Result<Value, RuntimeError> {
        let reference = value.as_ptr().map(GcRef::new).ok_or_else(|| {
            self.runtime_error(RuntimeErrorKind::TypeError {
                operation: "struct field read",
                expected: "struct",
                got: value.type_name().to_string(),
            })
        })?;
        let object = self
            .heap
            .get(reference)
            .ok_or_else(|| self.runtime_error(RuntimeErrorKind::UseAfterFree))?;
        let ObjectKind::Struct(structure) = &object.kind else {
            return Err(self.runtime_error(RuntimeErrorKind::TypeError {
                operation: "struct field read",
                expected: "struct",
                got: value.type_name().to_string(),
            }));
        };
        if structure.schema_id != schema_id {
            return Err(self.runtime_error(RuntimeErrorKind::TypeError {
                operation: "struct field read",
                expected: "matching struct type",
                got: "different struct type".to_string(),
            }));
        }
        structure
            .slots
            .get(usize::from(offset))
            .copied()
            .ok_or_else(|| {
                self.runtime_error(RuntimeErrorKind::InvalidBytecode(
                    "struct field offset is out of bounds".to_string(),
                ))
            })
    }

    fn write_struct_slot(
        &mut self,
        value: Value,
        schema_id: SchemaId,
        offset: u16,
        replacement: Value,
    ) -> Result<(), RuntimeError> {
        let reference = value.as_ptr().map(GcRef::new).ok_or_else(|| {
            self.runtime_error(RuntimeErrorKind::TypeError {
                operation: "struct field write",
                expected: "struct",
                got: value.type_name().to_string(),
            })
        })?;
        let Some(object) = self.heap.get(reference) else {
            return Err(self.runtime_error(RuntimeErrorKind::UseAfterFree));
        };
        let ObjectKind::Struct(structure) = &object.kind else {
            return Err(self.runtime_error(RuntimeErrorKind::TypeError {
                operation: "struct field write",
                expected: "struct",
                got: value.type_name().to_string(),
            }));
        };
        if structure.schema_id != schema_id {
            return Err(self.runtime_error(RuntimeErrorKind::TypeError {
                operation: "struct field write",
                expected: "matching struct type",
                got: "different struct type".to_string(),
            }));
        }
        if usize::from(offset) >= structure.slots.len() {
            return Err(self.runtime_error(RuntimeErrorKind::InvalidBytecode(
                "struct field offset is out of bounds".to_string(),
            )));
        }
        self.heap.write_barrier_value(reference, replacement);
        let Some(object) = self.heap.get_mut(reference) else {
            return Err(self.runtime_error(RuntimeErrorKind::UseAfterFree));
        };
        let ObjectKind::Struct(structure) = &mut object.kind else {
            return Err(self.runtime_error(RuntimeErrorKind::TypeError {
                operation: "struct field write",
                expected: "struct",
                got: value.type_name().to_string(),
            }));
        };
        let slot = structure
            .slots
            .get_mut(usize::from(offset))
            .expect("struct slot was checked before the barrier");
        *slot = replacement;
        Ok(())
    }
}

fn integer_fits(width: IntWidth, value: i64) -> bool {
    match width {
        IntWidth::I8 => i8::try_from(value).is_ok(),
        IntWidth::I16 => i16::try_from(value).is_ok(),
        IntWidth::I32 => i32::try_from(value).is_ok(),
        IntWidth::I64 => true,
        IntWidth::U8 => u8::try_from(value).is_ok(),
        IntWidth::U16 => u16::try_from(value).is_ok(),
        IntWidth::U32 => u32::try_from(value).is_ok(),
        IntWidth::U64 => value >= 0,
    }
}
