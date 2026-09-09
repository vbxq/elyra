use super::ConstraintReason;
use crate::types::{InferType, TypeVarId};
use aelys_syntax::{ModuleId, Span};
use std::fmt;

#[derive(Debug, Clone)]
pub struct TypeError {
    pub kind: TypeErrorKind,
    pub span: Span,
    pub reason: ConstraintReason,
}

impl TypeError {
    pub fn diagnostic_code(&self) -> u16 {
        self.kind.diagnostic_code()
    }

    pub fn defining_module(&self) -> Option<&str> {
        self.reason.defining_module()
    }
}

/// the position a projection is written in names the namespace, never its
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemNamespace {
    Type,
    Const,
}

impl ItemNamespace {
    pub fn other(self) -> Self {
        match self {
            Self::Type => Self::Const,
            Self::Const => Self::Type,
        }
    }

    pub fn noun(self) -> &'static str {
        match self {
            Self::Type => "associated type",
            Self::Const => "associated constant",
        }
    }

    pub fn keyword(self) -> &'static str {
        match self {
            Self::Type => "type",
            Self::Const => "const",
        }
    }

    fn position(self) -> &'static str {
        match self {
            Self::Type => "type",
            Self::Const => "value",
        }
    }
}

#[derive(Debug, Clone)]
pub enum AssociatedItemDisagreement {
    DeclaredType {
        declared: String,
        found: String,
    },
    ConstantValue {
        declared: String,
        found: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub enum ProjectionFailure {
    Unbound,
    NoImpl,
    SelfOutsideImpl,
    NominalParameter,
    WrongNamespace {
        found: ItemNamespace,
    },
    BuiltinReceiver,
    /// the receiver is generic and the definition is one of the impl's parameters
    ReceiverArguments {
        param: String,
    },
    /// the constant is defined, but its value cannot be computed: the checked
    NotComputable,
    NotConstant {
        declared: Option<String>,
    },
    LengthFromTypeParameter,
    UnspecializedGenericMethod,
    Ambiguous {
        traits: Vec<String>,
    },
    AmbiguousImplementors {
        types: Vec<String>,
    },
    AmbiguousInstantiations {
        trait_name: String,
        constructor: String,
        instantiations: Vec<String>,
    },
    Cyclic {
        path: Vec<String>,
        namespace: ItemNamespace,
    },
}

#[derive(Debug, Clone)]
pub enum TypeErrorKind {
    Mismatch {
        expected: InferType,
        found: InferType,
    },
    InfiniteType {
        var: TypeVarId,
        ty: InferType,
    },
    NotOneOf {
        ty: InferType,
        options: Vec<InferType>,
    },
    ArityMismatch {
        expected: usize,
        found: usize,
    },
    NotCallable {
        ty: InferType,
    },
    /// undefined variable
    UndefinedVariable {
        name: String,
    },
    /// undefined function
    UndefinedFunction {
        name: String,
    },
    RecursionLimit,
    NonExhaustiveMatch {
        missing: Vec<String>,
    },
    IgnoredResult,
    IgnoredOption,
    NullIsNotInSurface,
    DynamicIsNotInSurface,
    UntypedNativeValue {
        name: String,
    },
    UntypedNativeBoundary {
        name: String,
    },
    UnmaterializedAppliedType {
        name: String,
    },
    QuestionMarkOutsideResult,
    QuestionMarkTypeMismatch {
        source: InferType,
        target: InferType,
    },
    InvalidTryResidual {
        source: InferType,
        target: InferType,
    },
    UnsatisfiedTryConversion {
        source: InferType,
        target: InferType,
        source_error: InferType,
        target_error: InferType,
        candidates: Vec<String>,
    },
    ReservedIdentityConversion {
        ty: InferType,
    },
    UnresolvedSumType {
        constructor: String,
    },
    UnknownVariant {
        variant: String,
        expected: String,
    },
    InvalidSumMethod {
        method: String,
        receiver: InferType,
    },
    DynamicSumMethod {
        method: String,
    },
    PatternBindingMismatch {
        expected: Vec<String>,
        found: Vec<String>,
    },
    UntypedSumValue {
        name: String,
    },
    MissingReturnValue {
        expected: InferType,
    },
    MatchArmValueRequired,
    GenericArityMismatch {
        name: String,
        expected: usize,
        found: usize,
    },
    UntypedNativeTypeMismatch {
        name: String,
        expected: InferType,
    },
    InvalidIndex {
        receiver: InferType,
    },
    InvalidCollectionMethod {
        method: String,
        receiver: InferType,
    },
    InvalidStringMethod {
        method: String,
        receiver: InferType,
    },
    ConstantIndexOutOfBounds {
        index: i64,
        length: usize,
    },
    NotIterable {
        receiver: InferType,
    },
    UnknownField {
        structure: String,
        field: String,
    },
    MissingField {
        structure: String,
        field: String,
    },
    ModuleMemberNotPublic {
        module: String,
        member: String,
    },
    ModulePathSeparator {
        module: String,
        member: String,
    },
    SizedArrayElementNotDefaultable {
        element: InferType,
    },
    NegativeArraySize {
        size: i64,
        /// `some` when the length was written as a projection rather than a literal
        constant: Option<String>,
    },
    NonConstantArrayRepeat,
    ConstantSliceOutOfBounds {
        start: Option<i64>,
        end: Option<i64>,
        length: usize,
        inclusive: bool,
    },
    MutableCollectionRequired {
        method: String,
        receiver: InferType,
    },
    ReadOnlyCollectionRequired {
        method: String,
        receiver: InferType,
    },
    UnconsumedCollectionIterator,
    CollectionCollectRequiresPipeline,
    MutableCollectionAlias,
    GenericStructDeferred {
        name: String,
    },
    DuplicateNominal {
        name: String,
        keyword: &'static str,
        collides_with: Option<&'static str>,
    },
    DuplicateStructField {
        structure: String,
        field: String,
    },
    UnknownStruct {
        name: String,
    },
    InvalidStructMethod {
        method: String,
        structure: String,
    },
    ImmutableStructField {
        field: String,
    },
    PrivateFieldAccess {
        structure: String,
        field: String,
        owner: ModuleId,
        current: ModuleId,
        operation: String,
    },
    PrivateFieldConstruction {
        structure: String,
        field: String,
        owner: ModuleId,
        current: ModuleId,
        operation: String,
    },
    ImmutableStructMethod {
        method: String,
    },
    SharedLoanMutation {
        place: String,
    },
    MutableLoanOverlap {
        place: String,
    },
    MutableLoanAccess {
        place: String,
        access: String,
    },
    BorrowEscapes {
        place: String,
        context: String,
    },
    BorrowInvalidated {
        place: String,
        context: String,
    },
    TemporaryBorrow {
        place: String,
    },
    UnknownTrait {
        name: String,
    },
    MissingTraitMethod {
        trait_name: String,
        method: String,
    },
    MissingAssociatedItem {
        trait_name: String,
        item: String,
        namespace: ItemNamespace,
        declared_type: Option<String>,
    },
    DuplicateAssociatedItem {
        trait_name: String,
        item: String,
    },
    AssociatedItemTypeMismatch {
        trait_name: String,
        item: String,
        disagreement: AssociatedItemDisagreement,
    },
    AmbiguousAssociatedProjection {
        receiver: String,
        item: String,
        cause: ProjectionFailure,
    },
    AssociatedProjectionLimit {
        receiver: String,
        item: String,
        limit: usize,
    },
    AssociatedItemOutsideTraitImpl {
        target: String,
        item: String,
        keyword: &'static str,
        trait_name: Option<String>,
    },
    UninhabitedNominalCycle {
        keyword: &'static str,
        path: Vec<String>,
    },
    AssociatedBindingMismatch {
        trait_name: String,
        item: String,
        requested: InferType,
        found: InferType,
    },
    AssociatedConstBindingMismatch {
        trait_name: String,
        item: String,
        requested: i64,
        found: i64,
    },
    UndeclaredAssociatedBinding {
        trait_name: String,
        item: String,
    },
    AssociatedBindingNamespaceMismatch {
        trait_name: String,
        item: String,
        /// the right-hand side as written, so the message never renders a type
        value: String,
        declared: ItemNamespace,
    },
    UnevaluatedAssociatedConstBinding {
        trait_name: String,
        item: String,
        unevaluated: Vec<InferType>,
    },
    UnconstrainedImplTypeParam {
        param: String,
        target: String,
        call_site_binds: bool,
    },
    UnfoldableAssociatedConstType {
        trait_name: String,
        item: String,
        declared: String,
    },
    DuplicateTraitImpl {
        trait_name: String,
        target: String,
    },
    TraitMethodNotInTrait {
        trait_name: String,
        method: String,
    },
    TraitMethodSignatureMismatch {
        trait_name: String,
        method: String,
    },
    AmbiguousTraitMethod {
        target: String,
        method: String,
    },
    UnboundTypeParamMethod {
        param: String,
        method: String,
    },
    UnresolvedInstanceSymbol {
        name: String,
    },
    UnsatisfiedTraitBound {
        trait_name: String,
        ty: InferType,
    },
    MissingSupertraitImpl {
        trait_name: String,
        supertrait: String,
        ty: InferType,
    },
    OrphanTraitImpl {
        trait_name: String,
        target: InferType,
    },
    OverlappingTraitImpl {
        trait_name: String,
        target: InferType,
    },
    DuplicateTraitMethod {
        trait_name: String,
        method: String,
    },
    InvalidTraitReceiver {
        trait_name: String,
        method: String,
    },
    UnresolvedGenericType {
        name: String,
    },
    RecursiveMonomorphization {
        name: String,
    },
    MonomorphizationLimit {
        name: String,
    },
    MangledSymbolCollision {
        name: String,
    },
    EnumLayoutTooLarge {
        enum_name: String,
        item: String,
        count: usize,
        limit: usize,
    },
    NonExhaustiveStruct {
        structure: String,
    },
    UnreachablePattern {
        pattern: String,
    },
    UnresolvedTypeVariable,
    PoisonedType,
    UndeterminedType,
    GlobalWithoutSignature {
        name: String,
    },
    NamespaceIsNotAValue {
        name: String,
    },
    NotANamespace {
        name: String,
    },
    NoSuchMember {
        receiver: InferType,
        member: String,
    },
    UnknownTypeName {
        name: String,
    },
    TypeNotImported {
        name: String,
        module: String,
    },
    /// a nominal carried in only for an inlined body, which its own module never exported
    PrivateNominalNotExported {
        name: String,
        module: String,
    },
}

impl fmt::Display for TypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.write_kind(f)?;
        if self.kind.states_its_reason() {
            write!(f, " ({})", self.reason)?;
        }
        Ok(())
    }
}

