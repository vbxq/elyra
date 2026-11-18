mod common;
use common::*;

use aelys::{ExecutionOutcome, IsolateConfig, JitMode, RunOptions, Runtime};
use aelys_bytecode::{BytecodeBuffer, InstructionFormat, OpCode};
use aelys_opt::OptimizationLevel;
use aelys_runtime::Function;

fn compile_function(source: &str) -> Function {
    let src = aelys_syntax::Source::new("<test>", source);
    let tokens = aelys_frontend::lexer::Lexer::with_source(src.clone())
        .scan()
        .expect("lex");
    let stmts = aelys_frontend::parser::Parser::new(tokens, src.clone())
        .parse()
        .expect("parse");
    let typed = aelys_sema::TypeInference::infer_program(stmts, src.clone()).expect("types");
    aelys_backend::Compiler::new(None, src)
        .compile_typed(&typed)
        .expect("compile")
        .0
}

fn assemble_program(text: &str) -> Function {
    let mut functions = aelys_bytecode::asm::assemble(text).expect("assemble");
    let mut root = functions.remove(0);
    root.nested_functions = functions;
    root
}

fn run_avbc(bytes: &[u8]) -> Result<String, String> {
    let runtime = Runtime::with_jit_mode(JitMode::Off);
    let module = runtime
        .load_avbc(bytes, "<avbc>")
        .map_err(|error| error.to_string())?;
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    let outcome = isolate
        .execute(&module, RunOptions::default())
        .map_err(|error| error.to_string())?;
    let ExecutionOutcome::Returned(value) = outcome else {
        return Err(format!("no value: {outcome:?}"));
    };
    Ok(format!("{value:?}"))
}

fn with_word(function: &Function, index: usize, word: u32) -> Function {
    let mut words = function.bytecode.as_slice().to_vec();
    words[index] = word;
    let mut patched = function.clone();
    patched.bytecode = BytecodeBuffer::from_vec(words);
    patched
}

fn instruction_starts(words: &[u32]) -> Vec<bool> {
    let mut starts = vec![false; words.len() + 1];
    let mut ip = 0;
    while ip < words.len() {
        starts[ip] = true;
        let opcode = OpCode::from_u8((words[ip] >> 24) as u8).expect("opcode");
        ip += 1 + opcode.extension_words();
    }
    assert_eq!(ip, words.len(), "the walk must end on the last word");
    starts[words.len()] = true;
    starts
}

struct JumpSite {
    word: usize,
    short: bool,
    base: usize,
}

fn jump_sites(words: &[u32]) -> Vec<JumpSite> {
    let mut sites = Vec::new();
    let mut ip = 0;
    while ip < words.len() {
        let opcode = OpCode::from_u8((words[ip] >> 24) as u8).expect("opcode");
        let short = |word, base| JumpSite {
            word,
            short: true,
            base,
        };
        let long = |word, base| JumpSite {
            word,
            short: false,
            base,
        };
        match opcode {
            OpCode::Jump
            | OpCode::JumpIf
            | OpCode::JumpIfNot
            | OpCode::ForLoopI
            | OpCode::ForLoopIInc
            | OpCode::WhileLoopLt
            | OpCode::StringForLoop
            | OpCode::VecForLoop
            | OpCode::ArrayForLoop => sites.push(short(ip, ip + 1)),
            OpCode::Wide
                if (words[ip] >> 16) & 0xff == u32::from(u8::from(OpCode::WhileLoopLt)) =>
            {
                sites.push(short(ip + 1, ip + 3));
            }
            _ => match opcode.format() {
                InstructionFormat::AOffset32 => sites.push(long(ip + 1, ip + 2)),
                InstructionFormat::RegisterOffset32 | InstructionFormat::WideRegisterOffset32 => {
                    sites.push(long(ip + 2, ip + 3));
                }
                _ => {}
            },
        }
        ip += 1 + opcode.extension_words();
    }
    sites
}

fn retargeted(function: &Function, site: &JumpSite, target: usize) -> Option<Function> {
    let words = function.bytecode.as_slice();
    let offset = i64::try_from(target).unwrap() - i64::try_from(site.base).unwrap();
    let word = if site.short {
        let offset = i16::try_from(offset).ok()?;
        (words[site.word] & 0xffff_0000) | u32::from(offset as u16)
    } else {
        i32::try_from(offset).ok()? as u32
    };
    Some(with_word(function, site.word, word))
}

