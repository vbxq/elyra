mod definition;
mod error;
mod reason;

pub use definition::Constraint;
pub use error::{
    AliasSite, AssociatedItemDisagreement, ItemNamespace, ProjectionFailure, TypeError,
    TypeErrorKind,
};
pub use reason::ConstraintReason;
