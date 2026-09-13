use super::TypeInference;
use crate::constraint::{ConstraintReason, TypeError, TypeErrorKind};
use crate::typed_ast::{
    TypedExpr, TypedExprKind, TypedFmtStringPart, TypedFunction, TypedMatchArmBody, TypedParam,
    TypedPattern, TypedPatternKind, TypedStmt, TypedStmtKind,
};
use crate::types::{BoundSelection, InferType};
use crate::unify::Substitution;
use aelys_syntax::{MemberSeparator, Span};
use std::collections::{HashMap, HashSet, VecDeque};

const MAX_INSTANCES: usize = 65_536;
const MAX_ACTIVE_INSTANCES: usize = 1_024;

pub(crate) const BOUND_MARKER_PREFIX: &str = "__aelys_bound::";
pub(crate) const DISPLAY_MARKER_SYMBOL: &str = "__aelys_display::to_display";
const GENERIC_INSTANCE_PREFIX: &str = "__aelys_generic_";
const IMPL_INSTANCE_INFIX: &str = "$s2$";

pub(crate) fn bound_marker_symbol(trait_name: &str, param: &str, method: &str) -> String {
    format!(
        "{}{:08x}:{}{:08x}:{}{:08x}:{}",
        BOUND_MARKER_PREFIX,
        trait_name.len(),
        trait_name,
        param.len(),
        param,
        method.len(),
        method
    )
}

pub(crate) fn parse_bound_marker(symbol: &str) -> Option<(String, String, String)> {
    let mut cursor = symbol.strip_prefix(BOUND_MARKER_PREFIX)?;
    let mut parts = Vec::with_capacity(3);
    for _ in 0..3 {
        let (length, tail) = cursor.split_once(':')?;
        let length = usize::from_str_radix(length, 16).ok()?;
        if tail.len() < length || !tail.is_char_boundary(length) {
            return None;
        }
        let (value, tail) = tail.split_at(length);
        parts.push(value.to_string());
        cursor = tail;
    }
    if !cursor.is_empty() {
        return None;
    }
    let mut parts = parts.into_iter();
    Some((parts.next()?, parts.next()?, parts.next()?))
}

fn parse_fixed_len_fields(mut cursor: &str, count: usize) -> Option<Vec<String>> {
    let mut parts = Vec::with_capacity(count);
    for _ in 0..count {
        if cursor.len() < 8 || !cursor.is_char_boundary(8) {
            return None;
        }
        let (length, tail) = cursor.split_at(8);
        let length = usize::from_str_radix(length, 16).ok()?;
        let tail = tail.strip_prefix(':')?;
        if tail.len() < length || !tail.is_char_boundary(length) {
            return None;
        }
        let (value, tail) = tail.split_at(length);
        parts.push(value.to_string());
        cursor = tail;
    }
    Some(parts)
}

fn decode_generic_instance_name(hex: &str) -> Option<String> {
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    let mut bytes = Vec::with_capacity(hex.len() / 2);
    for pair in hex.as_bytes().chunks(2) {
        let pair = std::str::from_utf8(pair).ok()?;
        bytes.push(u8::from_str_radix(pair, 16).ok()?);
    }
    let decoded = String::from_utf8(bytes).ok()?;
    let (length, tail) = decoded.split_once(':')?;
    let length = length.parse::<usize>().ok()?;
    (tail.len() >= length && tail.is_char_boundary(length)).then(|| tail[..length].to_string())
}

fn user_facing_symbol(symbol: &str) -> String {
    let base = symbol.split(IMPL_INSTANCE_INFIX).next().unwrap_or(symbol);
    if base == DISPLAY_MARKER_SYMBOL {
        return "to_display".to_string();
    }
    if let Some(rest) = base.strip_prefix("__aelys_struct::")
        && let Some(parts) = parse_fixed_len_fields(rest, 2)
    {
        return format!("{}::{}", parts[0], parts[1]);
    }
    if let Some(rest) = base.strip_prefix("__aelys_trait::")
        && let Some(parts) = parse_fixed_len_fields(rest, 3)
    {
        return format!("{}::{}", parts[1], parts[2]);
    }
    if let Some((_, param, method)) = parse_bound_marker(base) {
        return format!("{param}::{method}");
    }
    if let Some(hex) = base.strip_prefix(GENERIC_INSTANCE_PREFIX)
        && let Some(name) = decode_generic_instance_name(hex)
    {
        return name;
    }
    "a generic item".to_string()
}

fn is_generated_instance_symbol(symbol: &str) -> bool {
    symbol == DISPLAY_MARKER_SYMBOL
        || symbol.starts_with(BOUND_MARKER_PREFIX)
        || symbol.starts_with(GENERIC_INSTANCE_PREFIX)
        || symbol.starts_with("__aelys_struct::")
        || symbol.starts_with("__aelys_trait::")
        || symbol.contains(IMPL_INSTANCE_INFIX)
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct InstanceKey {
    name: String,
    args: Vec<InferType>,
}

struct InstanceFrame {
    key: InstanceKey,
    ancestors: Vec<(String, Vec<InferType>)>,
}

fn format_type_application(name: &str, args: &[InferType]) -> String {
    let mut result = String::new();
    push_len_prefixed(&mut result, name);
    for arg in args {
        let mut encoded = String::new();
        encode_type_key(arg, &mut encoded);
        push_len_prefixed(&mut result, &encoded);
    }
    result
}

fn push_len_prefixed(result: &mut String, value: &str) {
    use std::fmt::Write;
    let _ = write!(result, "{}:", value.len());
    result.push_str(value);
}

fn encode_type_key(ty: &InferType, result: &mut String) {
    match ty {
        InferType::I8 => push_len_prefixed(result, "i8"),
        InferType::I16 => push_len_prefixed(result, "i16"),
        InferType::I32 => push_len_prefixed(result, "i32"),
        InferType::I64 => push_len_prefixed(result, "i64"),
        InferType::U8 => push_len_prefixed(result, "u8"),
        InferType::U16 => push_len_prefixed(result, "u16"),
        InferType::U32 => push_len_prefixed(result, "u32"),
        InferType::U64 => push_len_prefixed(result, "u64"),
        InferType::F32 => push_len_prefixed(result, "f32"),
        InferType::F64 => push_len_prefixed(result, "f64"),
        InferType::Bool => push_len_prefixed(result, "bool"),
        InferType::String => push_len_prefixed(result, "string"),
        InferType::Unit => push_len_prefixed(result, "unit"),
        InferType::Null => push_len_prefixed(result, "null"),
        InferType::Error => push_len_prefixed(result, "error"),
        InferType::Never => push_len_prefixed(result, "never"),
        InferType::Numeric => push_len_prefixed(result, "numeric"),
        InferType::Range => push_len_prefixed(result, "range"),
        InferType::Dynamic => push_len_prefixed(result, "dynamic"),
        InferType::Poison => push_len_prefixed(result, "poison"),
        InferType::UntypedNative(name) => {
            push_len_prefixed(result, "native");
            push_len_prefixed(result, name);
        }
        InferType::Struct(name) => {
            push_len_prefixed(result, "struct");
            push_len_prefixed(result, name);
        }
        InferType::Param(name) => {
            push_len_prefixed(result, "param");
            push_len_prefixed(result, name);
        }
        InferType::Projection {
            trait_name,
            item,
            self_ty,
        } => {
            push_len_prefixed(result, "projection");
            push_len_prefixed(result, trait_name.as_deref().unwrap_or(""));
            push_len_prefixed(result, item);
            encode_type_key(self_ty, result);
        }
        InferType::Var(id) => {
            push_len_prefixed(result, "var");
            push_len_prefixed(result, &id.0.to_string());
        }
        InferType::Option(inner) => {
            push_len_prefixed(result, "option");
            encode_type_key(inner, result);
        }
        InferType::Result(ok, err) => {
            push_len_prefixed(result, "result");
            encode_type_key(ok, result);
            encode_type_key(err, result);
        }
        InferType::Array(inner) => {
            push_len_prefixed(result, "array");
            encode_type_key(inner, result);
        }
        InferType::FixedArray(inner, length) => {
            push_len_prefixed(result, "fixed_array");
            push_len_prefixed(result, &length.to_string());
            encode_type_key(inner, result);
        }
        InferType::Vec(inner) => {
            push_len_prefixed(result, "vec");
            encode_type_key(inner, result);
        }
        InferType::Tuple(elements) => {
            push_len_prefixed(result, "tuple");
            push_len_prefixed(result, &elements.len().to_string());
            for element in elements {
                encode_type_key(element, result);
            }
        }
        InferType::Function { params, ret } => {
            push_len_prefixed(result, "function");
            push_len_prefixed(result, &params.len().to_string());
            for param in params {
                encode_type_key(param, result);
            }
            encode_type_key(ret, result);
        }
        InferType::Applied { name, args } => {
            push_len_prefixed(result, "applied");
            push_len_prefixed(result, name);
            push_len_prefixed(result, &args.len().to_string());
            for arg in args {
                encode_type_key(arg, result);
            }
        }
    }
}

fn nominal_instance_name(name: &str, args: &[InferType]) -> String {
    let canonical = format_type_application(name, args);
    let mut encoded = String::with_capacity(canonical.len() * 2);
    for byte in canonical.bytes() {
        use std::fmt::Write;
        let _ = write!(encoded, "{byte:02x}");
    }
    format!("__aelys_instance_{encoded}")
}

fn is_structural_subterm(candidate: &InferType, parent: &InferType) -> bool {
    match parent {
        InferType::Function { params, ret } => {
            params
                .iter()
                .any(|param| candidate == param || is_structural_subterm(candidate, param))
                || candidate == ret.as_ref()
                || is_structural_subterm(candidate, ret)
        }
        InferType::Array(inner)
        | InferType::FixedArray(inner, _)
        | InferType::Vec(inner)
        | InferType::Option(inner) => {
            candidate == inner.as_ref() || is_structural_subterm(candidate, inner)
        }
        InferType::Result(ok, err) => {
            candidate == ok.as_ref()
                || candidate == err.as_ref()
                || is_structural_subterm(candidate, ok)
                || is_structural_subterm(candidate, err)
        }
        InferType::Tuple(elements) => elements
            .iter()
            .any(|element| candidate == element || is_structural_subterm(candidate, element)),
        InferType::Applied { args, .. } => args
            .iter()
            .any(|arg| candidate == arg || is_structural_subterm(candidate, arg)),
        _ => false,
    }
}

fn is_strictly_decreasing(callee_args: &[InferType], caller_args: &[InferType]) -> bool {
    callee_args.len() == caller_args.len()
        && callee_args
            .iter()
            .zip(caller_args)
            .all(|(callee, caller)| callee == caller || is_structural_subterm(callee, caller))
        && callee_args
            .iter()
            .zip(caller_args)
            .any(|(callee, caller)| callee != caller)
}

fn collect_nominal_applications(
    ty: &InferType,
    type_table: &crate::types::TypeTable,
    applications: &mut HashMap<String, (String, Vec<InferType>)>,
) {
    match ty {
        InferType::Applied { name, args } => {
            let is_generic = type_table
                .get_struct(name)
                .is_some_and(|definition| definition.type_params.len() == args.len())
                || type_table
                    .get_enum(name)
                    .is_some_and(|definition| definition.type_params.len() == args.len());
            if is_generic && args.iter().all(InferType::is_concrete) {
                applications
                    .entry(format_type_application(name, args))
                    .or_insert_with(|| (name.clone(), args.clone()));
            }
            for arg in args {
                collect_nominal_applications(arg, type_table, applications);
            }
        }
        InferType::Projection { self_ty, .. } => {
            collect_nominal_applications(self_ty, type_table, applications);
        }
        InferType::Function { params, ret } => {
            for param in params {
                collect_nominal_applications(param, type_table, applications);
            }
            collect_nominal_applications(ret, type_table, applications);
        }
        InferType::Array(inner)
        | InferType::FixedArray(inner, _)
        | InferType::Vec(inner)
        | InferType::Option(inner) => collect_nominal_applications(inner, type_table, applications),
        InferType::Result(ok, err) => {
            collect_nominal_applications(ok, type_table, applications);
            collect_nominal_applications(err, type_table, applications);
        }
        InferType::Tuple(elements) => {
            for element in elements {
                collect_nominal_applications(element, type_table, applications);
            }
        }
        InferType::I8
        | InferType::I16
        | InferType::I32
        | InferType::I64
        | InferType::U8
        | InferType::U16
        | InferType::U32
        | InferType::U64
        | InferType::F32
        | InferType::F64
        | InferType::Bool
        | InferType::String
        | InferType::Unit
        | InferType::Null
        | InferType::Error
        | InferType::Never
        | InferType::Numeric
        | InferType::UntypedNative(_)
        | InferType::Struct(_)
        | InferType::Param(_)
        | InferType::Var(_)
        | InferType::Dynamic
        | InferType::Poison
        | InferType::Range => {}
    }
}

fn replace_nominal_type(ty: &InferType, replacements: &HashMap<String, String>) -> InferType {
    if let InferType::Applied { name, args } = ty {
        let key = format_type_application(name, args);
        if let Some(replacement) = replacements.get(&key) {
            return InferType::Struct(replacement.clone());
        }
        return InferType::Applied {
            name: name.clone(),
            args: args
                .iter()
                .map(|arg| replace_nominal_type(arg, replacements))
                .collect(),
        };
    }
    match ty {
        InferType::Function { params, ret } => InferType::Function {
            params: params
                .iter()
                .map(|param| replace_nominal_type(param, replacements))
                .collect(),
            ret: Box::new(replace_nominal_type(ret, replacements)),
        },
        InferType::Array(inner) => {
            InferType::Array(Box::new(replace_nominal_type(inner, replacements)))
        }
        InferType::FixedArray(inner, length) => {
            InferType::FixedArray(Box::new(replace_nominal_type(inner, replacements)), *length)
        }
        InferType::Vec(inner) => {
            InferType::Vec(Box::new(replace_nominal_type(inner, replacements)))
        }
        InferType::Option(inner) => {
            InferType::Option(Box::new(replace_nominal_type(inner, replacements)))
        }
        InferType::Result(ok, err) => InferType::Result(
            Box::new(replace_nominal_type(ok, replacements)),
            Box::new(replace_nominal_type(err, replacements)),
        ),
        InferType::Tuple(elements) => InferType::Tuple(
            elements
                .iter()
                .map(|element| replace_nominal_type(element, replacements))
                .collect(),
        ),
        InferType::Projection {
            trait_name,
            item,
            self_ty,
        } => InferType::Projection {
            trait_name: trait_name.clone(),
            item: item.clone(),
            self_ty: Box::new(replace_nominal_type(self_ty, replacements)),
        },
        _ => ty.clone(),
    }
}

fn rewrite_nominal_stmt(
    stmt: &mut TypedStmt,
    replacements: &HashMap<String, String>,
    type_table: &crate::types::TypeTable,
) {
    match &mut stmt.kind {
        TypedStmtKind::Expression(expr) => rewrite_nominal_expr(expr, replacements, type_table),
        TypedStmtKind::Let {
            initializer,
            var_type,
            ..
        } => {
            *var_type = replace_nominal_type(var_type, replacements);
            rewrite_nominal_expr(initializer, replacements, type_table);
        }
        TypedStmtKind::Block(stmts) => {
            for stmt in stmts {
                rewrite_nominal_stmt(stmt, replacements, type_table);
            }
        }
        TypedStmtKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            rewrite_nominal_expr(condition, replacements, type_table);
            rewrite_nominal_stmt(then_branch, replacements, type_table);
            if let Some(else_branch) = else_branch {
                rewrite_nominal_stmt(else_branch, replacements, type_table);
            }
        }
        TypedStmtKind::While { condition, body } => {
            rewrite_nominal_expr(condition, replacements, type_table);
            rewrite_nominal_stmt(body, replacements, type_table);
        }
        TypedStmtKind::For {
            start,
            end,
            step,
            body,
            ..
        } => {
            rewrite_nominal_expr(start, replacements, type_table);
            rewrite_nominal_expr(end, replacements, type_table);
            if let Some(step) = step.as_mut() {
                rewrite_nominal_expr(step, replacements, type_table);
            }
            rewrite_nominal_stmt(body, replacements, type_table);
        }
        TypedStmtKind::ForEach {
            iterable,
            elem_type,
            body,
            ..
        } => {
            *elem_type = replace_nominal_type(elem_type, replacements);
            rewrite_nominal_expr(iterable, replacements, type_table);
            rewrite_nominal_stmt(body, replacements, type_table);
        }
        TypedStmtKind::Return(expr) => {
            if let Some(expr) = expr {
                rewrite_nominal_expr(expr, replacements, type_table);
            }
        }
        TypedStmtKind::Function(function) => {
            for param in &mut function.params {
                param.ty = replace_nominal_type(&param.ty, replacements);
            }
            function.return_type = replace_nominal_type(&function.return_type, replacements);
            for (name, ty) in &mut function.captures {
                let _ = name;
                *ty = replace_nominal_type(ty, replacements);
            }
            for stmt in &mut function.body {
                rewrite_nominal_stmt(stmt, replacements, type_table);
            }
        }
        TypedStmtKind::ImplDecl { methods, .. } => {
            for method in methods {
                for param in &mut method.params {
                    param.ty = replace_nominal_type(&param.ty, replacements);
                }
                method.return_type = replace_nominal_type(&method.return_type, replacements);
                for stmt in &mut method.body {
                    rewrite_nominal_stmt(stmt, replacements, type_table);
                }
            }
        }
        TypedStmtKind::StructDecl { fields, .. } => {
            for (_, ty) in fields {
                *ty = replace_nominal_type(ty, replacements);
            }
        }
        TypedStmtKind::EnumDecl { variants, .. } => {
            for variant in variants {
                match &mut variant.fields {
                    crate::types::EnumVariantFieldsDef::Unit => {}
                    crate::types::EnumVariantFieldsDef::Tuple(fields) => {
                        for field in fields {
                            *field = replace_nominal_type(field, replacements);
                        }
                    }
                    crate::types::EnumVariantFieldsDef::Named(fields) => {
                        for field in fields {
                            field.ty = replace_nominal_type(&field.ty, replacements);
                        }
                    }
                }
            }
        }
        TypedStmtKind::Break
        | TypedStmtKind::Continue
        | TypedStmtKind::Needs(_)
        | TypedStmtKind::TraitDecl { .. } => {}
    }
}

