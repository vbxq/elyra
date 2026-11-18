mod borrows;
mod captures;
pub(crate) mod constants;
mod constraints;
pub mod entry;
mod exhaustive;
mod expr;
mod expr_sum;
mod finalize;
mod functions;
pub mod imports;
mod lambda;
pub(crate) mod monomorphize;
mod must_use;
mod places;
mod returns;
mod signatures;
mod stmt;
mod structs;
mod substitute;

use crate::constraint::{Constraint, ConstraintReason, TypeError, TypeErrorKind};
use crate::env::TypeEnv;
use crate::types::{InferType, TypeTable, TypeVarGen};
use aelys_common::Warning;
use aelys_syntax::{ModuleId, ReferenceKind};
use std::collections::{HashMap, HashSet};

const MAX_INFERENCE_DEPTH: usize = 1024;

pub const GENERATED_SYMBOL_PREFIX: &str = "__aelys_";

// global layout has to tell a mangled method from a `module::function` import
pub fn is_mangled_symbol(symbol: &str) -> bool {
    let Some(tail) = symbol.strip_prefix(GENERATED_SYMBOL_PREFIX) else {
        return false;
    };
    let Some((_, fields)) = tail.split_once("::") else {
        return false;
    };
    let bytes = fields.as_bytes();
    bytes.len() > 8 && bytes[8] == b':' && bytes[..8].iter().all(u8::is_ascii_hexdigit)
}

pub use aelys_common::naming::{
    MODULE_GLOBAL_PREFIX, is_module_scoped_global, module_scoped_global, unscoped_global_name,
};

// a vec grows through push and through any `mut` parameter it is handed, and a mutable array binding can be replaced from another function or closure
pub(crate) fn length_is_fixed(ty: &InferType, mutable: bool) -> bool {
    match ty {
        InferType::FixedArray(_, _) => true,
        InferType::Array(_) => !mutable,
        _ => false,
    }
}

#[cfg(test)]
mod mangled_symbol_tests {
    use super::is_mangled_symbol;

    #[test]
    fn a_mangled_method_is_recognised() {
        assert!(is_mangled_symbol(&super::functions::struct_method_symbol(
            "Point", "score"
        )));
        assert!(is_mangled_symbol(&super::functions::trait_method_symbol(
            "Scored",
            "Point",
            "score",
            &[]
        )));
        assert!(is_mangled_symbol(
            &super::monomorphize::bound_marker_symbol("Scored", "T", "score")
        ));
    }

    #[test]
    fn a_module_named_like_the_prefix_is_not_mangled() {
        assert!(!is_mangled_symbol("__aelys_struct::helper"));
        assert!(!is_mangled_symbol("__aelys_trait::helper"));
        assert!(!is_mangled_symbol("io::println"));
        assert!(!is_mangled_symbol("println"));
    }
}

#[derive(Debug, Clone)]
pub(crate) enum BoundItem {
    Type(InferType),
    Const(i64),
}

pub(crate) type AssociatedBindings = Vec<(String, String, Vec<(String, BoundItem)>)>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OccurrenceRole {
    Parameter,
    ReturnType,
    StructField,
    EnumVariantField,
    Bound,
    ImplHeader,
    ItemDefinition,
}

impl OccurrenceRole {
    pub(crate) fn describe(self) -> &'static str {
        match self {
            Self::Parameter => "a parameter type",
            Self::ReturnType => "a return type",
            Self::StructField => "a struct field",
            Self::EnumVariantField => "an enum variant field",
            Self::Bound => "a bound",
            Self::ImplHeader => "an impl header",
            Self::ItemDefinition => "an associated item definition",
        }
    }
}

/// keyed by the mangled impl symbol: a receiver call reaches no generic
pub(crate) struct ImplMethodSignature {
    pub(crate) params: Vec<InferType>,
    pub(crate) return_type: InferType,
}

/// only monomorphization can say what it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProjectionNamespace {
    Found,
    Opaque,
    WrongNamespace {
        found: crate::constraint::ItemNamespace,
    },
    Absent,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ConstResolution {
    Value(i64),
    Missing,
    Ambiguous(Vec<String>),
    AmbiguousInstantiations {
        trait_name: String,
        constructor: String,
        instantiations: Vec<String>,
    },
    Cyclic,
    /// checked arithmetic refused: overflow, or division/modulo by zero.
    NotComputable,
    NotConstant,
}

