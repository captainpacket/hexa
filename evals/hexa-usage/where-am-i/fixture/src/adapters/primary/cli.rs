//! Renders a page of titles for a terminal.

/// One title per line, numbered from `first`.
pub fn render(titles: &[String], first: usize) -> String {
    titles
        .iter()
        .enumerate()
        .map(|(i, t)| format!("{:>3}. {t}", first + i + 1))
        .collect::<Vec<_>>()
        .join("\n")
}
