use super::TypeInference;
use aelys_syntax::{ImportKind, NeedsStmt};

impl TypeInference {
    pub(super) fn handle_needs_stmt(&mut self, needs: &NeedsStmt) {
        match &needs.kind {
            ImportKind::Symbols(_) => {}
            ImportKind::Module { alias } => {
                let _module_name = alias
                    .clone()
                    .unwrap_or_else(|| needs.path.last().cloned().unwrap_or_default());
            }
            ImportKind::Wildcard => {}
        }
    }
}
