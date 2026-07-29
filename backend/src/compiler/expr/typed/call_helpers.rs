use super::super::Compiler;
use aelys_bytecode::OpCode;
use aelys_common::Result;
use aelys_syntax::Span;

impl Compiler {
    pub(super) fn compile_typed_builtin_call(
        &mut self,
        name: &str,
        args: &[aelys_sema::TypedExpr],
        dest: u16,
        span: Span,
    ) -> Result<()> {
        // Fallback for builtins like `type`.
        let idx = self.get_or_create_global_index(name);
        self.accessed_globals.insert(name.to_string());

        let arg_start = match dest.checked_add(1) {
            Some(s) => s,
            None => return self.compile_typed_call_generic(name, args, dest, span),
        };

        for i in 0..args.len() {
            let arg_reg = match arg_start
                .checked_add(u16::try_from(i).expect("register offset was range checked"))
            {
                Some(r) => r,
                None => return self.compile_typed_call_generic(name, args, dest, span),
            };
            if (arg_reg as usize) >= self.register_pool.len() {
                return self.compile_typed_call_generic(name, args, dest, span);
            }
            if self.register_pool[arg_reg as usize] {
                return self.compile_typed_call_generic(name, args, dest, span);
            }
        }

        for i in 0..args.len() {
            let arg_reg = arg_start + u16::try_from(i).expect("register offset was range checked");
            self.register_pool[arg_reg as usize] = true;
            if u32::from(arg_reg) >= self.next_register {
                self.next_register = u32::from(arg_reg) + 1;
            }
        }

        for (i, arg) in args.iter().enumerate() {
            let arg_reg = arg_start + u16::try_from(i).expect("register offset was range checked");
            self.compile_typed_expr(arg, arg_reg)?;
        }

        let nargs = self.checked_call_arity(args.len(), span)?;
        self.emit_call_global_cached(dest, idx, nargs, name, span);

        for i in (0..args.len()).rev() {
            let arg_reg = arg_start + u16::try_from(i).expect("register offset was range checked");
            self.free_register(arg_reg);
        }

        Ok(())
    }

    pub(super) fn compile_typed_call_generic(
        &mut self,
        name: &str,
        args: &[aelys_sema::TypedExpr],
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let nargs = self.checked_call_arity(args.len(), span)?;
        let register_count = args.len().saturating_add(1);
        let callee_reg = self.alloc_consecutive_registers_for_call(register_count, span)?;

        for i in 0..register_count {
            let reg = callee_reg + u16::try_from(i).expect("argument count was range checked");
            self.register_pool[reg as usize] = true;
            if u32::from(reg) >= self.next_register {
                self.next_register = u32::from(reg) + 1;
            }
        }

        self.compile_identifier(name, callee_reg, span)?;

        for (i, arg) in args.iter().enumerate() {
            let arg_reg =
                callee_reg + 1 + u16::try_from(i).expect("argument count was range checked");
            self.compile_typed_expr(arg, arg_reg)?;
        }

        self.emit_c(OpCode::Call, dest, callee_reg, nargs, span);

        for i in (0..register_count).rev() {
            let reg = callee_reg + u16::try_from(i).expect("argument count was range checked");
            self.free_register(reg);
        }

        Ok(())
    }
}