fn assert_jump_targets_must_start_an_instruction(function: &Function, label: &str) {
    aelys_runtime::verify_emitted_function(function)
        .unwrap_or_else(|error| panic!("{label}: the unpatched function must verify: {error}"));
    let words = function.bytecode.as_slice();
    let starts = instruction_starts(words);
    let sites = jump_sites(words);
    for site in &sites {
        for target in 0..=words.len() {
            let Some(patched) = retargeted(function, site, target) else {
                continue;
            };
            let verdict = aelys_runtime::verify_emitted_function(&patched);
            if starts[target] {
                assert!(
                    verdict.is_ok(),
                    "{label}: jump at word {} to instruction start {target} must verify: {verdict:?}",
                    site.word
                );
            } else {
                let error = verdict.expect_err(&format!(
                    "{label}: jump at word {} into the middle of an instruction at {target} must be refused",
                    site.word
                ));
                assert!(error.contains("inside an instruction"), "{label}: {error}");
            }
        }
    }
    for nested in &function.nested_functions {
        assert_jump_targets_must_start_an_instruction(nested, label);
    }
}

fn count_sites(function: &Function) -> usize {
    jump_sites(function.bytecode.as_slice()).len()
        + function
            .nested_functions
            .iter()
            .map(count_sites)
            .sum::<usize>()
}

const JMP_END: &str = r#"
.version 3
.function 0
  .arity 0
  .registers 6
  .globals
    0: "f"
    1: "io::println"
  .constants
    0: func @1 "f"
    1: int 65536
  .code
    LoadK     r0, 0
    SetGlobalIdx 0, r0
    LoadK     r3, 1
    LoadK     r4, 1
    LoadBool  r5, false
    CallGlobal r2, 0, 3
    CallGlobal r1, 1, 1
    Return    r1
.function 1
  .name "f"
  .arity 3
  .registers 60000
  .code
    JumpLong L0
    Return r0
  L0:
    MulShr 2, 0, 16, r0, r0, r59904, r0
"#;

#[test]
fn c1_a_long_jump_into_the_last_operand_word_of_mul_shr_is_refused() {
    let program = assemble_program(JMP_END);
    let callee = &program.nested_functions[0];
    assert_eq!(
        callee.bytecode.as_slice()[1],
        1,
        "JumpLong skips the Return"
    );
    assert_eq!(callee.bytecode.as_slice()[5], 0xEA00_0000);
    let mut patched = program.clone();
    patched.nested_functions[0] = with_word(callee, 1, 3);
    let bytes = aelys_bytecode::asm::serialize(&patched).expect("serialize");
    let refused = run_avbc(&bytes).expect_err("the jump lands on an operand word");
    assert!(refused.contains("inside an instruction"), "{refused}");
}

const EVERY_JUMP_FORM: &str = r#"
.version 3
.function 0
  .arity 0
  .registers 8
  .code
    LoadI r0, 0
    LoadI r1, 1
    MulShr 2, 0, 16, r3, r0, r1, r0
    ShlDiv 2, 1, 0, r3, r0, r1, r2
    JumpLong 0
    JumpIfLong r0, 0
    JumpIfNotLong r0, 0
    JumpIfWideLong r0, 0
    JumpIfNotWideLong r0, 0
    LoopWideLong ForLoopILong, r0, 0
    ForLoopILong r0, 0
    ForLoopIIncLong r0, 0
    VecForLoopLong r0, 0
    Wide 48, r0, r0, r0
    CallWide r4, r5, 1
    Jump 0
    JumpIf r0, 0
    JumpIfNot r0, 0
    ForLoopI r0, 0
    ForLoopIInc r0, 0
    WhileLoopLt r0, 0
    Return r0
"#;

#[test]
fn c1_every_jump_form_must_land_on_an_instruction_start() {
    let program = assemble_program(EVERY_JUMP_FORM);
    assert_eq!(count_sites(&program), 16);
    assert_jump_targets_must_start_an_instruction(&program, "every jump form");
}

