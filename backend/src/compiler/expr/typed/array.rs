use super::super::Compiler;
use aelys_bytecode::OpCode;
use aelys_common::Result;
use aelys_common::error::{CompileError, CompileErrorKind};
use aelys_sema::{InferType, ResolvedType, TypedExpr};
use aelys_syntax::Span;

impl Compiler {
    pub(super) fn compile_typed_array_repeat(
        &mut self,
        expr_ty: &InferType,
        value: &TypedExpr,
        count: &TypedExpr,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let inner = match expr_ty {
            InferType::Array(inner) | InferType::FixedArray(inner, _) => inner,
            _ => {
                return Err(CompileError::new(
                    CompileErrorKind::TypeInferenceError(
                        "array repeat has a non-array result type".to_string(),
                    ),
                    span,
                    self.source.clone(),
                )
                .into());
            }
        };
        let count_reg = self.alloc_register()?;
        self.compile_typed_expr(count, count_reg)?;
        let allocation_opcode = Self::select_typed_opcode(
            inner,
            OpCode::ArrayNewI,
            OpCode::ArrayNewF,
            OpCode::ArrayNewB,
            OpCode::ArrayNewP,
        );
        self.emit_a(allocation_opcode, dest, count_reg, 0, span);

        let value_reg = self.alloc_register()?;
        self.compile_typed_expr(value, value_reg)?;
        let loop_reg = self.alloc_consecutive_registers_for_call(3, span)?;
        self.alloc_consecutive_from(loop_reg, 3)?;
        let end_reg = loop_reg + 1;
        let step_reg = loop_reg + 2;
        self.emit_a(OpCode::Move, end_reg, count_reg, 0, span);
        self.emit_b(OpCode::LoadI, loop_reg, 0, span);
        self.emit_b(OpCode::LoadI, step_reg, 1, span);
        self.emit_a(OpCode::Sub, loop_reg, loop_reg, step_reg, span);
        let jump_to_loop = self.emit_jump(OpCode::Jump, span);
        let body_start = self.current_offset();
        let store_opcode = Self::select_typed_opcode(
            inner,
            OpCode::ArrayStoreI,
            OpCode::ArrayStoreF,
            OpCode::ArrayStoreB,
            OpCode::ArrayStoreP,
        );
        self.emit_a(store_opcode, dest, loop_reg, value_reg, span);
        self.patch_jump(jump_to_loop);
        self.emit_loop_back(
            OpCode::ForLoopI,
            OpCode::ForLoopILong,
            loop_reg,
            body_start,
            span,
        );
        self.free_register(step_reg);
        self.free_register(end_reg);
        self.free_register(loop_reg);
        self.free_register(value_reg);
        self.free_register(count_reg);
        Ok(())
    }

    pub(super) fn compile_typed_vec_repeat(
        &mut self,
        expr_ty: &InferType,
        value: &TypedExpr,
        count: &TypedExpr,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let InferType::Vec(inner) = expr_ty else {
            return Err(CompileError::new(
                CompileErrorKind::TypeInferenceError(
                    "vec repeat has a non-vector result type".to_string(),
                ),
                span,
                self.source.clone(),
            )
            .into());
        };
        let count_reg = self.alloc_register()?;
        self.compile_typed_expr(count, count_reg)?;
        let allocation_opcode = Self::select_typed_opcode(
            inner,
            OpCode::VecNewI,
            OpCode::VecNewF,
            OpCode::VecNewB,
            OpCode::VecNewP,
        );
        self.emit_a(allocation_opcode, dest, 0, 0, span);
        self.emit_a(OpCode::VecReserve, dest, count_reg, 0, span);

        let value_reg = self.alloc_register()?;
        self.compile_typed_expr(value, value_reg)?;
        let loop_reg = self.alloc_consecutive_registers_for_call(3, span)?;
        self.alloc_consecutive_from(loop_reg, 3)?;
        let end_reg = loop_reg + 1;
        let step_reg = loop_reg + 2;
        self.emit_a(OpCode::Move, end_reg, count_reg, 0, span);
        self.emit_b(OpCode::LoadI, loop_reg, 0, span);
        self.emit_b(OpCode::LoadI, step_reg, 1, span);
        self.emit_a(OpCode::Sub, loop_reg, loop_reg, step_reg, span);
        let jump_to_loop = self.emit_jump(OpCode::Jump, span);
        let body_start = self.current_offset();
        let push_opcode = Self::select_typed_opcode(
            inner,
            OpCode::VecPushI,
            OpCode::VecPushF,
            OpCode::VecPushB,
            OpCode::VecPushP,
        );
        self.emit_a(push_opcode, dest, value_reg, 0, span);
        self.patch_jump(jump_to_loop);
        self.emit_loop_back(
            OpCode::ForLoopI,
            OpCode::ForLoopILong,
            loop_reg,
            body_start,
            span,
        );
        self.free_register(step_reg);
        self.free_register(end_reg);
        self.free_register(loop_reg);
        self.free_register(value_reg);
        self.free_register(count_reg);
        Ok(())
    }

