use super::InferType;
use crate::constraint::ItemNamespace;
use aelys_syntax::{ModuleId, Span};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct StructField {
    pub name: String,
    pub ty: InferType,
    pub is_pub: bool,
    pub ordinal: u16,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct StructDef {
    pub name: String,
    pub type_params: Vec<String>,
    pub fields: Vec<StructField>,
    pub owner: ModuleId,
    pub is_pub: bool,
}

#[derive(Debug, Clone)]
pub struct StructMethod {
    pub name: String,
    pub symbol: String,
    pub params: Vec<InferType>,
    pub return_type: InferType,
    pub has_self: bool,
    pub mutable_self: bool,
    /// instantiates anew; the ones it inherits from its impl or its trait are
    pub own_type_params: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct TraitMethod {
    pub name: String,
    pub symbol: String,
    pub params: Vec<InferType>,
    pub return_type: InferType,
    pub has_self: bool,
    pub mutable_self: bool,
    pub own_type_params: Vec<String>,
    pub has_body: bool,
    /// written `default fn`, which opens the method to replacement by a more
    pub is_default: bool,
}

#[derive(Debug, Clone)]
pub struct TraitDef {
    pub name: String,
    pub owner: ModuleId,
    pub type_params: Vec<String>,
    /// a supertrait is written with its arguments, and the obligation it lays is
    pub super_bounds: Vec<(String, Vec<InferType>)>,
    pub methods: Vec<TraitMethod>,
    pub associated_types: Vec<String>,
    pub associated_consts: Vec<(String, InferType)>,
}

/// a header's own spelling, with its parameters numbered by first appearance so
pub(crate) fn positional_spelling(self_ty: &InferType) -> String {
    let mut seen: Vec<String> = Vec::new();
    render_positional(self_ty, &mut seen)
}

fn render_positional(ty: &InferType, seen: &mut Vec<String>) -> String {
    match ty {
        InferType::Param(name) => {
            let position = match seen.iter().position(|known| known == name) {
                Some(index) => index,
                None => {
                    seen.push(name.clone());
                    seen.len() - 1
                }
            };
            format!("${position}")
        }
        InferType::Applied { name, args } => {
            let rendered: Vec<String> = args
                .iter()
                .map(|arg| render_positional(arg, seen))
                .collect();
            format!("{name}<{}>", rendered.join(","))
        }
        other => other.source_spelling(),
    }
}

/// a trait at one instantiation, spelled so that two instantiations of one trait
pub(crate) fn instantiation_key(name: &str, args: &[InferType]) -> String {
    // positional numbering restarted per obligation renders two different
    let mut key = name.to_string();
    for arg in args {
        key.push('<');
        key.push_str(&parameter_marked_spelling(arg));
    }
    key
}

fn parameter_marked_spelling(ty: &InferType) -> String {
    match ty {
        InferType::Param(name) => format!("%{name}"),
        InferType::Applied { name, args } => {
            let rendered: Vec<String> = args.iter().map(parameter_marked_spelling).collect();
            format!("{name}<{}>", rendered.join(","))
        }
        InferType::Option(inner) => format!("Option<{}>", parameter_marked_spelling(inner)),
        InferType::Array(inner) => format!("[{}]", parameter_marked_spelling(inner)),
        InferType::FixedArray(inner, length) => {
            format!("[{};{length}]", parameter_marked_spelling(inner))
        }
        InferType::Vec(inner) => format!("Vec<{}>", parameter_marked_spelling(inner)),
        InferType::Result(ok, err) => format!(
            "Result<{},{}>",
            parameter_marked_spelling(ok),
            parameter_marked_spelling(err)
        ),
        InferType::Tuple(elements) => {
            let rendered: Vec<String> = elements.iter().map(parameter_marked_spelling).collect();
            format!("({})", rendered.join(","))
        }
        other => other.source_spelling(),
    }
}

/// the trait arguments an impl reaches for a given receiver: its declared ones,
fn instantiate_args(
    self_type: &InferType,
    args: &[InferType],
    receiver: &InferType,
) -> Vec<InferType> {
    let mut mapping = std::collections::HashMap::new();
    if crate::infer::monomorphize::match_types(self_type, receiver, &mut mapping).is_none() {
        return args.to_vec();
    }
    let mut substitution = crate::unify::Substitution::new();
    for (param, ty) in mapping {
        substitution.bind_param(param, ty);
    }
    args.iter().map(|arg| substitution.apply(arg)).collect()
}

/// a header is one pattern, its target and every trait argument matched under a
fn header_covers(
    pattern: &InferType,
    pattern_args: &[InferType],
    ty: &InferType,
    args: &[InferType],
) -> bool {
    let mut mapping = std::collections::HashMap::new();
    if crate::infer::monomorphize::match_types(pattern, ty, &mut mapping).is_none() {
        return false;
    }
    pattern_args.iter().zip(args).all(|(expected, actual)| {
        crate::infer::monomorphize::match_types(expected, actual, &mut mapping).is_some()
    })
}

/// a conversion header is a pair, target and source, matched under **one**
fn conversion_fits(general: &TraitImplDef, special: &TraitImplDef) -> bool {
    let mut mapping = std::collections::HashMap::new();
    crate::infer::monomorphize::match_types(&general.self_type, &special.self_type, &mut mapping)
        .and_then(|()| {
            crate::infer::monomorphize::match_types(
                &general.trait_args[0],
                &special.trait_args[0],
                &mut mapping,
            )
        })
        .is_some()
}

/// one header outranks another by covering it and not being covered by it
fn conversion_outranks(left: &TraitImplDef, right: &TraitImplDef) -> bool {
    conversion_fits(right, left) && !conversion_fits(left, right)
}

/// two impls of one trait at different instantiations declare different method
fn same_instantiation(left: &[InferType], right: &[InferType]) -> bool {
    left.len() == right.len() && left.iter().zip(right).all(|(a, b)| a == b)
}

/// two headers are comparable when one is an instance of the other, target and
fn comparable_headers(
    left_self: &InferType,
    left_args: &[InferType],
    right_self: &InferType,
    right_args: &[InferType],
) -> bool {
    if left_args.len() != right_args.len() {
        return false;
    }
    // a trait without arguments has one instantiation, and every impl of it is at
    if left_args.is_empty() {
        return true;
    }
    // two headers that differ only in the names of their parameters cover each
    header_covers(left_self, left_args, right_self, right_args)
        || header_covers(right_self, right_args, left_self, left_args)
}

/// a header subsumes another when a consistent substitution of its parameters
pub(crate) fn subsumes(general: &InferType, special: &InferType) -> bool {
    let mut mapping = std::collections::HashMap::new();
    crate::infer::monomorphize::match_types(general, special, &mut mapping).is_some()
}

pub(crate) fn strictly_more_specific(special: &InferType, general: &InferType) -> bool {
    subsumes(general, special) && !subsumes(special, general)
}

/// a negative impl states that a type does not implement a trait; it lives apart
pub enum SpecializationChoice {
    Keep,
    Redirect(String),
    Ambiguous(String),
    TooMany(String),
    /// the receiver reached a trait a denial removes it from; a call inside a
    Denied(String),
    /// no impl of the trait at the instantiation the receiver reaches covers it
    NoImpl(String, Vec<InferType>),
}

#[derive(Debug, Clone)]
pub struct NegativeImplDef {
    pub trait_name: String,
    pub trait_args: Vec<InferType>,
    pub self_type: InferType,
    pub owner: ModuleId,
    pub span: aelys_syntax::Span,
}

#[derive(Debug, Clone)]
pub struct TraitImplDef {
    pub opens: bool,
    pub trait_name: String,
    pub trait_args: Vec<InferType>,
    pub self_type: InferType,
    pub methods: Vec<TraitMethod>,
    pub associated_types: Vec<(String, InferType)>,
    pub associated_consts: Vec<(String, InferType, InferType, Option<i64>)>,
}

#[derive(Debug, Clone)]
pub struct EnumVariantDef {
    pub name: String,
    pub fields: EnumVariantFieldsDef,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum EnumVariantFieldsDef {
    Unit,
    Tuple(Vec<InferType>),
    Named(Vec<StructField>),
}

#[derive(Debug, Clone)]
pub struct EnumDef {
    pub name: String,
    pub type_params: Vec<String>,
    pub variants: Vec<EnumVariantDef>,
    pub owner: ModuleId,
    pub is_pub: bool,
}

// unresolved carries the candidate headers, so a caller never has to rebuild them for the diagnostic
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FromSelection {
    Identity,
    Selected(String),
    Unresolved(Vec<String>),
    Denied,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoundSelection {
    CompilerRule,
    /// impls of one instantiation of one trait, none of them outranking the rest
    AmbiguousSpecialization,
    Selected(String),
    Ambiguous,
    Missing,
}

#[derive(Debug, Clone, Default)]
pub struct TypeTable {
    structs: HashMap<String, StructDef>,
    enums: HashMap<String, EnumDef>,
    declaration_order: Vec<String>,
    schema_indices: HashMap<String, u16>,
    struct_definition_ordinals: HashMap<String, u32>,
    enum_schema_indices: HashMap<String, u16>,
    enum_definition_ordinals: HashMap<String, u32>,
    nominal_instance_args: HashMap<String, Vec<InferType>>,
    nominal_instance_origins: HashMap<String, String>,
    nominal_instances_by_key: HashMap<(String, String), String>,
    methods: HashMap<(String, String), StructMethod>,
    traits: HashMap<String, TraitDef>,
    trait_methods: HashMap<(String, String), Vec<TraitMethod>>,
    trait_impls: HashMap<(String, String, String), ()>,
    trait_impl_defs: Vec<TraitImplDef>,
    negative_impls: Vec<NegativeImplDef>,
}

impl TypeTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_struct(&mut self, def: StructDef) {
        if !self.structs.contains_key(&def.name) {
            if let Ok(index) = u16::try_from(self.schema_indices.len()) {
                self.declaration_order.push(def.name.clone());
                self.schema_indices.insert(def.name.clone(), index);
            }
            if !self.struct_definition_ordinals.contains_key(&def.name) {
                let ordinal =
                    u32::try_from(self.struct_definition_ordinals.len()).unwrap_or(u32::MAX);
                self.struct_definition_ordinals
                    .insert(def.name.clone(), ordinal);
            }
        }
        self.structs.insert(def.name.clone(), def);
    }

    pub fn get_struct(&self, name: &str) -> Option<&StructDef> {
        self.structs.get(name)
    }

    pub fn register_enum(&mut self, def: EnumDef) {
        if !self.enums.contains_key(&def.name) {
            if let Ok(index) = u16::try_from(self.enum_schema_indices.len()) {
                self.enum_schema_indices.insert(def.name.clone(), index);
            }
            if !self.enum_definition_ordinals.contains_key(&def.name) {
                let ordinal =
                    u32::try_from(self.enum_definition_ordinals.len()).unwrap_or(u32::MAX);
                self.enum_definition_ordinals
                    .insert(def.name.clone(), ordinal);
            }
        }
        self.enums.insert(def.name.clone(), def);
    }

    pub fn get_enum(&self, name: &str) -> Option<&EnumDef> {
        self.enums.get(name)
    }

    pub fn has_enum(&self, name: &str) -> bool {
        self.enums.contains_key(name)
    }

    pub fn has_nominal(&self, name: &str) -> bool {
        self.has_struct(name) || self.has_enum(name)
    }

    pub fn nominal_owner(&self, name: &str) -> Option<&ModuleId> {
        self.structs
            .get(name)
            .map(|def| &def.owner)
            .or_else(|| self.enums.get(name).map(|def| &def.owner))
            .or_else(|| self.traits.get(name).map(|def| &def.owner))
    }

    pub fn nominal_keyword(&self, name: &str) -> Option<&'static str> {
        if self.has_struct(name) {
            Some("struct")
        } else if self.has_enum(name) {
            Some("enum")
        } else {
            None
        }
    }

    pub fn has_struct(&self, name: &str) -> bool {
        self.structs.contains_key(name)
    }

    pub fn schema_index(&self, name: &str) -> Option<u16> {
        self.schema_indices.get(name).copied()
    }

    pub fn enum_schema_index(&self, name: &str) -> Option<u16> {
        self.enum_schema_indices.get(name).copied()
    }

    // struct ids are ordinals over the sorted instance key, never over the generated instance name
    fn struct_instance_key(&self, name: &str) -> (String, String) {
        let origin = self
            .nominal_instance_origins
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.to_string());
        let args = self
            .nominal_instance_args
            .get(name)
            .map(|args| type_args_key(args))
            .unwrap_or_default();
        (origin, args)
    }

    pub fn finalize_schema_indices(&mut self) {
        let mut struct_names: Vec<_> = self.structs.keys().cloned().collect();
        struct_names.sort_by_key(|name| self.struct_instance_key(name));
        self.declaration_order = struct_names.clone();
        self.schema_indices.clear();
        for (index, name) in struct_names.into_iter().enumerate() {
            if let Ok(index) = u16::try_from(index) {
                self.schema_indices.insert(name, index);
            }
        }

        let mut enum_names: Vec<_> = self.enums.keys().cloned().collect();
        enum_names.sort();
        self.enum_schema_indices.clear();
        for (index, name) in enum_names.into_iter().enumerate() {
            if let Ok(index) = u16::try_from(index) {
                self.enum_schema_indices.insert(name, index);
            }
        }
    }

    pub fn register_nominal_instance_args(&mut self, name: String, args: Vec<InferType>) {
        let args_key = type_args_key(&args);
        self.nominal_instance_args.insert(name.clone(), args);
        if let Some(origin) = self.nominal_instance_origins.get(&name).cloned() {
            self.nominal_instances_by_key
                .insert((origin, args_key), name);
        }
    }

    pub fn nominal_instance_args(&self, name: &str) -> Option<&[InferType]> {
        self.nominal_instance_args.get(name).map(Vec::as_slice)
    }

    pub fn nominal_instance_for(&self, origin: &str, args: &[InferType]) -> Option<&str> {
        self.nominal_instances_by_key
            .get(&(origin.to_string(), type_args_key(args)))
            .map(String::as_str)
    }

    pub fn register_nominal_instance_origin(&mut self, generated: String, origin: String) {
        self.nominal_instance_origins
            .insert(generated.clone(), origin.clone());
        if let Some(args_key) = self
            .nominal_instance_args
            .get(&generated)
            .map(|args| type_args_key(args))
        {
            self.nominal_instances_by_key
                .insert((origin.clone(), args_key), generated.clone());
        }
        if let Some(ordinal) = self.enum_definition_ordinals.get(&origin).copied() {
            self.enum_definition_ordinals
                .insert(generated.clone(), ordinal);
        }
        if let Some(ordinal) = self.struct_definition_ordinals.get(&origin).copied() {
            self.struct_definition_ordinals.insert(generated, ordinal);
        }
    }

    pub fn nominal_instance_origin(&self, name: &str) -> Option<&str> {
        self.nominal_instance_origins.get(name).map(String::as_str)
    }

    pub fn struct_definition_ordinals_snapshot(&self) -> HashMap<String, u32> {
        self.struct_definition_ordinals.clone()
    }

    pub fn enum_definition_ordinals_snapshot(&self) -> HashMap<String, u32> {
        self.enum_definition_ordinals.clone()
    }

    pub fn adopt_definition_ordinal(&mut self, name: &str, ordinal: u32) {
        self.struct_definition_ordinals
            .insert(name.to_string(), ordinal);
    }

    pub fn adopt_enum_definition_ordinal(&mut self, name: &str, ordinal: u32) {
        self.enum_definition_ordinals
            .insert(name.to_string(), ordinal);
    }

    pub fn enum_definition_ordinal(&self, name: &str) -> Option<u32> {
        self.enum_definition_ordinals.get(name).copied()
    }

    pub fn struct_definition_ordinal(&self, name: &str) -> Option<u32> {
        self.struct_definition_ordinals.get(name).copied()
    }

    pub fn open_nominal_templates(&self) -> (Vec<StructDef>, Vec<EnumDef>) {
        let mut structs: Vec<StructDef> = self
            .structs
            .values()
            .filter(|definition| !definition.type_params.is_empty())
            .cloned()
            .collect();
        structs.sort_by(|left, right| left.name.cmp(&right.name));
        let mut enums: Vec<EnumDef> = self
            .enums
            .values()
            .filter(|definition| !definition.type_params.is_empty())
            .cloned()
            .collect();
        enums.sort_by(|left, right| left.name.cmp(&right.name));
        (structs, enums)
    }

    pub fn remove_open_nominals(&mut self) {
        self.structs
            .retain(|_, definition| definition.type_params.is_empty());
        self.enums
            .retain(|_, definition| definition.type_params.is_empty());
        self.declaration_order
            .retain(|name| self.structs.contains_key(name));
        self.finalize_schema_indices();
    }

    pub fn field_offset(&self, structure: &str, field: &str) -> Option<u16> {
        self.structs
            .get(structure)?
            .fields
            .iter()
            .find(|candidate| candidate.name == field)
            .map(|candidate| candidate.ordinal)
    }

    pub fn register_method(&mut self, structure: String, method: StructMethod) {
        self.methods
            .insert((structure, method.name.clone()), method);
    }

    pub fn method(&self, structure: &str, name: &str) -> Option<&StructMethod> {
        self.methods.get(&(structure.to_string(), name.to_string()))
    }

    pub fn register_trait(&mut self, def: TraitDef) {
        self.traits.insert(def.name.clone(), def);
    }

    pub fn get_trait(&self, name: &str) -> Option<&TraitDef> {
        self.traits.get(name)
    }

    pub fn register_trait_impl(&mut self, trait_name: String, target: String) -> bool {
        self.register_trait_impl_with_args(trait_name, target, &[])
    }

    pub fn register_trait_impl_with_args(
        &mut self,
        trait_name: String,
        target: String,
        trait_args: &[InferType],
    ) -> bool {
        self.trait_impls
            .insert((trait_name, target, type_args_key(trait_args)), ())
            .is_none()
    }

    pub fn register_trait_impl_def(&mut self, definition: TraitImplDef) {
        self.trait_impl_defs.push(definition);
    }

    pub fn push_trait_impl_def(&mut self, definition: TraitImplDef) -> usize {
        self.trait_impl_defs.push(definition);
        self.trait_impl_defs.len() - 1
    }

    pub fn trait_impl_def_at_mut(&mut self, index: usize) -> Option<&mut TraitImplDef> {
        self.trait_impl_defs.get_mut(index)
    }

    pub fn remove_trait_impl_defs(&mut self, indices: &[usize]) {
        for index in indices.iter().rev() {
            if *index < self.trait_impl_defs.len() {
                self.trait_impl_defs.remove(*index);
            }
        }
    }

    // an entry whose header has not resolved yet must not read as a competing one
    pub fn trait_impl_overlap_among(
        &self,
        indices: &[usize],
        trait_name: &str,
        trait_args: &[InferType],
        self_type: &InferType,
        opens: bool,
    ) -> Vec<(usize, InferType)> {
        indices
            .iter()
            .filter_map(|index| {
                self.trait_impl_defs
                    .get(*index)
                    .map(|definition| (*index, definition))
            })
            .filter(|(_, definition)| {
                definition.trait_name == trait_name
                    && headers_overlap(
                        &definition.self_type,
                        &definition.trait_args,
                        self_type,
                        trait_args,
                    )
                    && !self.opens_to_specialization(definition, self_type, opens)
            })
            .map(|(index, definition)| (index, definition.self_type.clone()))
            .collect()
    }

    /// an impl that replaces an open root inherits the root's associated items,
    pub fn replaces_an_open_root(&self, trait_name: &str, self_type: &InferType) -> bool {
        self.trait_impl_defs.iter().any(|root| {
            root.opens
                && root.trait_name == trait_name
                && strictly_more_specific(self_type, &root.self_type)
        })
    }

    /// two headers of one trait that subsume each other: no type will ever tell
    pub fn equally_specific_overlap(
        &self,
        indices: &[usize],
        trait_name: &str,
        trait_args: &[InferType],
        self_type: &InferType,
        opens: bool,
    ) -> Vec<usize> {
        indices
            .iter()
            .copied()
            .filter(|index| {
                self.trait_impl_defs.get(*index).is_some_and(|definition| {
                    definition.trait_name == trait_name
                        && same_instantiation(&definition.trait_args, trait_args)
                        && (definition.opens || opens)
                        && subsumes(&definition.self_type, self_type)
                        && subsumes(self_type, &definition.self_type)
                })
            })
            .collect()
    }

    /// an overlap is licit when one header is strictly more specific than the
    fn opens_to_specialization(
        &self,
        seen: &TraitImplDef,
        incoming: &InferType,
        incoming_opens: bool,
    ) -> bool {
        if strictly_more_specific(incoming, &seen.self_type) {
            return seen.opens;
        }
        if strictly_more_specific(&seen.self_type, incoming) {
            return incoming_opens;
        }
        // neither outranks the other, so the pair alone cannot answer: they may
        self.shares_an_open_root(seen, incoming)
    }

    fn shares_an_open_root(&self, seen: &TraitImplDef, incoming: &InferType) -> bool {
        self.trait_impl_defs.iter().any(|root| {
            root.opens
                && root.trait_name == seen.trait_name
                && strictly_more_specific(&seen.self_type, &root.self_type)
                && strictly_more_specific(incoming, &root.self_type)
        })
    }

    pub fn types_match(&self, expected: &InferType, actual: &InferType) -> bool {
        if let InferType::Projection { .. } = expected
            && let Some(resolved) = self.resolve_projection(expected)
        {
            return type_matches(&resolved, actual) || type_matches(actual, &resolved);
        }
        if let InferType::Projection { .. } = actual
            && let Some(resolved) = self.resolve_projection(actual)
        {
            return type_matches(&resolved, expected) || type_matches(expected, &resolved);
        }
        type_matches(expected, actual) || type_matches(actual, expected)
    }

    pub fn trait_declaring_item(&self, trait_name: &str, item: &str) -> Option<String> {
        self.trait_declaring_item_in(trait_name, item, None)
    }

    /// every supertrait an impl of this trait at this instantiation must satisfy,
    pub fn supertrait_obligations(
        &self,
        trait_name: &str,
        trait_args: &[InferType],
    ) -> Vec<(String, Vec<InferType>)> {
        let mut out = Vec::new();
        let mut queue =
            std::collections::VecDeque::from([(trait_name.to_string(), trait_args.to_vec())]);
        // the identity of an obligation is its instantiation, not its name: a
        let mut seen = std::collections::HashSet::new();
        while let Some((name, args)) = queue.pop_front() {
            if !seen.insert(instantiation_key(&name, &args)) {
                continue;
            }
            if let Some(definition) = self.get_trait(&name) {
                let mut substitution = crate::unify::Substitution::new();
                for (param, ty) in definition.type_params.iter().zip(&args) {
                    substitution.bind_param(param.clone(), ty.clone());
                }
                for (super_name, super_args) in &definition.super_bounds {
                    queue.push_back((
                        super_name.clone(),
                        super_args
                            .iter()
                            .map(|arg| substitution.apply(arg))
                            .collect(),
                    ));
                }
            }
            out.push((name, args));
        }
        out
    }

    // keyed on one trait must accept an impl of any of them, and must not pick a
    pub fn supertrait_closure(&self, trait_name: &str) -> Vec<String> {
        let mut closure = Vec::new();
        let mut queue = std::collections::VecDeque::from([trait_name.to_string()]);
        let mut seen = std::collections::HashSet::new();
        while let Some(name) = queue.pop_front() {
            if !seen.insert(name.clone()) {
                continue;
            }
            if let Some(definition) = self.get_trait(&name) {
                queue.extend(definition.super_bounds.iter().map(|(name, _)| name.clone()));
            }
            closure.push(name);
        }
        closure
    }

    pub fn trait_declaring_item_in(
        &self,
        trait_name: &str,
        item: &str,
        namespace: Option<ItemNamespace>,
    ) -> Option<String> {
        let mut queue = vec![trait_name.to_string()];
        let mut seen = std::collections::HashSet::new();
        while let Some(name) = queue.pop() {
            if !seen.insert(name.clone()) {
                continue;
            }
            let Some(definition) = self.get_trait(&name) else {
                continue;
            };
            let declares_type = definition.associated_types.contains(&item.to_string());
            let declares_const = definition
                .associated_consts
                .iter()
                .any(|(candidate, _)| candidate == item);
            let declares = match namespace {
                Some(ItemNamespace::Type) => declares_type,
                Some(ItemNamespace::Const) => declares_const,
                None => declares_type || declares_const,
            };
            if declares {
                return Some(name);
            }
            queue.extend(definition.super_bounds.iter().map(|(name, _)| name.clone()));
        }
        None
    }

    pub fn declared_associated_items(
        &self,
        trait_name: &str,
        namespace: ItemNamespace,
    ) -> Vec<String> {
        if namespace == ItemNamespace::Const {
            return self
                .declared_associated_consts(trait_name)
                .into_iter()
                .map(|(item, _)| item)
                .collect();
        }
        let Some(definition) = self.get_trait(trait_name) else {
            return Vec::new();
        };
        let mut named = std::collections::HashSet::new();
        definition
            .associated_types
            .iter()
            .filter(|item| named.insert((*item).clone()))
            .cloned()
            .collect()
    }

    pub fn declared_associated_consts(&self, trait_name: &str) -> Vec<(String, InferType)> {
        let Some(definition) = self.get_trait(trait_name) else {
            return Vec::new();
        };
        let mut named = std::collections::HashSet::new();
        definition
            .associated_consts
            .iter()
            .filter(|(item, _)| named.insert(item.clone()))
            .cloned()
            .collect()
    }

    pub fn resolve_projection(&self, projection: &InferType) -> Option<InferType> {
        let InferType::Projection {
            trait_name,
            item,
            self_ty,
        } = projection
        else {
            return None;
        };
        let closure = trait_name
            .as_deref()
            .map(|name| self.supertrait_closure(name));
        if !self_ty.is_concrete() {
            return self.resolve_rigid_projection(closure.as_deref(), item, self_ty);
        }
        // a replacement of an open root supplies the item too, so a projection on a
        let candidates: Vec<&TraitImplDef> = self
            .trait_impl_defs
            .iter()
            .filter(|implementation| {
                closure
                    .as_deref()
                    .is_none_or(|names| names.contains(&implementation.trait_name))
                    && subsumes(&implementation.self_type, self_ty)
                    && implementation
                        .associated_types
                        .iter()
                        .any(|(name, _)| name == item)
            })
            .collect();
        let chosen = match candidates.as_slice() {
            [] => None,
            [only] => Some(*only),
            _ => {
                let minima: Vec<&&TraitImplDef> = candidates
                    .iter()
                    .filter(|candidate| {
                        !candidates.iter().any(|other| {
                            other.trait_name == candidate.trait_name
                                && strictly_more_specific(&other.self_type, &candidate.self_type)
                        })
                    })
                    .collect();
                match minima.as_slice() {
                    [only] => Some(**only),
                    _ => None,
                }
            }
        }?;
        let (_, ty) = chosen
            .associated_types
            .iter()
            .find(|(name, _)| name == item)?;
        Some(instantiate_impl_definition(&chosen.self_type, self_ty, ty))
    }

    /// a receiver whose type parameters stay open reads the item of the impl that covers it, when every impl that reaches it agrees
    fn resolve_rigid_projection(
        &self,
        closure: Option<&[String]>,
        item: &str,
        self_ty: &InferType,
    ) -> Option<InferType> {
        if !self_ty.is_rigid() {
            return None;
        }
        let defining = |implementation: &&TraitImplDef| {
            closure.is_none_or(|names| names.contains(&implementation.trait_name))
                && implementation
                    .associated_types
                    .iter()
                    .any(|(name, _)| name == item)
        };
        let value_of = |implementation: &TraitImplDef| {
            implementation
                .associated_types
                .iter()
                .find(|(name, _)| name == item)
                .map(|(_, ty)| ty.clone())
        };
        // of the impls covering the receiver, the most specific runs wherever the others would, so the others never answer for it
        let covering: Vec<&TraitImplDef> = self
            .trait_impl_defs
            .iter()
            .filter(defining)
            .filter(|implementation| subsumes(&implementation.self_type, self_ty))
            .collect();
        let root = *covering.iter().find(|candidate| {
            covering.iter().all(|other| {
                std::ptr::eq(*other, **candidate)
                    || strictly_more_specific(&candidate.self_type, &other.self_type)
            })
        })?;
        let answer = instantiate_impl_definition(&root.self_type, self_ty, &value_of(root)?);
        for other in self.trait_impl_defs.iter().filter(defining) {
            if std::ptr::eq(other, root)
                || (other.trait_name == root.trait_name
                    && strictly_more_specific(&root.self_type, &other.self_type))
            {
                continue;
            }
            let mut bindings = std::collections::HashMap::new();
            if !unify_headers(
                &tag_params(&other.self_type, "l:"),
                &tag_params(self_ty, "r:"),
                &mut bindings,
            ) {
                continue;
            }
            // where it applies, a more specific impl may cover the receiver whole, and then this one never runs on it
            let reached = settle_bindings(&tag_params(self_ty, "r:"), &bindings);
            let outranked_there = self.trait_impl_defs.iter().any(|better| {
                better.trait_name == other.trait_name
                    && strictly_more_specific(&better.self_type, &other.self_type)
                    && subsumes(&better.self_type, &reached)
            });
            if outranked_there {
                continue;
            }
            let theirs = settle_bindings(&tag_params(&value_of(other)?, "l:"), &bindings);
            let ours = settle_bindings(&tag_params(&answer, "r:"), &bindings);
            if theirs != ours {
                return None;
            }
        }
        Some(answer)
    }

    pub fn trait_impl_defs(&self) -> &[TraitImplDef] {
        &self.trait_impl_defs
    }

    pub fn push_negative_impl(&mut self, definition: NegativeImplDef) {
        self.negative_impls.push(definition);
    }

    /// two headers of one trait on one nominal that overlap without one being
    pub fn negative_header_clash(
        &self,
        trait_name: &str,
        self_type: &InferType,
        trait_args: &[InferType],
    ) -> bool {
        self.negative_impls.iter().any(|definition| {
            definition.trait_name == trait_name
                && headers_overlap(
                    &definition.self_type,
                    &definition.trait_args,
                    self_type,
                    trait_args,
                )
                && !strictly_more_specific(self_type, &definition.self_type)
                && !strictly_more_specific(&definition.self_type, self_type)
        })
    }

    /// only a denial strictly more specific than a positive header carves a hole
    pub fn positive_negative_clash(
        &self,
        trait_name: &str,
        self_type: &InferType,
        trait_args: &[InferType],
    ) -> bool {
        self.negative_impls.iter().any(|definition| {
            definition.trait_name == trait_name
                && headers_overlap(
                    &definition.self_type,
                    &definition.trait_args,
                    self_type,
                    trait_args,
                )
                && !strictly_more_specific(&definition.self_type, self_type)
        })
    }

    /// the mirror, for a negative header meeting the positive impls already seen
    pub fn positive_header_clash(
        &self,
        trait_name: &str,
        self_type: &InferType,
        trait_args: &[InferType],
    ) -> bool {
        self.trait_impl_defs.iter().any(|definition| {
            definition.trait_name == trait_name
                && headers_overlap(
                    &definition.self_type,
                    &definition.trait_args,
                    self_type,
                    trait_args,
                )
                && !(definition.opens && strictly_more_specific(self_type, &definition.self_type))
        })
    }

    /// the symbol of the most specific impl that can receive this type, when the
    pub const MAX_APPLICABLE_IMPLS: usize = 64;

    pub fn select_specialization(
        &self,
        receiver: &InferType,
        current: &str,
    ) -> SpecializationChoice {
        if !receiver.is_concrete() {
            return SpecializationChoice::Keep;
        }
        let Some(root) = self
            .trait_impl_defs
            .iter()
            .find(|definition| definition.methods.iter().any(|e| e.symbol == current))
        else {
            return SpecializationChoice::Keep;
        };
        let Some(method) = root
            .methods
            .iter()
            .find(|entry| entry.symbol == current)
            .map(|entry| entry.name.clone())
        else {
            return SpecializationChoice::Keep;
        };
        // is about the trait this receiver reaches, so they are instantiated for it
        let reached_args = instantiate_args(&root.self_type, &root.trait_args, receiver);
        if self.denies(&root.trait_name, receiver, &reached_args) {
            return SpecializationChoice::Denied(root.trait_name.clone());
        }
        let applicable: Vec<&TraitImplDef> = self
            .trait_impl_defs
            .iter()
            .filter(|definition| {
                definition.trait_name == root.trait_name
                    && comparable_headers(
                        &definition.self_type,
                        &definition.trait_args,
                        &root.self_type,
                        &root.trait_args,
                    )
                    && subsumes(&definition.self_type, receiver)
                    && definition.methods.iter().any(|entry| entry.name == method)
            })
            .collect();
        if applicable.len() > Self::MAX_APPLICABLE_IMPLS {
            return SpecializationChoice::TooMany(root.trait_name.clone());
        }
        let minima: Vec<&&TraitImplDef> = applicable
            .iter()
            .filter(|candidate| {
                !applicable
                    .iter()
                    .any(|other| strictly_more_specific(&other.self_type, &candidate.self_type))
            })
            .collect();
        if minima.len() > 1 {
            return SpecializationChoice::Ambiguous(root.trait_name.clone());
        }
        // no impl comparable with the one kept covers the receiver, so running the kept one would read a receiver it was never written for
        let Some(best) = minima.first() else {
            // arguments the receiver did not fix still name the impl's parameters, which no diagnostic may print
            let printable = if reached_args.iter().any(InferType::mentions_any_param) {
                Vec::new()
            } else {
                reached_args
            };
            return SpecializationChoice::NoImpl(root.trait_name.clone(), printable);
        };
        if std::ptr::eq(**best, root) {
            return SpecializationChoice::Keep;
        }
        best.methods
            .iter()
            .find(|entry| entry.name == method)
            .map(|entry| SpecializationChoice::Redirect(entry.symbol.clone()))
            .unwrap_or(SpecializationChoice::Keep)
    }

    /// when several impls of one trait supply the method and they form a chain,
    pub fn specialization_root(
        &self,
        target: &str,
        method: &str,
        among: &[String],
    ) -> Option<String> {
        // only the impls still in the running: two chains on one nominal each keep their own root once the receiver has set the other aside
        let supplying: Vec<&TraitImplDef> = self
            .trait_impl_defs
            .iter()
            .filter(|definition| {
                nominal_name(&definition.self_type).as_deref() == Some(target)
                    && definition
                        .methods
                        .iter()
                        .any(|entry| entry.name == method && among.contains(&entry.symbol))
            })
            .collect();
        if supplying.len() < 2 {
            return None;
        }
        let first = supplying.first()?;
        if supplying
            .iter()
            .any(|other| other.trait_name != first.trait_name)
        {
            return None;
        }
        let comparable = |left: &TraitImplDef, right: &TraitImplDef| {
            comparable_headers(
                &left.self_type,
                &left.trait_args,
                &right.self_type,
                &right.trait_args,
            )
        };
        // candidates that cannot both receive one type need no root: whichever is
        let disjoint = supplying.iter().all(|candidate| {
            supplying.iter().all(|other| {
                std::ptr::eq(*other, *candidate)
                    || !types_overlap(&other.self_type, &candidate.self_type)
            })
        });
        let root = match disjoint {
            true => {
                // a trait implemented at several instantiations is E0437's business,
                if supplying.iter().any(|other| !comparable(other, first)) {
                    return None;
                }
                first
            }
            // the root is the one every other refines, compared with it and not with whichever the file declared first
            false => {
                let found = supplying.iter().find(|candidate| {
                    supplying.iter().all(|other| {
                        std::ptr::eq(*other, **candidate)
                            || (comparable(other, candidate)
                                && strictly_more_specific(&other.self_type, &candidate.self_type))
                    })
                })?;
                if !found.opens {
                    return None;
                }
                found
            }
        };
        root.methods
            .iter()
            .find(|entry| entry.name == method)
            .map(|entry| entry.symbol.clone())
    }

    /// the symbols `method` receives from impls of `trait_name` at another instantiation than the one `header` names
    pub fn symbols_at_other_instantiations(
        &self,
        trait_name: &str,
        header: &InferType,
        args: &[InferType],
        method: &str,
    ) -> Vec<String> {
        self.trait_impl_defs
            .iter()
            .filter(|definition| {
                definition.trait_name == trait_name
                    && !headers_overlap(&definition.self_type, &definition.trait_args, header, args)
            })
            .flat_map(|definition| definition.methods.iter())
            .filter(|entry| entry.name == method)
            .map(|entry| entry.symbol.clone())
            .collect()
    }

    /// some instantiation of both reaches this header from the receiver
    pub fn reaches(&self, header: &InferType, receiver: &InferType) -> bool {
        headers_overlap(header, &[], receiver, &[])
    }

    /// the impl that supplies `symbol` receives the receiver whatever its own type parameters become
    pub fn impl_covers(&self, symbol: &str, receiver: &InferType) -> bool {
        self.trait_impl_defs
            .iter()
            .find(|definition| {
                definition
                    .methods
                    .iter()
                    .any(|entry| entry.symbol == symbol)
            })
            .is_some_and(|definition| subsumes(&definition.self_type, receiver))
    }

    /// the symbols an ordering leaves out: every applicable impl that another
    pub fn symbols_outranked(&self, receiver: &InferType, method: &str) -> Vec<String> {
        if !receiver.is_concrete() {
            return Vec::new();
        }
        let applicable: Vec<&TraitImplDef> = self
            .trait_impl_defs
            .iter()
            .filter(|definition| subsumes(&definition.self_type, receiver))
            .filter(|definition| definition.methods.iter().any(|entry| entry.name == method))
            .collect();
        let mut out = Vec::new();
        for definition in &applicable {
            let outranked = applicable.iter().any(|other| {
                other.trait_name == definition.trait_name
                    && comparable_headers(
                        &other.self_type,
                        &other.trait_args,
                        &definition.self_type,
                        &definition.trait_args,
                    )
                    && strictly_more_specific(&other.self_type, &definition.self_type)
            });
            if !outranked {
                continue;
            }
            for entry in &definition.methods {
                if entry.name == method {
                    out.push(entry.symbol.clone());
                }
            }
        }
        out
    }

    /// the symbols of impls whose self type cannot receive this receiver; a
    pub fn symbols_not_applying(&self, receiver: &InferType, method: &str) -> Vec<String> {
        let mut out = Vec::new();
        // an open receiver rules nothing out: every impl could still be the one
        if !receiver.is_concrete() && !receiver.is_rigid() {
            return out;
        }
        for definition in &self.trait_impl_defs {
            let reachable = match receiver.is_concrete() {
                true => subsumes(&definition.self_type, receiver),
                // a type parameter can still become anything, so only an impl that no instantiation of it reaches is out
                false => headers_overlap(&definition.self_type, &[], receiver, &[]),
            };
            if reachable {
                continue;
            }
            for entry in &definition.methods {
                if entry.name == method {
                    out.push(entry.symbol.clone());
                }
            }
        }
        out
    }

    /// the symbol an impl of this trait on this nominal gives the named method
    pub fn trait_impl_method(
        &self,
        trait_name: &str,
        target: &str,
        method: &str,
    ) -> Option<String> {
        let mut found: Option<String> = None;
        for definition in &self.trait_impl_defs {
            if definition.trait_name != trait_name
                || nominal_name(&definition.self_type).as_deref() != Some(target)
            {
                continue;
            }
            for entry in &definition.methods {
                if entry.name != method {
                    continue;
                }
                if found.is_some() {
                    return None;
                }
                found = Some(entry.symbol.clone());
            }
        }
        found
    }

    /// the traits whose impls on this nominal supply the named method
    pub fn traits_supplying(&self, target: &str, method: &str) -> Vec<String> {
        let mut names = Vec::new();
        for definition in &self.trait_impl_defs {
            if nominal_name(&definition.self_type).as_deref() != Some(target) {
                continue;
            }
            if !definition.methods.iter().any(|entry| entry.name == method) {
                continue;
            }
            if !names.contains(&definition.trait_name) {
                names.push(definition.trait_name.clone());
            }
        }
        names
    }

    /// the denial is matched against the receiver type, never against its bare
    pub fn denies(&self, trait_name: &str, ty: &InferType, trait_args: &[InferType]) -> bool {
        self.negative_impls.iter().any(|definition| {
            definition.trait_name == trait_name
                && definition.trait_args.len() == trait_args.len()
                && header_covers(
                    &definition.self_type,
                    &definition.trait_args,
                    ty,
                    trait_args,
                )
        })
    }

    /// the impls of `target` that supply any of `symbols`, as (trait name, its instantiation) one entry per impl
    pub fn trait_impls_supplying(
        &self,
        target: &str,
        symbols: &[String],
    ) -> Vec<(String, String, String)> {
        let mut out: Vec<(String, String, String)> = Vec::new();
        for definition in &self.trait_impl_defs {
            if nominal_name(&definition.self_type).as_deref() != Some(target) {
                continue;
            }
            if !definition
                .methods
                .iter()
                .any(|method| symbols.contains(&method.symbol))
            {
                continue;
            }
            // two impls reach one instantiation when each argument they give the trait is the same position of the receiver, or the same type
            let mut positions = HashMap::new();
            receiver_positions(&definition.self_type, &mut Vec::new(), &mut positions);
            let key: Vec<String> = definition
                .trait_args
                .iter()
                .map(|arg| arg.substitute_params(&positions).source_spelling())
                .collect();
            out.push((
                definition.trait_name.clone(),
                key.join(", "),
                format!(
                    "{} for {}",
                    trait_instantiation_spelling(&definition.trait_name, &definition.trait_args),
                    definition.self_type.source_spelling()
                ),
            ));
        }
        // the listing is read by a diagnostic, so source order must not reach it
        out.sort_by(|left, right| left.2.cmp(&right.2));
        out
    }

    pub fn trait_instantiations_of(&self, target: &str, trait_name: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for definition in &self.trait_impl_defs {
            if definition.trait_name != trait_name
                || nominal_name(&definition.self_type).as_deref() != Some(target)
            {
                continue;
            }
            let spelling = trait_instantiation_spelling(trait_name, &definition.trait_args);
            if !out.contains(&spelling) {
                out.push(spelling);
            }
        }
        // the listing is read by a diagnostic, so source order must not reach it
        out.sort();
        out
    }

    pub fn has_trait_impl(&self, trait_name: &str, target: &str) -> bool {
        self.has_trait_impl_with_args(trait_name, target, &[])
    }

    pub fn has_trait_impl_with_args(
        &self,
        trait_name: &str,
        target: &str,
        trait_args: &[InferType],
    ) -> bool {
        self.trait_impls.keys().any(|(name, impl_target, args)| {
            name == trait_name
                && impl_target == target
                && (trait_args.is_empty() || args == &type_args_key(trait_args))
        }) || self.trait_impl_defs.iter().any(|definition| {
            definition.trait_name == trait_name
                && nominal_name(&definition.self_type).as_deref() == Some(target)
                && (trait_args.is_empty()
                    || (definition.trait_args.len() == trait_args.len()
                        && definition
                            .trait_args
                            .iter()
                            .zip(trait_args)
                            .all(|(expected, actual)| self.types_match(expected, actual))))
        })
    }

    pub fn satisfies_bound(
        &self,
        trait_name: &str,
        ty: &InferType,
        trait_args: &[InferType],
    ) -> bool {
        // the denial is read first, and before the prelude short circuit, or a
        if self.denies(trait_name, ty, trait_args) {
            return false;
        }
        if crate::prelude::provides(trait_name, ty, trait_args) {
            return true;
        }
        self.implements(trait_name, ty, trait_args)
    }

    /// whether an impl of this trait covers this exact type with these exact trait
    pub fn implements(&self, trait_name: &str, ty: &InferType, trait_args: &[InferType]) -> bool {
        // answered by the name alone; only a concrete question can be refused on
        if !ty.is_concrete() || trait_args.iter().any(|arg| !arg.is_concrete()) {
            return nominal_name(ty).is_some_and(|target| {
                self.has_trait_impl_with_args(trait_name, &target, trait_args)
            });
        }
        self.trait_impl_defs.iter().any(|definition| {
            definition.trait_name == trait_name
                // instantiation, exactly as the registry answered before
                && if trait_args.is_empty() {
                    subsumes(&definition.self_type, ty)
                } else {
                    definition.trait_args.len() == trait_args.len()
                        && header_covers(
                            &definition.self_type,
                            &definition.trait_args,
                            ty,
                            trait_args,
                        )
                }
        }) || nominal_name(ty).is_some_and(|target| {
            // the registered triples carry no header types, so they answer only the
            self.trait_impls.keys().any(|(name, impl_target, args)| {
                name == trait_name
                    && *impl_target == target
                    && (trait_args.is_empty() || args == &type_args_key(trait_args))
            })
        })
    }

    /// the denial is read first, and before the prelude short circuit. a display
    pub fn select_bound_method(
        &self,
        trait_name: &str,
        ty: &InferType,
        method_name: &str,
    ) -> BoundSelection {
        if self.denies(trait_name, ty, &[]) {
            return BoundSelection::Missing;
        }
        if crate::prelude::provides(trait_name, ty, &[]) {
            return BoundSelection::CompilerRule;
        }
        // the bound names the trait, the receiver names the impl: a header the
        let mut candidates: Vec<(&TraitImplDef, String)> = Vec::new();
        for definition in &self.trait_impl_defs {
            if definition.trait_name != trait_name || !subsumes(&definition.self_type, ty) {
                continue;
            }
            for method in &definition.methods {
                if method.name == method_name
                    && !candidates.iter().any(|(_, known)| *known == method.symbol)
                {
                    candidates.push((definition, method.symbol.clone()));
                }
            }
        }
        // two instantiations of one trait declare different signatures, so which this receiver reads must be settled, whichever the file declared first
        let reached: Vec<Vec<InferType>> = candidates
            .iter()
            .map(|(definition, _)| {
                instantiate_args(&definition.self_type, &definition.trait_args, ty)
            })
            .collect();
        let one_instantiation = reached
            .first()
            .is_some_and(|first| reached.iter().all(|other| other == first));
        if candidates.len() > 1 && one_instantiation {
            let most_specific = candidates.iter().position(|(definition, _)| {
                candidates.iter().all(|(other, _)| {
                    std::ptr::eq(*definition, *other)
                        || strictly_more_specific(&definition.self_type, &other.self_type)
                })
            });
            // the one it selects must refine the others as a specialization does, header and trait arguments together, or no call can name it
            let refines_them = |index: usize| {
                let (chosen, _) = &candidates[index];
                candidates.iter().all(|(other, _)| {
                    std::ptr::eq(*other, *chosen)
                        || comparable_headers(
                            &chosen.self_type,
                            &chosen.trait_args,
                            &other.self_type,
                            &other.trait_args,
                        )
                })
            };
            return match most_specific {
                Some(index) if refines_them(index) => {
                    BoundSelection::Selected(candidates.swap_remove(index).1)
                }
                Some(_) => BoundSelection::Ambiguous,
                None => BoundSelection::AmbiguousSpecialization,
            };
        }
        match candidates.len() {
            0 => BoundSelection::Missing,
            1 => BoundSelection::Selected(candidates.swap_remove(0).1),
            // two impls of the same trait differing only in trait arguments cannot be told apart here
            _ => BoundSelection::Ambiguous,
        }
    }

    // empty when the impl is not unique, so an ambiguous target never resolves to one symbol
    pub fn sole_trait_impl_args(&self, trait_name: &str, target: &str) -> Vec<InferType> {
        let mut found: Option<&Vec<InferType>> = None;
        for definition in &self.trait_impl_defs {
            if definition.trait_name != trait_name
                || nominal_name(&definition.self_type).as_deref() != Some(target)
            {
                continue;
            }
            if found.is_some() {
                return Vec::new();
            }
            found = Some(&definition.trait_args);
        }
        found.cloned().unwrap_or_default()
    }

    pub fn select_from_conversion(&self, source: &InferType, target: &InferType) -> FromSelection {
        // the denial names the conversion this site asks for, target and source
        if self.denies(
            crate::prelude::FROM_TRAIT,
            target,
            std::slice::from_ref(source),
        ) {
            return FromSelection::Denied;
        }
        if crate::prelude::provides(
            crate::prelude::FROM_TRAIT,
            target,
            std::slice::from_ref(source),
        ) {
            return FromSelection::Identity;
        }
        // the two halves of a conversion header are matched under **one**
        let candidates: Vec<&TraitImplDef> = self
            .matching_from_impls(target)
            .filter(|definition| {
                let mut mapping = std::collections::HashMap::new();
                crate::infer::monomorphize::match_types(&definition.self_type, target, &mut mapping)
                    .and_then(|()| {
                        crate::infer::monomorphize::match_types(
                            &definition.trait_args[0],
                            source,
                            &mut mapping,
                        )
                    })
                    .is_some()
            })
            .collect();
        // a conversion is specialized like any other impl, and its order reads both
        let chosen = match candidates.len() {
            0 => None,
            1 => candidates.first().copied(),
            _ => {
                let minima: Vec<&&TraitImplDef> = candidates
                    .iter()
                    .filter(|candidate| {
                        !candidates
                            .iter()
                            .any(|other| conversion_outranks(other, candidate))
                    })
                    .collect();
                match minima.as_slice() {
                    [only] => Some(**only),
                    _ => None,
                }
            }
        };
        let symbol = chosen.and_then(|definition| {
            definition
                .methods
                .iter()
                .find(|method| {
                    method.name == crate::prelude::FROM_METHOD && !method.symbol.is_empty()
                })
                .map(|method| method.symbol.clone())
        });
        match symbol {
            Some(symbol) => FromSelection::Selected(symbol),
            None => FromSelection::Unresolved(self.conversion_headers_for(target)),
        }
    }

    pub fn conversion_headers_for(&self, target: &InferType) -> Vec<String> {
        self.matching_from_impls(target)
            .map(|definition| {
                format!(
                    "{}<{}> for {}",
                    crate::prelude::FROM_TRAIT,
                    definition.trait_args[0],
                    definition.self_type
                )
            })
            .collect()
    }

    fn matching_from_impls<'a>(
        &'a self,
        target: &'a InferType,
    ) -> impl Iterator<Item = &'a TraitImplDef> {
        self.trait_impl_defs.iter().filter(move |definition| {
            definition.trait_name == crate::prelude::FROM_TRAIT
                && definition.trait_args.len() == 1
                && type_matches(&definition.self_type, target)
        })
    }

    pub fn register_trait_method(&mut self, target: String, method: TraitMethod) {
        self.trait_methods
            .entry((target, method.name.clone()))
            .or_default()
            .push(method);
    }

    pub fn trait_methods(&self, target: &str, name: &str) -> &[TraitMethod] {
        self.trait_methods
            .get(&(target.to_string(), name.to_string()))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn trait_method(&self, target: &str, name: &str) -> Option<&TraitMethod> {
        let methods = self.trait_methods(target, name);
        (methods.len() == 1).then(|| &methods[0])
    }

    pub fn structs_in_declaration_order(&self) -> impl Iterator<Item = &StructDef> {
        self.declaration_order
            .iter()
            .filter_map(|name| self.structs.get(name))
    }

    pub fn enums_in_declaration_order(&self) -> impl Iterator<Item = &EnumDef> {
        let mut entries: Vec<_> = self.enums.values().collect();
        entries.sort_by_key(|def| self.enum_schema_indices.get(&def.name).copied());
        entries.into_iter()
    }

    // a caller that reports only the first entry must not see a different one on a
    pub fn unmaterialized_applied_types(&self) -> Vec<(InferType, Span)> {
        let mut found = Vec::new();
        for definition in self.structs.values() {
            for field in &definition.fields {
                collect_applied_types_at(
                    &field.ty,
                    &format!("{}.{}", definition.name, field.name),
                    field.span,
                    &mut found,
                );
            }
        }
        for definition in self.enums.values() {
            for variant in &definition.variants {
                let site = format!("{}.{}", definition.name, variant.name);
                match &variant.fields {
                    EnumVariantFieldsDef::Unit => {}
                    EnumVariantFieldsDef::Tuple(fields) => {
                        for (index, field) in fields.iter().enumerate() {
                            collect_applied_types_at(
                                field,
                                &format!("{site}.{index}"),
                                variant.span,
                                &mut found,
                            );
                        }
                    }
                    EnumVariantFieldsDef::Named(fields) => {
                        for field in fields {
                            collect_applied_types_at(
                                &field.ty,
                                &format!("{site}.{}", field.name),
                                field.span,
                                &mut found,
                            );
                        }
                    }
                }
            }
        }
        found.sort_by(|left, right| (&left.0, &left.1).cmp(&(&right.0, &right.1)));
        found
            .into_iter()
            .map(|(_, _, ty, span)| (ty, span))
            .collect()
    }
}

