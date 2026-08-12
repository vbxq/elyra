use super::super::Compiler;
use aelys_bytecode::{CastTarget, OpCode};
use aelys_common::Result;
use aelys_sema::{InferType, TypedExpr};
use aelys_syntax::Span;

impl Compiler {
    pub(super) fn compile_typed_cast(
        &mut self,
        inner: &TypedExpr,
        target: &InferType,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let target = if target.is_integer() {
            CastTarget::Int
        } else if target.is_float() {
            CastTarget::Float
        } else if *target == InferType::Bool {
            CastTarget::Bool
        } else {
            return self.compile_typed_expr(inner, dest);
        };

        if (inner.ty.is_integer() && target == CastTarget::Int)
            || (inner.ty.is_float() && target == CastTarget::Float)
            || (inner.ty == InferType::Bool && target == CastTarget::Bool)
        {
            return self.compile_typed_expr(inner, dest);
        }

        let source = self.alloc_register()?;
        self.compile_typed_expr(inner, source)?;
        self.emit_a(OpCode::Cast, dest, source, target as u8, span);
        self.free_register(source);
        Ok(())
    }
}
