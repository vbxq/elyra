pub(super) fn arg_range_available(register_pool: &[bool], start: u16, args_len: usize) -> bool {
    for i in 0..args_len {
        let Some(offset) = u16::try_from(i).ok() else {
            return false;
        };
        let arg_reg = match start.checked_add(offset) {
            Some(r) => r,
            None => return false,
        };
        if (arg_reg as usize) >= register_pool.len() || register_pool[arg_reg as usize] {
            return false;
        }
    }
    true
}
