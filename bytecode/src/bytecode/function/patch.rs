use super::Function;
use crate::bytecode::OpCode;

impl Function {
    /// Patch a jump instruction at the given offset
    pub fn patch_jump(&mut self, offset: usize) {
        self.patch_jump_to(offset, self.bytecode_builder.len());
    }

    pub fn patch_jump_to(&mut self, offset: usize, target: usize) {
        let opcode = u8::try_from(self.bytecode_builder[offset] >> 24)
            .expect("opcode byte is bounded by instruction encoding");
        if matches!(
            OpCode::from_u8(opcode),
            Some(OpCode::JumpIfWideLong | OpCode::JumpIfNotWideLong)
        ) {
            let Some(next) = offset.checked_add(3) else {
                self.jump_overflow = Some(usize::MAX);
                return;
            };
            let distance = if target >= next {
                target
                    .checked_sub(next)
                    .and_then(|value| i32::try_from(value).ok())
            } else {
                next.checked_sub(target)
                    .and_then(|value| i32::try_from(value).ok())
                    .and_then(i32::checked_neg)
            };
            let Some(distance) = distance else {
                self.jump_overflow = Some(target.abs_diff(next));
                return;
            };
            self.bytecode_builder[offset + 2] = u32::from_ne_bytes(distance.to_ne_bytes());
            return;
        }
        if matches!(
            OpCode::from_u8(opcode),
            Some(OpCode::JumpLong | OpCode::JumpIfLong | OpCode::JumpIfNotLong)
        ) {
            let Some(next) = offset.checked_add(2) else {
                self.jump_overflow = Some(usize::MAX);
                return;
            };
            let distance = if target >= next {
                target
                    .checked_sub(next)
                    .and_then(|value| i32::try_from(value).ok())
            } else {
                next.checked_sub(target)
                    .and_then(|value| i32::try_from(value).ok())
                    .and_then(i32::checked_neg)
            };
            let Some(distance) = distance else {
                self.jump_overflow = Some(target.abs_diff(next));
                return;
            };
            self.bytecode_builder[offset + 1] = u32::from_ne_bytes(distance.to_ne_bytes());
            return;
        }

        let Some(next) = offset.checked_add(1) else {
            self.jump_overflow = Some(usize::MAX);
            return;
        };
        let jump_dist = if target >= next {
            target
                .checked_sub(next)
                .and_then(|value| i16::try_from(value).ok())
        } else {
            next.checked_sub(target)
                .and_then(|value| i16::try_from(value).ok())
                .and_then(i16::checked_neg)
        };
        let Some(jump_dist) = jump_dist else {
            self.jump_overflow = Some(target.abs_diff(next));
            return;
        };
        let instr = self.bytecode_builder[offset];
        let op = instr >> 24;
        let a = (instr >> 16) & 0xFF;
        let encoded = u16::from_ne_bytes(jump_dist.to_ne_bytes());
        self.bytecode_builder[offset] = (op << 24) | (a << 16) | u32::from(encoded);
    }
}
