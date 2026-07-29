        {
            // Part 0: Decode and get function value
            let (dest_tmp, global_idx, nargs_tmp) = decode_abc(instr);
            dest = dest_tmp;
            nargs = nargs_tmp;
            let idx = global_idx as usize;

            self.frames[current_frame_idx].ip = ip;

            // Get the current function value at this global index
            let func_value = if idx < self.globals_by_index.len() {
                self.globals_by_index[idx]
            } else {
                return Err(
                    self.runtime_error(RuntimeErrorKind::UndefinedVariable(format!(
                        "global index {}",
                        idx
                    ))),
                );
            };

            current_func_ptr = match func_value.as_ptr() {
                Some(p) => p,
                None => {
                    if func_value.is_null() {
                        // Try to get the global name for a better error message
                        let global_name = self.heap.get(func_ref).and_then(|obj| match &obj.kind {
                            ObjectKind::Function(f) => {
                                f.function.global_layout.names().get(idx).cloned()
                            }
                            ObjectKind::Closure(c) => {
                                self.heap.get(c.function).and_then(|inner_obj| {
                                    if let ObjectKind::Function(f) = &inner_obj.kind {
                                        f.function.global_layout.names().get(idx).cloned()
                                    } else {
                                        None
                                    }
                                })
                            }
                            _ => None,
                        });
                        if let Some(name) = global_name {
                            return Err(self.runtime_error_with_hint(
                                RuntimeErrorKind::UndefinedVariable(name.clone()),
                                &name,
                            ));
                        }
                    }
                    return Err(self.runtime_error(RuntimeErrorKind::NotCallable(
                        self.value_type_name(func_value).to_string(),
                    )));
                }
            };

            callee_ref = GcRef::new(current_func_ptr);

            let cache_key = crate::vm::core::InlineCacheKey {
                function: func_ref,
                instruction_pointer: ip - 1,
            };
            let global_generation = self.global_generations.get(idx).copied().unwrap_or(0);
            let cache_hit = self.inline_call_cache.get(&cache_key).is_some_and(|entry| {
                entry.global_index == idx
                    && entry.global_generation == global_generation
                    && entry.target == callee_ref
                    && self.heap.get(entry.target).is_some()
            });
            if cache_hit {
                if REPORT {
                    self.execution_stats.cache_hits =
                        self.execution_stats.cache_hits.saturating_add(1);
                }
            } else {
                if REPORT {
                    self.execution_stats.cache_misses =
                        self.execution_stats.cache_misses.saturating_add(1);
                }
                self.inline_call_cache.insert(
                    cache_key,
                    crate::vm::core::InlineCallCacheEntry {
                        global_index: idx,
                        global_generation,
                        target: callee_ref,
                    },
                );
            }

            // Part 1: Determine call type
            call_data = match self.heap.get(callee_ref) {
                Some(obj) => match &obj.kind {
                    ObjectKind::Function(func) => {
                        let bc = &func.function.bytecode;
                        let consts = &func.constants;
                        let arity = func.arity();
                        let num_regs = func.num_registers();
                        let callee_gmap =
                            self.global_mapping_id_for_layout(&func.function.global_layout);
                        let bc_ptr = bc.as_ptr();
                        let bc_len = bc.len();
                        let const_ptr = consts.as_ptr();
                        let const_len = consts.len();

                        CallData::Function {
                            arity,
                            callee_gmap,
                            num_regs,
                            bc_ptr,
                            bc_len,
                            const_ptr,
                            const_len,
                        }
                    }
                    ObjectKind::Native(native) => CallData::Native {
                        native: native.clone(),
                    },
                    ObjectKind::Closure(closure) => {
                        let inner_gmap =
                            self.heap
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

                        let inner_func = closure.function;
                        let arity = closure.arity;
                        let num_regs = closure.num_registers;
                        let bc_ptr = closure.bytecode_ptr;
                        let bc_len = closure.bytecode_len;
                        let const_ptr = closure.constants_ptr;
                        let const_len = closure.constants_len;

                        CallData::Closure {
                            arity,
                            callee_gmap: inner_gmap,
                            num_regs,
                            inner_func,
                            bc_ptr,
                            bc_len,
                            const_ptr,
                            const_len,
                            upval_ptr: closure.upvalues.as_ptr(),
                            upval_len: closure.upvalues.len(),
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

            // Part 2: Execute the call
            match call_data {
                CallData::Function {
                    arity,
                    callee_gmap,
                    num_regs,
                    bc_ptr,
                    bc_len,
                    const_ptr,
                    const_len,
                } => {
                    self.ensure_function_verified(callee_ref)?;
                    if arity != u16::from(nargs) {
                        return Err(self.runtime_error(RuntimeErrorKind::ArityMismatch {
                            expected: arity,
                            got: u16::from(nargs),
                        }));
                    }
                    if callee_gmap != 0 && callee_gmap != global_mapping_id {
                        if global_mapping_id != 0 {
                            self.sync_current_function_globals();
                        }
                        self.prepare_globals_for_function(callee_ref);
                    }
                    let new_base = base
                        .checked_add(dest as usize)
                        .and_then(|v| v.checked_add(1))
                        .ok_or_else(|| self.runtime_error(RuntimeErrorKind::StackOverflow))?;
                    let needed = new_base
                        .checked_add(num_regs as usize)
                        .ok_or_else(|| self.runtime_error(RuntimeErrorKind::StackOverflow))?;
                    if needed > self.registers.len() {
                        self.registers.resize(needed, Value::null());
                        regs_ptr = self.registers.as_mut_ptr();
                        let _ = regs_ptr;
                    }
                    let mut new_frame = CallFrame::with_return_dest(
                        callee_ref, new_base, dest, bc_ptr, bc_len, const_ptr, const_len, num_regs,
                    );
                    new_frame.global_mapping_id = callee_gmap;
                    if self.frames.len() >= crate::vm::MAX_FRAMES {
                        return Err(self.runtime_error(RuntimeErrorKind::StackOverflow));
                    }
                    self.frames.push(new_frame);
                    reload_frame_state!();
                }
                CallData::Native { native } => {
                    if native.arity != u16::from(nargs) {
                        return Err(self.runtime_error(RuntimeErrorKind::ArityMismatch {
                            expected: native.arity,
                            got: u16::from(nargs),
                        }));
                    }
                    let mut args = Vec::with_capacity(nargs as usize);
                    for i in 0..nargs {
                        args.push(reg_get!(base + dest as usize + 1 + i as usize));
                    }
                    match self.call_cached_native(&native, &args) {
                        Ok(result) => {
                            self.registers[base + dest as usize] = result;
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
                    bc_ptr,
                    bc_len,
                    const_ptr,
                    const_len,
                    upval_ptr,
                    upval_len,
                } => {
                    self.ensure_function_verified(inner_func)?;
                    if arity != u16::from(nargs) {
                        return Err(self.runtime_error(RuntimeErrorKind::ArityMismatch {
                            expected: arity,
                            got: u16::from(nargs),
                        }));
                    }
                    if callee_gmap != 0 && callee_gmap != global_mapping_id {
                        if global_mapping_id != 0 {
                            self.sync_current_function_globals();
                        }
                        self.prepare_globals_for_function(inner_func);
                    }
                    let new_base = base
                        .checked_add(dest as usize)
                        .and_then(|v| v.checked_add(1))
                        .ok_or_else(|| self.runtime_error(RuntimeErrorKind::StackOverflow))?;
                    let needed = new_base
                        .checked_add(num_regs as usize)
                        .ok_or_else(|| self.runtime_error(RuntimeErrorKind::StackOverflow))?;
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
                    return Err(self
                        .runtime_error(RuntimeErrorKind::NotCallable("non-callable".to_string())));
                }
            }
        }
