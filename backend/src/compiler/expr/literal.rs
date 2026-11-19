use super::Compiler;
use aelys_bytecode::{Constant, OpCode, Value};
use aelys_common::Result;
use aelys_common::error::{CompileError, CompileErrorKind};
use aelys_syntax::Span;

// fits in 16-bit immediate?
pub(super) fn small_int_immediate(n: i64) -> Option<i16> {
    i16::try_from(n).ok()
}

impl Compiler {
    pub fn compile_literal_int(&mut self, n: i64, dest: u16, span: Span) -> Result<()> {
        match small_int_immediate(n) {
            Some(imm) => {
                self.emit_b(OpCode::LoadI, dest, imm, span);
            }
            None => self.compile_wide_literal(i128::from(n), dest, span)?,
        }
        Ok(())
    }

    // a u64 literal carries its bit pattern, so its value is read through the type
    pub fn compile_typed_literal_int(
        &mut self,
        n: i64,
        unsigned: bool,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        if unsigned && n < 0 {
            return self.compile_wide_literal(i128::from(n as u64), dest, span);
        }
        self.compile_literal_int(n, dest, span)
    }

    pub fn compile_pattern_int(&mut self, n: i128, dest: u16, span: Span) -> Result<()> {
        match i64::try_from(n) {
            Ok(n) => self.compile_literal_int(n, dest, span),
            Err(_) => self.compile_wide_literal(n, dest, span),
        }
    }

    fn compile_wide_literal(&mut self, value: i128, dest: u16, span: Span) -> Result<()> {
        let constant = Constant::integer(value).ok_or_else(|| {
            CompileError::new(
                CompileErrorKind::IntegerOverflow {
                    value: value.to_string(),
                    min: i64::MIN,
                    max: i64::MAX,
                },
                span,
                self.source.clone(),
            )
        })?;
        let k = self.current.add_structural_constant(constant);
        self.emit_load_constant(dest, k, span);
        Ok(())
    }

    pub fn compile_literal_float(&mut self, f: f64, dest: u16, span: Span) -> Result<()> {
        let k = self.add_constant(Value::float(f), span)?;
        self.emit_load_constant(dest, k, span);
        Ok(())
    }

    pub fn compile_literal_char(&mut self, c: char, dest: u16, span: Span) -> Result<()> {
        let k = self.add_constant(Value::char(c), span)?;
        self.emit_load_constant(dest, k, span);
        Ok(())
    }

    pub fn compile_literal_string(&mut self, s: &str, dest: u16, span: Span) -> Result<()> {
        let k = self.add_string_constant(s);
        self.emit_load_constant(dest, k, span);
        Ok(())
    }

    pub fn compile_literal_bool(&mut self, b: bool, dest: u16, span: Span) -> Result<()> {
        self.emit_a(OpCode::LoadBool, dest, if b { 1 } else { 0 }, 0, span);
        Ok(())
    }

    pub fn compile_literal_null(&mut self, dest: u16, span: Span) -> Result<()> {
        self.emit_a(OpCode::LoadNull, dest, 0, 0, span);
        Ok(())
    }

    pub fn compile_literal_unit(&mut self, dest: u16, span: Span) -> Result<()> {
        self.emit_a(OpCode::LoadUnit, dest, 0, 0, span);
        Ok(())
    }

    pub fn compile_literal_none(&mut self, dest: u16, span: Span) -> Result<()> {
        self.emit_a(OpCode::LoadNone, dest, 0, 0, span);
        Ok(())
    }
}
