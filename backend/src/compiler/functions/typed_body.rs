use super::super::Compiler;
use super::super::liveness::LivenessAnalysis;
use aelys_bytecode::OpCode;
use aelys_common::Result;
use aelys_sema::TypedFunction;
use std::collections::HashSet;

pub(super) fn compile_typed_body(
    nested_compiler: &mut Compiler,
    func: &TypedFunction,
) -> Result<()> {
    nested_compiler.begin_scope();

    for param in &func.params {
        let reg = nested_compiler.alloc_register()?;
        let resolved_type = aelys_sema::ResolvedType::from_infer_type(&param.ty);
        nested_compiler.add_local(param.name.clone(), param.mutable, reg, resolved_type);
    }

    let liveness = LivenessAnalysis::analyze_function(func);

    if func.body.is_empty() {
        nested_compiler.emit_a(OpCode::Return0, 0, 0, 0, func.span);
        nested_compiler.end_function_scope();
        return Ok(());
    }

    let last_idx = func.body.len() - 1;
    let mut already_freed = HashSet::new();
    for (stmt_idx, stmt) in func.body[..last_idx].iter().enumerate() {
        nested_compiler.compile_typed_stmt(stmt)?;
        nested_compiler.free_dead_locals(stmt_idx, &liveness, &mut already_freed);
    }

    let last_stmt = &func.body[last_idx];

    let unit_return = matches!(func.return_type, aelys_sema::InferType::Unit);
    let implicit_return_reg = if !unit_return && Compiler::typed_stmt_yields_value(last_stmt) {
        let result_reg = nested_compiler.alloc_register()?;
        nested_compiler.compile_typed_stmt_value(last_stmt, result_reg)?;
        Some(result_reg)
    } else {
        nested_compiler.compile_typed_stmt(last_stmt)?;
        nested_compiler.free_dead_locals(last_idx, &liveness, &mut already_freed);
        None
    };

    let ends_in_return = matches!(last_stmt.kind, aelys_sema::TypedStmtKind::Return(_));
    if let Some(result_reg) = implicit_return_reg {
        nested_compiler.close_upvalues_before_return(func.span);
        nested_compiler.emit_a(OpCode::Return, result_reg, 0, 0, func.span);
    } else if !ends_in_return {
        nested_compiler.close_upvalues_before_return(func.span);
        nested_compiler.emit_a(OpCode::Return0, 0, 0, 0, func.span);
    }

    nested_compiler.end_function_scope();
    Ok(())
}
