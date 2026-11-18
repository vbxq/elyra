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

const LV: &str = "pub let v = vec![1, 2, 3]
pub let vv = vec![vec![1], vec![2]]
pub let mut mv = vec![1, 2, 3]
pub struct P { pub x: int }
pub struct H { pub p: P }
pub let s = P { x: 1 }
pub let h = H { p: P { x: 1 } }
pub let mut ms = P { x: 1 }
pub let mut mh = H { p: P { x: 1 } }
pub let mut arr = [1, 2, 3]
pub fn msx() -> int { ms.x }
pub fn mhx() -> int { mh.p.x }
pub fn mvs() -> int { let mut t = 0
 for e in mv { t = t + e }
 t }
pub fn arr0() -> int { arr[0] }
";

#[test]
fn a_mutable_alias_of_an_imported_immutable_aggregate_is_refused() {
    let dir = tree(&[("lv.aelys", LV)]);
    let sources = [
        ("bare_vec.aelys", "needs lv\nlet mut w = v\n1\n", "v"),
        (
            "path_vec.aelys",
            "needs lv\nlet mut w = lv::v\nw.push(9)\n1\n",
            "lv::v",
        ),
        (
            "bare_struct.aelys",
            "needs lv\nlet mut z = s\nz.x = 5\n1\n",
            "s",
        ),
        (
            "path_struct.aelys",
            "needs lv\nlet mut z = lv::s\n1\n",
            "lv::s",
        ),
        (
            "alias.aelys",
            "needs lv as l\nlet mut z = l::s\n1\n",
            "l::s",
        ),
        (
            "selective.aelys",
            "needs s from lv\nlet mut z = s\n1\n",
            "s",
        ),
        ("paren.aelys", "needs lv\nlet mut z = (lv::s)\n1\n", "lv::s"),
        ("field.aelys", "needs lv\nlet mut z = lv::h.p\n1\n", "lv::h"),
        ("element.aelys", "needs lv\nlet mut z = vv[0]\n1\n", "vv"),
        (
            "infn.aelys",
            "needs lv\nfn f() -> int { let mut z = s\n z.x }\nf()\n",
            "s",
        ),
        (
            "closure.aelys",
            "needs lv\nlet c = fn() { let mut z = lv::v\n z.push(1) }\n1\n",
            "lv::v",
        ),
    ];
    for (file, source, root) in sources {
        let error = run_err(&dir, file, source);
        assert!(error.contains("E0323"), "{file}: {error}");
        assert!(
            !error.contains("dynamic"),
            "{file} roots at {root}: {error}"
        );
    }
    assert_eq!(
        run_int(
            &dir,
            "read.aelys",
            "needs lv\nlet z = lv::s\nlet mut q = lv::s.x\nq = 4\nlet mut w = lv::mv\nw.push(4)\nz.x * 100 + q * 10 + mvs()\n"
        ),
        100 + 40 + 10,
        "an immutable binding, a scalar copy and an alias of a `pub let mut` stay legal"
    );
}

#[test]
fn a_place_rooted_at_an_imported_mutable_variable_is_writable_live() {
    let dir = tree(&[("lv.aelys", LV)]);
    let cases = [
        ("field.aelys", "needs lv\nms.x = 3\nmsx()\n", 3),
        ("compound.aelys", "needs lv\nms.x += 3\nmsx()\n", 4),
        ("nested.aelys", "needs lv\nmh.p.x = 7\nmhx()\n", 7),
        (
            "infn.aelys",
            "needs lv\nfn f() { ms.x = 9 }\nf()\nmsx()\n",
            9,
        ),
        (
            "closure.aelys",
            "needs lv\nlet c = fn() { ms.x = 8 }\nc()\nmsx()\n",
            8,
        ),
        (
            "selective.aelys",
            "needs ms, msx from lv\nms.x = 5\nmsx()\n",
            5,
        ),
        ("index.aelys", "needs lv\nmv[0] = 10\nmvs()\n", 15),
        ("push.aelys", "needs lv\nmv.push(10)\nmvs()\n", 16),
        ("array.aelys", "needs lv\narr[0] = 10\narr0()\n", 10),
        (
            "alias.aelys",
            "needs lv\nlet mut z = ms\nz.x = 4\nmsx()\n",
            4,
        ),
    ];
    for (file, source, expected) in cases {
        assert_eq!(run_int(&dir, file, source), expected, "{file}");
    }
    for (file, source, code) in [
        ("imm_field.aelys", "needs lv\ns.x = 3\n1\n", "E0330"),
        ("imm_nested.aelys", "needs lv\nh.p.x = 3\n1\n", "E0330"),
        ("imm_push.aelys", "needs lv\nv.push(3)\n1\n", "E0318"),
        ("imm_index.aelys", "needs lv\nv[0] = 3\n1\n", "E0318"),
        (
            "own_imm.aelys",
            "needs lv\nlet ms = P { x: 1 }\nfn g() { ms.x = 6 }\n1\n",
            "E0330",
        ),
    ] {
        let error = run_err(&dir, file, source);
        assert!(error.contains(code), "{file}: {error}");
    }
}

