use aelys_common::error::CompileErrorKind;
use aelys_sema::constraint::{Constraint, ConstraintReason, TypeErrorKind};
use aelys_sema::types::{InferType, TypeVarId};
use aelys_syntax::Span;

use aelys::{CompileOptions, Runtime, run};
use aelys_runtime::Value;

mod common;
use common::{assert_associated_diagnostic, assert_located, assert_located_at};

fn run_ok(source: &str) -> Value {
    run(source, "sema_constraint_tests.aelys").expect("program should run")
}

fn compile_message(source: &str) -> String {
    match Runtime::new().compile(source, CompileOptions::default()) {
        Ok(_) => panic!("the source must be rejected"),
        Err(error) => error.to_string(),
    }
}

// two programs under swapped declaration order cannot show that a diagnostic is
const RENDER_REPEATS: usize = 20;

fn assert_renders_identically(source: &str, label: &str) {
    let first = compile_message(source);
    for round in 1..RENDER_REPEATS {
        let again = compile_message(source);
        assert_eq!(
            again, first,
            "{label}: compilation {round} of one unchanged program rendered other text:\n{first}\n----\n{again}"
        );
    }
}

#[test]
fn test_constraint_creation() {
    let c = Constraint::equal(
        InferType::I64,
        InferType::F64,
        Span::dummy(),
        ConstraintReason::BinaryOp {
            op: "+".to_string(),
        },
    );
    assert!(matches!(c, Constraint::Equal { .. }));
}

#[test]
fn test_one_of_constraint() {
    let c = Constraint::one_of(
        InferType::Var(TypeVarId(0)),
        vec![InferType::I64, InferType::F64, InferType::String],
        Span::dummy(),
        ConstraintReason::BinaryOp {
            op: "+".to_string(),
        },
    );
    assert!(matches!(c, Constraint::OneOf { .. }));
}

#[test]
fn test_type_error_display() {
    let err = aelys_sema::constraint::TypeError::mismatch(
        InferType::I64,
        InferType::F64,
        Span::dummy(),
        ConstraintReason::BinaryOp {
            op: "+".to_string(),
        },
    );
    let msg = format!("{}", err);
    assert!(msg.contains("type mismatch"));
    assert!(msg.contains("i64"));
    assert!(msg.contains("f64"));
}

#[test]
fn the_mangled_symbol_collision_keeps_its_own_code() {
    let kind = TypeErrorKind::MangledSymbolCollision {
        name: "probe$i64".to_string(),
    };
    assert_eq!(kind.diagnostic_code(), 355);
}

#[test]
fn the_enum_layout_diagnostic_keeps_code_346() {
    let kind = TypeErrorKind::EnumLayoutTooLarge {
        enum_name: "Huge".to_string(),
        item: "variant 'Payload'".to_string(),
        count: 65536,
        limit: 65535,
    };
    assert_eq!(kind.diagnostic_code(), 346);
}

#[test]
fn the_module_path_separator_renders_a_single_code() {
    let kind = TypeErrorKind::ModulePathSeparator {
        module: "sys".to_string(),
        member: "arch".to_string(),
    };
    assert_eq!(kind.diagnostic_code(), 411);
    assert_eq!(
        kind.diagnostic_code(),
        CompileErrorKind::ModulePathSeparator {
            module: "sys".to_string(),
            member: "arch".to_string(),
        }
        .code()
    );
}

#[test]
fn the_unreachable_pattern_diagnostic_has_its_own_code() {
    let kind = TypeErrorKind::UnreachablePattern {
        pattern: "Shape::A".to_string(),
    };
    assert_eq!(kind.diagnostic_code(), 356);
}

#[test]
fn the_named_inference_diagnostics_do_not_collapse_into_the_generic_code() {
    let named: Vec<(TypeErrorKind, u16)> = vec![
        (
            TypeErrorKind::GenericArityMismatch {
                name: "Pair".to_string(),
                expected: 2,
                found: 1,
            },
            357,
        ),
        (
            TypeErrorKind::PatternBindingMismatch {
                expected: vec!["a".to_string()],
                found: vec!["b".to_string()],
            },
            358,
        ),
        (
            TypeErrorKind::ArityMismatch {
                expected: 1,
                found: 2,
            },
            359,
        ),
        (TypeErrorKind::NotCallable { ty: InferType::I64 }, 360),
        (
            TypeErrorKind::InfiniteType {
                var: TypeVarId(0),
                ty: InferType::I64,
            },
            361,
        ),
        (
            TypeErrorKind::UndefinedFunction {
                name: "probe".to_string(),
            },
            362,
        ),
        (
            TypeErrorKind::UnknownField {
                structure: "Point".to_string(),
                field: "z".to_string(),
            },
            363,
        ),
        (
            TypeErrorKind::MissingField {
                structure: "Point".to_string(),
                field: "y".to_string(),
            },
            364,
        ),
        (
            TypeErrorKind::NotIterable {
                receiver: InferType::I64,
            },
            365,
        ),
        (
            TypeErrorKind::InvalidIndex {
                receiver: InferType::I64,
            },
            366,
        ),
        (
            TypeErrorKind::UntypedNativeTypeMismatch {
                name: "raw".to_string(),
                expected: InferType::I64,
            },
            367,
        ),
        (TypeErrorKind::RecursionLimit, 368),
        (
            TypeErrorKind::UnknownVariant {
                variant: "Shape::Z".to_string(),
                expected: "Shape".to_string(),
            },
            109,
        ),
        (TypeErrorKind::MatchArmValueRequired, 110),
        (
            TypeErrorKind::MissingReturnValue {
                expected: InferType::I64,
            },
            215,
        ),
    ];
    for (kind, expected) in named {
        assert_eq!(
            kind.diagnostic_code(),
            expected,
            "unexpected code for {kind:?}"
        );
    }
}

#[test]
fn the_generic_code_stays_reserved_for_plain_inference_failures() {
    assert_eq!(
        TypeErrorKind::Mismatch {
            expected: InferType::I64,
            found: InferType::F64,
        }
        .diagnostic_code(),
        301
    );
    assert_eq!(
        TypeErrorKind::UndefinedVariable {
            name: "type".to_string(),
        }
        .diagnostic_code(),
        301
    );
}

#[test]
fn associated_item_projection_executes() {
    let result = run_ok(
        r#"
trait Source {
    type Item
    fn next(self) -> Self::Item
}
struct Counter { value: int }
impl Source for Counter {
    type Item = int
    fn next(self) -> int { self.value }
}
fn generic_probe<T: Source>(source: T) -> T::Item {
    source.next()
}
fn probe() -> int {
    let counter = Counter { value: 7 }
    generic_probe(counter)
}
probe()
"#,
    );
    assert_eq!(result.as_int(), Some(7));
}

#[test]
fn associated_constant_in_expression_executes() {
    let result = run_ok(
        r#"
trait Limits {
    const LIMIT: int
}
struct Bounds {}
impl Limits for Bounds {
    const LIMIT: int = 3
}
fn probe() -> int {
    let values: [int; Bounds::LIMIT] = [1, 2, 3]
    values.len() + Bounds::LIMIT
}
probe()
"#,
    );
    assert_eq!(result.as_int(), Some(6));
}

#[test]
fn missing_associated_item_is_e0421() {
    let message = compile_message(
        r#"
trait Source {
    type Item
    fn next(self) -> Self::Item
}
struct Counter { value: int }
impl Source for Counter {
    fn next(self) -> int { self.value }
}
"#,
    );
    assert!(
        message.contains("E0421"),
        "expected E0421 for a missing associated item: {message}"
    );
    assert_associated_diagnostic(
        &message,
        "E0421",
        "Item",
        "impl of trait 'Source' for 'Counter'",
        "define 'type Item'",
    );
}

#[test]
fn associated_type_mismatch_is_e0422() {
    let message = compile_message(
        r#"
trait Limits {
    const LIMIT: int
}
struct Bounds {}
impl Limits for Bounds {
    const LIMIT: string = "ten"
}
"#,
    );
    assert!(
        message.contains("E0422"),
        "expected E0422 for a wrong associated item type: {message}"
    );
    assert_associated_diagnostic(
        &message,
        "E0422",
        "LIMIT",
        "declared type in the impl for 'Bounds'",
        "declare the same type as the trait",
    );
}

#[test]
fn ambiguous_projection_is_e0423() {
    let message = compile_message(
        r#"
trait Source {
    type Item
}
fn probe<T>(source: T) -> T::Item {
    source
}
"#,
    );
    assert!(
        message.contains("E0423"),
        "expected E0423 for an unresolved generic projection: {message}"
    );
    assert_associated_diagnostic(&message, "E0423", "Item", "a return type", "add a bound");
}

#[test]
fn object_binding_mismatch_is_e0424() {
    let message = compile_message(
        r#"
trait Source {
    type Item
    fn next(self) -> Self::Item
}
struct Counter { value: int }
impl Source for Counter {
    type Item = string
    fn next(self) -> string { "nope" }
}
fn pick<T: Source<Item = int>>(source: T) -> T::Item {
    source.next()
}
fn probe() -> int {
    let counter = Counter { value: 7 }
    pick(counter)
}
"#,
    );
    assert_associated_diagnostic(
        &message,
        "E0424",
        "Item",
        "a bound on 'T'",
        "change the requested binding or the impl",
    );
}

#[test]
fn projection_arithmetic_executes() {
    let result = run_ok(
        r#"
trait Source {
    type Item
    fn next(self) -> Self::Item
}
struct Counter { value: int }
impl Source for Counter {
    type Item = int
    fn next(self) -> int { self.value }
}
fn generic_probe<T: Source>(source: T) -> T::Item {
    source.next()
}
fn probe() -> int {
    let counter = Counter { value: 41 }
    generic_probe(counter) + 1
}
probe()
"#,
    );
    assert_eq!(result.as_int(), Some(42));
}

#[test]
fn projection_comparison_executes() {
    let result = run_ok(
        r#"
trait Source {
    type Item
    fn next(self) -> Self::Item
}
struct Counter { value: int }
impl Source for Counter {
    type Item = int
    fn next(self) -> int { self.value }
}
fn generic_probe<T: Source>(source: T) -> T::Item {
    source.next()
}
fn probe() -> int {
    let counter = Counter { value: 7 }
    if generic_probe(counter) > 3 { 1 } else { 0 }
}
probe()
"#,
    );
    assert_eq!(result.as_int(), Some(1));
}

#[test]
fn cyclic_projection_is_e0423() {
    let message = compile_message(
        r#"
trait Holder {
    type Item
}
struct Cell { value: int }
impl Holder for Cell {
    type Item = Cell::Item
}
fn probe() -> int { 1 }
probe()
"#,
    );
    assert_associated_diagnostic(
        &message,
        "E0423",
        "Cell::Item",
        "an associated item definition in the impl for 'Cell'",
        "break the cycle",
    );
}

#[test]
fn mutually_cyclic_projections_are_e0423() {
    let message = compile_message(
        r#"
trait First {
    type Left
}
trait Second {
    type Right
}
struct Cell { value: int }
impl First for Cell {
    type Left = Cell::Right
}
impl Second for Cell {
    type Right = Cell::Left
}
fn probe() -> int { 1 }
probe()
"#,
    );
    assert_associated_diagnostic(
        &message,
        "E0423",
        "Cell::",
        "an associated item definition in the impl for 'Cell'",
        "break the cycle",
    );
}

#[test]
fn ambiguous_concrete_projection_is_e0423() {
    let message = compile_message(
        r#"
trait Left {
    type Item
}
trait Right {
    type Item
}
struct Cell { value: int }
impl Left for Cell {
    type Item = int
}
impl Right for Cell {
    type Item = string
}
fn probe() -> Cell::Item { 1 }
probe()
"#,
    );
    assert_associated_diagnostic(
        &message,
        "E0423",
        "Cell::Item",
        "a return type",
        "remove one of the competing impls",
    );
}

#[test]
fn associated_type_in_an_inherent_impl_is_e0425() {
    let message = compile_message(
        r#"
struct Point { x: int }
impl Point {
    type Item = int
    fn get(self) -> int { self.x }
}
fn probe() -> int { 1 }
probe()
"#,
    );
    assert_associated_diagnostic(
        &message,
        "E0425",
        "Item",
        "inherent impl of 'Point'",
        "impl of a trait",
    );
}

#[test]
fn associated_const_in_an_inherent_impl_is_e0425() {
    let message = compile_message(
        r#"
struct Point { x: int }
impl Point {
    const LIMIT: int = 3
    fn get(self) -> int { self.x }
}
fn probe() -> int { 1 }
probe()
"#,
    );
    assert_associated_diagnostic(
        &message,
        "E0425",
        "LIMIT",
        "inherent impl of 'Point'",
        "impl of a trait",
    );
}

#[test]
fn an_item_no_trait_declares_is_rejected_in_a_trait_impl_too() {
    let both = "    type Item = int\n    const LIMIT: int = 3";
    assert_eq!(
        run_ok(&inherent_item_program(both, true)).as_int(),
        Some(7),
        "the two items 'Limits' declares must keep compiling in its impl"
    );
    let associated_type = compile_message(&inherent_item_program(
        &format!("{both}\n    type Other = int"),
        true,
    ));
    assert_associated_diagnostic(
        &associated_type,
        "E0425",
        "Other",
        "impl of trait 'Limits' for 'Bounds'",
        "impl of a trait that declares 'Other'",
    );
    let associated_const = compile_message(&inherent_item_program(
        &format!("{both}\n    const OTHER: int = 9"),
        true,
    ));
    assert_associated_diagnostic(
        &associated_const,
        "E0425",
        "OTHER",
        "impl of trait 'Limits' for 'Bounds'",
        "impl of a trait that declares 'OTHER'",
    );
}

#[test]
fn a_long_acyclic_projection_chain_compiles() {
    // worse, hand the poisoned type to a later stage as if it had type-checked.
    let depth = 80;
    let mut source = String::new();
    for index in 0..=depth {
        source.push_str(&format!("trait Chain{index} {{ type Item{index} }}\n"));
    }
    source.push_str("struct Link { value: int }\n");
    for index in 0..depth {
        source.push_str(&format!(
            "impl Chain{index} for Link {{ type Item{index} = Link::Item{} }}\n",
            index + 1
        ));
    }
    source.push_str(&format!(
        "impl Chain{depth} for Link {{ type Item{depth} = int }}\n"
    ));
    source.push_str("fn probe() -> Link::Item0 { 5 }\nprobe()\n");
    assert_eq!(run_ok(&source).as_int(), Some(5));
}

#[test]
fn a_deep_acyclic_projection_diamond_does_not_blow_up() {
    let depth = 40;
    let mut source = String::new();
    for index in 0..=depth {
        source.push_str(&format!("trait Fan{index} {{ type Item{index} }}\n"));
    }
    source.push_str("struct Node { value: int }\n");
    for index in 0..depth {
        source.push_str(&format!(
            "impl Fan{index} for Node {{ type Item{index} = Result<Node::Item{}, Node::Item{}> }}\n",
            index + 1,
            index + 1
        ));
    }
    source.push_str(&format!(
        "impl Fan{depth} for Node {{ type Item{depth} = int }}\n"
    ));
    // the projection must be *used*: an unused diamond never enters the
    source.push_str("fn probe() -> int {\n    let value: Node::Item0 = 1\n    1\n}\nprobe()\n");
    let message = compile_message(&source);
    assert_associated_diagnostic(
        &message,
        "E0427",
        "Node::Item",
        "type annotation on variable 'value'",
        "give one of them a concrete definition",
    );
}

#[test]
fn cyclic_associated_constant_is_e0423() {
    let message = compile_message(
        r#"
trait Limits {
    const LIMIT: int
}
struct Bounds {}
impl Limits for Bounds {
    const LIMIT: int = Bounds::LIMIT
}
fn probe() -> int { 1 }
probe()
"#,
    );
    assert_associated_diagnostic(
        &message,
        "E0423",
        "Bounds::LIMIT",
        "an associated item definition in the impl for 'Bounds'",
        "break the cycle",
    );
}

#[test]
fn an_ambiguous_associated_constant_does_not_depend_on_declaration_order() {
    let first = compile_message(
        r#"
trait Left { const LIMIT: int }
trait Right { const LIMIT: int }
struct Bounds {}
impl Left for Bounds { const LIMIT: int = 3 }
impl Right for Bounds { const LIMIT: int = 5 }
fn probe() -> int { Bounds::LIMIT }
probe()
"#,
    );
    let swapped = compile_message(
        r#"
trait Left { const LIMIT: int }
trait Right { const LIMIT: int }
struct Bounds {}
impl Right for Bounds { const LIMIT: int = 5 }
impl Left for Bounds { const LIMIT: int = 3 }
fn probe() -> int { Bounds::LIMIT }
probe()
"#,
    );
    assert_associated_diagnostic(
        &first,
        "E0423",
        "Bounds::LIMIT",
        "a value expression",
        "remove one of the competing",
    );
    assert_eq!(
        first, swapped,
        "declaration order must never change the diagnostic"
    );
    assert_renders_identically(
        r#"
trait Left { const LIMIT: int }
trait Right { const LIMIT: int }
struct Bounds {}
impl Left for Bounds { const LIMIT: int = 3 }
impl Right for Bounds { const LIMIT: int = 5 }
fn probe() -> int { Bounds::LIMIT }
probe()
"#,
        "the cross-impl ambiguity",
    );
}

#[test]
fn an_unresolvable_symbolic_array_length_is_e0423() {
    let message = compile_message(
        r#"
struct Bounds {}
fn probe() -> int {
    let values: [int; Bounds::MISSING] = [1, 2]
    values.len()
}
probe()
"#,
    );
    assert_associated_diagnostic(
        &message,
        "E0423",
        "Bounds::MISSING",
        "an array length",
        "no impl for",
    );
}

#[test]
fn a_trait_qualified_projection_resolves_through_its_only_impl() {
    let result = run_ok(
        r#"
trait Source {
    type Item
    const LIMIT: int
    fn next(self) -> Self::Item
}
struct Counter { value: int }
impl Source for Counter {
    type Item = int
    const LIMIT: int = 4
    fn next(self) -> int { self.value }
}
fn probe() -> Source::Item {
    let counter = Counter { value: 7 }
    counter.next() * Source::LIMIT
}
probe()
"#,
    );
    assert_eq!(result.as_int(), Some(28));
}

#[test]
fn a_trait_qualified_projection_with_two_impls_is_e0423() {
    let message = compile_message(
        r#"
trait Source { type Item }
struct First { value: int }
struct Second { value: int }
impl Source for First { type Item = int }
impl Source for Second { type Item = string }
fn probe() -> Source::Item { 1 }
probe()
"#,
    );
    assert_associated_diagnostic(
        &message,
        "E0423",
        "Source::Item",
        "a return type",
        "remove one of the competing",
    );
}

#[test]
fn self_names_the_impl_target_in_an_associated_item() {
    let result = run_ok(
        r#"
trait Holder { type Item }
trait Mirror { type Echo }
struct Cell { value: int }
impl Holder for Cell {
    type Item = int
}
impl Mirror for Cell {
    type Echo = Self::Item
}
fn probe() -> Cell::Echo { 9 }
probe()
"#,
    );
    assert_eq!(result.as_int(), Some(9));
}

#[test]
fn an_associated_constant_value_must_match_its_declared_type() {
    let message = compile_message(
        r#"
trait Limits {
    const LIMIT: int
}
struct Bounds {}
impl Limits for Bounds {
    const LIMIT: int = "ten"
}
fn probe() -> int { 1 }
probe()
"#,
    );
    assert_associated_diagnostic(
        &message,
        "E0422",
        "LIMIT",
        "constant value in the impl for 'Bounds'",
        "write an initialiser of type 'i64'",
    );
}

#[test]
fn a_constant_expression_is_evaluated_for_an_array_length() {
    let result = run_ok(
        r#"
trait Limits {
    const LIMIT: int
}
struct Bounds {}
impl Limits for Bounds {
    const LIMIT: int = 2 + 1
}
fn probe() -> int {
    let values: [int; Bounds::LIMIT] = [1, 2, 3]
    values.len() + Bounds::LIMIT
}
probe()
"#,
    );
    assert_eq!(result.as_int(), Some(6));
}

