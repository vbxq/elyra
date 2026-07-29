use super::VM;
use super::config::VmConfig;
use aelys_syntax::Source;
use std::sync::Arc;

impl VM {
    pub fn config(&self) -> &VmConfig {
        &self.config
    }

    pub fn program_args(&self) -> &[String] {
        &self.program_args
    }

    pub fn set_script_path(&mut self, path: String) {
        // On Windows, canonicalize() returns \\?\ prefixed paths which disable
        // path normalization (forward slashes won't be converted to backslashes).
        // Strip the prefix so that mixed-separator paths work correctly.
        #[cfg(windows)]
        let path = path
            .strip_prefix(r"\\?\")
            .map(|s| s.to_string())
            .unwrap_or(path);
        self.script_path = Some(path);
    }

    pub fn script_path(&self) -> Option<&str> {
        self.script_path.as_deref()
    }

    pub fn source(&self) -> &Arc<Source> {
        &self.source
    }

    pub fn set_source(&mut self, source: Arc<Source>) {
        self.source = source;
    }
}
