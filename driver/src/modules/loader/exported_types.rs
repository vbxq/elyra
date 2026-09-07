use aelys_common::Result;
use aelys_common::error::{AelysError, CompileError, CompileErrorKind};
use aelys_sema::infer::imports::ImportedTypes;
use aelys_sema::types::TypeTable;
use aelys_syntax::{Source, Span, Stmt, StmtKind};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::Arc;

#[derive(Debug, Clone, Default)]
pub struct ExportedTypes {
    pub types: ImportedTypes,
    pub impl_stmts: Vec<Stmt>,
    pub private_names: HashSet<String>,
    pub globals: BTreeMap<String, aelys_sema::types::InferType>,
    pub scoped_globals: BTreeSet<String>,
    pub source: Option<Arc<Source>>,
    pub private_types: ImportedTypes,
    pub private_impls: Vec<Stmt>,
    pub private_origins: BTreeMap<String, String>,
    pub private_module_sources: std::collections::HashMap<String, Arc<Source>>,
    pub own_private_types: ImportedTypes,
    pub own_private_impls: Vec<Stmt>,
}

#[derive(Debug, Clone, Copy)]
pub enum NominalScope<'a> {
    All,
    Only(&'a [String]),
}

pub struct NominalScopeEntry {
    pub module_path: String,
    pub wanted: Option<Vec<String>>,
    pub span: Span,
}

impl NominalScopeEntry {
    pub fn scope(&self) -> NominalScope<'_> {
        match &self.wanted {
            Some(names) => NominalScope::Only(names),
            None => NominalScope::All,
        }
    }
}

pub fn widen_nominal_scope(
    scopes: &mut Vec<NominalScopeEntry>,
    module_path: &str,
    wanted: Option<Vec<String>>,
    span: Span,
) {
    let Some(entry) = scopes
        .iter_mut()
        .find(|entry| entry.module_path == module_path)
    else {
        scopes.push(NominalScopeEntry {
            module_path: module_path.to_string(),
            wanted,
            span,
        });
        return;
    };
    match (entry.wanted.as_mut(), wanted) {
        (Some(names), Some(more)) => names.extend(more),
        (_, None) => entry.wanted = None,
        (None, Some(_)) => {}
    }
}

fn impl_ends(stmt: &Stmt) -> Option<(String, Option<String>)> {
    let StmtKind::ImplDecl {
        trait_path,
        self_type,
        ..
    } = &stmt.kind
    else {
        return None;
    };
    let target = self_type
        .path
        .last()
        .cloned()
        .unwrap_or_else(|| self_type.name.clone());
    let trait_name = trait_path
        .as_ref()
        .and_then(|path| path.path.last().cloned());
    Some((target, trait_name))
}

/// every word, so a name the carried text mentions is never missed; a word that
fn collect_identifiers(text: &str, out: &mut HashSet<String>) -> bool {
    let mut grew = false;
    let mut word = String::new();
    for ch in text.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            word.push(ch);
            continue;
        }
        if !word.is_empty() {
            grew |= out.insert(std::mem::take(&mut word));
        }
    }
    if !word.is_empty() {
        grew |= out.insert(word);
    }
    grew
}

fn field_types(def: &aelys_sema::types::EnumVariantFieldsDef) -> Vec<String> {
    match def {
        aelys_sema::types::EnumVariantFieldsDef::Unit => Vec::new(),
        aelys_sema::types::EnumVariantFieldsDef::Tuple(types) => {
            types.iter().map(ToString::to_string).collect()
        }
        aelys_sema::types::EnumVariantFieldsDef::Named(fields) => {
            fields.iter().map(|field| field.ty.to_string()).collect()
        }
    }
}

fn names_the_impls_reach(exported: &ExportedTypes, impls: &[Stmt]) -> HashSet<String> {
    let mut reached = HashSet::new();
    let Some(source) = exported.source.as_ref() else {
        return reached;
    };
    for stmt in impls {
        if let Some(text) = source.content.get(stmt.span.start..stmt.span.end) {
            collect_identifiers(text, &mut reached);
        }
    }
    loop {
        let mut grew = false;
        for def in &exported.types.structs {
            if !reached.contains(&def.name) {
                continue;
            }
            for field in &def.fields {
                grew |= collect_identifiers(&field.ty.to_string(), &mut reached);
            }
        }
        for def in &exported.types.enums {
            if !reached.contains(&def.name) {
                continue;
            }
            for variant in &def.variants {
                for ty in field_types(&variant.fields) {
                    grew |= collect_identifiers(&ty, &mut reached);
                }
            }
        }
        if !grew {
            return reached;
        }
    }
}

