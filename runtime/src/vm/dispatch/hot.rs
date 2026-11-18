use super::ops::sized::{fixed_value, sized_unary_value, wrap_binary};
use super::ops::structs::integer_fits;
use super::ops::sums::NONE_TAG;
use crate::vm::{CallFrame, GcObject, GcRef, MAX_FRAMES, ObjectKind, VM, Value};
use aelys_bytecode::object::{SumTag, UpvalueLocation};
use aelys_bytecode::{FixedKind, FunctionSchemas, OpCode, SizedInt, TypeDescriptor};
use std::sync::Arc;

// an 8-bit operand plus the two ForLoopI companions stays below this, even on unverified jump targets
const REGISTER_WINDOW: usize = 258;

const MOVE: u8 = OpCode::Move as u8;
const LOAD_I: u8 = OpCode::LoadI as u8;
const LOAD_K: u8 = OpCode::LoadK as u8;
const LOAD_NULL: u8 = OpCode::LoadNull as u8;
const LOAD_BOOL: u8 = OpCode::LoadBool as u8;
const LOAD_UNIT: u8 = OpCode::LoadUnit as u8;
const ADD: u8 = OpCode::Add as u8;
const SUB: u8 = OpCode::Sub as u8;
const NEG: u8 = OpCode::Neg as u8;
const CALL: u8 = OpCode::Call as u8;
const MUL_SHR: u8 = OpCode::MulShr as u8;
const SHL_DIV: u8 = OpCode::ShlDiv as u8;
const MUL: u8 = OpCode::Mul as u8;
const ADD_I: u8 = OpCode::AddI as u8;
const SUB_I: u8 = OpCode::SubI as u8;
const ADD_II: u8 = OpCode::AddII as u8;
const SUB_II: u8 = OpCode::SubII as u8;
const MUL_II: u8 = OpCode::MulII as u8;
const DIV_II: u8 = OpCode::DivII as u8;
const MOD_II: u8 = OpCode::ModII as u8;
const ADD_FF: u8 = OpCode::AddFF as u8;
const SUB_FF: u8 = OpCode::SubFF as u8;
const MUL_FF: u8 = OpCode::MulFF as u8;
const DIV_FF: u8 = OpCode::DivFF as u8;
const LT: u8 = OpCode::Lt as u8;
const LE: u8 = OpCode::Le as u8;
const GT: u8 = OpCode::Gt as u8;
const GE: u8 = OpCode::Ge as u8;
const LT_II: u8 = OpCode::LtII as u8;
const LE_II: u8 = OpCode::LeII as u8;
const GT_II: u8 = OpCode::GtII as u8;
const GE_II: u8 = OpCode::GeII as u8;
const EQ_II: u8 = OpCode::EqII as u8;
const NE_II: u8 = OpCode::NeII as u8;
const LT_FF: u8 = OpCode::LtFF as u8;
const LE_FF: u8 = OpCode::LeFF as u8;
const GT_FF: u8 = OpCode::GtFF as u8;
const GE_FF: u8 = OpCode::GeFF as u8;
const LT_IMM: u8 = OpCode::LtImm as u8;
const LE_IMM: u8 = OpCode::LeImm as u8;
const GT_IMM: u8 = OpCode::GtImm as u8;
const GE_IMM: u8 = OpCode::GeImm as u8;
const LT_I_IMM: u8 = OpCode::LtIImm as u8;
const LE_I_IMM: u8 = OpCode::LeIImm as u8;
const GT_I_IMM: u8 = OpCode::GtIImm as u8;
const GE_I_IMM: u8 = OpCode::GeIImm as u8;
const SHL: u8 = OpCode::Shl as u8;
const SHR: u8 = OpCode::Shr as u8;
const BIT_AND: u8 = OpCode::BitAnd as u8;
const BIT_OR: u8 = OpCode::BitOr as u8;
const BIT_XOR: u8 = OpCode::BitXor as u8;
const SHL_II: u8 = OpCode::ShlII as u8;
const SHR_II: u8 = OpCode::ShrII as u8;
const AND_II: u8 = OpCode::AndII as u8;
const OR_II: u8 = OpCode::OrII as u8;
const XOR_II: u8 = OpCode::XorII as u8;
const SHL_I_IMM: u8 = OpCode::ShlIImm as u8;
const SHR_I_IMM: u8 = OpCode::ShrIImm as u8;
const AND_I_IMM: u8 = OpCode::AndIImm as u8;
const OR_I_IMM: u8 = OpCode::OrIImm as u8;
const XOR_I_IMM: u8 = OpCode::XorIImm as u8;
const JUMP: u8 = OpCode::Jump as u8;
const JUMP_IF: u8 = OpCode::JumpIf as u8;
const JUMP_IF_NOT: u8 = OpCode::JumpIfNot as u8;
const JUMP_LONG: u8 = OpCode::JumpLong as u8;
const JUMP_IF_LONG: u8 = OpCode::JumpIfLong as u8;
const JUMP_IF_NOT_LONG: u8 = OpCode::JumpIfNotLong as u8;
const FOR_LOOP_I: u8 = OpCode::ForLoopI as u8;
const FOR_LOOP_I_INC: u8 = OpCode::ForLoopIInc as u8;
const WHILE_LOOP_LT: u8 = OpCode::WhileLoopLt as u8;
const GET_GLOBAL_IDX: u8 = OpCode::GetGlobalIdx as u8;
const SET_GLOBAL_IDX: u8 = OpCode::SetGlobalIdx as u8;
const ADD_GLOBAL_I: u8 = OpCode::AddGlobalI as u8;
const ARRAY_LOAD_I: u8 = OpCode::ArrayLoadI as u8;
const ARRAY_LOAD_F: u8 = OpCode::ArrayLoadF as u8;
const ARRAY_LOAD_B: u8 = OpCode::ArrayLoadB as u8;
const ARRAY_LOAD_P: u8 = OpCode::ArrayLoadP as u8;
const ARRAY_STORE_I: u8 = OpCode::ArrayStoreI as u8;
const ARRAY_STORE_F: u8 = OpCode::ArrayStoreF as u8;
const ARRAY_LEN: u8 = OpCode::ArrayLen as u8;
const VEC_LEN: u8 = OpCode::VecLen as u8;
const VEC_LOAD_I: u8 = OpCode::VecLoadI as u8;
const VEC_LOAD_F: u8 = OpCode::VecLoadF as u8;
const VEC_LOAD_B: u8 = OpCode::VecLoadB as u8;
const VEC_LOAD_P: u8 = OpCode::VecLoadP as u8;
const VEC_STORE_I: u8 = OpCode::VecStoreI as u8;
const VEC_STORE_F: u8 = OpCode::VecStoreF as u8;
const STRUCT_LOAD: u8 = OpCode::StructLoad as u8;
const STRUCT_STORE: u8 = OpCode::StructStore as u8;
const CALL_GLOBAL: u8 = OpCode::CallGlobal as u8;
const RETURN: u8 = OpCode::Return as u8;
const RETURN0: u8 = OpCode::Return0 as u8;
const GET_UPVAL: u8 = OpCode::GetUpval as u8;
const SET_UPVAL: u8 = OpCode::SetUpval as u8;
const CALL_CACHED: u8 = OpCode::CallCached as u8;
const LOAD_NONE: u8 = OpCode::LoadNone as u8;
const SUM_TEST: u8 = OpCode::SumTest as u8;
const SUM_PAYLOAD: u8 = OpCode::SumPayload as u8;
const ADD_W_I8: u8 = OpCode::AddWI8 as u8;
const DIV_W_I32: u8 = OpCode::DivWI32 as u8;
const NEG_W: u8 = OpCode::NegW as u8;
const NOT_W: u8 = OpCode::NotW as u8;

