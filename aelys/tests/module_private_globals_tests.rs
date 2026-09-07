use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use tempfile::TempDir;

use aelys_driver::{run_file, run_file_with_config_and_opt};
use aelys_opt::OptimizationLevel;
use aelys_runtime::VmConfig;

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

fn alpha_module(shared: i64) -> String {
    format!(
        "let shared = {shared}\npub struct A1 {{ pub v: int }}\npub trait TA {{\n    fn ta(self) -> int\n}}\nimpl TA for A1 {{\n    fn ta(self) -> int {{\n        self.v + shared\n    }}\n}}\n"
    )
}

fn beta_module(shared: i64) -> String {
    format!(
        "let shared = {shared}\npub struct B1 {{ pub v: int }}\npub trait TB {{\n    fn tb(self) -> int\n}}\nimpl TB for B1 {{\n    fn tb(self) -> int {{\n        self.v + shared\n    }}\n}}\n"
    )
}

const TWO_MODULE_MAIN: &str = "needs A1, TA from alpha\nneeds B1, TB from beta\nfn main() -> int {\n    let a = A1 { v: 0 }\n    let b = B1 { v: 0 }\n    return a.ta() * 1000 + b.tb()\n}\nmain()\n";

#[test]
fn two_modules_with_the_same_private_global_each_read_their_own() {
    let dir = create_module_env();
    write_file(&dir, "alpha.aelys", &alpha_module(10));
    write_file(&dir, "beta.aelys", &beta_module(200));
    let main_path = write_file(&dir, "main.aelys", TWO_MODULE_MAIN);

    let value = run_file(&main_path).expect("both private globals must resolve");
    assert_eq!(
        value.as_int(),
        Some(10200),
        "10200 pins alpha to 10 and beta to 200; one shared slot gives 200200"
    );
}

#[test]
fn the_second_module_constant_never_moves_the_first_answer() {
    let dir = create_module_env();
    write_file(&dir, "alpha.aelys", &alpha_module(10));
    write_file(&dir, "beta.aelys", &beta_module(999));
    let main_path = write_file(&dir, "main.aelys", TWO_MODULE_MAIN);

    let value = run_file(&main_path).expect("both private globals must resolve");
    assert_eq!(
        value.as_int(),
        Some(10999),
        "alpha still answers 10 once beta's constant moves"
    );
}

#[test]
fn a_public_and_a_private_global_of_the_same_name_stay_apart() {
    let dir = create_module_env();
    write_file(
        &dir,
        "pubmod.aelys",
        "pub let tag = 7\npub struct P1 { pub v: int }\npub trait TP {\n    fn tp(self) -> int\n}\nimpl TP for P1 {\n    fn tp(self) -> int {\n        self.v + tag\n    }\n}\n",
    );
    write_file(
        &dir,
        "privmod.aelys",
        "let tag = 500\npub struct Q1 { pub v: int }\npub trait TQ {\n    fn tq(self) -> int\n}\nimpl TQ for Q1 {\n    fn tq(self) -> int {\n        self.v + tag\n    }\n}\n",
    );
    let main_path = write_file(
        &dir,
        "main.aelys",
        "needs P1, TP from pubmod\nneeds Q1, TQ from privmod\nfn main() -> int {\n    let p = P1 { v: 0 }\n    let q = Q1 { v: 0 }\n    return p.tp() * 1000 + q.tq()\n}\nmain()\n",
    );

    let value = run_file(&main_path).expect("an exported and a private name must not share a slot");
    assert_eq!(value.as_int(), Some(7500));
}

#[test]
fn a_nested_module_path_keeps_its_own_private_global() {
    let dir = create_module_env();
    write_file(
        &dir,
        "pkg/sub/deep.aelys",
        "let level = 3\npub struct D1 { pub v: int }\npub trait TD {\n    fn td(self) -> int\n}\nimpl TD for D1 {\n    fn td(self) -> int {\n        self.v + level\n    }\n}\n",
    );
    write_file(
        &dir,
        "flat.aelys",
        "let level = 900\npub struct F1 { pub v: int }\npub trait TF {\n    fn tf(self) -> int\n}\nimpl TF for F1 {\n    fn tf(self) -> int {\n        self.v + level\n    }\n}\n",
    );
    let main_path = write_file(
        &dir,
        "main.aelys",
        "needs D1, TD from pkg::sub::deep\nneeds F1, TF from flat\nfn main() -> int {\n    let d = D1 { v: 0 }\n    let f = F1 { v: 0 }\n    return d.td() * 1000 + f.tf()\n}\nmain()\n",
    );

    let value = run_file(&main_path).expect("a nested path must carry its own module identity");
    assert_eq!(value.as_int(), Some(3900));
}

