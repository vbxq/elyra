//! Binary serialization for .avbc format

use crate::bytecode::{Constant, Function, GlobalLayout, UpvalueDescriptor};
use std::io::{self, Cursor, Read};
use thiserror::Error;

/// Magic bytes for .avbc files
pub const MAGIC: &[u8; 4] = b"VBXQ";

/// Current format version
pub const VERSION: u16 = 2;

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

const SECTION_MANIFEST: u32 = u32::from_le_bytes(*b"MANF");
const SECTION_BUNDLES: u32 = u32::from_le_bytes(*b"NBND");

/// Result type for deserialization with manifest and bundles
pub type DeserializeResult = Result<(Function, Option<Vec<u8>>, Vec<NativeBundle>)>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeBundle {
    pub name: String,
    pub target: String,
    pub checksum: String,
    pub bytes: Vec<u8>,
}

/// Binary format errors
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

/// Serialize a function to .avbc binary format
pub fn serialize(func: &Function) -> Result<Vec<u8>> {
    let mut writer = BinaryWriter::new();
    writer.write_program(func)?;
    Ok(writer.into_bytes())
}

/// Serialize a function to .avbc with optional manifest and bundles.
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

/// Deserialize .avbc binary format to a function
pub fn deserialize(data: &[u8]) -> Result<Function> {
    let reader = BinaryReader::new(data);
    reader.read_program()
}

/// Deserialize .avbc binary format to a function, optional manifest, and bundles.
pub fn deserialize_with_manifest(data: &[u8]) -> DeserializeResult {
    let reader = BinaryReader::new(data);
    reader.read_program_with_sections()
}

/// Binary writer for .avbc format
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
        self.write_u16(VERSION);
        self.write_u16(0); // Flags (reserved)

        let func_count = count_functions(func)?;
        self.write_u32(func_count);
        self.write_u32(0); // Reserved

        self.write_function(func, 0)
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

    fn write_function(&mut self, func: &Function, depth: usize) -> Result<()> {
        ensure_len(depth, MAX_NESTING_DEPTH, "function nesting depth")?;
        // Name
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

        // Metadata
        self.write_u16(func.arity);
        self.write_u32(func.num_registers);

        // Constants
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

        // Bytecode
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

        // Nested functions
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
            self.write_function(nested, depth.saturating_add(1))?;
        }

        // Upvalue descriptors (needed for closures)
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

        // Line info (RLE)
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

        // Global names (for indexed global access)
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

