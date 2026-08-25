use super::TypeInference;
use crate::types::{EnumDef, StructDef, TraitDef};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default)]
pub struct ImportedTypes {
    pub enums: Vec<EnumDef>,
    pub structs: Vec<StructDef>,
    pub traits: Vec<TraitDef>,
    pub withheld: BTreeMap<String, String>,
}

impl ImportedTypes {
    pub fn is_empty(&self) -> bool {
        self.enums.is_empty() && self.structs.is_empty() && self.traits.is_empty()
    }

    pub fn extend(&mut self, other: ImportedTypes) {
        self.enums.extend(other.enums);
        self.structs.extend(other.structs);
        self.traits.extend(other.traits);
        self.withheld.extend(other.withheld);
    }

    pub fn nominal_names(&self) -> Vec<String> {
        self.enums
            .iter()
            .map(|def| def.name.clone())
            .chain(self.structs.iter().map(|def| def.name.clone()))
            .chain(self.traits.iter().map(|def| def.name.clone()))
            .collect()
    }
}

impl TypeInference {
    pub(super) fn install_imported_types(&mut self, imported: ImportedTypes) {
        for def in imported.enums {
            self.type_table.register_enum(def);
        }
        for def in imported.structs {
            self.type_table.register_struct(def);
        }
        for def in imported.traits {
            self.type_table.register_trait(def);
        }
        self.withheld_nominals = imported.withheld;
    }

    pub(crate) fn withholding_module(&self, name: &str) -> Option<&str> {
        if self.type_table.has_nominal(name) || self.type_table.get_trait(name).is_some() {
            return None;
        }
        self.withheld_nominals.get(name).map(String::as_str)
    }

    /// a name the importing file never asked for reports how to ask for it; anything else keeps
    pub(crate) fn nominal_error_kind(
        &self,
        name: &str,
        fallback: crate::constraint::TypeErrorKind,
    ) -> crate::constraint::TypeErrorKind {
        match self.withholding_module(name) {
            Some(module) => crate::constraint::TypeErrorKind::TypeNotImported {
                name: name.to_string(),
                module: module.to_string(),
            },
            None => fallback,
        }
    }
}
