// NaN-boxing helpers for native modules
// mirrors aelys-runtime/src/vm/value.rs

use crate::{AelysValue, NativeHandle};

const QNAN: u64 = 0x7FF8_0000_0000_0000;
const TAG_INT: u64 = 0x0001_0000_0000_0000;
const TAG_BOOL: u64 = 0x0002_0000_0000_0000;
const TAG_NULL: u64 = 0x0003_0000_0000_0000;
const TAG_NAN: u64 = 0x0004_0000_0000_0000;
const PAYLOAD_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

pub fn value_null() -> AelysValue {
    AelysValue::from_bits(QNAN | TAG_NULL)
}

pub fn value_bool(b: bool) -> AelysValue {
    AelysValue::from_bits(QNAN | TAG_BOOL | u64::from(b))
}

pub fn value_int(n: i64) -> Option<AelysValue> {
    const INT_MIN: i64 = -(1i64 << 47);
    const INT_MAX: i64 = (1i64 << 47) - 1;
    if !(INT_MIN..=INT_MAX).contains(&n) {
        return None;
    }
    let payload = u64::from_ne_bytes(n.to_ne_bytes()) & PAYLOAD_MASK;
    Some(AelysValue::from_bits(QNAN | TAG_INT | payload))
}

pub fn value_float(n: f64) -> AelysValue {
    if n.is_nan() {
        AelysValue::from_bits(QNAN | TAG_NAN | 1)
    } else {
        AelysValue::from_bits(n.to_bits())
    }
}

pub fn value_as_int(v: AelysValue) -> i64 {
    let v = v.bits();
    if (v & (QNAN | 0x0007_0000_0000_0000)) == (QNAN | TAG_INT) {
        let payload = v & PAYLOAD_MASK;
        if payload & 0x0000_8000_0000_0000 != 0 {
            i64::from_ne_bytes((payload | 0xFFFF_0000_0000_0000).to_ne_bytes())
        } else {
            i64::try_from(payload).expect("positive integer payload fits i64")
        }
    } else {
        0
    }
}

pub fn value_as_float(v: AelysValue) -> f64 {
    let v = v.bits();
    if (v & QNAN) == QNAN {
        if (v & (QNAN | 0x0007_0000_0000_0000)) == (QNAN | TAG_NAN) {
            return f64::NAN;
        }
        return 0.0;
    }
    f64::from_bits(v)
}

pub fn value_as_bool(v: AelysValue) -> bool {
    let v = v.bits();
    if (v & (QNAN | 0x0007_0000_0000_0000)) == (QNAN | TAG_BOOL) {
        (v & 1) != 0
    } else {
        false
    }
}

pub fn value_is_null(v: AelysValue) -> bool {
    let v = v.bits();
    (v & (QNAN | 0x0007_0000_0000_0000)) == (QNAN | TAG_NULL)
}
pub fn value_is_int(v: AelysValue) -> bool {
    let v = v.bits();
    (v & (QNAN | 0x0007_0000_0000_0000)) == (QNAN | TAG_INT)
}
pub fn value_is_float(v: AelysValue) -> bool {
    let v = v.bits();
    (v & QNAN) != QNAN || (v & (QNAN | 0x0007_0000_0000_0000)) == (QNAN | TAG_NAN)
}
pub fn value_is_bool(v: AelysValue) -> bool {
    let v = v.bits();
    (v & (QNAN | 0x0007_0000_0000_0000)) == (QNAN | TAG_BOOL)
}
pub fn value_is_ptr(v: AelysValue) -> bool {
    let v = v.bits();
    (v & (QNAN | 0x0007_0000_0000_0000)) == QNAN
}
pub fn value_as_handle(v: AelysValue) -> Option<NativeHandle> {
    value_is_ptr(v).then(|| NativeHandle::from_payload(v.bits() & PAYLOAD_MASK))
}

pub fn value_from_handle(handle: NativeHandle) -> AelysValue {
    AelysValue::from_bits(QNAN | handle.payload())
}
