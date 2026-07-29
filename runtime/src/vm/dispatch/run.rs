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
        ) {
            (true, _) => self.run_fast_impl::<true, true>(),
            (false, true) => self.run_fast_impl::<false, true>(),
            (false, false) => self.run_fast_impl::<false, false>(),
        }
    }

    #[allow(unused_unsafe)]
    fn run_fast_impl<const REPORT: bool, const CONTROL: bool>(
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
                self.frames.pop();
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
                    include!("ops/load_store.rs");
                }

                // Arithmetic operations: Add(5), Sub(6), Mul(7), Div(8), Mod(9), Neg(10),
                // AddI(42), SubI(43), AddII(49)-ModII(53), AddFF(54)-ModFF(58),
                // AddIIG(82)-ModIIG(86), AddFFG(87)-ModFFG(91)
                5..=10 | 42..=43 | 49..=58 | 82..=91 => {
                    include!("ops/arithmetic.rs");
                }

                // Comparison operations: Eq(11), Ne(12), Lt(13), Le(14), Gt(15), Ge(16),
                // LtII(59)-NeII(64), LtFF(65)-NeFF(70), LtIImm(71)-GeIImm(74),
                // LtIIG(92)-NeIIG(97), LtFFG(98)-NeFFG(103)
                11..=16 | 59..=74 | 92..=103 => {
                    include!("ops/comparison.rs");
                }

                // Control flow operations: Not(17), Jump(18), JumpIf(19), JumpIfNot(20),
                // ForLoopI(40), ForLoopIInc(41), LtImm(44)-GeImm(47), WhileLoopLt(48),
                // StringForLoop(177), VecForLoop(178), ArrayForLoop(179)
                17..=20 | 26..=33 | 40..=41 | 44..=48 | 124..=125 | 127 | 177..=179 => {
                    include!("ops/control_flow.rs");
                }

                // Call operations: Call(21), Return(22), Return0(23), CallWide(34),
                // CallGlobal(77), CallCached(79), CallUpval(80), TailCallUpval(81)
                21..=23 | 34 | 77 | 79..=81 => {
                    include!("ops/calls.rs");
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
                        DispatchControl::Returned(_) | DispatchControl::ReturnToCaller { .. } => {
                            unreachable!("closure handler cannot change frames")
                        }
                    }
                }

                // Bitwise operations: Shl(105), Shr(106), BitAnd(107), BitOr(108), BitXor(109),
                // BitNot(110), ShlII(111)-XorII(115), NotI(116), ShlIImm(117)-XorIImm(121)
                105..=121 => {
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
                    include!("ops/arrays.rs");
                }

                184 => {
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
