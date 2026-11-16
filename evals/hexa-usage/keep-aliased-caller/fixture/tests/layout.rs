use shelf::usecases::layout::column_width;

#[test]
fn the_column_fits_the_widest_title() {
    assert_eq!(column_width(&["Emma".to_string(), " Ulysses ".to_string()]), 12);
}
