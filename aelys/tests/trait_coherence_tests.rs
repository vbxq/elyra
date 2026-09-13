use std::fs;
use std::path::PathBuf;

use aelys::{CompileOptions, Runtime, run};
use aelys_bytecode::Function;
use aelys_bytecode::asm::{deserialize, disassemble};
use tempfile::TempDir;

fn compiled(source: &str) -> Function {
    let module = Runtime::new()
        .compile(source, CompileOptions::default())
        .unwrap_or_else(|error| panic!("the program must compile: {error}\n{source}"));
    deserialize(module.avbc()).expect("the compiled module must deserialize")
}

fn compile_message(source: &str) -> String {
    match Runtime::new().compile(source, CompileOptions::default()) {
        Ok(_) => panic!("the program must be refused:\n{source}"),
        Err(error) => error.to_string(),
    }
}

fn value_of(source: &str) -> i64 {
    run(source, "test.aelys")
        .unwrap_or_else(|error| panic!("the program must run: {error}\n{source}"))
        .as_int()
        .expect("the probe returns an int")
}

fn walk(function: &Function, visit: &mut dyn FnMut(&Function)) {
    visit(function);
    for nested in &function.nested_functions {
        walk(nested, visit);
    }
}

fn trait_symbols(function: &Function) -> Vec<String> {
    let mut names = Vec::new();
    walk(function, &mut |function| {
        if let Some(name) = &function.name
            && name.starts_with("__aelys_trait::")
        {
            names.push(name.clone());
        }
    });
    names.sort();
    names.dedup();
    names
}

fn bodies_of(function: &Function, method: &str) -> Vec<String> {
    let suffix = format!("{:08x}:{method}", method.len());
    trait_symbols(function)
        .into_iter()
        .filter(|name| {
            name.split("$s2$")
                .next()
                .is_some_and(|base| base.ends_with(&suffix))
        })
        .collect()
}

/// a call names the impl it took; the returned value does not, and two
fn calls_of(function: &Function, caller: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    walk(function, &mut |candidate| {
        if candidate.name.as_deref() != Some(caller) {
            return;
        }
        // disassemble walks nested functions too, so keep only the lines between
        let text = disassemble(candidate);
        let mut headers = 0usize;
        let mut own = Vec::new();
        for line in text.lines() {
            if line.trim_start().starts_with(".name") {
                headers += 1;
                if headers > 1 {
                    break;
                }
                continue;
            }
            if headers == 1 {
                own.push(line);
            }
        }
        for line in own {
            let Some((instruction, comment)) = line.split_once(';') else {
                continue;
            };
            if !instruction.contains("Call") {
                continue;
            }
            let symbol = comment.trim().trim_end_matches("()");
            if symbol.starts_with("__aelys_trait::") {
                found.push(symbol.to_string());
            }
        }
    });
    found
}

fn in_module(files: &[(&str, &str)]) -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut entry = PathBuf::new();
    for (name, body) in files {
        let path = dir.path().join(name);
        fs::write(&path, body).expect("write module");
        if *name == "main.aelys" {
            entry = path;
        }
    }
    (dir, entry)
}

fn module_value(files: &[(&str, &str)]) -> i64 {
    let (_dir, entry) = in_module(files);
    aelys_driver::run_file(&entry)
        .unwrap_or_else(|error| panic!("the program must run: {error}"))
        .as_int()
        .expect("the probe returns an int")
}

const SPECIALIZED: &str = "\
trait Describe {
    fn describe(self) -> int;
}
struct Holder<T> { value: T }
impl<T> Describe for Holder<T> {
    default fn describe(self) -> int { 1 }
}
impl Describe for Holder<int> {
    fn describe(self) -> int { 2 }
}
fn probe() -> int {
    let a = Holder { value: true }
    let b = Holder { value: 7 }
    a.describe() * 10 + b.describe()
}
probe()
";

const SPECIALIZED_REVERSED: &str = "\
trait Describe {
    fn describe(self) -> int;
}
struct Holder<T> { value: T }
impl Describe for Holder<int> {
    fn describe(self) -> int { 2 }
}
impl<T> Describe for Holder<T> {
    default fn describe(self) -> int { 1 }
}
fn probe() -> int {
    let a = Holder { value: true }
    let b = Holder { value: 7 }
    a.describe() * 10 + b.describe()
}
probe()
";

/// the instance infix the monomorphizer writes for a generic impl body
const INSTANCE_INFIX: &str = "$s2$";

/// a mangled name carries the nominal in clear, length prefixed; only the
fn segment_of(name: &str) -> String {
    format!("{:08x}:{name}", name.len())
}

fn hex_of(text: &str) -> String {
    text.bytes().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn valid_specialization_is_deterministic() {
    assert_eq!(
        value_of(SPECIALIZED),
        12,
        "the generic body answers 1 for Holder<bool>, the specialization answers 2 for Holder<int>"
    );

    let function = compiled(SPECIALIZED);
    let bodies = bodies_of(&function, "describe");
    assert_eq!(
        bodies.len(),
        2,
        "the generic body and the specialization must both reach the artifact: {bodies:?}"
    );

    // the symbol a wrong selection would mint: the generic impl instantiated at int
    let wrong = hex_of("3:i64");
    assert!(
        !bodies
            .iter()
            .any(|name| name.contains(INSTANCE_INFIX) && name.contains(&wrong)),
        "the generic impl was instantiated at int, so the specialization was not selected: {bodies:?}"
    );
    assert!(
        bodies.iter().any(|name| !name.contains(INSTANCE_INFIX)),
        "the concrete specialization must have a body of its own: {bodies:?}"
    );

    let calls = calls_of(&function, "probe");
    assert_eq!(calls.len(), 2, "probe must call twice: {calls:?}");
    assert_ne!(
        calls[0], calls[1],
        "the two call sites took the same impl: {calls:?}"
    );
}

#[test]
fn reversed_source_order_selects_the_same_impl() {
    assert_eq!(value_of(SPECIALIZED), 12);
    assert_eq!(
        value_of(SPECIALIZED_REVERSED),
        12,
        "source order decided the value"
    );

    let forward = calls_of(&compiled(SPECIALIZED), "probe");
    let reversed = calls_of(&compiled(SPECIALIZED_REVERSED), "probe");
    assert_eq!(
        forward, reversed,
        "source order decided which impl each call site selected"
    );
}

#[test]
fn an_impl_symbol_carries_its_header_spelling() {
    let source = "\
trait Describe {
    fn describe(self) -> int;
}
struct Holder<T> { value: T }
impl<T> Describe for Holder<T> {
    fn describe(self) -> int { 1 }
}
fn probe() -> int {
    let h = Holder { value: 7 }
    h.describe()
}
probe()
";
    let bodies = bodies_of(&compiled(source), "describe");
    assert_eq!(bodies.len(), 1, "{bodies:?}");
    let base = bodies[0]
        .split(INSTANCE_INFIX)
        .next()
        .expect("a symbol has a base");
    assert_eq!(
        base, "__aelys_trait::00000008:Describe0000000a:Holder<$0>00000008:describe",
        "a symbol must be a function of its header alone, so two headers never collide"
    );
}

#[test]
fn an_impl_on_a_plain_nominal_keeps_its_symbol() {
    let source = "\
trait Describe {
    fn describe(self) -> int;
}
struct Point { x: int }
impl Describe for Point {
    fn describe(self) -> int { self.x }
}
fn probe() -> int {
    let p = Point { x: 7 }
    p.describe()
}
probe()
";
    let bodies = bodies_of(&compiled(source), "describe");
    assert_eq!(bodies.len(), 1, "{bodies:?}");
    assert_eq!(
        bodies[0], "__aelys_trait::00000008:Describe00000005:Point00000008:describe",
        "a plain nominal spells as its own name, so no artifact of an ordinary program moves"
    );
}

#[test]
fn a_negative_impl_refuses_the_call_it_denies() {
    let message = compile_message(
        "\
trait Describe {
    fn describe(self) -> int;
}
struct Holder<T> { value: T }
impl<T> Describe for Holder<T> {
    default fn describe(self) -> int { 1 }
}
impl !Describe for Holder<bool> {}
fn probe() -> int {
    let h = Holder { value: true }
    h.describe()
}
probe()
",
    );
    assert!(
        message.contains("error[E0338]"),
        "a denied trait is not implemented, and the call must say so: {message}"
    );

    let source = "\
trait Describe {
    fn describe(self) -> int;
}
struct Holder<T> { value: T }
impl<T> Describe for Holder<T> {
    default fn describe(self) -> int { 1 }
}
impl !Describe for Holder<bool> {}
fn probe() -> int {
    let h = Holder { value: 7 }
    h.describe()
}
probe()
";
    assert_eq!(
        value_of(source),
        1,
        "the negative impl must deny only the type it names"
    );
}

#[test]
fn a_negative_impl_does_not_satisfy_a_bound() {
    let message = compile_message(
        "\
trait Describe {
    fn describe(self) -> int;
}
struct Holder<T> { value: T }
impl<T> Describe for Holder<T> {
    default fn describe(self) -> int { 1 }
}
impl !Describe for Holder<bool> {}
fn show<T: Describe>(value: T) -> int {
    value.describe()
}
fn probe() -> int {
    let h = Holder { value: true }
    show(h)
}
probe()
",
    );
    assert!(
        message.contains("error[E0338]"),
        "a denied trait must not satisfy a bound that names it: {message}"
    );
}

#[test]
fn negative_impl_rejects_orphan_e0431() {
    let message = compile_message(
        "\
trait Describe {
    fn describe(self) -> int;
}
impl !Describe for Option<int> {}
fn probe() -> int { 1 }
probe()
",
    );
    assert!(
        message.contains("error[E0431]"),
        "a negative impl on a foreign type must be E0431: {message}"
    );
}

#[test]
fn negative_impl_orphan_matches_the_positive_path() {
    let negative = compile_message(
        "\
trait Describe {
    fn describe(self) -> int;
}
impl !Describe for int {}
fn probe() -> int { 1 }
probe()
",
    );
    let positive = compile_message(
        "\
trait Describe {
    fn describe(self) -> int;
}
impl Describe for int {
    fn describe(self) -> int { 1 }
}
fn probe() -> int { 1 }
probe()
",
    );
    assert!(
        positive.contains("error[E0101]"),
        "the positive path refuses a primitive at the parser: {positive}"
    );
    assert!(
        negative.contains("error[E0101]"),
        "the negative path must refuse it at the same place: {negative}"
    );
}

#[test]
fn negative_impl_on_an_imported_pair_matches_the_positive_path() {
    const LIBRARY: &str = "\
pub struct Point { pub x: int }
pub trait Norm {
    fn norm(self) -> int;
}
";
    assert_eq!(
        module_value(&[
            ("geo.aelys", LIBRARY),
            (
                "main.aelys",
                "\
needs geo
impl Norm for Point {
    fn norm(self) -> int { self.x }
}
fn probe() -> int {
    let p = Point { x: 5 }
    p.norm()
}
probe()
"
            ),
        ]),
        5,
        "an impl of an imported trait on an imported type is accepted today"
    );

    let (_dir, entry) = in_module(&[
        ("geo.aelys", LIBRARY),
        (
            "main.aelys",
            "\
needs geo
impl !Norm for Point {}
fn probe() -> int { 1 }
probe()
",
        ),
    ]);
    let outcome = aelys_driver::run_file(&entry);
    assert!(
        outcome.is_ok(),
        "the negative path must accept what the positive path accepts: {:?}",
        outcome.err().map(|error| error.to_string())
    );
}

#[test]
fn negative_impl_on_a_local_type_is_accepted() {
    let source = "\
trait Describe {
    fn describe(self) -> int;
}
struct Point { x: int }
struct Tag { y: int }
impl Describe for Point {
    fn describe(self) -> int { self.x }
}
impl !Describe for Tag {}
fn probe() -> int {
    let p = Point { x: 4 }
    p.describe()
}
probe()
";
    assert_eq!(value_of(source), 4);
    assert!(
        !trait_symbols(&compiled(source))
            .iter()
            .any(|name| name.contains(&segment_of("Tag"))),
        "a negative impl must leave no trait body behind"
    );
}

#[test]
fn positive_negative_overlap_is_e0432() {
    let message = compile_message(
        "\
trait Describe {
    fn describe(self) -> int;
}
struct Point { x: int }
impl Describe for Point {
    fn describe(self) -> int { self.x }
}
impl !Describe for Point {}
fn probe() -> int { 1 }
probe()
",
    );
    assert!(
        message.contains("error[E0432]"),
        "a positive and a negative impl of one trait for one type must be E0432: {message}"
    );
}

#[test]
fn positive_negative_overlap_is_e0432_in_either_order() {
    let forward = compile_message(
        "\
trait Describe {
    fn describe(self) -> int;
}
struct Point { x: int }
impl Describe for Point {
    fn describe(self) -> int { self.x }
}
impl !Describe for Point {}
fn probe() -> int { 1 }
probe()
",
    );
    let reversed = compile_message(
        "\
trait Describe {
    fn describe(self) -> int;
}
struct Point { x: int }
impl !Describe for Point {}
impl Describe for Point {
    fn describe(self) -> int { self.x }
}
fn probe() -> int { 1 }
probe()
",
    );
    assert!(forward.contains("error[E0432]"), "{forward}");
    assert!(
        reversed.contains("error[E0432]"),
        "source order decided the verdict: {reversed}"
    );
}

#[test]
fn overlapping_impl_is_e0433() {
    let message = compile_message(
        "\
trait Describe {
    fn describe(self) -> int;
}
struct Pair<A, B> { a: A, b: B }
impl<A> !Describe for Pair<A, int> {}
impl<B> !Describe for Pair<int, B> {}
fn probe() -> int { 1 }
probe()
",
    );
    assert!(
        message.contains("error[E0433]"),
        "two negative headers neither of which is more specific must be E0433: {message}"
    );
}

#[test]
fn an_ordered_pair_of_negative_headers_is_accepted() {
    let source = "\
trait Describe {
    fn describe(self) -> int;
}
struct Holder<T> { value: T }
impl<T> !Describe for Holder<T> {}
impl !Describe for Holder<int> {}
fn probe() -> int { 1 }
probe()
";
    let outcome = Runtime::new().compile(source, CompileOptions::default());
    assert!(
        outcome.is_ok(),
        "a negative header strictly more specific than another carves it, as on the positive path: {:?}",
        outcome.err().map(|error| error.to_string())
    );
}

#[test]
fn disjoint_negative_headers_are_accepted() {
    let source = "\
trait Describe {
    fn describe(self) -> int;
}
struct Holder<T> { value: T }
impl !Describe for Holder<int> {}
impl !Describe for Holder<bool> {}
fn probe() -> int { 1 }
probe()
";
    let outcome = Runtime::new().compile(source, CompileOptions::default());
    assert!(
        outcome.is_ok(),
        "two negative headers that cannot both apply are not an overlap: {:?}",
        outcome.err().map(|error| error.to_string())
    );
}

#[test]
fn overlapping_impl_is_e0433_in_either_order() {
    let reversed = compile_message(
        "\
trait Describe {
    fn describe(self) -> int;
}
struct Pair<A, B> { a: A, b: B }
impl<B> !Describe for Pair<int, B> {}
impl<A> !Describe for Pair<A, int> {}
fn probe() -> int { 1 }
probe()
",
    );
    assert!(
        reversed.contains("error[E0433]"),
        "source order decided the verdict: {reversed}"
    );
}

#[test]
fn a_plain_overlap_is_still_e0340() {
    let message = compile_message(
        "\
trait Describe {
    fn describe(self) -> int;
}
struct Holder<T> { value: T }
impl<T> Describe for Holder<T> {
    fn describe(self) -> int { 1 }
}
impl Describe for Holder<int> {
    fn describe(self) -> int { 2 }
}
fn probe() -> int { 1 }
probe()
",
    );
    assert!(
        message.contains("error[E0340]"),
        "without 'default fn' an overlap keeps the verdict and the code it has today: {message}"
    );
}

