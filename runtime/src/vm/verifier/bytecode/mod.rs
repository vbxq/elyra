use crate::vm::{
    CastTarget, Function, InstructionFormat, MAX_REGISTERS, OpCode, WideRegisterOperands,
};

mod arithmetic;
mod arrays;
mod calls;
mod closures;
mod control;
mod globals;
mod memory;
mod registers;

use super::checks::{
    check_call_args, check_const_index, check_jump, check_reg, check_reg_range, check_upval_index,
};
use std::collections::HashSet;

pub(super) fn verify_bytecode(func: &Function) -> Result<(), String> {
    if usize::try_from(func.num_registers).unwrap_or(usize::MAX) > MAX_REGISTERS {
        return Err(format!(
            "register count {} exceeds maximum {}",
            func.num_registers, MAX_REGISTERS
        ));
    }
    let num_regs = usize::try_from(func.num_registers)
        .map_err(|_| "register count does not fit this target".to_string())?;
    let constants_len = func.constants.len();
    let upvalues_len = func.upvalue_descriptors.len();
    let bytecode = &func.bytecode;

    if bytecode.len() > u32::MAX as usize {
        return Err(format!(
            "bytecode length {} exceeds maximum {} (u32::MAX)",
            bytecode.len(),
            u32::MAX
        ));
    }
    if constants_len > u32::MAX as usize {
        return Err(format!(
            "constants length {} exceeds maximum {} (u32::MAX)",
            constants_len,
            u32::MAX
        ));
    }

    verify_enum_schemas(func)?;
    verify_struct_schemas(func)?;
    verify_schema_descriptors(func)?;

    let mut ip = 0;
    while ip < bytecode.len() {
        let instr = bytecode[ip];
        let opcode_byte = (instr >> 24) as u8;
        let opcode = OpCode::from_u8(opcode_byte)
            .ok_or_else(|| format!("invalid opcode {} at {}", opcode_byte, ip))?;
        let a = ((instr >> 16) & 0xFF) as usize;
        let b = ((instr >> 8) & 0xFF) as usize;
        let c = (instr & 0xFF) as usize;
        let imm_bits = (instr & 0xFFFF) as u16;
        let imm = i16::from_ne_bytes(imm_bits.to_ne_bytes());

        if opcode == OpCode::Wide {
            let first = bytecode
                .as_slice()
                .get(ip + 1)
                .ok_or_else(|| format!("wide instruction at {ip} is missing operand word 1"))?;
            let second = bytecode
                .as_slice()
                .get(ip + 2)
                .ok_or_else(|| format!("wide instruction at {ip} is missing operand word 2"))?;
            let inner = OpCode::from_u8(a as u8)
                .ok_or_else(|| format!("wide instruction at {ip} has invalid inner opcode {a}"))?;
            let wide_a = (first >> 16) as usize;
            let wide_b = (first & 0xffff) as usize;
            let wide_c = (second >> 16) as usize;
            if !inner.supports_wide_registers() {
                return Err(format!(
                    "opcode {inner:?} does not have a verified wide-register form"
                ));
            }
            match inner.wide_register_operands() {
                Some(WideRegisterOperands::A) => verify_reg(wide_a, num_regs, "wide operand")?,
                Some(WideRegisterOperands::A2) => {
                    verify_reg_range(wide_a, 2, num_regs, "wide register pair")?;
                }
                Some(WideRegisterOperands::B) => verify_reg(wide_b, num_regs, "wide operand")?,
                Some(WideRegisterOperands::Ab) => {
                    verify_reg(wide_a, num_regs, "wide operand")?;
                    verify_reg(wide_b, num_regs, "wide operand")?;
                }
                Some(WideRegisterOperands::Abc) => {
                    verify_reg(wide_a, num_regs, "wide ternary")?;
                    verify_reg(wide_b, num_regs, "wide ternary")?;
                    verify_reg(wide_c, num_regs, "wide ternary")?;
                }
                None => unreachable!("wide support was checked above"),
            }
            let invalid_make_tag = inner == OpCode::MakeSum
                && wide_c > usize::from(aelys_bytecode::object::SumTag::ErrorMessage as u8);
            let invalid_test_tag = inner == OpCode::SumTest
                && wide_c > usize::from(aelys_bytecode::object::SumTag::ErrorMessage as u8)
                && wide_c != 4;
            if invalid_make_tag || invalid_test_tag {
                return Err(format!("invalid wide sum tag {wide_c}"));
            }
            if inner == OpCode::Cast
                && CastTarget::from_u8(
                    u8::try_from(wide_c).map_err(|_| "wide cast target exceeds u8")?,
                )
                .is_none()
            {
                return Err(format!("invalid wide cast target {wide_c}"));
            }
            if inner == OpCode::MatchFail && wide_a > 2 {
                return Err(format!("invalid wide match failure family {wide_a}"));
            }
            if inner == OpCode::MatchFail && wide_c > 1 {
                return Err(format!("invalid wide match failure flag {wide_c}"));
            }
            if inner == OpCode::LoadK {
                let index = (wide_b << 16) | wide_c;
                verify_const(index, constants_len, "LoadKWideRegister")?;
            }
            if inner == OpCode::WhileLoopLt {
                let offset_bits = wide_b as u16;
                let offset = i16::from_ne_bytes(offset_bits.to_ne_bytes());
                let adjusted_ip = ip
                    .checked_add(2)
                    .ok_or_else(|| "wide while ip overflow".to_string())?;
                super::checks::check_jump(adjusted_ip, offset, bytecode.len(), "wide while loop")?;
            }
            ip += 3;
            continue;
        }

        if opcode.format() == InstructionFormat::Struct {
            let first = bytecode
                .as_slice()
                .get(ip + 1)
                .ok_or_else(|| format!("{opcode:?} at {ip} is missing operand word 1"))?;
            let second = bytecode
                .as_slice()
                .get(ip + 2)
                .ok_or_else(|| format!("{opcode:?} at {ip} is missing operand word 2"))?;
            if instr & 0x00ff_0000 != 0
                || (matches!(
                    opcode,
                    OpCode::StructNew | OpCode::StructLoad | OpCode::StructStore
                ) && second & 0xffff != 0)
            {
                return Err(format!("{opcode:?} at {ip} has non-zero reserved bits"));
            }
            let schema_index = (instr & 0xffff) as usize;
            let wide_a = (first >> 16) as usize;
            let wide_b = (first & 0xffff) as usize;
            let wide_c = (second >> 16) as usize;
            match opcode {
                OpCode::StructNew => {
                    let schema = func
                        .struct_schemas
                        .get(schema_index)
                        .ok_or_else(|| format!("{opcode:?} at {ip} has invalid schema index"))?;
                    if schema.schema_id != u32::try_from(schema_index).unwrap_or(u32::MAX) {
                        return Err(format!("{opcode:?} at {ip} has mismatched schema id"));
                    }
                    verify_reg(wide_a, num_regs, "StructNew destination")?;
                    if wide_c != schema.fields.len() {
                        return Err(format!(
                            "StructNew field count {wide_c} does not match schema {}",
                            schema.fields.len()
                        ));
                    }
                    verify_reg_range(wide_b, wide_c, num_regs, "StructNew fields")?;
                }
                OpCode::StructLoad => {
                    let schema = func
                        .struct_schemas
                        .get(schema_index)
                        .ok_or_else(|| format!("{opcode:?} at {ip} has invalid schema index"))?;
                    if schema.schema_id != u32::try_from(schema_index).unwrap_or(u32::MAX) {
                        return Err(format!("{opcode:?} at {ip} has mismatched schema id"));
                    }
                    verify_reg(wide_a, num_regs, "StructLoad destination")?;
                    verify_reg(wide_b, num_regs, "StructLoad object")?;
                    if wide_c >= schema.fields.len() {
                        return Err("StructLoad field offset is out of bounds".to_string());
                    }
                }
                OpCode::StructStore => {
                    let schema = func
                        .struct_schemas
                        .get(schema_index)
                        .ok_or_else(|| format!("{opcode:?} at {ip} has invalid schema index"))?;
                    if schema.schema_id != u32::try_from(schema_index).unwrap_or(u32::MAX) {
                        return Err(format!("{opcode:?} at {ip} has mismatched schema id"));
                    }
                    verify_reg(wide_a, num_regs, "StructStore object")?;
                    verify_reg(wide_b, num_regs, "StructStore value")?;
                    if wide_c >= schema.fields.len() {
                        return Err("StructStore field offset is out of bounds".to_string());
                    }
                }
                OpCode::EnumNew => {
                    let schema = func
                        .enum_schemas
                        .get(schema_index)
                        .ok_or_else(|| format!("{opcode:?} at {ip} has invalid schema index"))?;
                    if schema.schema_id != u16::try_from(schema_index).unwrap_or(u16::MAX) {
                        return Err(format!("{opcode:?} at {ip} has mismatched schema id"));
                    }
                    let variant = schema
                        .variants
                        .get(wide_c)
                        .ok_or_else(|| format!("{opcode:?} at {ip} has invalid variant index"))?;
                    if variant.variant_id != u16::try_from(wide_c).unwrap_or(u16::MAX) {
                        return Err(format!("{opcode:?} at {ip} has mismatched variant id"));
                    }
                    let count = usize::from((second & 0xffff) as u16);
                    if count != variant.fields.len() {
                        return Err(format!(
                            "EnumNew field count {count} does not match variant {}",
                            variant.fields.len()
                        ));
                    }
                    verify_reg(wide_a, num_regs, "EnumNew destination")?;
                    verify_reg_range(wide_b, count, num_regs, "EnumNew fields")?;
                }
                OpCode::EnumTest => {
                    let schema = func
                        .enum_schemas
                        .get(schema_index)
                        .ok_or_else(|| format!("{opcode:?} at {ip} has invalid schema index"))?;
                    if schema.schema_id != u16::try_from(schema_index).unwrap_or(u16::MAX) {
                        return Err(format!("{opcode:?} at {ip} has mismatched schema id"));
                    }
                    if schema.variants.get(wide_c).is_none() {
                        return Err(format!("{opcode:?} at {ip} has invalid variant index"));
                    }
                    if schema.variants[wide_c].variant_id
                        != u16::try_from(wide_c).unwrap_or(u16::MAX)
                    {
                        return Err(format!("{opcode:?} at {ip} has mismatched variant id"));
                    }
                    if second & 0xffff != 0 {
                        return Err("EnumTest has non-zero reserved bits".to_string());
                    }
                    verify_reg(wide_a, num_regs, "EnumTest destination")?;
                    verify_reg(wide_b, num_regs, "EnumTest source")?;
                }
                OpCode::EnumLoad => {
                    let schema = func
                        .enum_schemas
                        .get(schema_index)
                        .ok_or_else(|| format!("{opcode:?} at {ip} has invalid schema index"))?;
                    if schema.schema_id != u16::try_from(schema_index).unwrap_or(u16::MAX) {
                        return Err(format!("{opcode:?} at {ip} has mismatched schema id"));
                    }
                    let variant = schema
                        .variants
                        .get(wide_c)
                        .ok_or_else(|| format!("{opcode:?} at {ip} has invalid variant index"))?;
                    if variant.variant_id != u16::try_from(wide_c).unwrap_or(u16::MAX) {
                        return Err(format!("{opcode:?} at {ip} has mismatched variant id"));
                    }
                    let field_offset = (second & 0xffff) as usize;
                    if field_offset >= variant.fields.len() {
                        return Err("EnumLoad field offset is out of bounds".to_string());
                    }
                    if variant.fields[field_offset].offset
                        != u16::try_from(field_offset).unwrap_or(u16::MAX)
                    {
                        return Err("EnumLoad field offset metadata is invalid".to_string());
                    }
                    verify_reg(wide_a, num_regs, "EnumLoad destination")?;
                    verify_reg(wide_b, num_regs, "EnumLoad source")?;
                }
                _ => unreachable!(),
            }
            ip += 3;
            continue;
        }

        if opcode.format() == InstructionFormat::Abc16 {
            let first = bytecode
                .as_slice()
                .get(ip + 1)
                .ok_or_else(|| format!("{opcode:?} at {ip} is missing operand word 1"))?;
            let second = bytecode
                .as_slice()
                .get(ip + 2)
                .ok_or_else(|| format!("{opcode:?} at {ip} is missing operand word 2"))?;
            if instr & 0x00ff_ffff != 0 || second & 0xffff != 0 {
                return Err(format!("{opcode:?} at {ip} has non-zero reserved bits"));
            }
            let wide_a = (first >> 16) as usize;
            let wide_b = (first & 0xffff) as usize;
            let wide_c = (second >> 16) as usize;
            match opcode {
                OpCode::CallWide => {
                    calls::verify(OpCode::Call, wide_a, wide_b, wide_c, num_regs, upvalues_len)?;
                }
                OpCode::ArrayLitWide => {
                    arrays::verify(OpCode::ArrayLit, wide_a, wide_b, wide_c, num_regs)?;
                }
                OpCode::VecLitWide => {
                    arrays::verify(OpCode::VecLit, wide_a, wide_b, wide_c, num_regs)?;
                }
                _ => return Err(format!("invalid 16-bit operand opcode {opcode:?}")),
            }
            ip += 3;
            continue;
        }

        if opcode.format() == InstructionFormat::RegisterOffset32 {
            let register_word = bytecode
                .as_slice()
                .get(ip + 1)
                .ok_or_else(|| format!("{opcode:?} at {ip} is missing its register word"))?;
            let offset_word = bytecode
                .as_slice()
                .get(ip + 2)
                .ok_or_else(|| format!("{opcode:?} at {ip} is missing its offset word"))?;
            if instr & 0x00ff_ffff != 0 || register_word & 0xffff != 0 {
                return Err(format!("{opcode:?} at {ip} has non-zero reserved bits"));
            }
            let register = (register_word >> 16) as usize;
            verify_reg(register, num_regs, "wide conditional jump")?;
            let offset = i32::from_ne_bytes(offset_word.to_ne_bytes());
            let adjusted_ip = ip
                .checked_add(1)
                .ok_or_else(|| format!("{opcode:?} ip overflow"))?;
            super::checks::check_jump_i32(
                adjusted_ip,
                offset,
                bytecode.len(),
                "wide conditional jump",
            )?;
            ip += 3;
            continue;
        }

        if opcode.format() == InstructionFormat::RegisterIndex32Aux {
            let operands = bytecode
                .as_slice()
                .get(ip + 1)
                .ok_or_else(|| format!("{opcode:?} at {ip} is missing its operand word"))?;
            let index = *bytecode
                .as_slice()
                .get(ip + 2)
                .ok_or_else(|| format!("{opcode:?} at {ip} is missing its index word"))?
                as usize;
            if instr & 0x00ff_ffff != 0 {
                return Err(format!("{opcode:?} at {ip} has non-zero reserved bits"));
            }
            let register = (operands >> 16) as usize;
            let upvalue_count = (operands & 0xffff) as usize;
            verify_reg(register, num_regs, "wide closure destination")?;
            verify_const(index, constants_len, "wide closure")?;
            let function_index = func.constants[index].as_nested_fn_marker().ok_or_else(|| {
                format!("wide closure constant {index} is not a nested function marker")
            })?;
            let nested = func.nested_functions.get(function_index).ok_or_else(|| {
                format!("wide closure nested function index {function_index} out of bounds")
            })?;
            if nested.upvalue_descriptors.len() != upvalue_count {
                return Err(format!(
                    "wide closure upvalue count {upvalue_count} does not match descriptors {}",
                    nested.upvalue_descriptors.len()
                ));
            }
            ip += 3;
            continue;
        }

        if opcode.format() == InstructionFormat::WideRegisterOffset32 {
            let register_word = bytecode
                .as_slice()
                .get(ip + 1)
                .ok_or_else(|| format!("{opcode:?} at {ip} is missing its register word"))?;
            let offset_word = bytecode
                .as_slice()
                .get(ip + 2)
                .ok_or_else(|| format!("{opcode:?} at {ip} is missing its offset word"))?;
            if instr & 0xffff != 0 || register_word & 0xffff != 0 {
                return Err(format!("{opcode:?} at {ip} has non-zero reserved bits"));
            }
            let inner = OpCode::from_u8(a as u8)
                .ok_or_else(|| format!("{opcode:?} at {ip} has invalid inner opcode {a}"))?;
            if !matches!(
                inner,
                OpCode::ForLoopILong
                    | OpCode::ForLoopIIncLong
                    | OpCode::StringForLoopLong
                    | OpCode::VecForLoopLong
                    | OpCode::ArrayForLoopLong
            ) {
                return Err(format!("{opcode:?} at {ip} cannot wrap {inner:?}"));
            }
            let register = (register_word >> 16) as usize;
            verify_reg_range(register, 3, num_regs, "wide loop")?;
            let offset = i32::from_ne_bytes(offset_word.to_ne_bytes());
            let adjusted_ip = ip
                .checked_add(1)
                .ok_or_else(|| format!("{opcode:?} ip overflow"))?;
            super::checks::check_jump_i32(adjusted_ip, offset, bytecode.len(), "wide loop")?;
            ip += 3;
            continue;
        }

        if opcode.format() == InstructionFormat::AOffset32 {
            let extension = bytecode
                .as_slice()
                .get(ip + 1)
                .ok_or_else(|| format!("long jump at {} is missing its extension word", ip))?;
            match opcode {
                OpCode::JumpLong => {}
                OpCode::JumpIfLong | OpCode::JumpIfNotLong => {
                    verify_reg(a, num_regs, "conditional long jump")?;
                }
                OpCode::ForLoopILong | OpCode::ForLoopIIncLong => {
                    verify_reg_range(a, 3, num_regs, "ForLoopILong")?;
                }
                OpCode::StringForLoopLong | OpCode::VecForLoopLong | OpCode::ArrayForLoopLong => {
                    verify_reg_range(a, 3, num_regs, "collection loop long")?;
                }
                _ => return Err(format!("invalid long-offset opcode {opcode:?}")),
            }
            let offset = i32::from_ne_bytes(extension.to_ne_bytes());
            super::checks::check_jump_i32(ip, offset, bytecode.len(), "long-offset instruction")?;
            ip += 2;
            continue;
        }

        if opcode.format() == InstructionFormat::AIndex32 {
            let extension = bytecode
                .as_slice()
                .get(ip + 1)
                .ok_or_else(|| format!("wide index at {} is missing its extension word", ip))?;
            let index = *extension as usize;
            verify_reg(a, num_regs, "wide-index instruction")?;
            if matches!(opcode, OpCode::LoadKWide | OpCode::MakeClosureWide) {
                verify_const(index, constants_len, "wide-index instruction")?;
            }
            if opcode == OpCode::MakeClosureWide {
                let constant = &func.constants[index];
                let function_index = constant.as_nested_fn_marker().ok_or_else(|| {
                    format!("MakeClosureWide constant {index} is not a nested function marker")
                })?;
                let nested = func.nested_functions.get(function_index).ok_or_else(|| {
                    format!("MakeClosureWide nested function index {function_index} out of bounds")
                })?;
                if nested.upvalue_descriptors.len() != b {
                    return Err(format!(
                        "MakeClosureWide upvalue count {b} does not match descriptors {}",
                        nested.upvalue_descriptors.len()
                    ));
                }
            }
            ip += 2;
            continue;
        }

        if registers::verify(opcode, a, b, c, imm, num_regs, constants_len)? {
            ip += 1;
            continue;
        }
        if arithmetic::verify(opcode, a, b, c, num_regs)? {
            ip += 1;
            continue;
        }
        if control::verify(opcode, ip, a, b, c, imm, num_regs, bytecode.len())? {
            ip += 1;
            continue;
        }
        if memory::verify(opcode, a, b, c, num_regs)? {
            ip += 1;
            continue;
        }
        if globals::verify(
            opcode,
            ip,
            a,
            b,
            c,
            imm,
            num_regs,
            constants_len,
            bytecode.len(),
        )? {
            ip += 1;
            continue;
        }
        if calls::verify(opcode, a, b, c, num_regs, upvalues_len)? {
            ip += 1;
            continue;
        }
        if closures::verify(func, opcode, a, b, c, num_regs, constants_len, upvalues_len)? {
            ip += 1;
            continue;
        }
        if arrays::verify(opcode, a, b, c, num_regs)? {
            ip += 1;
            continue;
        }

        return Err(format!("unhandled opcode {:?} at {}", opcode, ip));
    }

    Ok(())
}

