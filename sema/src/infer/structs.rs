use super::TypeInference;
use crate::constraint::{ConstraintReason, TypeError, TypeErrorKind};
use crate::types::{EnumDef, EnumVariantDef, EnumVariantFieldsDef, StructDef, StructField};
use aelys_syntax::{Stmt, StmtKind};
use std::collections::HashSet;

const MAX_ENUM_LAYOUT_ENTRIES: usize = u16::MAX as usize;

impl TypeInference {
    pub(super) fn collect_enums(&mut self, stmts: &[Stmt]) {
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
                    kind: TypeErrorKind::DuplicateStruct { name: name.clone() },
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
                            .map(|field| self.type_from_annotation(field))
                            .collect(),
                    ),
                    aelys_syntax::EnumVariantFields::Named(fields) => EnumVariantFieldsDef::Named(
                        fields
                            .iter()
                            .enumerate()
                            .map(|(ordinal, field)| StructField {
                                name: field.name.clone(),
                                ty: self.type_from_annotation(&field.type_annotation),
                                is_pub: field.is_pub,
                                ordinal: u16::try_from(ordinal).unwrap_or(u16::MAX),
                            })
                            .collect(),
                    ),
                };
                typed_variants.push(EnumVariantDef {
                    name: variant.name.clone(),
                    fields,
                });
            }
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

    pub(super) fn collect_structs(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            if let StmtKind::StructDecl {
                name,
                type_params,
                fields,
                is_pub,
            } = &stmt.kind
            {
                if self.type_table.has_nominal(name) {
                    self.errors.push(TypeError {
                        kind: TypeErrorKind::DuplicateStruct { name: name.clone() },
                        span: stmt.span,
                        reason: ConstraintReason::Other("struct declaration".to_string()),
                    });
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
                let struct_fields: Vec<StructField> = fields
                    .iter()
                    .enumerate()
                    .map(|(ordinal, f)| StructField {
                        name: f.name.clone(),
                        ty: self.type_from_annotation(&f.type_annotation),
                        is_pub: f.is_pub,
                        ordinal: u16::try_from(ordinal).unwrap_or(u16::MAX),
                    })
                    .collect();
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
}
