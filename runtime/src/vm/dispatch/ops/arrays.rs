macro_rules! execute_arrays {
    (
        $vm:ident, $opcode_byte:ident, $instr:ident, $base:ident,
        $current_frame_idx:ident, $ip:ident, $bytecode_ptr:ident,
        $reg_get:ident, $reg_set:ident
    ) => {{
        match $opcode_byte {
            130 => {
                let (a, b, _) = decode_abc($instr);
                let dest = $base + a as usize;
                let count_value = $reg_get!($base + b as usize).as_int().unwrap_or(0);
                if count_value < 0 {
                    return Err($vm.runtime_error(RuntimeErrorKind::TypeError {
                        operation: "array allocation",
                        expected: "a non-negative size",
                        got: count_value.to_string(),
                    }));
                }
                let count = usize::try_from(count_value).map_err(|_| {
                    $vm.runtime_error(RuntimeErrorKind::TypeError {
                        operation: "array allocation",
                        expected: "a representable size",
                        got: count_value.to_string(),
                    })
                })?;
                $vm.ensure_array_capacity(aelys_bytecode::object::TypeTag::Int, count)?;
                let array = AelysArray::new_ints(count);
                $vm.frames[$current_frame_idx].ip = $ip;
                match $vm.alloc_array(array) {
                    Ok(arr_ref) => {
                        $reg_set!(dest, Value::ptr(arr_ref.index()));
                    }
                    Err(e) => return Err(e),
                }
            }

            131 => {
                let (a, b, _) = decode_abc($instr);
                let dest = $base + a as usize;
                let count_value = $reg_get!($base + b as usize).as_int().unwrap_or(0);
                if count_value < 0 {
                    return Err($vm.runtime_error(RuntimeErrorKind::TypeError {
                        operation: "array allocation",
                        expected: "a non-negative size",
                        got: count_value.to_string(),
                    }));
                }
                let count = usize::try_from(count_value).map_err(|_| {
                    $vm.runtime_error(RuntimeErrorKind::TypeError {
                        operation: "array allocation",
                        expected: "a representable size",
                        got: count_value.to_string(),
                    })
                })?;
                $vm.ensure_array_capacity(aelys_bytecode::object::TypeTag::Float, count)?;
                let array = AelysArray::new_floats(count);
                $vm.frames[$current_frame_idx].ip = $ip;
                match $vm.alloc_array(array) {
                    Ok(arr_ref) => {
                        $reg_set!(dest, Value::ptr(arr_ref.index()));
                    }
                    Err(e) => return Err(e),
                }
            }

            132 => {
                let (a, b, _) = decode_abc($instr);
                let dest = $base + a as usize;
                let count_value = $reg_get!($base + b as usize).as_int().unwrap_or(0);
                if count_value < 0 {
                    return Err($vm.runtime_error(RuntimeErrorKind::TypeError {
                        operation: "array allocation",
                        expected: "a non-negative size",
                        got: count_value.to_string(),
                    }));
                }
                let count = usize::try_from(count_value).map_err(|_| {
                    $vm.runtime_error(RuntimeErrorKind::TypeError {
                        operation: "array allocation",
                        expected: "a representable size",
                        got: count_value.to_string(),
                    })
                })?;
                $vm.ensure_array_capacity(aelys_bytecode::object::TypeTag::Bool, count)?;
                let array = AelysArray::new_bools(count);
                $vm.frames[$current_frame_idx].ip = $ip;
                match $vm.alloc_array(array) {
                    Ok(arr_ref) => {
                        $reg_set!(dest, Value::ptr(arr_ref.index()));
                    }
                    Err(e) => return Err(e),
                }
            }

            133 => {
                let (a, b, _) = decode_abc($instr);
                let dest = $base + a as usize;
                let count_value = $reg_get!($base + b as usize).as_int().unwrap_or(0);
                if count_value < 0 {
                    return Err($vm.runtime_error(RuntimeErrorKind::TypeError {
                        operation: "array allocation",
                        expected: "a non-negative size",
                        got: count_value.to_string(),
                    }));
                }
                let count = usize::try_from(count_value).map_err(|_| {
                    $vm.runtime_error(RuntimeErrorKind::TypeError {
                        operation: "array allocation",
                        expected: "a representable size",
                        got: count_value.to_string(),
                    })
                })?;
                $vm.ensure_array_capacity(aelys_bytecode::object::TypeTag::Object, count)?;
                let array = AelysArray::new_objects(count);
                $vm.frames[$current_frame_idx].ip = $ip;
                match $vm.alloc_array(array) {
                    Ok(arr_ref) => {
                        $reg_set!(dest, Value::ptr(arr_ref.index()));
                    }
                    Err(e) => return Err(e),
                }
            }

            122 | 134 => {
                let (dest, start, count) = if $opcode_byte == 122 {
                    let first = unsafe { *$bytecode_ptr.add($ip) };
                    let second = unsafe { *$bytecode_ptr.add($ip + 1) };
                    $ip += 2;
                    (
                        $base + usize::try_from(first >> 16).expect("wide destination fits usize"),
                        $base
                            + usize::try_from(first & 0xffff)
                                .expect("wide start register fits usize"),
                        usize::try_from(second >> 16).expect("wide count fits usize"),
                    )
                } else {
                    let (a, b, c) = decode_abc($instr);
                    (
                        $base + usize::from(a),
                        $base + usize::from(b),
                        usize::from(c),
                    )
                };

                $vm.frames[$current_frame_idx].ip = $ip;

                if count == 0 {
                    let array = AelysArray::new_ints(0);
                    match $vm.alloc_array(array) {
                        Ok(arr_ref) => {
                            $reg_set!(dest, Value::ptr(arr_ref.index()));
                        }
                        Err(e) => return Err(e),
                    }
                } else {
                    let first = $reg_get!(start);
                    let array = if first.is_int() {
                        let mut data = Vec::with_capacity(count);
                        for i in 0..count {
                            data.push($reg_get!(start + i).as_int().unwrap_or(0));
                        }
                        AelysArray::from_ints(data)
                    } else if first.is_float() {
                        let mut data = Vec::with_capacity(count);
                        for i in 0..count {
                            data.push($reg_get!(start + i).as_float().unwrap_or(0.0));
                        }
                        AelysArray::from_floats(data)
                    } else if first.is_bool() {
                        let mut data = Vec::with_capacity(count);
                        for i in 0..count {
                            data.push($reg_get!(start + i).as_bool().unwrap_or(false));
                        }
                        AelysArray::from_bools(data)
                    } else {
                        let mut data = Vec::with_capacity(count);
                        for i in 0..count {
                            data.push($reg_get!(start + i));
                        }
                        AelysArray::from_objects(data)
                    };
                    match $vm.alloc_array(array) {
                        Ok(arr_ref) => {
                            $reg_set!(dest, Value::ptr(arr_ref.index()));
                        }
                        Err(e) => return Err(e),
                    }
                }
            }

            135 => {
                let (a, b, c) = decode_abc($instr);
                let dest = $base + a as usize;
                let arr_val = $reg_get!($base + b as usize);
                let idx = $reg_get!($base + c as usize).as_int().unwrap_or(-1);

                if idx < 0 {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    return Err($vm.runtime_error(RuntimeErrorKind::IndexOutOfBounds {
                        index: idx,
                        length: 0,
                    }));
                }

                let arr_ref = GcRef::new(arr_val.as_ptr().unwrap_or(0));
                if let Some(obj) = $vm.heap.get(arr_ref) {
                    if let ObjectKind::Array(arr) = &obj.kind {
                        if let Some(val) = arr.get(idx as usize) {
                            $reg_set!(dest, val);
                        } else {
                            $vm.frames[$current_frame_idx].ip = $ip;
                            return Err($vm.runtime_error(RuntimeErrorKind::IndexOutOfBounds {
                                index: idx,
                                length: arr.len() as i64,
                            }));
                        }
                    } else {
                        $vm.frames[$current_frame_idx].ip = $ip;
                        return Err($vm.runtime_error(RuntimeErrorKind::TypeError {
                            operation: "array load",
                            expected: "array",
                            got: "non-array object".to_string(),
                        }));
                    }
                } else {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    return Err($vm.runtime_error(RuntimeErrorKind::InvalidMemoryHandle));
                }
            }

            136 => {
                let (a, b, c) = decode_abc($instr);
                let dest = $base + a as usize;
                let arr_val = $reg_get!($base + b as usize);
                let idx = $reg_get!($base + c as usize).as_int().unwrap_or(-1);

                if idx < 0 {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    return Err($vm.runtime_error(RuntimeErrorKind::IndexOutOfBounds {
                        index: idx,
                        length: 0,
                    }));
                }

                let arr_ref = GcRef::new(arr_val.as_ptr().unwrap_or(0));
                if let Some(obj) = $vm.heap.get(arr_ref) {
                    if let ObjectKind::Array(arr) = &obj.kind {
                        if let Some(val) = arr.get(idx as usize) {
                            $reg_set!(dest, val);
                        } else {
                            $vm.frames[$current_frame_idx].ip = $ip;
                            return Err($vm.runtime_error(RuntimeErrorKind::IndexOutOfBounds {
                                index: idx,
                                length: arr.len() as i64,
                            }));
                        }
                    } else {
                        $vm.frames[$current_frame_idx].ip = $ip;
                        return Err($vm.runtime_error(RuntimeErrorKind::TypeError {
                            operation: "array load",
                            expected: "array",
                            got: "non-array object".to_string(),
                        }));
                    }
                } else {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    return Err($vm.runtime_error(RuntimeErrorKind::InvalidMemoryHandle));
                }
            }

            137 => {
                let (a, b, c) = decode_abc($instr);
                let dest = $base + a as usize;
                let arr_val = $reg_get!($base + b as usize);
                let idx = $reg_get!($base + c as usize).as_int().unwrap_or(-1);

                if idx < 0 {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    return Err($vm.runtime_error(RuntimeErrorKind::IndexOutOfBounds {
                        index: idx,
                        length: 0,
                    }));
                }

                let arr_ref = GcRef::new(arr_val.as_ptr().unwrap_or(0));
                if let Some(obj) = $vm.heap.get(arr_ref) {
                    if let ObjectKind::Array(arr) = &obj.kind {
                        if let Some(val) = arr.get(idx as usize) {
                            $reg_set!(dest, val);
                        } else {
                            $vm.frames[$current_frame_idx].ip = $ip;
                            return Err($vm.runtime_error(RuntimeErrorKind::IndexOutOfBounds {
                                index: idx,
                                length: arr.len() as i64,
                            }));
                        }
                    } else {
                        $vm.frames[$current_frame_idx].ip = $ip;
                        return Err($vm.runtime_error(RuntimeErrorKind::TypeError {
                            operation: "array load",
                            expected: "array",
                            got: "non-array object".to_string(),
                        }));
                    }
                } else {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    return Err($vm.runtime_error(RuntimeErrorKind::InvalidMemoryHandle));
                }
            }

            138 => {
                let (a, b, c) = decode_abc($instr);
                let dest = $base + a as usize;
                let arr_val = $reg_get!($base + b as usize);
                let idx = $reg_get!($base + c as usize).as_int().unwrap_or(-1);

                if idx < 0 {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    return Err($vm.runtime_error(RuntimeErrorKind::IndexOutOfBounds {
                        index: idx,
                        length: 0,
                    }));
                }

                let arr_ref = GcRef::new(arr_val.as_ptr().unwrap_or(0));
                if let Some(obj) = $vm.heap.get(arr_ref) {
                    if let ObjectKind::Array(arr) = &obj.kind {
                        if let Some(val) = arr.get(idx as usize) {
                            $reg_set!(dest, val);
                        } else {
                            $vm.frames[$current_frame_idx].ip = $ip;
                            return Err($vm.runtime_error(RuntimeErrorKind::IndexOutOfBounds {
                                index: idx,
                                length: arr.len() as i64,
                            }));
                        }
                    } else {
                        $vm.frames[$current_frame_idx].ip = $ip;
                        return Err($vm.runtime_error(RuntimeErrorKind::TypeError {
                            operation: "array load",
                            expected: "array",
                            got: "non-array object".to_string(),
                        }));
                    }
                } else {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    return Err($vm.runtime_error(RuntimeErrorKind::InvalidMemoryHandle));
                }
            }

            139 => {
                let (a, b, c) = decode_abc($instr);
                let dest = $base + a as usize;
                let arr_val = $reg_get!($base + b as usize);
                let idx = $reg_get!($base + c as usize).as_int().unwrap_or(-1);
                let value = $vm.array_get_option(arr_val, idx)?;
                $reg_set!(dest, value);
            }

            140 => {
                let (a, b, c) = decode_abc($instr);
                let dest = $base + a as usize;
                let arr_val = $reg_get!($base + b as usize);
                let idx = $reg_get!($base + c as usize).as_int().unwrap_or(-1);
                let value = $vm.array_get_option(arr_val, idx)?;
                $reg_set!(dest, value);
            }

            141 => {
                let (a, b, c) = decode_abc($instr);
                let dest = $base + a as usize;
                let arr_val = $reg_get!($base + b as usize);
                let idx = $reg_get!($base + c as usize).as_int().unwrap_or(-1);
                let value = $vm.array_get_option(arr_val, idx)?;
                $reg_set!(dest, value);
            }

            142 => {
                let (a, b, c) = decode_abc($instr);
                let dest = $base + a as usize;
                let arr_val = $reg_get!($base + b as usize);
                let idx = $reg_get!($base + c as usize).as_int().unwrap_or(-1);
                let value = $vm.array_get_option(arr_val, idx)?;
                $reg_set!(dest, value);
            }

            143 => {
                let (a, b, c) = decode_abc($instr);
                let arr_val = $reg_get!($base + a as usize);
                let idx = $reg_get!($base + b as usize).as_int().unwrap_or(-1);
                let value = $reg_get!($base + c as usize);

                if idx < 0 {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    return Err($vm.runtime_error(RuntimeErrorKind::IndexOutOfBounds {
                        index: idx,
                        length: 0,
                    }));
                }

                let arr_ref = GcRef::new(arr_val.as_ptr().unwrap_or(0));
                let arr_len = $vm.heap.get(arr_ref).and_then(|obj| {
                    if let ObjectKind::Array(arr) = &obj.kind {
                        Some(arr.len() as i64)
                    } else {
                        None
                    }
                });

                match arr_len {
                    Some(len) => {
                        if let Some(obj) = $vm.heap.get_mut(arr_ref)
                            && let ObjectKind::Array(arr) = &mut obj.kind
                            && !arr.set(idx as usize, value)
                        {
                            $vm.frames[$current_frame_idx].ip = $ip;
                            return Err($vm.runtime_error(RuntimeErrorKind::IndexOutOfBounds {
                                index: idx,
                                length: len,
                            }));
                        }
                    }
                    None => {
                        if $vm.heap.get(arr_ref).is_some() {
                            $vm.frames[$current_frame_idx].ip = $ip;
                            return Err($vm.runtime_error(RuntimeErrorKind::TypeError {
                                operation: "array store",
                                expected: "array",
                                got: "non-array object".to_string(),
                            }));
                        } else {
                            $vm.frames[$current_frame_idx].ip = $ip;
                            return Err($vm.runtime_error(RuntimeErrorKind::InvalidMemoryHandle));
                        }
                    }
                }
            }

            144 => {
                let (a, b, c) = decode_abc($instr);
                let arr_val = $reg_get!($base + a as usize);
                let idx = $reg_get!($base + b as usize).as_int().unwrap_or(-1);
                let value = $reg_get!($base + c as usize);

                if idx < 0 {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    return Err($vm.runtime_error(RuntimeErrorKind::IndexOutOfBounds {
                        index: idx,
                        length: 0,
                    }));
                }

                let arr_ref = GcRef::new(arr_val.as_ptr().unwrap_or(0));
                let arr_len = $vm.heap.get(arr_ref).and_then(|obj| {
                    if let ObjectKind::Array(arr) = &obj.kind {
                        Some(arr.len() as i64)
                    } else {
                        None
                    }
                });

                match arr_len {
                    Some(len) => {
                        if let Some(obj) = $vm.heap.get_mut(arr_ref)
                            && let ObjectKind::Array(arr) = &mut obj.kind
                            && !arr.set(idx as usize, value)
                        {
                            $vm.frames[$current_frame_idx].ip = $ip;
                            return Err($vm.runtime_error(RuntimeErrorKind::IndexOutOfBounds {
                                index: idx,
                                length: len,
                            }));
                        }
                    }
                    None => {
                        if $vm.heap.get(arr_ref).is_some() {
                            $vm.frames[$current_frame_idx].ip = $ip;
                            return Err($vm.runtime_error(RuntimeErrorKind::TypeError {
                                operation: "array store",
                                expected: "array",
                                got: "non-array object".to_string(),
                            }));
                        } else {
                            $vm.frames[$current_frame_idx].ip = $ip;
                            return Err($vm.runtime_error(RuntimeErrorKind::InvalidMemoryHandle));
                        }
                    }
                }
            }

            145 => {
                let (a, b, c) = decode_abc($instr);
                let arr_val = $reg_get!($base + a as usize);
                let idx = $reg_get!($base + b as usize).as_int().unwrap_or(-1);
                let value = $reg_get!($base + c as usize);

                if idx < 0 {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    return Err($vm.runtime_error(RuntimeErrorKind::IndexOutOfBounds {
                        index: idx,
                        length: 0,
                    }));
                }

                let arr_ref = GcRef::new(arr_val.as_ptr().unwrap_or(0));
                let arr_len = $vm.heap.get(arr_ref).and_then(|obj| {
                    if let ObjectKind::Array(arr) = &obj.kind {
                        Some(arr.len() as i64)
                    } else {
                        None
                    }
                });

                match arr_len {
                    Some(len) => {
                        if let Some(obj) = $vm.heap.get_mut(arr_ref)
                            && let ObjectKind::Array(arr) = &mut obj.kind
                            && !arr.set(idx as usize, value)
                        {
                            $vm.frames[$current_frame_idx].ip = $ip;
                            return Err($vm.runtime_error(RuntimeErrorKind::IndexOutOfBounds {
                                index: idx,
                                length: len,
                            }));
                        }
                    }
                    None => {
                        if $vm.heap.get(arr_ref).is_some() {
                            $vm.frames[$current_frame_idx].ip = $ip;
                            return Err($vm.runtime_error(RuntimeErrorKind::TypeError {
                                operation: "array store",
                                expected: "array",
                                got: "non-array object".to_string(),
                            }));
                        } else {
                            $vm.frames[$current_frame_idx].ip = $ip;
                            return Err($vm.runtime_error(RuntimeErrorKind::InvalidMemoryHandle));
                        }
                    }
                }
            }

            146 => {
                let (a, b, c) = decode_abc($instr);
                let arr_val = $reg_get!($base + a as usize);
                let idx = $reg_get!($base + b as usize).as_int().unwrap_or(-1);
                let value = $reg_get!($base + c as usize);

                if idx < 0 {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    return Err($vm.runtime_error(RuntimeErrorKind::IndexOutOfBounds {
                        index: idx,
                        length: 0,
                    }));
                }

                let arr_ref = GcRef::new(arr_val.as_ptr().unwrap_or(0));
                let arr_len = $vm.heap.get(arr_ref).and_then(|obj| {
                    if let ObjectKind::Array(arr) = &obj.kind {
                        Some(arr.len() as i64)
                    } else {
                        None
                    }
                });

                match arr_len {
                    Some(len) => {
                        if let Some(obj) = $vm.heap.get_mut(arr_ref)
                            && let ObjectKind::Array(arr) = &mut obj.kind
                            && !arr.set(idx as usize, value)
                        {
                            $vm.frames[$current_frame_idx].ip = $ip;
                            return Err($vm.runtime_error(RuntimeErrorKind::IndexOutOfBounds {
                                index: idx,
                                length: len,
                            }));
                        }
                    }
                    None => {
                        if $vm.heap.get(arr_ref).is_some() {
                            $vm.frames[$current_frame_idx].ip = $ip;
                            return Err($vm.runtime_error(RuntimeErrorKind::TypeError {
                                operation: "array store",
                                expected: "array",
                                got: "non-array object".to_string(),
                            }));
                        } else {
                            $vm.frames[$current_frame_idx].ip = $ip;
                            return Err($vm.runtime_error(RuntimeErrorKind::InvalidMemoryHandle));
                        }
                    }
                }
            }

            147 => {
                let (a, b, _) = decode_abc($instr);
                let dest = $base + a as usize;
                let arr_val = $reg_get!($base + b as usize);

                let arr_ref = GcRef::new(arr_val.as_ptr().unwrap_or(0));
                if let Some(obj) = $vm.heap.get(arr_ref) {
                    if let ObjectKind::Array(arr) = &obj.kind {
                        $reg_set!(dest, Value::int(arr.len() as i64));
                    } else if let ObjectKind::Vec(vec) = &obj.kind {
                        $reg_set!(dest, Value::int(vec.len() as i64));
                    } else {
                        $vm.frames[$current_frame_idx].ip = $ip;
                        return Err($vm.runtime_error(RuntimeErrorKind::TypeError {
                            operation: "array len",
                            expected: "array or vec",
                            got: "non-array object".to_string(),
                        }));
                    }
                } else {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    return Err($vm.runtime_error(RuntimeErrorKind::InvalidMemoryHandle));
                }
            }

            148 => {
                let (a, _, _) = decode_abc($instr);
                let dest = $base + a as usize;
                let vec = AelysVec::new_ints();
                $vm.frames[$current_frame_idx].ip = $ip;
                match $vm.alloc_vec(vec) {
                    Ok(vec_ref) => {
                        $reg_set!(dest, Value::ptr(vec_ref.index()));
                    }
                    Err(e) => return Err(e),
                }
            }

            149 => {
                let (a, _, _) = decode_abc($instr);
                let dest = $base + a as usize;
                let vec = AelysVec::new_floats();
                $vm.frames[$current_frame_idx].ip = $ip;
                match $vm.alloc_vec(vec) {
                    Ok(vec_ref) => {
                        $reg_set!(dest, Value::ptr(vec_ref.index()));
                    }
                    Err(e) => return Err(e),
                }
            }

            150 => {
                let (a, _, _) = decode_abc($instr);
                let dest = $base + a as usize;
                let vec = AelysVec::new_bools();
                $vm.frames[$current_frame_idx].ip = $ip;
                match $vm.alloc_vec(vec) {
                    Ok(vec_ref) => {
                        $reg_set!(dest, Value::ptr(vec_ref.index()));
                    }
                    Err(e) => return Err(e),
                }
            }

            151 => {
                let (a, _, _) = decode_abc($instr);
                let dest = $base + a as usize;
                let vec = AelysVec::new_objects();
                $vm.frames[$current_frame_idx].ip = $ip;
                match $vm.alloc_vec(vec) {
                    Ok(vec_ref) => {
                        $reg_set!(dest, Value::ptr(vec_ref.index()));
                    }
                    Err(e) => return Err(e),
                }
            }

            123 | 152 => {
                let (dest, start, count) = if $opcode_byte == 123 {
                    let first = unsafe { *$bytecode_ptr.add($ip) };
                    let second = unsafe { *$bytecode_ptr.add($ip + 1) };
                    $ip += 2;
                    (
                        $base + usize::try_from(first >> 16).expect("wide destination fits usize"),
                        $base
                            + usize::try_from(first & 0xffff)
                                .expect("wide start register fits usize"),
                        usize::try_from(second >> 16).expect("wide count fits usize"),
                    )
                } else {
                    let (a, b, c) = decode_abc($instr);
                    (
                        $base + usize::from(a),
                        $base + usize::from(b),
                        usize::from(c),
                    )
                };

                $vm.frames[$current_frame_idx].ip = $ip;

                if count == 0 {
                    let vec = AelysVec::new_ints();
                    match $vm.alloc_vec(vec) {
                        Ok(vec_ref) => {
                            $reg_set!(dest, Value::ptr(vec_ref.index()));
                        }
                        Err(e) => return Err(e),
                    }
                } else {
                    let first = $reg_get!(start);
                    let vec = if first.is_int() {
                        let mut data = Vec::with_capacity(count);
                        for i in 0..count {
                            data.push($reg_get!(start + i).as_int().unwrap_or(0));
                        }
                        AelysVec::from_ints(data)
                    } else if first.is_float() {
                        let mut data = Vec::with_capacity(count);
                        for i in 0..count {
                            data.push($reg_get!(start + i).as_float().unwrap_or(0.0));
                        }
                        AelysVec::from_floats(data)
                    } else if first.is_bool() {
                        let mut data = Vec::with_capacity(count);
                        for i in 0..count {
                            data.push($reg_get!(start + i).as_bool().unwrap_or(false));
                        }
                        AelysVec::from_bools(data)
                    } else {
                        let mut data = Vec::with_capacity(count);
                        for i in 0..count {
                            data.push($reg_get!(start + i));
                        }
                        AelysVec::from_objects(data)
                    };
                    match $vm.alloc_vec(vec) {
                        Ok(vec_ref) => {
                            $reg_set!(dest, Value::ptr(vec_ref.index()));
                        }
                        Err(e) => return Err(e),
                    }
                }
            }

            153 => {
                let (a, b, _) = decode_abc($instr);
                let vec_val = $reg_get!($base + a as usize);
                let value = $reg_get!($base + b as usize);
                let vec_ref = GcRef::new(vec_val.as_ptr().unwrap_or(0));
                if let Err(error) = $vm.push_vec_value(vec_ref, value) {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    return Err(error);
                }
            }

            154 => {
                let (a, b, _) = decode_abc($instr);
                let vec_val = $reg_get!($base + a as usize);
                let value = $reg_get!($base + b as usize);
                let vec_ref = GcRef::new(vec_val.as_ptr().unwrap_or(0));
                if let Err(error) = $vm.push_vec_value(vec_ref, value) {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    return Err(error);
                }
            }

            155 => {
                let (a, b, _) = decode_abc($instr);
                let vec_val = $reg_get!($base + a as usize);
                let value = $reg_get!($base + b as usize);
                let vec_ref = GcRef::new(vec_val.as_ptr().unwrap_or(0));
                if let Err(error) = $vm.push_vec_value(vec_ref, value) {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    return Err(error);
                }
            }

            156 => {
                let (a, b, _) = decode_abc($instr);
                let vec_val = $reg_get!($base + a as usize);
                let value = $reg_get!($base + b as usize);
                let vec_ref = GcRef::new(vec_val.as_ptr().unwrap_or(0));
                if let Err(error) = $vm.push_vec_value(vec_ref, value) {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    return Err(error);
                }
            }

            157 => {
                let (a, b, _) = decode_abc($instr);
                let dest = $base + a as usize;
                let vec_val = $reg_get!($base + b as usize);

                let val = $vm.pop_vec_option(vec_val)?;
                $reg_set!(dest, val);
            }

            158 => {
                let (a, b, _) = decode_abc($instr);
                let dest = $base + a as usize;
                let vec_val = $reg_get!($base + b as usize);

                let val = $vm.pop_vec_option(vec_val)?;
                $reg_set!(dest, val);
            }

            159 => {
                let (a, b, _) = decode_abc($instr);
                let dest = $base + a as usize;
                let vec_val = $reg_get!($base + b as usize);

                let val = $vm.pop_vec_option(vec_val)?;
                $reg_set!(dest, val);
            }

            160 => {
                let (a, b, _) = decode_abc($instr);
                let dest = $base + a as usize;
                let vec_val = $reg_get!($base + b as usize);

                let val = $vm.pop_vec_option(vec_val)?;
                $reg_set!(dest, val);
            }

            161 => {
                let (a, b, _) = decode_abc($instr);
                let dest = $base + a as usize;
                let vec_val = $reg_get!($base + b as usize);

                let vec_ref = GcRef::new(vec_val.as_ptr().unwrap_or(0));
                if let Some(obj) = $vm.heap.get(vec_ref) {
                    match &obj.kind {
                        ObjectKind::Vec(vec) => {
                            $reg_set!(dest, Value::int(vec.len() as i64));
                        }
                        ObjectKind::Array(arr) => {
                            $reg_set!(dest, Value::int(arr.len() as i64));
                        }
                        ObjectKind::String(s) => {
                            $reg_set!(dest, Value::int(s.len() as i64));
                        }
                        _ => {
                            $vm.frames[$current_frame_idx].ip = $ip;
                            return Err($vm.runtime_error(RuntimeErrorKind::TypeError {
                                operation: "len",
                                expected: "vec, array, or string",
                                got: "non-collection object".to_string(),
                            }));
                        }
                    }
                } else {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    return Err($vm.runtime_error(RuntimeErrorKind::InvalidMemoryHandle));
                }
            }

            162 => {
                let (a, b, _) = decode_abc($instr);
                let dest = $base + a as usize;
                let vec_val = $reg_get!($base + b as usize);

                let vec_ref = GcRef::new(vec_val.as_ptr().unwrap_or(0));
                if let Some(obj) = $vm.heap.get(vec_ref) {
                    if let ObjectKind::Vec(vec) = &obj.kind {
                        $reg_set!(dest, Value::int(vec.capacity() as i64));
                    } else {
                        $vm.frames[$current_frame_idx].ip = $ip;
                        return Err($vm.runtime_error(RuntimeErrorKind::TypeError {
                            operation: "vec capacity",
                            expected: "vec",
                            got: "non-vec object".to_string(),
                        }));
                    }
                } else {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    return Err($vm.runtime_error(RuntimeErrorKind::InvalidMemoryHandle));
                }
            }

            163 => {
                let (a, b, _) = decode_abc($instr);
                let vec_val = $reg_get!($base + a as usize);
                let additional_value = $reg_get!($base + b as usize).as_int().unwrap_or(0);
                if additional_value < 0 {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    return Err($vm.runtime_error(RuntimeErrorKind::TypeError {
                        operation: "vec reserve",
                        expected: "a non-negative amount",
                        got: additional_value.to_string(),
                    }));
                }
                let additional = usize::try_from(additional_value).map_err(|_| {
                    $vm.runtime_error(RuntimeErrorKind::TypeError {
                        operation: "vec reserve",
                        expected: "a representable amount",
                        got: additional_value.to_string(),
                    })
                })?;
                let vec_ref = GcRef::new(vec_val.as_ptr().unwrap_or(0));
                if let Err(error) = $vm.reserve_vec(vec_ref, additional) {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    return Err(error);
                }
            }

            164 => {
                let (a, b, c) = decode_abc($instr);
                let dest = $base + a as usize;
                let vec_val = $reg_get!($base + b as usize);
                let idx = $reg_get!($base + c as usize).as_int().unwrap_or(-1);

                if idx < 0 {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    return Err($vm.runtime_error(RuntimeErrorKind::IndexOutOfBounds {
                        index: idx,
                        length: 0,
                    }));
                }

                let vec_ref = GcRef::new(vec_val.as_ptr().unwrap_or(0));
                if let Some(obj) = $vm.heap.get(vec_ref) {
                    if let ObjectKind::Vec(vec) = &obj.kind {
                        if let Some(val) = vec.get(idx as usize) {
                            $reg_set!(dest, val);
                        } else {
                            $vm.frames[$current_frame_idx].ip = $ip;
                            return Err($vm.runtime_error(RuntimeErrorKind::IndexOutOfBounds {
                                index: idx,
                                length: vec.len() as i64,
                            }));
                        }
                    } else {
                        $vm.frames[$current_frame_idx].ip = $ip;
                        return Err($vm.runtime_error(RuntimeErrorKind::TypeError {
                            operation: "vec load",
                            expected: "vec",
                            got: "non-vec object".to_string(),
                        }));
                    }
                } else {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    return Err($vm.runtime_error(RuntimeErrorKind::InvalidMemoryHandle));
                }
            }

            165 => {
                let (a, b, c) = decode_abc($instr);
                let dest = $base + a as usize;
                let vec_val = $reg_get!($base + b as usize);
                let idx = $reg_get!($base + c as usize).as_int().unwrap_or(-1);

                if idx < 0 {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    return Err($vm.runtime_error(RuntimeErrorKind::IndexOutOfBounds {
                        index: idx,
                        length: 0,
                    }));
                }

                let vec_ref = GcRef::new(vec_val.as_ptr().unwrap_or(0));
                if let Some(obj) = $vm.heap.get(vec_ref) {
                    if let ObjectKind::Vec(vec) = &obj.kind {
                        if let Some(val) = vec.get(idx as usize) {
                            $reg_set!(dest, val);
                        } else {
                            $vm.frames[$current_frame_idx].ip = $ip;
                            return Err($vm.runtime_error(RuntimeErrorKind::IndexOutOfBounds {
                                index: idx,
                                length: vec.len() as i64,
                            }));
                        }
                    } else {
                        $vm.frames[$current_frame_idx].ip = $ip;
                        return Err($vm.runtime_error(RuntimeErrorKind::TypeError {
                            operation: "vec load",
                            expected: "vec",
                            got: "non-vec object".to_string(),
                        }));
                    }
                } else {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    return Err($vm.runtime_error(RuntimeErrorKind::InvalidMemoryHandle));
                }
            }

            166 => {
                let (a, b, c) = decode_abc($instr);
                let dest = $base + a as usize;
                let vec_val = $reg_get!($base + b as usize);
                let idx = $reg_get!($base + c as usize).as_int().unwrap_or(-1);

                if idx < 0 {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    return Err($vm.runtime_error(RuntimeErrorKind::IndexOutOfBounds {
                        index: idx,
                        length: 0,
                    }));
                }

                let vec_ref = GcRef::new(vec_val.as_ptr().unwrap_or(0));
                if let Some(obj) = $vm.heap.get(vec_ref) {
                    if let ObjectKind::Vec(vec) = &obj.kind {
                        if let Some(val) = vec.get(idx as usize) {
                            $reg_set!(dest, val);
                        } else {
                            $vm.frames[$current_frame_idx].ip = $ip;
                            return Err($vm.runtime_error(RuntimeErrorKind::IndexOutOfBounds {
                                index: idx,
                                length: vec.len() as i64,
                            }));
                        }
                    } else {
                        $vm.frames[$current_frame_idx].ip = $ip;
                        return Err($vm.runtime_error(RuntimeErrorKind::TypeError {
                            operation: "vec load",
                            expected: "vec",
                            got: "non-vec object".to_string(),
                        }));
                    }
                } else {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    return Err($vm.runtime_error(RuntimeErrorKind::InvalidMemoryHandle));
                }
            }

            167 => {
                let (a, b, c) = decode_abc($instr);
                let dest = $base + a as usize;
                let vec_val = $reg_get!($base + b as usize);
                let idx = $reg_get!($base + c as usize).as_int().unwrap_or(-1);

                if idx < 0 {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    return Err($vm.runtime_error(RuntimeErrorKind::IndexOutOfBounds {
                        index: idx,
                        length: 0,
                    }));
                }

                let vec_ref = GcRef::new(vec_val.as_ptr().unwrap_or(0));
                if let Some(obj) = $vm.heap.get(vec_ref) {
                    if let ObjectKind::Vec(vec) = &obj.kind {
                        if let Some(val) = vec.get(idx as usize) {
                            $reg_set!(dest, val);
                        } else {
                            $vm.frames[$current_frame_idx].ip = $ip;
                            return Err($vm.runtime_error(RuntimeErrorKind::IndexOutOfBounds {
                                index: idx,
                                length: vec.len() as i64,
                            }));
                        }
                    } else if let ObjectKind::Array(arr) = &obj.kind {
                        if let Some(val) = arr.get(idx as usize) {
                            $reg_set!(dest, val);
                        } else {
                            $vm.frames[$current_frame_idx].ip = $ip;
                            return Err($vm.runtime_error(RuntimeErrorKind::IndexOutOfBounds {
                                index: idx,
                                length: arr.len() as i64,
                            }));
                        }
                    } else if let ObjectKind::String(s) = &obj.kind {
                        let s_clone = s.as_str().to_string();
                        if let Some(ch) = s_clone.chars().nth(idx as usize) {
                            let mut buf = [0u8; 4];
                            let char_str = ch.encode_utf8(&mut buf);
                            $vm.frames[$current_frame_idx].ip = $ip;
                            match $vm.intern_string(char_str) {
                                Ok(str_ref) => {
                                    $reg_set!(dest, Value::ptr(str_ref.index()));
                                }
                                Err(e) => return Err(e),
                            }
                        } else {
                            $vm.frames[$current_frame_idx].ip = $ip;
                            return Err($vm.runtime_error(RuntimeErrorKind::IndexOutOfBounds {
                                index: idx,
                                length: s_clone.chars().count() as i64,
                            }));
                        }
                    } else {
                        $vm.frames[$current_frame_idx].ip = $ip;
                        return Err($vm.runtime_error(RuntimeErrorKind::TypeError {
                            operation: "vec load",
                            expected: "vec, array, or string",
                            got: "non-vec object".to_string(),
                        }));
                    }
                } else {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    return Err($vm.runtime_error(RuntimeErrorKind::InvalidMemoryHandle));
                }
            }

            168 => {
                let (a, b, c) = decode_abc($instr);
                let dest = $base + a as usize;
                let vec_val = $reg_get!($base + b as usize);
                let idx = $reg_get!($base + c as usize).as_int().unwrap_or(-1);
                let value = $vm.vec_get_option(vec_val, idx)?;
                $reg_set!(dest, value);
            }

            169 => {
                let (a, b, c) = decode_abc($instr);
                let dest = $base + a as usize;
                let vec_val = $reg_get!($base + b as usize);
                let idx = $reg_get!($base + c as usize).as_int().unwrap_or(-1);
                let value = $vm.vec_get_option(vec_val, idx)?;
                $reg_set!(dest, value);
            }

            170 => {
                let (a, b, c) = decode_abc($instr);
                let dest = $base + a as usize;
                let vec_val = $reg_get!($base + b as usize);
                let idx = $reg_get!($base + c as usize).as_int().unwrap_or(-1);
                let value = $vm.vec_get_option(vec_val, idx)?;
                $reg_set!(dest, value);
            }

            171 => {
                let (a, b, c) = decode_abc($instr);
                let dest = $base + a as usize;
                let vec_val = $reg_get!($base + b as usize);
                let idx = $reg_get!($base + c as usize).as_int().unwrap_or(-1);
                let value = $vm.vec_get_option(vec_val, idx)?;
                $reg_set!(dest, value);
            }

            172 => {
                let (a, b, c) = decode_abc($instr);
                let vec_val = $reg_get!($base + a as usize);
                let idx = $reg_get!($base + b as usize).as_int().unwrap_or(-1);
                let value = $reg_get!($base + c as usize);

                if idx < 0 {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    return Err($vm.runtime_error(RuntimeErrorKind::IndexOutOfBounds {
                        index: idx,
                        length: 0,
                    }));
                }

                let vec_ref = GcRef::new(vec_val.as_ptr().unwrap_or(0));
                let vec_len = $vm.heap.get(vec_ref).and_then(|obj| {
                    if let ObjectKind::Vec(vec) = &obj.kind {
                        Some(vec.len() as i64)
                    } else {
                        None
                    }
                });

                match vec_len {
                    Some(len) => {
                        if let Some(obj) = $vm.heap.get_mut(vec_ref)
                            && let ObjectKind::Vec(vec) = &mut obj.kind
                            && !vec.set(idx as usize, value)
                        {
                            $vm.frames[$current_frame_idx].ip = $ip;
                            return Err($vm.runtime_error(RuntimeErrorKind::IndexOutOfBounds {
                                index: idx,
                                length: len,
                            }));
                        }
                    }
                    None => {
                        if $vm.heap.get(vec_ref).is_some() {
                            $vm.frames[$current_frame_idx].ip = $ip;
                            return Err($vm.runtime_error(RuntimeErrorKind::TypeError {
                                operation: "vec store",
                                expected: "vec",
                                got: "non-vec object".to_string(),
                            }));
                        } else {
                            $vm.frames[$current_frame_idx].ip = $ip;
                            return Err($vm.runtime_error(RuntimeErrorKind::InvalidMemoryHandle));
                        }
                    }
                }
            }

            173 => {
                let (a, b, c) = decode_abc($instr);
                let vec_val = $reg_get!($base + a as usize);
                let idx = $reg_get!($base + b as usize).as_int().unwrap_or(-1);
                let value = $reg_get!($base + c as usize);

                if idx < 0 {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    return Err($vm.runtime_error(RuntimeErrorKind::IndexOutOfBounds {
                        index: idx,
                        length: 0,
                    }));
                }

                let vec_ref = GcRef::new(vec_val.as_ptr().unwrap_or(0));
                let vec_len = $vm.heap.get(vec_ref).and_then(|obj| {
                    if let ObjectKind::Vec(vec) = &obj.kind {
                        Some(vec.len() as i64)
                    } else {
                        None
                    }
                });

                match vec_len {
                    Some(len) => {
                        if let Some(obj) = $vm.heap.get_mut(vec_ref)
                            && let ObjectKind::Vec(vec) = &mut obj.kind
                            && !vec.set(idx as usize, value)
                        {
                            $vm.frames[$current_frame_idx].ip = $ip;
                            return Err($vm.runtime_error(RuntimeErrorKind::IndexOutOfBounds {
                                index: idx,
                                length: len,
                            }));
                        }
                    }
                    None => {
                        if $vm.heap.get(vec_ref).is_some() {
                            $vm.frames[$current_frame_idx].ip = $ip;
                            return Err($vm.runtime_error(RuntimeErrorKind::TypeError {
                                operation: "vec store",
                                expected: "vec",
                                got: "non-vec object".to_string(),
                            }));
                        } else {
                            $vm.frames[$current_frame_idx].ip = $ip;
                            return Err($vm.runtime_error(RuntimeErrorKind::InvalidMemoryHandle));
                        }
                    }
                }
            }

            174 => {
                let (a, b, c) = decode_abc($instr);
                let vec_val = $reg_get!($base + a as usize);
                let idx = $reg_get!($base + b as usize).as_int().unwrap_or(-1);
                let value = $reg_get!($base + c as usize);

                if idx < 0 {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    return Err($vm.runtime_error(RuntimeErrorKind::IndexOutOfBounds {
                        index: idx,
                        length: 0,
                    }));
                }

                let vec_ref = GcRef::new(vec_val.as_ptr().unwrap_or(0));
                let vec_len = $vm.heap.get(vec_ref).and_then(|obj| {
                    if let ObjectKind::Vec(vec) = &obj.kind {
                        Some(vec.len() as i64)
                    } else {
                        None
                    }
                });

                match vec_len {
                    Some(len) => {
                        if let Some(obj) = $vm.heap.get_mut(vec_ref)
                            && let ObjectKind::Vec(vec) = &mut obj.kind
                            && !vec.set(idx as usize, value)
                        {
                            $vm.frames[$current_frame_idx].ip = $ip;
                            return Err($vm.runtime_error(RuntimeErrorKind::IndexOutOfBounds {
                                index: idx,
                                length: len,
                            }));
                        }
                    }
                    None => {
                        if $vm.heap.get(vec_ref).is_some() {
                            $vm.frames[$current_frame_idx].ip = $ip;
                            return Err($vm.runtime_error(RuntimeErrorKind::TypeError {
                                operation: "vec store",
                                expected: "vec",
                                got: "non-vec object".to_string(),
                            }));
                        } else {
                            $vm.frames[$current_frame_idx].ip = $ip;
                            return Err($vm.runtime_error(RuntimeErrorKind::InvalidMemoryHandle));
                        }
                    }
                }
            }

            175 => {
                let (a, b, c) = decode_abc($instr);
                let vec_val = $reg_get!($base + a as usize);
                let idx = $reg_get!($base + b as usize).as_int().unwrap_or(-1);
                let value = $reg_get!($base + c as usize);

                if idx < 0 {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    return Err($vm.runtime_error(RuntimeErrorKind::IndexOutOfBounds {
                        index: idx,
                        length: 0,
                    }));
                }

                let vec_ref = GcRef::new(vec_val.as_ptr().unwrap_or(0));
                let container_len = $vm.heap.get(vec_ref).and_then(|obj| match &obj.kind {
                    ObjectKind::Vec(vec) => Some((true, vec.len() as i64)),
                    ObjectKind::Array(arr) => Some((false, arr.len() as i64)),
                    _ => None,
                });

                match container_len {
                    Some((true, len)) => {
                        if let Some(obj) = $vm.heap.get_mut(vec_ref)
                            && let ObjectKind::Vec(vec) = &mut obj.kind
                            && !vec.set(idx as usize, value)
                        {
                            $vm.frames[$current_frame_idx].ip = $ip;
                            return Err($vm.runtime_error(RuntimeErrorKind::IndexOutOfBounds {
                                index: idx,
                                length: len,
                            }));
                        }
                    }
                    Some((false, len)) => {
                        if let Some(obj) = $vm.heap.get_mut(vec_ref)
                            && let ObjectKind::Array(arr) = &mut obj.kind
                            && !arr.set(idx as usize, value)
                        {
                            $vm.frames[$current_frame_idx].ip = $ip;
                            return Err($vm.runtime_error(RuntimeErrorKind::IndexOutOfBounds {
                                index: idx,
                                length: len,
                            }));
                        }
                    }
                    None => {
                        if $vm.heap.get(vec_ref).is_some() {
                            $vm.frames[$current_frame_idx].ip = $ip;
                            return Err($vm.runtime_error(RuntimeErrorKind::TypeError {
                                operation: "vec store",
                                expected: "vec or array",
                                got: "non-vec object".to_string(),
                            }));
                        } else {
                            $vm.frames[$current_frame_idx].ip = $ip;
                            return Err($vm.runtime_error(RuntimeErrorKind::InvalidMemoryHandle));
                        }
                    }
                }
            }

            176 => {
                let (a, b, c) = decode_abc($instr);
                let dest = $base + a as usize;
                let str_val = $reg_get!($base + b as usize);
                let idx = $reg_get!($base + c as usize).as_int().unwrap_or(-1);

                if idx < 0 {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    return Err($vm.runtime_error(RuntimeErrorKind::IndexOutOfBounds {
                        index: idx,
                        length: 0,
                    }));
                }

                let str_ref = GcRef::new(str_val.as_ptr().unwrap_or(0));
                if let Some(obj) = $vm.heap.get(str_ref) {
                    if let ObjectKind::String(s) = &obj.kind {
                        let s_clone = s.as_str().to_string();
                        if let Some(ch) = s_clone.chars().nth(idx as usize) {
                            let mut buf = [0u8; 4];
                            let char_str = ch.encode_utf8(&mut buf);
                            $vm.frames[$current_frame_idx].ip = $ip;
                            match $vm.intern_string(char_str) {
                                Ok(new_ref) => {
                                    $reg_set!(dest, Value::ptr(new_ref.index()));
                                }
                                Err(e) => return Err(e),
                            }
                        } else {
                            $vm.frames[$current_frame_idx].ip = $ip;
                            return Err($vm.runtime_error(RuntimeErrorKind::IndexOutOfBounds {
                                index: idx,
                                length: s_clone.chars().count() as i64,
                            }));
                        }
                    } else {
                        $vm.frames[$current_frame_idx].ip = $ip;
                        return Err($vm.runtime_error(RuntimeErrorKind::TypeError {
                            operation: "string load char",
                            expected: "string",
                            got: "non-string object".to_string(),
                        }));
                    }
                } else {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    return Err($vm.runtime_error(RuntimeErrorKind::InvalidMemoryHandle));
                }
            }

            192 | 193 => {
                let (a, b, c) = decode_abc($instr);
                let start = $reg_get!($base + b as usize);
                let end = $reg_get!($base + c as usize);
                $vm.frames[$current_frame_idx].ip = $ip;
                let range = $vm.alloc_range(start, end, $opcode_byte == 193)?;
                $reg_set!($base + a as usize, Value::ptr(range.index()));
            }

            194 => {
                let (a, b, c) = decode_abc($instr);
                let array = $reg_get!($base + b as usize);
                let range = $reg_get!($base + c as usize);
                $vm.frames[$current_frame_idx].ip = $ip;
                let sliced = $vm.slice_array(array, range)?;
                $reg_set!($base + a as usize, sliced);
            }

            195 => {
                let (a, b, c) = decode_abc($instr);
                let vector = $reg_get!($base + b as usize);
                let range = $reg_get!($base + c as usize);
                $vm.frames[$current_frame_idx].ip = $ip;
                let sliced = $vm.slice_vec(vector, range)?;
                $reg_set!($base + a as usize, sliced);
            }

            _ => unreachable!(),
        }
    }};
}

