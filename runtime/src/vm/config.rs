use std::fmt;

#[derive(Debug, Clone)]
pub struct VmConfig {
    pub max_heap_bytes: u64,
    pub gc_stats: bool,
    pub gc_nursery_bytes: u64,
    pub gc_stress: bool,
}

impl VmConfig {
    pub const DEFAULT_MAX_HEAP_BYTES: u64 = 4 * 1024 * 1024 * 1024;
    pub const MIN_HEAP_BYTES: u64 = 1024 * 1024;

    pub const DEFAULT_GC_NURSERY_BYTES: u64 = aelys_bytecode::Heap::DEFAULT_NURSERY_BYTES;
    pub const MIN_GC_NURSERY_BYTES: u64 = aelys_bytecode::Heap::MIN_NURSERY_BYTES;

    pub fn new(max_heap_bytes: u64) -> Result<Self, VmConfigError> {
        let config = Self {
            max_heap_bytes,
            ..Self::default()
        };
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
        if self.gc_nursery_bytes < Self::MIN_GC_NURSERY_BYTES {
            return Err(VmConfigError::NurseryTooSmall {
                value: self.gc_nursery_bytes,
                min: Self::MIN_GC_NURSERY_BYTES,
            });
        }
        Ok(())
    }
}

impl Default for VmConfig {
    fn default() -> Self {
        Self {
            max_heap_bytes: Self::DEFAULT_MAX_HEAP_BYTES,
            gc_stats: false,
            gc_nursery_bytes: Self::DEFAULT_GC_NURSERY_BYTES,
            gc_stress: false,
        }
    }
}

#[derive(Debug, Clone)]
pub enum VmConfigError {
    MaxHeapTooSmall { value: u64, min: u64 },
    NurseryTooSmall { value: u64, min: u64 },
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
            VmConfigError::NurseryTooSmall { value, min } => {
                write!(
                    f,
                    "gc nursery too small: {} bytes (minimum {} bytes)",
                    value, min
                )
            }
        }
    }
}
