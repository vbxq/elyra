use aelys::run;
use aelys_runtime::Value;

fn run_ok(source: &str) -> Value {
    run(source, "test.aelys").expect("Expected program to run successfully")
}

#[allow(dead_code)]
fn run_err(source: &str) -> String {
    run(source, "test.aelys")
        .expect_err("Expected program to fail")
        .to_string()
}

#[test]
fn test_let_with_int_type() {
    let result = run_ok(
        r#"
        let x: int = 42
        x
    "#,
    );
    assert_eq!(result.as_int(), Some(42));
}

#[test]
#[allow(clippy::approx_constant)]
fn test_let_with_float_type() {
    let result = run_ok(
        r#"
        let x: float = 3.14
        x
    "#,
    );
    assert!((result.as_float().unwrap() - 3.14).abs() < 0.001);
}

#[test]
fn test_let_with_bool_type() {
    let result = run_ok(
        r#"
        let x: bool = true
        x
    "#,
    );
    assert_eq!(result.as_bool(), Some(true));
}

#[test]
fn test_let_with_string_type() {
    let result = run_ok(
        r#"
        let x: string = "hello"
        x
    "#,
    );
    assert!(result.as_ptr().is_some());
}

#[test]
fn test_let_mutable_with_type() {
    let result = run_ok(
        r#"
        let mut x: int = 10
        x += 5
        x
    "#,
    );
    assert_eq!(result.as_int(), Some(15));
}

#[test]
fn test_function_with_typed_params() {
    let result = run_ok(
        r#"
        fn add(a: int, b: int) {
            return a + b
        }
        add(3, 4)
    "#,
    );
    assert_eq!(result.as_int(), Some(7));
}

#[test]
fn test_function_with_return_type() {
    let result = run_ok(
        r#"
        fn square(x: int) -> int {
            return x * x
        }
        square(5)
    "#,
    );
    assert_eq!(result.as_int(), Some(25));
}

#[test]
fn test_function_with_typed_params_and_return() {
    let result = run_ok(
        r#"
        fn multiply(a: int, b: int) -> int {
            return a * b
        }
        multiply(6, 7)
    "#,
    );
    assert_eq!(result.as_int(), Some(42));
}

#[test]
fn test_function_void_return_type() {
    let result = run_ok(
        r#"
        let mut counter: int = 0

        fn increment() -> void {
            counter++
        }

        increment()
        increment()
        counter
    "#,
    );
    assert_eq!(result.as_int(), Some(2));
}

#[test]
fn test_lambda_with_typed_params() {
    let result = run_ok(
        r#"
        let add = fn(a: int, b: int) { return a + b }
        add(10, 20)
    "#,
    );
    assert_eq!(result.as_int(), Some(30));
}

#[test]
fn test_lambda_with_return_type() {
    let result = run_ok(
        r#"
        let double = fn(x: int) -> int { return x * 2 }
        double(15)
    "#,
    );
    assert_eq!(result.as_int(), Some(30));
}

#[test]
fn test_infer_int_literal() {
    let result = run_ok(
        r#"
        let x = 100
        x + 50
    "#,
    );
    assert_eq!(result.as_int(), Some(150));
}

#[test]
fn test_infer_float_literal() {
    let result = run_ok(
        r#"
        let x = 2.5
        x * 4.0
    "#,
    );
    assert!((result.as_float().unwrap() - 10.0).abs() < 0.001);
}

#[test]
fn test_infer_bool_literal() {
    let result = run_ok(
        r#"
        let x = true
        let y = false
        x and not y
    "#,
    );
    assert_eq!(result.as_bool(), Some(true));
}

#[test]
fn test_infer_from_binary_op() {
    let result = run_ok(
        r#"
        let sum = 10 + 20
        let diff = sum - 5
        diff
    "#,
    );
    assert_eq!(result.as_int(), Some(25));
}

#[test]
fn test_int_addition_specialized() {
    let result = run_ok(
        r#"
        let a: int = 100
        let b: int = 200
        a + b
    "#,
    );
    assert_eq!(result.as_int(), Some(300));
}

#[test]
fn enum_match_missing_variant_reports_named_diagnostic() {
    let error = run(
        r#"
        enum Color { Red, Blue }
        fn value(color: Color) -> int {
            match color {
                Color::Red => 1
            }
        }
        value(Color::Red)
        "#,
        "stage2_enum_match.aelys",
    )
    .expect_err("a match missing a closed enum variant must be rejected")
    .to_string();
    assert!(
        error.contains("error[E0302]")
            && error.contains("non-exhaustive match")
            && error.contains("Color::Blue"),
        "expected E0302 exhaustivity diagnostic, got: {error}"
    );
}

