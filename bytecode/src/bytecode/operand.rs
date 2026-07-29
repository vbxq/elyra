use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperandRangeError {
    kind: &'static str,
    value: usize,
}

impl OperandRangeError {
    pub fn kind(self) -> &'static str {
        self.kind
    }

    pub fn value(self) -> usize {
        self.value
    }
}

impl fmt::Display for OperandRangeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} value {} is out of range",
            self.kind, self.value
        )
    }
}

impl std::error::Error for OperandRangeError {}

macro_rules! unsigned_operand {
    ($name:ident, $inner:ty, $kind:literal) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name($inner);

        impl $name {
            pub const fn new(value: $inner) -> Self {
                Self(value)
            }

            pub const fn get(self) -> $inner {
                self.0
            }
        }

        impl TryFrom<usize> for $name {
            type Error = OperandRangeError;

            fn try_from(value: usize) -> Result<Self, Self::Error> {
                <$inner>::try_from(value)
                    .map(Self)
                    .map_err(|_| OperandRangeError { kind: $kind, value })
            }
        }

        impl From<$name> for $inner {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}

unsigned_operand!(Register, u16, "register");
unsigned_operand!(Arity, u16, "arity");
unsigned_operand!(ConstantIndex, u32, "constant index");
unsigned_operand!(GlobalIndex, u32, "global index");

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JumpOffset(i32);

impl JumpOffset {
    pub const fn new(value: i32) -> Self {
        Self(value)
    }

    pub const fn get(self) -> i32 {
        self.0
    }
}
