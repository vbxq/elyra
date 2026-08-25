use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SchemaId(pub u32);

impl fmt::Display for SchemaId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:08x}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum IntWidth {
    I8 = 0,
    I16 = 1,
    I32 = 2,
    I64 = 3,
    U8 = 4,
    U16 = 5,
    U32 = 6,
    U64 = 7,
}

impl IntWidth {
    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::I8),
            1 => Some(Self::I16),
            2 => Some(Self::I32),
            3 => Some(Self::I64),
            4 => Some(Self::U8),
            5 => Some(Self::U16),
            6 => Some(Self::U32),
            7 => Some(Self::U64),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum FloatWidth {
    F32 = 0,
    F64 = 1,
}

impl FloatWidth {
    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::F32),
            1 => Some(Self::F64),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypeDescriptor {
    Unit,
    Bool,
    Int(IntWidth),
    Float(FloatWidth),
    String,
    Option(Box<TypeDescriptor>),
    Result(Box<TypeDescriptor>, Box<TypeDescriptor>),
    Array(Box<TypeDescriptor>),
    FixedArray(Box<TypeDescriptor>, u32),
    Vec(Box<TypeDescriptor>),
    Struct(u32),
    Enum(u16),
    Function {
        params: Box<[TypeDescriptor]>,
        ret: Box<TypeDescriptor>,
    },
    Any,
    Error,
    Never,
}

impl fmt::Display for TypeDescriptor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unit => formatter.write_str("unit"),
            Self::Bool => formatter.write_str("bool"),
            Self::Int(width) => write!(formatter, "int:{width:?}"),
            Self::Float(width) => write!(formatter, "float:{width:?}"),
            Self::String => formatter.write_str("string"),
            Self::Option(inner) => write!(formatter, "option<{inner}>"),
            Self::Result(ok, err) => write!(formatter, "result<{ok},{err}>"),
            Self::Array(inner) => write!(formatter, "array<{inner}>"),
            Self::FixedArray(inner, len) => write!(formatter, "array<{inner};{len}>"),
            Self::Vec(inner) => write!(formatter, "vec<{inner}>"),
            Self::Struct(schema_id) => write!(formatter, "struct:{schema_id}"),
            Self::Enum(schema_id) => write!(formatter, "enum:{schema_id}"),
            Self::Function { params, ret } => {
                formatter.write_str("fn(")?;
                for (index, param) in params.iter().enumerate() {
                    if index > 0 {
                        formatter.write_str(",")?;
                    }
                    write!(formatter, "{param}")?;
                }
                write!(formatter, ") -> {ret}")
            }
            Self::Any => formatter.write_str("any"),
            Self::Error => formatter.write_str("error"),
            Self::Never => formatter.write_str("never"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StructFieldSchema {
    pub offset: u16,
    pub name: String,
    pub ty: TypeDescriptor,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StructSchema {
    pub schema_id: u32,
    pub ctor: DefId,
    pub type_args: Box<[TypeDescriptor]>,
    pub fields: Box<[StructFieldSchema]>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EnumFieldSchema {
    pub offset: u16,
    pub name: Option<String>,
    pub ty: TypeDescriptor,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DefId {
    pub package: String,
    pub module: Box<[String]>,
    pub ordinal: u32,
}

pub type EnumDefId = DefId;

impl DefId {
    pub fn from_display_name(name: &str, ordinal: u32) -> Self {
        let mut segments = name
            .split("::")
            .filter(|segment| !segment.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>();
        let package = if segments.len() > 1 {
            segments.remove(0)
        } else {
            "elyra".to_string()
        };
        if segments.is_empty() {
            segments.push(name.to_string());
        }
        Self {
            package,
            module: segments.into_boxed_slice(),
            ordinal,
        }
    }

    pub fn display_name(&self) -> String {
        std::iter::once(self.package.as_str())
            .chain(self.module.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join("::")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EnumVariantSchema {
    pub variant_id: u16,
    pub name: String,
    pub fields: Box<[EnumFieldSchema]>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EnumSchema {
    pub schema_id: u16,
    pub def_id: EnumDefId,
    pub arity: u16,
    pub type_args: Box<[TypeDescriptor]>,
    pub name: String,
    pub variants: Box<[EnumVariantSchema]>,
}

impl EnumSchema {
    pub fn new(name: String, variants: Vec<EnumVariantSchema>) -> Self {
        Self::with_id(0, name, variants)
    }

    pub fn with_id(schema_id: u16, name: String, mut variants: Vec<EnumVariantSchema>) -> Self {
        for (variant_id, variant) in variants.iter_mut().enumerate() {
            variant.variant_id = u16::try_from(variant_id).expect("enum variant count exceeds u16");
            for (offset, field) in variant.fields.iter_mut().enumerate() {
                field.offset = u16::try_from(offset).expect("enum field count exceeds u16");
            }
        }
        Self {
            schema_id,
            def_id: EnumDefId::from_display_name(&name, u32::from(schema_id)),
            arity: 0,
            type_args: Box::new([]),
            name,
            variants: variants.into_boxed_slice(),
        }
    }

    pub fn with_identity(
        schema_id: u16,
        def_id: EnumDefId,
        type_args: Vec<TypeDescriptor>,
        name: String,
        variants: Vec<EnumVariantSchema>,
    ) -> Self {
        let mut schema = Self::with_id(schema_id, name, variants);
        schema.def_id = def_id;
        schema.arity =
            u16::try_from(type_args.len()).expect("enum type argument count exceeds u16");
        schema.type_args = type_args.into_boxed_slice();
        schema
    }
}

impl StructSchema {
    pub fn new(name: String, fields: Vec<StructFieldSchema>) -> Self {
        Self::with_id(0, name, fields)
    }

    pub fn with_id(schema_id: u32, name: String, fields: Vec<StructFieldSchema>) -> Self {
        Self::with_identity(
            schema_id,
            DefId::from_display_name(&name, schema_id),
            Vec::new(),
            fields,
        )
    }

    pub fn with_identity(
        schema_id: u32,
        ctor: DefId,
        type_args: Vec<TypeDescriptor>,
        mut fields: Vec<StructFieldSchema>,
    ) -> Self {
        for (offset, field) in fields.iter_mut().enumerate() {
            field.offset = u16::try_from(offset).unwrap_or(u16::MAX);
        }
        Self {
            schema_id,
            ctor,
            type_args: type_args.into_boxed_slice(),
            fields: fields.into_boxed_slice(),
        }
    }

    pub fn display_name(&self) -> String {
        self.ctor.display_name()
    }
}
