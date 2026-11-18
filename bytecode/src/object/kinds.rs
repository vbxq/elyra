use super::{
    AelysArray, AelysClosure, AelysEnum, AelysFunction, AelysMap, AelysRange, AelysString,
    AelysStruct, AelysSum, AelysUpvalue, AelysVec, BoxedInt, NativeFunction,
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
    Map(Box<AelysMap>),
    Range(AelysRange),
    Sum(AelysSum),
    Enum(AelysEnum),
    Struct(Box<AelysStruct>),
    Int(BoxedInt),
}
