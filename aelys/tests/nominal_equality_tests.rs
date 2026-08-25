mod common;

use aelys::new_vm;
use aelys_runtime::{HostRoot, Value};
use common::{assert_aelys_bool, assert_aelys_error_contains};

#[test]
fn unit_enum_variants_compare_structurally() {
    let source = r#"
enum Color { Red, Blue }
let a = Color::Red
let b = Color::Red
let c = Color::Blue
"#;
    assert_aelys_bool(&format!("{source}\na == b"), true);
    assert_aelys_bool(&format!("{source}\na == c"), false);
    assert_aelys_bool(&format!("{source}\na != b"), false);
    assert_aelys_bool(&format!("{source}\na != c"), true);
}

#[test]
fn payload_enum_variants_compare_structurally() {
    let source = r#"
enum Shape { Pair(int, int), Unit }
let a = Shape::Pair(1, 2)
let b = Shape::Pair(1, 2)
let c = Shape::Pair(9, 9)
let d = Shape::Pair(1, 9)
let u = Shape::Unit
"#;
    assert_aelys_bool(&format!("{source}\na == b"), true);
    assert_aelys_bool(&format!("{source}\na == c"), false);
    assert_aelys_bool(&format!("{source}\na == d"), false);
    assert_aelys_bool(&format!("{source}\na == u"), false);
    assert_aelys_bool(&format!("{source}\na != b"), false);
    assert_aelys_bool(&format!("{source}\na != c"), true);
    assert_aelys_bool(&format!("{source}\nu == u"), true);
}

#[test]
fn enum_string_payload_compares_by_content() {
    let source = r#"
enum Named { Tag(string) }
fn tag(prefix: string) -> Named { Named::Tag(prefix + "llo") }
let a = tag("he")
let b = tag("he")
let c = tag("wor")
"#;
    assert_aelys_bool(&format!("{source}\na == b"), true);
    assert_aelys_bool(&format!("{source}\na == c"), false);
    assert_aelys_bool(&format!("{source}\na != b"), false);
    assert_aelys_bool(&format!("{source}\na != c"), true);
}

#[test]
fn enum_nested_enum_payload_compares_by_content() {
    let source = r#"
enum Inner { A(int), B }
enum Outer { Wrap(Inner), Plain }
let a = Outer::Wrap(Inner::A(1))
let b = Outer::Wrap(Inner::A(1))
let c = Outer::Wrap(Inner::A(2))
let d = Outer::Wrap(Inner::B)
let e = Outer::Plain
"#;
    assert_aelys_bool(&format!("{source}\na == b"), true);
    assert_aelys_bool(&format!("{source}\na == c"), false);
    assert_aelys_bool(&format!("{source}\na == d"), false);
    assert_aelys_bool(&format!("{source}\na == e"), false);
    assert_aelys_bool(&format!("{source}\na != b"), false);
    assert_aelys_bool(&format!("{source}\na != d"), true);
}

#[test]
fn enum_vec_payload_compares_by_content() {
    let source = r#"
enum Bag { Items(Vec<int>), Empty }
fn bag(head: int) -> Bag {
    let mut items = vec![head]
    items.push(2)
    Bag::Items(items)
}
let a = bag(1)
let b = bag(1)
let c = bag(5)
"#;
    assert_aelys_bool(&format!("{source}\na == b"), true);
    assert_aelys_bool(&format!("{source}\na == c"), false);
    assert_aelys_bool(&format!("{source}\na != c"), true);
}

#[test]
fn distinct_enum_types_are_not_comparable_at_the_source_level() {
    assert_aelys_error_contains(
        r#"
enum Color { Red, Blue }
enum Mood { Red, Blue }
let a = Color::Red
let b = Mood::Red
a == b
"#,
        "type mismatch",
    );
}

