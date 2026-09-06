use crate::types::InferType;
use std::fmt;

#[derive(Debug, Clone)]
pub enum ConstraintReason {
    BinaryOp {
        op: String,
    },
    BitwiseOp {
        op: String,
    },
    Argument {
        func_name: String,
        arg_index: usize,
    },
    Return {
        func_name: String,
    },
    Assignment {
        var_name: String,
    },
    TypeAnnotation {
        var_name: String,
    },
    IfCondition,
    IfBranches,
    WhileCondition,
    /// for loop bounds must be int
    ForBounds,
    Comparison,
    ArrayElement,
    ArrayIndex,
    /// range bounds must be int
    RangeBound,
    CollectionMethodReceiver {
        method: String,
    },
    InvalidCast,
    UnknownType {
        name: String,
    },
    IntLiteralOverflow {
        value: i64,
        target: InferType,
    },
    Other(String),
    DefinedIn {
        module: String,
        inner: Box<ConstraintReason>,
    },
}

impl ConstraintReason {
    pub fn defining_module(&self) -> Option<&str> {
        match self {
            ConstraintReason::DefinedIn { module, .. } => Some(module),
            _ => None,
        }
    }

    pub fn wrap_in_module(&mut self, module: &str) {
        if matches!(self, ConstraintReason::DefinedIn { .. }) {
            return;
        }
        let inner = std::mem::replace(self, ConstraintReason::IfCondition);
        *self = ConstraintReason::DefinedIn {
            module: module.to_string(),
            inner: Box::new(inner),
        };
    }
}

impl fmt::Display for ConstraintReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConstraintReason::BinaryOp { op } => write!(f, "binary operator '{}'", op),
            ConstraintReason::BitwiseOp { op } => {
                write!(f, "bitwise operator '{}' (requires integers)", op)
            }
            ConstraintReason::Argument {
                func_name,
                arg_index,
            } => {
                write!(f, "argument {} to function '{}'", arg_index + 1, func_name)
            }
            ConstraintReason::Return { func_name } => {
                write!(f, "return type of function '{}'", func_name)
            }
            ConstraintReason::Assignment { var_name } => {
                write!(f, "assignment to variable '{}'", var_name)
            }
            ConstraintReason::TypeAnnotation { var_name } => {
                write!(f, "type annotation on variable '{}'", var_name)
            }
            ConstraintReason::IfCondition => write!(f, "if condition"),
            ConstraintReason::IfBranches => write!(f, "if/else branches"),
            ConstraintReason::WhileCondition => write!(f, "while condition"),
            ConstraintReason::ForBounds => write!(f, "for loop bounds"),
            ConstraintReason::Comparison => write!(f, "comparison"),
            ConstraintReason::ArrayElement => write!(f, "array element"),
            ConstraintReason::ArrayIndex => write!(f, "array index"),
            ConstraintReason::RangeBound => write!(f, "range bound"),
            ConstraintReason::CollectionMethodReceiver { method } => {
                write!(f, "collection method '{}' receiver", method)
            }
            ConstraintReason::InvalidCast => write!(f, "invalid cast"),
            ConstraintReason::UnknownType { name } => write!(f, "unknown type '{}'", name),
            ConstraintReason::IntLiteralOverflow { value, target } => {
                write!(f, "integer literal {} does not fit in {:?}", value, target)
            }
            ConstraintReason::Other(s) => write!(f, "{}", s),
            ConstraintReason::DefinedIn { inner, .. } => inner.fmt(f),
        }
    }
}
