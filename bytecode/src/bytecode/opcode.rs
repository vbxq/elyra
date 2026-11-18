#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum OpCode {
    Move = 0,
    LoadI,
    LoadK,
    LoadNull,
    LoadBool,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Neg,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Not,
    Jump,
    JumpIf,
    JumpIfNot,
    Call,
    Return,
    Return0,
    GetGlobal,
    SetGlobal,
    JumpLong,
    JumpIfLong,
    JumpIfNotLong,
    ForLoopILong,
    ForLoopIIncLong,
    StringForLoopLong,
    VecForLoopLong,
    ArrayForLoopLong,
    CallWide,
    MakeClosure = 35,
    GetUpval,
    SetUpval,
    CloseUpvals,
    AddGlobalI,
    ForLoopI = 40,
    ForLoopIInc,
    AddI,
    SubI,
    LtImm,
    LeImm,
    GtImm,
    GeImm,
    WhileLoopLt,
    AddII,
    SubII,
    MulII,
    DivII,
    ModII,
    AddFF,
    SubFF,
    MulFF,
    DivFF,
    ModFF,
    LtII,
    LeII,
    GtII,
    GeII,
    EqII,
    NeII,
    LtFF,
    LeFF,
    GtFF,
    GeFF,
    EqFF,
    NeFF,
    LtIImm,
    LeIImm,
    GtIImm,
    GeIImm,
    GetGlobalIdx,
    SetGlobalIdx,
    CallGlobal,
    CallCached = 79,
    CallUpval,
    TailCallUpval,
    AddIIG,
    SubIIG,
    MulIIG,
    DivIIG,
    ModIIG,
    AddFFG,
    SubFFG,
    MulFFG,
    DivFFG,
    ModFFG,
    LtIIG,
    LeIIG,
    GtIIG,
    GeIIG,
    EqIIG,
    NeIIG,
    LtFFG,
    LeFFG,
    GtFFG,
    GeFFG,
    EqFFG,
    NeFFG,
    Shl = 105,
    Shr,
    BitAnd,
    BitOr,
    BitXor,
    BitNot,
    ShlII,
    ShrII,
    AndII,
    OrII,
    XorII,
    NotI,
    ShlIImm,
    ShrIImm,
    AndIImm,
    OrIImm,
    XorIImm,

    ArrayLitWide,
    VecLitWide,
    JumpIfWideLong,
    JumpIfNotWideLong,
    MakeClosureRegisterWide,
    LoopWideLong,

    ArrayNewI = 130,
    ArrayNewF,
    ArrayNewB,
    ArrayNewP,
    ArrayLit,
    ArrayLoadI,
    ArrayLoadF,
    ArrayLoadB,
    ArrayLoadP,
    ArrayGetI,
    ArrayGetF,
    ArrayGetB,
    ArrayGetP,
    ArrayStoreI,
    ArrayStoreF,
    ArrayStoreB,
    ArrayStoreP,
    ArrayLen,

    VecNewI,
    VecNewF,
    VecNewB,
    VecNewP,
    VecLit,
    VecPushI,
    VecPushF,
    VecPushB,
    VecPushP,
    VecPopI,
    VecPopF,
    VecPopB,
    VecPopP,
    VecLen,
    VecCap,
    VecReserve,
    VecLoadI,
    VecLoadF,
    VecLoadB,
    VecLoadP,
    VecGetI,
    VecGetF,
    VecGetB,
    VecGetP,
    VecStoreI,
    VecStoreF,
    VecStoreB,
    VecStoreP,

    StringLoadChar = 176,
    StringForLoop,
    VecForLoop,
    ArrayForLoop,
    LoadKWide,
    MakeClosureWide,
    GetGlobalIdxWide,
    SetGlobalIdxWide,
    Wide,
    LoadUnit,
    LoadNone,
    MakeSum,
    SumTest,
    SumPayload,
    MatchFail,
    Cast,
    RangeNew,
    RangeNewInclusive,
    ArraySlice,
    VecSlice,
    StructNew = 196,
    StructLoad,
    StructStore,
    EnumNew,
    EnumTest,
    EnumLoad,
    AddWI8 = 202,
    AddWI16,
    AddWI32,
    AddWU8,
    AddWU16,
    AddWU32,
    SubWI8,
    SubWI16,
    SubWI32,
    SubWU8,
    SubWU16,
    SubWU32,
    MulWI8,
    MulWI16,
    MulWI32,
    MulWU8,
    MulWU16,
    MulWU32,
    ShlWI8,
    ShlWI16,
    ShlWI32,
    ShlWU8,
    ShlWU16,
    ShlWU32,
    ShrW8,
    ShrW16,
    ShrW32,
    DivWI8,
    DivWI16,
    DivWI32,
    NegW,
    NotW,
    MulShr,
    ShlDiv,
}

