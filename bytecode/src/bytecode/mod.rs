// bytecode format and instruction encoding

mod buffer;
mod constant;
mod decode;
mod function;
mod global_layout;
mod opcode;
mod operand;
mod upvalue;

pub use buffer::BytecodeBuffer;
pub use constant::Constant;
pub use decode::{decode_a, decode_b, decode_c};
pub use function::Function;
pub use global_layout::GlobalLayout;
pub use opcode::{InstructionFormat, OpCode, WideRegisterOperands};
pub use operand::{Arity, ConstantIndex, GlobalIndex, JumpOffset, OperandRangeError, Register};
pub use upvalue::UpvalueDescriptor;
