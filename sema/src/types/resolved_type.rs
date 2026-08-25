use super::{InferType, TypeVarId};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ResolvedType {
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    F32,
    F64,
    Bool,
    String,
    Unit,
    Null,

    Option(Box<ResolvedType>),
    Result(Box<ResolvedType>, Box<ResolvedType>),
    Error,
    Never,
    UntypedNative(String),

    Function {
        params: Vec<ResolvedType>,
        ret: Box<ResolvedType>,
    },

    Array(Box<ResolvedType>),
    FixedArray(Box<ResolvedType>, usize),
    Vec(Box<ResolvedType>),
    Tuple(Vec<ResolvedType>),
    Range,

    Struct(std::string::String),

    Applied {
        name: String,
        args: Vec<ResolvedType>,
    },

    TypeParam(String),
    TypeVar(TypeVarId),

    Dynamic,

    // mirror of infertype::poison: it only exists so a poisoned type never looks certain
    Poison,

    Uncertain(Box<ResolvedType>),
}

impl ResolvedType {
    pub fn is_certain(&self) -> bool {
        !matches!(
            self,
            ResolvedType::Dynamic
                | ResolvedType::Poison
                | ResolvedType::Uncertain(_)
                | ResolvedType::TypeParam(_)
                | ResolvedType::TypeVar(_)
        )
    }

    pub fn is_integer(&self) -> bool {
        matches!(
            self,
            ResolvedType::I8
                | ResolvedType::I16
                | ResolvedType::I32
                | ResolvedType::I64
                | ResolvedType::U8
                | ResolvedType::U16
                | ResolvedType::U32
                | ResolvedType::U64
        )
    }

    pub fn is_float(&self) -> bool {
        matches!(self, ResolvedType::F32 | ResolvedType::F64)
    }

    pub fn is_numeric(&self) -> bool {
        self.is_integer() || self.is_float()
    }

    pub fn is_integer_ish(&self) -> bool {
        match self {
            t if t.is_integer() => true,
            ResolvedType::Uncertain(inner) => inner.is_integer(),
            _ => false,
        }
    }

    pub fn is_float_ish(&self) -> bool {
        match self {
            t if t.is_float() => true,
            ResolvedType::Uncertain(inner) => inner.is_float(),
            _ => false,
        }
    }

    pub fn needs_guard(&self) -> bool {
        matches!(self, ResolvedType::Uncertain(_))
    }

    pub fn unwrap_uncertain(&self) -> &ResolvedType {
        match self {
            ResolvedType::Uncertain(inner) => inner,
            other => other,
        }
    }