#[test]
fn a_negative_impl_body_is_refused() {
    for body in [
        "    fn describe(self) -> int { self.x }",
        "    default fn describe(self) -> int { self.x }",
    ] {
        let message = compile_message(&format!(
            "\
trait Describe {{
    fn describe(self) -> int;
}}
struct Point {{ x: int }}
impl !Describe for Point {{
{body}
}}
fn probe() -> int {{ 1 }}
probe()
"
        ));
        assert!(
            message.contains("error[E0432]"),
            "a negative header whose body supplies the method contradicts itself, whatever the item: {message}"
        );
    }
}

#[test]
fn misplaced_default_fn_is_e0441() {
    for source in [
        "\
trait Describe {
    default fn describe(self) -> int { 0 }
}
fn probe() -> int { 1 }
probe()
",
        "\
struct Point { x: int }
impl Point {
    default fn describe(self) -> int { self.x }
}
fn probe() -> int { 1 }
probe()
",
        "\
trait Describe {
    fn describe(self) -> int;
}
struct Point { x: int }
impl Describe for Point {
    default fn describe(self) -> int { self.x }
}
fn probe() -> int { 1 }
probe()
",
    ] {
        let message = compile_message(source);
        assert!(
            message.contains("error[E0441]"),
            "'default fn' outside a positive generic trait impl must be E0441: {message}\n{source}"
        );
    }
}

#[test]
fn unordered_specialization_is_e0442() {
    let message = compile_message(
        "\
trait Describe {
    fn describe(self) -> int;
}
struct Holder<T> { value: T }
impl<T> Describe for Holder<T> {
    default fn describe(self) -> int { 1 }
}
impl<U> Describe for Holder<U> {
    fn describe(self) -> int { 2 }
}
fn probe() -> int { 1 }
probe()
",
    );
    assert!(
        message.contains("error[E0442]"),
        "headers equal up to renaming are not strictly more specific: {message}"
    );
}

/// the same program that calls the method: a coherence verdict must not lose the
#[test]
fn unordered_specialization_is_e0442_even_when_the_trait_is_used() {
    let message = compile_message(
        "\
trait Describe {
    fn describe(self) -> int;
}
struct Holder<T> { value: T }
impl<T> Describe for Holder<T> {
    default fn describe(self) -> int { 1 }
}
impl<U> Describe for Holder<U> {
    fn describe(self) -> int { 2 }
}
fn probe() -> int {
    let h = Holder { value: 1 }
    h.describe()
}
probe()
",
    );
    assert!(
        message.contains("error[E0442]"),
        "the coherence verdict must win over the collision it causes: {message}"
    );
}

#[test]
fn a_denied_conversion_is_not_selected_at_a_call() {
    let message = compile_message(
        "\
struct MyErr { code: int }
struct Wrapper { held: int }
impl From<Wrapper> for MyErr {
    fn from(value: Wrapper) -> MyErr { MyErr { code: value.held } }
}
impl !From<Wrapper> for MyErr {}
fn probe() -> int {
    let w = Wrapper { held: 1 }
    let e: MyErr = MyErr::from(w)
    e.code
}
probe()
",
    );
    assert!(
        message.contains("error[E0432]"),
        "a conversion both provided and denied contradicts before any call: {message}"
    );
}

#[test]
fn unordered_specialization_is_e0442_in_either_order() {
    let reversed = compile_message(
        "\
trait Describe {
    fn describe(self) -> int;
}
struct Holder<T> { value: T }
impl<U> Describe for Holder<U> {
    default fn describe(self) -> int { 2 }
}
impl<T> Describe for Holder<T> {
    fn describe(self) -> int { 1 }
}
fn probe() -> int { 1 }
probe()
",
    );
    assert!(
        reversed.contains("error[E0442]"),
        "source order decided the verdict: {reversed}"
    );
}

const INCOMPARABLE: &str = "\
trait Describe {
    fn describe(self) -> int;
}
struct Pair<A, B> { a: A, b: B }
impl<A, B> Describe for Pair<A, B> {
    default fn describe(self) -> int { 0 }
}
impl<A> Describe for Pair<A, int> {
    fn describe(self) -> int { 1 }
}
impl<B> Describe for Pair<int, B> {
    fn describe(self) -> int { 2 }
}
";

#[test]
fn ambiguous_specialization_is_e0443() {
    let message = compile_message(&format!(
        "{INCOMPARABLE}\
fn probe() -> int {{
    let p = Pair {{ a: 1, b: 2 }}
    p.describe()
}}
probe()
"
    ));
    assert!(
        message.contains("error[E0443]"),
        "a call inside the intersection of two incomparable candidates has no minimum: {message}"
    );
}

#[test]
fn incomparable_candidates_outside_their_intersection_are_accepted() {
    assert_eq!(
        value_of(&format!(
            "{INCOMPARABLE}\
fn probe() -> int {{
    let p = Pair {{ a: true, b: 2 }}
    p.describe()
}}
probe()
"
        )),
        1,
        "two incomparable candidates are useful apart; only their intersection is ambiguous"
    );
}

#[test]
fn ambiguous_specialization_is_e0443_in_either_order() {
    let reversed = compile_message(
        "\
trait Describe {
    fn describe(self) -> int;
}
struct Pair<A, B> { a: A, b: B }
impl<B> Describe for Pair<int, B> {
    fn describe(self) -> int { 2 }
}
impl<A> Describe for Pair<A, int> {
    fn describe(self) -> int { 1 }
}
impl<A, B> Describe for Pair<A, B> {
    default fn describe(self) -> int { 0 }
}
fn probe() -> int {
    let p = Pair { a: 1, b: 2 }
    p.describe()
}
probe()
",
    );
    assert!(
        reversed.contains("error[E0443]"),
        "source order decided the verdict: {reversed}"
    );
}

#[test]
fn a_specialization_chain_selects_its_leaf() {
    let source = "\
trait Describe {
    fn describe(self) -> int;
}
struct Pair<A, B> { a: A, b: B }
impl<A, B> Describe for Pair<A, B> {
    default fn describe(self) -> int { 1 }
}
impl<A> Describe for Pair<A, int> {
    default fn describe(self) -> int { 2 }
}
impl Describe for Pair<int, int> {
    fn describe(self) -> int { 3 }
}
fn probe() -> int {
    let a = Pair { a: true, b: true }
    let b = Pair { a: true, b: 1 }
    let c = Pair { a: 1, b: 1 }
    a.describe() * 100 + b.describe() * 10 + c.describe()
}
probe()
";
    assert_eq!(
        value_of(source),
        123,
        "an impl may be both a replacement and replaceable"
    );
    let calls = calls_of(&compiled(source), "probe");
    assert_eq!(calls.len(), 3, "{calls:?}");
    assert_ne!(calls[0], calls[1], "{calls:?}");
    assert_ne!(calls[1], calls[2], "{calls:?}");
}

#[test]
fn recursive_specialization_keeps_the_existing_guard() {
    let message = compile_message(
        "\
trait Grow {
    fn grow(self) -> int;
}
struct Wrap<T> { value: T }
impl<T> Grow for Wrap<T> {
    default fn grow(self) -> int {
        let bigger = Wrap { value: self }
        bigger.grow()
    }
}
impl Grow for Wrap<int> {
    fn grow(self) -> int { 0 }
}
fn probe() -> int {
    let w = Wrap { value: true }
    w.grow()
}
probe()
",
    );
    assert!(
        message.contains("error[E0344]"),
        "the growing type argument is caught by the guard that already exists, not by a new one: {message}"
    );
}

#[test]
fn specialization_limit_is_e0444() {
    // c(8,4) = 70 pairwise incomparable headers, plus their open base
    let message = compile_message(&exploding_selection(8, 4));
    assert!(
        message.contains("error[E0444]"),
        "seventy-one applicable candidates is past the bound: {message}"
    );
}

/// one witness above the bound would leave every value between the two free; the
#[test]
fn the_selection_bound_is_pinned_from_both_sides() {
    let at = compile_message(&specialization_chain(64));
    assert!(
        at.contains("error[E0444]"),
        "sixty-five applicable candidates is one past the bound: {at}"
    );
    assert_eq!(
        value_of(&specialization_chain(63)),
        63,
        "sixty-four applicable candidates is the bound itself, and the leaf answers"
    );
}

/// a totally ordered chain: each header fixes one more position to int than the
fn specialization_chain(links: usize) -> String {
    let params: Vec<String> = (0..links).map(|index| format!("T{index}")).collect();
    let fields: Vec<String> = (0..links)
        .map(|index| format!("f{index}: T{index},"))
        .collect();
    let mut source = String::from("trait Rank {\n    fn rank(self) -> int;\n}\n");
    source.push_str(&format!(
        "struct Big<{}> {{ {} }}\n",
        params.join(", "),
        fields.join(" ")
    ));
    for fixed in 0..links {
        let mut header: Vec<String> = params.clone();
        for slot in header.iter_mut().take(fixed) {
            *slot = "int".to_string();
        }
        let declared: Vec<String> = params[fixed..].to_vec();
        source.push_str(&format!(
            "impl<{}> Rank for Big<{}> {{\n    default fn rank(self) -> int {{ {fixed} }}\n}}\n",
            declared.join(", "),
            header.join(", ")
        ));
    }
    let all_int: Vec<String> = (0..links).map(|_| "int".to_string()).collect();
    source.push_str(&format!(
        "impl Rank for Big<{}> {{\n    fn rank(self) -> int {{ {links} }}\n}}\n",
        all_int.join(", ")
    ));
    let values: Vec<String> = (0..links).map(|index| format!("f{index}: 1,")).collect();
    source.push_str(&format!(
        "fn probe() -> int {{\n    let big = Big {{ {} }}\n    big.rank()\n}}\nprobe()\n",
        values.join(" ")
    ));
    source
}

/// every k-subset of the positions fixed to int gives one header; two headers of
fn exploding_selection(width: usize, size: usize) -> String {
    let params: Vec<String> = (0..width).map(|index| format!("T{index}")).collect();
    let fields: Vec<String> = (0..width)
        .map(|index| format!("f{index}: T{index},"))
        .collect();
    let mut source = String::from("trait Describe {\n    fn describe(self) -> int;\n}\n");
    source.push_str(&format!(
        "struct Big<{}> {{ {} }}\n",
        params.join(", "),
        fields.join(" ")
    ));
    source.push_str(&format!(
        "impl<{}> Describe for Big<{}> {{\n    default fn describe(self) -> int {{ 0 }}\n}}\n",
        params.join(", "),
        params.join(", ")
    ));
    let mut answer = 0;
    for combination in subsets(width, size) {
        answer += 1;
        let header: Vec<String> = (0..width)
            .map(|index| {
                if combination.contains(&index) {
                    "int".to_string()
                } else {
                    params[index].clone()
                }
            })
            .collect();
        let declared: Vec<String> = (0..width)
            .filter(|index| !combination.contains(index))
            .map(|index| params[index].clone())
            .collect();
        source.push_str(&format!(
            "impl<{}> Describe for Big<{}> {{\n    fn describe(self) -> int {{ {answer} }}\n}}\n",
            declared.join(", "),
            header.join(", ")
        ));
    }
    let values: Vec<String> = (0..width).map(|index| format!("f{index}: 1,")).collect();
    source.push_str(&format!(
        "fn probe() -> int {{\n    let big = Big {{ {} }}\n    big.describe()\n}}\nprobe()\n",
        values.join(" ")
    ));
    source
}

fn subsets(width: usize, size: usize) -> Vec<Vec<usize>> {
    let mut out = Vec::new();
    let mut current = Vec::new();
    fn walk(
        start: usize,
        width: usize,
        size: usize,
        current: &mut Vec<usize>,
        out: &mut Vec<Vec<usize>>,
    ) {
        if current.len() == size {
            out.push(current.clone());
            return;
        }
        for index in start..width {
            current.push(index);
            walk(index + 1, width, size, current, out);
            current.pop();
        }
    }
    walk(0, width, size, &mut current, &mut out);
    out
}

#[test]
fn a_negative_header_differing_only_by_a_bound_is_e0432() {
    let source = "\
trait Show {
    fn show(self) -> int;
}
trait Describe {
    fn describe(self) -> int;
}
struct Holder<T> { value: T }
struct Good { g: int }
impl Show for Good {
    fn show(self) -> int { 7 }
}
impl<T> Describe for Holder<T> {
    fn describe(self) -> int { 1 }
}
impl<T> !Describe for Holder<T> where T: Show {}
fn probe() -> int { 1 }
probe()
";
    let message = compile_message(source);
    assert!(
        message.contains("error[E0432]"),
        "bounds do not order two headers, so these two contradict: {message}"
    );
}

#[test]
fn a_negative_header_carries_its_where_clause() {
    let source = "\
trait Show {
    fn show(self) -> int;
}
trait Describe {
    fn describe(self) -> int;
}
struct Holder<T> { value: T }
struct Good { g: int }
impl Show for Good {
    fn show(self) -> int { 7 }
}
impl<T> !Describe for Holder<T> where T: Show {}
fn probe() -> int { 1 }
probe()
";
    let outcome = Runtime::new().compile(source, CompileOptions::default());
    assert!(
        outcome.is_ok(),
        "a negative header parses a where clause as a positive one does: {:?}",
        outcome.err().map(|error| error.to_string())
    );
}

#[test]
fn an_inert_negative_type_parameter_is_accepted() {
    let source = "\
trait Describe {
    fn describe(self) -> int;
}
struct Point { x: int }
impl<T> !Describe for Point {}
fn probe() -> int { 1 }
probe()
";
    let outcome = Runtime::new().compile(source, CompileOptions::default());
    assert!(
        outcome.is_ok(),
        "a negative impl mints no symbol, so E0430's cause does not exist for it: {:?}",
        outcome.err().map(|error| error.to_string())
    );
}

#[test]
fn generic_negative_capture_is_e0431() {
    let message = compile_message(
        "\
trait Describe {
    fn describe(self) -> int;
}
impl<T> !Describe for T {}
fn probe() -> int { 1 }
probe()
",
    );
    assert!(
        message.contains("error[E0431]"),
        "a negative header whose self type is a bare parameter captures every type: {message}"
    );
}

#[test]
fn generic_negative_capture_matches_the_positive_path() {
    let positive = compile_message(
        "\
trait Describe {
    fn describe(self) -> int;
}
impl<T> Describe for T {
    fn describe(self) -> int { 1 }
}
fn probe() -> int { 1 }
probe()
",
    );
    assert!(
        positive.contains("error[E0339]"),
        "the positive path refuses the same capture: {positive}"
    );
}

#[test]
fn default_is_still_an_ordinary_identifier() {
    assert_eq!(
        value_of(
            "\
fn probe() -> int {
    let default = 5
    default + 1
}
probe()
"
        ),
        6,
        "'default' is contextual before 'fn' and must stay a name everywhere else"
    );
}

#[test]
fn a_specialization_keeps_one_definition_of_an_associated_item() {
    let source = "\
trait Carry {
    type Item;
    fn carry(self) -> int;
}
struct Holder<T> { value: T }
impl<T> Carry for Holder<T> {
    type Item = int
    default fn carry(self) -> int { 1 }
}
impl Carry for Holder<int> {
    fn carry(self) -> int { 2 }
}
fn probe() -> int {
    let a = Holder { value: true }
    let b = Holder { value: 7 }
    a.carry() * 10 + b.carry()
}
probe()
";
    assert_eq!(
        value_of(source),
        12,
        "a replacement inherits the base's associated items instead of redeclaring them"
    );
}