pub fn select_exported_nominals(
    exported: &ExportedTypes,
    module_path: &str,
    scope: NominalScope<'_>,
) -> (ImportedTypes, Vec<Stmt>) {
    let names = match scope {
        NominalScope::All => return (exported.types.clone(), exported.impl_stmts.clone()),
        NominalScope::Only(names) => names,
    };
    let wanted: HashSet<&str> = names.iter().map(String::as_str).collect();
    let exported_names: HashSet<String> = exported.types.nominal_names().into_iter().collect();

    let in_scope = |name: &String| !exported_names.contains(name) || wanted.contains(name.as_str());
    let impls: Vec<Stmt> = exported
        .impl_stmts
        .iter()
        .filter(|stmt| match impl_ends(stmt) {
            Some((target, trait_name)) => {
                in_scope(&target) && trait_name.as_ref().is_none_or(in_scope)
            }
            None => false,
        })
        .cloned()
        .collect();
    let reached = names_the_impls_reach(exported, &impls);

    let mut selected = ImportedTypes::default();
    let surface_path = module_path.replace('.', "::");
    let place = |selected: &mut ImportedTypes, name: &str| -> Placement {
        if wanted.contains(name) {
            return Placement::Named;
        }
        if reached.contains(name) {
            selected
                .private_nominals
                .insert(name.to_string(), surface_path.clone());
            return Placement::Private;
        }
        selected
            .withheld
            .insert(name.to_string(), surface_path.clone());
        Placement::Withheld
    };
    for def in &exported.types.enums {
        if place(&mut selected, &def.name) != Placement::Withheld {
            selected.enums.push(def.clone());
        }
    }
    for def in &exported.types.structs {
        if place(&mut selected, &def.name) != Placement::Withheld {
            selected.structs.push(def.clone());
        }
    }
    for def in &exported.types.traits {
        if place(&mut selected, &def.name) != Placement::Withheld {
            selected.traits.push(def.clone());
        }
    }

    (selected, impls)
}

#[derive(PartialEq, Eq)]
enum Placement {
    Named,
    Private,
    Withheld,
}

fn not_exportable(
    module_path: &str,
    name: &str,
    reason: &str,
    span: Span,
    source: Arc<Source>,
) -> AelysError {
    AelysError::Compile(CompileError::new(
        CompileErrorKind::TypeNotExportable {
            module: module_path.to_string(),
            name: name.to_string(),
            reason: reason.to_string(),
        },
        span,
        source,
    ))
}

const GENERIC_REASON: &str =
    "a generic declaration is erased by monomorphization before the module boundary";

const MISSING_REASON: &str = "the type checker produced no definition for it";

