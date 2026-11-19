use crate::prelude::DISPLAY_TRAIT;
use crate::types::InferType;

const NATIVE_MODULES: [&str; 7] = ["io", "sys", "time", "fs", "net", "math", "string"];

#[derive(Debug, Clone, PartialEq)]
pub enum NativeObligation {
    Bound(&'static str),
    OneOf(Vec<InferType>),
}

fn function(arity: usize, ret: InferType) -> InferType {
    InferType::function((0..arity).map(open_param).collect(), ret)
}

fn open_param(index: usize) -> InferType {
    InferType::Param(format!("T{index}"))
}

fn function_with(params: Vec<InferType>, ret: InferType) -> InferType {
    InferType::function(params, ret)
}

fn numeric_function(arity: usize, ret: InferType) -> InferType {
    function_with(vec![InferType::Numeric; arity], ret)
}

fn option(inner: InferType) -> InferType {
    InferType::Option(Box::new(inner))
}

fn result(ok: InferType, err: InferType) -> InferType {
    InferType::Result(Box::new(ok), Box::new(err))
}

fn byte_vec() -> InferType {
    InferType::Vec(Box::new(InferType::U8))
}

pub fn function_signature(path: &str) -> Option<InferType> {
    if let Some(signature) = builtin_signature(path) {
        return Some(signature);
    }
    if !path.contains("::") {
        for module in NATIVE_MODULES {
            if let Some(signature) = function_signature(&format!("{module}::{path}")) {
                return Some(signature);
            }
        }
        return None;
    }
    let (module, name) = path.split_once("::")?;
    let signature = match (module, name) {
        ("io", "print" | "println" | "eprint" | "eprintln" | "print_inline") => {
            function(1, InferType::Unit)
        }
        ("io", "flush" | "eflush") => function(0, InferType::Unit),
        ("io", "readline" | "read_char") => function(0, option(InferType::String)),
        ("io", "input") => function_with(vec![InferType::String], option(InferType::String)),
        ("io", "clear_screen" | "cursor_home" | "hide_cursor" | "show_cursor") => {
            function(0, InferType::Unit)
        }
        ("io", "move_cursor") => {
            function_with(vec![InferType::I64, InferType::I64], InferType::Unit)
        }

        ("sys", "args" | "env_vars" | "platform" | "arch" | "os" | "hostname") => {
            function(0, InferType::String)
        }
        ("sys", "cwd") => function(0, result(InferType::String, InferType::String)),
        ("sys", "arg") => function_with(vec![InferType::I64], option(InferType::String)),
        ("sys", "env") => function_with(vec![InferType::String], option(InferType::String)),
        ("sys", "script_path" | "script_dir" | "home") => function(0, option(InferType::String)),
        ("sys", "arg_count" | "pid" | "cpu_count") => function(0, InferType::I64),
        ("sys", "exec") => function_with(
            vec![InferType::String],
            result(InferType::I64, InferType::String),
        ),
        ("sys", "exec_output") => function_with(
            vec![InferType::String],
            result(InferType::String, InferType::String),
        ),
        ("sys", "exec_args" | "exec_args_output") => function_with(
            vec![InferType::String, InferType::String],
            if name == "exec_args" {
                result(InferType::I64, InferType::String)
            } else {
                result(InferType::String, InferType::String)
            },
        ),
        ("sys", "set_env") => {
            function_with(vec![InferType::String, InferType::String], InferType::Unit)
        }
        ("sys", "unset_env") => function_with(vec![InferType::String], InferType::Unit),
        ("sys", "set_cwd") => function_with(
            vec![InferType::String],
            result(InferType::Unit, InferType::String),
        ),
        ("sys", "random_seed" | "random_set_state") => {
            function_with(vec![InferType::I64], InferType::Unit)
        }
        ("sys", "exit") => function_with(vec![InferType::I64], InferType::Never),
        ("sys", "random") => function(0, InferType::F64),
        ("sys", "random_int") => function_with(
            vec![InferType::I64, InferType::I64],
            result(InferType::I64, InferType::String),
        ),
        ("sys", "random_state") => function(0, InferType::I64),
        (
            "sys",
            "gc_max_pause_us"
            | "gc_window_max_pause_us"
            | "gc_p99_pause_us"
            | "gc_max_pause_cpu_us"
            | "gc_total_pause_us"
            | "gc_pause_count"
            | "gc_minor_collections"
            | "gc_major_collections"
            | "gc_heap_bytes"
            | "gc_live_objects"
            | "gc_allocations",
        ) => function(0, InferType::I64),
        ("sys", "gc_stats") => function(0, InferType::String),
        ("sys", "gc_collect") => function(0, InferType::Unit),

        ("time", "now") => function(0, InferType::F64),
        ("time", "now_us" | "now_ns") => function(0, InferType::I64),
        ("time", "elapsed") => function_with(vec![InferType::I64], InferType::F64),
        ("time", "now_ms") => function(0, InferType::I64),
        ("time", "elapsed_ms" | "elapsed_us") => {
            function_with(vec![InferType::I64], InferType::I64)
        }
        (
            "time",
            "year" | "month" | "day" | "hour" | "minute" | "second" | "weekday" | "yearday",
        ) => function(0, InferType::I64),
        ("time", "timer") => function(0, InferType::I64),
        ("time", "reset") => function_with(vec![InferType::I64], InferType::Unit),
        ("time", "sleep" | "sleep_us") => function_with(vec![InferType::I64], InferType::Unit),
        ("time", "format") => function_with(vec![InferType::String], InferType::String),
        ("time", "iso" | "date" | "time_str") => function(0, InferType::String),

        ("fs", "open") => function_with(
            vec![InferType::String, InferType::String],
            result(InferType::I64, InferType::String),
        ),
        ("fs", "close") => function_with(
            vec![InferType::I64],
            result(InferType::Unit, InferType::String),
        ),
        ("fs", "read" | "read_all") => function_with(
            vec![InferType::I64],
            result(InferType::String, InferType::String),
        ),
        ("fs", "read_line") => function_with(
            vec![InferType::I64],
            result(option(InferType::String), InferType::String),
        ),
        ("fs", "read_chunk") => function_with(
            vec![InferType::I64, InferType::I64],
            result(byte_vec(), InferType::String),
        ),
        ("fs", "read_bytes") => function_with(
            vec![InferType::String],
            result(byte_vec(), InferType::String),
        ),
        ("fs", "write" | "write_line") => function_with(
            vec![InferType::I64, InferType::String],
            result(InferType::I64, InferType::String),
        ),
        ("fs", "write_chunk") => function_with(
            vec![InferType::I64, byte_vec()],
            result(InferType::I64, InferType::String),
        ),
        ("fs", "write_bytes") => function_with(
            vec![InferType::String, byte_vec()],
            result(InferType::Unit, InferType::String),
        ),
        ("fs", "read_text") => function_with(
            vec![InferType::String],
            result(InferType::String, InferType::String),
        ),
        ("fs", "write_text" | "append_text") => function_with(
            vec![InferType::String, InferType::String],
            result(InferType::I64, InferType::String),
        ),
        ("fs", "mkdir" | "mkdir_all" | "rmdir" | "delete") => function_with(
            vec![InferType::String],
            result(InferType::Bool, InferType::String),
        ),
        ("fs", "rename" | "copy") => function_with(
            vec![InferType::String, InferType::String],
            if name == "rename" {
                result(InferType::Bool, InferType::String)
            } else {
                result(InferType::I64, InferType::String)
            },
        ),
        ("fs", "exists" | "is_file" | "is_dir") => {
            function_with(vec![InferType::String], InferType::Bool)
        }
        ("fs", "size") => function_with(
            vec![InferType::String],
            result(InferType::I64, InferType::String),
        ),
        ("fs", "readdir") => function_with(
            vec![InferType::String],
            result(InferType::String, InferType::String),
        ),
        ("fs", "basename" | "dirname" | "extension") => {
            function_with(vec![InferType::String], InferType::String)
        }
        ("fs", "absolute") => function_with(
            vec![InferType::String],
            result(InferType::String, InferType::String),
        ),
        ("fs", "join") => function_with(
            vec![InferType::String, InferType::String],
            result(InferType::String, InferType::String),
        ),

        ("net", "connect") => function_with(
            vec![InferType::String, InferType::I64],
            option(InferType::I64),
        ),
        ("net", "connect_timeout") => function_with(
            vec![InferType::String, InferType::I64, InferType::I64],
            option(InferType::I64),
        ),
        ("net", "recv" | "recv_line") => {
            function_with(vec![InferType::I64], option(InferType::String))
        }
        ("net", "recv_bytes" | "udp_recv") => function_with(
            vec![InferType::I64, InferType::I64],
            option(InferType::String),
        ),
        ("net", "udp_recv_from") => function_with(
            vec![InferType::I64, InferType::I64],
            option(InferType::String),
        ),
        ("net", "local_addr" | "peer_addr") => {
            function_with(vec![InferType::I64], option(InferType::String))
        }
        ("net", "listen" | "udp_bind") => function_with(
            vec![InferType::String, InferType::I64],
            option(InferType::I64),
        ),
        ("net", "accept") => function_with(vec![InferType::I64], option(InferType::I64)),
        ("net", "udp_connect") => function_with(
            vec![InferType::I64, InferType::String, InferType::I64],
            result(InferType::Unit, InferType::String),
        ),
        ("net", "send" | "udp_send") => function_with(
            vec![InferType::I64, InferType::String],
            option(InferType::I64),
        ),
        ("net", "udp_send_to") => function_with(
            vec![InferType::I64, InferType::String, InferType::String],
            option(InferType::I64),
        ),
        ("net", "close") => function_with(
            vec![InferType::I64],
            result(InferType::Unit, InferType::String),
        ),
        ("net", "set_timeout") => function_with(
            vec![InferType::I64, InferType::I64],
            result(InferType::Unit, InferType::String),
        ),
        ("net", "set_nodelay") => function_with(
            vec![InferType::I64, InferType::Bool],
            result(InferType::Unit, InferType::String),
        ),
        ("net", "shutdown") => function_with(
            vec![InferType::I64, InferType::String],
            result(InferType::Unit, InferType::String),
        ),
        ("net", "udp_set_broadcast") => function_with(
            vec![InferType::I64, InferType::Bool],
            result(InferType::Unit, InferType::String),
        ),

        (
            "math",
            "sqrt" | "cbrt" | "sin" | "cos" | "tan" | "asin" | "acos" | "atan" | "sinh" | "cosh"
            | "tanh" | "exp" | "log" | "log10" | "log2" | "deg_to_rad" | "rad_to_deg",
        ) => numeric_function(1, InferType::F64),
        ("math", "atan2" | "hypot" | "fmod") => numeric_function(2, InferType::F64),
        ("math", "pow") => numeric_function(2, InferType::F64),
        ("math", "floor" | "ceil" | "round" | "trunc") => numeric_function(1, InferType::I64),
        ("math", "randint") => function_with(vec![InferType::I64, InferType::I64], InferType::I64),
        ("math", "is_nan" | "is_inf" | "is_finite") => numeric_function(1, InferType::Bool),
        ("math", "abs" | "sign") => numeric_function(1, InferType::Numeric),
        ("math", "min" | "max") => numeric_function(2, InferType::Numeric),
        ("math", "clamp") => numeric_function(3, InferType::Numeric),

        ("string", "len" | "char_len" | "byte_at" | "find" | "rfind" | "count" | "line_count") => {
            let params = match name {
                "len" | "char_len" | "line_count" => vec![InferType::String],
                "byte_at" => vec![InferType::String, InferType::I64],
                _ => vec![InferType::String, InferType::String],
            };
            function_with(params, InferType::I64)
        }
        ("string", "char_at") => {
            function_with(vec![InferType::String, InferType::I64], InferType::String)
        }
        ("string", "substr") => function_with(
            vec![InferType::String, InferType::I64, InferType::I64],
            InferType::String,
        ),
        ("string", "replace" | "replace_first") => function_with(
            vec![InferType::String, InferType::String, InferType::String],
            InferType::String,
        ),
        ("string", "split") => function_with(
            vec![InferType::String, InferType::String],
            InferType::Vec(Box::new(InferType::String)),
        ),
        ("string", "lines") => function_with(
            vec![InferType::String],
            InferType::Vec(Box::new(InferType::String)),
        ),
        ("string", "chars") => function_with(
            vec![InferType::String],
            InferType::Vec(Box::new(InferType::Char)),
        ),
        ("string", "to_int") => function_with(vec![InferType::String], option(InferType::I64)),
        ("string", "to_int_radix") => function_with(
            vec![InferType::String, InferType::I64],
            option(InferType::I64),
        ),
        ("string", "to_float") => function_with(vec![InferType::String], option(InferType::F64)),
        ("string", "to_bool") => function_with(vec![InferType::String], option(InferType::Bool)),
        ("string", "to_bytes") => function_with(vec![InferType::String], byte_vec()),
        ("string", "from_char") => function_with(vec![InferType::Char], InferType::String),
        ("string", "join" | "concat") => function_with(
            vec![InferType::String, InferType::String],
            InferType::String,
        ),
        ("string", "repeat") => {
            function_with(vec![InferType::String, InferType::I64], InferType::String)
        }
        ("string", "pad_left" | "pad_right") => function_with(
            vec![InferType::String, InferType::I64, InferType::String],
            InferType::String,
        ),
        (
            "string",
            "to_upper" | "to_lower" | "capitalize" | "reverse" | "trim" | "trim_start" | "trim_end",
        ) => function_with(vec![InferType::String], InferType::String),
        ("string", "contains" | "starts_with" | "ends_with") => {
            function_with(vec![InferType::String, InferType::String], InferType::Bool)
        }
        (
            "string",
            "is_empty" | "is_whitespace" | "is_numeric" | "is_alphabetic" | "is_alphanumeric",
        ) => function_with(vec![InferType::String], InferType::Bool),

        _ => return None,
    };
    Some(signature)
}

pub fn is_removed_convert_name(name: &str) -> bool {
    matches!(
        name,
        "parse_int"
            | "parse_int_radix"
            | "parse_float"
            | "parse_bool"
            | "to_string"
            | "to_hex"
            | "to_binary"
            | "to_octal"
            | "to_radix"
            | "to_int"
            | "to_float"
            | "to_bool"
            | "ord"
            | "chr"
            | "type_of"
            | "is_int"
            | "is_float"
            | "is_string"
            | "is_bool"
            | "is_function"
    )
}

pub fn removed_convert_replacement(name: &str) -> &'static str {
    match name {
        "parse_int" => "write `s.to_int()`, which returns Option<int>",
        "parse_int_radix" => "write `s.to_int_radix(r)`, which returns Option<int>",
        "parse_float" => "write `s.to_float()`, which returns Option<float>",
        "parse_bool" => "write `s.to_bool()`, which returns Option<bool>",
        "to_string" => "write `x.to_string()`",
        "to_hex" => "write `n.to_hex()`",
        "to_binary" => "write `n.to_binary()`",
        "to_octal" => "write `n.to_octal()`",
        "to_radix" => "write `n.to_radix(r)`",
        "to_int" => "write `x as int` for a number or a char, `s.to_int()` for a string",
        "to_float" => "write `x as float` for a number, `s.to_float()` for a string",
        "to_bool" => "compare explicitly, e.g. `n != 0`, or write `s.to_bool()` to parse a string",
        "ord" => "write `c as int` for a char",
        "chr" => "write `n as char`",
        "type_of" | "is_int" | "is_float" | "is_string" | "is_bool" | "is_function" => {
            "every value has a static type; match on an enum when a value can take several shapes"
        }
        _ => {
            "parse with `s.to_int()` or `s.to_float()`, format with `x.to_string()`, convert with `as`"
        }
    }
}

