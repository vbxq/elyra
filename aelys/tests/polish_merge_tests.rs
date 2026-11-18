use aelys::{
    CompileOptions, ExecutionOutcome, IsolateConfig, JitMode, RunOptions, Runtime, StructuredValue,
};
use aelys_opt::OptimizationLevel;

const LEVELS: [OptimizationLevel; 3] = [
    OptimizationLevel::None,
    OptimizationLevel::Standard,
    OptimizationLevel::Aggressive,
];

const JITS: [JitMode; 3] = [JitMode::Off, JitMode::Baseline, JitMode::Tiered];

fn value_at(
    source: &str,
    level: OptimizationLevel,
    jit: JitMode,
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
        .execute(&module, RunOptions::default())
        .map_err(|error| error.to_string())?
    {
        ExecutionOutcome::Returned(value) => isolate
            .structured_clone(value)
            .map_err(|error| format!("{error:?}")),
        _ => Err("program exited".to_string()),
    }
}

fn int_everywhere(source: &str, expected: i64) {
    for level in LEVELS {
        for jit in JITS {
            match value_at(source, level, jit) {
                Ok(StructuredValue::Int(got)) => {
                    assert_eq!(got, expected, "{level:?} {jit:?}:\n{source}");
                }
                other => panic!("{level:?} {jit:?}: expected {expected}, got {other:?}\n{source}"),
            }
        }
    }
}

fn string_everywhere(source: &str, expected: &str) {
    for level in LEVELS {
        for jit in JITS {
            match value_at(source, level, jit) {
                Ok(StructuredValue::String(got)) => {
                    assert_eq!(got, expected, "{level:?} {jit:?}:\n{source}");
                }
                other => panic!("{level:?} {jit:?}: expected {expected}, got {other:?}\n{source}"),
            }
        }
    }
}

fn rejected(source: &str, code: &str) {
    for level in LEVELS {
        match value_at(source, level, JitMode::Off) {
            Ok(value) => panic!("{level:?}: expected {code}, got {value:?}\n{source}"),
            Err(text) => assert!(
                text.contains(code),
                "{level:?}: expected {code}, got:\n{text}"
            ),
        }
    }
}

fn script_value(source: &str, level: OptimizationLevel) -> aelys_runtime::Value {
    let mut vm = aelys::new_vm().expect("a vm starts");
    aelys::run_with_vm_and_opt(&mut vm, source, "<polish>", level)
        .unwrap_or_else(|error| panic!("{level:?}: {error}\n{source}"))
}

fn accepted(source: &str) {
    script_value(source, OptimizationLevel::None);
}

fn script_int_at_every_level(source: &str, expected: i64) {
    for level in LEVELS {
        let value = script_value(source, level);
        assert_eq!(value.as_int(), Some(expected), "{level:?}:\n{source}");
    }
}

fn script_unit_at_every_level(source: &str) {
    for level in LEVELS {
        let value = script_value(source, level);
        assert!(value.is_unit(), "{level:?}: got {value:?}\n{source}");
    }
}

const NAMED: &str = r#"
trait Named { fn to_string(self) -> string; }
struct Q { v: int }
impl Named for Q { fn to_string(self) -> string { "q{self.v}" } }
"#;

#[test]
fn an_inherent_to_string_method_runs_instead_of_the_display_check() {
    string_everywhere(
        r#"
struct U { v: int }
impl U { fn to_string(self) -> string { "s{self.v}" } }
enum N { A(int) }
impl N { fn to_string(self) -> string { match self { N::A(k) => "n{k}" } } }
struct W<T> { v: T }
impl<T> W<T> { fn to_string(self) -> string { "w" } }
fn mk() -> U { U { v: 4 } }
fn main() -> string {
    let u = U { v: 7 }
    let xs = vec![mk(), mk()]
    let o = Some(mk())
    let t = match o { Some(x) => x.to_string(), None => "" }
    let mut acc = 0
    for i in 0..300 { acc += U { v: i }.to_string().len() }
    "{u.to_string()} {N::A(3).to_string()} {W { v: 1 }.to_string()} {xs[1].to_string()} {t} {acc}"
}
main()
"#,
        "s7 n3 w s4 s4 1090",
    );
}

