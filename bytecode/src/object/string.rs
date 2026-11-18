use std::cell::{Cell, UnsafeCell};
use std::hash::{Hash, Hasher};
use std::rc::Rc;

struct SharedBytes(UnsafeCell<Vec<u8>>);

// immutable string view over a buffer that later concatenations may extend past the view
pub struct AelysString {
    hash: Cell<u64>,
    buffer: Rc<SharedBytes>,
    len: usize,
    owned_bytes: usize,
    interned: Cell<bool>,
}

impl AelysString {
    pub fn new(s: &str) -> Self {
        Self::from_bytes(s.as_bytes().to_vec())
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        let len = bytes.len();
        Self {
            hash: Cell::new(0),
            buffer: Rc::new(SharedBytes(UnsafeCell::new(bytes))),
            len,
            owned_bytes: len,
            interned: Cell::new(false),
        }
    }

    // the caller holds the heap mutably, so no slice of any string sharing this buffer is alive
    pub(crate) fn concat(left: &Self, right: &Self) -> Self {
        let total = left.len + right.len;
        if Rc::ptr_eq(&left.buffer, &right.buffer) || right.len == 0 || left.len == 0 {
            let mut bytes = Vec::with_capacity(total);
            bytes.extend_from_slice(left.as_bytes());
            bytes.extend_from_slice(right.as_bytes());
            return Self::from_bytes(bytes);
        }
        let tip = unsafe { (*left.buffer.0.get()).len() == left.len };
        if !tip {
            let mut bytes = Vec::with_capacity(total.saturating_mul(2));
            bytes.extend_from_slice(left.as_bytes());
            bytes.extend_from_slice(right.as_bytes());
            let mut result = Self::from_bytes(bytes);
            result.owned_bytes = total;
            return result;
        }
        // the right operand lives in another buffer, so growing this one cannot move its bytes
        unsafe { (*left.buffer.0.get()).extend_from_slice(right.as_bytes()) };
        Self {
            hash: Cell::new(0),
            buffer: Rc::clone(&left.buffer),
            len: total,
            owned_bytes: right.len,
            interned: Cell::new(false),
        }
    }

    pub fn hash(&self) -> u64 {
        let cached = self.hash.get();
        if cached != 0 {
            return cached;
        }
        let computed = Self::compute_hash(self.as_bytes()).max(1);
        self.hash.set(computed);
        computed
    }

    pub fn as_bytes(&self) -> &[u8] {
        unsafe { &(&*self.buffer.0.get())[..self.len] }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn accounted_bytes(&self) -> usize {
        self.owned_bytes
    }

    pub(crate) fn mark_interned(&self) {
        self.interned.set(true);
    }

    pub(crate) fn is_interned(&self) -> bool {
        self.interned.get()
    }

    // SAFETY: we only store valid utf-8
    pub fn as_str(&self) -> &str {
        unsafe { std::str::from_utf8_unchecked(self.as_bytes()) }
    }

    fn compute_hash(bytes: &[u8]) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        bytes.hash(&mut h);
        h.finish()
    }
}

impl Clone for AelysString {
    fn clone(&self) -> Self {
        Self::from_bytes(self.as_bytes().to_vec())
    }
}

impl std::fmt::Debug for AelysString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("AelysString").field(&self.as_str()).finish()
    }
}

impl PartialEq for AelysString {
    fn eq(&self, other: &Self) -> bool {
        if self.len != other.len {
            return false;
        }
        let (left, right) = (self.hash.get(), other.hash.get());
        if left != 0 && right != 0 && left != right {
            return false;
        }
        self.as_bytes() == other.as_bytes()
    }
}

impl Eq for AelysString {}

impl Hash for AelysString {
    fn hash<H: Hasher>(&self, state: &mut H) {
        AelysString::hash(self).hash(state);
    }
}
