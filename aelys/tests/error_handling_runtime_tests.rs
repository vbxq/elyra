mod common;

use aelys::run_with_config_and_opt;
use aelys_opt::OptimizationLevel;
use aelys_runtime::VmConfig;
use common::{assert_aelys_int, run_aelys, run_aelys_err};

#[test]
fn result_constructor_and_match_execute() {
    assert_aelys_int(
        "fn read() -> Result<int, string> { Ok(7) }\nmatch read() { Ok(value) => value, Err(_) => 0 }",
        7,
    );
}

#[test]
fn question_mark_propagates_and_match_executes() {
    assert_aelys_int(
        "fn read() -> Result<int, string> { Ok(7) }\nfn convert() -> Result<int, string> { let value = read()?; Ok(value) }\nmatch convert() { Ok(value) => value, Err(_) => 0 }",
        7,
    );
}

#[test]
fn result_error_is_propagated_and_matched() {
    assert_aelys_int(
        "fn read() -> Result<int, string> { Err(\"bad\") }\nfn convert() -> Result<int, string> { let value = read()?; Ok(value) }\nmatch convert() { Ok(_) => 0, Err(message) => if message == \"bad\" { 1 } else { 0 } }",
        1,
    );
}

#[test]
fn option_none_is_propagated_and_matched() {
    assert_aelys_int(
        "fn read() -> Option<int> { None }\nfn convert() -> Option<int> { let value = read()?; Some(value) }\nmatch convert() { Some(_) => 0, None => 1 }",
        1,
    );
}

#[test]
fn question_mark_converts_string_errors_to_error_values() {
    assert_aelys_int(
        "fn read() -> Result<int, string> { Err(\"bad\") }\nfn convert() -> Result<int, Error> { let value = read()?; Ok(value) }\nmatch convert() { Ok(_) => 0, Err(Error::Message(message)) => if message == \"bad\" { 1 } else { 0 } }",
        1,
    );
}

#[test]
fn error_message_constructor_executes() {
    assert_aelys_int(
        "let value: Error = Error::Message(\"bad\")\nmatch value { Error::Message(message) => if message == \"bad\" { 1 } else { 0 } }",
        1,
    );
}

#[test]
fn option_combinators_execute() {
    assert_aelys_int(
        "fn add_one(value: int) -> int { value + 1 }\nfn choose(value: int) -> Option<int> { Some(value + 1) }\nfn fallback() -> int { 9 }\nSome(2).map(add_one).and_then(choose).unwrap_or_else(fallback)",
        4,
    );
}

#[test]
fn result_combinators_execute() {
    assert_aelys_int(
        "fn add_one(value: int) -> int { value + 1 }\nfn choose(value: int) -> Result<int, string> { Ok(value + 1) }\nfn recover(error: string) -> Result<int, string> { Ok(8) }\nfn read() -> Result<int, string> { Ok(2) }\nread().map(add_one).and_then(choose).or_else(recover).unwrap_or(0)",
        4,
    );
}

#[test]
fn result_unwrap_or_else_receives_the_error() {
    assert_aelys_int(
        "fn recover(error: string) -> int { error.len() }\nErr(\"bad\").unwrap_or_else(recover)",
        3,
    );
}

#[test]
fn result_ok_err_and_expect_execute() {
    assert_aelys_int(
        "fn read() -> Result<int, string> { Ok(3) }\nfn fail() -> Result<int, string> { Err(\"bad\") }\nread().ok().expect(\"ok\") + fail().err().unwrap().len()",
        6,
    );
}

#[test]
fn result_map_err_executes() {
    assert_aelys_int(
        "fn length(error: string) -> int { error.len() }\nlet original: Result<int, string> = Err(\"bad\")\noriginal.map_err(length).err().unwrap()",
        3,
    );
}

#[test]
fn combinator_failure_branches_execute() {
    assert_aelys_int(
        "fn fallback() -> int { 9 }\nfn fallback_option() -> Option<int> { Some(9) }\nfn recover(error: string) -> Result<int, string> { Ok(8) }\nNone.map(fn(value: int) -> int { value + 1 }).and_then(fn(value: int) -> Option<int> { Some(value) }).unwrap_or_else(fallback) + None.or_else(fallback_option).unwrap() + Err(\"bad\").or_else(recover).unwrap_or(0)",
        26,
    );
}

#[test]
fn unit_functions_do_not_return_null() {
    let result = run_aelys("fn noop() -> unit {}\nnoop()");
    assert!(result.is_unit(), "expected unit, got {result:?}");
}