#[test]
fn an_aliased_module_import_keeps_its_own_private_global() {
    let dir = create_module_env();
    write_file(
        &dir,
        "aliased.aelys",
        "let bump = 11\npub struct AA { pub v: int }\npub trait TAA {\n    fn taa(self) -> int\n}\nimpl TAA for AA {\n    fn taa(self) -> int {\n        self.v + bump\n    }\n}\n",
    );
    write_file(
        &dir,
        "other.aelys",
        "let bump = 400\npub struct BB { pub v: int }\npub trait TBB {\n    fn tbb(self) -> int\n}\nimpl TBB for BB {\n    fn tbb(self) -> int {\n        self.v + bump\n    }\n}\n",
    );
    let main_path = write_file(
        &dir,
        "main.aelys",
        "needs aliased as al\nneeds other as ot\nfn main() -> int {\n    let a = AA { v: 0 }\n    let b = BB { v: 0 }\n    return a.taa() * 1000 + b.tbb()\n}\nmain()\n",
    );

    let value = run_file(&main_path).expect("an alias must not merge two private globals");
    assert_eq!(value.as_int(), Some(11400));
}

#[test]
fn a_module_that_imports_a_third_reads_its_own_and_the_third_global() {
    let dir = create_module_env();
    write_file(&dir, "third.aelys", "pub let base = 5\n");
    write_file(
        &dir,
        "middle.aelys",
        "needs base from third\nlet delta = 20\npub struct MM { pub v: int }\npub trait TMM {\n    fn tmm(self) -> int\n}\nimpl TMM for MM {\n    fn tmm(self) -> int {\n        self.v + delta + base\n    }\n}\n",
    );
    write_file(
        &dir,
        "rival.aelys",
        "let delta = 700\npub struct RR { pub v: int }\npub trait TRR {\n    fn trr(self) -> int\n}\nimpl TRR for RR {\n    fn trr(self) -> int {\n        self.v + delta\n    }\n}\n",
    );
    let main_path = write_file(
        &dir,
        "main.aelys",
        "needs MM, TMM from middle\nneeds RR, TRR from rival\nfn main() -> int {\n    let m = MM { v: 0 }\n    let r = RR { v: 0 }\n    return m.tmm() * 1000 + r.trr()\n}\nmain()\n",
    );

    let value = run_file(&main_path).expect("a re-exported body must read both of its own names");
    assert_eq!(
        value.as_int(),
        Some(25700),
        "25 pins middle's own 20 plus the third module's 5, 700 pins the rival's own delta"
    );
}

#[test]
fn a_module_that_imports_a_third_calls_it_module_qualified_from_an_imported_body() {
    let dir = create_module_env();
    write_file(
        &dir,
        "third.aelys",
        "pub fn deepfn() -> int {\n    return 5\n}\n",
    );
    write_file(
        &dir,
        "middle.aelys",
        "needs third\nlet delta = 20\npub struct MM { pub v: int }\npub trait TMM {\n    fn tmm(self) -> int\n}\nimpl TMM for MM {\n    fn tmm(self) -> int {\n        return self.v + delta + third::deepfn()\n    }\n}\n",
    );
    let main_path = write_file(
        &dir,
        "main.aelys",
        "needs MM, TMM from middle\nfn main() -> int {\n    let m = MM { v: 100 }\n    return m.tmm()\n}\nmain()\n",
    );

    let value = run_file(&main_path).expect("an imported body must reach its module's imports");
    assert_eq!(
        value.as_int(),
        Some(125),
        "125 pins the importer's 100, middle's own 20 and the third module's 5"
    );

    let leak_path = write_file(
        &dir,
        "leak.aelys",
        "needs MM, TMM from middle\nfn main() -> int {\n    return third::deepfn()\n}\nmain()\n",
    );
    let leaked = run_file(&leak_path)
        .expect_err("the importer never wrote 'needs third'")
        .to_string();
    assert!(
        leaked.contains("third::deepfn"),
        "the alias belongs to the defining module alone: {leaked}"
    );
}

