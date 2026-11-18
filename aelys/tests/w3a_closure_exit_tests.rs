mod t9_support;

use aelys_bytecode::{Function, OpCode, decode_a, decode_b};
use aelys_driver::pipeline::compilation_pipeline_with_opt;
use aelys_opt::OptimizationLevel;
use aelys_syntax::Source;
use t9_support::run_ok;

const MODES: [&[&str]; 7] = [
    &["-O0"],
    &["-O1"],
    &["-O2"],
    &["-O3"],
    &["-ae.jit=on"],
    &["-O3", "-ae.jit=on"],
    &["-ae.gc-verify=on", "-ae.gc-stress=on"],
];

fn assert_all_modes(source: &str, expected: &[&str]) {
    for flags in MODES {
        assert_eq!(run_ok(source, flags), expected, "flags {flags:?}\n{source}");
    }
}

const ESCAPE_HEADER: &str = "struct S { f: fn() -> int, k: int }
struct Maker { base: int }
fn id(x: fn() -> int) -> fn() -> int { x }
fn drive(g: fn() -> int) -> int {
    let v = vec![100, 200]
    let mut t = 0
    let mut i = 0
    while i < 3 {
        t = t * 100 + g()
        i += 1
    }
    t * 10 + i + v[1] - 200
}
";

const ESCAPE_EXPECTED: &str = "612183";

struct EscapeForm {
    name: &'static str,
    ret: &'static str,
    tail: &'static str,
    extract: &'static str,
}

const FN_TY: &str = "fn() -> int";

const ESCAPE_FORMS: [EscapeForm; 13] = [
    EscapeForm {
        name: "var",
        ret: FN_TY,
        tail: "f",
        extract: "{}",
    },
    EscapeForm {
        name: "struct",
        ret: "S",
        tail: "S { f: f, k: 1 }",
        extract: "{}.f",
    },
    EscapeForm {
        name: "vec",
        ret: "Vec<fn() -> int>",
        tail: "vec![f]",
        extract: "{}[0]",
    },
    EscapeForm {
        name: "tuple",
        ret: "(fn() -> int, int)",
        tail: "(f, 1)",
        extract: "{}.0",
    },
    EscapeForm {
        name: "some",
        ret: "Option<fn() -> int>",
        tail: "Some(f)",
        extract: "{}.unwrap()",
    },
    EscapeForm {
        name: "if",
        ret: FN_TY,
        tail: "if n == 0 { f } else { f }",
        extract: "{}",
    },
    EscapeForm {
        name: "match",
        ret: FN_TY,
        tail: "match n { 0 => f, _ => f }",
        extract: "{}",
    },
    EscapeForm {
        name: "block",
        ret: FN_TY,
        tail: "{\n        let k = 1\n        f\n    }",
        extract: "{}",
    },
    EscapeForm {
        name: "id",
        ret: FN_TY,
        tail: "id(f)",
        extract: "{}",
    },
    EscapeForm {
        name: "paren_lambda",
        ret: FN_TY,
        tail: "(fn() -> int {\n        n += 3\n        n * 2\n    })",
        extract: "{}",
    },
    EscapeForm {
        name: "bare_lambda",
        ret: FN_TY,
        tail: "fn() -> int {\n        n += 3\n        n * 2\n    }",
        extract: "{}",
    },
    EscapeForm {
        name: "return_kw",
        ret: FN_TY,
        tail: "return f",
        extract: "{}",
    },
    EscapeForm {
        name: "return_struct",
        ret: "S",
        tail: "return S { f: f, k: 1 }",
        extract: "{}.f",
    },
];

#[derive(Clone, Copy)]
enum Context {
    Function,
    Method,
    Generic,
    TopLambda,
    LambdaInFunction,
}

fn maker_body(form: &EscapeForm, base: &str) -> String {
    format!(
        "    let mut n = {base}
    let f = fn() -> int {{
        n += 3
        n * 2
    }}
    {}
",
        form.tail
    )
}