fn collect_applied_types_at(
    ty: &InferType,
    site: &str,
    span: Span,
    found: &mut Vec<(String, String, InferType, Span)>,
) {
    let mut applied = Vec::new();
    collect_applied_types(ty, &mut applied);
    for candidate in applied {
        found.push((candidate.to_string(), site.to_string(), candidate, span));
    }
}

fn collect_applied_types(ty: &InferType, found: &mut Vec<InferType>) {
    match ty {
        InferType::Applied { args, .. } => {
            found.push(ty.clone());
            for arg in args {
                collect_applied_types(arg, found);
            }
        }
        InferType::Function { params, ret } => {
            for param in params {
                collect_applied_types(param, found);
            }
            collect_applied_types(ret, found);
        }
        InferType::Array(inner)
        | InferType::FixedArray(inner, _)
        | InferType::Vec(inner)
        | InferType::Option(inner) => collect_applied_types(inner, found),
        InferType::Result(ok, err) => {
            collect_applied_types(ok, found);
            collect_applied_types(err, found);
        }
        InferType::Tuple(elements) => {
            for element in elements {
                collect_applied_types(element, found);
            }
        }
        _ => {}
    }
}

fn type_args_key(args: &[InferType]) -> String {
    let mut key = String::new();
    for arg in args {
        use std::fmt::Write;
        let rendered = arg.to_string();
        let _ = write!(key, "{}:{}", rendered.len(), rendered);
    }
    key
}

