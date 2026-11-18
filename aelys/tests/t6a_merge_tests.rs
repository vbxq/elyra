use aelys::{
    CompileOptions, ExecutionOutcome, IsolateConfig, JitMode, RunOptions, Runtime, StructuredValue,
    new_vm_with_config, run_with_vm_and_opt,
};
use aelys_driver::run_file;
use aelys_opt::OptimizationLevel;
use aelys_runtime::VmConfig;
use std::path::PathBuf;
use tempfile::TempDir;

const LEVELS: [OptimizationLevel; 3] = [
    OptimizationLevel::None,
    OptimizationLevel::Standard,
    OptimizationLevel::Aggressive,
];

const UNINIT: &str = "read before its initialiser ran";

fn general_loop() -> RunOptions {
    RunOptions {
        max_instructions: Some(1_000_000_000_000),
        ..RunOptions::default()
    }
}

fn modes() -> [(&'static str, JitMode, RunOptions); 4] {
    [
        ("hot loop", JitMode::Off, RunOptions::default()),
        ("general loop", JitMode::Off, general_loop()),
        ("jit", JitMode::Tiered, RunOptions::default()),
        ("jit, general loop", JitMode::Tiered, general_loop()),
    ]
}

fn run(
    source: &str,
    level: OptimizationLevel,
    jit: JitMode,
    options: RunOptions,
) -> Result<StructuredValue, String> {
    let runtime = Runtime::with_jit_mode(jit);
    let module = runtime
        .compile(
            source,
            CompileOptions {
                optimization_level: level,
                ..CompileOptions::default()
            },
        )
        .map_err(|error| error.to_string())?;
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    match isolate
        .execute(&module, options)
        .map_err(|error| error.to_string())?
    {
        ExecutionOutcome::Returned(value) => isolate
            .structured_clone(value)
            .map_err(|error| format!("{error:?}")),
        _ => Err("program exited".to_string()),
    }
}

fn error_line(message: &str) -> Option<u32> {
    message
        .lines()
        .find_map(|line| line.trim().strip_prefix("--> "))
        .and_then(|location| location.rsplit(':').nth(1))
        .and_then(|line| line.parse().ok())
}

fn assert_int_everywhere(source: &str, expected: i64) {
    for level in LEVELS {
        for (name, jit, options) in modes() {
            match run(source, level, jit, options) {
                Ok(StructuredValue::Int(got)) => {
                    assert_eq!(got, expected, "{level:?} {name}:\n{source}");
                }
                other => panic!("{level:?} {name}: expected {expected}, got {other:?}\n{source}"),
            }
        }
    }
}

fn assert_error_everywhere(source: &str, needle: &str, line: u32) {
    for level in LEVELS {
        for (name, jit, options) in modes() {
            match run(source, level, jit, options) {
                Ok(value) => panic!("{level:?} {name}: expected an error, got {value:?}\n{source}"),
                Err(text) => {
                    assert!(text.contains(needle), "{level:?} {name}: {text}");
                    assert!(!text.contains("[E0"), "a compile error:\n{text}");
                    assert_eq!(error_line(&text), Some(line), "{level:?} {name}: {text}");
                }
            }
        }
    }
}

fn write(dir: &TempDir, name: &str, content: &str) -> PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, content).expect("write source");
    path
}

fn gc_configs() -> Vec<(&'static str, VmConfig)> {
    vec![
        (
            "stress",
            VmConfig {
                gc_stress: true,
                gc_nursery_bytes: VmConfig::MIN_GC_NURSERY_BYTES,
                ..VmConfig::default()
            },
        ),
        (
            "verify",
            VmConfig {
                gc_verify: true,
                gc_nursery_bytes: 4 * 1024,
                ..VmConfig::default()
            },
        ),
        (
            "verify, stress",
            VmConfig {
                gc_verify: true,
                gc_stress: true,
                gc_nursery_bytes: VmConfig::MIN_GC_NURSERY_BYTES,
                ..VmConfig::default()
            },
        ),
        ("default", VmConfig::default()),
    ]
}

fn assert_int_under_gc_pressure(source: &str, expected: i64) {
    for level in LEVELS {
        for (name, config) in gc_configs() {
            let mut vm = new_vm_with_config(config, Vec::new()).expect("vm");
            let value = run_with_vm_and_opt(&mut vm, source, "<t6a-merge>", level)
                .unwrap_or_else(|error| panic!("{level:?} {name}: {error}"));
            assert_eq!(value.as_int(), Some(expected), "{level:?} {name}");
            if name != "default" {
                assert!(
                    vm.heap().minor_collection_count() > 20,
                    "{level:?} {name}: only {} minor collections",
                    vm.heap().minor_collection_count()
                );
            }
        }
    }
}

