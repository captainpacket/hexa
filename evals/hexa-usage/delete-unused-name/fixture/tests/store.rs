use shelf::adapters::secondary::memory_store::MemoryStore;

#[test]
fn a_store_names_itself() {
    let store = MemoryStore::with(&["Emma"]);
    assert_eq!(store.legacy_label(), "memory");
}
