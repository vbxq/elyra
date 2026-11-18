use aelys_bytecode::{
    Constant, Function, InstructionFormat, IntWidth, OpCode, TypeDescriptor, Value,
};
use aelys_runtime::{
    FRAME_STATUS_CALL_IN_INTERPRETER, FRAME_STATUS_EXIT, FRAME_STATUS_NATIVE_ERROR, FrameCode,
    FrameCodeInfo, FrameCollectionKind, FrameData, FrameJitContext, FrameSiteKind,
};
use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::{
    AbiParam, Block, BlockArg, FuncRef, InstBuilder, MemFlagsData, SigRef, Signature, UserFuncName,
    Value as ClifValue, types,
};
use cranelift_codegen::isa::OwnedTargetIsa;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{Linkage, Module};
use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex};

const MAX_FRAME_REGISTERS: usize = 256;
const PAYLOAD: u64 = 0x0000_FFFF_FFFF_FFFF;
const TAG_BITS: u64 = 0x7FFF_0000_0000_0000;
const QNAN: u64 = 0x7FF8_0000_0000_0000;
const CHAR_FLAG: u64 = 0x0000_8000_0000_0000;

pub(crate) struct FrameCompiled {
    info: FrameCodeInfo,
    _module: Mutex<JITModule>,
}

