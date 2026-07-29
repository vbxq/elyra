use aelys_native::{
    AELYS_NATIVE_INTEGER_OVERFLOW, AELYS_NATIVE_INVALID_ARGUMENT, aelys_module, value_as_int,
    value_bool, value_int, value_null,
};

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
}

#[test]
fn generated_wrapper_validates_pointers_arity_and_types() {
    let input = value_int(7).unwrap();
    let mut output = value_null();

    assert_eq!(
        unsafe { __aelys_wrapper_identity(std::ptr::null_mut(), &input, 1, std::ptr::null_mut()) },
        AELYS_NATIVE_INVALID_ARGUMENT
    );
    assert_eq!(
        unsafe { __aelys_wrapper_identity(std::ptr::null_mut(), &input, 0, &mut output) },
        AELYS_NATIVE_INVALID_ARGUMENT
    );
    assert_eq!(
        unsafe { __aelys_wrapper_identity(std::ptr::null_mut(), std::ptr::null(), 1, &mut output) },
        AELYS_NATIVE_INVALID_ARGUMENT
    );
    let wrong_type = value_bool(true);
    assert_eq!(
        unsafe { __aelys_wrapper_identity(std::ptr::null_mut(), &wrong_type, 1, &mut output) },
        AELYS_NATIVE_INVALID_ARGUMENT
    );
    assert_eq!(
        unsafe { __aelys_wrapper_identity(std::ptr::null_mut(), &input, 1, &mut output) },
        0
    );
    assert_eq!(value_as_int(output), 7);
}

#[test]
fn generated_wrapper_reports_integer_overflow() {
    let mut output = value_null();
    assert_eq!(
        unsafe {
            __aelys_wrapper_overflowing(std::ptr::null_mut(), std::ptr::null(), 0, &mut output)
        },
        AELYS_NATIVE_INTEGER_OVERFLOW
    );
}
