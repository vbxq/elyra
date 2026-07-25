use super::super::Compiler;
use aelys_common::Result;
use aelys_syntax::Span;
use aelys_syntax::ast::Expr;

impl Compiler {
    pub fn try_compile_builtin_call(
        &mut self,
        _callee: &Expr,
        _args: &[Expr],
        _dest: u8,
        _span: Span,
    ) -> Result<bool> {
        // Manual heap builtins (alloc/free/load/store) have been removed
        Ok(false)
    }
}
