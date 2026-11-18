use super::super::super::ConstantFolder;
use aelys_bytecode::{IntOp, checked_int_op, wrapping_sized_op};
use aelys_sema::{InferType, TypedExpr, TypedExprKind};
use aelys_syntax::BinaryOp;

fn int_op(op: BinaryOp) -> Option<IntOp> {
    Some(match op {
        BinaryOp::Add => IntOp::Add,
        BinaryOp::Sub => IntOp::Sub,
        BinaryOp::Mul => IntOp::Mul,
        BinaryOp::Div => IntOp::Div,
        BinaryOp::Mod => IntOp::Rem,
        BinaryOp::Shl => IntOp::Shl,
        BinaryOp::Shr => IntOp::Shr,
        BinaryOp::BitAnd => IntOp::And,
        BinaryOp::BitOr => IntOp::Or,
        BinaryOp::BitXor => IntOp::Xor,
        _ => return None,
    })
}

impl ConstantFolder {
    pub(super) fn fold_int_binary(
        &mut self,
        a: i64,
        op: BinaryOp,
        b: i64,
        operand_ty: &InferType,
        original: &TypedExpr,
    ) -> Option<TypedExpr> {
        // helper to emit a bool result
        let bool_result = |this: &mut Self, v: bool| {
            this.stats.constants_folded += 1;
            Some(TypedExpr::new(
                TypedExprKind::Bool(v),
                InferType::Bool,
                original.span,
            ))
        };
        // helper to emit an int result
        let result_ty = if original.ty.is_integer() {
            original.ty.clone()
        } else {
            InferType::I64
        };
        let int_result = |this: &mut Self, v: i64| {
            this.stats.constants_folded += 1;
            Some(TypedExpr::new(
                TypedExprKind::Int(v),
                result_ty.clone(),
                original.span,
            ))
        };

        let (exact_a, exact_b) = (operand_ty.exact_int(a), operand_ty.exact_int(b));
        match op {
            BinaryOp::Lt => return bool_result(self, exact_a < exact_b),
            BinaryOp::Le => return bool_result(self, exact_a <= exact_b),
            BinaryOp::Gt => return bool_result(self, exact_a > exact_b),
            BinaryOp::Ge => return bool_result(self, exact_a >= exact_b),
            BinaryOp::Eq => return bool_result(self, exact_a == exact_b),
            BinaryOp::Ne => return bool_result(self, exact_a != exact_b),
            _ => {}
        }

        let op = int_op(op)?;
        // a trapping fold is left for the run, which raises the error at its own point
        let result = match original.ty.sized_int() {
            Some(kind) => wrapping_sized_op(op, kind, a, b).ok()?,
            None => checked_int_op(op, a, b).ok()?,
        };
        int_result(self, result)
    }
}
