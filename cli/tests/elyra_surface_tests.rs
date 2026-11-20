use std::path::PathBuf;
use std::process::{Command, Output};

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_elyra"))
        .args(args)
        .output()
        .unwrap()
}

fn successful(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn public_name_is_elyra() {
    let version = cli(&["version"]);
    assert!(version.status.success());
    assert!(String::from_utf8_lossy(&version.stderr).starts_with("Elyra v"));
    let help = cli(&["--help"]);
    let text = String::from_utf8_lossy(&help.stderr);
    assert!(help.status.success());
    assert!(text.contains("elyra run"));
    assert!(text.contains("main.elyra"));
    assert!(!text.contains("aelys"));
}

#[test]
fn sources_import_compile_and_disassemble_with_both_extensions() {
    for (ext, module_ext) in [
        ("elyra", "elyra"),
        ("aelys", "aelys"),
        ("elyra", "aelys"),
        ("aelys", "elyra"),
    ] {
        let dir: PathBuf = std::env::temp_dir().join(format!(
            "elyra_surface_{}_{ext}_{module_ext}",
            std::process::id()
        ));
        std::fs::create_dir_all(dir.join("nested")).unwrap();
        std::fs::write(
            dir.join(format!("m.{module_ext}")),
            "pub fn id<T>(x: T) -> T { x }\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("nested").join(format!("mod.{module_ext}")),
            "pub fn value() -> int { 5 }\n",
        )
        .unwrap();
        let source = dir.join(format!("main.{ext}"));
        std::fs::write(
            &source,
            "use m\nuse nested\nprintln(m::id(nested::value()))\n",
        )
        .unwrap();
        let source = source.to_str().unwrap();
        assert_eq!(successful(cli(&["run", source])), "5\n");
        successful(cli(&["asm", source, "--stdout"]));
        successful(cli(&["compile", source]));
        assert_eq!(
            successful(cli(&["run", dir.join("main.avbc").to_str().unwrap()])),
            "5\n"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn elyra_project_manifest_is_used() {
    let dir = std::env::temp_dir().join(format!("elyra_manifest_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("main.elyra");
    std::fs::write(&source, "println(5)\n").unwrap();
    std::fs::write(
        dir.join("elyra.toml"),
        "[build]\nbundle_native_modules = true\n",
    )
    .unwrap();
    let manifest = aelys_modules::Manifest::for_source_file(&source).unwrap();
    assert!(manifest.should_bundle_natives());
    std::fs::remove_dir_all(dir).unwrap();
}
