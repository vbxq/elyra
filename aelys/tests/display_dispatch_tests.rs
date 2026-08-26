use aelys_opt::OptimizationLevel;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

const TYPES: &str = r#"
needs std::io
needs std::convert

struct Loud { v: int }
impl Display for Loud {
    fn to_display(self) -> string { "loud" }
}

struct Quiet { v: int }
impl Display for Quiet {
    fn to_display(self) -> string { "quiet" }
}
"#;

fn run_string(source: &str) -> String {
    let mut vm = aelys::new_vm().expect("vm");
    let result = aelys::run_with_vm_and_opt(&mut vm, source, "<test>", OptimizationLevel::Standard)
        .expect("program should run");
    let ptr = result.as_ptr().expect("program should return a string");
    let heap = vm.heap();
    let object = heap
        .get(aelys_runtime::vm::GcRef::new(ptr))
        .expect("live string object");
    match &object.kind {
        aelys_runtime::vm::ObjectKind::String(text) => text.as_str().to_string(),
        other => panic!("expected a string result, got {other:?}"),
    }
}

// a stale binary would silently prove nothing, so the build runs once per test binary
fn cli_binary() -> PathBuf {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    BINARY
        .get_or_init(|| {
            let status = Command::new(env!("CARGO"))
                .args(["build", "-p", "aelys-cli"])
                .status()
                .expect("cargo build should run");
            assert!(status.success(), "building aelys-cli failed");
            let mut directory = std::env::current_exe().expect("test executable path");
            directory.pop();
            if directory.ends_with("deps") {
                directory.pop();
            }
            let binary = directory.join(format!("aelys-cli{}", std::env::consts::EXE_SUFFIX));
            assert!(binary.is_file(), "missing aelys-cli at {binary:?}");
            binary
        })
        .clone()
}

fn run_stdout(source: &str) -> String {
    let mut file = tempfile::Builder::new()
        .suffix(".aelys")
        .tempfile()
        .expect("temporary source file");
    file.write_all(source.as_bytes()).expect("write source");
    file.flush().expect("flush source");
    run_cli(file.path())
}

fn run_cli(path: &std::path::Path) -> String {
    let output = Command::new(cli_binary())
        .arg("run")
        .arg(path)
        .output()
        .expect("aelys-cli should run");
    assert!(
        output.status.success(),
        "aelys-cli exited with {:?}\nstdout: {}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).to_string()
}

#[test]
fn println_dispatches_to_the_user_display_impl() {
    let stdout = run_stdout(&format!(
        r#"{TYPES}
fn main() -> int {{
    println(Loud {{ v: 1 }})
    0
}}
main()
"#
    ));
    assert_eq!(stdout.lines().next(), Some("loud"), "stdout was {stdout:?}");
}

#[test]
fn tostring_dispatches_to_the_user_display_impl() {
    let text = run_string(&format!(
        r#"{TYPES}
fn probe() -> string {{
    __tostring(Loud {{ v: 1 }})
}}
probe()
"#
    ));
    assert_eq!(text, "loud");
}

#[test]
fn convert_to_string_dispatches_to_the_user_display_impl() {
    let text = run_string(&format!(
        r#"{TYPES}
fn probe() -> string {{
    convert::to_string(Loud {{ v: 1 }})
}}
probe()
"#
    ));
    assert_eq!(text, "loud");
}

#[test]
fn each_display_impl_dispatches_to_its_own_type() {
    let text = run_string(&format!(
        r#"{TYPES}
fn probe() -> string {{
    let a = Loud {{ v: 1 }}
    let b = Quiet {{ v: 2 }}
    __tostring(a) + "|" + __tostring(b) + "|" + __tostring(a)
}}
probe()
"#
    ));
    assert_eq!(text, "loud|quiet|loud");
}

#[test]
fn display_scalars_keep_the_compiler_rule_rendering() {
    let text = run_string(
        r#"
needs std::convert
fn probe() -> string {
    __tostring(42) + "|" + __tostring(true) + "|" + convert::to_string("plain")
}
probe()
"#,
    );
    assert_eq!(text, "42|true|plain");
}

#[test]
fn println_of_a_scalar_is_unchanged() {
    let stdout = run_stdout(
        r#"
needs std::io
fn main() -> int {
    println(42)
    0
}
main()
"#,
    );
    assert_eq!(stdout.lines().next(), Some("42"), "stdout was {stdout:?}");
}

#[test]
fn display_dispatch_survives_a_generic_call() {
    let text = run_string(&format!(
        r#"{TYPES}
fn render<T>(value: T) -> string {{
    __tostring(value)
}}
fn probe() -> string {{
    render(Loud {{ v: 1 }}) + "|" + render(Quiet {{ v: 2 }})
}}
probe()
"#
    ));
    assert_eq!(text, "loud|quiet");
}

#[test]
fn format_string_placeholder_dispatches_to_the_user_display_impl() {
    let stdout = run_stdout(&format!(
        r#"{TYPES}
fn main() -> int {{
    println("value = {{}}", Loud {{ v: 1 }})
    0
}}
main()
"#
    ));
    assert_eq!(
        stdout.lines().next(),
        Some("value = loud"),
        "stdout was {stdout:?}"
    );
}

#[test]
fn display_dispatch_reaches_a_generic_impl_method_body() {
    let text = run_string(&format!(
        r#"{TYPES}
struct Wrap<T> {{ inner: T }}
impl<T> Display for Wrap<T> {{
    fn to_display(self) -> string {{ "[" + __tostring(self.inner) + "]" }}
}}
fn probe() -> string {{
    __tostring(Wrap {{ inner: Loud {{ v: 1 }} }}) + __tostring(Wrap {{ inner: 5 }})
}}
probe()
"#
    ));
    assert_eq!(text, "[loud][5]");
}

#[test]
fn generic_display_use_requires_a_display_bound() {
    let mut vm = aelys::new_vm().expect("vm");
    let error = aelys::run_with_vm_and_opt(
        &mut vm,
        "struct Silent { value: int }\nfn render<T>(value: T) -> string { __tostring(value) }\nrender(Silent { value: 1 })",
        "<display-bound>",
        OptimizationLevel::Standard,
    )
    .expect_err("a generic display call must reject a type without Display");
    let message = error.to_string();
    assert!(message.contains("Display"), "{message}");
}

#[test]
fn an_imported_display_impl_still_dispatches() {
    let directory = tempfile::tempdir().expect("temporary module directory");
    std::fs::write(
        directory.path().join("shout.aelys"),
        "pub struct Loud { pub v: int }\n\nimpl Display for Loud {\n    fn to_display(self) -> string { \"loud\" }\n}\n",
    )
    .expect("write module");
    let main = directory.path().join("main.aelys");
    std::fs::write(
        &main,
        "needs std::io\nneeds shout\n\nfn main() -> int {\n    println(Loud { v: 1 })\n    0\n}\nmain()\n",
    )
    .expect("write main");
    let stdout = run_cli(&main);
    assert_eq!(stdout.lines().next(), Some("loud"), "stdout was {stdout:?}");
}

#[test]
fn format_string_interpolation_dispatches_to_the_user_display_impl() {
    let text = run_string(&format!(
        r#"{TYPES}
fn probe() -> string {{
    let a = Loud {{ v: 1 }}
    "value = {{a}}"
}}
probe()
"#
    ));
    assert_eq!(text, "value = loud");
}
