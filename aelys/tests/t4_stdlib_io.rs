use aelys_driver::run_file;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, body).unwrap();
    path
}

fn run_int(dir: &Path, body: &str) -> i64 {
    let path = script(dir, "main.aelys", body);
    let value = run_file(&path).unwrap_or_else(|error| panic!("{error}\n--- source\n{body}"));
    value
        .as_int()
        .unwrap_or_else(|| panic!("expected an int, got {value:?}"))
}

fn run_error(dir: &Path, body: &str) -> String {
    let path = script(dir, "main.aelys", body);
    match run_file(&path) {
        Ok(value) => panic!("expected a rejection, got {value:?}\n--- source\n{body}"),
        Err(error) => error.to_string(),
    }
}

fn quoted(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[test]
fn elapsed_us_is_an_int_that_mixes_with_float_arithmetic() {
    let dir = tempfile::tempdir().unwrap();
    let probe = "let t = time::timer()\n\
                 let mut i = 0\n\
                 while i < 1000 { i += 1 }\n\
                 let us = time::elapsed_us(t)\n\
                 let a = us / 1000.0\n\
                 let b = us - 0.0\n\
                 let c = us * 2.0\n\
                 let ok = a * 1000.0 == b && c == b + b && b >= 0.0 && b < 60000000.0\n";
    let forms = [
        format!("{probe}if ok {{ 1 }} else {{ 0 }}\n"),
        format!("fn probe() -> bool {{\n{probe}return ok\n}}\nif probe() {{ 1 }} else {{ 0 }}\n"),
        format!(
            "let f = fn() -> bool {{\n{probe}return ok\n}}\nif f() {{ 1 }} else {{ 0 }}\n"
        ),
        "let t = time::timer()\nlet us: int = time::elapsed_us(t)\nlet ms: int = time::elapsed_ms(t)\nif us >= ms * 1000 { 1 } else { 0 }\n".to_string(),
        "fn us_of(t: int) -> int { return time::elapsed_us(t) }\nlet t = time::timer()\nif us_of(t) + 1 > 0 { 1 } else { 0 }\n".to_string(),
        "needs elapsed_us, timer from std::time\nlet t = timer()\nlet x = elapsed_us(t) + 0.5\nif x > 0.0 && x < 60000000.0 { 1 } else { 0 }\n".to_string(),
    ];
    for form in forms {
        assert_eq!(run_int(dir.path(), &form), 1, "{form}");
    }
}

#[test]
fn read_bytes_after_read_line_continues_from_the_line_reader() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("mixed.txt");
    fs::write(&data, "line1\nABCDEF").unwrap();
    let path = quoted(&data);
    let body = format!(
        "needs std::fs\n\
         needs std::bytes as bytes\n\
         let f = match fs::open(\"{path}\", \"r\") {{ Ok(h) => h, Err(e) => -1 }}\n\
         let l = match fs::read_line(f) {{ Ok(Some(s)) => s, Ok(None) => \"<eof>\", Err(e) => e }}\n\
         let b = match fs::read_bytes(f, 3) {{ Ok(h) => h, Err(e) => -1 }}\n\
         let rest = match fs::read_bytes(f, 100) {{ Ok(h) => h, Err(e) => -1 }}\n\
         let mut code = 0\n\
         if l == \"line1\" {{ code += 1 }}\n\
         if bytes::size(b) == 3 && bytes::decode(b, 0, 3) == \"ABC\" {{ code += 10 }}\n\
         if bytes::size(rest) == 3 && bytes::decode(rest, 0, 3) == \"DEF\" {{ code += 100 }}\n\
         code\n"
    );
    assert_eq!(run_int(dir.path(), &body), 111);
}

#[test]
fn read_after_read_line_returns_the_rest_of_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("rest.txt");
    fs::write(&data, "head\nbody one\nbody two\n").unwrap();
    let path = quoted(&data);
    let body = format!(
        "needs std::fs\n\
         let f = match fs::open(\"{path}\", \"r\") {{ Ok(h) => h, Err(e) => -1 }}\n\
         let l = match fs::read_line(f) {{ Ok(Some(s)) => s, Ok(None) => \"\", Err(e) => e }}\n\
         let r = match fs::read(f) {{ Ok(s) => s, Err(e) => e }}\n\
         let a = match fs::read_all(f) {{ Ok(s) => s, Err(e) => e }}\n\
         let mut code = 0\n\
         if l == \"head\" {{ code += 1 }}\n\
         if r == \"body one\\nbody two\\n\" {{ code += 10 }}\n\
         if a == \"\" {{ code += 100 }}\n\
         code\n"
    );
    assert_eq!(run_int(dir.path(), &body), 111);
}

