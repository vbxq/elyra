use aelys_opt::OptimizationLevel;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

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

struct CliRun {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

impl CliRun {
    fn report(&self, label: &str) -> String {
        format!(
            "{label}: exit {:?}\nstdout: {}\nstderr: {}",
            self.code, self.stdout, self.stderr
        )
    }
}

fn cli(arguments: &[&str], directory: &Path) -> CliRun {
    let output = Command::new(cli_binary())
        .args(arguments)
        .current_dir(directory)
        .output()
        .expect("aelys-cli should run");
    CliRun {
        code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    }
}

fn write_program(directory: &Path, name: &str, source: &str) -> PathBuf {
    let path = directory.join(name);
    std::fs::write(&path, source).expect("write program");
    path
}

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

fn rejection(source: &str) -> String {
    let mut vm = aelys::new_vm().expect("vm");
    match aelys::run_with_vm_and_opt(&mut vm, source, "<test>", OptimizationLevel::Standard) {
        Ok(value) => panic!("the source must be rejected, it produced {value:?}"),
        Err(error) => error.to_string(),
    }
}

const PROGRAM_WITH_PRINTLN: &str = r#"fn main() -> int {
    println("hello")
    0
}
main()
"#;

#[test]
fn d1_both_front_doors_accept_a_println_program() {
    let directory = tempfile::tempdir().expect("temporary directory");
    write_program(directory.path(), "door.aelys", PROGRAM_WITH_PRINTLN);

    let ran = cli(&["run", "door.aelys"], directory.path());
    assert_eq!(ran.code, Some(0), "{}", ran.report("run"));
    assert_eq!(
        ran.stdout.lines().next(),
        Some("hello"),
        "{}",
        ran.report("run")
    );

    let compiled = cli(&["compile", "door.aelys"], directory.path());
    assert_eq!(compiled.code, Some(0), "{}", compiled.report("compile"));
    assert!(
        directory.path().join("door.avbc").is_file(),
        "{}",
        compiled.report("compile")
    );
}

#[test]
fn d1_the_compiled_artifact_still_runs() {
    let directory = tempfile::tempdir().expect("temporary directory");
    write_program(directory.path(), "door.aelys", PROGRAM_WITH_PRINTLN);

    let compiled = cli(&["compile", "door.aelys"], directory.path());
    assert_eq!(compiled.code, Some(0), "{}", compiled.report("compile"));

    let executed = cli(&["run", "door.avbc"], directory.path());
    assert_eq!(executed.code, Some(0), "{}", executed.report("run avbc"));
    assert_eq!(
        executed.stdout.lines().next(),
        Some("hello"),
        "{}",
        executed.report("run avbc")
    );
}

#[test]
fn d1_both_front_doors_reject_a_genuinely_undefined_name() {
    let directory = tempfile::tempdir().expect("temporary directory");
    write_program(
        directory.path(),
        "ghost.aelys",
        "fn main() -> int {\n    no_such_function(1)\n    0\n}\nmain()\n",
    );

    let ran = cli(&["run", "ghost.aelys"], directory.path());
    assert_eq!(ran.code, Some(1), "{}", ran.report("run"));
    assert!(
        ran.stderr.contains("no_such_function"),
        "{}",
        ran.report("run")
    );

    let compiled = cli(&["compile", "ghost.aelys"], directory.path());
    assert_eq!(compiled.code, Some(1), "{}", compiled.report("compile"));
    assert!(
        compiled.stderr.contains("no_such_function"),
        "{}",
        compiled.report("compile")
    );
    assert_eq!(
        diagnostic_code(&ran.stderr),
        diagnostic_code(&compiled.stderr),
        "the two doors reported different codes\n{}\n{}",
        ran.report("run"),
        compiled.report("compile")
    );
}

fn diagnostic_code(text: &str) -> Option<String> {
    let start = text.find("error[")? + "error[".len();
    let rest = &text[start..];
    let end = rest.find(']')?;
    Some(rest[..end].to_string())
}

const PLAIN_STRUCT: &str = r#"
needs std::io
needs std::convert

struct Plain { n: int }
"#;

const DISPLAY_STRUCT: &str = r#"
needs std::io
needs std::convert

struct Shown { n: int }
impl Display for Shown {
    fn to_display(self) -> string { "shown" }
}
"#;

#[test]
fn d2_a_struct_without_display_is_rejected_by_the_direct_call_paths() {
    for body in ["println(p)", "convert::to_string(p)", "__tostring(p)"] {
        let message = rejection(&format!(
            "{PLAIN_STRUCT}
fn probe() -> int {{
    let p = Plain {{ n: 1 }}
    {body}
    0
}}
probe()
"
        ));
        assert!(
            message.contains("E0338") && message.contains("Plain"),
            "{body} was accepted or misreported: {message}"
        );
    }
}

