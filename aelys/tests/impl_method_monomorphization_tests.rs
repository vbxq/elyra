use aelys::{CompileOptions, Runtime, run};
use aelys_bytecode::Function;
use aelys_bytecode::asm::deserialize;

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

fn run_message(source: &str) -> String {
    match run(source, "test.aelys") {
        Ok(value) => panic!("the program must be refused, it returned {value:?}:\n{source}"),
        Err(error) => error.to_string(),
    }
}

fn walk(function: &Function, visit: &mut dyn FnMut(&Function)) {
    visit(function);
    for nested in &function.nested_functions {
        walk(nested, visit);
    }
}

fn function_names(function: &Function) -> Vec<String> {
    let mut names = Vec::new();
    walk(function, &mut |function| {
        if let Some(name) = &function.name {
            names.push(name.clone());
        }
    });
    names.sort();
    names.dedup();
    names
}

fn schema_names(function: &Function) -> Vec<String> {
    let mut names = Vec::new();
    walk(function, &mut |function| {
        for schema in &function.struct_schemas {
            names.push(schema.ctor.display_name());
        }
        for schema in &function.enum_schemas {
            names.push(schema.def_id.display_name());
        }
    });
    names.sort();
    names.dedup();
    names
}

fn bodies_of(function: &Function, method: &str) -> Vec<String> {
    function_names(function)
        .into_iter()
        .filter(|name| name.starts_with("__aelys_struct::") || name.starts_with("__aelys_trait::"))
        .filter(|name| name.contains(method))
        .collect()
}

const PAIR_METHOD: &str = "\
struct Pair<A, B> { a: A, b: B }
struct Two { p: int, q: int }
struct G { n: int }

impl G {
    fn grow<T>(self, x: T, y: T) -> int {
        let p = Pair { a: x, b: y }
        self.n
    }
}

fn probe() -> int {
    let g = G { n: 7 }
    g.grow(1, 2)
}
probe()
";

#[test]
fn a_method_type_parameter_materializes_the_nominal_its_body_writes() {
    let function = compiled(PAIR_METHOD);
    let schemas = schema_names(&function);
    assert!(
        schemas.iter().any(|name| name.ends_with("::Pair")),
        "the Pair the method body writes must reach the schema table: {schemas:?}"
    );
    assert_eq!(
        run(PAIR_METHOD, "test.aelys")
            .expect("the program must run")
            .as_int(),
        Some(7)
    );
}

#[test]
fn a_method_type_parameter_gets_one_body_per_instantiation() {
    let source = "\
struct G { n: int }

impl G {
    fn id<T>(self, x: T) -> T {
        x
    }
}

fn probe() -> int {
    let g = G { n: 0 }
    let a = g.id(7)
    let b = g.id(true)
    if b { a } else { 0 }
}
probe()
";
    let function = compiled(source);
    let bodies = bodies_of(&function, "id");
    assert_eq!(
        bodies.len(),
        2,
        "two argument types must give two bodies: {bodies:?}"
    );
    assert_eq!(
        run(source, "test.aelys")
            .expect("the program must run")
            .as_int(),
        Some(7)
    );
}

#[test]
fn a_method_type_parameter_of_a_generic_impl_gets_one_body_per_instantiation() {
    let source = "\
struct Pair<A, B> { a: A, b: B }
struct G<U> { seed: U }

impl<U> G<U> {
    fn grow<T>(self, v: T) -> Pair<U, T> {
        Pair { a: self.seed, b: v }
    }
}

fn probe() -> int {
    let g = G { seed: 7 }
    let a = g.grow(1)
    let b = g.grow(true)
    if b.b { a.a * 10 + a.b + b.a } else { 0 }
}
probe()
";
    let function = compiled(source);
    let bodies = bodies_of(&function, "grow");
    assert_eq!(
        bodies.len(),
        2,
        "the impl half and the method half must both instantiate: {bodies:?}"
    );
    assert_eq!(
        run(source, "test.aelys")
            .expect("the program must run")
            .as_int(),
        Some(78)
    );
}

#[test]
fn a_generic_enum_built_by_a_generic_method_reaches_the_schema_table() {
    let source = "\
enum Holder<T> { Full(T), Empty }
struct G { n: int }

impl G {
    fn hold<T>(self, x: T) -> int {
        let h = Holder::Full(x)
        match h {
            Holder::Full(v) => self.n,
            Holder::Empty => 0,
        }
    }
}

fn probe() -> int {
    let g = G { n: 7 }
    g.hold(1)
}
probe()
";
    let function = compiled(source);
    let schemas = schema_names(&function);
    assert!(
        schemas.iter().any(|name| name.ends_with("::Holder")),
        "the enum the method body writes must reach the schema table: {schemas:?}"
    );
    assert_eq!(
        run(source, "test.aelys")
            .expect("the program must run")
            .as_int(),
        Some(7)
    );
}

