use super::super::config::VmConfig;
use super::{VmArgsError, VmArgsParsed};

pub fn parse_vm_args(args: &[String]) -> Result<VmArgsParsed, VmArgsError> {
    let mut config = VmConfig::default();
    let mut program_args = Vec::new();
    let mut max_instructions = None;
    let mut timeout_ms = None;
    let mut jit = None;
    let mut jit_sync = false;

    for arg in args {
        if let Some(value) = arg.strip_prefix("-ae.") {
            apply_vm_arg(
                value,
                arg,
                &mut config,
                &mut max_instructions,
                &mut timeout_ms,
                &mut jit,
                &mut jit_sync,
            )?;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--ae-") {
            apply_vm_arg(
                value,
                arg,
                &mut config,
                &mut max_instructions,
                &mut timeout_ms,
                &mut jit,
                &mut jit_sync,
            )?;
            continue;
        }
        program_args.push(arg.clone());
    }

    config.validate().map_err(VmArgsError::InvalidConfig)?;

    if jit_sync && jit == Some(false) {
        return Err(VmArgsError::InvalidValue {
            arg: "-ae.jit-sync".to_string(),
            value: "on".to_string(),
            reason: "synchronous compilation needs the jit, which -ae.jit=off turns off"
                .to_string(),
        });
    }
    let jit = jit.unwrap_or(jit_sync);

    Ok(VmArgsParsed {
        config,
        program_args,
        max_instructions,
        timeout_ms,
        jit,
        jit_sync,
    })
}

fn apply_vm_arg(
    value: &str,
    raw_arg: &str,
    config: &mut VmConfig,
    max_instructions: &mut Option<u64>,
    timeout_ms: &mut Option<u64>,
    jit: &mut Option<bool>,
    jit_sync: &mut bool,
) -> Result<(), VmArgsError> {
    let (key, raw_value) = value
        .split_once('=')
        .ok_or_else(|| VmArgsError::MissingValue(raw_arg.to_string()))?;

    match key {
        "max-heap" => {
            let bytes = parse_size_bytes(raw_value, raw_arg)?;
            config.max_heap_bytes = bytes;
            Ok(())
        }
        "gc-stats" => {
            config.gc_stats = parse_switch(raw_value, raw_arg)?;
            Ok(())
        }
        "gc-stress" => {
            config.gc_stress = parse_switch(raw_value, raw_arg)?;
            Ok(())
        }
        "gc-verify" => {
            config.gc_verify = parse_switch(raw_value, raw_arg)?;
            Ok(())
        }
        "gc-nursery" => {
            let bytes = parse_size_value(raw_value, raw_arg)?;
            if bytes < VmConfig::MIN_GC_NURSERY_BYTES {
                return Err(VmArgsError::InvalidValue {
                    arg: raw_arg.to_string(),
                    value: raw_value.to_string(),
                    reason: format!("must be >= {} bytes", VmConfig::MIN_GC_NURSERY_BYTES),
                });
            }
            config.gc_nursery_bytes = bytes;
            Ok(())
        }
        "max-instructions" => {
            *max_instructions = Some(parse_u64(raw_value, raw_arg)?);
            Ok(())
        }
        "timeout-ms" => {
            *timeout_ms = Some(parse_u64(raw_value, raw_arg)?);
            Ok(())
        }
        "jit" => {
            *jit = Some(parse_switch(raw_value, raw_arg)?);
            Ok(())
        }
        "jit-sync" => {
            *jit_sync = parse_switch(raw_value, raw_arg)?;
            Ok(())
        }
        _ => Err(VmArgsError::UnknownArgument(raw_arg.to_string())),
    }
}

fn parse_switch(value: &str, arg: &str) -> Result<bool, VmArgsError> {
    match value {
        "on" | "true" | "1" => Ok(true),
        "off" | "false" | "0" => Ok(false),
        _ => Err(VmArgsError::InvalidValue {
            arg: arg.to_string(),
            value: value.to_string(),
            reason: "expected on or off".to_string(),
        }),
    }
}

fn parse_u64(value: &str, arg: &str) -> Result<u64, VmArgsError> {
    value.parse().map_err(|_| VmArgsError::InvalidValue {
        arg: arg.to_string(),
        value: value.to_string(),
        reason: "expected an unsigned integer".to_string(),
    })
}

fn parse_size_bytes(value: &str, arg: &str) -> Result<u64, VmArgsError> {
    let bytes = parse_size_value(value, arg)?;
    if bytes < VmConfig::MIN_HEAP_BYTES {
        return Err(VmArgsError::InvalidValue {
            arg: arg.to_string(),
            value: value.to_string(),
            reason: format!("must be >= {} bytes", VmConfig::MIN_HEAP_BYTES),
        });
    }
    Ok(bytes)
}

fn parse_size_value(value: &str, arg: &str) -> Result<u64, VmArgsError> {
    if value.is_empty() {
        return Err(VmArgsError::InvalidValue {
            arg: arg.to_string(),
            value: value.to_string(),
            reason: "empty size".to_string(),
        });
    }

    let (number_str, suffix) = match value.chars().last() {
        Some(c) if c.is_ascii_alphabetic() => (&value[..value.len() - 1], Some(c)),
        _ => (value, None),
    };

    if number_str.is_empty() {
        return Err(VmArgsError::InvalidValue {
            arg: arg.to_string(),
            value: value.to_string(),
            reason: "missing numeric value".to_string(),
        });
    }

    if number_str.starts_with('-') {
        return Err(VmArgsError::InvalidValue {
            arg: arg.to_string(),
            value: value.to_string(),
            reason: "negative sizes are not allowed".to_string(),
        });
    }

    let number: u64 = number_str.parse().map_err(|_| VmArgsError::InvalidValue {
        arg: arg.to_string(),
        value: value.to_string(),
        reason: "invalid integer".to_string(),
    })?;

    let multiplier = match suffix {
        None => 1u64,
        Some('K') | Some('k') => 1024u64,
        Some('M') | Some('m') => 1024u64 * 1024u64,
        Some('G') | Some('g') => 1024u64 * 1024u64 * 1024u64,
        Some(_) => {
            return Err(VmArgsError::InvalidValue {
                arg: arg.to_string(),
                value: value.to_string(),
                reason: "invalid size suffix (use K, M, or G)".to_string(),
            });
        }
    };

    let bytes = number
        .checked_mul(multiplier)
        .ok_or_else(|| VmArgsError::InvalidValue {
            arg: arg.to_string(),
            value: value.to_string(),
            reason: "size overflows u64".to_string(),
        })?;

    Ok(bytes)
}
