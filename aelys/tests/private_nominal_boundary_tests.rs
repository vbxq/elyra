use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use tempfile::TempDir;

use aelys_driver::run_file;

fn create_module_env() -> TempDir {
    tempfile::tempdir().expect("Failed to create temp dir")
}

fn write_file(dir: &TempDir, path: &str, content: &str) -> PathBuf {
    let file_path = dir.path().join(path);
    if let Some(parent) = file_path.parent() {
        fs::create_dir_all(parent).expect("Failed to create parent directories");
    }
    let mut file = File::create(&file_path).expect("Failed to create file");
    write!(file, "{}", content).expect("Failed to write file");
    file_path
}

fn run(main: &std::path::Path) -> Result<i64, String> {
    match run_file(main) {
        Ok(value) => Ok(value.as_int().unwrap_or_default()),
        Err(error) => Err(error.to_string()),
    }
}

fn head(message: &str) -> &str {
    message.split("\n  --> ").next().unwrap_or(message)
}

const ENUM_SUPPORT: &str = "enum HiddenE { A(int), B }\n\npub struct Counter { pub v: int }\n\npub trait Pub {\n    fn get(self) -> int\n}\n\nimpl Pub for Counter {\n    fn get(self) -> int {\n        match HiddenE::A(self.v) {\n            HiddenE::A(n) => n,\n            HiddenE::B => 0,\n        }\n    }\n}\n";

const TRAIT_SUPPORT: &str = "trait Hid {\n    fn render(self) -> int\n}\n\npub struct Counter { pub v: int }\n\nimpl Hid for Counter {\n    fn render(self) -> int {\n        self.v * 10\n    }\n}\n\npub trait Pub {\n    fn get(self) -> int\n}\n\nimpl Pub for Counter {\n    fn get(self) -> int {\n        self.render() + 1\n    }\n}\n";

const STRUCT_SUPPORT: &str = "struct Hidden { h: int }\n\npub struct Counter { pub value: int }\n\npub trait Pub {\n    fn get(self) -> int\n}\n\nimpl Pub for Counter {\n    fn get(self) -> int {\n        let g = Hidden { h: self.value }\n        g.h + 1\n    }\n}\n";

#[test]
fn a_private_enum_tuple_constructor_is_refused_under_every_import_form() {
    for import in ["needs Counter, Pub from sup", "needs sup"] {
        let dir = create_module_env();
        write_file(&dir, "sup.aelys", ENUM_SUPPORT);
        let main = write_file(
            &dir,
            "main.aelys",
            &format!(
                "{import}\nfn probe() -> int {{\n    match HiddenE::A(3) {{\n        HiddenE::A(n) => n,\n        HiddenE::B => 0,\n    }}\n}}\nprobe()\n"
            ),
        );
        let message = run(&main).expect_err("the importer never imported the enum");
        assert!(
            message.contains("E0403") && message.contains("'HiddenE'"),
            "the tuple path answers like the struct path: {message}"
        );
        assert!(
            !message.contains("needs HiddenE from sup"),
            "a repair that would itself be refused must not be offered: {message}"
        );
    }
}

#[test]
fn a_private_enum_unit_path_is_refused_without_a_false_repair() {
    let dir = create_module_env();
    write_file(&dir, "sup.aelys", ENUM_SUPPORT);
    let main = write_file(
        &dir,
        "main.aelys",
        "needs Counter, Pub from sup\nfn probe() -> int {\n    let x = HiddenE::B\n    match x {\n        HiddenE::A(n) => n,\n        HiddenE::B => 9,\n    }\n}\nprobe()\n",
    );
    let message = run(&main).expect_err("the unit path answers like the tuple path");
    assert!(
        message.contains("E0403") && message.contains("'HiddenE'"),
        "both spellings refuse with the same code: {message}"
    );
    assert!(
        !message.contains("needs HiddenE from sup"),
        "the help the tuple path lost must not survive here: {message}"
    );
}

