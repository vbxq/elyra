use aelys_cli::cli::commands::compile::compile_to_avbc_with_output;
use aelys_opt::OptimizationLevel;
use std::path::{Path, PathBuf};

// refuses whatever the front-end does, so no compiler fix can defuse this gate
const BAD_SCHEMA_INDEX: &str = r#".version 3

.function 0
  .arity 0
  .registers 4

  .schemas
    0: "witness::P" 0 0 2
      0: "a" "int:I64"
      1: "b" "int:I64"

  .jit_unsupported_struct true

  .code
    0000: LoadI     r1, 3
    0001: LoadI     r2, 4
    0002: StructNew  1, r0, r1, 2, 0
    0005: Return    r0
"#;

// the same defect one level down, behind an entry that calls into a real module
const BAD_SCHEMA_INDEX_BELOW_ENTRY: &str = r#".version 3

.function 0
  .arity 0
  .registers 2

  .globals
    0: "carrier::probe"

  .code
    0000: CallGlobal r0, 0, 0
    0001: Return    r0

.function 1
  .name "inner"
  .arity 0
  .registers 4

  .schemas
    0: "witness::P" 0 0 2
      0: "a" "int:I64"
      1: "b" "int:I64"

  .jit_unsupported_struct true

  .code
    0000: LoadI     r1, 3
    0001: LoadI     r2, 4
    0002: StructNew  1, r0, r1, 2, 0
    0005: Return    r0
"#;

const STRUCT_PROGRAM: &str = r#"struct P { a: int, b: int }

fn probe() -> int {
    let p = P { a: 3, b: 4 }
    p.a + p.b
}

println(probe())
"#;

const FREE_FUNCTION_TYPE_PARAM: &str = r#"struct Pair<A, B> { a: A, b: B }
struct G { n: int }

fn grow<T>(g: G, x: T, y: T) -> int {
    let p = Pair { a: x, b: y }
    g.n
}

fn probe() -> int {
    let g = G { n: 7 }
    grow(g, 1, 2)
}

println(probe())
"#;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aelys_compile_verifier_gate_{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch directory");
    dir
}

fn write_source(dir: &Path, name: &str, source: &str) -> PathBuf {
    let path = dir.join(format!("{name}.aelys"));
    std::fs::write(&path, source).expect("source file");
    path
}

fn write_assembly(dir: &Path, name: &str, assembly: &str) -> PathBuf {
    let path = dir.join(format!("{name}.aasm"));
    std::fs::write(&path, assembly).expect("assembly file");
    path
}

fn refusal(input: &Path, output: &Path) -> String {
    match compile_to_avbc_with_output(
        input,
        Some(output.to_path_buf()),
        OptimizationLevel::Standard,
        None,
    ) {
        Ok(result) => panic!(
            "compile reported success and wrote {}",
            result.output_path.display()
        ),
        Err(error) => error,
    }
}

#[test]
fn compile_refuses_to_write_bytecode_its_own_verifier_rejects() {
    let dir = scratch("bad_schema_index");
    let witness = write_assembly(&dir, "witness", BAD_SCHEMA_INDEX);
    let output = dir.join("witness.avbc");

    let error = refusal(&witness, &output);

    assert!(
        error.contains("E0435"),
        "the refusal carries no diagnostic code:\n{error}"
    );
    assert!(
        error.contains("StructNew at 2 has invalid schema index"),
        "the refusal does not quote the verifier:\n{error}"
    );
    assert!(
        error.contains("witness.avbc"),
        "the refusal does not name the file it refused to write:\n{error}"
    );
    assert!(
        !output.exists(),
        "a refused compilation still left {} on disk",
        output.display()
    );

    // the gate is only worth having if it refuses what the runtime refuses and
    let run = std::process::Command::new(env!("CARGO_BIN_EXE_aelys-cli"))
        .arg("run")
        .arg(&witness)
        .output()
        .expect("aelys-cli run");
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert_eq!(
        run.status.code(),
        Some(1),
        "the runtime accepted what compile refused:\n{stderr}"
    );
    assert!(
        stderr.contains("StructNew at 2 has invalid schema index"),
        "the runtime refused the witness for another reason:\n{stderr}"
    );
}

#[test]
fn compile_still_accepts_a_program_the_verifier_accepts() {
    let dir = scratch("free_function_type_param");
    let source = write_source(&dir, "healthy", FREE_FUNCTION_TYPE_PARAM);
    let output = dir.join("healthy.avbc");

    let result = compile_to_avbc_with_output(
        &source,
        Some(output.clone()),
        OptimizationLevel::Standard,
        None,
    )
    .expect("an equivalent free function must still compile");
    assert!(result.output_path.is_file());

    let run = std::process::Command::new(env!("CARGO_BIN_EXE_aelys-cli"))
        .arg("run")
        .arg(&output)
        .output()
        .expect("aelys-cli run");

    let stdout = String::from_utf8_lossy(&run.stdout);
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert_eq!(
        run.status.code(),
        Some(0),
        "the accepted bytecode did not run:\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.lines().any(|line| line == "7"),
        "the accepted bytecode printed the wrong value:\n{stdout}"
    );
}

