use aelys::{call_function, new_vm, run_with_vm};
use aelys_driver::run_file;
use aelys_frontend::lexer::Lexer;
use aelys_frontend::parser::Parser;
use aelys_runtime::Value;
use aelys_syntax::{BinaryOp, ExprKind, MemberSeparator, Source, Stmt, StmtKind};

#[test]
fn while_condition_may_end_in_a_module_path_constant() {
    let mut vm = new_vm().expect("vm");
    run_with_vm(
        &mut vm,
        r#"
needs std::math
fn probe() -> int {
    let mut t: float = 0.0
    while t < math::PI {
        t = t + 1.0
    }
    7
}
"#,
        "<while-path-condition>",
    )
    .expect("a path constant must be allowed at the end of a while condition");

    assert_eq!(
        call_function(&mut vm, "probe", &[]).expect("probe"),
        Value::int(7)
    );
}

const IF_ENUM_CONDITION: &str = r#"
enum Color { Red, Blue }
fn probe(c: Color) -> int {
    if c == Color::Red {
        1
    } else {
        0
    }
}
fn on_red() -> int { probe(Color::Red) }
fn on_blue() -> int { probe(Color::Blue) }
"#;

#[test]
fn if_condition_may_end_in_an_enum_variant_path() {
    let mut vm = new_vm().expect("vm");
    run_with_vm(&mut vm, IF_ENUM_CONDITION, "<if-enum-condition>")
        .expect("an enum variant path must be allowed at the end of an if condition");

    call_function(&mut vm, "on_red", &[]).expect("on_red");
    call_function(&mut vm, "on_blue", &[]).expect("on_blue");
}

// the runtime cannot yet distinguish two enum values with `==`, so the branch
#[test]
fn the_enum_variant_condition_parses_as_a_comparison_and_a_block() {
    let source = Source::new("<if-enum-condition-ast>", IF_ENUM_CONDITION);
    let tokens = Lexer::with_source(source.clone()).scan().expect("scan");
    let stmts = Parser::new_rust_collections(tokens, source)
        .parse()
        .expect("parse");

    let StmtKind::Function(probe) = &stmts[1].kind else {
        panic!("expected the probe function, found {:?}", stmts[1].kind);
    };
    let StmtKind::If {
        condition,
        then_branch,
        else_branch,
    } = &probe.body[0].kind
    else {
        panic!("expected an if statement, found {:?}", probe.body[0].kind);
    };

    let ExprKind::Binary { op, right, .. } = &condition.kind else {
        panic!("expected a comparison, found {:?}", condition.kind);
    };
    assert_eq!(*op, BinaryOp::Eq);
    assert!(
        matches!(
            &right.kind,
            ExprKind::Member {
                separator: MemberSeparator::Path,
                member,
                ..
            } if member == "Red"
        ),
        "the condition must end in the plain path Color::Red, found {:?}",
        right.kind
    );

    assert_eq!(block_tail_int(then_branch), Some(1));
    assert_eq!(
        else_branch.as_deref().and_then(block_tail_int),
        Some(0),
        "the else block must survive the condition parse"
    );
}

fn block_tail_int(stmt: &Stmt) -> Option<i64> {
    let StmtKind::Block(stmts) = &stmt.kind else {
        return None;
    };
    match &stmts.last()?.kind {
        StmtKind::Expression(expr) => match expr.kind {
            ExprKind::Int(value) => Some(value),
            _ => None,
        },
        _ => None,
    }
}

#[test]
fn if_condition_may_end_in_a_module_path_constant() {
    let dir = tempfile::tempdir().expect("temp dir");
    std::fs::write(dir.path().join("limits.aelys"), "pub let COUNT = 3\n").expect("module");
    let main_path = dir.path().join("main.aelys");
    std::fs::write(
        &main_path,
        "needs limits\nfn probe(n: int) -> int {\n    if n == limits::COUNT {\n        1\n    } else {\n        0\n    }\n}\nprobe(3) * 10 + probe(4)\n",
    )
    .expect("main");

    let value =
        run_file(&main_path).expect("a path constant must be allowed at the end of a condition");
    assert_eq!(value, Value::int(10));
}

