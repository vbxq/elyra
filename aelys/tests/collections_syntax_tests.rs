use aelys_frontend::lexer::Lexer;
use aelys_frontend::parser::Parser;
use aelys_syntax::{Expr, ExprKind, Source, StmtKind, TokenKind};
fn parse(source: &str) -> Vec<aelys_syntax::Stmt> {
    let source = Source::new("<collections-syntax>", source);
    let tokens = Lexer::with_source(source.clone()).scan().unwrap();
    Parser::new_rust_collections(tokens, source)
        .parse()
        .unwrap()
}

fn parse_expression(source: &str) -> Expr {
    match &parse(source)[0].kind {
        StmtKind::Expression(expr) => expr.clone(),
        other => panic!("expected expression, got {other:?}"),
    }
}

fn parse_error(source: &str) -> String {
    let source = Source::new("<collections-syntax>", source);
    let tokens = Lexer::with_source(source.clone()).scan().unwrap();
    Parser::new_rust_collections(tokens, source)
        .parse()
        .unwrap_err()
        .to_string()
}

#[test]
fn lexer_distinguishes_bang_from_bang_eq() {
    let tokens = Lexer::new("vec![1] != vec![2]").scan().unwrap();
    let kinds: Vec<_> = tokens.into_iter().map(|token| token.kind).collect();
    assert!(matches!(kinds[1], TokenKind::Bang));
    assert!(matches!(kinds[5], TokenKind::BangEq));
}

#[test]
fn parser_builds_fixed_array_literal() {
    let expr = parse_expression("[1, 2, 3]");
    assert!(
        matches!(expr.kind, ExprKind::ArrayLiteral { ref elements, .. } if elements.len() == 3)
    );
}

#[test]
fn parser_builds_fixed_array_repeat() {
    let expr = parse_expression("[1; 3]");
    assert!(
        matches!(expr.kind, ExprKind::ArrayLiteral { ref elements, .. } if elements.len() == 1)
    );
    assert!(matches!(
        expr.repeat.as_deref().map(|value| &value.kind),
        Some(ExprKind::Int(3))
    ));
}

#[test]
fn parser_builds_vec_literals_and_repeats() {
    let literal = parse_expression("vec![1, 2]");
    assert!(
        matches!(literal.kind, ExprKind::VecLiteral { ref elements, .. } if elements.len() == 2)
    );

    let repeat = parse_expression("vec![1; 3]");
    assert!(
        matches!(repeat.kind, ExprKind::VecLiteral { ref elements, .. } if elements.len() == 1)
    );
    assert!(matches!(
        repeat.repeat.as_deref().map(|value| &value.kind),
        Some(ExprKind::Int(3))
    ));
}

#[test]
fn parser_builds_fixed_array_type_annotation() {
    let statements = parse("let values: [int; 3] = [1, 2, 3]");
    let StmtKind::Let {
        type_annotation: Some(annotation),
        ..
    } = &statements[0].kind
    else {
        panic!("expected typed let")
    };
    assert_eq!(annotation.name, "array");
    assert_eq!(annotation.array_length, Some(3));
    assert_eq!(annotation.type_params.len(), 1);
}

#[test]
fn parser_marks_read_only_collection_iteration() {
    let statements = parse("for x in &v { x }");
    let StmtKind::ForEach { iterable, .. } = &statements[0].kind else {
        panic!("expected foreach")
    };
    assert!(statements[0].read_only);
    assert!(matches!(iterable.kind, ExprKind::Identifier(ref name) if name == "v"));
}

#[test]
fn parser_rejects_legacy_collection_forms() {
    for source in [
        "[; 3]",
        "Array[1]",
        "Array[]",
        "Vec[1]",
        "Vec[]",
        "Array(3)",
        "array(3)",
        "array<int>(3)",
    ] {
        let error = parse_error(source);
        assert!(
            error.contains("legacy collection syntax"),
            "{source:?} produced unexpected diagnostic: {error}"
        );
    }
}
