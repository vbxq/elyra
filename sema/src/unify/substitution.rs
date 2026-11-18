use crate::types::{InferType, ParamMode, ParamModes, TypeVarId};
use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct Substitution {
    bindings: HashMap<TypeVarId, InferType>,
    param_bindings: HashMap<String, InferType>,
    mode_bindings: HashMap<TypeVarId, ParamMode>,
    closed_modes: bool,
}

impl Substitution {
    pub fn new() -> Self {
        Self {
            bindings: HashMap::new(),
            param_bindings: HashMap::new(),
            mode_bindings: HashMap::new(),
            closed_modes: false,
        }
    }

    pub fn bind_mode(&mut self, var: TypeVarId, mode: ParamMode) {
        if mode != ParamMode::Var(var) {
            self.mode_bindings.insert(var, mode);
        }
    }

    pub fn apply_mode(&self, mode: ParamMode) -> ParamMode {
        match mode {
            ParamMode::Var(var) => match self.mode_bindings.get(&var) {
                Some(bound) => self.apply_mode(*bound),
                None if self.closed_modes => ParamMode::Plain,
                None => mode,
            },
            other => other,
        }
    }

    pub fn apply_modes(&self, modes: &ParamModes) -> ParamModes {
        modes.map(|mode| self.apply_mode(mode))
    }

    // once solving is over, a parameter no call or value constrained is a plain one
    pub fn close_modes(&mut self) {
        self.closed_modes = true;
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
            InferType::Function { params, modes, ret } => InferType::Function {
                params: params.iter().map(|p| self.apply(p)).collect(),
                modes: self.apply_modes(modes),
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
            InferType::Map(key, value) => {
                InferType::Map(Box::new(self.apply(key)), Box::new(self.apply(value)))
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
            | InferType::Int64
            | InferType::U64
            | InferType::F32
            | InferType::F64
            | InferType::Bool
            | InferType::Char
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

        for (var, mode) in &self.mode_bindings {
            result.mode_bindings.insert(*var, other.apply_mode(*mode));
        }

        for (var, mode) in &other.mode_bindings {
            result.mode_bindings.entry(*var).or_insert(*mode);
        }
        result.closed_modes = self.closed_modes || other.closed_modes;

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
