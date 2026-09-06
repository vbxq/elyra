use aelys_backend::Compiler;
use aelys_common::Result;
use aelys_common::error::{AelysError, CompileError, CompileErrorKind};
use aelys_frontend::lexer::Lexer;
use aelys_frontend::parser::Parser;
use aelys_opt::{OptimizationLevel, Optimizer};
use aelys_runtime::{VM, Value, VmConfig};
use aelys_sema::TypeInference;
use aelys_syntax::{Source, Span};

pub fn run(source: &str, name: &str) -> Result<Value> {
    run_with_config(source, name, VmConfig::default(), Vec::new())
}

pub fn run_with_config(
    source: &str,
    name: &str,
    config: VmConfig,
    program_args: Vec<String>,
) -> Result<Value> {
    run_with_config_and_opt(
        source,
        name,
        config,
        program_args,
        OptimizationLevel::Standard,
    )
}

pub fn run_with_config_and_opt(
    source: &str,
    name: &str,
    config: VmConfig,
    program_args: Vec<String>,
    opt_level: OptimizationLevel,
) -> Result<Value> {
    let src = Source::new(name, source);
    let tokens = Lexer::with_source(src.clone()).scan()?;
    let stmts = Parser::new_rust_collections(tokens, src.clone()).parse()?;
    reject_unloadable_needs(&stmts, &src)?;

    let mut vm =
        VM::with_config_and_args(src.clone(), config, program_args).map_err(AelysError::Runtime)?;
    let resolved = crate::modules::resolve_globals(None, &vm);

    let typed_program = TypeInference::infer_program_with_imports_and_natives(
        stmts,
        src.clone(),
        resolved.module_aliases.clone(),
        resolved.known_globals.clone(),
        resolved.known_native_globals.clone(),
    )
    .map_err(|errors| {
        if let Some(err) = errors.first() {
            AelysError::Compile(CompileError::new(
                CompileErrorKind::NamedTypeError {
                    code: err.diagnostic_code(),
                    message: format!("{}", err),
                },
                err.span,
                src.clone(),
            ))
        } else {
            AelysError::Compile(CompileError::new(
                CompileErrorKind::TypeInferenceError("Unknown type error".to_string()),
                Span::dummy(),
                src.clone(),
            ))
        }
    })?;

    let mut optimizer = Optimizer::new(opt_level);
    let typed_program = optimizer.optimize(typed_program);

    let (function, _globals) = Compiler::with_modules(
        None,
        src.clone(),
        resolved.module_aliases,
        resolved.known_globals,
        resolved.known_native_globals,
        resolved.symbol_origins,
    )
    .compile_typed(&typed_program)?;

    let func_ref = vm.alloc_function(function).map_err(AelysError::Runtime)?;
    Ok(vm.execute(func_ref)?)
}

fn reject_unloadable_needs(
    statements: &[aelys_syntax::Stmt],
    source: &std::sync::Arc<Source>,
) -> Result<()> {
    for statement in statements {
        let aelys_syntax::StmtKind::Needs(needs) = &statement.kind else {
            continue;
        };
        return Err(AelysError::Compile(CompileError::new(
            CompileErrorKind::ModuleNotFound {
                module_path: needs.path.join("."),
                searched_paths: vec![
                    "no module search root: use run_file or run_with_vm to load modules"
                        .to_string(),
                ],
            },
            needs.span,
            source.clone(),
        )));
    }
    Ok(())
}

pub fn run_source(source: &str, name: &str, opt_level: Option<OptimizationLevel>) -> Result<Value> {
    run_with_config_and_opt(
        source,
        name,
        VmConfig::default(),
        Vec::new(),
        opt_level.unwrap_or(OptimizationLevel::Standard),
    )
}
