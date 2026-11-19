mod t9_support;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use t9_support::{Outcome, cli_binary, run_with};

// every expected value below follows from the C standard or was worked out by hand, never read back from the VM
const MODES: [&[&str]; 5] = [
    &["-O0"],
    &["-O2"],
    &["-ae.jit=on"],
    &["-ae.jit-sync=on"],
    &["-ae.gc-stress=on", "-ae.gc-verify=on"],
];

const TEST_LIBRARY: &str = r#"
#include <stdarg.h>
#include <stdint.h>
#include <string.h>

int8_t id_i8(int8_t x) { return x; }
int16_t id_i16(int16_t x) { return x; }
int32_t id_i32(int32_t x) { return x; }
int64_t id_i64(int64_t x) { return x; }
uint8_t id_u8(uint8_t x) { return x; }
uint16_t id_u16(uint16_t x) { return x; }
uint32_t id_u32(uint32_t x) { return x; }
uint64_t id_u64(uint64_t x) { return x; }
int64_t widen_i8(int8_t x) { return x; }
int64_t widen_u16(uint16_t x) { return x; }
int negate(_Bool b) { return !b; }
_Bool is_odd(int32_t x) { return (x & 1) != 0; }
float half_f32(float x) { return x / 2.0f; }

int64_t sum0(void) { return 7; }
int64_t sum1(int64_t a) { return a; }
int64_t sum2(int64_t a, int64_t b) { return a + 2 * b; }
int64_t sum3(int64_t a, int64_t b, int64_t c) { return a + 2 * b + 3 * c; }
int64_t sum4(int64_t a, int64_t b, int64_t c, int64_t d) { return a + 2 * b + 3 * c + 4 * d; }
int64_t sum6(int64_t a, int64_t b, int64_t c, int64_t d, int64_t e, int64_t f) {
    return a + 2 * b + 3 * c + 4 * d + 5 * e + 6 * f;
}
int64_t sum8(int64_t a, int64_t b, int64_t c, int64_t d, int64_t e, int64_t f, int64_t g, int64_t h) {
    return a + 2 * b + 3 * c + 4 * d + 5 * e + 6 * f + 7 * g + 8 * h;
}
int64_t sum10(int64_t a, int64_t b, int64_t c, int64_t d, int64_t e, int64_t f, int64_t g, int64_t h,
              int64_t i, int64_t j) {
    return a + 2 * b + 3 * c + 4 * d + 5 * e + 6 * f + 7 * g + 8 * h + 9 * i + 10 * j;
}
double mix12(int32_t a, double b, uint8_t c, float d, int64_t e, double f, int16_t g, double h,
             double i, double j, double k, double l) {
    return a + 2 * b + 3 * c + 4 * d + 5 * e + 6 * f + 7 * g + 8 * h + 9 * i + 10 * j + 11 * k + 12 * l;
}
double fsum10(double a, double b, double c, double d, double e, double f, double g, double h,
              double i, double j) {
    return a + 2 * b + 3 * c + 4 * d + 5 * e + 6 * f + 7 * g + 8 * h + 9 * i + 10 * j;
}
int64_t vsum(int n, ...) {
    va_list args;
    va_start(args, n);
    int64_t total = 0;
    for (int i = 0; i < n; i++) total += (i + 1) * va_arg(args, int64_t);
    va_end(args);
    return total;
}
double vfsum(int n, ...) {
    va_list args;
    va_start(args, n);
    double total = 0;
    for (int i = 0; i < n; i++) total += (i + 1) * va_arg(args, double);
    va_end(args);
    return total;
}

struct point { int32_t x; int32_t y; double weight; };
void point_init(struct point *p, int32_t x, int32_t y, double weight) {
    p->x = x;
    p->y = y;
    p->weight = weight;
}
double point_score(const struct point *p) { return p->x * 10 + p->y + p->weight; }
int64_t checksum(const uint8_t *bytes, uint64_t n) {
    int64_t total = 0;
    for (uint64_t i = 0; i < n; i++) total += (int64_t)(i + 1) * bytes[i];
    return total;
}
void fill_squares(uint8_t *bytes, uint64_t n) {
    for (uint64_t i = 0; i < n; i++) bytes[i] = (uint8_t)(i * i);
}
const char *greeting(void) { return "h\xc3\xa9llo"; }
const char *broken(void) { return "ab\xff"; }
const char *nothing(void) { return 0; }
size_t length_of(const char *text) { return strlen(text); }
"#;