#[test]
fn a_private_enum_still_answers_through_the_carried_body() {
    let dir = create_module_env();
    write_file(&dir, "sup.aelys", ENUM_SUPPORT);
    let main = write_file(
        &dir,
        "main.aelys",
        "needs Counter, Pub from sup\nfn probe() -> int {\n    Counter { v: 2 }.get()\n}\nprobe()\n",
    );
    assert_eq!(
        run(&main),
        Ok(2),
        "only the module's own enum answers the carried match"
    );
}

#[test]
fn a_private_enum_coexists_with_the_importer_s_own_declaration() {
    let dir = create_module_env();
    write_file(&dir, "sup.aelys", ENUM_SUPPORT);
    let main = write_file(
        &dir,
        "main.aelys",
        "needs Counter, Pub from sup\nenum HiddenE { A(int), B }\nfn probe() -> int {\n    let carried = Counter { v: 2 }.get()\n    let own = match HiddenE::A(3) {\n        HiddenE::A(n) => n,\n        HiddenE::B => 0,\n    }\n    carried * 100 + own\n}\nprobe()\n",
    );
    assert_eq!(
        run(&main),
        Ok(203),
        "2 is the carried body on the module's enum, 3 the importer's own"
    );
}

#[test]
fn a_private_token_gate_stays_closed() {
    let dir = create_module_env();
    write_file(
        &dir,
        "sup.aelys",
        "enum Token { Admin(int), Guest }\n\npub struct Gate { pub g: int }\n\npub trait Check {\n    fn check(self) -> int\n}\n\nimpl Check for Gate {\n    fn check(self) -> int {\n        let t = Token::Guest\n        grant(t)\n    }\n}\n\nfn grant(t: Token) -> int {\n    match t {\n        Token::Admin(n) => n * 1000,\n        Token::Guest => 1,\n    }\n}\n",
    );
    let main = write_file(
        &dir,
        "main.aelys",
        "needs Gate, Check from sup\nfn probe() -> int {\n    let a = Gate { g: 1 }.check()\n    let b = match Token::Admin(7) {\n        Token::Admin(n) => n * 1000,\n        Token::Guest => 1,\n    }\n    a + b\n}\nprobe()\n",
    );
    let message = run(&main).expect_err("the gate answers 1 while the forgery answers 7001");
    assert!(
        message.contains("E0403") && message.contains("'Token'"),
        "constructing the private token is refused: {message}"
    );

    let hidden = write_file(
        &dir,
        "hidden.aelys",
        "needs Gate, Check from sup\nfn probe() -> int {\n    grant(Token::Admin(9))\n}\nprobe()\n",
    );
    let message = run(&hidden).expect_err("the private function stays hidden");
    assert!(
        message.contains("E0301"),
        "only the enum path lost its check, and it is back: {message}"
    );
}

#[test]
fn a_private_trait_method_call_is_refused_like_an_absent_field() {
    for import in ["needs Counter, Pub from sup", "needs sup"] {
        let dir = create_module_env();
        write_file(&dir, "sup.aelys", TRAIT_SUPPORT);
        let main = write_file(
            &dir,
            "main.aelys",
            &format!(
                "{import}\nfn probe() -> int {{\n    let c = Counter {{ v: 2 }}\n    c.render()\n}}\nprobe()\n"
            ),
        );
        let message = run(&main).expect_err("the trait never crossed the boundary");
        assert!(
            message.contains("E0363") && message.contains("'render'"),
            "the refusal matches the baseline code: {message}"
        );
    }
}

#[test]
fn a_private_trait_qualified_call_is_refused() {
    let dir = create_module_env();
    write_file(&dir, "sup.aelys", TRAIT_SUPPORT);
    let main = write_file(
        &dir,
        "main.aelys",
        "needs Counter, Pub from sup\nfn probe() -> int {\n    Hid::render(Counter { v: 2 })\n}\nprobe()\n",
    );
    let message = run(&main).expect_err("naming the trait names what is not exported");
    assert!(
        message.contains("E0403") && message.contains("'Hid'"),
        "the qualified path refuses like the bound: {message}"
    );
}

