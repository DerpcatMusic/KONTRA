use super::{Edits, Override, Param};
use crate::{
    audio::Sample,
    engine::{Bank, Engine},
    import::{Group, Zone},
};

fn envelope(sustain: f32) -> crate::modulation::Ahdsr {
    crate::modulation::Ahdsr {
        attack_ms: 0.,
        hold_ms: 0.,
        decay_ms: 0.,
        release_ms: 0.,
        attack_curve: 0.,
        sustain,
        unknown_flag: 0,
        unknown_tail: Vec::new(),
    }
}

fn engines() -> (Engine, Engine) {
    let source = Sample {
        rate: 48_000,
        frames: (0..224)
            .map(|i| {
                [
                    (((i * 17) % 71) as f32 - 35.) / 128.,
                    (((i * 11) % 61) as f32 - 30.) / 128.,
                ]
            })
            .collect(),
    };
    let actual = Bank::from_samples(
        vec![Group {
            volume_env: Some(envelope(0.4)),
            ..Default::default()
        }],
        vec![Zone::default()],
        vec![(
            Default::default(),
            Sample {
                rate: source.rate,
                frames: source.frames.clone(),
            },
        )],
    )
    .unwrap();
    // Literal independently authored sustain levels for successive notes:
    // valid +.2, three rejected replacements, valid -.1, then clear.
    let levels = [0.6, 0.6, 0.6, 0.6, 0.3, 0.4];
    let groups = levels
        .into_iter()
        .map(|sustain| Group {
            volume_env: Some(envelope(sustain)),
            ..Default::default()
        })
        .collect();
    let zones = (0..6)
        .map(|group| {
            let key = 60 + group as u8;
            Zone {
                group,
                root: key,
                low_key: key,
                high_key: key,
                ..Default::default()
            }
        })
        .collect();
    let reference = Bank::from_samples(groups, zones, vec![(Default::default(), source)]).unwrap();
    let prepare = |bank| {
        let mut engine = Engine::default();
        engine.reset(48_000.);
        engine.set_bank(Some(Box::new(bank)));
        engine
    };
    (prepare(actual), prepare(reference))
}

fn performance(sizes: &[usize]) -> [[f32; 2]; 1536] {
    let (mut actual, mut reference) = engines();
    let (mut pcm, mut expected) = ([[0.; 2]; 1536], [[0.; 2]; 1536]);
    let update = |offset| Override {
        group: Some(0),
        param: Param::Sustain,
        offset,
    };
    assert_eq!(
        crate::plugin::tests::allocations(|| {
            actual.set_override(update(0.2));
            for stage in 0..6 {
                if (1..=3).contains(&stage) {
                    let invalid = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY][stage - 1];
                    let revision = actual.player.midi.revision;
                    let sustain = actual.state.settings[0].envelope.unwrap().sustain;
                    actual.set_override(update(invalid));
                    assert_eq!(actual.overrides.get(Some(0), Param::Sustain), 0.2);
                    assert_eq!(actual.state.settings[0].envelope.unwrap().sustain, sustain);
                    assert_eq!(
                        actual.player.midi.revision, revision,
                        "rejection must not invalidate live voices"
                    );
                } else if stage == 4 {
                    actual.set_override(update(-0.1));
                } else if stage == 5 {
                    actual.clear_overrides();
                }
                actual.note_on(0, 60, 127);
                reference.note_on(0, 60 + stage as u8, 127);
                let (mut at, mut block) = (stage * 256, 0);
                while at < (stage + 1) * 256 {
                    let n = sizes[block % sizes.len()].min((stage + 1) * 256 - at);
                    let (mut l, mut r, mut el, mut er) =
                        ([0.; 128], [0.; 128], [0.; 128], [0.; 128]);
                    actual.render_checked(&mut l[..n], &mut r[..n]).unwrap();
                    reference
                        .render_checked(&mut el[..n], &mut er[..n])
                        .unwrap();
                    for i in 0..n {
                        pcm[at + i] = [l[i], r[i]];
                        expected[at + i] = [el[i], er[i]];
                    }
                    at += n;
                    block += 1;
                }
            }
            assert_eq!(actual.active_voices(), 0);
            assert_eq!(reference.active_voices(), 0);
        }),
        0
    );
    let error = pcm
        .iter()
        .flatten()
        .zip(expected.iter().flatten())
        .map(|(a, b)| (a - b).abs())
        .fold(0_f32, f32::max);
    assert!(
        error < 2e-6,
        "accepted override/reference PCM error={error}"
    );
    for stage in 0..6 {
        assert!(
            pcm[stage * 256..stage * 256 + 200]
                .iter()
                .flatten()
                .any(|x| x.abs() > 0.01),
            "every rejected replacement must preserve an audible prior override"
        );
        assert!(pcm[stage * 256 + 224..(stage + 1) * 256]
            .iter()
            .flatten()
            .all(|x| *x == 0.));
    }
    pcm
}

#[test]
fn invalid_overrides_preserve_live_authored_pcm_and_allow_later_retarget_without_heap() {
    // The shared admission owner also refuses a new identity: no inert row is
    // stored by the editor/worker Edits caller when it reports no change.
    let mut edits = Edits::default();
    for offset in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert!(!edits.set(Override {
            group: None,
            param: Param::Sustain,
            offset
        }));
        assert!(edits.0.is_empty());
    }
    let whole = performance(&[128]);
    let irregular = performance(&[1, 7, 31, 127]);
    let error = whole
        .iter()
        .flatten()
        .zip(irregular.iter().flatten())
        .map(|(a, b)| (a - b).abs())
        .fold(0_f32, f32::max);
    assert!(
        error < 2e-6,
        "host fragmentation changed accepted override PCM: {error}"
    );
}
