use super::Compiler;
use aelys_bytecode::OpCode;
use aelys_common::Result;
use aelys_common::error::{CompileError, CompileErrorKind};
use aelys_syntax::Span;

impl Compiler {
    pub fn compile_break(&mut self, span: Span) -> Result<()> {
        if self.loop_stack.is_empty() {
            return Err(CompileError::new(
                CompileErrorKind::BreakOutsideLoop,
                span,
                self.source.clone(),
            )
            .into());
        }

        let jump_offset = self.emit_jump(OpCode::Jump, span);
        if let Some(loop_ctx) = self.loop_stack.last_mut() {
            loop_ctx.break_jumps.push(jump_offset);
        } else {
            return Err(CompileError::new(
                CompileErrorKind::BreakOutsideLoop,
                span,
                self.source.clone(),
            )
            .into());
        }

        Ok(())
    }

    pub fn compile_continue(&mut self, span: Span) -> Result<()> {
        if let Some(loop_ctx) = self.loop_stack.last() {
            let is_for_loop = loop_ctx.is_for_loop;
            let loop_start = loop_ctx.start;

            if is_for_loop {
                let jump_offset = self.emit_jump(OpCode::Jump, span);
                if let Some(loop_ctx) = self.loop_stack.last_mut() {
                    loop_ctx.continue_jumps.push(jump_offset);
                } else {
                    return Err(CompileError::new(
                        CompileErrorKind::ContinueOutsideLoop,
                        span,
                        self.source.clone(),
                    )
                    .into());
                }
            } else {
                self.emit_jump_back(loop_start, span);
            }
            Ok(())
        } else {
            Err(CompileError::new(
                CompileErrorKind::ContinueOutsideLoop,
                span,
                self.source.clone(),
            )
            .into())
        }
    }

    pub fn compile_return(
        &mut self,
        expr: Option<&aelys_syntax::ast::Expr>,
        span: Span,
    ) -> Result<()> {
        if self.function_depth == 0 {
            return Err(CompileError::new(
                CompileErrorKind::ReturnOutsideFunction,
                span,
                self.source.clone(),
            )
            .into());
        }

        if let Some(expr) = expr {
            let reg = self.alloc_register()?;
            self.compile_expr(expr, reg)?;

            let lowest_captured = self
                .locals
                .iter()
                .filter(|l| l.is_captured)
                .map(|l| l.register)
                .min();

            if let Some(from_reg) = lowest_captured {
                self.emit_a(OpCode::CloseUpvals, from_reg, 0, 0, span);
            }

            self.emit_a(OpCode::Return, reg, 0, 0, span);
            self.free_register(reg);
        } else {
            let lowest_captured = self
                .locals
                .iter()
                .filter(|l| l.is_captured)
                .map(|l| l.register)
                .min();

            if let Some(from_reg) = lowest_captured {
                self.emit_a(OpCode::CloseUpvals, from_reg, 0, 0, span);
            }

            self.emit_return0(span);
        }
        Ok(())
    }

    pub fn compile_typed_return(
        &mut self,
        expr: Option<&aelys_sema::TypedExpr>,
        span: Span,
    ) -> Result<()> {
        if self.function_depth == 0 {
            return Err(CompileError::new(
                CompileErrorKind::ReturnOutsideFunction,
                span,
                self.source.clone(),
            )
            .into());
        }

        if let Some(e) = expr {
            let reg = self.alloc_register()?;
            self.compile_typed_expr(e, reg)?;

            let lowest_captured = self
                .locals
                .iter()
                .filter(|l| l.is_captured)
                .map(|l| l.register)
                .min();

            if let Some(from_reg) = lowest_captured {
                self.emit_a(OpCode::CloseUpvals, from_reg, 0, 0, span);
            }

            self.emit_a(OpCode::Return, reg, 0, 0, span);
            self.free_register(reg);
        } else {
            let lowest_captured = self
                .locals
                .iter()
                .filter(|l| l.is_captured)
                .map(|l| l.register)
                .min();

            if let Some(from_reg) = lowest_captured {
                self.emit_a(OpCode::CloseUpvals, from_reg, 0, 0, span);
            }

            self.emit_a(OpCode::Return0, 0, 0, 0, span);
        }

        Ok(())
    }
}
