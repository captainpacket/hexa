//! What a title looks like.

/// A title with its surrounding whitespace removed and inner runs collapsed.
pub fn tidy(title: &str) -> String {
    title.split_whitespace().collect::<Vec<_>>().join(" ")
}
