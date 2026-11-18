use aelys::{new_vm, new_vm_with_config, run_with_vm_and_opt};
use aelys_opt::OptimizationLevel;
use aelys_runtime::{VM, VmConfig};

const CHUNK: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const BIG_CHUNKS: usize = 4096;
const BIG: usize = 1 + CHUNK.len() * BIG_CHUNKS;

fn big_prelude() -> String {
    format!(
        "fn bigstr(n: int) -> string {{\n    let mut s = \"y\"\n    let mut i = 0\n    while i < n {{ s += \"{CHUNK}\"; i += 1 }}\n    s\n}}\nlet big = bigstr({BIG_CHUNKS})\n"
    )
}

fn run(vm: &mut VM, source: &str) -> Result<(), String> {
    run_with_vm_and_opt(vm, source, "<test>", OptimizationLevel::Standard)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn budget_vm(bytes: u64) -> VM {
    new_vm_with_config(VmConfig::new(bytes).expect("a legal budget"), Vec::new())
        .expect("a vm starts")
}

fn retained_bytes_after(source: &str) -> usize {
    let mut vm = new_vm().expect("a vm starts");
    run(&mut vm, source).expect("the program runs");
    vm.collect();
    vm.heap().bytes_allocated()
}

#[test]
fn a_short_string_that_once_grew_is_charged_for_its_whole_buffer() {
    for keep_form in ["keep.push(small)", "keep.push(small + \"\")"] {
        let source = format!(
            "{}let mut keep: Vec<string> = vec![]\nlet mut i = 0\nwhile i < 40 {{\n    let small = \"k\" + \"{{i}}\"\n    let grown = small + big\n    if grown.len() != small.len() + big.len() {{ keep.push(\"BAD\") }}\n    {keep_form}\n    i += 1\n}}\nkeep.len()\n",
            big_prelude()
        );
        let retained = retained_bytes_after(&source);
        if keep_form == "keep.push(small)" {
            assert!(
                retained >= 40 * BIG,
                "40 kept views each pin a {BIG}-byte buffer, yet the heap counts {retained} bytes"
            );
        } else {
            assert!(
                retained < 4 * BIG,
                "copies of the short views pin nothing, yet the heap counts {retained} bytes"
            );
        }
    }
}

#[test]
fn the_doubled_capacity_of_a_copied_concatenation_is_charged() {
    let source = format!(
        "{}let mut keep: Vec<string> = vec![]\nlet mut i = 0\nwhile i < 20 {{\n    let base = \"b\" + \"{{i}}\"\n    let tip = base + \"z\"\n    let copied = base + big\n    keep.push(copied)\n    keep.push(tip)\n    i += 1\n}}\nkeep.len()\n",
        big_prelude()
    );
    let retained = retained_bytes_after(&source);
    assert!(
        retained >= 20 * 2 * BIG,
        "20 copies reserve twice their {BIG} bytes, yet the heap counts {retained} bytes"
    );
}

#[test]
fn hidden_buffers_exhaust_the_heap_budget_with_a_clean_error() {
    for keep in [
        "keep.push(small)",
        "keep.push(small)\n    let mut t: string = small\n    t.push('!')",
    ] {
        let source = format!(
            "{}let mut keep: Vec<string> = vec![]\nlet mut i = 0\nwhile i < 100 {{\n    let small = \"k\" + \"{{i}}\"\n    let grown = small + big\n    if grown.len() != small.len() + big.len() {{ keep.push(\"BAD\") }}\n    {keep}\n    i += 1\n}}\nkeep.len()\n",
            big_prelude()
        );
        let mut vm = budget_vm(4 * 1024 * 1024);
        let err = run(&mut vm, &source).expect_err("100 pinned buffers of 256 KiB exceed 4 MiB");
        assert!(err.contains("out of memory"), "{err}");
    }
}

#[test]
fn buffers_are_released_when_their_last_view_dies() {
    let source = format!(
        "{}let mut total = 0\nlet mut i = 0\nwhile i < 300 {{\n    let small = \"k\" + \"{{i}}\"\n    let grown = small + big\n    let again = small + \"x\"\n    let mut pushed: string = small\n    pushed.push('c')\n    total += grown.len() + again.len() + pushed.len()\n    i += 1\n}}\ntotal\n",
        big_prelude()
    );
    let mut vm = budget_vm(4 * 1024 * 1024);
    run(&mut vm, &source).expect("dead buffers return their bytes, so the loop fits");
    vm.collect();
    let retained = vm.heap().bytes_allocated();
    assert!(
        retained < 3 * BIG,
        "only `big` stays alive, yet the heap counts {retained} bytes"
    );
}

fn error_line(message: &str) -> Option<u32> {
    message
        .lines()
        .find_map(|line| line.trim().strip_prefix("--> "))
        .and_then(|location| location.rsplit(':').nth(1))
        .and_then(|line| line.parse::<u32>().ok())
}

#[test]
fn map_growth_past_the_budget_fails_at_the_insert() {
    for insert in ["m[i] = i", "let _ = m.insert(i, i)", "m[\"k{i}\"] = i"] {
        let key = if insert.contains('"') {
            "string"
        } else {
            "int"
        };
        let source = format!(
            "let mut m: Map<{key}, int> = Map::new()\nlet mut i = 0\nwhile i < 3000000 {{ {insert}; i += 1 }}\nprintln(\"filled {{m.len()}}\")\n"
        );
        let mut vm = budget_vm(8 * 1024 * 1024);
        let err = run(&mut vm, &source).expect_err("3M entries do not fit in 8 MiB");
        assert!(err.contains("out of memory"), "{insert}: {err}");
        assert_eq!(error_line(&err), Some(3), "{insert}: {err}");
    }
}

#[test]
fn a_map_that_fits_the_budget_still_fills() {
    let mut vm = budget_vm(8 * 1024 * 1024);
    run(
        &mut vm,
        "let mut m: Map<int, int> = Map::new()\nlet mut i = 0\nwhile i < 20000 { m[i] = i; i += 1 }\nlet mut j = 0\nwhile j < 20000 { let _ = m.remove(j); m[j + 1000000] = j; j += 1 }\nm.len()\n",
    )
    .expect("20k entries fit in 8 MiB");
}