#[test]
fn a_shallow_projection_diamond_still_compiles() {
    // the node budget must not reject an expansion a user can still read.
    let depth = 8;
    let mut source = String::new();
    for index in 0..=depth {
        source.push_str(&format!("trait Small{index} {{ type Item{index} }}\n"));
    }
    source.push_str("struct Leaf { value: int }\n");
    for index in 0..depth {
        source.push_str(&format!(
            "impl Small{index} for Leaf {{ type Item{index} = Result<Leaf::Item{}, Leaf::Item{}> }}\n",
            index + 1,
            index + 1
        ));
    }
    source.push_str(&format!(
        "impl Small{depth} for Leaf {{ type Item{depth} = int }}\n"
    ));
    // the projection must be *used*. an unused one never enters the normalizer,
    source.push_str(
        "fn probe() -> int {\n    let value: Leaf::Item0 = Ok(Ok(Ok(Ok(Ok(Ok(Ok(Ok(1))))))))\n    match value {\n        Ok(_) => 1\n        Err(_) => 0\n    }\n}\nprobe()\n",
    );
    assert_eq!(run_ok(&source).as_int(), Some(1));
}

#[test]
fn a_trait_qualified_projection_does_not_depend_on_impl_order() {
    let first = run_ok(
        r#"
trait Holder { type Item }
trait Source { type Other }
struct Cell { value: int }
impl Source for Cell { type Other = int }
impl Holder for Cell { type Item = Source::Other }
fn probe() -> int {
    let value: Cell::Item = 5
    value
}
probe()
"#,
    );
    let swapped = run_ok(
        r#"
trait Holder { type Item }
trait Source { type Other }
struct Cell { value: int }
impl Holder for Cell { type Item = Source::Other }
impl Source for Cell { type Other = int }
fn probe() -> int {
    let value: Cell::Item = 5
    value
}
probe()
"#,
    );
    assert_eq!(first.as_int(), Some(5));
    assert_eq!(swapped.as_int(), first.as_int());
}

#[test]
fn an_associated_constant_through_a_type_parameter_executes() {
    let result = run_ok(
        r#"
trait Limits {
    const LIMIT: int
    fn id(self) -> int
}
struct Small { value: int }
struct Large { value: int }
impl Limits for Small {
    const LIMIT: int = 3
    fn id(self) -> int { self.value }
}
impl Limits for Large {
    const LIMIT: int = 100
    fn id(self) -> int { self.value }
}
fn take<T: Limits>(item: T) -> int {
    item.id() + T::LIMIT
}
fn probe() -> int {
    let small = Small { value: 1 }
    let large = Large { value: 2 }
    take(small) * 1000 + take(large)
}
probe()
"#,
    );
    assert_eq!(result.as_int(), Some(4102));
}

#[test]
fn an_associated_constant_through_an_undeclared_bound_is_e0423() {
    let message = compile_message(
        r#"
trait Other {
    fn id(self) -> int
}
struct Cell { value: int }
impl Other for Cell {
    fn id(self) -> int { self.value }
}
fn take<T: Other>(item: T) -> int {
    item.id() + T::LIMIT
}
fn probe() -> int {
    take(Cell { value: 1 })
}
probe()
"#,
    );
    assert_associated_diagnostic(
        &message,
        "E0423",
        "T::LIMIT",
        "a value expression",
        "add a bound on 'T'",
    );
}

#[test]
fn an_associated_constant_may_name_another_one() {
    let result = run_ok(
        r#"
trait Limits {
    const BASE: int
    const LIMIT: int
}
struct Bounds {}
impl Limits for Bounds {
    const LIMIT: int = Self::BASE + 3
    const BASE: int = 2
}
fn probe() -> int {
    let values: [int; Bounds::LIMIT] = [1, 2, 3, 4, 5]
    values.len() + Bounds::LIMIT
}
probe()
"#,
    );
    assert_eq!(result.as_int(), Some(10));
}

#[test]
fn a_constant_that_cannot_be_an_array_length_says_so() {
    let message = compile_message(
        r#"
trait Limits {
    const LIMIT: int
}
struct Bounds {}
impl Limits for Bounds {
    const LIMIT: int = 0 - 3
}
fn probe() -> int {
    let values: [int; Bounds::LIMIT] = [1, 2]
    values.len()
}
probe()
"#,
    );
    assert_associated_diagnostic(
        &message,
        "E0423",
        "Bounds::LIMIT",
        "an array length",
        "cannot be an array length",
    );
}

fn projection_diamond(depth: usize) -> String {
    let mut source = String::new();
    for index in 0..=depth {
        source.push_str(&format!("trait Fan{index} {{ type Item{index} }}\n"));
    }
    source.push_str("struct Node { value: int }\n");
    for index in 0..depth {
        source.push_str(&format!(
            "impl Fan{index} for Node {{ type Item{index} = Result<Node::Item{}, Node::Item{}> }}\n",
            index + 1,
            index + 1
        ));
    }
    source.push_str(&format!(
        "impl Fan{depth} for Node {{ type Item{depth} = int }}\n"
    ));
    source.push_str("fn probe() -> int {\n    let value: Node::Item0 = ");
    source.push_str(&"Ok(".repeat(depth));
    source.push('1');
    source.push_str(&")".repeat(depth));
    source.push_str(
        "\n    match value {\n        Ok(_) => 1\n        Err(_) => 0\n    }\n}\nprobe()\n",
    );
    source
}

#[test]
fn the_projection_budget_admits_what_it_should_and_rejects_what_it_should() {
    // cannot pass by never reaching the budget at all: if the accepted case
    assert_eq!(run_ok(&projection_diamond(8)).as_int(), Some(1));
    let message = compile_message(&projection_diamond(14));
    assert_associated_diagnostic(
        &message,
        "E0427",
        "Node::Item",
        "type annotation on variable 'value'",
        "give one of them a concrete definition",
    );
}

#[test]
fn an_exponential_constant_graph_is_evaluated_once_per_constant() {
    let depth = 24;
    let mut source = String::from("trait Limits {\n");
    for index in 0..=depth {
        source.push_str(&format!("    const C{index}: int\n"));
    }
    source.push_str("}\nstruct Node { value: int }\nimpl Limits for Node {\n");
    for index in 0..depth {
        source.push_str(&format!(
            "    const C{index}: int = Self::C{} + Self::C{}\n",
            index + 1,
            index + 1
        ));
    }
    source.push_str(&format!("    const C{depth}: int = 1\n}}\n"));
    source.push_str("fn probe() -> int {\n    Node::C0\n}\nprobe()\n");
    assert_eq!(run_ok(&source).as_int(), Some(16_777_216));
}

#[test]
fn a_long_constant_chain_is_evaluated_without_a_budget() {
    // chain costs constant stack depth. a step budget here rejected legitimate
    let depth = 600;
    let mut source = String::from("trait Limits {\n");
    for index in 0..=depth {
        source.push_str(&format!("    const C{index}: int\n"));
    }
    source.push_str("}\nstruct Node { value: int }\nimpl Limits for Node {\n");
    for index in 0..depth {
        source.push_str(&format!(
            "    const C{index}: int = Self::C{} + 1\n",
            index + 1
        ));
    }
    source.push_str(&format!("    const C{depth}: int = 0\n}}\n"));
    source.push_str("fn probe() -> int {\n    Node::C0\n}\nprobe()\n");
    assert_eq!(run_ok(&source).as_int(), Some(depth as i64));
}

#[test]
fn a_trait_qualified_projection_in_a_signature_ignores_impl_order() {
    let between = compile_message(
        r#"
trait Source { type Item }
struct First { value: int }
struct Second { value: int }
impl Source for First { type Item = int }
fn probe() -> Source::Item {
    5
}
impl Source for Second { type Item = string }
probe()
"#,
    );
    let after = compile_message(
        r#"
trait Source { type Item }
struct First { value: int }
struct Second { value: int }
impl Source for First { type Item = int }
impl Source for Second { type Item = string }
fn probe() -> Source::Item {
    5
}
probe()
"#,
    );
    assert_associated_diagnostic(
        &between,
        "E0423",
        "Source::Item",
        "a return type",
        "remove one of the competing",
    );
    // differently; what must not differ is the diagnostic itself.
    assert_eq!(
        between.lines().next(),
        after.lines().next(),
        "a use written before an impl must diagnose exactly like one written after it"
    );
}

#[test]
fn a_trait_qualified_projection_resolves_before_its_impl_is_declared() {
    let result = run_ok(
        r#"
trait Source { type Item }
struct Counter { value: int }
fn probe() -> Source::Item {
    5
}
impl Source for Counter { type Item = int }
probe()
"#,
    );
    assert_eq!(result.as_int(), Some(5));
}

#[test]
fn a_constant_whose_value_cannot_be_computed_says_so() {
    // reporting this as an undefined function denied a definition the user had
    let message = compile_message(
        r#"
trait Limits {
    const LIMIT: int
}
struct Bounds {}
impl Limits for Bounds {
    const LIMIT: int = 1 / 0
}
fn probe() -> int {
    Bounds::LIMIT
}
probe()
"#,
    );
    assert_associated_diagnostic(
        &message,
        "E0423",
        "Bounds::LIMIT",
        "a value expression",
        "cannot be computed",
    );
}

#[test]
fn an_uncomputable_constant_used_as_a_length_says_why() {
    let message = compile_message(
        r#"
trait Limits {
    const LIMIT: int
}
struct Bounds {}
impl Limits for Bounds {
    const LIMIT: int = 9223372036854775807 + 1
}
fn probe() -> int {
    let values: [int; Bounds::LIMIT] = [1, 2]
    values.len()
}
probe()
"#,
    );
    assert_associated_diagnostic(
        &message,
        "E0423",
        "Bounds::LIMIT",
        "an array length",
        "cannot be computed",
    );
}

#[test]
fn an_associated_constant_through_a_type_parameter_works_inside_a_lambda() {
    let result = run_ok(
        r#"
trait Limits {
    const LIMIT: int
    fn id(self) -> int
}
struct Cell { value: int }
impl Limits for Cell {
    const LIMIT: int = 6
    fn id(self) -> int { self.value }
}
fn take<T: Limits>(item: T) -> int {
    let limit = fn() -> int { T::LIMIT }
    item.id() + limit()
}
fn probe() -> int {
    take(Cell { value: 1 })
}
probe()
"#,
    );
    assert_eq!(result.as_int(), Some(7));
}

#[test]
fn a_constant_resolves_the_same_through_every_receiver_spelling() {
    let result = run_ok(
        r#"
trait Source {
    const BASE: int
    const LIMIT: int
    fn own(self) -> int
}
struct Cell { value: int }
impl Source for Cell {
    const BASE: int = 5
    const LIMIT: int = Self::BASE
    fn own(self) -> int { Self::LIMIT }
}
fn probe() -> int {
    let cell = Cell { value: 0 }
    Source::LIMIT * 100 + Cell::LIMIT * 10 + cell.own()
}
probe()
"#,
    );
    assert_eq!(result.as_int(), Some(555));
}

#[test]
fn a_trait_qualified_constant_is_an_array_length() {
    let result = run_ok(
        r#"
trait Source {
    const LIMIT: int
}
struct Counter { value: int }
impl Source for Counter {
    const LIMIT: int = 3
}
fn probe() -> int {
    let slots: [int; Source::LIMIT] = [1, 2, 3]
    slots.len()
}
probe()
"#,
    );
    assert_eq!(result.as_int(), Some(3));
}

#[test]
fn a_struct_field_may_use_a_constant_declared_later() {
    let result = run_ok(
        r#"
trait Limits {
    const LIMIT: int
}
struct Holder { values: [int; Bounds::LIMIT] }
struct Bounds {}
impl Limits for Bounds {
    const LIMIT: int = 2
}
fn probe() -> int {
    let holder = Holder { values: [4, 5] }
    holder.values.len()
}
probe()
"#,
    );
    assert_eq!(result.as_int(), Some(2));
}

#[test]
fn a_projection_in_parameter_position_is_normalized_at_the_call_site() {
    // deducing t must not fail on it and the check must be retried once t is
    let result = run_ok(
        r#"
trait Source {
    type Item
    fn next(self) -> Self::Item
}
struct Counter { value: int }
impl Source for Counter {
    type Item = int
    fn next(self) -> int { self.value }
}
fn accept<T: Source>(item: T, value: T::Item) -> T::Item {
    item.next()
}
fn probe() -> int {
    accept(Counter { value: 7 }, 9)
}
probe()
"#,
    );
    assert_eq!(result.as_int(), Some(7));
}

#[test]
fn a_wrong_argument_for_a_projected_parameter_is_still_rejected() {
    // the companion of the test above: relaxing deduction must not relax the
    let message = compile_message(
        r#"
trait Source {
    type Item
    fn next(self) -> Self::Item
}
struct Counter { value: int }
impl Source for Counter {
    type Item = int
    fn next(self) -> int { self.value }
}
fn accept<T: Source>(item: T, value: T::Item) -> T::Item {
    item.next()
}
fn probe() -> int {
    accept(Counter { value: 7 }, "nope")
}
probe()
"#,
    );
    assert!(
        message.contains("E0301") && message.contains("string"),
        "a mismatched projected argument must still be rejected: {message}"
    );
}

// from this one generator, so an accepted and a rejected side cannot drift.
const CONSTRAINED_PRELUDE: &str = r#"
trait Source {
    type Item
    const LIMIT: int
    fn next(self) -> Self::Item
}
struct Counter { value: int }
impl Source for Counter {
    type Item = int
    const LIMIT: int = 3
    fn next(self) -> int { self.value }
}
struct Flagger { flag: bool }
impl Source for Flagger {
    type Item = bool
    const LIMIT: int = 9
    fn next(self) -> bool { self.flag }
}
struct Plain { note: int }
"#;

const SATISFYING_ARGUMENT: &str = "Counter { value: 1 }";
const WRONG_BINDING_ARGUMENT: &str = "Flagger { flag: true }";
const WRONG_BOUND_ARGUMENT: &str = "Plain { note: 1 }";

#[derive(Clone, Copy)]
enum CallForm {
    Free,
    FreeInAssociated,
    Lambda,
    GenericToGeneric,
    AssociatedFunction,
    Method,
}

const EVERY_CALL_FORM: &[CallForm] = &[
    CallForm::Free,
    CallForm::FreeInAssociated,
    CallForm::Lambda,
    CallForm::GenericToGeneric,
    CallForm::AssociatedFunction,
    CallForm::Method,
];

const SELF_AWARE_CALL_FORMS: &[CallForm] = &[CallForm::AssociatedFunction, CallForm::Method];

fn form_name(form: CallForm) -> &'static str {
    match form {
        CallForm::Free => "a free function",
        CallForm::FreeInAssociated => "a free function called from an associated function",
        CallForm::Lambda => "a call inside a lambda",
        CallForm::GenericToGeneric => "a generic-to-generic call",
        CallForm::AssociatedFunction => "an associated function of an inherent impl",
        CallForm::Method => "a method through a value",
    }
}

fn constrained_program(form: CallForm, bound: &str, argument: &str) -> String {
    let body = match form {
        CallForm::Free => format!(
            "fn drain<U: {bound}>(c: U) -> int {{ 7 }}\nfn probe() -> int {{ drain({argument}) }}\nprobe()"
        ),
        CallForm::FreeInAssociated => format!(
            "fn drain<U: {bound}>(c: U) -> int {{ 7 }}\nstruct Host {{ seed: int }}\nimpl Host {{\n    fn go() -> int {{ drain({argument}) }}\n}}\nHost::go()"
        ),
        CallForm::Lambda => format!(
            "fn drain<U: {bound}>(c: U) -> int {{ 7 }}\nfn probe() -> int {{\n    let f = fn() {{ return drain({argument}) }}\n    f()\n}}\nprobe()"
        ),
        CallForm::GenericToGeneric => format!(
            "fn drain<U: {bound}>(c: U) -> int {{ 7 }}\nfn outer<V>(c: V) -> int {{ drain(c) }}\nfn probe() -> int {{ outer({argument}) }}\nprobe()"
        ),
        CallForm::AssociatedFunction => format!(
            "impl Counter {{\n    fn drain<U: {bound}>(c: U) -> int {{ 7 }}\n}}\nfn probe() -> int {{ Counter::drain({argument}) }}\nprobe()"
        ),
        CallForm::Method => format!(
            "impl Counter {{\n    fn drain<U: {bound}>(self, c: U) -> int {{ 7 }}\n}}\nfn probe() -> int {{\n    let host = Counter {{ value: 1 }}\n    host.drain({argument})\n}}\nprobe()"
        ),
    };
    format!("{CONSTRAINED_PRELUDE}{body}\n")
}

fn rejected_message(form: CallForm, bound: &str, argument: &str) -> String {
    let source = constrained_program(form, bound, argument);
    match Runtime::new().compile(&source, CompileOptions::default()) {
        Ok(_) => panic!("{} must reject '{bound}' here:\n{source}", form_name(form)),
        Err(error) => error.to_string(),
    }
}

fn accepted_value(form: CallForm, bound: &str, argument: &str) -> i64 {
    let source = constrained_program(form, bound, argument);
    match run(&source, "sema_constraint_tests.aelys") {
        Ok(value) => value
            .as_int()
            .unwrap_or_else(|| panic!("{} must return an int:\n{source}", form_name(form))),
        Err(error) => panic!(
            "{} must keep accepting '{bound}': {error}\n{source}",
            form_name(form)
        ),
    }
}

fn assert_obligation_enforced(
    forms: &[CallForm],
    bound: &str,
    wrong: &str,
    code: &str,
    subject: &str,
    reason: &str,
    help: &str,
) {
    for form in forms {
        let message = rejected_message(*form, bound, wrong);
        assert_associated_diagnostic(&message, code, subject, reason, help);
        assert_eq!(
            accepted_value(*form, bound, SATISFYING_ARGUMENT),
            7,
            "{} must still accept the satisfying argument for '{bound}'",
            form_name(*form)
        );
    }
}

#[test]
fn every_call_form_enforces_a_trait_bound() {
    assert_obligation_enforced(
        EVERY_CALL_FORM,
        "Source",
        WRONG_BOUND_ARGUMENT,
        "E0338",
        "Plain",
        "",
        "add an impl or change the bound",
    );
}

#[test]
fn every_call_form_enforces_a_literal_binding() {
    assert_obligation_enforced(
        EVERY_CALL_FORM,
        "Source<Item = int>",
        WRONG_BINDING_ARGUMENT,
        "E0424",
        "Item",
        "a bound on 'U'",
        "change the requested binding or the impl",
    );
}

#[test]
fn every_call_form_enforces_a_nominal_binding() {
    assert_obligation_enforced(
        EVERY_CALL_FORM,
        "Source<Item = Counter::Item>",
        WRONG_BINDING_ARGUMENT,
        "E0424",
        "Item",
        "a bound on 'U'",
        "change the requested binding or the impl",
    );
}

#[test]
fn impl_call_forms_enforce_a_self_binding() {
    assert_obligation_enforced(
        SELF_AWARE_CALL_FORMS,
        "Source<Item = Self::Item>",
        WRONG_BINDING_ARGUMENT,
        "E0424",
        "Item",
        "a bound on 'U'",
        "change the requested binding or the impl",
    );
}

#[test]
fn impl_call_forms_enforce_a_self_constant_binding() {
    assert_obligation_enforced(
        SELF_AWARE_CALL_FORMS,
        "Source<LIMIT = Self::LIMIT>",
        WRONG_BINDING_ARGUMENT,
        "E0424",
        "LIMIT",
        "a bound on 'U'",
        "change the requested binding or the impl",
    );
}

#[test]
fn a_nominal_constant_binding_holds_on_a_free_function() {
    let message = rejected_message(
        CallForm::Free,
        "Source<LIMIT = Counter::LIMIT>",
        WRONG_BINDING_ARGUMENT,
    );
    assert_associated_diagnostic(
        &message,
        "E0424",
        "LIMIT",
        "a bound on 'U'",
        "change the requested binding or the impl",
    );
    assert_eq!(
        accepted_value(
            CallForm::Free,
            "Source<LIMIT = Counter::LIMIT>",
            SATISFYING_ARGUMENT
        ),
        7
    );
}

#[test]
fn a_method_bound_met_through_a_supertrait_is_accepted() {
    let result = run_ok(
        r#"
trait Base {
    fn base(self) -> int
}
trait Extra: Base {
    fn extra(self) -> int
}
struct Thing { value: int }
impl Base for Thing {
    fn base(self) -> int { 1 }
}
impl Extra for Thing {
    fn extra(self) -> int { 2 }
}
struct Host { seed: int }
impl Host {
    fn take<T: Extra>(self, v: T) -> int { 3 }
}
fn probe() -> int {
    let host = Host { seed: 0 }
    host.take(Thing { value: 1 })
}
probe()
"#,
    );
    assert_eq!(result.as_int(), Some(3));
}