const COMPILED_SHAPES: &str = r#"
struct P { x: int, y: int }
enum Shape { Dot, Box { w: int, h: int }, Line(int) }
fn area(s: Shape) -> int {
    match s {
        Shape::Dot => 0,
        Shape::Box { w, h } => w * h,
        Shape::Line(n) => n,
    }
}
fn walk(v: Vec<int>, a: [int; 3], s: string) -> int {
    let mut total = 0
    for x in v { total += x }
    for x in a { if x > 1 { total += x } else { total -= 1 } }
    for c in s { total += 1 }
    for i in 0..10 { total += i }
    for i in 0..=10 { total += i }
    let mut k = 0
    while k < 5 { k += 1 }
    let p = P { x: total, y: k }
    let f = fn(q: int) -> int { q + p.y }
    let fixed: i32 = 1000
    f(p.x) + area(Shape::Box { w: 2, h: 3 }) + fixed.mul_shr(3, 1) as int
}
walk(vec![1, 2, 3], [1, 2, 3], "abc")
"#;

#[test]
fn c1_compiled_programs_reject_every_jump_into_an_instruction_body() {
    let program = compile_function(COMPILED_SHAPES);
    assert!(count_sites(&program) >= 10, "{}", count_sites(&program));
    assert_jump_targets_must_start_an_instruction(&program, "compiled shapes");
}

fn run_marked_verified(words: Vec<u32>) -> String {
    let mut vm = aelys::new_vm().expect("vm");
    let mut function = Function::new(Some("truncated".to_string()), 0);
    function.num_registers = 8;
    function.bytecode = BytecodeBuffer::from_vec(words);
    let function = vm.alloc_function(function).expect("alloc");
    if let Some(object) = vm.heap_mut().get_mut(function)
        && let aelys_runtime::vm::ObjectKind::Function(inner) = &mut object.kind
    {
        inner.verified = true;
    }
    match vm.execute(function) {
        Ok(value) => format!("returned {value:?}"),
        Err(error) => error.to_string(),
    }
}

#[test]
fn c1_the_vm_refuses_a_multi_word_instruction_cut_by_the_end_of_the_bytecode() {
    let opcode = |op: OpCode, a: u8| (u32::from(u8::from(op)) << 24) | (u32::from(a) << 16);
    let cases = [
        opcode(OpCode::MulShr, 2),
        opcode(OpCode::ShlDiv, 2),
        opcode(OpCode::CallWide, 0),
        opcode(OpCode::ArrayLitWide, 0),
        opcode(OpCode::VecLitWide, 0),
        opcode(OpCode::JumpIfWideLong, 0),
        opcode(OpCode::JumpIfNotWideLong, 0),
        opcode(OpCode::MakeClosureRegisterWide, 0),
        opcode(OpCode::LoopWideLong, u8::from(OpCode::ForLoopILong)),
        opcode(OpCode::Wide, u8::from(OpCode::Move)),
        opcode(OpCode::StructNew, 0),
        opcode(OpCode::StructLoad, 0),
        opcode(OpCode::StructStore, 0),
        opcode(OpCode::EnumNew, 0),
        opcode(OpCode::EnumTest, 0),
        opcode(OpCode::EnumLoad, 0),
        opcode(OpCode::JumpLong, 0),
        opcode(OpCode::ForLoopILong, 0),
        opcode(OpCode::VecForLoopLong, 0),
        opcode(OpCode::LoadKWide, 0),
        opcode(OpCode::MakeClosureWide, 0),
        opcode(OpCode::GetGlobalIdxWide, 0),
        opcode(OpCode::SetGlobalIdxWide, 0),
    ];
    for word in cases {
        let op = OpCode::from_u8((word >> 24) as u8).unwrap();
        for present in 0..op.extension_words() {
            let mut words = vec![word];
            words.extend(std::iter::repeat_n(0, present));
            let error = run_marked_verified(words);
            assert!(
                error.contains("invalid bytecode") && error.contains("missing"),
                "{op:?} with {present} operand words: {error}"
            );
        }
    }
}

const LEVELS: [OptimizationLevel; 4] = [
    OptimizationLevel::None,
    OptimizationLevel::Basic,
    OptimizationLevel::Standard,
    OptimizationLevel::Aggressive,
];

fn run_at(source: &str, level: OptimizationLevel) -> Result<Option<i64>, String> {
    let mut vm = aelys::new_vm().expect("vm");
    aelys::run_with_vm_and_opt(&mut vm, source, "<test>", level)
        .map(|value| value.as_int())
        .map_err(|error| error.to_string())
}

fn assert_traps_at_every_level(source: &str, message: &str) {
    for level in LEVELS {
        let error = run_at(source, level).expect_err(&format!("{level:?} must trap on\n{source}"));
        assert!(error.contains(message), "{level:?}: {error}\n{source}");
    }
}

fn assert_int_at_every_level(source: &str, expected: i64) {
    for level in LEVELS {
        assert_eq!(
            run_at(source, level),
            Ok(Some(expected)),
            "{level:?}\n{source}"
        );
    }
}

