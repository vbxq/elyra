use super::super::Compiler;
use aelys_bytecode::OpCode;
use aelys_common::Result;
use aelys_syntax::Span;
use aelys_syntax::ast::Expr;

impl Compiler {
    pub fn compile_call_generic(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let nargs = self.checked_call_arity(args.len(), span)?;
        let register_count = args.len().saturating_add(1);
        let func_reg = self.alloc_consecutive_registers_for_call(register_count, span)?;

        for i in 0..register_count {
            let reg = func_reg + u16::try_from(i).expect("argument count was range checked");
            self.register_pool[reg as usize] = true;
            if u32::from(reg) >= self.next_register {
                self.next_register = u32::from(reg) + 1;
            }
        }

        self.compile_expr(callee, func_reg)?;

        for (i, arg) in args.iter().enumerate() {
            let arg_reg =
                func_reg + 1 + u16::try_from(i).expect("argument count was range checked");
            self.compile_expr(arg, arg_reg)?;
        }

        self.emit_c(OpCode::Call, dest, func_reg, nargs, span);

        for i in (0..register_count).rev() {
            let reg = func_reg + u16::try_from(i).expect("argument count was range checked");
            self.free_register(reg);
        }

        Ok(())
    }
}
