use super::super::Compiler;
use aelys_bytecode::OpCode;
use aelys_common::Result;
use aelys_syntax::Span;
use aelys_syntax::ast::UnaryOp;

impl Compiler {
    pub(super) fn compile_typed_unary(
        &mut self,
        op: UnaryOp,
        operand: &aelys_sema::TypedExpr,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        self.compile_typed_expr(operand, dest)?;

        if let Some(kind) = crate::opcode_select::sized_kind(
            &aelys_sema::ResolvedType::from_infer_type(&operand.ty),
        ) && matches!(op, UnaryOp::Neg | UnaryOp::BitNot)
        {
            let opcode = if op == UnaryOp::Neg {
                OpCode::NegW
            } else {
                OpCode::NotW
            };
            self.emit_a(opcode, dest, dest, u16::from(kind as u8), span);
            return Ok(());
        }

        let opcode = match op {
            UnaryOp::Neg => OpCode::Neg,
            UnaryOp::Not => OpCode::Not,
            UnaryOp::BitNot => OpCode::BitNot,
        };
        self.emit_a(opcode, dest, dest, 0, span);

        Ok(())
    }
}
