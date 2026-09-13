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

fn emitted_trait_symbols(source: &str) -> Vec<String> {
    let module = aelys::Runtime::new()
        .compile(source, aelys::CompileOptions::default())
        .expect("the program must compile");
    let function = aelys_bytecode::asm::deserialize(module.avbc())
        .expect("the compiled module must deserialize");
    let mut names = Vec::new();
    fn walk(function: &aelys_bytecode::Function, names: &mut Vec<String>) {
        if let Some(name) = &function.name
            && name.starts_with("__aelys_trait::")
        {
            names.push(name.clone());
        }
        for nested in &function.nested_functions {
            walk(nested, names);
        }
    }
    walk(&function, &mut names);
    names.sort();
    names.dedup();
    names
}

#[test]
fn an_impl_symbol_mangles_the_trait_the_header_spelling_and_the_method() {
    // one compilation cannot show that the spelling is stable; a review measured
    for _ in 0..8 {
        let repeat = emitted_trait_symbols(
            "trait Source {\n    fn next(self) -> int\n}\n\
struct Wrap<T> {  v: T }\n\
impl Source for Wrap<int> {\n    fn next(self) -> int {\n        return 1\n    }\n}\n\
fn go() -> int {\n    let a = Wrap { v: 5 }\n    return a.next()\n}\ngo()\n",
        );
        assert_eq!(
            repeat,
            ["__aelys_trait::00000006:Source00000009:Wrap<int>00000004:next"],
            "one unchanged program mangled two ways"
        );
    }

    let plain = emitted_trait_symbols(
        "trait Source {\n    fn next(self) -> int\n}\n\
struct Wrap<T> {  v: T }\n\
impl Source for Wrap<int> {\n    fn next(self) -> int {\n        return 1\n    }\n}\n\
fn go() -> int {\n    let a = Wrap { v: 5 }\n    return a.next()\n}\ngo()\n",
    );
    assert_eq!(
        plain,
        ["__aelys_trait::00000006:Source00000009:Wrap<int>00000004:next"],
        "the header's own spelling enters the symbol, so two headers never collide"
    );

    let generic = emitted_trait_symbols(
        "trait Source {\n    fn next(self) -> int\n}\n\
struct Wrap<T> { v: T }\n\
impl<T> Source for Wrap<T> {\n    fn next(self) -> int {\n        return 1\n    }\n}\n\
fn go() -> int {\n    let a = Wrap { v: 5 }\n    let b = Wrap { v: true }\n    return a.next() + b.next()\n}\ngo()\n",
    );
    assert_eq!(
        generic.len(),
        2,
        "a generic impl carries one suffix per instance: {generic:?}"
    );
    for symbol in &generic {
        let (head, encoded) = symbol
            .split_once("$s2$")
            .unwrap_or_else(|| panic!("no instance suffix on '{symbol}'"));
        assert_eq!(
            head, "__aelys_trait::00000006:Source00000008:Wrap<$0>00000004:next",
            "every instance of one header shares that header's spelling"
        );
        let decoded: String = String::from_utf8_lossy(
            &encoded
                .as_bytes()
                .chunks(2)
                .filter_map(|pair| std::str::from_utf8(pair).ok())
                .filter_map(|pair| u8::from_str_radix(pair, 16).ok())
                .collect::<Vec<u8>>(),
        )
        .into_owned();
        assert!(
            decoded.contains("Wrap"),
            "the suffix holds the substituted target type: {decoded}"
        );
    }
    assert_ne!(
        generic[0], generic[1],
        "two instances of a generic impl are two symbols"
    );
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
fn trait_self_is_usable_in_method_signatures() {
    let result = run_ok(
        r#"
        trait Identity {
            fn identity(self) -> Self;
        }
        struct Boxed { value: int }
        impl Identity for Boxed {
            fn identity(self) -> Self { self }
        }
        let boxed = Boxed { value: 7 }
        boxed.identity().value
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
        error.contains("expected int") && error.contains("found string"),
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
fn generic_impl_instantiation_hits_the_named_limit() {
    let error = run(
        r#"
        struct Box<T> { value: T }
        impl<T> Box<T> {
            fn recurse(self) -> int {
                let next = Box { value: self }
                next.recurse()
            }
        }
        Box { value: 0 }.recurse()
        "#,
        "stage2_impl_monomorphization_limit.aelys",
    )
    .expect_err("recursive generic impl growth must be rejected")
    .to_string();
    assert!(
        error.contains("monomorphization") && error.contains("limit"),
        "expected the named monomorphization limit diagnostic, got: {error}"
    );
}

#[test]
fn deferred_enum_member_resolves_to_its_method() {
    let result = run_ok(
        r#"
        enum Flag { On, Off }
        impl Flag {
            fn value(self) -> int {
                match self {
                    Flag::On => 1,
                    Flag::Off => 0
                }
            }
        }
        fn identity<T>(value: T) -> T { value }
        fn takes_flag(value: Flag) -> int { 1 }
        let flag = identity(Flag::On)
        takes_flag(flag)
        flag.value()
        "#,
    );
    assert_eq!(result.as_int(), Some(1));
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

#[test]
fn a_declared_return_type_is_the_expected_side_of_the_mismatch() {
    let error = run(
        r#"
        struct Pt { x: int }
        fn probe() -> Pt {
            return 5
        }
        probe()
        "#,
        "expected_found_return.aelys",
    )
    .expect_err("a return value that is not the declared type must be rejected")
    .to_string();
    assert!(
        error.contains("expected Pt, found int"),
        "the declared return type is the expected side, got: {error}"
    );
}

#[test]
fn a_let_annotation_is_the_expected_side_of_the_mismatch() {
    let error = run(
        r#"
        let v: int = "hello"
        v
        "#,
        "expected_found_let.aelys",
    )
    .expect_err("an initializer that is not the annotated type must be rejected")
    .to_string();
    assert!(
        error.contains("expected int, found string"),
        "the annotation is the expected side, got: {error}"
    );
}

#[test]
fn a_lambda_return_annotation_is_the_expected_side_of_the_mismatch() {
    let error = run(
        r#"
        let f = fn(x: int) -> string { return x }
        f(1)
        "#,
        "expected_found_lambda.aelys",
    )
    .expect_err("a lambda return that is not the declared type must be rejected")
    .to_string();
    assert!(
        error.contains("expected string, found int"),
        "the declared lambda return type is the expected side, got: {error}"
    );
}

#[test]
fn an_implicit_tail_return_reports_the_annotation_as_expected() {
    let error = run(
        r#"
        fn probe() -> int {
            "hello"
        }
        probe()
        "#,
        "expected_found_tail.aelys",
    )
    .expect_err("a tail value that is not the declared return type must be rejected")
    .to_string();
    assert!(
        error.contains("expected int, found string"),
        "the declared return type is the expected side, got: {error}"
    );
}

#[test]
fn a_call_argument_keeps_the_parameter_as_the_expected_side() {
    let error = run(
        r#"
        fn need_int(x: int) {}
        need_int("hello")
        0
        "#,
        "expected_found_argument.aelys",
    )
    .expect_err("an argument that is not the parameter type must be rejected")
    .to_string();
    assert!(
        error.contains("expected int, found string"),
        "the declared parameter type is the expected side, got: {error}"
    );
}

#[test]
fn an_array_element_mismatch_reports_the_established_element_type_as_expected() {
    let array = run_err(
        r#"
        fn probe() -> int {
            let xs = [1, "two", 3]
            return 0
        }
        probe()
        "#,
    );
    assert!(
        array.contains("expected int, found string"),
        "the established element type is the expected side, got: {array}"
    );

    let vector = run_err(
        r#"
        fn probe() -> int {
            let v = vec![1, "two"]
            return 0
        }
        probe()
        "#,
    );
    assert!(
        vector.contains("expected int, found string"),
        "the established element type is the expected side, got: {vector}"
    );
}

#[test]
fn an_index_assignment_reports_the_container_element_type_as_expected() {
    let array = run_err(
        r#"
        fn probe() -> int {
            let mut xs = [1, 2, 3]
            xs[0] = "no"
            return 0
        }
        probe()
        "#,
    );
    assert!(
        array.contains("expected int, found string"),
        "the container element type is the expected side, got: {array}"
    );

    let vector = run_err(
        r#"
        fn probe() -> int {
            let mut v = vec![1, 2]
            v[0] = "no"
            return 0
        }
        probe()
        "#,
    );
    assert!(
        vector.contains("expected int, found string"),
        "the container element type is the expected side, got: {vector}"
    );
}

#[test]
fn an_index_position_reports_the_required_integer_as_expected() {
    let read = run_err(
        r#"
        fn probe() -> int {
            let xs = [1, 2, 3]
            return xs["a"]
        }
        probe()
        "#,
    );
    assert!(
        read.contains("expected int, found string"),
        "the required index type is the expected side, got: {read}"
    );

    let write = run_err(
        r#"
        fn probe() -> int {
            let mut v = vec![1, 2]
            v["a"] = 3
            return 0
        }
        probe()
        "#,
    );
    assert!(
        write.contains("expected int, found string"),
        "the required index type is the expected side, got: {write}"
    );

    let repeat = run_err(
        r#"
        fn probe() -> int {
            let v = vec![0; "3"]
            return 0
        }
        probe()
        "#,
    );
    assert!(
        repeat.contains("expected int, found string"),
        "the required repeat count type is the expected side, got: {repeat}"
    );
}

#[test]
fn a_range_bound_reports_the_required_integer_as_expected() {
    let start = run_err(
        r#"
        fn probe() -> int {
            let r = "a"..3
            return 0
        }
        probe()
        "#,
    );
    assert!(
        start.contains("expected int, found string"),
        "the required bound type is the expected side, got: {start}"
    );

    let end = run_err(
        r#"
        fn probe() -> int {
            let r = 0.."b"
            return 0
        }
        probe()
        "#,
    );
    assert!(
        end.contains("expected int, found string"),
        "the required bound type is the expected side, got: {end}"
    );
}

#[test]
fn a_for_loop_header_reports_the_required_integer_as_expected() {
    let start = run_err(
        r#"
        fn probe() -> int {
            for i in "a"..3 { return 1 }
            return 0
        }
        probe()
        "#,
    );
    assert!(
        start.contains("expected int, found string"),
        "the required bound type is the expected side, got: {start}"
    );

    let end = run_err(
        r#"
        fn probe() -> int {
            for i in 0.."b" { return 1 }
            return 0
        }
        probe()
        "#,
    );
    assert!(
        end.contains("expected int, found string"),
        "the required bound type is the expected side, got: {end}"
    );

    let step = run_err(
        r#"
        fn probe() -> int {
            let mut t = 0
            for i in 0..10 step "x" { t = t + i }
            return t
        }
        probe()
        "#,
    );
    assert!(
        step.contains("expected int, found string"),
        "the required step type is the expected side, got: {step}"
    );
}

#[test]
fn a_condition_reports_the_required_boolean_as_expected() {
    let if_statement = run_err(
        r#"
        fn probe() -> int {
            if 5 { return 1 }
            return 0
        }
        probe()
        "#,
    );
    assert!(
        if_statement.contains("expected bool, found int"),
        "the required condition type is the expected side, got: {if_statement}"
    );

    let if_expression = run_err(
        r#"
        fn probe() -> int {
            let v = if 5 { 1 } else { 2 }
            return v
        }
        probe()
        "#,
    );
    assert!(
        if_expression.contains("expected bool, found int"),
        "the required condition type is the expected side, got: {if_expression}"
    );

    let tail_if = run_err(
        r#"
        fn probe() -> int {
            if 5 { 1 } else { 2 }
        }
        probe()
        "#,
    );
    assert!(
        tail_if.contains("expected bool, found int"),
        "the required condition type is the expected side, got: {tail_if}"
    );

    let while_loop = run_err(
        r#"
        fn probe() -> int {
            while 5 { return 1 }
            return 0
        }
        probe()
        "#,
    );
    assert!(
        while_loop.contains("expected bool, found int"),
        "the required condition type is the expected side, got: {while_loop}"
    );

    let match_guard = run_err(
        r#"
        fn probe() -> int {
            let n = 2
            let v = match n {
                1 if 7 => 10,
                _ => 20,
            }
            return v
        }
        probe()
        "#,
    );
    assert!(
        match_guard.contains("expected bool, found int"),
        "the required guard type is the expected side, got: {match_guard}"
    );

    let negation = run_err(
        r#"
        fn probe() -> int {
            let b = not 5
            return 0
        }
        probe()
        "#,
    );
    assert!(
        negation.contains("expected bool, found int"),
        "the required operand type is the expected side, got: {negation}"
    );
}

#[test]
fn a_logical_operand_reports_the_required_boolean_as_expected() {
    let left = run_err(
        r#"
        fn probe() -> int {
            let b = 5 && true
            return 0
        }
        probe()
        "#,
    );
    assert!(
        left.contains("expected bool, found int"),
        "the required operand type is the expected side, got: {left}"
    );

    let right = run_err(
        r#"
        fn probe() -> int {
            let b = true || 5
            return 0
        }
        probe()
        "#,
    );
    assert!(
        right.contains("expected bool, found int"),
        "the required operand type is the expected side, got: {right}"
    );
}

#[test]
fn an_if_else_result_reports_the_then_branch_type_as_expected() {
    let error = run_err(
        r#"
        fn probe() -> int {
            let c = true
            let v = if c { 1 } else { "two" }
            return 0
        }
        probe()
        "#,
    );
    assert!(
        error.contains("expected int, found string"),
        "the branch type settled first is the expected side, got: {error}"
    );
}

#[test]
fn a_match_result_reports_the_first_arm_type_as_expected() {
    let expression_arms = run_err(
        r#"
        fn probe() -> int {
            let n = 2
            let v = match n {
                1 => 10,
                _ => "twenty",
            }
            return 0
        }
        probe()
        "#,
    );
    assert!(
        expression_arms.contains("expected int, found string"),
        "the arm type settled first is the expected side, got: {expression_arms}"
    );

    let block_arms = run_err(
        r#"
        fn probe() -> int {
            let n = 2
            let v = match n {
                1 => { 10 }
                _ => { "twenty" }
            }
            return 0
        }
        probe()
        "#,
    );
    assert!(
        block_arms.contains("expected int, found string"),
        "the arm type settled first is the expected side, got: {block_arms}"
    );
}

#[test]
fn a_declared_field_type_is_the_expected_side_of_the_mismatch() {
    let struct_literal = run_err(
        r#"
        struct Pt { x: int }
        fn probe() -> int {
            let p = Pt { x: "no" }
            return 0
        }
        probe()
        "#,
    );
    assert!(
        struct_literal.contains("expected int, found string"),
        "the declared field type is the expected side, got: {struct_literal}"
    );

    let field_assignment = run_err(
        r#"
        struct Pt { x: int }
        fn probe() -> int {
            let mut p = Pt { x: 1 }
            p.x = "no"
            return 0
        }
        probe()
        "#,
    );
    assert!(
        field_assignment.contains("expected int, found string"),
        "the declared field type is the expected side, got: {field_assignment}"
    );

    let enum_literal = run_err(
        r#"
        enum Shape { Point { x: int } }
        fn probe() -> int {
            let s = Shape::Point { x: "no" }
            return 0
        }
        probe()
        "#,
    );
    assert!(
        enum_literal.contains("expected int, found string"),
        "the declared field type is the expected side, got: {enum_literal}"
    );

    let enum_constructor = run_err(
        r#"
        enum E { A(int) }
        fn probe() -> int {
            let e = E::A("no")
            return 0
        }
        probe()
        "#,
    );
    assert!(
        enum_constructor.contains("expected int, found string"),
        "the declared field type is the expected side, got: {enum_constructor}"
    );
}

#[test]
fn an_assignment_reports_the_variable_type_as_expected() {
    let error = run_err(
        r#"
        fn probe() -> int {
            let mut v = 1
            v = "no"
            return 0
        }
        probe()
        "#,
    );
    assert!(
        error.contains("expected int, found string"),
        "the variable type is the expected side, got: {error}"
    );
}

#[test]
fn a_builtin_parameter_type_is_the_expected_side_of_the_mismatch() {
    let string_builtin = run_err(
        r#"
        fn probe() -> int {
            let s = "abc".repeat("x")
            return 0
        }
        probe()
        "#,
    );
    assert!(
        string_builtin.contains("expected int, found string"),
        "the declared parameter type is the expected side, got: {string_builtin}"
    );

    let collection_builtin = run_err(
        r#"
        fn probe() -> int {
            let mut v = vec![1, 2]
            v.push("no")
            return 0
        }
        probe()
        "#,
    );
    assert!(
        collection_builtin.contains("expected int, found string"),
        "the declared parameter type is the expected side, got: {collection_builtin}"
    );

    let error_builtin = run_err(
        r#"
        fn probe() -> int {
            let e = Error::Message(5)
            return 0
        }
        probe()
        "#,
    );
    assert!(
        error_builtin.contains("expected string, found int"),
        "the declared parameter type is the expected side, got: {error_builtin}"
    );
}

#[test]
fn a_sum_method_parameter_type_is_the_expected_side_of_the_mismatch() {
    let resolved_receiver = run_err(
        r#"
        fn probe() -> int {
            let o = Some(1)
            return o.unwrap_or("x")
        }
        probe()
        "#,
    );
    assert!(
        resolved_receiver.contains("expected int, found string"),
        "the sum method parameter type is the expected side, got: {resolved_receiver}"
    );

    // the receiver must start out unresolved so the call takes the inferred-sum path
    let inferred_receiver = run_err(
        r#"
        fn probe() -> int {
            let mut o = None
            o = Some(1)
            return o.unwrap_or("x")
        }
        probe()
        "#,
    );
    assert!(
        inferred_receiver.contains("expected int, found string"),
        "the sum method parameter type is the expected side, got: {inferred_receiver}"
    );
}

#[test]
fn a_qualified_trait_call_keeps_the_parameter_as_the_expected_side() {
    let error = run_err(
        r#"
        struct P { v: int }
        trait Scale { fn scale(self, n: int) -> int; }
        impl Scale for P { fn scale(self, n: int) -> int { self.v * n } }
        let p = P { v: 2 }
        Scale::scale(p, "no")
        "#,
    );
    assert!(
        error.contains("expected int, found string"),
        "the declared parameter type is the expected side, got: {error}"
    );
}

#[test]
fn a_struct_field_can_name_a_struct_declared_later() {
    let result = run_ok(
        r#"
        struct A { b: B }
        struct B { v: int }
        fn probe() -> int {
            let a = A { b: B { v: 3 } }
            return a.b.v
        }
        probe()
        "#,
    );
    assert_eq!(result.as_int(), Some(3));
}

#[test]
fn a_struct_is_accepted_as_an_enum_tuple_payload_in_either_order() {
    let struct_first = run_ok(
        r#"
        struct Counter { v: int }
        enum Box4 { Full(Counter), Empty }
        fn score(b: Box4) -> int {
            match b {
                Box4::Full(c) => c.v,
                Box4::Empty => 0,
            }
        }
        score(Box4::Full(Counter { v: 5 }))
        "#,
    );
    assert_eq!(struct_first.as_int(), Some(5));

    let enum_first = run_ok(
        r#"
        enum Box4 { Full(Counter), Empty }
        struct Counter { v: int }
        fn score(b: Box4) -> int {
            match b {
                Box4::Full(c) => c.v,
                Box4::Empty => 0,
            }
        }
        score(Box4::Full(Counter { v: 5 }))
        "#,
    );
    assert_eq!(enum_first.as_int(), Some(5));
}

#[test]
fn a_struct_is_accepted_as_an_enum_record_payload() {
    let result = run_ok(
        r#"
        enum Box5 { Full { inner: Counter }, Empty }
        struct Counter { v: int }
        fn score(b: Box5) -> int {
            match b {
                Box5::Full { inner } => inner.v,
                Box5::Empty => 0,
            }
        }
        score(Box5::Full { inner: Counter { v: 6 } })
        "#,
    );
    assert_eq!(result.as_int(), Some(6));
}

#[test]
fn an_enum_payload_array_length_reads_an_associated_constant() {
    let result = run_ok(
        r#"
        struct Counter { v: int }
        trait Bounds { const LIMIT: int; }
        impl Bounds for Counter { const LIMIT: int = 3; }
        enum Box2 { Full([int; Counter::LIMIT]), Empty }
        fn size(b: Box2) -> int {
            match b {
                Box2::Full(xs) => xs[0],
                Box2::Empty => 0,
            }
        }
        size(Box2::Full([7, 8, 9]))
        "#,
    );
    assert_eq!(result.as_int(), Some(7));
}

#[test]
fn a_struct_field_can_name_a_trait_qualified_associated_type() {
    let result = run_ok(
        r#"
        struct Counter { v: int }
        trait Source { type Item; fn get(self) -> Self::Item; }
        impl Source for Counter { type Item = int; fn get(self) -> int { self.v } }
        struct Holder { item: Source::Item }
        fn probe(h: Holder) -> int { h.item }
        probe(Holder { item: 7 })
        "#,
    );
    assert_eq!(result.as_int(), Some(7));
}

#[test]
fn the_nominal_type_path_answers_the_same_whatever_the_file_order() {
    // byte identical and any difference is file order leaking in.
    let alpha_first = r#"struct Counter { v: int }
trait Alpha { type Item; }
trait Beta { type Item; }
impl Alpha for Counter { type Item = int; }
fn ident(item: Counter::Item) -> Counter::Item { item }
impl Beta for Counter { type Item = int; }
ident(7)"#;
    let beta_first = r#"struct Counter { v: int }
trait Alpha { type Item; }
trait Beta { type Item; }
impl Beta for Counter { type Item = int; }
fn ident(item: Counter::Item) -> Counter::Item { item }
impl Alpha for Counter { type Item = int; }
ident(7)"#;

    let mut sorted_first: Vec<&str> = alpha_first.lines().collect();
    let mut sorted_second: Vec<&str> = beta_first.lines().collect();
    sorted_first.sort_unstable();
    sorted_second.sort_unstable();
    assert_eq!(
        sorted_first, sorted_second,
        "the two sources must differ only in line order"
    );

    let expected = concat!(
        "error[E0423]: projection 'Counter::Item' is ambiguous: Alpha and Beta both define 'Item'",
        " for Counter; name the trait that declares the one you mean, as in 'Alpha::Item',",
        " or remove one of the competing impls, or rename the item so a single trait",
        " provides it (a parameter type)\n",
        "  --> test.aelys:5:16\n",
        "   |\n",
        " 5 | fn ident(item: Counter::Item) -> Counter::Item { item }\n",
        "   |                ^^^^^^^ the type checker rejected this program\n",
    );
    let first = run_err(alpha_first);
    let second = run_err(beta_first);
    assert_eq!(
        first, expected,
        "expected the ambiguous projection diagnostic, got: {first}"
    );
    assert_eq!(
        second, first,
        "the nominal type path answered differently for the same lines in a different order:\n{first}\n----\n{second}"
    );
}

#[test]
fn an_impl_internal_projection_answers_the_same_whatever_the_file_order() {
    // be byte identical and any difference is file order leaking in.
    let alpha_first = r#"struct Counter { v: int }
trait Alpha { type Item; }
trait Beta { type Item; }
trait Probe { fn probe(self) -> int; }
impl Alpha for Counter { type Item = int; }
impl Probe for Counter { fn probe(self) -> Counter::Item { self.v } }
impl Beta for Counter { type Item = string; }
Counter { v: 7 }.probe()"#;
    let beta_first = r#"struct Counter { v: int }
trait Alpha { type Item; }
trait Beta { type Item; }
trait Probe { fn probe(self) -> int; }
impl Beta for Counter { type Item = string; }
impl Probe for Counter { fn probe(self) -> Counter::Item { self.v } }
impl Alpha for Counter { type Item = int; }
Counter { v: 7 }.probe()"#;

    let mut sorted_first: Vec<&str> = alpha_first.lines().collect();
    let mut sorted_second: Vec<&str> = beta_first.lines().collect();
    sorted_first.sort_unstable();
    sorted_second.sort_unstable();
    assert_eq!(
        sorted_first, sorted_second,
        "the two sources must differ only in line order"
    );

    let expected = concat!(
        "error[E0423]: projection 'Counter::Item' is ambiguous: Alpha and Beta both define",
        " 'Item' for Counter; name the trait that declares the one you mean, as in",
        " 'Alpha::Item', or remove one of the competing impls, or rename the item so a",
        " single trait provides it (a return type)\n",
        "  --> test.aelys:6:44\n",
        "   |\n",
        " 6 | impl Probe for Counter { fn probe(self) -> Counter::Item { self.v } }\n",
        "   |                                            ^^^^^^^ the type checker rejected this program\n",
    );
    let first = run_err(alpha_first);
    let second = run_err(beta_first);
    assert_eq!(
        first, expected,
        "expected the competing definitions to be rejected, got: {first}"
    );
    assert_eq!(
        second, first,
        "the impl internal projection answered differently for the same lines in a different order:\n{first}\n----\n{second}"
    );
}

#[test]
fn a_struct_that_holds_itself_is_rejected_as_uninhabited() {
    let error = run_err(
        r#"
struct A { a: A }
fn probe() -> int { 1 }
probe()
        "#,
    );
    assert!(
        error.starts_with("error[E0428]"),
        "expected the uninhabited cycle diagnostic, got: {error}"
    );
    assert!(
        error.contains("A -> A"),
        "the diagnostic must name the cycle path, got: {error}"
    );
    assert!(
        error.contains(
            "break the cycle with an Option, a Vec, or an enum with a terminating variant"
        ),
        "the diagnostic must carry a corrective clause, got: {error}"
    );
    assert!(
        error.contains("--> test.aelys:2:12"),
        "the diagnostic must point at the field that closes the cycle, got: {error}"
    );
}

#[test]
fn two_structs_that_hold_each_other_are_rejected_as_uninhabited() {
    let error = run_err(
        r#"
struct A { b: B }
struct B { a: A }
fn probe() -> int { 1 }
probe()
        "#,
    );
    assert!(
        error.starts_with("error[E0428]") && error.contains("A -> B -> A"),
        "expected the mutual uninhabited cycle diagnostic, got: {error}"
    );
    assert!(
        error.contains(
            "break the cycle with an Option, a Vec, or an enum with a terminating variant"
        ),
        "the diagnostic must carry a corrective clause, got: {error}"
    );
    assert!(
        error.contains("--> test.aelys:2:12"),
        "the diagnostic must point at the field that opens the cycle, got: {error}"
    );
}

#[test]
fn a_three_struct_cycle_is_rejected_as_uninhabited() {
    let error = run_err(
        r#"
struct A { b: B }
struct B { c: C }
struct C { a: A }
fn probe() -> int { 1 }
probe()
        "#,
    );
    assert!(
        error.starts_with("error[E0428]") && error.contains("A -> B -> C -> A"),
        "expected the three node uninhabited cycle diagnostic, got: {error}"
    );
    assert!(
        error.contains("--> test.aelys:2:12"),
        "the diagnostic must point at the field that opens the cycle, got: {error}"
    );
}

#[test]
fn a_nominal_cycle_through_a_terminating_shape_is_accepted() {
    // the same generator as the rejected cases, one indirection apart: if these
    let through_option = run_ok(
        r#"
struct A { b: Option<A>, v: int }
fn probe() -> int {
    let a = A { b: None, v: 4 }
    return a.v
}
probe()
        "#,
    );
    assert_eq!(through_option.as_int(), Some(4));

    let through_vec = run_ok(
        r#"
struct A { b: Vec<A>, v: int }
fn probe() -> int {
    let a = A { b: vec![], v: 5 }
    return a.v
}
probe()
        "#,
    );
    assert_eq!(through_vec.as_int(), Some(5));

    let through_enum = run_ok(
        r#"
struct A { e: E, v: int }
enum E { Wrap(A), Nil }
fn probe() -> int {
    let a = A { e: E::Nil, v: 6 }
    return a.v
}
probe()
        "#,
    );
    assert_eq!(through_enum.as_int(), Some(6));
}

// two sources cannot drift apart and any difference in the rendering is file
fn impl_internal_acceptance_source(order: usize) -> String {
    let beta = "impl Beta for Counter { type Item = int; }";
    let blank = "";
    let (before, after) = if order == 0 {
        (beta, blank)
    } else {
        (blank, beta)
    };
    format!(
        "struct Counter {{ v: int }}\ntrait Alpha {{ type Item; }}\ntrait Beta {{ type Item; }}\nimpl Alpha for Counter {{ type Item = int; }}\n{before}\nimpl Counter {{ fn probe(self) -> Counter::Item {{ self.v }} }}\n{after}\nCounter {{ v: 7 }}.probe()"
    )
}

fn impl_internal_meaning_source(order: usize) -> String {
    let alpha = "impl Alpha for Counter { type Item = int; }";
    let beta = "impl Beta for Counter { type Item = string; }";
    let (before, after) = if order == 0 {
        (alpha, beta)
    } else {
        (beta, alpha)
    };
    format!(
        "struct Counter {{ v: int }}\ntrait Alpha {{ type Item; }}\ntrait Beta {{ type Item; }}\n{before}\nimpl Counter {{ fn probe(self) -> Counter::Item {{ self.v }} }}\n{after}\nCounter {{ v: 7 }}.probe()"
    )
}

fn single_definer_source(order: usize) -> String {
    let alpha = "impl Alpha for Counter { type Item = int; }";
    let blank = "";
    let (before, after) = if order == 0 {
        (alpha, blank)
    } else {
        (blank, alpha)
    };
    format!(
        "struct Counter {{ v: int }}\ntrait Alpha {{ type Item; }}\n{before}\nimpl Counter {{ fn probe(self) -> Counter::Item {{ self.v }} }}\n{after}\nCounter {{ v: 7 }}.probe()"
    )
}

fn impl_internal_length_source(order: usize) -> String {
    let alpha = "impl Alpha for Counter { const LIMIT: int = 2; }";
    let beta = "impl Beta for Counter { const LIMIT: int = 3; }";
    let (before, after) = if order == 0 {
        (alpha, beta)
    } else {
        (beta, alpha)
    };
    format!(
        "struct Counter {{ v: int }}\ntrait Alpha {{ const LIMIT: int; }}\ntrait Beta {{ const LIMIT: int; }}\n{before}\nimpl Counter {{ fn probe(self, xs: [int; Counter::LIMIT]) -> int {{ xs[0] }} }}\n{after}\nCounter {{ v: 7 }}.probe([4, 5, 6])"
    )
}

fn associated_definition_source(order: usize) -> String {
    let gamma = "impl Gamma for Counter { type Other = int; }";
    let blank = "";
    let (before, after) = if order == 0 {
        (gamma, blank)
    } else {
        (blank, gamma)
    };
    format!(
        "struct Counter {{ v: int }}\ntrait Alpha {{ type Item; }}\ntrait Beta {{ type Other; }}\ntrait Gamma {{ type Other; }}\nimpl Beta for Counter {{ type Other = int; }}\n{before}\nimpl Alpha for Counter {{ type Item = Counter::Other; }}\n{after}\nfn ident(x: Counter::Item) -> int {{ x }}\nident(7)"
    )
}

fn render(source: &str) -> String {
    match run(source, "test.aelys") {
        Ok(value) => format!("accepted, value {:?}", value.as_int()),
        Err(error) => error.to_string(),
    }
}

fn assert_same_lines(first: &str, second: &str) {
    let mut left: Vec<&str> = first.lines().collect();
    let mut right: Vec<&str> = second.lines().collect();
    left.sort_unstable();
    right.sort_unstable();
    assert_eq!(
        left, right,
        "the two sources must differ only in line order"
    );
}

#[test]
fn an_impl_internal_projection_accepts_or_rejects_the_same_whatever_the_file_order() {
    let first_source = impl_internal_acceptance_source(0);
    let second_source = impl_internal_acceptance_source(1);
    assert_same_lines(&first_source, &second_source);

    let expected = concat!(
        "error[E0423]: projection 'Counter::Item' is ambiguous: Alpha and Beta both define 'Item'",
        " for Counter; name the trait that declares the one you mean, as in 'Alpha::Item', or",
        " remove one of the competing impls, or rename the item so a single trait",
        " provides it (a return type)\n",
        "  --> test.aelys:6:34\n",
        "   |\n",
        " 6 | impl Counter { fn probe(self) -> Counter::Item { self.v } }\n",
        "   |                                  ^^^^^^^ the type checker rejected this program\n",
    );
    let first = render(&first_source);
    let second = render(&second_source);
    assert_eq!(
        first, second,
        "file order decided acceptance:\n{first}\n----\n{second}"
    );
    assert_eq!(first, expected);
}

#[test]
fn an_impl_internal_projection_names_the_same_type_whatever_the_file_order() {
    let first_source = impl_internal_meaning_source(0);
    let second_source = impl_internal_meaning_source(1);
    assert_same_lines(&first_source, &second_source);

    let expected = concat!(
        "error[E0423]: projection 'Counter::Item' is ambiguous: Alpha and Beta both define 'Item'",
        " for Counter; name the trait that declares the one you mean, as in 'Alpha::Item', or",
        " remove one of the competing impls, or rename the item so a single trait",
        " provides it (a return type)\n",
        "  --> test.aelys:5:34\n",
        "   |\n",
        " 5 | impl Counter { fn probe(self) -> Counter::Item { self.v } }\n",
        "   |                                  ^^^^^^^ the type checker rejected this program\n",
    );
    let first = render(&first_source);
    let second = render(&second_source);
    assert_eq!(
        first, second,
        "file order decided the type:\n{first}\n----\n{second}"
    );
    assert_eq!(first, expected);

    assert_eq!(run_ok(&single_definer_source(0)).as_int(), Some(7));
    assert_eq!(run_ok(&single_definer_source(1)).as_int(), Some(7));
}

#[test]
fn an_impl_internal_array_length_reads_the_same_constant_whatever_the_file_order() {
    let first_source = impl_internal_length_source(0);
    let second_source = impl_internal_length_source(1);
    assert_same_lines(&first_source, &second_source);

    let first = render(&first_source);
    let second = render(&second_source);
    assert_eq!(
        first, second,
        "file order decided the length:\n{first}\n----\n{second}"
    );
    let expected = concat!(
        "error[E0423]: projection 'Counter::LIMIT' is ambiguous: Alpha and Beta both define",
        " 'LIMIT' for Counter; name the trait that declares the one you mean, as in",
        " 'Alpha::LIMIT', or remove one of the competing impls, or rename the item so a single",
        " trait provides it (an array length)\n",
        "  --> test.aelys:5:35\n",
        "   |\n",
        " 5 | impl Counter { fn probe(self, xs: [int; Counter::LIMIT]) -> int { xs[0] } }\n",
        "   |                                   ^^^^^^^^^^^^^^^^^^^^^",
        " the type checker rejected this program\n",
    );
    assert_eq!(first, expected);
}

#[test]
fn an_associated_item_definition_sees_every_impl_whatever_the_file_order() {
    let first_source = associated_definition_source(0);
    let second_source = associated_definition_source(1);
    assert_same_lines(&first_source, &second_source);

    let expected = concat!(
        "error[E0423]: projection 'Counter::Other' is ambiguous: Beta and Gamma both define",
        " 'Other' for Counter; name the trait that declares the one you mean, as in",
        " 'Beta::Other', or remove one of the competing impls, or rename the item so a single",
        " trait provides it (an associated item definition)\n",
        "  --> test.aelys:7:38\n",
        "   |\n",
        " 7 | impl Alpha for Counter { type Item = Counter::Other; }\n",
        "   |                                      ^^^^^^^ the type checker rejected this program\n",
    );
    let first = render(&first_source);
    let second = render(&second_source);
    assert_eq!(
        first, second,
        "file order decided the definition:\n{first}\n----\n{second}"
    );
    assert_eq!(first, expected);
}

const LATE_GHOST: &str = "error[E0423]: projection 'Counter::Ghost' cannot be resolved: no impl for 'Counter' defines 'Ghost'; define it in an impl of a trait that declares 'Ghost' (an impl header)\n  --> test.aelys:8:11\n   |\n 8 | impl From<Counter::Ghost> for Wrapper { fn from(x: Counter::Ghost) -> Wrapper { Wrapper { w: x } } }\n   |           ^^^^^^^ the type checker rejected this program\n";

const LATE_CONST_WRONG: &str = "error[E0422]: associated item 'K' in impl of trait 'Alpha' declares type 'string', and the trait declares 'int' here; declare the same type as the trait (declared type in the impl for 'Counter')\n  --> test.aelys:8:26\n   |\n 8 | impl Alpha for Counter { const K: Counter::Item = 5; }\n   |                          ^^^^^^^^^^^^^^^^^^^^^^^^^^ the type checker rejected this program\n";

const DUPLICATE_PLAIN_SECOND: &str = "error[E0334]: duplicate implementation of trait 'Echo' for type 'Wrapper'\n  --> test.aelys:10:30\n   |\n10 | impl Echo<int> for Wrapper { fn echo(self, value: int) -> int { value } }\n   |                              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ the type checker rejected this program\n";

const DUPLICATE_PROJECTING_SECOND: &str = "error[E0334]: duplicate implementation of trait 'Echo' for type 'Wrapper'\n  --> test.aelys:10:40\n   |\n10 | impl Echo<Counter::Item> for Wrapper { fn echo(self, value: int) -> int { value } }\n   |                                        ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ the type checker rejected this program\n";

const OVERLAP_GENERIC_SECOND: &str = "error[E0340]: trait 'Echo' has overlapping implementations for Wrapper<T> and Wrapper<int>; add a disjoint bound\n  --> test.aelys:10:36\n   |\n10 | impl<T> Echo<int> for Wrapper<T> { fn echo(self, value: int) -> int { 2 } }\n   |                                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ the type checker rejected this program\n";

const OVERLAP_PROJECTING_SECOND: &str = "error[E0340]: trait 'Echo' has overlapping implementations for Wrapper<T> and Wrapper<int>; add a disjoint bound\n  --> test.aelys:10:45\n   |\n10 | impl Echo<Counter::Item> for Wrapper<int> { fn echo(self, value: int) -> int { 1 } }\n   |                                             ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ the type checker rejected this program\n";

fn duplicate_late_header_source(order: usize) -> String {
    let projecting =
        "impl Echo<Counter::Item> for Wrapper { fn echo(self, value: int) -> int { value } }";
    let plain = "impl Echo<int> for Wrapper { fn echo(self, value: int) -> int { value } }";
    let (first, second) = if order == 0 {
        (projecting, plain)
    } else {
        (plain, projecting)
    };
    format!(
        "struct Root {{ r: int }}\nstruct Counter {{ v: int }}\nstruct Wrapper {{ w: int }}\ntrait Base {{ type Base; }}\ntrait Beta<T> {{ type Item; }}\ntrait Echo<T> {{ fn echo(self, value: T) -> T; }}\nimpl Base for Root {{ type Base = int; }}\nimpl Beta<Root::Base> for Counter {{ type Item = int; }}\n{first}\n{second}\nfn ident(x: int) -> int {{\n    x\n}}\nident(7)"
    )
}

fn overlapping_late_header_source(order: usize) -> String {
    let projecting =
        "impl Echo<Counter::Item> for Wrapper<int> { fn echo(self, value: int) -> int { 1 } }";
    let generic = "impl<T> Echo<int> for Wrapper<T> { fn echo(self, value: int) -> int { 2 } }";
    let (first, second) = if order == 0 {
        (projecting, generic)
    } else {
        (generic, projecting)
    };
    format!(
        "struct Root {{ r: int }}\nstruct Counter {{ v: int }}\nstruct Wrapper<T> {{ w: T }}\ntrait Base {{ type Base; }}\ntrait Beta<T> {{ type Item; }}\ntrait Echo<T> {{ fn echo(self, value: T) -> T; }}\nimpl Base for Root {{ type Base = int; }}\nimpl Beta<Root::Base> for Counter {{ type Item = int; }}\n{first}\n{second}\nfn ident(x: int) -> int {{\n    x\n}}\nident(7)"
    )
}

#[test]
fn a_duplicate_impl_is_rejected_when_a_header_names_an_associated_item() {
    let first_source = duplicate_late_header_source(0);
    let second_source = duplicate_late_header_source(1);
    assert_same_lines(&first_source, &second_source);

    assert_eq!(render(&first_source), DUPLICATE_PLAIN_SECOND);
    assert_eq!(render(&second_source), DUPLICATE_PROJECTING_SECOND);
}

#[test]
fn overlapping_impls_are_rejected_when_a_header_names_an_associated_item() {
    let first_source = overlapping_late_header_source(0);
    let second_source = overlapping_late_header_source(1);
    assert_same_lines(&first_source, &second_source);

    assert_eq!(render(&first_source), OVERLAP_GENERIC_SECOND);
    assert_eq!(render(&second_source), OVERLAP_PROJECTING_SECOND);
}

// rounds cannot reach.
fn header_projection_source(order: usize, item: &str, late: bool) -> String {
    let (preamble, definer) = if late {
        (
            "struct Root { r: int }\ntrait Base { type Base; }\ntrait Beta<T> { type Item; }\nimpl Base for Root { type Base = int; }",
            "impl Beta<Root::Base> for Counter { type Item = int; }",
        )
    } else {
        (
            "trait Beta { type Item; }",
            "impl Beta for Counter { type Item = int; }",
        )
    };
    let blank = "";
    let (before, after) = if order == 0 {
        (definer, blank)
    } else {
        (blank, definer)
    };
    format!(
        "struct Counter {{ v: int }}\nstruct Wrapper {{ w: int }}\n{preamble}\n{before}\nimpl From<Counter::{item}> for Wrapper {{ fn from(x: Counter::{item}) -> Wrapper {{ Wrapper {{ w: x }} }} }}\n{after}\nfn probe() -> int {{\n    let w: Wrapper = Wrapper::from(5)\n    w.w\n}}\nprobe()"
    )
}

fn associated_const_type_source(order: usize, item_ty: &str, late: bool) -> String {
    let (preamble, definer) = if late {
        (
            "struct Root { r: int }\ntrait Base { type Base; }\ntrait Beta<T> { type Item; }\nimpl Base for Root { type Base = int; }".to_string(),
            format!("impl Beta<Root::Base> for Counter {{ type Item = {item_ty}; }}"),
        )
    } else {
        (
            "trait Beta { type Item; }".to_string(),
            format!("impl Beta for Counter {{ type Item = {item_ty}; }}"),
        )
    };
    let blank = String::new();
    let (before, after) = if order == 0 {
        (definer, blank)
    } else {
        (blank, definer)
    };
    format!(
        "struct Counter {{ v: int }}\ntrait Alpha {{ const K: int; }}\n{preamble}\n{before}\nimpl Alpha for Counter {{ const K: Counter::Item = 5; }}\n{after}\nfn probe() -> int {{\n    Counter::K\n}}\nprobe()"
    )
}

// it does not, so the acceptance cannot pass by the signature ceasing to read
fn own_associated_return_source(item_ty: &str) -> String {
    format!(
        "struct Counter {{ v: int }}\ntrait Alpha {{ type Item; fn get(self) -> Self::Item; }}\nimpl Alpha for Counter {{ type Item = {item_ty}; fn get(self) -> Self::Item {{ self.v }} }}\nCounter {{ v: 7 }}.get()"
    )
}

fn generic_associated_return_source(item_ty: &str) -> String {
    format!(
        "struct Wrapper<T> {{ w: T }}\ntrait Alpha {{ type Item; fn get(self) -> Self::Item; }}\nimpl Alpha for Wrapper<int> {{ type Item = {item_ty}; fn get(self) -> Self::Item {{ self.w }} }}\nWrapper {{ w: 7 }}.get()"
    )
}

#[test]
fn an_impl_header_projection_resolves_whatever_the_file_order() {
    for late in [false, true] {
        let first_source = header_projection_source(0, "Item", late);
        let second_source = header_projection_source(1, "Item", late);
        assert_same_lines(&first_source, &second_source);

        let first = render(&first_source);
        let second = render(&second_source);
        assert_eq!(
            first, second,
            "file order decided the header:\n{first}\n----\n{second}"
        );
        assert_eq!(first, "accepted, value Some(5)", "late definer: {late}");
    }

    let first_ghost = render(&header_projection_source(0, "Ghost", false));
    let second_ghost = render(&header_projection_source(1, "Ghost", false));
    assert_eq!(
        first_ghost, second_ghost,
        "file order decided the rejection:\n{first_ghost}\n----\n{second_ghost}"
    );
    let expected_ghost = concat!(
        "error[E0423]: projection 'Counter::Ghost' cannot be resolved: no impl for 'Counter'",
        " defines 'Ghost'; define it in an impl of a trait that declares 'Ghost' (an impl header)\n",
        "  --> test.aelys:5:11\n",
        "   |\n",
        " 5 | impl From<Counter::Ghost> for Wrapper { fn from(x: Counter::Ghost) -> Wrapper",
        " { Wrapper { w: x } } }\n",
        "   |           ^^^^^^^ the type checker rejected this program\n",
    );
    assert_eq!(first_ghost, expected_ghost);

    let first_late_ghost = render(&header_projection_source(0, "Ghost", true));
    let second_late_ghost = render(&header_projection_source(1, "Ghost", true));
    assert_eq!(
        first_late_ghost, second_late_ghost,
        "file order decided the rejection:\n{first_late_ghost}\n----\n{second_late_ghost}"
    );
    assert_eq!(first_late_ghost, LATE_GHOST);
}

#[test]
fn an_associated_constant_declared_type_resolves_whatever_the_file_order() {
    for late in [false, true] {
        let first_source = associated_const_type_source(0, "int", late);
        let second_source = associated_const_type_source(1, "int", late);
        assert_same_lines(&first_source, &second_source);

        let first = render(&first_source);
        let second = render(&second_source);
        assert_eq!(
            first, second,
            "file order decided the constant type:\n{first}\n----\n{second}"
        );
        assert_eq!(first, "accepted, value Some(5)", "late definer: {late}");
    }

    let first_wrong = render(&associated_const_type_source(0, "string", false));
    let second_wrong = render(&associated_const_type_source(1, "string", false));
    assert_eq!(
        first_wrong, second_wrong,
        "file order decided the rejection:\n{first_wrong}\n----\n{second_wrong}"
    );
    let expected_wrong = concat!(
        "error[E0422]: associated item 'K' in impl of trait 'Alpha' declares type 'string', and",
        " the trait declares 'int' here; declare the same type as the trait (declared type in",
        " the impl for 'Counter')\n",
        "  --> test.aelys:5:26\n",
        "   |\n",
        " 5 | impl Alpha for Counter { const K: Counter::Item = 5; }\n",
        "   |                          ^^^^^^^^^^^^^^^^^^^^^^^^^^ the type checker rejected",
        " this program\n",
    );
    assert_eq!(first_wrong, expected_wrong);

    let first_late_wrong = render(&associated_const_type_source(0, "string", true));
    let second_late_wrong = render(&associated_const_type_source(1, "string", true));
    assert_eq!(
        first_late_wrong, second_late_wrong,
        "file order decided the rejection:\n{first_late_wrong}\n----\n{second_late_wrong}"
    );
    assert_eq!(first_late_wrong, LATE_CONST_WRONG);
}

#[test]
fn a_method_signature_reads_the_associated_type_its_own_impl_defines() {
    assert_eq!(
        run_ok(&own_associated_return_source("int")).as_int(),
        Some(7)
    );
    let rejected = render(&own_associated_return_source("string"));
    assert!(
        rejected.starts_with("error[E0301]") && rejected.contains("expected string, found int"),
        "the mismatched right hand side must still be rejected:\n{rejected}"
    );
}

#[test]
fn a_generic_struct_impl_monomorphizes_through_its_own_associated_type() {
    assert_eq!(
        run_ok(&generic_associated_return_source("int")).as_int(),
        Some(7)
    );
    let rejected = render(&generic_associated_return_source("string"));
    assert!(
        rejected.starts_with("error[E0301]") && rejected.contains("expected string, found int"),
        "the mismatched right hand side must still be rejected:\n{rejected}"
    );
}

const NAMESPACE_PRELUDE: &str = concat!(
    "struct Counter { v: int }\n",
    "trait Source { type Item; const LIMIT: int; }\n",
    "impl Source for Counter { type Item = int; const LIMIT: int = 4; }\n",
);

// the accepted and the rejected side of a boundary cannot drift apart. no cell
fn namespace_grid_source(receiver: &str, item: &str, position: &str) -> String {
    let p = format!("{receiver}::{item}");
    let line = match (position, receiver) {
        ("parameter", "Self") => {
            format!("impl Counter {{ fn probe(self, x: {p}) -> int {{ return 1 }} }}")
        }
        ("parameter", "T") => format!("fn probe<T: Source>(s: T, x: {p}) -> int {{ return 1 }}"),
        ("parameter", _) => format!("fn probe(x: {p}) -> int {{ return 1 }}"),
        ("return", "Self") => {
            format!("impl Counter {{ fn probe(self) -> {p} {{ return self.v }} }}")
        }
        ("return", "T") => format!("fn probe<T: Source>(s: T, x: {p}) -> {p} {{ return x }}"),
        ("return", _) => format!("fn probe(x: {p}) -> {p} {{ return x }}"),
        ("field", _) => format!("struct Holder {{ it: {p} }}"),
        ("variant", _) => format!("enum Holder {{ One({p}) }}"),
        ("variant field", _) => format!("enum Holder {{ One {{ it: {p} }} }}"),
        ("annotation", "Self") => {
            format!("impl Counter {{ fn probe(self) -> int {{ let z: {p} = 3\n return 1 }} }}")
        }
        ("annotation", "T") => {
            format!("fn probe<T: Source>(s: T, x: {p}) -> int {{ let z: {p} = x\n return 1 }}")
        }
        ("annotation", _) => {
            format!("fn probe(x: {p}) -> int {{ let z: {p} = x\n return 1 }}")
        }
        ("value", "Self") => {
            format!("impl Counter {{ fn probe(self) -> int {{ return {p} + 1 }} }}")
        }
        ("value", "T") => format!("fn probe<T: Source>(s: T) -> int {{ return {p} + 1 }}"),
        ("value", _) => format!("fn probe() -> int {{ return {p} + 1 }}"),
        ("length", "Self") => format!(
            "impl Counter {{ fn probe(self) -> int {{ let a: [int; {p}] = [0, 0, 0, 0]\n return a[0] }} }}"
        ),
        ("length", "T") => format!(
            "fn probe<T: Source>(s: T) -> int {{ let a: [int; {p}] = [0, 0, 0, 0]\n return a[0] }}"
        ),
        ("length", _) => {
            format!("fn probe() -> int {{ let a: [int; {p}] = [0, 0, 0, 0]\n return a[0] }}")
        }
        _ => unreachable!("no grid cell for {position} at {receiver}"),
    };
    format!("{NAMESPACE_PRELUDE}{line}\n1")
}

fn position_reason(position: &str, receiver: &str) -> &'static str {
    match (position, receiver) {
        ("annotation", "Self") => "a type annotation",
        ("return", "Self") => "a return type",
        ("annotation" | "parameter" | "return", _) => "a parameter type",
        ("field", _) => "a struct field",
        ("variant" | "variant field", _) => "an enum variant field",
        ("value", _) => "a value expression",
        ("length", _) => "an array length",
        ("sibling" | "argument", _) => "a bound",
        _ => unreachable!("no reason for position {position}"),
    }
}

fn wrong_namespace_line(
    receiver: &str,
    item: &str,
    found_is_a_constant: bool,
    reason: &str,
) -> String {
    let (found, wanted, elsewhere) = if found_is_a_constant {
        ("associated constant", "associated type", "value")
    } else {
        ("associated type", "associated constant", "type")
    };
    format!(
        "error[E0423]: projection '{receiver}::{item}' cannot be resolved: '{item}' is an {found} of '{receiver}', not an {wanted}; name an {wanted} here, or use '{receiver}::{item}' where a {elsewhere} is expected ({reason})"
    )
}

fn absent_line(receiver: &str, item: &str, reason: &str) -> String {
    format!(
        "error[E0423]: projection '{receiver}::{item}' cannot be resolved: no impl for '{receiver}' defines '{item}'; define it in an impl of a trait that declares '{item}' ({reason})"
    )
}

fn grid_message(receiver: &str, item: &str, position: &str) -> String {
    let rendered = render(&namespace_grid_source(receiver, item, position));
    rendered.lines().next().unwrap_or_default().to_string()
}

fn grid_accepts(receiver: &str, item: &str, position: &str) -> String {
    render(&namespace_grid_source(receiver, item, position))
}

const NAMESPACE_TYPE_POSITIONS: [&str; 6] = [
    "annotation",
    "parameter",
    "return",
    "field",
    "variant",
    "variant field",
];

#[test]
fn a_constant_in_type_position_is_rejected_at_the_signature_with_no_call_site() {
    let source = namespace_grid_source("Counter", "LIMIT", "parameter");
    assert_eq!(
        source.matches("probe(").count(),
        1,
        "the program must contain no call to the declared function:\n{source}"
    );
    let expected = format!(
        "{}\n  --> test.aelys:4:13\n   |\n 4 | fn probe(x: Counter::LIMIT) -> int {{ return 1 }}\n   |             ^^^^^^^ the type checker rejected this program\n",
        wrong_namespace_line("Counter", "LIMIT", true, "a parameter type")
    );
    assert_eq!(render(&source), expected);
}

#[test]
fn a_constant_in_every_type_position_names_the_constant_namespace() {
    for position in NAMESPACE_TYPE_POSITIONS {
        for receiver in ["Counter", "Source"] {
            assert_eq!(
                grid_message(receiver, "LIMIT", position),
                wrong_namespace_line(receiver, "LIMIT", true, position_reason(position, receiver)),
                "cell {receiver}::LIMIT in {position} position"
            );
            assert_eq!(
                grid_accepts(receiver, "Item", position),
                "accepted, value Some(1)",
                "cell {receiver}::Item in {position} position must keep working"
            );
        }
    }
    for (receiver, position) in [
        ("Self", "return"),
        ("Self", "annotation"),
        ("T", "parameter"),
        ("T", "annotation"),
    ] {
        assert_eq!(
            grid_message(receiver, "LIMIT", position),
            wrong_namespace_line(
                rendered_receiver(receiver, position, false),
                "LIMIT",
                true,
                position_reason(position, receiver)
            ),
            "cell {receiver}::LIMIT in {position} position"
        );
        assert_eq!(
            grid_accepts(receiver, "Item", position),
            "accepted, value Some(1)",
            "cell {receiver}::Item in {position} position must keep working"
        );
    }
}

#[test]
fn a_type_in_value_position_names_the_type_namespace() {
    for receiver in ["Counter", "Source", "Self", "T"] {
        assert_eq!(
            grid_message(receiver, "Item", "value"),
            wrong_namespace_line(
                rendered_receiver(receiver, "value", false),
                "Item",
                false,
                "a value expression"
            ),
            "cell {receiver}::Item in value position"
        );
        assert_eq!(
            grid_accepts(receiver, "LIMIT", "value"),
            "accepted, value Some(1)",
            "cell {receiver}::LIMIT in value position must keep working"
        );
    }
}

#[test]
fn a_type_in_array_length_position_names_the_type_namespace() {
    for receiver in ["Counter", "Source", "Self", "T"] {
        assert_eq!(
            grid_message(receiver, "Item", "length"),
            wrong_namespace_line(
                rendered_receiver(receiver, "length", false),
                "Item",
                false,
                "an array length"
            ),
            "cell {receiver}::Item in array length position"
        );
    }
    for receiver in ["Counter", "Source", "Self"] {
        assert_eq!(
            grid_accepts(receiver, "LIMIT", "length"),
            "accepted, value Some(1)",
            "cell {receiver}::LIMIT in array length position must keep working"
        );
    }
}

#[test]
fn an_item_in_neither_namespace_is_reported_absent_not_accepted() {
    for position in NAMESPACE_TYPE_POSITIONS {
        assert_eq!(
            grid_message("Counter", "NOPE", position),
            absent_line("Counter", "NOPE", position_reason(position, "Counter")),
            "cell Counter::NOPE in {position} position"
        );
    }
    assert_eq!(
        grid_message("Counter", "NOPE", "length"),
        absent_line("Counter", "NOPE", "an array length")
    );
    // so absence there is not resolved here; it must still be rejected.
    assert_eq!(
        grid_message("Counter", "NOPE", "value"),
        absent_line("Counter", "NOPE", "a value expression"),
        "an absent item in value position must still be rejected"
    );
}

#[test]
fn a_projection_through_a_type_parameter_stays_opaque() {
    let accepted = run_ok(
        "struct Counter { v: int }\ntrait Source { type Item; fn next(self) -> Self::Item; }\nimpl Source for Counter { type Item = int; fn next(self) -> int { return self.v } }\nfn pull<T: Source>(source: T) -> T::Item { let x: T::Item = source.next()\n return x }\npull(Counter { v: 6 })",
    );
    assert_eq!(accepted.as_int(), Some(6));
}

#[test]
fn a_supertrait_bound_still_declares_the_projected_item() {
    let accepted = run_ok(
        "struct Counter { v: int }\ntrait Base { type Item; const LIMIT: int; }\ntrait Source: Base { fn next(self) -> int; }\nimpl Base for Counter { type Item = int; const LIMIT: int = 4; }\nimpl Source for Counter { fn next(self) -> int { return self.v } }\nfn pull<T: Source>(source: T, x: T::Item) -> int { return T::LIMIT }\n1",
    );
    assert_eq!(accepted.as_int(), Some(1));
}

#[test]
fn a_supertrait_declared_constant_in_type_position_names_the_constant_namespace() {
    let rendered = render(
        "struct Counter { v: int }\ntrait Base { type Item; const LIMIT: int; }\ntrait Source: Base { fn next(self) -> int; }\nimpl Base for Counter { type Item = int; const LIMIT: int = 4; }\nimpl Source for Counter { fn next(self) -> int { return self.v } }\nfn pull<T: Source>(source: T, x: T::LIMIT) -> int { return 1 }\n1",
    );
    assert_eq!(
        rendered.lines().next().unwrap_or_default(),
        wrong_namespace_line("T", "LIMIT", true, "a parameter type")
    );
}

fn namespace_order_source(order: usize) -> String {
    let other = "impl Other for Counter { const Item: int = 5; }";
    let blank = "";
    let (before, after) = if order == 0 {
        (other, blank)
    } else {
        (blank, other)
    };
    format!(
        "struct Counter {{ v: int }}\ntrait Source {{ const Item: int; }}\ntrait Other {{ const Item: int; }}\n{before}\nimpl Source for Counter {{ const Item: int = 4; }}\n{after}\nfn probe(x: Counter::Item) -> int {{ return 1 }}\n1"
    )
}

#[test]
fn the_namespace_diagnostic_does_not_depend_on_declaration_order() {
    let first_source = namespace_order_source(0);
    let second_source = namespace_order_source(1);
    assert_same_lines(&first_source, &second_source);
    let first = render(&first_source);
    let second = render(&second_source);
    assert_eq!(
        first, second,
        "declaration order decided the message:\n{first}\n----\n{second}"
    );
    assert_eq!(
        first.lines().next().unwrap_or_default(),
        wrong_namespace_line("Counter", "Item", true, "a parameter type")
    );
}

const GENERIC_NAMESPACE_PRELUDE: &str = concat!(
    "struct Wrap<T> { w: T }\n",
    "trait Source { type Item; const LIMIT: int; fn take(self, x: Self::Item) -> int; }\n",
    "impl Source for Wrap<int> { type Item = int; const LIMIT: int = 4; fn take(self, x: int) -> int { return x } }\n",
);

fn generic_namespace_grid_source(receiver: &str, item: &str, position: &str) -> String {
    let p = format!("{receiver}::{item}");
    // 'take' is the only way a generic body can consume a value of the projected
    let (line, call) = match (position, receiver) {
        ("parameter", "Self") => (
            format!("impl Wrap<int> {{ fn probe(self, x: {p}) -> int {{ return x + 1 }} }}"),
            "Wrap { w: 0 }.probe(6)".to_string(),
        ),
        ("parameter", "T") => (
            format!("fn probe<T: Source>(s: T, x: {p}) -> int {{ return s.take(x) + 1 }}"),
            "probe(Wrap { w: 0 }, 6)".to_string(),
        ),
        ("parameter", _) => (
            format!("fn probe(x: {p}) -> int {{ return x + 1 }}"),
            "probe(6)".to_string(),
        ),
        ("return", "Self") => (
            format!("impl Wrap<int> {{ fn probe(self) -> {p} {{ return self.w }} }}"),
            "Wrap { w: 6 }.probe() + 2".to_string(),
        ),
        ("return", "T") => (
            format!("fn probe<T: Source>(s: T, x: {p}) -> {p} {{ return x }}"),
            "probe(Wrap { w: 0 }, 6) + 2".to_string(),
        ),
        ("return", _) => (
            format!("fn probe(x: {p}) -> {p} {{ return x }}"),
            "probe(6) + 2".to_string(),
        ),
        ("annotation", "Self") => (
            format!(
                "impl Wrap<int> {{ fn probe(self) -> int {{ let z: {p} = self.w\n return z + 3 }} }}"
            ),
            "Wrap { w: 6 }.probe()".to_string(),
        ),
        ("annotation", "T") => (
            format!(
                "fn probe<T: Source>(s: T, x: {p}) -> int {{ let z: {p} = x\n return s.take(z) + 3 }}"
            ),
            "probe(Wrap { w: 0 }, 6)".to_string(),
        ),
        ("annotation", _) => (
            format!("fn probe(x: {p}) -> int {{ let z: {p} = x\n return z + 3 }}"),
            "probe(6)".to_string(),
        ),
        ("value", "Self") => (
            format!("impl Wrap<int> {{ fn probe(self) -> int {{ return {p} + 1 }} }}"),
            "Wrap { w: 0 }.probe()".to_string(),
        ),
        ("value", "T") => (
            format!("fn probe<T: Source>(s: T) -> int {{ return {p} + 1 }}"),
            "probe(Wrap { w: 0 })".to_string(),
        ),
        ("value", _) => (
            format!("fn probe() -> int {{ return {p} + 1 }}"),
            "probe()".to_string(),
        ),
        _ => unreachable!("no generic grid cell for {position} at {receiver}"),
    };
    format!("{GENERIC_NAMESPACE_PRELUDE}{line}\n{call}")
}

// each value is what calling probe produces, so a cell that only declares cannot pass
fn generic_grid_accepted(position: &str) -> String {
    let value = match position {
        "parameter" => 7,
        "return" => 8,
        "annotation" => 9,
        "value" => 5,
        _ => unreachable!("no accepted value for {position} position"),
    };
    format!("accepted, value Some({value})")
}

fn generic_grid_message(receiver: &str, item: &str, position: &str) -> String {
    let rendered = render(&generic_namespace_grid_source(receiver, item, position));
    rendered.lines().next().unwrap_or_default().to_string()
}

fn generic_grid_accepts(receiver: &str, item: &str, position: &str) -> String {
    render(&generic_namespace_grid_source(receiver, item, position))
}

fn unbound_line(receiver: &str, item: &str, reason: &str) -> String {
    format!(
        "error[E0423]: projection '{receiver}::{item}' cannot be resolved: no bound in scope declares '{item}'; add a bound on '{receiver}' whose trait declares '{item}' ({reason})"
    )
}

const GENERIC_SIGNATURE_POSITIONS: [&str; 2] = ["parameter", "return"];

fn rendered_receiver(receiver: &'static str, position: &str, generic: bool) -> &'static str {
    match (receiver, generic, position) {
        ("Self", false, _) => "Counter",
        ("Self", true, "parameter" | "return") => "Wrap<int>",
        ("Self", true, _) => "Wrap",
        _ => receiver,
    }
}

#[test]
fn a_constant_in_type_position_names_the_constant_namespace_over_a_generic_target() {
    for position in GENERIC_SIGNATURE_POSITIONS {
        for receiver in ["Wrap", "Source", "Self", "T"] {
            assert_eq!(
                generic_grid_message(receiver, "LIMIT", position),
                wrong_namespace_line(
                    rendered_receiver(receiver, position, true),
                    "LIMIT",
                    true,
                    position_reason(position, receiver)
                ),
                "cell {receiver}::LIMIT in {position} position over a generic target"
            );
        }
    }
}

#[test]
fn an_absent_item_over_a_generic_target_is_reported_absent_not_unmaterialized() {
    for position in GENERIC_SIGNATURE_POSITIONS {
        for receiver in ["Wrap", "Source", "Self"] {
            assert_eq!(
                generic_grid_message(receiver, "NOPE", position),
                absent_line(
                    rendered_receiver(receiver, position, true),
                    "NOPE",
                    position_reason(position, receiver)
                ),
                "cell {receiver}::NOPE in {position} position over a generic target"
            );
        }
        assert_eq!(
            generic_grid_message("T", "NOPE", position),
            unbound_line("T", "NOPE", position_reason(position, "T")),
            "cell T::NOPE in {position} position over a generic target"
        );
    }
}

#[test]
fn the_accepted_half_of_type_position_survives_a_generic_target() {
    for position in GENERIC_SIGNATURE_POSITIONS {
        for receiver in ["Wrap", "Source", "Self", "T"] {
            assert_eq!(
                generic_grid_accepts(receiver, "Item", position),
                generic_grid_accepted(position),
                "cell {receiver}::Item in {position} position must keep working"
            );
        }
    }
}

#[test]
fn a_type_in_value_position_names_the_type_namespace_over_a_generic_target() {
    for receiver in ["Source", "T"] {
        assert_eq!(
            generic_grid_message(receiver, "Item", "value"),
            wrong_namespace_line(receiver, "Item", false, "a value expression"),
            "cell {receiver}::Item in value position over a generic target"
        );
    }
    for receiver in ["Wrap", "Source", "Self", "T"] {
        assert_eq!(
            generic_grid_accepts(receiver, "LIMIT", "value"),
            generic_grid_accepted("value"),
            "cell {receiver}::LIMIT in value position must keep working"
        );
    }
}

#[test]
fn a_generic_target_resolves_below_the_signature_as_it_does_above() {
    assert_eq!(
        generic_grid_accepts("Self", "Item", "annotation"),
        generic_grid_accepted("annotation")
    );
    assert_eq!(
        generic_grid_accepts("Self", "LIMIT", "value"),
        generic_grid_accepted("value")
    );
    assert_eq!(
        generic_grid_message("Self", "NOPE", "return"),
        absent_line("Wrap<int>", "NOPE", "a return type")
    );
    assert_eq!(
        generic_grid_message("Wrap", "NOPE", "return"),
        absent_line("Wrap", "NOPE", "a parameter type")
    );
}

// must not reach the argument nested inside the annotation on the right.
fn nested_binding_source(binding: &str, nested: &str) -> String {
    format!(
        "struct Counter {{ v: int }}\nstruct Wrap<T> {{ w: T }}\ntrait Source {{ type Item; const LIMIT: int; }}\nimpl Source for Counter {{ type Item = int; const LIMIT: int = 4; }}\nfn probe<T: Source<{binding} = Wrap<Counter::{nested}>>>(s: T) -> int {{ return 1 }}\n1"
    )
}

#[test]
fn a_nested_annotation_argument_is_a_type_position_under_a_constant_binding() {
    for binding in ["Item", "LIMIT"] {
        assert_eq!(
            render(&nested_binding_source(binding, "Item")),
            "accepted, value Some(1)",
            "a nested associated type under the '{binding}' binding is a type position"
        );
        assert_eq!(
            render(&nested_binding_source(binding, "LIMIT"))
                .lines()
                .next()
                .unwrap_or_default(),
            wrong_namespace_line("Counter", "LIMIT", true, "a bound"),
            "a nested associated constant under the '{binding}' binding must still be caught"
        );
    }
}

#[test]
fn the_head_of_a_binding_keeps_the_namespace_of_the_binding() {
    let head = |binding: &str, item: &str| {
        format!(
            "struct Counter {{ v: int }}\ntrait Source {{ type Item; const LIMIT: int; }}\nimpl Source for Counter {{ type Item = int; const LIMIT: int = 4; }}\nfn probe<T: Source<{binding} = Counter::{item}>>(s: T) -> int {{ return 1 }}\n1"
        )
    };
    assert_eq!(render(&head("Item", "Item")), "accepted, value Some(1)");
    assert_eq!(render(&head("LIMIT", "LIMIT")), "accepted, value Some(1)");
    assert_eq!(
        render(&head("Item", "LIMIT"))
            .lines()
            .next()
            .unwrap_or_default(),
        wrong_namespace_line("Counter", "LIMIT", true, "a bound")
    );
    assert_eq!(
        render(&head("LIMIT", "Item"))
            .lines()
            .next()
            .unwrap_or_default(),
        wrong_namespace_line("Counter", "Item", false, "a bound")
    );
}

// and its span starts at the wrapper, so it must not be the message shown.
#[test]
fn a_namespace_error_under_a_generic_argument_outranks_the_surface_audit() {
    let source = |item: &str| {
        format!(
            "struct Counter {{ v: int }}\nstruct Wrap<T> {{ w: T }}\ntrait Source {{ type Item; const LIMIT: int; }}\nimpl Source for Counter {{ type Item = int; const LIMIT: int = 4; }}\nfn probe(x: Wrap<Counter::{item}>) -> int {{ return 1 }}\n1"
        )
    };
    assert_eq!(render(&source("Item")), "accepted, value Some(1)");
    assert_eq!(
        render(&source("LIMIT")).lines().next().unwrap_or_default(),
        wrong_namespace_line("Counter", "LIMIT", true, "a parameter type")
    );
    assert_eq!(
        render(&source("NOPE")).lines().next().unwrap_or_default(),
        absent_line("Counter", "NOPE", "a parameter type")
    );
}

const BOUND_SCOPE_PRELUDE: &str = concat!(
    "struct Counter { v: int }\n",
    "trait Source { type Item; const LIMIT: int; }\n",
    "trait Wrap<X> { fn w(self) -> int; }\n",
    "impl Source for Counter { type Item = int; const LIMIT: int = 4; }\n",
    "struct Host { h: int }\n",
);

// to the rejected one. no cell calls what it declares, so a signature that
fn bound_scope_source(form: &str, position: &str, item: &str) -> String {
    let (generics, params, ret, body) = match position {
        "parameter" => (
            "<U: Source>".to_string(),
            format!("x: U::{item}"),
            "int".to_string(),
            "return 1",
        ),
        "return" => (
            "<U: Source>".to_string(),
            format!("x: U::{item}"),
            format!("U::{item}"),
            "return x",
        ),
        "sibling" => (
            format!("<T: Source, U: Source<Item = T::{item}>>"),
            "a: T, b: U".to_string(),
            "int".to_string(),
            "return 1",
        ),
        "argument" => (
            format!("<T: Source, U: Wrap<T::{item}>>"),
            "a: T, b: U".to_string(),
            "int".to_string(),
            "return 1",
        ),
        _ => unreachable!("no bound-scope cell at {position}"),
    };
    let line = match form {
        "free" => format!("fn probe{generics}({params}) -> {ret} {{ {body} }}"),
        "method" => {
            format!("impl Host {{ fn probe{generics}(self, {params}) -> {ret} {{ {body} }} }}")
        }
        "trait" => format!("trait Sink {{ fn probe{generics}(self, {params}) -> {ret}; }}"),
        _ => unreachable!("no bound-scope form named {form}"),
    };
    format!("{BOUND_SCOPE_PRELUDE}{line}\n1")
}

const BOUND_SCOPE_FORMS: [&str; 3] = ["free", "method", "trait"];
const BOUND_SCOPE_POSITIONS: [&str; 4] = ["parameter", "return", "sibling", "argument"];

fn bound_scope_receiver(position: &str) -> &'static str {
    match position {
        "sibling" | "argument" => "T",
        _ => "U",
    }
}

#[test]
fn a_declared_bound_reaches_every_annotation_of_its_own_signature() {
    for form in BOUND_SCOPE_FORMS {
        for position in BOUND_SCOPE_POSITIONS {
            assert_eq!(
                render(&bound_scope_source(form, position, "Item")),
                "accepted, value Some(1)",
                "cell {form} x {position} must read the bound declared beside it"
            );
        }
    }
}

#[test]
fn an_item_no_bound_declares_is_still_rejected_in_every_form() {
    for form in BOUND_SCOPE_FORMS {
        for position in BOUND_SCOPE_POSITIONS {
            let receiver = bound_scope_receiver(position);
            assert_eq!(
                render(&bound_scope_source(form, position, "Gone"))
                    .lines()
                    .next()
                    .unwrap_or_default(),
                unbound_line(receiver, "Gone", position_reason(position, receiver)),
                "cell {form} x {position} must still reject an item no bound declares"
            );
        }
    }
}

const SHADOWED_BOUND_PRELUDE: &str = concat!(
    "struct Counter { v: int }\n",
    "trait Source { type Item; const LIMIT: int; }\n",
    "trait Other { fn away(self) -> int; }\n",
    "impl Source for Counter { type Item = int; const LIMIT: int = 4; }\n",
    "struct Host<T> { h: T }\n",
);

fn shadowed_bound_source(param: &str, namespace: &str) -> String {
    let line = match namespace {
        "type" => format!(
            "impl<T: Source> Host<T> {{ fn probe<{param}: Other>(self, x: {param}::Item) -> int {{ return 1 }} }}"
        ),
        "const" => format!(
            "impl<T: Source> Host<T> {{ fn probe<{param}: Other>(self, y: {param}) -> int {{ return {param}::LIMIT }} }}"
        ),
        _ => unreachable!("no shadowed-bound cell for {namespace}"),
    };
    format!("{SHADOWED_BOUND_PRELUDE}{line}\n1")
}

const SHADOWED_BOUND_NAMESPACES: [(&str, &str); 2] = [("type", "Item"), ("const", "LIMIT")];

#[test]
fn an_impl_bound_does_not_reach_a_method_type_parameter_of_the_same_name() {
    for param in ["T", "W"] {
        for (namespace, item) in SHADOWED_BOUND_NAMESPACES {
            assert_eq!(
                render(&shadowed_bound_source(param, namespace))
                    .lines()
                    .next()
                    .unwrap_or_default(),
                unbound_line(
                    param,
                    item,
                    if namespace == "type" {
                        "a parameter type"
                    } else {
                        "a value expression"
                    }
                ),
                "cell {param} x {namespace}: the impl's bound must not reach the method's '{param}'"
            );
        }
    }
}

const LAMBDA_BOUND_PRELUDE: &str = concat!(
    "struct Counter { v: int }\n",
    "trait Source { fn peek(self) -> int; }\n",
    "trait Other { fn away(self) -> int; }\n",
    "impl Source for Counter { fn peek(self) -> int { return self.v } }\n",
    "impl Other for Counter { fn away(self) -> int { return self.v } }\n",
);

// does not and must be rejected at every depth.
fn lambda_bound_source(bound: &str, depth: usize) -> String {
    let mut body = "return v.peek()".to_string();
    for level in 0..depth {
        body = format!("let g{level} = fn() -> int {{ {body} }}\n    return g{level}()");
    }
    format!(
        "{LAMBDA_BOUND_PRELUDE}fn probe<T: {bound}>(v: T) -> int {{ {body} }}\nprobe(Counter {{ v: 1 }})"
    )
}

const LAMBDA_BOUND_DEPTHS: [usize; 3] = [0, 1, 2];

// undefined symbol whatever the receiver is. the cell below pins that.
fn type_checks(source: &str) -> String {
    match aelys::Runtime::new().compile(source, aelys::CompileOptions::default()) {
        Ok(_) => "accepted".to_string(),
        Err(error) => error
            .to_string()
            .lines()
            .next()
            .unwrap_or_default()
            .to_string(),
    }
}

#[test]
fn a_bound_reaches_a_method_call_nested_in_a_lambda() {
    for depth in LAMBDA_BOUND_DEPTHS {
        assert_eq!(
            type_checks(&lambda_bound_source("Source", depth)),
            "accepted",
            "a bound method call must resolve {depth} lambdas deep"
        );
    }
    assert_eq!(
        render(&lambda_bound_source("Source", 0)),
        "accepted, value Some(1)",
        "the unwrapped counterpart must still run"
    );
}

// the mangled method symbol was undefined at run time because the vm skipped
#[test]
fn a_method_call_inside_a_lambda_runs() {
    assert_eq!(
        render(&lambda_bound_source("Source", 1)),
        "accepted, value Some(1)",
        "a bound method call one lambda deep must run"
    );
    assert_eq!(
        render(
            "struct Counter { v: int }\nimpl Counter { fn peek(self) -> int { return self.v } }\nfn probe(v: Counter) -> int { let g = fn() -> int { return v.peek() }\n    return g() }\nprobe(Counter { v: 1 })"
        ),
        "accepted, value Some(1)",
        "the same call must run with no trait and no bound in the program"
    );
}

#[test]
fn a_method_no_bound_provides_is_rejected_at_every_lambda_depth() {
    for depth in LAMBDA_BOUND_DEPTHS {
        assert_eq!(
            render(&lambda_bound_source("Other", depth))
                .lines()
                .next()
                .unwrap_or_default(),
            "error[E0351]: method 'peek' is not available on type parameter 'T' because no bound on 'T' provides it; add the bound 'T: Trait' that declares 'peek'",
            "an unprovided method must still be rejected {depth} lambdas deep"
        );
    }
}

// rendered for all three, so the accepted side and the message cannot drift
fn self_scope_source(scope: &str, position: &str) -> String {
    let head = "trait Sink { type Item; const LIMIT: int;";
    let line = match (scope, position) {
        ("impl", "annotation") => {
            "impl Counter { fn probe(self) -> int { let z: Self::Item = 3\n return 1 } }"
                .to_string()
        }
        ("trait", "annotation") => {
            format!("{head} fn probe(self) -> int {{ let z: Self::Item = 3\n return 1 }} }}")
        }
        ("top", "annotation") => {
            "fn probe() -> int { let z: Self::Item = 3\n return 1 }".to_string()
        }
        ("impl", "parameter") => {
            "impl Counter { fn probe(self, x: Self::Item) -> int { return 1 } }".to_string()
        }
        ("trait", "parameter") => format!("{head} fn probe(self, x: Self::Item) -> int; }}"),
        ("top", "parameter") => "fn probe(x: Self::Item) -> int { return 1 }".to_string(),
        ("impl", "return") => {
            "impl Counter { fn probe(self) -> Self::Item { return self.v } }".to_string()
        }
        ("trait", "return") => format!("{head} fn probe(self) -> Self::Item; }}"),
        ("top", "return") => "fn probe(x: int) -> Self::Item { return x }".to_string(),
        ("impl", "value") => {
            "impl Counter { fn probe(self) -> int { return Self::LIMIT + 1 } }".to_string()
        }
        ("trait", "value") => {
            format!("{head} fn probe(self) -> int {{ return Self::LIMIT + 1 }} }}")
        }
        ("top", "value") => "fn probe() -> int { return Self::LIMIT + 1 }".to_string(),
        ("impl", "length") => {
            "impl Counter { fn probe(self) -> int { let a: [int; Self::LIMIT] = [0, 0, 0, 0]\n return a[0] } }"
                .to_string()
        }
        ("trait", "length") => format!("{head} fn probe(self, a: [int; Self::LIMIT]) -> int; }}"),
        ("top", "length") => {
            "fn probe() -> int { let a: [int; Self::LIMIT] = [0, 0, 0, 0]\n return a[0] }"
                .to_string()
        }
        _ => unreachable!("no Self-scope cell at {scope} x {position}"),
    };
    format!("{NAMESPACE_PRELUDE}{line}\n1")
}

const SELF_SCOPE_POSITIONS: [&str; 5] = ["annotation", "parameter", "return", "value", "length"];

fn self_outside_impl_line(item: &str, reason: &str) -> String {
    format!(
        "error[E0423]: projection 'Self::{item}' cannot be resolved: 'Self' names a type only inside an impl or a trait declaration, and neither is open here; write the type itself in place of 'Self' ({reason})"
    )
}

fn self_scope_item(position: &str) -> &'static str {
    match position {
        "value" | "length" => "LIMIT",
        _ => "Item",
    }
}

#[test]
fn self_inside_an_impl_keeps_naming_the_impl_target() {
    for position in SELF_SCOPE_POSITIONS {
        assert_eq!(
            render(&self_scope_source("impl", position)),
            "accepted, value Some(1)",
            "'Self' in {position} position inside an impl must keep resolving"
        );
    }
}

#[test]
fn self_outside_an_impl_says_so_and_prescribes_no_bound() {
    for position in SELF_SCOPE_POSITIONS {
        let first = render(&self_scope_source("top", position))
            .lines()
            .next()
            .unwrap_or_default()
            .to_string();
        assert_eq!(
            first,
            self_outside_impl_line(self_scope_item(position), position_reason(position, "Self")),
            "'Self' in {position} position at file scope must name the real fault"
        );
        assert!(
            !first.contains("add a bound"),
            "the message must not prescribe a bound on 'Self': {first}"
        );
        assert!(
            !first.contains("undefined function"),
            "no call is written, so the message must not name one: {first}"
        );
    }
}

const SELF_TRAIT_RESOLVING_POSITIONS: [&str; 4] = ["annotation", "parameter", "return", "value"];

// a trait declaration keeps 'self' open, and the one position it cannot serve
#[test]
fn self_inside_a_trait_declaration_is_open_at_every_position() {
    for position in SELF_SCOPE_POSITIONS {
        let first = render(&self_scope_source("trait", position))
            .lines()
            .next()
            .unwrap_or_default()
            .to_string();
        assert_ne!(
            first,
            self_outside_impl_line(self_scope_item(position), position_reason(position, "Self")),
            "'Self' in {position} position inside a trait declaration is open, not closed"
        );
    }
    for position in SELF_TRAIT_RESOLVING_POSITIONS {
        assert_eq!(
            render(&self_scope_source("trait", position)),
            "accepted, value Some(1)",
            "'Self' in {position} position inside a trait declaration must resolve"
        );
    }
    assert_eq!(
        render(&self_scope_source("trait", "length"))
            .lines()
            .next()
            .unwrap_or_default(),
        "error[E0423]: array length 'Self::LIMIT' depends on the type parameter 'Self', and a fixed-array length must be known where the array is written; write the length as a literal, name the constant on a concrete type, or use a growable array (an array length)"
    );
}

// receiver is the accepted side and the parameter is the rejected one.
fn nominal_parameter_source(shape: &str, receiver: &str) -> String {
    let line = match shape {
        "struct" => format!("struct Holder<T> {{ item: {receiver}::Item }}"),
        "enum" => format!("enum Holder<T> {{ One({receiver}::Item) }}"),
        _ => unreachable!("no nominal-parameter cell for {shape}"),
    };
    format!("{NAMESPACE_PRELUDE}{line}\n1")
}

fn nominal_shape_reason(shape: &str) -> &'static str {
    match shape {
        "struct" => "a struct field",
        "enum" => "an enum variant field",
        _ => unreachable!("no reason for shape {shape}"),
    }
}

fn nominal_parameter_line(receiver: &str, item: &str, reason: &str) -> String {
    format!(
        "error[E0423]: projection '{receiver}::{item}' cannot be resolved: '{receiver}' is a type parameter of a struct or an enum, and those carry no bound, so nothing declares '{item}'; name a concrete type here, or give the struct or enum a parameter for '{item}' itself ({reason})"
    )
}

#[test]
fn a_projection_over_a_nominal_parameter_does_not_prescribe_a_parse_error() {
    for shape in ["struct", "enum"] {
        assert_eq!(
            render(&nominal_parameter_source(shape, "Counter")),
            "accepted, value Some(1)",
            "a concrete receiver in a {shape} field must keep resolving"
        );
        let first = render(&nominal_parameter_source(shape, "T"))
            .lines()
            .next()
            .unwrap_or_default()
            .to_string();
        assert_eq!(
            first,
            nominal_parameter_line("T", "Item", nominal_shape_reason(shape)),
            "a {shape} type parameter carries no bound and the message must say so"
        );
        assert!(
            !first.contains("add a bound"),
            "the grammar rejects a bound on a {shape} parameter, so the message must not \
             prescribe one: {first}"
        );
    }
}

// concrete receiver folds to a length, a type parameter cannot.
fn array_length_source(receiver: &str) -> String {
    let head = if receiver == "T" {
        "fn probe<T: Source>(s: T) -> int"
    } else {
        "fn probe() -> int"
    };
    format!(
        "{NAMESPACE_PRELUDE}{head} {{ let a: [int; {receiver}::LIMIT] = [0, 0, 0, 0]\n return a[0] }}\n1"
    )
}

#[test]
fn the_array_length_message_names_no_type_the_program_lacks() {
    assert_eq!(
        render(&array_length_source("Counter")),
        "accepted, value Some(1)",
        "a concrete receiver must still fold to a length"
    );
    let first = render(&array_length_source("T"))
        .lines()
        .next()
        .unwrap_or_default()
        .to_string();
    assert_eq!(
        first,
        "error[E0423]: array length 'T::LIMIT' depends on the type parameter 'T', and a fixed-array length must be known where the array is written; write the length as a literal, name the constant on a concrete type, or use a growable array (an array length)"
    );
    assert!(
        !first.contains("Bounds"),
        "the message must not send the reader after a type the program never declares: {first}"
    );
}

fn one_node_cycle_source(field: &str) -> String {
    format!("struct A {{ f: {field} }}\nfn probe() -> int {{ 1 }}\nprobe()\n")
}

#[test]
fn the_spellings_of_a_one_node_cycle_split_into_rejected_and_accepted_halves() {
    for field in ["A", "[A; 1]", "[A; 2]", "[[A; 1]; 2]"] {
        let rejected = run_err(&one_node_cycle_source(field));
        assert!(
            rejected.starts_with("error[E0428]") && rejected.contains("cycle A -> A"),
            "the field written '{field}' closes the cycle: {rejected}"
        );
    }

    // option, vec and result each own a constructor inhabited whatever the argument.
    for terminating in [
        "[A; 0]",
        "[[A; 0]; 3]",
        "[[A; 3]; 0]",
        "Vec<A>",
        "Option<A>",
        "Result<A, int>",
    ] {
        assert_eq!(
            run_ok(&one_node_cycle_source(terminating)).as_int(),
            Some(1),
            "{terminating} admits a value and must keep compiling"
        );
    }
}

// struct half above cannot drift apart.
fn self_referential_enum_source(variant: &str) -> String {
    format!("enum E {{ {variant} }}\nfn probe() -> int {{ 1 }}\nprobe()\n")
}

#[test]
fn a_self_referential_enum_with_no_terminating_variant_is_rejected() {
    for variant in ["V(E)", "V { inner: E }"] {
        let rejected = run_err(&self_referential_enum_source(variant));
        assert!(
            rejected.starts_with("error[E0428]") && rejected.contains("cycle E -> E"),
            "enum E {{ {variant} }} has no variant that terminates it: {rejected}"
        );
        assert!(
            rejected.contains("enum 'E' can never be constructed: its variants form"),
            "an enum must be accused as an enum, not as a struct: {rejected}"
        );
    }
    assert_eq!(
        run_ok(&self_referential_enum_source("Nil, V(E)")).as_int(),
        Some(1),
        "a terminating variant makes the same enum inhabited"
    );
    // graph and are rejected alike.
    let mixed = run_err("struct A { e: E }\nenum E { V(A) }\nfn probe() -> int { 1 }\nprobe()\n");
    assert!(
        mixed.starts_with("error[E0428]") && mixed.contains("cycle A -> E -> A"),
        "the struct/enum two node cycle must be rejected: {mixed}"
    );
    let direct =
        run_err("struct A { b: B }\nstruct B { a: A }\nfn probe() -> int { 1 }\nprobe()\n");
    assert!(
        direct.starts_with("error[E0428]") && direct.contains("cycle A -> B -> A"),
        "the struct twin of the same two node cycle must still be rejected: {direct}"
    );
}

fn struct_over_enum_source(payload: &str) -> String {
    format!("struct A {{ e: E }}\nenum E {{ {payload} }}\nfn probe() -> int {{ 1 }}\nprobe()\n")
}

#[test]
fn an_uninhabited_nominal_off_every_cycle_is_accepted() {
    // construct written on purpose and must not be banned.
    assert_eq!(run_ok(&struct_over_enum_source("")).as_int(), Some(1));
    assert_eq!(
        run_ok("enum E { }\nfn probe() -> int { 1 }\nprobe()\n").as_int(),
        Some(1)
    );
    let on_cycle = run_err(&struct_over_enum_source("V(A)"));
    assert!(
        on_cycle.starts_with("error[E0428]") && on_cycle.contains("cycle A -> E -> A"),
        "the same struct over an enum that names it back must be rejected: {on_cycle}"
    );
}

#[test]
fn a_recursive_enum_with_a_terminating_variant_still_folds() {
    assert_eq!(
        run_ok("enum List { Cons(int, List), Nil }\nfn probe() -> int { 6 }\nprobe()\n").as_int(),
        Some(6)
    );
}

fn ordered_cycle_source(order: [&str; 3], padded: bool) -> String {
    let fields = [("A", "b: B"), ("B", "c: C"), ("C", "a: A")];
    let accused = order
        .iter()
        .position(|name| *name == "A")
        .expect("A is one of the three");
    let mut source = if padded {
        "\n".repeat(2 - accused)
    } else {
        String::new()
    };
    for name in order {
        let (_, field) = fields
            .iter()
            .find(|(declared, _)| *declared == name)
            .expect("every order names the same three structs");
        source.push_str(&format!("struct {name} {{ {field} }}\n"));
    }
    source.push_str("fn probe() -> int { 1 }\nprobe()\n");
    source
}

const CYCLE_ORDERS: [[&str; 3]; 6] = [
    ["A", "B", "C"],
    ["A", "C", "B"],
    ["B", "A", "C"],
    ["B", "C", "A"],
    ["C", "A", "B"],
    ["C", "B", "A"],
];

#[test]
fn every_declaration_order_of_one_cycle_renders_the_same_diagnostic() {
    let expected = run_err(&ordered_cycle_source(CYCLE_ORDERS[0], true));
    assert!(
        expected.contains("cycle A -> B -> C -> A") && expected.contains("field 'b' of struct 'A'"),
        "the accused definition must be a property of the cycle: {expected}"
    );
    for order in CYCLE_ORDERS {
        let rendered = run_err(&ordered_cycle_source(order, true));
        assert_eq!(
            rendered, expected,
            "the order {order:?} rendered a different diagnostic:\n{expected}\n----\n{rendered}"
        );
    }

    let sentence = |rendered: &str| {
        rendered
            .split_once("\n  --> ")
            .expect("the diagnostic carries a location")
            .0
            .to_string()
    };
    let baseline = sentence(&run_err(&ordered_cycle_source(CYCLE_ORDERS[0], false)));
    for order in CYCLE_ORDERS {
        let rendered = run_err(&ordered_cycle_source(order, false));
        assert_eq!(
            sentence(&rendered),
            baseline,
            "the order {order:?} accused a different definition: {rendered}"
        );
        let line = order
            .iter()
            .position(|name| *name == "A")
            .expect("A is one of the three")
            + 1;
        assert!(
            rendered.contains(&format!("--> test.aelys:{line}:12")),
            "the caret must sit on A's field wherever the order put it: {rendered}"
        );
    }

    // two programs under swapped order cannot show that the text does not move:
    let padded = ordered_cycle_source(CYCLE_ORDERS[0], true);
    for round in 1..20 {
        assert_eq!(
            run_err(&padded),
            expected,
            "compilation {round} of one unchanged cycle rendered other text"
        );
    }
}

#[test]
fn a_cycle_through_a_composite_field_is_rejected() {
    let error = run_err(
        r#"
struct Pair { l: A, r: A }
struct A { p: Pair }
fn probe() -> int { 1 }
probe()
        "#,
    );
    assert!(
        error.starts_with("error[E0428]") && error.contains("cycle A -> Pair -> A"),
        "a cycle closing through a two field composite must be rejected: {error}"
    );
    assert!(
        error.contains("field 'p' of struct 'A'"),
        "the field of the accused definition that opens the cycle must be named: {error}"
    );
    // the same composite with one side broken is inhabited and must run.
    assert_eq!(
        run_ok(
            r#"
struct Pair { l: int, r: int }
struct A { p: Pair }
fn probe() -> int {
    let a = A { p: Pair { l: 3, r: 4 } }
    return a.p.l + a.p.r
}
probe()
            "#
        )
        .as_int(),
        Some(7)
    );
}

#[test]
fn a_struct_field_cannot_be_written_through_self() {
    let error = run_err("struct A { a: Self }\nfn probe() -> int { 1 }\nprobe()\n");
    assert!(
        error.starts_with("error[E0372]") && error.contains("unknown type 'Self'"),
        "Self in field position must be reported as an unknown type: {error}"
    );
}

#[test]
fn a_rho_shaped_graph_accuses_a_definition_on_the_cycle() {
    // s is a tail into the cycle and can never be built either, but the path
    let error = run_err(
        r#"
struct S { a: A }
struct A { b: B }
struct B { a: A }
fn probe() -> int { 1 }
probe()
        "#,
    );
    assert!(
        error.starts_with("error[E0428]") && error.contains("cycle A -> B -> A"),
        "the printed path must close: {error}"
    );
    assert!(
        !error.contains("S ->") && error.contains("field 'b' of struct 'A'"),
        "the tail S must not appear on the printed cycle: {error}"
    );
    assert!(
        error.contains("--> test.aelys:3:12"),
        "the span must sit on A's field, not on S's: {error}"
    );
}

fn projected_cycle_source(field: &str, item: &str) -> String {
    format!(
        r#"
struct Counter {{ v: int }}
trait Source {{
    type Item
}}
impl Source for Counter {{
    type Item = {item}
}}
struct A {{ b: {field} }}
fn probe() -> int {{ 1 }}
probe()
"#
    )
}

#[test]
fn a_cycle_closed_through_a_projection_is_still_reported() {
    for field in ["A", "Counter::Item", "Source::Item"] {
        let error = run_err(&projected_cycle_source(field, "A"));
        assert!(
            error.starts_with("error[E0428]") && error.contains("cycle A -> A"),
            "the field written '{field}' must close the cycle: {error}"
        );
    }
    for field in ["int", "Counter::Item", "Source::Item"] {
        let source = projected_cycle_source(field, "int").replace(
            "fn probe() -> int { 1 }",
            "fn probe() -> int {\n    let a = A { b: 7 }\n    return a.b\n}",
        );
        assert_eq!(
            run_ok(&source).as_int(),
            Some(7),
            "the field written '{field}' over int must keep compiling"
        );
    }
}

// a generic struct named in field position is never materialized, whatever the
fn generic_field_source(argument: &str) -> String {
    format!(
        "struct Wrap<T> {{ v: T }}\nstruct A {{ w: Wrap<{argument}> }}\nfn probe() -> int {{ 1 }}\nprobe()\n"
    )
}

#[test]
fn a_generic_struct_in_field_position_points_at_the_field_that_wrote_it() {
    for argument in ["int", "A"] {
        let error = run_err(&generic_field_source(argument));
        assert!(
            error.starts_with("error[E0349]")
                && error.contains("generic type 'Wrap' was not materialized"),
            "Wrap<{argument}> in field position reports E0349 today: {error}"
        );
        assert!(
            error.contains("--> test.aelys:2:12"),
            "the caret must sit on the field, at line 2 column 12: {error}"
        );
        assert!(
            error.contains(&format!(" 2 | struct A {{ w: Wrap<{argument}> }}")),
            "the caret must sit under the carrier field, not the generic: {error}"
        );
    }
    assert_eq!(
        run_ok(
            "struct Wrap<T> { v: T }\nfn probe() -> int {\n    let w = Wrap { v: 7 }\n    return w.v\n}\nprobe()\n"
        )
        .as_int(),
        Some(7)
    );
}

fn two_generic_uses(carrier: &str, box_first: bool) -> String {
    let (wrap, boxed) = match carrier {
        "struct" => ("struct A { w: Wrap<int> }", "struct B { b: Box2<int> }"),
        "enum" => ("enum A { W(Wrap<int>) }", "enum B { B2(Box2<int>) }"),
        _ => ("    let a = Wrap { v: 3 }", "    let b = Box2 { v: 4 }"),
    };
    let (first, second) = if box_first {
        (boxed, wrap)
    } else {
        (wrap, boxed)
    };
    let body = if carrier == "value" {
        format!("fn probe() -> int {{\n{first}\n{second}\n    return a.v + b.v\n}}\n")
    } else {
        format!("{first}\n{second}\nfn probe() -> int {{ 1 }}\n")
    };
    format!("struct Wrap<T> {{ v: T }}\nstruct Box2<T> {{ v: T }}\n{body}probe()\n")
}

fn e0349_accusation(message: &str) -> (String, String) {
    let name = message
        .split_once("generic type '")
        .and_then(|(_, rest)| rest.split_once('\''))
        .map(|(name, _)| name.to_string())
        .unwrap_or_default();
    let carrier = message
        .lines()
        .find_map(|line| line.split_once(" | "))
        .map(|(_, text)| text.trim().to_string())
        .unwrap_or_default();
    (name, carrier)
}

#[test]
fn an_unmaterialized_applied_type_accuses_the_same_field_every_time() {
    // one compilation cannot see this defect: the accusation was drawn from an
    for carrier in ["struct", "enum"] {
        let mut answers = Vec::new();
        for _ in 0..20 {
            answers.push(e0349_accusation(&run_err(&two_generic_uses(
                carrier, false,
            ))));
        }
        assert!(
            answers.windows(2).all(|pair| pair[0] == pair[1]),
            "twenty compilations of one unchanged {carrier} source must accuse the same field: {answers:?}"
        );
        let reversed = e0349_accusation(&run_err(&two_generic_uses(carrier, true)));
        assert_eq!(
            answers[0].0, reversed.0,
            "the accused generic must not depend on which {carrier} is declared first"
        );
        assert_eq!(
            answers[0].1, reversed.1,
            "the accused site must not depend on which {carrier} is declared first"
        );
    }
    for box_first in [false, true] {
        assert_eq!(
            run_ok(&two_generic_uses("value", box_first)).as_int(),
            Some(7),
            "the value-position half must keep compiling"
        );
    }
}

#[test]
fn an_unmaterialized_applied_type_points_at_the_occurrence_that_wrote_it() {
    for (carrier, expected) in [
        ("struct", "struct B { b: Box2<int> }"),
        ("enum", "enum B { B2(Box2<int>) }"),
    ] {
        let message = run_err(&two_generic_uses(carrier, false));
        let line = message
            .lines()
            .find_map(|line| line.trim().strip_prefix("--> "))
            .and_then(|location| location.rsplit(':').nth(1))
            .and_then(|line| line.parse::<u32>().ok());
        assert!(
            line.is_some_and(|line| line > 0),
            "E0349 must point at a real source line: {message}"
        );
        let (name, carrier_text) = e0349_accusation(&message);
        assert_eq!(name, "Box2", "E0349 must name the generic: {message}");
        assert_eq!(
            carrier_text, expected,
            "the caret must sit under the occurrence that wrote it: {message}"
        );
    }
}

// one generator renders the chain at any length. the backend bounds a
fn projection_chain_in_field_position(links: usize) -> String {
    let mut source = String::from("struct Counter { v: int }\ntrait Source {\n");
    for index in 0..=links {
        source.push_str(&format!("    type T{index}\n"));
    }
    source.push_str("}\nimpl Source for Counter {\n    type T0 = int\n");
    for index in 1..=links {
        source.push_str(&format!("    type T{index} = Counter::T{}\n", index - 1));
    }
    source.push_str(&format!(
        "}}\nstruct Holder {{ it: Counter::T{links} }}\nfn probe() -> int {{\n    let h = Holder {{ it: 7 }}\n    return h.it\n}}\nprobe()\n"
    ));
    source
}

// length that must be rejected are the same program but for the last field.
fn nominal_chain(links: usize, closed: bool) -> String {
    let mut source = String::new();
    for index in 0..links {
        source.push_str(&format!("struct S{index} {{ f: S{} }}\n", index + 1));
    }
    let tail = if closed {
        "S0".to_string()
    } else {
        "int".to_string()
    };
    source.push_str(&format!("struct S{links} {{ f: {tail} }}\n"));
    source.push_str("fn probe() -> int { 1 }\nprobe()\n");
    source
}

#[test]
fn a_long_nominal_chain_is_settled_without_a_recursion() {
    // the inhabitation fixpoint runs on a worklist because a chain this long
    const LINKS: usize = 20_000;
    assert_eq!(run_ok(&nominal_chain(LINKS, false)).as_int(), Some(1));
    let closed = run_err(&nominal_chain(LINKS, true));
    assert!(
        closed.starts_with("error[E0428]") && closed.contains("cycle S0 -> S1 -> "),
        "the same chain closed on its head must be rejected: {}",
        &closed[..closed.len().min(160)]
    );
}

#[test]
fn a_projection_chain_in_field_position_compiles_on_both_sides_of_the_hop_bound() {
    for links in [64, 65] {
        assert_eq!(
            run_ok(&projection_chain_in_field_position(links)).as_int(),
            Some(7),
            "a {links} link chain in field position must compile"
        );
    }
}

#[test]
fn an_impl_type_parameter_is_in_scope_in_its_associated_type_definition() {
    assert_eq!(
        run_ok(
            r#"
struct Wrap<T> { v: T }
trait Source { type Item; fn next(self) -> Self::Item; }
impl<T> Source for Wrap<T> {
    type Item = T
    fn next(self) -> Self::Item { self.v }
}
Wrap { v: 7 }.next()
            "#
        )
        .as_int(),
        Some(7)
    );
}

#[test]
fn an_impl_type_parameter_in_an_associated_type_answers_per_instantiation() {
    run_ok_string(
        r#"
struct Wrap<T> { v: T }
trait Source { type Item; fn next(self) -> Self::Item; }
impl<T> Source for Wrap<T> {
    type Item = T
    fn next(self) -> Self::Item { self.v }
}
fn probe() -> string {
    let n: int = Wrap { v: 20 }.next()
    let s: string = Wrap { v: "ab" }.next()
    if n == 20 {
        return s + "-20"
    }
    return s + "-no"
}
probe()
        "#,
        "ab-20",
    );
}

#[test]
fn an_impl_type_parameter_reaches_a_composite_associated_type() {
    assert_eq!(
        run_ok(
            r#"
struct Wrap<T> { v: T }
trait Source { type Item; fn next(self) -> Self::Item; }
impl<T> Source for Wrap<T> {
    type Item = Option<T>
    fn next(self) -> Self::Item { return Some(self.v) }
}
Wrap { v: 7 }.next().unwrap()
            "#
        )
        .as_int(),
        Some(7)
    );
}

#[test]
fn both_impl_type_parameters_are_in_scope_in_their_item_definitions() {
    // one side alone cannot tell `type right = b` from `type right = a`, so both are read
    run_ok_string(
        r#"
struct Pair<A, B> { a: A, b: B }
trait Sides { type Left; type Right; fn left(self) -> Self::Left; fn right(self) -> Self::Right; }
impl<A, B> Sides for Pair<A, B> {
    type Left = A
    type Right = B
    fn left(self) -> Self::Left { self.a }
    fn right(self) -> Self::Right { self.b }
}
fn probe() -> string {
    let p = Pair { a: 7, b: "x" }
    let left: int = p.left()
    let right: string = p.right()
    let q = Pair { a: "y", b: 9 }
    let swapped_left: string = q.left()
    let swapped_right: int = q.right()
    return right + swapped_left + "-" + (left + swapped_right).to_string()
}
probe()
            "#,
        "xy-16",
    );
}

#[test]
fn an_instantiation_the_impl_definition_refuses_is_still_a_mismatch() {
    let error = run_err(
        r#"
struct Wrap<T> { v: T }
trait Source { type Item; fn next(self) -> Self::Item; }
impl<T> Source for Wrap<T> {
    type Item = T
    fn next(self) -> Self::Item { self.v }
}
fn probe() -> int {
    let n: string = Wrap { v: 12 }.next()
    return 0
}
probe()
        "#,
    );
    assert!(
        error.starts_with("error[E0301]") && error.contains("expected string, found int"),
        "the projection must substitute the instantiation, not a wildcard: {error}"
    );
}

#[test]
fn a_type_parameter_only_in_an_item_definition_is_still_unconstrained() {
    let error = run_err(
        r#"
struct Cell { c: int }
trait Source { type Item; fn next(self) -> Self::Item; }
impl<T> Source for Cell {
    type Item = T
    fn next(self) -> Self::Item { return self.c }
}
Cell { c: 7 }.next()
        "#,
    );
    assert!(
        error.starts_with("error[E0430]") && error.contains("'T' does not appear in the impl"),
        "a parameter the target type never mentions stays unconstrained: {error}"
    );
}

#[test]
fn an_associated_constant_declared_with_an_impl_parameter_names_the_disagreement() {
    let error = run_err(
        r#"
struct Wrap<T> { v: T }
trait HasSeed { const SEED: int; fn seed(self) -> int; }
impl<T> HasSeed for Wrap<T> {
    const SEED: T = 4
    fn seed(self) -> int { Self::SEED }
}
Wrap { v: 7 }.seed()
        "#,
    );
    assert!(
        error.starts_with("error[E0422]")
            && error.contains("declares type 'T', and the trait declares 'int' here"),
        "the impl parameter is in scope, so the cause is the disagreement, and the message \
         names each side: {error}"
    );
}

#[test]
fn an_inherent_impl_still_owns_no_associated_item_under_a_type_parameter() {
    let error = run_err(
        r#"
struct Wrap<T> { v: T }
impl<T> Wrap<T> {
    type Item = T
    fn get(self) -> T { self.v }
}
Wrap { v: 7 }.get()
        "#,
    );
    assert!(
        error.starts_with("error[E0425]") && error.contains("inherent impl of 'Wrap'"),
        "an inherent impl defines no associated item at all: {error}"
    );
}

#[test]
fn a_concrete_impl_of_a_generic_target_keeps_its_associated_type() {
    assert_eq!(
        run_ok(
            r#"
struct Wrap<T> { v: T }
trait Source { type Item; fn next(self) -> Self::Item; }
impl Source for Wrap<int> {
    type Item = int
    fn next(self) -> Self::Item { self.v }
}
Wrap { v: 7 }.next()
            "#
        )
        .as_int(),
        Some(7)
    );
}

#[test]
fn an_inherited_item_written_in_the_inheriting_impl_is_reported_at_that_impl() {
    // a projection, and the inheriting impl must not become a second definition.
    let error = run_err(
        r#"
trait Base {
    type Item
    const LIMIT: int
    fn base(self) -> Self::Item;
}
trait Derived: Base {
    fn extra(self) -> Self::Item { self.base() }
    fn cap(self) -> int { Self::LIMIT }
}
struct Bullet { n: int }
impl Base for Bullet {
    type Item = int
    const LIMIT: int = 4
    fn base(self) -> int { self.n }
}
impl Derived for Bullet {
    type Item = int
}
Bullet { n: 3 }.extra() + Bullet { n: 3 }.cap()
        "#,
    );
    assert!(
        error.starts_with("error[E0425]")
            && error.contains("impl of trait 'Derived' for 'Bullet', which does not declare it")
            && error.contains("type Item = int"),
        "the redundant definition is the offence, and it is where the caret goes: {error}"
    );
}

#[test]
fn a_missing_supertrait_impl_is_reported_before_the_projection_it_leaves_open() {
    let error = run_err(
        r#"
trait Base {
    type Item
    const LIMIT: int
    fn base(self) -> Self::Item;
}
trait Derived: Base {
    fn extra(self) -> Self::Item { self.base() }
    fn cap(self) -> int { Self::LIMIT }
}
struct Bullet { n: int }
impl Derived for Bullet { }
Bullet { n: 3 }.extra()
        "#,
    );
    assert!(
        error.starts_with("error[E0338]")
            && error.contains("trait 'Base' is not implemented for Bullet")
            && error.contains("impl Derived for Bullet"),
        "the unmet obligation is the cause, not the projection it leaves open: {error}"
    );
}

#[derive(Clone, Copy)]
enum GateCallShape {
    Direct,
    Bound,
}

const EVERY_GATE_CALL_SHAPE: &[GateCallShape] = &[GateCallShape::Direct, GateCallShape::Bound];

fn supertrait_gate_program(
    base_impl: bool,
    use_above: bool,
    member: &str,
    shape: GateCallShape,
) -> String {
    let head = concat!(
        "struct Bullet { n: int }\n",
        "trait Base {\n    fn base(self) -> int\n}\n",
        "trait Derived: Base {\n    fn extra(self) -> int { self.base() * 7 }\n}\n",
    );
    let base = if base_impl {
        "impl Base for Bullet {\n    fn base(self) -> int { self.n }\n}\n"
    } else {
        ""
    };
    let call = match shape {
        GateCallShape::Direct => {
            format!("fn call_it(x: Bullet) -> int {{ return x.{member}() }}\n")
        }
        GateCallShape::Bound => {
            format!("fn call_it<T: Derived>(x: T) -> int {{ return x.{member}() }}\n")
        }
    };
    let derived = "impl Derived for Bullet { }\n";
    let body = if use_above {
        format!("{call}{derived}")
    } else {
        format!("{derived}{call}")
    };
    format!("{head}{base}{body}call_it(Bullet {{ n: 6 }})\n")
}

#[test]
fn a_missing_supertrait_impl_is_reported_wherever_the_use_stands() {
    for use_above in [true, false] {
        for shape in EVERY_GATE_CALL_SHAPE {
            let error = run_err(&supertrait_gate_program(false, use_above, "extra", *shape));
            assert!(
                error.starts_with("error[E0338]")
                    && error.contains("trait 'Base' is not implemented for Bullet")
                    && error.contains("the impl of 'Derived' requires it"),
                "the unmet obligation is the cause, with the use above the impl {use_above}: {error}"
            );
            assert_eq!(
                run_ok(&supertrait_gate_program(true, use_above, "extra", *shape)).as_int(),
                Some(42),
                "the adopted default must run the 6 through 'base' times seven"
            );
        }
        let absent = run_err(&supertrait_gate_program(
            false,
            use_above,
            "nope",
            GateCallShape::Direct,
        ));
        if use_above {
            assert!(
                absent.starts_with("error[E0363]") && absent.contains("'nope'"),
                "a name no trait declares is absent, not an obligation: {absent}"
            );
        } else {
            assert!(
                absent.starts_with("error[E0338]"),
                "the impl header still outranks a later absence by span: {absent}"
            );
        }
    }
}

#[test]
fn the_inherited_associated_item_block_of_the_specification_runs() {
    assert_eq!(
        run_ok(
            r#"
trait Base {
    type Item
    const LIMIT: int
    fn base(self) -> Self::Item;
}
trait Derived: Base {
    fn extra(self) -> Self::Item { self.base() }
    fn cap(self) -> int { Self::LIMIT }
}
struct Bullet { n: int }
impl Base for Bullet {
    type Item = int
    const LIMIT: int = 4
    fn base(self) -> int { self.n }
}
impl Derived for Bullet { }
Bullet { n: 3 }.extra() + Bullet { n: 3 }.cap()
            "#
        )
        .as_int(),
        Some(7)
    );
}

#[test]
fn an_associated_binding_reads_the_impl_definition_at_the_instantiation() {
    assert_eq!(
        run_ok(
            r#"
struct Wrap<T> { v: T }
trait Source { type Item; fn next(self) -> Self::Item; }
impl<T> Source for Wrap<T> {
    type Item = T
    fn next(self) -> Self::Item { self.v }
}
fn takes<S: Source<Item = int>>(s: S) -> int { return 1 }
takes(Wrap { v: 5 })
            "#
        )
        .as_int(),
        Some(1)
    );
}

#[test]
fn an_associated_binding_refuses_the_instantiation_the_impl_does_not_provide() {
    let error = run_err(
        r#"
struct Wrap<T> { v: T }
trait Source { type Item; fn next(self) -> Self::Item; }
impl<T> Source for Wrap<T> {
    type Item = T
    fn next(self) -> Self::Item { self.v }
}
fn takes<S: Source<Item = int>>(s: S) -> int { return 1 }
takes(Wrap { v: "x" })
        "#,
    );
    assert!(
        error.starts_with("error[E0424]") && error.contains("which provides string"),
        "the impl definition must be read at the instantiation, not left a parameter: {error}"
    );
}

fn one_trait_two_instantiations_const_source(order: usize) -> String {
    let int_impl = "impl Source for Wrap<int> { const LIMIT: int = 20; }";
    let string_impl = "impl Source for Wrap<string> { const LIMIT: int = 77; }";
    let (before, after) = if order == 0 {
        (int_impl, string_impl)
    } else {
        (string_impl, int_impl)
    };
    format!(
        "trait Source {{ const LIMIT: int; }}\nstruct Wrap<T> {{ v: T }}\n{before}\n{after}\nfn probe() -> int {{ return Wrap::LIMIT }}\nprobe()"
    )
}

fn one_trait_two_instantiations_type_source(order: usize) -> String {
    let int_impl = "impl Source for Wrap<int> { type Item = int; }";
    let string_impl = "impl Source for Wrap<string> { type Item = string; }";
    let (before, after) = if order == 0 {
        (int_impl, string_impl)
    } else {
        (string_impl, int_impl)
    };
    format!(
        "trait Source {{ type Item; }}\nstruct Wrap<T> {{ v: T }}\n{before}\n{after}\nfn probe(x: Wrap::Item) -> int {{ return 1 }}\nprobe(3)"
    )
}

#[test]
fn a_constant_on_two_instantiations_of_one_type_answers_the_same_whatever_the_file_order() {
    let first_source = one_trait_two_instantiations_const_source(0);
    let second_source = one_trait_two_instantiations_const_source(1);
    assert_same_lines(&first_source, &second_source);

    let first = render(&first_source);
    let second = render(&second_source);
    assert_eq!(
        first, second,
        "file order decided the constant:\n{first}\n----\n{second}"
    );
    let expected = concat!(
        "error[E0423]: projection 'Wrap::LIMIT' is ambiguous: 'Source' is implemented for",
        " Wrap<int> and Wrap<string>, and each defines 'LIMIT'; a projection names 'Wrap'",
        " without its type arguments and this language has no way to write them there, so",
        " neither naming the trait nor naming the type separates the definitions; keep a",
        " single impl of 'Source' for 'Wrap', or declare 'LIMIT' in a second trait and name",
        " that trait, as in 'OtherTrait::LIMIT' (a value expression)\n",
        "  --> test.aelys:5:28\n",
        "   |\n",
        " 5 | fn probe() -> int { return Wrap::LIMIT }\n",
        "   |                            ^^^^^^^^^^^ the type checker rejected this program\n",
    );
    assert_eq!(first, expected);
}

#[test]
fn an_associated_type_on_two_instantiations_of_one_type_names_the_instantiations() {
    let first_source = one_trait_two_instantiations_type_source(0);
    let second_source = one_trait_two_instantiations_type_source(1);
    assert_same_lines(&first_source, &second_source);

    let first = render(&first_source);
    let second = render(&second_source);
    assert_eq!(
        first, second,
        "file order decided the projection:\n{first}\n----\n{second}"
    );
    let expected = concat!(
        "error[E0423]: projection 'Wrap::Item' is ambiguous: 'Source' is implemented for",
        " Wrap<int> and Wrap<string>, and each defines 'Item'; a projection names 'Wrap'",
        " without its type arguments and this language has no way to write them there, so",
        " neither naming the trait nor naming the type separates the definitions; keep a",
        " single impl of 'Source' for 'Wrap', or declare 'Item' in a second trait and name",
        " that trait, as in 'OtherTrait::Item' (a parameter type)\n",
        "  --> test.aelys:5:13\n",
        "   |\n",
        " 5 | fn probe(x: Wrap::Item) -> int { return 1 }\n",
        "   |             ^^^^ the type checker rejected this program\n",
    );
    assert_eq!(first, expected);
}

#[test]
fn naming_the_trait_does_not_separate_two_instantiations_of_one_type() {
    let source = concat!(
        "trait Source { const LIMIT: int; }\n",
        "struct Wrap<T> { v: T }\n",
        "impl Source for Wrap<int> { const LIMIT: int = 20; }\n",
        "impl Source for Wrap<string> { const LIMIT: int = 77; }\n",
        "fn probe() -> int { return Source::LIMIT }\n",
        "probe()",
    );
    let rendered = render(source);
    assert!(
        rendered.contains("projection 'Source::LIMIT' is ambiguous: 'Source' is implemented for Wrap<int> and Wrap<string>"),
        "the trait-qualified spelling must reach the same answer, not the first impl:\n{rendered}"
    );
}

#[test]
fn naming_the_trait_does_not_separate_two_instantiations_for_an_associated_type() {
    let source = concat!(
        "trait Source { type Item; }\n",
        "struct Wrap<T> { v: T }\n",
        "impl Source for Wrap<int> { type Item = int; }\n",
        "impl Source for Wrap<string> { type Item = string; }\n",
        "fn probe(x: Source::Item) -> int { return 1 }\n",
        "probe(3)",
    );
    let rendered = render(source);
    assert!(
        rendered.contains("projection 'Source::Item' is ambiguous: 'Source' is implemented for Wrap<int> and Wrap<string>"),
        "the trait-qualified spelling must name the instantiations, not say 'Wrap both implement':\n{rendered}"
    );
}

#[test]
fn keeping_a_single_impl_is_a_repair_that_runs() {
    let result = run_ok(
        r#"
trait Source { const LIMIT: int; }
struct Wrap<T> { v: T }
impl Source for Wrap<int> { const LIMIT: int = 20; }
fn probe() -> int { return Wrap::LIMIT }
probe()
"#,
    );
    assert_eq!(result.as_int(), Some(20));
}

#[test]
fn a_second_trait_is_a_repair_that_runs_and_each_trait_answers_its_own_constant() {
    let source = concat!(
        "trait Source { const LIMIT: int; }\n",
        "trait Ceiling { const LIMIT: int; }\n",
        "struct Wrap<T> { v: T }\n",
        "impl Source for Wrap<int> { const LIMIT: int = 20; }\n",
        "impl Ceiling for Wrap<string> { const LIMIT: int = 77; }\n",
    );
    let from_source = run_ok(&format!(
        "{source}fn probe() -> int {{ return Source::LIMIT }}\nprobe()"
    ));
    assert_eq!(from_source.as_int(), Some(20));
    let from_ceiling = run_ok(&format!(
        "{source}fn probe() -> int {{ return Ceiling::LIMIT }}\nprobe()"
    ));
    assert_eq!(
        from_ceiling.as_int(),
        Some(77),
        "the second trait must answer its own definition, not the first impl's"
    );
}
