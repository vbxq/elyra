use super::Function;
use crate::bytecode::Constant;
use crate::value::Value;

impl Function {
    pub fn compute_global_layout_hash(&mut self) {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        if self.global_layout.names().is_empty() {
            self.global_layout_hash = 0;
        } else {
            let mut hasher = DefaultHasher::new();
            self.global_layout.names().hash(&mut hasher);
            self.global_layout_hash = hasher.finish() | 1;
        }
    }

    pub fn add_constant(&mut self, value: Value) -> u32 {
        self.add_structural_constant(Constant::from(value))
    }

    pub fn add_structural_constant(&mut self, value: Constant) -> u32 {
        for (i, existing) in self.constants.iter().enumerate() {
            if *existing == value {
                return u32::try_from(i).expect("constant index exceeds AVBC v3 range");
            }
        }

        let idx =
            u32::try_from(self.constants.len()).expect("constant index exceeds AVBC v3 range");
        self.constants.push(value);
        idx
    }

    pub fn add_constant_function(&mut self, func: Function) -> u32 {
        let func_idx = self.nested_functions.len();
        self.nested_functions.push(func);

        let marker = Constant::NestedFunction(
            u32::try_from(func_idx).expect("nested function index exceeds AVBC v3 range"),
        );

        let idx =
            u32::try_from(self.constants.len()).expect("constant index exceeds AVBC v3 range");
        self.constants.push(marker);
        idx
    }
}