struct Library {
    dir: tempfile::TempDir,
    path: PathBuf,
}

// the library is built once per test binary; without a C compiler every test that needs it says so and passes
fn library() -> Option<&'static Library> {
    static LIBRARY: OnceLock<Option<Library>> = OnceLock::new();
    LIBRARY
        .get_or_init(|| {
            let dir = tempfile::tempdir().expect("temporary directory");
            let source = dir.path().join("ffitest.c");
            std::fs::write(&source, TEST_LIBRARY).expect("write C source");
            let path = dir.path().join(format!(
                "{}ffitest{}",
                std::env::consts::DLL_PREFIX,
                std::env::consts::DLL_SUFFIX
            ));
            let compiler = std::env::var("CC").unwrap_or_else(|_| "cc".to_string());
            let built = Command::new(&compiler)
                .args(["-shared", "-fPIC", "-O2", "-o"])
                .arg(&path)
                .arg(&source)
                .status();
            match built {
                Ok(status) if status.success() => Some(Library { dir, path }),
                Ok(status) => panic!("{compiler} failed to build the FFI test library: {status}"),
                Err(error) => {
                    eprintln!("SKIPPED: no C compiler ({compiler}: {error}); the FFI test library was not built");
                    None
                }
            }
        })
        .as_ref()
}

fn header(library: &Library, declarations: &str) -> String {
    format!(
        "extern \"{}\" {{\n{declarations}\n}}\n",
        library.path.display()
    )
}

fn assert_lines(source: &str, expected: &str) {
    let expected: Vec<&str> = expected.lines().collect();
    for flags in MODES {
        let outcome = run_with(source, flags);
        assert_eq!(
            outcome.code,
            Some(0),
            "flags {flags:?}\nstdout: {}\nstderr: {}",
            outcome.stdout,
            outcome.stderr
        );
        assert_eq!(
            outcome.lines(),
            expected,
            "flags {flags:?}\nstderr: {}",
            outcome.stderr
        );
    }
}

fn assert_runtime_error(source: &str, message: &str) {
    for flags in MODES {
        let outcome = run_with(source, flags);
        assert_eq!(
            outcome.code,
            Some(1),
            "flags {flags:?}: {source}\nstdout: {}\nstderr: {}",
            outcome.stdout,
            outcome.stderr
        );
        assert!(
            outcome.stderr.contains(message),
            "flags {flags:?}: {source}\nwanted: {message}\nstderr: {}",
            outcome.stderr
        );
    }
}

fn assert_refused(source: &str, code: &str, message: &str) {
    let outcome = run_with(source, &[]);
    assert_ne!(outcome.code, Some(0), "{source}");
    assert!(
        outcome.stderr.contains(code) && outcome.stderr.contains(message),
        "{source}\nwanted: {code} {message}\nstderr: {}",
        outcome.stderr
    );
}

fn run_in(dir: &Path, name: &str, source: &str) -> Outcome {
    let script = dir.join(name);
    let mut file = std::fs::File::create(&script).expect("script file");
    file.write_all(source.as_bytes()).expect("write script");
    let output = Command::new(cli_binary())
        .arg("run")
        .arg(&script)
        .output()
        .expect("aelys-cli should run");
    Outcome {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        code: output.status.code(),
    }
}