const HOT_OPCODES: &[u8] = &[
    MOVE,
    LOAD_I,
    LOAD_K,
    LOAD_NULL,
    LOAD_BOOL,
    LOAD_UNIT,
    ADD,
    SUB,
    MUL,
    ADD_I,
    SUB_I,
    ADD_II,
    SUB_II,
    MUL_II,
    DIV_II,
    MOD_II,
    ADD_FF,
    SUB_FF,
    MUL_FF,
    DIV_FF,
    LT,
    LE,
    GT,
    GE,
    LT_II,
    LE_II,
    GT_II,
    GE_II,
    EQ_II,
    NE_II,
    LT_FF,
    LE_FF,
    GT_FF,
    GE_FF,
    LT_IMM,
    LE_IMM,
    GT_IMM,
    GE_IMM,
    LT_I_IMM,
    LE_I_IMM,
    GT_I_IMM,
    GE_I_IMM,
    SHL,
    SHR,
    BIT_AND,
    BIT_OR,
    BIT_XOR,
    SHL_II,
    SHR_II,
    AND_II,
    OR_II,
    XOR_II,
    SHL_I_IMM,
    SHR_I_IMM,
    AND_I_IMM,
    OR_I_IMM,
    XOR_I_IMM,
    JUMP,
    JUMP_IF,
    JUMP_IF_NOT,
    JUMP_LONG,
    JUMP_IF_LONG,
    JUMP_IF_NOT_LONG,
    FOR_LOOP_I,
    FOR_LOOP_I_INC,
    WHILE_LOOP_LT,
    GET_GLOBAL_IDX,
    SET_GLOBAL_IDX,
    ADD_GLOBAL_I,
    ARRAY_LOAD_I,
    ARRAY_LOAD_F,
    ARRAY_LOAD_B,
    ARRAY_LOAD_P,
    ARRAY_STORE_I,
    ARRAY_STORE_F,
    ARRAY_LEN,
    VEC_LEN,
    VEC_LOAD_I,
    VEC_LOAD_F,
    VEC_LOAD_B,
    VEC_LOAD_P,
    VEC_STORE_I,
    VEC_STORE_F,
    STRUCT_LOAD,
    STRUCT_STORE,
    CALL_GLOBAL,
    RETURN,
    RETURN0,
    GET_UPVAL,
    SET_UPVAL,
    CALL_CACHED,
    LOAD_NONE,
    SUM_TEST,
    SUM_PAYLOAD,
    NEG,
    CALL,
    MUL_SHR,
    SHL_DIV,
];

pub(super) static HOT: [bool; 256] = {
    let mut table = [false; 256];
    let mut index = 0;
    while index < HOT_OPCODES.len() {
        table[HOT_OPCODES[index] as usize] = true;
        index += 1;
    }
    let mut sized = ADD_W_I8;
    while sized <= NOT_W {
        table[sized as usize] = true;
        sized += 1;
    }
    table
};

#[inline(always)]
fn int48(value: i64) -> Option<Value> {
    ((value << 16) >> 16 == value).then(|| Value::int_wrapping(value))
}

#[inline(always)]
fn truthy(value: Value) -> bool {
    !value.is_null() && value.as_bool() != Some(false)
}

#[inline(always)]
fn shift_count(count: i64) -> Option<u32> {
    u32::try_from(count).ok().filter(|count| *count < 48)
}

#[inline(always)]
fn exact_shl(value: i64, count: u32) -> Option<Value> {
    let shifted = value.checked_shl(count)?;
    if shifted >> count != value {
        return None;
    }
    int48(shifted)
}

