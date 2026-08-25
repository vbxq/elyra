use super::super::Compiler;
use aelys_bytecode::UpvalueDescriptor;
use aelys_common::Result;
use aelys_common::error::{CompileError, CompileErrorKind};
use aelys_syntax::Span;

pub(super) fn finalize_lambda(
    parent: &mut Compiler,
    mut lambda_compiler: Compiler,
    dest: u16,
    span: Span,
) -> Result<()> {
    let global_layout = if lambda_compiler.accessed_globals.is_empty() {
        aelys_bytecode::GlobalLayout::empty()
    } else {
        let mut names = vec![String::new(); lambda_compiler.next_global_index as usize];
        for (name, &idx) in &lambda_compiler.global_indices {
            names[idx as usize] = name.clone();
        }
        aelys_bytecode::GlobalLayout::new(names)
    };
    lambda_compiler.current.global_layout = global_layout;
    lambda_compiler.current.compute_global_layout_hash();
    lambda_compiler.current.finalize_bytecode();
    lambda_compiler.update_jit_eligibility();

    parent.mark_captures_from_nested(&lambda_compiler);
    parent.fix_transitive_captures(&mut lambda_compiler.upvalues);

    for upvalue in &lambda_compiler.upvalues {
        lambda_compiler
            .current
            .upvalue_descriptors
            .push(UpvalueDescriptor {
                is_local: upvalue.is_local,
                index: upvalue.index,
            });
    }

    let compiled_func = lambda_compiler.current;
    let upvalue_count = lambda_compiler.upvalues.len();
    if upvalue_count > 255 {
        return Err(CompileError::new(
            CompileErrorKind::TooManyUpvalues,
            span,
            parent.source.clone(),
        )
        .into());
    }

    for (name, &idx) in &lambda_compiler.global_indices {
        if !parent.global_indices.contains_key(name) {
            parent.global_indices.insert(name.clone(), idx);
        }
    }
    if lambda_compiler.next_global_index > parent.next_global_index {
        parent.next_global_index = lambda_compiler.next_global_index;
    }

    let const_idx = parent.current.add_constant_function(compiled_func);
    if upvalue_count > 0 {
        let upvalue_count = u8::try_from(upvalue_count).map_err(|_| {
            CompileError::new(
                CompileErrorKind::TooManyUpvalues,
                span,
                parent.source.clone(),
            )
        })?;
        parent.emit_make_closure(dest, const_idx, upvalue_count, span);
    } else {
        parent.emit_load_constant(dest, const_idx, span);
    }

    Ok(())
}
