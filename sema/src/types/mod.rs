mod infer_type;
mod resolved_type;
mod type_table;
mod type_var;

pub use infer_type::InferType;
pub(crate) use infer_type::{SHADOWED_METHOD_SUFFIX, written_names};
pub use resolved_type::ResolvedType;
pub use type_table::{
    BoundSelection, EnumDef, EnumVariantDef, EnumVariantFieldsDef, FromSelection, NegativeImplDef,
    SpecializationChoice, StructDef, StructField, StructMethod, TraitDef, TraitImplDef,
    TraitMethod, TypeTable,
};
pub(crate) use type_table::{
    headers_unify, instantiate_impl_definition, nominal_name, trait_instantiation_spelling,
};
pub(crate) use type_table::{instantiation_key, positional_spelling};
pub use type_var::{TypeVarGen, TypeVarId};