// own, a shape `constrained_program` cannot render, so it has its own generator
fn generic_struct_impl_program(bound: &str, inner: &str) -> String {
    format!(
        r#"
trait Alpha {{
    fn tag(self) -> int
}}
trait Source {{
    type Item
    fn next(self) -> Self::Item
}}
struct Good {{ value: int }}
impl Alpha for Good {{
    fn tag(self) -> int {{ 5 }}
}}
impl Source for Good {{
    type Item = int
    fn next(self) -> int {{ self.value }}
}}
struct Bad {{ note: int }}
impl Source for Bad {{
    type Item = bool
    fn next(self) -> bool {{ true }}
}}
struct Wrapper<T> {{ inner: T }}
impl<T: {bound}> Wrapper<T> {{
    fn plain(self) -> int {{ 5 }}
}}
fn probe() -> int {{
    let w = Wrapper {{ inner: {inner} }}
    w.plain()
}}
probe()
"#
    )
}

const IMPL_BOUND_SATISFYING_INNER: &str = "Good { value: 1 }";
const IMPL_BOUND_VIOLATING_INNER: &str = "Bad { note: 1 }";

#[test]
fn a_method_on_a_generic_struct_keeps_its_impl_bound() {
    let accepted = run(
        &generic_struct_impl_program("Alpha", IMPL_BOUND_SATISFYING_INNER),
        "sema_constraint_tests.aelys",
    )
    .expect("the satisfying receiver must keep running");
    assert_eq!(accepted.as_int(), Some(5));
    let message = compile_message(&generic_struct_impl_program(
        "Alpha",
        IMPL_BOUND_VIOLATING_INNER,
    ));
    assert_associated_diagnostic(
        &message,
        "E0338",
        "Bad",
        "",
        "add an impl or change the bound",
    );
}

#[test]
fn a_method_on_a_generic_struct_keeps_its_impl_binding() {
    let accepted = run(
        &generic_struct_impl_program("Source<Item = int>", IMPL_BOUND_SATISFYING_INNER),
        "sema_constraint_tests.aelys",
    )
    .expect("the satisfying receiver must keep running");
    assert_eq!(accepted.as_int(), Some(5));
    let message = compile_message(&generic_struct_impl_program(
        "Source<Item = int>",
        IMPL_BOUND_VIOLATING_INNER,
    ));
    assert_associated_diagnostic(
        &message,
        "E0424",
        "Item",
        "a bound on 'T'",
        "change the requested binding or the impl",
    );
}

fn supertrait_binding_program(requested: &str) -> String {
    format!(
        r#"
trait Base {{
    type Item
    fn base(self) -> Self::Item
}}
trait Extra: Base {{
    fn extra(self) -> int
}}
struct Thing {{ value: int }}
impl Base for Thing {{
    type Item = int
    fn base(self) -> int {{ 1 }}
}}
impl Extra for Thing {{
    fn extra(self) -> int {{ 2 }}
}}
struct Host {{ seed: int }}
impl Host {{
    fn take<T: Extra<Item = {requested}>>(self, v: T) -> int {{ 3 }}
}}
fn probe() -> int {{
    let host = Host {{ seed: 0 }}
    host.take(Thing {{ value: 1 }})
}}
probe()
"#
    )
}

#[test]
fn a_binding_on_a_supertrait_associated_type_is_enforced() {
    let accepted = run(
        &supertrait_binding_program("int"),
        "sema_constraint_tests.aelys",
    )
    .expect("the binding the supertrait impl provides must keep running");
    assert_eq!(accepted.as_int(), Some(3));
    let message = compile_message(&supertrait_binding_program("bool"));
    assert_associated_diagnostic(
        &message,
        "E0424",
        "Item",
        "a bound on 'T'",
        "change the requested binding or the impl",
    );
}

fn nominal_where_clause_program(receiver: &str) -> String {
    format!(
        r#"
trait Source {{
    type Item
    fn next(self) -> Self::Item
}}
struct Counter {{ value: int }}
impl Source for Counter {{
    type Item = int
    fn next(self) -> int {{ self.value }}
}}
struct Plain {{ note: int }}
struct Host {{ seed: int }}
impl Host {{
    fn drain<U>(self, c: U) -> int where {receiver}: Source {{ 7 }}
}}
fn probe() -> int {{
    let host = Host {{ seed: 0 }}
    host.drain(Counter {{ value: 1 }})
}}
probe()
"#
    )
}

#[test]
fn a_method_where_clause_on_a_struct_is_enforced() {
    let accepted = run(
        &nominal_where_clause_program("Counter"),
        "sema_constraint_tests.aelys",
    )
    .expect("the met where clause must keep running");
    assert_eq!(accepted.as_int(), Some(7));
    let message = compile_message(&nominal_where_clause_program("Plain"));
    assert_associated_diagnostic(
        &message,
        "E0338",
        "Plain",
        "",
        "add an impl or change the bound",
    );
}

fn qualified_constant_binding_program(argument: &str) -> String {
    format!(
        r#"
trait Source {{
    const LIMIT: int
    fn next(self) -> int
}}
trait Other {{
    const LIMIT: int
}}
struct Counter {{ value: int }}
impl Source for Counter {{
    const LIMIT: int = 3
    fn next(self) -> int {{ self.value }}
}}
impl Other for Counter {{
    const LIMIT: int = 99
}}
struct Big {{ value: int }}
impl Source for Big {{
    const LIMIT: int = 99
    fn next(self) -> int {{ self.value }}
}}
fn drain<U: Source<LIMIT = Other::LIMIT>>(c: U) -> int {{ 7 }}
fn probe() -> int {{ drain({argument}) }}
probe()
"#
    )
}

#[test]
fn a_constant_binding_reads_the_trait_its_qualifier_names() {
    let accepted = run(
        &qualified_constant_binding_program("Big { value: 1 }"),
        "sema_constraint_tests.aelys",
    )
    .expect("the argument matching the qualified constant must keep running");
    assert_eq!(accepted.as_int(), Some(7));
    let message = compile_message(&qualified_constant_binding_program("Counter { value: 1 }"));
    assert_associated_diagnostic(
        &message,
        "E0424",
        "LIMIT",
        "a bound on 'U'",
        "change the requested binding or the impl",
    );
}

#[test]
fn a_constant_binding_no_side_can_evaluate_says_so() {
    let message = compile_message(
        r#"
trait Source {
    const LIMIT: int
    fn next(self) -> int
}
struct Counter { value: int }
impl Source for Counter {
    const LIMIT: int = 1 / 0
    fn next(self) -> int { self.value }
}
struct Big { value: int }
impl Source for Big {
    const LIMIT: int = 1 / 0
    fn next(self) -> int { self.value }
}
fn drain<U: Source<LIMIT = Counter::LIMIT>>(c: U) -> int { 7 }
fn probe() -> int { drain(Big { value: 1 }) }
probe()
"#,
    );
    assert!(
        message.contains("E0429") && message.contains("could not be evaluated"),
        "an unevaluable constant must not be reported as a disagreement: {message}"
    );
    assert!(
        !message.contains("E0424"),
        "an unevaluable constant must not be reported as a disagreement: {message}"
    );
}

// a generic impl and on the concrete twin, so an accepted and a rejected side
const ASSOCIATED_PRELUDE: &str = r#"
trait Source {
    fn next(self) -> int
}
struct Counter { value: int }
impl Source for Counter {
    fn next(self) -> int { return self.value }
}
struct Plain { note: int }
trait Src<S> {
    fn make(value: S) -> Self
}
"#;

#[derive(Clone, Copy)]
enum AssociatedShape {
    FirstArgument,
    SecondArgument,
    ReturnTypeOnly,
    MethodTypeParam,
    MethodTypeParamBounded,
    ImplTypeParam,
    TraitImpl,
}

const EVERY_ASSOCIATED_SHAPE: &[AssociatedShape] = &[
    AssociatedShape::FirstArgument,
    AssociatedShape::SecondArgument,
    AssociatedShape::ReturnTypeOnly,
    AssociatedShape::MethodTypeParam,
    AssociatedShape::TraitImpl,
];

fn associated_shape_name(shape: AssociatedShape) -> &'static str {
    match shape {
        AssociatedShape::FirstArgument => {
            "an associated function deducing the impl from argument 1"
        }
        AssociatedShape::SecondArgument => {
            "an associated function deducing the impl from argument 2"
        }
        AssociatedShape::ReturnTypeOnly => {
            "an associated function deducing the impl from the return type"
        }
        AssociatedShape::MethodTypeParam => {
            "an associated function carrying its own type parameter"
        }
        AssociatedShape::MethodTypeParamBounded => {
            "an associated function carrying its own bounded type parameter"
        }
        AssociatedShape::ImplTypeParam => {
            "an associated function on an impl carrying a type parameter its target omits"
        }
        AssociatedShape::TraitImpl => "an associated function of a trait impl",
    }
}

#[derive(Clone, Copy)]
struct AssociatedPayload {
    ty: &'static str,
    value: &'static str,
    field: &'static str,
}

const INT_PAYLOAD: AssociatedPayload = AssociatedPayload {
    ty: "int",
    value: "7",
    field: "",
};
const SATISFYING_PAYLOAD: AssociatedPayload = AssociatedPayload {
    ty: "Counter",
    value: "Counter { value: 7 }",
    field: ".value",
};
const VIOLATING_PAYLOAD: AssociatedPayload = AssociatedPayload {
    ty: "Plain",
    value: "Plain { note: 7 }",
    field: ".note",
};

fn associated_program(
    shape: AssociatedShape,
    generic: bool,
    bound: Option<&str>,
    payload: AssociatedPayload,
) -> String {
    let item = if generic { "T" } else { payload.ty };
    let target = if generic { "W<T>" } else { "W" };
    let struct_decl = if generic {
        "struct W<T> { inner: T }".to_string()
    } else {
        format!("struct W {{ inner: {} }}", payload.ty)
    };
    let header = match (generic, bound) {
        (true, Some(bound)) => format!("<T: {bound}>"),
        (true, None) => "<T>".to_string(),
        (false, _) => String::new(),
    };
    let value = payload.value;
    let field = payload.field;
    let held = payload.ty;
    let body = match shape {
        AssociatedShape::FirstArgument => format!(
            "impl{header} {target} {{\n    fn make(value: {item}) -> {target} {{ return W {{ inner: value }} }}\n}}\nfn probe() -> int {{\n    let w = W::make({value})\n    return w.inner{field}\n}}\nprobe()"
        ),
        AssociatedShape::SecondArgument => format!(
            "impl{header} {target} {{\n    fn make(marker: int, value: {item}) -> {target} {{ return W {{ inner: value }} }}\n}}\nfn probe() -> int {{\n    let w = W::make(0, {value})\n    return w.inner{field}\n}}\nprobe()"
        ),
        AssociatedShape::ReturnTypeOnly => format!(
            "impl{header} {target} {{\n    fn make() -> Option<{item}> {{ return None }}\n}}\nfn probe() -> int {{\n    let held: Option<{held}> = W::make()\n    return match held {{ Some(value) => value{field}, None => 7 }}\n}}\nprobe()"
        ),
        AssociatedShape::MethodTypeParam => format!(
            "impl{header} {target} {{\n    fn make<U>(value: {item}, extra: U) -> {target} {{ return W {{ inner: value }} }}\n}}\nfn probe() -> int {{\n    let w = W::make({value}, true)\n    return w.inner{field}\n}}\nprobe()"
        ),
        AssociatedShape::MethodTypeParamBounded => format!(
            "impl{header} {target} {{\n    fn make<U: Source>(value: {item}, extra: U) -> int {{ return extra.next() }}\n}}\nfn probe() -> int {{\n    return W::make({value}, Counter {{ value: 7 }})\n}}\nprobe()"
        ),
        AssociatedShape::ImplTypeParam => {
            let impl_header = match (generic, bound) {
                (true, Some(bound)) => format!("<T: {bound}, U: Source>"),
                (true, None) => "<T, U: Source>".to_string(),
                (false, _) => "<U: Source>".to_string(),
            };
            format!(
                "impl{impl_header} {target} {{\n    fn make(value: {item}, extra: U) -> int {{ return extra.next() }}\n}}\nfn probe() -> int {{\n    return W::make({value}, Counter {{ value: 7 }})\n}}\nprobe()"
            )
        }
        AssociatedShape::TraitImpl => format!(
            "impl{header} Src<{item}> for {target} {{\n    fn make(value: {item}) -> {target} {{ return W {{ inner: value }} }}\n}}\nfn probe() -> int {{\n    let w = W::make({value})\n    return w.inner{field}\n}}\nprobe()"
        ),
    };
    format!("{ASSOCIATED_PRELUDE}{struct_decl}\n{body}\n")
}

fn associated_value(source: &str, shape: AssociatedShape, description: &str) -> i64 {
    match run(source, "sema_constraint_tests.aelys") {
        Ok(value) => value.as_int().unwrap_or_else(|| {
            panic!(
                "{} must return an int {description}:\n{source}",
                associated_shape_name(shape)
            )
        }),
        Err(error) => panic!(
            "{} must run {description}: {error}\n{source}",
            associated_shape_name(shape)
        ),
    }
}

#[test]
fn every_associated_shape_runs_through_a_generic_impl() {
    for shape in EVERY_ASSOCIATED_SHAPE {
        let source = associated_program(*shape, true, None, INT_PAYLOAD);
        assert_eq!(
            associated_value(&source, *shape, "through a generic impl"),
            7,
            "{} must carry its value out of a generic impl",
            associated_shape_name(*shape)
        );
    }
}

#[test]
fn every_associated_shape_still_runs_through_a_concrete_impl() {
    for shape in EVERY_ASSOCIATED_SHAPE {
        let source = associated_program(*shape, false, None, INT_PAYLOAD);
        assert_eq!(
            associated_value(&source, *shape, "through a concrete impl"),
            7,
            "{} must keep working on a concrete impl",
            associated_shape_name(*shape)
        );
    }
}

#[test]
fn every_associated_shape_keeps_accepting_a_satisfying_bound() {
    for shape in EVERY_ASSOCIATED_SHAPE {
        let source = associated_program(*shape, true, Some("Source"), SATISFYING_PAYLOAD);
        assert_eq!(
            associated_value(&source, *shape, "with a satisfied impl bound"),
            7,
            "{} must accept an argument that satisfies the impl bound",
            associated_shape_name(*shape)
        );
    }
}

#[test]
fn an_impl_type_parameter_absent_from_the_target_type_is_rejected() {
    for (generic, bound, payload) in [
        (true, None, INT_PAYLOAD),
        (false, None, INT_PAYLOAD),
        (true, Some("Source"), SATISFYING_PAYLOAD),
    ] {
        let source = associated_program(AssociatedShape::ImplTypeParam, generic, bound, payload);
        let message = compile_message(&source);
        let target = if generic { "W<T>" } else { "W" };
        assert_associated_diagnostic(
            &message,
            "E0430",
            "'U'",
            &format!("inherent impl of '{target}'"),
            "move it onto the method",
        );
    }
}

// this shape would share one mangled name
#[test]
fn the_rejected_impl_type_parameter_is_the_shape_that_collided() {
    let source = r#"
trait Tag {
    fn tag(self) -> int
}
struct A { x: int }
struct B { x: int }
impl Tag for A {
    fn tag(self) -> int { return 10 }
}
impl Tag for B {
    fn tag(self) -> int { return 20 }
}
struct W<T> { inner: T }
impl<T, U: Tag> W<T> {
    fn make(seed: T, tagger: U) -> int {
        return tagger.tag()
    }
}
fn probe() -> int {
    return W::make(1, A { x: 0 }) * 100 + W::make(1, B { x: 0 })
}
probe()
"#;
    let message = compile_message(source);
    assert_associated_diagnostic(
        &message,
        "E0430",
        "'U'",
        "inherent impl of 'W<T>'",
        "move it onto the method",
    );
}

// impl-method type parameters are not monomorphized, so the advice e0352 offers
#[test]
fn a_bounded_method_type_parameter_on_a_generic_impl_stays_rejected() {
    let source = associated_program(
        AssociatedShape::MethodTypeParamBounded,
        true,
        None,
        INT_PAYLOAD,
    );
    let message = compile_message(&source);
    assert!(
        message.contains("E0352"),
        "a bounded method type parameter must stay rejected under E0352: {message}"
    );
}

#[test]
fn every_associated_shape_still_rejects_a_violated_bound() {
    for shape in EVERY_ASSOCIATED_SHAPE {
        let source = associated_program(*shape, true, Some("Source"), VIOLATING_PAYLOAD);
        let message = match Runtime::new().compile(&source, CompileOptions::default()) {
            Ok(_) => panic!(
                "{} must reject an argument that violates the impl bound:\n{source}",
                associated_shape_name(*shape)
            ),
            Err(error) => error.to_string(),
        };
        assert_associated_diagnostic(
            &message,
            "E0338",
            "Plain",
            "",
            "add an impl or change the bound",
        );
    }
}

#[test]
fn an_unspecialized_generic_call_names_the_call_not_a_mangled_symbol() {
    let message = compile_message(
        r#"
struct W<T> { inner: T }
impl<T> W<T> {
    fn label() -> int { return 1 }
}
fn probe() -> int { return W::label() }
probe()
"#,
    );
    assert_associated_diagnostic(
        &message,
        "E0352",
        "W::label",
        "",
        "add a type annotation that pins every type parameter",
    );
    assert!(
        !message.contains("__aelys"),
        "E0352 must never hand the user an internal mangled symbol: {message}"
    );
}

#[test]
fn a_recursive_generic_associated_function_is_a_diagnostic() {
    let message = compile_message(
        r#"
struct W<T> { inner: T }
impl<T> W<T> {
    fn deeper(value: T) -> int {
        let w = W { inner: value }
        return W::deeper(w)
    }
}
fn probe() -> int { return W::deeper(1) }
probe()
"#,
    );
    assert!(
        message.contains("E0344") || message.contains("E0345"),
        "an unbounded generic impl recursion must be a compile diagnostic: {message}"
    );
}

fn undeclared_binding_message(form: CallForm) -> String {
    rejected_message(form, "Source<Nope = int>", SATISFYING_ARGUMENT)
}

#[test]
fn a_binding_the_trait_never_declares_names_that_fault() {
    for form in EVERY_CALL_FORM {
        let message = undeclared_binding_message(*form);
        assert_associated_diagnostic(
            &message,
            "E0424",
            "Nope",
            "a bound on 'U'",
            "name an item the trait declares",
        );
        assert!(
            message.contains("declares no associated type or constant 'Nope'"),
            "{} must state that the trait declares no such item: {message}",
            form_name(*form)
        );
        assert!(
            !message.contains("poisoned type"),
            "{} must not leak the internal recovery marker: {message}",
            form_name(*form)
        );
        assert_eq!(
            accepted_value(*form, "Source<Item = int>", SATISFYING_ARGUMENT),
            7,
            "{} must still accept a binding the trait does declare",
            form_name(*form)
        );
    }
}

#[test]
fn a_constant_binding_mismatch_prints_both_values_it_compared() {
    for form in EVERY_CALL_FORM {
        let message = rejected_message(
            *form,
            "Source<LIMIT = Counter::LIMIT>",
            WRONG_BINDING_ARGUMENT,
        );
        assert_associated_diagnostic(
            &message,
            "E0424",
            "LIMIT",
            "a bound on 'U'",
            "change the requested binding or the impl",
        );
        assert!(
            message.contains("'Source::LIMIT = 3'") && message.contains("which provides 9"),
            "{} must compare the two folded values, not the projections it was handed: {message}",
            form_name(*form)
        );
    }
}

// and the rejected half of one program, so the reason a code renders is pinned

fn required_item_program(copies: usize) -> String {
    let mut items = String::new();
    for _ in 0..copies {
        items.push_str("    type Item = int\n");
    }
    format!(
        r#"
trait Source {{
    type Item
    fn next(self) -> Self::Item
}}
struct Counter {{ value: int }}
impl Source for Counter {{
{items}    fn next(self) -> int {{ self.value }}
}}
fn probe() -> int {{
    let counter = Counter {{ value: 7 }}
    counter.next()
}}
probe()
"#
    )
}

#[test]
fn a_missing_required_item_names_the_impl_it_is_missing_from() {
    let message = compile_message(&required_item_program(0));
    assert_associated_diagnostic(
        &message,
        "E0421",
        "Item",
        "impl of trait 'Source' for 'Counter'",
        "define 'type Item'",
    );
    assert_eq!(
        run_ok(&required_item_program(1)).as_int(),
        Some(7),
        "the impl that defines the item exactly once must keep running"
    );
}

