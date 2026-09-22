use aelys_backend::Compiler;
use aelys_bytecode::asm::disassemble;
use aelys_frontend::lexer::Lexer;
use aelys_frontend::parser::Parser;
use aelys_opt::{OptimizationLevel, Optimizer};
use aelys_sema::TypeInference;
use aelys_syntax::Source;

fn assembly_of(source: &str) -> String {
    let src = Source::new("<immediate>", source);
    let tokens = Lexer::with_source(src.clone()).scan().expect("lexes");
    let ast = Parser::new_rust_collections(tokens, src.clone())
        .parse()
        .expect("parses");
    let typed = TypeInference::infer_program(ast, src.clone()).expect("types");
    let mut optimizer = Optimizer::new(OptimizationLevel::Standard);
    let optimized = optimizer.optimize(typed);
    let (function, _globals) = Compiler::new(None, src)
        .compile_typed(&optimized)
        .expect("compiles");
    disassemble(&function)
}

/// a small literal on the right of an integer add or subtract becomes the operand of the instruction, which spares a register and a LoadI.
#[test]
fn an_integer_plus_a_small_literal_uses_the_immediate_instruction() {
    let assembly = assembly_of(
        r#"
fn bump(i: int) -> int {
    return i + 1
}
bump(3)
"#,
    );
    assert!(
        assembly.contains("AddI"),
        "the immediate form must be emitted, got:\n{assembly}"
    );
}

#[test]
fn an_integer_minus_a_small_literal_uses_the_immediate_instruction() {
    let assembly = assembly_of(
        r#"
fn bump(i: int) -> int {
    return i - 2
}
bump(3)
"#,
    );
    assert!(
        assembly.contains("SubI"),
        "the immediate form must be emitted, got:\n{assembly}"
    );
}

#[test]
fn a_literal_outside_the_immediate_range_keeps_the_register_form() {
    let assembly = assembly_of(
        r#"
fn bump(i: int) -> int {
    return i + 4096
}
bump(3)
"#,
    );
    assert!(
        assembly.contains("AddII"),
        "a literal that does not fit must stay in a register, got:\n{assembly}"
    );
}

#[test]
fn a_float_plus_a_small_literal_keeps_its_own_instruction() {
    let assembly = assembly_of(
        r#"
fn bump(x: float) -> float {
    return x + 1.0
}
bump(3.0)
"#,
    );
    assert!(
        !assembly.contains("AddI "),
        "the integer immediate must not be used on floats, got:\n{assembly}"
    );
}
