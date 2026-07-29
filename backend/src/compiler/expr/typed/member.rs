use super::super::Compiler;
use aelys_common::Result;
use aelys_syntax::Span;

impl Compiler {
    pub(super) fn compile_typed_member_access(
        &mut self,
        object: &aelys_sema::TypedExpr,
        member: &str,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        use aelys_sema::TypedExprKind;

        if let TypedExprKind::Identifier(module_name) = &object.kind
            && self.module_aliases.contains(module_name)
        {
            let global_name = format!("{}::{}", module_name, member);
            let idx = self.get_or_create_global_index(&global_name);
            self.accessed_globals.insert(global_name.clone());
            self.emit_get_global_index(dest, idx, span);
            return Ok(());
        }

        self.compile_identifier(member, dest, span)
    }
}