pub(crate) fn instantiate_impl_definition(
    impl_self_type: &InferType,
    actual_self_ty: &InferType,
    definition: &InferType,
) -> InferType {
    let mut bindings = std::collections::HashMap::new();
    bind_pattern_params(impl_self_type, actual_self_ty, &mut bindings);
    if bindings.is_empty() {
        return definition.clone();
    }
    definition.substitute_params(&bindings)
}

fn bind_pattern_params(
    pattern: &InferType,
    actual: &InferType,
    out: &mut std::collections::HashMap<String, InferType>,
) {
    match (pattern, actual) {
        (InferType::Param(name), actual) => {
            out.entry(name.clone()).or_insert_with(|| actual.clone());
        }
        (
            InferType::Applied {
                name: expected,
                args: expected_args,
            },
            InferType::Applied {
                name: found,
                args: found_args,
            },
        ) if expected == found && expected_args.len() == found_args.len() => {
            for (expected, found) in expected_args.iter().zip(found_args) {
                bind_pattern_params(expected, found, out);
            }
        }
        (InferType::Option(expected), InferType::Option(found))
        | (InferType::Array(expected), InferType::Array(found))
        | (InferType::Vec(expected), InferType::Vec(found)) => {
            bind_pattern_params(expected, found, out);
        }
        (InferType::FixedArray(expected, _), InferType::FixedArray(found, _)) => {
            bind_pattern_params(expected, found, out);
        }
        (InferType::Result(expected_ok, expected_err), InferType::Result(found_ok, found_err)) => {
            bind_pattern_params(expected_ok, found_ok, out);
            bind_pattern_params(expected_err, found_err, out);
        }
        (InferType::Tuple(expected), InferType::Tuple(found)) if expected.len() == found.len() => {
            for (expected, found) in expected.iter().zip(found) {
                bind_pattern_params(expected, found, out);
            }
        }
        (
            InferType::Function {
                params: expected_params,
                ret: expected_ret,
            },
            InferType::Function {
                params: found_params,
                ret: found_ret,
            },
        ) if expected_params.len() == found_params.len() => {
            for (expected, found) in expected_params.iter().zip(found_params) {
                bind_pattern_params(expected, found, out);
            }
            bind_pattern_params(expected_ret, found_ret, out);
        }
        (
            InferType::Projection {
                self_ty: expected, ..
            },
            InferType::Projection { self_ty: found, .. },
        ) => bind_pattern_params(expected, found, out),
        _ => {}
    }
}

