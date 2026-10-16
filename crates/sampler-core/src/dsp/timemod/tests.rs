use super::super::{BLOCK, Planar, ProcessorState};
use super::*;
use sampler_ir::fx::timemod::{Feedback, Lfo, Line, Tap};

const LFO_OFF: Lfo = Lfo { shape: LfoShape::Sine, rate: Rate::Hz(0.), phase: 0. };

pub(super) fn run(cfg: Config, rate: u32, tempo: f64, input: &[[f64; 2]], block: usize) -> (Vec<[f64; 2]>, bool) {
    let tm = TimeMod::compile(cfg, rate).unwrap();
    let mut slab = vec![[7.; 2]; tm.frames()]; // stale contents must never be read
    let mut state = ProcessorState::default();
    let (mut out, mut fault) = (Vec::new(), false);
    for chunk in input.chunks(block.min(BLOCK)) {
        let mut planar: Planar = [[0.; BLOCK]; 2];
        for (i, f) in chunk.iter().enumerate() {
            planar[0][i] = f[0];
            planar[1][i] = f[1];
        }
        fault |= tm.process(&mut state, &mut slab, &mut planar, chunk.len(), tempo);
        out.extend((0..chunk.len()).map(|i| [planar[0][i], planar[1][i]]));
    }
    (out, fault)
}

pub(super) fn impulse(n: usize, v: [f64; 2]) -> Vec<[f64; 2]> {
    let mut x = vec![[0.; 2]; n];
    x[0] = v;
    x
}

fn line(taps: &[Tap], max_ms: f64) -> Line {
    let mut l = Line::single(DelayTime::Frames(1), max_ms);
    l.taps[..taps.len()].copy_from_slice(taps);
    l.tap_count = taps.len() as u8;
    l
}

fn wrap(l: Line, lfo: Lfo) -> Config {
    Config { address: None, line: Some(l), allpass: None, order: Order::LineThenAllpass, lfo, dry: 0., wet: 1. }
}

#[test]
fn integer_delay_impulse_and_feedback_repeats() {
    let (out, fault) = run(Config::delay(5, [[0.5, 0.], [0., 0.]], 0., 1.), 48_000, 120., &impulse(20, [1., 2.]), 64);
    assert!(!fault);
    for (i, f) in out.iter().enumerate() {
        let k = i / 5;
        let expect = if i % 5 == 0 && k >= 1 { [0.5f64.powi(k as i32 - 1), if k == 1 { 2. } else { 0. }] } else { [0., 0.] };
        assert_eq!(*f, expect, "frame {i}");
    }
}

#[test]
fn ping_pong_alternates_channels() {
    let mut l = line(&[Tap::at(DelayTime::Frames(4))], 0.);
    l.interpolation = Interpolation::Nearest;
    l.input = [[1., 0.], [0., 0.]];
    l.feedback.matrix = [[0., 0.5], [0.5, 0.]];
    let (out, _) = run(wrap(l, LFO_OFF), 48_000, 120., &impulse(20, [1., 0.]), 7);
    assert_eq!(out[4], [1., 0.]);
    assert_eq!(out[8], [0., 0.5]);
    assert_eq!(out[12], [0.25, 0.]);
    assert_eq!(out[16], [0., 0.125]);
}