    pub(super) fn compile_typed_array_sized(
        &mut self,
        element_type: &Option<ResolvedType>,
        size: &TypedExpr,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let size_reg = self.alloc_register()?;
        self.compile_typed_expr(size, size_reg)?;

        let opcode = match element_type {
            Some(t) if t.is_integer() => OpCode::ArrayNewI,
            Some(t) if t.is_float() => OpCode::ArrayNewF,
            Some(ResolvedType::Bool) => OpCode::ArrayNewB,
            _ => OpCode::ArrayNewP,
        };

        self.emit_a(opcode, dest, size_reg, 0, span);
        self.free_register(size_reg);
        Ok(())
    }

    pub(super) fn compile_typed_array_literal(
        &mut self,
        expr_ty: &InferType,
        elements: &[TypedExpr],
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let count = elements.len();

        if count == 0 {
            let opcode = if let InferType::Array(inner) | InferType::FixedArray(inner, _) = expr_ty
            {
                Self::select_typed_opcode(
                    inner,
                    OpCode::ArrayNewI,
                    OpCode::ArrayNewF,
                    OpCode::ArrayNewB,
                    OpCode::ArrayNewP,
                )
            } else {
                OpCode::ArrayNewP
            };
            self.emit_a(opcode, dest, 0, 0, span);
            return Ok(());
        }

        let count_operand = self.checked_call_arity(count, span)?;
        let start_reg = self.alloc_consecutive_registers_for_call(count, span)?;

        for i in 0..count {
            let reg = start_reg + u16::try_from(i).expect("register offset was range checked");
            self.register_pool[reg as usize] = true;
            if u32::from(reg) >= self.next_register {
                self.next_register = u32::from(reg) + 1;
            }
        }

        for (i, elem) in elements.iter().enumerate() {
            let elem_reg = start_reg + u16::try_from(i).expect("register offset was range checked");
            self.compile_typed_expr(elem, elem_reg)?;
        }

        self.emit_counted_registers(
            OpCode::ArrayLit,
            OpCode::ArrayLitWide,
            dest,
            start_reg,
            count_operand,
            span,
        );

        for i in (0..count).rev() {
            let reg = start_reg + u16::try_from(i).expect("register offset was range checked");
            self.free_register(reg);
        }

        Ok(())
    }

    pub(super) fn compile_typed_vec_literal(
        &mut self,
        expr_ty: &InferType,
        elements: &[TypedExpr],
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let count = elements.len();

        if count == 0 {
            let opcode = if let InferType::Vec(inner) = expr_ty {
                Self::select_typed_opcode(
                    inner,
                    OpCode::VecNewI,
                    OpCode::VecNewF,
                    OpCode::VecNewB,
                    OpCode::VecNewP,
                )
            } else {
                OpCode::VecNewP
            };
            self.emit_a(opcode, dest, 0, 0, span);
            return Ok(());
        }

        let count_operand = self.checked_call_arity(count, span)?;
        let start_reg = self.alloc_consecutive_registers_for_call(count, span)?;

        for i in 0..count {
            let reg = start_reg + u16::try_from(i).expect("register offset was range checked");
            self.register_pool[reg as usize] = true;
            if u32::from(reg) >= self.next_register {
                self.next_register = u32::from(reg) + 1;
            }
        }

        for (i, elem) in elements.iter().enumerate() {
            let elem_reg = start_reg + u16::try_from(i).expect("register offset was range checked");
            self.compile_typed_expr(elem, elem_reg)?;
        }

        self.emit_counted_registers(
            OpCode::VecLit,
            OpCode::VecLitWide,
            dest,
            start_reg,
            count_operand,
            span,
        );

        for i in (0..count).rev() {
            let reg = start_reg + u16::try_from(i).expect("register offset was range checked");
            self.free_register(reg);
        }

        Ok(())
    }

