use aelys_bytecode::asm::{BinaryError, MAX_REGISTERS, deserialize, disassemble_to_string};
use aelys_common::{AelysError, CompileErrorKind, RuntimeErrorKind};
use aelys_driver::run_file;
use aelys_runtime::{ExecutionControl, Function, OpCode, VM};
use aelys_syntax::Source;
use std::path::{Path, PathBuf};

// header is magic(4) version(2) flags(2) func_count(4) reserved(4), then the root function's
const ROOT_NUM_REGISTERS_OFFSET: usize = 20;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn minimal_program_bytes(num_registers: u32) -> Vec<u8> {
    let mut func = Function::new(None, 0);
    func.num_registers = num_registers;
    func.emit_a(OpCode::Return0, 0, 0, 0, 1);
    func.finalize_bytecode();
    func.num_registers = num_registers;
    aelys_bytecode::asm::serialize(&func).expect("a minimal function serializes")
}

fn program_with_patched_register_count(raw: u32) -> Vec<u8> {
    let mut bytes = minimal_program_bytes(1);
    let field = ROOT_NUM_REGISTERS_OFFSET..ROOT_NUM_REGISTERS_OFFSET + 4;
    let found = u32::from_le_bytes(
        bytes[field.clone()]
            .try_into()
            .expect("the register count field is four bytes"),
    );
    assert_eq!(
        found, 1,
        "the root register count did not land at offset {ROOT_NUM_REGISTERS_OFFSET}; the header \
         layout changed and this fixture needs updating"
    );
    bytes[field].copy_from_slice(&raw.to_le_bytes());
    bytes
}

#[test]
fn the_reader_rejects_an_oversized_register_count() {
    for raw in [0x3000_0000u32, 0xFFFF_FFF0, MAX_REGISTERS + 1] {
        let bytes = program_with_patched_register_count(raw);
        match deserialize(&bytes) {
            Err(BinaryError::LimitExceeded { what, limit }) => {
                assert_eq!(what, "register count");
                assert_eq!(limit, MAX_REGISTERS as usize);
            }
            Err(other) => panic!("raw {raw:#x} gave {other:?}, expected a register count limit"),
            Ok(_) => panic!("raw {raw:#x} was accepted, an unbounded frame allocation follows"),
        }
    }
}

#[test]
fn the_reader_still_accepts_the_largest_legal_register_count() {
    let bytes = program_with_patched_register_count(MAX_REGISTERS);
    let func = deserialize(&bytes).expect("the ceiling itself stays legal");
    assert_eq!(func.num_registers, MAX_REGISTERS);
}

#[test]
fn the_writer_refuses_to_emit_an_oversized_register_count() {
    let mut func = Function::new(None, 0);
    func.emit_a(OpCode::Return0, 0, 0, 0, 1);
    func.finalize_bytecode();
    func.num_registers = MAX_REGISTERS + 1;
    match aelys_bytecode::asm::serialize(&func) {
        Err(BinaryError::LimitExceeded { what, limit }) => {
            assert_eq!(what, "register count");
            assert_eq!(limit, MAX_REGISTERS as usize);
        }
        Err(other) => panic!("expected a register count limit, got {other:?}"),
        Ok(_) => panic!("the writer emitted a file its own reader must reject"),
    }
}

#[test]
fn the_verifier_rejects_an_oversized_register_count() {
    let mut vm = VM::new(Source::new("test.aelys", "")).expect("a bare vm starts");
    let mut func = Function::new(Some("wide".to_string()), 0);
    func.num_registers = 1;
    func.emit_a(OpCode::Return0, 0, 0, 0, 1);
    func.finalize_bytecode();
    func.num_registers = MAX_REGISTERS + 1;

    let func_ref = vm.alloc_function(func).expect("allocation succeeds");
    let err = vm
        .execute(func_ref)
        .expect_err("an oversized register count must not reach frame setup");
    match err.kind {
        RuntimeErrorKind::InvalidBytecode(message) => {
            assert!(
                message.contains("register count"),
                "expected a register count rejection, got {message}"
            );
        }
        other => panic!("expected InvalidBytecode, got {other:?}"),
    }
}

#[test]
fn a_corrupted_jump_target_stays_inside_the_instruction_budget() {
    let bytes = std::fs::read(fixture("nonterminating_jump.avbc")).expect("the fixture is present");
    let func = deserialize(&bytes).expect("the reader terminates and accepts the file");

    let mut vm = VM::new(Source::new("test.aelys", "")).expect("a bare vm starts");
    let func_ref = vm.alloc_function(func).expect("allocation succeeds");
    vm.configure_execution(ExecutionControl {
        max_instructions: Some(100_000),
        ..ExecutionControl::default()
    });

    let err = vm
        .execute(func_ref)
        .expect_err("the cycle never returns on its own");
    match err.kind {
        RuntimeErrorKind::InstructionBudgetExceeded { limit } => assert_eq!(limit, 100_000),
        other => panic!("expected the budget to stop the cycle, got {other:?}"),
    }
}

#[test]
fn disassembly_survives_an_invalid_wide_loop_opcode() {
    let mut func = Function::new(None, 0);
    func.num_registers = 1;
    func.set_bytecode(vec![
        ((OpCode::LoopWideLong as u32) << 24) | (0xFEu32 << 16),
        0,
        0,
        (OpCode::Return0 as u32) << 24,
    ]);

    let text = disassemble_to_string(&func);
    assert!(
        text.contains("LoopWideLong <invalid 254>"),
        "expected the raw byte to be printed, got:\n{text}"
    );
}

