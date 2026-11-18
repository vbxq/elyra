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

const CNT: &str = "pub struct P { pub x: int }\npub struct H { pub p: P }\npub let v = vec![1, 2]\npub let h = H { p: P { x: 1 } }\npub let m: Map<int, int> = Map::new()\npub let mut mv = vec![1]\npub let mut mm: Map<string, int> = Map::new()\npub let mut ms = P { x: 0 }\npub let mut names = vec![\"a\"]\npub fn total() -> int { mv.len() * 1000 + mm.len() * 100 + ms.x * 10 + names[0].len() }\n";

#[test]
fn an_unwrap_failure_names_the_enum_variant_after_a_module_shifted_the_enum_ids() {
    let dir = tree(&[(
        "em.aelys",
        "pub enum M { Mx, My }\npub fn m0() -> M { M::Mx }\n",
    )]);
    let error = run_err(
        &dir,
        "main.aelys",
        "use em\nenum Apple { Red(int), Green(int) }\nenum Bird { Crow(int), Owl(int) }\nfn f() -> Result<int, Apple> { Err(Apple::Green(5)) }\nlet r = f()\nr.unwrap()\n",
    );
    assert!(error.contains("Apple::Green(5)"), "{error}");
    assert!(!error.contains("Bird"), "{error}");
}

#[test]
fn collection_natives_and_field_writes_reach_an_imported_mutable_variable() {
    let dir = tree(&[("cnt.aelys", CNT)]);
    assert_eq!(
        run_int(
            &dir,
            "main.aelys",
            "use cnt\nmv.push(5)\nmv.insert(0, 7)\nlet _ = mm.insert(\"k\", 1)\nms.x = 3\nms.x += 1\nnames[0].push('z')\nlet f = fn() { mv.push(9) }\nf()\ncnt::total()\n",
        ),
        4142
    );
}

#[test]
fn a_mutable_alias_of_an_imported_immutable_aggregate_is_e0323() {
    let dir = tree(&[("cnt.aelys", CNT)]);
    for body in [
        "let mut w = cnt::v",
        "let mut w = v",
        "let mut w = cnt::h.p",
        "let mut w = (cnt::h).p",
        "let mut q = cnt::m",
        "let mut q: Map<int, int> = Map::new()\nq = cnt::m",
        "fn grow(mut x: Vec<int>) { x.push(1) }\ngrow(cnt::v)",
    ] {
        let error = run_err(&dir, "bad.aelys", &format!("use cnt\n{body}\n0\n"));
        assert!(error.contains("E0323"), "{body}: {error}");
    }
}

#[test]
fn mutating_an_imported_immutable_collection_is_rejected() {
    let dir = tree(&[("cnt.aelys", CNT)]);
    for body in ["v.push(3)", "let _ = m.insert(1, 1)", "v[0] = 4"] {
        let error = run_err(&dir, "bad.aelys", &format!("use cnt\n{body}\n0\n"));
        assert!(error.contains("E0318"), "{body}: {error}");
    }
}

#[test]
fn a_carried_impl_body_calls_its_module_functions_and_keeps_their_mut_rule() {
    let dir = tree(&[
        (
            "m.aelys",
            "struct Cell<T> { v: T }\nfn unwrap(c: Cell<int>) -> int { c.v }\npub struct S { pub n: int }\nimpl S {\n    fn g(self) -> int { let c = Cell { v: self.n }; unwrap(c) + 2 }\n}\npub fn viag(s: S) -> int { s.g() }\n",
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
            "use m\nlet s = S { n: 10 }\nviag(s) * 100 + s.g()\n"
        ),
        1212
    );
    let error = run_err(&dir, "main2.aelys", "use m2\nS { n: 1 }.g()\n");
    assert!(error.contains("E0323"), "{error}");
}

#[test]
fn a_constant_two_whole_modules_export_is_ambiguous_bare_and_fine_qualified() {
    let dir = tree(&[
        ("ca.aelys", "pub const N = 3\n"),
        ("cb.aelys", "pub const N = 4\n"),
    ]);
    assert_eq!(
        run_int(
            &dir,
            "ok.aelys",
            "use ca\nuse cb\nlet a = [0; ca::N]\nlet b = [0; cb::N]\na.len() * 10 + b.len()\n"
        ),
        34
    );
    let error = run_err(&dir, "bad.aelys", "use ca\nuse cb\nN\n");
    assert!(error.contains("E0445"), "{error}");
}
