use std::fmt;

#[derive(Debug, Clone)]
pub struct VmConfig {
    pub max_heap_bytes: u64,
}

impl VmConfig {
    pub const DEFAULT_MAX_HEAP_BYTES: u64 = 4 * 1024 * 1024 * 1024;
    pub const MIN_HEAP_BYTES: u64 = 1024 * 1024;

    pub fn new(max_heap_bytes: u64) -> Result<Self, VmConfigError> {
        let config = Self { max_heap_bytes };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), VmConfigError> {
        if self.max_heap_bytes < Self::MIN_HEAP_BYTES {
            return Err(VmConfigError::MaxHeapTooSmall {
                value: self.max_heap_bytes,
                min: Self::MIN_HEAP_BYTES,
            });
        }
        Ok(())
    }
}

impl Default for VmConfig {
    fn default() -> Self {
        Self {
            max_heap_bytes: Self::DEFAULT_MAX_HEAP_BYTES,
        }
    }
}

#[derive(Debug, Clone)]
pub enum VmConfigError {
    MaxHeapTooSmall { value: u64, min: u64 },
}

impl fmt::Display for VmConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VmConfigError::MaxHeapTooSmall { value, min } => {
                write!(
                    f,
                    "max heap too small: {} bytes (minimum {} bytes)",
                    value, min
                )
            }
        }
    }
}
