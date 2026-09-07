use super::TypeVarId;
use std::collections::HashMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum InferType {
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

    Option(Box<InferType>),
    Result(Box<InferType>, Box<InferType>),
    Error,
    Never,
    Numeric,
    UntypedNative(String),

    Function {
        params: Vec<InferType>,
        ret: Box<InferType>,
    },

    Array(Box<InferType>),
    FixedArray(Box<InferType>, usize),
    Vec(Box<InferType>),
    Tuple(Vec<InferType>),
    Range,

    Struct(std::string::String),

    Applied {
        name: String,
        args: Vec<InferType>,
    },

    Param(String),

    /// an unresolved associated projection `self::item` or `t::item`.
    Projection {
        trait_name: Option<String>,
        item: String,
        self_ty: Box<InferType>,
    },

    Var(TypeVarId),

    Dynamic,

    // internal recovery marker: it never unifies, never lowers to a descriptor and fails the build
    Poison,
}

impl InferType {
    pub fn substitute_params(&self, substitutions: &HashMap<String, InferType>) -> Self {
        match self {
            InferType::Param(name) => substitutions
                .get(name)
                .cloned()
                .unwrap_or_else(|| self.clone()),
            InferType::Function { params, ret } => InferType::Function {
                params: params
                    .iter()
                    .map(|param| param.substitute_params(substitutions))
                    .collect(),
                ret: Box::new(ret.substitute_params(substitutions)),
            },
            InferType::Array(inner) => {
                InferType::Array(Box::new(inner.substitute_params(substitutions)))
            }
            InferType::FixedArray(inner, length) => {
                InferType::FixedArray(Box::new(inner.substitute_params(substitutions)), *length)
            }
            InferType::Vec(inner) => {
                InferType::Vec(Box::new(inner.substitute_params(substitutions)))
            }
            InferType::Option(inner) => {
                InferType::Option(Box::new(inner.substitute_params(substitutions)))
            }
            InferType::Result(ok, err) => InferType::Result(
                Box::new(ok.substitute_params(substitutions)),
                Box::new(err.substitute_params(substitutions)),
            ),
            InferType::Tuple(elements) => InferType::Tuple(
                elements
                    .iter()
                    .map(|element| element.substitute_params(substitutions))
                    .collect(),
            ),
            InferType::Applied { name, args } => InferType::Applied {
                name: name.clone(),
                args: args
                    .iter()
                    .map(|arg| arg.substitute_params(substitutions))
                    .collect(),
            },
            InferType::Projection {
                trait_name,
                item,
                self_ty,
            } => InferType::Projection {
                trait_name: trait_name.clone(),
                item: item.clone(),
                self_ty: Box::new(self_ty.substitute_params(substitutions)),
            },
            _ => self.clone(),
        }
    }

    pub const FOLDABLE_ASSOCIATED_CONST_TYPES: [InferType; 1] = [InferType::I64];

    pub fn folds_an_associated_constant(&self) -> bool {
        Self::FOLDABLE_ASSOCIATED_CONST_TYPES.contains(self)
    }

