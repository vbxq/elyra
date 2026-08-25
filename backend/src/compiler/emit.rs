use super::Compiler;
use aelys_bytecode::{Arity, Constant, OpCode, Register, Value};
use aelys_common::Result;
use aelys_common::error::{CompileError, CompileErrorKind};
use aelys_syntax::Span;

fn split_u32(value: u32) -> (u16, u16) {
    (
        u16::try_from(value >> 16).expect("upper half fits u16"),
        u16::try_from(value & u32::from(u16::MAX)).expect("lower half fits u16"),
    )
}

pub trait CompilerOperand {
    fn into_operand(self) -> u16;
}

impl CompilerOperand for u16 {
    fn into_operand(self) -> u16 {
        self
    }
}

impl CompilerOperand for u8 {
    fn into_operand(self) -> u16 {
        u16::from(self)
    }
}

impl CompilerOperand for i32 {
    fn into_operand(self) -> u16 {
        u16::try_from(self).expect("compiler operand must fit u16")
    }
}


impl Compiler {
    pub(super) fn update_jit_eligibility(&mut self) {
        self.current.jit_unsupported_struct = self.current.bytecode.as_slice().iter().any(|word| {
            matches!(
                OpCode::from_u8((word >> 24) as u8),
                Some(
                    OpCode::StructNew
                        | OpCode::StructLoad
                        | OpCode::StructStore
                        | OpCode::EnumNew
                        | OpCode::EnumTest
                        | OpCode::EnumLoad
                )
            )
        });
    }

    #[inline]
    pub fn current_line(&self, span: Span) -> u32 {
        span.line
    }

    pub fn emit_a(
        &mut self,
        op: OpCode,
        a: impl CompilerOperand,
        b: impl CompilerOperand,
        c: impl CompilerOperand,
        span: Span,
    ) {
        self.current.emit_register_abc(
            op,
            Register::new(a.into_operand()),
            Register::new(b.into_operand()),
            Register::new(c.into_operand()),
            self.current_line(span),
        );
    }

    pub fn emit_b(&mut self, op: OpCode, a: u16, imm: i16, span: Span) {
        let line = self.current_line(span);
        if let Ok(a) = u8::try_from(a) {
            self.current.emit_b(op, a, imm, line);
        } else {
            self.current.emit_register_abc(
                op,
                Register::new(a),
                Register::new(u16::from_ne_bytes(imm.to_ne_bytes())),
                Register::new(0),
                line,
            );
        }
    }

    pub fn emit_c(
        &mut self,
        op: OpCode,
        dest: u16,
        func: impl CompilerOperand,
        nargs: u16,
        span: Span,
    ) {
        let func = func.into_operand();
        if op == OpCode::Call
            || (op == OpCode::CallCached
                && (u8::try_from(dest).is_err()
                    || u8::try_from(func).is_err()
                    || u8::try_from(nargs).is_err()))
        {
            self.current.emit_call(
                Register::new(dest),
                Register::new(func),
                Arity::new(nargs),
                self.current_line(span),
            );
        } else {
            self.emit_a(op, dest, func, nargs, span);
        }
    }

    pub fn emit_jump(&mut self, op: OpCode, span: Span) -> usize {
        self.current.emit_jump(op, self.current_line(span))
    }

    pub fn emit_counted_registers(
        &mut self,
        compact_op: OpCode,
        wide_op: OpCode,
        dest: u16,
        start: u16,
        count: u16,
        span: Span,
    ) {
        self.current.emit_counted_registers(
            compact_op,
            wide_op,
            Register::new(dest),
            Register::new(start),
            count,
            self.current_line(span),
        );
    }

    pub fn emit_struct(
        &mut self,
        op: OpCode,
        schema_index: u16,
        a: u16,
        b: u16,
        c: u16,
        span: Span,
    ) {
        self.current
            .emit_struct(op, schema_index, a, b, c, self.current_line(span));
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
        span: Span,
    ) {
        self.current.emit_enum(
            op,
            schema_index,
            a,
            b,
            variant_or_field,
            count,
            self.current_line(span),
        );
    }

    pub fn emit_jump_if(&mut self, op: OpCode, reg: u16, span: Span) -> usize {
        self.current.emit_jump_if(op, reg, self.current_line(span))
    }

    pub fn patch_jump(&mut self, offset: usize) {
        self.current.patch_jump(offset);
    }

    pub fn patch_jump_to(&mut self, offset: usize, target: usize) {
        self.current.patch_jump_to(offset, target);
    }

    pub fn emit_jump_back(&mut self, target: usize, span: Span) {
        self.current.emit_jump_back(target, self.current_line(span));
    }