fn rewrite_nominal_expr(
    expr: &mut TypedExpr,
    replacements: &HashMap<String, String>,
    type_table: &crate::types::TypeTable,
) {
    expr.ty = replace_nominal_type(&expr.ty, replacements);
    let expression_type_name = nominal_type_name(&expr.ty);
    match &mut expr.kind {
        TypedExprKind::Binary { left, right, .. }
        | TypedExprKind::And { left, right }
        | TypedExprKind::Or { left, right } => {
            rewrite_nominal_expr(left, replacements, type_table);
            rewrite_nominal_expr(right, replacements, type_table);
        }
        TypedExprKind::Unary { operand, .. }
        | TypedExprKind::Grouping(operand)
        | TypedExprKind::Lambda(operand)
        | TypedExprKind::Try { operand, .. } => {
            rewrite_nominal_expr(operand, replacements, type_table)
        }
        TypedExprKind::Call { callee, args } => {
            rewrite_nominal_expr(callee, replacements, type_table);
            for arg in args {
                rewrite_nominal_expr(arg, replacements, type_table);
            }
        }
        TypedExprKind::Assign { value, .. } => {
            rewrite_nominal_expr(value, replacements, type_table)
        }
        TypedExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            rewrite_nominal_expr(condition, replacements, type_table);
            rewrite_nominal_expr(then_branch, replacements, type_table);
            rewrite_nominal_expr(else_branch, replacements, type_table);
        }
        TypedExprKind::Match { scrutinee, arms } => {
            rewrite_nominal_expr(scrutinee, replacements, type_table);
            for arm in arms {
                rewrite_nominal_pattern(&mut arm.pattern, replacements, type_table);
                if let Some(guard) = &mut arm.guard {
                    rewrite_nominal_expr(guard, replacements, type_table);
                }
                match &mut arm.body {
                    TypedMatchArmBody::Expr(expr) => {
                        rewrite_nominal_expr(expr, replacements, type_table)
                    }
                    TypedMatchArmBody::Block(stmts) => {
                        for stmt in stmts {
                            rewrite_nominal_stmt(stmt, replacements, type_table);
                        }
                    }
                }
            }
        }
        TypedExprKind::LambdaInner {
            params,
            return_type,
            captures,
            body,
        } => {
            for param in params {
                param.ty = replace_nominal_type(&param.ty, replacements);
            }
            *return_type = replace_nominal_type(return_type, replacements);
            for (_, ty) in captures {
                *ty = replace_nominal_type(ty, replacements);
            }
            for stmt in body {
                rewrite_nominal_stmt(stmt, replacements, type_table);
            }
        }
        TypedExprKind::Member { object, .. } | TypedExprKind::StructMethod { object, .. } => {
            rewrite_nominal_expr(object, replacements, type_table);
        }
        TypedExprKind::StructField {
            object,
            schema_index,
            ..
        } => {
            rewrite_nominal_expr(object, replacements, type_table);
            refresh_object_schema_index(object, schema_index, type_table);
        }
        TypedExprKind::MemberAssign {
            object,
            schema_index,
            value,
            ..
        } => {
            rewrite_nominal_expr(object, replacements, type_table);
            refresh_object_schema_index(object, schema_index, type_table);
            rewrite_nominal_expr(value, replacements, type_table);
        }
        TypedExprKind::ArrayLiteral {
            elements, repeat, ..
        }
        | TypedExprKind::VecLiteral {
            elements, repeat, ..
        } => {
            for element in elements {
                rewrite_nominal_expr(element, replacements, type_table);
            }
            if let Some(repeat) = repeat {
                rewrite_nominal_expr(repeat, replacements, type_table);
            }
        }
        TypedExprKind::ArraySized { size, .. } => {
            rewrite_nominal_expr(size, replacements, type_table)
        }
        TypedExprKind::Index { object, index }
        | TypedExprKind::Slice {
            object,
            range: index,
        } => {
            rewrite_nominal_expr(object, replacements, type_table);
            rewrite_nominal_expr(index, replacements, type_table);
        }
        TypedExprKind::IndexAssign {
            object,
            index,
            value,
        } => {
            rewrite_nominal_expr(object, replacements, type_table);
            rewrite_nominal_expr(index, replacements, type_table);
            rewrite_nominal_expr(value, replacements, type_table);
        }
        TypedExprKind::Range { start, end, .. } => {
            if let Some(start) = start {
                rewrite_nominal_expr(start, replacements, type_table);
            }
            if let Some(end) = end {
                rewrite_nominal_expr(end, replacements, type_table);
            }
        }
        TypedExprKind::StructLiteral {
            name,
            schema_index,
            fields,
            ..
        } => {
            if let Some(type_name) = expression_type_name.clone() {
                *name = type_name.clone();
                if let Some(index) = nominal_schema_index(type_table, &type_name) {
                    *schema_index = index;
                }
            }
            for (_, field) in fields {
                rewrite_nominal_expr(field, replacements, type_table);
            }
        }
        TypedExprKind::EnumConstruct {
            enum_name,
            schema_index,
            fields,
            ..
        } => {
            if let Some(type_name) = expression_type_name {
                *enum_name = type_name.clone();
                if let Some(index) = type_table.enum_schema_index(&type_name) {
                    *schema_index = index;
                }
            }
            for (_, field) in fields {
                rewrite_nominal_expr(field, replacements, type_table);
            }
        }
        TypedExprKind::Cast { expr, target } => {
            *target = replace_nominal_type(target, replacements);
            rewrite_nominal_expr(expr, replacements, type_table);
        }
        TypedExprKind::FmtString(parts) => {
            for part in parts {
                if let TypedFmtStringPart::Expr(expr) = part {
                    rewrite_nominal_expr(expr, replacements, type_table);
                }
            }
        }
        TypedExprKind::Int(_)
        | TypedExprKind::Float(_)
        | TypedExprKind::Bool(_)
        | TypedExprKind::String(_)
        | TypedExprKind::Unit
        | TypedExprKind::Null
        | TypedExprKind::AssociatedConst { .. }
        | TypedExprKind::Identifier(_) => {}
    }
}

fn rewrite_nominal_pattern(
    pattern: &mut TypedPattern,
    replacements: &HashMap<String, String>,
    type_table: &crate::types::TypeTable,
) {
    pattern.ty = replace_nominal_type(&pattern.ty, replacements);
    let type_name = nominal_type_name(&pattern.ty);
    match &mut pattern.kind {
        TypedPatternKind::Variant {
            path,
            enum_schema_index,
            enum_variant_index,
            fields,
            ..
        } => {
            if let Some(type_name) = type_name {
                if let Some(first) = path.first_mut() {
                    *first = type_name.clone();
                }
                *enum_schema_index = type_table.enum_schema_index(&type_name);
                *enum_variant_index = path
                    .last()
                    .and_then(|variant| {
                        type_table.get_enum(&type_name).and_then(|definition| {
                            definition
                                .variants
                                .iter()
                                .position(|candidate| candidate.name == *variant)
                        })
                    })
                    .and_then(|index| u16::try_from(index).ok());
            }
            for field in fields {
                rewrite_nominal_pattern(field, replacements, type_table);
            }
        }
        TypedPatternKind::Struct {
            name,
            schema_index,
            fields,
            ..
        } => {
            if let Some(type_name) = type_name {
                *name = type_name.clone();
                if let Some(index) = nominal_schema_index(type_table, &type_name) {
                    *schema_index = index;
                }
            }
            for (_, field, _) in fields {
                rewrite_nominal_pattern(field, replacements, type_table);
            }
        }
        TypedPatternKind::Or(fields) => {
            for field in fields {
                rewrite_nominal_pattern(field, replacements, type_table);
            }
        }
        TypedPatternKind::Wildcard
        | TypedPatternKind::Binding(_)
        | TypedPatternKind::Int(_)
        | TypedPatternKind::String(_)
        | TypedPatternKind::Bool(_) => {}
    }
}

fn nominal_type_name(ty: &InferType) -> Option<String> {
    match ty {
        InferType::Struct(name) => Some(name.clone()),
        _ => None,
    }
}

fn refresh_object_schema_index(
    object: &TypedExpr,
    schema_index: &mut u16,
    type_table: &crate::types::TypeTable,
) {
    if let Some(type_name) = nominal_type_name(&object.ty)
        && let Some(index) = nominal_schema_index(type_table, &type_name)
    {
        *schema_index = index;
    }
}

fn nominal_schema_index(type_table: &crate::types::TypeTable, name: &str) -> Option<u16> {
    type_table
        .schema_index(name)
        .or_else(|| type_table.enum_schema_index(name))
}

pub(super) fn collect_stmt_types(stmt: &TypedStmt, types: &mut Vec<(InferType, Span)>) {
    match &stmt.kind {
        TypedStmtKind::Expression(expr) => collect_expr_types(expr, types),
        TypedStmtKind::Let {
            initializer,
            var_type,
            ..
        } => {
            collect_type(var_type, stmt.span, types);
            collect_expr_types(initializer, types);
        }
        TypedStmtKind::Block(stmts) => {
            for stmt in stmts {
                collect_stmt_types(stmt, types);
            }
        }
        TypedStmtKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_expr_types(condition, types);
            collect_stmt_types(then_branch, types);
            if let Some(else_branch) = else_branch {
                collect_stmt_types(else_branch, types);
            }
        }
        TypedStmtKind::While { condition, body } => {
            collect_expr_types(condition, types);
            collect_stmt_types(body, types);
        }
        TypedStmtKind::For {
            start,
            end,
            step,
            body,
            ..
        } => {
            collect_expr_types(start, types);
            collect_expr_types(end, types);
            if let Some(step) = step.as_ref() {
                collect_expr_types(step, types);
            }
            collect_stmt_types(body, types);
        }
        TypedStmtKind::ForEach {
            iterable,
            elem_type,
            body,
            ..
        } => {
            collect_type(elem_type, stmt.span, types);
            collect_expr_types(iterable, types);
            collect_stmt_types(body, types);
        }
        TypedStmtKind::Return(expr) => {
            if let Some(expr) = expr {
                collect_expr_types(expr, types);
            }
        }
        TypedStmtKind::Function(function) => collect_function_types(function, types),
        TypedStmtKind::ImplDecl { methods, .. } => {
            for method in methods {
                collect_function_types(method, types);
            }
        }
        TypedStmtKind::StructDecl { fields, .. } => {
            for (_, ty) in fields {
                collect_type(ty, stmt.span, types);
            }
        }
        TypedStmtKind::EnumDecl { variants, .. } => {
            for variant in variants {
                match &variant.fields {
                    crate::types::EnumVariantFieldsDef::Unit => {}
                    crate::types::EnumVariantFieldsDef::Tuple(fields) => {
                        for field in fields {
                            collect_type(field, stmt.span, types);
                        }
                    }
                    crate::types::EnumVariantFieldsDef::Named(fields) => {
                        for field in fields {
                            collect_type(&field.ty, stmt.span, types);
                        }
                    }
                }
            }
        }
        TypedStmtKind::Break
        | TypedStmtKind::Continue
        | TypedStmtKind::TraitDecl { .. }
        | TypedStmtKind::Needs(_) => {}
    }
}

fn collect_function_types(function: &TypedFunction, types: &mut Vec<(InferType, Span)>) {
    for param in &function.params {
        collect_type(&param.ty, param.span, types);
    }
    collect_type(&function.return_type, function.span, types);
    for (_, ty) in &function.captures {
        collect_type(ty, function.span, types);
    }
    for stmt in &function.body {
        collect_stmt_types(stmt, types);
    }
}

fn collect_expr_types(expr: &TypedExpr, types: &mut Vec<(InferType, Span)>) {
    collect_type(&expr.ty, expr.span, types);
    match &expr.kind {
        TypedExprKind::Binary { left, right, .. }
        | TypedExprKind::And { left, right }
        | TypedExprKind::Or { left, right } => {
            collect_expr_types(left, types);
            collect_expr_types(right, types);
        }
        TypedExprKind::Unary { operand, .. }
        | TypedExprKind::Grouping(operand)
        | TypedExprKind::Lambda(operand)
        | TypedExprKind::Try { operand, .. } => collect_expr_types(operand, types),
        TypedExprKind::Call { callee, args } => {
            if !matches!(
                &callee.ty,
                InferType::Function { .. } if callee.ty.contains_dynamic()
            ) {
                collect_expr_types(callee, types);
            }
            for arg in args {
                collect_expr_types(arg, types);
            }
        }
        TypedExprKind::Assign { value, .. } => collect_expr_types(value, types),
        TypedExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_expr_types(condition, types);
            collect_expr_types(then_branch, types);
            collect_expr_types(else_branch, types);
        }
        TypedExprKind::Match { scrutinee, arms } => {
            collect_expr_types(scrutinee, types);
            for arm in arms {
                collect_pattern_types(&arm.pattern, types);
                if let Some(guard) = &arm.guard {
                    collect_expr_types(guard, types);
                }
                match &arm.body {
                    TypedMatchArmBody::Expr(expr) => collect_expr_types(expr, types),
                    TypedMatchArmBody::Block(stmts) => {
                        for stmt in stmts {
                            collect_stmt_types(stmt, types);
                        }
                    }
                }
            }
        }
        TypedExprKind::LambdaInner {
            params,
            return_type,
            body,
            captures,
        } => {
            for param in params {
                collect_type(&param.ty, param.span, types);
            }
            collect_type(return_type, expr.span, types);
            for (_, ty) in captures {
                collect_type(ty, expr.span, types);
            }
            for stmt in body {
                collect_stmt_types(stmt, types);
            }
        }
        TypedExprKind::Member {
            object, separator, ..
        } => {
            if *separator == aelys_syntax::MemberSeparator::Dot {
                collect_expr_types(object, types);
            }
        }
        TypedExprKind::StructField { object, .. } => collect_expr_types(object, types),
        TypedExprKind::StructMethod {
            object, separator, ..
        } => {
            if *separator == aelys_syntax::MemberSeparator::Dot {
                collect_expr_types(object, types);
            }
        }
        TypedExprKind::MemberAssign { object, value, .. } => {
            collect_expr_types(object, types);
            collect_expr_types(value, types);
        }
        TypedExprKind::ArrayLiteral {
            elements, repeat, ..
        }
        | TypedExprKind::VecLiteral {
            elements, repeat, ..
        } => {
            for element in elements {
                collect_expr_types(element, types);
            }
            if let Some(repeat) = repeat {
                collect_expr_types(repeat, types);
            }
        }
        TypedExprKind::ArraySized { size, .. } => collect_expr_types(size, types),
        TypedExprKind::Index { object, index }
        | TypedExprKind::Slice {
            object,
            range: index,
        } => {
            collect_expr_types(object, types);
            collect_expr_types(index, types);
        }
        TypedExprKind::IndexAssign {
            object,
            index,
            value,
        } => {
            collect_expr_types(object, types);
            collect_expr_types(index, types);
            collect_expr_types(value, types);
        }
        TypedExprKind::Range { start, end, .. } => {
            if let Some(start) = start {
                collect_expr_types(start, types);
            }
            if let Some(end) = end {
                collect_expr_types(end, types);
            }
        }
        TypedExprKind::StructLiteral { fields, .. } => {
            for (_, field) in fields {
                collect_expr_types(field, types);
            }
        }
        TypedExprKind::EnumConstruct { fields, .. } => {
            for (_, field) in fields {
                collect_expr_types(field, types);
            }
        }
        TypedExprKind::Cast { expr, target } => {
            collect_expr_types(expr, types);
            collect_type(target, expr.span, types);
        }
        TypedExprKind::FmtString(parts) => {
            for part in parts {
                if let TypedFmtStringPart::Expr(expr) = part {
                    collect_expr_types(expr, types);
                }
            }
        }
        TypedExprKind::Int(_)
        | TypedExprKind::Float(_)
        | TypedExprKind::Bool(_)
        | TypedExprKind::String(_)
        | TypedExprKind::Unit
        | TypedExprKind::Null
        | TypedExprKind::AssociatedConst { .. }
        | TypedExprKind::Identifier(_) => {}
    }
}