#[cfg(unix)]
#[test]
fn read_bytes_retries_short_reads_until_the_count_or_end_of_file() {
    let dir = tempfile::tempdir().unwrap();
    let fifo = dir.path().join("pipe");
    let status = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap();
    assert!(status.success());
    let writer_path = fifo.clone();
    let writer = std::thread::spawn(move || {
        let mut pipe = fs::OpenOptions::new()
            .write(true)
            .open(&writer_path)
            .unwrap();
        pipe.write_all(b"ABC").unwrap();
        pipe.flush().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(150));
        pipe.write_all(b"DEFGH").unwrap();
    });
    let path = quoted(&fifo);
    let body = format!(
        "needs std::fs\n\
         needs std::bytes as bytes\n\
         let f = match fs::open(\"{path}\", \"r\") {{ Ok(h) => h, Err(e) => -1 }}\n\
         let b = match fs::read_bytes(f, 6) {{ Ok(h) => h, Err(e) => -1 }}\n\
         let tail = match fs::read_bytes(f, 10) {{ Ok(h) => h, Err(e) => -1 }}\n\
         let mut code = bytes::size(b) * 100 + bytes::size(tail)\n\
         if bytes::decode(b, 0, bytes::size(b)) == \"ABCDEF\" {{ code += 10000 }}\n\
         code\n"
    );
    let result = run_int(dir.path(), &body);
    writer.join().unwrap();
    assert_eq!(result, 10000 + 600 + 2);
}

#[test]
fn read_file_bytes_reads_a_whole_file_past_the_read_bytes_cap() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("big.bin");
    let length: usize = 17 * 1024 * 1024 + 3;
    let content: Vec<u8> = (0..length).map(|i| (i * 7 % 251) as u8).collect();
    fs::write(&data, &content).unwrap();
    let path = quoted(&data);
    let probes = [0usize, 1, 16 * 1024 * 1024 + 5, length - 1];
    let mut expected = 0i64;
    for offset in probes {
        expected = expected * 251 + i64::from(content[offset]);
    }
    let body = format!(
        "needs std::fs\n\
         needs std::bytes as bytes\n\
         let b = match fs::read_file_bytes(\"{path}\") {{ Ok(h) => h, Err(e) => -1 }}\n\
         let mut acc = 0\n\
         for o in [{}] {{ acc = acc * 251 + bytes::read_u8(b, o) }}\n\
         if bytes::size(b) == {length} {{ acc }} else {{ -bytes::size(b) }}\n",
        probes.map(|p| p.to_string()).join(", ")
    );
    assert_eq!(run_int(dir.path(), &body), expected);
}

#[test]
fn read_file_bytes_reports_a_missing_file_and_reads_an_empty_one() {
    let dir = tempfile::tempdir().unwrap();
    let empty = dir.path().join("empty.bin");
    fs::write(&empty, b"").unwrap();
    let missing = quoted(&dir.path().join("missing.bin"));
    let empty = quoted(&empty);
    let body = format!(
        "needs std::fs\n\
         needs std::bytes as bytes\n\
         needs std::string as string\n\
         let a = match fs::read_file_bytes(\"{missing}\") {{\n\
             Ok(h) => 1000,\n\
             Err(e) => if string::contains(e, \"missing.bin\") {{ 1 }} else {{ 3000 }},\n\
         }}\n\
         let b = match fs::read_file_bytes(\"{empty}\") {{\n\
             Ok(h) => if bytes::size(h) == 0 {{ 10 }} else {{ 4000 }},\n\
             Err(e) => 2000,\n\
         }}\n\
         a + b\n"
    );
    assert_eq!(run_int(dir.path(), &body), 11);
}

