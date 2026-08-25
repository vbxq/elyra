use crate::bytecode::{
    Constant, DefId, EnumFieldSchema, EnumSchema, EnumVariantSchema, FloatWidth, Function,
    GlobalLayout, IntWidth, OpCode, StructFieldSchema, StructSchema, TypeDescriptor,
    UpvalueDescriptor,
};
use std::collections::{HashMap, HashSet};
use std::io::{self, Cursor, Read};
use thiserror::Error;

pub const MAGIC: &[u8; 4] = b"VBXQ";

pub const VERSION: u16 = 4;
const LEGACY_VERSION: u16 = 3;

const MAX_BYTECODE_LEN: usize = 1_000_000;
const MAX_CONSTANTS: usize = 1_000_000;
const MAX_NESTED_FUNCTIONS: usize = 4_096;
const MAX_UPVALUE_DESCRIPTORS: usize = 256;
const MAX_LINES: usize = 1_000_000;
const MAX_GLOBAL_NAMES: usize = 65_535;
const MAX_STRING_LEN: usize = 1_000_000;
const MAX_NESTING_DEPTH: usize = 64;
const MAX_SECTION_LEN: usize = 256 * 1024 * 1024;
const MAX_NATIVE_BUNDLES: usize = 65_535;
const MAX_STRUCT_SCHEMAS: usize = 65_535;
const MAX_STRUCT_FIELDS: usize = 65_535;
const MAX_ENUM_SCHEMAS: usize = 65_535;
const MAX_ENUM_VARIANTS: usize = 65_535;
const MAX_ENUM_FIELDS: usize = 65_535;
const MAX_SCHEMA_TABLE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_REGISTERS: u32 = 65_536;

const SECTION_MANIFEST: u32 = u32::from_le_bytes(*b"MANF");
const SECTION_BUNDLES: u32 = u32::from_le_bytes(*b"NBND");

pub type DeserializeResult = Result<(Function, Option<Vec<u8>>, Vec<NativeBundle>)>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeBundle {
    pub name: String,
    pub target: String,
    pub checksum: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Error)]
pub enum BinaryError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("Invalid magic number")]
    InvalidMagic,

    #[error("Unsupported version: {0}")]
    UnsupportedVersion(u16),

    #[error("Invalid constant type: {0}")]
    InvalidConstantType(u8),

    #[error("Invalid enum schema: {0}")]
    InvalidEnumSchema(String),

    #[error("Invalid struct schema: {0}")]
    InvalidStructSchema(String),

    #[error("Invalid nested function index: {index} (max: {max})")]
    InvalidNestedFunctionIndex { index: usize, max: usize },

    #[error("Invalid UTF-8 in string")]
    InvalidUtf8,

    #[error("Unexpected end of file")]
    UnexpectedEof,

    #[error("Limit exceeded: {what} (max {limit})")]
    LimitExceeded { what: &'static str, limit: usize },
}

pub type Result<T> = std::result::Result<T, BinaryError>;

type LegacyStructRecord = (String, Vec<StructFieldSchema>);

pub fn serialize(func: &Function) -> Result<Vec<u8>> {
    let mut writer = BinaryWriter::new();
    writer.write_program(func)?;
    Ok(writer.into_bytes())
}

pub fn serialize_with_manifest(
    func: &Function,
    manifest: Option<&[u8]>,
    bundles: Option<&[NativeBundle]>,
) -> Result<Vec<u8>> {
    let mut writer = BinaryWriter::new();
    writer.write_program(func)?;
    if let Some(manifest_bytes) = manifest {
        writer.write_section(SECTION_MANIFEST, manifest_bytes)?;
    }
    if let Some(bundles) = bundles {
        let data = build_bundles_section(bundles)?;
        writer.write_section(SECTION_BUNDLES, &data)?;
    }
    Ok(writer.into_bytes())
}

pub fn deserialize(data: &[u8]) -> Result<Function> {
    let reader = BinaryReader::new(data);
    reader.read_program()
}

pub fn deserialize_with_manifest(data: &[u8]) -> DeserializeResult {
    let reader = BinaryReader::new(data);
    reader.read_program_with_sections()
}

struct BinaryWriter {
    buffer: Vec<u8>,
}

impl BinaryWriter {
    fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    fn into_bytes(self) -> Vec<u8> {
        self.buffer
    }

    fn write_program(&mut self, func: &Function) -> Result<()> {
        self.write_bytes(MAGIC);
        let version = format_version(func);
        self.write_u16(version);
        self.write_u16(0); // Flags (reserved)

        let func_count = count_functions(func)?;
        self.write_u32(func_count);
        self.write_u32(0); // Reserved

        self.write_function(func, 0, version)
    }

    fn write_section(&mut self, tag: u32, data: &[u8]) -> Result<()> {
        ensure_len(data.len(), MAX_SECTION_LEN, "section length")?;
        self.write_u32(tag);
        self.write_u32(
            u32::try_from(data.len()).map_err(|_| BinaryError::LimitExceeded {
                what: "section length",
                limit: MAX_SECTION_LEN,
            })?,
        );
        self.write_bytes(data);
        Ok(())
    }

    fn write_function(&mut self, func: &Function, depth: usize, version: u16) -> Result<()> {
        ensure_len(depth, MAX_NESTING_DEPTH, "function nesting depth")?;
        if let Some(name) = &func.name {
            ensure_len(name.len(), usize::from(u16::MAX), "function name length")?;
            self.write_u16(
                u16::try_from(name.len()).map_err(|_| BinaryError::LimitExceeded {
                    what: "function name length",
                    limit: usize::from(u16::MAX),
                })?,
            );
            self.write_bytes(name.as_bytes());
        } else {
            self.write_u16(0);
        }

        self.write_u16(func.arity);
        if func.num_registers > MAX_REGISTERS {
            return Err(BinaryError::LimitExceeded {
                what: "register count",
                limit: MAX_REGISTERS as usize,
            });
        }
        self.write_u32(func.num_registers);

        if version >= VERSION {
            validate_enum_schemas(&func.enum_schemas)?;
            validate_struct_schemas(&func.struct_schemas)?;
            validate_schema_descriptors(&func.struct_schemas, &func.enum_schemas)?;
            if bytecode_has_enum_opcode(func.bytecode.as_slice()) && func.enum_schemas.is_empty() {
                return Err(BinaryError::InvalidEnumSchema(
                    "enum opcode requires an enum schema table".to_string(),
                ));
            }
        } else if !func.struct_schemas.is_empty() {
            return Err(BinaryError::InvalidStructSchema(
                "a struct schema table requires version 4".to_string(),
            ));
        }
        self.write_schema_table(&func.struct_schemas)?;
        if version >= VERSION {
            self.write_enum_schema_table(&func.enum_schemas)?;
        }
        self.write_u8(u8::from(func.jit_unsupported_struct));
        self.write_u8(0);

        ensure_len(func.constants.len(), MAX_CONSTANTS, "constant count")?;
        self.write_u32(u32::try_from(func.constants.len()).map_err(|_| {
            BinaryError::LimitExceeded {
                what: "constant count",
                limit: MAX_CONSTANTS,
            }
        })?);
        for constant in &func.constants {
            self.write_constant(constant)?;
        }

        ensure_len(func.bytecode.len(), MAX_BYTECODE_LEN, "bytecode length")?;
        self.write_u32(u32::try_from(func.bytecode.len()).map_err(|_| {
            BinaryError::LimitExceeded {
                what: "bytecode length",
                limit: MAX_BYTECODE_LEN,
            }
        })?);
        for &instr in func.bytecode.iter() {
            self.write_u32(instr);
        }

        ensure_len(
            func.nested_functions.len(),
            MAX_NESTED_FUNCTIONS,
            "nested function count",
        )?;
        self.write_u16(u16::try_from(func.nested_functions.len()).map_err(|_| {
            BinaryError::LimitExceeded {
                what: "nested function count",
                limit: MAX_NESTED_FUNCTIONS,
            }
        })?);
        for nested in &func.nested_functions {
            self.write_function(nested, depth.saturating_add(1), version)?;
        }

        ensure_len(
            func.upvalue_descriptors.len(),
            MAX_UPVALUE_DESCRIPTORS,
            "upvalue descriptor count",
        )?;
        self.write_u16(u16::try_from(func.upvalue_descriptors.len()).map_err(|_| {
            BinaryError::LimitExceeded {
                what: "upvalue descriptor count",
                limit: MAX_UPVALUE_DESCRIPTORS,
            }
        })?);
        for desc in &func.upvalue_descriptors {
            self.write_u8(if desc.is_local { 1 } else { 0 });
            self.write_u16(desc.index);
        }

        let line_limit = MAX_LINES.min(usize::from(u16::MAX));
        ensure_len(func.lines.len(), line_limit, "line entry count")?;
        self.write_u16(u16::try_from(func.lines.len()).map_err(|_| {
            BinaryError::LimitExceeded {
                what: "line entry count",
                limit: line_limit,
            }
        })?);
        for &(count, line) in &func.lines {
            self.write_u16(count);
            self.write_u32(line);
        }

        ensure_len(
            func.global_layout.names().len(),
            MAX_GLOBAL_NAMES,
            "global name count",
        )?;
        self.write_u16(
            u16::try_from(func.global_layout.names().len()).map_err(|_| {
                BinaryError::LimitExceeded {
                    what: "global name count",
                    limit: MAX_GLOBAL_NAMES,
                }
            })?,
        );
        for name in func.global_layout.names() {
            ensure_len(name.len(), usize::from(u16::MAX), "global name length")?;
            self.write_u16(
                u16::try_from(name.len()).map_err(|_| BinaryError::LimitExceeded {
                    what: "global name length",
                    limit: usize::from(u16::MAX),
                })?,
            );
            self.write_bytes(name.as_bytes());
        }
        Ok(())
    }

