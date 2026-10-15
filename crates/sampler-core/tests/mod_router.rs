//! The modulation router: any source reaches any registered node parameter with
//! its native law (Kontakt law compiler FUN_140873040, docs/re/NATIVE_ORACLE.md 3).
use sampler_core::v1_voice_controls::LfoFrequency;
use sampler_core::*;
mod support;

fn limits() -> Limits {
    Limits {
        notes: 4,
        channels: 0,
        performances: 1,
        families: 4,
        voices: 4,
        expressions: 4,
        decisions: 0,
        commands: 8,
        behaviors: 0,
        behavior_cells: 0,
        behavior_fuel: 0,
        note_cells: 0,
    }
}
fn input(id: i32) -> Input {
    Input {
        protocol: Protocol::Clap,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: Some(id),
    }
}
/// Frame k holds k / 1000 on the left, so left / (k / 1000) is the voice gain.
fn plan(frames: usize) -> Prepared {
    let pcm = (0..frames).map(|k| [k as f32 / 1000., 0.5]).collect();
    Prepared::new(
        48000,
        vec![Pcm::new(48000, pcm).unwrap()],
        vec![Region {
            sample: 0,
            key_low: 60,
            key_high: 60,
            root_key: None,
            velocity_low: 0.,
            velocity_high: 1.,
            gain: 1.,
            envelope: Envelope::default(),
            playback: Playback::default(),
        }],
        1,
    )
    .unwrap()
    .with_velocity_curves(vec![VelocityCurve::Constant])
    .unwrap()
}
fn modulated(sources: Vec<ModSource>, routes: Vec<ModRoute>) -> Prepared {
    plan(8192)
        .with_voice_modulation(
            vec![ModProgram {
                controls: vec![],
                breakpoints: vec![],
                sources,
                routes,
                shapes: vec![],
                envelope_controls: vec![],
            }],
            vec![Some(0)],
            vec![0],
        )
        .unwrap()
}
fn lfo(hz: f64, retrigger: bool) -> ModSource {
    ModSource::Lfo(Lfo {
        shape: LfoShape::Sine,
        rate: LfoRate::Hertz(hz),
        phase: 0.,
        delay: 0,
        fade: 0,
        retrigger,
        shared: false,
    })
}
/// The normalized frequency-lane point whose native rate is `hz`.
fn lane_of(hz: f64) -> f64 {
    let (mut low, mut high) = (0f64, 1f64);
    for _ in 0..60 {
        let mid = (low + high) / 2.;
        if LfoFrequency::hertz(mid as f32) < hz {
            low = mid;
        } else {
            high = mid;
        }
    }
    low
}
fn gains(rt: &mut Runtime, frames: usize, from: usize) -> Vec<f64> {
    let mut audio = vec![[0.; 2]; frames];
    support::without_heap(|| {
        for chunk in audio.chunks_mut(64) {
            rt.render(chunk).unwrap();
        }
    });
    audio
        .iter()
        .enumerate()
        .map(|(k, f)| f64::from(f[0]) / ((from + k).max(1) as f64 / 1000.))
        .collect()
}
fn start(p: Prepared, controller: u32) -> Runtime {
    let mut rt = Runtime::new(p, limits()).unwrap();
    support::without_heap(|| {
        rt.set_controller(rt.performance(0).unwrap(), 1, controller)
            .unwrap();
        rt.trigger(input(1), 60, 1.).unwrap();
    });
    rt
}

/// ID26 sits in the compiler's additive set, so the lane moves by
/// `intensity * source` from the saved point and the rate follows the native
/// exponential (0x140b07bd9).
#[test]
fn lfo_rate_route_adds_to_the_normalized_frequency_lane() {
    for retrigger in [false, true] {
        let saved = 2.;
        let depth = 0.1;
        let routed = modulated(
            vec![lfo(saved, retrigger), ModSource::Controller(1)],
            vec![
                ModRoute::new(0, ModTarget::Attenuate, 1.),
                ModRoute::new(1, ModTarget::SourceRate(0), depth),
            ],
        );
        let expected_hz = LfoFrequency::hertz((lane_of(saved) + depth) as f32);
        let reference = modulated(
            vec![lfo(expected_hz, true)],
            vec![ModRoute::new(0, ModTarget::Attenuate, 1.)],
        );
        let a = gains(&mut start(routed, u32::MAX), 2048, 0);
        let b = gains(&mut start(reference, 0), 2048, 0);
        let worst = a.iter().zip(&b).map(|(a, b)| (a - b).abs()).fold(0., f64::max);
        assert!(worst < 1e-4, "retrigger {retrigger}: {worst}");
    }
}