pub fn builtin_signature(name: &str) -> Option<InferType> {
    match name {
        "__tostring" => Some(function(1, InferType::String)),
        "panic" => Some(function_with(vec![InferType::String], InferType::Never)),
        "assert" => Some(function_with(
            vec![InferType::Bool, InferType::String],
            InferType::Unit,
        )),
        _ => None,
    }
}

pub fn canonical_path(path: &str) -> Option<String> {
    if builtin_signature(path).is_some() {
        return Some(path.to_string());
    }
    if !path.contains("::") {
        return NATIVE_MODULES.iter().find_map(|module| {
            let candidate = format!("{module}::{path}");
            function_signature(&candidate).map(|_| candidate)
        });
    }
    function_signature(path).map(|_| path.to_string())
}

pub fn parameter_obligations(path: &str) -> Vec<Option<NativeObligation>> {
    let bare = path.rsplit("::").next().unwrap_or(path);
    if !matches!(
        bare,
        "print" | "println" | "eprint" | "eprintln" | "print_inline" | "__tostring"
    ) {
        return Vec::new();
    }
    let Some(canonical) = canonical_path(path) else {
        return Vec::new();
    };
    match canonical.as_str() {
        "__tostring" | "io::print" | "io::println" | "io::eprint" | "io::eprintln"
        | "io::print_inline" => {
            vec![Some(NativeObligation::Bound(DISPLAY_TRAIT))]
        }
        _ => Vec::new(),
    }
}

