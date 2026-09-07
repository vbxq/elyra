use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use tempfile::TempDir;

use aelys_driver::run_file;

fn create_module_env() -> TempDir {
    tempfile::tempdir().expect("Failed to create temp dir")
}

fn write_file(dir: &TempDir, path: &str, content: &str) -> PathBuf {
    let file_path = dir.path().join(path);
    if let Some(parent) = file_path.parent() {
        fs::create_dir_all(parent).expect("Failed to create parent directories");
    }
    let mut file = File::create(&file_path).expect("Failed to create file");
    write!(file, "{}", content).expect("Failed to write file");
    file_path
}

const SHAPES_MODULE: &str = r#"
pub enum Shape {
    Circle(int),
    Square(int)
}
"#;

const GEOMETRY_MODULE: &str = r#"
pub struct Point {
    pub x: int,
    pub y: int,
}

pub trait Norm {
    fn norm(self) -> int
}

impl Norm for Point {
    fn norm(self) -> int {
        self.x * self.x + self.y * self.y
    }
}
"#;

#[test]
fn test_imported_enum_is_constructed_and_matched() {
    let dir = create_module_env();
    write_file(&dir, "shapes.aelys", SHAPES_MODULE);

    let main_path = write_file(
        &dir,
        "main.aelys",
        r#"
needs shapes
let c = Shape::Circle(5)
match c {
    Shape::Circle(r) => r * 2,
    Shape::Square(s) => s
}
"#,
    );

    let result = run_file(&main_path).expect("imported enum should be constructible and matchable");
    assert_eq!(result.as_int(), Some(10));
}

#[test]
fn test_imported_enum_match_must_stay_exhaustive() {
    let dir = create_module_env();
    write_file(&dir, "shapes.aelys", SHAPES_MODULE);

    let main_path = write_file(
        &dir,
        "main.aelys",
        r#"
needs shapes
let c = Shape::Circle(5)
match c {
    Shape::Circle(r) => r * 2
}
"#,
    );

    let error = run_file(&main_path).expect_err("a missing variant must be rejected");
    let message = error.to_string();
    assert!(
        message.contains("non-exhaustive match"),
        "unexpected diagnostic: {}",
        message
    );
    assert!(
        message.contains("Shape::Square"),
        "unexpected diagnostic: {}",
        message
    );
}

#[test]
fn test_imported_trait_method_on_imported_struct() {
    let dir = create_module_env();
    write_file(&dir, "geometry.aelys", GEOMETRY_MODULE);

    let main_path = write_file(
        &dir,
        "main.aelys",
        r#"
needs geometry
let p = Point { x: 3, y: 4 }
p.norm()
"#,
    );

    let result = run_file(&main_path).expect("imported trait method should be callable");
    assert_eq!(result.as_int(), Some(25));
}

#[test]
fn test_imported_trait_method_through_generic_bound() {
    let dir = create_module_env();
    write_file(&dir, "geometry.aelys", GEOMETRY_MODULE);

    let main_path = write_file(
        &dir,
        "main.aelys",
        r#"
needs geometry
fn measure<T>(value: T) -> int where T: Norm {
    value.norm()
}
let p = Point { x: 6, y: 8 }
measure(p)
"#,
    );

    let result = run_file(&main_path).expect("imported trait should be usable as a bound");
    assert_eq!(result.as_int(), Some(100));
}

#[test]
fn test_private_type_is_not_importable() {
    let dir = create_module_env();
    write_file(
        &dir,
        "hidden.aelys",
        r#"
enum Secret {
    A,
    B
}

pub fn visible() -> int {
    1
}
"#,
    );

    let main_path = write_file(
        &dir,
        "main.aelys",
        r#"
needs Secret from hidden
1
"#,
    );

    let error = run_file(&main_path).expect_err("a private type must not be importable");
    let message = error.to_string();
    assert!(
        message.contains("'Secret' is not public in module 'hidden'"),
        "unexpected diagnostic: {}",
        message
    );
}