#[test]
fn a_module_that_imports_a_type_reads_it_from_an_imported_body() {
    let dir = create_module_env();
    write_file(
        &dir,
        "core.aelys",
        "pub struct K { pub v: int }\npub trait TK {\n    fn tk(self) -> int\n}\nimpl TK for K {\n    fn tk(self) -> int {\n        return self.v + 1\n    }\n}\n",
    );
    write_file(
        &dir,
        "left.aelys",
        "needs K, TK from core\npub struct L { pub v: int }\npub trait TL {\n    fn tl(self) -> int\n}\npub fn viafree(v: int) -> int {\n    let k = K { v: v }\n    return k.tk()\n}\nimpl TL for L {\n    fn tl(self) -> int {\n        let k = K { v: self.v }\n        return k.tk()\n    }\n}\n",
    );
    let main_path = write_file(
        &dir,
        "main.aelys",
        "needs L, TL, viafree from left\nfn main() -> int {\n    let a = viafree(7)\n    let b = L { v: 20 }\n    return a * 1000 + b.tl()\n}\nmain()\n",
    );

    let value = run_file(&main_path).expect("an imported body must reach its module's own types");
    assert_eq!(
        value.as_int(),
        Some(8021),
        "8021 pins the free function twin at 8 and the impl body twin at 21"
    );

    let named_path = write_file(
        &dir,
        "named.aelys",
        "needs L, TL, viafree from left\nfn main() -> int {\n    let k = K { v: 1 }\n    return k.tk()\n}\nmain()\n",
    );
    let named = run_file(&named_path)
        .expect_err("the importer never wrote 'needs K from core'")
        .to_string();
    assert!(
        named.contains("'K' is exported by module 'core' but this file does not import it"),
        "the struct belongs to the defining module alone: {named}"
    );

    let bound_path = write_file(
        &dir,
        "bound.aelys",
        "needs L, TL, viafree from left\nfn pull<T: TK>(x: T) -> int {\n    return x.tk()\n}\nfn main() -> int {\n    return pull(L { v: 1 })\n}\nmain()\n",
    );
    let bound = run_file(&bound_path)
        .expect_err("the importer never wrote 'needs TK from core'")
        .to_string();
    assert!(
        bound.contains("'TK' is exported by module 'core' but this file does not import it"),
        "the trait belongs to the defining module alone: {bound}"
    );

    let clash_path = write_file(
        &dir,
        "clash.aelys",
        "needs L, TL, viafree from left\nstruct K { v: int }\nfn main() -> int {\n    return K { v: 1 }.v\n}\nmain()\n",
    );
    let clash = run_file(&clash_path)
        .expect_err("the importer's own name collides with the one the body needs")
        .to_string();
    assert!(
        clash.contains("symbol 'K' is exported by multiple modules: core, this module"),
        "the collision names the module, as an imported one does: {clash}"
    );
}

#[test]
fn the_importer_declaring_the_same_name_never_reaches_the_imported_body() {
    let dir = create_module_env();
    write_file(
        &dir,
        "support2.aelys",
        "let secret = 42\npub struct Counter { pub value: int }\npub trait Source {\n    fn next(self) -> int\n}\nimpl Source for Counter {\n    fn next(self) -> int {\n        let v = self.value + secret\n        return v\n    }\n}\n",
    );
    let main_path = write_file(
        &dir,
        "main4.aelys",
        "needs Counter, Source from support2\nlet secret = \"boom\"\nfn main() -> int {\n    let c = Counter { value: 89 }\n    return c.next()\n}\nmain()\n",
    );

    for level in [
        OptimizationLevel::None,
        OptimizationLevel::Basic,
        OptimizationLevel::Standard,
        OptimizationLevel::Aggressive,
    ] {
        let value =
            run_file_with_config_and_opt(&main_path, VmConfig::default(), Vec::new(), level)
                .unwrap_or_else(|err| panic!("{level:?} must accept the program: {err}"));
        assert_eq!(
            value.as_int(),
            Some(131),
            "{level:?} must read support2's 42, not the importer's string"
        );
    }
}

#[test]
fn a_lone_module_still_reaches_its_own_private_global() {
    let dir = create_module_env();
    write_file(&dir, "alpha.aelys", &alpha_module(10));
    let main_path = write_file(
        &dir,
        "main.aelys",
        "needs A1, TA from alpha\nfn main() -> int {\n    let a = A1 { v: 0 }\n    return a.ta()\n}\nmain()\n",
    );

    let value = run_file(&main_path).expect("one module alone must keep working");
    assert_eq!(value.as_int(), Some(10));
}

#[test]
fn a_local_binding_shadows_the_module_private_global() {
    let dir = create_module_env();
    write_file(
        &dir,
        "shadow.aelys",
        "let weight = 100\npub struct SS { pub v: int }\npub trait TSS {\n    fn tss(self) -> int\n}\nimpl TSS for SS {\n    fn tss(self) -> int {\n        let weight = 3\n        return self.v + weight\n    }\n}\n",
    );
    let main_path = write_file(
        &dir,
        "main.aelys",
        "needs SS, TSS from shadow\nfn main() -> int {\n    let s = SS { v: 1 }\n    return s.tss()\n}\nmain()\n",
    );

    let value = run_file(&main_path).expect("a local must win over the module global");
    assert_eq!(value.as_int(), Some(4));
}

const STORE_TOP_LEVEL: &str =
    "let mut v: int = 100\n\nv = v + 5\n\npub fn read_v() -> int {\n    return v\n}\n";

const MAIN_HOLDING_V: &str = "needs read_v from store\n\nlet mut v: int = 9\n\nfn main() -> int {\n    return read_v() * 1000 + v\n}\n\nmain()\n";