    pub fn foldable_associated_const_types() -> String {
        Self::FOLDABLE_ASSOCIATED_CONST_TYPES
            .iter()
            .map(InferType::source_spelling)
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub fn is_integer(&self) -> bool {
        matches!(
            self,
            InferType::I8
                | InferType::I16
                | InferType::I32
                | InferType::I64
                | InferType::U8
                | InferType::U16
                | InferType::U32
                | InferType::U64
        )
    }

    pub fn is_float(&self) -> bool {
        matches!(self, InferType::F32 | InferType::F64)
    }

    pub fn is_numeric(&self) -> bool {
        self.is_integer() || self.is_float() || matches!(self, InferType::Numeric)
    }

    pub fn has_vars(&self) -> bool {
        match self {
            InferType::Var(_) | InferType::Param(_) => true,
            InferType::Function { params, ret } => {
                params.iter().any(|p| p.has_vars()) || ret.has_vars()
            }
            InferType::Array(inner) | InferType::Vec(inner) | InferType::Option(inner) => {
                inner.has_vars()
            }
            InferType::FixedArray(inner, _) => inner.has_vars(),
            InferType::Result(ok, err) => ok.has_vars() || err.has_vars(),
            InferType::Tuple(elems) => elems.iter().any(|e| e.has_vars()),
            InferType::Applied { args, .. } => args.iter().any(|arg| arg.has_vars()),
            _ => false,
        }
    }

    pub fn contains_dynamic(&self) -> bool {
        match self {
            InferType::Dynamic => true,
            InferType::Function { params, ret } => {
                params.iter().any(Self::contains_dynamic) || ret.contains_dynamic()
            }
            InferType::Array(inner) | InferType::Vec(inner) | InferType::Option(inner) => {
                inner.contains_dynamic()
            }
            InferType::FixedArray(inner, _) => inner.contains_dynamic(),
            InferType::Result(ok, err) => ok.contains_dynamic() || err.contains_dynamic(),
            InferType::Tuple(elements) => elements.iter().any(Self::contains_dynamic),
            InferType::Applied { args, .. } => args.iter().any(InferType::contains_dynamic),
            _ => false,
        }
    }

    pub fn contains_poison(&self) -> bool {
        match self {
            InferType::Poison => true,
            InferType::Function { params, ret } => {
                params.iter().any(Self::contains_poison) || ret.contains_poison()
            }
            InferType::Array(inner) | InferType::Vec(inner) | InferType::Option(inner) => {
                inner.contains_poison()
            }
            InferType::FixedArray(inner, _) => inner.contains_poison(),
            InferType::Result(ok, err) => ok.contains_poison() || err.contains_poison(),
            InferType::Tuple(elements) => elements.iter().any(Self::contains_poison),
            InferType::Applied { args, .. } => args.iter().any(Self::contains_poison),
            InferType::Projection { self_ty, .. } => self_ty.contains_poison(),
            _ => false,
        }
    }

    pub fn mentions_param(&self, name: &str) -> bool {
        match self {
            InferType::Param(param) => param == name,
            InferType::Function { params, ret } => {
                params.iter().any(|param| param.mentions_param(name)) || ret.mentions_param(name)
            }
            InferType::Array(inner) | InferType::Vec(inner) | InferType::Option(inner) => {
                inner.mentions_param(name)
            }
            InferType::FixedArray(inner, _) => inner.mentions_param(name),
            InferType::Result(ok, err) => ok.mentions_param(name) || err.mentions_param(name),
            InferType::Tuple(elements) => elements.iter().any(|el| el.mentions_param(name)),
            InferType::Applied { args, .. } => args.iter().any(|arg| arg.mentions_param(name)),
            InferType::Projection { self_ty, .. } => self_ty.mentions_param(name),
            _ => false,
        }
    }

    pub fn is_resolved(&self) -> bool {
        !self.has_vars()
    }

    pub fn item_name(&self) -> &str {
        match self {
            InferType::Projection { item, .. } => item,
            _ => "",
        }
    }

    pub fn self_ty(&self) -> &InferType {
        match self {
            InferType::Projection { self_ty, .. } => self_ty,
            _ => self,
        }
    }

    pub fn is_concrete(&self) -> bool {
        match self {
            InferType::Function { params, ret } => {
                params.iter().all(InferType::is_concrete) && ret.is_concrete()
            }
            InferType::Array(inner)
            | InferType::FixedArray(inner, _)
            | InferType::Vec(inner)
            | InferType::Option(inner) => inner.is_concrete(),
            InferType::Result(ok, error) => ok.is_concrete() && error.is_concrete(),
            InferType::Tuple(elements) => elements.iter().all(InferType::is_concrete),
            InferType::Applied { args, .. } => args.iter().all(InferType::is_concrete),
            InferType::UntypedNative(_)
            | InferType::Var(_)
            | InferType::Param(_)
            | InferType::Projection { .. }
            | InferType::Dynamic
            | InferType::Poison => false,
            InferType::I8
            | InferType::I16
            | InferType::I32
            | InferType::I64
            | InferType::U8
            | InferType::U16
            | InferType::U32
            | InferType::U64
            | InferType::F32
            | InferType::F64
            | InferType::Bool
            | InferType::String
            | InferType::Unit
            | InferType::Null
            | InferType::Error
            | InferType::Never
            | InferType::Numeric
            | InferType::Struct(_)
            | InferType::Range => true,
        }
    }

    pub fn from_annotation(ann: &aelys_syntax::TypeAnnotation) -> Self {
        if ann.is_function_type() {
            let params = ann
                .fn_params
                .as_ref()
                .map(|ps| ps.iter().map(Self::from_annotation).collect())
                .unwrap_or_default();
            let ret = ann
                .fn_ret
                .as_ref()
                .map(|r| Self::from_annotation(r))
                .unwrap_or(InferType::Unit);
            return InferType::Function {
                params,
                ret: Box::new(ret),
            };
        }
        let name_lower = ann.name.to_lowercase();
        match name_lower.as_str() {
            "int" | "i64" | "int64" => InferType::I64,
            "i8" | "int8" => InferType::I8,
            "i16" | "int16" => InferType::I16,
            "i32" | "int32" => InferType::I32,
            "u8" | "uint8" => InferType::U8,
            "u16" | "uint16" => InferType::U16,
            "u32" | "uint32" => InferType::U32,
            "u64" | "uint64" => InferType::U64,
            "float" | "f64" | "float64" => InferType::F64,
            "f32" | "float32" => InferType::F32,
            "bool" => InferType::Bool,
            "string" => InferType::String,
            "unit" | "void" => InferType::Unit,
            "dynamic" => InferType::Dynamic,
            "null" => InferType::Null,
            "array" => {
                let inner = ann
                    .type_params
                    .first()
                    .map(Self::from_annotation)
                    .unwrap_or(InferType::Dynamic);
                match ann.array_length {
                    Some(length) => InferType::FixedArray(Box::new(inner), length as usize),
                    None => InferType::Array(Box::new(inner)),
                }
            }
            "vec" => {
                let inner = ann
                    .type_params
                    .first()
                    .map(Self::from_annotation)
                    .unwrap_or(InferType::Dynamic);
                InferType::Vec(Box::new(inner))
            }
            "option" => {
                let inner = ann
                    .type_params
                    .first()
                    .map(Self::from_annotation)
                    .unwrap_or(InferType::Dynamic);
                InferType::Option(Box::new(inner))
            }
            "result" => {
                let ok = ann
                    .type_params
                    .first()
                    .map(Self::from_annotation)
                    .unwrap_or(InferType::Dynamic);
                let err = ann
                    .type_params
                    .get(1)
                    .map(Self::from_annotation)
                    .unwrap_or(InferType::Dynamic);
                InferType::Result(Box::new(ok), Box::new(err))
            }
            "error" => InferType::Error,
            _ => {
                if ann.name.chars().next().is_some_and(|c| c.is_uppercase()) {
                    if ann.type_params.is_empty() {
                        InferType::Struct(ann.name.clone())
                    } else {
                        InferType::Applied {
                            name: ann.name.clone(),
                            args: ann.type_params.iter().map(Self::from_annotation).collect(),
                        }
                    }
                } else {
                    InferType::Dynamic
                }
            }
        }
    }

    pub fn from_name(name: &str) -> Self {
        match name.to_lowercase().as_str() {
            "int" | "i64" | "int64" => InferType::I64,
            "i8" | "int8" => InferType::I8,
            "i16" | "int16" => InferType::I16,
            "i32" | "int32" => InferType::I32,
            "u8" | "uint8" => InferType::U8,
            "u16" | "uint16" => InferType::U16,
            "u32" | "uint32" => InferType::U32,
            "u64" | "uint64" => InferType::U64,
            "float" | "f64" | "float64" => InferType::F64,
            "f32" | "float32" => InferType::F32,
            "bool" => InferType::Bool,
            "string" => InferType::String,
            "unit" | "void" => InferType::Unit,
            "dynamic" => InferType::Dynamic,
            "null" => InferType::Null,
            "option" => InferType::Option(Box::new(InferType::Dynamic)),
            "result" => {
                InferType::Result(Box::new(InferType::Dynamic), Box::new(InferType::Dynamic))
            }
            "error" => InferType::Error,
            _ => {
                if name.chars().next().is_some_and(|c| c.is_uppercase()) {
                    InferType::Struct(name.to_string())
                } else {
                    InferType::Dynamic
                }
            }
        }
    }

    pub fn as_var_id(&self) -> Option<TypeVarId> {
        match self {
            InferType::Var(id) => Some(*id),
            _ => None,
        }
    }

    pub fn int_fits(value: i64, ty: &InferType) -> bool {
        match ty {
            InferType::I8 => i8::try_from(value).is_ok(),
            InferType::I16 => i16::try_from(value).is_ok(),
            InferType::I32 => i32::try_from(value).is_ok(),
            InferType::I64 => true,
            InferType::U8 => u8::try_from(value).is_ok(),
            InferType::U16 => u16::try_from(value).is_ok(),
            InferType::U32 => u32::try_from(value).is_ok(),
            InferType::U64 => value >= 0,
            _ => false,
        }
    }

    pub fn all_integer_types() -> Vec<InferType> {
        vec![
            InferType::I8,
            InferType::I16,
            InferType::I32,
            InferType::I64,
            InferType::U8,
            InferType::U16,
            InferType::U32,
            InferType::U64,
        ]
    }

    pub fn all_float_types() -> Vec<InferType> {
        vec![InferType::F32, InferType::F64]
    }

    pub fn all_numeric_types() -> Vec<InferType> {
        let mut types = Self::all_integer_types();
        types.extend(Self::all_float_types());
        types
    }
}

impl fmt::Display for InferType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InferType::I8 => write!(f, "i8"),
            InferType::I16 => write!(f, "i16"),
            InferType::I32 => write!(f, "i32"),
            InferType::I64 => write!(f, "i64"),
            InferType::U8 => write!(f, "u8"),
            InferType::U16 => write!(f, "u16"),
            InferType::U32 => write!(f, "u32"),
            InferType::U64 => write!(f, "u64"),
            InferType::F32 => write!(f, "f32"),
            InferType::F64 => write!(f, "f64"),
            InferType::Bool => write!(f, "bool"),
            InferType::String => write!(f, "string"),
            InferType::Unit => write!(f, "unit"),
            InferType::Null => write!(f, "null"),
            InferType::Option(inner) => write!(f, "Option<{}>", inner),
            InferType::Result(ok, err) => write!(f, "Result<{}, {}>", ok, err),
            InferType::Error => write!(f, "Error"),
            InferType::Never => write!(f, "never"),
            InferType::Numeric => write!(f, "number"),
            InferType::UntypedNative(name) => write!(f, "untyped native '{}'", name),
            InferType::Function { params, ret } => {
                write!(f, "(")?;
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", p)?;
                }
                write!(f, ") -> {}", ret)
            }
            InferType::Array(inner) => write!(f, "[{}]", inner),
            InferType::FixedArray(inner, length) => write!(f, "[{}; {}]", inner, length),
            InferType::Vec(inner) => write!(f, "vec[{}]", inner),
            InferType::Tuple(elems) => {
                write!(f, "(")?;
                for (i, e) in elems.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", e)?;
                }
                write!(f, ")")
            }
            InferType::Range => write!(f, "range"),
            InferType::Struct(name) => write!(f, "{}", name),
            InferType::Applied { name, args } => {
                write!(f, "{name}<")?;
                for (index, arg) in args.iter().enumerate() {
                    if index > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{arg}")?;
                }
                write!(f, ">")
            }
            InferType::Param(name) => write!(f, "{name}"),
            InferType::Projection {
                trait_name,
                item,
                self_ty,
            } => match trait_name {
                Some(trait_name) => write!(f, "<{} as {}>::{}", self_ty, trait_name, item),
                None => write!(f, "{}::{}", self_ty, item),
            },
            InferType::Var(_) => write!(f, "inferred type"),
            InferType::Dynamic => write!(f, "dynamic"),
            InferType::Poison => write!(f, "poisoned type"),
        }
    }
}