#[test]
fn a_trait_default_body_carrying_its_own_type_parameter_is_specialized() {
    let source = "\
struct Pair<A, B> { a: A, b: B }

trait Grower {
    fn seed(self) -> int
    fn grow<T>(self, x: T) -> int {
        let p = Pair { a: x, b: x }
        self.seed()
    }
}

struct H { n: int }

impl Grower for H {
    fn seed(self) -> int {
        7
    }
}

fn probe() -> int {
    let h = H { n: 0 }
    h.grow(1) + h.grow(\"s\")
}
probe()
";
    let function = compiled(source);
    let schemas = schema_names(&function);
    assert!(
        schemas.iter().any(|name| name.ends_with("::Pair")),
        "the default body's nominal must reach the schema table: {schemas:?}"
    );
    let bodies = bodies_of(&function, "grow");
    assert_eq!(
        bodies.len(),
        2,
        "a default body adopted into an impl instantiates like any other: {bodies:?}"
    );
    assert_eq!(
        run(source, "test.aelys")
            .expect("the program must run")
            .as_int(),
        Some(14)
    );
}

#[test]
fn a_lambda_nested_in_a_generic_method_carries_the_substituted_type() {
    let source = "\
struct Pair<A, B> { a: A, b: B }
struct G { n: int }

impl G {
    fn grow<T>(self, x: T) -> int {
        let build = fn(v: T) -> Pair<T, T> { Pair { a: v, b: v } }
        let p = build(x)
        self.n
    }
}

fn probe() -> int {
    let g = G { n: 7 }
    g.grow(1)
}
probe()
";
    let function = compiled(source);
    let schemas = schema_names(&function);
    assert!(
        schemas.iter().any(|name| name.ends_with("::Pair")),
        "a nominal built inside a nested lambda must materialize too: {schemas:?}"
    );
    assert_eq!(
        run(source, "test.aelys")
            .expect("the program must run")
            .as_int(),
        Some(7)
    );
}

#[test]
fn an_unbound_method_type_parameter_is_the_free_function_diagnostic() {
    let prelude = "\
trait Source {
    type Item
    fn next(self) -> Self::Item;
}

struct Counter { value: int }

impl Source for Counter {
    type Item = int
    fn next(self) -> int {
        self.value
    }
}
";
    let free = format!(
        "{prelude}
fn drain<U: Source>(n: int) -> int {{
    n
}}

fn probe() -> int {{
    drain(5)
}}
probe()
"
    );
    let method = format!(
        "{prelude}
struct Tank {{ level: int }}

impl Tank {{
    fn drain<U: Source>(self, n: int) -> int {{
        n
    }}
}}

fn probe() -> int {{
    let t = Tank {{ level: 1 }}
    t.drain(5)
}}
probe()
"
    );
    let free_message = compile_message(&free);
    assert!(
        free_message.contains("E0343"),
        "the free twin must stay E0343: {free_message}"
    );
    let method_message = compile_message(&method);
    assert!(
        method_message.contains("E0343"),
        "a method type parameter nothing binds is the same refusal: {method_message}"
    );
}

#[test]
fn an_uncalled_generic_method_leaves_no_body_behind() {
    let source = "\
struct Pair<A, B> { a: A, b: B }
struct G { n: int }

impl G {
    fn grow<T>(self, x: T, y: T) -> int {
        let p = Pair { a: x, b: y }
        self.n
    }
}

fn probe() -> int {
    let g = G { n: 7 }
    g.n
}
probe()
";
    assert_eq!(
        run(source, "test.aelys")
            .expect("an uncalled generic method must not break the program")
            .as_int(),
        Some(7)
    );
    let bodies = bodies_of(&compiled(source), "grow");
    assert!(
        bodies.is_empty(),
        "an uncalled generic method is dropped like an uncalled generic function: {bodies:?}"
    );
}