#[test]
fn stripping_debug_info_keeps_global_layout_names() {
    let mut func = Function::new(Some("helper".to_string()), 0);
    func.global_layout =
        aelys_runtime::GlobalLayout::new(vec!["helper".to_string(), "io::println".to_string()]);
    func.strip_debug_info();

    assert_eq!(
        func.global_layout.names(),
        &["helper".to_string(), "io::println".to_string()]
    );
}

fn compile_error_kind(source: &str, name: &str) -> CompileErrorKind {
    let dir = tempfile::tempdir().expect("a temp dir");
    let path = dir.path().join(name);
    std::fs::write(&path, source).expect("the source is written");
    match run_file(&path) {
        Err(AelysError::Compile(err)) => err.kind,
        Err(other) => panic!("expected a compile error, got {other:?}"),
        Ok(value) => panic!("expected a compile error, the program produced {value:?}"),
    }
}

#[test]
fn a_deeply_nested_type_annotation_is_a_named_diagnostic() {
    let depth = 1600;
    let source = format!(
        "let x: {}int{} = vec![]\n",
        "Vec<".repeat(depth),
        ">".repeat(depth)
    );
    let kind = compile_error_kind(&source, "deep_annotation.aelys");
    assert!(
        matches!(kind, CompileErrorKind::TypeNestingTooDeep { .. }),
        "expected TypeNestingTooDeep, got {kind:?}"
    );
    assert_eq!(kind.code(), 380);
}

#[test]
fn a_deeply_nested_parameter_annotation_is_a_named_diagnostic() {
    let depth = 1600;
    let source = format!(
        "fn f(v: {}int{}) -> int {{ 0 }}\nf(vec![])\n",
        "Vec<".repeat(depth),
        ">".repeat(depth)
    );
    let kind = compile_error_kind(&source, "deep_parameter.aelys");
    assert!(
        matches!(kind, CompileErrorKind::TypeNestingTooDeep { .. }),
        "expected TypeNestingTooDeep, got {kind:?}"
    );
}

#[test]
fn a_reasonably_nested_type_annotation_still_compiles() {
    let depth = 8;
    let source = format!(
        "let x: {}int{} = vec![]\n",
        "Vec<".repeat(depth),
        ">".repeat(depth)
    );
    let dir = tempfile::tempdir().expect("a temp dir");
    let path = dir.path().join("shallow_annotation.aelys");
    std::fs::write(&path, &source).expect("the source is written");
    run_file(&path).expect("eight levels of nesting stay inside the limit");
}

fn assemble_root(text: &str) -> Function {
    let functions = aelys_bytecode::asm::assemble(text).expect("the assembly is well formed");
    functions
        .into_iter()
        .next()
        .expect("the assembly declares a function")
}

fn run_assembly_with_heap(
    text: &str,
    max_heap_bytes: u64,
) -> Result<aelys_runtime::Value, RuntimeErrorKind> {
    let config = aelys_runtime::VmConfig::new(max_heap_bytes).expect("the budget is legal");
    let mut vm = VM::with_config(Source::new("test.aasm", ""), config).expect("a bare vm starts");
    let func_ref = vm
        .alloc_function(assemble_root(text))
        .expect("allocation succeeds");
    vm.execute(func_ref).map_err(|err| err.kind)
}

fn global_index_program(set_op: &str, index: &str) -> String {
    format!(
        ".version 3\n\
         .function 0\n\
        \x20 .arity 0\n\
        \x20 .registers 2\n\
        \x20 .code\n\
        \x20   0000: LoadI     r0, 7\n\
        \x20   0001: {set_op} {index}, r0\n\
        \x20   0002: Return    r0\n"
    )
}

#[test]
fn a_wide_global_index_grows_the_store_only_inside_the_heap_budget() {
    let kind = run_assembly_with_heap(
        &global_index_program("SetGlobalIdxWide", "20000000"),
        8 * 1024 * 1024,
    )
    .expect_err("the store is a plain Vec, and the budget has to reach it too");
    match kind {
        RuntimeErrorKind::OutOfMemory { requested, max } => {
            assert_eq!(requested, 20_000_001 * 16);
            assert_eq!(max, 8 * 1024 * 1024);
        }
        other => panic!("expected the budget to refuse the growth, got {other:?}"),
    }
}

#[test]
fn a_narrow_global_index_grows_the_store_only_inside_the_heap_budget() {
    // the narrow operand is sixteen bits, so 65535 is the largest index it can
    let kind = run_assembly_with_heap(
        &global_index_program("SetGlobalIdx", "-1"),
        aelys_runtime::VmConfig::MIN_HEAP_BYTES,
    )
    .expect_err("65536 slots do not fit the smallest legal budget");
    match kind {
        RuntimeErrorKind::OutOfMemory { requested, max } => {
            assert_eq!(requested, 65_536 * 16);
            assert_eq!(max, aelys_runtime::VmConfig::MIN_HEAP_BYTES);
        }
        other => panic!("expected the budget to refuse the growth, got {other:?}"),
    }
}

#[test]
fn a_global_index_that_fits_the_heap_budget_still_runs() {
    let text = ".version 3\n\
                .function 0\n\
                \x20 .arity 0\n\
                \x20 .registers 2\n\
                \x20 .code\n\
                \x20   0000: LoadI     r0, 7\n\
                \x20   0001: SetGlobalIdxWide 65534, r0\n\
                \x20   0002: GetGlobalIdxWide r1, 65534\n\
                \x20   0003: Return    r1\n";
    let value = run_assembly_with_heap(text, 8 * 1024 * 1024)
        .expect("a store inside the budget still grows");
    assert_eq!(value.as_int(), Some(7));
}
