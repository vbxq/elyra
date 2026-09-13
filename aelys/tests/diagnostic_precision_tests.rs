use aelys::{CompileOptions, Runtime};

fn rendered_error(source: &str) -> String {
    match Runtime::new().compile(source, CompileOptions::default()) {
        Ok(_) => panic!("the source compiled, so it raises no diagnostic to inspect:\n{source}"),
        Err(error) => error.to_string(),
    }
}

fn locator(rendered: &str) -> (usize, usize) {
    let arrow = rendered
        .find("--> ")
        .unwrap_or_else(|| panic!("no `--> ` locator in:\n{rendered}"));
    let line = rendered[arrow + "--> ".len()..]
        .lines()
        .next()
        .unwrap_or_default();
    let mut parts = line.rsplitn(3, ':');
    let column = parts.next().and_then(|c| c.parse().ok());
    let row = parts.next().and_then(|r| r.parse().ok());
    match (row, column) {
        (Some(row), Some(column)) => (row, column),
        _ => panic!("unparsable locator `{line}` in:\n{rendered}"),
    }
}

fn underlined_line(source: &str, rendered: &str) -> String {
    let (row, _) = locator(rendered);
    source
        .lines()
        .nth(row - 1)
        .unwrap_or_else(|| panic!("the locator names line {row}, absent from:\n{source}"))
        .to_string()
}

const OVERFLOWING_LITERAL: &str = "\
fn probe() -> int {
    let a = 9223372036854775807
    a + 1
}
";

#[test]
fn e0209_underlines_the_literal_it_names() {
    let rendered = rendered_error(OVERFLOWING_LITERAL);
    assert!(
        rendered.contains("error[E0209]"),
        "expected E0209, got:\n{rendered}"
    );
    assert!(
        rendered.contains("integer literal '9223372036854775807'"),
        "expected the message to name the literal, got:\n{rendered}"
    );
    let underlined = underlined_line(OVERFLOWING_LITERAL, &rendered);
    assert!(
        underlined.contains("9223372036854775807"),
        "E0209 names `9223372036854775807` but underlines `{}`:\n{rendered}",
        underlined.trim()
    );
}

// the binding is never read, so dead code elimination drops it before the backend sees the literal
const DEAD_OVERFLOWING_LITERAL: &str = "\
fn probe() -> int {
    let a = 9223372036854775807
    7
}
probe()
";

#[test]
fn an_out_of_range_literal_is_refused_whatever_the_optimizer_does_with_it() {
    let rendered = rendered_error(DEAD_OVERFLOWING_LITERAL);
    assert!(
        rendered.contains("error[E0209]")
            && rendered.contains("integer literal '9223372036854775807'"),
        "an optimizer that removes the binding cannot make the program valid:\n{rendered}"
    );
    let underlined = underlined_line(DEAD_OVERFLOWING_LITERAL, &rendered);
    assert!(
        underlined.contains("9223372036854775807"),
        "E0209 must underline the literal, not `{}`:\n{rendered}",
        underlined.trim()
    );
}

#[test]
fn the_smallest_representable_integer_is_still_writable() {
    let source = "\
fn probe() -> int {
    let a = -140737488355328
    return a
}
probe()
";
    Runtime::new()
        .compile(source, CompileOptions::default())
        .expect("the range check reads the literal after the parser folds the leading '-' into it");
}

fn prescribed_spelling(rendered: &str) -> String {
    let open = rendered
        .find("written '")
        .unwrap_or_else(|| panic!("no prescribed spelling in:\n{rendered}"));
    let tail = &rendered[open + "written '".len()..];
    let close = tail
        .find('\'')
        .unwrap_or_else(|| panic!("unterminated prescribed spelling in:\n{rendered}"));
    tail[..close].to_string()
}

const MISSING_ASSOCIATED_CONST: &str = "\
trait Cap<T> {
    const LIMIT: T
    fn get(self) -> int;
}
struct Box { v: int }
impl Cap<int> for Box {
    fn get(self) -> int { 1 }
}
";

#[test]
fn e0421_prescribes_the_impl_instantiation_and_not_the_trait_parameter() {
    let rendered = rendered_error(MISSING_ASSOCIATED_CONST);
    assert!(
        rendered.contains("error[E0421]"),
        "expected E0421, got:\n{rendered}"
    );
    assert_eq!(
        prescribed_spelling(&rendered),
        "const LIMIT: int = <value>",
        "E0421 must spell the type the impl instantiates, not the trait's parameter:\n{rendered}"
    );
}

#[test]
fn the_spelling_e0421_prescribes_compiles() {
    let rendered = rendered_error(MISSING_ASSOCIATED_CONST);
    let remedy = prescribed_spelling(&rendered).replace("<value>", "0");
    let repaired = MISSING_ASSOCIATED_CONST.replace(
        "    fn get(self) -> int { 1 }",
        &format!("    {remedy}\n    fn get(self) -> int {{ 1 }}"),
    );
    if let Err(error) = Runtime::new().compile(&repaired, CompileOptions::default()) {
        panic!("E0421 prescribed `{remedy}`, which the compiler refuses:\n{repaired}\n{error}");
    }
}

