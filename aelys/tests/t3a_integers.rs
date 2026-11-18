mod common;
use common::*;

use aelys::{CompileOptions, ExecutionOutcome, IsolateConfig, JitMode, RunOptions, Runtime};
use aelys_opt::OptimizationLevel;

// values from a c oracle compiled with -fwrapv: wrap through unsigned arithmetic, c99 truncating division, shift counts masked to width-1
const I8_TABLE: [i64; 260] = [
    1, -1, 0, 0, 0, 0, 0, 0, 1, 1, -1, 1, 0, 0, 0, 0, 0, 0, -1, -1, 3, -3, 0, 0, 0, 0, 0, 0, 3, 3,
    127, -127, 0, 0, 0, 0, 0, 0, 127, 127, -128, -128, 0, 0, 0, 0, 0, 0, -128, -128, 2, 0, 1, 1, 0,
    2, 0, 1, 1, 0, 0, 2, -1, -1, 0, -128, 0, 1, -1, -2, 4, -2, 3, 0, 1, 8, 0, 1, 3, 2, -128, -126,
    127, 0, 1, -128, 0, 1, 127, 126, -127, -127, -128, 0, 1, 1, 1, 0, -127, -127, 0, -2, -1, -1, 0,
    -2, -1, 1, -1, -2, -2, 0, 1, 1, 0, -128, -1, -1, -1, 0, 2, -4, -3, 0, -1, -8, -1, 3, -1, -4,
    126, -128, -127, 0, -1, -128, -1, 127, -1, -128, 127, 127, -128, 0, -1, -1, -1, -128, -1, 127,
    -128, 126, 127, 127, 0, -2, 63, 1, 127, 126, 126, -128, -127, -127, 0, -128, 0, 127, -1, -128,
    -126, 124, 125, 42, 1, -8, 15, 3, 127, 124, -2, 0, 1, 1, 0, -128, 0, 127, 127, 0, -1, -1, -128,
    0, 127, 127, 127, 0, -1, -1, -127, 127, -128, -128, 0, 0, -64, 0, -127, -127, 127, -127, -128,
    -128, 0, 0, -1, -128, -1, 127, -125, 125, -128, -42, -2, 0, -16, 0, -125, -125, -1, 1, -128,
    -1, -1, 0, -1, 0, -1, -1, 0, 0, 0, 1, 0, -128, -128, -128, -128, 0, 0, -1, -1, -2, 1, 0, -127,
    -128, -128, 127,
];

const I16_TABLE: [i64; 260] = [
    1, -1, 0, 0, 0, 0, 0, 0, 1, 1, -1, 1, 0, 0, 0, 0, 0, 0, -1, -1, 3, -3, 0, 0, 0, 0, 0, 0, 3, 3,
    32767, -32767, 0, 0, 0, 0, 0, 0, 32767, 32767, -32768, -32768, 0, 0, 0, 0, 0, 0, -32768,
    -32768, 2, 0, 1, 1, 0, 2, 0, 1, 1, 0, 0, 2, -1, -1, 0, -32768, 0, 1, -1, -2, 4, -2, 3, 0, 1, 8,
    0, 1, 3, 2, -32768, -32766, 32767, 0, 1, -32768, 0, 1, 32767, 32766, -32767, -32767, -32768, 0,
    1, 1, 1, 0, -32767, -32767, 0, -2, -1, -1, 0, -2, -1, 1, -1, -2, -2, 0, 1, 1, 0, -32768, -1,
    -1, -1, 0, 2, -4, -3, 0, -1, -8, -1, 3, -1, -4, 32766, -32768, -32767, 0, -1, -32768, -1,
    32767, -1, -32768, 32767, 32767, -32768, 0, -1, -1, -1, -32768, -1, 32767, -32768, 32766,
    32767, 32767, 0, -2, 16383, 1, 32767, 32766, 32766, -32768, -32767, -32767, 0, -32768, 0,
    32767, -1, -32768, -32766, 32764, 32765, 10922, 1, -8, 4095, 3, 32767, 32764, -2, 0, 1, 1, 0,
    -32768, 0, 32767, 32767, 0, -1, -1, -32768, 0, 32767, 32767, 32767, 0, -1, -1, -32767, 32767,
    -32768, -32768, 0, 0, -16384, 0, -32767, -32767, 32767, -32767, -32768, -32768, 0, 0, -1,
    -32768, -1, 32767, -32765, 32765, -32768, -10922, -2, 0, -4096, 0, -32765, -32765, -1, 1,
    -32768, -1, -1, 0, -1, 0, -1, -1, 0, 0, 0, 1, 0, -32768, -32768, -32768, -32768, 0, 0, -1, -1,
    -2, 1, 0, -32767, -32768, -32768, 32767,
];

