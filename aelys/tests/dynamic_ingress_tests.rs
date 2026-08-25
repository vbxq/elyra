use aelys::run;
use aelys_frontend::{lexer::Lexer, parser::Parser};
use aelys_sema::{InferType, TypeInference};
use aelys_syntax::Source;
use std::collections::{HashMap, HashSet};

fn parse(source_text: &str) -> (Vec<aelys_syntax::Stmt>, std::sync::Arc<Source>) {
    let source = Source::new("<dynamic-ingress>", source_text);
    let tokens = Lexer::with_source(source.clone())
        .scan()
        .expect("the probe source must lex");
    let statements = Parser::new(tokens, source.clone())
        .parse()
        .expect("the probe source must parse");
    (statements, source)
}

fn module_imports() -> (HashSet<String>, HashSet<String>, HashMap<String, InferType>) {
    let mut aliases = HashSet::new();
    aliases.insert("util".to_string());
    let mut globals = HashSet::new();
    globals.insert("util::helper".to_string());
    let mut signatures = HashMap::new();
    signatures.insert(
        "util::helper".to_string(),
        InferType::Function {
            params: vec![InferType::I64],
            ret: Box::new(InferType::I64),
        },
    );
    (aliases, globals, signatures)
}

fn infer_codes(source_text: &str) -> Vec<u16> {
    let (statements, source) = parse(source_text);
    let (aliases, globals, signatures) = module_imports();
    match TypeInference::infer_program_full_with_native_signatures(
        statements,
        source,
        aliases,
        globals,
        HashSet::new(),
        signatures,
        Default::default(),
    ) {
        Ok(_) => panic!("the source must be rejected by type inference: {source_text}"),
        Err(errors) => errors
            .into_iter()
            .map(|error| error.diagnostic_code())
            .collect(),
    }
}

fn infer_report(source_text: &str) -> String {
    let (statements, source) = parse(source_text);
    let (aliases, globals, signatures) = module_imports();
    match TypeInference::infer_program_full_with_native_signatures(
        statements,
        source,
        aliases,
        globals,
        HashSet::new(),
        signatures,
        Default::default(),
    ) {
        Ok(_) => "accepted".to_string(),
        Err(errors) => errors
            .into_iter()
            .map(|error| format!("E{:04} {}", error.diagnostic_code(), error))
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

fn infer_accepts(source_text: &str) {
    let (statements, source) = parse(source_text);
    let (aliases, globals, signatures) = module_imports();
    if let Err(errors) = TypeInference::infer_program_full_with_native_signatures(
        statements,
        source,
        aliases,
        globals,
        HashSet::new(),
        signatures,
        Default::default(),
    ) {
        let rendered = errors
            .into_iter()
            .map(|error| format!("E{:04} {}", error.diagnostic_code(), error))
            .collect::<Vec<_>>()
            .join("\n");
        panic!("this legal program must still be accepted:\n{source_text}\n{rendered}");
    }
}

fn assert_reports(source_text: &str, code: u16) {
    let codes = infer_codes(source_text);
    assert!(
        codes.contains(&code),
        "expected E{code:04} for:\n{source_text}\ngot {codes:?}\n{}",
        infer_report(source_text)
    );
}

#[test]
fn a_module_alias_used_as_a_value_is_rejected() {
    assert_reports("let m = util\n1", 369);
}

#[test]
fn a_module_alias_returned_from_a_function_is_rejected() {
    assert_reports("fn grab() { util }\ngrab()\n1", 369);
}

#[test]
fn a_path_separator_on_a_local_value_is_rejected() {
    assert_reports("let x = 5\nx::foo", 370);
}

#[test]
fn an_unknown_member_on_an_integer_is_rejected() {
    assert_reports("let x = 5\nx.nope", 371);
}

#[test]
fn an_unknown_member_on_a_string_is_rejected() {
    assert_reports("let s = \"hi\"\ns.nope", 371);
}

#[test]
fn an_unknown_member_on_an_enum_value_is_rejected() {
    assert_reports("enum E { A, B }\nlet e = E::A\ne.foo", 371);
}

#[test]
fn an_unknown_struct_field_keeps_its_own_diagnostic() {
    let codes = infer_codes("struct P { x: int }\nlet p = P { x: 1 }\np.y");
    assert!(codes.contains(&363), "expected E0363, got {codes:?}");
    assert!(
        !codes.contains(&347),
        "the dynamic surface code must not appear, got {codes:?}"
    );
}

#[test]
fn an_unknown_type_name_in_an_annotation_is_rejected() {
    assert_reports("let x: widget = 5\nx", 372);
}

#[test]
fn an_unknown_type_name_in_a_parameter_is_rejected() {
    assert_reports("fn take(v: widget) -> int { 1 }\ntake(1)", 372);
}

#[test]
fn a_dot_on_a_module_alias_keeps_the_path_diagnostic() {
    let codes = infer_codes("util.helper(1)");
    assert!(codes.contains(&411), "expected E0411, got {codes:?}");
    assert!(
        !codes.contains(&369),
        "the namespace diagnostic must not pre-empt the path fix, got {codes:?}"
    );
}

#[test]
fn a_user_declaration_may_shadow_a_namespace_name() {
    infer_accepts("fn util() -> int { 7 }\nutil()");
}

#[test]
fn a_module_path_call_is_still_accepted() {
    infer_accepts("util::helper(1)");
}

#[test]
fn concrete_member_access_is_still_accepted() {
    infer_accepts(
        "struct P { x: int }\nimpl P { fn double(self) -> int { self.x * 2 } }\nlet p = P { x: 4 }\np.double() + p.x",
    );
}

#[test]
fn enum_paths_and_sum_values_are_still_accepted() {
    infer_accepts(
        "enum Shape { Circle, Square }\nlet s = Shape::Circle\nlet o = Option::Some(1)\nlet total = match o {\n    Option::Some(v) => v,\n    Option::None => 0,\n}\ntotal",
    );
}

#[test]
fn builtin_methods_on_primitives_are_still_accepted() {
    infer_accepts(
        "let s = \"hello\"\nlet n = s.len()\nlet v = [1, 2, 3]\nlet total = v[0] + n\nlet text = total.to_string()\ntext.len()",
    );
}

#[test]
fn a_program_using_every_repaired_path_still_runs() {
    let value = run(
        "struct P { x: int }\nimpl P { fn double(self) -> int { self.x * 2 } }\nenum Shape { Circle, Square }\nlet p = P { x: 4 }\nlet s = Shape::Circle\nlet mut total = p.double() + p.x\nfor item in [1, 2, 3] {\n    total = total + item\n}\ntotal",
        "test.aelys",
    )
    .expect("the repaired paths must not reject a legal program");
    assert_eq!(value.as_int(), Some(18));
}

#[test]
fn an_untyped_parameter_program_still_runs() {
    let value = run("fn add(a, b) { a + b }\nadd(2, 3)", "test.aelys")
        .expect("an untyped parameter program must still compile and run");
    assert_eq!(value.as_int(), Some(5));
}
