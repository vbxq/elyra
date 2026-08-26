use aelys_common::error::CompileErrorKind;
use aelys_sema::constraint::{Constraint, ConstraintReason, TypeErrorKind};
use aelys_sema::types::{InferType, TypeVarId};
use aelys_syntax::Span;

use aelys::{CompileOptions, Runtime, run};
use aelys_runtime::Value;

fn run_ok(source: &str) -> Value {
    run(source, "sema_constraint_tests.aelys").expect("program should run")
}

fn compile_message(source: &str) -> String {
    match Runtime::new().compile(source, CompileOptions::default()) {
        Ok(_) => panic!("the source must be rejected"),
        Err(error) => error.to_string(),
    }
}

fn assert_associated_diagnostic(message: &str, code: &str, subject: &str, help: &str) {
    assert!(message.contains(code), "expected {code}, got: {message}");
    assert!(
        message.contains("-->") && message.contains('^'),
        "{code} must render a source span with a caret: {message}"
    );
    // a dummy span renders as line 0 and would satisfy the check above while
    let located = message
        .lines()
        .find_map(|line| line.trim().strip_prefix("--> "))
        .and_then(|location| location.rsplit(':').nth(1))
        .and_then(|line| line.parse::<u32>().ok());
    assert!(
        located.is_some_and(|line| line > 0),
        "{code} must point at a real source line, not a dummy span: {message}"
    );
    assert!(
        message.contains(subject),
        "{code} must name '{subject}': {message}"
    );
    assert!(
        message.contains(help),
        "{code} must offer the corrective clause '{help}': {message}"
    );
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
    assert_associated_diagnostic(&message, "E0421", "Item", "define 'type Item'");
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
    assert_associated_diagnostic(&message, "E0423", "Item", "add a bound");
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
    assert_associated_diagnostic(&message, "E0423", "Cell::Item", "break the cycle");
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
    assert_associated_diagnostic(&message, "E0423", "Cell::", "break the cycle");
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
    assert_associated_diagnostic(&message, "E0425", "Item", "impl of a trait");
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
    assert_associated_diagnostic(&message, "E0425", "LIMIT", "impl of a trait");
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
    // walk explores every path and costs 2^n, so this hangs rather than fails.
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
    assert_associated_diagnostic(&message, "E0423", "Bounds::LIMIT", "break the cycle");
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
        "remove one of the competing",
    );
    assert_eq!(
        first, swapped,
        "declaration order must never change the diagnostic"
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
    assert_associated_diagnostic(&message, "E0423", "Bounds::MISSING", "no impl for");
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
    assert_associated_diagnostic(&message, "E0422", "LIMIT", "declare the same type");
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
    assert_associated_diagnostic(&message, "E0423", "T::LIMIT", "add a bound on 'T'");
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
        "give one of them a concrete definition",
    );
}

#[test]
fn an_exponential_constant_graph_is_evaluated_once_per_constant() {
    // this hangs rather than fails.
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
    assert_associated_diagnostic(&message, "E0423", "Bounds::LIMIT", "cannot be computed");
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
    assert_associated_diagnostic(&message, "E0423", "Bounds::LIMIT", "cannot be computed");
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
