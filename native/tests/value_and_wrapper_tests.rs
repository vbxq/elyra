use aelys_native::{
    AELYS_API_VERSION, AELYS_NATIVE_INTEGER_OVERFLOW, AELYS_NATIVE_INVALID_ARGUMENT,
    AELYS_NATIVE_OPTION_NONE, AELYS_NATIVE_RESULT_ERROR, AelysValue, AelysVmApi, NativeContext,
    aelys_module, value_as_handle, value_as_int, value_bool, value_int, value_is_null,
    value_is_unit, value_null,
};

#[aelys_module(name = "typed_results")]
mod typed_results {
    #[aelys_export]
    pub fn maybe(value: i64) -> Result<i64, String> {
        if value == 0 {
            Err("zero".to_string())
        } else {
            Ok(value)
        }
    }

    #[aelys_export]
    pub fn maybe_none(value: i64) -> Option<i64> {
        (value != 0).then_some(value)
    }

    #[aelys_export]
    pub fn maybe_text(value: i64) -> Option<String> {
        (value != 0).then(|| value.to_string())
    }

    #[aelys_export]
    pub fn text_result(value: i64) -> Result<String, String> {
        if value == 0 {
            Err("zero".to_string())
        } else {
            Ok(value.to_string())
        }
    }

    #[aelys_export]
    pub fn maybe_unit(value: i64) -> Option<()> {
        (value != 0).then_some(())
    }

    #[aelys_export]
    pub fn unit_result(value: i64) -> Result<(), String> {
        if value == 0 {
            Err("zero".to_string())
        } else {
            Ok(())
        }
    }
}

#[aelys_module(name = "checked_values", version = "0.22.0")]
mod exports {
    #[aelys_export]
    pub fn identity(value: i64) -> i64 {
        value
    }

    #[aelys_export]
    pub fn overflowing() -> i64 {
        i64::MAX
    }
}

#[aelys_module(name = "second_checked_values")]
mod second_exports {
    #[aelys_export]
    pub fn answer() -> i64 {
        42
    }

    #[aelys_export]
    pub fn noop() {}
}

#[test]
fn native_integer_constructor_checks_nan_boxing_range() {
    let minimum = -(1i64 << 47);
    let maximum = (1i64 << 47) - 1;
    for integer in [minimum, -1, 0, 1, maximum] {
        assert_eq!(value_as_int(value_int(integer).unwrap()), integer);
    }
    assert!(value_int(minimum - 1).is_none());
    assert!(value_int(maximum + 1).is_none());
    assert!(value_int(i64::MIN).is_none());
    assert!(value_int(i64::MAX).is_none());
    assert!(value_as_handle(value_null()).is_none());
}

#[test]
fn generated_wrapper_validates_pointers_arity_and_types() {
    let input = value_int(7).unwrap();
    let mut output = value_null();

    assert_eq!(
        unsafe {
            __aelys_wrapper_636865636b65645f76616c756573_identity(
                std::ptr::null_mut(),
                &input,
                1,
                std::ptr::null_mut(),
            )
        },
        AELYS_NATIVE_INVALID_ARGUMENT
    );
    assert_eq!(
        unsafe {
            __aelys_wrapper_636865636b65645f76616c756573_identity(
                std::ptr::null_mut(),
                &input,
                0,
                &mut output,
            )
        },
        AELYS_NATIVE_INVALID_ARGUMENT
    );
    assert_eq!(
        unsafe {
            __aelys_wrapper_636865636b65645f76616c756573_identity(
                std::ptr::null_mut(),
                std::ptr::null(),
                1,
                &mut output,
            )
        },
        AELYS_NATIVE_INVALID_ARGUMENT
    );
    let wrong_type = value_bool(true);
    assert_eq!(
        unsafe {
            __aelys_wrapper_636865636b65645f76616c756573_identity(
                std::ptr::null_mut(),
                &wrong_type,
                1,
                &mut output,
            )
        },
        AELYS_NATIVE_INVALID_ARGUMENT
    );
    assert_eq!(
        unsafe {
            __aelys_wrapper_636865636b65645f76616c756573_identity(
                std::ptr::null_mut(),
                &input,
                1,
                &mut output,
            )
        },
        0
    );
    assert_eq!(value_as_int(output), 7);
}

#[test]
fn generated_wrapper_reports_integer_overflow() {
    let mut output = value_null();
    assert_eq!(
        unsafe {
            __aelys_wrapper_636865636b65645f76616c756573_overflowing(
                std::ptr::null_mut(),
                std::ptr::null(),
                0,
                &mut output,
            )
        },
        AELYS_NATIVE_INTEGER_OVERFLOW
    );
}