#[test]
fn test_generic_public_type_reports_that_it_cannot_be_exported() {
    let dir = create_module_env();
    write_file(
        &dir,
        "holder.aelys",
        r#"
pub enum Holder<T> {
    One(T)
}
"#,
    );

    let main_path = write_file(
        &dir,
        "main.aelys",
        r#"
needs holder
1
"#,
    );

    let error = run_file(&main_path).expect_err("a generic public type must not be silently lost");
    let message = error.to_string();
    assert!(
        message.contains("cannot be exported"),
        "unexpected diagnostic: {}",
        message
    );
    assert!(
        message.contains("Holder"),
        "unexpected diagnostic: {}",
        message
    );
}

const CANDIDATE_BASE_MODULE: &str = r#"
pub struct Vector {
    pub x: int,
    pub y: int,
}

pub trait Norm {
    fn norm(self) -> int
}
"#;

const CANDIDATE_EXT_MODULE: &str = r#"
needs base

impl Norm for Vector {
    fn norm(self) -> int {
        self.x * self.x + self.y * self.y
    }
}
"#;

#[test]
fn test_impl_from_an_imported_module_is_a_candidate() {
    let dir = create_module_env();
    write_file(&dir, "base.aelys", CANDIDATE_BASE_MODULE);
    write_file(&dir, "ext.aelys", CANDIDATE_EXT_MODULE);

    let main_path = write_file(
        &dir,
        "main.aelys",
        r#"
needs base
needs ext
fn measure<T>(value: T) -> int where T: Norm {
    value.norm()
}
let v = Vector { x: 3, y: 4 }
measure(v)
"#,
    );

    let result = run_file(&main_path).expect("an impl from an imported module must be a candidate");
    assert_eq!(result.as_int(), Some(25));
}

#[test]
fn test_impl_from_an_unimported_module_is_not_a_candidate() {
    let dir = create_module_env();
    write_file(&dir, "base.aelys", CANDIDATE_BASE_MODULE);
    write_file(&dir, "ext.aelys", CANDIDATE_EXT_MODULE);

    let main_path = write_file(
        &dir,
        "main.aelys",
        r#"
needs base
fn measure<T>(value: T) -> int where T: Norm {
    value.norm()
}
let v = Vector { x: 3, y: 4 }
measure(v)
"#,
    );

    let error = run_file(&main_path).expect_err("an unimported impl must not be a candidate");
    let message = error.to_string();
    assert!(
        message.contains("trait 'Norm' is not implemented for Vector"),
        "unexpected diagnostic: {}",
        message
    );
}

#[test]
fn test_same_type_name_in_two_modules_is_rejected() {
    let dir = create_module_env();
    write_file(
        &dir,
        "left.aelys",
        r#"
pub struct Point {
    x: int,
}

impl Point {
    fn tag(self) -> int {
        1
    }
}
"#,
    );
    write_file(
        &dir,
        "right.aelys",
        r#"
pub struct Point {
    y: int,
}

impl Point {
    fn tag(self) -> int {
        2
    }
}
"#,
    );

    let main_path = write_file(
        &dir,
        "main.aelys",
        r#"
needs left
needs right
1
"#,
    );

    let error = run_file(&main_path).expect_err("a colliding type name must be rejected");
    let message = error.to_string();
    assert!(
        message.contains("Point"),
        "unexpected diagnostic: {}",
        message
    );
    assert!(
        message.contains("left") && message.contains("right"),
        "unexpected diagnostic: {}",
        message
    );
}

#[test]
fn test_imported_type_colliding_with_a_local_type_is_rejected() {
    let dir = create_module_env();
    write_file(
        &dir,
        "left.aelys",
        r#"
pub struct Point {
    x: int,
}
"#,
    );

    let main_path = write_file(
        &dir,
        "main.aelys",
        r#"
needs left
struct Point {
    y: int,
}
1
"#,
    );

    let error =
        run_file(&main_path).expect_err("a local type shadowing an import must be rejected");
    let message = error.to_string();
    assert!(
        message.contains("Point"),
        "unexpected diagnostic: {}",
        message
    );
}

const VISIBILITY_MODULE: &str = r#"
pub struct Point {
    pub x: int,
    y: int,
}