#[test]
fn a_nominal_carrying_two_impls_gets_two_base_symbols() {
    let source = "\
trait Describe {
    fn describe(self) -> int;
}
struct Holder<T> { value: T }
impl Describe for Holder<int> {
    fn describe(self) -> int { 1 }
}
impl Describe for Holder<bool> {
    fn describe(self) -> int { 2 }
}
fn probe() -> int {
    let a = Holder { value: 7 }
    let b = Holder { value: true }
    a.describe() * 10 + b.describe()
}
probe()
";
    assert_eq!(
        value_of(source),
        12,
        "two disjoint concrete impls of one trait on one generic nominal are licit"
    );
    let bodies = bodies_of(&compiled(source), "describe");
    assert_eq!(
        bodies.len(),
        2,
        "two impls must mangle to two symbols, or one overwrites the other: {bodies:?}"
    );
    assert_ne!(bodies[0], bodies[1], "{bodies:?}");
}

/// the two lifted frontiers must not answer any program once their forms are
#[test]
fn no_program_still_receives_a_lifted_frontier_code() {
    for source in [
        "\
trait Describe {
    fn describe(self) -> int;
}
struct Point { x: int }
impl !Describe for Point {}
fn probe() -> int { 1 }
probe()
",
        "\
trait Describe {
    default fn describe(self) -> int { 0 }
}
fn probe() -> int { 1 }
probe()
",
    ] {
        let outcome = Runtime::new().compile(source, CompileOptions::default());
        let message = outcome
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(
            !message.contains("error[E0114]") && !message.contains("error[E0115]"),
            "a lifted frontier still answers: {message}"
        );
    }
}

#[test]
fn denying_an_impl_the_prelude_provides_is_e0432() {
    let message = compile_message(
        "\
struct MyErr { code: int }
impl !From<MyErr> for MyErr {}
fn probe() -> int { 1 }
probe()
",
    );
    assert!(
        message.contains("error[E0432]"),
        "the compiler provides the identity conversion, so denying it contradicts: {message}"
    );
}

#[test]
fn denying_a_conversion_the_prelude_does_not_provide_is_accepted() {
    let source = "\
struct MyErr { code: int }
impl !From<int> for MyErr {}
fn probe() -> int { 1 }
probe()
";
    let outcome = Runtime::new().compile(source, CompileOptions::default());
    assert!(
        outcome.is_ok(),
        "only the identity conversion is reserved, so this one denies nothing that exists: {:?}",
        outcome.err().map(|error| error.to_string())
    );
}

#[test]
fn a_reserved_conversion_header_matches_the_positive_path() {
    let positive = compile_message(
        "\
struct MyErr { code: int }
impl From<MyErr> for MyErr {
    fn from(value: MyErr) -> MyErr { value }
}
fn probe() -> int { 1 }
probe()
",
    );
    assert!(
        positive.contains("error[E0375]"),
        "the positive path refuses the reserved header with its own code: {positive}"
    );
}

#[test]
fn a_denied_trait_is_refused_by_a_qualified_call() {
    // the qualified path is written against a concrete impl: on a generic one it
    let message = compile_message(
        "\
trait Describe {
    fn describe(self) -> int;
}
struct Tag { y: int }
impl Describe for Tag {
    fn describe(self) -> int { self.y }
}
impl !Describe for Tag {}
fn probe() -> int {
    let t = Tag { y: 4 }
    Describe::describe(t)
}
probe()
",
    );
    assert!(
        message.contains("error[E0432]"),
        "a trait both implemented and denied for one type contradicts: {message}"
    );
}

#[test]
fn a_denied_conversion_is_not_selected() {
    let message = compile_message(
        "\
struct MyErr { code: int }
impl From<int> for MyErr {
    fn from(value: int) -> MyErr { MyErr { code: value } }
}
impl !From<int> for MyErr {}
fn probe() -> int { 1 }
probe()
",
    );
    assert!(
        message.contains("error[E0432]"),
        "a conversion cannot be both provided and denied: {message}"
    );
}

/// a projection names its nominal, not an application of it, so a denial that
#[test]
fn a_projection_cannot_be_reached_through_a_denied_impl() {
    let message = compile_message(
        "\
trait Carry {
    type Item;
    fn carry(self) -> int;
}
struct Tag { y: int }
impl Carry for Tag {
    type Item = int
    fn carry(self) -> int { self.y }
}
impl !Carry for Tag {}
fn probe(x: Tag::Item) -> int {
    x
}
probe(1)
",
    );
    assert!(
        message.contains("error[E0432]"),
        "the only program that could deny a projection's own impl is a contradiction: {message}"
    );
}

#[test]
fn a_denied_supertrait_is_not_satisfied_through_the_hole() {
    let message = compile_message(
        "\
trait Base {
    fn base(self) -> int;
}
trait Derived: Base {
    fn derived(self) -> int;
}
struct Holder<T> { value: T }
impl<T> Base for Holder<T> {
    default fn base(self) -> int { 1 }
}
impl !Base for Holder<int> {}
impl Derived for Holder<int> {
    fn derived(self) -> int { 2 }
}
fn probe() -> int { 1 }
probe()
",
    );
    assert!(
        message.contains("error[E0338]"),
        "the hole must be seen at the type, not at the bare nominal: {message}"
    );
}

#[test]
fn a_plain_duplicate_keeps_its_code_beside_a_specialization() {
    let message = compile_message(
        "\
trait Describe {
    fn describe(self) -> int;
}
trait Other {
    fn other(self) -> int;
}
struct Holder<T> { value: T }
struct Tag { y: int }
impl<T> Describe for Holder<T> {
    default fn describe(self) -> int { 1 }
}
impl Describe for Holder<int> {
    fn describe(self) -> int { 2 }
}
impl Other for Tag {
    fn other(self) -> int { 1 }
}
impl Other for Tag {
    fn other(self) -> int { 2 }
}
fn probe() -> int { 1 }
probe()
",
    );
    assert!(
        message.contains("error[E0334]"),
        "a plain duplicate keeps E0334 whatever a specialization elsewhere does: {message}"
    );
}

/// two impls of one trait at different instantiations declare different method
#[test]
fn specialization_never_crosses_two_instantiations_of_one_trait() {
    for item in ["0", "1", "5", "99"] {
        let message = compile_message(&format!(
            "\
trait Feed<R> {{
    fn feed(self, item: R) -> int;
}}
struct Holder<T> {{ value: T }}
impl<T> Feed<int> for Holder<T> {{
    default fn feed(self, item: int) -> int {{ item + 1 }}
}}
impl Feed<bool> for Holder<int> {{
    fn feed(self, item: bool) -> int {{ if item {{ 100 }} else {{ 200 }} }}
}}
fn probe() -> int {{
    let h = Holder {{ value: 3 }}
    h.feed({item})
}}
probe()
"
        ));
        assert!(
            message.contains("error[E0437]"),
            "a trait implemented at two instantiations cannot be specialized across them: {message}"
        );
    }
}

#[test]
fn two_instantiations_of_one_trait_read_the_same_in_either_order() {
    let convert = |int_first: bool| {
        let on_int = "impl Convert<int> for Holder<int> {\n    fn convert(self) -> int { 7 }\n}\n";
        let on_bool =
            "impl Convert<bool> for Holder<bool> {\n    fn convert(self) -> bool { true }\n}\n";
        let (first, second) = if int_first {
            (on_int, on_bool)
        } else {
            (on_bool, on_int)
        };
        format!(
            "\
trait Convert<R> {{
    fn convert(self) -> R;
}}
struct Holder<T> {{ value: T }}
{first}{second}fn probe() -> int {{
    let a = Holder {{ value: 5 }}
    a.convert()
}}
probe()
"
        )
    };
    assert_eq!(
        compile_message(&convert(true)),
        compile_message(&convert(false)),
        "declaration order decided the verdict"
    );
}

#[test]
fn a_generic_trait_specializes_within_one_instantiation() {
    let source = "\
trait Feed<R> {
    fn feed(self, item: R) -> int;
}
struct Holder<T> { value: T }
impl<T> Feed<int> for Holder<T> {
    default fn feed(self, item: int) -> int { item + 1 }
}
impl Feed<int> for Holder<int> {
    fn feed(self, item: int) -> int { item + 2 }
}
fn probe() -> int {
    let loose = Holder { value: true }
    let pinned = Holder { value: 9 }
    loose.feed(10) * 100 + pinned.feed(10)
}
probe()
";
    assert_eq!(
        value_of(source),
        1112,
        "within one instantiation the chain still selects: 11 from the base, 12 from the leaf"
    );
}

#[test]
fn a_trait_with_two_parameters_is_not_specialized_across_them() {
    let message = compile_message(
        "\
trait Pairwise<A, B> {
    fn go(self, x: A, y: B) -> int;
}
struct H<T> { v: T }
impl<T> Pairwise<int, int> for H<T> {
    default fn go(self, x: int, y: int) -> int { x + y }
}
impl Pairwise<int, bool> for H<int> {
    fn go(self, x: int, y: bool) -> int { if y { 50 } else { 60 } }
}
fn probe() -> int {
    let h = H { v: 3 }
    h.go(1, 2)
}
probe()
",
    );
    assert!(
        message.contains("error[E0437]"),
        "a second trait parameter differing is still a different instantiation: {message}"
    );
}

#[test]
fn a_nested_self_type_is_not_specialized_across_instantiations() {
    let message = compile_message(
        "\
trait Feed<R> {
    fn feed(self, item: R) -> int;
}
struct W<T> { v: T }
struct H<T> { v: T }
impl<T> Feed<int> for H<W<T>> {
    default fn feed(self, item: int) -> int { item + 1 }
}
impl Feed<bool> for H<W<int>> {
    fn feed(self, item: bool) -> int { if item { 70 } else { 80 } }
}
fn probe() -> int {
    let h = H { v: W { v: 3 } }
    h.feed(0)
}
probe()
",
    );
    assert!(
        message.contains("error[E0437]"),
        "nesting the self type does not make two instantiations one: {message}"
    );
}

#[test]
fn a_generic_trait_specializes_in_return_position() {
    assert_eq!(
        value_of(
            "\
trait Make<R> {
    fn make(self) -> R;
}
struct H<T> { v: T }
impl<T> Make<int> for H<T> {
    default fn make(self) -> int { 1 }
}
impl Make<int> for H<int> {
    fn make(self) -> int { 2 }
}
fn probe() -> int {
    let a = H { v: true }
    let b = H { v: 9 }
    a.make() * 10 + b.make()
}
probe()
"
        ),
        12,
        "within one instantiation the return position specializes as the argument one does"
    );
}

/// the coherence verdicts share priority zero with the duplicate-nominal
#[test]
fn a_duplicate_nominal_still_outranks_a_coherence_verdict() {
    let message = compile_message(
        "\
trait Describe {
    fn describe(self) -> int;
}
struct Point { x: int }
struct Point { y: int }
impl Describe for Point {
    fn describe(self) -> int { 1 }
}
impl !Describe for Point {}
fn probe() -> int { 1 }
probe()
",
    );
    assert!(
        message.contains("error[E0326]"),
        "the duplicate declaration is the cause, the coherence verdict its consequence: {message}"
    );
}

/// a hole carved in a generic impl is the subtle case: the impl is emitted, but
#[test]
fn a_hole_emits_no_instance_for_the_type_it_denies() {
    let source = "\
trait Sealable {
    fn seal(self) -> int;
}
struct Open<T> { held: T }
impl<T> Sealable for Open<T> {
    default fn seal(self) -> int { 1 }
}
impl !Sealable for Open<bool> {}
fn probe() -> int {
    let o: Open<int> = Open { held: 5 }
    o.seal()
}
probe()
";
    assert_eq!(value_of(source), 1);
    let bodies = bodies_of(&compiled(source), "seal");
    assert_eq!(bodies.len(), 1, "{bodies:?}");
    assert!(
        bodies[0].contains(&hex_of("3:i64")),
        "the instance that is allowed must be there: {bodies:?}"
    );
    assert!(
        !bodies[0].contains(&hex_of("4:bool")),
        "the instance the denial names must not be: {bodies:?}"
    );

    // the program above never asks for the denied instance, so its absence
    let both_calls = "fn probe() -> int {
    let o: Open<int> = Open { held: 5 }
    let s: Open<bool> = Open { held: true }
    o.seal() * 10 + s.seal()
}
probe()
";
    let without_denial = format!(
        "trait Sealable {{
    fn seal(self) -> int;
}}
struct Open<T> {{ held: T }}
impl<T> Sealable for Open<T> {{
    default fn seal(self) -> int {{ 1 }}
}}
{both_calls}"
    );
    assert_eq!(value_of(&without_denial), 11);
    let open = bodies_of(&compiled(&without_denial), "seal");
    assert_eq!(
        open.len(),
        2,
        "without the denial both instances exist: {open:?}"
    );
    assert!(
        open.iter().any(|name| name.contains(&hex_of("4:bool"))),
        "the instance the denial would remove must exist without it: {open:?}"
    );

    let with_denial = format!(
        "trait Sealable {{
    fn seal(self) -> int;
}}
struct Open<T> {{ held: T }}
impl<T> Sealable for Open<T> {{
    default fn seal(self) -> int {{ 1 }}
}}
impl !Sealable for Open<bool> {{}}
{both_calls}"
    );
    let message = compile_message(&with_denial);
    assert!(
        message.contains("error[E0338]"),
        "the call the denial names must be refused before any instance is minted: {message}"
    );
}

#[test]
fn a_specialization_of_an_imported_trait_selects_across_the_module_boundary() {
    assert_eq!(
        module_value(&[
            (
                "lib.aelys",
                "\
pub trait Rank {
    fn rank(self) -> int;
}
pub struct Plain { pub n: int }
"
            ),
            (
                "main.aelys",
                "\
needs lib
struct Local<T> { held: T }
impl<T> Rank for Local<T> {
    default fn rank(self) -> int { 1 }
}
impl Rank for Local<int> {
    fn rank(self) -> int { 2 }
}
impl !Rank for Plain {}
fn probe() -> int {
    let a: Local<bool> = Local { held: true }
    let b: Local<int> = Local { held: 7 }
    a.rank() * 10 + b.rank()
}
probe()
"
            ),
        ]),
        12,
        "an imported trait specializes locally, and a denial on an imported type stands beside it"
    );
}

/// different answers, and one of them is a refusal
#[test]
fn a_hole_and_a_specialization_share_one_root() {
    let source = "\
trait Rank {
    fn rank(self) -> int;
}
struct C<T> { h: T }
impl<T> Rank for C<T> {
    default fn rank(self) -> int { 1 }
}
impl Rank for C<int> {
    fn rank(self) -> int { 2 }
}
impl !Rank for C<bool> {}
";
    assert_eq!(
        value_of(&format!(
            "{source}\
fn probe() -> int {{
    let a: C<string> = C {{ h: \"x\" }}
    let b: C<int> = C {{ h: 7 }}
    a.rank() * 10 + b.rank()
}}
probe()
"
        )),
        12,
        "the root answers the type it still covers, the replacement the type it took"
    );
    let message = compile_message(&format!(
        "{source}\
fn probe() -> int {{
    let a: C<bool> = C {{ h: true }}
    a.rank()
}}
probe()
"
    ));
    assert!(
        message.contains("error[E0338]"),
        "the type the hole names is out of the root's reach: {message}"
    );
}

