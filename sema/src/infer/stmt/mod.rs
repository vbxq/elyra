mod block;
mod implicit;
mod let_stmt;
mod loop_stmt;
mod needs;
mod return_stmt;

use super::TypeInference;
use crate::typed_ast::{TypedStmt, TypedStmtKind};
use aelys_syntax::{Stmt, StmtKind};

impl TypeInference {
    pub(super) fn infer_stmts(&mut self, stmts: &[Stmt]) -> Vec<TypedStmt> {
        stmts.iter().map(|s| self.infer_stmt(s)).collect()
    }

    pub(super) fn infer_stmt(&mut self, stmt: &Stmt) -> TypedStmt {
        let previous_module = self.current_module.clone();
        let mut definition_scope = false;
        let mut definition_mark = None;
        let mut definition_qualified: Vec<String> = Vec::new();
        if let Some(definition_module) = &stmt.definition_module {
            self.current_module = definition_module.clone();
            definition_scope = self.push_definition_module_globals(definition_module);
            definition_qualified = self.push_definition_module_imports(definition_module);
            definition_mark = Some((
                self.errors.len(),
                self.constraints.len(),
                definition_module.as_str().to_string(),
            ));
        }
        let kind = match &stmt.kind {
            StmtKind::Expression(expr) => {
                let typed_expr = self.infer_expr(expr);
                self.record_must_use_value(&typed_expr);
                TypedStmtKind::Expression(typed_expr)
            }
            StmtKind::Let {
                name,
                mutable,
                type_annotation,
                initializer,
                is_pub,
            } => self.infer_let_stmt(
                stmt.span,
                name,
                *mutable,
                type_annotation,
                initializer,
                *is_pub,
            ),
            StmtKind::Block(stmts) => self.infer_block_stmt(stmts),
            StmtKind::If {
                condition,
                then_branch,
                else_branch,
            } => self.infer_if_stmt(condition, then_branch, else_branch.as_deref()),
            StmtKind::While { condition, body } => self.infer_while_stmt(condition, body),
            StmtKind::For {
                iterator,
                start,
                end,
                inclusive,
                step,
                body,
            } => self.infer_for_stmt(
                iterator,
                start,
                end,
                *inclusive,
                step.as_ref().as_ref(),
                body,
            ),
            StmtKind::ForEach {
                iterator,
                iterable,
                body,
            } => self.infer_for_each_stmt(iterator, iterable, body, stmt.read_only, stmt.span),
            StmtKind::Return(expr) => self.infer_return_stmt(stmt.span, expr.as_ref()),
            StmtKind::Break => TypedStmtKind::Break,
            StmtKind::Continue => TypedStmtKind::Continue,
            StmtKind::Function(func) => {
                let typed_func = self.infer_function(func);
                TypedStmtKind::Function(typed_func)
            }
            StmtKind::ImplDecl {
                type_params,
                self_type,
                trait_path,
                methods,
                ..
            } => self.infer_impl_decl(self_type, type_params, methods, trait_path.as_ref()),
            StmtKind::TraitDecl {
                name, type_params, ..
            } => TypedStmtKind::TraitDecl {
                name: name.clone(),
                type_params: type_params.clone(),
            },
            StmtKind::Needs(needs) => {
                self.handle_needs_stmt(needs);
                TypedStmtKind::Needs(needs.clone())
            }
            StmtKind::StructDecl {
                name, type_params, ..
            } => TypedStmtKind::StructDecl {
                name: name.clone(),
                type_params: type_params.clone(),
                fields: self
                    .type_table
                    .get_struct(name)
                    .map(|definition| {
                        definition
                            .fields
                            .iter()
                            .map(|field| (field.name.clone(), field.ty.clone()))
                            .collect()
                    })
                    .unwrap_or_default(),
            },
            StmtKind::EnumDecl {
                name, type_params, ..
            } => TypedStmtKind::EnumDecl {
                name: name.clone(),
                type_params: type_params.clone(),
                variants: self
                    .type_table
                    .get_enum(name)
                    .map(|def| def.variants.clone())
                    .unwrap_or_default(),
            },
        };
        if definition_scope {
            self.env.pop_scope();
        }
        for name in definition_qualified {
            self.known_native_signatures.remove(&name);
        }
        if let Some((errors_mark, constraints_mark, module)) = definition_mark {
            for error in &mut self.errors[errors_mark..] {
                error.reason.wrap_in_module(&module);
            }
            for constraint in &mut self.constraints[constraints_mark..] {
                constraint.reason_mut().wrap_in_module(&module);
            }
        }
        self.current_module = previous_module;

        TypedStmt {
            kind,
            span: stmt.span,
            definition_module: stmt.definition_module.clone(),
        }
    }

    // a module its own importer never named, so the qualified name is in scope
    fn push_definition_module_imports(&mut self, module: &aelys_syntax::ModuleId) -> Vec<String> {
        let Some(globals) = self.module_globals.get(module.as_str()) else {
            return Vec::new();
        };
        let qualified: Vec<(String, crate::types::InferType)> = globals
            .iter()
            .filter(|(name, _)| name.contains("::"))
            .map(|(name, ty)| (name.clone(), ty.clone()))
            .collect();
        let mut installed = Vec::new();
        for (name, ty) in qualified {
            if self.known_native_signatures.contains_key(&name) {
                continue;
            }
            self.known_native_signatures.insert(name.clone(), ty);
            installed.push(name);
        }
        installed
    }

    // names it reads are that module's globals, which the importer never bound.
    fn push_definition_module_globals(&mut self, module: &aelys_syntax::ModuleId) -> bool {
        let Some(globals) = self.module_globals.get(module.as_str()) else {
            return false;
        };
        let globals: Vec<(String, crate::types::InferType)> = globals
            .iter()
            .map(|(name, ty)| (name.clone(), ty.clone()))
            .collect();
        let scoped = self
            .module_scoped_globals
            .get(module.as_str())
            .cloned()
            .unwrap_or_default();
        self.env.push_scope();
        for (name, ty) in globals {
            if scoped.contains(&name) {
                self.env.define_local_alias(
                    name.clone(),
                    crate::infer::module_scoped_global(module.as_str(), &name),
                );
            }
            self.env.define_local(name, ty);
        }
        true
    }
}
