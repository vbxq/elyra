
use crate::bytecode::{Constant, Function, OpCode, decode_a, decode_b};
use std::collections::{HashMap, HashSet};
use std::fmt::Write;

macro_rules! writeln_ignore {
    ($dst:expr) => { let _ = writeln!($dst); };
    ($dst:expr, $($arg:tt)*) => { let _ = writeln!($dst, $($arg)*); };
}

#[derive(Debug, Clone, Default)]
pub struct DisassemblerOptions {
    pub include_line_info: bool,
}

pub fn disassemble(func: &Function) -> String {
    disassemble_with_options(func, &DisassemblerOptions::default())
}

pub fn disassemble_with_options(func: &Function, options: &DisassemblerOptions) -> String {
    let mut output = String::new();
    let mut ctx = DisasmContext::new(options);

    writeln_ignore!(output, "; Aelys Assembly (.aasm)");
    writeln_ignore!(output, "; Disassembled from bytecode");
    writeln_ignore!(output);
    writeln_ignore!(output, ".version 3");
    writeln_ignore!(output);

    let mut all_functions = Vec::new();
    collect_functions(func, &mut all_functions);

    for (idx, f) in all_functions.iter().enumerate() {
        if idx > 0 {
            writeln_ignore!(output);
        }
        ctx.set_function_context(f, &all_functions);
        ctx.disassemble_function(&mut output, f, idx);
    }

    output
}

pub fn disassemble_to_string(func: &Function) -> String {
    disassemble(func)
}

fn collect_functions<'a>(func: &'a Function, out: &mut Vec<&'a Function>) {
    out.push(func);
    for f in &func.nested_functions {
        collect_functions(f, out);
    }
}

struct DisasmContext<'a> {
    options: &'a DisassemblerOptions,
    global_names: Vec<String>,
    nested_fn_names: Vec<Option<String>>,
}

impl<'a> DisasmContext<'a> {
    fn new(options: &'a DisassemblerOptions) -> Self {
        Self {
            options,
            global_names: Vec::new(),
            nested_fn_names: Vec::new(),
        }
    }

    fn set_function_context(&mut self, func: &Function, all_functions: &[&Function]) {
        self.global_names = func.global_layout.names().to_vec();
        self.nested_fn_names = func
            .nested_functions
            .iter()
            .map(|f| f.name.clone())
            .collect();
        if self.nested_fn_names.is_empty() {
            self.nested_fn_names = all_functions
                .iter()
                .skip(1)
                .map(|f| f.name.clone())
                .collect();
        }
    }

    fn global_name(&self, idx: usize) -> Option<&str> {
        self.global_names.get(idx).map(|s| s.as_str())
    }

    fn nested_fn_name(&self, idx: usize) -> Option<&str> {
        self.nested_fn_names.get(idx).and_then(|o| o.as_deref())
    }

    fn disassemble_function(&self, output: &mut String, func: &Function, func_idx: usize) {
        writeln_ignore!(output, "; !== FUN {}", func_idx);
        writeln_ignore!(output, ".function {}", func_idx);

        if let Some(name) = &func.name {
            writeln_ignore!(output, "  .name \"{}\"", escape_string(name));
        }
        writeln_ignore!(output, "  .arity {}", func.arity);
        writeln_ignore!(output, "  .registers {}", func.num_registers);

        if !func.global_layout.names().is_empty() {
            writeln_ignore!(output);
            writeln_ignore!(output, "  .globals");
            for (idx, name) in func.global_layout.names().iter().enumerate() {
                writeln_ignore!(output, "    {}: \"{}\"", idx, escape_string(name));
            }
        }
        writeln_ignore!(output);

        if !func.constants.is_empty() {
            writeln_ignore!(output, "  .constants");
            for (idx, constant) in func.constants.iter().enumerate() {
                let const_str = self.format_constant(constant, &func.nested_functions);
                writeln_ignore!(output, "    {}: {}", idx, const_str);
            }
            writeln_ignore!(output);
        }

        if !func.upvalue_descriptors.is_empty() {
            writeln_ignore!(output, "  .upvalues");
            for (idx, desc) in func.upvalue_descriptors.iter().enumerate() {
                let kind = if desc.is_local { "local" } else { "upvalue" };
                writeln_ignore!(output, "    {}: {} {}", idx, kind, desc.index);
            }
            writeln_ignore!(output);
        }

        if !func.struct_schemas.is_empty() {
            writeln_ignore!(output, "  .schemas");
            for (schema_index, schema) in func.struct_schemas.iter().enumerate() {
                writeln_ignore!(
                    output,
                    "    {}: \"{}\" {} {} {}{}",
                    schema_index,
                    escape_string(&schema.display_name()),
                    schema.ctor.ordinal,
                    schema.type_args.len(),
                    schema.fields.len(),
                    schema
                        .type_args
                        .iter()
                        .map(|ty| format!(" \"{}\"", escape_string(&ty.to_string())))
                        .collect::<String>()
                );
                for (field_index, field) in schema.fields.iter().enumerate() {
                    writeln_ignore!(
                        output,
                        "      {}: \"{}\" \"{}\"",
                        field_index,
                        escape_string(&field.name),
                        escape_string(&field.ty.to_string())
                    );
                }
            }
            writeln_ignore!(output);
        }

        if !func.enum_schemas.is_empty() {
            writeln_ignore!(output, "  .enum_schemas");
            for (schema_index, schema) in func.enum_schemas.iter().enumerate() {
                writeln_ignore!(
                    output,
                    "    {}: \"{}\" \"{}\" {} {} {}{}",
                    schema_index,
                    escape_string(&schema.name),
                    escape_string(&schema.def_id.display_name()),
                    schema.def_id.ordinal,
                    schema.arity,
                    schema.variants.len(),
                    schema
                        .type_args
                        .iter()
                        .map(|ty| format!(" \"{}\"", escape_string(&ty.to_string())))
                        .collect::<String>()
                );
                for (variant_index, variant) in schema.variants.iter().enumerate() {
                    writeln_ignore!(
                        output,
                        "      {}: \"{}\" {}",
                        variant_index,
                        escape_string(&variant.name),
                        variant.fields.len()
                    );
                    for (field_index, field) in variant.fields.iter().enumerate() {
                        let field_name = field.name.as_deref().unwrap_or("");
                        writeln_ignore!(
                            output,
                            "        {}: \"{}\" \"{}\"",
                            field_index,
                            escape_string(field_name),
                            escape_string(&field.ty.to_string())
                        );
                    }
                }
            }
            writeln_ignore!(output);
        }

        if func.jit_unsupported_struct {
            writeln_ignore!(output, "  .jit_unsupported_struct true");
            writeln_ignore!(output);
        }

        if func.bytecode.is_empty() {
            writeln_ignore!(output, "  .code");
            writeln_ignore!(output, "    ; (empty)");
            return;
        }

        let labels = self.collect_jump_targets(func.bytecode.as_slice());

        writeln_ignore!(output, "  .code");
        let mut skip_extension_words = 0usize;
        for (offset, &instr) in func.bytecode.iter().enumerate() {
            if skip_extension_words > 0 {
                skip_extension_words -= 1;
                continue;
            }

            if let Some(label) = labels.get(&offset) {
                writeln_ignore!(output, "  {}:", label);
            }

            let opcode =
                OpCode::from_u8(u8::try_from(instr >> 24).expect("opcode occupies one byte"));
            skip_extension_words = opcode.map_or(0, OpCode::extension_words);
            let extension = (skip_extension_words >= 1)
                .then(|| func.bytecode.as_slice().get(offset + 1).copied())
                .flatten();
            let second_extension = (skip_extension_words >= 2)
                .then(|| func.bytecode.as_slice().get(offset + 2).copied())
                .flatten();
            let disasm =
                self.disassemble_instruction(instr, extension, second_extension, offset, &labels);

            if self.options.include_line_info {
                let line = func.get_line(offset);
                if line > 0 {
                    writeln_ignore!(output, "    {:04}: {:30} ; line {}", offset, disasm, line);
                } else {
                    writeln_ignore!(output, "    {:04}: {}", offset, disasm);
                }
            } else {
                writeln_ignore!(output, "    {:04}: {}", offset, disasm);
            }
        }
    }

