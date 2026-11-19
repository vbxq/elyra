use super::roots::HostRoot;
use super::{GcRef, ObjectKind, VM, Value};
use aelys_bytecode::object::{AelysMap, AelysVec, MapKey, SumTag, TypeTag};
use aelys_common::error::{RuntimeError, RuntimeErrorKind};
use std::cmp::Ordering;

type Native = fn(&mut VM, &[Value]) -> Result<Value, RuntimeError>;

pub(crate) const COLLECTION_NATIVES: &[(&str, u16, Native)] = &[
    ("__vec_insert", 3, vec_insert),
    ("__vec_remove", 2, vec_remove),
    ("__vec_swap_remove", 2, vec_swap_remove),
    ("__vec_clear", 1, vec_clear),
    ("__vec_extend", 2, vec_extend),
    ("__vec_append", 2, vec_append),
    ("__vec_truncate", 2, vec_truncate),
    ("__vec_resize", 3, vec_resize),
    ("__vec_swap", 3, seq_swap),
    ("__vec_reverse", 1, seq_reverse),
    ("__vec_contains", 2, seq_contains),
    ("__vec_index_of", 2, seq_index_of),
    ("__vec_sort", 1, seq_sort),
    ("__vec_sort_by_begin", 1, sort_by_begin),
    ("__vec_sort_by_step", 2, sort_by_step),
    ("__vec_sort_by_finish", 2, sort_by_finish),
    ("__vec_first", 1, seq_first),
    ("__vec_last", 1, seq_last),
    ("__vec_set", 3, seq_set),
    ("__vec_fill", 2, seq_fill),
    ("__vec_join", 2, seq_join),
    ("__map_new", 0, map_new),
    ("__map_insert", 3, map_insert),
    ("__map_get", 2, map_get),
    ("__map_index", 2, map_index),
    ("__map_contains_key", 2, map_contains_key),
    ("__map_remove", 2, map_remove),
    ("__map_len", 1, map_len),
    ("__map_is_empty", 1, map_is_empty),
    ("__map_clear", 1, map_clear),
    ("__map_keys", 2, map_keys),
    ("__map_values", 2, map_values),
    ("__string_push", 2, string_push),
    ("__char_from_int", 1, char_from_int),
    ("__char_to_int", 1, char_to_int),
    ("__char_is_digit", 1, char_is_digit),
    ("__char_is_alpha", 1, char_is_alpha),
    ("__char_is_whitespace", 1, char_is_whitespace),
    ("__char_is_alphanumeric", 1, char_is_alphanumeric),
    ("__char_to_upper", 1, char_to_upper),
    ("__char_to_lower", 1, char_to_lower),
];

fn type_error(
    vm: &VM,
    operation: &'static str,
    expected: &'static str,
    got: Value,
) -> RuntimeError {
    vm.runtime_error(RuntimeErrorKind::TypeError {
        operation,
        expected,
        got: vm.value_type_name(got).to_string(),
    })
}

fn object(vm: &VM, value: Value, operation: &'static str) -> Result<GcRef, RuntimeError> {
    value
        .as_ptr()
        .map(GcRef::new)
        .ok_or_else(|| type_error(vm, operation, "collection", value))
}

fn int_arg(vm: &VM, value: Value, operation: &'static str) -> Result<i64, RuntimeError> {
    vm.int64_value(value)
        .ok_or_else(|| type_error(vm, operation, "int", value))
}

fn out_of_bounds(vm: &VM, index: i64, length: usize) -> RuntimeError {
    vm.runtime_error(RuntimeErrorKind::IndexOutOfBounds {
        index: i128::from(index),
        length: i64::try_from(length).unwrap_or(i64::MAX),
    })
}

fn checked_index(vm: &VM, index: i64, length: usize) -> Result<usize, RuntimeError> {
    usize::try_from(index)
        .ok()
        .filter(|position| *position < length)
        .ok_or_else(|| out_of_bounds(vm, index, length))
}

fn some(vm: &mut VM, value: Value) -> Result<Value, RuntimeError> {
    let sum = vm.alloc_sum(SumTag::OptionSome, value)?;
    Ok(Value::ptr(sum.index()))
}

fn sequence_len(vm: &VM, sequence: GcRef, operation: &'static str) -> Result<usize, RuntimeError> {
    match vm.heap.get(sequence).map(|object| &object.kind) {
        Some(ObjectKind::Vec(vector)) => Ok(vector.len()),
        Some(ObjectKind::Array(array)) => Ok(array.len()),
        _ => Err(vm.runtime_error(RuntimeErrorKind::TypeError {
            operation,
            expected: "vec or array",
            got: "another object".to_string(),
        })),
    }
}