#[test]
fn a_refusal_leaves_an_existing_artifact_where_it_was() {
    let dir = scratch("stale_artifact");
    let witness = write_assembly(&dir, "witness", BAD_SCHEMA_INDEX);
    let output = dir.join("witness.avbc");
    let previous = b"a healthy artifact from an earlier build";
    std::fs::write(&output, previous).expect("previous artifact");

    let error = refusal(&witness, &output);

    assert_eq!(
        std::fs::read(&output).expect("previous artifact"),
        previous,
        "the refusal overwrote or deleted an artifact it did not write"
    );
    assert!(
        error.contains("was left untouched"),
        "the refusal let a build read the stale artifact as absent:\n{error}"
    );
    assert!(
        !error.contains("nothing was written"),
        "the refusal claims a state the file system contradicts:\n{error}"
    );
}

#[test]
fn compiling_assembly_cannot_launder_bytecode_the_verifier_rejects() {
    let dir = scratch("assembly_backdoor");
    let source = write_source(&dir, "witness", STRUCT_PROGRAM);

    let assembly = dir.join("witness.aasm");
    let asm = std::process::Command::new(env!("CARGO_BIN_EXE_aelys-cli"))
        .arg("asm")
        .arg(&source)
        .arg("-o")
        .arg(&assembly)
        .output()
        .expect("aelys-cli asm");
    assert!(
        assembly.is_file(),
        "asm did not produce the assembly this test needs: {}",
        String::from_utf8_lossy(&asm.stderr)
    );

    // the untampered round trip has to succeed, or the refusal below would
    let straight = dir.join("straight.avbc");
    compile_to_avbc_with_output(
        &assembly,
        Some(straight.clone()),
        OptimizationLevel::Standard,
        None,
    )
    .expect("assembly produced by 'asm' must compile back");
    assert!(straight.is_file());

    let clean = std::fs::read_to_string(&assembly).expect("assembly text");
    let text = clean.replacen("StructNew  0,", "StructNew  9,", 1);
    assert_ne!(
        text, clean,
        "the assembly no longer carries the instruction this test tampers with:\n{clean}"
    );
    let tampered = write_assembly(&dir, "tampered", &text);
    let output = dir.join("tampered.avbc");

    let error = refusal(&tampered, &output);

    assert!(
        error.contains("E0435"),
        "the assembly path refused without a diagnostic code:\n{error}"
    );
    assert!(
        error.contains("has invalid schema index"),
        "the assembly path refused without quoting the verifier:\n{error}"
    );
    assert!(
        !error.contains("defect in the compiler"),
        "the assembly path blamed the compiler for bytes the user wrote:\n{error}"
    );
    assert!(
        !output.exists(),
        "the assembly path still left {} on disk",
        output.display()
    );
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the cli crate always has a workspace parent")
        .to_path_buf()
}

fn collect_sources(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if name == "target" || name.starts_with('.') {
                continue;
            }
            collect_sources(&path, found);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("aelys") {
            found.push(path);
        }
    }
}

fn verifier_verdict(bytecode: &Path) -> Result<(), String> {
    let bytes = std::fs::read(bytecode).map_err(|err| err.to_string())?;
    let (function, _manifest, _bundles) =
        aelys_bytecode::asm::deserialize_with_manifest(&bytes).map_err(|err| err.to_string())?;
    let source = aelys_syntax::Source::new(bytecode.display().to_string(), "");
    let vm = aelys_runtime::VM::with_config_and_args(
        source,
        aelys_runtime::VmConfig::default(),
        Vec::new(),
    )
    .map_err(|err| err.to_string())?;
    aelys_runtime::verify_function(&function, vm.heap(), 0)
}

#[test]
fn no_source_in_the_tree_compiles_to_bytecode_the_verifier_rejects() {
    let root = repo_root();
    let mut sources = Vec::new();
    collect_sources(&root, &mut sources);
    sources.sort();
    assert!(
        sources.len() > 60,
        "expected to discover the checked-in Aelys sources, found {}",
        sources.len()
    );

    let out_dir = scratch("corpus");
    let mut compiled = 0usize;
    let mut rejected = Vec::new();

    for (index, source) in sources.iter().enumerate() {
        let output = out_dir.join(format!("corpus_{index}.avbc"));
        let relative = source.strip_prefix(&root).unwrap_or(source);
        // the gate's own refusal is the case this walk exists to find, so it is
        match compile_to_avbc_with_output(
            source,
            Some(output.clone()),
            OptimizationLevel::None,
            None,
        ) {
            Ok(_) => {}
            Err(error) if error.contains("E0435") => {
                rejected.push(format!("{}: {error}", relative.display()));
                continue;
            }
            Err(_) => continue,
        }
        compiled += 1;
        if let Err(reason) = verifier_verdict(&output) {
            rejected.push(format!("{}: {reason}", relative.display()));
        }
    }

    let _ = std::fs::remove_dir_all(&out_dir);

    assert!(
        rejected.is_empty(),
        "{} source(s) compiled to bytecode the verifier refuses:\n{}",
        rejected.len(),
        rejected.join("\n")
    );
    assert!(
        compiled >= 40,
        "only {compiled} of {} sources compiled, so this gate checked almost nothing",
        sources.len()
    );
}