fn collect_pattern_types(pattern: &TypedPattern, types: &mut Vec<(InferType, Span)>) {
    collect_type(&pattern.ty, pattern.span, types);
    match &pattern.kind {
        TypedPatternKind::Variant { fields, .. } | TypedPatternKind::Or(fields) => {
            for field in fields {
                collect_pattern_types(field, types);
            }
        }
        TypedPatternKind::Struct { fields, .. } => {
            for (_, field, _) in fields {
                collect_pattern_types(field, types);
            }
        }
        TypedPatternKind::Wildcard
        | TypedPatternKind::Binding(_)
        | TypedPatternKind::Int(_)
        | TypedPatternKind::String(_)
        | TypedPatternKind::Bool(_) => {}
    }
}

fn collect_type(ty: &InferType, span: Span, types: &mut Vec<(InferType, Span)>) {
    types.push((ty.clone(), span));
    match ty {
        InferType::Function { params, ret } => {
            for param in params {
                collect_type(param, span, types);
            }
            collect_type(ret, span, types);
        }
        InferType::Array(inner)
        | InferType::FixedArray(inner, _)
        | InferType::Vec(inner)
        | InferType::Option(inner) => collect_type(inner, span, types),
        InferType::Result(ok, err) => {
            collect_type(ok, span, types);
            collect_type(err, span, types);
        }
        InferType::Tuple(elements) => {
            for element in elements {
                collect_type(element, span, types);
            }
        }
        InferType::Applied { args, .. } => {
            for arg in args {
                collect_type(arg, span, types);
            }
        }
        InferType::Projection { self_ty, .. } => collect_type(self_ty, span, types),
        _ => {}
    }
}

fn contains_open_generic_type(ty: &InferType) -> bool {
    match ty {
        InferType::Param(_)
        | InferType::Var(_)
        | InferType::Dynamic
        | InferType::UntypedNative(_) => true,
        InferType::Function { params, ret } => {
            params.iter().any(contains_open_generic_type) || contains_open_generic_type(ret)
        }
        InferType::Array(inner)
        | InferType::FixedArray(inner, _)
        | InferType::Vec(inner)
        | InferType::Option(inner) => contains_open_generic_type(inner),
        InferType::Result(ok, err) => {
            contains_open_generic_type(ok) || contains_open_generic_type(err)
        }
        InferType::Tuple(elements) => elements.iter().any(contains_open_generic_type),
        InferType::Applied { args, .. } => args.iter().any(contains_open_generic_type),
        InferType::Projection { self_ty, .. } => contains_open_generic_type(self_ty),
        _ => false,
    }
}

enum NominalDefinition {
    Struct(crate::types::StructDef),
    Enum(crate::types::EnumDef),
}

impl TypeInference {
    pub(super) fn monomorphize_program(&mut self, mut stmts: Vec<TypedStmt>) -> Vec<TypedStmt> {
        self.monomorphization_active.clear();
        let generic_defs = stmts
            .iter()
            .filter_map(|stmt| match &stmt.kind {
                TypedStmtKind::Function(function) if !function.type_params.is_empty() => {
                    Some((function.name.clone(), function.clone()))
                }
                _ => None,
            })
            .collect::<HashMap<_, _>>();

        let mut queue = VecDeque::new();
        let mut queued = HashSet::new();
        let mut symbols = HashMap::<InstanceKey, String>::new();
        let mut symbol_keys = HashMap::<String, InstanceKey>::new();
        let mut errors = Vec::new();

        for stmt in &mut stmts {
            if is_generic_function(stmt) {
                continue;
            }
            rewrite_stmt(
                self,
                stmt,
                &generic_defs,
                &mut queue,
                &mut queued,
                &mut symbols,
                &mut symbol_keys,
                &mut errors,
            );
        }

        if generic_defs.is_empty() {
            self.lower_display_dispatch(&mut stmts);
            let refused = self.monomorphize_impls(&mut stmts);
            self.materialize_nominals(&mut stmts);
            strip_display_markers(&mut stmts);
            self.errors.extend(errors);
            self.reject_unresolved_instance_symbols(&mut stmts, &refused);
            self.reject_unspecialized_associated_consts(&mut stmts);
            return stmts;
        }

        let mut specialized = Vec::new();
        while let Some(frame) = queue.pop_front() {
            let Some(template) = generic_defs.get(&frame.key.name).cloned() else {
                continue;
            };
            self.monomorphization_active = frame.ancestors;
            self.monomorphization_active
                .push((frame.key.name.clone(), frame.key.args.clone()));
            let instance = self.specialize_instance(
                &frame.key,
                &template,
                &generic_defs,
                &mut queue,
                &mut queued,
                &mut symbols,
                &mut symbol_keys,
                &mut errors,
            );
            // single pop point so no early exit inside the instance can leak a frame
            self.monomorphization_active.clear();
            let Some(function) = instance else {
                continue;
            };
            specialized.push(TypedStmt::new(
                TypedStmtKind::Function(function),
                template.span,
            ));
            if specialized.len() > MAX_INSTANCES {
                errors.push(TypeError {
                    kind: TypeErrorKind::MonomorphizationLimit {
                        name: frame.key.name.clone(),
                    },
                    span: template.span,
                    reason: ConstraintReason::Other("generic instance worklist".to_string()),
                });
                break;
            }
        }

        stmts.retain(|stmt| !is_generic_function(stmt));
        specialized.extend(stmts);
        stmts = specialized;
        self.lower_display_dispatch(&mut stmts);
        let refused = self.monomorphize_impls(&mut stmts);
        self.materialize_nominals(&mut stmts);
        strip_display_markers(&mut stmts);
        self.errors.extend(errors);
        self.reject_unresolved_instance_symbols(&mut stmts, &refused);
        self.reject_unspecialized_associated_consts(&mut stmts);
        stmts
    }

    fn lower_display_dispatch(&mut self, stmts: &mut [TypedStmt]) {
        let type_table = &self.type_table;
        for stmt in stmts.iter_mut() {
            visit_exprs_stmt(stmt, &mut |expr| resolve_display_marker(expr, type_table));
        }
    }

    pub(super) fn resolve_deferred_members(&mut self, stmts: &mut [TypedStmt]) {
        let type_table = &self.type_table;
        let mut errors = Vec::new();
        for stmt in stmts.iter_mut() {
            visit_exprs_stmt(stmt, &mut |expr| {
                resolve_deferred_member(expr, type_table, &mut errors);
            });
            visit_exprs_stmt(stmt, &mut |expr| {
                resolve_deferred_call_type(expr);
            });
            resolve_deferred_statement_types(stmt);
        }
        self.errors.extend(errors);
    }

    // a method carrying its own type parameters is never specialized, so a
    fn reject_unspecialized_associated_consts(&mut self, stmts: &mut [TypedStmt]) {
        let mut refused = Vec::new();
        for stmt in stmts.iter_mut() {
            visit_exprs_stmt(stmt, &mut |expr| {
                let TypedExprKind::AssociatedConst { param, item, .. } = &expr.kind else {
                    return;
                };
                refused.push((param.clone(), item.clone(), expr.span));
            });
        }
        for (param, item, span) in refused {
            self.errors.push(TypeError {
                kind: TypeErrorKind::AmbiguousAssociatedProjection {
                    receiver: param,
                    item,
                    cause: crate::constraint::ProjectionFailure::UnspecializedGenericMethod,
                },
                span,
                reason: ConstraintReason::Other("a value expression".to_string()),
            });
        }
    }