#[test]
fn a_generic_method_called_on_seventy_types_stays_inside_the_free_function_budget() {
    let mut source = String::from(
        "struct Logger { id: int }\n\nimpl Logger {\n    fn log<T>(self, x: T) -> int {\n        1\n    }\n}\n\n",
    );
    for index in 0..70 {
        source.push_str(&format!("struct T{index} {{ v: int }}\n"));
    }
    source
        .push_str("\nfn probe() -> int {\n    let lg = Logger { id: 0 }\n    let mut total = 0\n");
    for index in 0..70 {
        source.push_str(&format!(
            "    total = total + lg.log(T{index} {{ v: {index} }})\n"
        ));
    }
    source.push_str("    total\n}\nprobe()\n");
    assert_eq!(
        run(&source, "test.aelys")
            .expect("seventy instances are below the free function budget")
            .as_int(),
        Some(70)
    );
    let bodies = bodies_of(&compiled(&source), "log");
    assert_eq!(
        bodies.len(),
        70,
        "seventy argument types are seventy bodies: {}",
        bodies.len()
    );
}

#[test]
fn a_method_type_parameter_that_grows_without_decreasing_is_a_diagnostic() {
    let source = "\
struct Pair<A, B> { a: A, b: B }
struct G { n: int }

impl G {
    fn grow<T>(x: T, depth: int) -> int {
        if depth <= 0 {
            0
        } else {
            let p = Pair { a: x, b: x }
            G::grow(p, depth - 1)
        }
    }
}

fn probe() -> int {
    G::grow(1, 5)
}
probe()
";
    let message = run_message(source);
    assert!(
        message.contains("E0344"),
        "an unbounded method type recursion must be a compile diagnostic: {message}"
    );
}

#[test]
fn an_impl_type_parameter_that_grows_without_decreasing_is_a_diagnostic() {
    let source = "\
struct Pair<A, B> { a: A, b: B }
struct G<T> { n: T }

impl<T> G<T> {
    fn grow(self, depth: int) -> int {
        if depth <= 0 {
            0
        } else {
            let p = Pair { a: self.n, b: self.n }
            let g = G { n: p }
            g.grow(depth - 1)
        }
    }
}

fn probe() -> int {
    let g = G { n: 1 }
    g.grow(5)
}
probe()
";
    let message = run_message(source);
    assert!(
        message.contains("E0344"),
        "an unbounded impl type recursion must be a compile diagnostic: {message}"
    );
}

#[test]
fn a_generic_method_of_an_imported_impl_is_specialized_in_the_importer() {
    let dir = tempfile::tempdir().expect("a temporary module tree");
    let module = "\
struct Pair<A, B> { a: A, b: B }

pub struct G { pub n: int }

impl G {
    fn grow<T>(self, x: T, y: T) -> int {
        let p = Pair { a: x, b: y }
        self.n
    }
}
";
    std::fs::write(dir.path().join("holder.aelys"), module).expect("the module is written");
    let main = "\
needs holder

fn probe() -> int {
    let g = G { n: 7 }
    g.grow(1, 2)
}
probe()
";
    let main_path = dir.path().join("main.aelys");
    std::fs::write(&main_path, main).expect("the entry is written");
    let value = aelys_driver::run_file(&main_path).expect("the imported generic method must run");
    assert_eq!(value.as_int(), Some(7));
}

#[test]
fn an_unrelated_generic_method_does_not_silence_a_concrete_call() {
    let stifled = "\
struct G { n: int }
struct H { n: int }
impl H {
    fn phantom<T>(self) -> int { return 5 }
}
impl G {
    fn genm<T>(self, v: T) -> int { return 1 }
    fn plain(self, h: H) -> int { return h.phantom() }
}
println(3)
";
    let message = run_message(stifled);
    assert!(
        message.contains("E0343"),
        "a concrete call in a template body keeps its diagnostic even when the \
         template is never instantiated: {message}"
    );

    let without_the_template = "\
struct G { n: int }
struct H { n: int }
impl H {
    fn phantom<T>(self) -> int { return 5 }
}
impl G {
    fn plain(self, h: H) -> int { return h.phantom() }
}
println(3)
";
    assert!(
        run_message(without_the_template).contains("E0343"),
        "the same program without the generic method is the reference"
    );
}

#[test]
fn a_call_on_an_open_type_stays_unreported_until_it_is_instantiated() {
    let source = "\
struct G<T> { n: T }
impl<T> G<T> {
    fn keep(self) -> T { return self.n }
    fn twice(self) -> T { return self.keep() }
}
println(4)
";
    let value = aelys::run(source, "test.aelys")
        .expect("a template nobody instantiates carries no diagnostic of its own");
    let _ = value;
}
