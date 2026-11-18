use super::TypeInference;
use crate::constraint::{Constraint, ConstraintReason, TypeError, TypeErrorKind};
use crate::typed_ast::{TypedExpr, TypedExprKind, TypedStmt};
use crate::types::InferType;
use aelys_syntax::{Expr, ExprKind, MemberSeparator, Span};

pub(crate) fn map_key_types() -> Vec<InferType> {
    let mut keys = InferType::all_integer_types();
    keys.extend([InferType::String, InferType::Char, InferType::Bool]);
    keys
}

fn ordered_element_types() -> Vec<InferType> {
    let mut types = InferType::all_numeric_types();
    types.extend([InferType::String, InferType::Char, InferType::Bool]);
    types
}

fn container_method_name(member: &str) -> bool {
    matches!(
        member,
        "insert"
            | "remove"
            | "swap_remove"
            | "clear"
            | "extend"
            | "append"
            | "truncate"
            | "resize"
            | "swap"
            | "reverse"
            | "contains"
            | "index_of"
            | "sort"
            | "sort_by"
            | "first"
            | "last"
            | "set"
            | "fill"
            | "join"
            | "get"
            | "contains_key"
            | "len"
            | "is_empty"
            | "keys"
            | "values"
            | "is_digit"
            | "is_alpha"
            | "is_whitespace"
            | "is_alphanumeric"
            | "to_upper"
            | "to_lower"
            | "push"
    )
}

// a slot written from one shared value aliases every slot when the value is a reference type
fn holds_aggregate(ty: &InferType) -> bool {
    match ty {
        InferType::Struct(_)
        | InferType::Applied { .. }
        | InferType::Array(_)
        | InferType::FixedArray(_, _)
        | InferType::Vec(_)
        | InferType::Map(_, _)
        | InferType::Tuple(_) => true,
        InferType::Option(inner) => holds_aggregate(inner),
        InferType::Result(ok, err) => holds_aggregate(ok) || holds_aggregate(err),
        _ => false,
    }
}

struct MethodShape {
    params: Vec<InferType>,
    output: InferType,
    mutates: bool,
}

impl MethodShape {
    fn new(params: Vec<InferType>, output: InferType, mutates: bool) -> Self {
        Self {
            params,
            output,
            mutates,
        }
    }
}

fn invalid_map_key(ty: &InferType) -> Option<&InferType> {
    match ty {
        InferType::Map(key, value) => {
            if key.is_concrete() && !map_key_types().contains(key) {
                Some(key)
            } else {
                invalid_map_key(key).or_else(|| invalid_map_key(value))
            }
        }
        InferType::Array(inner)
        | InferType::FixedArray(inner, _)
        | InferType::Vec(inner)
        | InferType::Option(inner) => invalid_map_key(inner),
        InferType::Result(ok, err) => invalid_map_key(ok).or_else(|| invalid_map_key(err)),
        InferType::Tuple(elements) | InferType::Applied { args: elements, .. } => {
            elements.iter().find_map(invalid_map_key)
        }
        InferType::Function { params, ret } => params
            .iter()
            .find_map(invalid_map_key)
            .or_else(|| invalid_map_key(ret)),
        _ => None,
    }
}

fn instance_element_error(expr: &TypedExpr) -> Option<TypeErrorKind> {
    let TypedExprKind::Call { callee, args } = &expr.kind else {
        return None;
    };
    let TypedExprKind::Member {
        object,
        member,
        separator: MemberSeparator::Dot,
    } = &callee.kind
    else {
        return None;
    };
    let (InferType::Vec(element) | InferType::Array(element) | InferType::FixedArray(element, _)) =
        &object.ty
    else {
        return None;
    };
    match (member.as_str(), args.len()) {
        ("sort", 0) if element.is_concrete() && !ordered_element_types().contains(element) => {
            Some(TypeErrorKind::UnorderedSortElement {
                element: element.as_ref().clone(),
            })
        }
        ("fill", 1) | ("resize", 2) if holds_aggregate(element) => {
            Some(TypeErrorKind::SharedAggregateFill {
                method: member.clone(),
                element: element.as_ref().clone(),
            })
        }
        _ => None,
    }
}

