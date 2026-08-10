use super::super::Compiler;
use aelys_common::Result;
use aelys_common::error::{CompileError, CompileErrorKind};
use aelys_syntax::MemberSeparator;
use aelys_syntax::Span;
use aelys_syntax::ast::Expr;

impl Compiler {
    pub fn compile_member_access(
        &mut self,
        object: &Expr,
        member: &str,
        separator: MemberSeparator,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        if separator == MemberSeparator::Path
            && let Some(module_name) = Self::path_name(object)
            && self.has_module_alias(&module_name)
        {
            let qualified_name = format!("{}::{}", module_name, member);

            let idx = if let Some(&idx) = self.global_indices.get(&qualified_name) {
                idx
            } else {
                let idx = self.next_global_index;
                self.global_indices.insert(qualified_name.clone(), idx);
                self.next_global_index += 1;
                idx
            };

            self.accessed_globals.insert(qualified_name);
            self.emit_get_global_index(dest, idx, span);
            return Ok(());
        }

        if separator == MemberSeparator::Dot
            && let Some(module_name) = Self::path_name(object)
            && self.has_module_alias(&module_name)
        {
            return Err(CompileError::new(
                CompileErrorKind::ModulePathSeparator {
                    module: module_name.clone(),
                    member: member.to_string(),
                },
                span,
                self.source.clone(),
            )
            .into());
        }

        if Self::is_builtin(member) {
            let idx = if let Some(&idx) = self.global_indices.get(member) {
                idx
            } else {
                let idx = self.next_global_index;
                self.global_indices.insert(member.to_string(), idx);
                self.next_global_index += 1;
                idx
            };
            self.accessed_globals.insert(member.to_string());
            self.emit_get_global_index(dest, idx, span);
            Ok(())
        } else {
            Err(CompileError::new(
                CompileErrorKind::UndefinedVariable(member.to_string()),
                span,
                self.source.clone(),
            )
            .into())
        }
    }
}