    // a generated symbol with no definition must fail here, never as a runtime undefined variable
    fn reject_unresolved_instance_symbols(
        &mut self,
        stmts: &mut [TypedStmt],
        refused: &HashSet<(usize, usize)>,
    ) {
        let mut defined = HashSet::new();
        for stmt in stmts.iter() {
            collect_defined_symbols(stmt, &mut defined);
        }
        let mut missing = Vec::new();
        for stmt in stmts.iter_mut() {
            visit_exprs_stmt(stmt, &mut |expr| {
                let symbol = match &expr.kind {
                    TypedExprKind::StructMethod { symbol, .. } => symbol,
                    TypedExprKind::Identifier(name) => name,
                    TypedExprKind::Try {
                        conversion: Some(symbol),
                        ..
                    } => symbol,
                    _ => return,
                };
                if !is_generated_instance_symbol(symbol) || defined.contains(symbol) {
                    return;
                }
                if refused.contains(&(expr.span.start, expr.span.end)) {
                    return;
                }
                missing.push((symbol.clone(), expr.span));
            });
        }
        for (symbol, span) in missing {
            self.errors.push(TypeError {
                kind: TypeErrorKind::UnresolvedInstanceSymbol {
                    name: user_facing_symbol(&symbol),
                },
                span,
                reason: ConstraintReason::Other("generic instance resolution".to_string()),
            });
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn specialize_instance(
        &mut self,
        key: &InstanceKey,
        template: &TypedFunction,
        generic_defs: &HashMap<String, TypedFunction>,
        queue: &mut VecDeque<InstanceFrame>,
        queued: &mut HashSet<InstanceKey>,
        symbols: &mut HashMap<InstanceKey, String>,
        symbol_keys: &mut HashMap<String, InstanceKey>,
        errors: &mut Vec<TypeError>,
    ) -> Option<TypedFunction> {
        let symbol = symbols.get(key).cloned()?;
        let mut substitution = Substitution::new();
        for (name, ty) in template.type_params.iter().zip(&key.args) {
            substitution.bind_param(name.clone(), ty.clone());
        }
        let mut function = self.apply_substitution_func(template, &substitution);
        function.name = symbol;
        function.type_params.clear();
        {
            let constants = self.associated_const_resolutions();
            let substitution = &substitution;
            for stmt in &mut function.body {
                visit_exprs_stmt(stmt, &mut |expr| {
                    resolve_associated_const_node(expr, substitution, &constants, errors);
                });
            }
        }
        {
            let type_table = &self.type_table;
            let gate = &self.impls_missing_supertraits;
            let mut marker_errors = Vec::new();
            for stmt in &mut function.body {
                visit_exprs_stmt(stmt, &mut |expr| {
                    resolve_bound_marker(expr, type_table, gate, &mut marker_errors);
                });
            }
            errors.append(&mut marker_errors);
        }
        for stmt in &mut function.body {
            rewrite_stmt(
                self,
                stmt,
                generic_defs,
                queue,
                queued,
                symbols,
                symbol_keys,
                errors,
            );
        }
        let mut function_types = Vec::new();
        collect_function_types(&function, &mut function_types);
        if function_types
            .iter()
            .any(|(ty, _)| contains_open_generic_type(ty))
        {
            errors.push(TypeError {
                kind: TypeErrorKind::UnresolvedGenericType {
                    name: key.name.clone(),
                },
                span: template.span,
                reason: ConstraintReason::Other(
                    "specialized function still contains an open type".to_string(),
                ),
            });
            return None;
        }
        Some(function)
    }

    fn monomorphize_impls(&mut self, stmts: &mut Vec<TypedStmt>) -> HashSet<(usize, usize)> {
        let templates = stmts
            .iter()
            .filter_map(|stmt| match &stmt.kind {
                TypedStmtKind::ImplDecl {
                    type_params,
                    target_type,
                    trait_args,
                    trait_name,
                    target,
                    methods,
                } if is_impl_template(type_params, methods) => Some(GenericImplTemplate {
                    target: target.clone(),
                    trait_name: trait_name.clone(),
                    type_params: type_params.clone(),
                    target_type: target_type.clone(),
                    trait_args: trait_args.clone(),
                    methods: methods.clone(),
                    span: stmt.span,
                }),
                _ => None,
            })
            .collect::<Vec<_>>();
        let mut refused = HashSet::new();
        if templates.is_empty() {
            return refused;
        }
        let mut template_indices_by_method = HashMap::<String, Vec<usize>>::new();
        for (template_index, template) in templates.iter().enumerate() {
            for method in &template.methods {
                template_indices_by_method
                    .entry(method.name.clone())
                    .or_default()
                    .push(template_index);
            }
        }

        let mut calls = Vec::new();
        for stmt in stmts.iter() {
            let first = calls.len();
            collect_impl_calls_stmt(stmt, &mut calls);
            if matches!(
                &stmt.kind,
                TypedStmtKind::ImplDecl { type_params, methods, .. }
                    if is_impl_template(type_params, methods)
            ) {
                for call in &mut calls[first..] {
                    if call_carries_open_type(call) {
                        call.reportable = false;
                    }
                }
            }
        }
        let mut seen = HashSet::<ImplInstanceKey>::new();
        let mut impl_instances = HashMap::<(usize, Vec<InferType>), usize>::new();
        let mut next_call = 0;
        let mut replacements = HashMap::<String, Vec<ImplInstanceMethod>>::new();
        let mut specialized_by_template = vec![Vec::new(); templates.len()];
        let mut marker_errors = Vec::new();
        let mut limit_error = None;
        let mut deduction_errors = Vec::new();
        let mut collision_errors = Vec::new();
        let mut instance_symbol_keys = HashMap::<String, ImplInstanceKey>::new();
        let associated_constants = self.associated_const_resolutions();
        'impl_work: while next_call < calls.len() {
            let mut discovered = Vec::<(ImplInstanceKey, Vec<(String, Vec<InferType>)>)>::new();
            while next_call < calls.len() {
                let call_index = next_call;
                next_call += 1;
                let mut resolved = false;
                let mut open_param = None;
                for &template_index in template_indices_by_method
                    .get(&calls[call_index].symbol)
                    .into_iter()
                    .flatten()
                {
                    let template = &templates[template_index];
                    let deduction = match deduce_impl_arguments(template, &calls[call_index]) {
                        ImplDeductionOutcome::Resolved(deduction) => deduction,
                        ImplDeductionOutcome::OpenMethodParam(param) => {
                            open_param.get_or_insert(param);
                            continue;
                        }
                        ImplDeductionOutcome::NoMatch => continue,
                    };
                    resolved = true;
                    let Some(method_index) = template
                        .methods
                        .iter()
                        .position(|method| method.name == calls[call_index].symbol)
                    else {
                        continue;
                    };
                    let key = ImplInstanceKey {
                        template_index,
                        impl_args: deduction.impl_args,
                        method_index,
                        method_args: deduction.method_args,
                    };
                    if seen.contains(&key) {
                        continue;
                    }
                    let call = &calls[call_index];
                    if let Some(error) = recursion_error(template, &key, call) {
                        refused.insert((call.callee_span.start, call.callee_span.end));
                        deduction_errors.push(error);
                        continue;
                    }
                    if call.ancestors.len() >= MAX_ACTIVE_INSTANCES || seen.len() >= MAX_INSTANCES {
                        limit_error = Some(TypeError {
                            kind: TypeErrorKind::MonomorphizationLimit {
                                name: template.target.clone(),
                            },
                            span: template.span,
                            reason: ConstraintReason::Other(
                                "generic impl instance worklist".to_string(),
                            ),
                        });
                        break 'impl_work;
                    }
                    seen.insert(key.clone());
                    discovered.push((key, call.ancestors.clone()));
                }
                if resolved {
                    continue;
                }
                if let Some(param) = open_param
                    && calls[call_index].reportable
                {
                    let callee_span = calls[call_index].callee_span;
                    refused.insert((callee_span.start, callee_span.end));
                    deduction_errors.push(TypeError {
                        kind: TypeErrorKind::UnresolvedGenericType { name: param },
                        span: calls[call_index].span,
                        reason: ConstraintReason::Other(
                            "impl method call has no concrete instance".to_string(),
                        ),
                    });
                }
            }

            for (key, ancestors) in discovered {
                let template = &templates[key.template_index];
                let mut substitution = Substitution::new();
                for (param, arg) in template.type_params.iter().zip(&key.impl_args) {
                    substitution.bind_param(param.clone(), arg.clone());
                }
                let concrete_target = substitution.apply(&template.target_type);
                let impl_suffix = match template.type_params.is_empty() {
                    true => None,
                    false => Some(hex_encoded(&format_type_application(
                        &template.target,
                        std::slice::from_ref(&concrete_target),
                    ))),
                };
                let position =
                    match impl_instances.get(&(key.template_index, key.impl_args.clone())) {
                        Some(position) => *position,
                        None => {
                            let concrete_trait_args = template
                                .trait_args
                                .iter()
                                .map(|arg| substitution.apply(arg))
                                .collect::<Vec<_>>();
                            let position = specialized_by_template[key.template_index].len();
                            specialized_by_template[key.template_index].push(TypedStmt::new(
                                TypedStmtKind::ImplDecl {
                                    target: template.target.clone(),
                                    trait_name: template.trait_name.clone(),
                                    type_params: Vec::new(),
                                    target_type: concrete_target.clone(),
                                    trait_args: concrete_trait_args,
                                    methods: Vec::new(),
                                },
                                aelys_syntax::Span::dummy(),
                            ));
                            impl_instances
                                .insert((key.template_index, key.impl_args.clone()), position);
                            for (method_index, method) in template.methods.iter().enumerate() {
                                if !method.own_type_params.is_empty() {
                                    continue;
                                }
                                let plain = ImplInstanceKey {
                                    template_index: key.template_index,
                                    impl_args: key.impl_args.clone(),
                                    method_index,
                                    method_args: Vec::new(),
                                };
                                seen.insert(plain.clone());
                                self.emit_impl_instance_method(
                                    ImplInstanceEmission {
                                        template,
                                        key: &plain,
                                        ancestors: &ancestors,
                                        substitution: &substitution,
                                        concrete_target: &concrete_target,
                                        impl_suffix: impl_suffix.as_deref(),
                                        position,
                                    },
                                    &associated_constants,
                                    &mut specialized_by_template[key.template_index],
                                    &mut replacements,
                                    &mut instance_symbol_keys,
                                    &mut collision_errors,
                                    &mut marker_errors,
                                    &mut calls,
                                );
                            }
                            position
                        }
                    };
                if key.method_args.is_empty() {
                    continue;
                }
                let method = &template.methods[key.method_index];
                let mut substitution = substitution.clone();
                for (param, arg) in method.own_type_params.iter().zip(&key.method_args) {
                    substitution.bind_param(param.clone(), arg.clone());
                }
                self.emit_impl_instance_method(
                    ImplInstanceEmission {
                        template,
                        key: &key,
                        ancestors: &ancestors,
                        substitution: &substitution,
                        concrete_target: &concrete_target,
                        impl_suffix: impl_suffix.as_deref(),
                        position,
                    },
                    &associated_constants,
                    &mut specialized_by_template[key.template_index],
                    &mut replacements,
                    &mut instance_symbol_keys,
                    &mut collision_errors,
                    &mut marker_errors,
                    &mut calls,
                );
            }
        }
        self.errors.append(&mut collision_errors);
        if let Some(error) = limit_error {
            self.errors.push(error);
        }
        self.errors.append(&mut deduction_errors);
        self.errors.append(&mut marker_errors);

        let rewrite = ImplCallRewrite {
            templates: &templates,
            template_indices_by_method: &template_indices_by_method,
            replacements: &replacements,
        };
        for stmt in stmts.iter_mut() {
            rewrite_impl_call_symbols_stmt(stmt, &rewrite);
        }
        for group in specialized_by_template.iter_mut() {
            for stmt in group.iter_mut() {
                rewrite_impl_call_symbols_stmt(stmt, &rewrite);
            }
        }
        let original = std::mem::take(stmts);
        let mut rewritten = Vec::with_capacity(original.len());
        let mut template_index = 0;
        for stmt in original {
            if matches!(
                &stmt.kind,
                TypedStmtKind::ImplDecl { type_params, methods, .. }
                    if is_impl_template(type_params, methods)
            ) {
                rewritten.append(&mut specialized_by_template[template_index]);
                template_index += 1;
            } else {
                rewritten.push(stmt);
            }
        }
        *stmts = rewritten;
        refused
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_impl_instance_method(
        &self,
        emission: ImplInstanceEmission<'_>,
        associated_constants: &HashMap<(String, String), crate::infer::ConstResolution>,
        instances: &mut [TypedStmt],
        replacements: &mut HashMap<String, Vec<ImplInstanceMethod>>,
        instance_symbol_keys: &mut HashMap<String, ImplInstanceKey>,
        collision_errors: &mut Vec<TypeError>,
        marker_errors: &mut Vec<TypeError>,
        calls: &mut Vec<ImplCallSite>,
    ) {
        let template = emission.template;
        let key = emission.key;
        let source = &template.methods[key.method_index];
        let mut method = self.apply_substitution_func(source, emission.substitution);
        let original = source.name.clone();
        method.type_params.clear();
        method.own_type_params.clear();
        let mut symbol = original.clone();
        if let Some(suffix) = emission.impl_suffix {
            symbol.push_str(IMPL_INSTANCE_INFIX);
            symbol.push_str(suffix);
        }
        if !key.method_args.is_empty() {
            symbol.push_str(IMPL_INSTANCE_INFIX);
            symbol.push_str(&hex_encoded(&format_type_application("", &key.method_args)));
        }
        method.name = symbol;
        if let Some(previous) = instance_symbol_keys.get(&method.name)
            && previous != key
        {
            collision_errors.push(TypeError {
                kind: TypeErrorKind::MangledSymbolCollision {
                    name: template.target.clone(),
                },
                span: template.span,
                reason: ConstraintReason::Other(
                    "generic impl instance symbol collision".to_string(),
                ),
            });
        }
        instance_symbol_keys.insert(method.name.clone(), key.clone());
        replacements
            .entry(original.clone())
            .or_default()
            .push(ImplInstanceMethod {
                target: emission.concrete_target.clone(),
                args: key.impl_args.clone(),
                method_args: key.method_args.clone(),
                symbol: method.name.clone(),
            });
        {
            let type_table = &self.type_table;
            let gate = &self.impls_missing_supertraits;
            let substitution = emission.substitution;
            for stmt in &mut method.body {
                visit_exprs_stmt(stmt, &mut |expr| {
                    resolve_associated_const_node(
                        expr,
                        substitution,
                        associated_constants,
                        marker_errors,
                    );
                    resolve_bound_marker(expr, type_table, gate, marker_errors);
                });
            }
            // resolving a display marker drops the node the bound marker lives on,
            for stmt in &mut method.body {
                visit_exprs_stmt(stmt, &mut |expr| {
                    resolve_display_marker(expr, type_table);
                });
            }
        }
        let mut frame_args = key.impl_args.clone();
        frame_args.extend(key.method_args.iter().cloned());
        let mut ancestors = emission.ancestors.to_vec();
        ancestors.push((original, frame_args));
        let first = calls.len();
        for stmt in &method.body {
            collect_impl_calls_stmt(stmt, calls);
        }
        for call in &mut calls[first..] {
            call.ancestors = ancestors.clone();
        }
        let TypedStmtKind::ImplDecl { methods, .. } = &mut instances[emission.position].kind else {
            return;
        };
        methods.push(method);
    }

    fn materialize_nominals(&mut self, stmts: &mut [TypedStmt]) {
        let mut types = Vec::new();
        for stmt in stmts.iter() {
            collect_stmt_types(stmt, &mut types);
        }

        let mut applications = HashMap::<String, (String, Vec<InferType>)>::new();
        for (ty, _) in types {
            collect_nominal_applications(&ty, &self.type_table, &mut applications);
        }
        loop {
            let before = applications.len();
            let discovered = applications.values().cloned().collect::<Vec<_>>();
            for (name, args) in discovered {
                let substitutions = self.type_table.get_struct(&name).map(|definition| {
                    definition
                        .type_params
                        .iter()
                        .cloned()
                        .zip(args.iter().cloned())
                        .collect::<HashMap<_, _>>()
                });
                if let Some(definition) = self.type_table.get_struct(&name)
                    && let Some(substitutions) = &substitutions
                {
                    for field in &definition.fields {
                        let field_type = field.ty.substitute_params(substitutions);
                        collect_nominal_applications(
                            &field_type,
                            &self.type_table,
                            &mut applications,
                        );
                    }
                }
                let substitutions = self.type_table.get_enum(&name).map(|definition| {
                    definition
                        .type_params
                        .iter()
                        .cloned()
                        .zip(args.iter().cloned())
                        .collect::<HashMap<_, _>>()
                });
                if let Some(definition) = self.type_table.get_enum(&name)
                    && let Some(substitutions) = &substitutions
                {
                    for variant in &definition.variants {
                        match &variant.fields {
                            crate::types::EnumVariantFieldsDef::Unit => {}
                            crate::types::EnumVariantFieldsDef::Tuple(fields) => {
                                for field in fields {
                                    let field_type = field.substitute_params(substitutions);
                                    collect_nominal_applications(
                                        &field_type,
                                        &self.type_table,
                                        &mut applications,
                                    );
                                }
                            }
                            crate::types::EnumVariantFieldsDef::Named(fields) => {
                                for field in fields {
                                    let field_type = field.ty.substitute_params(substitutions);
                                    collect_nominal_applications(
                                        &field_type,
                                        &self.type_table,
                                        &mut applications,
                                    );
                                }
                            }
                        }
                    }
                }
            }
            if applications.len() == before {
                break;
            }
        }
        if applications.is_empty() {
            self.type_table.remove_open_nominals();
            let replacements = HashMap::new();
            for stmt in stmts {
                rewrite_nominal_stmt(stmt, &replacements, &self.type_table);
            }
            return;
        }

        let mut replacements = HashMap::new();
        for (key, (name, args)) in &applications {
            let generated = nominal_instance_name(name, args);
            replacements.insert(key.clone(), generated.clone());
            self.type_table
                .register_nominal_instance_args(generated.clone(), args.clone());
            self.type_table
                .register_nominal_instance_origin(generated, name.clone());
        }

        let definitions = applications
            .values()
            .filter_map(|(name, args)| {
                let generated = replacements.get(&format_type_application(name, args))?;
                if let Some(definition) = self.type_table.get_struct(name).cloned()
                    && !self.type_table.has_struct(generated)
                {
                    let parameter_substitutions = definition
                        .type_params
                        .iter()
                        .cloned()
                        .zip(args.iter().cloned())
                        .collect::<HashMap<_, _>>();
                    let fields = definition
                        .fields
                        .iter()
                        .map(|field| crate::types::StructField {
                            name: field.name.clone(),
                            ty: replace_nominal_type(
                                &field.ty.substitute_params(&parameter_substitutions),
                                &replacements,
                            ),
                            is_pub: field.is_pub,
                            ordinal: field.ordinal,
                            span: field.span,
                        })
                        .collect();
                    return Some((
                        generated.clone(),
                        NominalDefinition::Struct(crate::types::StructDef {
                            name: generated.clone(),
                            type_params: Vec::new(),
                            fields,
                            owner: definition.owner.clone(),
                            is_pub: definition.is_pub,
                        }),
                    ));
                }
                if let Some(definition) = self.type_table.get_enum(name).cloned()
                    && !self.type_table.has_enum(generated)
                {
                    let parameter_substitutions = definition
                        .type_params
                        .iter()
                        .cloned()
                        .zip(args.iter().cloned())
                        .collect::<HashMap<_, _>>();
                    let variants = definition
                        .variants
                        .iter()
                        .map(|variant| crate::types::EnumVariantDef {
                            name: variant.name.clone(),
                            span: variant.span,
                            fields: match &variant.fields {
                                crate::types::EnumVariantFieldsDef::Unit => {
                                    crate::types::EnumVariantFieldsDef::Unit
                                }
                                crate::types::EnumVariantFieldsDef::Tuple(fields) => {
                                    crate::types::EnumVariantFieldsDef::Tuple(
                                        fields
                                            .iter()
                                            .map(|field| {
                                                replace_nominal_type(
                                                    &field.substitute_params(
                                                        &parameter_substitutions,
                                                    ),
                                                    &replacements,
                                                )
                                            })
                                            .collect(),
                                    )
                                }
                                crate::types::EnumVariantFieldsDef::Named(fields) => {
                                    crate::types::EnumVariantFieldsDef::Named(
                                        fields
                                            .iter()
                                            .map(|field| crate::types::StructField {
                                                name: field.name.clone(),
                                                ty: replace_nominal_type(
                                                    &field.ty.substitute_params(
                                                        &parameter_substitutions,
                                                    ),
                                                    &replacements,
                                                ),
                                                is_pub: field.is_pub,
                                                ordinal: field.ordinal,
                                                span: field.span,
                                            })
                                            .collect(),
                                    )
                                }
                            },
                        })
                        .collect();
                    return Some((
                        generated.clone(),
                        NominalDefinition::Enum(crate::types::EnumDef {
                            name: generated.clone(),
                            type_params: Vec::new(),
                            variants,
                            owner: definition.owner.clone(),
                            is_pub: definition.is_pub,
                        }),
                    ));
                }
                None
            })
            .collect::<Vec<_>>();

        for (_, definition) in definitions {
            match definition {
                NominalDefinition::Struct(definition) => {
                    self.type_table.register_struct(definition)
                }
                NominalDefinition::Enum(definition) => self.type_table.register_enum(definition),
            }
        }
        self.type_table.remove_open_nominals();
        for stmt in stmts {
            rewrite_nominal_stmt(stmt, &replacements, &self.type_table);
        }
    }
}

fn collect_defined_symbols(stmt: &TypedStmt, defined: &mut HashSet<String>) {
    match &stmt.kind {
        TypedStmtKind::Function(function) => {
            defined.insert(function.name.clone());
            for stmt in &function.body {
                collect_defined_symbols(stmt, defined);
            }
        }
        TypedStmtKind::ImplDecl { methods, .. } => {
            for method in methods {
                defined.insert(method.name.clone());
                for stmt in &method.body {
                    collect_defined_symbols(stmt, defined);
                }
            }
        }
        TypedStmtKind::Block(stmts) => {
            for stmt in stmts {
                collect_defined_symbols(stmt, defined);
            }
        }
        TypedStmtKind::If {
            then_branch,
            else_branch,
            ..
        } => {
            collect_defined_symbols(then_branch, defined);
            if let Some(else_branch) = else_branch {
                collect_defined_symbols(else_branch, defined);
            }
        }
        TypedStmtKind::While { body, .. }
        | TypedStmtKind::For { body, .. }
        | TypedStmtKind::ForEach { body, .. } => collect_defined_symbols(body, defined),
        _ => {}
    }
}

fn is_generic_function(stmt: &TypedStmt) -> bool {
    matches!(&stmt.kind, TypedStmtKind::Function(function) if !function.type_params.is_empty())
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct ImplInstanceKey {
    template_index: usize,
    impl_args: Vec<InferType>,
    method_index: usize,
    method_args: Vec<InferType>,
}

struct ImplInstanceEmission<'a> {
    template: &'a GenericImplTemplate,
    key: &'a ImplInstanceKey,
    ancestors: &'a [(String, Vec<InferType>)],
    substitution: &'a Substitution,
    concrete_target: &'a InferType,
    impl_suffix: Option<&'a str>,
    position: usize,
}

fn hex_encoded(canonical: &str) -> String {
    let mut encoded = String::with_capacity(canonical.len() * 2);
    for byte in canonical.bytes() {
        use std::fmt::Write;
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

fn recursion_error(
    template: &GenericImplTemplate,
    key: &ImplInstanceKey,
    call: &ImplCallSite,
) -> Option<TypeError> {
    let mut frame_args = key.impl_args.clone();
    frame_args.extend(key.method_args.iter().cloned());
    let name = &template.methods[key.method_index].name;
    let depth = call.ancestors.iter().rev().position(|(frame, args)| {
        frame == name && *args != frame_args && !is_strictly_decreasing(&frame_args, args)
    })?;
    let reason = match depth {
        0 => "generic recursion changes the active type instance",
        _ => "mutually recursive generic instances do not decrease",
    };
    Some(TypeError {
        kind: TypeErrorKind::RecursiveMonomorphization {
            name: template.target.clone(),
        },
        span: call.span,
        reason: ConstraintReason::Other(reason.to_string()),
    })
}

#[derive(Clone)]
struct GenericImplTemplate {
    target: String,
    trait_name: Option<String>,
    type_params: Vec<String>,
    target_type: InferType,
    trait_args: Vec<InferType>,
    methods: Vec<TypedFunction>,
    span: Span,
}

// a `::` call has no receiver: its object is typed poison, so the impl
struct ImplCallSite {
    symbol: String,
    receiver: Option<InferType>,
    actuals: Vec<InferType>,
    result: InferType,
    span: Span,
    callee_span: Span,
    ancestors: Vec<(String, Vec<InferType>)>,
    // parameter it leaves unbound is not that call site's fault
    reportable: bool,
}

struct ImplDeduction {
    impl_args: Vec<InferType>,
    method_args: Vec<InferType>,
}

enum ImplDeductionOutcome {
    Resolved(ImplDeduction),
    OpenMethodParam(String),
    NoMatch,
}

// only a call that still holds an open type is waiting for an instantiation that
fn call_carries_open_type(call: &ImplCallSite) -> bool {
    call.receiver
        .iter()
        .chain(call.actuals.iter())
        .chain(std::iter::once(&call.result))
        .any(contains_open_generic_type)
}

fn is_impl_template(type_params: &[String], methods: &[TypedFunction]) -> bool {
    !type_params.is_empty()
        || methods
            .iter()
            .any(|method| !method.own_type_params.is_empty())
}

fn call_formals<'a>(method: &'a TypedFunction, call: &ImplCallSite) -> &'a [TypedParam] {
    let receiver_is_separate = call.receiver.is_some()
        && method
            .params
            .first()
            .is_some_and(|param| param.name == "self");
    match receiver_is_separate {
        true => &method.params[1..],
        false => &method.params,
    }
}

fn deduce_impl_arguments(
    template: &GenericImplTemplate,
    call: &ImplCallSite,
) -> ImplDeductionOutcome {
    let Some(method) = template
        .methods
        .iter()
        .find(|method| method.name == call.symbol)
    else {
        return ImplDeductionOutcome::NoMatch;
    };
    let mut mapping = HashMap::new();
    match &call.receiver {
        Some(receiver) => {
            if match_types(&template.target_type, receiver, &mut mapping).is_none() {
                return ImplDeductionOutcome::NoMatch;
            }
        }
        None => {
            for (formal, actual) in call_formals(method, call).iter().zip(&call.actuals) {
                if match_types(&formal.ty, actual, &mut mapping).is_none() {
                    return ImplDeductionOutcome::NoMatch;
                }
            }
            if match_types(&method.return_type, &call.result, &mut mapping).is_none() {
                return ImplDeductionOutcome::NoMatch;
            }
        }
    }
    let Some(impl_args) = template
        .type_params
        .iter()
        .map(|param| mapping.get(param).cloned())
        .collect::<Option<Vec<_>>>()
    else {
        return ImplDeductionOutcome::NoMatch;
    };
    if !impl_args.iter().all(|arg| arg.is_concrete()) {
        return ImplDeductionOutcome::NoMatch;
    }
    if method.own_type_params.is_empty() {
        return ImplDeductionOutcome::Resolved(ImplDeduction {
            impl_args,
            method_args: Vec::new(),
        });
    }
    // the impl half is already bound, so a formal the receiver contradicts is
    for (formal, actual) in call_formals(method, call).iter().zip(&call.actuals) {
        let snapshot = mapping.clone();
        if match_types(&formal.ty, actual, &mut mapping).is_none() {
            mapping = snapshot;
        }
    }
    let snapshot = mapping.clone();
    if match_types(&method.return_type, &call.result, &mut mapping).is_none() {
        mapping = snapshot;
    }
    let mut method_args = Vec::with_capacity(method.own_type_params.len());
    for param in &method.own_type_params {
        match mapping.get(param) {
            Some(ty) if ty.is_concrete() => method_args.push(ty.clone()),
            _ => return ImplDeductionOutcome::OpenMethodParam(param.clone()),
        }
    }
    ImplDeductionOutcome::Resolved(ImplDeduction {
        impl_args,
        method_args,
    })
}

fn collect_impl_calls_stmt(stmt: &TypedStmt, calls: &mut Vec<ImplCallSite>) {
    match &stmt.kind {
        TypedStmtKind::Expression(expr) => collect_impl_calls_expr(expr, calls),
        TypedStmtKind::Let { initializer, .. } => collect_impl_calls_expr(initializer, calls),
        TypedStmtKind::Block(stmts) => {
            for stmt in stmts {
                collect_impl_calls_stmt(stmt, calls);
            }
        }
        TypedStmtKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_impl_calls_expr(condition, calls);
            collect_impl_calls_stmt(then_branch, calls);
            if let Some(else_branch) = else_branch {
                collect_impl_calls_stmt(else_branch, calls);
            }
        }
        TypedStmtKind::While { condition, body } => {
            collect_impl_calls_expr(condition, calls);
            collect_impl_calls_stmt(body, calls);
        }
        TypedStmtKind::For {
            start,
            end,
            step,
            body,
            ..
        } => {
            collect_impl_calls_expr(start, calls);
            collect_impl_calls_expr(end, calls);
            if let Some(step) = step.as_ref() {
                collect_impl_calls_expr(step, calls);
            }
            collect_impl_calls_stmt(body, calls);
        }
        TypedStmtKind::ForEach { iterable, body, .. } => {
            collect_impl_calls_expr(iterable, calls);
            collect_impl_calls_stmt(body, calls);
        }
        TypedStmtKind::Return(expr) => {
            if let Some(expr) = expr {
                collect_impl_calls_expr(expr, calls);
            }
        }
        TypedStmtKind::Function(function) => {
            for stmt in &function.body {
                collect_impl_calls_stmt(stmt, calls);
            }
        }
        TypedStmtKind::ImplDecl { methods, .. } => {
            for method in methods {
                for stmt in &method.body {
                    collect_impl_calls_stmt(stmt, calls);
                }
            }
        }
        TypedStmtKind::Break
        | TypedStmtKind::Continue
        | TypedStmtKind::Needs(_)
        | TypedStmtKind::TraitDecl { .. }
        | TypedStmtKind::StructDecl { .. }
        | TypedStmtKind::EnumDecl { .. } => {}
    }
}

fn collect_impl_calls_expr(expr: &TypedExpr, calls: &mut Vec<ImplCallSite>) {
    match &expr.kind {
        TypedExprKind::Call { callee, args } => {
            if let TypedExprKind::StructMethod {
                object,
                symbol,
                separator,
                ..
            } = &callee.kind
            {
                let receiver =
                    (*separator == aelys_syntax::MemberSeparator::Dot).then(|| object.ty.clone());
                calls.push(ImplCallSite {
                    symbol: symbol.clone(),
                    receiver,
                    actuals: args.iter().map(|arg| arg.ty.clone()).collect(),
                    result: expr.ty.clone(),
                    span: expr.span,
                    callee_span: callee.span,
                    ancestors: Vec::new(),
                    reportable: true,
                });
            }
            collect_impl_calls_expr(callee, calls);
            for arg in args {
                collect_impl_calls_expr(arg, calls);
            }
        }
        TypedExprKind::Binary { left, right, .. }
        | TypedExprKind::And { left, right }
        | TypedExprKind::Or { left, right } => {
            collect_impl_calls_expr(left, calls);
            collect_impl_calls_expr(right, calls);
        }
        TypedExprKind::Try { operand, .. } => {
            if let Some(call) = try_conversion_call(expr) {
                calls.push(call);
            }
            collect_impl_calls_expr(operand, calls);
        }
        TypedExprKind::Unary { operand, .. }
        | TypedExprKind::Grouping(operand)
        | TypedExprKind::Lambda(operand) => collect_impl_calls_expr(operand, calls),
        TypedExprKind::Assign { value, .. } => collect_impl_calls_expr(value, calls),
        TypedExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_impl_calls_expr(condition, calls);
            collect_impl_calls_expr(then_branch, calls);
            collect_impl_calls_expr(else_branch, calls);
        }
        TypedExprKind::Match { scrutinee, arms } => {
            collect_impl_calls_expr(scrutinee, calls);
            for arm in arms {
                if let Some(guard) = &arm.guard {
                    collect_impl_calls_expr(guard, calls);
                }
                match &arm.body {
                    TypedMatchArmBody::Expr(expr) => collect_impl_calls_expr(expr, calls),
                    TypedMatchArmBody::Block(stmts) => {
                        for stmt in stmts {
                            collect_impl_calls_stmt(stmt, calls);
                        }
                    }
                }
            }
        }
        TypedExprKind::LambdaInner { body, .. } => {
            for stmt in body {
                collect_impl_calls_stmt(stmt, calls);
            }
        }
        TypedExprKind::Member { object, .. }
        | TypedExprKind::StructField { object, .. }
        | TypedExprKind::StructMethod { object, .. } => collect_impl_calls_expr(object, calls),
        TypedExprKind::MemberAssign { object, value, .. } => {
            collect_impl_calls_expr(object, calls);
            collect_impl_calls_expr(value, calls);
        }
        TypedExprKind::ArrayLiteral {
            elements, repeat, ..
        }
        | TypedExprKind::VecLiteral {
            elements, repeat, ..
        } => {
            for element in elements {
                collect_impl_calls_expr(element, calls);
            }
            if let Some(repeat) = repeat {
                collect_impl_calls_expr(repeat, calls);
            }
        }
        TypedExprKind::ArraySized { size, .. } => collect_impl_calls_expr(size, calls),
        TypedExprKind::Index { object, index }
        | TypedExprKind::Slice {
            object,
            range: index,
        } => {
            collect_impl_calls_expr(object, calls);
            collect_impl_calls_expr(index, calls);
        }
        TypedExprKind::IndexAssign {
            object,
            index,
            value,
        } => {
            collect_impl_calls_expr(object, calls);
            collect_impl_calls_expr(index, calls);
            collect_impl_calls_expr(value, calls);
        }
        TypedExprKind::Range { start, end, .. } => {
            if let Some(start) = start {
                collect_impl_calls_expr(start, calls);
            }
            if let Some(end) = end {
                collect_impl_calls_expr(end, calls);
            }
        }
        TypedExprKind::StructLiteral { fields, .. } => {
            for (_, field) in fields {
                collect_impl_calls_expr(field, calls);
            }
        }
        TypedExprKind::EnumConstruct { fields, .. } => {
            for (_, field) in fields {
                collect_impl_calls_expr(field, calls);
            }
        }
        TypedExprKind::Cast { expr, .. } => collect_impl_calls_expr(expr, calls),
        TypedExprKind::FmtString(parts) => {
            for part in parts {
                if let TypedFmtStringPart::Expr(expr) = part {
                    collect_impl_calls_expr(expr, calls);
                }
            }
        }
        TypedExprKind::Int(_)
        | TypedExprKind::Float(_)
        | TypedExprKind::Bool(_)
        | TypedExprKind::String(_)
        | TypedExprKind::Unit
        | TypedExprKind::Null
        | TypedExprKind::AssociatedConst { .. }
        | TypedExprKind::Identifier(_) => {}
    }
}

