mod s1_support;

use s1_support::run_ok;

// the program reads its own resident set from /proc/self/statm before and after the allocation
#[test]
fn fifty_million_bytes_take_about_fifty_megabytes_of_memory() {
    let source = r#"use std::fs
fn resident_mb() -> int {
    let fields = fs::read_text("/proc/self/statm").unwrap().split(" ")
    fields[1].to_int().unwrap() * 4096 / 1048576
}
let before = resident_mb()
let mut v: Vec<u8> = vec![0; 50000000]
v[0] = 1
v[49999999] = 255
let mut touched = 0
let mut i = 0
while i < v.len() {
    v[i] = 3
    touched += 1
    i += 4096
}
let after = resident_mb()
println("{v.len()} {v[0]} {v[49999999]} {touched}")
println(before)
println(after)
"#;
    let lines = run_ok(source, &[]);
    assert_eq!(lines[0], "50000000 3 255 12208");
    let before: u64 = lines[1].parse().expect("resident size before");
    let after: u64 = lines[2].parse().expect("resident size after");
    assert!(
        after < 80 && after - before < 60,
        "resident {before} MB before, {after} MB after"
    );
}
