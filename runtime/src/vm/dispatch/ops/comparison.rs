macro_rules! execute_comparison {
    (
        $vm:ident, $opcode_byte:ident, $instr:ident, $base:ident,
        $current_frame_idx:ident, $ip:ident, $reg_get:ident,
        $reg_ref:ident, $reg_set:ident
    ) => {{
        match $opcode_byte {
            11 => {
                let (a, b, c) = decode_abc($instr);
                let lhs = $reg_get!($base + b as usize);
                let rhs = $reg_get!($base + c as usize);
                let result = $vm.values_equal(lhs, rhs);
                $reg_set!($base + a as usize, Value::bool(result));
            }
            12 => {
                let (a, b, c) = decode_abc($instr);
                let lhs = $reg_get!($base + b as usize);
                let rhs = $reg_get!($base + c as usize);
                let result = $vm.values_equal(lhs, rhs);
                $reg_set!($base + a as usize, Value::bool(!result));
            }

            13 => {
                let (a, b, c) = decode_abc($instr);
                let left = $reg_get!($base + b as usize);
                let right = $reg_get!($base + c as usize);
                if let (Some(l), Some(r)) = (left.as_int(), right.as_int()) {
                    $reg_set!($base + a as usize, Value::bool(l < r));
                } else {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    match $vm.compare_lt(left, right) {
                        Ok(result) => $reg_set!($base + a as usize, Value::bool(result)),
                        Err(e) => return Err(e),
                    }
                }
            }

            14 => {
                let (a, b, c) = decode_abc($instr);
                let left = $reg_get!($base + b as usize);
                let right = $reg_get!($base + c as usize);
                if let (Some(l), Some(r)) = (left.as_int(), right.as_int()) {
                    $reg_set!($base + a as usize, Value::bool(l <= r));
                } else {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    match $vm.compare_le(left, right) {
                        Ok(result) => $reg_set!($base + a as usize, Value::bool(result)),
                        Err(e) => return Err(e),
                    }
                }
            }

            15 => {
                let (a, b, c) = decode_abc($instr);
                let left = $reg_get!($base + b as usize);
                let right = $reg_get!($base + c as usize);
                if let (Some(l), Some(r)) = (left.as_int(), right.as_int()) {
                    $reg_set!($base + a as usize, Value::bool(l > r));
                } else {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    match $vm.compare_gt(left, right) {
                        Ok(result) => $reg_set!($base + a as usize, Value::bool(result)),
                        Err(e) => return Err(e),
                    }
                }
            }

            16 => {
                let (a, b, c) = decode_abc($instr);
                let left = $reg_get!($base + b as usize);
                let right = $reg_get!($base + c as usize);
                if let (Some(l), Some(r)) = (left.as_int(), right.as_int()) {
                    $reg_set!($base + a as usize, Value::bool(l >= r));
                } else {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    match $vm.compare_ge(left, right) {
                        Ok(result) => $reg_set!($base + a as usize, Value::bool(result)),
                        Err(e) => return Err(e),
                    }
                }
            }

            59 => {
                let (a, b, c) = decode_abc($instr);
                let left = *$reg_ref!($base + b as usize);
                let right = *$reg_ref!($base + c as usize);
                let result = if left.both_inline_int_bits(&right) {
                    left.as_int_unchecked() < right.as_int_unchecked()
                } else {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    $vm.int_compare(left, right, "<")?.is_lt()
                };
                $reg_set!($base + a as usize, Value::bool(result));
            }

            60 => {
                let (a, b, c) = decode_abc($instr);
                let left = *$reg_ref!($base + b as usize);
                let right = *$reg_ref!($base + c as usize);
                let result = if left.both_inline_int_bits(&right) {
                    left.as_int_unchecked() <= right.as_int_unchecked()
                } else {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    $vm.int_compare(left, right, "<=")?.is_le()
                };
                $reg_set!($base + a as usize, Value::bool(result));
            }

            61 => {
                let (a, b, c) = decode_abc($instr);
                let left = *$reg_ref!($base + b as usize);
                let right = *$reg_ref!($base + c as usize);
                let result = if left.both_inline_int_bits(&right) {
                    left.as_int_unchecked() > right.as_int_unchecked()
                } else {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    $vm.int_compare(left, right, ">")?.is_gt()
                };
                $reg_set!($base + a as usize, Value::bool(result));
            }

            62 => {
                let (a, b, c) = decode_abc($instr);
                let left = *$reg_ref!($base + b as usize);
                let right = *$reg_ref!($base + c as usize);
                let result = if left.both_inline_int_bits(&right) {
                    left.as_int_unchecked() >= right.as_int_unchecked()
                } else {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    $vm.int_compare(left, right, ">=")?.is_ge()
                };
                $reg_set!($base + a as usize, Value::bool(result));
            }

            63 => {
                let (a, b, c) = decode_abc($instr);
                let left = *$reg_ref!($base + b as usize);
                let right = *$reg_ref!($base + c as usize);
                let result = if left.both_inline_int_bits(&right) {
                    left.as_int_unchecked() == right.as_int_unchecked()
                } else {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    $vm.int_compare(left, right, "==")?.is_eq()
                };
                $reg_set!($base + a as usize, Value::bool(result));
            }

            64 => {
                let (a, b, c) = decode_abc($instr);
                let left = *$reg_ref!($base + b as usize);
                let right = *$reg_ref!($base + c as usize);
                let result = if left.both_inline_int_bits(&right) {
                    left.as_int_unchecked() != right.as_int_unchecked()
                } else {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    $vm.int_compare(left, right, "!=")?.is_ne()
                };
                $reg_set!($base + a as usize, Value::bool(result));
            }

            65 => {
                let (a, b, c) = decode_abc($instr);
                let l = $reg_ref!($base + b as usize).as_float_unchecked();
                let r = $reg_ref!($base + c as usize).as_float_unchecked();
                $reg_set!($base + a as usize, Value::bool(l < r));
            }

            66 => {
                let (a, b, c) = decode_abc($instr);
                let l = $reg_ref!($base + b as usize).as_float_unchecked();
                let r = $reg_ref!($base + c as usize).as_float_unchecked();
                $reg_set!($base + a as usize, Value::bool(l <= r));
            }

            67 => {
                let (a, b, c) = decode_abc($instr);
                let l = $reg_ref!($base + b as usize).as_float_unchecked();
                let r = $reg_ref!($base + c as usize).as_float_unchecked();
                $reg_set!($base + a as usize, Value::bool(l > r));
            }

            68 => {
                let (a, b, c) = decode_abc($instr);
                let l = $reg_ref!($base + b as usize).as_float_unchecked();
                let r = $reg_ref!($base + c as usize).as_float_unchecked();
                $reg_set!($base + a as usize, Value::bool(l >= r));
            }

            69 => {
                let (a, b, c) = decode_abc($instr);
                let l = $reg_ref!($base + b as usize).as_float_unchecked();
                let r = $reg_ref!($base + c as usize).as_float_unchecked();
                $reg_set!($base + a as usize, Value::bool(l == r));
            }

            70 => {
                let (a, b, c) = decode_abc($instr);
                let l = $reg_ref!($base + b as usize).as_float_unchecked();
                let r = $reg_ref!($base + c as usize).as_float_unchecked();
                $reg_set!($base + a as usize, Value::bool(l != r));
            }

            71 => {
                let (a, b, c) = decode_abc($instr);
                let left = *$reg_ref!($base + b as usize);
                let result = if left.has_inline_int_bit() {
                    left.as_int_unchecked() < c as i64
                } else {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    $vm.int_compare(left, Value::int(c as i64), "<")?.is_lt()
                };
                $reg_set!($base + a as usize, Value::bool(result));
            }

            72 => {
                let (a, b, c) = decode_abc($instr);
                let left = *$reg_ref!($base + b as usize);
                let result = if left.has_inline_int_bit() {
                    left.as_int_unchecked() <= c as i64
                } else {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    $vm.int_compare(left, Value::int(c as i64), "<=")?.is_le()
                };
                $reg_set!($base + a as usize, Value::bool(result));
            }

            73 => {
                let (a, b, c) = decode_abc($instr);
                let left = *$reg_ref!($base + b as usize);
                let result = if left.has_inline_int_bit() {
                    left.as_int_unchecked() > c as i64
                } else {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    $vm.int_compare(left, Value::int(c as i64), ">")?.is_gt()
                };
                $reg_set!($base + a as usize, Value::bool(result));
            }

            74 => {
                let (a, b, c) = decode_abc($instr);
                let left = *$reg_ref!($base + b as usize);
                let result = if left.has_inline_int_bit() {
                    left.as_int_unchecked() >= c as i64
                } else {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    $vm.int_compare(left, Value::int(c as i64), ">=")?.is_ge()
                };
                $reg_set!($base + a as usize, Value::bool(result));
            }

            92 => {
                let (a, b, c) = decode_abc($instr);
                let left = $reg_get!($base + b as usize);
                let right = $reg_get!($base + c as usize);
                if let (Some(l), Some(r)) = (left.as_int(), right.as_int()) {
                    $reg_set!($base + a as usize, Value::bool(l < r));
                } else if let (Some(l), Some(r)) = ($vm.int_value(left), $vm.int_value(right)) {
                    $reg_set!($base + a as usize, Value::bool(l < r));
                } else {
                    let l_float = $vm.number_as_f64(left);
                    let r_float = $vm.number_as_f64(right);
                    if let (Some(l), Some(r)) = (l_float, r_float) {
                        $reg_set!($base + a as usize, Value::bool(l < r));
                    } else {
                        $reg_set!($base + a as usize, Value::bool(false));
                    }
                }
            }

            93 => {
                let (a, b, c) = decode_abc($instr);
                let left = $reg_get!($base + b as usize);
                let right = $reg_get!($base + c as usize);
                if let (Some(l), Some(r)) = (left.as_int(), right.as_int()) {
                    $reg_set!($base + a as usize, Value::bool(l <= r));
                } else if let (Some(l), Some(r)) = ($vm.int_value(left), $vm.int_value(right)) {
                    $reg_set!($base + a as usize, Value::bool(l <= r));
                } else {
                    let l_float = $vm.number_as_f64(left);
                    let r_float = $vm.number_as_f64(right);
                    if let (Some(l), Some(r)) = (l_float, r_float) {
                        $reg_set!($base + a as usize, Value::bool(l <= r));
                    } else {
                        $reg_set!($base + a as usize, Value::bool(false));
                    }
                }
            }

            94 => {
                let (a, b, c) = decode_abc($instr);
                let left = $reg_get!($base + b as usize);
                let right = $reg_get!($base + c as usize);
                if let (Some(l), Some(r)) = (left.as_int(), right.as_int()) {
                    $reg_set!($base + a as usize, Value::bool(l > r));
                } else if let (Some(l), Some(r)) = ($vm.int_value(left), $vm.int_value(right)) {
                    $reg_set!($base + a as usize, Value::bool(l > r));
                } else {
                    let l_float = $vm.number_as_f64(left);
                    let r_float = $vm.number_as_f64(right);
                    if let (Some(l), Some(r)) = (l_float, r_float) {
                        $reg_set!($base + a as usize, Value::bool(l > r));
                    } else {
                        $reg_set!($base + a as usize, Value::bool(false));
                    }
                }
            }

            95 => {
                let (a, b, c) = decode_abc($instr);
                let left = $reg_get!($base + b as usize);
                let right = $reg_get!($base + c as usize);
                if let (Some(l), Some(r)) = (left.as_int(), right.as_int()) {
                    $reg_set!($base + a as usize, Value::bool(l >= r));
                } else if let (Some(l), Some(r)) = ($vm.int_value(left), $vm.int_value(right)) {
                    $reg_set!($base + a as usize, Value::bool(l >= r));
                } else {
                    let l_float = $vm.number_as_f64(left);
                    let r_float = $vm.number_as_f64(right);
                    if let (Some(l), Some(r)) = (l_float, r_float) {
                        $reg_set!($base + a as usize, Value::bool(l >= r));
                    } else {
                        $reg_set!($base + a as usize, Value::bool(false));
                    }
                }
            }

            96 => {
                let (a, b, c) = decode_abc($instr);
                let left = $reg_get!($base + b as usize);
                let right = $reg_get!($base + c as usize);
                if let (Some(l), Some(r)) = (left.as_int(), right.as_int()) {
                    $reg_set!($base + a as usize, Value::bool(l == r));
                } else if let (Some(l), Some(r)) = ($vm.int_value(left), $vm.int_value(right)) {
                    $reg_set!($base + a as usize, Value::bool(l == r));
                } else {
                    let l_float = $vm.number_as_f64(left);
                    let r_float = $vm.number_as_f64(right);
                    if let (Some(l), Some(r)) = (l_float, r_float) {
                        $reg_set!($base + a as usize, Value::bool(l == r));
                    } else {
                        let eq = $vm.values_equal(left, right);
                        $reg_set!($base + a as usize, Value::bool(eq));
                    }
                }
            }

            97 => {
                let (a, b, c) = decode_abc($instr);
                let left = $reg_get!($base + b as usize);
                let right = $reg_get!($base + c as usize);
                if let (Some(l), Some(r)) = (left.as_int(), right.as_int()) {
                    $reg_set!($base + a as usize, Value::bool(l != r));
                } else if let (Some(l), Some(r)) = ($vm.int_value(left), $vm.int_value(right)) {
                    $reg_set!($base + a as usize, Value::bool(l != r));
                } else {
                    let l_float = $vm.number_as_f64(left);
                    let r_float = $vm.number_as_f64(right);
                    if let (Some(l), Some(r)) = (l_float, r_float) {
                        $reg_set!($base + a as usize, Value::bool(l != r));
                    } else {
                        let eq = $vm.values_equal(left, right);
                        $reg_set!($base + a as usize, Value::bool(!eq));
                    }
                }
            }

            98 => {
                let (a, b, c) = decode_abc($instr);
                let left = $reg_get!($base + b as usize);
                let right = $reg_get!($base + c as usize);
                let l_float = $vm.number_as_f64(left);
                let r_float = $vm.number_as_f64(right);
                if let (Some(l), Some(r)) = (l_float, r_float) {
                    $reg_set!($base + a as usize, Value::bool(l < r));
                } else {
                    $reg_set!($base + a as usize, Value::bool(false));
                }
            }

            99 => {
                let (a, b, c) = decode_abc($instr);
                let left = $reg_get!($base + b as usize);
                let right = $reg_get!($base + c as usize);
                let l_float = $vm.number_as_f64(left);
                let r_float = $vm.number_as_f64(right);
                if let (Some(l), Some(r)) = (l_float, r_float) {
                    $reg_set!($base + a as usize, Value::bool(l <= r));
                } else {
                    $reg_set!($base + a as usize, Value::bool(false));
                }
            }

            100 => {
                let (a, b, c) = decode_abc($instr);
                let left = $reg_get!($base + b as usize);
                let right = $reg_get!($base + c as usize);
                let l_float = $vm.number_as_f64(left);
                let r_float = $vm.number_as_f64(right);
                if let (Some(l), Some(r)) = (l_float, r_float) {
                    $reg_set!($base + a as usize, Value::bool(l > r));
                } else {
                    $reg_set!($base + a as usize, Value::bool(false));
                }
            }

            101 => {
                let (a, b, c) = decode_abc($instr);
                let left = $reg_get!($base + b as usize);
                let right = $reg_get!($base + c as usize);
                let l_float = $vm.number_as_f64(left);
                let r_float = $vm.number_as_f64(right);
                if let (Some(l), Some(r)) = (l_float, r_float) {
                    $reg_set!($base + a as usize, Value::bool(l >= r));
                } else {
                    $reg_set!($base + a as usize, Value::bool(false));
                }
            }

            102 => {
                let (a, b, c) = decode_abc($instr);
                let left = $reg_get!($base + b as usize);
                let right = $reg_get!($base + c as usize);
                let l_float = $vm.number_as_f64(left);
                let r_float = $vm.number_as_f64(right);
                if let (Some(l), Some(r)) = (l_float, r_float) {
                    $reg_set!($base + a as usize, Value::bool(l == r));
                } else {
                    let eq = $vm.values_equal(left, right);
                    $reg_set!($base + a as usize, Value::bool(eq));
                }
            }

            103 => {
                let (a, b, c) = decode_abc($instr);
                let left = $reg_get!($base + b as usize);
                let right = $reg_get!($base + c as usize);
                let l_float = $vm.number_as_f64(left);
                let r_float = $vm.number_as_f64(right);
                if let (Some(l), Some(r)) = (l_float, r_float) {
                    $reg_set!($base + a as usize, Value::bool(l != r));
                } else {
                    let eq = $vm.values_equal(left, right);
                    $reg_set!($base + a as usize, Value::bool(!eq));
                }
            }

            _ => unreachable!(),
        }
    }};
}

pub(crate) use execute_comparison;