fn escape_program(context: Context) -> String {
    let mut source = ESCAPE_HEADER.to_string();
    let mut calls = String::new();
    let mut methods = String::new();
    for form in &ESCAPE_FORMS {
        let name = format!("make_{}", form.name);
        let call = match context {
            Context::Function | Context::TopLambda | Context::LambdaInFunction => {
                format!("{name}()")
            }
            Context::Method => format!("Maker {{ base: 0 }}.{name}()"),
            Context::Generic => format!("{name}(0)"),
        };
        let use_line = format!("println(drive({}))\n", form.extract.replace("{}", &call));
        match context {
            Context::Function => {
                source.push_str(&format!(
                    "fn {name}() -> {} {{\n{}}}\n",
                    form.ret,
                    maker_body(form, "0")
                ));
                calls.push_str(&use_line);
            }
            Context::Method => {
                methods.push_str(&format!(
                    "    fn {name}(self) -> {} {{\n{}}}\n",
                    form.ret,
                    maker_body(form, "self.base")
                ));
                calls.push_str(&use_line);
            }
            Context::Generic => {
                source.push_str(&format!(
                    "fn {name}<T>(_seed: T) -> {} {{\n{}}}\n",
                    form.ret,
                    maker_body(form, "0")
                ));
                calls.push_str(&use_line);
            }
            Context::TopLambda | Context::LambdaInFunction => {
                calls.push_str(&format!(
                    "let {name} = fn() -> {} {{\n{}}}\n{use_line}",
                    form.ret,
                    maker_body(form, "0")
                ));
            }
        }
    }
    match context {
        Context::Method => {
            source.push_str(&format!("impl Maker {{\n{methods}}}\n{calls}"));
        }
        Context::LambdaInFunction => {
            source.push_str(&format!("fn run_all() {{\n{calls}}}\nrun_all()\n"));
        }
        _ => source.push_str(&calls),
    }
    source
}

fn assert_escape_context(context: Context) {
    let source = escape_program(context);
    let expected = vec![ESCAPE_EXPECTED; ESCAPE_FORMS.len()];
    assert_all_modes(&source, &expected);
}

#[test]
fn closures_escaping_by_every_tail_form_of_a_function_keep_their_own_state() {
    assert_escape_context(Context::Function);
}

#[test]
fn closures_escaping_by_every_tail_form_of_a_method_keep_their_own_state() {
    assert_escape_context(Context::Method);
}

#[test]
fn closures_escaping_by_every_tail_form_of_a_generic_function_keep_their_own_state() {
    assert_escape_context(Context::Generic);
}

#[test]
fn closures_escaping_by_every_tail_form_of_a_top_level_lambda_keep_their_own_state() {
    assert_escape_context(Context::TopLambda);
}

#[test]
fn closures_escaping_by_every_tail_form_of_a_lambda_inside_a_function_keep_their_own_state() {
    assert_escape_context(Context::LambdaInFunction);
}

const EXIT_DRIVER: &str = "fn drive(fs: Vec<fn() -> int>) -> int {
    let v = vec![7, 8]
    let f = fs[0]
    let a = f()
    let b = f()
    let c = f()
    a * 10000 + b * 100 + c + v[1]
}
";

const PUSH: &str = "fs.push(fn() -> int {
        n += 1
        n * 10
    })";

fn exit_program(declarations: &str, register: &str) -> String {
    format!(
        "{declarations}{EXIT_DRIVER}let mut fs: Vec<fn() -> int> = vec![]\n{register}\nprintln(drive(fs))\n"
    )
}

