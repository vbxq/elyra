use super::Parser;
use aelys_common::Result;
use aelys_syntax::{Decorator, Function, Stmt, StmtKind, TokenKind};

impl Parser {
    pub(super) fn function_declaration(
        &mut self,
        decorators: Vec<Decorator>,
        is_pub: bool,
    ) -> Result<Stmt> {
        self.function_declaration_with_default(decorators, is_pub, false)
    }

    pub(super) fn function_declaration_with_default(
        &mut self,
        decorators: Vec<Decorator>,
        is_pub: bool,
        is_default: bool,
    ) -> Result<Stmt> {
        let start_span = self.peek().span;
        self.advance();

        let name = if self.match_token(&TokenKind::From) {
            "from".to_string()
        } else {
            self.consume_identifier("function name")?
        };

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

        self.consume(&TokenKind::LBrace, "{")?;

        let body = self.block_statements()?;
        let end_span = self.previous().span;

        let function = Function {
            name: name.clone(),
            is_default,
            type_params,
            where_clauses,
            params,
            return_type,
            body,
            decorators,
            is_pub,
            span: start_span.merge(end_span),
        };

        Ok(Stmt::new(
            StmtKind::Function(function),
            start_span.merge(end_span),
        ))
    }
}
