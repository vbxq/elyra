use super::Parser;
use crate::lexer::Lexer;
use aelys_common::Result;
use aelys_common::error::{CompileError, CompileErrorKind};
use aelys_syntax::{
    Expr, ExprKind, FmtPart, FmtStringPart, MatchArm, MatchArmBody, Pattern, PatternKind, Source,
    Stmt, StmtKind, StructFieldInit, StructPatternField, TokenKind,
};
use std::sync::Arc;

impl Parser {
    pub(super) fn lambda_expression(&mut self, start_span: aelys_syntax::Span) -> Result<Expr> {
        self.consume(&TokenKind::LParen, "(")?;

        let mut params = Vec::new();
        if !self.check(&TokenKind::RParen) {
            loop {
                params.push(self.parse_parameter()?);
                if !self.match_token(&TokenKind::Comma) {
                    break;
                }
            }
        }
        self.consume(&TokenKind::RParen, ")")?;

        let return_type = if self.match_token(&TokenKind::Arrow) {
            Some(self.parse_type_annotation()?)
        } else {
            None
        };

        let body = if self.check(&TokenKind::LBrace) {
            self.advance();
            self.block_statements()?
        } else {
            let expr = self.expression()?;
            vec![Stmt::new(StmtKind::Expression(expr.clone()), expr.span)]
        };

        let end_span = self.previous().span;

        Ok(Expr::new(
            ExprKind::Lambda {
                params,
                return_type,
                body,
            },
            start_span.merge(end_span),
        ))
    }

    pub(super) fn if_expression(&mut self, start_span: aelys_syntax::Span) -> Result<Expr> {
        if self.check(&TokenKind::Let) {
            return self.if_let_expression(start_span, true);
        }
        let condition = self.with_brace_construction(false, Parser::expression)?;
        self.consume(&TokenKind::LBrace, "{")?;

        let then_branch = self.block_expression()?;

        self.consume(&TokenKind::Else, "else")?;
        let else_branch = self.else_expression()?;

        let end_span = self.previous().span;

        Ok(Expr::new(
            ExprKind::If {
                condition: Box::new(condition),
                then_branch: Box::new(then_branch),
                else_branch: Box::new(else_branch),
            },
            start_span.merge(end_span),
        ))
    }

    fn else_expression(&mut self) -> Result<Expr> {
        if self.match_token(&TokenKind::If) {
            let span = self.previous().span;
            return self.if_expression(span);
        }
        self.consume(&TokenKind::LBrace, "{")?;
        self.block_expression()
    }

    pub(crate) fn if_let_expression(
        &mut self,
        start_span: aelys_syntax::Span,
        else_required: bool,
    ) -> Result<Expr> {
        if !else_required {
            return self.if_let_chain(start_span).map(|(expr, _)| expr);
        }
        self.consume(&TokenKind::Let, "let")?;
        let pattern = self.parse_pattern()?;
        self.consume(&TokenKind::Eq, "=")?;
        let scrutinee = self.with_brace_construction(false, Parser::expression)?;
        self.consume(&TokenKind::LBrace, "{")?;
        let then_start = self.previous().span;
        let then_body = self.block_statements()?;
        let then_span = then_start.merge(self.previous().span);

        let (else_body, else_span) = if self.match_token(&TokenKind::Else) {
            let else_start = self.peek().span;
            let else_expr = self.else_expression()?;
            let span = else_start.merge(self.previous().span);
            let body = match else_expr.kind {
                ExprKind::Block(stmts) => stmts,
                _ => vec![Stmt::new(StmtKind::Expression(else_expr), span)],
            };
            (body, span)
        } else {
            return Err(self.error(CompileErrorKind::UnexpectedToken {
                expected: "else".to_string(),
                found: self.peek().kind.to_string(),
            }));
        };
        let span = start_span.merge(self.previous().span);
        Ok(Self::if_let_match(
            pattern, scrutinee, then_body, then_span, else_body, else_span, span,
        ))
    }