#[test]
fn a_trait_to_string_method_wins_over_the_builtin_through_a_bound_a_generic_impl_and_a_closure() {
    string_everywhere(
        &format!(
            r#"{NAMED}
fn nm<T: Named>(t: T) -> string {{ t.to_string() }}
struct Holder<T> {{ t: T }}
impl<T: Named> Holder<T> {{ fn show(self) -> string {{ self.t.to_string() }} }}
fn main() -> string {{
    let f = fn(q: Q) -> string {{ q.to_string() }}
    let mut acc = ""
    for q in vec![Q {{ v: 3 }}, Q {{ v: 4 }}] {{ acc = acc + q.to_string() }}
    let h = Holder {{ t: Q {{ v: 2 }} }}
    "{{nm(Q {{ v: 5 }})}} {{h.show()}} {{f(Q {{ v: 6 }})}} {{acc}} {{Q {{ v: 8 }}.to_string()}}"
}}
main()
"#
        ),
        "q5 q2 q6 q3q4 q8",
    );
}

#[test]
fn to_string_on_a_type_without_its_own_method_keeps_the_display_check() {
    rejected("struct P { v: int }\nP { v: 1 }.to_string()\n", "E0338");
    rejected(
        "struct U { v: int }\nimpl U { fn to_string(self) -> string { \"u\" } }\nvec![U { v: 1 }].to_string()\n",
        "E0338",
    );
}

#[test]
fn a_final_match_keeps_its_value_unless_every_completing_arm_assigns() {
    script_int_at_every_level("let mut a = 1\nmatch a { 1 => 9, _ => { a = 2 } }\n", 9);
    script_int_at_every_level("let mut a = 2\nmatch a { 1 => { a = 5 }, _ => 7 }\n", 7);
    script_int_at_every_level("let mut a = 1\nif a == 1 { 9 } else { a = 2 }\n", 9);
    script_int_at_every_level(
        "let mut a = 1\nmatch a { 1 => match a { 1 => 6, _ => { a = 1 } }, _ => { a = 3 } }\n",
        6,
    );
    script_unit_at_every_level("let mut a = 1\nmatch a { 1 => { a = 9 }, _ => { a = 2 } }\n");
    script_unit_at_every_level("let mut a = 1\nmatch a { 1 => { a = 5 }, _ => panic(\"x\") }\n");
    script_unit_at_every_level("let mut a = 1\nif a == 1 { a = 3 } else { a = 2 }\n");
}

#[test]
fn a_fixed_point_receiver_and_right_operand_are_read_before_later_operands_run() {
    int_everywhere(
        r#"
fn main() -> int {
    let mut m = 10
    let bm = fn() -> int { m = 100; 1 }
    let r1 = m.mul_shr(bm(), 0)
    m = 10
    let r2 = m.shl_div(bm(), 0)
    m = 10
    let mut k = 3
    let sk = fn() -> int { k = 1000; 1 }
    let r3 = m.mul_shr(k, sk())
    let mut y: i32 = 6
    let by = fn() -> i32 { y = 100; 2 }
    let r4 = y.mul_shr(by(), 1)
    let mut t = 0
    let mut i = 0
    while i < 400 {
        let mut z = 4
        let bz = fn() -> int { z = 1000; 3 }
        t += z.mul_shr(bz(), 0)
        i += 1
    }
    r1 * 1000000 + r2 * 10000 + r3 * 100 + (r4 as int) + t * 0 + (t - 4800)
}
main()
"#,
        10_000_000 + 10 * 10_000 + 15 * 100 + 6,
    );
}

#[test]
fn a_mutable_function_variable_is_read_before_the_arguments_of_its_call() {
    int_everywhere(
        r#"
fn early() -> int { cb(rebind()) }
let mut cb: fn(int) -> int = fn(k: int) -> int { k }
fn rebind() -> int { cb = fn(k: int) -> int { k * 7 }; 10 }
fn late() -> int { cb(rebind()) }
fn local() -> int {
    let mut f = fn(k: int) -> int { k + 1 }
    let reb = fn() -> int { f = fn(k: int) -> int { k * 100 }; 2 }
    let a = f(reb())
    f = fn(k: int) -> int { k + 1 }
    let g = fn() -> int { f(reb()) }
    a * 10 + g()
}
let a = cb(rebind())
cb = fn(k: int) -> int { k }
let b = late()
cb = fn(k: int) -> int { k }
let c = early()
a * 100000 + b * 1000 + c * 10 + local() - 33
"#,
        10 * 100_000 + 10 * 1000 + 10 * 10,
    );
}