/// where two incomparable impls meet, a bound must say the same thing a direct
#[test]
fn an_ambiguous_specialization_through_a_bound_is_e0443() {
    let source = "\
trait D {
    fn d(self) -> int;
}
struct P<A, B> { a: A, b: B }
impl<A, B> D for P<A, B> {
    default fn d(self) -> int { 0 }
}
impl<A> D for P<A, int> {
    fn d(self) -> int { 1 }
}
impl<B> D for P<int, B> {
    fn d(self) -> int { 2 }
}
fn via<T: D>(v: T) -> int {
    v.d()
}
";
    let message = compile_message(&format!(
        "{source}\
fn probe() -> int {{
    let p: P<int, int> = P {{ a: 1, b: 2 }}
    via(p)
}}
probe()
"
    ));
    assert!(
        message.contains("error[E0443]"),
        "the bound met the intersection of two incomparable impls: {message}"
    );
    assert_eq!(
        value_of(&format!(
            "{source}\
fn probe() -> int {{
    let a: P<bool, int> = P {{ a: true, b: 2 }}
    let b: P<int, bool> = P {{ a: 1, b: true }}
    via(a) * 10 + via(b)
}}
probe()
"
        )),
        12,
        "outside their intersection the two impls are useful apart"
    );
}

/// a bound names the trait, not the impl; the receiver that reaches the call is
#[test]
fn a_specialization_is_selected_through_a_generic_bound() {
    let source = "\
trait D {
    fn d(self) -> int;
}
struct H<T> { v: T }
impl<T> D for H<T> {
    default fn d(self) -> int { 1 }
}
impl D for H<int> {
    fn d(self) -> int { 2 }
}
fn via<T: D>(v: T) -> int {
    v.d()
}
";
    assert_eq!(
        value_of(&format!(
            "{source}\
fn probe() -> int {{
    let h: H<int> = H {{ v: 3 }}
    via(h)
}}
probe()
"
        )),
        2,
        "the bound reached the root where the receiver names the replacement"
    );
    assert_eq!(
        value_of(&format!(
            "{source}\
fn probe() -> int {{
    let h: H<bool> = H {{ v: true }}
    via(h)
}}
probe()
"
        )),
        1,
        "the type the replacement does not cover still answers from the root"
    );
    assert_eq!(
        value_of(&format!(
            "{source}\
fn probe() -> int {{
    let a: H<int> = H {{ v: 3 }}
    let b: H<bool> = H {{ v: true }}
    via(a) * 10 + via(b)
}}
probe()
"
        )),
        21,
        "one bound, two receivers, two impls"
    );

    let both = format!(
        "{source}\
fn probe() -> int {{
    let a: H<int> = H {{ v: 3 }}
    let b: H<bool> = H {{ v: true }}
    via(a) * 10 + via(b)
}}
probe()
"
    );
    let bodies = bodies_of(&compiled(&both), "d");
    assert_eq!(bodies.len(), 2, "one body per impl reached: {bodies:?}");
    assert!(
        bodies
            .iter()
            .any(|name| !name.contains(INSTANCE_INFIX) && name.contains(&segment_of("H<int>"))),
        "the replacement must have its own body: {bodies:?}"
    );
    assert!(
        !bodies
            .iter()
            .any(|name| name.contains(INSTANCE_INFIX) && name.contains(&hex_of("3:i64"))),
        "the root was instantiated at int, so the bound did not take the replacement: {bodies:?}"
    );
}

/// the only shape that carries a hole on a conversion is a generic impl the
#[test]
fn a_hole_in_a_generic_conversion_refuses_the_question_mark() {
    let source = "\
struct Pair<T> { tag: T, code: int }
struct Boxed<T> { tag: T, code: int }
impl<T> From<Pair<T>> for Boxed<T> {
    default fn from(value: Pair<T>) -> Boxed<T> { Boxed { tag: value.tag, code: value.code } }
}
";
    let body = "\
fn inner() -> Result<int, Pair<bool>> {
    return Result::Err(Pair { tag: true, code: 3 })
}
fn probe() -> Result<int, Boxed<bool>> {
    let v = inner()?
    return Result::Ok(v)
}
match probe() {
    Result::Ok(v) => v,
    Result::Err(e) => e.code,
}
";
    assert_eq!(
        value_of(&format!("{source}{body}")),
        3,
        "without the denial the generic conversion is selected and instantiated"
    );
    let message = compile_message(&format!(
        "{source}impl !From<Pair<bool>> for Boxed<bool> {{}}\n{body}"
    ));
    assert!(
        message.contains("error[E0338]"),
        "the denied conversion may not be selected by '?': {message}"
    );
}

/// parameter, so the denial it would meet is not the one the instance meets; the
#[test]
fn a_denied_conversion_is_refused_in_every_generic_shape() {
    let hole = "\
struct Pair<T> { tag: T, code: int }
struct Boxed<T> { tag: T, code: int }
impl<T> From<Pair<T>> for Boxed<T> {
    default fn from(value: Pair<T>) -> Boxed<T> { Boxed { tag: value.tag, code: value.code } }
}
impl !From<Pair<bool>> for Boxed<bool> {}
";
    let open = "\
struct Pair<T> { tag: T, code: int }
struct Boxed<T> { tag: T, code: int }
impl<T> From<Pair<T>> for Boxed<T> {
    default fn from(value: Pair<T>) -> Boxed<T> { Boxed { tag: value.tag, code: value.code } }
}
";
    let in_a_generic_function = "\
fn relay<T>(r: Result<int, Pair<T>>) -> Result<int, Boxed<T>> {
    let v = r?
    return Result::Ok(v)
}
fn probe() -> int {
    let a: Result<int, Pair<bool>> = Result::Err(Pair { tag: true, code: 3 })
    match relay(a) {
        Result::Ok(v) => v,
        Result::Err(e) => e.code,
    }
}
probe()
";
    let in_an_impl_method = "\
trait Relay<T> {
    fn relay(self, r: Result<int, Pair<T>>) -> Result<int, Boxed<T>>;
}
struct R<T> { v: T }
impl<T> Relay<T> for R<T> {
    fn relay(self, r: Result<int, Pair<T>>) -> Result<int, Boxed<T>> {
        let v = r?
        return Result::Ok(v)
    }
}
fn probe() -> int {
    let rr: R<bool> = R { v: true }
    let a: Result<int, Pair<bool>> = Result::Err(Pair { tag: true, code: 3 })
    match rr.relay(a) {
        Result::Ok(v) => v,
        Result::Err(e) => e.code,
    }
}
probe()
";
    for body in [in_a_generic_function, in_an_impl_method] {
        let message = compile_message(&format!("{hole}{body}"));
        assert!(
            message.contains("error[E0338]"),
            "the instance meets the denial the template could not see: {message}"
        );
        assert_eq!(
            value_of(&format!("{open}{body}")),
            3,
            "without the denial the same shape converts"
        );
    }

    // one template, two instances: the denied one must be refused and, without
    let two = "\
fn relay<T>(r: Result<int, Pair<T>>) -> Result<int, Boxed<T>> {
    let v = r?
    return Result::Ok(v)
}
fn probe() -> int {
    let a: Result<int, Pair<bool>> = Result::Err(Pair { tag: true, code: 3 })
    let b: Result<int, Pair<int>> = Result::Err(Pair { tag: 1, code: 5 })
    let x = match relay(a) { Result::Ok(v) => v, Result::Err(e) => e.code }
    let y = match relay(b) { Result::Ok(v) => v, Result::Err(e) => e.code }
    return x * 100 + y
}
probe()
";
    let message = compile_message(&format!("{hole}{two}"));
    assert!(
        message.contains("error[E0338]"),
        "one instance of two is denied, and that is enough to refuse: {message}"
    );
    assert_eq!(value_of(&format!("{open}{two}")), 305);
}

/// a conversion is specialized like any other impl, and the instance decides:
#[test]
fn a_conversion_takes_the_specialization_its_instance_names() {
    let source = "\
struct A<T> { code: int }
struct W<T> { code: int }
impl<T> From<A<T>> for W<T> {
    default fn from(v: A<T>) -> W<T> { W { code: v.code + 1 } }
}
impl From<A<bool>> for W<bool> {
    fn from(v: A<bool>) -> W<bool> { W { code: v.code + 100 } }
}
";
    assert_eq!(
        value_of(&format!(
            "{source}\
fn relay(r: Result<int, A<bool>>) -> Result<int, W<bool>> {{
    let v = r?
    return Result::Ok(v)
}}
fn probe() -> int {{
    let x: Result<int, A<bool>> = Result::Err(A {{ code: 3 }})
    match relay(x) {{ Result::Ok(v) => v, Result::Err(e) => e.code }}
}}
probe()
"
        )),
        103,
        "the replacement covers this source, so the root must not answer"
    );
    assert_eq!(
        value_of(&format!(
            "{source}\
fn relay<T>(r: Result<int, A<T>>) -> Result<int, W<T>> {{
    let v = r?
    return Result::Ok(v)
}}
fn probe() -> int {{
    let x: Result<int, A<bool>> = Result::Err(A {{ code: 3 }})
    let y: Result<int, A<int>> = Result::Err(A {{ code: 5 }})
    let p = match relay(x) {{ Result::Ok(v) => v, Result::Err(e) => e.code }}
    let q = match relay(y) {{ Result::Ok(v) => v, Result::Err(e) => e.code }}
    return p * 100 + q
}}
probe()
"
        )),
        10306,
        "one template, two instances, two different impls"
    );
}

/// `default fn` on one of two impls at different instantiations does not make
#[test]
fn two_instantiations_are_not_made_a_specialization_by_default_fn() {
    let with_default = "\
trait Describe<R> {
    fn describe(self, r: R) -> int;
}
struct Holder<T> { value: T }
impl<T> Describe<int> for Holder<T> {
    default fn describe(self, r: int) -> int { 1 }
}
impl<T> Describe<bool> for Holder<T> {
    fn describe(self, r: bool) -> int { 2 }
}
fn probe() -> int {
    let h: Holder<int> = Holder { value: 1 }
    h.describe(1)
}
probe()
";
    let without_default = with_default.replace(
        "    default fn describe(self, r: int)",
        "    fn describe(self, r: int)",
    );
    let with = compile_message(with_default);
    let without = compile_message(&without_default);
    assert!(
        with.contains("error[E0437]"),
        "an unrelated 'default fn' may not change which code answers: {with}"
    );
    assert_eq!(
        with.lines().next(),
        without.lines().next(),
        "the sentence must not depend on a 'default fn' that decides nothing"
    );
}

/// the order between two conversion headers reads them as one pair too: binding
#[test]
fn the_order_between_two_conversions_binds_both_halves_at_once() {
    let source = "\
struct A<T> { code: int }
struct W<T, U> { code: int }
impl<T, U> From<A<U>> for W<T, U> {
    default fn from(v: A<U>) -> W<T, U> { W { code: v.code + 1 } }
}
impl<V> From<A<V>> for W<V, bool> {
    fn from(v: A<V>) -> W<V, bool> { W { code: v.code + 100 } }
}
";
    let message = compile_message(&format!(
        "{source}\
fn relay(r: Result<int, A<bool>>) -> Result<int, W<bool, bool>> {{
    let v = r?
    return Result::Ok(v)
}}
fn probe() -> int {{
    let x: Result<int, A<bool>> = Result::Err(A {{ code: 3 }})
    match relay(x) {{ Result::Ok(v) => v, Result::Err(e) => e.code }}
}}
probe()
"
    ));
    assert!(
        message.contains("error[E0374]"),
        "neither header outranks the other under one substitution: {message}"
    );
    let twin = compile_message(
        "\
trait Take<S> {
    fn take(self, s: S) -> int;
}
struct A<T> { code: int }
struct W<T, U> { code: int }
impl<T, U> Take<A<U>> for W<T, U> {
    default fn take(self, s: A<U>) -> int { return s.code + 1 }
}
impl<V> Take<A<V>> for W<V, bool> {
    fn take(self, s: A<V>) -> int { return s.code + 100 }
}
fn probe() -> int {
    let w: W<bool, bool> = W { code: 0 }
    let a: A<bool> = A { code: 3 }
    return w.take(a)
}
probe()
",
    );
    assert!(
        twin.contains("error[E0437]"),
        "the ordinary trait refuses the same pair: {twin}"
    );
}

/// two conversion headers can be useful apart and undecidable where they meet,
#[test]
fn two_incomparable_conversions_are_refused_only_where_they_meet() {
    let source = "\
struct P<A, B> { code: int }
struct Q<A, B> { code: int }
impl<A, B> From<P<A, B>> for Q<A, B> {
    default fn from(v: P<A, B>) -> Q<A, B> { Q { code: v.code } }
}
impl<A> From<P<A, int>> for Q<A, int> {
    fn from(v: P<A, int>) -> Q<A, int> { Q { code: v.code + 1 } }
}
impl<B> From<P<int, B>> for Q<int, B> {
    fn from(v: P<int, B>) -> Q<int, B> { Q { code: v.code + 2 } }
}
";
    let message = compile_message(&format!(
        "{source}\
fn relay(r: Result<int, P<int, int>>) -> Result<int, Q<int, int>> {{
    let v = r?
    return Result::Ok(v)
}}
fn probe() -> int {{
    let a: Result<int, P<int, int>> = Result::Err(P {{ code: 3 }})
    match relay(a) {{ Result::Ok(v) => v, Result::Err(e) => e.code }}
}}
probe()
"
    ));
    assert!(
        message.contains("error[E0374]"),
        "neither header outranks the other on this type: {message}"
    );
    assert_eq!(
        value_of(&format!(
            "{source}\
fn relay(r: Result<int, P<bool, int>>) -> Result<int, Q<bool, int>> {{
    let v = r?
    return Result::Ok(v)
}}
fn probe() -> int {{
    let a: Result<int, P<bool, int>> = Result::Err(P {{ code: 3 }})
    match relay(a) {{ Result::Ok(v) => v, Result::Err(e) => e.code }}
}}
probe()
"
        )),
        4,
        "outside their intersection one of the two answers alone"
    );
}

/// the two halves of a conversion header name one substitution, not two: a header
#[test]
fn a_conversion_header_binds_its_target_and_source_together() {
    let message = compile_message(
        "\
struct Pair<T> { code: int }
struct Boxed<T> { code: int }
impl<T> From<Pair<T>> for Boxed<T> {
    fn from(value: Pair<T>) -> Boxed<T> { Boxed { code: value.code } }
}
fn relay(r: Result<int, Pair<bool>>) -> Result<int, Boxed<int>> {
    let v = r?
    return Result::Ok(v)
}
fn probe() -> int {
    let a: Result<int, Pair<bool>> = Result::Err(Pair { code: 3 })
    match relay(a) {
        Result::Ok(v) => v,
        Result::Err(e) => e.code,
    }
}
probe()
",
    );
    assert!(
        message.contains("error[E0374]"),
        "'Pair<bool>' and 'Boxed<int>' cannot both be 'T': {message}"
    );
}

/// a bound declared and not used leaves no marker and no residual the substitution
#[test]
fn a_declared_but_unused_bound_still_says_the_denial() {
    let source = "\
trait Describe {
    fn describe(self) -> int;
}
struct Holder<T> { value: T }
impl !Describe for Holder<bool> {}
fn call<T: Describe>(x: T) -> int {
    return 9
}
fn outer<U>(y: U) -> int {
    return call(y)
}
fn probe() -> int {
    let a: Holder<bool> = Holder { value: true }
    return outer(a)
}
probe()
";
    let message = compile_message(source);
    assert!(
        message.contains("denied"),
        "the instance bound check keeps the provenance too: {message}"
    );
}