pub fn make() -> Point {
    Point { x: 1, y: 2 }
}

fn owner_read(point: Point) -> int {
    point.y
}
"#;

fn assert_visibility_diagnostic(error: aelys_common::error::AelysError, code: &str) {
    let message = error.to_string();
    assert!(message.contains(code), "expected {code}, got: {message}");
    assert!(
        message.contains("owner module 'shapes'") && message.contains("current module"),
        "visibility context missing from diagnostic: {message}"
    );
    assert!(
        message.contains("help:"),
        "visibility help missing from diagnostic: {message}"
    );
}

#[test]
fn public_nominal_return_is_allowed() {
    let dir = create_module_env();
    write_file(&dir, "shapes.aelys", VISIBILITY_MODULE);

    let main_path = write_file(
        &dir,
        "main.aelys",
        r#"
needs shapes
let point = shapes::make()
point.x
"#,
    );

    let result = run_file(&main_path).expect("public nominal values must cross module boundaries");
    assert_eq!(result.as_int(), Some(1));
}

#[test]
fn private_field_read_is_e0412() {
    let dir = create_module_env();
    write_file(&dir, "shapes.aelys", VISIBILITY_MODULE);
    let main_path = write_file(
        &dir,
        "main.aelys",
        "needs shapes\nlet point = shapes::make()\npoint.y\n",
    );

    let error = run_file(&main_path).expect_err("a private field read must be rejected");
    assert_visibility_diagnostic(error, "E0412");
}

#[test]
fn private_field_write_is_e0412() {
    let dir = create_module_env();
    write_file(&dir, "shapes.aelys", VISIBILITY_MODULE);
    let main_path = write_file(
        &dir,
        "main.aelys",
        "needs shapes\nlet mut point = shapes::make()\npoint.y = 7\npoint.x\n",
    );

    let error = run_file(&main_path).expect_err("a private field write must be rejected");
    assert_visibility_diagnostic(error, "E0412");
}

#[test]
fn private_field_literal_is_e0413() {
    let dir = create_module_env();
    write_file(&dir, "shapes.aelys", VISIBILITY_MODULE);
    let main_path = write_file(
        &dir,
        "main.aelys",
        "needs Point from shapes\nPoint { x: 1, y: 2 }\n",
    );

    let error = run_file(&main_path).expect_err("a private field literal must be rejected");
    assert_visibility_diagnostic(error, "E0413");
}

#[test]
fn private_field_pattern_is_e0413() {
    let dir = create_module_env();
    write_file(&dir, "shapes.aelys", VISIBILITY_MODULE);
    let main_path = write_file(
        &dir,
        "main.aelys",
        "needs shapes\nlet point = shapes::make()\nmatch point { Point { y, .. } => y }\n",
    );

    let error = run_file(&main_path).expect_err("a private field pattern must be rejected");
    assert_visibility_diagnostic(error, "E0413");
}

#[test]
fn private_nominal_return_is_e0407() {
    let dir = create_module_env();
    write_file(
        &dir,
        "hidden.aelys",
        r#"
struct Hidden {
    value: int,
}

pub fn leak() -> Hidden {
    Hidden { value: 1 }
}
"#,
    );

    let main_path = write_file(
        &dir,
        "main.aelys",
        r#"
needs hidden
1
"#,
    );

    let error = run_file(&main_path).expect_err("a private nominal type must not be exported");
    let message = error.to_string();
    assert!(
        message.contains("E0407") && message.contains("cannot be exported from module 'hidden'"),
        "unexpected diagnostic: {}",
        message
    );
}

const GAUGE_MODULE: &str = r#"pub let mut ticks = 7
pub struct Gauge { pub base: int }
impl Gauge {
    fn tick(self) -> int {
        return self.base + ticks
    }
}
pub fn ambient() -> int {
    return ticks
}
"#;