#[test]
fn h1_folded_int_shift_left_traps_like_the_runtime() {
    for source in [
        "let a = 16 << 63\na",
        "16 << 63",
        "fn sh(a: int, b: int) -> int { a << b }\nsh(16, 63)",
        "@inline\nfn sh(a: int, b: int) -> int { a << b }\nsh(16, 63)",
        "fn c() -> int {\n    let r: int = (((-35) * 16171857) / (16 << 63))\n    r\n}\nc()",
        "fn c() -> int {\n    let r: int = (((23 * ((-140737488355328) << 27)) << 70368744177664) ^ (-18))\n    r\n}\nc()",
        "1 << 47",
        "(-1) << 48",
        "3 << 46",
        "let mut x = 1\nx = x << 60\nx",
    ] {
        assert_traps_at_every_level(source, "integer overflow");
    }
}

#[test]
fn h1_folded_int_shift_counts_are_masked_like_the_runtime() {
    let cases: [(&str, i64); 7] = [
        ("1 << 46", 1 << 46),
        ("(-1) << 47", -(1 << 47)),
        ("5 << 64", 5),
        ("5 << 65", 10),
        ("(-8) >> 1", -4),
        ("(-8) >> 65", -4),
        ("1024 >> -54", 1),
    ];
    for (source, expected) in cases {
        assert_int_at_every_level(source, expected);
    }
    assert_traps_at_every_level("1 << -1", "integer overflow");
}

#[test]
fn m1_literals_take_the_sized_type_of_an_enum_payload() {
    let prelude = "enum E { A(u8), B { x: i16 } }\n";
    let cases: [(&str, i64); 8] = [
        (
            "let e = E::A(200)\nmatch e { E::A(v) => (v + v) as int, E::B { x } => 0 }",
            144,
        ),
        (
            "let f = E::B { x: -32768 }\nmatch f { E::A(v) => 0, E::B { x } => (x - 1) as int }",
            32767,
        ),
        (
            "fn mk() -> E { E::A(255) }\nmatch mk() { E::A(v) => (v + 1) as int, E::B { x } => 9 }",
            0,
        ),
        (
            "let v = vec![E::A(1), E::A(255)]\nlet mut t = 0\nfor e in v { t += match e { E::A(x) => x as int, E::B { x } => 0 } }\nt",
            256,
        ),
        (
            "struct S { e: E }\nimpl S { fn get(self) -> u8 { match self.e { E::A(v) => v + 6, E::B { x } => 0 } } }\nS { e: E::A(250) }.get() as int",
            0,
        ),
        (
            "let f = fn() -> E { E::B { x: 32767 } }\nmatch f() { E::A(v) => 0, E::B { x } => (x + 1) as int }",
            -32768,
        ),
        (
            "fn g<T>(t: T) -> E { E::A(128) }\nmatch g(1) { E::A(v) => (v + v) as int, E::B { x } => 0 }",
            0,
        ),
        (
            "let e = E::A(255 - 1)\nmatch e { E::A(v) => v as int, E::B { x } => 0 }",
            254,
        ),
    ];
    for (body, expected) in cases {
        assert_int_at_every_level(&format!("{prelude}{body}"), expected);
    }
    for (body, message) in [
        ("E::A(256)", "integer literal 256 does not fit in u8"),
        ("E::A(-1)", "integer literal -1 does not fit in u8"),
        (
            "E::B { x: 40000 }",
            "integer literal 40000 does not fit in i16",
        ),
    ] {
        let error = run_aelys_err(&format!("{prelude}{body}"));
        assert!(
            error.contains("E0301") && error.contains(message),
            "{error}"
        );
    }
}

