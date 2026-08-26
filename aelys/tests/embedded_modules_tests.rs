//! the embedding api must compile a multi-module program, hand back an isolate the host can call

use aelys::{CompileOptions, ExecutionOutcome, IsolateConfig, RunOptions, Runtime, Value};
use aelys_common::error::{AelysError, CompileErrorKind};
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

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

fn module_dir() -> TempDir {
    tempfile::tempdir().expect("temp dir")
}

fn write_file(dir: &TempDir, name: &str, content: &str) -> PathBuf {
    let path = dir.path().join(name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("parent directories");
    }
    fs::write(&path, content).expect("write module");
    path
}

fn module_not_found(error: AelysError) -> (String, Vec<String>) {
    match error {
        AelysError::Compile(error) => {
            assert_eq!(
                error.kind.code(),
                401,
                "expected E0401, got {:?}",
                error.kind
            );
            match error.kind {
                CompileErrorKind::ModuleNotFound {
                    module_path,
                    searched_paths,
                } => (module_path, searched_paths),
                other => panic!("expected ModuleNotFound, got {other:?}"),
            }
        }
        other => panic!("expected a compile error, got {other:?}"),
    }
}

// that still holds a callable isolate
#[test]
fn an_imported_trait_method_is_callable_through_the_embedding_api() {
    let dir = module_dir();
    write_file(&dir, "geometry.aelys", GEOMETRY_MODULE);
    let entry = write_file(
        &dir,
        "main.aelys",
        r#"
needs geometry

fn stage2_probe(x: int, y: int) -> int {
    let p = Point { x: x, y: y }
    p.norm()
}
0
"#,
    );

    let runtime = Runtime::new();
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    let module = isolate
        .compile_file(&entry, CompileOptions::default())
        .expect("a two-file program must compile");
    let outcome = isolate
        .execute(&module, RunOptions::default())
        .expect("the entry module must run");
    assert!(matches!(outcome, ExecutionOutcome::Returned(_)));

    let probe = isolate
        .get_function("stage2_probe")
        .expect("the host must resolve a function of the multi-module program");
    let result = isolate
        .call(&probe, &[Value::int(3), Value::int(4)])
        .expect("the imported trait method must dispatch");
    assert_eq!(result.as_int(), Some(25));
}

#[test]
fn an_in_memory_entry_can_import_from_a_module_root() {
    let dir = module_dir();
    write_file(&dir, "geometry.aelys", GEOMETRY_MODULE);

    let runtime = Runtime::new();
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    let module = isolate
        .compile_with_root(
            "needs geometry\nfn measure() -> int {\n    let p = Point { x: 6, y: 8 }\n    p.norm()\n}\n0\n",
            dir.path(),
            CompileOptions::default(),
        )
        .expect("an embedded entry source must import from the module root");
    isolate
        .execute(&module, RunOptions::default())
        .expect("the entry module must run");

    let measure = isolate.get_function("measure").expect("resolve measure");
    let result = isolate.call(&measure, &[]).expect("call measure");
    assert_eq!(result.as_int(), Some(100));
}

#[test]
fn a_host_native_module_is_reachable_from_a_multi_module_program() {
    let dir = module_dir();
    write_file(
        &dir,
        "helper.aelys",
        "pub fn double(n: int) -> int {\n    n * 2\n}\n",
    );
    let entry = write_file(
        &dir,
        "main.aelys",
        r#"
needs helper
needs probe

fn combined() -> int {
    double(probe::answer())
}
0
"#,
    );

    let runtime = Runtime::new();
    unsafe { runtime.register_native_module(probe_module::descriptor()) }
        .expect("register the host native module");
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    let module = isolate
        .compile_file(&entry, CompileOptions::default())
        .expect("a host native module must stay reachable from a multi-module program");
    // re-running the entry must not drop the imported module's globals
    for _ in 0..3 {
        isolate
            .execute(&module, RunOptions::default())
            .expect("the entry module must run");
        let combined = isolate.get_function("combined").expect("resolve combined");
        let result = isolate.call(&combined, &[]).expect("call combined");
        assert_eq!(result.as_int(), Some(84));
    }
}