    // the flag tells whether the chain ends with a plain else block
    pub(crate) fn if_let_chain(&mut self, start_span: aelys_syntax::Span) -> Result<(Expr, bool)> {
        self.consume(&TokenKind::Let, "let")?;
        let pattern = self.parse_pattern()?;
        self.consume(&TokenKind::Eq, "=")?;
        let scrutinee = self.with_brace_construction(false, Parser::expression)?;
        self.consume(&TokenKind::LBrace, "{")?;
        let then_start = self.previous().span;
        let mut then_body = self.block_statements()?;
        let then_span = then_start.merge(self.previous().span);

        let (mut else_body, else_span, complete) = if self.match_token(&TokenKind::Else) {
            let else_start = self.peek().span;
            let (stmt, complete) = self.else_statement_chain()?;
            let span = else_start.merge(self.previous().span);
            let body = match stmt.kind {
                StmtKind::Block(stmts) => stmts,
                _ => vec![stmt],
            };
            (body, span, complete)
        } else {
            (Vec::new(), then_span, false)
        };
        if !complete {
            then_body.push(Stmt::new(
                StmtKind::Expression(Expr::new(ExprKind::Unit, then_span)),
                then_span,
            ));
            if !else_body.is_empty() {
                else_body.push(Stmt::new(
                    StmtKind::Expression(Expr::new(ExprKind::Unit, else_span)),
                    else_span,
                ));
            }
        }
        let span = start_span.merge(self.previous().span);
        Ok((
            Self::if_let_match(
                pattern, scrutinee, then_body, then_span, else_body, else_span, span,
            ),
            complete,
        ))
    }

    fn if_let_match(
        pattern: Pattern,
        scrutinee: Expr,
        then_body: Vec<Stmt>,
        then_span: aelys_syntax::Span,
        else_body: Vec<Stmt>,
        else_span: aelys_syntax::Span,
        span: aelys_syntax::Span,
    ) -> Expr {
        let arms = vec![
            MatchArm {
                span: pattern.span.merge(then_span),
                pattern,
                guard: None,
                body: MatchArmBody::Block(then_body),
                synthetic: false,
            },
            MatchArm {
                pattern: Pattern {
                    kind: PatternKind::Wildcard,
                    span: else_span,
                },
                guard: None,
                body: MatchArmBody::Block(else_body),
                span: else_span,
                synthetic: true,
            },
        ];
        Expr::new(
            ExprKind::Match {
                scrutinee: Box::new(scrutinee),
                arms,
            },
            span,
        )
    }

    pub(super) fn match_expression(&mut self, start_span: aelys_syntax::Span) -> Result<Expr> {
        let scrutinee = self.with_brace_construction(false, Parser::expression)?;
        self.consume(&TokenKind::LBrace, "{")?;
        let arms = self.with_brace_construction(true, Parser::match_arms)?;

        self.consume(&TokenKind::RBrace, "}")?;
        Ok(Expr::new(
            ExprKind::Match {
                scrutinee: Box::new(scrutinee),
                arms,
            },
            start_span.merge(self.previous().span),
        ))
    }

    fn match_arms(&mut self) -> Result<Vec<MatchArm>> {
        let mut arms = Vec::new();

        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            if self.match_token(&TokenKind::Semicolon) || self.match_token(&TokenKind::Comma) {
                continue;
            }

            let pattern = self.parse_pattern()?;
            let guard = if self.match_token(&TokenKind::If) {
                Some(self.expression()?)
            } else {
                None
            };
            self.consume(&TokenKind::FatArrow, "=>")?;

            let body = if self.match_token(&TokenKind::LBrace) {
                MatchArmBody::Block(self.block_statements()?)
            } else {
                MatchArmBody::Expr(Box::new(self.expression()?))
            };
            let end_span = self.previous().span;
            let span = pattern.span.merge(end_span);
            arms.push(MatchArm {
                pattern,
                guard,
                body,
                span,
                synthetic: false,
            });

            if self.match_token(&TokenKind::Comma) || self.match_token(&TokenKind::Semicolon) {
                continue;
            }
            if !self.check(&TokenKind::RBrace) {
                return Err(self.error(CompileErrorKind::UnexpectedToken {
                    expected: "comma, semicolon, or '}'".to_string(),
                    found: self.peek().kind.to_string(),
                }));
            }
        }