fn type_matches(pattern: &InferType, actual: &InferType) -> bool {
    match (pattern, actual) {
        (InferType::Param(_), _) | (InferType::Var(_), _) => true,
        (InferType::Struct(expected), InferType::Struct(found)) => expected == found,
        (
            InferType::Applied {
                name: expected,
                args: expected_args,
            },
            InferType::Applied {
                name: found,
                args: found_args,
            },
        ) => {
            expected == found
                && expected_args.len() == found_args.len()
                && expected_args
                    .iter()
                    .zip(found_args)
                    .all(|(expected, found)| type_matches(expected, found))
        }
        (InferType::Option(expected), InferType::Option(found))
        | (InferType::Array(expected), InferType::Array(found))
        | (InferType::Vec(expected), InferType::Vec(found)) => type_matches(expected, found),
        (
            InferType::Projection {
                item: expected_item,
                self_ty: expected_self,
                ..
            },
            InferType::Projection {
                item: found_item,
                self_ty: found_self,
                ..
            },
        ) => expected_item == found_item && type_matches(expected_self, found_self),
        (InferType::Result(expected_ok, expected_err), InferType::Result(found_ok, found_err)) => {
            type_matches(expected_ok, found_ok) && type_matches(expected_err, found_err)
        }
        (
            InferType::FixedArray(expected, expected_len),
            InferType::FixedArray(found, found_len),
        ) => expected_len == found_len && type_matches(expected, found),
        (InferType::Tuple(expected), InferType::Tuple(found)) => {
            expected.len() == found.len()
                && expected
                    .iter()
                    .zip(found)
                    .all(|(expected, found)| type_matches(expected, found))
        }
        (
            InferType::Function {
                params: expected_params,
                ret: expected_ret,
            },
            InferType::Function {
                params: found_params,
                ret: found_ret,
            },
        ) => {
            expected_params.len() == found_params.len()
                && expected_params
                    .iter()
                    .zip(found_params)
                    .all(|(expected, found)| type_matches(expected, found))
                && type_matches(expected_ret, found_ret)
        }
        _ => pattern == actual,
    }
}

