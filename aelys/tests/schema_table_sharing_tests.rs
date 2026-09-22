use aelys_bytecode::{
    AelysFunction, DefId, Function, IntWidth, SchemaId, StructFieldSchema, StructSchema,
    TypeDescriptor,
};
use std::sync::Arc;

fn schema(id: u32, name: &str, fields: usize) -> StructSchema {
    StructSchema {
        schema_id: id,
        ctor: DefId::from_display_name(name, id),
        type_args: Box::from([]),
        fields: (0..fields)
            .map(|index| StructFieldSchema {
                offset: u16::try_from(index).expect("test field offset fits"),
                name: format!("f{index}"),
                ty: TypeDescriptor::Int(IntWidth::I64),
            })
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    }
}

fn function_with_schemas(count: usize, fields: usize) -> Function {
    let mut function = Function::new(Some("probe".to_string()), 0);
    function.struct_schemas = (0..count)
        .map(|index| {
            let id = u32::try_from(index).expect("test schema id fits");
            schema(id, &format!("S{index}"), fields)
        })
        .collect();
    function.schema_ids = (0..count)
        .map(|index| SchemaId(u32::try_from(index).expect("test schema id fits")))
        .collect();
    function
}

#[test]
fn a_function_hands_out_the_same_schema_table_to_every_reader() {
    let object = AelysFunction::new(function_with_schemas(4, 3));
    let first = object.schemas();
    let second = object.schemas();
    assert!(
        Arc::ptr_eq(&first, &second),
        "each struct instruction would otherwise copy the whole schema table"
    );
}

#[test]
fn the_shared_table_carries_the_schemas_of_its_function() {
    let object = AelysFunction::new(function_with_schemas(3, 2));
    let schemas = object.schemas();
    assert_eq!(schemas.structs.len(), 3);
    assert_eq!(
        schemas.runtime_ids,
        vec![SchemaId(0), SchemaId(1), SchemaId(2)]
    );
    assert_eq!(schemas.structs[2].fields.len(), 2);
    assert!(schemas.enums.is_empty());
}

#[test]
fn a_function_without_schemas_hands_out_an_empty_table() {
    let object = AelysFunction::new(Function::new(Some("bare".to_string()), 0));
    let schemas = object.schemas();
    assert!(schemas.structs.is_empty());
    assert!(schemas.runtime_ids.is_empty());
}
