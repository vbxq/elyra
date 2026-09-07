use crate::modules::load_modules_for_program;
use aelys_backend::Compiler;
use aelys_common::error::{AelysError, CompileError, CompileErrorKind};
use aelys_common::{Result, Warning};
use aelys_frontend::lexer::Lexer;
use aelys_frontend::parser::Parser;
use aelys_opt::{OptimizationLevel, Optimizer};
use aelys_runtime::{ExecutionControl, VM, Value, VmConfig};
use aelys_sema::TypeInference;
use aelys_syntax::{Source, Span};

pub struct RunResult {
    pub value: Value,
    pub warnings: Vec<Warning>,
}

pub fn run_file(file_path: &std::path::Path) -> Result<Value> {
    run_file_with_config(file_path, VmConfig::default(), Vec::new())
}

pub fn run_file_with_config(
    file_path: &std::path::Path,
    config: VmConfig,
    program_args: Vec<String>,
) -> Result<Value> {
    run_file_with_config_and_opt(file_path, config, program_args, OptimizationLevel::Standard)
}

pub fn run_file_with_config_and_opt(
    file_path: &std::path::Path,
    config: VmConfig,
    program_args: Vec<String>,
    opt_level: OptimizationLevel,
) -> Result<Value> {
    let result = run_file_full(file_path, config, program_args, opt_level)?;
    Ok(result.value)
}

pub fn run_file_full(
    file_path: &std::path::Path,
    config: VmConfig,
    program_args: Vec<String>,
    opt_level: OptimizationLevel,
) -> Result<RunResult> {
    run_file_full_with_control(
        file_path,
        config,
        program_args,
        opt_level,
        ExecutionControl::default(),
    )
}

pub fn run_file_full_with_control(
    file_path: &std::path::Path,
    config: VmConfig,
    program_args: Vec<String>,
    opt_level: OptimizationLevel,
    execution_control: ExecutionControl,
) -> Result<RunResult> {
    let content = std::fs::read_to_string(file_path).map_err(|_| {
        AelysError::Compile(CompileError::new(
            CompileErrorKind::ModuleNotFound {
                module_path: file_path.display().to_string(),
                searched_paths: vec![file_path.display().to_string()],
            },
            Span::dummy(),
            Source::new(file_path.display().to_string(), ""),
        ))
    })?;

    let name = file_path.display().to_string();
    let src = Source::new(&name, &content);

    let tokens = Lexer::with_source(src.clone()).scan()?;
    let stmts = Parser::new_rust_collections(tokens, src.clone()).parse()?;

    let mut vm =
        VM::with_config_and_args(src.clone(), config, program_args).map_err(AelysError::Runtime)?;
    vm.configure_execution(execution_control);

    if let Ok(abs_path) = file_path.canonicalize() {
        vm.set_script_path(abs_path.display().to_string());
    } else {
        vm.set_script_path(file_path.display().to_string());
    }

    let imports = load_modules_for_program(&stmts, file_path, src.clone(), &mut vm)?;

    let main_stmts: Vec<_> = imports
        .imported_impl_stmts
        .iter()
        .cloned()
        .chain(
            stmts
                .into_iter()
                .filter(|s| !matches!(s.kind, aelys_syntax::StmtKind::Needs(_))),
        )
        .collect();

    let resolved = crate::modules::resolve_globals(Some(&imports), &vm);
    let all_native_signatures = imports.native_signatures.clone();

    let inference_result = TypeInference::infer_program_full_with_native_signatures(
        main_stmts,
        src.clone(),
        resolved.module_aliases.clone(),
        resolved.known_globals.clone(),
        resolved.known_native_globals.clone(),
        all_native_signatures,
        imports.imported_types.clone(),
    )
    .map_err(|errors| {
        if let Some(err) = errors.first() {
            AelysError::Compile(CompileError::new(
                CompileErrorKind::NamedTypeError {
                    code: err.diagnostic_code(),
                    message: format!("{}", err),
                },
                err.span,
                crate::modules::diagnostic_source(Some(&imports), err.defining_module(), &src),
            ))
        } else {
            AelysError::Compile(CompileError::new(
                CompileErrorKind::TypeInferenceError("Unknown type error".to_string()),
                Span::dummy(),
                src.clone(),
            ))
        }
    })?;

    let mut warnings: Vec<Warning> = inference_result
        .warnings
        .into_iter()
        .map(|mut w| {
            if w.source.is_none() {
                w.source = Some(src.clone());
            }
            w
        })
        .collect();

    let mut optimizer = Optimizer::new(opt_level);
    let typed_program = optimizer.optimize(inference_result.program);

    let opt_warnings: Vec<Warning> = optimizer
        .take_warnings()
        .into_iter()
        .map(|mut w| {
            if w.source.is_none() {
                w.source = Some(src.clone());
            }
            w
        })
        .collect();
    warnings.extend(opt_warnings);

    let compiler = Compiler::with_modules(
        None,
        src.clone(),
        resolved.module_aliases,
        resolved.codegen_globals,
        resolved.known_native_globals,
        resolved.symbol_origins,
    )
    .with_module_sources(imports.module_sources.clone());
    let (function, _globals) = compiler.compile_typed(&typed_program)?;

    let func_ref = vm.alloc_function(function).map_err(AelysError::Runtime)?;
    let value = vm.execute(func_ref)?;

    Ok(RunResult { value, warnings })
}
