//! Assembler: Parses .aasm text and produces bytecode

use super::lexer::{Lexer, Token};
use crate::bytecode::{Constant, Function, GlobalLayout, UpvalueDescriptor};
use std::collections::HashMap;
use thiserror::Error;

/// Assembler error types
#[derive(Debug, Error)]
pub enum AssemblerError {
    #[error("Parse error at line {line}: {message}")]
    ParseError { line: usize, message: String },

    #[error("Unknown opcode: {0}")]
    UnknownOpcode(String),

    #[error("Undefined label: {0}")]
    UndefinedLabel(String),

    #[error("Duplicate label: {0}")]
    DuplicateLabel(String),

    #[error("Invalid register: {0}")]
    InvalidRegister(String),

    #[error("Invalid number: {0}")]
    InvalidNumber(String),

    #[error("Invalid string literal: {0}")]
    InvalidString(String),

    #[error("Expected {expected}, got {got}")]
    Expected { expected: String, got: String },

    #[error("Unexpected end of input")]
    UnexpectedEof,

    #[error("Unsupported assembly version: {0} (expected 2)")]
    UnsupportedVersion(i64),
}

/// Result type for assembler operations
pub type Result<T> = std::result::Result<T, AssemblerError>;

/// Assemble .aasm source into bytecode functions
pub fn assemble(source: &str) -> Result<Vec<Function>> {
    let mut parser = AasmParser::new(source);
    parser.parse()
}

/// Convenience function that takes a string
pub fn assemble_from_string(source: &str) -> Result<Vec<Function>> {
    assemble(source)
}

/// Parser for .aasm files
pub(super) struct AasmParser<'a> {
    pub(super) lexer: Lexer<'a>,
    pub(super) current: Token,
}

impl<'a> AasmParser<'a> {
    fn new(source: &'a str) -> Self {
        let mut lexer = Lexer::new(source);
        let current = lexer.next_token().unwrap_or(Token::Eof);
        Self { lexer, current }
    }

    pub(super) fn advance(&mut self) -> Result<Token> {
        let prev = std::mem::replace(&mut self.current, self.lexer.next_token()?);
        Ok(prev)
    }

    fn skip_newlines(&mut self) -> Result<()> {
        while self.current == Token::Newline {
            self.advance()?;
        }
        Ok(())
    }

    fn expect(&mut self, expected: Token) -> Result<()> {
        if std::mem::discriminant(&self.current) == std::mem::discriminant(&expected) {
            self.advance()?;
            Ok(())
        } else {
            Err(AssemblerError::Expected {
                expected: format!("{:?}", expected),
                got: format!("{:?}", self.current),
            })
        }
    }

    fn parse(&mut self) -> Result<Vec<Function>> {
        let mut functions = Vec::new();

        self.skip_newlines()?;

        if let Token::Directive(ref d) = self.current
            && d == "version"
        {
            self.advance()?;
            if let Token::Int(version) = self.current {
                if version != 2 {
                    return Err(AssemblerError::UnsupportedVersion(version));
                }
                self.advance()?;
            }
            self.skip_newlines()?;
        }

        while self.current != Token::Eof {
            self.skip_newlines()?;
            if self.current == Token::Eof {
                break;
            }

            if let Token::Directive(ref d) = self.current.clone() {
                if d == "function" {
                    let func = self.parse_function()?;
                    functions.push(func);
                } else {
                    return Err(AssemblerError::ParseError {
                        line: self.lexer.current_line(),
                        message: format!("Expected .function, got .{}", d),
                    });
                }
            } else {
                self.skip_newlines()?;
                if self.current == Token::Eof {
                    break;
                }
                if let Token::Directive(_) = self.current {
                    continue;
                }
                return Err(AssemblerError::ParseError {
                    line: self.lexer.current_line(),
                    message: format!("Expected directive, got {:?}", self.current),
                });
            }
        }

        Ok(functions)
    }