pub(crate) fn headers_unify(left: &InferType, right: &InferType) -> bool {
    types_overlap(left, right)
}

/// two headers overlap when **one** type satisfies both. that is a unification, not
fn types_overlap(left: &InferType, right: &InferType) -> bool {
    headers_overlap(left, &[], right, &[])
}

/// a header is its target **and** its trait arguments, and the overlap question is
fn headers_overlap(
    left: &InferType,
    left_args: &[InferType],
    right: &InferType,
    right_args: &[InferType],
) -> bool {
    if left_args.len() != right_args.len() {
        return false;
    }
    let mut bindings = std::collections::HashMap::new();
    if !unify_headers(
        &tag_params(left, "l:"),
        &tag_params(right, "r:"),
        &mut bindings,
    ) {
        return false;
    }
    left_args.iter().zip(right_args).all(|(left, right)| {
        unify_headers(
            &tag_params(left, "l:"),
            &tag_params(right, "r:"),
            &mut bindings,
        )
    })
}

/// the two headers name their parameters independently, so one side is renamed
fn tag_params(ty: &InferType, prefix: &str) -> InferType {
    match ty {
        InferType::Param(name) => InferType::Param(format!("{prefix}{name}")),
        InferType::Applied { name, args } => InferType::Applied {
            name: name.clone(),
            args: args.iter().map(|arg| tag_params(arg, prefix)).collect(),
        },
        InferType::Option(inner) => InferType::Option(Box::new(tag_params(inner, prefix))),
        InferType::Array(inner) => InferType::Array(Box::new(tag_params(inner, prefix))),
        InferType::FixedArray(inner, length) => {
            InferType::FixedArray(Box::new(tag_params(inner, prefix)), *length)
        }
        InferType::Vec(inner) => InferType::Vec(Box::new(tag_params(inner, prefix))),
        InferType::Result(ok, err) => InferType::Result(
            Box::new(tag_params(ok, prefix)),
            Box::new(tag_params(err, prefix)),
        ),
        InferType::Tuple(elements) => {
            InferType::Tuple(elements.iter().map(|e| tag_params(e, prefix)).collect())
        }
        InferType::Function { params, ret } => InferType::Function {
            params: params.iter().map(|p| tag_params(p, prefix)).collect(),
            ret: Box::new(tag_params(ret, prefix)),
        },
        other => other.clone(),
    }
}

