use super::super::Compiler;

impl Compiler {
    // deprecated - types tracked via TypedExpr now
    pub fn get_register_type(&self, _reg: u16) -> aelys_sema::ResolvedType {
        aelys_sema::ResolvedType::Dynamic
    }
    pub fn set_register_type(&mut self, _reg: u16, _typ: aelys_sema::ResolvedType) {}
}