const CARRIER_MODULE: &str = r#"pub fn probe() -> int {
    7
}
"#;

#[test]
fn a_defect_below_the_entry_function_is_refused_under_the_same_code() {
    let dir = scratch("below_entry");
    write_source(&dir, "carrier", CARRIER_MODULE);
    let main = write_assembly(&dir, "main", BAD_SCHEMA_INDEX_BELOW_ENTRY);
    let output = dir.join("main.avbc");

    let error = refusal(&main, &output);

    assert!(
        error.contains("E0435"),
        "a defect below the entry function was refused with a raw message:\n{error}"
    );
    assert!(
        error.contains("StructNew at 2 has invalid schema index"),
        "the refusal does not quote the verifier on the nested function:\n{error}"
    );
    assert!(
        !output.exists(),
        "the nested defect still left {} on disk",
        output.display()
    );

    // the same entry without its module: the verdict cannot be taken at all
    let orphan = scratch("below_entry_orphan");
    let alone = write_assembly(&orphan, "main", BAD_SCHEMA_INDEX_BELOW_ENTRY);
    let orphan_output = orphan.join("main.avbc");
    let missing = refusal(&alone, &orphan_output);
    assert!(
        missing.contains("module not found: 'carrier'"),
        "the assembly path verified without loading the module it requires:\n{missing}"
    );
    assert!(
        !orphan_output.exists(),
        "the assembly path wrote {} without its module",
        orphan_output.display()
    );
}

const INLINE_RECURSIVE: &str = r#"@inline
fn fact(n: int) -> int {
    if n <= 1 { return 1 }
    return n * fact(n - 1)
}

println(fact(5))
"#;

#[test]
fn a_compile_that_exits_nonzero_leaves_no_artifact() {
    let dir = scratch("werror");
    let source = write_source(&dir, "warned", INLINE_RECURSIVE);
    let output = dir.join("warned.avbc");

    let cli = env!("CARGO_BIN_EXE_aelys-cli");
    let accepted = std::process::Command::new(cli)
        .args(["compile", &source.display().to_string(), "-o"])
        .arg(&output)
        .arg("-Wall")
        .output()
        .expect("aelys-cli compile");
    assert_eq!(
        accepted.status.code(),
        Some(0),
        "the fixture must warn without failing:\n{}",
        String::from_utf8_lossy(&accepted.stderr)
    );
    assert!(
        String::from_utf8_lossy(&accepted.stderr).contains("warning"),
        "the fixture stopped warning, so -Werror below proves nothing"
    );
    std::fs::remove_file(&output).expect("accepted artifact");

    let refused = std::process::Command::new(cli)
        .args(["compile", &source.display().to_string(), "-o"])
        .arg(&output)
        .args(["-Wall", "-Werror"])
        .output()
        .expect("aelys-cli compile");
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert_eq!(
        refused.status.code(),
        Some(1),
        "-Werror did not fail on a warning:\n{stderr}"
    );
    assert!(
        !output.exists(),
        "-Werror reported failure and still left {} on disk",
        output.display()
    );
}

#[test]
fn a_name_the_encoder_cannot_carry_is_refused_with_a_code_and_a_span() {
    let dir = scratch("name_too_long");
    let name = "A".repeat(70_000);
    let source = write_source(
        &dir,
        "wide",
        &format!("struct {name} {{ n: int }}\nlet v = {name} {{ n: 5 }}\nv.n\n"),
    );
    let output = dir.join("wide.avbc");

    let refusal = refusal(&source, &output);
    assert!(
        refusal.contains("error[E0436]"),
        "a refusal that renounces has to carry a code: {}",
        &refusal[..refusal.len().min(200)]
    );
    assert!(
        refusal.contains("wide.aelys:1:1"),
        "the refusal has to point at the declaration it names: {}",
        &refusal[..refusal.len().min(200)]
    );
    assert!(
        refusal.contains("(70000 chars)"),
        "the refusal has to say how wide the name it blames is: {}",
        &refusal[..refusal.len().min(200)]
    );
    assert!(
        !output.exists(),
        "the refusal left {} on disk",
        output.display()
    );
}
