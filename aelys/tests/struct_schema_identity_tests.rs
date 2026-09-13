use aelys::{CompileOptions, Runtime};
use aelys_bytecode::asm::{assemble, deserialize, disassemble, serialize};
use aelys_bytecode::{
    DefId, EnumSchema, EnumVariantSchema, Function, IntWidth, OpCode, StructFieldSchema,
    StructSchema, TypeDescriptor,
};
use aelys_runtime::{VM, Value};
use aelys_syntax::Source;

const AVBC_V3: u16 = 3;
const AVBC_V4: u16 = 4;

fn string_u16(text: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    let len = u16::try_from(text.len()).expect("test string fits u16");
    bytes.extend_from_slice(&len.to_le_bytes());
    bytes.extend_from_slice(text.as_bytes());
    bytes
}

fn def_id_bytes(package: &str, module: &[&str], ordinal: u32) -> Vec<u8> {
    let mut bytes = string_u16(package);
    let count = u16::try_from(module.len()).expect("test module path fits u16");
    bytes.extend_from_slice(&count.to_le_bytes());
    for segment in module {
        bytes.extend_from_slice(&string_u16(segment));
    }
    bytes.extend_from_slice(&ordinal.to_le_bytes());
    bytes
}

fn struct_record(
    schema_id: u32,
    package: &str,
    module: &[&str],
    ordinal: u32,
    type_args: &[Vec<u8>],
    fields: &[(u16, &str, Vec<u8>)],
) -> Vec<u8> {
    let mut bytes = schema_id.to_le_bytes().to_vec();
    bytes.extend_from_slice(&def_id_bytes(package, module, ordinal));
    let type_arg_count = u16::try_from(type_args.len()).expect("test type arg count fits u16");
    let field_count = u16::try_from(fields.len()).expect("test field count fits u16");
    bytes.extend_from_slice(&type_arg_count.to_le_bytes());
    bytes.extend_from_slice(&field_count.to_le_bytes());
    for descriptor in type_args {
        bytes.extend_from_slice(descriptor);
    }
    for (offset, name, descriptor) in fields {
        bytes.extend_from_slice(&offset.to_le_bytes());
        bytes.extend_from_slice(&string_u16(name));
        bytes.extend_from_slice(descriptor);
    }
    bytes
}

fn table(records: &[Vec<u8>]) -> Vec<u8> {
    let count = u16::try_from(records.len()).expect("test record count fits u16");
    let mut bytes = count.to_le_bytes().to_vec();
    bytes.extend_from_slice(&0u16.to_le_bytes());
    for record in records {
        bytes.extend_from_slice(record);
    }
    bytes
}