impl TypeError {
    fn write_kind(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            TypeErrorKind::Mismatch { expected, found } => {
                write!(
                    f,
                    "type mismatch: expected {}, found {} ({})",
                    expected, found, self.reason
                )
            }
            TypeErrorKind::InfiniteType { var, ty } => {
                write!(f, "infinite type: {} = {} ({})", var, ty, self.reason)
            }
            TypeErrorKind::NotOneOf { ty, options } => {
                if let ConstraintReason::CollectionMethodReceiver { method } = &self.reason {
                    let required = match method.as_str() {
                        "push" | "pop" | "capacity" | "reserve" => "a vector",
                        "len" | "is_empty" | "get" => "a string, array, or vector",
                        _ => "an array or vector",
                    };
                    return write!(
                        f,
                        "collection method '{}' requires {}, found {}",
                        method, required, ty
                    );
                }
                let options = options
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(
                    f,
                    "type {} is not one of [{}] ({})",
                    ty, options, self.reason
                )
            }
            TypeErrorKind::ArityMismatch { expected, found } => {
                write!(
                    f,
                    "wrong number of arguments: expected {}, found {} ({})",
                    expected, found, self.reason
                )
            }
            TypeErrorKind::NotCallable { ty } => {
                write!(f, "type {} is not callable ({})", ty, self.reason)
            }
            TypeErrorKind::UndefinedVariable { name } => {
                write!(
                    f,
                    "undefined variable: {}",
                    crate::unscoped_global_name(name)
                )
            }
            TypeErrorKind::UndefinedFunction { name } => {
                write!(
                    f,
                    "undefined function: {}",
                    crate::unscoped_global_name(name)
                )
            }
            TypeErrorKind::RecursionLimit => {
                write!(f, "type inference recursion limit exceeded")
            }
            TypeErrorKind::NonExhaustiveMatch { missing } => {
                write!(
                    f,
                    "non-exhaustive match; missing {}\n   = help: add a missing arm or '_'",
                    missing.join(", ")
                )
            }
            TypeErrorKind::IgnoredResult => write!(f, "unused Result value"),
            TypeErrorKind::IgnoredOption => write!(f, "unused Option value"),
            TypeErrorKind::NullIsNotInSurface => write!(
                f,
                "null is not part of Aelys; use Option for absence or Result for failure"
            ),
            TypeErrorKind::DynamicIsNotInSurface => write!(
                f,
                "dynamic is not part of Aelys; use a concrete type or an explicit enum"
            ),
            TypeErrorKind::UntypedNativeValue { name } => write!(
                f,
                "native value '{}' has no Aelys type; declare its signature before using it",
                name
            ),
            TypeErrorKind::UntypedNativeBoundary { name } => write!(
                f,
                "native value '{}' crosses the typed Aelys boundary without a signature; declare its ABI signature before using it",
                name
            ),
            TypeErrorKind::UnmaterializedAppliedType { name } => write!(
                f,
                "generic type '{}' was not materialized before code generation",
                name
            ),
            TypeErrorKind::QuestionMarkOutsideResult => {
                write!(
                    f,
                    "cannot use '?' here; the enclosing function must return Option or Result"
                )
            }
            TypeErrorKind::QuestionMarkTypeMismatch { source, target } => {
                write!(
                    f,
                    "cannot propagate {} with '?' from a function returning {}",
                    source, target
                )
            }
            TypeErrorKind::InvalidTryResidual { source, target } => {
                let residual = if matches!(source, InferType::Option(_)) {
                    "an Option residual cannot satisfy a Result return"
                } else {
                    "a Result residual cannot satisfy an Option return"
                };
                write!(
                    f,
                    "cannot propagate {} with '?' from a function returning {}: {}",
                    source, target, residual
                )
            }
            TypeErrorKind::UnsatisfiedTryConversion {
                source,
                target,
                source_error,
                target_error,
                candidates,
            } => {
                write!(
                    f,
                    "cannot propagate {} with '?' from a function returning {}: no unique From<{}> for {} conversion",
                    source, target, source_error, target_error
                )?;
                if !candidates.is_empty() {
                    write!(f, "; candidate impls: {}", candidates.join(", "))?;
                }
                write!(f, "; use map_err or convert the error explicitly")
            }
            TypeErrorKind::ReservedIdentityConversion { ty } => write!(
                f,
                "From<{}> for {} is reserved; the compiler already provides the identity conversion",
                ty, ty
            ),
            TypeErrorKind::UnresolvedSumType { constructor } => {
                write!(f, "cannot infer the sum type for {}", constructor)
            }
            TypeErrorKind::UnknownVariant { variant, expected } => {
                write!(f, "unknown variant '{}' for {}", variant, expected)
            }
            TypeErrorKind::InvalidSumMethod { method, receiver } => {
                write!(f, "method '{}' is not available on {}", method, receiver)
            }
            TypeErrorKind::DynamicSumMethod { method } => write!(
                f,
                "dynamic value cannot use sum method '{}'; annotate it as Option<T> or Result<T, E>",
                method
            ),
            TypeErrorKind::PatternBindingMismatch { expected, found } => write!(
                f,
                "or-pattern alternatives must bind the same names; expected {}, found {}",
                expected.join(", "),
                found.join(", ")
            ),
            TypeErrorKind::UntypedSumValue { name } => {
                write!(
                    f,
                    "cannot use untyped native value '{}' as Option or Result",
                    name
                )
            }
            TypeErrorKind::MissingReturnValue { expected } => {
                write!(
                    f,
                    "function can fall through without returning {}",
                    expected
                )
            }
            TypeErrorKind::MatchArmValueRequired => {
                write!(f, "match arm must produce a value or diverge")
            }
            TypeErrorKind::GenericArityMismatch {
                name,
                expected,
                found,
            } => write!(
                f,
                "generic type '{}' expects {} parameter(s), found {}",
                name, expected, found
            ),
            TypeErrorKind::UntypedNativeTypeMismatch { name, expected } => write!(
                f,
                "untyped native '{}' cannot satisfy annotation {}",
                name, expected
            ),
            TypeErrorKind::InvalidIndex { receiver } => {
                write!(f, "cannot index a value of type {}", receiver)
            }
            TypeErrorKind::InvalidCollectionMethod { method, receiver } => {
                let required = match method.as_str() {
                    "push" | "pop" | "capacity" | "reserve" => "a vector",
                    "len" | "is_empty" | "get" => "a string, array, or vector",
                    _ => "an array or vector",
                };
                write!(
                    f,
                    "collection method '{}' requires {}, found {}",
                    method, required, receiver
                )
            }
            TypeErrorKind::InvalidStringMethod { method, receiver } => write!(
                f,
                "string method '{}' requires a string receiver, found {}",
                method, receiver
            ),
            TypeErrorKind::ConstantIndexOutOfBounds { index, length } => write!(
                f,
                "constant index {} is out of bounds for a collection of length {}",
                index, length
            ),
            TypeErrorKind::NotIterable { receiver } => {
                write!(f, "cannot iterate over a value of type {}", receiver)
            }
            TypeErrorKind::UnknownField { structure, field } => {
                write!(f, "unknown field '{}' on struct {}", field, structure)
            }
            TypeErrorKind::MissingField { structure, field } => {
                write!(f, "missing field '{}' in struct {}", field, structure)
            }
            TypeErrorKind::ModuleMemberNotPublic { module, member } => write!(
                f,
                "module member '{}::{}' is not public; add 'pub' to its declaration",
                module, member
            ),
            TypeErrorKind::ModulePathSeparator { module, member } => write!(
                f,
                "module members are reached with '::'; write '{}::{}'",
                module, member
            ),
            TypeErrorKind::SizedArrayElementNotDefaultable { element } => write!(
                f,
                "cannot create a sized array of {}; initialize its elements explicitly",
                element
            ),
            TypeErrorKind::NegativeArraySize { size, constant } => match constant {
                Some(path) => write!(
                    f,
                    "array size cannot be negative: {size}, the value of constant '{path}'; give the constant a value between 0 and the maximum array length"
                ),
                None => write!(f, "array size cannot be negative: {}", size),
            },
            TypeErrorKind::NonConstantArrayRepeat => write!(
                f,
                "array repeat count must be a non-negative compile-time constant; use Vec for a dynamic count"
            ),
            TypeErrorKind::ConstantSliceOutOfBounds {
                start,
                end,
                length,
                inclusive,
            } => write!(
                f,
                "constant slice {}{} is out of bounds for a collection of length {}",
                start.map_or_else(|| "..".to_string(), |value| value.to_string()),
                if *inclusive {
                    end.map_or_else(|| "..=".to_string(), |value| format!("..={value}"))
                } else {
                    end.map_or_else(|| "..".to_string(), |value| format!("..{value}"))
                },
                length
            ),
            TypeErrorKind::MutableCollectionRequired { method, receiver } => {
                write!(
                    f,
                    "collection operation '{}' requires a mutable collection receiver, found {}",
                    method, receiver
                )
            }
            TypeErrorKind::ReadOnlyCollectionRequired { method, receiver } => {
                write!(
                    f,
                    "collection method '{}' is unavailable through a read-only receiver of type {}",
                    method, receiver
                )
            }
            TypeErrorKind::UnconsumedCollectionIterator => write!(
                f,
                "iterator value must be consumed with map, filter, fold, or collect"
            ),
            TypeErrorKind::CollectionCollectRequiresPipeline => write!(
                f,
                "collect is a pipeline terminal; call iter, map, or filter first"
            ),
            TypeErrorKind::MutableCollectionAlias => write!(
                f,
                "mutable aliases of collections are not supported; mutate the original binding"
            ),
            TypeErrorKind::GenericStructDeferred { .. } => {
                write!(f, "generic struct support is reserved for Stage 2")
            }
            TypeErrorKind::DuplicateNominal {
                name,
                keyword,
                collides_with,
            } => match collides_with {
                Some(other) => write!(
                    f,
                    "duplicate {keyword} declaration '{name}': the name is already taken by the {other} '{name}', and one name cannot be both; rename the {keyword} or the {other}"
                ),
                None => write!(f, "duplicate {keyword} declaration '{name}'"),
            },
            TypeErrorKind::DuplicateStructField { structure, field } => {
                write!(f, "duplicate field '{}' in struct {}", field, structure)
            }
            TypeErrorKind::UnknownStruct { name } => write!(f, "unknown struct '{}'", name),
            TypeErrorKind::InvalidStructMethod { method, structure } => {
                write!(
                    f,
                    "method '{}' is not available on struct {}",
                    method, structure
                )
            }
            TypeErrorKind::ImmutableStructField { field } => {
                write!(f, "cannot assign to immutable struct field '{}'", field)
            }
            TypeErrorKind::PrivateFieldAccess {
                structure,
                field,
                owner,
                current,
                operation,
            } => write!(
                f,
                "private field '{}.{}' cannot be {}: owner module '{}', current module '{}'; {}\n   = help: declare the field `pub` or access it from the owner module or a descendant",
                structure, field, operation, owner, current, self.reason
            ),
            TypeErrorKind::PrivateFieldConstruction {
                structure,
                field,
                owner,
                current,
                operation,
            } => write!(
                f,
                "private field '{}.{}' cannot be used in {}: owner module '{}', current module '{}'; {}\n   = help: declare the field `pub`, use a public constructor, or omit it with a rest pattern",
                structure, field, operation, owner, current, self.reason
            ),
            TypeErrorKind::ImmutableStructMethod { method } => {
                write!(
                    f,
                    "cannot call mutable struct method '{}' on an immutable receiver",
                    method
                )
            }
            TypeErrorKind::SharedLoanMutation { place } => write!(
                f,
                "cannot mutate '{}': shared loan is read-only\n   = help: use '&mut {}' and a mutable root when mutation is required",
                place, place
            ),
            TypeErrorKind::MutableLoanOverlap { place } => write!(
                f,
                "borrow of '{}' would overlap an existing mutable loan\n   = help: end the first borrow before taking another mutable loan",
                place
            ),
            TypeErrorKind::MutableLoanAccess { place, access } => write!(
                f,
                "cannot {} '{}': a mutable loan is live\n   = help: finish the mutable borrow before reading, moving, writing, or reallocating the place",
                access, place
            ),
            TypeErrorKind::BorrowEscapes { place, context } => write!(
                f,
                "borrow of '{}' escapes its call-scoped region ({})\n   = help: keep the borrow in a call argument or receiver position",
                place, context
            ),
            TypeErrorKind::BorrowInvalidated { place, context } => write!(
                f,
                "borrow of '{}' was invalidated by {}\n   = help: do not mutate, move, or reallocate a place while its forwarded loan is live",
                place, context
            ),
            TypeErrorKind::TemporaryBorrow { place } => write!(
                f,
                "cannot borrow temporary '{}': temporary borrow has no live owner\n   = help: bind the value to a local before borrowing it",
                place
            ),
            TypeErrorKind::UnknownTrait { name } => write!(f, "unknown trait '{}'", name),
            TypeErrorKind::MissingTraitMethod { trait_name, method } => write!(
                f,
                "trait '{}' is not implemented for this type: missing method '{}'",
                trait_name, method
            ),
            TypeErrorKind::MissingAssociatedItem {
                trait_name,
                item,
                namespace,
                declared_type,
            } => {
                let keyword = namespace.keyword();
                let spelling = match namespace {
                    ItemNamespace::Type => format!("type {item} = <type>"),
                    ItemNamespace::Const => match declared_type {
                        Some(declared) => format!("const {item}: {declared} = <value>"),
                        None => format!("const {item}: <type> = <value>"),
                    },
                };
                write!(
                    f,
                    "implementation of trait '{trait_name}' is missing required associated item '{item}'; define '{keyword} {item}' in the impl body, written '{spelling}'"
                )
            }
            TypeErrorKind::DuplicateAssociatedItem { trait_name, item } => write!(
                f,
                "implementation of trait '{trait_name}' defines associated item '{item}' more than once; each required item must be defined exactly once"
            ),
            TypeErrorKind::AssociatedItemTypeMismatch {
                trait_name,
                item,
                disagreement,
            } => match disagreement {
                AssociatedItemDisagreement::DeclaredType { declared, found } => write!(
                    f,
                    "associated item '{item}' in impl of trait '{trait_name}' declares type '{found}', and the trait declares '{declared}' here; declare the same type as the trait"
                ),
                AssociatedItemDisagreement::ConstantValue { declared, found } => match found {
                    Some(found) => write!(
                        f,
                        "associated item '{item}' in impl of trait '{trait_name}' declares type '{declared}', which agrees with the trait, and is initialised with a value of type '{found}'; write an initialiser of type '{declared}'"
                    ),
                    None => write!(
                        f,
                        "associated item '{item}' in impl of trait '{trait_name}' declares type '{declared}', which agrees with the trait, and is initialised with an expression whose type no rule establishes; write an initialiser of type '{declared}' the compiler can fold"
                    ),
                },
            },
            TypeErrorKind::AmbiguousAssociatedProjection {
                receiver,
                item,
                cause,
            } => match cause {
                ProjectionFailure::Unbound => write!(
                    f,
                    "projection '{receiver}::{item}' cannot be resolved: no bound in scope declares '{item}'; add a bound on '{receiver}' whose trait declares '{item}'"
                ),
                ProjectionFailure::NoImpl => write!(
                    f,
                    "projection '{receiver}::{item}' cannot be resolved: no impl for '{receiver}' defines '{item}'; define it in an impl of a trait that declares '{item}'"
                ),
                ProjectionFailure::SelfOutsideImpl => write!(
                    f,
                    "projection '{receiver}::{item}' cannot be resolved: '{receiver}' names a type only inside an impl or a trait declaration, and neither is open here; write the type itself in place of '{receiver}'"
                ),
                ProjectionFailure::NominalParameter => write!(
                    f,
                    "projection '{receiver}::{item}' cannot be resolved: '{receiver}' is a type parameter of a struct or an enum, and those carry no bound, so nothing declares '{item}'; name a concrete type here, or give the struct or enum a parameter for '{item}' itself"
                ),
                ProjectionFailure::WrongNamespace { found } => write!(
                    f,
                    "projection '{receiver}::{item}' cannot be resolved: '{item}' is an {} of '{receiver}', not an {}; name an {} here, or use '{receiver}::{item}' where a {} is expected",
                    found.noun(),
                    found.other().noun(),
                    found.other().noun(),
                    found.position()
                ),
                ProjectionFailure::BuiltinReceiver => write!(
                    f,
                    "projection '{receiver}::{item}' cannot be resolved: '{receiver}' is a built-in type, and an associated item is defined only in an impl, which a built-in type cannot have; name a struct, an enum, a trait, or a bounded type parameter here"
                ),
                ProjectionFailure::ReceiverArguments { param } => write!(
                    f,
                    "projection '{receiver}::{item}' cannot be resolved: the impl that defines '{item}' for '{receiver}' defines it as its own type parameter '{param}', and a receiver written without type arguments says nothing about '{param}'; define '{item}' as a type that does not name '{param}', or add an impl for the instantiation you mean"
                ),
                ProjectionFailure::NotComputable => write!(
                    f,
                    "constant '{receiver}::{item}' is defined but its value cannot be computed: the arithmetic overflows or divides by zero; give it a value that evaluates"
                ),
                ProjectionFailure::NotConstant { declared } => match declared {
                    Some(declared) => write!(
                        f,
                        "constant '{receiver}::{item}' is declared with type '{declared}' and its value is not a constant integer expression; only an integer constant is folded, so no '{declared}' constant can be read back"
                    ),
                    None => write!(
                        f,
                        "constant '{receiver}::{item}' is defined but its value is not a constant integer expression; use integer literals and '+ - * / %' only"
                    ),
                },
                ProjectionFailure::LengthFromTypeParameter => write!(
                    f,
                    "array length '{receiver}::{item}' depends on the type parameter '{receiver}', and a fixed-array length must be known where the array is written; write the length as a literal, name the constant on a concrete type, or use a growable array"
                ),
                ProjectionFailure::UnspecializedGenericMethod => write!(
                    f,
                    "constant '{receiver}::{item}' cannot be read here: '{receiver}' is a type parameter of the method itself, and such a method is compiled a single time whatever the types it is called with, so '{item}' has no single value; name the constant on a concrete type, or move the body into a free generic function, which is specialized"
                ),
                ProjectionFailure::Ambiguous { traits } => write!(
                    f,
                    "projection '{receiver}::{item}' is ambiguous: {} both define '{item}' for {receiver}; name the trait that declares the one you mean, as in '{}::{item}', or remove one of the competing impls, or rename the item so a single trait provides it",
                    traits.join(" and "),
                    traits.first().map(String::as_str).unwrap_or(receiver)
                ),
                ProjectionFailure::AmbiguousImplementors { types } => write!(
                    f,
                    "projection '{receiver}::{item}' is ambiguous: {} both implement '{receiver}' and define '{item}'; name the type that owns the one you mean, as in '{}::{item}', or remove one of the competing impls",
                    types.join(" and "),
                    types.first().map(String::as_str).unwrap_or(receiver)
                ),
                ProjectionFailure::AmbiguousInstantiations {
                    trait_name,
                    constructor,
                    instantiations,
                } => write!(
                    f,
                    "projection '{receiver}::{item}' is ambiguous: '{trait_name}' is implemented for {}, and each defines '{item}'; a projection names '{constructor}' without its type arguments and this language has no way to write them there, so neither naming the trait nor naming the type separates the definitions; keep a single impl of '{trait_name}' for '{constructor}', or declare '{item}' in a second trait and name that trait, as in 'OtherTrait::{item}'",
                    instantiations.join(" and ")
                ),
                ProjectionFailure::Cyclic { path, namespace } => write!(
                    f,
                    "projection '{receiver}::{item}' forms a cycle: its definition resolves back to itself through {}; give the {} a concrete definition to break the cycle",
                    path.join(" -> "),
                    namespace.noun()
                ),
            },
            TypeErrorKind::AssociatedProjectionLimit {
                receiver,
                item,
                limit,
            } => write!(
                f,
                "resolving projection '{receiver}::{item}' expands to more than {limit} type nodes; an associated type that names another one more than once doubles the expansion at each step, so give one of them a concrete definition"
            ),
            TypeErrorKind::AssociatedItemOutsideTraitImpl {
                target,
                item,
                keyword,
                trait_name,
            } => match trait_name {
                Some(trait_name) => write!(
                    f,
                    "associated item '{keyword} {item}' is defined in the impl of trait '{trait_name}' for '{target}', which does not declare it; move it into an impl of a trait that declares '{item}'"
                ),
                None => write!(
                    f,
                    "associated item '{keyword} {item}' is defined in the inherent impl of '{target}', which declares no associated item; move it into an impl of a trait that declares '{item}'"
                ),
            },
            TypeErrorKind::UninhabitedNominalCycle { keyword, path } => {
                let (members, repair) = if *keyword == "enum" {
                    (
                        "variants",
                        "a variant carrying none of the types on the cycle",
                    )
                } else {
                    ("fields", "an enum with a terminating variant")
                };
                write!(
                    f,
                    "{keyword} '{}' can never be constructed: its {members} form the cycle {}, so building one would first require building another; break the cycle with an Option, a Vec, or {repair}",
                    path.first().map(String::as_str).unwrap_or_default(),
                    path.join(" -> ")
                )
            }
            TypeErrorKind::AssociatedBindingMismatch {
                trait_name,
                item,
                requested,
                found,
            } => write!(
                f,
                "associated binding '{}::{} = {}' disagrees with the selected impl, which provides {}; change the requested binding or the impl",
                trait_name,
                item,
                requested.source_spelling(),
                found.source_spelling()
            ),
            TypeErrorKind::AssociatedConstBindingMismatch {
                trait_name,
                item,
                requested,
                found,
            } => write!(
                f,
                "associated binding '{}::{} = {}' disagrees with the selected impl, which provides {}; change the requested binding or the impl",
                trait_name, item, requested, found
            ),
            TypeErrorKind::UndeclaredAssociatedBinding { trait_name, item } => write!(
                f,
                "associated binding '{}::{}' constrains nothing: trait '{}' declares no associated type or constant '{}'; name an item the trait declares, or drop the binding",
                trait_name, item, trait_name, item
            ),
            TypeErrorKind::AssociatedBindingNamespaceMismatch {
                trait_name,
                item,
                value,
                declared,
            } => write!(
                f,
                "associated binding '{}::{} = {}' binds a {} to an {}; bind a {}, or name an {}",
                trait_name,
                item,
                value,
                declared.other().position(),
                declared.noun(),
                declared.position(),
                declared.other().noun()
            ),
            TypeErrorKind::UnevaluatedAssociatedConstBinding {
                trait_name,
                item,
                unevaluated,
            } => write!(
                f,
                "associated binding '{}::{}' cannot be checked because {} could not be evaluated; give the constant a value the compiler can fold",
                trait_name,
                item,
                unevaluated
                    .iter()
                    .map(InferType::source_spelling)
                    .collect::<Vec<_>>()
                    .join(" and ")
            ),
            TypeErrorKind::UnfoldableAssociatedConstType {
                trait_name,
                item,
                declared,
            } => write!(
                f,
                "associated constant '{item}' of trait '{trait_name}' is declared with type '{declared}', and the compiler folds no constant of that type, so an impl could define it and no reader could ever get a value back; declare it with a type the compiler folds a constant of: {}",
                InferType::foldable_associated_const_types()
            ),
            TypeErrorKind::UnconstrainedImplTypeParam {
                param,
                target,
                call_site_binds,
            } => {
                if *call_site_binds {
                    write!(
                        f,
                        "impl type parameter '{param}' does not appear in the impl target type '{target}', though a call site determines it; an impl instance is named by its trait, its target type and its method, and by nothing that records '{param}', so two choices of it would share one symbol; mention '{param}' in the target type or move it onto the method"
                    )
                } else {
                    write!(
                        f,
                        "impl type parameter '{param}' does not appear in the impl target type '{target}', so no call site can determine it; mention '{param}' in the target type or move it onto the method"
                    )
                }
            }
            TypeErrorKind::DuplicateTraitImpl { trait_name, target } => write!(
                f,
                "duplicate implementation of trait '{}' for type '{}'",
                trait_name, target
            ),
            TypeErrorKind::TraitMethodNotInTrait { trait_name, method } => write!(
                f,
                "method '{}' is not declared by trait '{}'",
                method, trait_name
            ),
            TypeErrorKind::TraitMethodSignatureMismatch { trait_name, method } => write!(
                f,
                "method '{}' does not match the signature declared by trait '{}'",
                method, trait_name
            ),
            TypeErrorKind::AmbiguousTraitMethod { target, method } => write!(
                f,
                "method '{}' on '{}' is provided by more than one trait; use a qualified call",
                method, target
            ),
            TypeErrorKind::UnboundTypeParamMethod { param, method } => write!(
                f,
                "method '{}' is not available on type parameter '{}' because no bound on '{}' provides it; add the bound '{}: Trait' that declares '{}'",
                method, param, param, param, method
            ),
            TypeErrorKind::UnresolvedInstanceSymbol { name } => write!(
                f,
                "'{}' was not specialized for this call because a type parameter stayed unknown; add a type annotation that pins every type parameter of the call",
                name
            ),
            TypeErrorKind::UnsatisfiedTraitBound { trait_name, ty } => write!(
                f,
                "trait '{}' is not implemented for {}; add an impl or change the bound",
                trait_name, ty
            ),
            TypeErrorKind::MissingSupertraitImpl {
                trait_name,
                supertrait,
                ty,
            } => write!(
                f,
                "trait '{supertrait}' is not implemented for {ty}, and the impl of '{trait_name}' requires it; implement '{supertrait}' for {ty}, or drop '{supertrait}' from the supertraits of '{trait_name}'"
            ),
            TypeErrorKind::OrphanTraitImpl { trait_name, target } => write!(
                f,
                "cannot implement trait '{}' for {}; the trait or type must be local",
                trait_name, target
            ),
            TypeErrorKind::OverlappingTraitImpl { trait_name, target } => write!(
                f,
                "trait '{}' has overlapping implementations for {}; add a disjoint bound",
                trait_name, target
            ),
            TypeErrorKind::DuplicateTraitMethod { trait_name, method } => write!(
                f,
                "trait '{}' declares method '{}' more than once",
                trait_name, method
            ),
            TypeErrorKind::InvalidTraitReceiver { trait_name, method } => write!(
                f,
                "trait '{}' method '{}' must use a by-value self receiver",
                trait_name, method
            ),
            TypeErrorKind::UnresolvedGenericType { name } => write!(
                f,
                "cannot infer the concrete type for generic parameter '{}'; add a type argument",
                name
            ),
            TypeErrorKind::RecursiveMonomorphization { name } => write!(
                f,
                "generic instantiation of '{}' is recursive without a decreasing type argument",
                name
            ),
            TypeErrorKind::MonomorphizationLimit { name } => write!(
                f,
                "generic instantiation limit exceeded while compiling '{}'",
                name
            ),
            TypeErrorKind::MangledSymbolCollision { name } => {
                write!(f, "two instances of '{}' mangle to one symbol", name)
            }
            TypeErrorKind::EnumLayoutTooLarge {
                enum_name,
                item,
                count,
                limit,
            } => write!(
                f,
                "EnumLayoutTooLarge: enum '{}' {} has {} entries, but the bytecode limit is {}; reduce the payload or split the enum",
                enum_name, item, count, limit
            ),
            TypeErrorKind::NonExhaustiveStruct { structure } => write!(
                f,
                "non-exhaustive struct match for {}; add '_' or an irrefutable field pattern",
                structure
            ),
            TypeErrorKind::UnreachablePattern { pattern } => write!(
                f,
                "unreachable pattern: {} is already covered by an earlier arm\n   = help: remove the arm or move it above the arm that covers it",
                pattern
            ),
            TypeErrorKind::UnresolvedTypeVariable => write!(
                f,
                "a type here stayed unresolved after inference; add a type annotation so every surface value has a concrete type"
            ),
            TypeErrorKind::PoisonedType => write!(
                f,
                "this type was poisoned by an earlier type error; fix the errors above and compile again"
            ),
            TypeErrorKind::UndeterminedType => write!(
                f,
                "the type of this value could not be determined; add a type annotation so it has a concrete type"
            ),
            TypeErrorKind::GlobalWithoutSignature { name } => write!(
                f,
                "'{}' is announced as a global but the compiler holds no type signature for it, so it cannot be used here",
                name
            ),
            TypeErrorKind::NamespaceIsNotAValue { name } => write!(
                f,
                "'{}' names a namespace, not a value; reach its members with '{}::member'",
                name, name
            ),
            TypeErrorKind::NotANamespace { name } => write!(
                f,
                "'{}' is a value, not a namespace; reach its members with '{}.member'",
                name, name
            ),
            TypeErrorKind::NoSuchMember { receiver, member } => write!(
                f,
                "no field or method '{}' on a value of type {}",
                member, receiver
            ),
            TypeErrorKind::UnknownTypeName { name } => {
                write!(f, "unknown type '{}'; no such type is in scope", name)
            }
            TypeErrorKind::TypeNotImported { name, module } => write!(
                f,
                "'{}' is exported by module '{}' but this file does not import it\n   \
                 = help: write `needs {} from {}`",
                name, module, name, module
            ),
            TypeErrorKind::PrivateNominalNotExported { name, module } => write!(
                f,
                "'{name}' is not public in module '{module}'; it is registered here only for a \
                 body this file inlines\n   \
                 = help: declare it 'pub' in {module}.aelys, or declare a type of this file's own"
            ),
        }
    }
}

