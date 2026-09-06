use aelys::run_with_config_and_opt;
use aelys_backend::Compiler;
use aelys_bytecode::{BytecodeBuffer, OpCode, Register};
use aelys_frontend::lexer::Lexer;
use aelys_frontend::parser::Parser;
use aelys_opt::{OptimizationLevel, Optimizer};
use aelys_runtime::{Function, GlobalLayout, VM, Value, VmConfig};
use aelys_sema::TypeInference;
use aelys_syntax::Source;
use std::sync::Arc;

#[test]
fn global_layout_interns_by_names() {
    let a = GlobalLayout::new(vec!["alpha".to_string(), "beta".to_string()]);
    let b = GlobalLayout::new(vec!["alpha".to_string(), "beta".to_string()]);
    assert!(Arc::ptr_eq(&a, &b));
    assert_eq!(a.id(), b.id());
}

#[test]
fn global_layout_ids_unique_for_different_names() {
    let a = GlobalLayout::new(vec!["alpha".to_string()]);
    let b = GlobalLayout::new(vec!["beta".to_string()]);
    assert_ne!(a.id(), 0);
    assert_ne!(b.id(), 0);
    assert_ne!(a.id(), b.id());
}

#[test]
fn global_layout_empty_is_singleton() {
    let a = GlobalLayout::empty();
    let b = GlobalLayout::empty();
    assert!(Arc::ptr_eq(&a, &b));
    assert_eq!(a.id(), 0);
    assert_eq!(b.id(), 0);
}

const LEVELS: [OptimizationLevel; 2] = [OptimizationLevel::None, OptimizationLevel::Standard];

const COUNTER: &str = "struct Holder { c: Counter }
struct Counter { v: int }
impl Counter {
    fn peek(self) -> int { return self.v }
    fn twice(self) -> Counter { return Counter { v: self.v * 2 } }
    fn make(n: int) -> Counter { return Counter { v: n } }
}
";

fn value_at(source: &str, level: OptimizationLevel) -> Result<Option<i64>, String> {
    run_with_config_and_opt(source, "test.aelys", VmConfig::default(), Vec::new(), level)
        .map(|value| value.as_int())
        .map_err(|error| error.to_string())
}

fn with_counter(body: &str) -> String {
    format!("{COUNTER}{body}")
}

fn crossing_witnesses() -> Vec<(&'static str, String, i64)> {
    vec![
        (
            "a method call inside a lambda",
            with_counter(
                "fn probe(c: Counter) -> int {
    let g = fn() -> int { return c.peek() }
    return g()
}
probe(Counter { v: 4 })",
            ),
            4,
        ),
        (
            "a method on a struct field",
            with_counter(
                "fn probe(b: Holder) -> int {
    let g = fn() -> int { return b.c.peek() }
    return g()
}
probe(Holder { c: Counter { v: 4 } })",
            ),
            4,
        ),
        (
            "a chained call",
            with_counter(
                "fn probe(c: Counter) -> int {
    let g = fn() -> int { return c.twice().peek() }
    return g()
}
probe(Counter { v: 4 })",
            ),
            8,
        ),
        (
            "a receiver that is the lambda parameter",
            with_counter(
                "fn probe(c: Counter) -> int {
    let g = fn(x: Counter) -> int { return x.peek() }
    return g(c)
}
probe(Counter { v: 4 })",
            ),
            4,
        ),
        (
            "an associated call",
            with_counter(
                "fn probe(n: int) -> int {
    let g = fn() -> int { return Counter::make(n).v }
    return g()
}
probe(4)",
            ),
            4,
        ),
        (
            "a lambda inside a lambda",
            with_counter(
                "fn probe(c: Counter) -> int {
    let g = fn() -> int {
        let h = fn() -> int { return c.peek() }
        return h()
    }
    return g()
}
probe(Counter { v: 4 })",
            ),
            4,
        ),
        (
            "a trait default method",
            "struct Counter { v: int }
trait Source {
    fn peek(self) -> int;
    fn doubled(self) -> int { return self.peek() * 2 }
}
impl Source for Counter {
    fn peek(self) -> int { return self.v }
}
fn probe(c: Counter) -> int {
    let g = fn() -> int { return c.doubled() }
    return g()
}
probe(Counter { v: 4 })"
                .to_string(),
            8,
        ),
        (
            "a free function call inside a lambda",
            "fn helper(n: int) -> int { return n + 1 }
fn probe(x: int) -> int {
    let g = fn() -> int { return helper(x) }
    return g()
}
probe(1)"
                .to_string(),
            2,
        ),
        (
            "a method call with no lambda at all",
            with_counter(
                "fn getter(c: Counter) -> int { return c.peek() }
fn apply(f, c: Counter) -> int { return f(c) }
apply(getter, Counter { v: 7 })",
            ),
            7,
        ),
        (
            "a later top level call after such a chain",
            with_counter(
                "fn getter(c: Counter) -> int { return c.peek() }
fn apply(f, c: Counter) -> int { return f(c) }
fn direct(c: Counter) -> int { return c.peek() }
let a = direct(Counter { v: 1 })
let b = apply(getter, Counter { v: 20 })
let d = direct(Counter { v: 300 })
a + b + d",
            ),
            321,
        ),
        (
            "a top level let mut read inside a lambda",
            "let mut total = 10
fn probe(x: int) -> int {
    let g = fn() -> int { return total + x }
    return g()
}
probe(1)"
                .to_string(),
            11,
        ),
        (
            "a top level let mut read after an intervening call",
            "let mut total = 10
fn side(n: int) -> int { return n }
fn probe(x: int) -> int {
    let k = side(0)
    let g = fn() -> int { return total + x }
    return g() + k
}
probe(1)"
                .to_string(),
            11,
        ),
    ]
}

