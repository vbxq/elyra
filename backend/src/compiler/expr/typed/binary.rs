use super::super::Compiler;
use aelys_bytecode::OpCode;
use aelys_common::Result;
use aelys_syntax::Span;
use aelys_syntax::ast::BinaryOp;

impl Compiler {
    pub(super) fn get_typed_local_register(&self, expr: &aelys_sema::TypedExpr) -> Option<u16> {
        if let aelys_sema::TypedExprKind::Identifier(name) = &expr.kind
            && let Some((reg, _)) = self.resolve_variable(name)
        {
            return Some(reg);
        }
        if let aelys_sema::TypedExprKind::Int(literal) = &expr.kind {
            return self.hoisted_literal(*literal);
        }
        None
    }

    pub(super) fn compile_typed_binary(
        &mut self,
        left: &aelys_sema::TypedExpr,
        op: BinaryOp,
        right: &aelys_sema::TypedExpr,
        dest: u16,
        span: Span,
    ) -> Result<()> {
        let left_resolved = aelys_sema::ResolvedType::from_infer_type(&left.ty);
        let right_resolved = aelys_sema::ResolvedType::from_infer_type(&right.ty);
        let opcode = crate::opcode_select::select_opcode(op, &left_resolved, &right_resolved);

        // a small literal on the right becomes the operand, which spares a register and a LoadI
        if let aelys_sema::TypedExprKind::Int(literal) = &right.kind
            && (0..=255).contains(literal)
            && let Some(immediate) = match (op, opcode) {
                (BinaryOp::Add, OpCode::AddII) => Some(OpCode::AddI),
                (BinaryOp::Sub, OpCode::SubII) => Some(OpCode::SubI),
                (BinaryOp::Shr, OpCode::ShrII) => Some(OpCode::ShrIImm),
                (BinaryOp::BitAnd, OpCode::AndII) => Some(OpCode::AndIImm),
                (BinaryOp::BitOr, OpCode::OrII) => Some(OpCode::OrIImm),
                (BinaryOp::BitXor, OpCode::XorII) => Some(OpCode::XorIImm),
                _ => None,
            }
        {
            let left_reg = match self.get_typed_local_register(left) {
                Some(register) => register,
                None => {
                    self.compile_typed_expr(left, dest)?;
                    dest
                }
            };
            self.emit_a(
                immediate,
                dest,
                left_reg,
                u8::try_from(*literal).expect("immediate was range checked"),
                span,
            );
            return Ok(());
        }

        // a call on the right can write a mutable local through a closure, so it is read first
        let left_may_change = matches!(&left.kind, aelys_sema::TypedExprKind::Identifier(name)
                if self.resolve_variable(name).is_some_and(|(_, mutable)| mutable))
            && Self::typed_expr_may_have_side_effects(right);

        if !left_may_change && let Some(left_local_reg) = self.get_typed_local_register(left) {
            let (right_reg, right_needs_free) =
                if let Some(r) = self.get_typed_local_register(right) {
                    (r, false)
                } else {
                    let temp = self.alloc_register()?;
                    self.compile_typed_expr(right, temp)?;
                    (temp, true)
                };

            self.emit_a(opcode, dest, left_local_reg, right_reg, span);

            if right_needs_free {
                self.free_register(right_reg);
            }
            return Ok(());
        }

        if let Some(right_local_reg) = self.get_typed_local_register(right) {
            self.compile_typed_expr(left, dest)?;
            self.emit_a(opcode, dest, dest, right_local_reg, span);
            return Ok(());
        }

        let right_has_side_effects = Self::typed_expr_may_have_side_effects(right);

        if right_has_side_effects {
            let left_reg = self.alloc_register()?;
            self.compile_typed_expr(left, left_reg)?;

            let right_reg = self.alloc_register()?;
            self.compile_typed_expr(right, right_reg)?;

            self.emit_a(opcode, dest, left_reg, right_reg, span);

            self.free_register(right_reg);
            self.free_register(left_reg);
        } else {
            self.compile_typed_expr(left, dest)?;

            let right_reg = self.alloc_register()?;
            self.compile_typed_expr(right, right_reg)?;

            self.emit_a(opcode, dest, dest, right_reg, span);

            self.free_register(right_reg);
        }

        Ok(())
    }

    pub(super) fn try_compile_fixed_point_call(
        &mut self,
        callee: &aelys_sema::TypedExpr,
        args: &[aelys_sema::TypedExpr],
        dest: u16,
        span: Span,
    ) -> Result<bool> {
        let aelys_sema::TypedExprKind::Member {
            object,
            member,
            separator: aelys_syntax::MemberSeparator::Dot,
        } = &callee.kind
        else {
            return Ok(false);
        };
        let opcode = match member.as_str() {
            "mul_shr" => OpCode::MulShr,
            "shl_div" => OpCode::ShlDiv,
            _ => return Ok(false),
        };
        let [right, shift] = args else {
            return Ok(false);
        };
        let resolved = aelys_sema::ResolvedType::from_infer_type(&object.ty);
        let kind = match crate::opcode_select::sized_kind(&resolved) {
            Some(kind) => aelys_bytecode::FixedKind::Sized(kind),
            None if resolved == aelys_sema::ResolvedType::I64 => aelys_bytecode::FixedKind::Int,
            None => return Ok(false),
        };
        let mut temporaries = Vec::new();
        let mut operand = |this: &mut Self, expr: &aelys_sema::TypedExpr| -> Result<u16> {
            if let Some(register) = this.get_typed_local_register(expr) {
                return Ok(register);
            }
            let register = this.alloc_register()?;
            this.compile_typed_expr(expr, register)?;
            temporaries.push(register);
            Ok(register)
        };
        let left_register = operand(self, object)?;
        let right_register = operand(self, right)?;
        let shift = match &shift.kind {
            aelys_sema::TypedExprKind::Int(value) if (0..=63).contains(value) => {
                aelys_bytecode::FixedShift::Immediate(
                    u8::try_from(*value).expect("the shift was range checked"),
                )
            }
            _ => aelys_bytecode::FixedShift::Register(operand(self, shift)?),
        };
        let line = self.current_line(span);
        self.current.emit_fixed(
            opcode,
            kind,
            shift,
            dest,
            left_register,
            right_register,
            line,
        );
        for register in temporaries.into_iter().rev() {
            self.free_register(register);
        }
        Ok(true)
    }
}