    fn write_constant(&mut self, value: &Constant) -> Result<()> {
        match value {
            Constant::Null => self.write_u8(0),
            Constant::Bool(value) => {
                self.write_u8(1);
                self.write_u8(u8::from(*value));
            }
            Constant::Int(value) => {
                self.write_u8(2);
                self.write_i64(*value);
            }
            Constant::Float(bits) => {
                self.write_u8(3);
                self.write_u64(*bits);
            }
            Constant::String(value) => {
                ensure_len(value.len(), MAX_STRING_LEN, "constant string length")?;
                self.write_u8(4);
                self.write_u32(u32::try_from(value.len()).map_err(|_| {
                    BinaryError::LimitExceeded {
                        what: "constant string length",
                        limit: MAX_STRING_LEN,
                    }
                })?);
                self.write_bytes(value.as_bytes());
            }
            Constant::NestedFunction(index) => {
                self.write_u8(5);
                self.write_u32(*index);
            }
        }
        Ok(())
    }

    fn write_schema_table(&mut self, schemas: &[StructSchema]) -> Result<()> {
        ensure_len(schemas.len(), MAX_STRUCT_SCHEMAS, "struct schema count")?;
        self.write_u16(
            u16::try_from(schemas.len()).map_err(|_| BinaryError::LimitExceeded {
                what: "struct schema count",
                limit: MAX_STRUCT_SCHEMAS,
            })?,
        );
        self.write_u16(0);
        let start = self.buffer.len();
        for schema in schemas {
            self.write_u32(schema.schema_id);
            self.write_def_id(&schema.ctor)?;
            ensure_len(
                schema.type_args.len(),
                MAX_STRUCT_FIELDS,
                "struct type argument count",
            )?;
            self.write_u16(u16::try_from(schema.type_args.len()).map_err(|_| {
                BinaryError::LimitExceeded {
                    what: "struct type argument count",
                    limit: MAX_STRUCT_FIELDS,
                }
            })?);
            ensure_len(schema.fields.len(), MAX_STRUCT_FIELDS, "struct field count")?;
            self.write_u16(u16::try_from(schema.fields.len()).map_err(|_| {
                BinaryError::LimitExceeded {
                    what: "struct field count",
                    limit: MAX_STRUCT_FIELDS,
                }
            })?);
            for type_arg in &schema.type_args {
                self.write_descriptor(type_arg)?;
            }
            for field in &schema.fields {
                self.write_u16(field.offset);
                write_string_u16(&mut self.buffer, &field.name, "struct field name")?;
                self.write_descriptor(&field.ty)?;
            }
            ensure_len(
                self.buffer.len().saturating_sub(start),
                MAX_SCHEMA_TABLE_BYTES,
                "struct schema table",
            )?;
        }
        ensure_len(
            self.buffer.len().saturating_sub(start),
            MAX_SCHEMA_TABLE_BYTES,
            "struct schema table",
        )?;
        Ok(())
    }

    fn write_enum_schema_table(&mut self, schemas: &[EnumSchema]) -> Result<()> {
        validate_enum_schemas(schemas)?;
        ensure_len(schemas.len(), MAX_ENUM_SCHEMAS, "enum schema count")?;
        self.write_u16(u16::try_from(schemas.len()).expect("enum schema count was checked"));
        self.write_u16(0);
        let start = self.buffer.len();
        for schema in schemas {
            self.write_u16(schema.schema_id);
            self.write_def_id(&schema.def_id)?;
            write_string_u16(&mut self.buffer, &schema.name, "enum schema name")?;
            self.write_u16(schema.arity);
            ensure_len(
                schema.type_args.len(),
                MAX_ENUM_FIELDS,
                "enum type argument count",
            )?;
            self.write_u16(
                u16::try_from(schema.type_args.len()).expect("enum type argument count checked"),
            );
            ensure_len(
                schema.variants.len(),
                MAX_ENUM_VARIANTS,
                "enum variant count",
            )?;
            self.write_u16(
                u16::try_from(schema.variants.len()).expect("enum variant count was checked"),
            );
            for type_arg in &schema.type_args {
                self.write_descriptor(type_arg)?;
            }
            for variant in &schema.variants {
                self.write_u16(variant.variant_id);
                write_string_u16(&mut self.buffer, &variant.name, "enum variant name")?;
                ensure_len(variant.fields.len(), MAX_ENUM_FIELDS, "enum field count")?;
                self.write_u16(
                    u16::try_from(variant.fields.len()).expect("enum field count was checked"),
                );
                let field_kind = if variant.fields.is_empty() {
                    0
                } else if variant.fields.iter().all(|field| field.name.is_some()) {
                    2
                } else {
                    1
                };
                self.write_u8(field_kind);
                self.write_u8(0);
                for field in &variant.fields {
                    self.write_u16(field.offset);
                    if field_kind == 2 {
                        write_string_u16(
                            &mut self.buffer,
                            field.name.as_deref().unwrap_or_default(),
                            "enum field name",
                        )?;
                    } else {
                        self.write_u16(0);
                    }
                    self.write_descriptor(&field.ty)?;
                }
                ensure_len(
                    self.buffer.len().saturating_sub(start),
                    MAX_SCHEMA_TABLE_BYTES,
                    "enum schema table",
                )?;
            }
        }
        ensure_len(
            self.buffer.len().saturating_sub(start),
            MAX_SCHEMA_TABLE_BYTES,
            "enum schema table",
        )?;
        Ok(())
    }

    fn write_def_id(&mut self, def_id: &DefId) -> Result<()> {
        write_string_u16(&mut self.buffer, &def_id.package, "schema package")?;
        ensure_len(
            def_id.module.len(),
            MAX_ENUM_FIELDS,
            "schema module segment count",
        )?;
        self.write_u16(u16::try_from(def_id.module.len()).map_err(|_| {
            BinaryError::LimitExceeded {
                what: "schema module segment count",
                limit: MAX_ENUM_FIELDS,
            }
        })?);
        for segment in &def_id.module {
            write_string_u16(&mut self.buffer, segment, "schema module segment")?;
        }
        self.write_u32(def_id.ordinal);
        Ok(())
    }

