//! Shared pitch admission previews never become waveform producers.
use super::*;

#[test]
fn shared_pitch_mono_admission_preview_controls_and_fallback_match_manual_poly_pcm_without_heap() {
    let whole = super::shared_volume::performance(&[128], true);
    let split = super::shared_volume::performance(&[1, 7, 31, 127], true);
    equal_partitions(&whole, &split);
}
