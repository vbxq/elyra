use super::TypeInference;
use crate::constraint::{ConstraintReason, TypeError, TypeErrorKind};
use crate::types::InferType;
use aelys_syntax::{BinaryOp, Expr, ExprKind, MemberSeparator, Stmt, StmtKind, UnaryOp};

#[derive(Debug, Clone, PartialEq)]
pub enum ConstValue {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
}

// a sized result wraps to its bit pattern; an int result outside i64 does not fold
fn int_in(ty: &InferType, exact: i128) -> Option<ConstValue> {
    if ty.is_sized_integer() {
        return Some(ConstValue::Int(ty.wrap_int(exact as i64)));
    }
    i64::try_from(exact).ok().map(ConstValue::Int)
}

pub(crate) type LeafValue<'a> = &'a dyn Fn(&Expr) -> Option<ConstValue>;
pub(crate) type LeafType<'a> = &'a dyn Fn(&Expr) -> Option<InferType>;

fn is_shift(op: BinaryOp) -> bool {
    matches!(op, BinaryOp::Shl | BinaryOp::Shr)
}

fn is_comparison(op: BinaryOp) -> bool {
    matches!(
        op,
        BinaryOp::Eq | BinaryOp::Ne | BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge
    )
}

fn int_type_named(name: &str) -> Option<InferType> {
    Some(InferType::from_name(name)).filter(InferType::is_integer)
}

fn written_int_type(expr: &Expr, leaf_type: LeafType<'_>) -> Option<InferType> {
    let written = |expr: &Expr| written_int_type(expr, leaf_type);
    match &expr.kind {
        ExprKind::Int(_) => None,
        ExprKind::Grouping(inner) => written(inner),
        ExprKind::Unary {
            op: UnaryOp::Neg | UnaryOp::BitNot,
            operand,
        } => written(operand),
        ExprKind::Binary { left, op, .. } if is_shift(*op) => written(left),
        ExprKind::Binary { op, .. } if is_comparison(*op) => None,
        ExprKind::Binary { left, right, .. } => written(left).or_else(|| written(right)),
        ExprKind::Cast { target, .. } => int_type_named(&target.name),
        ExprKind::Identifier(_) | ExprKind::Member { .. } => {
            leaf_type(expr).filter(InferType::is_integer)
        }
        _ => None,
    }
}

fn operand_types(
    left: &Expr,
    op: BinaryOp,
    right: &Expr,
    leaf_type: LeafType<'_>,
    context: &InferType,
) -> (InferType, InferType) {
    let written = |expr: &Expr| written_int_type(expr, leaf_type);
    if is_shift(op) {
        return (
            written(left).unwrap_or_else(|| context.clone()),
            written(right).unwrap_or(InferType::I64),
        );
    }
    let fallback = if is_comparison(op) {
        InferType::I64
    } else {
        context.clone()
    };
    let shared = written(left).or_else(|| written(right)).unwrap_or(fallback);
    (shared.clone(), shared)
}

pub(crate) fn fold_const(
    expr: &Expr,
    leaf: LeafValue<'_>,
    leaf_type: LeafType<'_>,
    context: &InferType,
) -> Option<ConstValue> {
    let fold = |expr: &Expr, context: &InferType| fold_const(expr, leaf, leaf_type, context);
    match &expr.kind {
        ExprKind::Int(value) => int_in(context, i128::from(*value)),
        ExprKind::Float(value) => Some(ConstValue::Float(*value)),
        ExprKind::Bool(value) => Some(ConstValue::Bool(*value)),
        ExprKind::String(value) => Some(ConstValue::Str(value.clone())),
        ExprKind::Grouping(inner) => fold(inner, context),
        ExprKind::Unary { op, operand } => match (op, fold(operand, context)?) {
            (UnaryOp::Neg, ConstValue::Int(value)) => int_in(context, -context.exact_int(value)),
            (UnaryOp::Neg, ConstValue::Float(value)) => Some(ConstValue::Float(-value)),
            (UnaryOp::BitNot, ConstValue::Int(value)) => int_in(context, i128::from(!value)),
            (UnaryOp::Not, ConstValue::Bool(value)) => Some(ConstValue::Bool(!value)),
            _ => None,
        },
        ExprKind::And { left, right } => match (
            fold(left, &InferType::Bool)?,
            fold(right, &InferType::Bool)?,
        ) {
            (ConstValue::Bool(left), ConstValue::Bool(right)) => {
                Some(ConstValue::Bool(left && right))
            }
            _ => None,
        },
        ExprKind::Or { left, right } => match (
            fold(left, &InferType::Bool)?,
            fold(right, &InferType::Bool)?,
        ) {
            (ConstValue::Bool(left), ConstValue::Bool(right)) => {
                Some(ConstValue::Bool(left || right))
            }
            _ => None,
        },
        ExprKind::Binary { left, op, right } => {
            let (left_ty, right_ty) = operand_types(left, *op, right, leaf_type, context);
            fold_binary(
                fold(left, &left_ty)?,
                *op,
                fold(right, &right_ty)?,
                &left_ty,
            )
        }
        ExprKind::Cast {
            expr: inner,
            target,
        } => {
            let source = written_int_type(inner, leaf_type).unwrap_or(InferType::I64);
            fold_cast(
                fold(inner, &source)?,
                &source,
                &InferType::from_name(&target.name),
            )
        }
        _ => leaf(expr),
    }
}