#[test]
fn an_unresolvable_needs_is_rejected_by_the_string_compile_path() {
    let runtime = Runtime::new();
    let Err(error) = runtime.compile(
        "needs there_is_no_such_module_xyz\nfn stage2_probe() -> int { 42 }\nfn main(frame: int) { }\n",
        CompileOptions::default(),
    ) else {
        panic!("an unresolvable needs must not be silently accepted");
    };
    let (module_path, searched_paths) = module_not_found(error);
    assert_eq!(module_path, "there_is_no_such_module_xyz");
    assert!(
        !searched_paths.is_empty(),
        "the diagnostic must say what was searched"
    );
}

#[test]
fn an_unresolvable_needs_is_rejected_by_the_file_compile_path() {
    let dir = module_dir();
    let entry = write_file(
        &dir,
        "main.aelys",
        "needs there_is_no_such_module_xyz\nfn stage2_probe() -> int { 42 }\n0\n",
    );

    let runtime = Runtime::new();
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    let Err(error) = isolate.compile_file(&entry, CompileOptions::default()) else {
        panic!("an unresolvable needs must not be silently accepted");
    };
    let (module_path, searched_paths) = module_not_found(error);
    assert_eq!(module_path, "there_is_no_such_module_xyz");
    assert!(
        searched_paths
            .iter()
            .any(|path| path.contains("there_is_no_such_module_xyz")),
        "the diagnostic must list the paths searched, got {searched_paths:?}"
    );
}

#[test]
fn a_builtin_module_needs_still_compiles() {
    let runtime = Runtime::new();
    let module = runtime
        .compile(
            "needs std::math\nfn root(n: float) -> float { math::sqrt(n) }\n0\n",
            CompileOptions::default(),
        )
        .expect("a built-in module the runtime provides directly must stay accepted");
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    isolate
        .execute(&module, RunOptions::default())
        .expect("the module must run");
    let root = isolate.get_function("root").expect("resolve root");
    let result = isolate
        .call(&root, &[Value::float(9.0)])
        .expect("call root");
    assert_eq!(result.as_float(), Some(3.0));

    let dir = module_dir();
    let entry = write_file(
        &dir,
        "main.aelys",
        "needs std::math\nfn root(n: float) -> float { math::sqrt(n) }\n0\n",
    );
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    isolate
        .compile_file(&entry, CompileOptions::default())
        .expect("a built-in module must resolve on the file path too");
}

#[test]
fn the_string_compile_path_is_unchanged() {
    let runtime = Runtime::new();
    let module = runtime
        .compile(
            "fn twice(n: int) -> int { return n * 2 }\n0",
            CompileOptions::default(),
        )
        .expect("a single-string program must still compile");
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    assert!(matches!(
        isolate.execute(&module, RunOptions::default()),
        Ok(ExecutionOutcome::Returned(_))
    ));
    let twice = isolate.get_function("twice").expect("resolve twice");
    let result = isolate.call(&twice, &[Value::int(21)]).expect("call twice");
    assert_eq!(result.as_int(), Some(42));
    assert!(!module.avbc().is_empty());
}

#[test]
fn a_module_compiled_for_one_isolate_is_refused_by_another() {
    let dir = module_dir();
    write_file(&dir, "geometry.aelys", GEOMETRY_MODULE);
    let entry = write_file(
        &dir,
        "main.aelys",
        "needs geometry\nlet p = Point { x: 3, y: 4 }\np.norm()\n",
    );

    let runtime = Runtime::new();
    let mut first = runtime.new_isolate(IsolateConfig::default());
    let module = first
        .compile_file(&entry, CompileOptions::default())
        .expect("compile the two-file program");
    let mut second = runtime.new_isolate(IsolateConfig::default());
    assert!(
        second.execute(&module, RunOptions::default()).is_err(),
        "a module linked against one isolate must not silently run on another"
    );
    assert!(matches!(
        first.execute(&module, RunOptions::default()),
        Ok(ExecutionOutcome::Returned(_))
    ));
}

mod probe_module {
    use aelys_native::aelys_module;

    #[aelys_module(name = "probe", version = "0.1.0")]
    mod exports {
        #[aelys_export]
        pub fn answer() -> i64 {
            42
        }
    }

    pub unsafe fn descriptor() -> &'static aelys_native::AelysModuleDescriptor {
        let pointer = &raw const aelys_module_descriptor_70726f6265;
        unsafe { &*pointer }
    }
}
