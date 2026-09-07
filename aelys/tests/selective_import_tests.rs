use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use tempfile::TempDir;

use aelys_driver::run_file;

mod common;
use common::{assert_associated_diagnostic, assert_located_at};

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
    pub seed: int,
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
    pub n: int,
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

const ASSOCIATED_SUPPORT: &str = r#"
pub struct Counter { pub v: int }

pub trait Source {
    type Item
    const LIMIT: int
    fn next(self) -> Self::Item
}

impl Source for Counter {
    type Item = int
    const LIMIT: int = 4
    fn next(self) -> int { return self.v }
}
"#;

fn imported_projection_main(receiver: &str, item: &str, position: &str, split: bool) -> String {
    let projection = format!("{receiver}::{item}");
    let line = match position {
        "parameter" => {
            format!("fn probe(x: {projection}) -> int {{\n    return x + 100\n}}\nprobe(2)\n")
        }
        "return" => format!(
            "fn probe(x: {projection}) -> {projection} {{\n    return x\n}}\nprobe(2) + 100\n"
        ),
        "annotation" => format!(
            "fn probe(x: {projection}) -> int {{\n    let z: {projection} = x\n    return z + 100\n}}\nprobe(2)\n"
        ),
        "field" => format!(
            "struct Holder {{ it: {projection} }}\nfn probe() -> int {{\n    let h = Holder {{ it: 2 }}\n    return h.it + 100\n}}\nprobe()\n"
        ),
        "bound" => format!(
            "fn pull<T: Source>(s: T, x: T::Item) -> T::Item {{\n    return x\n}}\nfn probe(x: {projection}) -> int {{\n    return pull(Counter {{ v: 0 }}, x) + 100\n}}\nprobe(2)\n"
        ),
        "value" => format!("fn probe() -> int {{ return {projection} + 1 }}\nprobe()\n"),
        "length" => format!(
            "fn probe() -> int {{\n    let a: [int; {projection}] = [0, 0, 0, 7]\n    return a[3]\n}}\nprobe()\n"
        ),
        "method" => "fn probe() -> int { return Counter { v: 6 }.next() }\nprobe()\n".to_string(),
        other => unreachable!("no imported cell for {other}"),
    };
    let imports = match split {
        true => "needs Counter from support\nneeds Source from support",
        false => "needs Counter, Source from support",
    };
    format!("{imports}\n{line}")
}

fn run_imported(receiver: &str, item: &str, position: &str, split: bool) -> Result<i64, String> {
    let dir = create_module_env();
    write_file(&dir, "support.aelys", ASSOCIATED_SUPPORT);
    let main = write_file(
        &dir,
        "main.aelys",
        &imported_projection_main(receiver, item, position, split),
    );
    match run_file(&main) {
        Ok(value) => Ok(value.as_int().unwrap_or_default()),
        Err(error) => Err(error.to_string()),
    }
}

const IMPORTED_TYPE_POSITIONS: [&str; 5] = ["parameter", "return", "annotation", "field", "bound"];

#[test]
fn an_associated_type_crosses_a_selective_import_in_every_type_position() {
    for position in IMPORTED_TYPE_POSITIONS {
        for receiver in ["Counter", "Source"] {
            for split in [false, true] {
                assert_eq!(
                    run_imported(receiver, "Item", position, split),
                    Ok(102),
                    "cell {receiver}::Item in {position} position must resolve across the import \
                     and carry an int through it, split {split}"
                );
            }
        }
    }
}

#[test]
fn an_imported_impl_method_crosses_a_selective_import_written_either_way() {
    for split in [false, true] {
        assert_eq!(
            run_imported("Counter", "Item", "method", split),
            Ok(6),
            "the method of the imported impl must run, split {split}"
        );
    }
}

#[test]
fn an_associated_constant_crosses_a_selective_import_in_both_value_positions() {
    for split in [false, true] {
        assert_eq!(
            run_imported("Counter", "LIMIT", "value", split),
            Ok(5),
            "an imported constant must fold in value position, split {split}"
        );
        assert_eq!(
            run_imported("Counter", "LIMIT", "length", split),
            Ok(7),
            "an imported constant must fold into an array length, split {split}"
        );
    }
}

