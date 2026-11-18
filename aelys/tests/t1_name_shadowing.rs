use aelys_opt::OptimizationLevel;

fn int_at_every_level(source: &str) -> i64 {
    let mut seen = None;
    for level in [
        OptimizationLevel::None,
        OptimizationLevel::Standard,
        OptimizationLevel::Aggressive,
    ] {
        let mut vm = aelys::new_vm().expect("vm");
        let value = aelys::run_with_vm_and_opt(&mut vm, source, "<t1>", level)
            .unwrap_or_else(|error| panic!("{level:?}: {error}\n{source}"));
        let value = value
            .as_int()
            .unwrap_or_else(|| panic!("{level:?}: not an int: {value:?}\n{source}"));
        if let Some(previous) = seen {
            assert_eq!(previous, value, "{level:?} disagrees\n{source}");
        }
        seen = Some(value);
    }
    seen.expect("at least one level ran")
}

const STDLIB_NAMES: &[&str] = &[
    "sqrt",
    "sin",
    "cos",
    "tan",
    "exp",
    "log",
    "pow",
    "find",
    "len",
    "min",
    "max",
    "clamp",
    "now",
    "contains",
    "split",
    "replace",
    "count",
    "abs",
    "floor",
    "random",
    "to_string",
    "to_int",
    "parse_int",
    "trim",
];

fn shadowing_program(name: &str) -> String {
    format!(
        r#"
fn {name}(t: int) -> int {{
    for i in 0..3 {{ if i == t {{ return i }} }}
    t + 100
}}
struct Holder {{ v: int }}
impl Holder {{
    fn through_method(self) -> int {{ {name}(self.v) }}
}}
fn second(a: int, b: int, c: int) -> int {{ b }}
fn returned() -> int {{ return {name}(4) }}
fn nested() -> int {{
    fn inner() -> int {{ second(0, {name}(4), 1) }}
    inner()
}}
let in_argument = second(0, {name}(4), 1)
let in_vec = vec![{name}(4), {name}(4)][1]
let in_array = [{name}(4), 0][0]
let in_field = Holder {{ v: {name}(4) }}.v
let in_closure = fn() {{ return second(0, {name}(4), 1) }}
let in_method = Holder {{ v: 4 }}.through_method()
let as_value = {name}
let through_value = as_value(4)
in_argument + in_vec + in_array + in_field + in_closure() + in_method + returned() + nested() + through_value + {name}(4) + {name}(1)
"#
    )
}

#[test]
fn a_user_function_named_like_an_auto_registered_native_always_wins() {
    for name in STDLIB_NAMES {
        assert_eq!(
            int_at_every_level(&shadowing_program(name)),
            104 * 10 + 1,
            "user function `{name}` was replaced by the native one"
        );
    }
}

#[test]
fn shadowing_a_native_does_not_clobber_its_qualified_name() {
    assert_eq!(
        int_at_every_level(
            r#"
fn sqrt(t: int) -> int { t + 100 }
fn min(t: int) -> int { t + 200 }
let qualified = math::sqrt(16.0)
let low = math::min(3, 9)
sqrt(4) * 1000 + min(1) + (qualified as int) * 10 + low
"#
        ),
        104_000 + 201 + 40 + 3
    );
}

#[test]
fn user_methods_named_like_string_and_collection_builtins_win() {
    assert_eq!(
        int_at_every_level(
            r#"
struct Dir { names: Vec<string> }
impl Dir {
    fn find(self, name: string) -> int { if self.names[0] == name { 7 } else { -1 } }
    fn len(self) -> int { 40 }
    fn trim(self) -> int { 300 }
    fn push(mut self, s: string) -> int { self.names.push(s); 2000 }
    fn get(self, i: int) -> int { 10000 * (i + 1) }
    fn contains(self, s: string) -> bool { s == "zz" }
    fn is_empty(self) -> int { 100000 }
    fn to_upper(self) -> int { 1000000 }
    fn pop(self) -> int { 10000000 }
}
let mut d = Dir { names: vec!["A"] }
let c = if d.contains("zz") { 100000000 } else { 0 }
d.find("A") + d.len() + d.trim() + d.push("B") + d.get(0) + d.is_empty() + d.to_upper() + d.pop() + c + d.names.len() * 1000000000
"#
        ),
        7 + 40 + 300 + 2000 + 10000 + 100000 + 1000000 + 10000000 + 100000000 + 2000000000
    );
}

#[test]
fn trait_methods_named_like_builtins_dispatch_through_generic_bounds_and_generic_structs() {
    assert_eq!(
        int_at_every_level(
            r#"
trait Sized2 { fn len(self) -> int; fn find(self, x: int) -> int; }
struct Q { a: int }
impl Sized2 for Q {
    fn len(self) -> int { 11 }
    fn find(self, x: int) -> int { x * 2 }
}
struct Wrap<T> { inner: T }
impl<T> Wrap<T> { fn len(self) -> int { 500 } }
fn generic_len<T: Sized2>(t: T) -> int { t.len() + t.find(50) }
generic_len(Q { a: 1 }) + Wrap { inner: 3 }.len() + "abc".len() * 1000 + vec![1, 2].len() * 10000
"#
        ),
        11 + 100 + 500 + 3000 + 20000
    );
}
