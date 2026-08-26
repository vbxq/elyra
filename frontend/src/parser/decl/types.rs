use super::Parser;
use aelys_common::Result;
use aelys_common::error::CompileErrorKind;
use aelys_syntax::{Parameter, ReferenceKind, TokenKind, TypeAnnotation};

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

        if self.match_token(&TokenKind::Ampersand) {
            let reference = if self.match_token(&TokenKind::Mut) {
                ReferenceKind::Mutable
            } else {
                ReferenceKind::Shared
            };
            let inner = self.parse_type_annotation()?;
            if inner.reference.is_some() {
                return Err(self.error(CompileErrorKind::UnexpectedToken {
                    expected: "a concrete type after '&'".to_string(),
                    found: "reference type".to_string(),
                }));
            }
            let span = start_span.merge(inner.span);
            return Ok(inner.with_reference(reference, span));
        }

        if self.match_token(&TokenKind::Fn) {
            return self.parse_function_type_annotation(start_span);
        }

        if self.match_token(&TokenKind::LBracket) {
            let element = self.parse_type_annotation()?;
            self.consume(&TokenKind::Semicolon, ";")?;
            let length_token = self.advance().clone();
            let length = match length_token.kind {
                TokenKind::Int(value) if value >= 0 => value as u64,
                TokenKind::Identifier(name) => {
                    // `[t; bounds::limit]` symbolic length from an associated
                    let mut path = vec![name];
                    while self.match_token(&TokenKind::ColonColon) {
                        path.push(self.consume_identifier("associated constant path segment")?);
                    }
                    self.consume(&TokenKind::RBracket, "]")?;
                    let end_span = self.previous().span;
                    return Ok(TypeAnnotation::fixed_array_symbolic(
                        element,
                        path,
                        start_span.merge(end_span),
                    ));
                }
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
            if matches!(self.peek().kind, TokenKind::Identifier(_))
                && matches!(self.peek_at(1).kind, TokenKind::Eq)
            {
                let mut bindings = Vec::new();
                loop {
                    let binding_name = self.consume_identifier("associated binding name")?;
                    self.consume(&TokenKind::Eq, "=")?;
                    let binding_ty = self.parse_type_annotation()?;
                    bindings.push((binding_name, binding_ty));
                    if !self.match_token(&TokenKind::Comma) {
                        break;
                    }
                }
                self.consume_generic_close()?;
                let end_span = self.previous().span;
                let mut annotation =
                    TypeAnnotation::new(path.last().cloned().unwrap_or(first_name), start_span);
                annotation.path = path;
                annotation.associated_bindings = bindings;
                annotation.span = start_span.merge(end_span);
                return Ok(annotation);
            }
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

    pub fn parse_parameter(&mut self) -> Result<Parameter> {
        let span = self.peek().span;
        let receiver_reference = if self.match_token(&TokenKind::Ampersand) {
            let reference = if self.match_token(&TokenKind::Mut) {
                ReferenceKind::Mutable
            } else {
                ReferenceKind::Shared
            };
            let name = self.consume_identifier("parameter name")?;
            if name != "self" {
                return Err(self.error(CompileErrorKind::UnexpectedToken {
                    expected: "'self' after a receiver borrow".to_string(),
                    found: name,
                }));
            }
            Some(reference)
        } else {
            None
        };
        let mutable = receiver_reference
            .is_some_and(|reference| reference == ReferenceKind::Mutable)
            || self.match_token(&TokenKind::Mut);
        let name = if receiver_reference.is_some() {
            "self".to_string()
        } else {
            self.consume_identifier("parameter name")?
        };

        let type_annotation = if self.match_token(&TokenKind::Colon) {
            Some(self.parse_type_annotation()?)
        } else {
            None
        };

        let end_span = self.previous().span;
        let mut parameter = Parameter::new(name, mutable, type_annotation, span.merge(end_span));
        parameter.reference = receiver_reference.or_else(|| {
            parameter
                .type_annotation
                .as_ref()
                .and_then(|annotation| annotation.reference)
        });
        Ok(parameter)
    }
}