#[test]
fn the_module_top_level_and_a_free_function_write_and_read_one_slot() {
    let dir = create_module_env();
    write_file(&dir, "store.aelys", STORE_TOP_LEVEL);
    let main_path = write_file(&dir, "main.aelys", MAIN_HOLDING_V);

    let value = run_file(&main_path).expect("the module must reach its own global");
    assert_eq!(
        value.as_int(),
        Some(105009),
        "105 is the module's own slot after its top level ran; 9 is the importer's own v"
    );
}

#[test]
fn a_free_function_writes_the_slot_a_second_free_function_reads() {
    let dir = create_module_env();
    write_file(
        &dir,
        "store.aelys",
        "let mut v: int = 100\n\npub fn bump_v() -> int {\n    v = v + 5\n    return v\n}\n\npub fn read_v() -> int {\n    return v\n}\n",
    );
    let main_path = write_file(
        &dir,
        "main.aelys",
        "needs bump_v, read_v from store\n\nlet mut v: int = 9\n\nfn main() -> int {\n    let a = bump_v()\n    let b = read_v()\n    return a * 100000 + b * 100 + v\n}\n\nmain()\n",
    );

    let value = run_file(&main_path).expect("both free functions must reach one slot");
    assert_eq!(
        value.as_int(),
        Some(10510509),
        "105 twice is the module's slot written then read; 9 is the importer's own v"
    );
}

#[test]
fn the_module_own_impl_body_writes_the_slot_its_free_function_reads() {
    let dir = create_module_env();
    write_file(
        &dir,
        "store.aelys",
        "let mut v: int = 100\n\npub struct Cell { pub k: int }\npub trait Step {\n    fn tick(self) -> int\n}\nimpl Step for Cell {\n    fn tick(self) -> int {\n        v = v + 5\n        return v\n    }\n}\n\nlet seeded: int = Cell { k: 0 }.tick()\n\npub fn read_v() -> int {\n    return v\n}\n\npub fn read_seeded() -> int {\n    return seeded\n}\n",
    );
    let main_path = write_file(
        &dir,
        "main.aelys",
        "needs read_v, read_seeded from store\n\nlet mut v: int = 9\n\nfn main() -> int {\n    return read_seeded() * 100000 + read_v() * 100 + v\n}\n\nmain()\n",
    );

    let value = run_file(&main_path).expect("the module's own impl body must reach one slot");
    assert_eq!(
        value.as_int(),
        Some(10510509),
        "the impl body wrote 105 and the free function reads the same 105"
    );
}

#[test]
fn an_imported_impl_body_writes_the_slot_the_module_free_function_reads() {
    let dir = create_module_env();
    write_file(
        &dir,
        "store.aelys",
        "let mut v: int = 100\n\npub struct Cell { pub k: int }\npub trait Step {\n    fn tick(self) -> int\n}\nimpl Step for Cell {\n    fn tick(self) -> int {\n        v = v + 5\n        return v\n    }\n}\n\npub fn read_v() -> int {\n    return v\n}\n",
    );
    let main_path = write_file(
        &dir,
        "main.aelys",
        "needs Cell, Step, read_v from store\n\nlet mut v: int = 9\n\nfn main() -> int {\n    let c = Cell { k: 0 }\n    let a = c.tick()\n    let b = read_v()\n    return a * 100000 + b * 100 + v\n}\n\nmain()\n",
    );

    let value = run_file(&main_path).expect("an imported body and a free function share one slot");
    assert_eq!(
        value.as_int(),
        Some(10510509),
        "the imported body wrote 105 and the module's free function reads that 105"
    );
}

#[test]
fn a_public_module_global_is_read_by_its_own_function_not_by_the_importer_binding() {
    let dir = create_module_env();
    write_file(
        &dir,
        "alpha.aelys",
        "pub let mut v: int = 100\n\npub fn get_alpha() -> int {\n    return v\n}\n",
    );
    let main_path = write_file(
        &dir,
        "main.aelys",
        "needs alpha\n\nlet mut v: int = 9\n\nfn main() -> int {\n    return get_alpha() * 1000 + v\n}\n\nmain()\n",
    );

    let value = run_file(&main_path).expect("the module function must read its own global");
    assert_eq!(
        value.as_int(),
        Some(100009),
        "100 is alpha's own v; one shared slot answers 9009"
    );
}

#[test]
fn two_aliased_modules_with_the_same_public_global_each_read_their_own() {
    let dir = create_module_env();
    write_file(
        &dir,
        "alpha.aelys",
        "pub let mut v: int = 100\n\npub fn get_alpha() -> int {\n    return v\n}\n",
    );
    write_file(
        &dir,
        "beta.aelys",
        "pub let mut v: int = 500\n\npub fn get_beta() -> int {\n    return v\n}\n",
    );
    let main_path = write_file(
        &dir,
        "main.aelys",
        "needs alpha as a\nneeds beta as b\n\nfn main() -> int {\n    return a::get_alpha() * 1000 + b::get_beta()\n}\n\nmain()\n",
    );

    let value = run_file(&main_path).expect("each aliased module keeps its own global");
    assert_eq!(
        value.as_int(),
        Some(100500),
        "one shared slot answers 500500, the alias being the escape past E0410"
    );
}