/// a type with every bound parameter replaced by what it is bound to, through chains of bindings
fn settle_bindings(
    ty: &InferType,
    bindings: &std::collections::HashMap<String, InferType>,
) -> InferType {
    let mut current = ty.clone();
    for _ in 0..=bindings.len() {
        let next = current.substitute_params(bindings);
        if next == current {
            break;
        }
        current = next;
    }
    current
}

fn resolved_binding(
    ty: &InferType,
    bindings: &std::collections::HashMap<String, InferType>,
) -> InferType {
    let mut current = ty.clone();
    while let InferType::Param(name) = &current {
        match bindings.get(name) {
            Some(bound) if *bound != current => current = bound.clone(),
            _ => break,
        }
    }
    current
}

/// a parameter cannot stand for a type that contains it: the answer would be an
fn occurs_in(
    name: &str,
    ty: &InferType,
    bindings: &std::collections::HashMap<String, InferType>,
) -> bool {
    match resolved_binding(ty, bindings) {
        InferType::Param(found) => found == name,
        InferType::Applied { args, .. } => args.iter().any(|arg| occurs_in(name, arg, bindings)),
        InferType::Option(inner)
        | InferType::Array(inner)
        | InferType::FixedArray(inner, _)
        | InferType::Vec(inner) => occurs_in(name, &inner, bindings),
        InferType::Result(ok, err) => {
            occurs_in(name, &ok, bindings) || occurs_in(name, &err, bindings)
        }
        InferType::Tuple(elements) => elements.iter().any(|e| occurs_in(name, e, bindings)),
        InferType::Function { params, ret } => {
            params.iter().any(|p| occurs_in(name, p, bindings)) || occurs_in(name, &ret, bindings)
        }
        _ => false,
    }
}

