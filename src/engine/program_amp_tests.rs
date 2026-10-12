//! Program amplifier follows the completed program signal, with native host-window
//! decisions. The reference renderer keeps program controls at unity and center.
use super::super::params::{Address, GroupPar};
use super::super::{Bank, Engine, HostNote, HostPattern};
use crate::{
    fx::{
        Chain, Effect, Kind, ProgramFx,
        params::{Field, Params, Value},
    },
    import::{Group, Instrument, ProgramControls, Zone},
};
use ni_file::kontakt::objects::ProgramDataPrivateParams;

const END: usize = 8192;
fn root(id: i32) -> HostNote {
    HostNote {
        port: 0,
        channel: 4,
        key: 60,
        id,
        clap: true,
    }
}
fn off(n: HostNote) -> HostPattern {
    HostPattern {
        port: 0,
        channel: 4,
        key: 60,
        id: n.id,
        clap: true,
    }
}
fn effect(slot: usize, gain: f32) -> Effect {
    Effect {
        slot,
        kind: Kind::Gainer,
        version: 0,
        bypass: false,
        output_gain: 1.,
        dry_level: 0.,
        params: Params::Gainer(crate::fx::params::Gainer { gain }),
    }
}
struct Fixture {
    _dir: tempfile::TempDir,
    instrument: Instrument,
}
impl Fixture {
    fn new(nonlinear: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("program-amplifier.wav");
        let mut wav = hound::WavWriter::create(
            &path,
            hound::WavSpec {
                channels: 2,
                sample_rate: 48000,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )
        .unwrap();
        for i in 0..24000 {
            let p = std::f64::consts::TAU * i as f64 / 200.;
            let sample = (0.17 * p.sin() + 0.045 * (2. * p).sin()) as f32;
            wav.write_sample(sample).unwrap();
            wav.write_sample(sample).unwrap();
        }
        wav.finalize().unwrap();
        let private = ProgramDataPrivateParams {
            version: 0xae,
            data: [
                0u32.to_le_bytes().as_slice(),
                &[1, 1],
                12i32.to_le_bytes().as_slice(),
            ]
            .concat(),
        };
        assert!(private.controller_options().unwrap().is_some());
        let mut fx = ProgramFx::default();
        if nonlinear {
            fx.insert = Chain {
                slots: vec![Effect {
                    slot: 0,
                    kind: Kind::SurroundPanner,
                    version: 0,
                    bypass: false,
                    output_gain: 1.,
                    dry_level: 0.,
                    params: Params::Fields(vec![
                        Field {
                            name: "param_0",
                            value: Value::Number(1.),
                        },
                        Field {
                            name: "param_1",
                            value: Value::Number(0.),
                        },
                    ]),
                }],
            };
            fx.main = Chain {
                slots: vec![effect(0, 1.3)],
            };
        }
        Self {
            _dir: dir,
            instrument: Instrument {
                program: ProgramControls {
                    gain: 0.25,
                    pan: 0.5,
                    tune: 0.,
                },
                native_program_private: Some(private),
                sampling_profile: crate::import::SamplingProfile::Kontakt,
                groups: vec![Group {
                    pan: -0.75,
                    ..Default::default()
                }],
                zones: vec![Zone {
                    sample: path,
                    root: 60,
                    ..Default::default()
                }],
                fx,
                ..Default::default()
            },
        }
    }
    fn bank(&self, unity: bool) -> Bank {
        let mut i = Instrument {
            program: self.instrument.program,
            native_program_private: self.instrument.native_program_private.clone(),
            sampling_profile: self.instrument.sampling_profile,
            groups: self.instrument.groups.clone(),
            zones: self.instrument.zones.clone(),
            fx: self.instrument.fx.clone(),
            ..Default::default()
        };
        if unity {
            i.program = ProgramControls::default();
        }
        Bank::load_counting(
            &i,
            usize::MAX,
            crate::engine::Streaming::RamOnly,
            &[],
            &std::sync::atomic::AtomicU32::new(0),
        )
        .unwrap()
    }
    fn engine(&self, unity: bool) -> Engine {
        let mut e = Engine::default();
        e.reset(48000.);
        e.attack = 0.;
        e.release = 0.005;
        e.set_bank(Some(Box::new(self.bank(unity))));
        e.set_fx(self.instrument.fx.processor_for_groups(
            48000.,
            128,
            &[],
            &self.instrument.groups,
        ));
        e
    }
}

// Independent scalar post-processing of the reference's complete program PCM.
// This retains emitted values separately from target authority, and selects the
// recurrence once per declared host window or sample-timed target change.
struct Oracle {
    retained: [f32; 2],
    target: Option<[f32; 2]>,
    epoch: usize,
    moving: [bool; 2],
}
impl Oracle {
    fn new() -> Self {
        Self {
            retained: [0.; 2],
            target: None,
            epoch: usize::MAX,
            moving: [false; 2],
        }
    }
    fn frame(&mut self, input: [f32; 2], target: [f32; 2], epoch: usize, fed: bool) -> [f32; 2] {
        if !fed {
            self.target = None;
            return input;
        }
        if self.target.is_none() {
            self.retained = target;
        }
        if self.epoch != epoch || self.target != Some(target) {
            let quotient = (self.retained[0] + 1e-12).abs() / (target[0] + 1e-12).abs();
            self.moving = [
                !(0.999..=1.001).contains(&quotient),
                (self.retained[1] - target[1]).abs() > (10f32 / 48000.) * 2.,
            ];
            self.target = Some(target);
            self.epoch = epoch;
        }
        let amplitude = if self.moving[0] {
            self.retained[0]
        } else {
            target[0]
        };
        let pan = if self.moving[1] {
            self.retained[1]
        } else {
            target[1]
        };
        let out = [
            input[0] * amplitude * (1. - pan.max(0.)),
            input[1] * amplitude * (1. + pan.min(0.)),
        ];
        if self.moving[0] {
            self.retained[0] += (target[0] - self.retained[0]) * (1f32 / 1800.);
        }
        if self.moving[1] {
            let increment = if self.retained[1] <= target[1] {
                (10f32 / 48000.) * 2.
            } else {
                -20f32 / 48000.
            };
            self.retained[1] = if increment > 0. {
                (self.retained[1] + increment).min(target[1])
            } else {
                (self.retained[1] + increment).max(target[1])
            };
        }
        out
    }
}
fn perform(nonlinear: bool, sizes: &[usize]) -> Vec<[f32; 2]> {
    let fixture = Fixture::new(nonlinear);
    let (mut actual, mut reference) = (fixture.engine(false), fixture.engine(true));
    let (mut fill, mut plain_fill) = (
        Some(Box::new(fixture.bank(false))),
        Some(Box::new(fixture.bank(true))),
    );
    let mut pcm = vec![[0.; 2]; END];
    let mut oracle = Oracle::new();
    let mut target = [0.25, 0.5];
    let mut step = 0;
    assert!(actual.host_note_on(root(1), 4, 60, 127, 0.));
    assert!(reference.host_note_on(root(1), 4, 60, 127, 0.));
    for window in (0..END).step_by(256) {
        actual.begin_audio_block(256, 1, false);
        reference.begin_audio_block(256, 1, false);
        let mut at = window;
        while at < window + 256 {
            match at {
                32 => {
                    target = [2., -0.6];
                    assert!(actual.write(Address::Instrument(GroupPar::Volume), target[0]));
                    assert!(actual.write(Address::Instrument(GroupPar::Pan), target[1]));
                }
                257 => {
                    actual.cc_from(4, 4, 7, 127);
                    let x = (127f64 * 7874. * 1e-6) as f32;
                    target[0] = (x * x * x).clamp(0., 1.) * 4.;
                }
                529 => {
                    actual.cc_from(4, 4, 10, 64);
                    let x = (64f64 * 7874. * 1e-6) as f32;
                    let bipolar = (2. * x - 1.).clamp(-1., 1.);
                    target[1] = 2. * (0.5 * bipolar + 0.5) - 1.;
                }
                773 => {
                    target = [0.0625, 0.75];
                    assert!(actual.write(Address::Instrument(GroupPar::Volume), target[0]));
                    assert!(actual.write(Address::Instrument(GroupPar::Pan), target[1]));
                }
                1536 => {
                    actual.upgrade_bank(fill.take().unwrap());
                    reference.upgrade_bank(plain_fill.take().unwrap());
                }
                4096 => {
                    actual.host_note_off(off(root(1)));
                    reference.host_note_off(off(root(1)));
                }
                6144 => {
                    // A long source-free dry gap reseeds rather than catching up.
                    target = [0.2, -0.5];
                    assert!(actual.write(Address::Instrument(GroupPar::Volume), target[0]));
                    assert!(actual.write(Address::Instrument(GroupPar::Pan), target[1]));
                    assert!(actual.host_note_on(root(2), 4, 60, 127, 0.));
                    assert!(reference.host_note_on(root(2), 4, 60, 127, 0.));
                }
                6912 => {
                    actual.host_note_off(off(root(2)));
                    reference.host_note_off(off(root(2)));
                }
                _ => {}
            }
            let boundary = [32, 257, 529, 773, 1536, 4096, 6144, 6912, END]
                .into_iter()
                .filter(|x| *x > at)
                .min()
                .unwrap();
            let n = sizes[step % sizes.len()]
                .min(window + 256 - at)
                .min(boundary - at);
            let fed = nonlinear || !reference.player.voices.is_empty();
            let (mut l, mut r, mut x, mut y) = ([0.; 128], [0.; 128], [0.; 128], [0.; 128]);
            assert_eq!(
                crate::plugin::tests::allocations(|| {
                    actual.render(&mut [], &mut []);
                    reference.render(&mut [], &mut []);
                    actual.render(&mut l[..n], &mut r[..n]);
                    reference.render(&mut x[..n], &mut y[..n]);
                    actual.mark_host_notes();
                    reference.mark_host_notes();
                }),
                0
            );
            for i in 0..n {
                let expected = oracle.frame([x[i], y[i]], target, window, fed);
                pcm[at + i] = [l[i], r[i]];
                assert!(
                    (l[i] - expected[0]).abs() < 3e-6 && (r[i] - expected[1]).abs() < 3e-6,
                    "post-program scalar mismatch at {} nonlinear={nonlinear}: {:?} != {expected:?}",
                    at + i,
                    pcm[at + i]
                );
            }
            at += n;
            step += 1;
        }
    }
    for id in [1, 2] {
        assert_eq!(
            actual.host_note_end_at(root(id)),
            reference.host_note_end_at(root(id))
        );
        assert!(actual.host_note_end_at(root(id)).is_some());
    }
    // Opposing group and program pans are two balances, so an initial center
    // sum would be detectably wrong even before the first target change.
    assert!(pcm[..32].iter().any(|v| v[0].abs() > 1e-4));
    pcm
}
#[test]
fn validated_native_program_amplifier_follows_nonlinear_program_pcm_and_host_window_ramps_without_heap()
 {
    for nonlinear in [false, true] {
        let fixed = perform(nonlinear, &[128]);
        let fragmented = perform(nonlinear, &[1, 7, 31, 127]);
        assert!(
            fixed
                .iter()
                .zip(fragmented)
                .all(|(a, b)| (a[0] - b[0]).abs() < 3e-6 && (a[1] - b[1]).abs() < 3e-6)
        );
    }
}

#[test]
fn native_direct_group_output_bypasses_program_racks_and_amplifier() {
    let (processed, plain) = (Fixture::new(true), Fixture::new(false));
    for sizes in [&[128][..], &[1, 7, 31, 127][..]] {
        let (mut actual, mut reference) = (processed.engine(false), plain.engine(true));
        // OUTPUT_CHANNEL2 selects the validated direct route, beyond program
        // insert/main and amplifier. The oracle has no program processors.
        let route = Address::Group(0, GroupPar::Output);
        assert!(actual.write(route, route.decode(2)));
        assert!(reference.write(route, route.decode(2)));
        assert!(actual.host_note_on(root(9), 4, 60, 127, 0.));
        assert!(reference.host_note_on(root(9), 4, 60, 127, 0.));
        let (mut at, mut block, mut audible) = (0, 0, false);
        while at < 1024 {
            if at % 256 == 0 {
                actual.begin_audio_block(256, 1, false);
                reference.begin_audio_block(256, 1, false);
            }
            if at == 16 {
                assert!(actual.write(Address::Instrument(GroupPar::Volume), 3.));
                assert!(actual.write(Address::Instrument(GroupPar::Pan), -1.));
            }
            if at == 512 {
                actual.host_note_off(off(root(9)));
                reference.host_note_off(off(root(9)));
            }
            let boundary = [16, 256, 512, 768, 1024]
                .into_iter()
                .filter(|v| *v > at)
                .min()
                .unwrap();
            let n = sizes[block % sizes.len()].min(boundary - at);
            let (mut l, mut r, mut x, mut y) = ([0.; 128], [0.; 128], [0.; 128], [0.; 128]);
            assert_eq!(
                crate::plugin::tests::allocations(|| {
                    actual.render(&mut l[..n], &mut r[..n]);
                    reference.render(&mut x[..n], &mut y[..n]);
                    actual.mark_host_notes();
                    reference.mark_host_notes();
                }),
                0
            );
            assert!(l[..n].iter().chain(&r[..n]).all(|v| *v == 0.));
            let played = actual
                .fx()
                .direct_outs(n)
                .find(|(channel, ..)| *channel == 2);
            let plain = reference
                .fx()
                .direct_outs(n)
                .find(|(channel, ..)| *channel == 2);
            match (played, plain) {
                (Some((_, a, b)), Some((_, c, d))) => {
                    for i in 0..n {
                        audible |= a[i].abs() > 1e-4;
                        assert!(
                            (a[i] - c[i]).abs() < 2e-7 && (b[i] - d[i]).abs() < 2e-7,
                            "direct route changed frame{}",
                            at + i
                        );
                    }
                }
                (None, None) => {
                    // direct_outs enumerates fed channels, rather than keeping
                    // an output row alive after its exact source has retired.
                    assert!(
                        actual
                            .host_note_end_at(root(9))
                            .is_some_and(|end| end <= at as u64)
                    );
                    assert!(
                        reference
                            .host_note_end_at(root(9))
                            .is_some_and(|end| end <= at as u64)
                    );
                    assert!(actual.player.voices.is_empty() && reference.player.voices.is_empty());
                }
                _ => panic!("direct output2 publication differs at frame{at}"),
            }
            at += n;
            block += 1;
        }
        assert!(audible);
        assert_eq!(
            actual.host_note_end_at(root(9)),
            reference.host_note_end_at(root(9))
        );
        assert!(actual.host_note_end_at(root(9)).is_some());
    }
}

#[test]
fn native_gain_tolerance_retains_hidden_current_and_decision_until_host_window_end() {
    let fixture = Fixture::new(false);
    for sizes in [&[128][..], &[1, 7, 31, 127][..]] {
        let (mut actual, mut reference) = (fixture.engine(false), fixture.engine(true));
        assert!(actual.write(Address::Instrument(GroupPar::Volume), 1.));
        assert!(actual.write(Address::Instrument(GroupPar::Pan), 0.));
        assert!(actual.host_note_on(root(10), 4, 60, 127, 0.));
        assert!(reference.host_note_on(root(10), 4, 60, 127, 0.));
        let mut scalar = Oracle::new();
        let (mut target, mut at, mut block) = (1., 0, 0);
        while at < 2048 {
            if at % 256 == 0 {
                actual.begin_audio_block(256, 1, false);
                reference.begin_audio_block(256, 1, false);
            }
            match at {
                32 => {
                    target = 1.0005;
                    assert!(actual.write(Address::Instrument(GroupPar::Volume), target));
                }
                257 => {
                    target = 1.0015;
                    assert!(actual.write(Address::Instrument(GroupPar::Volume), target));
                }
                1536 => {
                    actual.host_note_off(off(root(10)));
                    reference.host_note_off(off(root(10)));
                }
                _ => {}
            }
            let boundary = [32, 257, 1536, 2048]
                .into_iter()
                .filter(|x| *x > at)
                .min()
                .unwrap()
                .min((at / 256 + 1) * 256);
            let n = sizes[block % sizes.len()].min(boundary - at);
            let fed = !reference.player.voices.is_empty();
            let (mut l, mut r, mut x, mut y) = ([0.; 128], [0.; 128], [0.; 128], [0.; 128]);
            assert_eq!(
                crate::plugin::tests::allocations(|| {
                    actual.render(&mut l[..n], &mut r[..n]);
                    reference.render(&mut x[..n], &mut y[..n]);
                    actual.mark_host_notes();
                    reference.mark_host_notes();
                }),
                0
            );
            for i in 0..n {
                let expected = scalar.frame([x[i], y[i]], [target, 0.], at / 256 * 256, fed);
                assert!(
                    (l[i] - expected[0]).abs() < 2e-7 && (r[i] - expected[1]).abs() < 2e-7,
                    "host-window tolerance frame{}",
                    at + i
                );
            }
            at += n;
            block += 1;
        }
        assert_eq!(
            actual.host_note_end_at(root(10)),
            reference.host_note_end_at(root(10))
        );
        assert!(actual.host_note_end_at(root(10)).is_some());
    }
}

#[test]
fn native_program_activity_uses_real_input_and_returns_then_reseeds_after_idle_without_heap() {
    // Gain-only, bypassed conservative DSP, and a tap without a return cannot
    // keep the amplifier moving through an otherwise empty interval. A real
    // quiet group tap to a Gain return does feed it, even with direct dry output.
    for variant in 0..5 {
        let mut fixture = Fixture::new(variant == 1);
        fixture.instrument.fx.main = Chain {
            slots: vec![effect(0, 1.3)],
        };
        if variant == 1 {
            fixture.instrument.fx.insert.slots[0].bypass = true;
        }
        if (2..=3).contains(&variant) {
            let tap = Effect {
                slot: 7,
                kind: Kind::SendLevels,
                version: 0,
                bypass: false,
                output_gain: 1.,
                dry_level: 0.,
                params: Params::SendLevels(crate::fx::params::SendLevels {
                    sends: vec![if variant == 3 { 1e-7 } else { 1. }],
                    outputs: Vec::new(),
                }),
            };
            if variant == 2 {
                fixture.instrument.fx.insert = Chain { slots: vec![tap] };
            } else {
                fixture.instrument.groups[0].amp_split_slot = Some(8);
                fixture.instrument.groups[0].fx = Chain { slots: vec![tap] };
                fixture.instrument.fx.send = Chain {
                    slots: vec![effect(0, 1e6)],
                };
                // The quiet send impulse is amplified only after the tap. No
                // PCM threshold may erase its actual work receipt or waveform.
                let mut wav = hound::WavWriter::create(
                    &fixture.instrument.zones[0].sample,
                    hound::WavSpec {
                        channels: 2,
                        sample_rate: 48000,
                        bits_per_sample: 32,
                        sample_format: hound::SampleFormat::Float,
                    },
                )
                .unwrap();
                for i in 0..24000 {
                    let value = if i == 0 { 0.17f32 } else { 0. };
                    wav.write_sample(value).unwrap();
                    wav.write_sample(value).unwrap();
                }
                wav.finalize().unwrap();
            }
        }
        if variant == 4 {
            fixture.instrument.fx.insert = Chain {
                slots: vec![Effect {
                    slot: 0,
                    kind: Kind::StereoModeller,
                    version: 0,
                    bypass: false,
                    output_gain: 1.,
                    dry_level: 0.,
                    params: Params::StereoModeller(crate::fx::params::StereoModeller {
                        spread: 0.6,
                        pan: 0.3,
                        pseudo_stereo: false,
                    }),
                }],
            };
        }
        let mut dry_fixture = Fixture::new(false);
        dry_fixture.instrument.fx.main = Chain {
            slots: vec![effect(0, 1.3)],
        };
        for sizes in [&[128][..], &[1, 7, 31, 127][..]] {
            let (mut actual, mut reference) = (fixture.engine(false), fixture.engine(true));
            let mut dry = (variant == 4).then(|| dry_fixture.engine(true));
            let mut transformed = false;
            if variant == 3 {
                let route = Address::Group(0, GroupPar::Output);
                for engine in [&mut actual, &mut reference] {
                    assert!(engine.write(route, route.decode(2)));
                }
            }
            let mut scalar = Oracle::new();
            let (mut target, mut at, mut block, mut audible) = ([0.25, 0.5], 0, 0, false);
            while at < 4096 {
                if at % 256 == 0 {
                    actual.begin_audio_block(256, 1, false);
                    reference.begin_audio_block(256, 1, false);
                    if let Some(dry) = &mut dry {
                        dry.begin_audio_block(256, 1, false);
                    }
                }
                match at {
                    0 | 2048 => {
                        let id = if at == 0 { 20 } else { 21 };
                        assert!(actual.host_note_on(root(id), 4, 60, 127, 0.));
                        assert!(reference.host_note_on(root(id), 4, 60, 127, 0.));
                        if let Some(dry) = &mut dry {
                            assert!(dry.host_note_on(root(id), 4, 60, 127, 0.));
                        }
                    }
                    32 => {
                        target = [2., -0.6];
                        assert!(actual.write(Address::Instrument(GroupPar::Volume), target[0]));
                        assert!(actual.write(Address::Instrument(GroupPar::Pan), target[1]));
                    }
                    512 | 2560 => {
                        let id = if at == 512 { 20 } else { 21 };
                        actual.host_note_off(off(root(id)));
                        reference.host_note_off(off(root(id)));
                        if let Some(dry) = &mut dry {
                            dry.host_note_off(off(root(id)));
                        }
                    }
                    1536 => {
                        assert!(actual.player.voices.is_empty());
                        assert!(reference.player.voices.is_empty());
                        target = [0.1, 0.75];
                        assert!(actual.write(Address::Instrument(GroupPar::Volume), target[0]));
                        assert!(actual.write(Address::Instrument(GroupPar::Pan), target[1]));
                    }
                    _ => {}
                }
                let boundary = [32, 512, 1536, 2048, 2560, 4096]
                    .into_iter()
                    .filter(|x| *x > at)
                    .min()
                    .unwrap()
                    .min((at / 256 + 1) * 256);
                let n = sizes[block % sizes.len()].min(boundary - at);
                // Independent source lifetime, not a processor activity query.
                let fed = !reference.player.voices.is_empty();
                let (mut l, mut r, mut x, mut y) = ([0.; 128], [0.; 128], [0.; 128], [0.; 128]);
                let (mut u, mut v) = ([0.; 128], [0.; 128]);
                assert_eq!(
                    crate::plugin::tests::allocations(|| {
                        actual.render(&mut l[..n], &mut r[..n]);
                        reference.render(&mut x[..n], &mut y[..n]);
                        actual.mark_host_notes();
                        reference.mark_host_notes();
                        if let Some(dry) = &mut dry {
                            dry.render(&mut u[..n], &mut v[..n]);
                            dry.mark_host_notes();
                        }
                    }),
                    0
                );
                for i in 0..n {
                    if dry.is_some() {
                        // An exterior no-Stereo renderer supplies the input to
                        // the independently specified mid/side matrix.
                        let mid = 0.5 * (u[i] + v[i]);
                        let side = 0.5 * (u[i] - v[i]) * 1.6;
                        assert!((x[i] - (mid + side) * 0.7).abs() < 2e-7);
                        assert!((y[i] - (mid - side)).abs() < 2e-7);
                        transformed |= (x[i] - u[i]).abs().max((y[i] - v[i]).abs()) > 0.001;
                    }
                    let expected = scalar.frame([x[i], y[i]], target, at / 256 * 256, fed);
                    assert!(
                        (l[i] - expected[0]).abs() < 2e-7 && (r[i] - expected[1]).abs() < 2e-7,
                        "activity variant{variant} frame{}: {:?} vs {expected:?}",
                        at + i,
                        [l[i], r[i]]
                    );
                    audible |= x[i].abs().max(y[i].abs()) > 0.001;
                    if (1536..2048).contains(&(at + i)) {
                        assert_eq!([l[i], r[i]], [0., 0.]);
                    }
                }
                at += n;
                block += 1;
            }
            assert!(audible, "real source/return must be audible");
            if variant == 4 {
                assert!(transformed, "Stereo matrix must change actual PCM");
            }
            for id in [20, 21] {
                assert_eq!(
                    actual.host_note_end_at(root(id)),
                    reference.host_note_end_at(root(id))
                );
                assert!(actual.host_note_end_at(root(id)).is_some());
                if let Some(dry) = &dry {
                    assert_eq!(
                        dry.host_note_end_at(root(id)),
                        reference.host_note_end_at(root(id))
                    );
                }
            }
        }
    }
}

#[path = "external_send_tests.rs"]
mod external_send_tests;