#[test]
fn interpolation_kinds_are_exact_on_polynomials() {
    // Rate 1000: 3.5 ms is 3.5 frames. Linear is exact on a ramp; Hermite on a
    // ramp; Lagrange also on a cubic.
    let ramp: Vec<[f64; 2]> = (0..64).map(|n| [n as f64, -(n as f64)]).collect();
    let cubic: Vec<[f64; 2]> = (0..64).map(|n| { let t = n as f64; [t * t * t * 1e-3, 0.] }).collect();
    for (kind, tol_ramp) in [(Interpolation::Linear, 1e-12), (Interpolation::Hermite, 1e-12), (Interpolation::Lagrange, 1e-12)] {
        let mut l = line(&[Tap::at(DelayTime::Ms(3.5))], 8.);
        l.interpolation = kind;
        let (out, _) = run(wrap(l, LFO_OFF), 1000, 120., &ramp, 64);
        for n in 8..64 {
            assert!((out[n][0] - (n as f64 - 3.5)).abs() < tol_ramp, "{kind:?} {n}");
            assert!((out[n][1] + (n as f64 - 3.5)).abs() < tol_ramp);
        }
    }
    let mut l = line(&[Tap::at(DelayTime::Ms(3.25))], 8.);
    l.interpolation = Interpolation::Lagrange;
    let (out, _) = run(wrap(l, LFO_OFF), 1000, 120., &cubic, 64);
    for n in 8..64 {
        let t = n as f64 - 3.25;
        assert!((out[n][0] - t * t * t * 1e-3).abs() < 1e-9);
    }
}

#[test]
fn unwritten_ring_reads_silence_not_stale_slab() {
    let mut l = line(&[Tap::at(DelayTime::Ms(10.))], 10.);
    l.interpolation = Interpolation::Hermite;
    let (out, _) = run(wrap(l, LFO_OFF), 1000, 120., &vec![[1., 1.]; 30], 64);
    assert!(out[..9].iter().all(|f| *f == [0., 0.]), "{:?}", &out[..9]);
    assert!((out[20][0] - 1.).abs() < 1e-12);
}

#[test]
fn voices_keep_unity_gain_and_modulation_stays_in_range() {
    let mut l = line(&[Tap { time: DelayTime::Ms(8.), level: 1., pan: 0., depth_ms: 3. }], 12.);
    l.voices = 4;
    l.voice_phase = 1.;
    l.interpolation = Interpolation::Hermite;
    let lfo = Lfo { shape: LfoShape::Sine, rate: Rate::Hz(5.), phase: 0.25 };
    // DC passes at unity once the ring is full, whatever the modulation.
    let (out, fault) = run(wrap(l, lfo), 1000, 120., &vec![[1., 1.]; 600], 33);
    assert!(!fault);
    assert!(out[20..].iter().all(|f| (f[0] - 1.).abs() < 1e-9 && (f[1] - 1.).abs() < 1e-9));
    // A ramp reads back n - d(t) with d inside [8 - 3, 8 + 3] frames.
    let ramp: Vec<[f64; 2]> = (0..600).map(|n| [n as f64; 2]).collect();
    let mut l = line(&[Tap { time: DelayTime::Ms(8.), level: 1., pan: 0., depth_ms: 3. }], 12.);
    l.interpolation = Interpolation::Linear;
    let (out, _) = run(wrap(l, lfo), 1000, 120., &ramp, 64);
    let mut seen = (f64::MAX, f64::MIN);
    for n in 20..600 {
        let d = n as f64 - out[n][0];
        seen = (seen.0.min(d), seen.1.max(d));
    }
    assert!(seen.0 > 4.9 && seen.0 < 5.2 && seen.1 > 10.8 && seen.1 < 11.1, "{seen:?}");
}

#[test]
fn output_is_independent_of_block_partition() {
    let mut l = line(&[Tap { time: DelayTime::Ms(6.), level: 0.7, pan: 0.3, depth_ms: 2. }, Tap::at(DelayTime::Beats(0.01))], 20.);
    l.voices = 3;
    l.voice_phase = 0.5;
    l.voice_spread = 0.8;
    l.feedback = Feedback { matrix: [[0.3, 0.1], [0.1, 0.3]], source: 0, lowpass: 0.5, highpass: 0.1, drive: 0.5 };
    let mut cfg = wrap(l, Lfo { shape: LfoShape::Triangle, rate: Rate::Hz(3.), phase: 0.1 });
    cfg.allpass = Some(Allpass { stages: 6, law: SweepLaw::Sweep { min_hz: 200., max_hz: 3000., depth: 1. }, feedback: 0.3 });
    cfg.dry = 0.5;
    cfg.wet = 0.5;
    let input: Vec<[f64; 2]> = (0..700).map(|n| [((n * 7) % 13) as f64 - 6., ((n * 5) % 11) as f64 - 5.]).collect();
    for with_allpass in [true, false] {
        if !with_allpass {
            cfg.allpass = None;
        }
        let (whole, f0) = run(cfg, 8000, 100., &input, BLOCK);
        assert!(!f0);
        for block in [1, 7, 31] {
            assert_eq!(whole, run(cfg, 8000, 100., &input, block).0, "block {block}");
        }
    }
}

