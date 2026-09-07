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
}

#[derive(Debug, Clone)]
pub struct TraitMethod {
    pub name: String,
    pub symbol: String,
    pub params: Vec<InferType>,
    pub return_type: InferType,
    pub has_self: bool,
    pub mutable_self: bool,
    pub has_body: bool,
}

#[derive(Debug, Clone)]
pub struct TraitDef {
    pub name: String,
    pub owner: ModuleId,
    pub type_params: Vec<String>,
    pub super_bounds: Vec<String>,
    pub methods: Vec<TraitMethod>,
    pub associated_types: Vec<String>,
    pub associated_consts: Vec<(String, InferType)>,
}

#[derive(Debug, Clone)]
pub struct TraitImplDef {
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoundSelection {
    CompilerRule,
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

    pub fn enum_definition_ordinal(&self, name: &str) -> Option<u32> {
        self.enum_definition_ordinals.get(name).copied()
    }

    pub fn struct_definition_ordinal(&self, name: &str) -> Option<u32> {
        self.struct_definition_ordinals.get(name).copied()
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

    // has not resolved yet cannot be mistaken for a competing one.
    pub fn trait_impl_overlaps_among(
        &self,
        indices: &[usize],
        trait_name: &str,
        trait_args: &[InferType],
        self_type: &InferType,
    ) -> bool {
        indices
            .iter()
            .filter_map(|index| self.trait_impl_defs.get(*index))
            .any(|definition| {
                definition.trait_name == trait_name
                    && definition.trait_args.len() == trait_args.len()
                    && definition
                        .trait_args
                        .iter()
                        .zip(trait_args)
                        .all(|(left, right)| types_overlap(left, right))
                    && types_overlap(&definition.self_type, self_type)
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
                queue.extend(definition.super_bounds.iter().cloned());
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
            queue.extend(definition.super_bounds.iter().cloned());
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
        if !self_ty.is_concrete() {
            return None;
        }
        let closure = trait_name
            .as_deref()
            .map(|name| self.supertrait_closure(name));
        let mut found = Vec::new();
        for implementation in &self.trait_impl_defs {
            if closure
                .as_deref()
                .is_some_and(|names| !names.contains(&implementation.trait_name))
            {
                continue;
            }
            if !type_matches(&implementation.self_type, self_ty)
                && !type_matches(self_ty, &implementation.self_type)
            {
                continue;
            }
            if let Some((_, ty)) = implementation
                .associated_types
                .iter()
                .find(|(name, _)| name == item)
            {
                found.push(instantiate_impl_definition(
                    &implementation.self_type,
                    self_ty,
                    ty,
                ));
            }
        }
        if found.len() == 1 {
            Some(found.swap_remove(0))
        } else {
            None
        }
    }

    pub fn trait_impl_defs(&self) -> &[TraitImplDef] {
        &self.trait_impl_defs
    }

    pub fn trait_method_candidates(&self, receiver: &InferType, name: &str) -> Vec<TraitMethod> {
        self.trait_impl_defs
            .iter()
            .filter(|implementation| type_matches(&implementation.self_type, receiver))
            .flat_map(|implementation| {
                implementation
                    .methods
                    .iter()
                    .filter(move |method| method.name == name)
                    .cloned()
            })
            .collect()
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
        if crate::prelude::provides(trait_name, ty, trait_args) {
            return true;
        }
        nominal_name(ty)
            .is_some_and(|target| self.has_trait_impl_with_args(trait_name, &target, trait_args))
    }

    pub fn select_bound_method(
        &self,
        trait_name: &str,
        ty: &InferType,
        method_name: &str,
    ) -> BoundSelection {
        if crate::prelude::provides(trait_name, ty, &[]) {
            return BoundSelection::CompilerRule;
        }
        let mut symbols = Vec::new();
        if let Some(target) = nominal_name(ty) {
            for definition in &self.trait_impl_defs {
                if definition.trait_name != trait_name
                    || nominal_name(&definition.self_type).as_deref() != Some(target.as_str())
                {
                    continue;
                }
                for method in &definition.methods {
                    if method.name == method_name && !symbols.contains(&method.symbol) {
                        symbols.push(method.symbol.clone());
                    }
                }
            }
        }
        match symbols.len() {
            0 => BoundSelection::Missing,
            1 => BoundSelection::Selected(symbols.swap_remove(0)),
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
        if crate::prelude::provides(
            crate::prelude::FROM_TRAIT,
            target,
            std::slice::from_ref(source),
        ) {
            return FromSelection::Identity;
        }
        let mut symbols = Vec::new();
        for definition in self.matching_from_impls(target) {
            if !type_matches(&definition.trait_args[0], source) {
                continue;
            }
            for method in &definition.methods {
                if method.name == crate::prelude::FROM_METHOD
                    && !method.symbol.is_empty()
                    && !symbols.contains(&method.symbol)
                {
                    symbols.push(method.symbol.clone());
                }
            }
        }
        if symbols.len() == 1 {
            return FromSelection::Selected(symbols.swap_remove(0));
        }
        FromSelection::Unresolved(self.conversion_headers_for(target))
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
        _ => pattern == actual,
    }
}

pub(crate) fn headers_unify(left: &InferType, right: &InferType) -> bool {
    types_overlap(left, right)
}

fn types_overlap(left: &InferType, right: &InferType) -> bool {
    match (left, right) {
        (InferType::Param(_), _) | (InferType::Var(_), _) => true,
        (_, InferType::Param(_)) | (_, InferType::Var(_)) => true,
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
                    .all(|(left, right)| types_overlap(left, right))
        }
        (InferType::Option(left), InferType::Option(right))
        | (InferType::Array(left), InferType::Array(right))
        | (InferType::Vec(left), InferType::Vec(right)) => types_overlap(left, right),
        (InferType::Result(left_ok, left_err), InferType::Result(right_ok, right_err)) => {
            types_overlap(left_ok, right_ok) && types_overlap(left_err, right_err)
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
