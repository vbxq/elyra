mod t9_support;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use t9_support::{Outcome, cli_binary, run_with};

const MODES: [&[&str]; 5] = [
    &["-O0"],
    &["-O3"],
    &["-ae.jit=on"],
    &["-ae.jit-sync=on"],
    &["-ae.gc-stress=on", "-ae.gc-verify=on"],
];

const LIBRARY_A: &str = r#"
#include <stdarg.h>
#include <stdint.h>
int which(void) { return 1; }
int64_t widen(int32_t x) { return x; }
int take_sc_as_int(signed char x) { return x; }
int64_t vsum(int n, ...) {
    va_list args;
    va_start(args, n);
    int64_t total = 0;
    for (int i = 0; i < n; i++) total += va_arg(args, long long);
    va_end(args);
    return total;
}
"#;

const LIBRARY_B: &str = r#"
#include <stdarg.h>
#include <stdint.h>
int which(void) { return 2; }
int64_t widen(int8_t x) { return x; }
int64_t vsum(int n, ...) {
    va_list args;
    va_start(args, n);
    int64_t total = 0;
    for (int i = 0; i < n; i++) total += 10 * va_arg(args, long long);
    va_end(args);
    return total;
}
"#;

struct Libraries {
    _dir: tempfile::TempDir,
    a: PathBuf,
    b: PathBuf,
}

// without a C compiler every test that needs the libraries says so and passes
fn libraries() -> Option<&'static Libraries> {
    static LIBRARIES: OnceLock<Option<Libraries>> = OnceLock::new();
    LIBRARIES
        .get_or_init(|| {
            let dir = tempfile::tempdir().expect("temporary directory");
            let compiler = std::env::var("CC").unwrap_or_else(|_| "cc".to_string());
            let mut built = Vec::new();
            for (name, source) in [("fixa", LIBRARY_A), ("fixb", LIBRARY_B)] {
                let c_file = dir.path().join(format!("{name}.c"));
                std::fs::write(&c_file, source).expect("write C source");
                let library = dir.path().join(library_file(name));
                match Command::new(&compiler)
                    .args(["-shared", "-fPIC", "-O2", "-o"])
                    .arg(&library)
                    .arg(&c_file)
                    .status()
                {
                    Ok(status) if status.success() => built.push(library),
                    Ok(status) => panic!("{compiler} failed to build {name}: {status}"),
                    Err(error) => {
                        eprintln!("SKIPPED: no C compiler ({compiler}: {error})");
                        return None;
                    }
                }
            }
            let b = built.pop().expect("library b");
            let a = built.pop().expect("library a");
            Some(Libraries { _dir: dir, a, b })
        })
        .as_ref()
}

fn library_file(name: &str) -> String {
    format!(
        "{}{name}{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    )
}

fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temporary directory");
    for (name, text) in files {
        let path = dir.path().join(name);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("directories");
        std::fs::write(path, text).expect("write file");
    }
    dir
}

fn install(library: &Path, dir: &Path, name: &str) {
    std::fs::create_dir_all(dir).expect("directories");
    std::fs::copy(library, dir.join(library_file(name))).expect("copy library");
}

fn run_script(script: &Path, flags: &[&str]) -> Outcome {
    let output = Command::new(cli_binary())
        .arg("run")
        .args(flags)
        .arg(script)
        .output()
        .expect("aelys-cli should run");
    Outcome {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        code: output.status.code(),
    }
}

fn assert_script(script: &Path, expected: &str) {
    let expected: Vec<&str> = expected.lines().collect();
    for flags in MODES {
        let outcome = run_script(script, flags);
        assert_eq!(
            outcome.code,
            Some(0),
            "{script:?} flags {flags:?}\nstdout: {}\nstderr: {}",
            outcome.stdout,
            outcome.stderr
        );
        assert_eq!(
            outcome.lines(),
            expected,
            "{script:?} flags {flags:?}\nstderr: {}",
            outcome.stderr
        );
    }
}

fn assert_lines(source: &str, expected: &str) {
    let expected: Vec<&str> = expected.lines().collect();
    for flags in MODES {
        let outcome = run_with(source, flags);
        assert_eq!(
            outcome.code,
            Some(0),
            "flags {flags:?}\nstdout: {}\nstderr: {}",
            outcome.stdout,
            outcome.stderr
        );
        assert_eq!(
            outcome.lines(),
            expected,
            "flags {flags:?}\nstderr: {}",
            outcome.stderr
        );
    }
}

fn assert_refused(source: &str, code: &str, message: &str) {
    let outcome = run_with(source, &[]);
    assert_ne!(outcome.code, Some(0), "{source}");
    assert!(
        outcome.stderr.contains(code) && outcome.stderr.contains(message),
        "{source}\nwanted: {code} {message}\nstderr: {}",
        outcome.stderr
    );
}

