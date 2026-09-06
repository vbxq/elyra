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
        "struct Cell { n: int }\npub struct Dial { pub base: int }\nimpl Dial {\n    fn read(self) -> int {\n        let c = Cell { n: 3 }\n        return self.base + c.n\n    }\n}\n",
    );
    let main_path = write_file(
        &dir,
        "main.aelys",
        "needs Dial from dials\n\n\n\n\nfn main() -> int {\n    let d = Dial { base: 1 }\n    return d.read()\n}\nmain()\n",
    );
    let error = run_file(&main_path).expect_err("a private nominal in the body must be rejected");
    let message = error.to_string();
    assert!(
        message.contains("dials.aelys:5:17"),
        "the diagnostic must name the defining file and its real line: {message}"
    );
    assert!(
        message.contains("let c = Cell { n: 3 }"),
        "the rendered line must be the one the span indexes: {message}"
    );
    assert!(
        !message.contains("main.aelys"),
        "the importer's file must not be named: {message}"
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
