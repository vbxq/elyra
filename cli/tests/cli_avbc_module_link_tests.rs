use aelys_opt::OptimizationLevel;

fn source_and_artifact(name: &str, files: &[(&str, &str)]) -> (String, String) {
    let dir = std::env::temp_dir().join(format!("aelys_cli_link_{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (relative, content) in files {
        let path = dir.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, content).unwrap();
    }

    let src_path = dir.join("main.aelys");
    let from_source = std::process::Command::new(env!("CARGO_BIN_EXE_aelys-cli"))
        .arg("run")
        .arg(&src_path)
        .output()
        .unwrap();
    assert_eq!(
        from_source.status.code(),
        Some(0),
        "the program does not run from source: {}",
        String::from_utf8_lossy(&from_source.stderr)
    );

    let bytecode_path =
        aelys_cli::cli::commands::compile::compile_to_avbc(&src_path, OptimizationLevel::None)
            .unwrap_or_else(|err| panic!("{name} did not compile: {err}"));

    let from_artifact = std::process::Command::new(env!("CARGO_BIN_EXE_aelys-cli"))
        .arg("run")
        .arg(&bytecode_path)
        .output()
        .unwrap();
    assert_eq!(
        from_artifact.status.code(),
        Some(0),
        "the artefact does not run: {}",
        String::from_utf8_lossy(&from_artifact.stderr)
    );

    (
        String::from_utf8_lossy(&from_source.stdout).into_owned(),
        String::from_utf8_lossy(&from_artifact.stdout).into_owned(),
    )
}

fn assert_agrees(name: &str, files: &[(&str, &str)], expected: &str) {
    let (source, artifact) = source_and_artifact(name, files);
    assert_eq!(source.lines().next(), Some(expected), "{name} from source");
    assert_eq!(
        artifact.lines().next(),
        Some(expected),
        "{name} from its artefact"
    );
}

#[test]
fn an_unqualified_module_function_survives_its_own_bytecode() {
    assert_agrees(
        "unqualified_fn",
        &[
            ("holder.aelys", "pub fn from_module() -> int { return 5 }\n"),
            (
                "main.aelys",
                "needs holder\nfn probe() -> int { return from_module() + 1 }\nprintln(probe())\n",
            ),
        ],
        "6",
    );
}

#[test]
fn an_unqualified_module_global_survives_its_own_bytecode() {
    assert_agrees(
        "unqualified_let",
        &[
            ("holder.aelys", "pub let SEED: int = 7\n"),
            (
                "main.aelys",
                "needs holder\nfn probe() -> int { return SEED + 1 }\nprintln(probe())\n",
            ),
        ],
        "8",
    );
}

#[test]
fn an_aliased_module_survives_its_own_bytecode() {
    assert_agrees(
        "aliased",
        &[
            ("holder.aelys", "pub fn from_module() -> int { return 5 }\n"),
            (
                "main.aelys",
                "needs holder as h\nprintln(h::from_module() + 1)\n",
            ),
        ],
        "6",
    );
}

#[test]
fn a_nested_module_path_survives_its_own_bytecode() {
    assert_agrees(
        "nested_path",
        &[
            ("lib/helper.aelys", "pub fn help() -> int { return 9 }\n"),
            ("main.aelys", "needs lib::helper\nprintln(help())\n"),
        ],
        "9",
    );
}

#[test]
fn a_selective_import_survives_its_own_bytecode() {
    assert_agrees(
        "selective",
        &[
            ("holder.aelys", "pub fn from_module() -> int { return 5 }\n"),
            (
                "main.aelys",
                "needs from_module from holder\nprintln(from_module() + 1)\n",
            ),
        ],
        "6",
    );
}

#[test]
fn a_transitive_module_survives_its_own_bytecode() {
    assert_agrees(
        "transitive",
        &[
            ("deep.aelys", "pub fn deep_val() -> int { return 3 }\n"),
            (
                "mid.aelys",
                "needs deep\npub fn mid_val() -> int { return deep_val() + 1 }\n",
            ),
            ("main.aelys", "needs mid\nprintln(mid_val())\n"),
        ],
        "4",
    );
}

