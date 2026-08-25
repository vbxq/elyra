mod common;

use aelys_bytecode::{AelysArray, AelysVec, ArrayData, TypeTag, Value};
use common::{
    assert_aelys_bool, assert_aelys_error_contains, assert_aelys_int, assert_aelys_str, run_aelys,
    run_aelys_ok,
};

#[test]
fn test_array_new_ints() {
    let arr = AelysArray::new_ints(5);
    assert_eq!(arr.len(), 5);
    assert!(!arr.is_empty());
    assert_eq!(arr.type_tag(), TypeTag::Int);

    for i in 0..5 {
        assert_eq!(arr.get(i), Some(Value::int(0)));
    }
}

#[test]
fn test_array_new_floats() {
    let arr = AelysArray::new_floats(3);
    assert_eq!(arr.len(), 3);
    assert_eq!(arr.type_tag(), TypeTag::Float);

    for i in 0..3 {
        assert_eq!(arr.get(i), Some(Value::float(0.0)));
    }
}

#[test]
fn test_array_new_bools() {
    let arr = AelysArray::new_bools(4);
    assert_eq!(arr.len(), 4);
    assert_eq!(arr.type_tag(), TypeTag::Bool);

    for i in 0..4 {
        assert_eq!(arr.get(i), Some(Value::bool(false)));
    }
}

#[test]
fn test_array_new_objects() {
    let arr = AelysArray::new_objects(2);
    assert_eq!(arr.len(), 2);
    assert_eq!(arr.type_tag(), TypeTag::Object);

    for i in 0..2 {
        assert!(arr.get(i).unwrap().is_null());
    }
}

#[test]
fn test_array_from_ints() {
    let arr = AelysArray::from_ints(vec![1, 2, 3, 4, 5]);
    assert_eq!(arr.len(), 5);
    assert_eq!(arr.type_tag(), TypeTag::Int);

    assert_eq!(arr.get(0), Some(Value::int(1)));
    assert_eq!(arr.get(2), Some(Value::int(3)));
    assert_eq!(arr.get(4), Some(Value::int(5)));
    assert_eq!(arr.get(5), None); // Out of bounds
}

#[test]
fn test_array_from_floats() {
    let arr = AelysArray::from_floats(vec![1.5, 2.5, 3.5]);
    assert_eq!(arr.len(), 3);
    assert_eq!(arr.type_tag(), TypeTag::Float);

    assert_eq!(arr.get(0), Some(Value::float(1.5)));
    assert_eq!(arr.get(1), Some(Value::float(2.5)));
    assert_eq!(arr.get(2), Some(Value::float(3.5)));
}

#[test]
fn test_array_from_bools() {
    let arr = AelysArray::from_bools(vec![true, false, true]);
    assert_eq!(arr.len(), 3);
    assert_eq!(arr.type_tag(), TypeTag::Bool);

    assert_eq!(arr.get(0), Some(Value::bool(true)));
    assert_eq!(arr.get(1), Some(Value::bool(false)));
    assert_eq!(arr.get(2), Some(Value::bool(true)));
}

#[test]
fn test_array_set() {
    let mut arr = AelysArray::new_ints(3);

    assert!(arr.set(0, Value::int(10)));
    assert!(arr.set(1, Value::int(20)));
    assert!(arr.set(2, Value::int(30)));

    assert_eq!(arr.get(0), Some(Value::int(10)));
    assert_eq!(arr.get(1), Some(Value::int(20)));
    assert_eq!(arr.get(2), Some(Value::int(30)));

    assert!(!arr.set(3, Value::int(40)));

    assert!(!arr.set(0, Value::float(1.5)));
}

#[test]
fn test_array_empty() {
    let arr = AelysArray::new_ints(0);
    assert_eq!(arr.len(), 0);
    assert!(arr.is_empty());
    assert_eq!(arr.get(0), None);
}

#[test]
fn test_array_size_bytes() {
    let arr_ints = AelysArray::from_ints(vec![1, 2, 3, 4]);
    let arr_bools = AelysArray::from_bools(vec![true, false, true, false]);

    assert!(arr_ints.size_bytes() > arr_bools.size_bytes());
}

