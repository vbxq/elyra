use super::super::Compiler;
use aelys_bytecode::OpCode;
use aelys_bytecode::TypeTag;
use aelys_common::Result;
use aelys_sema::{InferType, TypedExpr, TypedExprKind};
use aelys_syntax::{MemberSeparator, Span};

pub(super) enum Operand<'a> {
    Expr(&'a TypedExpr),
    Reg(u16),
    Int(i16),
}

fn sequence_native(member: &str, arity: usize) -> bool {
    matches!(
        (member, arity),
        ("insert" | "resize" | "swap" | "set", 2)
            | (
                "remove"
                    | "swap_remove"
                    | "extend"
                    | "append"
                    | "truncate"
                    | "contains"
                    | "index_of"
                    | "fill"
                    | "join"
                    | "sort_by",
                1
            )
            | ("clear" | "reverse" | "sort" | "first" | "last", 0)
    )
}

fn map_native(member: &str, arity: usize) -> bool {
    matches!(
        (member, arity),
        ("insert", 2)
            | ("get" | "contains_key" | "remove", 1)
            | ("len" | "is_empty" | "clear" | "keys" | "values", 0)
    )
}

fn element_tag(element: &InferType) -> TypeTag {
    if element.is_integer() {
        TypeTag::Int
    } else if element.is_float() {
        TypeTag::Float
    } else if *element == InferType::Bool {
        TypeTag::Bool
    } else {
        TypeTag::Object
    }
}

