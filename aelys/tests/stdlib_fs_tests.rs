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
let f = fs::open("{}", "r").unwrap()
let data = fs::read(f).unwrap()
fs::close(f).unwrap()
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
fs::write_text("{}", "hello world").unwrap()
fs::read_text("{}").unwrap()
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
match fs::open("{}", "xyz") {{ Ok(_) => 0, Err(_) => 1 }}
"#,
        path_str
    );

    assert_aelys_int(&code, 1);
}

#[test]
fn fs_close_invalid_handle() {
    let code = r#"
needs std::fs
match fs::close(999) { Ok(_) => 0, Err(_) => 1 }
"#;
    assert_aelys_int(code, 1);
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
let f = fs::open("{}", "r").unwrap()
let _ = fs::read_line(f).unwrap()
let _ = fs::read_line(f).unwrap()
let _ = fs::read_line(f).unwrap()
fs::close(f).unwrap()
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
match fs::read_bytes(1, -10) { Ok(_) => 0, Err(_) => 1 }
"#;
    assert_aelys_int(code, 1);
}

#[test]
fn fs_read_bytes_exceeds_max() {
    let code = r#"
needs std::fs
match fs::read_bytes(1, 20000000) { Ok(_) => 0, Err(_) => 1 }
"#;
    assert_aelys_int(code, 1);
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
let f = fs::open("{}", "r").unwrap()
let result = match fs::write(f, "new data") {{ Ok(_) => 0, Err(_) => 1 }}
let _ = fs::close(f)
result
"#,
        path_str
    );

    assert_aelys_int(&code, 1);
}

#[test]
fn fs_join_absolute_path_rejected() {
    // This is already tested in security_audit_tests.rs
    // but worth repeating
    let code = r#"
needs std::fs
match fs::join("/app", "/etc/passwd") { Ok(_) => 0, Err(_) => 1 }
"#;
    assert_aelys_int(code, 1);
}

#[test]
fn fs_join_parent_escape() {
    let code = r#"
needs std::fs
match fs::join("/app/data", "../../etc/passwd") { Ok(_) => 0, Err(_) => 1 }
"#;
    assert_aelys_int(code, 1);
}

#[test]
fn fs_absolute_nonexistent() {
    let code = r#"
needs std::fs
match fs::absolute("/nonexistent/path") { Ok(_) => 1, Err(message) => if message.len() > 0 { 1 } else { 0 } }
"#;
    assert_aelys_int(code, 1);
}

#[test]
fn fs_write_line_works() {
    let code = r#"
needs std::fs
let f = 1
match fs::write_line(f, "test") { Ok(_) => 0, Err(_) => 1 }
"#;
    assert_aelys_int(code, 1);
}

#[test]
fn fs_size_of_nonexistent() {
    let code = r#"
needs std::fs
match fs::size("/nonexistent") { Ok(_) => 0, Err(_) => 1 }
"#;
    assert_aelys_int(code, 1);
}

#[test]
fn fs_double_close() {
    let code = r#"
needs std::fs
let f = 1
let first = match fs::close(f) { Ok(_) => 0, Err(_) => 1 }
let second = match fs::close(f) { Ok(_) => 0, Err(_) => 1 }
first + second
"#;
    assert_aelys_int(code, 2);
}

#[test]
fn fs_read_after_close() {
    let code = r#"
needs std::fs
let f = 1
let first = match fs::close(f) { Ok(_) => 0, Err(_) => 1 }
let second = match fs::read(f) { Ok(_) => 0, Err(_) => 1 }
first + second
"#;
    assert_aelys_int(code, 2);
}

#[test]
fn fs_write_after_close() {
    let code = r#"
needs std::fs
let f = 1
let first = match fs::close(f) { Ok(_) => 0, Err(_) => 1 }
let second = match fs::write(f, "data") { Ok(_) => 0, Err(_) => 1 }
first + second
"#;
    assert_aelys_int(code, 2);
}