/// a replacement of an open root supplies the associated item too, so a projection
#[test]
fn a_projection_on_a_specialized_nominal_resolves_to_the_replacement() {
    let source = "\
struct Wrap<T> { v: T }
struct Other { k: int }
trait Carrier {
    type Item;
    fn make(self) -> Self::Item;
}
struct Maker<T> { n: int }
impl<T> Carrier for Maker<T> {
    type Item = Wrap<bool>;
    default fn make(self) -> Wrap<bool> { return Wrap { v: true } }
}
impl Carrier for Maker<int> {
    type Item = Other;
    fn make(self) -> Other { return Other { k: 7 } }
}
impl<T> Display for Wrap<T> {
    fn to_display(self) -> string { return \"WRAPPED\" }
}
";
    let show = "\
fn show<T: Carrier>(t: T) -> string {
    let item = t.make()
    return \"{item}\"
}
";
    let message = compile_message(&format!(
        "{source}impl !Display for Other {{}}\n{show}\
fn probe() -> int {{
    let m: Maker<int> = Maker {{ n: 1 }}
    return show(m).len()
}}
probe()
"
    ));
    assert!(
        message.contains("error[E0338]") && message.contains("denied"),
        "the projection names the type the replacement supplies: {message}"
    );
    assert_eq!(
        value_of(&format!(
            "{source}{show}\
fn probe() -> int {{
    let m: Maker<bool> = Maker {{ n: 1 }}
    return show(m).len()
}}
probe()
"
        )),
        7,
        "off the replacement the root's item answers, and 'WRAPPED' is seven long"
    );
}

/// the object still carries the projection it was written with; once the marker is
#[test]
fn a_projection_receiver_carries_the_type_it_resolved_to() {
    assert_eq!(
        value_of(
            "\
struct Wrap<T> { v: T }
trait Carrier {
    type Item;
    fn make(self) -> Self::Item;
}
struct Maker { n: int }
impl Carrier for Maker {
    type Item = Wrap<bool>;
    fn make(self) -> Wrap<bool> { return Wrap { v: true } }
}
impl<T> Display for Wrap<T> {
    default fn to_display(self) -> string { return \"GENERIC\" }
}
fn show<T: Carrier>(t: T) -> string {
    let item = t.make()
    return \"{item}\"
}
fn probe() -> int {
    let m: Maker = Maker { n: 1 }
    return show(m).len()
}
probe()
"
        ),
        7,
        "a generic impl reached through a projection must be instantiated, not refused"
    );
}

/// a receiver that is still a projection is not yet concrete, and a marker on it
#[test]
fn a_projection_receiver_resolves_before_the_marker_is_judged() {
    let carrier = "\
struct Wrap<T> { v: T }
trait Carrier {
    type Item;
    fn make(self) -> Self::Item;
}
struct Maker { n: int }
impl Carrier for Maker {
    type Item = Wrap<bool>;
    fn make(self) -> Wrap<bool> { return Wrap { v: true } }
}
";
    let show = "\
fn show<T: Carrier>(t: T) -> string {
    let item = t.make()
    return \"{item}\"
}
fn probe() -> int {
    let m: Maker = Maker { n: 1 }
    return show(m).len()
}
probe()
";
    // the denial covers the type the projection resolves to
    let message = compile_message(&format!(
        "{carrier}\
impl<T> Display for Wrap<T> {{
    default fn to_display(self) -> string {{ return \"w\" }}
}}
impl !Display for Wrap<bool> {{}}
{show}"
    ));
    assert!(
        message.contains("error[E0338]") && message.contains("denied"),
        "a projection is a spelling of a type, not a place the denial stops: {message}"
    );

    assert_eq!(
        value_of(&format!(
            "{carrier}\
impl<T> Display for Wrap<T> {{
    default fn to_display(self) -> string {{ return \"generic\" }}
}}
impl Display for Wrap<bool> {{
    fn to_display(self) -> string {{ return \"special\" }}
}}
{show}"
        )),
        7,
        "'special' is seven characters, 'generic' is seven too, so read the string itself"
    );
    assert_eq!(
        value_of(&format!(
            "{carrier}\
impl<T> Display for Wrap<T> {{
    default fn to_display(self) -> string {{ return \"gg\" }}
}}
impl Display for Wrap<bool> {{
    fn to_display(self) -> string {{ return \"sssss\" }}
}}
{show}"
        )),
        5,
        "the replacement covers the type the projection names"
    );
}

/// the display marker is the one path to a trait method that leaves no bound
#[test]
fn a_denied_display_is_refused_where_the_marker_resolves() {
    let source = "\
struct Wrap<T> { v: T }
impl<T> Display for Wrap<T> {
    default fn to_display(self) -> string { return \"w\" }
}
impl !Display for Wrap<bool> {}
";
    // the marker resolved before monomorphization
    let message = compile_message(&format!(
        "{source}\
fn probe() -> int {{
    let w: Wrap<bool> = Wrap {{ v: true }}
    let s = \"{{w}}\"
    return 1
}}
probe()
"
    ));
    assert!(
        message.contains("error[E0338]") && message.contains("denied"),
        "the hole covers Display like any other trait: {message}"
    );

    let through_a_generic_function = "\
fn show<T>(w: Wrap<T>) -> string {
    return \"{w}\"
}
fn probe() -> int {
    let w: Wrap<bool> = Wrap { v: true }
    return show(w).len()
}
probe()
";
    let through_a_bare_parameter = "\
fn show<T>(x: T) -> string {
    return \"{x}\"
}
fn probe() -> int {
    let w: Wrap<bool> = Wrap { v: true }
    return show(w).len()
}
probe()
";
    let through_a_generic_impl_method = "\
trait Show<T> {
    fn show(self, w: Wrap<T>) -> string;
}
struct S<T> { v: T }
impl<T> Show<T> for S<T> {
    fn show(self, w: Wrap<T>) -> string { return \"{w}\" }
}
fn probe() -> int {
    let s: S<bool> = S { v: true }
    let w: Wrap<bool> = Wrap { v: true }
    return s.show(w).len()
}
probe()
";
    for body in [
        through_a_generic_function,
        through_a_bare_parameter,
        through_a_generic_impl_method,
    ] {
        let message = compile_message(&format!("{source}{body}"));
        assert!(
            message.contains("error[E0338]") && message.contains("denied"),
            "a template is not a place the denial stops applying: {message}"
        );
    }

    assert_eq!(
        value_of(&format!(
            "{source}\
fn show<T>(w: Wrap<T>) -> string {{
    return \"{{w}}\"
}}
fn probe() -> int {{
    let w: Wrap<int> = Wrap {{ v: 1 }}
    return show(w).len()
}}
probe()
"
        )),
        1,
        "the type the hole does not name still displays, through the same template"
    );
}

/// a denial and a missing impl are two different things, and the repair differs:
#[test]
fn a_denied_bound_says_that_it_is_denied() {
    let denied = compile_message(
        "\
trait Describe {
    fn describe(self) -> int;
}
struct Holder<T> { value: T }
impl<T> Describe for Holder<T> {
    default fn describe(self) -> int { 1 }
}
impl !Describe for Holder<bool> {}
fn call<T: Describe>(x: T) -> int {
    return x.describe()
}
fn probe() -> int {
    let a: Holder<bool> = Holder { value: true }
    return call(a)
}
probe()
",
    );
    assert!(
        denied.contains("denied"),
        "the bound is not unimplemented, it is denied: {denied}"
    );
    let missing = compile_message(
        "\
trait Describe {
    fn describe(self) -> int;
}
struct Plain { value: int }
fn call<T: Describe>(x: T) -> int {
    return x.describe()
}
fn probe() -> int {
    let a: Plain = Plain { value: 1 }
    return call(a)
}
probe()
",
    );
    assert!(
        missing.contains("not implemented") && !missing.contains("denied"),
        "a type with no impl at all is not denied: {missing}"
    );

    // a denial with no positive impl beside it: the residual path finds nothing to
    let alone = "\
trait Describe {
    fn describe(self) -> int;
}
struct Holder<T> { value: T }
impl !Describe for Holder<bool> {}
";
    let through_a_bound = compile_message(&format!(
        "{alone}\
fn call<T: Describe>(x: T) -> int {{
    return x.describe()
}}
fn probe() -> int {{
    let a: Holder<bool> = Holder {{ value: true }}
    return call(a)
}}
probe()
"
    ));
    assert!(
        through_a_bound.contains("denied"),
        "the bound marker at a concrete receiver keeps the provenance: {through_a_bound}"
    );
    let through_a_qualified_call = compile_message(&format!(
        "{alone}\
fn probe() -> int {{
    let a: Holder<bool> = Holder {{ value: true }}
    return Describe::describe(a)
}}
probe()
"
    ));
    assert!(
        through_a_qualified_call.contains("denied"),
        "the qualified call keeps the provenance: {through_a_qualified_call}"
    );
}

/// a call inside a generic body carries no bound, so no obligation is left to
#[test]
fn a_denial_is_honoured_in_a_generic_body_without_a_bound() {
    let denied = "\
trait Describe {
    fn describe(self) -> int;
}
struct Holder<T> { value: T }
impl<T> Describe for Holder<T> {
    default fn describe(self) -> int { 1 }
}
impl !Describe for Holder<bool> {}
";
    let open = "\
trait Describe {
    fn describe(self) -> int;
}
struct Holder<T> { value: T }
impl<T> Describe for Holder<T> {
    default fn describe(self) -> int { 1 }
}
impl Describe for Holder<int> {
    fn describe(self) -> int { 2 }
}
";
    let body = "\
fn call<T>(x: Holder<T>) -> int {
    return x.describe()
}
fn probe() -> int {
    let a: Holder<bool> = Holder { value: true }
    return call(a)
}
probe()
";
    let message = compile_message(&format!("{denied}{body}"));
    assert!(
        message.contains("error[E0338]"),
        "a generic body is not a place the denial stops applying: {message}"
    );
    // selects through the generic body
    assert_eq!(
        value_of(&format!(
            "{open}\
fn call<T>(x: Holder<T>) -> int {{
    return x.describe()
}}
fn probe() -> int {{
    let a: Holder<int> = Holder {{ value: 7 }}
    let b: Holder<bool> = Holder {{ value: true }}
    return call(a) * 10 + call(b)
}}
probe()
"
        )),
        21,
        "the positive half of the same path must keep working"
    );
}

/// a parameter the source does not mention is still pinned by the signature the
#[test]
fn a_conversion_takes_the_target_its_signature_pins() {
    let source = "\
struct Plain { code: int }
struct Tagged<T> { code: int }
impl<T> From<Plain> for Tagged<T> {
    fn from(value: Plain) -> Tagged<T> { Tagged { code: value.code } }
}
";
    assert_eq!(
        value_of(&format!(
            "{source}\
fn relay(r: Result<int, Plain>) -> Result<int, Tagged<bool>> {{
    let v = r?
    return Result::Ok(v)
}}
fn probe() -> int {{
    let a: Result<int, Plain> = Result::Err(Plain {{ code: 3 }})
    match relay(a) {{
        Result::Ok(v) => v,
        Result::Err(e) => e.code,
    }}
}}
probe()
"
        )),
        3,
        "the return type of 'relay' names the instance the '?' needs"
    );
    assert_eq!(
        value_of(&format!(
            "{source}\
fn probe() -> int {{
    let p: Plain = Plain {{ code: 3 }}
    let t: Tagged<bool> = Tagged::from(p)
    t.code
}}
probe()
"
        )),
        3,
        "the twin written by hand, which never went through a '?'"
    );
}

#[test]
fn a_generic_conversion_names_its_instance_from_every_shape() {
    let conversion = "\
struct Pair<T> { tag: T, code: int }
struct Boxed<T> { tag: T, code: int }
impl<T> From<Pair<T>> for Boxed<T> {
    fn from(value: Pair<T>) -> Boxed<T> { Boxed { tag: value.tag, code: value.code } }
}
";
    let unwrap = "\
fn seen(r: Result<int, Boxed<bool>>) -> int {
    match r {
        Result::Ok(v) => v,
        Result::Err(e) => e.code,
    }
}
";

    // the `?` inside a generic function, instantiated once
    assert_eq!(
        value_of(&format!(
            "{conversion}{unwrap}\
fn relay<T>(r: Result<int, Pair<T>>) -> Result<int, Boxed<T>> {{
    let v = r?
    return Result::Ok(v)
}}
fn probe() -> int {{
    let a: Result<int, Pair<bool>> = Result::Err(Pair {{ tag: true, code: 3 }})
    seen(relay(a))
}}
probe()
"
        )),
        3,
        "a '?' in a generic function must name the instance for this instantiation"
    );

    // the same generic function at two instantiations, which share one span
    assert_eq!(
        value_of(&format!(
            "{conversion}\
fn relay<T>(r: Result<int, Pair<T>>) -> Result<int, Boxed<T>> {{
    let v = r?
    return Result::Ok(v)
}}
fn probe() -> int {{
    let a: Result<int, Pair<bool>> = Result::Err(Pair {{ tag: true, code: 3 }})
    let b: Result<int, Pair<int>> = Result::Err(Pair {{ tag: 7, code: 5 }})
    let x = match relay(a) {{ Result::Ok(v) => v, Result::Err(e) => e.code }}
    let y = match relay(b) {{ Result::Ok(v) => v, Result::Err(e) => e.code }}
    x * 10 + y
}}
probe()
"
        )),
        35,
        "two instantiations of one function share a span, and must not share an instance"
    );

    // the `?` inside the method of a generic impl
    assert_eq!(
        value_of(&format!(
            "{conversion}{unwrap}\
trait Relay<T> {{
    fn relay(self, r: Result<int, Pair<T>>) -> Result<int, Boxed<T>>;
}}
struct R<T> {{ v: T }}
impl<T> Relay<T> for R<T> {{
    fn relay(self, r: Result<int, Pair<T>>) -> Result<int, Boxed<T>> {{
        let v = r?
        return Result::Ok(v)
    }}
}}
fn probe() -> int {{
    let rr: R<bool> = R {{ v: true }}
    let a: Result<int, Pair<bool>> = Result::Err(Pair {{ tag: true, code: 3 }})
    seen(rr.relay(a))
}}
probe()
"
        )),
        3,
        "a '?' in a generic impl method is instantiated by the impl worklist"
    );
}

/// a generic conversion reached by `?` must name the instance, not the template
#[test]
fn a_generic_conversion_through_the_question_mark_names_its_instance() {
    let source = "\
struct Pair<T> { tag: T, code: int }
struct Boxed<T> { tag: T, code: int }
impl<T> From<Pair<T>> for Boxed<T> {
    fn from(value: Pair<T>) -> Boxed<T> { Boxed { tag: value.tag, code: value.code } }
}
fn inner() -> Result<int, Pair<bool>> {
    return Result::Err(Pair { tag: true, code: 3 })
}
fn probe() -> Result<int, Boxed<bool>> {
    let v = inner()?
    return Result::Ok(v)
}
match probe() {
    Result::Ok(v) => v,
    Result::Err(e) => e.code,
}
";
    assert_eq!(value_of(source), 3);
    let calls = calls_of(&compiled(source), "probe");
    assert_eq!(
        calls.len(),
        1,
        "the conversion is the only trait call: {calls:?}"
    );
    assert!(
        calls[0].contains(INSTANCE_INFIX),
        "the call names the template instead of the instance it needs: {calls:?}"
    );
}

/// reader acts on must read the same too, or the file order is still an input
#[test]
fn a_coherence_diagnostic_reads_the_same_in_either_order() {
    let pairs: [(&str, &str, &str); 4] = [
        (
            "\
trait D { fn d(self) -> int; }
struct P<A, B> { a: A, b: B }
",
            "impl<B> D for P<int, B> {\n    fn d(self) -> int { 1 }\n}\n",
            "impl<A> D for P<A, int> {\n    fn d(self) -> int { 2 }\n}\n",
        ),
        (
            "\
trait D { fn d(self) -> int; }
struct H<T> { v: T }
",
            "impl<T> D for H<T> {\n    default fn d(self) -> int { 1 }\n}\n",
            "impl<U> D for H<U> {\n    fn d(self) -> int { 2 }\n}\n",
        ),
        (
            "\
trait D { fn d(self) -> int; }
struct Point { x: int }
",
            "impl D for Point {\n    fn d(self) -> int { 1 }\n}\n",
            "impl !D for Point {}\n",
        ),
        (
            "\
trait D { fn d(self) -> int; }
struct H<T> { v: T }
",
            "impl<T> !D for H<T> {}\n",
            "impl<U> !D for H<U> {}\n",
        ),
    ];
    let tail = "fn probe() -> int { 1 }\nprobe()\n";
    // the snippet quotes the header the span lands on, which the two orders
    let sentence = |message: String| message.lines().next().unwrap_or_default().to_string();
    for (header, first, second) in pairs {
        let forward = sentence(compile_message(&format!("{header}{first}{second}{tail}")));
        let reversed = sentence(compile_message(&format!("{header}{second}{first}{tail}")));
        assert_eq!(
            forward, reversed,
            "the file order reached the text of the diagnostic"
        );
    }
}

