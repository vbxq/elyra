use aelys::run_with_config_and_opt;
use aelys_opt::OptimizationLevel;
use aelys_runtime::VmConfig;

fn value_of(source: &str) -> String {
    let result = run_with_config_and_opt(
        source,
        "global_writeback",
        VmConfig::default(),
        Vec::new(),
        OptimizationLevel::Standard,
    )
    .expect("program runs");
    format!("{result:?}")
}

/// the write-back of indexed globals is skipped when nothing was written, so a writer must still see it after a call
#[test]
fn a_global_written_in_one_function_is_read_by_another() {
    let source = r#"
let mut counter = 0

fn bump() {
    counter = counter + 7
    return 0
}

fn read_back() -> int {
    return counter
}

fn main() -> int {
    bump()
    return read_back()
}

main()
"#;
    assert_eq!(value_of(source), "7");
}

#[test]
fn a_global_written_between_two_calls_keeps_its_last_value() {
    let source = r#"
let mut counter = 0

fn bump() {
    counter = counter + 1
    return 0
}

fn read_back() -> int {
    return counter
}

fn main() -> int {
    bump()
    read_back()
    bump()
    read_back()
    bump()
    return read_back()
}

main()
"#;
    assert_eq!(value_of(source), "3");
}

#[test]
fn a_function_that_only_reads_globals_leaves_them_alone() {
    let source = r#"
let mut counter = 5

fn read_back() -> int {
    return counter
}

fn twice() -> int {
    return read_back() + read_back()
}

fn main() -> int {
    let first = twice()
    counter = counter + 1
    return first + twice()
}

main()
"#;
    assert_eq!(value_of(source), "22");
}

/// prepared views are kept per mapping and reinstalled on every call between two functions with different global lists
#[test]
fn a_view_reinstalled_after_a_write_carries_the_new_value() {
    let source = r#"
let mut shared = 0
let mut witness = 0

fn writer(value: int) -> int {
    shared = value
    return shared
}

fn reader() -> int {
    witness = shared
    return witness
}

fn main() -> int {
    let mut i = 1
    let mut total = 0
    while i <= 5 {
        writer(i * 10)
        total = total + reader()
        i = i + 1
    }
    return total
}

main()
"#;
    assert_eq!(value_of(source), "150");
}

#[test]
fn two_functions_with_different_global_lists_keep_their_own_view() {
    let source = r#"
let mut a = 1
let mut b = 2
let mut c = 3

fn only_a() -> int {
    a = a + 1
    return a
}

fn only_b() -> int {
    b = b + a
    return b
}

fn all_three() -> int {
    c = a + b + c
    return c
}

fn main() -> int {
    only_a()
    only_b()
    all_three()
    only_a()
    only_b()
    return all_three()
}

main()
"#;
    assert_eq!(value_of(source), "19");
}
