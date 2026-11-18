// Octal literal support contributed by Keggek (ported to rust)
// https://codeberg.org/gek

use super::{Lexer, Result};
use aelys_common::error::{AelysError, CompileErrorKind};
use aelys_syntax::TokenKind;

impl Lexer {
    pub(super) fn number(&mut self) -> Result<()> {
        if self.chars.get(self.start) == Some(&'0') {
            if self.match_char('x') || self.match_char('X') {
                return self.hex_number();
            }
            if self.match_char('b') || self.match_char('B') {
                return self.binary_number();
            }
            if self.match_char('o') || self.match_char('O') {
                return self.octal_number();
            }
        }

        while self.peek().is_ascii_digit() || self.peek() == '_' {
            self.advance();
        }

        let mut is_float = false;
        let tuple_index = matches!(
            self.tokens.last().map(|token| &token.kind),
            Some(TokenKind::Dot)
        );

        if !tuple_index && self.peek() == '.' && self.peek_next().is_ascii_digit() {
            is_float = true;
            self.advance();
            while self.peek().is_ascii_digit() || self.peek() == '_' {
                self.advance();
            }
        }

        if !tuple_index && (self.peek() == 'e' || self.peek() == 'E') {
            is_float = true;
            self.advance();
            if self.peek() == '+' || self.peek() == '-' {
                self.advance();
            }
            while self.peek().is_ascii_digit() || self.peek() == '_' {
                self.advance();
            }
        }

        let text: String = self.chars[self.start..self.current]
            .iter()
            .filter(|&&c| c != '_')
            .collect();

        if is_float {
            match text.parse::<f64>() {
                Ok(n) => self.add_token(TokenKind::Float(n)),
                Err(_) => {
                    return Err(AelysError::Compile(
                        self.error(CompileErrorKind::InvalidNumber(text)),
                    ));
                }
            }
        } else {
            match text.parse::<u64>() {
                Ok(n) => self.add_integer(n),
                Err(_) => {
                    return Err(AelysError::Compile(
                        self.error(CompileErrorKind::InvalidNumber(text)),
                    ));
                }
            }
        }

        Ok(())
    }

    // a literal above i64::MAX only fits u64; the parser types it so
    fn add_integer(&mut self, value: u64) {
        match i64::try_from(value) {
            Ok(value) => self.add_token(TokenKind::Int(value)),
            Err(_) => self.add_token(TokenKind::UInt(value)),
        }
    }

    fn hex_number(&mut self) -> Result<()> {
        while self.peek().is_ascii_hexdigit() || self.peek() == '_' {
            self.advance();
        }

        let text: String = self.chars[self.start + 2..self.current]
            .iter()
            .filter(|&&c| c != '_')
            .collect();

        match u64::from_str_radix(&text, 16) {
            Ok(n) => self.add_integer(n),
            Err(_) => {
                let full: String = self.chars[self.start..self.current].iter().collect();
                return Err(AelysError::Compile(
                    self.error(CompileErrorKind::InvalidNumber(full)),
                ));
            }
        }

        Ok(())
    }

    fn binary_number(&mut self) -> Result<()> {
        while self.peek() == '0' || self.peek() == '1' || self.peek() == '_' {
            self.advance();
        }

        let text: String = self.chars[self.start + 2..self.current]
            .iter()
            .filter(|&&c| c != '_')
            .collect();

        match u64::from_str_radix(&text, 2) {
            Ok(n) => self.add_integer(n),
            Err(_) => {
                let full: String = self.chars[self.start..self.current].iter().collect();
                return Err(AelysError::Compile(
                    self.error(CompileErrorKind::InvalidNumber(full)),
                ));
            }
        }

        Ok(())
    }

    fn octal_number(&mut self) -> Result<()> {
        while matches!(self.peek(), '0'..='7' | '_') {
            self.advance();
        }

        let text: String = self.chars[self.start + 2..self.current]
            .iter()
            .filter(|&&c| c != '_')
            .collect();

        match u64::from_str_radix(&text, 8) {
            Ok(n) => self.add_integer(n),
            Err(_) => {
                let full: String = self.chars[self.start..self.current].iter().collect();
                return Err(AelysError::Compile(
                    self.error(CompileErrorKind::InvalidNumber(full)),
                ));
            }
        }

        Ok(())
    }
}