#[test]
fn a_private_function_never_resolves_to_a_same_named_function_of_another_module() {
    for (helper_a, helper_b, items_a, items_b) in [
        ("helper", "helper", "items", "items"),
        ("helper", "helper", "items_a", "items_b"),
        ("helper_a", "helper_b", "items", "items"),
    ] {
        let dir = tree(&[
            (
                "a.aelys",
                &format!(
                    "let mut {items_a}: Vec<int> = vec![]\nfn {helper_a}(x: int) -> int {{ x + {items_a}.len() }}\npub fn fill_a() {{ {items_a} = vec![1, 2, 3] }}\npub fn from_a(x: int) -> int {{ {helper_a}(x) }}\npub fn via_closure(x: int) -> int {{ let f = fn(y: int) -> int {{ return {helper_a}(y) }}\n f(x) }}\n"
                ),
            ),
            (
                "b.aelys",
                &format!(
                    "needs a\nlet mut {items_b}: Vec<int> = vec![]\nfn {helper_b}(x: int) -> int {{ x * 100 + {items_b}.len() }}\npub fn fill_b() {{ {items_b} = vec![9] }}\npub fn from_b(x: int) -> int {{ {helper_b}(x) }}\n"
                ),
            ),
        ]);
        assert_eq!(
            run_int(
                &dir,
                "main.aelys",
                "needs a\nneeds b\na::fill_a()\nlet p = a::from_a(5) * 1000 + b::from_b(5)\nb::fill_b()\np * 1000000 + a::from_a(5) * 1000 + b::from_b(5) + a::via_closure(1) * 0\n"
            ),
            8_500 * 1_000_000 + 8_000 + 501,
            "{helper_a}/{helper_b}"
        );
    }
    let dir = tree(&[
        (
            "p.aelys",
            "fn helper() -> int { 1 }\npub fn p() -> int { helper() }\n",
        ),
        (
            "q.aelys",
            "pub fn helper() -> int { 2 }\npub fn q() -> int { helper() }\n",
        ),
    ]);
    assert_eq!(
        run_int(
            &dir,
            "pq.aelys",
            "needs p\nneeds q\nfn helper() -> int { 3 }\np::p() * 100 + q::q() * 10 + helper()\n"
        ),
        123,
        "a private, a public and an entry-file function of one name stay three functions"
    );
}

#[test]
fn a_carried_impl_body_resolves_its_module_names_in_a_transitive_importer() {
    let dir = tree(&[
        (
            "lib.aelys",
            "pub struct Box { pub v: int }\nlet base = 100\nfn make_box(v: int) -> Box { Box { v: v } }\npub fn mk(v: int) -> Box { make_box(v) }\nimpl Box {\n    fn new(v: int) -> Box { make_box(v) }\n    fn bump(self) -> int { self.v + base }\n}\n",
        ),
        (
            "mid.aelys",
            "needs lib\npub fn twice(v: int) -> int { lib::mk(v).v * 2 }\npub fn boxed(v: int) -> Box { lib::mk(v) }\n",
        ),
        (
            "top.aelys",
            "needs mid\npub fn four(v: int) -> int { mid::twice(v) * 2 }\npub fn bumped(v: int) -> int { mid::boxed(v).bump() }\n",
        ),
    ]);
    assert_eq!(
        run_int(
            &dir,
            "main.aelys",
            "needs top\ntop::four(3) * 1000 + top::bumped(5)\n"
        ),
        12_105,
    );
}

