use super::super::Compiler;
use aelys_bytecode::{OpCode, SumTag};
use aelys_common::Result;
use aelys_sema::{InferType, TypedExpr};
use aelys_syntax::Span;

impl Compiler {
    pub(super) fn compile_typed_sum_method_call(
        &mut self,
        object: &TypedExpr,
        member: &str,
        args: &[TypedExpr],
        dest: u16,
        span: Span,
    ) -> Result<bool> {
        let (family, value_type, error_type) = match &object.ty {
            InferType::Option(value) => ("Option", value.as_ref(), None),
            InferType::Result(value, error) => ("Result", value.as_ref(), Some(error.as_ref())),
            _ => return Ok(false),
        };

        let recognized = matches!(
            member,
            "unwrap"
                | "expect"
                | "unwrap_or"
                | "unwrap_or_else"
                | "ok"
                | "err"
                | "map"
                | "map_err"
                | "and_then"
                | "or_else"
        );
        if !recognized {
            return Ok(false);
        }

        let source = self.alloc_register()?;
        self.compile_typed_expr(object, source)?;

        match member {
            "unwrap" | "expect" => {
                let message = if member == "expect" {
                    let message = self.alloc_register()?;
                    self.compile_typed_expr(&args[0], message)?;
                    Some(message)
                } else {
                    None
                };
                self.compile_sum_unwrap(source, family, dest, message, span)?;
                if let Some(message) = message {
                    self.free_register(message);
                }
            }
            "unwrap_or" => {
                let fallback = self.alloc_register()?;
                self.compile_typed_expr(&args[0], fallback)?;
                self.compile_sum_unwrap_or(source, family, fallback, dest, span)?;
                self.free_register(fallback);
            }
            "unwrap_or_else" => {
                if family == "Option" {
                    self.compile_sum_unwrap_or_else(source, family, &args[0], dest, span)?;
                } else {
                    self.compile_sum_unwrap_or_else_with_error(source, &args[0], dest, span)?;
                }
            }
            "ok" => self.compile_sum_ok(source, dest, span)?,
            "err" => self.compile_sum_err(source, dest, span)?,
            "map" => self.compile_sum_map(source, family, &args[0], dest, span)?,
            "map_err" => self.compile_sum_map_err(source, &args[0], dest, span)?,
            "and_then" => self.compile_sum_and_then(source, family, &args[0], dest, span)?,
            "or_else" => self.compile_sum_or_else(source, family, &args[0], dest, span)?,
            _ => unreachable!(),
        }

        let _ = (value_type, error_type);
        self.free_register(source);
        Ok(true)
    }

    fn compile_sum_unwrap(
        &mut self,
        source: u16,
        family: &str,
        dest: u16,
        message: Option<u16>,
        span: Span,
    ) -> Result<()> {
        let tag = if family == "Option" {
            SumTag::OptionSome as u8
        } else {
            SumTag::ResultOk as u8
        };
        let condition = self.alloc_register()?;
        self.emit_a(OpCode::SumTest, condition, source, tag, span);
        let failure = self.emit_jump_if(OpCode::JumpIfNot, condition, span);
        self.free_register(condition);
        self.emit_a(OpCode::SumPayload, dest, source, 0, span);
        let end = self.emit_jump(OpCode::Jump, span);
        self.patch_jump(failure);
        let family_code = if family == "Option" { 1 } else { 2 };
        self.emit_a(
            OpCode::MatchFail,
            family_code,
            message.unwrap_or(0),
            u8::from(message.is_some()),
            span,
        );
        self.patch_jump(end);
        Ok(())
    }

    fn compile_sum_unwrap_or(
        &mut self,
        source: u16,
        family: &str,
        fallback: u16,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let tag = if family == "Option" {
            SumTag::OptionSome as u8
        } else {
            SumTag::ResultOk as u8
        };
        let condition = self.alloc_register()?;
        self.emit_a(OpCode::SumTest, condition, source, tag, span);
        let failure = self.emit_jump_if(OpCode::JumpIfNot, condition, span);
        self.free_register(condition);
        self.emit_a(OpCode::SumPayload, dest, source, 0, span);
        let end = self.emit_jump(OpCode::Jump, span);
        self.patch_jump(failure);
        self.emit_a(OpCode::Move, dest, fallback, 0, span);
        self.patch_jump(end);
        Ok(())
    }