fn sequence_values(
    vm: &VM,
    sequence: GcRef,
    operation: &'static str,
) -> Result<Vec<Value>, RuntimeError> {
    match vm.heap.get(sequence).map(|object| &object.kind) {
        Some(ObjectKind::Vec(vector)) => Ok((0..vector.len())
            .filter_map(|index| vector.get(index))
            .collect()),
        Some(ObjectKind::Array(array)) => Ok((0..array.len())
            .filter_map(|index| array.get(index))
            .collect()),
        _ => Err(vm.runtime_error(RuntimeErrorKind::TypeError {
            operation,
            expected: "vec or array",
            got: "another object".to_string(),
        })),
    }
}

fn sequence_get(vm: &VM, sequence: GcRef, index: usize) -> Option<Value> {
    match vm.heap.get(sequence).map(|object| &object.kind) {
        Some(ObjectKind::Vec(vector)) => vector.get(index),
        Some(ObjectKind::Array(array)) => array.get(index),
        _ => None,
    }
}

fn write_sequence(
    vm: &mut VM,
    sequence: GcRef,
    values: &[Value],
    operation: &'static str,
) -> Result<(), RuntimeError> {
    for value in values {
        vm.admit_boxed_int(sequence, *value);
    }
    let written = match vm.heap.get_mut(sequence).map(|object| &mut object.kind) {
        Some(ObjectKind::Vec(vector)) => values
            .iter()
            .enumerate()
            .all(|(index, value)| vector.set(index, *value)),
        Some(ObjectKind::Array(array)) => values
            .iter()
            .enumerate()
            .all(|(index, value)| array.set(index, *value)),
        _ => false,
    };
    vm.heap.write_barrier_elements(sequence, 0..values.len());
    if written {
        Ok(())
    } else {
        Err(vm.runtime_error(RuntimeErrorKind::TypeError {
            operation,
            expected: "matching element type",
            got: "incompatible element".to_string(),
        }))
    }
}

fn with_vec<T>(
    vm: &mut VM,
    vector: GcRef,
    operation: &'static str,
    body: impl FnOnce(&mut AelysVec) -> T,
) -> Result<T, RuntimeError> {
    match vm.heap.get_mut(vector).map(|object| &mut object.kind) {
        Some(ObjectKind::Vec(inner)) => Ok(body(inner)),
        _ => Err(vm.runtime_error(RuntimeErrorKind::TypeError {
            operation,
            expected: "vec",
            got: "another object".to_string(),
        })),
    }
}

fn vec_insert(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let vector = object(vm, args[0], "vec insert")?;
    let index = int_arg(vm, args[1], "vec insert")?;
    let length = sequence_len(vm, vector, "vec insert")?;
    let position = usize::try_from(index)
        .ok()
        .filter(|position| *position <= length)
        .ok_or_else(|| out_of_bounds(vm, index, length))?;
    vm.reserve_vec(vector, 1)?;
    vm.admit_boxed_int(vector, args[2]);
    if with_vec(vm, vector, "vec insert", |inner| {
        inner.insert(position, args[2])
    })? {
        vm.heap.write_barrier_elements(vector, position..length + 1);
        Ok(Value::unit())
    } else {
        Err(type_error(
            vm,
            "vec insert",
            "matching element type",
            args[2],
        ))
    }
}

fn vec_remove(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let vector = object(vm, args[0], "vec remove")?;
    let index = int_arg(vm, args[1], "vec remove")?;
    let length = sequence_len(vm, vector, "vec remove")?;
    let position = checked_index(vm, index, length)?;
    let removed = with_vec(vm, vector, "vec remove", |inner| inner.remove(position))?
        .ok_or_else(|| out_of_bounds(vm, index, length))?;
    vm.heap.write_barrier_elements(vector, position..length);
    Ok(removed)
}

fn vec_swap_remove(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let vector = object(vm, args[0], "vec swap_remove")?;
    let index = int_arg(vm, args[1], "vec swap_remove")?;
    let length = sequence_len(vm, vector, "vec swap_remove")?;
    let position = checked_index(vm, index, length)?;
    let removed = with_vec(vm, vector, "vec swap_remove", |inner| {
        inner.swap_remove(position)
    })?
    .ok_or_else(|| out_of_bounds(vm, index, length))?;
    vm.heap
        .write_barrier_elements(vector, position..position + 1);
    Ok(removed)
}

fn vec_clear(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let vector = object(vm, args[0], "vec clear")?;
    with_vec(vm, vector, "vec clear", AelysVec::clear)?;
    Ok(Value::unit())
}

fn push_all(
    vm: &mut VM,
    vector: GcRef,
    values: &[Value],
    operation: &'static str,
) -> Result<(), RuntimeError> {
    vm.reserve_vec(vector, values.len())?;
    for value in values {
        vm.admit_boxed_int(vector, *value);
    }
    let start = sequence_len(vm, vector, operation)?;
    let pushed = with_vec(vm, vector, operation, |inner| {
        values.iter().all(|value| inner.push(*value))
    })?;
    vm.heap
        .write_barrier_elements(vector, start..start + values.len());
    if pushed {
        Ok(())
    } else {
        Err(vm.runtime_error(RuntimeErrorKind::TypeError {
            operation,
            expected: "matching element type",
            got: "incompatible element".to_string(),
        }))
    }
}

