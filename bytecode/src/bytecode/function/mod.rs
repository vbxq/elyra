use super::buffer::BytecodeBuffer;
use super::global_layout::GlobalLayout;
use super::opcode::OpCode;
use super::schema::{EnumSchema, SchemaId, StructSchema};
use super::upvalue::UpvalueDescriptor;
use crate::bytecode::Constant;
use std::sync::Arc;

mod constants;
mod lines;
mod patch;
mod registers;
mod storage;

#[derive(Debug, Clone)]
pub struct Function {
    pub name: Option<String>,
    pub arity: u16,
    pub num_registers: u32,
    pub bytecode: BytecodeBuffer,
    bytecode_builder: Vec<u32>, // temp storage during compilation
    jump_overflow: Option<usize>,
    wide_operand_error: Option<OpCode>,
    pub constants: Vec<Constant>,
    pub nested_functions: Vec<Function>,
    pub upvalue_descriptors: Vec<UpvalueDescriptor>,
    pub lines: Vec<(u16, u32)>,
    pub global_layout: Arc<GlobalLayout>,
    pub global_layout_hash: u64,
    pub struct_schemas: Vec<StructSchema>,
    pub enum_schemas: Vec<EnumSchema>,
    pub schema_ids: Vec<SchemaId>,
    pub jit_unsupported_struct: bool,
}

impl Function {
    pub fn new(name: Option<String>, arity: u16) -> Self {
        Self {
            name,
            arity,
            num_registers: 0,
            bytecode: BytecodeBuffer::empty(),
            bytecode_builder: Vec::new(),
            jump_overflow: None,
            wide_operand_error: None,
            constants: Vec::new(),
            nested_functions: Vec::new(),
            upvalue_descriptors: Vec::new(),
            lines: Vec::new(),
            global_layout: GlobalLayout::empty(),
            global_layout_hash: 0,
            struct_schemas: Vec::new(),
            enum_schemas: Vec::new(),
            schema_ids: Vec::new(),
            jit_unsupported_struct: false,
        }
    }

    pub fn jump_overflow(&self) -> Option<usize> {
        self.jump_overflow.or_else(|| {
            self.nested_functions
                .iter()
                .find_map(Function::jump_overflow)
        })
    }

    pub fn wide_operand_error(&self) -> Option<OpCode> {
        self.wide_operand_error.or_else(|| {
            self.nested_functions
                .iter()
                .find_map(Function::wide_operand_error)
        })
    }

    pub fn record_wide_operand_error(&mut self, op: OpCode) {
        self.wide_operand_error.get_or_insert(op);
    }

    pub fn finalize_bytecode(&mut self) {
        if !self.bytecode_builder.is_empty() {
            self.bytecode = BytecodeBuffer::from_vec(std::mem::take(&mut self.bytecode_builder));
        }
        let needed = registers::required_registers(self.bytecode.as_slice());
        if needed > self.num_registers as usize {
            self.num_registers = u32::try_from(needed).unwrap_or(u32::MAX);
        }
        for f in &mut self.nested_functions {
            f.finalize_bytecode();
        }
    }

    pub fn emit_a(&mut self, op: OpCode, a: u8, b: u8, c: u8, line: u32) {
        self.emit_raw(
            (u32::from(u8::from(op)) << 24)
                | (u32::from(a) << 16)
                | (u32::from(b) << 8)
                | u32::from(c),
            line,
        );
    }

    pub fn emit_b(&mut self, op: OpCode, a: u8, imm: i16, line: u32) {
        self.emit_raw(
            (u32::from(u8::from(op)) << 24)
                | (u32::from(a) << 16)
                | u32::from(u16::from_ne_bytes(imm.to_ne_bytes())),
            line,
        );
    }

    pub fn emit_c(&mut self, op: OpCode, dest: u8, func: u8, nargs: u8, line: u32) {
        self.emit_a(op, dest, func, nargs, line);
    }

    pub fn emit_call(
        &mut self,
        dest: super::Register,
        func: super::Register,
        nargs: super::Arity,
        line: u32,
    ) {
        if let (Ok(dest), Ok(func), Ok(nargs)) = (
            u8::try_from(dest.get()),
            u8::try_from(func.get()),
            u8::try_from(nargs.get()),
        ) {
            self.emit_c(OpCode::Call, dest, func, nargs, line);
            return;
        }
        self.emit_a(OpCode::CallWide, 0, 0, 0, line);
        self.push_raw((u32::from(dest.get()) << 16) | u32::from(func.get()));
        self.push_raw(u32::from(nargs.get()) << 16);
        self.record_lines(2, line);
    }