const FIND: &str = "fn find(i: int) -> Option<int> { if i > 1 { Some(i) } else { None } }\nfn g() -> bool { true }\n";

#[test]
fn a_value_assigned_right_before_a_loop_exit_is_an_unused_option() {
    for body in [
        "fn main() { let mut r = find(0)\n loop { if r.is_some() { println(1) }\n r = find(5)\n break } }\nmain()",
        "fn main() -> int { let mut r = find(0)\n loop { if r.is_some() { println(1) }\n r = find(5)\n return 3 } }\nmain()",
        "fn main() { let mut r = find(0)\n while g() { if r.is_some() { println(1) }\n r = find(5)\n break } }\nmain()",
        "fn main() { let mut r = find(0)\n for i in 0..3 { if r.is_some() { println(1) }\n match i { 1 => { r = find(i); break }, _ => {} } } }\nmain()",
        "fn main() { let mut r = find(0)\n for i in 0..3 { if r.is_some() { println(1) }\n match i { 1 => { r = find(i)\n break }, _ => {} } } }\nmain()",
    ] {
        rejected(&format!("{FIND}{body}"), "E0304");
    }
}

#[test]
fn a_value_assigned_in_a_loop_and_read_by_a_later_iteration_stays_used() {
    for body in [
        "fn main() { let mut r = find(0)\n loop { if r.is_some() { break }\n r = find(5) } }\nmain()",
        "fn main() { let mut r = find(0)\n loop { if r.is_some() { break }\n r = find(5)\n if g() { break } } }\nmain()",
        "fn main() { let mut r = find(0)\n for i in 0..3 { if r.is_some() { println(1) }\n if i == 1 { r = find(i); break }\n r = find(9) } }\nmain()",
        "fn main() { let mut r = find(0)\n loop { if r.is_some() { println(1) }\n r = find(5)\n let f = fn() -> int { return 1 }\n if g() { break } } }\nmain()",
        "fn main() { let mut r = find(0)\n loop { if r.is_some() { println(1) }\n r = find(5)\n break }\n println(r.is_some()) }\nmain()",
        "fn main() { let mut r = find(0)\n for i in 0..2 { loop { if r.is_some() { println(1) }\n r = find(5)\n break } } }\nmain()",
    ] {
        accepted(&format!("{FIND}{body}"));
    }
    int_everywhere(
        "struct N { v: int, next: Option<N> }\nfn main() -> int {\n let mut cur = Some(N { v: 1, next: Some(N { v: 2, next: None }) })\n let mut s = 0\n while let Some(n) = cur { s += n.v\n cur = n.next }\n s\n}\nmain()",
        3,
    );
}

fn chain_program(calls: usize) -> String {
    format!(
        "struct C {{ n: int }}\nimpl C {{\n    fn inc(self) -> C {{ C {{ n: self.n + 1 }} }}\n}}\nlet c = C {{ n: 0 }}\nlet r = c{}.n\nr",
        ".inc()".repeat(calls)
    )
}

#[test]
fn a_method_chain_past_the_typed_tree_limit_is_the_recursion_limit_not_an_undetermined_type() {
    std::thread::Builder::new()
        .stack_size(8 << 20)
        .spawn(|| {
            for calls in [512, 700, 1000] {
                let source = chain_program(calls);
                let text = match Runtime::new().compile(&source, CompileOptions::default()) {
                    Ok(_) => panic!("a {calls}-call chain must be rejected"),
                    Err(error) => error.to_string(),
                };
                assert!(text.contains("E0368"), "{calls}: {text}");
                assert!(!text.contains("E0377"), "{calls}: {text}");
            }
        })
        .expect("spawn")
        .join()
        .expect("the body must not panic");
}

#[test]
fn closures_made_in_a_guarded_arm_and_in_the_falling_through_last_arm_keep_their_captures() {
    int_everywhere(
        r#"
fn mk(k: int) -> fn() -> int {
    match k {
        n if n > 100 => fn() -> int { n * 2 },
        n if n > 50 => fn() -> int { n + 1 },
        m => fn() -> int { m * 3 },
    }
}
let mut t = 0
let mut i = 0
while i < 300 {
    let f = mk(i)
    let g = mk(299 - i)
    t += f() + g()
    i += 1
}
t
"#,
        (0..300)
            .map(|i: i64| {
                if i > 100 {
                    i * 2
                } else if i > 50 {
                    i + 1
                } else {
                    i * 3
                }
            })
            .sum::<i64>()
            * 2,
    );
}