#[test]
fn enum_match_missing_named_variant_suggests_a_rest_pattern() {
    let error = run(
        r#"
        enum Shape { Empty, Point { x: int, y: int } }
        fn value(shape: Shape) -> int {
            match shape {
                Shape::Empty => 0,
            }
        }
        value(Shape::Empty)
        "#,
        "stage2_named_enum_match.aelys",
    )
    .expect_err("a match missing a named enum variant must be rejected")
    .to_string();
    assert!(
        error.contains("Shape::Point { .. }") && error.contains("add a missing arm"),
        "expected a repairable named-enum diagnostic, got: {error}"
    );
}

#[test]
fn enum_pattern_rejects_a_wrong_qualified_path() {
    let error = run(
        r#"
        enum Shape { Pair(int), Empty }
        fn value(shape: Shape) -> int {
            match shape {
                Other::Pair(number) => number,
                Shape::Pair(number) => number,
                Shape::Empty => 0,
            }
        }
        value(Shape::Empty)
        "#,
        "stage2_wrong_enum_path.aelys",
    )
    .expect_err("a pattern from another enum path must be rejected")
    .to_string();
    assert!(
        error.contains("unknown variant") && error.contains("Other::Pair"),
        "expected a named enum path diagnostic, got: {error}"
    );
}

#[test]
fn named_enum_pattern_rejects_a_wrong_qualified_path() {
    let error = run(
        r#"
        enum Shape { Point { x: int }, Empty }
        fn value(shape: Shape) -> int {
            match shape {
                Other::Point { x } => x,
                Shape::Point { x } => x,
                Shape::Empty => 0,
            }
        }
        value(Shape::Empty)
        "#,
        "stage2_wrong_named_enum_path.aelys",
    )
    .expect_err("a named pattern from another enum path must be rejected")
    .to_string();
    assert!(
        error.contains("unknown variant") && error.contains("Other::Point"),
        "expected a named enum path diagnostic, got: {error}"
    );
}

#[test]
fn data_enum_construction_and_matching_execute() {
    let value = run(
        r#"
        enum Message { Number(int), Pair(int, int), Empty }
        fn read(message: Message) -> int {
            match message {
                Message::Number(value) => value,
                Message::Pair(left, right) => left + right,
                Message::Empty => 0,
            }
        }
        read(Message::Pair(2, 3))
        "#,
        "stage2_enum_runtime.aelys",
    )
    .expect("data-carrying enum should execute");
    assert_eq!(value, aelys_runtime::Value::int(5));
}

#[test]
fn data_carrying_enum_variants_execute_and_bind() {
    let result = run_ok(
        r#"
        enum Shape {
            Unit,
            Pair(int, int),
            Point { x: int, y: int },
        }

        fn score(shape: Shape) -> int {
            match shape {
                Shape::Unit => 1,
                Shape::Pair(left, right) => left + right,
                Shape::Point { x, y } => x + y,
            }
        }

        score(Shape::Unit) + score(Shape::Pair(2, 3)) + score(Shape::Point { x: 4, y: 5 })
        "#,
    );
    assert_eq!(result.as_int(), Some(15));
}

#[test]
fn enum_fields_can_reference_a_later_declaration() {
    let result = run_ok(
        r#"
        enum Outer { Wrapped(Inner) }
        enum Inner { Number(int), Empty }

        fn score(value: Outer) -> int {
            match value {
                Outer::Wrapped(Inner::Number(number)) => number,
                Outer::Wrapped(Inner::Empty) => 0,
            }
        }

        score(Outer::Wrapped(Inner::Number(9)))
        "#,
    );
    assert_eq!(result.as_int(), Some(9));
}

#[test]
fn enum_alternation_patterns_execute() {
    let result = run_ok(
        r#"
        enum Color { Red, Blue, Green }
        fn score(color: Color) -> int {
            match color {
                Color::Red | Color::Blue => 1,
                Color::Green => 2,
            }
        }
        score(Color::Green)
        "#,
    );
    assert_eq!(result.as_int(), Some(2));
}

#[test]
fn data_enum_alternation_binds_the_matching_variant() {
    let result = run_ok(
        r#"
        enum Color { Red(int), Blue(int), Green }
        fn score(color: Color) -> int {
            match color {
                Color::Red(value) | Color::Blue(value) => value,
                Color::Green => 0,
            }
        }
        score(Color::Blue(7))
        "#,
    );
    assert_eq!(result.as_int(), Some(7));
}

#[test]
fn enum_payload_over_u16_slots_has_a_named_compile_error() {
    let slot_count = usize::from(u16::MAX) + 1;
    let fields = std::iter::repeat_n("int", slot_count)
        .collect::<Vec<_>>()
        .join(", ");
    let values = std::iter::repeat_n("0", slot_count)
        .collect::<Vec<_>>()
        .join(", ");
    let source = format!(
        "enum Huge {{ Payload({fields}) }}\nfn make() -> Huge {{ Huge::Payload({values}) }}\nmake()"
    );
    let error = run(&source, "stage2_enum_layout.aelys")
        .expect_err("an enum payload that cannot fit the bytecode slot count must be rejected")
        .to_string();
    assert!(
        error.contains("error[E0346]")
            && error.contains("EnumLayoutTooLarge")
            && error.contains("65536"),
        "expected the named enum layout diagnostic, got: {error}"
    );
}