const I32_TABLE: [i64; 260] = [
    1,
    -1,
    0,
    0,
    0,
    0,
    0,
    0,
    1,
    1,
    -1,
    1,
    0,
    0,
    0,
    0,
    0,
    0,
    -1,
    -1,
    3,
    -3,
    0,
    0,
    0,
    0,
    0,
    0,
    3,
    3,
    2147483647,
    -2147483647,
    0,
    0,
    0,
    0,
    0,
    0,
    2147483647,
    2147483647,
    -2147483648,
    -2147483648,
    0,
    0,
    0,
    0,
    0,
    0,
    -2147483648,
    -2147483648,
    2,
    0,
    1,
    1,
    0,
    2,
    0,
    1,
    1,
    0,
    0,
    2,
    -1,
    -1,
    0,
    -2147483648,
    0,
    1,
    -1,
    -2,
    4,
    -2,
    3,
    0,
    1,
    8,
    0,
    1,
    3,
    2,
    -2147483648,
    -2147483646,
    2147483647,
    0,
    1,
    -2147483648,
    0,
    1,
    2147483647,
    2147483646,
    -2147483647,
    -2147483647,
    -2147483648,
    0,
    1,
    1,
    1,
    0,
    -2147483647,
    -2147483647,
    0,
    -2,
    -1,
    -1,
    0,
    -2,
    -1,
    1,
    -1,
    -2,
    -2,
    0,
    1,
    1,
    0,
    -2147483648,
    -1,
    -1,
    -1,
    0,
    2,
    -4,
    -3,
    0,
    -1,
    -8,
    -1,
    3,
    -1,
    -4,
    2147483646,
    -2147483648,
    -2147483647,
    0,
    -1,
    -2147483648,
    -1,
    2147483647,
    -1,
    -2147483648,
    2147483647,
    2147483647,
    -2147483648,
    0,
    -1,
    -1,
    -1,
    -2147483648,
    -1,
    2147483647,
    -2147483648,
    2147483646,
    2147483647,
    2147483647,
    0,
    -2,
    1073741823,
    1,
    2147483647,
    2147483646,
    2147483646,
    -2147483648,
    -2147483647,
    -2147483647,
    0,
    -2147483648,
    0,
    2147483647,
    -1,
    -2147483648,
    -2147483646,
    2147483644,
    2147483645,
    715827882,
    1,
    -8,
    268435455,
    3,
    2147483647,
    2147483644,
    -2,
    0,
    1,
    1,
    0,
    -2147483648,
    0,
    2147483647,
    2147483647,
    0,
    -1,
    -1,
    -2147483648,
    0,
    2147483647,
    2147483647,
    2147483647,
    0,
    -1,
    -1,
    -2147483647,
    2147483647,
    -2147483648,
    -2147483648,
    0,
    0,
    -1073741824,
    0,
    -2147483647,
    -2147483647,
    2147483647,
    -2147483647,
    -2147483648,
    -2147483648,
    0,
    0,
    -1,
    -2147483648,
    -1,
    2147483647,
    -2147483645,
    2147483645,
    -2147483648,
    -715827882,
    -2,
    0,
    -268435456,
    0,
    -2147483645,
    -2147483645,
    -1,
    1,
    -2147483648,
    -1,
    -1,
    0,
    -1,
    0,
    -1,
    -1,
    0,
    0,
    0,
    1,
    0,
    -2147483648,
    -2147483648,
    -2147483648,
    -2147483648,
    0,
    0,
    -1,
    -1,
    -2,
    1,
    0,
    -2147483647,
    -2147483648,
    -2147483648,
    2147483647,
];

const U8_TABLE: [i64; 260] = [
    1, 255, 0, 0, 0, 0, 0, 0, 1, 1, 3, 253, 0, 0, 0, 0, 0, 0, 3, 3, 7, 249, 0, 0, 0, 0, 0, 0, 7, 7,
    255, 1, 0, 0, 0, 0, 0, 0, 255, 255, 128, 128, 0, 0, 0, 0, 0, 0, 128, 128, 2, 0, 1, 1, 0, 2, 0,
    1, 1, 0, 4, 254, 3, 0, 1, 8, 0, 1, 3, 2, 8, 250, 7, 0, 1, 128, 0, 1, 7, 6, 0, 2, 255, 0, 1,
    128, 0, 1, 255, 254, 129, 129, 128, 0, 1, 1, 1, 0, 129, 129, 201, 199, 200, 200, 0, 144, 100,
    0, 201, 201, 203, 197, 88, 66, 2, 64, 25, 0, 203, 203, 207, 193, 120, 28, 4, 0, 1, 0, 207, 207,
    199, 201, 56, 0, 200, 0, 1, 200, 255, 55, 72, 72, 0, 1, 72, 200, 200, 128, 200, 72, 0, 254,
    255, 255, 0, 254, 127, 1, 255, 254, 2, 252, 253, 85, 0, 248, 31, 3, 255, 252, 6, 248, 249, 36,
    3, 128, 1, 7, 255, 248, 254, 0, 1, 1, 0, 128, 1, 255, 255, 0, 127, 127, 128, 1, 127, 255, 255,
    128, 255, 127, 129, 127, 128, 128, 0, 0, 64, 0, 129, 129, 131, 125, 128, 42, 2, 0, 16, 0, 131,
    131, 135, 121, 128, 18, 2, 0, 1, 0, 135, 135, 127, 129, 128, 0, 128, 0, 1, 128, 255, 127, 0, 0,
    0, 1, 0, 128, 128, 128, 128, 0, 0, 255, 255, 254, 56, 55, 1, 0, 128, 127,
];