#[test]
fn a_module_free_function_reads_its_own_immutable_global() {
    let dir = create_module_env();
    write_file(
        &dir,
        "store.aelys",
        "let k: int = 7\n\npub fn read_k() -> int {\n    return k\n}\n",
    );
    let main_path = write_file(
        &dir,
        "main.aelys",
        "needs read_k from store\n\nlet k: int = 500\n\nfn main() -> int {\n    return read_k() * 1000 + k\n}\n\nmain()\n",
    );

    let value = run_file(&main_path).expect("an immutable module global stays the module's own");
    assert_eq!(value.as_int(), Some(7500));
}

#[test]
fn an_imported_impl_body_reads_its_module_immutable_global() {
    let dir = create_module_env();
    write_file(
        &dir,
        "store.aelys",
        "let k: int = 7\n\npub struct Cell { pub n: int }\npub trait Step {\n    fn tick(self) -> int\n}\nimpl Step for Cell {\n    fn tick(self) -> int {\n        return self.n + k\n    }\n}\n",
    );
    let main_path = write_file(
        &dir,
        "main.aelys",
        "needs Cell, Step from store\n\nlet k: int = 500\n\nfn main() -> int {\n    let c = Cell { n: 0 }\n    return c.tick() * 1000 + k\n}\n\nmain()\n",
    );

    let value = run_file(&main_path).expect("an imported body reads its own module's constant");
    assert_eq!(value.as_int(), Some(7500));
}

fn diagnostic_head(message: &str) -> &str {
    message.split("\n  --> ").next().unwrap_or(message)
}

const CARRIED_CORED: &str = "pub struct K { pub v: int }\npub trait TKD {\n    fn shared(self) -> int\n}\nimpl TKD for K {\n    fn shared(self) -> int {\n        return self.v + 1\n    }\n}\n";

const CARRIED_COREC: &str = "pub struct K { pub v: int }\npub trait TKC {\n    fn shared(self) -> int\n}\nimpl TKC for K {\n    fn shared(self) -> int {\n        return self.v + 1000\n    }\n}\n";

const CARRIED_MID3: &str = "needs K, TKD from cored\npub struct L3 { pub v: int }\npub trait TL3 {\n    fn tl3(self) -> int\n}\nimpl TL3 for L3 {\n    fn tl3(self) -> int {\n        let k = K { v: self.v }\n        return k.shared()\n    }\n}\n";

const CARRIED_MID4: &str = "needs K, TKC from corec\npub struct L4 { pub v: int }\npub trait TL4 {\n    fn tl4(self) -> int\n}\nimpl TL4 for L4 {\n    fn tl4(self) -> int {\n        let k = K { v: self.v }\n        return k.shared()\n    }\n}\n";

#[test]
fn two_carried_declarations_of_one_name_are_settled_at_the_importer() {
    let dir = create_module_env();
    write_file(&dir, "cored.aelys", CARRIED_CORED);
    write_file(&dir, "corec.aelys", CARRIED_COREC);
    write_file(&dir, "mid3.aelys", CARRIED_MID3);
    write_file(&dir, "mid4.aelys", CARRIED_MID4);

    let only3 = write_file(
        &dir,
        "only3.aelys",
        "needs L3, TL3 from mid3\nfn main() -> int {\n    let a = L3 { v: 5 }\n    return a.tl3()\n}\nmain()\n",
    );
    assert_eq!(
        run_file(&only3)
            .expect("one carrier alone must run")
            .as_int(),
        Some(6),
        "6 is cored's K, whose shared adds 1"
    );

    let only4 = write_file(
        &dir,
        "only4.aelys",
        "needs L4, TL4 from mid4\nfn main() -> int {\n    let b = L4 { v: 5 }\n    return b.tl4()\n}\nmain()\n",
    );
    assert_eq!(
        run_file(&only4)
            .expect("the other carrier alone must run")
            .as_int(),
        Some(1005),
        "1005 is corec's K, whose shared adds 1000"
    );

    let both = write_file(
        &dir,
        "both.aelys",
        "needs L3, TL3 from mid3\nneeds L4, TL4 from mid4\nfn main() -> int {\n    let a = L3 { v: 5 }\n    let b = L4 { v: 5 }\n    return a.tl3() + b.tl4()\n}\nmain()\n",
    );
    let clash = run_file(&both)
        .expect_err("two modules carry a different 'K' into one table")
        .to_string();
    assert!(clash.contains("E0410"), "unexpected diagnostic: {clash}");
    assert!(
        clash.contains("symbol 'K' is exported by multiple modules: corec, cored"),
        "the report must name both declaring modules: {clash}"
    );
    assert!(
        clash.contains("carried into this file by: mid3, mid4"),
        "the report must name the modules the file did write: {clash}"
    );
    assert!(
        clash.contains("/both.aelys:"),
        "the report belongs to the file that pulled both in: {clash}"
    );

    let swapped = write_file(
        &dir,
        "swapped.aelys",
        "needs L4, TL4 from mid4\nneeds L3, TL3 from mid3\nfn main() -> int {\n    let a = L3 { v: 5 }\n    let b = L4 { v: 5 }\n    return a.tl3() + b.tl4()\n}\nmain()\n",
    );
    let swapped_clash = run_file(&swapped)
        .expect_err("the order of the two lines cannot decide the answer")
        .to_string();
    assert_eq!(
        diagnostic_head(&clash),
        diagnostic_head(&swapped_clash),
        "the report must not depend on the order of the two `needs` lines"
    );

    let twice = run_file(&both)
        .expect_err("the same program must report the same thing")
        .to_string();
    assert_eq!(clash, twice, "one program compiled twice renders once");
}

