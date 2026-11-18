use super::Lexer;
use aelys_common::Result;
use aelys_common::error::{AelysError, CompileErrorKind};
use aelys_syntax::TokenKind;

impl Lexer {
    pub(super) fn identifier(&mut self) -> Result<()> {
        while self.peek().is_alphanumeric() || self.peek() == '_' {
            self.advance();
        }

        let text: String = self.chars[self.start..self.current].iter().collect();

        let kind = match text.as_str() {
            "let" => TokenKind::Let,
            "mut" => TokenKind::Mut,
            "fn" => TokenKind::Fn,
            "if" => TokenKind::If,
            "else" => TokenKind::Else,
            "while" => TokenKind::While,
            "return" => TokenKind::Return,
            "break" => TokenKind::Break,
            "continue" => TokenKind::Continue,
            "and" => TokenKind::And,
            "or" => TokenKind::Or,
            "not" => TokenKind::Not,
            "true" => TokenKind::True,
            "false" => TokenKind::False,
            "null" => TokenKind::Null,
            "pub" => TokenKind::Pub,
            "needs" => TokenKind::Needs,
            "as" => TokenKind::As,
            "from" => TokenKind::From,
            "for" => TokenKind::For,
            "in" => TokenKind::In,
            "struct" => TokenKind::Struct,
            "enum" => TokenKind::Enum,
            "trait" => TokenKind::Trait,
            "impl" => TokenKind::Impl,
            "match" => TokenKind::Match,
            _ if text.starts_with(aelys_syntax::TUPLE_STRUCT_PREFIX) => {
                return Err(AelysError::Compile(
                    self.error(CompileErrorKind::ReservedName { name: text }),
                ));
            }
            _ => TokenKind::Identifier(text),
        };

        self.add_token(kind);
        Ok(())
    }
}
