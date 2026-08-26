use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModuleId(String);

impl ModuleId {
    pub fn new(path: impl Into<String>) -> Self {
        Self(path.into().replace("::", "."))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_same_or_descendant_of(&self, owner: &Self) -> bool {
        self == owner
            || (owner.0.is_empty() && !self.0.is_empty())
            || self.0.starts_with(&format!("{}.", owner.0))
    }
}

impl fmt::Display for ModuleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