impl TypeInference {
    // generic bodies are checked once with type parameters, so the element and key rules run again on every instance
    pub(in crate::infer) fn validate_collection_instances(&mut self, stmts: &mut [TypedStmt]) {
        let mut reported = self
            .errors
            .iter()
            .map(|error| {
                (
                    error.kind.diagnostic_code(),
                    error.span.start,
                    error.span.end,
                )
            })
            .collect::<std::collections::HashSet<_>>();
        let mut reported_keys = self
            .errors
            .iter()
            .filter_map(|error| match &error.kind {
                TypeErrorKind::InvalidMapKey { key } => Some(key.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let mut found = Vec::new();
        for stmt in stmts.iter_mut() {
            super::super::monomorphize::visit_exprs_stmt(stmt, &mut |expr| {
                if let Some(kind) = instance_element_error(expr) {
                    found.push((kind, expr.span));
                }
            });
        }
        for stmt in stmts.iter() {
            let mut types = Vec::new();
            super::super::monomorphize::collect_stmt_types(stmt, &mut types);
            for (ty, span) in types {
                if let Some(key) = invalid_map_key(&ty)
                    && !reported_keys.contains(key)
                {
                    reported_keys.push(key.clone());
                    found.push((TypeErrorKind::InvalidMapKey { key: key.clone() }, span));
                }
            }
        }
        for (kind, span) in found {
            if reported.insert((kind.diagnostic_code(), span.start, span.end)) {
                self.errors.push(TypeError {
                    kind,
                    span,
                    reason: ConstraintReason::Other("collection instance".to_string()),
                });
            }
        }
    }

    pub(crate) fn require_map_key(&mut self, key: &InferType, span: Span) {
        self.constraints.push(Constraint::one_of(
            key.clone(),
            map_key_types(),
            span,
            ConstraintReason::MapKey,
        ));
    }

    pub(super) fn infer_container_call(
        &mut self,
        callee: &Expr,
        args: &[Expr],
    ) -> Option<(TypedExprKind, InferType)> {
        if let Some(result) = self.infer_map_new(callee, args) {
            return Some(result);
        }
        let ExprKind::Member {
            object,
            member,
            separator: MemberSeparator::Dot,
        } = &callee.kind
        else {
            return None;
        };
        if !container_method_name(member) {
            return None;
        }
        let typed_object = self.infer_expr(object);
        let receiver = typed_object.ty.clone();
        let shape = match &receiver {
            InferType::Vec(element) => {
                self.sequence_shape(element, true, member, args.len(), callee.span)
            }
            InferType::Array(element) | InferType::FixedArray(element, _) => {
                self.sequence_shape(element, false, member, args.len(), callee.span)
            }
            InferType::Map(key, value) => Some(map_shape(key, value, member, args.len())?),
            InferType::Char => char_shape(member, args.len()),
            InferType::String if member == "push" && args.len() == 1 => {
                return Some(self.infer_string_push(typed_object, object, &args[0], callee.span));
            }
            _ => None,
        };
        let shape = match shape {
            Some(shape) => shape,
            None if matches!(receiver, InferType::Map(_, _))
                || (matches!(receiver, InferType::Array(_) | InferType::FixedArray(_, _))
                    && vec_only_method(member)) =>
            {
                return Some(self.reject_container_method(typed_object, member, args, callee));
            }
            None => return None,
        };
        if shape.mutates {
            self.require_mutable_collection(object, member, &receiver);
        }
        let mut typed_args: Vec<TypedExpr> = args
            .iter()
            .map(|arg| self.infer_expr(unborrowed(arg)))
            .collect();
        if member == "append" && matches!(receiver, InferType::Vec(_)) {
            self.require_mutable_collection(unborrowed(&args[0]), member, &receiver);
        }
        let mut params = shape.params;
        if member == "extend"
            && let (Some(InferType::Vec(element)), Some(arg)) = (params.first(), typed_args.first())
        {
            match &arg.ty {
                InferType::FixedArray(_, length) => {
                    params[0] = InferType::FixedArray(element.clone(), *length);
                }
                InferType::Array(_) => params[0] = InferType::Array(element.clone()),
                _ => {}
            }
        }
        for (index, (arg, expected)) in typed_args.iter_mut().zip(params.iter()).enumerate() {
            narrow_literal(arg, expected);
            let reason = ConstraintReason::Argument {
                func_name: format!("{}::{member}", receiver_family(&receiver)),
                arg_index: index,
            };
            if !self.reject_dynamic(&arg.ty, expected, arg.span, reason.clone())
                && !self.reject_untyped_native(&arg.ty, expected, arg.span, reason.clone())
            {
                self.constraints.push(Constraint::equal(
                    expected.clone(),
                    arg.ty.clone(),
                    arg.span,
                    reason,
                ));
            }
        }
        let output = shape.output;
        let typed_callee = TypedExpr::new(
            TypedExprKind::Member {
                object: Box::new(typed_object),
                member: member.clone(),
                separator: MemberSeparator::Dot,
            },
            InferType::Function {
                params,
                ret: Box::new(output.clone()),
            },
            callee.span,
        );
        Some((
            TypedExprKind::Call {
                callee: Box::new(typed_callee),
                args: typed_args,
            },
            output,
        ))
    }

    fn sequence_shape(
        &mut self,
        element: &InferType,
        is_vec: bool,
        member: &str,
        arity: usize,
        span: Span,
    ) -> Option<MethodShape> {
        let t = element.clone();
        let vec_only = |shape: MethodShape| is_vec.then_some(shape);
        let shape = match (member, arity) {
            ("insert", 2) => vec_only(MethodShape::new(
                vec![InferType::I64, t],
                InferType::Unit,
                true,
            ))?,
            ("remove" | "swap_remove", 1) => {
                vec_only(MethodShape::new(vec![InferType::I64], t, true))?
            }
            ("clear", 0) => vec_only(MethodShape::new(Vec::new(), InferType::Unit, true))?,
            ("extend" | "append", 1) => vec_only(MethodShape::new(
                vec![InferType::Vec(Box::new(t))],
                InferType::Unit,
                true,
            ))?,
            ("truncate", 1) => vec_only(MethodShape::new(
                vec![InferType::I64],
                InferType::Unit,
                true,
            ))?,
            ("resize", 2) => {
                self.reject_shared_fill(member, element, span);
                vec_only(MethodShape::new(
                    vec![InferType::I64, t],
                    InferType::Unit,
                    true,
                ))?
            }
            ("swap", 2) => {
                MethodShape::new(vec![InferType::I64, InferType::I64], InferType::Unit, true)
            }
            ("reverse", 0) => MethodShape::new(Vec::new(), InferType::Unit, true),
            ("contains", 1) => MethodShape::new(vec![t], InferType::Bool, false),
            ("index_of", 1) => {
                MethodShape::new(vec![t], InferType::Option(Box::new(InferType::I64)), false)
            }
            ("sort", 0) => {
                self.require_ordered_element(element, span);
                MethodShape::new(Vec::new(), InferType::Unit, true)
            }
            ("sort_by", 1) => MethodShape::new(
                vec![InferType::Function {
                    params: vec![t.clone(), t],
                    ret: Box::new(InferType::I64),
                }],
                InferType::Unit,
                true,
            ),
            ("first" | "last", 0) => {
                MethodShape::new(Vec::new(), InferType::Option(Box::new(t)), false)
            }
            ("set", 2) => MethodShape::new(vec![InferType::I64, t], InferType::Unit, true),
            ("fill", 1) => {
                self.reject_shared_fill(member, element, span);
                MethodShape::new(vec![t], InferType::Unit, true)
            }
            ("join", 1) => {
                self.constraints.push(Constraint::equal(
                    InferType::String,
                    t,
                    span,
                    ConstraintReason::Other("join needs a collection of strings".to_string()),
                ));
                MethodShape::new(vec![InferType::String], InferType::String, false)
            }
            _ => return None,
        };
        Some(shape)
    }

    fn require_ordered_element(&mut self, element: &InferType, span: Span) {
        if element.is_concrete() {
            if !ordered_element_types().contains(element) {
                self.errors.push(TypeError {
                    kind: TypeErrorKind::UnorderedSortElement {
                        element: element.clone(),
                    },
                    span,
                    reason: ConstraintReason::Other("sort element".to_string()),
                });
            }
        } else if matches!(element, InferType::Var(_)) {
            self.constraints.push(Constraint::one_of(
                element.clone(),
                ordered_element_types(),
                span,
                ConstraintReason::Other("sort element".to_string()),
            ));
        }
    }

    fn reject_shared_fill(&mut self, member: &str, element: &InferType, span: Span) {
        if holds_aggregate(element) {
            self.errors.push(TypeError {
                kind: TypeErrorKind::SharedAggregateFill {
                    method: member.to_string(),
                    element: element.clone(),
                },
                span,
                reason: ConstraintReason::Other("shared aggregate fill".to_string()),
            });
        }
    }

    fn infer_map_new(
        &mut self,
        callee: &Expr,
        args: &[Expr],
    ) -> Option<(TypedExprKind, InferType)> {
        let ExprKind::Member {
            object,
            member,
            separator: MemberSeparator::Path,
        } = &callee.kind
        else {
            return None;
        };
        let ExprKind::Identifier(name) = &object.kind else {
            return None;
        };
        if name != "Map"
            || member != "new"
            || self.type_table.has_nominal(name)
            || self.env.lookup(name).is_some()
        {
            return None;
        }
        let typed_args: Vec<TypedExpr> = args.iter().map(|arg| self.infer_expr(arg)).collect();
        if !typed_args.is_empty() {
            self.errors.push(TypeError {
                kind: TypeErrorKind::ArityMismatch {
                    expected: 0,
                    found: typed_args.len(),
                },
                span: callee.span,
                reason: ConstraintReason::Other("Map::new".to_string()),
            });
        }
        let key = self.type_gen.fresh();
        let value = self.type_gen.fresh();
        self.require_map_key(&key, callee.span);
        let map = InferType::Map(Box::new(key), Box::new(value));
        let typed_callee = TypedExpr::new(
            TypedExprKind::Member {
                object: Box::new(TypedExpr::new(
                    TypedExprKind::Unit,
                    map.clone(),
                    object.span,
                )),
                member: member.clone(),
                separator: MemberSeparator::Path,
            },
            InferType::Function {
                params: Vec::new(),
                ret: Box::new(map.clone()),
            },
            callee.span,
        );
        Some((
            TypedExprKind::Call {
                callee: Box::new(typed_callee),
                args: typed_args,
            },
            map,
        ))
    }

    fn infer_string_push(
        &mut self,
        typed_object: TypedExpr,
        object: &Expr,
        arg: &Expr,
        span: Span,
    ) -> (TypedExprKind, InferType) {
        let typed_arg = self.infer_expr(arg);
        let reason = ConstraintReason::Argument {
            func_name: "string::push".to_string(),
            arg_index: 0,
        };
        if !self.reject_dynamic(
            &typed_arg.ty,
            &InferType::Char,
            typed_arg.span,
            reason.clone(),
        ) {
            self.constraints.push(Constraint::equal(
                InferType::Char,
                typed_arg.ty.clone(),
                typed_arg.span,
                reason,
            ));
        }
        let call = TypedExpr::new(
            TypedExprKind::Call {
                callee: Box::new(TypedExpr::new(
                    TypedExprKind::Member {
                        object: Box::new(typed_object),
                        member: "push".to_string(),
                        separator: MemberSeparator::Dot,
                    },
                    InferType::Function {
                        params: vec![InferType::Char],
                        ret: Box::new(InferType::String),
                    },
                    span,
                )),
                args: vec![typed_arg],
            },
            InferType::String,
            span,
        );
        let ExprKind::Identifier(name) = &object.kind else {
            self.errors.push(TypeError {
                kind: TypeErrorKind::MutableCollectionRequired {
                    method: "push".to_string(),
                    place: crate::infer::places::render_place(object),
                    root: crate::infer::places::place_root(object).map(str::to_string),
                },
                span: object.span,
                reason: ConstraintReason::Other("string push needs a variable".to_string()),
            });
            return (call.kind, InferType::Poison);
        };
        self.check_write_binding(name, span, "assignment");
        let resolved = self.env.lookup_alias(name).unwrap_or(name).to_string();
        (
            TypedExprKind::Assign {
                name: resolved,
                value: Box::new(call),
            },
            InferType::String,
        )
    }

    fn reject_container_method(
        &mut self,
        typed_object: TypedExpr,
        member: &str,
        args: &[Expr],
        callee: &Expr,
    ) -> (TypedExprKind, InferType) {
        let typed_args: Vec<TypedExpr> = args
            .iter()
            .map(|arg| self.infer_expr(unborrowed(arg)))
            .collect();
        self.errors.push(TypeError {
            kind: TypeErrorKind::InvalidCollectionMethod {
                method: member.to_string(),
                receiver: typed_object.ty.clone(),
            },
            span: typed_object.span,
            reason: ConstraintReason::Other("collection method receiver".to_string()),
        });
        let typed_callee = TypedExpr::new(
            TypedExprKind::Member {
                object: Box::new(typed_object),
                member: member.to_string(),
                separator: MemberSeparator::Dot,
            },
            InferType::Function {
                params: vec![InferType::Poison; typed_args.len()],
                ret: Box::new(InferType::Poison),
            },
            callee.span,
        );
        (
            TypedExprKind::Call {
                callee: Box::new(typed_callee),
                args: typed_args,
            },
            InferType::Poison,
        )
    }
}

fn map_shape(
    key: &InferType,
    value: &InferType,
    member: &str,
    arity: usize,
) -> Option<MethodShape> {
    let k = key.clone();
    let v = value.clone();
    let shape = match (member, arity) {
        ("insert", 2) => MethodShape::new(vec![k, v.clone()], InferType::Option(Box::new(v)), true),
        ("get", 1) => MethodShape::new(vec![k], InferType::Option(Box::new(v)), false),
        ("contains_key", 1) => MethodShape::new(vec![k], InferType::Bool, false),
        ("remove", 1) => MethodShape::new(vec![k], InferType::Option(Box::new(v)), true),
        ("len", 0) => MethodShape::new(Vec::new(), InferType::I64, false),
        ("is_empty", 0) => MethodShape::new(Vec::new(), InferType::Bool, false),
        ("clear", 0) => MethodShape::new(Vec::new(), InferType::Unit, true),
        ("keys", 0) => MethodShape::new(Vec::new(), InferType::Vec(Box::new(k)), false),
        ("values", 0) => MethodShape::new(Vec::new(), InferType::Vec(Box::new(v)), false),
        _ => return None,
    };
    Some(shape)
}

fn char_shape(member: &str, arity: usize) -> Option<MethodShape> {
    match (member, arity) {
        ("is_digit" | "is_alpha" | "is_whitespace" | "is_alphanumeric", 0) => {
            Some(MethodShape::new(Vec::new(), InferType::Bool, false))
        }
        ("to_upper" | "to_lower", 0) => Some(MethodShape::new(Vec::new(), InferType::Char, false)),
        _ => None,
    }
}

fn vec_only_method(member: &str) -> bool {
    matches!(
        member,
        "insert" | "remove" | "swap_remove" | "clear" | "extend" | "append" | "truncate" | "resize"
    )
}

fn receiver_family(receiver: &InferType) -> &'static str {
    match receiver {
        InferType::Map(_, _) => "Map",
        InferType::Char => "char",
        InferType::Vec(_) => "Vec",
        _ => "array",
    }
}

fn unborrowed(arg: &Expr) -> &Expr {
    match &arg.kind {
        ExprKind::Borrow {
            mutable: false,
            operand,
        } => operand,
        _ => arg,
    }
}

fn narrow_literal(arg: &mut TypedExpr, expected: &InferType) {
    if let TypedExprKind::Int(value) = &arg.kind
        && expected.is_integer()
        && *expected != InferType::I64
        && InferType::int_fits(*value, expected)
    {
        arg.ty = expected.clone();
    }
}
