use super::Parser;
use aelys_common::Result;
use aelys_common::error::CompileErrorKind;
use aelys_syntax::{
    EnumVariantDecl, EnumVariantFields, Stmt, StmtKind, StructFieldDecl, TokenKind,
};

impl Parser {
    pub(super) fn enum_declaration(&mut self, is_pub: bool) -> Result<Stmt> {
        let start_span = self.peek().span;
        self.advance();
        let name = self.consume_identifier("enum name")?;
        if name.chars().next().is_none_or(|c| !c.is_uppercase()) {
            return Err(self.error(CompileErrorKind::UnexpectedToken {
                expected: "capitalized enum name".to_string(),
                found: name,
            }));
        }

        let type_params = if self.match_token(&TokenKind::Lt) {
            let mut params = Vec::new();
            loop {
                params.push(self.consume_identifier("type parameter")?);
                if !self.match_token(&TokenKind::Comma) {
                    break;
                }
            }
            self.consume(&TokenKind::Gt, ">")?;
            params
        } else {
            Vec::new()
        };

        self.consume(&TokenKind::LBrace, "{")?;
        let mut variants = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            if self.match_token(&TokenKind::Comma) || self.match_token(&TokenKind::Semicolon) {
                continue;
            }
            let variant_span = self.peek().span;
            let variant_name = self.consume_identifier("enum variant name")?;
            if variants
                .iter()
                .any(|variant: &EnumVariantDecl| variant.name == variant_name)
            {
                return Err(self.error(CompileErrorKind::InvalidPattern {
                    reason: format!("duplicate enum variant '{variant_name}'"),
                }));
            }

            let fields = if self.match_token(&TokenKind::LParen) {
                let mut fields = Vec::new();
                if !self.check(&TokenKind::RParen) {
                    loop {
                        fields.push(self.parse_type_annotation()?);
                        if !self.match_token(&TokenKind::Comma) {
                            break;
                        }
                        if self.check(&TokenKind::RParen) {
                            break;
                        }
                    }
                }
                self.consume(&TokenKind::RParen, ")")?;
                EnumVariantFields::Tuple(fields)
            } else if self.match_token(&TokenKind::LBrace) {
                let mut fields = Vec::new();
                while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
                    if self.match_token(&TokenKind::Comma)
                        || self.match_token(&TokenKind::Semicolon)
                    {
                        continue;
                    }
                    let field_span = self.peek().span;
                    let field_name = self.consume_identifier("enum field name")?;
                    if fields
                        .iter()
                        .any(|field: &StructFieldDecl| field.name == field_name)
                    {
                        return Err(self.error(CompileErrorKind::InvalidPattern {
                            reason: format!("duplicate enum field '{field_name}'"),
                        }));
                    }
                    self.consume(&TokenKind::Colon, ":")?;
                    let type_annotation = self.parse_type_annotation()?;
                    let end_span = self.previous().span;
                    fields.push(StructFieldDecl {
                        name: field_name,
                        type_annotation,
                        is_pub: false,
                        span: field_span.merge(end_span),
                    });
                    if !self.match_token(&TokenKind::Comma)
                        && !self.match_token(&TokenKind::Semicolon)
                        && !self.check(&TokenKind::RBrace)
                    {
                        return Err(self.error(CompileErrorKind::UnexpectedToken {
                            expected: "comma, semicolon, or '}'".to_string(),
                            found: self.peek().kind.to_string(),
                        }));
                    }
                }
                self.consume(&TokenKind::RBrace, "}")?;
                EnumVariantFields::Named(fields)
            } else {
                EnumVariantFields::Unit
            };

            let end_span = self.previous().span;
            variants.push(EnumVariantDecl {
                name: variant_name,
                fields,
                span: variant_span.merge(end_span),
            });

            if !self.check(&TokenKind::RBrace)
                && !self.check(&TokenKind::Comma)
                && !self.check(&TokenKind::Semicolon)
            {
                return Err(self.error(CompileErrorKind::UnexpectedToken {
                    expected: "comma, semicolon, or '}'".to_string(),
                    found: self.peek().kind.to_string(),
                }));
            }
        }
        self.consume(&TokenKind::RBrace, "}")?;
        let end_span = self.previous().span;
        Ok(Stmt::new(
            StmtKind::EnumDecl {
                name,
                type_params,
                variants,
                is_pub,
            },
            start_span.merge(end_span),
        ))
    }
}
