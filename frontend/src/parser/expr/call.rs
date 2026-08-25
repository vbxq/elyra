use super::Parser;
use aelys_common::Result;
use aelys_syntax::{BinaryOp, Expr, ExprKind, MemberSeparator, TokenKind};

impl Parser {
    pub(super) fn call(&mut self) -> Result<Expr> {
        let mut expr = self.primary()?;

        loop {
            if self.check(&TokenKind::ColonColon) && self.peek_at(1).kind == TokenKind::Lt {
                self.advance();
                self.advance();
                let mut type_args = vec![self.parse_type_annotation()?];
                while self.match_token(&TokenKind::Comma) {
                    type_args.push(self.parse_type_annotation()?);
                }
                self.consume_generic_close()?;
                let span = expr.span.merge(self.previous().span);
                expr = Expr::new(
                    ExprKind::GenericApply {
                        callee: Box::new(expr),
                        type_args,
                    },
                    span,
                );
            } else if self.match_token(&TokenKind::LParen) {
                let args = self.with_brace_construction(true, |parser| {
                    let mut args = Vec::new();
                    if !parser.check(&TokenKind::RParen) {
                        loop {
                            args.push(parser.expression()?);
                            if !parser.match_token(&TokenKind::Comma) {
                                break;
                            }
                        }
                    }
                    Ok(args)
                })?;

                self.consume(&TokenKind::RParen, ")")?;
                let span = expr.span.merge(self.previous().span);

                expr = Expr::new(
                    ExprKind::Call {
                        callee: Box::new(expr),
                        args,
                    },
                    span,
                );
            } else if self.match_token(&TokenKind::Dot) {
                let member = self.consume_identifier("member name")?;
                let span = expr.span.merge(self.previous().span);

                expr = Expr::new(
                    ExprKind::Member {
                        object: Box::new(expr),
                        member,
                        separator: MemberSeparator::Dot,
                    },
                    span,
                );
            } else if self.match_token(&TokenKind::ColonColon) {
                let member = if self.match_token(&TokenKind::From) {
                    "from".to_string()
                } else {
                    self.consume_identifier("path segment")?
                };
                if !matches!(
                    &expr.kind,
                    ExprKind::Identifier(_)
                        | ExprKind::GenericApply { .. }
                        | ExprKind::Member {
                            separator: MemberSeparator::Path,
                            ..
                        }
                ) {
                    return Err(self.error(
                        aelys_common::error::CompileErrorKind::UnexpectedToken {
                            expected: "identifier in module path".to_string(),
                            found: member,
                        },
                    ));
                }
                let span = expr.span.merge(self.previous().span);

                expr = Expr::new(
                    ExprKind::Member {
                        object: Box::new(expr),
                        member,
                        separator: MemberSeparator::Path,
                    },
                    span,
                );
            } else if self.check(&TokenKind::LBrace) && self.brace_construction_allowed() {
                let Some((path, type_args)) = expression_path_with_type_args(&expr) else {
                    break;
                };
                if path.len() == 1 && !type_args.is_empty() {
                    let name = path[0].clone();
                    let fields = self.parse_struct_literal_fields()?;
                    let span = expr.span.merge(self.previous().span);
                    expr = Expr::new(
                        ExprKind::StructLiteral {
                            name,
                            type_args,
                            fields,
                        },
                        span,
                    );
                    continue;
                }
                if path.len() < 2 {
                    break;
                }
                self.advance();
                let fields = self.with_brace_construction(true, |parser| {
                    let mut fields = Vec::new();
                    while !parser.check(&TokenKind::RBrace) && !parser.is_at_end() {
                        let field_span = parser.peek().span;
                        let field_name = parser.consume_identifier("enum field name")?;
                        if fields
                            .iter()
                            .any(|field: &aelys_syntax::StructFieldInit| field.name == field_name)
                        {
                            return Err(parser.error(
                                aelys_common::error::CompileErrorKind::InvalidPattern {
                                    reason: format!("duplicate enum field '{field_name}'"),
                                },
                            ));
                        }
                        parser.consume(&TokenKind::Colon, ":")?;
                        let value = parser.expression()?;
                        let end_span = parser.previous().span;
                        fields.push(aelys_syntax::StructFieldInit {
                            name: field_name,
                            value: Box::new(value),
                            span: field_span.merge(end_span),
                        });
                        if !parser.match_token(&TokenKind::Comma) {
                            break;
                        }
                    }
                    Ok(fields)
                })?;
                self.consume(&TokenKind::RBrace, "}")?;
                let span = expr.span.merge(self.previous().span);
                expr = if type_args.is_empty() {
                    Expr::new(ExprKind::EnumLiteral { path, fields }, span)
                } else {
                    Expr::new(
                        ExprKind::GenericEnumLiteral {
                            path,
                            type_args,
                            fields,
                        },
                        span,
                    )
                };
            } else if self.match_token(&TokenKind::LBracket) {
                let index_or_range =
                    self.with_brace_construction(true, Parser::parse_index_or_range)?;
                self.consume(&TokenKind::RBracket, "]")?;
                let span = expr.span.merge(self.previous().span);

                if matches!(index_or_range.kind, ExprKind::Range { .. }) {
                    expr = Expr::new(
                        ExprKind::Slice {
                            object: Box::new(expr),
                            range: Box::new(index_or_range),
                        },
                        span,
                    );
                } else {
                    expr = Expr::new(
                        ExprKind::Index {
                            object: Box::new(expr),
                            index: Box::new(index_or_range),
                        },
                        span,
                    );
                }
            } else if self.check(&TokenKind::PlusPlus) || self.check(&TokenKind::MinusMinus) {
                let op = if self.match_token(&TokenKind::PlusPlus) {
                    BinaryOp::Add
                } else {
                    self.advance(); // consume MinusMinus
                    BinaryOp::Sub
                };
                let span = expr.span.merge(self.previous().span);

                if let ExprKind::Identifier(ref name) = expr.kind {
                    let one = Expr::new(ExprKind::Int(1), self.previous().span);
                    let binary = Expr::new(
                        ExprKind::Binary {
                            left: Box::new(expr.clone()),
                            op,
                            right: Box::new(one),
                        },
                        span,
                    );
                    expr = Expr::new(
                        ExprKind::Assign {
                            name: name.clone(),
                            value: Box::new(binary),
                        },
                        span,
                    );
                } else {
                    break;
                }
            } else if self.match_token(&TokenKind::As) {
                let target = self.parse_type_annotation()?;
                let span = expr.span.merge(self.previous().span);
                expr = Expr::new(
                    ExprKind::Cast {
                        expr: Box::new(expr),
                        target,
                    },
                    span,
                );
            } else if self.match_token(&TokenKind::Question) {
                let span = expr.span.merge(self.previous().span);
                expr = Expr::new(ExprKind::Try(Box::new(expr)), span);
            } else {
                break;
            }
        }

        Ok(expr)
    }

