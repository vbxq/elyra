mod common;
use common::*;

fn assert_code(source: &str, code: &str) {
    let err = run_aelys_err(source);
    assert!(err.contains(code), "expected {code}, got: {err}");
    assert_located(code, &err);
}

const STRUCT: &str = "struct P { k: int }\n";

#[test]
fn generic_sort_of_unorderable_elements_is_rejected_at_the_instantiation() {
    for (element, value) in [
        ("P", "P { k: 3 }"),
        ("Vec<int>", "vec![3]"),
        ("Map<int, int>", "Map::new()"),
        ("Option<int>", "Some(3)"),
    ] {
        let forms = [
            format!(
                "fn sort_it<T>(mut v: Vec<T>) {{ v.sort() }}\nlet mut ps: Vec<{element}> = vec![{value}]\nsort_it(ps)\n0"
            ),
            format!(
                "fn sort_it<T>(mut a: [T; 2]) {{ a.sort() }}\nlet mut ps: [{element}; 2] = [{value}, {value}]\nsort_it(ps)\n0"
            ),
            format!(
                "fn inner<T>(mut v: Vec<T>) {{ v.sort() }}\nfn outer<U>(v: Vec<U>) {{ inner(v) }}\nlet ps: Vec<{element}> = vec![{value}]\nouter(ps)\n0"
            ),
            format!(
                "fn sort_it<T>(mut v: Vec<T>) {{\n    let f = fn() {{\n        v.sort()\n    }}\n    f()\n}}\nlet mut ps: Vec<{element}> = vec![{value}]\nsort_it(ps)\n0"
            ),
            format!(
                "struct Sorter {{ n: int }}\nimpl Sorter {{\n    fn order<T>(self, mut v: Vec<T>) {{ v.sort() }}\n}}\nlet mut ps: Vec<{element}> = vec![{value}]\nSorter {{ n: 0 }}.order(ps)\n0"
            ),
        ];
        for form in forms {
            assert_code(&format!("{STRUCT}{form}"), "E0471");
        }
    }
}

#[test]
fn generic_sort_still_sorts_orderable_elements() {
    assert_aelys_str(
        r#"
fn sort_it<T>(mut v: Vec<T>) { v.sort() }
fn sort_arr<T>(mut a: [T; 3]) { a.sort() }
let mut ints = vec![3, 1, 2]
sort_it(ints)
let mut words = vec!["b", "c", "a"]
sort_it(words)
let mut cs = vec!['z', 'a']
sort_it(cs)
let mut fs = [2.5, -1.0, 0.5]
sort_arr(fs)
"{ints[0]}{ints[1]}{ints[2]} {words[0]}{words[1]}{words[2]} {cs[0]}{cs[1]} {fs[0]} {fs[2]}"
"#,
        "123 abc az -1.0 2.5",
    );
}

#[test]
fn generic_fill_and_resize_of_aggregates_copy_into_every_slot() {
    for (element, value, change, read) in [
        ("P", "P { k: 0 }", "ps[i].k = 9", "ps[j].k"),
        ("Vec<int>", "vec![0]", "ps[i].push(9)", "ps[j].len()"),
        ("Map<int, int>", "Map::new()", "ps[i][4] = 9", "ps[j].len()"),
        ("[int; 2]", "[0, 0]", "ps[i][1] = 9", "ps[j][1]"),
    ] {
        let expected = if element == "Vec<int>" { "1" } else { "0" };
        let forms = [
            format!(
                "fn fill_it<T>(mut v: Vec<T>, x: T) {{ v.fill(x) }}\nlet mut ps: Vec<{element}> = vec![{value}, {value}]\nfill_it(ps, {value})\n"
            ),
            format!(
                "fn fill_it<T>(mut a: [T; 2], x: T) {{ a.fill(x) }}\nlet mut ps: [{element}; 2] = [{value}, {value}]\nfill_it(ps, {value})\n"
            ),
            format!(
                "fn grow<T>(mut v: Vec<T>, x: T) {{ v.resize(3, x) }}\nlet mut ps: Vec<{element}> = vec![]\ngrow(ps, {value})\n"
            ),
            format!(
                "struct Filler {{ n: int }}\nimpl Filler {{\n    fn reset<T>(self, mut v: Vec<T>, x: T) {{ v.fill(x) }}\n}}\nlet mut ps: Vec<{element}> = vec![{value}, {value}]\nFiller {{ n: 0 }}.reset(ps, {value})\n"
            ),
        ];
        for form in forms {
            assert_aelys_str(
                &format!("{STRUCT}{form}let i = 0\nlet j = 1\n{change}\n\"{{{read}}}\""),
                expected,
            );
        }
    }
}

