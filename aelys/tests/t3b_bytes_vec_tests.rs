mod common;
use common::*;

const PRELUDE: &str = "needs std::bytes\nneeds std::time as time\n";

fn program(body: &str) -> String {
    format!("{PRELUDE}{body}")
}

#[test]
fn a_frame_round_trips_through_a_byte_buffer() {
    assert_aelys_str(
        &program(
            r#"
let mut frame: Vec<int> = vec![0; 64000]
for i in 0..64000 { frame[i] = (i * 7) % 256 }
let buf = bytes::alloc(64010)
bytes::copy_from_vec(buf, 10, frame).unwrap()
let a = bytes::read_u8(buf, 10)
let b = bytes::read_u8(buf, 10 + 37)
let c = bytes::read_u8(buf, 10 + 63999)
let d = bytes::read_u8(buf, 9)
let mut back: Vec<int> = vec![1; 64000]
bytes::copy_to_vec(buf, 10, back).unwrap()
let mut same = 0
for i in 0..64000 { if back[i] == frame[i] { same += 1 } }
"{a} {b} {c} {d} {same} {back.len()}"
"#,
        ),
        "0 3 249 0 64000 64000",
    );
}

#[test]
fn bad_elements_and_ranges_are_errors_and_write_nothing() {
    assert_aelys_str(
        &program(
            r#"
fn bad(r: Result<unit, string>) -> bool { match r { Ok(_) => false, Err(_) => true } }
let buf = bytes::alloc(8)
bytes::fill(buf, 0, 8, 9)
let high = bytes::copy_from_vec(buf, 0, vec![1, 2, 300, 4])
let low = bytes::copy_from_vec(buf, 0, vec![1, -1])
let past = bytes::copy_from_vec(buf, 6, vec![1, 2, 3])
let neg = bytes::copy_from_vec(buf, -1, vec![1])
let mut out: Vec<int> = vec![7, 7, 7]
let read_past = bytes::copy_to_vec(buf, 6, out)
let fits = bytes::copy_from_vec(buf, 5, vec![1, 2, 3])
let empty = bytes::copy_from_vec(buf, 8, vec![])
let e1 = match high { Ok(_) => "ok", Err(m) => m }
let e2 = match low { Ok(_) => "ok", Err(m) => m }
"{e1}|{e2}|{bad(past)} {bad(neg)} {bad(read_past)} {not bad(fits)} {not bad(empty)}|{bytes::read_u8(buf, 0)}{bytes::read_u8(buf, 2)}{bytes::read_u8(buf, 7)}|{out[0]}{out[2]}"
"#,
        ),
        "copy_from_vec: element 2 is 300, not a byte in 0..=255|copy_from_vec: element 1 is -1, not a byte in 0..=255|true true true true true|993|77",
    );
}

#[test]
fn only_a_vec_of_int_is_accepted() {
    for argument in ["vec![\"a\"]", "vec![1.5]", "[1, 2]", "7"] {
        let err = run_aelys_err(&program(&format!(
            "let buf = bytes::alloc(4)\nlet _ = bytes::copy_from_vec(buf, 0, {argument})\n0"
        )));
        assert!(err.contains("error[E0"), "{argument}: {err}");
    }
}

#[test]
fn one_bulk_copy_costs_far_less_than_a_frame_of_single_writes() {
    let source = program(
        r#"
let frame: Vec<int> = vec![200; 64000]
let buf = bytes::alloc(64000)
let t0 = time::now_ms()
for _ in 0..10 { bytes::copy_from_vec(buf, 0, frame).unwrap() }
let t1 = time::now_ms()
for i in 0..64000 { bytes::write_u8(buf, i, frame[i]) }
let t2 = time::now_ms()
"{t1 - t0} {t2 - t1}"
"#,
    );
    let mut vm = aelys::new_vm().expect("a vm starts");
    let value = aelys::run_with_vm_and_opt(
        &mut vm,
        &source,
        "<test>",
        aelys_opt::OptimizationLevel::Standard,
    )
    .expect("the program runs");
    let heap = vm.heap();
    let object = heap
        .get(aelys_runtime::vm::GcRef::new(
            value.as_ptr().expect("a string"),
        ))
        .expect("a live string");
    let aelys_runtime::vm::ObjectKind::String(text) = &object.kind else {
        panic!("a string");
    };
    let mut parts = text.as_str().split(' ').map(|p| p.parse::<i64>().unwrap());
    let (bulk, single) = (parts.next().unwrap(), parts.next().unwrap());
    assert!(
        bulk < single.max(20),
        "ten bulk copies took {bulk} ms against {single} ms for one frame of write_u8"
    );
}
