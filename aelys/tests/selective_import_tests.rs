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

const SUPPORT_MODULE: &str = r#"
pub struct Stage2Beacon {
    seed: int,
}

pub enum Stage2Signal {
    Up,
    Down
}

pub trait Stage2Imported {
    fn imported_mark(self) -> int
}

impl Stage2Imported for Stage2Beacon {
    fn imported_mark(self) -> int {
        self.seed * 7
    }
}
"#;


#[test]
fn test_selective_trait_import_does_not_bring_an_unnamed_struct_into_scope() {
    let dir = create_module_env();
    write_file(&dir, "stage2_support.aelys", SUPPORT_MODULE);

    let main_path = write_file(
        &dir,
        "main.aelys",
        r#"
needs Stage2Imported from stage2_support
let b = Stage2Beacon { seed: 3 }
b.seed
"#,
    );

    let error = run_file(&main_path)
        .expect_err("a type that the selective import does not name must not be in scope");
    let message = error.to_string();
    assert!(
        message.contains("E0378"),
        "unexpected diagnostic: {}",
        message
    );
    assert!(
        message.contains("Stage2Beacon"),
        "unexpected diagnostic: {}",
        message
    );
    assert!(
        message.contains("stage2_support"),
        "unexpected diagnostic: {}",
        message
    );
    assert!(
        message.contains("needs Stage2Beacon from stage2_support"),
        "the diagnostic must say how to import it: {}",
        message
    );
}

#[test]
fn test_selective_struct_import_does_not_bring_an_unnamed_enum_into_scope() {
    let dir = create_module_env();
    write_file(&dir, "stage2_support.aelys", SUPPORT_MODULE);

    let main_path = write_file(
        &dir,
        "main.aelys",
        r#"
needs Stage2Beacon from stage2_support
match Stage2Signal::Up {
    Stage2Signal::Up => 1,
    Stage2Signal::Down => 2
}
"#,
    );

    let error = run_file(&main_path).expect_err("an unnamed enum must not be in scope");
    let message = error.to_string();
    assert!(
        message.contains("E0378"),
        "unexpected diagnostic: {}",
        message
    );
    assert!(
        message.contains("Stage2Signal"),
        "unexpected diagnostic: {}",
        message
    );
}

#[test]
fn test_selective_struct_import_does_not_bring_an_unnamed_trait_into_scope() {
    let dir = create_module_env();
    write_file(&dir, "stage2_support.aelys", SUPPORT_MODULE);

    let main_path = write_file(
        &dir,
        "main.aelys",
        r#"
needs Stage2Beacon from stage2_support
fn measure<T>(value: T) -> int where T: Stage2Imported {
    value.imported_mark()
}
measure(Stage2Beacon { seed: 2 })
"#,
    );

    let error = run_file(&main_path).expect_err("an unnamed trait must not be in scope");
    let message = error.to_string();
    assert!(
        message.contains("E0378"),
        "unexpected diagnostic: {}",
        message
    );
    assert!(
        message.contains("Stage2Imported"),
        "unexpected diagnostic: {}",
        message
    );
}


#[test]
fn test_the_named_type_is_usable() {
    let dir = create_module_env();
    write_file(&dir, "stage2_support.aelys", SUPPORT_MODULE);

    let main_path = write_file(
        &dir,
        "main.aelys",
        r#"
needs Stage2Beacon from stage2_support
let b = Stage2Beacon { seed: 3 }
b.seed * 2
"#,
    );

    let result = run_file(&main_path).expect("the named type must stay usable");
    assert_eq!(result.as_int(), Some(6));
}

#[test]
fn test_several_named_types_are_usable_together() {
    let dir = create_module_env();
    write_file(&dir, "stage2_support.aelys", SUPPORT_MODULE);

    let main_path = write_file(
        &dir,
        "main.aelys",
        r#"
needs Stage2Beacon, Stage2Signal from stage2_support
let b = Stage2Beacon { seed: 3 }
let s = match Stage2Signal::Down {
    Stage2Signal::Up => 1,
    Stage2Signal::Down => 10
}
b.seed + s
"#,
    );

    let result = run_file(&main_path).expect("every named type must stay usable");
    assert_eq!(result.as_int(), Some(13));
}

