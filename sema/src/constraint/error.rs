use super::ConstraintReason;
use crate::types::{InferType, TypeVarId};
use aelys_syntax::Span;
use std::fmt;

/// Type error during inference
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
}

#[derive(Debug, Clone)]
pub enum TypeErrorKind {
    /// Two types could not be unified
    Mismatch {
        expected: InferType,
        found: InferType,
    },
    /// Infinite type (occurs check failed)
    InfiniteType {
        var: TypeVarId,
        ty: InferType,
    },
    /// Type is not one of the expected options
    NotOneOf {
        ty: InferType,
        options: Vec<InferType>,
    },
    /// Arity mismatch in function call
    ArityMismatch {
        expected: usize,
        found: usize,
    },
    /// Tried to call a non-function
    NotCallable {
        ty: InferType,
    },
    /// Undefined variable
    UndefinedVariable {
        name: String,
    },
    /// Undefined function
    UndefinedFunction {
        name: String,
    },
    /// Recursion depth limit exceeded in type inference
    RecursionLimit,
    NonExhaustiveMatch {
        missing: Vec<String>,
    },
    IgnoredResult,
    IgnoredOption,
    NullIsNotInSurface,
    QuestionMarkOutsideResult,
    QuestionMarkTypeMismatch {
        source: InferType,
        target: InferType,
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
    SizedArrayElementNotDefaultable {
        element: InferType,
    },
}

impl fmt::Display for TypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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
                        "len" | "get" => "a string, array, or vector",
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
                write!(f, "undefined variable: {}", name)
            }
            TypeErrorKind::UndefinedFunction { name } => {
                write!(f, "undefined function: {}", name)
            }
            TypeErrorKind::RecursionLimit => {
                write!(f, "type inference recursion limit exceeded")
            }
            TypeErrorKind::NonExhaustiveMatch { missing } => {
                write!(f, "non-exhaustive match; missing {}", missing.join(", "))
            }
            TypeErrorKind::IgnoredResult => write!(f, "unused Result value"),
            TypeErrorKind::IgnoredOption => write!(f, "unused Option value"),
            TypeErrorKind::NullIsNotInSurface => write!(
                f,
                "null is not part of Aelys; use Option for absence or Result for failure"
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
                    "len" | "get" => "a string, array, or vector",
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
            TypeErrorKind::SizedArrayElementNotDefaultable { element } => write!(
                f,
                "cannot create a sized array of {}; initialize its elements explicitly",
                element
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
    pub fn diagnostic_code(&self) -> u16 {
        match self {
            Self::NonExhaustiveMatch { .. } => 302,
            Self::IgnoredResult => 303,
            Self::IgnoredOption => 304,
            Self::NullIsNotInSurface => 106,
            Self::QuestionMarkOutsideResult => 305,
            Self::QuestionMarkTypeMismatch { .. } => 306,
            Self::UnresolvedSumType { .. } => 307,
            Self::UntypedSumValue { .. } => 308,
            Self::InvalidSumMethod { .. } => 309,
            Self::DynamicSumMethod { .. } => 310,
            Self::InvalidCollectionMethod { .. } => 311,
            Self::InvalidStringMethod { .. } => 312,
            Self::ModuleMemberNotPublic { .. } => 313,
            Self::SizedArrayElementNotDefaultable { .. } => 314,
            _ => 301,
        }
    }
}
