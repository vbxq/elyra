use aelys::{
    CompileOptions, ExecutionOutcome, IsolateConfig, JitMode, RunOptions, Runtime,
    StructuredCloneError, call_function, new_vm, run_with_vm,
};
use aelys_bytecode::asm::{assemble, deserialize, disassemble, serialize};
use aelys_bytecode::{
    EnumFieldSchema, EnumSchema, EnumVariantSchema, FloatWidth, Function, IntWidth, OpCode,
    StructFieldSchema, StructSchema, TypeDescriptor,
};
use aelys_common::RuntimeErrorKind;
use aelys_runtime::{VM, Value};
use aelys_syntax::Source;

#[test]
fn host_call_executes_struct_field_read_and_mutation() {
    let mut vm = new_vm().expect("vm");
    run_with_vm(
        &mut vm,
        r#"
struct Point { x: int, y: int }
fn read_point() -> int {
    let p = Point { x: 7, y: 4 }
    p.x + p.y
}
fn mutate_point() -> int {
    let mut p = Point { x: 7, y: 4 }
    p.x = 30
    p.x
}
"#,
        "structs",
    )
    .expect("struct definitions should compile");

    assert_eq!(
        call_function(&mut vm, "read_point", &[]).expect("read_point"),
        Value::int(11)
    );
    assert_eq!(
        call_function(&mut vm, "mutate_point", &[]).expect("mutate_point"),
        Value::int(30)
    );
}

#[test]
fn host_call_executes_impl_method_and_associated_function() {
    let mut vm = new_vm().expect("vm");
    run_with_vm(
        &mut vm,
        r#"
struct Point { x: int, y: int }
impl Point {
    fn origin() -> Point { Point { x: 0, y: 0 } }
    fn translate(mut self, dx: int) -> int {
        self.x = self.x + dx
        self.x
    }
}

fn use_methods() -> int {
    let mut p = Point::origin()
    p.translate(5)
}
"#,
        "struct-methods",
    )
    .expect("struct methods should compile");

    assert_eq!(
        call_function(&mut vm, "use_methods", &[]).expect("use_methods"),
        Value::int(5)
    );
}

#[test]
fn host_call_executes_a_trait_associated_function() {
    let mut vm = new_vm().expect("vm");
    run_with_vm(
        &mut vm,
        r#"
trait Factory {
    fn origin() -> Point;
}
struct Point { x: int }
impl Factory for Point {
    fn origin() -> Point { Point { x: 12 } }
}
fn use_factory() -> int {
    Point::origin().x
}
"#,
        "trait-associated-function",
    )
    .expect("a trait associated function should resolve statically");
    assert_eq!(
        call_function(&mut vm, "use_factory", &[]).expect("use_factory"),
        Value::int(12)
    );
}

#[test]
fn host_call_executes_trait_method_directly() {
    let mut vm = new_vm().expect("vm");
    run_with_vm(
        &mut vm,
        r#"
trait Scorable {
    fn score(self) -> int;
}
struct Point { x: int }
impl Scorable for Point {
    fn score(self) -> int { self.x + 1 }
}
fn use_trait() -> int {
    let point = Point { x: 6 }
    point.score()
}
"#,
        "trait-methods",
    )
    .expect("trait method should compile");
    assert_eq!(
        call_function(&mut vm, "use_trait", &[]).expect("use_trait"),
        Value::int(7)
    );
}

#[test]
fn host_call_executes_a_trait_default_method() {
    let mut vm = new_vm().expect("vm");
    run_with_vm(
        &mut vm,
        r#"
trait Scorable {
    fn score(self) -> int { self.x + 2 }
}
struct Point { x: int }
impl Scorable for Point {}
fn use_default() -> int {
    let point = Point { x: 5 }
    point.score()
}
"#,
        "trait-default-methods",
    )
    .expect("a default trait method should be copied into the impl");
    assert_eq!(
        call_function(&mut vm, "use_default", &[]).expect("use_default"),
        Value::int(7)
    );
}