pub(crate) use execute_arrays;

use crate::vm::{ObjectKind, VM, Value};
use aelys_bytecode::object::{AelysRange, SumTag};
use aelys_common::error::{RuntimeError, RuntimeErrorKind};

impl VM {
    pub(in crate::vm::dispatch) fn alloc_range(
        &mut self,
        start: Value,
        end: Value,
        inclusive: bool,
    ) -> Result<crate::vm::GcRef, RuntimeError> {
        let start = self.range_bound(start, "start")?;
        let end = self.range_bound(end, "end")?;
        self.alloc_object(crate::vm::GcObject::new(ObjectKind::Range(
            AelysRange::new(start, end, inclusive),
        )))
    }

    fn range_bound(&self, value: Value, bound: &'static str) -> Result<Option<i64>, RuntimeError> {
        if value.is_none() {
            return Ok(None);
        }
        value.as_int().map(Some).ok_or_else(|| {
            self.runtime_error(RuntimeErrorKind::TypeError {
                operation: "range construction",
                expected: "integer or Option::None",
                got: format!("{bound} is {}", value.type_name()),
            })
        })
    }

    fn slice_bounds(
        &self,
        range_value: Value,
        length: usize,
    ) -> Result<(usize, usize), RuntimeError> {
        let pointer = range_value.as_ptr().ok_or_else(|| {
            self.runtime_error(RuntimeErrorKind::TypeError {
                operation: "collection slice",
                expected: "range",
                got: range_value.type_name().to_string(),
            })
        })?;
        let range = match self.heap.get(crate::vm::GcRef::new(pointer)) {
            Some(object) => match &object.kind {
                ObjectKind::Range(range) => *range,
                _ => {
                    return Err(self.runtime_error(RuntimeErrorKind::TypeError {
                        operation: "collection slice",
                        expected: "range",
                        got: range_value.type_name().to_string(),
                    }));
                }
            },
            None => return Err(self.runtime_error(RuntimeErrorKind::InvalidMemoryHandle)),
        };
        let start = range.start.unwrap_or(0);
        let end = match range.end {
            Some(value) if range.inclusive => value.checked_add(1).ok_or_else(|| {
                self.runtime_error(RuntimeErrorKind::IndexOutOfBounds {
                    index: value,
                    length: i64::try_from(length).unwrap_or(i64::MAX),
                })
            })?,
            Some(value) => value,
            None => i64::try_from(length).unwrap_or(i64::MAX),
        };
        let length_i64 = i64::try_from(length).unwrap_or(i64::MAX);
        if start < 0 || end < 0 || start > end || end > length_i64 {
            return Err(self.runtime_error(RuntimeErrorKind::IndexOutOfBounds {
                index: if start < 0 || start > length_i64 {
                    start
                } else {
                    end
                },
                length: length_i64,
            }));
        }
        Ok((
            usize::try_from(start).expect("validated slice start is non-negative"),
            usize::try_from(end).expect("validated slice end is non-negative"),
        ))
    }

