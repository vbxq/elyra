use aelys_opt::OptimizationLevel;

// a mangled method symbol is global, so two modules that both hide a nominal of the same
fn source_and_artifact(name: &str, files: &[(&str, &str)]) -> (String, String) {
    let dir = std::env::temp_dir().join(format!("aelys_cli_private_nominal_{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (relative, content) in files {
        let path = dir.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, content).unwrap();
    }

    let src_path = dir.join("main.aelys");
    let from_source = std::process::Command::new(env!("CARGO_BIN_EXE_aelys-cli"))
        .arg("run")
        .arg(&src_path)
        .output()
        .unwrap();
    assert_eq!(
        from_source.status.code(),
        Some(0),
        "{name} does not run from source: {}",
        String::from_utf8_lossy(&from_source.stderr)
    );

    let bytecode_path =
        aelys_cli::cli::commands::compile::compile_to_avbc(&src_path, OptimizationLevel::None)
            .unwrap_or_else(|err| panic!("{name} did not compile: {err}"));

    let from_artifact = std::process::Command::new(env!("CARGO_BIN_EXE_aelys-cli"))
        .arg("run")
        .arg(&bytecode_path)
        .output()
        .unwrap();
    assert_eq!(
        from_artifact.status.code(),
        Some(0),
        "{name} does not run from its artefact: {}",
        String::from_utf8_lossy(&from_artifact.stderr)
    );

    (
        String::from_utf8_lossy(&from_source.stdout).into_owned(),
        String::from_utf8_lossy(&from_artifact.stdout).into_owned(),
    )
}

fn assert_agrees(name: &str, files: &[(&str, &str)], expected: &str, why: &str) {
    let (source, artifact) = source_and_artifact(name, files);
    assert_eq!(
        source.lines().next(),
        Some(expected),
        "{name} from source: {why}"
    );
    assert_eq!(
        artifact.lines().next(),
        Some(expected),
        "{name} from its artefact: {why}"
    );
}

const SUM_MAIN: &str = "needs alpha\nneeds beta\nfn probe() -> int {\n    return alpha::take_alpha() + beta::take_beta()\n}\nprintln(probe())\n";

fn private_struct_module(taker: &str, answer: i64) -> String {
    format!(
        "struct S {{ v: int }}\n\nimpl S {{\n    fn get(self) -> int {{\n        return {answer}\n    }}\n}}\n\npub fn {taker}() -> int {{\n    let s = S {{ v: 0 }}\n    return s.get()\n}}\n"
    )
}

fn private_enum_module(taker: &str, answer: i64) -> String {
    format!(
        "enum S {{ A, B }}\n\nimpl S {{\n    fn get(self) -> int {{\n        return {answer}\n    }}\n}}\n\npub fn {taker}() -> int {{\n    let s = S::A\n    return s.get()\n}}\n"
    )
}

fn private_trait_module(taker: &str, answer: i64) -> String {
    format!(
        "needs Cell from core\n\ntrait Probe {{\n    fn probe(self) -> int\n}}\n\nimpl Probe for Cell {{\n    fn probe(self) -> int {{\n        return {answer}\n    }}\n}}\n\npub fn {taker}() -> int {{\n    let c = Cell {{ v: 0 }}\n    return c.probe()\n}}\n"
    )
}

#[test]
fn two_modules_hiding_one_struct_name_each_answer_their_own_method() {
    assert_agrees(
        "struct_pair",
        &[
            ("alpha.aelys", &private_struct_module("take_alpha", 5)),
            ("beta.aelys", &private_struct_module("take_beta", 7)),
            ("main.aelys", SUM_MAIN),
        ],
        "12",
        "12 pins alpha to 5 and beta to 7; one shared method symbol gives 14 or 10",
    );
}

#[test]
fn two_modules_hiding_one_enum_name_each_answer_their_own_method() {
    assert_agrees(
        "enum_pair",
        &[
            ("alpha.aelys", &private_enum_module("take_alpha", 5)),
            ("beta.aelys", &private_enum_module("take_beta", 7)),
            ("main.aelys", SUM_MAIN),
        ],
        "12",
        "12 pins alpha to 5 and beta to 7; one shared method symbol gives 14 or 10",
    );
}

#[test]
fn two_modules_hiding_one_trait_name_on_one_target_each_answer_their_own_method() {
    assert_agrees(
        "trait_pair",
        &[
            ("core.aelys", "pub struct Cell { pub v: int }\n"),
            ("alpha.aelys", &private_trait_module("take_alpha", 5)),
            ("beta.aelys", &private_trait_module("take_beta", 7)),
            ("main.aelys", SUM_MAIN),
        ],
        "12",
        "the two private traits share a bare name and a target, so one symbol carries both",
    );
}

