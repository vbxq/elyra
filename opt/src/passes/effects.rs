use aelys_sema::{
    InferType, TypedExpr, TypedExprKind, TypedFmtStringPart, TypedMatchArm, TypedMatchArmBody,
    TypedPatternKind, TypedStmtKind,
};
use aelys_syntax::{BinaryOp, UnaryOp};

const INT_MIN: i64 = -(1i64 << 47);
const INT_MAX: i64 = (1i64 << 47) - 1;

fn in_vm_range(value: i64) -> Option<i64> {
    (INT_MIN..=INT_MAX).contains(&value).then_some(value)
}

// an expression may be deleted, duplicated or reordered only when it can neither write nor trap
pub(crate) fn has_effects(expr: &TypedExpr) -> bool {
    match &expr.kind {
        TypedExprKind::Int(_)
        | TypedExprKind::Float(_)
        | TypedExprKind::Bool(_)
        | TypedExprKind::Char(_)
        | TypedExprKind::String(_)
        | TypedExprKind::Unit
        | TypedExprKind::Null
        | TypedExprKind::AssociatedConst { .. }
        | TypedExprKind::Identifier(_)
        | TypedExprKind::Lambda(_)
        | TypedExprKind::LambdaInner { .. } => false,

        TypedExprKind::Call { .. }
        | TypedExprKind::Assign { .. }
        | TypedExprKind::IndexAssign { .. }
        | TypedExprKind::MemberAssign { .. }
        | TypedExprKind::Try { .. }
        | TypedExprKind::Member { .. }
        | TypedExprKind::Slice { .. }
        | TypedExprKind::Loop { .. } => true,

        TypedExprKind::Block(stmts) => stmts.iter().any(|stmt| match &stmt.kind {
            TypedStmtKind::Expression(e) => has_effects(e),
            _ => true,
        }),

        TypedExprKind::Grouping(inner) => has_effects(inner),
        TypedExprKind::And { left, right } | TypedExprKind::Or { left, right } => {
            has_effects(left) || has_effects(right)
        }
        TypedExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => has_effects(condition) || has_effects(then_branch) || has_effects(else_branch),

        TypedExprKind::Binary { left, op, right } => {
            has_effects(left) || has_effects(right) || binary_can_trap(expr, left, *op, right)
        }
        TypedExprKind::Unary { op, operand } => {
            has_effects(operand)
                || match op {
                    UnaryOp::Not => !matches!(operand.ty, InferType::Bool),
                    UnaryOp::BitNot => !operand.ty.is_integer(),
                    UnaryOp::Neg => !operand.ty.is_float() && const_int(expr).is_none(),
                }
        }

        TypedExprKind::StructField { object, .. } | TypedExprKind::StructMethod { object, .. } => {
            has_effects(object)
        }
        TypedExprKind::Index { object, index } => {
            has_effects(object) || has_effects(index) || !index_in_bounds(object, index)
        }
        TypedExprKind::Range { start, end, .. } => {
            start.as_ref().is_some_and(|s| has_effects(s))
                || end.as_ref().is_some_and(|e| has_effects(e))
        }
        TypedExprKind::ArrayLiteral {
            elements, repeat, ..
        }
        | TypedExprKind::VecLiteral {
            elements, repeat, ..
        } => {
            elements.iter().any(has_effects)
                || repeat.as_ref().is_some_and(|count| !is_small_count(count))
        }
        TypedExprKind::ArraySized { size, .. } => !is_small_count(size),
        TypedExprKind::FmtString(parts) => parts.iter().any(|part| match part {
            TypedFmtStringPart::Expr(e) => has_effects(e) || !is_plain_display(&e.ty),
            _ => false,
        }),
        TypedExprKind::StructLiteral { fields, .. } => fields.iter().any(|(_, v)| has_effects(v)),
        TypedExprKind::EnumConstruct { fields, .. } => fields.iter().any(|(_, v)| has_effects(v)),
        TypedExprKind::Cast {
            expr: inner,
            target,
        } => has_effects(inner) || !cast_cannot_trap(&inner.ty, target),
        TypedExprKind::Match { scrutinee, arms } => {
            has_effects(scrutinee) || !ends_irrefutably(arms) || arms.iter().any(arm_has_effects)
        }
    }
}