#[test]
fn libc_scalars_strings_and_variadics() {
    let source = r#"extern "c" {
    fn abs(x: i32) -> i32
    fn labs(x: int) -> int
    fn strlen(s: cstr) -> u64
    fn atoi(s: cstr) -> i32
    fn snprintf(buf: &mut [u8], n: u64, fmt: cstr, ...) -> i32
}
println(abs(-5))
let n = -42
println(abs(n))
let small: i8 = -7
println(abs(small as i32))
println(labs(-9223372036854775807))
println(strlen("hello"))
println(strlen(""))
println(strlen("h\u{e9}"))
println(atoi("  1234xyz"))
let mut buf: Vec<u8> = vec![0; 64]
let written = snprintf(&mut buf, 64, "%s|%lld|%.3f|%d|%u", "abc", -9000000000, 2.5, 7 as i32, 4000000000 as u32)
println(written)
buf.truncate(written as int)
println(buf.to_utf8().unwrap())
let mut tiny: Vec<u8> = vec![0; 4]
println(snprintf(&mut tiny, 4, "%s", "truncated"))
println(tiny[3])
"#;
    assert_lines(
        source,
        "5\n42\n7\n9223372036854775807\n5\n0\n3\n1234\n34\nabc|-9000000000|2.500|7|4000000000\n9\n0",
    );
}

#[test]
fn libm_doubles_and_floats() {
    let source = r#"extern "m" {
    fn cos(x: float) -> float
    fn pow(x: float, y: float) -> float
    fn sqrt(x: f64) -> f64
    fn cosf(x: f32) -> f32
    fn sqrtf(x: f32) -> f32
}
println(cos(0.0))
println(pow(2.0, 10.0))
println(pow(10.0, -2.0))
println(sqrt(2.0))
println(cosf(0.0))
let quarter: f32 = 0.25
println(sqrtf(quarter))
println(sqrtf(2.0) as float)
"#;
    assert_lines(
        source,
        "1.0\n1024.0\n0.01\n1.4142135623730951\n1.0\n0.5\n1.4142135381698608",
    );
}

#[test]
fn getenv_returns_a_copied_cstr_and_a_nullable_ptr() {
    let home = std::env::var("HOME").expect("HOME is set for the test process");
    let source = r#"extern "c" {
    fn getenv(name: cstr) -> cstr
}
extern "c" { fn secure_getenv(name: cstr) -> ptr }
println(getenv("HOME"))
let missing = secure_getenv("AELYS_FFI_SURELY_UNSET_VARIABLE")
println(missing.is_null())
let home = secure_getenv("HOME")
println(ptr::to_string(home) == getenv("HOME"))
"#;
    assert_lines(source, &format!("{home}\ntrue\ntrue"));
    assert_runtime_error(
        "extern \"c\" { fn getenv(name: cstr) -> cstr }\nprintln(getenv(\"AELYS_FFI_SURELY_UNSET_VARIABLE\"))\n",
        "C function 'getenv': returned a null cstr",
    );
}

#[test]
fn malloc_free_and_pointer_access() {
    let source = r#"extern "c" {
    fn malloc(n: u64) -> ptr
    fn free(p: ptr)
    fn memset(p: ptr, c: i32, n: u64) -> ptr
}
let p = malloc(32)
println(p.is_null())
memset(p, 0, 32)
p.write_i32(0, -123456)
p.write_u8(4, 255)
p.write_i16(6, -2)
p.write_u64(8, 18446744073709551615)
p.write_f64(16, 6.25)
p.write_f32(24, 1.5)
println(p.read_i32(0))
println(p.read_u8(4))
println(p.read_i8(4))
println(p.read_u16(6))
println(p.read_i16(6))
println(p.read_u64(8))
println(p.read_i64(8))
println(p.read_int(8))
println(p.read_u32(8))
println(p.read_f64(16))
println(p.read_f32(24))
let q = p.offset(16)
println(q.read_f64(0))
println(q.offset(-16) == p)
println(q.addr() - p.addr())
p.write_ptr(0, q)
println(p.read_ptr(0) == q)
free(p)
let z = ptr::null()
println(z.is_null())
println(z.addr())
println(ptr::from_addr(4096).addr())
let a = ptr::alloc(3)
println(a.read_u8(0) + a.read_u8(1) + a.read_u8(2))
ptr::free(a)
"#;
    assert_lines(
        source,
        "false\n-123456\n255\n-1\n65534\n-2\n18446744073709551615\n-1\n-1\n4294967295\n6.25\n1.5\n6.25\ntrue\n16\ntrue\ntrue\n0\n4096\n0",
    );
}

