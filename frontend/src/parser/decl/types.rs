use super::Parser;
use aelys_common::Result;
use aelys_common::error::CompileErrorKind;
use aelys_syntax::{Parameter, TokenKind, TypeAnnotation};

// can never build a type the binary format cannot carry
const MAX_TYPE_NESTING_DEPTH: usize = 64;

impl Parser {
    pub fn parse_type_annotation(&mut self) -> Result<TypeAnnotation> {
        self.type_depth += 1;
        if self.type_depth > MAX_TYPE_NESTING_DEPTH {
            self.type_depth -= 1;
            return Err(self.error(CompileErrorKind::TypeNestingTooDeep {
                max: MAX_TYPE_NESTING_DEPTH,
            }));
        }
        let parsed = self.parse_type_annotation_inner();
        self.type_depth -= 1;
        parsed
    }

    fn parse_type_annotation_inner(&mut self) -> Result<TypeAnnotation> {
        let start_span = self.peek().span;

        if self.match_token(&TokenKind::Fn) {
            return self.parse_function_type_annotation(start_span);
        }

        if self.match_token(&TokenKind::LBracket) {
            let element = self.parse_type_annotation()?;
            self.consume(&TokenKind::Semicolon, ";")?;
            let length_token = self.advance().clone();
            let length = match length_token.kind {
                TokenKind::Int(value) if value >= 0 => value as u64,
                _ => {
                    return Err(self.error(CompileErrorKind::UnexpectedToken {
                        expected: "non-negative array length".to_string(),
                        found: length_token.kind.to_string(),
                    }));
                }
            };
            self.consume(&TokenKind::RBracket, "]")?;
            let end_span = self.previous().span;
            return Ok(TypeAnnotation::fixed_array(
                element,
                length,
                start_span.merge(end_span),
            ));
        }

        if self.match_token(&TokenKind::Null) {
            return Err(self.error(CompileErrorKind::NullIsNotInSurface));
        }

        self.reject_trait_object()?;

        let first_name = self.consume_identifier("type name")?;
        let mut path = vec![first_name.clone()];
        while self.match_token(&TokenKind::ColonColon) {
            path.push(self.consume_identifier("type path segment")?);
        }

        if !self.legacy_collections && first_name.eq_ignore_ascii_case("array") {
            return Err(self.error(CompileErrorKind::UnexpectedToken {
                expected: "Rust-style collection type syntax".to_string(),
                found: "legacy collection syntax".to_string(),
            }));
        }

        if self.match_token(&TokenKind::Lt) {
            let mut type_params = vec![self.parse_type_annotation()?];
            while self.match_token(&TokenKind::Comma) {
                type_params.push(self.parse_type_annotation()?);
            }
            self.consume_generic_close()?;
            let end_span = self.previous().span;
            let mut annotation = TypeAnnotation::with_params(
                path.last().cloned().unwrap_or(first_name),
                type_params,
                start_span.merge(end_span),
            );
            annotation.path = path;
            Ok(annotation)
        } else {
            let mut annotation =
                TypeAnnotation::new(path.last().cloned().unwrap_or(first_name), start_span);
            annotation.path = path;
            Ok(annotation)
        }
    }

    pub(crate) fn consume_generic_close(&mut self) -> Result<()> {
        if self.match_token(&TokenKind::Gt) {
            return Ok(());
        }
        if self.check(&TokenKind::Shr) {
            let token = self.advance().clone();
            let first = aelys_syntax::Token::new(TokenKind::Gt, token.span);
            let second = aelys_syntax::Token::new(TokenKind::Gt, token.span);
            self.tokens
                .splice(self.current..self.current, [second, first]);
            self.advance();
            return Ok(());
        }
        Err(self.error(CompileErrorKind::UnexpectedToken {
            expected: ">".to_string(),
            found: self.peek().kind.to_string(),
        }))
    }

    fn parse_function_type_annotation(
        &mut self,
        start_span: aelys_syntax::Span,
    ) -> Result<TypeAnnotation> {
        self.consume(&TokenKind::LParen, "(")?;
        let mut params = Vec::new();
        if !self.check(&TokenKind::RParen) {
            params.push(self.parse_type_annotation()?);
            while self.match_token(&TokenKind::Comma) {
                params.push(self.parse_type_annotation()?);
            }
        }
        self.consume(&TokenKind::RParen, ")")?;
        self.consume(&TokenKind::Arrow, "->")?;
        let ret = self.parse_type_annotation()?;
        let end_span = self.previous().span;
        Ok(TypeAnnotation::function_type(
            params,
            ret,
            start_span.merge(end_span),
        ))
    }

    fn reject_trait_object(&self) -> Result<()> {
        let TokenKind::Identifier(word) = &self.peek().kind else {
            return Ok(());
        };
        if word != "dyn" {
            return Ok(());
        }
        let TokenKind::Identifier(trait_name) = &self.peek_at(1).kind else {
            return Ok(());
        };
        Err(self.error(CompileErrorKind::TraitObjectDeferred {
            trait_name: trait_name.clone(),
        }))
    }

    fn reject_borrowing_receiver(&self) -> Result<()> {
        if !self.check(&TokenKind::Ampersand) {
            return Ok(());
        }
        let borrows_mutably = matches!(self.peek_at(1).kind, TokenKind::Mut);
        let name_offset = if borrows_mutably { 2 } else { 1 };
        let receives_self = matches!(
            &self.peek_at(name_offset).kind,
            TokenKind::Identifier(name) if name == "self"
        );
        if !receives_self {
            return Ok(());
        }
        let form = if borrows_mutably {
            "&mut self"
        } else {
            "&self"
        };
        Err(self.error(CompileErrorKind::BorrowingReceiverDeferred {
            form: form.to_string(),
        }))
    }

    pub fn parse_parameter(&mut self) -> Result<Parameter> {
        let span = self.peek().span;
        self.reject_borrowing_receiver()?;
        let mutable = self.match_token(&TokenKind::Mut);
        let name = self.consume_identifier("parameter name")?;

        let type_annotation = if self.match_token(&TokenKind::Colon) {
            Some(self.parse_type_annotation()?)
        } else {
            None
        };

        let end_span = self.previous().span;
        Ok(Parameter::new(
            name,
            mutable,
            type_annotation,
            span.merge(end_span),
        ))
    }
}
