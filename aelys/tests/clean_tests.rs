mod s1_support;

use aelys::{CompileOptions, ExecutionOutcome, IsolateConfig, RunOptions, Runtime};
use aelys_runtime::parse_vm_args;
use s1_support::{assert_lines_everywhere, compile_error, runtime_error};

mod cleannat {
    use aelys_native::aelys_module;

    #[aelys_module(name = "cleannat", version = "0.1.0")]
    mod exports {
        #[aelys_export]
        pub fn triple(x: i64) -> i64 {
            x * 3
        }

        #[aelys_export]
        pub fn offset(x: i64, by: i64) -> i64 {
            x + by
        }
    }

    pub unsafe fn descriptor() -> &'static aelys_native::AelysModuleDescriptor {
        let pointer = &raw const aelys_module_descriptor_636c65616e6e6174;
        unsafe { &*pointer }
    }
}

fn run_registered(source: &str) -> Result<String, String> {
    let runtime = Runtime::new();
    unsafe { runtime.register_native_module(cleannat::descriptor()) }.unwrap();
    let module = runtime
        .compile(source, CompileOptions::default())
        .map_err(|error| error.to_string())?;
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    match isolate
        .execute(&module, RunOptions::default())
        .map_err(|error| error.to_string())?
    {
        ExecutionOutcome::Returned(value) => Ok(isolate.value_to_string(value)),
        other => Err(format!("expected a value, got {other:?}")),
    }
}

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|arg| arg.to_string()).collect()
}

#[test]
fn jit_sync_turns_the_jit_on() {
    let parsed = parse_vm_args(&args(&["-ae.jit-sync=on"])).expect("valid arguments");
    assert!(parsed.jit && parsed.jit_sync);
    let parsed = parse_vm_args(&args(&["-ae.jit-sync=on", "-ae.jit=on"])).expect("valid");
    assert!(parsed.jit && parsed.jit_sync);
    let parsed = parse_vm_args(&args(&["-ae.jit-sync=off"])).expect("valid arguments");
    assert!(!parsed.jit && !parsed.jit_sync);
    for order in [
        ["-ae.jit=off", "-ae.jit-sync=on"],
        ["-ae.jit-sync=on", "-ae.jit=off"],
    ] {
        let error = parse_vm_args(&args(&order))
            .err()
            .expect("a synchronous jit that is turned off is refused");
        assert!(error.to_string().contains("-ae.jit-sync"), "{error}");
    }
}

#[test]
fn abs_is_a_method_of_every_integer_type_and_of_float() {
    assert_lines_everywhere(
        "println((-3).abs())
let a: int = -9223372036854775807
println(a.abs())
let b: i64 = -9223372036854775807 - 1
println(b.abs())
let c: i32 = -2147483648
println(c.abs())
let d: i8 = -128
println(d.abs())
let e: i16 = -300
println(e.abs())
let f: u8 = 200
println(f.abs())
let g: u64 = 18446744073709551615
println(g.abs())
let h: i32 = 77
println(h.abs())
println((-2.5).abs())
let k: f32 = -1.5
println(k.abs())
",
        &[
            "3",
            "9223372036854775807",
            "-9223372036854775808",
            "-2147483648",
            "-128",
            "300",
            "200",
            "18446744073709551615",
            "77",
            "2.5",
            "1.5",
        ],
    );
}

#[test]
fn abs_in_functions_closures_and_generic_bodies() {
    assert_lines_everywhere(
        "fn dist(a: int, b: int) -> int { (a - b).abs() }
fn wrap(x: i8) -> i8 { x.abs() }
let f = fn(x: i32) -> i32 { x.abs() }
let mut total = 0
for i in -5..5 { total += i.abs() }
println(dist(3, 10))
println(wrap(-100))
println(f(-9))
println(total)
",
        &["7", "100", "9", "25"],
    );
}

#[test]
fn abs_of_the_smallest_int_traps() {
    let error = runtime_error("let m = -9223372036854775807 - 1\nprintln(m.abs())\n");
    assert!(error.contains("overflow"), "{error}");
}