// a final word the hot loop returns on or leaves at keeps sequential flow from running off the code
#[inline(always)]
fn ends_safely(code: *const u32, code_len: usize) -> bool {
    code_len > 0 && {
        let last = (unsafe { *code.add(code_len - 1) } >> 24) as u8;
        matches!(last, RETURN | RETURN0) || !HOT[last as usize]
    }
}

struct HotCall {
    function: GcRef,
    code: *const u32,
    code_len: usize,
    constants: *const Value,
    constants_len: usize,
    upvalues: *const GcRef,
    upvalues_len: usize,
    registers: u32,
    mapping: usize,
}

fn scalar_fits(value: Value, descriptor: &TypeDescriptor) -> bool {
    match descriptor {
        TypeDescriptor::Int(width) => value
            .as_int()
            .is_some_and(|value| integer_fits(*width, value)),
        TypeDescriptor::Float(_) => value.as_float().is_some(),
        TypeDescriptor::Bool => value.as_bool().is_some(),
        TypeDescriptor::Char => value.is_char(),
        _ => false,
    }
}

impl VM {
    #[inline(always)]
    fn frame_jit_quiet_at(&self, target: usize) -> bool {
        self.frames
            .last()
            .is_some_and(|frame| frame.jit_quiet_ip == target)
    }

    fn jit_loops_settled(&self) -> bool {
        !self.frame_jit_active()
    }

    fn jit_ignores(&self, function: GcRef) -> bool {
        !self.frame_jit_active() || self.frame_jit_interprets(function)
    }

    fn hot_schemas(&self, function: GcRef) -> Option<Arc<FunctionSchemas>> {
        let ObjectKind::Function(function) = &self.heap.get(function)?.kind else {
            return None;
        };
        let schemas = function.schemas();
        (schemas.runtime_ids.len() == schemas.structs.len()).then_some(schemas)
    }

    #[inline(always)]
    fn hot_global_callee<const JIT: bool>(&self, global: usize, arity: usize) -> Option<HotCall> {
        let callee = GcRef::new(self.globals_by_index.get(global)?.as_ptr()?);
        let ObjectKind::Function(target) = &self.heap.get(callee)?.kind else {
            return None;
        };
        if !target.verified || target.arity() != arity as u16 {
            return None;
        }
        let ignored = !JIT || self.jit_ignores(callee);
        ignored.then(|| HotCall {
            function: callee,
            code: target.function.bytecode.as_ptr(),
            code_len: target.function.bytecode.len(),
            constants: target.constants.as_ptr(),
            constants_len: target.constants.len(),
            upvalues: std::ptr::null(),
            upvalues_len: 0,
            registers: target.num_registers(),
            mapping: target.function.global_layout.id(),
        })
    }

    fn hot_value_callee(&self, value: Value, arity: usize) -> Option<HotCall> {
        let callee = GcRef::new(value.as_ptr()?);
        match &self.heap.get(callee)?.kind {
            ObjectKind::Function(target) => {
                if !target.verified || target.arity() != arity as u16 {
                    return None;
                }
                Some(HotCall {
                    function: callee,
                    code: target.function.bytecode.as_ptr(),
                    code_len: target.function.bytecode.len(),
                    constants: target.constants.as_ptr(),
                    constants_len: target.constants.len(),
                    upvalues: std::ptr::null(),
                    upvalues_len: 0,
                    registers: target.num_registers(),
                    mapping: target.function.global_layout.id(),
                })
            }
            ObjectKind::Closure(closure) => {
                let inner = closure.function;
                let ObjectKind::Function(target) = &self.heap.get(inner)?.kind else {
                    return None;
                };
                if !target.verified || closure.arity != arity as u16 {
                    return None;
                }
                Some(HotCall {
                    function: inner,
                    code: closure.bytecode_ptr,
                    code_len: closure.bytecode_len,
                    constants: closure.constants_ptr,
                    constants_len: closure.constants_len,
                    upvalues: closure.upvalues.as_ptr(),
                    upvalues_len: closure.upvalues.len(),
                    registers: closure.num_registers,
                    mapping: target.function.global_layout.id(),
                })
            }
            _ => None,
        }
    }

    #[inline(always)]
    fn hot_call_callee<const JIT: bool>(&self, value: Value, arity: usize) -> Option<HotCall> {
        let target = self.hot_value_callee(value, arity)?;
        if JIT && target.function.index() == value.as_ptr()? && !self.jit_ignores(target.function) {
            return None;
        }
        Some(target)
    }

    #[inline(never)]
    fn hot_store_global(&mut self, index: usize, value: Value) -> bool {
        self.store_global_in_view(index, value)
    }

    // operands is the two words after the instruction, both inside the code
    #[inline(never)]
    fn hot_fixed(&mut self, instr: u32, operands: *const u32, regs: *mut Value) -> bool {
        let (first, second) = unsafe { (*operands, *operands.add(1)) };
        let target = (first >> 16) as usize;
        let left = (first & 0xffff) as usize;
        let right = (second >> 16) as usize;
        let shift_register = (second & 0xffff) as usize;
        let mode = (instr >> 8) & 0xff;
        if target >= REGISTER_WINDOW
            || left >= REGISTER_WINDOW
            || right >= REGISTER_WINDOW
            || (mode != 0 && shift_register >= REGISTER_WINDOW)
        {
            return false;
        }
        let Some(kind) = FixedKind::from_u8((instr >> 16) as u8) else {
            return false;
        };
        let int = |index: usize| unsafe { (*regs.add(index)).as_int_unchecked() };
        let shift = if mode == 0 {
            i64::from(instr as u8)
        } else {
            int(shift_register)
        };
        let opcode = if (instr >> 24) as u8 == MUL_SHR {
            OpCode::MulShr
        } else {
            OpCode::ShlDiv
        };
        match fixed_value(opcode, kind, int(left), int(right), shift) {
            Some(Some(result)) => {
                unsafe { *regs.add(target) = Value::int_wrapping(result) };
                true
            }
            _ => false,
        }
    }

