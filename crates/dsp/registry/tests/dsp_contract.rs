//! Unified DSP contract test suite exercising [`kontra_dsp::core::DspCore`]
//! across all DSP families: block-size invariance (`[1, 7, 64, 256]`),
//! finite stability, clean state reset, and moldable `Space` morphing.

use kontra_dsp::core::contract::assert_block_invariant_and_finite;
use kontra_dsp::delay::{DualDelay, DualDelaySettings};
use kontra_dsp::drive::{
    AmpStack, AmpStackSettings, AmpVoicing, Cabinet, CabinetSettings, Circuit, Clipper, ColorSat,
    ColorSatSettings, Curve, Overdrive, Shaper, Tone, Voicing,
};
use kontra_dsp::filter::{
    PassBank, PassBankSettings, SpectralMorph, SpectralMorphSettings, Wah, WahSettings,
};
use kontra_dsp::reverb::{Convolver, Diffuser, Fdn, Plate, Settings, Space};
use kontra_dsp::spatial::{StereoDetune, StereoDetuneSettings};

const RATE: u32 = 48_000;

fn reverb_settings() -> Settings {
    Settings {
        seconds: 1.8,
        size: 1.0,
        damping: 0.4,
        diffusion: 0.85,
        predelay_ms: 12.0,
        high_cut_hz: 8_000.0,
        low_shelf_db: -3.0,
        stereo: 0.9,
        modulation_ms: 0.0,
        modulation_hz: 0.0,
    }
}

fn short_ir() -> Convolver {
    let mut ir_l = [0.0_f32; 256];
    let mut ir_r = [0.0_f32; 256];
    ir_l[0] = 0.7;
    ir_r[0] = 0.7;
    ir_l[64] = 0.25;
    ir_r[64] = -0.25;
    ir_l[180] = -0.1;
    ir_r[180] = 0.12;
    Convolver::new(&ir_l, &ir_r)
}

#[test]
fn dsp_families_satisfy_block_invariance_finite_and_reset_contract() {
    assert_block_invariant_and_finite(
        || {
            PassBank::new(
                PassBankSettings {
                    hp_on: true,
                    hp_hz: 180.0,
                    lp_on: true,
                    lp_hz: 6_500.0,
                },
                RATE,
            )
        },
        512,
        1e-5,
    );

    assert_block_invariant_and_finite(
        || {
            Wah::new(
                WahSettings {
                    peak_hz: 1200.0,
                    peak_q: 5.0,
                    peak_gain: 2.0,
                    low_hz: 300.0,
                    low_gain: 1.0,
                    mid_hz: 2000.0,
                    mid_gain: 0.2,
                    high_hz: 3400.0,
                    high_gain: 6.0,
                    mono: false,
                },
                RATE,
            )
        },
        512,
        1e-5,
    );

    assert_block_invariant_and_finite(
        || {
            SpectralMorph::new(
                SpectralMorphSettings {
                    morph: 0.45,
                    amount: 0.85,
                    bank: 1,
                    tracking: true,
                },
                RATE,
            )
        },
        512,
        1e-5,
    );

    assert_block_invariant_and_finite(
        || {
            Shaper::new(
                Curve::Tanh(4.0),
                Tone {
                    pre_lowpass_hz: Some(8_000.0),
                    lowpass_hz: Some(6_000.0),
                    highpass_hz: Some(80.0),
                    sections: kontra_dsp::core::Sections::NONE,
                },
                RATE,
            )
        },
        512,
        1e-5,
    );

    assert_block_invariant_and_finite(
        || {
            Clipper::new(
                Circuit {
                    drive: 4.0,
                    limit: 0.8,
                    tone_hz: 4_000.0,
                    coupling_hz: 40.0,
                    saturation: 1.0,
                    slope: 1.0,
                    asymmetry: 0.2,
                    dc_hz: 20.0,
                },
                RATE,
            )
        },
        512,
        1e-5,
    );

    assert_block_invariant_and_finite(
        || {
            Overdrive::new(
                Voicing {
                    split_hz: [220.0, 720.0, 2_400.0],
                    smooth_hz: [3_500.0, 5_000.0, 7_500.0],
                    drive: 4.0,
                    mix: 0.8,
                    level: 0.9,
                    curve: std::borrow::Cow::Borrowed(&[0.0, 0.5, 0.85, 1.0]),
                },
                RATE,
            )
        },
        512,
        1e-5,
    );

    assert_block_invariant_and_finite(
        || {
            AmpStack::new(
                AmpStackSettings {
                    voicing: AmpVoicing::BritishHighGain,
                    preamp_gain: 4.0,
                    drive: 2.0,
                    master: 0.9,
                    high_gain: true,
                    bright: true,
                    low_boost: false,
                    high_boost: false,
                    bass_db: 2.0,
                    mid_db: -1.5,
                    mid_hz: 800.0,
                    treble_db: 3.0,
                    presence_db: 1.5,
                    resonance_db: 1.0,
                    tone_cut_hz: 12_000.0,
                    tremolo_hz: 4.0,
                    tremolo_depth: 0.3,
                    geq_enabled: false,
                    geq_db: [0.0; 9],
                    geq_gain_db: 0.0,
                    mono: false,
                },
                RATE,
            )
        },
        512,
        1e-5,
    );

    assert_block_invariant_and_finite(
        || {
            ColorSat::new(
                ColorSatSettings {
                    character: 3,
                    drive: 0.7,
                    boost: true,
                    boost_db: 6.0,
                    bass_saver: 0.4,
                    lpf_hz: 14_000.0,
                    hpf_hz: 40.0,
                    bass_db: 2.0,
                    mid_db: -1.0,
                    mid_hz: 1200.0,
                    mid_q: 1.2,
                    treble_db: 2.5,
                    mix: 0.8,
                },
                RATE,
            )
        },
        512,
        1e-5,
    );

    assert_block_invariant_and_finite(
        || {
            Cabinet::new(
                CabinetSettings {
                    model: 2,
                    size: 0.5,
                    air: 0.6,
                    bass_db: 1.5,
                    treble_db: -1.0,
                },
                RATE,
            )
        },
        512,
        1e-5,
    );

    assert_block_invariant_and_finite(|| Plate::new(reverb_settings(), RATE), 512, 1e-5);
    assert_block_invariant_and_finite(|| Fdn::new(reverb_settings(), RATE), 512, 1e-5);
    assert_block_invariant_and_finite(|| Diffuser::new(reverb_settings(), RATE), 512, 1e-5);
    assert_block_invariant_and_finite(short_ir, 512, 1e-4);
    assert_block_invariant_and_finite(
        || Space::hybrid(short_ir(), reverb_settings(), RATE, 0.5),
        512,
        1e-4,
    );

    assert_block_invariant_and_finite(
        || {
            DualDelay::new(
                DualDelaySettings {
                    predelay_ms: [4.0, 7.0],
                    time_ms: [20.0, 31.0],
                    feedback: [0.35, 0.3],
                    cross_feedback: 0.15,
                    level: [0.8, 0.8],
                    width: 0.9,
                },
                RATE,
                200.0,
            )
        },
        512,
        1e-5,
    );

    assert_block_invariant_and_finite(
        || {
            StereoDetune::new(
                StereoDetuneSettings {
                    split_hz: 300.0,
                    ratio: 1.008,
                    spread: 0.8,
                    mix: 0.6,
                },
                RATE,
            )
        },
        512,
        1e-5,
    );
}

