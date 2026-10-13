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
                source: 1,
                shape: Some(0),
                law: ModScaleLaw::KontaktIntensity { depth: -0.5, flags: 0x12, unit: 1. },
            }),
            ..ModRoute::new(0, ModTarget::Attenuate, 0.8)
        }],
        shapes: vec![vec![(0., 0.5), (1., 0.5)]],
    };
    let p = plan(8192)
        .with_voice_modulation(vec![program], vec![Some(0)], vec![0])
        .unwrap();
    let g = gains(&mut start(p, 0), 1024, 0);
    // 0.8 * (1 - (1 - 0.5) * -0.5) = 1.0 of the way to full attenuation.
    assert!((g[900] - 60. / 127.).abs() < 1e-3, "{}", g[900]);
}
