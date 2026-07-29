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
}