const CONTAINER: &str = "pub trait Ranked {\n    fn rank(self) -> int\n}\npub struct Score { pub v: int }\nimpl Ranked for Score {\n    fn rank(self) -> int { self.v }\n}\nlet bonus = 1000\npub struct Stack<T> { pub items: Vec<T> }\nimpl<T> Stack<T> {\n    pub fn pushed(self, v: T) -> Stack<T> {\n        let mut out: Vec<T> = vec![]\n        for x in self.items { out.push(x) }\n        out.push(v)\n        Stack { items: out }\n    }\n    pub fn size(self) -> int { self.items.len() }\n    pub fn top(self) -> T { self.items[self.items.len() - 1] }\n}\npub fn new_stack<T>() -> Stack<T> { Stack { items: vec![] } }\nfn pick<T>(a: T, b: T, ka: int, kb: int) -> T { if ka >= kb { a } else { b } }\npub fn largest<T>(v: Vec<T>, key: fn(T) -> int) -> T {\n    let mut best = v[0]\n    for x in v { best = pick(x, best, key(x), key(best)) }\n    best\n}\npub fn best<T: Ranked>(v: Vec<T>) -> int {\n    let mut top = v[0].rank()\n    for x in v { if x.rank() > top { top = x.rank() } }\n    top + bonus\n}\npub fn describe<T: Display>(x: T) -> string { \"<{x}>\" }\n";
const GENERIC_HELPERS: &str = "needs container\nstruct Cell<T> { v: T }\nfn wrap<T>(x: T) -> Cell<T> { Cell { v: x } }\npub fn twice<T>(x: T) -> Vec<T> { let c = wrap(x)\n vec![c.v, c.v] }\npub fn count<T>(n: int, x: T) -> int { if n == 0 { return 0 }\n 1 + count(n - 1, x) }\npub fn big<T>(v: Vec<T>, key: fn(T) -> int) -> T { container::largest(v, key) }\npub fn local_use() -> int { twice(3).len() + count(2, \"a\") }\n";
const GENERIC_SUMS: &str = "let mut calls = 0\nfn note() { calls = calls + 1 }\npub fn calls_so_far() -> int { calls }\npub enum Maybe<T> { Nope, Has(T) }\npub fn has<T>(x: T) -> Maybe<T> { note()\n Maybe::Has(x) }\npub fn unwrap_or<T>(m: Maybe<T>, d: T) -> T { match m { Maybe::Nope => d, Maybe::Has(v) => v } }\npub fn is_even<T>(n: int, x: T) -> bool { if n == 0 { return true }\n is_odd(n - 1, x) }\npub fn is_odd<T>(n: int, x: T) -> bool { if n == 0 { return false }\n is_even(n - 1, x) }\npub struct Bag<T> { pub items: Vec<T> }\nimpl<T> Bag<T> {\n    pub fn first_or(self, d: T) -> T { note()\n if self.items.len() == 0 { d } else { self.items[0] } }\n}\n";

fn assert_string(dir: &TempDir, main: &str, source: &str, expected: &str) {
    let body = source.trim_end_matches('\n');
    let (head, last) = body.rsplit_once('\n').unwrap_or(("", body));
    let escaped = expected.replace('\\', "\\\\").replace('"', "\\\"");
    let checked = format!(
        "{head}\nlet out = {last}\nprintln(out)\nif out == \"{escaped}\" {{ 1 }} else {{ 0 }}\n"
    );
    assert_eq!(
        run_int(dir, main, &checked),
        1,
        "{main} must end on {expected:?}"
    );
}