/// A rate change moves the LFO's phase from where it is: the output has no step.
#[test]
fn lfo_rate_change_keeps_the_phase_continuous() {
    for retrigger in [false, true] {
        let p = modulated(
            vec![lfo(2., retrigger), ModSource::Controller(1)],
            vec![
                ModRoute::new(0, ModTarget::Attenuate, 1.),
                ModRoute::new(1, ModTarget::SourceRate(0), 0.1),
            ],
        );
        let mut rt = start(p, 0);
        let mut g = gains(&mut rt, 3840, 0);
        support::without_heap(|| {
            rt.set_controller(rt.performance(0).unwrap(), 1, u32::MAX)
                .unwrap();
        });
        g.extend(gains(&mut rt, 3840, 3840));
        let step = g[2..].windows(2).map(|w| (w[1] - w[0]).abs()).fold(0., f64::max);
        assert!(step < 0.005, "retrigger {retrigger}: step {step}");
    }
}

/// ID25 acts on `$intensity` only: the target's own lag, a shaped assignment and
/// the stored depth sign (flag 0x02) all combine through the same expression.
#[test]
fn intensity_scale_admits_shaped_signed_assignments_and_lagged_routes() {
    let program = ModProgram {
        controls: vec![],
        breakpoints: vec![],
        sources: vec![ModSource::Key, ModSource::Constant],
        routes: vec![ModRoute {
            lag: 480,
            scale: Some(ModScale {
                lag: 0,
                source: 1,
                shape: Some(0),
                law: ModScaleLaw::KontaktIntensity { depth: -0.5, flags: 0x12, unit: 1. },
            }),
            ..ModRoute::new(0, ModTarget::Attenuate, 0.8)
        }],
        shapes: vec![vec![(0., 0.5), (1., 0.5)]],
        envelope_controls: vec![],
    };
    let p = plan(8192)
        .with_voice_modulation(vec![program], vec![Some(0)], vec![0])
        .unwrap();
    let g = gains(&mut start(p, 0), 1024, 0);
    // 0.8 * (1 - (1 - 0.5) * -0.5) = 1.0 of the way to full attenuation.
    assert!((g[900] - 60. / 127.).abs() < 1e-3, "{}", g[900]);
}

/// The lane's rate law is the native parameter-713 codec: the retained landmarks
/// (docs: specs/modulators-lfo-part.md) hold for `LfoFrequency::hertz`.
#[test]
fn frequency_lane_matches_the_native_codec_landmarks() {
    for (slider, hz) in [(0., 0.0104154348), (0.25, 0.124558449), (0.5, 1.49068451), (0.75, 17.7412109), (1., 213.142578)] {
        let got = LfoFrequency::hertz(slider);
        assert!((got - hz).abs() < hz * 1e-6, "{slider}: {got} vs {hz}");
    }
}

fn intensity(depth: f64) -> ModScaleLaw {
    ModScaleLaw::KontaktIntensity { depth, flags: 0x10, unit: 1. }
}

/// Two assignments to one lane chain: each template's `$in` is the previous result.
#[test]
fn intensity_assignments_chain_on_the_lane() {
    let scale = |shape, depth| ModScale { lag: 0, source: 1, shape: Some(shape), law: intensity(depth) };
    let program = ModProgram {
        controls: vec![],
        breakpoints: vec![],
        sources: vec![ModSource::Key, ModSource::Constant],
        routes: vec![ModRoute {
            scale: Some(scale(0, 0.5)),
            more_scales: [Some(scale(1, 0.5)), None, None, None, None, None, None, None],
            ..ModRoute::new(0, ModTarget::Attenuate, 0.8)
        }],
        shapes: vec![vec![(0., 0.5), (1., 0.5)], vec![(0., 0.2), (1., 0.2)]],
        envelope_controls: vec![],
    };
    let p = plan(8192)
        .with_voice_modulation(vec![program], vec![Some(0)], vec![0])
        .unwrap();
    let g = gains(&mut start(p, 0), 1024, 0);
    // 0.8 * (1 - 0.5 * 0.5) * (1 - 0.8 * 0.5) = 0.36 of the way to attenuation.
    let key = 60. / 127.;
    assert!((g[900] - (1. - 0.36 * (1. - key))).abs() < 1e-3, "{}", g[900]);
}

