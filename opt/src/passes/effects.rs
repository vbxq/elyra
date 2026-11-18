use aelys_sema::{
    InferType, TypedExpr, TypedExprKind, TypedFmtStringPart, TypedMatchArm, TypedMatchArmBody,
    TypedPatternKind, TypedStmt, TypedStmtKind,
};
use aelys_syntax::{BinaryOp, UnaryOp};
use std::collections::HashSet;

// an expression may be deleted, duplicated or reordered only when it can neither write nor trap
#[derive(Debug, Default, Clone)]
pub(crate) struct Effects {
    // globals a read may find unset: their `let` can run after a function that reads them
    unset_globals: HashSet<String>,
}

impl Effects {
    pub(crate) fn of_program(stmts: &[TypedStmt]) -> Self {
        let mut all = HashSet::new();
        let mut rebound = HashSet::new();
        for stmt in stmts {
            if let TypedStmtKind::Let { name, .. } = &stmt.kind
                && !all.insert(name.clone())
            {
                rebound.insert(name.clone());
            }
        }
        let mut effects = Effects { unset_globals: all };
        let mut code_ran = false;
        for stmt in stmts {
            match &stmt.kind {
                TypedStmtKind::Function(_)
                | TypedStmtKind::ImplDecl { .. }
                | TypedStmtKind::StructDecl { .. }
                | TypedStmtKind::TraitDecl { .. }
                | TypedStmtKind::EnumDecl { .. }
                | TypedStmtKind::Needs(_) => {}
                TypedStmtKind::Let {
                    name, initializer, ..
                } if !code_ran => {
                    if may_run_code(initializer) {
                        code_ran = true;
                    } else if !rebound.contains(name) && !effects.has_effects(initializer) {
                        effects.unset_globals.remove(name);
                    }
                }
                TypedStmtKind::Expression(expr) if !may_run_code(expr) => {}
                _ => code_ran = true,
            }
        }
        effects
    }

    fn reads_unset_global(&self, name: &str) -> bool {
        self.unset_globals.contains(name) || aelys_sema::is_module_scoped_global(name)
    }