    pub(super) fn compile_typed_index_access(
        &mut self,
        object: &TypedExpr,
        index: &TypedExpr,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let obj_reg = self.alloc_register()?;
        self.compile_typed_expr(object, obj_reg)?;

        let idx_reg = self.alloc_register()?;
        self.compile_typed_expr(index, idx_reg)?;

        let opcode = match &object.ty {
            InferType::Vec(inner) => Self::select_typed_opcode(
                inner,
                OpCode::VecLoadI,
                OpCode::VecLoadF,
                OpCode::VecLoadB,
                OpCode::VecLoadP,
            ),
            InferType::Array(inner) | InferType::FixedArray(inner, _) => Self::select_typed_opcode(
                inner,
                OpCode::ArrayLoadI,
                OpCode::ArrayLoadF,
                OpCode::ArrayLoadB,
                OpCode::ArrayLoadP,
            ),
            InferType::String => OpCode::StringLoadChar,
            _ => OpCode::VecLoadP,
        };

        self.emit_a(opcode, dest, obj_reg, idx_reg, span);

        self.free_register(idx_reg);
        self.free_register(obj_reg);

        Ok(())
    }

    pub(super) fn compile_typed_index_assign(
        &mut self,
        object: &TypedExpr,
        index: &TypedExpr,
        value: &TypedExpr,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let obj_reg = self.alloc_register()?;
        self.compile_typed_expr(object, obj_reg)?;

        let idx_reg = self.alloc_register()?;
        self.compile_typed_expr(index, idx_reg)?;

        let val_reg = self.alloc_register()?;
        self.compile_typed_expr(value, val_reg)?;

        let opcode = match &object.ty {
            InferType::Vec(inner) => Self::select_typed_opcode(
                inner,
                OpCode::VecStoreI,
                OpCode::VecStoreF,
                OpCode::VecStoreB,
                OpCode::VecStoreP,
            ),
            InferType::Array(inner) | InferType::FixedArray(inner, _) => Self::select_typed_opcode(
                inner,
                OpCode::ArrayStoreI,
                OpCode::ArrayStoreF,
                OpCode::ArrayStoreB,
                OpCode::ArrayStoreP,
            ),
            _ => OpCode::VecStoreP,
        };

        self.emit_a(opcode, obj_reg, idx_reg, val_reg, span);

        if dest != val_reg {
            self.emit_a(OpCode::Move, dest, val_reg, 0, span);
        }

        self.free_register(val_reg);
        self.free_register(idx_reg);
        self.free_register(obj_reg);

        Ok(())
    }

    pub(super) fn compile_typed_slice(
        &mut self,
        object: &TypedExpr,
        range: &TypedExpr,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let opcode = match &object.ty {
            InferType::Array(_) | InferType::FixedArray(_, _) => OpCode::ArraySlice,
            InferType::Vec(_) => OpCode::VecSlice,
            receiver => {
                return Err(CompileError::new(
                    CompileErrorKind::TypeInferenceError(format!(
                        "cannot slice a value of type {receiver}"
                    )),
                    span,
                    self.source.clone(),
                )
                .into());
            }
        };

        let object_reg = self.alloc_register()?;
        self.compile_typed_expr(object, object_reg)?;
        let range_reg = self.alloc_register()?;
        self.compile_typed_expr(range, range_reg)?;
        self.emit_a(opcode, dest, object_reg, range_reg, span);
        self.free_register(range_reg);
        self.free_register(object_reg);
        Ok(())
    }

    pub(super) fn compile_typed_range(
        &mut self,
        start: &Option<Box<TypedExpr>>,
        end: &Option<Box<TypedExpr>>,
        inclusive: bool,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let start_reg = self.alloc_register()?;
        if let Some(start) = start {
            self.compile_typed_expr(start, start_reg)?;
        } else {
            self.emit_a(OpCode::LoadNone, start_reg, 0, 0, span);
        }

        let end_reg = self.alloc_register()?;
        if let Some(end) = end {
            self.compile_typed_expr(end, end_reg)?;
        } else {
            self.emit_a(OpCode::LoadNone, end_reg, 0, 0, span);
        }

        let opcode = if inclusive {
            OpCode::RangeNewInclusive
        } else {
            OpCode::RangeNew
        };
        self.emit_a(opcode, dest, start_reg, end_reg, span);
        self.free_register(end_reg);
        self.free_register(start_reg);
        Ok(())
    }

    fn select_typed_opcode(
        inner: &InferType,
        i: OpCode,
        f: OpCode,
        b: OpCode,
        p: OpCode,
    ) -> OpCode {
        match inner {
            t if t.is_integer() => i,
            t if t.is_float() => f,
            InferType::Bool => b,
            _ => p,
        }
    }

