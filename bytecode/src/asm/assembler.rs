
use super::lexer::{Lexer, Token};
use crate::bytecode::{
    Constant, DefId, EnumDefId, EnumFieldSchema, EnumSchema, EnumVariantSchema, FloatWidth,
    Function, GlobalLayout, IntWidth, StructFieldSchema, StructSchema, TypeDescriptor,
    UpvalueDescriptor,
};
use std::collections::HashMap;
use thiserror::Error;

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

    #[error("Unsupported assembly version: {0} (expected 3)")]
    UnsupportedVersion(i64),
}

pub type Result<T> = std::result::Result<T, AssemblerError>;

pub fn assemble(source: &str) -> Result<Vec<Function>> {
    let mut parser = AasmParser::new(source);
    parser.parse()
}

pub fn assemble_from_string(source: &str) -> Result<Vec<Function>> {
    assemble(source)
}

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
                if version != 3 {
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
        let mut struct_schemas = Vec::new();
        let mut enum_schemas = Vec::new();
        let mut jit_unsupported_struct = false;
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
                    "schemas" => {
                        self.advance()?;
                        self.skip_newlines()?;
                        struct_schemas = self.parse_schemas()?;
                    }
                    "enum_schemas" => {
                        self.advance()?;
                        self.skip_newlines()?;
                        enum_schemas = self.parse_enum_schemas()?;
                    }
                    "jit_unsupported_struct" => {
                        self.advance()?;
                        jit_unsupported_struct = match self.advance()? {
                            Token::Bool(value) => value,
                            Token::Ident(value) if value == "true" => true,
                            Token::Ident(value) if value == "false" => false,
                            token => {
                                return Err(AssemblerError::Expected {
                                    expected: "boolean".to_string(),
                                    got: format!("{token:?}"),
                                });
                            }
                        };
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

            let instr = bytecode[offset];
            let patched = if is_conditional {
                let a = (instr >> 16) & 0xFF;
                let op = instr >> 24;
                let relative = u16::from_ne_bytes(relative.to_ne_bytes());
                (op << 24) | (a << 16) | u32::from(relative)
            } else {
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
        func.struct_schemas = struct_schemas;
        func.enum_schemas = enum_schemas;
        func.jit_unsupported_struct = jit_unsupported_struct;
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

    fn parse_schemas(&mut self) -> Result<Vec<StructSchema>> {
        let mut schemas = Vec::new();

        loop {
            match &self.current {
                Token::Directive(_) | Token::Eof => break,
                Token::Newline => {
                    self.advance()?;
                }
                Token::Int(index) => {
                    let index = *index as usize;
                    self.advance()?;
                    self.expect(Token::Colon)?;
                    let def_name = match self.advance()? {
                        Token::String(name) => name,
                        token => {
                            return Err(AssemblerError::Expected {
                                expected: "struct definition name".to_string(),
                                got: format!("{token:?}"),
                            });
                        }
                    };
                    let ordinal = self.parse_u32()?;
                    let type_arg_count = self.parse_u16()? as usize;
                    let field_count = self.parse_u16()? as usize;
                    if index != schemas.len() {
                        return Err(AssemblerError::ParseError {
                            line: self.lexer.current_line(),
                            message: format!("struct schema index {index} is not contiguous"),
                        });
                    }

                    let mut type_args = Vec::with_capacity(type_arg_count);
                    for _ in 0..type_arg_count {
                        let descriptor = match self.advance()? {
                            Token::String(descriptor) => descriptor,
                            token => {
                                return Err(AssemblerError::Expected {
                                    expected: "struct type argument descriptor".to_string(),
                                    got: format!("{token:?}"),
                                });
                            }
                        };
                        type_args.push(parse_type_descriptor_text(&descriptor).map_err(
                            |message| AssemblerError::ParseError {
                                line: self.lexer.current_line(),
                                message,
                            },
                        )?);
                    }

                    self.skip_newlines()?;
                    let mut fields = Vec::with_capacity(field_count);
                    for _ in 0..field_count {
                        let field_index = match self.advance()? {
                            Token::Int(index) => index as usize,
                            token => {
                                return Err(AssemblerError::Expected {
                                    expected: "struct field index".to_string(),
                                    got: format!("{token:?}"),
                                });
                            }
                        };
                        self.expect(Token::Colon)?;
                        let field_name = match self.advance()? {
                            Token::String(name) => name,
                            token => {
                                return Err(AssemblerError::Expected {
                                    expected: "struct field name".to_string(),
                                    got: format!("{token:?}"),
                                });
                            }
                        };
                        let descriptor_text = match self.advance()? {
                            Token::String(descriptor) => descriptor,
                            token => {
                                return Err(AssemblerError::Expected {
                                    expected: "struct field descriptor".to_string(),
                                    got: format!("{token:?}"),
                                });
                            }
                        };
                        let descriptor =
                            parse_type_descriptor_text(&descriptor_text).map_err(|message| {
                                AssemblerError::ParseError {
                                    line: self.lexer.current_line(),
                                    message,
                                }
                            })?;
                        if field_index != fields.len() {
                            return Err(AssemblerError::ParseError {
                                line: self.lexer.current_line(),
                                message: format!(
                                    "struct field index {field_index} is not contiguous"
                                ),
                            });
                        }
                        fields.push(StructFieldSchema {
                            offset: u16::try_from(field_index).map_err(|_| {
                                AssemblerError::InvalidNumber(format!(
                                    "struct field index {field_index}"
                                ))
                            })?,
                            name: field_name,
                            ty: descriptor,
                        });
                        self.skip_newlines()?;
                    }
                    schemas.push(StructSchema::with_identity(
                        u32::try_from(index).map_err(|_| {
                            AssemblerError::InvalidNumber(format!("struct schema index {index}"))
                        })?,
                        DefId::from_display_name(&def_name, ordinal),
                        type_args,
                        fields,
                    ));
                }
                token => {
                    return Err(AssemblerError::ParseError {
                        line: self.lexer.current_line(),
                        message: format!("unexpected token in schemas: {token:?}"),
                    });
                }
            }
        }

        Ok(schemas)
    }

    fn parse_enum_schemas(&mut self) -> Result<Vec<EnumSchema>> {
        let mut schemas = Vec::new();

        loop {
            match &self.current {
                Token::Directive(_) | Token::Eof => break,
                Token::Newline => {
                    self.advance()?;
                }
                Token::Int(index) => {
                    let index = *index as usize;
                    self.advance()?;
                    self.expect(Token::Colon)?;
                    let name = match self.advance()? {
                        Token::String(name) => name,
                        token => {
                            return Err(AssemblerError::Expected {
                                expected: "enum schema name".to_string(),
                                got: format!("{token:?}"),
                            });
                        }
                    };
                    let def_name = match self.advance()? {
                        Token::String(name) => name,
                        token => {
                            return Err(AssemblerError::Expected {
                                expected: "enum definition name".to_string(),
                                got: format!("{token:?}"),
                            });
                        }
                    };
                    let ordinal = self.parse_u32()?;
                    let arity = self.parse_u16()? as usize;
                    let variant_count = self.parse_u16()? as usize;
                    if index != schemas.len() {
                        return Err(AssemblerError::ParseError {
                            line: self.lexer.current_line(),
                            message: format!("enum schema index {index} is not contiguous"),
                        });
                    }

                    let mut type_args = Vec::with_capacity(arity);
                    for _ in 0..arity {
                        let descriptor = match self.advance()? {
                            Token::String(descriptor) => descriptor,
                            token => {
                                return Err(AssemblerError::Expected {
                                    expected: "enum type argument descriptor".to_string(),
                                    got: format!("{token:?}"),
                                });
                            }
                        };
                        type_args.push(parse_type_descriptor_text(&descriptor).map_err(
                            |message| AssemblerError::ParseError {
                                line: self.lexer.current_line(),
                                message,
                            },
                        )?);
                    }

                    self.skip_newlines()?;
                    let mut variants = Vec::with_capacity(variant_count);
                    for _ in 0..variant_count {
                        let variant_index = match self.advance()? {
                            Token::Int(index) => index as usize,
                            token => {
                                return Err(AssemblerError::Expected {
                                    expected: "enum variant index".to_string(),
                                    got: format!("{token:?}"),
                                });
                            }
                        };
                        self.expect(Token::Colon)?;
                        let variant_name = match self.advance()? {
                            Token::String(name) => name,
                            token => {
                                return Err(AssemblerError::Expected {
                                    expected: "enum variant name".to_string(),
                                    got: format!("{token:?}"),
                                });
                            }
                        };
                        let field_count = self.parse_u16()? as usize;
                        if variant_index != variants.len() {
                            return Err(AssemblerError::ParseError {
                                line: self.lexer.current_line(),
                                message: format!(
                                    "enum variant index {variant_index} is not contiguous"
                                ),
                            });
                        }

                        self.skip_newlines()?;
                        let mut fields = Vec::with_capacity(field_count);
                        for _ in 0..field_count {
                            let field_index = match self.advance()? {
                                Token::Int(index) => index as usize,
                                token => {
                                    return Err(AssemblerError::Expected {
                                        expected: "enum field index".to_string(),
                                        got: format!("{token:?}"),
                                    });
                                }
                            };
                            self.expect(Token::Colon)?;
                            let field_name = match self.advance()? {
                                Token::String(name) if !name.is_empty() => Some(name),
                                Token::String(_) => None,
                                token => {
                                    return Err(AssemblerError::Expected {
                                        expected: "enum field name".to_string(),
                                        got: format!("{token:?}"),
                                    });
                                }
                            };
                            let descriptor_text = match self.advance()? {
                                Token::String(descriptor) => descriptor,
                                token => {
                                    return Err(AssemblerError::Expected {
                                        expected: "enum field descriptor".to_string(),
                                        got: format!("{token:?}"),
                                    });
                                }
                            };
                            if field_index != fields.len() {
                                return Err(AssemblerError::ParseError {
                                    line: self.lexer.current_line(),
                                    message: format!(
                                        "enum field index {field_index} is not contiguous"
                                    ),
                                });
                            }
                            let ty = parse_type_descriptor_text(&descriptor_text).map_err(
                                |message| AssemblerError::ParseError {
                                    line: self.lexer.current_line(),
                                    message,
                                },
                            )?;
                            fields.push(EnumFieldSchema {
                                offset: u16::try_from(field_index).map_err(|_| {
                                    AssemblerError::InvalidNumber(format!(
                                        "enum field index {field_index}"
                                    ))
                                })?,
                                name: field_name,
                                ty,
                            });
                            self.skip_newlines()?;
                        }
                        variants.push(EnumVariantSchema {
                            variant_id: u16::try_from(variant_index).map_err(|_| {
                                AssemblerError::InvalidNumber(format!(
                                    "enum variant index {variant_index}"
                                ))
                            })?,
                            name: variant_name,
                            fields: fields.into_boxed_slice(),
                        });
                    }
                    schemas.push(EnumSchema::with_identity(
                        u16::try_from(index).map_err(|_| {
                            AssemblerError::InvalidNumber(format!("enum schema index {index}"))
                        })?,
                        EnumDefId::from_display_name(&def_name, ordinal),
                        type_args,
                        name,
                        variants,
                    ));
                }
                token => {
                    return Err(AssemblerError::ParseError {
                        line: self.lexer.current_line(),
                        message: format!("unexpected token in enum schemas: {token:?}"),
                    });
                }
            }
        }

        Ok(schemas)
    }

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
                    let idx = *idx as usize;
                    self.advance()?;
                    self.expect(Token::Colon)?;

                    if let Token::String(name) = self.advance()? {
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
                    let idx = *idx as usize;
                    self.advance()?;
                    self.expect(Token::Colon)?;

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

                    let index = self.parse_u16()?;

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
                        message: "raw heap pointers are not valid AVBC v3 constants".to_string(),
                    }),
                    "func" => {
                        self.expect(Token::At)?;
                        if let Token::Int(n) = self.advance()? {
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
                    self.advance()?;
                    self.expect(Token::Colon)?;
                    self.parse_instruction(bytecode, label_refs)?;
                }
                Token::Ident(_) => {
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

    pub(super) fn skip_comma(&mut self) -> Result<()> {
        if self.current == Token::Comma {
            self.advance()?;
        }
        Ok(())
    }
}

struct TypeDescriptorParser<'a> {
    input: &'a str,
    position: usize,
}

impl<'a> TypeDescriptorParser<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, position: 0 }
    }

    fn parse(mut self) -> std::result::Result<TypeDescriptor, String> {
        let descriptor = self.parse_descriptor()?;
        if self.position != self.input.len() {
            return Err(format!(
                "unexpected descriptor suffix: {}",
                &self.input[self.position..]
            ));
        }
        Ok(descriptor)
    }

    fn parse_descriptor(&mut self) -> std::result::Result<TypeDescriptor, String> {
        if self.consume("unit") {
            return Ok(TypeDescriptor::Unit);
        }
        if self.consume("bool") {
            return Ok(TypeDescriptor::Bool);
        }
        if self.consume("string") {
            return Ok(TypeDescriptor::String);
        }
        if self.consume("any") {
            return Ok(TypeDescriptor::Any);
        }
        if self.consume("error") {
            return Ok(TypeDescriptor::Error);
        }
        if self.consume("never") {
            return Ok(TypeDescriptor::Never);
        }
        if self.consume("int:") {
            let width = self.take_until(|character| !character.is_ascii_alphanumeric());
            let width = match width {
                "I8" => IntWidth::I8,
                "I16" => IntWidth::I16,
                "I32" => IntWidth::I32,
                "I64" => IntWidth::I64,
                "U8" => IntWidth::U8,
                "U16" => IntWidth::U16,
                "U32" => IntWidth::U32,
                "U64" => IntWidth::U64,
                other => return Err(format!("unknown integer width: {other}")),
            };
            return Ok(TypeDescriptor::Int(width));
        }
        if self.consume("float:") {
            let width = self.take_until(|character| !character.is_ascii_alphanumeric());
            let width = match width {
                "F32" => FloatWidth::F32,
                "F64" => FloatWidth::F64,
                other => return Err(format!("unknown float width: {other}")),
            };
            return Ok(TypeDescriptor::Float(width));
        }
        if self.consume("struct:") {
            let schema_id = self.take_until(|character| matches!(character, ',' | '>' | ';'));
            let schema_id = schema_id
                .parse::<u32>()
                .map_err(|_| "struct descriptor schema id is invalid".to_string())?;
            return Ok(TypeDescriptor::Struct(schema_id));
        }
        if self.consume("enum:") {
            let schema_id = self.take_until(|character| matches!(character, ',' | '>' | ';'));
            let schema_id = schema_id
                .parse::<u16>()
                .map_err(|_| "enum descriptor schema id is invalid".to_string())?;
            return Ok(TypeDescriptor::Enum(schema_id));
        }
        if self.consume("option<") {
            let inner = self.parse_descriptor()?;
            self.expect('>')?;
            return Ok(TypeDescriptor::Option(Box::new(inner)));
        }
        if self.consume("result<") {
            let ok = self.parse_descriptor()?;
            self.expect(',')?;
            let err = self.parse_descriptor()?;
            self.expect('>')?;
            return Ok(TypeDescriptor::Result(Box::new(ok), Box::new(err)));
        }
        if self.consume("array<") {
            let inner = self.parse_descriptor()?;
            if self.consume(";") {
                let length = self.take_until(|character| !character.is_ascii_digit());
                let length = length
                    .parse::<u32>()
                    .map_err(|_| format!("invalid fixed array length: {length}"))?;
                self.expect('>')?;
                return Ok(TypeDescriptor::FixedArray(Box::new(inner), length));
            }
            self.expect('>')?;
            return Ok(TypeDescriptor::Array(Box::new(inner)));
        }
        if self.consume("vec<") {
            let inner = self.parse_descriptor()?;
            self.expect('>')?;
            return Ok(TypeDescriptor::Vec(Box::new(inner)));
        }
        Err(format!(
            "unknown type descriptor at {}",
            &self.input[self.position..]
        ))
    }

    fn consume(&mut self, prefix: &str) -> bool {
        if self.input[self.position..].starts_with(prefix) {
            self.position += prefix.len();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, expected: char) -> std::result::Result<(), String> {
        if self.input[self.position..].starts_with(expected) {
            self.position += expected.len_utf8();
            Ok(())
        } else {
            Err(format!("expected '{expected}' in type descriptor"))
        }
    }

    fn take_until<F>(&mut self, stop: F) -> &str
    where
        F: Fn(char) -> bool,
    {
        let start = self.position;
        while let Some(character) = self.input[self.position..].chars().next() {
            if stop(character) {
                break;
            }
            self.position += character.len_utf8();
        }
        &self.input[start..self.position]
    }
}

fn parse_type_descriptor_text(input: &str) -> std::result::Result<TypeDescriptor, String> {
    TypeDescriptorParser::new(input).parse()
}
