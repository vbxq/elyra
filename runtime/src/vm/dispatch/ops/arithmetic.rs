macro_rules! execute_arithmetic {
    (
        $vm:ident, $opcode_byte:ident, $instr:ident, $base:ident,
        $current_frame_idx:ident, $ip:ident, $reg_get:ident,
        $reg_ref:ident, $reg_set:ident, $int_value:ident
    ) => {{
        // Arithmetic: Add(5), Sub(6), Mul(7), Div(8), Mod(9), Neg(10),
        // plus the specialized int/float variants (AddI, AddII, AddFF, etc)
        use aelys_bytecode::IntOp;

        macro_rules! int_slow {
            ($op:expr, $left:expr, $right:expr) => {{
                $vm.frames[$current_frame_idx].ip = $ip;
                $vm.int_binary($op, $left, $right)?
            }};
        }

        match $opcode_byte {
            // Add (5)
            5 => {
                let (a, b, c) = decode_abc($instr);
                let left = $reg_get!($base + b as usize);
                let right = $reg_get!($base + c as usize);

                if let (Some(l), Some(r)) = (left.as_int(), right.as_int()) {
                    $reg_set!($base + a as usize, $int_value!(l.wrapping_add(r)));
                } else {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    match $vm.add_values(left, right) {
                        Ok(result) => $reg_set!($base + a as usize, result),
                        Err(e) => return Err(e),
                    }
                }
            }

            // Sub (6)
            6 => {
                let (a, b, c) = decode_abc($instr);
                let left = $reg_get!($base + b as usize);
                let right = $reg_get!($base + c as usize);

                if let (Some(l), Some(r)) = (left.as_int(), right.as_int()) {
                    $reg_set!($base + a as usize, $int_value!(l.wrapping_sub(r)));
                } else {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    match $vm.sub_values(left, right) {
                        Ok(result) => $reg_set!($base + a as usize, result),
                        Err(e) => return Err(e),
                    }
                }
            }

            // Mul (7)
            7 => {
                let (a, b, c) = decode_abc($instr);
                let left = $reg_get!($base + b as usize);
                let right = $reg_get!($base + c as usize);

                if let (Some(l), Some(r)) = (left.as_int(), right.as_int()) {
                    let product = match l.checked_mul(r) {
                        Some(product) => product,
                        None => {
                            $vm.frames[$current_frame_idx].ip = $ip;
                            return Err($vm.runtime_error(RuntimeErrorKind::IntegerOverflow));
                        }
                    };
                    $reg_set!($base + a as usize, $int_value!(product));
                } else {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    match $vm.mul_values(left, right) {
                        Ok(result) => $reg_set!($base + a as usize, result),
                        Err(e) => return Err(e),
                    }
                }
            }

            // Div (8) - no fast path because we need to check for div by zero anyway
            8 => {
                let (a, b, c) = decode_abc($instr);
                let left = $reg_get!($base + b as usize);
                let right = $reg_get!($base + c as usize);
                $vm.frames[$current_frame_idx].ip = $ip;
                match $vm.div_values(left, right) {
                    Ok(result) => $reg_set!($base + a as usize, result),
                    Err(e) => return Err(e),
                }
            }

            // Mod (9) - same deal
            9 => {
                let (a, b, c) = decode_abc($instr);
                let left = $reg_get!($base + b as usize);
                let right = $reg_get!($base + c as usize);
                $vm.frames[$current_frame_idx].ip = $ip;
                match $vm.mod_values(left, right) {
                    Ok(result) => $reg_set!($base + a as usize, result),
                    Err(e) => return Err(e),
                }
            }

            // Neg (10)
            10 => {
                let (a, b, _) = decode_abc($instr);
                let value = $reg_get!($base + b as usize);
                if let Some(n) = value.as_int() {
                    $reg_set!($base + a as usize, $int_value!(-n));
                } else {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    match $vm.neg_value(value) {
                        Ok(result) => $reg_set!($base + a as usize, result),
                        Err(e) => return Err(e),
                    }
                }
            }

            // AddI (42)
            42 => {
                let (a, b, c) = decode_abc($instr);
                let left = *$reg_ref!($base + b as usize);
                if left.has_inline_int_bit() {
                    $reg_set!(
                        $base + a as usize,
                        $int_value!(left.as_int_unchecked() + c as i64)
                    );
                } else {
                    let result = int_slow!(IntOp::Add, left, Value::int(c as i64));
                    $reg_set!($base + a as usize, result);
                }
            }

            // SubI (43)
            43 => {
                let (a, b, c) = decode_abc($instr);
                let left = *$reg_ref!($base + b as usize);
                if left.has_inline_int_bit() {
                    $reg_set!(
                        $base + a as usize,
                        $int_value!(left.as_int_unchecked() - c as i64)
                    );
                } else {
                    let result = int_slow!(IntOp::Sub, left, Value::int(c as i64));
                    $reg_set!($base + a as usize, result);
                }
            }

            // AddII (49)
            49 => {
                let (a, b, c) = decode_abc($instr);
                let left = *$reg_ref!($base + b as usize);
                let right = *$reg_ref!($base + c as usize);
                if left.both_inline_int_bits(&right) {
                    let (l, r) = (left.as_int_unchecked(), right.as_int_unchecked());
                    $reg_set!($base + a as usize, $int_value!(l + r));
                } else {
                    let result = int_slow!(IntOp::Add, left, right);
                    $reg_set!($base + a as usize, result);
                }
            }

            // SubII (50)
            50 => {
                let (a, b, c) = decode_abc($instr);
                let left = *$reg_ref!($base + b as usize);
                let right = *$reg_ref!($base + c as usize);
                if left.both_inline_int_bits(&right) {
                    let (l, r) = (left.as_int_unchecked(), right.as_int_unchecked());
                    $reg_set!($base + a as usize, $int_value!(l - r));
                } else {
                    let result = int_slow!(IntOp::Sub, left, right);
                    $reg_set!($base + a as usize, result);
                }
            }

            // MulII (51)
            51 => {
                let (a, b, c) = decode_abc($instr);
                let left = *$reg_ref!($base + b as usize);
                let right = *$reg_ref!($base + c as usize);
                let product = if left.both_inline_int_bits(&right) {
                    left.as_int_unchecked().checked_mul(right.as_int_unchecked())
                } else {
                    None
                };
                match product {
                    Some(product) => $reg_set!($base + a as usize, $int_value!(product)),
                    None => {
                        let result = int_slow!(IntOp::Mul, left, right);
                        $reg_set!($base + a as usize, result);
                    }
                }
            }

            // DivII (52)
            52 => {
                let (a, b, c) = decode_abc($instr);
                let left = *$reg_ref!($base + b as usize);
                let right = *$reg_ref!($base + c as usize);
                if left.both_inline_int_bits(&right) && right.as_int_unchecked() != 0 {
                    let (l, r) = (left.as_int_unchecked(), right.as_int_unchecked());
                    $reg_set!($base + a as usize, $int_value!(l / r));
                } else {
                    let result = int_slow!(IntOp::Div, left, right);
                    $reg_set!($base + a as usize, result);
                }
            }

            // ModII (53)
            53 => {
                let (a, b, c) = decode_abc($instr);
                let left = *$reg_ref!($base + b as usize);
                let right = *$reg_ref!($base + c as usize);
                if left.both_inline_int_bits(&right) && right.as_int_unchecked() != 0 {
                    let (l, r) = (left.as_int_unchecked(), right.as_int_unchecked());
                    $reg_set!($base + a as usize, $int_value!(l % r));
                } else {
                    let result = int_slow!(IntOp::Rem, left, right);
                    $reg_set!($base + a as usize, result);
                }
            }

            // AddFF (54)
            54 => {
                let (a, b, c) = decode_abc($instr);
                let l = $reg_ref!($base + b as usize).as_float_unchecked();
                let r = $reg_ref!($base + c as usize).as_float_unchecked();
                $reg_set!($base + a as usize, Value::float(l + r));
            }

            // SubFF (55)
            55 => {
                let (a, b, c) = decode_abc($instr);
                let l = $reg_ref!($base + b as usize).as_float_unchecked();
                let r = $reg_ref!($base + c as usize).as_float_unchecked();
                $reg_set!($base + a as usize, Value::float(l - r));
            }

            // MulFF (56)
            56 => {
                let (a, b, c) = decode_abc($instr);
                let l = $reg_ref!($base + b as usize).as_float_unchecked();
                let r = $reg_ref!($base + c as usize).as_float_unchecked();
                $reg_set!($base + a as usize, Value::float(l * r));
            }

            // DivFF (57)
            57 => {
                let (a, b, c) = decode_abc($instr);
                let l = $reg_ref!($base + b as usize).as_float_unchecked();
                let r = $reg_ref!($base + c as usize).as_float_unchecked();
                $reg_set!($base + a as usize, Value::float(l / r));
            }

            // ModFF (58)
            58 => {
                let (a, b, c) = decode_abc($instr);
                let l = $reg_ref!($base + b as usize).as_float_unchecked();
                let r = $reg_ref!($base + c as usize).as_float_unchecked();
                $reg_set!($base + a as usize, Value::float(l % r));
            }

            // AddIIG (82)
            82 => {
                let (a, b, c) = decode_abc($instr);
                let left = $reg_get!($base + b as usize);
                let right = $reg_get!($base + c as usize);
                if let (Some(l), Some(r)) = (left.as_int(), right.as_int()) {
                    $reg_set!($base + a as usize, $int_value!(l.wrapping_add(r)));
                } else if $vm.is_int_value(left) && $vm.is_int_value(right) {
                    let result = int_slow!(IntOp::Add, left, right);
                    $reg_set!($base + a as usize, result);
                } else {
                    let l_float = $vm.number_as_f64(left);
                    let r_float = $vm.number_as_f64(right);
                    if let (Some(l), Some(r)) = (l_float, r_float) {
                        $reg_set!($base + a as usize, Value::float(l + r));
                    } else {
                        $vm.frames[$current_frame_idx].ip = $ip;
                        match $vm.add_values(left, right) {
                            Ok(result) => $reg_set!($base + a as usize, result),
                            Err(e) => return Err(e),
                        }
                    }
                }
            }

            // SubIIG (83)
            83 => {
                let (a, b, c) = decode_abc($instr);
                let left = $reg_get!($base + b as usize);
                let right = $reg_get!($base + c as usize);
                if let (Some(l), Some(r)) = (left.as_int(), right.as_int()) {
                    $reg_set!($base + a as usize, $int_value!(l.wrapping_sub(r)));
                } else if $vm.is_int_value(left) && $vm.is_int_value(right) {
                    let result = int_slow!(IntOp::Sub, left, right);
                    $reg_set!($base + a as usize, result);
                } else {
                    let l_float = $vm.number_as_f64(left);
                    let r_float = $vm.number_as_f64(right);
                    if let (Some(l), Some(r)) = (l_float, r_float) {
                        $reg_set!($base + a as usize, Value::float(l - r));
                    } else {
                        $vm.frames[$current_frame_idx].ip = $ip;
                        match $vm.sub_values(left, right) {
                            Ok(result) => $reg_set!($base + a as usize, result),
                            Err(e) => return Err(e),
                        }
                    }
                }
            }

            // MulIIG (84)
            84 => {
                let (a, b, c) = decode_abc($instr);
                let left = $reg_get!($base + b as usize);
                let right = $reg_get!($base + c as usize);
                if $vm.is_int_value(left) && $vm.is_int_value(right) {
                    let result = int_slow!(IntOp::Mul, left, right);
                    $reg_set!($base + a as usize, result);
                } else {
                    let l_float = $vm.number_as_f64(left);
                    let r_float = $vm.number_as_f64(right);
                    if let (Some(l), Some(r)) = (l_float, r_float) {
                        $reg_set!($base + a as usize, Value::float(l * r));
                    } else {
                        $vm.frames[$current_frame_idx].ip = $ip;
                        match $vm.mul_values(left, right) {
                            Ok(result) => $reg_set!($base + a as usize, result),
                            Err(e) => return Err(e),
                        }
                    }
                }
            }

            // DivIIG (85)
            85 => {
                let (a, b, c) = decode_abc($instr);
                let left = $reg_get!($base + b as usize);
                let right = $reg_get!($base + c as usize);
                if $vm.is_int_value(left) && $vm.is_int_value(right) {
                    let result = int_slow!(IntOp::Div, left, right);
                    $reg_set!($base + a as usize, result);
                } else {
                    let l_float = $vm.number_as_f64(left);
                    let r_float = $vm.number_as_f64(right);
                    if let (Some(l), Some(r)) = (l_float, r_float) {
                        $reg_set!($base + a as usize, Value::float(l / r));
                    } else {
                        $vm.frames[$current_frame_idx].ip = $ip;
                        match $vm.div_values(left, right) {
                            Ok(result) => $reg_set!($base + a as usize, result),
                            Err(e) => return Err(e),
                        }
                    }
                }
            }

            // ModIIG (86)
            86 => {
                let (a, b, c) = decode_abc($instr);
                let left = $reg_get!($base + b as usize);
                let right = $reg_get!($base + c as usize);
                if $vm.is_int_value(left) && $vm.is_int_value(right) {
                    let result = int_slow!(IntOp::Rem, left, right);
                    $reg_set!($base + a as usize, result);
                } else {
                    let l_float = $vm.number_as_f64(left);
                    let r_float = $vm.number_as_f64(right);
                    if let (Some(l), Some(r)) = (l_float, r_float) {
                        $reg_set!($base + a as usize, Value::float(l % r));
                    } else {
                        $vm.frames[$current_frame_idx].ip = $ip;
                        match $vm.mod_values(left, right) {
                            Ok(result) => $reg_set!($base + a as usize, result),
                            Err(e) => return Err(e),
                        }
                    }
                }
            }

            // AddFFG (87)
            87 => {
                let (a, b, c) = decode_abc($instr);
                let left = $reg_get!($base + b as usize);
                let right = $reg_get!($base + c as usize);
                let l_float = $vm.number_as_f64(left);
                let r_float = $vm.number_as_f64(right);
                if let (Some(l), Some(r)) = (l_float, r_float) {
                    $reg_set!($base + a as usize, Value::float(l + r));
                } else {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    match $vm.add_values(left, right) {
                        Ok(result) => $reg_set!($base + a as usize, result),
                        Err(e) => return Err(e),
                    }
                }
            }

            // SubFFG (88)
            88 => {
                let (a, b, c) = decode_abc($instr);
                let left = $reg_get!($base + b as usize);
                let right = $reg_get!($base + c as usize);
                let l_float = $vm.number_as_f64(left);
                let r_float = $vm.number_as_f64(right);
                if let (Some(l), Some(r)) = (l_float, r_float) {
                    $reg_set!($base + a as usize, Value::float(l - r));
                } else {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    match $vm.sub_values(left, right) {
                        Ok(result) => $reg_set!($base + a as usize, result),
                        Err(e) => return Err(e),
                    }
                }
            }

            // MulFFG (89)
            89 => {
                let (a, b, c) = decode_abc($instr);
                let left = $reg_get!($base + b as usize);
                let right = $reg_get!($base + c as usize);
                let l_float = $vm.number_as_f64(left);
                let r_float = $vm.number_as_f64(right);
                if let (Some(l), Some(r)) = (l_float, r_float) {
                    $reg_set!($base + a as usize, Value::float(l * r));
                } else {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    match $vm.mul_values(left, right) {
                        Ok(result) => $reg_set!($base + a as usize, result),
                        Err(e) => return Err(e),
                    }
                }
            }

            // DivFFG (90)
            90 => {
                let (a, b, c) = decode_abc($instr);
                let left = $reg_get!($base + b as usize);
                let right = $reg_get!($base + c as usize);
                let l_float = $vm.number_as_f64(left);
                let r_float = $vm.number_as_f64(right);
                if let (Some(l), Some(r)) = (l_float, r_float) {
                    $reg_set!($base + a as usize, Value::float(l / r));
                } else {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    match $vm.div_values(left, right) {
                        Ok(result) => $reg_set!($base + a as usize, result),
                        Err(e) => return Err(e),
                    }
                }
            }

            // ModFFG (91)
            91 => {
                let (a, b, c) = decode_abc($instr);
                let left = $reg_get!($base + b as usize);
                let right = $reg_get!($base + c as usize);
                let l_float = $vm.number_as_f64(left);
                let r_float = $vm.number_as_f64(right);
                if let (Some(l), Some(r)) = (l_float, r_float) {
                    $reg_set!($base + a as usize, Value::float(l % r));
                } else {
                    $vm.frames[$current_frame_idx].ip = $ip;
                    match $vm.mod_values(left, right) {
                        Ok(result) => $reg_set!($base + a as usize, result),
                        Err(e) => return Err(e),
                    }
                }
            }
            _ => unreachable!(),
        }
    }};
}

pub(crate) use execute_arithmetic;