#[test]
fn a_private_trait_bound_is_refused_without_a_false_repair() {
    let dir = create_module_env();
    write_file(&dir, "sup.aelys", TRAIT_SUPPORT);
    let main = write_file(
        &dir,
        "main.aelys",
        "needs Counter, Pub from sup\nfn only<T: Hid>(x: T) -> int {\n    x.render()\n}\nfn probe() -> int {\n    only(Counter { v: 2 })\n}\nprobe()\n",
    );
    let message = run(&main).expect_err("no `needs` line reaches the trait");
    assert!(
        message.contains("E0403") && message.contains("'Hid'"),
        "the bound refuses like the impl header: {message}"
    );
    assert!(
        !message.contains("needs Hid from sup"),
        "the repair names a line that would itself be refused: {message}"
    );
}

#[test]
fn a_private_trait_still_answers_through_the_carried_body() {
    let dir = create_module_env();
    write_file(&dir, "sup.aelys", TRAIT_SUPPORT);
    let main = write_file(
        &dir,
        "main.aelys",
        "needs Counter, Pub from sup\nfn probe() -> int {\n    Counter { v: 2 }.get()\n}\nprobe()\n",
    );
    assert_eq!(
        run(&main),
        Ok(21),
        "21 is the private render times 10 plus 1, reached from inside"
    );
}

#[test]
fn a_public_but_unimported_trait_keeps_its_import_repair() {
    let dir = create_module_env();
    write_file(
        &dir,
        "sup.aelys",
        &TRAIT_SUPPORT.replace("trait Hid", "pub trait Hid"),
    );
    let main = write_file(
        &dir,
        "main.aelys",
        "needs Counter, Pub from sup\nfn probe() -> int {\n    let c = Counter { v: 2 }\n    c.render()\n}\nprobe()\n",
    );
    let message = run(&main).expect_err("the public trait is merely unimported here");
    assert!(
        message.contains("E0363"),
        "withholding a public trait still answers E0363: {message}"
    );
}

#[test]
fn a_private_struct_coexists_with_the_importer_s_own_declaration() {
    let dir = create_module_env();
    write_file(&dir, "support.aelys", STRUCT_SUPPORT);
    let main = write_file(
        &dir,
        "main.aelys",
        "needs Counter, Pub from support\nstruct Hidden { q: string }\nfn probe() -> int {\n    let h = Hidden { q: \"own\" }\n    let c = Counter { value: 3 }\n    c.get() + h.q.len()\n}\nprobe()\n",
    );
    assert_eq!(
        run(&main),
        Ok(7),
        "4 is the carried body on the module's struct, 3 the importer's own string"
    );
}

#[test]
fn a_private_struct_coexists_even_when_the_body_goes_uncalled() {
    let dir = create_module_env();
    write_file(&dir, "support.aelys", STRUCT_SUPPORT);
    let main = write_file(
        &dir,
        "main.aelys",
        "needs Counter, Pub from support\nstruct Hidden { q: int }\nfn probe() -> int {\n    let h = Hidden { q: 2 }\n    h.q\n}\nprobe()\n",
    );
    assert_eq!(
        run(&main),
        Ok(2),
        "no call into the carried body is needed for the two declarations to stand apart"
    );
}

#[test]
fn a_private_struct_direct_naming_stays_refused() {
    for import in ["needs Counter, Pub from support", "needs support"] {
        let dir = create_module_env();
        write_file(&dir, "support.aelys", STRUCT_SUPPORT);
        let main = write_file(
            &dir,
            "main.aelys",
            &format!(
                "{import}\nfn probe() -> int {{\n    let h = Hidden {{ h: 1 }}\n    h.h\n}}\nprobe()\n"
            ),
        );
        let message = run(&main).expect_err("the struct never crossed the boundary");
        assert!(
            message.contains("E0403") && message.contains("'Hidden'"),
            "direct naming refuses under every import form: {message}"
        );
    }
}

