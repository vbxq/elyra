use super::TypeInference;
use crate::constraint::{ConstraintReason, TypeError, TypeErrorKind};
use aelys_syntax::{BinaryOp, Expr, ExprKind, MemberSeparator, Stmt, StmtKind, UnaryOp};

const INT_MIN: i64 = -(1 << 47);
const INT_MAX: i64 = (1 << 47) - 1;

#[derive(Debug, Clone, PartialEq)]
pub enum ConstValue {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
}

fn int(value: Option<i64>) -> Option<ConstValue> {
    value
        .filter(|value| (INT_MIN..=INT_MAX).contains(value))
        .map(ConstValue::Int)
}

pub(crate) fn fold_const(
    expr: &Expr,
    leaf: &dyn Fn(&Expr) -> Option<ConstValue>,
) -> Option<ConstValue> {
    let fold = |expr: &Expr| fold_const(expr, leaf);
    match &expr.kind {
        ExprKind::Int(value) => int(Some(*value)),
        ExprKind::Float(value) => Some(ConstValue::Float(*value)),
        ExprKind::Bool(value) => Some(ConstValue::Bool(*value)),
        ExprKind::String(value) => Some(ConstValue::Str(value.clone())),
        ExprKind::Grouping(inner) => fold(inner),
        ExprKind::Unary { op, operand } => match (op, fold(operand)?) {
            (UnaryOp::Neg, ConstValue::Int(value)) => int(value.checked_neg()),
            (UnaryOp::Neg, ConstValue::Float(value)) => Some(ConstValue::Float(-value)),
            (UnaryOp::BitNot, ConstValue::Int(value)) => int(Some(!value)),
            (UnaryOp::Not, ConstValue::Bool(value)) => Some(ConstValue::Bool(!value)),
            _ => None,
        },
        ExprKind::And { left, right } => match (fold(left)?, fold(right)?) {
            (ConstValue::Bool(left), ConstValue::Bool(right)) => {
                Some(ConstValue::Bool(left && right))
            }
            _ => None,
        },
        ExprKind::Or { left, right } => match (fold(left)?, fold(right)?) {
            (ConstValue::Bool(left), ConstValue::Bool(right)) => {
                Some(ConstValue::Bool(left || right))
            }
            _ => None,
        },
        ExprKind::Binary { left, op, right } => fold_binary(fold(left)?, *op, fold(right)?),
        ExprKind::Cast { expr, target } => match (fold(expr)?, target.name.as_str()) {
            (ConstValue::Int(value), "int") => Some(ConstValue::Int(value)),
            (ConstValue::Int(value), "float") => Some(ConstValue::Float(value as f64)),
            (ConstValue::Float(value), "float") => Some(ConstValue::Float(value)),
            (ConstValue::Float(value), "int") if value.is_finite() => {
                let truncated = value.trunc();
                (truncated >= INT_MIN as f64 && truncated <= INT_MAX as f64)
                    .then_some(ConstValue::Int(truncated as i64))
            }
            _ => None,
        },
        _ => leaf(expr),
    }
}

fn fold_binary(left: ConstValue, op: BinaryOp, right: ConstValue) -> Option<ConstValue> {
    use ConstValue::{Bool, Float, Int, Str};
    match (left, right) {
        (Int(left), Int(right)) => match op {
            BinaryOp::Add => int(left.checked_add(right)),
            BinaryOp::Sub => int(left.checked_sub(right)),
            BinaryOp::Mul => int(left.checked_mul(right)),
            BinaryOp::Div => int(left.checked_div(right)),
            BinaryOp::Mod => int(left.checked_rem(right)),
            BinaryOp::Shl => int(u32::try_from(right)
                .ok()
                .filter(|shift| *shift < 48)
                .and_then(|shift| left.checked_mul(1i64 << shift))),
            BinaryOp::Shr => int(u32::try_from(right).ok().map(|shift| left >> shift.min(63))),
            BinaryOp::BitAnd => int(Some(left & right)),
            BinaryOp::BitOr => int(Some(left | right)),
            BinaryOp::BitXor => int(Some(left ^ right)),
            BinaryOp::Eq => Some(Bool(left == right)),
            BinaryOp::Ne => Some(Bool(left != right)),
            BinaryOp::Lt => Some(Bool(left < right)),
            BinaryOp::Le => Some(Bool(left <= right)),
            BinaryOp::Gt => Some(Bool(left > right)),
            BinaryOp::Ge => Some(Bool(left >= right)),
        },
        (Float(left), Float(right)) => match op {
            BinaryOp::Add => Some(Float(left + right)),
            BinaryOp::Sub => Some(Float(left - right)),
            BinaryOp::Mul => Some(Float(left * right)),
            BinaryOp::Div => Some(Float(left / right)),
            BinaryOp::Mod => Some(Float(left % right)),
            BinaryOp::Eq => Some(Bool(left == right)),
            BinaryOp::Ne => Some(Bool(left != right)),
            BinaryOp::Lt => Some(Bool(left < right)),
            BinaryOp::Le => Some(Bool(left <= right)),
            BinaryOp::Gt => Some(Bool(left > right)),
            BinaryOp::Ge => Some(Bool(left >= right)),
            _ => None,
        },
        (Bool(left), Bool(right)) => match op {
            BinaryOp::Eq => Some(Bool(left == right)),
            BinaryOp::Ne => Some(Bool(left != right)),
            _ => None,
        },
        (Str(left), Str(right)) => match op {
            BinaryOp::Add => Some(Str(left + &right)),
            BinaryOp::Eq => Some(Bool(left == right)),
            BinaryOp::Ne => Some(Bool(left != right)),
            _ => None,
        },
        _ => None,
    }
}