/// `default fn` is how an author says this impl admits exceptions; an impl
#[test]
fn a_hole_in_a_closed_impl_is_e0432() {
    let message = compile_message(
        "\
trait Maker {
    fn make(self) -> int;
}
struct B<T> { v: T }
impl<T> Maker for B<T> {
    fn make(self) -> int { 1 }
}
impl !Maker for B<bool> {}
fn probe() -> int { 1 }
probe()
",
    );
    assert!(
        message.contains("error[E0432]"),
        "only an impl opened by 'default fn' admits a hole: {message}"
    );
}

#[test]
fn a_hole_in_a_closed_impl_is_e0432_in_either_order() {
    let message = compile_message(
        "\
trait Maker {
    fn make(self) -> int;
}
struct B<T> { v: T }
impl !Maker for B<bool> {}
impl<T> Maker for B<T> {
    fn make(self) -> int { 1 }
}
fn probe() -> int { 1 }
probe()
",
    );
    assert!(
        message.contains("error[E0432]"),
        "which header comes first may not decide: {message}"
    );
}

/// the denial declared first is the only shape that reaches
#[test]
fn a_hole_declared_before_the_impl_it_carves_still_carves_it() {
    let source = "\
trait Maker {
    fn make(self) -> int;
}
struct Box2<T> { v: T }
impl !Maker for Box2<bool> {}
impl<T> Maker for Box2<T> {
    default fn make(self) -> int { 6 }
}
";
    assert_eq!(
        value_of(&format!(
            "{source}\
fn probe() -> int {{
    let b: Box2<int> = Box2 {{ v: 1 }}
    b.make()
}}
probe()
"
        )),
        6,
        "the root still answers every type the hole does not name"
    );
    let message = compile_message(&format!(
        "{source}\
fn probe() -> int {{
    let b: Box2<bool> = Box2 {{ v: true }}
    b.make()
}}
probe()
"
    ));
    assert!(
        message.contains("error[E0338]"),
        "the hole is carved whichever header comes first: {message}"
    );
}

/// every site that asks whether an impl can receive a type must answer the same;
#[test]
fn an_impl_that_does_not_apply_changes_nothing_about_a_call() {
    let base = "\
trait Tr {
    fn m(self) -> int;
}
struct H<T, U> { n: int }
impl<A, B> Tr for H<A, B> {
    default fn m(self) -> int { return 40 }
}
";
    let diagonal = "impl<T> Tr for H<T, T> {\n    default fn m(self) -> int { return 20 }\n}\n";
    let vector = "impl<T> Tr for H<Vec<T>, T> {\n    default fn m(self) -> int { return 40 }\n}\n";
    let direct = "\
fn probe() -> int {
    let h: H<Vec<bool>, bool> = H { n: 0 }
    return h.m()
}
probe()
";
    let bound = "\
fn via<Z: Tr>(z: Z) -> int {
    return z.m()
}
fn probe() -> int {
    let h: H<Vec<bool>, bool> = H { n: 0 }
    return via(h)
}
probe()
";
    assert_eq!(
        value_of(&format!("{base}{vector}{direct}")),
        40,
        "the vector impl is the one that receives this type"
    );
    assert_eq!(
        value_of(&format!("{base}{diagonal}{vector}{direct}")),
        40,
        "the diagonal impl does not receive this type, so it decides nothing"
    );
    assert_eq!(
        value_of(&format!("{base}{diagonal}{vector}{bound}")),
        40,
        "and the two ways of writing the call agree"
    );
}

/// which means the two headers share no instance; following the binding instead
#[test]
fn two_headers_that_would_need_an_infinite_type_do_not_overlap() {
    assert_eq!(
        value_of(
            "\
trait D {
    fn d(self) -> int;
}
struct T3<A, B, C> { a: A, b: B, c: C }
impl<X> D for T3<X, X, X> {
    fn d(self) -> int { 1 }
}
impl<Y> D for T3<Y, Vec<Y>, Y> {
    fn d(self) -> int { 2 }
}
fn probe() -> int { 1 }
probe()
"
        ),
        1,
        "'Y' would have to be a vector of itself, so no type is in both headers"
    );
    // the same shape through the two other header predicates
    for denial in [
        "impl<Y> !D for T3<Y, Vec<Y>, Y> {}\n",
        "impl<Y> !D for T3<Y, Option<Y>, Y> {}\n",
    ] {
        assert_eq!(
            value_of(&format!(
                "\
trait D {{
    fn d(self) -> int;
}}
struct T3<A, B, C> {{ a: A, b: B, c: C }}
impl<X> D for T3<X, X, X> {{
    fn d(self) -> int {{ 1 }}
}}
{denial}fn probe() -> int {{ 1 }}
probe()
"
            )),
            1,
            "a denial that shares no instance with an impl contradicts nothing"
        );
    }
}

/// does not know it declares disjoint two headers that meet
#[test]
fn an_array_header_overlaps_like_the_vector_it_mirrors() {
    let shape = |inner: &str, concrete: &str| {
        format!(
            "\
trait D {{
    fn d(self) -> int;
}}
struct Holder<T> {{ v: T }}
impl<T> D for Holder<{inner}> {{
    fn d(self) -> int {{ 1 }}
}}
impl D for Holder<{concrete}> {{
    fn d(self) -> int {{ 2 }}
}}
fn probe() -> int {{ 1 }}
probe()
"
        )
    };
    let vector = compile_message(&shape("Vec<T>", "Vec<int>"));
    let array = compile_message(&shape("[T; 3]", "[int; 3]"));
    assert!(
        vector.contains("error[E0340]"),
        "the vector pair overlaps: {vector}"
    );
    assert_eq!(
        array.lines().next().map(|line| line
            .replace("[int; 3]", "Vec<int>")
            .replace("[T; 3]", "Vec<T>")),
        vector.lines().next().map(|line| line.to_string()),
        "an array header must read like the vector header it mirrors"
    );
}

/// two headers that no type satisfies at once do not overlap, whatever the
#[test]
fn two_headers_with_no_common_instance_do_not_overlap() {
    let source = "\
trait Tr {
    fn m(self) -> int;
}
struct H<T, U> { n: int }
impl<T> Tr for H<T, T> {
    fn m(self) -> int { return 1 }
}
impl Tr for H<bool, int> {
    fn m(self) -> int { return 2 }
}
";
    assert_eq!(
        value_of(&format!(
            "{source}\
fn probe() -> int {{
    let a: H<bool, bool> = H {{ n: 0 }}
    let b: H<bool, int> = H {{ n: 0 }}
    return a.m() * 10 + b.m()
}}
probe()
"
        )),
        12,
        "no type is both on the diagonal and <bool, int>, so the two are disjoint"
    );
    // and the pair that really does overlap is still refused
    let message = compile_message(
        "\
trait Tr {
    fn m(self) -> int;
}
struct H<T, U> { n: int }
impl<T> Tr for H<T, T> {
    fn m(self) -> int { return 1 }
}
impl Tr for H<bool, bool> {
    fn m(self) -> int { return 2 }
}
fn probe() -> int { 1 }
probe()
",
    );
    assert!(
        message.contains("error[E0340]") || message.contains("error[E0442]"),
        "<bool, bool> is on the diagonal, so these two do meet: {message}"
    );
}

/// two different parameters are two different obligations, and a numbering that
#[test]
fn two_parameters_are_two_obligations() {
    let source = "\
trait Super<A> {
    fn s(self, a: A) -> int;
}
struct H { n: int }
impl Super<int> for H {
    fn s(self, a: int) -> int { return 1 }
}
impl Super<bool> for H {
    fn s(self, a: bool) -> int { return 2 }
}
";
    let spelled = compile_message(&format!(
        "{source}\
fn g<X: Super<int> + Super<bool>>(x: X) -> int {{
    return x.s(1)
}}
fn probe() -> int {{
    return g(H {{ n: 0 }})
}}
probe()
"
    ));
    let parameters = compile_message(&format!(
        "{source}\
fn g<M, N, X: Super<M> + Super<N>>(x: X, m: M, n: N) -> int {{
    return x.s(1)
}}
fn probe() -> int {{
    return g(H {{ n: 0 }}, 1, true)
}}
probe()
"
    ));
    assert!(
        spelled.contains("error[E0437]"),
        "two instantiations spelled out are two: {spelled}"
    );
    assert!(
        parameters.contains("error[E0437]"),
        "two instantiations named by two parameters are two as well: {parameters}"
    );
}

/// obligations apart must not read them as one
#[test]
fn a_struct_and_a_parameter_of_the_same_name_are_two_obligations() {
    let shape = |parameter: &str| {
        format!(
            "\
trait Super<A> {{
    fn s(self, a: A) -> int;
}}
struct N {{ v: int }}
struct H {{ n: int }}
trait Sub: Super<N> {{
    fn b(self) -> int;
}}
impl Super<N> for H {{
    fn s(self, a: N) -> int {{ return a.v }}
}}
impl Super<bool> for H {{
    fn s(self, a: bool) -> int {{ if a {{ return 111 }}  return 222 }}
}}
impl Sub for H {{
    fn b(self) -> int {{ return 3 }}
}}
fn g<{parameter}, X: Sub + Super<{parameter}>>(x: X, n: {parameter}) -> int {{
    return x.b()
}}
fn probe() -> int {{
    let h = H {{ n: 0 }}
    return g(h, true)
}}
probe()
"
        )
    };
    assert_eq!(
        value_of(&shape("N")),
        3,
        "'Super<N>' the nominal and 'Super<N>' the parameter are two obligations, \
         and both are met"
    );
    // and when the parameter's obligation is **not** met, dropping it as a
    let unmet = compile_message(
        "\
trait Super<A> {
    fn s(self, a: A) -> int;
}
struct N { v: int }
struct H { n: int }
trait Sub: Super<N> {
    fn b(self) -> int;
}
impl Super<N> for H {
    fn s(self, a: N) -> int { return a.v }
}
impl Sub for H {
    fn b(self) -> int { return 3 }
}
fn g<N, X: Sub + Super<N>>(x: X, n: N) -> int {
    return x.b()
}
fn probe() -> int {
    let h = H { n: 0 }
    return g(h, true)
}
probe()
",
    );
    assert!(
        unmet.contains("error[E0338]"),
        "'Super<bool>' is required by the bound and 'H' does not have it: {unmet}"
    );
    assert_eq!(
        value_of(&shape("M")),
        3,
        "the name of a function's type parameter may not decide the verdict"
    );
}

/// a bound and a denial name an instantiation, and the diagnostic that judges them
#[test]
fn an_unsatisfied_bound_names_the_instantiation_it_judged() {
    let source = "\
trait Super<A> {
    fn s(self, a: A) -> int;
}
struct H { n: int }
impl Super<bool> for H {
    fn s(self, a: bool) -> int { if a { return 111 }  return 222 }
}
";
    let missing = compile_message(&format!(
        "{source}\
fn g<X: Super<int>>(x: X) -> int {{
    return x.s(7)
}}
fn probe() -> int {{
    let h = H {{ n: 0 }}
    return g(h)
}}
probe()
"
    ));
    assert!(
        missing.contains("Super<int>"),
        "'Super' is implemented for H; 'Super<int>' is what is missing: {missing}"
    );
    let denied = compile_message(&format!(
        "{source}impl !Super<int> for H {{}}\n\
fn g<X: Super<int>>(x: X) -> int {{
    return x.s(7)
}}
fn probe() -> int {{
    let h = H {{ n: 0 }}
    return g(h)
}}
probe()
"
    ));
    assert!(
        denied.contains("Super<int>"),
        "'Super' is not denied for H; 'Super<int>' is: {denied}"
    );
}

/// the listing of instantiations a call cannot choose between reads the impls, not
#[test]
fn the_instantiations_a_bound_cannot_choose_read_in_one_order() {
    let shape = |first: &str, second: &str| {
        format!(
            "\
trait Super<A> {{
    fn s(self, a: A) -> int;
}}
struct H {{ n: int }}
trait Sub<B>: Super<{first}> + Super<{second}> {{
    fn b(self) -> int;
}}
impl Super<bool> for H {{
    fn s(self, a: bool) -> int {{ return 1 }}
}}
impl Super<int> for H {{
    fn s(self, a: int) -> int {{ return 2 }}
}}
impl Sub<bool> for H {{
    fn b(self) -> int {{ return 3 }}
}}
fn g<X: Sub<bool>>(x: X) -> int {{
    return x.s(7)
}}
fn probe() -> int {{
    let h = H {{ n: 0 }}
    return g(h)
}}
probe()
"
        )
    };
    let forward = compile_message(&shape("B", "int"));
    let reversed = compile_message(&shape("int", "B"));
    assert_eq!(
        forward.lines().next(),
        reversed.lines().next(),
        "the order the supertraits were written in reached the sentence"
    );
}

/// a method of a generic impl has the impl's parameter already fixed by the
#[test]
fn a_method_of_a_generic_impl_keeps_the_parameter_its_receiver_fixed() {
    let through_a_method = compile_message(
        "\
struct Holder<A> { v: A }
impl<A> Holder<A> {
    fn get(self) -> A { return self.v }
    fn bad(self) -> bool { return self.get() }
}
fn probe() -> int {
    let h = Holder { v: \"no\" }
    if h.bad() { return 111 }
    return 222
}
probe()
",
    );
    assert!(
        through_a_method.contains("error[E0301]"),
        "'A' is fixed by the receiver and is not 'bool': {through_a_method}"
    );
    // the same value read straight from the field, which was always refused
    let through_the_field = compile_message(
        "\
struct Holder<A> { v: A }
impl<A> Holder<A> {
    fn bad(self) -> bool { return self.v }
}
fn probe() -> int {
    let h = Holder { v: \"no\" }
    if h.bad() { return 111 }
    return 222
}
probe()
",
    );
    assert!(
        through_the_field.contains("error[E0301]"),
        "the field and the method that returns it must agree: {through_the_field}"
    );
    assert_eq!(
        value_of(
            "\
struct Holder<A> { v: A }
impl<A> Holder<A> {
    fn get(self) -> A { return self.v }
    fn twice(self) -> A { return self.get() }
}
fn probe() -> int {
    let h = Holder { v: 7 }
    return h.twice()
}
probe()
"
        ),
        7,
        "a method returning the impl's parameter returns the receiver's type"
    );
}

#[test]
fn a_method_parameter_of_its_own_is_new_at_every_use() {
    let shape = |declared: &str, extra: &str, passed: &str| {
        format!(
            "\
trait Super {{
    fn ident<T>(self, t: T) -> T;
}}
struct H {{ n: int }}
impl Super for H {{
    fn ident<T>(self, t: T) -> T {{ return t }}
}}
fn g<{declared}X: Super>(x: X{extra}) -> int {{
    return x.ident(1) + 1
}}
fn probe() -> int {{
    return g(H {{ n: 0 }}{passed})
}}
probe()
"
        )
    };
    for (declared, extra, passed) in [
        ("", "", ""),
        ("Q, ", ", q: Q", ", true"),
        ("T, ", ", t: T", ", true"),
    ] {
        assert_eq!(
            value_of(&shape(declared, extra, passed)),
            2,
            "the name the enclosing function gives a parameter of its own decides nothing"
        );
    }
}

