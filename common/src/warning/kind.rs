use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WarningKind {
    InlineRecursive,
    InlineMutualRecursion {
        cycle: Vec<String>,
    },
    InlineHasCaptures,
    InlinePublicFunction,
    InlineNativeFunction,

    // TODO: emit these warnings in the optimizer
    UnusedVariable {
        name: String,
    },
    UnusedFunction {
        name: String,
    },
    UnusedImport {
        module: String,
    },
    DeprecatedFunction {
        name: String,
        replacement: Option<String>,
    },
    ShadowedVariable {
        name: String,
    },

    UnknownType {
        name: String,
    },
    UnknownTypeParameter {
        param: String,
        in_type: String,
    },
    IncompatibleComparison {
        left: String,
        right: String,
        op: String,
    },
}

impl WarningKind {
    pub fn is_inline_related(&self) -> bool {
        matches!(
            self,
            WarningKind::InlineRecursive
                | WarningKind::InlineMutualRecursion { .. }
                | WarningKind::InlineHasCaptures
                | WarningKind::InlinePublicFunction
                | WarningKind::InlineNativeFunction
        )
    }

    pub fn category(&self) -> &'static str {
        match self {
            WarningKind::InlineRecursive
            | WarningKind::InlineMutualRecursion { .. }
            | WarningKind::InlineHasCaptures
            | WarningKind::InlinePublicFunction
            | WarningKind::InlineNativeFunction => "inline",

            // TODO: when these warnings are emitted, assign proper categories
            WarningKind::UnusedVariable { .. }
            | WarningKind::UnusedFunction { .. }
            | WarningKind::UnusedImport { .. } => "unused",

            WarningKind::DeprecatedFunction { .. } => "deprecated",

            WarningKind::ShadowedVariable { .. } => "shadow",

            WarningKind::UnknownType { .. }
            | WarningKind::UnknownTypeParameter { .. }
            | WarningKind::IncompatibleComparison { .. } => "type",
        }
    }
}

impl fmt::Display for WarningKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.category())
    }
}