#[test]
fn a_cstr_result_pointing_into_its_argument_is_read_before_the_argument_is_freed() {
    let source = r#"extern "c" {
    fn strchr(s: cstr, c: i32) -> cstr
    fn strstr(hay: cstr, needle: cstr) -> cstr
    fn strrchr(s: cstr, c: i32) -> cstr
}
let mut s = "x"
let mut k = 0
while k < 18 { s = s + s; k += 1 }
println(strchr(s + "!tail", 33))
println(strstr(s + "needle-end", "needle").len())
println(strrchr("a/b/" + s + "/leaf", 47))
fn pad(n: int) -> string {
    let mut t = ""
    let mut j = 0
    while j < n { t = t + "ab"; j += 1 }
    t
}
let mut bad = 0
let mut i = 0
while i < 200 {
    let found = strstr(pad(i) + "needle-" + pad(10) + "-end", "needle")
    if found != "needle-" + pad(10) + "-end" { bad += 1 }
    let tail = strchr("hello world " + pad(i), 119)
    if tail != "world " + pad(i) { bad += 1 }
    i += 1
}
println(bad)
"#;
    assert_lines(source, "!tail\n10\n/leaf\n0");
}

#[test]
fn two_modules_declaring_one_name_from_two_libraries_each_call_their_own() {
    let Some(libraries) = libraries() else { return };
    let dir = project(&[
        (
            "ma.aelys",
            "extern \"fixa\" {\n    fn which() -> i32\n    fn widen(x: i32) -> i64\n}\npub fn w() -> i32 { which() }\npub fn wide(x: i32) -> i64 { widen(x) }\n",
        ),
        (
            "mb.aelys",
            "extern \"fixb\" {\n    fn which() -> i32\n    fn widen(x: i8) -> i64\n}\npub fn w() -> i32 { which() }\npub fn wide(x: i8) -> i64 { widen(x) }\n",
        ),
        (
            "main.aelys",
            "use ma\nuse mb\nprintln(ma::w())\nprintln(mb::w())\nprintln(ma::wide(100000))\nprintln(mb::wide(-5))\n",
        ),
        (
            "same.aelys",
            "use ma\nextern \"fixb\" { fn which() -> i32 }\nprintln(ma::w())\nprintln(which())\n",
        ),
    ]);
    install(&libraries.a, dir.path(), "fixa");
    install(&libraries.b, dir.path(), "fixb");
    assert_script(&dir.path().join("main.aelys"), "1\n2\n100000\n-5");
    assert_script(&dir.path().join("same.aelys"), "1\n2");
}

#[test]
fn a_function_or_global_of_the_main_script_never_replaces_a_module_extern() {
    let dir = project(&[
        (
            "mc.aelys",
            "extern \"c\" { fn abs(x: i32) -> i32 }\npub fn mag(x: i32) -> i32 { abs(x) }\n",
        ),
        ("md.aelys", "pub fn abs(x: int) -> int { 2000 + x }\n"),
        (
            "fn_main.aelys",
            "use mc\nfn abs(x: int) -> int { 1000 + x }\nprintln(mc::mag(-5))\nprintln(abs(-5))\n",
        ),
        (
            "let_main.aelys",
            "use mc\nlet abs = 7\nprintln(mc::mag(-5))\nprintln(abs)\n",
        ),
        (
            "extern_main.aelys",
            "use md\nextern \"c\" { fn abs(x: i32) -> i32 }\nprintln(md::abs(-5))\nprintln(abs(-5))\n",
        ),
    ]);
    assert_script(&dir.path().join("fn_main.aelys"), "5\n995");
    assert_script(&dir.path().join("let_main.aelys"), "5\n7");
    assert_script(&dir.path().join("extern_main.aelys"), "1995\n5");
}

#[test]
fn a_main_extern_of_another_signature_never_replaces_a_module_extern() {
    let home = std::env::var("HOME").expect("HOME is set for the test process");
    let dir = project(&[
        (
            "libc_mod.aelys",
            "extern \"c\" { fn strlen(s: cstr) -> u64 }\npub fn size(s: string) -> u64 { strlen(s) }\n",
        ),
        (
            "envmod.aelys",
            "extern \"c\" { fn getenv(n: cstr) -> cstr }\npub fn home() -> string { getenv(\"HOME\") }\npub fn home_len() -> int { getenv(\"HOME\").len() }\n",
        ),
        (
            "strlen_main.aelys",
            "use libc_mod\nextern \"c\" { fn strlen(p: ptr) -> u64 }\nprintln(libc_mod::size(\"hello\"))\nlet q = ptr::alloc(4)\nq.write_u8(0, 65)\nprintln(strlen(q))\n",
        ),
        (
            "env_main.aelys",
            "use envmod\nextern \"c\" { fn getenv(n: cstr) -> ptr }\nprintln(getenv(\"HOME\").is_null())\nprintln(envmod::home_len() > 0)\nlet h = envmod::home()\nprintln(h + \"/x\")\nprintln(h.len() == envmod::home_len())\n",
        ),
    ]);
    assert_script(&dir.path().join("strlen_main.aelys"), "5\n1");
    assert_script(
        &dir.path().join("env_main.aelys"),
        &format!("false\ntrue\n{home}/x\ntrue"),
    );
}