#[test]
fn trait_method_resolves_to_a_direct_impl_call() {
    let mut vm = new_vm().expect("vm");
    run_with_vm(
        &mut vm,
        r#"
trait Printable {
    fn value(self) -> int;
}
struct Point { x: int }
impl Printable for Point {
    fn value(self) -> int { self.x }
}
fn read() -> int {
    let point = Point { x: 7 }
    point.value()
}
"#,
        "trait-method",
    )
    .expect("a unique trait method should resolve statically");
    assert_eq!(
        call_function(&mut vm, "read", &[]).expect("read"),
        Value::int(7)
    );
}

#[test]
fn qualified_trait_method_selects_the_named_impl() {
    let mut vm = new_vm().expect("vm");
    run_with_vm(
        &mut vm,
        r#"
trait Printable {
    fn value(self) -> int;
}
struct Point { x: int }
impl Printable for Point {
    fn value(self) -> int { self.x + 2 }
}
fn read() -> int {
    let point = Point { x: 7 }
    Printable::value(point)
}
"#,
        "qualified-trait-method",
    )
    .expect("a qualified trait method should resolve statically");
    assert_eq!(
        call_function(&mut vm, "read", &[]).expect("read"),
        Value::int(9)
    );
}

#[test]
fn trait_impl_missing_method_has_named_diagnostic() {
    let mut vm = new_vm().expect("vm");
    let error = run_with_vm(
        &mut vm,
        r#"
trait Scorable {
    fn score(self) -> int;
}
struct Point { x: int }
impl Scorable for Point {}
fn use_trait() -> int { 0 }
"#,
        "trait-missing-method",
    )
    .expect_err("an impl must provide every required trait method");
    let message = error.to_string();
    assert!(message.contains("E0333"), "{message}");
    assert!(message.contains("missing method 'score'"), "{message}");
}

#[test]
fn struct_fields_win_over_same_named_methods() {
    let mut vm = new_vm().expect("vm");
    run_with_vm(
        &mut vm,
        r#"
struct Point { x: int }
impl Point {
    fn x(self) -> int { 99 }
}

fn read_field() -> int {
    let point = Point { x: 7 }
    point.x
}
"#,
        "struct-field-precedence",
    )
    .expect("field and method names must remain unambiguous");
    assert_eq!(
        call_function(&mut vm, "read_field", &[]).expect("read_field"),
        Value::int(7)
    );
}

#[test]
fn immutable_mut_self_method_is_rejected() {
    let mut vm = new_vm().expect("vm");
    let error = run_with_vm(
        &mut vm,
        r#"
struct Point { x: int }
impl Point {
    fn shift(mut self) -> int {
        self.x = self.x + 1
        self.x
    }
}
fn bad() -> int {
    let point = Point { x: 7 }
    point.shift()
}
"#,
        "immutable-mut-self",
    )
    .expect_err("mut self methods require a mutable receiver");
    assert!(
        error
            .to_string()
            .contains("cannot call mutable struct method 'shift' on an immutable receiver"),
        "{error}"
    );
}

#[test]
fn nested_irrefutable_struct_patterns_are_exhaustive() {
    let mut vm = new_vm().expect("vm");
    run_with_vm(
        &mut vm,
        r#"
struct Inner { value: int }
struct Outer { inner: Inner }
fn read_nested() -> int {
    let outer = Outer { inner: Inner { value: 7 } }
    match outer {
        Outer { inner: Inner { value } } => value,
    }
}
"#,
        "nested-struct-patterns",
    )
    .expect("all nested fields make the pattern irrefutable");
    assert_eq!(
        call_function(&mut vm, "read_nested", &[]).expect("read_nested"),
        Value::int(7)
    );
}

#[test]
fn struct_match_binds_fields_through_host_call() {
    let mut vm = new_vm().expect("vm");
    run_with_vm(
        &mut vm,
        r#"
struct Point { x: int, y: int }
fn classify() -> int {
    let p = Point { x: 7, y: 4 }
    match p {
        Point { x, y } => x + y,
    }
}
"#,
        "struct-patterns",
    )
    .expect("struct patterns should compile");

    assert_eq!(
        call_function(&mut vm, "classify", &[]).expect("classify"),
        Value::int(11)
    );
}