fn visit_exprs_stmt(stmt: &mut TypedStmt, visit: &mut dyn FnMut(&mut TypedExpr)) {
    match &mut stmt.kind {
        TypedStmtKind::Expression(expr) => visit_exprs_expr(expr, visit),
        TypedStmtKind::Let { initializer, .. } => visit_exprs_expr(initializer, visit),
        TypedStmtKind::Block(stmts) => {
            for stmt in stmts {
                visit_exprs_stmt(stmt, visit);
            }
        }
        TypedStmtKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            visit_exprs_expr(condition, visit);
            visit_exprs_stmt(then_branch, visit);
            if let Some(else_branch) = else_branch {
                visit_exprs_stmt(else_branch, visit);
            }
        }
        TypedStmtKind::While { condition, body } => {
            visit_exprs_expr(condition, visit);
            visit_exprs_stmt(body, visit);
        }
        TypedStmtKind::For {
            start,
            end,
            step,
            body,
            ..
        } => {
            visit_exprs_expr(start, visit);
            visit_exprs_expr(end, visit);
            if let Some(step) = step.as_mut() {
                visit_exprs_expr(step, visit);
            }
            visit_exprs_stmt(body, visit);
        }
        TypedStmtKind::ForEach { iterable, body, .. } => {
            visit_exprs_expr(iterable, visit);
            visit_exprs_stmt(body, visit);
        }
        TypedStmtKind::Return(expr) => {
            if let Some(expr) = expr {
                visit_exprs_expr(expr, visit);
            }
        }
        TypedStmtKind::Function(function) => {
            for stmt in &mut function.body {
                visit_exprs_stmt(stmt, visit);
            }
        }
        TypedStmtKind::ImplDecl { methods, .. } => {
            for method in methods {
                for stmt in &mut method.body {
                    visit_exprs_stmt(stmt, visit);
                }
            }
        }
        TypedStmtKind::Break
        | TypedStmtKind::Continue
        | TypedStmtKind::Needs(_)
        | TypedStmtKind::TraitDecl { .. }
        | TypedStmtKind::StructDecl { .. }
        | TypedStmtKind::EnumDecl { .. } => {}
    }
}

fn visit_exprs_expr(expr: &mut TypedExpr, visit: &mut dyn FnMut(&mut TypedExpr)) {
    visit(expr);
    match &mut expr.kind {
        TypedExprKind::Call { callee, args } => {
            visit_exprs_expr(callee, visit);
            for arg in args {
                visit_exprs_expr(arg, visit);
            }
        }
        TypedExprKind::Binary { left, right, .. }
        | TypedExprKind::And { left, right }
        | TypedExprKind::Or { left, right } => {
            visit_exprs_expr(left, visit);
            visit_exprs_expr(right, visit);
        }
        TypedExprKind::Unary { operand, .. }
        | TypedExprKind::Grouping(operand)
        | TypedExprKind::Lambda(operand)
        | TypedExprKind::Try { operand, .. } => visit_exprs_expr(operand, visit),
        TypedExprKind::Assign { value, .. } => visit_exprs_expr(value, visit),
        TypedExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            visit_exprs_expr(condition, visit);
            visit_exprs_expr(then_branch, visit);
            visit_exprs_expr(else_branch, visit);
        }
        TypedExprKind::Match { scrutinee, arms } => {
            visit_exprs_expr(scrutinee, visit);
            for arm in arms {
                if let Some(guard) = &mut arm.guard {
                    visit_exprs_expr(guard, visit);
                }
                match &mut arm.body {
                    TypedMatchArmBody::Expr(expr) => visit_exprs_expr(expr, visit),
                    TypedMatchArmBody::Block(stmts) => {
                        for stmt in stmts {
                            visit_exprs_stmt(stmt, visit);
                        }
                    }
                }
            }
        }
        TypedExprKind::LambdaInner { body, .. } => {
            for stmt in body {
                visit_exprs_stmt(stmt, visit);
            }
        }
        TypedExprKind::Member { object, .. }
        | TypedExprKind::StructField { object, .. }
        | TypedExprKind::StructMethod { object, .. } => visit_exprs_expr(object, visit),
        TypedExprKind::MemberAssign { object, value, .. } => {
            visit_exprs_expr(object, visit);
            visit_exprs_expr(value, visit);
        }
        TypedExprKind::ArrayLiteral {
            elements, repeat, ..
        }
        | TypedExprKind::VecLiteral {
            elements, repeat, ..
        } => {
            for element in elements {
                visit_exprs_expr(element, visit);
            }
            if let Some(repeat) = repeat {
                visit_exprs_expr(repeat, visit);
            }
        }
        TypedExprKind::ArraySized { size, .. } => visit_exprs_expr(size, visit),
        TypedExprKind::Index { object, index }
        | TypedExprKind::Slice {
            object,
            range: index,
        } => {
            visit_exprs_expr(object, visit);
            visit_exprs_expr(index, visit);
        }
        TypedExprKind::IndexAssign {
            object,
            index,
            value,
        } => {
            visit_exprs_expr(object, visit);
            visit_exprs_expr(index, visit);
            visit_exprs_expr(value, visit);
        }
        TypedExprKind::Range { start, end, .. } => {
            if let Some(start) = start {
                visit_exprs_expr(start, visit);
            }
            if let Some(end) = end {
                visit_exprs_expr(end, visit);
            }
        }
        TypedExprKind::StructLiteral { fields, .. } => {
            for (_, field) in fields {
                visit_exprs_expr(field, visit);
            }
        }
        TypedExprKind::EnumConstruct { fields, .. } => {
            for (_, field) in fields {
                visit_exprs_expr(field, visit);
            }
        }
        TypedExprKind::Cast { expr, .. } => visit_exprs_expr(expr, visit),
        TypedExprKind::FmtString(parts) => {
            for part in parts {
                if let TypedFmtStringPart::Expr(expr) = part {
                    visit_exprs_expr(expr, visit);
                }
            }
        }
        TypedExprKind::Int(_)
        | TypedExprKind::Float(_)
        | TypedExprKind::Bool(_)
        | TypedExprKind::String(_)
        | TypedExprKind::Unit
        | TypedExprKind::Null
        | TypedExprKind::AssociatedConst { .. }
        | TypedExprKind::Identifier(_) => {}
    }
}

