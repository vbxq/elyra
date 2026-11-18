use aelys::{
    CompileOptions, ExecutionOutcome, IsolateConfig, JitMode, RunOptions, Runtime, StructuredValue,
};
use aelys_backend::Compiler;
use aelys_bytecode::OpCode;
use aelys_bytecode::asm::disassemble;
use aelys_frontend::lexer::Lexer;
use aelys_frontend::parser::Parser;
use aelys_opt::{OptimizationLevel, Optimizer};
use aelys_sema::TypeInference;
use aelys_syntax::{Source, Span};
use std::collections::{BTreeSet, HashMap};

const PRELUDE: &str = r#"
let mut sink = 0

fn hog(a: int) -> int {
    let x1 = a + 1000
    let x2 = x1 * 3
    let x3 = x2 + 7
    let x4 = x3 * 5
    let x5 = x4 + x1
    let x6 = x5 - x2
    sink = x6 + x4
    a
}
"#;

#[derive(Clone, Copy, Debug)]
enum Expected {
    Int(i64),
    Text(&'static str),
}

const LEVELS: [OptimizationLevel; 4] = [
    OptimizationLevel::None,
    OptimizationLevel::Basic,
    OptimizationLevel::Standard,
    OptimizationLevel::Aggressive,
];

const CORPUS: &[(&str, &str, Expected)] = &[
    (
        "reproducer",
        r#"
fn callee(a: int) {
    let x1 = a + 1
    let x2 = x1 * 2
    let x3 = x2 + 3
    let x4 = x3 * 2
    sink = x4
    if a > 100 { callee(a - 1) }
}

fn f(n: int) -> int {
    let a = n + 1
    let b = n + 2
    let c = n + 3
    let keep = n + 1000
    sink = a + b + c
    callee(n)
    keep
}

f(1)
"#,
        Expected::Int(1001),
    ),
    (
        "hot_loop",
        r#"
fn f(n: int) -> int {
    let a = n + 1
    let b = n + 2
    let keep = n + 1000
    sink = a + b
    let r = hog(n)
    keep + r
}

let mut total = 0
for i in 0..3000 {
    total = total + f(i)
}
total
"#,
        Expected::Int(11997000),
    ),
    (
        "nested_args",
        r#"
fn add3(a: int, b: int, c: int) -> int {
    let t1 = a * 100
    let t2 = b * 10
    let t3 = t1 + t2
    let t4 = t3 + c
    sink = t4 + 1
    t4
}

fn f(n: int) -> int {
    let a = n + 1
    let b = n + 2
    let keep1 = n * 1000
    let c = n + 3
    let keep2 = n * 10000
    sink = a + b + c
    let r = add3(hog(n), hog(n + 1), add3(hog(0), hog(0), hog(n + 2)))
    r + keep1 + keep2
}

f(1)
"#,
        Expected::Int(11123),
    ),
    (
        "expr_temporaries",
        r#"
fn g(n: int) -> int {
    let p = n * 3
    let q = n * 5
    let dead1 = n + 11
    let dead2 = n + 12
    sink = dead1 + dead2
    (p * 2) + (q * 3) + hog(n) * 7 + (p * q) + hog(n + 1)
}

g(2)
"#,
        Expected::Int(119),
    ),
    (
        "method_call",
        r#"
struct Acc { base: int, inc: int }

impl Acc {
    fn bump(self, k: int) -> int {
        let u1 = self.base + k
        let u2 = u1 * 2
        let u3 = u2 + self.inc
        let u4 = u3 * 3
        let u5 = u4 + u1
        sink = u5
        self.base + k
    }
}

fn m(n: int) -> int {
    let acc = Acc { base: 10, inc: 1 }
    let d1 = n + 1
    let d2 = n + 2
    let keep = n * 100
    sink = d1 + d2
    let r = acc.bump(n)
    r + keep + acc.bump(hog(1))
}

m(3)
"#,
        Expected::Int(324),
    ),
    (
        "closure_call",
        r#"
fn c(n: int) -> int {
    let d1 = n + 1
    let d2 = n + 2
    let keep = n * 100
    let cl = fn(x: int) -> int {
        let y1 = x + 1000
        let y2 = y1 * 2
        let y3 = y2 + 5
        let y4 = y3 * 2
        sink = y4
        x + 1
    }
    sink = d1 + d2
    let r = cl(n)
    r + keep
}

fn u(n: int) -> int {
    let inner = fn(x: int) -> int {
        let y1 = x + 1000
        let y2 = y1 * 2
        let y3 = y2 + 5
        sink = y3
        x * 2
    }
    let outer = fn(x: int) -> int {
        let d1 = x + 1
        let d2 = x + 2
        let keep = x * 100
        sink = d1 + d2
        let r = inner(x)
        r + keep
    }
    outer(n)
}

c(4) + u(5)
"#,
        Expected::Int(915),
    ),
    (
        "loops",
        r#"
fn lp(n: int) -> int {
    let mut total = 0
    let keep = 7
    for i in 0..n {
        let d1 = i + 1
        let d2 = i + 2
        let k2 = i * 10
        sink = d1 + d2
        total = total + hog(i) + k2 + keep
    }
    let mut j = 0
    while j < n {
        let e1 = j + 1
        let e2 = j * 3
        sink = e1
        total = total + hog(j) + e2
        j = j + 1
    }
    let v = vec![100, 200]
    for x in v {
        let e1 = x + 1
        let e3 = x * 2
        sink = e1
        total = total + hog(x) + e3
    }
    total
}

lp(5)
"#,
        Expected::Int(1085),
    ),
    (
        "many_args",
        r#"
fn many(a: int, b: int, c: int, d: int, e: int, f: int, g: int, h: int, i: int, j: int) -> int {
    let s1 = a + b
    let s2 = c + d
    let s3 = e + f
    let s4 = g + h
    let s5 = s1 * s2 + s3 * s4
    sink = s5
    a + 2 * b + 3 * c + 4 * d + 5 * e + 6 * f + 7 * g + 8 * h + 9 * i + 10 * j
}

fn mm(n: int) -> int {
    let d1 = n + 1
    let d2 = n + 2
    let keep = n * 1000
    sink = d1 + d2
    many(n, n, n, n, n, n, n, n, n, n) + keep + many(1, 1, 1, 1, 1, 1, 1, 1, 1, hog(1))
}

mm(1)
"#,
        Expected::Int(1110),
    ),
    (
        "recursion",
        r#"
fn fib(n: int) -> int {
    if n < 2 {
        return n
    }
    let d1 = n + 1
    let d2 = n + 2
    let keep = n * 3
    sink = d1 + d2
    let a = fib(n - 1)
    let b = fib(n - 2)
    a + b + keep - n * 3
}

fib(15)
"#,
        Expected::Int(610),
    ),
    (
        "interpolation",
        r#"
struct P { v: int }

impl Display for P {
    fn to_display(self) -> string {
        let z1 = self.v + 1000
        let z2 = z1 * 2
        let z3 = z2 + 3
        let z4 = z3 * 2
        sink = z4
        "p{self.v}"
    }
}

fn s(n: int) -> string {
    let d1 = n + 1
    let d2 = n + 2
    let keep = n * 100
    sink = d1 + d2
    "{hog(n)}-{keep}-{hog(n + 1)}-{keep + 1}"
}

fn ds(n: int) -> string {
    let p = P { v: n }
    let d1 = n + 1
    let keep = n * 7
    sink = d1
    let t = hog(n).to_string()
    "{p}:{keep}:{p}:{t}:{keep}"
}

"{s(2)}|{ds(3)}"
"#,
        Expected::Text("2-200-3-201|p3:21:p3:3:21"),
    ),
    (
        "inlined",
        r#"
@inline
fn il(a: int) -> int {
    let w1 = a + 1000
    let w2 = w1 * 2
    sink = w2
    a + 1
}

@inline_always
fn ila(a: int) -> int {
    a * 2 + hog(a)
}

fn ii(n: int) -> int {
    let d1 = n + 1
    let d2 = n + 2
    let keep = n * 100
    sink = d1 + d2
    il(n) + ila(n) + keep + il(hog(n))
}

ii(2)
"#,
        Expected::Int(212),
    ),
    (
        "pipeline",
        r#"
fn pl(n: int) -> int {
    let d1 = n + 1
    let d2 = n + 2
    let keep = n * 100
    sink = d1 + d2
    let v = vec![1, 2, 3]
    let mapped = v.map(fn(x: int) -> int { let q1 = x + 1000; let q2 = q1 * 2; sink = q2; x * 10 })
    let kept = mapped.filter(fn(x: int) -> bool { let q1 = x + 1000; let q2 = q1 * 2; sink = q2; x > 10 })
    let total = kept.fold(0, fn(acc: int, x: int) -> int { let q1 = x + 1000; let q2 = q1 + acc; sink = q2; acc + x })
    total + keep + mapped.len()
}

pl(1)
"#,
        Expected::Int(153),
    ),
    (
        "sum_callbacks",
        r#"
fn om(n: int) -> int {
    let d1 = n + 1
    let keep = n * 100
    sink = d1
    let o: Option<int> = Some(n)
    let m = o.map(fn(x: int) -> int { let q1 = x + 1000; let q2 = q1 * 3; sink = q2; x + 5 })
    let none: Option<int> = None
    let e = none.unwrap_or_else(fn() -> int { let q1 = 1000; let q2 = q1 * 3; sink = q2; 9 })
    m.unwrap() + e + keep
}

om(1)
"#,
        Expected::Int(115),
    ),
    (
        "generic_call",
        r#"
fn pick<T>(a: T, b: T) -> T {
    let z1 = 1000
    let z2 = z1 * 2
    let z3 = z2 * 2
    sink = z3
    b
}

fn gg(n: int) -> int {
    let d1 = n + 1
    let keep = n * 100
    sink = d1
    pick(n, n + 1) + keep + pick(hog(1), hog(2))
}

gg(2)
"#,
        Expected::Int(205),
    ),
    (
        "positions",
        r#"
fn ci(n: int) -> int {
    let mut v = vec![10, 20, 30, 40]
    let d1 = n + 1
    let d2 = n + 2
    let mut keep = n * 100
    sink = d1 + d2
    if hog(n) > 0 {
        keep = keep + v[hog(1)]
    }
    let arr = [hog(1), hog(2), keep]
    v[hog(2)] = hog(5)
    keep + arr[0] + arr[1] + v[2]
}

ci(1)
"#,
        Expected::Int(128),
    ),
    (
        "match_arms",
        r#"
enum Sh { A(int), B(int, int) }

fn mt(n: int) -> int {
    let d1 = n + 1
    let keep = n * 100
    sink = d1
    let s: Sh = if n > 0 { Sh::B(n, n + 1) } else { Sh::A(n) }
    let r = match s {
        Sh::A(x) => hog(x),
        Sh::B(x, y) => hog(x) + hog(y) * 10,
    }
    r + keep
}

mt(2)
"#,
        Expected::Int(232),
    ),
    (
        "struct_fields",
        r#"
struct Tri { a: int, b: int, c: int }

fn st(n: int) -> int {
    let d1 = n + 1
    let keep = n * 100
    sink = d1
    let t = Tri { a: hog(n), b: keep, c: hog(n + 2) }
    t.a + t.b + t.c + keep
}

st(1)
"#,
        Expected::Int(204),
    ),
    (
        "assign_low_local",
        r#"
fn as1(n: int) -> int {
    let mut r = 0
    let d1 = n + 1
    let keep = n * 100
    sink = d1
    r = hog(n)
    r = r + hog(n + 1)
    r + keep
}

as1(1)
"#,
        Expected::Int(103),
    ),
    (
        "try_from",
        r#"
struct ParseErr { line: int }
struct AppErr { line: int }

impl From<ParseErr> for AppErr {
    fn from(source: ParseErr) -> AppErr {
        let z1 = source.line + 1000
        let z2 = z1 * 2
        let z3 = z2 * 2
        sink = z3
        AppErr { line: source.line * 2 }
    }
}

fn parse(n: int) -> Result<int, ParseErr> {
    if n > 0 {
        return Err(ParseErr { line: n })
    }
    Ok(5)
}

fn run(n: int) -> Result<int, AppErr> {
    let d1 = n + 1
    let keep = n * 100
    sink = d1
    let value = parse(n)?
    Ok(value + keep)
}

fn top(n: int) -> int {
    let d1 = n + 1
    let keep = n * 1000
    sink = d1
    let r = match run(n) {
        Ok(v) => v,
        Err(e) => e.line,
    }
    r + keep
}

top(0) + top(3)
"#,
        Expected::Int(3011),
    ),
];

fn program(body: &str) -> String {
    format!("{PRELUDE}\n{body}")
}

fn run(source: &str, mode: JitMode, level: OptimizationLevel) -> StructuredValue {
    let runtime = Runtime::with_jit_mode(mode);
    let module = runtime
        .compile(
            source,
            CompileOptions {
                optimization_level: level,
                ..CompileOptions::default()
            },
        )
        .unwrap_or_else(|error| panic!("compile failed at {mode:?}/{level:?}: {error}"));
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    let ExecutionOutcome::Returned(value) = isolate
        .execute(&module, RunOptions::default())
        .unwrap_or_else(|error| panic!("run failed at {mode:?}/{level:?}: {error}"))
    else {
        panic!("program exited instead of returning");
    };
    isolate.structured_clone(value).expect("plain result")
}

fn assembly_at(source: &str, level: OptimizationLevel) -> String {
    let src = Source::new("<t8>", source);
    let tokens = Lexer::with_source(src.clone()).scan().expect("lexes");
    let ast = Parser::new_rust_collections(tokens, src.clone())
        .parse()
        .expect("parses");
    let typed = TypeInference::infer_program(ast, src.clone()).expect("types");
    let optimized = Optimizer::new(level).optimize(typed);
    let (function, _globals) = Compiler::new(None, src)
        .compile_typed(&optimized)
        .expect("compiles");
    disassemble(&function)
}

struct Insn {
    pc: usize,
    op: String,
    tokens: Vec<String>,
}

struct Func {
    name: String,
    insns: Vec<Insn>,
    labels: HashMap<String, usize>,
}

fn parse_assembly(text: &str) -> Vec<Func> {
    let mut funcs: Vec<Func> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with(".function ") {
            funcs.push(Func {
                name: "<top>".to_string(),
                insns: Vec::new(),
                labels: HashMap::new(),
            });
            continue;
        }
        let Some(current) = funcs.last_mut() else {
            continue;
        };
        if let Some(name) = trimmed.strip_prefix(".name ") {
            current.name = name.trim_matches('"').to_string();
            continue;
        }
        if let Some(label) = trimmed.strip_suffix(':')
            && label.starts_with('L')
            && label[1..].chars().all(|c| c.is_ascii_digit())
        {
            current
                .labels
                .insert(label.to_string(), current.insns.len());
            continue;
        }
        let Some((pc, rest)) = trimmed.split_once(": ") else {
            continue;
        };
        if pc.len() != 4 || !pc.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let rest = rest.split(';').next().unwrap_or("").trim();
        let (op, operands) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        current.insns.push(Insn {
            pc: pc.parse().expect("four digits"),
            op: op.to_string(),
            tokens: operands
                .split(',')
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect(),
        });
    }
    funcs
}