#[test]
fn empty_vec_pop_returns_option_none() {
    assert_aelys_int(
        "let mut value: Vec<int> = vec![]\nmatch value.pop() { Some(_) => 0, None => 1 }",
        1,
    );
}

#[test]
fn slicing_executes_without_reaching_the_backend_panic() {
    assert_aelys_int("[1, 2, 3][0..2].len()", 2);
}

#[test]
fn vector_slicing_supports_inclusive_and_open_bounds() {
    assert_aelys_int("vec![1, 2, 3][1..=2].len()", 2);
    assert_aelys_int("vec![1, 2, 3][..].len()", 3);
    assert_aelys_int("vec![1, 2, 3][1..].len()", 2);
}

#[test]
fn invalid_slice_bounds_raise_a_runtime_error() {
    let error = run_aelys_err("[1, 2, 3][2..1]");
    assert!(error.contains("out of bounds"), "{error}");
}

#[test]
fn negative_vec_reserve_is_a_runtime_error() {
    let error = run_aelys_err("let mut value: Vec<int> = vec![1]\nvalue.reserve(-1)\n0");
    assert!(error.contains("non-negative"), "{error}");
}

#[test]
fn oversized_vec_reserve_is_rejected_before_backing_storage_allocation() {
    let error =
        run_aelys_err("let mut value: Vec<int> = vec![1]\nvalue.reserve(140737488355327)\n0");
    assert!(error.contains("out of memory"), "{error}");
}

#[test]
fn vec_reserve_obeys_the_vm_heap_limit_before_growing() {
    let config = VmConfig::new(VmConfig::MIN_HEAP_BYTES).unwrap();
    let error = run_with_config_and_opt(
        "let mut value: Vec<int> = vec![1]\nvalue.reserve(200000)\n0",
        "<limited-vec-reserve>",
        config,
        Vec::new(),
        OptimizationLevel::Standard,
    )
    .expect_err("reserve must obey the VM heap limit");
    assert!(error.to_string().contains("out of memory"), "{error}");
}

#[test]
fn oversized_array_is_rejected_before_backing_storage_allocation() {
    let config = VmConfig::default();
    let error = run_with_config_and_opt(
        "vec![0; 140737488355327]",
        "<oversized-vec-repeat>",
        config,
        Vec::new(),
        OptimizationLevel::Standard,
    )
    .expect_err("the allocation must fail");
    assert!(error.to_string().contains("out of memory"), "{error}");
}

#[test]
fn unwrap_and_expect_fail_with_structured_messages() {
    let unwrap_error = run_aelys_err("let value: Option<int> = None\nvalue.unwrap()");
    assert!(
        unwrap_error.contains("Option::unwrap failed"),
        "{unwrap_error}"
    );

    let expect_error =
        run_aelys_err("let value: Option<int> = None\nvalue.expect(\"missing value\")");
    assert!(expect_error.contains("missing value"), "{expect_error}");
}

#[test]
fn native_null_is_normalized_to_option_none() {
    assert_aelys_int(
        "needs std::convert\nlet parsed: Option<int> = convert::parse_int(\"not an integer\")\nmatch parsed { Some(_) => 0, None => 1 }",
        1,
    );
}

#[test]
fn successful_native_option_is_wrapped_in_some() {
    assert_aelys_int(
        "needs std::convert\nmatch convert::parse_int(\"42\") { Some(value) => value, None => 0 }",
        42,
    );
}

#[test]
fn qualified_sum_paths_execute() {
    assert_aelys_int(
        "let value: Option<int> = Option::Some(4)\nlet none: Option<int> = Option::None\nmatch value { Option::Some(number) => number, Option::None => match none { Option::None => 4, Option::Some(_) => 0 } }",
        4,
    );
}

#[test]
fn nested_or_patterns_and_guards_execute() {
    assert_aelys_int(
        "match Some(2) { Some(1) | Some(2) if true => 1, Some(value) if value > 2 => value, Some(_) => 0, None => 0 }",
        1,
    );
}

#[test]
fn or_pattern_bindings_execute_on_the_selected_alternative() {
    assert_aelys_int(
        "let value: Result<Option<int>, Option<int> > = Err(Some(9))\nmatch value { Ok(Some(number)) | Err(Some(number)) => number, Ok(None) | Err(None) => 0 }",
        9,
    );
}

#[test]
fn match_block_tail_values_are_consumed_by_the_arm() {
    assert_aelys_int(
        "fn choose(flag: bool) -> Result<int, string> { match flag { true => { Ok(3) }, false => { Err(\"bad\") } } }\nmatch choose(true) { Ok(value) => value, Err(_) => 0 }",
        3,
    );
}
