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
        .unwrap_or_else(|| panic!("{main} must end on an int"))
}

fn run_err(dir: &TempDir, main: &str, source: &str) -> String {
    let path = write(dir.path(), main, source);
    run_file(&path)
        .map(|value| panic!("{main} must be refused, got {value:?}"))
        .unwrap_err()
        .to_string()
}

const OTHER: &str = "pub struct Post { pub top: int }
pub enum Kind { A, B(int) }
pub struct Zed { pub z: int, pub k: Kind }
pub fn post(n: int) -> Post { Post { top: n } }
";

const GEOM: &str = "pub struct Vec2 { pub x: int, pub y: int }
pub enum Dir { N, E, S, W }
pub struct Seg { pub a: Vec2, pub b: Vec2, pub dir: Dir }
pub enum Shape { Line(Seg), Dot(Vec2), Many(Vec<Vec2>) }
pub trait Measure {
    fn measure(self) -> int
}
impl Measure for Vec2 {
    fn measure(self) -> int { self.x + self.y }
}
pub fn v(x: int, y: int) -> Vec2 { Vec2 { x: x, y: y } }
pub fn dir_code(d: Dir) -> int {
    match d { Dir::N => 1, Dir::E => 2, Dir::S => 3, Dir::W => 4 }
}
";

