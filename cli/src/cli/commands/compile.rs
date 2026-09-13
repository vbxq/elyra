use super::run::{
    RequiredModuleError, load_required_modules, required_modules, unresolved_required_module,
};
use aelys_backend::Compiler;
use aelys_bytecode::asm::{NativeBundle, RequiredImport, RequiredImportKind};
use aelys_common::error::{
    AelysError, CompileError, CompileErrorKind, RejectedBytecodeArtifact, RejectedBytecodeOrigin,
    RejectedBytecodeStage, RuntimeErrorKind,
};
use aelys_common::{Warning, WarningConfig};
use aelys_driver::modules::{LoadedNativeInfo, load_modules_with_loader, resolve_globals};
use aelys_frontend::lexer::Lexer;
use aelys_frontend::parser::Parser;
use aelys_modules::manifest::Manifest;
use aelys_opt::{OptimizationLevel, Optimizer};
use aelys_runtime::{VM, VmConfig};
use aelys_syntax::{ImportKind, NeedsStmt, Source, Span, StmtKind};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[allow(dead_code)]
pub fn compile_to_avbc(path: &Path, opt_level: OptimizationLevel) -> Result<PathBuf, String> {
    compile_to_avbc_with_output(path, None, opt_level, None).map(|r| r.output_path)
}

pub struct CompileResult {
    pub output_path: PathBuf,
    pub warnings: Vec<Warning>,
    pub written: bool,
}

pub fn compile_to_avbc_with_output(
    path: &Path,
    output: Option<PathBuf>,
    opt_level: OptimizationLevel,
    source_for_warnings: Option<Arc<Source>>,
) -> Result<CompileResult, String> {
    compile_to_avbc_gated(path, output, opt_level, source_for_warnings, None)
}