    pub(in crate::vm::dispatch) fn slice_array(
        &mut self,
        array_value: Value,
        range_value: Value,
    ) -> Result<Value, RuntimeError> {
        let pointer = array_value.as_ptr().ok_or_else(|| {
            self.runtime_error(RuntimeErrorKind::TypeError {
                operation: "array slice",
                expected: "array",
                got: array_value.type_name().to_string(),
            })
        })?;
        let array = match self.heap.get(crate::vm::GcRef::new(pointer)) {
            Some(object) => match &object.kind {
                ObjectKind::Array(array) => array.clone(),
                _ => {
                    return Err(self.runtime_error(RuntimeErrorKind::TypeError {
                        operation: "array slice",
                        expected: "array",
                        got: array_value.type_name().to_string(),
                    }));
                }
            },
            None => return Err(self.runtime_error(RuntimeErrorKind::InvalidMemoryHandle)),
        };
        let (start, end) = self.slice_bounds(range_value, array.len())?;
        let result = self.alloc_vec(aelys_bytecode::object::AelysVec::from_array(
            &array.slice(start, end),
        ))?;
        Ok(Value::ptr(result.index()))
    }

    pub(in crate::vm::dispatch) fn slice_vec(
        &mut self,
        vector_value: Value,
        range_value: Value,
    ) -> Result<Value, RuntimeError> {
        let pointer = vector_value.as_ptr().ok_or_else(|| {
            self.runtime_error(RuntimeErrorKind::TypeError {
                operation: "vec slice",
                expected: "vec",
                got: vector_value.type_name().to_string(),
            })
        })?;
        let vector = match self.heap.get(crate::vm::GcRef::new(pointer)) {
            Some(object) => match &object.kind {
                ObjectKind::Vec(vector) => vector.clone(),
                _ => {
                    return Err(self.runtime_error(RuntimeErrorKind::TypeError {
                        operation: "vec slice",
                        expected: "vec",
                        got: vector_value.type_name().to_string(),
                    }));
                }
            },
            None => return Err(self.runtime_error(RuntimeErrorKind::InvalidMemoryHandle)),
        };
        let (start, end) = self.slice_bounds(range_value, vector.len())?;
        let result = self.alloc_vec(vector.slice(start, end))?;
        Ok(Value::ptr(result.index()))
    }

