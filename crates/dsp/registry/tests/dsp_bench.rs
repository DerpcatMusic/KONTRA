//! Reproducible DSP kernel benchmark suite (48 kHz, 256-frame blocks).
//!
//! Run in release mode:
//! `build-slot cargo test --manifest-path plugins/kontra/Cargo.toml -p kontra-dsp --release --test dsp_bench -- --ignored --nocapture`

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::borrow::Cow;
use std::hint::black_box;
use std::time::Instant;

use kontra_dsp::core::{Fader, Levels, Section, Sections};
use kontra_dsp::delay::{
    Choir, ChoirSettings, CombBank, CombSettings, DualDelay, DualDelaySettings, ModDelay, Phaser,
    PhaserSettings, ScanShape, Settings as DelaySettings,
};
use kontra_dsp::drive::{ColorSat, ColorSatSettings, Curve, Shaper, Tone};
use kontra_dsp::dynamics::{
    Compressor, CompressorSettings, Detector, Limiter, LimiterSettings, Transient,
    TransientSettings,
};
use kontra_dsp::eq::{Bell, bell};
use kontra_dsp::filter::{
    Cascade, CascadeSettings, CascadeStage, Clocked, ClockedShape, Formant, LadderKind,
    NotchSettings, Notches, Pass, Shape, Svf, VowelSet, Vowels,
};
use kontra_dsp::reverb::{Fdn, Settings as ReverbSettings};
use kontra_dsp::spatial::{Rotary, RotarySettings};

const RATE: u32 = 48_000;
const BLOCK: usize = 256;

fn test_signal() -> Vec<[f32; 2]> {
    (0..BLOCK)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let l = 0.4 * (std::f32::consts::TAU * 220.0 * t).sin()
                + 0.2 * (std::f32::consts::TAU * 1320.0 * t).sin();
            let r = 0.4 * (std::f32::consts::TAU * 330.0 * t).sin()
                + 0.2 * (std::f32::consts::TAU * 1760.0 * t).sin();
            [l, r]
        })
        .collect()
}

fn bench_kernel(label: &str, blocks: usize, mut f: impl FnMut(&mut [[f32; 2]])) {
    let src = test_signal();
    let mut io = src.clone();
    // Warmup
    for _ in 0..32 {
        io.copy_from_slice(&src);
        f(&mut io);
        black_box(&io);
    }
    let t0 = Instant::now();
    for _ in 0..blocks {
        io.copy_from_slice(&src);
        f(&mut io);
        black_box(&io);
    }
    let elapsed_ns = t0.elapsed().as_nanos() as f64;
    let ns_per_frame = elapsed_ns / (blocks * BLOCK) as f64;
    let us_per_block = ns_per_frame * BLOCK as f64 / 1000.0;
    let cpu_pct = (ns_per_frame / (1e9 / f64::from(RATE))) * 100.0;
    println!(
        "{label:<40} | {ns_per_frame:>7.2} ns/f | {us_per_block:>6.2} us/256 | {cpu_pct:>5.3}% CPU"
    );
}

