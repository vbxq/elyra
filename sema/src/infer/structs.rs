use super::TypeInference;
use crate::constraint::{ConstraintReason, TypeError, TypeErrorKind};
use crate::types::{
    EnumDef, EnumVariantDef, EnumVariantFieldsDef, InferType, StructDef, StructField,
};
use aelys_syntax::{Span, Stmt, StmtKind};
use std::collections::{HashMap, HashSet};

const MAX_ENUM_LAYOUT_ENTRIES: usize = u16::MAX as usize;

pub(super) struct DeclaredNominals {
    enums: HashSet<String>,
    structs: HashSet<String>,
}

impl TypeInference {
    pub(super) fn declare_nominals(&mut self, stmts: &[Stmt]) -> DeclaredNominals {
        DeclaredNominals {
            enums: self.declare_enums(stmts),
            structs: self.declare_structs(stmts),
        }
    }

    pub(super) fn resolve_nominal_bodies(&mut self, stmts: &[Stmt], declared: DeclaredNominals) {
        let DeclaredNominals { enums, structs } = declared;
        self.resolve_enum_bodies(stmts, enums);
        self.resolve_struct_bodies(stmts, structs);
    }

    fn declare_enums(&mut self, stmts: &[Stmt]) -> HashSet<String> {
        let enum_count = stmts
            .iter()
            .filter(|stmt| matches!(&stmt.kind, StmtKind::EnumDecl { .. }))
            .count();
        if enum_count > MAX_ENUM_LAYOUT_ENTRIES
            && let Some(stmt) = stmts
                .iter()
                .find(|stmt| matches!(&stmt.kind, StmtKind::EnumDecl { .. }))
        {
            let enum_name = match &stmt.kind {
                StmtKind::EnumDecl { name, .. } => name.clone(),
                _ => unreachable!(),
            };
            self.errors.push(TypeError {
                kind: TypeErrorKind::EnumLayoutTooLarge {
                    enum_name,
                    item: "schema table".to_string(),
                    count: enum_count,
                    limit: MAX_ENUM_LAYOUT_ENTRIES,
                },
                span: stmt.span,
                reason: ConstraintReason::Other("enum schema table".to_string()),
            });
        }
        let mut seen = HashSet::new();
        let mut accepted = HashSet::new();
        for stmt in stmts {
            let StmtKind::EnumDecl {
                name,
                type_params,
                is_pub,
                ..
            } = &stmt.kind
            else {
                continue;
            };

            if self.type_table.has_nominal(name) || !seen.insert(name.clone()) {
                self.errors.push(TypeError {
                    kind: TypeErrorKind::DuplicateNominal {
                        name: name.clone(),
                        keyword: "enum",
                        collides_with: self
                            .type_table
                            .nominal_keyword(name)
                            .filter(|kind| *kind != "enum"),
                    },
                    span: stmt.span,
                    reason: ConstraintReason::Other("enum declaration".to_string()),
                });
                continue;
            }

            self.type_table.register_enum(EnumDef {
                name: name.clone(),
                type_params: type_params.clone(),
                variants: Vec::new(),
                owner: self.current_module.clone(),
                is_pub: *is_pub,
            });
            accepted.insert(name.clone());
        }
        accepted
    }

