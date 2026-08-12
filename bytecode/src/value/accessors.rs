use super::{PAYLOAD_MASK, QNAN, TAG_MASK, TAG_NESTED_FN, Value};

impl Value {
    pub fn as_int(&self) -> Option<i64> {
        if !self.is_int() {
            return None;
        }
        let payload = self.0 & PAYLOAD_MASK;
        Some(i64::from_ne_bytes((payload << 16).to_ne_bytes()) >> 16) // sign-extend from 48 bits
    }

    pub fn as_float(&self) -> Option<f64> {
        self.is_float().then(|| f64::from_bits(self.0))
    }

    pub fn as_bool(&self) -> Option<bool> {
        self.is_bool().then_some((self.0 & 1) != 0)
    }

    pub fn is_unit(&self) -> bool {
        (self.0 & (QNAN | TAG_MASK)) == (QNAN | super::TAG_UNIT)
    }

    pub fn is_none(&self) -> bool {
        (self.0 & (QNAN | TAG_MASK)) == (QNAN | super::TAG_NONE)
    }

    pub fn as_ptr(&self) -> Option<usize> {
        self.is_ptr().then(|| {
            usize::try_from(self.0 & PAYLOAD_MASK).expect("pointer payload fits target usize")
        })
    }

    /// Check if this is a nested function marker and return the index if so.
    pub fn as_nested_fn_marker(&self) -> Option<usize> {
        if (self.0 & (QNAN | TAG_MASK)) == (QNAN | TAG_NESTED_FN) {
            Some(
                usize::try_from(self.0 & PAYLOAD_MASK)
                    .expect("nested function payload fits target usize"),
            )
        } else {
            None
        }
    }

    // unchecked variants for type-specialized opcodes (hot paths)
    #[inline(always)]
    pub fn as_int_unchecked(&self) -> i64 {
        debug_assert!(self.is_int(), "type confusion: not an int");
        i64::from_ne_bytes(((self.0 & PAYLOAD_MASK) << 16).to_ne_bytes()) >> 16
    }

    #[inline(always)]
    pub fn as_float_unchecked(&self) -> f64 {
        debug_assert!(self.is_float(), "type confusion: not a float");
        f64::from_bits(self.0)
    }
}
