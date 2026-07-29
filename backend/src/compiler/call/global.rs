use super::super::Compiler;
use aelys_common::Result;
use aelys_syntax::Span;
use aelys_syntax::ast::{Expr, ExprKind};

impl Compiler {
    // CallGlobal optimization: if we know it's a global function call, we can
    // skip the local/upvalue lookup entirely and use a cached global index.
    // Big win for stdlib calls like print(), len(), etc.
    pub(super) fn try_compile_global_call(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        dest: u16,
        span: Span,
    ) -> Result<bool> {
        if let ExprKind::Identifier(name) = &callee.kind
            // only use this path if the name isn't shadowed by a local or upvalue
            && self.resolve_variable(name).is_none() && self.resolve_upvalue(name).is_none()
        {
            let global_idx = if let Some(&idx) = self.global_indices.get(name) {
                idx
            } else if self.globals.contains_key(name)
                || Self::is_builtin(name)
                || self.known_globals.contains(name)
            {
                let idx = self.next_global_index;
                self.global_indices.insert(name.to_string(), idx);
                self.next_global_index += 1;
                idx
            } else {
                return Ok(false);
            };

            self.accessed_globals.insert(name.to_string());

            let arg_start = match self.checked_arg_start(dest) {
                Some(s) => s,
                None => {
                    self.compile_call_generic(callee, args, dest, span)?;
                    return Ok(true);
                }
            };

            if !self.reserve_arg_registers(arg_start, args.len()) {
                self.compile_call_generic(callee, args, dest, span)?;
                return Ok(true);
            }

            for (i, arg) in args.iter().enumerate() {
                let arg_reg =
                    arg_start + u16::try_from(i).expect("register offset was range checked");
                self.compile_expr(arg, arg_reg)?;
            }

            let nargs = self.checked_call_arity(args.len(), span)?;
            self.emit_call_global_cached(dest, global_idx, nargs, name, span);
            self.release_arg_registers(arg_start, args.len());
            return Ok(true);
        }

        Ok(false)
    }
}
