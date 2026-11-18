use crate::{
    AELYS_NATIVE_INVALID_ARGUMENT, AelysValue, NativeContext, alloc_string_from_context,
    read_string_from_value, vm_api,
};
use core::marker::PhantomData;

pub struct CallContext<'call> {
    raw: *mut NativeContext,
    _call: PhantomData<&'call mut NativeContext>,
}

impl<'call> CallContext<'call> {
    pub fn alloc_string(&mut self, value: &str) -> Result<AelysValue, i32> {
        unsafe { alloc_string_from_context(self.raw, value) }
    }

    pub fn read_string(&self, value: AelysValue) -> Option<String> {
        unsafe { read_string_from_value(self.raw, value) }
    }

    // sound only because the vm refuses a second borrow of one handle during a call
    pub fn borrow_bytes(&self, handle: AelysValue) -> Result<&'call mut [u8], i32> {
        let borrow = vm_api()
            .and_then(|api| api.borrow_bytes)
            .ok_or(AELYS_NATIVE_INVALID_ARGUMENT)?;
        let mut pointer: *mut u8 = core::ptr::null_mut();
        let mut length: usize = 0;
        let status = borrow(self.raw, handle, &mut pointer, &mut length);
        if status != 0 {
            return Err(status);
        }
        if pointer.is_null() {
            if length == 0 {
                return Ok(&mut []);
            }
            return Err(AELYS_NATIVE_INVALID_ARGUMENT);
        }
        Ok(unsafe { core::slice::from_raw_parts_mut(pointer, length) })
    }

    pub fn borrow_bytes_ref(&self, handle: AelysValue) -> Result<&'call [u8], i32> {
        self.borrow_bytes(handle).map(|bytes| &*bytes)
    }

    pub fn as_raw(&self) -> *mut NativeContext {
        self.raw
    }
}

#[allow(clippy::missing_safety_doc)]
pub unsafe fn with_call_context<R>(
    raw: *mut NativeContext,
    body: impl for<'call> FnOnce(&mut CallContext<'call>) -> R,
) -> R {
    let mut context = CallContext {
        raw,
        _call: PhantomData,
    };
    body(&mut context)
}
