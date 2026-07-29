use crate::vm::{VM, Value};
use aelys_common::error::{RuntimeError, RuntimeErrorKind};

pub(super) enum DispatchControl {
    Continue,
    Returned(Value),
    ReturnToCaller {
        destination: u16,
        value: Value,
        switch_globals: bool,
    },
}

pub(super) struct DispatchState {
    pub(super) base: usize,
    pub(super) constants: *const Value,
    pub(super) constants_len: usize,
    pub(super) registers: *mut Value,
    pub(super) registers_len: usize,
    pub(super) frame_index: usize,
}

impl DispatchState {
    #[inline(always)]
    pub(super) fn read_register(
        &self,
        vm: &mut VM,
        index: usize,
        ip: usize,
    ) -> Result<Value, RuntimeError> {
        if index >= self.registers_len {
            vm.frames[self.frame_index].ip = ip;
            return Err(vm.runtime_error(RuntimeErrorKind::InvalidRegister {
                reg: index,
                max: self.registers_len,
            }));
        }
        // SAFETY: index was checked and the register allocation is stable during this handler.
        Ok(unsafe { *self.registers.add(index) })
    }

    #[inline(always)]
    pub(super) fn write_register(
        &self,
        vm: &mut VM,
        index: usize,
        value: Value,
        ip: usize,
    ) -> Result<(), RuntimeError> {
        if index >= self.registers_len {
            vm.frames[self.frame_index].ip = ip;
            return Err(vm.runtime_error(RuntimeErrorKind::InvalidRegister {
                reg: index,
                max: self.registers_len,
            }));
        }
        // SAFETY: index was checked and the register allocation is stable during this handler.
        unsafe {
            *self.registers.add(index) = value;
        }
        Ok(())
    }

    #[inline(always)]
    pub(super) fn constant(&self, index: usize) -> Option<Value> {
        if index >= self.constants_len {
            return None;
        }
        // SAFETY: index was checked against the materialized constant count.
        Some(unsafe { *self.constants.add(index) })
    }

    #[inline(always)]
    pub(super) fn save_ip(&self, vm: &mut VM, ip: usize) {
        vm.frames[self.frame_index].ip = ip;
    }
}
