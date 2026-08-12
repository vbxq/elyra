// FNV-1a hash for export validation

use crate::{AelysExport, AelysModuleDescriptor};
use core::ffi::CStr;

const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

/// Computes a hash of the export table for module validation.
///
/// # Safety
/// - `exports` must be a valid pointer to an array of `export_count` AelysExport entries, or null if `export_count` is 0.
/// - Each export's `name` pointer must be a valid null-terminated C string or null.
pub unsafe fn compute_exports_hash(exports: *const AelysExport, export_count: u32) -> u64 {
    let mut hash = FNV_OFFSET_BASIS;
    hash_u64(&mut hash, u64::from(export_count));
    if export_count == 0 || exports.is_null() {
        return hash;
    }

    let count = usize::try_from(export_count).expect("u32 export count fits target usize");
    let slice = unsafe { core::slice::from_raw_parts(exports, count) };
    for export in slice {
        let name_bytes = if export.name.is_null() {
            &[][..]
        } else {
            unsafe { CStr::from_ptr(export.name) }.to_bytes()
        };
        hash_bytes(&mut hash, name_bytes);
        hash_u64(&mut hash, u64::from(export.kind as u32));
        hash_bytes(&mut hash, &export.arity.to_le_bytes());
        hash_u64(
            &mut hash,
            u64::try_from(export.value.addr()).expect("pointer address fits u64"),
        );
        hash_u64(
            &mut hash,
            u64::try_from(export.signature.addr()).expect("pointer address fits u64"),
        );
    }

    hash
}

/// Initializes the exports_hash field of a module descriptor.
///
/// # Safety
/// - `descriptor` must be a valid pointer to an AelysModuleDescriptor, or null (in which case this is a no-op).
/// - The descriptor's `exports` and `export_count` fields must be valid as per `compute_exports_hash` requirements.
pub unsafe fn init_descriptor_exports_hash(descriptor: *mut AelysModuleDescriptor) {
    if descriptor.is_null() {
        return;
    }
    let desc = unsafe { &mut *descriptor };
    let hash = unsafe { compute_exports_hash(desc.exports(), desc.export_count()) };
    desc.set_exports_hash(hash);
}

fn hash_bytes(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(FNV_PRIME);
    }
}

fn hash_u64(hash: &mut u64, value: u64) {
    hash_bytes(hash, &value.to_le_bytes());
}