#[test]
fn an_imported_impl_body_reads_the_global_of_the_module_that_defined_it() {
    let dir = create_module_env();
    write_file(&dir, "gauge.aelys", GAUGE_MODULE);
    let main_path = write_file(
        &dir,
        "main.aelys",
        "needs Gauge from gauge\nfn main() -> int {\n    let g = Gauge { base: 1 }\n    return g.tick()\n}\nmain()\n",
    );
    let value = run_file(&main_path).expect("the imported impl body must resolve its own global");
    assert_eq!(
        value.as_int(),
        Some(8),
        "only the module's global 'ticks' of 7 added to a base of 1 gives this"
    );
}

#[test]
fn an_imported_impl_body_reads_a_global_its_own_module_only_imported() {
    let dir = create_module_env();
    write_file(&dir, "base.aelys", "pub let stride = 4\n");
    write_file(
        &dir,
        "mid.aelys",
        "needs stride from base\npub struct Meter { pub v: int }\nimpl Meter {\n    fn walk(self) -> int {\n        return self.v + stride\n    }\n}\n",
    );
    let main_path = write_file(
        &dir,
        "main.aelys",
        "needs Meter from mid\nfn main() -> int {\n    let m = Meter { v: 2 }\n    return m.walk()\n}\nmain()\n",
    );
    let value = run_file(&main_path).expect("the impl body must read what its own module read");
    assert_eq!(value.as_int(), Some(6));
}

#[test]
fn a_module_private_global_stays_invisible_to_the_importer() {
    let dir = create_module_env();
    write_file(
        &dir,
        "gauge.aelys",
        "let mut ticks = 7\npub struct Gauge { pub base: int }\nimpl Gauge {\n    fn tick(self) -> int {\n        return self.base + ticks\n    }\n}\n",
    );
    let running = write_file(
        &dir,
        "reader.aelys",
        "needs Gauge from gauge\nfn main() -> int {\n    let g = Gauge { base: 1 }\n    return g.tick()\n}\nmain()\n",
    );
    assert_eq!(
        run_file(&running)
            .expect("a private global is still the impl body's own")
            .as_int(),
        Some(8)
    );

    let leaking = write_file(
        &dir,
        "leaker.aelys",
        "needs Gauge from gauge\nfn main() -> int {\n    return ticks\n}\nmain()\n",
    );
    let error = run_file(&leaking).expect_err("the importer must not see the module's global");
    let message = error.to_string();
    assert!(
        message.contains("ticks") && message.contains("leaker.aelys"),
        "the leak must be refused against the importer's own file: {message}"
    );
}

#[test]
fn a_diagnostic_from_an_imported_impl_body_points_at_the_defining_file() {
    let dir = create_module_env();
    write_file(
        &dir,
        "dials.aelys",
        "pub struct Dial { pub base: int }\npub trait Gauge {\n    type Unit\n    fn g(self) -> int\n}\nimpl Gauge for Dial {\n    type Unit = int\n    fn g(self) -> int {\n        return 1\n    }\n}\nimpl Dial {\n    fn read(self) -> int {\n        let c: Dial::Unit = 3\n        return self.base + c\n    }\n}\n",
    );
    let main_path = write_file(
        &dir,
        "main.aelys",
        "needs Dial, Gauge from dials\ntrait Aaa {\n    type Unit\n    fn a(self) -> int\n}\nimpl Aaa for Dial {\n    type Unit = string\n    fn a(self) -> int {\n        return 2\n    }\n}\nfn main() -> int {\n    let d = Dial { base: 1 }\n    return d.read()\n}\nmain()\n",
    );
    let error = run_file(&main_path).expect_err("the ambiguity the importer creates is rejected");
    let message = error.to_string();
    assert!(
        message.contains("dials.aelys:14:16"),
        "the diagnostic must name the defining file and its real line: {message}"
    );
    assert!(
        message.contains("let c: Dial::Unit = 3"),
        "the rendered line must be the one the span indexes: {message}"
    );
    assert!(
        !message.contains("main.aelys"),
        "the importer's file must not be named: {message}"
    );
    assert!(
        message.contains("as in 'Gauge::Unit'"),
        "the repair offered inside a carried body names a trait that file can write: {message}"
    );
}

