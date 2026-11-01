use kontra_law::kontakt::modulation_shaper;

/// Compare a curved shaper with native endpoint and midpoint vectors.
#[test]
fn shaper_matches_native_at_midi_endpoints_and_midpoint() {
    let law = modulation_shaper(&[(0.0, 0.0, -0.169), (1.0, 1.0, 0.0)]);
    for (cc, expected) in [(0, 0.0), (64, 0.797_428_1), (127, 1.0)] {
        let got = law.eval(cc as f32 / 127.0);
        assert!(
            (got - expected).abs() < 0.001,
            "CC {cc}: {got} vs {expected}"
        );
    }
}
