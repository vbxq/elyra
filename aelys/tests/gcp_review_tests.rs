use aelys::{new_vm_with_config, run_with_vm_and_opt};
use aelys_opt::OptimizationLevel;
use aelys_runtime::{VM, VmConfig};

fn stressed() -> VmConfig {
    VmConfig {
        gc_verify: true,
        gc_stress: true,
        gc_nursery_bytes: VmConfig::MIN_GC_NURSERY_BYTES,
        ..VmConfig::default()
    }
}

fn run(vm: &mut VM, source: &str) -> i64 {
    run_with_vm_and_opt(vm, source, "<rev-gcp>", OptimizationLevel::Standard)
        .unwrap_or_else(|error| panic!("program failed: {error}"))
        .as_int()
        .expect("int result")
}

fn module(i: usize) -> String {
    let mut body = String::new();
    for k in 0..40 {
        body.push_str(&format!("    s = s + {k} * x + \"{i}-{k}\".len()\n"));
    }
    format!(
        "fn f(x: int) -> int {{\n    let mut s = 0\n{body}    s\n}}\nlet g = fn(y: int) -> int {{ return f(y) + 1 }}\ng({i})\n"
    )
}

fn expected(i: usize) -> i64 {
    let mut s = 0i64;
    for k in 0..40i64 {
        s += k * i as i64 + format!("{i}-{k}").len() as i64;
    }
    s + 1
}

fn many_modules_on_one_vm(config: VmConfig, count: usize) {
    let mut vm = new_vm_with_config(config, Vec::new()).expect("vm");
    for i in 0..count {
        assert_eq!(run(&mut vm, &module(i)), expected(i), "module {i}");
        if i % 7 == 0 {
            vm.collect();
        }
        if i == count - 1 {
            let live = vm.heap().object_count();
            assert!(
                live < 20_000,
                "functions of dead modules must be freed: {live} live objects"
            );
        }
    }
}

#[test]
fn functions_of_dead_modules_freed_in_the_background_under_verify() {
    many_modules_on_one_vm(stressed(), 120);
}

#[test]
fn concurrent_vms_share_the_reclaimer() {
    let handles: Vec<_> = (0..4)
        .map(|t| {
            std::thread::spawn(move || {
                let config = if t % 2 == 0 { stressed() } else { VmConfig::default() };
                many_modules_on_one_vm(config, 60);
                let mut vm = new_vm_with_config(VmConfig::default(), Vec::new()).expect("vm");
                let source = "let mut t = 0\nfor r in 0..6 { let mut b: Vec<u8> = vec![]\n b.resize(8000000, 3)\n let mut v: Vec<string> = vec![]\n for i in 0..20000 { v.push(\"s{i}\") }\n t = t + b.read_u8(7999999) + v[19999].len() }\nt";
                assert_eq!(run(&mut vm, source), 6 * (3 + 6));
            })
        })
        .collect();
    for handle in handles {
        handle.join().expect("thread");
    }
}

#[test]
fn vms_dropped_with_frees_pending() {
    for round in 0..40 {
        let mut vm = new_vm_with_config(VmConfig::default(), Vec::new()).expect("vm");
        let source = "let mut t = 0\nfor r in 0..3 { let mut b: Vec<u8> = vec![]\n b.resize(16000000, 5)\n t = t + b.read_u8(0) }\nt";
        assert_eq!(run(&mut vm, source), 15, "round {round}");
        vm.collect();
        drop(vm);
    }
}
