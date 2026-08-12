use super::super::Compiler;
use aelys_bytecode::{OpCode, SumTag};
use aelys_common::Result;
use aelys_sema::{InferType, TypedMatchArm, TypedMatchArmBody, TypedPattern, TypedPatternKind};
use aelys_syntax::Span;
use std::collections::HashMap;

impl Compiler {
    pub(super) fn compile_typed_try(
        &mut self,
        inner: &aelys_sema::TypedExpr,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let source_reg = self.alloc_register()?;
        self.compile_typed_expr(inner, source_reg)?;

        let (success_tag, is_option) = match &inner.ty {
            InferType::Option(_) => (SumTag::OptionSome as u8, true),
            InferType::Result(_, _) => (SumTag::ResultOk as u8, false),
            _ => {
                self.free_register(source_reg);
                return Err(aelys_common::error::CompileError::new(
                    aelys_common::error::CompileErrorKind::TypeInferenceError(
                        "the ? operand must be Option or Result".to_string(),
                    ),
                    span,
                    self.source.clone(),
                )
                .into());
            }
        };

        let test_reg = self.alloc_register()?;
        self.emit_a(OpCode::SumTest, test_reg, source_reg, success_tag, span);
        let failure_jump = self.emit_jump_if(OpCode::JumpIfNot, test_reg, span);
        self.free_register(test_reg);

        self.emit_a(OpCode::SumPayload, dest, source_reg, 0, span);
        let end_jump = self.emit_jump(OpCode::Jump, span);
        self.patch_jump(failure_jump);

        if is_option {
            self.emit_return_from_try(source_reg, span);
        } else {
            let target_error = self.current_return_type.as_ref().and_then(|ty| match ty {
                InferType::Result(_, error) => Some(error.as_ref()),
                _ => None,
            });
            let source_error = match &inner.ty {
                InferType::Result(_, error) => error.as_ref(),
                _ => unreachable!(),
            };
            if matches!(source_error, InferType::String)
                && matches!(target_error, Some(InferType::Error))
            {
                let payload_reg = self.alloc_register()?;
                let error_reg = self.alloc_register()?;
                let result_reg = self.alloc_register()?;
                self.emit_a(OpCode::SumPayload, payload_reg, source_reg, 0, span);
                self.emit_a(
                    OpCode::MakeSum,
                    error_reg,
                    payload_reg,
                    SumTag::ErrorMessage as u8,
                    span,
                );
                self.emit_a(
                    OpCode::MakeSum,
                    result_reg,
                    error_reg,
                    SumTag::ResultErr as u8,
                    span,
                );
                self.emit_return_from_try(result_reg, span);
                self.free_register(result_reg);
                self.free_register(error_reg);
                self.free_register(payload_reg);
            } else {
                self.emit_return_from_try(source_reg, span);
            }
        }

        self.patch_jump(end_jump);
        self.free_register(source_reg);
        Ok(())
    }

    fn emit_return_from_try(&mut self, register: u16, span: Span) {
        if let Some(from_reg) = self
            .locals
            .iter()
            .filter(|local| local.is_captured)
            .map(|local| local.register)
            .min()
        {
            self.emit_a(OpCode::CloseUpvals, from_reg, 0, 0, span);
        }
        self.emit_a(OpCode::Return, register, 0, 0, span);
    }

