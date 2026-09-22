use aelys_backend::Compiler;
use aelys_bytecode::asm::disassemble;
use aelys_frontend::lexer::Lexer;
use aelys_frontend::parser::Parser;
use aelys_opt::{OptimizationLevel, Optimizer};
use aelys_sema::TypeInference;
use aelys_syntax::Source;

fn assembly_of(source: &str) -> String {
    let src = Source::new("<index>", source);
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

fn body_of(assembly: &str, name: &str) -> String {
    let marker = format!(".name \"{name}\"");
    let start = assembly
        .find(&marker)
        .unwrap_or_else(|| panic!("function {name} is in the assembly:\n{assembly}"));
    let rest = &assembly[start..];
    let end = rest.find("; !== FUN").unwrap_or(rest.len());
    rest[..end].to_string()
}

/// reading a vec through registers that already hold the vec and the index costs one instruction
#[test]
fn reading_an_element_uses_the_registers_the_operands_already_sit_in() {
    let assembly = assembly_of(
        r#"
fn read(i: int) -> int {
    let a = vec![10, 20, 30]
    return a[i]
}
read(1)
"#,
    );
    let body = body_of(&assembly, "read");
    assert!(
        body.contains("VecLoadI"),
        "must read through the vec opcode"
    );
    assert!(
        !body.contains("Move"),
        "neither operand needs a copy:\n{body}"
    );
}

#[test]
fn writing_an_element_uses_the_registers_the_operands_already_sit_in() {
    let assembly = assembly_of(
        r#"
fn write(i: int, v: int) -> int {
    let mut a = vec![10, 20, 30]
    a[i] = v
    return a[0]
}
write(1, 7)
"#,
    );
    let body = body_of(&assembly, "write");
    assert!(
        body.contains("VecStoreI"),
        "must write through the vec opcode"
    );
    assert!(
        body.matches("Move").count() <= 1,
        "only the value of the assignment may be copied, for its own result:\n{body}"
    );
}

#[test]
fn an_index_that_must_be_computed_still_gets_its_own_register() {
    let assembly = assembly_of(
        r#"
fn read(i: int) -> int {
    let a = vec![10, 20, 30]
    return a[i + 1]
}
read(0)
"#,
    );
    let body = body_of(&assembly, "read");
    assert!(
        body.contains("VecLoadI"),
        "must read through the vec opcode"
    );
    assert!(
        body.contains("AddI"),
        "the computed index is built before the read:\n{body}"
    );
}
