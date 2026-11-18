use super::{Lexer, Result};
use aelys_common::error::{AelysError, CompileErrorKind};
use aelys_syntax::{FmtPart, TokenKind};

impl Lexer {
    pub(super) fn string(&mut self) -> Result<()> {
        let mut parts: Vec<FmtPart> = Vec::new();
        let mut current_literal = String::new();
        let mut has_format = false;

        while self.peek() != '"' && !self.is_at_end() {
            if self.peek() == '\n' {
                self.line += 1;
                self.column = 1;
            }

            match self.peek() {
                '\\' => {
                    self.advance();
                    let escaped = match self.peek() {
                        'n' => '\n',
                        't' => '\t',
                        'r' => '\r',
                        '\\' => '\\',
                        '"' => '"',
                        '0' => '\0',
                        '\'' => '\'',
                        'u' => {
                            let scalar = self.unicode_scalar()?;
                            current_literal.push(scalar);
                            continue;
                        }
                        c => {
                            return Err(AelysError::Compile(
                                self.error(CompileErrorKind::InvalidEscape(c)),
                            ));
                        }
                    };
                    self.advance();
                    current_literal.push(escaped);
                }
                '{' => {
                    self.advance();
                    if self.peek() == '{' {
                        // {{ -> literal {
                        self.advance();
                        current_literal.push('{');
                    } else {
                        // start of format expression or placeholder
                        has_format = true;
                        if !current_literal.is_empty() {
                            parts.push(FmtPart::Literal(std::mem::take(&mut current_literal)));
                        }

                        if self.peek() == '}' {
                            // {} -> placeholder
                            self.advance();
                            parts.push(FmtPart::Placeholder);
                        } else {
                            // {expr} -> expression
                            let expr = self.scan_format_expr()?;
                            parts.push(FmtPart::Expr(expr));
                        }
                    }
                }
                '}' => {
                    self.advance();
                    if self.peek() == '}' {
                        // }} -> literal }
                        self.advance();
                        current_literal.push('}');
                    } else {
                        return Err(AelysError::Compile(
                            self.error(CompileErrorKind::UnmatchedCloseBrace),
                        ));
                    }
                }
                _ => {
                    current_literal.push(self.advance());
                }
            }
        }

        if self.is_at_end() {
            return Err(AelysError::Compile(
                self.error(CompileErrorKind::UnterminatedString),
            ));
        }

        self.advance(); // closing "

        if has_format {
            if !current_literal.is_empty() {
                parts.push(FmtPart::Literal(current_literal));
            }
            self.add_token(TokenKind::FmtString(parts));
        } else {
            self.add_token(TokenKind::String(current_literal));
        }

        Ok(())
    }

    pub(super) fn char_literal(&mut self) -> Result<()> {
        let invalid = |lexer: &Self, reason: &str| {
            AelysError::Compile(lexer.error(CompileErrorKind::InvalidCharLiteral {
                reason: reason.to_string(),
            }))
        };
        if self.is_at_end() || self.peek() == '\n' {
            return Err(invalid(self, "missing closing quote"));
        }
        if (self.peek().is_alphabetic() || self.peek() == '_') && self.peek_next() != '\'' {
            return self.label();
        }
        let value = match self.peek() {
            '\'' => return Err(invalid(self, "a char literal holds exactly one character")),
            '\\' => {
                self.advance();
                match self.peek() {
                    'n' => self.escaped('\n'),
                    't' => self.escaped('\t'),
                    'r' => self.escaped('\r'),
                    '\\' => self.escaped('\\'),
                    '"' => self.escaped('"'),
                    '0' => self.escaped('\0'),
                    '\'' => self.escaped('\''),
                    'u' => self.unicode_scalar()?,
                    c => {
                        return Err(AelysError::Compile(
                            self.error(CompileErrorKind::InvalidEscape(c)),
                        ));
                    }
                }
            }
            _ => self.advance(),
        };
        if self.peek() != '\'' {
            return Err(invalid(
                self,
                if self.is_at_end() || self.peek() == '\n' {
                    "missing closing quote"
                } else {
                    "a char literal holds exactly one character; use \"...\" for a string"
                },
            ));
        }
        self.advance();
        self.add_token(TokenKind::Char(value));
        Ok(())
    }

    // a quote, one character and a quote is a char; a quote and a longer name with no closing quote is a label
    fn label(&mut self) -> Result<()> {
        while self.peek().is_alphanumeric() || self.peek() == '_' {
            self.advance();
        }
        if self.peek() == '\'' {
            return Err(AelysError::Compile(
                self.error(CompileErrorKind::InvalidCharLiteral {
                    reason: "a char literal holds exactly one character; use \"...\" for a string"
                        .to_string(),
                }),
            ));
        }
        let name: String = self.chars[self.start + 1..self.current].iter().collect();
        self.add_token(TokenKind::Label(name));
        Ok(())
    }

    fn escaped(&mut self, value: char) -> char {
        self.advance();
        value
    }

    fn unicode_scalar(&mut self) -> Result<char> {
        self.advance();
        let invalid =
            |lexer: &Self| {
                AelysError::Compile(lexer.error(CompileErrorKind::InvalidCharLiteral {
                reason: "a unicode escape is \\u{...} with 1 to 6 hex digits naming a scalar value"
                    .to_string(),
            }))
            };
        if self.peek() != '{' {
            return Err(invalid(self));
        }
        self.advance();
        let mut digits = String::new();
        while self.peek().is_ascii_hexdigit() && digits.len() < 6 {
            digits.push(self.advance());
        }
        if self.peek() != '}' || digits.is_empty() {
            return Err(invalid(self));
        }
        self.advance();
        u32::from_str_radix(&digits, 16)
            .ok()
            .and_then(char::from_u32)
            .ok_or_else(|| invalid(self))
    }

    fn scan_format_expr(&mut self) -> Result<String> {
        let mut expr = String::new();
        let mut brace_depth = 1;

        while !self.is_at_end() {
            let c = self.peek();
            match c {
                '{' => {
                    brace_depth += 1;
                    expr.push(self.advance());
                }
                '}' => {
                    brace_depth -= 1;
                    if brace_depth == 0 {
                        self.advance();
                        return Ok(expr);
                    }
                    expr.push(self.advance());
                }
                '"' => {
                    expr.push(self.advance());
                    while !self.is_at_end() && self.peek() != '"' {
                        if self.peek() == '\\' {
                            expr.push(self.advance());
                            if !self.is_at_end() {
                                expr.push(self.advance());
                            }
                        } else {
                            if self.peek() == '\n' {
                                self.line += 1;
                                self.column = 1;
                            }
                            expr.push(self.advance());
                        }
                    }
                    if self.is_at_end() {
                        return Err(AelysError::Compile(
                            self.error(CompileErrorKind::UnterminatedFmtExpr),
                        ));
                    }
                    expr.push(self.advance());
                }
                '\n' => {
                    self.line += 1;
                    self.column = 1;
                    expr.push(self.advance());
                }
                _ => {
                    expr.push(self.advance());
                }
            }
        }

        Err(AelysError::Compile(
            self.error(CompileErrorKind::UnterminatedFmtExpr),
        ))
    }
}
