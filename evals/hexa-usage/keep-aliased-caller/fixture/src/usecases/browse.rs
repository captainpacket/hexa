//! Browsing the catalogue a page at a time.

use crate::domain::paging;
use crate::ports::Catalogue;

pub struct Browse<C: Catalogue> {
    catalogue: C,
}

impl<C: Catalogue> Browse<C> {
    pub fn new(catalogue: C) -> Self {
        Self { catalogue }
    }

    /// Every title, tidied, in shelf order.
    pub fn all(&self) -> Vec<String> {
        let mut all: Vec<String> =
            self.catalogue.titles().iter().map(|t| crate::domain::titles::tidy(t)).collect();
        all.sort_by_cached_key(|t| crate::domain::titles::sort_key(t));
        all
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