/// Binary reader for .avbc format
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
        // Header
        let mut magic = [0u8; 4];
        self.cursor.read_exact(&mut magic)?;
        if &magic != MAGIC {
            return Err(BinaryError::InvalidMagic);
        }

        let version = self.read_u16()?;
        if version != VERSION {
            return Err(BinaryError::UnsupportedVersion(version));
        }

        let _flags = self.read_u16()?;
        let _func_count = self.read_u32()?;
        let _reserved = self.read_u32()?;

        // Read main function (which includes nested functions)
        let func = self.read_function(0)?;

        Ok(func)
    }

    fn read_program_with_sections(mut self) -> DeserializeResult {
        // Header
        let mut magic = [0u8; 4];
        self.cursor.read_exact(&mut magic)?;
        if &magic != MAGIC {
            return Err(BinaryError::InvalidMagic);
        }

        let version = self.read_u16()?;
        if version != VERSION {
            return Err(BinaryError::UnsupportedVersion(version));
        }

        let _flags = self.read_u16()?;
        let _func_count = self.read_u32()?;
        let _reserved = self.read_u32()?;

        let func = self.read_function(0)?;
        let (manifest, bundles) = self.read_sections()?;

        Ok((func, manifest, bundles))
    }

    fn read_function(&mut self, depth: usize) -> Result<Function> {
        if depth > MAX_NESTING_DEPTH {
            return Err(BinaryError::LimitExceeded {
                what: "function nesting depth",
                limit: MAX_NESTING_DEPTH,
            });
        }
        // Name
        let name_len = self.read_u16()? as usize;
        if name_len > MAX_STRING_LEN {
            return Err(BinaryError::LimitExceeded {
                what: "function name length",
                limit: MAX_STRING_LEN,
            });
        }
        let name = if name_len > 0 {
            let mut bytes = vec![0u8; name_len];
            self.cursor.read_exact(&mut bytes)?;
            Some(String::from_utf8(bytes).map_err(|_| BinaryError::InvalidUtf8)?)
        } else {
            None
        };

        // Metadata
        let arity = self.read_u16()?;
        let num_registers = self.read_u32()?;

        // Constants
        let const_count = self.read_u32()? as usize;
        if const_count > MAX_CONSTANTS {
            return Err(BinaryError::LimitExceeded {
                what: "constants",
                limit: MAX_CONSTANTS,
            });
        }
        let mut constants = Vec::with_capacity(const_count);
        for _ in 0..const_count {
            let value = self.read_constant()?;
            constants.push(value);
        }

        // Bytecode
        let bc_len = self.read_u32()? as usize;
        if bc_len > MAX_BYTECODE_LEN {
            return Err(BinaryError::LimitExceeded {
                what: "bytecode length",
                limit: MAX_BYTECODE_LEN,
            });
        }
        let mut bytecode = Vec::with_capacity(bc_len);
        for _ in 0..bc_len {
            bytecode.push(self.read_u32()?);
        }

        // Nested functions
        let nested_count = self.read_u16()? as usize;
        if nested_count > MAX_NESTED_FUNCTIONS {
            return Err(BinaryError::LimitExceeded {
                what: "nested functions",
                limit: MAX_NESTED_FUNCTIONS,
            });
        }

        Self::validate_func_markers(&constants, nested_count)?;

        let mut nested_functions = Vec::with_capacity(nested_count);
        for _ in 0..nested_count {
            nested_functions.push(self.read_function(depth + 1)?);
        }

        // Upvalue descriptors (needed for closures)
        let upvalue_count = self.read_u16()? as usize;
        if upvalue_count > MAX_UPVALUE_DESCRIPTORS {
            return Err(BinaryError::LimitExceeded {
                what: "upvalue descriptors",
                limit: MAX_UPVALUE_DESCRIPTORS,
            });
        }
        let mut upvalue_descriptors = Vec::with_capacity(upvalue_count);
        for _ in 0..upvalue_count {
            let is_local = self.read_u8()? != 0;
            let index = self.read_u16()?;
            upvalue_descriptors.push(UpvalueDescriptor { is_local, index });
        }

        // Line info
        let lines_count = self.read_u16()? as usize;
        if lines_count > MAX_LINES {
            return Err(BinaryError::LimitExceeded {
                what: "line info entries",
                limit: MAX_LINES,
            });
        }
        let mut lines = Vec::with_capacity(lines_count);
        for _ in 0..lines_count {
            let count = self.read_u16()?;
            let line = self.read_u32()?;
            lines.push((count, line));
        }

        // Global names (for indexed global access)
        let global_names_count = self.read_u16()? as usize;
        if global_names_count > MAX_GLOBAL_NAMES {
            return Err(BinaryError::LimitExceeded {
                what: "global names",
                limit: MAX_GLOBAL_NAMES,
            });
        }
        let mut global_names = Vec::with_capacity(global_names_count);
        for _ in 0..global_names_count {
            let name_len = self.read_u16()? as usize;
            if name_len > MAX_STRING_LEN {
                return Err(BinaryError::LimitExceeded {
                    what: "global name length",
                    limit: MAX_STRING_LEN,
                });
            }
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

        // Compute global_layout_hash from global layout names
        let mut func = Function::new(name, arity);
        func.num_registers = num_registers;
        func.set_bytecode(bytecode);
        func.constants = constants;
        func.nested_functions = nested_functions;
        func.upvalue_descriptors = upvalue_descriptors;
        func.lines = lines;
        func.global_layout = GlobalLayout::new(global_names);
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
                // TAG_BOOL
                let b = self.read_u8()? != 0;
                Ok(Constant::Bool(b))
            }
            2 => {
                // TAG_INT
                let n = self.read_i64()?;
                Ok(Constant::Int(n))
            }
            3 => {
                // TAG_FLOAT
                let bits = self.read_u64()?;
                Ok(Constant::Float(bits))
            }
            4 => {
                // TAG_STRING
                let len = self.read_u32()? as usize;
                if len > MAX_STRING_LEN {
                    return Err(BinaryError::LimitExceeded {
                        what: "string length",
                        limit: MAX_STRING_LEN,
                    });
                }
                let mut bytes = vec![0u8; len];
                self.cursor.read_exact(&mut bytes)?;
                let s = String::from_utf8(bytes).map_err(|_| BinaryError::InvalidUtf8)?;
                Ok(Constant::String(s))
            }
            5 => {
                // TAG_FUNC (nested function marker with dedicated tag)
                Ok(Constant::NestedFunction(self.read_u32()?))
            }
            _ => Err(BinaryError::InvalidConstantType(tag)),
        }
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
            let data = self.read_bytes(len)?;
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
        let pos = self.cursor.position() as usize;
        let len = self.cursor.get_ref().len();
        len.saturating_sub(pos)
    }

    fn read_bytes(&mut self, len: usize) -> Result<Vec<u8>> {
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

fn parse_bundles_section(data: &[u8]) -> Result<Vec<NativeBundle>> {
    let mut cursor = Cursor::new(data);
    let count = read_u32_from(&mut cursor)? as usize;
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
    let mut buf = vec![0u8; len];
    cursor.read_exact(&mut buf)?;
    Ok(buf)
}

/// Count total number of functions (including nested)
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
