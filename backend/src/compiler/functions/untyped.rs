use super::super::Compiler;
use super::untyped_body::compile_untyped_body;
use super::untyped_finalize::finalize_untyped_function;
use aelys_common::Result;

impl Compiler {
    pub fn compile_function(&mut self, func: &aelys_syntax::ast::Function) -> Result<()> {
        let func_var_reg = self.declare_variable(&func.name, false)?;

        if self.scope_depth == 0 {
            self.globals.insert(func.name.clone(), false);
            if !self.global_indices.contains_key(&func.name) {
                let idx = self.next_global_index;
                self.global_indices.insert(func.name.clone(), idx);
                self.next_global_index += 1;
            }
        }

        let globals = self.globals.clone();
        let global_indices = self.global_indices.clone();
        let enclosing_locals = self.locals.clone();
        let enclosing_upvalues = self.upvalues.clone();

        let mut func_compiler = Compiler::for_nested_function(
            Some(func.name.clone()),
            self.source.clone(),
            globals,
            global_indices,
            self.next_global_index,
            enclosing_locals,
            enclosing_upvalues,
            self.all_enclosing_locals.clone(),
            self.module_aliases.clone(),
            self.known_globals.clone(),
            self.known_native_globals.clone(),
            self.symbol_origins.clone(),
        );

        func_compiler.begin_scope();

        for param in &func.params {
            func_compiler.declare_variable(&param.name, false)?;
        }

        let body_result = compile_untyped_body(&mut func_compiler, func)?;

        func_compiler.end_scope();

        if !body_result.returned {
            func_compiler.emit_return0(func.span);
        }

        func_compiler.current.num_registers = func_compiler.next_register;
        func_compiler.current.arity = u16::try_from(func.params.len()).map_err(|_| {
            aelys_common::error::CompileError::new(
                aelys_common::error::CompileErrorKind::TooManyArguments,
                func.span,
                self.source.clone(),
            )
        })?;

        finalize_untyped_function(self, func_compiler, &func.name, func.span, func_var_reg)
    }
}