#[test]
fn test_vec_new_empty() {
    let v = AelysVec::new_ints();
    assert_eq!(v.len(), 0);
    assert!(v.is_empty());
    assert_eq!(v.type_tag(), TypeTag::Int);
}

#[test]
fn test_vec_push_pop() {
    let mut v = AelysVec::new_ints();

    assert!(v.push(Value::int(1)));
    assert!(v.push(Value::int(2)));
    assert!(v.push(Value::int(3)));

    assert_eq!(v.len(), 3);
    assert!(!v.is_empty());

    assert_eq!(v.pop(), Some(Value::int(3)));
    assert_eq!(v.pop(), Some(Value::int(2)));
    assert_eq!(v.pop(), Some(Value::int(1)));
    assert_eq!(v.pop(), None);

    assert!(v.is_empty());
}

#[test]
fn test_vec_push_wrong_type() {
    let mut v = AelysVec::new_ints();

    assert!(!v.push(Value::float(1.5)));
    assert!(!v.push(Value::bool(true)));

    assert!(v.is_empty());
}

#[test]
fn test_vec_get_set() {
    let mut v = AelysVec::from_ints(vec![10, 20, 30]);

    assert_eq!(v.get(0), Some(Value::int(10)));
    assert_eq!(v.get(1), Some(Value::int(20)));
    assert_eq!(v.get(2), Some(Value::int(30)));
    assert_eq!(v.get(3), None);

    assert!(v.set(1, Value::int(25)));
    assert_eq!(v.get(1), Some(Value::int(25)));

    assert!(!v.set(5, Value::int(50)));
}

#[test]
fn test_vec_reserve() {
    let mut v = AelysVec::new_floats();
    assert_eq!(v.capacity(), 0);

    v.reserve(10);
    assert!(v.capacity() >= 10);
    assert!(v.is_empty()); // Reserve doesn't add elements
}

#[test]
fn vec_heap_size_ignores_allocator_slack() {
    let mut backing = Vec::with_capacity(64);
    backing.push(7);
    let v = AelysVec::from_ints(backing);

    assert!(v.capacity() >= 64);
    assert_eq!(
        v.size_bytes(),
        AelysVec::size_bytes_for(TypeTag::Int, 1).unwrap()
    );
}

#[test]
fn test_vec_clear() {
    let mut v = AelysVec::from_bools(vec![true, false, true]);
    assert_eq!(v.len(), 3);

    v.clear();
    assert!(v.is_empty());
    assert_eq!(v.len(), 0);
}

#[test]
fn test_vec_shrink_to_fit() {
    let mut v = AelysVec::new_ints();
    v.reserve(100);
    assert!(v.capacity() >= 100);

    v.push(Value::int(1));
    v.shrink_to_fit();
    assert!(v.capacity() >= 1);
}

#[test]
fn test_vec_to_array() {
    let v = AelysVec::from_ints(vec![1, 2, 3]);
    let arr = v.to_array();

    assert_eq!(arr.len(), 3);
    assert_eq!(arr.type_tag(), TypeTag::Int);
    assert_eq!(arr.get(0), Some(Value::int(1)));
    assert_eq!(arr.get(1), Some(Value::int(2)));
    assert_eq!(arr.get(2), Some(Value::int(3)));
}

#[test]
fn test_vec_with_capacity() {
    let v = AelysVec::with_capacity_ints(10);
    assert!(v.is_empty());
    assert!(v.capacity() >= 10);
}

#[test]
fn test_type_tag_from_u8() {
    assert_eq!(TypeTag::from_u8(0), Some(TypeTag::Int));
    assert_eq!(TypeTag::from_u8(1), Some(TypeTag::Float));
    assert_eq!(TypeTag::from_u8(2), Some(TypeTag::Bool));
    assert_eq!(TypeTag::from_u8(3), Some(TypeTag::Object));
    assert_eq!(TypeTag::from_u8(4), None);
    assert_eq!(TypeTag::from_u8(255), None);
}

