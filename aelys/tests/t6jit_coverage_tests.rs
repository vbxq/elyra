use aelys::{
    AelysError, CompileOptions, ExecutionOutcome, IsolateConfig, JitMode, RunOptions, Runtime,
    Value,
};

struct Run {
    value: Value,
    entries: usize,
    osr: u64,
}

fn execute(source: &str, mode: JitMode) -> Result<Run, AelysError> {
    let runtime = Runtime::with_jit_mode(mode);
    let module = runtime.compile(source, CompileOptions::default())?;
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    let outcome = isolate.execute(&module, RunOptions::default())?;
    let ExecutionOutcome::Returned(value) = outcome else {
        panic!("expected a returned value, got {outcome:?}");
    };
    Ok(Run {
        value,
        entries: runtime.jit_cache_entries(),
        osr: runtime.jit_osr_executions(),
    })
}

fn compiled(source: &str) -> Run {
    let interpreted = execute(source, JitMode::Off).expect("interpreted run");
    let compiled = execute(source, JitMode::Tiered).expect("compiled run");
    assert_eq!(interpreted.value, compiled.value, "{source}");
    assert!(compiled.entries >= 1, "never compiled:\n{source}");
    compiled
}

fn int_of(run: &Run) -> i64 {
    run.value.as_int().expect("integer result")
}

const CONTEXTS: [&str; 3] = ["osr", "entry", "nested"];

fn in_context(context: &str, body: &str, n: i64) -> String {
    match context {
        "osr" => format!(
            "fn advance(acc: int, i: int) -> int {{\n    return 0\n}}\nfn run(n: int) -> int {{\n    let mut acc = 7\n    for i in 0..n {{\n        {body}\n    }}\n    return acc\n}}\nrun({n})\n"
        ),
        "entry" => format!(
            "fn advance(acc: int, i: int) -> int {{\n    let mut acc = acc\n    {body}\n    return acc\n}}\nlet mut acc = 7\nlet mut i = 0\nwhile i < {n} {{\n    acc = advance(acc, i)\n    i = i + 1\n}}\nacc\n"
        ),
        _ => format!(
            "fn advance(acc: int, i: int) -> int {{\n    let mut acc = acc\n    {body}\n    return acc\n}}\nfn run(n: int) -> int {{\n    let mut acc = 7\n    for i in 0..n {{\n        acc = advance(acc, i) + advance(0, 0) * 0\n    }}\n    return acc\n}}\nrun({n})\n"
        ),
    }
}

fn check_forms(forms: &[(&str, fn(i64, i64) -> i64)]) {
    for (body, reference) in forms {
        let n = 30_000;
        let mut expected = 7i64;
        for i in 0..n {
            expected = reference(expected, i);
        }
        for context in CONTEXTS {
            let run = compiled(&in_context(context, body, n));
            assert_eq!(int_of(&run), expected, "{body} in {context}");
            if context == "osr" {
                assert!(run.osr >= 1, "{body} never entered from its loop");
            }
        }
    }
}

#[test]
fn every_bitwise_operator_matches_in_every_context() {
    check_forms(&[
        ("acc = (acc & 0xFFFF) | (i & 0xF0)", |a, i| {
            (a & 0xFFFF) | (i & 0xF0)
        }),
        ("acc = (acc ^ (i * 31)) & 0xFFFFFF", |a, i| {
            (a ^ (i * 31)) & 0xFF_FFFF
        }),
        ("acc = ~acc & 0xFFFFF", |a, _| !a & 0xF_FFFF),
        ("acc = ((acc << 3) + i) & 0xFFFFFFF", |a, i| {
            ((a << 3) + i) & 0xFFF_FFFF
        }),
        ("acc = (acc >> (i & 3)) + i", |a, i| (a >> (i & 3)) + i),
        ("acc = ((0 - acc) >> 2) & 0xFFFF", |a, _| {
            ((0 - a) >> 2) & 0xFFFF
        }),
        ("acc = ((acc << 47) >> 47) + i", |a, i| {
            let shifted = (a << 47) & 0xFFFF_FFFF_FFFF;
            let shifted = (shifted << 16) >> 16;
            (shifted >> 47) + i
        }),
        ("acc = (acc | 1) ^ (i >> 1)", |a, i| (a | 1) ^ (i >> 1)),
    ]);
}

#[test]
fn integer_division_and_remainder_match_in_every_context() {
    check_forms(&[
        ("acc = acc / 3 + i % 7", |a, i| a / 3 + i % 7),
        ("acc = (acc - 50000) / 7 + (0 - i) % 5", |a, i| {
            (a - 50000) / 7 + (-i) % 5
        }),
        ("acc = (acc + i) % 1009", |a, i| (a + i) % 1009),
    ]);
}

