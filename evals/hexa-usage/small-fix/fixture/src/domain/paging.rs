//! Splitting a list into pages.

/// How many pages `total` items fill, `per_page` to a page.
pub fn page_count(total: usize, per_page: usize) -> usize {
    if per_page == 0 {
        return 0;
    }
    total / per_page + 1
}

/// The half-open range of item indices on page `page` (counted from 0).
pub fn page_range(total: usize, per_page: usize, page: usize) -> std::ops::Range<usize> {
    let start = (page * per_page).min(total);
    let end = (start + per_page).min(total);
    start..end
}