    pub fn emit_loop_back(
        &mut self,
        short_op: OpCode,
        long_op: OpCode,
        register: u16,
        target: usize,
        span: Span,
    ) {
        let Ok(narrow_register) = u8::try_from(register) else {
            self.current
                .emit_wide_loop_back(long_op, register, target, self.current_line(span));
            return;
        };
        self.current.emit_loop_back(
            short_op,
            long_op,
            narrow_register,
            target,
            self.current_line(span),
        );
    }

    pub fn emit_return0(&mut self, span: Span) {
        self.current
            .emit_a(OpCode::Return0, 0, 0, 0, self.current_line(span));
    }

    pub fn current_offset(&self) -> usize {
        self.current.current_offset()
    }

    pub fn checked_call_arity(&self, count: usize, span: Span) -> Result<u16> {
        u16::try_from(count).map_err(|_| {
            CompileError::new(
                CompileErrorKind::TooManyArguments,
                span,
                self.source.clone(),
            )
            .into()
        })
    }

    pub fn add_constant(&mut self, value: Value, _span: Span) -> Result<u32> {
        let idx = self.current.add_constant(value);
        Ok(idx)
    }

    pub fn add_string_constant(&mut self, value: &str) -> u32 {
        self.current
            .add_structural_constant(Constant::String(value.to_string()))
    }

    pub fn emit_load_constant(&mut self, dest: u16, index: u32, span: Span) {
        let line = self.current_line(span);
        if let (Ok(dest), Ok(index)) = (u8::try_from(dest), u16::try_from(index)) {
            self.current.emit_b(
                OpCode::LoadK,
                dest,
                i16::from_ne_bytes(index.to_ne_bytes()),
                line,
            );
        } else if let Ok(dest) = u8::try_from(dest) {
            self.current
                .emit_index32(OpCode::LoadKWide, dest, index, line);
        } else {
            let (high, low) = split_u32(index);
            self.current.emit_register_abc(
                OpCode::LoadK,
                Register::new(dest),
                Register::new(high),
                Register::new(low),
                line,
            );
        }
    }

    pub fn emit_make_closure(&mut self, dest: u16, index: u32, upvalues: u8, span: Span) {
        let line = self.current_line(span);
        let Ok(narrow_dest) = u8::try_from(dest) else {
            self.current.emit_closure_register_wide(
                Register::new(dest),
                index,
                u16::from(upvalues),
                line,
            );
            return;
        };
        if let Ok(index) = u8::try_from(index) {
            self.current
                .emit_a(OpCode::MakeClosure, narrow_dest, index, upvalues, line);
        } else {
            self.current.emit_index32_with_aux(
                OpCode::MakeClosureWide,
                narrow_dest,
                upvalues,
                index,
                line,
            );
        }
    }

    pub fn emit_get_global_index(&mut self, dest: u16, index: u32, span: Span) {
        let line = self.current_line(span);
        if let (Ok(dest), Ok(index)) = (u8::try_from(dest), u16::try_from(index)) {
            self.current.emit_b(
                OpCode::GetGlobalIdx,
                dest,
                i16::from_ne_bytes(index.to_ne_bytes()),
                line,
            );
        } else if let Ok(dest) = u8::try_from(dest) {
            self.current
                .emit_index32(OpCode::GetGlobalIdxWide, dest, index, line);
        } else {
            let (high, low) = split_u32(index);
            self.current.emit_register_abc(
                OpCode::GetGlobalIdx,
                Register::new(dest),
                Register::new(high),
                Register::new(low),
                line,
            );
        }
    }

    pub fn emit_set_global_index(&mut self, source: u16, index: u32, span: Span) {
        let line = self.current_line(span);
        if let (Ok(source), Ok(index)) = (u8::try_from(source), u16::try_from(index)) {
            self.current.emit_b(
                OpCode::SetGlobalIdx,
                source,
                i16::from_ne_bytes(index.to_ne_bytes()),
                line,
            );
        } else if let Ok(source) = u8::try_from(source) {
            self.current
                .emit_index32(OpCode::SetGlobalIdxWide, source, index, line);
        } else {
            let (high, low) = split_u32(index);
            self.current.emit_register_abc(
                OpCode::SetGlobalIdx,
                Register::new(source),
                Register::new(high),
                Register::new(low),
                line,
            );
        }
    }

    pub fn emit_call_global_cached(
        &mut self,
        dest: u16,
        global_idx: u32,
        nargs: u16,
        _global_name: &str,
        span: Span,
    ) {
        let line = self.current_line(span);

        if global_idx > u32::from(u8::MAX)
            || u8::try_from(dest).is_err()
            || u8::try_from(nargs).is_err()
        {
            self.emit_get_global_index(dest, global_idx, span);
            self.emit_c(OpCode::Call, dest, dest, nargs, span);
            return;
        }

        let global_idx = u8::try_from(global_idx).expect("global index was range checked");

        let _ = line;
        self.emit_a(OpCode::CallGlobal, dest, u16::from(global_idx), nargs, span);
    }
}
