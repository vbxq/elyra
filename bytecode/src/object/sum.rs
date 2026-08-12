use crate::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SumTag {
    OptionSome = 0,
    ResultOk = 1,
    ResultErr = 2,
    ErrorMessage = 3,
}

impl SumTag {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::OptionSome),
            1 => Some(Self::ResultOk),
            2 => Some(Self::ResultErr),
            3 => Some(Self::ErrorMessage),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct AelysSum {
    pub tag: SumTag,
    pub payload: Value,
}

impl AelysSum {
    pub fn new(tag: SumTag, payload: Value) -> Self {
        Self { tag, payload }
    }
}