fn verify_enum_schemas(func: &Function) -> Result<(), String> {
    let mut enum_names = HashSet::with_capacity(func.enum_schemas.len());
    let mut enum_identities = HashSet::with_capacity(func.enum_schemas.len());
    for (schema_index, schema) in func.enum_schemas.iter().enumerate() {
        if schema.name.is_empty() || !enum_names.insert(schema.name.as_str()) {
            return Err("enum schema names must be non-empty and unique".to_string());
        }
        if schema.def_id.package.is_empty()
            || schema.def_id.module.is_empty()
            || schema.def_id.module.iter().any(String::is_empty)
            || !enum_identities.insert((&schema.def_id, schema.type_args.as_ref()))
        {
            return Err("enum definition paths must be non-empty and unique".to_string());
        }
        if schema.schema_id != u16::try_from(schema_index).unwrap_or(u16::MAX) {
            return Err("enum schema ids must be ordered from zero".to_string());
        }
        if schema.arity != u16::try_from(schema.type_args.len()).unwrap_or(u16::MAX) {
            return Err(format!(
                "enum {} has mismatched type parameter arity",
                schema.name
            ));
        }
        for type_arg in &schema.type_args {
            verify_enum_descriptor(type_arg, &func.enum_schemas, 0)?;
        }
        let mut variant_names = HashSet::with_capacity(schema.variants.len());
        for (variant_index, variant) in schema.variants.iter().enumerate() {
            if variant.name.is_empty() || !variant_names.insert(variant.name.as_str()) {
                return Err(format!(
                    "enum {} has duplicate or empty variant name",
                    schema.name
                ));
            }
            if variant.variant_id != u16::try_from(variant_index).unwrap_or(u16::MAX) {
                return Err(format!("enum {} has unordered variant ids", schema.name));
            }
            let named = variant
                .fields
                .iter()
                .filter(|field| field.name.is_some())
                .count();
            if named != 0 && named != variant.fields.len() {
                return Err(format!(
                    "enum {}::{} mixes named and tuple fields",
                    schema.name, variant.name
                ));
            }
            let mut field_names = HashSet::new();
            for (field_index, field) in variant.fields.iter().enumerate() {
                if field.offset != u16::try_from(field_index).unwrap_or(u16::MAX) {
                    return Err(format!(
                        "enum {}::{} has unordered field offsets",
                        schema.name, variant.name
                    ));
                }
                if let Some(name) = &field.name
                    && (name.is_empty() || !field_names.insert(name.as_str()))
                {
                    return Err(format!(
                        "enum {}::{} has duplicate or empty field name",
                        schema.name, variant.name
                    ));
                }
                verify_enum_descriptor(&field.ty, &func.enum_schemas, 0)?;
            }
        }
    }
    Ok(())
}