    fn option_value(&mut self, payload: Option<Value>) -> Result<Value, RuntimeError> {
        match payload {
            Some(payload) => {
                let sum = self.alloc_sum(SumTag::OptionSome, payload)?;
                Ok(Value::ptr(sum.index()))
            }
            None => Ok(Value::none()),
        }
    }

    pub(in crate::vm::dispatch) fn pop_vec_option(
        &mut self,
        vector_value: Value,
    ) -> Result<Value, RuntimeError> {
        let vector_ref = crate::vm::GcRef::new(vector_value.as_ptr().unwrap_or(0));
        let payload = match self.heap.get_mut(vector_ref) {
            Some(object) => match &mut object.kind {
                ObjectKind::Vec(vector) => vector.pop(),
                _ => {
                    return Err(self.runtime_error(RuntimeErrorKind::TypeError {
                        operation: "vec pop",
                        expected: "vec",
                        got: "non-vec object".to_string(),
                    }));
                }
            },
            None => return Err(self.runtime_error(RuntimeErrorKind::InvalidMemoryHandle)),
        };
        self.option_value(payload)
    }

    pub(in crate::vm::dispatch) fn array_get_option(
        &mut self,
        array_value: Value,
        index: i64,
    ) -> Result<Value, RuntimeError> {
        if index < 0 {
            return Ok(Value::none());
        }
        let array_ref = crate::vm::GcRef::new(array_value.as_ptr().unwrap_or(0));
        let payload = match self.heap.get(array_ref) {
            Some(object) => match &object.kind {
                ObjectKind::Array(array) => array.get(index as usize),
                _ => {
                    return Err(self.runtime_error(RuntimeErrorKind::TypeError {
                        operation: "array get",
                        expected: "array",
                        got: "non-array object".to_string(),
                    }));
                }
            },
            None => return Err(self.runtime_error(RuntimeErrorKind::InvalidMemoryHandle)),
        };
        self.option_value(payload)
    }

    pub(in crate::vm::dispatch) fn vec_get_option(
        &mut self,
        vector_value: Value,
        index: i64,
    ) -> Result<Value, RuntimeError> {
        if index < 0 {
            return Ok(Value::none());
        }
        let vector_ref = crate::vm::GcRef::new(vector_value.as_ptr().unwrap_or(0));
        let payload = match self.heap.get(vector_ref) {
            Some(object) => match &object.kind {
                ObjectKind::Vec(vector) => vector.get(index as usize),
                _ => {
                    return Err(self.runtime_error(RuntimeErrorKind::TypeError {
                        operation: "vec get",
                        expected: "vec",
                        got: "non-vec object".to_string(),
                    }));
                }
            },
            None => return Err(self.runtime_error(RuntimeErrorKind::InvalidMemoryHandle)),
        };
        self.option_value(payload)
    }
}
