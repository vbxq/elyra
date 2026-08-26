use aelys::{CompileOptions, Runtime, run};
use aelys_runtime::Value;

fn run_ok(source: &str) -> Value {
    run(source, "method_call_tests.aelys").expect("program should run")
}

fn compile_result(source: &str) -> Result<(), String> {
    Runtime::new()
        .compile(source, CompileOptions::default())
        .map(|_| ())
        .map_err(|error| error.to_string())
}

// sum3 stays past the inline threshold, or the occupied argument window never forms
const D2_PRELUDE: &str = r#"
struct P { v: int }
impl P { fn base(self) -> int { self.v } }
trait Scale { fn scale(self) -> int; fn twice(self) -> int { 2 } }
impl Scale for P { fn scale(self) -> int { self.v * 10 } }
fn sum3(a: int, b: int, c: int) -> int {
    let mut total = 0
    let mut i = 0
    while i < 1 {
        total = a * 100 + b * 10 + c
        i = i + 1
    }
    total
}
"#;

fn run_d2(body: &str) -> Value {
    let mut source = String::from(D2_PRELUDE);
    source.push_str(body);
    run_ok(&source)
}

#[test]
fn trait_method_resolves_on_enum_receiver() {
    let result = run_ok(
        r#"
trait Get { fn get(self) -> int; }
enum E { A(int), B }
impl Get for E { fn get(self) -> int { match self { E::A(n) => n, E::B => 0 } } }
let e = E::A(7)
e.get()
"#,
    );
    assert_eq!(result.as_int(), Some(7));
}

#[test]
fn inherent_method_resolves_on_enum_receiver() {
    let result = run_ok(
        r#"
enum Counter { Zero, N(int) }
impl Counter {
    fn value(self) -> int { match self { Counter::Zero => 0, Counter::N(n) => n } }
}
let c = Counter::N(9)
c.value()
"#,
    );
    assert_eq!(result.as_int(), Some(9));
}

#[test]
fn trait_default_body_resolves_on_enum_receiver() {
    let result = run_ok(
        r#"
trait Named { fn tag(self) -> int; fn label(self) -> int { self.tag() + 42 } }
enum Flag { On }
impl Named for Flag { fn tag(self) -> int { 5 } }
let f = Flag::On
f.label()
"#,
    );
    assert_eq!(result.as_int(), Some(47));
}

#[test]
fn trait_method_dispatch_is_observable_across_two_enums() {
    let result = run_ok(
        r#"
trait Code { fn code(self) -> int; }
enum Alpha { One, Two(int) }
enum Beta { Left, Right(int) }
impl Code for Alpha { fn code(self) -> int { match self { Alpha::One => 1, Alpha::Two(n) => n } } }
impl Code for Beta { fn code(self) -> int { match self { Beta::Left => 100, Beta::Right(n) => n * 100 } } }
let a = Alpha::Two(7)
let b = Beta::Right(3)
a.code() * 1000 + b.code()
"#,
    );
    assert_eq!(result.as_int(), Some(7300));
}

#[test]
fn inherent_method_in_first_of_three_arguments_keeps_receiver() {
    let result = run_d2(
        r#"
let p = P { v: 4 }
sum3(p.base(), 2, 3)
"#,
    );
    assert_eq!(result.as_int(), Some(423));
}

#[test]
fn inherent_method_in_second_of_three_arguments_keeps_receiver() {
    let result = run_d2(
        r#"
let p = P { v: 4 }
sum3(1, p.base(), 3)
"#,
    );
    assert_eq!(result.as_int(), Some(143));
}

#[test]
fn trait_method_in_non_final_argument_keeps_receiver() {
    let result = run_d2(
        r#"
let p = P { v: 4 }
sum3(1, p.scale(), 3)
"#,
    );
    assert_eq!(result.as_int(), Some(503));
}

#[test]
fn trait_default_method_in_non_final_argument_keeps_receiver() {
    let result = run_d2(
        r#"
let p = P { v: 4 }
sum3(1, p.twice(), 3)
"#,
    );
    assert_eq!(result.as_int(), Some(123));
}

#[test]
fn inherent_method_in_non_final_array_element_keeps_receiver() {
    let result = run_d2(
        r#"
let p = P { v: 4 }
let cells = [1, p.base(), 3]
cells[0] * 100 + cells[1] * 10 + cells[2]
"#,
    );
    assert_eq!(result.as_int(), Some(143));
}

#[test]
fn trait_method_in_non_final_array_element_keeps_receiver() {
    let result = run_d2(
        r#"
let p = P { v: 4 }
let cells = [1, p.scale(), 3]
cells[0] * 1000 + cells[1] * 10 + cells[2]
"#,
    );
    assert_eq!(result.as_int(), Some(1403));
}

#[test]
fn method_in_final_argument_still_works() {
    let result = run_d2(
        r#"
let p = P { v: 4 }
sum3(1, 2, p.base())
"#,
    );
    assert_eq!(result.as_int(), Some(124));
}

#[test]
fn method_in_final_array_element_still_works() {
    let result = run_d2(
        r#"
let p = P { v: 4 }
let cells = [1, 2, p.base()]
cells[0] * 100 + cells[1] * 10 + cells[2]
"#,
    );
    assert_eq!(result.as_int(), Some(124));
}