#[test]
fn generated_unit_wrapper_returns_unit_not_null() {
    let mut output = value_null();
    assert_eq!(
        unsafe {
            __aelys_wrapper_7365636f6e645f636865636b65645f76616c756573_noop(
                std::ptr::null_mut(),
                std::ptr::null(),
                0,
                &mut output,
            )
        },
        0
    );
    assert!(!value_is_null(output));
    assert!(value_is_unit(output));
}

#[test]
fn generated_sum_wrappers_encode_option_and_result_statuses() {
    let input = value_int(7).unwrap();
    let mut output = value_null();
    assert_eq!(
        unsafe {
            __aelys_wrapper_74797065645f726573756c7473_maybe(
                std::ptr::null_mut(),
                &input,
                1,
                &mut output,
            )
        },
        0
    );
    assert_eq!(value_as_int(output), 7);

    let zero = value_int(0).unwrap();
    assert_eq!(
        unsafe {
            __aelys_wrapper_74797065645f726573756c7473_maybe(
                std::ptr::null_mut(),
                &zero,
                1,
                &mut output,
            )
        },
        AELYS_NATIVE_RESULT_ERROR
    );
    assert_eq!(
        unsafe {
            __aelys_wrapper_74797065645f726573756c7473_maybe_none(
                std::ptr::null_mut(),
                &zero,
                1,
                &mut output,
            )
        },
        AELYS_NATIVE_OPTION_NONE
    );
}

extern "C" fn test_alloc_string(
    _context: *mut NativeContext,
    bytes: *const u8,
    length: usize,
    out: *mut AelysValue,
) -> i32 {
    let bytes = unsafe { std::slice::from_raw_parts(bytes, length) };
    let value = match bytes {
        b"7" => 1,
        b"zero" => 4,
        _ => return AELYS_NATIVE_INVALID_ARGUMENT,
    };
    unsafe {
        *out = value_int(value).unwrap();
    }
    0
}

#[test]
fn generated_sum_wrappers_encode_string_and_unit_payloads() {
    aelys_native::store_vm_api(&AelysVmApi {
        api_version: AELYS_API_VERSION,
        size: std::mem::size_of::<AelysVmApi>() as u32,
        register_function: None,
        register_constant: None,
        register_type: None,
        alloc_string: Some(test_alloc_string),
        read_string: None,
        _reserved: [0; 3],
    });

    let input = value_int(7).unwrap();
    let zero = value_int(0).unwrap();
    let mut output = value_null();

    assert_eq!(
        unsafe {
            __aelys_wrapper_74797065645f726573756c7473_maybe_text(
                std::ptr::null_mut(),
                &input,
                1,
                &mut output,
            )
        },
        0
    );
    assert_eq!(value_as_int(output), 1);
    assert_eq!(
        unsafe {
            __aelys_wrapper_74797065645f726573756c7473_text_result(
                std::ptr::null_mut(),
                &input,
                1,
                &mut output,
            )
        },
        0
    );
    assert_eq!(value_as_int(output), 1);
    assert_eq!(
        unsafe {
            __aelys_wrapper_74797065645f726573756c7473_text_result(
                std::ptr::null_mut(),
                &zero,
                1,
                &mut output,
            )
        },
        AELYS_NATIVE_RESULT_ERROR
    );
    assert_eq!(value_as_int(output), 4);

    assert_eq!(
        unsafe {
            __aelys_wrapper_74797065645f726573756c7473_maybe_unit(
                std::ptr::null_mut(),
                &input,
                1,
                &mut output,
            )
        },
        0
    );
    assert!(value_is_unit(output));
    assert_eq!(
        unsafe {
            __aelys_wrapper_74797065645f726573756c7473_maybe_unit(
                std::ptr::null_mut(),
                &zero,
                1,
                &mut output,
            )
        },
        AELYS_NATIVE_OPTION_NONE
    );
    assert_eq!(
        unsafe {
            __aelys_wrapper_74797065645f726573756c7473_unit_result(
                std::ptr::null_mut(),
                &input,
                1,
                &mut output,
            )
        },
        0
    );
    assert!(value_is_unit(output));
    assert_eq!(
        unsafe {
            __aelys_wrapper_74797065645f726573756c7473_unit_result(
                std::ptr::null_mut(),
                &zero,
                1,
                &mut output,
            )
        },
        AELYS_NATIVE_RESULT_ERROR
    );
    assert_eq!(value_as_int(output), 4);
}