    fn write_descriptor(&mut self, descriptor: &TypeDescriptor) -> Result<()> {
        match descriptor {
            TypeDescriptor::Unit => self.write_u8(0),
            TypeDescriptor::Bool => self.write_u8(1),
            TypeDescriptor::Int(width) => {
                self.write_u8(2);
                self.write_u8(*width as u8);
            }
            TypeDescriptor::Float(width) => {
                self.write_u8(3);
                self.write_u8(*width as u8);
            }
            TypeDescriptor::String => self.write_u8(4),
            TypeDescriptor::Option(inner) => {
                self.write_u8(5);
                self.write_descriptor(inner)?;
            }
            TypeDescriptor::Result(ok, err) => {
                self.write_u8(6);
                self.write_descriptor(ok)?;
                self.write_descriptor(err)?;
            }
            TypeDescriptor::Array(inner) => {
                self.write_u8(7);
                self.write_descriptor(inner)?;
            }
            TypeDescriptor::FixedArray(inner, len) => {
                self.write_u8(8);
                self.write_u32(*len);
                self.write_descriptor(inner)?;
            }
            TypeDescriptor::Vec(inner) => {
                self.write_u8(9);
                self.write_descriptor(inner)?;
            }
            TypeDescriptor::Struct(schema_id) => {
                self.write_u8(10);
                self.write_u32(*schema_id);
            }
            TypeDescriptor::Any => self.write_u8(11),
            TypeDescriptor::Error => self.write_u8(12),
            TypeDescriptor::Never => self.write_u8(13),
            TypeDescriptor::Enum(schema_id) => {
                self.write_u8(14);
                self.write_u16(*schema_id);
            }
            TypeDescriptor::Function { params, ret } => {
                self.write_u8(15);
                ensure_len(params.len(), MAX_STRUCT_FIELDS, "function parameter count")?;
                self.write_u16(u16::try_from(params.len()).map_err(|_| {
                    BinaryError::LimitExceeded {
                        what: "function parameter count",
                        limit: MAX_STRUCT_FIELDS,
                    }
                })?);
                for param in params {
                    self.write_descriptor(param)?;
                }
                self.write_descriptor(ret)?;
            }
        }
        Ok(())
    }

    fn write_bytes(&mut self, bytes: &[u8]) {
        self.buffer.extend_from_slice(bytes);
    }

    fn write_u8(&mut self, v: u8) {
        self.buffer.push(v);
    }

    fn write_u16(&mut self, v: u16) {
        self.buffer.extend_from_slice(&v.to_le_bytes());
    }

    fn write_u32(&mut self, v: u32) {
        self.buffer.extend_from_slice(&v.to_le_bytes());
    }

    fn write_u64(&mut self, v: u64) {
        self.buffer.extend_from_slice(&v.to_le_bytes());
    }

    fn write_i64(&mut self, v: i64) {
        self.buffer.extend_from_slice(&v.to_le_bytes());
    }
}

fn format_version(func: &Function) -> u16 {
    if requires_v4(func) {
        VERSION
    } else {
        LEGACY_VERSION
    }
}

fn requires_v4(func: &Function) -> bool {
    !func.enum_schemas.is_empty()
        || !func.struct_schemas.is_empty()
        || bytecode_has_enum_opcode(func.bytecode.as_slice())
        || func.nested_functions.iter().any(requires_v4)
}

fn bytecode_has_enum_opcode(bytecode: &[u32]) -> bool {
    let mut ip = 0;
    while let Some(&instruction) = bytecode.get(ip) {
        let opcode = OpCode::from_u8((instruction >> 24) as u8);
        if matches!(
            opcode,
            Some(OpCode::EnumNew | OpCode::EnumTest | OpCode::EnumLoad)
        ) {
            return true;
        }
        ip = ip.saturating_add(1 + opcode.map_or(0, OpCode::extension_words));
    }
    false
}

fn build_bundles_section(bundles: &[NativeBundle]) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    ensure_len(bundles.len(), MAX_NATIVE_BUNDLES, "native bundle count")?;
    write_u32_to(
        &mut buf,
        u32::try_from(bundles.len()).map_err(|_| BinaryError::LimitExceeded {
            what: "native bundle count",
            limit: MAX_NATIVE_BUNDLES,
        })?,
    );
    for bundle in bundles {
        write_string_to(&mut buf, &bundle.name)?;
        write_string_to(&mut buf, &bundle.target)?;
        write_string_to(&mut buf, &bundle.checksum)?;
        write_bytes_to(&mut buf, &bundle.bytes)?;
    }
    ensure_len(buf.len(), MAX_SECTION_LEN, "native bundle section length")?;
    Ok(buf)
}

fn write_u32_to(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_le_bytes());
}

fn write_string_to(buf: &mut Vec<u8>, s: &str) -> Result<()> {
    ensure_len(s.len(), MAX_STRING_LEN, "bundle string length")?;
    write_u32_to(
        buf,
        u32::try_from(s.len()).map_err(|_| BinaryError::LimitExceeded {
            what: "bundle string length",
            limit: MAX_STRING_LEN,
        })?,
    );
    buf.extend_from_slice(s.as_bytes());
    Ok(())
}

fn write_string_u16(buf: &mut Vec<u8>, s: &str, what: &'static str) -> Result<()> {
    ensure_len(s.len(), usize::from(u16::MAX), what)?;
    buf.extend_from_slice(
        &u16::try_from(s.len())
            .map_err(|_| BinaryError::LimitExceeded {
                what,
                limit: usize::from(u16::MAX),
            })?
            .to_le_bytes(),
    );
    buf.extend_from_slice(s.as_bytes());
    Ok(())
}

fn write_bytes_to(buf: &mut Vec<u8>, bytes: &[u8]) -> Result<()> {
    ensure_len(bytes.len(), MAX_SECTION_LEN, "bundle byte length")?;
    write_u32_to(
        buf,
        u32::try_from(bytes.len()).map_err(|_| BinaryError::LimitExceeded {
            what: "bundle byte length",
            limit: MAX_SECTION_LEN,
        })?,
    );
    buf.extend_from_slice(bytes);
    Ok(())
}

// a wire count may never claim more bytes than the input could still supply
fn ensure_within_remaining(
    count: usize,
    min_bytes_per_record: usize,
    remaining: usize,
    what: &'static str,
) -> Result<()> {
    let stride = min_bytes_per_record.max(1);
    if count.saturating_mul(stride) > remaining {
        return Err(BinaryError::LimitExceeded {
            what,
            limit: remaining / stride,
        });
    }
    Ok(())
}

fn remaining_in(cursor: &Cursor<&[u8]>) -> usize {
    let pos = cursor.position() as usize;
    cursor.get_ref().len().saturating_sub(pos)
}

struct BinaryReader<'a> {
    cursor: Cursor<&'a [u8]>,
}

