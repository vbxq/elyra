use super::super::Compiler;
use aelys_common::Result;
use aelys_common::error::{CompileError, CompileErrorKind};
use aelys_syntax::MemberSeparator;
use aelys_syntax::Span;
use aelys_syntax::ast::Expr;

impl Compiler {
    pub(super) fn try_compile_module_call(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        dest: u16,
        span: Span,
    ) -> Result<bool> {
        if let Some((module_name, member, separator)) = Self::is_member_call(callee)
            && self.has_module_alias(&module_name)
        {
            if separator == MemberSeparator::Dot {
                return Err(CompileError::new(
                    CompileErrorKind::ModulePathSeparator {
                        module: module_name.to_string(),
                        member: member.to_string(),
                    },
                    span,
                    self.source.clone(),
                )
                .into());
            }
            let qualified_name = format!("{}::{}", module_name, member);
            let global_idx = self.get_or_create_global_index(&qualified_name);
            self.accessed_globals.insert(qualified_name.clone());

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
            self.emit_call_global_cached(dest, global_idx, nargs, &qualified_name, span);
            self.release_arg_registers(arg_start, args.len());
            return Ok(true);
        }

        Ok(false)
    }
}
