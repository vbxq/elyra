
use aelys_cli::cli::commands::compile::compile_to_avbc_with_output;
use aelys_opt::OptimizationLevel;
use std::path::{Path, PathBuf};

// residue tracked by separate increments and must shrink, never grow
const KNOWN_FAILING: &[(&str, &str)] = &[
    (
        "examples/aelys-http-server/lib/router.aelys",
        "E0401: a library fragment, `needs config` resolves next to the fragment and not to the server root",
    ),
    (
        "examples/native/opengl/cube.aelys",
        "E0406: the checked-in opengl native module is built against native ABI 2, the runtime wants 5",
    ),
    (
        "examples/native/opengl/use_opengl.aelys",
        "E0406: the checked-in opengl native module is built against native ABI 2, the runtime wants 5",
    ),
    (
        "main.aelys",
        "E0301: the program itself is wrong, main is declared -> i64 but its body ends in println",
    ),
    // the manual heap intrinsics these call were deleted from the runtime, so they cannot run either way
    (
        "examples/benchmark/fair_comparison.aelys",
        "E0376: `store` has no signature and no runtime implementation",
    ),
    (
        "examples/benchmark/latency_comparison.aelys",
        "E0376: `store` has no signature and no runtime implementation",
    ),
    (
        "examples/benchmark/mandelbrot_nogc.aelys",
        "E0376: `store` has no signature and no runtime implementation",
    ),
    (
        "examples/graphical_demo/donut.aelys",
        "E0376: `store` has no signature and no runtime implementation",
    ),
    (
        "examples/lang/simple_no_gc_demo.aelys",
        "E0376: `store` has no signature and no runtime implementation",
    ),
];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the cli crate always has a workspace parent")
        .to_path_buf()
}

fn collect_sources(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_sources(&path, found);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("aelys") {
            found.push(path);
        }
    }
}

fn shipped_sources(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    collect_sources(&root.join("examples"), &mut found);
    collect_sources(&root.join("modules"), &mut found);
    let main = root.join("main.aelys");
    if main.is_file() {
        found.push(main);
    }
    found.sort();
    found
}

#[test]
fn every_shipped_example_still_compiles() {
    let root = repo_root();
    let sources = shipped_sources(&root);
    assert!(
        sources.len() > 30,
        "expected to discover the shipped examples, found {}",
        sources.len()
    );

    let out_dir = std::env::temp_dir().join("aelys_examples_compile_guard");
    let _ = std::fs::remove_dir_all(&out_dir);
    std::fs::create_dir_all(&out_dir).expect("temp output directory");

    let mut unexpected_failures = Vec::new();
    let mut unexpected_successes = Vec::new();

    for (index, source) in sources.iter().enumerate() {
        let relative = source
            .strip_prefix(&root)
            .unwrap_or(source)
            .to_string_lossy()
            .replace('\\', "/");
        let known_failing = KNOWN_FAILING.iter().any(|(path, _)| *path == relative);

        let output = out_dir.join(format!("guard_{index}.avbc"));
        let result =
            compile_to_avbc_with_output(source, Some(output), OptimizationLevel::None, None);

        match (result, known_failing) {
            (Err(error), false) => unexpected_failures.push(format!("{relative}\n{error}")),
            (Ok(_), true) => unexpected_successes.push(relative),
            _ => {}
        }
    }

    let _ = std::fs::remove_dir_all(&out_dir);

    assert!(
        unexpected_failures.is_empty(),
        "{} shipped source(s) no longer compile:\n\n{}",
        unexpected_failures.len(),
        unexpected_failures.join("\n\n")
    );
    assert!(
        unexpected_successes.is_empty(),
        "these sources now compile and must be removed from KNOWN_FAILING: {}",
        unexpected_successes.join(", ")
    );
}
