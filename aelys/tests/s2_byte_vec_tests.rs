use aelys::{CompileOptions, ExecutionOutcome, IsolateConfig, JitMode, RunOptions, Runtime};
use aelys_opt::OptimizationLevel;

const LEVELS: [OptimizationLevel; 2] = [OptimizationLevel::None, OptimizationLevel::Standard];
const JITS: [JitMode; 2] = [JitMode::Off, JitMode::Tiered];

fn run_at(source: &str, level: OptimizationLevel, jit: JitMode) -> Result<String, String> {
    let runtime = Runtime::with_jit_mode(jit);
    let module = runtime
        .compile(
            source,
            CompileOptions {
                optimization_level: level,
                ..CompileOptions::default()
            },
        )
        .map_err(|error| error.to_string())?;
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    match isolate
        .execute(&module, RunOptions::default())
        .map_err(|error| error.to_string())?
    {
        ExecutionOutcome::Returned(value) => Ok(isolate.value_to_string(value)),
        other => Err(format!("{other:?}")),
    }
}

fn assert_everywhere(source: &str, expected: &str) {
    for level in LEVELS {
        for jit in JITS {
            match run_at(source, level, jit) {
                Ok(got) => assert_eq!(got, expected, "{level:?} {jit:?}:\n{source}"),
                Err(error) => panic!("{level:?} {jit:?}: {error}\n{source}"),
            }
        }
    }
}

fn assert_trap_everywhere(source: &str, needle: &str) {
    for level in LEVELS {
        for jit in JITS {
            match run_at(source, level, jit) {
                Ok(got) => panic!("{level:?} {jit:?}: expected a trap, got {got}\n{source}"),
                Err(error) => {
                    assert!(
                        error.contains(needle) && !error.contains("error[E"),
                        "{level:?} {jit:?}: expected '{needle}', got:\n{error}"
                    );
                }
            }
        }
    }
}

fn assert_rejected(source: &str, needles: &[&str]) {
    let error = match Runtime::new().compile(source, CompileOptions::default()) {
        Ok(_) => panic!("the source must be rejected:\n{source}"),
        Err(error) => error.to_string(),
    };
    for needle in needles {
        assert!(error.contains(needle), "expected '{needle}' in:\n{error}");
    }
}

const RENDER: &str = "fn render(b: Vec<u8>) -> string {\n    let mut s = \"\"\n    for i in 0..b.len() {\n        if i > 0 { s += \",\" }\n        s += \"{b[i]}\"\n    }\n    s\n}\n";

#[test]
fn every_integer_read_matches_struct_unpack_at_every_offset() {
    for (method, width, expected) in READS {
        let last = 16 - width;
        let source = format!(
            "let b: Vec<u8> = vec![{SAMPLE}]\nlet mut out = \"\"\nfor rep in 0..300 {{\n    let mut s = \"\"\n    for o in 0..{last} + 1 {{\n        if o > 0 {{ s += \",\" }}\n        s += \"{{b.{method}(o)}}\"\n    }}\n    out = s\n}}\nout\n"
        );
        assert_everywhere(&source, expected);
    }
}

#[test]
fn a_read_one_byte_past_the_end_or_before_the_start_traps_with_the_offset() {
    for (method, width, _) in READS {
        let past = 16 - width + 1;
        let source = format!("let b: Vec<u8> = vec![{SAMPLE}]\nlet x = b.{method}({past})\nx\n");
        assert_trap_everywhere(
            &source,
            &format!(
                "{method}: bytes {past}..{} are out of bounds for length 16",
                past + width
            ),
        );
        let source = format!("let b: Vec<u8> = vec![{SAMPLE}]\nlet x = b.{method}(0 - 1)\nx\n");
        assert_trap_everywhere(
            &source,
            &format!(
                "{method}: bytes -1..{} are out of bounds for length 16",
                *width as i64 - 1
            ),
        );
    }
    let empty = "let b: Vec<u8> = vec![]\nlet x = b.read_u8(0)\nx\n";
    assert_trap_everywhere(empty, "read_u8: bytes 0..1 are out of bounds for length 0");
}

