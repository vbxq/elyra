mod atom;
mod binary;
mod call;
mod unary;

use super::Parser;
use aelys_common::Result;
use aelys_common::error::{CompileError, CompileErrorKind};
use aelys_syntax::{
    BinaryOp, Expr, ExprKind, MatchArm, MatchArmBody, Pattern, PatternKind, Span, TokenKind,
};
use std::sync::Arc;

impl Parser {
    pub fn expression(&mut self) -> Result<Expr> {
        self.enter_recursion()?;
        let result = self.assignment();
        self.exit_recursion();
        result
    }

    fn assignment(&mut self) -> Result<Expr> {
        if self.check(&TokenKind::DotDot) || self.check(&TokenKind::DotDotEq) {
            return self.parse_expression_range(None);
        }

        let expr = self.or_expr()?;

        if self.check(&TokenKind::DotDot) || self.check(&TokenKind::DotDotEq) {
            return self.parse_expression_range(Some(expr));
        }

        if self.match_token(&TokenKind::Eq) {
            let value = self.assignment()?;

            if let ExprKind::Identifier(name) = expr.kind {
                let span = expr.span.merge(value.span);
                return Ok(Expr::new(
                    ExprKind::Assign {
                        name,
                        value: Box::new(value),
                    },
                    span,
                ));
            }

            if let ExprKind::Member {
                object,
                member,
                separator: aelys_syntax::MemberSeparator::Dot,
            } = expr.kind
            {
                let span = object.span.merge(value.span);
                return Ok(Expr::new(
                    ExprKind::MemberAssign {
                        object,
                        member,
                        value: Box::new(value),
                    },
                    span,
                ));
            }

            if let ExprKind::Index { object, index } = expr.kind {
                let span = object.span.merge(value.span);
                return Ok(Expr::new(
                    ExprKind::IndexAssign {
                        object,
                        index,
                        value: Box::new(value),
                    },
                    span,
                ));
            }

            return Err(CompileError::new(
                CompileErrorKind::InvalidAssignmentTarget,
                expr.span,
                Arc::clone(&self.source),
            )
            .into());
        }

        if let Some(op) = self.match_compound_assign() {
            let rhs = self.assignment()?;
            let end_span = self.previous().span;
            if !matches!(
                expr.kind,
                ExprKind::Identifier(_)
                    | ExprKind::Index { .. }
                    | ExprKind::Member {
                        separator: aelys_syntax::MemberSeparator::Dot,
                        ..
                    }
            ) {
                return Err(CompileError::new(
                    CompileErrorKind::InvalidAssignmentTarget,
                    expr.span,
                    Arc::clone(&self.source),
                )
                .into());
            }
            return Ok(self.desugar_compound(expr, op, rhs, end_span));
        }

        Ok(expr)
    }

    pub(super) fn desugar_compound(
        &mut self,
        target: Expr,
        op: BinaryOp,
        rhs: Expr,
        end_span: Span,
    ) -> Expr {
        let mut hoisted = Vec::new();
        let place = self.hoist_place_operands(target, &mut hoisted);
        let mut result = Self::compound_assign(place, op, rhs, end_span);
        for (name, operand) in hoisted.into_iter().rev() {
            let span = operand.span.merge(result.span);
            let pattern = Pattern {
                kind: PatternKind::Binding(name),
                span: operand.span,
            };
            result = Expr::new(
                ExprKind::Match {
                    scrutinee: Box::new(operand),
                    arms: vec![MatchArm {
                        pattern,
                        guard: None,
                        span: result.span,
                        body: MatchArmBody::Expr(Box::new(result)),
                    }],
                },
                span,
            );
        }
        result
    }

    fn compound_assign(place: Expr, op: BinaryOp, rhs: Expr, end_span: Span) -> Expr {
        let binary = Expr::new(
            ExprKind::Binary {
                left: Box::new(place.clone()),
                op,
                right: Box::new(rhs),
            },
            place.span.merge(end_span),
        );
        let span = place.span.merge(binary.span);
        let value = Box::new(binary);
        let kind = match place.kind {
            ExprKind::Identifier(name) => ExprKind::Assign { name, value },
            ExprKind::Index { object, index } => ExprKind::IndexAssign {
                object,
                index,
                value,
            },
            ExprKind::Member { object, member, .. } => ExprKind::MemberAssign {
                object,
                member,
                value,
            },
            _ => unreachable!("compound assignment target checked by the caller"),
        };
        Expr::new(kind, span)
    }