#[test]
fn methods_apply_to_the_result_of_a_generic_call() {
    assert_lines_everywhere(
        "fn gid<T>(x: T) -> T { x }
fn second<A, B>(a: A, b: B) -> B { b }
struct G { v: int }
impl G { fn get(self) -> int { self.v } }
println(gid(-2.5).abs())
println(gid('a').to_upper())
println(gid(G { v: 4 }).get())
println(gid(gid('q')).to_upper())
println(second(1, G { v: 9 }).get())
fn inside() -> char { gid('m').to_upper() }
println(inside())
let v: Vec<int> = vec![1, 2, 3]
println(gid(v).len())
",
        &["2.5", "A", "4", "Q", "9", "M", "3"],
    );
}

#[test]
fn a_non_bool_match_guard_is_named_a_match_guard() {
    let error = compile_error("let x = 3\nlet r = match x { n if n => 1, _ => 2 }\nprintln(r)\n");
    assert!(error.contains("error[E0301]"), "{error}");
    assert!(error.contains("match guard"), "{error}");
    assert!(!error.contains("if condition"), "{error}");
}

#[test]
fn an_ambiguous_import_under_a_method_call_is_reported_as_ambiguous() {
    for source in [
        "use std::fs\nuse std::net\nfn f(b: int) -> bool { return close(b).is_ok() }\n",
        "use std::fs\nuse std::net\nfn f(b: int) -> bool { let x = close(b).is_ok()\n x }\n",
        "use std::fs\nuse std::net\nlet ok = close(1).is_err()\n",
    ] {
        let error = compile_error(source);
        assert!(error.contains("error[E0445]"), "{source}\n{error}");
        assert!(!error.contains("dynamic"), "{source}\n{error}");
    }
}

#[test]
fn string_bytes_is_refused_with_to_bytes_as_the_replacement() {
    for source in [
        "let s = \"ab\"\nprintln(s.bytes().len())\n",
        "fn f(s: string) -> int { s.bytes().len() }\nprintln(f(\"x\"))\n",
    ] {
        let error = compile_error(source);
        assert!(error.contains("error[E0482]"), "{error}");
        assert!(error.contains("s.to_bytes()"), "{error}");
    }
    assert_lines_everywhere(
        "let b = \"A\u{e9}\".to_bytes()\nprintln(b.len())\nprintln(b[1])\n",
        &["3", "195"],
    );
}

#[test]
fn an_avbc_file_of_an_older_format_version_is_refused() {
    let runtime = Runtime::new();
    let module = runtime
        .compile("40 + 2", CompileOptions::default())
        .expect("compile");
    let current = u16::from_le_bytes([module.avbc()[4], module.avbc()[5]]);
    assert_eq!(current, 6);
    for older in [3u16, 4, 5] {
        let mut bytes = module.avbc().to_vec();
        bytes[4..6].copy_from_slice(&older.to_le_bytes());
        let error = aelys_bytecode::asm::deserialize(&bytes)
            .err()
            .expect("an older version is refused");
        assert_eq!(error.to_string(), format!("Unsupported version: {older}"));
    }
}

#[test]
fn a_registered_native_module_honours_every_use_form() {
    assert_eq!(
        run_registered("use cleannat::{triple}\ntriple(14)").unwrap(),
        "42"
    );
    assert_eq!(
        run_registered("use cleannat::{triple, offset}\nfn f() -> int { offset(triple(3), 1) }\nf()")
            .unwrap(),
        "10"
    );
    assert_eq!(
        run_registered("use cleannat as n\nn::offset(n::triple(2), 4)").unwrap(),
        "10"
    );
    assert_eq!(
        run_registered("use cleannat as n\nlet g = fn(x: int) -> int { n::triple(x) }\ng(5)")
            .unwrap(),
        "15"
    );
    assert_eq!(
        run_registered("use cleannat::*\ntriple(offset(1, 1))").unwrap(),
        "6"
    );
    assert_eq!(
        run_registered("use cleannat\ncleannat::triple(7)").unwrap(),
        "21"
    );
    let missing = run_registered("use cleannat::{nope}\n1").unwrap_err();
    assert!(missing.contains("nope"), "{missing}");
    let unimported = run_registered("use cleannat::{offset}\ntriple(1)").unwrap_err();
    assert!(unimported.contains("triple"), "{unimported}");
}

#[test]
fn the_runtime_api_honours_every_use_form_of_the_standard_library() {
    assert_eq!(
        run_registered("use std::math as m\nm::abs(-3) + 1").unwrap(),
        "4"
    );
    assert_eq!(
        run_registered("use std::string::{repeat}\nrepeat(\"ab\", 2)").unwrap(),
        "abab"
    );
}
