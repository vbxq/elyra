use crate::types::{InferType, TypeVarId};
use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct Substitution {
    bindings: HashMap<TypeVarId, InferType>,
    param_bindings: HashMap<String, InferType>,
}

impl Substitution {
    pub fn new() -> Self {
        Self {
            bindings: HashMap::new(),
            param_bindings: HashMap::new(),
        }
    }

    pub fn bind(&mut self, var: TypeVarId, ty: InferType) {
        if ty != InferType::Var(var) {
            self.bindings.insert(var, ty);
        }
    }

    pub fn bind_param(&mut self, name: impl Into<String>, ty: InferType) {
        self.param_bindings.insert(name.into(), ty);
    }

    pub fn is_bound(&self, var: TypeVarId) -> bool {
        self.bindings.contains_key(&var)
    }

    pub fn get(&self, var: TypeVarId) -> Option<&InferType> {
        self.bindings.get(&var)
    }

    pub fn apply(&self, ty: &InferType) -> InferType {
        match ty {
            InferType::Var(id) => {
                if let Some(bound) = self.bindings.get(id) {
                    self.apply(bound)
                } else {
                    ty.clone()
                }
            }
            InferType::Param(name) => self
                .param_bindings
                .get(name)
                .map_or_else(|| ty.clone(), |bound| self.apply(bound)),
            InferType::Function { params, ret } => InferType::Function {
                params: params.iter().map(|p| self.apply(p)).collect(),
                ret: Box::new(self.apply(ret)),
            },
            InferType::Array(inner) => InferType::Array(Box::new(self.apply(inner))),
            InferType::FixedArray(inner, length) => {
                InferType::FixedArray(Box::new(self.apply(inner)), *length)
            }
            InferType::Vec(inner) => InferType::Vec(Box::new(self.apply(inner))),
            InferType::Option(inner) => InferType::Option(Box::new(self.apply(inner))),
            InferType::Result(ok, err) => {
                InferType::Result(Box::new(self.apply(ok)), Box::new(self.apply(err)))
            }
            InferType::Tuple(elems) => {
                InferType::Tuple(elems.iter().map(|e| self.apply(e)).collect())
            }
            InferType::Applied { name, args } => InferType::Applied {
                name: name.clone(),
                args: args.iter().map(|arg| self.apply(arg)).collect(),
            },
            InferType::Projection {
                trait_name,
                item,
                self_ty,
            } => InferType::Projection {
                trait_name: trait_name.clone(),
                item: item.clone(),
                self_ty: Box::new(self.apply(self_ty)),
            },
            InferType::I8
            | InferType::I16
            | InferType::I32
            | InferType::I64
            | InferType::U8
            | InferType::U16
            | InferType::U32
            | InferType::U64
            | InferType::F32
            | InferType::F64
            | InferType::Bool
            | InferType::String
            | InferType::Unit
            | InferType::Null
            | InferType::Error
            | InferType::Never
            | InferType::Numeric
            | InferType::UntypedNative(_)
            | InferType::Range
            | InferType::Struct(_)
            | InferType::Dynamic
            | InferType::Poison => ty.clone(),
        }
    }

    pub fn compose(&self, other: &Substitution) -> Substitution {
        let mut result = Substitution::new();

        for (var, ty) in &self.bindings {
            result.bindings.insert(*var, other.apply(ty));
        }

        for (var, ty) in &other.bindings {
            if !result.bindings.contains_key(var) {
                result.bindings.insert(*var, ty.clone());
            }
        }

        for (name, ty) in &self.param_bindings {
            result.param_bindings.insert(name.clone(), other.apply(ty));
        }

        for (name, ty) in &other.param_bindings {
            if !result.param_bindings.contains_key(name) {
                result.param_bindings.insert(name.clone(), ty.clone());
            }
        }

        result
    }

    pub fn bindings(&self) -> &HashMap<TypeVarId, InferType> {
        &self.bindings
    }

    pub fn len(&self) -> usize {
        self.bindings.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }
}
