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

    // End scope: close upvalues for captured variables, free registers.
    // Note: we only emit CloseUpvals if something was actually captured.
    // Earlier versions emitted it unconditionally which was wasteful
    pub fn end_scope(&mut self) {
        self.scope_depth = self.scope_depth.saturating_sub(1);

        if let Some(scope) = self.scopes.pop() {
            // Only need to close upvalues if any locals were captured
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
