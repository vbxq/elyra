use super::TypeInference;
use crate::constraint::{Constraint, TypeError, TypeErrorKind};
use crate::types::InferType;
use crate::unify::{Substitution, UnifyError, unify, unify_error_to_type_error};
use aelys_syntax::Span;
use std::collections::HashMap;

/// rejected at collection; this bounds an acyclic but exponentially expanding
const MAX_PROJECTION_NODES: usize = 4096;

fn type_node_count(ty: &InferType) -> usize {
    fn walk(ty: &InferType, count: &mut usize) {
        if *count > MAX_PROJECTION_NODES {
            return;
        }
        *count += 1;
        match ty {
            InferType::Option(inner)
            | InferType::Array(inner)
            | InferType::FixedArray(inner, _)
            | InferType::Vec(inner)
            | InferType::Projection { self_ty: inner, .. } => walk(inner, count),
            InferType::Result(ok, err) => {
                walk(ok, count);
                walk(err, count);
            }
            InferType::Tuple(elements) => {
                for element in elements {
                    walk(element, count);
                }
            }
            InferType::Applied { args, .. } => {
                for arg in args {
                    walk(arg, count);
                }
            }
            InferType::Function { params, ret } => {
                for param in params {
                    walk(param, count);
                }
                walk(ret, count);
            }
            _ => {}
        }
    }
    let mut count = 0;
    walk(ty, &mut count);
    count
}

struct NormalizeState {
    visiting: Vec<InferType>,
    cache: HashMap<InferType, InferType>,
    overflowed: Option<(String, String)>,
}

impl TypeInference {
    /// monomorphizer substitutes it and the backend resolves it.
    pub(super) fn normalize_projection_types(&mut self, ty: &InferType, span: Span) -> InferType {
        let mut state = NormalizeState {
            visiting: Vec::new(),
            cache: HashMap::new(),
            overflowed: None,
        };
        let normalized = self.normalize_projection_types_bounded(ty, &mut state);
        if let Some((receiver, item)) = state.overflowed {
            self.errors.push(TypeError {
                kind: TypeErrorKind::AssociatedProjectionLimit {
                    receiver,
                    item,
                    limit: MAX_PROJECTION_NODES,
                },
                span,
                reason: crate::constraint::ConstraintReason::Other(
                    "associated projection normalization".to_string(),
                ),
            });
            return InferType::Poison;
        }
        normalized
    }

    /// already rejected with e0423 when the impls are collected, so reaching
    fn normalize_projection_types_bounded(
        &self,
        ty: &InferType,
        state: &mut NormalizeState,
    ) -> InferType {
        if state.overflowed.is_some() {
            return InferType::Poison;
        }
        match ty {
            InferType::Projection { item, self_ty, .. } => {
                if state.visiting.iter().any(|seen| seen == ty) {
                    self.projection_cycle_escaped.set(true);
                    return InferType::Poison;
                }
                if let Some(cached) = state.cache.get(ty) {
                    return cached.clone();
                }
                let Some(resolved) = self.resolve_associated_projection(ty) else {
                    return ty.clone();
                };
                state.visiting.push(ty.clone());
                let normalized = self.normalize_projection_types_bounded(&resolved, state);
                state.visiting.pop();
                if type_node_count(&normalized) > MAX_PROJECTION_NODES {
                    state.overflowed = Some((self_ty.to_string(), item.clone()));
                    return InferType::Poison;
                }
                state.cache.insert(ty.clone(), normalized.clone());
                normalized
            }
            InferType::Function { params, ret } => InferType::Function {
                params: params
                    .iter()
                    .map(|param| self.normalize_projection_types_bounded(param, state))
                    .collect(),
                ret: Box::new(self.normalize_projection_types_bounded(ret, state)),
            },
            InferType::Array(inner) => InferType::Array(Box::new(
                self.normalize_projection_types_bounded(inner, state),
            )),
            InferType::FixedArray(inner, length) => InferType::FixedArray(
                Box::new(self.normalize_projection_types_bounded(inner, state)),
                *length,
            ),
            InferType::Vec(inner) => InferType::Vec(Box::new(
                self.normalize_projection_types_bounded(inner, state),
            )),
            InferType::Option(inner) => InferType::Option(Box::new(
                self.normalize_projection_types_bounded(inner, state),
            )),
            InferType::Result(ok, err) => InferType::Result(
                Box::new(self.normalize_projection_types_bounded(ok, state)),
                Box::new(self.normalize_projection_types_bounded(err, state)),
            ),
            InferType::Tuple(elements) => InferType::Tuple(
                elements
                    .iter()
                    .map(|element| self.normalize_projection_types_bounded(element, state))
                    .collect(),
            ),
            InferType::Applied { name, args } => InferType::Applied {
                name: name.clone(),
                args: args
                    .iter()
                    .map(|arg| self.normalize_projection_types_bounded(arg, state))
                    .collect(),
            },
            _ => ty.clone(),
        }
    }