#[test]
fn test_array_data_accessors() {
    let data_ints = ArrayData::Ints(vec![1, 2, 3].into_boxed_slice());
    assert!(data_ints.as_ints().is_some());
    assert!(data_ints.as_floats().is_none());
    assert!(data_ints.as_bools().is_none());
    assert!(data_ints.as_objects().is_none());

    let data_floats = ArrayData::Floats(vec![1.0, 2.0].into_boxed_slice());
    assert!(data_floats.as_floats().is_some());
    assert!(data_floats.as_ints().is_none());

    let data_bools = ArrayData::Bools(vec![1, 0, 1].into_boxed_slice());
    assert!(data_bools.as_bools().is_some());
    assert!(data_bools.as_floats().is_none());

    let data_objects = ArrayData::Objects(vec![Value::null()].into_boxed_slice());
    assert!(data_objects.as_objects().is_some());
    assert!(data_objects.as_bools().is_none());
}

#[test]
fn test_array_data_mutable_accessors() {
    let mut data = ArrayData::Ints(vec![1, 2, 3].into_boxed_slice());

    if let Some(ints) = data.as_ints_mut() {
        ints[0] = 100;
    }

    assert_eq!(data.as_ints().unwrap()[0], 100);
}

#[test]
fn test_e2e_empty_array_literal() {
    let result = run_aelys_ok("let arr: [int; 0] = []; 0");
    assert_eq!(result.as_int(), Some(0));
}

#[test]
fn test_e2e_int_array_literal() {
    assert_aelys_int("let arr = [1, 2, 3]; arr[0]", 1);
    assert_aelys_int("let arr = [1, 2, 3]; arr[1]", 2);
    assert_aelys_int("let arr = [1, 2, 3]; arr[2]", 3);
}

#[test]
fn test_e2e_array_length() {
    assert_aelys_int("let arr = [1, 2, 3, 4, 5]; arr.len()", 5);
    assert_aelys_int("let arr: [int; 0] = []; arr.len()", 0);
}

#[test]
fn test_e2e_array_index_expression() {
    assert_aelys_int("let arr = [10, 20, 30]; let i = 1; arr[i]", 20);
    assert_aelys_int("let arr = [10, 20, 30]; arr[1 + 1]", 30);
}

#[test]
fn test_e2e_array_store() {
    assert_aelys_int("let mut arr = [1, 2, 3]; arr[0] = 100; arr[0]", 100);
    assert_aelys_int("let mut arr = [1, 2, 3]; arr[1] = 200; arr[1]", 200);
}

#[test]
fn test_e2e_float_array() {
    let result = run_aelys("let arr = [1.5, 2.5, 3.5]; arr[1]");
    assert_eq!(result.as_float(), Some(2.5));
}

#[test]
fn test_e2e_bool_array() {
    assert_aelys_bool("let arr = [true, false, true]; arr[0]", true);
    assert_aelys_bool("let arr = [true, false, true]; arr[1]", false);
}

#[test]
fn test_e2e_nested_array_access() {
    assert_aelys_int(
        r#"
        fn get_second(arr) {
            arr[1]
        }
        let a = [10, 20, 30];
        get_second(a)
        "#,
        20,
    );
}

#[test]
fn test_e2e_array_in_loop() {
    assert_aelys_int(
        r#"
        let mut arr = [1, 2, 3, 4, 5];
        let mut sum = 0;
        let mut i = 0;
        while i < arr.len() {
            sum += arr[i];
            i++;
        }
        sum
        "#,
        15, // 1+2+3+4+5
    );
}

#[test]
fn test_e2e_array_trailing_comma() {
    assert_aelys_int("let arr = [1, 2, 3,]; arr[2]", 3);
}

#[test]
fn test_e2e_empty_vec_literal() {
    let result = run_aelys_ok("let mut v: Vec<int> = vec![]; 0");
    assert_eq!(result.as_int(), Some(0));
}

#[test]
fn test_e2e_vec_literal() {
    assert_aelys_int("let mut v = vec![1, 2, 3]; v[0]", 1);
    assert_aelys_int("let mut v = vec![1, 2, 3]; v[2]", 3);
}

#[test]
fn test_e2e_vec_length() {
    assert_aelys_int("let mut v = vec![1, 2, 3, 4]; v.len()", 4);
}

#[test]
fn test_e2e_vec_push() {
    assert_aelys_int(
        r#"
        let mut v = vec![1, 2];
        v.push(3);
        v[2]
        "#,
        3,
    );
}

#[test]
fn test_e2e_vec_pop() {
    assert_aelys_int(
        r#"
        let mut v = vec![1, 2, 3];
        v.pop().unwrap()
        "#,
        3,
    );
}