/// An assignment's lag smooths its processed source before the law.
#[test]
fn intensity_assignment_lag_smooths_its_source() {
    let range = |lag| {
        let program = ModProgram {
            controls: vec![],
            breakpoints: vec![],
            sources: vec![ModSource::Key, lfo(5., true)],
            routes: vec![ModRoute {
                scale: Some(ModScale { lag, source: 1, shape: None, law: intensity(1.) }),
                ..ModRoute::new(0, ModTarget::Attenuate, 1.)
            }],
            shapes: vec![],
            envelope_controls: vec![],
        };
        let p = plan(8192)
            .with_voice_modulation(vec![program], vec![Some(0)], vec![0])
            .unwrap();
        let g = gains(&mut start(p, 0), 4800, 0);
        let tail = &g[2400..];
        tail.iter().cloned().fold(f64::MIN, f64::max) - tail.iter().cloned().fold(f64::MAX, f64::min)
    };
    let (plain, lagged) = (range(0), range(4800));
    assert!(plain > 0.1 && lagged < plain * 0.6, "{plain} {lagged}");
}

/// A Kontakt external volume route with a proved source type runs through the
/// native `prepare_source`/`combine` laws (voice_mod/laws.rs) and, on the
/// modeled H target, agrees with the generic path; a 128-sample table is read
/// at 127 steps. Unproved source types (keyPos, 7) keep the generic path.
#[test]
fn native_routes_agree_with_the_generic_path_on_modeled_targets() {
    let table: Vec<(f64, f64)> =
        (0..128).map(|k| (k as f64 / 127., 1. - (k as f64 / 127.) * 0.5)).collect();
    let run = |native: Option<ModNative>, shape: Option<usize>, invert: bool| {
        let program = ModProgram {
            controls: vec![],
            breakpoints: vec![],
            sources: vec![ModSource::Constant],
            routes: vec![ModRoute { shape, invert, native, ..ModRoute::new(0, ModTarget::Attenuate, 0.8) }],
            shapes: vec![table.clone()],
            envelope_controls: vec![],
        };
        let p = plan(8192).with_voice_modulation(vec![program], vec![Some(0)], vec![0]).unwrap();
        gains(&mut start(p, 0), 1024, 0)[900]
    };
    let id = |source_type| Some(ModNative { target: 2, flags: 0, source_type });
    for (shape, invert) in [(None, false), (None, true), (Some(0), false)] {
        let generic = run(None, shape, invert);
        assert!((run(id(8), shape, invert) - generic).abs() < 1e-6, "{shape:?} {invert}");
        assert_eq!(run(id(7), shape, invert), generic);
    }
}

/// The semantic source identities stay fixed while their dense indices move.
/// Sine sources do not depend on the index-derived random seed.
fn rate_scale_graph(order: [usize; 4], additional: bool, chained: bool, transitive: bool, native: bool) -> Prepared {
    let mut first = match lfo(3., true) {
        ModSource::Lfo(lfo) => lfo,
        _ => unreachable!(),
    };
    first.phase = 0.25;
    let mut second = first;
    second.rate = LfoRate::Hertz(5.);
    second.phase = 0.75;
    let semantic = [lfo(2., true), ModSource::Constant, ModSource::Lfo(first), ModSource::Lfo(second)];
    let index = |identity| order.iter().position(|&i| i == identity).unwrap();
    let law = if native {
        ModScaleLaw::KontaktIntensity { depth: 0.35, flags: 0x14, unit: 1. }
    } else {
        ModScaleLaw::Multiply
    };
    let scale = ModScale { source: index(2), shape: Some(0), lag: 173, law };
    let mut rate = ModRoute::new(index(1), ModTarget::SourceRate(index(0) as u32), 0.12);
    if additional {
        rate.more_scales[0] = Some(scale);
    } else {
        rate.scale = Some(scale);
    }
    if chained {
        // A gap before the last additional slot must not hide a dependency.
        let last = rate.more_scales.len() - 1;
        rate.more_scales[last] = Some(ModScale { source: index(3), shape: None, lag: 73, law });
    }
    let mut routes = vec![ModRoute::new(index(0), ModTarget::Attenuate, 1.), rate];
    if transitive {
        let mut rate = ModRoute::new(index(1), ModTarget::SourceRate(index(2) as u32), 0.08);
        rate.more_scales[7] = Some(ModScale { source: index(3), shape: None, lag: 101, law });
        routes.push(rate);
    }
    plan(8192)
        .with_voice_modulation(
            vec![ModProgram {
                sources: order.map(|i| semantic[i]).to_vec(),
                routes,
                shapes: vec![vec![(0., 0.15), (0.5, 0.7), (1., 0.95)]],
                ..Default::default()
            }],
            vec![Some(0)],
            vec![0],
        )
        .unwrap()
}

