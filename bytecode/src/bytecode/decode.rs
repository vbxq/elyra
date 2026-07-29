use super::opcode::OpCode;

// instruction formats:
//   A: op(8) | a(8) | b(8) | c(8)   - 3 regs
//   B: op(8) | a(8) | imm(16)       - reg + signed immediate
//   C: same as A                    - just different semantics (call)

pub fn decode_a(instr: u32) -> (OpCode, u8, u8, u8) {
    let opcode = u8::try_from(instr >> 24).expect("opcode occupies one byte");
    let op = OpCode::from_u8(opcode).unwrap_or(OpCode::Move);
    (
        op,
        u8::try_from((instr >> 16) & 0xFF).expect("operand occupies one byte"),
        u8::try_from((instr >> 8) & 0xFF).expect("operand occupies one byte"),
        u8::try_from(instr & 0xFF).expect("operand occupies one byte"),
    )
}

pub fn decode_b(instr: u32) -> (OpCode, u8, i16) {
    let opcode = u8::try_from(instr >> 24).expect("opcode occupies one byte");
    let op = OpCode::from_u8(opcode).unwrap_or(OpCode::Move);
    let a = u8::try_from((instr >> 16) & 0xFF).expect("operand occupies one byte");
    let immediate = u16::try_from(instr & 0xFFFF).expect("immediate occupies two bytes");
    (op, a, i16::from_ne_bytes(immediate.to_ne_bytes()))
}

pub fn decode_c(instr: u32) -> (OpCode, u8, u8, u8) {
    decode_a(instr)
}
