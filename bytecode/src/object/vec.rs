use super::array::{AelysArray, ArrayData, TypeTag};
use crate::value::Value;

macro_rules! each_variant {
    ($data:expr, $v:ident => $body:expr) => {
        match $data {
            VecData::Ints($v) => $body,
            VecData::Floats($v) => $body,
            VecData::Bools($v) => $body,
            VecData::Objects($v) => $body,
            VecData::Bytes($v) => $body,
        }
    };
}

#[derive(Debug, Clone)]
pub enum VecData {
    Ints(Vec<i64>),
    Floats(Vec<f64>),
    Bools(Vec<u8>),
    Objects(Vec<Value>),
    Bytes(Vec<u8>),
}

impl VecData {
    pub fn len(&self) -> usize {
        match self {
            Self::Ints(v) => v.len(),
            Self::Floats(v) => v.len(),
            Self::Bools(v) => v.len(),
            Self::Objects(v) => v.len(),
            Self::Bytes(v) => v.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn capacity(&self) -> usize {
        match self {
            Self::Ints(v) => v.capacity(),
            Self::Floats(v) => v.capacity(),
            Self::Bools(v) => v.capacity(),
            Self::Objects(v) => v.capacity(),
            Self::Bytes(v) => v.capacity(),
        }
    }

    pub fn type_tag(&self) -> TypeTag {
        match self {
            Self::Ints(_) => TypeTag::Int,
            Self::Floats(_) => TypeTag::Float,
            Self::Bools(_) => TypeTag::Bool,
            Self::Objects(_) => TypeTag::Object,
            Self::Bytes(_) => TypeTag::Byte,
        }
    }

    pub fn as_ints(&self) -> Option<&[i64]> {
        match self {
            Self::Ints(v) => Some(v),
            _ => None,
        }
    }
    pub fn as_ints_mut(&mut self) -> Option<&mut Vec<i64>> {
        match self {
            Self::Ints(v) => Some(v),
            _ => None,
        }
    }
    pub fn as_floats(&self) -> Option<&[f64]> {
        match self {
            Self::Floats(v) => Some(v),
            _ => None,
        }
    }
    pub fn as_floats_mut(&mut self) -> Option<&mut Vec<f64>> {
        match self {
            Self::Floats(v) => Some(v),
            _ => None,
        }
    }
    pub fn as_bools(&self) -> Option<&[u8]> {
        match self {
            Self::Bools(v) => Some(v),
            _ => None,
        }
    }
    pub fn as_bools_mut(&mut self) -> Option<&mut Vec<u8>> {
        match self {
            Self::Bools(v) => Some(v),
            _ => None,
        }
    }
    pub fn as_objects(&self) -> Option<&[Value]> {
        match self {
            Self::Objects(v) => Some(v),
            _ => None,
        }
    }
    pub fn as_objects_mut(&mut self) -> Option<&mut Vec<Value>> {
        match self {
            Self::Objects(v) => Some(v),
            _ => None,
        }
    }
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Bytes(v) => Some(v),
            _ => None,
        }
    }
    pub fn as_bytes_mut(&mut self) -> Option<&mut Vec<u8>> {
        match self {
            Self::Bytes(v) => Some(v),
            _ => None,
        }
    }
}

fn byte_of(value: Value) -> Option<u8> {
    value.as_int().and_then(|int| u8::try_from(int).ok())
}

#[derive(Debug, Clone)]
pub struct AelysVec {
    pub data: VecData,
    accounted_capacity: usize,
    accounted_tag: TypeTag,
}

impl AelysVec {
    fn accounted(data: VecData, accounted_capacity: usize) -> Self {
        let accounted_tag = data.type_tag();
        Self {
            data,
            accounted_capacity,
            accounted_tag,
        }
    }

    pub fn new_ints() -> Self {
        Self::accounted(VecData::Ints(Vec::new()), 0)
    }
    pub fn new_floats() -> Self {
        Self::accounted(VecData::Floats(Vec::new()), 0)
    }
    pub fn new_bools() -> Self {
        Self::accounted(VecData::Bools(Vec::new()), 0)
    }
    pub fn new_objects() -> Self {
        Self::accounted(VecData::Objects(Vec::new()), 0)
    }
    pub fn new_bytes() -> Self {
        Self::accounted(VecData::Bytes(Vec::new()), 0)
    }

