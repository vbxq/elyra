pub(super) fn check_reg(reg: usize, num_regs: usize, op: &str) -> Result<(), String> {
    if reg >= num_regs {
        return Err(format!(
            "{} uses register {} out of bounds (max {})",
            op,
            reg,
            num_regs.saturating_sub(1)
        ));
    }
    Ok(())
}

pub(super) fn check_reg_range(
    base: usize,
    count: usize,
    num_regs: usize,
    op: &str,
) -> Result<(), String> {
    if count == 0 {
        return Ok(());
    }
    let last = base
        .checked_add(count - 1)
        .ok_or_else(|| format!("{} register range overflow at base {}", op, base))?;
    if last >= num_regs {
        return Err(format!(
            "{} uses registers r{}..r{} but only {} registers available",
            op, base, last, num_regs
        ));
    }
    Ok(())
}

pub(super) fn check_const_index(idx: usize, constants_len: usize, op: &str) -> Result<(), String> {
    if idx >= constants_len {
        return Err(format!(
            "{} uses constant {} out of bounds (max {})",
            op,
            idx,
            constants_len.saturating_sub(1)
        ));
    }
    Ok(())
}

pub(super) fn check_upval_index(idx: usize, upvalues_len: usize, op: &str) -> Result<(), String> {
    if idx >= upvalues_len {
        return Err(format!(
            "{} uses upvalue {} out of bounds (max {})",
            op,
            idx,
            upvalues_len.saturating_sub(1)
        ));
    }
    Ok(())
}

pub(super) fn check_call_args(
    start_reg: usize,
    nargs: usize,
    num_regs: usize,
    op: &str,
) -> Result<(), String> {
    if nargs == 0 {
        return Ok(());
    }
    let last = start_reg
        .checked_add(nargs)
        .ok_or_else(|| format!("{} argument overflow", op))?;
    if last >= num_regs {
        return Err(format!(
            "{} uses args through r{} out of bounds (max {})",
            op,
            last,
            num_regs.saturating_sub(1)
        ));
    }
    Ok(())
}

pub(super) fn check_jump(ip: usize, offset: i16, bc_len: usize, op: &str) -> Result<usize, String> {
    let next_ip = ip
        .checked_add(1)
        .ok_or_else(|| format!("{} ip overflow", op))?;
    let next_ip = i64::try_from(next_ip).map_err(|_| format!("{} ip exceeds i64", op))?;
    let target = next_ip
        .checked_add(i64::from(offset))
        .ok_or_else(|| format!("{} jump overflow", op))?;
    let Ok(target_index) = usize::try_from(target) else {
        return Err(format!("{} jump target {} out of bounds", op, target));
    };
    if target_index > bc_len {
        return Err(format!("{} jump target {} out of bounds", op, target));
    }
    Ok(target_index)
}

pub(super) fn check_jump_i32(
    ip: usize,
    offset: i32,
    bc_len: usize,
    op: &str,
) -> Result<usize, String> {
    let next_ip = ip
        .checked_add(2)
        .ok_or_else(|| format!("{} ip overflow", op))?;
    let next_ip = i64::try_from(next_ip).map_err(|_| format!("{} ip exceeds i64", op))?;
    let target = next_ip
        .checked_add(i64::from(offset))
        .ok_or_else(|| format!("{} jump overflow", op))?;
    let Ok(target_index) = usize::try_from(target) else {
        return Err(format!("{} jump target {} out of bounds", op, target));
    };
    if target_index > bc_len {
        return Err(format!("{} jump target {} out of bounds", op, target));
    }
    Ok(target_index)
}