fn fold_cast(value: ConstValue, source: &InferType, target: &InferType) -> Option<ConstValue> {
    match (value, target) {
        (ConstValue::Int(value), InferType::I64) => Some(ConstValue::Int(value)),
        (ConstValue::Int(value), target) if target.is_sized_integer() => {
            Some(ConstValue::Int(target.wrap_int(value)))
        }
        (ConstValue::Int(value), InferType::F64) => {
            Some(ConstValue::Float(source.exact_int(value) as f64))
        }
        (ConstValue::Float(value), InferType::F64) => Some(ConstValue::Float(value)),
        (ConstValue::Float(value), InferType::I64) if value.is_finite() => {
            let truncated = value.trunc();
            (-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0)
                .contains(&truncated)
                .then_some(ConstValue::Int(truncated as i64))
        }
        (ConstValue::Float(value), target) if target.is_sized_integer() && value.is_finite() => {
            let modulus = 2f64.powi(i32::try_from(target.int_bits()?).ok()?);
            Some(ConstValue::Int(
                target.wrap_int((value.trunc() % modulus) as i128 as i64),
            ))
        }
        _ => None,
    }
}

pub(crate) fn fold_failure(
    expr: &Expr,
    leaf: LeafValue<'_>,
    leaf_type: LeafType<'_>,
    context: &InferType,
) -> Option<String> {
    let fold = |expr: &Expr, context: &InferType| fold_const(expr, leaf, leaf_type, context);
    let failure = |expr: &Expr, context: &InferType| fold_failure(expr, leaf, leaf_type, context);
    match &expr.kind {
        ExprKind::Int(value) if int_in(context, i128::from(*value)).is_none() => {
            Some(format!("{value} is outside the int range"))
        }
        ExprKind::Grouping(inner) => failure(inner, context),
        ExprKind::Unary { op, operand } => {
            if let Some(cause) = failure(operand, context) {
                return Some(cause);
            }
            match (op, fold(operand, context)?) {
                (UnaryOp::Neg, ConstValue::Int(value))
                    if int_in(context, -context.exact_int(value)).is_none() =>
                {
                    Some(format!("-({value}) overflows the int range"))
                }
                _ => None,
            }
        }
        ExprKind::And { left, right } | ExprKind::Or { left, right } => {
            failure(left, &InferType::Bool).or_else(|| failure(right, &InferType::Bool))
        }
        ExprKind::Binary { left, op, right } => {
            let (left_ty, right_ty) = operand_types(left, *op, right, leaf_type, context);
            if let Some(cause) = failure(left, &left_ty).or_else(|| failure(right, &right_ty)) {
                return Some(cause);
            }
            let (left, right) = (fold(left, &left_ty)?, fold(right, &right_ty)?);
            if fold_binary(left.clone(), *op, right.clone(), &left_ty).is_some() {
                return None;
            }
            let (ConstValue::Int(left), ConstValue::Int(right)) = (left, right) else {
                return None;
            };
            match op {
                BinaryOp::Div if right == 0 => Some("division by zero".to_string()),
                BinaryOp::Mod if right == 0 => Some("remainder by zero".to_string()),
                _ => Some(format!("{left} {op} {right} overflows the int range")),
            }
        }
        ExprKind::Cast {
            expr: inner,
            target,
        } => {
            let source = written_int_type(inner, leaf_type).unwrap_or(InferType::I64);
            if let Some(cause) = failure(inner, &source) {
                return Some(cause);
            }
            match fold(inner, &source)? {
                ConstValue::Float(value) if fold(expr, context).is_none() => {
                    let target = InferType::from_name(&target.name);
                    if target.is_sized_integer() {
                        Some(format!("{value} cannot be converted to {target}"))
                    } else {
                        Some(format!("{value} does not fit in an int"))
                    }
                }
                _ => None,
            }
        }
        _ => None,
    }
}

