use std::fmt;

use super::config::{VmConfig, VmConfigError};

mod parse;

pub use parse::parse_vm_args;

pub struct VmArgsParsed {
    pub config: VmConfig,
    pub program_args: Vec<String>,
    pub max_instructions: Option<u64>,
    pub timeout_ms: Option<u64>,
    /// off unless asked for: the compiler turns down any function that calls an Elyra function, divides, takes a modulus or touches a struct
    pub jit: bool,
}

#[derive(Debug)]
pub enum VmArgsError {
    UnknownArgument(String),
    MissingValue(String),
    InvalidValue {
        arg: String,
        value: String,
        reason: String,
    },
    InvalidConfig(VmConfigError),
}

impl fmt::Display for VmArgsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VmArgsError::UnknownArgument(arg) => {
                write!(f, "unknown VM argument: {}", arg)
            }
            VmArgsError::MissingValue(arg) => {
                write!(f, "missing value for VM argument: {}", arg)
            }
            VmArgsError::InvalidValue { arg, value, reason } => {
                write!(f, "invalid value for {}: '{}' ({})", arg, value, reason)
            }
            VmArgsError::InvalidConfig(err) => write!(f, "invalid VM configuration: {}", err),
        }
    }
}
