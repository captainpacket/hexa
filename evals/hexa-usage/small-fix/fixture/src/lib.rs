//! shelf — a tiny catalogue, in ports and adapters.
//!
//! This file is the composition root: the only place an adapter is named.

pub mod adapters;
pub mod domain;
pub mod ports;
pub mod usecases;

use adapters::secondary::memory_store::MemoryStore;
use usecases::browse::Browse;

/// A catalogue of `titles`, held in memory.
pub fn shelf(titles: &[&str]) -> Browse<MemoryStore> {
    Browse::new(MemoryStore::with(titles))
}
