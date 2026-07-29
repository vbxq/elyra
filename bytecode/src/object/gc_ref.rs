#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GcRef {
    index: u32,
    generation: u16,
}

impl GcRef {
    const INDEX_MASK: u64 = 0xFFFF_FFFF;

    pub fn new(raw: usize) -> Self {
        assert!(raw <= 0x0000_FFFF_FFFF_FFFF, "GC handle is out of range");
        let raw = u64::try_from(raw).expect("validated GC handle fits u64");
        Self {
            index: u32::try_from(raw & Self::INDEX_MASK).expect("GC index is masked to 32 bits"),
            generation: u16::try_from((raw >> 32) & u64::from(u16::MAX))
                .expect("GC generation is masked to 16 bits"),
        }
    }

    pub fn index(self) -> usize {
        usize::try_from(self.raw()).expect("GC handle fits target usize")
    }

    pub(crate) fn from_parts(index: u32, generation: u16) -> Self {
        Self { index, generation }
    }

    pub(crate) fn slot_index(self) -> usize {
        usize::try_from(self.index).expect("u32 index fits target usize")
    }

    pub(crate) fn generation(self) -> u16 {
        self.generation
    }

    pub(crate) fn raw(self) -> u64 {
        u64::from(self.index) | (u64::from(self.generation) << 32)
    }
}

impl From<usize> for GcRef {
    fn from(raw: usize) -> Self {
        Self::new(raw)
    }
}

impl From<GcRef> for usize {
    fn from(gc_ref: GcRef) -> Self {
        gc_ref.index()
    }
}