#[test]
fn an_aliased_stdlib_module_survives_its_own_bytecode() {
    assert_agrees(
        "aliased_std",
        &[("main.aelys", "needs std::math as m\nprintln(m::abs(-4))\n")],
        "4",
    );
}

#[test]
fn an_artifact_records_the_imports_the_entry_declares() {
    use aelys_bytecode::asm::{RequiredImport, RequiredImportKind};

    let dir = std::env::temp_dir().join("aelys_cli_link_recorded");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("holder.aelys"), "pub fn v() -> int { return 1 }\n").unwrap();
    let src_path = dir.join("main.aelys");
    std::fs::write(
        &src_path,
        "needs holder as h\nneeds std::math\nprintln(h::v())\n",
    )
    .unwrap();

    let bytecode_path =
        aelys_cli::cli::commands::compile::compile_to_avbc(&src_path, OptimizationLevel::None)
            .unwrap();
    let bytes = std::fs::read(bytecode_path).unwrap();
    let sections = aelys_bytecode::asm::deserialize_with_sections(&bytes).unwrap();

    assert_eq!(
        sections.requires,
        vec![
            RequiredImport {
                path: vec!["holder".to_string()],
                kind: RequiredImportKind::Module {
                    alias: Some("h".to_string())
                },
            },
            RequiredImport {
                path: vec!["std".to_string(), "math".to_string()],
                kind: RequiredImportKind::Module { alias: None },
            },
        ],
        "the artefact has to carry the imports in the order the entry declares them"
    );
}

#[test]
fn a_program_with_no_import_records_no_section() {
    let dir = std::env::temp_dir().join("aelys_cli_link_no_import");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let src_path = dir.join("main.aelys");
    std::fs::write(&src_path, "println(1)\n").unwrap();

    let bytecode_path =
        aelys_cli::cli::commands::compile::compile_to_avbc(&src_path, OptimizationLevel::None)
            .unwrap();
    let bytes = std::fs::read(bytecode_path).unwrap();
    let sections = aelys_bytecode::asm::deserialize_with_sections(&bytes).unwrap();

    assert!(sections.requires.is_empty());
}

#[test]
fn an_assembled_artifact_whose_module_resolves_nowhere_is_refused() {
    let dir = std::env::temp_dir().join("aelys_cli_link_unresolvable");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("holder.aelys"), "pub fn v() -> int { return 1 }\n").unwrap();
    let src_path = dir.join("main.aelys");
    std::fs::write(&src_path, "needs holder\nprintln(holder::v())\n").unwrap();

    let bytecode_path =
        aelys_cli::cli::commands::compile::compile_to_avbc(&src_path, OptimizationLevel::None)
            .unwrap();

    let asm_path = dir.join("main.aasm");
    let disassembled = std::process::Command::new(env!("CARGO_BIN_EXE_aelys-cli"))
        .arg("asm")
        .arg(&bytecode_path)
        .output()
        .unwrap();
    assert_eq!(disassembled.status.code(), Some(0));
    assert!(asm_path.exists());

    let elsewhere = std::env::temp_dir().join("aelys_cli_link_unresolvable_out");
    let _ = std::fs::remove_dir_all(&elsewhere);
    std::fs::create_dir_all(&elsewhere).unwrap();
    let moved = elsewhere.join("main.aasm");
    std::fs::rename(&asm_path, &moved).unwrap();

    let refused = std::process::Command::new(env!("CARGO_BIN_EXE_aelys-cli"))
        .arg("compile")
        .arg(&moved)
        .output()
        .unwrap();
    assert_eq!(
        refused.status.code(),
        Some(1),
        "an artefact whose module resolves nowhere must not be written"
    );
    assert!(
        !elsewhere.join("main.avbc").exists(),
        "the refusal left an artefact behind"
    );
}
