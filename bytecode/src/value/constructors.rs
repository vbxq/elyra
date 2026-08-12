use super::{
    CANONICAL_NAN, IntegerOverflowError, PAYLOAD_MASK, QNAN, TAG_BOOL, TAG_INT, TAG_NESTED_FN,
    TAG_NONE, TAG_NULL, TAG_PTR, TAG_UNIT, Value,
};

impl Value {
    #[inline(always)]
    /// Construct an integer, panicking when it cannot be represented by the
    /// 48-bit NaN-box payload. Use [`Self::int_checked`] for host input.
    pub fn int(n: i64) -> Self {
        Self::int_checked(n).unwrap_or_else(|_| {
            panic!(
                "integer {n} is outside the supported range {}..={}",
                Self::INT_MIN,
                Self::INT_MAX
            )
        })
    }

    #[inline(always)]
    /// Construct an integer without panicking on out-of-range host data.
    pub fn int_checked(n: i64) -> Result<Self, IntegerOverflowError> {
        if (Self::INT_MIN..=Self::INT_MAX).contains(&n) {
            let bits = u64::from_ne_bytes(n.to_ne_bytes());
            Ok(Self(QNAN | TAG_INT | (bits & PAYLOAD_MASK)))
        } else {
            Err(IntegerOverflowError { value: n })
        }
    }

    #[inline(always)]
    pub fn int_wrapping(n: i64) -> Self {
        let bits = u64::from_ne_bytes(n.to_ne_bytes());
        Self(QNAN | TAG_INT | (bits & PAYLOAD_MASK))
    }

    pub fn float(n: f64) -> Self {
        let bits = n.to_bits();
        if bits & 0x7FFF_FFFF_FFFF_FFFF > 0x7FF0_0000_0000_0000 {
            Self(CANONICAL_NAN)
        } else {
            Self(bits)
        }
    }

    pub fn bool(b: bool) -> Self {
        Self(QNAN | TAG_BOOL | u64::from(b))
    }

    pub fn null() -> Self {
        Self(QNAN | TAG_NULL)
    }

    pub fn unit() -> Self {
        Self(QNAN | TAG_UNIT)
    }

    pub fn none() -> Self {
        Self(QNAN | TAG_NONE)
    }

    pub fn ptr(p: usize) -> Self {
        let payload_limit = usize::try_from(PAYLOAD_MASK).unwrap_or(usize::MAX);
        assert!(p <= payload_limit, "ptr too big for NaN boxing");
        Self(QNAN | TAG_PTR | u64::try_from(p).expect("pointer payload fits u64"))
    }

    /// create a nested function marker for use in constants array.
    /// this uses a dedicated tag that can't collide with heap pointers.
    pub fn nested_fn_marker(idx: usize) -> Self {
        let payload_limit = usize::try_from(PAYLOAD_MASK).unwrap_or(usize::MAX);
        assert!(idx <= payload_limit, "nested fn index too big");
        Self(QNAN | TAG_NESTED_FN | u64::try_from(idx).expect("nested function payload fits u64"))
    }
}
