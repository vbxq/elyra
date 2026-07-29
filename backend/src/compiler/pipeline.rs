use super::Compiler;
use aelys_bytecode::{Function, GlobalLayout, OpCode};
use aelys_common::Result;
use aelys_common::error::{CompileError, CompileErrorKind};
use aelys_sema::{TypedProgram, TypedStmtKind};
use std::collections::HashMap;
use std::sync::Arc;

impl Compiler {
    pub fn compile_typed(
        mut self,
        program: &TypedProgram,
    ) -> Result<(Function, HashMap<String, bool>)> {
        for stmt in &program.stmts {
            match &stmt.kind {
                TypedStmtKind::Function(func) => {
                    self.globals.insert(func.name.clone(), false);
                    if !self.global_indices.contains_key(&func.name) {
                        let idx = self.next_global_index;
                        self.global_indices.insert(func.name.clone(), idx);
                        self.next_global_index += 1;
                    }
                }
                TypedStmtKind::Let { name, mutable, .. } => {
                    self.globals.insert(name.clone(), *mutable);
                    if !self.global_indices.contains_key(name) {
                        let idx = self.next_global_index;
                        self.global_indices.insert(name.clone(), idx);
                        self.next_global_index += 1;
                    }
                }
                _ => {}
            }
        }

        if program.stmts.is_empty() {
            self.emit_return0(aelys_syntax::Span::dummy());
        } else {
            let last_idx = program.stmts.len() - 1;

            for stmt in &program.stmts[..last_idx] {
                self.compile_typed_stmt(stmt)?;
            }

            let last_stmt = &program.stmts[last_idx];
            match &last_stmt.kind {
                TypedStmtKind::Expression(expr) => {
                    let result_reg = self.alloc_register()?;
                    self.compile_typed_expr(expr, result_reg)?;
                    self.emit_a(OpCode::Return, result_reg, 0, 0, last_stmt.span);
                }
                // If with else branch can return a value
                TypedStmtKind::If {
                    condition,
                    then_branch,
                    else_branch: Some(else_branch),
                } => {
                    let result_reg = self.alloc_register()?;
                    let cond_reg = self.alloc_register()?;
                    self.compile_typed_expr(condition, cond_reg)?;
                    let else_jump = self.emit_jump_if(OpCode::JumpIfNot, cond_reg, condition.span);
                    self.free_register(cond_reg);

                    self.compile_typed_if_branch_for_return(then_branch, result_reg)?;
                    let end_jump = self.emit_jump(OpCode::Jump, then_branch.span);
                    self.patch_jump(else_jump);
                    self.compile_typed_if_branch_for_return(else_branch, result_reg)?;
                    self.patch_jump(end_jump);

                    self.emit_a(OpCode::Return, result_reg, 0, 0, last_stmt.span);
                }
                // Block can return value of its last expression
                TypedStmtKind::Block(_) => {
                    let result_reg = self.alloc_register()?;
                    self.compile_typed_if_branch_for_return(last_stmt, result_reg)?;
                    self.emit_a(OpCode::Return, result_reg, 0, 0, last_stmt.span);
                }
                _ => {
                    self.compile_typed_stmt(last_stmt)?;
                    self.emit_return0(last_stmt.span);
                }
            }
        }

        self.current.num_registers = self.next_register;
        self.current.global_layout = self.build_global_layout();
        self.current.compute_global_layout_hash();
        self.current.finalize_bytecode();

        if self.current.wide_operand_error().is_some() {
            return Err(CompileError::new(
                CompileErrorKind::TooManyRegisters,
                aelys_syntax::Span::dummy(),
                self.source.clone(),
            )
            .into());
        }
        if let Some(distance) = self.current.jump_overflow() {
            return Err(CompileError::new(
                CompileErrorKind::JumpOffsetTooLarge { distance },
                aelys_syntax::Span::dummy(),
                self.source.clone(),
            )
            .into());
        }

        Ok((self.current, self.globals))
    }

    pub(super) fn build_global_layout(&self) -> Arc<GlobalLayout> {
        if self.accessed_globals.is_empty() {
            GlobalLayout::empty()
        } else {
            let global_count = usize::try_from(self.next_global_index)
                .expect("u32 global count fits target usize");
            let mut names = vec![String::new(); global_count];
            for (name, &idx) in &self.global_indices {
                if self.accessed_globals.contains(name) {
                    names[usize::try_from(idx).expect("u32 global index fits target usize")] =
                        name.clone();
                }
            }
            GlobalLayout::new(names)
        }
    }
}