#[test]
#[ignore = "benchmark"]
fn dsp_kernel_suite() {
    println!("\n=== KONTRA DSP KERNEL BENCHMARK (48 kHz, 256-frame blocks) ===");

    // Core & EQ
    let mut bq = bell(
        Bell {
            hz: 1000.0,
            q: 1.0,
            gain_db: 6.0,
        },
        RATE,
    );
    bench_kernel("core::Biquad (f64 stereo)", 4000, |io| bq.process(io));

    let mut fader = Fader::new(RATE, 3.7, 50.0);
    bench_kernel("core::Fader (settled non-unity)", 4000, |io| {
        fader.process(io, 0.75, [0.8, 0.9]);
    });

    let mut levels = Levels::new(RATE);
    let wet = test_signal();
    bench_kernel("core::Levels::mix (settled)", 4000, |io| {
        levels.mix(io, &wet, 0.5, 0.5);
    });

    // Filters
    let mut svf1 = Svf::new(Shape {
        pass: Pass::Low,
        hz: 1200.0,
        stages: 1,
        ladder: None,
        damping: [0.707; 3],
        gain: 1.0,
    });
    svf1.set_cutoff(1200.0, RATE);
    bench_kernel("filter::Svf (1-stage LP2)", 4000, |io| svf1.process(io));

    let mut svf2 = Svf::new(Shape {
        pass: Pass::Low,
        hz: 1200.0,
        stages: 2,
        ladder: None,
        damping: [0.707; 3],
        gain: 1.0,
    });
    svf2.set_cutoff(1200.0, RATE);
    bench_kernel("filter::Svf (2-stage LP4)", 4000, |io| svf2.process(io));

    let mut svf_onepole = Svf::new(Shape {
        pass: Pass::Low,
        hz: 1200.0,
        stages: 0,
        ladder: Some(LadderKind::OnePole),
        damping: [2.0; 3],
        gain: 1.0,
    });
    svf_onepole.set_cutoff(1200.0, RATE);
    bench_kernel("filter::Svf (OnePole)", 4000, |io| svf_onepole.process(io));

    let mut svf_ladder = Svf::new(Shape {
        pass: Pass::Low,
        hz: 1200.0,
        stages: 1,
        ladder: Some(LadderKind::ZeroDelay(4)),
        damping: [0.8; 3],
        gain: 1.0,
    });
    svf_ladder.set_cutoff(1200.0, RATE);
    bench_kernel("filter::Ladder (ZeroDelay 4-pole)", 2000, |io| {
        svf_ladder.process(io);
    });

    let mut svf_huov = Svf::new(Shape {
        pass: Pass::Low,
        hz: 1200.0,
        stages: 1,
        ladder: Some(LadderKind::Explicit { taps: 4, fat: 1.0 }),
        damping: [0.8; 3],
        gain: 1.0,
    });
    svf_huov.set_cutoff(1200.0, RATE);
    bench_kernel("filter::Huovilainen (2x oversampled)", 1000, |io| {
        svf_huov.process(io);
    });

    let mut svf_sk = Svf::new(Shape {
        pass: Pass::Low,
        hz: 1200.0,
        stages: 1,
        ladder: Some(LadderKind::SallenKey {
            morph: 1.0,
            curve: [2.065_561, 1.749_079],
        }),
        damping: [0.8; 3],
        gain: 1.0,
    });
    svf_sk.set_cutoff(1200.0, RATE);
    bench_kernel("filter::SallenKey (MS20 Newton)", 1000, |io| {
        svf_sk.process(io);
    });

    let mut formant = Formant::new(
        Vowels {
            talk: 0.4,
            sharp: 0.6,
            size: 0.5,
            set: VowelSet::First,
        },
        RATE,
    );
    bench_kernel("filter::Formant (3 parallel biquads)", 2000, |io| {
        formant.process(io);
    });

    let mut clocked = Clocked::new(ClockedShape::Allpass, [0.4, 0.7], RATE);
    bench_kernel("filter::Clocked (Allpass)", 3000, |io| clocked.process(io));

    let mut cascade = Cascade::new(
        CascadeSettings {
            hz: 900.0,
            shifts: [0.5, 0.5],
            stages: [CascadeStage {
                q: 0.6,
                limit: 1.0,
                peak: 2.0,
                morph: 0.25,
                wet: 1.0,
            }; 3],
            gain: 1.0,
        },
        RATE,
    );
    bench_kernel("filter::Cascade (3-stage SVF)", 3000, |io| {
        cascade.process(io);
    });

    let mut notches = Notches::new(
        NotchSettings {
            stages: 6,
            hz: 622.0,
            octaves: 4.6,
            spread: 0.0,
            spread_swing: 1.0,
            spread_limit: 2.7,
            feedback: 0.5,
            loop_width: 8.0,
            loop_top: 0.1,
            lfo_hz: 0.9,
            rounding: 0.0,
            depth: 1.0,
            warp: 0.1,
            phase: 0.25,
            sign: 1.0,
            mix: 1.0,
            low_cut_hz: 5.0,
            internal_hz: 90_000.0,
        },
        RATE,
    );
    bench_kernel("filter::Notches (6-stage 2x phaser)", 1000, |io| {
        notches.process(io);
    });

    // Drive
    let mut shaper_soft = Shaper::new(Curve::Soft(0.7), Tone::NONE, RATE);
    bench_kernel("drive::Shaper (Soft 2x)", 2000, |io| {
        shaper_soft.process(io);
    });

    let mut shaper_tanh = Shaper::new(Curve::Tanh(3.0), Tone::NONE, RATE);
    bench_kernel("drive::Shaper (Tanh 2x)", 2000, |io| {
        shaper_tanh.process(io);
    });

    let mut shaper_tube = Shaper::new(
        Curve::Tube(0.8),
        Tone {
            pre_lowpass_hz: Some(8000.0),
            lowpass_hz: Some(12000.0),
            highpass_hz: Some(80.0),
            sections: Sections::NONE,
        },
        RATE,
    );
    bench_kernel("drive::Shaper (Tube + filters 2x)", 2000, |io| {
        shaper_tube.process(io);
    });

    let mut color_sat = ColorSat::new(
        ColorSatSettings {
            character: 0,
            drive: 0.6,
            boost: false,
            boost_db: 0.0,
            bass_saver: 0.3,
            lpf_hz: 14_000.0,
            hpf_hz: 40.0,
            bass_db: 2.0,
            mid_db: 1.0,
            mid_hz: 1200.0,
            mid_q: 1.0,
            treble_db: -1.0,
            mix: 0.8,
        },
        RATE,
    );
    bench_kernel("drive::ColorSat (char 0)", 1000, |io| {
        color_sat.process(io);
    });

    // Dynamics
    let mut comp = Compressor::new(
        CompressorSettings {
            threshold_db: -18.0,
            ratio: 4.0,
            knee_db: 6.0,
            attack_ms: 5.0,
            release_ms: 60.0,
            makeup_db: 3.0,
            link: true,
            mix: 1.0,
            detector: Detector::Input,
            sidechain_hz: None,
        },
        RATE,
    );
    bench_kernel("dynamics::Compressor (FF linked)", 2000, |io| {
        comp.process(io);
    });

    let mut limiter = Limiter::new(
        LimiterSettings {
            input_db: 6.0,
            ceiling_db: -0.3,
            release_ms: 50.0,
            knee_db: 2.0,
            attack_ms: 0.5,
            smooth_ms: 5.0,
            slow: 0.2,
            lookahead_ms: 1.5,
            clip: true,
        },
        RATE,
    );
    bench_kernel("dynamics::Limiter (lookahead)", 2000, |io| {
        limiter.process(io);
    });

    let mut transient = Transient::new(
        TransientSettings {
            attack: 6.0,
            sustain: -3.0,
            gain_db: 0.0,
            clip: true,
        },
        RATE,
    );
    bench_kernel("dynamics::Transient", 2000, |io| transient.process(io));

    // Delays & ModFX
    let mut mod_delay = ModDelay::new(
        DelaySettings {
            ms: 15.0,
            depth_ms: 4.0,
            second_head: true,
            input: [[1.0, 0.0], [0.0, 1.0]],
            feedback: [[0.35, 0.0], [0.0, 0.35]],
            output: [[1.0, 0.0], [0.0, 1.0]],
            damping_hz: 8000.0,
            low_cut_hz: 120.0,
            steep_low_cut: false,
            body: Cow::Borrowed(&[]),
            triangle_s: None,
            shape: Cow::Borrowed(&[]),
            noise: [0.0; 2],
            high_shelf: None,
            spread_ms: 1.0,
            lfo_hz: 0.9,
            phase: 0.25,
            direct: 0.5,
        },
        RATE,
        50.0,
    );
    bench_kernel("delay::ModDelay (2-head chorus)", 2000, |io| {
        mod_delay.process(io);
    });

    let mut phaser = Phaser::new(
        PhaserSettings {
            stages: 6,
            triangle: false,
            hz: 600.0,
            spread_hz: 1800.0,
            exponent: 1.5,
            feedback: 0.5,
            lfo_hz: 0.8,
            phase: 0.25,
        },
        RATE,
    );
    bench_kernel("delay::Phaser (6-stage)", 2000, |io| phaser.process(io));

    let mut dual = DualDelay::new(
        DualDelaySettings {
            predelay_ms: [10.0, 15.0],
            time_ms: [180.0, 240.0],
            feedback: [0.4, 0.4],
            cross_feedback: 0.15,
            level: [0.8, 0.8],
            width: 0.9,
        },
        RATE,
        500.0,
    );
    bench_kernel("delay::DualDelay", 2000, |io| dual.process(io));

    let mut comb_bank = CombBank::new(
        CombSettings {
            root_hz: 110.0,
            ratio: [1.0, 1.25, 1.5, 2.0],
            detune: [1.01, 0.99, 0.98, 1.02],
            feedback: 0.85,
            voices: 3.0,
            damping: 0.1,
            depth: 0.4,
            lfo_hz: 0.9,
            offset: 0.1,
            polarity: 1.0,
            thru_zero: 0.0,
            scan: 0.0,
            scan_depth: 0.0,
            scan_shape: ScanShape::Triangle,
            width: 0.6,
            mix: 0.7,
        },
        RATE,
        50.0,
    );
    bench_kernel("delay::CombBank (4-voice)", 1000, |io| {
        comb_bank.process(io);
    });

    let low = Section {
        hz: 6000.0,
        num: [1.0, 0.0, 0.0],
        den: [1.0, std::f32::consts::SQRT_2, 1.0],
    };
    let mut choir = Choir::new(
        ChoirSettings {
            delay_s: [0.007, 0.011, 0.013, 0.017, 0.019, 0.023, 0.025],
            swing_s: [0.003; 7],
            lfo_hz: [0.31, 0.47, 0.53, 0.71, 0.83, 0.97, 1.13],
            start: [0.0, 0.14, 0.29, 0.43, 0.57, 0.71, 0.86],
            bend: [0.2; 7],
            wobble_hz: [0.0, 0.0, 1.7, 0.0, 0.0, 2.3, 0.0],
            wobble: [0.0, 0.0, 0.3, 0.0, 0.0, 0.2, 0.0],
            smooth_hz: [20.0; 7],
            filter: [low; 7],
            loop_gain: [0.3; 7],
            scatter: [0.0, 0.4, 0.1, 0.2, 0.1, 0.1, 0.1],
            sign: 1.0,
            input: 0.5,
            level: [0.4; 7],
            angle: [-1.2, -0.8, -0.4, 0.0, 0.4, 0.8, 1.2],
            bus: low,
            gain: 0.9,
        },
        RATE,
    );
    bench_kernel("delay::Choir (7-voice)", 1000, |io| choir.process(io));

    // Reverb & Spatial
    let mut fdn = Fdn::new(
        ReverbSettings {
            seconds: 2.4,
            size: 1.0,
            damping: 0.4,
            diffusion: 0.7,
            predelay_ms: 20.0,
            high_cut_hz: 9000.0,
            low_shelf_db: 2.0,
            stereo: 1.0,
            modulation_ms: 1.5,
            modulation_hz: 0.7,
        },
        RATE,
    );
    bench_kernel("reverb::Fdn (8-line modulated)", 1000, |io| {
        fdn.process(io);
    });

    let mut rotary = Rotary::new(
        RotarySettings {
            fast: true,
            horn_slow_hz: 0.81,
            horn_fast_hz: 6.09,
            drum_slow_hz: 0.68,
            drum_fast_hz: 5.40,
            accel_hi: 0.5,
            accel_lo: 0.5,
            crossover_hz: 800.0,
            balance: 0.5,
            distance: 0.3,
            mix: 1.0,
        },
        RATE,
    );
    bench_kernel("spatial::Rotary", 1000, |io| rotary.process(io));
}