    fn parse_index_or_range(&mut self) -> Result<Expr> {
        let start_span = self.peek().span;

        if self.check(&TokenKind::DotDot) || self.check(&TokenKind::DotDotEq) {
            let inclusive = self.match_token(&TokenKind::DotDotEq);
            if !inclusive {
                self.advance(); // consume DotDot
            }

            let end = if !self.check(&TokenKind::RBracket) {
                Some(Box::new(self.expression()?))
            } else {
                None
            };

            let end_span = self.previous().span;
            return Ok(Expr::new(
                ExprKind::Range {
                    start: None,
                    end,
                    inclusive,
                },
                start_span.merge(end_span),
            ));
        }

        let first = self.expression()?;

        if self.check(&TokenKind::DotDot) || self.check(&TokenKind::DotDotEq) {
            let inclusive = self.match_token(&TokenKind::DotDotEq);
            if !inclusive {
                self.advance(); // consume DotDot
            }

            let end = if !self.check(&TokenKind::RBracket) {
                Some(Box::new(self.expression()?))
            } else {
                None
            };

            let end_span = self.previous().span;
            return Ok(Expr::new(
                ExprKind::Range {
                    start: Some(Box::new(first)),
                    end,
                    inclusive,
                },
                start_span.merge(end_span),
            ));
        }

        Ok(first)
    }
}

fn expression_path_with_type_args(
    expr: &Expr,
) -> Option<(Vec<String>, Vec<aelys_syntax::TypeAnnotation>)> {
    match &expr.kind {
        ExprKind::Identifier(name) => Some((vec![name.clone()], Vec::new())),
        ExprKind::GenericApply { callee, type_args } => {
            let (path, _) = expression_path_with_type_args(callee)?;
            Some((path, type_args.clone()))
        }
        ExprKind::Member {
            object,
            member,
            separator: MemberSeparator::Path,
        } => {
            let (mut path, type_args) = expression_path_with_type_args(object)?;
            path.push(member.clone());
            Some((path, type_args))
        }
        _ => None,
    }
}
