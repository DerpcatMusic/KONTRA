use kontra_law::{Law, laws};

laws!(LINEAR => "laws/linear.toml");

#[test]
fn embedded_law_round_trips_through_its_source() {
    let spec = LINEAR.parse().expect("embedded law parses");
    assert_eq!(spec.law, Law::Linear { lo: 0.0, hi: 1.0 });
    assert_eq!(spec.tolerance_db, 0.1);
    assert_eq!(spec.id, Some(LINEAR.id));
    assert!(
        kontra_law::parse(LINEAR.text)
            .expect("anonymous parses")
            .id
            .is_none()
    );
    assert_eq!(LINEAR.path, "laws/linear.toml");
}