#[test]
fn an_associated_type_reaching_a_private_nominal_keeps_its_collision() {
    let dir = create_module_env();
    write_file(
        &dir,
        "support.aelys",
        "struct Hidden { n: int }\n\npub struct Counter { pub c: int }\n\npub trait Source {\n    type Item\n    fn get(self) -> int\n}\n\nimpl Source for Counter {\n    type Item = Hidden\n    fn get(self) -> int {\n        return self.c\n    }\n}\n",
    );
    let main = write_file(
        &dir,
        "main.aelys",
        "needs Counter, Source from support\nstruct Hidden { pub other: int }\nfn probe() -> int {\n    return 1\n}\nprobe()\n",
    );
    let message = run(&main).expect_err("the associated item exposes the name");
    assert!(
        message.contains("E0410") && message.contains("'Hidden'"),
        "a name the public contract exposes still collides: {message}"
    );
}

#[test]
fn a_public_struct_against_a_private_namesake_runs() {
    let dir = create_module_env();
    write_file(&dir, "sup.aelys", STRUCT_SUPPORT);
    write_file(
        &dir,
        "pubmod.aelys",
        "pub struct Node { pub p: int }\npub fn mkn(x: int) -> Node {\n    Node { p: x }\n}\n",
    );
    let main = write_file(
        &dir,
        "main.aelys",
        "needs Counter, Pub from sup\nneeds Node, mkn from pubmod\nfn probe() -> int {\n    let n = mkn(4)\n    Counter { value: 3 }.get() + n.p\n}\nprobe()\n",
    );
    assert_eq!(
        run(&main),
        Ok(8),
        "4 is the carried body on the private struct, 4 the public struct's field"
    );
}

#[test]
fn two_private_namesakes_from_two_modules_run_side_by_side() {
    let dir = create_module_env();
    write_file(&dir, "support.aelys", STRUCT_SUPPORT);
    write_file(
        &dir,
        "other.aelys",
        "struct Hidden { z: int }\n\npub struct Box2 { pub v: int }\n\npub trait Second {\n    fn two(self) -> int\n}\n\nimpl Second for Box2 {\n    fn two(self) -> int {\n        let n = Hidden { z: self.v }\n        n.z * 2\n    }\n}\n",
    );
    let main = write_file(
        &dir,
        "main.aelys",
        "needs Counter, Pub from support\nneeds Box2, Second from other\nfn probe() -> int {\n    Counter { value: 3 }.get() + Box2 { v: 2 }.two()\n}\nprobe()\n",
    );
    assert_eq!(
        run(&main),
        Ok(8),
        "4 and 4 are the two modules' own structs answering apart"
    );
}

#[test]
fn a_whole_module_import_coexists_with_an_own_declaration() {
    let dir = create_module_env();
    write_file(&dir, "support.aelys", STRUCT_SUPPORT);
    let main = write_file(
        &dir,
        "main.aelys",
        "needs support\nstruct Hidden { q: int }\nfn probe() -> int {\n    let n = Hidden { q: 4 }\n    n.q\n}\nprobe()\n",
    );
    assert_eq!(
        run(&main),
        Ok(4),
        "a whole-module import brings no bare name into scope to collide with"
    );
}