    pub fn from_infer_type(ty: &InferType) -> Self {
        match ty {
            InferType::I8 => ResolvedType::I8,
            InferType::I16 => ResolvedType::I16,
            InferType::I32 => ResolvedType::I32,
            InferType::I64 => ResolvedType::I64,
            InferType::U8 => ResolvedType::U8,
            InferType::U16 => ResolvedType::U16,
            InferType::U32 => ResolvedType::U32,
            InferType::U64 => ResolvedType::U64,
            InferType::F32 => ResolvedType::F32,
            InferType::F64 => ResolvedType::F64,
            InferType::Bool => ResolvedType::Bool,
            InferType::String => ResolvedType::String,
            InferType::Unit => ResolvedType::Unit,
            InferType::Null => ResolvedType::Null,
            InferType::Option(inner) => {
                ResolvedType::Option(Box::new(ResolvedType::from_infer_type(inner)))
            }
            InferType::Result(ok, err) => ResolvedType::Result(
                Box::new(ResolvedType::from_infer_type(ok)),
                Box::new(ResolvedType::from_infer_type(err)),
            ),
            InferType::Error => ResolvedType::Error,
            InferType::Never => ResolvedType::Never,
            InferType::Numeric => ResolvedType::I64,
            InferType::UntypedNative(name) => ResolvedType::UntypedNative(name.clone()),
            InferType::Function { params, ret } => ResolvedType::Function {
                params: params.iter().map(ResolvedType::from_infer_type).collect(),
                ret: Box::new(ResolvedType::from_infer_type(ret)),
            },
            InferType::Array(inner) => {
                ResolvedType::Array(Box::new(ResolvedType::from_infer_type(inner)))
            }
            InferType::FixedArray(inner, length) => {
                ResolvedType::FixedArray(Box::new(ResolvedType::from_infer_type(inner)), *length)
            }
            InferType::Vec(inner) => {
                ResolvedType::Vec(Box::new(ResolvedType::from_infer_type(inner)))
            }
            InferType::Tuple(elems) => {
                ResolvedType::Tuple(elems.iter().map(ResolvedType::from_infer_type).collect())
            }
            InferType::Range => ResolvedType::Range,
            InferType::Struct(name) => ResolvedType::Struct(name.clone()),
            InferType::Applied { name, args } => ResolvedType::Applied {
                name: name.clone(),
                args: args.iter().map(Self::from_infer_type).collect(),
            },
            InferType::Param(name) => ResolvedType::TypeParam(name.clone()),
            InferType::Var(id) => ResolvedType::TypeVar(*id),
            InferType::Dynamic => ResolvedType::Dynamic,
            InferType::Poison => ResolvedType::Poison,
        }
    }
}

impl fmt::Display for ResolvedType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResolvedType::I8 => write!(f, "i8"),
            ResolvedType::I16 => write!(f, "i16"),
            ResolvedType::I32 => write!(f, "i32"),
            ResolvedType::I64 => write!(f, "i64"),
            ResolvedType::U8 => write!(f, "u8"),
            ResolvedType::U16 => write!(f, "u16"),
            ResolvedType::U32 => write!(f, "u32"),
            ResolvedType::U64 => write!(f, "u64"),
            ResolvedType::F32 => write!(f, "f32"),
            ResolvedType::F64 => write!(f, "f64"),
            ResolvedType::Bool => write!(f, "bool"),
            ResolvedType::String => write!(f, "string"),
            ResolvedType::Unit => write!(f, "unit"),
            ResolvedType::Null => write!(f, "null"),
            ResolvedType::Option(inner) => write!(f, "Option<{}>", inner),
            ResolvedType::Result(ok, err) => write!(f, "Result<{}, {}>", ok, err),
            ResolvedType::Error => write!(f, "Error"),
            ResolvedType::Never => write!(f, "never"),
            ResolvedType::UntypedNative(name) => write!(f, "untyped native '{}'", name),
            ResolvedType::Function { params, ret } => {
                write!(f, "(")?;
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", p)?;
                }
                write!(f, ") -> {}", ret)
            }
            ResolvedType::Array(inner) => write!(f, "[{}]", inner),
            ResolvedType::FixedArray(inner, length) => write!(f, "[{}; {}]", inner, length),
            ResolvedType::Vec(inner) => write!(f, "vec[{}]", inner),
            ResolvedType::Tuple(elems) => {
                write!(f, "(")?;
                for (i, e) in elems.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", e)?;
                }
                write!(f, ")")
            }
            ResolvedType::Range => write!(f, "range"),
            ResolvedType::Struct(name) => write!(f, "{}", name),
            ResolvedType::Applied { name, args } => {
                write!(f, "{name}<")?;
                for (index, arg) in args.iter().enumerate() {
                    if index > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{arg}")?;
                }
                write!(f, ">")
            }
            ResolvedType::TypeParam(name) => write!(f, "{name}"),
            ResolvedType::TypeVar(id) => write!(f, "?{id}"),
            ResolvedType::Dynamic => write!(f, "dynamic"),
            ResolvedType::Poison => write!(f, "poisoned type"),
            ResolvedType::Uncertain(inner) => write!(f, "?{}", inner),
        }
    }
}