#[test]
fn method_call_in_non_final_argument_of_an_enum_receiver() {
    let result = run_d2(
        r#"
enum Tally { Some(int), None }
impl Tally { fn amount(self) -> int { match self { Tally::Some(n) => n, Tally::None => 0 } } }
let t = Tally::Some(6)
sum3(1, t.amount(), 3)
"#,
    );
    assert_eq!(result.as_int(), Some(163));
}

#[test]
fn associated_function_resolves_on_an_enum_path() {
    let result = run_ok(
        r#"
enum E { A(int), B }
impl E {
    fn zero() -> E { E::B }
    fn amount(self) -> int { match self { E::A(n) => n, E::B => 4 } }
}
let z = E::zero()
z.amount()
"#,
    );
    assert_eq!(result.as_int(), Some(4));
}

#[test]
fn unknown_enum_path_member_is_still_an_unknown_variant() {
    let error = compile_result(
        r#"
enum E { A(int), B }
impl E { fn zero() -> E { E::B } }
fn use_it() -> int { E::nope }
"#,
    )
    .expect_err("an unknown enum path member should be rejected");
    assert!(
        error.contains("E0109"),
        "expected the unknown variant diagnostic, got: {}",
        error
    );
}

fn wide_instantiation_source(count: usize) -> String {
    let mut source = String::from("fn tag<T>(x: T) -> int { 1 }\n");
    for index in 0..count {
        source.push_str(&format!("struct S{} {{ v: int }}\n", index));
    }
    source.push_str("fn total() -> int {\n    let mut acc = 0\n");
    for index in 0..count {
        source.push_str(&format!("    acc = acc + tag(S{} {{ v: 1 }})\n", index));
    }
    source.push_str("    acc\n}\n");
    source
}

fn deep_instantiation_source(depth: usize) -> String {
    let mut source = String::from("fn g0<T>(x: T) -> int { 1 }\n");
    for index in 1..depth {
        source.push_str(&format!(
            "fn g{}<T>(x: T) -> int {{ g{}(x) + 1 }}\n",
            index,
            index - 1
        ));
    }
    source.push_str(&format!("fn top() -> int {{ g{}(7) }}\n", depth - 1));
    source
}

#[test]
fn more_than_one_thousand_distinct_instantiations_compile() {
    let source = wide_instantiation_source(1_030);
    assert_eq!(compile_result(&source), Ok(()));
}

#[test]
fn active_instantiation_stack_deeper_than_the_limit_is_rejected() {
    let source = deep_instantiation_source(1_100);
    let error = compile_result(&source).expect_err("deep instantiation chain should be rejected");
    assert!(
        error.contains("E0345"),
        "expected the instantiation limit diagnostic, got: {}",
        error
    );
}

#[test]
fn active_instantiation_stack_at_the_limit_still_compiles() {
    let source = deep_instantiation_source(1_000);
    assert_eq!(compile_result(&source), Ok(()));
}

#[test]
fn borrowed_receiver_executes() {
    let result = run_ok(
        r#"
struct Point { x: int }
impl Point { fn read(&self) -> int { self.x } }
fn use_point() -> int {
    let point = Point { x: 4 }
    point.read()
}
use_point()
"#,
    );
    assert_eq!(result.as_int(), Some(4));
}

fn assert_borrow_error(source: &str, code: &str, fragment: &str) {
    let error = compile_result(source).expect_err("borrow source should be rejected");
    assert!(
        error.contains(code),
        "expected {code} in diagnostic: {error}"
    );
    assert!(
        error.contains(fragment),
        "expected {fragment:?} in diagnostic: {error}"
    );
}

#[test]
fn shared_receiver_cannot_mutate_e0414() {
    assert_borrow_error(
        r#"
struct Point { x: int }
impl Point { fn bad(&self) -> int { self.x = 3; self.x } }
fn use_point() -> int {
    let point = Point { x: 1 }
    point.bad()
}
use_point()
"#,
        "E0414",
        "shared loan",
    );
}

#[test]
fn two_mutable_loans_conflict_e0415() {
    assert_borrow_error(
        r#"
struct Point { x: int }
fn clash(a: &mut Point, b: &mut Point) -> int { a.x + b.x }
let point = Point { x: 1 }
clash(&mut point, &mut point)
"#,
        "E0415",
        "overlap",
    );
}

#[test]
fn read_over_mutable_loan_e0416() {
    assert_borrow_error(
        r#"
struct Point { x: int }
fn clash(a: &mut Point, b: &Point) -> int { a.x + b.x }
let mut point = Point { x: 1 }
clash(&mut point, point)
"#,
        "E0416",
        "mutable loan",
    );
}

#[test]
fn borrow_escape_e0417() {
    assert_borrow_error(
        r#"
struct Point { x: int }
fn leak(point: &Point) -> &Point { point }
"#,
        "E0417",
        "escapes",
    );
}

#[test]
fn borrow_invalidation_e0418() {
    assert_borrow_error(
        r#"
struct Point { x: int }
fn observe(point: &Point) -> int { point.x }
fn bad(point: &mut Point) -> int {
    observe(point)
    point.x = 2
    point.x
}
let point = Point { x: 1 }
bad(&mut point)
"#,
        "E0418",
        "invalidated",
    );
}

#[test]
fn temporary_borrow_e0419() {
    assert_borrow_error(
        r#"
struct Point { x: int }
fn read(point: &Point) -> int { point.x }
read(&Point { x: 1 })
"#,
        "E0419",
        "temporary",
    );
}