// hand rolled avbc so the reader is exercised on bytes the serializer would refuse to produce
fn program(version: u16, struct_table: &[u8], enum_table: &[u8]) -> Vec<u8> {
    let mut bytes = b"VBXQ".to_vec();
    bytes.extend_from_slice(&version.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(struct_table);
    if version >= AVBC_V4 {
        bytes.extend_from_slice(enum_table);
    }
    bytes.push(0);
    bytes.push(0);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes
}

fn int64() -> Vec<u8> {
    vec![2, IntWidth::I64 as u8]
}

fn struct_descriptor(schema_id: u32) -> Vec<u8> {
    let mut bytes = vec![10];
    bytes.extend_from_slice(&schema_id.to_le_bytes());
    bytes
}

fn empty_enum_table() -> Vec<u8> {
    table(&[])
}

fn field(offset: u16, name: &str, descriptor: Vec<u8>) -> (u16, &str, Vec<u8>) {
    (offset, name, descriptor)
}

#[test]
fn struct_schema_round_trips_through_avbc_v4_with_identity_and_offsets() {
    let schema = StructSchema::with_identity(
        0,
        DefId::from_display_name("pkg::inner::Pair", 3),
        vec![TypeDescriptor::Int(IntWidth::I64), TypeDescriptor::String],
        vec![
            StructFieldSchema {
                offset: 0,
                name: "left".to_string(),
                ty: TypeDescriptor::Int(IntWidth::I64),
            },
            StructFieldSchema {
                offset: 1,
                name: "right".to_string(),
                ty: TypeDescriptor::String,
            },
        ],
    );
    let mut function = Function::new(Some("pair".to_string()), 0);
    function.struct_schemas = vec![schema.clone()];

    let bytes = serialize(&function).expect("concrete struct schema should serialize");
    assert_eq!(&bytes[4..6], &AVBC_V4.to_le_bytes());
    let loaded = deserialize(&bytes).expect("concrete struct schema should deserialize");
    assert_eq!(
        loaded.struct_schemas.len(),
        1,
        "{:?}",
        loaded.struct_schemas
    );
    let decoded = &loaded.struct_schemas[0];
    assert_eq!(decoded.schema_id, 0);
    assert_eq!(decoded.ctor.package, "pkg");
    assert_eq!(
        decoded.ctor.module.as_ref(),
        ["inner".to_string(), "Pair".to_string()]
    );
    assert_eq!(decoded.ctor.ordinal, 3);
    assert_eq!(
        decoded.type_args.as_ref(),
        [TypeDescriptor::Int(IntWidth::I64), TypeDescriptor::String]
    );
    assert_eq!(decoded.fields[0].offset, 0);
    assert_eq!(decoded.fields[1].offset, 1);
    assert_eq!(decoded, &schema);
}

#[test]
fn generic_struct_instances_keep_constructor_identity_separate_from_arguments() {
    let runtime = Runtime::new();
    let module = runtime
        .compile(
            r#"
struct Holder<T> { value: T }
let integer = Holder { value: 7 }
let text = Holder { value: "ok" }
let _ = integer
let _ = text
0
"#,
            CompileOptions::default(),
        )
        .expect("distinct generic struct instances should compile");
    let function = deserialize(module.avbc()).expect("compiled module should deserialize");
    let instances: Vec<_> = function
        .struct_schemas
        .iter()
        .filter(|schema| schema.type_args.len() == 1)
        .collect();
    assert_eq!(instances.len(), 2, "{:?}", function.struct_schemas);
    assert_eq!(instances[0].ctor, instances[1].ctor);
    assert_ne!(instances[0].type_args, instances[1].type_args);
    assert_ne!(instances[0].schema_id, instances[1].schema_id);
}

#[test]
fn function_fields_have_a_concrete_schema_descriptor() {
    let runtime = Runtime::new();
    let module = runtime
        .compile(
            "struct Holder { callback: fn(int) -> int }\n0",
            CompileOptions::default(),
        )
        .expect("function-typed fields should compile");
    let function = deserialize(module.avbc()).expect("compiled module should deserialize");
    let descriptor = &function.struct_schemas[0].fields[0].ty;
    assert!(
        matches!(descriptor, TypeDescriptor::Function { .. }),
        "function field lowered to {descriptor}"
    );
}

fn point_fields() -> Vec<StructFieldSchema> {
    vec![StructFieldSchema {
        offset: 0,
        name: "x".to_string(),
        ty: TypeDescriptor::Int(IntWidth::I64),
    }]
}

fn producer(ctor: DefId) -> Function {
    let mut function = Function::new(Some("make".to_string()), 0);
    function.num_registers = 2;
    function.struct_schemas = vec![StructSchema::with_identity(
        0,
        ctor,
        Vec::new(),
        point_fields(),
    )];
    function.jit_unsupported_struct = true;
    function.emit_b(OpCode::LoadI, 0, 7, 1);
    function.emit_struct(OpCode::StructNew, 0, 1, 0, 1, 1);
    function.emit_a(OpCode::Return, 1, 0, 0, 1);
    function.finalize_bytecode();
    function
}

fn reader(ctor: DefId) -> Function {
    let mut function = Function::new(Some("read".to_string()), 1);
    function.num_registers = 2;
    function.struct_schemas = vec![StructSchema::with_identity(
        0,
        ctor,
        Vec::new(),
        point_fields(),
    )];
    function.jit_unsupported_struct = true;
    function.emit_struct(OpCode::StructLoad, 0, 1, 0, 0, 1);
    function.emit_a(OpCode::Return, 1, 0, 0, 1);
    function.finalize_bytecode();
    function
}

fn read_back_with(producer_ctor: DefId, reader_ctor: DefId) -> Result<Value, String> {
    let mut vm = VM::new(Source::new("struct-identity", "")).expect("vm");
    let make = vm
        .alloc_function(producer(producer_ctor))
        .expect("producer allocation");
    let instance = vm.execute(make).expect("producer execution");
    let read = vm
        .alloc_function(reader(reader_ctor))
        .expect("reader allocation");
    vm.call_value(Value::ptr(read.index()), &[instance])
        .map_err(|error| error.to_string())
}

#[test]
fn structurally_identical_structs_with_different_constructors_do_not_merge() {
    let same = read_back_with(
        DefId::from_display_name("pkg::alpha::Point", 0),
        DefId::from_display_name("pkg::alpha::Point", 0),
    )
    .expect("one constructor identity must keep one runtime schema");
    assert_eq!(same, Value::int(7));

    let error = read_back_with(
        DefId::from_display_name("pkg::alpha::Point", 0),
        DefId::from_display_name("pkg::beta::Point", 0),
    )
    .expect_err("two constructor identities must not share a runtime schema");
    assert!(error.contains("different struct type"), "{error}");
}

#[test]
fn reader_rejects_a_duplicate_struct_schema_id() {
    let record = struct_record(0, "pkg", &["a", "Point"], 0, &[], &[field(0, "x", int64())]);
    let other = struct_record(0, "pkg", &["a", "Other"], 1, &[], &[field(0, "x", int64())]);
    let bytes = program(AVBC_V4, &table(&[record, other]), &empty_enum_table());
    let error = deserialize(&bytes).expect_err("duplicate struct schema ids must be rejected");
    assert!(error.to_string().contains("struct schema ids"), "{error}");
}

#[test]
fn reader_rejects_a_duplicate_struct_instance_key() {
    let first = struct_record(0, "pkg", &["a", "Point"], 0, &[], &[field(0, "x", int64())]);
    let second = struct_record(1, "pkg", &["a", "Point"], 0, &[], &[field(0, "x", int64())]);
    let bytes = program(AVBC_V4, &table(&[first, second]), &empty_enum_table());
    let error = deserialize(&bytes).expect_err("duplicate struct instance keys must be rejected");
    assert!(
        error.to_string().contains("struct definition paths"),
        "{error}"
    );
}

#[test]
fn reader_rejects_a_struct_field_offset_that_is_not_its_ordinal() {
    let record = struct_record(
        0,
        "pkg",
        &["a", "Point"],
        0,
        &[],
        &[field(0, "x", int64()), field(0, "y", int64())],
    );
    let bytes = program(AVBC_V4, &table(&[record]), &empty_enum_table());
    let error = deserialize(&bytes).expect_err("a field offset must equal its ordinal");
    assert!(
        error.to_string().contains("unordered field offsets"),
        "{error}"
    );
}

#[test]
fn reader_rejects_a_duplicate_struct_field_name() {
    let record = struct_record(
        0,
        "pkg",
        &["a", "Point"],
        0,
        &[],
        &[field(0, "x", int64()), field(1, "x", int64())],
    );
    let bytes = program(AVBC_V4, &table(&[record]), &empty_enum_table());
    let error = deserialize(&bytes).expect_err("duplicate field names must be rejected");
    assert!(
        error.to_string().contains("duplicate or empty field name"),
        "{error}"
    );
}

#[test]
fn struct_descriptor_id_outside_the_struct_table_is_rejected() {
    let record = struct_record(
        0,
        "pkg",
        &["a", "Point"],
        0,
        &[],
        &[field(0, "next", struct_descriptor(4))],
    );
    let bytes = program(AVBC_V4, &table(&[record]), &empty_enum_table());
    let error = deserialize(&bytes).expect_err("an out of range struct id must be rejected");
    assert!(
        error.to_string().contains("unknown struct schema id 4"),
        "{error}"
    );

    let mut function = Function::new(Some("dangling".to_string()), 0);
    function.struct_schemas = vec![StructSchema::with_identity(
        0,
        DefId::from_display_name("pkg::a::Point", 0),
        Vec::new(),
        vec![StructFieldSchema {
            offset: 0,
            name: "next".to_string(),
            ty: TypeDescriptor::Struct(4),
        }],
    )];
    let error = serialize(&function).expect_err("an out of range struct id must not serialize");
    assert!(
        error.to_string().contains("unknown struct schema id 4"),
        "{error}"
    );
}

#[test]
fn an_enum_id_used_where_a_struct_id_is_expected_is_rejected() {
    let mut function = Function::new(Some("crossed".to_string()), 0);
    function.enum_schemas = vec![
        EnumSchema::with_id(
            0,
            "Zero".to_string(),
            vec![EnumVariantSchema {
                variant_id: 0,
                name: "Unit".to_string(),
                fields: Vec::new().into_boxed_slice(),
            }],
        ),
        EnumSchema::with_id(
            1,
            "One".to_string(),
            vec![EnumVariantSchema {
                variant_id: 0,
                name: "Unit".to_string(),
                fields: Vec::new().into_boxed_slice(),
            }],
        ),
    ];
    function.struct_schemas = vec![StructSchema::with_identity(
        0,
        DefId::from_display_name("pkg::a::Point", 0),
        Vec::new(),
        vec![StructFieldSchema {
            offset: 0,
            name: "crossed".to_string(),
            ty: TypeDescriptor::Struct(1),
        }],
    )];
    let error =
        serialize(&function).expect_err("an enum id is not addressable in the struct namespace");
    assert!(
        error.to_string().contains("unknown struct schema id 1"),
        "{error}"
    );
}

#[test]
fn avbc_v3_struct_records_read_as_zero_argument_legacy_shapes() {
    let mut record = string_u16("legacy::Point");
    record.extend_from_slice(&1u16.to_le_bytes());
    record.extend_from_slice(&0u16.to_le_bytes());
    record.extend_from_slice(&string_u16("x"));
    record.extend_from_slice(&int64());

    let mut legacy_table = 1u16.to_le_bytes().to_vec();
    legacy_table.extend_from_slice(&0u16.to_le_bytes());
    legacy_table.extend_from_slice(&record);

    let bytes = program(AVBC_V3, &legacy_table, &[]);
    let loaded = deserialize(&bytes).expect("a version 3 struct table stays readable");
    assert_eq!(loaded.struct_schemas.len(), 1);
    let schema = &loaded.struct_schemas[0];
    assert_eq!(schema.schema_id, 0);
    assert!(schema.type_args.is_empty());
    assert_eq!(schema.ctor.package, "legacy");
    assert_eq!(schema.ctor.module.as_ref(), ["Point".to_string()]);
    assert_eq!(schema.fields[0].offset, 0);
    assert_eq!(schema.fields[0].name, "x");
}

#[test]
fn aasm_round_trips_a_generic_struct_instance_schema() {
    let schema = StructSchema::with_identity(
        0,
        DefId::from_display_name("pkg::inner::Holder", 2),
        vec![TypeDescriptor::Int(IntWidth::I64)],
        vec![StructFieldSchema {
            offset: 0,
            name: "value".to_string(),
            ty: TypeDescriptor::Struct(0),
        }],
    );
    let mut function = Function::new(Some("holder".to_string()), 0);
    function.struct_schemas = vec![schema.clone()];

    let assembly = disassemble(&function);
    let assembled = assemble(&assembly).expect("struct instance assembly should round-trip");
    assert_eq!(assembled[0].struct_schemas, vec![schema]);
}

fn producer_of(schema: StructSchema) -> Function {
    let mut function = Function::new(Some("make".to_string()), 0);
    function.num_registers = 2;
    function.struct_schemas = vec![schema];
    function.jit_unsupported_struct = true;
    function.emit_b(OpCode::LoadI, 0, 7, 1);
    function.emit_struct(OpCode::StructNew, 0, 1, 0, 1, 1);
    function.emit_a(OpCode::Return, 1, 0, 0, 1);
    function.finalize_bytecode();
    function
}

fn reader_of(schema: StructSchema) -> Function {
    let mut function = Function::new(Some("read".to_string()), 1);
    function.num_registers = 2;
    function.struct_schemas = vec![schema];
    function.jit_unsupported_struct = true;
    function.emit_struct(OpCode::StructLoad, 0, 1, 0, 0, 1);
    function.emit_a(OpCode::Return, 1, 0, 0, 1);
    function.finalize_bytecode();
    function
}

fn read_back_schema(produced: StructSchema, read: StructSchema) -> Result<Value, String> {
    let mut vm = VM::new(Source::new("struct-identity", "")).expect("vm");
    let make = vm
        .alloc_function(producer_of(produced))
        .expect("producer allocation");
    let instance = vm.execute(make).expect("producer execution");
    let read = vm
        .alloc_function(reader_of(read))
        .expect("reader allocation");
    vm.call_value(Value::ptr(read.index()), &[instance])
        .map_err(|error| error.to_string())
}

fn holder(ordinal: u32, type_args: Vec<TypeDescriptor>, field: &str) -> StructSchema {
    StructSchema::with_identity(
        0,
        DefId::from_display_name("pkg::alpha::Holder", ordinal),
        type_args,
        vec![StructFieldSchema {
            offset: 0,
            name: field.to_string(),
            ty: TypeDescriptor::Int(IntWidth::I64),
        }],
    )
}

#[test]
fn a_runtime_schema_id_is_shared_exactly_when_the_compared_parts_agree() {
    let reference = || holder(0, Vec::new(), "x");
    assert_eq!(
        read_back_schema(reference(), reference()),
        Ok(Value::int(7)),
        "two schemas that agree on every compared part are one schema"
    );
    for (part, other) in [
        ("the constructor", holder(1, Vec::new(), "x")),
        (
            "a type argument",
            holder(0, vec![TypeDescriptor::Int(IntWidth::I64)], "x"),
        ),
        ("a field name", holder(0, Vec::new(), "y")),
    ] {
        let error = read_back_schema(reference(), other)
            .expect_err(&format!("{part} must separate two runtime schemas"));
        assert!(
            error.contains("different struct type"),
            "two schemas that disagree on {part} must not be given one runtime id: {error}"
        );
    }
}