#[test]
fn an_aliased_import_coexists_with_an_own_declaration() {
    let dir = create_module_env();
    write_file(
        &dir,
        "sup.aelys",
        "struct Hidden { h: int }\n\npub struct Counter { pub value: int }\n\npub fn mk(v: int) -> Counter {\n    Counter { value: v }\n}\n\npub trait Pub {\n    fn get(self) -> int\n}\n\nimpl Pub for Counter {\n    fn get(self) -> int {\n        let g = Hidden { h: self.value }\n        g.h + 1\n    }\n}\n",
    );
    let main = write_file(
        &dir,
        "main.aelys",
        "needs sup as s\nstruct Hidden { q: int }\nfn probe() -> int {\n    let h = Hidden { q: 5 }\n    s::mk(3).get() + h.q\n}\nprobe()\n",
    );
    assert_eq!(
        run(&main),
        Ok(9),
        "4 is the carried body through the alias, 5 the importer's own struct"
    );
}

#[test]
fn a_private_name_two_hops_down_coexists_with_an_own_declaration() {
    let dir = create_module_env();
    write_file(
        &dir,
        "deep.aelys",
        "struct Leaf { l: int }\n\npub struct Deep { pub d: int }\n\npub trait DeepT {\n    fn dg(self) -> int\n}\n\nimpl DeepT for Deep {\n    fn dg(self) -> int {\n        let x = Leaf { l: self.d }\n        x.l + 100\n    }\n}\n",
    );
    write_file(
        &dir,
        "mid.aelys",
        "needs Deep, DeepT from deep\n\npub struct Mid { pub m: int }\n\npub trait MidT {\n    fn mg(self) -> int\n}\n\nimpl MidT for Mid {\n    fn mg(self) -> int {\n        Deep { d: self.m }.dg() + 5\n    }\n}\n",
    );
    let main = write_file(
        &dir,
        "main.aelys",
        "needs Mid, MidT from mid\nstruct Leaf { z: int }\nfn probe() -> int {\n    let a = Mid { m: 1 }.mg()\n    let l = Leaf { z: 2 }\n    a + l.z\n}\nprobe()\n",
    );
    assert_eq!(
        run(&main),
        Ok(108),
        "106 is the two-level chain on the module's own leaf, 2 the importer's own"
    );
}

#[test]
fn a_body_only_private_generic_travels_with_the_body() {
    let dir = create_module_env();
    write_file(
        &dir,
        "sup.aelys",
        "struct Gen<T> { g: T }\n\npub struct P { pub v: int }\n\npub trait PT {\n    fn pg(self) -> int\n}\n\nimpl PT for P {\n    fn pg(self) -> int {\n        let a = Gen { g: self.v }\n        a.g + 1\n    }\n}\n",
    );
    let main = write_file(
        &dir,
        "main.aelys",
        "needs P, PT from sup\nfn probe() -> int {\n    P { v: 3 }.pg()\n}\nprobe()\n",
    );
    assert_eq!(
        run(&main),
        Ok(4),
        "the generic answers from inside the carried body, where it always did"
    );
}

#[test]
fn an_associated_type_reaching_a_private_generic_stays_refused() {
    for import in ["needs Counter, Source from support", "needs support"] {
        let dir = create_module_env();
        write_file(
            &dir,
            "support.aelys",
            "struct Hidden<T> { n: T }\n\npub struct Counter { pub c: int }\n\npub trait Source {\n    type Item\n    fn get(self) -> int\n}\n\nimpl Source for Counter {\n    type Item = Hidden<int>\n    fn get(self) -> int {\n        return self.c\n    }\n}\n",
        );
        let main = write_file(
            &dir,
            "main.aelys",
            &format!("{import}\nfn probe() -> int {{\n    return 1\n}}\nprobe()\n"),
        );
        let message = run(&main).expect_err("the associated item would expose the generic");
        assert!(
            message.contains("E0407") && message.contains("'Hidden'"),
            "the refusal names the type the impl wrote: {message}"
        );
        assert!(
            message.contains("support.aelys") && !message.contains("main.aelys"),
            "the span belongs to the defining file: {message}"
        );
    }
}

