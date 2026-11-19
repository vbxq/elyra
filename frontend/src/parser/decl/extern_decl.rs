use super::Parser;
use aelys_common::Result;
use aelys_common::error::CompileErrorKind;
use aelys_syntax::{
    ExternBlock, ExternFunction, ExternParam, ReferenceKind, Stmt, StmtKind, TokenKind,
    TypeAnnotation,
};

impl Parser {
    pub(crate) fn starts_extern_block(&self) -> bool {
        matches!(&self.peek().kind, TokenKind::Identifier(word) if word == "extern")
            && matches!(self.peek_at(1).kind, TokenKind::String(_))
    }

    pub(super) fn extern_block(&mut self, is_pub: bool) -> Result<Stmt> {
        let start = self.advance().span;
        let library = match self.advance().kind.clone() {
            TokenKind::String(library) => library,
            other => {
                return Err(self.error(CompileErrorKind::UnexpectedToken {
                    expected: "library name string after extern".to_string(),
                    found: other.to_string(),
                }));
            }
        };
        self.consume(&TokenKind::LBrace, "{")?;
        let mut functions = Vec::new();
        loop {
            while self.match_token(&TokenKind::Semicolon) {}
            if self.match_token(&TokenKind::RBrace) {
                break;
            }
            let item_pub = self.match_token(&TokenKind::Pub);
            if !self.check(&TokenKind::Fn) {
                return Err(self.error(CompileErrorKind::UnexpectedToken {
                    expected: "fn declaration or '}' in an extern block".to_string(),
                    found: self.peek().kind.to_string(),
                }));
            }
            functions.push(self.extern_function(is_pub || item_pub)?);
        }
        let span = start.merge(self.previous().span);
        Ok(Stmt::new(
            StmtKind::Extern(ExternBlock {
                library,
                functions,
                is_pub,
            }),
            span,
        ))
    }

    fn extern_function(&mut self, is_pub: bool) -> Result<ExternFunction> {
        let start = self.advance().span;
        let name = self.consume_declared_identifier("C function name")?;
        self.consume(&TokenKind::LParen, "(")?;
        let mut params = Vec::new();
        let mut variadic = false;
        if !self.check(&TokenKind::RParen) {
            loop {
                if self.check(&TokenKind::DotDot) && self.peek_at(1).kind == TokenKind::Dot {
                    self.advance();
                    self.advance();
                    variadic = true;
                    if !self.check(&TokenKind::RParen) {
                        return Err(self.error(CompileErrorKind::UnexpectedToken {
                            expected: "')' after '...', the last parameter".to_string(),
                            found: self.peek().kind.to_string(),
                        }));
                    }
                    break;
                }
                let param_start = self.peek().span;
                let param_name = self.consume_identifier("parameter name")?;
                self.consume(&TokenKind::Colon, ":")?;
                let type_annotation = self.extern_param_type()?;
                params.push(ExternParam {
                    name: param_name,
                    span: param_start.merge(type_annotation.span),
                    type_annotation,
                });
                if !self.match_token(&TokenKind::Comma) {
                    break;
                }
            }
        }
        self.consume(&TokenKind::RParen, ")")?;
        let return_type = if self.match_token(&TokenKind::Arrow) {
            Some(self.extern_param_type()?)
        } else {
            None
        };
        Ok(ExternFunction {
            name,
            params,
            variadic,
            return_type,
            is_pub,
            span: start.merge(self.previous().span),
        })
    }

    fn extern_param_type(&mut self) -> Result<TypeAnnotation> {
        let start = self.peek().span;
        let opens_slice = self.check(&TokenKind::Ampersand)
            && (self.peek_at(1).kind == TokenKind::LBracket
                || (self.peek_at(1).kind == TokenKind::Mut
                    && self.peek_at(2).kind == TokenKind::LBracket));
        if !opens_slice {
            return self.parse_type_annotation();
        }
        self.advance();
        let reference = if self.match_token(&TokenKind::Mut) {
            ReferenceKind::Mutable
        } else {
            ReferenceKind::Shared
        };
        self.consume(&TokenKind::LBracket, "[")?;
        let element = self.parse_type_annotation()?;
        self.consume(&TokenKind::RBracket, "]")?;
        let span = start.merge(self.previous().span);
        Ok(
            TypeAnnotation::with_params("slice".to_string(), vec![element], span)
                .with_reference(reference, span),
        )
    }
}
