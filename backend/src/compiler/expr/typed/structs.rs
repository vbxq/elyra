use super::super::Compiler;
use aelys_bytecode::OpCode;
use aelys_common::Result;
use aelys_sema::TypedExpr;
use aelys_syntax::Span;

impl Compiler {
    pub(super) fn compile_typed_enum_construct(
        &mut self,
        schema_index: u16,
        variant_index: u16,
        fields: &[(Option<String>, Box<TypedExpr>)],
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let count = u16::try_from(fields.len()).map_err(|_| {
            aelys_common::error::CompileError::new(
                aelys_common::error::CompileErrorKind::TooManyRegisters,
                span,
                self.source.clone(),
            )
        })?;
        let start = if count == 0 {
            dest
        } else {
            self.alloc_consecutive_registers_for_call(usize::from(count), span)?
        };
        for (offset, (_, field)) in fields.iter().enumerate() {
            let register = start
                .checked_add(u16::try_from(offset).map_err(|_| {
                    aelys_common::error::CompileError::new(
                        aelys_common::error::CompileErrorKind::TooManyRegisters,
                        span,
                        self.source.clone(),
                    )
                })?)
                .ok_or_else(|| {
                    aelys_common::error::CompileError::new(
                        aelys_common::error::CompileErrorKind::TooManyRegisters,
                        span,
                        self.source.clone(),
                    )
                })?;
            if register != dest {
                self.register_pool[register as usize] = true;
                self.next_register = self.next_register.max(u32::from(register) + 1);
            }
            self.compile_typed_expr(field, register)?;
        }
        self.emit_enum(
            OpCode::EnumNew,
            schema_index,
            dest,
            start,
            variant_index,
            count,
            span,
        );
        if count != 0 {
            for offset in (0..count).rev() {
                let register = start + offset;
                if register != dest {
                    self.free_register(register);
                }
            }
        }
        Ok(())
    }

    pub(super) fn compile_typed_struct_literal(
        &mut self,
        schema_index: u16,
        fields: &[(String, Box<TypedExpr>)],
        field_offsets: &[u16],
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let count = u16::try_from(field_offsets.len()).map_err(|_| {
            aelys_common::error::CompileError::new(
                aelys_common::error::CompileErrorKind::TooManyRegisters,
                span,
                self.source.clone(),
            )
        })?;
        let start = if count == 0 {
            dest
        } else {
            self.alloc_consecutive_registers_for_call(usize::from(count), span)?
        };
        for (field, offset) in fields.iter().zip(field_offsets.iter().copied()) {
            let register = start.checked_add(offset).ok_or_else(|| {
                aelys_common::error::CompileError::new(
                    aelys_common::error::CompileErrorKind::TooManyRegisters,
                    span,
                    self.source.clone(),
                )
            })?;
            if register != dest {
                self.register_pool[register as usize] = true;
                self.next_register = self.next_register.max(u32::from(register) + 1);
            }
            self.compile_typed_expr(&field.1, register)?;
        }
        self.emit_struct(OpCode::StructNew, schema_index, dest, start, count, span);
        if count != 0 {
            for offset in (0..count).rev() {
                let register = start + offset;
                if register != dest {
                    self.free_register(register);
                }
            }
        }
        Ok(())
    }

    pub(super) fn compile_typed_struct_field(
        &mut self,
        object: &TypedExpr,
        offset: u16,
        schema_index: u16,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let object_reg = self.alloc_register()?;
        self.compile_typed_expr(object, object_reg)?;
        self.emit_struct(
            OpCode::StructLoad,
            schema_index,
            dest,
            object_reg,
            offset,
            span,
        );
        self.free_register(object_reg);
        Ok(())
    }

    pub(super) fn compile_typed_struct_field_assign(
        &mut self,
        object: &TypedExpr,
        value: &TypedExpr,
        offset: u16,
        schema_index: u16,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let object_reg = self.alloc_register()?;
        self.compile_typed_expr(object, object_reg)?;
        self.compile_typed_expr(value, dest)?;
        self.emit_struct(
            OpCode::StructStore,
            schema_index,
            object_reg,
            dest,
            offset,
            span,
        );
        self.free_register(object_reg);
        Ok(())
    }
}
