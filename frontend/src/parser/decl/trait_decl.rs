use super::Parser;
use aelys_common::Result;
use aelys_common::error::CompileErrorKind;
use aelys_syntax::{Function, Stmt, StmtKind, TokenKind, TraitMethod, TypeAnnotation, WhereClause};

impl Parser {
    pub(super) fn trait_declaration(&mut self, is_pub: bool) -> Result<Stmt> {
        let start_span = self.peek().span;
        self.advance();

        let name = self.consume_identifier("trait name")?;
        if name.chars().next().is_none_or(|c| !c.is_uppercase()) {
            return Err(self.error(CompileErrorKind::UnexpectedToken {
                expected: "capitalized trait name".to_string(),
                found: name,
            }));
        }

        let (type_params, mut where_clauses) = self.parse_type_params_with_bounds()?;
        let super_bounds = if self.match_token(&TokenKind::Colon) {
            self.parse_bound_list()?
        } else {
            Vec::new()
        };
        where_clauses.extend(self.parse_where_clauses()?);

        self.consume(&TokenKind::LBrace, "{")?;
        let mut methods = Vec::new();
        let mut associated_types = Vec::new();
        let mut associated_consts = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            if self.match_token(&TokenKind::Semicolon) {
                continue;
            }
            if self.check(&TokenKind::Fn) {
                methods.push(self.trait_method_declaration()?);
                continue;
            }
            if let TokenKind::Identifier(word) = &self.peek().kind
                && matches!(self.peek_at(1).kind, TokenKind::Identifier(_))
            {
                let item_span = self.peek().span;
                match word.as_str() {
                    "type" => {
                        self.advance();
                        let name = self.consume_identifier("associated type name")?;
                        associated_types.push(aelys_syntax::AssociatedTypeDecl {
                            name,
                            span: item_span.merge(self.previous().span),
                        });
                        self.match_token(&TokenKind::Semicolon);
                        continue;
                    }
                    "const" => {
                        self.advance();
                        let name = self.consume_identifier("associated constant name")?;
                        self.consume(&TokenKind::Colon, ":")?;
                        let type_annotation = self.parse_type_annotation()?;
                        associated_consts.push(aelys_syntax::AssociatedConstDecl {
                            name,
                            type_annotation,
                            span: item_span.merge(self.previous().span),
                        });
                        self.match_token(&TokenKind::Semicolon);
                        continue;
                    }
                    _ => {}
                }
            }
            self.reject_deferred_body_item()?;
            return Err(self.error(CompileErrorKind::UnexpectedToken {
                expected: "fn, type, or const in trait declaration".to_string(),
                found: self.peek().kind.to_string(),
            }));
        }
        self.consume(&TokenKind::RBrace, "}")?;

        Ok(Stmt::new(
            StmtKind::TraitDecl {
                name,
                type_params,
                super_bounds,
                where_clauses,
                methods,
                associated_types,
                associated_consts,
                is_pub,
            },
            start_span.merge(self.previous().span),
        ))
    }

    pub(super) fn parse_type_params_with_bounds(
        &mut self,
    ) -> Result<(Vec<String>, Vec<WhereClause>)> {
        if !self.match_token(&TokenKind::Lt) {
            return Ok((Vec::new(), Vec::new()));
        }

        let mut params = Vec::new();
        let mut where_clauses = Vec::new();
        loop {
            let span = self.peek().span;
            let name = self.consume_identifier("type parameter")?;
            params.push(name.clone());
            if self.match_token(&TokenKind::Colon) {
                let bounds = self.parse_bound_list()?;
                let end = bounds.last().map_or(span, |bound| bound.span);
                where_clauses.push(WhereClause {
                    type_annotation: TypeAnnotation::new(name, span),
                    bounds,
                    span: span.merge(end),
                });
            }
            if !self.match_token(&TokenKind::Comma) {
                break;
            }
        }
        self.consume_generic_close()?;
        Ok((params, where_clauses))
    }

    pub(super) fn parse_bound_list(&mut self) -> Result<Vec<TypeAnnotation>> {
        let mut bounds = vec![self.parse_type_annotation()?];
        while self.match_token(&TokenKind::Plus) {
            bounds.push(self.parse_type_annotation()?);
        }
        Ok(bounds)
    }

    pub(super) fn parse_where_clauses(&mut self) -> Result<Vec<WhereClause>> {
        if !self.match_word("where") {
            return Ok(Vec::new());
        }

        let mut clauses = Vec::new();
        loop {
            let clause_start = self.peek().span;
            let type_annotation = self.parse_type_annotation()?;
            self.consume(&TokenKind::Colon, ":")?;
            let bounds = self.parse_bound_list()?;
            let clause_span = clause_start.merge(
                bounds
                    .last()
                    .map_or(type_annotation.span, |bound| bound.span),
            );
            clauses.push(WhereClause {
                type_annotation,
                bounds,
                span: clause_span,
            });
            if !self.match_token(&TokenKind::Comma) {
                break;
            }
        }
        Ok(clauses)
    }

    pub(super) fn reject_deferred_body_item(&self) -> Result<()> {
        let TokenKind::Identifier(word) = &self.peek().kind else {
            return Ok(());
        };
        match word.as_str() {
            "default" if matches!(self.peek_at(1).kind, TokenKind::Fn) => {
                Err(self.error(CompileErrorKind::SpecializationDeferred))
            }
            _ => Ok(()),
        }
    }

    fn match_word(&mut self, word: &str) -> bool {
        if matches!(&self.peek().kind, TokenKind::Identifier(name) if name == word) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn trait_method_declaration(&mut self) -> Result<TraitMethod> {
        let start_span = self.peek().span;
        self.advance();

        let name = self.consume_identifier("trait method name")?;
        let (type_params, mut where_clauses) = self.parse_type_params_with_bounds()?;
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
        where_clauses.extend(self.parse_where_clauses()?);

        let (body, has_body, end_span) = if self.match_token(&TokenKind::LBrace) {
            let body = self.block_statements()?;
            (body, true, self.previous().span)
        } else {
            self.consume(&TokenKind::Semicolon, ";")?;
            (Vec::new(), false, self.previous().span)
        };

        Ok(TraitMethod {
            function: Function {
                name,
                type_params,
                where_clauses,
                params,
                return_type,
                body,
                decorators: Vec::new(),
                is_pub: false,
                span: start_span.merge(end_span),
            },
            has_body,
        })
    }
}