#[test]
fn variadic_values_closures_and_methods_of_a_module_keep_its_externs() {
    let Some(libraries) = libraries() else { return };
    let dir = project(&[
        (
            "mv.aelys",
            r#"extern "fixa" {
    fn which() -> i32
    fn vsum(n: i32, ...) -> i64
}
struct Box { k: int }
impl Box {
    fn get(self) -> i32 { which() + self.k as i32 }
}
pub fn value() -> fn() -> i32 { which }
pub fn boxed() -> i32 { Box { k: 10 }.get() }
pub fn closure() -> i32 {
    let c = fn(x: i32) -> i32 { which() + x }
    c(20)
}
pub fn var() -> i64 { vsum(2, 1, 1) }
pub fn hot() -> i64 {
    let mut i = 0
    let mut s: i64 = 0
    while i < 20000 { s += which() as i64; i += 1 }
    s
}
"#,
        ),
        (
            "main.aelys",
            r#"use mv
fn which() -> i32 { 7 }
extern "fixb" { fn vsum(n: i32, ...) -> i64 }
println(mv::value()())
println(mv::boxed())
println(mv::closure())
println(mv::var())
println(vsum(2, 1, 1))
println(mv::hot())
println(which())
"#,
        ),
    ]);
    install(&libraries.a, dir.path(), "fixa");
    install(&libraries.b, dir.path(), "fixb");
    assert_script(&dir.path().join("main.aelys"), "1\n11\n21\n2\n20\n20000\n7");
}

#[test]
fn a_generic_module_function_calls_its_own_module_extern() {
    let Some(libraries) = libraries() else { return };
    let dir = project(&[
        (
            "gm.aelys",
            r#"extern "fixa" {
    fn take_sc_as_int(x: i8) -> i32
    fn which() -> i32
    fn vsum(n: i32, ...) -> i64
}
pub fn tag<T>(x: T) -> i32 { take_sc_as_int(-7) }
pub fn total<T>(x: T) -> i64 { vsum(3, 1, 2, 3) }
pub fn w<T>(x: T) -> i32 { which() }
pub fn handle<T>(x: T) -> i32 {
    let f = which
    f() + 100
}
pub fn lam<T>(x: T) -> i32 {
    let g = fn() -> i32 { which() + 1000 }
    g()
}
"#,
        ),
        (
            "alone.aelys",
            "use gm\nprintln(gm::tag(5))\nprintln(gm::tag(\"s\"))\nprintln(gm::total(1.5))\n",
        ),
        (
            "clash.aelys",
            r#"use gm
extern "fixb" {
    fn which() -> i32
    fn vsum(n: i32, ...) -> i64
}
println(gm::w(1))
println(which())
println(gm::total(1))
println(vsum(3, 1, 2, 3))
println(gm::handle("h"))
println(gm::lam(2.5))
"#,
        ),
        (
            "mid.aelys",
            "use gm\npub fn go() -> i64 { gm::total(1) + gm::w(1) as i64 }\n",
        ),
        (
            "through.aelys",
            "use mid\nextern \"fixb\" { fn vsum(n: i32, ...) -> i64 }\nprintln(mid::go())\nprintln(vsum(1, 5))\n",
        ),
        (
            "mid2.aelys",
            "use gm\nextern \"fixb\" { fn which() -> i32 }\npub fn g2<T>(x: T) -> i64 { gm::total(x) + which() as i64 }\n",
        ),
        (
            "nested.aelys",
            "use mid2\nextern \"fixa\" { fn vsum(n: i32, ...) -> i64 }\nprintln(mid2::g2(1))\nprintln(mid2::g2(\"x\"))\nprintln(vsum(2, 2, 2))\n",
        ),
    ]);
    install(&libraries.a, dir.path(), "fixa");
    install(&libraries.b, dir.path(), "fixb");
    assert_script(&dir.path().join("alone.aelys"), "-7\n-7\n6");
    assert_script(&dir.path().join("clash.aelys"), "1\n2\n6\n60\n101\n1001");
    assert_script(&dir.path().join("through.aelys"), "7\n50");
    assert_script(&dir.path().join("nested.aelys"), "8\n8\n4");
}

