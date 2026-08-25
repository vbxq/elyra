use super::super::Compiler;
use aelys_bytecode::OpCode;
use aelys_common::Result;
use aelys_common::error::{CompileError, CompileErrorKind};
use aelys_sema::{InferType, TypedExpr};
use aelys_syntax::Span;

impl Compiler {
    pub(super) fn compile_typed_collection_pipeline(
        &mut self,
        object: &TypedExpr,
        member: &str,
        args: &[TypedExpr],
        callee_type: &InferType,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        match member {
            "iter" if args.is_empty() => {
                return self.compile_typed_expr(object, dest);
            }
            "collect" if args.is_empty() => {
                let InferType::Vec(output_inner) = collection_call_result(callee_type) else {
                    return self.pipeline_error("collect must produce a vector", span);
                };
                return self.compile_typed_collect(object, output_inner, dest, span);
            }
            "map" if args.len() == 1 => {
                let InferType::Vec(output_inner) = collection_call_result(callee_type) else {
                    return self.pipeline_error("map must produce a vector", span);
                };
                return self.compile_typed_map(object, &args[0], output_inner, dest, span);
            }
            "filter" if args.len() == 1 => {
                let InferType::Vec(output_inner) = collection_call_result(callee_type) else {
                    return self.pipeline_error("filter must produce a vector", span);
                };
                return self.compile_typed_filter(object, &args[0], output_inner, dest, span);
            }
            "fold" if args.len() == 2 => {
                return self.compile_typed_fold(object, &args[0], &args[1], dest, span);
            }
            _ => {}
        }

        self.pipeline_error("invalid collection pipeline call", span)
    }

    fn compile_typed_collect(
        &mut self,
        object: &TypedExpr,
        output_inner: &InferType,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let source_reg = self.alloc_register()?;
        self.compile_typed_expr(object, source_reg)?;
        self.emit_a(select_vec_new(output_inner), dest, 0, 0, span);

        let loop_base = self.alloc_consecutive_registers_for_call(3, span)?;
        self.alloc_consecutive_from(loop_base, 3)?;
        let index_reg = loop_base + 1;
        let source_loop_reg = loop_base + 2;
        self.emit_a(OpCode::Move, source_loop_reg, source_reg, 0, span);
        self.emit_b(OpCode::LoadI, index_reg, 0, span);
        let jump_to_loop = self.emit_jump(OpCode::Jump, span);
        let body_start = self.current_offset();
        self.emit_a(select_vec_push(output_inner), dest, loop_base, 0, span);
        self.patch_jump(jump_to_loop);
        self.emit_collection_loop(object, loop_base, body_start, span);

        self.free_register(source_loop_reg);
        self.free_register(index_reg);
        self.free_register(loop_base);
        self.free_register(source_reg);
        Ok(())
    }

    fn compile_typed_map(
        &mut self,
        object: &TypedExpr,
        callback: &TypedExpr,
        output_inner: &InferType,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let source_reg = self.alloc_register()?;
        self.compile_typed_expr(object, source_reg)?;
        self.emit_a(select_vec_new(output_inner), dest, 0, 0, span);

        let loop_base = self.alloc_consecutive_registers_for_call(3, span)?;
        self.alloc_consecutive_from(loop_base, 3)?;
        let index_reg = loop_base + 1;
        let source_loop_reg = loop_base + 2;
        let callback_base = self.alloc_consecutive_registers_for_call(3, span)?;
        self.alloc_consecutive_from(callback_base, 3)?;
        let callback_result = callback_base + 1;
        let callback_arg = callback_base + 2;
        self.compile_typed_expr(callback, callback_base)?;
        self.emit_a(OpCode::Move, source_loop_reg, source_reg, 0, span);
        self.emit_b(OpCode::LoadI, index_reg, 0, span);
        let jump_to_loop = self.emit_jump(OpCode::Jump, span);
        let body_start = self.current_offset();
        self.emit_a(OpCode::Move, callback_arg, loop_base, 0, span);
        self.emit_c(OpCode::CallCached, callback_result, callback_base, 1, span);
        self.emit_a(
            select_vec_push(output_inner),
            dest,
            callback_result,
            0,
            span,
        );
        self.patch_jump(jump_to_loop);
        self.emit_collection_loop(object, loop_base, body_start, span);

        self.free_register(source_loop_reg);
        self.free_register(index_reg);
        self.free_register(loop_base);
        self.free_register(callback_result);
        self.free_register(callback_arg);
        self.free_register(callback_base);
        self.free_register(source_reg);
        Ok(())
    }