impl<'a> BinaryReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            cursor: Cursor::new(data),
        }
    }

    fn read_program(mut self) -> Result<Function> {
        let mut magic = [0u8; 4];
        self.cursor.read_exact(&mut magic)?;
        if &magic != MAGIC {
            return Err(BinaryError::InvalidMagic);
        }

        let version = self.read_u16()?;
        if version != LEGACY_VERSION && version != VERSION {
            return Err(BinaryError::UnsupportedVersion(version));
        }

        let _flags = self.read_u16()?;
        let _func_count = self.read_u32()?;
        let _reserved = self.read_u32()?;

        let func = self.read_function(0, version)?;
        self.read_sections()?;

        Ok(func)
    }

    fn read_program_with_sections(mut self) -> DeserializeResult {
        let mut magic = [0u8; 4];
        self.cursor.read_exact(&mut magic)?;
        if &magic != MAGIC {
            return Err(BinaryError::InvalidMagic);
        }

        let version = self.read_u16()?;
        if version != LEGACY_VERSION && version != VERSION {
            return Err(BinaryError::UnsupportedVersion(version));
        }

        let _flags = self.read_u16()?;
        let _func_count = self.read_u32()?;
        let _reserved = self.read_u32()?;

        let func = self.read_function(0, version)?;
        let (manifest, bundles) = self.read_sections()?;

        Ok((func, manifest, bundles))
    }

    fn read_function(&mut self, depth: usize, version: u16) -> Result<Function> {
        if depth > MAX_NESTING_DEPTH {
            return Err(BinaryError::LimitExceeded {
                what: "function nesting depth",
                limit: MAX_NESTING_DEPTH,
            });
        }
        let name_len = self.read_u16()? as usize;
        if name_len > MAX_STRING_LEN {
            return Err(BinaryError::LimitExceeded {
                what: "function name length",
                limit: MAX_STRING_LEN,
            });
        }
        self.ensure_records(name_len, 1, "function name length")?;
        let name = if name_len > 0 {
            let mut bytes = vec![0u8; name_len];
            self.cursor.read_exact(&mut bytes)?;
            Some(String::from_utf8(bytes).map_err(|_| BinaryError::InvalidUtf8)?)
        } else {
            None
        };

        let arity = self.read_u16()?;
        let num_registers = self.read_u32()?;
        if num_registers > MAX_REGISTERS {
            return Err(BinaryError::LimitExceeded {
                what: "register count",
                limit: MAX_REGISTERS as usize,
            });
        }
        let struct_schemas = if version >= VERSION {
            self.read_schema_table()?
        } else {
            self.read_legacy_schema_table()?
        };
        let enum_schemas = if version >= VERSION {
            self.read_enum_schema_table()?
        } else {
            Vec::new()
        };
        validate_struct_schemas(&struct_schemas)?;
        if version >= VERSION {
            validate_enum_schemas(&enum_schemas)?;
        }
        validate_schema_descriptors(&struct_schemas, &enum_schemas)?;
        let jit_unsupported_struct = self.read_u8()? != 0;
        let reserved = self.read_u8()?;
        if reserved != 0 {
            return Err(BinaryError::InvalidConstantType(reserved));
        }

        let const_count = self.read_u32()? as usize;
        if const_count > MAX_CONSTANTS {
            return Err(BinaryError::LimitExceeded {
                what: "constants",
                limit: MAX_CONSTANTS,
            });
        }
        self.ensure_records(const_count, 1, "constants")?;
        let mut constants = Vec::with_capacity(const_count);
        for _ in 0..const_count {
            let value = self.read_constant()?;
            constants.push(value);
        }

        let bc_len = self.read_u32()? as usize;
        if bc_len > MAX_BYTECODE_LEN {
            return Err(BinaryError::LimitExceeded {
                what: "bytecode length",
                limit: MAX_BYTECODE_LEN,
            });
        }
        self.ensure_records(bc_len, 4, "bytecode length")?;
        let mut bytecode = Vec::with_capacity(bc_len);
        for _ in 0..bc_len {
            bytecode.push(self.read_u32()?);
        }
        if version < VERSION && bytecode_has_enum_opcode(&bytecode) {
            return Err(BinaryError::InvalidEnumSchema(
                "version 3 cannot contain enum opcode".to_string(),
            ));
        }

        let nested_count = self.read_u16()? as usize;
        if nested_count > MAX_NESTED_FUNCTIONS {
            return Err(BinaryError::LimitExceeded {
                what: "nested functions",
                limit: MAX_NESTED_FUNCTIONS,
            });
        }

        Self::validate_func_markers(&constants, nested_count)?;

        self.ensure_records(nested_count, 30, "nested functions")?;
        let mut nested_functions = Vec::with_capacity(nested_count);
        for _ in 0..nested_count {
            nested_functions.push(self.read_function(depth + 1, version)?);
        }

        let upvalue_count = self.read_u16()? as usize;
        if upvalue_count > MAX_UPVALUE_DESCRIPTORS {
            return Err(BinaryError::LimitExceeded {
                what: "upvalue descriptors",
                limit: MAX_UPVALUE_DESCRIPTORS,
            });
        }
        self.ensure_records(upvalue_count, 3, "upvalue descriptors")?;
        let mut upvalue_descriptors = Vec::with_capacity(upvalue_count);
        for _ in 0..upvalue_count {
            let is_local = self.read_u8()? != 0;
            let index = self.read_u16()?;
            upvalue_descriptors.push(UpvalueDescriptor { is_local, index });
        }

        let lines_count = self.read_u16()? as usize;
        if lines_count > MAX_LINES {
            return Err(BinaryError::LimitExceeded {
                what: "line info entries",
                limit: MAX_LINES,
            });
        }
        self.ensure_records(lines_count, 6, "line info entries")?;
        let mut lines = Vec::with_capacity(lines_count);
        for _ in 0..lines_count {
            let count = self.read_u16()?;
            let line = self.read_u32()?;
            lines.push((count, line));
        }

        let global_names_count = self.read_u16()? as usize;
        if global_names_count > MAX_GLOBAL_NAMES {
            return Err(BinaryError::LimitExceeded {
                what: "global names",
                limit: MAX_GLOBAL_NAMES,
            });
        }
        self.ensure_records(global_names_count, 2, "global names")?;
        let mut global_names = Vec::with_capacity(global_names_count);
        for _ in 0..global_names_count {
            let name_len = self.read_u16()? as usize;
            if name_len > MAX_STRING_LEN {
                return Err(BinaryError::LimitExceeded {
                    what: "global name length",
                    limit: MAX_STRING_LEN,
                });
            }
            self.ensure_records(name_len, 1, "global name length")?;
            let name = if name_len > 0 {
                let mut bytes = vec![0u8; name_len];
                self.cursor.read_exact(&mut bytes)?;
                String::from_utf8(bytes).map_err(|_| {
                    BinaryError::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "Invalid UTF-8 in global name",
                    ))
                })?
            } else {
                String::new()
            };
            global_names.push(name);
        }

        let mut func = Function::new(name, arity);
        func.num_registers = num_registers;
        func.set_bytecode(bytecode);
        func.constants = constants;
        func.nested_functions = nested_functions;
        func.upvalue_descriptors = upvalue_descriptors;
        func.lines = lines;
        func.global_layout = GlobalLayout::new(global_names);
        func.struct_schemas = struct_schemas;
        func.enum_schemas = enum_schemas;
        func.jit_unsupported_struct = jit_unsupported_struct;
        func.compute_global_layout_hash();

        Ok(func)
    }

    fn validate_func_markers(constants: &[Constant], nested_count: usize) -> Result<()> {
        for constant in constants {
            if let Some(func_idx) = constant.as_nested_fn_marker()
                && func_idx >= nested_count
            {
                return Err(BinaryError::InvalidNestedFunctionIndex {
                    index: func_idx,
                    max: nested_count.saturating_sub(1),
                });
            }
        }
        Ok(())
    }

    fn read_constant(&mut self) -> Result<Constant> {
        let tag = self.read_u8()?;
        match tag {
            0 => Ok(Constant::Null),
            1 => {
                let b = self.read_u8()? != 0;
                Ok(Constant::Bool(b))
            }
            2 => {
                let n = self.read_i64()?;
                Ok(Constant::Int(n))
            }
            3 => {
                let bits = self.read_u64()?;
                Ok(Constant::Float(bits))
            }
            4 => {
                let len = self.read_u32()? as usize;
                if len > MAX_STRING_LEN {
                    return Err(BinaryError::LimitExceeded {
                        what: "string length",
                        limit: MAX_STRING_LEN,
                    });
                }
                self.ensure_records(len, 1, "string length")?;
                let mut bytes = vec![0u8; len];
                self.cursor.read_exact(&mut bytes)?;
                let s = String::from_utf8(bytes).map_err(|_| BinaryError::InvalidUtf8)?;
                Ok(Constant::String(s))
            }
            5 => Ok(Constant::NestedFunction(self.read_u32()?)),
            _ => Err(BinaryError::InvalidConstantType(tag)),
        }
    }

    fn read_schema_table(&mut self) -> Result<Vec<StructSchema>> {
        let count = usize::from(self.read_u16()?);
        let reserved = self.read_u16()?;
        if reserved != 0 {
            return Err(BinaryError::InvalidConstantType(reserved as u8));
        }
        let start = self.cursor.position() as usize;
        if count > MAX_STRUCT_SCHEMAS {
            return Err(BinaryError::LimitExceeded {
                what: "struct schema count",
                limit: MAX_STRUCT_SCHEMAS,
            });
        }
        self.ensure_records(count, 16, "struct schema count")?;
        let mut schemas = Vec::with_capacity(count);
        for _ in 0..count {
            let schema_id = self.read_u32()?;
            let ctor = self.read_def_id()?;
            let type_arg_count = usize::from(self.read_u16()?);
            let field_count = usize::from(self.read_u16()?);
            if type_arg_count > MAX_STRUCT_FIELDS {
                return Err(BinaryError::LimitExceeded {
                    what: "struct type argument count",
                    limit: MAX_STRUCT_FIELDS,
                });
            }
            if field_count > MAX_STRUCT_FIELDS {
                return Err(BinaryError::LimitExceeded {
                    what: "struct field count",
                    limit: MAX_STRUCT_FIELDS,
                });
            }
            self.ensure_records(type_arg_count, 1, "struct type argument count")?;
            let mut type_args = Vec::with_capacity(type_arg_count);
            for _ in 0..type_arg_count {
                type_args.push(self.read_descriptor(0)?);
            }
            self.ensure_records(field_count, 5, "struct field count")?;
            let mut fields = Vec::with_capacity(field_count);
            for _ in 0..field_count {
                let offset = self.read_u16()?;
                let field_name = self.read_string_u16("struct field name")?;
                fields.push(StructFieldSchema {
                    offset,
                    name: field_name,
                    ty: self.read_descriptor(0)?,
                });
                ensure_len(
                    (self.cursor.position() as usize).saturating_sub(start),
                    MAX_SCHEMA_TABLE_BYTES,
                    "struct schema table",
                )?;
            }
            schemas.push(StructSchema {
                schema_id,
                ctor,
                type_args: type_args.into_boxed_slice(),
                fields: fields.into_boxed_slice(),
            });
        }
        validate_struct_schemas(&schemas)?;
        ensure_len(
            (self.cursor.position() as usize).saturating_sub(start),
            MAX_SCHEMA_TABLE_BYTES,
            "struct schema table",
        )?;
        Ok(schemas)
    }

    fn read_legacy_schema_table(&mut self) -> Result<Vec<StructSchema>> {
        let count = usize::from(self.read_u16()?);
        let reserved = self.read_u16()?;
        if reserved != 0 {
            return Err(BinaryError::InvalidConstantType(reserved as u8));
        }
        if count > MAX_STRUCT_SCHEMAS {
            return Err(BinaryError::LimitExceeded {
                what: "struct schema count",
                limit: MAX_STRUCT_SCHEMAS,
            });
        }
        // the smallest legacy struct schema record is a u16 name length plus a u16 field count and reserved pair
        self.ensure_records(count, 6, "struct schema count")?;
        let start = self.cursor.position();
        let names = self.read_legacy_records(count, None)?;
        let mut resolved = HashMap::with_capacity(names.len());
        for (index, (name, _)) in names.iter().enumerate() {
            let id = u32::try_from(index).unwrap_or(u32::MAX);
            if resolved.insert(name.clone(), id).is_some() {
                return Err(BinaryError::InvalidStructSchema(format!(
                    "version 3 struct schema name {name} is duplicated"
                )));
            }
        }
        self.cursor.set_position(start);
        let records = self.read_legacy_records(count, Some(&resolved))?;
        Ok(records
            .into_iter()
            .enumerate()
            .map(|(index, (name, fields))| {
                StructSchema::with_identity(
                    u32::try_from(index).unwrap_or(u32::MAX),
                    DefId::from_display_name(&name, u32::try_from(index).unwrap_or(u32::MAX)),
                    Vec::new(),
                    fields,
                )
            })
            .collect())
    }

    fn read_legacy_records(
        &mut self,
        count: usize,
        names: Option<&HashMap<String, u32>>,
    ) -> Result<Vec<LegacyStructRecord>> {
        let start = self.cursor.position() as usize;
        let mut records = Vec::with_capacity(count);
        for _ in 0..count {
            let name = self.read_string_u16("struct schema name")?;
            let field_count = usize::from(self.read_u16()?);
            let field_reserved = self.read_u16()?;
            if field_reserved != 0 {
                return Err(BinaryError::InvalidConstantType(field_reserved as u8));
            }
            if field_count > MAX_STRUCT_FIELDS {
                return Err(BinaryError::LimitExceeded {
                    what: "struct field count",
                    limit: MAX_STRUCT_FIELDS,
                });
            }
            self.ensure_records(field_count, 3, "struct field count")?;
            let mut fields = Vec::with_capacity(field_count);
            for offset in 0..field_count {
                let field_name = self.read_string_u16("struct field name")?;
                fields.push(StructFieldSchema {
                    offset: u16::try_from(offset).unwrap_or(u16::MAX),
                    name: field_name,
                    ty: self.read_legacy_descriptor(0, names)?,
                });
                ensure_len(
                    (self.cursor.position() as usize).saturating_sub(start),
                    MAX_SCHEMA_TABLE_BYTES,
                    "struct schema table",
                )?;
            }
            records.push((name, fields));
        }
        ensure_len(
            (self.cursor.position() as usize).saturating_sub(start),
            MAX_SCHEMA_TABLE_BYTES,
            "struct schema table",
        )?;
        Ok(records)
    }

    fn read_enum_schema_table(&mut self) -> Result<Vec<EnumSchema>> {
        let count = usize::from(self.read_u16()?);
        let reserved = self.read_u16()?;
        if reserved != 0 {
            return Err(BinaryError::InvalidConstantType(reserved as u8));
        }
        let start = self.cursor.position() as usize;
        if count > MAX_ENUM_SCHEMAS {
            return Err(BinaryError::LimitExceeded {
                what: "enum schema count",
                limit: MAX_ENUM_SCHEMAS,
            });
        }
        self.ensure_records(count, 18, "enum schema count")?;
        let mut schemas = Vec::with_capacity(count);
        for _ in 0..count {
            let schema_id = self.read_u16()?;
            let def_id = self.read_def_id()?;
            let name = self.read_string_u16("enum schema name")?;
            let arity = self.read_u16()?;
            let type_arg_count = usize::from(self.read_u16()?);
            let variant_count = usize::from(self.read_u16()?);
            if type_arg_count > MAX_ENUM_FIELDS {
                return Err(BinaryError::LimitExceeded {
                    what: "enum type argument count",
                    limit: MAX_ENUM_FIELDS,
                });
            }
            if variant_count > MAX_ENUM_VARIANTS {
                return Err(BinaryError::LimitExceeded {
                    what: "enum variant count",
                    limit: MAX_ENUM_VARIANTS,
                });
            }
            self.ensure_records(type_arg_count, 1, "enum type argument count")?;
            let mut type_args = Vec::with_capacity(type_arg_count);
            for _ in 0..type_arg_count {
                type_args.push(self.read_descriptor(0)?);
            }
            self.ensure_records(variant_count, 8, "enum variant count")?;
            let mut variants = Vec::with_capacity(variant_count);
            for _ in 0..variant_count {
                let variant_id = self.read_u16()?;
                let variant_name = self.read_string_u16("enum variant name")?;
                let field_count = usize::from(self.read_u16()?);
                let field_kind = self.read_u8()?;
                let field_reserved = self.read_u8()?;
                if field_reserved != 0 || field_kind > 2 {
                    return Err(BinaryError::InvalidEnumSchema(
                        "invalid enum field kind or reserved byte".to_string(),
                    ));
                }
                if field_count > MAX_ENUM_FIELDS {
                    return Err(BinaryError::LimitExceeded {
                        what: "enum field count",
                        limit: MAX_ENUM_FIELDS,
                    });
                }
                if field_kind == 0 && field_count != 0 {
                    return Err(BinaryError::InvalidEnumSchema(
                        "unit enum variant has fields".to_string(),
                    ));
                }
                self.ensure_records(field_count, 5, "enum field count")?;
                let mut fields = Vec::with_capacity(field_count);
                for _ in 0..field_count {
                    let offset = self.read_u16()?;
                    let field_name = if field_kind == 2 {
                        Some(self.read_string_u16("enum field name")?)
                    } else {
                        let name_len = self.read_u16()?;
                        if name_len != 0 {
                            return Err(BinaryError::InvalidEnumSchema(
                                "tuple enum field has a non-empty name".to_string(),
                            ));
                        }
                        None
                    };
                    fields.push(EnumFieldSchema {
                        offset,
                        name: field_name,
                        ty: self.read_descriptor(0)?,
                    });
                    ensure_len(
                        (self.cursor.position() as usize).saturating_sub(start),
                        MAX_SCHEMA_TABLE_BYTES,
                        "enum schema table",
                    )?;
                }
                variants.push(EnumVariantSchema {
                    variant_id,
                    name: variant_name,
                    fields: fields.into_boxed_slice(),
                });
            }
            schemas.push(EnumSchema {
                schema_id,
                name,
                def_id,
                arity,
                type_args: type_args.into_boxed_slice(),
                variants: variants.into_boxed_slice(),
            });
        }
        validate_enum_schemas(&schemas)?;
        ensure_len(
            (self.cursor.position() as usize).saturating_sub(start),
            MAX_SCHEMA_TABLE_BYTES,
            "enum schema table",
        )?;
        Ok(schemas)
    }

    fn read_def_id(&mut self) -> Result<DefId> {
        let package = self.read_string_u16("schema package")?;
        let module_count = usize::from(self.read_u16()?);
        if package.is_empty() || module_count == 0 || module_count > MAX_ENUM_FIELDS {
            return Err(BinaryError::InvalidEnumSchema(
                "schema definition path is empty or too large".to_string(),
            ));
        }
        self.ensure_records(module_count, 2, "schema module segment count")?;
        let mut module = Vec::with_capacity(module_count);
        for _ in 0..module_count {
            let segment = self.read_string_u16("schema module segment")?;
            if segment.is_empty() {
                return Err(BinaryError::InvalidEnumSchema(
                    "schema module segment is empty".to_string(),
                ));
            }
            module.push(segment);
        }
        Ok(DefId {
            package,
            module: module.into_boxed_slice(),
            ordinal: self.read_u32()?,
        })
    }

    fn read_descriptor(&mut self, depth: usize) -> Result<TypeDescriptor> {
        if depth > MAX_NESTING_DEPTH {
            return Err(BinaryError::LimitExceeded {
                what: "struct descriptor depth",
                limit: MAX_NESTING_DEPTH,
            });
        }
        Ok(match self.read_u8()? {
            0 => TypeDescriptor::Unit,
            1 => TypeDescriptor::Bool,
            2 => TypeDescriptor::Int(
                IntWidth::from_u8(self.read_u8()?).ok_or(BinaryError::InvalidConstantType(2))?,
            ),
            3 => TypeDescriptor::Float(
                FloatWidth::from_u8(self.read_u8()?).ok_or(BinaryError::InvalidConstantType(3))?,
            ),
            4 => TypeDescriptor::String,
            5 => TypeDescriptor::Option(Box::new(self.read_descriptor(depth + 1)?)),
            6 => TypeDescriptor::Result(
                Box::new(self.read_descriptor(depth + 1)?),
                Box::new(self.read_descriptor(depth + 1)?),
            ),
            7 => TypeDescriptor::Array(Box::new(self.read_descriptor(depth + 1)?)),
            8 => {
                let len = self.read_u32()?;
                TypeDescriptor::FixedArray(Box::new(self.read_descriptor(depth + 1)?), len)
            }
            9 => TypeDescriptor::Vec(Box::new(self.read_descriptor(depth + 1)?)),
            10 => TypeDescriptor::Struct(self.read_u32()?),
            11 => TypeDescriptor::Any,
            12 => TypeDescriptor::Error,
            13 => TypeDescriptor::Never,
            14 => TypeDescriptor::Enum(self.read_u16()?),
            15 => {
                let count = usize::from(self.read_u16()?);
                if count > MAX_STRUCT_FIELDS {
                    return Err(BinaryError::LimitExceeded {
                        what: "function parameter count",
                        limit: MAX_STRUCT_FIELDS,
                    });
                }
                self.ensure_records(count, 1, "function parameter count")?;
                let mut params = Vec::with_capacity(count);
                for _ in 0..count {
                    params.push(self.read_descriptor(depth + 1)?);
                }
                TypeDescriptor::Function {
                    params: params.into_boxed_slice(),
                    ret: Box::new(self.read_descriptor(depth + 1)?),
                }
            }
            tag => return Err(BinaryError::InvalidConstantType(tag)),
        })
    }

    fn read_legacy_descriptor(
        &mut self,
        depth: usize,
        names: Option<&HashMap<String, u32>>,
    ) -> Result<TypeDescriptor> {
        if depth > MAX_NESTING_DEPTH {
            return Err(BinaryError::LimitExceeded {
                what: "struct descriptor depth",
                limit: MAX_NESTING_DEPTH,
            });
        }
        Ok(match self.read_u8()? {
            0 => TypeDescriptor::Unit,
            1 => TypeDescriptor::Bool,
            2 => TypeDescriptor::Int(
                IntWidth::from_u8(self.read_u8()?).ok_or(BinaryError::InvalidConstantType(2))?,
            ),
            3 => TypeDescriptor::Float(
                FloatWidth::from_u8(self.read_u8()?).ok_or(BinaryError::InvalidConstantType(3))?,
            ),
            4 => TypeDescriptor::String,
            5 => TypeDescriptor::Option(Box::new(self.read_legacy_descriptor(depth + 1, names)?)),
            6 => TypeDescriptor::Result(
                Box::new(self.read_legacy_descriptor(depth + 1, names)?),
                Box::new(self.read_legacy_descriptor(depth + 1, names)?),
            ),
            7 => TypeDescriptor::Array(Box::new(self.read_legacy_descriptor(depth + 1, names)?)),
            8 => {
                let len = self.read_u32()?;
                TypeDescriptor::FixedArray(
                    Box::new(self.read_legacy_descriptor(depth + 1, names)?),
                    len,
                )
            }
            9 => TypeDescriptor::Vec(Box::new(self.read_legacy_descriptor(depth + 1, names)?)),
            10 => {
                let name = self.read_string_u16("struct descriptor name")?;
                match names {
                    Some(resolved) => {
                        TypeDescriptor::Struct(resolved.get(&name).copied().ok_or_else(|| {
                            BinaryError::InvalidStructSchema(format!(
                                "version 3 struct descriptor names unknown schema {name}"
                            ))
                        })?)
                    }
                    None => TypeDescriptor::Struct(0),
                }
            }
            11 => TypeDescriptor::Any,
            12 => TypeDescriptor::Error,
            13 => TypeDescriptor::Never,
            tag => return Err(BinaryError::InvalidConstantType(tag)),
        })
    }

    fn read_string_u16(&mut self, what: &'static str) -> Result<String> {
        let len = usize::from(self.read_u16()?);
        if len > MAX_STRING_LEN {
            return Err(BinaryError::LimitExceeded {
                what,
                limit: MAX_STRING_LEN,
            });
        }
        self.ensure_records(len, 1, what)?;
        let mut bytes = vec![0u8; len];
        self.cursor.read_exact(&mut bytes)?;
        String::from_utf8(bytes).map_err(|_| BinaryError::InvalidUtf8)
    }

    fn read_sections(&mut self) -> Result<(Option<Vec<u8>>, Vec<NativeBundle>)> {
        let mut manifest = None;
        let mut bundles = Vec::new();
        while self.remaining() > 0 {
            let tag = self.read_u32()?;
            let len = self.read_u32()? as usize;
            if len > MAX_SECTION_LEN {
                return Err(BinaryError::LimitExceeded {
                    what: "section length",
                    limit: MAX_SECTION_LEN,
                });
            }
            let data = self.read_bytes(len, "section length")?;
            match tag {
                SECTION_MANIFEST => {
                    manifest = Some(data);
                }
                SECTION_BUNDLES => {
                    let mut parsed = parse_bundles_section(&data)?;
                    bundles.append(&mut parsed);
                }
                _ => {}
            }
        }
        Ok((manifest, bundles))
    }

    fn remaining(&self) -> usize {
        remaining_in(&self.cursor)
    }

    fn ensure_records(
        &self,
        count: usize,
        min_bytes_per_record: usize,
        what: &'static str,
    ) -> Result<()> {
        ensure_within_remaining(count, min_bytes_per_record, self.remaining(), what)
    }

    fn read_bytes(&mut self, len: usize, what: &'static str) -> Result<Vec<u8>> {
        self.ensure_records(len, 1, what)?;
        let mut buf = vec![0u8; len];
        self.cursor.read_exact(&mut buf)?;
        Ok(buf)
    }

    fn read_u8(&mut self) -> Result<u8> {
        let mut buf = [0u8; 1];
        self.cursor.read_exact(&mut buf)?;
        Ok(buf[0])
    }

    fn read_u16(&mut self) -> Result<u16> {
        let mut buf = [0u8; 2];
        self.cursor.read_exact(&mut buf)?;
        Ok(u16::from_le_bytes(buf))
    }

    fn read_u32(&mut self) -> Result<u32> {
        let mut buf = [0u8; 4];
        self.cursor.read_exact(&mut buf)?;
        Ok(u32::from_le_bytes(buf))
    }

    fn read_u64(&mut self) -> Result<u64> {
        let mut buf = [0u8; 8];
        self.cursor.read_exact(&mut buf)?;
        Ok(u64::from_le_bytes(buf))
    }

    fn read_i64(&mut self) -> Result<i64> {
        let mut buf = [0u8; 8];
        self.cursor.read_exact(&mut buf)?;
        Ok(i64::from_le_bytes(buf))
    }
}

