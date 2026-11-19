use super::super::{Compiler, Scope};
use aelys_bytecode::{OpCode, Register};

impl Compiler {
    pub fn begin_scope(&mut self) {
        self.scope_depth += 1;
        self.scopes.push(Scope {
            start: self.locals.len(),
            captured_registers: Vec::new(),
        });
    }

    pub fn close_scope_upvalues(&mut self) {
        if let Some(&lowest_captured) = self
            .scopes
            .last()
            .and_then(|scope| scope.captured_registers.iter().min())
        {
            self.current.emit_register_abc(
                OpCode::CloseUpvals,
                Register::new(lowest_captured),
                Register::new(0),
                Register::new(0),
                0,
            );
        }
    }

    // a closure compiled later in an enclosing loop body may already hold an open upvalue here
    pub(crate) fn close_upvalues_before_return(&mut self, span: aelys_syntax::Span) {
        let loops = &self.loop_stack;
        let lowest = self
            .locals
            .iter()
            .filter(|local| {
                local.is_captured
                    || loops
                        .iter()
                        .any(|context| context.captured_names.contains(&local.name))
            })
            .map(|local| local.register)
            .min();
        if let Some(register) = lowest {
            self.emit_a(OpCode::CloseUpvals, register, 0, 0, span);
        }
    }

    pub(crate) fn end_function_scope(&mut self) {
        self.scope_depth = self.scope_depth.saturating_sub(1);
        if let Some(scope) = self.scopes.pop() {
            let released: Vec<_> = self
                .locals
                .drain(scope.start..)
                .map(|local| local.register)
                .collect();
            for register in released {
                self.free_register(register);
            }
        }
    }

    pub fn end_scope(&mut self) {
        self.scope_depth = self.scope_depth.saturating_sub(1);

        if let Some(scope) = self.scopes.pop() {
            if let Some(&lowest_captured) = scope.captured_registers.iter().min() {
                self.current.emit_register_abc(
                    OpCode::CloseUpvals,
                    Register::new(lowest_captured),
                    Register::new(0),
                    Register::new(0),
                    0,
                );
            }

            let released: Vec<_> = self
                .locals
                .drain(scope.start..)
                .map(|local| local.register)
                .collect();
            for register in released {
                self.free_register(register);
            }
        }
    }
}