fn verify_enum_descriptor(
    descriptor: &aelys_bytecode::TypeDescriptor,
    enum_schemas: &[aelys_bytecode::EnumSchema],
    depth: usize,
) -> Result<(), String> {
    if depth > 64 {
        return Err("enum field descriptor nesting exceeds 64".to_string());
    }
    match descriptor {
        aelys_bytecode::TypeDescriptor::Any | aelys_bytecode::TypeDescriptor::Never => {
            return Err("enum descriptors must be concrete".to_string());
        }
        aelys_bytecode::TypeDescriptor::Enum(schema_id) => {
            if enum_schemas
                .get(usize::from(*schema_id))
                .is_none_or(|schema| schema.schema_id != *schema_id)
            {
                return Err(format!(
                    "enum field refers to unknown schema id {schema_id}"
                ));
            }
        }
        aelys_bytecode::TypeDescriptor::Option(inner)
        | aelys_bytecode::TypeDescriptor::Array(inner)
        | aelys_bytecode::TypeDescriptor::FixedArray(inner, _)
        | aelys_bytecode::TypeDescriptor::Vec(inner) => {
            verify_enum_descriptor(inner, enum_schemas, depth + 1)?;
        }
        aelys_bytecode::TypeDescriptor::Result(ok, err) => {
            verify_enum_descriptor(ok, enum_schemas, depth + 1)?;
            verify_enum_descriptor(err, enum_schemas, depth + 1)?;
        }
        aelys_bytecode::TypeDescriptor::Function { params, ret } => {
            for param in params {
                verify_enum_descriptor(param, enum_schemas, depth + 1)?;
            }
            verify_enum_descriptor(ret, enum_schemas, depth + 1)?;
        }
        _ => {}
    }
    Ok(())
}

