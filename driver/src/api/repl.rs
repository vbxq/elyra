use crate::modules::load_modules_for_program;
use aelys_backend::Compiler;
use aelys_common::Result;
use aelys_common::error::{AelysError, CompileError, CompileErrorKind};
use aelys_frontend::lexer::Lexer;
use aelys_frontend::parser::Parser;
use aelys_opt::{OptimizationLevel, Optimizer};
use aelys_runtime::{VM, Value};
use aelys_sema::TypeInference;
use aelys_sema::typed_ast::TypedStmtKind;
use aelys_sema::types::{InferType, TypeTable};
use aelys_syntax::{Source, Span, Stmt, StmtKind};
use std::collections::HashMap;
use std::sync::Arc;

pub fn run_with_vm(vm: &mut VM, source: &str, name: &str) -> Result<Value> {
    run_with_vm_and_opt(vm, source, name, OptimizationLevel::Basic)
}

pub fn run_with_vm_and_opt(
    vm: &mut VM,
    source: &str,
    name: &str,
    opt_level: OptimizationLevel,
) -> Result<Value> {
    let src = Source::new(name, source);
    let tokens = Lexer::with_source(src.clone()).scan()?;
    let stmts = Parser::new_rust_collections(tokens, src.clone()).parse()?;
    run_parsed(vm, src, stmts, opt_level, HashMap::new()).map(|(value, _)| value)
}

#[derive(Default)]
pub struct ReplSession {
    declarations: Vec<(String, String)>,
    global_types: HashMap<String, InferType>,
}

impl ReplSession {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn run_line(
        &mut self,
        vm: &mut VM,
        line: &str,
        name: &str,
        opt_level: OptimizationLevel,
    ) -> Result<Value> {
        let line_src = Source::new(name, line);
        let tokens = Lexer::with_source(line_src.clone()).scan()?;
        let line_stmts = Parser::new_rust_collections(tokens, line_src).parse()?;
        let new_declarations: Vec<(String, String)> = line_stmts
            .iter()
            .filter_map(|stmt| {
                let key = declaration_key(stmt, line)?;
                let text = line.get(stmt.span.start..stmt.span.end)?;
                Some((key, text.to_string()))
            })
            .collect();
        let mut program = String::new();
        for (key, text) in &self.declarations {
            if new_declarations.iter().all(|(new_key, _)| new_key != key) {
                program.push_str(text);
                program.push('\n');
            }
        }
        let prefix_lines = u32::try_from(program.lines().count()).unwrap_or(u32::MAX);
        program.push_str(line);
        let src = Source::new(name, &program);
        let tokens = Lexer::with_source(src.clone()).scan()?;
        let mut stmts = Parser::new_rust_collections(tokens, src.clone()).parse()?;
        for stmt in &mut stmts {
            if stmt.span.line > prefix_lines
                && let StmtKind::Let {
                    is_pub, is_const, ..
                } = &mut stmt.kind
                && !*is_const
            {
                *is_pub = true;
            }
        }
        let prefix_bytes = program.len() - line.len();
        let (value, globals) = run_parsed(vm, src, stmts, opt_level, self.global_types.clone())
            .map_err(|error| relocate_to_line(error, prefix_lines, prefix_bytes, name, line))?;
        self.global_types.extend(globals);
        for (key, text) in new_declarations {
            match self.declarations.iter_mut().find(|(old, _)| *old == key) {
                Some(slot) => slot.1 = text,
                None => self.declarations.push((key, text)),
            }
        }
        Ok(value)
    }
}

fn relocate_to_line(
    error: AelysError,
    prefix_lines: u32,
    prefix_bytes: usize,
    name: &str,
    line: &str,
) -> AelysError {
    match error {
        AelysError::Compile(error) if error.span.line > prefix_lines => {
            let mut span = error.span;
            span.line -= prefix_lines;
            span.start = span.start.saturating_sub(prefix_bytes);
            span.end = span.end.saturating_sub(prefix_bytes);
            AelysError::Compile(CompileError::new(error.kind, span, Source::new(name, line)))
        }
        AelysError::Runtime(mut error) => {
            let line_source = Source::new(name, line);
            for frame in &mut error.stack_trace {
                let in_session = frame
                    .source
                    .as_ref()
                    .is_none_or(|source| source.name == error.source.name);
                if in_session && frame.line > prefix_lines {
                    frame.line -= prefix_lines;
                    frame.source = Some(line_source.clone());
                }
            }
            AelysError::Runtime(error)
        }
        other => other,
    }
}

// a monomorphized instance name exists only in the program that created it, so the next line names it by origin
fn source_level_type(ty: &InferType, table: &TypeTable) -> InferType {
    let map = |inner: &InferType| Box::new(source_level_type(inner, table));
    let all = |items: &[InferType]| {
        items
            .iter()
            .map(|item| source_level_type(item, table))
            .collect::<Vec<_>>()
    };
    match ty {
        InferType::Struct(name) => match (
            table.nominal_instance_origin(name),
            table.nominal_instance_args(name),
        ) {
            (Some(origin), Some(args)) => InferType::Applied {
                name: origin.to_string(),
                args: all(args),
            },
            _ => ty.clone(),
        },
        InferType::Applied { name, args } => InferType::Applied {
            name: name.clone(),
            args: all(args),
        },
        InferType::Option(inner) => InferType::Option(map(inner)),
        InferType::Result(ok, err) => InferType::Result(map(ok), map(err)),
        InferType::Array(inner) => InferType::Array(map(inner)),
        InferType::FixedArray(inner, length) => InferType::FixedArray(map(inner), *length),
        InferType::Vec(inner) => InferType::Vec(map(inner)),
        InferType::Map(key, value) => InferType::Map(map(key), map(value)),
        InferType::Tuple(items) => InferType::Tuple(all(items)),
        InferType::Function { params, modes, ret } => InferType::Function {
            params: all(params),
            modes: modes.clone(),
            ret: map(ret),
        },
        other => other.clone(),
    }
}

