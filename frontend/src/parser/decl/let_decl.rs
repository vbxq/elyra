use super::Parser;
use aelys_common::Result;
use aelys_common::error::CompileErrorKind;
use aelys_syntax::{
    Expr, ExprKind, MatchArm, MatchArmBody, MemberSeparator, Pattern, PatternKind, Span, Stmt,
    StmtKind, StructPatternField, TokenKind,
};

impl Parser {
    pub(super) fn let_declaration(&mut self, is_pub: bool) -> Result<Stmt> {
        let start_span = self.peek().span;
        self.advance();

        let mutable = self.match_token(&TokenKind::Mut);
        if self.starts_destructuring_pattern() {
            return self.destructuring_let(start_span, mutable, is_pub);
        }
        let name = self.consume_identifier("variable name")?;

        let type_annotation = if self.match_token(&TokenKind::Colon) {
            Some(self.parse_type_annotation()?)
        } else {
            None
        };

        self.consume(&TokenKind::Eq, "=")?;
        let initializer = self.expression()?;
        self.consume_semicolon()?;

        let end_span = self.previous().span;

        Ok(Stmt::new(
            StmtKind::Let {
                name,
                mutable,
                type_annotation,
                initializer,
                is_pub,
            },
            start_span.merge(end_span),
        ))
    }

    fn starts_destructuring_pattern(&self) -> bool {
        match &self.peek().kind {
            TokenKind::LParen => true,
            TokenKind::Identifier(name) => {
                name.chars().next().is_some_and(char::is_uppercase)
                    && matches!(
                        self.peek_at(1).kind,
                        TokenKind::LBrace | TokenKind::LParen | TokenKind::ColonColon
                    )
            }
            _ => false,
        }
    }

    fn destructuring_let(&mut self, start_span: Span, mutable: bool, is_pub: bool) -> Result<Stmt> {
        let pattern = self.parse_pattern()?;
        if is_pub {
            return Err(self.error(CompileErrorKind::InvalidPattern {
                reason: "a destructuring `let` cannot be `pub`".to_string(),
            }));
        }
        let type_annotation = if self.match_token(&TokenKind::Colon) {
            Some(self.parse_type_annotation()?)
        } else {
            None
        };
        self.consume(&TokenKind::Eq, "=")?;
        let initializer = self.expression()?;
        self.consume_semicolon()?;
        let span = start_span.merge(self.previous().span);

        let check = irrefutable_check_pattern(&pattern).map_err(|reason| {
            aelys_common::error::AelysError::from(aelys_common::error::CompileError::new(
                CompileErrorKind::InvalidPattern { reason },
                pattern.span,
                std::sync::Arc::clone(&self.source),
            ))
        })?;

        let unit_tag = self
            .source
            .name
            .bytes()
            .fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
            });
        let temp = format!("__aelys_let_{unit_tag:016x}_{}", self.destructure_count);
        self.destructure_count += 1;
        let temp_expr = Expr::new(ExprKind::Identifier(temp.clone()), pattern.span);
        let checker = Expr::new(
            ExprKind::Match {
                scrutinee: Box::new(temp_expr.clone()),
                arms: vec![MatchArm {
                    pattern: check,
                    guard: None,
                    body: MatchArmBody::Expr(Box::new(Expr::new(ExprKind::Unit, pattern.span))),
                    span: pattern.span,
                    synthetic: false,
                }],
            },
            pattern.span,
        );
        self.pending
            .push(Stmt::new(StmtKind::Expression(checker), pattern.span));
        bind_pattern(&pattern, temp_expr, mutable, &mut self.pending);

        Ok(Stmt::new(
            StmtKind::Let {
                name: temp,
                mutable: false,
                type_annotation,
                initializer,
                is_pub: false,
            },
            span,
        ))
    }
}

fn irrefutable_check_pattern(pattern: &Pattern) -> std::result::Result<Pattern, String> {
    let kind = match &pattern.kind {
        PatternKind::Wildcard | PatternKind::Binding(_) => PatternKind::Wildcard,
        PatternKind::Struct {
            path,
            type_args,
            fields,
            ..
        } if path.len() == 1 => {
            let mut kept = Vec::new();
            for field in fields {
                let checked = irrefutable_check_pattern(&field.pattern)?;
                if !matches!(checked.kind, PatternKind::Wildcard) {
                    kept.push(StructPatternField {
                        name: field.name.clone(),
                        pattern: checked,
                        span: field.span,
                    });
                }
            }
            PatternKind::Struct {
                path: path.clone(),
                type_args: type_args.clone(),
                fields: kept,
                has_rest: true,
            }
        }
        _ => {
            return Err(
                "refutable pattern in `let`: only names, `_`, tuples and struct patterns can be destructured; use `match` or `if let` for a pattern that can fail"
                    .to_string(),
            );
        }
    };
    Ok(Pattern {
        kind,
        span: pattern.span,
    })
}

fn bind_pattern(pattern: &Pattern, source: Expr, mutable: bool, out: &mut Vec<Stmt>) {
    match &pattern.kind {
        PatternKind::Binding(name) => out.push(Stmt::new(
            StmtKind::Let {
                name: name.clone(),
                mutable,
                type_annotation: None,
                initializer: source,
                is_pub: false,
            },
            pattern.span,
        )),
        PatternKind::Struct { fields, .. } => {
            for field in fields {
                let access = Expr::new(
                    ExprKind::Member {
                        object: Box::new(source.clone()),
                        member: field.name.clone(),
                        separator: MemberSeparator::Dot,
                    },
                    field.span,
                );
                bind_pattern(&field.pattern, access, mutable, out);
            }
        }
        _ => {}
    }
}