fn exit_paths() -> Vec<(&'static str, String, &'static str)> {
    let unit_call = "reg(fs, 5)";
    let value_call = "let _ = reg(fs, 5)";
    vec![
        (
            "tail_push",
            format!("fn reg(mut fs: Vec<fn() -> int>, k: int) {{\n    let mut n = k\n    {PUSH}\n}}\n"),
            unit_call,
        ),
        (
            "tail_if_without_else",
            format!(
                "fn reg(mut fs: Vec<fn() -> int>, k: int) {{\n    let mut n = k\n    if k > 0 {{\n        {PUSH}\n    }}\n}}\n"
            ),
            unit_call,
        ),
        (
            "tail_for_loop",
            format!(
                "fn reg(mut fs: Vec<fn() -> int>, k: int) {{\n    let mut n = k\n    for _i in 0..1 {{\n        {PUSH}\n    }}\n}}\n"
            ),
            unit_call,
        ),
        (
            "tail_unit_assignment",
            format!(
                "let mut g = 0\nfn reg(mut fs: Vec<fn() -> int>, k: int) {{\n    let mut n = k\n    {PUSH}\n    g = 1\n}}\n"
            ),
            unit_call,
        ),
        (
            "map_store",
            "fn reg(mut fs: Vec<fn() -> int>, k: int) {\n    let mut m: Map<int, fn() -> int> = Map::new()\n    let mut n = k\n    m[0] = fn() -> int {\n        n += 1\n        n * 10\n    }\n    fs.push(m[0])\n}\n".to_string(),
            unit_call,
        ),
        (
            "method_tail",
            "struct R { fs: Vec<fn() -> int> }\nimpl R {\n    fn add(mut self, k: int) {\n        let mut n = k\n        self.fs.push(fn() -> int {\n            n += 1\n            n * 10\n        })\n    }\n}\nfn reg(mut fs: Vec<fn() -> int>, k: int) {\n    let mut r = R { fs: vec![] }\n    r.add(k)\n    fs.push(r.fs[0])\n}\n".to_string(),
            unit_call,
        ),
        (
            "tail_int",
            format!("fn reg(mut fs: Vec<fn() -> int>, k: int) -> int {{\n    let mut n = k\n    {PUSH}\n    k\n}}\n"),
            value_call,
        ),
        (
            "tail_match",
            format!(
                "fn reg(mut fs: Vec<fn() -> int>, k: int) -> int {{\n    let mut n = k\n    match k {{\n        5 => {{\n            {PUSH}\n            1\n        }}\n        _ => 0,\n    }}\n}}\n"
            ),
            value_call,
        ),
        (
            "inner_block_scope",
            format!(
                "fn reg(mut fs: Vec<fn() -> int>, k: int) {{\n    {{\n        let mut n = k\n        {PUSH}\n    }}\n    let pad = vec![1, 2, 3]\n    let _p = pad[0]\n}}\n"
            ),
            unit_call,
        ),
        (
            "explicit_return",
            format!("fn reg(mut fs: Vec<fn() -> int>, k: int) -> int {{\n    let mut n = k\n    {PUSH}\n    return k\n}}\n"),
            value_call,
        ),
        (
            "return_in_loop",
            format!(
                "fn reg(mut fs: Vec<fn() -> int>, k: int) -> int {{\n    let mut n = k\n    loop {{\n        {PUSH}\n        return 1\n    }}\n}}\n"
            ),
            value_call,
        ),
        (
            "question_mark_exit",
            format!(
                "fn reg(mut fs: Vec<fn() -> int>, k: int) -> Option<int> {{\n    let mut n = k\n    {PUSH}\n    let none: Option<int> = None\n    let x = none?\n    Some(x)\n}}\n"
            ),
            value_call,
        ),
        (
            "early_return_before_the_capture_in_a_later_iteration",
            format!(
                "fn reg(mut fs: Vec<fn() -> int>, k: int) -> int {{\n    let mut n = k\n    let mut i = 0\n    while true {{\n        if i == 1 {{\n            return n\n        }}\n        {PUSH}\n        i += 1\n    }}\n    0\n}}\n"
            ),
            value_call,
        ),
        (
            "unit_return_before_the_capture_in_a_later_iteration",
            format!(
                "fn reg(mut fs: Vec<fn() -> int>, k: int) {{\n    let mut n = k\n    let mut i = 0\n    while true {{\n        if i == 1 {{\n            return\n        }}\n        {PUSH}\n        i += 1\n    }}\n}}\n"
            ),
            unit_call,
        ),
        (
            "tail_if_else_value",
            format!(
                "fn reg(mut fs: Vec<fn() -> int>, k: int) -> int {{\n    let mut n = k\n    if k > 0 {{\n        {PUSH}\n        1\n    }} else {{\n        0\n    }}\n}}\n"
            ),
            value_call,
        ),
        (
            "generic_tail",
            format!(
                "fn reg<T>(mut fs: Vec<fn() -> int>, k: int, seed: T) -> T {{\n    let mut n = k\n    {PUSH}\n    seed\n}}\n"
            ),
            "let _ = reg(fs, 5, true)",
        ),
        (
            "lambda_tail",
            format!(
                "let reg = fn(mut fs: Vec<fn() -> int>, k: int) {{\n    let mut n = k\n    {PUSH}\n}}\n"
            ),
            unit_call,
        ),
        (
            "lambda_value_tail",
            format!(
                "let reg = fn(mut fs: Vec<fn() -> int>, k: int) -> int {{\n    let mut n = k\n    {PUSH}\n    k\n}}\n"
            ),
            value_call,
        ),
    ]
}