#[test]
fn a_duplicated_required_item_names_the_impl_that_repeats_it() {
    let message = compile_message(&required_item_program(2));
    assert_associated_diagnostic(
        &message,
        "E0426",
        "Item",
        "impl of trait 'Source' for 'Counter'",
        "defined exactly once",
    );
}

fn associated_const_program(declared: &str, value: &str) -> String {
    format!(
        r#"
trait Limits {{
    const LIMIT: int
}}
struct Bounds {{}}
impl Limits for Bounds {{
    const LIMIT: {declared} = {value}
}}
fn probe() -> int {{ 7 }}
probe()
"#
    )
}

#[test]
fn an_associated_constant_reports_which_half_of_the_impl_disagrees() {
    assert_eq!(
        run_ok(&associated_const_program("int", "3")).as_int(),
        Some(7),
        "the agreeing constant must keep compiling"
    );
    let declared = compile_message(&associated_const_program("string", "\"ten\""));
    assert_associated_diagnostic(
        &declared,
        "E0422",
        "LIMIT",
        "declared type in the impl for 'Bounds'",
        "declare the same type as the trait",
    );
    let value = compile_message(&associated_const_program("int", "\"ten\""));
    assert_associated_diagnostic(
        &value,
        "E0422",
        "LIMIT",
        "constant value in the impl for 'Bounds'",
        "write an initialiser of type 'i64'",
    );
}

fn inherent_item_program(item: &str, in_trait_impl: bool) -> String {
    let header = if in_trait_impl {
        "impl Limits for Bounds"
    } else {
        "impl Bounds"
    };
    format!(
        r#"
trait Limits {{
    type Item
    const LIMIT: int
}}
struct Bounds {{}}
{header} {{
{item}
}}
fn probe() -> int {{ 7 }}
probe()
"#
    )
}

#[test]
fn an_item_no_trait_declares_names_the_inherent_impl_that_holds_it() {
    let both = "    type Item = int\n    const LIMIT: int = 3";
    assert_eq!(
        run_ok(&inherent_item_program(both, true)).as_int(),
        Some(7),
        "the same two items inside a trait impl must keep compiling"
    );
    let associated_type = compile_message(&inherent_item_program("    type Item = int", false));
    assert_associated_diagnostic(
        &associated_type,
        "E0425",
        "Item",
        "inherent impl of 'Bounds'",
        "impl of a trait that declares 'Item'",
    );
    let associated_const =
        compile_message(&inherent_item_program("    const LIMIT: int = 3", false));
    assert_associated_diagnostic(
        &associated_const,
        "E0425",
        "LIMIT",
        "inherent impl of 'Bounds'",
        "impl of a trait that declares 'LIMIT'",
    );
}

#[derive(Clone, Copy)]
enum ProjectionRole {
    ReturnType,
    Parameter,
    LetAnnotation,
    StructField,
    ArrayLength,
    Bound,
    ValueExpression,
}

const EVERY_PROJECTION_ROLE: &[(ProjectionRole, &str)] = &[
    (ProjectionRole::ReturnType, "a return type"),
    (ProjectionRole::Parameter, "a parameter type"),
    (ProjectionRole::LetAnnotation, "a type annotation"),
    (ProjectionRole::StructField, "a struct field"),
    (ProjectionRole::ArrayLength, "an array length"),
    (ProjectionRole::Bound, "a bound"),
    (ProjectionRole::ValueExpression, "a value expression"),
];

fn projection_role_program(role: ProjectionRole, competing: bool) -> String {
    let second = if competing {
        r#"impl Right for Cell {
    type Item = int
    const LIMIT: int = 2
}
"#
    } else {
        ""
    };
    let body = match role {
        ProjectionRole::ReturnType => "fn probe() -> Cell::Item { 7 }\nprobe()",
        ProjectionRole::Parameter => {
            "fn drain(v: Cell::Item) -> int { v }\nfn probe() -> int { drain(7) }\nprobe()"
        }
        ProjectionRole::LetAnnotation => {
            "fn probe() -> int {\n    let v: Cell::Item = 7\n    v\n}\nprobe()"
        }
        ProjectionRole::StructField => {
            "struct Holder { item: Cell::Item }\nfn probe() -> int {\n    let h = Holder { item: 7 }\n    h.item\n}\nprobe()"
        }
        ProjectionRole::ArrayLength => {
            "fn probe() -> int {\n    let values: [int; Cell::LIMIT] = [3, 4]\n    values.len() + 5\n}\nprobe()"
        }
        ProjectionRole::Bound => {
            "fn drain<U: Left<Item = Cell::Item>>(c: U) -> int { 7 }\nfn probe() -> int { drain(Cell { value: 1 }) }\nprobe()"
        }
        ProjectionRole::ValueExpression => "fn probe() -> int { Cell::LIMIT + 5 }\nprobe()",
    };
    format!(
        r#"
trait Left {{
    type Item
    const LIMIT: int
}}
trait Right {{
    type Item
    const LIMIT: int
}}
struct Cell {{ value: int }}
impl Left for Cell {{
    type Item = int
    const LIMIT: int = 2
}}
{second}{body}
"#
    )
}

#[test]
fn an_unresolved_projection_states_the_position_it_was_written_in() {
    for (role, expected) in EVERY_PROJECTION_ROLE {
        assert_eq!(
            run_ok(&projection_role_program(*role, false)).as_int(),
            Some(7),
            "the single-impl half of '{expected}' must keep running"
        );
        let message = compile_message(&projection_role_program(*role, true));
        assert_associated_diagnostic(
            &message,
            "E0423",
            "Cell::",
            expected,
            "remove one of the competing impls",
        );
    }
}

#[derive(Clone, Copy)]
enum ImplProjectionRole {
    MethodReturn,
    MethodParameter,
    AssociatedConstType,
    ArrayLength,
    LetAnnotation,
    ValueExpression,
}

const EVERY_IMPL_PROJECTION_ROLE: &[(ImplProjectionRole, &str, &str)] = &[
    (ImplProjectionRole::MethodReturn, "a return type", "Item"),
    (
        ImplProjectionRole::MethodParameter,
        "a parameter type",
        "Item",
    ),
    (
        ImplProjectionRole::AssociatedConstType,
        "an associated item definition",
        "Item",
    ),
    (ImplProjectionRole::ArrayLength, "an array length", "LIMIT"),
    (
        ImplProjectionRole::LetAnnotation,
        "a type annotation",
        "Item",
    ),
    (
        ImplProjectionRole::ValueExpression,
        "a value expression",
        "LIMIT",
    ),
];

fn impl_projection_role_program(role: ImplProjectionRole, competing: bool) -> String {
    let beta = if competing {
        "impl Beta for Counter {\n    type Item = int\n    const LIMIT: int = 3\n}\n"
    } else {
        ""
    };
    let (declaration, definition, probe) = match role {
        ImplProjectionRole::MethodReturn => (
            "    fn next(self) -> int\n",
            "    fn next(self) -> Counter::Item { return self.v }\n",
            "    return c.next()\n",
        ),
        ImplProjectionRole::MethodParameter => (
            "    fn take(self, x: int) -> int\n",
            "    fn take(self, x: Counter::Item) -> int { return x }\n",
            "    return c.take(5)\n",
        ),
        ImplProjectionRole::AssociatedConstType => (
            "    const K: int\n",
            "    const K: Counter::Item = 5\n",
            "    return Counter::K + c.v - 5\n",
        ),
        ImplProjectionRole::ArrayLength => (
            "    fn sizes(self) -> int\n",
            "    fn sizes(self) -> int {\n        let xs: [int; Counter::LIMIT] = [1, 2]\n        return xs.len() + 3\n    }\n",
            "    return c.sizes()\n",
        ),
        ImplProjectionRole::LetAnnotation => (
            "    fn locals(self) -> int\n",
            "    fn locals(self) -> int {\n        let x: Counter::Item = 5\n        return x\n    }\n",
            "    return c.locals()\n",
        ),
        ImplProjectionRole::ValueExpression => (
            "    fn value(self) -> int\n",
            "    fn value(self) -> int { return Counter::LIMIT + 3 }\n",
            "    return c.value()\n",
        ),
    };
    format!(
        r#"struct Counter {{ v: int }}
trait Alpha {{
    type Item
    const LIMIT: int
}}
trait Beta {{
    type Item
    const LIMIT: int
}}
trait Gamma {{
{declaration}}}
impl Alpha for Counter {{
    type Item = int
    const LIMIT: int = 2
}}
{beta}impl Gamma for Counter {{
{definition}}}
fn probe() -> int {{
    let c = Counter {{ v: 5 }}
{probe}}}
probe()
"#
    )
}

#[test]
fn an_ambiguous_projection_inside_a_trait_impl_states_its_position_everywhere() {
    for (role, expected, item) in EVERY_IMPL_PROJECTION_ROLE {
        assert_eq!(
            run_ok(&impl_projection_role_program(*role, false)).as_int(),
            Some(5),
            "the single-impl half of '{expected}' must keep running"
        );
        let message = compile_message(&impl_projection_role_program(*role, true));
        assert_associated_diagnostic(
            &message,
            "E0423",
            &format!("Counter::{item}"),
            expected,
            "remove one of the competing impls",
        );
        assert!(
            message.contains("Alpha and Beta"),
            "'{expected}' must name both competing traits: {message}"
        );
    }
}

#[derive(Clone, Copy)]
enum BindingValue {
    Literal,
    Path,
}

const EVERY_BINDING_VALUE: &[BindingValue] = &[BindingValue::Literal, BindingValue::Path];

fn constant_binding(value: BindingValue, agreeing: bool) -> String {
    match (value, agreeing) {
        (BindingValue::Literal, true) => "Source<LIMIT = 3>".to_string(),
        (BindingValue::Literal, false) => "Source<LIMIT = 9>".to_string(),
        (BindingValue::Path, true) => "Source<LIMIT = Counter::LIMIT>".to_string(),
        (BindingValue::Path, false) => "Source<LIMIT = Flagger::LIMIT>".to_string(),
    }
}

#[test]
fn a_constant_binding_accepts_and_rejects_the_same_way_written_either_way() {
    for value in EVERY_BINDING_VALUE {
        for form in EVERY_CALL_FORM {
            let bound = constant_binding(*value, true);
            assert_eq!(
                accepted_value(*form, &bound, SATISFYING_ARGUMENT),
                7,
                "{} must accept '{bound}' for the impl that provides 3",
                form_name(*form)
            );
            let message = rejected_message(*form, &bound, WRONG_BINDING_ARGUMENT);
            assert_associated_diagnostic(
                &message,
                "E0424",
                "LIMIT",
                "a bound on 'U'",
                "change the requested binding or the impl",
            );
            assert!(
                message.contains("'Source::LIMIT = 3'") && message.contains("which provides 9"),
                "{} must compare the two values for '{bound}': {message}",
                form_name(*form)
            );
            let other = constant_binding(*value, false);
            assert_eq!(
                accepted_value(*form, &other, WRONG_BINDING_ARGUMENT),
                7,
                "{} must accept '{other}' for the impl that provides 9",
                form_name(*form)
            );
        }
    }
}

#[test]
fn a_literal_bound_to_an_associated_type_names_the_namespace_it_is_not() {
    let message = rejected_message(CallForm::Free, "Source<Item = 3>", SATISFYING_ARGUMENT);
    assert_associated_diagnostic(
        &message,
        "E0424",
        "associated binding 'Source::Item = 3'",
        "a bound",
        "bind a type, or name an associated constant",
    );
    assert!(
        !message.contains("projection"),
        "a binding is not a projection and the program writes none: {message}"
    );
}

#[test]
fn a_type_bound_to_an_associated_constant_names_the_namespace_it_is_not() {
    let message = rejected_message(CallForm::Free, "Source<LIMIT = int>", SATISFYING_ARGUMENT);
    assert_associated_diagnostic(
        &message,
        "E0424",
        "associated binding 'Source::LIMIT = int'",
        "a bound",
        "bind a value, or name an associated type",
    );
    assert!(
        !message.contains("i64"),
        "the message must spell the type the source wrote: {message}"
    );
}

// rejected, and the same duplication of an item no trait declares is not.
fn duplicated_item_program(declared: bool, namespace: &str, first: &str, second: &str) -> String {
    let item = if declared { "Item" } else { "Other" };
    let declaration = match namespace {
        "type" => "    type Item\n",
        _ => "    const Item: int\n",
    };
    let definition = |value: &str| match namespace {
        "type" => format!("    type {item} = {value}\n"),
        _ => format!("    const {item}: int = {value}\n"),
    };
    let body = format!("{}{}", definition(first), definition(second));
    let probe = match namespace {
        "type" => format!("fn probe(x: Bounds::{item}) -> int {{ return 1 }}\nprobe(1)\n"),
        _ => format!("fn probe() -> int {{ return Bounds::{item} }}\nprobe()\n"),
    };
    format!(
        "struct Bounds {{ v: int }}\ntrait Limits {{\n{declaration}}}\nstruct Filler {{ w: int }}\nimpl Limits for Bounds {{\n{}{body}}}\n{probe}",
        if declared {
            String::new()
        } else {
            definition_of_the_declared_item(namespace)
        }
    )
}

fn definition_of_the_declared_item(namespace: &str) -> String {
    match namespace {
        "type" => "    type Item = int\n".to_string(),
        _ => "    const Item: int = 1\n".to_string(),
    }
}

#[test]
fn an_item_no_trait_declares_is_rejected_before_it_can_be_defined_twice() {
    for (namespace, subject) in [("type", "Item"), ("const", "Item")] {
        let declared = compile_message(&duplicated_item_program(true, namespace, "int", "int"));
        assert_associated_diagnostic(
            &declared,
            "E0426",
            subject,
            "impl of trait 'Limits' for 'Bounds'",
            "defined exactly once",
        );
    }

    // same impl never reaches it; e0425 takes the first of the two definitions
    for (first, second) in [("4", "9"), ("9", "4")] {
        let repeated = compile_message(&duplicated_item_program(false, "const", first, second));
        assert_associated_diagnostic(
            &repeated,
            "E0425",
            "Other",
            "impl of trait 'Limits' for 'Bounds'",
            "impl of a trait that declares 'Other'",
        );
    }
    for (first, second) in [("int", "string"), ("string", "int")] {
        let repeated = compile_message(&duplicated_item_program(false, "type", first, second));
        assert_associated_diagnostic(
            &repeated,
            "E0425",
            "Other",
            "impl of trait 'Limits' for 'Bounds'",
            "impl of a trait that declares 'Other'",
        );
    }
}

fn inherited_item_program(
    namespace: &str,
    in_supertrait: bool,
    base_impl: bool,
    times: usize,
) -> String {
    let (declaration, definition, returned, tail) = match namespace {
        "type" => (
            "    type Item",
            "    type Item = int",
            "3",
            "fn take(x: T::Item) -> int {\n    return x\n}\nfn probe() -> int {\n    let t = T { v: 1 }\n    return take(t.go())\n}\nprobe()\n",
        ),
        _ => (
            "    const LIMIT: int",
            "    const LIMIT: int = 3",
            "0",
            "fn probe() -> int {\n    let t = T { v: 1 }\n    return t.go() + T::LIMIT\n}\nprobe()\n",
        ),
    };
    let mut source = String::new();
    if in_supertrait {
        source.push_str(&format!("trait Base {{\n{declaration}\n}}\n"));
        source.push_str("trait Derived: Base {\n    fn go(self) -> int\n}\n");
    } else {
        source.push_str(&format!(
            "trait Derived {{\n{declaration}\n    fn go(self) -> int\n}}\n"
        ));
    }
    source.push_str("struct T { v: int }\n");
    if in_supertrait && base_impl {
        source.push_str(&format!("impl Base for T {{\n{definition}\n}}\n"));
    }
    source.push_str("impl Derived for T {\n");
    for _ in 0..times {
        source.push_str(definition);
        source.push('\n');
    }
    source.push_str(&format!("    fn go(self) -> int {{ {returned} }}\n}}\n"));
    source.push_str(tail);
    source
}

#[test]
fn an_item_a_supertrait_declares_is_still_defined_exactly_once() {
    for (namespace, item) in [("type", "Item"), ("const", "LIMIT")] {
        let message = compile_message(&inherited_item_program(namespace, false, false, 2));
        assert_associated_diagnostic(
            &message,
            "E0426",
            item,
            "impl of trait 'Derived' for 'T'",
            "defined exactly once",
        );
        assert!(
            message.contains("implementation of trait 'Derived'"),
            "E0426 must name the trait that declares '{item}': {message}"
        );
        assert_eq!(
            run_ok(&inherited_item_program(namespace, true, true, 0)).as_int(),
            Some(3),
            "the definition of '{item}' in the supertrait impl must resolve and run"
        );
        assert_eq!(
            run_ok(&inherited_item_program(namespace, false, false, 1)).as_int(),
            Some(3),
            "a locally declared '{item}' defined once must resolve and run"
        );
        let missing = compile_message(&inherited_item_program(namespace, false, false, 0));
        assert_associated_diagnostic(
            &missing,
            "E0421",
            item,
            "impl of trait 'Derived' for 'T'",
            "in the impl body",
        );
    }
}

// bounded type parameter, so the accepted half keeps the rejected half honest.
fn array_length_receiver_program(receiver: &str) -> String {
    let header = if receiver == "T" {
        "fn probe<T: Source>(s: T) -> int {"
    } else {
        "fn probe() -> int {"
    };
    let call = if receiver == "T" {
        "probe(Counter { v: 1 })"
    } else {
        "probe()"
    };
    format!(
        r#"
struct Counter {{ v: int }}
trait Source {{
    const LIMIT: int
}}
impl Source for Counter {{
    const LIMIT: int = 4
}}
{header}
    let a: [int; {receiver}::LIMIT] = [0, 0, 0, 7]
    return a[3]
}}
{call}
"#
    )
}

#[test]
fn an_array_length_taken_from_a_type_parameter_names_the_parameter() {
    assert_eq!(
        run_ok(&array_length_receiver_program("Counter")).as_int(),
        Some(7),
        "the same length over a concrete receiver must keep compiling"
    );
    let message = compile_message(&array_length_receiver_program("T"));
    assert_associated_diagnostic(
        &message,
        "E0423",
        "array length 'T::LIMIT'",
        "an array length",
        "name the constant on a concrete type",
    );
    assert!(
        message.contains("depends on the type parameter 'T'"),
        "the diagnostic must say which parameter blocks it: {message}"
    );
}

// folds a value and the arms that refuse to are rendered from one program.
fn folded_constant_program(value: &str, position: &str) -> String {
    let body = match position {
        "length" => "    let a: [int; Bounds::LIMIT] = [0, 0, 0, 7]\n    return a[3]",
        _ => "    return Bounds::LIMIT",
    };
    format!(
        r#"
struct Bounds {{ v: int }}
trait Limits {{
    const LIMIT: int
}}
impl Limits for Bounds {{
    const LIMIT: int = {value}
}}
fn probe() -> int {{
{body}
}}
probe()
"#
    )
}

#[test]
fn a_constant_the_folder_refuses_says_which_refusal_it_is() {
    assert_eq!(
        run_ok(&folded_constant_program("2 + 2", "length")).as_int(),
        Some(7),
        "an arithmetic constant must still fold into a length"
    );
    assert_eq!(
        run_ok(&folded_constant_program("2 + 2", "value")).as_int(),
        Some(4),
        "the same constant must still fold into a value"
    );

    for position in ["length", "value"] {
        // different refusal from arithmetic that cannot be computed.
        let not_constant = compile_message(&folded_constant_program("3 & 1", position));
        assert_associated_diagnostic(
            &not_constant,
            "E0423",
            "constant 'Bounds::LIMIT'",
            if position == "length" {
                "an array length"
            } else {
                "a value expression"
            },
            "use integer literals and '+ - * / %' only",
        );
        let not_computable = compile_message(&folded_constant_program("1 / 0", position));
        assert_associated_diagnostic(
            &not_computable,
            "E0423",
            "constant 'Bounds::LIMIT'",
            if position == "length" {
                "an array length"
            } else {
                "a value expression"
            },
            "give it a value that evaluates",
        );
        assert!(
            !not_computable.contains("not a constant integer expression"),
            "the two refusals must not share one message: {not_computable}"
        );
    }
}