// a run that ends in rc=1 must leave no artefact behind, so -werror has to be
fn compile_to_avbc_gated(
    path: &Path,
    output: Option<PathBuf>,
    opt_level: OptimizationLevel,
    source_for_warnings: Option<Arc<Source>>,
    warning_gate: Option<&WarningConfig>,
) -> Result<CompileResult, String> {
    let output_path = output.unwrap_or_else(|| output_path_for(path));

    match detect_format(path) {
        CompileInput::Assembly => {
            assemble_to_avbc(path, &output_path)?;
            return Ok(CompileResult {
                output_path,
                warnings: Vec::new(),
                written: true,
            });
        }
        CompileInput::Bytecode => {
            return Err("input is already bytecode".to_string());
        }
        CompileInput::Source => {}
    }

    let content = std::fs::read_to_string(path)
        .map_err(|err| format!("failed to read {}: {}", path.display(), err))?;

    let name = path.display().to_string();
    let src = Source::new(&name, &content);

    let tokens = Lexer::with_source(src.clone())
        .scan()
        .map_err(|err| err.to_string())?;
    let stmts = Parser::new_rust_collections(tokens, src.clone())
        .parse()
        .map_err(|err| err.to_string())?;

    let mut vm = VM::with_config_and_args(src.clone(), VmConfig::default(), Vec::new())
        .map_err(|err| err.to_string())?;
    if let Ok(abs_path) = path.canonicalize() {
        vm.set_script_path(abs_path.display().to_string());
    } else {
        vm.set_script_path(path.display().to_string());
    }

    let declared_imports: Vec<RequiredImport> = stmts
        .iter()
        .filter_map(|stmt| match &stmt.kind {
            StmtKind::Needs(needs) => Some(required_import_of(needs)),
            _ => None,
        })
        .collect();
    let widest_declaration = widest_declared_name(&stmts);

    let (imports, loader) = load_modules_with_loader(&stmts, path, src.clone(), &mut vm)
        .map_err(|err| module_load_error(err, &output_path))?;

    let main_stmts: Vec<_> = imports
        .imported_impl_stmts
        .iter()
        .cloned()
        .chain(
            stmts
                .into_iter()
                .filter(|stmt| !matches!(stmt.kind, StmtKind::Needs(_))),
        )
        .collect();

    let resolved = resolve_globals(Some(&imports), &vm);
    let all_known_globals = resolved.known_globals;
    let codegen_globals = resolved.codegen_globals;
    let all_module_aliases = resolved.module_aliases;
    let all_known_native_globals = resolved.known_native_globals;

    let typed_program = aelys_sema::TypeInference::infer_program_full_with_native_signatures(
        main_stmts,
        src.clone(),
        all_module_aliases.clone(),
        all_known_globals.clone(),
        all_known_native_globals.clone(),
        imports.native_signatures.clone(),
        imports.imported_types.clone(),
    )
    .map(|result| result.program)
    .map_err(|errors| {
        if let Some(err) = errors.first() {
            CompileError::new(
                CompileErrorKind::NamedTypeError {
                    code: err.diagnostic_code(),
                    message: format!("{}", err),
                },
                err.span,
                aelys_driver::modules::diagnostic_source(
                    Some(&imports),
                    err.defining_module(),
                    &src,
                ),
            )
            .to_string()
        } else {
            CompileError::new(
                CompileErrorKind::TypeInferenceError("Unknown type error".to_string()),
                aelys_syntax::Span::dummy(),
                src.clone(),
            )
            .to_string()
        }
    })?;

    let mut optimizer = Optimizer::new(opt_level);
    let typed_program = optimizer.optimize(typed_program);

    let warnings: Vec<Warning> = optimizer
        .take_warnings()
        .into_iter()
        .map(|mut w| {
            if w.source.is_none() {
                w.source = source_for_warnings.clone().or_else(|| Some(src.clone()));
            }
            w
        })
        .collect();

    let (mut function, _globals) = Compiler::with_modules(
        None,
        src.clone(),
        all_module_aliases,
        codegen_globals,
        all_known_native_globals,
        resolved.symbol_origins,
    )
    .with_module_sources(imports.module_sources.clone())
    .compile_typed(&typed_program)
    .map_err(|err| err.to_string())?;

    if opt_level != OptimizationLevel::None {
        function.strip_debug_info();
    }

    let manifest_bytes = loader.manifest().map(Manifest::to_bytes);
    let should_bundle = loader
        .manifest()
        .map(|m| m.should_bundle_natives())
        .unwrap_or(false);

    let bundles = if should_bundle && !loader.loaded_native_modules().is_empty() {
        Some(build_native_bundles(loader.loaded_native_modules())?)
    } else {
        None
    };
    let bytes = aelys_bytecode::asm::serialize_with_sections(
        &function,
        manifest_bytes.as_deref(),
        bundles.as_deref(),
        &declared_imports,
    )
    .map_err(|err| {
        serialize_error(
            &err.to_string(),
            widest_declaration.as_ref(),
            &output_path,
            &src,
        )
    })?;

    reject_unverifiable_bytecode(&bytes, &vm, &output_path, RejectedBytecodeOrigin::Compiler)?;
    reject_unloadable_bytecode(&bytes, &declared_imports, path, src.clone(), &output_path)?;

    let blocked_by_warnings = warning_gate.is_some_and(|config| {
        config.treat_as_error && warnings.iter().any(|w| config.is_enabled(&w.kind))
    });
    if !blocked_by_warnings {
        std::fs::write(&output_path, bytes)
            .map_err(|err| format!("failed to write {}: {}", output_path.display(), err))?;
    }

    Ok(CompileResult {
        output_path,
        warnings,
        written: !blocked_by_warnings,
    })
}

// the encoder gives up on a name, not on a byte offset, so the refusal points at
fn serialize_error(
    reason: &str,
    widest: Option<&(String, Span)>,
    output_path: &Path,
    src: &Arc<Source>,
) -> String {
    let kind = CompileErrorKind::BytecodeEncodingRefused {
        output: output_path.display().to_string(),
        reason: reason.to_string(),
        longest_name: widest.map(|(name, _)| truncate_name(name)),
        artifact: artifact_state(output_path),
    };
    let span = widest.map(|(_, span)| *span).unwrap_or_else(Span::dummy);
    CompileError::new(kind, span, src.clone()).to_string()
}

const NAME_EXCERPT: usize = 48;

