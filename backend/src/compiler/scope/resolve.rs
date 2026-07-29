use super::super::{Compiler, Upvalue};

// TODO: this clone dance for enclosing_locals is ugly, maybe Rc
impl Compiler {
    pub fn resolve_variable_typed(
        &self,
        name: &str,
    ) -> Option<(u16, bool, &aelys_sema::ResolvedType)> {
        for local in self.locals.iter().rev() {
            if local.is_freed {
                continue;
            }
            if local.name == name {
                return Some((local.register, local.mutable, &local.resolved_type));
            }
        }
        None
    }

    pub fn resolve_variable(&self, name: &str) -> Option<(u16, bool)> {
        for local in self.locals.iter().rev() {
            if local.is_freed {
                continue;
            }
            if local.name == name {
                return Some((local.register, local.mutable));
            }
        }
        None
    }

    pub fn resolve_upvalue(&mut self, name: &str) -> Option<(u8, bool)> {
        for (i, upvalue) in self.upvalues.iter().enumerate() {
            if upvalue.name == name {
                return Some((u8::try_from(i).ok()?, upvalue.mutable));
            }
        }

        if let Some(ref mut enclosing_locals) = self.enclosing_locals.clone() {
            for (i, local) in enclosing_locals.iter().enumerate() {
                if local.name == name {
                    if let Some(ref mut locals) = self.enclosing_locals {
                        locals[i].is_captured = true;
                    }

                    let upvalue_index = u8::try_from(self.upvalues.len()).unwrap_or(u8::MAX);
                    self.upvalues.push(Upvalue {
                        is_local: true,
                        index: local.register,
                        name: name.to_string(),
                        mutable: local.mutable,
                    });
                    return Some((upvalue_index, local.mutable));
                }
            }
        }

        if let Some(ref enclosing_upvalues) = self.enclosing_upvalues.clone() {
            for (i, upvalue) in enclosing_upvalues.iter().enumerate() {
                if upvalue.name == name {
                    let upvalue_index = u8::try_from(self.upvalues.len()).unwrap_or(u8::MAX);
                    self.upvalues.push(Upvalue {
                        is_local: false,
                        index: u16::try_from(i).ok()?,
                        name: name.to_string(),
                        mutable: upvalue.mutable,
                    });
                    return Some((upvalue_index, upvalue.mutable));
                }
            }
        }

        for (depth, ancestor_locals) in self.all_enclosing_locals.iter().enumerate().skip(1) {
            for local in ancestor_locals.iter() {
                if local.name == name {
                    let upvalue_index = u8::try_from(self.upvalues.len()).unwrap_or(u8::MAX);
                    self.upvalues.push(Upvalue {
                        is_local: false,
                        index: u16::try_from(depth - 1).ok()? | 0x80,
                        name: name.to_string(),
                        mutable: local.mutable,
                    });
                    return Some((upvalue_index, local.mutable));
                }
            }
        }

        None
    }
}