#[test]
fn non_exhaustive_struct_match_has_e324() {
    let mut vm = new_vm().expect("vm");
    let error = run_with_vm(
        &mut vm,
        r#"
struct Point { x: int, y: int }
fn classify() -> int {
    let p = Point { x: 7, y: 4 }
    match p {
        Point { x: 0, .. } => 1,
    }
}
"#,
        "struct-patterns",
    )
    .expect_err("a refutable struct-only match must be rejected");
    let message = error.to_string();
    assert!(message.contains("E0324"), "{message}");
    assert!(
        message.contains(
            "non-exhaustive struct match for Point; add '_' or an irrefutable field pattern"
        ),
        "{message}"
    );
}

#[test]
fn generic_structs_infer_and_read_concrete_fields() {
    let mut vm = new_vm().expect("vm");
    run_with_vm(
        &mut vm,
        r#"
struct Box<T> { value: T }
fn read_box() -> int {
    let boxed = Box { value: 7 }
    boxed.value
}

"#,
        "generic-struct",
    )
    .expect("generic structs should infer their concrete field type");
    assert_eq!(
        call_function(&mut vm, "read_box", &[]).expect("read_box"),
        Value::int(7)
    );
}

#[test]
fn generic_trait_impl_method_executes_for_a_concrete_instance() {
    let mut vm = new_vm().expect("vm");
    run_with_vm(
        &mut vm,
        r#"
trait Scorable {
    fn score(self) -> int;
}
struct Box<T> { value: T }
impl<T> Scorable for Box<T> {
    fn score(self) -> int { 7 }
}
fn read_box() -> int {
    let boxed: Box<int> = Box { value: 3 }
    boxed.score()
}
"#,
        "generic-trait-impl",
    )
    .expect("generic trait implementation should compile");
    assert_eq!(
        call_function(&mut vm, "read_box", &[]).expect("read_box"),
        Value::int(7)
    );
}

#[test]
fn generic_struct_schema_has_no_open_any_field() {
    let runtime = Runtime::new();
    let module = runtime
        .compile(
            r#"
struct Box<T> { value: T }
fn read_box() -> int {
    let boxed = Box { value: 7 }
    boxed.value
}
"#,
            CompileOptions::default(),
        )
        .expect("generic struct should compile");
    let function = deserialize(module.avbc()).expect("compiled module should deserialize");
    let schemas = &function.struct_schemas;
    assert!(
        !schemas.is_empty(),
        "instantiating Box<int> must emit a struct schema"
    );
    let instance = schemas
        .iter()
        .find(|schema| decode_instance_name(&schema.display_name()).contains("Box"))
        .unwrap_or_else(|| panic!("no Box instance schema was emitted: {schemas:?}"));
    assert_eq!(
        instance.type_args.as_ref(),
        [TypeDescriptor::Int(IntWidth::I64)],
        "Box schema must be monomorphized to int: {instance:?}"
    );
    assert_eq!(instance.fields.len(), 1, "{instance:?}");
    assert_eq!(instance.fields[0].name, "value", "{instance:?}");
    assert_eq!(
        instance.fields[0].ty,
        TypeDescriptor::Int(IntWidth::I64),
        "{instance:?}"
    );
}

