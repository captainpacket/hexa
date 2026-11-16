//! Renders a page of titles for a terminal.
//!
//! Titles used to carry a `legacy_label` beside them; the number is the label
//! now.

/// What `--help` says about the columns.
pub const COLUMNS: &str = "number, title (the legacy_label column was removed)";

/// One title per line, numbered from `first`.
pub fn render(titles: &[String], first: usize) -> String {
    titles
        .iter()
        .enumerate()
        .map(|(i, t)| format!("{:>3}. {t}", first + i + 1))
        .collect::<Vec<_>>()
        .join("\n")
}