#[test]
fn pointer_misuse_is_a_runtime_error() {
    assert_runtime_error(
        "let p = ptr::null()\nprintln(p.read_i32(0))\n",
        "ptr.read_i32: access through a null ptr",
    );
    assert_runtime_error(
        "let p = ptr::null()\np.write_u8(0, 1)\n",
        "ptr.write_u8: access through a null ptr",
    );
    assert_runtime_error(
        "let p = ptr::alloc(8)\nlet v = 256\np.write_u8(0, v)\n",
        "ptr.write_u8: value 256 does not fit in u8",
    );
    assert_runtime_error(
        "let p = ptr::alloc(8)\nlet v = -1\np.write_u32(0, v)\n",
        "ptr.write_u32: value -1 does not fit in u32",
    );
    assert_runtime_error(
        "let p = ptr::from_addr(8)\nprintln(p.offset(-9).addr())\n",
        "ptr.offset: offset -9 moves the address outside the address space",
    );
    assert_runtime_error(
        "println(ptr::from_addr(-1).addr())\n",
        "ptr::from_addr: -1 is not an address",
    );
    assert_runtime_error(
        "println(ptr::to_string(ptr::null()))\n",
        "ptr::to_string: read a null cstr",
    );
}

#[test]
fn byte_buffers_are_lent_to_memset_and_memcpy() {
    let source = r#"extern "c" {
    fn memset(b: &mut [u8], c: i32, n: u64) -> ptr
    fn memcpy(dst: &mut [u8], src: &[u8], n: u64) -> ptr
    fn memcmp(a: &[u8], b: &[u8], n: u64) -> i32
}
let mut a: Vec<u8> = vec![0; 8]
memset(&mut a, 171, 5)
println(a)
let src: Vec<u8> = vec![1, 2, 3, 4, 5, 6, 7, 8]
let mut dst: Vec<u8> = vec![0; 8]
memcpy(&mut dst, &src, 8)
println(dst)
println(memcmp(&src, &dst, 8))
println(memcmp(&src, &src, 8))
dst[7] = 9
println(memcmp(&src, &dst, 8) < 0)
let mut empty: Vec<u8> = vec![]
memset(&mut empty, 1, 0)
println(empty.len())
"#;
    assert_lines(
        source,
        "[171, 171, 171, 171, 171, 0, 0, 0]\n[1, 2, 3, 4, 5, 6, 7, 8]\n0\n0\ntrue\n0",
    );
}

#[test]
fn a_mutable_buffer_lent_twice_through_a_function_value_is_refused_at_run_time() {
    assert_runtime_error(
        "extern \"c\" { fn memcpy(d: &mut [u8], s: &[u8], n: u64) -> ptr }\nlet mut v: Vec<u8> = vec![1, 2, 3, 4]\nlet f = memcpy\nf(v, v, 4)\n",
        "C function 'memcpy': argument 2: byte buffer borrowed mutably while another borrow of it is live in the same call",
    );
}