impl InferType {
    /// new type cannot reach a message under its internal spelling
    pub fn source_spelling(&self) -> String {
        match self {
            InferType::I64 => "int".to_string(),
            InferType::F64 => "float".to_string(),
            InferType::Vec(inner) => format!("Vec<{}>", inner.source_spelling()),
            InferType::Option(inner) => format!("Option<{}>", inner.source_spelling()),
            InferType::Result(ok, err) => format!(
                "Result<{}, {}>",
                ok.source_spelling(),
                err.source_spelling()
            ),
            InferType::Array(inner) => format!("[{}]", inner.source_spelling()),
            InferType::FixedArray(inner, length) => {
                format!("[{}; {length}]", inner.source_spelling())
            }
            InferType::Function { params, ret } => format!(
                "fn({}) -> {}",
                params
                    .iter()
                    .map(InferType::source_spelling)
                    .collect::<Vec<_>>()
                    .join(", "),
                ret.source_spelling()
            ),
            InferType::Applied { name, args } => format!(
                "{name}<{}>",
                args.iter()
                    .map(InferType::source_spelling)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            InferType::Tuple(elements) => format!(
                "({})",
                elements
                    .iter()
                    .map(InferType::source_spelling)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            InferType::Projection { item, self_ty, .. } => {
                format!("{}::{}", self_ty.source_spelling(), item)
            }
            InferType::I8
            | InferType::I16
            | InferType::I32
            | InferType::U8
            | InferType::U16
            | InferType::U32
            | InferType::U64
            | InferType::F32
            | InferType::Bool
            | InferType::String
            | InferType::Unit
            | InferType::Null
            | InferType::Error
            | InferType::Never
            | InferType::Numeric
            | InferType::UntypedNative(_)
            | InferType::Range
            | InferType::Struct(_)
            | InferType::Param(_)
            | InferType::Var(_)
            | InferType::Dynamic
            | InferType::Poison => self.to_string(),
        }
    }
}