    fn parse_function(&mut self) -> Result<Function> {
        // .function N
        self.expect(Token::Directive("function".to_string()))?;
        let _func_idx = match self.advance()? {
            Token::Int(n) => n as usize,
            t => {
                return Err(AssemblerError::Expected {
                    expected: "function index".to_string(),
                    got: format!("{:?}", t),
                });
            }
        };
        self.skip_newlines()?;

        let mut name = None;
        let mut arity = 0u16;
        let mut num_registers = 0u32;
        let mut constants = Vec::new();
        let mut bytecode = Vec::new();
        let mut global_names = Vec::new();
        let mut upvalue_descriptors = Vec::new();
        let mut labels: HashMap<String, usize> = HashMap::new();
        let mut label_refs: Vec<(usize, String, bool, u8)> = Vec::new(); // (offset, label, is_conditional, offset_word)

        loop {
            match &self.current {
                Token::Directive(d) => match d.as_str() {
                    "name" => {
                        self.advance()?;
                        if let Token::String(s) = self.advance()? {
                            name = Some(s);
                        }
                    }
                    "arity" => {
                        self.advance()?;
                        if let Token::Int(n) = self.advance()? {
                            arity = u16::try_from(n).map_err(|_| {
                                AssemblerError::InvalidNumber(format!("Arity is out of range: {n}"))
                            })?;
                        }
                    }
                    "registers" => {
                        self.advance()?;
                        if let Token::Int(n) = self.advance()? {
                            num_registers = u32::try_from(n).map_err(|_| {
                                AssemblerError::InvalidNumber(format!(
                                    "Register count is out of range: {n}"
                                ))
                            })?;
                        }
                    }
                    "globals" => {
                        self.advance()?;
                        self.skip_newlines()?;
                        global_names = self.parse_globals()?;
                    }
                    "constants" => {
                        self.advance()?;
                        self.skip_newlines()?;
                        constants = self.parse_constants()?;
                    }
                    "upvalues" => {
                        self.advance()?;
                        self.skip_newlines()?;
                        upvalue_descriptors = self.parse_upvalues()?;
                    }
                    "code" => {
                        self.advance()?;
                        self.skip_newlines()?;
                        self.parse_code(&mut bytecode, &mut labels, &mut label_refs)?;
                        break;
                    }
                    "function" => break,
                    _ => {
                        self.advance()?;
                    }
                },
                Token::Newline => {
                    self.advance()?;
                }
                Token::Eof => break,
                _ => break,
            }
        }

        /*
         * This is the label patching phase.
         * For each label reference, we look up the target label's offset,
         * calculate the relative jump distance, and patch the instruction.
         */
        for (offset, label, is_conditional, offset_word) in label_refs {
            let target = labels
                .get(&label)
                .ok_or_else(|| AssemblerError::UndefinedLabel(label.clone()))?;

            let target = i64::try_from(*target).map_err(|_| {
                AssemblerError::InvalidNumber(format!("Label offset is too large: {target}"))
            })?;
            let offset_i64 = i64::try_from(offset).map_err(|_| {
                AssemblerError::InvalidNumber(format!("Instruction offset is too large: {offset}"))
            })?;
            let instruction_words = i64::from(offset_word) + 1;
            let relative = target - offset_i64 - instruction_words;

            if offset_word > 0 {
                let relative = i32::try_from(relative).map_err(|_| {
                    AssemblerError::InvalidNumber(format!(
                        "Long jump offset is out of range: {relative}"
                    ))
                })?;
                bytecode[offset + usize::from(offset_word)] =
                    u32::from_ne_bytes(relative.to_ne_bytes());
                continue;
            }

            let relative = i16::try_from(relative).map_err(|_| {
                AssemblerError::InvalidNumber(format!("Jump offset is out of range: {relative}"))
            })?;

            // Patch the instruction
            let instr = bytecode[offset];
            let patched = if is_conditional {
                // Keep the register in A field
                let a = (instr >> 16) & 0xFF;
                let op = instr >> 24;
                let relative = u16::from_ne_bytes(relative.to_ne_bytes());
                (op << 24) | (a << 16) | u32::from(relative)
            } else {
                // Jump has no register
                let op = instr >> 24;
                let relative = u16::from_ne_bytes(relative.to_ne_bytes());
                (op << 24) | u32::from(relative)
            };
            bytecode[offset] = patched;
        }

        let mut func = Function::new(name, arity);
        func.num_registers = num_registers;
        func.set_bytecode(bytecode);
        func.constants = constants;
        func.global_layout = GlobalLayout::new(global_names);
        func.upvalue_descriptors = upvalue_descriptors;
        func.compute_global_layout_hash();

        Ok(func)
    }

    fn parse_constants(&mut self) -> Result<Vec<Constant>> {
        let mut constants = Vec::new();

        loop {
            match &self.current {
                Token::Directive(_) => break,
                Token::Eof => break,
                Token::Newline => {
                    self.advance()?;
                    continue;
                }
                Token::Int(idx) => {
                    // Parse constant: INDEX: TYPE VALUE
                    let _idx = *idx;
                    self.advance()?;
                    self.expect(Token::Colon)?;

                    let value = self.parse_constant_value()?;
                    constants.push(value);
                    self.skip_newlines()?;
                }
                _ => break,
            }
        }

        Ok(constants)
    }