fn competing_projection_program(competing: bool, twin: &str) -> String {
    let beta = if competing {
        "impl Beta for Counter {\n    type Item = string\n}\n"
    } else {
        ""
    };
    let (declaration, definition, probe) = match twin {
        "const" => (
            "    const K: int\n",
            "    const K: Counter::Item = 5\n",
            "fn probe() -> int { return Counter::K }\nprobe()\n",
        ),
        _ => (
            "    fn next(self) -> int\n",
            "    fn next(self) -> Counter::Item { return self.v }\n",
            "fn probe() -> int {\n    let c = Counter { v: 5 }\n    return c.next()\n}\nprobe()\n",
        ),
    };
    format!(
        r#"struct Counter {{ v: int }}
trait Alpha {{
    type Item
}}
trait Beta {{
    type Item
}}
trait Gamma {{
{declaration}}}
impl Alpha for Counter {{
    type Item = int
}}
{beta}impl Gamma for Counter {{
{definition}}}
{probe}"#
    )
}

#[test]
fn an_ambiguous_projection_in_a_signature_names_the_competing_traits() {
    for twin in ["const", "method"] {
        assert_eq!(
            run_ok(&competing_projection_program(false, twin)).as_int(),
            Some(5),
            "the {twin} twin must compile while a single impl provides 'Item'"
        );
        let message = compile_message(&competing_projection_program(true, twin));
        let position = if twin == "const" {
            "an associated item definition"
        } else {
            "a return type"
        };
        assert_associated_diagnostic(
            &message,
            "E0423",
            "Counter::Item",
            position,
            "remove one of the competing impls",
        );
        assert!(
            message.contains("Alpha and Beta"),
            "the {twin} twin must name both competing traits: {message}"
        );
    }
}

fn constant_chain(links: usize, forward: bool) -> String {
    let mut source = String::from("trait Limits {\n");
    for index in 0..=links {
        source.push_str(&format!("    const C{index}: int\n"));
    }
    source.push_str("}\nstruct Node { value: int }\nimpl Limits for Node {\n");
    if forward {
        for index in 0..links {
            source.push_str(&format!(
                "    const C{index}: int = Self::C{} + 1\n",
                index + 1
            ));
        }
        source.push_str(&format!("    const C{links}: int = 0\n}}\n"));
        source.push_str("fn probe() -> int {\n    Node::C0\n}\nprobe()\n");
    } else {
        source.push_str("    const C0: int = 0\n");
        for index in 1..=links {
            source.push_str(&format!(
                "    const C{index}: int = Self::C{} + 1\n",
                index - 1
            ));
        }
        source.push_str("}\n");
        source.push_str(&format!(
            "fn probe() -> int {{\n    Node::C{links}\n}}\nprobe()\n"
        ));
    }
    source
}

#[test]
fn a_four_thousand_link_constant_chain_is_evaluated() {
    assert_eq!(
        run_ok(&constant_chain(4000, false)).as_int(),
        Some(4000),
        "a 4000 link chain must fold without a budget"
    );
    assert_eq!(
        run_ok(&constant_chain(1200, true)).as_int(),
        Some(1200),
        "the forward direction must fold past the length the suite already pins"
    );
}

// so the length that folds and the shape that is rejected come out of one program.
fn projection_chain(links: usize, forward: bool, closed: bool, constant: bool) -> String {
    let mut edges: Vec<(usize, Option<usize>)> = Vec::new();
    if forward {
        for index in 0..links {
            edges.push((index, Some(index + 1)));
        }
        edges.push((links, if closed { Some(0) } else { None }));
    } else {
        edges.push((0, if closed { Some(links) } else { None }));
        for index in 1..=links {
            edges.push((index, Some(index - 1)));
        }
    }
    let mut source = String::from("trait Limits {\n");
    for index in 0..=links {
        if constant {
            source.push_str(&format!("    const C{index}: int\n"));
        } else {
            source.push_str(&format!("    type C{index}\n"));
        }
    }
    source.push_str("}\nstruct Node { value: int }\nimpl Limits for Node {\n");
    for (item, names) in &edges {
        let line = match (constant, names) {
            (true, Some(next)) => format!("    const C{item}: int = Self::C{next} + 1\n"),
            (true, None) => format!("    const C{item}: int = 0\n"),
            (false, Some(next)) => format!("    type C{item} = Self::C{next}\n"),
            (false, None) => format!("    type C{item} = int\n"),
        };
        source.push_str(&line);
    }
    source.push_str("}\n");
    if constant {
        let head = if forward { 0 } else { links };
        source.push_str(&format!(
            "fn probe() -> int {{\n    Node::C{head}\n}}\nprobe()\n"
        ));
    } else {
        source.push_str("fn probe() -> int {\n    return 1\n}\nprobe()\n");
    }
    source
}

#[test]
fn a_forward_projection_chain_outlives_the_recursive_search() {
    assert_eq!(
        run_ok(&projection_chain(4000, true, false, true)).as_int(),
        Some(4000),
        "a forward constant chain must fold whatever the direction of reference"
    );
    assert_eq!(
        run_ok(&projection_chain(4000, true, false, false)).as_int(),
        Some(1),
        "the associated type twin walks the same graph and must survive it too"
    );
}

#[test]
fn a_projection_chain_closed_on_its_own_head_is_rejected() {
    for constant in [true, false] {
        for forward in [true, false] {
            let message = compile_message(&projection_chain(3, forward, true, constant));
            assert_associated_diagnostic(
                &message,
                "E0423",
                "projection 'Node::C0' forms a cycle",
                "an associated item definition in the impl for 'Node'",
                if constant {
                    "give the associated constant a concrete definition to break the cycle"
                } else {
                    "give the associated type a concrete definition to break the cycle"
                },
            );
        }
    }
}

// in either order, so the accepted half and the rejected half share a program.
fn supertrait_declaration_program(
    distance: Option<usize>,
    reversed: bool,
    concrete: bool,
    rival: bool,
) -> String {
    let at = |level: usize| {
        if distance == Some(level) {
            "    type Item\n"
        } else {
            ""
        }
    };
    let defined_at = |level: usize| {
        if distance == Some(level) {
            "    type Item = int\n"
        } else {
            ""
        }
    };
    let mut traits = [
        format!("trait Root {{\n{}}}\n", at(2)),
        format!("trait Middle: Root {{\n{}}}\n", at(1)),
        format!(
            "trait Leaf: Middle {{\n{}    fn get(self) -> Self::Item\n}}\n",
            at(0)
        ),
    ];
    if reversed {
        traits.reverse();
    }
    let returned = if concrete { "int" } else { "Self::Item" };
    let (rival_trait, rival_impl) = if rival {
        (
            "trait Rival {\n    type Item\n}\n",
            "impl Rival for Node {\n    type Item = string\n}\n",
        )
    } else {
        ("", "")
    };
    format!(
        "{}{rival_trait}struct Node {{ value: int }}\nimpl Root for Node {{\n{}}}\nimpl Middle for Node {{\n{}}}\n{rival_impl}impl Leaf for Node {{\n{}    fn get(self) -> {returned} {{\n        return self.value\n    }}\n}}\nfn probe() -> int {{\n    let node = Node {{ value: 4 }}\n    return node.get()\n}}\nprobe()\n",
        traits.concat(),
        defined_at(2),
        defined_at(1),
        defined_at(0),
    )
}

#[test]
fn a_supertrait_item_is_visible_to_the_trait_declaration_that_inherits_it() {
    for reversed in [false, true] {
        for distance in 0..=2 {
            for concrete in [false, true] {
                assert_eq!(
                    run_ok(&supertrait_declaration_program(
                        Some(distance),
                        reversed,
                        concrete,
                        false
                    ))
                    .as_int(),
                    Some(4),
                    "'Item' declared {distance} supertraits up must resolve in a signature and run"
                );
            }
            assert_eq!(
                run_ok(&supertrait_declaration_program(
                    Some(distance),
                    reversed,
                    true,
                    true
                ))
                .as_int(),
                Some(4),
                "a trait outside the chain must not reach the inherited projection"
            );
        }
        let competing = compile_message(&supertrait_declaration_program(
            Some(2),
            reversed,
            false,
            true,
        ));
        assert_associated_diagnostic(
            &competing,
            "E0423",
            "projection 'Node::Item' is ambiguous",
            "a return type",
            "remove one of the competing impls",
        );
        assert!(
            competing.contains("Rival and Root both define 'Item' for Node"),
            "the two competing traits and the impl target must all be named: {competing}"
        );
        let message = compile_message(&supertrait_declaration_program(None, reversed, true, false));
        assert_associated_diagnostic(
            &message,
            "E0423",
            "projection 'Self::Item'",
            "a return type",
            "add a bound on 'Self' whose trait declares 'Item'",
        );
        assert!(
            message.contains("no bound in scope declares 'Item'"),
            "an item nothing in the chain declares stays rejected: {message}"
        );
    }
}

// never spelled in the impl, so nothing but the inherited declaration can
fn competing_supertrait_signature_program(competing: bool) -> String {
    let beta_item = if competing { "    type Item\n" } else { "" };
    let beta_impl = if competing {
        "    type Item = bool\n"
    } else {
        ""
    };
    format!(
        r#"trait Alpha {{
    type Item
}}
trait Beta {{
{beta_item}}}
trait Derived: Alpha + Beta {{
    fn get(self) -> Self::Item
}}
struct Node {{ value: int }}
impl Alpha for Node {{
    type Item = int
}}
impl Beta for Node {{
{beta_impl}}}
impl Derived for Node {{
    fn get(self) -> int {{
        return self.value
    }}
}}
fn probe() -> int {{
    let node = Node {{ value: 3 }}
    return node.get()
}}
probe()
"#
    )
}

#[test]
fn a_concrete_signature_names_the_projection_that_could_not_be_resolved() {
    assert_eq!(
        run_ok(&competing_supertrait_signature_program(false)).as_int(),
        Some(3),
        "one declaring supertrait must resolve the projection the impl writes out"
    );
    let message = compile_message(&competing_supertrait_signature_program(true));
    assert_associated_diagnostic(
        &message,
        "E0423",
        "projection 'Node::Item' is ambiguous",
        "impl of trait 'Derived' for 'Node'",
        "remove one of the competing impls",
    );
    assert!(
        message.contains("Alpha and Beta"),
        "the diagnostic must name the two traits that compete: {message}"
    );
    assert!(
        !message.contains("E0336"),
        "a signature mismatch names neither the projection nor the ambiguity: {message}"
    );
}

// signature position a constant has. both spellings are rejected with the same
fn inherited_length_program(in_supertrait: bool) -> String {
    let (root, leaf) = if in_supertrait {
        ("    const LIMIT: int\n", "")
    } else {
        ("", "    const LIMIT: int\n")
    };
    let (root_impl, leaf_impl) = if in_supertrait {
        ("    const LIMIT: int = 2\n", "")
    } else {
        ("", "    const LIMIT: int = 2\n")
    };
    format!(
        r#"trait Root {{
{root}}}
trait Leaf: Root {{
{leaf}    fn get(self) -> [int; Self::LIMIT]
}}
struct Node {{ value: int }}
impl Root for Node {{
{root_impl}}}
impl Leaf for Node {{
{leaf_impl}    fn get(self) -> [int; 2] {{
        return [4, 5]
    }}
}}
fn probe() -> int {{
    let node = Node {{ value: 1 }}
    let a = node.get()
    return a[0]
}}
probe()
"#
    )
}

#[test]
fn an_inherited_constant_is_no_more_a_signature_length_than_a_local_one() {
    let inherited = compile_message(&inherited_length_program(true));
    let local = compile_message(&inherited_length_program(false));
    for message in [&inherited, &local] {
        assert_associated_diagnostic(
            message,
            "E0423",
            "array length 'Self::LIMIT'",
            "an array length",
            "name the constant on a concrete type",
        );
    }
    assert_eq!(
        inherited, local,
        "the rejection must not depend on which trait declares the constant"
    );
}

fn two_supertrait_program(competing: bool) -> String {
    let beta_item = if competing { "    type Item\n" } else { "" };
    let beta_impl = if competing {
        "    type Item = bool\n"
    } else {
        ""
    };
    format!(
        r#"trait Alpha {{
    type Item
}}
trait Beta {{
{beta_item}}}
trait Derived: Alpha + Beta {{
    fn get(self) -> Self::Item
}}
struct Node {{ value: int }}
impl Alpha for Node {{
    type Item = int
}}
impl Beta for Node {{
{beta_impl}}}
impl Derived for Node {{
    fn get(self) -> Self::Item {{
        return 3
    }}
}}
fn probe() -> int {{
    let node = Node {{ value: 1 }}
    return node.get()
}}
probe()
"#
    )
}

#[test]
fn two_supertraits_declaring_one_item_name_do_not_silently_pick_one() {
    assert_eq!(
        run_ok(&two_supertrait_program(false)).as_int(),
        Some(3),
        "one declaring supertrait must resolve the projection"
    );
    // parameter does; the choice is made, and refused, where an impl supplies it.
    let message = compile_message(&two_supertrait_program(true));
    assert_associated_diagnostic(
        &message,
        "E0423",
        "projection 'Node::Item' is ambiguous",
        "a return type",
        "remove one of the competing impls",
    );
    assert!(
        message.contains("Alpha and Beta both define 'Item' for Node"),
        "both declaring supertraits and the impl target must be named: {message}"
    );
}

// that uses it. the amendment leaves this position rejected either way, so the
fn array_length_from_self_program(inherited: bool) -> String {
    if inherited {
        r#"trait Base {
    const LIMIT: int
}
trait Derived: Base {
    fn buffer(self) -> [int; Self::LIMIT]
}
fn probe() -> int { return 1 }
probe()
"#
        .to_string()
    } else {
        r#"trait Derived {
    const LIMIT: int
    fn buffer(self) -> [int; Self::LIMIT]
}
fn probe() -> int { return 1 }
probe()
"#
        .to_string()
    }
}

#[test]
fn an_array_length_taken_from_self_is_rejected_inherited_or_not() {
    for inherited in [true, false] {
        let message = compile_message(&array_length_from_self_program(inherited));
        assert_associated_diagnostic(
            &message,
            "E0423",
            "array length 'Self::LIMIT' depends on the type parameter 'Self'",
            "an array length",
            "write the length as a literal, name the constant on a concrete type, or use a growable array",
        );
    }
}

// mangled symbol both forms once collided in is reached from the trait form.
fn unconstrained_impl_parameter_program(on_trait: bool, mention_the_parameter: bool) -> String {
    let target = if mention_the_parameter {
        "Wrap<T>"
    } else {
        "Wrap<int>"
    };
    let header = if on_trait {
        format!("impl<T> Source for {target}")
    } else {
        format!("impl<T> {target}")
    };
    let declaration = if on_trait {
        "trait Source {\n    fn mark(self) -> int\n}\n"
    } else {
        ""
    };
    format!(
        r#"struct Wrap<T> {{ v: T }}
{declaration}{header} {{
    fn mark(self) -> int {{ return 1 }}
}}
fn probe() -> int {{
    let w = Wrap {{ v: 3 }}
    return w.mark()
}}
probe()
"#
    )
}

#[test]
fn an_unconstrained_type_parameter_is_rejected_on_a_trait_impl_as_well() {
    for on_trait in [true, false] {
        assert_eq!(
            run_ok(&unconstrained_impl_parameter_program(on_trait, true)).as_int(),
            Some(1),
            "the same impl with the parameter in its target must keep compiling"
        );
        let message = compile_message(&unconstrained_impl_parameter_program(on_trait, false));
        let reason = if on_trait {
            "impl of trait 'Source' for 'Wrap<i64>'"
        } else {
            "inherent impl of 'Wrap<i64>'"
        };
        assert_associated_diagnostic(&message, "E0430", "'T'", reason, "move it onto the method");
    }
}

// target type alone, that refuses; measured with the refusal disabled, both
fn impl_parameter_placement(placement: &str) -> String {
    let (header, method, call) = match placement {
        "target" => (
            "impl<T> Take<Box2<T>> for Wrap<T>",
            "    fn take(x: Box2<T>, k: int) -> int {\n        return k + 100\n    }\n",
            "Wrap::take(Box2 { v: 2 }, 2)",
        ),
        "trait argument" => (
            "impl<T> Take<Box2<T>> for Wrap<int>",
            "    fn take(x: Box2<T>, k: int) -> int {\n        return k + 100\n    }\n",
            "Wrap::take(Box2 { v: 2 }, 2)",
        ),
        "method signature" => (
            "impl<T> Wrap<int>",
            "    fn take(x: T, k: int) -> int {\n        return k + 100\n    }\n",
            "Wrap::take(2, 2)",
        ),
        "where clause" => (
            "impl<T> Wrap<int> where T: Tag",
            "    fn take(x: int, k: int) -> int {\n        return k + 100\n    }\n",
            "Wrap::take(2, 2)",
        ),
        other => unreachable!("no placement {other}"),
    };
    format!(
        "struct Wrap<T> {{ v: T }}\nstruct Box2<T> {{ v: T }}\n\
         trait Tag {{\n    fn tag(self) -> int\n}}\n\
         trait Take<A> {{\n    fn take(x: A, k: int) -> int\n}}\n\
         {header} {{\n{method}}}\n\
         fn probe() -> int {{\n    return {call}\n}}\nprobe()\n"
    )
}

#[test]
fn e0430_states_the_reason_that_holds_where_the_parameter_stands() {
    assert_eq!(
        run_ok(&impl_parameter_placement("target")).as_int(),
        Some(102),
        "the parameter written into the target type must bind at the call site and run"
    );
    for placement in ["trait argument", "method signature"] {
        let message = compile_message(&impl_parameter_placement(placement));
        let reason = match placement {
            "trait argument" => "impl of trait 'Take' for 'Wrap<i64>'",
            _ => "inherent impl of 'Wrap<i64>'",
        };
        assert_associated_diagnostic(
            &message,
            "E0430",
            "does not appear in the impl target type 'Wrap<i64>', though a call site determines it",
            reason,
            "move it onto the method",
        );
        assert!(
            message.contains("named by its target type alone"),
            "the {placement} shape must state the symbol that refuses it: {message}"
        );
        assert!(
            !message.contains("no call site can determine it"),
            "a call site does determine 'T' in the {placement} shape: {message}"
        );
    }
    let confined = compile_message(&impl_parameter_placement("where clause"));
    assert_associated_diagnostic(
        &confined,
        "E0430",
        "does not appear in the impl target type 'Wrap<i64>', so no call site can determine it",
        "inherent impl of 'Wrap<i64>'",
        "move it onto the method",
    );
    assert!(
        !confined.contains("a call site determines it"),
        "nothing binds a parameter confined to a where clause: {confined}"
    );
}

// which the monomorphizer binds once per instance, or to the method itself,
fn parameter_owner_program(on_the_method: bool, constant: bool, called: bool) -> String {
    let prelude = "struct Counter { v: int }\ntrait Base { type Item; const LIMIT: int; }\nimpl Base for Counter { type Item = int; const LIMIT: int = 3; }\n";
    let (signature, body) = if constant {
        if on_the_method {
            ("fn probe<T: Base>(self, x: T) -> int", "return T::LIMIT")
        } else {
            ("fn probe(self) -> int", "return T::LIMIT")
        }
    } else if on_the_method {
        (
            "fn probe<T: Base>(self, x: T, seed: T::Item) -> T::Item",
            "return seed",
        )
    } else {
        ("fn probe(self, seed: T::Item) -> T::Item", "return seed")
    };
    let (receiver, block) = if on_the_method {
        ("struct Host { n: int }\n", "impl Host".to_string())
    } else {
        (
            "struct Wrap<T> { inner: T }\n",
            "impl<T: Base> Wrap<T>".to_string(),
        )
    };
    let body_of_main = if !called {
        "    return 0".to_string()
    } else if on_the_method {
        let call = if constant {
            "h.probe(Counter { v: 1 })"
        } else {
            "h.probe(Counter { v: 1 }, 9)"
        };
        format!("    let h = Host {{ n: 0 }}\n    return {call}")
    } else {
        let call = if constant { "w.probe()" } else { "w.probe(9)" };
        format!("    let w = Wrap {{ inner: Counter {{ v: 1 }} }}\n    return {call}")
    };
    format!(
        "{prelude}{receiver}{block} {{\n    {signature} {{\n        {body}\n    }}\n}}\nfn main() -> int {{\n{body_of_main}\n}}\nmain()\n"
    )
}

#[test]
fn an_associated_constant_resolves_through_an_impl_parameter() {
    assert_eq!(
        run_ok(&parameter_owner_program(false, true, true)).as_int(),
        Some(3),
        "the constant twin of the associated type must reach its value"
    );
    assert_eq!(
        run_ok(&parameter_owner_program(false, false, true)).as_int(),
        Some(9)
    );
    assert_eq!(
        run_ok(&parameter_owner_program(false, true, false)).as_int(),
        Some(0)
    );
    assert_eq!(
        run_ok(&parameter_owner_program(false, false, false)).as_int(),
        Some(0)
    );
}

