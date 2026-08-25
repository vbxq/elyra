use super::{
    AelysArray, AelysClosure, AelysEnum, AelysFunction, AelysRange, AelysString, AelysStruct,
    AelysSum, AelysUpvalue, AelysVec, NativeFunction,
};

#[derive(Debug)]
pub enum ObjectKind {
    String(AelysString),
    Function(Box<AelysFunction>),
    Native(NativeFunction),
    Upvalue(AelysUpvalue),
    Closure(AelysClosure),
    Array(AelysArray),
    Vec(AelysVec),
    Range(AelysRange),
    Sum(AelysSum),
    Enum(AelysEnum),
    Struct(Box<AelysStruct>),
}
