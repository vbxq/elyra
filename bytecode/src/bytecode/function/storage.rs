use super::Function;
use crate::bytecode::{BytecodeBuffer, OpCode};

impl Function {
    /// Set bytecode directly from a Vec (for assembler/binary loading).
    pub fn set_bytecode(&mut self, bytecode: Vec<u32>) {
        self.bytecode = BytecodeBuffer::from_vec(bytecode);
        self.bytecode_builder.clear();
    }

    /// Push a raw instruction to the bytecode builder.
    pub fn push_raw(&mut self, instr: u32) {
        self.bytecode_builder.push(instr);
    }

    /// Get a mutable reference to a bytecode instruction at the given index.
    pub fn bytecode_mut(&mut self, index: usize) -> &mut u32 {
        &mut self.bytecode_builder[index]
    }

    /// Get an immutable reference to a bytecode instruction at the given index.
    pub fn bytecode_at(&self, index: usize) -> u32 {
        self.bytecode_builder[index]
    }

    /// Get current instruction count (for jump patching)
    pub fn current_offset(&self) -> usize {
        self.bytecode_builder.len()
    }

    /// Emit a jump and return its offset for later patching
    pub fn emit_jump(&mut self, op: OpCode, line: u32) -> usize {
        let offset = self.current_offset();
        let long_op = match op {
            OpCode::Jump => OpCode::JumpLong,
            other => other,
        };
        self.emit_a(long_op, 0, 0, 0, line);
        self.push_raw(0);
        self.record_lines(1, line);
        offset
    }

    /// Emit a conditional jump and return its offset
    pub fn emit_jump_if(&mut self, op: OpCode, reg: u16, line: u32) -> usize {
        let offset = self.current_offset();
        if u8::try_from(reg).is_err() {
            let wide_op = match op {
                OpCode::JumpIf => OpCode::JumpIfWideLong,
                OpCode::JumpIfNot => OpCode::JumpIfNotWideLong,
                other => other,
            };
            self.emit_a(wide_op, 0, 0, 0, line);
            self.push_raw(u32::from(reg) << 16);
            self.push_raw(0);
            self.record_lines(2, line);
            return offset;
        }
        let long_op = match op {
            OpCode::JumpIf => OpCode::JumpIfLong,
            OpCode::JumpIfNot => OpCode::JumpIfNotLong,
            other => other,
        };
        self.emit_a(
            long_op,
            u8::try_from(reg).expect("register was range checked"),
            0,
            0,
            line,
        );
        self.push_raw(0);
        self.record_lines(1, line);
        offset
    }

    pub fn emit_jump_back(&mut self, target: usize, line: u32) {
        let offset = self.current_offset();
        let Some(delta) = target
            .checked_sub(offset.saturating_add(2))
            .and_then(|distance| i32::try_from(distance).ok())
            .or_else(|| {
                offset
                    .checked_add(2)
                    .and_then(|next| next.checked_sub(target))
                    .and_then(|distance| i32::try_from(distance).ok())
                    .and_then(i32::checked_neg)
            })
        else {
            self.jump_overflow = Some(offset.abs_diff(target));
            return;
        };
        self.emit_a(OpCode::JumpLong, 0, 0, 0, line);
        self.push_raw(u32::from_ne_bytes(delta.to_ne_bytes()));
        self.record_lines(1, line);
    }

    pub fn emit_loop_back(
        &mut self,
        short_op: OpCode,
        long_op: OpCode,
        register: u8,
        target: usize,
        line: u32,
    ) {
        let offset = self.current_offset();
        let short_delta = offset
            .checked_add(1)
            .and_then(|next| next.checked_sub(target))
            .and_then(|distance| i16::try_from(distance).ok())
            .and_then(i16::checked_neg);
        if let Some(delta) = short_delta {
            self.emit_b(short_op, register, delta, line);
            return;
        }

        let Some(delta) = offset
            .checked_add(2)
            .and_then(|next| next.checked_sub(target))
            .and_then(|distance| i32::try_from(distance).ok())
            .and_then(i32::checked_neg)
        else {
            self.jump_overflow = Some(offset.abs_diff(target));
            return;
        };
        self.emit_a(long_op, register, 0, 0, line);
        self.push_raw(u32::from_ne_bytes(delta.to_ne_bytes()));
        self.record_lines(1, line);
    }

    pub fn emit_wide_loop_back(
        &mut self,
        inner_op: OpCode,
        register: u16,
        target: usize,
        line: u32,
    ) {
        let offset = self.current_offset();
        let Some(delta) = offset
            .checked_add(3)
            .and_then(|next| next.checked_sub(target))
            .and_then(|distance| i32::try_from(distance).ok())
            .and_then(i32::checked_neg)
        else {
            self.jump_overflow = Some(offset.abs_diff(target));
            return;
        };
        self.emit_a(OpCode::LoopWideLong, u8::from(inner_op), 0, 0, line);
        self.push_raw(u32::from(register) << 16);
        self.push_raw(u32::from_ne_bytes(delta.to_ne_bytes()));
        self.record_lines(2, line);
    }
}
