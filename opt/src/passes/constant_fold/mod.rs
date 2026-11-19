
mod expr;
mod stmt;

use super::{OptimizationPass, OptimizationStats};
use aelys_sema::TypedProgram;

const MAX_FOLDED_STRING_LEN: usize = 4096; // don't bloat constant pool with huge strings

pub struct ConstantFolder {
    stats: OptimizationStats,
}

impl ConstantFolder {
    pub fn new() -> Self {
        Self {
            stats: OptimizationStats::new(),
        }
    }
}

impl Default for ConstantFolder {
    fn default() -> Self {
        Self::new()
    }
}

impl OptimizationPass for ConstantFolder {
    fn name(&self) -> &'static str {
        "constant_fold"
    }

    fn run(&mut self, program: &mut TypedProgram) -> OptimizationStats {
        self.stats = OptimizationStats::new();
        for stmt in &mut program.stmts {
            self.optimize_stmt(stmt);
        }
        self.stats.clone()
    }
}