const ONE_TRAIT_TWO_INSTANTIATIONS: &str = "\
trait Src<A> { fn next(self) -> int; }
struct Counter { v: int }
impl Src<int> for Counter { fn next(self) -> int { 1 } }
impl Src<bool> for Counter { fn next(self) -> int { 2 } }
let c = Counter { v: 0 }
println(c.next())
";

const TWO_TRAITS: &str = "\
trait Src { fn next(self) -> int; }
trait Alt { fn next(self) -> int; }
struct Counter { v: int }
impl Src for Counter { fn next(self) -> int { 1 } }
impl Alt for Counter { fn next(self) -> int { 2 } }
let c = Counter { v: 0 }
println(c.next())
";

#[test]
fn one_trait_implemented_twice_is_not_reported_as_two_traits() {
    let rendered = rendered_error(ONE_TRAIT_TWO_INSTANTIATIONS);
    assert!(
        !rendered.contains("more than one trait"),
        "one trait supplies the method, so the diagnostic must not claim several:\n{rendered}"
    );
    assert!(
        rendered.contains("error[E0437]"),
        "expected E0437 for a trait implemented at two instantiations, got:\n{rendered}"
    );
    for expected in ["'Src'", "Src<int>", "Src<bool>"] {
        assert!(
            rendered.contains(expected),
            "the diagnostic must name {expected}:\n{rendered}"
        );
    }
}

#[test]
fn the_one_trait_ambiguity_does_not_prescribe_a_qualified_call() {
    let rendered = rendered_error(ONE_TRAIT_TWO_INSTANTIATIONS);
    assert!(
        !rendered.contains("qualified call"),
        "a qualified call cannot select one of two instantiations, so it must not be \
         prescribed:\n{rendered}"
    );
    let qualified = ONE_TRAIT_TWO_INSTANTIATIONS.replace("c.next()", "Src::next(c)");
    assert!(
        Runtime::new()
            .compile(&qualified, CompileOptions::default())
            .is_err(),
        "this test stands on `Src::next(c)` still being refused; it now compiles, so the \
         prescription may return"
    );
}

#[test]
fn two_traits_keep_e0337_and_its_qualified_call_remedy() {
    let rendered = rendered_error(TWO_TRAITS);
    assert!(
        rendered.contains("error[E0337]") && rendered.contains("more than one trait"),
        "two traits supplying one method stay E0337:\n{rendered}"
    );
    let qualified = TWO_TRAITS.replace("c.next()", "Src::next(c)");
    if let Err(error) = Runtime::new().compile(&qualified, CompileOptions::default()) {
        panic!("E0337 prescribes a qualified call, which must compile:\n{error}");
    }
}

fn write_module(dir: &tempfile::TempDir, name: &str, content: &str) -> std::path::PathBuf {
    let path = dir.path().join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("the module directory must be creatable");
    }
    std::fs::write(&path, content).expect("the module file must be writable");
    path
}

#[test]
fn e0326_across_modules_names_the_two_modules() {
    let dir = tempfile::tempdir().expect("a temporary module tree");
    write_module(&dir, "beta.aelys", "pub struct Shared { b: int }\n");
    write_module(
        &dir,
        "gamma.aelys",
        "needs Shared from beta\npub struct Shared { a: int }\npub fn gamma_use() -> int { 1 }\n",
    );
    let entry = write_module(
        &dir,
        "main.aelys",
        "needs gamma_use from gamma\nfn main() -> int { gamma_use() }\nmain()\n",
    );
    let rendered = aelys_driver::run_file(&entry)
        .expect_err("two modules declaring 'Shared' must be refused")
        .to_string();
    assert!(
        rendered.contains("error[E0326]"),
        "expected E0326, got:\n{rendered}"
    );
    for module in ["'gamma'", "'beta'"] {
        assert!(
            rendered.contains(module),
            "the module to module clash must name {module}, as E0410 names both at the entry \
             file:\n{rendered}"
        );
    }
}

const PRIVATE_GENERIC_LIB: &str = "\
struct Hidden<T> { v: T }
pub struct Holder { pub n: int }
pub trait Carry {
    type Item
    fn carry(self) -> int
}
impl Carry for Holder {
    type Item = Hidden<int>
    fn carry(self) -> int { self.n }
}
pub fn make() -> Holder { Holder { n: 3 } }
";

