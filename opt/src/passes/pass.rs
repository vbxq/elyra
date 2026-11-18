use super::OptimizationStats;
use aelys_sema::TypedProgram;

pub trait OptimizationPass: Send {
    fn name(&self) -> &'static str;
    fn run(&mut self, program: &mut TypedProgram) -> OptimizationStats;
}