#[test]
fn private_generics_outside_carried_bodies_stay_importable() {
    let dir = create_module_env();
    write_file(
        &dir,
        "sup.aelys",
        "struct Gen<T> { g: T }\n\nfn helper(n: int) -> int {\n    let a = Gen { g: n }\n    a.g + 1\n}\n\npub fn api(n: int) -> int {\n    helper(n) + 1\n}\n",
    );
    write_file(
        &dir,
        "sup2.aelys",
        "struct Unused<T> { g: T }\n\npub fn api2(n: int) -> int {\n    n + 1\n}\n",
    );
    let main = write_file(
        &dir,
        "main.aelys",
        "needs api from sup\nneeds api2 from sup2\nfn probe() -> int {\n    api(3) + api2(3)\n}\nprobe()\n",
    );
    assert_eq!(
        run(&main),
        Ok(9),
        "5 is the private generic through the private function, 4 the untouched one"
    );
}

#[test]
fn private_functions_and_globals_still_travel_and_coexist() {
    let dir = create_module_env();
    write_file(
        &dir,
        "sup.aelys",
        "fn helper(n: int) -> int {\n    n * 10\n}\n\nlet gseed = 100\n\npub struct C { pub v: int }\n\npub trait T {\n    fn go(self) -> int\n}\n\nimpl T for C {\n    fn go(self) -> int {\n        helper(self.v) + gseed\n    }\n}\n",
    );
    let main = write_file(
        &dir,
        "main.aelys",
        "needs C, T from sup\nfn helper(n: int) -> int {\n    n + 1\n}\nlet gseed = 7\nfn probe() -> int {\n    C { v: 2 }.go() + helper(1) + gseed\n}\nprobe()\n",
    );
    assert_eq!(
        run(&main),
        Ok(129),
        "20 and 100 are the module's own, 2 and 7 the importer's own"
    );
}

#[test]
fn refusals_are_byte_identical_under_swapped_order_and_repetition() {
    let dir = create_module_env();
    write_file(&dir, "support.aelys", STRUCT_SUPPORT);
    write_file(
        &dir,
        "other.aelys",
        "struct Hidden { z: int }\n\npub struct Box2 { pub v: int }\n\npub trait Second {\n    fn two(self) -> int\n}\n\nimpl Second for Box2 {\n    fn two(self) -> int {\n        let n = Hidden { z: self.v }\n        n.z * 2\n    }\n}\n",
    );
    let first = write_file(
        &dir,
        "first.aelys",
        "needs Counter, Pub from support\nneeds Box2, Second from other\nfn probe() -> int {\n    Counter { value: 3 }.get() + Box2 { v: 2 }.two()\n}\nprobe()\n",
    );
    let second = write_file(
        &dir,
        "second.aelys",
        "needs Box2, Second from other\nneeds Counter, Pub from support\nfn probe() -> int {\n    Counter { value: 3 }.get() + Box2 { v: 2 }.two()\n}\nprobe()\n",
    );
    let mut seen = std::collections::HashSet::new();
    for _ in 0..20 {
        match run(&first) {
            Ok(value) => {
                seen.insert(value);
            }
            Err(message) => panic!("the pair must run, not refuse: {message}"),
        }
    }
    assert_eq!(seen.len(), 1, "twenty compilations render one answer");
    assert_eq!(
        run(&first),
        Ok(8),
        "4 and 4 are the two modules' own structs answering apart"
    );
    assert_eq!(
        head(
            &run(&second)
                .map(|value| value.to_string())
                .unwrap_or_default()
        ),
        head(
            &run(&first)
                .map(|value| value.to_string())
                .unwrap_or_default()
        ),
        "the order of the two `needs` lines cannot decide the answer"
    );

    let dir = create_module_env();
    write_file(&dir, "sup.aelys", ENUM_SUPPORT);
    let refused = write_file(
        &dir,
        "main.aelys",
        "needs Counter, Pub from sup\nfn probe() -> int {\n    match HiddenE::A(3) {\n        HiddenE::A(n) => n,\n        HiddenE::B => 0,\n    }\n}\nprobe()\n",
    );
    let first_message = run(&refused).expect_err("the enum stays refused");
    for _ in 0..20 {
        assert_eq!(
            run(&refused).expect_err("the enum stays refused"),
            first_message,
            "twenty compilations render one diagnostic"
        );
    }
}

