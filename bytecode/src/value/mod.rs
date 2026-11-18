#![doc = include_str!("docs.md")]

mod accessors;
mod checks;
mod constructors;
mod fmt;
mod util;

#[derive(Debug, Clone)]
pub struct IntegerOverflowError {
    pub value: i64,
}

// NaN-boxing: all values fit in 64 bits. Floats are stored directly,
// everything else uses the NaN space (quiet NaN has 51 bits of payload).
// This approach is used by LuaJIT, JavaScriptCore, etc. - proven fast.
//
// an integer outside the 48-bit payload lives in a heap BoxedInt; every integer has exactly one encoding
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct Value(u64);

// Tag bits are in bits 48-50, payload in bits 0-47
const QNAN: u64 = 0x7FF8_0000_0000_0000;
const TAG_MASK: u64 = 0x0007_0000_0000_0000;
const TAG_PTR: u64 = 0x0000_0000_0000_0000;
const TAG_INT: u64 = 0x0001_0000_0000_0000;
const TAG_BOOL: u64 = 0x0002_0000_0000_0000;
const TAG_NULL: u64 = 0x0003_0000_0000_0000;
const TAG_NAN: u64 = 0x0004_0000_0000_0000;
const TAG_NESTED_FN: u64 = 0x0005_0000_0000_0000; // marker for nested functions in constants
const TAG_UNIT: u64 = 0x0006_0000_0000_0000;
const TAG_NONE: u64 = 0x0007_0000_0000_0000;
const PAYLOAD_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
// a char shares the null tag and is told apart by the top payload bit, so every other tag check stays one mask
const CHAR_FLAG: u64 = 0x0000_8000_0000_0000;
const CANONICAL_NAN: u64 = QNAN | TAG_NAN | 1;

impl Value {
    pub const INLINE_INT_MIN: i64 = -(1i64 << 47);
    pub const INLINE_INT_MAX: i64 = (1i64 << 47) - 1;
}