#[test]
fn a_global_read_in_the_hot_loop_before_its_initialiser_is_an_error() {
    let source = "fn total(n: int) -> int {
    let mut s = 0
    for i in 0..n {
        s = s + i * 2
    }
    s + later
}
let first = total(10)
let later = 5
first
";
    assert_error_everywhere(source, &format!("global `later` {UNINIT}"), 6);
    assert_int_everywhere(
        "fn total(n: int) -> int {
    let mut s = 0
    for i in 0..n {
        s = s + i * 2
    }
    s + later
}
let later = 5
total(10)
",
        95,
    );
}

#[test]
fn a_module_function_reading_a_later_module_variable_through_its_prepared_view_is_an_error() {
    let dir = TempDir::new().expect("temp dir");
    write(
        &dir,
        "m.aelys",
        "pub fn read_later() -> int {
    let mut s = 0
    for i in 0..4 {
        s = s + i
    }
    s + later
}
pub let early = read_later()
pub let later = 7
",
    );
    let main = write(&dir, "main.aelys", "needs m\nm::early\n");
    let error = run_file(&main).expect_err("the module reads `later` before it is set");
    let text = error.to_string();
    assert!(text.contains(&format!("global `later` {UNINIT}")), "{text}");
    assert_eq!(error_line(&text), Some(6), "{text}");

    write(
        &dir,
        "m.aelys",
        "pub fn read_later() -> int {
    let mut s = 0
    for i in 0..4 {
        s = s + i
    }
    s + later
}
pub let later = 7
pub let early = read_later()
",
    );
    let value = run_file(&main).expect("runs");
    assert_eq!(value.as_int(), Some(13));
}

#[test]
fn errors_after_hot_code_keep_their_own_lines() {
    let prefix = "fn run() -> int {
    let mut s = 0
    for i in 0..5 {
        s = s + i
    }
    s = s - 20
";
    let tail = "let x = run()
let later_v = 3
let later_f: fn() -> int = fn() -> int { 1 }
let mut later_m = 0
x
";
    let cases: [(&str, &str, u32); 9] = [
        ("    let q = later_v + s\n", "global `later_v`", 7),
        ("    let q = later_f()\n", "global `later_f`", 7),
        (
            "    later_m = later_m + s\n    let q = 1\n",
            "global `later_m`",
            7,
        ),
        (
            "    assert(s == 1, \"nope\")\n    let q = 1\n",
            "assertion failed: nope",
            7,
        ),
        (
            "    let a: i8 = 5\n    let z: i8 = (s + 10) as i8\n    let q8: i8 = a / z\n    let q = q8 as int\n",
            "division by zero",
            9,
        ),
        (
            "    let big = 100000000000\n    let q = big * big * s\n",
            "integer overflow",
            8,
        ),
        (
            "    let v = vec![1, 2]\n    let q = v[s + 100]\n",
            "out of bounds",
            8,
        ),
        (
            "    let t = \"x\" + s.to_string()\n    let q = t as int\n",
            "expected integer",
            8,
        ),
        (
            "    let mut m: Map<int, int> = Map::new()\n    let q = m[s]\n",
            "map key not found",
            8,
        ),
    ];
    for (body, needle, line) in cases {
        let source = format!("{prefix}{body}    return s + q\n}}\n{tail}");
        assert_error_everywhere(&source, needle, line);
    }
    let unwrap = format!(
        "fn e() -> Result<int, string> {{ Err(\"bad\") }}\n{prefix}    let q = e().unwrap()\n    return s + q\n}}\n{tail}"
    );
    assert_error_everywhere(&unwrap, "unwrap failed on Err: bad", 8);
}

#[test]
fn sized_arithmetic_and_small_literal_masks_wrap_at_their_width_in_the_hot_loop() {
    assert_int_everywhere(
        "fn run() -> int {
    let mut x: u8 = 7
    let mut y: i8 = -100
    let mut z: i16 = 300
    let mut w: u32 = 4000000000
    let mut d: i32 = -2147483000
    let mut acc = 0
    for i in 0..50 {
        x = x * 13 + 7
        y = y - 29
        z = z << 3
        w = w + 1234567891
        d = d / 3 - 7
        let m: u8 = (x >> 1) & 0x7f
        let n: u8 = x ^ 255
        let o: i8 = -y
        let p: u8 = ~x
        let q: i8 = (y ^ 127) | 3
        let r: u8 = (x | 128) ^ 85
        let t: u8 = x << 3
        let u = ((acc * 3) >> 2) & 255
        acc = (acc * 31 + (x as int) + (y as int) * 3 + (z as int) * 5 + (w as int) % 1000 + (d as int) % 977 + (m as int) + (n as int) + (o as int) + (p as int) + (q as int) * 7 + (r as int) * 11 + (t as int) * 13 + u) % 1000000007
    }
    return acc
}
run()
",
        709_830_417,
    );
}