const MATRIX_SUPPORT: &str = "struct PrivS { h: int }\nenum PrivE { A(int), B }\ntrait PrivT {\n    fn pt(self) -> int\n}\nfn privfn(n: int) -> int {\n    n * 10\n}\nlet privg = 100\npub struct PubS { pub v: int }\npub enum PubE { A(int), B }\npub trait PubT {\n    fn qt(self) -> int\n}\nimpl PubT for PubS {\n    fn qt(self) -> int {\n        self.v + 1\n    }\n}\npub fn pubfn(n: int) -> int {\n    n + 2\n}\npub let pubg = 7\npub struct Carrier { pub v: int }\npub fn mkc(v: int) -> Carrier {\n    Carrier { v: v }\n}\npub trait Carry {\n    fn go(self) -> int\n}\nimpl PrivT for Carrier {\n    fn pt(self) -> int {\n        let s = PrivS { h: self.v }\n        let e = match PrivE::A(s.h) {\n            PrivE::A(n) => n,\n            PrivE::B => 0,\n        }\n        e + privfn(1) + privg\n    }\n}\nimpl Carry for Carrier {\n    fn go(self) -> int {\n        self.pt() + 1000\n    }\n}\n";

#[test]
fn each_kind_answers_its_own_code_in_each_importer_cell() {
    let dir = create_module_env();
    write_file(&dir, "sup.aelys", MATRIX_SUPPORT);
    let cells: &[(&str, &str, &str)] = &[
        (
            "needs Carrier, Carry from sup",
            "let s = PrivS { h: 1 }\ns.h\n",
            "E0403",
        ),
        (
            "needs Carrier, Carry from sup",
            "match PrivE::A(1) {\n PrivE::A(n) => n,\n PrivE::B => 0,\n}\n",
            "E0403",
        ),
        (
            "needs Carrier, Carry from sup",
            "let x = PrivE::B\nmatch x {\n PrivE::A(n) => n,\n PrivE::B => 9,\n}\n",
            "E0403",
        ),
        (
            "needs Carrier, Carry from sup",
            "let c = Carrier { v: 1 }\nc.pt()\n",
            "E0363",
        ),
        (
            "needs Carrier, Carry from sup",
            "PrivT::pt(Carrier { v: 1 })\n",
            "E0403",
        ),
        ("needs Carrier, Carry from sup", "privfn(1)\n", "E0301"),
        ("needs Carrier, Carry from sup", "privg\n", "E0301"),
        ("needs sup", "let s = PrivS { h: 1 }\ns.h\n", "E0403"),
        (
            "needs sup",
            "match PrivE::A(1) {\n PrivE::A(n) => n,\n PrivE::B => 0,\n}\n",
            "E0403",
        ),
        ("needs sup", "let c = Carrier { v: 1 }\nc.pt()\n", "E0363"),
        ("needs sup as sp", "let x = PrivS { h: 1 }\nx.h\n", "E0403"),
        (
            "needs Carrier, Carry from sup\nstruct PrivS { q: int }",
            "let s = PrivS { q: 1 }\ns.q\n",
            "run:1",
        ),
        (
            "needs Carrier, Carry from sup\nenum PrivE { A(int), B }",
            "match PrivE::A(1) {\n PrivE::A(n) => n,\n PrivE::B => 0,\n}\n",
            "run:1",
        ),
        (
            "needs Carrier, Carry from sup\ntrait PrivT {\n fn pt(self) -> int\n}",
            "let c = Carrier { v: 1 }\nc.pt()\n",
            "E0410",
        ),
        (
            "needs Carrier, Carry from sup\nfn privfn(n: int) -> int {\n n + 1\n}",
            "privfn(1)\n",
            "run:2",
        ),
        (
            "needs Carrier, Carry from sup\nlet privg = 7",
            "privg\n",
            "run:7",
        ),
        (
            "needs sup\nstruct PrivS { q: int }",
            "let s = PrivS { q: 1 }\ns.q\n",
            "run:1",
        ),
        (
            "needs sup\ntrait PrivT {\n fn pt(self) -> int\n}",
            "let c = Carrier { v: 1 }\nc.pt()\n",
            "E0410",
        ),
        (
            "needs sup as sp\nstruct PrivS { q: int }",
            "let c = sp::mkc(2).go()\nlet x = PrivS { q: 1 }\nc + x.q\n",
            "run:1113",
        ),
        (
            "needs Carrier, Carry from sup\nstruct PrivS { q: int }",
            "let c = Carrier { v: 2 }.go()\nlet s = PrivS { q: 1 }\nc + s.q\n",
            "run:1113",
        ),
        (
            "needs Carrier, Carry from sup",
            "Carrier { v: 2 }.go()\n",
            "run:1112",
        ),
        ("needs sup", "Carrier { v: 2 }.go()\n", "run:1112"),
        ("needs sup as sp", "sp::mkc(2).go()\n", "run:1112"),
    ];
    for (index, (import, body, expected)) in cells.iter().enumerate() {
        let main = write_file(
            &dir,
            &format!("cell{index}.aelys"),
            &format!("{import}\nfn probe() -> int {{\n{body}}}\nprobe()\n"),
        );
        let outcome = match run(&main) {
            Ok(value) => format!("run:{value}"),
            Err(message) => {
                let code = ["E0403", "E0363", "E0410", "E0301"]
                    .into_iter()
                    .find(|code| message.contains(code))
                    .unwrap_or("?");
                code.to_string()
            }
        };
        assert_eq!(
            outcome, *expected,
            "cell {index} (`{import}` with `{body}`) answers `{expected}`"
        );
    }
}