fn register(token: &str) -> Option<usize> {
    token.strip_prefix('r').and_then(|n| n.parse().ok())
}

fn number(token: &str) -> usize {
    token.parse().unwrap_or(0)
}

struct Effect {
    reads: BTreeSet<usize>,
    writes: BTreeSet<usize>,
    successors: Vec<usize>,
    window: Option<(usize, usize)>,
}

fn effect(func: &Func, index: usize, pc_index: &HashMap<usize, usize>) -> Effect {
    let insn = &func.insns[index];
    let t = &insn.tokens;
    let regs: Vec<usize> = t.iter().filter_map(|tok| register(tok)).collect();
    let range = |start: usize, len: usize| (start..start + len).collect::<BTreeSet<usize>>();
    let mut reads = BTreeSet::new();
    let mut writes = BTreeSet::new();
    let mut window = None;
    let mut successors = if index + 1 < func.insns.len() {
        vec![index + 1]
    } else {
        Vec::new()
    };
    let op = insn.op.as_str();
    match op {
        "Call" | "CallWide" => {
            let (d, f, k) = (regs[0], regs[1], number(&t[2]));
            reads = range(f + 1, k);
            reads.insert(f);
            writes.insert(d);
            window = Some((f + 1, d));
        }
        "CallCached" => {
            let (d, f, k) = (regs[0], regs[1], number(&t[2]));
            reads = range(d + 1, k);
            reads.insert(f);
            writes.insert(d);
            window = Some((d + 1, d));
        }
        "CallGlobal" | "CallUpval" | "TailCallUpval" => {
            let (d, k) = (regs[0], number(&t[2]));
            reads = range(d + 1, k);
            writes.insert(d);
            window = Some((d + 1, d));
        }
        "VecLit" | "ArrayLit" | "VecLitWide" | "ArrayLitWide" => {
            reads = range(regs[1], number(&t[2]));
            writes.insert(regs[0]);
        }
        "StructNew" => {
            reads = range(regs[1], number(&t[3]));
            writes.insert(regs[0]);
        }
        "EnumNew" => {
            reads = range(regs[1], number(&t[4]));
            writes.insert(regs[0]);
        }
        "MatchFail" => {
            let (source, flags) = (regs[1], number(&t[2]));
            if flags & 1 == 1 {
                reads.insert(source);
            }
            if flags & 2 == 2 {
                reads.insert(source + (flags & 1));
            }
        }
        _ if op.starts_with("ForLoopI")
            || op == "WhileLoopLt"
            || op.starts_with("VecForLoop")
            || op.starts_with("ArrayForLoop")
            || op.starts_with("StringForLoop") =>
        {
            let base = regs[0];
            if op.starts_with("ForLoopI") {
                reads = range(base, 3);
                writes.insert(base);
            } else if op == "WhileLoopLt" {
                reads = range(base, 2);
            } else {
                reads = range(base + 1, 2);
                writes = range(base, 2);
            }
            let target = &t[1];
            if let Some(&label) = func.labels.get(target) {
                successors.push(label);
            } else if let Ok(offset) = target.parse::<i64>() {
                let pc = i64::try_from(insn.pc).expect("pc fits") + 1 + offset;
                if let Some(&j) = usize::try_from(pc).ok().and_then(|pc| pc_index.get(&pc)) {
                    successors.push(j);
                }
            }
        }
        "SetGlobalIdx" | "SetGlobalIdxWide" | "SetGlobal" | "SetUpval" | "JumpIf" | "JumpIfNot"
        | "JumpIfLong" | "JumpIfNotLong" | "JumpIfWideLong" | "JumpIfNotWideLong" | "Return"
        | "ReturnWide" | "VecReserve" | "StructStore" | "CloseUpvals" | "Return0" => {
            reads = regs.iter().copied().collect();
        }
        _ if op.starts_with("VecPush")
            || op.starts_with("VecStore")
            || op.starts_with("ArrayStore") =>
        {
            reads = regs.iter().copied().collect();
        }
        _ => {
            if let Some((&first, rest)) = regs.split_first() {
                writes.insert(first);
                reads = rest.iter().copied().collect();
            }
        }
    }
    if op == "CloseUpvals" {
        reads.clear();
    }
    let jumps = op == "Jump" || op == "JumpLong";
    if jumps {
        successors.clear();
    }
    if !op.contains("ForLoop") && op != "WhileLoopLt" {
        for tok in t {
            if let Some(&label) = func.labels.get(tok) {
                successors.push(label);
            }
        }
    }
    if matches!(
        op,
        "Return" | "Return0" | "ReturnWide" | "MatchFail" | "TailCallUpval"
    ) {
        successors.clear();
    }
    Effect {
        reads,
        writes,
        successors,
        window,
    }
}