    fn compile_sum_unwrap_or_else(
        &mut self,
        source: u16,
        family: &str,
        closure: &TypedExpr,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let tag = if family == "Option" {
            SumTag::OptionSome as u8
        } else {
            SumTag::ResultOk as u8
        };
        let condition = self.alloc_register()?;
        self.emit_a(OpCode::SumTest, condition, source, tag, span);
        let failure = self.emit_jump_if(OpCode::JumpIfNot, condition, span);
        self.free_register(condition);
        self.emit_a(OpCode::SumPayload, dest, source, 0, span);
        let end = self.emit_jump(OpCode::Jump, span);
        self.patch_jump(failure);
        self.compile_typed_callable(closure, &[], dest, span)?;
        self.patch_jump(end);
        Ok(())
    }

    fn compile_sum_unwrap_or_else_with_error(
        &mut self,
        source: u16,
        closure: &TypedExpr,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let condition = self.alloc_register()?;
        self.emit_a(
            OpCode::SumTest,
            condition,
            source,
            SumTag::ResultOk as u8,
            span,
        );
        let failure = self.emit_jump_if(OpCode::JumpIfNot, condition, span);
        self.free_register(condition);
        self.emit_a(OpCode::SumPayload, dest, source, 0, span);
        let end = self.emit_jump(OpCode::Jump, span);
        self.patch_jump(failure);
        let error = self.alloc_register()?;
        self.emit_a(OpCode::SumPayload, error, source, 0, span);
        self.compile_typed_callable(closure, &[error], dest, span)?;
        self.free_register(error);
        self.patch_jump(end);
        Ok(())
    }

    fn compile_sum_ok(&mut self, source: u16, dest: u16, span: Span) -> Result<()> {
        let condition = self.alloc_register()?;
        self.emit_a(
            OpCode::SumTest,
            condition,
            source,
            SumTag::ResultOk as u8,
            span,
        );
        let failure = self.emit_jump_if(OpCode::JumpIfNot, condition, span);
        self.free_register(condition);
        let payload = self.alloc_register()?;
        self.emit_a(OpCode::SumPayload, payload, source, 0, span);
        self.emit_a(
            OpCode::MakeSum,
            dest,
            payload,
            SumTag::OptionSome as u8,
            span,
        );
        self.free_register(payload);
        let end = self.emit_jump(OpCode::Jump, span);
        self.patch_jump(failure);
        self.emit_a(OpCode::LoadNone, dest, 0, 0, span);
        self.patch_jump(end);
        Ok(())
    }

    fn compile_sum_err(&mut self, source: u16, dest: u16, span: Span) -> Result<()> {
        let condition = self.alloc_register()?;
        self.emit_a(
            OpCode::SumTest,
            condition,
            source,
            SumTag::ResultOk as u8,
            span,
        );
        let failure = self.emit_jump_if(OpCode::JumpIfNot, condition, span);
        self.free_register(condition);
        self.emit_a(OpCode::LoadNone, dest, 0, 0, span);
        let end = self.emit_jump(OpCode::Jump, span);
        self.patch_jump(failure);
        let payload = self.alloc_register()?;
        self.emit_a(OpCode::SumPayload, payload, source, 0, span);
        self.emit_a(
            OpCode::MakeSum,
            dest,
            payload,
            SumTag::OptionSome as u8,
            span,
        );
        self.free_register(payload);
        self.patch_jump(end);
        Ok(())
    }

    fn compile_sum_map(
        &mut self,
        source: u16,
        family: &str,
        closure: &TypedExpr,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let success_tag = if family == "Option" {
            SumTag::OptionSome as u8
        } else {
            SumTag::ResultOk as u8
        };
        let condition = self.alloc_register()?;
        self.emit_a(OpCode::SumTest, condition, source, success_tag, span);
        let failure = self.emit_jump_if(OpCode::JumpIfNot, condition, span);
        self.free_register(condition);
        let payload = self.alloc_register()?;
        let mapped = self.alloc_register()?;
        self.emit_a(OpCode::SumPayload, payload, source, 0, span);
        self.compile_typed_callable(closure, &[payload], mapped, span)?;
        self.emit_a(OpCode::MakeSum, dest, mapped, success_tag, span);
        self.free_register(mapped);
        self.free_register(payload);
        let end = self.emit_jump(OpCode::Jump, span);
        self.patch_jump(failure);
        if family == "Option" {
            self.emit_a(OpCode::LoadNone, dest, 0, 0, span);
        } else {
            self.emit_a(OpCode::Move, dest, source, 0, span);
        }
        self.patch_jump(end);
        Ok(())
    }