#[test]
fn named_enum_fields_are_emitted_in_declaration_order() {
    let result = run_ok(
        r#"
        enum Point {
            Value { x: int, y: int },
        }
        fn score(point: Point) -> int {
            match point {
                Point::Value { x, y } => x * 10 + y,
            }
        }
        score(Point::Value { y: 3, x: 4 })
        "#,
    );
    assert_eq!(result.as_int(), Some(43));
}

#[test]
fn nested_data_carrying_enum_patterns_execute() {
    let result = run_ok(
        r#"
        enum Inner {
            Unit,
            Point { x: int, y: int },
        }
        enum Outer {
            Wrapped(Inner),
            Empty,
        }
        fn score(value: Outer) -> int {
            match value {
                Outer::Wrapped(Inner::Point { x, .. }) => x,
                Outer::Wrapped(Inner::Unit) => 3,
                Outer::Empty => 4,
            }
        }
        score(Outer::Wrapped(Inner::Point { x: 8, y: 9 }))
        "#,
    );
    assert_eq!(result.as_int(), Some(8));
}

#[test]
fn multi_field_enum_patterns_union_nested_coverage() {
    let result = run_ok(
        r#"
        enum Inner { A, B }
        enum Outer { Pair(Inner, Inner), Empty }
        fn score(value: Outer) -> int {
            match value {
                Outer::Pair(Inner::A, _) => 1,
                Outer::Pair(Inner::B, _) => 2,
                Outer::Empty => 3,
            }
        }
        score(Outer::Pair(Inner::B, Inner::A))
        "#,
    );
    assert_eq!(result.as_int(), Some(2));
}

#[test]
fn nested_enum_match_missing_variant_reports_named_diagnostic() {
    let error = run(
        r#"
        enum Inner {
            Unit,
            Point { x: int },
        }
        enum Outer {
            Wrapped(Inner),
        }
        fn score(value: Outer) -> int {
            match value {
                Outer::Wrapped(Inner::Point { x, .. }) => x,
            }
        }
        score(Outer::Wrapped(Inner::Unit))
        "#,
        "stage2_nested_enum_match.aelys",
    )
    .expect_err("a nested enum match missing a variant must be rejected")
    .to_string();
    assert!(
        error.contains("non-exhaustive match") && error.contains("Outer::Wrapped"),
        "expected nested exhaustivity diagnostic, got: {error}"
    );
}

#[test]
fn guarded_enum_arm_does_not_count_as_exhaustive() {
    let error = run(
        r#"
        enum Color { Red, Blue }
        fn value(color: Color) -> int {
            match color {
                Color::Red if true => 1,
                Color::Blue => 2,
            }
        }
        value(Color::Red)
        "#,
        "stage2_guarded_enum_match.aelys",
    )
    .expect_err("a guarded enum arm must not provide exhaustivity coverage")
    .to_string();
    assert!(
        error.contains("error[E0302]")
            && error.contains("non-exhaustive match")
            && error.contains("Color::Red"),
        "expected guarded-arm exhaustivity diagnostic, got: {error}"
    );
}

#[test]
fn trait_impl_for_builtin_collection_reports_orphan_diagnostic() {
    let error = run(
        r#"
        trait Marker {
            fn mark(self) -> int;
        }
        impl Marker for Vec<int> {
            fn mark(self) -> int { 0 }
        }
        "#,
        "stage2_orphan_trait.aelys",
    )
    .expect_err("a trait implementation for a builtin collection must be rejected")
    .to_string();
    assert!(
        error.contains("cannot implement trait 'Marker'")
            && error.contains("the trait or type must be local"),
        "expected orphan-rule diagnostic, got: {error}"
    );
}

#[test]
fn overlapping_generic_trait_impl_reports_named_diagnostic() {
    let error = run(
        r#"
        trait Marker {
            fn mark(self) -> int;
        }
        struct Box<T> { value: T }
        impl<T> Marker for Box<T> {
            fn mark(self) -> int { 1 }
        }
        impl Marker for Box<int> {
            fn mark(self) -> int { 2 }
        }
        "#,
        "stage2_overlapping_trait.aelys",
    )
    .expect_err("overlapping generic trait implementations must be rejected")
    .to_string();
    assert!(
        error.contains("overlapping implementations") && error.contains("Marker"),
        "expected overlap diagnostic, got: {error}"
    );
}