struct ImplInstanceMethod {
    target: InferType,
    args: Vec<InferType>,
    method_args: Vec<InferType>,
    symbol: String,
}

struct ImplCallRewrite<'a> {
    templates: &'a [GenericImplTemplate],
    template_indices_by_method: &'a HashMap<String, Vec<usize>>,
    replacements: &'a HashMap<String, Vec<ImplInstanceMethod>>,
}

/// the conversion a `?` selected, read as a call of the impl method it names:
fn try_conversion_call(expr: &TypedExpr) -> Option<ImplCallSite> {
    let TypedExprKind::Try {
        operand,
        conversion: Some(symbol),
        conversion_target: Some(target),
    } = &expr.kind
    else {
        return None;
    };
    let InferType::Result(_, source) = &operand.ty else {
        return None;
    };
    Some(ImplCallSite {
        symbol: symbol.clone(),
        receiver: None,
        actuals: vec![source.as_ref().clone()],
        result: target.clone(),
        span: expr.span,
        callee_span: expr.span,
        ancestors: Vec::new(),
        reportable: false,
    })
}

fn rewrite_try_conversion(expr: &mut TypedExpr, rewrite: &ImplCallRewrite<'_>) {
    let Some(call) = try_conversion_call(expr) else {
        return;
    };
    let Some(candidates) = rewrite.replacements.get(&call.symbol) else {
        return;
    };
    let TypedExprKind::Try {
        conversion: Some(symbol),
        ..
    } = &mut expr.kind
    else {
        return;
    };
    for &template_index in rewrite
        .template_indices_by_method
        .get(&call.symbol)
        .into_iter()
        .flatten()
    {
        let ImplDeductionOutcome::Resolved(deduction) =
            deduce_impl_arguments(&rewrite.templates[template_index], &call)
        else {
            continue;
        };
        if let Some(instance) = candidates.iter().find(|candidate| {
            candidate.args == deduction.impl_args && candidate.method_args == deduction.method_args
        }) {
            *symbol = instance.symbol.clone();
            return;
        }
    }
}

fn rewrite_impl_call_symbols_stmt(stmt: &mut TypedStmt, rewrite: &ImplCallRewrite<'_>) {
    visit_exprs_stmt(stmt, &mut |expr| {
        rewrite_impl_call_symbol(expr, rewrite);
        rewrite_try_conversion(expr, rewrite);
    });
}

fn rewrite_impl_call_symbol(expr: &mut TypedExpr, rewrite: &ImplCallRewrite<'_>) {
    let result = expr.ty.clone();
    let span = expr.span;
    let TypedExprKind::Call { callee, args } = &mut expr.kind else {
        return;
    };
    let TypedExprKind::StructMethod {
        object,
        symbol,
        separator,
        ..
    } = &mut callee.kind
    else {
        return;
    };
    let Some(candidates) = rewrite.replacements.get(symbol) else {
        return;
    };
    let receiver = (*separator == aelys_syntax::MemberSeparator::Dot).then(|| object.ty.clone());
    // the same deduction the worklist ran, so a call can never land on an
    let call = ImplCallSite {
        symbol: symbol.clone(),
        receiver: receiver.clone(),
        actuals: args.iter().map(|arg| arg.ty.clone()).collect(),
        result,
        span,
        callee_span: span,
        ancestors: Vec::new(),
        reportable: false,
    };
    for &template_index in rewrite
        .template_indices_by_method
        .get(&call.symbol)
        .into_iter()
        .flatten()
    {
        let ImplDeductionOutcome::Resolved(deduction) =
            deduce_impl_arguments(&rewrite.templates[template_index], &call)
        else {
            continue;
        };
        if let Some(instance) = candidates.iter().find(|candidate| {
            candidate.args == deduction.impl_args && candidate.method_args == deduction.method_args
        }) {
            *symbol = instance.symbol.clone();
            return;
        }
    }
    let Some(receiver) = receiver else {
        return;
    };
    if let Some(instance) = candidates
        .iter()
        .find(|candidate| candidate.target == receiver && candidate.method_args.is_empty())
    {
        *symbol = instance.symbol.clone();
    }
}

fn resolve_associated_const_node(
    expr: &mut TypedExpr,
    substitution: &crate::unify::Substitution,
    constants: &std::collections::HashMap<(String, String), crate::infer::ConstResolution>,
    errors: &mut Vec<TypeError>,
) {
    let TypedExprKind::AssociatedConst {
        param,
        trait_name,
        item,
    } = &expr.kind
    else {
        return;
    };
    let concrete = substitution.apply(&InferType::Param(param.clone()));
    // that this substitution does not bind; the guard after monomorphization
    if !concrete.is_concrete() {
        return;
    }
    let receiver = match &concrete {
        InferType::Struct(name) => name.clone(),
        InferType::Applied { name, .. } => name.clone(),
        other => other.source_spelling(),
    };
    let non_integer = match expr.ty.is_integer() {
        true => None,
        false => Some(expr.ty.source_spelling()),
    };
    match constants.get(&(receiver.clone(), item.clone())) {
        Some(crate::infer::ConstResolution::Value(value)) => {
            expr.kind = TypedExprKind::Int(*value);
            expr.ty = InferType::I64;
        }
        found => {
            let cause = match found {
                Some(resolution) => crate::infer::expr::member::projection_failure_for(
                    resolution,
                    &receiver,
                    item,
                    false,
                    non_integer,
                ),
                None => crate::constraint::ProjectionFailure::NoImpl,
            };
            errors.push(TypeError {
                kind: crate::constraint::TypeErrorKind::AmbiguousAssociatedProjection {
                    receiver,
                    item: item.clone(),
                    cause,
                },
                span: expr.span,
                reason: crate::constraint::ConstraintReason::Other(format!(
                    "associated constant of trait '{trait_name}'"
                )),
            });
            expr.kind = TypedExprKind::Null;
            expr.ty = InferType::Poison;
        }
    }
}

fn resolve_bound_marker(
    expr: &mut TypedExpr,
    type_table: &crate::types::TypeTable,
    gate: &std::collections::BTreeMap<(String, String), (InferType, Vec<String>)>,
    errors: &mut Vec<TypeError>,
) {
    let span = expr.span;
    let TypedExprKind::StructMethod { object, symbol, .. } = &mut expr.kind else {
        return;
    };
    let marker = if symbol == DISPLAY_MARKER_SYMBOL {
        Some((
            crate::prelude::DISPLAY_TRAIT.to_string(),
            String::new(),
            crate::prelude::DISPLAY_METHOD.to_string(),
        ))
    } else {
        parse_bound_marker(symbol)
    };
    let Some((trait_name, _param, method_name)) = marker else {
        return;
    };
    let Some(receiver) = concrete_receiver(type_table, &object.ty) else {
        return;
    };
    object.ty = receiver.clone();
    match type_table.select_bound_method(&trait_name, &receiver, &method_name) {
        BoundSelection::Selected(resolved) => *symbol = resolved,
        BoundSelection::CompilerRule => {}
        BoundSelection::Missing => {
            let gated = crate::types::nominal_name(&receiver).and_then(|target| {
                super::signatures::supertrait_gate_error_for(
                    type_table,
                    gate,
                    &target,
                    &method_name,
                    span,
                )
            });
            let denied = type_table.denies(&trait_name, &receiver, &[]);
            errors.push(gated.unwrap_or_else(|| TypeError {
                kind: TypeErrorKind::UnsatisfiedTraitBound {
                    trait_name,
                    trait_args: Vec::new(),
                    ty: receiver,
                    denied,
                },
                span,
                reason: ConstraintReason::Other("bound method at a concrete instance".to_string()),
            }));
        }
        BoundSelection::AmbiguousSpecialization => errors.push(TypeError {
            kind: TypeErrorKind::AmbiguousSpecialization {
                trait_name,
                target: receiver,
            },
            span,
            reason: ConstraintReason::Other("bound method at a concrete instance".to_string()),
        }),
        BoundSelection::Ambiguous => {
            let target =
                crate::types::nominal_name(&receiver).unwrap_or_else(|| receiver.to_string());
            errors.push(TypeError {
                kind: super::signatures::ambiguous_trait_instantiation_kind(
                    type_table,
                    &target,
                    &method_name,
                    &trait_name,
                ),
                span,
                reason: ConstraintReason::Other("bound method at a concrete instance".to_string()),
            })
        }
    }
}

fn resolve_deferred_member(
    expr: &mut TypedExpr,
    type_table: &crate::types::TypeTable,
    errors: &mut Vec<TypeError>,
) {
    let TypedExprKind::Member {
        object,
        member,
        separator: MemberSeparator::Dot,
    } = &mut expr.kind
    else {
        return;
    };
    if matches!(expr.ty, InferType::Poison) {
        return;
    }
    let Some((name, substitutions)) =
        crate::infer::expr::member::nominal_parts(&object.ty, type_table)
    else {
        return;
    };
    if let Some(definition) = type_table.get_struct(&name)
        && let Some(field) = definition.fields.iter().find(|field| field.name == *member)
    {
        let field_ty = field.ty.substitute_params(&substitutions);
        if expr.ty.is_concrete() && expr.ty != field_ty {
            errors.push(TypeError {
                kind: TypeErrorKind::Mismatch {
                    expected: field_ty.clone(),
                    found: expr.ty.clone(),
                },
                span: expr.span,
                reason: ConstraintReason::Other("deferred field lookup".to_string()),
            });
        }
        expr.kind = TypedExprKind::StructField {
            object: std::mem::replace(
                object,
                Box::new(TypedExpr::new(
                    TypedExprKind::Unit,
                    InferType::Unit,
                    expr.span,
                )),
            ),
            member: member.clone(),
            offset: field.ordinal,
            schema_index: type_table.schema_index(&name).unwrap_or(0),
        };
        expr.ty = field_ty;
        return;
    }

    let inherent = type_table
        .method(&name, member)
        .filter(|method| method.has_self)
        .cloned();
    let method = if inherent.is_some() {
        inherent
    } else {
        let candidates: Vec<_> = type_table
            .trait_methods(&name, member)
            .iter()
            .filter(|candidate| candidate.has_self)
            .cloned()
            .collect();
        match candidates.as_slice() {
            [candidate] => Some(crate::types::StructMethod {
                name: candidate.name.clone(),
                symbol: candidate.symbol.clone(),
                params: candidate.params.clone(),
                return_type: candidate.return_type.clone(),
                has_self: candidate.has_self,
                mutable_self: candidate.mutable_self,
                own_type_params: Vec::new(),
            }),
            [] => None,
            _ => {
                let symbols: Vec<String> = candidates
                    .iter()
                    .map(|candidate| candidate.symbol.clone())
                    .collect();
                errors.push(TypeError {
                    kind: super::signatures::ambiguous_trait_method_kind(
                        type_table, &name, member, &symbols,
                    ),
                    span: expr.span,
                    reason: ConstraintReason::Other("deferred member lookup".to_string()),
                });
                return;
            }
        }
    };
    let Some(mut method) = method else {
        errors.push(TypeError {
            kind: TypeErrorKind::UnknownField {
                structure: name,
                field: member.clone(),
            },
            span: expr.span,
            reason: ConstraintReason::Other("deferred member lookup".to_string()),
        });
        return;
    };
    method.params = method
        .params
        .iter()
        .map(|param| param.substitute_params(&substitutions))
        .collect();
    method.return_type = method.return_type.substitute_params(&substitutions);
    let function_type = InferType::Function {
        params: method.params.into_iter().skip(1).collect(),
        ret: Box::new(method.return_type),
    };
    if expr.ty.is_concrete() && expr.ty != function_type {
        errors.push(TypeError {
            kind: TypeErrorKind::Mismatch {
                expected: function_type.clone(),
                found: expr.ty.clone(),
            },
            span: expr.span,
            reason: ConstraintReason::Other("deferred method lookup".to_string()),
        });
    }
    let receiver = std::mem::replace(
        object,
        Box::new(TypedExpr::new(
            TypedExprKind::Unit,
            InferType::Unit,
            expr.span,
        )),
    );
    expr.kind = TypedExprKind::StructMethod {
        object: receiver,
        symbol: method.symbol,
        method: member.clone(),
        separator: MemberSeparator::Dot,
    };
    expr.ty = function_type;
}

fn resolve_deferred_call_type(expr: &mut TypedExpr) {
    let TypedExprKind::Call { callee, .. } = &expr.kind else {
        return;
    };
    let InferType::Function { ret, .. } = &callee.ty else {
        return;
    };
    if !expr.ty.is_concrete() {
        expr.ty = ret.as_ref().clone();
    }
}

fn resolve_deferred_statement_types(stmt: &mut TypedStmt) {
    match &mut stmt.kind {
        TypedStmtKind::Let {
            initializer,
            var_type,
            ..
        } if matches!(var_type, InferType::Var(_)) && initializer.ty.is_concrete() => {
            *var_type = initializer.ty.clone();
        }
        TypedStmtKind::Block(stmts) => {
            for stmt in stmts {
                resolve_deferred_statement_types(stmt);
            }
        }
        TypedStmtKind::If {
            then_branch,
            else_branch,
            ..
        } => {
            resolve_deferred_statement_types(then_branch);
            if let Some(else_branch) = else_branch {
                resolve_deferred_statement_types(else_branch);
            }
        }
        TypedStmtKind::While { body, .. }
        | TypedStmtKind::For { body, .. }
        | TypedStmtKind::ForEach { body, .. } => resolve_deferred_statement_types(body),
        TypedStmtKind::Function(function) => {
            for stmt in &mut function.body {
                resolve_deferred_statement_types(stmt);
            }
        }
        TypedStmtKind::ImplDecl { methods, .. } => {
            for method in methods {
                for stmt in &mut method.body {
                    resolve_deferred_statement_types(stmt);
                }
            }
        }
        TypedStmtKind::Expression(_)
        | TypedStmtKind::Return(_)
        | TypedStmtKind::Break
        | TypedStmtKind::Continue
        | TypedStmtKind::Needs(_)
        | TypedStmtKind::TraitDecl { .. }
        | TypedStmtKind::StructDecl { .. }
        | TypedStmtKind::EnumDecl { .. }
        | TypedStmtKind::Let { .. } => {}
    }
}