fn declaration_key(stmt: &Stmt, line: &str) -> Option<String> {
    match &stmt.kind {
        StmtKind::Function(function) => Some(format!("fn {}", function.name)),
        StmtKind::StructDecl { name, .. } | StmtKind::EnumDecl { name, .. } => {
            Some(format!("type {name}"))
        }
        StmtKind::TraitDecl { name, .. } => Some(format!("trait {name}")),
        StmtKind::Let {
            name,
            is_const: true,
            ..
        } => Some(format!("const {name}")),
        StmtKind::ImplDecl { .. } => {
            let text = line.get(stmt.span.start..stmt.span.end)?;
            let header = text.split('{').next().unwrap_or(text);
            Some(format!(
                "impl {}",
                header.split_whitespace().collect::<Vec<_>>().join(" ")
            ))
        }
        _ => None,
    }
}

fn run_parsed(
    vm: &mut VM,
    src: Arc<Source>,
    stmts: Vec<Stmt>,
    opt_level: OptimizationLevel,
    session_types: HashMap<String, InferType>,
) -> Result<(Value, HashMap<String, InferType>)> {
    vm.clear_frames();

    let has_needs = stmts
        .iter()
        .any(|s| matches!(s.kind, aelys_syntax::StmtKind::Needs(_)));

    let resolved = crate::modules::resolve_globals(None, vm);
    let mut module_aliases = resolved.module_aliases;
    let mut known_globals = resolved.known_globals;
    let mut known_native_globals = resolved.known_native_globals;
    let mut native_signatures = session_types;
    for global in &known_globals {
        if native_signatures.contains_key(global) {
            continue;
        }
        let Some(value) = vm.get_global(global) else {
            continue;
        };
        let ty = if vm.is_int_value(value) {
            Some(aelys_sema::types::InferType::I64)
        } else if value.as_float().is_some() {
            Some(aelys_sema::types::InferType::F64)
        } else if value.as_bool().is_some() {
            Some(aelys_sema::types::InferType::Bool)
        } else {
            None
        };
        if let Some(ty) = ty {
            native_signatures.insert(global.clone(), ty);
        }
    }
    let mut symbol_origins = resolved.symbol_origins;

    let main_stmts = if has_needs {
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let repl_path = cwd.join("repl.aelys");

        let imports = load_modules_for_program(&stmts, &repl_path, src.clone(), vm)?;

        module_aliases.extend(imports.module_aliases.iter().cloned());
        vm.add_repl_module_aliases(&imports.module_aliases);

        known_globals.extend(imports.known_globals.iter().cloned());
        vm.add_repl_known_globals(&imports.known_globals);

        known_native_globals.extend(imports.known_native_globals.iter().cloned());
        vm.add_repl_known_native_globals(&imports.known_native_globals);
        native_signatures.extend(imports.native_signatures.clone());

        symbol_origins.extend(
            imports
                .symbol_origins
                .iter()
                .map(|(k, v)| (k.clone(), v.clone())),
        );
        vm.add_repl_symbol_origins(&imports.symbol_origins);

        stmts
            .into_iter()
            .filter(|s| !matches!(s.kind, aelys_syntax::StmtKind::Needs(_)))
            .collect()
    } else {
        stmts
    };

    let typed_program = TypeInference::infer_program_with_imports_and_native_signatures(
        main_stmts,
        src.clone(),
        module_aliases.clone(),
        known_globals.clone(),
        known_native_globals.clone(),
        native_signatures,
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

    let declared_globals: HashMap<String, InferType> = typed_program
        .stmts
        .iter()
        .filter_map(|stmt| match &stmt.kind {
            TypedStmtKind::Let { name, var_type, .. } => Some((
                name.clone(),
                source_level_type(var_type, &typed_program.type_table),
            )),
            _ => None,
        })
        .collect();

    let mut optimizer = Optimizer::new(opt_level);
    let typed_program = optimizer.optimize(typed_program);

    let existing_globals = vm.global_mutability().clone();
    let compiler = Compiler::with_modules_and_globals(
        None,
        src.clone(),
        module_aliases,
        known_globals,
        known_native_globals,
        symbol_origins,
        existing_globals,
    );
    let (function, new_globals) = compiler.compile_typed(&typed_program)?;

    vm.update_global_mutability(new_globals);

    let global_names: Vec<String> = function.global_layout.names().to_vec();

    let func_ref = vm.alloc_function(function)?;
    let result = vm.execute(func_ref)?;

    vm.sync_globals_to_hashmap(&global_names);

    let new_globals_set: std::collections::HashSet<String> = global_names
        .into_iter()
        .filter(|name| !name.is_empty())
        .collect();
    vm.add_repl_known_globals(&new_globals_set);

    Ok((result, declared_globals))
}