#[test]
fn source_rate_scale_dependencies_are_current_under_source_reordering() {
    for additional in [false, true] {
        for (chained, transitive) in [(false, false), (true, false), (false, true)] {
            for native in [false, true] {
                for partition in [1, 7, 32, 64, 127] {
                    // The reference places both independent scalers before the destination.
                    for order in [[0, 1, 2, 3], [0, 3, 1, 2], [3, 0, 2, 1], [1, 2, 3, 0]] {
                        let mut actual = start(rate_scale_graph(order, additional, chained, transitive, native), 0);
                        let mut expected = start(rate_scale_graph([3, 2, 1, 0], additional, chained, transitive, native), 0);
                        let mut a = vec![[0.; 2]; 2048];
                        let mut b = vec![[0.; 2]; 2048];
                        for id in [1, 2] {
                            support::without_heap(|| {
                                for (a, b) in a.chunks_mut(partition).zip(b.chunks_mut(partition)) {
                                    actual.render(a).unwrap();
                                    expected.render(b).unwrap();
                                }
                            });
                            for (frame, (a, b)) in a.iter().zip(&b).enumerate() {
                                assert_eq!(a.map(f32::to_bits), b.map(f32::to_bits),
                                    "order {order:?}, additional {additional}, chained {chained}, transitive {transitive}, native {native}, partition {partition}, frame {frame}");
                            }
                            support::without_heap(|| {
                                for rt in [&mut actual, &mut expected] {
                                    rt.note_off(input(id), None).unwrap();
                                    rt.render(&mut [[0.; 2]; 64]).unwrap();
                                    assert_eq!(rt.voice_count(), 0);
                                    if id == 1 { rt.trigger(input(2), 60, 1.).unwrap(); }
                                }
                            });
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn source_rate_rejects_cycles_through_primary_or_additional_scales() {
    for additional in [false, true] {
        for mutual in [false, true] {
            let mut routes = Vec::new();
            for target in 0..if mutual { 2 } else { 1 } {
                let dependency = if mutual { 1 - target } else { target };
                let scale = ModScale { source: dependency, shape: None, lag: 0, law: ModScaleLaw::Multiply };
                let mut route = ModRoute::new(2, ModTarget::SourceRate(target as u32), 0.12);
                if additional { route.more_scales[7] = Some(scale); } else { route.scale = Some(scale); }
                routes.push(route);
            }
            let result = plan(8192).with_voice_modulation(
                vec![ModProgram { sources: vec![lfo(2., true), lfo(3., true), ModSource::Constant], routes, ..Default::default() }],
                vec![Some(0)], vec![0],
            );
            assert!(matches!(result, Err(Error::InvalidInput)), "additional {additional}, mutual {mutual}");
        }
    }
}

#[test]
fn source_phase_rejects_every_scale_storage_instead_of_ignoring_it() {
    // Storage zero is the primary scale; 1..=8 are additional scale slots.
    for storage in 0..=8 {
        let mut route = ModRoute::new(1, ModTarget::SourcePhase(0), 0.25);
        let scale = ModScale { source: 2, shape: None, lag: 0, law: ModScaleLaw::Multiply };
        if storage == 0 { route.scale = Some(scale); } else { route.more_scales[storage - 1] = Some(scale); }
        let result = plan(8192).with_voice_modulation(
            vec![ModProgram { sources: vec![lfo(2., true), ModSource::Constant, ModSource::Constant], routes: vec![route], ..Default::default() }],
            vec![Some(0)], vec![0],
        );
        assert!(matches!(result, Err(Error::InvalidInput)), "phase scale storage {storage}");
    }
}

fn phase_base_plan(saved: f64, offset: Option<f64>) -> Prepared {
    let ModSource::Lfo(mut receiver) = lfo(3., true) else { unreachable!() };
    receiver.phase = saved;
    let mut routes = vec![ModRoute::new(0, ModTarget::Attenuate, 1.)];
    if let Some(depth) = offset {
        routes.push(ModRoute::new(1, ModTarget::SourcePhase(0), depth));
    }
    modulated(vec![ModSource::Lfo(receiver), ModSource::Constant], routes)
}

#[test]
fn source_phase_saved_base_reaches_pcm_across_late_onsets_reuse_and_partitions() {
    // Dyadic lanes avoid inventing a different intermediate precision policy.
    for (saved, offset, combined) in [
        (0.125, 0., 0.125), (0.125, 0.125, 0.25),
        (0.875, -0.5, 0.375), (0.5, -0.375, 0.125),
    ] {
        for partition in [1, 7, 31, 64, 127, 256] {
            for silent_before in [0, 2400] {
                let mut actual = Runtime::new(phase_base_plan(saved, Some(offset)), limits()).unwrap();
                let mut reference = Runtime::new(phase_base_plan(combined, None), limits()).unwrap();
                let mut silence = vec![[0.; 2]; silent_before];
                support::without_heap(|| {
                    actual.render(&mut silence).unwrap();
                    reference.render(&mut silence).unwrap();
                });
                let mut a = [[0.; 2]; 1024];
                let mut b = [[0.; 2]; 1024];
                for id in [1, 2] {
                    support::without_heap(|| {
                        actual.trigger(input(id), 60, 1.).unwrap();
                        reference.trigger(input(id), 60, 1.).unwrap();
                        for (a, b) in a.chunks_mut(partition).zip(b.chunks_mut(partition)) {
                            actual.render(a).unwrap(); reference.render(b).unwrap();
                        }
                    });
                    assert!(a.iter().flatten().all(|v| v.is_finite()));
                    assert!(a.iter().flatten().any(|&v| v != 0.));
                    for (a, b) in a.iter().zip(&b) { assert_eq!(a.map(f32::to_bits), b.map(f32::to_bits)); }
                    support::without_heap(|| {
                        actual.note_off(input(id), None).unwrap();
                        reference.note_off(input(id), None).unwrap();
                        actual.render(&mut [[0.; 2]; 64]).unwrap();
                        reference.render(&mut [[0.; 2]; 64]).unwrap();
                    });
                    assert_eq!(actual.voice_count(), 0); assert_eq!(reference.voice_count(), 0);
                }
            }
        }
    }
}

fn phase_base_controlled_plan(saved: f64, depth: f64, bypass: f64) -> Prepared {
    const DEPTH: ControlId = ControlId(180001);
    const BYPASS: ControlId = ControlId(180002);
    let ModSource::Lfo(mut receiver) = lfo(3., true) else { unreachable!() };
    receiver.phase = saved;
    plan(8192).with_controls(vec![
        ControlDefinition { id: DEPTH, domain: ControlDomain::Real { min: -1., max: 1. }, default: ControlValue::Real(depth) },
        ControlDefinition { id: BYPASS, domain: ControlDomain::Real { min: 0., max: 1. }, default: ControlValue::Real(bypass) },
    ]).unwrap().with_voice_modulation(vec![ModProgram {
        controls: vec![(1, [DEPTH, BYPASS])],
        sources: vec![ModSource::Lfo(receiver), ModSource::Constant],
        routes: vec![ModRoute::new(0, ModTarget::Attenuate, 1.),
            ModRoute::new(1, ModTarget::SourcePhase(0), depth)],
        ..Default::default()
    }], vec![Some(0)], vec![0]).unwrap()
}

#[test]
fn source_phase_public_controls_preserve_base_bypass_and_onset_only_consumption() {
    const DEPTH: ControlId = ControlId(180001);
    const BYPASS: ControlId = ControlId(180002);
    for partition in [1, 7, 31, 64, 127, 256] {
        let mut actual = Runtime::new(phase_base_controlled_plan(0.375, 0.125, 0.), limits()).unwrap();
        let owner = actual.active_plan();
        let mut now = 2400usize;
        let mut silent = vec![[0.; 2]; now];
        support::without_heap(|| actual.render(&mut silent).unwrap());
        for (id, depth, bypass, combined) in [
            (1, 0.125, 0., 0.5),
            (2, -0.25, 0., 0.125),
            (3, -0.25, 1., 0.375),
        ] {
            let mut reference = Runtime::new(phase_base_plan(combined, None), limits()).unwrap();
            let mut prefix = vec![[0.; 2]; now];
            support::without_heap(|| {
                reference.render(&mut prefix).unwrap();
                actual.edit_controls(owner, None, &[
                    ControlWrite { id: DEPTH, value: ControlValue::Real(depth) },
                    ControlWrite { id: BYPASS, value: ControlValue::Real(bypass) },
                ]).unwrap();
                actual.trigger(input(id), 60, 1.).unwrap();
                reference.trigger(input(id), 60, 1.).unwrap();
            });
            assert_eq!(actual.control_value(owner, DEPTH), Ok(ControlValue::Real(depth)));
            assert_eq!(actual.control_value(owner, BYPASS), Ok(ControlValue::Real(bypass)));
            for segment in 0..2 {
                if segment == 1 {
                    // Stored controls are live; a held cycle has consumed phase.
                    support::without_heap(|| {
                        actual.edit_controls(owner, None, &[
                            ControlWrite { id: DEPTH, value: ControlValue::Real(0.25) },
                            ControlWrite { id: BYPASS, value: ControlValue::Real(1. - bypass) },
                        ]).unwrap();
                    });
                }
                let mut a = [[0.; 2]; 512];
                let mut b = [[0.; 2]; 512];
                support::without_heap(|| {
                    for (a, b) in a.chunks_mut(partition).zip(b.chunks_mut(partition)) {
                        actual.render(a).unwrap(); reference.render(b).unwrap();
                    }
                });
                assert!(a.iter().flatten().any(|&v| v != 0.));
                for (a, b) in a.iter().zip(&b) { assert_eq!(a.map(f32::to_bits), b.map(f32::to_bits)); }
                now += 512;
            }
            support::without_heap(|| {
                actual.note_off(input(id), None).unwrap(); reference.note_off(input(id), None).unwrap();
                actual.render(&mut [[0.; 2]; 64]).unwrap(); reference.render(&mut [[0.; 2]; 64]).unwrap();
            });
            now += 64;
            assert_eq!(actual.voice_count(), 0); assert_eq!(reference.voice_count(), 0);
        }
    }
}

#[test]
fn source_phase_no_route_reference_pcm_receipt() {
    // Recorded before/after the seed fix as a mutation guard; no native-host claim.
    for partition in [1, 7, 31, 64, 127, 256] {
        let mut rt = Runtime::new(phase_base_plan(f64::from(0.37f32), None), limits()).unwrap();
        let mut hash = 0xcbf29ce484222325u64;
        let mut prefix = [[0.; 2]; 2400];
        support::without_heap(|| rt.render(&mut prefix).unwrap());
        for id in [1, 2] {
            let mut audio = [[0.; 2]; 1024];
            support::without_heap(|| {
                rt.trigger(input(id), 60, 1.).unwrap();
                for block in audio.chunks_mut(partition) { rt.render(block).unwrap(); }
            });
            assert!(audio.iter().flatten().any(|&v| v != 0.));
            for value in audio.iter().flatten() {
                assert!(value.is_finite());
                for byte in value.to_bits().to_le_bytes() { hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3); }
            }
            support::without_heap(|| {
                rt.note_off(input(id), None).unwrap(); rt.render(&mut [[0.; 2]; 64]).unwrap();
            });
            assert_eq!(rt.voice_count(), 0);
        }
        println!("phase_no_route_reference partition={partition} pcm_fnv1a64={hash:016x}");
    }
}