    /// Parse global names section: INDEX: "name"
    fn parse_globals(&mut self) -> Result<Vec<String>> {
        let mut globals = Vec::new();

        loop {
            match &self.current {
                Token::Directive(_) => break,
                Token::Eof => break,
                Token::Newline => {
                    self.advance()?;
                    continue;
                }
                Token::Int(idx) => {
                    // Parse global: INDEX: "name"
                    let idx = *idx as usize;
                    self.advance()?;
                    self.expect(Token::Colon)?;

                    if let Token::String(name) = self.advance()? {
                        // Ensure the vec is large enough
                        if globals.len() <= idx {
                            globals.resize(idx + 1, String::new());
                        }
                        globals[idx] = name;
                    }
                    self.skip_newlines()?;
                }
                _ => break,
            }
        }

        Ok(globals)
    }

    /// Parse upvalue descriptors section: INDEX: (local|upvalue) INDEX
    fn parse_upvalues(&mut self) -> Result<Vec<UpvalueDescriptor>> {
        let mut upvalues = Vec::new();

        loop {
            match &self.current {
                Token::Directive(_) => break,
                Token::Eof => break,
                Token::Newline => {
                    self.advance()?;
                    continue;
                }
                Token::Int(idx) => {
                    // Parse upvalue: INDEX: (local|upvalue) INDEX
                    let idx = *idx as usize;
                    self.advance()?;
                    self.expect(Token::Colon)?;

                    // Parse kind: "local" or "upvalue"
                    let is_local = match &self.current {
                        Token::Ident(s) if s == "local" => {
                            self.advance()?;
                            true
                        }
                        Token::Ident(s) if s == "upvalue" => {
                            self.advance()?;
                            false
                        }
                        _ => {
                            return Err(AssemblerError::Expected {
                                expected: "local or upvalue".to_string(),
                                got: format!("{:?}", self.current),
                            });
                        }
                    };

                    // Parse the index
                    let index = self.parse_u16()?;

                    // Ensure the vec is large enough
                    if upvalues.len() <= idx {
                        upvalues.resize(
                            idx + 1,
                            UpvalueDescriptor {
                                is_local: true,
                                index: 0,
                            },
                        );
                    }
                    upvalues[idx] = UpvalueDescriptor { is_local, index };
                    self.skip_newlines()?;
                }
                _ => break,
            }
        }

        Ok(upvalues)
    }

    /// Parse a constant value of the form TYPE VALUE
    fn parse_constant_value(&mut self) -> Result<Constant> {
        match self.advance()? {
            Token::Ident(type_name) => {
                match type_name.as_str() {
                    "int" => {
                        if let Token::Int(n) = self.advance()? {
                            Ok(Constant::Int(n))
                        } else {
                            Err(AssemblerError::Expected {
                                expected: "integer".to_string(),
                                got: format!("{:?}", self.current),
                            })
                        }
                    }
                    "float" => match self.advance()? {
                        Token::Float(f) => Ok(Constant::Float(f.to_bits())),
                        Token::Int(n) => Ok(Constant::Float((n as f64).to_bits())),
                        Token::Ident(s) if s == "nan" => Ok(Constant::Float(f64::NAN.to_bits())),
                        Token::Ident(s) if s == "inf" => {
                            Ok(Constant::Float(f64::INFINITY.to_bits()))
                        }
                        t => Err(AssemblerError::Expected {
                            expected: "float".to_string(),
                            got: format!("{:?}", t),
                        }),
                    },
                    "bool" => match self.advance()? {
                        Token::Bool(b) => Ok(Constant::Bool(b)),
                        Token::Ident(s) if s == "true" => Ok(Constant::Bool(true)),
                        Token::Ident(s) if s == "false" => Ok(Constant::Bool(false)),
                        t => Err(AssemblerError::Expected {
                            expected: "bool".to_string(),
                            got: format!("{:?}", t),
                        }),
                    },
                    "string" => {
                        if let Token::String(s) = self.advance()? {
                            Ok(Constant::String(s))
                        } else {
                            Err(AssemblerError::Expected {
                                expected: "string".to_string(),
                                got: format!("{:?}", self.current),
                            })
                        }
                    }
                    "ptr" => Err(AssemblerError::ParseError {
                        line: self.lexer.current_line(),
                        message: "raw heap pointers are not valid AVBC v2 constants".to_string(),
                    }),
                    "func" => {
                        // func @N
                        self.expect(Token::At)?;
                        if let Token::Int(n) = self.advance()? {
                            // Encode as nested function marker (uses dedicated tag)
                            let index = u32::try_from(n - 1).map_err(|_| {
                                AssemblerError::InvalidNumber(format!(
                                    "invalid nested function index: {n}"
                                ))
                            })?;
                            if matches!(self.current, Token::String(_)) {
                                self.advance()?;
                            }
                            Ok(Constant::NestedFunction(index))
                        } else {
                            Err(AssemblerError::Expected {
                                expected: "function index".to_string(),
                                got: format!("{:?}", self.current),
                            })
                        }
                    }
                    "null" => Ok(Constant::Null),
                    "native" => {
                        if let Token::String(_) = self.advance()? {
                            // We can't recreate native functions, return null
                            Ok(Constant::Null)
                        } else {
                            Ok(Constant::Null)
                        }
                    }
                    _ => Err(AssemblerError::Expected {
                        expected: "constant type".to_string(),
                        got: type_name,
                    }),
                }
            }
            Token::Null => Ok(Constant::Null),
            t => Err(AssemblerError::Expected {
                expected: "constant type".to_string(),
                got: format!("{:?}", t),
            }),
        }
    }

