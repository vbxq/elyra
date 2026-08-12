//! Opcode parsing for the assembler

use super::assembler::{AasmParser, AssemblerError, Result};
use super::lexer::Token;
use crate::bytecode::OpCode;

impl<'a> AasmParser<'a> {
    pub(super) fn parse_instruction(
        &mut self,
        bytecode: &mut Vec<u32>,
        label_refs: &mut Vec<(usize, String, bool, u8)>,
    ) -> Result<()> {
        let opcode_name = match self.advance()? {
            Token::Ident(name) => name,
            t => {
                return Err(AssemblerError::Expected {
                    expected: "opcode".to_string(),
                    got: format!("{:?}", t),
                });
            }
        };

        let mut extension_words = Vec::new();
        let instr = match opcode_name.as_str() {
            "Move" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let b = self.parse_register()?;
                encode_a(OpCode::Move, a, b, 0)
            }
            "LoadI" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let imm = self.parse_i16()?;
                encode_b(OpCode::LoadI, a, imm)
            }
            "LoadK" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let k = self.parse_i16()?;
                encode_b(OpCode::LoadK, a, k)
            }
            "LoadKWide" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let index = self.parse_u32()?;
                extension_words.push(index);
                encode_a(OpCode::LoadKWide, a, 0, 0)
            }
            "LoadNull" => {
                let a = self.parse_register()?;
                encode_a(OpCode::LoadNull, a, 0, 0)
            }
            "LoadUnit" => {
                let a = self.parse_register()?;
                encode_a(OpCode::LoadUnit, a, 0, 0)
            }
            "LoadNone" => {
                let a = self.parse_register()?;
                encode_a(OpCode::LoadNone, a, 0, 0)
            }
            "LoadBool" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let b = match self.advance()? {
                    Token::Bool(b) => {
                        if b {
                            1
                        } else {
                            0
                        }
                    }
                    Token::Ident(s) if s == "true" => 1,
                    Token::Ident(s) if s == "false" => 0,
                    t => {
                        return Err(AssemblerError::Expected {
                            expected: "bool".to_string(),
                            got: format!("{:?}", t),
                        });
                    }
                };
                encode_a(OpCode::LoadBool, a, b, 0)
            }
            "Add" => self.parse_ternary_reg(OpCode::Add)?,
            "Sub" => self.parse_ternary_reg(OpCode::Sub)?,
            "Mul" => self.parse_ternary_reg(OpCode::Mul)?,
            "Div" => self.parse_ternary_reg(OpCode::Div)?,
            "Mod" => self.parse_ternary_reg(OpCode::Mod)?,
            "MakeSum" => self.parse_tagged_ternary(OpCode::MakeSum)?,
            "SumTest" => self.parse_tagged_ternary(OpCode::SumTest)?,
            "SumPayload" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let b = self.parse_register()?;
                encode_a(OpCode::SumPayload, a, b, 0)
            }
            "MatchFail" => {
                if matches!(self.current, Token::Newline | Token::Eof) {
                    encode_a(OpCode::MatchFail, 0, 0, 0)
                } else {
                    let a = self.parse_register()?;
                    self.skip_comma()?;
                    let b = self.parse_register()?;
                    self.skip_comma()?;
                    let c = self.parse_u8()?;
                    encode_a(OpCode::MatchFail, a, b, c)
                }
            }
            "Cast" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let b = self.parse_register()?;
                self.skip_comma()?;
                let c = self.parse_u8()?;
                encode_a(OpCode::Cast, a, b, c)
            }
            "RangeNew" => self.parse_ternary_reg(OpCode::RangeNew)?,
            "RangeNewInclusive" => self.parse_ternary_reg(OpCode::RangeNewInclusive)?,
            "ArraySlice" => self.parse_ternary_reg(OpCode::ArraySlice)?,
            "VecSlice" => self.parse_ternary_reg(OpCode::VecSlice)?,
            "Neg" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let b = self.parse_register()?;
                encode_a(OpCode::Neg, a, b, 0)
            }
            "Eq" => self.parse_ternary_reg(OpCode::Eq)?,
            "Ne" => self.parse_ternary_reg(OpCode::Ne)?,
            "Lt" => self.parse_ternary_reg(OpCode::Lt)?,
            "Le" => self.parse_ternary_reg(OpCode::Le)?,
            "Gt" => self.parse_ternary_reg(OpCode::Gt)?,
            "Ge" => self.parse_ternary_reg(OpCode::Ge)?,
            "Not" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let b = self.parse_register()?;
                encode_a(OpCode::Not, a, b, 0)
            }
            "Jump" => {
                let (offset, label) = self.parse_jump_target()?;
                let instr = encode_b(OpCode::Jump, 0, offset);
                if let Some(lbl) = label {
                    label_refs.push((bytecode.len(), lbl, false, 0));
                }
                instr
            }
            "JumpIf" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let (offset, label) = self.parse_jump_target()?;
                let instr = encode_b(OpCode::JumpIf, a, offset);
                if let Some(lbl) = label {
                    label_refs.push((bytecode.len(), lbl, true, 0));
                }
                instr
            }
            "JumpIfNot" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let (offset, label) = self.parse_jump_target()?;
                let instr = encode_b(OpCode::JumpIfNot, a, offset);
                if let Some(lbl) = label {
                    label_refs.push((bytecode.len(), lbl, true, 0));
                }
                instr
            }
            "JumpLong" => {
                let (offset, label) = self.parse_long_jump_target()?;
                extension_words.push(u32::from_ne_bytes(offset.to_ne_bytes()));
                if let Some(lbl) = label {
                    label_refs.push((bytecode.len(), lbl, false, 1));
                }
                encode_a(OpCode::JumpLong, 0, 0, 0)
            }
            "JumpIfLong" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let (offset, label) = self.parse_long_jump_target()?;
                extension_words.push(u32::from_ne_bytes(offset.to_ne_bytes()));
                if let Some(lbl) = label {
                    label_refs.push((bytecode.len(), lbl, true, 1));
                }
                encode_a(OpCode::JumpIfLong, a, 0, 0)
            }
            "JumpIfNotLong" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let (offset, label) = self.parse_long_jump_target()?;
                extension_words.push(u32::from_ne_bytes(offset.to_ne_bytes()));
                if let Some(lbl) = label {
                    label_refs.push((bytecode.len(), lbl, true, 1));
                }
                encode_a(OpCode::JumpIfNotLong, a, 0, 0)
            }
            "JumpIfWideLong" => {
                let register = self.parse_wide_register()?;
                self.skip_comma()?;
                let (offset, label) = self.parse_long_jump_target()?;
                extension_words.push(u32::from(register) << 16);
                extension_words.push(u32::from_ne_bytes(offset.to_ne_bytes()));
                if let Some(label) = label {
                    label_refs.push((bytecode.len(), label, true, 2));
                }
                encode_a(OpCode::JumpIfWideLong, 0, 0, 0)
            }
            "JumpIfNotWideLong" => {
                let register = self.parse_wide_register()?;
                self.skip_comma()?;
                let (offset, label) = self.parse_long_jump_target()?;
                extension_words.push(u32::from(register) << 16);
                extension_words.push(u32::from_ne_bytes(offset.to_ne_bytes()));
                if let Some(label) = label {
                    label_refs.push((bytecode.len(), label, true, 2));
                }
                encode_a(OpCode::JumpIfNotWideLong, 0, 0, 0)
            }
            "LoopWideLong" => {
                let inner = match self.advance()? {
                    Token::Ident(name) if name == "ForLoopILong" => OpCode::ForLoopILong,
                    Token::Ident(name) if name == "ForLoopIIncLong" => OpCode::ForLoopIIncLong,
                    Token::Ident(name) if name == "StringForLoopLong" => OpCode::StringForLoopLong,
                    Token::Ident(name) if name == "VecForLoopLong" => OpCode::VecForLoopLong,
                    Token::Ident(name) if name == "ArrayForLoopLong" => OpCode::ArrayForLoopLong,
                    token => {
                        return Err(AssemblerError::Expected {
                            expected: "wide loop opcode".to_string(),
                            got: format!("{token:?}"),
                        });
                    }
                };
                self.skip_comma()?;
                let register = self.parse_wide_register()?;
                self.skip_comma()?;
                let (offset, label) = self.parse_long_jump_target()?;
                extension_words.push(u32::from(register) << 16);
                extension_words.push(u32::from_ne_bytes(offset.to_ne_bytes()));
                if let Some(label) = label {
                    label_refs.push((bytecode.len(), label, true, 2));
                }
                encode_a(OpCode::LoopWideLong, u8::from(inner), 0, 0)
            }
            "ForLoopILong" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let (offset, label) = self.parse_long_jump_target()?;
                extension_words.push(u32::from_ne_bytes(offset.to_ne_bytes()));
                if let Some(lbl) = label {
                    label_refs.push((bytecode.len(), lbl, true, 1));
                }
                encode_a(OpCode::ForLoopILong, a, 0, 0)
            }
            "ForLoopIIncLong" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let (offset, label) = self.parse_long_jump_target()?;
                extension_words.push(u32::from_ne_bytes(offset.to_ne_bytes()));
                if let Some(lbl) = label {
                    label_refs.push((bytecode.len(), lbl, true, 1));
                }
                encode_a(OpCode::ForLoopIIncLong, a, 0, 0)
            }
            "StringForLoopLong" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let (offset, label) = self.parse_long_jump_target()?;
                extension_words.push(u32::from_ne_bytes(offset.to_ne_bytes()));
                if let Some(lbl) = label {
                    label_refs.push((bytecode.len(), lbl, true, 1));
                }
                encode_a(OpCode::StringForLoopLong, a, 0, 0)
            }
            "VecForLoopLong" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let (offset, label) = self.parse_long_jump_target()?;
                extension_words.push(u32::from_ne_bytes(offset.to_ne_bytes()));
                if let Some(lbl) = label {
                    label_refs.push((bytecode.len(), lbl, true, 1));
                }
                encode_a(OpCode::VecForLoopLong, a, 0, 0)
            }
            "ArrayForLoopLong" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let (offset, label) = self.parse_long_jump_target()?;
                extension_words.push(u32::from_ne_bytes(offset.to_ne_bytes()));
                if let Some(lbl) = label {
                    label_refs.push((bytecode.len(), lbl, true, 1));
                }
                encode_a(OpCode::ArrayForLoopLong, a, 0, 0)
            }
            "Call" => {
                let dest = self.parse_register()?;
                self.skip_comma()?;
                let func = self.parse_register()?;
                self.skip_comma()?;
                let nargs = self.parse_u8()?;
                encode_a(OpCode::Call, dest, func, nargs)
            }
            "CallWide" => {
                let dest = self.parse_wide_register()?;
                self.skip_comma()?;
                let func = self.parse_wide_register()?;
                self.skip_comma()?;
                let nargs = self.parse_u16()?;
                extension_words.extend(encode_wide_operands(dest, func, nargs));
                encode_a(OpCode::CallWide, 0, 0, 0)
            }
            "ArrayLit" => {
                let dest = self.parse_register()?;
                self.skip_comma()?;
                let start = self.parse_register()?;
                self.skip_comma()?;
                let count = self.parse_u8()?;
                encode_a(OpCode::ArrayLit, dest, start, count)
            }
            "ArrayLitWide" => {
                let dest = self.parse_wide_register()?;
                self.skip_comma()?;
                let start = self.parse_wide_register()?;
                self.skip_comma()?;
                let count = self.parse_u16()?;
                extension_words.extend(encode_wide_operands(dest, start, count));
                encode_a(OpCode::ArrayLitWide, 0, 0, 0)
            }
            "VecLit" => {
                let dest = self.parse_register()?;
                self.skip_comma()?;
                let start = self.parse_register()?;
                self.skip_comma()?;
                let count = self.parse_u8()?;
                encode_a(OpCode::VecLit, dest, start, count)
            }
            "VecLitWide" => {
                let dest = self.parse_wide_register()?;
                self.skip_comma()?;
                let start = self.parse_wide_register()?;
                self.skip_comma()?;
                let count = self.parse_u16()?;
                extension_words.extend(encode_wide_operands(dest, start, count));
                encode_a(OpCode::VecLitWide, 0, 0, 0)
            }
            "Return" => {
                let a = self.parse_register()?;
                encode_a(OpCode::Return, a, 0, 0)
            }
            "Return0" => encode_a(OpCode::Return0, 0, 0, 0),
            "GetGlobal" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let k = self.parse_u8()?;
                encode_a(OpCode::GetGlobal, a, k, 0)
            }
            "SetGlobal" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let k = self.parse_u8()?;
                encode_a(OpCode::SetGlobal, a, k, 0)
            }
            "GetGlobalIdx" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let idx = self.parse_i16()?;
                encode_b(OpCode::GetGlobalIdx, a, idx)
            }
            "SetGlobalIdx" => {
                let idx = self.parse_i16()?;
                self.skip_comma()?;
                let a = self.parse_register()?;
                encode_b(OpCode::SetGlobalIdx, a, idx)
            }
            "GetGlobalIdxWide" => {
                let register = self.parse_register()?;
                self.skip_comma()?;
                let index = self.parse_u32()?;
                extension_words.push(index);
                encode_a(OpCode::GetGlobalIdxWide, register, 0, 0)
            }
            "SetGlobalIdxWide" => {
                let index = self.parse_u32()?;
                self.skip_comma()?;
                let register = self.parse_register()?;
                extension_words.push(index);
                encode_a(OpCode::SetGlobalIdxWide, register, 0, 0)
            }
            "MakeClosure" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                // Handle 'kN' format (e.g., k0, k1) where N is the constant index
                let k = if let Token::Ident(s) = &self.current {
                    if let Some(num_str) = s.strip_prefix('k') {
                        let k = num_str.parse::<u8>().map_err(|_| {
                            AssemblerError::InvalidNumber(format!("Invalid constant index: {}", s))
                        })?;
                        self.advance()?;
                        k
                    } else {
                        self.parse_u8()?
                    }
                } else {
                    self.parse_u8()?
                };
                self.skip_comma()?;
                let upval_count = self.parse_u8()?;
                encode_a(OpCode::MakeClosure, a, k, upval_count)
            }
            "MakeClosureWide" => {
                let register = self.parse_register()?;
                self.skip_comma()?;
                let index = self.parse_u32()?;
                self.skip_comma()?;
                let upvalue_count = self.parse_u8()?;
                extension_words.push(index);
                encode_a(OpCode::MakeClosureWide, register, upvalue_count, 0)
            }
            "MakeClosureRegisterWide" => {
                let register = self.parse_wide_register()?;
                self.skip_comma()?;
                let index = self.parse_u32()?;
                self.skip_comma()?;
                let upvalue_count = self.parse_u16()?;
                extension_words.push((u32::from(register) << 16) | u32::from(upvalue_count));
                extension_words.push(index);
                encode_a(OpCode::MakeClosureRegisterWide, 0, 0, 0)
            }
            "GetUpval" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                // Parse 'upval[N]' format
                let idx = self.parse_upval_index()?;
                encode_a(OpCode::GetUpval, a, idx, 0)
            }
            "SetUpval" => {
                // Parse 'upval[N]' format
                let idx = self.parse_upval_index()?;
                self.skip_comma()?;
                let src = self.parse_register()?;
                encode_a(OpCode::SetUpval, idx, src, 0)
            }
            "CloseUpvals" => {
                let a = self.parse_register()?;
                encode_a(OpCode::CloseUpvals, a, 0, 0)
            }
            "ForLoopI" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let offset = self.parse_i16()?;
                // Skip any trailing comment (iter+=step; ...)
                while self.current != Token::Newline && self.current != Token::Eof {
                    self.advance()?;
                }
                encode_b(OpCode::ForLoopI, a, offset)
            }
            "ForLoopIInc" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let offset = self.parse_i16()?;
                // Skip any trailing comment
                while self.current != Token::Newline && self.current != Token::Eof {
                    self.advance()?;
                }
                encode_b(OpCode::ForLoopIInc, a, offset)
            }
            // New immediate opcodes
            "AddI" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let b = self.parse_register()?;
                self.skip_comma()?;
                let c = self.parse_u8()?;
                encode_a(OpCode::AddI, a, b, c)
            }
            "SubI" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let b = self.parse_register()?;
                self.skip_comma()?;
                let c = self.parse_u8()?;
                encode_a(OpCode::SubI, a, b, c)
            }
            "LtImm" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let imm = self.parse_i16()?;
                encode_b(OpCode::LtImm, a, imm)
            }
            "LeImm" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let imm = self.parse_i16()?;
                encode_b(OpCode::LeImm, a, imm)
            }
            "GtImm" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let imm = self.parse_i16()?;
                encode_b(OpCode::GtImm, a, imm)
            }
            "GeImm" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let imm = self.parse_i16()?;
                encode_b(OpCode::GeImm, a, imm)
            }
            "WhileLoopLt" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let offset = self.parse_i16()?;
                // Skip any trailing comment
                while self.current != Token::Newline && self.current != Token::Eof {
                    self.advance()?;
                }
                encode_b(OpCode::WhileLoopLt, a, offset)
            }
            // Type-specialized integer opcodes
            "AddII" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let b = self.parse_register()?;
                self.skip_comma()?;
                let c = self.parse_register()?;
                encode_a(OpCode::AddII, a, b, c)
            }
            "SubII" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let b = self.parse_register()?;
                self.skip_comma()?;
                let c = self.parse_register()?;
                encode_a(OpCode::SubII, a, b, c)
            }
            "MulII" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let b = self.parse_register()?;
                self.skip_comma()?;
                let c = self.parse_register()?;
                encode_a(OpCode::MulII, a, b, c)
            }
            "DivII" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let b = self.parse_register()?;
                self.skip_comma()?;
                let c = self.parse_register()?;
                encode_a(OpCode::DivII, a, b, c)
            }
            "ModII" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let b = self.parse_register()?;
                self.skip_comma()?;
                let c = self.parse_register()?;
                encode_a(OpCode::ModII, a, b, c)
            }
            // Type-specialized float opcodes
            "AddFF" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let b = self.parse_register()?;
                self.skip_comma()?;
                let c = self.parse_register()?;
                encode_a(OpCode::AddFF, a, b, c)
            }
            "SubFF" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let b = self.parse_register()?;
                self.skip_comma()?;
                let c = self.parse_register()?;
                encode_a(OpCode::SubFF, a, b, c)
            }
            "MulFF" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let b = self.parse_register()?;
                self.skip_comma()?;
                let c = self.parse_register()?;
                encode_a(OpCode::MulFF, a, b, c)
            }
            "DivFF" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let b = self.parse_register()?;
                self.skip_comma()?;
                let c = self.parse_register()?;
                encode_a(OpCode::DivFF, a, b, c)
            }
            "ModFF" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let b = self.parse_register()?;
                self.skip_comma()?;
                let c = self.parse_register()?;
                encode_a(OpCode::ModFF, a, b, c)
            }
            // Integer comparisons
            "LtII" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let b = self.parse_register()?;
                self.skip_comma()?;
                let c = self.parse_register()?;
                encode_a(OpCode::LtII, a, b, c)
            }
            "LeII" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let b = self.parse_register()?;
                self.skip_comma()?;
                let c = self.parse_register()?;
                encode_a(OpCode::LeII, a, b, c)
            }
            "GtII" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let b = self.parse_register()?;
                self.skip_comma()?;
                let c = self.parse_register()?;
                encode_a(OpCode::GtII, a, b, c)
            }
            "GeII" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let b = self.parse_register()?;
                self.skip_comma()?;
                let c = self.parse_register()?;
                encode_a(OpCode::GeII, a, b, c)
            }
            "EqII" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let b = self.parse_register()?;
                self.skip_comma()?;
                let c = self.parse_register()?;
                encode_a(OpCode::EqII, a, b, c)
            }
            "NeII" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let b = self.parse_register()?;
                self.skip_comma()?;
                let c = self.parse_register()?;
                encode_a(OpCode::NeII, a, b, c)
            }
            // Float comparisons
            "LtFF" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let b = self.parse_register()?;
                self.skip_comma()?;
                let c = self.parse_register()?;
                encode_a(OpCode::LtFF, a, b, c)
            }
            "LeFF" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let b = self.parse_register()?;
                self.skip_comma()?;
                let c = self.parse_register()?;
                encode_a(OpCode::LeFF, a, b, c)
            }
            "GtFF" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let b = self.parse_register()?;
                self.skip_comma()?;
                let c = self.parse_register()?;
                encode_a(OpCode::GtFF, a, b, c)
            }
            "GeFF" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let b = self.parse_register()?;
                self.skip_comma()?;
                let c = self.parse_register()?;
                encode_a(OpCode::GeFF, a, b, c)
            }
            "EqFF" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let b = self.parse_register()?;
                self.skip_comma()?;
                let c = self.parse_register()?;
                encode_a(OpCode::EqFF, a, b, c)
            }
            "NeFF" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let b = self.parse_register()?;
                self.skip_comma()?;
                let c = self.parse_register()?;
                encode_a(OpCode::NeFF, a, b, c)
            }
            // Integer immediate comparisons
            "LtIImm" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let b = self.parse_register()?;
                self.skip_comma()?;
                let c = self.parse_u8()?;
                encode_a(OpCode::LtIImm, a, b, c)
            }
            "LeIImm" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let b = self.parse_register()?;
                self.skip_comma()?;
                let c = self.parse_u8()?;
                encode_a(OpCode::LeIImm, a, b, c)
            }
            "GtIImm" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let b = self.parse_register()?;
                self.skip_comma()?;
                let c = self.parse_u8()?;
                encode_a(OpCode::GtIImm, a, b, c)
            }
            "GeIImm" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let b = self.parse_register()?;
                self.skip_comma()?;
                let c = self.parse_u8()?;
                encode_a(OpCode::GeIImm, a, b, c)
            }
            "CallCached" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let b = self.parse_register()?;
                self.skip_comma()?;
                let c = self.parse_u8()?;
                encode_a(OpCode::CallCached, a, b, c)
            }
            "CallGlobal" => {
                let dest = self.parse_register()?;
                self.skip_comma()?;
                let global_idx = self.parse_u8()?;
                self.skip_comma()?;
                let nargs = self.parse_u8()?;
                encode_a(OpCode::CallGlobal, dest, global_idx, nargs)
            }
            "CallUpval" => {
                // Format: CallUpval r<dest>, upval[N], <nargs>
                let dest = self.parse_register()?;
                self.skip_comma()?;
                let upval_idx = self.parse_upval_index()?;
                self.skip_comma()?;
                let nargs = self.parse_u8()?;
                encode_a(OpCode::CallUpval, dest, upval_idx, nargs)
            }
            "TailCallUpval" => {
                // Format: TailCallUpval r<dest>, upval[N], <nargs>
                let dest = self.parse_register()?;
                self.skip_comma()?;
                let upval_idx = self.parse_upval_index()?;
                self.skip_comma()?;
                let nargs = self.parse_u8()?;
                encode_a(OpCode::TailCallUpval, dest, upval_idx, nargs)
            }
            "AddGlobalI" => {
                let dest = self.parse_register()?;
                self.skip_comma()?;
                let source = self.parse_register()?;
                self.skip_comma()?;
                let global = self.parse_u8()?;
                encode_a(OpCode::AddGlobalI, dest, source, global)
            }
            "AddIIG" => self.parse_ternary_reg(OpCode::AddIIG)?,
            "SubIIG" => self.parse_ternary_reg(OpCode::SubIIG)?,
            "MulIIG" => self.parse_ternary_reg(OpCode::MulIIG)?,
            "DivIIG" => self.parse_ternary_reg(OpCode::DivIIG)?,
            "ModIIG" => self.parse_ternary_reg(OpCode::ModIIG)?,
            "AddFFG" => self.parse_ternary_reg(OpCode::AddFFG)?,
            "SubFFG" => self.parse_ternary_reg(OpCode::SubFFG)?,
            "MulFFG" => self.parse_ternary_reg(OpCode::MulFFG)?,
            "DivFFG" => self.parse_ternary_reg(OpCode::DivFFG)?,
            "ModFFG" => self.parse_ternary_reg(OpCode::ModFFG)?,
            "LtIIG" => self.parse_ternary_reg(OpCode::LtIIG)?,
            "LeIIG" => self.parse_ternary_reg(OpCode::LeIIG)?,
            "GtIIG" => self.parse_ternary_reg(OpCode::GtIIG)?,
            "GeIIG" => self.parse_ternary_reg(OpCode::GeIIG)?,
            "EqIIG" => self.parse_ternary_reg(OpCode::EqIIG)?,
            "NeIIG" => self.parse_ternary_reg(OpCode::NeIIG)?,
            "LtFFG" => self.parse_ternary_reg(OpCode::LtFFG)?,
            "LeFFG" => self.parse_ternary_reg(OpCode::LeFFG)?,
            "GtFFG" => self.parse_ternary_reg(OpCode::GtFFG)?,
            "GeFFG" => self.parse_ternary_reg(OpCode::GeFFG)?,
            "EqFFG" => self.parse_ternary_reg(OpCode::EqFFG)?,
            "NeFFG" => self.parse_ternary_reg(OpCode::NeFFG)?,
            "Shl" => self.parse_ternary_reg(OpCode::Shl)?,
            "Shr" => self.parse_ternary_reg(OpCode::Shr)?,
            "BitAnd" => self.parse_ternary_reg(OpCode::BitAnd)?,
            "BitOr" => self.parse_ternary_reg(OpCode::BitOr)?,
            "BitXor" => self.parse_ternary_reg(OpCode::BitXor)?,
            "BitNot" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let b = self.parse_register()?;
                encode_a(OpCode::BitNot, a, b, 0)
            }
            "ShlII" => self.parse_ternary_reg(OpCode::ShlII)?,
            "ShrII" => self.parse_ternary_reg(OpCode::ShrII)?,
            "AndII" => self.parse_ternary_reg(OpCode::AndII)?,
            "OrII" => self.parse_ternary_reg(OpCode::OrII)?,
            "XorII" => self.parse_ternary_reg(OpCode::XorII)?,
            "NotI" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let b = self.parse_register()?;
                encode_a(OpCode::NotI, a, b, 0)
            }
            "ShlIImm" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let b = self.parse_register()?;
                self.skip_comma()?;
                let c = self.parse_u8()?;
                encode_a(OpCode::ShlIImm, a, b, c)
            }
            "ShrIImm" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let b = self.parse_register()?;
                self.skip_comma()?;
                let c = self.parse_u8()?;
                encode_a(OpCode::ShrIImm, a, b, c)
            }
            "AndIImm" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let b = self.parse_register()?;
                self.skip_comma()?;
                let c = self.parse_u8()?;
                encode_a(OpCode::AndIImm, a, b, c)
            }
            "OrIImm" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let b = self.parse_register()?;
                self.skip_comma()?;
                let c = self.parse_u8()?;
                encode_a(OpCode::OrIImm, a, b, c)
            }
            "XorIImm" => {
                let a = self.parse_register()?;
                self.skip_comma()?;
                let b = self.parse_register()?;
                self.skip_comma()?;
                let c = self.parse_u8()?;
                encode_a(OpCode::XorIImm, a, b, c)
            }
            "Wide" => {
                let inner = self.parse_u8()?;
                self.skip_comma()?;
                let a = self.parse_wide_register()?;
                self.skip_comma()?;
                let b = self.parse_wide_register()?;
                self.skip_comma()?;
                let c = self.parse_wide_register()?;
                extension_words.extend(encode_wide_operands(a, b, c));
                encode_a(OpCode::Wide, inner, 0, 0)
            }
            "MoveWide" => {
                let dest = self.parse_wide_register()?;
                self.skip_comma()?;
                let source = self.parse_wide_register()?;
                extension_words.extend(encode_wide_operands(dest, source, 0));
                encode_a(OpCode::Wide, u8::from(OpCode::Move), 0, 0)
            }
            "LoadNullWide" => {
                let dest = self.parse_wide_register()?;
                extension_words.extend(encode_wide_operands(dest, 0, 0));
                encode_a(OpCode::Wide, u8::from(OpCode::LoadNull), 0, 0)
            }
            "AddWide" => {
                let dest = self.parse_wide_register()?;
                self.skip_comma()?;
                let left = self.parse_wide_register()?;
                self.skip_comma()?;
                let right = self.parse_wide_register()?;
                extension_words.extend(encode_wide_operands(dest, left, right));
                encode_a(OpCode::Wide, u8::from(OpCode::Add), 0, 0)
            }
            "ReturnWide" => {
                let register = self.parse_wide_register()?;
                extension_words.extend(encode_wide_operands(register, 0, 0));
                encode_a(OpCode::Wide, u8::from(OpCode::Return), 0, 0)
            }
            _ => return Err(AssemblerError::UnknownOpcode(opcode_name)),
        };

        bytecode.push(instr);
        bytecode.extend(extension_words);
        Ok(())
    }

    fn parse_ternary_reg(&mut self, op: OpCode) -> Result<u32> {
        let a = self.parse_register()?;
        self.skip_comma()?;
        let b = self.parse_register()?;
        self.skip_comma()?;
        let c = self.parse_register()?;
        Ok(encode_a(op, a, b, c))
    }

    fn parse_tagged_ternary(&mut self, op: OpCode) -> Result<u32> {
        let a = self.parse_register()?;
        self.skip_comma()?;
        let b = self.parse_register()?;
        self.skip_comma()?;
        let tag = self.parse_u8()?;
        Ok(encode_a(op, a, b, tag))
    }

    fn parse_jump_target(&mut self) -> Result<(i16, Option<String>)> {
        match &self.current {
            Token::LabelRef(name) => {
                let label = name.clone();
                self.advance()?;
                Ok((0, Some(label)))
            }
            Token::At => {
                self.advance()?;
                if let Token::Int(n) = self.advance()? {
                    let offset = i16::try_from(n).map_err(|_| {
                        AssemblerError::InvalidNumber(format!("Jump offset is out of range: {n}"))
                    })?;
                    Ok((offset, None))
                } else {
                    Err(AssemblerError::Expected {
                        expected: "offset".to_string(),
                        got: format!("{:?}", self.current),
                    })
                }
            }
            Token::Int(n) => {
                let offset = *n;
                self.advance()?;
                let offset = i16::try_from(offset).map_err(|_| {
                    AssemblerError::InvalidNumber(format!("Jump offset is out of range: {offset}"))
                })?;
                Ok((offset, None))
            }
            _ => Err(AssemblerError::Expected {
                expected: "label or offset".to_string(),
                got: format!("{:?}", self.current),
            }),
        }
    }

    fn parse_long_jump_target(&mut self) -> Result<(i32, Option<String>)> {
        match &self.current {
            Token::LabelRef(name) => {
                let label = name.clone();
                self.advance()?;
                Ok((0, Some(label)))
            }
            Token::At => {
                self.advance()?;
                if let Token::Int(n) = self.advance()? {
                    let offset = i32::try_from(n).map_err(|_| {
                        AssemblerError::InvalidNumber(format!(
                            "Long jump offset is out of range: {n}"
                        ))
                    })?;
                    Ok((offset, None))
                } else {
                    Err(AssemblerError::Expected {
                        expected: "offset".to_string(),
                        got: format!("{:?}", self.current),
                    })
                }
            }
            Token::Int(n) => {
                let offset = *n;
                self.advance()?;
                let offset = i32::try_from(offset).map_err(|_| {
                    AssemblerError::InvalidNumber(format!(
                        "Long jump offset is out of range: {offset}"
                    ))
                })?;
                Ok((offset, None))
            }
            _ => Err(AssemblerError::Expected {
                expected: "label or offset".to_string(),
                got: format!("{:?}", self.current),
            }),
        }
    }

    /// Parse 'upval[N]' format and return N
    pub(super) fn parse_upval_index(&mut self) -> Result<u8> {
        // Expect 'upval' identifier
        if let Token::Ident(s) = &self.current {
            if s != "upval" {
                return Err(AssemblerError::Expected {
                    expected: "upval".to_string(),
                    got: format!("{:?}", self.current),
                });
            }
            self.advance()?;
        } else {
            return Err(AssemblerError::Expected {
                expected: "upval".to_string(),
                got: format!("{:?}", self.current),
            });
        }

        // Expect '['
        if self.current != Token::LBracket {
            return Err(AssemblerError::Expected {
                expected: "[".to_string(),
                got: format!("{:?}", self.current),
            });
        }
        self.advance()?;

        // Parse the index
        let idx = self.parse_u8()?;

        // Expect ']'
        if self.current != Token::RBracket {
            return Err(AssemblerError::Expected {
                expected: "]".to_string(),
                got: format!("{:?}", self.current),
            });
        }
        self.advance()?;

        Ok(idx)
    }
}

/// Encode a Format A instruction
pub(super) fn encode_a(op: OpCode, a: u8, b: u8, c: u8) -> u32 {
    (u32::from(u8::from(op)) << 24) | (u32::from(a) << 16) | (u32::from(b) << 8) | u32::from(c)
}

/// Encode a Format B instruction
pub(super) fn encode_b(op: OpCode, a: u8, imm: i16) -> u32 {
    let immediate = u16::from_ne_bytes(imm.to_ne_bytes());
    (u32::from(u8::from(op)) << 24) | (u32::from(a) << 16) | u32::from(immediate)
}

fn encode_wide_operands(a: u16, b: u16, c: u16) -> [u32; 2] {
    [(u32::from(a) << 16) | u32::from(b), u32::from(c) << 16]
}