fn truncate_name(name: &str) -> String {
    if name.chars().count() <= NAME_EXCERPT {
        return name.to_string();
    }
    let head: String = name.chars().take(NAME_EXCERPT).collect();
    format!("{head}... ({} chars)", name.chars().count())
}

fn widest_declared_name(stmts: &[aelys_syntax::Stmt]) -> Option<(String, Span)> {
    let mut widest: Option<(String, Span)> = None;
    let mut consider = |name: &String, span: Span| {
        if widest
            .as_ref()
            .is_none_or(|(current, _)| current.len() < name.len())
        {
            widest = Some((name.clone(), span));
        }
    };
    for stmt in stmts {
        match &stmt.kind {
            StmtKind::StructDecl { name, .. }
            | StmtKind::EnumDecl { name, .. }
            | StmtKind::TraitDecl { name, .. }
            | StmtKind::Let { name, .. } => consider(name, stmt.span),
            StmtKind::Function(function) => consider(&function.name, function.span),
            StmtKind::ImplDecl { methods, .. } => {
                for method in methods {
                    consider(&method.name, method.span);
                }
            }
            _ => {}
        }
    }
    widest
}

fn required_import_of(needs: &NeedsStmt) -> RequiredImport {
    let kind = match &needs.kind {
        ImportKind::Module { alias } => RequiredImportKind::Module {
            alias: alias.clone(),
        },
        ImportKind::Symbols(symbols) => RequiredImportKind::Symbols(symbols.clone()),
        ImportKind::Wildcard => RequiredImportKind::Wildcard,
    };
    RequiredImport {
        path: needs.path.clone(),
        kind,
    }
}

// load, and an artefact whose recorded imports are not the ones the entry asked
fn reject_unloadable_bytecode(
    bytes: &[u8],
    declared: &[RequiredImport],
    entry_path: &Path,
    source: Arc<Source>,
    output_path: &Path,
) -> Result<(), String> {
    let sections = match aelys_bytecode::asm::deserialize_with_sections(bytes) {
        Ok(sections) => sections,
        Err(err) => {
            return Err(invalid_bytecode_error(
                err.to_string(),
                output_path,
                RejectedBytecodeOrigin::Compiler,
                RejectedBytecodeStage::Reader,
            ));
        }
    };

    if sections.requires != declared {
        return Err(invalid_bytecode_error(
            "recorded imports do not match the ones the program declares".to_string(),
            output_path,
            RejectedBytecodeOrigin::Compiler,
            RejectedBytecodeStage::Reader,
        ));
    }

    let required = required_modules(&sections.function, &sections.requires);
    let bundled: std::collections::HashSet<String> = sections
        .bundles
        .iter()
        .map(|bundle| bundle.name.clone())
        .collect();
    if let Some(reason) =
        unresolved_required_module(entry_path, source, &required, &bundled, sections.manifest)
    {
        return Err(invalid_bytecode_error(
            reason,
            output_path,
            RejectedBytecodeOrigin::Compiler,
            RejectedBytecodeStage::Loader,
        ));
    }

    Ok(())
}

// the verdict has to be taken on the bytes `run` will read back, not on the
fn reject_unverifiable_bytecode(
    bytes: &[u8],
    vm: &VM,
    output_path: &Path,
    origin: RejectedBytecodeOrigin,
) -> Result<(), String> {
    match aelys_bytecode::asm::deserialize_with_manifest(bytes) {
        Ok((function, _manifest, _bundles)) => {
            aelys_runtime::verify_function(&function, vm.heap(), 0).map_err(|reason| {
                invalid_bytecode_error(reason, output_path, origin, RejectedBytecodeStage::Verifier)
            })
        }
        Err(err) => Err(invalid_bytecode_error(
            err.to_string(),
            output_path,
            origin,
            RejectedBytecodeStage::Reader,
        )),
    }
}

fn module_load_error(err: AelysError, output_path: &Path) -> String {
    if let AelysError::Runtime(runtime) = &err
        && runtime.is_verifier_verdict()
        && let RuntimeErrorKind::InvalidBytecode(reason) = &runtime.kind
    {
        return invalid_bytecode_error(
            reason.clone(),
            output_path,
            RejectedBytecodeOrigin::Compiler,
            RejectedBytecodeStage::Verifier,
        );
    }
    err.to_string()
}