#[test]
fn a_carried_declaration_against_an_explicit_one_reads_the_same_under_either_order() {
    let dir = create_module_env();
    write_file(
        &dir,
        "corea.aelys",
        "pub struct K { pub v: int }\npub trait TKA {\n    fn ka(self) -> int\n}\nimpl TKA for K {\n    fn ka(self) -> int {\n        return self.v + 1\n    }\n}\n",
    );
    write_file(
        &dir,
        "coreb.aelys",
        "pub struct K { pub a: string, pub b: string, pub v: int }\npub trait TKB {\n    fn kb(self) -> int\n}\nimpl TKB for K {\n    fn kb(self) -> int {\n        return self.v + 2\n    }\n}\n",
    );
    write_file(
        &dir,
        "midk.aelys",
        "needs K, TKA from corea\npub struct L { pub v: int }\npub trait TL {\n    fn tl(self) -> int\n}\nimpl TL for L {\n    fn tl(self) -> int {\n        let k = K { v: self.v }\n        return k.ka()\n    }\n}\n",
    );

    let first = write_file(
        &dir,
        "first.aelys",
        "needs L, TL from midk\nneeds K, TKB from coreb\nfn main() -> int {\n    let k = K { a: \"x\", b: \"y\", v: 3 }\n    return k.kb()\n}\nmain()\n",
    );
    let second = write_file(
        &dir,
        "second.aelys",
        "needs K, TKB from coreb\nneeds L, TL from midk\nfn main() -> int {\n    let k = K { a: \"x\", b: \"y\", v: 3 }\n    return k.kb()\n}\nmain()\n",
    );

    let first_error = run_file(&first)
        .expect_err("an explicit 'K' and a carried one are two declarations")
        .to_string();
    let second_error = run_file(&second)
        .expect_err("an explicit 'K' and a carried one are two declarations")
        .to_string();
    assert!(
        first_error.contains("symbol 'K' is exported by multiple modules: corea, coreb"),
        "unexpected diagnostic: {first_error}"
    );
    assert!(
        first_error.contains("carried into this file by: midk"),
        "the carried side must name the module the file wrote: {first_error}"
    );
    assert_eq!(
        diagnostic_head(&first_error),
        diagnostic_head(&second_error),
        "the order of the two `needs` lines cannot decide the answer"
    );
}

const CARRIED_CORE3: &str = "pub fn helper() -> int {\n    return 3\n}\npub let gvar: int = 7\npub struct K3 { pub v: int }\npub trait TK3 {\n    fn tk3(self) -> int\n}\n";

const CARRIED_CORE3_IMPL: &str =
    "impl TK3 for K3 {\n    fn tk3(self) -> int {\n        return self.v + helper()\n    }\n}\n";

#[test]
fn a_qualified_call_travels_whether_or_not_the_defining_module_has_an_impl() {
    for with_impl in [true, false] {
        let dir = create_module_env();
        let core = if with_impl {
            format!("{CARRIED_CORE3}{CARRIED_CORE3_IMPL}")
        } else {
            CARRIED_CORE3.to_string()
        };
        write_file(&dir, "core3.aelys", &core);
        write_file(
            &dir,
            "mid5.aelys",
            "needs core3\npub struct L4 { pub v: int }\npub trait TL4 {\n    fn tl4(self) -> int\n}\nimpl TL4 for L4 {\n    fn tl4(self) -> int {\n        return core3::helper() * 10\n    }\n}\n",
        );
        let main_path = write_file(
            &dir,
            "main5.aelys",
            "needs L4, TL4 from mid5\nfn main() -> int {\n    let l = L4 { v: 5 }\n    return l.tl4()\n}\nmain()\n",
        );
        let value = run_file(&main_path)
            .unwrap_or_else(|error| panic!("with_impl={with_impl}: {error}"))
            .as_int();
        assert_eq!(
            value,
            Some(30),
            "30 is core3's own helper, reached through the module path"
        );
    }
}