fn ends_irrefutably(arms: &[TypedMatchArm]) -> bool {
    arms.last().is_some_and(|arm| {
        arm.guard.is_none()
            && matches!(
                arm.pattern.kind,
                TypedPatternKind::Binding(_) | TypedPatternKind::Wildcard
            )
    })
}

fn arm_has_effects(arm: &TypedMatchArm) -> bool {
    if arm.guard.as_ref().is_some_and(has_effects) {
        return true;
    }
    match &arm.body {
        TypedMatchArmBody::Expr(e) => has_effects(e),
        TypedMatchArmBody::Block(stmts) => stmts.iter().any(|stmt| match &stmt.kind {
            TypedStmtKind::Expression(e) => has_effects(e),
            _ => true,
        }),
    }
}

fn binary_can_trap(whole: &TypedExpr, left: &TypedExpr, op: BinaryOp, right: &TypedExpr) -> bool {
    let (lt, rt) = (&left.ty, &right.ty);
    let ints = lt.is_integer() && rt.is_integer();
    let numbers = (lt.is_integer() || lt.is_float()) && (rt.is_integer() || rt.is_float());
    let floats = numbers && (lt.is_float() || rt.is_float());
    match op {
        BinaryOp::Eq | BinaryOp::Ne | BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
            !(numbers
                || (matches!(lt, InferType::Bool) && matches!(rt, InferType::Bool))
                || (matches!(lt, InferType::String) && matches!(rt, InferType::String)))
        }
        BinaryOp::Shl | BinaryOp::Shr | BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor => {
            !ints
        }
        BinaryOp::Add => {
            let strings = matches!(lt, InferType::String) && matches!(rt, InferType::String);
            !(floats || strings || (ints && const_int(whole).is_some()))
        }
        BinaryOp::Sub | BinaryOp::Mul => !(floats || (ints && const_int(whole).is_some())),
        BinaryOp::Div | BinaryOp::Mod => {
            if floats {
                return false;
            }
            if !ints {
                return true;
            }
            match const_int(right) {
                Some(0) | Some(-1) | None => const_int(whole).is_none(),
                Some(_) => false,
            }
        }
    }
}

pub(crate) fn const_int(expr: &TypedExpr) -> Option<i64> {
    match &expr.kind {
        TypedExprKind::Int(n) => in_vm_range(*n),
        TypedExprKind::Grouping(inner) => const_int(inner),
        TypedExprKind::Unary {
            op: UnaryOp::Neg,
            operand,
        } => in_vm_range(const_int(operand)?.checked_neg()?),
        TypedExprKind::Binary { left, op, right }
            if left.ty.is_integer() && right.ty.is_integer() =>
        {
            let a = const_int(left)?;
            let b = const_int(right)?;
            let value = match op {
                BinaryOp::Add => a.checked_add(b)?,
                BinaryOp::Sub => a.checked_sub(b)?,
                BinaryOp::Mul => a.checked_mul(b)?,
                BinaryOp::Div if b != 0 => a.checked_div(b)?,
                BinaryOp::Mod if b != 0 => a.checked_rem(b)?,
                BinaryOp::BitAnd => a & b,
                BinaryOp::BitOr => a | b,
                BinaryOp::BitXor => a ^ b,
                _ => return None,
            };
            in_vm_range(value)
        }
        _ => None,
    }
}

fn index_in_bounds(object: &TypedExpr, index: &TypedExpr) -> bool {
    let Some(i) = const_int(index) else {
        return false;
    };
    let length = match (&object.kind, &object.ty) {
        (_, InferType::FixedArray(_, n)) => *n,
        (
            TypedExprKind::ArrayLiteral {
                elements,
                repeat: None,
                ..
            },
            _,
        )
        | (
            TypedExprKind::VecLiteral {
                elements,
                repeat: None,
                ..
            },
            _,
        ) => elements.len(),
        _ => return false,
    };
    usize::try_from(i).is_ok_and(|i| i < length)
}

fn is_small_count(count: &TypedExpr) -> bool {
    const_int(count).is_some_and(|n| (0..=4096).contains(&n))
}

fn is_plain_display(ty: &InferType) -> bool {
    ty.is_integer() || ty.is_float() || matches!(ty, InferType::Bool | InferType::String)
}

fn cast_cannot_trap(from: &InferType, to: &InferType) -> bool {
    from == to
        || (from.is_integer() && (to.is_integer() || to.is_float()))
        || (from.is_float() && to.is_float())
}
