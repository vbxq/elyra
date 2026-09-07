use super::TypeInference;
use crate::constraint::{ConstraintReason, TypeError, TypeErrorKind};
use crate::typed_ast::TypedExprKind;
use crate::types::InferType;
use aelys_syntax::Span;

impl TypeInference {
    pub(super) fn infer_identifier_expr(
        &mut self,
        name: &str,
        span: Span,
    ) -> (TypedExprKind, InferType) {
        if name == "None" || name == "Option::None" {
            let ty = InferType::Option(Box::new(self.type_gen.fresh()));
            self.record_sum_type("None", ty.clone(), span);
            return (TypedExprKind::Identifier(name.to_string()), ty);
        }

        if let Some(ty) = self
            .env
            .lookup(name)
            .or_else(|| self.env.lookup_function_ref(name))
        {
            let ty = ty.clone();
            let resolved = self.env.lookup_alias(name).unwrap_or(name).to_string();
            return (TypedExprKind::Identifier(resolved), ty);
        }

        if self.env.is_namespace(name) {
            self.errors.push(TypeError {
                kind: TypeErrorKind::NamespaceIsNotAValue {
                    name: name.to_string(),
                },
                span,
                reason: ConstraintReason::Other("namespace used as a value".to_string()),
            });
            return (
                TypedExprKind::Identifier(name.to_string()),
                InferType::Poison,
            );
        }

        if let Some(ty) = crate::native::builtin_signature(name) {
            return (TypedExprKind::Identifier(name.to_string()), ty);
        }

        if let Some(ty) = self.known_native_signatures.get(name) {
            return (TypedExprKind::Identifier(name.to_string()), ty.clone());
        }

        if let Some(ty) = crate::native::function_signature(name) {
            return (TypedExprKind::Identifier(name.to_string()), ty);
        }

        if let Some(ty) = crate::native::constant_signature(name) {
            return (TypedExprKind::Identifier(name.to_string()), ty);
        }

        if self.known_native_globals.contains(name) {
            let ty = InferType::UntypedNative(name.to_string());
            return (TypedExprKind::Identifier(name.to_string()), ty);
        }

        let ty = self
            .known_native_globals
            .contains(name)
            .then(|| InferType::UntypedNative(name.to_string()));

        let ty = ty.unwrap_or_else(|| {
            let kind = if self.globals_without_signature.contains(name) {
                TypeErrorKind::GlobalWithoutSignature {
                    name: name.to_string(),
                }
            } else {
                TypeErrorKind::UndefinedVariable {
                    name: name.to_string(),
                }
            };
            self.errors.push(TypeError {
                kind,
                span,
                reason: ConstraintReason::Other("variable lookup".to_string()),
            });
            InferType::Poison
        });

        (TypedExprKind::Identifier(name.to_string()), ty)
    }
}