#[test]
fn a_labeled_while_loop_keeps_its_hoisted_wide_literals() {
    assert_int_everywhere(
        "fn run() -> int {
    let mut i = 0
    let mut acc = 0
    'outer: while i < 100000 {
        acc = (acc * 100003 + i * 700001) % 1000000007
        let mut j = 0
        while j < 3 {
            acc = (acc + j * 900001) % 1000000007
            if acc % 100003 == 17 {
                break 'outer
            }
            j = j + 1
        }
        i = i + 1
    }
    acc * 1000 + i % 1000
}
run()
",
        129_203_893_218,
    );
}

#[test]
fn a_map_element_assignment_statement_stores_its_value() {
    assert_int_everywhere(
        "fn run() -> int {
    let mut m: Map<int, int> = Map::new()
    let mut names: Map<int, string> = Map::new()
    for i in 0..500 {
        m[i % 37] = i * 3
        names[i % 5] = \"n{i}\"
    }
    let mut s = 0
    for k in 0..37 {
        s = s * 7 % 1000000007 + m[k]
    }
    s * 100 + names[4].len()
}
run()
",
        {
            let mut values = [0i64; 37];
            for i in 0..500i64 {
                values[(i % 37) as usize] = i * 3;
            }
            let mut s = 0i64;
            for value in values {
                s = s * 7 % 1_000_000_007 + value;
            }
            s * 100 + "n499".len() as i64
        },
    );
}

#[test]
fn a_bare_imported_variable_written_on_both_sides_is_read_live_by_hot_code() {
    let dir = TempDir::new().expect("temp dir");
    write(
        &dir,
        "cnt.aelys",
        "pub let mut counter = 0
pub fn bump(n: int) {
    for i in 0..n {
        counter = counter + 1
    }
}
",
    );
    let main = write(
        &dir,
        "main.aelys",
        "needs cnt
fn go() -> int {
    let mut s = 0
    for k in 0..10 {
        cnt::bump(k)
        s = s * 3 + cnt::counter + counter
        if k % 3 == 0 {
            counter = counter + 100
        }
    }
    s * 1000 + counter
}
go()
",
    );
    let mut s = 0i64;
    let mut counter = 0i64;
    for k in 0..10 {
        counter += k;
        s = s * 3 + 2 * counter;
        if k % 3 == 0 {
            counter += 100;
        }
    }
    let value = run_file(&main).expect("runs");
    assert_eq!(value.as_int(), Some(s * 1000 + counter));
}

#[test]
fn stores_into_heap_objects_from_hot_code_survive_young_and_checked_collections() {
    let names: Vec<String> = (0..199)
        .map(|i| format!("r29_{i}"))
        .chain(std::iter::once("g49".to_string()))
        .collect();
    let hash = |text: &str, mut acc: i64| {
        for byte in text.bytes() {
            acc = (acc * 31 + i64::from(byte)) % 1_000_000_007;
        }
        acc
    };
    let mut acc = 7i64;
    for name in &names {
        acc = hash(name, acc);
    }
    for j in 0..40i64 {
        acc = hash(&format!("r29_{}", j * 5), acc);
        acc = (acc + 31 * j) % 1_000_000_007;
    }
    for i in 0..200i64 {
        acc = (acc * 7 + i + i * 435) % 1_000_000_007;
    }
    for j in 0..40i64 {
        acc = hash(&format!("r29_{}", j * 5), acc);
    }
    let expected = (acc + 30 * 1000 + "r29_29".len() as i64) % 1_000_000_007;
    assert_int_under_gc_pressure(
        r#"
struct Cell { name: string, n: int }
fn make_keeper() -> fn(string) -> int {
    let mut held = "start"
    let mut count = 0
    return fn(s: string) -> int {
        held = s
        count = count + 1
        return count * 1000 + held.len()
    }
}
fn h(s: string, acc: int) -> int {
    let mut x = acc
    for c in s.chars() { x = (x * 31 + (c as int)) % 1000000007 }
    x
}
fn run() -> int {
    let mut names: Vec<string> = vec![]
    for i in 0..200 { names.push("n{i}") }
    let mut cells: Vec<Cell> = vec![]
    for i in 0..40 { cells.push(Cell { name: "c", n: i }) }
    let mut nums: Vec<int> = vec![]
    for i in 0..200 { nums.push(i) }
    let mut tags: Map<int, string> = Map::new()
    let keep = make_keeper()
    let mut k = 0
    for round in 0..30 {
        for i in 0..200 {
            names[i] = "r{round}_{i}"
            nums[i] = nums[i] + round * i
        }
        for j in 0..40 {
            cells[j].name = names[j * 5]
            cells[j].n = cells[j].n + j
            tags[j] = cells[j].name
        }
        k = keep(names[round])
        let mut grow: Vec<string> = vec![]
        for i in 0..50 { grow.push("g{i}") }
        names[199] = grow[49]
    }
    let mut acc = 7
    for i in 0..200 { acc = h(names[i], acc) }
    for j in 0..40 {
        acc = h(cells[j].name, acc)
        acc = (acc + cells[j].n) % 1000000007
    }
    for i in 0..200 { acc = (acc * 7 + nums[i]) % 1000000007 }
    for j in 0..40 { acc = h(tags[j], acc) }
    (acc + k) % 1000000007
}
run()
"#,
        expected,
    );
}