fn verify_struct_schemas(func: &Function) -> Result<(), String> {
    let mut identities = HashSet::with_capacity(func.struct_schemas.len());
    for (schema_index, schema) in func.struct_schemas.iter().enumerate() {
        if schema.schema_id != u32::try_from(schema_index).unwrap_or(u32::MAX) {
            return Err("struct schema ids must be ordered from zero".to_string());
        }
        if schema.ctor.package.is_empty()
            || schema.ctor.module.is_empty()
            || schema.ctor.module.iter().any(String::is_empty)
            || !identities.insert((&schema.ctor, schema.type_args.as_ref()))
        {
            return Err("struct definition paths must be non-empty and unique".to_string());
        }
        let mut field_names = HashSet::with_capacity(schema.fields.len());
        for (field_index, field) in schema.fields.iter().enumerate() {
            if field.offset != u16::try_from(field_index).unwrap_or(u16::MAX) {
                return Err(format!(
                    "struct {} has unordered field offsets",
                    schema.display_name()
                ));
            }
            if field.name.is_empty() || !field_names.insert(field.name.as_str()) {
                return Err(format!(
                    "struct {} has a duplicate or empty field name",
                    schema.display_name()
                ));
            }
        }
    }
    Ok(())
}

fn verify_schema_descriptors(func: &Function) -> Result<(), String> {
    let struct_count = u32::try_from(func.struct_schemas.len()).unwrap_or(u32::MAX);
    for schema in &func.struct_schemas {
        for type_arg in &schema.type_args {
            verify_descriptor_ids(type_arg, struct_count, &func.enum_schemas, 0)?;
        }
        for field in &schema.fields {
            verify_descriptor_ids(&field.ty, struct_count, &func.enum_schemas, 0)?;
        }
    }
    for schema in &func.enum_schemas {
        for type_arg in &schema.type_args {
            verify_descriptor_ids(type_arg, struct_count, &func.enum_schemas, 0)?;
        }
        for variant in &schema.variants {
            for field in &variant.fields {
                verify_descriptor_ids(&field.ty, struct_count, &func.enum_schemas, 0)?;
            }
        }
    }
    Ok(())
}

