use aelys_bytecode::object::SumTag;
use aelys_runtime::Value;
use aelys_runtime::stdlib::{ByteBuffer, Resource, register_std_module};
use aelys_runtime::vm::{GcRef, ObjectKind, VM};
use aelys_sema::InferType;
use aelys_sema::native::function_signature;
use aelys_syntax::Source;
use std::collections::BTreeSet;
use std::path::Path;

const MODULES: [&str; 9] = [
    "io", "convert", "sys", "time", "fs", "net", "math", "string", "bytes",
];

// blocking on stdin, ending the process, or mutating process-wide state shared with other tests
const NOT_CALLED: &[&str] = &[
    "io::readline",
    "io::read_char",
    "io::input",
    "sys::exit",
    "sys::exec",
    "sys::exec_output",
    "sys::exec_args",
    "sys::exec_args_output",
    "sys::set_cwd",
    "sys::set_env",
    "sys::unset_env",
    "sys::random_seed",
    "sys::random_set_state",
    "net::connect",
    "net::connect_timeout",
    "net::accept",
    "net::recv",
    "net::recv_line",
    "net::recv_bytes",
    "net::udp_recv",
    "net::udp_recv_from",
];

struct Fixture {
    dir: tempfile::TempDir,
}

impl Fixture {
    fn path(&self, name: &str) -> String {
        self.dir.path().join(name).to_string_lossy().into_owned()
    }
}

fn string(vm: &mut VM, text: &str) -> Value {
    Value::ptr(vm.alloc_string(text).unwrap().index())
}

fn handle(index: usize) -> Value {
    Value::int(i64::try_from(index).unwrap())
}

fn open_file(vm: &mut VM, path: &str, mode: &str) -> Value {
    let path = string(vm, path);
    let mode = string(vm, mode);
    let result = vm.call_function_by_name("fs::open", &[path, mode]).unwrap();
    let pointer = result.as_ptr().unwrap();
    match &vm.heap().get(GcRef::new(pointer)).unwrap().kind {
        ObjectKind::Sum(sum) if sum.tag == SumTag::ResultOk => sum.payload,
        other => panic!("fs::open failed: {other:?}"),
    }
}

fn arguments(vm: &mut VM, fixture: &Fixture, path: &str, params: &[InferType]) -> Vec<Value> {
    let (module, name) = path.split_once("::").unwrap();
    let file = fixture.path("data.txt");
    std::fs::write(&file, "alpha\nbeta\n").unwrap();
    let mut out = Vec::new();
    for (index, param) in params.iter().enumerate() {
        let value = match (module, name, index, param) {
            ("bytes", "alloc", 0, _) => Value::int(8),
            ("bytes", _, 0, InferType::I64) | ("bytes", "equals", 1, InferType::I64) => {
                handle(vm.store_resource(Resource::ByteBuffer(ByteBuffer { data: vec![0; 16] })))
            }
            ("time", "elapsed" | "elapsed_ms" | "elapsed_us" | "reset", 0, _) => {
                handle(vm.store_resource(Resource::Timer(std::time::Instant::now())))
            }
            ("time", "sleep" | "sleep_us", 0, _) => Value::int(0),
            ("fs", "close" | "read" | "read_all" | "read_line" | "read_bytes", 0, _) => {
                open_file(vm, &file, "r")
            }
            ("fs", "write" | "write_bytes" | "write_line", 0, _) => {
                open_file(vm, &fixture.path("out.txt"), "w")
            }
            ("fs", "mkdir" | "mkdir_all" | "rmdir", 0, _) => {
                string(vm, &fixture.path(&format!("dir_{name}")))
            }
            ("fs", "rename", 1, _) => string(vm, &fixture.path("renamed.txt")),
            ("fs", "copy", 1, _) => string(vm, &fixture.path("copied.txt")),
            ("fs", "delete" | "rename", 0, _) => {
                let victim = fixture.path(&format!("victim_{name}.txt"));
                std::fs::write(&victim, "x").unwrap();
                string(vm, &victim)
            }
            ("fs", "readdir", 0, _) => string(vm, &fixture.dir.path().to_string_lossy()),
            ("fs", "write_text" | "append_text", 0, _) => string(vm, &fixture.path("text.txt")),
            ("fs", _, 0, InferType::String) => string(vm, &file),
            ("net", "listen" | "udp_bind", 0, _) => string(vm, "127.0.0.1"),
            ("net", "listen" | "udp_bind", 1, _) => Value::int(0),
            ("net", "udp_connect" | "udp_send_to", 1, _) => string(vm, "127.0.0.1"),
            ("net", "udp_send_to", 2, _) => string(vm, "x"),
            ("net", "udp_connect", 2, _) => Value::int(9),
            ("net", "shutdown", 1, _) => string(vm, "both"),
            ("net", _, 0, InferType::I64) => {
                let host = string(vm, "127.0.0.1");
                let socket = vm
                    .call_function_by_name("net::udp_bind", &[host, Value::int(0)])
                    .unwrap();
                let pointer = socket.as_ptr().unwrap();
                match &vm.heap().get(GcRef::new(pointer)).unwrap().kind {
                    ObjectKind::Sum(sum) => sum.payload,
                    other => panic!("net::udp_bind failed: {other:?}"),
                }
            }
            ("convert", "parse_int_radix" | "to_radix", 1, _) => Value::int(16),
            ("time", "format", 0, _) => string(vm, "%Y"),
            ("string", "pad_left" | "pad_right", 2, _) => string(vm, "*"),
            ("bytes", "decode", 2, _) | ("bytes", "copy", 4, _) => Value::int(2),
            (_, _, _, InferType::I64 | InferType::Param(_)) => Value::int(1),
            (_, _, _, InferType::Numeric) => Value::int(2),
            (_, _, _, InferType::Bool) => Value::bool(true),
            (_, _, _, InferType::String) => string(vm, "12"),
            (_, _, _, other) => panic!("{path}: no argument for parameter type {other:?}"),
        };
        out.push(value);
    }
    out
}

