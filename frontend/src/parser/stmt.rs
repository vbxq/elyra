use super::Parser;
use aelys_common::Result;
use aelys_common::error::CompileErrorKind;
use aelys_syntax::{
    Expr, ExprKind, MatchArm, MatchArmBody, Pattern, PatternKind, Stmt, StmtKind, TokenKind,
};

impl Parser {
    pub fn statement(&mut self) -> Result<Stmt> {
        if let TokenKind::Label(label) = self.peek().kind.clone()
            && self.peek_at(1).kind == TokenKind::Colon
        {
            self.advance();
            self.advance();
            return self.labeled_loop_statement(label);
        }

        if self.check(&TokenKind::If) && self.peek_at(1).kind == TokenKind::Let {
            let span = self.advance().span;
            let expr = self.if_let_expression(span, false)?;
            self.match_token(&TokenKind::Semicolon);
            let span = expr.span;
            return Ok(Stmt::new(StmtKind::Expression(expr), span));
        }

        if self.match_token(&TokenKind::If) {
            return self.if_statement();
        }

        if self.check(&TokenKind::While) {
            return self.labeled_loop_statement_body(None);
        }

        if self.is_loop_keyword() {
            return self.labeled_loop_statement_body(None);
        }

        if self.match_token(&TokenKind::For) {
            return self.for_statement(None);
        }

        if self.match_token(&TokenKind::Break) {
            let span = self.previous().span;
            let label = self.optional_label();
            let value = if matches!(
                self.peek().kind,
                TokenKind::Semicolon | TokenKind::RBrace | TokenKind::Eof
            ) {
                None
            } else {
                Some(self.expression()?)
            };
            self.consume_semicolon()?;
            let span = span.merge(self.previous().span);
            return Ok(Stmt::new(StmtKind::Break { label, value }, span));
        }

        if self.match_token(&TokenKind::Continue) {
            let span = self.previous().span;
            let label = self.optional_label();
            self.consume_semicolon()?;
            let span = span.merge(self.previous().span);
            return Ok(Stmt::new(StmtKind::Continue { label }, span));
        }

        if self.match_token(&TokenKind::Return) {
            return self.return_statement();
        }

        if self.match_token(&TokenKind::LBrace) {
            return Ok(Stmt::new(
                StmtKind::Block(self.block_statements()?),
                self.previous().span,
            ));
        }

        self.expression_statement()
    }

    pub(crate) fn arm_jump(&mut self) -> Result<Option<Stmt>> {
        let ends_arm = |kind: &TokenKind| {
            matches!(
                kind,
                TokenKind::Comma | TokenKind::Semicolon | TokenKind::RBrace | TokenKind::Eof
            )
        };
        let span = self.peek().span;
        let kind = if self.match_token(&TokenKind::Break) {
            let label = self.optional_label();
            let value = if ends_arm(&self.peek().kind) {
                None
            } else {
                Some(self.expression()?)
            };
            StmtKind::Break { label, value }
        } else if self.match_token(&TokenKind::Continue) {
            StmtKind::Continue {
                label: self.optional_label(),
            }
        } else if self.match_token(&TokenKind::Return) {
            if ends_arm(&self.peek().kind) {
                StmtKind::Return(None)
            } else {
                StmtKind::Return(Some(self.expression()?))
            }
        } else {
            return Ok(None);
        };
        Ok(Some(Stmt::new(kind, span.merge(self.previous().span))))
    }

    pub(crate) fn is_loop_keyword(&self) -> bool {
        matches!(&self.peek().kind, TokenKind::Identifier(name) if name == "loop")
            && self.peek_at(1).kind == TokenKind::LBrace
    }

    fn optional_label(&mut self) -> Option<String> {
        if let TokenKind::Label(label) = self.peek().kind.clone() {
            self.advance();
            Some(label)
        } else {
            None
        }
    }

    fn labeled_loop_statement(&mut self, label: String) -> Result<Stmt> {
        if self.match_token(&TokenKind::For) {
            return self.for_statement(Some(label));
        }
        if self.check(&TokenKind::While) || self.is_loop_keyword() {
            return self.labeled_loop_statement_body(Some(label));
        }
        Err(self.error(CompileErrorKind::UnexpectedToken {
            expected: "for, while, or loop after a label".to_string(),
            found: self.peek().kind.to_string(),
        }))
    }

    fn labeled_loop_statement_body(&mut self, label: Option<String>) -> Result<Stmt> {
        if self.is_loop_keyword() {
            let span = self.advance().span;
            let expr = self.loop_expression(span, label)?;
            self.match_token(&TokenKind::Semicolon);
            let span = expr.span;
            return Ok(Stmt::new(StmtKind::Expression(expr), span));
        }
        self.consume(&TokenKind::While, "while")?;
        if self.check(&TokenKind::Let) {
            let span = self.previous().span;
            let expr = self.while_let_expression(span, label)?;
            self.match_token(&TokenKind::Semicolon);
            let span = expr.span;
            return Ok(Stmt::new(StmtKind::Expression(expr), span));
        }
        self.while_statement(label)
    }