fn vec_extend(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let vector = object(vm, args[0], "vec extend")?;
    let source = object(vm, args[1], "vec extend")?;
    let values = sequence_values(vm, source, "vec extend")?;
    push_all(vm, vector, &values, "vec extend")?;
    Ok(Value::unit())
}

fn vec_append(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let vector = object(vm, args[0], "vec append")?;
    let source = object(vm, args[1], "vec append")?;
    let values = sequence_values(vm, source, "vec append")?;
    push_all(vm, vector, &values, "vec append")?;
    if source != vector {
        with_vec(vm, source, "vec append", AelysVec::clear)?;
    }
    Ok(Value::unit())
}

fn vec_truncate(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let vector = object(vm, args[0], "vec truncate")?;
    let length = int_arg(vm, args[1], "vec truncate")?;
    let current = sequence_len(vm, vector, "vec truncate")?;
    let target = usize::try_from(length).map_err(|_| out_of_bounds(vm, length, current))?;
    with_vec(vm, vector, "vec truncate", |inner| inner.truncate(target))?;
    Ok(Value::unit())
}

fn vec_resize(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let vector = object(vm, args[0], "vec resize")?;
    let length = int_arg(vm, args[1], "vec resize")?;
    let current = sequence_len(vm, vector, "vec resize")?;
    let target = usize::try_from(length).map_err(|_| out_of_bounds(vm, length, current))?;
    let packed = matches!(
        vm.heap.get(vector).map(|object| &object.kind),
        Some(ObjectKind::Vec(inner)) if inner.data.as_bytes().is_some()
    );
    if target <= current {
        with_vec(vm, vector, "vec resize", |inner| inner.truncate(target))?;
    } else if packed && let Some(byte) = byte_value(&args[2]) {
        vm.reserve_vec(vector, target - current)?;
        with_vec(vm, vector, "vec resize", |inner| {
            if let Some(bytes) = inner.data.as_bytes_mut() {
                bytes.resize(target, byte);
            }
        })?;
    } else if is_fresh_per_slot(vm, args[2])? {
        vm.reserve_vec(vector, target - current)?;
        for _ in current..target {
            let copy = vm.deep_clone_value(args[2])?;
            let _pin = copy
                .as_ptr()
                .map(|raw| HostRoot::new(&vm.host_roots, GcRef::new(raw), vm.id));
            push_all(vm, vector, &[copy], "vec resize")?;
        }
    } else {
        vm.reserve_vec(vector, target - current)?;
        let fill = [args[2]; 256];
        let mut remaining = target - current;
        while remaining > 0 {
            let count = remaining.min(fill.len());
            push_all(vm, vector, &fill[..count], "vec resize")?;
            remaining -= count;
        }
    }
    Ok(Value::unit())
}

fn seq_swap(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let sequence = object(vm, args[0], "swap")?;
    let first = int_arg(vm, args[1], "swap")?;
    let second = int_arg(vm, args[2], "swap")?;
    let length = sequence_len(vm, sequence, "swap")?;
    let a = checked_index(vm, first, length)?;
    let b = checked_index(vm, second, length)?;
    let left = sequence_get(vm, sequence, a).unwrap_or_default();
    let right = sequence_get(vm, sequence, b).unwrap_or_default();
    match vm.heap.get_mut(sequence).map(|object| &mut object.kind) {
        Some(ObjectKind::Vec(inner)) => inner.swap(a, b),
        Some(ObjectKind::Array(inner)) => {
            inner.set(a, right);
            inner.set(b, left);
        }
        _ => {}
    }
    vm.heap.write_barrier_element(sequence, a, right);
    vm.heap.write_barrier_element(sequence, b, left);
    Ok(Value::unit())
}

fn seq_reverse(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let sequence = object(vm, args[0], "reverse")?;
    let mut values = sequence_values(vm, sequence, "reverse")?;
    values.reverse();
    write_sequence(vm, sequence, &values, "reverse")?;
    Ok(Value::unit())
}

fn seq_contains(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let sequence = object(vm, args[0], "contains")?;
    let values = sequence_values(vm, sequence, "contains")?;
    Ok(Value::bool(
        values.iter().any(|value| vm.values_equal(*value, args[1])),
    ))
}

fn seq_index_of(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let sequence = object(vm, args[0], "index_of")?;
    let values = sequence_values(vm, sequence, "index_of")?;
    match values
        .iter()
        .position(|value| vm.values_equal(*value, args[1]))
    {
        Some(position) => some(vm, Value::int(i64::try_from(position).unwrap_or(i64::MAX))),
        None => Ok(Value::none()),
    }
}

