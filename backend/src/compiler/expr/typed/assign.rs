use super::super::Compiler;
use aelys_bytecode::OpCode;
use aelys_common::Result;
use aelys_common::error::{CompileError, CompileErrorKind};
use aelys_syntax::Span;
use aelys_syntax::ast::BinaryOp;

impl Compiler {
    fn try_compile_global_integer_add(
        &mut self,
        name: &str,
        value: &aelys_sema::TypedExpr,
        dest: u16,
        span: Span,
    ) -> Result<bool> {
        let aelys_sema::TypedExprKind::Binary { left, op, right } = &value.kind else {
            return Ok(false);
        };
        let aelys_sema::TypedExprKind::Identifier(left_name) = &left.kind else {
            return Ok(false);
        };
        if *op != BinaryOp::Add || left_name != name {
            return Ok(false);
        }
        if !aelys_sema::ResolvedType::from_infer_type(&left.ty).is_integer()
            || !aelys_sema::ResolvedType::from_infer_type(&right.ty).is_integer()
        {
            return Ok(false);
        }
        let index = self.get_or_create_global_index_raw(name);
        let (Ok(narrow_dest), Ok(index)) = (u8::try_from(dest), u8::try_from(index)) else {
            return Ok(false);
        };
        if Self::typed_expr_may_have_side_effects(right) {
            return Ok(false);
        }
        let source = if let aelys_sema::TypedExprKind::Identifier(right_name) = &right.kind
            && let Some((source, _)) = self.resolve_variable(right_name)
        {
            source
        } else {
            self.compile_typed_expr(right, dest)?;
            dest
        };
        let Ok(source) = u8::try_from(source) else {
            return Ok(false);
        };
        self.accessed_globals.insert(name.to_string());
        let line = self.current_line(span);
        self.current
            .emit_a(OpCode::AddGlobalI, narrow_dest, source, index, line);
        Ok(true)
    }

    pub(super) fn compile_typed_assign(
        &mut self,
        name: &str,
        value: &aelys_sema::TypedExpr,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        if self.resolve_variable(name).is_none()
            && self.resolve_upvalue(name).is_none()
            && self.globals.get(name).copied() == Some(true)
            && self.try_compile_global_integer_add(name, value, dest, span)?
        {
            return Ok(());
        }
        self.compile_typed_expr(value, dest)?;

        if let Some((reg, mutable)) = self.resolve_variable(name) {
            if !mutable {
                return Err(CompileError::new(
                    CompileErrorKind::AssignToImmutable(name.to_string()),
                    span,
                    self.source.clone(),
                )
                .into());
            }
            if reg != dest {
                self.emit_a(OpCode::Move, reg, dest, 0, span);
            }
        } else if let Some((upval_idx, mutable)) = self.resolve_upvalue(name) {
            if !mutable {
                return Err(CompileError::new(
                    CompileErrorKind::AssignToImmutable(name.to_string()),
                    span,
                    self.source.clone(),
                )
                .into());
            }
            self.emit_a(OpCode::SetUpval, upval_idx, dest, 0, span);
        } else {
            if let Some(&mutable) = self.globals.get(name)
                && !mutable
            {
                return Err(CompileError::new(
                    CompileErrorKind::AssignToImmutable(name.to_string()),
                    span,
                    self.source.clone(),
                )
                .into());
            }
            // For assignments to user-defined globals, use raw index without translation
            let idx = self.get_or_create_global_index_raw(name);
            self.accessed_globals.insert(name.to_string());
            self.emit_set_global_index(dest, idx, span);
        }

        Ok(())
    }

