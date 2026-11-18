use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

use aelys_driver::run_file;

fn tree(files: &[(&str, &str)]) -> TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    for (path, content) in files {
        write(dir.path(), path, content);
    }
    dir
}

fn write(root: &Path, path: &str, content: &str) -> PathBuf {
    let file = root.join(path);
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent).expect("parent dirs");
    }
    fs::write(&file, content).expect("write fixture");
    file
}

fn run_int(dir: &TempDir, main: &str, source: &str) -> i64 {
    let path = write(dir.path(), main, source);
    let value = run_file(&path).unwrap_or_else(|error| panic!("{main} must run:\n{error}"));
    value
        .as_int()
        .unwrap_or_else(|| panic!("{main} must end on an int, got {value:?}"))
}

fn run_err(dir: &TempDir, main: &str, source: &str) -> String {
    let path = write(dir.path(), main, source);
    run_file(&path)
        .map(|value| panic!("{main} must be refused, got {value:?}"))
        .unwrap_err()
        .to_string()
}

#[test]
fn a_module_private_function_named_like_a_builtin_never_replaces_the_builtin() {
    let dir = tree(&[(
        "geo.aelys",
        "fn abs(x: int) -> int { if x < 0 { return 0 - x }\n return x }\n\
         fn max(a: int, b: int) -> int { if a > b { return a }\n return b }\n\
         pub fn dist(a: int, b: int) -> int { abs(a - b) }\n\
         pub fn big(a: int, b: int) -> int { max(a, b) }\n",
    )]);
    assert_eq!(
        run_int(
            &dir,
            "main.aelys",
            "needs geo\n\
             let a = dist(3, 10)\n\
             let b = big(3, 10)\n\
             let c = if math::abs(-2.5) == 2.5 { 1 } else { 0 }\n\
             let d = if math::max(1.5, 2.5) == 2.5 { 1 } else { 0 }\n\
             let e = if abs(-4.5) == 4.5 { 1 } else { 0 }\n\
             a * 10000 + b * 100 + c * 10 + d + e * 1000\n"
        ),
        72_011,
    );
}

#[test]
fn a_builtin_named_function_defined_after_its_use_is_the_module_own() {
    let dir = tree(&[(
        "late.aelys",
        "pub fn dist(a: int, b: int) -> int { abs(a - b) }\n\
         pub fn via_closure(a: int) -> int { let f = fn(x: int) -> int { return abs(x) }\n f(a) }\n\
         pub struct W { pub n: int }\n\
         impl W { pub fn mag(self) -> int { abs(self.n) } }\n\
         fn abs(x: int) -> int { if x < 0 { return 1000 - x }\n return x }\n",
    )]);
    assert_eq!(
        run_int(
            &dir,
            "main.aelys",
            "needs late\n\
             let w = W { n: -4 }\n\
             let k = if math::abs(-2.5) == 2.5 { 1 } else { 0 }\n\
             dist(3, 10) * 1000000 + via_closure(-2) * 100 + w.mag() + k * 100000000\n"
        ),
        100_000_000 + 1007 * 1_000_000 + 1002 * 100 + 1004,
    );
}

#[test]
fn a_module_private_string_helper_never_replaces_the_string_builtin() {
    let dir = tree(&[(
        "mstr.aelys",
        "fn trim(s: string) -> string { \"T\" }\npub fn tt() -> string { trim(\" a \") }\n",
    )]);
    assert_eq!(
        run_int(
            &dir,
            "main.aelys",
            "needs mstr\n\
             let a = if tt() == \"T\" { 1 } else { 0 }\n\
             let b = if string::trim(\"  b  \") == \"b\" { 1 } else { 0 }\n\
             a * 10 + b\n"
        ),
        11,
    );
}

#[test]
fn an_entry_file_function_named_like_a_builtin_never_replaces_it_in_a_module() {
    let dir = tree(&[(
        "user.aelys",
        "pub fn lo() -> int { if math::min(1.5, 0.5) == 0.5 { 1 } else { 0 } }\n",
    )]);
    assert_eq!(
        run_int(
            &dir,
            "main.aelys",
            "needs user\nlet a = min(3, 4)\nfn min(a: int, b: int) -> int { 77 }\na * 10 + user::lo()\n"
        ),
        771,
    );
}

#[test]
fn a_user_module_named_like_a_builtin_module_is_the_one_called() {
    let dir = tree(&[
        (
            "math.aelys",
            "pub fn sqrt(x: float) -> float { 42.0 }\npub fn mine() -> int { 7 }\n",
        ),
        ("time.aelys", "pub fn now() -> int { 5 }\n"),
    ]);
    assert_eq!(
        run_int(
            &dir,
            "m1.aelys",
            "needs math\nlet s = if math::sqrt(16.0) == 42.0 { 1 } else { 0 }\nmath::mine() * 10 + s\n"
        ),
        71,
    );
    assert_eq!(run_int(&dir, "m7.aelys", "needs time\ntime::now()\n"), 5);
}