    #[inline(never)]
    fn hot_closure_and_sum(&mut self, instr: u32, regs: *mut Value, base: usize) -> bool {
        let a = ((instr >> 16) & 0xff) as usize;
        let b = ((instr >> 8) & 0xff) as usize;
        let c = (instr & 0xff) as u8;
        let value = match (instr >> 24) as u8 {
            GET_UPVAL => match self.upvalue_location(b) {
                Some(UpvalueLocation::Open {
                    frame_base,
                    register,
                }) => {
                    let index = frame_base + register as usize;
                    if index >= self.registers.len() {
                        return false;
                    }
                    unsafe { *regs.sub(base).add(index) }
                }
                Some(UpvalueLocation::Closed(value)) => value,
                None => return false,
            },
            SET_UPVAL => {
                let value = unsafe { *regs.add(b) };
                let Some(frame) = self.frames.last() else {
                    return false;
                };
                if a >= frame.upvalues_len {
                    return false;
                }
                let cell = unsafe { *frame.upvalues_ptr.add(a) };
                let registers = self.registers.len();
                let Some(ObjectKind::Upvalue(cell)) =
                    self.heap.get_mut(cell).map(|object| &mut object.kind)
                else {
                    return false;
                };
                match &mut cell.location {
                    UpvalueLocation::Open {
                        frame_base,
                        register,
                    } => {
                        let index = *frame_base + *register as usize;
                        if index >= registers {
                            return false;
                        }
                        unsafe { *regs.sub(base).add(index) = value }
                    }
                    UpvalueLocation::Closed(slot) => {
                        if value.as_ptr().is_some() {
                            return false;
                        }
                        *slot = value;
                    }
                }
                return true;
            }
            LOAD_NONE => Value::none(),
            SUM_TEST => {
                let value = unsafe { *regs.add(b) };
                Value::bool(if c == NONE_TAG {
                    value.is_none()
                } else {
                    let Some(expected) = SumTag::from_u8(c) else {
                        return false;
                    };
                    match value.as_ptr() {
                        Some(raw) => match self.heap.get(GcRef::new(raw)) {
                            Some(object) => {
                                matches!(&object.kind, ObjectKind::Sum(sum) if sum.tag == expected)
                            }
                            None => return false,
                        },
                        None => false,
                    }
                })
            }
            SUM_PAYLOAD => {
                let value = unsafe { *regs.add(b) };
                let Some(raw) = value.as_ptr() else {
                    return false;
                };
                match self.heap.get(GcRef::new(raw)).map(|object| &object.kind) {
                    Some(ObjectKind::Sum(sum)) => sum.payload,
                    _ => return false,
                }
            }
            _ => return false,
        };
        unsafe { *regs.add(a) = value };
        true
    }

    fn upvalue_location(&self, index: usize) -> Option<UpvalueLocation> {
        let frame = self.frames.last()?;
        if index >= frame.upvalues_len {
            return None;
        }
        let cell = unsafe { *frame.upvalues_ptr.add(index) };
        match &self.heap.get(cell)?.kind {
            ObjectKind::Upvalue(cell) => Some(cell.location.clone()),
            _ => None,
        }
    }