    pub(crate) fn has_effects(&self, expr: &TypedExpr) -> bool {
        match &expr.kind {
            TypedExprKind::Int(_)
            | TypedExprKind::Float(_)
            | TypedExprKind::Bool(_)
            | TypedExprKind::Char(_)
            | TypedExprKind::String(_)
            | TypedExprKind::Unit
            | TypedExprKind::Null
            | TypedExprKind::AssociatedConst { .. }
            | TypedExprKind::Lambda(_)
            | TypedExprKind::LambdaInner { .. } => false,

            TypedExprKind::Identifier(name) => self.reads_unset_global(name),

            TypedExprKind::Call { .. }
            | TypedExprKind::Assign { .. }
            | TypedExprKind::IndexAssign { .. }
            | TypedExprKind::MemberAssign { .. }
            | TypedExprKind::Try { .. }
            | TypedExprKind::Member { .. }
            | TypedExprKind::Slice { .. }
            | TypedExprKind::Loop { .. } => true,

            TypedExprKind::Grouping(inner) => self.has_effects(inner),
            TypedExprKind::And { left, right } | TypedExprKind::Or { left, right } => {
                self.has_effects(left) || self.has_effects(right)
            }
            TypedExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.has_effects(condition)
                    || self.has_effects(then_branch)
                    || self.has_effects(else_branch)
            }

            TypedExprKind::Binary { left, op, right } => {
                self.has_effects(left)
                    || self.has_effects(right)
                    || binary_can_trap(expr, left, *op, right)
            }
            TypedExprKind::Unary { op, operand } => {
                self.has_effects(operand)
                    || match op {
                        UnaryOp::Not => !matches!(operand.ty, InferType::Bool),
                        UnaryOp::BitNot => !operand.ty.is_integer(),
                        UnaryOp::Neg => {
                            !(operand.ty.is_float()
                                || operand.ty.is_sized_integer()
                                || const_int(expr).is_some())
                        }
                    }
            }

            TypedExprKind::StructField { object, .. }
            | TypedExprKind::StructMethod { object, .. } => self.has_effects(object),
            TypedExprKind::Index { object, index } => {
                self.has_effects(object)
                    || self.has_effects(index)
                    || !index_in_bounds(object, index)
            }
            TypedExprKind::Range { start, end, .. } => {
                start.as_ref().is_some_and(|s| self.has_effects(s))
                    || end.as_ref().is_some_and(|e| self.has_effects(e))
            }
            TypedExprKind::ArrayLiteral {
                elements, repeat, ..
            }
            | TypedExprKind::VecLiteral {
                elements, repeat, ..
            } => {
                elements.iter().any(|e| self.has_effects(e))
                    || repeat.as_ref().is_some_and(|count| !is_small_count(count))
            }
            TypedExprKind::ArraySized { size, .. } => !is_small_count(size),
            TypedExprKind::FmtString(parts) => parts.iter().any(|part| match part {
                TypedFmtStringPart::Expr(e) => self.has_effects(e) || !is_plain_display(&e.ty),
                _ => false,
            }),
            TypedExprKind::StructLiteral { fields, .. } => {
                fields.iter().any(|(_, v)| self.has_effects(v))
            }
            TypedExprKind::EnumConstruct { fields, .. } => {
                fields.iter().any(|(_, v)| self.has_effects(v))
            }
            TypedExprKind::Cast {
                expr: inner,
                target,
            } => self.has_effects(inner) || !cast_cannot_trap(&inner.ty, target),
            TypedExprKind::Match { scrutinee, arms } => {
                self.has_effects(scrutinee)
                    || !ends_irrefutably(arms)
                    || arms.iter().any(|arm| self.arm_has_effects(arm))
            }
            TypedExprKind::Block(stmts) => stmts.iter().any(|stmt| self.stmt_has_effects(stmt)),
        }
    }

    fn stmt_has_effects(&self, stmt: &TypedStmt) -> bool {
        match &stmt.kind {
            TypedStmtKind::Expression(e) => self.has_effects(e),
            _ => true,
        }
    }

    fn arm_has_effects(&self, arm: &TypedMatchArm) -> bool {
        if arm.guard.as_ref().is_some_and(|g| self.has_effects(g)) {
            return true;
        }
        match &arm.body {
            TypedMatchArmBody::Expr(e) => self.has_effects(e),
            TypedMatchArmBody::Block(stmts) => stmts.iter().any(|stmt| self.stmt_has_effects(stmt)),
        }
    }
}

// whether evaluating the expression can enter user code, which may read a global not yet bound
fn may_run_code(expr: &TypedExpr) -> bool {
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
        TypedExprKind::Grouping(inner) => may_run_code(inner),
        TypedExprKind::And { left, right } | TypedExprKind::Or { left, right } => {
            may_run_code(left) || may_run_code(right)
        }
        TypedExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => may_run_code(condition) || may_run_code(then_branch) || may_run_code(else_branch),
        TypedExprKind::Binary { left, right, .. } => {
            !is_primitive(&left.ty)
                || !is_primitive(&right.ty)
                || may_run_code(left)
                || may_run_code(right)
        }
        TypedExprKind::Unary { operand, .. } => !is_primitive(&operand.ty) || may_run_code(operand),
        TypedExprKind::Cast { expr: inner, .. } => !is_primitive(&inner.ty) || may_run_code(inner),
        TypedExprKind::StructField { object, .. } => may_run_code(object),
        TypedExprKind::Index { object, index } => {
            !matches!(
                object.ty,
                InferType::FixedArray(..) | InferType::Array(_) | InferType::Vec(_)
            ) || may_run_code(object)
                || may_run_code(index)
        }
        TypedExprKind::Range { start, end, .. } => {
            start.as_ref().is_some_and(|s| may_run_code(s))
                || end.as_ref().is_some_and(|e| may_run_code(e))
        }
        TypedExprKind::ArrayLiteral {
            elements, repeat, ..
        }
        | TypedExprKind::VecLiteral {
            elements, repeat, ..
        } => elements.iter().any(may_run_code) || repeat.as_ref().is_some_and(|r| may_run_code(r)),
        TypedExprKind::ArraySized { size, .. } => may_run_code(size),
        TypedExprKind::FmtString(parts) => parts.iter().any(|part| match part {
            TypedFmtStringPart::Expr(e) => !is_plain_display(&e.ty) || may_run_code(e),
            _ => false,
        }),
        TypedExprKind::StructLiteral { fields, .. } => fields.iter().any(|(_, v)| may_run_code(v)),
        TypedExprKind::EnumConstruct { fields, .. } => fields.iter().any(|(_, v)| may_run_code(v)),
        _ => true,
    }
}

