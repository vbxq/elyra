use super::TypeInference;
use crate::constraint::{Constraint, TypeError};
use crate::types::InferType;
use crate::unify::{Substitution, UnifyError, unify, unify_error_to_type_error};

impl TypeInference {
    pub(super) fn solve_constraints(&mut self) -> Substitution {
        let mut subst = Substitution::new();

        for constraint in self.constraints.clone() {
            if let Constraint::Equal {
                left,
                right,
                span,
                reason,
            } = constraint
            {
                let left_resolved = subst.apply(&left);
                let right_resolved = subst.apply(&right);

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

        for constraint in self.constraints.clone() {
            if let Constraint::OneOf {
                ty,
                options,
                span,
                reason,
            } = constraint
            {
                let resolved = subst.apply(&ty);

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