#[test]
fn test_e2e_vec_push_pop_sequence() {
    assert_aelys_int(
        r#"
        let mut v = vec![];
        v.push(10);
        v.push(20);
        v.push(30);
        let a = v.pop().unwrap();
        let b = v.pop().unwrap();
        a + b
        "#,
        50, // 30 + 20
    );
}

#[test]
fn test_e2e_vec_store() {
    assert_aelys_int("let mut v = vec![1, 2, 3]; v[1] = 100; v[1]", 100);
}

#[test]
fn test_e2e_typed_array_int() {
    assert_aelys_int("let arr = [1, 2, 3]; arr[1]", 2);
}

#[test]
fn test_e2e_typed_array_float() {
    let result = run_aelys("let arr = [1.0, 2.0, 3.0]; arr[2]");
    assert_eq!(result.as_float(), Some(3.0));
}

#[test]
fn test_e2e_typed_array_bool() {
    assert_aelys_bool("let arr = [true, false]; arr[1]", false);
}

#[test]
fn test_e2e_typed_vec_int() {
    assert_aelys_int("let mut v = vec![5, 10, 15]; v[0]", 5);
}

#[test]
fn test_e2e_array_oob_read() {
    assert_aelys_error_contains("let arr = [1, 2, 3]; arr[10]", "out of bounds");
}

#[test]
fn test_e2e_array_oob_write() {
    assert_aelys_error_contains("let arr = [1, 2, 3]; arr[5] = 10", "out of bounds");
}

#[test]
fn test_e2e_array_negative_index() {
    assert_aelys_error_contains("let arr = [1, 2, 3]; arr[-1]", "");
}

#[test]
fn test_e2e_vec_oob_read() {
    assert_aelys_error_contains("let mut v = vec![1, 2]; v[5]", "out of bounds");
}

#[test]
fn test_e2e_array_sum_loop() {
    assert_aelys_int(
        r#"
        let arr = [10, 20, 30, 40, 50];
        let mut sum = 0;
        let mut i = 0;
        while i < arr.len() {
            sum += arr[i];
            i++;
        }
        sum
        "#,
        150,
    );
}

#[test]
fn test_e2e_array_modify_in_loop() {
    assert_aelys_int(
        r#"
        let mut arr = [1, 2, 3, 4, 5];
        let mut i = 0;
        while i < arr.len() {
            arr[i] *= 2;
            i++;
        }
        arr[0] + arr[1] + arr[2] + arr[3] + arr[4]
        "#,
        30, // 2+4+6+8+10
    );
}

#[test]
fn test_e2e_array_find_max() {
    assert_aelys_int(
        r#"
        let arr = [5, 2, 9, 1, 7];
        let mut max = arr[0];
        let mut i = 1;
        while i < arr.len() {
            if arr[i] > max {
                max = arr[i];
            }
            i++;
        }
        max
        "#,
        9,
    );
}

#[test]
fn test_e2e_array_swap() {
    assert_aelys_int(
        r#"
        let mut arr = [10, 20];
        let tmp = arr[0];
        arr[0] = arr[1];
        arr[1] = tmp;
        arr[0] * 10 + arr[1]
        "#,
        210, // 20*10 + 10
    );
}

#[test]
fn test_e2e_array_passed_to_function() {
    assert_aelys_int(
        r#"
        fn sum_arr(a) -> int {
            let mut s = 0;
            let mut i = 0;
            while i < a.len() {
                s += a[i];
                i++;
            }
            return s
        }
        let arr = [1, 2, 3, 4];
        sum_arr(arr)
        "#,
        10,
    );
}

#[test]
fn test_e2e_array_returned_from_function() {
    assert_aelys_int(
        r#"
        fn make_arr() {
            return [100, 200, 300]
        }
        let a = make_arr();
        a[1]
        "#,
        200,
    );
}

#[test]
fn test_e2e_array_nested_access() {
    assert_aelys_int(
        r#"
        let a = [1, 2, 3];
        let b = [10, 20, 30];
        a[0] + b[a[1]]
        "#,
        31, // 1 + b[2] = 1 + 30
    );
}

