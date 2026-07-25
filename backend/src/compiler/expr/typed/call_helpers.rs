use super::super::Compiler;
use aelys_bytecode::OpCode;
use aelys_common::Result;
use aelys_syntax::Span;

impl Compiler {
    pub(super) fn compile_typed_builtin_call(
        &mut self,
        name: &str,
        args: &[aelys_sema::TypedExpr],
        dest: u8,
        span: Span,
    ) -> Result<()> {
        // fallback to CallGlobalNative for builtins like 'type'
        let idx = self.get_or_create_global_index(name);
        self.accessed_globals.insert(name.to_string());

        if idx > 255 {
            return self.compile_typed_call_generic(name, args, dest, span);
        }

        let arg_start = match dest.checked_add(1) {
            Some(s) => s,
            None => return self.compile_typed_call_generic(name, args, dest, span),
        };

        for i in 0..args.len() {
            let arg_reg = match arg_start.checked_add(i as u8) {
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
            let arg_reg = arg_start + i as u8;
            self.register_pool[arg_reg as usize] = true;
            if arg_reg >= self.next_register {
                self.next_register = arg_reg + 1;
            }
        }

        for (i, arg) in args.iter().enumerate() {
            let arg_reg = arg_start + i as u8;
            self.compile_typed_expr(arg, arg_reg)?;
        }

        self.emit_call_global_cached(dest, idx as u8, args.len() as u8, name, span);

        for i in (0..args.len()).rev() {
            let arg_reg = arg_start + i as u8;
            self.register_pool[arg_reg as usize] = false;
        }

        Ok(())
    }

    pub(super) fn compile_typed_call_generic(
        &mut self,
        name: &str,
        args: &[aelys_sema::TypedExpr],
        dest: u8,
        span: Span,
    ) -> Result<()> {
        let nargs = args.len();
        let callee_reg = self.alloc_consecutive_registers_for_call(nargs as u8 + 1, span)?;

        for i in 0..=nargs {
            let reg = callee_reg + i as u8;
            self.register_pool[reg as usize] = true;
            if reg >= self.next_register {
                self.next_register = reg + 1;
            }
        }

        self.compile_identifier(name, callee_reg, span)?;

        for (i, arg) in args.iter().enumerate() {
            let arg_reg = callee_reg + 1 + i as u8;
            self.compile_typed_expr(arg, arg_reg)?;
        }

        self.emit_c(OpCode::Call, dest, callee_reg, args.len() as u8, span);

        for i in (0..=nargs).rev() {
            let reg = callee_reg + i as u8;
            self.register_pool[reg as usize] = false;
        }

        Ok(())
    }
}
