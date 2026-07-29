use aelys_cli::cli::commands::run::run_with_options;
use aelys_common::WarningConfig;
use aelys_opt::OptimizationLevel;

#[test]
fn run_rejects_invalid_vm_args() {
    let err = run_with_options(
        "missing.aelys",
        Vec::new(),
        vec!["-ae.max-heap=1".to_string()],
        OptimizationLevel::None,
        WarningConfig::new(),
    )
    .unwrap_err();

    assert!(err.contains("invalid value for"));
}

#[test]
fn run_accepts_aasm_file() {
    let dir = std::env::temp_dir().join("aelys_cli_run_aasm");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let aasm_path = dir.join("program.aasm");
    std::fs::write(
        &aasm_path,
        ".version 2\n.function 0\n  .arity 0\n  .registers 1\n  .constants\n    0: int 2\n  .code\n    0000: LoadK r0, 0\n    0001: Return r0\n",
    )
    .unwrap();

    let result = run_with_options(
        aasm_path.to_str().unwrap(),
        Vec::new(),
        Vec::new(),
        OptimizationLevel::Standard,
        WarningConfig::new(),
    );

    assert!(result.is_ok());
}

#[test]
fn run_enforces_instruction_budget() {
    let dir = std::env::temp_dir().join("aelys_cli_instruction_budget");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("loop.aelys");
    std::fs::write(&path, "while true {}\n").unwrap();

    let error = run_with_options(
        path.to_str().unwrap(),
        Vec::new(),
        vec!["-ae.max-instructions=10".to_string()],
        OptimizationLevel::None,
        WarningConfig::new(),
    )
    .unwrap_err();
    assert!(error.contains("instruction budget exhausted"));
}

#[test]
fn run_translates_sys_exit_to_process_status() {
    let dir = std::env::temp_dir().join("aelys_cli_sys_exit");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("exit.aelys");
    std::fs::write(&path, "needs std.sys\nsys.exit(7)\n").unwrap();

    let status = run_with_options(
        path.to_str().unwrap(),
        Vec::new(),
        Vec::new(),
        OptimizationLevel::None,
        WarningConfig::new(),
    )
    .unwrap();
    assert_eq!(status, 7);
}