#[test]
fn every_c_integer_type_converts_exactly_and_refuses_what_does_not_fit() {
    let Some(library) = library() else { return };
    let declarations = "fn id_i8(x: i8) -> i8\nfn id_i16(x: i16) -> i16\nfn id_i32(x: i32) -> i32\nfn id_i64(x: i64) -> i64\nfn id_u8(x: u8) -> u8\nfn id_u16(x: u16) -> u16\nfn id_u32(x: u32) -> u32\nfn id_u64(x: u64) -> u64\nfn widen_i8(x: i8) -> int\nfn widen_u16(x: u16) -> int\nfn negate(b: bool) -> i32\nfn is_odd(x: i32) -> bool\nfn half_f32(x: f32) -> f32";
    let body = r#"let edges = [-128, 127]
for e in edges { println(id_i8(e)) }
for e in [-32768, 32767] { println(id_i16(e)) }
for e in [-2147483648, 2147483647] { println(id_i32(e)) }
for e in [-9223372036854775807 - 1, 9223372036854775807] { println(id_i64(e)) }
for e in [0, 255] { println(id_u8(e)) }
for e in [0, 65535] { println(id_u16(e)) }
for e in [0, 4294967295] { println(id_u32(e)) }
println(id_u64(0))
println(id_u64(18446744073709551615))
let wide: u64 = 9223372036854775808
println(id_u64(wide))
let typed: i8 = -100
println(id_i8(typed))
let sized: u16 = 40000
println(widen_u16(sized))
println(widen_i8(-1))
println(negate(true))
println(negate(false))
println(is_odd(3))
println(is_odd(-4))
println(half_f32(3.0))
"#;
    let source = header(library, declarations) + body;
    assert_lines(
        &source,
        "-128\n127\n-32768\n32767\n-2147483648\n2147483647\n-9223372036854775808\n9223372036854775807\n0\n255\n0\n65535\n0\n4294967295\n0\n18446744073709551615\n9223372036854775808\n-100\n40000\n-1\n0\n1\ntrue\nfalse\n1.5",
    );
    let refusals = [
        ("id_i8", "-129", "i8"),
        ("id_i8", "128", "i8"),
        ("id_i16", "-32769", "i16"),
        ("id_i16", "32768", "i16"),
        ("id_i32", "-2147483649", "i32"),
        ("id_i32", "2147483648", "i32"),
        ("id_u8", "-1", "u8"),
        ("id_u8", "256", "u8"),
        ("id_u16", "-1", "u16"),
        ("id_u16", "65536", "u16"),
        ("id_u32", "-1", "u32"),
        ("id_u32", "4294967296", "u32"),
        ("id_u64", "-1", "u64"),
    ];
    for (function, value, ty) in refusals {
        let source =
            header(library, declarations) + &format!("let v = {value}\nprintln({function}(v))\n");
        assert_runtime_error(
            &source,
            &format!("C function '{function}': argument 1: {value} does not fit in {ty}"),
        );
    }
    let above =
        header(library, declarations) + "let v: u64 = 9223372036854775808\nprintln(id_i64(v))\n";
    assert_runtime_error(
        &above,
        "C function 'id_i64': argument 1: 9223372036854775808 does not fit in i64",
    );
    assert_refused(
        &(header(library, declarations) + "println(id_u8(256))\n"),
        "E0301",
        "integer literal 256 does not fit in u8",
    );
}

#[test]
fn register_and_stack_arguments_up_to_twelve() {
    let Some(library) = library() else { return };
    let declarations = "fn sum0() -> int\nfn sum1(a: int) -> int\nfn sum2(a: int, b: int) -> int\nfn sum3(a: int, b: int, c: int) -> int\nfn sum4(a: int, b: int, c: int, d: int) -> int\nfn sum6(a: int, b: int, c: int, d: int, e: int, f: int) -> int\nfn sum8(a: int, b: int, c: int, d: int, e: int, f: int, g: int, h: int) -> int\nfn sum10(a: int, b: int, c: int, d: int, e: int, f: int, g: int, h: int, i: int, j: int) -> int\nfn mix12(a: i32, b: float, c: u8, d: f32, e: i64, f: float, g: i16, h: float, i: float, j: float, k: float, l: float) -> float\nfn fsum10(a: float, b: float, c: float, d: float, e: float, f: float, g: float, h: float, i: float, j: float) -> float\nfn vsum(n: i32, ...) -> int\nfn vfsum(n: i32, ...) -> float";
    let body = r#"println(sum0())
println(sum1(5))
println(sum2(1, 1))
println(sum3(1, 1, 1))
println(sum4(1, 1, 1, 1))
println(sum6(1, 1, 1, 1, 1, 1))
println(sum8(1, 1, 1, 1, 1, 1, 1, 1))
println(sum10(1, 1, 1, 1, 1, 1, 1, 1, 1, -1))
println(mix12(1, 0.5, 200, 0.25, -3, 1.0, -2, 0.125, 1.0, 1.0, 1.0, 1.0))
println(fsum10(1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.5))
println(vsum(3, 10, 20, 30))
println(vsum(10, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1))
println(vfsum(2, 0.5, 0.25))
println(vfsum(10, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 2.0))
fn hot(n: int) -> int {
    let mut total = 0
    let mut i = 0
    while i < n {
        total += sum2(i, 1) + vsum(2, i, 1)
        i += 1
    }
    total
}
println(hot(20000))
"#;
    let source = header(library, declarations) + body;
    assert_lines(
        &source,
        "7\n5\n3\n6\n10\n21\n36\n35\n623.0\n50.0\n140\n55\n1.0\n65.0\n400060000",
    );
}