pub fn collect_exported_types(
    stmts: &[Stmt],
    type_table: &TypeTable,
    module_path: &str,
    source: Arc<Source>,
) -> Result<ExportedTypes> {
    let mut exported = ExportedTypes::default();
    let mut private_generics: BTreeMap<String, Span> = BTreeMap::new();

    for stmt in stmts {
        let (name, type_params, is_pub) = match &stmt.kind {
            StmtKind::EnumDecl {
                name,
                type_params,
                is_pub,
                ..
            }
            | StmtKind::StructDecl {
                name,
                type_params,
                is_pub,
                ..
            }
            | StmtKind::TraitDecl {
                name,
                type_params,
                is_pub,
                ..
            } => (name, type_params, *is_pub),
            _ => continue,
        };

        if !is_pub {
            exported.private_names.insert(name.clone());
            if !type_params.is_empty() {
                // monomorphization erased it in this module, so it has no definition to send
                private_generics.insert(name.clone(), stmt.span);
                continue;
            }
            match &stmt.kind {
                StmtKind::EnumDecl { .. } => {
                    if let Some(def) = type_table.get_enum(name) {
                        exported.own_private_types.enums.push(def.clone());
                    }
                }
                StmtKind::StructDecl { .. } => {
                    if let Some(def) = type_table.get_struct(name) {
                        exported.own_private_types.structs.push(def.clone());
                    }
                }
                _ => {
                    if let Some(def) = type_table.get_trait(name) {
                        exported.own_private_types.traits.push(def.clone());
                    }
                }
            }
            continue;
        }
        if !type_params.is_empty() {
            return Err(not_exportable(
                module_path,
                name,
                GENERIC_REASON,
                stmt.span,
                source,
            ));
        }

        let found = match &stmt.kind {
            StmtKind::EnumDecl { .. } => type_table
                .get_enum(name)
                .map(|def| exported.types.enums.push(def.clone())),
            StmtKind::StructDecl { .. } => type_table
                .get_struct(name)
                .map(|def| exported.types.structs.push(def.clone())),
            _ => type_table
                .get_trait(name)
                .map(|def| exported.types.traits.push(def.clone())),
        };
        if found.is_none() {
            return Err(not_exportable(
                module_path,
                name,
                MISSING_REASON,
                stmt.span,
                source,
            ));
        }
    }

    for stmt in stmts {
        let StmtKind::ImplDecl {
            trait_path,
            self_type,
            ..
        } = &stmt.kind
        else {
            continue;
        };
        let target = self_type
            .path
            .last()
            .cloned()
            .unwrap_or_else(|| self_type.name.clone());
        let trait_name = trait_path.as_ref().map(|path| path.path.join("::"));
        if exported.private_names.contains(&target)
            || trait_name
                .as_ref()
                .is_some_and(|name| exported.private_names.contains(name))
        {
            exported.own_private_impls.push(stmt.clone());
            continue;
        }
        exported.impl_stmts.push(stmt.clone());
    }

    // a body that travels reaches the module's private declarations, and a generic one has
    if !private_generics.is_empty() {
        let mut named = HashSet::new();
        for stmt in exported
            .impl_stmts
            .iter()
            .chain(&exported.own_private_impls)
        {
            if let Some(text) = source.content.get(stmt.span.start..stmt.span.end) {
                collect_identifiers(text, &mut named);
            }
        }
        for (name, span) in private_generics {
            if named.contains(&name) {
                return Err(not_exportable(
                    module_path,
                    &name,
                    GENERIC_REASON,
                    span,
                    source,
                ));
            }
        }
    }

    Ok(exported)
}

const BOUNDARY_REASON: &str =
    "a private nominal declaration cannot cross the module boundary; make the declaration public";

fn mentions_nominal(ty: &aelys_sema::InferType, type_table: &TypeTable) -> Option<String> {
    use aelys_sema::InferType;
    match ty {
        InferType::Struct(name) => type_table
            .get_struct(name)
            .is_some_and(|def| !def.is_pub)
            .then(|| name.clone())
            .or_else(|| {
                type_table
                    .get_enum(name)
                    .is_some_and(|def| !def.is_pub)
                    .then(|| name.clone())
            }),
        InferType::Applied { name, args } => {
            let private_base = type_table.get_struct(name).is_some_and(|def| !def.is_pub)
                || type_table.get_enum(name).is_some_and(|def| !def.is_pub);
            private_base.then(|| name.clone()).or_else(|| {
                args.iter()
                    .find_map(|arg| mentions_nominal(arg, type_table))
            })
        }
        InferType::Option(inner)
        | InferType::Array(inner)
        | InferType::FixedArray(inner, _)
        | InferType::Vec(inner) => mentions_nominal(inner, type_table),
        InferType::Result(ok, err) => {
            mentions_nominal(ok, type_table).or_else(|| mentions_nominal(err, type_table))
        }
        InferType::Tuple(elements) => elements
            .iter()
            .find_map(|element| mentions_nominal(element, type_table)),
        InferType::Function { params, ret } => params
            .iter()
            .find_map(|param| mentions_nominal(param, type_table))
            .or_else(|| mentions_nominal(ret, type_table)),
        _ => None,
    }
}

pub fn reject_nominal_boundary_signatures(
    stmts: &[aelys_sema::TypedStmt],
    type_table: &TypeTable,
    module_path: &str,
    source: &Arc<Source>,
) -> Result<()> {
    use aelys_sema::TypedStmtKind;
    for stmt in stmts {
        let found = match &stmt.kind {
            TypedStmtKind::Function(function) if function.is_pub => function
                .params
                .iter()
                .find_map(|param| mentions_nominal(&param.ty, type_table))
                .or_else(|| mentions_nominal(&function.return_type, type_table))
                .map(|nominal| (function.name.clone(), nominal)),
            TypedStmtKind::Let {
                name,
                is_pub: true,
                var_type,
                ..
            } => mentions_nominal(var_type, type_table).map(|nominal| (name.clone(), nominal)),
            _ => None,
        };
        if let Some((_, nominal)) = found {
            return Err(not_exportable(
                module_path,
                &nominal,
                BOUNDARY_REASON,
                stmt.span,
                source.clone(),
            ));
        }
    }
    Ok(())
}
