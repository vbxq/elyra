mod access;
mod layout;
mod sync;

const MAX_READY_LAYOUT: usize = 1 << 20;

// layouts of one module share an index space, so the view keeps every name it has merged and a switch between such layouts touches nothing
#[derive(Default)]
pub(crate) struct GlobalView {
    pub(crate) names: Vec<String>,
    pub(crate) generation: u64,
    ready: Vec<u64>,
    written: Vec<u32>,
    written_flags: Vec<bool>,
}

impl GlobalView {
    pub(crate) fn reset(&mut self, names: &[String], ready: Option<usize>) {
        self.names.clear();
        self.names.extend_from_slice(names);
        self.generation = self.generation.wrapping_add(1);
        self.written.clear();
        self.written_flags.clear();
        self.written_flags.resize(names.len(), false);
        if let Some(id) = ready {
            self.mark_ready(id);
        }
    }

    pub(crate) fn clear(&mut self) {
        self.reset(&[], None);
    }

    fn mark_ready(&mut self, id: usize) {
        if self.generation == 0 {
            self.generation = 1;
        }
        if id > MAX_READY_LAYOUT {
            return;
        }
        if self.ready.len() <= id {
            self.ready.resize(id + 1, 0);
        }
        self.ready[id] = self.generation;
    }

    fn is_ready(&self, id: usize) -> bool {
        self.generation != 0 && self.ready.get(id) == Some(&self.generation)
    }

    pub(crate) fn note_write(&mut self, index: usize) {
        if index >= self.written_flags.len() {
            self.written_flags.resize(index + 1, false);
        }
        if !self.written_flags[index] {
            self.written_flags[index] = true;
            if let Ok(index) = u32::try_from(index) {
                self.written.push(index);
            }
        }
    }
}