#[test]
fn every_function_exit_path_closes_the_locals_its_closures_captured() {
    for (name, declarations, call) in exit_paths() {
        let source = exit_program(&declarations, call);
        for flags in MODES {
            assert_eq!(
                run_ok(&source, flags),
                ["607088"],
                "exit path {name}, flags {flags:?}\n{source}"
            );
        }
    }
}

#[test]
fn a_tail_returned_closure_does_not_overwrite_the_next_frame() {
    let counter = "fn make() -> fn(int) -> int {
    let mut calls = 0
    let f = fn(a: int) -> int {
        calls += 1
        calls * 100 + a
    }
    f
}
let f = make()
println(f(1))
println(f(2))
";
    assert_all_modes(counter, &["101", "202"]);
    let loop_counter = "fn make() -> fn() -> int {
    let mut n = 0
    let f = fn() -> int {
        n += 7
        n
    }
    f
}
fn drive(g: fn() -> int) -> int {
    let mut i = 0
    let mut iters = 0
    while i < 3 {
        let _x = g()
        i += 1
        iters += 1
    }
    iters
}
println(drive(make()))
";
    assert_all_modes(loop_counter, &["3"]);
    let type_confusion = "fn make() -> fn() -> int {
    let mut n = 0
    let f = fn() -> int {
        n += 7
        n
    }
    f
}
fn drive(g: fn() -> int) -> int {
    let v0 = vec![0, 1]
    let a = g()
    let b = g()
    a + b + v0[1]
}
println(drive(make()))
";
    assert_all_modes(type_confusion, &["22"]);
}

#[test]
fn returning_a_captured_local_by_value_gives_its_latest_value() {
    let source = "fn tail() -> int {
    let mut n = 1
    let g = fn() { n += 1 }
    g()
    g()
    n
}
fn tail_if(c: bool) -> int {
    let mut n = 10
    let g = fn() { n *= 3 }
    g()
    if c { n } else { n + 1 }
}
fn explicit() -> int {
    let mut n = 4
    let g = fn() { n -= 1 }
    g()
    return n
}
fn shared() -> (int, int) {
    let mut n = 0
    let inc = fn() { n += 5 }
    let get = fn() -> int { n }
    inc()
    (n, get())
}
let pair = shared()
println(tail())
println(tail_if(true))
println(tail_if(false))
println(explicit())
println(pair.0 * 100 + pair.1)
";
    assert_all_modes(source, &["3", "30", "31", "3", "505"]);
}

#[test]
fn a_closure_returned_by_a_closure_keeps_both_levels_of_state() {
    let source = "fn outer() -> fn() -> fn() -> int {
    let mut made = 0
    let mk = fn() -> fn() -> int {
        made += 1
        let mut n = made * 100
        let f = fn() -> int {
            n += 1
            n
        }
        f
    }
    mk
}
fn drive(g: fn() -> int) -> int {
    let v = vec![1, 2, 3]
    let mut t = 0
    let mut i = 0
    while i < 2 {
        t = t + g()
        i += 1
    }
    t * 10 + i + v[2] - 3
}
let mk = outer()
let a = mk()
let b = mk()
println(drive(a))
println(drive(b))
println(a())
";
    assert_all_modes(source, &["2032", "4032", "103"]);
}

