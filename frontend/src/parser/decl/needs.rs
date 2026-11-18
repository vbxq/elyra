use super::Parser;
use aelys_common::Result;
use aelys_common::error::{CompileError, CompileErrorKind};
use aelys_syntax::{ImportKind, NeedsStmt, Stmt, StmtKind, TokenKind};
use std::sync::Arc;

impl Parser {
    pub(super) fn starts_removed_needs(&self) -> bool {
        matches!(&self.peek().kind, TokenKind::Identifier(word) if word == "needs")
            && matches!(self.peek_at(1).kind, TokenKind::Identifier(_))
    }

    pub(super) fn removed_needs(&mut self) -> Result<Stmt> {
        let span = self.peek().span;
        self.advance();
        let mut words = Vec::new();
        while !matches!(self.peek().kind, TokenKind::Semicolon | TokenKind::Eof) {
            words.push(self.advance().kind.to_string());
        }
        Err(CompileError::new(
            CompileErrorKind::RemovedNeedsImport {
                rewritten: rewrite_needs(&words),
            },
            span,
            Arc::clone(&self.source),
        )
        .into())
    }

    pub(super) fn use_declaration(&mut self) -> Result<Stmt> {
        let start_span = self.peek().span;
        self.advance();

        let mut path = vec![self.consume_identifier("module name")?];
        let mut kind = None;
        while self.match_token(&TokenKind::ColonColon) {
            if self.match_token(&TokenKind::Star) {
                kind = Some(ImportKind::Wildcard);
                break;
            }
            if self.match_token(&TokenKind::LBrace) {
                kind = Some(ImportKind::Symbols(self.use_symbol_list()?));
                break;
            }
            path.push(self.consume_identifier("module path segment")?);
        }

        if path.len() >= 2 && path[0] == "std" && path[1] == "convert" {
            return Err(CompileError::new(
                CompileErrorKind::RemovedConvertModule,
                start_span,
                Arc::clone(&self.source),
            )
            .into());
        }

        if kind.is_none() && self.match_token(&TokenKind::Dot) {
            let member = self.consume_identifier("module path segment")?;
            return Err(self.error(CompileErrorKind::ModulePathSeparator {
                module: path.join("::"),
                member,
            }));
        }

        let kind = match kind {
            Some(kind) => kind,
            None if self.match_token(&TokenKind::As) => ImportKind::Module {
                alias: Some(self.consume_declared_identifier("alias name")?),
            },
            None => ImportKind::Module { alias: None },
        };

        self.consume_semicolon()?;
        let span = start_span.merge(self.previous().span);
        Ok(Stmt::new(
            StmtKind::Needs(NeedsStmt { path, kind, span }),
            span,
        ))
    }

    fn use_symbol_list(&mut self) -> Result<Vec<String>> {
        let mut symbols = Vec::new();
        loop {
            self.skip_line_ends();
            if self.match_token(&TokenKind::RBrace) {
                break;
            }
            symbols.push(self.consume_identifier("symbol name")?);
            self.skip_line_ends();
            if self.match_token(&TokenKind::Comma) {
                continue;
            }
            self.skip_line_ends();
            self.consume(&TokenKind::RBrace, "',' or '}'")?;
            break;
        }
        if symbols.is_empty() {
            return Err(self.error(CompileErrorKind::UnexpectedToken {
                expected: "at least one symbol name".to_string(),
                found: "}".to_string(),
            }));
        }
        Ok(symbols)
    }
}

fn rewrite_needs(words: &[String]) -> String {
    let join = |words: &[String]| {
        let mut out = String::new();
        for word in words {
            match word.as_str() {
                "as" => out.push_str(" as "),
                "," => out.push_str(", "),
                _ => out.push_str(word),
            }
        }
        out
    };
    match words.iter().position(|word| word == "from") {
        Some(at) => format!("use {}::{{{}}}", join(&words[at + 1..]), join(&words[..at])),
        None => format!("use {}", join(words)),
    }
}