// the callee frame starts at the window base and may use any number of registers, so nothing live may sit at or above it
fn clobbering_calls(assembly: &str) -> Vec<String> {
    let mut problems = Vec::new();
    for func in parse_assembly(assembly) {
        let pc_index: HashMap<usize, usize> = func
            .insns
            .iter()
            .enumerate()
            .map(|(i, insn)| (insn.pc, i))
            .collect();
        let effects: Vec<Effect> = (0..func.insns.len())
            .map(|i| effect(&func, i, &pc_index))
            .collect();
        let n = effects.len();
        let mut live_in = vec![BTreeSet::new(); n];
        let mut live_out = vec![BTreeSet::new(); n];
        let mut changed = true;
        while changed {
            changed = false;
            for i in (0..n).rev() {
                let mut out = BTreeSet::new();
                for &s in &effects[i].successors {
                    out.extend(live_in[s].iter().copied());
                }
                let mut input: BTreeSet<usize> =
                    out.difference(&effects[i].writes).copied().collect();
                input.extend(effects[i].reads.iter().copied());
                if out != live_out[i] || input != live_in[i] {
                    live_out[i] = out;
                    live_in[i] = input;
                    changed = true;
                }
            }
        }
        for (i, eff) in effects.iter().enumerate() {
            let Some((base, dest)) = eff.window else {
                continue;
            };
            let clobbered: Vec<usize> = live_out[i]
                .iter()
                .copied()
                .filter(|&r| r >= base && r != dest)
                .collect();
            if !clobbered.is_empty() {
                problems.push(format!(
                    "{} pc {:04} {} window r{base} overwrites live {clobbered:?}",
                    func.name, func.insns[i].pc, func.insns[i].op
                ));
            }
        }
    }
    problems
}

