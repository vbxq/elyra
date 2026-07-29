use aelys_native::{AELYS_ABI_VERSION, AelysModuleDescriptor, AelysTypeDescriptor, AelysVmApi};

fn assert_sync<T: Sync>() {}

#[test]
fn descriptor_abi_version_matches() {
    assert_eq!(AelysModuleDescriptor::ABI_VERSION, AELYS_ABI_VERSION);
    assert_sync::<AelysVmApi>();
    assert_sync::<AelysTypeDescriptor>();
}