    fn resolve_enum_bodies(&mut self, stmts: &[Stmt], mut accepted: HashSet<String>) {
        for stmt in stmts {
            let StmtKind::EnumDecl {
                name,
                type_params,
                variants,
                is_pub,
                ..
            } = &stmt.kind
            else {
                continue;
            };
            if !accepted.remove(name) {
                continue;
            }

            if variants.len() > MAX_ENUM_LAYOUT_ENTRIES {
                self.errors.push(TypeError {
                    kind: TypeErrorKind::EnumLayoutTooLarge {
                        enum_name: name.clone(),
                        item: "variants".to_string(),
                        count: variants.len(),
                        limit: MAX_ENUM_LAYOUT_ENTRIES,
                    },
                    span: stmt.span,
                    reason: ConstraintReason::Other("enum schema layout".to_string()),
                });
            }

            let saved_type_params =
                std::mem::replace(&mut self.type_params_in_scope, type_params.clone());
            let saved_nominal_scope = std::mem::replace(&mut self.nominal_parameter_scope, true);
            let mut typed_variants = Vec::with_capacity(variants.len());
            for variant in variants {
                let field_count = match &variant.fields {
                    aelys_syntax::EnumVariantFields::Unit => 0,
                    aelys_syntax::EnumVariantFields::Tuple(fields) => fields.len(),
                    aelys_syntax::EnumVariantFields::Named(fields) => fields.len(),
                };
                if field_count > MAX_ENUM_LAYOUT_ENTRIES {
                    self.errors.push(TypeError {
                        kind: TypeErrorKind::EnumLayoutTooLarge {
                            enum_name: name.clone(),
                            item: format!("variant '{}' payload", variant.name),
                            count: field_count,
                            limit: MAX_ENUM_LAYOUT_ENTRIES,
                        },
                        span: variant.span,
                        reason: ConstraintReason::Other("enum schema layout".to_string()),
                    });
                }
                let fields = match &variant.fields {
                    aelys_syntax::EnumVariantFields::Unit => EnumVariantFieldsDef::Unit,
                    aelys_syntax::EnumVariantFields::Tuple(fields) => EnumVariantFieldsDef::Tuple(
                        fields
                            .iter()
                            .map(|field| {
                                self.type_from_annotation_as(
                                    crate::infer::OccurrenceRole::EnumVariantField,
                                    field,
                                )
                            })
                            .collect(),
                    ),
                    aelys_syntax::EnumVariantFields::Named(fields) => EnumVariantFieldsDef::Named(
                        fields
                            .iter()
                            .enumerate()
                            .map(|(ordinal, field)| StructField {
                                name: field.name.clone(),
                                ty: self.type_from_annotation_as(
                                    crate::infer::OccurrenceRole::EnumVariantField,
                                    &field.type_annotation,
                                ),
                                is_pub: field.is_pub,
                                ordinal: u16::try_from(ordinal).unwrap_or(u16::MAX),
                                span: field.span,
                            })
                            .collect(),
                    ),
                };
                typed_variants.push(EnumVariantDef {
                    name: variant.name.clone(),
                    fields,
                    span: variant.span,
                });
            }
            self.nominal_parameter_scope = saved_nominal_scope;
            self.type_params_in_scope = saved_type_params;
            self.type_table.register_enum(EnumDef {
                name: name.clone(),
                type_params: type_params.clone(),
                variants: typed_variants,
                owner: self.current_module.clone(),
                is_pub: *is_pub,
            });
        }
    }

    // and result each own a constructor inhabited whatever their arguments, so they
    fn inhabitation_requirement(nodes: &HashMap<&str, usize>, ty: &InferType) -> Option<usize> {
        let mut ty = ty;
        loop {
            match ty {
                InferType::FixedArray(_, 0) => return None,
                InferType::FixedArray(inner, _) => ty = inner,
                InferType::Struct(name) | InferType::Applied { name, .. } => {
                    return nodes.get(name.as_str()).copied();
                }
                _ => return None,
            }
        }
    }

    pub(super) fn validate_nominal_inhabitation(&mut self, stmts: &[Stmt]) {
        for error in self.uninhabited_nominal_cycles(stmts) {
            self.errors.push(error);
        }
    }

