// heap object types

mod array;
mod closure;
mod function;
mod gc_object;
mod gc_ref;
mod kinds;
mod native;
mod range;
mod string;
mod sum;
mod upvalue;
mod vec;

pub use array::{AelysArray, ArrayData, TypeTag};
pub use closure::{AelysClosure, ClosureCache};
pub use function::AelysFunction;
pub use gc_object::GcObject;
pub use gc_ref::GcRef;
pub use kinds::ObjectKind;
pub use native::NativeFunction;
pub use range::AelysRange;
pub use string::AelysString;
pub use sum::{AelysSum, SumTag};
pub use upvalue::{AelysUpvalue, UpvalueLocation};
pub use vec::AelysVec;