impl FrameCode for FrameCompiled {
    fn info(&self) -> &FrameCodeInfo {
        &self.info
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FrameRefusal {
    Registers,
    Decode,
    NoEntry,
    EmptyEntry,
    Captures,
    Codegen(String),
}

// an open upvalue aliases a register of this frame, which machine code keeps in its own variables
fn captures_locals(function: &Function) -> bool {
    function.nested_functions.iter().any(|nested| {
        nested
            .upvalue_descriptors
            .iter()
            .any(|descriptor| descriptor.is_local)
    })
}

#[derive(Clone, Copy, Debug)]
struct Inst {
    ip: usize,
    next: usize,
    op: OpCode,
    a: usize,
    b: usize,
    c: usize,
    imm: i64,
    d: usize,
    target: Option<usize>,
    valid: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Flow {
    Next,
    Jump(usize),
    Branch(usize),
    Return,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum CacheKind {
    Collection(u64),
    Struct(usize),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum FieldCheck {
    Int(IntWidth),
    Float,
    Bool,
    Unit,
}

#[derive(Clone, Debug)]
struct Effect {
    uses: Vec<usize>,
    def: Option<usize>,
    flow: Flow,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Live([u64; 4]);

impl Live {
    fn all(count: usize) -> Self {
        let mut live = Self::default();
        for register in 0..count {
            live.insert(register);
        }
        live
    }
    fn insert(&mut self, register: usize) {
        self.0[register / 64] |= 1 << (register % 64);
    }
    fn remove(&mut self, register: usize) {
        self.0[register / 64] &= !(1 << (register % 64));
    }
    fn contains(&self, register: usize) -> bool {
        self.0[register / 64] & (1 << (register % 64)) != 0
    }
    fn union(&mut self, other: &Self) {
        for (left, right) in self.0.iter_mut().zip(other.0) {
            *left |= right;
        }
    }
    fn registers(&self, count: usize) -> impl Iterator<Item = usize> + '_ {
        (0..count).filter(|register| self.contains(*register))
    }
}

fn decode(function: &Function) -> Option<Vec<Inst>> {
    let words = function.bytecode.as_slice();
    let mut decoded = Vec::new();
    let mut ip = 0usize;
    while ip < words.len() {
        let word = words[ip];
        let op = OpCode::from_u8(u8::try_from(word >> 24).ok()?)?;
        let next = ip.checked_add(op.extension_words().checked_add(1)?)?;
        if next > words.len() {
            return None;
        }
        let byte = |shift: u32| usize::try_from((word >> shift) & 0xff).unwrap_or(0);
        let mut inst = Inst {
            ip,
            next,
            op,
            a: byte(16),
            b: byte(8),
            c: byte(0),
            imm: 0,
            d: 0,
            target: None,
            valid: true,
        };
        match op.format() {
            InstructionFormat::Abc => {}
            InstructionFormat::AImm16 => {
                let immediate =
                    i16::from_ne_bytes(u16::try_from(word & 0xffff).ok()?.to_ne_bytes());
                inst.imm = i64::from(immediate);
            }
            InstructionFormat::AOffset32 | InstructionFormat::AIndex32 => {
                inst.imm = match op.format() {
                    InstructionFormat::AOffset32 => {
                        i64::from(i32::from_ne_bytes(words[ip + 1].to_ne_bytes()))
                    }
                    _ => i64::from(words[ip + 1]),
                };
            }
            InstructionFormat::Struct => {
                let first = words[ip + 1];
                let second = words[ip + 2];
                inst.imm = i64::from(word & 0xffff);
                inst.a = usize::try_from(first >> 16).ok()?;
                inst.b = usize::try_from(first & 0xffff).ok()?;
                inst.c = usize::try_from(second >> 16).ok()?;
                inst.d = usize::try_from(second & 0xffff).ok()?;
                inst.valid = word & 0x00ff_0000 == 0
                    && (inst.d == 0 || matches!(op, OpCode::EnumLoad | OpCode::EnumNew));
            }
            InstructionFormat::Fixed => {
                let first = words[ip + 1];
                let second = words[ip + 2];
                inst.imm = i64::from(word & 0x00ff_ffff);
                inst.a = usize::try_from(first >> 16).ok()?;
                inst.b = usize::try_from(first & 0xffff).ok()?;
                inst.c = usize::try_from(second >> 16).ok()?;
                inst.d = usize::try_from(second & 0xffff).ok()?;
            }
            _ => inst.valid = false,
        }
        if is_jump(op) {
            let offset = isize::try_from(inst.imm).ok()?;
            let target = isize::try_from(next).ok()?.checked_add(offset)?;
            inst.target = Some(usize::try_from(target).ok()?);
        }
        decoded.push(inst);
        ip = next;
    }
    Some(decoded)
}

fn is_jump(op: OpCode) -> bool {
    matches!(
        op,
        OpCode::Jump
            | OpCode::JumpIf
            | OpCode::JumpIfNot
            | OpCode::JumpLong
            | OpCode::JumpIfLong
            | OpCode::JumpIfNotLong
            | OpCode::ForLoopI
            | OpCode::ForLoopIInc
            | OpCode::ForLoopILong
            | OpCode::ForLoopIIncLong
            | OpCode::WhileLoopLt
            | OpCode::StringForLoop
            | OpCode::VecForLoop
            | OpCode::ArrayForLoop
            | OpCode::StringForLoopLong
            | OpCode::VecForLoopLong
            | OpCode::ArrayForLoopLong
    )
}

struct Analysis<'a> {
    function: &'a Function,
    registers: usize,
}

impl Analysis<'_> {
    fn global_name(&self, index: usize) -> Option<&str> {
        self.function
            .global_layout
            .names()
            .get(index)
            .map(String::as_str)
    }

    fn field_check(&self, schema: usize, field: usize) -> Option<FieldCheck> {
        let schema = self.function.struct_schemas.get(schema)?;
        match &schema.fields.get(field)?.ty {
            TypeDescriptor::Int(width) => Some(FieldCheck::Int(*width)),
            TypeDescriptor::Float(_) => Some(FieldCheck::Float),
            TypeDescriptor::Bool => Some(FieldCheck::Bool),
            TypeDescriptor::Unit => Some(FieldCheck::Unit),
            _ => None,
        }
    }

    fn constant_supported(&self, index: usize) -> bool {
        matches!(
            self.function.constants.get(index),
            Some(
                Constant::Null
                    | Constant::Bool(_)
                    | Constant::Int(_)
                    | Constant::Float(_)
                    | Constant::String(_)
            )
        )
    }

    fn effect(&self, inst: &Inst) -> Option<Effect> {
        use OpCode::*;
        if !inst.valid {
            return None;
        }
        let (a, b, c) = (inst.a, inst.b, inst.c);
        let simple = |uses: Vec<usize>, def: Option<usize>| Effect {
            uses,
            def,
            flow: Flow::Next,
        };
        let effect = match inst.op {
            Move => simple(vec![b], Some(a)),
            LoadI | LoadNull | LoadBool | LoadUnit | LoadNone => simple(vec![], Some(a)),
            LoadK | LoadKWide => {
                if !self.constant_supported(constant_index(inst)?) {
                    return None;
                }
                simple(vec![], Some(a))
            }
            GetGlobalIdx | GetGlobalIdxWide => {
                self.named_global(self.global_index(inst)?)?;
                simple(vec![], Some(a))
            }
            SetGlobalIdx | SetGlobalIdxWide => {
                self.named_global(self.global_index(inst)?)?;
                simple(vec![a], None)
            }
            AddGlobalI => {
                self.named_global(c)?;
                simple(vec![b], Some(a))
            }
            Add | Sub | Mul | AddII | SubII | MulII | DivII | ModII | AddFF | SubFF | MulFF
            | DivFF | ModFF | AddIIG | SubIIG | MulIIG | DivIIG | ModIIG | Eq | Ne | Lt | Le
            | Gt | Ge | LtII | LeII | GtII | GeII | EqII | NeII | LtFF | LeFF | GtFF | GeFF
            | EqFF | NeFF | LtIIG | LeIIG | GtIIG | GeIIG | EqIIG | NeIIG | Shl | Shr | BitAnd
            | BitOr | BitXor | ShlII | ShrII | AndII | OrII | XorII => simple(vec![b, c], Some(a)),
            Neg | Not | AddI | SubI | LtImm | LeImm | GtImm | GeImm | LtIImm | LeIImm | GtIImm
            | GeIImm | BitNot | NotI | ShlIImm | ShrIImm | AndIImm | OrIImm | XorIImm => {
                simple(vec![b], Some(a))
            }
            Jump | JumpLong => Effect {
                uses: vec![],
                def: None,
                flow: Flow::Jump(inst.target?),
            },
            JumpIf | JumpIfNot | JumpIfLong | JumpIfNotLong => Effect {
                uses: vec![a],
                def: None,
                flow: Flow::Branch(inst.target?),
            },
            ForLoopI | ForLoopIInc | ForLoopILong | ForLoopIIncLong => Effect {
                uses: vec![a, a + 1, a + 2],
                def: Some(a),
                flow: Flow::Branch(inst.target?),
            },
            WhileLoopLt => Effect {
                uses: vec![a, a + 1],
                def: None,
                flow: Flow::Branch(inst.target?),
            },
            Return => Effect {
                uses: vec![a],
                def: None,
                flow: Flow::Return,
            },
            Return0 => Effect {
                uses: vec![],
                def: None,
                flow: Flow::Return,
            },
            CallGlobal => {
                self.named_global(b)?;
                simple((a + 1..a + 1 + c).collect(), Some(a))
            }
            Call => {
                let mut uses = vec![b];
                uses.extend(b + 1..b + 1 + c);
                simple(uses, Some(a))
            }
            CallCached => {
                let mut uses = vec![b];
                uses.extend(a + 1..a + 1 + c);
                simple(uses, Some(a))
            }
            ArrayLoadI | ArrayLoadF | ArrayLoadB | ArrayLoadP | VecLoadI | VecLoadF | VecLoadB
            | VecLoadP => simple(vec![b, c], Some(a)),
            ArrayStoreI | ArrayStoreF | ArrayStoreB | ArrayStoreP | VecStoreI | VecStoreF
            | VecStoreB | VecStoreP => simple(vec![a, b, c], None),
            ArrayLen | VecLen => simple(vec![b], Some(a)),
            VecPushI | VecPushF | VecPushB | VecPushP => simple(vec![a, b], None),
            StructLoad => {
                let schema = usize::try_from(inst.imm).ok()?;
                self.function.struct_schemas.get(schema)?.fields.get(c)?;
                simple(vec![b], Some(a))
            }
            StructStore => {
                let schema = usize::try_from(inst.imm).ok()?;
                self.field_check(schema, c)?;
                simple(vec![a, b], None)
            }
            MulShr | ShlDiv => {
                if fixed_kind(inst) != aelys_bytecode::FixedKind::INT_BYTE {
                    return None;
                }
                if fixed_mode(inst) == 0 {
                    simple(vec![b, c], Some(a))
                } else {
                    simple(vec![b, c, inst.d], Some(a))
                }
            }
            EnumTest => {
                let schema = usize::try_from(inst.imm).ok()?;
                self.function.enum_schemas.get(schema)?.variants.get(c)?;
                simple(vec![b], Some(a))
            }
            EnumLoad => {
                let schema = usize::try_from(inst.imm).ok()?;
                let variant = self.function.enum_schemas.get(schema)?.variants.get(c)?;
                if variant.fields.get(inst.d)?.offset != u16::try_from(inst.d).ok()? {
                    return None;
                }
                simple(vec![b], Some(a))
            }
            VecForLoop | VecForLoopLong | ArrayForLoop | ArrayForLoopLong => Effect {
                uses: vec![a + 1, a + 2],
                def: None,
                flow: Flow::Branch(inst.target?),
            },
            _ => return None,
        };
        if effect
            .uses
            .iter()
            .chain(effect.def.iter())
            .any(|register| *register >= self.registers)
        {
            return None;
        }
        Some(effect)
    }

    fn global_index(&self, inst: &Inst) -> Option<usize> {
        match inst.op {
            OpCode::GetGlobalIdx | OpCode::SetGlobalIdx => usize::try_from(inst.imm & 0xffff).ok(),
            OpCode::GetGlobalIdxWide | OpCode::SetGlobalIdxWide => usize::try_from(inst.imm).ok(),
            _ => None,
        }
    }

    fn named_global(&self, index: usize) -> Option<usize> {
        self.global_name(index)
            .is_some_and(|name| !name.is_empty())
            .then_some(index)
    }

    fn uses_globals(&self, inst: &Inst) -> bool {
        matches!(
            inst.op,
            OpCode::GetGlobalIdx
                | OpCode::GetGlobalIdxWide
                | OpCode::SetGlobalIdx
                | OpCode::SetGlobalIdxWide
                | OpCode::AddGlobalI
                | OpCode::CallGlobal
        )
    }

    fn live_effect(&self, inst: &Inst) -> LiveEffect {
        use OpCode::*;
        if let Some(effect) = self.effect(inst) {
            return LiveEffect {
                uses: Some(effect.uses),
                def: effect.def,
                flow: effect.flow,
            };
        }
        let every = || LiveEffect {
            uses: None,
            def: None,
            flow: Flow::Return,
        };
        let (a, b, c) = (inst.a, inst.b, inst.c);
        let words = self.function.bytecode.as_slice();
        let simple = |uses: Vec<usize>, def: Option<usize>| LiveEffect {
            uses: Some(uses),
            def,
            flow: Flow::Next,
        };
        let effect = match inst.op {
            Div | Mod | AddFFG | SubFFG | MulFFG | DivFFG | ModFFG | LtFFG | LeFFG | GtFFG
            | GeFFG | EqFFG | NeFFG | ArrayGetI | ArrayGetF | ArrayGetB | ArrayGetP | VecGetI
            | VecGetF | VecGetB | VecGetP | StringLoadChar | RangeNew | RangeNewInclusive
            | ArraySlice | VecSlice
                if inst.valid =>
            {
                simple(vec![b, c], Some(a))
            }
            GetGlobal | GetUpval | VecNewI | VecNewF | VecNewB | VecNewP if inst.valid => {
                simple(vec![], Some(a))
            }
            SetGlobal if inst.valid => simple(vec![a], None),
            SetUpval if inst.valid => simple(vec![b], None),
            ArrayNewI | ArrayNewF | ArrayNewB | ArrayNewP | VecPopI | VecPopF | VecPopB
            | VecPopP | VecCap | Cast | MakeSum | SumTest | SumPayload
                if inst.valid =>
            {
                simple(vec![b], Some(a))
            }
            VecReserve if inst.valid => simple(vec![a, b], None),
            ArrayLit | VecLit if inst.valid => simple((b..b + c).collect(), Some(a)),
            CallUpval if inst.valid => simple((a + 1..a + 1 + c).collect(), Some(a)),
            TailCallUpval if inst.valid => LiveEffect {
                uses: Some((a + 1..a + 1 + c).collect()),
                def: None,
                flow: Flow::Return,
            },
            MatchFail if inst.valid => LiveEffect {
                uses: Some(if c == 1 { vec![b] } else { vec![] }),
                def: None,
                flow: Flow::Return,
            },
            StringForLoop | VecForLoop | ArrayForLoop | StringForLoopLong | VecForLoopLong
            | ArrayForLoopLong
                if inst.valid =>
            {
                match inst.target {
                    Some(target) => LiveEffect {
                        uses: Some(vec![a, a + 1, a + 2]),
                        def: None,
                        flow: Flow::Branch(target),
                    },
                    None => every(),
                }
            }
            MulShr | ShlDiv => simple(vec![b, c, inst.d], Some(a)),
            StructNew | EnumNew | EnumTest | EnumLoad => {
                let (Some(first), Some(second)) = (words.get(inst.ip + 1), words.get(inst.ip + 2))
                else {
                    return every();
                };
                let destination = (first >> 16) as usize;
                let source = (first & 0xffff) as usize;
                let uses = match inst.op {
                    StructNew => (source..source + (second >> 16) as usize).collect(),
                    EnumNew => (source..source + (second & 0xffff) as usize).collect(),
                    _ => vec![source],
                };
                simple(uses, Some(destination))
            }
            _ => every(),
        };
        let out_of_range = effect
            .uses
            .iter()
            .flatten()
            .chain(effect.def.iter())
            .any(|register| *register >= self.registers);
        if out_of_range { every() } else { effect }
    }
}

// registers whose every definition in the region is an integer, so machine code keeps them unboxed
fn integer_registers(
    analysis: &Analysis<'_>,
    region: &Region,
    instructions: &[Inst],
    by_ip: &HashMap<usize, usize>,
) -> Live {
    use OpCode::*;
    let insts = region
        .order
        .iter()
        .filter(|ip| matches!(region.effects.get(ip), Some(Some(_))))
        .filter_map(|ip| by_ip.get(ip).map(|index| instructions[*index]))
        .collect::<Vec<_>>();
    let mut candidate = Live::all(analysis.registers);
    let mut wanted = Live::default();
    let mut counters = Live::default();
    let entry = region
        .live_in
        .get(&region.entry)
        .copied()
        .unwrap_or_default();
    for inst in &insts {
        let loop_registers: &[usize] = match inst.op {
            ForLoopI | ForLoopIInc | ForLoopILong | ForLoopIIncLong => {
                &[inst.a, inst.a + 1, inst.a + 2]
            }
            WhileLoopLt => &[inst.a, inst.a + 1],
            _ => &[],
        };
        for register in loop_registers {
            counters.insert(*register);
        }
    }
    for inst in &insts {
        let int_uses: &[usize] = match inst.op {
            AddII | SubII | MulII | DivII | ModII | LtII | LeII | GtII | GeII | EqII | NeII
            | ShlII | ShrII | AndII | OrII | XorII => &[inst.b, inst.c],
            NotI | AddI | SubI | LtIImm | LeIImm | GtIImm | GeIImm | ShlIImm | ShrIImm
            | AndIImm | OrIImm | XorIImm => &[inst.b],
            ForLoopI | ForLoopIInc | ForLoopILong | ForLoopIIncLong => {
                &[inst.a, inst.a + 1, inst.a + 2]
            }
            WhileLoopLt => &[inst.a, inst.a + 1],
            ArrayLoadI | ArrayLoadF | ArrayLoadB | ArrayLoadP | VecLoadI | VecLoadF | VecLoadB
            | VecLoadP => &[inst.c],
            ArrayStoreI | VecStoreI => &[inst.b, inst.c],
            MulShr | ShlDiv => &[inst.b, inst.c],
            ArrayStoreF | ArrayStoreB | ArrayStoreP | VecStoreF | VecStoreB | VecStoreP => {
                &[inst.b]
            }
            _ => &[],
        };
        for register in int_uses {
            wanted.insert(*register);
        }
    }
    for inst in &insts {
        match inst.op {
            VecForLoop | VecForLoopLong | ArrayForLoop | ArrayForLoopLong => {
                wanted.insert(inst.a);
                wanted.insert(inst.a + 1);
            }
            _ => {
                if let Some(def) = analysis.effect(inst).and_then(|effect| effect.def)
                    && def < analysis.registers
                {
                    wanted.insert(def);
                }
            }
        }
    }
    loop {
        let mut changed = false;
        for inst in &insts {
            let Some(effect) = analysis.effect(inst) else {
                continue;
            };
            let mut defs = Vec::new();
            match inst.op {
                VecForLoop | VecForLoopLong | ArrayForLoop | ArrayForLoopLong => {
                    defs.push((inst.a, false));
                    defs.push((inst.a + 1, true));
                }
                _ => {
                    if let Some(def) = effect.def {
                        let integer = match inst.op {
                            LoadI | AddII | SubII | MulII | DivII | ModII | Add | Sub | Mul
                            | AddIIG | SubIIG | MulIIG | DivIIG | ModIIG | AddI | SubI | Shl
                            | Shr | BitAnd | BitOr | BitXor | ShlII | ShrII | AndII | OrII
                            | XorII | NotI | BitNot | ShlIImm | ShrIImm | AndIImm | OrIImm
                            | XorIImm | ForLoopI | ForLoopIInc | ForLoopILong | ForLoopIIncLong
                            | VecLoadI | ArrayLoadI | VecLen | ArrayLen | AddGlobalI | MulShr
                            | ShlDiv => true,
                            LoadK | LoadKWide => constant_index(inst).is_some_and(|index| {
                                matches!(
                                    analysis.function.constants.get(index),
                                    Some(Constant::Int(_))
                                )
                            }),
                            StructLoad => usize::try_from(inst.imm).is_ok_and(|schema| {
                                matches!(
                                    analysis.field_check(schema, inst.c),
                                    Some(FieldCheck::Int(_))
                                )
                            }),
                            Move => {
                                candidate.contains(inst.b)
                                    && wanted.contains(inst.b)
                                    && (!entry.contains(inst.b) || counters.contains(inst.b))
                            }
                            _ => false,
                        };
                        defs.push((def, integer));
                    }
                }
            }
            for (register, integer) in defs {
                if register < analysis.registers {
                    if !integer && candidate.contains(register) {
                        candidate.remove(register);
                        changed = true;
                    }
                }
            }
        }
        if !changed {
            break;
        }
    }
    let mut ints = Live::default();
    for register in 0..analysis.registers {
        if candidate.contains(register)
            && wanted.contains(register)
            && (!entry.contains(register) || counters.contains(register))
        {
            ints.insert(register);
        }
    }
    ints
}

#[derive(Clone, Debug)]
struct LiveEffect {
    uses: Option<Vec<usize>>,
    def: Option<usize>,
    flow: Flow,
}

fn function_liveness(
    analysis: &Analysis<'_>,
    instructions: &[Inst],
    by_ip: &HashMap<usize, usize>,
) -> (HashMap<usize, Live>, HashMap<usize, Live>) {
    let all = Live::all(analysis.registers);
    let effects = instructions
        .iter()
        .map(|inst| analysis.live_effect(inst))
        .collect::<Vec<_>>();
    let mut live_in = vec![Live::default(); instructions.len()];
    let mut live_out = vec![Live::default(); instructions.len()];
    let successor_live = |live_in: &[Live], ip: usize| match by_ip.get(&ip) {
        Some(index) => live_in[*index],
        None => all,
    };
    loop {
        let mut changed = false;
        for index in (0..instructions.len()).rev() {
            let inst = &instructions[index];
            let effect = &effects[index];
            let mut out = Live::default();
            match effect.flow {
                Flow::Next => out.union(&successor_live(&live_in, inst.next)),
                Flow::Jump(target) => out.union(&successor_live(&live_in, target)),
                Flow::Branch(target) => {
                    out.union(&successor_live(&live_in, target));
                    out.union(&successor_live(&live_in, inst.next));
                }
                Flow::Return => {}
            }
            let input = match &effect.uses {
                None => all,
                Some(uses) => {
                    let mut input = out;
                    if let Some(def) = effect.def {
                        input.remove(def);
                    }
                    for &register in uses {
                        input.insert(register);
                    }
                    input
                }
            };
            if live_out[index] != out || live_in[index] != input {
                live_out[index] = out;
                live_in[index] = input;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let by_ip_map = |values: Vec<Live>| {
        instructions
            .iter()
            .zip(values)
            .map(|(inst, live)| (inst.ip, live))
            .collect::<HashMap<_, _>>()
    };
    (by_ip_map(live_in), by_ip_map(live_out))
}

fn fixed_kind(inst: &Inst) -> u8 {
    ((inst.imm >> 16) & 0xff) as u8
}

fn fixed_mode(inst: &Inst) -> u8 {
    ((inst.imm >> 8) & 0xff) as u8
}

fn constant_index(inst: &Inst) -> Option<usize> {
    match inst.op {
        OpCode::LoadK => usize::try_from(inst.imm & 0xffff).ok(),
        OpCode::LoadKWide => usize::try_from(inst.imm).ok(),
        _ => None,
    }
}

struct Region {
    entry: usize,
    order: Vec<usize>,
    effects: HashMap<usize, Option<Effect>>,
    live_in: HashMap<usize, Live>,
    live_out: HashMap<usize, Live>,
    leaders: BTreeSet<usize>,
}

fn region(
    analysis: &Analysis<'_>,
    instructions: &[Inst],
    by_ip: &HashMap<usize, usize>,
    entry: usize,
) -> Option<Region> {
    let mut effects = HashMap::new();
    let mut stack = vec![entry];
    let mut leaders = BTreeSet::from([entry]);
    while let Some(ip) = stack.pop() {
        if effects.contains_key(&ip) {
            continue;
        }
        let Some(&index) = by_ip.get(&ip) else {
            effects.insert(ip, None);
            continue;
        };
        let inst = &instructions[index];
        let effect = analysis.effect(inst);
        if let Some(effect) = &effect {
            match effect.flow {
                Flow::Next => stack.push(inst.next),
                Flow::Jump(target) => {
                    leaders.insert(target);
                    leaders.insert(inst.next);
                    stack.push(target);
                }
                Flow::Branch(target) => {
                    leaders.insert(target);
                    leaders.insert(inst.next);
                    stack.push(target);
                    stack.push(inst.next);
                }
                Flow::Return => {
                    leaders.insert(inst.next);
                }
            }
        } else {
            leaders.insert(inst.next);
        }
        effects.insert(ip, effect);
    }
    let mut order = effects.keys().copied().collect::<Vec<_>>();
    order.sort_unstable();
    leaders.retain(|ip| effects.contains_key(ip));
    let (live_in, live_out) = function_liveness(analysis, instructions, by_ip);
    Some(Region {
        entry,
        order,
        effects,
        live_in,
        live_out,
        leaders,
    })
}

struct Helpers {
    shl_div: FuncRef,
    enumeration: FuncRef,
    element: FuncRef,
    view: FuncRef,
    set_global: FuncRef,
    call: FuncRef,
    push: FuncRef,
    collection: FuncRef,
    structure: FuncRef,
    store_reference: FuncRef,
    vec_push: FuncRef,
    fmod: FuncRef,
    entry_signature: SigRef,
}

struct Lowerer<'a, 'b> {
    builder: FunctionBuilder<'b>,
    analysis: &'a Analysis<'a>,
    region: &'a Region,
    instructions: &'a [Inst],
    by_ip: &'a HashMap<usize, usize>,
    blocks: HashMap<usize, Block>,
    variables: Vec<Variable>,
    caches: HashMap<(usize, CacheKind), CacheVars>,
    cache_kinds: Vec<CacheKind>,
    sites: Vec<FrameSiteKind>,
    helpers: Helpers,
    context: ClifValue,
    registers: ClifValue,
    data: ClifValue,
    pending_exits: Vec<(Block, usize, Live)>,
    current: Option<(usize, Live, Option<Block>)>,
    known: HashMap<ClifValue, Known>,
    origins: HashMap<usize, usize>,
    progress: Variable,
    uses_globals: bool,
    ints: Live,
}

// a resolved storage address stays valid while the register still holds the object it was resolved for
#[derive(Clone, Copy)]
struct CacheVars {
    owner: Variable,
    pointer: Variable,
    length: Variable,
}

const NO_OWNER: i64 = -1;

#[derive(Clone, Copy, Default)]
struct Known {
    int: Option<ClifValue>,
    float: Option<ClifValue>,
    boolean: Option<ClifValue>,
}

fn int_value(value: i64) -> u64 {
    Value::int_wrapping(value).raw()
}

impl<'a, 'b> Lowerer<'a, 'b> {
    fn trusted() -> MemFlagsData {
        MemFlagsData::trusted()
    }

    fn constant(&mut self, bits: u64) -> ClifValue {
        self.builder
            .ins()
            .iconst(types::I64, i64::from_ne_bytes(bits.to_ne_bytes()))
    }

    fn get(&mut self, register: usize) -> ClifValue {
        let value = self.builder.use_var(self.variables[register]);
        if self.ints.contains(register) {
            return self.box_int_in_range(value);
        }
        value
    }

    fn forget_origin(&mut self, register: usize) {
        self.origins
            .retain(|copy, origin| *copy != register && *origin != register);
    }

    fn set(&mut self, register: usize, value: ClifValue) {
        self.forget_origin(register);
        let value = if self.ints.contains(register) {
            self.unbox_int(value)
        } else {
            value
        };
        self.builder.def_var(self.variables[register], value);
    }

    fn set_moved(&mut self, destination: usize, source: usize) {
        self.forget_origin(destination);
        if destination != source {
            let origin = self.origins.get(&source).copied().unwrap_or(source);
            self.origins.insert(destination, origin);
        }
        let value = if self.ints.contains(destination) && self.ints.contains(source) {
            self.builder.use_var(self.variables[source])
        } else {
            let boxed = self.get(source);
            if self.ints.contains(destination) {
                self.unbox_int(boxed)
            } else {
                boxed
            }
        };
        self.builder.def_var(self.variables[destination], value);
        let kinds = self.cache_kinds.clone();
        for kind in kinds {
            let Some(target) = self.caches.get(&(destination, kind)).copied() else {
                continue;
            };
            if let Some(origin) = self.caches.get(&(source, kind)).copied() {
                for (to, from) in [
                    (target.owner, origin.owner),
                    (target.pointer, origin.pointer),
                    (target.length, origin.length),
                ] {
                    let copied = self.builder.use_var(from);
                    self.builder.def_var(to, copied);
                }
            }
        }
    }

    fn exit(&mut self) -> Block {
        let Some((ip, live, existing)) = self.current else {
            unreachable!("exits are only taken while an instruction is lowered");
        };
        if let Some(block) = existing {
            return block;
        }
        let block = self.builder.create_block();
        self.builder.set_cold_block(block);
        self.pending_exits.push((block, ip, live));
        self.current = Some((ip, live, Some(block)));
        block
    }

    fn finish_exits(&mut self) {
        while let Some((block, ip, live)) = self.pending_exits.pop() {
            self.builder.seal_block(block);
            self.builder.switch_to_block(block);
            self.flush(&live, None);
            self.leave(ip);
        }
    }

    fn flush(&mut self, live: &Live, skip: Option<usize>) {
        for register in live.registers(self.analysis.registers).collect::<Vec<_>>() {
            if Some(register) == skip {
                continue;
            }
            let value = self.get(register);
            let offset = i32::try_from(register * 8).unwrap_or(i32::MAX);
            self.builder
                .ins()
                .store(Self::trusted(), value, self.registers, offset);
        }
    }

    fn commit_progress(&mut self) {
        let progress = self.builder.use_var(self.progress);
        let previous = self.builder.ins().load(
            types::I64,
            Self::trusted(),
            self.context,
            FrameJitContext::PROGRESS,
        );
        let total = self.builder.ins().iadd(previous, progress);
        self.builder.ins().store(
            Self::trusted(),
            total,
            self.context,
            FrameJitContext::PROGRESS,
        );
    }

    fn count_backedge(&mut self, inst: &Inst, target: usize) {
        if target <= inst.ip {
            let progress = self.builder.use_var(self.progress);
            let next = self.builder.ins().iadd_imm_s(progress, 1);
            self.builder.def_var(self.progress, next);
        }
    }

    // every global read of machine code goes through here, so a read check belongs in this one place
    fn load_global(&mut self, index: usize, exit: Block) -> ClifValue {
        let values = self.builder.ins().load(
            types::I64,
            Self::trusted(),
            self.data,
            FrameData::VIEW_VALUES,
        );
        let length =
            self.builder
                .ins()
                .load(types::I64, Self::trusted(), self.data, FrameData::VIEW_LEN);
        let inside = self.builder.ins().icmp_imm_s(
            IntCC::UnsignedGreaterThan,
            length,
            i64::try_from(index).unwrap_or(i64::MAX),
        );
        self.guard(inside, exit);
        let offset = i32::try_from(index * 8).unwrap_or(i32::MAX);
        let value = self
            .builder
            .ins()
            .load(types::I64, Self::trusted(), values, offset);
        let initialized = self.builder.ins().icmp_imm_s(
            IntCC::NotEqual,
            value,
            i64::from_ne_bytes(Value::uninit().raw().to_ne_bytes()),
        );
        self.guard(initialized, exit);
        value
    }

    fn store_global(&mut self, index: usize, value: ClifValue, exit: Block) {
        let index = self
            .builder
            .ins()
            .iconst(types::I64, i64::try_from(index).unwrap_or(i64::MAX));
        let call = self.builder.ins().call(
            self.helpers.set_global,
            &[self.context, self.data, index, value],
        );
        let stored = self.builder.inst_results(call)[0];
        self.guard(stored, exit);
    }

    fn unbox_entry(&mut self, live: &Live, loaded: &Live, entry_ip: usize) {
        let mut exit = None;
        for register in self
            .ints
            .registers(self.analysis.registers)
            .collect::<Vec<_>>()
        {
            let value = self.builder.use_var(self.variables[register]);
            if live.contains(register) && loaded.contains(register) {
                let block = *exit.get_or_insert_with(|| {
                    let block = self.builder.create_block();
                    self.builder.set_cold_block(block);
                    self.pending_exits.push((block, entry_ip, Live::default()));
                    block
                });
                let is_int = self.tag_is(value, int_value(0) & TAG_BITS);
                self.guard(is_int, block);
            }
            let shifted = self.builder.ins().ishl_imm_s(value, 16);
            let raw = self.builder.ins().sshr_imm_s(shifted, 16);
            self.builder.def_var(self.variables[register], raw);
        }
    }

    // the view holds every name of this function once its generation matches the one recorded in the data block
    fn ensure_view(&mut self) {
        if !self.uses_globals {
            return;
        }
        let current = self.builder.ins().load(
            types::I64,
            Self::trusted(),
            self.context,
            FrameJitContext::VIEW_GENERATION,
        );
        let prepared = self.builder.ins().load(
            types::I64,
            Self::trusted(),
            self.data,
            FrameData::VIEW_GENERATION,
        );
        let ready = self.builder.ins().icmp(IntCC::Equal, current, prepared);
        let refresh = self.builder.create_block();
        let done = self.builder.create_block();
        self.builder.set_cold_block(refresh);
        self.builder.ins().brif(ready, done, &[], refresh, &[]);
        self.builder.seal_block(refresh);
        self.builder.switch_to_block(refresh);
        self.builder
            .ins()
            .call(self.helpers.view, &[self.context, self.data]);
        self.builder.ins().jump(done, &[]);
        self.builder.seal_block(done);
        self.builder.switch_to_block(done);
    }

    fn leave(&mut self, ip: usize) {
        self.commit_progress();
        let status = self
            .builder
            .ins()
            .iconst(types::I64, i64::try_from(FRAME_STATUS_EXIT).unwrap_or(1));
        self.builder.ins().store(
            Self::trusted(),
            status,
            self.context,
            FrameJitContext::STATUS,
        );
        let resume = self
            .builder
            .ins()
            .iconst(types::I64, i64::try_from(ip).unwrap_or(i64::MAX));
        self.builder.ins().store(
            Self::trusted(),
            resume,
            self.context,
            FrameJitContext::RESUME_IP,
        );
        let zero = self.builder.ins().iconst(types::I64, 0);
        self.builder.ins().return_(&[zero]);
    }

    fn guard(&mut self, condition: ClifValue, exit: Block) {
        let continuation = self.builder.create_block();
        self.builder
            .ins()
            .brif(condition, continuation, &[], exit, &[]);
        self.builder.seal_block(continuation);
        self.builder.switch_to_block(continuation);
    }

    fn unbox_int(&mut self, value: ClifValue) -> ClifValue {
        if let Some(raw) = self.known.get(&value).and_then(|known| known.int) {
            return raw;
        }
        let shifted = self.builder.ins().ishl_imm_s(value, 16);
        self.builder.ins().sshr_imm_s(shifted, 16)
    }

    fn box_int(&mut self, value: ClifValue) -> ClifValue {
        let payload = self.builder.ins().band_imm_s(value, PAYLOAD as i64);
        let tag = self.constant(int_value(0));
        self.builder.ins().bor(payload, tag)
    }

    fn box_int_checked(&mut self, value: ClifValue, exit: Block) -> ClifValue {
        let shifted = self.builder.ins().ishl_imm_s(value, 16);
        let narrowed = self.builder.ins().sshr_imm_s(shifted, 16);
        let fits = self.builder.ins().icmp(IntCC::Equal, narrowed, value);
        self.guard(fits, exit);
        self.box_int_in_range(value)
    }

    fn box_int_in_range(&mut self, value: ClifValue) -> ClifValue {
        let boxed = self.box_int(value);
        self.known.insert(
            boxed,
            Known {
                int: Some(value),
                ..Known::default()
            },
        );
        boxed
    }

    fn tag_is(&mut self, value: ClifValue, tag: u64) -> ClifValue {
        let masked = self.builder.ins().band_imm_s(value, TAG_BITS as i64);
        self.builder
            .ins()
            .icmp_imm_s(IntCC::Equal, masked, i64::from_ne_bytes(tag.to_ne_bytes()))
    }

    fn is_int(&mut self, value: ClifValue) -> ClifValue {
        if self
            .known
            .get(&value)
            .is_some_and(|known| known.int.is_some())
        {
            return self.builder.ins().iconst(types::I8, 1);
        }
        self.tag_is(value, int_value(0) & TAG_BITS)
    }

    fn is_bool(&mut self, value: ClifValue) -> ClifValue {
        self.tag_is(value, Value::bool(false).raw() & TAG_BITS)
    }

    fn is_null(&mut self, value: ClifValue) -> ClifValue {
        let mask = TAG_BITS | CHAR_FLAG;
        let masked = self
            .builder
            .ins()
            .band_imm_s(value, i64::from_ne_bytes(mask.to_ne_bytes()));
        self.builder.ins().icmp_imm_s(
            IntCC::Equal,
            masked,
            i64::from_ne_bytes((Value::null().raw() & mask).to_ne_bytes()),
        )
    }

    fn is_float(&mut self, value: ClifValue) -> ClifValue {
        if self
            .known
            .get(&value)
            .is_some_and(|known| known.float.is_some())
        {
            return self.builder.ins().iconst(types::I8, 1);
        }
        let masked = self.builder.ins().band_imm_s(value, QNAN as i64);
        let not_nan_boxed = self
            .builder
            .ins()
            .icmp_imm_s(IntCC::NotEqual, masked, QNAN as i64);
        let canonical = self.tag_is(value, (QNAN | 0x0004_0000_0000_0000) & TAG_BITS);
        self.builder.ins().bor(not_nan_boxed, canonical)
    }

    fn unbox_float(&mut self, value: ClifValue) -> ClifValue {
        if let Some(raw) = self.known.get(&value).and_then(|known| known.float) {
            return raw;
        }
        self.builder
            .ins()
            .bitcast(types::F64, MemFlagsData::new(), value)
    }

    fn box_float(&mut self, value: ClifValue) -> ClifValue {
        let bits = self
            .builder
            .ins()
            .bitcast(types::I64, MemFlagsData::new(), value);
        let unordered = self.builder.ins().fcmp(FloatCC::Unordered, value, value);
        let canonical = self.constant(Value::float(f64::NAN).raw());
        let boxed = self.builder.ins().select(unordered, canonical, bits);
        self.known.insert(
            boxed,
            Known {
                float: Some(value),
                ..Known::default()
            },
        );
        boxed
    }

    fn box_bool(&mut self, value: ClifValue) -> ClifValue {
        let wide = self.builder.ins().uextend(types::I64, value);
        let tag = self.constant(Value::bool(false).raw());
        let boxed = self.builder.ins().bor(wide, tag);
        self.known.insert(
            boxed,
            Known {
                boolean: Some(value),
                ..Known::default()
            },
        );
        boxed
    }

    fn falsy(&mut self, value: ClifValue) -> ClifValue {
        if let Some(raw) = self.known.get(&value).and_then(|known| known.boolean) {
            return self.builder.ins().icmp_imm_s(IntCC::Equal, raw, 0);
        }
        let null = self.is_null(value);
        let boolean = self.is_bool(value);
        let low = self.builder.ins().band_imm_s(value, 1);
        let zero = self.builder.ins().icmp_imm_s(IntCC::Equal, low, 0);
        let false_bool = self.builder.ins().band(boolean, zero);
        self.builder.ins().bor(null, false_bool)
    }

    fn both_ints(&mut self, left: ClifValue, right: ClifValue) -> ClifValue {
        let left = self.is_int(left);
        let right = self.is_int(right);
        self.builder.ins().band(left, right)
    }

    fn resolve(&mut self, register: usize, kind: CacheKind, exit: Block) -> (ClifValue, ClifValue) {
        let cache = self.caches[&(register, kind)];
        let (pointer, length) = (cache.pointer, cache.length);
        let value = self.get(register);
        let owner = self.builder.use_var(cache.owner);
        let hit = self.builder.ins().icmp(IntCC::Equal, owner, value);
        let miss = self.builder.create_block();
        let have = self.builder.create_block();
        self.builder.set_cold_block(miss);
        self.builder.ins().brif(hit, have, &[], miss, &[]);
        self.builder.seal_block(miss);
        self.builder.switch_to_block(miss);
        let status = match kind {
            CacheKind::Collection(kind) => {
                let kind = self
                    .builder
                    .ins()
                    .iconst(types::I64, i64::try_from(kind).unwrap_or(0));
                let call = self
                    .builder
                    .ins()
                    .call(self.helpers.collection, &[self.context, value, kind]);
                self.builder.inst_results(call)[0]
            }
            CacheKind::Struct(schema) => {
                let schema = self
                    .builder
                    .ins()
                    .iconst(types::I64, i64::try_from(schema).unwrap_or(i64::MAX));
                let call = self.builder.ins().call(
                    self.helpers.structure,
                    &[self.context, self.data, value, schema],
                );
                self.builder.inst_results(call)[0]
            }
        };
        let filled = self.builder.create_block();
        self.builder.ins().brif(status, filled, &[], exit, &[]);
        self.builder.seal_block(filled);
        self.builder.switch_to_block(filled);
        let new_pointer = self.builder.ins().load(
            types::I64,
            Self::trusted(),
            self.context,
            FrameJitContext::SCRATCH0,
        );
        let new_length = self.builder.ins().load(
            types::I64,
            Self::trusted(),
            self.context,
            FrameJitContext::SCRATCH1,
        );
        self.builder.def_var(pointer, new_pointer);
        self.builder.def_var(length, new_length);
        self.builder.def_var(cache.owner, value);
        if let Some(origin) = self.origins.get(&register).copied()
            && let Some(origin_cache) = self.caches.get(&(origin, kind)).copied()
        {
            self.builder.def_var(origin_cache.pointer, new_pointer);
            self.builder.def_var(origin_cache.length, new_length);
            self.builder.def_var(origin_cache.owner, value);
        }
        self.builder.ins().jump(have, &[]);
        self.builder.seal_block(have);
        self.builder.switch_to_block(have);
        let pointer = self.builder.use_var(pointer);
        let length = self.builder.use_var(length);
        (pointer, length)
    }

    fn index_in_bounds(&mut self, register: usize, length: ClifValue, exit: Block) -> ClifValue {
        let value = self.get(register);
        let is_int = self.is_int(value);
        self.guard(is_int, exit);
        let index = self.unbox_int(value);
        let below = self
            .builder
            .ins()
            .icmp(IntCC::UnsignedLessThan, index, length);
        self.guard(below, exit);
        index
    }

    fn site(&mut self, kind: FrameSiteKind) -> (usize, i32) {
        let index = self.sites.len();
        self.sites.push(kind);
        (index, FrameData::site_offset(index).unwrap_or(i32::MAX))
    }

    fn lower_block(&mut self, leader: usize) -> Result<(), FrameRefusal> {
        let block = self.blocks[&leader];
        self.builder.switch_to_block(block);
        self.known.clear();
        self.origins.clear();
        self.lower_block_body(leader)?;
        self.finish_exits();
        Ok(())
    }

    fn lower_block_body(&mut self, leader: usize) -> Result<(), FrameRefusal> {
        let mut ip = leader;
        loop {
            let effect = self.region.effects.get(&ip).cloned().flatten();
            let Some(&index) = self.by_ip.get(&ip) else {
                let live = Live::all(self.analysis.registers);
                self.flush(&live, None);
                self.leave(ip);
                return Ok(());
            };
            let inst = self.instructions[index];
            let Some(effect) = effect else {
                let live = self.region.live_in[&ip];
                self.flush(&live, None);
                self.leave(ip);
                return Ok(());
            };
            if self.lower_instruction(&inst, &effect)? {
                return Ok(());
            }
            ip = inst.next;
            if self.region.leaders.contains(&ip) {
                let target = self.blocks[&ip];
                self.builder.ins().jump(target, &[]);
                return Ok(());
            }
        }
    }

    fn int_binary(&mut self, inst: &Inst, exit: Block, generic: bool) {
        let left = self.get(inst.b);
        let right = self.get(inst.c);
        if generic {
            let ints = self.both_ints(left, right);
            self.guard(ints, exit);
        }
        let left = self.unbox_int(left);
        let right = self.unbox_int(right);
        let raw = match inst.op {
            OpCode::Add | OpCode::AddII | OpCode::AddIIG => self.builder.ins().iadd(left, right),
            OpCode::Sub | OpCode::SubII | OpCode::SubIIG => self.builder.ins().isub(left, right),
            OpCode::Mul | OpCode::MulII | OpCode::MulIIG => {
                let low = self.builder.ins().imul(left, right);
                let high = self.builder.ins().smulhi(left, right);
                let sign = self.builder.ins().sshr_imm_s(low, 63);
                let fits = self.builder.ins().icmp(IntCC::Equal, high, sign);
                self.guard(fits, exit);
                low
            }
            _ => {
                let nonzero = self.builder.ins().icmp_imm_s(IntCC::NotEqual, right, 0);
                self.guard(nonzero, exit);
                if matches!(inst.op, OpCode::DivII | OpCode::DivIIG) {
                    self.builder.ins().sdiv(left, right)
                } else {
                    self.builder.ins().srem(left, right)
                }
            }
        };
        let boxed = self.box_int_checked(raw, exit);
        self.set(inst.a, boxed);
    }

    fn compare_ints(&mut self, inst: &Inst, condition: IntCC, exit: Option<Block>) {
        let left = self.get(inst.b);
        let right = self.get(inst.c);
        if let Some(exit) = exit {
            let ints = self.both_ints(left, right);
            self.guard(ints, exit);
        }
        let left = self.unbox_int(left);
        let right = self.unbox_int(right);
        let result = self.builder.ins().icmp(condition, left, right);
        let boxed = self.box_bool(result);
        self.set(inst.a, boxed);
    }

    fn compare_immediate(&mut self, inst: &Inst, condition: IntCC) {
        let left = self.get(inst.b);
        let left = self.unbox_int(left);
        let result =
            self.builder
                .ins()
                .icmp_imm_s(condition, left, i64::try_from(inst.c).unwrap_or(0));
        let boxed = self.box_bool(result);
        self.set(inst.a, boxed);
    }

    fn compare_floats(&mut self, inst: &Inst, condition: FloatCC) {
        let left = self.get(inst.b);
        let right = self.get(inst.c);
        let left = self.unbox_float(left);
        let right = self.unbox_float(right);
        let result = self.builder.ins().fcmp(condition, left, right);
        let boxed = self.box_bool(result);
        self.set(inst.a, boxed);
    }

    // the only copy of the interpreter's shift rule: count masked to 63, a left shift that leaves the 48-bit range traps
    fn shift(&mut self, left: ClifValue, count: ClifValue, op: OpCode) -> ClifValue {
        let count = self.builder.ins().band_imm_s(count, 63);
        let left = self.unbox_int(left);
        if matches!(op, OpCode::Shl | OpCode::ShlII | OpCode::ShlIImm) {
            let exit = self.exit();
            let raw = self.builder.ins().ishl(left, count);
            let back = self.builder.ins().sshr(raw, count);
            let exact = self.builder.ins().icmp(IntCC::Equal, back, left);
            self.guard(exact, exit);
            self.box_int_checked(raw, exit)
        } else {
            let raw = self.builder.ins().sshr(left, count);
            self.box_int_in_range(raw)
        }
    }

    fn bitwise(&mut self, inst: &Inst, exit: Option<Block>) {
        let left = self.get(inst.b);
        let right = self.get(inst.c);
        if let Some(exit) = exit {
            let ints = self.both_ints(left, right);
            self.guard(ints, exit);
        }
        let result = match inst.op {
            OpCode::Shl | OpCode::ShlII | OpCode::Shr | OpCode::ShrII => {
                let count = self.unbox_int(right);
                self.shift(left, count, inst.op)
            }
            _ => {
                let left = self.unbox_int(left);
                let right = self.unbox_int(right);
                let raw = match inst.op {
                    OpCode::BitAnd | OpCode::AndII => self.builder.ins().band(left, right),
                    OpCode::BitOr | OpCode::OrII => self.builder.ins().bor(left, right),
                    _ => self.builder.ins().bxor(left, right),
                };
                self.box_int_in_range(raw)
            }
        };
        self.set(inst.a, result);
    }

    fn lower_instruction(&mut self, inst: &Inst, effect: &Effect) -> Result<bool, FrameRefusal> {
        use OpCode::*;
        let live = self.region.live_in[&inst.ip];
        self.current = Some((inst.ip, live, None));
        match inst.op {
            Move => self.set_moved(inst.a, inst.b),
            LoadI => {
                let value = self.constant(int_value(inst.imm));
                self.set(inst.a, value);
            }
            LoadNull => {
                let value = self.constant(Value::null().raw());
                self.set(inst.a, value);
            }
            LoadBool => {
                let value = self.constant(Value::bool(inst.b != 0).raw());
                self.set(inst.a, value);
            }
            LoadUnit => {
                let value = self.constant(Value::unit().raw());
                self.set(inst.a, value);
            }
            LoadNone => {
                let value = self.constant(Value::none().raw());
                self.set(inst.a, value);
            }
            LoadK | LoadKWide => {
                let index = constant_index(inst).ok_or(FrameRefusal::Decode)?;
                let value = match self.analysis.function.constants.get(index) {
                    Some(Constant::Null) => self.constant(Value::null().raw()),
                    Some(Constant::Bool(value)) => self.constant(Value::bool(*value).raw()),
                    Some(Constant::Int(value)) => {
                        let value = Value::int_checked(*value)
                            .map_err(|_| FrameRefusal::Decode)?
                            .raw();
                        self.constant(value)
                    }
                    Some(Constant::Float(bits)) => {
                        self.constant(Value::float(f64::from_bits(*bits)).raw())
                    }
                    Some(Constant::String(_)) => {
                        let constants = self.builder.ins().load(
                            types::I64,
                            Self::trusted(),
                            self.data,
                            FrameData::CONSTANTS,
                        );
                        let offset = i32::try_from(index * 8).map_err(|_| FrameRefusal::Decode)?;
                        self.builder
                            .ins()
                            .load(types::I64, Self::trusted(), constants, offset)
                    }
                    _ => return Err(FrameRefusal::Decode),
                };
                self.set(inst.a, value);
            }
            GetGlobalIdx | GetGlobalIdxWide => {
                let index = self
                    .analysis
                    .global_index(inst)
                    .ok_or(FrameRefusal::Decode)?;
                let exit = self.exit();
                let value = self.load_global(index, exit);
                self.set(inst.a, value);
            }
            SetGlobalIdx | SetGlobalIdxWide => {
                let index = self
                    .analysis
                    .global_index(inst)
                    .ok_or(FrameRefusal::Decode)?;
                let exit = self.exit();
                let value = self.get(inst.a);
                self.store_global(index, value, exit);
            }
            AddGlobalI => {
                let exit = self.exit();
                let current = self.load_global(inst.c, exit);
                let right = self.get(inst.b);
                let ints = self.both_ints(current, right);
                self.guard(ints, exit);
                let left = self.unbox_int(current);
                let right = self.unbox_int(right);
                let sum = self.builder.ins().iadd(left, right);
                let boxed = self.box_int_checked(sum, exit);
                self.store_global(inst.c, boxed, exit);
                self.set(inst.a, boxed);
            }
            AddII | SubII | MulII | DivII | ModII => {
                let exit = self.exit();
                self.int_binary(inst, exit, false);
            }
            Add | Sub | Mul | AddIIG | SubIIG | MulIIG | DivIIG | ModIIG => {
                let exit = self.exit();
                self.int_binary(inst, exit, true);
            }
            AddI | SubI => {
                let exit = self.exit();
                let left = self.get(inst.b);
                let left = self.unbox_int(left);
                let immediate = i64::try_from(inst.c).unwrap_or(0);
                let raw = if inst.op == AddI {
                    self.builder.ins().iadd_imm_s(left, immediate)
                } else {
                    self.builder.ins().iadd_imm_s(left, -immediate)
                };
                let boxed = self.box_int_checked(raw, exit);
                self.set(inst.a, boxed);
            }
            AddFF | SubFF | MulFF | DivFF | ModFF => {
                let left = self.get(inst.b);
                let right = self.get(inst.c);
                let left = self.unbox_float(left);
                let right = self.unbox_float(right);
                let raw = match inst.op {
                    AddFF => self.builder.ins().fadd(left, right),
                    SubFF => self.builder.ins().fsub(left, right),
                    MulFF => self.builder.ins().fmul(left, right),
                    DivFF => self.builder.ins().fdiv(left, right),
                    _ => {
                        let call = self.builder.ins().call(self.helpers.fmod, &[left, right]);
                        self.builder.inst_results(call)[0]
                    }
                };
                let boxed = self.box_float(raw);
                self.set(inst.a, boxed);
            }
            Neg => {
                let exit = self.exit();
                let value = self.get(inst.b);
                let int_case = self.builder.create_block();
                let float_check = self.builder.create_block();
                let float_case = self.builder.create_block();
                let join = self.builder.create_block();
                let result = self.builder.append_block_param(join, types::I64);
                let is_int = self.is_int(value);
                self.builder
                    .ins()
                    .brif(is_int, int_case, &[], float_check, &[]);
                self.builder.seal_block(int_case);
                self.builder.seal_block(float_check);
                self.builder.switch_to_block(int_case);
                let number = self.unbox_int(value);
                let negated = self.builder.ins().ineg(number);
                let boxed = self.box_int_checked(negated, exit);
                self.builder.ins().jump(join, &[BlockArg::from(boxed)]);
                self.builder.switch_to_block(float_check);
                let is_float = self.is_float(value);
                self.builder
                    .ins()
                    .brif(is_float, float_case, &[], exit, &[]);
                self.builder.seal_block(float_case);
                self.builder.switch_to_block(float_case);
                let number = self.unbox_float(value);
                let negated = self.builder.ins().fneg(number);
                let boxed = self.box_float(negated);
                self.builder.ins().jump(join, &[BlockArg::from(boxed)]);
                self.builder.seal_block(join);
                self.builder.switch_to_block(join);
                self.set(inst.a, result);
            }
            Not => {
                let value = self.get(inst.b);
                let falsy = self.falsy(value);
                let int_zero = self.constant(int_value(0));
                let zero = self.builder.ins().icmp(IntCC::Equal, value, int_zero);
                let result = self.builder.ins().bor(falsy, zero);
                let boxed = self.box_bool(result);
                self.set(inst.a, boxed);
            }
            LtII => self.compare_ints(inst, IntCC::SignedLessThan, None),
            LeII => self.compare_ints(inst, IntCC::SignedLessThanOrEqual, None),
            GtII => self.compare_ints(inst, IntCC::SignedGreaterThan, None),
            GeII => self.compare_ints(inst, IntCC::SignedGreaterThanOrEqual, None),
            EqII => self.compare_ints(inst, IntCC::Equal, None),
            NeII => self.compare_ints(inst, IntCC::NotEqual, None),
            Lt | LtIIG => {
                let exit = self.exit();
                self.compare_ints(inst, IntCC::SignedLessThan, Some(exit));
            }
            Le | LeIIG => {
                let exit = self.exit();
                self.compare_ints(inst, IntCC::SignedLessThanOrEqual, Some(exit));
            }
            Gt | GtIIG => {
                let exit = self.exit();
                self.compare_ints(inst, IntCC::SignedGreaterThan, Some(exit));
            }
            Ge | GeIIG => {
                let exit = self.exit();
                self.compare_ints(inst, IntCC::SignedGreaterThanOrEqual, Some(exit));
            }
            Eq | Ne => {
                let exit = self.exit();
                let left = self.get(inst.b);
                let right = self.get(inst.c);
                let mut plain = None;
                for value in [left, right] {
                    let float = self.is_float(value);
                    let pointer = self.tag_is(value, Value::ptr(0).raw() & TAG_BITS);
                    let either = self.builder.ins().bor(float, pointer);
                    let simple = self.builder.ins().icmp_imm_s(IntCC::Equal, either, 0);
                    plain = Some(match plain {
                        Some(previous) => self.builder.ins().band(previous, simple),
                        None => simple,
                    });
                }
                if let Some(plain) = plain {
                    self.guard(plain, exit);
                }
                let condition = if inst.op == Eq {
                    IntCC::Equal
                } else {
                    IntCC::NotEqual
                };
                let result = self.builder.ins().icmp(condition, left, right);
                let boxed = self.box_bool(result);
                self.set(inst.a, boxed);
            }
            EqIIG => {
                let exit = self.exit();
                self.compare_ints(inst, IntCC::Equal, Some(exit));
            }
            NeIIG => {
                let exit = self.exit();
                self.compare_ints(inst, IntCC::NotEqual, Some(exit));
            }
            LtFF => self.compare_floats(inst, FloatCC::LessThan),
            LeFF => self.compare_floats(inst, FloatCC::LessThanOrEqual),
            GtFF => self.compare_floats(inst, FloatCC::GreaterThan),
            GeFF => self.compare_floats(inst, FloatCC::GreaterThanOrEqual),
            EqFF => self.compare_floats(inst, FloatCC::Equal),
            NeFF => self.compare_floats(inst, FloatCC::NotEqual),
            LtImm | LtIImm => self.compare_immediate(inst, IntCC::SignedLessThan),
            LeImm | LeIImm => self.compare_immediate(inst, IntCC::SignedLessThanOrEqual),
            GtImm | GtIImm => self.compare_immediate(inst, IntCC::SignedGreaterThan),
            GeImm | GeIImm => self.compare_immediate(inst, IntCC::SignedGreaterThanOrEqual),
            ShlII | ShrII | AndII | OrII | XorII => self.bitwise(inst, None),
            Shl | Shr | BitAnd | BitOr | BitXor => {
                let exit = self.exit();
                self.bitwise(inst, Some(exit));
            }
            NotI | BitNot => {
                let value = self.get(inst.b);
                if inst.op == BitNot {
                    let exit = self.exit();
                    let is_int = self.is_int(value);
                    self.guard(is_int, exit);
                }
                let number = self.unbox_int(value);
                let inverted = self.builder.ins().bnot(number);
                let boxed = self.box_int_in_range(inverted);
                self.set(inst.a, boxed);
            }
            ShlIImm | ShrIImm => {
                let value = self.get(inst.b);
                let count = self
                    .builder
                    .ins()
                    .iconst(types::I64, i64::try_from(inst.c).unwrap_or(0));
                let result = self.shift(value, count, inst.op);
                self.set(inst.a, result);
            }
            AndIImm | OrIImm | XorIImm => {
                let value = self.get(inst.b);
                let number = self.unbox_int(value);
                let immediate = i64::try_from(inst.c).unwrap_or(0);
                let raw = match inst.op {
                    AndIImm => self.builder.ins().band_imm_s(number, immediate),
                    OrIImm => self.builder.ins().bor_imm_s(number, immediate),
                    _ => self.builder.ins().bxor_imm_s(number, immediate),
                };
                let boxed = self.box_int_in_range(raw);
                self.set(inst.a, boxed);
            }
            Jump | JumpLong => {
                let Flow::Jump(target) = effect.flow else {
                    return Err(FrameRefusal::Decode);
                };
                self.count_backedge(inst, target);
                let target = self.block_for(target);
                self.builder.ins().jump(target, &[]);
                return Ok(true);
            }
            JumpIf | JumpIfNot | JumpIfLong | JumpIfNotLong => {
                let Flow::Branch(target) = effect.flow else {
                    return Err(FrameRefusal::Decode);
                };
                let value = self.get(inst.a);
                let falsy = self.falsy(value);
                self.count_backedge(inst, target);
                let taken = self.block_for(target);
                let fallthrough = self.block_for(inst.next);
                if matches!(inst.op, JumpIf | JumpIfLong) {
                    self.builder.ins().brif(falsy, fallthrough, &[], taken, &[]);
                } else {
                    self.builder.ins().brif(falsy, taken, &[], fallthrough, &[]);
                }
                return Ok(true);
            }
            ForLoopI | ForLoopIInc | ForLoopILong | ForLoopIIncLong => {
                let Flow::Branch(target) = effect.flow else {
                    return Err(FrameRefusal::Decode);
                };
                let exit = self.exit();
                let iterator = self.get(inst.a);
                let end = self.get(inst.a + 1);
                let step = self.get(inst.a + 2);
                let iterator = self.unbox_int(iterator);
                let end = self.unbox_int(end);
                let step = self.unbox_int(step);
                let next = self.builder.ins().iadd(iterator, step);
                let boxed = self.box_int_checked(next, exit);
                self.set(inst.a, boxed);
                let inclusive = matches!(inst.op, ForLoopIInc | ForLoopIIncLong);
                let (up, down) = if inclusive {
                    (
                        IntCC::SignedLessThanOrEqual,
                        IntCC::SignedGreaterThanOrEqual,
                    )
                } else {
                    (IntCC::SignedLessThan, IntCC::SignedGreaterThan)
                };
                let ascending = self
                    .builder
                    .ins()
                    .icmp_imm_s(IntCC::SignedGreaterThan, step, 0);
                let up = self.builder.ins().icmp(up, next, end);
                let down = self.builder.ins().icmp(down, next, end);
                let continues = self.builder.ins().select(ascending, up, down);
                self.count_backedge(inst, target);
                let taken = self.block_for(target);
                let fallthrough = self.block_for(inst.next);
                self.builder
                    .ins()
                    .brif(continues, taken, &[], fallthrough, &[]);
                return Ok(true);
            }
            WhileLoopLt => {
                let Flow::Branch(target) = effect.flow else {
                    return Err(FrameRefusal::Decode);
                };
                let iterator = self.get(inst.a);
                let limit = self.get(inst.a + 1);
                let iterator = self.unbox_int(iterator);
                let limit = self.unbox_int(limit);
                let continues = self
                    .builder
                    .ins()
                    .icmp(IntCC::SignedLessThan, iterator, limit);
                self.count_backedge(inst, target);
                let taken = self.block_for(target);
                let fallthrough = self.block_for(inst.next);
                self.builder
                    .ins()
                    .brif(continues, taken, &[], fallthrough, &[]);
                return Ok(true);
            }
            Return => {
                self.commit_progress();
                let value = self.get(inst.a);
                self.builder.ins().return_(&[value]);
                return Ok(true);
            }
            Return0 => {
                self.commit_progress();
                let value = self.constant(Value::unit().raw());
                self.builder.ins().return_(&[value]);
                return Ok(true);
            }
            CallGlobal | Call | CallCached => {
                self.lower_call(inst)?;
            }
            ArrayLoadI | ArrayLoadF | ArrayLoadB | ArrayLoadP | VecLoadI | VecLoadF | VecLoadB
            | VecLoadP => {
                let exit = self.exit();
                let kind = collection_kind(inst.op);
                let (pointer, length) =
                    self.resolve(inst.b, CacheKind::Collection(kind as u64), exit);
                let index = self.index_in_bounds(inst.c, length, exit);
                let value = match kind {
                    FrameCollectionKind::ArrayBools | FrameCollectionKind::VecBools => {
                        let address = self.builder.ins().iadd(pointer, index);
                        let byte =
                            self.builder
                                .ins()
                                .uload8(types::I64, Self::trusted(), address, 0);
                        let set = self.builder.ins().icmp_imm_s(IntCC::NotEqual, byte, 0);
                        self.box_bool(set)
                    }
                    _ => {
                        let offset = self.builder.ins().ishl_imm_s(index, 3);
                        let address = self.builder.ins().iadd(pointer, offset);
                        let raw = self
                            .builder
                            .ins()
                            .load(types::I64, Self::trusted(), address, 0);
                        match kind {
                            FrameCollectionKind::ArrayInts | FrameCollectionKind::VecInts => {
                                self.box_int_in_range(raw)
                            }
                            FrameCollectionKind::ArrayFloats | FrameCollectionKind::VecFloats => {
                                let float = self.unbox_float(raw);
                                self.box_float(float)
                            }
                            _ => raw,
                        }
                    }
                };
                self.set(inst.a, value);
            }
            ArrayStoreI | ArrayStoreF | ArrayStoreB | ArrayStoreP | VecStoreI | VecStoreF
            | VecStoreB | VecStoreP => {
                let exit = self.exit();
                let kind = collection_kind(inst.op);
                let (pointer, length) =
                    self.resolve(inst.a, CacheKind::Collection(kind as u64), exit);
                let index = self.index_in_bounds(inst.b, length, exit);
                let value = self.get(inst.c);
                match kind {
                    FrameCollectionKind::ArrayBools | FrameCollectionKind::VecBools => {
                        let is_bool = self.is_bool(value);
                        self.guard(is_bool, exit);
                        let bit = self.builder.ins().band_imm_s(value, 1);
                        let address = self.builder.ins().iadd(pointer, index);
                        self.builder.ins().istore8(Self::trusted(), bit, address, 0);
                    }
                    _ => {
                        let offset = self.builder.ins().ishl_imm_s(index, 3);
                        let address = self.builder.ins().iadd(pointer, offset);
                        match kind {
                            FrameCollectionKind::ArrayInts | FrameCollectionKind::VecInts => {
                                let is_int = self.is_int(value);
                                self.guard(is_int, exit);
                                let number = self.unbox_int(value);
                                self.builder
                                    .ins()
                                    .store(Self::trusted(), number, address, 0);
                            }
                            FrameCollectionKind::ArrayFloats | FrameCollectionKind::VecFloats => {
                                let is_float = self.is_float(value);
                                self.guard(is_float, exit);
                                self.builder.ins().store(Self::trusted(), value, address, 0);
                            }
                            _ => {
                                let previous = self.builder.ins().load(
                                    types::I64,
                                    Self::trusted(),
                                    address,
                                    0,
                                );
                                let unchanged =
                                    self.builder.ins().icmp(IntCC::Equal, previous, value);
                                let store = self.builder.create_block();
                                let done = self.builder.create_block();
                                self.builder.ins().brif(unchanged, done, &[], store, &[]);
                                self.builder.seal_block(store);
                                self.builder.switch_to_block(store);
                                let owner = self.get(inst.a);
                                let call = self.builder.ins().call(
                                    self.helpers.store_reference,
                                    &[self.context, owner, address, value, index],
                                );
                                let stored = self.builder.inst_results(call)[0];
                                self.guard(stored, exit);
                                self.builder.ins().jump(done, &[]);
                                self.builder.seal_block(done);
                                self.builder.switch_to_block(done);
                            }
                        }
                    }
                }
            }
            VecPushI | VecPushF | VecPushB | VecPushP => {
                let exit = self.exit();
                let vector = self.get(inst.a);
                let value = self.get(inst.b);
                let call = self
                    .builder
                    .ins()
                    .call(self.helpers.vec_push, &[self.context, vector, value]);
                let pushed = self.builder.inst_results(call)[0];
                self.guard(pushed, exit);
                self.forget_vectors();
            }
            ArrayLen | VecLen => {
                let exit = self.exit();
                let (_, length) = self.resolve(inst.b, CacheKind::Collection(LENGTH_KIND), exit);
                let boxed = self.box_int_in_range(length);
                self.set(inst.a, boxed);
            }
            MulShr | ShlDiv => {
                let exit = self.exit();
                let left = self.get(inst.b);
                let left = self.unbox_int(left);
                let right = self.get(inst.c);
                let right = self.unbox_int(right);
                let shift = if fixed_mode(inst) == 0 {
                    self.builder.ins().iconst(types::I64, inst.imm & 0xff)
                } else {
                    let shift = self.get(inst.d);
                    self.unbox_int(shift)
                };
                let shift = self.builder.ins().band_imm_s(shift, 63);
                let raw = if inst.op == MulShr {
                    let wide_left = self.builder.ins().sextend(types::I128, left);
                    let wide_right = self.builder.ins().sextend(types::I128, right);
                    let product = self.builder.ins().imul(wide_left, wide_right);
                    let exact = self.builder.ins().sshr(product, shift);
                    let low = self.builder.ins().ireduce(types::I64, exact);
                    let back = self.builder.ins().sextend(types::I128, low);
                    let fits = self.builder.ins().icmp(IntCC::Equal, back, exact);
                    self.guard(fits, exit);
                    low
                } else {
                    let call = self
                        .builder
                        .ins()
                        .call(self.helpers.shl_div, &[self.context, left, right, shift]);
                    let ok = self.builder.inst_results(call)[0];
                    self.guard(ok, exit);
                    self.builder.ins().load(
                        types::I64,
                        Self::trusted(),
                        self.context,
                        FrameJitContext::SCRATCH0,
                    )
                };
                let boxed = self.box_int_checked(raw, exit);
                self.set(inst.a, boxed);
            }
            EnumTest | EnumLoad => {
                let exit = self.exit();
                let source = self.get(inst.b);
                let schema = self
                    .builder
                    .ins()
                    .iconst(types::I64, i64::try_from(inst.imm).unwrap_or(i64::MAX));
                let call = self.builder.ins().call(
                    self.helpers.enumeration,
                    &[self.context, self.data, source, schema],
                );
                let tag = self.builder.inst_results(call)[0];
                let expected = i64::try_from(inst.c + 1).map_err(|_| FrameRefusal::Decode)?;
                if inst.op == EnumTest {
                    let invalid = self.constant(aelys_runtime::FRAME_ENUM_INVALID);
                    let valid = self.builder.ins().icmp(IntCC::NotEqual, tag, invalid);
                    self.guard(valid, exit);
                    let matches = self.builder.ins().icmp_imm_s(IntCC::Equal, tag, expected);
                    let boxed = self.box_bool(matches);
                    self.set(inst.a, boxed);
                } else {
                    let matches = self.builder.ins().icmp_imm_s(IntCC::Equal, tag, expected);
                    self.guard(matches, exit);
                    let slots = self.builder.ins().load(
                        types::I64,
                        Self::trusted(),
                        self.context,
                        FrameJitContext::SCRATCH0,
                    );
                    let count = self.builder.ins().load(
                        types::I64,
                        Self::trusted(),
                        self.context,
                        FrameJitContext::SCRATCH1,
                    );
                    let inside = self.builder.ins().icmp_imm_s(
                        IntCC::UnsignedGreaterThan,
                        count,
                        i64::try_from(inst.d).unwrap_or(i64::MAX),
                    );
                    self.guard(inside, exit);
                    let offset = i32::try_from(inst.d * 8).map_err(|_| FrameRefusal::Decode)?;
                    let value = self
                        .builder
                        .ins()
                        .load(types::I64, Self::trusted(), slots, offset);
                    self.set(inst.a, value);
                }
            }
            VecForLoop | VecForLoopLong | ArrayForLoop | ArrayForLoopLong => {
                let Flow::Branch(target) = effect.flow else {
                    return Err(FrameRefusal::Decode);
                };
                let exit = self.exit();
                let collection = self.get(inst.a + 2);
                let counter = self.get(inst.a + 1);
                let is_int = self.is_int(counter);
                let raw = self.unbox_int(counter);
                let zero = self.builder.ins().iconst(types::I64, 0);
                let index = self.builder.ins().select(is_int, raw, zero);
                let array = self.builder.ins().iconst(
                    types::I64,
                    i64::from(matches!(inst.op, ArrayForLoop | ArrayForLoopLong)),
                );
                let call = self.builder.ins().call(
                    self.helpers.element,
                    &[self.context, collection, index, array],
                );
                let found = self.builder.inst_results(call)[0];
                let body = self.builder.create_block();
                let done = self.block_for(inst.next);
                self.builder.ins().brif(found, body, &[], done, &[]);
                self.builder.seal_block(body);
                self.builder.switch_to_block(body);
                let element = self.builder.ins().load(
                    types::I64,
                    Self::trusted(),
                    self.context,
                    FrameJitContext::SCRATCH0,
                );
                let next = self.builder.ins().iadd_imm_s(index, 1);
                let boxed = self.box_int_checked(next, exit);
                self.set(inst.a, element);
                self.set(inst.a + 1, boxed);
                self.count_backedge(inst, target);
                let taken = self.block_for(target);
                self.builder.ins().jump(taken, &[]);
                return Ok(true);
            }
            StructLoad => {
                let exit = self.exit();
                let schema = usize::try_from(inst.imm).map_err(|_| FrameRefusal::Decode)?;
                let (slots, count) = self.resolve(inst.b, CacheKind::Struct(schema), exit);
                let inside = self.builder.ins().icmp_imm_s(
                    IntCC::UnsignedGreaterThan,
                    count,
                    i64::try_from(inst.c).unwrap_or(i64::MAX),
                );
                self.guard(inside, exit);
                let offset = i32::try_from(inst.c * 8).map_err(|_| FrameRefusal::Decode)?;
                let value = self
                    .builder
                    .ins()
                    .load(types::I64, Self::trusted(), slots, offset);
                self.set(inst.a, value);
            }
            StructStore => {
                let exit = self.exit();
                let schema = usize::try_from(inst.imm).map_err(|_| FrameRefusal::Decode)?;
                let check = self
                    .analysis
                    .field_check(schema, inst.c)
                    .ok_or(FrameRefusal::Decode)?;
                let (slots, count) = self.resolve(inst.a, CacheKind::Struct(schema), exit);
                let inside = self.builder.ins().icmp_imm_s(
                    IntCC::UnsignedGreaterThan,
                    count,
                    i64::try_from(inst.c).unwrap_or(i64::MAX),
                );
                self.guard(inside, exit);
                let value = self.get(inst.b);
                match check {
                    FieldCheck::Int(width) => {
                        let is_int = self.is_int(value);
                        self.guard(is_int, exit);
                        if let Some((low, high)) = width_bounds(width) {
                            let number = self.unbox_int(value);
                            let above = self.builder.ins().icmp_imm_s(
                                IntCC::SignedGreaterThanOrEqual,
                                number,
                                low,
                            );
                            let below = self.builder.ins().icmp_imm_s(
                                IntCC::SignedLessThanOrEqual,
                                number,
                                high,
                            );
                            let fits = self.builder.ins().band(above, below);
                            self.guard(fits, exit);
                        }
                    }
                    FieldCheck::Float => {
                        let is_float = self.is_float(value);
                        self.guard(is_float, exit);
                    }
                    FieldCheck::Bool => {
                        let is_bool = self.is_bool(value);
                        self.guard(is_bool, exit);
                    }
                    FieldCheck::Unit => {
                        let is_unit = self.tag_is(value, Value::unit().raw() & TAG_BITS);
                        self.guard(is_unit, exit);
                    }
                }
                let offset = i32::try_from(inst.c * 8).map_err(|_| FrameRefusal::Decode)?;
                self.builder
                    .ins()
                    .store(Self::trusted(), value, slots, offset);
            }
            _ => return Err(FrameRefusal::Decode),
        }
        Ok(false)
    }

    fn block_for(&mut self, ip: usize) -> Block {
        if let Some(block) = self.blocks.get(&ip) {
            return *block;
        }
        let block = self.builder.create_block();
        self.builder.set_cold_block(block);
        self.blocks.insert(ip, block);
        let live = self
            .region
            .live_in
            .get(&ip)
            .copied()
            .unwrap_or_else(|| Live::all(self.analysis.registers));
        self.pending_exits.push((block, ip, live));
        block
    }

    // a native called below may collect: every live value that can be a reference must sit in the register stack
    fn flush_references(&mut self, live: &Live, skip: usize, window_start: usize) {
        for register in live.registers(self.analysis.registers).collect::<Vec<_>>() {
            if register == skip || register >= window_start {
                continue;
            }
            let value = self.get(register);
            if self.known.contains_key(&value) {
                continue;
            }
            let offset = i32::try_from(register * 8).unwrap_or(i32::MAX);
            self.builder
                .ins()
                .store(Self::trusted(), value, self.registers, offset);
        }
    }

    fn lower_call(&mut self, inst: &Inst) -> Result<(), FrameRefusal> {
        let window_start = call_window(inst);
        let count = inst.c;
        let destination = inst.a;
        let live_in = self.region.live_in[&inst.ip];
        let live_out = self.region.live_out[&inst.ip];
        let window_offset = i64::try_from(window_start * 8).map_err(|_| FrameRefusal::Decode)?;
        let window = self.builder.ins().iadd_imm_s(self.registers, window_offset);
        for argument in 0..count {
            let value = self.get(window_start + argument);
            let offset = i32::try_from(argument * 8).map_err(|_| FrameRefusal::Decode)?;
            self.builder
                .ins()
                .store(Self::trusted(), value, window, offset);
        }
        let (kind, callee) = if inst.op == OpCode::CallGlobal {
            let name = self
                .analysis
                .global_name(inst.b)
                .ok_or(FrameRefusal::Decode)?;
            let exit = self.exit();
            let callee = self.load_global(inst.b, exit);
            (
                FrameSiteKind::CallGlobal {
                    name: Arc::from(name),
                },
                callee,
            )
        } else {
            (FrameSiteKind::CallRegister, self.get(inst.b))
        };
        self.flush_references(&live_out, destination, window_start);
        let (site, offset) = self.site(kind);
        let entry = self.builder.ins().load(
            types::I64,
            Self::trusted(),
            self.data,
            offset + FrameData::SITE_ENTRY,
        );
        let cached = self.builder.ins().load(
            types::I64,
            Self::trusted(),
            self.data,
            offset + FrameData::SITE_VALUE,
        );
        let matches = self.builder.ins().icmp(IntCC::Equal, callee, cached);
        let compiled = self.builder.ins().icmp_imm_s(IntCC::NotEqual, entry, 0);
        let registers = self.builder.ins().load(
            types::I64,
            Self::trusted(),
            self.data,
            offset + FrameData::SITE_REGISTERS,
        );
        let window_bytes = self.builder.ins().ishl_imm_s(registers, 3);
        let window_end = self.builder.ins().iadd(window, window_bytes);
        let stack_end = self.builder.ins().load(
            types::I64,
            Self::trusted(),
            self.context,
            FrameJitContext::REGISTERS_END,
        );
        let fits = self
            .builder
            .ins()
            .icmp(IntCC::UnsignedLessThanOrEqual, window_end, stack_end);
        let depth = self.builder.ins().load(
            types::I64,
            Self::trusted(),
            self.context,
            FrameJitContext::DEPTH,
        );
        let deep_enough = self.builder.ins().icmp_imm_s(IntCC::NotEqual, depth, 0);
        let ready = self.builder.ins().band(matches, compiled);
        let ready = self.builder.ins().band(ready, fits);
        let ready = self.builder.ins().band(ready, deep_enough);
        let fast = self.builder.create_block();
        let slow = self.builder.create_block();
        let join = self.builder.create_block();
        let result = self.builder.append_block_param(join, types::I64);
        self.builder.ins().brif(ready, fast, &[], slow, &[]);
        self.builder.seal_block(fast);
        self.builder.seal_block(slow);
        self.builder.switch_to_block(fast);
        let callee_data = self.builder.ins().load(
            types::I64,
            Self::trusted(),
            self.data,
            offset + FrameData::SITE_DATA,
        );
        let lowered = self.builder.ins().iadd_imm_s(depth, -1);
        self.builder.ins().store(
            Self::trusted(),
            lowered,
            self.context,
            FrameJitContext::DEPTH,
        );
        let call = self.builder.ins().call_indirect(
            self.helpers.entry_signature,
            entry,
            &[self.context, window, callee_data],
        );
        let returned = self.builder.inst_results(call)[0];
        let depth_after = self.builder.ins().load(
            types::I64,
            Self::trusted(),
            self.context,
            FrameJitContext::DEPTH,
        );
        let restored = self.builder.ins().iadd_imm_s(depth_after, 1);
        self.builder.ins().store(
            Self::trusted(),
            restored,
            self.context,
            FrameJitContext::DEPTH,
        );
        self.builder.ins().jump(join, &[BlockArg::from(returned)]);
        self.builder.switch_to_block(slow);
        let site_value = self
            .builder
            .ins()
            .iconst(types::I64, i64::try_from(site).unwrap_or(0));
        let count_value = self
            .builder
            .ins()
            .iconst(types::I64, i64::try_from(count).unwrap_or(0));
        let call = self.builder.ins().call(
            self.helpers.call,
            &[
                self.context,
                self.data,
                site_value,
                callee,
                window,
                count_value,
            ],
        );
        let returned = self.builder.inst_results(call)[0];
        self.builder.ins().jump(join, &[BlockArg::from(returned)]);
        self.builder.seal_block(join);
        self.builder.switch_to_block(join);
        let status = self.builder.ins().load(
            types::I64,
            Self::trusted(),
            self.context,
            FrameJitContext::STATUS,
        );
        let ok = self.builder.create_block();
        let failed = self.builder.create_block();
        self.builder.set_cold_block(failed);
        self.builder.ins().brif(status, failed, &[], ok, &[]);
        self.builder.seal_block(failed);
        self.builder.seal_block(ok);
        self.builder.switch_to_block(failed);
        let in_interpreter = self.builder.create_block();
        let not_in_interpreter = self.builder.create_block();
        let native_failed = self.builder.create_block();
        let unwound = self.builder.create_block();
        for block in [in_interpreter, not_in_interpreter, native_failed, unwound] {
            self.builder.set_cold_block(block);
        }
        let interpret = self.builder.ins().icmp_imm_s(
            IntCC::Equal,
            status,
            i64::try_from(FRAME_STATUS_CALL_IN_INTERPRETER).unwrap_or(2),
        );
        self.builder
            .ins()
            .brif(interpret, in_interpreter, &[], not_in_interpreter, &[]);
        self.builder.seal_block(in_interpreter);
        self.builder.seal_block(not_in_interpreter);
        self.builder.switch_to_block(in_interpreter);
        self.flush(&live_in, None);
        self.leave(inst.ip);
        self.builder.switch_to_block(not_in_interpreter);
        let native_error = self.builder.ins().icmp_imm_s(
            IntCC::Equal,
            status,
            i64::try_from(FRAME_STATUS_NATIVE_ERROR).unwrap_or(3),
        );
        self.builder
            .ins()
            .brif(native_error, native_failed, &[], unwound, &[]);
        self.builder.seal_block(native_failed);
        self.builder.seal_block(unwound);
        self.builder.switch_to_block(native_failed);
        self.flush(&live_out, Some(destination));
        self.leave(inst.next);
        self.builder.switch_to_block(unwound);
        let destination_value = self
            .builder
            .ins()
            .iconst(types::I64, i64::try_from(destination).unwrap_or(0));
        self.builder.ins().call(
            self.helpers.push,
            &[self.context, callee, window, destination_value],
        );
        self.flush(&live_out, Some(destination));
        self.leave(inst.next);
        self.builder.switch_to_block(ok);
        self.set(destination, result);
        self.forget_vectors();
        self.ensure_view();
        Ok(())
    }

    fn forget_vectors(&mut self) {
        let vector_kinds = [
            FrameCollectionKind::VecInts,
            FrameCollectionKind::VecFloats,
            FrameCollectionKind::VecBools,
            FrameCollectionKind::VecObjects,
            FrameCollectionKind::Length,
        ];
        let entries = self
            .caches
            .iter()
            .filter(|((_, kind), _)| {
                matches!(kind, CacheKind::Collection(kind)
                    if vector_kinds.iter().any(|vector| *vector as u64 == *kind))
            })
            .map(|(_, cache)| cache.owner)
            .collect::<Vec<_>>();
        for owner in entries {
            let none = self.builder.ins().iconst(types::I64, NO_OWNER);
            self.builder.def_var(owner, none);
        }
    }
}

fn call_window(inst: &Inst) -> usize {
    if inst.op == OpCode::Call {
        inst.b + 1
    } else {
        inst.a + 1
    }
}

const LENGTH_KIND: u64 = FrameCollectionKind::Length as u64;

fn width_bounds(width: IntWidth) -> Option<(i64, i64)> {
    match width {
        IntWidth::I8 => Some((i64::from(i8::MIN), i64::from(i8::MAX))),
        IntWidth::I16 => Some((i64::from(i16::MIN), i64::from(i16::MAX))),
        IntWidth::I32 => Some((i64::from(i32::MIN), i64::from(i32::MAX))),
        IntWidth::I64 => None,
        IntWidth::U8 => Some((0, i64::from(u8::MAX))),
        IntWidth::U16 => Some((0, i64::from(u16::MAX))),
        IntWidth::U32 => Some((0, i64::from(u32::MAX))),
        IntWidth::U64 => Some((0, i64::MAX)),
    }
}

fn collection_kind(op: OpCode) -> FrameCollectionKind {
    use OpCode::*;
    match op {
        ArrayLoadI | ArrayStoreI => FrameCollectionKind::ArrayInts,
        ArrayLoadF | ArrayStoreF => FrameCollectionKind::ArrayFloats,
        ArrayLoadB | ArrayStoreB => FrameCollectionKind::ArrayBools,
        ArrayLoadP | ArrayStoreP => FrameCollectionKind::ArrayObjects,
        VecLoadI | VecStoreI => FrameCollectionKind::VecInts,
        VecLoadF | VecStoreF => FrameCollectionKind::VecFloats,
        VecLoadB | VecStoreB => FrameCollectionKind::VecBools,
        _ => FrameCollectionKind::VecObjects,
    }
}

fn cache_kind_of(inst: &Inst) -> Option<(usize, CacheKind)> {
    use OpCode::*;
    match inst.op {
        ArrayLoadI | ArrayLoadF | ArrayLoadB | ArrayLoadP | VecLoadI | VecLoadF | VecLoadB
        | VecLoadP => Some((
            inst.b,
            CacheKind::Collection(collection_kind(inst.op) as u64),
        )),
        ArrayStoreI | ArrayStoreF | ArrayStoreB | ArrayStoreP | VecStoreI | VecStoreF
        | VecStoreB | VecStoreP => Some((
            inst.a,
            CacheKind::Collection(collection_kind(inst.op) as u64),
        )),
        ArrayLen | VecLen => Some((inst.b, CacheKind::Collection(LENGTH_KIND))),
        StructLoad => Some((inst.b, CacheKind::Struct(usize::try_from(inst.imm).ok()?))),
        StructStore => Some((inst.a, CacheKind::Struct(usize::try_from(inst.imm).ok()?))),
        _ => None,
    }
}

fn entry_name(function: &Function, osr_ip: Option<u32>) -> String {
    let name = function.name.as_deref().unwrap_or("<script>");
    match osr_ip {
        Some(ip) => format!("aelys:{name}@{ip}"),
        None => format!("aelys:{name}"),
    }
}

pub(crate) fn frame_isa() -> Result<OwnedTargetIsa, String> {
    let mut flags = settings::builder();
    for (name, value) in [
        ("use_colocated_libcalls", "false"),
        ("is_pic", "false"),
        ("opt_level", "speed"),
        ("enable_verifier", "false"),
    ] {
        flags.set(name, value).map_err(|error| error.to_string())?;
    }
    cranelift_native::builder()
        .map_err(|error| error.to_string())?
        .finish(settings::Flags::new(flags))
        .map_err(|error| error.to_string())
}

pub(crate) fn compile_frame(
    isa: &OwnedTargetIsa,
    function: &Function,
    osr_ip: Option<u32>,
) -> Result<FrameCompiled, FrameRefusal> {
    let registers = usize::try_from(function.num_registers).map_err(|_| FrameRefusal::Registers)?;
    if registers > MAX_FRAME_REGISTERS || registers < usize::from(function.arity) {
        return Err(FrameRefusal::Registers);
    }
    if captures_locals(function) {
        return Err(FrameRefusal::Captures);
    }
    let instructions = decode(function).ok_or(FrameRefusal::Decode)?;
    let by_ip = instructions
        .iter()
        .enumerate()
        .map(|(index, inst)| (inst.ip, index))
        .collect::<HashMap<_, _>>();
    let entry_ip = match osr_ip {
        Some(ip) => usize::try_from(ip).map_err(|_| FrameRefusal::NoEntry)?,
        None => 0,
    };
    if !by_ip.contains_key(&entry_ip) {
        return Err(FrameRefusal::NoEntry);
    }
    let analysis = Analysis {
        function,
        registers,
    };
    let region = region(&analysis, &instructions, &by_ip, entry_ip).ok_or(FrameRefusal::Decode)?;
    if region.effects.get(&entry_ip).is_none_or(Option::is_none) {
        return Err(FrameRefusal::EmptyEntry);
    }
    if region.effects.keys().any(|ip| !by_ip.contains_key(ip)) {
        return Err(FrameRefusal::NoEntry);
    }
    let completes = region
        .effects
        .values()
        .flatten()
        .any(|effect| match effect.flow {
            Flow::Return => osr_ip.is_none(),
            Flow::Jump(target) | Flow::Branch(target) => osr_ip.is_some() && target == entry_ip,
            Flow::Next => false,
        });
    if !completes {
        return Err(FrameRefusal::EmptyEntry);
    }
    for (ip, effect) in &region.effects {
        let Some(effect) = effect else { continue };
        let Some(&index) = by_ip.get(ip) else {
            continue;
        };
        let inst = &instructions[index];
        if matches!(
            inst.op,
            OpCode::CallGlobal | OpCode::Call | OpCode::CallCached
        ) {
            let window_start = call_window(inst);
            let live_out = region.live_out[ip];
            if live_out
                .registers(registers)
                .any(|register| register >= window_start)
            {
                return Err(FrameRefusal::Decode);
            }
            let _ = effect;
        }
    }

    let mut jit_builder =
        JITBuilder::with_isa(Arc::clone(isa), cranelift_module::default_libcall_names());
    let symbols: [(&str, *const u8); 12] = [
        (
            "aelys_frame_shl_div",
            aelys_runtime::aelys_frame_shl_div as *const u8,
        ),
        (
            "aelys_frame_enum",
            aelys_runtime::aelys_frame_enum as *const u8,
        ),
        (
            "aelys_frame_element",
            aelys_runtime::aelys_frame_element as *const u8,
        ),
        (
            "aelys_frame_view",
            aelys_runtime::aelys_frame_view as *const u8,
        ),
        (
            "aelys_frame_set_global",
            aelys_runtime::aelys_frame_set_global as *const u8,
        ),
        (
            "aelys_frame_call",
            aelys_runtime::aelys_frame_call as *const u8,
        ),
        (
            "aelys_frame_push",
            aelys_runtime::aelys_frame_push as *const u8,
        ),
        (
            "aelys_frame_collection",
            aelys_runtime::aelys_frame_collection as *const u8,
        ),
        (
            "aelys_frame_struct",
            aelys_runtime::aelys_frame_struct as *const u8,
        ),
        (
            "aelys_frame_store_reference",
            aelys_runtime::aelys_frame_store_reference as *const u8,
        ),
        (
            "aelys_frame_fmod",
            aelys_runtime::aelys_frame_fmod as *const u8,
        ),
        (
            "aelys_frame_vec_push",
            aelys_runtime::aelys_frame_vec_push as *const u8,
        ),
    ];
    for (name, address) in symbols {
        jit_builder.symbol(name, address);
    }
    let mut module = JITModule::new(jit_builder);
    let pointer = module.target_config().pointer_type();
    let frontend_config = module.target_config();
    let mut context = module.make_context();
    let mut entry_signature = module.make_signature();
    for _ in 0..3 {
        entry_signature.params.push(AbiParam::new(pointer));
    }
    entry_signature.returns.push(AbiParam::new(types::I64));
    context.func.signature = entry_signature.clone();
    context.func.name = UserFuncName::user(0, 0);
    let function_id = module
        .declare_function(
            &entry_name(function, osr_ip),
            Linkage::Local,
            &entry_signature,
        )
        .map_err(|error| FrameRefusal::Codegen(error.to_string()))?;
    let declare = |module: &mut JITModule,
                   func: &mut cranelift_codegen::ir::Function,
                   name: &str,
                   params: &[types::Type],
                   returns: &[types::Type]|
     -> Result<FuncRef, FrameRefusal> {
        let mut signature: Signature = module.make_signature();
        for param in params {
            signature.params.push(AbiParam::new(*param));
        }
        for result in returns {
            signature.returns.push(AbiParam::new(*result));
        }
        let id = module
            .declare_function(name, Linkage::Import, &signature)
            .map_err(|error| FrameRefusal::Codegen(error.to_string()))?;
        Ok(module.declare_func_in_func(id, func))
    };
    let i64t = types::I64;
    let helpers = Helpers {
        shl_div: declare(
            &mut module,
            &mut context.func,
            "aelys_frame_shl_div",
            &[pointer, i64t, i64t, i64t],
            &[i64t],
        )?,
        enumeration: declare(
            &mut module,
            &mut context.func,
            "aelys_frame_enum",
            &[pointer, pointer, i64t, i64t],
            &[i64t],
        )?,
        element: declare(
            &mut module,
            &mut context.func,
            "aelys_frame_element",
            &[pointer, i64t, i64t, i64t],
            &[i64t],
        )?,
        view: declare(
            &mut module,
            &mut context.func,
            "aelys_frame_view",
            &[pointer, pointer],
            &[i64t],
        )?,
        set_global: declare(
            &mut module,
            &mut context.func,
            "aelys_frame_set_global",
            &[pointer, pointer, i64t, i64t],
            &[i64t],
        )?,
        call: declare(
            &mut module,
            &mut context.func,
            "aelys_frame_call",
            &[pointer, pointer, i64t, i64t, pointer, i64t],
            &[i64t],
        )?,
        push: declare(
            &mut module,
            &mut context.func,
            "aelys_frame_push",
            &[pointer, i64t, pointer, i64t],
            &[i64t],
        )?,
        collection: declare(
            &mut module,
            &mut context.func,
            "aelys_frame_collection",
            &[pointer, i64t, i64t],
            &[i64t],
        )?,
        structure: declare(
            &mut module,
            &mut context.func,
            "aelys_frame_struct",
            &[pointer, pointer, i64t, i64t],
            &[i64t],
        )?,
        store_reference: declare(
            &mut module,
            &mut context.func,
            "aelys_frame_store_reference",
            &[pointer, i64t, pointer, i64t, i64t],
            &[i64t],
        )?,
        vec_push: declare(
            &mut module,
            &mut context.func,
            "aelys_frame_vec_push",
            &[pointer, i64t, i64t],
            &[i64t],
        )?,
        fmod: declare(
            &mut module,
            &mut context.func,
            "aelys_frame_fmod",
            &[types::F64, types::F64],
            &[types::F64],
        )?,
        entry_signature: context.func.import_signature(entry_signature.clone()),
    };

    let mut builder_context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut context.func, &mut builder_context);
    let entry_block = builder.create_block();
    builder.append_block_params_for_function_params(entry_block);
    builder.switch_to_block(entry_block);
    builder.seal_block(entry_block);
    let context_value = builder.block_params(entry_block)[0];
    let registers_value = builder.block_params(entry_block)[1];
    let data_value = builder.block_params(entry_block)[2];
    let variables = (0..registers)
        .map(|_| builder.declare_var(types::I64))
        .collect::<Vec<_>>();
    let mut cache_kinds = Vec::new();
    for ip in &region.order {
        if let Some(Some(_)) = region.effects.get(ip)
            && let Some(index) = by_ip.get(ip)
            && let Some((_, kind)) = cache_kind_of(&instructions[*index])
            && !cache_kinds.contains(&kind)
        {
            cache_kinds.push(kind);
        }
    }
    let mut cached = std::collections::HashSet::new();
    for ip in &region.order {
        if let Some(Some(_)) = region.effects.get(ip)
            && let Some(index) = by_ip.get(ip)
            && let Some(pair) = cache_kind_of(&instructions[*index])
        {
            cached.insert(pair);
        }
    }
    let mut caches = HashMap::new();
    for register in 0..registers {
        for kind in cache_kinds
            .iter()
            .filter(|kind| cached.contains(&(register, **kind)))
        {
            let cache = CacheVars {
                owner: builder.declare_var(types::I64),
                pointer: builder.declare_var(types::I64),
                length: builder.declare_var(types::I64),
            };
            let zero = builder.ins().iconst(types::I64, 0);
            let none = builder.ins().iconst(types::I64, NO_OWNER);
            builder.def_var(cache.owner, none);
            builder.def_var(cache.pointer, zero);
            builder.def_var(cache.length, zero);
            caches.insert((register, *kind), cache);
        }
    }
    let null = builder.ins().iconst(
        types::I64,
        i64::from_ne_bytes(Value::null().raw().to_ne_bytes()),
    );
    for (register, variable) in variables.iter().enumerate() {
        let loaded = osr_ip.is_some() || register < usize::from(function.arity);
        let value = if loaded {
            let offset = i32::try_from(register * 8).map_err(|_| FrameRefusal::Registers)?;
            builder
                .ins()
                .load(types::I64, MemFlagsData::trusted(), registers_value, offset)
        } else {
            null
        };
        builder.def_var(*variable, value);
    }
    let ints = integer_registers(&analysis, &region, &instructions, &by_ip);
    let progress = builder.declare_var(types::I64);
    let zero = builder.ins().iconst(types::I64, 0);
    builder.def_var(progress, zero);
    let uses_globals = region.order.iter().any(|ip| {
        matches!(region.effects.get(ip), Some(Some(_)))
            && by_ip
                .get(ip)
                .is_some_and(|index| analysis.uses_globals(&instructions[*index]))
    });
    let mut blocks = HashMap::new();
    for leader in &region.leaders {
        blocks.insert(*leader, builder.create_block());
    }
    let first = blocks[&entry_ip];
    let mut lowerer = Lowerer {
        builder,
        analysis: &analysis,
        region: &region,
        instructions: &instructions,
        by_ip: &by_ip,
        blocks,
        variables,
        caches,
        cache_kinds,
        sites: Vec::new(),
        helpers,
        context: context_value,
        registers: registers_value,
        data: data_value,
        pending_exits: Vec::new(),
        current: None,
        known: HashMap::new(),
        origins: HashMap::new(),
        progress,
        uses_globals,
        ints: Live::default(),
    };
    lowerer.ints = ints;
    let entry_live = region.live_in.get(&entry_ip).copied().unwrap_or_default();
    let mut loaded = Live::default();
    for register in 0..registers {
        if osr_ip.is_some() || register < usize::from(function.arity) {
            loaded.insert(register);
        }
    }
    lowerer.unbox_entry(&entry_live, &loaded, entry_ip);
    lowerer.ensure_view();
    lowerer.builder.ins().jump(first, &[]);
    let leaders = region.leaders.iter().copied().collect::<Vec<_>>();
    for leader in leaders {
        lowerer.lower_block(leader)?;
    }
    lowerer.finish_exits();
    let sites = std::mem::take(&mut lowerer.sites);
    let mut builder = lowerer.builder;
    builder.seal_all_blocks();
    builder.finalize(frontend_config);
    module
        .define_function(function_id, &mut context)
        .map_err(|error| FrameRefusal::Codegen(format!("{error:?}")))?;
    module.clear_context(&mut context);
    module
        .finalize_definitions()
        .map_err(|error| FrameRefusal::Codegen(error.to_string()))?;
    let address = module.get_finalized_function(function_id) as usize;
    Ok(FrameCompiled {
        info: FrameCodeInfo {
            address,
            num_registers: function.num_registers,
            arity: function.arity,
            osr_ip,
            sites,
        },
        _module: Mutex::new(module),
    })
}