impl VM {
    pub(crate) fn order_values(&self, left: Value, right: Value) -> Option<Ordering> {
        if let (Some(a), Some(b)) = (self.int_value(left), self.int_value(right)) {
            return Some(a.cmp(&b));
        }
        if let (Some(a), Some(b)) = (left.as_float(), right.as_float()) {
            return Some(a.total_cmp(&b));
        }
        if let (Some(a), Some(b)) = (left.as_char(), right.as_char()) {
            return Some(a.cmp(&b));
        }
        if let (Some(a), Some(b)) = (left.as_bool(), right.as_bool()) {
            return Some(a.cmp(&b));
        }
        match (self.string_bytes(left), self.string_bytes(right)) {
            (Some(a), Some(b)) => Some(a.cmp(b)),
            _ => None,
        }
    }

    pub(crate) fn string_bytes(&self, value: Value) -> Option<&[u8]> {
        match &self.heap.get(GcRef::new(value.as_ptr()?))?.kind {
            ObjectKind::String(text) => Some(text.as_bytes()),
            _ => None,
        }
    }

    pub(crate) fn map_key(&self, value: Value) -> Option<MapKey> {
        if let Some(wide) = self.heap.boxed_int(value) {
            return Some(MapKey::Wide(wide));
        }
        if value.as_ptr().is_some() {
            return self
                .string_bytes(value)
                .map(|bytes| MapKey::Text(bytes.into()));
        }
        if value.as_float().is_some() {
            return None;
        }
        Some(MapKey::Scalar(value_bits(value)))
    }
}

fn value_bits(value: Value) -> u64 {
    if let Some(n) = value.as_int() {
        return u64::from_ne_bytes(n.to_ne_bytes()) ^ 0x1000_0000_0000_0000;
    }
    if let Some(c) = value.as_char() {
        return u64::from(u32::from(c)) ^ 0x2000_0000_0000_0000;
    }
    if let Some(b) = value.as_bool() {
        return u64::from(b) ^ 0x3000_0000_0000_0000;
    }
    0x4000_0000_0000_0000
}

fn seq_sort(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let sequence = object(vm, args[0], "sort")?;
    let mut values = sequence_values(vm, sequence, "sort")?;
    if let Some(&first) = values.first()
        && let Some(&bad) = values
            .iter()
            .find(|value| vm.order_values(first, **value).is_none())
    {
        return Err(type_error(
            vm,
            "sort",
            "elements with a built-in order",
            bad,
        ));
    }
    values.sort_by(|left, right| {
        vm.order_values(*left, *right)
            .expect("every element was checked against the first")
    });
    write_sequence(vm, sequence, &values, "sort")?;
    Ok(Value::unit())
}

const SORT_LEFT: usize = 0;
const SORT_RIGHT: usize = 1;
const SORT_N: usize = 2;
const SORT_WIDTH: usize = 3;
const SORT_LO: usize = 4;
const SORT_MID: usize = 5;
const SORT_HI: usize = 6;
const SORT_I: usize = 7;
const SORT_J: usize = 8;
const SORT_K: usize = 9;
const SORT_PENDING: usize = 10;
const SORT_SWAPPED: usize = 11;
const SORT_HEADER: usize = 12;

fn sort_by_begin(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let sequence = object(vm, args[0], "sort_by")?;
    let length = sequence_len(vm, sequence, "sort_by")?;
    let n = i64::try_from(length).unwrap_or(i64::MAX);
    let mut state = vec![0i64; SORT_HEADER + 2 * length];
    state[SORT_N] = n;
    state[SORT_WIDTH] = 1;
    for index in 0..length {
        state[SORT_HEADER + index] = i64::try_from(index).unwrap_or(i64::MAX);
    }
    start_run(&mut state, 0);
    let reference = vm.alloc_vec(AelysVec::from_ints(state))?;
    Ok(Value::ptr(reference.index()))
}

fn start_run(state: &mut [i64], lo: i64) {
    let n = state[SORT_N];
    let width = state[SORT_WIDTH];
    state[SORT_LO] = lo;
    state[SORT_MID] = (lo + width).min(n);
    state[SORT_HI] = (lo + 2 * width).min(n);
    state[SORT_I] = lo;
    state[SORT_J] = state[SORT_MID];
    state[SORT_K] = lo;
}

fn sort_slots(state: &[i64]) -> (usize, usize) {
    let n = usize::try_from(state[SORT_N]).unwrap_or(0);
    if state[SORT_SWAPPED] == 0 {
        (SORT_HEADER, SORT_HEADER + n)
    } else {
        (SORT_HEADER + n, SORT_HEADER)
    }
}

fn at(base: usize, offset: i64) -> usize {
    base + usize::try_from(offset).unwrap_or(0)
}