#[test]
fn generic_trait_impl_dispatches_for_a_concrete_instance() {
    let result = run_ok(
        r#"
        trait Extract {
            fn extract(self) -> int;
        }
        struct Box<T> { value: T }
        impl<T> Extract for Box<T> {
            fn extract(self) -> int { 7 }
        }
        fn read(value: Box<int>) -> int {
            value.extract()
        }
        read(Box { value: 3 })
        "#,
    );
    assert_eq!(result.as_int(), Some(7));
}

#[test]
fn generic_trait_argument_substitutes_in_method_signature() {
    let result = run_ok(
        r#"
        trait Echo<T> {
            fn echo(self, value: T) -> T;
        }
        struct Box<T> { value: T }
        impl<T> Echo<T> for Box<T> {
            fn echo(self, value: T) -> T { value }
        }
        fn read(value: Box<int>) -> int {
            value.echo(8)
        }
        read(Box { value: 3 })
        "#,
    );
    assert_eq!(result.as_int(), Some(8));
}

#[test]
fn generic_trait_argument_rejects_a_wrong_method_argument() {
    let error = run(
        r#"
        trait Echo<T> {
            fn echo(self, value: T) -> T;
        }
        struct Box<T> { value: T }
        impl<T> Echo<T> for Box<T> {
            fn echo(self, value: T) -> T { value }
        }
        fn read(value: Box<int>) -> int {
            value.echo("wrong")
        }
        read(Box { value: 3 })
        "#,
        "stage2_generic_trait_argument.aelys",
    )
    .expect_err("a generic trait method must use the concrete trait argument")
    .to_string();
    assert!(
        error.contains("expected i64") && error.contains("found string"),
        "{error}"
    );
}

#[test]
fn applied_trait_arguments_select_the_impl_signature() {
    let result = run_ok(
        r#"
        trait Echo<T> {
            fn echo(self, value: T) -> T;
        }
        struct Box<U> { value: U }
        impl Echo<int> for Box<string> {
            fn echo(self, value: int) -> int { value }
        }
        fn read(value: Box<string>) -> int {
            value.echo(8)
        }
        read(Box { value: "stored" })
        "#,
    );
    assert_eq!(result.as_int(), Some(8));
}

#[test]
fn applied_trait_arguments_reject_a_mismatched_impl_method() {
    let error = run(
        r#"
        trait Echo<T> {
            fn echo(self, value: T) -> T;
        }
        struct Box<U> { value: U }
        impl Echo<int> for Box<string> {
            fn echo(self, value: string) -> string { value }
        }
        "#,
        "stage2_applied_trait_signature.aelys",
    )
    .expect_err("an impl must substitute the applied trait arguments")
    .to_string();
    assert!(error.contains("does not match the signature"), "{error}");
}

#[test]
fn trait_impl_rejects_a_mismatched_method_signature() {
    let error = run(
        r#"
        trait Marker {
            fn mark(self, value: int) -> int;
        }
        struct Point { value: int }
        impl Marker for Point {
            fn mark(self, value: string) -> string { value }
        }
        "#,
        "stage2_trait_signature.aelys",
    )
    .expect_err("a trait impl method must match every parameter and its return")
    .to_string();
    assert!(error.contains("does not match the signature"), "{error}");
}

#[test]
fn associated_trait_method_ambiguity_requires_a_qualified_trait() {
    let error = run(
        r#"
        trait First {
            fn build() -> int;
        }
        trait Second {
            fn build() -> int;
        }
        struct Point { value: int }
        impl First for Point {
            fn build() -> int { 1 }
        }
        impl Second for Point {
            fn build() -> int { 2 }
        }
        Point::build()
        "#,
        "stage2_trait_ambiguity.aelys",
    )
    .expect_err("an ambiguous associated trait method must be rejected")
    .to_string();
    assert!(error.contains("error[E0337]"), "expected E0337: {error}");
    assert!(
        error.contains("provided by more than one trait"),
        "expected the ambiguity diagnostic: {error}"
    );
}

#[test]
fn recursive_generic_instantiation_rejects_type_growth() {
    let error = run(
        r#"
        fn first<T>(value: T, count: int) -> int {
            if count == 0 { 0 } else { second(vec![value], count - 1) }
        }
        fn second<T>(value: T, count: int) -> int {
            if count == 0 { 0 } else { first(vec![value], count - 1) }
        }
        first(1, 1)
        "#,
        "stage2_recursive_monomorphization.aelys",
    )
    .expect_err("generic type growth in a recursive SCC must be rejected")
    .to_string();
    assert!(
        error.contains("recursive") && error.contains("generic"),
        "{error}"
    );
}

#[test]
fn direct_generic_instantiation_rejects_type_growth() {
    let error = run(
        r#"
        fn looped<T>(value: T, count: int) -> int {
            if count == 0 { 0 } else { looped("fixed", count - 1) }
        }
        looped(1, 1)
        "#,
        "stage2_direct_recursive_monomorphization.aelys",
    )
    .expect_err("a direct generic call that changes its type must be rejected")
    .to_string();
    assert!(
        error.contains("recursive") && error.contains("generic"),
        "{error}"
    );
}