    pub fn with_capacity_ints(cap: usize) -> Self {
        Self::accounted(VecData::Ints(Vec::with_capacity(cap)), cap)
    }
    pub fn with_capacity_floats(cap: usize) -> Self {
        Self::accounted(VecData::Floats(Vec::with_capacity(cap)), cap)
    }
    pub fn with_capacity_bools(cap: usize) -> Self {
        Self::accounted(VecData::Bools(Vec::with_capacity(cap)), cap)
    }
    pub fn with_capacity_objects(cap: usize) -> Self {
        Self::accounted(VecData::Objects(Vec::with_capacity(cap)), cap)
    }

    pub fn from_ints(data: Vec<i64>) -> Self {
        let accounted_capacity = data.len();
        Self::accounted(VecData::Ints(data), accounted_capacity)
    }
    pub fn from_floats(data: Vec<f64>) -> Self {
        let accounted_capacity = data.len();
        Self::accounted(VecData::Floats(data), accounted_capacity)
    }
    pub fn from_bools(data: Vec<bool>) -> Self {
        let data: Vec<u8> = data.iter().copied().map(u8::from).collect();
        let accounted_capacity = data.len();
        Self::accounted(VecData::Bools(data), accounted_capacity)
    }
    pub fn from_objects(data: Vec<Value>) -> Self {
        let accounted_capacity = data.len();
        Self::accounted(VecData::Objects(data), accounted_capacity)
    }
    pub fn from_bytes(data: Vec<u8>) -> Self {
        let accounted_capacity = data.len();
        Self::accounted(VecData::Bytes(data), accounted_capacity)
    }

