/// Decode ABC format instruction.
#[inline(always)]
pub(super) fn decode_abc(instr: u32) -> (u8, u8, u8) {
    let a = u8::try_from((instr >> 16) & 0xFF).expect("operand is masked to one byte");
    let b = u8::try_from((instr >> 8) & 0xFF).expect("operand is masked to one byte");
    let c = u8::try_from(instr & 0xFF).expect("operand is masked to one byte");
    (a, b, c)
}

/// Decode A + imm16 format instruction.
#[inline(always)]
pub(super) fn decode_aimm(instr: u32) -> (u8, i16) {
    let a = u8::try_from((instr >> 16) & 0xFF).expect("operand is masked to one byte");
    let bits = u16::try_from(instr & 0xFFFF).expect("immediate is masked to two bytes");
    let imm = i16::from_ne_bytes(bits.to_ne_bytes());
    (a, imm)
}
