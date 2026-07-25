use super::super::config::VmConfig;
use super::{VmArgsError, VmArgsParsed};

pub fn parse_vm_args(args: &[String]) -> Result<VmArgsParsed, VmArgsError> {
    let mut config = VmConfig::default();
    let mut program_args = Vec::new();

    for arg in args {
        if let Some(value) = arg.strip_prefix("-ae.") {
            apply_vm_arg(value, arg, &mut config)?;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--ae-") {
            apply_vm_arg(value, arg, &mut config)?;
            continue;
        }
        program_args.push(arg.clone());
    }

    config.validate().map_err(VmArgsError::InvalidConfig)?;

    Ok(VmArgsParsed {
        config,
        program_args,
    })
}

fn apply_vm_arg(
    value: &str,
    raw_arg: &str,
    config: &mut VmConfig,
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
        _ => Err(VmArgsError::UnknownArgument(raw_arg.to_string())),
    }
}

fn parse_size_bytes(value: &str, arg: &str) -> Result<u64, VmArgsError> {
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

    if bytes < VmConfig::MIN_HEAP_BYTES {
        return Err(VmArgsError::InvalidValue {
            arg: arg.to_string(),
            value: value.to_string(),
            reason: format!("must be >= {} bytes", VmConfig::MIN_HEAP_BYTES),
        });
    }

    Ok(bytes)
}