#[macro_export]
macro_rules! sized_binary_opcodes {
    () => {
        $crate::OpCode::AddWI8
            | $crate::OpCode::AddWI16
            | $crate::OpCode::AddWI32
            | $crate::OpCode::AddWU8
            | $crate::OpCode::AddWU16
            | $crate::OpCode::AddWU32
            | $crate::OpCode::SubWI8
            | $crate::OpCode::SubWI16
            | $crate::OpCode::SubWI32
            | $crate::OpCode::SubWU8
            | $crate::OpCode::SubWU16
            | $crate::OpCode::SubWU32
            | $crate::OpCode::MulWI8
            | $crate::OpCode::MulWI16
            | $crate::OpCode::MulWI32
            | $crate::OpCode::MulWU8
            | $crate::OpCode::MulWU16
            | $crate::OpCode::MulWU32
            | $crate::OpCode::ShlWI8
            | $crate::OpCode::ShlWI16
            | $crate::OpCode::ShlWI32
            | $crate::OpCode::ShlWU8
            | $crate::OpCode::ShlWU16
            | $crate::OpCode::ShlWU32
            | $crate::OpCode::ShrW8
            | $crate::OpCode::ShrW16
            | $crate::OpCode::ShrW32
            | $crate::OpCode::DivWI8
            | $crate::OpCode::DivWI16
            | $crate::OpCode::DivWI32
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum SizedInt {
    I8 = 0,
    I16 = 1,
    I32 = 2,
    U8 = 3,
    U16 = 4,
    U32 = 5,
}

impl SizedInt {
    pub const ALL: [SizedInt; 6] = [
        SizedInt::I8,
        SizedInt::I16,
        SizedInt::I32,
        SizedInt::U8,
        SizedInt::U16,
        SizedInt::U32,
    ];

    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::I8),
            1 => Some(Self::I16),
            2 => Some(Self::I32),
            3 => Some(Self::U8),
            4 => Some(Self::U16),
            5 => Some(Self::U32),
            _ => None,
        }
    }

    pub const fn bits(self) -> u32 {
        match self {
            Self::I8 | Self::U8 => 8,
            Self::I16 | Self::U16 => 16,
            Self::I32 | Self::U32 => 32,
        }
    }

    pub const fn signed(self) -> bool {
        matches!(self, Self::I8 | Self::I16 | Self::I32)
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::I8 => "i8",
            Self::I16 => "i16",
            Self::I32 => "i32",
            Self::U8 => "u8",
            Self::U16 => "u16",
            Self::U32 => "u32",
        }
    }

    #[inline(always)]
    pub const fn wrap(self, value: i64) -> i64 {
        match self {
            Self::I8 => value as i8 as i64,
            Self::I16 => value as i16 as i64,
            Self::I32 => value as i32 as i64,
            Self::U8 => value as u8 as i64,
            Self::U16 => value as u16 as i64,
            Self::U32 => value as u32 as i64,
        }
    }

    pub const fn min(self) -> i64 {
        match self {
            Self::I8 => i8::MIN as i64,
            Self::I16 => i16::MIN as i64,
            Self::I32 => i32::MIN as i64,
            Self::U8 | Self::U16 | Self::U32 => 0,
        }
    }

    pub const fn max(self) -> i64 {
        match self {
            Self::I8 => i8::MAX as i64,
            Self::I16 => i16::MAX as i64,
            Self::I32 => i32::MAX as i64,
            Self::U8 => u8::MAX as i64,
            Self::U16 => u16::MAX as i64,
            Self::U32 => u32::MAX as i64,
        }
    }

    pub const fn contains(self, value: i64) -> bool {
        value >= self.min() && value <= self.max()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WrappingOp {
    Add,
    Sub,
    Mul,
    Shl,
}

/// the receiver width of `mul_shr` and `shl_div`; `Int` is the 48-bit checked integer
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixedKind {
    Sized(SizedInt),
    Int,
}

impl FixedKind {
    pub const INT_BYTE: u8 = 6;

    pub const fn to_u8(self) -> u8 {
        match self {
            Self::Sized(kind) => kind as u8,
            Self::Int => Self::INT_BYTE,
        }
    }

    pub const fn from_u8(value: u8) -> Option<Self> {
        if value == Self::INT_BYTE {
            return Some(Self::Int);
        }
        match SizedInt::from_u8(value) {
            Some(kind) => Some(Self::Sized(kind)),
            None => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CastTarget {
    Int = 0,
    Float = 1,
    Bool = 2,
    I8 = 3,
    I16 = 4,
    I32 = 5,
    U8 = 6,
    U16 = 7,
    U32 = 8,
}

impl CastTarget {
    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Int),
            1 => Some(Self::Float),
            2 => Some(Self::Bool),
            3 => Some(Self::I8),
            4 => Some(Self::I16),
            5 => Some(Self::I32),
            6 => Some(Self::U8),
            7 => Some(Self::U16),
            8 => Some(Self::U32),
            _ => None,
        }
    }

    pub const fn sized(kind: SizedInt) -> Self {
        match kind {
            SizedInt::I8 => Self::I8,
            SizedInt::I16 => Self::I16,
            SizedInt::I32 => Self::I32,
            SizedInt::U8 => Self::U8,
            SizedInt::U16 => Self::U16,
            SizedInt::U32 => Self::U32,
        }
    }

    pub const fn sized_kind(self) -> Option<SizedInt> {
        match self {
            Self::I8 => Some(SizedInt::I8),
            Self::I16 => Some(SizedInt::I16),
            Self::I32 => Some(SizedInt::I32),
            Self::U8 => Some(SizedInt::U8),
            Self::U16 => Some(SizedInt::U16),
            Self::U32 => Some(SizedInt::U32),
            Self::Int | Self::Float | Self::Bool => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstructionFormat {
    Abc,
    AImm16,
    AOffset32,
    AIndex32,
    Abc16,
    RegisterOffset32,
    RegisterIndex32Aux,
    WideRegisterOffset32,
    WideAbc,
    Struct,
    Fixed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WideRegisterOperands {
    A,
    A2,
    B,
    Ab,
    Abc,
}

impl From<OpCode> for u8 {
    fn from(opcode: OpCode) -> Self {
        opcode as Self
    }
}

impl OpCode {
    pub const fn from_u8(byte: u8) -> Option<Self> {
        if byte <= 77
            || (byte >= 79 && byte <= 103)
            || (byte >= 105 && byte <= 127)
            || (byte >= 130 && byte <= 235)
        {
            Some(unsafe { std::mem::transmute::<u8, OpCode>(byte) })
        } else {
            None
        }
    }

    pub const fn format(self) -> InstructionFormat {
        match self {
            Self::LoadI
            | Self::LoadK
            | Self::Jump
            | Self::JumpIf
            | Self::JumpIfNot
            | Self::GetGlobalIdx
            | Self::SetGlobalIdx
            | Self::ForLoopI
            | Self::ForLoopIInc
            | Self::LtImm
            | Self::LeImm
            | Self::GtImm
            | Self::GeImm
            | Self::WhileLoopLt
            | Self::StringForLoop
            | Self::VecForLoop
            | Self::ArrayForLoop => InstructionFormat::AImm16,
            Self::JumpLong
            | Self::JumpIfLong
            | Self::JumpIfNotLong
            | Self::ForLoopILong
            | Self::ForLoopIIncLong
            | Self::StringForLoopLong
            | Self::VecForLoopLong
            | Self::ArrayForLoopLong => InstructionFormat::AOffset32,
            Self::LoadKWide
            | Self::MakeClosureWide
            | Self::GetGlobalIdxWide
            | Self::SetGlobalIdxWide => InstructionFormat::AIndex32,
            Self::CallWide | Self::ArrayLitWide | Self::VecLitWide => InstructionFormat::Abc16,
            Self::JumpIfWideLong | Self::JumpIfNotWideLong => InstructionFormat::RegisterOffset32,
            Self::MakeClosureRegisterWide => InstructionFormat::RegisterIndex32Aux,
            Self::LoopWideLong => InstructionFormat::WideRegisterOffset32,
            Self::Wide => InstructionFormat::WideAbc,
            Self::StructNew
            | Self::StructLoad
            | Self::StructStore
            | Self::EnumNew
            | Self::EnumTest
            | Self::EnumLoad => InstructionFormat::Struct,
            Self::MulShr | Self::ShlDiv => InstructionFormat::Fixed,
            _ => InstructionFormat::Abc,
        }
    }

    pub const fn extension_words(self) -> usize {
        match self.format() {
            InstructionFormat::AOffset32 | InstructionFormat::AIndex32 => 1,
            InstructionFormat::Abc16
            | InstructionFormat::RegisterOffset32
            | InstructionFormat::RegisterIndex32Aux
            | InstructionFormat::WideRegisterOffset32
            | InstructionFormat::WideAbc => 2,
            InstructionFormat::Struct | InstructionFormat::Fixed => 2,
            InstructionFormat::Abc | InstructionFormat::AImm16 => 0,
        }
    }

    pub const fn wide_register_operands(self) -> Option<WideRegisterOperands> {
        match self {
            Self::LoadI
            | Self::LoadK
            | Self::LoadNull
            | Self::LoadBool
            | Self::LoadUnit
            | Self::LoadNone
            | Self::GetGlobalIdx
            | Self::SetGlobalIdx
            | Self::Return
            | Self::GetUpval
            | Self::CloseUpvals => Some(WideRegisterOperands::A),
            Self::WhileLoopLt => Some(WideRegisterOperands::A2),
            Self::SetUpval => Some(WideRegisterOperands::B),
            Self::Move
            | Self::Neg
            | Self::Not
            | Self::AddI
            | Self::SubI
            | Self::LtIImm
            | Self::LeIImm
            | Self::GtIImm
            | Self::GeIImm
            | Self::BitNot
            | Self::NotI
            | Self::ShlIImm
            | Self::ShrIImm
            | Self::AndIImm
            | Self::OrIImm
            | Self::XorIImm
            | Self::ArrayNewI
            | Self::ArrayNewF
            | Self::ArrayNewB
            | Self::ArrayNewP
            | Self::ArrayLen
            | Self::VecPushI
            | Self::VecPushF
            | Self::VecPushB
            | Self::VecPushP
            | Self::VecPopI
            | Self::VecPopF
            | Self::VecPopB
            | Self::VecPopP
            | Self::VecLen
            | Self::VecCap
            | Self::VecReserve
            | Self::NegW
            | Self::NotW => Some(WideRegisterOperands::Ab),
            Self::VecNewI | Self::VecNewF | Self::VecNewB | Self::VecNewP => {
                Some(WideRegisterOperands::A)
            }
            Self::Add
            | Self::Sub
            | Self::Mul
            | Self::Div
            | Self::Mod
            | Self::Eq
            | Self::Ne
            | Self::Lt
            | Self::Le
            | Self::Gt
            | Self::Ge
            | Self::AddII
            | Self::SubII
            | Self::MulII
            | Self::DivII
            | Self::ModII
            | Self::AddFF
            | Self::SubFF
            | Self::MulFF
            | Self::DivFF
            | Self::ModFF
            | Self::LtII
            | Self::LeII
            | Self::GtII
            | Self::GeII
            | Self::EqII
            | Self::NeII
            | Self::LtFF
            | Self::LeFF
            | Self::GtFF
            | Self::GeFF
            | Self::EqFF
            | Self::NeFF
            | Self::AddIIG
            | Self::SubIIG
            | Self::MulIIG
            | Self::DivIIG
            | Self::ModIIG
            | Self::AddFFG
            | Self::SubFFG
            | Self::MulFFG
            | Self::DivFFG
            | Self::ModFFG
            | Self::LtIIG
            | Self::LeIIG
            | Self::GtIIG
            | Self::GeIIG
            | Self::EqIIG
            | Self::NeIIG
            | Self::LtFFG
            | Self::LeFFG
            | Self::GtFFG
            | Self::GeFFG
            | Self::EqFFG
            | Self::NeFFG
            | Self::Shl
            | Self::Shr
            | Self::BitAnd
            | Self::BitOr
            | Self::BitXor
            | Self::ShlII
            | Self::ShrII
            | Self::AndII
            | Self::OrII
            | Self::XorII
            | Self::ArrayLoadI
            | Self::ArrayLoadF
            | Self::ArrayLoadB
            | Self::ArrayLoadP
            | Self::ArrayGetI
            | Self::ArrayGetF
            | Self::ArrayGetB
            | Self::ArrayGetP
            | Self::ArrayStoreI
            | Self::ArrayStoreF
            | Self::ArrayStoreB
            | Self::ArrayStoreP
            | Self::VecLoadI
            | Self::VecLoadF
            | Self::VecLoadB
            | Self::VecLoadP
            | Self::VecGetI
            | Self::VecGetF
            | Self::VecGetB
            | Self::VecGetP
            | Self::VecStoreI
            | Self::VecStoreF
            | Self::VecStoreB
            | Self::VecStoreP
            | Self::StringLoadChar
            | Self::RangeNew
            | Self::RangeNewInclusive
            | Self::ArraySlice
            | Self::VecSlice => Some(WideRegisterOperands::Abc),
            sized_binary_opcodes!() => Some(WideRegisterOperands::Abc),
            Self::MakeSum | Self::SumTest | Self::SumPayload | Self::Cast | Self::MatchFail => {
                Some(WideRegisterOperands::Ab)
            }
            _ => None,
        }
    }

    pub const fn is_sized_binary(self) -> bool {
        let byte = self as u8;
        byte >= Self::AddWI8 as u8 && byte <= Self::DivWI32 as u8
    }

    pub const fn wrapping(op: WrappingOp, kind: SizedInt) -> Self {
        let base = match op {
            WrappingOp::Add => Self::AddWI8,
            WrappingOp::Sub => Self::SubWI8,
            WrappingOp::Mul => Self::MulWI8,
            WrappingOp::Shl => Self::ShlWI8,
        };
        match Self::from_u8(base as u8 + kind as u8) {
            Some(opcode) => opcode,
            None => unreachable!(),
        }
    }

    pub const fn wrapping_shr(kind: SizedInt) -> Self {
        match kind.bits() {
            8 => Self::ShrW8,
            16 => Self::ShrW16,
            _ => Self::ShrW32,
        }
    }

    /// `None` for an unsigned kind, whose quotient never leaves its range
    pub const fn wrapping_div(kind: SizedInt) -> Option<Self> {
        match kind {
            SizedInt::I8 => Some(Self::DivWI8),
            SizedInt::I16 => Some(Self::DivWI16),
            SizedInt::I32 => Some(Self::DivWI32),
            SizedInt::U8 | SizedInt::U16 | SizedInt::U32 => None,
        }
    }

    pub const fn wrapping_parts(self) -> Option<(WrappingOp, SizedInt)> {
        let byte = self as u8;
        if byte < Self::AddWI8 as u8 || byte > Self::ShlWU32 as u8 {
            return None;
        }
        let offset = byte - Self::AddWI8 as u8;
        let op = match offset / 6 {
            0 => WrappingOp::Add,
            1 => WrappingOp::Sub,
            2 => WrappingOp::Mul,
            _ => WrappingOp::Shl,
        };
        match SizedInt::from_u8(offset % 6) {
            Some(kind) => Some((op, kind)),
            None => None,
        }
    }

    pub const fn supports_wide_registers(self) -> bool {
        self.wide_register_operands().is_some()
    }
}