#[test]
fn synced_taps_and_rates_follow_tempo() {
    // One beat at 120 BPM is 500 ms.
    let beat = wrap(line(&[Tap::at(DelayTime::Beats(1.))], 600.), LFO_OFF);
    let ms = wrap(line(&[Tap::at(DelayTime::Ms(500.))], 600.), LFO_OFF);
    let x = impulse(2000, [1., 1.]);
    assert_eq!(run(beat, 1000, 120., &x, 64).0, run(ms, 1000, 120., &x, 64).0);
    // Native getter: 2 beats per cycle at 120 BPM is the same 1 Hz LFO.
    let mut ap = Config { address: None, line: None, allpass: Some(Allpass { stages: 4, law: SweepLaw::Legacy { depth: 0.5 }, feedback: 0.2 }), order: Order::LineThenAllpass, lfo: Lfo { shape: LfoShape::Sine, rate: Rate::Hz(1.), phase: 0.25 }, dry: 0., wet: 1. };
    let x: Vec<[f64; 2]> = (0..300).map(|n| [(n as f64 * 0.3).sin(), 0.5]).collect();
    let free = run(ap, 48_000, 120., &x, 64).0;
    ap.lfo.rate = Rate::Sync { unit: 2., speed: 1. };
    assert_eq!(run(ap, 48_000, 120., &x, 64).0, free);
    assert_eq!(run(ap, 48_000, 0.05, &x, 64).0, free, "tempo below .1 reads 120");
}

#[test]
fn phaser_filter_matches_native_four_stage_impulse() {
    let tm = TimeMod::phaser_filter([0., 0.]).unwrap();
    let mut slab = vec![[0.; 2]; tm.frames()];
    let mut state = ProcessorState::default();
    let mut block: Planar = [[0.; BLOCK]; 2];
    block[0][0] = 1.;
    block[1][0] = -0.5;
    tm.process(&mut state, &mut slab, &mut block, 4, 120.);
    for (i, e) in [0.926062942f64, -0.136394471, -0.114671811, -0.095316768].into_iter().enumerate() {
        assert!((block[0][i] - e).abs() < 2e-7 && (block[1][i] + e * 0.5).abs() < 2e-7, "{i}");
    }
}

#[test]
fn loops_stay_bounded_and_nonfinite_input_faults() {
    let mut l = line(&[Tap::at(DelayTime::Ms(3.))], 5.);
    l.feedback = Feedback { matrix: [[0.9, 0.], [0., 0.9]], source: 0, lowpass: 0.7, highpass: 0.05, drive: 4. };
    let loud = vec![[50., -50.]; 500];
    let (out, fault) = run(wrap(l, LFO_OFF), 1000, 120., &loud, 64);
    assert!(!fault && out.iter().all(|f| f[0].abs() < 600. && f[1].abs() < 600.));
    let mut bad = loud;
    bad[10][0] = f64::NAN;
    assert!(run(wrap(l, LFO_OFF), 1000, 120., &bad, 64).1);
}

#[test]
fn right_channel_scales_the_delay_time() {
    let mut l = line(&[Tap::at(DelayTime::Frames(4))], 0.);
    l.interpolation = Interpolation::Nearest;
    l.right_scale = 2.;
    let (out, _) = run(wrap(l, LFO_OFF), 48_000, 120., &impulse(12, [1., 1.]), 5);
    assert_eq!(out[4], [1., 0.]);
    assert_eq!(out[8], [0., 1.]);
}

