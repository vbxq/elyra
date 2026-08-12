use aelys::{CompileOptions, Runtime};
use aelys_frontend::{lexer::Lexer, parser::Parser};
use aelys_sema::TypeInference;
use aelys_syntax::Source;
use std::collections::HashSet;

fn compile_message(source: &str) -> String {
    match Runtime::new().compile(source, CompileOptions::default()) {
        Ok(_) => panic!("the source must be rejected"),
        Err(error) => error.to_string(),
    }
}

fn compile_ok(source: &str) {
    Runtime::new()
        .compile(source, CompileOptions::default())
        .expect("the source should compile");
}

fn infer_message(source_text: &str) -> String {
    let source = Source::new("<inference>", source_text);
    let tokens = Lexer::with_source(source.clone()).scan().unwrap();
    let statements = Parser::new(tokens, source.clone()).parse().unwrap();
    let errors =
        match TypeInference::infer_program_full(statements, source, HashSet::new(), HashSet::new())
        {
            Ok(_) => panic!("the source must be rejected by type inference"),
            Err(errors) => errors,
        };
    errors
        .into_iter()
        .map(|error| error.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn compile_untyped_native(source_text: &str) -> String {
    let source = Source::new("<native-boundary>", source_text);
    let tokens = Lexer::with_source(source.clone()).scan().unwrap();
    let statements = Parser::new(tokens, source.clone()).parse().unwrap();
    let mut aliases = HashSet::new();
    aliases.insert("custom".to_string());
    let mut globals = HashSet::new();
    globals.insert("custom::read".to_string());
    let mut natives = HashSet::new();
    natives.insert("custom::read".to_string());
    let result = TypeInference::infer_program_full_with_natives(
        statements, source, aliases, globals, natives,
    );
    let errors = match result {
        Ok(_) => panic!("an untyped native must not enter a sum operation"),
        Err(errors) => errors,
    };
    errors
        .into_iter()
        .map(|error| error.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn compile_untyped_native_annotation(source_text: &str) -> String {
    let source = Source::new("<native-annotation>", source_text);
    let tokens = Lexer::with_source(source.clone()).scan().unwrap();
    let statements = Parser::new(tokens, source.clone()).parse().unwrap();
    let mut aliases = HashSet::new();
    aliases.insert("custom".to_string());
    let mut globals = HashSet::new();
    globals.insert("custom::read".to_string());
    let mut natives = HashSet::new();
    natives.insert("custom::read".to_string());
    let errors = match TypeInference::infer_program_full_with_natives(
        statements, source, aliases, globals, natives,
    ) {
        Ok(_) => panic!("an untyped native must not satisfy an annotation"),
        Err(errors) => errors,
    };
    errors
        .into_iter()
        .map(|error| error.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn compile_untyped_native_ok(source_text: &str) {
    let source = Source::new("<native-boundary-ok>", source_text);
    let tokens = Lexer::with_source(source.clone()).scan().unwrap();
    let statements = Parser::new(tokens, source.clone()).parse().unwrap();
    let mut aliases = HashSet::new();
    aliases.insert("custom".to_string());
    let mut globals = HashSet::new();
    globals.insert("custom::read".to_string());
    let mut natives = HashSet::new();
    natives.insert("custom::read".to_string());
    TypeInference::infer_program_full_with_natives(statements, source, aliases, globals, natives)
        .expect("explicit dynamic must accept an untyped native value");
}

#[test]
fn missing_result_variant_has_a_named_diagnostic() {
    let message = compile_message(
        "fn read() -> Result<int, string> {\n    match Ok(1) {\n        Ok(value) => value\n    }\n}\nread()",
    );
    assert!(message.contains("non-exhaustive match"), "{message}");
    assert!(message.contains("Err"), "{message}");
}

#[test]
fn public_compile_errors_preserve_the_semantic_code() {
    let message = compile_message(
        "fn read() -> Result<int, string> {\n    match Ok(1) {\n        Ok(value) => value\n    }\n}\nread()",
    );
    assert!(message.contains("error[E0302]"), "{message}");
}

#[test]
fn non_exhaustive_match_red_is_isolated_from_must_use() {
    let message = compile_message(
        "let source: Result<int, string> = Ok(1)\nlet value = match source { Ok(number) => number }\nvalue",
    );
    assert!(message.contains("non-exhaustive match"), "{message}");
    assert!(!message.contains("unused Result"), "{message}");
}

#[test]
fn qualified_variant_from_the_wrong_sum_is_rejected() {
    let message =
        compile_message("match Some(1) { Result::Some(value) => value, Option::None => 0 }");
    assert!(message.contains("unknown variant"), "{message}");
}

#[test]
fn or_patterns_must_bind_the_same_names() {
    let message = compile_message("match Some(1) { Some(value) | None => value, Some(_) => 0 }");
    assert!(message.contains("bind the same names"), "{message}");
}

#[test]
fn ignored_result_has_a_named_diagnostic() {
    let message = compile_message(
        "fn read() -> Result<int, string> { Ok(1) }\nfn main() {\n    if true { read() }\n}\nmain()",
    );
    assert!(message.contains("unused Result"), "{message}");
}

#[test]
fn unused_result_binding_has_a_named_diagnostic() {
    let message =
        compile_message("fn read() -> Result<int, string> { Ok(1) }\nlet value = read()\n0");
    assert!(message.contains("unused Result"), "{message}");
}

#[test]
fn reassigning_a_result_cannot_discard_the_old_value() {
    let message = compile_message(
        "let mut value: Result<int, string> = Ok(1)\nlet _ = value.unwrap()\nlet _ = value = Ok(2)\n0",
    );
    assert!(message.contains("unused Result"), "{message}");
}

#[test]
fn an_assigned_result_can_be_used_by_the_assignment_expression() {
    compile_ok(
        "let mut value: Result<int, string> = Ok(1)\nlet _ = value.unwrap()\nlet _ = (value = Ok(2)).unwrap()\n0",
    );
}

#[test]
fn an_assigned_result_can_be_returned() {
    compile_ok(
        "fn update() -> Result<int, string> { let mut value: Result<int, string> = Ok(1)\nlet _ = value.unwrap()\nreturn value = Ok(2) }\nmatch update() { Ok(number) => number, Err(_) => 0 }",
    );
}

#[test]
fn implicit_unit_tail_cannot_discard_a_result() {
    let message = compile_message(
        "fn read() -> Result<int, string> { Ok(1) }\nfn discard() -> unit { read() }\ndiscard()",
    );
    assert!(message.contains("unused Result"), "{message}");
}

#[test]
fn question_mark_conversion_has_a_named_diagnostic() {
    let message = compile_message(
        "fn read() -> Result<int, string> { Ok(1) }\nfn convert() -> Result<int, int> {\n    let value = read()?\n    Ok(value)\n}\nconvert()",
    );
    assert!(message.contains("cannot propagate"), "{message}");
}

#[test]
fn null_is_rejected_with_an_option_or_result_fix() {
    let message = compile_message("null");
    assert!(message.contains("null is not part of Aelys"), "{message}");
    assert!(
        message.contains("Option") || message.contains("Result"),
        "{message}"
    );
}

#[test]
fn null_type_annotation_is_rejected_with_the_same_diagnostic() {
    let message = compile_message("let value: null = 0\nvalue");
    assert!(message.contains("null is not part of Aelys"), "{message}");
}

#[test]
fn null_pattern_is_rejected_with_the_same_diagnostic() {
    let message = compile_message("match Some(1) { null => 1, _ => 0 }");
    assert!(message.contains("null is not part of Aelys"), "{message}");
}

#[test]
fn null_introspection_is_not_a_surface_api() {
    let message = compile_message("needs std::convert\nconvert::is_null(1)");
    assert!(message.contains("null is not part of Aelys"), "{message}");
}

#[test]
fn ignored_option_has_a_named_diagnostic() {
    let message = compile_message(
        "fn read() -> Option<int> { Some(1) }\nfn main() { if true { read() } }\nmain()",
    );
    assert!(message.contains("unused Option"), "{message}");
}

#[test]
fn explicit_underscore_is_the_only_discard() {
    compile_ok("fn read() -> Result<int, string> { Ok(1) }\nfn main() { let _ = read() }\nmain()");
}

#[test]
fn non_unit_function_cannot_fall_through() {
    let message = compile_message("fn missing() -> int {}\nmissing()");
    assert!(message.contains("function can fall through"), "{message}");
}

#[test]
fn typed_lambda_cannot_fall_through() {
    let message =
        compile_message("let missing = fn() -> int {}\nlet value: int = missing()\nvalue");
    assert!(message.contains("function can fall through"), "{message}");
}

#[test]
fn untyped_native_cannot_be_propagated_with_question_mark() {
    let message = compile_untyped_native(
        "fn convert() -> Option<int> { let value = custom::read()?; Some(value) }\nconvert()",
    );
    assert!(message.contains("untyped native"), "{message}");
}

#[test]
fn mismatched_return_is_a_compile_error() {
    let message = compile_message("fn wrong() -> int { \"text\" }\nwrong()");
    assert!(message.contains("type mismatch"), "{message}");
}

#[test]
fn non_boolean_match_guard_is_a_compile_error() {
    let message =
        compile_message("match Some(1) { Some(value) if 1 => value, Some(_) => 0, None => 0 }");
    assert!(message.contains("type mismatch"), "{message}");
    assert!(message.contains("if condition"), "{message}");
}

#[test]
fn incompatible_match_arm_values_are_a_compile_error() {
    let message = compile_message("match Some(1) { Some(_) => 1, None => \"missing\" }");
    assert!(message.contains("type mismatch"), "{message}");
}

#[test]
fn question_mark_checks_the_success_type() {
    let message = compile_message(
        "fn read() -> Result<int, string> { Ok(1) }\nfn convert() -> Result<string, string> { let value = read()?; Ok(value) }\nconvert()",
    );
    assert!(message.contains("cannot propagate"), "{message}");
}

#[test]
fn untyped_native_cannot_satisfy_a_sum_annotation() {
    let message = compile_untyped_native_annotation(
        "let value: Option<int> = custom::read()\nmatch value { Some(_) => 1, None => 0 }",
    );
    assert!(message.contains("untyped native"), "{message}");
}

#[test]
fn explicit_dynamic_is_the_native_escape_hatch() {
    compile_untyped_native_ok("let value: dynamic = custom::read()\n0");
}

#[test]
fn dynamic_value_cannot_satisfy_a_concrete_annotation() {
    let message = compile_message("let value: dynamic = \"text\"\nlet number: int = value\nnumber");
    assert!(message.contains("type mismatch"), "{message}");
}

#[test]
fn dynamic_payload_cannot_satisfy_a_concrete_sum_annotation() {
    let message = compile_message(
        "let value: dynamic = 1\nlet option: Option<int> = Some(value)\nmatch option { Some(number) => number, None => 0 }",
    );
    assert!(message.contains("type mismatch"), "{message}");
}

#[test]
fn sized_object_arrays_are_rejected_before_they_can_expose_null() {
    let message = compile_message("let values = Array<String>(1)\n0");
    assert!(message.contains("sized array"), "{message}");
}

#[test]
fn dynamic_callback_cannot_satisfy_a_concrete_map_result() {
    let message = compile_message(
        "fn convert(value: int) -> dynamic { \"bad\" }\nSome(1).map(convert).unwrap() + 1",
    );
    assert!(message.contains("type mismatch"), "{message}");
}

#[test]
fn dynamic_values_cannot_use_sum_methods_without_a_sum_type() {
    let message = compile_message(
        "fn read() -> Result<int, string> { Ok(1) }\nlet value: dynamic = read()\nvalue.unwrap()",
    );
    assert!(
        message.contains("dynamic value cannot use sum method"),
        "{message}"
    );
}

#[test]
fn dynamic_value_cannot_satisfy_a_sum_method_argument() {
    let message = compile_message(
        "fn read() -> Option<int> { Some(1) }\nlet fallback: dynamic = 0\nread().unwrap_or(fallback)",
    );
    assert!(message.contains("type mismatch"), "{message}");
}

#[test]
fn untyped_native_cannot_cross_a_typed_operator_boundary() {
    let message = compile_untyped_native("custom::read() == 1");
    assert!(message.contains("untyped native"), "{message}");
}

#[test]
fn standard_native_signature_rejects_wrong_math_argument() {
    let message = compile_message("needs std::math\nmath::sqrt(\"bad\")");
    assert!(message.contains("not one of"), "{message}");
}

#[test]
fn numeric_return_cannot_satisfy_a_concrete_integer_return() {
    let message = compile_message("fn wrong() -> int { math::abs(-1.2) }\nwrong() + 1");
    assert!(message.contains("type mismatch"), "{message}");
}

#[test]
fn dynamic_if_branch_cannot_satisfy_a_concrete_return() {
    let message = compile_message(
        "let value: dynamic = \"bad\"\nfn wrong() -> int { if true { value } else { 1 } }\nwrong()",
    );
    assert!(message.contains("type mismatch"), "{message}");
}

#[test]
fn dynamic_match_branch_cannot_satisfy_a_concrete_return() {
    let message = compile_message(
        "let value: dynamic = \"bad\"\nfn wrong() -> int { match true { true => value, false => 1 } }\nwrong()",
    );
    assert!(message.contains("type mismatch"), "{message}");
}

#[test]
fn dynamic_array_element_cannot_satisfy_a_concrete_array() {
    let message = compile_message(
        "let value: dynamic = \"bad\"\nlet numbers: Array<int> = [1, value]\nnumbers[1] + 1",
    );
    assert!(message.contains("type mismatch"), "{message}");
}

#[test]
fn dynamic_arithmetic_cannot_satisfy_a_concrete_return() {
    let message =
        compile_message("let value: dynamic = \"bad\"\nfn wrong() -> int { value + 1 }\nwrong()");
    assert!(message.contains("type mismatch"), "{message}");
}

#[test]
fn dynamic_arithmetic_is_rejected_before_execution() {
    let message = compile_message("let value: dynamic = \"bad\"\nvalue + 1");
    assert!(message.contains("dynamic"), "{message}");
    assert!(message.contains("binary operator '+'"), "{message}");
}

#[test]
fn dynamic_unary_numeric_operators_are_rejected_before_execution() {
    for source in [
        "let value: dynamic = \"bad\"\n-value",
        "let value: dynamic = \"bad\"\n~value",
    ] {
        let message = compile_message(source);
        assert!(message.contains("dynamic"), "{message}");
        assert!(message.contains("operator"), "{message}");
    }
}

#[test]
fn dynamic_equality_is_rejected_before_execution() {
    let message = compile_message("let value: dynamic = \"bad\"\nvalue == 1");
    assert!(message.contains("dynamic"), "{message}");
    assert!(message.contains("comparison"), "{message}");
}

#[test]
fn dynamic_if_condition_is_a_compile_error() {
    let message = compile_message("let value: dynamic = \"bad\"\nif value { 1 } else { 0 }");
    assert!(message.contains("type mismatch"), "{message}");
}

#[test]
fn dynamic_ordering_operand_is_a_compile_error() {
    let message = compile_message("let value: dynamic = \"bad\"\nvalue > 0");
    assert!(message.contains("type mismatch"), "{message}");
}

#[test]
fn inferred_not_operand_must_be_boolean() {
    let message = compile_message("fn invert(value) -> bool { not value }\ninvert(1)");
    assert!(message.contains("type mismatch"), "{message}");
}

#[test]
fn dynamic_implicit_return_condition_is_a_compile_error() {
    let message = compile_message(
        "let value: dynamic = \"bad\"\nfn wrong() -> int { if value { 1 } else { 0 } }\nwrong()",
    );
    assert!(message.contains("type mismatch"), "{message}");
}

#[test]
fn dynamic_index_cannot_satisfy_a_concrete_index() {
    let message = compile_message("let index: dynamic = 0\n[1][index]");
    assert!(message.contains("type mismatch"), "{message}");
}

#[test]
fn dynamic_index_receiver_is_a_compile_error() {
    let message = compile_message("let value: dynamic = 1\nvalue[0]");
    assert!(message.contains("cannot index"), "{message}");
}

#[test]
fn dynamic_index_assignment_receiver_is_a_compile_error() {
    let message = compile_message("let value: dynamic = 1\nvalue[0] = 2\n0");
    assert!(message.contains("cannot index"), "{message}");
}

#[test]
fn dynamic_for_each_receiver_is_a_compile_error() {
    let message = compile_message("let values: dynamic = 1\nfor value in values { value }\n0");
    assert!(message.contains("cannot iterate"), "{message}");
}

#[test]
fn scalar_collection_method_receiver_is_a_compile_error() {
    let message = compile_message("fn length(value) -> int { value.len() }\nlength(1)");
    assert!(message.contains("collection"), "{message}");
}

#[test]
fn scalar_collection_push_receiver_is_a_compile_error() {
    let message = compile_message("fn push(value) { value.push(1) }\npush(1)");
    assert!(message.contains("collection"), "{message}");
}

#[test]
fn collection_diagnostics_hide_inference_variables() {
    let message = compile_message("fn push(value) { value.push(1) }\npush(1)");
    assert!(message.contains("vector"), "{message}");
    assert!(!message.contains("TypeVarId"), "{message}");
}

#[test]
fn dynamic_string_method_receiver_is_a_compile_error() {
    let message = compile_message("let value: dynamic = \"bad\"\nvalue.to_upper()");
    assert!(message.contains("string method"), "{message}");
}

#[test]
fn scalar_string_method_receiver_is_a_compile_error() {
    let message = compile_message("fn upper(value) { value.to_upper() }\nupper(1)");
    assert!(message.contains("string method"), "{message}");
}

#[test]
fn scalar_index_assignment_receiver_is_a_compile_error() {
    let message = compile_message("fn set(value) { value[0] = 1 }\nset(1)");
    assert!(message.contains("not one of"), "{message}");
}

#[test]
fn dynamic_size_cannot_satisfy_a_concrete_array_size() {
    let message =
        compile_message("let size: dynamic = 2\nlet values = Array<int>[; size]\nvalues.len()");
    assert!(message.contains("type mismatch"), "{message}");
}

#[test]
fn dynamic_match_guard_cannot_satisfy_a_boolean_guard() {
    let message = compile_message(
        "let guard: dynamic = true\nmatch Some(1) { Some(value) if guard => value, None => 0, Some(_) => 0 }",
    );
    assert!(message.contains("type mismatch"), "{message}");
}

#[test]
fn dynamic_error_message_cannot_satisfy_a_string_message() {
    let message = compile_message("let message: dynamic = 1\nError::Message(message)");
    assert!(message.contains("type mismatch"), "{message}");
}

#[test]
fn untyped_native_cannot_drive_a_guard() {
    let message = compile_untyped_native(
        "match Some(1) { Some(value) if custom::read() => value, None => 0 }",
    );
    assert!(message.contains("untyped native"), "{message}");
}

#[test]
fn untyped_native_cannot_call_a_sum_combinator() {
    let message = compile_untyped_native("custom::read().unwrap()");
    assert!(message.contains("untyped native"), "{message}");
}

#[test]
fn scalar_indexing_is_rejected() {
    let message = compile_message("1[0]");
    assert!(message.contains("cannot index"), "{message}");
}

#[test]
fn integer_iteration_is_rejected() {
    let message = compile_message("for value in 1 { value }");
    assert!(message.contains("cannot iterate"), "{message}");
}

#[test]
fn vector_assignment_checks_the_element_type() {
    let message = compile_message("let values: Vec<int> = Vec[1]\nvalues[0] = true\n0");
    assert!(message.contains("type mismatch"), "{message}");
}

#[test]
fn constant_index_out_of_bounds_is_rejected() {
    let message = compile_message("[1, 2, 3][3]");
    assert!(
        message.contains("constant index 3 is out of bounds"),
        "{message}"
    );
}

#[test]
fn constant_string_index_out_of_bounds_is_rejected() {
    let message = compile_message("\"ab\"[2]");
    assert!(message.contains("out of bounds"), "{message}");
}

#[test]
fn unknown_struct_fields_are_rejected() {
    let message = infer_message("struct Point { x: int }\nlet point = Point { x: 1 }\npoint.y");
    assert!(message.contains("unknown field 'y'"), "{message}");
}

#[test]
fn question_mark_rejects_an_unconvertible_error_type() {
    let message = compile_message(
        "fn read() -> Result<int, bool> { Ok(1) }\nfn convert() -> Result<int, Error> { let value = read()?; Ok(value) }\nconvert()",
    );
    assert!(message.contains("cannot propagate"), "{message}");
}

#[test]
fn question_mark_rejects_dynamic_boundaries() {
    for source in [
        "fn read() -> Result<int, dynamic> { let value: Result<int, dynamic> = Err(1); value }\nfn convert() -> Result<int, dynamic> { let _ = read()?; let value: Result<int, dynamic> = Err(1); value }\nlet _ = convert()",
        "fn read() -> Option<dynamic> { let value: Option<dynamic> = None; value }\nfn convert() -> Option<dynamic> { let _ = read()?; let value: Option<dynamic> = None; value }\nlet _ = convert()",
    ] {
        let message = compile_message(source);
        assert!(message.contains("cannot propagate"), "{message}");
        assert!(message.contains("dynamic"), "{message}");
    }
}

#[test]
fn generic_arity_is_checked_in_function_parameter() {
    let message = compile_message("fn take(value: Option<int, string>) -> int { 1 }\ntake(None)");
    assert!(message.contains("generic type 'Option'"), "{message}");
}

#[test]
fn generic_arity_is_checked_in_function_return() {
    let message = compile_message("fn make() -> Result<int> { Ok(1) }\nmake()");
    assert!(message.contains("generic type 'Result'"), "{message}");
}

#[test]
fn generic_arity_is_checked_in_cast() {
    let message = compile_message("let value = 1 as Option<int, string>; 0");
    assert!(message.contains("generic type 'Option'"), "{message}");
}

#[test]
fn generic_arity_is_checked_in_struct_fields() {
    let message = compile_message("struct Box { value: Option<int, string> }\n0");
    assert!(message.contains("generic type 'Option'"), "{message}");
}

#[test]
fn generic_arity_is_checked() {
    let message = compile_message("let value: Option<int, string> = None\nvalue");
    assert!(message.contains("generic"), "{message}");
}

#[test]
fn match_arm_without_a_value_is_rejected() {
    let message = compile_message("match Some(1) { Some(_) => { let x = 1 }, None => 0 }");
    assert!(
        message.contains("match arm must produce a value"),
        "{message}"
    );
}

#[test]
fn nested_alternatives_cover_each_closed_value() {
    compile_ok("match Some(true) { Some(true) | Some(false) => 1, None => 0 }");
}

#[test]
fn underscore_discard_does_not_bind_a_variable() {
    let message = compile_message(
        "fn read() -> Result<int, string> { Ok(1) }\nfn main() { let _ = read(); _ }\nmain()",
    );
    assert!(message.contains("undefined variable"), "{message}");
}

#[test]
fn invalid_sum_variant_expression_is_rejected() {
    let message = compile_message("let value = Result::None\n0");
    assert!(message.contains("unknown variant"), "{message}");
}
