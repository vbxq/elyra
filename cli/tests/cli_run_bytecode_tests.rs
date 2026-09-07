use aelys_cli::cli::commands::run::run_with_options;
use aelys_common::WarningConfig;
use aelys_opt::OptimizationLevel;

#[test]
fn run_accepts_bytecode_with_magic() {
    let dir = std::env::temp_dir().join("aelys_cli_run_bytecode");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bytecode_path = dir.join("program.avbc");

    let mut function = aelys_bytecode::Function::new(None, 0);
    function.constants.push(aelys_bytecode::Constant::Int(2));
    function.num_registers = 1;
    function.emit_b(aelys_bytecode::OpCode::LoadK, 0, 0, 1);
    function.emit_a(aelys_bytecode::OpCode::Return, 0, 0, 0, 1);
    function.finalize_bytecode();

    let bytes = aelys_bytecode::asm::serialize(&function).unwrap();
    std::fs::write(&bytecode_path, bytes).unwrap();

    let result = run_with_options(
        bytecode_path.to_str().unwrap(),
        Vec::new(),
        Vec::new(),
        OptimizationLevel::Standard,
        WarningConfig::new(),
    );

    assert!(result.is_ok());
}

#[test]
fn run_bytecode_registers_stdlib_globals() {
    let dir = std::env::temp_dir().join("aelys_cli_run_stdlib");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let src_path = dir.join("hello.aelys");
    std::fs::write(&src_path, "needs std::io\nio::print(\"hi\")\n").unwrap();

    let bytecode_path =
        aelys_cli::cli::commands::compile::compile_to_avbc(&src_path, OptimizationLevel::None)
            .unwrap();

    let result = run_with_options(
        bytecode_path.to_str().unwrap(),
        Vec::new(),
        Vec::new(),
        OptimizationLevel::Standard,
        WarningConfig::new(),
    );

    assert!(result.is_ok());
}

fn compile_and_run_capturing_stdout(name: &str, source: &str) -> (String, String, Option<i32>) {
    let dir = std::env::temp_dir().join(format!("aelys_cli_avbc_{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let src_path = dir.join(format!("{name}.aelys"));
    std::fs::write(&src_path, source).unwrap();

    let bytecode_path =
        aelys_cli::cli::commands::compile::compile_to_avbc(&src_path, OptimizationLevel::Standard)
            .unwrap();

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_aelys-cli"))
        .arg("run")
        .arg(&bytecode_path)
        .output()
        .unwrap();

    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.code(),
    )
}

#[test]
fn compiled_bytecode_calls_a_string_stdlib_helper_twice() {
    let (stdout, stderr, code) = compile_and_run_capturing_stdout(
        "string_helper_twice",
        "needs std::io\nneeds std::convert\nfn f(t: string) -> int { t.len() }\n\
         io::println(convert::to_string(f(\"abcd\")))\nio::println(convert::to_string(f(\"ab\")))\n",
    );

    assert_eq!(code, Some(0), "stdout:\n{stdout}\nstderr:\n{stderr}");
    let printed: Vec<&str> = stdout.lines().collect();
    assert_eq!(
        printed.first(),
        Some(&"4"),
        "first call printed the wrong value; stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert_eq!(
        printed.get(1),
        Some(&"2"),
        "second call to the same helper failed; stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        !stderr.contains("undefined variable"),
        "a global layout name was lost:\n{stderr}"
    );
}

#[test]
fn compiled_bytecode_calls_a_math_stdlib_helper_twice() {
    let (stdout, stderr, code) = compile_and_run_capturing_stdout(
        "math_helper_twice",
        "needs std::io\nneeds std::convert\nneeds std::math\nfn g(n: int) -> int { math::abs(n) }\n\
         io::println(convert::to_string(g(-4)))\nio::println(convert::to_string(g(-2)))\n",
    );

    assert_eq!(code, Some(0), "stdout:\n{stdout}\nstderr:\n{stderr}");
    let printed: Vec<&str> = stdout.lines().collect();
    assert_eq!(
        printed.first(),
        Some(&"4"),
        "stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert_eq!(
        printed.get(1),
        Some(&"2"),
        "stdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

#[test]
fn an_inherent_impl_method_survives_its_own_bytecode() {
    let (stdout, stderr, code) = compile_and_run_capturing_stdout(
        "inherent_impl_method",
        "struct Point { x: int }\n\
         impl Point {\n    fn score(self) -> int {\n        return self.x + 1;\n    }\n}\n\
         Point { x: 7 }.score()\n",
    );

    assert_eq!(code, Some(0), "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert_eq!(
        stdout.trim(),
        "8",
        "the method did not run from the product; stdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

#[test]
fn a_trait_impl_method_survives_its_own_bytecode() {
    let (stdout, stderr, code) = compile_and_run_capturing_stdout(
        "trait_impl_method",
        "trait Scored {\n    fn score(self) -> int;\n}\n\
         struct Point { x: int }\n\
         impl Scored for Point {\n    fn score(self) -> int {\n        return self.x + 1;\n    }\n}\n\
         Point { x: 7 }.score()\n",
    );

    assert_eq!(code, Some(0), "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert_eq!(
        stdout.trim(),
        "8",
        "the trait method did not run from the product; stdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

#[test]
fn an_inherent_impl_with_no_method_survives_its_own_bytecode() {
    let (stdout, stderr, code) = compile_and_run_capturing_stdout(
        "inherent_impl_no_method",
        "struct Point { x: int }\nimpl Point {\n}\nPoint { x: 7 }.x + 1\n",
    );

    assert_eq!(code, Some(0), "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert_eq!(stdout.trim(), "8", "stdout:\n{stdout}\nstderr:\n{stderr}");
}

#[test]
fn a_trait_impl_with_no_method_survives_its_own_bytecode() {
    let (stdout, stderr, code) = compile_and_run_capturing_stdout(
        "trait_impl_no_method",
        "trait Holder {\n    type Item\n    const CAP: int\n}\n\
         struct Point { x: int }\n\
         impl Holder for Point {\n    type Item = int\n    const CAP: int = 34\n}\n\
         fn widen(v: Point::Item) -> int {\n    return v + Point::CAP\n}\n\
         widen(Point { x: 7 }.x) + 1\n",
    );

    assert_eq!(code, Some(0), "stdout:\n{stdout}\nstderr:\n{stderr}");
    assert_eq!(
        stdout.trim(),
        "42",
        "42 is 7 through 'Point::Item' plus the 34 of 'Point::CAP' plus 1; \
         stdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

#[test]
fn a_module_named_like_the_mangling_prefix_still_loads_from_bytecode() {
    let dir = std::env::temp_dir().join("aelys_cli_avbc_prefix_module");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("__aelys_struct.aelys"),
        "pub fn helper() -> int {\n    return 5;\n}\n",
    )
    .unwrap();
    let src_path = dir.join("prefix_module.aelys");
    std::fs::write(
        &src_path,
        "needs __aelys_struct\n__aelys_struct::helper()\n",
    )
    .unwrap();

    let bytecode_path =
        aelys_cli::cli::commands::compile::compile_to_avbc(&src_path, OptimizationLevel::Standard)
            .unwrap();

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_aelys-cli"))
        .arg("run")
        .arg(&bytecode_path)
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        stdout.trim(),
        "5",
        "the module scan mistook a real import for a mangled method; stdout:\n{stdout}\nstderr:\n{stderr}"
    );
}