/// a method reached through a bound has the trait's parameters already replaced by
#[test]
fn a_bound_method_keeps_the_parameter_the_function_declared() {
    let source = "\
trait Super<A> {
    fn s(self) -> A;
}
struct H { n: int }
impl Super<string> for H {
    fn s(self) -> string { return \"no\" }
}
";
    // the return type is 'm', and 'm' is not 'bool'
    let wrong = compile_message(&format!(
        "{source}\
fn g<M, X: Super<M>>(x: X, m: M) -> bool {{
    return x.s()
}}
fn probe() -> int {{
    if g(H {{ n: 0 }}, \"a\") {{ return 111 }}
    return 222
}}
probe()
"
    ));
    assert!(
        wrong.contains("error[E0301]"),
        "the bound gives 'M', and the signature promises 'bool': {wrong}"
    );
    // the same written with the argument spelled out, which was already refused
    let spelled = compile_message(&format!(
        "{source}\
fn g<X: Super<string>>(x: X) -> bool {{
    return x.s()
}}
fn probe() -> int {{
    if g(H {{ n: 0 }}) {{ return 111 }}
    return 222
}}
probe()
"
    ));
    assert!(
        spelled.contains("error[E0301]"),
        "the two spellings of one bound must agree: {spelled}"
    );
    assert_eq!(
        value_of(&format!(
            "{source}\
fn g<M, X: Super<M>>(x: X, m: M) -> M {{
    return x.s()
}}
fn probe() -> int {{
    return g(H {{ n: 0 }}, \"a\").len()
}}
probe()
"
        )),
        2,
        "the signature that says 'M' gets 'M', and 'M' is 'string' here"
    );
    // a generic function calling itself at a bigger type still reaches its own
    let recursive = compile_message(
        "\
struct Pair<A, B> { a: A, b: B }
fn grow<T>(t: T) -> int {
    return grow(Pair { a: t, b: t })
}
fn probe() -> int {
    return grow(1)
}
probe()
",
    );
    assert!(
        recursive.contains("error[E0344]"),
        "the callee's own parameters are still freshened at each use: {recursive}"
    );
}

/// a trait that takes parameters is not a type until they are given: naming it
#[test]
fn a_parameterised_trait_named_without_its_arguments_is_refused() {
    let source = "\
trait Super<A> {
    fn s(self) -> A;
}
struct H { n: int }
impl Super<string> for H {
    fn s(self) -> string { return \"hi\" }
}
";
    // in a bound
    let bound = compile_message(&format!(
        "{source}\
fn g<X: Super>(x: X) -> int {{
    return x.s() + 1
}}
fn probe() -> int {{
    let h = H {{ n: 0 }}
    return g(h)
}}
probe()
"
    ));
    assert!(
        bound.contains("error[E0357]"),
        "'Super' takes one argument and the bound gives none: {bound}"
    );
    // in a supertrait
    let supertrait = compile_message(&format!(
        "{source}\
trait Sub: Super {{
    fn b(self) -> int;
}}
fn probe() -> int {{ 1 }}
probe()
"
    ));
    assert!(
        supertrait.contains("error[E0357]"),
        "a supertrait names its arguments like any other mention: {supertrait}"
    );
    // in an impl header
    let header = compile_message(
        "\
trait Super<A> {
    fn s(self) -> A;
}
struct H { n: int }
impl Super for H {
    fn s(self) -> string { return \"hi\" }
}
fn probe() -> int { 1 }
probe()
",
    );
    assert!(
        header.contains("error[E0357]"),
        "an impl header names them too: {header}"
    );
    assert_eq!(
        value_of(&format!(
            "{source}\
fn g<X: Super<string>>(x: X) -> string {{
    return x.s()
}}
fn probe() -> int {{
    let h = H {{ n: 0 }}
    return g(h).len()
}}
probe()
"
        )),
        2,
        "the bound written with its argument selects the impl and keeps its type"
    );
}

/// never meet a denial that names one, so a written `impl !` was invisible
#[test]
fn a_denial_is_not_hidden_by_a_bound_that_omits_its_arguments() {
    let message = compile_message(
        "\
trait Super<A> {
    fn s(self, a: A) -> int;
}
struct H { n: int }
impl Super<bool> for H {
    fn s(self, a: bool) -> int { if a { return 111 }  return 222 }
}
impl !Super<int> for H {}
fn g<X: Super>(x: X) -> int {
    return x.s(7)
}
fn probe() -> int {
    let h = H { n: 0 }
    return g(h)
}
probe()
",
    );
    assert!(
        message.contains("error[E0357]") || message.contains("error[E0338]"),
        "the program denies the instantiation this call needs: {message}"
    );
}

/// a supertrait named twice at two instantiations lays two obligations; keeping
#[test]
fn a_supertrait_named_twice_lays_both_its_obligations() {
    let source = "\
trait S<A> {
    fn s(self, a: A) -> int;
}
trait T: S<bool> + S<int> {
    fn t(self) -> int;
}
struct H { n: int }
";
    let only_bool = format!(
        "{source}\
impl S<bool> for H {{
    fn s(self, a: bool) -> int {{ if a {{ return 111 }}  return 222 }}
}}
impl T for H {{
    fn t(self) -> int {{ return 0 }}
}}
fn probe() -> int {{ 1 }}
probe()
"
    );
    let message = compile_message(&only_bool);
    assert!(
        message.contains("error[E0338]"),
        "'T' requires 'S<int>' too, and 'H' does not have it: {message}"
    );
    // and it says which instantiation is missing, since the other one is present
    assert!(
        message.contains("S<int>"),
        "naming the trait without its instantiation would be false here: {message}"
    );

    for present in ["bool", "int"] {
        let missing = if present == "bool" { "int" } else { "bool" };
        let one = format!(
            "{source}\
impl S<{present}> for H {{
    fn s(self, a: {present}) -> int {{ return 1 }}
}}
impl T for H {{
    fn t(self) -> int {{ return 0 }}
}}
fn probe() -> int {{ 1 }}
probe()
"
        );
        let message = compile_message(&one);
        assert!(
            message.contains("error[E0338]"),
            "'S<{missing}>' is required and absent: {message}"
        );
    }

    // both present: the impl is legal. a call on either is e0437 by d7, two
    assert_eq!(
        value_of(&format!(
            "{source}\
impl S<bool> for H {{
    fn s(self, a: bool) -> int {{ if a {{ return 10 }}  return 20 }}
}}
impl S<int> for H {{
    fn s(self, a: int) -> int {{ return a + 1 }}
}}
impl T for H {{
    fn t(self) -> int {{ return 7 }}
}}
fn probe() -> int {{
    let h = H {{ n: 0 }}
    return h.t()
}}
probe()
"
        )),
        7,
        "both obligations met, the impl of the subtrait stands"
    );
}

/// a subtrait that passes its own parameter to one supertrait and a fixed type to
#[test]
fn a_subtrait_lays_every_supertrait_it_names() {
    let source = "\
trait Super<A> {
    fn s(self, a: A) -> int;
}
trait Sub<B>: Super<B> + Super<int> {
    fn b(self) -> int;
}
struct H { n: int }
impl Super<bool> for H {
    fn s(self, a: bool) -> int { if a { return 111 }  return 222 }
}
";
    let message = compile_message(&format!(
        "{source}\
impl Sub<bool> for H {{
    fn b(self) -> int {{ return 2 }}
}}
fn probe() -> int {{ 1 }}
probe()
"
    ));
    assert!(
        message.contains("error[E0338]"),
        "'Sub<bool>' requires 'Super<int>' as well: {message}"
    );
    let denied = compile_message(&format!(
        "{source}\
impl Super<int> for H {{
    fn s(self, a: int) -> int {{ return a }}
}}
impl !Super<int> for H {{}}
impl Sub<bool> for H {{
    fn b(self) -> int {{ return 2 }}
}}
fn probe() -> int {{ 1 }}
probe()
"
    ));
    assert!(
        denied.contains("error[E0432]") || denied.contains("error[E0338]"),
        "the denial of an instantiation the subtrait requires refuses it: {denied}"
    );
}

/// a supertrait is written with its arguments, and the obligation it lays on an
#[test]
fn a_supertrait_obligation_reads_its_instantiation() {
    let source = "\
trait Super<A> {
    fn s(self, a: A) -> int;
}
trait Sub: Super<int> {
    fn b(self) -> int;
}
struct H { n: int }
";
    let wrong = compile_message(&format!(
        "{source}\
impl Super<bool> for H {{
    fn s(self, a: bool) -> int {{ if a {{ return 111 }}  return 222 }}
}}
impl Sub for H {{
    fn b(self) -> int {{ return 2 }}
}}
fn viasub<X: Sub>(x: X) -> int {{
    return x.s(7)
}}
fn probe() -> int {{
    let h = H {{ n: 0 }}
    return viasub(h)
}}
probe()
"
    ));
    assert!(
        wrong.contains("error[E0338]"),
        "'H' implements 'Super<bool>', and 'Sub' requires 'Super<int>': {wrong}"
    );
    assert_eq!(
        value_of(&format!(
            "{source}\
impl Super<int> for H {{
    fn s(self, a: int) -> int {{ return a + 1 }}
}}
impl Sub for H {{
    fn b(self) -> int {{ return 2 }}
}}
fn viasub<X: Sub>(x: X) -> int {{
    return x.s(7)
}}
fn probe() -> int {{
    let h = H {{ n: 0 }}
    return viasub(h)
}}
probe()
"
        )),
        8,
        "the impl that really carries the required instantiation answers"
    );
}

/// a denial of the very instantiation a supertrait requires contradicts the impl
#[test]
fn a_denied_supertrait_instantiation_refuses_the_subtrait_impl() {
    let message = compile_message(
        "\
trait Super<A> {
    fn s(self, a: A) -> int;
}
trait Sub: Super<int> {
    fn b(self) -> int;
}
struct H { n: int }
impl Super<bool> for H {
    fn s(self, a: bool) -> int { if a { return 111 }  return 222 }
}
impl !Super<int> for H {}
impl Sub for H {
    fn b(self) -> int { return 2 }
}
fn viasub<X: Sub>(x: X) -> int {
    return x.s(7)
}
fn probe() -> int {
    let h = H { n: 0 }
    return viasub(h)
}
probe()
",
    );
    assert!(
        message.contains("error[E0338]") || message.contains("error[E0432]"),
        "the program denies the very thing 'Sub' requires: {message}"
    );
}

/// a subtrait passes its own parameters to its supertrait, so the obligation an
#[test]
fn a_supertrait_argument_follows_the_subtrait_that_names_it() {
    let source = "\
trait Super<A> {
    fn s(self, a: A) -> int;
}
trait Sub<B>: Super<B> {
    fn b(self) -> int;
}
struct H { n: int }
impl Super<bool> for H {
    fn s(self, a: bool) -> int { if a { return 1 }  return 2 }
}
";
    let wrong = compile_message(&format!(
        "{source}\
impl Sub<int> for H {{
    fn b(self) -> int {{ return 2 }}
}}
fn probe() -> int {{ 1 }}
probe()
"
    ));
    assert!(
        wrong.contains("error[E0338]"),
        "'Sub<int>' requires 'Super<int>', which 'H' does not have: {wrong}"
    );
    assert_eq!(
        value_of(&format!(
            "{source}\
impl Sub<bool> for H {{
    fn b(self) -> int {{ return 2 }}
}}
fn viasub<X: Sub<bool>>(x: X) -> int {{
    return x.s(true)
}}
fn probe() -> int {{
    let h = H {{ n: 0 }}
    return viasub(h)
}}
probe()
"
        )),
        1,
        "'Sub<bool>' requires 'Super<bool>', which it has"
    );
}

/// the name of a type parameter is not part of a header's identity, so renaming
#[test]
fn two_headers_equal_up_to_renaming_are_one_instantiation() {
    let shape = |parameter: &str| {
        format!(
            "\
trait Feed<R> {{
    fn feed(self, item: R) -> int;
}}
struct Holder<A, B> {{ a: A, b: B }}
impl<A, B> Feed<A> for Holder<A, B> {{
    default fn feed(self, item: A) -> int {{ return 1 }}
}}
impl<{parameter}> Feed<{parameter}> for Holder<{parameter}, int> {{
    fn feed(self, item: {parameter}) -> int {{ return 2 }}
}}
fn probe() -> int {{
    let h = Holder {{ a: true, b: 7 }}
    return h.feed(false)
}}
probe()
"
        )
    };
    assert_eq!(value_of(&shape("A")), 2, "the replacement answers");
    assert_eq!(
        value_of(&shape("Z")),
        2,
        "and it still answers when its parameter is spelled differently"
    );
}

/// a bound reads the whole header of the impl it claims to satisfy: the same
#[test]
fn a_bound_reads_the_whole_header_it_claims_to_satisfy() {
    let source = "\
trait Conv<A> {
    fn c(self, a: A) -> A;
}
struct Holder<T> { v: T }
impl<T> Conv<T> for Holder<T> {
    fn c(self, a: T) -> T { return self.v }
}
fn authorized<X: Conv<bool>>(x: X) -> bool {
    return x.c(false)
}
";
    // 'holder<int>' implements 'conv<int>' and never 'conv<bool>'
    let message = compile_message(&format!(
        "{source}\
fn probe() -> int {{
    let h: Holder<int> = Holder {{ v: 0 }}
    if authorized(h) {{
        return 111
    }}
    return 222
}}
probe()
"
    ));
    assert!(
        message.contains("error[E0338]"),
        "the bound may not accept an instantiation the receiver does not have: {message}"
    );
    // the receiver that really does implement it answers, and answers honestly
    assert_eq!(
        value_of(&format!(
            "{source}\
fn probe() -> int {{
    let h: Holder<bool> = Holder {{ v: false }}
    if authorized(h) {{
        return 111
    }}
    return 222
}}
probe()
"
        )),
        222,
        "the honest twin returns what its own value says"
    );
    // a trait whose two arguments come from one parameter is not satisfied by two
    let two = compile_message(
        "\
trait Pairwise<A, B> {
    fn p(self) -> int;
}
struct Holder<T> { v: T }
impl<T> Pairwise<T, T> for Holder<T> {
    fn p(self) -> int { return 1 }
}
fn need<X: Pairwise<int, string>>(x: X) -> int {
    return x.p()
}
fn probe() -> int {
    let h: Holder<int> = Holder { v: 0 }
    return need(h)
}
probe()
",
    );
    assert!(
        two.contains("error[E0338]"),
        "'T' cannot be both 'int' and 'string': {two}"
    );
}

/// the overlap question is one question about one pair of headers, so the same
#[test]
fn two_headers_that_disagree_on_one_parameter_do_not_overlap() {
    let disjoint = "\
trait D2<A> {
    fn d(self) -> int;
}
struct H1<A> { a: A }
impl<T> D2<T> for H1<T> {
    fn d(self) -> int { return 1 }
}
";
    assert_eq!(
        value_of(&format!(
            "{disjoint}impl D2<bool> for H1<int> {{\n    fn d(self) -> int {{ return 2 }}\n}}\n\
fn probe() -> int {{ return 5 }}
probe()
"
        )),
        5,
        "'H1<int>: D2<bool>' is not in the family the first header names"
    );
    assert_eq!(
        value_of(&format!(
            "{disjoint}impl !D2<bool> for H1<int> {{}}\n\
fn probe() -> int {{ return 5 }}
probe()
"
        )),
        5,
        "and the denial of that same pair contradicts nothing"
    );
    // the pair that really does meet is still refused
    let message = compile_message(&format!(
        "{disjoint}impl D2<int> for H1<int> {{\n    fn d(self) -> int {{ return 2 }}\n}}\n\
fn probe() -> int {{ return 5 }}
probe()
"
    ));
    assert!(
        message.contains("error[E0340]") || message.contains("error[E0433]"),
        "'H1<int>: D2<int>' is in the family the first header names: {message}"
    );
}

