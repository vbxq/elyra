pub mod constraint;
pub mod env;
pub mod infer;
pub mod native;
pub mod prelude;
pub mod typed_ast;
pub mod types;
pub mod unify;

pub use constraint::{Constraint, ConstraintReason, TypeError};
pub use env::TypeEnv;
pub use infer::{
    GENERATED_SYMBOL_PREFIX, TypeInference,
    entry::{InferenceInputs, InferenceResult},
    is_mangled_symbol, is_module_scoped_global, module_scoped_global, unscoped_global_name,
};
pub use typed_ast::{
    TypedExpr, TypedExprKind, TypedFmtStringPart, TypedFunction, TypedMatchArm, TypedMatchArmBody,
    TypedParam, TypedPattern, TypedPatternKind, TypedProgram, TypedStmt, TypedStmtKind,
};
pub use types::{
    EnumDef, EnumVariantDef, EnumVariantFieldsDef, InferType, ResolvedType, StructDef, StructField,
    TraitDef, TraitImplDef, TraitMethod, TypeTable, TypeVarGen, TypeVarId,
};
pub use unify::{Substitution, UnifyError};