        Ok(arms)
    }

    pub(crate) fn parse_pattern(&mut self) -> Result<Pattern> {
        let first = self.parse_pattern_atom()?;
        if !self.check(&TokenKind::Pipe) {
            return Ok(first);
        }

        let mut alternatives = vec![first];
        while self.match_token(&TokenKind::Pipe) {
            alternatives.push(self.parse_pattern_atom()?);
        }
        let span = alternatives
            .first()
            .map(|pattern| pattern.span)
            .unwrap_or_else(|| self.peek().span)
            .merge(
                alternatives
                    .last()
                    .map(|pattern| pattern.span)
                    .unwrap_or_else(|| self.peek().span),
            );
        Ok(Pattern {
            kind: PatternKind::Or(alternatives),
            span,
        })
    }

    fn parse_pattern_atom(&mut self) -> Result<Pattern> {
        let token = self.advance().clone();
        let start_span = token.span;
        let kind = match token.kind {
            TokenKind::Identifier(name) if name == "_" => PatternKind::Wildcard,
            TokenKind::Identifier(name) => {
                let mut path = vec![name];
                let mut type_args = Vec::new();
                while self.match_token(&TokenKind::ColonColon) {
                    if self.match_token(&TokenKind::Lt) {
                        type_args.push(self.parse_type_annotation()?);
                        while self.match_token(&TokenKind::Comma) {
                            type_args.push(self.parse_type_annotation()?);
                        }
                        self.consume_generic_close()?;
                    } else {
                        path.push(self.consume_identifier("variant name")?);
                    }
                }

                let fields = if self.match_token(&TokenKind::LParen) {
                    let mut fields = Vec::new();
                    if !self.check(&TokenKind::RParen) {
                        loop {
                            fields.push(self.parse_pattern()?);
                            if !self.match_token(&TokenKind::Comma) {
                                break;
                            }
                            if self.check(&TokenKind::RParen) {
                                break;
                            }
                        }
                    }
                    self.consume(&TokenKind::RParen, ")")?;
                    fields
                } else {
                    Vec::new()
                };

                if self.match_token(&TokenKind::LBrace) {
                    let mut struct_fields = Vec::new();
                    let mut has_rest = false;
                    while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
                        if self.match_token(&TokenKind::DotDot) {
                            has_rest = true;
                            if self.match_token(&TokenKind::Comma)
                                && !self.check(&TokenKind::RBrace)
                            {
                                return Err(self.error(CompileErrorKind::InvalidPattern {
                                    reason: "struct rest must be the last pattern".to_string(),
                                }));
                            }
                            break;
                        }
                        let field_span = self.peek().span;
                        let field_name = self.consume_identifier("struct pattern field")?;
                        let field_pattern = if self.match_token(&TokenKind::Colon) {
                            self.parse_pattern()?
                        } else {
                            Pattern {
                                kind: PatternKind::Binding(field_name.clone()),
                                span: field_span,
                            }
                        };
                        if struct_fields
                            .iter()
                            .any(|field: &StructPatternField| field.name == field_name)
                        {
                            return Err(self.error(CompileErrorKind::InvalidPattern {
                                reason: format!("duplicate struct pattern field '{field_name}'"),
                            }));
                        }
                        struct_fields.push(StructPatternField {
                            name: field_name,
                            pattern: field_pattern.clone(),
                            span: field_span.merge(field_pattern.span),
                        });
                        if !self.match_token(&TokenKind::Comma) {
                            break;
                        }
                    }
                    self.consume(&TokenKind::RBrace, "}")?;
                    PatternKind::Struct {
                        path,
                        type_args,
                        fields: struct_fields,
                        has_rest,
                    }
                } else {
                    let is_variant_name = matches!(
                        path.last().map(String::as_str),
                        Some("Ok" | "Err" | "Some" | "None")
                    );
                    if path.len() == 1 && fields.is_empty() && !is_variant_name {
                        self.reject_reserved_name(&path[0], start_span)?;
                        PatternKind::Binding(path.remove(0))
                    } else {
                        PatternKind::Variant {
                            path,
                            type_args,
                            fields,
                        }
                    }
                }
            }
            TokenKind::LParen => {
                let mut elements = Vec::new();
                if !self.check(&TokenKind::RParen) {
                    loop {
                        elements.push(self.parse_pattern()?);
                        if !self.match_token(&TokenKind::Comma) || self.check(&TokenKind::RParen) {
                            break;
                        }
                    }
                }
                self.consume(&TokenKind::RParen, ")")?;
                match elements.len() {
                    0 => {
                        return Err(self.error(CompileErrorKind::InvalidPattern {
                            reason: "`()` is not a pattern; use `_`".to_string(),
                        }));
                    }
                    1 => return Ok(elements.remove(0)),
                    arity => PatternKind::Struct {
                        path: vec![self.tuple_struct_name(arity)?],
                        type_args: Vec::new(),
                        fields: elements
                            .into_iter()
                            .enumerate()
                            .map(|(index, pattern)| StructPatternField {
                                name: index.to_string(),
                                span: pattern.span,
                                pattern,
                            })
                            .collect(),
                        has_rest: false,
                    },
                }
            }
            TokenKind::Int(value) => self.range_pattern_tail(value)?,
            TokenKind::Minus => {
                let value = match self.advance().kind {
                    TokenKind::Int(value) => -value,
                    _ => {
                        return Err(self.error(CompileErrorKind::ExpectedPattern));
                    }
                };
                self.range_pattern_tail(value)?
            }
            TokenKind::DotDotEq | TokenKind::DotDot => {
                let inclusive = token.kind == TokenKind::DotDotEq;
                let end = self.range_pattern_bound()?;
                range_pattern(None, Some(end), inclusive)
                    .map_err(|reason| self.error(CompileErrorKind::InvalidPattern { reason }))?
            }
            TokenKind::String(value) => PatternKind::String(value),
            TokenKind::Char(value) => PatternKind::Char(value),
            TokenKind::True => PatternKind::Bool(true),
            TokenKind::False => PatternKind::Bool(false),
            TokenKind::Null => return Err(self.error(CompileErrorKind::NullIsNotInSurface)),
            _ => return Err(self.error(CompileErrorKind::ExpectedPattern)),
        };

        Ok(Pattern {
            kind,
            span: start_span.merge(self.previous().span),
        })
    }

    fn range_pattern_tail(&mut self, start: i64) -> Result<PatternKind> {
        let inclusive = if self.match_token(&TokenKind::DotDotEq) {
            true
        } else if self.match_token(&TokenKind::DotDot) {
            false
        } else {
            return Ok(PatternKind::Int(start));
        };
        let end = if matches!(self.peek().kind, TokenKind::Int(_) | TokenKind::Minus) {
            Some(self.range_pattern_bound()?)
        } else if inclusive {
            return Err(self.error(CompileErrorKind::InvalidPattern {
                reason: "an inclusive range pattern needs an end; write `lo..` for an open range"
                    .to_string(),
            }));
        } else {
            None
        };
        range_pattern(Some(start), end, inclusive)
            .map_err(|reason| self.error(CompileErrorKind::InvalidPattern { reason }))
    }

    fn range_pattern_bound(&mut self) -> Result<i64> {
        let negative = self.match_token(&TokenKind::Minus);
        match self.advance().kind {
            TokenKind::Int(value) => Ok(if negative { -value } else { value }),
            _ => Err(self.error(CompileErrorKind::InvalidPattern {
                reason: "a range pattern bound must be an integer literal".to_string(),
            })),
        }
    }

    pub(super) fn block_expression(&mut self) -> Result<Expr> {
        let start_span = self.previous().span;
        let mut stmts = self.block_statements()?;
        let span = start_span.merge(self.previous().span);
        if stmts.is_empty() {
            return Ok(Expr::new(ExprKind::Unit, span));
        }
        if stmts.len() == 1
            && let StmtKind::Expression(expr) = &stmts[0].kind
            && !matches!(expr.kind, ExprKind::Loop { .. })
        {
            let Some(Stmt {
                kind: StmtKind::Expression(expr),
                ..
            }) = stmts.pop()
            else {
                unreachable!("the single statement was matched as an expression above");
            };
            return Ok(expr);
        }
        Ok(Expr::new(ExprKind::Block(stmts), span))
    }

    pub(super) fn primary(&mut self) -> Result<Expr> {
        if self.is_at_end() {
            return Err(self.error(CompileErrorKind::ExpectedExpression));
        }
        let token = self.advance();
        let span = token.span;
        let token_kind = token.kind.clone();

        let kind = match token_kind {
            TokenKind::Int(n) => ExprKind::Int(n),
            TokenKind::Float(n) => ExprKind::Float(n),
            TokenKind::String(s) => ExprKind::String(s),
            TokenKind::Char(c) => ExprKind::Char(c),
            TokenKind::FmtString(parts) => {
                return self.parse_fmt_string(parts, span);
            }
            TokenKind::True => ExprKind::Bool(true),
            TokenKind::False => ExprKind::Bool(false),
            TokenKind::Null => return Err(self.error(CompileErrorKind::NullIsNotInSurface)),
            TokenKind::Match => return self.match_expression(span),
            TokenKind::Identifier(ref name) if name == "vec" && self.check(&TokenKind::Bang) => {
                return self.vec_macro_literal(span);
            }
            TokenKind::Identifier(ref name)
                if name.eq_ignore_ascii_case("array") || name.eq_ignore_ascii_case("vec") =>
            {
                let name = name.clone();
                return self.typed_collection_literal(name, span);
            }
            TokenKind::Identifier(ref name)
                if name.chars().next().is_some_and(|c| c.is_uppercase())
                    && self.check(&TokenKind::LBrace)
                    && self.brace_construction_allowed()
                    && matches!(self.peek_at(1).kind, TokenKind::Identifier(_))
                    && matches!(self.peek_at(2).kind, TokenKind::Colon) =>
            {
                let name = name.clone();
                return self.struct_literal(name, Vec::new(), span);
            }
            TokenKind::Identifier(ref name)
                if name == "loop"
                    && self.check(&TokenKind::LBrace)
                    && self.brace_construction_allowed() =>
            {
                return self.loop_expression(span, None);
            }
            TokenKind::Label(label)
                if self.check(&TokenKind::Colon)
                    && matches!(&self.peek_at(1).kind, TokenKind::Identifier(name) if name == "loop")
                    && self.peek_at(2).kind == TokenKind::LBrace =>
            {
                self.advance();
                let loop_span = self.advance().span;
                return self.loop_expression(span.merge(loop_span), Some(label));
            }
            TokenKind::Identifier(name) => ExprKind::Identifier(name),

            TokenKind::LBrace if self.brace_construction_allowed() => {
                return self.block_expression();
            }

            TokenKind::LBracket => {
                return self.array_literal(span);
            }

            TokenKind::LParen => {
                if self.check(&TokenKind::RParen) {
                    let end_span = self.advance().span;
                    return Ok(Expr::new(ExprKind::Unit, span.merge(end_span)));
                }
                let inner = self.with_brace_construction(true, Parser::expression)?;
                if self.check(&TokenKind::Comma) {
                    return self.tuple_literal(inner, span);
                }
                self.consume(&TokenKind::RParen, ")")?;
                let end_span = self.previous().span;
                return Ok(Expr::new(
                    ExprKind::Grouping(Box::new(inner)),
                    span.merge(end_span),
                ));
            }

            TokenKind::If => {
                return self.if_expression(span);
            }

            TokenKind::Fn => {
                return self.lambda_expression(span);
            }

            _ => {
                return Err(CompileError::new(
                    CompileErrorKind::ExpectedExpression,
                    span,
                    Arc::clone(&self.source),
                )
                .into());
            }
        };

        Ok(Expr::new(kind, span))
    }

    fn tuple_literal(&mut self, first: Expr, start_span: aelys_syntax::Span) -> Result<Expr> {
        let mut elements = vec![first];
        self.with_brace_construction(true, |parser| {
            while parser.match_token(&TokenKind::Comma) {
                if parser.check(&TokenKind::RParen) {
                    break;
                }
                elements.push(parser.expression()?);
            }
            Ok(())
        })?;
        self.consume(&TokenKind::RParen, ")")?;
        let span = start_span.merge(self.previous().span);
        if elements.len() < 2 {
            return Err(self.error(CompileErrorKind::UnexpectedToken {
                expected: "a second tuple element".to_string(),
                found: ")".to_string(),
            }));
        }
        let name = self.tuple_struct_name(elements.len())?;
        let fields = elements
            .into_iter()
            .enumerate()
            .map(|(index, value)| StructFieldInit {
                name: index.to_string(),
                span: value.span,
                value: Box::new(value),
            })
            .collect();
        Ok(Expr::new(
            ExprKind::StructLiteral {
                name,
                type_args: Vec::new(),
                fields,
            },
            span,
        ))
    }

    fn array_literal(&mut self, start_span: aelys_syntax::Span) -> Result<Expr> {
        self.with_brace_construction(true, |parser| parser.array_literal_body(start_span))
    }

    fn array_literal_body(&mut self, start_span: aelys_syntax::Span) -> Result<Expr> {
        if self.match_token(&TokenKind::Semicolon) {
            if !self.legacy_collections {
                return Err(self.error(CompileErrorKind::UnexpectedToken {
                    expected: "Rust-style collection syntax".to_string(),
                    found: "legacy collection syntax".to_string(),
                }));
            }
            let size = self.expression()?;
            self.consume(&TokenKind::RBracket, "]")?;
            let end_span = self.previous().span;
            return Ok(Expr::new(
                ExprKind::ArraySized {
                    element_type: None,
                    size: Box::new(size),
                },
                start_span.merge(end_span),
            ));
        }

        let mut elements = Vec::new();

        if !self.check(&TokenKind::RBracket) {
            let first = self.expression()?;
            elements.push(first);
            if self.match_token(&TokenKind::Semicolon) {
                let repeat = self.expression()?;
                self.consume(&TokenKind::RBracket, "]")?;
                let end_span = self.previous().span;
                return Ok(Expr::with_repeat(
                    ExprKind::ArrayLiteral {
                        element_type: None,
                        elements,
                    },
                    start_span.merge(end_span),
                    repeat,
                ));
            }
            while self.match_token(&TokenKind::Comma) {
                if self.check(&TokenKind::RBracket) {
                    break;
                }
                elements.push(self.expression()?);
            }
        }

        self.consume(&TokenKind::RBracket, "]")?;
        let end_span = self.previous().span;

        Ok(Expr::new(
            ExprKind::ArrayLiteral {
                element_type: None,
                elements,
            },
            start_span.merge(end_span),
        ))
    }

    fn vec_macro_literal(&mut self, start_span: aelys_syntax::Span) -> Result<Expr> {
        self.with_brace_construction(true, |parser| parser.vec_macro_literal_body(start_span))
    }

    fn vec_macro_literal_body(&mut self, start_span: aelys_syntax::Span) -> Result<Expr> {
        self.consume(&TokenKind::Bang, "!")?;
        self.consume(&TokenKind::LBracket, "[")?;
        let mut elements = Vec::new();

        if !self.check(&TokenKind::RBracket) {
            let first = self.expression()?;
            elements.push(first);
            if self.match_token(&TokenKind::Semicolon) {
                let repeat = self.expression()?;
                self.consume(&TokenKind::RBracket, "]")?;
                let end_span = self.previous().span;
                return Ok(Expr::with_repeat(
                    ExprKind::VecLiteral {
                        element_type: None,
                        elements,
                    },
                    start_span.merge(end_span),
                    repeat,
                ));
            }
            while self.match_token(&TokenKind::Comma) {
                if self.check(&TokenKind::RBracket) {
                    break;
                }
                elements.push(self.expression()?);
            }
        }

        self.consume(&TokenKind::RBracket, "]")?;
        let end_span = self.previous().span;
        Ok(Expr::new(
            ExprKind::VecLiteral {
                element_type: None,
                elements,
            },
            start_span.merge(end_span),
        ))
    }

    fn typed_collection_literal(
        &mut self,
        collection_name: String,
        start_span: aelys_syntax::Span,
    ) -> Result<Expr> {
        self.with_brace_construction(true, |parser| {
            parser.typed_collection_literal_body(collection_name, start_span)
        })
    }

    fn typed_collection_literal_body(
        &mut self,
        collection_name: String,
        start_span: aelys_syntax::Span,
    ) -> Result<Expr> {
        if !self.legacy_collections {
            return Err(self.error(CompileErrorKind::UnexpectedToken {
                expected: "Rust-style collection syntax".to_string(),
                found: "legacy collection syntax".to_string(),
            }));
        }

        let element_type = if self.match_token(&TokenKind::Lt) {
            let type_ann = self.parse_type_annotation()?;
            self.consume(&TokenKind::Gt, ">")?;
            Some(type_ann)
        } else {
            None
        };

        if collection_name.eq_ignore_ascii_case("array") && self.match_token(&TokenKind::LParen) {
            let size = self.expression()?;
            self.consume(&TokenKind::RParen, ")")?;
            let end_span = self.previous().span;

            return Ok(Expr::new(
                ExprKind::ArraySized {
                    element_type,
                    size: Box::new(size),
                },
                start_span.merge(end_span),
            ));
        }

        self.consume(&TokenKind::LBracket, "[")?;

        if self.match_token(&TokenKind::Semicolon) {
            let size = self.expression()?;
            self.consume(&TokenKind::RBracket, "]")?;
            let end_span = self.previous().span;
            return Ok(Expr::new(
                ExprKind::ArraySized {
                    element_type,
                    size: Box::new(size),
                },
                start_span.merge(end_span),
            ));
        }

        let mut elements = Vec::new();
        if !self.check(&TokenKind::RBracket) {
            loop {
                elements.push(self.expression()?);
                if !self.match_token(&TokenKind::Comma) {
                    break;
                }
                if self.check(&TokenKind::RBracket) {
                    break;
                }
            }
        }

        self.consume(&TokenKind::RBracket, "]")?;
        let end_span = self.previous().span;

        let kind = if collection_name.eq_ignore_ascii_case("vec") {
            ExprKind::VecLiteral {
                element_type,
                elements,
            }
        } else {
            ExprKind::ArrayLiteral {
                element_type,
                elements,
            }
        };

        Ok(Expr::new(kind, start_span.merge(end_span)))
    }

    fn struct_literal(
        &mut self,
        name: String,
        type_args: Vec<aelys_syntax::TypeAnnotation>,
        start_span: aelys_syntax::Span,
    ) -> Result<Expr> {
        let fields = self.parse_struct_literal_fields()?;
        let end_span = self.previous().span;

        Ok(Expr::new(
            ExprKind::StructLiteral {
                name,
                type_args,
                fields,
            },
            start_span.merge(end_span),
        ))
    }

    pub(super) fn parse_struct_literal_fields(&mut self) -> Result<Vec<StructFieldInit>> {
        self.consume(&TokenKind::LBrace, "{")?;

        self.with_brace_construction(true, |parser| {
            let mut fields = Vec::new();
            while !parser.check(&TokenKind::RBrace) && !parser.is_at_end() {
                let field_span = parser.peek().span;
                let field_name = parser.consume_identifier("field name")?;
                if fields
                    .iter()
                    .any(|field: &StructFieldInit| field.name == field_name)
                {
                    return Err(parser.error(CompileErrorKind::InvalidPattern {
                        reason: format!("duplicate struct literal field '{field_name}'"),
                    }));
                }
                parser.consume(&TokenKind::Colon, ":")?;
                let value = parser.expression()?;
                let end_span = parser.previous().span;

                fields.push(StructFieldInit {
                    name: field_name,
                    value: Box::new(value),
                    span: field_span.merge(end_span),
                });

                if !parser.match_token(&TokenKind::Comma) {
                    break;
                }
            }

            parser.consume(&TokenKind::RBrace, "}")?;
            Ok(fields)
        })
    }

    fn parse_fmt_string(&mut self, parts: Vec<FmtPart>, span: aelys_syntax::Span) -> Result<Expr> {
        let mut result = Vec::new();

        for part in parts {
            match part {
                FmtPart::Literal(s) => result.push(FmtStringPart::Literal(s)),
                FmtPart::Placeholder => result.push(FmtStringPart::Placeholder),
                FmtPart::Expr(expr_str) => {
                    let expr = self.parse_inline_expr(&expr_str, span)?;
                    result.push(FmtStringPart::Expr(Box::new(expr)));
                }
            }
        }

        Ok(Expr::new(ExprKind::FmtString(result), span))
    }

    fn parse_inline_expr(&self, code: &str, span: aelys_syntax::Span) -> Result<Expr> {
        let source = Source::new("<fmt-expr>", code);
        let lexer = Lexer::with_source(Arc::clone(&source));
        let tokens = lexer.scan().map_err(|e| {
            CompileError::new(
                CompileErrorKind::UnexpectedToken {
                    expected: "expression".to_string(),
                    found: format!("invalid expression in format string: {}", e),
                },
                span,
                Arc::clone(&self.source),
            )
        })?;

        let mut parser = Parser::new_rust_collections(tokens, source);
        let mut expr = parser.expression().map_err(|e| {
            aelys_common::error::AelysError::Compile(CompileError::new(
                CompileErrorKind::UnexpectedToken {
                    expected: "expression".to_string(),
                    found: format!("invalid expression in format string: {}", e),
                },
                span,
                Arc::clone(&self.source),
            ))
        })?;

        remap_expr_spans(&mut expr, span);
        Ok(expr)
    }
}