#[test]
fn match_scrutinee_may_end_in_an_enum_variant_path() {
    let mut vm = new_vm().expect("vm");
    run_with_vm(
        &mut vm,
        r#"
enum Color { Red, Blue }
fn probe() -> int {
    match Color::Blue {
        Color::Red => 1,
        Color::Blue => 2,
    }
}
"#,
        "<match-enum-scrutinee>",
    )
    .expect("an enum variant path must be allowed as a match scrutinee");

    assert_eq!(
        call_function(&mut vm, "probe", &[]).expect("probe"),
        Value::int(2)
    );
}

#[test]
fn for_header_may_end_in_a_module_path_constant() {
    let dir = tempfile::tempdir().expect("temp dir");
    std::fs::write(dir.path().join("limits.aelys"), "pub let COUNT = 3\n").expect("module");
    let main_path = dir.path().join("main.aelys");
    std::fs::write(
        &main_path,
        "needs limits\nlet mut total = 0\nfor i in 0..limits::COUNT {\n    total = total + 1\n}\ntotal\n",
    )
    .expect("main");

    let value = run_file(&main_path).expect("a path constant must be allowed in a for header");
    assert_eq!(value, Value::int(3));
}

#[test]
fn construction_still_parses_in_a_plain_value_position() {
    let mut vm = new_vm().expect("vm");
    run_with_vm(
        &mut vm,
        r#"
struct Point { x: int, y: int }
enum Shape { Circle { radius: int }, Square { side: int } }
fn probe() -> int {
    let p = Point { x: 3, y: 4 }
    let s = Shape::Circle { radius: 5 }
    let extra = match s {
        Shape::Circle { radius } => radius,
        Shape::Square { side } => side,
    }
    p.x + p.y + extra
}
"#,
        "<construction-value-position>",
    )
    .expect("brace construction must still work outside condition position");

    assert_eq!(
        call_function(&mut vm, "probe", &[]).expect("probe"),
        Value::int(12)
    );
}

#[test]
fn construction_inside_a_call_argument_inside_a_condition_still_parses() {
    let mut vm = new_vm().expect("vm");
    run_with_vm(
        &mut vm,
        r#"
struct Point { x: int, y: int }
fn below(p: Point) -> bool { p.x < 3 }
fn probe() -> int {
    let mut n = 0
    while below(Point { x: n, y: 0 }) {
        n = n + 1
    }
    n
}
"#,
        "<construction-in-call-argument>",
    )
    .expect("the restriction must lift inside a call argument list");

    assert_eq!(
        call_function(&mut vm, "probe", &[]).expect("probe"),
        Value::int(3)
    );
}

#[test]
fn construction_inside_an_index_bracket_inside_a_condition_still_parses() {
    let mut vm = new_vm().expect("vm");
    run_with_vm(
        &mut vm,
        r#"
struct Index { at: int }
fn slot(i: Index) -> int { i.at }
fn probe() -> int {
    let values = vec![10, 20, 30]
    let mut seen = 0
    if values[slot(Index { at: 2 })] == 30 {
        seen = 1
    }
    seen
}
"#,
        "<construction-in-index>",
    )
    .expect("the restriction must lift inside an index bracket");

    assert_eq!(
        call_function(&mut vm, "probe", &[]).expect("probe"),
        Value::int(1)
    );
}

#[test]
fn parenthesised_construction_is_the_escape_hatch_in_condition_position() {
    let mut vm = new_vm().expect("vm");
    run_with_vm(
        &mut vm,
        r#"
struct Flag { on: bool }
fn probe() -> int {
    if (Flag { on: true }).on {
        1
    } else {
        0
    }
}
"#,
        "<parenthesised-construction>",
    )
    .expect("a parenthesised construction must be accepted in condition position");

    assert_eq!(
        call_function(&mut vm, "probe", &[]).expect("probe"),
        Value::int(1)
    );
}

#[test]
fn a_nested_if_inside_a_condition_restores_the_restriction() {
    let dir = tempfile::tempdir().expect("temp dir");
    std::fs::write(dir.path().join("limits.aelys"), "pub let ZERO = 0\n").expect("module");
    let main_path = dir.path().join("main.aelys");
    std::fs::write(
        &main_path,
        "needs limits\nfn pick(n: int) -> bool { n == 1 }\nfn probe() -> int {\n    let mut total = 0\n    if pick(if total == limits::ZERO { 1 } else { 2 }) {\n        total = 5\n    }\n    total\n}\nprobe()\n",
    )
    .expect("main");

    let value = run_file(&main_path).expect(
        "a nested if inside a condition must re-apply the restriction to its own condition",
    );
    assert_eq!(value, Value::int(5));
}