pub(crate) struct ConstEvalState {
    visiting: Vec<(String, String)>,
    cache: HashMap<(String, String), ConstResolution>,
}

impl ConstEvalState {
    fn new(root: (String, String)) -> Self {
        Self {
            visiting: vec![root],
            cache: HashMap::new(),
        }
    }
}

const KNOWN_TYPE_NAMES: &[&str] = &[
    "int", "i8", "i16", "i32", "i64", "int8", "int16", "int32", "int64", "u8", "u16", "u32", "u64",
    "uint8", "uint16", "uint32", "uint64", "float", "f32", "f64", "float32", "float64", "bool",
    "string", "char", "void", "unit", "dynamic", "array", "vec", "map", "option", "result",
    "error",
];

pub struct TypeInference {
    type_gen: TypeVarGen,
    constraints: Vec<Constraint>,
    env: TypeEnv,
    errors: Vec<TypeError>,
    return_type_stack: Vec<InferType>,
    depth: usize,
    warnings: Vec<Warning>,
    pub(crate) type_table: TypeTable,
    type_params_in_scope: Vec<String>,
    /// a method that redeclares a parameter of its impl or of the nominal it is
    method_param_renames: std::collections::HashMap<String, String>,
    freshened_own_params: Option<Vec<InferType>>,
    /// impls withdrawn because a later impl clashes with them: both of a pair go
    retracted_by_overlap: std::collections::HashSet<usize>,
    trait_defaults: HashMap<(String, String), aelys_syntax::Function>,
    generic_function_bounds: HashMap<String, Vec<(String, String, Vec<InferType>)>>,
    function_type_params: HashMap<String, Vec<String>>,
    function_reference_modes: HashMap<String, Vec<Option<ReferenceKind>>>,
    allow_reference_annotation: bool,
    allow_direct_borrow: bool,
    callee_position: bool,
    borrow_call_scopes: Vec<Vec<borrows::ActiveLoan>>,
    forwarded_mutable_borrows: HashSet<String>,
    try_residuals: Vec<expr_sum::TryResidual>,
    try_conversions: HashMap<(usize, usize), (String, InferType, InferType)>,
    /// with `&self`, so its verdicts are filed here and drained once
    conversion_verdicts: std::cell::RefCell<
        Vec<(
            crate::types::FromSelection,
            InferType,
            InferType,
            aelys_syntax::Span,
        )>,
    >,
    must_use_values: Vec<expr_sum::MustUseResidual>,
    sum_method_residuals: Vec<expr_sum::SumMethodResidual>,
    match_exhaustivity_residuals: Vec<expr_sum::MatchExhaustivityResidual>,
    loop_frames: Vec<stmt::LoopFrame>,
    match_unit_residuals: Vec<(InferType, Vec<aelys_syntax::Span>)>,
    dynamic_residuals: Vec<DynamicResidual>,
    bound_residuals: Vec<BoundResidual>,
    bound_deductions: Vec<BoundDeduction>,
    alias_residuals: Vec<places::AliasResidual>,
    imported_constants: HashMap<String, constants::ConstValue>,
    global_constants: HashMap<String, constants::ConstValue>,
    exported_constants: std::collections::BTreeMap<String, constants::ConstValue>,
    function_mutable_params: HashMap<String, Vec<Option<String>>>,
    fall_through_residuals: Vec<(InferType, aelys_syntax::Span, String)>,
    statement_match: bool,
    script_tail: bool,
    surface_dynamic_spans: HashSet<(usize, usize)>,
    pending_int_literals: Vec<(InferType, Vec<(i64, aelys_syntax::Span)>)>,
    nominal_expectation: Option<InferType>,
    constructor_expectation: Option<InferType>,
    sum_type_residuals: Vec<expr_sum::SumTypeResidual>,
    explicit_dynamic_functions: HashSet<String>,
    module_aliases: HashSet<String>,
    known_globals: HashSet<String>,
    globals_without_signature: HashSet<String>,
    known_native_globals: HashSet<String>,
    known_native_signatures: HashMap<String, InferType>,
    pub(crate) current_module: ModuleId,
    collection_iter_allowed: bool,
    receiver_memo: Option<ReceiverMemo>,
    withheld_nominals: std::collections::BTreeMap<String, String>,
    private_nominals: std::collections::BTreeMap<String, String>,
    unexported_nominals: std::collections::BTreeSet<String>,
    root_module: ModuleId,
    module_globals:
        std::collections::BTreeMap<String, std::collections::BTreeMap<String, InferType>>,
    module_scoped_globals: std::collections::BTreeMap<String, std::collections::BTreeSet<String>>,
    module_scopes: std::collections::BTreeMap<String, imports::ModuleScope>,
    ambiguous_imports: std::collections::BTreeMap<String, Vec<String>>,
    immutable_imports: std::collections::BTreeSet<String>,
    scope_own_globals: bool,
    pub(crate) monomorphization_active: Vec<(String, Vec<InferType>)>,
    current_trait_name: Option<String>,
    current_trait_associated_items: Vec<String>,
    projected_associated_const_types: Vec<ProjectedAssociatedConstType>,
    nominal_parameter_scope: bool,
    /// bounds of the function currently being collected, for `t::item` projections.
    current_function_bounds: Vec<(String, String, Vec<InferType>)>,
    current_function_bindings: AssociatedBindings,
    /// associated bindings per (function, subject param): `item = int` bounds.
    generic_function_bindings: HashMap<String, AssociatedBindings>,
    impl_method_signatures: HashMap<String, ImplMethodSignature>,
    impl_method_symbol_targets: HashMap<String, InferType>,
    /// deeply nested typed tree cannot overflow the stack.
    substitution_depth: std::cell::Cell<usize>,
    associated_type_definitions: Vec<AssociatedTypeDefinition>,
    /// memo for `associated_const_resolutions`, which the monomorphizer asks for at
    associated_const_resolution_cache:
        std::cell::RefCell<Option<HashMap<(String, String), ConstResolution>>>,
    trait_qualified_items: HashMap<(String, String), Vec<String>>,
    /// demand, after every impl is registered, so the result cannot depend on
    associated_const_exprs: HashMap<(String, String), (String, aelys_syntax::Expr)>,
    defer_projection_resolution: bool,
    annotation_namespace: crate::constraint::ItemNamespace,
    occurrence_role: Option<OccurrenceRole>,
    current_impl_self: Option<InferType>,
    in_trait_default_body: bool,
    /// in an impl's bodies, its header, which `Self::Item` is read on
    current_impl_header: Option<InferType>,
    /// in a default body, the trait, header and arguments of the impl adopting it
    adopted_instantiation: Option<(String, InferType, Vec<InferType>)>,
    impls_missing_supertraits:
        std::collections::BTreeMap<(String, String), (InferType, Vec<String>)>,
    /// reject, so the compile reports it instead of yielding a poisoned type.
    projection_cycle_escaped: std::cell::Cell<bool>,
    /// compile reports a recursion-limit error instead of silently emitting a
    substitution_overflowed: std::cell::Cell<Option<aelys_syntax::Span>>,
    specialization_verdicts: std::cell::RefCell<
        Vec<(
            crate::types::SpecializationChoice,
            InferType,
            aelys_syntax::Span,
        )>,
    >,
}

