use super::TypeInference;
use crate::types::{EnumDef, InferType, StructDef, TraitDef};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Default)]
pub struct ImportedTypes {
    pub enums: Vec<EnumDef>,
    pub structs: Vec<StructDef>,
    pub traits: Vec<TraitDef>,
    pub withheld: BTreeMap<String, String>,
    pub module_globals: BTreeMap<String, BTreeMap<String, InferType>>,
    pub module_scoped_globals: BTreeMap<String, BTreeSet<String>>,
    // declares it; the importing file never asked for it and may not name it
    pub private_nominals: BTreeMap<String, String>,
    // the subset of `private_nominals` its own module never made public, so no `needs` line
    pub unexported_nominals: BTreeSet<String>,
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
        self.module_globals.extend(other.module_globals);
        self.module_scoped_globals
            .extend(other.module_scoped_globals);
        self.private_nominals.extend(other.private_nominals);
        self.unexported_nominals.extend(other.unexported_nominals);
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
        self.module_globals = imported.module_globals;
        self.module_scoped_globals = imported.module_scoped_globals;
        self.private_nominals = imported.private_nominals;
        self.unexported_nominals = imported.unexported_nominals;
    }

    // an importer that never named it reads it as absent, though the table holds it for the
    pub(crate) fn refused_private_nominal(&self, name: &str) -> Option<&str> {
        if self.current_module != self.root_module {
            return None;
        }
        self.private_nominals.get(name).map(String::as_str)
    }

    pub(crate) fn private_nominal_error(
        &self,
        name: &str,
        span: aelys_syntax::Span,
    ) -> Option<crate::constraint::TypeError> {
        let module = self.refused_private_nominal(name)?.to_string();
        let kind = match self.unexported_nominals.contains(name) {
            true => crate::constraint::TypeErrorKind::PrivateNominalNotExported {
                name: name.to_string(),
                module,
            },
            false => crate::constraint::TypeErrorKind::TypeNotImported {
                name: name.to_string(),
                module,
            },
        };
        Some(crate::constraint::TypeError {
            kind,
            span,
            reason: crate::constraint::ConstraintReason::UnknownType {
                name: name.to_string(),
            },
        })
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
