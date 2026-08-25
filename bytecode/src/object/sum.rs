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

#[derive(Debug, Clone)]
pub struct AelysEnum {
    pub enum_id: u16,
    pub variant_id: u16,
    pub slot_count: u16,
    pub slots: Box<[Value]>,
}

impl AelysEnum {
    pub fn try_new(enum_id: u16, variant_id: u16, slots: Vec<Value>) -> Option<Self> {
        let slot_count = u16::try_from(slots.len()).ok()?;
        Some(Self {
            enum_id,
            variant_id,
            slot_count,
            slots: slots.into_boxed_slice(),
        })
    }

    pub fn new(enum_id: u16, variant_id: u16, slots: Vec<Value>) -> Self {
        Self::try_new(enum_id, variant_id, slots).expect("enum payload exceeds u16 slot limit")
    }

    pub fn size_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.slots.len() * std::mem::size_of::<Value>()
    }
}
