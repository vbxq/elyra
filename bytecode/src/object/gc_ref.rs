#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GcRef {
    index: u32,
    generation: u32,
}

impl GcRef {
    const INDEX_BITS: u32 = 24;
    const INDEX_MASK: u64 = (1u64 << Self::INDEX_BITS) - 1;
    pub(crate) const MAX_GENERATION: u32 = (1u32 << (48 - Self::INDEX_BITS)) - 1;

    pub fn new(raw: usize) -> Self {
        assert!(raw <= 0x0000_FFFF_FFFF_FFFF, "GC handle is out of range");
        let raw = u64::try_from(raw).expect("validated GC handle fits u64");
        Self {
            index: u32::try_from(raw & Self::INDEX_MASK).expect("GC index is masked to 32 bits"),
            generation: u32::try_from(raw >> Self::INDEX_BITS)
                .expect("GC generation is masked to 24 bits"),
        }
    }

    pub fn index(self) -> usize {
        usize::try_from(self.raw()).expect("GC handle fits target usize")
    }

    pub(crate) fn from_parts(index: u32, generation: u32) -> Self {
        assert!(
            u64::from(index) <= Self::INDEX_MASK,
            "GC index exceeds 24 bits"
        );
        assert!(
            generation <= Self::MAX_GENERATION,
            "GC generation exceeds 24 bits"
        );
        Self { index, generation }
    }

    pub(crate) fn slot_index(self) -> usize {
        usize::try_from(self.index).expect("u32 index fits target usize")
    }

    pub(crate) fn generation(self) -> u32 {
        self.generation
    }

    pub(crate) fn raw(self) -> u64 {
        u64::from(self.index) | (u64::from(self.generation) << Self::INDEX_BITS)
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