const CNT: &str = "pub let mut n = 0\npub fn get() -> int { n }\npub fn bump() { n = n + 1 }\n";

#[test]
fn assigning_an_imported_function_is_refused_in_every_position() {
    let dir = tree(&[
        ("cnt.aelys", CNT),
        (
            "hack.aelys",
            "needs cnt\npub fn hack() { get = fn() -> int { return 999 } }\n",
        ),
    ]);
    let sources = [
        (
            "top.aelys",
            "needs cnt\nget = fn() -> int { return 999 }\ncnt::get()\n",
        ),
        (
            "sel.aelys",
            "needs get from cnt\nget = fn() -> int { return 999 }\nget()\n",
        ),
        (
            "infn.aelys",
            "needs cnt\nfn f() { get = fn() -> int { return 9 } }\nf()\nget()\n",
        ),
        (
            "clos.aelys",
            "needs cnt\nlet h = fn() { bump = fn() { n = n + 100 } }\nh()\nget()\n",
        ),
        ("mod.aelys", "needs hack\nhack::hack()\n1\n"),
    ];
    for (file, source) in sources {
        let error = run_err(&dir, file, source);
        assert!(error.contains("E0446"), "{file}: {error}");
        assert!(error.contains("it is a function"), "{file}: {error}");
    }
}

#[test]
fn a_file_own_function_shadowing_an_import_keeps_the_local_refusal() {
    let dir = tree(&[("cnt.aelys", CNT)]);
    let error = run_err(
        &dir,
        "own.aelys",
        "needs cnt\nfn get() -> int { 4 }\nget = fn() -> int { return 9 }\nget()\n",
    );
    assert!(error.contains("E0203"), "{error}");
    assert_eq!(
        run_int(
            &dir,
            "read.aelys",
            "needs cnt\nfn get() -> int { 4 }\ncnt::bump()\nget() * 10 + cnt::get()\n"
        ),
        41,
    );
    assert_eq!(
        run_int(
            &dir,
            "param.aelys",
            "needs cnt\nfn f() -> int { let mut get = 1\n get = 3\n get }\nf()\n"
        ),
        3,
    );
}

#[test]
fn writing_an_ambiguous_bare_import_is_the_ambiguity_error() {
    let dir = tree(&[
        ("p.aelys", "pub let mut kk = 1\npub fn sz() -> int { 1 }\n"),
        ("q.aelys", "pub let mut kk = 2\npub fn sz() -> int { 2 }\n"),
    ]);
    let sources = [
        ("top.aelys", "needs p\nneeds q\nkk = 3\n1\n"),
        ("infn.aelys", "needs p\nneeds q\nfn f() { kk = 3 }\n1\n"),
        (
            "clos.aelys",
            "needs p\nneeds q\nlet c = fn() { kk = 3 }\n1\n",
        ),
        ("compound.aelys", "needs p\nneeds q\nkk += 1\n1\n"),
    ];
    for (file, source) in sources {
        let error = run_err(&dir, file, source);
        assert!(error.contains("E0445"), "{file}: {error}");
        assert!(error.contains("'p::kk' or 'q::kk'"), "{file}: {error}");
    }
    assert_eq!(
        run_int(
            &dir,
            "own.aelys",
            "needs p\nneeds q\nlet mut kk = 0\nkk = 3\nkk * 10 + p::kk\n"
        ),
        31,
    );
}

#[test]
fn a_selective_import_clash_never_suggests_an_alias() {
    let dir = tree(&[
        ("p.aelys", "pub fn sz() -> int { 1 }\n"),
        ("q.aelys", "pub fn sz() -> int { 2 }\n"),
        (
            "nm.aelys",
            "needs sz from p\nneeds sz from q\npub fn f() -> int { 1 }\n",
        ),
    ]);
    for (file, source) in [
        ("main.aelys", "needs sz from p\nneeds sz from q\n1\n"),
        ("nested.aelys", "needs nm\n1\n"),
        (
            "std.aelys",
            "needs size from std::bytes\nneeds size from std::fs\n1\n",
        ),
    ] {
        let error = run_err(&dir, file, source);
        assert!(error.contains("E0410"), "{file}: {error}");
        assert!(!error.contains("'as' alias"), "{file}: {error}");
        assert!(error.contains("from one module only"), "{file}: {error}");
    }
}
