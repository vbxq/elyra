use crate::types::InferType;

fn function(arity: usize, ret: InferType) -> InferType {
    InferType::Function {
        params: vec![InferType::Dynamic; arity],
        ret: Box::new(ret),
    }
}

fn function_with(params: Vec<InferType>, ret: InferType) -> InferType {
    InferType::Function {
        params,
        ret: Box::new(ret),
    }
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

pub fn function_signature(path: &str) -> Option<InferType> {
    if let Some(signature) = builtin_signature(path) {
        return Some(signature);
    }
    if !path.contains("::") {
        for module in [
            "io", "convert", "sys", "time", "fs", "net", "math", "string", "bytes",
        ] {
            if let Some(signature) = function_signature(&format!("{module}::{path}")) {
                return Some(signature);
            }
        }
        return None;
    }
    let (module, name) = path.split_once("::")?;
    let signature = match (module, name) {
        ("io", "print" | "println" | "eprint" | "eprintln" | "print_inline") => {
            function_with(vec![InferType::Dynamic], InferType::Unit)
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

        ("convert", "parse_int") => function_with(vec![InferType::String], option(InferType::I64)),
        ("convert", "parse_int_radix") => function_with(
            vec![InferType::String, InferType::I64],
            option(InferType::I64),
        ),
        ("convert", "parse_float") => {
            function_with(vec![InferType::String], option(InferType::F64))
        }
        ("convert", "parse_bool") => {
            function_with(vec![InferType::String], option(InferType::Bool))
        }
        ("convert", "to_string" | "type_of") => {
            function_with(vec![InferType::Dynamic], InferType::String)
        }
        ("convert", "to_hex" | "to_binary" | "to_octal") => {
            function_with(vec![InferType::I64], InferType::String)
        }
        ("convert", "to_radix") => {
            function_with(vec![InferType::I64, InferType::I64], InferType::String)
        }
        ("convert", "chr") => function_with(vec![InferType::I64], InferType::String),
        ("convert", "to_int" | "to_float" | "to_bool") => function(
            1,
            match name {
                "to_int" => InferType::I64,
                "to_float" => InferType::F64,
                _ => InferType::Bool,
            },
        ),
        ("convert", "ord") => function_with(vec![InferType::String], InferType::I64),
        ("convert", "is_int" | "is_float" | "is_string" | "is_bool") => {
            function(1, InferType::Bool)
        }
        ("convert", "is_function") => function(1, InferType::Bool),

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

        ("time", "now" | "now_us" | "now_ns") => function(0, InferType::F64),
        ("time", "elapsed" | "elapsed_us") => function_with(vec![InferType::I64], InferType::F64),
        ("time", "now_ms") => function(0, InferType::I64),
        ("time", "elapsed_ms") => function_with(vec![InferType::I64], InferType::I64),
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
        ("fs", "read_bytes") => function_with(
            vec![InferType::I64, InferType::I64],
            result(InferType::I64, InferType::String),
        ),
        ("fs", "write" | "write_bytes" | "write_line") => function_with(
            vec![InferType::I64, InferType::String],
            result(InferType::I64, InferType::String),
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
        ("string", "split" | "join" | "concat") => function_with(
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
            "chars" | "bytes" | "to_upper" | "to_lower" | "capitalize" | "reverse" | "trim"
            | "trim_start" | "trim_end" | "lines",
        ) => function_with(vec![InferType::String], InferType::String),
        ("string", "contains" | "starts_with" | "ends_with") => {
            function_with(vec![InferType::String, InferType::String], InferType::Bool)
        }
        (
            "string",
            "is_empty" | "is_whitespace" | "is_numeric" | "is_alphabetic" | "is_alphanumeric",
        ) => function_with(vec![InferType::String], InferType::Bool),

        ("bytes", "alloc") => function_with(vec![InferType::I64], InferType::I64),
        ("bytes", "from_string") => function_with(vec![InferType::String], InferType::I64),
        ("bytes", "clone" | "size" | "free") => function_with(
            vec![InferType::I64],
            if name == "free" {
                InferType::Unit
            } else {
                InferType::I64
            },
        ),
        ("bytes", "resize") => function_with(vec![InferType::I64, InferType::I64], InferType::Unit),
        ("bytes", "equals") => function_with(vec![InferType::I64, InferType::I64], InferType::Bool),
        (
            "bytes",
            "read_u8" | "read_i8" | "read_u16" | "read_i16" | "read_u16_be" | "read_i16_be"
            | "read_u32" | "read_i32" | "read_u32_be" | "read_i32_be" | "read_u64" | "read_i64"
            | "read_u64_be" | "read_i64_be",
        ) => function_with(vec![InferType::I64, InferType::I64], InferType::I64),
        ("bytes", "read_f32" | "read_f64" | "read_f32_be" | "read_f64_be") => {
            function_with(vec![InferType::I64, InferType::I64], InferType::F64)
        }
        ("bytes", "write_string") => function_with(
            vec![InferType::I64, InferType::I64, InferType::String],
            InferType::I64,
        ),
        ("bytes", "find") => function_with(
            vec![
                InferType::I64,
                InferType::I64,
                InferType::I64,
                InferType::I64,
            ],
            InferType::I64,
        ),
        ("bytes", "decode") => function_with(
            vec![InferType::I64, InferType::I64, InferType::I64],
            InferType::String,
        ),
        (
            "bytes",
            "write_u8" | "write_i8" | "write_u16" | "write_i16" | "write_u16_be" | "write_i16_be"
            | "write_u32" | "write_i32" | "write_u32_be" | "write_i32_be" | "write_u64"
            | "write_i64" | "write_u64_be" | "write_i64_be",
        ) => function_with(
            vec![InferType::I64, InferType::I64, InferType::I64],
            InferType::Unit,
        ),
        ("bytes", "write_f32" | "write_f64" | "write_f32_be" | "write_f64_be") => function_with(
            vec![InferType::I64, InferType::I64, InferType::Numeric],
            InferType::Unit,
        ),
        ("bytes", "copy") => function_with(
            vec![
                InferType::I64,
                InferType::I64,
                InferType::I64,
                InferType::I64,
                InferType::I64,
            ],
            InferType::Unit,
        ),
        ("bytes", "fill") => function_with(
            vec![
                InferType::I64,
                InferType::I64,
                InferType::I64,
                InferType::I64,
            ],
            InferType::Unit,
        ),
        ("bytes", "reverse" | "swap") => function_with(
            vec![InferType::I64, InferType::I64, InferType::I64],
            InferType::Unit,
        ),

        _ => return None,
    };
    Some(signature)
}

pub fn builtin_signature(name: &str) -> Option<InferType> {
    match name {
        "__tostring" | "type" => Some(function(1, InferType::String)),
        _ => None,
    }
}

pub fn constant_signature(path: &str) -> Option<InferType> {
    match path {
        "PI" | "E" | "TAU" | "INF" | "NEG_INF" | "math::PI" | "math::E" | "math::TAU"
        | "math::INF" | "math::NEG_INF" => Some(InferType::F64),
        _ => None,
    }
}