#[test]
fn an_imported_impl_body_writes_the_mutable_global_of_its_own_module() {
    let dir = create_module_env();
    write_file(
        &dir,
        "tally.aelys",
        "pub let mut counter = 0\npub struct Tally { pub by: int }\nimpl Tally {\n    fn bump(self) -> int {\n        counter = counter + self.by\n        return counter\n    }\n}\n",
    );
    let main_path = write_file(
        &dir,
        "main.aelys",
        "needs Tally from tally\nfn main() -> int {\n    let t = Tally { by: 3 }\n    return t.bump() + t.bump()\n}\nmain()\n",
    );
    let value = run_file(&main_path).expect("the body must write its own module's global");
    assert_eq!(
        value.as_int(),
        Some(9),
        "3 then 6 is the only sum a global carried across both calls gives"
    );
}

#[test]
fn a_generic_target_type_cannot_cross_a_module_boundary() {
    let dir = create_module_env();
    write_file(
        &dir,
        "wraps.aelys",
        "pub struct Wrap<T> { pub v: T }\npub trait Source {\n    fn next(self) -> int\n}\nimpl Source for Wrap<bool> {\n    fn next(self) -> int {\n        return 2\n    }\n}\n",
    );
    let main_path = write_file(
        &dir,
        "main.aelys",
        "needs Wrap, Source from wraps\nimpl Source for Wrap<int> {\n    fn next(self) -> int {\n        return 1\n    }\n}\nfn main() -> int {\n    let w = Wrap { v: 7 }\n    return Source::next(w)\n}\nmain()\n",
    );
    let error = run_file(&main_path).expect_err("a generic export must be refused");
    let message = error.to_string();
    assert!(
        message.contains("E0407") && message.contains("Wrap"),
        "the export boundary, not the symbol guard, closes this shape, and it names the \
         declaration it refuses: {message}"
    );
    assert!(
        message.contains("monomorphization"),
        "the refusal states why a generic declaration cannot cross: {message}"
    );
    for other in ["E0355", "E0334", "E0340"] {
        assert!(
            !message.contains(other),
            "the boundary refuses the export before any symbol or coherence guard sees it, \
             so {other} must not appear: {message}"
        );
    }

    // the same two files with the target type made concrete cross and run, so the refusal
    let concrete = create_module_env();
    write_file(
        &concrete,
        "wraps.aelys",
        "pub struct Wrap { pub v: bool }\npub trait Source {\n    fn next(self) -> int\n}\nimpl Source for Wrap {\n    fn next(self) -> int {\n        return 2\n    }\n}\n",
    );
    let concrete_main = write_file(
        &concrete,
        "main.aelys",
        "needs Wrap, Source from wraps\nfn main() -> int {\n    let w = Wrap { v: true }\n    return Source::next(w)\n}\nmain()\n",
    );
    assert_eq!(
        run_file(&concrete_main)
            .expect("a concrete target type crosses the boundary")
            .as_int(),
        Some(2),
        "2 is the imported body's own answer"
    );
}

#[test]
fn one_imported_impl_reaching_two_importers_is_registered_once() {
    let dir = create_module_env();
    write_file(
        &dir,
        "alpha.aelys",
        "pub struct Point { pub x: int }\npub trait Norm {\n    fn norm(self) -> int\n}\nimpl Norm for Point {\n    fn norm(self) -> int {\n        return self.x * 3\n    }\n}\n",
    );
    write_file(
        &dir,
        "beta.aelys",
        "needs Point, Norm from alpha\npub fn beta_norm(v: int) -> int {\n    let p = Point { x: v }\n    return p.norm()\n}\n",
    );
    let main_path = write_file(
        &dir,
        "main.aelys",
        "needs Point, Norm from alpha\nneeds beta_norm from beta\nfn main() -> int {\n    let p = Point { x: 2 }\n    return p.norm() * 100 + beta_norm(1)\n}\nmain()\n",
    );
    let value = run_file(&main_path).expect("one impl inlined twice must not collide with itself");
    assert_eq!(
        value.as_int(),
        Some(603),
        "603 pins both call sites to the one imported body"
    );
}