#[test]
fn a_generic_function_carried_from_a_subdirectory_finds_the_library_beside_its_module() {
    let Some(libraries) = libraries() else { return };
    let dir = project(&[
        (
            "sub/sg.aelys",
            "extern \"fixsub\" {\n    fn vsum(n: i32, ...) -> i64\n    fn which() -> i32\n}\npub fn total<T>(x: T) -> i64 { vsum(2, 20, 22) }\npub fn w<T>(x: T) -> i32 { which() }\n",
        ),
        (
            "main.aelys",
            "use sub::sg\nprintln(sg::total(1))\nprintln(sg::w(1))\n",
        ),
    ]);
    install(&libraries.a, &dir.path().join("sub"), "fixsub");
    assert_script(&dir.path().join("main.aelys"), "42\n1");
}

#[test]
fn a_ptr_prints_its_address_in_hex_and_never_reads_memory() {
    let source = r#"let p = ptr::from_addr(4096)
println(p)
println("{p}")
println("{}", p)
println(p.to_string())
println(ptr::from_addr(18446744073709551615).to_string())
println(ptr::null())
fn show<T>(x: T) -> string { "{x}" }
fn conv<T: Display>(x: T) -> string { x.to_string() }
println(show(p))
println(conv(p))
struct Handle { p: ptr }
let h = Handle { p: p.offset(16) }
println("{h.p}")
let q = ptr::alloc(4)
q.write_u8(0, 104)
println(ptr::to_string(q))
println(q.to_string() == "{q}")
println(q.to_string() == ptr::from_addr(q.addr()).to_string())
ptr::free(q)
"#;
    assert_lines(
        source,
        "0x1000\n0x1000\n0x1000\n0x1000\n0xffffffffffffffff\n0x0\n0x1000\n0x1000\n0x1010\nh\ntrue\ntrue",
    );
    assert_refused(
        "let v = vec![ptr::null()]\nprintln(v)\n",
        "E0338",
        "not implemented for Vec<ptr>",
    );
    assert_refused(
        "let o = Some(ptr::null())\nprintln(\"{o}\")\n",
        "E0338",
        "not implemented for Option<ptr>",
    );
    assert_refused(
        "let o = Some(ptr::null())\nprintln(o.to_string())\n",
        "E0338",
        "not implemented for Option<ptr>",
    );
    assert_refused(
        "let p = ptr::null()\nprintln(p.to_string(1))\n",
        "E0359",
        "expected 0, found 1",
    );
    assert_refused("let p = ptr::null()\nprintln(p.len())\n", "E0", "ptr");
}

#[test]
fn a_ptr_is_a_map_key_compared_by_address() {
    let source = r#"let mut m: Map<ptr, int> = Map::new()
let a = ptr::from_addr(18446744073709551615)
let b = ptr::from_addr(4096)
let _ = m.insert(a, 1)
let _ = m.insert(b, 2)
let _ = m.insert(ptr::from_addr(18446744073709551615), 3)
println(m.len())
println(m.get(a).unwrap())
println(m.get(b.offset(0)).unwrap())
println(m.contains_key(ptr::null()))
let keys = m.keys()
println(keys[0] == a)
println(keys[1])
"#;
    assert_lines(source, "2\n3\n2\nfalse\ntrue\n0x1000");
    assert_refused(
        "let m: Map<ptr, int> = Map::new()\nprintln(m)\n",
        "E0338",
        "not implemented for Map<ptr, int>",
    );
}

#[test]
fn ffi_diagnostics_name_the_actual_mistake() {
    assert_refused(
        "extern \"c\" { fn sscanf(s: cstr, f: cstr, ...) -> i32 }\nlet mut out: Vec<u8> = vec![0; 8]\nlet n = sscanf(\"1234\", \"%d\", &mut out)\n",
        "E0494",
        "a borrow of a Vec<u8> cannot be passed in the `...` part of `sscanf`\n   = help: pass a ptr",
    );
    assert_refused(
        "extern \"c\" { fn memset(b: &mut [u8], c: i32, n: u64) -> ptr }\nlet v: Vec<u8> = vec![0]\nmemset(&mut v, 0, 1)\n",
        "E0415",
        "cannot borrow 'v' as mutable: the binding 'v' is immutable",
    );
    assert_refused(
        "extern \"c\" { fn memset(b: &mut [u8], c: i32, n: u64) -> ptr }\nfn g(v: Vec<u8>) { memset(&mut v, 0, 1) }\n",
        "E0415",
        "the binding 'v' is immutable",
    );
    for flags in MODES {
        let outcome = run_with(
            "extern \"aelys_no_such_library\" { fn open_it() }\nopen_it()\n",
            flags,
        );
        assert_eq!(outcome.code, Some(1), "{flags:?} {}", outcome.stderr);
        assert!(
            outcome.stderr.contains(
                "cannot load C library 'aelys_no_such_library' for C function 'open_it': tried"
            ),
            "{flags:?} {}",
            outcome.stderr
        );
    }
}