#[test]
fn the_entry_file_s_own_hidden_struct_never_answers_for_its_dependency() {
    assert_agrees(
        "entry_against_module",
        &[
            ("lib.aelys", &private_struct_module("take", 5)),
            (
                "main.aelys",
                "needs lib\n\nstruct S { v: int }\n\nimpl S {\n    fn get(self) -> int {\n        return 100\n    }\n}\n\nfn probe() -> int {\n    let s = S { v: 0 }\n    return lib::take() + s.get()\n}\nprintln(probe())\n",
            ),
        ],
        "105",
        "105 keeps the module on 5 and the entry file on 100; one symbol gives 200 or 10",
    );
}

#[test]
fn a_third_module_never_moves_the_two_that_share_a_hidden_name() {
    assert_agrees(
        "three_modules",
        &[
            ("alpha.aelys", &private_struct_module("take_alpha", 5)),
            ("beta.aelys", &private_struct_module("take_beta", 7)),
            (
                "gamma.aelys",
                "struct R { v: int }\n\nimpl R {\n    fn get(self) -> int {\n        return 11\n    }\n}\n\npub fn take_gamma() -> int {\n    let r = R { v: 0 }\n    return r.get()\n}\n",
            ),
            (
                "main.aelys",
                "needs alpha\nneeds beta\nneeds gamma\nfn probe() -> int {\n    return alpha::take_alpha() + beta::take_beta() + gamma::take_gamma()\n}\nprintln(probe())\n",
            ),
        ],
        "23",
        "23 is 5 plus 7 plus 11; the untouched third module must not shift either answer",
    );
}

#[test]
fn one_imported_impl_reaching_two_importers_stays_one_symbol() {
    assert_agrees(
        "shared_impl",
        &[
            (
                "alpha.aelys",
                "pub struct Point { pub x: int }\npub trait Norm {\n    fn norm(self) -> int\n}\nimpl Norm for Point {\n    fn norm(self) -> int {\n        return self.x * 3\n    }\n}\n",
            ),
            (
                "beta.aelys",
                "needs Point, Norm from alpha\npub fn beta_norm(v: int) -> int {\n    let p = Point { x: v }\n    return p.norm()\n}\n",
            ),
            (
                "main.aelys",
                "needs Point, Norm from alpha\nneeds beta_norm from beta\nfn probe() -> int {\n    let p = Point { x: 2 }\n    return p.norm() * 100 + beta_norm(1)\n}\nprintln(probe())\n",
            ),
        ],
        "603",
        "603 pins both call sites to the one imported body, which must not be split in two",
    );
}

#[test]
fn two_modules_hiding_one_global_name_each_read_their_own() {
    assert_agrees(
        "global_pair",
        &[
            (
                "alpha.aelys",
                "let shared = 10\npub struct A1 { pub v: int }\npub trait TA {\n    fn ta(self) -> int\n}\nimpl TA for A1 {\n    fn ta(self) -> int {\n        return self.v + shared\n    }\n}\n",
            ),
            (
                "beta.aelys",
                "let shared = 200\npub struct B1 { pub v: int }\npub trait TB {\n    fn tb(self) -> int\n}\nimpl TB for B1 {\n    fn tb(self) -> int {\n        return self.v + shared\n    }\n}\n",
            ),
            (
                "main.aelys",
                "needs A1, TA from alpha\nneeds B1, TB from beta\nfn probe() -> int {\n    let a = A1 { v: 0 }\n    let b = B1 { v: 0 }\n    return a.ta() * 1000 + b.tb()\n}\nprintln(probe())\n",
            ),
        ],
        "10200",
        "the private global already scopes per module and must stay that way",
    );
}

#[test]
fn two_modules_hiding_one_function_name_each_call_their_own() {
    assert_agrees(
        "function_pair",
        &[
            (
                "alpha.aelys",
                "fn helper() -> int {\n    return 5\n}\n\npub fn take_alpha() -> int {\n    return helper()\n}\n",
            ),
            (
                "beta.aelys",
                "fn helper() -> int {\n    return 7\n}\n\npub fn take_beta() -> int {\n    return helper()\n}\n",
            ),
            ("main.aelys", SUM_MAIN),
        ],
        "12",
        "the private function already scopes per module and must stay that way",
    );
}
