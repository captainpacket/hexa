//! The catalogue, held in memory.

use crate::ports::Catalogue;

pub struct MemoryStore {
    titles: Vec<String>,
}

impl MemoryStore {
    pub fn with(titles: &[&str]) -> Self {
        Self { titles: titles.iter().map(|t| t.to_string()).collect() }
    }
}

impl Catalogue for MemoryStore {
    fn titles(&self) -> Vec<String> {
        self.titles.clone()
    }
}