#[test]
fn test_e2e_vec_build_and_sum() {
    assert_aelys_int(
        r#"
        let mut v = vec![];
        v.push(1);
        v.push(2);
        v.push(3);
        v.push(4);
        v.push(5);
        let mut sum = 0;
        let mut i = 0;
        while i < v.len() {
            sum += v[i];
            i++;
        }
        sum
        "#,
        15,
    );
}

#[test]
fn test_e2e_vec_pop_all() {
    assert_aelys_int(
        r#"
        let mut v = vec![10, 20, 30];
        let a = v.pop().unwrap();
        let b = v.pop().unwrap();
        let c = v.pop().unwrap();
        a + b + c
        "#,
        60,
    );
}

#[test]
fn test_e2e_vec_modify_elements() {
    assert_aelys_int(
        r#"
        let mut v = vec![1, 2, 3];
        v[0] = 100;
        v[1] = 200;
        v[2] = 300;
        v[0] + v[1] + v[2]
        "#,
        600,
    );
}

#[test]
fn test_e2e_vec_capacity_after_reserve() {
    assert_aelys_bool(
        r#"
        let mut v: Vec<int> = vec![];
        v.reserve(50);
        v.capacity() >= 50
        "#,
        true,
    );
}

#[test]
fn test_e2e_vec_grow_dynamically() {
    assert_aelys_int(
        r#"
        let mut v = vec![];
        let mut i = 0;
        while i < 100 {
            v.push(i);
            i++;
        }
        v.len()
        "#,
        100,
    );
}

#[test]
fn test_e2e_vec_passed_to_function() {
    assert_aelys_int(
        r#"
        fn double_all(mut v) {
            let mut i = 0;
            while i < v.len() {
                v[i] *= 2;
                i++;
            }
        }
        let mut v = vec![1, 2, 3];
        double_all(v);
        v[0] + v[1] + v[2]
        "#,
        12, // 2+4+6
    );
}

#[test]
fn test_e2e_vec_stack_operations() {
    assert_aelys_int(
        r#"
        let mut stack = vec![];
        stack.push(1);
        stack.push(2);
        stack.push(3);
        let a = stack.pop().unwrap();
        stack.push(4);
        let b = stack.pop().unwrap();
        let c = stack.pop().unwrap();
        a * 100 + b * 10 + c
        "#,
        342, // 3*100 + 4*10 + 2
    );
}

#[test]
fn test_e2e_float_array_sum() {
    let result = run_aelys(
        r#"
        let arr = [1.5, 2.5, 3.0];
        let mut sum = 0.0;
        let mut i = 0;
        while i < arr.len() {
            sum += arr[i];
            i++;
        }
        sum
        "#,
    );
    assert_eq!(result.as_float(), Some(7.0));
}

#[test]
fn test_e2e_float_vec_push_pop() {
    let result = run_aelys(
        r#"
        let mut v = vec![1.0, 2.0];
        v.push(3.5);
        v.pop().unwrap()
        "#,
    );
    assert_eq!(result.as_float(), Some(3.5));
}

#[test]
fn test_e2e_bool_array_all_true() {
    assert_aelys_bool(
        r#"
        let arr = [true, true, true];
        let mut all = true;
        let mut i = 0;
        while i < arr.len() {
            if arr[i] == false { all = false }
            i++;
        }
        all
        "#,
        true,
    );
}

#[test]
fn test_e2e_bool_array_any_true() {
    assert_aelys_bool(
        r#"
        let arr = [false, true, false];
        let mut any = false;
        let mut i = 0;
        while i < arr.len() {
            if arr[i] { any = true }
            i++;
        }
        any
        "#,
        true,
    );
}

#[test]
fn test_e2e_bool_vec_push_pop() {
    assert_aelys_bool(
        r#"
        let mut v = vec![false, false];
        v.push(true);
        v.pop().unwrap()
        "#,
        true,
    );
}

#[test]
fn test_e2e_single_element_array() {
    assert_aelys_int("let arr = [42]; arr[0]", 42);
    assert_aelys_int("let arr = [42]; arr.len()", 1);
}

#[test]
fn test_e2e_single_element_vec() {
    assert_aelys_int("let mut v = vec![99]; v[0]", 99);
    assert_aelys_int("let mut v = vec![99]; v.len()", 1);
}

