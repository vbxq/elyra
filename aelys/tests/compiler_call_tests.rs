use aelys_backend::call_window_available;

#[test]
fn call_window_available_checks_bounds_and_usage() {
    let mut pool = [false; 256];
    pool[3] = true;

    assert!(call_window_available(&pool, 3, 0, 3));
    assert!(!call_window_available(&pool, 4, 2, 2));
    assert!(!call_window_available(&pool, 4, 254, 3));
}

#[test]
fn call_window_available_rejects_a_live_register_above_the_argument_slots() {
    let mut pool = [false; 256];
    pool[9] = true;

    // the callee frame starts at slot 5 and is as long as the callee needs, so the live slot 9
    assert!(!call_window_available(&pool, 10, 5, 0));
    assert!(!call_window_available(&pool, 10, 5, 2));
    assert!(call_window_available(&pool, 10, 10, 0));
}

#[test]
fn call_window_available_ignores_registers_below_the_callee_frame() {
    let mut pool = [false; 256];
    pool[0] = true;
    pool[1] = true;

    assert!(call_window_available(&pool, 2, 2, 0));
    assert!(call_window_available(&pool, 2, 2, 4));
}