    fn hoist_place_operands(&mut self, expr: Expr, hoisted: &mut Vec<(String, Expr)>) -> Expr {
        let span = expr.span;
        let kind = match expr.kind {
            ExprKind::Index { object, index } => {
                let object = self.hoist_place_operands(*object, hoisted);
                let index = if Self::is_duplicable(&index) {
                    *index
                } else {
                    let name = format!("__aelys_place{}", self.synthetic_counter);
                    self.synthetic_counter += 1;
                    let index_span = index.span;
                    hoisted.push((name.clone(), *index));
                    Expr::new(ExprKind::Identifier(name), index_span)
                };
                ExprKind::Index {
                    object: Box::new(object),
                    index: Box::new(index),
                }
            }
            ExprKind::Member {
                object,
                member,
                separator: aelys_syntax::MemberSeparator::Dot,
            } => ExprKind::Member {
                object: Box::new(self.hoist_place_operands(*object, hoisted)),
                member,
                separator: aelys_syntax::MemberSeparator::Dot,
            },
            ExprKind::Grouping(inner) => {
                ExprKind::Grouping(Box::new(self.hoist_place_operands(*inner, hoisted)))
            }
            other => other,
        };
        Expr::new(kind, span)
    }

    fn is_duplicable(expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::String(_)
            | ExprKind::Bool(_)
            | ExprKind::Null
            | ExprKind::Unit
            | ExprKind::Identifier(_) => true,
            ExprKind::Grouping(inner) => Self::is_duplicable(inner),
            ExprKind::Unary { operand, .. } => Self::is_duplicable(operand),
            ExprKind::Cast { expr, .. } => Self::is_duplicable(expr),
            ExprKind::Binary { left, right, .. }
            | ExprKind::And { left, right }
            | ExprKind::Or { left, right } => {
                Self::is_duplicable(left) && Self::is_duplicable(right)
            }
            ExprKind::Member { object, .. } => Self::is_duplicable(object),
            ExprKind::Index { object, index } => {
                Self::is_duplicable(object) && Self::is_duplicable(index)
            }
            _ => false,
        }
    }

    fn parse_expression_range(&mut self, start: Option<Expr>) -> Result<Expr> {
        let range_start = start
            .as_ref()
            .map(|expr| expr.span)
            .unwrap_or(self.peek().span);
        let inclusive = self.match_token(&TokenKind::DotDotEq);
        if !inclusive {
            self.consume(&TokenKind::DotDot, "..")?;
        }
        let end = if matches!(
            self.peek().kind,
            TokenKind::Semicolon
                | TokenKind::RBracket
                | TokenKind::RParen
                | TokenKind::RBrace
                | TokenKind::Comma
                | TokenKind::Eof
                | TokenKind::Step
        ) {
            None
        } else {
            Some(Box::new(self.expression()?))
        };
        let end_span = end
            .as_ref()
            .map(|expr| expr.span)
            .unwrap_or(self.previous().span);
        Ok(Expr::new(
            ExprKind::Range {
                start: start.map(Box::new),
                end,
                inclusive,
            },
            range_start.merge(end_span),
        ))
    }

    fn match_compound_assign(&mut self) -> Option<BinaryOp> {
        let op = match self.peek().kind {
            TokenKind::PlusEq => BinaryOp::Add,
            TokenKind::MinusEq => BinaryOp::Sub,
            TokenKind::StarEq => BinaryOp::Mul,
            TokenKind::SlashEq => BinaryOp::Div,
            TokenKind::PercentEq => BinaryOp::Mod,
            _ => return None,
        };
        self.advance();
        Some(op)
    }

    fn or_expr(&mut self) -> Result<Expr> {
        let mut left = self.and_expr()?;

        while self.match_token(&TokenKind::Or) {
            let right = self.and_expr()?;
            let span = left.span.merge(right.span);
            left = Expr::new(
                ExprKind::Or {
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            );
        }

        Ok(left)
    }

    fn and_expr(&mut self) -> Result<Expr> {
        let mut left = self.bit_or()?;

        while self.match_token(&TokenKind::And) {
            let right = self.bit_or()?;
            let span = left.span.merge(right.span);
            left = Expr::new(
                ExprKind::And {
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            );
        }

        Ok(left)
    }

    pub fn match_binary_op(&mut self, kinds: &[TokenKind]) -> Option<BinaryOp> {
        for kind in kinds {
            if self.check(kind) {
                self.advance();
                return token_to_binary_op(kind);
            }
        }
        None
    }
}

fn token_to_binary_op(kind: &TokenKind) -> Option<BinaryOp> {
    match kind {
        TokenKind::Plus => Some(BinaryOp::Add),
        TokenKind::Minus => Some(BinaryOp::Sub),
        TokenKind::Star => Some(BinaryOp::Mul),
        TokenKind::Slash => Some(BinaryOp::Div),
        TokenKind::Percent => Some(BinaryOp::Mod),
        TokenKind::EqEq => Some(BinaryOp::Eq),
        TokenKind::BangEq => Some(BinaryOp::Ne),
        TokenKind::Lt => Some(BinaryOp::Lt),
        TokenKind::LtEq => Some(BinaryOp::Le),
        TokenKind::Gt => Some(BinaryOp::Gt),
        TokenKind::GtEq => Some(BinaryOp::Ge),
        TokenKind::Shl => Some(BinaryOp::Shl),
        TokenKind::Shr => Some(BinaryOp::Shr),
        TokenKind::Ampersand => Some(BinaryOp::BitAnd),
        TokenKind::Pipe => Some(BinaryOp::BitOr),
        TokenKind::Caret => Some(BinaryOp::BitXor),
        _ => None,
    }
}