    fn parse_code(
        &mut self,
        bytecode: &mut Vec<u32>,
        labels: &mut HashMap<String, usize>,
        label_refs: &mut Vec<(usize, String, bool, u8)>,
    ) -> Result<()> {
        loop {
            match &self.current {
                Token::Directive(_) => break,
                Token::Eof => break,
                Token::Newline => {
                    self.advance()?;
                    continue;
                }
                Token::LabelRef(name) => {
                    // This is a label definition (L0:)
                    let label_name = name.clone();
                    self.advance()?;
                    if self.current == Token::Colon {
                        self.advance()?;
                        if labels.contains_key(&label_name) {
                            return Err(AssemblerError::DuplicateLabel(label_name));
                        }
                        labels.insert(label_name, bytecode.len());
                    } else {
                        return Err(AssemblerError::Expected {
                            expected: "colon after label".to_string(),
                            got: format!("{:?}", self.current),
                        });
                    }
                }
                Token::Int(_) => {
                    // Skip instruction offset
                    self.advance()?;
                    self.expect(Token::Colon)?;
                    self.parse_instruction(bytecode, label_refs)?;
                }
                Token::Ident(_) => {
                    // Instruction without offset
                    self.parse_instruction(bytecode, label_refs)?;
                }
                _ => {
                    return Err(AssemblerError::ParseError {
                        line: self.lexer.current_line(),
                        message: format!("Unexpected token in code: {:?}", self.current),
                    });
                }
            }
        }
        Ok(())
    }

    pub(super) fn parse_register(&mut self) -> Result<u8> {
        match self.advance()? {
            Token::Register(r) => {
                u8::try_from(r).map_err(|_| AssemblerError::InvalidRegister(format!("r{r}")))
            }
            t => Err(AssemblerError::Expected {
                expected: "register".to_string(),
                got: format!("{:?}", t),
            }),
        }
    }

    pub(super) fn parse_wide_register(&mut self) -> Result<u16> {
        match self.advance()? {
            Token::Register(register) => Ok(register),
            token => Err(AssemblerError::Expected {
                expected: "register".to_string(),
                got: format!("{token:?}"),
            }),
        }
    }

    pub(super) fn parse_u8(&mut self) -> Result<u8> {
        match self.advance()? {
            Token::Int(n) if (0..=255).contains(&n) => u8::try_from(n)
                .map_err(|_| AssemblerError::InvalidNumber(format!("{n} (must be 0-255)"))),
            Token::Int(n) => Err(AssemblerError::InvalidNumber(format!(
                "{} (must be 0-255)",
                n
            ))),
            t => Err(AssemblerError::Expected {
                expected: "u8".to_string(),
                got: format!("{:?}", t),
            }),
        }
    }

    pub(super) fn parse_u16(&mut self) -> Result<u16> {
        match self.advance()? {
            Token::Int(n) => u16::try_from(n).map_err(|_| {
                AssemblerError::InvalidNumber(format!("{n} (must fit unsigned 16-bit)"))
            }),
            token => Err(AssemblerError::Expected {
                expected: "u16".to_string(),
                got: format!("{token:?}"),
            }),
        }
    }

    pub(super) fn parse_i16(&mut self) -> Result<i16> {
        match self.advance()? {
            Token::Int(n) if n >= i16::MIN as i64 && n <= i16::MAX as i64 => i16::try_from(n)
                .map_err(|_| AssemblerError::InvalidNumber(format!("{n} (must fit i16)"))),
            Token::Int(n) => Err(AssemblerError::InvalidNumber(format!(
                "{} (must fit i16)",
                n
            ))),
            t => Err(AssemblerError::Expected {
                expected: "i16".to_string(),
                got: format!("{:?}", t),
            }),
        }
    }

    pub(super) fn parse_u32(&mut self) -> Result<u32> {
        match self.advance()? {
            Token::Int(n) => u32::try_from(n).map_err(|_| {
                AssemblerError::InvalidNumber(format!("{n} (must fit unsigned 32-bit)"))
            }),
            token => Err(AssemblerError::Expected {
                expected: "u32".to_string(),
                got: format!("{token:?}"),
            }),
        }
    }

    pub(super) fn skip_comma(&mut self) -> Result<()> {
        if self.current == Token::Comma {
            self.advance()?;
        }
        Ok(())
    }
}