impl Compiler {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn compile_container_call(
        &mut self,
        object: &TypedExpr,
        member: &str,
        separator: MemberSeparator,
        args: &[TypedExpr],
        dest: u16,
        span: Span,
    ) -> Result<bool> {
        if separator == MemberSeparator::Path {
            if member == "new"
                && args.is_empty()
                && matches!(object.kind, TypedExprKind::Unit)
                && matches!(object.ty, InferType::Map(_, _))
            {
                self.call_native("__map_new", &[], dest, span)?;
                return Ok(true);
            }
            return Ok(false);
        }
        let mut operands = vec![Operand::Expr(object)];
        operands.extend(args.iter().map(Operand::Expr));
        match &object.ty {
            InferType::Vec(element)
            | InferType::Array(element)
            | InferType::FixedArray(element, _)
                if sequence_native(member, args.len()) =>
            {
                if member == "sort_by" {
                    self.compile_sort_by(object, element, &args[0], dest, span)?;
                } else {
                    self.call_native(&format!("__vec_{member}"), &operands, dest, span)?;
                }
                Ok(true)
            }
            InferType::Map(key, value) if map_native(member, args.len()) => {
                let tag = match member {
                    "keys" => Some(element_tag(key)),
                    "values" => Some(element_tag(value)),
                    _ => None,
                };
                if let Some(tag) = tag {
                    operands.push(Operand::Int(i16::from(tag as u8)));
                }
                self.call_native(&format!("__map_{member}"), &operands, dest, span)?;
                Ok(true)
            }
            InferType::Char
                if args.is_empty()
                    && matches!(
                        member,
                        "is_digit"
                            | "is_alpha"
                            | "is_whitespace"
                            | "is_alphanumeric"
                            | "to_upper"
                            | "to_lower"
                    ) =>
            {
                self.call_native(&format!("__char_{member}"), &operands, dest, span)?;
                Ok(true)
            }
            InferType::String if member == "push" && args.len() == 1 => {
                self.call_native("__string_push", &operands, dest, span)?;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    pub(super) fn compile_map_index(
        &mut self,
        object: &TypedExpr,
        index: &TypedExpr,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        self.call_native(
            "__map_index",
            &[Operand::Expr(object), Operand::Expr(index)],
            dest,
            span,
        )
    }

    pub(super) fn compile_map_index_assign(
        &mut self,
        object: &TypedExpr,
        index: &TypedExpr,
        value: &TypedExpr,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        self.call_native(
            "__map_insert",
            &[
                Operand::Expr(object),
                Operand::Expr(index),
                Operand::Expr(value),
            ],
            dest,
            span,
        )
    }

    pub(super) fn compile_char_cast(
        &mut self,
        inner: &TypedExpr,
        target: &InferType,
        dest: u16,
        span: Span,
    ) -> Result<bool> {
        let native = match (&inner.ty, target) {
            (InferType::Char, InferType::Char) => {
                return self.compile_typed_expr(inner, dest).map(|()| true);
            }
            (InferType::Char, _) => "__char_to_int",
            (_, InferType::Char) => "__char_from_int",
            _ => return Ok(false),
        };
        self.call_native(native, &[Operand::Expr(inner)], dest, span)?;
        Ok(true)
    }

    pub(super) fn call_native(
        &mut self,
        name: &str,
        operands: &[Operand<'_>],
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let count = operands.len() + 1;
        let base = self.alloc_consecutive_registers_for_call(count, span)?;
        self.alloc_consecutive_from(base, count)?;
        for (offset, operand) in operands.iter().enumerate() {
            let register = base
                + 1
                + u16::try_from(offset).expect("a native call takes a handful of operands");
            match operand {
                Operand::Expr(expr) => self.compile_typed_expr(expr, register)?,
                Operand::Reg(source) => self.emit_a(OpCode::Move, register, *source, 0, span),
                Operand::Int(value) => self.emit_b(OpCode::LoadI, register, *value, span),
            }
        }
        let global = self.get_or_create_global_index(name);
        self.accessed_globals.insert(name.to_string());
        let arity = self.checked_call_arity(operands.len(), span)?;
        self.emit_call_global_cached(base, global, arity, name, span);
        if dest != base {
            self.emit_a(OpCode::Move, dest, base, 0, span);
        }
        for offset in (0..count).rev() {
            self.free_register(base + u16::try_from(offset).expect("bounded by count"));
        }
        Ok(())
    }

    // the comparator runs as ordinary bytecode between two native steps of a merge sort that only ever holds indices
    fn compile_sort_by(
        &mut self,
        object: &TypedExpr,
        element: &InferType,
        comparator: &TypedExpr,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let sequence = self.alloc_register()?;
        self.compile_typed_expr(object, sequence)?;
        let state = self.alloc_register()?;
        self.call_native(
            "__vec_sort_by_begin",
            &[Operand::Reg(sequence)],
            state,
            span,
        )?;
        let call_base = self.alloc_consecutive_registers_for_call(4, span)?;
        self.alloc_consecutive_from(call_base, 4)?;
        let result = call_base + 1;
        let left = call_base + 2;
        let right = call_base + 3;
        self.compile_typed_expr(comparator, call_base)?;
        let more = self.alloc_register()?;
        let slot = self.alloc_register()?;
        self.emit_b(OpCode::LoadI, result, 0, span);
        let load = match &object.ty {
            InferType::Vec(_) => Self::element_load(
                element,
                [
                    OpCode::VecLoadI,
                    OpCode::VecLoadF,
                    OpCode::VecLoadB,
                    OpCode::VecLoadP,
                ],
            ),
            _ => Self::element_load(
                element,
                [
                    OpCode::ArrayLoadI,
                    OpCode::ArrayLoadF,
                    OpCode::ArrayLoadB,
                    OpCode::ArrayLoadP,
                ],
            ),
        };
        let loop_start = self.current_offset();
        self.call_native(
            "__vec_sort_by_step",
            &[Operand::Reg(state), Operand::Reg(result)],
            more,
            span,
        )?;
        let exit = self.emit_jump_if(OpCode::JumpIfNot, more, span);
        self.emit_b(OpCode::LoadI, slot, 0, span);
        self.emit_a(OpCode::VecLoadI, slot, state, slot, span);
        self.emit_a(load, left, sequence, slot, span);
        self.emit_b(OpCode::LoadI, slot, 1, span);
        self.emit_a(OpCode::VecLoadI, slot, state, slot, span);
        self.emit_a(load, right, sequence, slot, span);
        self.emit_c(OpCode::CallCached, result, call_base, 2, span);
        self.emit_jump_back(loop_start, span);
        self.patch_jump(exit);
        self.call_native(
            "__vec_sort_by_finish",
            &[Operand::Reg(state), Operand::Reg(sequence)],
            dest,
            span,
        )?;
        self.free_register(slot);
        self.free_register(more);
        self.free_register(right);
        self.free_register(left);
        self.free_register(result);
        self.free_register(call_base);
        self.free_register(state);
        self.free_register(sequence);
        Ok(())
    }

    fn element_load(element: &InferType, opcodes: [OpCode; 4]) -> OpCode {
        match element {
            t if t.is_integer() => opcodes[0],
            t if t.is_float() => opcodes[1],
            InferType::Bool => opcodes[2],
            _ => opcodes[3],
        }
    }
}