#[test]
fn mutually_recursive_generics_at_one_instance_terminate() {
    let result = run_ok(
        r#"
        fn h<T>(x: T) -> int { 1 }
        fn f<T>(x: T) -> int { g(x) }
        fn g<T>(x: T) -> int { f(x) + h(x) }
        fn probe() -> int { f(1) }
        7
        "#,
    );
    assert_eq!(result.as_int(), Some(7));
}

#[test]
fn generic_function_instantiation_is_fresh_per_call() {
    let result = run_ok(
        r#"
        fn id<T>(value: T) -> T {
            value
        }
        let number = id(7)
        let text = id("ok")
        number
        "#,
    );
    assert_eq!(result.as_int(), Some(7));
}

#[test]
fn turbofish_selects_a_generic_function_type() {
    let result = run_ok(
        r#"
        fn id<T>(value: T) -> T {
            value
        }
        id::<int>(7)
        "#,
    );
    assert_eq!(result.as_int(), Some(7));
}

#[test]
fn generic_enum_constructor_and_match_execute() {
    let result = run_ok(
        r#"
        enum Maybe<T> { Some(T), None }
        fn read(value: Maybe<int>) -> int {
            match value {
                Maybe::Some(number) => number,
                Maybe::None => 0,
            }
        }
        read(Maybe::<int>::Some(7))
        "#,
    );
    assert_eq!(result.as_int(), Some(7));
}

#[test]
fn generic_enum_turbofish_patterns_execute_and_check_coverage() {
    let result = run_ok(
        r#"
        enum Maybe<T> { Some(T), None }
        fn read(value: Maybe<int>) -> int {
            match value {
                Maybe::<int>::Some(number) => number,
                Maybe::<int>::None => 0,
            }
        }
        read(Maybe::<int>::Some(11))
        "#,
    );
    assert_eq!(result.as_int(), Some(11));
}

#[test]
fn generic_struct_turbofish_constructs_and_substitutes_fields() {
    let value = run(
        r#"
        struct Box<T> { value: T }
        let value = Box::<int> { value: 7 }
        value.value
        "#,
        "stage2_generic_struct_turbofish.aelys",
    )
    .expect("a generic struct turbofish must compile and execute");
    assert_eq!(value, aelys_runtime::Value::int(7));
}

#[test]
fn nested_applied_structs_materialize_each_concrete_schema() {
    let value = run(
        r#"
        struct Inner<T> { value: T }
        struct Wrapper<T> { value: T }
        struct Outer<T> { inner: Wrapper<T> }
        let inner = Inner::<int> { value: 7 }
        let outer = Outer::<Inner<int>> { inner: Wrapper { value: inner } }
        outer.inner.value.value
        "#,
        "stage2_nested_applied_structs.aelys",
    )
    .expect("nested applied structs must lower to concrete schemas");
    assert_eq!(value, aelys_runtime::Value::int(7));
}

#[test]
fn generic_function_checks_a_trait_bound_at_the_call_site() {
    let result = run_ok(
        r#"
trait Scorable {
    fn score(self) -> int;
}
struct Point { x: int }
impl Scorable for Point {
    fn score(self) -> int { self.x }
}
fn identity<T: Scorable>(value: T) -> T {
    value
}
let point: Point = identity(Point { x: 9 })
point.x
"#,
    );
    assert_eq!(result.as_int(), Some(9));
}

#[test]
fn generic_trait_impl_dispatches_after_substituting_impl_parameters() {
    let result = run_ok(
        r#"
        trait Echo<T> {
            fn echo(self, value: T) -> T;
        }
        struct Box<U> { value: U }
        impl<T> Echo<T> for Box<T> {
            fn echo(self, value: T) -> T { value }
        }
        let value = Box::<int> { value: 1 }
        value.echo(7)
        "#,
    );
    assert_eq!(result.as_int(), Some(7));
}

#[test]
fn generic_function_checks_applied_trait_arguments_in_its_bound() {
    let error = run(
        r#"
        trait Echo<T> {
            fn echo(self, value: T) -> T;
        }
        struct Box<U> { value: U }
        impl Echo<int> for Box<string> {
            fn echo(self, value: int) -> int { value }
        }
        fn accept<T: Echo<string>>(value: T) -> T { value }
        accept(Box::<string> { value: "text" })
        "#,
        "stage2_applied_trait_bound.aelys",
    )
    .expect_err("a trait bound with the wrong applied arguments must be rejected")
    .to_string();
    assert!(error.contains("error[E0338]"), "expected E0338: {error}");
    assert!(error.contains("Echo"), "expected the bound name: {error}");
}