fn verify_descriptor_ids(
    descriptor: &aelys_bytecode::TypeDescriptor,
    struct_count: u32,
    enum_schemas: &[aelys_bytecode::EnumSchema],
    depth: usize,
) -> Result<(), String> {
    if depth > 64 {
        return Err("schema descriptor nesting exceeds 64".to_string());
    }
    match descriptor {
        aelys_bytecode::TypeDescriptor::Struct(schema_id) => {
            if *schema_id >= struct_count {
                return Err(format!(
                    "field refers to unknown struct schema id {schema_id}"
                ));
            }
        }
        aelys_bytecode::TypeDescriptor::Enum(schema_id) => {
            if enum_schemas
                .get(usize::from(*schema_id))
                .is_none_or(|schema| schema.schema_id != *schema_id)
            {
                return Err(format!(
                    "enum field refers to unknown schema id {schema_id}"
                ));
            }
        }
        aelys_bytecode::TypeDescriptor::Option(inner)
        | aelys_bytecode::TypeDescriptor::Array(inner)
        | aelys_bytecode::TypeDescriptor::FixedArray(inner, _)
        | aelys_bytecode::TypeDescriptor::Vec(inner) => {
            verify_descriptor_ids(inner, struct_count, enum_schemas, depth + 1)?;
        }
        aelys_bytecode::TypeDescriptor::Result(ok, err) => {
            verify_descriptor_ids(ok, struct_count, enum_schemas, depth + 1)?;
            verify_descriptor_ids(err, struct_count, enum_schemas, depth + 1)?;
        }
        aelys_bytecode::TypeDescriptor::Function { params, ret } => {
            for param in params {
                verify_descriptor_ids(param, struct_count, enum_schemas, depth + 1)?;
            }
            verify_descriptor_ids(ret, struct_count, enum_schemas, depth + 1)?;
        }
        aelys_bytecode::TypeDescriptor::Any | aelys_bytecode::TypeDescriptor::Never => {
            return Err("schema descriptors must be concrete".to_string());
        }
        _ => {}
    }
    Ok(())
}