    pub fn from_array(array: &AelysArray) -> Self {
        match &array.data {
            ArrayData::Ints(values) => Self::from_ints(values.to_vec()),
            ArrayData::Floats(values) => Self::from_floats(values.to_vec()),
            ArrayData::Bools(values) => {
                Self::accounted(VecData::Bools(values.to_vec()), values.len())
            }
            ArrayData::Objects(values) => Self::from_objects(values.to_vec()),
        }
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn slice(&self, start: usize, end: usize) -> Self {
        let data = match &self.data {
            VecData::Ints(values) => VecData::Ints(values[start..end].to_vec()),
            VecData::Floats(values) => VecData::Floats(values[start..end].to_vec()),
            VecData::Bools(values) => VecData::Bools(values[start..end].to_vec()),
            VecData::Objects(values) => VecData::Objects(values[start..end].to_vec()),
            VecData::Bytes(values) => VecData::Bytes(values[start..end].to_vec()),
        };
        let accounted_capacity = data.len();
        Self::accounted(data, accounted_capacity)
    }

    pub fn capacity(&self) -> usize {
        self.data.capacity()
    }

    pub fn accepts(&self, value: Value) -> bool {
        match &self.data {
            VecData::Ints(_) | VecData::Bytes(_) => value.as_int().is_some(),
            VecData::Floats(_) => value.as_float().is_some(),
            VecData::Bools(_) => value.as_bool().is_some(),
            VecData::Objects(_) => true,
        }
    }
    pub fn type_tag(&self) -> TypeTag {
        self.data.type_tag()
    }

    // packed ints only ever hold inline-range values; a boxed int turns the storage into objects first
    pub fn promote_ints_to_objects(&mut self) {
        match &self.data {
            VecData::Ints(values) => {
                let mut objects = Vec::with_capacity(values.capacity());
                objects.extend(values.iter().map(|value| Value::int(*value)));
                self.data = VecData::Objects(objects);
            }
            VecData::Bytes(values) => {
                let mut objects = Vec::with_capacity(values.capacity());
                objects.extend(values.iter().map(|value| Value::int(i64::from(*value))));
                self.data = VecData::Objects(objects);
            }
            _ => {}
        }
    }

    pub fn widen_bytes_to_ints(&mut self) {
        if let VecData::Bytes(values) = &self.data {
            let mut ints = Vec::with_capacity(values.capacity());
            ints.extend(values.iter().map(|value| i64::from(*value)));
            self.data = VecData::Ints(ints);
        }
    }

    pub fn pack_bytes(&mut self) -> bool {
        let packed: Option<Vec<u8>> = match &self.data {
            VecData::Bytes(_) => return true,
            VecData::Ints(values) => values
                .iter()
                .map(|value| u8::try_from(*value).ok())
                .collect(),
            VecData::Objects(values) => values.iter().map(|value| byte_of(*value)).collect(),
            VecData::Floats(values) if values.is_empty() => Some(Vec::new()),
            VecData::Bools(values) if values.is_empty() => Some(Vec::new()),
            _ => None,
        };
        match packed {
            Some(bytes) => {
                self.data = VecData::Bytes(bytes);
                self.accounted_tag = TypeTag::Byte;
                true
            }
            None => false,
        }
    }

    pub fn get(&self, index: usize) -> Option<Value> {
        if index >= self.len() {
            return None;
        }
        Some(match &self.data {
            VecData::Ints(v) => Value::int(v[index]),
            VecData::Floats(v) => Value::float(v[index]),
            VecData::Bools(v) => Value::bool(v[index] != 0),
            VecData::Objects(v) => v[index],
            VecData::Bytes(v) => Value::int(i64::from(v[index])),
        })
    }

    pub fn set(&mut self, index: usize, value: Value) -> bool {
        if index >= self.len() {
            return false;
        }
        match &mut self.data {
            VecData::Ints(v) => {
                if let Some(val) = value.as_int() {
                    v[index] = val;
                    true
                } else {
                    false
                }
            }
            VecData::Floats(v) => {
                if let Some(val) = value.as_float() {
                    v[index] = val;
                    true
                } else {
                    false
                }
            }
            VecData::Bools(v) => {
                if let Some(val) = value.as_bool() {
                    v[index] = u8::from(val);
                    true
                } else {
                    false
                }
            }
            VecData::Objects(v) => {
                v[index] = value;
                true
            }
            VecData::Bytes(v) => match byte_of(value) {
                Some(byte) => {
                    v[index] = byte;
                    true
                }
                None if value.as_int().is_some() => {
                    self.widen_bytes_to_ints();
                    self.set(index, value)
                }
                None => false,
            },
        }
    }

    pub fn push(&mut self, value: Value) -> bool {
        let pushed = match &mut self.data {
            VecData::Ints(v) => {
                if let Some(val) = value.as_int() {
                    v.push(val);
                    true
                } else {
                    false
                }
            }
            VecData::Floats(v) => {
                if let Some(val) = value.as_float() {
                    v.push(val);
                    true
                } else {
                    false
                }
            }
            VecData::Bools(v) => {
                if let Some(val) = value.as_bool() {
                    v.push(u8::from(val));
                    true
                } else {
                    false
                }
            }
            VecData::Objects(v) => {
                v.push(value);
                true
            }
            VecData::Bytes(v) => match byte_of(value) {
                Some(byte) => {
                    v.push(byte);
                    true
                }
                None if value.as_int().is_some() => {
                    self.widen_bytes_to_ints();
                    return self.push(value);
                }
                None => false,
            },
        };
        if pushed {
            self.accounted_capacity = self.accounted_capacity.max(self.len());
        }
        pushed
    }

    pub fn pop(&mut self) -> Option<Value> {
        match &mut self.data {
            VecData::Ints(v) => v.pop().map(Value::int),
            VecData::Floats(v) => v.pop().map(Value::float),
            VecData::Bools(v) => v.pop().map(|b| Value::bool(b != 0)),
            VecData::Objects(v) => v.pop(),
            VecData::Bytes(v) => v.pop().map(|b| Value::int(i64::from(b))),
        }
    }

    pub fn insert(&mut self, index: usize, value: Value) -> bool {
        if index > self.len() || !self.push(value) {
            return false;
        }
        each_variant!(&mut self.data, v => v[index..].rotate_right(1));
        true
    }

    pub fn remove(&mut self, index: usize) -> Option<Value> {
        let removed = self.get(index)?;
        each_variant!(&mut self.data, v => {
            v.remove(index);
        });
        Some(removed)
    }

    pub fn swap_remove(&mut self, index: usize) -> Option<Value> {
        let removed = self.get(index)?;
        each_variant!(&mut self.data, v => {
            v.swap_remove(index);
        });
        Some(removed)
    }

    pub fn truncate(&mut self, len: usize) {
        each_variant!(&mut self.data, v => v.truncate(len));
    }

    pub fn swap(&mut self, a: usize, b: usize) {
        each_variant!(&mut self.data, v => v.swap(a, b));
    }

    pub fn reverse(&mut self) {
        each_variant!(&mut self.data, v => v.reverse());
    }

    pub fn reserve(&mut self, additional: usize) {
        let _ = self.try_reserve(additional);
    }

    pub fn try_reserve(&mut self, additional: usize) -> bool {
        let target = self.len().checked_add(additional);
        let reserved = match &mut self.data {
            VecData::Ints(v) => v.try_reserve(additional).is_ok(),
            VecData::Floats(v) => v.try_reserve(additional).is_ok(),
            VecData::Bools(v) => v.try_reserve(additional).is_ok(),
            VecData::Objects(v) => v.try_reserve(additional).is_ok(),
            VecData::Bytes(v) => v.try_reserve(additional).is_ok(),
        };
        if reserved && let Some(target) = target {
            self.accounted_capacity = self.accounted_capacity.max(target);
            self.accounted_tag = self.type_tag();
        }
        reserved
    }

    pub fn try_reserve_exact(&mut self, additional: usize) -> bool {
        let Some(target) = self.len().checked_add(additional) else {
            return false;
        };
        let reserved = match &mut self.data {
            VecData::Ints(v) => v.try_reserve_exact(additional).is_ok(),
            VecData::Floats(v) => v.try_reserve_exact(additional).is_ok(),
            VecData::Bools(v) => v.try_reserve_exact(additional).is_ok(),
            VecData::Objects(v) => v.try_reserve_exact(additional).is_ok(),
            VecData::Bytes(v) => v.try_reserve_exact(additional).is_ok(),
        };
        if reserved {
            self.accounted_capacity = self.accounted_capacity.max(target);
            self.accounted_tag = self.type_tag();
        }
        reserved
    }

    pub fn clear(&mut self) {
        match &mut self.data {
            VecData::Ints(v) => v.clear(),
            VecData::Floats(v) => v.clear(),
            VecData::Bools(v) => v.clear(),
            VecData::Objects(v) => v.clear(),
            VecData::Bytes(v) => v.clear(),
        }
    }

    pub fn shrink_to_fit(&mut self) {
        match &mut self.data {
            VecData::Ints(v) => v.shrink_to_fit(),
            VecData::Floats(v) => v.shrink_to_fit(),
            VecData::Bools(v) => v.shrink_to_fit(),
            VecData::Objects(v) => v.shrink_to_fit(),
            VecData::Bytes(v) => v.shrink_to_fit(),
        }
        self.accounted_capacity = self.capacity();
        self.accounted_tag = self.type_tag();
    }

    pub fn to_array(&self) -> AelysArray {
        AelysArray {
            data: match &self.data {
                VecData::Ints(v) => ArrayData::Ints(v.clone().into_boxed_slice()),
                VecData::Floats(v) => ArrayData::Floats(v.clone().into_boxed_slice()),
                VecData::Bools(v) => ArrayData::Bools(v.clone().into_boxed_slice()),
                VecData::Objects(v) => ArrayData::Objects(v.clone().into_boxed_slice()),
                VecData::Bytes(v) => ArrayData::Ints(v.iter().map(|b| i64::from(*b)).collect()),
            },
        }
    }

    pub fn objects(&self) -> Option<&[Value]> {
        match &self.data {
            VecData::Objects(v) => Some(v),
            _ => None,
        }
    }

    pub fn size_bytes_for(type_tag: TypeTag, capacity: usize) -> Option<usize> {
        let element_size = match type_tag {
            TypeTag::Int | TypeTag::Float | TypeTag::Object => 8,
            TypeTag::Bool | TypeTag::Byte => 1,
        };
        std::mem::size_of::<Self>().checked_add(capacity.checked_mul(element_size)?)
    }

    pub fn try_size_bytes(&self) -> Option<usize> {
        Self::size_bytes_for(self.accounted_tag, self.accounted_capacity)
    }

    pub fn size_bytes(&self) -> usize {
        self.try_size_bytes().unwrap_or(usize::MAX)
    }
}
