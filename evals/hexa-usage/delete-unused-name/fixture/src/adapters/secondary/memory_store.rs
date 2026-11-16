//! The catalogue, held in memory.

use crate::ports::Catalogue;

pub struct MemoryStore {
    titles: Vec<String>,
    label: String,
}

impl MemoryStore {
    pub fn with(titles: &[&str]) -> Self {
        Self { titles: titles.iter().map(|t| t.to_string()).collect(), label: "memory".to_string() }
    }

    /// The name this store was opened under, for logs.
    pub fn legacy_label(&self) -> &str {
        &self.label
    }
}

impl Catalogue for MemoryStore {
    fn titles(&self) -> Vec<String> {
        self.titles.clone()
    }
}
