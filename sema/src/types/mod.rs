mod infer_type;
mod resolved_type;
mod type_table;
mod type_var;

pub use infer_type::InferType;
pub use resolved_type::ResolvedType;
pub use type_table::{
    BoundSelection, EnumDef, EnumVariantDef, EnumVariantFieldsDef, FromSelection, StructDef,
    StructField, StructMethod, TraitDef, TraitImplDef, TraitMethod, TypeTable,
};
pub(crate) use type_table::{headers_unify, nominal_name};
pub use type_var::{TypeVarGen, TypeVarId};
