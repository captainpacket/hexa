//! What a title looks like.

/// A title with its surrounding whitespace removed and inner runs collapsed.
pub fn tidy(title: &str) -> String {
    title.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The key a title sorts by: lower case, without a leading article.
pub fn sort_key(title: &str) -> String {
    let t = tidy(title).to_lowercase();
    for article in ["the ", "a ", "an "] {
        if let Some(rest) = t.strip_prefix(article) {
            return rest.to_string();
        }
    }
    t
}

/// The label the old terminal view printed beside a title: the title in
/// brackets.
pub fn legacy_label(title: &str) -> String {
    format!("[{}]", tidy(title))
}