struct AssociatedTypeDefinition {
    receiver: String,
    item: String,
    namespace: crate::constraint::ItemNamespace,
    edges: Vec<(String, String)>,
    span: aelys_syntax::Span,
}

pub(crate) struct ProjectedAssociatedConstType {
    pub(crate) trait_name: String,
    pub(crate) item: String,
    pub(crate) declared: InferType,
    pub(crate) span: aelys_syntax::Span,
}

struct DynamicResidual {
    found: InferType,
    expected: InferType,
    span: aelys_syntax::Span,
    reason: ConstraintReason,
}

pub(crate) struct BoundDeduction {
    pub(crate) subject: InferType,
    pub(crate) trait_name: String,
    pub(crate) trait_args: Vec<InferType>,
}

pub(crate) struct BoundResidual {
    pub(crate) ty: InferType,
    pub(crate) trait_name: String,
    pub(crate) trait_args: Vec<InferType>,
    pub(crate) span: aelys_syntax::Span,
    pub(crate) reason: ConstraintReason,
    pub(crate) nominal_only: bool,
    // a receiver declaring a method of this name runs that method, so the bound does not apply to it
    pub(crate) unless_method: Option<&'static str>,
}

impl TypeInference {
    pub(crate) fn fresh_param_modes(&mut self, count: usize) -> crate::types::ParamModes {
        crate::types::ParamModes::from_modes(
            (0..count)
                .map(|_| crate::types::ParamMode::Var(self.type_gen.fresh_id()))
                .collect(),
        )
    }
}