pub fn constant_signature(path: &str) -> Option<InferType> {
    match path {
        "PI" | "E" | "TAU" | "INF" | "NEG_INF" | "math::PI" | "math::E" | "math::TAU"
        | "math::INF" | "math::NEG_INF" => Some(InferType::F64),
        _ => None,
    }
}

pub fn removed_bytes_replacement(name: &str) -> &'static str {
    match name {
        "alloc" => "write `let mut v: Vec<u8> = vec![0; n]`",
        "free" => "drop the call: a `Vec<u8>` is reclaimed by the garbage collector",
        "size" => "write `v.len()`",
        "resize" => "write `v.resize(n, 0)`",
        "clone" => "write `v.clone()`",
        "equals" => "write `v == w`",
        "read_u8" => "write `v[offset]` (a `u8`) or `v.read_u8(offset)` (an `int`)",
        "write_u8" => "write `v[offset] = x` or `v.write_u8(offset, x)`",
        "read_i8" => "write `v.read_i8(offset)`",
        "write_i8" => "write `v.write_i8(offset, x)`",
        "read_u16" | "read_i16" | "read_u32" | "read_i32" | "read_u64" | "read_i64"
        | "read_f32" | "read_f64" => {
            "call the little-endian method on the Vec<u8>: `v.read_u16_le(offset)` (likewise `read_i16_le`, `read_u32_le`, `read_i32_le`, `read_u64_le`, `read_i64_le`, `read_f32_le`, `read_f64_le`)"
        }
        "write_u16" | "write_i16" | "write_u32" | "write_i32" | "write_u64" | "write_i64"
        | "write_f32" | "write_f64" => {
            "call the little-endian method on the Vec<u8>: `v.write_u16_le(offset, x)` (likewise for i16, u32, i32, u64, i64, f32 and f64)"
        }
        name if name.starts_with("read_") => {
            "call the method on the Vec<u8>: `v.read_u16_be(offset)`"
        }
        name if name.starts_with("write_") => {
            "call the method on the Vec<u8>: `v.write_u16_be(offset, x)`"
        }
        "copy" => {
            "write `v.copy_within(start, end, dest)` inside one Vec<u8>, or index both Vecs in a loop"
        }
        "copy_from_vec" | "copy_to_vec" => {
            "keep the data in a `Vec<u8>` and pass it to the native as `&v`, or `&mut v` when it writes"
        }
        "fill" => "write `v.fill(x)`",
        "from_string" => "write `s.to_bytes()`",
        "decode" => "write `v[start..end].to_utf8()`, which is `None` for invalid UTF-8",
        "write_string" => {
            "append `s.to_bytes()` with `v.extend(...)`, or store its bytes with indexing"
        }
        "find" => "write `v.index_of(b)`",
        "reverse" => "write `v.reverse()`",
        "swap" => "write `v.swap(i, j)`",
        _ => "see the `Vec<u8>` methods in the standard library documentation",
    }
}