#[test]
fn every_integer_write_matches_struct_pack_at_aligned_misaligned_and_last_offsets() {
    for (method, rows) in WRITES {
        let mut body = String::from(RENDER);
        body.push_str("let mut out = \"\"\n");
        for (offset, value, _) in rows.iter() {
            body.push_str(&format!(
                "let mut b{offset}_{n}: Vec<u8> = vec![0; 12]\nb{offset}_{n}.{method}({offset}, {value})\nout += render(b{offset}_{n}) + \"|\"\n",
                n = value.unsigned_abs()
            ));
        }
        body.push_str("out\n");
        let expected: String = rows
            .iter()
            .map(|(_, _, bytes)| format!("{bytes}|"))
            .collect();
        assert_everywhere(&body, &expected);
    }
}

#[test]
fn a_write_past_the_end_or_out_of_its_range_traps_naming_the_offset_or_value() {
    for (method, rows) in WRITES {
        let width = 12 - rows[2].0;
        let past = 12 - width + 1;
        let source = format!("let mut b: Vec<u8> = vec![0; 12]\nb.{method}({past}, 1)\n0\n");
        assert_trap_everywhere(
            &source,
            &format!(
                "{method}: bytes {past}..{} are out of bounds for length 12",
                past + width
            ),
        );
    }
    for (method, value, target) in [
        ("write_u8", "256", "u8"),
        ("write_u8", "0 - 1", "u8"),
        ("write_i8", "0 - 129", "i8"),
        ("write_i8", "128", "i8"),
        ("write_u16_le", "65536", "u16"),
        ("write_i16_be", "32768", "i16"),
        ("write_u32_be", "4294967296", "u32"),
        ("write_i32_le", "0 - 2147483649", "i32"),
        ("write_u64_le", "0 - 1", "u64"),
    ] {
        let shown = value.replace("0 - ", "-");
        let source = format!(
            "{RENDER}let mut b: Vec<u8> = vec![7; 8]\nlet r = b.{method}(0, {value})\nrender(b)\n"
        );
        assert_trap_everywhere(
            &source,
            &format!("{method}: value {shown} does not fit in {target}"),
        );
    }
}

#[test]
fn a_64_bit_read_returns_an_int_and_traps_on_a_value_outside_int() {
    for (method, bytes, value) in WIDE_READS {
        let source = format!(
            "let b: Vec<u8> = vec![{bytes}]\nlet mut x = 0\nfor rep in 0..300 {{ x = b.{method}(3) }}\nx\n"
        );
        assert_everywhere(&source, &value.to_string());
    }
    for (method, bytes, shown) in [
        (
            "read_u64_le",
            "255, 255, 255, 255, 255, 255, 255, 255",
            "18446744073709551615",
        ),
        (
            "read_u64_be",
            "128, 0, 0, 0, 0, 0, 0, 0",
            "9223372036854775808",
        ),
        (
            "read_u64_le",
            "0, 0, 0, 0, 0, 0, 0, 128",
            "9223372036854775808",
        ),
    ] {
        let source = format!("let b: Vec<u8> = vec![{bytes}]\nlet x = b.{method}(0)\nx\n");
        assert_trap_everywhere(
            &source,
            &format!("{method}: value {shown} does not fit in int"),
        );
    }
}

#[test]
fn float_reads_and_writes_match_struct_pack_bit_for_bit() {
    for (kind, value, bytes, back) in FLOATS {
        let source = format!(
            "{RENDER}let mut w: Vec<u8> = vec![0; 13]\nw.write_{kind}(5, {value})\nlet r: Vec<u8> = vec![{bytes}]\nlet same = r.read_{kind}(5) == {back}\n\"{{render(w)}} {{same}}\"\n"
        );
        assert_everywhere(&source, &format!("{bytes} true"));
    }
    let past = "let b: Vec<u8> = vec![0; 7]\nlet x = b.read_f64_be(0)\nx\n";
    assert_trap_everywhere(
        past,
        "read_f64_be: bytes 0..8 are out of bounds for length 7",
    );
}