    pub(crate) fn loop_expression(
        &mut self,
        start_span: aelys_syntax::Span,
        label: Option<String>,
    ) -> Result<Expr> {
        self.consume(&TokenKind::LBrace, "{")?;
        let body = self.block_statements()?;
        let span = start_span.merge(self.previous().span);
        Ok(Expr::new(ExprKind::Loop { body, label }, span))
    }

    fn while_let_expression(
        &mut self,
        start_span: aelys_syntax::Span,
        label: Option<String>,
    ) -> Result<Expr> {
        self.consume(&TokenKind::Let, "let")?;
        let pattern = self.parse_pattern()?;
        self.consume(&TokenKind::Eq, "=")?;
        let scrutinee = self.with_brace_construction(false, Parser::expression)?;
        self.consume(&TokenKind::LBrace, "{")?;
        let body = self.block_statements()?;
        let span = start_span.merge(self.previous().span);
        let mut body_stmts = body;
        body_stmts.push(Stmt::new(
            StmtKind::Expression(Expr::new(ExprKind::Unit, span)),
            span,
        ));
        let arms = vec![
            MatchArm {
                span: pattern.span,
                pattern,
                guard: None,
                body: MatchArmBody::Block(body_stmts),
                synthetic: false,
            },
            MatchArm {
                pattern: Pattern {
                    kind: PatternKind::Wildcard,
                    span,
                },
                guard: None,
                body: MatchArmBody::Block(vec![Stmt::new(
                    StmtKind::Break {
                        label: None,
                        value: None,
                    },
                    span,
                )]),
                span,
                synthetic: true,
            },
        ];
        let matcher = Expr::new(
            ExprKind::Match {
                scrutinee: Box::new(scrutinee),
                arms,
            },
            span,
        );
        Ok(Expr::new(
            ExprKind::Loop {
                body: vec![Stmt::new(StmtKind::Expression(matcher), span)],
                label,
            },
            span,
        ))
    }

    fn if_statement(&mut self) -> Result<Stmt> {
        self.if_statement_chain().map(|(stmt, _)| stmt)
    }

    // the flag tells whether the chain ends with a plain else block
    pub(crate) fn if_statement_chain(&mut self) -> Result<(Stmt, bool)> {
        let start_span = self.previous().span;
        let condition = self.with_brace_construction(false, Parser::expression)?;
        self.consume(&TokenKind::LBrace, "{")?;
        let then_branch = self.block_statement()?;

        let (else_branch, complete) = if self.match_token(&TokenKind::Else) {
            let (stmt, complete) = self.else_statement_chain()?;
            (Some(Box::new(stmt)), complete)
        } else {
            (None, false)
        };

        let end_span = self.previous().span;

        Ok((
            Stmt::new(
                StmtKind::If {
                    condition,
                    then_branch: Box::new(then_branch),
                    else_branch,
                },
                start_span.merge(end_span),
            ),
            complete,
        ))
    }

    pub(crate) fn else_statement_chain(&mut self) -> Result<(Stmt, bool)> {
        if self.match_token(&TokenKind::If) {
            if self.check(&TokenKind::Let) {
                let span = self.previous().span;
                let (expr, complete) = self.if_let_chain(span)?;
                let span = expr.span;
                return Ok((Stmt::new(StmtKind::Expression(expr), span), complete));
            }
            return self.if_statement_chain();
        }
        self.consume(&TokenKind::LBrace, "{")?;
        Ok((self.block_statement()?, true))
    }

    fn while_statement(&mut self, label: Option<String>) -> Result<Stmt> {
        let start_span = self.previous().span;
        let condition = self.with_brace_construction(false, Parser::expression)?;
        self.consume(&TokenKind::LBrace, "{")?;
        let body = self.block_statement()?;
        let end_span = self.previous().span;

        Ok(Stmt::new(
            StmtKind::While {
                condition,
                body: Box::new(body),
                label,
            },
            start_span.merge(end_span),
        ))
    }

    fn bind_before(mut bindings: Vec<Stmt>, body: Stmt) -> Stmt {
        if bindings.is_empty() {
            return body;
        }
        let span = body.span;
        bindings.push(body);
        Stmt::new(StmtKind::Block(bindings), span)
    }

