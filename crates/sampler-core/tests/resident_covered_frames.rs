use sampler_core::Pcm;
mod support;

#[test]
fn covered_frames_counts_partial_disjoint_current_ranges_without_heap() {
    let pcm = Pcm::streamed(48000, 64).unwrap();
    pcm.set_ranges(vec![
        (4, vec![[0.25; 2]; 8].into()),
        (20, vec![[0.5; 2]; 8].into()),
    ])
    .unwrap();
    let clone = pcm.clone();
    let other = Pcm::streamed(48000, 64).unwrap();
    support::without_heap(|| {
        for (range, count) in [
            (0..64, 16),
            (4..12, 8),
            (8..24, 8),
            (0..4, 0),
            (12..20, 0),
            (28..64, 0),
            (64..65, 0),
            (5..5, 0),
            (10..9, 0),
        ] {
            assert_eq!(pcm.resident_covered_frames(range.clone()), count);
            assert_eq!(clone.resident_covered_frames(range), count);
        }
        assert_eq!(other.resident_covered_frames(0..64), 0);
    });
    let retired = pcm
        .set_ranges(vec![(0, vec![[0.75; 2]; 4].into())])
        .unwrap();
    support::without_heap(|| {
        assert_eq!(
            pcm.resident_covered_frames(0..64),
            4,
            "retired pinned generations do not extend current coverage"
        );
        assert_eq!(pcm.resident_covered_frames(4..12), 0);
    });
    drop(retired);
}

#[test]
fn fully_resident_coverage_stays_bounded_by_asset_metadata() {
    let pcm = Pcm::new(48000, vec![[0.25; 2]; 64].into()).unwrap();
    support::without_heap(|| {
        assert_eq!(pcm.resident_covered_frames(0..64), 64);
        assert_eq!(pcm.resident_covered_frames(7..23), 16);
        assert_eq!(pcm.resident_covered_frames(63..65), 0);
        assert_eq!(pcm.resident_covered_frames(64..64), 0);
    });
}
