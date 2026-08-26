use aelys_cli::cli::commands::compile::compile_to_avbc;
use aelys_opt::OptimizationLevel;

#[test]
fn compile_writes_avbc() {
    let dir = std::env::temp_dir().join("aelys_cli_compile_test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let src_path = dir.join("main.aelys");
    std::fs::write(&src_path, "let x = 1\n").unwrap();

    let output = compile_to_avbc(&src_path, OptimizationLevel::None).unwrap();

    assert!(output.exists());
    assert_eq!(output.extension().unwrap(), "avbc");
}

#[test]
fn compile_errors_keep_the_named_diagnostic_code() {
    let dir = std::env::temp_dir().join("aelys_cli_compile_diagnostic_test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let src_path = dir.join("main.aelys");
    std::fs::write(&src_path, "type(42)\n").unwrap();

    let error = compile_to_avbc(&src_path, OptimizationLevel::None)
        .expect_err("runtime type discovery must be rejected");

    assert!(error.contains("error[E0301]"), "{error}");
    assert!(error.contains("undefined variable: type"), "{error}");
}

#[test]
fn compile_accepts_a_type_imported_from_another_module() {
    let dir = std::env::temp_dir().join("aelys_cli_compile_cross_module_types_test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("geometry.aelys"),
        "pub struct Point {\n    pub x: int,\n    pub y: int,\n}\n\npub trait Norm {\n    fn norm(self) -> int\n}\n\nimpl Norm for Point {\n    fn norm(self) -> int { self.x * self.x + self.y * self.y }\n}\n",
    )
    .unwrap();
    let src_path = dir.join("main.aelys");
    std::fs::write(
        &src_path,
        "needs geometry\nlet p = Point { x: 3, y: 4 }\np.norm()\n",
    )
    .unwrap();

    let output = compile_to_avbc(&src_path, OptimizationLevel::None)
        .expect("an imported type must compile to bytecode");

    assert!(output.exists());
}