// one step of a bottom-up merge sort over a permutation, paused at every comparison it needs
fn advance_sort(state: &mut [i64], comparison: i64) -> bool {
    let n = state[SORT_N];
    if n < 2 {
        return false;
    }
    let (mut src, mut dst) = sort_slots(state);
    if state[SORT_PENDING] == 1 {
        state[SORT_PENDING] = 0;
        let k = at(dst, state[SORT_K]);
        if comparison < 0 {
            state[k] = state[at(src, state[SORT_J])];
            state[SORT_J] += 1;
        } else {
            state[k] = state[at(src, state[SORT_I])];
            state[SORT_I] += 1;
        }
        state[SORT_K] += 1;
    }
    loop {
        if state[SORT_LO] < n {
            let (mid, hi) = (state[SORT_MID], state[SORT_HI]);
            if state[SORT_I] < mid && state[SORT_J] < hi {
                state[SORT_LEFT] = state[at(src, state[SORT_J])];
                state[SORT_RIGHT] = state[at(src, state[SORT_I])];
                state[SORT_PENDING] = 1;
                return true;
            }
            while state[SORT_I] < mid {
                state[at(dst, state[SORT_K])] = state[at(src, state[SORT_I])];
                state[SORT_I] += 1;
                state[SORT_K] += 1;
            }
            while state[SORT_J] < hi {
                state[at(dst, state[SORT_K])] = state[at(src, state[SORT_J])];
                state[SORT_J] += 1;
                state[SORT_K] += 1;
            }
            start_run(state, hi);
            continue;
        }
        state[SORT_SWAPPED] ^= 1;
        std::mem::swap(&mut src, &mut dst);
        state[SORT_WIDTH] *= 2;
        if state[SORT_WIDTH] >= n {
            return false;
        }
        start_run(state, 0);
    }
}

fn sort_by_step(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let state = object(vm, args[0], "sort_by")?;
    let comparison = int_arg(vm, args[1], "sort_by comparator")?;
    let more = with_vec(vm, state, "sort_by", |inner| {
        inner
            .data
            .as_ints_mut()
            .map(|slots| advance_sort(slots, comparison))
    })?;
    more.map(Value::bool)
        .ok_or_else(|| type_error(vm, "sort_by", "sort state", args[0]))
}

fn sort_by_finish(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let state = object(vm, args[0], "sort_by")?;
    let sequence = object(vm, args[1], "sort_by")?;
    let order: Vec<usize> = match vm.heap.get(state).map(|object| &object.kind) {
        Some(ObjectKind::Vec(inner)) => {
            let slots = inner.data.as_ints().unwrap_or(&[]);
            let (src, _) = sort_slots(slots);
            let n = usize::try_from(slots[SORT_N]).unwrap_or(0);
            slots[src..src + n]
                .iter()
                .map(|index| usize::try_from(*index).unwrap_or(0))
                .collect()
        }
        _ => return Err(type_error(vm, "sort_by", "sort state", args[0])),
    };
    let values = sequence_values(vm, sequence, "sort_by")?;
    if values.len() != order.len() {
        return Err(vm.runtime_error(RuntimeErrorKind::VecChangedDuringSort));
    }
    let sorted: Vec<Value> = order.iter().map(|index| values[*index]).collect();
    write_sequence(vm, sequence, &sorted, "sort_by")?;
    Ok(Value::unit())
}

fn seq_first(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let sequence = object(vm, args[0], "first")?;
    match sequence_get(vm, sequence, 0) {
        Some(value) => some(vm, value),
        None => Ok(Value::none()),
    }
}

fn seq_last(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let sequence = object(vm, args[0], "last")?;
    let length = sequence_len(vm, sequence, "last")?;
    match length
        .checked_sub(1)
        .and_then(|index| sequence_get(vm, sequence, index))
    {
        Some(value) => some(vm, value),
        None => Ok(Value::none()),
    }
}

fn seq_set(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let sequence = object(vm, args[0], "set")?;
    let index = int_arg(vm, args[1], "set")?;
    let length = sequence_len(vm, sequence, "set")?;
    let position = checked_index(vm, index, length)?;
    vm.admit_boxed_int(sequence, args[2]);
    let written = match vm.heap.get_mut(sequence).map(|object| &mut object.kind) {
        Some(ObjectKind::Vec(inner)) => inner.set(position, args[2]),
        Some(ObjectKind::Array(inner)) => inner.set(position, args[2]),
        _ => false,
    };
    if written {
        vm.heap.write_barrier_element(sequence, position, args[2]);
        Ok(Value::unit())
    } else {
        Err(type_error(vm, "set", "matching element type", args[2]))
    }
}

