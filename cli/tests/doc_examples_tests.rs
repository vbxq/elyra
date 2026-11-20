use std::path::Path;
use std::process::Command;

#[test]
fn every_documentation_example_runs_as_documented() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the cli crate always has a workspace parent");
    if Command::new("python3").arg("--version").output().is_err() {
        eprintln!("python3 is not available, the documentation examples are not checked");
        return;
    }
    let work = std::env::temp_dir().join(format!("aelys-doc-examples-{}", std::process::id()));
    std::fs::create_dir_all(&work).expect("create the doctest work directory");
    let output = Command::new("python3")
        .arg(root.join("tools/doctest.py"))
        .arg("--aelys")
        .arg(env!("CARGO_BIN_EXE_elyra"))
        .arg("--root")
        .arg(root)
        .arg("--work-dir")
        .arg(&work)
        .current_dir(root)
        .output()
        .expect("run tools/doctest.py");
    let _ = std::fs::remove_dir_all(&work);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "documentation examples failed:\n{stdout}\n{stderr}"
    );
}