    fn collect_jump_targets(&self, bytecode: &[u32]) -> HashMap<usize, String> {
        let mut targets = HashSet::new();

        let mut offset = 0;
        while offset < bytecode.len() {
            let instr = bytecode[offset];
            let opcode =
                OpCode::from_u8(u8::try_from(instr >> 24).expect("opcode occupies one byte"));

            if let Some(OpCode::Jump | OpCode::JumpIf | OpCode::JumpIfNot) = opcode {
                let (_, _, imm) = decode_b(instr);
                let target = if imm >= 0 {
                    offset.wrapping_add(1).wrapping_add(imm as usize)
                } else {
                    offset.wrapping_add(1).wrapping_sub((-imm) as usize)
                };
                targets.insert(target);
            } else if opcode
                .is_some_and(|op| op.format() == crate::bytecode::InstructionFormat::AOffset32)
                && let Some(extension) = bytecode.get(offset + 1)
            {
                let relative = i32::from_ne_bytes(extension.to_ne_bytes());
                let target = (offset as i64 + 2 + i64::from(relative)) as usize;
                targets.insert(target);
            } else if opcode.is_some_and(|op| {
                matches!(
                    op.format(),
                    crate::bytecode::InstructionFormat::RegisterOffset32
                        | crate::bytecode::InstructionFormat::WideRegisterOffset32
                )
            }) && let Some(extension) = bytecode.get(offset + 2)
            {
                let relative = i32::from_ne_bytes(extension.to_ne_bytes());
                let target = (offset as i64 + 3 + i64::from(relative)) as usize;
                targets.insert(target);
            }
            offset += 1 + opcode.map_or(0, OpCode::extension_words);
        }

        let mut sorted_targets: Vec<_> = targets.into_iter().collect();
        sorted_targets.sort();

        let mut labels = HashMap::new();
        for (idx, target) in sorted_targets.into_iter().enumerate() {
            labels.insert(target, format!("L{}", idx));
        }

        labels
    }