fn already_passing_witnesses() -> Vec<(&'static str, String, i64)> {
    vec![
        (
            "an enum variant construction in a lambda",
            "enum Shape { Dot, Pair(int, int) }
fn probe(n: int) -> int {
    let g = fn() -> Shape { return Shape::Pair(n, n) }
    let s = g()
    match s {
        Shape::Dot => { return 0 }
        Shape::Pair(a, b) => { return a + b }
    }
}
probe(3)"
                .to_string(),
            6,
        ),
        (
            "a field access in a lambda",
            with_counter(
                "fn probe(c: Counter) -> int {
    let g = fn() -> int { return c.v }
    return g()
}
probe(Counter { v: 5 })",
            ),
            5,
        ),
        (
            "a lambda called at top level",
            with_counter(
                "let g = fn(c: Counter) -> int { return c.peek() }
g(Counter { v: 4 })",
            ),
            4,
        ),
        (
            "a lambda passed as an argument",
            with_counter(
                "fn apply(f, c: Counter) -> int { return f(c) }
fn probe(c: Counter) -> int { return apply(fn(x: Counter) -> int { return x.peek() }, c) }
probe(Counter { v: 4 })",
            ),
            4,
        ),
        (
            "a method called both outside and inside a lambda",
            with_counter(
                "fn probe(c: Counter) -> int {
    let a = c.peek()
    let g = fn() -> int { return c.peek() }
    return a + g()
}
probe(Counter { v: 3 })",
            ),
            6,
        ),
        (
            "a top level lambda reading a top level let mut",
            "let mut total = 10
let g = fn(x: int) -> int { return total + x }
g(1)"
                .to_string(),
            11,
        ),
    ]
}

#[test]
fn a_call_crossing_a_global_layout_boundary_resolves_the_callee_globals() {
    for (name, source, expected) in crossing_witnesses() {
        for level in LEVELS {
            assert_eq!(
                value_at(&source, level),
                Ok(Some(expected)),
                "{name} must run at {level:?}"
            );
        }
    }
}

#[test]
fn the_shapes_that_already_ran_keep_running() {
    for (name, source, expected) in already_passing_witnesses() {
        for level in LEVELS {
            assert_eq!(
                value_at(&source, level),
                Ok(Some(expected)),
                "{name} must keep running at {level:?}"
            );
        }
    }
}

// opcode cannot use the unchecked accessor.
#[test]
fn a_non_int_global_reaching_the_int_specialised_add_is_a_structured_error() {
    let source = "let mut total = 0
fn bump(x: int) -> int {
    total = total + x
    return total
}
bump(1)";
    for level in LEVELS {
        let (src, function) = compile_at(source, level);
        let mut vm = VM::new(src).expect("the VM must start");
        let root = vm.alloc_function(function).expect("the root must allocate");
        vm.execute(root).expect("the program must run as written");
        vm.set_global("total".to_string(), Value::bool(true));
        let error = vm
            .call_function_by_name("bump", &[Value::int(1)])
            .expect_err("a bool global must not reach the unchecked accessor")
            .to_string();
        assert!(
            error.contains("expected int") && error.contains("Bool"),
            "the failure must name what was expected and what was found, got {error} at {level:?}"
        );
    }
}