#[test]
fn structs_cross_by_pointer_and_buffers_by_slice() {
    let Some(library) = library() else { return };
    let declarations = "fn point_init(p: ptr, x: i32, y: i32, weight: float)\nfn point_score(p: ptr) -> float\nfn checksum(bytes: &[u8], n: u64) -> int\nfn fill_squares(bytes: &mut [u8], n: u64)\nfn greeting() -> cstr\nfn length_of(text: cstr) -> u64";
    let body = r#"let p = ptr::alloc(16)
point_init(p, 3, -4, 0.5)
println(p.read_i32(0))
println(p.read_i32(4))
println(p.read_f64(8))
println(point_score(p))
p.write_i32(0, 10)
println(point_score(p))
ptr::free(p)
let mut bytes: Vec<u8> = vec![0; 20]
fill_squares(&mut bytes, 20)
println(bytes[19])
println(checksum(&bytes, 20))
println(greeting())
println(greeting().len())
println(length_of("caf\u{e9}"))
"#;
    let source = header(library, declarations) + body;
    let checksum: i64 = (0..20i64).map(|i| (i + 1) * ((i * i) % 256)).sum();
    assert_lines(
        &source,
        &format!("3\n-4\n0.5\n26.5\n96.5\n105\n{checksum}\nh\u{e9}llo\n6\n5"),
    );
}

#[test]
fn a_returned_cstr_must_be_utf8_and_non_null() {
    let Some(library) = library() else { return };
    let declarations = "fn broken() -> cstr\nfn nothing() -> cstr";
    assert_runtime_error(
        &(header(library, declarations) + "println(broken())\n"),
        "C function 'broken': returned a string that is not valid UTF-8 (byte 2 of 3)",
    );
    assert_runtime_error(
        &(header(library, declarations) + "println(nothing())\n"),
        "C function 'nothing': returned a null cstr",
    );
    assert_runtime_error(
        "extern \"c\" { fn strlen(s: cstr) -> u64 }\nprintln(strlen(\"a\\u{0}b\"))\n",
        "C function 'strlen': argument 1: the string holds a NUL byte at offset 1",
    );
}

#[test]
fn a_bare_library_name_is_found_next_to_the_script() {
    let Some(library) = library() else { return };
    let outcome = run_in(
        library.dir.path(),
        "beside.aelys",
        "extern \"ffitest\" { fn sum2(a: int, b: int) -> int }\nprintln(sum2(40, 1))\n",
    );
    assert_eq!(outcome.code, Some(0), "stderr: {}", outcome.stderr);
    assert_eq!(outcome.lines(), vec!["42"]);
}

#[test]
fn a_manifest_names_the_library_file() {
    let Some(library) = library() else { return };
    let dir = tempfile::tempdir().expect("temporary directory");
    std::fs::write(
        dir.path().join("aelys.toml"),
        format!("[library.engine]\npath = \"{}\"\n", library.path.display()),
    )
    .expect("manifest");
    let outcome = run_in(
        dir.path(),
        "manifest.aelys",
        "extern \"engine\" { fn sum1(a: int) -> int }\nprintln(sum1(9))\n",
    );
    assert_eq!(outcome.code, Some(0), "stderr: {}", outcome.stderr);
    assert_eq!(outcome.lines(), vec!["9"]);
}

#[test]
fn a_missing_library_or_symbol_is_named_at_its_first_call() {
    let outcome = run_with(
        "extern \"aelys_no_such_library\" { fn f() }\nprintln(\"before\")\nf()\n",
        &[],
    );
    assert_eq!(outcome.code, Some(1));
    assert_eq!(outcome.lines(), vec!["before"]);
    assert!(
        outcome.stderr.contains(
            "cannot load C library 'aelys_no_such_library': tried libaelys_no_such_library.so"
        ),
        "{}",
        outcome.stderr
    );
    assert_runtime_error(
        "extern \"c\" { fn aelys_no_such_symbol() }\naelys_no_such_symbol()\n",
        "has no symbol 'aelys_no_such_symbol'",
    );
    let unused = run_with(
        "extern \"aelys_no_such_library\" { fn f() }\nprintln(\"never called\")\n",
        &[],
    );
    assert_eq!(unused.code, Some(0), "{}", unused.stderr);
}