#[test]
fn test_e2e_empty_vec_len() {
    assert_aelys_int("let mut v: Vec<int> = vec![]; v.len()", 0);
}

#[test]
fn test_e2e_empty_array_len() {
    assert_aelys_int("let arr: [int; 0] = []; arr.len()", 0);
}

#[test]
fn test_e2e_vec_pop_returns_correct_type() {
    assert_aelys_int(
        r#"
        let mut v = vec![1, 2, 3];
        let x = v.pop().unwrap();
        x * 10
        "#,
        30,
    );
}

#[test]
fn test_e2e_array_len_in_expression() {
    assert_aelys_int("let arr = [1, 2, 3, 4, 5]; arr.len() * 2", 10);
}

#[test]
fn test_e2e_vec_len_in_condition() {
    assert_aelys_int(
        r#"
        let mut v = vec![1, 2, 3];
        if v.len() > 2 { 100 } else { 0 }
        "#,
        100,
    );
}


#[test]
fn test_e2e_vec_push_string() {
    assert_aelys_str(
        r#"
        let mut v = vec![]
        v.push("hello")
        v.push("world")
        v[0]
        "#,
        "hello",
    );
}

#[test]
fn test_e2e_vec_push_string_in_typed_fn() {
    assert_aelys_int(
        r#"
        fn build(n: int) -> int {
            let mut buffer = vec![]
            for i in 0..n {
                buffer.push("-")
            }
            return buffer.len()
        }
        build(5)
        "#,
        5,
    );
}

#[test]
fn test_e2e_vec_push_float() {
    let result = run_aelys(
        r#"
        let mut v = vec![]
        v.push(1.5)
        v.push(2.5)
        v[0]
        "#,
    );
    assert_eq!(result.as_float(), Some(1.5));
}

#[test]
fn test_e2e_vec_push_bool() {
    assert_aelys_bool(
        r#"
        let mut v = vec![]
        v.push(true)
        v.push(false)
        v[0]
        "#,
        true,
    );
}

#[test]
fn test_e2e_vec_push_then_modify() {
    assert_aelys_str(
        r#"
        let mut v = vec![]
        v.push("a")
        v.push("b")
        v[1] = "z"
        v[1]
        "#,
        "z",
    );
}

#[test]
fn test_e2e_vec_push_in_untyped_fn() {
    assert_aelys_int(
        r#"
        fn build_vec() {
            let mut v = vec![]
            v.push(10)
            v.push(20)
            v.push(30)
            return v.len()
        }
        build_vec()
        "#,
        3,
    );
}


#[test]
fn test_e2e_2d_array_basic() {
    assert_aelys_int(
        r#"
        let matrix = [[1, 2, 3], [4, 5, 6], [7, 8, 9]];
        matrix[0][0]
        "#,
        1,
    );
}

#[test]
fn test_e2e_2d_array_access() {
    assert_aelys_int("let m = [[1, 2], [3, 4]]; m[0][1]", 2);
    assert_aelys_int("let m = [[1, 2], [3, 4]]; m[1][0]", 3);
    assert_aelys_int("let m = [[1, 2], [3, 4]]; m[1][1]", 4);
}

#[test]
fn test_e2e_2d_array_write() {
    assert_aelys_int(
        r#"
        let mut m = [[1, 2], [3, 4]];
        m[0][1] = 99;
        m[0][1]
        "#,
        99,
    );
}

#[test]
fn test_e2e_2d_array_row_access() {
    assert_aelys_int(
        r#"
        let matrix = [[10, 20, 30], [40, 50, 60]];
        let row = matrix[1];
        row[2]
        "#,
        60,
    );
}

#[test]
fn test_e2e_2d_array_sum() {
    assert_aelys_int(
        r#"
        let m = [[1, 2, 3], [4, 5, 6]];
        let mut sum = 0;
        let mut i = 0;
        while i < 2 {
            let mut j = 0;
            while j < 3 {
                sum += m[i][j];
                j++;
            }
            i++;
        }
        sum
        "#,
        21, // 1+2+3+4+5+6
    );
}

#[test]
fn test_e2e_2d_vec_basic() {
    assert_aelys_int(
        r#"
        let mut grid = vec![vec![1, 2], vec![3, 4]];
        grid[0][1]
        "#,
        2,
    );
}