fn fold_binary(
    left: ConstValue,
    op: BinaryOp,
    right: ConstValue,
    ty: &InferType,
) -> Option<ConstValue> {
    use ConstValue::{Bool, Float, Int, Str};
    match (left, right) {
        (Int(left), Int(right)) => {
            let (wide_left, wide_right) = (ty.exact_int(left), ty.exact_int(right));
            match op {
                BinaryOp::Add => int_in(ty, wide_left + wide_right),
                BinaryOp::Sub => int_in(ty, wide_left - wide_right),
                BinaryOp::Mul => int_in(ty, wide_left.wrapping_mul(wide_right)),
                BinaryOp::Div if right != 0 => int_in(ty, wide_left / wide_right),
                BinaryOp::Mod if right != 0 => int_in(ty, wide_left.wrapping_rem(wide_right)),
                BinaryOp::Div | BinaryOp::Mod => None,
                BinaryOp::Shl => int_in(ty, wide_left << (right & shift_mask(ty))),
                BinaryOp::Shr => int_in(ty, wide_left >> (right & shift_mask(ty))),
                BinaryOp::BitAnd => int_in(ty, wide_left & wide_right),
                BinaryOp::BitOr => int_in(ty, wide_left | wide_right),
                BinaryOp::BitXor => int_in(ty, wide_left ^ wide_right),
                BinaryOp::Eq => Some(Bool(wide_left == wide_right)),
                BinaryOp::Ne => Some(Bool(wide_left != wide_right)),
                BinaryOp::Lt => Some(Bool(wide_left < wide_right)),
                BinaryOp::Le => Some(Bool(wide_left <= wide_right)),
                BinaryOp::Gt => Some(Bool(wide_left > wide_right)),
                BinaryOp::Ge => Some(Bool(wide_left >= wide_right)),
            }
        }
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

fn shift_mask(ty: &InferType) -> i64 {
    ty.int_bits().map_or(63, |bits| i64::from(bits) - 1)
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

    pub(crate) fn constant_leaf_type(&self, expr: &Expr) -> Option<InferType> {
        let ExprKind::Identifier(name) = &expr.kind else {
            return None;
        };
        let constant = self.env.lookup_constant(name).is_some()
            || (!self.env.binds_value(name)
                && (self.global_constants.contains_key(name)
                    || self.imported_constants.contains_key(name)));
        constant
            .then(|| self.env.lookup(name).cloned())
            .flatten()
            .filter(InferType::is_integer)
    }

    pub(crate) fn constant_context(
        &self,
        name: &str,
        annotation: Option<&aelys_syntax::TypeAnnotation>,
    ) -> InferType {
        annotation
            .filter(|_| name != "_")
            .and_then(|_| self.env.lookup(name).cloned())
            .filter(InferType::is_integer)
            .or_else(|| annotation.and_then(|annotation| int_type_named(&annotation.name)))
            .unwrap_or(InferType::I64)
    }

    pub(crate) fn fold_constant(&self, expr: &Expr, context: &InferType) -> Option<ConstValue> {
        fold_const(
            expr,
            &|leaf| self.constant_leaf(leaf),
            &|leaf| self.constant_leaf_type(leaf),
            context,
        )
    }

    pub(crate) fn declare_constant(
        &mut self,
        name: &str,
        initializer: &Expr,
        context: &InferType,
        span: aelys_syntax::Span,
    ) -> Option<ConstValue> {
        if name != "_" && !self.env.first_constant_declaration(name) {
            self.errors.push(TypeError {
                kind: TypeErrorKind::DuplicateConstant {
                    name: name.to_string(),
                },
                span,
                reason: ConstraintReason::Other("constant declaration".to_string()),
            });
        }
        match self.fold_constant(initializer, context) {
            Some(value) => {
                if name != "_" {
                    self.env.define_constant(name, value.clone());
                }
                Some(value)
            }
            None => {
                let cause = fold_failure(
                    initializer,
                    &|leaf| self.constant_leaf(leaf),
                    &|leaf| self.constant_leaf_type(leaf),
                    context,
                );
                self.errors.push(TypeError {
                    kind: TypeErrorKind::NonConstantInitializer {
                        name: name.to_string(),
                        cause,
                    },
                    span: initializer.span,
                    reason: ConstraintReason::Other("constant initializer".to_string()),
                });
                None
            }
        }
    }

    pub(crate) fn predeclare_constants(&mut self, stmts: &[Stmt], bind: bool) {
        let mut pending: Vec<(&str, &Expr, Option<&aelys_syntax::TypeAnnotation>)> = stmts
            .iter()
            .filter_map(|stmt| match &stmt.kind {
                StmtKind::Let {
                    name,
                    initializer,
                    type_annotation,
                    is_const: true,
                    ..
                } => Some((name.as_str(), initializer, type_annotation.as_ref())),
                _ => None,
            })
            .collect();
        loop {
            let before = pending.len();
            pending.retain(|(name, initializer, annotation)| {
                let context = self.constant_context(name, *annotation);
                match self.fold_constant(initializer, &context) {
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
                }
            });
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
