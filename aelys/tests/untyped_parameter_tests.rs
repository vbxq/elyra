// reductions of the shipped examples that stage 2 rejected with e0354 and no earlier

mod common;

use aelys_sema::TypeError;
use common::assert_aelys_int;
use std::collections::HashSet;

fn inference_errors(text: &str, known_globals: HashSet<String>) -> Vec<TypeError> {
    let source = aelys_syntax::Source::new("<untyped-param>", text);
    let tokens = aelys_frontend::lexer::Lexer::with_source(source.clone())
        .scan()
        .expect("the fixture must lex");
    let statements = aelys_frontend::parser::Parser::new(tokens, source.clone())
        .parse()
        .expect("the fixture must parse");
    match aelys_sema::TypeInference::infer_program_full(
        statements,
        source,
        HashSet::new(),
        known_globals,
    ) {
        Ok(_) => Vec::new(),
        Err(errors) => errors,
    }
}

const HISTOGRAM: &str = r#"
fn count_frequency(data, num_bins: int) {
    let mut bins = vec![]
    for i in 0..num_bins {
        bins.push(0)
    }
    for i in 0..data.len() {
        let bin_idx = data[i]
        if bin_idx >= 0 and bin_idx < num_bins {
            bins[bin_idx] = bins[bin_idx] + 1
        }
    }
    return bins
}

let scores = [0, 1, 1, 2, 2, 2]
let bins = count_frequency(scores, 3)
bins[2]
"#;

const ARRAY_SUM: &str = r#"
fn fill_array(n: int) {
    let mut arr = vec![]
    let mut i = 0
    while i < n {
        arr.push(i)
        i = i + 1
    }
    return arr
}

fn sum_array(arr) -> int {
    let mut sum = 0
    let mut i = 0
    let len = arr.len()
    while i < len {
        sum = sum + arr[i]
        i = i + 1
    }
    return sum
}

let n = 10
let arr = fill_array(n)
let sum = sum_array(arr)
sum
"#;

const MATRIX_MULT: &str = r#"
fn create_matrix(n: int, val: int) {
    let mut matrix = vec![]
    let size = n * n
    let mut i = 0
    while i < size {
        matrix.push(val)
        i = i + 1
    }
    return matrix
}

fn matrix_mult(a, b, n: int) {
    let mut c = create_matrix(n, 0)
    let mut i = 0
    while i < n {
        let mut j = 0
        while j < n {
            let mut sum = 0
            let mut k = 0
            while k < n {
                sum = sum + a[i * n + k] * b[k * n + j]
                k = k + 1
            }
            c[i * n + j] = sum
            j = j + 1
        }
        i = i + 1
    }
    return c
}

let n = 2
let a = create_matrix(n, 2)
let b = create_matrix(n, 3)
let c = matrix_mult(a, b, n)
c[0]
"#;

const ARRAY_SORT: &str = r#"
fn fill_descending(n: int) {
    let mut arr = vec![]
    let mut i = n
    while i > 0 {
        arr.push(i)
        i = i - 1
    }
    return arr
}

fn bubble_sort(mut arr) {
    let n = arr.len()
    let mut i = 0
    while i < n - 1 {
        let mut j = 0
        while j < n - i - 1 {
            if arr[j] > arr[j + 1] {
                let tmp = arr[j]
                arr[j] = arr[j + 1]
                arr[j + 1] = tmp
            }
            j = j + 1
        }
        i = i + 1
    }
}

let n = 5
let mut arr = fill_descending(n)
bubble_sort(arr)
arr[0]
"#;

#[test]
fn histogram_reduction_compiles_and_counts() {
    assert_aelys_int(HISTOGRAM, 3);
}

#[test]
fn array_sum_reduction_compiles_and_sums() {
    assert_aelys_int(ARRAY_SUM, 45);
}

#[test]
fn matrix_mult_reduction_compiles_and_multiplies() {
    assert_aelys_int(MATRIX_MULT, 12);
}

#[test]
fn array_sort_reduction_compiles_and_sorts() {
    assert_aelys_int(ARRAY_SORT, 1);
}

#[test]
fn untyped_parameters_still_add_two_ints() {
    assert_aelys_int("fn add(a, b) { a + b }\nadd(2, 3)", 5);
}

const UNRESOLVED_PARAMETER: &str = r#"fn never_pinned(shape) -> int {
    return 1
}
0
"#;

#[test]
fn an_unresolved_parameter_points_at_the_parameter() {
    let errors = inference_errors(UNRESOLVED_PARAMETER, HashSet::new());
    assert_eq!(
        errors.len(),
        1,
        "expected exactly one diagnostic, got {:?}",
        errors.iter().map(ToString::to_string).collect::<Vec<_>>()
    );
    let error = &errors[0];
    assert_eq!(error.diagnostic_code(), 353);

    let start = UNRESOLVED_PARAMETER
        .find("shape")
        .expect("the fixture names the parameter");
    assert_eq!(
        (error.span.start, error.span.end),
        (start, start + "shape".len()),
        "the caret must sit on the parameter, not the whole function"
    );

    let rendered = error.to_string();
    assert!(
        !rendered.contains("fix the errors above"),
        "the only diagnostic must not point at diagnostics that do not exist: {rendered}"
    );
}

const SIGNATURELESS_GLOBAL: &str = r#"fn use_it() -> int {
    return store(1, 2, 3)
}
use_it()
"#;

#[test]
fn a_global_without_a_signature_names_itself() {
    let mut known = HashSet::new();
    known.insert("store".to_string());
    let errors = inference_errors(SIGNATURELESS_GLOBAL, known);
    let error = errors.first().expect("the fixture must be rejected");
    assert_eq!(error.diagnostic_code(), 376);

    let start = SIGNATURELESS_GLOBAL
        .find("store")
        .expect("the fixture names the global");
    assert_eq!(
        (error.span.start, error.span.end),
        (start, start + "store".len()),
        "the caret must sit on the unusable global"
    );

    let rendered = error.to_string();
    assert!(
        rendered.contains("store"),
        "the diagnostic must name the global: {rendered}"
    );
    assert!(
        !rendered.contains("fix the errors above"),
        "there is no earlier error to fix: {rendered}"
    );
}

#[test]
fn no_diagnostic_blames_an_error_that_was_never_reported() {
    for fixture in [HISTOGRAM, ARRAY_SUM, MATRIX_MULT, ARRAY_SORT] {
        let errors = inference_errors(fixture, HashSet::new());
        assert!(
            errors.is_empty(),
            "fixture must type check, got {:?}",
            errors.iter().map(ToString::to_string).collect::<Vec<_>>()
        );
    }
    let errors = inference_errors(UNRESOLVED_PARAMETER, HashSet::new());
    assert!(
        errors.iter().all(|error| error.diagnostic_code() != 354),
        "E0354 may only follow a diagnostic that was actually reported"
    );
}