const WAD: &str = "needs Vec2, Seg, Dir, Shape, Measure, v from geom
pub struct Lump { pub name: string, pub pts: Vec<Vec2>, pub seg: Seg, pub shape: Shape, pub opt: Option<Vec2> }
impl Measure for Lump {
    fn measure(self) -> int { self.pts.len() * 1000 + self.seg.a.measure() }
}
pub fn load(n: int) -> Lump {
    let mut pts: Vec<Vec2> = vec![]
    for i in 0..n { pts.push(v(i, i * 10)) }
    let seg = Seg { a: v(1, 2), b: v(3, 4), dir: Dir::S }
    Lump { name: \"E1M1\", pts: pts, seg: seg, shape: Shape::Many(vec![v(5, 6), v(7, 8)]), opt: Some(v(9, 9)) }
}
";

const RENDER: &str = "needs geom
needs Lump, load from wad
needs other
pub fn shape_sum(s: Shape) -> int {
    match s {
        Shape::Line(seg) => seg.a.x + seg.b.y + dir_code(seg.dir),
        Shape::Dot(p) => p.x * p.y,
        Shape::Many(ps) => {
            let mut t = 0
            for p in ps { t = t + p.x + p.y }
            t
        }
    }
}
pub fn total(l: Lump) -> int {
    let mut t = 0
    for p in l.pts { t = t + p.y }
    t = t + l.seg.b.x * 100 + dir_code(l.seg.dir)
    t = t + shape_sum(l.shape)
    let o = match l.opt { Some(p) => p.x, None => 0 }
    t + o + other::post(1).top
}
";

const MATRIX_BODY: &str = "struct Box<T> { v: T }
let l = load(4)
let a = render::total(l)
let s1 = render::shape_sum(Shape::Line(Seg { a: v(1, 1), b: v(2, 2), dir: Dir::W }))
let s2 = render::shape_sum(Shape::Dot(l.seg.a))
let b = Box { v: l.seg.b }
let z = Zed { z: 1, k: Kind::B(5) }
let zk = match z.k { Kind::A => 0, Kind::B(n) => n }
let mut l2 = load(2)
l2.seg.a.x = 50
let m = l2.measure() + l.seg.a.measure()
a * 1000000 + s1 * 10000 + s2 * 100 + b.v.y + zk + m
";

#[test]
fn nested_struct_and_enum_schemas_agree_across_modules_under_every_needs_order() {
    let dir = tree(&[
        ("other.aelys", OTHER),
        ("geom.aelys", GEOM),
        ("wad.aelys", WAD),
        ("render.aelys", RENDER),
    ]);
    let orders = [
        [
            "needs other",
            "needs render",
            "needs Lump, load from wad",
            "needs geom",
        ],
        [
            "needs geom",
            "needs Lump, load from wad",
            "needs render",
            "needs other",
        ],
        [
            "needs Lump, load from wad",
            "needs other",
            "needs geom",
            "needs render",
        ],
    ];
    for (index, order) in orders.iter().enumerate() {
        let source = format!("{}\n{MATRIX_BODY}", order.join("\n"));
        assert_eq!(
            run_int(&dir, &format!("main{index}.aelys"), &source),
            399_072_264,
            "total 399, line 7, dot 2, box 4, payload 5, measures 2052 + 3 under order {order:?}"
        );
    }
}

#[test]
fn recursive_array_and_payload_schemas_cross_three_modules() {
    let dir = tree(&[
        (
            "a.aelys",
            "pub struct Node { pub v: int, pub kids: Vec<Node> }
pub struct Tri { pub pts: [int; 3] }
pub fn leaf(v: int) -> Node { Node { v: v, kids: vec![] } }
pub fn sum(n: Node) -> int {
    let mut t = n.v
    for k in n.kids { t = t + sum(k) }
    t
}
pub fn maker() -> fn(int) -> Node {
    return fn(x: int) -> Node { leaf(x * 2) }
}
",
        ),
        (
            "b.aelys",
            "pub struct Pad { pub q: int }\npub enum E { One(Pad), Two }\n",
        ),
        (
            "c.aelys",
            "needs Node, leaf, Tri from a
needs Pad from b
pub enum Wrap { N(Node), P(Pad), T(Tri) }
pub fn wsum(w: Wrap) -> int { match w { Wrap::N(n) => n.v, Wrap::P(p) => p.q, Wrap::T(t) => t.pts[2] } }
",
        ),
    ]);
    let value = run_int(
        &dir,
        "main.aelys",
        "needs b
needs c
needs a
struct Local { n: Node }
let mut ks: Vec<Node> = vec![]
ks.push(leaf(2))
ks.push(Node { v: 3, kids: vec![leaf(4)] })
let root = Node { v: 1, kids: ks }
let s = a::sum(root)
let f = vec![leaf(7), leaf(8)][0]
let g = a::maker()
let loc = Local { n: g(5) }
let w1 = c::wsum(Wrap::N(leaf(11)))
let w2 = c::wsum(Wrap::P(Pad { q: 12 }))
let w3 = c::wsum(Wrap::T(Tri { pts: [1, 2, 13] }))
let e = match E::One(Pad { q: 6 }) { E::One(p) => p.q, E::Two => 0 }
s * 100000000 + f.v * 1000000 + loc.n.v * 10000 + w1 + w2 + w3 + e * 1000
",
    );
    assert_eq!(
        value, 1_007_106_036,
        "tree 10, first 7, closure 10, payloads 36, enum 6"
    );
}

#[test]
fn an_enum_built_in_one_module_matches_in_another_that_declares_other_enums() {
    let dir = tree(&[
        (
            "a.aelys",
            "pub enum Shape { Circle(int), Square(int) }
pub fn make(n: int) -> Shape { if n > 0 { Shape::Circle(n) } else { Shape::Square(0 - n) } }
pub fn area(s: Shape) -> int { match s { Shape::Circle(r) => r * 3, Shape::Square(w) => w * w } }
",
        ),
        (
            "b.aelys",
            "pub enum Aaa { X, Y }\npub enum Bbb { P, Q }\npub fn aaa() -> Aaa { Aaa::Y }\n",
        ),
    ]);
    let value = run_int(
        &dir,
        "main.aelys",
        "needs b
needs a
let s = a::make(5)
let r = match s { Shape::Circle(r) => r, Shape::Square(w) => 100 + w }
let y = match b::aaa() { Aaa::X => 1, Aaa::Y => 2 }
a::area(s) * 10000 + r * 100 + a::area(Shape::Square(4)) + y * 1000000
",
    );
    assert_eq!(value, 2_150_516, "area 15, binding 5, square 16, variant Y");
}

#[test]
fn a_runtime_error_inside_a_module_names_the_module_file_and_line() {
    let dir = tree(&[(
        "rt.aelys",
        "pub fn boom(v: Vec<int>) -> int {\n    let x = 1\n    v[5] + x\n}\n",
    )]);
    let error = run_err(
        &dir,
        "main.aelys",
        "needs rt\nlet v = vec![1]\nlet pad = 0\nrt::boom(v)\n",
    );
    assert!(error.contains("rt.aelys:3:"), "{error}");
    assert!(error.contains("v[5] + x"), "{error}");
    assert!(error.contains("main.aelys:4)"), "{error}");
}

const SCOPED_LIB: &str = "needs std::bytes as by
needs util as u
needs thrice from util
pub struct Key { pub lo: int, pub hi: int }
pub enum Kind { A(int), B }
struct Hidden { v: int }
pub let SCALE = 10
let secret = 7
fn helper(n: int) -> int { n + secret }
pub fn key_of(n: int) -> Key { Key { lo: n, hi: n * 2 } }
pub struct Table { pub total: int }
impl Table {
    pub fn make(n: int) -> Table {
        let k = key_of(n)
        let h = Hidden { v: helper(n) }
        let kind = Kind::A(u::twice(n))
        let extra = match kind { Kind::A(x) => x, Kind::B => 0 }
        let b = by::from_string(\"AB\")
        let first = by::read_u8(b, 0)
        Table { total: k.lo + k.hi + h.v + extra + thrice(1) + SCALE + first + math::abs(0 - 1) + convert::parse_int(\"5\").unwrap_or(0) }
    }
    fn get(self) -> int { self.total }
}
";

const SCOPED_UTIL: &str =
    "pub fn twice(x: int) -> int { x * 2 }\npub fn thrice(x: int) -> int { x * 3 }\n";

#[test]
fn a_carried_impl_body_resolves_every_name_in_its_own_module() {
    let dir = tree(&[("util.aelys", SCOPED_UTIL), ("lib.aelys", SCOPED_LIB)]);
    let expected = 5 + 10 + 12 + 10 + 3 + 10 + 65 + 1 + 5;
    assert_eq!(
        run_int(
            &dir,
            "by_name.aelys",
            "needs Table from lib\nTable::make(5).get()\n"
        ),
        expected,
        "the importer never named Key, Kind, by, u, thrice, helper or SCALE"
    );
    assert_eq!(
        run_int(
            &dir,
            "rebound.aelys",
            "needs lib\nneeds std::fs as by\nneeds util as u\nlet thrice = 1000\nfn helper(n: int) -> int { n * 1000 }\nTable::make(5).get()\n"
        ),
        expected,
        "the importer's own by, u, thrice and helper must not leak into the body"
    );
    write(
        dir.path(),
        "mid.aelys",
        "needs Table from lib\npub fn run() -> int { Table::make(5).get() }\n",
    );
    assert_eq!(
        run_int(&dir, "through_mid.aelys", "needs mid\nmid::run()\n"),
        expected,
        "a module importer has no prelude of its own to lend the body"
    );
}

#[test]
fn a_type_reached_through_an_imported_signature_keeps_its_fields_and_methods() {
    let dir = tree(&[(
        "mob.aelys",
        "pub struct Mobj { pub hp: int, pub x: int }
impl Mobj {
    pub fn double(self) -> int { self.hp * 2 }
}
pub fn spawn(hp: int) -> Mobj { Mobj { hp: hp, x: 3 } }
",
    )]);
    assert_eq!(
        run_int(
            &dir,
            "use.aelys",
            "needs spawn from mob\nlet m = spawn(5)\nm.double() * 100 + m.x\n"
        ),
        1003
    );
    let error = run_err(
        &dir,
        "name.aelys",
        "needs spawn from mob\nlet m = Mobj { hp: 1, x: 2 }\n",
    );
    assert!(
        error.contains("E0378"),
        "naming it still needs an import: {error}"
    );
}

#[test]
fn builtin_modules_are_visible_inside_an_imported_module() {
    let dir = tree(&[(
        "helper.aelys",
        "pub fn letter(n: int) -> string { convert::chr(n) }
pub fn root(x: float) -> int { math::floor(math::sqrt(x)) }
pub fn clock_ok() -> bool { time::now_ms() > 0 }
pub fn shout() -> int {
    println(\"from helper\")
    sqrt(16.0) as int
}
",
    )]);
    let value = run_int(
        &dir,
        "main.aelys",
        "needs helper
let l = helper::letter(65)
let c = if helper::clock_ok() { 1 } else { 0 }
let a = if l == \"A\" { 1 } else { 0 }
helper::root(81.0) * 1000 + helper::shout() * 10 + c + a * 100000
",
    );
    assert_eq!(value, 109_041, "letter A, root 9, shout 4, clock 1");
}

#[test]
fn an_undefined_variable_hint_stays_outside_the_quoted_name() {
    let message = aelys_common::error::CompileErrorKind::UndefinedVariable(
        "sqrt\n\nhint: did you mean 'sq'?".to_string(),
    )
    .message();
    assert_eq!(
        message,
        "undefined variable 'sqrt'\n   = hint: did you mean 'sq'?"
    );
    let runtime = aelys_common::error::RuntimeErrorKind::UndefinedVariable(
        "sqrt\n\nhint: did you mean 'sq'?".to_string(),
    )
    .message();
    assert_eq!(
        runtime,
        "undefined variable 'sqrt'\n   = hint: did you mean 'sq'?"
    );
}

#[test]
fn two_whole_module_imports_sharing_a_name_clash_only_on_a_bare_use() {
    let dir = tree(&[]);
    assert_eq!(
        run_int(
            &dir,
            "qualified.aelys",
            "needs std::fs\nneeds std::bytes\nlet b = bytes::alloc(4)\nbytes::size(b)\n"
        ),
        4
    );
    let bare = run_err(
        &dir,
        "bare.aelys",
        "needs std::fs\nneeds std::bytes\nlet b = bytes::alloc(4)\nsize(b)\n",
    );
    assert!(bare.contains("E0410"), "{bare}");
    assert!(bare.contains("'size'"), "{bare}");
    assert!(
        bare.contains("bare.aelys:4:1"),
        "the use site carries it: {bare}"
    );
    assert_eq!(
        run_int(
            &dir,
            "named.aelys",
            "needs std::fs\nneeds size from std::bytes\nneeds std::bytes\nsize(bytes::alloc(3))\n"
        ),
        3,
        "a name imported by name wins over the whole-module ones"
    );
    let twice = run_err(
        &dir,
        "twice.aelys",
        "needs size from std::fs\nneeds size from std::bytes\n",
    );
    assert!(twice.contains("E0410"), "{twice}");
    assert!(twice.contains("twice.aelys:2:1"), "{twice}");
}

#[test]
fn user_modules_sharing_a_name_clash_only_on_a_bare_use() {
    let dir = tree(&[
        ("wad.aelys", "pub fn init() -> int { 1 }\n"),
        ("gfx.aelys", "pub fn init() -> int { 20 }\n"),
        (
            "both.aelys",
            "needs wad\nneeds gfx\npub fn go() -> int { wad::init() + gfx::init() }\npub fn bad() -> int { init() }\n",
        ),
    ]);
    assert_eq!(
        run_int(
            &dir,
            "main.aelys",
            "needs wad\nneeds gfx\nwad::init() * 100 + gfx::init()\n"
        ),
        120
    );
    let error = run_err(&dir, "bare.aelys", "needs wad\nneeds gfx\ninit()\n");
    assert!(error.contains("E0410"), "{error}");
    let nested = run_err(&dir, "nested.aelys", "needs both\nboth::go()\n");
    assert!(
        nested.contains("E0410") && nested.contains("both.aelys:4:"),
        "{nested}"
    );
}

#[test]
fn needs_order_never_decides_which_file_a_dotted_path_names() {
    let dir = tree(&[
        ("lib/util.aelys", "pub fn twice(x: int) -> int { x * 2 }\n"),
        (
            "game/mobj.aelys",
            "needs lib::util\npub struct Mobj { pub hp: int, pub x: int }\npub fn spawn(hp: int) -> Mobj { Mobj { hp: hp, x: util::twice(hp) } }\n",
        ),
    ]);
    let first =
        "needs lib::util\nneeds Mobj, spawn from game::mobj\nspawn(5).x * 100 + util::twice(1)\n";
    let second =
        "needs Mobj, spawn from game::mobj\nneeds lib::util\nspawn(5).x * 100 + util::twice(1)\n";
    assert_eq!(
        run_int(&dir, "b.aelys", first),
        1002,
        "game/ falls back to the root lib"
    );
    assert_eq!(
        run_int(&dir, "c.aelys", second),
        1002,
        "the same lines swapped"
    );

    write(
        dir.path(),
        "game/lib/util.aelys",
        "pub fn twice(x: int) -> int { x * 1000 }\n",
    );
    assert_eq!(
        run_int(&dir, "b2.aelys", first),
        500_002,
        "game/lib/util beside the importer wins, the root keeps its own"
    );
    assert_eq!(run_int(&dir, "c2.aelys", second), 500_002);
}

#[test]
fn one_file_reached_by_two_dotted_paths_is_one_module_instance() {
    let dir = tree(&[
        (
            "game/lib/counter.aelys",
            "pub let mut hits = 0\npub fn bump() -> int {\n    hits = hits + 1\n    hits\n}\n",
        ),
        (
            "game/mobj.aelys",
            "needs lib::counter\npub fn hit() -> int { counter::bump() }\n",
        ),
    ]);
    assert_eq!(
        run_int(
            &dir,
            "main.aelys",
            "needs game::lib::counter\nneeds game::mobj\nlet a = counter::bump()\nlet b = mobj::hit()\nlet c = counter::bump()\na * 100 + b * 10 + c\n"
        ),
        123,
        "one instance counts 1, 2, 3"
    );
}

#[test]
fn an_alias_or_a_bare_import_never_rebinds_another_module_reference() {
    let dir = tree(&[
        ("ua.aelys", "pub fn f() -> int { 1 }\n"),
        ("ub.aelys", "pub fn f() -> int { 20 }\n"),
        (
            "ma.aelys",
            "needs ua as u\npub fn run() -> int { u::f() }\n",
        ),
        (
            "mb.aelys",
            "needs ub as u\npub fn run() -> int { u::f() }\n",
        ),
        ("na.aelys", "needs f from ua\npub fn run() -> int { f() }\n"),
        ("nb.aelys", "needs f from ub\npub fn run() -> int { f() }\n"),
        (
            "sa.aelys",
            "needs std::fs as io2\npub fn run() -> bool { io2::exists(\"/\") }\n",
        ),
        (
            "sb.aelys",
            "needs std::bytes as io2\npub fn run() -> int { io2::size(io2::alloc(7)) }\n",
        ),
    ]);
    assert_eq!(
        run_int(
            &dir,
            "main.aelys",
            "needs ma\nneeds mb\nneeds na\nneeds nb\nneeds sa\nneeds sb\nlet s = if sa::run() { 1 } else { 0 }\nma::run() * 1000 + mb::run() * 100 + na::run() * 10 + nb::run() + s * 100000 + sb::run() * 1000000\n"
        ),
        7_103_030,
        "each module keeps its own u, f and io2"
    );
}

#[test]
fn pub_is_accepted_on_impl_items_inherent_trait_and_across_modules() {
    let dir = tree(&[(
        "shape.aelys",
        "pub struct Sq { pub w: int }
pub trait Area {
    fn area(self) -> int
}
impl Sq {
    pub fn new(w: int) -> Sq { Sq { w: w } }
    pub fn twice(self) -> int { self.w * 2 }
}
impl Area for Sq {
    pub fn area(self) -> int { self.w * self.w }
}
",
    )]);
    assert_eq!(
        run_int(
            &dir,
            "main.aelys",
            "needs Sq, Area from shape\nlet s = Sq::new(3)\ns.area() * 100 + s.twice()\n"
        ),
        906
    );
}

#[test]
fn a_generic_pub_function_is_refused_at_its_declaration_with_a_named_reason() {
    let dir = tree(&[(
        "gen.aelys",
        "pub fn plain(x: int) -> int { x }\npub fn first<T>(v: Vec<T>) -> T { v[0] }\n",
    )]);
    let error = run_err(&dir, "main.aelys", "needs gen\ngen::plain(1)\n");
    assert!(error.contains("E0407"), "{error}");
    assert!(error.contains("'first'"), "{error}");
    assert!(
        error.contains("gen.aelys:2:"),
        "the declaration carries it: {error}"
    );
    write(
        dir.path(),
        "private.aelys",
        "fn first<T>(v: Vec<T>) -> T { v[0] }\npub fn head(v: Vec<int>) -> int { first(v) }\n",
    );
    assert_eq!(
        run_int(
            &dir,
            "ok.aelys",
            "needs private\nprivate::head(vec![4, 5])\n"
        ),
        4,
        "a private generic helper stays usable inside its module"
    );
}

#[test]
fn an_imported_variable_is_the_declaring_module_slot_for_reads_and_writes() {
    let dir = tree(&[
        (
            "alpha.aelys",
            "pub let mut v: int = 100\npub fn get() -> int { v }\n",
        ),
        ("cst.aelys", "pub let SCALE = 10\n"),
    ]);
    assert_eq!(
        run_int(
            &dir,
            "write.aelys",
            "needs v, get from alpha\nfn bump() { v = v + 1 }\nbump()\nbump()\nv * 1000 + get()\n"
        ),
        102_102,
        "both bumps land in alpha's own v"
    );
    let fixed = run_err(&dir, "fixed.aelys", "needs cst\nSCALE = 3\nSCALE\n");
    assert!(fixed.contains("E0203"), "{fixed}");
    assert_eq!(
        run_int(
            &dir,
            "shadow.aelys",
            "needs cst\nlet mut SCALE = 1\nSCALE = 3\nSCALE * 100 + cst::SCALE\n"
        ),
        310,
        "a file's own declaration shadows the import and leaves the module alone"
    );
}