#[test]
fn extern_functions_are_values_and_hoisted() {
    let source = r#"fn early() -> i32 { abs(-11) }
println(early())
extern "c" { fn abs(x: i32) -> i32 }
let f = abs
let g: fn(i32) -> i32 = abs
println(f(-1) + g(-2))
fn apply(h: fn(i32) -> i32, x: i32) -> i32 { h(x) }
println(apply(abs, -30))
"#;
    assert_lines(source, "11\n3\n30");
}

#[test]
fn ptr_stays_opaque() {
    assert_refused(
        "let p = ptr::null()\nprintln(p + 1)\n",
        "E0301",
        "binary operator '+'",
    );
    assert_refused(
        "let p = ptr::null()\nlet n: int = p\n",
        "E0301",
        "expected int, found ptr",
    );
    assert_refused(
        "let p = ptr::null()\nprintln(p as int)\n",
        "E0301",
        "invalid cast",
    );
    assert_refused(
        "let p = ptr::null()\nprintln(p)\n",
        "E0338",
        "trait 'Display' is not implemented for ptr",
    );
    assert_lines(
        "struct Handle { p: ptr }\nlet h = Handle { p: ptr::null() }\nlet v: Vec<ptr> = vec![h.p, ptr::from_addr(16)]\nlet o: Option<ptr> = Some(v[1])\nprintln(o.unwrap().addr())\nprintln(v[0] == h.p)\nprintln(v[0] != v[1])\n",
        "16\ntrue\ntrue",
    );
    assert_lines("let ptr = 3\nprintln(ptr + 1)\n", "4");
}

#[test]
fn extern_declarations_are_checked() {
    assert_refused(
        "extern \"c\" { fn f(s: string) }\n",
        "E0490",
        "type `string` cannot cross the C boundary (parameter 1 `s` of `f`)",
    );
    assert_refused(
        "struct P { x: int }\nextern \"c\" { fn f(p: P) }\n",
        "E0490",
        "structs are not passed by value",
    );
    assert_refused(
        "extern \"c\" { fn f(v: Vec<u8>) }\n",
        "E0490",
        "a byte buffer is `&[u8]` or `&mut [u8]`",
    );
    assert_refused(
        "extern \"c\" { fn f(v: &[int]) }\n",
        "E0490",
        "type `&[int]` cannot cross",
    );
    assert_refused(
        "extern \"c\" { fn f() -> &[u8] }\n",
        "E0490",
        "a returned pointer is a `ptr`",
    );
    assert_refused(
        "extern \"c\" { fn qsort(b: ptr, n: u64, s: u64, cmp: fn(ptr, ptr) -> i32) }\n",
        "E0491",
        "parameter `cmp` of `qsort` is a function: C callbacks into Elyra are not supported",
    );
    assert_refused(
        "fn h() {\n    extern \"c\" { fn abs(x: i32) -> i32 }\n}\n",
        "E0492",
        "only allowed at the top level",
    );
    assert_refused(
        "extern \"c\" { fn printf(f: cstr, ...) -> i32 }\nlet p = printf\n",
        "E0493",
        "variadic C function `printf` can only be called directly",
    );
    assert_refused(
        "extern \"c\" { fn printf(f: cstr, ...) -> i32 }\nprintf(\"%d\", vec![1])\n",
        "E0494",
        "a value of type Vec<int> cannot be passed in the `...` part of `printf`",
    );
    assert_refused(
        "fn g() {}\nextern \"c\" { fn g() }\n",
        "E0495",
        "`g` is declared twice",
    );
    assert_refused(
        "extern \"c\" { fn abs(x: i32) -> i32 }\nextern \"m\" { fn abs(x: i32) -> i32 }\n",
        "E0495",
        "`abs` is declared twice",
    );
    assert_refused(
        "extern \"c\" { pub fn abs(x: i32) -> i32 }\n",
        "E0496",
        "extern function `abs` cannot be `pub`",
    );
    assert_refused(
        "extern \"c\" { fn abs(x: i32) -> i32 }\nabs(1, 2)\n",
        "E0359",
        "expected 1, found 2",
    );
    assert_refused(
        "extern \"c\" { fn abs(x: i32) -> i32 }\nabs(1.5)\n",
        "E0301",
        "expected i32, found float",
    );
    assert_refused(
        "extern \"c\" { fn memcpy(d: &mut [u8], s: &[u8], n: u64) -> ptr }\nlet mut v: Vec<u8> = vec![1]\nmemcpy(&mut v, &v, 1)\n",
        "E0416",
        "a mutable loan is live",
    );
    assert_refused(
        "extern \"c\" { fn memset(b: &mut [u8], c: i32, n: u64) -> ptr }\nlet v: Vec<u8> = vec![0]\nmemset(&mut v, 0, 1)\n",
        "E0",
        "v",
    );
    assert_refused(
        "extern \"c\" { fn memset(b: &mut [u8], c: i32, n: u64) -> ptr }\nlet mut v: Vec<u8> = vec![0]\nmemset(v, 0, 1)\n",
        "E0417",
        "requires '&'",
    );
}

