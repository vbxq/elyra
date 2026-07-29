use super::super::Compiler;
use aelys_bytecode::OpCode;
use aelys_common::Result;
use aelys_syntax::Span;
use aelys_syntax::ast::{Expr, ExprKind};

impl Compiler {
    // CallCached optimization: when calling a function stored in a local
    // variable, we can use the cached call path which avoids the global
    // index lookup overhead of CallGlobal.
    //
    // Example: let f = some_function; f(args)  ->  CallCached r(dest), r(f), nargs
    // Instead of: let f = some_function; f(args)  ->  Call r(dest), r(f), nargs
    //
    // CallCached handles Function, Native, and Closure objects, while
    // Call always calls through a Function wrapper.
    pub(super) fn try_compile_cached_call(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        dest: u16,
        span: Span,
    ) -> Result<bool> {
        if let ExprKind::Identifier(name) = &callee.kind {
            // Only use this path if it's a local variable (not a global)
            if let Some((callee_reg, _mutable)) = self.resolve_variable(name) {
                let nargs = args.len();
                let Some(arg_start) = dest.checked_add(1) else {
                    return Ok(false);
                };
                if !self.reserve_arg_registers(arg_start, nargs) {
                    return Ok(false);
                }

                for (i, arg) in args.iter().enumerate() {
                    let arg_reg =
                        arg_start + u16::try_from(i).expect("register offset was range checked");
                    self.compile_expr(arg, arg_reg)?;
                }

                let call_arity = self.checked_call_arity(nargs, span)?;
                self.emit_c(OpCode::CallCached, dest, callee_reg, call_arity, span);
                self.release_arg_registers(arg_start, nargs);
                return Ok(true);
            }
        }

        Ok(false)
    }
}
