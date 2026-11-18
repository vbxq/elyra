mod s1_support;

use aelys::{CompileOptions, ExecutionOutcome, IsolateConfig, RunOptions, Runtime};
use s1_support::{
    assert_diagnostic, assert_lines_everywhere, compile_error, project_ok, run_project,
};
use std::path::{Path, PathBuf};
use std::process::Command;

mod s1nat {
    use aelys_native::aelys_module;

    #[aelys_module(name = "s1nat", version = "0.1.0")]
    mod exports {
        #[aelys_export]
        pub fn triple(x: i64) -> i64 {
            x * 3
        }

        #[aelys_export]
        pub fn offset(x: i64, by: i64) -> i64 {
            x + by
        }
    }

    pub unsafe fn descriptor() -> &'static aelys_native::AelysModuleDescriptor {
        let pointer = &raw const aelys_module_descriptor_73316e6174;
        unsafe { &*pointer }
    }
}

fn run_native(source: &str) -> Result<String, String> {
    let runtime = Runtime::new();
    unsafe { runtime.register_native_module(s1nat::descriptor()) }.unwrap();
    let module = runtime
        .compile(source, CompileOptions::default())
        .map_err(|error| error.to_string())?;
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    match isolate
        .execute(&module, RunOptions::default())
        .map_err(|error| error.to_string())?
    {
        ExecutionOutcome::Returned(value) => Ok(isolate.value_to_string(value)),
        other => panic!("expected a value, got {other:?}"),
    }
}

