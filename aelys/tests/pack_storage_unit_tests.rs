use aelys_bytecode::Value;
use aelys_bytecode::object::{AelysVec, TypeTag, VecData};

fn ints(vector: &AelysVec) -> Vec<i64> {
    (0..vector.len())
        .map(|index| vector.get(index).and_then(|value| value.as_int()).unwrap())
        .collect()
}

#[test]
fn packed_storage_keeps_exactly_the_values_0_to_255() {
    let mut vector = AelysVec::new_bytes();
    for value in [0, 1, 127, 128, 255] {
        assert!(vector.push(Value::int(value)));
    }
    assert!(!vector.push(Value::int(256)));
    assert!(!vector.push(Value::int(-1)));
    assert!(!vector.push(Value::float(1.0)));
    assert!(!vector.set(0, Value::int(256)));
    assert!(vector.set(0, Value::int(42)));
    assert_eq!(vector.type_tag(), TypeTag::Byte);
    assert_eq!(vector.data.as_bytes(), Some(&[42u8, 1, 127, 128, 255][..]));
    assert_eq!(vector.pop().and_then(|value| value.as_int()), Some(255));
    assert!(vector.insert(1, Value::int(9)));
    assert_eq!(ints(&vector), [42, 9, 1, 127, 128]);
    assert!(vector.data.as_ints().is_none());
}

#[test]
fn one_byte_per_element_is_charged_to_the_heap() {
    let packed = AelysVec::from_bytes(vec![0; 1000]);
    let wide = AelysVec::from_ints(vec![0; 1000]);
    assert_eq!(wide.size_bytes() - packed.size_bytes(), 7000);
    assert_eq!(
        AelysVec::size_bytes_for(TypeTag::Byte, 1000),
        Some(packed.size_bytes())
    );
}

#[test]
fn packed_storage_widens_to_ints_or_objects_without_changing_a_value() {
    let mut vector = AelysVec::from_bytes(vec![1, 200, 255]);
    vector.widen_bytes_to_ints();
    assert!(matches!(vector.data, VecData::Ints(_)));
    assert!(vector.set(1, Value::int(70000)));
    assert_eq!(ints(&vector), [1, 70000, 255]);

    let mut vector = AelysVec::from_bytes(vec![3, 4]);
    vector.promote_ints_to_objects();
    assert!(matches!(vector.data, VecData::Objects(_)));
    assert_eq!(ints(&vector), [3, 4]);
}

#[test]
fn wider_storage_packs_only_when_every_value_is_a_byte() {
    let mut vector = AelysVec::from_ints(vec![0, 17, 255]);
    assert!(vector.pack_bytes());
    assert_eq!(vector.data.as_bytes(), Some(&[0u8, 17, 255][..]));

    let mut vector = AelysVec::from_ints(vec![0, 256]);
    assert!(!vector.pack_bytes());
    assert_eq!(ints(&vector), [0, 256]);

    let mut vector = AelysVec::from_objects(vec![Value::int(5), Value::int(6)]);
    assert!(vector.pack_bytes());
    assert_eq!(vector.data.as_bytes(), Some(&[5u8, 6][..]));

    let mut vector = AelysVec::from_floats(vec![1.0]);
    assert!(!vector.pack_bytes());
}

#[test]
fn slices_clones_and_arrays_of_packed_storage_keep_the_bytes() {
    let vector = AelysVec::from_bytes(vec![10, 20, 30, 40]);
    let slice = vector.slice(1, 3);
    assert_eq!(slice.data.as_bytes(), Some(&[20u8, 30][..]));
    let array = vector.to_array();
    assert_eq!(array.data.as_ints(), Some(&[10i64, 20, 30, 40][..]));
    let copy = vector.clone();
    assert_eq!(copy.data.as_bytes(), vector.data.as_bytes());
}
