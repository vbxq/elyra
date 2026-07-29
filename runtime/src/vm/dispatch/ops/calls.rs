        // Call operations: Call(21), Return(22), Return0(23), CallWide(34),
        // CallGlobal(77), CallCached(79), CallUpval(80), TailCallUpval(81)

        match opcode_byte {
            // Call (21), CallWide (34)
            21 | 34 => {
                let mut dest: u16 = 0;
                let mut func_reg: u16 = 0;
                let mut nargs: u16 = 0;
                let mut callee_ref = GcRef::new(0);
                enum CallData {
                    Function {
                        arity: u16,
                        callee_gmap: usize,
                        num_regs: u32,
                        bytecode_ptr: *const u32,
                        bytecode_len: usize,
                        constants_ptr: *const Value,
                        constants_len: usize,
                    },
                    Native {
                        native: crate::vm::NativeFunction,
                    },
                    Closure {
                        arity: u16,
                        callee_gmap: usize,
                        num_regs: u32,
                        inner_func: GcRef,
                        bytecode_ptr: *const u32,
                        bytecode_len: usize,
                        constants_ptr: *const Value,
                        constants_len: usize,
                        upvalues_ptr: *const GcRef,
                        upvalues_len: usize,
                    },
                    Invalid,
                }
                let mut call_data = CallData::Invalid;
                let _ = (dest, func_reg, nargs, callee_ref);
                let _ = &call_data;

                // Part 0: Decode and get function value
                {
                    if opcode_byte == 34 {
                        let first = unsafe { *bytecode_ptr.add(ip) };
                        let second = unsafe { *bytecode_ptr.add(ip + 1) };
                        ip += 2;
                        dest = u16::try_from(first >> 16).expect("wide destination fits u16");
                        func_reg = u16::try_from(first & 0xffff).expect("wide callee fits u16");
                        nargs = u16::try_from(second >> 16).expect("wide arity fits u16");
                    } else {
                        let (dest_tmp, func_reg_tmp, nargs_tmp) = decode_abc(instr);
                        dest = u16::from(dest_tmp);
                        func_reg = u16::from(func_reg_tmp);
                        nargs = u16::from(nargs_tmp);
                    }

                    // Save IP before call
                    self.frames[current_frame_idx].ip = ip;

                    // Get function value
                    let func_value = reg_get!(base + usize::from(func_reg));
                    let func_ptr = match func_value.as_ptr() {
                        Some(p) => p,
                        None => {
                            return Err(self.runtime_error(RuntimeErrorKind::NotCallable(
                                self.value_type_name(func_value).to_string(),
                            )));
                        }
                    };

                    callee_ref = GcRef::new(func_ptr);
                }

                // Part 1: Determine call type
                {
                    call_data = match self.heap.get(callee_ref) {
                        Some(obj) => match &obj.kind {
                            ObjectKind::Function(func) => {
                                let bc = &func.function.bytecode;
                                let consts = &func.constants;
                                CallData::Function {
                                    arity: func.arity(),
                                    callee_gmap: self
                                        .global_mapping_id_for_layout(&func.function.global_layout),
                                    num_regs: func.num_registers(),
                                    bytecode_ptr: bc.as_ptr(),
                                    bytecode_len: bc.len(),
                                    constants_ptr: consts.as_ptr(),
                                    constants_len: consts.len(),
                                }
                            }
                            ObjectKind::Native(native) => CallData::Native {
                                native: native.clone(),
                            },
                            ObjectKind::Closure(closure) => {
                                let inner_gmap = self
                                    .heap
                                    .get(closure.function)
                                    .and_then(|inner| {
                                        if let ObjectKind::Function(f) = &inner.kind {
                                            Some(self.global_mapping_id_for_layout(
                                                &f.function.global_layout,
                                            ))
                                        } else {
                                            None
                                        }
                                    })
                                    .unwrap_or(0);
                                CallData::Closure {
                                    arity: closure.arity,
                                    callee_gmap: inner_gmap,
                                    num_regs: closure.num_registers,
                                    inner_func: closure.function,
                                    bytecode_ptr: closure.bytecode_ptr,
                                    bytecode_len: closure.bytecode_len,
                                    constants_ptr: closure.constants_ptr,
                                    constants_len: closure.constants_len,
                                    upvalues_ptr: closure.upvalues.as_ptr(),
                                    upvalues_len: closure.upvalues.len(),
                                }
                            }
                            _ => CallData::Invalid,
                        },
                        None => {
                            return Err(self.runtime_error(RuntimeErrorKind::NotCallable(
                                "invalid reference".to_string(),
                            )));
                        }
                    };
                }

                // Part 2: Execute call
                {
                    match call_data {
                        CallData::Function {
                            arity,
                            callee_gmap,
                            num_regs,
                            bytecode_ptr: bc_ptr,
                            bytecode_len: bc_len,
                            constants_ptr: const_ptr,
                            constants_len: const_len,
                        } => {
                            self.ensure_function_verified(callee_ref)?;
                            if arity != nargs {
                                return Err(self.runtime_error(RuntimeErrorKind::ArityMismatch {
                                    expected: arity,
                                    got: nargs,
                                }));
                            }

                            if callee_gmap != 0 && callee_gmap != global_mapping_id {
                                if global_mapping_id != 0 {
                                    self.sync_current_function_globals();
                                }
                                self.prepare_globals_for_function(callee_ref);
                            }

                            let new_base = base
                                .checked_add(usize::from(func_reg))
                                .and_then(|v| v.checked_add(1))
                                .ok_or_else(|| {
                                    self.runtime_error(RuntimeErrorKind::StackOverflow)
                                })?;
                            let needed =
                                new_base.checked_add(num_regs as usize).ok_or_else(|| {
                                    self.runtime_error(RuntimeErrorKind::StackOverflow)
                                })?;
                            if needed > self.registers.len() {
                                self.registers.resize(needed, Value::null());
                                regs_ptr = self.registers.as_mut_ptr();
                                let _ = regs_ptr;
                            }

                            let mut new_frame = CallFrame::with_return_dest(
                                callee_ref, new_base, dest, bc_ptr, bc_len, const_ptr, const_len,
                                num_regs,
                            );
                            new_frame.global_mapping_id = callee_gmap;

                            if self.frames.len() >= crate::vm::MAX_FRAMES {
                                return Err(self.runtime_error(RuntimeErrorKind::StackOverflow));
                            }
                            self.frames.push(new_frame);
                            reload_frame_state!();
                        }

                        CallData::Native { native } => {
                            if native.arity != nargs {
                                return Err(self.runtime_error(RuntimeErrorKind::ArityMismatch {
                                    expected: native.arity,
                                    got: nargs,
                                }));
                            }

                            let mut args = Vec::with_capacity(usize::from(nargs));
                            for i in 0..nargs {
                                args.push(reg_get!(
                                    base + usize::from(func_reg) + 1 + usize::from(i)
                                ));
                            }

                            match self.call_cached_native(&native, &args) {
                                Ok(result) => {
                                    self.registers[base + usize::from(dest)] = result;
                                    regs_ptr = self.registers.as_mut_ptr();
                                }
                                Err(e) => return Err(e),
                            }
                        }

                        CallData::Closure {
                            arity,
                            callee_gmap,
                            num_regs,
                            inner_func,
                            bytecode_ptr: bc_ptr,
                            bytecode_len: bc_len,
                            constants_ptr: const_ptr,
                            constants_len: const_len,
                            upvalues_ptr: upval_ptr,
                            upvalues_len: upval_len,
                        } => {
                            self.ensure_function_verified(inner_func)?;
                            if arity != nargs {
                                return Err(self.runtime_error(RuntimeErrorKind::ArityMismatch {
                                    expected: arity,
                                    got: nargs,
                                }));
                            }

                            if callee_gmap != 0 && callee_gmap != global_mapping_id {
                                if global_mapping_id != 0 {
                                    self.sync_current_function_globals();
                                }
                                self.prepare_globals_for_function(inner_func);
                            }

                            let new_base = base
                                .checked_add(usize::from(func_reg))
                                .and_then(|v| v.checked_add(1))
                                .ok_or_else(|| {
                                    self.runtime_error(RuntimeErrorKind::StackOverflow)
                                })?;
                            let needed =
                                new_base.checked_add(num_regs as usize).ok_or_else(|| {
                                    self.runtime_error(RuntimeErrorKind::StackOverflow)
                                })?;
                            if needed > self.registers.len() {
                                self.registers.resize(needed, Value::null());
                                regs_ptr = self.registers.as_mut_ptr();
                                let _ = regs_ptr;
                            }

                            let mut new_frame = CallFrame::with_upvalues(
                                inner_func, new_base, dest, bc_ptr, bc_len, const_ptr, const_len,
                                upval_ptr, upval_len, num_regs,
                            );
                            new_frame.global_mapping_id = callee_gmap;

                            if self.frames.len() >= crate::vm::MAX_FRAMES {
                                return Err(self.runtime_error(RuntimeErrorKind::StackOverflow));
                            }
                            self.frames.push(new_frame);
                            reload_frame_state!();
                        }

                        CallData::Invalid => {
                            return Err(self.runtime_error(RuntimeErrorKind::NotCallable(
                                "non-callable object".to_string(),
                            )));
                        }
                    }
                }
            }

            // Return (22)
            22 => {
                let (a, _, _) = decode_abc(instr);
                let result = reg_get!(base + a as usize);

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

                self.frames.pop();

                if self.frames.is_empty() {
                    return Ok(result);
                }

                reload_frame_state!();

                if needs_switch && caller_gmap != 0 {
                    self.prepare_globals_for_function(func_ref);
                }

                reg_set!(base + dest as usize, result);
            }

            // Return0 (23)
            23 => {
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

                self.frames.pop();

                if self.frames.is_empty() {
                    return Ok(Value::null());
                }

                reload_frame_state!();

                if needs_switch && caller_gmap != 0 {
                    self.prepare_globals_for_function(func_ref);
                }

                reg_set!(base + dest as usize, Value::null());
            }

            // CallGlobal (77)
            77 => {
                let mut dest: u8 = 0;
                let mut nargs: u8 = 0;
                let mut current_func_ptr: usize = 0;
                let mut callee_ref = GcRef::new(0);
                enum CallData {
                    Function {
                        arity: u16,
                        callee_gmap: usize,
                        num_regs: u32,
                        bc_ptr: *const u32,
                        bc_len: usize,
                        const_ptr: *const Value,
                        const_len: usize,
                    },
                    Native {
                        native: crate::vm::NativeFunction,
                    },
                    Closure {
                        arity: u16,
                        callee_gmap: usize,
                        num_regs: u32,
                        inner_func: GcRef,
                        bc_ptr: *const u32,
                        bc_len: usize,
                        const_ptr: *const Value,
                        const_len: usize,
                        upval_ptr: *const GcRef,
                        upval_len: usize,
                    },
                    Invalid,
                }
                let mut call_data = CallData::Invalid;
                let _ = (dest, nargs, current_func_ptr, callee_ref);
                let _ = &call_data;
        include!("call_global.rs");
            }

            // CallCached (79) - Call with function in register
            79 => {
                let mut dest: u8 = 0;
                let mut func_reg: u8 = 0;
                let mut nargs: u8 = 0;
                let mut callee_ref = GcRef::new(0);
                enum CallCachedData {
                    Function {
                        arity: u16,
                        callee_gmap: usize,
                        num_regs: u32,
                        bc_ptr: *const u32,
                        bc_len: usize,
                        const_ptr: *const Value,
                        const_len: usize,
                    },
                    Native {
                        native: crate::vm::NativeFunction,
                    },
                    Closure {
                        arity: u16,
                        callee_gmap: usize,
                        num_regs: u32,
                        inner_func: GcRef,
                        bc_ptr: *const u32,
                        bc_len: usize,
                        const_ptr: *const Value,
                        const_len: usize,
                        upval_ptr: *const GcRef,
                        upval_len: usize,
                    },
                    Invalid,
                }
                let mut call_data = CallCachedData::Invalid;
                let _ = (dest, func_reg, nargs, callee_ref);
                let _ = &call_data;
        include!("call_cached.rs");
            }

            // CallUpval (80) - Call function from upvalue
            80 => {
                let mut dest: u8 = 0;
                let mut upval_idx: u8 = 0;
                let mut nargs: u8 = 0;
                let mut callee_ref = GcRef::new(0);
                enum CallUpvalData {
                    Function {
                        arity: u16,
                        callee_gmap: usize,
                        num_regs: u32,
                        bc_ptr: *const u32,
                        bc_len: usize,
                        const_ptr: *const Value,
                        const_len: usize,
                    },
                    Closure {
                        arity: u16,
                        callee_gmap: usize,
                        num_regs: u32,
                        inner_func: GcRef,
                        bc_ptr: *const u32,
                        bc_len: usize,
                        const_ptr: *const Value,
                        const_len: usize,
                        upval_ptr: *const GcRef,
                        upval_len: usize,
                    },
                    Invalid,
                }
                let mut call_data = CallUpvalData::Invalid;
                let _ = (dest, upval_idx, nargs, callee_ref);
                let _ = &call_data;
        include!("call_upval.rs");
            }

            // TailCallUpval (81) - Tail call function from upvalue
            81 => {
                let state = DispatchState {
                    base,
                    constants: constants_ptr,
                    constants_len,
                    registers: regs_ptr,
                    registers_len: regs_len,
                    frame_index: current_frame_idx,
                };
                match super::ops::tail_call_upval::execute(
                    self,
                    &state,
                    ip,
                    upvalues_ptr,
                    upvalues_len,
                    global_mapping_id,
                    instr,
                )? {
                    DispatchControl::ReloadFrame => reload_frame_state!(),
                    DispatchControl::Continue
                    | DispatchControl::Returned(_)
                    | DispatchControl::ReturnToCaller { .. } => {
                        unreachable!("tail call handler must reload the active frame")
                    }
                }
            }

            _ => unreachable!(),
        }
