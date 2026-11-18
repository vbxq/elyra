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

const LIB: &str = "pub struct S { pub n: int }\npub fn touch(mut s: S) -> int {\n    s.n = s.n + 1\n    s.n\n}\npub fn look(s: S) -> int { s.n }\npub let mut handler = touch\npub fn apply(f: fn(mut S) -> int, mut s: S) -> int { f(s) }\npub fn peek(f: fn(S) -> int, s: S) -> int { f(s) }\n";

#[test]
fn an_imported_mut_function_runs_through_function_values_on_a_mutable_root() {
    let dir = tree(&[("lib.aelys", LIB)]);
    assert_eq!(
        run_int(
            &dir,
            "main.aelys",
            "needs lib\nlet mut a = S { n: 1 }\nlet h = touch\nlet q = lib::touch\nlet fs = vec![touch]\nh(a) * 10000 + q(a) * 1000 + fs[0](a) * 100 + apply(touch, a) * 10 + lib::apply(lib::touch, a) - look(a)\n",
        ),
        2 * 10000 + 3 * 1000 + 4 * 100 + 5 * 10 + 6 - 6
    );
}

#[test]
fn an_imported_mut_function_value_rejects_an_immutable_root() {
    let dir = tree(&[("lib.aelys", LIB)]);
    for body in [
        "let h = touch\nh(a)",
        "let h = lib::touch\nh(a)",
        "touch(a)",
        "lib::touch(a)",
        "handler(a)",
        "let fs = vec![touch]\nfs[0](a)",
        "apply(touch, a)",
        "let g = fn(f: fn(mut S) -> int) -> int { f(a) }\ng(touch)",
    ] {
        let error = run_err(
            &dir,
            "bad.aelys",
            &format!("needs lib\nlet a = S {{ n: 1 }}\n{body}\n0\n"),
        );
        assert!(error.contains("E0323"), "{body}: {error}");
    }
}

#[test]
fn an_imported_function_type_keeps_mut_apart_from_plain() {
    let dir = tree(&[("lib.aelys", LIB)]);
    for body in ["let f: fn(S) -> int = touch", "peek(touch, S { n: 1 })"] {
        let error = run_err(&dir, "bad.aelys", &format!("needs lib\n{body}\n0\n"));
        assert!(error.contains("E0301"), "{body}: {error}");
    }
    assert_eq!(
        run_int(
            &dir,
            "plain.aelys",
            "needs lib\nlet a = S { n: 7 }\nlet h = look\nh(a) + peek(look, a)\n"
        ),
        14
    );
}

#[test]
fn a_carried_impl_body_calls_its_module_functions_through_the_typed_rule() {
    let dir = tree(&[
        (
            "m.aelys",
            "struct Cell<T> { v: T }\nfn unwrap(c: Cell<int>) -> int { c.v }\nfn bump(mut c: Cell<int>) -> int {\n    c.v = c.v + 1\n    c.v\n}\npub struct S { pub n: int }\nimpl S {\n    fn g(self) -> int {\n        let mut c = Cell { v: self.n }\n        unwrap(c) + bump(c)\n    }\n}\npub fn viag(s: S) -> int { s.g() }\n",
        ),
        (
            "m2.aelys",
            "struct Cell { v: int }\nfn touch(mut c: Cell) { c.v = 9 }\npub struct S { pub n: int }\nimpl S {\n    fn g(self) -> int { let c = Cell { v: self.n }; touch(c); c.v }\n}\n",
        ),
    ]);
    assert_eq!(
        run_int(
            &dir,
            "main.aelys",
            "needs m\nlet s = S { n: 10 }\nviag(s) * 100 + s.g()\n"
        ),
        2121
    );
    let error = run_err(&dir, "main2.aelys", "needs m2\nS { n: 1 }.g()\n");
    assert!(error.contains("E0323"), "{error}");
}
