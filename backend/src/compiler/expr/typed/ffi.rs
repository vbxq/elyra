use super::super::Compiler;
use super::containers::Operand;
use aelys_common::Result;
use aelys_sema::{InferType, TypedExpr, TypedExprKind, TypedExternFunction};
use aelys_syntax::{MemberSeparator, Span};

fn text(value: &str, span: Span) -> TypedExpr {
    TypedExpr::new(
        TypedExprKind::String(value.to_string()),
        InferType::String,
        span,
    )
}

impl Compiler {
    pub(crate) fn compile_typed_extern(
        &mut self,
        functions: &[TypedExternFunction],
        span: Span,
    ) -> Result<()> {
        let origin = self.source.name.clone();
        for function in functions {
            let operands = [
                text(&function.library, span),
                text(&function.symbol, span),
                text(&function.signature.encode(), span),
                text(&origin, span),
            ];
            let operands: Vec<Operand<'_>> = operands.iter().map(Operand::Expr).collect();
            let register = self.alloc_register()?;
            self.call_native("__ffi_bind", &operands, register, span)?;
            let index = self.get_or_create_global_index(&function.name);
            self.accessed_globals.insert(function.name.clone());
            self.emit_set_global_index(register, index, span);
            self.free_register(register);
        }
        Ok(())
    }

    pub(super) fn compile_ptr_call(
        &mut self,
        object: &TypedExpr,
        member: &str,
        separator: MemberSeparator,
        args: &[TypedExpr],
        dest: u16,
        span: Span,
    ) -> Result<bool> {
        if object.ty != InferType::Ptr {
            return Ok(false);
        }
        let mut operands = Vec::with_capacity(args.len() + 1);
        match separator {
            MemberSeparator::Path if matches!(object.kind, TypedExprKind::Unit) => {}
            MemberSeparator::Dot => operands.push(Operand::Expr(object)),
            MemberSeparator::Path => return Ok(false),
        }
        operands.extend(args.iter().map(Operand::Expr));
        self.call_native(&format!("__ptr_{member}"), &operands, dest, span)?;
        Ok(true)
    }
}