#[test]
fn an_associated_constant_through_a_method_parameter_is_refused_by_name() {
    for called in [true, false] {
        let message = compile_message(&parameter_owner_program(true, true, called));
        assert_associated_diagnostic(
            &message,
            "E0423",
            "'T::LIMIT'",
            "a value expression",
            "free generic function",
        );
    }
    assert_eq!(
        run_ok(&parameter_owner_program(true, false, true)).as_int(),
        Some(9),
        "the associated type through the same parameter is erased and still runs"
    );
}

#[test]
fn an_associated_constant_through_an_impl_parameter_reads_each_instance() {
    let accepted = run_ok(
        "struct Counter { v: int }\nstruct Other { w: int }\ntrait Base { const LIMIT: int; }\nimpl Base for Counter { const LIMIT: int = 3; }\nimpl Base for Other { const LIMIT: int = 5; }\nstruct Wrap<T> { inner: T }\nimpl<T: Base> Wrap<T> {\n    fn limit(self) -> int {\n        return T::LIMIT\n    }\n}\nfn main() -> int {\n    let a = Wrap { inner: Counter { v: 1 } }\n    let b = Wrap { inner: Other { w: 2 } }\n    return a.limit() * 10 + b.limit()\n}\nmain()\n",
    );
    assert_eq!(accepted.as_int(), Some(35));
}

fn inherited_associated_const_program(declared: &str, value: &str) -> String {
    format!(
        "struct Counter {{ v: int }}\ntrait Base {{ const LIMIT: int; }}\ntrait Derived: Base {{ fn f(self) -> int; }}\nimpl Base for Counter {{ const LIMIT: {declared} = {value}; }}\nimpl Derived for Counter {{ fn f(self) -> int {{ return 1 }} }}\nfn main() -> int {{\n    let c = Counter {{ v: 1 }}\n    return Counter::LIMIT + c.f()\n}}\nmain()\n"
    )
}

#[test]
fn a_supertrait_declared_constant_is_checked_against_its_declaration() {
    assert_eq!(
        run_ok(&inherited_associated_const_program("int", "3")).as_int(),
        Some(4),
        "a constant matching its declaration must resolve and run"
    );
    let declared_mismatch = compile_message(&inherited_associated_const_program("string", "\"x\""));
    assert!(
        declared_mismatch.contains("trait 'Base'"),
        "the diagnostic must name the implemented trait: {declared_mismatch}"
    );
    assert_associated_diagnostic(
        &declared_mismatch,
        "E0422",
        "'LIMIT'",
        "declared type in the impl for 'Counter'",
        "declare the same type as the trait",
    );
    let value_mismatch = compile_message(&inherited_associated_const_program("int", "\"x\""));
    assert_associated_diagnostic(
        &value_mismatch,
        "E0422",
        "'LIMIT'",
        "constant value in the impl for 'Counter'",
        "write an initialiser of type 'i64'",
    );
}

const ASSOCIATED_ITEM_PRELUDE: &str = concat!(
    "struct Counter { v: int }\n",
    "struct Wrap<T> { w: T }\n",
    "trait Source { type Item; const LIMIT: int; }\n",
    "impl Source for Counter { type Item = int; const LIMIT: int = 4; }\n",
    "impl Source for Wrap<int> { type Item = int; const LIMIT: int = 4; }\n",
    "trait Extra { fn extra(self) -> int; }\n",
);

// in the program moves, so the rejected half stops being rejected as soon as
fn associated_item_source(context: &str, receiver: &str, item: &str, position: &str) -> String {
    let projection = format!("{receiver}::{item}");
    let body = match position {
        "value" => format!("return {projection}"),
        "type" => format!("let z: {projection} = 1\n return z"),
        _ => unreachable!("no position {position}"),
    };
    let line = match context {
        "inherent impl" => {
            format!(
                "impl Counter {{ fn probe(self) -> int {{ {body} }} }}\nCounter {{ v: 1 }}.probe()"
            )
        }
        "trait impl" => format!(
            "impl Extra for Counter {{ fn extra(self) -> int {{ {body} }} }}\nCounter {{ v: 1 }}.extra()"
        ),
        "generic impl" => {
            format!(
                "impl Wrap<int> {{ fn probe(self) -> int {{ {body} }} }}\nWrap {{ w: 1 }}.probe()"
            )
        }
        "trait default body" => format!(
            "trait Plain {{ fn plain(self) -> int {{ {body} }} }}\nimpl Plain for Counter {{ }}\nCounter {{ v: 1 }}.plain()"
        ),
        "free function" => format!("fn probe() -> int {{ {body} }}\nprobe()"),
        "type parameter" => {
            format!("fn probe<T: Source>(s: T) -> int {{ {body} }}\nprobe(Counter {{ v: 1 }})")
        }
        _ => unreachable!("no context {context}"),
    };
    format!("{ASSOCIATED_ITEM_PRELUDE}{line}\n")
}

const ABSENT_ITEM_CELLS: [(&str, &str); 7] = [
    ("inherent impl", "Self"),
    ("inherent impl", "Counter"),
    ("trait impl", "Self"),
    ("generic impl", "Self"),
    ("trait default body", "Self"),
    ("free function", "Counter"),
    ("type parameter", "T"),
];

fn rendered_receiver(context: &str, receiver: &'static str) -> &'static str {
    match (context, receiver) {
        ("inherent impl" | "trait impl", "Self") => "Counter",
        ("generic impl", "Self") => "Wrap",
        _ => receiver,
    }
}

fn absent_item_clauses(receiver: &str) -> (String, String) {
    if receiver == "T" {
        (
            "no bound in scope declares 'NOPE'".to_string(),
            "add a bound on 'T' whose trait declares 'NOPE'".to_string(),
        )
    } else {
        (
            format!("no impl for '{receiver}' defines 'NOPE'"),
            "define it in an impl of a trait that declares 'NOPE'".to_string(),
        )
    }
}

#[test]
fn an_associated_item_absent_in_value_position_is_a_projection_not_an_undefined_function() {
    for (context, receiver) in ABSENT_ITEM_CELLS {
        assert_eq!(
            run_ok(&associated_item_source(context, receiver, "LIMIT", "value")).as_int(),
            Some(4),
            "the accepted half of the {context} cell must keep reading '{receiver}::LIMIT'"
        );
        let message = compile_message(&associated_item_source(context, receiver, "NOPE", "value"));
        let named = rendered_receiver(context, receiver);
        let (explanation, help) = absent_item_clauses(named);
        assert_associated_diagnostic(
            &message,
            "E0423",
            &format!("projection '{named}::NOPE'"),
            "a value expression",
            &help,
        );
        assert!(
            message.contains(&explanation),
            "the {context} cell must state why '{receiver}::NOPE' does not resolve: {message}"
        );
        assert!(
            !message.contains("undefined function"),
            "a value read of an absent associated item is not a call: {message}"
        );
    }
}

fn diagnostic_clause(message: &str) -> String {
    let line = message.lines().next().unwrap_or_default();
    match line.rsplit_once(" (") {
        Some((clause, _)) => clause.to_string(),
        None => line.to_string(),
    }
}

const TWIN_POSITION_CELLS: [(&str, &str); 5] = [
    ("inherent impl", "Self"),
    ("inherent impl", "Counter"),
    ("trait impl", "Self"),
    ("trait default body", "Self"),
    ("free function", "Counter"),
];

#[test]
fn an_absent_associated_item_reads_the_same_in_the_value_and_the_type_position() {
    for (context, receiver) in TWIN_POSITION_CELLS {
        assert_eq!(
            run_ok(&associated_item_source(context, receiver, "Item", "type")).as_int(),
            Some(1),
            "the accepted half of the {context} cell must keep naming '{receiver}::Item'"
        );
        let read = compile_message(&associated_item_source(context, receiver, "NOPE", "value"));
        let named = compile_message(&associated_item_source(context, receiver, "NOPE", "type"));
        assert_eq!(
            diagnostic_clause(&read),
            diagnostic_clause(&named),
            "the {context} cell must give '{receiver}::NOPE' one diagnostic in both positions"
        );
        assert!(
            read.contains("(a value expression)") && named.contains("(a type annotation)"),
            "each position must still state its own occurrence:\n{read}\n{named}"
        );
    }
}

fn absent_item_in_a_trait_default_body(first: &str, second: &str, item: &str) -> String {
    let limit = |name: &str| if name == "A" { 4 } else { 7 };
    format!(
        "struct A {{ v: int }}\nstruct B {{ v: int }}\n\
         trait Plain {{\n    const LIMIT: int\n    fn plain(self) -> int {{\n        return Self::{item}\n    }}\n}}\n\
         impl Plain for {first} {{\n    const LIMIT: int = {}\n}}\n\
         impl Plain for {second} {{\n    const LIMIT: int = {}\n}}\n\
         fn probe() -> int {{\n    return A {{ v: 0 }}.plain() + B {{ v: 0 }}.plain()\n}}\nprobe()\n",
        limit(first),
        limit(second)
    )
}

fn self_projection_program(
    context: &str,
    arm: &str,
    position: &str,
    written: &str,
    faulty: bool,
) -> String {
    let (mut const_item, mut type_item, mut limit) = ("LIMIT", "Item", "4");
    if faulty {
        match arm {
            "absent" => {
                const_item = "NOPE";
                type_item = "NOPE";
            }
            "namespace" => {
                const_item = "Item";
                type_item = "LIMIT";
            }
            "unevaluable" => limit = "1 / 0",
            "out of range" => limit = "0 - 3",
            "not constant" => limit = "2 << 70",
            _ => {}
        }
    }
    let body = match position {
        "value" => format!("        return {written}::{const_item}\n"),
        "type" => format!("        let z: {written}::{type_item} = 4\n        return z\n"),
        _ => format!(
            "        let a: [int; {written}::{const_item}] = [0, 0, 0, 4]\n        return a[3]\n"
        ),
    };
    let mut source = format!(
        "struct Counter {{ v: int }}\nstruct Wrap<T> {{ w: T }}\n\
         trait Extra {{\n    fn extra(self) -> int\n}}\n\
         trait Source {{\n    type Item\n    const LIMIT: int\n}}\n\
         impl Source for Counter {{\n    type Item = int\n    const LIMIT: int = {limit}\n}}\n\
         impl Source for Wrap<int> {{\n    type Item = int\n    const LIMIT: int = {limit}\n}}\n"
    );
    if arm == "ambiguous" && faulty {
        source += "trait Rival {\n    type Item\n    const LIMIT: int\n}\n\
                   impl Rival for Counter {\n    type Item = int\n    const LIMIT: int = 4\n}\n";
    }
    let call = match context {
        "inherent impl" => {
            source += &format!("impl Counter {{\n    fn probe(self) -> int {{\n{body}    }}\n}}\n");
            "Counter { v: 1 }.probe()"
        }
        "trait impl" => {
            source += &format!(
                "impl Extra for Counter {{\n    fn extra(self) -> int {{\n{body}    }}\n}}\n"
            );
            "Counter { v: 1 }.extra()"
        }
        _ => {
            source +=
                &format!("impl Wrap<int> {{\n    fn probe(self) -> int {{\n{body}    }}\n}}\n");
            "Wrap { w: 1 }.probe()"
        }
    };
    source + &format!("fn go() -> int {{\n    return {call}\n}}\ngo()\n")
}

// the clause that tells one cause from another, so a cell cannot drift onto a
fn projection_cause_clause(arm: &str) -> &'static str {
    match arm {
        "absent" => "no impl for",
        "namespace" => "is an associated",
        "ambiguous" => "is ambiguous",
        "unevaluable" => "its value cannot be computed",
        "out of range" => "cannot be an array length",
        _ => "is not a constant integer expression",
    }
}

fn self_projection_cells() -> Vec<(&'static str, &'static str, &'static str)> {
    let mut cells = Vec::new();
    for context in ["inherent impl", "trait impl"] {
        for arm in ["absent", "namespace", "ambiguous"] {
            for position in ["value", "type", "length"] {
                cells.push((context, arm, position));
            }
        }
    }
    for position in ["value", "length"] {
        cells.push(("generic impl", "absent", position));
    }
    for context in ["inherent impl", "trait impl", "generic impl"] {
        for arm in ["unevaluable", "not constant"] {
            for position in ["value", "length"] {
                cells.push((context, arm, position));
            }
        }
        cells.push((context, "out of range", "length"));
    }
    cells
}

#[test]
fn an_absent_item_on_self_names_the_impl_target() {
    for (context, arm, position) in self_projection_cells() {
        let target = match context {
            "generic impl" => "Wrap",
            _ => "Counter",
        };
        for written in ["Self", target] {
            assert_eq!(
                run_ok(&self_projection_program(
                    context, arm, position, written, false
                ))
                .as_int(),
                Some(4),
                "the accepted half of the {context} {arm} cell in {position} position must run \
                 through '{written}'"
            );
        }
        let through_self = compile_message(&self_projection_program(
            context, arm, position, "Self", true,
        ));
        assert!(
            through_self.contains(projection_cause_clause(arm)),
            "the {context} {arm} cell in {position} position must reach its own cause: \
             {through_self}"
        );
        assert!(
            through_self.contains(&format!("'{target}::")),
            "the {context} {arm} cell in {position} position must name the impl target: \
             {through_self}"
        );
        assert!(
            !through_self.contains("'Self"),
            "the compiler knows what 'Self' is here and must not print it: {through_self}"
        );
        let through_target = compile_message(&self_projection_program(
            context, arm, position, target, true,
        ));
        // the sentence and the occurrence it states must not.
        assert_eq!(
            through_self.lines().next(),
            through_target.lines().next(),
            "'Self' and '{target}' must render alike in the {context} {arm} cell at {position}"
        );
    }
}

#[test]
fn a_trait_default_body_keeps_the_self_it_wrote() {
    for (first, second) in [("A", "B"), ("B", "A")] {
        assert_eq!(
            run_ok(&absent_item_in_a_trait_default_body(first, second, "LIMIT")).as_int(),
            Some(11),
            "the default body must read the constant of each impl it runs for"
        );
    }
    let ordered = compile_message(&absent_item_in_a_trait_default_body("A", "B", "NOPE"));
    let swapped = compile_message(&absent_item_in_a_trait_default_body("B", "A", "NOPE"));
    assert_associated_diagnostic(
        &ordered,
        "E0423",
        "projection 'Self::NOPE'",
        "a value expression",
        "define it in an impl of a trait that declares 'NOPE'",
    );
    assert_eq!(
        ordered, swapped,
        "a default body serves every impl, so neither may lend it a name:\n{ordered}\n----\n{swapped}"
    );
}

fn static_path_source(item: &str, called: bool) -> String {
    let path = format!("Counter::{item}");
    let expr = if called { format!("{path}(5)") } else { path };
    format!(
        "struct Counter {{ v: int }}\ntrait Source {{ const LIMIT: int; }}\nimpl Source for Counter {{ const LIMIT: int = 4; }}\nimpl Counter {{ fn make(x: int) -> int {{ return x }} }}\nfn probe() -> int {{ return {expr} }}\nprobe()\n"
    )
}

#[test]
fn a_call_to_a_missing_static_method_stays_an_undefined_function() {
    assert_eq!(
        run_ok(&static_path_source("make", true)).as_int(),
        Some(5),
        "the accepted half must keep calling a defined static method"
    );
    assert_eq!(
        run_ok(&static_path_source("LIMIT", false)).as_int(),
        Some(4),
        "the accepted half must keep reading a defined associated constant"
    );
    let called = compile_message(&static_path_source("nope", true));
    assert_eq!(
        called.lines().next().unwrap_or_default(),
        "error[E0362]: undefined function: Counter::nope"
    );
    let read = compile_message(&static_path_source("nope", false));
    assert_associated_diagnostic(
        &read,
        "E0423",
        "projection 'Counter::nope'",
        "a value expression",
        "define it in an impl of a trait that declares 'nope'",
    );
}

fn associated_const_value_source(value: &str, through_a_bound: bool) -> String {
    let prelude = format!(
        "struct Counter {{ v: int }}\ntrait Source {{ const LIMIT: int; }}\nimpl Source for Counter {{ const LIMIT: int = {value}; }}\n"
    );
    if through_a_bound {
        format!(
            "{prelude}fn probe<T: Source>(s: T) -> int {{ return T::LIMIT }}\nprobe(Counter {{ v: 1 }})\n"
        )
    } else {
        format!("{prelude}fn probe() -> int {{ return Counter::LIMIT }}\nprobe()\n")
    }
}

#[test]
fn a_constant_that_does_not_evaluate_is_not_reported_absent() {
    for through_a_bound in [false, true] {
        assert_eq!(
            run_ok(&associated_const_value_source("4", through_a_bound)).as_int(),
            Some(4),
            "the accepted half must keep reading a constant that evaluates"
        );
        let reason = if through_a_bound {
            "associated constant of trait 'Source'"
        } else {
            "a value expression"
        };
        let uncomputable =
            compile_message(&associated_const_value_source("1 / 0", through_a_bound));
        assert_associated_diagnostic(
            &uncomputable,
            "E0423",
            "constant 'Counter::LIMIT'",
            reason,
            "give it a value that evaluates",
        );
        assert!(
            uncomputable.contains(
                "is defined but its value cannot be computed: the arithmetic overflows or divides by zero"
            ),
            "the diagnostic must state why the value is missing: {uncomputable}"
        );
        let not_constant =
            compile_message(&associated_const_value_source("1 < 2", through_a_bound));
        assert_associated_diagnostic(
            &not_constant,
            "E0423",
            "constant 'Counter::LIMIT'",
            reason,
            "use integer literals and '+ - * / %' only",
        );
        assert!(
            not_constant.contains("is defined but its value is not a constant integer expression"),
            "the diagnostic must state why the value is missing: {not_constant}"
        );
        for message in [&uncomputable, &not_constant] {
            assert!(
                !message.contains("no impl for 'Counter' defines 'LIMIT'"),
                "the constant is defined, so no diagnostic may call it absent: {message}"
            );
        }
    }
}

// back to a dummy span and still surfaces in the rendered message, with the
fn header_diagnostic_program(shape: &str, with_method: bool) -> String {
    let declared = if with_method {
        "    fn f(self) -> int\n"
    } else {
        ""
    };
    let defined = if with_method {
        "    fn f(self) -> int {\n        return 1\n    }\n"
    } else {
        ""
    };
    let tail = "fn probe() -> int {\n    return 1\n}\nprobe()\n";
    match shape {
        "duplicate_trait" => format!(
            "trait S1 {{\n    type Item\n}}\ntrait S1 {{\n    type Other\n{declared}}}\n{tail}"
        ),
        "unknown_trait" => format!(
            "struct C {{ v: int }}\nimpl Nope for C {{\n    type Item = int\n{defined}}}\n{tail}"
        ),
        "reserved_from" => {
            format!("struct C {{ v: int }}\nimpl From<C> for C {{\n{defined}}}\n{tail}")
        }
        "duplicate_impl" => format!(
            "trait S1 {{\n    type Item\n{declared}}}\nstruct C {{ v: int }}\n\
             impl S1 for C {{\n    type Item = int\n{defined}}}\n\
             impl S1 for C {{\n    type Item = int\n{defined}}}\n{tail}"
        ),
        "overlapping_impl" => format!(
            "trait S1 {{\n    type Item\n{declared}}}\nstruct C<T> {{ v: T }}\n\
             impl<T> S1 for C<T> {{\n    type Item = int\n{defined}}}\n\
             impl<U> S1 for C<U> {{\n    type Item = int\n{defined}}}\n{tail}"
        ),
        _ => format!(
            "trait S1 {{\n    type Item\n    fn f(self) -> int\n    fn g(self) -> int\n}}\n\
             struct C {{ v: int }}\n\
             impl S1 for C {{\n    type Item = int\n{defined}}}\n{tail}"
        ),
    }
}

#[test]
fn an_impl_that_defines_only_associated_items_still_locates_its_header_diagnostic() {
    for (shape, code) in [
        ("duplicate_trait", "E0326"),
        ("unknown_trait", "E0332"),
        ("reserved_from", "E0375"),
        ("duplicate_impl", "E0334"),
        ("overlapping_impl", "E0340"),
        ("missing_method", "E0333"),
    ] {
        for with_method in [false, true] {
            let message = compile_message(&header_diagnostic_program(shape, with_method));
            assert!(
                message.contains(code),
                "'{shape}' with_method={with_method} must be rejected with {code}: {message}"
            );
            assert_located(code, &message);
        }
    }
}

