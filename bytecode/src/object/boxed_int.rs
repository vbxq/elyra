#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoxedInt {
    bits: u64,
    unsigned: bool,
}

impl BoxedInt {
    pub fn new(value: i128) -> Option<Self> {
        if let Ok(signed) = i64::try_from(value) {
            Some(Self {
                bits: signed as u64,
                unsigned: false,
            })
        } else {
            u64::try_from(value).ok().map(|bits| Self {
                bits,
                unsigned: true,
            })
        }
    }

    #[inline]
    pub fn value(self) -> i128 {
        if self.unsigned {
            i128::from(self.bits)
        } else {
            i128::from(self.bits as i64)
        }
    }
}
