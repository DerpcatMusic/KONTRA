//! Owned fade wiring/heap checks, not measured Kontakt clock or coefficient parity.
use sampler_core::{Instruction as I, *};
mod support;

#[test]
fn fade_curve_dispatch_and_chained_plain_render_do_not_allocate_or_free() {
    for curve in [
        FadeCurve::Linear,
        FadeCurve::EqualPower,
        FadeCurve::SCurve,
        FadeCurve::Exponential,
        FadeCurve::Logarithmic,
    ] {
        for out in [false, true] {
            for chain in [false, true] {
                for block in [1, 17, 128] {
                    let mut plan = Prepared::new(
                        48000,
                        vec![Pcm::new(48000, vec![[0.5; 2]; 512].into_boxed_slice()).unwrap()],
                        vec![Region {
                            sample: 0,
                            key_low: 60,
                            key_high: 60,
                            root_key: None,
                            velocity_low: 0.,
                            velocity_high: 1.,
                            gain: 1.,
                            envelope: Envelope::new(0, 0, 0, 1., 64).unwrap(),
                            playback: Default::default(),
                        }],
                        1,
                    )
                    .unwrap()
                    .with_velocity_curves(vec![VelocityCurve::Constant])
                    .unwrap()
                    .with_programs(
                        vec![
                            Program::new(vec![
                                I::ReadEventId { local: 0 },
                                I::SetLocal {
                                    local: 1,
                                    value: 384,
                                },
                                I::SetLocal {
                                    local: 2,
                                    value: curve as i64,
                                },
                                I::FadeEvent {
                                    event: 0,
                                    frames: 1,
                                    out,
                                    stop: out,
                                    curve: Some(2),
                                },
                                I::End,
                            ])
                            .unwrap(),
                        ],
                        None,
                    )
                    .unwrap();
                    if chain {
                        plan = plan
                            .with_voice_chains(
                                vec![
                                    VoiceChain::new(
                                        vec![Processor::Gain(1.)],
                                        vec![Processor::Gain(1.)],
                                        0,
                                    )
                                    .unwrap(),
                                ],
                                vec![Some(0)],
                            )
                            .unwrap();
                    }
                    let capacity = Limits::for_plan(&plan, 1, 1);
                    let mut rt = Runtime::new(plan, capacity).unwrap();
                    let mut audio = [[0.; 2]; 448];
                    support::without_heap(|| {
                        let note = rt
                            .trigger(
                                Input {
                                    protocol: Protocol::Native,
                                    port: 0,
                                    group: 0,
                                    channel: 0,
                                    key: 60,
                                    external_id: None,
                                },
                                60,
                                1.,
                            )
                            .unwrap();
                        assert_eq!(rt.voice_count(), 1);
                        let callback = rt.start_behavior(note, 0).unwrap();
                        assert_eq!(rt.behavior_outcome(callback), Ok(Some(Outcome::Finished)));
                        for chunk in audio.chunks_mut(block) {
                            rt.render(chunk).unwrap();
                        }
                    });
                    // Independent frame16/384 values; an unfaded or cell-linear result fails.
                    let gain = match (curve, out) {
                        (FadeCurve::Linear, false) => 0.041666666666666664,
                        (FadeCurve::Linear, true) => 0.9583333333333334,
                        (FadeCurve::EqualPower, false) => 0.06540312923014306,
                        (FadeCurve::EqualPower, true) => 0.9978589232386035,
                        (FadeCurve::SCurve, false) => 0.004277569313094809,
                        (FadeCurve::SCurve, true) => 0.9957224306869052,
                        (FadeCurve::Exponential, false) => 0.001736111111111111,
                        (FadeCurve::Exponential, true) => 0.9184027777777778,
                        (FadeCurve::Logarithmic, false) => 0.08159722222222221,
                        (FadeCurve::Logarithmic, true) => 0.9982638888888888,
                    };
                    assert!(
                        audio[15]
                            .iter()
                            .all(|v| (*v - (0.5 * gain) as f32).abs() < 1e-7),
                        "{curve:?}, out {out}, chain {chain}, block {block}: {:?}",
                        audio[15]
                    );
                    // Guard is meaningful only if the active fade actually rendered.
                    assert!(
                        audio[15]
                            .iter()
                            .all(|v| v.is_finite() && *v > 0. && *v < 0.5),
                        "{curve:?}, out {out}, chain {chain}, block {block}: {:?}",
                        audio[15]
                    );
                    assert_eq!(audio[383], [if out { 0. } else { 0.5 }; 2]);
                    assert_eq!(rt.voice_count(), usize::from(!out));
                    assert!(rt.take_fault().is_none());
                }
            }
        }
    }
}