#[test]
fn e0407_underlines_the_binding_that_exposes_the_private_generic() {
    let dir = tempfile::tempdir().expect("a temporary module tree");
    write_module(&dir, "lib.aelys", PRIVATE_GENERIC_LIB);
    let entry = write_module(
        &dir,
        "main.aelys",
        "needs Holder, Carry, make from lib\nfn main() -> int {\n    let h = make()\n    \
         return h.carry()\n}\nmain()\n",
    );
    let rendered = aelys_driver::run_file(&entry)
        .expect_err("a private generic reaching the import contract must be refused")
        .to_string();
    assert!(
        rendered.contains("error[E0407]"),
        "expected E0407, got:\n{rendered}"
    );
    let underlined = underlined_line(PRIVATE_GENERIC_LIB, &rendered);
    assert!(
        underlined.contains("type Item = Hidden<int>"),
        "E0407 must underline the binding that exposes 'Hidden', not its declaration; it \
         underlines `{}`:\n{rendered}",
        underlined.trim()
    );
}

struct Surface {
    source: &'static str,
    code: &'static str,
    written: &'static str,
    internal: &'static str,
}

const SURFACE_SPELLINGS: &[Surface] = &[
    Surface {
        source: "fn probe() -> string {\n    return 1\n}\nprobe()\n",
        code: "E0301",
        written: "found int",
        internal: "i64",
    },
    Surface {
        source: "trait Mark { fn mark(self) -> int; }\nstruct S { v: int }\n\
                 impl Mark for S { fn mark(self) -> int { 1 } }\nprintln(Mark::mark(5))\n",
        code: "E0329",
        written: "int",
        internal: "i64",
    },
    Surface {
        source: "trait Scorable { fn score(self) -> int; }\n\
                 fn total<T: Scorable>(v: T) -> int { v.score() }\nprintln(total(5))\n",
        code: "E0338",
        written: "is not implemented for int",
        internal: "i64",
    },
    Surface {
        source: "struct Wrapper<T> { w: T }\ntrait Echo<T> { fn echo(self, value: T) -> T; }\n\
                 impl<T> Echo<int> for Wrapper<T> { fn echo(self, value: int) -> int { 2 } }\n\
                 impl Echo<int> for Wrapper<int> { fn echo(self, value: int) -> int { 1 } }\n\
                 fn probe() -> int {\n    return 1\n}\nprobe()\n",
        code: "E0340",
        written: "for Wrapper<int>",
        internal: "i64",
    },
    Surface {
        source: "let v = vec![1, 2]\nprintln(v.nope())\n",
        code: "E0371",
        written: "Vec<int>",
        internal: "vec[",
    },
];

#[test]
fn diagnostics_write_the_spelling_a_program_could_have_written() {
    let mut wrong = Vec::new();
    for case in SURFACE_SPELLINGS {
        let rendered = rendered_error(case.source);
        if !rendered.contains(case.code) {
            wrong.push(format!("expected {}, got:\n{rendered}", case.code));
            continue;
        }
        if rendered.contains(case.internal) {
            wrong.push(format!(
                "{} still writes the internal `{}`:\n{rendered}",
                case.code, case.internal
            ));
        }
        if !rendered.contains(case.written) {
            wrong.push(format!(
                "{} must write `{}`:\n{rendered}",
                case.code, case.written
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn e0329_does_not_call_a_builtin_type_a_struct() {
    let rendered = rendered_error(
        "trait Mark { fn mark(self) -> int; }\nstruct S { v: int }\n\
         impl Mark for S { fn mark(self) -> int { 1 } }\nprintln(Mark::mark(5))\n",
    );
    assert!(
        rendered.contains("E0329"),
        "expected E0329, got:\n{rendered}"
    );
    assert!(
        !rendered.contains("struct int") && !rendered.contains("struct i64"),
        "'int' is a built-in, not a struct:\n{rendered}"
    );
}

#[test]
fn the_spec_writes_its_own_rule_in_every_diagnostic_it_quotes() {
    let spec = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("the aelys crate sits one level below the workspace root")
            .join("docs/language-spec.md"),
    )
    .expect("the specification must be readable");
    let offenders: Vec<String> = spec
        .lines()
        .enumerate()
        .filter(|(_, line)| {
            let quoted = line.contains("E0") || line.contains("error[");
            quoted && (line.contains(" i64") || line.contains(" f64") || line.contains("vec["))
        })
        .map(|(index, line)| format!("line {}: {}", index + 1, line.trim()))
        .collect();
    assert!(
        offenders.is_empty(),
        "the specification writes an internal spelling in a diagnostic it quotes:\n{}",
        offenders.join("\n")
    );
}

const QUALIFIED_TRAIT_CALL_MISMATCH: &str = "\
trait Mark { fn mark(self, n: int) -> int; }
struct S { v: int }
impl Mark for S { fn mark(self, n: int) -> int { n } }
println(Mark::mark(S { v: 1 }, \"x\"))
";

#[test]
fn a_diagnostic_reason_never_carries_a_mangled_symbol() {
    let rendered = rendered_error(QUALIFIED_TRAIT_CALL_MISMATCH);
    assert!(
        !rendered.contains("__aelys"),
        "a generated symbol reached the reader:\n{rendered}"
    );
    assert!(
        rendered.contains("argument 2 to function 'Mark::mark'"),
        "the reason names the call as it was written:\n{rendered}"
    );
}
