use crate::vm::{Function, Heap};

pub(super) fn verify_constants(func: &Function, _heap: &Heap) -> Result<(), String> {
    for (idx, value) in func.constants.iter().enumerate() {
        // Check for nested function marker (uses dedicated tag)
        if let Some(func_idx) = value.as_nested_fn_marker() {
            if func_idx >= func.nested_functions.len() {
                return Err(format!(
                    "constant {} has invalid nested function index {}",
                    idx, func_idx
                ));
            }
            continue;
        }
    }
    Ok(())
}