pub(super) fn verify_call_args(
    base_reg: usize,
    nargs: usize,
    num_regs: usize,
    op: &str,
) -> Result<(), String> {
    check_reg(base_reg, num_regs, op)?;
    check_call_args(base_reg, nargs, num_regs, op)
}

pub(super) fn verify_const(idx: usize, constants_len: usize, op: &str) -> Result<(), String> {
    check_const_index(idx, constants_len, op)
}

pub(super) fn verify_jump(
    ip: usize,
    imm: i16,
    bytecode_len: usize,
    op: &str,
) -> Result<(), String> {
    check_jump(ip, imm, bytecode_len, op)
}

pub(super) fn verify_reg(reg: usize, num_regs: usize, op: &str) -> Result<(), String> {
    check_reg(reg, num_regs, op)
}

pub(super) fn verify_upval(idx: usize, upvalues_len: usize, op: &str) -> Result<(), String> {
    check_upval_index(idx, upvalues_len, op)
}

pub(super) fn verify_reg_range(
    base: usize,
    count: usize,
    num_regs: usize,
    op: &str,
) -> Result<(), String> {
    check_reg_range(base, count, num_regs, op)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aelys_bytecode::{
        EnumFieldSchema, EnumSchema, EnumVariantSchema, IntWidth, TypeDescriptor,
    };

    #[test]
    fn enum_load_rejects_field_offset_for_expected_variant() {
        let mut function = Function::new(Some("bad_enum_load".to_string()), 0);
        function.num_registers = 2;
        function.enum_schemas = vec![EnumSchema::new(
            "test::Shape".to_string(),
            vec![EnumVariantSchema {
                variant_id: 0,
                name: "Point".to_string(),
                fields: vec![EnumFieldSchema {
                    offset: 0,
                    name: Some("x".to_string()),
                    ty: TypeDescriptor::Int(IntWidth::I64),
                }]
                .into_boxed_slice(),
            }],
        )];
        function.emit_enum(OpCode::EnumLoad, 0, 0, 1, 0, 1, 1);
        function.finalize_bytecode();

        let error = verify_bytecode(&function).expect_err("invalid enum field offset");
        assert!(
            error.contains("EnumLoad field offset is out of bounds"),
            "{error}"
        );
    }

    #[test]
    fn verifier_rejects_duplicate_enum_variant_names() {
        let mut function = Function::new(Some("duplicate_enum_variant".to_string()), 0);
        function.enum_schemas = vec![EnumSchema::new(
            "test::Shape".to_string(),
            vec![
                EnumVariantSchema {
                    variant_id: 0,
                    name: "Same".to_string(),
                    fields: Vec::new().into_boxed_slice(),
                },
                EnumVariantSchema {
                    variant_id: 1,
                    name: "Same".to_string(),
                    fields: Vec::new().into_boxed_slice(),
                },
            ],
        )];

        let error = verify_bytecode(&function).expect_err("duplicate enum variant");
        assert!(error.contains("duplicate or empty variant name"), "{error}");
    }
}