#[test]
fn an_imported_projection_keeps_its_namespace_and_absence_diagnostics() {
    for position in IMPORTED_TYPE_POSITIONS {
        for split in [false, true] {
            let Err(message) = run_imported("Counter", "LIMIT", position, split) else {
                panic!("a constant in {position} position must be rejected across the import");
            };
            assert!(
                message.contains("error[E0423]")
                    && message.contains("'LIMIT' is an associated constant of 'Counter'"),
                "cell Counter::LIMIT in {position} position, split {split}: {message}"
            );
            let Err(absent) = run_imported("Counter", "NOPE", position, split) else {
                panic!("an absent item in {position} position must be rejected across the import");
            };
            assert!(
                absent.contains("error[E0423]")
                    && absent.contains("no impl for 'Counter' defines 'NOPE'"),
                "cell Counter::NOPE in {position} position, split {split}: {absent}"
            );
        }
    }
    for split in [false, true] {
        let Err(type_in_value) = run_imported("Counter", "Item", "value", split) else {
            panic!("a type in value position must be rejected across the import");
        };
        assert!(
            type_in_value.contains("'Item' is an associated type of 'Counter'"),
            "cell Counter::Item in value position, split {split}: {type_in_value}"
        );
    }
}

#[test]
fn a_projection_needs_the_trait_that_declares_it_to_be_imported_too() {
    let dir = create_module_env();
    write_file(&dir, "support.aelys", ASSOCIATED_SUPPORT);
    let main = write_file(
        &dir,
        "main.aelys",
        "needs Counter from support\nfn probe(x: Counter::Item) -> int { return 1 }\n1\n",
    );
    let error = run_file(&main).expect_err("the trait must be in scope for the item to resolve");
    let message = error.to_string();
    assert!(
        message.contains("error[E0423]")
            && message.contains("no impl for 'Counter' defines 'Item'"),
        "importing the type alone must report the item absent: {message}"
    );
}

#[test]
fn a_module_qualified_receiver_is_not_a_projection_receiver() {
    // measured, not desired: the whole-module and aliased import forms bring
    for (import, receiver) in [
        ("needs support", "support::Counter"),
        ("needs support as s", "s::Counter"),
    ] {
        let dir = create_module_env();
        write_file(&dir, "support.aelys", ASSOCIATED_SUPPORT);
        let main = write_file(
            &dir,
            "main.aelys",
            &format!("{import}\nfn probe(x: {receiver}::Item) -> int {{ return 1 }}\n1\n"),
        );
        let error = run_file(&main).expect_err("a module path cannot receive a projection");
        let message = error.to_string();
        assert!(
            message.contains("error[E0372]") && message.contains("no such type is in scope"),
            "the {import} form reports the module head as an unknown type: {message}"
        );
    }
}

#[test]
fn an_impl_written_beside_an_imported_trait_still_answers_a_projection() {
    let dir = create_module_env();
    write_file(
        &dir,
        "support.aelys",
        "pub trait Source {\n    type Item\n    const LIMIT: int\n}\n",
    );
    let main = write_file(
        &dir,
        "main.aelys",
        r#"
needs Source from support
struct Counter { v: int }
impl Source for Counter {
    type Item = int
    const LIMIT: int = 4
}
fn probe(x: Counter::Item) -> int { return x + Counter::LIMIT }
probe(3)
"#,
    );
    let value = run_file(&main).expect("a local impl of an imported trait must resolve");
    assert_eq!(value.as_int(), Some(7));
}

const WHOLE_MODULE_SUPPORT: &str = r#"
pub struct Cell { pub v: int }

pub trait Source {
    type Item
    const LIMIT: int
}

impl Source for Cell {
    type Item = int
    const LIMIT: int = 4
}

pub fn make() -> int { return 11 }

fn secret() -> int { return 7 }
"#;