fn invalid_bytecode_error(
    reason: String,
    output_path: &Path,
    origin: RejectedBytecodeOrigin,
    stage: RejectedBytecodeStage,
) -> String {
    let kind = CompileErrorKind::EmittedBytecodeRejected {
        output: output_path.display().to_string(),
        reason,
        origin,
        stage,
        artifact: artifact_state(output_path),
    };
    format!("error[E{:04}]: {}", kind.code(), kind.message())
}

// the refusal never deletes an artifact it did not write, so a build reading
fn artifact_state(output_path: &Path) -> RejectedBytecodeArtifact {
    if output_path.exists() {
        RejectedBytecodeArtifact::PreviousLeftInPlace
    } else {
        RejectedBytecodeArtifact::Absent
    }
}

pub fn run_with_options(
    path: &str,
    output: Option<String>,
    opt_level: OptimizationLevel,
    warn_config: WarningConfig,
) -> Result<i32, String> {
    let output = output.map(PathBuf::from);
    let result =
        compile_to_avbc_gated(Path::new(path), output, opt_level, None, Some(&warn_config))?;

    let reported: Vec<_> = result
        .warnings
        .iter()
        .filter(|w| warn_config.is_enabled(&w.kind))
        .collect();
    for w in &reported {
        eprintln!("{}", w);
    }

    if !result.written {
        let count = reported.len();
        return Err(format!(
            "aborting due to {} warning{}",
            count,
            if count == 1 { "" } else { "s" }
        ));
    }

    eprintln!("Wrote {}", result.output_path.display());
    Ok(0)
}

#[allow(dead_code)]
pub fn emit_air(path: &str, opt_level: OptimizationLevel) -> Result<i32, String> {
    let _ = (path, opt_level);
    Err("--emit-air is no longer supported".to_string())
}

fn output_path_for(path: &Path) -> PathBuf {
    let mut output = path.to_path_buf();
    output.set_extension("avbc");
    output
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompileInput {
    Source,
    Assembly,
    Bytecode,
}

fn detect_format(path: &Path) -> CompileInput {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "aasm" => CompileInput::Assembly,
        "avbc" => CompileInput::Bytecode,
        _ => CompileInput::Source,
    }
}

fn assemble_to_avbc(path: &Path, output_path: &Path) -> Result<(), String> {
    let content = std::fs::read_to_string(path)
        .map_err(|err| format!("failed to read {}: {}", path.display(), err))?;
    let functions = aelys_bytecode::asm::assemble(&content).map_err(|err| err.to_string())?;
    if functions.is_empty() {
        return Err("no functions found in assembly file".to_string());
    }
    let function = reconstruct_function_hierarchy(functions);
    let src = Source::new(path.display().to_string(), &content);
    let bytes = aelys_bytecode::asm::serialize(&function)
        .map_err(|err| serialize_error(&err.to_string(), None, output_path, &src))?;

    let mut vm = VM::with_config_and_args(src.clone(), VmConfig::default(), Vec::new())
        .map_err(|err| err.to_string())?;
    if let Ok(abs_path) = path.canonicalize() {
        vm.set_script_path(abs_path.display().to_string());
    } else {
        vm.set_script_path(path.display().to_string());
    }

    let required = required_modules(&function, &[]);
    load_required_modules(
        &mut vm,
        path,
        src,
        &required,
        None,
        &std::collections::HashMap::new(),
    )
    .map_err(|err| match err {
        RequiredModuleError::Load(err) => module_load_error(err, output_path),
        RequiredModuleError::Native(message) => message,
    })?;

    reject_unverifiable_bytecode(&bytes, &vm, output_path, RejectedBytecodeOrigin::Assembly)?;

    std::fs::write(output_path, bytes)
        .map_err(|err| format!("failed to write {}: {}", output_path.display(), err))?;
    Ok(())
}

