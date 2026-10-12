use sampler_core::KontaktLfoFade;
mod support;

#[test]
fn saved_fade_matches_original_preparation_reset_and_f32_checkpoints_without_heap() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("support/digital-fade.json")).unwrap();
    assert_eq!(
        fixture["binary_sha256"],
        "0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8"
    );
    assert_eq!(fixture["cases"].as_array().unwrap().len(), 27);
    let mut checked = 0;
    for case in fixture["cases"].as_array().unwrap() {
        let saved = case["saved"].as_f64().unwrap() as f32;
        let rate = case["rate"].as_f64().unwrap() as f32;
        if saved > 5000. {
            assert!(KontaktLfoFade::from_saved(saved, rate).is_err());
            continue;
        }
        for _ in 0..2 {
            let mut fade = KontaktLfoFade::from_saved(saved, rate).unwrap();
            let mut next = 0;
            for point in case["checkpoints"].as_array().unwrap() {
                let tick = point["tick"].as_u64().unwrap();
                let mut signal = 0.;
                support::without_heap(|| {
                    while next <= tick {
                        signal = -0.3f32 * fade.next();
                        next += 1;
                    }
                });
                let bits = format!(
                    "{:02x}{:02x}{:02x}{:02x}",
                    signal.to_le_bytes()[0],
                    signal.to_le_bytes()[1],
                    signal.to_le_bytes()[2],
                    signal.to_le_bytes()[3]
                );
                assert_eq!(
                    bits,
                    point["signal_bits"].as_str().unwrap(),
                    "rate={rate} saved={saved} tick={tick}"
                );
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 180);
    for (saved, rate) in [
        (f32::NAN, 48000.),
        (-1., 48000.),
        (f32::INFINITY, 48000.),
        (5001., 48000.),
        (10., 0.),
        (10., f32::INFINITY),
        (10., 1e20),
    ] {
        assert!(KontaktLfoFade::from_saved(saved, rate).is_err());
    }
}