/// the matcher a bound reads must know every constructor the language writes, or
#[test]
fn a_bound_finds_its_impl_through_every_constructor() {
    let shape = |argument: &str, concrete: &str| {
        format!(
            "\
struct Holder<T> {{ v: T }}
trait Conv<A> {{
    fn c(self) -> int;
}}
impl<T> Conv<{argument}> for Holder<T> {{
    fn c(self) -> int {{ return 11 }}
}}
fn need<X: Conv<{concrete}>>(x: X) -> int {{
    return x.c()
}}
fn probe() -> int {{
    let h: Holder<int> = Holder {{ v: 1 }}
    return need(h)
}}
probe()
"
        )
    };
    for (argument, concrete) in [
        ("Vec<T>", "Vec<int>"),
        ("Option<T>", "Option<int>"),
        ("[T; 3]", "[int; 3]"),
        ("fn(T) -> int", "fn(int) -> int"),
    ] {
        assert_eq!(
            value_of(&shape(argument, concrete)),
            11,
            "the impl written with '{argument}' answers the bound written with '{concrete}'"
        );
    }
}

/// the denial is asked about the trait the receiver actually reaches, so the
#[test]
fn a_denied_instantiation_is_refused_through_a_generic_root() {
    let source = "\
trait Tr<R> {
    fn m(self) -> int;
}
struct H<T> { n: int }
impl<T> Tr<T> for H<T> {
    default fn m(self) -> int { return 10 }
}
";
    let message = compile_message(&format!(
        "{source}impl !Tr<int> for H<int> {{}}\
fn probe() -> int {{
    let h: H<int> = H {{ n: 0 }}
    return h.m()
}}
probe()
"
    ));
    assert!(
        message.contains("error[E0338]"),
        "the root reaches 'Tr<int>' for this receiver, which is what is denied: {message}"
    );
    assert_eq!(
        value_of(&format!(
            "{source}impl !Tr<int> for H<int> {{}}\
fn probe() -> int {{
    let h: H<bool> = H {{ n: 0 }}
    return h.m()
}}
probe()
"
        )),
        10,
        "the instantiation the denial does not name still answers"
    );
    assert_eq!(
        value_of(&format!(
            "{source}\
fn probe() -> int {{
    let h: H<int> = H {{ n: 0 }}
    return h.m()
}}
probe()
"
        )),
        10,
        "without the denial the same call answers"
    );
    // the same denial reached through a bound, which asks with concrete arguments
    let through_a_bound = compile_message(&format!(
        "{source}impl !Tr<int> for H<int> {{}}\
fn via<Z: Tr<int>>(z: Z) -> int {{
    return z.m()
}}
fn probe() -> int {{
    let h: H<int> = H {{ n: 0 }}
    return via(h)
}}
probe()
"
    ));
    assert!(
        through_a_bound.contains("error[E0338]"),
        "both ways of writing the call must agree: {through_a_bound}"
    );
}

/// a header that repeats a parameter names the types where the two occurrences
#[test]
fn a_repeated_parameter_denies_only_where_its_occurrences_agree() {
    let source = "\
trait Tr {
    fn m(self) -> int;
}
struct H<T, U> { n: int }
impl<A, B> Tr for H<A, B> {
    default fn m(self) -> int { return 10 }
}
impl<T> !Tr for H<T, T> {}
";
    let message = compile_message(&format!(
        "{source}\
fn probe() -> int {{
    let h: H<bool, bool> = H {{ n: 0 }}
    return h.m()
}}
probe()
"
    ));
    assert!(
        message.contains("error[E0338]"),
        "the diagonal is what the denial names: {message}"
    );
    assert_eq!(
        value_of(&format!(
            "{source}\
fn probe() -> int {{
    let h: H<bool, int> = H {{ n: 0 }}
    return h.m()
}}
probe()
"
        )),
        10,
        "off the diagonal the denial says nothing, and the root answers"
    );
    // the same pair reached through a bound, where another selector reads the header
    assert_eq!(
        value_of(&format!(
            "{source}\
fn via<Z: Tr>(z: Z) -> int {{
    return z.m()
}}
fn probe() -> int {{
    let h: H<bool, int> = H {{ n: 0 }}
    return via(h)
}}
probe()
"
        )),
        10,
        "the bound reads the same header and must read it the same way"
    );
}

/// an impl whose header repeats a parameter is not applicable to a receiver whose
#[test]
fn a_repeated_parameter_impl_is_not_applicable_off_its_diagonal() {
    let source = "\
trait Tr {
    fn m(self) -> int;
}
struct H<T, U> { n: int }
impl<A, B> Tr for H<A, B> {
    default fn m(self) -> int { return 10 }
}
impl<T> Tr for H<T, T> {
    fn m(self) -> int { return 1 }
}
";
    assert_eq!(
        value_of(&format!(
            "{source}\
fn probe() -> int {{
    let h: H<bool, bool> = H {{ n: 0 }}
    return h.m()
}}
probe()
"
        )),
        1,
        "on the diagonal the narrower impl answers"
    );
    assert_eq!(
        value_of(&format!(
            "{source}\
fn probe() -> int {{
    let h: H<bool, int> = H {{ n: 0 }}
    return h.m()
}}
probe()
"
        )),
        10,
        "off it, the only applicable impl is the general one"
    );
    assert_eq!(
        value_of(&format!(
            "{source}\
fn via<Z: Tr>(z: Z) -> int {{
    return z.m()
}}
fn probe() -> int {{
    let a: H<bool, bool> = H {{ n: 0 }}
    let b: H<bool, int> = H {{ n: 0 }}
    return via(a) * 100 + via(b)
}}
probe()
"
        )),
        110,
        "the bound selector reads the header the same way"
    );
}

/// a denial names one instantiation of a generic trait; the impls of the other
#[test]
fn a_denial_of_one_instantiation_leaves_the_others() {
    let source = "\
trait Feed<R> {
    fn feed(self, item: R) -> int;
}
struct H<T> { v: T }
impl Feed<int> for H<int> {
    fn feed(self, item: int) -> int { 5 }
}
";
    let call = "\
fn probe() -> int {
    let h: H<int> = H { v: 3 }
    h.feed(1)
}
probe()
";
    assert_eq!(
        value_of(&format!("{source}impl !Feed<bool> for H<int> {{}}\n{call}")),
        5,
        "denying 'Feed<bool>' says nothing about 'Feed<int>'"
    );
    assert_eq!(
        value_of(&format!("{source}{call}")),
        5,
        "the counter witness without the denial must answer the same"
    );
}

/// the same denial seen through a bound, where the residual carries the
#[test]
fn a_denial_of_one_instantiation_leaves_a_bound_on_another() {
    assert_eq!(
        value_of(
            "\
trait Feed<R> {
    fn feed(self, item: R) -> int;
}
struct H<T> { v: T }
impl Feed<int> for H<int> {
    fn feed(self, item: int) -> int { 5 }
}
impl !Feed<bool> for H<int> {}
fn via<T: Feed<int>>(v: T) -> int {
    v.feed(1)
}
fn probe() -> int {
    let h: H<int> = H { v: 3 }
    via(h)
}
probe()
"
        ),
        5,
        "a bound on 'Feed<int>' is satisfied by an impl of 'Feed<int>'"
    );
}

/// a trait's own default body and `default fn` are two different things: the
#[test]
fn a_trait_default_body_is_not_a_specialization_marker() {
    assert_eq!(
        value_of(
            "\
trait Rank {
    fn rank(self) -> int {
        return 9
    }
    fn tag(self) -> int;
}
struct C<T> { h: T }
impl<T> Rank for C<T> {
    default fn rank(self) -> int { 1 }
    default fn tag(self) -> int { 0 }
}
impl Rank for C<int> {
    fn rank(self) -> int { 2 }
    fn tag(self) -> int { 0 }
}
fn probe() -> int {
    let a: C<bool> = C { h: true }
    let b: C<int> = C { h: 7 }
    a.rank() * 10 + b.rank()
}
probe()
"
        ),
        12,
        "the trait's 9 is reached by neither: both impls write the method"
    );
}

/// only a denial more specific than a positive header carves a hole in it; a
#[test]
fn a_denial_more_general_than_a_positive_impl_is_e0432() {
    let message = compile_message(
        "\
trait Maker {
    fn make(self) -> int;
}
struct Box2<T> { v: T }
impl Maker for Box2<int> {
    fn make(self) -> int { 6 }
}
impl<T> !Maker for Box2<T> {}
fn probe() -> int {
    let b: Box2<int> = Box2 { v: 1 }
    b.make()
}
probe()
",
    );
    assert!(
        message.contains("error[E0432]"),
        "a blanket denial cannot be overridden by a narrower impl: {message}"
    );
}

#[test]
fn a_denied_conversion_is_refused_where_the_question_mark_selects_it() {
    let message = compile_message(
        "\
struct Wrapper { held: int }
struct MyErr { code: int }
impl From<Wrapper> for MyErr {
    fn from(value: Wrapper) -> MyErr { MyErr { code: value.held } }
}
impl !From<Wrapper> for MyErr {}
fn inner() -> Result<int, Wrapper> {
    return Result::Err(Wrapper { held: 3 })
}
fn probe() -> Result<int, MyErr> {
    let v = inner()?
    return Result::Ok(v)
}
match probe() {
    Result::Ok(v) => v,
    Result::Err(e) => e.code,
}
",
    );
    assert!(
        message.contains("error[E0432]"),
        "a conversion both provided and denied contradicts, whichever site selects it: {message}"
    );
}

#[test]
fn e0437_lists_its_instantiations_in_one_order() {
    let feed = |int_first: bool| {
        let on_int = "impl<T> Feed<int> for H<T> {\n    fn feed(self, item: int) -> int { 1 }\n}\n";
        let on_bool =
            "impl Feed<bool> for H<int> {\n    fn feed(self, item: bool) -> int { 2 }\n}\n";
        let (first, second) = if int_first {
            (on_int, on_bool)
        } else {
            (on_bool, on_int)
        };
        format!(
            "\
trait Feed<R> {{
    fn feed(self, item: R) -> int;
}}
struct H<T> {{ v: T }}
{first}{second}fn show<T: Feed<int>>(v: T) -> int {{
    v.feed(1)
}}
fn probe() -> int {{
    let h: H<int> = H {{ v: 3 }}
    show(h)
}}
probe()
"
        )
    };
    assert_eq!(
        compile_message(&feed(true)),
        compile_message(&feed(false)),
        "the listing of instantiations must not read the source order"
    );
}

/// the caller's parameter is rigid at the call, the method's own parameter is
#[test]
fn a_method_keeps_its_own_parameter_when_the_caller_names_it_too() {
    let inherent = "\
struct H { n: int }
impl H {
    fn ident<T>(self, t: T) -> T { return t }
}
fn g<T>(x: H, t: T) -> int {
    return x.ident(1) + 1
}
fn probe() -> int {
    return g(H { n: 0 }, true)
}
probe()
";
    let through_trait = "\
trait Super {
    fn ident<T>(self, t: T) -> T;
}
struct H { n: int }
impl Super for H {
    fn ident<T>(self, t: T) -> T { return t }
}
fn g<T>(x: H, t: T) -> int {
    return x.ident(1) + 1
}
fn probe() -> int {
    return g(H { n: 0 }, true)
}
probe()
";
    let through_bound = "\
trait Super {
    fn ident<T>(self, t: T) -> T;
}
struct H { n: int }
impl Super for H {
    fn ident<T>(self, t: T) -> T { return t }
}
fn g<T, X: Super>(x: X, t: T) -> int {
    return x.ident(1) + 1
}
fn probe() -> int {
    return g(H { n: 0 }, true)
}
probe()
";
    assert_eq!(
        value_of(inherent),
        2,
        "an inherent method's own parameter is deduced from the argument"
    );
    assert_eq!(
        value_of(through_trait),
        2,
        "a trait impl's method keeps the parameter its own declaration gives it"
    );
    assert_eq!(
        value_of(through_bound),
        2,
        "the same method reached through a bound answers the same"
    );
}

/// a qualified call has no receiver to fix anything, so nothing else freshens
#[test]
fn a_qualified_call_freshens_the_method_own_parameters() {
    let alone = "\
trait Super {
    fn ident<P>(self, t: P) -> P;
}
struct H { n: int }
impl Super for H {
    fn ident<P>(self, t: P) -> P { return t }
}
fn probe() -> int {
    let h = H { n: 0 }
    return Super::ident(h, 1) + 1
}
probe()
";
    let under_a_caller_of_the_same_name = "\
trait Super {
    fn ident<T>(self, t: T) -> T;
}
struct H { n: int }
impl Super for H {
    fn ident<T>(self, t: T) -> T { return t }
}
fn g<T>(x: H, t: T) -> int {
    return Super::ident(x, 1) + 1
}
fn probe() -> int {
    return g(H { n: 0 }, true)
}
probe()
";
    assert_eq!(
        value_of(alone),
        2,
        "the method's own parameter is deduced from the argument the path supplies"
    );
    assert_eq!(
        value_of(under_a_caller_of_the_same_name),
        2,
        "an enclosing parameter of the same spelling does not reach into the path"
    );
}

/// a method's own type parameter and one of the same spelling on the impl or on
#[test]
fn a_method_own_parameter_is_not_the_one_its_nominal_declares() {
    let crossing = "\
struct P<A> { v: A }
impl<A> P<A> {
    fn get<A>(self) -> A { return self.v }
}
fn probe() -> bool {
    let p = P { v: 7 }
    return p.get()
}
probe()
";
    let deduced_from_the_argument = "\
struct P<A> { v: A }
impl<A> P<A> {
    fn swap<A>(self, other: P<A>) -> A { return other.v }
}
fn probe() -> int {
    let p = P { v: 7 }
    let q = P { v: true }
    if p.swap(q) { return 111 }
    return 222
}
probe()
";
    let widening = "\
trait Pair<B> {
    fn mk<A>(self, a: A, b: B) -> B;
}
struct P<A> { v: A }
impl<A> Pair<A> for P<A> {
    fn mk<A>(self, a: A, b: A) -> A { return b }
}
fn probe() -> bool {
    let p = P { v: 7 }
    return p.mk(true, true)
}
probe()
";
    let refused = compile_message(crossing);
    assert!(
        refused.contains("error[E0301]"),
        "a body that returns the nominal's parameter where the method's was \
         declared is refused: {refused}"
    );
    assert!(
        refused.contains("of the method"),
        "and the two spellings must not read alike in the message: {refused}"
    );
    assert_eq!(
        value_of(deduced_from_the_argument),
        111,
        "the method's own parameter is the argument's type, not the receiver's"
    );
    let widened = compile_message(widening);
    assert!(
        widened.contains("error[E0336]"),
        "a position the trait fixes is not open to a method's own parameter: {widened}"
    );
}

/// a specialized method is reached with two impls in scope, where an unspecialized
#[test]
fn a_specialized_method_keeps_its_own_parameter() {
    assert_eq!(
        value_of(
            "\
trait Tr {
    fn pick<T>(self, t: T) -> T;
}
struct H<A> { v: A }
impl<A> Tr for H<A> {
    default fn pick<T>(self, t: T) -> T { return t }
}
impl Tr for H<int> {
    fn pick<T>(self, t: T) -> T { return t }
}
fn g<T>(x: H<int>, t: T) -> int {
    return x.pick(1) + 1
}
fn probe() -> int {
    let h: H<int> = H { v: 0 }
    return g(h, true)
}
probe()
"
        ),
        2,
        "the specialized method's own parameter is deduced from the argument"
    );
}
