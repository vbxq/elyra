use crate::value::Value;
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MapKey {
    Scalar(u64),
    Text(Box<[u8]>),
}

impl MapKey {
    fn heap_bytes(&self) -> usize {
        match self {
            Self::Scalar(_) => 0,
            Self::Text(bytes) => bytes.len(),
        }
    }
}

#[derive(Default)]
pub struct KeyHasher(u64);

impl Hasher for KeyHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        let mut h = self.0 ^ 0xcbf2_9ce4_8422_2325;
        for &b in bytes {
            h = (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
        }
        self.0 = h;
    }

    fn write_u64(&mut self, value: u64) {
        let mixed = (self.0 ^ value).wrapping_mul(0x9e37_79b9_7f4a_7c15);
        self.0 = mixed ^ (mixed >> 29);
    }

    fn write_usize(&mut self, value: usize) {
        self.write_u64(value as u64);
    }
}

type KeyIndex = HashMap<MapKey, usize, BuildHasherDefault<KeyHasher>>;

// insertion-ordered: removals leave a hole that a later compaction closes
#[derive(Debug, Clone, Default)]
pub struct AelysMap {
    entries: Vec<Option<(Value, Value)>>,
    index: KeyIndex,
    key_bytes: usize,
    accounted: usize,
}

impl AelysMap {
    pub fn new() -> Self {
        let mut map = Self::default();
        map.accounted = map.measure();
        map
    }

    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    pub fn get(&self, key: &MapKey) -> Option<Value> {
        let position = *self.index.get(key)?;
        self.entries[position].map(|(_, value)| value)
    }

    pub fn contains_key(&self, key: &MapKey) -> bool {
        self.index.contains_key(key)
    }

    pub fn insert(&mut self, key: MapKey, key_value: Value, value: Value) -> Option<Value> {
        if let Some(&position) = self.index.get(&key) {
            let slot = self.entries[position]
                .as_mut()
                .expect("an indexed entry is live");
            return Some(std::mem::replace(&mut slot.1, value));
        }
        self.key_bytes += key.heap_bytes();
        self.index.insert(key, self.entries.len());
        self.entries.push(Some((key_value, value)));
        None
    }

    pub fn remove(&mut self, key: &MapKey) -> Option<Value> {
        let position = self.index.remove(key)?;
        self.key_bytes -= key.heap_bytes();
        let (_, value) = self.entries[position].take()?;
        if self.entries.len() > 16 && self.index.len() * 2 < self.entries.len() {
            self.compact();
        }
        Some(value)
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.index.clear();
        self.key_bytes = 0;
    }

    pub fn entries(&self) -> impl Iterator<Item = (Value, Value)> + '_ {
        self.entries.iter().flatten().copied()
    }

    pub fn measure(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.entries.capacity() * std::mem::size_of::<Option<(Value, Value)>>()
            + self.index.capacity() * (std::mem::size_of::<MapKey>() + 16)
            + self.key_bytes
    }

    pub fn accounted_bytes(&self) -> usize {
        self.accounted
    }

    pub fn set_accounted_bytes(&mut self, bytes: usize) {
        self.accounted = bytes;
    }

    fn compact(&mut self) {
        self.entries.retain(Option::is_some);
        let mut by_position: Vec<(usize, MapKey)> = self
            .index
            .drain()
            .map(|(key, position)| (position, key))
            .collect();
        by_position.sort_unstable_by_key(|(position, _)| *position);
        for (position, (_, key)) in by_position.into_iter().enumerate() {
            self.index.insert(key, position);
        }
    }
}