    pub(super) fn solve_constraints(&mut self) -> Substitution {
        let mut subst = Substitution::new();

        // a projection over a type parameter cannot be resolved before that
        let mut deferred = Vec::new();
        for constraint in self.constraints.clone() {
            if let Constraint::Equal {
                left,
                right,
                span,
                reason,
            } = constraint
            {
                let left_resolved = self.normalize_projection_types(&subst.apply(&left), span);
                let right_resolved = self.normalize_projection_types(&subst.apply(&right), span);

                // the constraint carrying an unresolved projection is often the
                if contains_open_projection(&left_resolved)
                    || contains_open_projection(&right_resolved)
                {
                    let components = decompose_for_deferral(&left_resolved, &right_resolved);
                    for (component_left, component_right) in components {
                        if contains_open_projection(&component_left)
                            || contains_open_projection(&component_right)
                        {
                            deferred.push((component_left, component_right, span, reason.clone()));
                            continue;
                        }
                        if unify(&component_left, &component_right, &mut subst).is_err() {
                            deferred.push((component_left, component_right, span, reason.clone()));
                        }
                    }
                    continue;
                }

                match unify(&left_resolved, &right_resolved, &mut subst) {
                    Ok(()) => {}
                    Err(UnifyError::Poisoned) => {
                        self.poison(&left_resolved, &mut subst);
                        self.poison(&right_resolved, &mut subst);
                    }
                    Err(e) => {
                        let err = unify_error_to_type_error(e, span, reason);
                        self.errors.push(err);

                        self.poison(&left_resolved, &mut subst);
                        self.poison(&right_resolved, &mut subst);
                    }
                }
            }
        }

        for (left, right, span, reason) in deferred {
            let left_resolved = self.normalize_projection_types(&subst.apply(&left), span);
            let right_resolved = self.normalize_projection_types(&subst.apply(&right), span);
            match unify(&left_resolved, &right_resolved, &mut subst) {
                Ok(()) => {}
                Err(UnifyError::Poisoned) => {
                    self.poison(&left_resolved, &mut subst);
                    self.poison(&right_resolved, &mut subst);
                }
                Err(e) => {
                    self.errors.push(unify_error_to_type_error(e, span, reason));
                    self.poison(&left_resolved, &mut subst);
                    self.poison(&right_resolved, &mut subst);
                }
            }
        }

        for constraint in self.constraints.clone() {
            if let Constraint::OneOf {
                ty,
                options,
                span,
                reason,
            } = constraint
            {
                let resolved = self.normalize_projection_types(&subst.apply(&ty), span);

                match &resolved {
                    InferType::Var(_) | InferType::Dynamic | InferType::Poison => {}
                    concrete => {
                        let solutions = options
                            .iter()
                            .filter_map(|option| {
                                let mut candidate = subst.clone();
                                unify(concrete, option, &mut candidate)
                                    .ok()
                                    .map(|()| candidate)
                            })
                            .take(2)
                            .collect::<Vec<_>>();

                        match solutions.len() {
                            0 => {
                                self.errors.push(TypeError::not_one_of(
                                    concrete.clone(),
                                    options.clone(),
                                    span,
                                    reason,
                                ));
                                self.poison(&resolved, &mut subst);
                            }
                            1 => {
                                if let Some(solution) = solutions.into_iter().next() {
                                    subst = solution;
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        subst
    }

    fn poison(&mut self, ty: &InferType, subst: &mut Substitution) {
        match ty {
            InferType::Var(id) => {
                subst.bind(*id, InferType::Poison);
            }
            InferType::Function { params, ret } => {
                for p in params {
                    self.poison(p, subst);
                }
                self.poison(ret, subst);
            }
            InferType::Array(inner) => {
                self.poison(inner, subst);
            }
            InferType::FixedArray(inner, _) => {
                self.poison(inner, subst);
            }
            InferType::Tuple(elems) => {
                for e in elems {
                    self.poison(e, subst);
                }
            }
            _ => {}
        }
    }
}

fn contains_open_projection(ty: &InferType) -> bool {
    match ty {
        InferType::Projection { .. } => true,
        InferType::Option(inner)
        | InferType::Array(inner)
        | InferType::FixedArray(inner, _)
        | InferType::Vec(inner) => contains_open_projection(inner),
        InferType::Result(ok, err) => contains_open_projection(ok) || contains_open_projection(err),
        InferType::Tuple(elements) => elements.iter().any(contains_open_projection),
        InferType::Applied { args, .. } => args.iter().any(contains_open_projection),
        InferType::Function { params, ret } => {
            params.iter().any(contains_open_projection) || contains_open_projection(ret)
        }
        _ => false,
    }
}

fn decompose_for_deferral(left: &InferType, right: &InferType) -> Vec<(InferType, InferType)> {
    let (
        InferType::Function {
            params: left_params,
            ret: left_ret,
        },
        InferType::Function {
            params: right_params,
            ret: right_ret,
        },
    ) = (left, right)
    else {
        return vec![(left.clone(), right.clone())];
    };
    if left_params.len() != right_params.len() {
        return vec![(left.clone(), right.clone())];
    }
    let mut components: Vec<(InferType, InferType)> = left_params
        .iter()
        .cloned()
        .zip(right_params.iter().cloned())
        .collect();
    components.push((left_ret.as_ref().clone(), right_ret.as_ref().clone()));
    components
}
