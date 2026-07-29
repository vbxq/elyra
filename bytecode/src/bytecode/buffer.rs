use std::sync::Arc;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BytecodeBuffer(Arc<[u32]>);

impl BytecodeBuffer {
    pub fn new(data: Box<[u32]>) -> Self {
        Self(Arc::from(data))
    }

    pub fn empty() -> Self {
        Self::default()
    }

    pub fn from_vec(data: Vec<u32>) -> Self {
        Self(Arc::from(data))
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[inline(always)]
    pub fn as_ptr(&self) -> *const u32 {
        self.0.as_ptr()
    }

    #[inline(always)]
    pub fn read(&self, offset: usize) -> u32 {
        self.0[offset]
    }

    pub fn as_slice(&self) -> &[u32] {
        &self.0
    }

    pub fn iter(&self) -> impl Iterator<Item = &u32> {
        self.0.iter()
    }
}

impl std::ops::Index<usize> for BytecodeBuffer {
    type Output = u32;

    fn index(&self, index: usize) -> &Self::Output {
        &self.0[index]
    }
}

impl From<Vec<u32>> for BytecodeBuffer {
    fn from(data: Vec<u32>) -> Self {
        Self::from_vec(data)
    }
}

impl From<Arc<[u32]>> for BytecodeBuffer {
    fn from(data: Arc<[u32]>) -> Self {
        Self(data)
    }
}