fn validate_enum_schemas(schemas: &[EnumSchema]) -> Result<()> {
    let names: HashSet<&str> = schemas.iter().map(|schema| schema.name.as_str()).collect();
    let ids: HashSet<u16> = schemas.iter().map(|schema| schema.schema_id).collect();
    let identities: HashSet<(&DefId, &[TypeDescriptor])> = schemas
        .iter()
        .map(|schema| (&schema.def_id, schema.type_args.as_ref()))
        .collect();
    if schemas
        .iter()
        .any(|schema| schema.arity != u16::try_from(schema.type_args.len()).unwrap_or(u16::MAX))
    {
        return Err(BinaryError::InvalidEnumSchema(
            "enum type parameter arity does not match concrete type arguments".to_string(),
        ));
    }
    if names.len() != schemas.len()
        || ids.len() != schemas.len()
        || identities.len() != schemas.len()
        || schemas.iter().any(|schema| schema.name.is_empty())
        || schemas.iter().any(|schema| {
            schema.def_id.package.is_empty()
                || schema.def_id.module.is_empty()
                || schema.def_id.module.iter().any(String::is_empty)
        })
        || schemas
            .iter()
            .enumerate()
            .any(|(index, schema)| schema.schema_id != u16::try_from(index).unwrap_or(u16::MAX))
    {
        return Err(BinaryError::InvalidEnumSchema(
            "schema ids and names must be non-empty, unique, and ordered".to_string(),
        ));
    }
    for schema in schemas {
        for type_arg in &schema.type_args {
            validate_enum_descriptor(type_arg, &ids, 0)?;
        }
        let mut variants = HashSet::new();
        for (variant_index, variant) in schema.variants.iter().enumerate() {
            if variant.name.is_empty()
                || !variants.insert(variant.name.as_str())
                || variant.variant_id != u16::try_from(variant_index).unwrap_or(u16::MAX)
            {
                return Err(BinaryError::InvalidEnumSchema(format!(
                    "{} has duplicate or empty variant name",
                    schema.name
                )));
            }
            let named = variant
                .fields
                .iter()
                .filter(|field| field.name.is_some())
                .count();
            if named != 0 && named != variant.fields.len() {
                return Err(BinaryError::InvalidEnumSchema(format!(
                    "{}::{} mixes named and tuple fields",
                    schema.name, variant.name
                )));
            }
            let mut fields = HashSet::new();
            for (field_index, field) in variant.fields.iter().enumerate() {
                if field.offset != u16::try_from(field_index).unwrap_or(u16::MAX) {
                    return Err(BinaryError::InvalidEnumSchema(format!(
                        "{}::{} has unordered field offsets",
                        schema.name, variant.name
                    )));
                }
                if let Some(name) = &field.name
                    && (name.is_empty() || !fields.insert(name.as_str()))
                {
                    return Err(BinaryError::InvalidEnumSchema(format!(
                        "{}::{} has duplicate or empty field name",
                        schema.name, variant.name
                    )));
                }
                validate_enum_descriptor(&field.ty, &ids, 0)?;
            }
        }
    }
    Ok(())
}