#[test]
fn test_e2e_2d_vec_push() {
    assert_aelys_int(
        r#"
        let mut grid = vec![vec![1, 2], vec![3, 4]];
        grid[0].push(99);
        grid[0][2]
        "#,
        99,
    );
}

#[test]
fn test_e2e_2d_vec_modify() {
    assert_aelys_int(
        r#"
        let mut grid = vec![vec![1, 2], vec![3, 4]];
        grid[1][0] = 100;
        grid[1][0]
        "#,
        100,
    );
}

#[test]
fn test_e2e_3d_array() {
    assert_aelys_int(
        r#"
        let mut cube = [[[1, 2], [3, 4]], [[5, 6], [7, 8]]];
        cube[1][0][1]
        "#,
        6, // cube[1][0][1] = 6
    );
}

#[test]
fn test_e2e_3d_array_write() {
    assert_aelys_int(
        r#"
        let mut cube = [[[1, 2], [3, 4]], [[5, 6], [7, 8]]];
        cube[0][1][0] = 999;
        cube[0][1][0]
        "#,
        999,
    );
}

#[test]
fn test_e2e_2d_array_in_function() {
    assert_aelys_int(
        r#"
        fn get_element(matrix, row, col) {
            return matrix[row][col]
        }
        let m = [[10, 20, 30], [40, 50, 60]];
        get_element(m, 1, 2)
        "#,
        60,
    );
}

#[test]
fn test_e2e_2d_array_transpose() {
    assert_aelys_int(
        r#"
        let m = [[1, 2], [3, 4]];
        let mut t = [[0, 0], [0, 0]];
        let mut i = 0;
        while i < 2 {
            let mut j = 0;
            while j < 2 {
                t[j][i] = m[i][j];
                j++;
            }
            i++;
        }
        t[0][1] + t[1][0]
        "#,
        5, // t[0][1]=3, t[1][0]=2, sum=5
    );
}

#[test]
fn test_e2e_mixed_array_vec() {
    assert_aelys_int(
        r#"
        let data = [vec![1, 2], vec![3, 4, 5]];
        data[1][2]
        "#,
        5,
    );
}

#[test]
fn test_e2e_vec_of_arrays() {
    assert_aelys_int(
        r#"
        let data = vec![[1, 2], [3, 4]];
        data[0][1]
        "#,
        2,
    );
}

#[test]
fn test_e2e_2d_array_float() {
    let result = run_aelys(
        r#"
        let m = [[1.0, 2.0], [3.0, 4.0]];
        m[0][1] + m[1][1]
        "#,
    );
    assert_eq!(result.as_float(), Some(6.0));
}

#[test]
fn test_e2e_2d_array_bool() {
    assert_aelys_bool(
        r#"
        let grid = [[true, false], [false, true]];
        grid[0][0]
        "#,
        true,
    );
}

#[test]
fn test_e2e_2d_array_len() {
    assert_aelys_int(
        r#"
        let m = [[1, 2, 3], [4, 5, 6]];
        let rows = m.len();
        let cols = m[0].len();
        rows * 10 + cols
        "#,
        23, // 2 rows, 3 cols -> 23
    );
}

#[test]
fn test_e2e_2d_array_computed_index() {
    assert_aelys_int(
        r#"
        let m = [[1, 2, 3], [4, 5, 6], [7, 8, 9]];
        let i = 1;
        let j = 2;
        m[i][j]
        "#,
        6,
    );
}

#[test]
fn test_e2e_2d_array_nested_computed() {
    assert_aelys_int(
        r#"
        let indices = [[0, 1], [1, 0]];
        let data = [[10, 20], [30, 40]];
        let row = indices[0][1];
        let col = indices[1][0];
        data[row][col]
        "#,
        40, // indices[0][1]=1, indices[1][0]=1, so data[1][1]=40
    );
}

#[test]
fn test_e2e_2d_array_find_max() {
    assert_aelys_int(
        r#"
        let m = [[5, 2, 8], [1, 9, 3], [4, 6, 7]];
        let mut max = m[0][0];
        let mut i = 0;
        while i < 3 {
            let mut j = 0;
            while j < 3 {
                if m[i][j] > max {
                    max = m[i][j];
                }
                j++;
            }
            i++;
        }
        max
        "#,
        9,
    );
}
