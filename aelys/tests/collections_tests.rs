mod common;

use aelys::{CompileOptions, Runtime};
use common::{assert_aelys_bool, assert_aelys_error_contains, assert_aelys_int, assert_aelys_str};

fn compile_message(source: &str) -> String {
    match Runtime::new().compile(source, CompileOptions::default()) {
        Ok(_) => panic!("the source must be rejected"),
        Err(error) => error.to_string(),
    }
}

#[test]
fn rust_style_repeats_allocate_and_fill_collections() {
    assert_aelys_int("let a = [7; 3]; a.len()", 3);
    assert_aelys_int("let a = [7; 3]; a[2]", 7);
    assert_aelys_int("let v = vec![9; 4]; v.len()", 4);
    assert_aelys_int("let v = vec![9; 4]; v[3]", 9);
}

#[test]
fn object_repeats_store_real_values() {
    assert_aelys_str("let values = [\"ok\"; 2]; values[1]", "ok");
}

#[test]
fn indexing_a_repeat_does_not_expose_null() {
    assert_aelys_error_contains("let a = [7; 2]; a[2]", "out of bounds");
}

#[test]
fn constant_index_past_a_repeat_is_rejected_at_compile_time() {
    let message = compile_message("let a = [7; 2]; a[2]");
    assert!(message.contains("constant index 2"), "{message}");
}

#[test]
fn constant_slice_bounds_are_rejected_at_compile_time() {
    let message = compile_message("let a = [1, 2, 3]; a[1..4]");
    assert!(message.contains("slice"), "{message}");
}

#[test]
fn descending_inclusive_slice_is_rejected_at_compile_time() {
    let message = compile_message("let a = [1, 2, 3]; a[2..=1]");
    assert!(message.contains("slice"), "{message}");
}

#[test]
fn collections_report_empty_without_runtime_sentinels() {
    assert_aelys_bool("let a = [7; 1]; a.is_empty()", false);
    assert_aelys_bool("let v = vec![1; 0]; v.is_empty()", true);
}

#[test]
fn get_returns_option_instead_of_null_on_miss() {
    assert_aelys_int(
        "fn value(item: Option<int>) -> int { match item { Some(x) => x, None => 0 } } value(vec![4].get(0)) + value(vec![4].get(3))",
        4,
    );
}

#[test]
fn array_slices_are_owned_vectors() {
    assert_aelys_int(
        "let mut slice = [1, 2, 3][0..2]; slice.push(4); slice.len()",
        3,
    );
}

#[test]
fn ranges_are_first_class_values() {
    assert_aelys_int("let span = 1..3; [0, 1, 2, 3][span].len()", 2);
}

#[test]
fn immutable_vec_cannot_be_mutated() {
    let message = compile_message("let v = vec![1]; v.push(2)");
    assert!(message.contains("mutable"), "{message}");
}

#[test]
fn read_only_iteration_rejects_collection_mutation() {
    let message = compile_message("let mut v = vec![1]; for x in &v { v.push(x) }; v.len()");
    assert!(message.contains("read-only"), "{message}");
}

#[test]
fn owned_iteration_rejects_collection_mutation() {
    let message = compile_message("let mut v = vec![1]; for x in v { v.push(x) }; v.len()");
    assert!(message.contains("read-only"), "{message}");
}

#[test]
fn read_only_indexed_iteration_rejects_collection_mutation() {
    let message = compile_message(
        "let mut grid = vec![vec![1]]; for x in &grid[0] { grid[0].push(x) }; grid.len()",
    );
    assert!(message.contains("read-only"), "{message}");
}

#[test]
fn read_only_iteration_rejects_mutable_aliases() {
    let message = compile_message(
        "let mut v = vec![1]; for x in &v { let mut alias = v; alias.push(x) }; v.len()",
    );
    assert!(message.contains("read-only"), "{message}");
}

#[test]
fn mutable_collection_aliases_are_rejected() {
    let message = compile_message("let mut v = vec![1]; let mut alias = v; alias.push(2)");
    assert!(message.contains("alias"), "{message}");
}

#[test]
fn legacy_collection_type_annotations_are_rejected() {
    let message = compile_message("let values: Array<int> = [1]\nvalues[0]");
    assert!(message.contains("legacy collection syntax"), "{message}");
}

#[test]
fn immutable_collection_cannot_be_index_assigned() {
    let message = compile_message("let v = vec![1]; v[0] = 2; v[0]");
    assert!(message.contains("mutable"), "{message}");
}

#[test]
fn bare_iter_must_be_consumed() {
    let message = compile_message("let v = vec![1]; v.iter()");
    assert!(message.contains("iterator"), "{message}");
}

#[test]
fn collect_requires_a_pipeline_source() {
    let message = compile_message("let v = vec![1]; v.collect()");
    assert!(message.contains("pipeline"), "{message}");
}

#[test]
fn collection_pipelines_execute_without_runtime_iterators() {
    assert_aelys_int(
        "let v = vec![1, 2, 3]; v.map(fn(x: int) -> int { return x * 2 }).len()",
        3,
    );
    assert_aelys_int(
        "let v = vec![1, 2, 3]; v.filter(fn(x: int) -> bool { return x % 2 == 0 })[0]",
        2,
    );
    assert_aelys_int(
        "let v = vec![1, 2, 3]; v.fold(0, fn(acc: int, x: int) -> int { return acc + x })",
        6,
    );
    assert_aelys_int(
        "let v = vec![1, 2, 3]; v.iter().map(fn(x: int) -> int { return x + 1 }).collect().len()",
        3,
    );
    assert_aelys_int(
        "let mut v = vec![1, 2]; let c = v.iter().collect(); v.push(3); c.len()",
        2,
    );
}
