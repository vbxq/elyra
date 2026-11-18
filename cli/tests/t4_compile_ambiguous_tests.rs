use aelys_cli::cli::commands::compile::compile_to_avbc;
use aelys_opt::OptimizationLevel;

fn compile_error(case: &str, files: &[(&str, &str)], main: &str) -> String {
    let dir = std::env::temp_dir().join(format!("aelys_t4_compile_ambiguous_{case}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (name, body) in files {
        std::fs::write(dir.join(name), body).unwrap();
    }
    let src_path = dir.join("main.aelys");
    std::fs::write(&src_path, main).unwrap();
    compile_to_avbc(&src_path, OptimizationLevel::None)
        .expect_err("an ambiguous bare name must not compile")
}

#[test]
fn compile_reports_e0445_for_a_bare_name_two_whole_modules_export() {
    for (case, main, name, owners) in [
        (
            "top",
            "needs std::fs\nneeds std::bytes\nlet b = bytes::alloc(7)\nprintln(\"{size(b)}\")\n",
            "size",
            ["fs", "bytes"],
        ),
        (
            "rev",
            "needs std::bytes\nneeds std::fs\nlet b = bytes::alloc(7)\nlet n = size(b)\n",
            "size",
            ["bytes", "fs"],
        ),
        (
            "fn",
            "needs std::fs\nneeds std::bytes\nfn f(b: int) -> int {\n    copy(b, 0, b, 0, 1)\n    return 1\n}\nf(1)\n",
            "copy",
            ["fs", "bytes"],
        ),
        (
            "value",
            "needs std::fs\nneeds std::bytes\nlet s = size\n",
            "size",
            ["fs", "bytes"],
        ),
    ] {
        let error = compile_error(case, &[], main);
        assert!(error.contains("E0445"), "{case}: {error}");
        assert!(!error.contains("E0379"), "{case}: {error}");
        assert!(
            error.contains(&format!("symbol '{name}' is exported by multiple modules")),
            "{case}: {error}"
        );
        assert!(
            error.contains(&format!("'{}::{name}'", owners[0]))
                && error.contains(&format!("'{}::{name}'", owners[1])),
            "{case}: {error}"
        );
    }
}

#[test]
fn compile_reports_e0445_for_a_bare_name_three_user_modules_export() {
    let modules = [
        ("ma.aelys", "pub fn val() -> int { return 1 }\n"),
        ("mb.aelys", "pub fn val() -> int { return 2 }\n"),
        ("mc.aelys", "pub fn val() -> int { return 3 }\n"),
    ];
    let error = compile_error(
        "user3",
        &modules,
        "needs ma\nneeds mb\nneeds mc\nlet v = val()\n",
    );
    assert!(error.contains("E0445"), "{error}");
    assert!(
        error.contains("'ma::val' or 'mb::val' or 'mc::val'"),
        "{error}"
    );
}
