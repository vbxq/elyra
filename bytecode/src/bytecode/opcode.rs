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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CastTarget {
    Int = 0,
    Float = 1,
    Bool = 2,
}

impl CastTarget {
    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Int),
            1 => Some(Self::Float),
            2 => Some(Self::Bool),
            _ => None,
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
    pub fn from_u8(byte: u8) -> Option<Self> {
        if byte <= 77
            || (79..=103).contains(&byte)
            || (105..=127).contains(&byte)
            || (130..=195).contains(&byte)
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
            | Self::VecReserve => Some(WideRegisterOperands::Ab),
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
            Self::MakeSum | Self::SumTest | Self::SumPayload | Self::Cast | Self::MatchFail => {
                Some(WideRegisterOperands::Ab)
            }
            _ => None,
        }
    }

    pub const fn supports_wide_registers(self) -> bool {
        self.wide_register_operands().is_some()
    }
}
