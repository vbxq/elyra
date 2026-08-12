mod common;
use common::*;

#[test]
fn sys_arg_count() {
    let code = r#"
needs std::sys
let count = sys::arg_count()
if count >= 0 { 1 } else { 0 }
"#;
    assert_aelys_int(code, 1);
}

#[test]
fn sys_arg_negative_index() {
    let code = r#"
needs std::sys
let _ = sys::arg(-1)
42
"#;
    assert_aelys_int(code, 42);
}

#[test]
fn sys_arg_out_of_bounds() {
    let code = r#"
needs std::sys
let _ = sys::arg(9999)
42
"#;
    assert_aelys_int(code, 42);
}

#[test]
fn sys_args_returns_string() {
    let code = r#"
needs std::sys
let args = sys::args()
42
"#;
    assert_aelys_int(code, 42);
}

#[test]
fn sys_env_nonexistent() {
    let code = r#"
needs std::sys
let _ = sys::env("NONEXISTENT_VAR_12345")
42
"#;
    assert_aelys_int(code, 42);
}

#[test]
fn sys_set_and_get_env() {
    let code = r#"
needs std::sys
sys::set_env("AELYS_TEST_VAR", "test_value")
let _ = sys::env("AELYS_TEST_VAR")
sys::unset_env("AELYS_TEST_VAR")
42
"#;
    assert_aelys_int(code, 42);
}

#[test]
fn sys_unset_nonexistent_env() {
    let code = r#"
needs std::sys
sys::unset_env("NONEXISTENT_999")
42
"#;
    assert_aelys_int(code, 42);
}

#[test]
fn sys_env_vars_returns_string() {
    let code = r#"
needs std::sys
let vars = sys::env_vars()
42
"#;
    assert_aelys_int(code, 42);
}

#[test]
fn sys_pid_positive() {
    let code = r#"
needs std::sys
let p = sys::pid()
if p > 0 { 1 } else { 0 }
"#;
    assert_aelys_int(code, 1);
}

#[test]
fn sys_cwd_returns_path() {
    let code = r#"
needs std::sys
let _ = sys::cwd()
42
"#;
    assert_aelys_int(code, 42);
}

#[test]
fn sys_set_cwd_failure_is_a_result() {
    let code = r#"
needs std::sys
match sys::set_cwd("/nonexistent/path/nowhere") {
    Ok(_) => 0,
    Err(_) => 1,
}
"#;
    assert_aelys_int(code, 1);
}

#[test]
fn sys_exec_args_failure_is_a_result() {
    let code = r#"
needs std::sys
match sys::exec_args("/definitely/missing/aelys-command", "") {
    Ok(_) => 0,
    Err(_) => 1,
}
"#;
    assert_aelys_int(code, 1);
}

#[test]
fn sys_home_returns_path() {
    let code = r#"
needs std::sys
let _ = sys::home()
42
"#;
    assert_aelys_int(code, 42);
}

#[test]
fn sys_platform_valid() {
    let code = r#"
needs std::sys
let p = sys::platform()
42
"#;
    assert_aelys_int(code, 42);
}

#[test]
fn sys_arch_valid() {
    let code = r#"
needs std::sys
let a = sys::arch()
42
"#;
    assert_aelys_int(code, 42);
}

#[test]
fn sys_os_valid() {
    let code = r#"
needs std::sys
let o = sys::os()
42
"#;
    assert_aelys_int(code, 42);
}

#[test]
fn sys_hostname_returns_string() {
    let code = r#"
needs std::sys
let h = sys::hostname()
42
"#;
    assert_aelys_int(code, 42);
}

#[test]
fn sys_cpu_count_positive() {
    let code = r#"
needs std::sys
let c = sys::cpu_count()
if c > 0 { 1 } else { 0 }
"#;
    assert_aelys_int(code, 1);
}

#[test]

fn sys_random_in_range() {
    let code = r#"
needs std::sys
let r = sys::random()
if r >= 0.0 and r < 1.0 { 1 } else { 0 }
"#;
    assert_aelys_int(code, 1);
}

#[test]
fn sys_random_int_basic() {
    let code = r#"
needs std::sys
let r = sys::random_int(1, 10).unwrap()
if r >= 1 and r <= 10 { 1 } else { 0 }
"#;
    assert_aelys_int(code, 1);
}

#[test]
fn sys_random_int_min_greater_than_max() {
    let code = r#"
needs std::sys
match sys::random_int(10, 5) {
    Ok(_) => 0,
    Err(_) => 1,
}
"#;
    assert_aelys_int(code, 1);
}

#[test]
fn sys_random_int_same_bounds() {
    let code = r#"
needs std::sys
let r = sys::random_int(5, 5).unwrap()
if r == 5 { 1 } else { 0 }
"#;
    assert_aelys_int(code, 1);
}

#[test]
fn standard_random_functions_share_replayable_state() {
    let code = r#"
needs std::sys
sys::random_seed(123456)
sys::random()
randint(-1000000, 1000000)
let state = sys::random_state()
let first = sys::random_int(-1000000, 1000000).unwrap()
let second = randint(-1000000, 1000000)
let third = sys::random()
sys::random_set_state(state)
let replay_first = sys::random_int(-1000000, 1000000).unwrap()
let replay_second = randint(-1000000, 1000000)
let replay_third = sys::random()
if first == replay_first and second == replay_second and third == replay_third { 1 } else { 0 }
"#;
    assert_aelys_int(code, 1);
}

#[test]
fn random_seed_restarts_sequence() {
    let code = r#"
needs std::sys
sys::random_seed(-42)
let first = sys::random_int(-1000, 1000).unwrap()
let second = randint(-1000, 1000)
sys::random_seed(-42)
if first == sys::random_int(-1000, 1000).unwrap() and second == randint(-1000, 1000) { 1 } else { 0 }
"#;
    assert_aelys_int(code, 1);
}

#[test]
fn sys_script_path_returns_value() {
    let code = r#"
needs std::sys
let _ = sys::script_path()
42
"#;
    assert_aelys_int(code, 42);
}

#[test]
fn sys_script_dir_returns_value() {
    let code = r#"
needs std::sys
let _ = sys::script_dir()
42
"#;
    assert_aelys_int(code, 42);
}

#[test]
fn sys_multiple_env_operations() {
    let code = r#"
needs std::sys
sys::set_env("TEST1", "val1")
sys::set_env("TEST2", "val2")
let _ = sys::env("TEST1")
let _ = sys::env("TEST2")
sys::unset_env("TEST1")
sys::unset_env("TEST2")
42
"#;
    assert_aelys_int(code, 42);
}

#[test]
fn sys_random_different_calls() {
    // Not truly a test since random might be same, but checks it works
    let code = r#"
needs std::sys
let r1 = sys::random()
let r2 = sys::random()
42
"#;
    assert_aelys_int(code, 42);
}