fn unify_headers(
    left: &InferType,
    right: &InferType,
    bindings: &mut std::collections::HashMap<String, InferType>,
) -> bool {
    let left = resolved_binding(left, bindings);
    let right = resolved_binding(right, bindings);
    match (&left, &right) {
        (InferType::Var(_), _) | (_, InferType::Var(_)) => true,
        (InferType::Param(name), other) | (other, InferType::Param(name)) => {
            if matches!(other, InferType::Param(found) if found == name) {
                return true;
            }
            if occurs_in(name, other, bindings) {
                return false;
            }
            bindings.insert(name.clone(), other.clone());
            true
        }
        (InferType::Struct(left), InferType::Struct(right)) => left == right,
        (
            InferType::Applied {
                name: left_name,
                args: left_args,
            },
            InferType::Applied {
                name: right_name,
                args: right_args,
            },
        ) => {
            left_name == right_name
                && left_args.len() == right_args.len()
                && left_args
                    .iter()
                    .zip(right_args)
                    .all(|(left, right)| unify_headers(left, right, bindings))
        }
        (InferType::Option(left), InferType::Option(right))
        | (InferType::Array(left), InferType::Array(right))
        | (InferType::Vec(left), InferType::Vec(right)) => unify_headers(left, right, bindings),
        (InferType::FixedArray(left, left_len), InferType::FixedArray(right, right_len)) => {
            left_len == right_len && unify_headers(left, right, bindings)
        }
        (InferType::Result(left_ok, left_err), InferType::Result(right_ok, right_err)) => {
            unify_headers(left_ok, right_ok, bindings)
                && unify_headers(left_err, right_err, bindings)
        }
        (InferType::Tuple(left), InferType::Tuple(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| unify_headers(left, right, bindings))
        }
        (
            InferType::Function {
                params: left_params,
                ret: left_ret,
            },
            InferType::Function {
                params: right_params,
                ret: right_ret,
            },
        ) => {
            left_params.len() == right_params.len()
                && left_params
                    .iter()
                    .zip(right_params)
                    .all(|(left, right)| unify_headers(left, right, bindings))
                && unify_headers(left_ret, right_ret, bindings)
        }
        _ => left == right,
    }
}