    fn disassemble_instruction(
        &self,
        instr: u32,
        extension: Option<u32>,
        second_extension: Option<u32>,
        offset: usize,
        labels: &HashMap<usize, String>,
    ) -> String {
        let opcode_byte = u8::try_from(instr >> 24).expect("opcode occupies one byte");
        let opcode = match OpCode::from_u8(opcode_byte) {
            Some(op) => op,
            None => return format!(".word 0x{:08x}", instr),
        };

        match opcode {
            OpCode::Move => {
                let (_, a, b, _) = decode_a(instr);
                format!("Move      r{}, r{}", a, b)
            }
            OpCode::LoadNull => {
                let (_, a, _, _) = decode_a(instr);
                format!("LoadNull  r{}", a)
            }
            OpCode::LoadUnit => {
                let (_, a, _, _) = decode_a(instr);
                format!("LoadUnit   r{}", a)
            }
            OpCode::LoadNone => {
                let (_, a, _, _) = decode_a(instr);
                format!("LoadNone   r{}", a)
            }
            OpCode::LoadBool => {
                let (_, a, b, _) = decode_a(instr);
                format!("LoadBool  r{}, {}", a, b != 0)
            }
            OpCode::Add => {
                let (_, a, b, c) = decode_a(instr);
                format!("Add       r{}, r{}, r{}", a, b, c)
            }
            OpCode::Sub => {
                let (_, a, b, c) = decode_a(instr);
                format!("Sub       r{}, r{}, r{}", a, b, c)
            }
            OpCode::Mul => {
                let (_, a, b, c) = decode_a(instr);
                format!("Mul       r{}, r{}, r{}", a, b, c)
            }
            OpCode::Div => {
                let (_, a, b, c) = decode_a(instr);
                format!("Div       r{}, r{}, r{}", a, b, c)
            }
            OpCode::Mod => {
                let (_, a, b, c) = decode_a(instr);
                format!("Mod       r{}, r{}, r{}", a, b, c)
            }
            OpCode::Neg => {
                let (_, a, b, _) = decode_a(instr);
                format!("Neg       r{}, r{}", a, b)
            }
            OpCode::Eq => {
                let (_, a, b, c) = decode_a(instr);
                format!("Eq        r{}, r{}, r{}", a, b, c)
            }
            OpCode::Ne => {
                let (_, a, b, c) = decode_a(instr);
                format!("Ne        r{}, r{}, r{}", a, b, c)
            }
            OpCode::Lt => {
                let (_, a, b, c) = decode_a(instr);
                format!("Lt        r{}, r{}, r{}", a, b, c)
            }
            OpCode::Le => {
                let (_, a, b, c) = decode_a(instr);
                format!("Le        r{}, r{}, r{}", a, b, c)
            }
            OpCode::Gt => {
                let (_, a, b, c) = decode_a(instr);
                format!("Gt        r{}, r{}, r{}", a, b, c)
            }
            OpCode::Ge => {
                let (_, a, b, c) = decode_a(instr);
                format!("Ge        r{}, r{}, r{}", a, b, c)
            }
            OpCode::Not => {
                let (_, a, b, _) = decode_a(instr);
                format!("Not       r{}, r{}", a, b)
            }
            OpCode::JumpLong
            | OpCode::JumpIfLong
            | OpCode::JumpIfNotLong
            | OpCode::ForLoopILong
            | OpCode::ForLoopIIncLong
            | OpCode::StringForLoopLong
            | OpCode::VecForLoopLong
            | OpCode::ArrayForLoopLong => {
                let (_, register, _, _) = decode_a(instr);
                let relative = i32::from_ne_bytes(extension.unwrap_or(0).to_ne_bytes());
                let target = (offset as i64 + 2 + i64::from(relative)) as usize;
                let target = labels
                    .get(&target)
                    .cloned()
                    .unwrap_or_else(|| format!("@{}", target));
                match opcode {
                    OpCode::JumpLong => format!("JumpLong  {}", target),
                    OpCode::JumpIfLong => format!("JumpIfLong r{}, {}", register, target),
                    OpCode::JumpIfNotLong => {
                        format!("JumpIfNotLong r{}, {}", register, target)
                    }
                    OpCode::ForLoopILong => format!("ForLoopILong r{}, {}", register, target),
                    OpCode::ForLoopIIncLong => {
                        format!("ForLoopIIncLong r{}, {}", register, target)
                    }
                    OpCode::StringForLoopLong => {
                        format!("StringForLoopLong r{}, {}", register, target)
                    }
                    OpCode::VecForLoopLong => {
                        format!("VecForLoopLong r{}, {}", register, target)
                    }
                    OpCode::ArrayForLoopLong => {
                        format!("ArrayForLoopLong r{}, {}", register, target)
                    }
                    _ => unreachable!(),
                }
            }
            OpCode::JumpIfWideLong | OpCode::JumpIfNotWideLong => {
                let register = extension.unwrap_or(0) >> 16;
                let relative = i32::from_ne_bytes(second_extension.unwrap_or(0).to_ne_bytes());
                let target = (offset as i64 + 3 + i64::from(relative)) as usize;
                let target = labels
                    .get(&target)
                    .cloned()
                    .unwrap_or_else(|| format!("@{target}"));
                if opcode == OpCode::JumpIfWideLong {
                    format!("JumpIfWideLong r{register}, {target}")
                } else {
                    format!("JumpIfNotWideLong r{register}, {target}")
                }
            }
            OpCode::LoopWideLong => {
                let (_, inner, _, _) = decode_a(instr);
                let inner = match OpCode::from_u8(inner) {
                    Some(op) => format!("{op:?}"),
                    None => format!("<invalid {inner}>"),
                };
                let register = extension.unwrap_or(0) >> 16;
                let relative = i32::from_ne_bytes(second_extension.unwrap_or(0).to_ne_bytes());
                let target = (offset as i64 + 3 + i64::from(relative)) as usize;
                let target = labels
                    .get(&target)
                    .cloned()
                    .unwrap_or_else(|| format!("@{target}"));
                format!("LoopWideLong {inner}, r{register}, {target}")
            }
            OpCode::Call => {
                let (_, dest, func, nargs) = decode_a(instr);
                format!("Call      r{}, r{}, {}", dest, func, nargs)
            }
            OpCode::CallWide => {
                let first = extension.unwrap_or(0);
                let second = second_extension.unwrap_or(0);
                let dest = first >> 16;
                let func = first & 0xffff;
                let nargs = second >> 16;
                format!("CallWide  r{dest}, r{func}, {nargs}")
            }
            OpCode::Return => {
                let (_, a, _, _) = decode_a(instr);
                format!("Return    r{}", a)
            }
            OpCode::Return0 => "Return0".to_string(),
            OpCode::GetGlobal => {
                let (_, a, k, _) = decode_a(instr);
                format!("GetGlobal r{}, {}", a, k)
            }
            OpCode::SetGlobal => {
                let (_, a, k, _) = decode_a(instr);
                format!("SetGlobal r{}, {}", a, k)
            }
            OpCode::LoadI => {
                let (_, a, imm) = decode_b(instr);
                format!("LoadI     r{}, {}", a, imm)
            }
            OpCode::LoadK => {
                let (_, a, k) = decode_b(instr);
                format!("LoadK     r{}, {}", a, k)
            }
            OpCode::LoadKWide => {
                let (_, a, _, _) = decode_a(instr);
                format!("LoadKWide r{}, {}", a, extension.unwrap_or(0))
            }
            OpCode::GetGlobalIdxWide => {
                let (_, register, _, _) = decode_a(instr);
                format!("GetGlobalIdxWide r{}, {}", register, extension.unwrap_or(0))
            }
            OpCode::SetGlobalIdxWide => {
                let (_, register, _, _) = decode_a(instr);
                format!("SetGlobalIdxWide {}, r{}", extension.unwrap_or(0), register)
            }
            OpCode::Jump => {
                let (_, _, imm) = decode_b(instr);
                let target = if imm >= 0 {
                    offset.wrapping_add(1).wrapping_add(imm as usize)
                } else {
                    offset.wrapping_add(1).wrapping_sub((-imm) as usize)
                };
                if let Some(label) = labels.get(&target) {
                    format!("Jump      {}", label)
                } else {
                    format!("Jump      @{}", target)
                }
            }
            OpCode::JumpIf => {
                let (_, a, imm) = decode_b(instr);
                let target = if imm >= 0 {
                    offset.wrapping_add(1).wrapping_add(imm as usize)
                } else {
                    offset.wrapping_add(1).wrapping_sub((-imm) as usize)
                };
                if let Some(label) = labels.get(&target) {
                    format!("JumpIf    r{}, {}", a, label)
                } else {
                    format!("JumpIf    r{}, @{}", a, target)
                }
            }
            OpCode::JumpIfNot => {
                let (_, a, imm) = decode_b(instr);
                let target = if imm >= 0 {
                    offset.wrapping_add(1).wrapping_add(imm as usize)
                } else {
                    offset.wrapping_add(1).wrapping_sub((-imm) as usize)
                };
                if let Some(label) = labels.get(&target) {
                    format!("JumpIfNot r{}, {}", a, label)
                } else {
                    format!("JumpIfNot r{}, @{}", a, target)
                }
            }

            OpCode::MakeClosure => {
                let (_, a, k, upval_count) = decode_a(instr);
                format!("MakeClosure r{}, k{}, {}", a, k, upval_count)
            }
            OpCode::MakeClosureWide => {
                let (_, register, upvalue_count, _) = decode_a(instr);
                format!(
                    "MakeClosureWide r{}, {}, {}",
                    register,
                    extension.unwrap_or(0),
                    upvalue_count
                )
            }
            OpCode::MakeClosureRegisterWide => {
                let first = extension.unwrap_or(0);
                format!(
                    "MakeClosureRegisterWide r{}, {}, {}",
                    first >> 16,
                    second_extension.unwrap_or(0),
                    first & 0xffff
                )
            }
            OpCode::Wide => {
                let (_, inner, _, _) = decode_a(instr);
                let first = extension.unwrap_or(0);
                let second = second_extension.unwrap_or(0);
                let a = first >> 16;
                let b = first & 0xffff;
                let c = second >> 16;
                match OpCode::from_u8(inner) {
                    Some(OpCode::Move) => format!("MoveWide r{a}, r{b}"),
                    Some(OpCode::LoadNull) => format!("LoadNullWide r{a}"),
                    Some(OpCode::Add) => format!("AddWide r{a}, r{b}, r{c}"),
                    Some(OpCode::Return) => format!("ReturnWide r{a}"),
                    Some(_) => format!("Wide {inner}, r{a}, r{b}, r{c}"),
                    None => format!(".word 0x{instr:08x} ; invalid wide opcode {inner}"),
                }
            }
            OpCode::StructNew
            | OpCode::StructLoad
            | OpCode::StructStore
            | OpCode::EnumNew
            | OpCode::EnumTest
            | OpCode::EnumLoad => {
                let schema = instr & 0xffff;
                let first = extension.unwrap_or(0);
                let second = second_extension.unwrap_or(0);
                let a = first >> 16;
                let b = first & 0xffff;
                let c = second >> 16;
                let d = second & 0xffff;
                // all six print five operands because the parser reads five; the low half of the second word is reserved and zero for the struct ops and enumtest
                match opcode {
                    OpCode::StructNew => format!("StructNew  {schema}, r{a}, r{b}, {c}, {d}"),
                    OpCode::StructLoad => format!("StructLoad {schema}, r{a}, r{b}, {c}, {d}"),
                    OpCode::StructStore => format!("StructStore {schema}, r{a}, r{b}, {c}, {d}"),
                    OpCode::EnumNew => format!("EnumNew    {schema}, r{a}, r{b}, {c}, {d}"),
                    OpCode::EnumTest => format!("EnumTest   {schema}, r{a}, r{b}, {c}, {d}"),
                    OpCode::EnumLoad => format!("EnumLoad   {schema}, r{a}, r{b}, {c}, {d}"),
                    _ => unreachable!(),
                }
            }
            OpCode::GetUpval => {
                let (_, a, upval_idx, _) = decode_a(instr);
                format!("GetUpval  r{}, upval[{}]", a, upval_idx)
            }
            OpCode::SetUpval => {
                let (_, upval_idx, src, _) = decode_a(instr);
                format!("SetUpval  upval[{}], r{}", upval_idx, src)
            }
            OpCode::CloseUpvals => {
                let (_, from_reg, _, _) = decode_a(instr);
                format!("CloseUpvals r{}", from_reg)
            }
            OpCode::ForLoopI => {
                let (_, a, offset) = decode_b(instr);
                format!("ForLoopI  r{}, {}", a, offset)
            }
            OpCode::ForLoopIInc => {
                let (_, a, offset) = decode_b(instr);
                format!("ForLoopIInc r{}, {}", a, offset)
            }
            OpCode::AddI => {
                let (_, a, b, c) = decode_a(instr);
                format!("AddI      r{}, r{}, {}", a, b, c)
            }
            OpCode::SubI => {
                let (_, a, b, c) = decode_a(instr);
                format!("SubI      r{}, r{}, {}", a, b, c)
            }
            OpCode::LtImm => {
                let (_, a, imm) = decode_b(instr);
                format!("LtImm     r{}, {}", a, imm)
            }
            OpCode::LeImm => {
                let (_, a, imm) = decode_b(instr);
                format!("LeImm     r{}, {}", a, imm)
            }
            OpCode::GtImm => {
                let (_, a, imm) = decode_b(instr);
                format!("GtImm     r{}, {}", a, imm)
            }
            OpCode::GeImm => {
                let (_, a, imm) = decode_b(instr);
                format!("GeImm     r{}, {}", a, imm)
            }
            OpCode::WhileLoopLt => {
                let (_, a, offset) = decode_b(instr);
                format!("WhileLoopLt r{}, {}", a, offset)
            }
            OpCode::AddII => {
                let (_, a, b, c) = decode_a(instr);
                format!("AddII     r{}, r{}, r{}", a, b, c)
            }
            OpCode::SubII => {
                let (_, a, b, c) = decode_a(instr);
                format!("SubII     r{}, r{}, r{}", a, b, c)
            }
            OpCode::MulII => {
                let (_, a, b, c) = decode_a(instr);
                format!("MulII     r{}, r{}, r{}", a, b, c)
            }
            OpCode::DivII => {
                let (_, a, b, c) = decode_a(instr);
                format!("DivII     r{}, r{}, r{}", a, b, c)
            }
            OpCode::ModII => {
                let (_, a, b, c) = decode_a(instr);
                format!("ModII     r{}, r{}, r{}", a, b, c)
            }
            OpCode::AddFF => {
                let (_, a, b, c) = decode_a(instr);
                format!("AddFF     r{}, r{}, r{}", a, b, c)
            }
            OpCode::SubFF => {
                let (_, a, b, c) = decode_a(instr);
                format!("SubFF     r{}, r{}, r{}", a, b, c)
            }
            OpCode::MulFF => {
                let (_, a, b, c) = decode_a(instr);
                format!("MulFF     r{}, r{}, r{}", a, b, c)
            }
            OpCode::DivFF => {
                let (_, a, b, c) = decode_a(instr);
                format!("DivFF     r{}, r{}, r{}", a, b, c)
            }
            OpCode::ModFF => {
                let (_, a, b, c) = decode_a(instr);
                format!("ModFF     r{}, r{}, r{}", a, b, c)
            }
            OpCode::LtII => {
                let (_, a, b, c) = decode_a(instr);
                format!("LtII      r{}, r{}, r{}", a, b, c)
            }
            OpCode::LeII => {
                let (_, a, b, c) = decode_a(instr);
                format!("LeII      r{}, r{}, r{}", a, b, c)
            }
            OpCode::GtII => {
                let (_, a, b, c) = decode_a(instr);
                format!("GtII      r{}, r{}, r{}", a, b, c)
            }
            OpCode::GeII => {
                let (_, a, b, c) = decode_a(instr);
                format!("GeII      r{}, r{}, r{}", a, b, c)
            }
            OpCode::EqII => {
                let (_, a, b, c) = decode_a(instr);
                format!("EqII      r{}, r{}, r{}", a, b, c)
            }
            OpCode::NeII => {
                let (_, a, b, c) = decode_a(instr);
                format!("NeII      r{}, r{}, r{}", a, b, c)
            }
            OpCode::LtFF => {
                let (_, a, b, c) = decode_a(instr);
                format!("LtFF      r{}, r{}, r{}", a, b, c)
            }
            OpCode::LeFF => {
                let (_, a, b, c) = decode_a(instr);
                format!("LeFF      r{}, r{}, r{}", a, b, c)
            }
            OpCode::GtFF => {
                let (_, a, b, c) = decode_a(instr);
                format!("GtFF      r{}, r{}, r{}", a, b, c)
            }
            OpCode::GeFF => {
                let (_, a, b, c) = decode_a(instr);
                format!("GeFF      r{}, r{}, r{}", a, b, c)
            }
            OpCode::EqFF => {
                let (_, a, b, c) = decode_a(instr);
                format!("EqFF      r{}, r{}, r{}", a, b, c)
            }
            OpCode::NeFF => {
                let (_, a, b, c) = decode_a(instr);
                format!("NeFF      r{}, r{}, r{}", a, b, c)
            }
            OpCode::LtIImm => {
                let (_, a, b, c) = decode_a(instr);
                format!("LtIImm    r{}, r{}, {}", a, b, c)
            }
            OpCode::LeIImm => {
                let (_, a, b, c) = decode_a(instr);
                format!("LeIImm    r{}, r{}, {}", a, b, c)
            }
            OpCode::GtIImm => {
                let (_, a, b, c) = decode_a(instr);
                format!("GtIImm    r{}, r{}, {}", a, b, c)
            }
            OpCode::GeIImm => {
                let (_, a, b, c) = decode_a(instr);
                format!("GeIImm    r{}, r{}, {}", a, b, c)
            }
            OpCode::GetGlobalIdx => {
                let (_, a, imm) = decode_b(instr);
                match self.global_name(imm as usize) {
                    Some(name) => format!("GetGlobalIdx r{}, {}  ; {}", a, imm, name),
                    None => format!("GetGlobalIdx r{}, {}", a, imm),
                }
            }
            OpCode::SetGlobalIdx => {
                let (_, a, imm) = decode_b(instr);
                match self.global_name(imm as usize) {
                    Some(name) => format!("SetGlobalIdx {}, r{}  ; {}", imm, a, name),
                    None => format!("SetGlobalIdx {}, r{}", imm, a),
                }
            }
            OpCode::CallCached => {
                let (_, a, b, c) = decode_a(instr);
                format!("CallCached r{}, r{}, {}", a, b, c)
            }
            OpCode::CallGlobal => {
                let (_, dest, global_idx, nargs) = decode_a(instr);
                match self.global_name(global_idx as usize) {
                    Some(name) => format!(
                        "CallGlobal r{}, {}, {}  ; {}()",
                        dest, global_idx, nargs, name
                    ),
                    None => format!("CallGlobal r{}, {}, {}", dest, global_idx, nargs),
                }
            }
            OpCode::CallUpval => {
                let (_, dest, upval_idx, nargs) = decode_a(instr);
                format!("CallUpval r{}, upval[{}], {}", dest, upval_idx, nargs)
            }
            OpCode::TailCallUpval => {
                let (_, dest, upval_idx, nargs) = decode_a(instr);
                format!("TailCallUpval r{}, upval[{}], {}", dest, upval_idx, nargs)
            }
            OpCode::AddGlobalI => {
                let (_, dest, source, global) = decode_a(instr);
                format!("AddGlobalI r{}, r{}, {}", dest, source, global)
            }

            OpCode::AddIIG => {
                let (_, a, b, c) = decode_a(instr);
                format!("AddIIG    r{}, r{}, r{}", a, b, c)
            }
            OpCode::SubIIG => {
                let (_, a, b, c) = decode_a(instr);
                format!("SubIIG    r{}, r{}, r{}", a, b, c)
            }
            OpCode::MulIIG => {
                let (_, a, b, c) = decode_a(instr);
                format!("MulIIG    r{}, r{}, r{}", a, b, c)
            }
            OpCode::DivIIG => {
                let (_, a, b, c) = decode_a(instr);
                format!("DivIIG    r{}, r{}, r{}", a, b, c)
            }
            OpCode::ModIIG => {
                let (_, a, b, c) = decode_a(instr);
                format!("ModIIG    r{}, r{}, r{}", a, b, c)
            }

            OpCode::AddFFG => {
                let (_, a, b, c) = decode_a(instr);
                format!("AddFFG    r{}, r{}, r{}", a, b, c)
            }
            OpCode::SubFFG => {
                let (_, a, b, c) = decode_a(instr);
                format!("SubFFG    r{}, r{}, r{}", a, b, c)
            }
            OpCode::MulFFG => {
                let (_, a, b, c) = decode_a(instr);
                format!("MulFFG    r{}, r{}, r{}", a, b, c)
            }
            OpCode::DivFFG => {
                let (_, a, b, c) = decode_a(instr);
                format!("DivFFG    r{}, r{}, r{}", a, b, c)
            }
            OpCode::ModFFG => {
                let (_, a, b, c) = decode_a(instr);
                format!("ModFFG    r{}, r{}, r{}", a, b, c)
            }

            OpCode::LtIIG => {
                let (_, a, b, c) = decode_a(instr);
                format!("LtIIG     r{}, r{}, r{}", a, b, c)
            }
            OpCode::LeIIG => {
                let (_, a, b, c) = decode_a(instr);
                format!("LeIIG     r{}, r{}, r{}", a, b, c)
            }
            OpCode::GtIIG => {
                let (_, a, b, c) = decode_a(instr);
                format!("GtIIG     r{}, r{}, r{}", a, b, c)
            }
            OpCode::GeIIG => {
                let (_, a, b, c) = decode_a(instr);
                format!("GeIIG     r{}, r{}, r{}", a, b, c)
            }
            OpCode::EqIIG => {
                let (_, a, b, c) = decode_a(instr);
                format!("EqIIG     r{}, r{}, r{}", a, b, c)
            }
            OpCode::NeIIG => {
                let (_, a, b, c) = decode_a(instr);
                format!("NeIIG     r{}, r{}, r{}", a, b, c)
            }

            OpCode::LtFFG => {
                let (_, a, b, c) = decode_a(instr);
                format!("LtFFG     r{}, r{}, r{}", a, b, c)
            }
            OpCode::LeFFG => {
                let (_, a, b, c) = decode_a(instr);
                format!("LeFFG     r{}, r{}, r{}", a, b, c)
            }
            OpCode::GtFFG => {
                let (_, a, b, c) = decode_a(instr);
                format!("GtFFG     r{}, r{}, r{}", a, b, c)
            }
            OpCode::GeFFG => {
                let (_, a, b, c) = decode_a(instr);
                format!("GeFFG     r{}, r{}, r{}", a, b, c)
            }
            OpCode::EqFFG => {
                let (_, a, b, c) = decode_a(instr);
                format!("EqFFG     r{}, r{}, r{}", a, b, c)
            }
            OpCode::NeFFG => {
                let (_, a, b, c) = decode_a(instr);
                format!("NeFFG     r{}, r{}, r{}", a, b, c)
            }

            OpCode::Shl => {
                let (_, a, b, c) = decode_a(instr);
                format!("Shl       r{}, r{}, r{}", a, b, c)
            }
            OpCode::Shr => {
                let (_, a, b, c) = decode_a(instr);
                format!("Shr       r{}, r{}, r{}", a, b, c)
            }
            OpCode::BitAnd => {
                let (_, a, b, c) = decode_a(instr);
                format!("BitAnd    r{}, r{}, r{}", a, b, c)
            }
            OpCode::BitOr => {
                let (_, a, b, c) = decode_a(instr);
                format!("BitOr     r{}, r{}, r{}", a, b, c)
            }
            OpCode::BitXor => {
                let (_, a, b, c) = decode_a(instr);
                format!("BitXor    r{}, r{}, r{}", a, b, c)
            }
            OpCode::BitNot => {
                let (_, a, b, _) = decode_a(instr);
                format!("BitNot    r{}, r{}", a, b)
            }

            OpCode::ShlII => {
                let (_, a, b, c) = decode_a(instr);
                format!("ShlII     r{}, r{}, r{}", a, b, c)
            }
            OpCode::ShrII => {
                let (_, a, b, c) = decode_a(instr);
                format!("ShrII     r{}, r{}, r{}", a, b, c)
            }
            OpCode::AndII => {
                let (_, a, b, c) = decode_a(instr);
                format!("AndII     r{}, r{}, r{}", a, b, c)
            }
            OpCode::OrII => {
                let (_, a, b, c) = decode_a(instr);
                format!("OrII      r{}, r{}, r{}", a, b, c)
            }
            OpCode::XorII => {
                let (_, a, b, c) = decode_a(instr);
                format!("XorII     r{}, r{}, r{}", a, b, c)
            }
            OpCode::NotI => {
                let (_, a, b, _) = decode_a(instr);
                format!("NotI      r{}, r{}", a, b)
            }

            OpCode::ShlIImm => {
                let (_, a, b, c) = decode_a(instr);
                format!("ShlIImm   r{}, r{}, {}", a, b, c)
            }
            OpCode::ShrIImm => {
                let (_, a, b, c) = decode_a(instr);
                format!("ShrIImm   r{}, r{}, {}", a, b, c)
            }
            OpCode::AndIImm => {
                let (_, a, b, c) = decode_a(instr);
                format!("AndIImm   r{}, r{}, {}", a, b, c)
            }
            OpCode::OrIImm => {
                let (_, a, b, c) = decode_a(instr);
                format!("OrIImm    r{}, r{}, {}", a, b, c)
            }
            OpCode::XorIImm => {
                let (_, a, b, c) = decode_a(instr);
                format!("XorIImm   r{}, r{}, {}", a, b, c)
            }

            OpCode::ArrayNewI => {
                let (_, a, b, _) = decode_a(instr);
                format!("ArrayNewI r{}, r{}", a, b)
            }
            OpCode::ArrayNewF => {
                let (_, a, b, _) = decode_a(instr);
                format!("ArrayNewF r{}, r{}", a, b)
            }
            OpCode::ArrayNewB => {
                let (_, a, b, _) = decode_a(instr);
                format!("ArrayNewB r{}, r{}", a, b)
            }
            OpCode::ArrayNewP => {
                let (_, a, b, _) = decode_a(instr);
                format!("ArrayNewP r{}, r{}", a, b)
            }
            OpCode::ArrayLit => {
                let (_, a, b, c) = decode_a(instr);
                format!("ArrayLit  r{}, r{}, {}", a, b, c)
            }
            OpCode::ArrayLitWide => {
                let first = extension.unwrap_or(0);
                let second = second_extension.unwrap_or(0);
                let dest = first >> 16;
                let start = first & 0xffff;
                let count = second >> 16;
                format!("ArrayLitWide r{dest}, r{start}, {count}")
            }
            OpCode::ArrayLoadI => {
                let (_, a, b, c) = decode_a(instr);
                format!("ArrayLoadI r{}, r{}, r{}", a, b, c)
            }
            OpCode::ArrayLoadF => {
                let (_, a, b, c) = decode_a(instr);
                format!("ArrayLoadF r{}, r{}, r{}", a, b, c)
            }
            OpCode::ArrayLoadB => {
                let (_, a, b, c) = decode_a(instr);
                format!("ArrayLoadB r{}, r{}, r{}", a, b, c)
            }
            OpCode::ArrayLoadP => {
                let (_, a, b, c) = decode_a(instr);
                format!("ArrayLoadP r{}, r{}, r{}", a, b, c)
            }
            OpCode::ArrayGetI => {
                let (_, a, b, c) = decode_a(instr);
                format!("ArrayGetI r{}, r{}, r{}", a, b, c)
            }
            OpCode::ArrayGetF => {
                let (_, a, b, c) = decode_a(instr);
                format!("ArrayGetF r{}, r{}, r{}", a, b, c)
            }
            OpCode::ArrayGetB => {
                let (_, a, b, c) = decode_a(instr);
                format!("ArrayGetB r{}, r{}, r{}", a, b, c)
            }
            OpCode::ArrayGetP => {
                let (_, a, b, c) = decode_a(instr);
                format!("ArrayGetP r{}, r{}, r{}", a, b, c)
            }
            OpCode::ArrayStoreI => {
                let (_, a, b, c) = decode_a(instr);
                format!("ArrayStoreI r{}, r{}, r{}", a, b, c)
            }
            OpCode::ArrayStoreF => {
                let (_, a, b, c) = decode_a(instr);
                format!("ArrayStoreF r{}, r{}, r{}", a, b, c)
            }
            OpCode::ArrayStoreB => {
                let (_, a, b, c) = decode_a(instr);
                format!("ArrayStoreB r{}, r{}, r{}", a, b, c)
            }
            OpCode::ArrayStoreP => {
                let (_, a, b, c) = decode_a(instr);
                format!("ArrayStoreP r{}, r{}, r{}", a, b, c)
            }
            OpCode::ArrayLen => {
                let (_, a, b, _) = decode_a(instr);
                format!("ArrayLen  r{}, r{}", a, b)
            }

            OpCode::VecNewI => {
                let (_, a, b, _) = decode_a(instr);
                format!("VecNewI   r{}, r{}", a, b)
            }
            OpCode::VecNewF => {
                let (_, a, b, _) = decode_a(instr);
                format!("VecNewF   r{}, r{}", a, b)
            }
            OpCode::VecNewB => {
                let (_, a, b, _) = decode_a(instr);
                format!("VecNewB   r{}, r{}", a, b)
            }
            OpCode::VecNewP => {
                let (_, a, b, _) = decode_a(instr);
                format!("VecNewP   r{}, r{}", a, b)
            }
            OpCode::VecLit => {
                let (_, a, b, c) = decode_a(instr);
                format!("VecLit    r{}, r{}, {}", a, b, c)
            }
            OpCode::VecLitWide => {
                let first = extension.unwrap_or(0);
                let second = second_extension.unwrap_or(0);
                let dest = first >> 16;
                let start = first & 0xffff;
                let count = second >> 16;
                format!("VecLitWide r{dest}, r{start}, {count}")
            }
            OpCode::VecPushI => {
                let (_, a, b, _) = decode_a(instr);
                format!("VecPushI  r{}, r{}", a, b)
            }
            OpCode::VecPushF => {
                let (_, a, b, _) = decode_a(instr);
                format!("VecPushF  r{}, r{}", a, b)
            }
            OpCode::VecPushB => {
                let (_, a, b, _) = decode_a(instr);
                format!("VecPushB  r{}, r{}", a, b)
            }
            OpCode::VecPushP => {
                let (_, a, b, _) = decode_a(instr);
                format!("VecPushP  r{}, r{}", a, b)
            }
            OpCode::VecPopI => {
                let (_, a, b, _) = decode_a(instr);
                format!("VecPopI   r{}, r{}", a, b)
            }
            OpCode::VecPopF => {
                let (_, a, b, _) = decode_a(instr);
                format!("VecPopF   r{}, r{}", a, b)
            }
            OpCode::VecPopB => {
                let (_, a, b, _) = decode_a(instr);
                format!("VecPopB   r{}, r{}", a, b)
            }
            OpCode::VecPopP => {
                let (_, a, b, _) = decode_a(instr);
                format!("VecPopP   r{}, r{}", a, b)
            }
            OpCode::VecLen => {
                let (_, a, b, _) = decode_a(instr);
                format!("VecLen    r{}, r{}", a, b)
            }
            OpCode::VecCap => {
                let (_, a, b, _) = decode_a(instr);
                format!("VecCap    r{}, r{}", a, b)
            }
            OpCode::VecReserve => {
                let (_, a, b, _) = decode_a(instr);
                format!("VecReserve r{}, r{}", a, b)
            }
            OpCode::VecLoadI => {
                let (_, a, b, c) = decode_a(instr);
                format!("VecLoadI  r{}, r{}, r{}", a, b, c)
            }
            OpCode::VecLoadF => {
                let (_, a, b, c) = decode_a(instr);
                format!("VecLoadF  r{}, r{}, r{}", a, b, c)
            }
            OpCode::VecLoadB => {
                let (_, a, b, c) = decode_a(instr);
                format!("VecLoadB  r{}, r{}, r{}", a, b, c)
            }
            OpCode::VecLoadP => {
                let (_, a, b, c) = decode_a(instr);
                format!("VecLoadP  r{}, r{}, r{}", a, b, c)
            }
            OpCode::VecGetI => {
                let (_, a, b, c) = decode_a(instr);
                format!("VecGetI   r{}, r{}, r{}", a, b, c)
            }
            OpCode::VecGetF => {
                let (_, a, b, c) = decode_a(instr);
                format!("VecGetF   r{}, r{}, r{}", a, b, c)
            }
            OpCode::VecGetB => {
                let (_, a, b, c) = decode_a(instr);
                format!("VecGetB   r{}, r{}, r{}", a, b, c)
            }
            OpCode::VecGetP => {
                let (_, a, b, c) = decode_a(instr);
                format!("VecGetP   r{}, r{}, r{}", a, b, c)
            }
            OpCode::VecStoreI => {
                let (_, a, b, c) = decode_a(instr);
                format!("VecStoreI r{}, r{}, r{}", a, b, c)
            }
            OpCode::VecStoreF => {
                let (_, a, b, c) = decode_a(instr);
                format!("VecStoreF r{}, r{}, r{}", a, b, c)
            }
            OpCode::VecStoreB => {
                let (_, a, b, c) = decode_a(instr);
                format!("VecStoreB r{}, r{}, r{}", a, b, c)
            }
            OpCode::VecStoreP => {
                let (_, a, b, c) = decode_a(instr);
                format!("VecStoreP r{}, r{}, r{}", a, b, c)
            }
            OpCode::StringLoadChar => {
                let (_, a, b, c) = decode_a(instr);
                format!("StringLoadChar r{}, r{}, r{}", a, b, c)
            }
            OpCode::RangeNew => {
                let (_, a, b, c) = decode_a(instr);
                format!("RangeNew   r{}, r{}, r{}", a, b, c)
            }
            OpCode::RangeNewInclusive => {
                let (_, a, b, c) = decode_a(instr);
                format!("RangeNewInclusive r{}, r{}, r{}", a, b, c)
            }
            OpCode::ArraySlice => {
                let (_, a, b, c) = decode_a(instr);
                format!("ArraySlice r{}, r{}, r{}", a, b, c)
            }
            OpCode::VecSlice => {
                let (_, a, b, c) = decode_a(instr);
                format!("VecSlice   r{}, r{}, r{}", a, b, c)
            }
            OpCode::MakeSum => {
                let (_, a, b, c) = decode_a(instr);
                format!("MakeSum   r{}, r{}, {}", a, b, c)
            }
            OpCode::SumTest => {
                let (_, a, b, c) = decode_a(instr);
                format!("SumTest   r{}, r{}, {}", a, b, c)
            }
            OpCode::SumPayload => {
                let (_, a, b, _) = decode_a(instr);
                format!("SumPayload r{}, r{}", a, b)
            }
            OpCode::MatchFail => {
                let (_, a, b, c) = decode_a(instr);
                format!("MatchFail r{}, r{}, {}", a, b, c)
            }
            OpCode::Cast => {
                let (_, a, b, c) = decode_a(instr);
                format!("Cast      r{}, r{}, {}", a, b, c)
            }
            OpCode::StringForLoop => {
                let (_, a, imm) = decode_b(instr);
                format!("StringForLoop r{}, {}", a, imm)
            }
            OpCode::VecForLoop => {
                let (_, a, imm) = decode_b(instr);
                format!("VecForLoop r{}, {}", a, imm)
            }
            OpCode::ArrayForLoop => {
                let (_, a, imm) = decode_b(instr);
                format!("ArrayForLoop r{}, {}", a, imm)
            }
        }
    }

