use aelys_common::Result;
use aelys_common::error::{AelysError, CompileError, CompileErrorKind};
use aelys_sema::infer::imports::ImportedTypes;
use aelys_sema::types::TypeTable;
use aelys_syntax::{Source, Span, Stmt, StmtKind};
use std::collections::HashSet;
use std::sync::Arc;

#[derive(Debug, Clone, Default)]
pub struct ExportedTypes {
    pub types: ImportedTypes,
    pub impl_stmts: Vec<Stmt>,
    pub private_names: HashSet<String>,
}

#[derive(Debug, Clone, Copy)]
pub enum NominalScope<'a> {
    All,
    Only(&'a [String]),
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

/// only when every end it names is itself in scope, so importing a trait alone never smuggles in
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

    let mut selected = ImportedTypes::default();
    let surface_path = module_path.replace('.', "::");
    let withhold = |selected: &mut ImportedTypes, name: &str| {
        selected
            .withheld
            .insert(name.to_string(), surface_path.clone());
    };
    for def in &exported.types.enums {
        if wanted.contains(def.name.as_str()) {
            selected.enums.push(def.clone());
        } else {
            withhold(&mut selected, &def.name);
        }
    }
    for def in &exported.types.structs {
        if wanted.contains(def.name.as_str()) {
            selected.structs.push(def.clone());
        } else {
            withhold(&mut selected, &def.name);
        }
    }
    for def in &exported.types.traits {
        if wanted.contains(def.name.as_str()) {
            selected.traits.push(def.clone());
        } else {
            withhold(&mut selected, &def.name);
        }
    }

    let in_scope = |name: &String| !exported_names.contains(name) || wanted.contains(name.as_str());
    let impls = exported
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

    (selected, impls)
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
            continue;
        }
        exported.impl_stmts.push(stmt.clone());
    }

    Ok(exported)
}

const BOUNDARY_REASON: &str = "a value of a nominal type cannot cross a module boundary yet because a struct or enum schema id is assigned per compilation unit";

fn mentions_nominal(ty: &aelys_sema::InferType, type_table: &TypeTable) -> Option<String> {
    use aelys_sema::InferType;
    match ty {
        InferType::Struct(name) => type_table.has_nominal(name).then(|| name.clone()),
        InferType::Applied { name, args } => type_table
            .has_nominal(name)
            .then(|| name.clone())
            .or_else(|| {
                args.iter()
                    .find_map(|arg| mentions_nominal(arg, type_table))
            }),
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
