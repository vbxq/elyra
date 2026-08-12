use super::{
    AelysArray, AelysClosure, AelysFunction, AelysRange, AelysString, AelysSum, AelysUpvalue,
    AelysVec, NativeFunction,
};

/// The different types of GC-managed objects.
#[derive(Debug)]
pub enum ObjectKind {
    String(AelysString),
    Function(AelysFunction),
    Native(NativeFunction),
    Upvalue(AelysUpvalue),
    Closure(AelysClosure),
    Array(AelysArray),
    Vec(AelysVec),
    Range(AelysRange),
    Sum(AelysSum),
}