const U16_TABLE: [i64; 260] = [
    1, 65535, 0, 0, 0, 0, 0, 0, 1, 1, 3, 65533, 0, 0, 0, 0, 0, 0, 3, 3, 15, 65521, 0, 0, 0, 0, 0,
    0, 15, 15, 65535, 1, 0, 0, 0, 0, 0, 0, 65535, 65535, 32768, 32768, 0, 0, 0, 0, 0, 0, 32768,
    32768, 2, 0, 1, 1, 0, 2, 0, 1, 1, 0, 4, 65534, 3, 0, 1, 8, 0, 1, 3, 2, 16, 65522, 15, 0, 1,
    32768, 0, 1, 15, 14, 0, 2, 65535, 0, 1, 32768, 0, 1, 65535, 65534, 32769, 32769, 32768, 0, 1,
    1, 1, 0, 32769, 32769, 40001, 39999, 40000, 40000, 0, 14464, 20000, 0, 40001, 40001, 40003,
    39997, 54464, 13333, 1, 57856, 5000, 0, 40003, 40003, 40015, 39985, 10176, 2666, 10, 0, 1, 0,
    40015, 40015, 39999, 40001, 25536, 0, 40000, 0, 1, 40000, 65535, 25535, 7232, 7232, 0, 1, 7232,
    40000, 40000, 32768, 40000, 7232, 0, 65534, 65535, 65535, 0, 65534, 32767, 1, 65535, 65534, 2,
    65532, 65533, 21845, 0, 65528, 8191, 3, 65535, 65532, 14, 65520, 65521, 4369, 0, 32768, 1, 15,
    65535, 65520, 65534, 0, 1, 1, 0, 32768, 1, 65535, 65535, 0, 32767, 32767, 32768, 1, 32767,
    65535, 65535, 32768, 65535, 32767, 32769, 32767, 32768, 32768, 0, 0, 16384, 0, 32769, 32769,
    32771, 32765, 32768, 10922, 2, 0, 4096, 0, 32771, 32771, 32783, 32753, 32768, 2184, 8, 0, 1, 0,
    32783, 32783, 32767, 32769, 32768, 0, 32768, 0, 1, 32768, 65535, 32767, 0, 0, 0, 1, 0, 32768,
    32768, 32768, 32768, 0, 0, 65535, 65535, 65534, 25536, 25535, 1, 0, 32768, 32767,
];

const U32_TABLE: [i64; 260] = [
    1, 4294967295, 0, 0, 0, 0, 0, 0, 1, 1, 3, 4294967293, 0, 0, 0, 0, 0, 0, 3, 3, 31, 4294967265,
    0, 0, 0, 0, 0, 0, 31, 31, 4294967295, 1, 0, 0, 0, 0, 0, 0, 4294967295, 4294967295, 2147483648,
    2147483648, 0, 0, 0, 0, 0, 0, 2147483648, 2147483648, 2, 0, 1, 1, 0, 2, 0, 1, 1, 0, 4,
    4294967294, 3, 0, 1, 8, 0, 1, 3, 2, 32, 4294967266, 31, 0, 1, 2147483648, 0, 1, 31, 30, 0, 2,
    4294967295, 0, 1, 2147483648, 0, 1, 4294967295, 4294967294, 2147483649, 2147483649, 2147483648,
    0, 1, 1, 1, 0, 2147483649, 2147483649, 3000000001, 2999999999, 3000000000, 3000000000, 0,
    1705032704, 1500000000, 0, 3000000001, 3000000001, 3000000003, 2999999997, 410065408,
    1000000000, 0, 2525163520, 375000000, 0, 3000000003, 3000000003, 3000000031, 2999999969,
    2805686784, 96774193, 17, 0, 1, 0, 3000000031, 3000000031, 2999999999, 3000000001, 1294967296,
    0, 3000000000, 0, 1, 3000000000, 4294967295, 1294967295, 852516352, 852516352, 0, 1, 852516352,
    3000000000, 3000000000, 2147483648, 3000000000, 852516352, 0, 4294967294, 4294967295,
    4294967295, 0, 4294967294, 2147483647, 1, 4294967295, 4294967294, 2, 4294967292, 4294967293,
    1431655765, 0, 4294967288, 536870911, 3, 4294967295, 4294967292, 30, 4294967264, 4294967265,
    138547332, 3, 2147483648, 1, 31, 4294967295, 4294967264, 4294967294, 0, 1, 1, 0, 2147483648, 1,
    4294967295, 4294967295, 0, 2147483647, 2147483647, 2147483648, 1, 2147483647, 4294967295,
    4294967295, 2147483648, 4294967295, 2147483647, 2147483649, 2147483647, 2147483648, 2147483648,
    0, 0, 1073741824, 0, 2147483649, 2147483649, 2147483651, 2147483645, 2147483648, 715827882, 2,
    0, 268435456, 0, 2147483651, 2147483651, 2147483679, 2147483617, 2147483648, 69273666, 2, 0, 1,
    0, 2147483679, 2147483679, 2147483647, 2147483649, 2147483648, 0, 2147483648, 0, 1, 2147483648,
    4294967295, 2147483647, 0, 0, 0, 1, 0, 2147483648, 2147483648, 2147483648, 2147483648, 0, 0,
    4294967295, 4294967295, 4294967294, 1294967296, 1294967295, 1, 0, 2147483648, 2147483647,
];