fn legacy(time: f32, unit: f32, feedback: f32, damping: f32, pan: f32) -> Config {
    let mut l = Line::single(DelayTime::Ms(f64::from(time)), 2900.);
    l.native = sampler_ir::fx::timemod::Native::LegacyDelay { time, unit, feedback, damping, pan };
    wrap(l, LFO_OFF)
}

#[test]
fn legacy_delay_native_vector_echoes_and_ping_pong() {
    // 48 kHz, 0.1 ms = 4 frames. The wet output is the lagging low-pass state, so
    // the first echo (input * feedback * 0.98) appears at D + 1 and repeats every D.
    let g = 0.5f32 * 0.98;
    let (out, fault) = run(legacy(0.1, 0., 0.5, 0., 0.), 48_000, 120., &impulse(24, [1., 0.]), 7);
    assert!(!fault);
    for (i, f) in out.iter().enumerate() {
        let expect = match i {
            5 => g,
            10 => g * g,
            15 => g * g * g,
            20 => g * g * g * g,
            _ => 0.,
        };
        assert!((f[0] - f64::from(expect)).abs() < 1e-7 && f[1] == 0., "frame {i}: {f:?}");
    }
    // Pan 1: the right input is dropped and every echo crosses to the other side.
    let (out, _) = run(legacy(0.1, 0., 0.5, 0., 1.), 48_000, 120., &impulse(24, [1., 0.]), 64);
    assert!((out[5][0] - f64::from(g)).abs() < 1e-7 && out[5][1] == 0.);
    assert!(out[10][0] == 0. && (out[10][1] - f64::from(g * g)).abs() < 1e-7);
    assert!((out[15][0] - f64::from(g * g * g)).abs() < 1e-7 && out[15][1] == 0.);
    // Damping 1 is a 0.3 one-pole in the loop: the first echo is spread, not a spike.
    let (out, _) = run(legacy(0.1, 0., 0.5, 1., 0.), 48_000, 120., &impulse(24, [1., 0.]), 64);
    assert!((out[5][0] - f64::from(g * 0.3)).abs() < 1e-7 && out[6][0] > 0.);
}

#[test]
fn legacy_delay_synced_time_follows_tempo() {
    // unit 1, time 1: 60 / 120 s * 1000 = 500 ms = 24000 frames at 48 kHz.
    let mut x = vec![[0.; 2]; 24_010];
    x[0] = [1., 0.];
    let (out, _) = run(legacy(1., 1., 1., 0., 0.), 48_000, 120., &x, 4096);
    let first = out.iter().position(|f| f[0] != 0.).unwrap();
    assert_eq!(first, 24_001);
    // Tempo below 0.1 BPM reads as 120.
    let (out, _) = run(legacy(1., 1., 1., 0., 0.), 48_000, 0., &x, 4096);
    assert_eq!(out.iter().position(|f| f[0] != 0.).unwrap(), 24_001);
    // A synced count below one unit reads as one (0x140a74bf0, flag +0x20).
    let (out, _) = run(legacy(0.25, 1., 1., 0., 0.), 48_000, 120., &x, 4096);
    assert_eq!(out.iter().position(|f| f[0] != 0.).unwrap(), 24_001);    // Count 6 of half notes (factor 2) is 6 s at 120 BPM: clamped to 5800 ms.
    let mut x = vec![[0.; 2]; 255_790];
    x[0] = [1., 0.];
    let (out, _) = run(legacy(6., 2., 1., 0., 0.), 44_100, 120., &x, 4096);
    assert_eq!(out.iter().position(|f| f[0] != 0.).unwrap(), 255_781);
}

fn flanger(color: f32, depth: f32, feedback: f32, hz: f32) -> Config {
    let mut l = Line::single(DelayTime::Ms(f64::from(color)), 6.);
    l.native = Native::LegacyModDelay {
        base_ms: color,
        excursion: f64::from(depth) * 50.,
        feedback: feedback * 0.95,
        second_head: false,
        smoothing: 0.,
    };
    wrap(l, Lfo { shape: LfoShape::Sine, rate: Rate::Hz(hz), phase: 0.25 })
}

