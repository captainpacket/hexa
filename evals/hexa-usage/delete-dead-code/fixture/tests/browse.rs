use shelf::adapters::primary::cli::render;

#[test]
fn a_page_lists_its_titles_tidied() {
    let s = shelf::shelf(&["  Dune ", "Emma", "Ulysses"]);
    assert_eq!(s.page(0, 2), vec!["Dune", "Emma"]);
    assert_eq!(s.pages(2), 2);
}

#[test]
fn rendering_numbers_from_the_first_title() {
    assert_eq!(render(&["Emma".to_string()], 1), "  2. Emma");
}
