// global constant propagation - inlines immutable top-level let bindings

mod collect;
mod scope;
mod substitute;

use super::{OptimizationPass, OptimizationStats};
use aelys_sema::TypedProgram;
use scope::ShadowStack;
use std::collections::HashMap;

pub struct GlobalConstantPropagator {
    constants: HashMap<String, aelys_sema::TypedExpr>,
    shadows: ShadowStack,
    stats: OptimizationStats,
}

impl GlobalConstantPropagator {
    pub fn new() -> Self {
        Self {
            constants: HashMap::new(),
            shadows: ShadowStack::default(),
            stats: OptimizationStats::new(),
        }
    }
}

impl Default for GlobalConstantPropagator {
    fn default() -> Self {
        Self::new()
    }
}

impl OptimizationPass for GlobalConstantPropagator {
    fn name(&self) -> &'static str {
        "global_const_prop"
    }

    fn run(&mut self, program: &mut TypedProgram) -> OptimizationStats {
        self.constants.clear();
        self.shadows.clear();
        self.stats = OptimizationStats::new();

        self.collect_global_constants(&program.stmts);
        for stmt in &mut program.stmts {
            self.substitute_in_stmt(stmt);
        }

        self.stats.clone()
    }
}