pub(crate) fn nominal_name(ty: &InferType) -> Option<String> {
    match ty {
        InferType::Struct(name) | InferType::Applied { name, .. } => Some(name.clone()),
        _ => None,
    }
}

pub(crate) fn trait_instantiation_spelling(trait_name: &str, trait_args: &[InferType]) -> String {
    if trait_args.is_empty() {
        return trait_name.to_string();
    }
    let args: Vec<String> = trait_args.iter().map(InferType::source_spelling).collect();
    format!("{trait_name}<{}>", args.join(", "))
}

/// each parameter of a header, mapped to the first position of the receiver it stands at, spelled so that no source can write it
fn receiver_positions(ty: &InferType, path: &mut Vec<usize>, out: &mut HashMap<String, InferType>) {
    let mut visit = |index: usize, inner: &InferType, out: &mut HashMap<String, InferType>| {
        path.push(index);
        receiver_positions(inner, path, out);
        path.pop();
    };
    match ty {
        InferType::Param(name) => {
            let spelled: Vec<String> = path.iter().map(usize::to_string).collect();
            out.entry(name.clone())
                .or_insert_with(|| InferType::Param(format!("@{}", spelled.join("."))));
        }
        InferType::Applied { args, .. } | InferType::Tuple(args) => {
            for (index, arg) in args.iter().enumerate() {
                visit(index, arg, out);
            }
        }
        InferType::Vec(inner)
        | InferType::Array(inner)
        | InferType::Option(inner)
        | InferType::FixedArray(inner, _) => visit(0, inner, out),
        InferType::Result(ok, err) => {
            visit(0, ok, out);
            visit(1, err, out);
        }
        InferType::Function { params, ret } => {
            for (index, param) in params.iter().enumerate() {
                visit(index, param, out);
            }
            visit(params.len(), ret, out);
        }
        _ => {}
    }
}