fn range_pattern(
    start: Option<i64>,
    end: Option<i64>,
    inclusive: bool,
) -> std::result::Result<PatternKind, String> {
    if let (Some(start), Some(end)) = (start, end) {
        let empty = if inclusive { start > end } else { start >= end };
        if empty {
            return Err(format!(
                "range pattern {start}{}{end} matches no value",
                if inclusive { "..=" } else { ".." }
            ));
        }
    }
    Ok(PatternKind::Range {
        start,
        end,
        inclusive,
    })
}

fn remap_expr_spans(expr: &mut Expr, span: aelys_syntax::Span) {
    expr.span = span;
    if let Some(repeat) = &mut expr.repeat {
        remap_expr_spans(repeat, span);
    }
    match &mut expr.kind {
        ExprKind::Binary { left, right, .. } => {
            remap_expr_spans(left, span);
            remap_expr_spans(right, span);
        }
        ExprKind::Unary { operand, .. } => {
            remap_expr_spans(operand, span);
        }
        ExprKind::And { left, right } | ExprKind::Or { left, right } => {
            remap_expr_spans(left, span);
            remap_expr_spans(right, span);
        }
        ExprKind::Call { callee, args } => {
            remap_expr_spans(callee, span);
            for arg in args {
                remap_expr_spans(arg, span);
            }
        }
        ExprKind::GenericApply { callee, .. } => {
            remap_expr_spans(callee, span);
        }
        ExprKind::Assign { value, .. } => {
            remap_expr_spans(value, span);
        }
        ExprKind::MemberAssign { object, value, .. } => {
            remap_expr_spans(object, span);
            remap_expr_spans(value, span);
        }
        ExprKind::Grouping(inner) => {
            remap_expr_spans(inner, span);
        }
        ExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            remap_expr_spans(condition, span);
            remap_expr_spans(then_branch, span);
            remap_expr_spans(else_branch, span);
        }
        ExprKind::Lambda { body, .. } => {
            for stmt in body {
                remap_stmt_spans(stmt, span);
            }
        }
        ExprKind::Member { object, .. } => {
            remap_expr_spans(object, span);
        }
        ExprKind::ArrayLiteral { elements, .. } | ExprKind::VecLiteral { elements, .. } => {
            for el in elements {
                remap_expr_spans(el, span);
            }
        }
        ExprKind::ArraySized { size, .. } => {
            remap_expr_spans(size, span);
        }
        ExprKind::Index { object, index } => {
            remap_expr_spans(object, span);
            remap_expr_spans(index, span);
        }
        ExprKind::IndexAssign {
            object,
            index,
            value,
        } => {
            remap_expr_spans(object, span);
            remap_expr_spans(index, span);
            remap_expr_spans(value, span);
        }
        ExprKind::Range { start, end, .. } => {
            if let Some(s) = start {
                remap_expr_spans(s, span);
            }
            if let Some(e) = end {
                remap_expr_spans(e, span);
            }
        }
        ExprKind::Slice { object, range } => {
            remap_expr_spans(object, span);
            remap_expr_spans(range, span);
        }
        ExprKind::FmtString(parts) => {
            for part in parts {
                if let FmtStringPart::Expr(e) = part {
                    remap_expr_spans(e, span);
                }
            }
        }
        ExprKind::StructLiteral { fields, .. } => {
            for field in fields {
                remap_expr_spans(&mut field.value, span);
            }
        }
        ExprKind::EnumLiteral { fields, .. } => {
            for field in fields {
                remap_expr_spans(&mut field.value, span);
            }
        }
        ExprKind::GenericEnumLiteral { fields, .. } => {
            for field in fields {
                remap_expr_spans(&mut field.value, span);
            }
        }
        ExprKind::Cast { expr, .. } => {
            remap_expr_spans(expr, span);
        }
        ExprKind::Block(body) | ExprKind::Loop { body, .. } => {
            for stmt in body {
                remap_stmt_spans(stmt, span);
            }
        }
        _ => {}
    }
}