#[test]
fn generic_function_rejects_an_unsatisfied_trait_bound() {
    let error = run(
        r#"
trait Scorable {
    fn score(self) -> int;
}
fn identity<T: Scorable>(value: T) -> T {
    value
}
identity(9)
"#,
        "stage2_trait_bound.aelys",
    )
    .expect_err("a generic call with no matching impl must be rejected")
    .to_string();
    assert!(error.contains("error[E0338]"), "expected E0338: {error}");
    assert!(error.contains("Scorable"), "expected bound name: {error}");
}

#[test]
fn generic_function_rejects_dynamic_argument_for_trait_bound() {
    let error = run(
        r#"
trait Scorable {
    fn score(self) -> int;
}
fn identity<T: Scorable>(value: T) -> T {
    value
}
let value: dynamic = 9
identity::<dynamic>(value)
"#,
        "stage2_dynamic_trait_bound.aelys",
    )
    .expect_err("a dynamic argument must not satisfy a generic trait bound");
    let error = error.to_string();
    assert!(error.contains("error[E0347]"), "expected E0347: {error}");
    assert!(
        error.contains("dynamic is not part of Aelys"),
        "expected the surface diagnostic: {error}"
    );
}

#[test]
fn explicit_dynamic_type_is_rejected_at_the_surface_boundary() {
    let error = run(
        "let value: dynamic = 1\nvalue",
        "stage2_dynamic_surface.aelys",
    )
    .expect_err("dynamic must not be a surface type")
    .to_string();
    assert!(
        error.contains("dynamic is not part of Aelys"),
        "expected the named dynamic surface diagnostic: {error}"
    );
}

#[test]
fn generic_function_value_without_instantiation_is_rejected() {
    let error = run(
        r#"
fn id<T>(value: T) -> T {
    value
}
let function = id
function(7)
"#,
        "stage2_open_generic_value.aelys",
    )
    .expect_err("an open generic function value must not reach the backend");
    let error = error.to_string();
    assert!(error.contains("error[E0343]"), "expected E0343: {error}");
    assert!(
        error.contains("generic parameter 'T'"),
        "expected the unresolved parameter to be named: {error}"
    );
    assert!(
        error.contains("add a type argument"),
        "expected the repair: {error}"
    );
}

#[test]
fn test_int_subtraction_specialized() {
    let result = run_ok(
        r#"
        let a: int = 500
        let b: int = 123
        a - b
    "#,
    );
    assert_eq!(result.as_int(), Some(377));
}

#[test]
fn test_int_multiplication_specialized() {
    let result = run_ok(
        r#"
        let a: int = 12
        let b: int = 11
        a * b
    "#,
    );
    assert_eq!(result.as_int(), Some(132));
}

#[test]
fn test_int_division_specialized() {
    let result = run_ok(
        r#"
        let a: int = 100
        let b: int = 4
        a / b
    "#,
    );
    assert_eq!(result.as_int(), Some(25));
}

#[test]
fn test_int_modulo_specialized() {
    let result = run_ok(
        r#"
        let a: int = 17
        let b: int = 5
        a % b
    "#,
    );
    assert_eq!(result.as_int(), Some(2));
}

#[test]
fn test_int_comparison_lt_specialized() {
    let result = run_ok(
        r#"
        let a: int = 10
        let b: int = 20
        a < b
    "#,
    );
    assert_eq!(result.as_bool(), Some(true));
}

#[test]
fn test_int_comparison_le_specialized() {
    let result = run_ok(
        r#"
        let a: int = 10
        let b: int = 10
        a <= b
    "#,
    );
    assert_eq!(result.as_bool(), Some(true));
}

#[test]
fn test_int_comparison_gt_specialized() {
    let result = run_ok(
        r#"
        let a: int = 30
        let b: int = 20
        a > b
    "#,
    );
    assert_eq!(result.as_bool(), Some(true));
}

#[test]
fn test_int_comparison_ge_specialized() {
    let result = run_ok(
        r#"
        let a: int = 20
        let b: int = 20
        a >= b
    "#,
    );
    assert_eq!(result.as_bool(), Some(true));
}

#[test]
fn test_int_equality_specialized() {
    let result = run_ok(
        r#"
        let a: int = 42
        let b: int = 42
        a == b
    "#,
    );
    assert_eq!(result.as_bool(), Some(true));
}

#[test]
fn test_int_not_equal_specialized() {
    let result = run_ok(
        r#"
        let a: int = 42
        let b: int = 43
        a != b
    "#,
    );
    assert_eq!(result.as_bool(), Some(true));
}

#[test]
fn test_float_addition_specialized() {
    let result = run_ok(
        r#"
        let a: float = 1.5
        let b: float = 2.5
        a + b
    "#,
    );
    assert!((result.as_float().unwrap() - 4.0).abs() < 0.001);
}

#[test]
fn test_float_subtraction_specialized() {
    let result = run_ok(
        r#"
        let a: float = 10.0
        let b: float = 3.5
        a - b
    "#,
    );
    assert!((result.as_float().unwrap() - 6.5).abs() < 0.001);
}