    pub fn emit_counted_registers(
        &mut self,
        compact_op: OpCode,
        wide_op: OpCode,
        dest: super::Register,
        start: super::Register,
        count: u16,
        line: u32,
    ) {
        if let (Ok(dest), Ok(start), Ok(count)) = (
            u8::try_from(dest.get()),
            u8::try_from(start.get()),
            u8::try_from(count),
        ) {
            self.emit_a(compact_op, dest, start, count, line);
            return;
        }
        assert_eq!(wide_op.format(), super::InstructionFormat::Abc16);
        self.emit_a(wide_op, 0, 0, 0, line);
        self.push_raw((u32::from(dest.get()) << 16) | u32::from(start.get()));
        self.push_raw(u32::from(count) << 16);
        self.record_lines(2, line);
    }

    pub fn emit_closure_register_wide(
        &mut self,
        dest: super::Register,
        index: u32,
        upvalue_count: u16,
        line: u32,
    ) {
        self.emit_a(OpCode::MakeClosureRegisterWide, 0, 0, 0, line);
        self.push_raw((u32::from(dest.get()) << 16) | u32::from(upvalue_count));
        self.push_raw(index);
        self.record_lines(2, line);
    }

    pub fn emit_index32(&mut self, op: OpCode, register: u8, index: u32, line: u32) {
        self.emit_index32_with_aux(op, register, 0, index, line);
    }

    pub fn emit_index32_with_aux(
        &mut self,
        op: OpCode,
        register: u8,
        aux: u8,
        index: u32,
        line: u32,
    ) {
        assert_eq!(op.format(), super::InstructionFormat::AIndex32);
        self.emit_a(op, register, aux, 0, line);
        self.push_raw(index);
        self.record_lines(1, line);
    }

    pub fn emit_wide_abc(
        &mut self,
        op: OpCode,
        a: super::Register,
        b: super::Register,
        c: super::Register,
        line: u32,
    ) {
        assert_ne!(op, OpCode::Wide);
        self.emit_a(OpCode::Wide, u8::from(op), 0, 0, line);
        self.push_raw((u32::from(a.get()) << 16) | u32::from(b.get()));
        self.push_raw(u32::from(c.get()) << 16);
        self.record_lines(2, line);
    }

    pub fn emit_register_abc(
        &mut self,
        op: OpCode,
        a: super::Register,
        b: super::Register,
        c: super::Register,
        line: u32,
    ) {
        let Ok(a8) = u8::try_from(a.get()) else {
            self.emit_wide_or_record_error(op, a, b, c, line);
            return;
        };
        let Ok(b8) = u8::try_from(b.get()) else {
            self.emit_wide_or_record_error(op, a, b, c, line);
            return;
        };
        let Ok(c8) = u8::try_from(c.get()) else {
            self.emit_wide_or_record_error(op, a, b, c, line);
            return;
        };
        self.emit_a(op, a8, b8, c8, line);
    }

    pub fn emit_struct(
        &mut self,
        op: OpCode,
        schema_index: u16,
        a: u16,
        b: u16,
        c: u16,
        line: u32,
    ) {
        debug_assert!(matches!(
            op,
            OpCode::StructNew | OpCode::StructLoad | OpCode::StructStore
        ));
        self.emit_raw(
            (u32::from(u8::from(op)) << 24) | u32::from(schema_index),
            line,
        );
        self.push_raw((u32::from(a) << 16) | u32::from(b));
        self.push_raw(u32::from(c) << 16);
        self.record_lines(2, line);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn emit_enum(
        &mut self,
        op: OpCode,
        schema_index: u16,
        a: u16,
        b: u16,
        variant_or_field: u16,
        count: u16,
        line: u32,
    ) {
        debug_assert!(matches!(
            op,
            OpCode::EnumNew | OpCode::EnumTest | OpCode::EnumLoad
        ));
        self.emit_raw(
            (u32::from(u8::from(op)) << 24) | u32::from(schema_index),
            line,
        );
        self.push_raw((u32::from(a) << 16) | u32::from(b));
        self.push_raw((u32::from(variant_or_field) << 16) | u32::from(count));
        self.record_lines(2, line);
    }

    fn emit_wide_or_record_error(
        &mut self,
        op: OpCode,
        a: super::Register,
        b: super::Register,
        c: super::Register,
        line: u32,
    ) {
        if op.supports_wide_registers() {
            self.emit_wide_abc(op, a, b, c, line);
        } else {
            self.record_wide_operand_error(op);
        }
    }

    fn emit_raw(&mut self, instr: u32, line: u32) {
        self.bytecode_builder.push(instr);
        self.add_line(line);
    }

    pub fn strip_debug_info(&mut self) {
        self.name = None;
        self.lines.clear();
        // global layout names are load bearing, the vm re-resolves each slot by name whenever the
        for nested in &mut self.nested_functions {
            nested.strip_debug_info();
        }
    }
}
