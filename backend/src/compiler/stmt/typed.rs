use super::Compiler;
use aelys_common::Result;

impl Compiler {
    pub fn compile_typed_stmt(&mut self, stmt: &aelys_sema::TypedStmt) -> Result<()> {
        use aelys_sema::TypedStmtKind;

        match &stmt.kind {
            TypedStmtKind::Expression(expr) => {
                let reg = self.alloc_register()?;
                match &expr.kind {
                    aelys_sema::TypedExprKind::IndexAssign {
                        object,
                        index,
                        value,
                    } => self.compile_typed_index_assign(object, index, value, None, expr.span)?,
                    aelys_sema::TypedExprKind::Assign { name, value } => {
                        self.compile_typed_assign_statement(name, value, reg, expr.span)?
                    }
                    _ => self.compile_typed_expr(expr, reg)?,
                }
                self.free_register(reg);
                Ok(())
            }
            TypedStmtKind::Let {
                name,
                mutable,
                initializer,
                var_type,
                is_pub,
            } => {
                let resolved_type = aelys_sema::ResolvedType::from_infer_type(var_type);
                self.compile_typed_let(
                    name,
                    *mutable,
                    initializer,
                    &resolved_type,
                    *is_pub,
                    stmt.span,
                )
            }
            TypedStmtKind::Block(stmts) => {
                self.begin_scope();
                for s in stmts {
                    self.compile_typed_stmt(s)?;
                }
                self.end_scope();
                Ok(())
            }
            TypedStmtKind::If {
                condition,
                then_branch,
                else_branch,
            } => self.compile_typed_if_stmt(
                condition,
                then_branch,
                else_branch.as_deref(),
                stmt.span,
            ),
            TypedStmtKind::While {
                condition,
                body,
                label,
            } => self.compile_typed_while(condition, body, stmt.span, label.as_ref()),
            TypedStmtKind::For {
                iterator,
                start,
                end,
                inclusive,
                step,
                body,
                label,
            } => self.compile_typed_for(
                iterator,
                start,
                end,
                *inclusive,
                step.as_ref().as_ref(),
                body,
                stmt.span,
                label.as_ref(),
            ),
            TypedStmtKind::ForEach {
                iterator,
                iterable,
                elem_type,
                read_only: _,
                body,
                label,
            } => self.compile_typed_for_each(
                iterator,
                iterable,
                elem_type,
                body,
                stmt.span,
                label.as_ref(),
            ),
            TypedStmtKind::Return(expr) => self.compile_typed_return(expr.as_ref(), stmt.span),
            TypedStmtKind::Break { label, value } => {
                self.compile_typed_break(label.as_ref(), value.as_ref(), stmt.span)
            }
            TypedStmtKind::Continue { label } => {
                self.compile_typed_continue(label.as_ref(), stmt.span)
            }
            TypedStmtKind::Function(func) => self.compile_typed_function(func),
            TypedStmtKind::ImplDecl { methods, .. } => {
                for method in methods {
                    self.compile_typed_function(method)?;
                }
                Ok(())
            }
            TypedStmtKind::Needs(_needs) => Ok(()),
            TypedStmtKind::Extern(functions) => self.compile_typed_extern(functions, stmt.span),
            TypedStmtKind::StructDecl { .. }
            | TypedStmtKind::TraitDecl { .. }
            | TypedStmtKind::EnumDecl { .. } => Ok(()),
        }
    }
}