#[test]
fn an_aliased_call_travels_whether_or_not_the_defining_module_has_an_impl() {
    for with_impl in [true, false] {
        let dir = create_module_env();
        let core = if with_impl {
            format!("{CARRIED_CORE3}{CARRIED_CORE3_IMPL}")
        } else {
            CARRIED_CORE3.to_string()
        };
        write_file(&dir, "core3.aelys", &core);
        write_file(
            &dir,
            "mid6.aelys",
            "needs core3 as c3\npub struct L6 { pub v: int }\npub trait TL6 {\n    fn tl6(self) -> int\n}\nimpl TL6 for L6 {\n    fn tl6(self) -> int {\n        return c3::helper() * 10\n    }\n}\n",
        );
        let aliased = write_file(
            &dir,
            "main6.aelys",
            "needs L6, TL6 from mid6\nfn main() -> int {\n    let l = L6 { v: 5 }\n    return l.tl6()\n}\nmain()\n",
        );
        let value = run_file(&aliased)
            .unwrap_or_else(|error| panic!("with_impl={with_impl}: {error}"))
            .as_int();
        assert_eq!(
            value,
            Some(30),
            "30 is core3's helper reached through the alias"
        );
    }
}

#[test]
fn a_selectively_imported_function_and_global_travel_with_the_inlined_body() {
    let dir = create_module_env();
    write_file(
        &dir,
        "core3.aelys",
        &format!("{CARRIED_CORE3}{CARRIED_CORE3_IMPL}"),
    );
    write_file(
        &dir,
        "mid7.aelys",
        "needs K3, TK3, helper, gvar from core3\npub struct L7 { pub v: int }\npub trait TL7 {\n    fn tl7(self) -> int\n}\nimpl TL7 for L7 {\n    fn tl7(self) -> int {\n        let k = K3 { v: self.v }\n        return k.tk3() + helper() * 10 + gvar * 100\n    }\n}\n",
    );
    let selective = write_file(
        &dir,
        "main7.aelys",
        "needs L7, TL7 from mid7\nfn main() -> int {\n    let l = L7 { v: 5 }\n    return l.tl7()\n}\nmain()\n",
    );
    assert_eq!(
        run_file(&selective)
            .expect("a selectively imported function and global must travel")
            .as_int(),
        Some(738),
        "738 is 8 from core3's own impl, 30 from helper and 700 from gvar"
    );
}

#[test]
fn a_module_private_function_travels_with_the_body_the_importer_inlines() {
    let dir = create_module_env();
    write_file(&dir, "c1.aelys", "pub fn f1() -> int {\n    return 2\n}\n");
    write_file(
        &dir,
        "c2.aelys",
        "needs f1 from c1\npub fn f2() -> int {\n    return f1() * 3\n}\n",
    );
    write_file(
        &dir,
        "c3.aelys",
        "needs f2 from c2\nfn secret() -> int {\n    return 4\n}\npub struct L { pub v: int }\npub trait TL {\n    fn tl(self) -> int\n}\nimpl TL for L {\n    fn tl(self) -> int {\n        return f2() * 100 + secret() * 10 + self.v\n    }\n}\n",
    );
    let main_path = write_file(
        &dir,
        "main8.aelys",
        "needs L, TL from c3\nfn main() -> int {\n    let l = L { v: 7 }\n    return l.tl()\n}\nmain()\n",
    );
    assert_eq!(
        run_file(&main_path)
            .expect("a body reaches its own module's private function and its imports")
            .as_int(),
        Some(647),
        "600 is f1 through f2, 40 is c3's own private function, 7 is the receiver"
    );
}