#[test]
fn loops_of_every_shape_match() {
    let source = r#"
fn shapes(n: int) -> int {
    let mut acc = 0
    for i in 0..n { acc = acc + i }
    for i in 0..=n { acc = acc + (i & 3) }
    let mut j = 0
    while j < n {
        acc = acc ^ j
        j = j + 3
    }
    let mut k = n
    while k > 0 {
        if k % 2 == 0 { acc = acc + 1 } else { acc = acc - 1 }
        k = k - 1
    }
    return acc
}
shapes(40000)
"#;
    let n = 40_000i64;
    let mut acc = (0..n).sum::<i64>();
    acc += (0..=n).map(|i| i & 3).sum::<i64>();
    let mut j = 0;
    while j < n {
        acc ^= j;
        j += 3;
    }
    let mut k = n;
    while k > 0 {
        acc += if k % 2 == 0 { 1 } else { -1 };
        k -= 1;
    }
    let run = compiled(source);
    assert_eq!(int_of(&run), acc);
    assert!(run.osr >= 1);
}

#[test]
fn every_element_kind_loads_and_stores_through_vec_and_array() {
    let source = r#"
fn elements(n: int) -> int {
    let mut vi: Vec<int> = vec![0; 64]
    let mut vf: Vec<float> = vec![0.0; 64]
    let mut vb: Vec<bool> = vec![false; 64]
    let mut ai: [int; 64] = [0; 64]
    let mut af: [float; 64] = [0.0; 64]
    let mut ab: [bool; 64] = [false; 64]
    let mut acc = 0
    for i in 0..n {
        let k = i % 64
        vi[k] = vi[k] + i
        ai[k] = ai[(k + 1) % 64] ^ i
        vf[k] = vf[k] + 0.5
        af[k] = af[k] * 0.5 + 1.0
        vb[k] = vb[k] == false
        ab[k] = vb[k] == ab[(k + 3) % 64]
        if vb[k] { acc = acc + 1 }
        if ab[k] { acc = acc + 2 }
    }
    let mut s = 0
    for k in 0..64 {
        s = (s + vi[k] + ai[k] + vi.len() + ai.len()) & 0xFFFFFFF
        if vf[k] > 100.0 { s = s + 1 }
        if af[k] > 1.5 { s = s + 3 }
    }
    return s * 100000 + acc
}
elements(30000)
"#;
    let n = 30_000i64;
    let (mut vi, mut ai) = ([0i64; 64], [0i64; 64]);
    let (mut vf, mut af) = ([0f64; 64], [0f64; 64]);
    let (mut vb, mut ab) = ([false; 64], [false; 64]);
    let mut acc = 0i64;
    for i in 0..n {
        let k = (i % 64) as usize;
        vi[k] += i;
        ai[k] = ai[(k + 1) % 64] ^ i;
        vf[k] += 0.5;
        af[k] = af[k] * 0.5 + 1.0;
        vb[k] = !vb[k];
        ab[k] = vb[k] == ab[(k + 3) % 64];
        if vb[k] {
            acc += 1;
        }
        if ab[k] {
            acc += 2;
        }
    }
    let mut s = 0i64;
    for k in 0..64 {
        s = (s + vi[k] + ai[k] + 64 + 64) & 0xFFF_FFFF;
        if vf[k] > 100.0 {
            s += 1;
        }
        if af[k] > 1.5 {
            s += 3;
        }
    }
    let run = compiled(source);
    assert_eq!(int_of(&run), s * 100_000 + acc);
}

#[test]
fn struct_fields_of_every_scalar_kind_match_in_methods_and_vectors() {
    let source = r#"
struct Mob { x: int, speed: float, alive: bool, hp: int }
impl Mob {
    fn advance(mut self, i: int) -> int {
        self.x = (self.x + i) & 0xFFFF
        self.speed = self.speed * 0.5 + 1.0
        if self.hp > 0 { self.hp = self.hp - 1 } else { self.alive = false }
        return self.x
    }
}
fn world(n: int) -> int {
    let mut mobs: Vec<Mob> = vec![]
    for j in 0..16 { mobs.push(Mob { x: j, speed: 0.0, alive: true, hp: j * 100 }) }
    let mut acc = 0
    for i in 0..n {
        let mut m = mobs[i % 16]
        acc = (acc + m.advance(i)) & 0xFFFFFFF
        mobs[i % 16] = m
    }
    for j in 0..16 {
        if mobs[j].alive { acc = acc + 1000 }
        if mobs[j].speed > 1.5 { acc = acc + 1 }
    }
    return acc
}
world(40000)
"#;
    let n = 40_000i64;
    let mut mobs = (0..16i64)
        .map(|j| (j, 0.0f64, true, j * 100))
        .collect::<Vec<_>>();
    let mut acc = 0i64;
    for i in 0..n {
        let m = &mut mobs[(i % 16) as usize];
        m.0 = (m.0 + i) & 0xFFFF;
        m.1 = m.1 * 0.5 + 1.0;
        if m.3 > 0 {
            m.3 -= 1;
        } else {
            m.2 = false;
        }
        acc = (acc + m.0) & 0xFFF_FFFF;
    }
    for m in &mobs {
        if m.2 {
            acc += 1000;
        }
        if m.1 > 1.5 {
            acc += 1;
        }
    }
    let run = compiled(source);
    assert_eq!(int_of(&run), acc);
}

