use super::Compiler;
use aelys_bytecode::OpCode;
use aelys_common::Result;
use aelys_common::error::{CompileError, CompileErrorKind};
use aelys_sema::{TypedExpr, TypedStmt, TypedStmtKind};
use aelys_syntax::Span;

impl Compiler {
    pub fn typed_stmt_yields_value(stmt: &TypedStmt) -> bool {
        match &stmt.kind {
            TypedStmtKind::Expression(_) => true,
            TypedStmtKind::If {
                then_branch,
                else_branch: Some(else_branch),
                ..
            } => {
                Self::typed_stmt_yields_value(then_branch)
                    || Self::typed_stmt_yields_value(else_branch)
            }
            TypedStmtKind::Block(stmts) => stmts.last().is_some_and(Self::typed_stmt_yields_value),
            _ => false,
        }
    }

    pub fn compile_typed_stmt_value(&mut self, stmt: &TypedStmt, dest: u16) -> Result<()> {
        match &stmt.kind {
            TypedStmtKind::Expression(expr) => self.compile_typed_expr(expr, dest),
            TypedStmtKind::Block(stmts) => self.compile_typed_stmts_value(stmts, dest, stmt.span),
            TypedStmtKind::If {
                condition,
                then_branch,
                else_branch: Some(else_branch),
            } => {
                let cond_reg = self.alloc_register()?;
                self.compile_typed_expr(condition, cond_reg)?;
                let jump_to_else = self.emit_jump_if(OpCode::JumpIfNot, cond_reg, condition.span);
                self.free_register(cond_reg);

                self.compile_typed_stmt_value(then_branch, dest)?;
                let jump_to_end = self.emit_jump(OpCode::Jump, then_branch.span);
                self.patch_jump(jump_to_else);
                self.compile_typed_stmt_value(else_branch, dest)?;
                self.patch_jump(jump_to_end);
                Ok(())
            }
            _ => {
                self.compile_typed_stmt(stmt)?;
                self.emit_a(OpCode::LoadUnit, dest, 0, 0, stmt.span);
                Ok(())
            }
        }
    }

    pub fn compile_typed_stmts_value(
        &mut self,
        stmts: &[TypedStmt],
        dest: u16,
        span: Span,
    ) -> Result<()> {
        self.begin_scope();
        match stmts.split_last() {
            None => self.emit_a(OpCode::LoadUnit, dest, 0, 0, span),
            Some((last, prefix)) => {
                for stmt in prefix {
                    self.compile_typed_stmt(stmt)?;
                }
                self.compile_typed_stmt_value(last, dest)?;
            }
        }
        self.end_scope();
        Ok(())
    }

    pub fn compile_typed_loop(
        &mut self,
        body: &[TypedStmt],
        label: Option<&String>,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        self.emit_a(OpCode::LoadUnit, dest, 0, 0, span);
        let loop_start = self.current_offset();
        let mut context = super::super::LoopContext::new(loop_start, false, self.locals.len());
        context.label = label.cloned();
        context.result = Some(dest);
        context.captured_names = super::super::liveness::captured_names(body);
        self.loop_stack.push(context);

        self.begin_scope();
        for stmt in body {
            self.compile_typed_stmt(stmt)?;
        }
        self.end_scope();
        self.emit_jump_back(loop_start, span);

        let context = self.loop_stack.pop().ok_or_else(|| {
            CompileError::new(
                CompileErrorKind::BreakOutsideLoop,
                span,
                self.source.clone(),
            )
        })?;
        for jump in context.break_jumps {
            self.patch_jump(jump);
        }
        Ok(())
    }

    fn loop_target(
        &self,
        label: Option<&String>,
        span: Span,
        outside: CompileErrorKind,
    ) -> Result<usize> {
        let found = match label {
            None => self.loop_stack.len().checked_sub(1),
            Some(label) => self
                .loop_stack
                .iter()
                .rposition(|context| context.label.as_ref() == Some(label)),
        };
        found.ok_or_else(|| CompileError::new(outside, span, self.source.clone()).into())
    }

    // a closure created in the loop body keeps its own copy of the iteration's locals
    fn close_loop_upvalues(&mut self, target: usize, span: Span) {
        let start = self.loop_stack[target].locals_start;
        let lowest = self
            .locals
            .iter()
            .skip(start)
            .filter(|local| {
                local.is_captured || self.loop_stack[target].captured_names.contains(&local.name)
            })
            .map(|local| local.register)
            .min();
        if let Some(register) = lowest {
            self.emit_a(OpCode::CloseUpvals, register, 0, 0, span);
        }
    }

    pub fn compile_typed_break(
        &mut self,
        label: Option<&String>,
        value: Option<&TypedExpr>,
        span: Span,
    ) -> Result<()> {
        let target = self.loop_target(label, span, CompileErrorKind::BreakOutsideLoop)?;
        if let Some(value) = value {
            let Some(result) = self.loop_stack[target].result else {
                return Err(CompileError::new(
                    CompileErrorKind::TypeInferenceError(
                        "`break` with a value reached a loop that produces no value".to_string(),
                    ),
                    span,
                    self.source.clone(),
                )
                .into());
            };
            self.compile_typed_expr(value, result)?;
        }
        self.close_loop_upvalues(target, span);
        let jump = self.emit_jump(OpCode::Jump, span);
        self.loop_stack[target].break_jumps.push(jump);
        Ok(())
    }

    pub fn compile_typed_continue(&mut self, label: Option<&String>, span: Span) -> Result<()> {
        let target = self.loop_target(label, span, CompileErrorKind::ContinueOutsideLoop)?;
        self.close_loop_upvalues(target, span);
        if self.loop_stack[target].is_for_loop {
            let jump = self.emit_jump(OpCode::Jump, span);
            self.loop_stack[target].continue_jumps.push(jump);
        } else {
            let start = self.loop_stack[target].start;
            self.emit_jump_back(start, span);
        }
        Ok(())
    }
}
