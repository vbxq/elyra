// flat switch dispatch, hot state in locals
// FIXME: computed goto would be faster but Rust doesn't support it

use super::decode::{decode_abc, decode_aimm};
use super::state::{DispatchControl, DispatchState};
use crate::vm::{CallFrame, GcRef, MAX_REGISTERS, ObjectKind, VM, Value};
use aelys_bytecode::object::{AelysArray, AelysVec};
use aelys_common::error::{RuntimeError, RuntimeErrorKind};

impl VM {
    #[allow(unused_unsafe)]
    pub fn run_fast(&mut self) -> Result<Value, RuntimeError> {
        match (
            self.execution_control.report,
            self.execution_control_enabled(),
            self.jit_executor.is_some(),
        ) {
            (true, _, _) => self.run_fast_impl::<true, true, false>(),
            (false, true, _) => self.run_fast_impl::<false, true, false>(),
            (false, false, true) => self.run_fast_impl::<false, false, true>(),
            (false, false, false) => self.run_fast_impl::<false, false, false>(),
        }
    }

    #[allow(unused_unsafe)]
    fn run_fast_impl<const REPORT: bool, const CONTROL: bool, const JIT: bool>(
        &mut self,
    ) -> Result<Value, RuntimeError> {
        if self.frames.is_empty() {
            return Ok(Value::null());
        }

        const REGISTER_STACK_SIZE: usize = 16384;
        let frame_registers =
            usize::try_from(self.frames.last().unwrap().num_registers).map_err(|_| {
                self.runtime_error(RuntimeErrorKind::InvalidBytecode(
                    "register count does not fit this target".to_string(),
                ))
            })?;
        let required_registers = REGISTER_STACK_SIZE.max(frame_registers);
        if required_registers > MAX_REGISTERS {
            return Err(self.runtime_error(RuntimeErrorKind::InvalidRegister {
                reg: required_registers - 1,
                max: MAX_REGISTERS - 1,
            }));
        }
        if self.registers.capacity() < required_registers {
            self.registers
                .reserve(required_registers - self.registers.len());
        }
        if self.registers.len() < required_registers {
            self.registers.resize(required_registers, Value::null());
        }
        // SAFETY: Register stack is now fully allocated, pointers are stable

        // Load frame state into local variables for faster access
        let frame_idx = self.frames.len() - 1;
        let frame = &self.frames[frame_idx];
        let mut ip = frame.ip;
        let mut base = frame.base;
        let mut func_ref = frame.function;
        let mut bytecode_ptr = frame.bytecode_ptr;
        let mut bytecode_len = frame.bytecode_len;
        let mut constants_ptr = frame.constants_ptr;
        let mut constants_len = frame.constants_len;
        let mut upvalues_ptr = frame.upvalues_ptr;
        let mut upvalues_len = frame.upvalues_len;
        let mut current_frame_idx = frame_idx;
        let mut global_mapping_id = frame.global_mapping_id;

        macro_rules! reload_frame_state {
            () => {{
                current_frame_idx = self.frames.len() - 1;
                let frame = &self.frames[current_frame_idx];
                ip = frame.ip;
                base = frame.base;
                func_ref = frame.function;
                bytecode_ptr = frame.bytecode_ptr;
                bytecode_len = frame.bytecode_len;
                constants_ptr = frame.constants_ptr;
                constants_len = frame.constants_len;
                upvalues_ptr = frame.upvalues_ptr;
                upvalues_len = frame.upvalues_len;
                global_mapping_id = frame.global_mapping_id;
            }};
        }

        loop {
            // Check end of bytecode
            if ip >= bytecode_len {
                self.pop_frame_with_jit_metadata();
                if self.frames.is_empty() {
                    return Ok(Value::null());
                }
                let previous_gmap = global_mapping_id;
                // Reload frame state
                reload_frame_state!();
                if global_mapping_id != 0 && global_mapping_id != previous_gmap {
                    global_mapping_id = self.prepare_globals_for_function(func_ref);
                }
                continue;
            }

            if CONTROL {
                self.check_execution_control()?;
            }

            // Fetch instruction
            let instr = unsafe { *bytecode_ptr.add(ip) };
            ip += 1;

            let opcode_byte = u8::try_from(instr >> 24).expect("opcode occupies one byte");
            if REPORT {
                self.execution_stats.last_function = Some(func_ref);
                self.execution_stats.last_instruction_pointer = Some(ip - 1);
            }

            // Get registers pointer (may change after resize, but we refresh it for calls)
            let mut regs_ptr = self.registers.as_mut_ptr();
            let regs_len = self.registers.len();

            // Bounds-checking macro - returns RuntimeError for out-of-bounds access
            macro_rules! check_reg {
                ($idx:expr) => {{
                    let idx = $idx;
                    if idx >= regs_len {
                        self.frames[current_frame_idx].ip = ip;
                        return Err(self.runtime_error(RuntimeErrorKind::InvalidRegister {
                            reg: idx,
                            max: regs_len,
                        }));
                    }
                }};
            }

            #[allow(unused_unsafe)]
            macro_rules! reg_get {
                ($idx:expr) => {{
                    let idx = $idx;
                    check_reg!(idx);
                    // SAFETY: bounds checked above
                    unsafe { *regs_ptr.add(idx) }
                }};
            }
            #[allow(unused_unsafe)]
            macro_rules! reg_ref {
                ($idx:expr) => {{
                    let idx = $idx;
                    check_reg!(idx);
                    // SAFETY: bounds checked above
                    unsafe { &*regs_ptr.add(idx) }
                }};
            }
            #[allow(unused_unsafe)]
            macro_rules! reg_set {
                ($idx:expr, $val:expr) => {{
                    let idx = $idx;
                    check_reg!(idx);
                    // SAFETY: bounds checked above
                    unsafe {
                        *regs_ptr.add(idx) = $val;
                    }
                }};
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

            // Semantic dispatch - opcodes grouped by functionality
            match opcode_byte {
                // Load/Store operations: Move(0), LoadI(1), LoadK(2), LoadNull(3), LoadBool(4),
                // GetGlobalIdx(75), SetGlobalIdx(76)
                0..=4 | 75..=76 | 180 | 182..=183 => {
                    let state = DispatchState {
                        base,
                        constants: constants_ptr,
                        constants_len,
                        registers: regs_ptr,
                        registers_len: regs_len,
                        frame_index: current_frame_idx,
                    };
                    match self.execute_load_store(
                        &state,
                        &mut ip,
                        func_ref,
                        bytecode_ptr,
                        opcode_byte,
                        instr,
                    )? {
                        DispatchControl::Continue => {}
                        DispatchControl::ReloadFrame => {
                            unreachable!("load/store handler cannot change frames")
                        }
                        DispatchControl::Returned(_) | DispatchControl::ReturnToCaller { .. } => {
                            unreachable!("load/store handler cannot change frames")
                        }
                    }
                }

                39 => {
                    let (dest, source, global) = decode_abc(instr);
                    let left = self
                        .globals_by_index
                        .get(usize::from(global))
                        .copied()
                        .unwrap_or_else(Value::null)
                        .as_int_unchecked();
                    let right = reg_ref!(base + usize::from(source)).as_int_unchecked();
                    let result = int_value!(left.wrapping_add(right));
                    self.set_global_by_index(usize::from(global), result);
                    reg_set!(base + usize::from(dest), result);
                }

                // Arithmetic operations: Add(5), Sub(6), Mul(7), Div(8), Mod(9), Neg(10),
                // AddI(42), SubI(43), AddII(49)-ModII(53), AddFF(54)-ModFF(58),
                // AddIIG(82)-ModIIG(86), AddFFG(87)-ModFFG(91)
                5..=10 | 42..=43 | 49..=58 | 82..=91 => {
                    super::ops::arithmetic::execute_arithmetic!(
                        self,
                        opcode_byte,
                        instr,
                        base,
                        current_frame_idx,
                        ip,
                        reg_get,
                        reg_ref,
                        reg_set,
                        int_value
                    );
                }

                // Comparison operations: Eq(11), Ne(12), Lt(13), Le(14), Gt(15), Ge(16),
                // LtII(59)-NeII(64), LtFF(65)-NeFF(70), LtIImm(71)-GeIImm(74),
                // LtIIG(92)-NeIIG(97), LtFFG(98)-NeFFG(103)
                11..=16 | 59..=74 | 92..=103 => {
                    super::ops::comparison::execute_comparison!(
                        self,
                        opcode_byte,
                        instr,
                        base,
                        current_frame_idx,
                        ip,
                        reg_get,
                        reg_ref,
                        reg_set
                    );
                }

                // Control flow operations: Not(17), Jump(18), JumpIf(19), JumpIfNot(20),
                // ForLoopI(40), ForLoopIInc(41), LtImm(44)-GeImm(47), WhileLoopLt(48),
                // StringForLoop(177), VecForLoop(178), ArrayForLoop(179)
                17..=20 | 26..=33 | 40..=41 | 44..=48 | 124..=125 | 127 | 177..=179 => {
                    let branch_origin = ip;
                    super::ops::control_flow::execute_control_flow!(
                        self,
                        opcode_byte,
                        instr,
                        base,
                        current_frame_idx,
                        ip,
                        bytecode_ptr,
                        bytecode_len,
                        reg_get,
                        reg_ref,
                        reg_set,
                        int_value
                    );
                    if JIT
                        && ip < branch_origin
                        && let Some(result) = self.record_jit_backedge(func_ref, ip)
                    {
                        if let Some(result) = self.finish_jit_osr(result)? {
                            return Ok(result);
                        }
                        reload_frame_state!();
                    }
                }

                // Call operations: Call(21), Return(22), Return0(23), CallWide(34),
                // CallGlobal(77), CallCached(79), CallUpval(80), TailCallUpval(81)
                21..=23 | 34 | 77 | 79..=81 => {
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
                                    dest = u16::try_from(first >> 16)
                                        .expect("wide destination fits u16");
                                    func_reg = u16::try_from(first & 0xffff)
                                        .expect("wide callee fits u16");
                                    nargs =
                                        u16::try_from(second >> 16).expect("wide arity fits u16");
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
                                        return Err(self.runtime_error(
                                            RuntimeErrorKind::NotCallable(
                                                self.value_type_name(func_value).to_string(),
                                            ),
                                        ));
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
                                                callee_gmap: self.global_mapping_id_for_layout(
                                                    &func.function.global_layout,
                                                ),
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
                                        return Err(self.runtime_error(
                                            RuntimeErrorKind::NotCallable(
                                                "invalid reference".to_string(),
                                            ),
                                        ));
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
                                            return Err(self.runtime_error(
                                                RuntimeErrorKind::ArityMismatch {
                                                    expected: arity,
                                                    got: nargs,
                                                },
                                            ));
                                        }

                                        let mut jit_deopt = None;
                                        if JIT {
                                            let argument_start = base
                                                .checked_add(usize::from(func_reg))
                                                .and_then(|value| value.checked_add(1))
                                                .ok_or_else(|| {
                                                    self.runtime_error(
                                                        RuntimeErrorKind::StackOverflow,
                                                    )
                                                })?;
                                            match self.try_execute_jit_register_call(
                                                    callee_ref,
                                                    argument_start,
                                                    nargs,
                                                )? {
                                                crate::vm::jit::JitRegisterCallResult::Returned(result) => {
                                                    reg_set!(base + usize::from(dest), result);
                                                    continue;
                                                }
                                                crate::vm::jit::JitRegisterCallResult::Deoptimized { bytecode_ip, registers } => {
                                                    jit_deopt = Some((bytecode_ip, registers));
                                                }
                                                crate::vm::jit::JitRegisterCallResult::Unsupported => {}
                                            }
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
                                        let needed = new_base
                                            .checked_add(num_regs as usize)
                                            .ok_or_else(|| {
                                                self.runtime_error(RuntimeErrorKind::StackOverflow)
                                            })?;
                                        if needed > self.registers.len() {
                                            self.registers.resize(needed, Value::null());
                                            regs_ptr = self.registers.as_mut_ptr();
                                            let _ = regs_ptr;
                                        }

                                        let mut new_frame = CallFrame::with_return_dest(
                                            callee_ref, new_base, dest, bc_ptr, bc_len, const_ptr,
                                            const_len, num_regs,
                                        );
                                        if let Some((bytecode_ip, registers)) = jit_deopt {
                                            self.apply_jit_deoptimization(
                                                &mut new_frame,
                                                bytecode_ip,
                                                registers,
                                            )?;
                                        }
                                        new_frame.global_mapping_id = callee_gmap;

                                        if self.frames.len() >= crate::vm::MAX_FRAMES {
                                            return Err(
                                                self.runtime_error(RuntimeErrorKind::StackOverflow)
                                            );
                                        }
                                        self.frames.push(new_frame);
                                        reload_frame_state!();
                                    }

                                    CallData::Native { native } => {
                                        if native.arity != nargs {
                                            return Err(self.runtime_error(
                                                RuntimeErrorKind::ArityMismatch {
                                                    expected: native.arity,
                                                    got: nargs,
                                                },
                                            ));
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
                                            return Err(self.runtime_error(
                                                RuntimeErrorKind::ArityMismatch {
                                                    expected: arity,
                                                    got: nargs,
                                                },
                                            ));
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
                                        let needed = new_base
                                            .checked_add(num_regs as usize)
                                            .ok_or_else(|| {
                                                self.runtime_error(RuntimeErrorKind::StackOverflow)
                                            })?;
                                        if needed > self.registers.len() {
                                            self.registers.resize(needed, Value::null());
                                            regs_ptr = self.registers.as_mut_ptr();
                                            let _ = regs_ptr;
                                        }

                                        let mut new_frame = CallFrame::with_upvalues(
                                            inner_func, new_base, dest, bc_ptr, bc_len, const_ptr,
                                            const_len, upval_ptr, upval_len, num_regs,
                                        );
                                        new_frame.global_mapping_id = callee_gmap;

                                        if self.frames.len() >= crate::vm::MAX_FRAMES {
                                            return Err(
                                                self.runtime_error(RuntimeErrorKind::StackOverflow)
                                            );
                                        }
                                        self.frames.push(new_frame);
                                        reload_frame_state!();
                                    }

                                    CallData::Invalid => {
                                        return Err(self.runtime_error(
                                            RuntimeErrorKind::NotCallable(
                                                "non-callable object".to_string(),
                                            ),
                                        ));
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

                            self.pop_frame_with_jit_metadata();

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

                            self.pop_frame_with_jit_metadata();

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
                                    return Err(self.runtime_error(
                                        RuntimeErrorKind::UndefinedVariable(format!(
                                            "global index {}",
                                            idx
                                        )),
                                    ));
                                };

                                current_func_ptr = match func_value.as_ptr() {
                                    Some(p) => p,
                                    None => {
                                        if func_value.is_null() {
                                            // Try to get the global name for a better error message
                                            let global_name =
                                                self.heap.get(func_ref).and_then(|obj| match &obj
                                                    .kind
                                                {
                                                    ObjectKind::Function(f) => f
                                                        .function
                                                        .global_layout
                                                        .names()
                                                        .get(idx)
                                                        .cloned(),
                                                    ObjectKind::Closure(c) => self
                                                        .heap
                                                        .get(c.function)
                                                        .and_then(|inner_obj| {
                                                            if let ObjectKind::Function(f) =
                                                                &inner_obj.kind
                                                            {
                                                                f.function
                                                                    .global_layout
                                                                    .names()
                                                                    .get(idx)
                                                                    .cloned()
                                                            } else {
                                                                None
                                                            }
                                                        }),
                                                    _ => None,
                                                });
                                            if let Some(name) = global_name {
                                                return Err(self.runtime_error_with_hint(
                                                    RuntimeErrorKind::UndefinedVariable(
                                                        name.clone(),
                                                    ),
                                                    &name,
                                                ));
                                            }
                                        }
                                        return Err(self.runtime_error(
                                            RuntimeErrorKind::NotCallable(
                                                self.value_type_name(func_value).to_string(),
                                            ),
                                        ));
                                    }
                                };

                                callee_ref = GcRef::new(current_func_ptr);

                                let cache_key = crate::vm::core::InlineCacheKey {
                                    function: func_ref,
                                    instruction_pointer: ip - 1,
                                };
                                let global_generation =
                                    self.global_generations.get(idx).copied().unwrap_or(0);
                                let cache_hit = self.probe_inline_call_cache(
                                    cache_key,
                                    idx,
                                    global_generation,
                                    callee_ref,
                                );
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
                                }

                                // Part 1: Determine call type
                                call_data = match self.heap.get(callee_ref) {
                                    Some(obj) => match &obj.kind {
                                        ObjectKind::Function(func) => {
                                            let bc = &func.function.bytecode;
                                            let consts = &func.constants;
                                            let arity = func.arity();
                                            let num_regs = func.num_registers();
                                            let callee_gmap = self.global_mapping_id_for_layout(
                                                &func.function.global_layout,
                                            );
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
                                        return Err(self.runtime_error(
                                            RuntimeErrorKind::NotCallable(
                                                "invalid reference".to_string(),
                                            ),
                                        ));
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
                                            return Err(self.runtime_error(
                                                RuntimeErrorKind::ArityMismatch {
                                                    expected: arity,
                                                    got: u16::from(nargs),
                                                },
                                            ));
                                        }
                                        let mut jit_deopt = None;
                                        if JIT {
                                            let argument_start = base
                                                .checked_add(usize::from(dest))
                                                .and_then(|value| value.checked_add(1))
                                                .ok_or_else(|| {
                                                    self.runtime_error(
                                                        RuntimeErrorKind::StackOverflow,
                                                    )
                                                })?;
                                            match self.try_execute_jit_register_call(
                                                    callee_ref,
                                                    argument_start,
                                                    u16::from(nargs),
                                                )? {
                                                crate::vm::jit::JitRegisterCallResult::Returned(result) => {
                                                    reg_set!(base + usize::from(dest), result);
                                                    continue;
                                                }
                                                crate::vm::jit::JitRegisterCallResult::Deoptimized { bytecode_ip, registers } => {
                                                    jit_deopt = Some((bytecode_ip, registers));
                                                }
                                                crate::vm::jit::JitRegisterCallResult::Unsupported => {}
                                            }
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
                                            .ok_or_else(|| {
                                                self.runtime_error(RuntimeErrorKind::StackOverflow)
                                            })?;
                                        let needed = new_base
                                            .checked_add(num_regs as usize)
                                            .ok_or_else(|| {
                                                self.runtime_error(RuntimeErrorKind::StackOverflow)
                                            })?;
                                        if needed > self.registers.len() {
                                            self.registers.resize(needed, Value::null());
                                            regs_ptr = self.registers.as_mut_ptr();
                                            let _ = regs_ptr;
                                        }
                                        let mut new_frame = CallFrame::with_return_dest(
                                            callee_ref, new_base, dest, bc_ptr, bc_len, const_ptr,
                                            const_len, num_regs,
                                        );
                                        if let Some((bytecode_ip, registers)) = jit_deopt {
                                            self.apply_jit_deoptimization(
                                                &mut new_frame,
                                                bytecode_ip,
                                                registers,
                                            )?;
                                        }
                                        new_frame.global_mapping_id = callee_gmap;
                                        if self.frames.len() >= crate::vm::MAX_FRAMES {
                                            return Err(
                                                self.runtime_error(RuntimeErrorKind::StackOverflow)
                                            );
                                        }
                                        self.frames.push(new_frame);
                                        reload_frame_state!();
                                    }
                                    CallData::Native { native } => {
                                        if native.arity != u16::from(nargs) {
                                            return Err(self.runtime_error(
                                                RuntimeErrorKind::ArityMismatch {
                                                    expected: native.arity,
                                                    got: u16::from(nargs),
                                                },
                                            ));
                                        }
                                        let mut args = Vec::with_capacity(nargs as usize);
                                        for i in 0..nargs {
                                            args.push(reg_get!(
                                                base + dest as usize + 1 + i as usize
                                            ));
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
                                            return Err(self.runtime_error(
                                                RuntimeErrorKind::ArityMismatch {
                                                    expected: arity,
                                                    got: u16::from(nargs),
                                                },
                                            ));
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
                                            .ok_or_else(|| {
                                                self.runtime_error(RuntimeErrorKind::StackOverflow)
                                            })?;
                                        let needed = new_base
                                            .checked_add(num_regs as usize)
                                            .ok_or_else(|| {
                                                self.runtime_error(RuntimeErrorKind::StackOverflow)
                                            })?;
                                        if needed > self.registers.len() {
                                            self.registers.resize(needed, Value::null());
                                            regs_ptr = self.registers.as_mut_ptr();
                                            let _ = regs_ptr;
                                        }
                                        let mut new_frame = CallFrame::with_upvalues(
                                            inner_func, new_base, dest, bc_ptr, bc_len, const_ptr,
                                            const_len, upval_ptr, upval_len, num_regs,
                                        );
                                        new_frame.global_mapping_id = callee_gmap;
                                        if self.frames.len() >= crate::vm::MAX_FRAMES {
                                            return Err(
                                                self.runtime_error(RuntimeErrorKind::StackOverflow)
                                            );
                                        }
                                        self.frames.push(new_frame);
                                        reload_frame_state!();
                                    }
                                    CallData::Invalid => {
                                        return Err(self.runtime_error(
                                            RuntimeErrorKind::NotCallable(
                                                "non-callable".to_string(),
                                            ),
                                        ));
                                    }
                                }
                            }
                        }

                        // CallCached (79) - Call with function in register
                        79 => {
                            let state = DispatchState {
                                base,
                                constants: constants_ptr,
                                constants_len,
                                registers: regs_ptr,
                                registers_len: regs_len,
                                frame_index: current_frame_idx,
                            };
                            match super::ops::call_cached::execute(
                                self,
                                &state,
                                ip,
                                global_mapping_id,
                                instr,
                            )? {
                                DispatchControl::Continue => {}
                                DispatchControl::ReloadFrame => reload_frame_state!(),
                                DispatchControl::Returned(_)
                                | DispatchControl::ReturnToCaller { .. } => {
                                    unreachable!(
                                        "cached call handler cannot return from the active frame"
                                    )
                                }
                            }
                        }

                        // CallUpval (80) - Call function from upvalue
                        80 => {
                            let state = DispatchState {
                                base,
                                constants: constants_ptr,
                                constants_len,
                                registers: regs_ptr,
                                registers_len: regs_len,
                                frame_index: current_frame_idx,
                            };
                            match super::ops::call_upval::execute(
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
                                    unreachable!("upvalue call handler must enter a new frame")
                                }
                            }
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
                }

                // Global variable operations: GetGlobal(24), SetGlobal(25)
                24..=25 => {
                    let state = DispatchState {
                        base,
                        constants: constants_ptr,
                        constants_len,
                        registers: regs_ptr,
                        registers_len: regs_len,
                        frame_index: current_frame_idx,
                    };
                    super::ops::globals::execute(self, &state, ip, opcode_byte, instr)?;
                }

                // Closure operations: MakeClosure(35), GetUpval(36),
                // SetUpval(37), CloseUpvals(38)
                35..=38 | 126 | 181 => {
                    let state = DispatchState {
                        base,
                        constants: constants_ptr,
                        constants_len,
                        registers: regs_ptr,
                        registers_len: regs_len,
                        frame_index: current_frame_idx,
                    };
                    match super::ops::closures::execute(
                        self,
                        &state,
                        &mut ip,
                        func_ref,
                        bytecode_ptr,
                        upvalues_ptr,
                        upvalues_len,
                        opcode_byte,
                        instr,
                    )? {
                        DispatchControl::Continue => {}
                        DispatchControl::ReloadFrame => {
                            unreachable!("closure handler cannot change frames")
                        }
                        DispatchControl::Returned(_) | DispatchControl::ReturnToCaller { .. } => {
                            unreachable!("closure handler cannot change frames")
                        }
                    }
                }

                // Bitwise operations: Shl(105), Shr(106), BitAnd(107), BitOr(108), BitXor(109),
                // BitNot(110), ShlII(111)-XorII(115), NotI(116), ShlIImm(117)-XorIImm(121)
                109 => {
                    super::ops::bitwise::execute_xor(
                        self,
                        ip,
                        base,
                        current_frame_idx,
                        regs_ptr,
                        regs_len,
                        instr,
                    )?;
                }
                105..=108 | 110..=121 => {
                    super::ops::bitwise::execute(
                        self,
                        ip,
                        base,
                        current_frame_idx,
                        regs_ptr,
                        regs_len,
                        opcode_byte,
                        instr,
                    )?;
                }

                // Array, Vec, and String operations: wide literals 122-123, compact 130-176
                122..=123 | 130..=176 => {
                    super::ops::arrays::execute_arrays!(
                        self,
                        opcode_byte,
                        instr,
                        base,
                        current_frame_idx,
                        ip,
                        bytecode_ptr,
                        reg_get,
                        reg_set
                    );
                }

                184 => {
                    let branch_origin = ip;
                    let active_function = func_ref;
                    let wide_opcode =
                        u8::try_from((instr >> 16) & 0xff).expect("wide opcode occupies one byte");
                    let state = DispatchState {
                        base,
                        constants: constants_ptr,
                        constants_len,
                        registers: regs_ptr,
                        registers_len: regs_len,
                        frame_index: current_frame_idx,
                    };
                    match self.execute_wide(
                        &state,
                        &mut ip,
                        func_ref,
                        bytecode_ptr,
                        upvalues_ptr,
                        upvalues_len,
                        instr,
                    )? {
                        DispatchControl::Continue => {}
                        DispatchControl::ReloadFrame => reload_frame_state!(),
                        DispatchControl::Returned(value) => return Ok(value),
                        DispatchControl::ReturnToCaller {
                            destination,
                            value,
                            switch_globals,
                        } => {
                            reload_frame_state!();
                            if switch_globals {
                                global_mapping_id = self.prepare_globals_for_function(func_ref);
                            }
                            regs_ptr = self.registers.as_mut_ptr();
                            reg_set!(base + usize::from(destination), value);
                        }
                    }
                    if JIT
                        && wide_opcode == 48
                        && ip < branch_origin
                        && let Some(result) = self.record_jit_backedge(active_function, ip)
                    {
                        if let Some(result) = self.finish_jit_osr(result)? {
                            return Ok(result);
                        }
                        reload_frame_state!();
                    }
                }

                _ => {
                    self.frames[current_frame_idx].ip = ip;
                    return Err(self.runtime_error(RuntimeErrorKind::InvalidOpcode {
                        opcode: opcode_byte,
                    }));
                }
            }
        }
    }
}