fn is_primitive(ty: &InferType) -> bool {
    ty.is_integer() || ty.is_float() || matches!(ty, InferType::Bool | InferType::Char)
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

fn binary_can_trap(whole: &TypedExpr, left: &TypedExpr, op: BinaryOp, right: &TypedExpr) -> bool {
    let (lt, rt) = (&left.ty, &right.ty);
    let ints = lt.is_integer() && rt.is_integer();
    let wraps = ints && lt.is_sized_integer();
    let numbers = (lt.is_integer() || lt.is_float()) && (rt.is_integer() || rt.is_float());
    let floats = numbers && (lt.is_float() || rt.is_float());
    match op {
        BinaryOp::Eq | BinaryOp::Ne | BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
            !(numbers
                || (matches!(lt, InferType::Bool) && matches!(rt, InferType::Bool))
                || (matches!(lt, InferType::Char) && matches!(rt, InferType::Char))
                || (matches!(lt, InferType::String) && matches!(rt, InferType::String)))
        }
        BinaryOp::Shr | BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor => !ints,
        BinaryOp::Shl => !(wraps || (ints && const_int(whole).is_some())),
        BinaryOp::Add => {
            let strings = matches!(lt, InferType::String) && matches!(rt, InferType::String);
            !(floats || strings || wraps || (ints && const_int(whole).is_some()))
        }
        BinaryOp::Sub | BinaryOp::Mul => !(floats || wraps || (ints && const_int(whole).is_some())),
        BinaryOp::Div | BinaryOp::Mod => {
            if floats {
                return false;
            }
            if !ints {
                return true;
            }
            match const_int(right) {
                None | Some(0) => true,
                Some(-1) => !wraps && const_int(whole).is_none(),
                Some(_) => false,
            }
        }
    }
}

pub(crate) fn const_int(expr: &TypedExpr) -> Option<i64> {
    match &expr.kind {
        TypedExprKind::Int(n) => Some(*n),
        TypedExprKind::Grouping(inner) => const_int(inner),
        _ if expr.ty.is_sized_integer() => None,
        TypedExprKind::Unary {
            op: UnaryOp::Neg,
            operand,
        } => const_int(operand)?.checked_neg(),
        TypedExprKind::Binary { left, op, right }
            if left.ty.is_integer() && right.ty.is_integer() && !left.ty.is_sized_integer() =>
        {
            let a = const_int(left)?;
            let b = const_int(right)?;
            let value = match op {
                BinaryOp::Add => a.checked_add(b)?,
                BinaryOp::Sub => a.checked_sub(b)?,
                BinaryOp::Mul => a.checked_mul(b)?,
                BinaryOp::Div if b != 0 => a.checked_div(b)?,
                BinaryOp::Mod if b != 0 => a.checked_rem(b)?,
                BinaryOp::Shl => i64::try_from(i128::from(a) << (b & 63)).ok()?,
                BinaryOp::Shr => a >> (b & 63),
                BinaryOp::BitAnd => a & b,
                BinaryOp::BitOr => a | b,
                BinaryOp::BitXor => a ^ b,
                _ => return None,
            };
            Some(value)
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
    ty.is_integer()
        || ty.is_float()
        || matches!(ty, InferType::Bool | InferType::Char | InferType::String)
}

fn cast_cannot_trap(from: &InferType, to: &InferType) -> bool {
    if from == to {
        return true;
    }
    let from_exact = from.is_integer() || matches!(from, InferType::Bool | InferType::Char);
    from_exact && (to.is_integer() || to.is_float()) || (from.is_float() && to.is_float())
}