// a module can carry a global index its own layout never declared, and the
#[test]
fn a_global_index_past_the_declared_layout_is_rejected_before_it_runs() {
    let source = "let mut total = 7
fn read(x: int) -> int { return total + x }
read(1)";
    for level in LEVELS {
        let (src, mut function) = compile_at(source, level);
        let (declared, forged) =
            patch_first_global_read(&mut function).expect("the source must read a global by index");
        let mut vm = VM::new(src).expect("the VM must start");
        let root = vm.alloc_function(function).expect("the root must allocate");
        let error = vm
            .execute(root)
            .expect_err("an index past the declared layout must not reach the mapping")
            .to_string();
        assert!(
            error.contains(&format!("below the declared {declared}"))
                && error.contains(&format!("found {forged}")),
            "the rejection must name what was expected and what was found, got {error} at {level:?}"
        );
    }
}

// index the projection never reached instead of substituting null.
#[test]
fn a_global_read_past_the_projection_raises_instead_of_reading_null() {
    let cases = [
        ("GetGlobalIdx", narrow_global_read()),
        ("GetGlobalIdxWide", wide_index_global_read()),
        ("wide GetGlobalIdx", wide_register_global_read()),
    ];
    for (name, function) in cases {
        let mut vm = VM::new(Source::new("test.aelys", "")).expect("the VM must start");
        let root = vm.alloc_function(function).expect("the root must allocate");
        let error = vm
            .execute(root)
            .expect_err("an unmapped global index must not read as null")
            .to_string();
        assert!(
            error.contains("below the mapped 0") && error.contains("found 9"),
            "{name} must name what was expected and what was found, got {error}"
        );
    }
}

fn narrow_global_read() -> Function {
    let mut function = Function::new(Some("narrow".to_string()), 0);
    function.num_registers = 1;
    function.emit_b(OpCode::GetGlobalIdx, 0, 9, 1);
    function.emit_a(OpCode::Return, 0, 0, 0, 1);
    function.finalize_bytecode();
    function
}

fn wide_index_global_read() -> Function {
    let mut function = Function::new(Some("wide_index".to_string()), 0);
    function.num_registers = 1;
    function.emit_index32(OpCode::GetGlobalIdxWide, 0, 9, 1);
    function.emit_a(OpCode::Return, 0, 0, 0, 1);
    function.finalize_bytecode();
    function
}

fn wide_register_global_read() -> Function {
    let mut function = Function::new(Some("wide_register".to_string()), 0);
    function.num_registers = 301;
    function.emit_register_abc(
        OpCode::GetGlobalIdx,
        Register::new(300),
        Register::new(0),
        Register::new(9),
        1,
    );
    function.emit_a(OpCode::Return, 0, 0, 0, 1);
    function.finalize_bytecode();
    function
}

fn patch_first_global_read(function: &mut Function) -> Option<(usize, usize)> {
    let declared = function.global_layout.names().len();
    if declared > 0 {
        let forged = declared + 3;
        let mut words = function.bytecode.as_slice().to_vec();
        if let Some(word) = words
            .iter_mut()
            .find(|word| u8::try_from(**word >> 24) == Ok(OpCode::GetGlobalIdx as u8))
        {
            *word = (*word & 0xffff_0000) | u32::try_from(forged).expect("the index fits a word");
            function.bytecode = BytecodeBuffer::from_vec(words);
            return Some((declared, forged));
        }
    }
    function
        .nested_functions
        .iter_mut()
        .find_map(patch_first_global_read)
}

fn compile_at(source: &str, level: OptimizationLevel) -> (Arc<Source>, Function) {
    let src = Source::new("test.aelys", source);
    let tokens = Lexer::with_source(src.clone())
        .scan()
        .expect("the source must lex");
    let statements = Parser::new_rust_collections(tokens, src.clone())
        .parse()
        .expect("the source must parse");
    let typed =
        TypeInference::infer_program(statements, src.clone()).expect("the source must type check");
    let typed = Optimizer::new(level).optimize(typed);
    let (function, _globals) = Compiler::new(None, src.clone())
        .compile_typed(&typed)
        .expect("the source must compile");
    (src, function)
}
