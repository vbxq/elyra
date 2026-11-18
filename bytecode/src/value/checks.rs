use super::{
    CHAR_FLAG, QNAN, TAG_BOOL, TAG_INT, TAG_MASK, TAG_NESTED_FN, TAG_NONE, TAG_NULL, TAG_PTR,
    TAG_UNIT, Value,
};

impl Value {
    // floats are the only values that don't have QNAN set (except actual NaN which we canonicalize)
    // Canonical NaN (TAG_NAN) is also a float. TAG_NESTED_FN is not a float.
    pub fn is_float(&self) -> bool {
        if (self.0 & QNAN) != QNAN {
            return true;
        }
        let tag = self.0 & TAG_MASK;
        tag != TAG_PTR
            && tag != TAG_INT
            && tag != TAG_BOOL
            && tag != TAG_NULL
            && tag != TAG_UNIT
            && tag != TAG_NONE
            && tag != TAG_NESTED_FN
    }

    pub fn is_int(&self) -> bool {
        (self.0 & (QNAN | TAG_MASK)) == (QNAN | TAG_INT)
    }
    // only meaningful on a value statically typed as an integer: inline ints carry this bit, boxed ones do not
    #[inline(always)]
    pub fn has_inline_int_bit(&self) -> bool {
        self.0 & TAG_INT != 0
    }
    #[inline(always)]
    pub fn both_inline_int_bits(&self, other: &Self) -> bool {
        self.0 & other.0 & TAG_INT != 0
    }
    pub fn is_bool(&self) -> bool {
        (self.0 & (QNAN | TAG_MASK)) == (QNAN | TAG_BOOL)
    }
    pub fn is_null(&self) -> bool {
        (self.0 & (QNAN | TAG_MASK | CHAR_FLAG)) == (QNAN | TAG_NULL)
    }
    pub fn is_char(&self) -> bool {
        (self.0 & (QNAN | TAG_MASK | CHAR_FLAG)) == (QNAN | TAG_NULL | CHAR_FLAG)
    }
    #[inline(always)]
    pub fn is_uninit(&self) -> bool {
        self.0 == (QNAN | TAG_NULL | 1)
    }
    pub fn is_ptr(&self) -> bool {
        (self.0 & (QNAN | TAG_MASK)) == (QNAN | TAG_PTR)
    }
}