// monomorphized instance schema names are the hex-encoded canonical type application
fn decode_instance_name(name: &str) -> String {
    let Some((_, encoded)) = name.split_once("__aelys_instance_") else {
        return name.to_string();
    };
    let bytes: Vec<u8> = encoded
        .as_bytes()
        .chunks(2)
        .filter_map(|pair| std::str::from_utf8(pair).ok())
        .filter_map(|pair| u8::from_str_radix(pair, 16).ok())
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

#[test]
fn generic_enum_schema_carries_concrete_type_arguments() {
    let runtime = Runtime::new();
    let module = runtime
        .compile(
            r#"
enum Maybe<T> { Some(T), None }
fn make() -> Maybe<int> { Maybe::<int>::Some(7) }
make()
"#,
            CompileOptions::default(),
        )
        .expect("generic enum should compile");
    let function = deserialize(module.avbc()).expect("compiled module should deserialize");
    assert!(
        function.enum_schemas.iter().any(|schema| {
            schema
                .type_args
                .iter()
                .any(|ty| matches!(ty, TypeDescriptor::Int(IntWidth::I64)))
        }),
        "{:?}",
        function.enum_schemas
    );
}

#[test]
fn generic_enum_instances_keep_constructor_identity_separate_from_arguments() {
    let runtime = Runtime::new();
    let module = runtime
        .compile(
            r#"
enum Maybe<T> { Some(T), None }
let integer = Maybe::<int>::Some(7)
let text = Maybe::<string>::Some("ok")
let _ = integer
let _ = text
0
"#,
            CompileOptions::default(),
        )
        .expect("distinct generic enum instances should compile");
    let function = deserialize(module.avbc()).expect("compiled module should deserialize");
    let instances: Vec<_> = function
        .enum_schemas
        .iter()
        .filter(|schema| schema.arity == 1)
        .collect();
    assert_eq!(instances.len(), 2, "{instances:?}");
    assert_eq!(instances[0].def_id, instances[1].def_id);
    assert_ne!(instances[0].type_args, instances[1].type_args);
}

#[test]
fn immutable_struct_field_write_is_rejected() {
    let mut vm = new_vm().expect("vm");
    let error = run_with_vm(
        &mut vm,
        r#"
struct Point { x: int }
fn bad() -> int {
    let p = Point { x: 1 }
    p.x = 2
    p.x
}
"#,
        "immutable-struct",
    )
    .expect_err("field writes require a mutable root");
    assert!(
        error.to_string().contains("cannot assign to immutable"),
        "{error}"
    );
}

#[test]
fn schema_table_roundtrips_the_descriptor_grammar() {
    let fields = vec![
        StructFieldSchema {
            offset: 0,
            name: "unit".to_string(),
            ty: TypeDescriptor::Unit,
        },
        StructFieldSchema {
            offset: 0,
            name: "bool".to_string(),
            ty: TypeDescriptor::Bool,
        },
        StructFieldSchema {
            offset: 0,
            name: "i8".to_string(),
            ty: TypeDescriptor::Int(IntWidth::I8),
        },
        StructFieldSchema {
            offset: 0,
            name: "f32".to_string(),
            ty: TypeDescriptor::Float(FloatWidth::F32),
        },
        StructFieldSchema {
            offset: 0,
            name: "string".to_string(),
            ty: TypeDescriptor::String,
        },
        StructFieldSchema {
            offset: 0,
            name: "option".to_string(),
            ty: TypeDescriptor::Option(Box::new(TypeDescriptor::Int(IntWidth::I64))),
        },
        StructFieldSchema {
            offset: 0,
            name: "result".to_string(),
            ty: TypeDescriptor::Result(
                Box::new(TypeDescriptor::Bool),
                Box::new(TypeDescriptor::Error),
            ),
        },
        StructFieldSchema {
            offset: 0,
            name: "array".to_string(),
            ty: TypeDescriptor::Array(Box::new(TypeDescriptor::Any)),
        },
        StructFieldSchema {
            offset: 0,
            name: "fixed".to_string(),
            ty: TypeDescriptor::FixedArray(Box::new(TypeDescriptor::String), 3),
        },
        StructFieldSchema {
            offset: 0,
            name: "vec".to_string(),
            ty: TypeDescriptor::Vec(Box::new(TypeDescriptor::Float(FloatWidth::F64))),
        },
        StructFieldSchema {
            offset: 0,
            name: "nested".to_string(),
            ty: TypeDescriptor::Struct(0),
        },
        StructFieldSchema {
            offset: 0,
            name: "any".to_string(),
            ty: TypeDescriptor::Any,
        },
        StructFieldSchema {
            offset: 0,
            name: "error".to_string(),
            ty: TypeDescriptor::Error,
        },
        StructFieldSchema {
            offset: 0,
            name: "never".to_string(),
            ty: TypeDescriptor::Never,
        },
    ];
    let mut function = Function::new(Some("schema".to_string()), 0);
    function.struct_schemas = vec![StructSchema::new("Record".to_string(), fields)];
    function.jit_unsupported_struct = true;

    let bytes = serialize(&function).expect("schema function should serialize");
    assert_eq!(&bytes[4..6], &4u16.to_le_bytes());
    let loaded = deserialize(&bytes).expect("schema function should deserialize");
    assert_eq!(loaded.struct_schemas, function.struct_schemas);
    assert!(loaded.jit_unsupported_struct);
}

#[test]
fn enum_schema_table_roundtrips_avbc_v4() {
    let mut function = Function::new(Some("enum_schema".to_string()), 0);
    function.enum_schemas = vec![EnumSchema::new(
        "Shape".to_string(),
        vec![
            EnumVariantSchema {
                variant_id: 0,
                name: "Unit".to_string(),
                fields: Vec::new().into_boxed_slice(),
            },
            EnumVariantSchema {
                variant_id: 1,
                name: "Point".to_string(),
                fields: vec![
                    EnumFieldSchema {
                        offset: 0,
                        name: Some("x".to_string()),
                        ty: TypeDescriptor::Int(IntWidth::I64),
                    },
                    EnumFieldSchema {
                        offset: 1,
                        name: Some("label".to_string()),
                        ty: TypeDescriptor::String,
                    },
                ]
                .into_boxed_slice(),
            },
        ],
    )];
    function.jit_unsupported_struct = true;

    let bytes = serialize(&function).expect("enum schema function should serialize");
    assert_eq!(&bytes[4..6], &4u16.to_le_bytes());
    let loaded = deserialize(&bytes).expect("enum schema function should deserialize");
    assert_eq!(loaded.enum_schemas, function.enum_schemas);
    assert!(loaded.jit_unsupported_struct);
}

#[test]
fn avbc_v3_rejects_enum_opcodes_without_a_stage2_table() {
    let mut bytes = b"VBXQ".to_vec();
    bytes.extend_from_slice(&3u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u8.to_le_bytes());
    bytes.extend_from_slice(&0u8.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&4u32.to_le_bytes());
    bytes.extend_from_slice(&(u32::from(u8::from(OpCode::EnumTest)) << 24).to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&(u32::from(u8::from(OpCode::Return)) << 24).to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    let error = deserialize(&bytes).expect_err("version 3 cannot carry enum opcodes");
    assert!(
        error
            .to_string()
            .contains("version 3 cannot contain enum opcode"),
        "{error}"
    );
}

#[test]
fn avbc_rejects_enum_opcodes_without_a_schema_table() {
    let mut function = Function::new(Some("enum_opcode_without_schema".to_string()), 0);
    function.num_registers = 1;
    function.emit_enum(OpCode::EnumTest, 0, 0, 0, 0, 0, 1);
    function.emit_a(OpCode::Return, 0, 0, 0, 1);
    function.finalize_bytecode();

    let error = serialize(&function).expect_err("an enum opcode needs a schema table");
    assert!(
        error
            .to_string()
            .contains("enum opcode requires an enum schema table"),
        "{error}"
    );
}

#[test]
fn enum_schema_serialization_rejects_duplicate_variant_names() {
    let mut function = Function::new(Some("invalid_enum_schema".to_string()), 0);
    function.enum_schemas = vec![EnumSchema::new(
        "test::Shape".to_string(),
        vec![
            EnumVariantSchema {
                variant_id: 0,
                name: "Same".to_string(),
                fields: Vec::new().into_boxed_slice(),
            },
            EnumVariantSchema {
                variant_id: 1,
                name: "Same".to_string(),
                fields: Vec::new().into_boxed_slice(),
            },
        ],
    )];

    let error = serialize(&function).expect_err("duplicate enum variant");
    assert!(
        error
            .to_string()
            .contains("duplicate or empty variant name"),
        "{error}"
    );
}

#[test]
fn enum_schema_serialization_rejects_mismatched_type_arity() {
    let mut function = Function::new(Some("invalid_enum_arity".to_string()), 0);
    let mut schema = EnumSchema::new(
        "test::Maybe".to_string(),
        vec![EnumVariantSchema {
            variant_id: 0,
            name: "None".to_string(),
            fields: Vec::new().into_boxed_slice(),
        }],
    );
    schema.type_args = vec![TypeDescriptor::Int(IntWidth::I64)].into_boxed_slice();
    function.enum_schemas = vec![schema];

    let error = serialize(&function).expect_err("enum type arity must match its type arguments");
    assert!(
        error.to_string().contains("type parameter arity"),
        "{error}"
    );
}

#[test]
fn malformed_enum_value_is_rejected_by_enum_test() {
    let mut vm = new_vm().expect("vm");
    run_with_vm(
        &mut vm,
        r#"
        enum Shape { Unit, Point(int) }
        fn matches_unit(shape: Shape) -> bool {
            match shape {
                Shape::Unit => true,
                Shape::Point(_) => false,
            }
        }
        "#,
        "malformed-enum",
    )
    .expect("enum function should compile");

    let reference = vm
        .alloc_enum(0, 0, vec![Value::int(7)])
        .expect("malformed enum should be allocatable for the boundary test");
    let error = call_function(&mut vm, "matches_unit", &[Value::ptr(reference.index())])
        .expect_err("an enum with the wrong slot count must not match");
    assert!(
        error
            .to_string()
            .contains("enum object slot count does not match schema"),
        "{error}"
    );
}

#[test]
fn enum_load_reports_named_invalid_enum_field_boundary() {
    let mut vm = new_vm().expect("vm");
    let mut function = Function::new(Some("bad_enum_load".to_string()), 1);
    function.num_registers = 2;
    function.enum_schemas = vec![EnumSchema::new(
        "test::Shape".to_string(),
        vec![
            EnumVariantSchema {
                variant_id: 0,
                name: "Zero".to_string(),
                fields: vec![EnumFieldSchema {
                    offset: 0,
                    name: None,
                    ty: TypeDescriptor::Int(IntWidth::I64),
                }]
                .into_boxed_slice(),
            },
            EnumVariantSchema {
                variant_id: 1,
                name: "One".to_string(),
                fields: vec![EnumFieldSchema {
                    offset: 0,
                    name: None,
                    ty: TypeDescriptor::Int(IntWidth::I64),
                }]
                .into_boxed_slice(),
            },
        ],
    )];
    function.jit_unsupported_struct = true;
    function.emit_enum(OpCode::EnumLoad, 0, 1, 0, 0, 0, 1);
    function.emit_a(OpCode::Return, 0, 0, 0, 1);
    function.finalize_bytecode();

    let function_ref = vm.alloc_function(function).expect("function");
    let enum_ref = vm.alloc_enum(0, 1, vec![Value::int(7)]).expect("enum");
    let error = vm
        .call_value(
            Value::ptr(function_ref.index()),
            &[Value::ptr(enum_ref.index())],
        )
        .expect_err("loading a different enum variant must fail at the runtime boundary");
    assert!(
        matches!(error.kind, RuntimeErrorKind::InvalidEnumField { .. }),
        "{error:?}"
    );
}

#[test]
fn nested_enum_field_type_is_checked_before_construction() {
    let mut vm = new_vm().expect("vm");
    run_with_vm(
        &mut vm,
        r#"
        enum Inner { Value(int) }
        enum Outer { Wrapped(Inner) }
        fn wrap(inner: Inner) -> Outer {
            Outer::Wrapped(inner)
        }
        "#,
        "malformed-nested-enum",
    )
    .expect("nested enum function should compile");

    let malformed_inner = vm
        .alloc_enum(0, 0, vec![Value::bool(true)])
        .expect("malformed nested enum should be allocatable for the boundary test");
    let error = call_function(&mut vm, "wrap", &[Value::ptr(malformed_inner.index())])
        .expect_err("an enum field with the wrong value type must be rejected");
    assert!(
        error
            .to_string()
            .contains("enum value does not satisfy schema field type"),
        "{error}"
    );
}

#[test]
fn nested_enum_descriptor_selects_avbc_v4() {
    let mut function = Function::new(Some("nested_enum_schema".to_string()), 0);
    function.enum_schemas = vec![EnumSchema::new(
        "test::Shape".to_string(),
        vec![EnumVariantSchema {
            variant_id: 0,
            name: "Unit".to_string(),
            fields: Vec::new().into_boxed_slice(),
        }],
    )];
    function.struct_schemas = vec![StructSchema::new(
        "Holder".to_string(),
        vec![StructFieldSchema {
            offset: 0,
            name: "value".to_string(),
            ty: TypeDescriptor::Option(Box::new(TypeDescriptor::Enum(0))),
        }],
    )];

    let bytes = serialize(&function).expect("nested enum schema should serialize");
    assert_eq!(&bytes[4..6], &4u16.to_le_bytes());
    let loaded = deserialize(&bytes).expect("nested enum schema should deserialize");
    assert_eq!(loaded.struct_schemas, function.struct_schemas);
}

#[test]
fn enum_descriptor_rejects_unknown_schema_id() {
    let mut function = Function::new(Some("unknown_enum_descriptor".to_string()), 0);
    function.struct_schemas = vec![StructSchema::new(
        "Holder".to_string(),
        vec![StructFieldSchema {
            offset: 0,
            name: "value".to_string(),
            ty: TypeDescriptor::Enum(4),
        }],
    )];

    let error = serialize(&function).expect_err("unknown enum schema id");
    assert!(error.to_string().contains("unknown schema id 4"), "{error}");
}

#[test]
fn schema_function_rejects_a_nonzero_reserved_byte() {
    let mut function = Function::new(None, 0);
    function.jit_unsupported_struct = false;
    let mut bytes = serialize(&function).expect("function should serialize");
    bytes[29] = 1;
    let error = deserialize(&bytes).expect_err("reserved schema metadata must be zero");
    assert!(
        error.to_string().contains("Invalid constant type"),
        "{error}"
    );
}

#[test]
fn aasm_roundtrips_struct_schema_metadata() {
    let mut function = Function::new(Some("schema_asm".to_string()), 0);
    function.struct_schemas = vec![StructSchema::new(
        "Point".to_string(),
        vec![StructFieldSchema {
            offset: 0,
            name: "x".to_string(),
            ty: TypeDescriptor::Int(IntWidth::I64),
        }],
    )];
    function.jit_unsupported_struct = true;

    let assembly = disassemble(&function);
    let assembled = assemble(&assembly).expect("struct schema assembly should round-trip");
    assert_eq!(assembled[0].struct_schemas, function.struct_schemas);
    assert!(assembled[0].jit_unsupported_struct);
}

#[test]
fn aasm_roundtrips_data_enum_schema_and_executes() {
    let mut function = Function::new(Some("enum_schema_asm".to_string()), 0);
    function.num_registers = 3;
    function.enum_schemas = vec![EnumSchema::new(
        "test::Shape".to_string(),
        vec![EnumVariantSchema {
            variant_id: 0,
            name: "Pair".to_string(),
            fields: vec![EnumFieldSchema {
                offset: 0,
                name: None,
                ty: TypeDescriptor::Int(IntWidth::I64),
            }]
            .into_boxed_slice(),
        }],
    )];
    function.jit_unsupported_struct = true;
    function.emit_b(OpCode::LoadI, 0, 7, 1);
    function.emit_b(OpCode::LoadI, 2, 99, 1);
    function.emit_enum(OpCode::EnumNew, 0, 1, 0, 0, 1, 1);
    function.emit_enum(OpCode::EnumLoad, 0, 2, 1, 0, 0, 1);
    function.emit_a(OpCode::Return, 2, 0, 0, 1);
    function.finalize_bytecode();

    let assembly = disassemble(&function);
    let assembled = assemble(&assembly).expect("enum schema assembly should round-trip");
    assert_eq!(assembled[0].enum_schemas, function.enum_schemas);

    let mut vm = new_vm().expect("vm");
    let reference = vm
        .alloc_function(assembled.into_iter().next().expect("assembled function"))
        .expect("function allocation");
    assert_eq!(
        vm.execute(reference).expect("enum execution"),
        Value::int(7)
    );
}

#[test]
fn malformed_struct_field_value_is_rejected_before_allocation() {
    let mut function = Function::new(Some("bad_struct".to_string()), 0);
    function.num_registers = 2;
    function.struct_schemas = vec![StructSchema::new(
        "OnlyString".to_string(),
        vec![StructFieldSchema {
            offset: 0,
            name: "value".to_string(),
            ty: TypeDescriptor::String,
        }],
    )];
    function.jit_unsupported_struct = true;
    function.emit_b(OpCode::LoadI, 0, 7, 1);
    function.emit_struct(OpCode::StructNew, 0, 1, 0, 1, 1);
    function.emit_a(OpCode::Return, 1, 0, 0, 1);
    function.finalize_bytecode();

    let mut vm = VM::new(Source::new("bad-struct", "")).expect("vm");
    let reference = vm.alloc_function(function).expect("function allocation");
    let error = vm
        .execute(reference)
        .expect_err("invalid field values must not reach the heap");
    assert!(
        error
            .to_string()
            .contains("struct value does not satisfy schema field type"),
        "{error}"
    );
}

#[test]
fn verifier_rejects_struct_field_offset_before_dispatch() {
    let mut function = Function::new(Some("bad_struct_offset".to_string()), 0);
    function.num_registers = 1;
    function.struct_schemas = vec![StructSchema::new(
        "Point".to_string(),
        vec![StructFieldSchema {
            offset: 0,
            name: "x".to_string(),
            ty: TypeDescriptor::Int(IntWidth::I64),
        }],
    )];
    function.jit_unsupported_struct = true;
    function.emit_struct(OpCode::StructLoad, 0, 0, 0, 1, 1);
    function.finalize_bytecode();

    let mut vm = VM::new(Source::new("bad-struct-offset", "")).expect("vm");
    let reference = vm.alloc_function(function).expect("function allocation");
    let error = vm
        .execute(reference)
        .expect_err("an invalid field offset must fail verification");
    assert!(
        error
            .to_string()
            .contains("StructLoad field offset is out of bounds"),
        "{error}"
    );
    assert_eq!(vm.execution_stats().instructions, 0);
}

#[test]
fn struct_functions_never_enter_the_jit_cache() {
    let runtime = Runtime::with_jit_mode(JitMode::Baseline);
    let module = runtime
        .compile(
            "struct Point { x: int }\nfn value() -> int { let p = Point { x: 7 }\n p.x }\nvalue()",
            CompileOptions::default(),
        )
        .expect("struct module should compile");
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    assert_eq!(
        isolate.execute(&module, RunOptions::default()).unwrap(),
        ExecutionOutcome::Returned(Value::int(7))
    );
    assert_eq!(runtime.jit_cache_entries(), 0);
}

#[test]
fn structured_clone_rejects_struct_values_explicitly() {
    let runtime = Runtime::with_jit_mode(JitMode::Off);
    let module = runtime
        .compile(
            "struct Point { x: int }\nfn make() -> Point { Point { x: 7 } }\nmake()",
            CompileOptions::default(),
        )
        .expect("struct module should compile");
    let mut isolate = runtime.new_isolate(IsolateConfig::default());
    let ExecutionOutcome::Returned(value) = isolate
        .execute(&module, RunOptions::default())
        .expect("struct value should execute")
    else {
        panic!("struct value should be returned");
    };
    assert!(matches!(
        isolate.structured_clone(value),
        Err(StructuredCloneError::StructValuesUnsupported)
    ));
}

#[test]
fn schema_indices_survive_dropping_an_unused_generic_struct() {
    let mut vm = new_vm().expect("vm");
    run_with_vm(
        &mut vm,
        r#"
struct Bag<T> { v: T }
struct Cat { p: int, q: int, r: int }
struct Dog { a: int, b: int }
fn probe() -> int {
    let c = Cat { p: 1, q: 2, r: 3 }
    let d = Dog { a: 8, b: 9 }
    c.r
}
"#,
        "schema-indices",
    )
    .expect("concrete structs beside an unused generic struct should compile");

    assert_eq!(
        call_function(&mut vm, "probe", &[]).expect("probe"),
        Value::int(3)
    );
}

#[test]
fn string_schema_indices_survive_dropping_an_unused_generic_struct() {
    let mut vm = new_vm().expect("vm");
    run_with_vm(
        &mut vm,
        r#"
struct Bag<T> { v: T }
struct Cat { p: string }
struct Dog { a: int }
fn probe() -> string {
    let c = Cat { p: "cat" }
    let d = Dog { a: 8 }
    c.p
}
"#,
        "schema-indices-string",
    )
    .expect("string fields beside an unused generic struct should compile");

    let value = call_function(&mut vm, "probe", &[]).expect("probe");
    assert_eq!(vm.value_to_string(value), "cat");
}

#[test]
fn generic_struct_field_reads_use_the_instance_schema() {
    let mut vm = new_vm().expect("vm");
    run_with_vm(
        &mut vm,
        r#"
struct Bag<T> { v: T, w: T }
struct Zed { a: int }
fn probe() -> int {
    let z = Zed { a: 1 }
    let b = Bag { v: 7, w: 8 }
    b.w
}
"#,
        "generic-instance-schema",
    )
    .expect("a generic struct instance beside a concrete struct should compile");

    assert_eq!(
        call_function(&mut vm, "probe", &[]).expect("probe"),
        Value::int(8)
    );
}