fn matches_expected(value: &StructuredValue, expected: Expected) -> bool {
    match (value, expected) {
        (StructuredValue::Int(got), Expected::Int(want)) => *got == want,
        (StructuredValue::String(got), Expected::Text(want)) => got == want,
        _ => false,
    }
}

#[test]
fn every_call_keeps_the_callers_live_registers_at_every_level_and_tier() {
    let mut failures = Vec::new();
    for (name, body, expected) in CORPUS {
        let source = program(body);
        for mode in [JitMode::Off, JitMode::Baseline, JitMode::Tiered] {
            for level in LEVELS {
                let value = run(&source, mode, level);
                if !matches_expected(&value, *expected) {
                    failures.push(format!(
                        "{name} {mode:?}/{level:?}: got {value:?}, want {expected:?}"
                    ));
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "wrong answers:\n{}",
        failures.join("\n")
    );
}

#[test]
fn no_emitted_call_window_sits_below_a_live_register() {
    let mut failures = Vec::new();
    for (name, body, _) in CORPUS {
        let source = program(body);
        for level in LEVELS {
            for problem in clobbering_calls(&assembly_at(&source, level)) {
                failures.push(format!("{name} {level:?}: {problem}"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "clobbering calls:\n{}",
        failures.join("\n")
    );
}

#[test]
fn the_scanner_sees_a_window_placed_under_a_live_local() {
    let assembly = r#"
.function 0
  .name "f"
  .code
    0000: AddI      r1, r0, 1
    0001: LoadI     r4, 7
    0002: GetGlobalIdx r2, 1  ; callee
    0003: Move      r3, r0
    0004: Call      r1, r2, 1
    0005: Move      r0, r4
    0006: Return    r0
"#;
    let problems = clobbering_calls(assembly);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].contains("live [4]"), "{problems:?}");
}

#[test]
fn a_call_emitted_under_a_live_register_is_refused_with_e0216() {
    let mut compiler = Compiler::new(None, Source::new("<t8>", ""));
    let registers: Vec<u16> = (0..5)
        .map(|_| compiler.alloc_register().expect("register"))
        .collect();
    assert_eq!(registers, vec![0, 1, 2, 3, 4]);
    compiler.free_register(3);
    compiler.free_register(2);
    compiler.emit_a(OpCode::Call, 1u16, 2u16, 0u16, Span::dummy());
    let error = compiler
        .finish_registers()
        .expect_err("r4 is live above the window");
    let text = error.to_string();
    assert!(text.contains("E0216"), "{text}");
    assert!(text.contains("r4"), "{text}");
}

#[test]
fn a_call_window_above_every_live_register_is_accepted() {
    let mut compiler = Compiler::new(None, Source::new("<t8>", ""));
    for _ in 0..5 {
        compiler.alloc_register().expect("register");
    }
    compiler.free_register(3);
    compiler.free_register(2);
    let window = compiler
        .alloc_call_window(2, Span::dummy())
        .expect("window");
    assert_eq!(window, 5);
    compiler.emit_a(OpCode::Call, 1u16, window, 1u16, Span::dummy());
    compiler
        .finish_registers()
        .expect("nothing live above the window");
}