fn type_errors(source: &str) -> Vec<aelys_sema::TypeError> {
    let source = aelys_syntax::Source::new("sema_constraint_tests.aelys", source);
    let tokens = aelys_frontend::lexer::Lexer::with_source(source.clone())
        .scan()
        .expect("the source must lex");
    let stmts = aelys_frontend::parser::Parser::new_rust_collections(tokens, source.clone())
        .parse()
        .expect("the source must parse");
    aelys_sema::TypeInference::infer_program_full_with_native_signatures(
        stmts,
        source,
        std::collections::HashSet::new(),
        std::collections::HashSet::new(),
        std::collections::HashSet::new(),
        std::collections::HashMap::new(),
        aelys_sema::infer::imports::ImportedTypes::default(),
    )
    .err()
    .expect("the source must be rejected")
}

fn absent_target_program(trait_impl: bool, with_method: bool) -> String {
    let defined = if with_method {
        "    fn f(self) -> int {\n        return 1\n    }\n"
    } else {
        ""
    };
    let tail = "fn probe() -> int {\n    return 1\n}\nprobe()\n";
    if trait_impl {
        format!(
            "trait S1 {{\n    type Item\n}}\nimpl S1 for Nope {{\n    type Item = int\n{defined}}}\n{tail}"
        )
    } else {
        format!("impl Nope {{\n{defined}}}\n{tail}")
    }
}

#[test]
fn an_impl_header_on_an_absent_type_locates_its_own_diagnostic() {
    for (trait_impl, code) in [(true, 339u16), (false, 328u16)] {
        for with_method in [false, true] {
            let errors = type_errors(&absent_target_program(trait_impl, with_method));
            let located = errors
                .iter()
                .find(|error| error.diagnostic_code() == code)
                .unwrap_or_else(|| {
                    panic!(
                        "E0{code} must be emitted with_method={with_method}: {:?}",
                        errors
                            .iter()
                            .map(|e| e.diagnostic_code())
                            .collect::<Vec<_>>()
                    )
                });
            assert!(
                located.span.line > 0,
                "E0{code} must carry a real source line with_method={with_method}"
            );
        }
    }
}

#[test]
fn a_cyclic_associated_item_names_the_namespace_it_is_written_in() {
    for forward in [false, true] {
        let constant = compile_message(&projection_chain(3, forward, true, true));
        assert_associated_diagnostic(
            &constant,
            "E0423",
            "projection 'Node::C0' forms a cycle",
            "an associated item definition in the impl for 'Node'",
            "give the associated constant a concrete definition to break the cycle",
        );
        let associated_type = compile_message(&projection_chain(3, forward, true, false));
        assert_associated_diagnostic(
            &associated_type,
            "E0423",
            "projection 'Node::C0' forms a cycle",
            "an associated item definition in the impl for 'Node'",
            "give the associated type a concrete definition to break the cycle",
        );
    }
}

#[test]
fn a_duplicate_associated_item_names_the_impl_it_stands_in() {
    for (namespace, item) in [("type", "Item"), ("const", "LIMIT")] {
        let own = compile_message(&inherited_item_program(namespace, false, false, 2));
        assert_associated_diagnostic(
            &own,
            "E0426",
            &format!(
                "implementation of trait 'Derived' defines associated item '{item}' more than once"
            ),
            "impl of trait 'Derived' for 'T'",
            "defined exactly once",
        );
    }
}

#[derive(Clone, Copy)]
enum FanPosition {
    StructField,
    EnumPayload,
    Parameter,
    ReturnType,
}

impl FanPosition {
    fn reason(self) -> &'static str {
        match self {
            Self::StructField => "a struct field",
            Self::EnumPayload => "an enum variant field",
            Self::Parameter => "a parameter type",
            Self::ReturnType => "a return type",
        }
    }
}

fn projection_fan(depth: usize, position: FanPosition) -> String {
    let mut source = String::new();
    for index in 0..=depth {
        source.push_str(&format!("trait Fan{index} {{ type Item{index} }}\n"));
    }
    source.push_str("struct Node { value: int }\n");
    for index in 0..depth {
        source.push_str(&format!(
            "impl Fan{index} for Node {{ type Item{index} = Result<Node::Item{}, Node::Item{}> }}\n",
            index + 1,
            index + 1
        ));
    }
    source.push_str(&format!(
        "impl Fan{depth} for Node {{ type Item{depth} = int }}\n"
    ));
    let value = format!("{}1{}", "Ok(".repeat(depth), ")".repeat(depth));
    let arms = |scrutinee: &str| {
        format!("match {scrutinee} {{\n        Ok(_) => 1\n        Err(_) => 0\n    }}")
    };
    match position {
        FanPosition::StructField => {
            source.push_str("struct Cell { slot: Node::Item0 }\n");
            source.push_str(&format!(
                "fn probe() -> int {{\n    let held = Cell {{ slot: {value} }}\n    {}\n}}\n",
                arms("held.slot")
            ));
        }
        FanPosition::EnumPayload => {
            source.push_str("enum Holder { Slot(Node::Item0), Empty }\n");
            source.push_str(&format!(
                "fn probe() -> int {{\n    match Holder::Slot({value}) {{\n        Holder::Slot(inner) => {}\n        Holder::Empty => 0\n    }}\n}}\n",
                arms("inner")
            ));
        }
        FanPosition::Parameter => {
            source.push_str(&format!(
                "fn take(slot: Node::Item0) -> int {{\n    {}\n}}\n",
                arms("slot")
            ));
            source.push_str(&format!("fn probe() -> int {{\n    take({value})\n}}\n"));
        }
        FanPosition::ReturnType => {
            source.push_str(&format!("fn make() -> Node::Item0 {{\n    {value}\n}}\n"));
            source.push_str(&format!(
                "fn probe() -> int {{\n    {}\n}}\n",
                arms("make()")
            ));
        }
    }
    source.push_str("probe()\n");
    source
}

#[test]
fn the_projection_budget_binds_every_annotation_position() {
    for position in [
        FanPosition::StructField,
        FanPosition::EnumPayload,
        FanPosition::Parameter,
        FanPosition::ReturnType,
    ] {
        assert_eq!(
            run_ok(&projection_fan(11, position)).as_int(),
            Some(1),
            "the accepted half of '{}' must run, not merely compile",
            position.reason()
        );
        let message = compile_message(&projection_fan(12, position));
        assert_associated_diagnostic(
            &message,
            "E0427",
            "Node::Item",
            position.reason(),
            "give one of them a concrete definition",
        );
    }
}

fn missing_associated_item(namespace: &str) -> String {
    let declaration = if namespace == "type" {
        "type Item"
    } else {
        "const LIMIT: int"
    };
    format!(
        "trait Source {{ {declaration} }}\nstruct Node {{ v: int }}\nimpl Source for Node {{}}\nfn probe() -> int {{\n    1\n}}\nprobe()\n"
    )
}

#[test]
fn the_missing_associated_item_help_names_the_declared_namespace() {
    for (namespace, item, wrong) in [
        ("type", "type Item", "const Item"),
        ("const", "const LIMIT", "type LIMIT"),
    ] {
        let message = compile_message(&missing_associated_item(namespace));
        assert_associated_diagnostic(
            &message,
            "E0421",
            "missing required associated item",
            "impl of trait 'Source' for 'Node'",
            &format!("define '{item}' in the impl body"),
        );
        assert!(
            !message.contains(wrong),
            "E0421 must not offer '{wrong}': {message}"
        );
    }
}

fn duplicate_declaration(keyword: &str) -> String {
    let body = match keyword {
        "struct" => "struct Twice { v: int }\nstruct Twice { w: int }\n",
        "enum" => "enum Twice { Red, Green }\nenum Twice { Blue, Black }\n",
        _ => "trait Twice { type Item }\ntrait Twice { type Other }\n",
    };
    format!("{body}fn probe() -> int {{\n    1\n}}\nprobe()\n")
}

#[test]
fn the_duplicate_declaration_diagnostic_names_the_keyword_it_read() {
    for keyword in ["struct", "enum", "trait"] {
        let message = compile_message(&duplicate_declaration(keyword));
        assert_associated_diagnostic(
            &message,
            "E0326",
            &format!("duplicate {keyword} declaration 'Twice'"),
            "",
            "'Twice'",
        );
    }
}

fn two_declarations_named(first: &str, second: &str, collide: bool) -> String {
    let names = if collide {
        ["Twin", "Twin"]
    } else {
        ["Alpha", "Beta"]
    };
    let mut source = String::new();
    let mut read = Vec::new();
    for (half, (keyword, value)) in [(first, 4), (second, 7)].into_iter().enumerate() {
        let name = names[half];
        if keyword == "trait" {
            source += &format!(
                "trait {name} {{\n    const LIMIT: int\n}}\n\
                 struct Carrier{half} {{ v: int }}\n\
                 impl {name} for Carrier{half} {{\n    const LIMIT: int = {value}\n}}\n"
            );
            read.push(format!("{name}::LIMIT"));
            continue;
        }
        source += &match keyword {
            "enum" => format!("enum {name} {{ Red, Green }}\n"),
            _ => format!("struct {name} {{ v: int }}\n"),
        };
        source += &format!(
            "trait Mark{half} {{\n    const MARK: int\n}}\n\
             impl Mark{half} for {name} {{\n    const MARK: int = {value}\n}}\n"
        );
        read.push(format!("{name}::MARK"));
    }
    source
        + &format!(
            "fn probe() -> int {{\n    return {}\n}}\nprobe()\n",
            read.join(" + ")
        )
}

const DECLARATION_RANK: [&str; 3] = ["enum", "struct", "trait"];

fn accused_and_shadowed(first: &str, second: &str) -> (&'static str, Option<&'static str>) {
    let rank = |keyword: &str| {
        DECLARATION_RANK
            .iter()
            .position(|declared| *declared == keyword)
            .expect("every keyword is ranked")
    };
    let (later, earlier) = if rank(first) >= rank(second) {
        (rank(first), rank(second))
    } else {
        (rank(second), rank(first))
    };
    match later == earlier {
        true => (DECLARATION_RANK[later], None),
        false => (DECLARATION_RANK[later], Some(DECLARATION_RANK[earlier])),
    }
}

#[test]
fn a_cross_kind_collision_names_both_kinds_and_offers_the_rename() {
    for first in DECLARATION_RANK {
        for second in DECLARATION_RANK {
            assert_eq!(
                run_ok(&two_declarations_named(first, second, false)).as_int(),
                Some(11),
                "a '{first}' beside a '{second}' under two names must read a constant from each"
            );
            let message = compile_message(&two_declarations_named(first, second, true));
            let (accused, shadowed) = accused_and_shadowed(first, second);
            assert_associated_diagnostic(
                &message,
                "E0326",
                &format!("duplicate {accused} declaration 'Twin'"),
                "",
                "'Twin'",
            );
            match shadowed {
                Some(other) => {
                    assert!(
                        message.contains(&format!("already taken by the {other} 'Twin'"))
                            && message.contains(&format!("rename the {accused} or the {other}")),
                        "a '{first}' colliding with a '{second}' must name both kinds and \
                         offer the rename: {message}"
                    );
                }
                None => {
                    assert!(
                        !message.contains("already taken by"),
                        "a '{first}' colliding with a second '{second}' names one kind: {message}"
                    );
                }
            }
        }
    }
    assert_renders_identically(
        &two_declarations_named("struct", "trait", true),
        "the cross-kind collision",
    );
    assert_renders_identically(
        &two_declarations_named("trait", "trait", true),
        "the same-kind collision",
    );
}

fn compile_errors(source: &str) -> Vec<aelys_sema::constraint::TypeError> {
    let src = aelys_syntax::Source::new("sema_constraint_tests.aelys", source);
    let tokens = aelys_frontend::lexer::Lexer::with_source(src.clone())
        .scan()
        .expect("the source must lex");
    let ast = aelys_frontend::parser::Parser::new(tokens, src.clone())
        .parse()
        .expect("the source must parse");
    match aelys_sema::TypeInference::infer_program_full(
        ast,
        src,
        Default::default(),
        Default::default(),
    ) {
        Ok(_) => Vec::new(),
        Err(errors) => errors,
    }
}

fn unmet_supertraits(source: &str) -> Vec<String> {
    compile_errors(source)
        .into_iter()
        .filter_map(|error| match error.kind {
            TypeErrorKind::MissingSupertraitImpl { supertrait, .. } => Some(supertrait),
            _ => None,
        })
        .collect()
}

fn supertrait_chain(links: usize, implemented_from: usize, namespace: &str) -> String {
    let (declaration, definition, returned, tail) = match namespace {
        "type" => (
            "    type Item",
            "    type Item = int",
            "3",
            "fn take(x: T::Item) -> int {\n    return x\n}\nfn probe() -> int {\n    let t = T { v: 1 }\n    return take(t.go())\n}\nprobe()\n",
        ),
        _ => (
            "    const LIMIT: int",
            "    const LIMIT: int = 3",
            "0",
            "fn probe() -> int {\n    let t = T { v: 1 }\n    return t.go() + T::LIMIT\n}\nprobe()\n",
        ),
    };
    let leaf = links - 1;
    let mut source = format!("trait S0 {{\n{declaration}\n}}\n");
    for index in 1..links {
        let body = if index == leaf {
            "\n    fn go(self) -> int\n"
        } else {
            ""
        };
        source.push_str(&format!("trait S{index}: S{} {{{body}}}\n", index - 1));
    }
    source.push_str("struct T { v: int }\n");
    for index in implemented_from..links {
        let body = if index == 0 {
            format!("\n{definition}\n")
        } else if index == leaf {
            format!("\n    fn go(self) -> int {{ {returned} }}\n")
        } else {
            String::new()
        };
        source.push_str(&format!("impl S{index} for T {{{body}}}\n"));
    }
    source.push_str(tail);
    source
}

#[test]
fn implementing_a_trait_obliges_implementing_its_supertraits() {
    for namespace in ["type", "const"] {
        assert!(
            unmet_supertraits(&inherited_item_program(namespace, false, false, 1)).is_empty(),
            "an impl of a trait with no supertraits must raise no obligation"
        );
        for links in [2, 3, 4] {
            assert_eq!(
                run_ok(&supertrait_chain(links, 0, namespace)).as_int(),
                Some(3),
                "a chain of {links} links fully implemented must resolve the item and run"
            );
            for implemented_from in 1..links {
                let source = supertrait_chain(links, implemented_from, namespace);
                let expected: Vec<String> = (0..implemented_from)
                    .map(|index| format!("S{index}"))
                    .collect();
                let mut named: Vec<String> = unmet_supertraits(&source);
                named.sort();
                named.dedup();
                assert_eq!(
                    named, expected,
                    "the chain of {links} links implemented from S{implemented_from} up must \
                     oblige exactly {expected:?} in the {namespace} namespace"
                );
                let message = compile_message(&source);
                assert!(
                    message.contains("error[E0338]")
                        && message.contains("trait 'S0' is not implemented for T"),
                    "the obligation must be reported as E0338 naming 'S0': {message}"
                );
                assert!(
                    message.contains(&format!("impl S{implemented_from} for T")),
                    "E0338 must quote the impl header that made the promise: {message}"
                );
            }
        }
    }
}

fn bound_reaching_the_supertrait(base_impl: bool, through_the_item: bool) -> String {
    let mut source = if through_the_item {
        String::from("trait Base {\n    type Item\n    fn seed(self) -> Self::Item\n}\n")
    } else {
        String::from("trait Base {\n    fn seed(self) -> int\n}\n")
    };
    source.push_str("trait Derived: Base {\n    fn tag(self) -> int\n}\n");
    source.push_str("struct T { v: int }\n");
    if base_impl {
        source.push_str(match through_the_item {
            true => "impl Base for T {\n    type Item = int\n    fn seed(self) -> int { 2 }\n}\n",
            false => "impl Base for T {\n    fn seed(self) -> int { 2 }\n}\n",
        });
    }
    source.push_str("impl Derived for T {\n    fn tag(self) -> int { 1 }\n}\n");
    source.push_str(match through_the_item {
        true => "fn use_it<X: Derived>(x: X) -> X::Item {\n    return x.seed()\n}\n",
        false => "fn use_it<X: Derived>(x: X) -> int {\n    return x.seed() + x.tag()\n}\n",
    });
    source.push_str("fn probe() -> int {\n    return use_it(T { v: 1 })\n}\nprobe()\n");
    source
}

#[test]
fn a_bound_reaching_a_supertrait_names_the_impl_that_broke_the_promise() {
    for through_the_item in [false, true] {
        let expected = if through_the_item { 2 } else { 3 };
        assert_eq!(
            run_ok(&bound_reaching_the_supertrait(true, through_the_item)).as_int(),
            Some(expected),
            "the bound must reach the supertrait and run"
        );
        let message = compile_message(&bound_reaching_the_supertrait(false, through_the_item));
        assert!(
            message.contains("error[E0338]")
                && message.contains("trait 'Base' is not implemented for T"),
            "the missing supertrait impl must be E0338: {message}"
        );
        assert!(
            message.contains("impl Derived for T") && !message.contains("use_it"),
            "E0338 must quote the impl header and not the caller: {message}"
        );
    }
}

fn unsatisfied_trait_site(at_the_impl_header: bool) -> String {
    if at_the_impl_header {
        String::from(
            "trait Base {\n    fn base(self) -> int\n}\ntrait Derived: Base {\n    fn tag(self) -> int\n}\nstruct T { v: int }\nimpl Derived for T {\n    fn tag(self) -> int { 1 }\n}\nfn probe() -> int {\n    let t = T { v: 1 }\n    return t.tag()\n}\nprobe()\n",
        )
    } else {
        String::from(
            "trait Base {\n    fn base(self) -> int\n}\nstruct T { v: int }\nfn rank<X: Base>(x: X) -> int {\n    return x.base()\n}\nfn probe() -> int {\n    return rank(T { v: 1 })\n}\nprobe()\n",
        )
    }
}

#[test]
fn each_site_of_e0338_offers_a_repair_writable_where_it_stands() {
    let at_the_bound = compile_message(&unsatisfied_trait_site(false));
    assert!(
        at_the_bound.contains("error[E0338]")
            && at_the_bound
                .contains("trait 'Base' is not implemented for T; add an impl or change the bound"),
        "the bound site keeps its clause: {at_the_bound}"
    );
    let at_the_impl = compile_message(&unsatisfied_trait_site(true));
    assert!(
        at_the_impl.contains("error[E0338]")
            && at_the_impl.contains(
                "trait 'Base' is not implemented for T, and the impl of 'Derived' requires it; \
                 implement 'Base' for T, or drop 'Base' from the supertraits of 'Derived'"
            ),
        "the impl header names the two repairs writable at that line: {at_the_impl}"
    );
    // repair the reader cannot perform there.
    assert!(
        !at_the_impl.contains("change the bound"),
        "the impl header must not offer to change a bound: {at_the_impl}"
    );
    assert!(
        at_the_impl.contains("impl Derived for T"),
        "the caret must stand on the impl header: {at_the_impl}"
    );
}

fn supertrait_diamond(root_impl: bool, branch_impls: bool, namespace: &str) -> String {
    let (declaration, definition, returned, tail) = match namespace {
        "type" => (
            "    type Item",
            "    type Item = int",
            "3",
            "fn take(x: T::Item) -> int {\n    return x\n}\nfn probe() -> int {\n    let t = T { v: 1 }\n    return take(t.go())\n}\nprobe()\n",
        ),
        _ => (
            "    const LIMIT: int",
            "    const LIMIT: int = 3",
            "0",
            "fn probe() -> int {\n    let t = T { v: 1 }\n    return t.go() + T::LIMIT\n}\nprobe()\n",
        ),
    };
    let mut source = format!("trait Root {{\n{declaration}\n}}\n");
    source.push_str("trait B1: Root {}\n");
    source.push_str("trait B2: Root {}\n");
    source.push_str("trait D: B1 + B2 {\n    fn go(self) -> int\n}\n");
    source.push_str("struct T { v: int }\n");
    if root_impl {
        source.push_str(&format!("impl Root for T {{\n{definition}\n}}\n"));
    }
    if branch_impls {
        source.push_str("impl B1 for T {}\n");
        source.push_str("impl B2 for T {}\n");
    }
    source.push_str(&format!(
        "impl D for T {{\n    fn go(self) -> int {{ {returned} }}\n}}\n"
    ));
    source.push_str(tail);
    source
}

