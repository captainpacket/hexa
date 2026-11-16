use shelf::domain::paging::{page_count, page_range};

#[test]
fn a_partial_last_page_is_a_page() {
    assert_eq!(page_count(11, 5), 3);
}

#[test]
fn an_even_split_adds_no_empty_page() {
    assert_eq!(page_count(10, 5), 2);
}

#[test]
fn nothing_fills_no_pages() {
    assert_eq!(page_count(0, 5), 0);
}

#[test]
fn the_last_page_holds_the_rest() {
    assert_eq!(page_range(11, 5, 2), 10..11);
}
