// global constant propagation - inlines immutable top-level let bindings

mod collect;
mod scope;
mod substitute;

use super::{OptimizationPass, OptimizationStats};
use aelys_sema::TypedProgram;
use scope::ShadowStack;
use std::collections::{HashMap, HashSet};

pub struct GlobalConstantPropagator {
    constants: HashMap<String, aelys_sema::TypedExpr>,
    // bound after top-level code first runs, so a function called earlier can still see them unset
    late: HashSet<String>,
    late_bound: HashSet<String>,
    function_depth: usize,
    shadows: ShadowStack,
    stats: OptimizationStats,
}

impl GlobalConstantPropagator {
    pub fn new() -> Self {
        Self {
            constants: HashMap::new(),
            late: HashSet::new(),
            late_bound: HashSet::new(),
            function_depth: 0,
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
        self.late.clear();
        self.late_bound.clear();
        self.function_depth = 0;
        self.shadows.clear();
        self.stats = OptimizationStats::new();

        self.collect_global_constants(&program.stmts);
        for stmt in &mut program.stmts {
            self.substitute_in_stmt(stmt);
            if let aelys_sema::TypedStmtKind::Let { name, .. } = &stmt.kind {
                self.late_bound.insert(name.clone());
            }
        }

        self.stats.clone()
    }
}
