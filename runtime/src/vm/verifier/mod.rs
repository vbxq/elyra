use crate::vm::{Function, Heap};

mod bytecode;
mod checks;
mod constants;

pub const MAX_FUNCTION_NESTING: usize = 64;

pub fn verify_function(func: &Function, heap: &Heap, depth: usize) -> Result<(), String> {
    if depth > MAX_FUNCTION_NESTING {
        return Err(format!(
            "nesting depth {} exceeds max {}",
            depth, MAX_FUNCTION_NESTING
        ));
    }

    let required_jit_unsupported = required_jit_unsupported(func);
    if func.jit_unsupported_struct != required_jit_unsupported {
        return Err(format!(
            "jit struct exclusion flag is {}, expected {}",
            func.jit_unsupported_struct, required_jit_unsupported
        ));
    }

    constants::verify_constants(func, heap)?;
    bytecode::verify_bytecode(func)?;

    for nested in &func.nested_functions {
        verify_function(nested, heap, depth + 1)?;
    }

    Ok(())
}

fn required_jit_unsupported(func: &Function) -> bool {
    func.bytecode.as_slice().iter().any(|word| {
        matches!(
            aelys_bytecode::OpCode::from_u8((word >> 24) as u8),
            Some(
                aelys_bytecode::OpCode::StructNew
                    | aelys_bytecode::OpCode::StructLoad
                    | aelys_bytecode::OpCode::StructStore
                    | aelys_bytecode::OpCode::EnumNew
                    | aelys_bytecode::OpCode::EnumTest
                    | aelys_bytecode::OpCode::EnumLoad
            )
        )
    })
}