    pub(super) fn compile_typed_match(
        &mut self,
        scrutinee: &aelys_sema::TypedExpr,
        arms: &[TypedMatchArm],
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let scrutinee_reg = self.alloc_register()?;
        self.compile_typed_expr(scrutinee, scrutinee_reg)?;
        let mut end_jumps = Vec::new();

        for arm in arms {
            self.begin_scope();
            let mut binding_registers = HashMap::new();
            self.reserve_pattern_bindings(&arm.pattern, &mut binding_registers)?;
            let mut failure_jumps = Vec::new();
            self.compile_pattern_tests(scrutinee_reg, &arm.pattern, &mut failure_jumps)?;
            self.compile_pattern_bindings(scrutinee_reg, &arm.pattern, &binding_registers)?;

            if let Some(guard) = &arm.guard {
                let guard_reg = self.alloc_register()?;
                self.compile_typed_expr(guard, guard_reg)?;
                failure_jumps.push(self.emit_jump_if(OpCode::JumpIfNot, guard_reg, guard.span));
                self.free_register(guard_reg);
            }

            self.compile_typed_match_body(&arm.body, dest, arm.span)?;
            end_jumps.push(self.emit_jump(OpCode::Jump, arm.span));
            self.end_scope();

            let next_arm = self.current_offset();
            for jump in failure_jumps {
                self.patch_jump_to(jump, next_arm);
            }
        }

        self.emit_a(OpCode::MatchFail, 0, 0, 0, span);
        let end = self.current_offset();
        for jump in end_jumps {
            self.patch_jump_to(jump, end);
        }
        self.free_register(scrutinee_reg);
        Ok(())
    }

    fn compile_typed_match_body(
        &mut self,
        body: &TypedMatchArmBody,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        match body {
            TypedMatchArmBody::Expr(expr) => self.compile_typed_expr(expr, dest),
            TypedMatchArmBody::Block(stmts) => {
                self.begin_scope();
                if stmts.is_empty() {
                    self.compile_literal_unit(dest, span)?;
                } else {
                    for stmt in &stmts[..stmts.len() - 1] {
                        self.compile_typed_stmt(stmt)?;
                    }
                    match &stmts[stmts.len() - 1].kind {
                        aelys_sema::TypedStmtKind::Expression(expr) => {
                            self.compile_typed_expr(expr, dest)?;
                        }
                        _ => {
                            self.compile_typed_stmt(&stmts[stmts.len() - 1])?;
                            self.compile_literal_unit(dest, span)?;
                        }
                    }
                }
                self.end_scope();
                Ok(())
            }
        }
    }

    fn compile_pattern_tests(
        &mut self,
        source: u16,
        pattern: &TypedPattern,
        failures: &mut Vec<usize>,
    ) -> Result<()> {
        match &pattern.kind {
            TypedPatternKind::Wildcard | TypedPatternKind::Binding(_) => Ok(()),
            TypedPatternKind::Int(value) => {
                let literal = self.alloc_register()?;
                let condition = self.alloc_register()?;
                self.compile_literal_int(*value, literal, pattern.span)?;
                self.emit_a(OpCode::Eq, condition, source, literal, pattern.span);
                failures.push(self.emit_jump_if(OpCode::JumpIfNot, condition, pattern.span));
                self.free_register(condition);
                self.free_register(literal);
                Ok(())
            }
            TypedPatternKind::String(value) => {
                let literal = self.alloc_register()?;
                let condition = self.alloc_register()?;
                self.compile_literal_string(value, literal, pattern.span)?;
                self.emit_a(OpCode::Eq, condition, source, literal, pattern.span);
                failures.push(self.emit_jump_if(OpCode::JumpIfNot, condition, pattern.span));
                self.free_register(condition);
                self.free_register(literal);
                Ok(())
            }
            TypedPatternKind::Bool(value) => {
                let literal = self.alloc_register()?;
                let condition = self.alloc_register()?;
                self.compile_literal_bool(*value, literal, pattern.span)?;
                self.emit_a(OpCode::Eq, condition, source, literal, pattern.span);
                failures.push(self.emit_jump_if(OpCode::JumpIfNot, condition, pattern.span));
                self.free_register(condition);
                self.free_register(literal);
                Ok(())
            }
            TypedPatternKind::Variant { path, fields } => {
                let tag = sum_tag(path).ok_or_else(|| {
                    aelys_common::error::CompileError::new(
                        aelys_common::error::CompileErrorKind::TypeInferenceError(
                            "unknown sum pattern".to_string(),
                        ),
                        pattern.span,
                        self.source.clone(),
                    )
                })?;
                let condition = self.alloc_register()?;
                self.emit_a(OpCode::SumTest, condition, source, tag, pattern.span);
                failures.push(self.emit_jump_if(OpCode::JumpIfNot, condition, pattern.span));
                self.free_register(condition);
                if let Some(field) = fields.first() {
                    let payload = self.alloc_register()?;
                    self.emit_a(OpCode::SumPayload, payload, source, 0, pattern.span);
                    self.compile_pattern_tests(payload, field, failures)?;
                    self.free_register(payload);
                }
                Ok(())
            }
            TypedPatternKind::Or(alternatives) => {
                let mut pending_failures = Vec::new();
                let mut success_jumps = Vec::new();
                for alternative in alternatives {
                    let alternative_start = self.current_offset();
                    for jump in pending_failures.drain(..) {
                        self.patch_jump_to(jump, alternative_start);
                    }
                    let mut alternative_failures = Vec::new();
                    self.compile_pattern_tests(source, alternative, &mut alternative_failures)?;
                    success_jumps.push(self.emit_jump(OpCode::Jump, alternative.span));
                    pending_failures = alternative_failures;
                }
                failures.extend(pending_failures);
                let join = self.current_offset();
                for jump in success_jumps {
                    self.patch_jump_to(jump, join);
                }
                Ok(())
            }
        }
    }

