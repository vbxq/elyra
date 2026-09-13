pub mod assembler;
pub mod binary;
pub mod disasm;
mod lexer;
mod opcodes;

pub use assembler::{AssemblerError, assemble, assemble_from_string};
pub use binary::{
    BinaryError, MAX_REGISTERS, NativeBundle, ProgramSections, RequiredImport, RequiredImportKind,
    deserialize, deserialize_with_manifest, deserialize_with_sections, serialize,
    serialize_with_manifest, serialize_with_sections,
};
pub use disasm::{DisassemblerOptions, disassemble, disassemble_to_string};