#[test]
fn a_generic_function_and_type_exported_by_a_module_take_the_importer_types() {
    let dir = tree(&[("container.aelys", CONTAINER)]);
    let main = "needs container
struct Card { power: int }
impl Ranked for Card {
    fn rank(self) -> int { self.power }
}
impl Display for Card {
    fn to_display(self) -> string { \"card{self.power}\" }
}
let a = largest(vec![3, 9, 2], fn(x: int) -> int { return x })
let b = container::largest(vec![\"ab\", \"abcd\", \"c\"], fn(s: string) -> int { return s.len() })
let c = largest(vec![Card { power: 4 }, Card { power: 7 }], fn(k: Card) -> int { return k.power })
let e: Stack<int> = new_stack()
let s = e.pushed(5).pushed(6)
let t = Stack { items: vec![\"x\"] }.pushed(\"yz\")
let r = best(vec![Card { power: 4 }, Card { power: 8 }]) + best(vec![Score { v: 3 }])
\"{a} {b} {c.power} {s.size()} {s.top()} {t.top()} {r} {describe(5)} {describe(\"q\")} {describe(Card { power: 2 })}\"
";
    assert_string(
        &dir,
        "main.aelys",
        main,
        "9 abcd 7 2 6 yz 2011 <5> <q> <card2>",
    );
    let spellings = [
        (
            "selective.aelys",
            "needs largest from container\nlargest(vec![1, 5, 2], fn(x: int) -> int { return x })\n",
            5,
        ),
        (
            "alias.aelys",
            "needs container as c\nc::largest(vec![1, 5, 2], fn(x: int) -> int { return x })\n",
            5,
        ),
        (
            "shadow_let.aelys",
            "needs container\nlet largest = 4\nlargest + 1\n",
            5,
        ),
        (
            "shadow_param.aelys",
            "needs container\nfn f(largest: int) -> int { largest * 2 }\nf(3)\n",
            6,
        ),
        (
            "own_fn.aelys",
            "needs container\nfn largest(x: int) -> int { 9 }\nlargest(1)\n",
            9,
        ),
        (
            "in_fn.aelys",
            "needs container\nfn f() -> int { let g = fn(v: Vec<int>) -> int { return largest(v, fn(x: int) -> int { return 0 - x }) }\n g(vec![4, 1, 3]) }\nf()\n",
            1,
        ),
    ];
    for (file, source, expected) in spellings {
        assert_eq!(run_int(&dir, file, source), expected, "{file}");
    }
    let hidden = run_err(
        &dir,
        "alias_bare.aelys",
        "needs container as c\nlargest(vec![1], fn(x: int) -> int { return x })\n",
    );
    assert!(
        hidden.contains("E0301"),
        "an aliased import binds no bare name: {hidden}"
    );
}

#[test]
fn an_exported_generic_reaches_its_module_privates_and_other_generics() {
    let dir = tree(&[
        ("container.aelys", CONTAINER),
        ("g2.aelys", GENERIC_HELPERS),
        ("p3.aelys", GENERIC_SUMS),
    ]);
    assert_eq!(
        run_int(
            &dir,
            "helpers.aelys",
            "needs g2\ntwice(\"s\").len() * 100 + count(5, 1.5) * 10 + local_use()\n"
        ),
        254
    );
    assert_eq!(
        run_int(
            &dir,
            "transitive.aelys",
            "needs g2\nbig(vec![2, 8, 1], fn(x: int) -> int { return x })\n"
        ),
        8,
        "g2's generic calls container's generic, which this file never imported"
    );
    assert_string(
        &dir,
        "sums.aelys",
        "needs p3\nlet a = unwrap_or(has(4), 0)\nlet b = unwrap_or(Maybe::Nope, \"d\")\nlet e = if is_even(10, 1.0) { 1 } else { 0 }\nlet g = Bag { items: vec![5] }.first_or(0)\nlet h = Bag { items: vec![] }.first_or(\"x\")\n\"{a} {b} {e} {g} {h} {calls_so_far()}\"\n",
        "4 d 1 5 x 3",
    );
    let private = run_err(&dir, "private.aelys", "needs g2\nwrap(1)\n");
    assert!(
        private.contains("E0301"),
        "a private generic stays private: {private}"
    );
}

#[test]
fn bare_generic_imports_follow_the_ambiguity_rules() {
    let dir = tree(&[
        ("p1.aelys", "pub fn same<T>(x: T) -> T { x }\n"),
        ("p2.aelys", "pub fn same<T>(x: T) -> int { 7 }\n"),
        ("p5.aelys", "pub fn same() -> int { 5 }\n"),
    ]);
    for (file, source) in [
        ("two_generic.aelys", "needs p1\nneeds p2\nsame(1)\n"),
        ("generic_and_plain.aelys", "needs p1\nneeds p5\nsame()\n"),
    ] {
        let error = run_err(&dir, file, source);
        assert!(error.contains("E0445"), "{file}: {error}");
    }
    let clash = run_err(
        &dir,
        "selective.aelys",
        "needs same from p1\nneeds same from p2\n1\n",
    );
    assert!(
        clash.contains("E0410") && clash.contains("'same'"),
        "{clash}"
    );
    assert_eq!(
        run_int(
            &dir,
            "qualified.aelys",
            "needs p1\nneeds p2\np1::same(3) * 10 + p2::same(3)\n"
        ),
        37
    );
    assert_eq!(
        run_int(
            &dir,
            "named.aelys",
            "needs same from p2\nneeds p1\nsame(1)\n"
        ),
        7
    );
    assert_eq!(
        run_int(
            &dir,
            "plain_named.aelys",
            "needs p1\nneeds same from p5\nsame()\n"
        ),
        5
    );
}

#[test]
fn a_private_or_public_generic_type_built_in_a_module_matches_the_importer_instance() {
    let dir = tree(&[
        (
            "g.aelys",
            "pub struct Box<T> { pub v: T }\npub enum Opt<T> { No, Yes(T) }\npub fn boxed(n: int) -> Box<int> { Box { v: n } }\npub fn unbox(b: Box<int>) -> int { b.v }\npub fn yes(n: int) -> Opt<int> { Opt::Yes(n) }\npub fn unyes(o: Opt<int>) -> int { match o { Opt::No => -1, Opt::Yes(n) => n } }\n",
        ),
        (
            "h.aelys",
            "needs g\npub fn hboxed(n: int) -> Box<int> { Box { v: n * 10 } }\npub fn hunbox(b: Box<int>) -> int { b.v + 1 }\npub fn hyes(n: int) -> Opt<int> { Opt::Yes(n * 10) }\npub fn hunyes(o: Opt<int>) -> int { match o { Opt::No => -1, Opt::Yes(n) => n + 1 } }\npub fn hstr(s: string) -> Box<string> { Box { v: s } }\n",
        ),
        (
            "m.aelys",
            "struct Cell<T> { v: T }\nenum Wr<T> { Nil, One(T) }\nfn wrap(n: int) -> Cell<int> { Cell { v: n } }\nfn wrapo(n: int) -> Wr<int> { Wr::One(n) }\nfn unwrap(c: Cell<int>) -> int { c.v }\npub struct S { pub n: int }\nimpl S {\n    fn f(self) -> int { let c = wrap(self.n); c.v + 1 }\n    fn g(self) -> int { let c = Cell { v: self.n }; unwrap(c) + 2 }\n    fn h(self) -> int { match wrapo(self.n) { Wr::Nil => -1, Wr::One(k) => k + 3 } }\n    fn k(self) -> int { let c: Cell<string> = Cell { v: \"abc\" }; c.v.len() }\n}\npub fn viaf(s: S) -> int { s.f() + s.g() + s.h() }\n",
        ),
    ]);
    assert_eq!(
        run_int(
            &dir,
            "public.aelys",
            "needs g\nneeds h\nlet a = Box { v: 5 }\nlet o = Opt::Yes(8)\nlet w = match hyes(9) { Opt::No => 0, Opt::Yes(n) => n }\nlet bs: Box<string> = Box { v: \"yo\" }\nlet mut v: Vec<Box<int>> = vec![]\nv.push(hboxed(1))\nv.push(boxed(2))\nv.push(Box { v: 3 })\nlet mut t = 0\nfor b in v { t = t + unbox(b) + hunbox(b) }\nunbox(a) + hunbox(boxed(3)) * 10 + hunyes(o) * 100 + unyes(o) * 1000 + w * 10000 + t * 1000000 + bs.v.len() * 100000000\n"
        ),
        5 + 40 + 900 + 8000 + 900000 + 33_000_000 + 200_000_000
    );
    assert_eq!(
        run_int(
            &dir,
            "private.aelys",
            "needs m\nlet s = S { n: 10 }\ns.f() * 1000000 + s.g() * 10000 + s.h() * 100 + viaf(s)\n"
        ),
        11_121_336
    );
}
