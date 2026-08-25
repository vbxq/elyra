use super::Parser;
use aelys_common::Result;
use aelys_common::error::CompileErrorKind;
use aelys_syntax::{Stmt, StmtKind, TokenKind};

impl Parser {
    pub(super) fn impl_declaration(&mut self) -> Result<Stmt> {
        let start_span = self.peek().span;
        self.advance();

        let (type_params, mut where_clauses) = self.parse_type_params_with_bounds()?;
        if self.check(&TokenKind::Bang) {
            return Err(self.error(CompileErrorKind::NegativeImplDeferred));
        }
        let first_type = self.parse_type_annotation()?;
        if first_type
            .path
            .last()
            .and_then(|name| name.chars().next())
            .is_none_or(|c| !c.is_uppercase())
        {
            return Err(self.error(CompileErrorKind::UnexpectedToken {
                expected: "capitalized trait or type name".to_string(),
                found: first_type.name,
            }));
        }

        let (trait_path, self_type) = if self.match_token(&TokenKind::For) {
            let self_type = self.parse_type_annotation()?;
            if self_type
                .path
                .last()
                .and_then(|name| name.chars().next())
                .is_none_or(|c| !c.is_uppercase())
            {
                return Err(self.error(CompileErrorKind::UnexpectedToken {
                    expected: "capitalized type name".to_string(),
                    found: self_type.name,
                }));
            }
            (Some(first_type), self_type)
        } else {
            (None, first_type)
        };
        where_clauses.extend(self.parse_where_clauses()?);

        self.consume(&TokenKind::LBrace, "{")?;
        let mut methods = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            if self.match_token(&TokenKind::Semicolon) {
                continue;
            }
            self.reject_deferred_body_item()?;
            let method = self.function_declaration(Vec::new(), false)?;
            let StmtKind::Function(method) = method.kind else {
                unreachable!("impl parser only accepts functions")
            };
            methods.push(method);
        }
        self.consume(&TokenKind::RBrace, "}")?;
        Ok(Stmt::new(
            StmtKind::ImplDecl {
                type_params,
                trait_path,
                self_type,
                where_clauses,
                methods,
            },
            start_span.merge(self.previous().span),
        ))
    }
}