impl TypeInference {
    pub(super) fn reject_untyped_native(
        &mut self,
        found: &InferType,
        _expected: &InferType,
        span: aelys_syntax::Span,
        reason: ConstraintReason,
    ) -> bool {
        let InferType::UntypedNative(name) = found else {
            return false;
        };
        self.errors.push(TypeError {
            kind: TypeErrorKind::UntypedNativeBoundary { name: name.clone() },
            span,
            reason,
        });
        true
    }

    pub(super) fn reject_dynamic(
        &mut self,
        found: &InferType,
        expected: &InferType,
        span: aelys_syntax::Span,
        reason: ConstraintReason,
    ) -> bool {
        if let Some(expected) = dynamic_target(found, expected) {
            self.dynamic_residuals.push(DynamicResidual {
                found: InferType::Dynamic,
                expected,
                span,
                reason,
            });
            return true;
        }
        if !contains_concrete_dynamic(found, expected) {
            if (found.contains_dynamic() && matches!(expected, InferType::Var(_)))
                || (found.has_vars() && !matches!(expected, InferType::Dynamic | InferType::Var(_)))
            {
                self.dynamic_residuals.push(DynamicResidual {
                    found: found.clone(),
                    expected: expected.clone(),
                    span,
                    reason,
                });
            }
            return false;
        }
        self.errors.push(TypeError {
            kind: TypeErrorKind::Mismatch {
                expected: expected.clone(),
                found: found.clone(),
            },
            span,
            reason,
        });
        true
    }
}

fn dynamic_target(found: &InferType, expected: &InferType) -> Option<InferType> {
    match (found, expected) {
        (InferType::Dynamic, InferType::Var(_)) => Some(expected.clone()),
        (InferType::Option(found), InferType::Option(expected))
        | (InferType::Array(found), InferType::Array(expected))
        | (InferType::Vec(found), InferType::Vec(expected)) => dynamic_target(found, expected),
        (InferType::FixedArray(found, _), InferType::FixedArray(expected, _)) => {
            dynamic_target(found, expected)
        }
        (InferType::Result(found_ok, found_err), InferType::Result(expected_ok, expected_err))
        | (InferType::Map(found_ok, found_err), InferType::Map(expected_ok, expected_err)) => {
            dynamic_target(found_ok, expected_ok)
                .or_else(|| dynamic_target(found_err, expected_err))
        }
        (InferType::Tuple(found), InferType::Tuple(expected)) => found
            .iter()
            .zip(expected.iter())
            .find_map(|(found, expected)| dynamic_target(found, expected)),
        (
            InferType::Function {
                params: found_params,
                ret: found_ret,
                ..
            },
            InferType::Function {
                params: expected_params,
                ret: expected_ret,
                ..
            },
        ) => found_params
            .iter()
            .zip(expected_params.iter())
            .find_map(|(found, expected)| dynamic_target(found, expected))
            .or_else(|| dynamic_target(found_ret, expected_ret)),
        _ => None,
    }
}

impl TypeInference {
    pub(super) fn validate_dynamic_residuals(&mut self, subst: &crate::unify::Substitution) {
        for residual in &self.dynamic_residuals {
            let found = subst.apply(&residual.found);
            let expected = subst.apply(&residual.expected);
            if matches!(expected, InferType::Var(_) | InferType::Dynamic) {
                continue;
            }
            if !contains_concrete_dynamic(&found, &expected) {
                continue;
            }
            self.errors.push(TypeError {
                kind: TypeErrorKind::Mismatch { expected, found },
                span: residual.span,
                reason: residual.reason.clone(),
            });
        }
    }

