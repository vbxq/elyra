use super::super::Compiler;
use aelys_bytecode::{GlobalLayout, UpvalueDescriptor};
use aelys_common::Result;
use aelys_common::error::{CompileError, CompileErrorKind};
use aelys_syntax::Span;
use std::sync::Arc;

pub(super) fn finalize_untyped_function(
    parent: &mut Compiler,
    mut func_compiler: Compiler,
    func_name: &str,
    func_span: Span,
    func_var_reg: u16,
) -> Result<()> {
    func_compiler.current.global_layout = build_untyped_global_layout(&func_compiler);
    func_compiler.current.compute_global_layout_hash();
    func_compiler.current.finalize_bytecode();
    func_compiler.update_jit_eligibility();

    parent.mark_captures_from_nested(&func_compiler);
    parent.fix_transitive_captures(&mut func_compiler.upvalues);

    for upvalue in &func_compiler.upvalues {
        func_compiler
            .current
            .upvalue_descriptors
            .push(UpvalueDescriptor {
                is_local: upvalue.is_local,
                index: upvalue.index,
            });
    }

    let compiled_func = func_compiler.current;
    let upvalue_count = func_compiler.upvalues.len();
    if upvalue_count > 255 {
        return Err(CompileError::new(
            CompileErrorKind::TooManyUpvalues,
            func_span,
            parent.source.clone(),
        )
        .into());
    }

    for (name, &idx) in &func_compiler.global_indices {
        if !parent.global_indices.contains_key(name) {
            parent.global_indices.insert(name.clone(), idx);
        }
    }
    if func_compiler.next_global_index > parent.next_global_index {
        parent.next_global_index = func_compiler.next_global_index;
    }
    let const_idx = parent.current.add_constant_function(compiled_func);

    if upvalue_count > 0 {
        let upvalue_count = u8::try_from(upvalue_count).map_err(|_| {
            CompileError::new(
                CompileErrorKind::TooManyUpvalues,
                func_span,
                parent.source.clone(),
            )
        })?;
        parent.emit_make_closure(func_var_reg, const_idx, upvalue_count, func_span);
    } else {
        parent.emit_load_constant(func_var_reg, const_idx, func_span);
    }

    let idx = parent.get_or_create_global_index(func_name);
    parent.accessed_globals.insert(func_name.to_string());
    parent.emit_set_global_index(func_var_reg, idx, func_span);

    Ok(())
}

fn build_untyped_global_layout(compiler: &Compiler) -> Arc<GlobalLayout> {
    if compiler.accessed_globals.is_empty() {
        GlobalLayout::empty()
    } else {
        let mut names = vec![String::new(); compiler.next_global_index as usize];
        for (name, &idx) in &compiler.global_indices {
            names[idx as usize] = name.clone();
        }
        GlobalLayout::new(names)
    }
}