#[test]
fn same_variant_of_two_different_enums_compares_unequal_in_the_vm() {
    let mut vm = new_vm().expect("vm");
    let left = vm.alloc_enum(0, 0, Vec::new()).expect("left enum");
    let right = vm.alloc_enum(1, 0, Vec::new()).expect("right enum");
    let same = vm.alloc_enum(0, 0, Vec::new()).expect("twin enum");
    let other_variant = vm.alloc_enum(0, 1, Vec::new()).expect("other variant");

    let left_value = Value::ptr(left.index());
    let right_value = Value::ptr(right.index());
    let same_value = Value::ptr(same.index());
    let other_value = Value::ptr(other_variant.index());

    assert!(
        vm.values_equal(left_value, same_value),
        "same enum id and variant id must compare equal"
    );
    assert!(
        !vm.values_equal(left_value, right_value),
        "the same variant index of two different enums must compare unequal"
    );
    assert!(
        !vm.values_equal(left_value, other_value),
        "different variants of the same enum must compare unequal"
    );
}

#[test]
fn enum_string_payload_compares_by_content_on_distinct_heap_objects() {
    let mut vm = new_vm().expect("vm");
    let left_text = vm.alloc_string("hello").expect("left string");
    let right_text = vm.alloc_string("hello").expect("right string");
    let other_text = vm.alloc_string("world").expect("other string");
    assert_ne!(
        left_text.index(),
        right_text.index(),
        "the test needs two distinct string objects"
    );

    let left = vm
        .alloc_enum(0, 0, vec![Value::ptr(left_text.index())])
        .expect("left enum");
    let right = vm
        .alloc_enum(0, 0, vec![Value::ptr(right_text.index())])
        .expect("right enum");
    let other = vm
        .alloc_enum(0, 0, vec![Value::ptr(other_text.index())])
        .expect("other enum");

    assert!(vm.values_equal(Value::ptr(left.index()), Value::ptr(right.index())));
    assert!(!vm.values_equal(Value::ptr(left.index()), Value::ptr(other.index())));
}

#[test]
fn structs_compare_structurally() {
    let source = r#"
struct Point { x: int, y: int }
let a = Point { x: 1, y: 2 }
let b = Point { x: 1, y: 2 }
let c = Point { x: 9, y: 2 }
"#;
    assert_aelys_bool(&format!("{source}\na == b"), true);
    assert_aelys_bool(&format!("{source}\na == c"), false);
    assert_aelys_bool(&format!("{source}\na != b"), false);
    assert_aelys_bool(&format!("{source}\na != c"), true);
}

#[test]
fn struct_string_and_enum_fields_compare_by_content() {
    let source = r#"
enum Kind { One, Two }
struct Tagged { name: string, kind: Kind }
fn tagged(prefix: string, kind: Kind) -> Tagged { Tagged { name: prefix + "x", kind: kind } }
let a = tagged("a", Kind::One)
let b = tagged("a", Kind::One)
let c = tagged("a", Kind::Two)
let d = tagged("z", Kind::One)
"#;
    assert_aelys_bool(&format!("{source}\na == b"), true);
    assert_aelys_bool(&format!("{source}\na == c"), false);
    assert_aelys_bool(&format!("{source}\na == d"), false);
    assert_aelys_bool(&format!("{source}\na != b"), false);
}

#[test]
fn distinct_struct_types_are_not_comparable_at_the_source_level() {
    assert_aelys_error_contains(
        r#"
struct A { x: int }
struct B { x: int }
let a = A { x: 1 }
let b = B { x: 1 }
a == b
"#,
        "type mismatch",
    );
}