const OPS: [&str; 10] = ["+", "-", "*", "/", "%", "<<", ">>", "&", "|", "^"];

struct Kind {
    name: &'static str,
    xs: [&'static str; 5],
    ys: [&'static str; 5],
    table: &'static [i64],
}

const KINDS: [Kind; 6] = [
    Kind {
        name: "i8",
        xs: ["0", "1", "-1", "127", "-128"],
        ys: ["1", "-1", "3", "127", "-128"],
        table: &I8_TABLE,
    },
    Kind {
        name: "i16",
        xs: ["0", "1", "-1", "32767", "-32768"],
        ys: ["1", "-1", "3", "32767", "-32768"],
        table: &I16_TABLE,
    },
    Kind {
        name: "i32",
        xs: ["0", "1", "-1", "2147483647", "-2147483648"],
        ys: ["1", "-1", "3", "2147483647", "-2147483648"],
        table: &I32_TABLE,
    },
    Kind {
        name: "u8",
        xs: ["0", "1", "200", "255", "128"],
        ys: ["1", "3", "7", "255", "128"],
        table: &U8_TABLE,
    },
    Kind {
        name: "u16",
        xs: ["0", "1", "40000", "65535", "32768"],
        ys: ["1", "3", "15", "65535", "32768"],
        table: &U16_TABLE,
    },
    Kind {
        name: "u32",
        xs: ["0", "1", "3000000000", "4294967295", "2147483648"],
        ys: ["1", "3", "31", "4294967295", "2147483648"],
        table: &U32_TABLE,
    },
];

fn run_string(source: &str, level: OptimizationLevel) -> String {
    let mut vm = aelys::new_vm().expect("vm");
    let result = aelys::run_with_vm_and_opt(&mut vm, source, "<test>", level)
        .unwrap_or_else(|error| panic!("program must run: {error}\n{source}"));
    let pointer = result.as_ptr().expect("program must return a string");
    match &vm
        .heap()
        .get(aelys_runtime::vm::GcRef::new(pointer))
        .expect("live string")
        .kind
    {
        aelys_runtime::vm::ObjectKind::String(text) => text.as_str().to_string(),
        other => panic!("expected a string, got {other:?}"),
    }
}

fn parse_list(text: &str) -> Vec<i64> {
    text.split(',')
        .filter(|item| !item.is_empty())
        .map(|item| {
            item.parse()
                .unwrap_or_else(|_| panic!("not an integer: {item}"))
        })
        .collect()
}

fn assert_table(kind: &Kind, got: &[i64], path: &str) {
    assert_eq!(
        got.len(),
        kind.table.len(),
        "{} {path}: result count",
        kind.name
    );
    for (index, (got, expected)) in got.iter().zip(kind.table).enumerate() {
        let label = if index < 250 {
            let (pair, op) = (index / 10, index % 10);
            format!(
                "{} {} {} {}",
                kind.xs[pair / 5],
                OPS[op],
                kind.ys[pair % 5],
                kind.name
            )
        } else {
            let unary = (index - 250) % 2;
            format!(
                "{}{} {}",
                ["-", "~"][unary],
                kind.xs[(index - 250) / 2],
                kind.name
            )
        };
        assert_eq!(got, expected, "{path}: {label}");
    }
}

fn runtime_table_program(kind: &Kind) -> String {
    let ty = kind.name;
    let mut source = String::new();
    for (index, op) in OPS.iter().enumerate() {
        source.push_str(&format!(
            "fn op{index}(a: {ty}, b: {ty}) -> {ty} {{ a {op} b }}\n"
        ));
    }
    source.push_str(&format!("fn neg(a: {ty}) -> {ty} {{ -a }}\n"));
    source.push_str(&format!("fn inv(a: {ty}) -> {ty} {{ ~a }}\n"));
    source.push_str(&format!("let xs: [{ty}; 5] = [{}]\n", kind.xs.join(", ")));
    source.push_str(&format!("let ys: [{ty}; 5] = [{}]\n", kind.ys.join(", ")));
    source.push_str("let mut out = \"\"\nfor i in 0..5 {\n    for j in 0..5 {\n");
    source.push_str("        out += \"");
    for index in 0..OPS.len() {
        source.push_str(&format!("{{op{index}(xs[i], ys[j])}},"));
    }
    source.push_str(
        "\"\n    }\n}\nfor i in 0..5 {\n    out += \"{neg(xs[i])},{inv(xs[i])},\"\n}\nout\n",
    );
    source
}

fn folded_table_program(kind: &Kind) -> String {
    let ty = kind.name;
    let mut source = String::from("let mut out = \"\"\n");
    let mut counter = 0;
    for x in kind.xs {
        for y in kind.ys {
            for op in OPS {
                source.push_str(&format!("let r{counter}: {ty} = ({x}) {op} ({y})\n"));
                source.push_str(&format!("out += \"{{r{counter}}},\"\n"));
                counter += 1;
            }
        }
    }
    for x in kind.xs {
        source.push_str(&format!("let r{counter}: {ty} = -({x})\n"));
        source.push_str(&format!("out += \"{{r{counter}}},\"\n"));
        counter += 1;
        source.push_str(&format!("let r{counter}: {ty} = ~({x})\n"));
        source.push_str(&format!("out += \"{{r{counter}}},\"\n"));
        counter += 1;
    }
    source.push_str("out\n");
    source
}

#[test]
fn sized_arithmetic_matches_the_c_oracle_at_runtime() {
    for kind in &KINDS {
        let source = runtime_table_program(kind);
        for level in [OptimizationLevel::None, OptimizationLevel::Standard] {
            let got = parse_list(&run_string(&source, level));
            assert_table(kind, &got, &format!("runtime {level:?}"));
        }
    }
}

#[test]
fn constant_folding_of_sized_arithmetic_matches_the_c_oracle() {
    for kind in &KINDS {
        let source = folded_table_program(kind);
        for level in [OptimizationLevel::None, OptimizationLevel::Standard] {
            let got = parse_list(&run_string(&source, level));
            assert_table(kind, &got, &format!("folded {level:?}"));
        }
    }
}

#[test]
fn doom_fixed_point_and_angle_arithmetic() {
    let source = r#"
let FRACBITS = 16
let ANG45: u32 = 0x20000000
let ANG90: u32 = 0x40000000
fn fixed_mul(a: i32, b: i32) -> i32 { a.mul_shr(b, FRACBITS) }
fn fixed_div(a: i32, b: i32) -> i32 { a.shl_div(b, 16) }
let mut table: [i32; 8192] = [0; 8192]
table[8191] = 77
let angle: u32 = 0xFFFFFFFF
let one: u32 = 1
let wrapped = angle + one
let negated: u32 = 0 - ANG45
"{fixed_mul(0x7FFF0000, 0x20000)},{fixed_mul(3 << 16, -(2 << 16) - 0x8000)},{fixed_mul(-1, 1)},{fixed_div(10 << 16, 4 << 16)},{fixed_div(-7 << 16, 2 << 16)},{wrapped},{ANG90 * 3},{angle >> 19},{negated},{table[angle >> 19]}"
"#;
    let got = run_string(source, OptimizationLevel::Standard);
    assert_eq!(
        got,
        "-131072,-491520,-1,163840,-229376,0,3221225472,8191,3758096384,77"
    );
}

#[test]
fn fixed_point_methods_keep_an_exact_intermediate() {
    let source = r#"
let big = 1 << 40
let a: i32 = 0x7FFFFFFF
let b: i32 = 0x7FFFFFFF
let n = 31
"{big.mul_shr(big, 40)},{a.mul_shr(b, n)},{a.mul_shr(b, 0)},{(5).shl_div(2, 0)},{(-5).shl_div(2, 0)},{big.shl_div(1 << 20, 4)},{a.shl_div(1, 16)}"
"#;
    assert_eq!(
        run_string(source, OptimizationLevel::Standard),
        "1099511627776,2147483646,1,2,-2,16777216,-65536"
    );
}

#[test]
fn int_fixed_point_overflow_and_zero_divisor_trap() {
    assert_aelys_error_contains("let big = 1 << 46\nbig.mul_shr(big, 0)", "integer overflow");
    assert_aelys_error_contains("let x = 7\nx.shl_div(0, 4)", "division by zero");
    assert_aelys_error_contains("let x: i32 = 7\nx.shl_div(0, 4)", "division by zero");
}

#[test]
fn fixed_point_methods_are_limited_to_i32_and_int() {
    let error = run_aelys_err("let x: u16 = 5\nx.mul_shr(3, 1)");
    assert!(error.contains("E0371"), "{error}");
    let error = run_aelys_err("let x: i32 = 5\nlet y = 3\nx.mul_shr(y, 1)");
    assert!(error.contains("E0301"), "{error}");
    let error = run_aelys_err("let x: i32 = 5\nx.mul_shr(3)");
    assert!(error.contains("E0359"), "{error}");
}

#[test]
fn literals_take_the_sized_type_of_their_context() {
    let source = r#"
struct S { v: u8, w: i32, a: [i16; 2] }
fn take(x: u8, y: i32) -> int { x as int + y as int }
fn give() -> u8 { 250 + 10 }
fn give_explicit() -> u16 { return 65535 + 2 }
fn pick(c: bool) -> i8 { if c { 100 } else { -100 } }
impl S {
    fn bump(mut self) { self.v += 10 }
}
fn twice<T>(x: T) -> T { x }
let x: i32 = 5
let y: i32 = 3
let p: i32 = (x * y) >> 1
let mut s = S { v: 250, w: 1, a: [1, -2] }
s.bump()
s.w = 0x7FFFFFFF
s.w += 1
let mut v: Vec<u8> = vec![1, 2, 255]
v.push(254)
v[0] = 200
v[0] += 100
let mut arr: [i32; 2] = [1, 2]
arr[1] += 0x7FFFFFFF
let grid: [[u8; 2]; 2] = [[255, 1], [2, 3]]
let rep: [u16; 3] = [65535; 3]
let mut k: u8 = 255
k++
let mut c: u8 = 0
for i in 0..3 { c += 100 }
let m: u8 = 7
let label = match m { 7 => "seven", _ => "other" }
let add1 = fn(a: u8) -> u8 { a + 1 }
let mut bounded = 0
let limit: i16 = 4
for i in 1..limit { bounded = bounded + i as int }
let big8: u8 = 255
let tw: u8 = twice(big8) + 1
let cmp = x < 6
"{p},{s.v},{s.w},{s.a[1]},{v[0]},{v[3]},{arr[1]},{grid[0][0]},{rep[2]},{k},{c},{label},{add1(255)},{take(255, -1)},{give()},{give_explicit()},{pick(false)},{bounded},{tw},{cmp}"
"#;
    assert_eq!(
        run_string(source, OptimizationLevel::None),
        "7,4,-2147483648,-2,44,254,-2147483647,255,65535,0,44,seven,0,254,4,1,-100,6,0,true"
    );
    assert_eq!(
        run_string(source, OptimizationLevel::Standard),
        "7,4,-2147483648,-2,44,254,-2147483647,255,65535,0,44,seven,0,254,4,1,-100,6,0,true"
    );
}

#[test]
fn audit_reproducers_b5_and_b6_compile() {
    assert_aelys_int(
        "let x: i32 = 5\nlet y: i32 = 3\nlet p: i32 = (x * y) >> 16\np as int",
        0,
    );
    assert_aelys_int("struct S { v: u8 }\nlet s = S { v: 1 }\ns.v as int", 1);
    assert_aelys_int("let v: Vec<u8> = vec![1, 2]\nv[1] as int", 2);
    assert_aelys_int("let a: [i32; 2] = [1, 2]\na[0] as int", 1);
}

#[test]
fn an_out_of_range_literal_for_its_sized_type_is_a_compile_error() {
    for source in [
        "let a: i8 = 128",
        "let a: u8 = -1",
        "let a: u32 = 4294967296",
        "let a: i32 = 2147483648",
        "struct S { v: u8 }\nlet s = S { v: 256 }",
        "fn f(x: u16) -> int { 0 }\nf(65536)",
        "let x: u8 = 1\nlet y = x + 256",
        "let v: Vec<i8> = vec![1, 200]",
        "let x: u8 = 3\nmatch x { 300 => 1, _ => 0 }",
    ] {
        let error = run_aelys_err(source);
        assert!(
            error.contains("E0301") && error.contains("does not fit in"),
            "{source}: {error}"
        );
    }
}

#[test]
fn mixing_integer_types_in_a_binary_operation_is_refused_with_help() {
    for (source, left, right) in [
        ("let a: i32 = 1\nlet b = 2\na + b", "i32", "int"),
        ("let a: u8 = 1\nlet b: u16 = 2\na < b", "u8", "u16"),
        ("let a: u32 = 1\nlet s = 3\na | s", "u32", "int"),
        ("let a: i16 = 1\nlet b: i32 = 2\na == b", "i16", "i32"),
    ] {
        let error = run_aelys_err(source);
        assert!(error.contains("E0460"), "{source}: {error}");
        assert!(
            error.contains(&format!("{left} and {right}")),
            "{source}: {error}"
        );
        assert!(error.contains("convert with `as`"), "{source}: {error}");
    }
    assert_aelys_int("let a: i32 = 1\nlet b = 2\n(a + b as i32) as int", 3);
}

#[test]
fn i64_and_u64_are_refused_because_int_is_48_bits() {
    for source in [
        "let a: i64 = 1",
        "let a: u64 = 1",
        "fn f(x: i64) -> int { 0 }",
        "fn f() -> u64 { 0 }",
        "struct S { v: i64 }",
        "let x = 100 as i64",
        "let v: Vec<u64> = vec![]",
        "let a: int64 = 1",
    ] {
        let error = run_aelys_err(source);
        assert!(
            error.contains("E0461") && error.contains("int is a 48-bit integer"),
            "{source}: {error}"
        );
    }
}

#[test]
fn as_conversions_wrap_truncate_and_trap() {
    let source = r#"
fn id(x: float) -> float { x }
let big = 3000000000
let neg = -1
let small: i8 = -1
let wide: u32 = 4294967295
"{big as i32},{neg as u32},{neg as u8},{small as u16},{small as int},{wide as i32},{wide as u8},{(70000 as i16)},{id(3.9e9) as i32},{id(-2.7) as u8},{id(-2.7) as int},{id(2.9) as i8},{id(1e20) as u16},{true as u8},{small as float},{wide as float}"
"#;
    assert_eq!(
        run_string(source, OptimizationLevel::None),
        "-1294967296,4294967295,255,65535,-1,-1,255,4464,-394967296,254,-2,2,0,1,-1.0,4294967295.0"
    );
    assert_eq!(
        run_string(source, OptimizationLevel::Standard),
        "-1294967296,4294967295,255,65535,-1,-1,255,4464,-394967296,254,-2,2,0,1,-1.0,4294967295.0"
    );
    for target in ["int", "i32", "u8"] {
        for (value, spelling) in [
            ("id(0.0) / id(0.0)", "NaN"),
            ("id(1.0) / id(0.0)", "inf"),
            ("id(-1.0) / id(0.0)", "-inf"),
        ] {
            let source =
                format!("fn id(x: float) -> float {{ x }}\nlet v = {value}\nv as {target}");
            if target == "int" {
                assert_aelys_error_contains(&source, "integer overflow");
            } else {
                assert_aelys_error_contains(
                    &source,
                    &format!("cannot convert {spelling} to {target}"),
                );
            }
        }
    }
    assert_aelys_error_contains(
        "fn id(x: float) -> float { x }\nid(1e20) as int",
        "integer overflow",
    );
}

#[test]
fn b8_a_float_to_int_overflow_is_reported_at_the_cast() {
    let error = run_aelys_err(
        "fn fid(x: float) -> float { x }\nlet inf = fid(1.0) / fid(0.0)\nlet x = inf as int\nprintln(\"{x}\")",
    );
    assert!(error.contains("integer overflow"), "{error}");
    assert!(
        error
            .lines()
            .any(|line| line.trim_start().starts_with("-->") && line.trim_end().ends_with(":3:1")),
        "{error}"
    );
}

#[test]
fn b2_a_sized_struct_field_holds_the_wrapped_value() {
    let source = r#"
struct S { v: u8, w: i16 }
let one: u8 = 1
let mut s = S { v: one, w: 0 }
let t: u8 = 200
s.v = t + t
let big: i16 = 32767
s.w = big + 1
let fresh = S { v: 255, w: -32768 }
"{s.v},{s.w},{fresh.v}"
"#;
    assert_eq!(
        run_string(source, OptimizationLevel::None),
        "144,-32768,255"
    );
    assert_eq!(
        run_string(source, OptimizationLevel::Standard),
        "144,-32768,255"
    );
}

#[test]
fn int_shift_left_traps_on_overflow_like_multiplication() {
    let source = "fn shl(a: int, n: int) -> int { a << n }\n";
    assert_aelys_int(&format!("{source}shl(1, 46)"), 1 << 46);
    assert_aelys_int(&format!("{source}shl(-1, 47)"), -(1 << 47));
    assert_aelys_int(&format!("{source}shl(0, 63)"), 0);
    for call in ["shl(1, 47)", "shl(3, 46)", "shl(-3, 46)", "shl(1, 63)"] {
        assert_aelys_error_contains(&format!("{source}{call}"), "integer overflow");
    }
}

#[test]
fn int_multiplication_overflow_never_wraps_back_into_range() {
    let source = "fn mul(a: int, b: int) -> int { a * b }\n";
    assert_aelys_error_contains(
        &format!("{source}mul(1 << 40, 1 << 24)"),
        "integer overflow",
    );
    assert_aelys_error_contains(
        &format!("{source}mul(1 << 46, 1 << 18)"),
        "integer overflow",
    );
    assert_aelys_int(&format!("{source}mul(1 << 23, 1 << 23)"), 1 << 46);
}

#[test]
fn sized_division_by_zero_traps_and_min_over_minus_one_wraps() {
    for ty in ["i8", "i16", "i32", "u8", "u16", "u32"] {
        let source = format!("fn d(a: {ty}, b: {ty}) -> {ty} {{ a / b }}\nd(1, 0)");
        assert_aelys_error_contains(&source, "division by zero");
        let source = format!("fn r(a: {ty}, b: {ty}) -> {ty} {{ a % b }}\nr(1, 0)");
        assert_aelys_error_contains(&source, "division by zero");
    }
    assert_aelys_int(
        "fn d(a: i32, b: i32) -> i32 { a / b }\nd(-2147483648, -1) as int",
        -2147483648,
    );
    assert_aelys_int("fn r(a: i8, b: i8) -> i8 { a % b }\nr(-128, -1) as int", 0);
}

#[test]
fn sized_values_index_collections() {
    let source = r#"
let v = vec![10, 20, 30, 40]
let i: u32 = 3
let j: i16 = 1
let mut a = [0, 0, 0]
a[j] = 5
"{v[i]},{a[1]}"
"#;
    assert_eq!(run_string(source, OptimizationLevel::Standard), "40,5");
}

const JIT_TARGET: bool = cfg!(all(target_os = "linux", target_arch = "x86_64"));

const JIT_DRIVER: &str = r#"
let mut total = 0
let mut index = 0
while index < 12000 {
    total = outer(index)
    index = index + 1
}
total
"#;

fn run_tiered(prelude: &str) -> (i64, usize) {
    let source = format!("{prelude}{JIT_DRIVER}");
    let runtime = Runtime::with_jit_mode(JitMode::Tiered);
    let options = CompileOptions {
        optimization_level: OptimizationLevel::None,
        ..CompileOptions::default()
    };
    let module = runtime.compile(&source, options).expect("compile");
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    let outcome = isolate
        .execute(&module, RunOptions::default())
        .expect("run");
    let ExecutionOutcome::Returned(value) = outcome else {
        panic!("workload must return, got {outcome:?}");
    };
    (value.as_int().expect("int"), runtime.jit_cache_entries())
}

#[test]
fn the_jit_refuses_sized_arithmetic_and_the_result_stays_exact() {
    let sized = r#"
fn outer(value: int) -> int {
    let a: i32 = 2147483647
    let b = value as i32
    let c: i32 = a + b
    let d: u8 = 250
    let e: u8 = d * 3
    return c as int + e as int + a.mul_shr(b, 16) as int
}
"#;
    let (value, entries) = run_tiered(sized);
    assert_eq!(value, -2147471650 + 238 + 393183231);
    assert_eq!(
        entries, 0,
        "a function with sized arithmetic must stay interpreted"
    );
    let control = r#"
fn outer(value: int) -> int {
    return value + 1
}
"#;
    let (value, entries) = run_tiered(control);
    assert_eq!(value, 12000);
    if JIT_TARGET {
        assert!(entries > 0, "the harness must observe a compiled function");
    }
}

fn compile_function(source: &str) -> aelys_runtime::Function {
    let src = aelys_syntax::Source::new("<test>", source);
    let tokens = aelys_frontend::lexer::Lexer::with_source(src.clone())
        .scan()
        .expect("lex");
    let stmts = aelys_frontend::parser::Parser::new(tokens, src.clone())
        .parse()
        .expect("parse");
    let typed = aelys_sema::TypeInference::infer_program(stmts, src.clone()).expect("types");
    aelys_backend::Compiler::new(None, src)
        .compile_typed(&typed)
        .expect("compile")
        .0
}

fn run_avbc(bytes: &[u8]) -> Result<i64, String> {
    let runtime = Runtime::with_jit_mode(JitMode::Off);
    let module = runtime
        .load_avbc(bytes, "<avbc>")
        .map_err(|error| error.to_string())?;
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    let outcome = isolate
        .execute(&module, RunOptions::default())
        .map_err(|error| error.to_string())?;
    let ExecutionOutcome::Returned(value) = outcome else {
        return Err(format!("no value: {outcome:?}"));
    };
    value.as_int().ok_or_else(|| "not an int".to_string())
}

const ENCODED: &str = r#"
fn f(a: i32, b: u8, c: i16) -> int {
    let x: i32 = a * 3 + a
    let y: u8 = b + 7
    let z: i16 = -c
    let w: u8 = ~b
    let q: i32 = x.mul_shr(a, 4)
    let r: i32 = x.shl_div(5, 3)
    x as int + y as int + z as int + w as int + q as int + r as int + (a as u16) as int
}
f(0x7FFFFFFF, 250, -32768)
"#;

const ENCODED_VALUE: i64 = -4 + 1 - 32768 + 5 - 536870912 - 6 + 65535;

#[test]
fn sized_instructions_survive_the_binary_format_and_its_verifier() {
    let runtime = Runtime::with_jit_mode(JitMode::Off);
    let module = runtime
        .compile(ENCODED, CompileOptions::default())
        .expect("compile");
    let bytes = module.avbc().to_vec();
    assert_eq!(u16::from_le_bytes([bytes[4], bytes[5]]), 5);
    assert_eq!(run_avbc(&bytes), Ok(ENCODED_VALUE));

    let mut legacy = bytes.clone();
    legacy[4..6].copy_from_slice(&4u16.to_le_bytes());
    let refused = run_avbc(&legacy).expect_err("a version 4 file cannot carry sized instructions");
    assert!(refused.contains("sized integer"), "{refused}");

    let text = aelys_bytecode::asm::disassemble(&compile_function(ENCODED));
    assert!(text.contains("MulWI32") && text.contains("MulShr") && text.contains("NegW"));
    let bytes = aelys_bytecode::asm::serialize(&assemble_program(&text)).expect("serialize");
    assert_eq!(run_avbc(&bytes), Ok(ENCODED_VALUE));
}

fn assemble_program(text: &str) -> aelys_runtime::Function {
    let mut functions = aelys_bytecode::asm::assemble(text).expect("assemble");
    let mut root = functions.remove(0);
    root.nested_functions = functions;
    root
}

#[test]
fn the_verifier_refuses_a_malformed_sized_instruction() {
    let text = aelys_bytecode::asm::disassemble(&compile_function(ENCODED));
    let line = text
        .lines()
        .find(|line| line.contains("NegW"))
        .expect("a sized negation");
    let width = line.rsplit(',').next().expect("width operand");
    let broken = line.replacen(width, " 9", 1);
    let bytes = aelys_bytecode::asm::serialize(&assemble_program(&text.replacen(line, &broken, 1)))
        .expect("serialize");
    let refused = run_avbc(&bytes).expect_err("width 9 names no integer type");
    assert!(refused.contains("invalid integer width 9"), "{refused}");
}

#[test]
fn a_program_without_sized_instructions_keeps_its_old_format_version() {
    let runtime = Runtime::with_jit_mode(JitMode::Off);
    let module = runtime
        .compile("let a = 1\na + 2", CompileOptions::default())
        .expect("compile");
    assert_eq!(u16::from_le_bytes([module.avbc()[4], module.avbc()[5]]), 3);
}

#[test]
fn sized_arithmetic_works_beyond_the_narrow_register_window() {
    let mut source = String::from("fn f(seed: i32) -> i32 {\n");
    for index in 0..300 {
        source.push_str(&format!("    let v{index}: i32 = seed + {index}\n"));
    }
    source.push_str("    let mut acc: i32 = v299 * v298\n    acc = acc - v297\n    acc = -acc\n    acc = acc.mul_shr(v296, 1)\n    acc\n}\nf(2147483000) as int\n");
    let expected = {
        let w = |v: i64| v as i32 as i64;
        let v = |i: i64| w(2147483000 + i);
        let mut acc = w(v(299) * v(298));
        acc = w(acc - v(297));
        acc = w(-acc);
        w((acc * v(296)) >> 1)
    };
    assert_aelys_int(&source, expected);
}