fn seq_fill(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let sequence = object(vm, args[0], "fill")?;
    if let Some(byte) = byte_value(&args[1])
        && let Some(ObjectKind::Vec(vector)) =
            vm.heap.get_mut(sequence).map(|object| &mut object.kind)
        && let Some(bytes) = vector.data.as_bytes_mut()
    {
        bytes.fill(byte);
        return Ok(Value::unit());
    }
    let length = sequence_len(vm, sequence, "fill")?;
    if is_fresh_per_slot(vm, args[1])? {
        for index in 0..length {
            let copy = vm.deep_clone_value(args[1])?;
            write_sequence_slot(vm, sequence, index, copy, "fill")?;
        }
        return Ok(Value::unit());
    }
    let values = vec![args[1]; length];
    write_sequence(vm, sequence, &values, "fill")?;
    Ok(Value::unit())
}

fn is_fresh_per_slot(vm: &VM, value: Value) -> Result<bool, RuntimeError> {
    let Some(raw) = value.as_ptr() else {
        return Ok(false);
    };
    match vm.heap.get(GcRef::new(raw)).map(|object| &object.kind) {
        Some(
            ObjectKind::Struct(_)
            | ObjectKind::Enum(_)
            | ObjectKind::Sum(_)
            | ObjectKind::Array(_)
            | ObjectKind::Vec(_)
            | ObjectKind::Map(_),
        ) => Ok(true),
        Some(_) => Ok(false),
        None => Err(vm.runtime_error(RuntimeErrorKind::UseAfterFree)),
    }
}

fn write_sequence_slot(
    vm: &mut VM,
    sequence: GcRef,
    index: usize,
    value: Value,
    operation: &'static str,
) -> Result<(), RuntimeError> {
    vm.admit_boxed_int(sequence, value);
    let written = match vm.heap.get_mut(sequence).map(|object| &mut object.kind) {
        Some(ObjectKind::Vec(vector)) => vector.set(index, value),
        Some(ObjectKind::Array(array)) => array.set(index, value),
        _ => false,
    };
    if written {
        vm.heap.write_barrier_element(sequence, index, value);
        Ok(())
    } else {
        Err(vm.runtime_error(RuntimeErrorKind::TypeError {
            operation,
            expected: "matching element type",
            got: "incompatible element".to_string(),
        }))
    }
}

fn seq_join(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let sequence = object(vm, args[0], "join")?;
    let values = sequence_values(vm, sequence, "join")?;
    let separator = vm
        .string_bytes(args[1])
        .ok_or_else(|| type_error(vm, "join", "string", args[1]))?
        .to_vec();
    let mut joined = Vec::new();
    for (position, value) in values.iter().enumerate() {
        if position > 0 {
            joined.extend_from_slice(&separator);
        }
        let bytes = vm
            .string_bytes(*value)
            .ok_or_else(|| type_error(vm, "join", "string element", *value))?;
        joined.extend_from_slice(bytes);
    }
    let text = String::from_utf8(joined).unwrap_or_default();
    let reference = vm.alloc_string(&text)?;
    Ok(Value::ptr(reference.index()))
}

fn map_ref(vm: &VM, value: Value, operation: &'static str) -> Result<GcRef, RuntimeError> {
    let reference = object(vm, value, operation)?;
    match vm.heap.get(reference).map(|object| &object.kind) {
        Some(ObjectKind::Map(_)) => Ok(reference),
        _ => Err(type_error(vm, operation, "map", value)),
    }
}

fn key_of(vm: &VM, value: Value, operation: &'static str) -> Result<MapKey, RuntimeError> {
    vm.map_key(value)
        .ok_or_else(|| type_error(vm, operation, "int, string, char or bool key", value))
}

fn read_map<T>(vm: &VM, map: GcRef, body: impl FnOnce(&AelysMap) -> T) -> T {
    match vm.heap.get(map).map(|object| &object.kind) {
        Some(ObjectKind::Map(inner)) => body(inner),
        _ => unreachable!("map_ref checked the object kind"),
    }
}

fn write_map<T>(vm: &mut VM, map: GcRef, body: impl FnOnce(&mut AelysMap) -> T) -> T {
    let (result, previous, current) = match vm.heap.get_mut(map).map(|object| &mut object.kind) {
        Some(ObjectKind::Map(inner)) => {
            let result = body(inner);
            let previous = inner.accounted_bytes();
            let current = inner.measure();
            inner.set_accounted_bytes(current);
            (result, previous, current)
        }
        _ => unreachable!("map_ref checked the object kind"),
    };
    vm.heap.account_reallocation(previous, current);
    result
}

fn map_new(vm: &mut VM, _args: &[Value]) -> Result<Value, RuntimeError> {
    let map = AelysMap::new();
    let reference = vm.alloc_object(aelys_bytecode::GcObject::new(ObjectKind::Map(Box::new(
        map,
    ))))?;
    Ok(Value::ptr(reference.index()))
}