fn validate_struct_schemas(schemas: &[StructSchema]) -> Result<()> {
    let ids: HashSet<u32> = schemas.iter().map(|schema| schema.schema_id).collect();
    let identities: HashSet<(&DefId, &[TypeDescriptor])> = schemas
        .iter()
        .map(|schema| (&schema.ctor, schema.type_args.as_ref()))
        .collect();
    if ids.len() != schemas.len()
        || schemas
            .iter()
            .enumerate()
            .any(|(index, schema)| schema.schema_id != u32::try_from(index).unwrap_or(u32::MAX))
    {
        return Err(BinaryError::InvalidStructSchema(
            "struct schema ids must be unique and ordered from zero".to_string(),
        ));
    }
    if identities.len() != schemas.len()
        || schemas.iter().any(|schema| {
            schema.ctor.package.is_empty()
                || schema.ctor.module.is_empty()
                || schema.ctor.module.iter().any(String::is_empty)
        })
    {
        return Err(BinaryError::InvalidStructSchema(
            "struct definition paths must be non-empty and unique per instance".to_string(),
        ));
    }
    for schema in schemas {
        let mut names = HashSet::with_capacity(schema.fields.len());
        for (index, field) in schema.fields.iter().enumerate() {
            if field.offset != u16::try_from(index).unwrap_or(u16::MAX) {
                return Err(BinaryError::InvalidStructSchema(format!(
                    "{} has unordered field offsets",
                    schema.display_name()
                )));
            }
            if field.name.is_empty() || !names.insert(field.name.as_str()) {
                return Err(BinaryError::InvalidStructSchema(format!(
                    "{} has a duplicate or empty field name",
                    schema.display_name()
                )));
            }
        }
    }
    Ok(())
}