fn chorus(depth: f32, hz: f32, phase: f32) -> Config {
    let mut l = Line::single(DelayTime::Frames(1), 0.);
    l.native = Native::LegacyModDelay {
        base_ms: 0.,
        excursion: f64::from(depth) * 75.,
        feedback: 0.,
        second_head: true,
        smoothing: 0.95,
    };
    wrap(l, Lfo { shape: LfoShape::Sine, rate: Rate::Hz(hz), phase })
}

#[test]
fn legacy_chorus_native_vector_two_smoothed_heads() {
    // Base 0 frames (unwritten +8), so the excursion clamps to -1 whatever the depth.
    // Head A at LFO sin 0 reads one frame back; head B's LFO starts at
    // wrap(1 + 0.015) -> sin = -0.094108224, reading t = 0.09423828 past it
    // (f32 position 4096.094). Values from f32 emulation of 0x140aa0510.
    let x = impulse(8, [1., 1.]);
    for depth in [0., 1.] {
        let (out, fault) = run(chorus(depth, 0., 0.), 48_000, 120., &x, 64);
        assert!(!fault);
        assert!((out[0][0] - 0.093_347_23).abs() < 1e-7, "{:?}", out[0]);
        assert!((out[1][0] - 1.851_864_9).abs() < 1e-6, "{:?}", out[1]);
        assert_eq!(out[0][0], out[0][1], "phase 0: both channels alike");
    }
    // Partition independence up to the per-block f32 origin rebase.
    let x: Vec<[f64; 2]> = (0..3000).map(|i| [(i as f64 * 0.05).sin(), (i as f64 * 0.031).cos()]).collect();
    let (a, _) = run(chorus(0.7, 2., 1.), 48_000, 120., &x, 5);
    let (b, _) = run(chorus(0.7, 2., 1.), 48_000, 120., &x, 64);
    assert!(a.iter().zip(&b).all(|(p, q)| (p[0] - q[0]).abs() < 2e-3 && (p[1] - q[1]).abs() < 2e-3));
    assert!(a.iter().flatten().all(|v| v.abs() < 2.5), "two heads, Lagrange overshoot");
}

#[test]
fn legacy_flanger_native_vector_delay_and_negative_feedback() {
    // 1 ms at 48 kHz = 48 frames; the read sits one frame further (position - 1).
    let mut x = vec![[0.; 2]; 200];
    x[0] = [1., 0.5];
    let (out, fault) = run(flanger(1., 0., 1., 0.), 48_000, 120., &x, 64);
    assert!(!fault);
    let fb = 1.0f32 * 0.95;
    for (i, f) in out.iter().enumerate() {
        let expect = match i {
            49 => [1., 0.5],
            98 => [-f64::from(fb), -0.5 * f64::from(fb)],
            147 => [f64::from(fb * fb), 0.5 * f64::from(fb * fb)],
            196 => [-f64::from(fb * fb * fb), -0.5 * f64::from(fb * fb * fb)],
            _ => [0., 0.],
        };
        assert!((f[0] - expect[0]).abs() < 1e-6 && (f[1] - expect[1]).abs() < 1e-6, "frame {i}: {f:?}");
    }
}

#[test]
fn legacy_flanger_is_partition_independent_and_stays_bounded() {
    let x: Vec<[f64; 2]> = (0..2000).map(|i| [(i as f64 * 0.05).sin(), (i as f64 * 0.031).cos()]).collect();
    let (a, fault) = run(flanger(3., 0.8, 0.9, 1.5), 48_000, 120., &x, 7);
    let (b, _) = run(flanger(3., 0.8, 0.9, 1.5), 48_000, 120., &x, 64);
    // The native float position origin is rebased per block, so partitions agree to f32 rounding.
    assert!(!fault);
    assert!(a.iter().zip(&b).all(|(p, q)| (p[0] - q[0]).abs() < 2e-3 && (p[1] - q[1]).abs() < 2e-3));
    assert!(a.iter().flatten().all(|v| v.abs() < 10.));
    assert!(a.iter().any(|f| f[0] != f[1]), "the second channel's LFO is phase offset");
}