#[test]
fn a_diamond_obliges_the_shared_root_once() {
    for namespace in ["type", "const"] {
        assert_eq!(
            run_ok(&supertrait_diamond(true, true, namespace)).as_int(),
            Some(3),
            "the fully implemented diamond must resolve the item and run"
        );
        let mut named = unmet_supertraits(&supertrait_diamond(false, false, namespace));
        named.sort();
        assert_eq!(
            named,
            vec!["B1".to_string(), "B2".to_string(), "Root".to_string()],
            "the impl of 'D' must oblige both branches and the shared root once"
        );
        let mut through_branches = unmet_supertraits(&supertrait_diamond(false, true, namespace));
        through_branches.sort();
        assert_eq!(
            through_branches,
            vec!["Root".to_string(), "Root".to_string(), "Root".to_string()],
            "the impls of 'B1', 'B2' and 'D' must each oblige 'Root' exactly once"
        );
    }
}

// bounds and the two branch impls in either order, and with or without the two
fn diamond_in_order(
    traits_swapped: bool,
    bounds_swapped: bool,
    impls_swapped: bool,
    branch_impls: bool,
) -> String {
    let branch = |first: bool| {
        if first == traits_swapped {
            "trait B2: Root {}\n"
        } else {
            "trait B1: Root {}\n"
        }
    };
    let bounds = if bounds_swapped { "B2 + B1" } else { "B1 + B2" };
    let impls = match (branch_impls, impls_swapped) {
        (false, _) => String::new(),
        (true, true) => String::from("impl B2 for T {}\nimpl B1 for T {}\n"),
        (true, false) => String::from("impl B1 for T {}\nimpl B2 for T {}\n"),
    };
    format!(
        "trait Root {{\n    type Item\n}}\n{}{}trait D: {bounds} {{\n    fn go(self) -> int\n}}\nstruct T {{ v: int }}\n{impls}impl D for T {{\n    fn go(self) -> int {{ 3 }}\n}}\nfn probe() -> int {{\n    1\n}}\nprobe()\n",
        branch(true),
        branch(false)
    )
}

#[test]
fn the_supertrait_obligation_does_not_depend_on_declaration_order() {
    let expected = compile_message(&diamond_in_order(false, false, false, false));
    assert!(
        expected.contains("error[E0338]") && expected.contains("trait 'B1' is not implemented"),
        "the witness must render the first of three obligations: {expected}"
    );
    for traits_swapped in [false, true] {
        for bounds_swapped in [false, true] {
            let message = compile_message(&diamond_in_order(
                traits_swapped,
                bounds_swapped,
                false,
                false,
            ));
            assert_eq!(
                message, expected,
                "swapping the branch declarations ({traits_swapped}) or the bound list \
                 ({bounds_swapped}) renders a different obligation"
            );
        }
    }
    let ordered = compile_message(&diamond_in_order(false, false, false, true));
    let swapped = compile_message(&diamond_in_order(false, false, true, true));
    assert!(
        swapped.contains("impl B2 for T") && ordered.contains("impl B1 for T"),
        "the impl that stands first must be the one named: {swapped}"
    );
    assert_eq!(
        swapped.replace("B2", "B1"),
        ordered,
        "swapping the two impls changes more than which of them is accused: {swapped}"
    );
    assert_renders_identically(
        &diamond_in_order(false, false, false, false),
        "the supertrait obligation",
    );
}

fn item_written_in_the_inheriting_impl(
    namespace: &str,
    base_defines: bool,
    derived_times: usize,
) -> String {
    let (declaration, definition, returned, tail) = match namespace {
        "type" => (
            "    type Item",
            "    type Item = int",
            "3",
            "fn take(x: T::Item) -> int {\n    return x\n}\nfn probe() -> int {\n    let t = T { v: 1 }\n    return take(t.go())\n}\nprobe()\n",
        ),
        _ => (
            "    const LIMIT: int",
            "    const LIMIT: int = 3",
            "0",
            "fn probe() -> int {\n    let t = T { v: 1 }\n    return t.go() + T::LIMIT\n}\nprobe()\n",
        ),
    };
    let mut source = format!("trait Base {{\n{declaration}\n}}\n");
    source.push_str("trait Derived: Base {\n    fn go(self) -> int\n}\n");
    source.push_str("struct T { v: int }\n");
    let base_body = match base_defines {
        true => format!("\n{definition}\n"),
        false => String::new(),
    };
    source.push_str(&format!("impl Base for T {{{base_body}}}\n"));
    source.push_str("impl Derived for T {\n");
    for _ in 0..derived_times {
        source.push_str(&format!("{definition}\n"));
    }
    source.push_str(&format!("    fn go(self) -> int {{ {returned} }}\n}}\n"));
    source.push_str(tail);
    source
}

#[test]
fn an_item_a_supertrait_declares_may_not_be_written_in_the_inheriting_impl() {
    for (namespace, keyword, item) in [("type", "type", "Item"), ("const", "const", "LIMIT")] {
        assert_eq!(
            run_ok(&item_written_in_the_inheriting_impl(namespace, true, 0)).as_int(),
            Some(3),
            "the definition of '{item}' in the impl of 'Base' must resolve and run"
        );
        let message = compile_message(&item_written_in_the_inheriting_impl(namespace, true, 1));
        assert_associated_diagnostic(
            &message,
            "E0425",
            &format!(
                "associated item '{keyword} {item}' is defined in the impl of trait 'Derived' for 'T', which does not declare it"
            ),
            "impl of trait 'Derived' for 'T'",
            &format!("move it into an impl of a trait that declares '{item}'"),
        );
        assert!(
            !message.contains("no trait declares it"),
            "E0425 accuses the implemented trait, and a supertrait does declare '{item}': {message}"
        );
        for derived_times in [0, 1] {
            let missing = compile_message(&item_written_in_the_inheriting_impl(
                namespace,
                false,
                derived_times,
            ));
            assert_associated_diagnostic(
                &missing,
                "E0421",
                &format!(
                    "implementation of trait 'Base' is missing required associated item '{item}'"
                ),
                "impl of trait 'Base' for 'T'",
                &format!("define '{keyword} {item}' in the impl body"),
            );
        }
        let duplicated = compile_errors(&item_written_in_the_inheriting_impl(namespace, true, 2));
        assert!(
            !duplicated
                .iter()
                .any(|error| matches!(&error.kind, TypeErrorKind::DuplicateAssociatedItem { .. })),
            "'{item}' is no business of the impl of 'Derived', so it is not counted there"
        );
        assert_eq!(
            duplicated
                .iter()
                .filter(|error| matches!(
                    &error.kind,
                    TypeErrorKind::AssociatedItemOutsideTraitImpl { .. }
                ))
                .count(),
            2,
            "both copies of '{item}' must be reported where they stand"
        );
    }
}

// item no impl defines, which is the rejected half of the same program.
#[derive(Clone, Copy)]
enum TraitProjectionRole {
    MethodReturn,
    MethodParameter,
    AssociatedConstType,
    ArrayLength,
    WhereBinding,
    DefaultBodyAnnotation,
    DefaultBodyReturn,
    DefaultBodyValue,
}

const EVERY_TRAIT_PROJECTION_ROLE: &[(TraitProjectionRole, &str)] = &[
    (TraitProjectionRole::MethodReturn, "a return type"),
    (TraitProjectionRole::MethodParameter, "a parameter type"),
    (
        TraitProjectionRole::AssociatedConstType,
        "an associated item definition",
    ),
    (TraitProjectionRole::ArrayLength, "an array length"),
    (TraitProjectionRole::WhereBinding, "a bound"),
    (
        TraitProjectionRole::DefaultBodyAnnotation,
        "a type annotation",
    ),
    (TraitProjectionRole::DefaultBodyReturn, "a return type"),
    (TraitProjectionRole::DefaultBodyValue, "a value expression"),
];

const TRAIT_PROJECTION_PRELUDE: &str = r#"trait Chain {
    type Item
    const LIMIT: int
    fn ping(self) -> int
}
struct Node { n: int }
impl Chain for Node {
    type Item = int
    const LIMIT: int = 2
    fn ping(self) -> int { return 1 }
}
struct Holder { h: int }
"#;

fn trait_projection_role_program(role: TraitProjectionRole, absent: bool) -> String {
    let ty = if absent { "Node::Nope" } else { "Node::Item" };
    let value = if absent { "Node::NOPE" } else { "Node::LIMIT" };
    let body = match role {
        TraitProjectionRole::MethodReturn => format!(
            "trait Second {{\n    fn fetch(self) -> {ty}\n}}\n\
             impl Second for Holder {{ fn fetch(self) -> int {{ return self.h }} }}\n\
             fn probe() -> int {{ return Holder {{ h: 7 }}.fetch() }}\nprobe()\n"
        ),
        TraitProjectionRole::MethodParameter => format!(
            "trait Second {{\n    fn fetch(self, v: {ty}) -> int\n}}\n\
             impl Second for Holder {{ fn fetch(self, v: int) -> int {{ return v }} }}\n\
             fn probe() -> int {{ return Holder {{ h: 1 }}.fetch(7) }}\nprobe()\n"
        ),
        TraitProjectionRole::AssociatedConstType => format!(
            "trait Second {{\n    const CAP: {ty}\n    fn fetch(self) -> int\n}}\n\
             impl Second for Holder {{\n    const CAP: int = 7\n    \
             fn fetch(self) -> int {{ return Holder::CAP }}\n}}\n\
             fn probe() -> int {{ return Holder {{ h: 1 }}.fetch() }}\nprobe()\n"
        ),
        TraitProjectionRole::ArrayLength => format!(
            "trait Second {{\n    fn fetch(self) -> [int; {value}]\n}}\n\
             impl Second for Holder {{ fn fetch(self) -> [int; 2] {{ return [3, 4] }} }}\n\
             fn probe() -> int {{\n    let a = Holder {{ h: 1 }}.fetch()\n    \
             return a[0] + a[1]\n}}\nprobe()\n"
        ),
        TraitProjectionRole::WhereBinding => format!(
            "trait Second {{\n    fn pull<T>(self, v: T) -> int where T: Chain<Item = {ty}>;\n}}\n\
             impl Second for Holder {{ fn pull<T>(self, v: T) -> int \
             where T: Chain<Item = {ty}> {{ return self.h }} }}\n\
             fn probe() -> int {{ return Holder {{ h: 7 }}.pull(Node {{ n: 1 }}) }}\nprobe()\n"
        ),
        TraitProjectionRole::DefaultBodyAnnotation => format!(
            "trait Second {{\n    fn fetch(self) -> int {{\n        let v: {ty} = 7\n        \
             return v\n    }}\n}}\n\
             impl Second for Holder {{ }}\n\
             fn probe() -> int {{ return Holder {{ h: 1 }}.fetch() }}\nprobe()\n"
        ),
        TraitProjectionRole::DefaultBodyReturn => format!(
            "trait Second {{\n    fn fetch(self) -> {ty} {{ return 7 }}\n}}\n\
             impl Second for Holder {{ }}\n\
             fn probe() -> int {{ return Holder {{ h: 1 }}.fetch() }}\nprobe()\n"
        ),
        TraitProjectionRole::DefaultBodyValue => format!(
            "trait Second {{\n    fn fetch(self) -> int {{ return {value} + 5 }}\n}}\n\
             impl Second for Holder {{ }}\n\
             fn probe() -> int {{ return Holder {{ h: 1 }}.fetch() }}\nprobe()\n"
        ),
    };
    format!("{TRAIT_PROJECTION_PRELUDE}{body}")
}

#[test]
fn a_nominal_projection_in_a_trait_declaration_resolves_wherever_it_is_written() {
    for (role, expected) in EVERY_TRAIT_PROJECTION_ROLE {
        assert_eq!(
            run_ok(&trait_projection_role_program(*role, false)).as_int(),
            Some(7),
            "the defined half of '{expected}' must run, not merely compile"
        );
    }
}

#[test]
fn an_absent_nominal_projection_in_a_trait_declaration_states_its_position() {
    for (role, expected) in EVERY_TRAIT_PROJECTION_ROLE {
        let message = compile_message(&trait_projection_role_program(*role, true));
        assert_associated_diagnostic(
            &message,
            "E0423",
            "cannot be resolved: no impl for 'Node' defines",
            expected,
            "define it in an impl of a trait that declares",
        );
    }
}

fn nominal_beside_trait(
    keyword: &str,
    nominal: &str,
    impls: usize,
    nominal_reading: bool,
    trait_method: bool,
) -> String {
    let declared_method = match trait_method {
        true => "    fn ping(self) -> int\n",
        false => "",
    };
    let mut source =
        format!("trait Twin {{\n    type Item\n    const LIMIT: int\n{declared_method}}}\n");
    source += &match keyword {
        "enum" => format!("enum {nominal} {{ Red, Green }}\n"),
        _ => format!("struct {nominal} {{ v: int }}\n"),
    };
    for (name, value) in [("Alpha", 4), ("Beta", 7)].iter().take(impls) {
        let defined_method = match trait_method {
            true => format!("    fn ping(self) -> int {{ return {value} }}\n"),
            false => String::new(),
        };
        source += &format!(
            "struct {name} {{ v: int }}\n\
             impl Twin for {name} {{\n    type Item = int\n    const LIMIT: int = {value}\n\
             {defined_method}}}\n"
        );
    }
    let mut read = "take(Twin::LIMIT)".to_string();
    if nominal_reading {
        source += &format!(
            "trait Other {{\n    type Item\n    const LIMIT: int\n}}\n\
             impl Other for {nominal} {{\n    type Item = int\n    const LIMIT: int = 9\n}}\n"
        );
        read += &format!(" + {nominal}::LIMIT");
    }
    if trait_method && impls > 0 {
        read += " + Alpha { v: 0 }.ping()";
    }
    source += &format!(
        "fn take(x: Twin::Item) -> int {{\n    return x\n}}\n\
         fn probe() -> int {{\n    return {read}\n}}\nprobe()\n"
    );
    source
}

#[test]
fn a_nominal_sharing_a_trait_name_is_e0326() {
    for keyword in ["struct", "enum"] {
        for impls in [0, 1, 2] {
            for nominal_reading in [false, true] {
                for trait_method in [false, true] {
                    let message = compile_message(&nominal_beside_trait(
                        keyword,
                        "Twin",
                        impls,
                        nominal_reading,
                        trait_method,
                    ));
                    assert_associated_diagnostic(
                        &message,
                        "E0326",
                        "duplicate trait declaration 'Twin'",
                        "",
                        "'Twin'",
                    );
                    assert_located_at("E0326", &message, 1);
                    assert!(
                        !message.contains("E0423"),
                        "the collision, not a projection that cannot see past it, \
                         answers for '{keyword}' with {impls} impls: {message}"
                    );
                }
            }
        }
    }
}

#[test]
fn a_nominal_that_shares_no_trait_name_still_projects_both_namespaces() {
    for keyword in ["struct", "enum"] {
        for (nominal_reading, trait_method, expected) in [
            (false, false, 4),
            (true, false, 13),
            (false, true, 8),
            (true, true, 17),
        ] {
            let result = run_ok(&nominal_beside_trait(
                keyword,
                "Cell",
                1,
                nominal_reading,
                trait_method,
            ));
            assert_eq!(
                result.as_int(),
                Some(expected),
                "'{keyword}' named apart from the trait resolves both readings"
            );
        }
    }
}

#[test]
fn a_name_collision_outranks_the_projection_it_breaks() {
    let message = compile_message(
        r#"
struct Twin { v: int }
fn probe() -> int {
    return Twin::LIMIT
}
trait Twin {
    const LIMIT: int
}
struct Alpha { v: int }
impl Twin for Alpha {
    const LIMIT: int = 4
}
probe()
"#,
    );
    assert_associated_diagnostic(
        &message,
        "E0326",
        "duplicate trait declaration 'Twin'",
        "",
        "'Twin'",
    );
    assert_located_at("E0326", &message, 6);
}

#[test]
fn an_associated_constant_value_mismatch_accuses_the_initialiser_and_not_the_declaration() {
    assert_eq!(
        run_ok(&associated_const_program("int", "3")).as_int(),
        Some(7),
        "the agreeing constant must keep compiling"
    );
    let value = compile_message(&associated_const_program("int", "\"ten\""));
    assert_associated_diagnostic(
        &value,
        "E0422",
        "LIMIT",
        "constant value in the impl for 'Bounds'",
        "write an initialiser of type 'i64'",
    );
    assert!(
        value.contains("is initialised with a value of type 'string'"),
        "the value mismatch must name the type the initialiser produced: {value}"
    );
    assert!(
        !value.contains("declare the same type as the trait"),
        "the value mismatch must not prescribe a declaration that already agrees: {value}"
    );
    let declared = compile_message(&associated_const_program("string", "\"ten\""));
    assert!(
        declared.contains("declare the same type as the trait"),
        "the declared mismatch keeps its own sentence: {declared}"
    );
    assert!(
        !declared.contains("write an initialiser"),
        "the declared mismatch must not borrow the value sentence: {declared}"
    );
}

fn array_length_program(length: &str) -> String {
    format!(
        "struct Node {{ v: int }}\ntrait Cap {{ const LIMIT: int; }}\nimpl Cap for Node {{ const LIMIT: int = 3; }}\nimpl Node {{\n    fn base(self) -> int {{ return self.v }}\n}}\nfn main() -> int {{\n    let node = Node {{ v: 2 }}\n    let xs: [int; Node::LIMIT] = [node.base(); {length}]\n    return xs[0] + xs[2]\n}}\nmain()\n"
    )
}

#[test]
fn an_array_repeat_count_folds_every_constant_its_annotation_folds() {
    for length in ["Node::LIMIT", "Cap::LIMIT", "Node::LIMIT + 0"] {
        assert_eq!(
            run_ok(&array_length_program(length)).as_int(),
            Some(4),
            "the repeat count '{length}' must produce a length of three, whose third element reads"
        );
    }
    let in_impl = run_ok(
        "struct Node { v: int }\ntrait Cap { const LIMIT: int; }\nimpl Cap for Node { const LIMIT: int = 3; }\nimpl Node {\n    fn row(self) -> int {\n        let xs: [int; Self::LIMIT] = [self.v; Self::LIMIT]\n        return xs[0] + xs[2]\n    }\n}\nfn main() -> int {\n    let node = Node { v: 5 }\n    return node.row()\n}\nmain()\n",
    );
    assert_eq!(in_impl.as_int(), Some(10), "'Self::LIMIT' folds in an impl");
}

#[test]
fn an_array_repeat_count_that_no_constant_resolves_keeps_its_own_diagnostic() {
    let dynamic = compile_message(
        "fn main() -> int {\n    let n = 3\n    let xs = [1; n]\n    return xs[0]\n}\nmain()\n",
    );
    assert!(
        dynamic.contains("E0316") && dynamic.contains("use Vec for a dynamic count"),
        "a repeat count no constant resolves keeps E0316: {dynamic}"
    );
    let negative = compile_message(
        "struct Node { v: int }\ntrait Cap { const LIMIT: int; }\nimpl Cap for Node { const LIMIT: int = -1; }\nfn main() -> int {\n    let xs = [1; Node::LIMIT]\n    return xs[0]\n}\nmain()\n",
    );
    assert!(
        negative.contains("E0315") && negative.contains("-1"),
        "a constant that folds to a negative length names the value: {negative}"
    );
    let non_integer = compile_message(
        "struct Node { v: int }\ntrait Cap { const LABEL: string; }\nimpl Cap for Node { const LABEL: string = \"x\"; }\nfn main() -> int {\n    let xs = [1; Node::LABEL]\n    return xs[0]\n}\nmain()\n",
    );
    assert!(
        non_integer.contains("E0423") && non_integer.contains("not a constant integer expression"),
        "a constant that is not an integer answers as the annotation does: {non_integer}"
    );
    let through_bound = compile_message(
        "struct Node { v: int }\ntrait Cap { const LIMIT: int; }\nimpl Cap for Node { const LIMIT: int = 3; }\nstruct Wrap<T> { inner: T }\nimpl<T: Cap> Wrap<T> {\n    fn row(self) -> int {\n        let xs = [1; T::LIMIT]\n        return xs[0]\n    }\n}\nfn main() -> int {\n    let w = Wrap { inner: Node { v: 1 } }\n    return w.row()\n}\nmain()\n",
    );
    assert!(
        through_bound.contains("E0423") && through_bound.contains("type parameter 'T'"),
        "a constant reached through a bound is not a dynamic count: {through_bound}"
    );
}
