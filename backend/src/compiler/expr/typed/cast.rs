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
        let target_kind =
            crate::opcode_select::sized_kind(&aelys_sema::ResolvedType::from_infer_type(target));
        if self.compile_char_cast(
            inner,
            target,
            target_kind.map(CastTarget::sized),
            dest,
            span,
        )? {
            return Ok(());
        }
        let source_kind =
            crate::opcode_select::sized_kind(&aelys_sema::ResolvedType::from_infer_type(&inner.ty));
        let target = if let Some(kind) = target_kind {
            CastTarget::sized(kind)
        } else if target.is_integer() {
            CastTarget::Int
        } else if target.is_float() {
            CastTarget::Float
        } else if *target == InferType::Bool {
            CastTarget::Bool
        } else {
            return self.compile_typed_expr(inner, dest);
        };

        let identity = match (target.sized_kind(), source_kind) {
            (Some(to), Some(from)) => {
                to == from || (to.contains(from.min()) && to.contains(from.max()))
            }
            (Some(_), None) => false,
            (None, Some(aelys_bytecode::SizedInt::U64)) => false,
            (None, _) => {
                (inner.ty.is_integer() && target == CastTarget::Int)
                    || (inner.ty.is_float() && target == CastTarget::Float)
                    || (inner.ty == InferType::Bool && target == CastTarget::Bool)
            }
        };
        if identity {
            return self.compile_typed_expr(inner, dest);
        }

        let source = self.alloc_register()?;
        self.compile_typed_expr(inner, source)?;
        self.emit_a(OpCode::Cast, dest, source, u16::from(target as u8), span);
        self.free_register(source);
        Ok(())
    }
}
