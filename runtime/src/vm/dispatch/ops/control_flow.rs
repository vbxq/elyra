macro_rules! execute_control_flow {
    (
        $vm:ident, $opcode_byte:ident, $instr:ident, $base:ident,
        $current_frame_idx:ident, $ip:ident, $bytecode_ptr:ident,
        $bytecode_len:ident, $reg_get:ident, $reg_ref:ident,
        $reg_set:ident, $int_value:ident
    ) => {{
        // Control flow operations: Not(17), Jump(18), JumpIf(19), JumpIfNot(20),
        // ForLoopI(40), ForLoopIInc(41), LtImm(44)-GeImm(47), WhileLoopLt(48)

        match $opcode_byte {
            // Not (17)
            17 => {
                let (a, b, _) = decode_abc($instr);
                let value = $reg_get!($base + b as usize);
                let is_falsy =
                    value.is_null() || value.as_bool() == Some(false) || value.as_int() == Some(0);
                $reg_set!($base + a as usize, Value::bool(is_falsy));
            }

            // Jump (18)
            18 => {
                let (_, imm) = decode_aimm($instr);
                $ip = ($ip as isize + imm as isize) as usize;
            }

            // JumpIf (19)
            19 => {
                let (a, imm) = decode_aimm($instr);
                let value = $reg_get!($base + a as usize);
                // Truthy: not null and not false
                if !value.is_null() && value.as_bool() != Some(false) {
                    $ip = ($ip as isize + imm as isize) as usize;
                }
            }

            // JumpIfNot (20)
            20 => {
                let (a, imm) = decode_aimm($instr);
                let value = $reg_get!($base + a as usize);
                // Falsy: null or false
                if value.is_null() || value.as_bool() == Some(false) {
                    $ip = ($ip as isize + imm as isize) as usize;
                }
            }

            26..=28 => {
                if $ip >= $bytecode_len {
                    return Err($vm.runtime_error(RuntimeErrorKind::InvalidBytecode(
                        "long jump is missing its extension word".to_string(),
                    )));
                }
                let offset = i32::from_ne_bytes(unsafe { *$bytecode_ptr.add($ip) }.to_ne_bytes());
                $ip += 1;
                let take = match $opcode_byte {
                    26 => true,
                    27 => {
                        let (register, _, _) = decode_abc($instr);
                        let value = $reg_get!($base + register as usize);
                        !value.is_null() && value.as_bool() != Some(false)
                    }
                    28 => {
                        let (register, _, _) = decode_abc($instr);
                        let value = $reg_get!($base + register as usize);
                        value.is_null() || value.as_bool() == Some(false)
                    }
                    _ => unreachable!(),
                };
                if take {
                    $ip = ($ip as isize + offset as isize) as usize;
                }
            }

            124..=125 => {
                if $ip + 1 >= $bytecode_len {
                    return Err($vm.runtime_error(RuntimeErrorKind::InvalidBytecode(
                        "wide conditional jump is missing an extension word".to_string(),
                    )));
                }
                let register_word = unsafe { *$bytecode_ptr.add($ip) };
                let offset =
                    i32::from_ne_bytes(unsafe { *$bytecode_ptr.add($ip + 1) }.to_ne_bytes());
                $ip += 2;
                let register =
                    usize::try_from(register_word >> 16).expect("wide register fits usize");
                let value = $reg_get!($base + register);
                let take = if $opcode_byte == 124 {
                    !value.is_null() && value.as_bool() != Some(false)
                } else {
                    value.is_null() || value.as_bool() == Some(false)
                };
                if take {
                    $ip = ($ip as isize + offset as isize) as usize;
                }
            }

            127 => {
                let (inner, _, _) = decode_abc($instr);
                let register_word = unsafe { *$bytecode_ptr.add($ip) };
                let offset =
                    i32::from_ne_bytes(unsafe { *$bytecode_ptr.add($ip + 1) }.to_ne_bytes());
                $ip += 2;
                let register =
                    usize::try_from(register_word >> 16).expect("wide register fits usize");
                let first = $base + register;

                match inner {
                    29 | 30 => {
                        let iter = $reg_ref!(first).as_int_unchecked();
                        let end = $reg_ref!(first + 1).as_int_unchecked();
                        let step = $reg_ref!(first + 2).as_int_unchecked();
                        let new_iter = iter.wrapping_add(step);
                        $reg_set!(first, $int_value!(new_iter));
                        let inclusive = inner == 30;
                        let should_continue = if step > 0 {
                            if inclusive {
                                new_iter <= end
                            } else {
                                new_iter < end
                            }
                        } else if inclusive {
                            new_iter >= end
                        } else {
                            new_iter > end
                        };
                        if should_continue {
                            $ip = ($ip as isize + offset as isize) as usize;
                        }
                    }
                    31 => {
                        let string_value = $reg_get!(first + 2);
                        let byte_offset = $reg_get!(first + 1).as_int().unwrap_or(0);
                        let byte_offset = usize::try_from(byte_offset).unwrap_or(0);
                        let string_ref = GcRef::new(string_value.as_ptr().unwrap_or(0));
                        let character = $vm.heap.get(string_ref).and_then(|object| {
                            if let ObjectKind::String(string) = &object.kind {
                                string
                                    .as_str()
                                    .get(byte_offset..)?
                                    .chars()
                                    .next()
                                    .map(|character| character.to_string())
                            } else {
                                None
                            }
                        });
                        if let Some(character) = character {
                            $vm.frames[$current_frame_idx].ip = $ip;
                            let character_ref = $vm.intern_string(&character)?;
                            $reg_set!(first, Value::ptr(character_ref.index()));
                            $reg_set!(
                                first + 1,
                                $int_value!(
                                    i64::try_from(byte_offset + character.len())
                                        .unwrap_or(i64::MAX)
                                )
                            );
                            $ip = ($ip as isize + offset as isize) as usize;
                        }
                    }
                    32 => {
                        let vector_value = $reg_get!(first + 2);
                        let index = $reg_get!(first + 1).as_int().unwrap_or(0);
                        let vector_ref = GcRef::new(vector_value.as_ptr().unwrap_or(0));
                        if let Ok(index) = usize::try_from(index)
                            && let Some(object) = $vm.heap.get(vector_ref)
                            && let ObjectKind::Vec(vector) = &object.kind
                            && let Some(value) = vector.get(index)
                        {
                            $reg_set!(first, value);
                            $reg_set!(
                                first + 1,
                                $int_value!(i64::try_from(index + 1).unwrap_or(i64::MAX))
                            );
                            $ip = ($ip as isize + offset as isize) as usize;
                        }
                    }
                    33 => {
                        let array_value = $reg_get!(first + 2);
                        let index = $reg_get!(first + 1).as_int().unwrap_or(0);
                        let array_ref = GcRef::new(array_value.as_ptr().unwrap_or(0));
                        if let Ok(index) = usize::try_from(index)
                            && let Some(object) = $vm.heap.get(array_ref)
                            && let ObjectKind::Array(array) = &object.kind
                            && let Some(value) = array.get(index)
                        {
                            $reg_set!(first, value);
                            $reg_set!(
                                first + 1,
                                $int_value!(i64::try_from(index + 1).unwrap_or(i64::MAX))
                            );
                            $ip = ($ip as isize + offset as isize) as usize;
                        }
                    }
                    _ => unreachable!(),
                }
            }

            29..=30 | 40..=41 => {
                let (a, imm) = if $opcode_byte <= 30 {
                    if $ip >= $bytecode_len {
                        return Err($vm.runtime_error(RuntimeErrorKind::InvalidBytecode(
                            "long integer loop is missing its extension word".to_string(),
                        )));
                    }
                    let (a, _, _) = decode_abc($instr);
                    let offset =
                        i32::from_ne_bytes(unsafe { *$bytecode_ptr.add($ip) }.to_ne_bytes());
                    $ip += 1;
                    (a, offset)
                } else {
                    let (a, offset) = decode_aimm($instr);
                    (a, i32::from(offset))
                };
                let iter_idx = $base + a as usize;

                let iter = $reg_ref!(iter_idx).as_int_unchecked();
                let end = $reg_ref!(iter_idx + 1).as_int_unchecked();
                let step = $reg_ref!(iter_idx + 2).as_int_unchecked();

                let new_iter = iter.wrapping_add(step);
                $reg_set!(iter_idx, $int_value!(new_iter));

                let inclusive = matches!($opcode_byte, 30 | 41);
                let should_continue = if step > 0 {
                    if inclusive {
                        new_iter <= end
                    } else {
                        new_iter < end
                    }
                } else {
                    if inclusive {
                        new_iter >= end
                    } else {
                        new_iter > end
                    }
                };
                if should_continue {
                    $ip = ($ip as isize + imm as isize) as usize;
                }
            }

            // LtImm (44)
            44 => {
                let (a, b, c) = decode_abc($instr);
                let l = $reg_ref!($base + b as usize).as_int_unchecked();
                $reg_set!($base + a as usize, Value::bool(l < c as i64));
            }

            // LeImm (45)
            45 => {
                let (a, b, c) = decode_abc($instr);
                let l = $reg_ref!($base + b as usize).as_int_unchecked();
                $reg_set!($base + a as usize, Value::bool(l <= c as i64));
            }

            // GtImm (46)
            46 => {
                let (a, b, c) = decode_abc($instr);
                let l = $reg_ref!($base + b as usize).as_int_unchecked();
                $reg_set!($base + a as usize, Value::bool(l > c as i64));
            }

            // GeImm (47)
            47 => {
                let (a, b, c) = decode_abc($instr);
                let l = $reg_ref!($base + b as usize).as_int_unchecked();
                $reg_set!($base + a as usize, Value::bool(l >= c as i64));
            }

            // WhileLoopLt (48)
            48 => {
                let (a, imm) = decode_aimm($instr);
                let iter = $reg_ref!($base + a as usize).as_int_unchecked();
                let limit = $reg_ref!($base + a as usize + 1).as_int_unchecked();
                if iter < limit {
                    $ip = ($ip as isize + imm as isize) as usize;
                }
            }

            31 | 177 => {
                let (a, imm) = if $opcode_byte == 31 {
                    if $ip >= $bytecode_len {
                        return Err($vm.runtime_error(RuntimeErrorKind::InvalidBytecode(
                            "long string loop is missing its extension word".to_string(),
                        )));
                    }
                    let (a, _, _) = decode_abc($instr);
                    let offset =
                        i32::from_ne_bytes(unsafe { *$bytecode_ptr.add($ip) }.to_ne_bytes());
                    $ip += 1;
                    (a, offset)
                } else {
                    let (a, offset) = decode_aimm($instr);
                    (a, i32::from(offset))
                };
                let char_idx = $base + a as usize;
                let offset_idx = char_idx + 1;
                let str_idx = char_idx + 2;

                let str_val = $reg_get!(str_idx);
                let byte_offset = $reg_get!(offset_idx).as_int().unwrap_or(0) as usize;

                let str_ref = GcRef::new(str_val.as_ptr().unwrap_or(0));
                if let Some(obj) = $vm.heap.get(str_ref)
                    && let ObjectKind::String(s) = &obj.kind
                {
                    let s_str = s.as_str();
                    if byte_offset < s_str.len() {
                        let remaining = &s_str[byte_offset..];
                        if let Some(ch) = remaining.chars().next() {
                            let char_len = ch.len_utf8();
                            let mut buf = [0u8; 4];
                            let char_str = ch.encode_utf8(&mut buf);
                            $vm.frames[$current_frame_idx].ip = $ip;
                            match $vm.intern_string(char_str) {
                                Ok(new_ref) => {
                                    $reg_set!(char_idx, Value::ptr(new_ref.index()));
                                    $reg_set!(
                                        offset_idx,
                                        $int_value!((byte_offset + char_len) as i64)
                                    );
                                    $ip = ($ip as isize + imm as isize) as usize;
                                }
                                Err(e) => return Err(e),
                            }
                        }
                        // else: invalid UTF-8, just exit loop
                    }
                    // else: byte_offset >= len, loop ends (don't jump)
                }
            }

            32 | 178 => {
                let (a, imm) = if $opcode_byte == 32 {
                    if $ip >= $bytecode_len {
                        return Err($vm.runtime_error(RuntimeErrorKind::InvalidBytecode(
                            "long vec loop is missing its extension word".to_string(),
                        )));
                    }
                    let (a, _, _) = decode_abc($instr);
                    let offset =
                        i32::from_ne_bytes(unsafe { *$bytecode_ptr.add($ip) }.to_ne_bytes());
                    $ip += 1;
                    (a, offset)
                } else {
                    let (a, offset) = decode_aimm($instr);
                    (a, i32::from(offset))
                };
                let elem_idx = $base + a as usize;
                let index_idx = elem_idx + 1;
                let vec_idx = elem_idx + 2;

                let vec_val = $reg_get!(vec_idx);
                let index = $reg_get!(index_idx).as_int().unwrap_or(0);

                let vec_ref = GcRef::new(vec_val.as_ptr().unwrap_or(0));
                if let Some(obj) = $vm.heap.get(vec_ref)
                    && let ObjectKind::Vec(v) = &obj.kind
                    && (index as usize) < v.len()
                    && let Some(elem) = v.get(index as usize)
                {
                    $reg_set!(elem_idx, elem);
                    $reg_set!(index_idx, $int_value!(index + 1));
                    $ip = ($ip as isize + imm as isize) as usize;
                }
            }

            33 | 179 => {
                let (a, imm) = if $opcode_byte == 33 {
                    if $ip >= $bytecode_len {
                        return Err($vm.runtime_error(RuntimeErrorKind::InvalidBytecode(
                            "long array loop is missing its extension word".to_string(),
                        )));
                    }
                    let (a, _, _) = decode_abc($instr);
                    let offset =
                        i32::from_ne_bytes(unsafe { *$bytecode_ptr.add($ip) }.to_ne_bytes());
                    $ip += 1;
                    (a, offset)
                } else {
                    let (a, offset) = decode_aimm($instr);
                    (a, i32::from(offset))
                };
                let elem_idx = $base + a as usize;
                let index_idx = elem_idx + 1;
                let arr_idx = elem_idx + 2;

                let arr_val = $reg_get!(arr_idx);
                let index = $reg_get!(index_idx).as_int().unwrap_or(0);

                let arr_ref = GcRef::new(arr_val.as_ptr().unwrap_or(0));
                if let Some(obj) = $vm.heap.get(arr_ref)
                    && let ObjectKind::Array(a) = &obj.kind
                    && (index as usize) < a.len()
                    && let Some(elem) = a.get(index as usize)
                {
                    $reg_set!(elem_idx, elem);
                    $reg_set!(index_idx, $int_value!(index + 1));
                    $ip = ($ip as isize + imm as isize) as usize;
                }
            }

            _ => unreachable!(),
        }
    }};
}

pub(crate) use execute_control_flow;
