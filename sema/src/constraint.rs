
mod definition;
mod error;
mod reason;

pub use definition::Constraint;
pub use error::{ProjectionFailure, TypeError, TypeErrorKind};
pub use reason::ConstraintReason;