    fn format_constant(&self, value: &Constant, nested_functions: &[Function]) -> String {
        match value {
            Constant::Null => "null".to_string(),
            Constant::Bool(value) => format!("bool {value}"),
            Constant::Int(value) => format!("int {value}"),
            Constant::Float(bits) => {
                let value = f64::from_bits(*bits);
                if value.is_nan() {
                    "float nan".to_string()
                } else if value == f64::INFINITY {
                    "float inf".to_string()
                } else if value == f64::NEG_INFINITY {
                    "float -inf".to_string()
                } else {
                    format!("float {value}")
                }
            }
            Constant::String(value) => format!("string \"{}\"", escape_string(value)),
            Constant::NestedFunction(index) => {
                let index = *index as usize;
                let name = nested_functions
                    .get(index)
                    .and_then(|function| function.name.as_deref())
                    .or_else(|| self.nested_fn_name(index));
                match name {
                    Some(name) => {
                        format!("func @{} \"{}\"", index + 1, escape_string(name))
                    }
                    None => format!("func @{}", index + 1),
                }
            }
        }
    }
}

pub fn escape_string(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            '\\' => result.push_str("\\\\"),
            '"' => result.push_str("\\\""),
            '\0' => result.push_str("\\0"),
            c if c.is_control() => {
                for byte in c.to_string().bytes() {
                    result.push_str(&format!("\\x{:02x}", byte));
                }
            }
            c => result.push(c),
        }
    }
    result
}