fn reconstruct_function_hierarchy(
    mut functions: Vec<aelys_bytecode::Function>,
) -> aelys_bytecode::Function {
    if functions.len() <= 1 {
        return functions
            .into_iter()
            .next()
            .unwrap_or_else(|| aelys_bytecode::Function::new(None, 0));
    }

    let mut main_func = functions.remove(0);
    main_func.nested_functions = functions;
    main_func
}

fn build_native_bundles(
    modules: &std::collections::HashMap<String, LoadedNativeInfo>,
) -> Result<Vec<NativeBundle>, String> {
    let mut bundles = Vec::new();
    for (name, info) in modules {
        let bytes = std::fs::read(&info.file_path)
            .map_err(|err| format!("failed to read {}: {}", info.file_path.display(), err))?;
        let checksum = compute_simple_hash(&bytes);
        let target = current_target_triple();
        bundles.push(NativeBundle {
            name: name.clone(),
            target,
            checksum,
            bytes,
        });
    }
    Ok(bundles)
}

fn compute_simple_hash(data: &[u8]) -> String {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;
    let mut hash = FNV_OFFSET;
    for &byte in data {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{:016x}", hash)
}

fn current_target_triple() -> String {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        "x86_64-unknown-linux-gnu".to_string()
    }
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        "aarch64-unknown-linux-gnu".to_string()
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        "x86_64-apple-darwin".to_string()
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        "aarch64-apple-darwin".to_string()
    }
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        "x86_64-pc-windows-msvc".to_string()
    }
    #[cfg(not(any(
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "x86_64"),
        all(target_os = "macos", target_arch = "aarch64"),
        all(target_os = "windows", target_arch = "x86_64"),
    )))]
    {
        "unknown".to_string()
    }
}

#[cfg(test)]
mod unloadable_gate_tests {
    use super::*;
    use aelys_bytecode::asm::{RequiredImport, RequiredImportKind};

    fn artifact_requiring(requires: &[RequiredImport]) -> Vec<u8> {
        let function = aelys_bytecode::Function::new(None, 0);
        aelys_bytecode::asm::serialize_with_sections(&function, None, None, requires)
            .expect("a function with no code serializes")
    }

    fn module_import(name: &str) -> RequiredImport {
        RequiredImport {
            path: vec![name.to_string()],
            kind: RequiredImportKind::Module { alias: None },
        }
    }

    fn entry_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("aelys_unloadable_gate_{name}"));
        std::fs::create_dir_all(&dir).expect("the entry directory is writable");
        std::fs::write(dir.join("holder.aelys"), "pub fn v() -> int { return 1 }\n")
            .expect("the module is writable");
        dir
    }

    fn verdict(dir: &Path, bytes: &[u8], declared: &[RequiredImport]) -> Result<(), String> {
        let entry = dir.join("main.aelys");
        let source = Source::new(entry.display().to_string(), "");
        reject_unloadable_bytecode(bytes, declared, &entry, source, &dir.join("main.avbc"))
    }

    #[test]
    fn an_artifact_naming_a_module_nothing_resolves_is_refused() {
        let dir = entry_dir("unresolved");
        let requires = vec![module_import("nowhere")];
        let err = verdict(&dir, &artifact_requiring(&requires), &requires)
            .expect_err("a module nothing resolves has to be refused");
        assert!(err.starts_with("error[E0435]:"), "{err}");
        assert!(err.contains("cannot be loaded"), "{err}");
        assert!(err.contains("nowhere"), "{err}");
    }

    #[test]
    fn an_artifact_naming_a_module_that_resolves_is_accepted() {
        let dir = entry_dir("resolved");
        let requires = vec![module_import("holder")];
        verdict(&dir, &artifact_requiring(&requires), &requires)
            .expect("a module sitting next to the entry resolves");
    }

    #[test]
    fn an_artifact_recording_imports_the_entry_never_declared_is_refused() {
        let dir = entry_dir("mismatch");
        let recorded = vec![module_import("holder")];
        let declared = vec![module_import("holder"), module_import("std")];
        let err = verdict(&dir, &artifact_requiring(&recorded), &declared)
            .expect_err("recorded imports that are not the declared ones have to be refused");
        assert!(err.starts_with("error[E0435]:"), "{err}");
        assert!(err.contains("recorded imports"), "{err}");
    }
}