#[test]
fn utf8_round_trips_and_invalid_bytes_are_none() {
    let source = format!(
        "{RENDER}let text = \"h\u{e9}llo \u{20ac}!\"\nlet b = text.to_bytes()\nlet back = match b.to_utf8() {{ Some(s) => s, None => \"<none>\" }}\nlet cut: Vec<u8> = vec![104, 195]\nlet lone: Vec<u8> = vec![255]\nlet surrogate: Vec<u8> = vec![237, 160, 128]\nlet empty: Vec<u8> = vec![]\nlet e = match empty.to_utf8() {{ Some(s) => \"[{{s}}]\", None => \"<none>\" }}\n\"{{b.len()}}|{{render(b)}}|{{back == text}}|{{cut.to_utf8().is_none()}} {{lone.to_utf8().is_none()}} {{surrogate.to_utf8().is_none()}}|{{e}}\"\n"
    );
    let expected = format!(
        "11|{}|true|true true true|[]",
        "h\u{e9}llo \u{20ac}!"
            .bytes()
            .map(|b| b.to_string())
            .collect::<Vec<_>>()
            .join(",")
    );
    assert_everywhere(&source, &expected);
}

#[test]
fn copy_within_moves_overlapping_ranges_like_memmove() {
    let source = format!(
        "{RENDER}let mut a: Vec<u8> = vec![1, 2, 3, 4, 5, 6, 7, 8]\na.copy_within(0, 5, 2)\nlet mut b: Vec<u8> = vec![1, 2, 3, 4, 5, 6, 7, 8]\nb.copy_within(3, 8, 0)\nlet mut c: Vec<u8> = vec![1, 2, 3]\nc.copy_within(1, 1, 3)\n\"{{render(a)}}|{{render(b)}}|{{render(c)}}\"\n"
    );
    assert_everywhere(&source, "1,2,1,2,3,4,5,8|4,5,6,7,8,6,7,8|1,2,3");
    assert_trap_everywhere(
        "let mut a: Vec<u8> = vec![1, 2, 3, 4]\na.copy_within(1, 3, 3)\n0\n",
        "copy_within: bytes 3..5 are out of bounds for length 4",
    );
}

#[test]
fn byte_methods_need_a_vec_of_u8_and_writes_need_a_mutable_binding() {
    assert_rejected(
        "let v: Vec<int> = vec![1, 2]\nlet x = v.read_u16_le(0)\n",
        &["error[E0"],
    );
    assert_rejected(
        "let v: Vec<u8> = vec![1, 2]\nv.write_u16_le(0, 3)\n",
        &["E0318"],
    );
    assert_rejected(
        "let v: Vec<u8> = vec![1, 2]\nlet x: u8 = v.read_u8(0)\n",
        &["error[E0"],
    );
}

#[test]
fn the_removed_bytes_module_is_a_named_diagnostic_with_the_replacement() {
    assert_rejected(
        "use std::bytes\nlet b = bytes::alloc(4)\n",
        &[
            "E0480",
            "'std::bytes' was removed",
            "Vec<u8>",
            "fs::read_bytes(path)",
        ],
    );
    assert_rejected("use std::bytes::{alloc}\n", &["E0480"]);
    assert_rejected(
        "let b = bytes::alloc(4)\n",
        &["E0481", "`bytes::alloc` no longer exists", "vec![0; n]"],
    );
    assert_rejected(
        "let b: Vec<u8> = vec![0; 4]\nlet x = bytes::read_u16(b, 0)\n",
        &["E0481", "read_u16_le(offset)"],
    );
    assert_rejected("let x = bytes::decode(1, 0, 4)\n", &["E0481", "to_utf8()"]);
}