    pub(super) fn validate_fall_through_residuals(&mut self, subst: &crate::unify::Substitution) {
        for (ty, span, func_name) in std::mem::take(&mut self.fall_through_residuals) {
            let expected = subst.apply(&ty);
            if !expected.is_concrete()
                || matches!(
                    expected,
                    InferType::Unit | InferType::Never | InferType::Dynamic
                )
            {
                continue;
            }
            self.errors.push(TypeError {
                kind: TypeErrorKind::MissingReturnValue { expected },
                span,
                reason: ConstraintReason::Return { func_name },
            });
        }
    }

    pub(super) fn validate_bound_residuals(&mut self, subst: &crate::unify::Substitution) {
        let mut reported = Vec::new();
        for residual in &self.bound_residuals {
            let ty = subst.apply(&residual.ty);
            if !ty.is_concrete() {
                continue;
            }
            let checked = if residual.trait_name == crate::prelude::DISPLAY_TRAIT {
                crate::prelude::contains_nominal(&ty)
            } else {
                matches!(ty, InferType::Struct(_) | InferType::Applied { .. })
            };
            if residual.nominal_only && !checked {
                continue;
            }
            if residual
                .unless_method
                .is_some_and(|method| self.declares_self_method(&ty, method))
            {
                continue;
            }
            let trait_args: Vec<InferType> = residual
                .trait_args
                .iter()
                .map(|arg| subst.apply(arg))
                .collect();
            if self
                .type_table
                .satisfies_bound(&residual.trait_name, &ty, &trait_args)
            {
                continue;
            }
            let denied = self
                .type_table
                .denies(&residual.trait_name, &ty, &trait_args);
            reported.push(TypeError {
                kind: TypeErrorKind::UnsatisfiedTraitBound {
                    trait_name: residual.trait_name.clone(),
                    trait_args: trait_args.clone(),
                    ty,
                    denied,
                },
                span: residual.span,
                reason: residual.reason.clone(),
            });
        }
        self.errors.extend(reported);
    }
}

fn contains_concrete_dynamic(found: &InferType, expected: &InferType) -> bool {
    if matches!(expected, InferType::Dynamic | InferType::Var(_)) {
        return false;
    }
    match (found, expected) {
        (InferType::Dynamic, _) => true,
        (InferType::Option(found), InferType::Option(expected))
        | (InferType::Array(found), InferType::Array(expected))
        | (InferType::Vec(found), InferType::Vec(expected)) => {
            contains_concrete_dynamic(found, expected)
        }
        (InferType::FixedArray(found, _), InferType::FixedArray(expected, _)) => {
            contains_concrete_dynamic(found, expected)
        }
        (InferType::Result(found_ok, found_err), InferType::Result(expected_ok, expected_err))
        | (InferType::Map(found_ok, found_err), InferType::Map(expected_ok, expected_err)) => {
            contains_concrete_dynamic(found_ok, expected_ok)
                || contains_concrete_dynamic(found_err, expected_err)
        }
        (InferType::Tuple(found), InferType::Tuple(expected)) => found
            .iter()
            .zip(expected.iter())
            .any(|(found, expected)| contains_concrete_dynamic(found, expected)),
        (
            InferType::Function {
                params: found_params,
                ret: found_ret,
                ..
            },
            InferType::Function {
                params: expected_params,
                ret: expected_ret,
                ..
            },
        ) => {
            found_params
                .iter()
                .zip(expected_params.iter())
                .any(|(found, expected)| contains_concrete_dynamic(found, expected))
                || (matches!(found_ret.as_ref(), InferType::Dynamic)
                    && matches!(expected_ret.as_ref(), InferType::Var(_)))
                || contains_concrete_dynamic(found_ret, expected_ret)
        }
        _ => false,
    }
}

pub(crate) use crate::types::SHADOWED_METHOD_SUFFIX;

pub(crate) fn shadowed_method_param(name: &str) -> String {
    format!("{name}{SHADOWED_METHOD_SUFFIX}")
}

struct ReceiverMemo {
    object: *const aelys_syntax::Expr,
    iter_allowed: bool,
    typed: Option<crate::typed_ast::TypedExpr>,
}
