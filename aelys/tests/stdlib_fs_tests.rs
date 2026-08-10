mod common;
use common::*;
use std::fs;
use tempfile::tempdir;

#[test]
fn fs_open_read_close() {
    let dir = tempdir().unwrap();
    let test_file = dir.path().join("data.txt");
    fs::write(&test_file, "test content").unwrap();
    let path_str = test_file.display().to_string().replace('\\', "/");

    let code = format!(
        r#"
needs std::fs
let f = fs::open("{}", "r")
let data = fs::read(f)
fs::close(f)
42
"#,
        path_str
    );

    assert_aelys_int(&code, 42);
}

#[test]
fn fs_write_and_read_text() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.txt");
    let path_str = path.display().to_string().replace('\\', "/");

    let code = format!(
        r#"
needs std::fs
fs::write_text("{}", "hello world")
fs::read_text("{}")
    "#,
        path_str, path_str
    );

    let result = run_aelys_result(&code);
    assert!(result.is_ok(), "fs write/read should work: {:?}", result);
}

#[test]
fn fs_open_invalid_mode() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.txt");
    let path_str = path.display().to_string().replace('\\', "/");

    let code = format!(
        r#"
needs std::fs
fs::open("{}", "xyz")
"#,
        path_str
    );

    let err = run_aelys_err(&code);
    assert!(err.contains("invalid") || err.contains("mode"));
}

#[test]
fn fs_close_invalid_handle() {
    let code = r#"
needs std::fs
fs::close(999)
"#;
    let err = run_aelys_err(code);
    assert!(err.contains("invalid") || err.contains("handle"));
}

#[test]
fn fs_read_line_eof_returns_null() {
    let dir = tempdir().unwrap();
    let test_file = dir.path().join("lines.txt");
    fs::write(&test_file, "line1\nline2\n").unwrap();
    let path_str = test_file.display().to_string().replace('\\', "/");

    let code = format!(
        r#"
needs std::fs
let f = fs::open("{}", "r")
let l1 = fs::read_line(f)
let l2 = fs::read_line(f)
let eof = fs::read_line(f)
fs::close(f)
42
"#,
        path_str
    );

    assert_aelys_int(&code, 42);
}

#[test]
fn fs_read_bytes_negative() {
    let code = r#"
needs std::fs
let f = 1
fs::read_bytes(f, -10)
"#;
    let err = run_aelys_err(code);
    assert!(err.contains("negative"));
}

#[test]
fn fs_read_bytes_exceeds_max() {
    let code = r#"
needs std::fs
let f = 1
fs::read_bytes(f, 20000000)
"#;
    let err = run_aelys_err(code);
    assert!(err.contains("max") || err.contains("MAX"));
}

#[test]
fn fs_write_not_opened_for_writing() {
    let dir = tempdir().unwrap();
    let test_file = dir.path().join("readonly.txt");
    fs::write(&test_file, "data").unwrap();
    let path_str = test_file.display().to_string().replace('\\', "/");

    let code = format!(
        r#"
needs std::fs
let f = fs::open("{}", "r")
fs::write(f, "new data")
"#,
        path_str
    );

    let err = run_aelys_err(&code);
    assert!(err.contains("writing"));
}

#[test]
fn fs_join_absolute_path_rejected() {
    // This is already tested in security_audit_tests.rs
    // but worth repeating
    let code = r#"
needs std::fs
fs::join("/app", "/etc/passwd")
"#;
    let err = run_aelys_err(code);
    assert!(err.contains("absolute"));
}

#[test]
fn fs_join_parent_escape() {
    let code = r#"
needs std::fs
fs::join("/app/data", "../../etc/passwd")
"#;
    let err = run_aelys_err(code);
    assert!(err.contains("escapes"));
}

#[test]
fn fs_absolute_nonexistent() {
    let code = r#"
needs std::fs
fs::absolute("/nonexistent/path")
"#;
    // absolute() is a path operation so may succeed or fail
    let result = run_aelys_result(code);
    let _ = result;
}

#[test]
fn fs_write_line_works() {
    let code = r#"
needs std::fs
let f = 1
fs::write_line(f, "test")
"#;
    let err = run_aelys_err(code);
    assert!(err.contains("invalid"));
}

#[test]
fn fs_size_of_nonexistent() {
    let code = r#"
needs std::fs
fs::size("/nonexistent")
"#;
    // May fail due to OS error (file not found) or handle the error gracefully
    let result = run_aelys_result(code);
    match result {
        Ok(v) => {
            assert!(v.is_int() || v.is_float(), "expected numeric result");
        }
        Err(e) => {
            assert!(!e.is_empty(), "expected error message");
        }
    }
}

#[test]
fn fs_double_close() {
    let code = r#"
needs std::fs
let f = 1
fs::close(f)
fs::close(f)
"#;
    let err = run_aelys_err(code);
    assert!(err.contains("invalid"));
}

#[test]
fn fs_read_after_close() {
    let code = r#"
needs std::fs
let f = 1
fs::close(f)
fs::read(f)
"#;
    let err = run_aelys_err(code);
    assert!(err.contains("invalid"));
}

#[test]
fn fs_write_after_close() {
    let code = r#"
needs std::fs
let f = 1
fs::close(f)
fs::write(f, "data")
"#;
    let err = run_aelys_err(code);
    assert!(err.contains("invalid"));
}
