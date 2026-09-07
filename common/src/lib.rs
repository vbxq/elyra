pub mod error;
pub mod naming;
pub mod result;
pub mod warning;

pub use error::{
    AelysError, CompileError, CompileErrorKind, PrivateFieldDetail, RuntimeError, RuntimeErrorKind,
    StackFrame,
};
pub use naming::{
    MODULE_GLOBAL_PREFIX, is_module_scoped_global, module_scoped_global, unscoped_global_name,
};
pub use result::Result;
pub use warning::{Warning, WarningCollector, WarningConfig, WarningKind, format_warnings};