    fn compile_typed_filter(
        &mut self,
        object: &TypedExpr,
        callback: &TypedExpr,
        output_inner: &InferType,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let source_reg = self.alloc_register()?;
        self.compile_typed_expr(object, source_reg)?;
        self.emit_a(select_vec_new(output_inner), dest, 0, 0, span);

        let loop_base = self.alloc_consecutive_registers_for_call(3, span)?;
        self.alloc_consecutive_from(loop_base, 3)?;
        let index_reg = loop_base + 1;
        let source_loop_reg = loop_base + 2;
        let callback_base = self.alloc_consecutive_registers_for_call(3, span)?;
        self.alloc_consecutive_from(callback_base, 3)?;
        let predicate = callback_base + 1;
        let callback_arg = callback_base + 2;
        self.compile_typed_expr(callback, callback_base)?;
        self.emit_a(OpCode::Move, source_loop_reg, source_reg, 0, span);
        self.emit_b(OpCode::LoadI, index_reg, 0, span);
        let jump_to_loop = self.emit_jump(OpCode::Jump, span);
        let body_start = self.current_offset();
        self.emit_a(OpCode::Move, callback_arg, loop_base, 0, span);
        self.emit_c(OpCode::CallCached, predicate, callback_base, 1, span);
        let skip = self.emit_jump_if(OpCode::JumpIfNot, predicate, span);
        self.emit_a(select_vec_push(output_inner), dest, loop_base, 0, span);
        self.patch_jump(skip);
        self.patch_jump(jump_to_loop);
        self.emit_collection_loop(object, loop_base, body_start, span);

        self.free_register(source_loop_reg);
        self.free_register(index_reg);
        self.free_register(loop_base);
        self.free_register(predicate);
        self.free_register(callback_arg);
        self.free_register(callback_base);
        self.free_register(source_reg);
        Ok(())
    }

    fn compile_typed_fold(
        &mut self,
        object: &TypedExpr,
        initial: &TypedExpr,
        callback: &TypedExpr,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let source_reg = self.alloc_register()?;
        self.compile_typed_expr(object, source_reg)?;
        let loop_base = self.alloc_consecutive_registers_for_call(3, span)?;
        self.alloc_consecutive_from(loop_base, 3)?;
        let index_reg = loop_base + 1;
        let source_loop_reg = loop_base + 2;
        let callback_base = self.alloc_consecutive_registers_for_call(4, span)?;
        self.alloc_consecutive_from(callback_base, 4)?;
        let accumulator = callback_base + 1;
        let accumulator_arg = callback_base + 2;
        let element_arg = callback_base + 3;
        self.compile_typed_expr(callback, callback_base)?;
        self.compile_typed_expr(initial, accumulator)?;
        self.emit_a(OpCode::Move, source_loop_reg, source_reg, 0, span);
        self.emit_b(OpCode::LoadI, index_reg, 0, span);
        let jump_to_loop = self.emit_jump(OpCode::Jump, span);
        let body_start = self.current_offset();
        self.emit_a(OpCode::Move, accumulator_arg, accumulator, 0, span);
        self.emit_a(OpCode::Move, element_arg, loop_base, 0, span);
        self.emit_c(OpCode::CallCached, accumulator, callback_base, 2, span);
        self.patch_jump(jump_to_loop);
        self.emit_collection_loop(object, loop_base, body_start, span);

        self.free_register(source_loop_reg);
        self.free_register(index_reg);
        self.free_register(loop_base);
        self.free_register(element_arg);
        self.free_register(accumulator_arg);
        self.free_register(callback_base);
        self.emit_a(OpCode::Move, dest, accumulator, 0, span);
        self.free_register(accumulator);
        self.free_register(source_reg);
        Ok(())
    }

    fn emit_collection_loop(
        &mut self,
        object: &TypedExpr,
        loop_base: u16,
        body_start: usize,
        span: Span,
    ) {
        let (short, long) = match &object.ty {
            InferType::Array(_) | InferType::FixedArray(_, _) => {
                (OpCode::ArrayForLoop, OpCode::ArrayForLoopLong)
            }
            _ => (OpCode::VecForLoop, OpCode::VecForLoopLong),
        };
        self.emit_loop_back(short, long, loop_base, body_start, span);
    }

    fn pipeline_error(&self, message: &str, span: Span) -> aelys_common::Result<()> {
        Err(CompileError::new(
            CompileErrorKind::TypeInferenceError(message.to_string()),
            span,
            self.source.clone(),
        )
        .into())
    }
}

fn collection_call_result(callee_type: &InferType) -> &InferType {
    match callee_type {
        InferType::Function { ret, .. } => ret,
        _ => callee_type,
    }
}

fn select_vec_new(inner: &InferType) -> OpCode {
    if inner.is_integer() {
        OpCode::VecNewI
    } else if inner.is_float() {
        OpCode::VecNewF
    } else if matches!(inner, InferType::Bool) {
        OpCode::VecNewB
    } else {
        OpCode::VecNewP
    }
}

fn select_vec_push(inner: &InferType) -> OpCode {
    if inner.is_integer() {
        OpCode::VecPushI
    } else if inner.is_float() {
        OpCode::VecPushF
    } else if matches!(inner, InferType::Bool) {
        OpCode::VecPushB
    } else {
        OpCode::VecPushP
    }
}
