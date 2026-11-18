mod control_flow;
mod control_if;
mod decl;
mod expression;
mod hoist;
mod looping;
mod typed;
mod value;

use super::Compiler;
use aelys_common::Result;
use aelys_syntax::Span;
use aelys_syntax::ast::{Stmt, StmtKind};

impl Compiler {
    pub fn compile_stmt(&mut self, stmt: &Stmt) -> Result<()> {
        match &stmt.kind {
            StmtKind::Expression(expr) => self.compile_expression_stmt(expr),
            StmtKind::Let {
                name,
                mutable,
                type_annotation,
                initializer,
                is_pub,
                ..
            } => self.compile_let(
                name,
                *mutable,
                type_annotation.as_ref(),
                initializer,
                *is_pub,
            ),
            StmtKind::Block(stmts) => self.compile_block(stmts),
            StmtKind::If {
                condition,
                then_branch,
                else_branch,
            } => self.compile_if(condition, then_branch, else_branch.as_deref()),
            StmtKind::While {
                condition,
                body,
                label: None,
            } => self.compile_while(condition, body),
            StmtKind::For {
                iterator,
                start,
                end,
                inclusive,
                step,
                body,
                label: None,
            } => self.compile_for(
                iterator,
                start,
                end,
                *inclusive,
                step.as_ref().as_ref(),
                body,
                stmt.span,
            ),
            StmtKind::While { .. } | StmtKind::For { .. } => Err(untyped_unsupported(
                "a labeled loop",
                stmt.span,
                &self.source,
            )),
            StmtKind::ForEach { .. } => Ok(()),
            StmtKind::Break {
                label: None,
                value: None,
            } => self.compile_break(stmt.span),
            StmtKind::Continue { label: None } => self.compile_continue(stmt.span),
            StmtKind::Break { .. } | StmtKind::Continue { .. } => Err(untyped_unsupported(
                "a labeled or valued `break`/`continue`",
                stmt.span,
                &self.source,
            )),
            StmtKind::Return(expr) => self.compile_return(expr.as_ref(), stmt.span),
            StmtKind::Function(func) => self.compile_function(func),
            StmtKind::ImplDecl { .. } => Ok(()),
            StmtKind::Needs(needs) => self.compile_needs(needs, stmt.span),
            StmtKind::StructDecl { .. }
            | StmtKind::TraitDecl { .. }
            | StmtKind::EnumDecl { .. } => Ok(()),
        }
    }

    pub fn compile_block(&mut self, stmts: &[Stmt]) -> Result<()> {
        self.begin_scope();
        for s in stmts {
            self.compile_stmt(s)?;
        }
        self.end_scope();
        Ok(())
    }

    pub fn compile_needs(&mut self, _: &aelys_syntax::ast::NeedsStmt, _: Span) -> Result<()> {
        Ok(())
    }
}

pub(crate) fn untyped_unsupported(
    what: &str,
    span: Span,
    source: &std::sync::Arc<aelys_syntax::Source>,
) -> aelys_common::AelysError {
    aelys_common::error::CompileError::new(
        aelys_common::error::CompileErrorKind::TypeInferenceError(format!(
            "{what} needs the typed pipeline"
        )),
        span,
        source.clone(),
    )
    .into()
}