#[test]
fn values_held_only_by_a_native_or_a_closed_capture_survive_collections() {
    let hash = |text: &str, mut acc: i64| {
        for byte in text.bytes() {
            acc = (acc * 31 + i64::from(byte)) % 1_000_000_007;
        }
        acc
    };
    let mut held = "start".to_string();
    let mut count = 0i64;
    let mut keep = |text: String| {
        let before = held.len() as i64 * 100 + count;
        held = text;
        count += 1;
        before
    };
    let (mut k, mut acc) = (0i64, 7i64);
    for round in 0..30 {
        k = (k * 7 + keep(format!("k{round}_{}", k % 10))) % 1_000_000_007;
        for i in 0..20 {
            acc = hash(&format!("p{round}_{}", 19 - i), acc);
            acc = hash(&format!("m{round}_{i}"), acc);
        }
    }
    let expected = (acc * 31 + k + keep("end".to_string())) % 1_000_000_007;
    assert_int_under_gc_pressure(
        r#"
fn h(s: string, acc: int) -> int {
    let mut x = acc
    for c in s.chars() { x = (x * 31 + (c as int)) % 1000000007 }
    x
}
fn make_keeper() -> fn(string) -> int {
    let mut held = "start"
    let mut count = 0
    return fn(s: string) -> int {
        let before = held.len() * 100 + count
        held = s
        count = count + 1
        return before
    }
}
fn run() -> int {
    let keep = make_keeper()
    let mut k = 0
    let mut acc = 7
    let mut stack: Vec<string> = vec![]
    let mut m: Map<int, string> = Map::new()
    for round in 0..30 {
        k = (k * 7 + keep("k{round}_{k % 10}")) % 1000000007
        for i in 0..20 {
            stack.push("p{round}_{i}")
            m[i] = "m{round}_{i}"
        }
        for i in 0..20 {
            let top = stack.pop()
            acc = match top { Some(s) => h(s, acc), None => acc + 1 }
            let gone = m.remove(i)
            acc = match gone { Some(s) => h(s, acc), None => acc + 3 }
        }
    }
    (acc * 31 + k + keep("end")) % 1000000007
}
run()
"#,
        expected,
    );
}

#[test]
fn discarded_assignments_to_registers_past_255_and_around_calls_keep_their_values() {
    let mut source = String::from("fn g(a: int, b: int) -> int { a * 2 + b }\nfn f() -> int {\n");
    for i in 0..300 {
        source.push_str(&format!("    let mut v{i} = {i}\n"));
    }
    source.push_str(
        "    for k in 0..50 {
        v299 = v1 + v298 * 3 % 1000
        v2 = v299 & 255
        v298 = g(v297, v2) % 10007
        v0 = v298 >> 3
        v297 = v0 ^ 77
    }
    v0 + v2 + v297 + v298 + v299
}
f()
",
    );
    let mut v: Vec<i64> = (0..300).collect();
    for _ in 0..50 {
        v[299] = v[1] + v[298] * 3 % 1000;
        v[2] = v[299] & 255;
        v[298] = (v[297] * 2 + v[2]) % 10007;
        v[0] = v[298] >> 3;
        v[297] = v[0] ^ 77;
    }
    assert_int_everywhere(&source, v[0] + v[2] + v[297] + v[298] + v[299]);
}