#[test]
fn a_carried_name_the_importer_declares_names_a_repair_that_exists() {
    let dir = create_module_env();
    write_file(
        &dir,
        "corep.aelys",
        "pub struct K { pub v: int }\npub trait TK {\n    fn tk(self) -> int\n}\nimpl TK for K {\n    fn tk(self) -> int {\n        return self.v + 1\n    }\n}\n",
    );
    write_file(
        &dir,
        "midp.aelys",
        "needs K, TK from corep\npub struct M { pub v: int }\npub trait TM {\n    fn tm(self) -> int\n}\nimpl TM for M {\n    fn tm(self) -> int {\n        let k = K { v: self.v }\n        return k.tk()\n    }\n}\n",
    );
    write_file(
        &dir,
        "topp.aelys",
        "needs M, TM from midp\npub struct P { pub v: int }\npub trait TP {\n    fn tp(self) -> int\n}\nimpl TP for P {\n    fn tp(self) -> int {\n        let m = M { v: self.v }\n        return m.tm() + 100\n    }\n}\n",
    );

    let declaring = write_file(
        &dir,
        "declaring.aelys",
        "needs P, TP from topp\nstruct K { v: int }\nfn main() -> int {\n    let p = P { v: 5 }\n    let own = K { v: 1 }\n    return p.tp() + own.v\n}\nmain()\n",
    );
    let clash = run_file(&declaring)
        .expect_err("the file's own 'K' would rebind the one a carried body reads")
        .to_string();
    assert!(
        clash.contains("symbol 'K' is exported by multiple modules: corep, this module"),
        "unexpected diagnostic: {clash}"
    );
    assert!(
        clash.contains("carried into this file by: topp"),
        "the file never named corep, so the report must say how it arrives: {clash}"
    );
    assert!(
        clash.contains("rename this file's 'K'"),
        "the hint must name a repair the file can make: {clash}"
    );
    assert!(
        !clash.contains("use 'as' alias"),
        "a selective import has no 'as' form: {clash}"
    );

    let importing = write_file(
        &dir,
        "importing.aelys",
        "needs P, TP from topp\nneeds K from corep\nfn main() -> int {\n    let p = P { v: 5 }\n    let k = K { v: 1 }\n    return p.tp() + k.v\n}\nmain()\n",
    );
    assert_eq!(
        run_file(&importing)
            .expect("the repair E0378 names must work")
            .as_int(),
        Some(107),
        "106 is the three-level chain and 1 is the importer's own value"
    );
}

#[test]
fn no_module_diagnostic_shows_the_module_global_prefix() {
    let dir = create_module_env();
    write_file(
        &dir,
        "core3.aelys",
        &format!("{CARRIED_CORE3}{CARRIED_CORE3_IMPL}"),
    );
    write_file(
        &dir,
        "mid9.aelys",
        "needs core3 as c9\npub struct L9 { pub v: int }\npub trait TL9 {\n    fn tl9(self) -> int\n}\nimpl TL9 for L9 {\n    fn tl9(self) -> int {\n        return helper() * 10\n    }\n}\n",
    );
    let aliased = write_file(
        &dir,
        "main9.aelys",
        "needs L9, TL9 from mid9\nfn main() -> int {\n    let l = L9 { v: 5 }\n    return l.tl9()\n}\nmain()\n",
    );
    let error = run_file(&aliased)
        .expect_err("an alias hides the bare name")
        .to_string();
    assert!(
        !error.contains(aelys_common::MODULE_GLOBAL_PREFIX),
        "the mangled name names nothing the reader wrote: {error}"
    );
    assert!(
        error.contains("undefined variable 'helper'"),
        "unexpected diagnostic: {error}"
    );
    assert!(
        error.contains("/mid9.aelys:"),
        "the report belongs to the file that owns the statement: {error}"
    );
}

#[test]
fn a_body_two_levels_down_reports_in_the_file_that_owns_it() {
    let dir = create_module_env();
    write_file(
        &dir,
        "corex.aelys",
        "pub struct K { pub v: int }\npub trait TA {\n    fn shared(self) -> int\n}\npub trait TB {\n    fn shared(self) -> int\n}\nimpl TA for K {\n    fn shared(self) -> int {\n        return self.v + 1\n    }\n}\nimpl TB for K {\n    fn shared(self) -> int {\n        return self.v + 1000\n    }\n}\n",
    );
    write_file(
        &dir,
        "deepx.aelys",
        "needs K, TA from corex\npub struct D { pub v: int }\npub trait TD {\n    fn td(self) -> int\n}\nimpl TD for D {\n    fn td(self) -> int {\n        let k = K { v: self.v }\n        return k.shared()\n    }\n}\n",
    );
    write_file(
        &dir,
        "midx.aelys",
        "needs D, TD from deepx\npub struct L { pub v: int }\npub trait TL {\n    fn tl(self) -> int\n}\nimpl TL for L {\n    fn tl(self) -> int {\n        let d = D { v: self.v }\n        return d.td() * 10\n    }\n}\n",
    );

    let plain = write_file(
        &dir,
        "plain.aelys",
        "needs L, TL from midx\nfn main() -> int {\n    let l = L { v: 5 }\n    return l.tl()\n}\nmain()\n",
    );
    assert_eq!(
        run_file(&plain)
            .expect("two levels of carried bodies must run")
            .as_int(),
        Some(60),
        "60 is corex's TA, which adds 1, times deepx's caller"
    );

    let widened = write_file(
        &dir,
        "widened.aelys",
        "needs L, TL from midx\nneeds K, TA, TB from corex\nfn main() -> int {\n    let l = L { v: 5 }\n    return l.tl()\n}\nmain()\n",
    );
    let error = run_file(&widened)
        .expect_err("the second trait makes the carried body's call ambiguous")
        .to_string();
    assert!(error.contains("E0337"), "unexpected diagnostic: {error}");
    assert!(
        error.contains("/deepx.aelys:"),
        "the report belongs to the file that owns the call, not to the importer: {error}"
    );
}
