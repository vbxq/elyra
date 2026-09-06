use aelys_cli::cli::commands::asm::asm_transform;

#[test]
fn asm_emits_aasm_from_source() {
    let dir = std::env::temp_dir().join("aelys_cli_asm_test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let src_path = dir.join("sample.aelys");
    std::fs::write(&src_path, "let x = 1\n").unwrap();

    let output = asm_transform(&src_path).unwrap();

    assert!(output.exists());
    assert_eq!(output.extension().unwrap(), "aasm");
}

#[test]
fn asm_compile_and_run_agree_on_a_program_that_uses_a_builtin() {
    let dir = std::env::temp_dir().join("aelys_cli_asm_builtin");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let src_path = dir.join("builtin.aelys");
    std::fs::write(&src_path, "println(\"hi\")\n0\n").unwrap();

    let asm_output = asm_transform(&src_path).expect("asm accepts what run executes");
    let text = std::fs::read_to_string(&asm_output).unwrap();
    assert!(
        text.contains("io::println"),
        "the disassembly lost the builtin call:\n{text}"
    );

    let bytecode_path = aelys_cli::cli::commands::compile::compile_to_avbc(
        &src_path,
        aelys_opt::OptimizationLevel::Standard,
    )
    .expect("compile accepts the same program");

    for target in [&src_path, &bytecode_path] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_aelys-cli"))
            .arg("run")
            .arg(target)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(
            stdout.lines().next(),
            Some("hi"),
            "running {} disagreed; stdout:\n{stdout}\nstderr:\n{stderr}",
            target.display()
        );
    }
}