#[test]
fn d2_a_struct_without_display_is_rejected_by_the_format_string_paths() {
    for body in ["println(\"{p}\")", "print(\"{}\", p)"] {
        let message = rejection(&format!(
            "{PLAIN_STRUCT}
fn probe() -> int {{
    let p = Plain {{ n: 1 }}
    {body}
    0
}}
probe()
"
        ));
        assert!(
            message.contains("E0338") && message.contains("Plain"),
            "{body} was accepted or misreported: {message}"
        );
    }
}

#[test]
fn d2_a_struct_without_display_is_rejected_by_a_bare_interpolation() {
    let message = rejection(&format!(
        "{PLAIN_STRUCT}
fn probe() -> string {{
    let p = Plain {{ n: 1 }}
    \"value = {{p}}\"
}}
probe()
"
    ));
    assert!(
        message.contains("E0338") && message.contains("Plain"),
        "a bare interpolation was accepted or misreported: {message}"
    );
}

#[test]
fn d2_a_struct_with_display_still_dispatches_through_the_string_paths() {
    for body in [
        "convert::to_string(s)",
        "__tostring(s)",
        "\"{s}\"",
        "\"a{s}b{s}\"",
    ] {
        let text = run_string(&format!(
            "{DISPLAY_STRUCT}
fn probe() -> string {{
    let s = Shown {{ n: 1 }}
    {body}
}}
probe()
"
        ));
        assert!(
            text.contains("shown"),
            "{body} rendered {text:?} instead of dispatching"
        );
    }
}

#[test]
fn d2_a_struct_with_display_still_dispatches_through_the_printing_paths() {
    let directory = tempfile::tempdir().expect("temporary directory");
    for (index, body) in ["println(s)", "println(\"{s}\")", "print(\"{}\", s)"]
        .into_iter()
        .enumerate()
    {
        let name = format!("shown{index}.aelys");
        write_program(
            directory.path(),
            &name,
            &format!(
                "{DISPLAY_STRUCT}
fn main() -> int {{
    let s = Shown {{ n: 1 }}
    {body}
    0
}}
main()
"
            ),
        );
        let ran = cli(&["run", &name], directory.path());
        assert_eq!(ran.code, Some(0), "{}", ran.report(body));
        assert!(ran.stdout.starts_with("shown"), "{}", ran.report(body));
    }
}

const FROM_IMPL: &str = r#"
struct Source { n: int }
struct Target { m: int }

impl From<Source> for Target {
    fn from(source: Source) -> Target { Target { m: source.n * 2 } }
}
"#;

#[test]
fn d3_from_can_be_called_by_name() {
    let mut vm = aelys::new_vm().expect("vm");
    aelys::run_with_vm(
        &mut vm,
        &format!(
            "{FROM_IMPL}
fn converted() -> int {{
    let target = Target::from(Source {{ n: 4 }})
    target.m
}}
"
        ),
        "from-by-name",
    )
    .expect("calling From::from by name should compile");

    assert_eq!(
        aelys::call_function(&mut vm, "converted", &[]).expect("converted"),
        aelys_runtime::Value::int(8)
    );
}

#[test]
fn d3_from_by_name_and_the_question_mark_reach_the_same_impl() {
    let mut vm = aelys::new_vm().expect("vm");
    aelys::run_with_vm(
        &mut vm,
        &format!(
            "{FROM_IMPL}
fn failing() -> Result<int, Source> {{ Err(Source {{ n: 4 }}) }}

fn through_try() -> Result<int, Target> {{
    let value = failing()?
    Ok(value)
}}

fn try_code() -> int {{
    match through_try() {{
        Ok(_) => 0,
        Err(problem) => problem.m,
    }}
}}

fn named_code() -> int {{
    Target::from(Source {{ n: 4 }}).m
}}
"
        ),
        "from-both-ways",
    )
    .expect("both spellings should compile together");

    let by_try = aelys::call_function(&mut vm, "try_code", &[]).expect("try_code");
    let by_name = aelys::call_function(&mut vm, "named_code", &[]).expect("named_code");
    assert_eq!(by_try, aelys_runtime::Value::int(8));
    assert_eq!(by_name, aelys_runtime::Value::int(8));
}

#[test]
fn d3_needs_from_still_parses() {
    let directory = tempfile::tempdir().expect("temporary directory");
    write_program(
        directory.path(),
        "helper.aelys",
        "pub fn doubled(n: int) -> int { n * 2 }\n",
    );
    write_program(
        directory.path(),
        "main.aelys",
        "needs doubled from helper\n\nfn main() -> int {\n    doubled(21)\n}\nmain()\n",
    );

    let ran = cli(&["run", "main.aelys"], directory.path());
    assert_eq!(ran.code, Some(0), "{}", ran.report("needs from"));
    assert_eq!(
        ran.stdout.lines().next(),
        Some("42"),
        "{}",
        ran.report("needs from")
    );
}
