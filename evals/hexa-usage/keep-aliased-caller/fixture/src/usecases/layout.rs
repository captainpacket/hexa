//! How much room a page of titles needs.

use crate::domain::titles::shelf_width as widest;

/// The column a page of `titles` needs: the number, the dot and the title.
pub fn column_width(titles: &[String]) -> usize {
    widest(titles) + 5
}