#[test]
fn option_values_compare_structurally() {
    assert_aelys_bool(
        "let a: Option<int> = Some(1)\nlet b: Option<int> = Some(1)\na == b",
        true,
    );
    assert_aelys_bool(
        "let a: Option<int> = Some(1)\nlet b: Option<int> = Some(5)\na == b",
        false,
    );
    assert_aelys_bool(
        "let a: Option<int> = None\nlet b: Option<int> = None\na == b",
        true,
    );
    assert_aelys_bool(
        "let a: Option<int> = Some(1)\nlet b: Option<int> = None\na == b",
        false,
    );
    assert_aelys_bool(
        "let a: Option<int> = Some(1)\nlet b: Option<int> = Some(1)\na != b",
        false,
    );
    assert_aelys_bool(
        "let a: Option<int> = Some(1)\nlet b: Option<int> = Some(5)\na != b",
        true,
    );
    assert_aelys_bool(
        "let a: Option<int> = Some(1)\nlet b: Option<int> = None\na != b",
        true,
    );
}

#[test]
fn option_string_payload_compares_by_content() {
    let wrap = "fn wrap(prefix: string) -> Option<string> { Some(prefix + \"llo\") }";
    assert_aelys_bool(
        &format!("{wrap}\nlet a = wrap(\"he\")\nlet b = wrap(\"he\")\na == b"),
        true,
    );
    assert_aelys_bool(
        &format!("{wrap}\nlet a = wrap(\"he\")\nlet b = wrap(\"wor\")\na == b"),
        false,
    );
    assert_aelys_bool(
        &format!("{wrap}\nlet a = wrap(\"he\")\nlet b = wrap(\"wor\")\na != b"),
        true,
    );
}

#[test]
fn result_values_compare_structurally() {
    assert_aelys_bool(
        "let a: Result<int, string> = Ok(1)\nlet b: Result<int, string> = Ok(1)\na == b",
        true,
    );
    assert_aelys_bool(
        "let a: Result<int, string> = Ok(1)\nlet b: Result<int, string> = Ok(5)\na == b",
        false,
    );
    assert_aelys_bool(
        "let a: Result<int, string> = Err(\"bad\")\nlet b: Result<int, string> = Err(\"bad\")\na == b",
        true,
    );
    assert_aelys_bool(
        "let a: Result<int, string> = Err(\"bad\")\nlet b: Result<int, string> = Err(\"worse\")\na == b",
        false,
    );
    assert_aelys_bool(
        "let a: Result<int, string> = Ok(1)\nlet b: Result<int, string> = Err(\"bad\")\na == b",
        false,
    );
    assert_aelys_bool(
        "let a: Result<int, string> = Ok(1)\nlet b: Result<int, string> = Ok(1)\na != b",
        false,
    );
    assert_aelys_bool(
        "let a: Result<int, string> = Ok(1)\nlet b: Result<int, string> = Err(\"bad\")\na != b",
        true,
    );
}

fn build_chain(vm: &mut aelys_runtime::VM, depth: usize, tail_variant: u16) -> (Value, HostRoot) {
    let tail = vm.alloc_enum(0, tail_variant, Vec::new()).expect("tail");
    let mut head = tail;
    let mut root = vm.pin_host_ref(head);
    for index in 0..depth {
        let node = vm
            .alloc_enum(
                0,
                0,
                vec![Value::int(index as i64), Value::ptr(head.index())],
            )
            .expect("chain node");
        head = node;
        root = vm.pin_host_ref(head);
    }
    (Value::ptr(head.index()), root)
}

#[test]
fn deeply_nested_enum_equality_does_not_overflow_the_native_stack() {
    const DEPTH: usize = 60_000;
    let mut vm = new_vm().expect("vm");
    let (left, left_root) = build_chain(&mut vm, DEPTH, 1);
    let (right, right_root) = build_chain(&mut vm, DEPTH, 1);
    let (shorter, shorter_root) = build_chain(&mut vm, DEPTH - 1, 1);
    let (other_tail, other_tail_root) = build_chain(&mut vm, DEPTH, 2);

    assert!(vm.values_equal(left, right));
    assert!(!vm.values_equal(left, shorter));
    assert!(!vm.values_equal(left, other_tail));

    drop((left_root, right_root, shorter_root, other_tail_root));
}