fn validate_schema_descriptors(
    schemas: &[StructSchema],
    enum_schemas: &[EnumSchema],
) -> Result<()> {
    let struct_count = u32::try_from(schemas.len()).unwrap_or(u32::MAX);
    let enum_ids: HashSet<u16> = enum_schemas.iter().map(|schema| schema.schema_id).collect();
    for schema in schemas {
        for type_arg in &schema.type_args {
            validate_descriptor_ids(type_arg, struct_count, &enum_ids, 0)?;
        }
        for field in &schema.fields {
            validate_descriptor_ids(&field.ty, struct_count, &enum_ids, 0)?;
        }
    }
    for schema in enum_schemas {
        for type_arg in &schema.type_args {
            validate_descriptor_ids(type_arg, struct_count, &enum_ids, 0)?;
        }
        for variant in &schema.variants {
            for field in &variant.fields {
                validate_descriptor_ids(&field.ty, struct_count, &enum_ids, 0)?;
            }
        }
    }
    Ok(())
}

fn validate_descriptor_ids(
    descriptor: &TypeDescriptor,
    struct_count: u32,
    enum_ids: &HashSet<u16>,
    depth: usize,
) -> Result<()> {
    if depth > MAX_NESTING_DEPTH {
        return Err(BinaryError::LimitExceeded {
            what: "struct descriptor depth",
            limit: MAX_NESTING_DEPTH,
        });
    }
    match descriptor {
        TypeDescriptor::Struct(schema_id) if *schema_id >= struct_count => Err(
            BinaryError::InvalidStructSchema(format!("unknown struct schema id {schema_id}")),
        ),
        TypeDescriptor::Enum(schema_id) if !enum_ids.contains(schema_id) => {
            Err(BinaryError::InvalidEnumSchema(format!(
                "field refers to unknown schema id {schema_id}"
            )))
        }
        TypeDescriptor::Option(inner)
        | TypeDescriptor::Array(inner)
        | TypeDescriptor::FixedArray(inner, _)
        | TypeDescriptor::Vec(inner) => {
            validate_descriptor_ids(inner, struct_count, enum_ids, depth + 1)
        }
        TypeDescriptor::Result(ok, err) => {
            validate_descriptor_ids(ok, struct_count, enum_ids, depth + 1)?;
            validate_descriptor_ids(err, struct_count, enum_ids, depth + 1)
        }
        TypeDescriptor::Function { params, ret } => {
            for param in params {
                validate_descriptor_ids(param, struct_count, enum_ids, depth + 1)?;
            }
            validate_descriptor_ids(ret, struct_count, enum_ids, depth + 1)
        }
        TypeDescriptor::Any | TypeDescriptor::Never => Err(BinaryError::InvalidStructSchema(
            "struct descriptors must be concrete".to_string(),
        )),
        _ => Ok(()),
    }
}