    fn compile_sum_map_err(
        &mut self,
        source: u16,
        closure: &TypedExpr,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let condition = self.alloc_register()?;
        self.emit_a(
            OpCode::SumTest,
            condition,
            source,
            SumTag::ResultOk as u8,
            span,
        );
        let failure = self.emit_jump_if(OpCode::JumpIfNot, condition, span);
        self.free_register(condition);
        self.emit_a(OpCode::Move, dest, source, 0, span);
        let end = self.emit_jump(OpCode::Jump, span);
        self.patch_jump(failure);
        let error = self.alloc_register()?;
        let mapped = self.alloc_register()?;
        self.emit_a(OpCode::SumPayload, error, source, 0, span);
        self.compile_typed_callable(closure, &[error], mapped, span)?;
        self.emit_a(OpCode::MakeSum, dest, mapped, SumTag::ResultErr as u8, span);
        self.free_register(mapped);
        self.free_register(error);
        self.patch_jump(end);
        Ok(())
    }

    fn compile_sum_and_then(
        &mut self,
        source: u16,
        family: &str,
        closure: &TypedExpr,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let success_tag = if family == "Option" {
            SumTag::OptionSome as u8
        } else {
            SumTag::ResultOk as u8
        };
        let condition = self.alloc_register()?;
        self.emit_a(OpCode::SumTest, condition, source, success_tag, span);
        let failure = self.emit_jump_if(OpCode::JumpIfNot, condition, span);
        self.free_register(condition);
        let payload = self.alloc_register()?;
        self.emit_a(OpCode::SumPayload, payload, source, 0, span);
        self.compile_typed_callable(closure, &[payload], dest, span)?;
        self.free_register(payload);
        let end = self.emit_jump(OpCode::Jump, span);
        self.patch_jump(failure);
        if family == "Option" {
            self.emit_a(OpCode::LoadNone, dest, 0, 0, span);
        } else {
            self.emit_a(OpCode::Move, dest, source, 0, span);
        }
        self.patch_jump(end);
        Ok(())
    }

    fn compile_sum_or_else(
        &mut self,
        source: u16,
        family: &str,
        closure: &TypedExpr,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let success_tag = if family == "Option" {
            SumTag::OptionSome as u8
        } else {
            SumTag::ResultOk as u8
        };
        let condition = self.alloc_register()?;
        self.emit_a(OpCode::SumTest, condition, source, success_tag, span);
        let failure = self.emit_jump_if(OpCode::JumpIfNot, condition, span);
        self.free_register(condition);
        self.emit_a(OpCode::Move, dest, source, 0, span);
        let end = self.emit_jump(OpCode::Jump, span);
        self.patch_jump(failure);
        if family == "Option" {
            self.compile_typed_callable(closure, &[], dest, span)?;
        } else {
            let error = self.alloc_register()?;
            self.emit_a(OpCode::SumPayload, error, source, 0, span);
            self.compile_typed_callable(closure, &[error], dest, span)?;
            self.free_register(error);
        }
        self.patch_jump(end);
        Ok(())
    }

    fn compile_typed_callable(
        &mut self,
        callable: &TypedExpr,
        args: &[u16],
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let count = args.len() + 2;
        let base = self.alloc_consecutive_registers_for_call(count, span)?;
        for offset in 0..count {
            let register = base + u16::try_from(offset).expect("call register offset fits");
            self.register_pool[register as usize] = true;
            self.next_register = self.next_register.max(u32::from(register) + 1);
        }
        self.compile_typed_expr(callable, base)?;
        for (index, source) in args.iter().enumerate() {
            let register = base + 1 + u16::try_from(index).expect("call register offset fits");
            self.emit_a(OpCode::Move, register, *source, 0, span);
        }
        self.emit_c(
            OpCode::Call,
            dest,
            base,
            u16::try_from(args.len()).expect("sum method arity fits"),
            span,
        );
        for offset in (0..count).rev() {
            self.free_register(base + u16::try_from(offset).expect("call register offset fits"));
        }
        Ok(())
    }
}