#[test]
fn space_molds_across_diffusion_convolution_and_hybrid_fdn() {
    let s = Settings {
        predelay_ms: 0.0,
        ..reverb_settings()
    };

    // Pure diffusion cloud: smears a single impulse across dozens of ms with finite bounded energy.
    let mut diff = Space::diffuser(s, RATE);
    let mut io = vec![[0.0_f32; 2]; 4096];
    if let Some(first) = io.first_mut() {
        *first = [1.0, 1.0];
    }
    diff.process(&mut io);
    assert!(io.iter().flatten().all(|v| v.is_finite()));
    let active = io.iter().filter(|f| f[0].abs() > 1e-4).count();
    assert!(
        active > 200,
        "Diffuser must smear impulse into dense cloud, got {active} active frames"
    );

    // Hybrid Space: blend=0.0 matches pure Convolver; blend=1.0 matches pure Fdn; 0.5 is exact midpoint.
    let render = |mut space: Space| {
        let mut buf = vec![[0.0_f32; 2]; 2048];
        if let Some(first) = buf.first_mut() {
            *first = [1.0, -0.5];
        }
        space.process(&mut buf);
        buf
    };
    let pure_conv = render(Space::convolver(short_ir()));
    let pure_fdn = render(Space::fdn(s, RATE));
    let hyb_0 = render(Space::hybrid(short_ir(), s, RATE, 0.0));
    let hyb_50 = render(Space::hybrid(short_ir(), s, RATE, 0.5));
    let hyb_100 = render(Space::hybrid(short_ir(), s, RATE, 1.0));

    let max_diff = |a: &[[f32; 2]], b: &[[f32; 2]]| {
        a.iter()
            .zip(b)
            .map(|(x, y)| (x[0] - y[0]).abs().max((x[1] - y[1]).abs()))
            .fold(0.0_f32, f32::max)
    };
    assert!(
        max_diff(&hyb_0, &pure_conv) < 1e-5,
        "blend=0 must match Convolver"
    );
    assert!(
        max_diff(&hyb_100, &pure_fdn) < 1e-5,
        "blend=1 must match Fdn"
    );
    let mid_err = hyb_50
        .iter()
        .zip(pure_conv.iter().zip(&pure_fdn))
        .map(|(h, (c, f))| {
            let expected = [0.5 * (c[0] + f[0]), 0.5 * (c[1] + f[1])];
            (h[0] - expected[0]).abs().max((h[1] - expected[1]).abs())
        })
        .fold(0.0_f32, f32::max);
    assert!(
        mid_err < 1e-5,
        "blend=0.5 must equal 50/50 Convolver+Fdn mix, err={mid_err}"
    );
}
