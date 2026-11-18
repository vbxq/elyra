use super::Parser;
use aelys_common::Result;
use aelys_syntax::{Expr, ExprKind, TokenKind, UnaryOp};

impl Parser {
    pub(super) fn unary(&mut self) -> Result<Expr> {
        if self.match_token(&TokenKind::Ampersand) {
            let start = self.previous().span;
            let mutable = self.match_token(&TokenKind::Mut);
            let operand = self.nested(Self::unary)?;
            let span = start.merge(operand.span);
            return Ok(Expr::new(
                ExprKind::Borrow {
                    mutable,
                    operand: Box::new(operand),
                },
                span,
            ));
        }

        if self.match_token(&TokenKind::Minus) {
            let start = self.previous().span;
            let operand = self.nested(Self::unary)?;
            let span = start.merge(operand.span);

            if super::atom::is_unsigned_int_min(&operand) {
                return Ok(Expr::new(ExprKind::Int(i64::MIN), span));
            }
            match operand.kind {
                ExprKind::Int(n) => {
                    return Ok(Expr::new(ExprKind::Int(n.wrapping_neg()), span));
                }
                ExprKind::Float(f) => {
                    return Ok(Expr::new(ExprKind::Float(-f), span));
                }
                _ => {}
            }

            return Ok(Expr::new(
                ExprKind::Unary {
                    op: UnaryOp::Neg,
                    operand: Box::new(operand),
                },
                span,
            ));
        }

        self.reject_word_not()?;
        if self.match_token(&TokenKind::Bang) {
            let start = self.previous().span;
            let operand = self.nested(Self::unary)?;
            let span = start.merge(operand.span);
            return Ok(Expr::new(
                ExprKind::Unary {
                    op: UnaryOp::Not,
                    operand: Box::new(operand),
                },
                span,
            ));
        }

        if self.match_token(&TokenKind::Tilde) {
            let start = self.previous().span;
            let operand = self.nested(Self::unary)?;
            let span = start.merge(operand.span);
            return Ok(Expr::new(
                ExprKind::Unary {
                    op: UnaryOp::BitNot,
                    operand: Box::new(operand),
                },
                span,
            ));
        }

        self.call()
    }
}