#[test]
fn generic_fill_and_resize_still_work_on_scalars() {
    assert_aelys_str(
        r#"
fn fill_it<T>(mut v: Vec<T>, x: T) { v.fill(x) }
fn grow<T>(mut v: Vec<T>, x: T) { v.resize(4, x) }
let mut ints = vec![1, 2]
fill_it(ints, 7)
grow(ints, 9)
let mut words = vec!["a"]
grow(words, "z")
let mut out = ""
for x in ints { out += "{x}" }
for w in words { out += w }
out
"#,
        "7799azzz",
    );
}

#[test]
fn generic_map_keys_are_checked_at_the_instantiation() {
    for (key, value) in [
        ("float", "1.5"),
        ("P", "P { k: 1 }"),
        ("Vec<int>", "vec![1]"),
    ] {
        let forms = [
            format!(
                "fn mk<K, V>(k: K, v: V) -> Map<K, V> {{\n    let mut m: Map<K, V> = Map::new()\n    m[k] = v\n    m\n}}\nlet m = mk({value}, 2)\n0"
            ),
            format!(
                "fn mk<K>(k: K) -> int {{\n    let mut m: Map<K, int> = Map::new()\n    m[k] = 1\n    m.len()\n}}\nmk({value})\n"
            ),
            format!(
                "fn mk<K>(k: K) -> int {{\n    let mut m = Map::new()\n    let _ = m.insert(k, 1)\n    m.len()\n}}\nmk({value})\n"
            ),
            format!(
                "fn empty<K>(k: K) -> Map<K, int> {{ Map::new() }}\nlet e: Map<{key}, int> = empty({value})\n0"
            ),
            format!(
                "fn inner<K>(k: K) -> int {{\n    let mut m: Map<K, int> = Map::new()\n    m[k] = 1\n    m.len()\n}}\nfn outer<J>(j: J) -> int {{ inner(j) }}\nouter({value})\n"
            ),
        ];
        for form in forms {
            assert_code(&format!("{STRUCT}{form}"), "E0470");
        }
    }
}

#[test]
fn generic_maps_still_work_on_valid_keys() {
    assert_aelys_str(
        r#"
fn mk<K, V>(k: K, v: V) -> Map<K, V> {
    let mut m: Map<K, V> = Map::new()
    m[k] = v
    m
}
let a = mk("x", 1)
let b = mk(7, "seven")
let c = mk('q', true)
let d = mk(false, 2.5)
let x = a["x"]
"{x} {b[7]} {c['q']} {d[false]}"
"#,
        "1 seven true 2.5",
    );
}

#[test]
fn the_runtime_sort_traps_on_elements_without_an_order() {
    let text = ".version 3\n.function 0\n  .arity 0\n  .registers 4\n  .globals\n    0: \"__vec_sort\"\n  .schemas\n    0: \"t::P\" 0 0 1\n      0: \"k\" \"int:I64\"\n  .jit_unsupported_struct true\n  .code\n    0000: LoadI     r3, 2\n    0001: StructNew  0, r1, r3, 1, 0\n    0004: LoadI     r3, 1\n    0005: StructNew  0, r2, r3, 1, 0\n    0008: VecLit    r0, r1, 2\n    0009: Move      r2, r0\n    0010: CallGlobal r1, 0, 1\n    0011: Return    r1\n";
    let function = aelys_bytecode::asm::assemble(text)
        .expect("the assembly is well formed")
        .into_iter()
        .next()
        .expect("one function");
    let mut vm = aelys_runtime::VM::with_config(
        aelys_syntax::Source::new("sort.aasm", ""),
        aelys_runtime::VmConfig::default(),
    )
    .expect("a bare vm starts");
    let function = vm.alloc_function(function).expect("allocation succeeds");
    let err = vm
        .execute(function)
        .expect_err("two structs have no order, so the sort must not pretend they are equal");
    let message = err.to_string();
    assert!(
        message.contains("'sort'") && message.contains("built-in order"),
        "{message}"
    );
}