const OWN_PRIVATE_KINDS: [(&str, &str, i64); 5] = [
    (
        "struct Held { pub h: int }",
        "        let held = Held { h: 3 }\n        return held.h",
        3,
    ),
    (
        "enum Held { Small, Large }",
        "        let held = Held::Large\n        match held {\n            Held::Small => { return 1 }\n            Held::Large => { return 21 }\n        }",
        21,
    ),
    (
        "trait Held {\n    fn held(self) -> int\n}\nimpl Held for Dial {\n    fn held(self) -> int {\n        return 31\n    }\n}",
        "        return self.held()",
        31,
    ),
    (
        "fn held() -> int {\n    return 41\n}",
        "        return held()",
        41,
    ),
    ("let held = 51", "        return held", 51),
];

fn own_private_module(declaration: &str, body: &str) -> String {
    format!(
        "{declaration}\n\npub struct Dial {{ pub base: int }}\n\nimpl Dial {{\n    fn read(self) -> int {{\n{body}\n    }}\n}}\n"
    )
}

#[test]
fn every_private_declaration_of_a_module_travels_with_the_body_that_reads_it() {
    for (declaration, body, expected) in OWN_PRIVATE_KINDS {
        let dir = create_module_env();
        write_file(&dir, "dials.aelys", &own_private_module(declaration, body));
        let main_path = write_file(
            &dir,
            "main.aelys",
            "needs Dial from dials\nfn main() -> int {\n    let d = Dial { base: 0 }\n    return d.read()\n}\nmain()\n",
        );
        let value = run_file(&main_path)
            .unwrap_or_else(|error| panic!("'{declaration}' must travel: {error}"));
        assert_eq!(
            value.as_int(),
            Some(expected),
            "only the module's own '{declaration}' produces {expected}"
        );
    }
}

#[test]
fn a_private_declaration_of_a_module_is_never_answered_by_the_importer_s_own() {
    for (declaration, body, expected) in OWN_PRIVATE_KINDS {
        let dir = create_module_env();
        write_file(&dir, "dials.aelys", &own_private_module(declaration, body));
        // a nominal reaches one flat table and is refused; a function and a global have a
        let rival = match declaration.starts_with("fn ") || declaration.starts_with("let ") {
            true => format!("{}\n", declaration.replace(&expected.to_string(), "99")),
            false => "struct Held { pub other: int }\n".to_string(),
        };
        let main_path = write_file(
            &dir,
            "main.aelys",
            &format!(
                "needs Dial from dials\n{rival}fn main() -> int {{\n    let d = Dial {{ base: 0 }}\n    return d.read()\n}}\nmain()\n"
            ),
        );
        match run_file(&main_path) {
            Ok(value) => assert_eq!(
                value.as_int(),
                Some(expected),
                "the carried body reads its own module's '{declaration}', not the importer's"
            ),
            Err(error) => {
                let message = error.to_string();
                assert!(
                    message.contains("E0410"),
                    "a nominal the importer also declares is refused, never rebound: {message}"
                );
            }
        }
    }
}

#[test]
fn a_projection_onto_a_carried_private_type_is_not_the_importer_s_type() {
    let dir = create_module_env();
    write_file(
        &dir,
        "dials.aelys",
        "struct Held { pub h: int }\npub struct Dial { pub base: int }\npub trait Source {\n    type Item\n    fn seed(self) -> int\n}\nimpl Source for Dial {\n    type Item = Held\n    fn seed(self) -> int {\n        return 5\n    }\n}\n",
    );
    let main_path = write_file(
        &dir,
        "main.aelys",
        "needs Dial, Source from dials\nstruct Held { pub h: int }\nfn main() -> int {\n    let a: Dial::Item = Held { h: 3 }\n    return a.h\n}\nmain()\n",
    );
    let message = run_file(&main_path)
        .expect_err("'Dial::Item' names the module's type, which this file did not declare")
        .to_string();
    assert!(
        message.contains("E0410") && message.contains("'Held'"),
        "the importer's own 'Held' may not answer the module's projection: {message}"
    );
}