fn successors(code: &[u32], ip: usize) -> (Vec<usize>, usize) {
    let instr = code[ip];
    let op = OpCode::from_u8((instr >> 24) as u8).expect("valid opcode");
    let width = 1 + op.extension_words();
    let next = ip + width;
    let relative = |base: usize, delta: i64| usize::try_from(base as i64 + delta).expect("target");
    let short_target = || relative(ip + 1, i64::from(decode_b(instr).2));
    let long_target = |at: usize, base: usize| {
        relative(base, i64::from(i32::from_ne_bytes(code[at].to_ne_bytes())))
    };
    let targets = match op {
        OpCode::Return | OpCode::Return0 => vec![],
        OpCode::Jump => vec![short_target()],
        OpCode::JumpLong => vec![long_target(ip + 1, ip + 2)],
        OpCode::JumpIf
        | OpCode::JumpIfNot
        | OpCode::ForLoopI
        | OpCode::ForLoopIInc
        | OpCode::WhileLoopLt
        | OpCode::StringForLoop
        | OpCode::VecForLoop
        | OpCode::ArrayForLoop => vec![next, short_target()],
        OpCode::JumpIfLong
        | OpCode::JumpIfNotLong
        | OpCode::ForLoopILong
        | OpCode::ForLoopIIncLong
        | OpCode::StringForLoopLong
        | OpCode::VecForLoopLong
        | OpCode::ArrayForLoopLong => vec![next, long_target(ip + 1, ip + 2)],
        OpCode::JumpIfWideLong | OpCode::JumpIfNotWideLong | OpCode::LoopWideLong => {
            vec![next, long_target(ip + 2, ip + 3)]
        }
        _ => vec![next],
    };
    (targets, next)
}

fn local_captures(function: &Function, instr: u32) -> Option<u16> {
    let op = OpCode::from_u8((instr >> 24) as u8)?;
    assert!(
        !matches!(
            op,
            OpCode::MakeClosureWide | OpCode::MakeClosureRegisterWide
        ),
        "scanner corpus must stay narrow"
    );
    if op != OpCode::MakeClosure {
        return None;
    }
    let (_, _, constant, _) = decode_a(instr);
    let nested = function.constants[usize::from(constant)]
        .as_nested_fn_marker()
        .expect("closure constant");
    function.nested_functions[nested]
        .upvalue_descriptors
        .iter()
        .filter(|descriptor| descriptor.is_local)
        .map(|descriptor| descriptor.index)
        .min()
}

fn close_register(code: &[u32], ip: usize) -> Option<u16> {
    let instr = code[ip];
    let op = OpCode::from_u8((instr >> 24) as u8)?;
    match op {
        OpCode::CloseUpvals => Some(u16::from(decode_a(instr).1)),
        OpCode::Wide if decode_a(instr).1 == OpCode::CloseUpvals as u8 => {
            Some(u16::try_from(code[ip + 1] >> 16).expect("wide register"))
        }
        _ => None,
    }
}

