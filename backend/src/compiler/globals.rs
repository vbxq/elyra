use super::Compiler;

impl Compiler {
    pub fn get_or_create_global_index(&mut self, name: &str) -> u32 {
        let actual_name = self.resolve_global_name(name).to_string();
        // callers record the spelling they were given, which the layout does not hold
        if actual_name != name {
            self.accessed_globals.insert(actual_name.clone());
        }
        self.get_or_create_global_index_raw(&actual_name)
    }

    pub fn get_or_create_global_index_raw(&mut self, name: &str) -> u32 {
        if let Some(&idx) = self.global_indices.get(name) {
            idx
        } else {
            let idx = self.next_global_index;
            self.global_indices.insert(name.to_string(), idx);
            self.next_global_index += 1;
            idx
        }
    }

    pub fn resolve_global_name<'a>(&'a self, name: &'a str) -> &'a str {
        if self.globals.contains_key(name) {
            return name;
        }
        self.symbol_origins
            .get(name)
            .filter(|origin| origin.contains("::"))
            .map(String::as_str)
            .unwrap_or(name)
    }
}