    // e0428 fires where the inhabitation fixpoint fails for a nominal that lies on a
    fn uninhabited_nominal_cycles(&self, stmts: &[Stmt]) -> Vec<TypeError> {
        let mut declared: Vec<(&str, &'static str)> = Vec::new();
        let mut seen: HashSet<&str> = HashSet::new();
        for stmt in stmts {
            let (name, keyword) = match &stmt.kind {
                StmtKind::StructDecl { name, .. } => (name.as_str(), "struct"),
                StmtKind::EnumDecl { name, .. } => (name.as_str(), "enum"),
                _ => continue,
            };
            if !seen.insert(name) {
                continue;
            }
            let known = match keyword {
                "struct" => self.type_table.get_struct(name).is_some(),
                _ => self.type_table.get_enum(name).is_some(),
            };
            if known {
                declared.push((name, keyword));
            }
        }
        declared.sort_unstable_by_key(|(name, _)| *name);
        let count = declared.len();
        let nodes: HashMap<&str, usize> = declared
            .iter()
            .enumerate()
            .map(|(index, (name, _))| (*name, index))
            .collect();

        // one clause per enum variant, exactly one for a struct: the node is inhabited
        let mut clauses: Vec<Vec<Vec<usize>>> = vec![Vec::new(); count];
        let mut edges: Vec<Vec<(usize, Span, String)>> = vec![Vec::new(); count];
        let mut filled = vec![false; count];
        for stmt in stmts {
            match &stmt.kind {
                StmtKind::StructDecl { name, fields, .. } => {
                    let Some(&node) = nodes.get(name.as_str()) else {
                        continue;
                    };
                    if std::mem::replace(&mut filled[node], true) {
                        continue;
                    }
                    let Some(def) = self.type_table.get_struct(name) else {
                        continue;
                    };
                    let mut clause = Vec::new();
                    for (resolved, declared) in def.fields.iter().zip(fields) {
                        let Some(target) = Self::inhabitation_requirement(&nodes, &resolved.ty)
                        else {
                            continue;
                        };
                        clause.push(target);
                        edges[node].push((target, declared.span, declared.name.clone()));
                    }
                    clauses[node] = vec![clause];
                }
                StmtKind::EnumDecl { name, variants, .. } => {
                    let Some(&node) = nodes.get(name.as_str()) else {
                        continue;
                    };
                    if std::mem::replace(&mut filled[node], true) {
                        continue;
                    }
                    let Some(def) = self.type_table.get_enum(name) else {
                        continue;
                    };
                    let mut variant_clauses = Vec::new();
                    for (resolved, declared) in def.variants.iter().zip(variants) {
                        let payloads: Vec<&InferType> = match &resolved.fields {
                            EnumVariantFieldsDef::Unit => Vec::new(),
                            EnumVariantFieldsDef::Tuple(types) => types.iter().collect(),
                            EnumVariantFieldsDef::Named(fields) => {
                                fields.iter().map(|field| &field.ty).collect()
                            }
                        };
                        let mut clause = Vec::new();
                        for payload in payloads {
                            let Some(target) = Self::inhabitation_requirement(&nodes, payload)
                            else {
                                continue;
                            };
                            clause.push(target);
                            edges[node].push((target, declared.span, declared.name.clone()));
                        }
                        variant_clauses.push(clause);
                    }
                    clauses[node] = variant_clauses;
                }
                _ => continue,
            }
        }

        let mut successors: Vec<Vec<usize>> = edges
            .iter()
            .map(|out| out.iter().map(|(target, _, _)| *target).collect())
            .collect();
        for out in &mut successors {
            out.sort_unstable();
            out.dedup();
        }

        let inhabited = Self::inhabitation_fixpoint(&clauses, &successors);
        let (component, component_size) = Self::constructor_graph_components(&successors);

        let mut errors = Vec::new();
        let mut reported: HashSet<usize> = HashSet::new();
        for node in 0..count {
            let on_cycle = component_size[component[node]] > 1 || successors[node].contains(&node);
            if inhabited[node] || !on_cycle || !reported.insert(component[node]) {
                continue;
            }
            let Some(cycle) = Self::canonical_cycle(node, &successors, &component) else {
                continue;
            };
            let Some((_, span, label)) = edges[node]
                .iter()
                .find(|(target, _, _)| *target == cycle[1])
            else {
                continue;
            };
            let (name, keyword) = declared[node];
            let reason = match keyword {
                "struct" => format!("field '{label}' of struct '{name}'"),
                _ => format!("variant '{label}' of enum '{name}'"),
            };
            errors.push(TypeError {
                kind: TypeErrorKind::UninhabitedNominalCycle {
                    keyword,
                    path: cycle
                        .iter()
                        .map(|&node| declared[node].0.to_string())
                        .collect(),
                },
                span: *span,
                reason: ConstraintReason::Other(reason),
            });
        }
        errors
    }

    // thousands of links. it terminates because a node reaches `inhabited` at most
    fn inhabitation_fixpoint(clauses: &[Vec<Vec<usize>>], successors: &[Vec<usize>]) -> Vec<bool> {
        let count = clauses.len();
        let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); count];
        for (node, out) in successors.iter().enumerate() {
            for &target in out {
                dependents[target].push(node);
            }
        }
        let mut inhabited = vec![false; count];
        let mut queued = vec![true; count];
        let mut queue: std::collections::VecDeque<usize> = (0..count).collect();
        while let Some(node) = queue.pop_front() {
            queued[node] = false;
            if inhabited[node] {
                continue;
            }
            let satisfied = clauses[node]
                .iter()
                .any(|clause| clause.iter().all(|&target| inhabited[target]));
            if !satisfied {
                continue;
            }
            inhabited[node] = true;
            for &dependent in &dependents[node] {
                if !inhabited[dependent] && !queued[dependent] {
                    queued[dependent] = true;
                    queue.push_back(dependent);
                }
            }
        }
        inhabited
    }

    fn constructor_graph_components(successors: &[Vec<usize>]) -> (Vec<usize>, Vec<usize>) {
        let count = successors.len();
        const UNVISITED: usize = usize::MAX;
        let mut index = vec![UNVISITED; count];
        let mut low = vec![0usize; count];
        let mut on_stack = vec![false; count];
        let mut component = vec![UNVISITED; count];
        let mut sizes: Vec<usize> = Vec::new();
        let mut stack: Vec<usize> = Vec::new();
        let mut next_index = 0usize;
        for root in 0..count {
            if index[root] != UNVISITED {
                continue;
            }
            index[root] = next_index;
            low[root] = next_index;
            next_index += 1;
            stack.push(root);
            on_stack[root] = true;
            let mut calls: Vec<(usize, usize)> = vec![(root, 0)];
            while let Some(&(node, offset)) = calls.last() {
                if let Some(&target) = successors[node].get(offset) {
                    calls.last_mut().expect("the frame was just read").1 += 1;
                    if index[target] == UNVISITED {
                        index[target] = next_index;
                        low[target] = next_index;
                        next_index += 1;
                        stack.push(target);
                        on_stack[target] = true;
                        calls.push((target, 0));
                    } else if on_stack[target] {
                        low[node] = low[node].min(index[target]);
                    }
                    continue;
                }
                calls.pop();
                if let Some(&(parent, _)) = calls.last() {
                    low[parent] = low[parent].min(low[node]);
                }
                if low[node] == index[node] {
                    let mut size = 0;
                    while let Some(member) = stack.pop() {
                        on_stack[member] = false;
                        component[member] = sizes.len();
                        size += 1;
                        if member == node {
                            break;
                        }
                    }
                    sizes.push(size);
                }
            }
        }
        (component, sizes)
    }

    fn canonical_cycle(
        root: usize,
        successors: &[Vec<usize>],
        component: &[usize],
    ) -> Option<Vec<usize>> {
        let mut parent: HashMap<usize, usize> = HashMap::new();
        let mut seen: HashSet<usize> = HashSet::from([root]);
        let mut queue: std::collections::VecDeque<usize> = std::collections::VecDeque::from([root]);
        let mut closer = None;
        'search: while let Some(node) = queue.pop_front() {
            for &target in &successors[node] {
                if component[target] != component[root] {
                    continue;
                }
                if target == root {
                    closer = Some(node);
                    break 'search;
                }
                if seen.insert(target) {
                    parent.insert(target, node);
                    queue.push_back(target);
                }
            }
        }
        let mut path = vec![closer?];
        while let Some(&previous) = parent.get(path.last().expect("the path is never empty")) {
            path.push(previous);
        }
        path.reverse();
        path.push(root);
        Some(path)
    }

    fn declare_structs(&mut self, stmts: &[Stmt]) -> HashSet<String> {
        let mut accepted = HashSet::new();
        for stmt in stmts {
            let StmtKind::StructDecl {
                name,
                type_params,
                is_pub,
                ..
            } = &stmt.kind
            else {
                continue;
            };

            if self.type_table.has_nominal(name) {
                self.errors.push(TypeError {
                    kind: TypeErrorKind::DuplicateNominal {
                        name: name.clone(),
                        keyword: "struct",
                        collides_with: self
                            .type_table
                            .nominal_keyword(name)
                            .filter(|kind| *kind != "struct"),
                    },
                    span: stmt.span,
                    reason: ConstraintReason::Other("struct declaration".to_string()),
                });
                continue;
            }

            self.type_table.register_struct(StructDef {
                name: name.clone(),
                type_params: type_params.clone(),
                fields: Vec::new(),
                owner: self.current_module.clone(),
                is_pub: *is_pub,
            });
            accepted.insert(name.clone());
        }
        accepted
    }

    fn resolve_struct_bodies(&mut self, stmts: &[Stmt], mut accepted: HashSet<String>) {
        for stmt in stmts {
            let StmtKind::StructDecl {
                name,
                type_params,
                fields,
                is_pub,
            } = &stmt.kind
            else {
                continue;
            };
            if !accepted.remove(name) {
                continue;
            }

            let mut field_names = HashSet::new();
            for field in fields {
                if !field_names.insert(field.name.clone()) {
                    self.errors.push(TypeError {
                        kind: TypeErrorKind::DuplicateStructField {
                            structure: name.clone(),
                            field: field.name.clone(),
                        },
                        span: field.span,
                        reason: ConstraintReason::Other("struct field declaration".to_string()),
                    });
                }
            }

            let saved_type_params =
                std::mem::replace(&mut self.type_params_in_scope, type_params.clone());
            let saved_nominal_scope = std::mem::replace(&mut self.nominal_parameter_scope, true);
            let struct_fields: Vec<StructField> = fields
                .iter()
                .enumerate()
                .map(|(ordinal, f)| StructField {
                    name: f.name.clone(),
                    ty: self.type_from_annotation_as(
                        crate::infer::OccurrenceRole::StructField,
                        &f.type_annotation,
                    ),
                    is_pub: f.is_pub,
                    ordinal: u16::try_from(ordinal).unwrap_or(u16::MAX),
                    span: f.span,
                })
                .collect();
            self.nominal_parameter_scope = saved_nominal_scope;
            self.type_params_in_scope = saved_type_params;

            self.type_table.register_struct(StructDef {
                name: name.clone(),
                type_params: type_params.clone(),
                fields: struct_fields,
                owner: self.current_module.clone(),
                is_pub: *is_pub,
            });
        }
    }
}
