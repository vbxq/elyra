// callglobal, callcached and callupval base the callee frame at the caller's dst + 1 and give it
pub fn call_window_available(
    register_pool: &[bool],
    live_high_water: u32,
    arg_start: u16,
    args_len: usize,
) -> bool {
    let start = usize::from(arg_start);
    if start >= register_pool.len() {
        return false;
    }
    let Some(arg_end) = start.checked_add(args_len) else {
        return false;
    };
    if arg_end > register_pool.len() {
        return false;
    }
    let high_water = usize::try_from(live_high_water).unwrap_or(register_pool.len());
    let scan_end = arg_end.max(high_water).min(register_pool.len());
    !register_pool[start..scan_end].iter().any(|used| *used)
}