    fn reserve_pattern_bindings(
        &mut self,
        pattern: &TypedPattern,
        registers: &mut HashMap<String, u16>,
    ) -> Result<()> {
        match &pattern.kind {
            TypedPatternKind::Binding(name) => {
                if !registers.contains_key(name) {
                    let register = self.alloc_register()?;
                    self.add_local(
                        name.clone(),
                        false,
                        register,
                        aelys_sema::ResolvedType::from_infer_type(&pattern.ty),
                    );
                    registers.insert(name.clone(), register);
                }
            }
            TypedPatternKind::Variant { fields, .. } | TypedPatternKind::Or(fields) => {
                for field in fields {
                    self.reserve_pattern_bindings(field, registers)?;
                }
            }
            TypedPatternKind::Wildcard
            | TypedPatternKind::Int(_)
            | TypedPatternKind::String(_)
            | TypedPatternKind::Bool(_) => {}
        }
        Ok(())
    }

    fn compile_pattern_bindings(
        &mut self,
        source: u16,
        pattern: &TypedPattern,
        registers: &HashMap<String, u16>,
    ) -> Result<()> {
        match &pattern.kind {
            TypedPatternKind::Binding(name) => {
                let register = registers.get(name).copied().ok_or_else(|| {
                    aelys_common::error::CompileError::new(
                        aelys_common::error::CompileErrorKind::TypeInferenceError(
                            "pattern binding register is missing".to_string(),
                        ),
                        pattern.span,
                        self.source.clone(),
                    )
                })?;
                self.emit_a(OpCode::Move, register, source, 0, pattern.span);
                Ok(())
            }
            TypedPatternKind::Variant { fields, .. } => {
                if let Some(field) = fields.first() {
                    let payload = self.alloc_register()?;
                    self.emit_a(OpCode::SumPayload, payload, source, 0, pattern.span);
                    self.compile_pattern_bindings(payload, field, registers)?;
                    self.free_register(payload);
                }
                Ok(())
            }
            TypedPatternKind::Or(alternatives) => {
                if let Some(first) = alternatives.first() {
                    self.compile_pattern_bindings(source, first, registers)?;
                }
                Ok(())
            }
            TypedPatternKind::Wildcard
            | TypedPatternKind::Int(_)
            | TypedPatternKind::String(_)
            | TypedPatternKind::Bool(_) => Ok(()),
        }
    }
}

fn sum_tag(path: &[String]) -> Option<u8> {
    match path.last()?.as_str() {
        "None" => Some(4),
        "Some" => Some(SumTag::OptionSome as u8),
        "Ok" => Some(SumTag::ResultOk as u8),
        "Err" => Some(SumTag::ResultErr as u8),
        "Message" => Some(SumTag::ErrorMessage as u8),
        _ => None,
    }
}
