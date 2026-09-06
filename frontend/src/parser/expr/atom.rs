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
        let condition = self.with_brace_construction(false, Parser::expression)?;
        self.consume(&TokenKind::LBrace, "{")?;

        let then_branch = self.block_expression()?;

        self.consume(&TokenKind::Else, "else")?;
        self.consume(&TokenKind::LBrace, "{")?;
        let else_branch = self.block_expression()?;

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

    fn parse_pattern(&mut self) -> Result<Pattern> {
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
            TokenKind::Int(value) => PatternKind::Int(value),
            TokenKind::Minus => {
                let value = match self.advance().kind {
                    TokenKind::Int(value) => -value,
                    _ => {
                        return Err(self.error(CompileErrorKind::ExpectedPattern));
                    }
                };
                PatternKind::Int(value)
            }
            TokenKind::String(value) => PatternKind::String(value),
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

    pub(super) fn block_expression(&mut self) -> Result<Expr> {
        self.with_brace_construction(true, |parser| {
            let mut stmts = Vec::new();

            while !parser.check(&TokenKind::RBrace) && !parser.is_at_end() {
                if parser.is_expression_start() {
                    let expr = parser.expression()?;

                    if parser.check(&TokenKind::RBrace) {
                        parser.consume(&TokenKind::RBrace, "}")?;

                        return Ok(expr);
                    }

                    parser.consume_semicolon()?;
                    let span = expr.span;
                    stmts.push(Stmt::new(StmtKind::Expression(expr), span));
                } else {
                    if parser.match_token(&TokenKind::Semicolon) {
                        continue;
                    }
                    stmts.push(parser.declaration()?);
                }
            }

            parser.consume(&TokenKind::RBrace, "}")?;

            Ok(Expr::new(ExprKind::Unit, parser.previous().span))
        })
    }

    pub(super) fn is_expression_start(&self) -> bool {
        matches!(
            self.peek().kind,
            TokenKind::Int(_)
                | TokenKind::Float(_)
                | TokenKind::String(_)
                | TokenKind::FmtString(_)
                | TokenKind::True
                | TokenKind::False
                | TokenKind::Null
                | TokenKind::Match
                | TokenKind::Identifier(_)
                | TokenKind::LParen
                | TokenKind::LBracket
                | TokenKind::Minus
                | TokenKind::Not
                | TokenKind::If
                | TokenKind::Fn
        )
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
            TokenKind::Identifier(name) => ExprKind::Identifier(name),

            TokenKind::LBracket => {
                return self.array_literal(span);
            }

            TokenKind::LParen => {
                if self.check(&TokenKind::RParen) {
                    let end_span = self.advance().span;
                    return Ok(Expr::new(ExprKind::Unit, span.merge(end_span)));
                }
                let inner = self.with_brace_construction(true, Parser::expression)?;
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
        StmtKind::While { condition, body } => {
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
        StmtKind::Return(Some(expr)) => remap_expr_spans(expr, span),
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