    pub(super) fn compile_array_len(
        &mut self,
        object: &TypedExpr,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let obj_reg = self.alloc_register()?;
        self.compile_typed_expr(object, obj_reg)?;
        self.emit_a(OpCode::ArrayLen, dest, obj_reg, 0, span);
        self.free_register(obj_reg);
        Ok(())
    }

    pub(super) fn compile_vec_len(
        &mut self,
        object: &TypedExpr,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let obj_reg = self.alloc_register()?;
        self.compile_typed_expr(object, obj_reg)?;
        self.emit_a(OpCode::VecLen, dest, obj_reg, 0, span);
        self.free_register(obj_reg);
        Ok(())
    }

    pub(super) fn compile_collection_is_empty(
        &mut self,
        object: &TypedExpr,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let len_reg = self.alloc_register()?;
        match &object.ty {
            InferType::Array(_) | InferType::FixedArray(_, _) => {
                self.compile_array_len(object, len_reg, span)?;
            }
            _ => self.compile_vec_len(object, len_reg, span)?,
        }
        let zero_reg = self.alloc_register()?;
        self.emit_b(OpCode::LoadI, zero_reg, 0, span);
        self.emit_a(OpCode::Eq, dest, len_reg, zero_reg, span);
        self.free_register(zero_reg);
        self.free_register(len_reg);
        Ok(())
    }

    pub(super) fn compile_vec_push(
        &mut self,
        object: &TypedExpr,
        inner: &InferType,
        value: &TypedExpr,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let obj_reg = self.alloc_register()?;
        self.compile_typed_expr(object, obj_reg)?;

        let val_reg = self.alloc_register()?;
        self.compile_typed_expr(value, val_reg)?;

        let opcode = Self::select_typed_opcode(
            inner,
            OpCode::VecPushI,
            OpCode::VecPushF,
            OpCode::VecPushB,
            OpCode::VecPushP,
        );

        self.emit_a(opcode, obj_reg, val_reg, 0, span);
        self.emit_a(OpCode::LoadUnit, dest, 0, 0, span);

        self.free_register(val_reg);
        self.free_register(obj_reg);
        Ok(())
    }

    pub(super) fn compile_vec_pop(
        &mut self,
        object: &TypedExpr,
        inner: &InferType,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let obj_reg = self.alloc_register()?;
        self.compile_typed_expr(object, obj_reg)?;

        let opcode = Self::select_typed_opcode(
            inner,
            OpCode::VecPopI,
            OpCode::VecPopF,
            OpCode::VecPopB,
            OpCode::VecPopP,
        );

        self.emit_a(opcode, dest, obj_reg, 0, span);
        self.free_register(obj_reg);
        Ok(())
    }

    pub(super) fn compile_typed_collection_get(
        &mut self,
        object: &TypedExpr,
        index: &TypedExpr,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let obj_reg = self.alloc_register()?;
        self.compile_typed_expr(object, obj_reg)?;
        let idx_reg = self.alloc_register()?;
        self.compile_typed_expr(index, idx_reg)?;
        let opcode = match &object.ty {
            InferType::Array(inner) | InferType::FixedArray(inner, _) => Self::select_typed_opcode(
                inner,
                OpCode::ArrayGetI,
                OpCode::ArrayGetF,
                OpCode::ArrayGetB,
                OpCode::ArrayGetP,
            ),
            InferType::Vec(inner) => Self::select_typed_opcode(
                inner,
                OpCode::VecGetI,
                OpCode::VecGetF,
                OpCode::VecGetB,
                OpCode::VecGetP,
            ),
            _ => OpCode::VecGetP,
        };
        self.emit_a(opcode, dest, obj_reg, idx_reg, span);
        self.free_register(idx_reg);
        self.free_register(obj_reg);
        Ok(())
    }

    pub(super) fn compile_vec_capacity(
        &mut self,
        object: &TypedExpr,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let obj_reg = self.alloc_register()?;
        self.compile_typed_expr(object, obj_reg)?;
        self.emit_a(OpCode::VecCap, dest, obj_reg, 0, span);
        self.free_register(obj_reg);
        Ok(())
    }

    pub(super) fn compile_vec_reserve(
        &mut self,
        object: &TypedExpr,
        capacity: &TypedExpr,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let obj_reg = self.alloc_register()?;
        self.compile_typed_expr(object, obj_reg)?;

        let cap_reg = self.alloc_register()?;
        self.compile_typed_expr(capacity, cap_reg)?;

        self.emit_a(OpCode::VecReserve, obj_reg, cap_reg, 0, span);
        self.emit_a(OpCode::LoadUnit, dest, 0, 0, span);

        self.free_register(cap_reg);
        self.free_register(obj_reg);
        Ok(())
    }
}