impl std::error::Error for TypeError {}

impl TypeError {
    pub fn mismatch(
        expected: InferType,
        found: InferType,
        span: Span,
        reason: ConstraintReason,
    ) -> Self {
        TypeError {
            kind: TypeErrorKind::Mismatch { expected, found },
            span,
            reason,
        }
    }

    pub fn infinite_type(
        var: TypeVarId,
        ty: InferType,
        span: Span,
        reason: ConstraintReason,
    ) -> Self {
        TypeError {
            kind: TypeErrorKind::InfiniteType { var, ty },
            span,
            reason,
        }
    }

    pub fn not_one_of(
        ty: InferType,
        options: Vec<InferType>,
        span: Span,
        reason: ConstraintReason,
    ) -> Self {
        TypeError {
            kind: TypeErrorKind::NotOneOf { ty, options },
            span,
            reason,
        }
    }

    pub fn arity_mismatch(
        expected: usize,
        found: usize,
        span: Span,
        reason: ConstraintReason,
    ) -> Self {
        TypeError {
            kind: TypeErrorKind::ArityMismatch { expected, found },
            span,
            reason,
        }
    }

    pub fn not_callable(ty: InferType, span: Span, reason: ConstraintReason) -> Self {
        TypeError {
            kind: TypeErrorKind::NotCallable { ty },
            span,
            reason,
        }
    }

