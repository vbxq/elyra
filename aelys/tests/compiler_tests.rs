use aelys_backend::Compiler;
use aelys_frontend::lexer::Lexer;
use aelys_frontend::parser::Parser;
use aelys_sema::{TypeInference, TypedExprKind, TypedPatternKind, TypedStmtKind};
use aelys_syntax::Source;

#[test]
fn compile_typed_program_to_bytecode() {
    let src = Source::new("<t>", "fn f() -> string { \"hi\" } f()");
    let tokens = Lexer::with_source(src.clone()).scan().unwrap();
    let ast = Parser::new(tokens, src.clone()).parse().unwrap();
    let typed = TypeInference::infer_program(ast, src.clone()).unwrap();

    let (func, _globals) = Compiler::new(None, src).compile_typed(&typed).unwrap();
    assert!(!func.bytecode.is_empty());
    assert!(func.nested_functions.iter().any(|nested| {
        nested
            .constants
            .iter()
            .any(|constant| constant.as_string() == Some("hi"))
    }));
}

#[test]
fn enum_patterns_carry_checked_variant_metadata() {
    let src = Source::new(
        "<enum-metadata>",
        r#"
enum Shape { Unit, Pair(int) }
match Shape::Pair(9) {
    Shape::Unit => 0,
    Shape::Pair(value) => value,
}
"#,
    );
    let tokens = Lexer::with_source(src.clone()).scan().unwrap();
    let ast = Parser::new(tokens, src.clone()).parse().unwrap();
    let typed = TypeInference::infer_program(ast, src).unwrap();
    let TypedStmtKind::Expression(match_expr) = &typed.stmts[1].kind else {
        panic!("expected the top-level match expression");
    };
    let TypedExprKind::Match { arms, .. } = &match_expr.kind else {
        panic!("expected a match expression");
    };
    let TypedPatternKind::Variant {
        enum_schema_index,
        enum_variant_index,
        ..
    } = &arms[1].pattern.kind
    else {
        panic!("expected the Pair variant pattern");
    };
    assert_eq!(*enum_schema_index, Some(0));
    assert_eq!(*enum_variant_index, Some(1));
}