#[test]
fn an_extern_block_in_a_module_serves_that_module() {
    let dir = tempfile::tempdir().expect("temporary directory");
    std::fs::write(
        dir.path().join("cfmt.aelys"),
        "extern \"c\" { fn snprintf(b: &mut [u8], n: u64, f: cstr, ...) -> i32 }\nextern \"c\" { fn abs(x: i32) -> i32 }\npub fn show(n: int) -> string {\n    let mut b: Vec<u8> = vec![0; 32]\n    let k = snprintf(&mut b, 32, \"<%lld>\", n)\n    b.truncate(k as int)\n    b.to_utf8().unwrap()\n}\npub fn magnitude(x: i32) -> i32 { abs(x) }\n",
    )
    .expect("module");
    let outcome = run_in(
        dir.path(),
        "main.aelys",
        "use cfmt\nextern \"c\" { fn abs(x: i32) -> i32 }\nprintln(cfmt::show(-77))\nprintln(cfmt::magnitude(-5) + abs(-6))\n",
    );
    assert_eq!(outcome.code, Some(0), "stderr: {}", outcome.stderr);
    assert_eq!(outcome.lines(), vec!["<-77>", "11"]);
}

fn run_example(name: &str, args: &[&str], env: &[(&str, &str)]) -> Outcome {
    let example = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../examples/ffi")
        .join(name);
    let mut command = Command::new(cli_binary());
    command.arg("run").arg(&example).args(args);
    for (key, value) in env {
        command.env(key, value);
    }
    let output = command.output().expect("aelys-cli should run");
    Outcome {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        code: output.status.code(),
    }
}

#[test]
fn the_sdl_window_example_presents_frames_without_a_wrapper_module() {
    let outcome = run_example("sdl_window.aelys", &["3"], &[("SDL_VIDEODRIVER", "dummy")]);
    if outcome.stderr.contains("cannot load C library 'SDL2'") {
        eprintln!("SKIPPED: libSDL2 is not installed");
        return;
    }
    assert_eq!(outcome.code, Some(0), "stderr: {}", outcome.stderr);
    assert_eq!(outcome.lines(), vec!["presented 3 frames"]);
}

// opens a real window, so it runs only on the display a caller names, such as a virtual Xvfb one
#[test]
fn the_gl_example_clears_through_mesa() {
    let Ok(display) = std::env::var("AELYS_FFI_DISPLAY") else {
        eprintln!("SKIPPED: set AELYS_FFI_DISPLAY to a (virtual) X display to run the GL example");
        return;
    };
    let outcome = run_example("gl_clear.aelys", &["10"], &[("DISPLAY", &display)]);
    assert_eq!(outcome.code, Some(0), "stderr: {}", outcome.stderr);
    let lines = outcome.lines();
    assert!(lines[0].starts_with("renderer: "), "{lines:?}");
    assert_eq!(lines[1], "last clear rgba 64 230 191 255");
}