    pub(crate) fn compile_typed_assign_statement(
        &mut self,
        name: &str,
        value: &aelys_sema::TypedExpr,
        temp: u16,
        span: Span,
    ) -> Result<()> {
        let Some((reg, true)) = self.resolve_variable(name) else {
            return self.compile_typed_assign(name, value, temp, span);
        };
        if !Self::has_single_final_writer(value) {
            return self.compile_typed_assign(name, value, temp, span);
        }
        let start = self.current.current_offset();
        self.compile_typed_expr(value, temp)?;
        let end = self.current.current_offset();
        if let Some(position) = self.last_instruction_start(start, end)
            && self.current.bytecode_at(position) >> 24 == u32::from(u8::from(OpCode::StructLoad))
            && self.current.bytecode_at(position + 1) >> 16 == u32::from(temp)
        {
            let operands = self.current.bytecode_at(position + 1);
            *self.current.bytecode_mut(position + 1) = (operands & 0xffff) | (u32::from(reg) << 16);
            return Ok(());
        }
        if let Some(position) = self.last_instruction_start(start, end)
            && let (Ok(target), Ok(temp)) = (u8::try_from(reg), u8::try_from(temp))
        {
            let word = self.current.bytecode_at(position);
            if ((word >> 16) & 0xff) as u8 == temp
                && OpCode::from_u8((word >> 24) as u8).is_some_and(Self::writes_only_its_a_operand)
            {
                *self.current.bytecode_mut(position) =
                    (word & 0xff00_ffff) | (u32::from(target) << 16);
                return Ok(());
            }
        }
        if reg != temp {
            self.emit_a(OpCode::Move, reg, temp, 0, span);
        }
        Ok(())
    }

    fn has_single_final_writer(value: &aelys_sema::TypedExpr) -> bool {
        use aelys_sema::TypedExprKind;
        match &value.kind {
            TypedExprKind::Binary { .. }
            | TypedExprKind::Unary { .. }
            | TypedExprKind::Identifier(_)
            | TypedExprKind::Int(_)
            | TypedExprKind::Float(_)
            | TypedExprKind::Bool(_)
            | TypedExprKind::Index { .. }
            | TypedExprKind::StructField { .. } => true,
            TypedExprKind::Grouping(inner) => Self::has_single_final_writer(inner),
            _ => false,
        }
    }

    fn last_instruction_start(&self, start: usize, end: usize) -> Option<usize> {
        let mut position = start;
        while position < end {
            let opcode = OpCode::from_u8((self.current.bytecode_at(position) >> 24) as u8)?;
            let next = position + 1 + opcode.extension_words();
            if next == end {
                return Some(position);
            }
            position = next;
        }
        None
    }

    fn writes_only_its_a_operand(opcode: OpCode) -> bool {
        use OpCode::*;
        matches!(
            opcode,
            Move | LoadI
                | LoadK
                | LoadNull
                | LoadBool
                | Add
                | Sub
                | Mul
                | Div
                | Mod
                | Neg
                | Eq
                | Ne
                | Lt
                | Le
                | Gt
                | Ge
                | Not
                | AddI
                | SubI
                | LtImm
                | LeImm
                | GtImm
                | GeImm
                | AddII
                | SubII
                | MulII
                | DivII
                | ModII
                | AddFF
                | SubFF
                | MulFF
                | DivFF
                | ModFF
                | LtII
                | LeII
                | GtII
                | GeII
                | EqII
                | NeII
                | LtFF
                | LeFF
                | GtFF
                | GeFF
                | EqFF
                | NeFF
                | LtIImm
                | LeIImm
                | GtIImm
                | GeIImm
                | GetGlobalIdx
                | Shl
                | Shr
                | BitAnd
                | BitOr
                | BitXor
                | BitNot
                | ShlII
                | ShrII
                | AndII
                | OrII
                | XorII
                | NotI
                | ShlIImm
                | ShrIImm
                | AndIImm
                | OrIImm
                | XorIImm
                | ArrayLoadI
                | ArrayLoadF
                | ArrayLoadB
                | ArrayLoadP
                | ArrayLen
                | VecLen
                | VecLoadI
                | VecLoadF
                | VecLoadB
                | VecLoadP
        )
    }
}
