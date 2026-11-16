//! Contracts between the layers.

/// Where the titles come from.
pub trait Catalogue {
    fn titles(&self) -> Vec<String>;
}
