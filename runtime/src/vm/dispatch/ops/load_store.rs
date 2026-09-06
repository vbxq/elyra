use super::super::decode::{decode_abc, decode_aimm};
use super::super::state::{DispatchControl, DispatchState};
use crate::vm::{GcRef, VM, Value};
use aelys_common::error::{RuntimeError, RuntimeErrorKind};

impl VM {
    #[allow(clippy::too_many_arguments)]
    #[inline(always)]
    pub(in crate::vm::dispatch) fn execute_load_store(
        &mut self,
        state: &DispatchState,
        instruction_pointer: &mut usize,
        func_ref: GcRef,
        bytecode_ptr: *const u32,
        opcode_byte: u8,
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


        match opcode_byte {
            0 => {
                let (a, b, _) = decode_abc(instr);
                let val = reg_get!(base + b as usize);
                reg_set!(base + a as usize, val);
            }

            1 => {
                let (a, imm) = decode_aimm(instr);
                reg_set!(base + a as usize, Value::int(imm as i64));
            }

            2 => {
                let (a, imm) = decode_aimm(instr);
                let k = usize::from(u16::from_ne_bytes(imm.to_ne_bytes()));

                if k >= constants_len {
                    self.frames[current_frame_idx].ip = ip;
                    return Err(
                        self.runtime_error(RuntimeErrorKind::InvalidBytecode(format!(
                            "invalid constant index: {} (max: {})",
                            k, constants_len
                        ))),
                    );
                }
                let constant = unsafe { *constants_ptr.add(k) };

                if let Some(func_idx) = constant.as_nested_fn_marker() {
                    self.frames[current_frame_idx].ip = ip;
                    match self.get_nested_function(func_ref, func_idx) {
                        Ok(nested_func) => {
                            self.verify_function_value(&nested_func)?;
                            let func_obj_ref = self.alloc_function(nested_func)?;
                            self.inherit_jit_key(func_ref, func_obj_ref, func_idx);
                            reg_set!(base + a as usize, Value::ptr(func_obj_ref.index()));
                        }
                        Err(e) => return Err(e),
                    }
                } else {
                    reg_set!(base + a as usize, constant);
                }
            }

            180 => {
                let (a, _, _) = decode_abc(instr);
                let index = unsafe { *bytecode_ptr.add(ip) } as usize;
                ip += 1;
                if index >= constants_len {
                    self.frames[current_frame_idx].ip = ip;
                    return Err(
                        self.runtime_error(RuntimeErrorKind::InvalidBytecode(format!(
                            "constant index {index} out of bounds"
                        ))),
                    );
                }
                let constant = unsafe { *constants_ptr.add(index) };
                if let Some(func_idx) = constant.as_nested_fn_marker() {
                    self.frames[current_frame_idx].ip = ip;
                    match self.get_nested_function(func_ref, func_idx) {
                        Ok(nested_func) => {
                            self.verify_function_value(&nested_func)?;
                            let func_obj_ref = self.alloc_function(nested_func)?;
                            self.inherit_jit_key(func_ref, func_obj_ref, func_idx);
                            reg_set!(base + a as usize, Value::ptr(func_obj_ref.index()));
                        }
                        Err(error) => return Err(error),
                    }
                } else {
                    reg_set!(base + a as usize, constant);
                }
            }

            3 => {
                let (a, _, _) = decode_abc(instr);
                reg_set!(base + a as usize, Value::null());
            }

            4 => {
                let (a, b, _) = decode_abc(instr);
                reg_set!(base + a as usize, Value::bool(b != 0));
            }

            75 => {
                let (a, imm) = decode_aimm(instr);
                let idx = usize::from(u16::from_ne_bytes(imm.to_ne_bytes()));
                let Some(value) = self.globals_by_index.get(idx).copied() else {
                    self.frames[current_frame_idx].ip = ip;
                    return Err(self.global_index_error("GetGlobalIdx", idx));
                };
                reg_set!(base + a as usize, value);
            }

            76 => {
                let (a, imm) = decode_aimm(instr);
                let idx = usize::from(u16::from_ne_bytes(imm.to_ne_bytes()));
                let value = reg_get!(base + a as usize);
                if let Err(error) = self.set_global_by_index_checked(idx, value) {
                    self.frames[current_frame_idx].ip = ip;
                    return Err(error);
                }
            }

            182 => {
                let (register, _, _) = decode_abc(instr);
                let index = unsafe { *bytecode_ptr.add(ip) } as usize;
                ip += 1;
                let Some(value) = self.globals_by_index.get(index).copied() else {
                    self.frames[current_frame_idx].ip = ip;
                    return Err(self.global_index_error("GetGlobalIdxWide", index));
                };
                reg_set!(base + register as usize, value);
            }

            183 => {
                let (register, _, _) = decode_abc(instr);
                let index = unsafe { *bytecode_ptr.add(ip) } as usize;
                ip += 1;
                let value = reg_get!(base + register as usize);
                if let Err(error) = self.set_global_by_index_checked(index, value) {
                    self.frames[current_frame_idx].ip = ip;
                    return Err(error);
                }
            }

            _ => unreachable!(),
        }
        *instruction_pointer = ip;
        Ok(DispatchControl::Continue)
    }
}
