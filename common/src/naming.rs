// the compiler mangles a module-private global into one slot per module; the prefix names
pub const MODULE_GLOBAL_PREFIX: &str = "__aelys_modglobal::";

pub fn module_scoped_global(module: &str, name: &str) -> String {
    format!("{MODULE_GLOBAL_PREFIX}{module}::{name}")
}

pub fn is_module_scoped_global(symbol: &str) -> bool {
    symbol.starts_with(MODULE_GLOBAL_PREFIX)
}

// a module id never holds `::`, so the last pair separates the module from the name
pub fn unscoped_global_name(symbol: &str) -> &str {
    symbol
        .strip_prefix(MODULE_GLOBAL_PREFIX)
        .and_then(|tail| tail.rsplit_once("::"))
        .map_or(symbol, |(_, name)| name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_message_shows_the_module_global_prefix() {
        let mangled = module_scoped_global("mid", "helper");
        assert!(is_module_scoped_global(&mangled));

        let compile = crate::error::CompileErrorKind::UndefinedVariable(mangled.clone()).message();
        assert_eq!(compile, "undefined variable 'helper'");

        let runtime = crate::error::RuntimeErrorKind::UndefinedVariable(mangled.clone()).message();
        assert_eq!(runtime, "undefined variable 'helper'");

        let warning = crate::warning::WarningKind::UnusedVariable {
            name: mangled.clone(),
        }
        .message(None);
        assert_eq!(warning, "unused variable 'helper'");

        let shadowed =
            crate::warning::WarningKind::ShadowedVariable { name: mangled }.message(None);
        assert_eq!(shadowed, "variable 'helper' shadows a previous binding");
    }

    #[test]
    fn a_name_the_reader_wrote_is_left_alone() {
        assert_eq!(unscoped_global_name("core3::helper"), "core3::helper");
        assert_eq!(unscoped_global_name("helper"), "helper");
    }
}
