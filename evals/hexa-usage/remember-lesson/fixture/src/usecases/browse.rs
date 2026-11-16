//! Browsing the catalogue a page at a time.

use crate::domain::{paging, titles};
use crate::ports::Catalogue;

pub struct Browse<C: Catalogue> {
    catalogue: C,
}

impl<C: Catalogue> Browse<C> {
    pub fn new(catalogue: C) -> Self {
        Self { catalogue }
    }

    /// Every title, tidied.
    pub fn all(&self) -> Vec<String> {
        self.catalogue.titles().iter().map(|t| titles::tidy(t)).collect()
    }

    /// How many pages the catalogue fills.
    pub fn pages(&self, per_page: usize) -> usize {
        paging::page_count(self.catalogue.titles().len(), per_page)
    }

    /// The titles on page `page` (counted from 0).
    pub fn page(&self, page: usize, per_page: usize) -> Vec<String> {
        let all = self.all();
        all[paging::page_range(all.len(), per_page, page)].to_vec()
    }
}