fn map_insert(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let map = map_ref(vm, args[0], "map insert")?;
    let key = key_of(vm, args[1], "map insert")?;
    let growth = |vm: &VM| {
        let bytes = read_map(vm, map, |inner| inner.insert_growth(&key));
        u64::try_from(bytes).unwrap_or(u64::MAX)
    };
    let predicted = growth(vm);
    if predicted > 0 {
        vm.maybe_collect_for(predicted);
        vm.ensure_heap_capacity(growth(vm))?;
    }
    let previous = write_map(vm, map, |inner| inner.insert(key, args[1], args[2]));
    vm.heap.write_barrier_value(map, args[1]);
    vm.heap.write_barrier_value(map, args[2]);
    match previous {
        Some(previous) => some(vm, previous),
        None => Ok(Value::none()),
    }
}

fn map_get(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let map = map_ref(vm, args[0], "map get")?;
    let key = key_of(vm, args[1], "map get")?;
    match read_map(vm, map, |inner| inner.get(&key)) {
        Some(value) => some(vm, value),
        None => Ok(Value::none()),
    }
}

fn map_index(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let map = map_ref(vm, args[0], "map index")?;
    let key = key_of(vm, args[1], "map index")?;
    read_map(vm, map, |inner| inner.get(&key)).ok_or_else(|| {
        let rendered = match args[1].as_ptr() {
            Some(_) if !vm.is_int_value(args[1]) => format!("{:?}", vm.value_to_string(args[1])),
            Some(_) => vm.value_to_string(args[1]),
            None => match args[1].as_char() {
                Some(c) => format!("{c:?}"),
                None => vm.value_to_string(args[1]),
            },
        };
        vm.runtime_error(RuntimeErrorKind::MapKeyNotFound { key: rendered })
    })
}

fn map_contains_key(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let map = map_ref(vm, args[0], "map contains_key")?;
    let key = key_of(vm, args[1], "map contains_key")?;
    Ok(Value::bool(read_map(vm, map, |inner| {
        inner.contains_key(&key)
    })))
}

fn map_remove(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let map = map_ref(vm, args[0], "map remove")?;
    let key = key_of(vm, args[1], "map remove")?;
    match write_map(vm, map, |inner| inner.remove(&key)) {
        Some(previous) => some(vm, previous),
        None => Ok(Value::none()),
    }
}

fn map_len(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let map = map_ref(vm, args[0], "map len")?;
    let length = read_map(vm, map, AelysMap::len);
    Ok(Value::int(i64::try_from(length).unwrap_or(i64::MAX)))
}

fn map_is_empty(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let map = map_ref(vm, args[0], "map is_empty")?;
    Ok(Value::bool(read_map(vm, map, AelysMap::is_empty)))
}

fn map_clear(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let map = map_ref(vm, args[0], "map clear")?;
    write_map(vm, map, AelysMap::clear);
    Ok(Value::unit())
}

fn typed_vec(tag: TypeTag, values: Vec<Value>) -> AelysVec {
    match tag {
        TypeTag::Int if values.iter().all(Value::is_int) => {
            AelysVec::from_ints(values.iter().filter_map(Value::as_int).collect())
        }
        TypeTag::Int => AelysVec::from_objects(values),
        TypeTag::Float => {
            AelysVec::from_floats(values.iter().filter_map(Value::as_float).collect())
        }
        TypeTag::Bool => AelysVec::from_bools(values.iter().filter_map(Value::as_bool).collect()),
        TypeTag::Object => AelysVec::from_objects(values),
        TypeTag::Byte => match values.iter().map(byte_value).collect() {
            Some(bytes) => AelysVec::from_bytes(bytes),
            None => typed_vec(TypeTag::Int, values),
        },
    }
}

fn byte_value(value: &Value) -> Option<u8> {
    value.as_int().and_then(|int| u8::try_from(int).ok())
}

fn element_tag(vm: &VM, value: Value) -> Result<TypeTag, RuntimeError> {
    value
        .as_int()
        .and_then(|raw| u8::try_from(raw).ok())
        .and_then(TypeTag::from_u8)
        .ok_or_else(|| type_error(vm, "collection element tag", "type tag", value))
}

fn map_keys(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let map = map_ref(vm, args[0], "map keys")?;
    let tag = element_tag(vm, args[1])?;
    let keys: Vec<Value> = read_map(vm, map, |inner| inner.entries().map(|(k, _)| k).collect());
    let reference = vm.alloc_vec(typed_vec(tag, keys))?;
    Ok(Value::ptr(reference.index()))
}

fn map_values(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let map = map_ref(vm, args[0], "map values")?;
    let tag = element_tag(vm, args[1])?;
    let values: Vec<Value> = read_map(vm, map, |inner| inner.entries().map(|(_, v)| v).collect());
    let reference = vm.alloc_vec(typed_vec(tag, values))?;
    Ok(Value::ptr(reference.index()))
}

