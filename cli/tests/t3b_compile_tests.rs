use aelys_cli::cli::commands::asm::asm_transform;
use aelys_cli::cli::commands::compile::compile_to_avbc;
use aelys_opt::OptimizationLevel;
use std::path::{Path, PathBuf};

const PROGRAM: &str = r#"let mut v = vec![3, 1, 2]
v.sort()
v.insert(0, 9)
let mut m: Map<int, int> = Map::new()
m[1] = v[1]
let c = 65 as char
let mut s = "x"
s.push(c)
s.push('z')
println("{m[1]} {c} {s} {c.is_alpha()} {v.len()} {m.contains_key(1)} {c as int}")
"#;

const EXPECTED: &str = "1 A xAz true 4 true 65";

const ASM_PROGRAM: &str = r#"let mut v = vec![3, 1, 2]
v.sort()
v.insert(0, 9)
v.swap(0, 3)
let mut m: Map<int, int> = Map::new()
m[1] = v.first().unwrap()
let c = 65 as char
let mut s = "x"
s.push(c)
s.push('z')
println("{m[1]} {c} {s} {c.is_alpha()} {v.last().unwrap()} {m.contains_key(1)} {c as int}")
"#;

const ASM_EXPECTED: &str = "3 A xAz true 9 true 65";

fn fresh_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aelys_t3b_compile_{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run_cli(target: &Path) -> (String, String, Option<i32>) {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_aelys-cli"))
        .arg("run")
        .arg(target)
        .output()
        .unwrap();
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.code(),
    )
}

fn assert_prints(target: &Path, expected: &str) {
    let (stdout, stderr, code) = run_cli(target);
    assert_eq!(
        (code, stdout.lines().next()),
        (Some(0), Some(expected)),
        "running {}; stderr:\n{stderr}",
        target.display()
    );
}

#[test]
fn collection_natives_compile_to_bytecode_that_runs() {
    for (name, level) in [
        ("o0", OptimizationLevel::None),
        ("o2", OptimizationLevel::Standard),
    ] {
        let dir = fresh_dir(name);
        let src = dir.join("collections.aelys");
        std::fs::write(&src, PROGRAM).unwrap();
        assert_prints(&src, EXPECTED);
        let avbc = compile_to_avbc(&src, level).expect("compile accepts the collection natives");
        assert_prints(&avbc, EXPECTED);
    }
}

#[test]
fn collection_natives_survive_an_assembly_round_trip() {
    let dir = fresh_dir("aasm");
    let src = dir.join("collections.aelys");
    std::fs::write(&src, ASM_PROGRAM).unwrap();
    let aasm = asm_transform(&src).expect("asm accepts the collection natives");
    let first_text = std::fs::read_to_string(&aasm).unwrap();
    assert_prints(&aasm, ASM_EXPECTED);
    let avbc = compile_to_avbc(&aasm, OptimizationLevel::Standard)
        .expect("the assembly assembles back to bytecode");
    assert_prints(&avbc, ASM_EXPECTED);
    let again = asm_transform(&avbc).expect("the bytecode disassembles again");
    assert_eq!(first_text, std::fs::read_to_string(&again).unwrap());
}