    // leaves before any effect of the instruction at the returned ip
    #[allow(clippy::too_many_arguments)]
    #[inline(never)]
    pub(super) fn run_hot<const JIT: bool>(
        &mut self,
        mut ip: usize,
        mut base: usize,
        mut function: GcRef,
        mut code: *const u32,
        mut code_len: usize,
        mut constants: *const Value,
        mut constants_len: usize,
    ) -> (usize, bool) {
        let mut changed = false;
        macro_rules! leave {
            () => {
                return (ip, changed)
            };
        }
        match base.checked_add(REGISTER_WINDOW) {
            Some(end) if end <= self.registers.len() && ip < code_len => {}
            _ => leave!(),
        }
        if !ends_safely(code, code_len) {
            leave!();
        }
        let mut regs = unsafe { self.registers.as_mut_ptr().add(base) };
        let mut schemas: Option<Arc<FunctionSchemas>> = None;
        let mut loops_stay = (!JIT).then_some(true);

        macro_rules! get {
            ($index:expr) => {
                unsafe { *regs.add($index) }
            };
        }
        macro_rules! set {
            ($index:expr, $value:expr) => {{
                let value = $value;
                unsafe { *regs.add($index) = value }
            }};
        }
        macro_rules! int {
            ($index:expr) => {
                get!($index).as_int_unchecked()
            };
        }
        macro_rules! or_leave {
            ($option:expr) => {
                match $option {
                    Some(value) => value,
                    None => leave!(),
                }
            };
        }
        macro_rules! both_ints {
            ($b:expr, $c:expr) => {
                match (get!($b).as_int(), get!($c).as_int()) {
                    (Some(left), Some(right)) => (left, right),
                    _ => leave!(),
                }
            };
        }
        macro_rules! object {
            ($value:expr) => {
                match $value.as_ptr() {
                    Some(raw) => match self.heap.get(GcRef::new(raw)) {
                        Some(object) => &object.kind,
                        None => leave!(),
                    },
                    None => leave!(),
                }
            };
        }
        macro_rules! object_mut {
            ($value:expr) => {
                match $value.as_ptr() {
                    Some(raw) => match self.heap.get_mut(GcRef::new(raw)) {
                        Some(object) => &mut object.kind,
                        None => leave!(),
                    },
                    None => leave!(),
                }
            };
        }
        macro_rules! index {
            ($index:expr) => {
                or_leave!(get!($index).as_index())
            };
        }
        macro_rules! slot {
            ($value:expr) => {
                or_leave!(
                    $value
                        .ptr_payload()
                        .and_then(|payload| self.heap.slot_object(payload))
                )
            };
        }
        macro_rules! slot_mut {
            ($value:expr) => {
                or_leave!(
                    $value
                        .ptr_payload()
                        .and_then(|payload| self.heap.slot_object_mut(payload))
                )
            };
        }
        macro_rules! struct_operands {
            ($instr:ident, $next:ident) => {{
                let (instr, next) = ($instr, $next);
                if next + 2 >= code_len || instr & 0x00ff_0000 != 0 {
                    leave!();
                }
                let first = unsafe { *code.add(next) };
                let second = unsafe { *code.add(next + 1) };
                let target = (first >> 16) as usize;
                let source = (first & 0xffff) as usize;
                if second & 0xffff != 0 || target >= REGISTER_WINDOW || source >= REGISTER_WINDOW {
                    leave!();
                }
                if schemas.is_none() {
                    schemas = self.hot_schemas(function);
                }
                let tables = or_leave!(schemas.as_deref());
                let schema_index = (instr & 0xffff) as usize;
                let schema = or_leave!(tables.structs.get(schema_index));
                if schema.schema_id as usize != schema_index {
                    leave!();
                }
                let field = (second >> 16) as usize;
                (
                    target,
                    source,
                    field,
                    schema,
                    tables.runtime_ids[schema_index],
                )
            }};
        }
        macro_rules! settled {
            () => {
                match loops_stay {
                    Some(settled) => settled,
                    None => {
                        let settled = self.jit_loops_settled();
                        loops_stay = Some(settled);
                        settled
                    }
                }
            };
        }
        macro_rules! branch {
            ($target:expr) => {{
                let target = $target;
                if (target <= ip && !(settled!() || self.frame_jit_quiet_at(target)))
                    || target >= code_len
                {
                    leave!();
                }
                target
            }};
        }

        loop {
            let instr = unsafe { *code.add(ip) };
            let a = ((instr >> 16) & 0xff) as usize;
            let b = ((instr >> 8) & 0xff) as usize;
            let c = (instr & 0xff) as usize;
            let next = ip + 1;
            macro_rules! enter {
                ($target:expr, $window:expr) => {{
                    let target: HotCall = $target;
                    let window = $window;
                    if !ends_safely(target.code, target.code_len) {
                        leave!();
                    }
                    let callee_base = base + window + 1;
                    match callee_base.checked_add((target.registers as usize).max(REGISTER_WINDOW))
                    {
                        Some(end) if end <= self.registers.len() => {}
                        _ => leave!(),
                    }
                    let depth = self.frames.len();
                    if depth >= MAX_FRAMES {
                        leave!();
                    }
                    let caller = &mut self.frames[depth - 1];
                    caller.ip = next;
                    if target.mapping != caller.global_mapping_id {
                        self.sync_current_function_globals();
                        self.switch_globals(target.mapping, target.function);
                    }
                    let mut frame = CallFrame::with_upvalues(
                        target.function,
                        callee_base,
                        a as u16,
                        target.code,
                        target.code_len,
                        target.constants,
                        target.constants_len,
                        target.upvalues,
                        target.upvalues_len,
                        target.registers,
                    );
                    frame.global_mapping_id = target.mapping;
                    self.frames.push(frame);
                    changed = true;
                    base = callee_base;
                    function = target.function;
                    code = target.code;
                    code_len = target.code_len;
                    constants = target.constants;
                    constants_len = target.constants_len;
                    regs = unsafe { self.registers.as_mut_ptr().add(base) };
                    schemas = None;
                    loops_stay = (!JIT).then_some(true);
                    0
                }};
            }
            ip = match (instr >> 24) as u8 {
                MOVE => {
                    set!(a, get!(b));
                    next
                }
                LOAD_I => {
                    set!(a, Value::int_wrapping(i64::from(instr as u16 as i16)));
                    next
                }
                LOAD_K => {
                    let index = (instr & 0xffff) as usize;
                    if index >= constants_len {
                        leave!();
                    }
                    let constant = unsafe { *constants.add(index) };
                    if constant.as_nested_fn_marker().is_some() {
                        leave!();
                    }
                    set!(a, constant);
                    next
                }
                LOAD_NULL => {
                    set!(a, Value::null());
                    next
                }
                LOAD_BOOL => {
                    set!(a, Value::bool(b != 0));
                    next
                }
                LOAD_UNIT => {
                    set!(a, Value::unit());
                    next
                }
                ADD => {
                    let (left, right) = both_ints!(b, c);
                    set!(a, or_leave!(int48(left.wrapping_add(right))));
                    next
                }
                SUB => {
                    let (left, right) = both_ints!(b, c);
                    set!(a, or_leave!(int48(left.wrapping_sub(right))));
                    next
                }
                MUL => {
                    let (left, right) = both_ints!(b, c);
                    set!(a, or_leave!(left.checked_mul(right).and_then(int48)));
                    next
                }
                ADD_I => {
                    set!(a, or_leave!(int48(int!(b).wrapping_add(c as i64))));
                    next
                }
                SUB_I => {
                    set!(a, or_leave!(int48(int!(b).wrapping_sub(c as i64))));
                    next
                }
                ADD_II => {
                    set!(a, or_leave!(int48(int!(b).wrapping_add(int!(c)))));
                    next
                }
                SUB_II => {
                    set!(a, or_leave!(int48(int!(b).wrapping_sub(int!(c)))));
                    next
                }
                MUL_II => {
                    set!(a, or_leave!(int!(b).checked_mul(int!(c)).and_then(int48)));
                    next
                }
                DIV_II => {
                    let right = int!(c);
                    if right == 0 {
                        leave!();
                    }
                    set!(a, or_leave!(int!(b).checked_div(right).and_then(int48)));
                    next
                }
                MOD_II => {
                    let right = int!(c);
                    if right == 0 {
                        leave!();
                    }
                    set!(a, or_leave!(int!(b).checked_rem(right).and_then(int48)));
                    next
                }
                ADD_FF => {
                    let value = get!(b).as_float_unchecked() + get!(c).as_float_unchecked();
                    set!(a, Value::float(value));
                    next
                }
                SUB_FF => {
                    let value = get!(b).as_float_unchecked() - get!(c).as_float_unchecked();
                    set!(a, Value::float(value));
                    next
                }
                MUL_FF => {
                    let value = get!(b).as_float_unchecked() * get!(c).as_float_unchecked();
                    set!(a, Value::float(value));
                    next
                }
                DIV_FF => {
                    let value = get!(b).as_float_unchecked() / get!(c).as_float_unchecked();
                    set!(a, Value::float(value));
                    next
                }
                LT => {
                    let (left, right) = both_ints!(b, c);
                    set!(a, Value::bool(left < right));
                    next
                }
                LE => {
                    let (left, right) = both_ints!(b, c);
                    set!(a, Value::bool(left <= right));
                    next
                }
                GT => {
                    let (left, right) = both_ints!(b, c);
                    set!(a, Value::bool(left > right));
                    next
                }
                GE => {
                    let (left, right) = both_ints!(b, c);
                    set!(a, Value::bool(left >= right));
                    next
                }
                LT_II => {
                    set!(a, Value::bool(int!(b) < int!(c)));
                    next
                }
                LE_II => {
                    set!(a, Value::bool(int!(b) <= int!(c)));
                    next
                }
                GT_II => {
                    set!(a, Value::bool(int!(b) > int!(c)));
                    next
                }
                GE_II => {
                    set!(a, Value::bool(int!(b) >= int!(c)));
                    next
                }
                EQ_II => {
                    set!(a, Value::bool(int!(b) == int!(c)));
                    next
                }
                NE_II => {
                    set!(a, Value::bool(int!(b) != int!(c)));
                    next
                }
                LT_FF => {
                    let value = get!(b).as_float_unchecked() < get!(c).as_float_unchecked();
                    set!(a, Value::bool(value));
                    next
                }
                LE_FF => {
                    let value = get!(b).as_float_unchecked() <= get!(c).as_float_unchecked();
                    set!(a, Value::bool(value));
                    next
                }
                GT_FF => {
                    let value = get!(b).as_float_unchecked() > get!(c).as_float_unchecked();
                    set!(a, Value::bool(value));
                    next
                }
                GE_FF => {
                    let value = get!(b).as_float_unchecked() >= get!(c).as_float_unchecked();
                    set!(a, Value::bool(value));
                    next
                }
                LT_IMM | LT_I_IMM => {
                    set!(a, Value::bool(int!(b) < c as i64));
                    next
                }
                LE_IMM | LE_I_IMM => {
                    set!(a, Value::bool(int!(b) <= c as i64));
                    next
                }
                GT_IMM | GT_I_IMM => {
                    set!(a, Value::bool(int!(b) > c as i64));
                    next
                }
                GE_IMM | GE_I_IMM => {
                    set!(a, Value::bool(int!(b) >= c as i64));
                    next
                }
                SHL => {
                    let (left, right) = both_ints!(b, c);
                    set!(a, or_leave!(exact_shl(left, or_leave!(shift_count(right)))));
                    next
                }
                SHR => {
                    let (left, right) = both_ints!(b, c);
                    set!(
                        a,
                        Value::int_wrapping(left >> or_leave!(shift_count(right)))
                    );
                    next
                }
                BIT_AND => {
                    let (left, right) = both_ints!(b, c);
                    set!(a, Value::int_wrapping(left & right));
                    next
                }
                BIT_OR => {
                    let (left, right) = both_ints!(b, c);
                    set!(a, Value::int_wrapping(left | right));
                    next
                }
                BIT_XOR => {
                    let (left, right) = both_ints!(b, c);
                    set!(a, Value::int_wrapping(left ^ right));
                    next
                }
                SHL_II => {
                    let count = or_leave!(shift_count(int!(c)));
                    set!(a, or_leave!(exact_shl(int!(b), count)));
                    next
                }
                SHR_II => {
                    let count = or_leave!(shift_count(int!(c)));
                    set!(a, Value::int_wrapping(int!(b) >> count));
                    next
                }
                AND_II => {
                    set!(a, Value::int_wrapping(int!(b) & int!(c)));
                    next
                }
                OR_II => {
                    set!(a, Value::int_wrapping(int!(b) | int!(c)));
                    next
                }
                XOR_II => {
                    set!(a, Value::int_wrapping(int!(b) ^ int!(c)));
                    next
                }
                SHL_I_IMM => {
                    let count = or_leave!(shift_count(c as i64));
                    set!(a, or_leave!(exact_shl(int!(b), count)));
                    next
                }
                SHR_I_IMM => {
                    let count = or_leave!(shift_count(c as i64));
                    set!(a, Value::int_wrapping(int!(b) >> count));
                    next
                }
                AND_I_IMM => {
                    set!(a, Value::int_wrapping(int!(b) & c as i64));
                    next
                }
                OR_I_IMM => {
                    set!(a, Value::int_wrapping(int!(b) | c as i64));
                    next
                }
                XOR_I_IMM => {
                    set!(a, Value::int_wrapping(int!(b) ^ c as i64));
                    next
                }
                JUMP => branch!(next.wrapping_add_signed(isize::from(instr as u16 as i16))),
                JUMP_IF => {
                    if truthy(get!(a)) {
                        branch!(next.wrapping_add_signed(isize::from(instr as u16 as i16)))
                    } else {
                        next
                    }
                }
                JUMP_IF_NOT => {
                    if truthy(get!(a)) {
                        next
                    } else {
                        branch!(next.wrapping_add_signed(isize::from(instr as u16 as i16)))
                    }
                }
                JUMP_LONG | JUMP_IF_LONG | JUMP_IF_NOT_LONG => {
                    let offset = unsafe { *code.add(next) } as i32;
                    let after = next + 1;
                    if after >= code_len {
                        leave!();
                    }
                    let take = match (instr >> 24) as u8 {
                        JUMP_LONG => true,
                        JUMP_IF_LONG => truthy(get!(a)),
                        _ => !truthy(get!(a)),
                    };
                    if take {
                        branch!(after.wrapping_add_signed(offset as isize))
                    } else {
                        after
                    }
                }
                FOR_LOOP_I | FOR_LOOP_I_INC => {
                    let iter = int!(a);
                    let end = int!(a + 1);
                    let step = int!(a + 2);
                    let advanced = iter.wrapping_add(step);
                    let value = or_leave!(int48(advanced));
                    let inclusive = (instr >> 24) as u8 == FOR_LOOP_I_INC;
                    let again = if step > 0 {
                        if inclusive {
                            advanced <= end
                        } else {
                            advanced < end
                        }
                    } else if inclusive {
                        advanced >= end
                    } else {
                        advanced > end
                    };
                    let target = next.wrapping_add_signed(isize::from(instr as u16 as i16));
                    if again && ((target <= ip && !settled!()) || target >= code_len) {
                        leave!();
                    }
                    set!(a, value);
                    if again { target } else { next }
                }
                WHILE_LOOP_LT => {
                    if int!(a) < int!(a + 1) {
                        branch!(next.wrapping_add_signed(isize::from(instr as u16 as i16)))
                    } else {
                        next
                    }
                }
                GET_GLOBAL_IDX => {
                    let index = (instr & 0xffff) as usize;
                    let value = or_leave!(self.globals_by_index.get(index).copied());
                    if value.is_uninit() {
                        leave!();
                    }
                    set!(a, value);
                    next
                }
                // int storage only ever receives 48-bit values, so a load needs no range check
                VEC_LOAD_I => {
                    let index = index!(c);
                    let Some(GcObject {
                        kind: ObjectKind::Vec(vector),
                        ..
                    }) = slot!(get!(b))
                    else {
                        leave!();
                    };
                    let Some(&value) = vector.data.as_ints().and_then(|items| items.get(index))
                    else {
                        leave!();
                    };
                    set!(a, Value::int_wrapping(value));
                    next
                }
                SET_GLOBAL_IDX => {
                    if !self.hot_store_global((instr & 0xffff) as usize, get!(a)) {
                        leave!();
                    }
                    next
                }
                ADD_GLOBAL_I => {
                    let global = or_leave!(self.globals_by_index.get(c).copied());
                    let (left, right) = (global, get!(b));
                    if !left.is_int() || !right.is_int() {
                        leave!();
                    }
                    let result = or_leave!(int48(
                        left.as_int_unchecked()
                            .wrapping_add(right.as_int_unchecked())
                    ));
                    if !self.hot_store_global(c, result) {
                        leave!();
                    }
                    set!(a, result);
                    next
                }
                VEC_LOAD_F | VEC_LOAD_B | VEC_LOAD_P => {
                    let index = index!(c);
                    let ObjectKind::Vec(vector) = object!(get!(b)) else {
                        leave!();
                    };
                    set!(a, or_leave!(vector.get(index)));
                    next
                }
                ARRAY_LOAD_I => {
                    let index = index!(c);
                    let Some(GcObject {
                        kind: ObjectKind::Array(array),
                        ..
                    }) = slot!(get!(b))
                    else {
                        leave!();
                    };
                    let Some(&value) = array.data.as_ints().and_then(|items| items.get(index))
                    else {
                        leave!();
                    };
                    set!(a, Value::int_wrapping(value));
                    next
                }
                ARRAY_LOAD_F | ARRAY_LOAD_B | ARRAY_LOAD_P => {
                    let index = index!(c);
                    let ObjectKind::Array(array) = object!(get!(b)) else {
                        leave!();
                    };
                    set!(a, or_leave!(array.get(index)));
                    next
                }
                VEC_STORE_I => {
                    let index = index!(b);
                    let value = get!(c);
                    if !value.is_int() {
                        leave!();
                    }
                    let value = value.as_int_unchecked();
                    let Some(GcObject {
                        kind: ObjectKind::Vec(vector),
                        ..
                    }) = slot_mut!(get!(a))
                    else {
                        leave!();
                    };
                    let Some(slot) = vector
                        .data
                        .as_ints_mut()
                        .and_then(|items| items.get_mut(index))
                    else {
                        leave!();
                    };
                    *slot = value;
                    next
                }
                VEC_STORE_F => {
                    let index = index!(b);
                    let value = or_leave!(get!(c).as_float());
                    let ObjectKind::Vec(vector) = object_mut!(get!(a)) else {
                        leave!();
                    };
                    let Some(slot) = vector
                        .data
                        .as_floats_mut()
                        .and_then(|items| items.get_mut(index))
                    else {
                        leave!();
                    };
                    *slot = value;
                    next
                }
                ARRAY_STORE_I => {
                    let index = index!(b);
                    let value = get!(c);
                    if !value.is_int() {
                        leave!();
                    }
                    let value = value.as_int_unchecked();
                    let Some(GcObject {
                        kind: ObjectKind::Array(array),
                        ..
                    }) = slot_mut!(get!(a))
                    else {
                        leave!();
                    };
                    let Some(slot) = array
                        .data
                        .as_ints_mut()
                        .and_then(|items| items.get_mut(index))
                    else {
                        leave!();
                    };
                    *slot = value;
                    next
                }
                ARRAY_STORE_F => {
                    let index = index!(b);
                    let value = or_leave!(get!(c).as_float());
                    let ObjectKind::Array(array) = object_mut!(get!(a)) else {
                        leave!();
                    };
                    let Some(slot) = array
                        .data
                        .as_floats_mut()
                        .and_then(|items| items.get_mut(index))
                    else {
                        leave!();
                    };
                    *slot = value;
                    next
                }
                STRUCT_LOAD => {
                    let (target, source, field, _, schema_id) = struct_operands!(instr, next);
                    let ObjectKind::Struct(structure) = object!(get!(source)) else {
                        leave!();
                    };
                    if structure.schema_id != schema_id {
                        leave!();
                    }
                    set!(target, *or_leave!(structure.slots.get(field)));
                    next + 2
                }
                STRUCT_STORE => {
                    let (target, source, field, schema, schema_id) = struct_operands!(instr, next);
                    let value = get!(source);
                    if !scalar_fits(value, &or_leave!(schema.fields.get(field)).ty) {
                        leave!();
                    }
                    let ObjectKind::Struct(structure) = object_mut!(get!(target)) else {
                        leave!();
                    };
                    if structure.schema_id != schema_id {
                        leave!();
                    }
                    *or_leave!(structure.slots.get_mut(field)) = value;
                    next + 2
                }
                CALL_GLOBAL => enter!(or_leave!(self.hot_global_callee::<JIT>(b, c)), a),
                CALL_CACHED => enter!(or_leave!(self.hot_value_callee(get!(b), c)), a),
                CALL => enter!(or_leave!(self.hot_call_callee::<JIT>(get!(b), c)), b),
                NEG => {
                    let value = get!(b);
                    if value.is_int() {
                        set!(a, or_leave!(int48(value.as_int_unchecked().wrapping_neg())));
                    } else if value.is_float() {
                        set!(a, Value::float(-value.as_float_unchecked()));
                    } else {
                        leave!();
                    }
                    next
                }
                MUL_SHR | SHL_DIV => {
                    if next + 2 >= code_len
                        || !self.hot_fixed(instr, unsafe { code.add(next) }, regs)
                    {
                        leave!();
                    }
                    next + 2
                }
                GET_UPVAL | SET_UPVAL | LOAD_NONE | SUM_TEST | SUM_PAYLOAD => {
                    if !self.hot_closure_and_sum(instr, regs, base) {
                        leave!();
                    }
                    next
                }
                RETURN | RETURN0 => {
                    let depth = self.frames.len();
                    if depth < 2 {
                        leave!();
                    }
                    let result = if (instr >> 24) as u8 == RETURN {
                        get!(a)
                    } else {
                        Value::unit()
                    };
                    let returning = &self.frames[depth - 1];
                    let destination = usize::from(returning.return_dest());
                    let switch =
                        returning.global_mapping_id != self.frames[depth - 2].global_mapping_id;
                    if switch {
                        self.sync_current_function_globals();
                    }
                    self.frames.pop();
                    changed = true;
                    let frame = &self.frames[depth - 2];
                    let mapping = frame.global_mapping_id;
                    ip = frame.ip;
                    base = frame.base;
                    function = frame.function;
                    code = frame.bytecode_ptr;
                    code_len = frame.bytecode_len;
                    constants = frame.constants_ptr;
                    constants_len = frame.constants_len;
                    if switch {
                        self.switch_globals(mapping, function);
                    }
                    self.registers[base + destination] = result;
                    schemas = None;
                    loops_stay = (!JIT).then_some(true);
                    match base.checked_add(REGISTER_WINDOW) {
                        Some(end)
                            if end <= self.registers.len()
                                && ip < code_len
                                && ends_safely(code, code_len) => {}
                        _ => leave!(),
                    }
                    regs = unsafe { self.registers.as_mut_ptr().add(base) };
                    ip
                }
                op @ ADD_W_I8..=DIV_W_I32 => {
                    set!(
                        a,
                        Value::int_wrapping(or_leave!(wrap_binary(op, int!(b), int!(c))))
                    );
                    next
                }
                NEG_W | NOT_W => {
                    let kind = or_leave!(SizedInt::from_u8(c as u8));
                    let opcode = if (instr >> 24) as u8 == NEG_W {
                        OpCode::NegW
                    } else {
                        OpCode::NotW
                    };
                    set!(
                        a,
                        Value::int_wrapping(sized_unary_value(opcode, kind, int!(b)))
                    );
                    next
                }
                VEC_LEN | ARRAY_LEN => {
                    let length = match object!(get!(b)) {
                        ObjectKind::Vec(vector) => vector.len(),
                        ObjectKind::Array(array) => array.len(),
                        _ => leave!(),
                    };
                    set!(a, or_leave!(i64::try_from(length).ok().and_then(int48)));
                    next
                }
                _ => leave!(),
            };
        }
    }
}