#[test]
fn an_associated_type_reaching_a_private_enum_stays_unconstructible() {
    let dir = create_module_env();
    write_file(
        &dir,
        "sup.aelys",
        "enum HiddenE { A(int), B }\n\npub struct Counter { pub c: int }\n\npub trait Source {\n    type Item\n    fn get(self) -> int\n}\n\nimpl Source for Counter {\n    type Item = HiddenE\n    fn get(self) -> int {\n        return self.c\n    }\n}\n",
    );
    let main = write_file(
        &dir,
        "main.aelys",
        "needs Counter, Source from sup\nfn probe() -> int {\n    match HiddenE::A(3) {\n        HiddenE::A(n) => n,\n        HiddenE::B => 0,\n    }\n}\nprobe()\n",
    );
    let message = run(&main).expect_err("the associated item exposes the name");
    assert!(
        message.contains("E0403") && message.contains("'HiddenE'"),
        "constructing the exposed private enum is refused: {message}"
    );
}

#[test]
fn a_private_trait_method_is_absent_on_a_receiver_known_at_its_instance() {
    let dir = create_module_env();
    write_file(&dir, "sup.aelys", TRAIT_SUPPORT);
    let main = write_file(
        &dir,
        "main.aelys",
        "needs Counter, Pub from sup\nfn id<Z>(z: Z) -> Z { return z }\nfn probe() -> int {\n    let c = id(Counter { v: 2 })\n    c.render()\n}\nprobe()\n",
    );
    let message = run(&main).expect_err("the trait never crossed the boundary");
    assert!(
        message.contains("E0363") && message.contains("'render'") && !message.contains('@'),
        "a receiver known only at its instance does not reveal the trait: {message}"
    );
}