    pub fn undefined_variable(name: String, span: Span) -> Self {
        TypeError {
            kind: TypeErrorKind::UndefinedVariable { name },
            span,
            reason: ConstraintReason::Other("variable lookup".to_string()),
        }
    }

    pub fn undefined_function(name: String, span: Span) -> Self {
        TypeError {
            kind: TypeErrorKind::UndefinedFunction { name },
            span,
            reason: ConstraintReason::Other("function call".to_string()),
        }
    }

    pub fn recursion_limit(span: Span) -> Self {
        TypeError {
            kind: TypeErrorKind::RecursionLimit,
            span,
            reason: ConstraintReason::Other("recursion limit".to_string()),
        }
    }
}

impl TypeErrorKind {
    fn states_its_reason(&self) -> bool {
        matches!(self.diagnostic_code(), 421..=430 | 434)
    }

    pub fn diagnostic_code(&self) -> u16 {
        match self {
            Self::NonExhaustiveMatch { .. } => 302,
            Self::IgnoredResult => 303,
            Self::IgnoredOption => 304,
            Self::NullIsNotInSurface => 106,
            Self::DynamicIsNotInSurface => 347,
            Self::UntypedNativeValue { .. } => 348,
            Self::UntypedNativeBoundary { .. } => 379,
            Self::UnmaterializedAppliedType { .. } => 349,
            Self::QuestionMarkOutsideResult => 305,
            Self::QuestionMarkTypeMismatch { .. } => 306,
            Self::UnresolvedSumType { .. } => 307,
            Self::UntypedSumValue { .. } => 308,
            Self::InvalidSumMethod { .. } => 309,
            Self::DynamicSumMethod { .. } => 310,
            Self::InvalidCollectionMethod { .. } => 311,
            Self::InvalidStringMethod { .. } => 312,
            Self::ModuleMemberNotPublic { .. } => 313,
            Self::ModulePathSeparator { .. } => 411,
            Self::PrivateFieldAccess { .. } => 412,
            Self::PrivateFieldConstruction { .. } => 413,
            Self::SizedArrayElementNotDefaultable { .. } => 314,
            Self::NegativeArraySize { .. } => 315,
            Self::NonConstantArrayRepeat => 316,
            Self::ConstantSliceOutOfBounds { .. } => 317,
            Self::MutableCollectionRequired { .. } => 318,
            Self::ReadOnlyCollectionRequired { .. } => 319,
            Self::UnconsumedCollectionIterator => 320,
            Self::ConstantIndexOutOfBounds { .. } => 321,
            Self::CollectionCollectRequiresPipeline => 322,
            Self::MutableCollectionAlias => 323,
            Self::NonExhaustiveStruct { .. } => 324,
            Self::GenericStructDeferred { .. } => 325,
            Self::DuplicateNominal { .. } => 326,
            Self::DuplicateStructField { .. } => 327,
            Self::UnknownStruct { .. } => 328,
            Self::InvalidStructMethod { .. } => 329,
            Self::ImmutableStructField { .. } => 330,
            Self::ImmutableStructMethod { .. } => 331,
            Self::SharedLoanMutation { .. } => 414,
            Self::MutableLoanOverlap { .. } => 415,
            Self::MutableLoanAccess { .. } => 416,
            Self::BorrowEscapes { .. } => 417,
            Self::BorrowInvalidated { .. } => 418,
            Self::TemporaryBorrow { .. } => 419,
            Self::UnknownTrait { .. } => 332,
            Self::MissingTraitMethod { .. } => 333,
            Self::MissingAssociatedItem { .. } => 421,
            Self::AssociatedItemTypeMismatch { .. } => 422,
            Self::AmbiguousAssociatedProjection { .. } => 423,
            Self::AssociatedItemOutsideTraitImpl { .. } => 425,
            Self::AssociatedProjectionLimit { .. } => 427,
            Self::UninhabitedNominalCycle { .. } => 428,
            Self::AssociatedBindingMismatch { .. }
            | Self::AssociatedConstBindingMismatch { .. }
            | Self::UndeclaredAssociatedBinding { .. }
            | Self::AssociatedBindingNamespaceMismatch { .. } => 424,
            Self::UnevaluatedAssociatedConstBinding { .. } => 429,
            Self::UnconstrainedImplTypeParam { .. } => 430,
            Self::UnfoldableAssociatedConstType { .. } => 434,
            Self::DuplicateAssociatedItem { .. } => 426,
            Self::DuplicateTraitImpl { .. } => 334,
            Self::TraitMethodNotInTrait { .. } => 335,
            Self::TraitMethodSignatureMismatch { .. } => 336,
            Self::AmbiguousTraitMethod { .. } => 337,
            Self::UnboundTypeParamMethod { .. } => 351,
            Self::UnresolvedInstanceSymbol { .. } => 352,
            Self::UnresolvedTypeVariable => 353,
            Self::PoisonedType => 354,
            Self::UndeterminedType => 377,
            Self::GlobalWithoutSignature { .. } => 376,
            Self::UnsatisfiedTraitBound { .. } | Self::MissingSupertraitImpl { .. } => 338,
            Self::OrphanTraitImpl { .. } => 339,
            Self::OverlappingTraitImpl { .. } => 340,
            Self::DuplicateTraitMethod { .. } => 341,
            Self::InvalidTraitReceiver { .. } => 342,
            Self::UnresolvedGenericType { .. } => 343,
            Self::RecursiveMonomorphization { .. } => 344,
            Self::MonomorphizationLimit { .. } => 345,
            Self::MangledSymbolCollision { .. } => 355,
            Self::EnumLayoutTooLarge { .. } => 346,
            Self::UnreachablePattern { .. } => 356,
            Self::GenericArityMismatch { .. } => 357,
            Self::PatternBindingMismatch { .. } => 358,
            Self::ArityMismatch { .. } => 359,
            Self::NotCallable { .. } => 360,
            Self::InfiniteType { .. } => 361,
            Self::UndefinedFunction { .. } => 362,
            Self::UnknownField { .. } => 363,
            Self::MissingField { .. } => 364,
            Self::NotIterable { .. } => 365,
            Self::InvalidIndex { .. } => 366,
            Self::UntypedNativeTypeMismatch { .. } => 367,
            Self::RecursionLimit => 368,
            Self::NamespaceIsNotAValue { .. } => 369,
            Self::NotANamespace { .. } => 370,
            Self::NoSuchMember { .. } => 371,
            Self::UnknownTypeName { .. } => 372,
            Self::TypeNotImported { .. } => 378,
            // the same refusal e0403 states at the module boundary, reached from the type checker
            Self::PrivateNominalNotExported { .. } => 403,
            Self::InvalidTryResidual { .. } => 373,
            Self::UnsatisfiedTryConversion { .. } => 374,
            Self::ReservedIdentityConversion { .. } => 375,
            Self::UnknownVariant { .. } => 109,
            Self::MatchArmValueRequired => 110,
            Self::MissingReturnValue { .. } => 215,
            // an undefined name keeps 301 because the surface pins that code for it
            Self::Mismatch { .. } | Self::NotOneOf { .. } | Self::UndefinedVariable { .. } => 301,
        }
    }
}
