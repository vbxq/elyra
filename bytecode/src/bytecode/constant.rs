use crate::{Heap, Value};

#[derive(Debug, Clone, PartialEq)]
pub enum Constant {
    Null,
    Bool(bool),
    Int(i64),
    Float(u64),
    String(String),
    NestedFunction(u32),
}

impl Constant {
    pub fn from_value(value: Value) -> Option<Self> {
        if value.is_null() {
            Some(Self::Null)
        } else if let Some(value) = value.as_bool() {
            Some(Self::Bool(value))
        } else if let Some(value) = value.as_int() {
            Some(Self::Int(value))
        } else if let Some(value) = value.as_float() {
            Some(Self::Float(value.to_bits()))
        } else {
            value
                .as_nested_fn_marker()
                .and_then(|index| u32::try_from(index).ok())
                .map(Self::NestedFunction)
        }
    }

    pub fn materialize(&self, heap: &mut Heap) -> Value {
        match self {
            Self::Null => Value::null(),
            Self::Bool(value) => Value::bool(*value),
            Self::Int(value) => Value::int(*value),
            Self::Float(bits) => Value::float(f64::from_bits(*bits)),
            Self::String(value) => Value::ptr(heap.intern_string(value).index()),
            Self::NestedFunction(index) => Value::nested_fn_marker(*index as usize),
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            Self::Int(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_float(&self) -> Option<f64> {
        match self {
            Self::Float(bits) => Some(f64::from_bits(*bits)),
            _ => None,
        }
    }

    pub fn as_string(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_nested_fn_marker(&self) -> Option<usize> {
        match self {
            Self::NestedFunction(index) => Some(*index as usize),
            _ => None,
        }
    }
}

impl From<Value> for Constant {
    fn from(value: Value) -> Self {
        Self::from_value(value).expect("heap references cannot be stored in structural constants")
    }
}