    fn for_statement(&mut self, label: Option<String>) -> Result<Stmt> {
        let start_span = self.previous().span;
        let pattern = if self.starts_destructuring_pattern() {
            Some(self.parse_pattern()?)
        } else {
            None
        };
        let (iterator, bindings) = match &pattern {
            Some(pattern) => self.destructure(pattern, "for", false)?,
            None => (
                self.consume_declared_identifier("loop variable")?,
                Vec::new(),
            ),
        };
        self.consume(&TokenKind::In, "in")?;
        let read_only = self.match_token(&TokenKind::Ampersand);

        let first_expr = self.with_brace_construction(false, Parser::expression)?;

        if let ExprKind::Range {
            start: Some(start),
            end: Some(end),
            inclusive,
        } = first_expr.kind
        {
            let step = if self.match_step_keyword() {
                Some(self.with_brace_construction(false, Parser::expression)?)
            } else {
                None
            };

            self.consume(&TokenKind::LBrace, "{")?;
            let body = Self::bind_before(bindings, self.block_statement()?);
            let end_span = self.previous().span;

            return Ok(Stmt::new(
                StmtKind::For {
                    iterator,
                    start: *start,
                    end: *end,
                    inclusive,
                    step: Box::new(step),
                    body: Box::new(body),
                    label,
                },
                start_span.merge(end_span),
            ));
        }

        if self.check(&TokenKind::DotDot) || self.check(&TokenKind::DotDotEq) {
            let inclusive = if self.match_token(&TokenKind::DotDotEq) {
                true
            } else {
                self.consume(&TokenKind::DotDot, "..")?;
                false
            };
            let end = self.with_brace_construction(false, Parser::expression)?;

            let step = if self.match_step_keyword() {
                Some(self.with_brace_construction(false, Parser::expression)?)
            } else {
                None
            };

            self.consume(&TokenKind::LBrace, "{")?;
            let body = Self::bind_before(bindings, self.block_statement()?);
            let end_span = self.previous().span;

            Ok(Stmt::new(
                StmtKind::For {
                    iterator,
                    start: first_expr,
                    end,
                    inclusive,
                    step: Box::new(step),
                    body: Box::new(body),
                    label,
                },
                start_span.merge(end_span),
            ))
        } else {
            self.consume(&TokenKind::LBrace, "{")?;
            let body = Self::bind_before(bindings, self.block_statement()?);
            let end_span = self.previous().span;

            Ok(if read_only {
                Stmt::read_only(
                    StmtKind::ForEach {
                        iterator,
                        iterable: first_expr,
                        body: Box::new(body),
                        label,
                    },
                    start_span.merge(end_span),
                )
            } else {
                Stmt::new(
                    StmtKind::ForEach {
                        iterator,
                        iterable: first_expr,
                        body: Box::new(body),
                        label,
                    },
                    start_span.merge(end_span),
                )
            })
        }
    }

    fn return_statement(&mut self) -> Result<Stmt> {
        let start_span = self.previous().span;

        if self.check(&TokenKind::Semicolon) {
            self.consume_semicolon()?;
            return Ok(Stmt::new(StmtKind::Return(None), start_span));
        }
        if self.check(&TokenKind::RBrace) {
            return Ok(Stmt::new(StmtKind::Return(None), start_span));
        }

        let value = self.expression()?;
        self.consume_semicolon()?;
        let end_span = self.previous().span;

        Ok(Stmt::new(
            StmtKind::Return(Some(value)),
            start_span.merge(end_span),
        ))
    }

    fn expression_statement(&mut self) -> Result<Stmt> {
        let expr = self.expression()?;
        self.consume_semicolon()?;
        let span = expr.span;
        Ok(Stmt::new(StmtKind::Expression(expr), span))
    }

    fn block_statement(&mut self) -> Result<Stmt> {
        let start_span = self.previous().span;
        let stmts = self.block_statements()?;
        let end_span = self.previous().span;
        Ok(Stmt::new(
            StmtKind::Block(stmts),
            start_span.merge(end_span),
        ))
    }

    pub(crate) fn block_statements(&mut self) -> Result<Vec<Stmt>> {
        self.nested(|parser| parser.block_statements_unguarded())
    }

    fn block_statements_unguarded(&mut self) -> Result<Vec<Stmt>> {
        self.with_brace_construction(true, |parser| {
            let mut stmts = Vec::new();

            while !parser.check(&TokenKind::RBrace) && !parser.is_at_end() {
                if parser.match_token(&TokenKind::Semicolon) {
                    continue;
                }
                stmts.push(parser.declaration()?);
                stmts.append(&mut parser.pending);
            }

            parser.consume(&TokenKind::RBrace, "}")?;
            Ok(stmts)
        })
    }

    pub(crate) fn consume_semicolon(&mut self) -> Result<()> {
        if self.match_token(&TokenKind::Semicolon) || self.check(&TokenKind::RBrace) {
            Ok(())
        } else {
            Err(self.error(CompileErrorKind::UnexpectedToken {
                expected: "semicolon or newline".to_string(),
                found: self.peek().kind.to_string(),
            }))
        }
    }
}