fn validate_enum_descriptor(
    descriptor: &TypeDescriptor,
    ids: &HashSet<u16>,
    depth: usize,
) -> Result<()> {
    if depth > MAX_NESTING_DEPTH {
        return Err(BinaryError::LimitExceeded {
            what: "enum descriptor depth",
            limit: MAX_NESTING_DEPTH,
        });
    }
    match descriptor {
        TypeDescriptor::Any | TypeDescriptor::Never => Err(BinaryError::InvalidEnumSchema(
            "enum descriptors must be concrete".to_string(),
        )),
        TypeDescriptor::Enum(schema_id) if !ids.contains(schema_id) => {
            Err(BinaryError::InvalidEnumSchema(format!(
                "field refers to unknown schema id {schema_id}"
            )))
        }
        TypeDescriptor::Option(inner)
        | TypeDescriptor::Array(inner)
        | TypeDescriptor::FixedArray(inner, _)
        | TypeDescriptor::Vec(inner) => validate_enum_descriptor(inner, ids, depth + 1),
        TypeDescriptor::Result(ok, err) => {
            validate_enum_descriptor(ok, ids, depth + 1)?;
            validate_enum_descriptor(err, ids, depth + 1)
        }
        TypeDescriptor::Function { params, ret } => {
            for param in params {
                validate_enum_descriptor(param, ids, depth + 1)?;
            }
            validate_enum_descriptor(ret, ids, depth + 1)
        }
        _ => Ok(()),
    }
}

fn parse_bundles_section(data: &[u8]) -> Result<Vec<NativeBundle>> {
    let mut cursor = Cursor::new(data);
    let count = read_u32_from(&mut cursor)? as usize;
    ensure_len(count, MAX_NATIVE_BUNDLES, "native bundle count")?;
    ensure_within_remaining(count, 16, remaining_in(&cursor), "native bundle count")?;
    let mut bundles = Vec::with_capacity(count);
    for _ in 0..count {
        let name = read_string_from(&mut cursor, "bundle name")?;
        let target = read_string_from(&mut cursor, "bundle target")?;
        let checksum = read_string_from(&mut cursor, "bundle checksum")?;
        let bytes = read_bytes_from(&mut cursor, "bundle bytes")?;
        bundles.push(NativeBundle {
            name,
            target,
            checksum,
            bytes,
        });
    }
    Ok(bundles)
}

fn read_u32_from(cursor: &mut Cursor<&[u8]>) -> Result<u32> {
    let mut buf = [0u8; 4];
    cursor.read_exact(&mut buf)?;
    Ok(u32::from_le_bytes(buf))
}

fn read_string_from(cursor: &mut Cursor<&[u8]>, what: &'static str) -> Result<String> {
    let len = read_u32_from(cursor)? as usize;
    if len > MAX_STRING_LEN {
        return Err(BinaryError::LimitExceeded {
            what,
            limit: MAX_STRING_LEN,
        });
    }
    ensure_within_remaining(len, 1, remaining_in(cursor), what)?;
    let mut buf = vec![0u8; len];
    cursor.read_exact(&mut buf)?;
    String::from_utf8(buf).map_err(|_| BinaryError::InvalidUtf8)
}

fn read_bytes_from(cursor: &mut Cursor<&[u8]>, what: &'static str) -> Result<Vec<u8>> {
    let len = read_u32_from(cursor)? as usize;
    if len > MAX_SECTION_LEN {
        return Err(BinaryError::LimitExceeded {
            what,
            limit: MAX_SECTION_LEN,
        });
    }
    ensure_within_remaining(len, 1, remaining_in(cursor), what)?;
    let mut buf = vec![0u8; len];
    cursor.read_exact(&mut buf)?;
    Ok(buf)
}

fn ensure_len(len: usize, limit: usize, what: &'static str) -> Result<()> {
    if len > limit {
        return Err(BinaryError::LimitExceeded { what, limit });
    }
    Ok(())
}

fn count_functions(func: &Function) -> Result<u32> {
    let mut count = 0u32;
    let mut pending = vec![func];
    while let Some(function) = pending.pop() {
        count = count.checked_add(1).ok_or(BinaryError::LimitExceeded {
            what: "function count",
            limit: usize::MAX,
        })?;
        pending.extend(function.nested_functions.iter());
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_program() -> Vec<u8> {
        serialize(&Function::new(None, 0)).expect("empty function serializes")
    }

    fn with_section(tag: u32, payload: &[u8]) -> Vec<u8> {
        let mut bytes = minimal_program();
        bytes.extend_from_slice(&tag.to_le_bytes());
        let len = u32::try_from(payload.len()).expect("payload length fits u32");
        bytes.extend_from_slice(&len.to_le_bytes());
        bytes.extend_from_slice(payload);
        bytes
    }

    fn header_up_to_constant_count(const_count: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&LEGACY_VERSION.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.push(0);
        bytes.push(0);
        bytes.extend_from_slice(&const_count.to_le_bytes());
        bytes
    }

    fn expect_limit(result: Result<impl std::fmt::Debug>, expected: &str) {
        match result {
            Err(BinaryError::LimitExceeded { what, .. }) => assert_eq!(what, expected),
            Err(other) => panic!("expected LimitExceeded({expected}), got {other:?}"),
            Ok(value) => panic!("expected LimitExceeded({expected}), got Ok({value:?})"),
        }
    }

    #[test]
    fn empty_program_round_trips() {
        let bytes = minimal_program();
        let func = deserialize(&bytes).expect("empty program deserializes");
        assert_eq!(func.arity, 0);
    }

    #[test]
    fn bundle_count_beyond_remaining_input_is_rejected() {
        let bytes = with_section(SECTION_BUNDLES, &u32::MAX.to_le_bytes());
        expect_limit(deserialize_with_manifest(&bytes), "native bundle count");
    }

    #[test]
    fn section_length_beyond_remaining_input_is_rejected() {
        let mut bytes = minimal_program();
        bytes.extend_from_slice(&SECTION_MANIFEST.to_le_bytes());
        bytes.extend_from_slice(&0x0f00_0000u32.to_le_bytes());
        expect_limit(deserialize_with_manifest(&bytes), "section length");
    }

    #[test]
    fn bundle_string_length_beyond_remaining_input_is_rejected() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&1u32.to_le_bytes());
        payload.extend_from_slice(&900_000u32.to_le_bytes());
        payload.extend_from_slice(&[0u8; 12]);
        let bytes = with_section(SECTION_BUNDLES, &payload);
        expect_limit(deserialize_with_manifest(&bytes), "bundle name");
    }

    #[test]
    fn constant_count_beyond_remaining_input_is_rejected() {
        let bytes = header_up_to_constant_count(900_000);
        expect_limit(deserialize(&bytes), "constants");
    }

    #[test]
    fn global_name_count_beyond_remaining_input_is_rejected() {
        let mut bytes = header_up_to_constant_count(0);
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&40_000u16.to_le_bytes());
        expect_limit(deserialize(&bytes), "global names");
    }

    #[test]
    fn struct_schema_count_beyond_remaining_input_is_rejected() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&LEGACY_VERSION.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&40_000u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        expect_limit(deserialize(&bytes), "struct schema count");
    }

    #[test]
    fn trailing_bytes_after_a_program_are_rejected() {
        let mut bytes = minimal_program();
        bytes.extend_from_slice(&[0xde, 0xad, 0xbe]);
        assert!(deserialize(&bytes).is_err(), "trailing bytes must not pass");
    }
}