fn scan_function(function: &Function, path: &str, problems: &mut Vec<String>) {
    let code = function.bytecode.as_slice();
    let mut open: Vec<Option<Option<u16>>> = vec![None; code.len() + 1];
    let mut worklist = vec![(0usize, None::<u16>)];
    while let Some((ip, incoming)) = worklist.pop() {
        if ip >= code.len() {
            problems.push(format!("{path}: control falls off the end at {ip}"));
            continue;
        }
        let merged = match open[ip] {
            None => incoming,
            Some(previous) => match (previous, incoming) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            },
        };
        if open[ip] == Some(merged) {
            continue;
        }
        open[ip] = Some(merged);
        let mut state = merged;
        if let Some(lowest) = local_captures(function, code[ip]) {
            state = Some(state.map_or(lowest, |current| current.min(lowest)));
        }
        if let Some(from) = close_register(code, ip)
            && state.is_some_and(|lowest| lowest >= from)
        {
            state = None;
        }
        let op = OpCode::from_u8((code[ip] >> 24) as u8).expect("valid opcode");
        if matches!(op, OpCode::Return | OpCode::Return0)
            && let Some(lowest) = state
        {
            problems.push(format!(
                "{path}: {op:?} at {ip} leaves the upvalue of r{lowest} open"
            ));
        }
        let (targets, _) = successors(code, ip);
        for target in targets {
            worklist.push((target, state));
        }
    }
    let mut ip = 0;
    let mut last_reachable_terminates = false;
    while ip < code.len() {
        let op = OpCode::from_u8((code[ip] >> 24) as u8).expect("valid opcode");
        let (_, next) = successors(code, ip);
        if open[ip].is_some() {
            last_reachable_terminates = matches!(
                op,
                OpCode::Return | OpCode::Return0 | OpCode::Jump | OpCode::JumpLong
            );
        } else if close_register(code, ip).is_some() && next >= code.len() {
            problems.push(format!("{path}: dead {op:?} at {ip} ends the function"));
        }
        ip = next;
    }
    if !last_reachable_terminates {
        problems.push(format!(
            "{path}: the last reachable instruction does not return"
        ));
    }
    for (index, nested) in function.nested_functions.iter().enumerate() {
        let name = nested.name.clone().unwrap_or_else(|| format!("#{index}"));
        scan_function(nested, &format!("{path}/{name}"), problems);
    }
}

fn scanner_corpus() -> Vec<(String, String)> {
    let mut corpus: Vec<(String, String)> = [
        Context::Function,
        Context::Method,
        Context::Generic,
        Context::TopLambda,
        Context::LambdaInFunction,
    ]
    .into_iter()
    .enumerate()
    .map(|(index, context)| (format!("escape{index}"), escape_program(context)))
    .collect();
    for (name, declarations, call) in exit_paths() {
        corpus.push((name.to_string(), exit_program(&declarations, call)));
    }
    corpus
        .into_iter()
        .map(|(name, source)| (name, without_stdlib(&source)))
        .collect()
}

fn without_stdlib(source: &str) -> String {
    format!(
        "fn show(_value: int) {{}}\n{}",
        source.replace("println(", "show(")
    )
}

#[test]
fn every_return_of_every_compiled_function_closes_its_captured_locals() {
    let mut problems = Vec::new();
    let levels = [
        OptimizationLevel::None,
        OptimizationLevel::Basic,
        OptimizationLevel::Standard,
        OptimizationLevel::Aggressive,
    ];
    for (name, source) in scanner_corpus() {
        for level in levels {
            let mut pipeline = compilation_pipeline_with_opt(level);
            let function = pipeline
                .compile(Source::new("scan.aelys", &source))
                .unwrap_or_else(|error| panic!("{name} does not compile: {error:?}"));
            scan_function(&function, &format!("{name}@{level:?}"), &mut problems);
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn the_scanner_flags_a_return_that_leaves_an_upvalue_open() {
    let source = "fn make() -> fn() -> int {\n    let mut n = 0\n    let f = fn() -> int {\n        n += 1\n        n\n    }\n    return f\n}\nprintln(make()())\n";
    let mut pipeline = compilation_pipeline_with_opt(OptimizationLevel::None);
    let mut function = pipeline
        .compile(Source::new("scan.aelys", without_stdlib(source)))
        .expect("compiles");
    let make = function
        .nested_functions
        .iter_mut()
        .find(|nested| nested.name.as_deref() == Some("make"))
        .expect("make");
    let words = make.bytecode.as_slice().to_vec();
    let close = (0..words.len())
        .find(|&ip| close_register(&words, ip).is_some())
        .expect("make closes its upvalue");
    let mut patched = words.clone();
    patched[close] = (OpCode::Move as u32) << 24;
    make.bytecode = patched.into();
    let mut problems = Vec::new();
    scan_function(&function, "patched", &mut problems);
    assert!(
        problems
            .iter()
            .any(|problem| problem.contains("leaves the upvalue of r0 open")),
        "{problems:?}"
    );
}