#[test]
fn calls_with_any_argument_count_and_mutual_recursion_match() {
    let source = r#"
fn zero() -> int { return 3 }
fn one(a: int) -> int { return a + zero() }
fn two(a: int, b: int) -> int { return one(a) * 2 - b }
fn four(a: int, b: int, c: int, d: int) -> int { return two(a, b) + two(c, d) }
fn even(n: int) -> bool {
    if n == 0 { return true }
    return odd(n - 1)
}
fn odd(n: int) -> bool {
    if n == 0 { return false }
    return even(n - 1)
}
fn calls(n: int) -> int {
    let mut acc = 0
    for i in 0..n {
        acc = (acc + four(i, i & 7, i >> 2, 5)) & 0xFFFFFFF
        if even(i & 31) { acc = acc + 1 }
    }
    return acc
}
calls(20000)
"#;
    let one = |a: i64| a + 3;
    let two = |a: i64, b: i64| one(a) * 2 - b;
    let mut acc = 0i64;
    for i in 0..20_000i64 {
        acc = (acc + two(i, i & 7) + two(i >> 2, 5)) & 0xFFF_FFFF;
        if (i & 31) % 2 == 0 {
            acc += 1;
        }
    }
    let run = compiled(source);
    assert_eq!(int_of(&run), acc);
}

fn trap(source: &str) -> String {
    let interpreted = execute(source, JitMode::Off)
        .err()
        .expect("interpreted trap");
    let runtime = Runtime::with_jit_mode(JitMode::Tiered);
    let module = runtime
        .compile(source, CompileOptions::default())
        .expect("compiles");
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    let compiled = isolate
        .execute(&module, RunOptions::default())
        .err()
        .expect("compiled trap");
    let (AelysError::Runtime(left), AelysError::Runtime(right)) = (&interpreted, &compiled) else {
        panic!("expected runtime errors, got {interpreted:?} and {compiled:?}");
    };
    assert_eq!(format!("{left}"), format!("{right}"), "{source}");
    assert!(
        runtime.jit_cache_entries() >= 1,
        "never compiled:\n{source}"
    );
    assert!(
        runtime.jit_deoptimizations() >= 1,
        "never left machine code:\n{source}"
    );
    format!("{:?}", right.kind)
}

#[test]
fn stores_out_of_bounds_trap_like_the_interpreter_for_every_container() {
    for (declaration, store) in [
        ("let mut c: Vec<int> = vec![0; 8]", "c[i / 2000] = i"),
        ("let mut c: [int; 8] = [0; 8]", "c[i / 2000] = i"),
        ("let mut c: Vec<float> = vec![0.0; 8]", "c[i / 2000] = 1.5"),
        ("let mut c: [bool; 8] = [false; 8]", "c[i / 2000] = true"),
        ("let mut c: Vec<int> = vec![0; 8]", "c[7 - i / 2000] = i"),
    ] {
        let source = format!(
            "fn run(n: int) -> int {{\n    {declaration}\n    for i in 0..n {{\n        {store}\n    }}\n    return 0\n}}\nrun(20000)\n"
        );
        let kind = trap(&source);
        assert!(kind.starts_with("IndexOutOfBounds"), "{kind}");
    }
}

#[test]
fn arithmetic_traps_match_the_interpreter_in_callees_of_compiled_code() {
    for (expression, expected) in [
        ("a * 10000000000", "overflow"),
        ("a / (b - 15000)", "division"),
        ("a % (b - 15000)", "division"),
        ("((a / 1000) << 20) * ((a / 1000) << 20)", "overflow"),
    ] {
        let source = format!(
            "fn inner(a: int, b: int) -> int {{\n    let mut r = 0\n    for k in 0..1 {{ r = {expression} }}\n    return r\n}}\nfn run(n: int) -> int {{\n    let mut s = 0\n    for i in 0..n {{\n        s = (s + inner(i, i)) & 0xFFFFFF\n    }}\n    return s\n}}\nrun(20000)\n"
        );
        let kind = trap(&source);
        let wanted = match expected {
            "overflow" => "IntegerOverflow",
            _ => "DivisionByZero",
        };
        assert!(kind.starts_with(wanted), "{expression}: {kind}");
    }
}
