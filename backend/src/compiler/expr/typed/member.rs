use super::super::Compiler;
use aelys_common::Result;
use aelys_syntax::MemberSeparator;
use aelys_syntax::Span;

impl Compiler {
    pub(super) fn compile_typed_identifier(
        &mut self,
        name: &str,
        dest: u16,
        span: Span,
    ) -> aelys_common::Result<()> {
        if name == "None" || name == "Option::None" {
            return self.compile_literal_none(dest, span);
        }
        self.compile_identifier(name, dest, span)
    }

    pub(super) fn typed_path_name(expr: &aelys_sema::TypedExpr) -> Option<String> {
        use aelys_sema::TypedExprKind;

        match &expr.kind {
            TypedExprKind::Identifier(name) => Some(name.clone()),
            TypedExprKind::Member {
                object,
                member,
                separator: MemberSeparator::Path,
            } => {
                let mut path = Self::typed_path_name(object)?;
                path.push_str("::");
                path.push_str(member);
                Some(path)
            }
            _ => None,
        }
    }

    pub(super) fn compile_typed_member_access(
        &mut self,
        object: &aelys_sema::TypedExpr,
        member: &str,
        separator: MemberSeparator,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        if separator == MemberSeparator::Path
            && let Some(module_name) = Self::typed_path_name(object)
            && module_name == "Option"
            && member == "None"
        {
            return self.compile_literal_none(dest, span);
        }

        if separator == MemberSeparator::Path
            && let Some(module_name) = Self::typed_path_name(object)
            && self.has_module_alias(&module_name)
        {
            let global_name = format!("{}::{}", module_name, member);
            let idx = self.get_or_create_global_index(&global_name);
            self.accessed_globals.insert(global_name.clone());
            self.emit_get_global_index(dest, idx, span);
            return Ok(());
        }

        if separator == MemberSeparator::Dot
            && let Some(module_name) = Self::typed_path_name(object)
            && self.has_module_alias(&module_name)
        {
            return Err(aelys_common::error::AelysError::Compile(
                aelys_common::error::CompileError::new(
                    aelys_common::error::CompileErrorKind::ModulePathSeparator {
                        module: module_name.clone(),
                        member: member.to_string(),
                    },
                    span,
                    self.source.clone(),
                ),
            ));
        }

        self.compile_identifier(member, dest, span)
    }
}