fn describe(vm: &VM, value: Value) -> String {
    if value.is_int() {
        return "int".into();
    }
    if value.is_float() {
        return "float".into();
    }
    if value.is_bool() {
        return "bool".into();
    }
    if value.is_unit() {
        return "unit".into();
    }
    if value.is_null() {
        return "null".into();
    }
    if value.is_none() {
        return "None".into();
    }
    match value.as_ptr().and_then(|p| vm.heap().get(GcRef::new(p))) {
        Some(object) => match &object.kind {
            ObjectKind::String(_) => "string".into(),
            ObjectKind::Sum(sum) => format!("{:?}({})", sum.tag, describe(vm, sum.payload)),
            other => format!("object {other:?}"),
        },
        None => format!("unknown {value:?}"),
    }
}

fn matches(vm: &VM, value: Value, expected: &InferType) -> bool {
    match expected {
        InferType::I64 => value.is_int(),
        InferType::F64 => value.is_float(),
        InferType::Bool => value.is_bool(),
        InferType::Unit => value.is_unit(),
        InferType::Numeric => value.is_int() || value.is_float(),
        InferType::String => value
            .as_ptr()
            .and_then(|p| vm.heap().get(GcRef::new(p)))
            .is_some_and(|o| matches!(o.kind, ObjectKind::String(_))),
        InferType::Option(inner) => {
            value.is_none() || sum_payload(vm, value, SumTag::OptionSome, inner)
        }
        InferType::Result(ok, err) => {
            sum_payload(vm, value, SumTag::ResultOk, ok)
                || sum_payload(vm, value, SumTag::ResultErr, err)
        }
        _ => false,
    }
}

fn sum_payload(vm: &VM, value: Value, tag: SumTag, inner: &InferType) -> bool {
    match value.as_ptr().and_then(|p| vm.heap().get(GcRef::new(p))) {
        Some(object) => match &object.kind {
            ObjectKind::Sum(sum) if sum.tag == tag => matches(vm, sum.payload, inner),
            _ => false,
        },
        None => false,
    }
}

fn arity_of(vm: &VM, path: &str) -> u16 {
    let value = vm.get_function_value(path).unwrap();
    match &vm
        .heap()
        .get(GcRef::new(value.as_ptr().unwrap()))
        .unwrap()
        .kind
    {
        ObjectKind::Native(native) => native.arity,
        other => panic!("{path} is not a native: {other:?}"),
    }
}

#[test]
fn every_standard_native_returns_the_type_sema_gives_it() {
    let fixture = Fixture {
        dir: tempfile::tempdir().unwrap(),
    };
    let mut checked = BTreeSet::new();
    let mut failures = Vec::new();
    let mut runtime_errors = Vec::new();
    let mut registered = BTreeSet::new();
    for module in MODULES {
        let mut vm = VM::new(Source::new("signatures", "")).unwrap();
        for other in MODULES {
            register_std_module(&mut vm, other).unwrap();
        }
        let exports = register_std_module(&mut vm, module).unwrap();
        for path in exports.native_functions {
            registered.insert(path.clone());
            let Some(InferType::Function { params, ret }) = function_signature(&path) else {
                failures.push(format!(
                    "{path}: registered at runtime but sema has no signature"
                ));
                continue;
            };
            let arity = arity_of(&vm, &path);
            if usize::from(arity) != params.len() {
                failures.push(format!(
                    "{path}: runtime arity {arity}, sema {} parameters",
                    params.len()
                ));
                continue;
            }
            if NOT_CALLED.contains(&path.as_str()) || matches!(*ret, InferType::Never) {
                continue;
            }
            let mut variants = vec![arguments(&mut vm, &fixture, &path, &params)];
            if params.contains(&InferType::Numeric) {
                let floats = variants[0]
                    .iter()
                    .zip(&params)
                    .map(|(value, param)| match param {
                        InferType::Numeric => Value::float(2.5),
                        _ => *value,
                    })
                    .collect();
                variants.push(floats);
            }
            for args in variants {
                match vm.call_function_by_name(&path, &args) {
                    Ok(value) => {
                        if !matches(&vm, value, &ret) {
                            failures.push(format!(
                                "{path}: sema says {ret:?}, runtime returned {}",
                                describe(&vm, value)
                            ));
                        }
                        checked.insert(path.clone());
                    }
                    Err(error) => runtime_errors.push(format!("{path}: {error}")),
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(runtime_errors.is_empty(), "{}", runtime_errors.join("\n"));
    for path in NOT_CALLED {
        assert!(
            registered.contains(*path),
            "{path} is not registered any more"
        );
    }
    let expected_checked = registered.len() - NOT_CALLED.len();
    assert_eq!(
        checked.len(),
        expected_checked,
        "every registered native except the listed ones must have been called"
    );
    assert!(registered.len() > 200, "{} registered", registered.len());
    assert!(Path::new(&fixture.path("data.txt")).exists());
}
