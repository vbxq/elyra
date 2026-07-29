use aelys_bytecode::asm::binary::{deserialize_with_manifest, serialize_with_manifest};
use aelys_runtime::Function;

#[test]
fn avbc_round_trip_manifest() {
    let func = Function::new(None, 0);
    let manifest = b"[build]\nbundle_native_modules = true\n".to_vec();

    let bytes = serialize_with_manifest(&func, Some(&manifest), None).unwrap();
    let (_func, decoded_manifest, _bundles) = deserialize_with_manifest(&bytes).expect("read");

    assert_eq!(decoded_manifest.unwrap(), manifest);
}