// receiver as the bare type the `needs` brought into scope, the rejected half
fn whole_module_main(receiver: &str, item: &str, position: &str) -> String {
    let projection = format!("{receiver}::{item}");
    let line = match position {
        "annotation" => format!(
            "fn probe(x: {projection}) -> int {{\n    let z: {projection} = x\n    return z\n}}\nprobe(3)\n"
        ),
        "parameter" => format!("fn probe(x: {projection}) -> int {{ return 1 }}\nprobe(3)\n"),
        "return" => format!("fn probe(x: {projection}) -> {projection} {{ return x }}\nprobe(3)\n"),
        "field" => format!("struct Holder {{ it: {projection} }}\nHolder {{ it: 5 }}.it\n"),
        "binding" => format!(
            "fn probe() -> int {{\n    let z: {projection} = 6\n    return z\n}}\nprobe()\n"
        ),
        "value" => format!("fn probe() -> int {{ return {projection} + 1 }}\nprobe()\n"),
        "length" => format!(
            "fn probe() -> int {{\n    let a: [int; {projection}] = [0, 0, 0, 7]\n    return a[3]\n}}\nprobe()\n"
        ),
        other => unreachable!("no whole-module cell for {other}"),
    };
    format!("needs support\n{line}")
}

fn run_whole_module(receiver: &str, item: &str, position: &str) -> Result<i64, String> {
    let dir = create_module_env();
    write_file(&dir, "support.aelys", WHOLE_MODULE_SUPPORT);
    let main = write_file(
        &dir,
        "main.aelys",
        &whole_module_main(receiver, item, position),
    );
    match run_file(&main) {
        Ok(value) => Ok(value.as_int().unwrap_or_default()),
        Err(error) => Err(error.to_string()),
    }
}

const WHOLE_MODULE_POSITIONS: [&str; 7] = [
    "annotation",
    "parameter",
    "return",
    "field",
    "binding",
    "value",
    "length",
];

#[test]
fn a_module_qualified_receiver_never_reports_a_visibility_cause() {
    for item in ["Item", "LIMIT"] {
        for position in WHOLE_MODULE_POSITIONS {
            let message = run_whole_module("support::Cell", item, position)
                .expect_err("a module-qualified receiver carries no projection");
            assert_associated_diagnostic(
                &message,
                "E0372",
                "unknown type 'support'",
                "",
                "no such type is in scope",
            );
            assert!(
                !message.contains("is not public"),
                "support::Cell::{item} in {position} position must not claim a visibility cause: {message}"
            );
            assert!(
                !message.contains("add 'pub'"),
                "support::Cell::{item} in {position} position must not prescribe a 'pub' that cannot be written inside an impl: {message}"
            );
        }
    }
}

#[test]
fn the_bare_receiver_the_module_brought_in_still_answers_every_projection() {
    for (position, expected) in [
        ("annotation", 3),
        ("parameter", 1),
        ("return", 3),
        ("field", 5),
        ("binding", 6),
    ] {
        assert_eq!(
            run_whole_module("Cell", "Item", position),
            Ok(expected),
            "Cell::Item in {position} position must run after `needs support`"
        );
    }
    for (position, expected) in [("value", 5), ("length", 7)] {
        assert_eq!(
            run_whole_module("Cell", "LIMIT", position),
            Ok(expected),
            "Cell::LIMIT in {position} position must fold after `needs support`"
        );
    }
}

fn run_module_member(name: &str) -> Result<i64, String> {
    let dir = create_module_env();
    write_file(&dir, "support.aelys", WHOLE_MODULE_SUPPORT);
    let main = write_file(
        &dir,
        "main.aelys",
        &format!("needs support\nsupport::{name}()\n"),
    );
    match run_file(&main) {
        Ok(value) => Ok(value.as_int().unwrap_or_default()),
        Err(error) => Err(error.to_string()),
    }
}

#[test]
fn a_module_member_still_reports_the_visibility_it_looked_up() {
    assert_eq!(
        run_module_member("make"),
        Ok(11),
        "an exported free function must run through its module alias"
    );
    let message =
        run_module_member("secret").expect_err("an unexported free function must be rejected");
    assert_associated_diagnostic(
        &message,
        "E0313",
        "module member 'support::secret'",
        "",
        "is not public; add 'pub' to its declaration",
    );
}

const NOMINAL_ITEM_SUPPORT: &str = r#"
pub struct Payload { pub n: int }

pub struct Counter { pub c: int }

pub trait Source {
    type Item

    fn make(self) -> Payload
}

impl Source for Counter {
    type Item = Payload

    fn make(self) -> Payload {
        return Payload { n: self.c * 3 }
    }
}
"#;