#[test]
fn test_the_hint_quotes_a_nested_module_path_in_surface_form() {
    let dir = create_module_env();
    write_file(&dir, "lib/support.aelys", SUPPORT_MODULE);

    let main_path = write_file(
        &dir,
        "main.aelys",
        r#"
needs Stage2Imported from lib::support
let b = Stage2Beacon { seed: 3 }
b.seed
"#,
    );

    let error = run_file(&main_path).expect_err("an unnamed type must not be in scope");
    let message = error.to_string();
    assert!(
        message.contains("needs Stage2Beacon from lib::support"),
        "the hint must be written the way the surface writes it: {}",
        message
    );
}


#[test]
fn test_whole_module_form_still_imports_everything_public() {
    let dir = create_module_env();
    write_file(&dir, "stage2_support.aelys", SUPPORT_MODULE);

    let main_path = write_file(
        &dir,
        "main.aelys",
        r#"
needs stage2_support
let b = Stage2Beacon { seed: 3 }
let s = match Stage2Signal::Up {
    Stage2Signal::Up => 100,
    Stage2Signal::Down => 200
}
b.imported_mark() + s
"#,
    );

    let result = run_file(&main_path).expect("the whole-module form must import everything public");
    assert_eq!(result.as_int(), Some(121));
}


#[test]
fn test_impl_is_withheld_when_only_the_self_type_is_imported() {
    let dir = create_module_env();
    write_file(&dir, "stage2_support.aelys", SUPPORT_MODULE);

    let main_path = write_file(
        &dir,
        "main.aelys",
        r#"
needs Stage2Beacon from stage2_support
let b = Stage2Beacon { seed: 3 }
b.imported_mark()
"#,
    );

    let error = run_file(&main_path)
        .expect_err("an impl whose trait is out of scope must not supply a method");
    let message = error.to_string();
    assert!(
        message.contains("imported_mark"),
        "unexpected diagnostic: {}",
        message
    );
}

#[test]
fn test_impl_is_withheld_when_only_the_trait_is_imported() {
    let dir = create_module_env();
    write_file(&dir, "stage2_support.aelys", SUPPORT_MODULE);

    let main_path = write_file(
        &dir,
        "main.aelys",
        r#"
needs Stage2Imported from stage2_support
struct Local { n: int }
impl Stage2Imported for Local {
    fn imported_mark(self) -> int {
        self.n
    }
}
Local { n: 4 }.imported_mark()
"#,
    );

    let result =
        run_file(&main_path).expect("a selectively imported trait must stay implementable locally");
    assert_eq!(result.as_int(), Some(4));
}

#[test]
fn test_impl_travels_when_both_ends_are_imported() {
    let dir = create_module_env();
    write_file(&dir, "stage2_support.aelys", SUPPORT_MODULE);

    let main_path = write_file(
        &dir,
        "main.aelys",
        r#"
needs Stage2Beacon, Stage2Imported from stage2_support
let b = Stage2Beacon { seed: 3 }
b.imported_mark()
"#,
    );

    let result = run_file(&main_path).expect("an impl must travel when both its ends are in scope");
    assert_eq!(result.as_int(), Some(21));
}


#[test]
fn test_inherent_impl_travels_with_a_selectively_imported_struct() {
    let dir = create_module_env();
    write_file(
        &dir,
        "inherent_support.aelys",
        r#"
pub struct Counter {
    n: int,
}

pub struct Unused {
    m: int,
}

impl Counter {
    fn doubled(self) -> int {
        self.n * 2
    }
}
"#,
    );

    let main_path = write_file(
        &dir,
        "main.aelys",
        r#"
needs Counter from inherent_support
Counter { n: 21 }.doubled()
"#,
    );

    let result = run_file(&main_path).expect("an inherent impl must travel with its self type");
    assert_eq!(result.as_int(), Some(42));
}