#[test]
fn m2_explicit_generic_instantiations_type_their_literals() {
    let holder = "enum Holder<T> { Full(T), Empty }\n";
    let boxed = "struct Box<T> { value: T }\n";
    let named = "enum G<T> { P { v: T }, Q }\n";
    let cases: [(String, i64); 9] = [
        (
            format!(
                "{holder}let h = Holder::<u8>::Full(200)\nmatch h {{ Holder::Full(v) => (v + v) as int, Holder::Empty => 0 }}"
            ),
            144,
        ),
        (
            format!(
                "{holder}let h: Holder<u8> = Holder::Full(200)\nmatch h {{ Holder::Full(v) => (v + v) as int, Holder::Empty => 0 }}"
            ),
            144,
        ),
        (
            format!(
                "{holder}fn take(h: Holder<i8>) -> i8 {{ match h {{ Holder::Full(v) => v, Holder::Empty => 0 }} }}\n(take(Holder::Full(-128)) - 1) as int"
            ),
            127,
        ),
        (
            format!(
                "{holder}let h = Holder::Full(200)\nmatch h {{ Holder::Full(v) => v * 1000000, Holder::Empty => 0 }}"
            ),
            200000000,
        ),
        (
            format!("{boxed}let b: Box<u8> = Box {{ value: 200 }}\n(b.value + b.value) as int"),
            144,
        ),
        (
            format!("{boxed}let c = Box::<i8> {{ value: 100 }}\n(c.value + c.value) as int"),
            -56,
        ),
        (
            format!(
                "{boxed}fn mk() -> Box<u16> {{ Box {{ value: 65535 }} }}\n(mk().value + 1) as int"
            ),
            0,
        ),
        (
            format!(
                "{boxed}fn mk() -> Box<u16> {{\n    return Box {{ value: 65535 }}\n}}\n(mk().value + 2) as int"
            ),
            1,
        ),
        (format!("{named}let g: G<i8> = G::P {{ v: -128 }}\n7"), 7),
    ];
    for (source, expected) in cases {
        assert_int_at_every_level(&source, expected);
    }
    for (source, message) in [
        (
            format!("{holder}Holder::<u8>::Full(300)"),
            "integer literal 300 does not fit in u8",
        ),
        (
            format!("{holder}let h: Holder<i8> = Holder::Full(128)"),
            "integer literal 128 does not fit in i8",
        ),
        (
            format!("{boxed}let b: Box<u8> = Box {{ value: 300 }}"),
            "integer literal 300 does not fit in u8",
        ),
        (
            format!("{holder}let h: Holder<string> = Holder::Full(200)"),
            "expected string, found int",
        ),
        (
            format!("{named}let g: G<i8> = G::P {{ v: -129 }}"),
            "integer literal -129 does not fit in i8",
        ),
        (
            format!("{holder}fn take(h: Holder<u16>) -> int {{ 0 }}\ntake(Holder::Full(65536))"),
            "integer literal 65536 does not fit in u16",
        ),
    ] {
        let error = run_aelys_err(&source);
        assert!(
            error.contains("E0301") && error.contains(message),
            "{error}"
        );
    }
}

#[test]
fn l1_branch_literals_take_the_sized_type_of_a_sibling_or_the_context() {
    let cases: [(&str, i64); 8] = [
        (
            "enum E { A(u8), B { x: i16 } }\nlet e = E::A(200)\nmatch e { E::A(v) => v + v, E::B { x } => 0 } as int",
            144,
        ),
        (
            "enum E { A(u8), B { x: i16 } }\nlet f = E::B { x: -32768 }\nlet g: i16 = match f { E::A(v) => 0, E::B { x } => x - 1 }\ng as int",
            32767,
        ),
        ("let w: i16 = match 3 { 1 => 5, _ => -7 }\nw as int", -7),
        (
            "let y: u8 = 7\nlet z = match 3 { 1 => 0, _ => y }\n(z - 8) as int",
            255,
        ),
        (
            "fn g(c: int) -> u8 { match c { 1 => 9, _ => 0 } }\n(g(2) - 1) as int",
            255,
        ),
        (
            "let y: u8 = 7\nlet z = if true { 250 } else { y }\n(z + z) as int",
            244,
        ),
        (
            "let x: u8 = 3\nlet z: u8 = if false { x } else { 0 }\n(z - 1) as int",
            255,
        ),
        ("let q: u8 = 200\nmatch q { 200 => 1, _ => 2 }", 1),
    ];
    for (source, expected) in cases {
        assert_int_at_every_level(source, expected);
    }
    for (source, message) in [
        (
            "let y: u8 = 7\nlet z = match 3 { 1 => 300, _ => y }",
            "integer literal 300 does not fit in u8",
        ),
        (
            "fn g(c: int) -> u8 { match c { 1 => 9, _ => 300 } }",
            "integer literal 300 does not fit in u8",
        ),
        (
            "let y: i8 = 7\nlet z = if true { y } else { -129 }",
            "integer literal -129 does not fit in i8",
        ),
    ] {
        let error = run_aelys_err(source);
        assert!(
            error.contains("E0301") && error.contains(message),
            "{error}"
        );
    }
}
