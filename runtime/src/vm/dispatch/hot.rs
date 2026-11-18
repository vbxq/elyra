use crate::vm::{GcRef, ObjectKind, VM, Value};
use aelys_bytecode::OpCode;

// any 8-bit operand plus the two companions of ForLoopI stays inside this window, even on words a jump reaches unverified
const REGISTER_WINDOW: usize = 258;

const MOVE: u8 = OpCode::Move as u8;
const LOAD_I: u8 = OpCode::LoadI as u8;
const LOAD_K: u8 = OpCode::LoadK as u8;
const LOAD_NULL: u8 = OpCode::LoadNull as u8;
const LOAD_BOOL: u8 = OpCode::LoadBool as u8;
const LOAD_UNIT: u8 = OpCode::LoadUnit as u8;
const ADD: u8 = OpCode::Add as u8;
const SUB: u8 = OpCode::Sub as u8;
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

#[inline(always)]
fn int48(value: i64) -> Option<Value> {
    Value::int_checked(value).ok()
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

impl VM {
    // returns the ip of the first instruction it does not finish, before any of its effects, for the general loop to run
    #[inline(never)]
    pub(super) fn run_hot<const JIT: bool>(
        &mut self,
        mut ip: usize,
        base: usize,
        code: *const u32,
        code_len: usize,
        constants: *const Value,
        constants_len: usize,
    ) -> usize {
        match base.checked_add(REGISTER_WINDOW) {
            Some(end) if end <= self.registers.len() => {}
            _ => return ip,
        }
        let regs = unsafe { self.registers.as_mut_ptr().add(base) };

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
                    None => return ip,
                }
            };
        }
        macro_rules! both_ints {
            ($b:expr, $c:expr) => {
                match (get!($b).as_int(), get!($c).as_int()) {
                    (Some(left), Some(right)) => (left, right),
                    _ => return ip,
                }
            };
        }
        macro_rules! object {
            ($value:expr) => {
                match $value.as_ptr() {
                    Some(raw) => match self.heap.get(GcRef::new(raw)) {
                        Some(object) => &object.kind,
                        None => return ip,
                    },
                    None => return ip,
                }
            };
        }
        macro_rules! object_mut {
            ($value:expr) => {
                match $value.as_ptr() {
                    Some(raw) => match self.heap.get_mut_for_scalar_write(GcRef::new(raw)) {
                        Some(object) => &mut object.kind,
                        None => return ip,
                    },
                    None => return ip,
                }
            };
        }
        macro_rules! index {
            ($index:expr) => {
                match get!($index).as_int() {
                    Some(index) if index >= 0 => index as usize,
                    _ => return ip,
                }
            };
        }
        macro_rules! branch {
            ($target:expr) => {{
                let target = $target;
                if JIT && target <= ip {
                    return ip;
                }
                target
            }};
        }

        loop {
            if ip >= code_len {
                return ip;
            }
            let instr = unsafe { *code.add(ip) };
            let a = ((instr >> 16) & 0xff) as usize;
            let b = ((instr >> 8) & 0xff) as usize;
            let c = (instr & 0xff) as usize;
            let imm = i64::from((instr & 0xffff) as u16 as i16);
            let next = ip + 1;
            ip = match (instr >> 24) as u8 {
                MOVE => {
                    set!(a, get!(b));
                    next
                }
                LOAD_I => {
                    set!(a, Value::int_wrapping(imm));
                    next
                }
                LOAD_K => {
                    let index = (instr & 0xffff) as usize;
                    if index >= constants_len {
                        return ip;
                    }
                    let constant = unsafe { *constants.add(index) };
                    if constant.as_nested_fn_marker().is_some() {
                        return ip;
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
                        return ip;
                    }
                    set!(a, or_leave!(int!(b).checked_div(right).and_then(int48)));
                    next
                }
                MOD_II => {
                    let right = int!(c);
                    if right == 0 {
                        return ip;
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
                JUMP => branch!(next.wrapping_add_signed(imm as isize)),
                JUMP_IF => {
                    if truthy(get!(a)) {
                        branch!(next.wrapping_add_signed(imm as isize))
                    } else {
                        next
                    }
                }
                JUMP_IF_NOT => {
                    if truthy(get!(a)) {
                        next
                    } else {
                        branch!(next.wrapping_add_signed(imm as isize))
                    }
                }
                JUMP_LONG | JUMP_IF_LONG | JUMP_IF_NOT_LONG => {
                    if next >= code_len {
                        return ip;
                    }
                    let offset = unsafe { *code.add(next) } as i32;
                    let after = next + 1;
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
                    let target = next.wrapping_add_signed(imm as isize);
                    if again && JIT && target <= ip {
                        return ip;
                    }
                    set!(a, value);
                    if again { target } else { next }
                }
                WHILE_LOOP_LT => {
                    if int!(a) < int!(a + 1) {
                        branch!(next.wrapping_add_signed(imm as isize))
                    } else {
                        next
                    }
                }
                GET_GLOBAL_IDX => {
                    let index = (instr & 0xffff) as usize;
                    set!(a, or_leave!(self.globals_by_index.get(index).copied()));
                    next
                }
                VEC_LOAD_I => {
                    let index = index!(c);
                    let ObjectKind::Vec(vector) = object!(get!(b)) else {
                        return ip;
                    };
                    let Some(&value) = vector.data.as_ints().and_then(|items| items.get(index))
                    else {
                        return ip;
                    };
                    set!(a, or_leave!(int48(value)));
                    next
                }
                VEC_LOAD_F | VEC_LOAD_B | VEC_LOAD_P => {
                    let index = index!(c);
                    let ObjectKind::Vec(vector) = object!(get!(b)) else {
                        return ip;
                    };
                    set!(a, or_leave!(vector.get(index)));
                    next
                }
                ARRAY_LOAD_I => {
                    let index = index!(c);
                    let ObjectKind::Array(array) = object!(get!(b)) else {
                        return ip;
                    };
                    let Some(&value) = array.data.as_ints().and_then(|items| items.get(index))
                    else {
                        return ip;
                    };
                    set!(a, or_leave!(int48(value)));
                    next
                }
                ARRAY_LOAD_F | ARRAY_LOAD_B | ARRAY_LOAD_P => {
                    let index = index!(c);
                    let ObjectKind::Array(array) = object!(get!(b)) else {
                        return ip;
                    };
                    set!(a, or_leave!(array.get(index)));
                    next
                }
                VEC_STORE_I => {
                    let index = index!(b);
                    let value = or_leave!(get!(c).as_int());
                    let ObjectKind::Vec(vector) = object_mut!(get!(a)) else {
                        return ip;
                    };
                    let Some(slot) = vector
                        .data
                        .as_ints_mut()
                        .and_then(|items| items.get_mut(index))
                    else {
                        return ip;
                    };
                    *slot = value;
                    next
                }
                VEC_STORE_F => {
                    let index = index!(b);
                    let value = or_leave!(get!(c).as_float());
                    let ObjectKind::Vec(vector) = object_mut!(get!(a)) else {
                        return ip;
                    };
                    let Some(slot) = vector
                        .data
                        .as_floats_mut()
                        .and_then(|items| items.get_mut(index))
                    else {
                        return ip;
                    };
                    *slot = value;
                    next
                }
                ARRAY_STORE_I => {
                    let index = index!(b);
                    let value = or_leave!(get!(c).as_int());
                    let ObjectKind::Array(array) = object_mut!(get!(a)) else {
                        return ip;
                    };
                    let Some(slot) = array
                        .data
                        .as_ints_mut()
                        .and_then(|items| items.get_mut(index))
                    else {
                        return ip;
                    };
                    *slot = value;
                    next
                }
                ARRAY_STORE_F => {
                    let index = index!(b);
                    let value = or_leave!(get!(c).as_float());
                    let ObjectKind::Array(array) = object_mut!(get!(a)) else {
                        return ip;
                    };
                    let Some(slot) = array
                        .data
                        .as_floats_mut()
                        .and_then(|items| items.get_mut(index))
                    else {
                        return ip;
                    };
                    *slot = value;
                    next
                }
                VEC_LEN | ARRAY_LEN => {
                    let length = match object!(get!(b)) {
                        ObjectKind::Vec(vector) => vector.len(),
                        ObjectKind::Array(array) => array.len(),
                        _ => return ip,
                    };
                    set!(a, or_leave!(i64::try_from(length).ok().and_then(int48)));
                    next
                }
                _ => return ip,
            };
        }
    }
}
