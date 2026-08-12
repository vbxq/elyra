use super::super::Compiler;
use aelys_bytecode::{OpCode, SumTag};
use aelys_common::Result;
use aelys_sema::TypedExprKind;
use aelys_syntax::MemberSeparator;
use aelys_syntax::Span;

impl Compiler {
    pub(super) fn compile_typed_sum_constructor(
        &mut self,
        callee: &aelys_sema::TypedExpr,
        args: &[aelys_sema::TypedExpr],
        dest: u16,
        span: Span,
    ) -> Result<bool> {
        let tag = match &callee.kind {
            TypedExprKind::Identifier(name) => match name.as_str() {
                "Some" if args.len() == 1 => Some(SumTag::OptionSome),
                "Ok" if args.len() == 1 => Some(SumTag::ResultOk),
                "Err" if args.len() == 1 => Some(SumTag::ResultErr),
                _ => None,
            },
            TypedExprKind::Member {
                object,
                member,
                separator: MemberSeparator::Path,
            } => match (&object.kind, member.as_str(), args.len()) {
                (TypedExprKind::Identifier(name), "Some", 1) if name == "Option" => {
                    Some(SumTag::OptionSome)
                }
                (TypedExprKind::Identifier(name), "Ok", 1) if name == "Result" => {
                    Some(SumTag::ResultOk)
                }
                (TypedExprKind::Identifier(name), "Err", 1) if name == "Result" => {
                    Some(SumTag::ResultErr)
                }
                (TypedExprKind::Identifier(name), "Message", 1) if name == "Error" => {
                    Some(SumTag::ErrorMessage)
                }
                _ => None,
            },
            _ => None,
        };

        let Some(tag) = tag else {
            return Ok(false);
        };

        let payload = self.alloc_register()?;
        self.compile_typed_expr(&args[0], payload)?;
        self.emit_a(OpCode::MakeSum, dest, payload, tag as u8, span);
        self.free_register(payload);
        Ok(true)
    }
}