fn uvi_lfo(wave: sampler_ir::fx::timemod::UviWave, direct: bool, hz: f32, phase: f32) -> Lfo {
    Lfo { shape: LfoShape::Uvi { wave, direct }, rate: Rate::Hz(hz), phase }
}

#[test]
fn uvi_table_lfo_vectors() {
    use sampler_ir::fx::timemod::UviWave;
    // 0.8 Hz at 48 kHz: both increment forms give 71582 (0x1412e02a0 / 0x1412eeec0).
    assert_eq!((uvi_increment(0.8, 48_000., false), uvi_increment(0.8, 48_000., true)), (71_582, 71_582));
    let tri = uvi_table(UviWave::Triangle);
    assert_eq!([uvi_read(tri, 0), uvi_read(tri, 0x4000_0000), uvi_read(tri, 0x8000_0000), uvi_read(tri, 0xc000_0000)], [0., 1., 0., -1.]);
    assert_eq!(uvi_read(tri, 0x2000_0000), 0.5);
    let sine = uvi_table(UviWave::Sine);
    assert!((uvi_read(sine, 0x4000_0000) - 1.).abs() < 1e-7 && uvi_read(sine, 0) == 0.);
    assert_eq!(uvi_phase(0.25), 0x4000_0000);
}

#[test]
fn uvi_chorus_native_vector_two_summed_voices() {
    // DelayTime 0.2, Depth 0, Mix 1 at 48 kHz: taps at 480 and 672 frames, unity each.
    let mut l = Line::single(DelayTime::Ms(10.), 68.);
    l.voices = 2;
    l.voice_phase = 1.;
    l.voice_delay_ms = 4.;
    let cfg = Config { dry: 1., wet: 2., ..wrap(l, uvi_lfo(sampler_ir::fx::timemod::UviWave::Triangle, false, 0.8, 0.25)) };
    let (out, fault) = run(cfg, 48_000, 120., &impulse(700, [1., 1.]), 64);
    assert!(!fault);
    for (i, f) in out.iter().enumerate() {
        let expect = if matches!(i, 0 | 480 | 672) { 1. } else { 0. };
        assert!((f[0] - expect).abs() < 1e-12 && (f[1] - expect).abs() < 1e-12, "frame {i}: {f:?}");
    }
}

#[test]
fn uvi_phaser_and_phasor_coefficient_vectors() {
    use sampler_ir::fx::timemod::UviWave;
    // Phaser, Min = Max = 1000 Hz at 48 kHz: a = (1 - 1/24) / (1 + 1/24) = 0.92; six
    // stages answer a^6 at the first frame, added to the dry input.
    let phaser = Config {
        address: None,
        line: None,
        allpass: Some(Allpass { stages: 6, law: SweepLaw::Linear { min_hz: 1000., max_hz: 1000. }, feedback: 0. }),
        order: Order::LineThenAllpass,
        lfo: uvi_lfo(UviWave::Sine, true, 0.8, 0.),
        dry: 1.,
        wet: 1.,
    };
    let (out, _) = run(phaser, 48_000, 120., &impulse(4, [1., 1.]), 64);
    let a = (1. - 1. / 24f32) / (1. / 24f32 + 1.);
    assert!((out[0][0] - (1. + f64::from(a).powi(6))).abs() < 1e-6, "{:?}", out[0]);
    // Phasor defaults (Order 3, 200..3000 Hz) at the initial m = 0.5: UVI a = -0.90764648.
    let phasor = Config {
        allpass: Some(Allpass { stages: 6, law: SweepLaw::DelayDomain { min_hz: 200., max_hz: 3000., order: 3 }, feedback: 0. }),
        lfo: uvi_lfo(UviWave::Triangle, false, 0.3, 0.25),
        dry: 0.,
        ..phaser
    };
    let (out, _) = run(phasor, 48_000, 120., &impulse(4, [1., 0.]), 64);
    assert!((out[0][0] - 0.907_646_48f64.powi(6)).abs() < 1e-5, "{:?}", out[0]);
}