#[test]
fn test_float_multiplication_specialized() {
    let result = run_ok(
        r#"
        let a: float = 2.5
        let b: float = 4.0
        a * b
    "#,
    );
    assert!((result.as_float().unwrap() - 10.0).abs() < 0.001);
}

#[test]
fn test_float_division_specialized() {
    let result = run_ok(
        r#"
        let a: float = 15.0
        let b: float = 3.0
        a / b
    "#,
    );
    assert!((result.as_float().unwrap() - 5.0).abs() < 0.001);
}

#[test]
fn test_int_float_mixed_arithmetic() {
    let result = run_ok(
        r#"
        let a: int = 5
        let b: float = 2.5
        a + b
    "#,
    );
    assert!((result.as_float().unwrap() - 7.5).abs() < 0.001);
}

#[test]
fn test_inferred_types_in_loop() {
    let result = run_ok(
        r#"
        let mut sum = 0
        let mut i = 0
        while i < 10 {
            sum += i
            i++
        }
        sum
    "#,
    );
    assert_eq!(result.as_int(), Some(45));
}

#[test]
fn test_typed_loop_counter() {
    let result = run_ok(
        r#"
        let mut sum: int = 0
        let mut i: int = 0
        while i < 5 {
            sum += i * i
            i++
        }
        sum
    "#,
    );
    assert_eq!(result.as_int(), Some(30));
}

#[test]
fn test_for_loop_with_typed_bounds() {
    let result = run_ok(
        r#"
        let mut sum: int = 0
        for i in 1..6 {
            sum += i
        }
        sum
    "#,
    );
    assert_eq!(result.as_int(), Some(15));
}

#[test]
fn test_nested_function_with_types() {
    let result = run_ok(
        r#"
        fn outer(x: int) -> int {
            fn inner(y: int) -> int {
                return y * 2
            }
            return inner(x) + 1
        }
        outer(10)
    "#,
    );
    assert_eq!(result.as_int(), Some(21));
}

#[test]
fn test_closure_with_typed_capture() {
    let result = run_ok(
        r#"
        fn make_adder(x: int) {
            return fn(y: int) -> int {
                return x + y
            }
        }
        let add10 = make_adder(10)
        add10(5)
    "#,
    );
    assert_eq!(result.as_int(), Some(15));
}

#[test]
fn test_recursive_function_with_types() {
    let result = run_ok(
        r#"
        fn factorial(n: int) -> int {
            if n <= 1 {
                return 1
            }
            return n * factorial(n - 1)
        }
        factorial(5)
    "#,
    );
    assert_eq!(result.as_int(), Some(120));
}

#[test]
fn test_multiple_typed_functions() {
    let result = run_ok(
        r#"
        fn square(x: int) -> int {
            return x * x
        }

        fn cube(x: int) -> int {
            return x * square(x)
        }

        cube(3)
    "#,
    );
    assert_eq!(result.as_int(), Some(27));
}

#[test]
fn test_various_int_types() {
    let result = run_ok(
        r#"
        let a: int = 10
        let b: int = 20
        let c: int = 30
        let d: int64 = 40
        a + b + c + d
    "#,
    );
    assert_eq!(result.as_int(), Some(100));
}

#[test]
fn test_various_uint_types() {
    let result = run_ok(
        r#"
        let a: int = 10
        let b: int = 20
        a + b
    "#,
    );
    assert_eq!(result.as_int(), Some(30));
}

#[test]
fn test_various_float_types() {
    let result = run_ok(
        r#"
        let a: float = 1.5
        let b: float64 = 2.5
        a + b
    "#,
    );
    assert!((result.as_float().unwrap() - 4.0).abs() < 0.001);
}

#[test]
fn test_function_without_type_annotations() {
    let result = run_ok(
        r#"
        fn add(a, b) {
            return a + b
        }
        add(10, 20)
    "#,
    );
    assert_eq!(result.as_int(), Some(30));
}

#[test]
fn test_let_without_type_annotation() {
    let result = run_ok(
        r#"
        let x = 42
        let y = x + 8
        y
    "#,
    );
    assert_eq!(result.as_int(), Some(50));
}

#[test]
fn test_lambda_without_type_annotations() {
    let result = run_ok(
        r#"
        let double = fn(x) { return x * 2 }
        double(25)
    "#,
    );
    assert_eq!(result.as_int(), Some(50));
}

fn run_ok_string(source: &str, expected: &str) {
    use aelys::{new_vm, run_with_vm};
    let mut vm = new_vm().expect("Failed to create VM");
    let result =
        run_with_vm(&mut vm, source, "test.aelys").expect("Expected program to run successfully");
    let ptr = result
        .as_ptr()
        .unwrap_or_else(|| panic!("expected a heap string result, got {result:?}"));
    let heap = vm.heap();
    let object = heap
        .get(aelys_runtime::vm::GcRef::new(ptr))
        .expect("expected a live heap object");
    let aelys_runtime::vm::ObjectKind::String(text) = &object.kind else {
        panic!("expected a string result, got {:?}", object.kind);
    };
    assert_eq!(text.as_str(), expected);
}