pub(crate) fn mark_display_argument(arg: &mut TypedExpr) {
    if matches!(arg.kind, TypedExprKind::FmtString(_))
        || crate::prelude::provides(crate::prelude::DISPLAY_TRAIT, &arg.ty, &[])
    {
        return;
    }
    let span = arg.span;
    let placeholder = TypedExpr::new(TypedExprKind::Unit, InferType::Unit, span);
    let object = std::mem::replace(arg, placeholder);
    let object_ty = object.ty.clone();
    let callee = TypedExpr::new(
        TypedExprKind::StructMethod {
            object: Box::new(object),
            symbol: DISPLAY_MARKER_SYMBOL.to_string(),
            method: crate::prelude::DISPLAY_METHOD.to_string(),
            separator: aelys_syntax::MemberSeparator::Dot,
        },
        InferType::Function {
            params: vec![object_ty],
            ret: Box::new(InferType::String),
        },
        span,
    );
    *arg = TypedExpr::new(
        TypedExprKind::Call {
            callee: Box::new(callee),
            args: Vec::new(),
        },
        InferType::String,
        span,
    );
}

fn resolve_display_marker(expr: &mut TypedExpr, type_table: &crate::types::TypeTable) {
    let TypedExprKind::Call { callee, .. } = &mut expr.kind else {
        return;
    };
    let TypedExprKind::StructMethod { object, symbol, .. } = &mut callee.kind else {
        return;
    };
    if symbol != DISPLAY_MARKER_SYMBOL {
        return;
    }
    let Some(receiver) = concrete_receiver(type_table, &object.ty) else {
        return;
    };
    if let BoundSelection::Selected(resolved) = type_table.select_bound_method(
        crate::prelude::DISPLAY_TRAIT,
        &receiver,
        crate::prelude::DISPLAY_METHOD,
    ) {
        // the object still carries the projection it was written with; the call it
        object.ty = receiver;
        *symbol = resolved;
        return;
    }
    strip_display_marker(expr);
}

/// a projection is a spelling of a type, not a type of its own: a marker whose
fn concrete_receiver(type_table: &crate::types::TypeTable, ty: &InferType) -> Option<InferType> {
    if ty.is_concrete() {
        return Some(ty.clone());
    }
    type_table
        .resolve_projection(ty)
        .filter(|resolved| resolved.is_concrete())
}

fn strip_display_markers(stmts: &mut [TypedStmt]) {
    for stmt in stmts.iter_mut() {
        visit_exprs_stmt(stmt, &mut strip_display_marker);
    }
}

// an unresolved marker must never reach code generation, so the argument goes back to the host untouched
fn strip_display_marker(expr: &mut TypedExpr) {
    let TypedExprKind::Call { callee, .. } = &mut expr.kind else {
        return;
    };
    let TypedExprKind::StructMethod { object, symbol, .. } = &mut callee.kind else {
        return;
    };
    if symbol != DISPLAY_MARKER_SYMBOL {
        return;
    }
    let placeholder = TypedExpr::new(TypedExprKind::Unit, InferType::Unit, expr.span);
    *expr = std::mem::replace(object.as_mut(), placeholder);
}

#[allow(clippy::too_many_arguments)]
fn rewrite_stmt(
    inference: &mut TypeInference,
    stmt: &mut TypedStmt,
    generic_defs: &HashMap<String, TypedFunction>,
    queue: &mut VecDeque<InstanceFrame>,
    queued: &mut HashSet<InstanceKey>,
    symbols: &mut HashMap<InstanceKey, String>,
    symbol_keys: &mut HashMap<String, InstanceKey>,
    errors: &mut Vec<TypeError>,
) {
    match &mut stmt.kind {
        TypedStmtKind::Expression(expr) => rewrite_expr(
            inference,
            expr,
            generic_defs,
            queue,
            queued,
            symbols,
            symbol_keys,
            errors,
        ),
        TypedStmtKind::Let { initializer, .. } => rewrite_expr(
            inference,
            initializer,
            generic_defs,
            queue,
            queued,
            symbols,
            symbol_keys,
            errors,
        ),
        TypedStmtKind::Block(stmts) => {
            for stmt in stmts {
                rewrite_stmt(
                    inference,
                    stmt,
                    generic_defs,
                    queue,
                    queued,
                    symbols,
                    symbol_keys,
                    errors,
                );
            }
        }
        TypedStmtKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            rewrite_expr(
                inference,
                condition,
                generic_defs,
                queue,
                queued,
                symbols,
                symbol_keys,
                errors,
            );
            rewrite_stmt(
                inference,
                then_branch,
                generic_defs,
                queue,
                queued,
                symbols,
                symbol_keys,
                errors,
            );
            if let Some(else_branch) = else_branch {
                rewrite_stmt(
                    inference,
                    else_branch,
                    generic_defs,
                    queue,
                    queued,
                    symbols,
                    symbol_keys,
                    errors,
                );
            }
        }
        TypedStmtKind::While { condition, body } => {
            rewrite_expr(
                inference,
                condition,
                generic_defs,
                queue,
                queued,
                symbols,
                symbol_keys,
                errors,
            );
            rewrite_stmt(
                inference,
                body,
                generic_defs,
                queue,
                queued,
                symbols,
                symbol_keys,
                errors,
            );
        }
        TypedStmtKind::For {
            start,
            end,
            step,
            body,
            ..
        } => {
            rewrite_expr(
                inference,
                start,
                generic_defs,
                queue,
                queued,
                symbols,
                symbol_keys,
                errors,
            );
            rewrite_expr(
                inference,
                end,
                generic_defs,
                queue,
                queued,
                symbols,
                symbol_keys,
                errors,
            );
            if let Some(step) = step.as_mut() {
                rewrite_expr(
                    inference,
                    step,
                    generic_defs,
                    queue,
                    queued,
                    symbols,
                    symbol_keys,
                    errors,
                );
            }
            rewrite_stmt(
                inference,
                body,
                generic_defs,
                queue,
                queued,
                symbols,
                symbol_keys,
                errors,
            );
        }
        TypedStmtKind::ForEach { iterable, body, .. } => {
            rewrite_expr(
                inference,
                iterable,
                generic_defs,
                queue,
                queued,
                symbols,
                symbol_keys,
                errors,
            );
            rewrite_stmt(
                inference,
                body,
                generic_defs,
                queue,
                queued,
                symbols,
                symbol_keys,
                errors,
            );
        }
        TypedStmtKind::Return(expr) => {
            if let Some(expr) = expr {
                rewrite_expr(
                    inference,
                    expr,
                    generic_defs,
                    queue,
                    queued,
                    symbols,
                    symbol_keys,
                    errors,
                );
            }
        }
        TypedStmtKind::Function(function) => {
            for stmt in &mut function.body {
                rewrite_stmt(
                    inference,
                    stmt,
                    generic_defs,
                    queue,
                    queued,
                    symbols,
                    symbol_keys,
                    errors,
                );
            }
        }
        TypedStmtKind::ImplDecl { methods, .. } => {
            for method in methods {
                for stmt in &mut method.body {
                    rewrite_stmt(
                        inference,
                        stmt,
                        generic_defs,
                        queue,
                        queued,
                        symbols,
                        symbol_keys,
                        errors,
                    );
                }
            }
        }
        TypedStmtKind::Break
        | TypedStmtKind::Continue
        | TypedStmtKind::Needs(_)
        | TypedStmtKind::TraitDecl { .. }
        | TypedStmtKind::StructDecl { .. }
        | TypedStmtKind::EnumDecl { .. } => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn rewrite_expr(
    inference: &mut TypeInference,
    expr: &mut TypedExpr,
    generic_defs: &HashMap<String, TypedFunction>,
    queue: &mut VecDeque<InstanceFrame>,
    queued: &mut HashSet<InstanceKey>,
    symbols: &mut HashMap<InstanceKey, String>,
    symbol_keys: &mut HashMap<String, InstanceKey>,
    errors: &mut Vec<TypeError>,
) {
    match &mut expr.kind {
        TypedExprKind::Binary { left, right, .. }
        | TypedExprKind::And { left, right }
        | TypedExprKind::Or { left, right } => {
            rewrite_expr(
                inference,
                left,
                generic_defs,
                queue,
                queued,
                symbols,
                symbol_keys,
                errors,
            );
            rewrite_expr(
                inference,
                right,
                generic_defs,
                queue,
                queued,
                symbols,
                symbol_keys,
                errors,
            );
        }
        TypedExprKind::Unary { operand, .. }
        | TypedExprKind::Grouping(operand)
        | TypedExprKind::Lambda(operand)
        | TypedExprKind::Try { operand, .. } => rewrite_expr(
            inference,
            operand,
            generic_defs,
            queue,
            queued,
            symbols,
            symbol_keys,
            errors,
        ),
        TypedExprKind::Call { callee, args } => {
            rewrite_expr(
                inference,
                callee,
                generic_defs,
                queue,
                queued,
                symbols,
                symbol_keys,
                errors,
            );
            for arg in args.iter_mut() {
                rewrite_expr(
                    inference,
                    arg,
                    generic_defs,
                    queue,
                    queued,
                    symbols,
                    symbol_keys,
                    errors,
                );
            }
            rewrite_generic_call(
                inference,
                callee,
                args,
                &mut expr.ty,
                expr.span,
                generic_defs,
                queue,
                queued,
                symbols,
                symbol_keys,
                errors,
            );
        }
        TypedExprKind::Assign { value, .. } => rewrite_expr(
            inference,
            value,
            generic_defs,
            queue,
            queued,
            symbols,
            symbol_keys,
            errors,
        ),
        TypedExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            rewrite_expr(
                inference,
                condition,
                generic_defs,
                queue,
                queued,
                symbols,
                symbol_keys,
                errors,
            );
            rewrite_expr(
                inference,
                then_branch,
                generic_defs,
                queue,
                queued,
                symbols,
                symbol_keys,
                errors,
            );
            rewrite_expr(
                inference,
                else_branch,
                generic_defs,
                queue,
                queued,
                symbols,
                symbol_keys,
                errors,
            );
        }
        TypedExprKind::Match { scrutinee, arms } => {
            rewrite_expr(
                inference,
                scrutinee,
                generic_defs,
                queue,
                queued,
                symbols,
                symbol_keys,
                errors,
            );
            for arm in arms {
                rewrite_pattern(&mut arm.pattern);
                if let Some(guard) = &mut arm.guard {
                    rewrite_expr(
                        inference,
                        guard,
                        generic_defs,
                        queue,
                        queued,
                        symbols,
                        symbol_keys,
                        errors,
                    );
                }
                match &mut arm.body {
                    TypedMatchArmBody::Expr(expr) => rewrite_expr(
                        inference,
                        expr,
                        generic_defs,
                        queue,
                        queued,
                        symbols,
                        symbol_keys,
                        errors,
                    ),
                    TypedMatchArmBody::Block(stmts) => {
                        for stmt in stmts {
                            rewrite_stmt(
                                inference,
                                stmt,
                                generic_defs,
                                queue,
                                queued,
                                symbols,
                                symbol_keys,
                                errors,
                            );
                        }
                    }
                }
            }
        }
        TypedExprKind::LambdaInner { body, .. } => {
            for stmt in body {
                rewrite_stmt(
                    inference,
                    stmt,
                    generic_defs,
                    queue,
                    queued,
                    symbols,
                    symbol_keys,
                    errors,
                );
            }
        }
        TypedExprKind::Member { object, .. }
        | TypedExprKind::StructField { object, .. }
        | TypedExprKind::StructMethod { object, .. } => rewrite_expr(
            inference,
            object,
            generic_defs,
            queue,
            queued,
            symbols,
            symbol_keys,
            errors,
        ),
        TypedExprKind::MemberAssign { object, value, .. } => {
            rewrite_expr(
                inference,
                object,
                generic_defs,
                queue,
                queued,
                symbols,
                symbol_keys,
                errors,
            );
            rewrite_expr(
                inference,
                value,
                generic_defs,
                queue,
                queued,
                symbols,
                symbol_keys,
                errors,
            );
        }
        TypedExprKind::ArrayLiteral {
            elements, repeat, ..
        }
        | TypedExprKind::VecLiteral {
            elements, repeat, ..
        } => {
            for element in elements {
                rewrite_expr(
                    inference,
                    element,
                    generic_defs,
                    queue,
                    queued,
                    symbols,
                    symbol_keys,
                    errors,
                );
            }
            if let Some(repeat) = repeat {
                rewrite_expr(
                    inference,
                    repeat,
                    generic_defs,
                    queue,
                    queued,
                    symbols,
                    symbol_keys,
                    errors,
                );
            }
        }
        TypedExprKind::ArraySized { size, .. } => rewrite_expr(
            inference,
            size,
            generic_defs,
            queue,
            queued,
            symbols,
            symbol_keys,
            errors,
        ),
        TypedExprKind::Index { object, index }
        | TypedExprKind::Slice {
            object,
            range: index,
        } => {
            rewrite_expr(
                inference,
                object,
                generic_defs,
                queue,
                queued,
                symbols,
                symbol_keys,
                errors,
            );
            rewrite_expr(
                inference,
                index,
                generic_defs,
                queue,
                queued,
                symbols,
                symbol_keys,
                errors,
            );
        }
        TypedExprKind::IndexAssign {
            object,
            index,
            value,
        } => {
            rewrite_expr(
                inference,
                object,
                generic_defs,
                queue,
                queued,
                symbols,
                symbol_keys,
                errors,
            );
            rewrite_expr(
                inference,
                index,
                generic_defs,
                queue,
                queued,
                symbols,
                symbol_keys,
                errors,
            );
            rewrite_expr(
                inference,
                value,
                generic_defs,
                queue,
                queued,
                symbols,
                symbol_keys,
                errors,
            );
        }
        TypedExprKind::Range { start, end, .. } => {
            if let Some(start) = start {
                rewrite_expr(
                    inference,
                    start,
                    generic_defs,
                    queue,
                    queued,
                    symbols,
                    symbol_keys,
                    errors,
                );
            }
            if let Some(end) = end {
                rewrite_expr(
                    inference,
                    end,
                    generic_defs,
                    queue,
                    queued,
                    symbols,
                    symbol_keys,
                    errors,
                );
            }
        }
        TypedExprKind::StructLiteral { fields, .. } => {
            for (_, field) in fields {
                rewrite_expr(
                    inference,
                    field,
                    generic_defs,
                    queue,
                    queued,
                    symbols,
                    symbol_keys,
                    errors,
                );
            }
        }
        TypedExprKind::EnumConstruct { fields, .. } => {
            for (_, field) in fields {
                rewrite_expr(
                    inference,
                    field,
                    generic_defs,
                    queue,
                    queued,
                    symbols,
                    symbol_keys,
                    errors,
                );
            }
        }
        TypedExprKind::Cast { expr, .. } => rewrite_expr(
            inference,
            expr,
            generic_defs,
            queue,
            queued,
            symbols,
            symbol_keys,
            errors,
        ),
        TypedExprKind::FmtString(parts) => {
            for part in parts {
                if let TypedFmtStringPart::Expr(expr) = part {
                    rewrite_expr(
                        inference,
                        expr,
                        generic_defs,
                        queue,
                        queued,
                        symbols,
                        symbol_keys,
                        errors,
                    );
                }
            }
        }
        TypedExprKind::Int(_)
        | TypedExprKind::Float(_)
        | TypedExprKind::Bool(_)
        | TypedExprKind::String(_)
        | TypedExprKind::Unit
        | TypedExprKind::Null
        | TypedExprKind::AssociatedConst { .. }
        | TypedExprKind::Identifier(_) => {}
    }
}

