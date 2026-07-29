use crate::compiler::Compiler;
use aelys_bytecode::OpCode;
use aelys_common::Result;
use aelys_syntax::Span;
use aelys_syntax::ast::{Expr, TypeAnnotation};

impl Compiler {
    pub fn compile_array_sized(
        &mut self,
        _element_type: &Option<TypeAnnotation>,
        size: &Expr,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        // For now, compile as ArrayNewP with size from register
        // TODO: Use ArrayFill opcode when added
        let size_reg = self.alloc_register()?;
        self.compile_expr(size, size_reg)?;
        self.emit_a(OpCode::ArrayNewP, dest, size_reg, 0, span);
        self.free_register(size_reg);
        Ok(())
    }

    pub fn compile_array_literal(
        &mut self,
        elements: &[Expr],
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let count = elements.len();

        if count == 0 {
            self.emit_a(OpCode::ArrayNewP, dest, 0, 0, span);
            return Ok(());
        }

        let count_operand = self.checked_call_arity(count, span)?;
        let start_reg = self.alloc_consecutive_registers_for_call(count, span)?;

        for i in 0..count {
            let reg = start_reg + u16::try_from(i).expect("register offset was range checked");
            self.register_pool[reg as usize] = true;
            if u32::from(reg) >= self.next_register {
                self.next_register = u32::from(reg) + 1;
            }
        }

        for (i, elem) in elements.iter().enumerate() {
            let elem_reg = start_reg + u16::try_from(i).expect("register offset was range checked");
            self.compile_expr(elem, elem_reg)?;
        }

        self.emit_counted_registers(
            OpCode::ArrayLit,
            OpCode::ArrayLitWide,
            dest,
            start_reg,
            count_operand,
            span,
        );

        for i in (0..count).rev() {
            let reg = start_reg + u16::try_from(i).expect("register offset was range checked");
            self.free_register(reg);
        }

        Ok(())
    }

    pub fn compile_vec_literal(&mut self, elements: &[Expr], dest: u16, span: Span) -> Result<()> {
        let count = elements.len();

        if count == 0 {
            self.emit_a(OpCode::VecNewP, dest, 0, 0, span);
            return Ok(());
        }

        let count_operand = self.checked_call_arity(count, span)?;
        let start_reg = self.alloc_consecutive_registers_for_call(count, span)?;

        for i in 0..count {
            let reg = start_reg + u16::try_from(i).expect("register offset was range checked");
            self.register_pool[reg as usize] = true;
            if u32::from(reg) >= self.next_register {
                self.next_register = u32::from(reg) + 1;
            }
        }

        for (i, elem) in elements.iter().enumerate() {
            let elem_reg = start_reg + u16::try_from(i).expect("register offset was range checked");
            self.compile_expr(elem, elem_reg)?;
        }

        self.emit_counted_registers(
            OpCode::VecLit,
            OpCode::VecLitWide,
            dest,
            start_reg,
            count_operand,
            span,
        );

        for i in (0..count).rev() {
            let reg = start_reg + u16::try_from(i).expect("register offset was range checked");
            self.free_register(reg);
        }

        Ok(())
    }

    pub fn compile_index_access(
        &mut self,
        object: &Expr,
        index: &Expr,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let obj_reg = self.alloc_register()?;
        self.compile_expr(object, obj_reg)?;

        let idx_reg = self.alloc_register()?;
        self.compile_expr(index, idx_reg)?;

        self.emit_a(OpCode::ArrayLoadP, dest, obj_reg, idx_reg, span);

        self.free_register(idx_reg);
        self.free_register(obj_reg);

        Ok(())
    }

    pub fn compile_index_assign(
        &mut self,
        object: &Expr,
        index: &Expr,
        value: &Expr,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let obj_reg = self.alloc_register()?;
        self.compile_expr(object, obj_reg)?;

        let idx_reg = self.alloc_register()?;
        self.compile_expr(index, idx_reg)?;

        let val_reg = self.alloc_register()?;
        self.compile_expr(value, val_reg)?;

        self.emit_a(OpCode::ArrayStoreP, obj_reg, idx_reg, val_reg, span);

        if dest != val_reg {
            self.emit_a(OpCode::Move, dest, val_reg, 0, span);
        }

        self.free_register(val_reg);
        self.free_register(idx_reg);
        self.free_register(obj_reg);

        Ok(())
    }

    pub fn compile_slice(
        &mut self,
        _object: &Expr,
        _range: &Expr,
        _dest: u16,
        _span: Span,
    ) -> Result<()> {
        todo!("slice")
    }

    pub fn compile_range(
        &mut self,
        _start: &Option<Box<Expr>>,
        _end: &Option<Box<Expr>>,
        _inclusive: bool,
        _dest: u16,
        _span: Span,
    ) -> Result<()> {
        todo!("range")
    }
}