const SAMPLE: &str = "0, 255, 128, 127, 1, 254, 18, 52, 86, 120, 154, 188, 222, 240, 0, 128";
const READS: &[(&str, usize, &str)] = &[
    (
        "read_u8",
        1,
        "0,255,128,127,1,254,18,52,86,120,154,188,222,240,0,128",
    ),
    (
        "read_i8",
        1,
        "0,-1,-128,127,1,-2,18,52,86,120,-102,-68,-34,-16,0,-128",
    ),
    (
        "read_u16_le",
        2,
        "65280,33023,32640,383,65025,4862,13330,22068,30806,39544,48282,57020,61662,240,32768",
    ),
    (
        "read_u16_be",
        2,
        "255,65408,32895,32513,510,65042,4660,13398,22136,30874,39612,48350,57072,61440,128",
    ),
    (
        "read_i16_le",
        2,
        "-256,-32513,32640,383,-511,4862,13330,22068,30806,-25992,-17254,-8516,-3874,240,-32768",
    ),
    (
        "read_i16_be",
        2,
        "255,-128,-32641,32513,510,-494,4660,13398,22136,30874,-25924,-17186,-8464,-4096,128",
    ),
    (
        "read_u32_le",
        4,
        "2139160320,25133311,4261511040,318636415,873659905,1446253310,2018915346,2591577652,3164239958,3736902264,4041129114,15785660,2147545310",
    ),
    (
        "read_u32_be",
        4,
        "16744575,4286611201,2155807230,2130837010,33428020,4262605910,305419896,878082202,1450744508,2023406814,2596069104,3168727040,3740270720",
    ),
    (
        "read_i32_le",
        4,
        "2139160320,25133311,-33456256,318636415,873659905,1446253310,2018915346,-1703389644,-1130727338,-558065032,-253838182,15785660,-2147421986",
    ),
    (
        "read_i32_be",
        4,
        "16744575,-8356095,-2139160066,2130837010,33428020,-32361386,305419896,878082202,1450744508,2023406814,-1698898192,-1126240256,-554696576",
    ),
];
type WriteRow = (usize, i64, &'static str);

const WRITES: &[(&str, &[WriteRow])] = &[
    (
        "write_u8",
        &[
            (0, 0, "0,0,0,0,0,0,0,0,0,0,0,0"),
            (3, 0, "0,0,0,0,0,0,0,0,0,0,0,0"),
            (11, 0, "0,0,0,0,0,0,0,0,0,0,0,0"),
            (0, 255, "255,0,0,0,0,0,0,0,0,0,0,0"),
            (3, 255, "0,0,0,255,0,0,0,0,0,0,0,0"),
            (11, 255, "0,0,0,0,0,0,0,0,0,0,0,255"),
            (0, 128, "128,0,0,0,0,0,0,0,0,0,0,0"),
            (3, 128, "0,0,0,128,0,0,0,0,0,0,0,0"),
            (11, 128, "0,0,0,0,0,0,0,0,0,0,0,128"),
        ],
    ),
    (
        "write_i8",
        &[
            (0, -128, "128,0,0,0,0,0,0,0,0,0,0,0"),
            (3, -128, "0,0,0,128,0,0,0,0,0,0,0,0"),
            (11, -128, "0,0,0,0,0,0,0,0,0,0,0,128"),
            (0, 127, "127,0,0,0,0,0,0,0,0,0,0,0"),
            (3, 127, "0,0,0,127,0,0,0,0,0,0,0,0"),
            (11, 127, "0,0,0,0,0,0,0,0,0,0,0,127"),
            (0, -1, "255,0,0,0,0,0,0,0,0,0,0,0"),
            (3, -1, "0,0,0,255,0,0,0,0,0,0,0,0"),
            (11, -1, "0,0,0,0,0,0,0,0,0,0,0,255"),
            (0, 0, "0,0,0,0,0,0,0,0,0,0,0,0"),
            (3, 0, "0,0,0,0,0,0,0,0,0,0,0,0"),
            (11, 0, "0,0,0,0,0,0,0,0,0,0,0,0"),
        ],
    ),
    (
        "write_u16_le",
        &[
            (0, 0, "0,0,0,0,0,0,0,0,0,0,0,0"),
            (3, 0, "0,0,0,0,0,0,0,0,0,0,0,0"),
            (10, 0, "0,0,0,0,0,0,0,0,0,0,0,0"),
            (0, 65535, "255,255,0,0,0,0,0,0,0,0,0,0"),
            (3, 65535, "0,0,0,255,255,0,0,0,0,0,0,0"),
            (10, 65535, "0,0,0,0,0,0,0,0,0,0,255,255"),
            (0, 4660, "52,18,0,0,0,0,0,0,0,0,0,0"),
            (3, 4660, "0,0,0,52,18,0,0,0,0,0,0,0"),
            (10, 4660, "0,0,0,0,0,0,0,0,0,0,52,18"),
        ],
    ),
    (
        "write_u16_be",
        &[
            (0, 0, "0,0,0,0,0,0,0,0,0,0,0,0"),
            (3, 0, "0,0,0,0,0,0,0,0,0,0,0,0"),
            (10, 0, "0,0,0,0,0,0,0,0,0,0,0,0"),
            (0, 65535, "255,255,0,0,0,0,0,0,0,0,0,0"),
            (3, 65535, "0,0,0,255,255,0,0,0,0,0,0,0"),
            (10, 65535, "0,0,0,0,0,0,0,0,0,0,255,255"),
            (0, 4660, "18,52,0,0,0,0,0,0,0,0,0,0"),
            (3, 4660, "0,0,0,18,52,0,0,0,0,0,0,0"),
            (10, 4660, "0,0,0,0,0,0,0,0,0,0,18,52"),
        ],
    ),
    (
        "write_i16_le",
        &[
            (0, -32768, "0,128,0,0,0,0,0,0,0,0,0,0"),
            (3, -32768, "0,0,0,0,128,0,0,0,0,0,0,0"),
            (10, -32768, "0,0,0,0,0,0,0,0,0,0,0,128"),
            (0, 32767, "255,127,0,0,0,0,0,0,0,0,0,0"),
            (3, 32767, "0,0,0,255,127,0,0,0,0,0,0,0"),
            (10, 32767, "0,0,0,0,0,0,0,0,0,0,255,127"),
            (0, -1, "255,255,0,0,0,0,0,0,0,0,0,0"),
            (3, -1, "0,0,0,255,255,0,0,0,0,0,0,0"),
            (10, -1, "0,0,0,0,0,0,0,0,0,0,255,255"),
        ],
    ),
    (
        "write_i16_be",
        &[
            (0, -32768, "128,0,0,0,0,0,0,0,0,0,0,0"),
            (3, -32768, "0,0,0,128,0,0,0,0,0,0,0,0"),
            (10, -32768, "0,0,0,0,0,0,0,0,0,0,128,0"),
            (0, 32767, "127,255,0,0,0,0,0,0,0,0,0,0"),
            (3, 32767, "0,0,0,127,255,0,0,0,0,0,0,0"),
            (10, 32767, "0,0,0,0,0,0,0,0,0,0,127,255"),
            (0, -1, "255,255,0,0,0,0,0,0,0,0,0,0"),
            (3, -1, "0,0,0,255,255,0,0,0,0,0,0,0"),
            (10, -1, "0,0,0,0,0,0,0,0,0,0,255,255"),
        ],
    ),
    (
        "write_u32_le",
        &[
            (0, 0, "0,0,0,0,0,0,0,0,0,0,0,0"),
            (3, 0, "0,0,0,0,0,0,0,0,0,0,0,0"),
            (8, 0, "0,0,0,0,0,0,0,0,0,0,0,0"),
            (0, 4294967295, "255,255,255,255,0,0,0,0,0,0,0,0"),
            (3, 4294967295, "0,0,0,255,255,255,255,0,0,0,0,0"),
            (8, 4294967295, "0,0,0,0,0,0,0,0,255,255,255,255"),
            (0, 3735928559, "239,190,173,222,0,0,0,0,0,0,0,0"),
            (3, 3735928559, "0,0,0,239,190,173,222,0,0,0,0,0"),
            (8, 3735928559, "0,0,0,0,0,0,0,0,239,190,173,222"),
        ],
    ),
    (
        "write_u32_be",
        &[
            (0, 0, "0,0,0,0,0,0,0,0,0,0,0,0"),
            (3, 0, "0,0,0,0,0,0,0,0,0,0,0,0"),
            (8, 0, "0,0,0,0,0,0,0,0,0,0,0,0"),
            (0, 4294967295, "255,255,255,255,0,0,0,0,0,0,0,0"),
            (3, 4294967295, "0,0,0,255,255,255,255,0,0,0,0,0"),
            (8, 4294967295, "0,0,0,0,0,0,0,0,255,255,255,255"),
            (0, 3735928559, "222,173,190,239,0,0,0,0,0,0,0,0"),
            (3, 3735928559, "0,0,0,222,173,190,239,0,0,0,0,0"),
            (8, 3735928559, "0,0,0,0,0,0,0,0,222,173,190,239"),
        ],
    ),
    (
        "write_i32_le",
        &[
            (0, -2147483648, "0,0,0,128,0,0,0,0,0,0,0,0"),
            (3, -2147483648, "0,0,0,0,0,0,128,0,0,0,0,0"),
            (8, -2147483648, "0,0,0,0,0,0,0,0,0,0,0,128"),
            (0, 2147483647, "255,255,255,127,0,0,0,0,0,0,0,0"),
            (3, 2147483647, "0,0,0,255,255,255,127,0,0,0,0,0"),
            (8, 2147483647, "0,0,0,0,0,0,0,0,255,255,255,127"),
            (0, -2, "254,255,255,255,0,0,0,0,0,0,0,0"),
            (3, -2, "0,0,0,254,255,255,255,0,0,0,0,0"),
            (8, -2, "0,0,0,0,0,0,0,0,254,255,255,255"),
        ],
    ),
    (
        "write_i32_be",
        &[
            (0, -2147483648, "128,0,0,0,0,0,0,0,0,0,0,0"),
            (3, -2147483648, "0,0,0,128,0,0,0,0,0,0,0,0"),
            (8, -2147483648, "0,0,0,0,0,0,0,0,128,0,0,0"),
            (0, 2147483647, "127,255,255,255,0,0,0,0,0,0,0,0"),
            (3, 2147483647, "0,0,0,127,255,255,255,0,0,0,0,0"),
            (8, 2147483647, "0,0,0,0,0,0,0,0,127,255,255,255"),
            (0, -2, "255,255,255,254,0,0,0,0,0,0,0,0"),
            (3, -2, "0,0,0,255,255,255,254,0,0,0,0,0"),
            (8, -2, "0,0,0,0,0,0,0,0,255,255,255,254"),
        ],
    ),
    (
        "write_u64_le",
        &[
            (0, 0, "0,0,0,0,0,0,0,0,0,0,0,0"),
            (3, 0, "0,0,0,0,0,0,0,0,0,0,0,0"),
            (4, 0, "0,0,0,0,0,0,0,0,0,0,0,0"),
            (0, 140737488355327, "255,255,255,255,255,127,0,0,0,0,0,0"),
            (3, 140737488355327, "0,0,0,255,255,255,255,255,127,0,0,0"),
            (4, 140737488355327, "0,0,0,0,255,255,255,255,255,127,0,0"),
            (0, 1250999896491, "171,137,103,69,35,1,0,0,0,0,0,0"),
            (3, 1250999896491, "0,0,0,171,137,103,69,35,1,0,0,0"),
            (4, 1250999896491, "0,0,0,0,171,137,103,69,35,1,0,0"),
        ],
    ),
    (
        "write_u64_be",
        &[
            (0, 0, "0,0,0,0,0,0,0,0,0,0,0,0"),
            (3, 0, "0,0,0,0,0,0,0,0,0,0,0,0"),
            (4, 0, "0,0,0,0,0,0,0,0,0,0,0,0"),
            (0, 140737488355327, "0,0,127,255,255,255,255,255,0,0,0,0"),
            (3, 140737488355327, "0,0,0,0,0,127,255,255,255,255,255,0"),
            (4, 140737488355327, "0,0,0,0,0,0,127,255,255,255,255,255"),
            (0, 1250999896491, "0,0,1,35,69,103,137,171,0,0,0,0"),
            (3, 1250999896491, "0,0,0,0,0,1,35,69,103,137,171,0"),
            (4, 1250999896491, "0,0,0,0,0,0,1,35,69,103,137,171"),
        ],
    ),
    (
        "write_i64_le",
        &[
            (0, -140737488355328, "0,0,0,0,0,128,255,255,0,0,0,0"),
            (3, -140737488355328, "0,0,0,0,0,0,0,0,128,255,255,0"),
            (4, -140737488355328, "0,0,0,0,0,0,0,0,0,128,255,255"),
            (0, 140737488355327, "255,255,255,255,255,127,0,0,0,0,0,0"),
            (3, 140737488355327, "0,0,0,255,255,255,255,255,127,0,0,0"),
            (4, 140737488355327, "0,0,0,0,255,255,255,255,255,127,0,0"),
            (0, -1, "255,255,255,255,255,255,255,255,0,0,0,0"),
            (3, -1, "0,0,0,255,255,255,255,255,255,255,255,0"),
            (4, -1, "0,0,0,0,255,255,255,255,255,255,255,255"),
            (0, 0, "0,0,0,0,0,0,0,0,0,0,0,0"),
            (3, 0, "0,0,0,0,0,0,0,0,0,0,0,0"),
            (4, 0, "0,0,0,0,0,0,0,0,0,0,0,0"),
        ],
    ),
    (
        "write_i64_be",
        &[
            (0, -140737488355328, "255,255,128,0,0,0,0,0,0,0,0,0"),
            (3, -140737488355328, "0,0,0,255,255,128,0,0,0,0,0,0"),
            (4, -140737488355328, "0,0,0,0,255,255,128,0,0,0,0,0"),
            (0, 140737488355327, "0,0,127,255,255,255,255,255,0,0,0,0"),
            (3, 140737488355327, "0,0,0,0,0,127,255,255,255,255,255,0"),
            (4, 140737488355327, "0,0,0,0,0,0,127,255,255,255,255,255"),
            (0, -1, "255,255,255,255,255,255,255,255,0,0,0,0"),
            (3, -1, "0,0,0,255,255,255,255,255,255,255,255,0"),
            (4, -1, "0,0,0,0,255,255,255,255,255,255,255,255"),
            (0, 0, "0,0,0,0,0,0,0,0,0,0,0,0"),
            (3, 0, "0,0,0,0,0,0,0,0,0,0,0,0"),
            (4, 0, "0,0,0,0,0,0,0,0,0,0,0,0"),
        ],
    ),
];
const WIDE_READS: &[(&str, &str, i64)] = &[
    ("read_u64_le", "0,0,0,0,0,0,0,0,0,0,0", 0),
    (
        "read_u64_le",
        "0,0,0,255,255,255,255,255,127,0,0",
        140737488355327,
    ),
    (
        "read_u64_le",
        "0,0,0,246,229,212,195,178,113,0,0",
        125012603626998,
    ),
    ("read_u64_be", "0,0,0,0,0,0,0,0,0,0,0", 0),
    (
        "read_u64_be",
        "0,0,0,0,0,127,255,255,255,255,255",
        140737488355327,
    ),
    (
        "read_u64_be",
        "0,0,0,0,0,113,178,195,212,229,246",
        125012603626998,
    ),
    (
        "read_i64_le",
        "0,0,0,0,0,0,0,0,128,255,255",
        -140737488355328,
    ),
    (
        "read_i64_le",
        "0,0,0,255,255,255,255,255,127,0,0",
        140737488355327,
    ),
    ("read_i64_le", "0,0,0,255,255,255,255,255,255,255,255", -1),
    (
        "read_i64_le",
        "0,0,0,68,101,135,169,203,237,255,255",
        -20015998343868,
    ),
    (
        "read_i64_be",
        "0,0,0,255,255,128,0,0,0,0,0",
        -140737488355328,
    ),
    (
        "read_i64_be",
        "0,0,0,0,0,127,255,255,255,255,255",
        140737488355327,
    ),
    ("read_i64_be", "0,0,0,255,255,255,255,255,255,255,255", -1),
    (
        "read_i64_be",
        "0,0,0,255,255,237,203,169,135,101,68",
        -20015998343868,
    ),
];
const FLOATS: &[(&str, &str, &str, &str)] = &[
    ("f32_le", "1.5", "0,0,0,0,0,0,0,192,63,0,0,0,0", "1.5"),
    ("f32_le", "-2.25", "0,0,0,0,0,0,0,16,192,0,0,0,0", "-2.25"),
    ("f32_le", "0.0", "0,0,0,0,0,0,0,0,0,0,0,0,0", "0.0"),
    (
        "f32_le",
        "3.4028234663852886e38",
        "0,0,0,0,0,255,255,127,127,0,0,0,0",
        "3.4028234663852886e+38",
    ),
    (
        "f32_le",
        "1.401298464324817e-45",
        "0,0,0,0,0,1,0,0,0,0,0,0,0",
        "1.401298464324817e-45",
    ),
    ("f32_be", "1.5", "0,0,0,0,0,63,192,0,0,0,0,0,0", "1.5"),
    ("f32_be", "-2.25", "0,0,0,0,0,192,16,0,0,0,0,0,0", "-2.25"),
    ("f32_be", "0.0", "0,0,0,0,0,0,0,0,0,0,0,0,0", "0.0"),
    (
        "f32_be",
        "3.4028234663852886e38",
        "0,0,0,0,0,127,127,255,255,0,0,0,0",
        "3.4028234663852886e+38",
    ),
    (
        "f32_be",
        "1.401298464324817e-45",
        "0,0,0,0,0,0,0,0,1,0,0,0,0",
        "1.401298464324817e-45",
    ),
    ("f64_le", "1.5", "0,0,0,0,0,0,0,0,0,0,0,248,63", "1.5"),
    ("f64_le", "-2.25", "0,0,0,0,0,0,0,0,0,0,0,2,192", "-2.25"),
    (
        "f64_le",
        "1e300",
        "0,0,0,0,0,156,117,0,136,60,228,55,126",
        "1e+300",
    ),
    ("f64_le", "5e-324", "0,0,0,0,0,1,0,0,0,0,0,0,0", "5e-324"),
    ("f64_le", "-0.0", "0,0,0,0,0,0,0,0,0,0,0,0,128", "-0.0"),
    ("f64_be", "1.5", "0,0,0,0,0,63,248,0,0,0,0,0,0", "1.5"),
    ("f64_be", "-2.25", "0,0,0,0,0,192,2,0,0,0,0,0,0", "-2.25"),
    (
        "f64_be",
        "1e300",
        "0,0,0,0,0,126,55,228,60,136,0,117,156",
        "1e+300",
    ),
    ("f64_be", "5e-324", "0,0,0,0,0,0,0,0,0,0,0,0,1", "5e-324"),
    ("f64_be", "-0.0", "0,0,0,0,0,128,0,0,0,0,0,0,0", "-0.0"),
];