#[test]
fn two_whole_standard_modules_sharing_a_name_resolve_qualified_uses() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("five.txt");
    fs::write(&file, "12345").unwrap();
    let path = quoted(&file);
    let copy_target = quoted(&dir.path().join("copy.txt"));
    let core = format!(
        "let b = bytes::alloc(4)\n\
         let c = bytes::alloc(4)\n\
         bytes::write_u8(b, 1, 9)\n\
         bytes::copy(b, 0, c, 0, 4)\n\
         let fsize = match fs::size(\"{path}\") {{ Ok(n) => n, Err(e) => -1 }}\n\
         let copied = match fs::copy(\"{path}\", \"{copy_target}\") {{ Ok(n) => n, Err(e) => -1 }}\n\
         let r = bytes::size(b) * 1000 + fsize * 100 + copied * 10 + bytes::read_u8(c, 1)\n"
    );
    let expected = 4 * 1000 + 5 * 100 + 5 * 10 + 9;
    for header in [
        "needs std::fs\nneeds std::bytes\n",
        "needs std::bytes\nneeds std::fs\n",
        "needs std::fs\nneeds std::bytes\nneeds std::string\n",
    ] {
        let top = format!("{header}{core}r\n");
        assert_eq!(run_int(dir.path(), &top), expected, "{top}");
        let in_fn = format!("{header}fn go() -> int {{\n{core}return r\n}}\ngo()\n");
        assert_eq!(run_int(dir.path(), &in_fn), expected, "{in_fn}");
        let in_closure = format!("{header}let go = fn() -> int {{\n{core}return r\n}}\ngo()\n");
        assert_eq!(run_int(dir.path(), &in_closure), expected, "{in_closure}");
    }
}

#[test]
fn a_bare_use_of_a_name_two_whole_modules_export_is_e0445() {
    let dir = tempfile::tempdir().unwrap();
    for (body, name) in [
        (
            "needs std::fs\nneeds std::bytes\nlet b = bytes::alloc(4)\nsize(b)\n",
            "size",
        ),
        (
            "needs std::bytes\nneeds std::fs\nlet b = bytes::alloc(4)\nsize(b)\n",
            "size",
        ),
        (
            "needs std::fs\nneeds std::bytes\nfn f(b: int) -> int {\n    copy(b, 0, b, 0, 1)\n    return 1\n}\nf(1)\n",
            "copy",
        ),
        (
            "needs std::fs\nneeds std::bytes\nlet g = fn(b: int) -> int { return size(b) }\ng(1)\n",
            "size",
        ),
        ("needs std::fs\nneeds std::bytes\nlet s = size\n0\n", "size"),
    ] {
        let message = run_error(dir.path(), body);
        assert!(message.contains("E0445"), "{body}\n{message}");
        assert!(
            message.contains(&format!("symbol '{name}' is exported by multiple modules")),
            "{message}"
        );
        assert!(
            message.contains(&format!("'bytes::{name}' or 'fs::{name}'")),
            "{message}"
        );
    }
}

#[test]
fn a_local_declaration_shadows_a_name_two_whole_modules_export() {
    let dir = tempfile::tempdir().unwrap();
    let body = "needs std::fs\nneeds std::bytes\nfn size(x: int) -> int { return x * 2 }\nsize(21) + bytes::size(bytes::alloc(5))\n";
    assert_eq!(run_int(dir.path(), body), 47);
}

#[test]
fn two_whole_user_modules_sharing_a_function_name_resolve_qualified_uses() {
    let dir = tempfile::tempdir().unwrap();
    script(
        dir.path(),
        "alpha.aelys",
        "pub fn get() -> int {\n    return 100\n}\npub fn only_alpha() -> int {\n    return 1\n}\n",
    );
    script(
        dir.path(),
        "beta.aelys",
        "pub fn get() -> int {\n    return 500\n}\n",
    );
    assert_eq!(
        run_int(
            dir.path(),
            "needs alpha\nneeds beta\nalpha::get() * 1000 + beta::get() + only_alpha()\n"
        ),
        100_501
    );
    let message = run_error(dir.path(), "needs alpha\nneeds beta\nget()\n");
    assert!(message.contains("E0445"), "{message}");
    assert!(message.contains("'alpha::get' or 'beta::get'"), "{message}");
}
