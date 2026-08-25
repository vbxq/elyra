use super::super::Compiler;
use aelys_bytecode::UpvalueDescriptor;
use aelys_common::Result;
use aelys_common::error::{CompileError, CompileErrorKind};
use aelys_sema::TypedFunction;

pub(super) fn finalize_typed_function(
    parent: &mut Compiler,
    mut nested_compiler: Compiler,
    func: &TypedFunction,
    func_var_reg: u16,
) -> Result<()> {
    nested_compiler.current.num_registers = nested_compiler.next_register;
    nested_compiler.current.global_layout = nested_compiler.build_global_layout();
    nested_compiler.current.compute_global_layout_hash();
    nested_compiler.current.finalize_bytecode();
    nested_compiler.update_jit_eligibility();

    parent.mark_captures_from_nested(&nested_compiler);

    let mut compiled_func = nested_compiler.current.clone();
    let mut nested_upvalues = nested_compiler.upvalues.clone();
    parent.fix_transitive_captures(&mut nested_upvalues);

    for upvalue in &nested_upvalues {
        compiled_func.upvalue_descriptors.push(UpvalueDescriptor {
            is_local: upvalue.is_local,
            index: upvalue.index,
        });
    }

    for (name, idx) in &nested_compiler.global_indices {
        if !parent.global_indices.contains_key(name) {
            parent.global_indices.insert(name.clone(), *idx);
        }
    }
    parent.next_global_index = nested_compiler.next_global_index;

    let const_idx = parent.current.add_constant_function(compiled_func);

    if nested_upvalues.is_empty() {
        parent.emit_load_constant(func_var_reg, const_idx, func.span);
    } else {
        let upvalue_count = u8::try_from(nested_upvalues.len()).map_err(|_| {
            CompileError::new(
                CompileErrorKind::TooManyUpvalues,
                func.span,
                parent.source.clone(),
            )
        })?;
        parent.emit_make_closure(func_var_reg, const_idx, upvalue_count, func.span);
    }

    if parent.scope_depth == 0 {
        let idx = parent.get_or_create_global_index(&func.name);
        parent.accessed_globals.insert(func.name.clone());
        parent.emit_set_global_index(func_var_reg, idx, func.span);
    }

    Ok(())
}