fn string_arg<'a>(
    vm: &'a VM,
    value: Value,
    operation: &'static str,
) -> Result<&'a str, RuntimeError> {
    vm.string_bytes(value)
        .map(|bytes| std::str::from_utf8(bytes).unwrap_or_default())
        .ok_or_else(|| type_error(vm, operation, "string", value))
}

fn char_arg(vm: &VM, value: Value, operation: &'static str) -> Result<char, RuntimeError> {
    value
        .as_char()
        .ok_or_else(|| type_error(vm, operation, "char", value))
}

fn string_vec(vm: &mut VM, parts: Vec<String>) -> Result<Value, RuntimeError> {
    let vector = vm.alloc_vec(AelysVec::with_capacity_objects(parts.len()))?;
    let _root = HostRoot::new(&vm.host_roots, vector, vm.id);
    for part in parts {
        let text = Value::ptr(vm.alloc_string(&part)?.index());
        let index = with_vec(vm, vector, "string split", |inner| {
            inner.push(text);
            inner.len() - 1
        })?;
        vm.heap.write_barrier_element(vector, index, text);
    }
    Ok(Value::ptr(vector.index()))
}

pub(crate) fn string_chars(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let chars: Vec<Value> = string_arg(vm, args[0], "string chars")?
        .chars()
        .map(Value::char)
        .collect();
    let reference = vm.alloc_vec(AelysVec::from_objects(chars))?;
    Ok(Value::ptr(reference.index()))
}

pub(crate) fn string_bytes(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let bytes: Vec<i64> = string_arg(vm, args[0], "string bytes")?
        .bytes()
        .map(i64::from)
        .collect();
    let reference = vm.alloc_vec(AelysVec::from_ints(bytes))?;
    Ok(Value::ptr(reference.index()))
}

pub(crate) fn string_split(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let text = string_arg(vm, args[0], "string split")?;
    let separator = string_arg(vm, args[1], "string split")?;
    let parts: Vec<String> = if separator.is_empty() {
        text.chars().map(String::from).collect()
    } else {
        text.split(separator).map(str::to_string).collect()
    };
    string_vec(vm, parts)
}

pub(crate) fn string_lines(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let parts: Vec<String> = string_arg(vm, args[0], "string lines")?
        .lines()
        .map(str::to_string)
        .collect();
    string_vec(vm, parts)
}

fn string_push(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let c = char_arg(vm, args[1], "string push")?;
    let mut buffer = [0u8; 4];
    let suffix = vm.alloc_string(c.encode_utf8(&mut buffer))?;
    let suffix_value = Value::ptr(suffix.index());
    let _root = HostRoot::new(&vm.host_roots, suffix, vm.id);
    vm.add_values(args[0], suffix_value)
}

pub(crate) fn string_from_char(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let c = char_arg(vm, args[0], "string from_char")?;
    let mut buffer = [0u8; 4];
    let reference = vm.alloc_string(c.encode_utf8(&mut buffer))?;
    Ok(Value::ptr(reference.index()))
}

pub(crate) fn char_from_scalar(vm: &VM, value: Value) -> Result<Value, RuntimeError> {
    let raw = vm
        .int_value(value)
        .ok_or_else(|| type_error(vm, "int as char", "int", value))?;
    u32::try_from(raw)
        .ok()
        .and_then(char::from_u32)
        .map(Value::char)
        .ok_or_else(|| vm.runtime_error(RuntimeErrorKind::InvalidCharScalar { value: raw }))
}

fn char_from_int(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    char_from_scalar(vm, args[0])
}

fn char_to_int(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let c = char_arg(vm, args[0], "char as int")?;
    Ok(Value::int(i64::from(u32::from(c))))
}

fn char_is_digit(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    Ok(Value::bool(
        char_arg(vm, args[0], "char is_digit")?.is_ascii_digit(),
    ))
}

fn char_is_alpha(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    Ok(Value::bool(
        char_arg(vm, args[0], "char is_alpha")?.is_alphabetic(),
    ))
}

fn char_is_whitespace(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    Ok(Value::bool(
        char_arg(vm, args[0], "char is_whitespace")?.is_whitespace(),
    ))
}

fn char_is_alphanumeric(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    Ok(Value::bool(
        char_arg(vm, args[0], "char is_alphanumeric")?.is_alphanumeric(),
    ))
}

fn single_case(mut mapped: impl Iterator<Item = char>, original: char) -> char {
    match (mapped.next(), mapped.next()) {
        (Some(c), None) => c,
        _ => original,
    }
}

fn char_to_upper(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let c = char_arg(vm, args[0], "char to_upper")?;
    Ok(Value::char(single_case(c.to_uppercase(), c)))
}

fn char_to_lower(vm: &mut VM, args: &[Value]) -> Result<Value, RuntimeError> {
    let c = char_arg(vm, args[0], "char to_lower")?;
    Ok(Value::char(single_case(c.to_lowercase(), c)))
}
