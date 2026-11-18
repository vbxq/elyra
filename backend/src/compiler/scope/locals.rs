use super::super::{Compiler, Local};
use aelys_common::Result;
use aelys_common::error::{CompileError, CompileErrorKind};
use aelys_syntax::Span;

impl Compiler {
    pub fn declare_variable(&mut self, name: &str, mutable: bool) -> Result<u16> {
        for local in self.locals.iter().rev() {
            if local.depth < self.scope_depth {
                break;
            }
            if local.name == name {
                return Err(CompileError::new(
                    CompileErrorKind::VariableAlreadyDefined(name.to_string()),
                    Span::dummy(),
                    self.source.clone(),
                )
                .into());
            }
        }

        let register = self.alloc_register()?;

        self.locals.push(Local {
            name: name.to_string(),
            depth: self.scope_depth,
            mutable,
            register,
            is_captured: false,
            resolved_type: aelys_sema::ResolvedType::Dynamic,
            is_freed: false,
        });

        Ok(register)
    }

    pub fn mark_local_captured(&mut self, register: u16) {
        let owner = self
            .locals
            .iter()
            .position(|local| local.register == register && !local.is_freed)
            .or_else(|| {
                self.locals
                    .iter()
                    .position(|local| local.register == register)
            });
        if let Some(index) = owner {
            self.locals[index].is_captured = true;
        }

        // only the scope that declares the local closes it; an outer local stays shared
        let scope = match owner {
            Some(index) => self
                .scopes
                .iter_mut()
                .rev()
                .find(|scope| scope.start <= index),
            None => self.scopes.last_mut(),
        };
        if let Some(scope) = scope
            && !scope.captured_registers.contains(&register)
        {
            scope.captured_registers.push(register);
        }
    }
}
