//! Saved unsynchronized shared Digital: authored Hz changes future windows only.
use super::*;

pub(super) fn native_windows() -> Vec<(usize, usize, f64, f64)> {
    // Initial48Hz uses the independently established native f32 reciprocal law.
    // Later steps are actual PE95ecd0 receipts at500k/750k/1m; no production
    // parameter converter or source Clock is used by this exterior PCM oracle.
    // The0 write at799 changes authored Hz but not committed window771; an
    // idle750k edit supersedes it before any subsequent source window begins.
    let mut phase = 0.;
    [
        (0, 0x3fa0624dc0000000),
        (257, 0x3fa0624dc0000000),
        (514, 0x3f50484180000000),
        (771, 0x3f50484180000000),
        (1542, 0x3f883900a0000000),
        (1799, 0x3f883900a0000000),
        (2056, 0x3fc2302ba0000000),
    ]
    .into_iter()
    .map(|(base, bits)| {
        let delta = f64::from_bits(bits);
        let row = (base, base + 257, phase, delta);
        phase += 257. / 32. * delta;
        while phase > 1. {
            phase -= 1.;
        }
        row
    })
    .collect()
}

#[test]
fn saved_shared_unsync_live_ksp_frequency_retains_committed_generation_and_complete_stereo_pcm_without_heap()
 {
    let (files, mut instrument) = fixture_source(false, false, None, None, Some(48.), true);
    let source = &instrument.groups[0].pitch_lfos[0];
    assert!(source.shared && source.note_value <= 1e-9);
    assert_eq!(source.count, 48.);
    instrument.scripts = vec![
        "on init
        declare $after := 0
        declare $write := 0
        make_persistent($after)
        end on
        on controller
        select ($write)
        case 0
            set_engine_par($ENGINE_PAR_INTMOD_FREQUENCY,500000,0,0,-1)
        case 1
            set_engine_par($ENGINE_PAR_INTMOD_FREQUENCY,0,0,0,-1)
        case 2
            set_engine_par($ENGINE_PAR_INTMOD_FREQUENCY,750000,0,0,-1)
        case 3
            set_engine_par($ENGINE_PAR_INTMOD_FREQUENCY,1000000,0,0,-1)
        end select
        inc($write)
        $after := get_engine_par($ENGINE_PAR_INTMOD_FREQUENCY,0,0,-1)
        end on"
            .into(),
    ];
    let cache = files.path().join("cache");
    let cached = crate::cache::load(&cache, &instrument.path, 0).unwrap();
    assert_eq!(
        cached.groups[0].pitch_lfos[0],
        instrument.groups[0].pitch_lfos[0]
    );
    let whole = performance_mode(&cache, &instrument, &[128], false);
    let split = performance_mode(&cache, &instrument, &[1, 7, 31, 127, 13, 64], false);
    assert!(
        whole
            .iter()
            .flatten()
            .zip(split.iter().flatten())
            .all(|(a, b)| (a - b).abs() < 3e-5)
    );
}