fn remap_stmt_spans(stmt: &mut Stmt, span: aelys_syntax::Span) {
    stmt.span = span;
    match &mut stmt.kind {
        StmtKind::Expression(expr) => remap_expr_spans(expr, span),
        StmtKind::Let { initializer, .. } => remap_expr_spans(initializer, span),
        StmtKind::Block(stmts) => {
            for s in stmts {
                remap_stmt_spans(s, span);
            }
        }
        StmtKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            remap_expr_spans(condition, span);
            remap_stmt_spans(then_branch, span);
            if let Some(e) = else_branch {
                remap_stmt_spans(e, span);
            }
        }
        StmtKind::While {
            condition, body, ..
        } => {
            remap_expr_spans(condition, span);
            remap_stmt_spans(body, span);
        }
        StmtKind::For {
            start,
            end,
            step,
            body,
            ..
        } => {
            remap_expr_spans(start, span);
            remap_expr_spans(end, span);
            if let Some(s) = step.as_mut() {
                remap_expr_spans(s, span);
            }
            remap_stmt_spans(body, span);
        }
        StmtKind::Return(Some(expr))
        | StmtKind::Break {
            value: Some(expr), ..
        } => remap_expr_spans(expr, span),
        StmtKind::ForEach { iterable, body, .. } => {
            remap_expr_spans(iterable, span);
            remap_stmt_spans(body, span);
        }
        StmtKind::Function(func) => {
            for s in &mut func.body {
                remap_stmt_spans(s, span);
            }
        }
        StmtKind::ImplDecl { methods, .. } => {
            for method in methods {
                for s in &mut method.body {
                    remap_stmt_spans(s, span);
                }
            }
        }
        StmtKind::TraitDecl { methods, .. } => {
            for method in methods {
                for s in &mut method.function.body {
                    remap_stmt_spans(s, span);
                }
            }
        }
        _ => {}
    }
}
