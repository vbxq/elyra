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
        let mut associated_types = Vec::new();
        let mut associated_consts = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            if self.match_token(&TokenKind::Semicolon) {
                continue;
            }
            if self.check(&TokenKind::Fn) {
                let method = self.function_declaration(Vec::new(), false)?;
                let StmtKind::Function(method) = method.kind else {
                    unreachable!("impl parser only accepts functions")
                };
                methods.push(method);
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
                        self.consume(&TokenKind::Eq, "=")?;
                        let value = self.parse_type_annotation()?;
                        associated_types.push(aelys_syntax::AssociatedTypeDef {
                            name,
                            value,
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
                        self.consume(&TokenKind::Eq, "=")?;
                        let value = self.expression()?;
                        associated_consts.push(aelys_syntax::AssociatedConstDef {
                            name,
                            type_annotation,
                            value,
                            span: item_span.merge(self.previous().span),
                        });
                        self.match_token(&TokenKind::Semicolon);
                        continue;
                    }
                    _ => {}
                }
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
                associated_types,
                associated_consts,
            },
            start_span.merge(self.previous().span),
        ))
    }
}