fn rewrite_pattern(pattern: &mut TypedPattern) {
    match &mut pattern.kind {
        TypedPatternKind::Variant { fields, .. } | TypedPatternKind::Or(fields) => {
            for field in fields {
                rewrite_pattern(field);
            }
        }
        TypedPatternKind::Struct { fields, .. } => {
            for (_, field, _) in fields {
                rewrite_pattern(field);
            }
        }
        TypedPatternKind::Wildcard
        | TypedPatternKind::Binding(_)
        | TypedPatternKind::Int(_)
        | TypedPatternKind::String(_)
        | TypedPatternKind::Bool(_) => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn rewrite_generic_call(
    inference: &mut TypeInference,
    callee: &mut TypedExpr,
    args: &[TypedExpr],
    call_ty: &mut InferType,
    call_span: aelys_syntax::Span,
    generic_defs: &HashMap<String, TypedFunction>,
    queue: &mut VecDeque<InstanceFrame>,
    queued: &mut HashSet<InstanceKey>,
    symbols: &mut HashMap<InstanceKey, String>,
    symbol_keys: &mut HashMap<String, InstanceKey>,
    errors: &mut Vec<TypeError>,
) {
    let name = match &callee.kind {
        TypedExprKind::Identifier(name) => name,
        TypedExprKind::StructMethod { .. } => {
            check_method_call_obligations(inference, callee, args, call_ty, call_span, errors);
            return;
        }
        _ => return,
    };
    let Some(template) = generic_defs.get(name) else {
        return;
    };
    let Some(args_for_instance) = instance_arguments(template, callee, args, call_ty) else {
        errors.push(TypeError {
            kind: TypeErrorKind::UnresolvedGenericType {
                name: template
                    .type_params
                    .first()
                    .cloned()
                    .unwrap_or_else(|| name.clone()),
            },
            span: call_span,
            reason: ConstraintReason::Other("generic call has no concrete instance".to_string()),
        });
        return;
    };
    if args_for_instance.iter().any(|ty| !ty.is_concrete()) {
        errors.push(TypeError {
            kind: TypeErrorKind::UnresolvedGenericType {
                name: template
                    .type_params
                    .first()
                    .cloned()
                    .unwrap_or_else(|| name.clone()),
            },
            span: call_span,
            reason: ConstraintReason::Other("generic call has an open type".to_string()),
        });
        return;
    }
    let key = InstanceKey {
        name: name.clone(),
        args: args_for_instance,
    };
    let recursive_edge =
        inference
            .monomorphization_active
            .iter()
            .rev()
            .position(|(frame_name, frame_args)| {
                *frame_name == key.name
                    && *frame_args != key.args
                    && !is_strictly_decreasing(&key.args, frame_args)
            });
    if let Some(depth) = recursive_edge {
        let reason = if depth == 0 {
            "generic recursion changes the active type instance"
        } else {
            "mutually recursive generic instances do not decrease"
        };
        errors.push(TypeError {
            kind: TypeErrorKind::RecursiveMonomorphization {
                name: key.name.clone(),
            },
            span: call_span,
            reason: ConstraintReason::Other(reason.to_string()),
        });
        return;
    }
    let resolved = template
        .type_params
        .iter()
        .cloned()
        .zip(key.args.iter().cloned())
        .collect::<HashMap<_, _>>();
    check_instance_trait_bounds(inference, name, &resolved, call_span, errors);
    let symbol = instance_symbol(&key, symbols, symbol_keys, errors, call_span);
    if inference.monomorphization_active.len() >= MAX_ACTIVE_INSTANCES {
        errors.push(TypeError {
            kind: TypeErrorKind::MonomorphizationLimit {
                name: key.name.clone(),
            },
            span: call_span,
            reason: ConstraintReason::Other("active generic instantiation stack".to_string()),
        });
        return;
    }
    if !queued.contains(&key) && queued.len() >= MAX_INSTANCES {
        errors.push(TypeError {
            kind: TypeErrorKind::MonomorphizationLimit {
                name: key.name.clone(),
            },
            span: call_span,
            reason: ConstraintReason::Other("total reachable generic instances".to_string()),
        });
        return;
    }
    if queued.insert(key.clone()) {
        queue.push_back(InstanceFrame {
            key: key.clone(),
            ancestors: inference.monomorphization_active.clone(),
        });
    }
    callee.kind = TypedExprKind::Identifier(symbol);
    let mut substitution = Substitution::new();
    for (param, ty) in template.type_params.iter().zip(&key.args) {
        substitution.bind_param(param.clone(), ty.clone());
    }
    callee.ty = substitution.apply(&callee.ty);
    *call_ty = substitution.apply(call_ty);
}

fn check_instance_trait_bounds(
    inference: &TypeInference,
    name: &str,
    resolved: &HashMap<String, InferType>,
    span: aelys_syntax::Span,
    errors: &mut Vec<TypeError>,
) {
    let Some(bounds) = inference.generic_function_bounds.get(name) else {
        check_instance_trait_bindings(inference, name, resolved, span, errors);
        return;
    };
    let substitution = subject_substitution(resolved);
    for (subject, trait_name, trait_args) in bounds {
        let Some(actual) = subject_type(inference, resolved, subject) else {
            continue;
        };
        let trait_args = trait_args
            .iter()
            .map(|argument| substitution.apply(argument))
            .collect::<Vec<_>>();
        let satisfied = inference
            .type_table
            .satisfies_bound(trait_name, &actual, &trait_args);
        if !satisfied {
            let denied = inference
                .type_table
                .denies(trait_name, &actual, &trait_args);
            errors.push(TypeError {
                kind: TypeErrorKind::UnsatisfiedTraitBound {
                    trait_name: trait_name.clone(),
                    trait_args: trait_args.clone(),
                    ty: actual,
                    denied,
                },
                span,
                reason: ConstraintReason::Other("generic trait bound".to_string()),
            });
        }
    }
    check_instance_trait_bindings(inference, name, resolved, span, errors);
}

fn subject_substitution(resolved: &HashMap<String, InferType>) -> Substitution {
    let mut substitution = Substitution::new();
    for (param, ty) in resolved {
        substitution.bind_param(param.clone(), ty.clone());
    }
    substitution
}

// an unresolved subject is either the nominal that `where counter: source`
fn subject_type(
    inference: &TypeInference,
    resolved: &HashMap<String, InferType>,
    subject: &str,
) -> Option<InferType> {
    match resolved.get(subject) {
        Some(actual) => Some(actual.clone()),
        None if inference.type_table.has_nominal(subject) => {
            Some(InferType::Struct(subject.to_string()))
        }
        None => None,
    }
}

fn check_instance_trait_bindings(
    inference: &TypeInference,
    name: &str,
    resolved: &HashMap<String, InferType>,
    span: aelys_syntax::Span,
    errors: &mut Vec<TypeError>,
) {
    let Some(bindings) = inference.generic_function_bindings.get(name) else {
        return;
    };
    let substitution = subject_substitution(resolved);
    for (subject, trait_name, items) in bindings {
        let Some(actual) = subject_type(inference, resolved, subject) else {
            continue;
        };
        for (item, requested) in items {
            let requested = match requested {
                crate::infer::BoundItem::Type(ty) => {
                    crate::infer::BoundItem::Type(substitution.apply(ty))
                }
                crate::infer::BoundItem::Const(value) => crate::infer::BoundItem::Const(*value),
            };
            let projection = InferType::Projection {
                trait_name: Some(trait_name.clone()),
                item: item.clone(),
                self_ty: Box::new(actual.clone()),
            };
            if inference.type_table.get_trait(trait_name).is_none() {
                continue;
            }
            // lookups walk the super bounds.
            let declares_const = inference
                .type_table
                .trait_declaring_item_in(
                    trait_name,
                    item,
                    Some(crate::constraint::ItemNamespace::Const),
                )
                .is_some();
            let declares_type = inference
                .type_table
                .trait_declaring_item_in(
                    trait_name,
                    item,
                    Some(crate::constraint::ItemNamespace::Type),
                )
                .is_some();
            let is_associated_const = match requested {
                crate::infer::BoundItem::Const(_) => declares_const,
                crate::infer::BoundItem::Type(_) => !declares_type,
            };
            if !declares_const && !declares_type {
                errors.push(TypeError {
                    kind: TypeErrorKind::UndeclaredAssociatedBinding {
                        trait_name: trait_name.clone(),
                        item: item.clone(),
                    },
                    span,
                    reason: binding_reason(subject),
                });
                continue;
            }
            if is_associated_const {
                check_associated_const_binding(
                    inference,
                    trait_name,
                    item,
                    subject,
                    &requested,
                    &projection,
                    span,
                    errors,
                );
                continue;
            }
            // monomorphization, so dropping it here loses no diagnostic.
            let crate::infer::BoundItem::Type(requested) = requested else {
                continue;
            };
            let requested = inference
                .type_table
                .resolve_projection(&requested)
                .unwrap_or(requested);
            match inference.type_table.resolve_projection(&projection) {
                Some(found) if inference.type_table.types_match(&requested, &found) => {}
                Some(found) => {
                    errors.push(TypeError {
                        kind: TypeErrorKind::AssociatedBindingMismatch {
                            trait_name: trait_name.clone(),
                            item: item.clone(),
                            requested,
                            found,
                        },
                        span,
                        reason: binding_reason(subject),
                    });
                }
                None => {
                    errors.push(TypeError {
                        kind: TypeErrorKind::AssociatedBindingMismatch {
                            trait_name: trait_name.clone(),
                            item: item.clone(),
                            requested,
                            found: InferType::Poison,
                        },
                        span,
                        reason: binding_reason(subject),
                    });
                }
            }
        }
    }
}

// an associated-const binding cannot be compared as a type: `resolve_projection`
#[allow(clippy::too_many_arguments)]
fn check_associated_const_binding(
    inference: &TypeInference,
    trait_name: &str,
    item: &str,
    subject: &str,
    requested: &crate::infer::BoundItem,
    projection: &InferType,
    span: aelys_syntax::Span,
    errors: &mut Vec<TypeError>,
) {
    let requested_value = match requested {
        crate::infer::BoundItem::Const(value) => Some(*value),
        crate::infer::BoundItem::Type(ty) => associated_const_value(inference, ty),
    };
    let found_value = associated_const_value(inference, projection);
    let (Some(left), Some(right)) = (requested_value, found_value) else {
        let mut unevaluated = Vec::new();
        if let (None, crate::infer::BoundItem::Type(ty)) = (requested_value, requested) {
            unevaluated.push(ty.clone());
        }
        if found_value.is_none() {
            unevaluated.push(projection.clone());
        }
        errors.push(TypeError {
            kind: TypeErrorKind::UnevaluatedAssociatedConstBinding {
                trait_name: trait_name.to_string(),
                item: item.to_string(),
                unevaluated,
            },
            span,
            reason: binding_reason(subject),
        });
        return;
    };
    if left == right {
        return;
    }
    errors.push(TypeError {
        kind: TypeErrorKind::AssociatedConstBindingMismatch {
            trait_name: trait_name.to_string(),
            item: item.to_string(),
            requested: left,
            found: right,
        },
        span,
        reason: binding_reason(subject),
    });
}

fn binding_reason(subject: &str) -> ConstraintReason {
    ConstraintReason::Other(format!("a bound on '{subject}'"))
}

fn associated_const_value(inference: &TypeInference, ty: &InferType) -> Option<i64> {
    let InferType::Projection {
        trait_name,
        item,
        self_ty,
    } = ty
    else {
        return None;
    };
    let receiver = match self_ty.as_ref() {
        InferType::Struct(name) => name.clone(),
        InferType::Applied { name, .. } => name.clone(),
        other => other.to_string(),
    };
    // table cannot key them apart, so the written qualifier picks the impl.
    if let Some(trait_name) = trait_name {
        let candidates = inference.associated_const_candidates(&receiver, item);
        if candidates.len() > 1 {
            let declaring = inference
                .type_table
                .trait_declaring_item(trait_name, item)
                .unwrap_or_else(|| trait_name.clone());
            return candidates
                .into_iter()
                .find(|(name, _, _)| *name == declaring)
                .and_then(|(_, _, value)| value);
        }
    }
    match inference
        .associated_const_resolutions()
        .get(&(receiver, item.clone()))
    {
        Some(crate::infer::ConstResolution::Value(value)) => Some(*value),
        _ => None,
    }
}

// a receiver call names its callee by the mangled impl symbol, so the instance
fn check_method_call_obligations(
    inference: &TypeInference,
    callee: &TypedExpr,
    args: &[TypedExpr],
    call_ty: &InferType,
    span: aelys_syntax::Span,
    errors: &mut Vec<TypeError>,
) {
    let TypedExprKind::StructMethod {
        object,
        symbol,
        separator,
        ..
    } = &callee.kind
    else {
        return;
    };
    let Some(signature) = inference.impl_method_signatures.get(symbol) else {
        return;
    };
    let mut actuals = Vec::with_capacity(args.len() + 1);
    if *separator == aelys_syntax::MemberSeparator::Dot {
        actuals.push(object.ty.clone());
    }
    actuals.extend(args.iter().map(|arg| arg.ty.clone()));
    let mut resolved = HashMap::new();
    for (formal, actual) in signature.params.iter().zip(&actuals) {
        match_types(formal, actual, &mut resolved);
    }
    match_types(&signature.return_type, call_ty, &mut resolved);
    resolved.retain(|_, ty| ty.is_concrete());
    check_instance_trait_bounds(inference, symbol, &resolved, span, errors);
}

fn instance_arguments(
    template: &TypedFunction,
    callee: &TypedExpr,
    args: &[TypedExpr],
    call_ty: &InferType,
) -> Option<Vec<InferType>> {
    let mut mapping = HashMap::new();
    for (formal, actual) in template.params.iter().zip(args) {
        match_types(&formal.ty, &actual.ty, &mut mapping)?;
    }
    if let InferType::Function { params, ret } = &callee.ty {
        for (formal, actual) in template.params.iter().map(|param| &param.ty).zip(params) {
            match_types(formal, actual, &mut mapping)?;
        }
        match_types(&template.return_type, ret, &mut mapping)?;
    }
    match_types(&template.return_type, call_ty, &mut mapping)?;
    template
        .type_params
        .iter()
        .map(|name| mapping.get(name).cloned())
        .collect()
}

pub(crate) fn match_types(
    formal: &InferType,
    actual: &InferType,
    mapping: &mut HashMap<String, InferType>,
) -> Option<()> {
    match (formal, actual) {
        (InferType::Param(name), actual) => {
            if let Some(previous) = mapping.get(name) {
                (previous == actual).then_some(())
            } else {
                mapping.insert(name.clone(), actual.clone());
                Some(())
            }
        }
        (
            InferType::Function {
                params,
                ret: formal_ret,
            },
            InferType::Function {
                params: actual_params,
                ret: actual_ret,
            },
        ) if params.len() == actual_params.len() => {
            for (formal, actual) in params.iter().zip(actual_params) {
                match_types(formal, actual, mapping)?;
            }
            match_types(formal_ret, actual_ret, mapping)
        }
        (InferType::Array(formal), InferType::Array(actual))
        | (InferType::Vec(formal), InferType::Vec(actual))
        | (InferType::Option(formal), InferType::Option(actual)) => {
            match_types(formal, actual, mapping)
        }
        (InferType::FixedArray(formal, formal_len), InferType::FixedArray(actual, actual_len))
            if formal_len == actual_len =>
        {
            match_types(formal, actual, mapping)
        }
        (InferType::Result(formal_ok, formal_err), InferType::Result(actual_ok, actual_err)) => {
            match_types(formal_ok, actual_ok, mapping)?;
            match_types(formal_err, actual_err, mapping)
        }
        (
            InferType::Projection {
                item: formal_item,
                self_ty: formal_self,
                ..
            },
            InferType::Projection {
                item: actual_item,
                self_ty: actual_self,
                ..
            },
        ) if formal_item == actual_item => match_types(formal_self, actual_self, mapping),
        (
            InferType::Applied {
                name: formal_name,
                args: formal_args,
            },
            InferType::Applied {
                name: actual_name,
                args: actual_args,
            },
        ) if formal_name == actual_name && formal_args.len() == actual_args.len() => {
            for (formal, actual) in formal_args.iter().zip(actual_args) {
                match_types(formal, actual, mapping)?;
            }
            Some(())
        }
        (InferType::Projection { .. }, _) => Some(()),
        _ if formal == actual || matches!(formal, InferType::Var(_)) => Some(()),
        _ => None,
    }
}

fn instance_symbol(
    key: &InstanceKey,
    symbols: &mut HashMap<InstanceKey, String>,
    symbol_keys: &mut HashMap<String, InstanceKey>,
    errors: &mut Vec<TypeError>,
    span: aelys_syntax::Span,
) -> String {
    if let Some(symbol) = symbols.get(key) {
        return symbol.clone();
    }
    let canonical = format_type_application(&key.name, &key.args);
    let mut encoded = String::with_capacity(canonical.len() * 2);
    for byte in canonical.bytes() {
        use std::fmt::Write;
        let _ = write!(encoded, "{byte:02x}");
    }
    let symbol = format!("{GENERIC_INSTANCE_PREFIX}{encoded}");
    if let Some(previous) = symbol_keys.get(&symbol)
        && previous != key
    {
        errors.push(TypeError {
            kind: TypeErrorKind::MangledSymbolCollision {
                name: key.name.clone(),
            },
            span,
            reason: ConstraintReason::Other("generic instance symbol collision".to_string()),
        });
    }
    symbols.insert(key.clone(), symbol.clone());
    symbol_keys.insert(symbol.clone(), key.clone());
    symbol
}
