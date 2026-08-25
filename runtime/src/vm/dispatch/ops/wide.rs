use super::super::state::{DispatchControl, DispatchState};
use crate::vm::{GcRef, ObjectKind, OpCode, VM, Value};
use aelys_bytecode::object::{AelysArray, AelysVec};
use aelys_common::error::{RuntimeError, RuntimeErrorKind};

impl VM {
    #[allow(clippy::too_many_arguments)]
    #[inline]
    pub(in crate::vm::dispatch) fn execute_wide(
        &mut self,
        state: &DispatchState,
        instruction_pointer: &mut usize,
        func_ref: GcRef,
        bytecode_ptr: *const u32,
        upvalues_ptr: *const GcRef,
        upvalues_len: usize,
        instr: u32,
    ) -> Result<DispatchControl, RuntimeError> {
        let mut ip = *instruction_pointer;
        let base = state.base;
        let constants_ptr = state.constants;
        let constants_len = state.constants_len;
        let current_frame_idx = state.frame_index;

        macro_rules! reg_get {
            ($index:expr) => {
                state.read_register(self, $index, ip)?
            };
        }
        macro_rules! reg_set {
            ($index:expr, $value:expr) => {
                state.write_register(self, $index, $value, ip)?
            };
        }
        macro_rules! int_value {
            ($value:expr) => {{
                match Value::int_checked($value) {
                    Ok(value) => value,
                    Err(_) => {
                        self.frames[current_frame_idx].ip = ip;
                        return Err(self.runtime_error(RuntimeErrorKind::IntegerOverflow));
                    }
                }
            }};
        }

        let inner_byte = u8::try_from((instr >> 16) & 0xff).expect("wide opcode occupies one byte");
        let inner_opcode = OpCode::from_u8(inner_byte).ok_or_else(|| {
            self.runtime_error(RuntimeErrorKind::InvalidBytecode(format!(
                "invalid wide inner opcode {inner_byte}"
            )))
        })?;
        let first = unsafe { *bytecode_ptr.add(ip) };
        let second = unsafe { *bytecode_ptr.add(ip + 1) };
        ip += 2;
        let a = (first >> 16) as usize;
        let b = (first & 0xffff) as usize;
        let c = (second >> 16) as usize;

        match inner_opcode {
            OpCode::Move => {
                let value = reg_get!(base + b);
                reg_set!(base + a, value);
            }
            OpCode::LoadI => {
                let immediate_bits = u16::try_from(b).expect("wide immediate occupies two bytes");
                let immediate = i16::from_ne_bytes(immediate_bits.to_ne_bytes());
                reg_set!(base + a, Value::int(i64::from(immediate)));
            }
            OpCode::LoadK => {
                let index = (b << 16) | c;
                if index >= constants_len {
                    self.frames[current_frame_idx].ip = ip;
                    return Err(
                        self.runtime_error(RuntimeErrorKind::InvalidBytecode(format!(
                            "constant index {index} out of bounds"
                        ))),
                    );
                }
                let constant = unsafe { *constants_ptr.add(index) };
                if let Some(function_index) = constant.as_nested_fn_marker() {
                    self.frames[current_frame_idx].ip = ip;
                    let nested_function = self.get_nested_function(func_ref, function_index)?;
                    self.verify_function_value(&nested_function)?;
                    let function_ref = self.alloc_function(nested_function)?;
                    self.inherit_jit_key(func_ref, function_ref, function_index);
                    reg_set!(base + a, Value::ptr(function_ref.index()));
                } else {
                    reg_set!(base + a, constant);
                }
            }
            OpCode::GetGlobalIdx => {
                let index = (b << 16) | c;
                let value = self
                    .globals_by_index
                    .get(index)
                    .copied()
                    .unwrap_or(Value::null());
                reg_set!(base + a, value);
            }
            OpCode::SetGlobalIdx => {
                let index = (b << 16) | c;
                let value = reg_get!(base + a);
                self.set_global_by_index(index, value);
            }
            OpCode::LoadNull => reg_set!(base + a, Value::null()),
            OpCode::LoadUnit => reg_set!(base + a, Value::unit()),
            OpCode::LoadNone => reg_set!(base + a, Value::none()),
            OpCode::LoadBool => reg_set!(base + a, Value::bool(b != 0)),
            OpCode::RangeNew | OpCode::RangeNewInclusive => {
                let start = reg_get!(base + b);
                let end = reg_get!(base + c);
                self.frames[current_frame_idx].ip = ip;
                let range =
                    self.alloc_range(start, end, inner_opcode == OpCode::RangeNewInclusive)?;
                reg_set!(base + a, Value::ptr(range.index()));
            }
            OpCode::MakeSum => {
                let tag =
                    aelys_bytecode::object::SumTag::from_u8(u8::try_from(c).map_err(|_| {
                        self.runtime_error(RuntimeErrorKind::InvalidBytecode(
                            "wide sum tag exceeds u8".to_string(),
                        ))
                    })?)
                    .ok_or_else(|| {
                        self.runtime_error(RuntimeErrorKind::InvalidBytecode(format!(
                            "invalid sum tag {c}"
                        )))
                    })?;
                let payload = reg_get!(base + b);
                let sum = self.alloc_sum(tag, payload)?;
                reg_set!(base + a, Value::ptr(sum.index()));
            }
            OpCode::SumTest => {
                let value = reg_get!(base + b);
                let tag = u8::try_from(c).map_err(|_| {
                    self.runtime_error(RuntimeErrorKind::InvalidBytecode(
                        "wide sum tag exceeds u8".to_string(),
                    ))
                })?;
                let matches = if tag == 4 {
                    value.is_none()
                } else {
                    let expected =
                        aelys_bytecode::object::SumTag::from_u8(tag).ok_or_else(|| {
                            self.runtime_error(RuntimeErrorKind::InvalidBytecode(format!(
                                "invalid sum tag {tag}"
                            )))
                        })?;
                    value.as_ptr().is_some_and(|pointer| {
                        self.heap.get(GcRef::new(pointer)).is_some_and(|object| {
                            match &object.kind {
                                ObjectKind::Sum(sum) => sum.tag == expected,
                                _ => false,
                            }
                        })
                    })
                };
                reg_set!(base + a, Value::bool(matches));
            }
            OpCode::SumPayload => {
                let value = reg_get!(base + b);
                let pointer = value.as_ptr().ok_or_else(|| {
                    self.runtime_error(RuntimeErrorKind::TypeError {
                        operation: "sum payload",
                        expected: "sum",
                        got: value.type_name().to_string(),
                    })
                })?;
                let object = self
                    .heap
                    .get(GcRef::new(pointer))
                    .ok_or_else(|| self.runtime_error(RuntimeErrorKind::UseAfterFree))?;
                let ObjectKind::Sum(sum) = &object.kind else {
                    return Err(self.runtime_error(RuntimeErrorKind::TypeError {
                        operation: "sum payload",
                        expected: "sum",
                        got: value.type_name().to_string(),
                    }));
                };
                reg_set!(base + a, sum.payload);
            }
            OpCode::Cast => {
                let target = u8::try_from(c)
                    .ok()
                    .and_then(aelys_bytecode::CastTarget::from_u8)
                    .ok_or_else(|| {
                        self.runtime_error(RuntimeErrorKind::InvalidBytecode(format!(
                            "invalid cast target {c}"
                        )))
                    })?;
                let value = reg_get!(base + b);
                let result = self.cast_value(value, target)?;
                reg_set!(base + a, result);
            }
            OpCode::MatchFail => {
                if a == 0 {
                    return Err(self.runtime_error(RuntimeErrorKind::InvalidBytecode(
                        "match reached an invalid runtime value".to_string(),
                    )));
                }
                let message = if c == 1 {
                    Some(reg_get!(base + b))
                } else {
                    None
                };
                return Err(self.sum_unwrap_error(u8::try_from(a).unwrap_or(u8::MAX), message));
            }
            OpCode::Add | OpCode::AddII | OpCode::AddFF | OpCode::AddIIG | OpCode::AddFFG => {
                let left = reg_get!(base + b);
                let right = reg_get!(base + c);
                let result = if let (Some(left), Some(right)) = (left.as_int(), right.as_int()) {
                    int_value!(left.wrapping_add(right))
                } else {
                    self.frames[current_frame_idx].ip = ip;
                    self.add_values(left, right)?
                };
                reg_set!(base + a, result);
            }
            OpCode::Sub | OpCode::SubII | OpCode::SubFF | OpCode::SubIIG | OpCode::SubFFG => {
                let left = reg_get!(base + b);
                let right = reg_get!(base + c);
                let result = if let (Some(left), Some(right)) = (left.as_int(), right.as_int()) {
                    int_value!(left.wrapping_sub(right))
                } else {
                    self.frames[current_frame_idx].ip = ip;
                    self.sub_values(left, right)?
                };
                reg_set!(base + a, result);
            }
            OpCode::Mul | OpCode::MulII | OpCode::MulFF | OpCode::MulIIG | OpCode::MulFFG => {
                let left = reg_get!(base + b);
                let right = reg_get!(base + c);
                let result = if let (Some(left), Some(right)) = (left.as_int(), right.as_int()) {
                    int_value!(left.wrapping_mul(right))
                } else {
                    self.frames[current_frame_idx].ip = ip;
                    self.mul_values(left, right)?
                };
                reg_set!(base + a, result);
            }
            OpCode::Div | OpCode::DivII | OpCode::DivFF | OpCode::DivIIG | OpCode::DivFFG => {
                let left = reg_get!(base + b);
                let right = reg_get!(base + c);
                self.frames[current_frame_idx].ip = ip;
                reg_set!(base + a, self.div_values(left, right)?);
            }
            OpCode::Mod | OpCode::ModII | OpCode::ModFF | OpCode::ModIIG | OpCode::ModFFG => {
                let left = reg_get!(base + b);
                let right = reg_get!(base + c);
                self.frames[current_frame_idx].ip = ip;
                reg_set!(base + a, self.mod_values(left, right)?);
            }
            OpCode::Neg => {
                let value = reg_get!(base + b);
                self.frames[current_frame_idx].ip = ip;
                reg_set!(base + a, self.neg_value(value)?);
            }
            OpCode::AddI | OpCode::SubI => {
                let source = reg_get!(base + b);
                let value = source.as_int().ok_or_else(|| {
                    self.runtime_error(RuntimeErrorKind::TypeError {
                        operation: "wide integer immediate",
                        expected: "integer",
                        got: self.value_type_name(source).to_string(),
                    })
                })?;
                let result = if inner_opcode == OpCode::AddI {
                    value.wrapping_add(c as i64)
                } else {
                    value.wrapping_sub(c as i64)
                };
                reg_set!(base + a, int_value!(result));
            }
            OpCode::Eq
            | OpCode::Ne
            | OpCode::EqII
            | OpCode::NeII
            | OpCode::EqFF
            | OpCode::NeFF
            | OpCode::EqIIG
            | OpCode::NeIIG
            | OpCode::EqFFG
            | OpCode::NeFFG => {
                let left = reg_get!(base + b);
                let right = reg_get!(base + c);
                let mut equal = self.values_equal(left, right);
                if matches!(
                    inner_opcode,
                    OpCode::Ne | OpCode::NeII | OpCode::NeFF | OpCode::NeIIG | OpCode::NeFFG
                ) {
                    equal = !equal;
                }
                reg_set!(base + a, Value::bool(equal));
            }
            OpCode::Lt
            | OpCode::Le
            | OpCode::Gt
            | OpCode::Ge
            | OpCode::LtII
            | OpCode::LeII
            | OpCode::GtII
            | OpCode::GeII
            | OpCode::LtFF
            | OpCode::LeFF
            | OpCode::GtFF
            | OpCode::GeFF
            | OpCode::LtIIG
            | OpCode::LeIIG
            | OpCode::GtIIG
            | OpCode::GeIIG
            | OpCode::LtFFG
            | OpCode::LeFFG
            | OpCode::GtFFG
            | OpCode::GeFFG => {
                let left = reg_get!(base + b);
                let right = reg_get!(base + c);
                let result = match inner_opcode {
                    OpCode::Lt | OpCode::LtII | OpCode::LtFF | OpCode::LtIIG | OpCode::LtFFG => {
                        self.compare_lt(left, right)?
                    }
                    OpCode::Le | OpCode::LeII | OpCode::LeFF | OpCode::LeIIG | OpCode::LeFFG => {
                        self.compare_le(left, right)?
                    }
                    OpCode::Gt | OpCode::GtII | OpCode::GtFF | OpCode::GtIIG | OpCode::GtFFG => {
                        self.compare_gt(left, right)?
                    }
                    _ => self.compare_ge(left, right)?,
                };
                reg_set!(base + a, Value::bool(result));
            }
            OpCode::LtIImm | OpCode::LeIImm | OpCode::GtIImm | OpCode::GeIImm => {
                let source = reg_get!(base + b);
                let left = source.as_int().ok_or_else(|| {
                    self.runtime_error(RuntimeErrorKind::TypeError {
                        operation: "wide integer comparison",
                        expected: "integer",
                        got: self.value_type_name(source).to_string(),
                    })
                })?;
                let right = c as i64;
                let result = match inner_opcode {
                    OpCode::LtIImm => left < right,
                    OpCode::LeIImm => left <= right,
                    OpCode::GtIImm => left > right,
                    _ => left >= right,
                };
                reg_set!(base + a, Value::bool(result));
            }
            OpCode::Not => {
                let value = reg_get!(base + b);
                let is_falsy =
                    value.is_null() || value.as_bool() == Some(false) || value.as_int() == Some(0);
                reg_set!(base + a, Value::bool(is_falsy));
            }
            OpCode::Shl
            | OpCode::Shr
            | OpCode::BitAnd
            | OpCode::BitOr
            | OpCode::BitXor
            | OpCode::ShlII
            | OpCode::ShrII
            | OpCode::AndII
            | OpCode::OrII
            | OpCode::XorII => {
                let left_value = reg_get!(base + b);
                let right_value = reg_get!(base + c);
                let left = left_value.as_int().ok_or_else(|| {
                    self.runtime_error(RuntimeErrorKind::TypeError {
                        operation: "wide bitwise",
                        expected: "integer",
                        got: self.value_type_name(left_value).to_string(),
                    })
                })?;
                let right = right_value.as_int().ok_or_else(|| {
                    self.runtime_error(RuntimeErrorKind::TypeError {
                        operation: "wide bitwise",
                        expected: "integer",
                        got: self.value_type_name(right_value).to_string(),
                    })
                })?;
                let result = match inner_opcode {
                    OpCode::Shl | OpCode::ShlII => left << (right & 63),
                    OpCode::Shr | OpCode::ShrII => left >> (right & 63),
                    OpCode::BitAnd | OpCode::AndII => left & right,
                    OpCode::BitOr | OpCode::OrII => left | right,
                    _ => left ^ right,
                };
                reg_set!(base + a, Value::int_wrapping(result));
            }
            OpCode::BitNot | OpCode::NotI => {
                let value = reg_get!(base + b);
                let value = value.as_int().ok_or_else(|| {
                    self.runtime_error(RuntimeErrorKind::TypeError {
                        operation: "wide bitwise not",
                        expected: "integer",
                        got: self.value_type_name(value).to_string(),
                    })
                })?;
                reg_set!(base + a, Value::int_wrapping(!value));
            }
            OpCode::ShlIImm
            | OpCode::ShrIImm
            | OpCode::AndIImm
            | OpCode::OrIImm
            | OpCode::XorIImm => {
                let value = reg_get!(base + b);
                let value = value.as_int().ok_or_else(|| {
                    self.runtime_error(RuntimeErrorKind::TypeError {
                        operation: "wide bitwise immediate",
                        expected: "integer",
                        got: self.value_type_name(value).to_string(),
                    })
                })?;
                let immediate = c as i64;
                let result = match inner_opcode {
                    OpCode::ShlIImm => value << (immediate & 63),
                    OpCode::ShrIImm => value >> (immediate & 63),
                    OpCode::AndIImm => value & immediate,
                    OpCode::OrIImm => value | immediate,
                    _ => value ^ immediate,
                };
                reg_set!(base + a, Value::int_wrapping(result));
            }
            OpCode::ArrayLoadI | OpCode::ArrayLoadF | OpCode::ArrayLoadB | OpCode::ArrayLoadP => {
                let array_value = reg_get!(base + b);
                let index_value = reg_get!(base + c);
                let index = index_value.as_int().unwrap_or(-1);
                if index < 0 {
                    self.frames[current_frame_idx].ip = ip;
                    return Err(
                        self.runtime_error(RuntimeErrorKind::IndexOutOfBounds { index, length: 0 })
                    );
                }
                let index_usize = usize::try_from(index).map_err(|_| {
                    self.runtime_error(RuntimeErrorKind::IndexOutOfBounds { index, length: 0 })
                })?;
                let array_ref = GcRef::new(array_value.as_ptr().unwrap_or(0));
                let value = match self.heap.get(array_ref) {
                    Some(object) => match &object.kind {
                        ObjectKind::Array(array) => array.get(index_usize).ok_or_else(|| {
                            self.runtime_error(RuntimeErrorKind::IndexOutOfBounds {
                                index,
                                length: i64::try_from(array.len()).unwrap_or(i64::MAX),
                            })
                        })?,
                        _ => {
                            return Err(self.runtime_error(RuntimeErrorKind::TypeError {
                                operation: "array load",
                                expected: "array",
                                got: "non-array object".to_string(),
                            }));
                        }
                    },
                    None => {
                        return Err(self.runtime_error(RuntimeErrorKind::InvalidMemoryHandle));
                    }
                };
                reg_set!(base + a, value);
            }
            OpCode::ArrayGetI | OpCode::ArrayGetF | OpCode::ArrayGetB | OpCode::ArrayGetP => {
                let array_value = reg_get!(base + b);
                let index = reg_get!(base + c).as_int().unwrap_or(-1);
                let value = self.array_get_option(array_value, index)?;
                reg_set!(base + a, value);
            }
            OpCode::ArrayStoreI
            | OpCode::ArrayStoreF
            | OpCode::ArrayStoreB
            | OpCode::ArrayStoreP => {
                let array_value = reg_get!(base + a);
                let index = reg_get!(base + b).as_int().unwrap_or(-1);
                let value = reg_get!(base + c);
                let index_usize = usize::try_from(index).map_err(|_| {
                    self.runtime_error(RuntimeErrorKind::IndexOutOfBounds { index, length: 0 })
                })?;
                let array_ref = GcRef::new(array_value.as_ptr().unwrap_or(0));
                let length = match self.heap.get(array_ref) {
                    Some(object) => match &object.kind {
                        ObjectKind::Array(array) => i64::try_from(array.len()).unwrap_or(i64::MAX),
                        _ => {
                            return Err(self.runtime_error(RuntimeErrorKind::TypeError {
                                operation: "array store",
                                expected: "array",
                                got: "non-array object".to_string(),
                            }));
                        }
                    },
                    None => {
                        return Err(self.runtime_error(RuntimeErrorKind::InvalidMemoryHandle));
                    }
                };
                let stored =
                    self.heap
                        .get_mut(array_ref)
                        .is_some_and(|object| match &mut object.kind {
                            ObjectKind::Array(array) => array.set(index_usize, value),
                            _ => false,
                        });
                if !stored {
                    return Err(
                        self.runtime_error(RuntimeErrorKind::IndexOutOfBounds { index, length })
                    );
                }
            }
            OpCode::ArrayNewI | OpCode::ArrayNewF | OpCode::ArrayNewB | OpCode::ArrayNewP => {
                let count = reg_get!(base + b).as_int().unwrap_or(0);
                if count < 0 {
                    return Err(self.runtime_error(RuntimeErrorKind::TypeError {
                        operation: "array allocation",
                        expected: "a non-negative size",
                        got: count.to_string(),
                    }));
                }
                let count = usize::try_from(count).map_err(|_| {
                    self.runtime_error(RuntimeErrorKind::TypeError {
                        operation: "array allocation",
                        expected: "a representable size",
                        got: count.to_string(),
                    })
                })?;
                let type_tag = match inner_opcode {
                    OpCode::ArrayNewI => aelys_bytecode::object::TypeTag::Int,
                    OpCode::ArrayNewF => aelys_bytecode::object::TypeTag::Float,
                    OpCode::ArrayNewB => aelys_bytecode::object::TypeTag::Bool,
                    _ => aelys_bytecode::object::TypeTag::Object,
                };
                self.ensure_array_capacity(type_tag, count)?;
                let array = match inner_opcode {
                    OpCode::ArrayNewI => AelysArray::new_ints(count),
                    OpCode::ArrayNewF => AelysArray::new_floats(count),
                    OpCode::ArrayNewB => AelysArray::new_bools(count),
                    _ => AelysArray::new_objects(count),
                };
                self.frames[current_frame_idx].ip = ip;
                let array_ref = self.alloc_array(array)?;
                reg_set!(base + a, Value::ptr(array_ref.index()));
            }
            OpCode::ArrayLen => {
                let value = reg_get!(base + b);
                let reference = GcRef::new(value.as_ptr().unwrap_or(0));
                let length = match self.heap.get(reference) {
                    Some(object) => match &object.kind {
                        ObjectKind::Array(array) => array.len(),
                        ObjectKind::Vec(vector) => vector.len(),
                        _ => {
                            return Err(self.runtime_error(RuntimeErrorKind::TypeError {
                                operation: "array len",
                                expected: "array or vec",
                                got: "non-array object".to_string(),
                            }));
                        }
                    },
                    None => {
                        return Err(self.runtime_error(RuntimeErrorKind::InvalidMemoryHandle));
                    }
                };
                reg_set!(
                    base + a,
                    Value::int(i64::try_from(length).unwrap_or(i64::MAX))
                );
            }
            OpCode::ArraySlice => {
                let array = reg_get!(base + b);
                let range = reg_get!(base + c);
                self.frames[current_frame_idx].ip = ip;
                let sliced = self.slice_array(array, range)?;
                reg_set!(base + a, sliced);
            }
            OpCode::VecSlice => {
                let vector = reg_get!(base + b);
                let range = reg_get!(base + c);
                self.frames[current_frame_idx].ip = ip;
                let sliced = self.slice_vec(vector, range)?;
                reg_set!(base + a, sliced);
            }
            OpCode::VecNewI | OpCode::VecNewF | OpCode::VecNewB | OpCode::VecNewP => {
                let vector = match inner_opcode {
                    OpCode::VecNewI => AelysVec::new_ints(),
                    OpCode::VecNewF => AelysVec::new_floats(),
                    OpCode::VecNewB => AelysVec::new_bools(),
                    _ => AelysVec::new_objects(),
                };
                self.frames[current_frame_idx].ip = ip;
                let vector_ref = self.alloc_vec(vector)?;
                reg_set!(base + a, Value::ptr(vector_ref.index()));
            }
            OpCode::VecPushI | OpCode::VecPushF | OpCode::VecPushB | OpCode::VecPushP => {
                let vector_value = reg_get!(base + a);
                let value = reg_get!(base + b);
                let vector_ref = GcRef::new(vector_value.as_ptr().unwrap_or(0));
                self.push_vec_value(vector_ref, value)?;
            }
            OpCode::VecPopI | OpCode::VecPopF | OpCode::VecPopB | OpCode::VecPopP => {
                let vector_value = reg_get!(base + b);
                let value = self.pop_vec_option(vector_value)?;
                reg_set!(base + a, value);
            }
            OpCode::VecLen => {
                let vector_value = reg_get!(base + b);
                let vector_ref = GcRef::new(vector_value.as_ptr().unwrap_or(0));
                let length = match self.heap.get(vector_ref) {
                    Some(object) => match &object.kind {
                        ObjectKind::Vec(vector) => vector.len(),
                        ObjectKind::Array(array) => array.len(),
                        ObjectKind::String(string) => string.len(),
                        _ => {
                            return Err(self.runtime_error(RuntimeErrorKind::TypeError {
                                operation: "len",
                                expected: "vec, array, or string",
                                got: "non-collection object".to_string(),
                            }));
                        }
                    },
                    None => {
                        return Err(self.runtime_error(RuntimeErrorKind::InvalidMemoryHandle));
                    }
                };
                reg_set!(
                    base + a,
                    Value::int(i64::try_from(length).unwrap_or(i64::MAX))
                );
            }
            OpCode::VecCap => {
                let vector_value = reg_get!(base + b);
                let vector_ref = GcRef::new(vector_value.as_ptr().unwrap_or(0));
                let capacity = match self.heap.get(vector_ref) {
                    Some(object) => match &object.kind {
                        ObjectKind::Vec(vector) => vector.capacity(),
                        _ => {
                            return Err(self.runtime_error(RuntimeErrorKind::TypeError {
                                operation: "vec capacity",
                                expected: "vec",
                                got: "non-vec object".to_string(),
                            }));
                        }
                    },
                    None => {
                        return Err(self.runtime_error(RuntimeErrorKind::InvalidMemoryHandle));
                    }
                };
                reg_set!(
                    base + a,
                    Value::int(i64::try_from(capacity).unwrap_or(i64::MAX))
                );
            }
            OpCode::VecReserve => {
                let vector_value = reg_get!(base + a);
                let additional = reg_get!(base + b).as_int().unwrap_or(0);
                if additional < 0 {
                    return Err(self.runtime_error(RuntimeErrorKind::TypeError {
                        operation: "vec reserve",
                        expected: "a non-negative amount",
                        got: additional.to_string(),
                    }));
                }
                let additional = usize::try_from(additional).map_err(|_| {
                    self.runtime_error(RuntimeErrorKind::TypeError {
                        operation: "vec reserve",
                        expected: "a representable amount",
                        got: additional.to_string(),
                    })
                })?;
                let vector_ref = GcRef::new(vector_value.as_ptr().unwrap_or(0));
                self.reserve_vec(vector_ref, additional)?;
            }
            OpCode::VecLoadI | OpCode::VecLoadF | OpCode::VecLoadB | OpCode::VecLoadP => {
                enum LoadedValue {
                    Value(Value),
                    Character(String),
                }

                let vec_value = reg_get!(base + b);
                let index = reg_get!(base + c).as_int().unwrap_or(-1);
                let index_usize = usize::try_from(index).map_err(|_| {
                    self.runtime_error(RuntimeErrorKind::IndexOutOfBounds { index, length: 0 })
                })?;
                let vec_ref = GcRef::new(vec_value.as_ptr().unwrap_or(0));
                let loaded = match self.heap.get(vec_ref) {
                    Some(object) => match &object.kind {
                        ObjectKind::Vec(vector) => {
                            LoadedValue::Value(vector.get(index_usize).ok_or_else(|| {
                                self.runtime_error(RuntimeErrorKind::IndexOutOfBounds {
                                    index,
                                    length: i64::try_from(vector.len()).unwrap_or(i64::MAX),
                                })
                            })?)
                        }
                        ObjectKind::Array(array) if inner_opcode == OpCode::VecLoadP => {
                            LoadedValue::Value(array.get(index_usize).ok_or_else(|| {
                                self.runtime_error(RuntimeErrorKind::IndexOutOfBounds {
                                    index,
                                    length: i64::try_from(array.len()).unwrap_or(i64::MAX),
                                })
                            })?)
                        }
                        ObjectKind::String(string) if inner_opcode == OpCode::VecLoadP => {
                            let character =
                                string.as_str().chars().nth(index_usize).ok_or_else(|| {
                                    self.runtime_error(RuntimeErrorKind::IndexOutOfBounds {
                                        index,
                                        length: i64::try_from(string.as_str().chars().count())
                                            .unwrap_or(i64::MAX),
                                    })
                                })?;
                            LoadedValue::Character(character.to_string())
                        }
                        _ => {
                            return Err(self.runtime_error(RuntimeErrorKind::TypeError {
                                operation: "vec load",
                                expected: if inner_opcode == OpCode::VecLoadP {
                                    "vec, array, or string"
                                } else {
                                    "vec"
                                },
                                got: "non-vec object".to_string(),
                            }));
                        }
                    },
                    None => {
                        return Err(self.runtime_error(RuntimeErrorKind::InvalidMemoryHandle));
                    }
                };
                let value = match loaded {
                    LoadedValue::Value(value) => value,
                    LoadedValue::Character(character) => {
                        self.frames[current_frame_idx].ip = ip;
                        Value::ptr(self.intern_string(&character)?.index())
                    }
                };
                reg_set!(base + a, value);
            }
            OpCode::VecGetI | OpCode::VecGetF | OpCode::VecGetB | OpCode::VecGetP => {
                let vec_value = reg_get!(base + b);
                let index = reg_get!(base + c).as_int().unwrap_or(-1);
                let value = self.vec_get_option(vec_value, index)?;
                reg_set!(base + a, value);
            }
            OpCode::VecStoreI | OpCode::VecStoreF | OpCode::VecStoreB | OpCode::VecStoreP => {
                let vec_value = reg_get!(base + a);
                let index = reg_get!(base + b).as_int().unwrap_or(-1);
                let value = reg_get!(base + c);
                let index_usize = usize::try_from(index).map_err(|_| {
                    self.runtime_error(RuntimeErrorKind::IndexOutOfBounds { index, length: 0 })
                })?;
                let vec_ref = GcRef::new(vec_value.as_ptr().unwrap_or(0));
                let length = match self.heap.get(vec_ref) {
                    Some(object) => match &object.kind {
                        ObjectKind::Vec(vector) => i64::try_from(vector.len()).unwrap_or(i64::MAX),
                        _ => {
                            return Err(self.runtime_error(RuntimeErrorKind::TypeError {
                                operation: "vec store",
                                expected: "vec",
                                got: "non-vec object".to_string(),
                            }));
                        }
                    },
                    None => {
                        return Err(self.runtime_error(RuntimeErrorKind::InvalidMemoryHandle));
                    }
                };
                let stored =
                    self.heap
                        .get_mut(vec_ref)
                        .is_some_and(|object| match &mut object.kind {
                            ObjectKind::Vec(vector) => vector.set(index_usize, value),
                            _ => false,
                        });
                if !stored {
                    return Err(
                        self.runtime_error(RuntimeErrorKind::IndexOutOfBounds { index, length })
                    );
                }
            }
            OpCode::StringLoadChar => {
                let string_value = reg_get!(base + b);
                let index = reg_get!(base + c).as_int().unwrap_or(-1);
                let index_usize = usize::try_from(index).map_err(|_| {
                    self.runtime_error(RuntimeErrorKind::IndexOutOfBounds { index, length: 0 })
                })?;
                let string_ref = GcRef::new(string_value.as_ptr().unwrap_or(0));
                let character = match self.heap.get(string_ref) {
                    Some(object) => match &object.kind {
                        ObjectKind::String(string) => {
                            let owned = string.as_str().to_string();
                            match owned.chars().nth(index_usize) {
                                Some(character) => character.to_string(),
                                None => {
                                    return Err(self.runtime_error(
                                        RuntimeErrorKind::IndexOutOfBounds {
                                            index,
                                            length: i64::try_from(owned.chars().count())
                                                .unwrap_or(i64::MAX),
                                        },
                                    ));
                                }
                            }
                        }
                        _ => {
                            return Err(self.runtime_error(RuntimeErrorKind::TypeError {
                                operation: "string load char",
                                expected: "string",
                                got: "non-string object".to_string(),
                            }));
                        }
                    },
                    None => {
                        return Err(self.runtime_error(RuntimeErrorKind::InvalidMemoryHandle));
                    }
                };
                self.frames[current_frame_idx].ip = ip;
                let character_ref = self.intern_string(&character)?;
                reg_set!(base + a, Value::ptr(character_ref.index()));
            }
            OpCode::GetUpval => {
                if b >= upvalues_len {
                    return Err(
                        self.runtime_error(RuntimeErrorKind::UndefinedVariable(format!(
                            "upvalue index {b} out of bounds"
                        ))),
                    );
                }
                let upvalue_ref = unsafe { *upvalues_ptr.add(b) };
                let value = self.get_upvalue_value(upvalue_ref);
                reg_set!(base + a, value);
            }
            OpCode::SetUpval => {
                if a >= upvalues_len {
                    return Err(
                        self.runtime_error(RuntimeErrorKind::UndefinedVariable(format!(
                            "upvalue index {a} out of bounds"
                        ))),
                    );
                }
                let upvalue_ref = unsafe { *upvalues_ptr.add(a) };
                let value = reg_get!(base + b);
                self.set_upvalue_value(upvalue_ref, value);
            }
            OpCode::CloseUpvals => self.close_upvalues_from(base + a),
            OpCode::WhileLoopLt => {
                let iter = reg_get!(base + a).as_int().ok_or_else(|| {
                    self.runtime_error(RuntimeErrorKind::TypeError {
                        operation: "wide while loop",
                        expected: "integer",
                        got: "non-integer value".to_string(),
                    })
                })?;
                let limit = reg_get!(base + a + 1).as_int().ok_or_else(|| {
                    self.runtime_error(RuntimeErrorKind::TypeError {
                        operation: "wide while loop",
                        expected: "integer",
                        got: "non-integer value".to_string(),
                    })
                })?;
                if iter < limit {
                    let offset_bits = u16::try_from(b).expect("wide immediate fits u16");
                    let offset = i16::from_ne_bytes(offset_bits.to_ne_bytes());
                    ip = (ip as isize + isize::from(offset)) as usize;
                }
            }
            OpCode::Return => {
                let result = reg_get!(base + a);
                let dest = self.frames[current_frame_idx].return_dest();
                let current_gmap = self.frames[current_frame_idx].global_mapping_id;
                let caller_gmap = if self.frames.len() > 1 {
                    self.frames[current_frame_idx - 1].global_mapping_id
                } else {
                    0
                };
                let needs_switch = current_gmap != 0 && current_gmap != caller_gmap;
                if needs_switch && caller_gmap != 0 {
                    self.sync_current_function_globals();
                }
                self.pop_frame_with_jit_metadata();
                if self.frames.is_empty() {
                    return Ok(DispatchControl::Returned(result));
                }
                return Ok(DispatchControl::ReturnToCaller {
                    destination: dest,
                    value: result,
                    switch_globals: needs_switch && caller_gmap != 0,
                });
            }
            _ => {
                self.frames[current_frame_idx].ip = ip;
                return Err(
                    self.runtime_error(RuntimeErrorKind::InvalidBytecode(format!(
                        "unsupported wide inner opcode {inner_byte}"
                    ))),
                );
            }
        }
        *instruction_pointer = ip;
        Ok(DispatchControl::Continue)
    }
}