fn nominal_projection_main(import: &str, receiver: &str, position: &str) -> String {
    let item = format!("{receiver}::Item");
    let body = match position {
        "field" => format!(
            "struct Holder {{ it: {item} }}\nfn probe() -> int {{\n    let h = Holder {{ it: Counter {{ c: 4 }}.make() }}\n    return h.it.n + 100\n}}\nprobe()\n"
        ),
        "parameter" => format!(
            "fn take(x: {item}) -> int {{\n    return x.n + 100\n}}\nfn probe() -> int {{\n    return take(Counter {{ c: 4 }}.make())\n}}\nprobe()\n"
        ),
        "return" => format!(
            "fn give(c: Counter) -> {item} {{\n    return c.make()\n}}\nfn probe() -> int {{\n    return give(Counter {{ c: 4 }}).n + 100\n}}\nprobe()\n"
        ),
        "annotation" => format!(
            "fn probe() -> int {{\n    let p: {item} = Counter {{ c: 4 }}.make()\n    return p.n + 100\n}}\nprobe()\n"
        ),
        "enum payload" => format!(
            "enum Slot {{ Full({item}), Empty }}\nfn probe() -> int {{\n    let s = Slot::Full(Counter {{ c: 4 }}.make())\n    return match s {{\n        Slot::Full(p) => p.n + 100,\n        Slot::Empty => 0\n    }}\n}}\nprobe()\n"
        ),
        "generic argument" => format!(
            "struct Box<T> {{ it: T }}\nfn probe() -> int {{\n    let b = Box {{ it: Counter {{ c: 4 }}.make() }}\n    let p: {item} = b.it\n    return p.n + 100\n}}\nprobe()\n"
        ),
        "bound" => format!(
            "fn pull<T: Source>(s: T, x: T::Item) -> T::Item {{\n    return x\n}}\nfn probe() -> int {{\n    let p: {item} = pull(Counter {{ c: 4 }}, Counter {{ c: 4 }}.make())\n    return p.n + 100\n}}\nprobe()\n"
        ),
        other => unreachable!("no nominal cell for {other}"),
    };
    format!("{import}\n{body}")
}

fn run_nominal_projection(import: &str, receiver: &str, position: &str) -> Result<i64, String> {
    let dir = create_module_env();
    write_file(&dir, "support.aelys", NOMINAL_ITEM_SUPPORT);
    let main = write_file(
        &dir,
        "main.aelys",
        &nominal_projection_main(import, receiver, position),
    );
    match run_file(&main) {
        Ok(value) => Ok(value.as_int().unwrap_or_default()),
        Err(error) => Err(error.to_string()),
    }
}

const NOMINAL_IMPORT_FORMS: [&str; 4] = [
    "needs Counter, Source from support",
    "needs Counter from support\nneeds Source from support",
    "needs support",
    "needs support as s",
];

const NOMINAL_ITEM_POSITIONS: [&str; 7] = [
    "field",
    "parameter",
    "return",
    "annotation",
    "enum payload",
    "generic argument",
    "bound",
];

#[test]
fn a_nominal_associated_type_crosses_every_import_form_in_every_type_position() {
    for import in NOMINAL_IMPORT_FORMS {
        for receiver in ["Counter", "Source"] {
            for position in NOMINAL_ITEM_POSITIONS {
                assert_eq!(
                    run_nominal_projection(import, receiver, position),
                    Ok(112),
                    "'{receiver}::Item' resolves to the module's own 'Payload' in {position} \
                     position under `{import}`, and 4 * 3 + 100 is the sum only that resolution \
                     gives"
                );
            }
        }
    }
}

#[test]
fn a_type_a_carried_impl_needs_is_registered_but_stays_unnameable() {
    let dir = create_module_env();
    write_file(&dir, "support.aelys", NOMINAL_ITEM_SUPPORT);
    let main = write_file(
        &dir,
        "main.aelys",
        "needs Counter, Source from support\nstruct Holder { it: Payload }\nfn probe() -> int {\n    return 1\n}\nprobe()\n",
    );
    let message = run_file(&main)
        .expect_err("the importer never named 'Payload', so it may not write it")
        .to_string();
    assert!(
        message.contains("E0378") && message.contains("needs Payload from support"),
        "the name the carried impl needs is refused to the importer with its repair: {message}"
    );
    assert_located_at("E0378", &message, 2);
}