/// UVI reads: TrackDelay's `ceil(seconds·rate)` (0.01049 s at 48 kHz is 504
/// frames, 0x14133a2e0) and ring mode 0's floor (0x1416485a0).
#[test]
fn floor_and_ceil_reads_land_on_whole_frames() {
    for (kind, frame) in [(Interpolation::Ceil, 504), (Interpolation::Floor, 503)] {
        let mut l = line(&[Tap::at(DelayTime::Ms(10.49))], 20.);
        l.interpolation = kind;
        let (out, _) = run(wrap(l, LFO_OFF), 48_000, 120., &impulse(600, [1., 1.]), 64);
        let hits: Vec<usize> = (0..600).filter(|&i| out[i][0] != 0.).collect();
        assert_eq!(hits, [frame], "{kind:?}");
    }
}

#[test]
fn addressed_phaser_filter_exposes_cutoff_and_resonance_lanes() {
    use sampler_ir::ProcessorParameter as P;
    let address = sampler_ir::SlotAddress { group: 0, slot: 0, generic: -1 };
    let mut cfg = Config::phaser_filter(0.25, 0.5, Some(address));
    let lanes = cfg.lanes();
    assert_eq!(lanes.iter().map(|l| l.role).collect::<Vec<_>>(), [P::Cutoff, P::Resonance]);
    assert!((lanes[0].value - 0.25).abs() < 1e-7 && (lanes[1].value - 0.5).abs() < 1e-7);
    cfg.set(P::Cutoff, 0.75);
    cfg.set(P::Resonance, 1.);
    let fixed = TimeMod::phaser_filter([0.75, 1.]).unwrap().config();
    assert_eq!(cfg.allpass, fixed.allpass);
    assert!(Config::phaser_filter(0.25, 0.5, None).lanes().is_empty());
}

/// Exercise every prepared oscillator through both the per-frame LFO and
/// shifted voice reads, with the same scalar sample history at every partition.
#[test]
fn every_oscillator_preserves_voice_reads_across_partitions() {
    let input: Vec<_> = (0..513).map(|i| [((i * 17) % 31) as f64 / 31., -((i * 7) % 23) as f64 / 23.]).collect();
    let shapes = [
        LfoShape::Sine, LfoShape::Triangle, LfoShape::Saw, LfoShape::Square,
        LfoShape::Uvi { wave: UviWave::Sine, direct: false },
    ];
    for shape in shapes {
        let mut l = line(&[Tap { time: DelayTime::Ms(8.), level: 1., pan: 0., depth_ms: 2. }], 15.);
        l.voices = 3;
        l.voice_phase = 0.75;
        l.interpolation = Interpolation::Linear;
        let cfg = wrap(l, Lfo { shape, rate: Rate::Hz(3.), phase: 0.2 });
        let (expected, fault) = run(cfg, 1000, 120., &input, 64);
        assert!(!fault);
        for partition in [1, 3, 17, 63] {
            let (actual, fault) = run(cfg, 1000, 120., &input, partition);
            assert!(!fault);
            assert_eq!(actual, expected, "{shape:?}, partition {partition}");
        }
    }
}

#[test]
fn every_interpolation_keeps_integer_reads_and_ring_wrap_exact() {
    let slab = std::array::from_fn::<_, 16, _>(|i| [i as f64, -(i as f64)]);
    for kind in [Interpolation::Nearest, Interpolation::Floor, Interpolation::Ceil,
        Interpolation::Linear, Interpolation::Hermite, Interpolation::Lagrange] {
        for position in 0..16 {
            for delay in 2..12 {
                for channel in 0..2 {
                    let expected = slab[(position + 16 - delay) % 16][channel];
                    assert_eq!(read(&slab, 16, position, 16, delay as f64, kind, channel), expected);
                    assert_eq!(read(&slab, 16, position, 0, delay as f64, kind, channel), 0.);
                }
            }
        }
    }
}
