use crate::types::{InferType, TraitDef, TraitImplDef, TraitMethod, TypeTable};

pub const DISPLAY_TRAIT: &str = "Display";
pub const DISPLAY_METHOD: &str = "to_display";

pub const FROM_TRAIT: &str = "From";
pub const FROM_METHOD: &str = "from";
pub const FROM_SOURCE_PARAM: &str = "Source";

pub const STRING_TO_ERROR_SYMBOL: &str = "__aelys_from::string_to_error";

pub fn register(table: &mut TypeTable) {
    table.register_trait(TraitDef {
        owner: aelys_syntax::ModuleId::new("<prelude>"),
        name: DISPLAY_TRAIT.to_string(),
        type_params: Vec::new(),
        super_bounds: Vec::new(),
        methods: vec![TraitMethod {
            name: DISPLAY_METHOD.to_string(),
            symbol: String::new(),
            params: vec![InferType::Param("Self".to_string())],
            return_type: InferType::String,
            has_self: true,
            mutable_self: false,
            own_type_params: Vec::new(),
            has_body: false,
            is_default: false,
        }],
        associated_types: Vec::new(),
        associated_consts: Vec::new(),
    });
    table.register_trait(TraitDef {
        owner: aelys_syntax::ModuleId::new("<prelude>"),
        name: FROM_TRAIT.to_string(),
        type_params: vec![FROM_SOURCE_PARAM.to_string()],
        super_bounds: Vec::new(),
        methods: vec![TraitMethod {
            name: FROM_METHOD.to_string(),
            symbol: String::new(),
            params: vec![InferType::Param(FROM_SOURCE_PARAM.to_string())],
            return_type: InferType::Param("Self".to_string()),
            has_self: false,
            mutable_self: false,
            own_type_params: Vec::new(),
            has_body: false,
            is_default: false,
        }],
        associated_types: Vec::new(),
        associated_consts: Vec::new(),
    });
    table.register_trait_impl_with_args(
        FROM_TRAIT.to_string(),
        InferType::Error.to_string(),
        &[InferType::String],
    );
    table.register_trait_impl_def(TraitImplDef {
        opens: false,
        trait_name: FROM_TRAIT.to_string(),
        trait_args: vec![InferType::String],
        self_type: InferType::Error,
        methods: vec![TraitMethod {
            name: FROM_METHOD.to_string(),
            symbol: STRING_TO_ERROR_SYMBOL.to_string(),
            params: vec![InferType::String],
            return_type: InferType::Error,
            has_self: false,
            mutable_self: false,
            own_type_params: Vec::new(),
            has_body: false,
            is_default: false,
        }],
        associated_types: Vec::new(),
        associated_consts: Vec::new(),
    });
}

pub fn provides(trait_name: &str, ty: &InferType, trait_args: &[InferType]) -> bool {
    if trait_name == DISPLAY_TRAIT && trait_args.is_empty() && is_display_scalar(ty) {
        return true;
    }
    trait_name == FROM_TRAIT && matches!(trait_args, [source] if source == ty)
}

// the identity header is reserved, so a user impl can never shadow the compiler rule
pub fn reserves_header(trait_name: &str, ty: &InferType, trait_args: &[InferType]) -> bool {
    trait_name == FROM_TRAIT
        && matches!(trait_args, [source] if crate::types::headers_unify(source, ty))
}

fn is_display_scalar(ty: &InferType) -> bool {
    matches!(
        ty,
        InferType::I8
            | InferType::I16
            | InferType::I32
            | InferType::I64
            | InferType::U8
            | InferType::U16
            | InferType::U32
            | InferType::U64
            | InferType::F32
            | InferType::F64
            | InferType::Bool
            | InferType::String
    )
}