#[test]
fn the_standard_library_imports_in_every_form() {
    assert_lines_everywhere(
        "use std::math
use std::math as m
use std::string::{repeat, to_upper}
println(math::sqrt(16.0))
println(m::abs(-3))
println(repeat(to_upper(\"ab\"), 2))
",
        &["4.0", "3", "ABAB"],
    );
}

#[test]
fn a_selection_spans_lines_and_takes_a_trailing_comma() {
    assert_lines_everywhere(
        "use std::math::{
    sqrt,
    PI,
}
use std::string::{repeat,}
println(sqrt(PI * PI) == PI)
println(repeat(\"x\", 3))
",
        &["true", "xxx"],
    );
}

#[test]
fn a_wildcard_binds_every_export() {
    assert_lines_everywhere(
        "use std::math::*\nprintln(sqrt(9.0) + floor(1.5))\n",
        &["4.0"],
    );
}

const GEO: &str = "pub struct P { pub x: int }
pub enum Dir { N, S(int) }
pub fn make(x: int) -> P { P { x: x } }
pub fn turn(d: Dir) -> int { match d { Dir::N => 0, Dir::S(n) => n } }
pub fn largest<T>(v: Vec<T>, key: fn(T) -> int) -> T {
    let mut best = v[0]
    for x in v { if key(x) > key(best) { best = x } }
    best
}
pub let mut hits: int = 0
pub fn hit() -> int {
    hits += 1
    hits
}
";

const UTIL: &str = "pub fn twice(x: int) -> int { x * 2 }\npub let BASE = 40\n";

#[test]
fn a_whole_module_is_reached_through_its_path() {
    let files = [
        ("geo.aelys", GEO),
        ("lib/util.aelys", UTIL),
        (
            "main.aelys",
            "use geo
use lib::util
println(geo::make(5).x + util::twice(1))
println(geo::turn(Dir::S(7)))
println(util::BASE)
",
        ),
    ];
    assert_eq!(project_ok(&files, "main.aelys"), ["7", "7", "40"]);
}

#[test]
fn a_selection_binds_functions_types_and_constants_by_name() {
    let files = [
        ("geo.aelys", GEO),
        ("lib/util.aelys", UTIL),
        (
            "main.aelys",
            "use geo::{P, Dir, make, turn}
use lib::util::{twice, BASE}
let p: P = make(3)
let q = P { x: 4 }
println(p.x + q.x)
println(turn(Dir::S(twice(BASE))))
",
        ),
    ];
    assert_eq!(project_ok(&files, "main.aelys"), ["7", "80"]);
}

#[test]
fn an_alias_replaces_the_module_name() {
    let files = [
        ("geo.aelys", GEO),
        (
            "main.aelys",
            "use geo as g\nprintln(g::make(9).x)\nprintln(g::turn(Dir::N))\n",
        ),
    ];
    assert_eq!(project_ok(&files, "main.aelys"), ["9", "0"]);
}

#[test]
fn imported_variables_stay_the_module_slot() {
    let files = [
        ("geo.aelys", GEO),
        (
            "main.aelys",
            "use geo
use geo::{hits, hit}
hit()
geo::hit()
println(hits)
println(geo::hits)
fn bump() { hits = hits + 10 }
bump()
println(geo::hits)
",
        ),
    ];
    assert_eq!(project_ok(&files, "main.aelys"), ["2", "2", "12"]);
}

#[test]
fn one_type_reached_through_two_spellings_is_one_type() {
    let files = [
        ("geo.aelys", GEO),
        (
            "mid.aelys",
            "use geo::{P}\npub fn shift(p: P) -> P { P { x: p.x + 100 } }\n",
        ),
        (
            "main.aelys",
            "use geo
use mid
use geo::{P}
let a: P = geo::make(1)
let b: P = mid::shift(a)
fn read(p: P) -> int { p.x }
println(read(b))
println(read(mid::shift(P { x: 2 })))
",
        ),
    ];
    assert_eq!(project_ok(&files, "main.aelys"), ["101", "102"]);
}

#[test]
fn a_generic_export_is_a_template_instantiated_per_use() {
    let files = [
        ("geo.aelys", GEO),
        (
            "main.aelys",
            "use geo::{largest}
use geo as g
println(largest(vec![3, 9, 2], fn(x: int) -> int { x }))
println(g::largest(vec![\"a\", \"ccc\", \"bb\"], fn(s: string) -> int { s.len() }))
",
        ),
    ];
    assert_eq!(project_ok(&files, "main.aelys"), ["9", "ccc"]);
}

fn native_test_library() -> PathBuf {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let target_dir = manifest_dir.join("target/native_test");
    let status = Command::new(env!("CARGO"))
        .arg("build")
        .arg("--manifest-path")
        .arg(manifest_dir.join("tests/fixtures/native_test/Cargo.toml"))
        .arg("--release")
        .env("CARGO_TARGET_DIR", &target_dir)
        .status()
        .expect("cargo build should run");
    assert!(status.success(), "native test crate build failed");
    let library = target_dir.join("release").join(format!(
        "{}aelys_native_test{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    ));
    assert!(library.exists(), "missing built library at {library:?}");
    library
}

#[test]
fn a_native_library_module_is_imported_in_every_form() {
    let library = native_test_library();
    for (source, expected) in [
        ("use native_test\nnative_test::add(1, 2)\n", 10),
        (
            "use native_test as nt\nnt::add(1, 2) + nt::b::c(3, 4)\n",
            20,
        ),
        ("use native_test::{add}\nadd(1, 2) * 2\n", 20),
    ] {
        let dir = tempfile::tempdir().unwrap();
        std::fs::copy(
            &library,
            dir.path()
                .join(format!("native_test.{}", std::env::consts::DLL_EXTENSION)),
        )
        .unwrap();
        let main = dir.path().join("main.aelys");
        std::fs::write(&main, source).unwrap();
        let value =
            aelys_driver::run_file(&main).unwrap_or_else(|error| panic!("{source}: {error}"));
        assert_eq!(value.as_int(), Some(expected), "{source}");
    }
}

#[test]
fn a_registered_native_module_is_imported_like_any_module() {
    assert_eq!(run_native("use s1nat\ns1nat::triple(14)").unwrap(), "42");
    assert_eq!(
        run_native("use s1nat\nfn f() -> int { s1nat::offset(s1nat::triple(3), 1) }\nf()").unwrap(),
        "10"
    );
    let old = run_native("needs triple from s1nat\ntriple(1)").unwrap_err();
    assert!(old.contains("error[E0118]"), "{old}");
    assert!(old.contains("= help: use s1nat::{triple}"), "{old}");
}

#[test]
fn needs_is_refused_with_the_use_line_to_write() {
    for (source, help) in [
        ("needs std::math\n1\n", "use std::math"),
        ("needs std::math as m\n1\n", "use std::math as m"),
        (
            "needs sqrt, PI from std::math\n1\n",
            "use std::math::{sqrt, PI}",
        ),
        ("needs sqrt from std::math\n1\n", "use std::math::{sqrt}"),
        ("needs std::math::*\n1\n", "use std::math::*"),
        ("fn f() -> int {\n    1\n}\nneeds geo\nf()\n", "use geo"),
    ] {
        let stderr = compile_error(source);
        assert_diagnostic(&stderr, "E0118", help);
        assert!(stderr.contains("write `use`"), "{stderr}");
    }
}

#[test]
fn needs_in_a_module_file_is_refused_in_that_file() {
    let files = [
        ("old.aelys", "needs geo\npub fn one() -> int { 1 }\n"),
        ("geo.aelys", GEO),
        ("main.aelys", "use old\nprintln(old::one())\n"),
    ];
    let outcome = run_project(&files, "main.aelys");
    assert_eq!(outcome.code, Some(1), "{}", outcome.stderr);
    assert_diagnostic(&outcome.stderr, "E0118", "use geo");
    assert!(outcome.stderr.contains("old.aelys"), "{}", outcome.stderr);
}

#[test]
fn use_is_a_keyword() {
    let stderr = compile_error("let use = 1\nprintln(use)\n");
    assert!(stderr.contains("error[E0101]"), "{stderr}");
}

#[test]
fn the_convert_module_is_gone() {
    let stderr = compile_error("use std::convert\nprintln(1)\n");
    assert!(stderr.contains("error[E0449]"), "{stderr}");
    assert!(stderr.contains("s.to_int()"), "{stderr}");
    let stderr = compile_error("use std::convert as c\nprintln(1)\n");
    assert!(stderr.contains("error[E0449]"), "{stderr}");
}