const PRIVATE_ITEM_SUPPORT: &str = r#"
struct Hidden { n: int }

pub struct Counter { pub c: int }

pub trait Source {
    type Item

    fn get(self) -> int
}

impl Source for Counter {
    type Item = Hidden

    fn get(self) -> int {
        return self.c
    }
}
"#;

// the same module with the private declaration made generic: monomorphization erased it
const PRIVATE_GENERIC_ITEM_SUPPORT: &str = r#"
struct Hidden<T> { n: T }

pub struct Counter { pub c: int }

pub trait Source {
    type Item

    fn get(self) -> int
}

impl Source for Counter {
    type Item = Hidden<int>

    fn get(self) -> int {
        return self.c
    }
}
"#;

#[test]
fn a_refusal_inside_a_carried_impl_names_the_file_that_owns_the_span() {
    for import in ["needs Counter, Source from support", "needs support"] {
        let dir = create_module_env();
        write_file(&dir, "support.aelys", PRIVATE_GENERIC_ITEM_SUPPORT);
        let main = write_file(
            &dir,
            "main.aelys",
            &format!("{import}\nfn probe() -> int {{\n    return 1\n}}\nprobe()\n"),
        );
        let message = run_file(&main)
            .expect_err("a generic private type has no definition to travel with the body")
            .to_string();
        assert!(
            message.contains("E0407") && message.contains("'Hidden'"),
            "the refusal names the type the impl wrote: {message}"
        );
        assert!(
            message.contains("support.aelys") && !message.contains("main.aelys"),
            "the span belongs to the defining file and must name it: {message}"
        );
        assert!(
            message.contains("struct Hidden<T> { n: T }"),
            "the rendered line must be the one the span covers, not an empty line past the end \
             of another file: {message}"
        );
        assert_located_at("E0407", &message, 2);
    }
}

#[test]
fn a_module_s_own_private_nominal_travels_with_the_body_that_names_it() {
    for import in ["needs Counter, Source from support", "needs support"] {
        let dir = create_module_env();
        write_file(&dir, "support.aelys", PRIVATE_ITEM_SUPPORT);
        let main = write_file(
            &dir,
            "main.aelys",
            &format!(
                "{import}\nfn probe() -> int {{\n    let c = Counter {{ c: 6 }}\n    \
                 return c.get()\n}}\nprobe()\n"
            ),
        );
        assert_eq!(
            run_file(&main)
                .expect("the body reads the private type of its own module")
                .as_int(),
            Some(6),
            "only the module's own 'Hidden' answers 'type Item = Hidden'"
        );

        let naming = write_file(
            &dir,
            "naming.aelys",
            &format!(
                "{import}\nfn probe() -> int {{\n    let h = Hidden {{ n: 1 }}\n    return h.n\n}}\nprobe()\n"
            ),
        );
        let message = run_file(&naming)
            .expect_err("the importer may not name what only a carried body reaches")
            .to_string();
        assert!(
            message.contains("E0403") && message.contains("'Hidden'"),
            "the importer's own use of the carried name is refused: {message}"
        );
        assert!(
            message.contains("not public in module 'support'"),
            "no `needs` line reaches it, and the message says why: {message}"
        );
        assert!(
            !message.contains("needs Hidden from support"),
            "a repair that would itself be refused must not be offered: {message}"
        );
    }
}

#[test]
fn a_carried_private_nominal_does_not_bind_to_the_importer_s_own_declaration() {
    let dir = create_module_env();
    write_file(&dir, "support.aelys", PRIVATE_ITEM_SUPPORT);
    let main = write_file(
        &dir,
        "main.aelys",
        "needs Counter, Source from support\nstruct Hidden { pub other: int }\nfn probe() -> int {\n    return 1\n}\nprobe()\n",
    );
    let message = run_file(&main)
        .expect_err("two declarations of one name reach one flat table")
        .to_string();
    assert!(
        message.contains("E0410") && message.contains("'Hidden'"),
        "the collision is reported rather than resolved in the importer's favour: {message}"
    );
    assert!(
        message.contains("main.aelys"),
        "the file that declared the clashing name is where the repair is written: {message}"
    );
}