fn path_name(expr: &Expr) -> Option<String> {
    match &expr.kind {
        ExprKind::Identifier(name) => Some(name.clone()),
        ExprKind::Member {
            object,
            member,
            separator: MemberSeparator::Path,
        } => Some(format!("{}::{member}", path_name(object)?)),
        _ => None,
    }
}

impl TypeInference {
    pub(crate) fn constant_leaf(&self, expr: &Expr) -> Option<ConstValue> {
        if let ExprKind::Identifier(name) = &expr.kind {
            if let Some(value) = self.env.lookup_constant(name) {
                return Some(value);
            }
            if self.env.binds_value(name) {
                return None;
            }
            return self
                .global_constants
                .get(name)
                .or_else(|| self.imported_constants.get(name))
                .cloned();
        }
        if let Some(path) = path_name(expr)
            && let Some(value) = self.imported_constants.get(&path)
        {
            return Some(value.clone());
        }
        self.constant_path_int(expr).map(ConstValue::Int)
    }

    pub(crate) fn constant_int_leaf(&self, expr: &Expr) -> Option<i64> {
        match self.constant_leaf(expr)? {
            ConstValue::Int(value) => Some(value),
            _ => None,
        }
    }

    pub(crate) fn fold_constant(&self, expr: &Expr) -> Option<ConstValue> {
        fold_const(expr, &|leaf| self.constant_leaf(leaf))
    }

    pub(crate) fn declare_constant(
        &mut self,
        name: &str,
        initializer: &Expr,
    ) -> Option<ConstValue> {
        match self.fold_constant(initializer) {
            Some(value) => {
                if name != "_" {
                    self.env.define_constant(name, value.clone());
                }
                Some(value)
            }
            None => {
                self.errors.push(TypeError {
                    kind: TypeErrorKind::NonConstantInitializer {
                        name: name.to_string(),
                    },
                    span: initializer.span,
                    reason: ConstraintReason::Other("constant initializer".to_string()),
                });
                None
            }
        }
    }

    pub(crate) fn predeclare_constants(&mut self, stmts: &[Stmt], bind: bool) {
        let mut pending: Vec<(&str, &Expr)> = stmts
            .iter()
            .filter_map(|stmt| match &stmt.kind {
                StmtKind::Let {
                    name,
                    initializer,
                    is_const: true,
                    ..
                } => Some((name.as_str(), initializer)),
                _ => None,
            })
            .collect();
        loop {
            let before = pending.len();
            pending.retain(
                |(name, initializer)| match self.fold_constant(initializer) {
                    Some(value) => {
                        if bind {
                            self.env.define_constant(name, value.clone());
                            self.exported_constants
                                .insert(name.to_string(), value.clone());
                        }
                        self.global_constants.insert(name.to_string(), value);
                        false
                    }
                    None => true,
                },
            );
            if pending.is_empty() || pending.len() == before {
                break;
            }
        }
    }
}

impl ConstValue {
    pub(crate) fn infer_type(&self) -> crate::types::InferType {
        use crate::types::InferType;
        match self {
            ConstValue::Int(_) => InferType::I64,
            ConstValue::Float(_) => InferType::F64,
            ConstValue::Bool(_) => InferType::Bool,
            ConstValue::Str(_) => InferType::String,
        }
    }

    pub(crate) fn literal(&self) -> crate::typed_ast::TypedExprKind {
        use crate::typed_ast::TypedExprKind;
        match self {
            ConstValue::Int(value) => TypedExprKind::Int(*value),
            ConstValue::Float(value) => TypedExprKind::Float(*value),
            ConstValue::Bool(value) => TypedExprKind::Bool(*value),
            ConstValue::Str(value) => TypedExprKind::String(value.clone()),
        }
    }
}