#[test]
fn generic_where_bound_dispatches_a_trait_method() {
    run_ok_string(
        r#"
trait Show { fn show(self) -> string; }
struct Plain { v: int }
impl Show for Plain { fn show(self) -> string { "plain" } }
fn render<T>(x: T) -> string where T: Show { x.show() }
fn probe() -> string { render(Plain { v: 1 }) }
probe()
"#,
        "plain",
    );
}

#[test]
fn generic_inline_bound_dispatches_a_trait_method() {
    let result = run_ok(
        r#"
trait Scorable { fn score(self) -> int; }
struct Point { v: int }
impl Scorable for Point { fn score(self) -> int { 41 } }
fn total<T: Scorable>(value: T) -> int { value.score() }
fn probe() -> int { total(Point { v: 1 }) }
probe()
"#,
    );
    assert_eq!(result.as_int(), Some(41));
}

#[test]
fn generic_bound_dispatches_a_trait_default_method() {
    let result = run_ok(
        r#"
trait Scorable {
    fn base(self) -> int;
    fn score(self) -> int { self.base() + 1 }
}
struct Point { v: int }
impl Scorable for Point { fn base(self) -> int { 6 } }
fn total<T: Scorable>(value: T) -> int { value.score() }
fn probe() -> int { total(Point { v: 1 }) }
probe()
"#,
    );
    assert_eq!(result.as_int(), Some(7));
}

#[test]
fn generic_bound_selects_the_impl_of_each_concrete_type() {
    let result = run_ok(
        r#"
trait Scorable { fn score(self) -> int; }
struct Alpha { v: int }
struct Beta { v: int }
impl Scorable for Alpha { fn score(self) -> int { 10 } }
impl Scorable for Beta { fn score(self) -> int { 20 } }
fn total<T: Scorable>(value: T) -> int { value.score() }
fn probe() -> int { total(Alpha { v: 1 }) * 100 + total(Beta { v: 1 }) }
probe()
"#,
    );
    assert_eq!(result.as_int(), Some(1020));
}

#[test]
fn generic_impl_method_calls_a_sibling_method() {
    let result = run_ok(
        r#"
struct Box<T> { v: T }
impl<T> Box<T> {
    fn get(self) -> T { self.v }
    fn twice(self) -> T { self.get() }
}
fn probe() -> int { Box { v: 7 }.twice() }
probe()
"#,
    );
    assert_eq!(result.as_int(), Some(7));
}

#[test]
fn turbofish_binds_type_arguments_in_declaration_order() {
    let result = run_ok(
        r#"
fn swap<A, B>(b: B, a: A) -> A { a }
fn probe() -> int {
    let r: int = swap::<int, string>("text", 7)
    r
}
probe()
"#,
    );
    assert_eq!(result.as_int(), Some(7));
}

#[test]
fn turbofish_declaration_order_selects_the_observable_impl() {
    let result = run_ok(
        r#"
trait Scorable { fn score(self) -> int; }
struct Alpha { v: int }
struct Beta { v: int }
impl Scorable for Alpha { fn score(self) -> int { 10 } }
impl Scorable for Beta { fn score(self) -> int { 20 } }
fn weigh<A, B>(b: B, a: A) -> int where A: Scorable, B: Scorable {
    a.score() * 100 + b.score()
}
fn probe() -> int { weigh::<Alpha, Beta>(Beta { v: 0 }, Alpha { v: 0 }) }
probe()
"#,
    );
    assert_eq!(result.as_int(), Some(1020));
}

#[test]
fn a_generic_arity_mismatch_has_a_named_compile_error() {
    let error = run(
        "fn probe(value: Result<int>) -> int { 0 }\nprobe(Ok(1))",
        "arity.aelys",
    )
    .expect_err("a generic type used with the wrong parameter count must be rejected")
    .to_string();
    assert!(
        error.contains("error[E0357]") && error.contains("expects 2 parameter(s), found 1"),
        "expected the named generic arity diagnostic, got: {error}"
    );
}

#[test]
fn an_or_pattern_binding_mismatch_has_a_named_compile_error() {
    let error = run(
        r#"
        enum Color { Red(int), Blue(int) }
        fn score(color: Color) -> int {
            match color {
                Color::Red(value) | Color::Blue(other) => 0,
            }
        }
        score(Color::Red(1))
        "#,
        "or_pattern.aelys",
    )
    .expect_err("or-pattern alternatives that bind different names must be rejected")
    .to_string();
    assert!(
        error.contains("error[E0358]") && error.contains("must bind the same names"),
        "expected the named or-pattern binding diagnostic, got: {error}"
    );
}
