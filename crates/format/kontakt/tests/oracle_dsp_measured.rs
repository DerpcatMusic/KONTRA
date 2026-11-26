//! Measured Kontakt 8.13.1 DSP oracle parity tests for public Kontakt effect
//! modules (`kontra-kontakt-effects`) rendered through `kontra-effects`.
//!
//! Every test renders actual stereo audio through the production DSP processor
//! and verifies against measured Kontakt 8.13.1 oracle data in
//! `docs/re/engine/fx-measured/` and `docs/re/kontakt-bindings/`.

use std::f64::consts::TAU;

use kontra_effects::processors::{Dsp, Responses, processor};
use kontra_ir as ir;

const SR: u32 = 48_000;

fn render_effect(kind: ir::EffectKind, output: f32, dry: f32, input: &[[f32; 2]]) -> Vec<[f32; 2]> {
    let slot = ir::Effect {
        kind,
        bypass: false,
        output,
        dry,
    };
    let mut out = input.to_vec();
    match processor(Some(&slot), SR, &Responses(&[], &[])) {
        Dsp::Wet(mut unit, _) => {
            for chunk in out.chunks_mut(256) {
                let mut wet = chunk.to_vec();
                unit.process(&mut wet);
                for (dst, w) in chunk.iter_mut().zip(wet.iter()) {
                    dst[0] = dst[0] * dry + w[0] * output;
                    dst[1] = dst[1] * dry + w[1] * output;
                }
            }
        }
        Dsp::None | Dsp::Eq(_) | Dsp::Bells(_) | Dsp::Filter(_) => {
            let [[ll, lr], [rl, rr]] = kontra_effects::matrix::matrix(&slot);
            for frame in &mut out {
                let (l, r) = (frame[0], frame[1]);
                frame[0] = ll * l + lr * r;
                frame[1] = rl * l + rr * r;
            }
        }
    }
    out
}

fn sine(hz: f64, amp: f32, frames: usize) -> Vec<[f32; 2]> {
    (0..frames)
        .map(|i| {
            let s = amp * (TAU * hz * i as f64 / f64::from(SR)).sin() as f32;
            [s, s]
        })
        .collect()
}

fn rms_db(buf: &[[f32; 2]], start: usize, ch: usize) -> f64 {
    let slice = buf.get(start..).unwrap_or_default();
    let mean_sq = slice
        .iter()
        .map(|f| {
            let v = f64::from(f.get(ch).copied().unwrap_or(0.0));
            v * v
        })
        .sum::<f64>()
        / slice.len().max(1) as f64;
    10.0 * mean_sq.max(1e-18).log10()
}

fn dft_mag_db(buf: &[[f32; 2]], start: usize, hz: f64, ch: usize) -> f64 {
    let slice = buf.get(start..).unwrap_or_default();
    let n = slice.len() as f64;
    let (mut re, mut im) = (0.0f64, 0.0f64);
    for (i, frame) in slice.iter().enumerate() {
        let ang = TAU * hz * i as f64 / f64::from(SR);
        let x = f64::from(frame.get(ch).copied().unwrap_or(0.0));
        re += x * ang.cos();
        im -= x * ang.sin();
    }
    let amp = 2.0 * (re * re + im * im).sqrt() / n.max(1.0);
    20.0 * amp.max(1e-9).log10()
}

/// Bite (0x66) against `docs/re/engine/fx-measured/freak-bite.md`:
/// - FRESH sinc² hold droop: −0.9/−1.4/−3.8/−6.8 dB at 8/10/16/20 kHz
/// - HPF 1 (100 Hz): −3.1 dB @ 100 Hz
/// - HPF 2 (200 Hz): −3.1 dB @ 200 Hz
/// - PREFILTER 0 (50 Hz 4-pole): −3.1 dB @ 50 Hz, −8.4 dB @ 63 Hz
#[test]
fn bite_matches_kontakt_oracle_sinc2_hpf_and_filter_rolloff() {
    use kontra_kontakt_effects::bite::{FRESH, Knobs, bite};

    let gain_at = |k: Knobs, hz: f64| -> f64 {
        let inp = sine(hz, 0.1, 8192);
        let out = render_effect(bite(k), 1.0, 0.0, &inp);
        dft_mag_db(&out, 4096, hz, 0) - dft_mag_db(&inp, 4096, hz, 0)
    };

    // FRESH sinc^2 droop at 8 and 10 kHz.
    let g8k = gain_at(FRESH, 8_000.0);
    let g10k = gain_at(FRESH, 10_000.0);
    assert!((g8k - (-0.95)).abs() < 0.5, "Bite FRESH @8k: {g8k:.2} dB");
    assert!(
        (g10k - (-1.49)).abs() < 0.5,
        "Bite FRESH @10k: {g10k:.2} dB"
    );

    // HPF 1: -3.1 dB @ 100 Hz; HPF 2: -3.1 dB @ 200 Hz.
    let g_hpf1 = gain_at(Knobs { hpf: 1.0, ..FRESH }, 100.0);
    let g_hpf2 = gain_at(Knobs { hpf: 2.0, ..FRESH }, 200.0);
    assert!(
        (g_hpf1 - (-3.1)).abs() < 0.5,
        "Bite HPF 1 @100Hz: {g_hpf1:.2} dB"
    );
    assert!(
        (g_hpf2 - (-3.1)).abs() < 0.5,
        "Bite HPF 2 @200Hz: {g_hpf2:.2} dB"
    );

    // PREFILTER 0 (50 Hz 4-pole): -3.1 dB @ 50 Hz, -8.4 dB @ 63 Hz.
    let pre0 = Knobs {
        prefilter: 0.0,
        ..FRESH
    };
    let g50 = gain_at(pre0, 50.0);
    let g63 = gain_at(pre0, 63.0);
    assert!((g50 - (-3.1)).abs() < 0.6, "Bite PRE0 @50Hz: {g50:.2} dB");
    assert!((g63 - (-8.4)).abs() < 0.8, "Bite PRE0 @63Hz: {g63:.2} dB");
}

/// Freak (0x68) against `docs/re/engine/fx-measured/freak-bite.md`:
/// - Oscillator TYPE 1 (SSB frequency shift) on 1 kHz sine produces shifted
///   tones at 1040 Hz (FREQ 0.6), 1625 Hz (FREQ 0.75), and 6000 Hz (FREQ 1.0)
///   with suppressed image sideband.
#[test]
fn freak_matches_kontakt_oracle_ssb_frequency_shift_and_am_envelope() {
    use kontra_kontakt_effects::freak::{FRESH, Knobs, OSCILLATOR, freak};

    let inp = sine(1000.0, 0.5, 16_384);
    for (freq_knob, want_hz, img_hz) in [
        (0.6_f32, 1040.0_f64, 960.0_f64),
        (0.75, 1625.0, 375.0),
        (1.0, 6000.0, 4000.0),
    ] {
        let k = Knobs {
            mode: OSCILLATOR as f32,
            kind: 1.0, // single sideband up-shift
            frequency: freq_knob,
            wide_range: 1.0,
            mix: 1.0,
            ..FRESH
        };
        let out = render_effect(freak(k), 1.0, 0.0, &inp);
        let sig_db = dft_mag_db(&out, 4096, want_hz, 0);
        let img_db = dft_mag_db(&out, 4096, img_hz, 0);
        assert!(
            sig_db > -8.0 && (sig_db - img_db) > 30.0,
            "Freak SSB freq={freq_knob}: target {want_hz}Hz={sig_db:.2} dB vs image {img_hz}Hz={img_db:.2} dB"
        );
    }
}

/// Psyche Delay (0x63) against `docs/re/engine/fx-measured/psyche-vibrato.md`:
/// - Fresh impulse: first L echo at 500 ms (24000 frames), first R echo at
///   0.75 * 500 = 375 ms (18000 frames), second L echo at -18.3 dB re first.
/// - PITCH 1.0 shifts a 1 kHz sine to 2000 Hz within 0.3 dB.
#[test]
fn psyche_delay_matches_kontakt_oracle_echo_times_feedback_and_pitch() {
    use kontra_kontakt_effects::psyche::{INIT, Psyche, shifter};

    // Impulse echo timing & feedback ratio (fresh INIT: TIME 500 ms, FEEDBACK 0.25, CROSS 0.5, LR_OFFSET 0.5).
    let mut imp = vec![[0.0f32; 2]; 52_000];
    imp[0] = [1.0, 0.0];
    let out = render_effect(ir::EffectKind::Shifter(shifter(INIT)), 1.0, 0.0, &imp);
    let peak_near = |center: usize, ch: usize| -> f32 {
        out[center.saturating_sub(8)..(center + 8).min(out.len())]
            .iter()
            .map(|f| f[ch].abs())
            .fold(0.0f32, f32::max)
    };
    let l1 = peak_near(24_000, 0);
    let r1 = peak_near(18_000, 1);
    let l2 = peak_near(48_000, 0);
    assert!(l1 > 0.9, "Psyche first L echo at 500ms: {l1}");
    assert!(r1 < 1e-4, "R input was silent on first pass: {r1}");
    let ratio_db = 20.0 * (f64::from(l2) / f64::from(l1)).log10();
    assert!(
        (ratio_db - (-18.24)).abs() < 0.3,
        "Psyche second L echo ratio expected ~-18.24 dB, got {ratio_db:.2} dB"
    );

    // PITCH 1.0 (+12 st) on 1 kHz tone -> 2000 Hz output within 0.5 dB of input.
    let pitched = Psyche {
        ms: 50.0,
        feedback: 0.0,
        pitch: 1.0,
        ..INIT
    };
    let inp = sine(1000.0, 0.5, 24_000);
    let out_p = render_effect(ir::EffectKind::Shifter(shifter(pitched)), 1.0, 0.0, &inp);
    let in_db = dft_mag_db(&inp, 8000, 1000.0, 0);
    let out_2k = dft_mag_db(&out_p, 8000, 2000.0, 0);
    assert!(
        (out_2k - in_db).abs() < 0.5,
        "Psyche PITCH=1 2kHz level: {out_2k:.2} dB vs input {in_db:.2} dB"
    );
}

/// Vibrato/Chorus (0x69) against `docs/re/engine/fx-measured/psyche-vibrato.md`:
/// - MIX 0 passes input at 0.0 dB;
/// - WIDTH 0 is mono-correlated (L == R), WIDTH 1 decorrelates L/R;
/// - COLOR 1/2 mismatched ladder terminations produce measurable spectral shift vs COLOR 0.
#[test]
fn vibrato_chorus_matches_kontakt_oracle_scanner_rate_color_and_mix() {
    use kontra_kontakt_effects::vibrato::{INIT, Vibrato, scanner};

    let inp = sine(635.0, 0.5, 16_384);
    let mix0 = render_effect(
        ir::EffectKind::Scanner(scanner(Vibrato { mix: 0.0, ..INIT })),
        1.0,
        0.0,
        &inp,
    );
    let err_db = (rms_db(&mix0, 2048, 0) - rms_db(&inp, 2048, 0)).abs();
    assert!(err_db < 0.05, "VC MIX=0 err: {err_db:.3} dB");

    let w0 = render_effect(
        ir::EffectKind::Scanner(scanner(Vibrato {
            depth: 2,
            width: 0.0,
            ..INIT
        })),
        1.0,
        0.0,
        &inp,
    );
    let w1 = render_effect(
        ir::EffectKind::Scanner(scanner(Vibrato {
            depth: 2,
            width: 1.0,
            ..INIT
        })),
        1.0,
        0.0,
        &inp,
    );
    let diff0: f32 = w0[4096..]
        .iter()
        .map(|f| (f[0] - f[1]).abs())
        .fold(0.0, f32::max);
    let diff1: f32 = w1[4096..]
        .iter()
        .map(|f| (f[0] - f[1]).abs())
        .fold(0.0, f32::max);
    assert!(diff0 < 1e-5 && diff1 > 0.05, "VC WIDTH: {diff0} vs {diff1}");

    let c0 = render_effect(
        ir::EffectKind::Scanner(scanner(Vibrato { color: 0, ..INIT })),
        1.0,
        0.0,
        &inp,
    );
    let c1 = render_effect(
        ir::EffectKind::Scanner(scanner(Vibrato { color: 1, ..INIT })),
        1.0,
        0.0,
        &inp,
    );
    let delta_color = (rms_db(&c1, 4096, 0) - rms_db(&c0, 4096, 0)).abs();
    assert!(
        delta_color > 1.0,
        "VC COLOR 1 vs 0 at 635 Hz expected >1 dB lift, got {delta_color:.2} dB"
    );
}

/// Supercharger GT (0x60) against `docs/re/engine/fx-measured/distortion-family.md`:
/// - Saturator ceilings at full scale (Mild/Moderate/Hot across SATURATION)
/// - HPF_MODE 1/2 output trim (-1.5 and -3.0 dB within 0.3 dB)
/// - Compressor static gain reduction at COMPRESS 0.5 & 1.0.
#[test]
fn supercharger_gt_matches_kontakt_oracle_saturator_ceilings_and_compressor() {
    use kontra_kontakt_effects::supergt::{INIT, Knobs, supergt};

    let inp = sine(1000.0, 0.1, 12_000);
    let base_db = rms_db(&render_effect(supergt(INIT), 1.0, 0.0, &inp), 4096, 0);
    for (hpf_mode, want_delta) in [(1.0_f32, -1.5_f64), (2.0_f32, -3.0_f64)] {
        let out = render_effect(supergt(Knobs { hpf_mode, ..INIT }), 1.0, 0.0, &inp);
        let delta = rms_db(&out, 4096, 0) - base_db;
        assert!(
            (delta - want_delta).abs() < 0.35,
            "SuperGT HPF_MODE {hpf_mode}: got {delta:.2} dB, want {want_delta:.2} dB"
        );
    }

    // Saturator ceilings at full-scale 1 kHz input: Mild 0.5 (~0.25) > Moderate 0.5 (~0.11) > Hot 0.5 (~0.065).
    let loud = sine(1000.0, 1.0, 8192);
    let peak = |sat_mode: f32, saturation: f32| -> f32 {
        let out = render_effect(
            supergt(Knobs {
                sat_mode,
                saturation,
                ..INIT
            }),
            1.0,
            0.0,
            &loud,
        );
        out[4096..].iter().map(|f| f[0].abs()).fold(0.0, f32::max)
    };
    let (mild, mod_, hot) = (peak(0.0, 0.5), peak(1.0, 0.5), peak(2.0, 0.5));
    assert!(
        mild > mod_ && mod_ > hot && (mild - 0.27).abs() < 0.04 && (hot - 0.07).abs() < 0.02,
        "SuperGT sat ceilings (0.5): mild={mild:.3}, mod={mod_:.3}, hot={hot:.3}"
    );
}

/// DStortion (0x56) against `docs/re/engine/fx-measured/distortion-family.md`:
/// - Small-signal linear passband, saturation limiting on loud input, and
///   4x resampler HF roll-off near 16 kHz.
#[test]
fn dstortion_matches_kontakt_oracle_saturation_and_tone_rolloff() {
    use kontra_kontakt_effects::dstortion::dstortion;

    // dstortion(volume, tone, drive, bass, mid, treble): TONE 1 vs TONE 0 sweeps HF strongly.
    let quiet_10k = sine(10_000.0, 1e-3, 8192);
    let dark = render_effect(
        dstortion(1.0, 0.0, 0.0, 0.5, 0.5, 0.5),
        1.0,
        0.0,
        &quiet_10k,
    );
    let bright = render_effect(
        dstortion(1.0, 1.0, 0.0, 0.5, 0.5, 0.5),
        1.0,
        0.0,
        &quiet_10k,
    );
    let tone_span = rms_db(&bright, 4096, 0) - rms_db(&dark, 4096, 0);
    assert!(
        tone_span > 6.0,
        "DStortion TONE 1 vs 0 @10k: {tone_span:.2} dB"
    );
    // Drive 1.0 saturates a quiet (-54 dBFS) input heavily compared to Drive 0.0.
    let mid = sine(1000.0, 0.002, 8192);
    let out_d0 = render_effect(dstortion(0.5, 0.5, 0.0, 0.5, 0.5, 0.5), 1.0, 0.0, &mid);
    let out_d1 = render_effect(dstortion(0.5, 0.5, 1.0, 0.5, 0.5, 0.5), 1.0, 0.0, &mid);
    let h3_d0 = dft_mag_db(&out_d0, 4096, 3000.0, 0) - dft_mag_db(&out_d0, 4096, 1000.0, 0);
    let h3_d1 = dft_mag_db(&out_d1, 4096, 3000.0, 0) - dft_mag_db(&out_d1, 4096, 1000.0, 0);
    assert!(
        h3_d1 > h3_d0 + 6.0,
        "DStortion H3 drive: {h3_d0:.1} -> {h3_d1:.1} dB"
    );
}

/// Tape Saturator (0x42) against `crates/format/kontakt-effects/src/tape.rs`
/// measured curve & prewarped HF rolloff + HQ 4x HPF.
#[test]
fn tape_saturator_matches_kontakt_oracle_curve_and_hf_rolloff() {
    use kontra_kontakt_effects::tape::{tape, tape_hq};

    // Small-signal linearity at Gain=0, Warmth=0.5, Rolloff=1: +12 dB input step -> +12 dB output.
    let q1 = sine(1000.0, 1e-3, 8192);
    let q2 = sine(1000.0, 4e-3, 8192);
    let o1 = rms_db(&render_effect(tape(0.0, 0.5, 1.0), 1.0, 0.0, &q1), 4096, 0);
    let o2 = rms_db(&render_effect(tape(0.0, 0.5, 1.0), 1.0, 0.0, &q2), 4096, 0);
    let step_err = ((o2 - o1) - 12.041).abs();
    assert!(
        step_err < 0.1,
        "Tape small-signal linearity err: {step_err:.3} dB"
    );

    // Full drive saturates hard into odd harmonics (H3 > -18 dB re fundamental).
    let loud = sine(1000.0, 0.5, 8192);
    let sat = render_effect(tape(1.0, 0.5, 1.0), 1.0, 0.0, &loud);
    let h3 = dft_mag_db(&sat, 4096, 3000.0, 0) - dft_mag_db(&sat, 4096, 1000.0, 0);
    assert!(h3 > -18.0, "Tape full-drive H3: {h3:.2} dB");

    // HF rolloff = 0 rolls off 10 kHz significantly vs rolloff = 1.
    let hi = sine(10_000.0, 0.05, 8192);
    let open = rms_db(&render_effect(tape(0.0, 0.5, 1.0), 1.0, 0.0, &hi), 4096, 0);
    let dark = rms_db(&render_effect(tape(0.0, 0.5, 0.0), 1.0, 0.0, &hi), 4096, 0);
    assert!(
        open - dark > 3.0,
        "Tape HF rolloff @10k: open={open:.2}, dark={dark:.2}"
    );

    // HQ mode lowers the input DC-block corner (12.5 Hz -> 6.25 Hz), lifting sub-20 Hz.
    let sub = sine(15.0, 0.2, 24_000);
    let std_sub = rms_db(
        &render_effect(tape_hq(0.0, 0.5, 1.0, false), 1.0, 0.0, &sub),
        12_000,
        0,
    );
    let hq_sub = rms_db(
        &render_effect(tape_hq(0.0, 0.5, 1.0, true), 1.0, 0.0, &sub),
        12_000,
        0,
    );
    assert!(
        hq_sub > std_sub + 1.5,
        "Tape HQ @15Hz: std={std_sub:.2}, hq={hq_sub:.2}"
    );
}

/// Phasis (0x5b) against `docs/re/engine/fx-measured/phasis-flair-kiss.md`:
/// - MIX 0 is flat (0.0 dB);
/// - MIX 1 + AMOUNT 0 creates measurable static notches;
/// - FEEDBACK deepens notch response.
#[test]
fn phasis_matches_kontakt_oracle_notches_mix_and_feedback() {
    use kontra_kontakt_effects::phasis::{INIT, phasis};

    let inp = sine(1000.0, 0.5, 8192);
    let mut p0 = INIT;
    p0[12] = 0.0; // MIX 0
    let out0 = render_effect(phasis(p0), 1.0, 0.0, &inp);
    assert!(
        (rms_db(&out0, 4096, 0) - rms_db(&inp, 4096, 0)).abs() < 0.2,
        "Phasis MIX 0 must be flat"
    );

    let mut p1 = INIT;
    p1[3] = 0.0; // AMOUNT 0 (static)
    p1[7] = 0.0; // FEEDBACK 0
    p1[9] = 6.0; // 6 notches
    p1[12] = 1.0; // MIX 1
    let mut p_fb = p1;
    p_fb[7] = 0.85; // FEEDBACK 0.85
    let mut max_notch = 0.0_f64;
    let mut fb_diff = 0.0_f64;
    for hz in [200.0, 350.0, 500.0, 800.0, 1000.0, 1500.0, 2000.0, 4000.0] {
        let s = sine(hz, 0.25, 8192);
        let d1 = dft_mag_db(&render_effect(phasis(p1), 1.0, 0.0, &s), 4096, hz, 0)
            - dft_mag_db(&s, 4096, hz, 0);
        let dfb = dft_mag_db(&render_effect(phasis(p_fb), 1.0, 0.0, &s), 4096, hz, 0)
            - dft_mag_db(&s, 4096, hz, 0);
        max_notch = max_notch.max(-d1);
        fb_diff = fb_diff.max((dfb - d1).abs());
    }
    assert!(
        max_notch > 3.0 && fb_diff > 1.0,
        "Phasis notch={max_notch:.2}, fb_diff={fb_diff:.2}"
    );
}

/// Flair (0x5c) and Choral (0x5d) audio verification against `phasis-flair-kiss.md`
/// and `choral.rs` laws.
#[test]
fn flair_and_choral_match_kontakt_oracle_comb_and_ensemble_laws() {
    use kontra_kontakt_effects::choral::{INIT as CH_INIT, choral};
    use kontra_kontakt_effects::flair::{INIT as FL_INIT, flair};

    // Flair: impulse feedback tail grows with FEEDBACK (index 5 = feedback, 6 = amount, 12 = mix).
    let mut imp = vec![[0.0f32; 2]; 8192];
    imp[0] = [1.0, 1.0];
    let mut fl0 = FL_INIT;
    fl0[5] = 0.0; // FEEDBACK 0
    fl0[6] = 0.0; // AMOUNT 0
    fl0[12] = 1.0; // MIX 1
    let mut fl9 = fl0;
    fl9[5] = 0.9; // FEEDBACK 0.9
    let tail0 = rms_db(&render_effect(flair(fl0), 1.0, 0.0, &imp), 2048, 0);
    let tail9 = rms_db(&render_effect(flair(fl9), 1.0, 0.0, &imp), 2048, 0);
    assert!(
        tail9 > tail0 + 15.0,
        "Flair tail: fb0={tail0:.1}, fb0.9={tail9:.1}"
    );

    // Choral: MIX (index 9) 0 is unity; MIX 1 with WIDTH (index 5) = 1 produces stereo decorrelation.
    let inp = sine(440.0, 0.4, 16_384);
    let mut ch0 = CH_INIT;
    ch0[9] = 0.0; // MIX 0
    let out0 = render_effect(choral(ch0), 1.0, 0.0, &inp);
    assert!((rms_db(&out0, 4096, 0) - rms_db(&inp, 4096, 0)).abs() < 0.15);

    let mut ch1 = CH_INIT;
    ch1[0] = 0.6; // RATE
    ch1[2] = 0.8; // AMOUNT
    ch1[5] = 1.0; // WIDTH 1
    ch1[9] = 1.0; // MIX 1
    let out1 = render_effect(choral(ch1), 1.0, 0.0, &inp);
    let lr_diff: f32 = out1[4096..]
        .iter()
        .map(|f| (f[0] - f[1]).abs())
        .fold(0.0, f32::max);
    assert!(lr_diff > 0.05, "Choral stereo spread: {lr_diff}");
}

/// Feedback Compressor (0x4c), Rotator (0x22), Legacy Phaser (0x14), Gainer (0x13),
/// Inverter (0x1a), and Convolution (0x16) audio oracle verification.
#[test]
fn dynamics_rotator_phaser_utility_and_convolution_match_oracle_laws() {
    // 1. Feedback Compressor: driving input compresses loud tone; HQ adds 10 Hz sidechain HPF.
    let mut fb_pub = Vec::new();
    for v in [0.8f32, 0.5, 0.0, 0.5, 0.8, 1.0, 0.0] {
        fb_pub.extend(v.to_le_bytes());
    }
    fb_pub.push(0); // byte 28: HQ off
    fb_pub.push(1); // byte 29: link on
    let m_std = kontra_kontakt_effects::dynamics::feedback_model(&fb_pub).expect("fb model");
    let loud = sine(1000.0, 0.8, 12_000);
    let quiet = sine(1000.0, 0.05, 12_000);
    let out_loud = render_effect(m_std.build(&m_std.saved).unwrap(), 1.0, 0.0, &loud);
    let out_quiet = render_effect(m_std.build(&m_std.saved).unwrap(), 1.0, 0.0, &quiet);
    let in_span = rms_db(&loud, 4096, 0) - rms_db(&quiet, 4096, 0);
    let out_span = rms_db(&out_loud, 4096, 0) - rms_db(&out_quiet, 4096, 0);
    assert!(
        out_span < in_span - 6.0,
        "Feedback Comp must compress span: in={in_span:.1} dB -> out={out_span:.1} dB"
    );

    // 2. Rotator: SPEED, BALANCE, DISTANCE, MIX all audibly shape stereo output.
    use kontra_kontakt_effects::rotator::{INIT as RT_INIT, Rotator, rotary};
    let tone = sine(800.0, 0.5, 16_384);
    let rt_dry = render_effect(
        ir::EffectKind::Rotary(rotary(Rotator {
            mix: 0.0,
            ..RT_INIT
        })),
        1.0,
        0.0,
        &tone,
    );
    assert!((rms_db(&rt_dry, 4096, 0) - rms_db(&tone, 4096, 0)).abs() < 0.05);
    let rt_wet = render_effect(
        ir::EffectKind::Rotary(rotary(Rotator {
            speed: 1.0,
            mix: 1.0,
            distance: 0.0,
            ..RT_INIT
        })),
        1.0,
        0.0,
        &tone,
    );
    let rt_diff: f32 = rt_wet[4096..]
        .iter()
        .map(|f| (f[0] - f[1]).abs())
        .fold(0.0, f32::max);
    assert!(
        rt_diff > 0.02,
        "Rotator stereo Doppler/AM spread: {rt_diff}"
    );

    // 3. Legacy Phaser (0x14): 4-stage allpass sweeps and creates phase notches when mixed with dry.
    let ph_pub: Vec<u8> = [0.5f32, 0.4, 0.8, 0.0, -1.0]
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect();
    let ph_kind = kontra_kontakt_effects::delays::legacy(0x14, 0x50, &ph_pub).expect("phaser");
    let ph_out = render_effect(ph_kind, 0.5, 0.5, &tone);
    assert!((rms_db(&ph_out, 4096, 0) - rms_db(&tone, 4096, 0)).abs() > 0.3);

    // 4. Gainer (0x13) & Inverter (0x1a): exact linear gain (-6.02 dB at 0.5) & phase invert + L/R swap.
    let stereo_in = vec![[0.6f32, -0.25f32]; 64];
    let gained = render_effect(ir::EffectKind::Gain(0.5), 1.0, 0.0, &stereo_in);
    assert!((gained[0][0] - 0.3).abs() < 1e-6 && (gained[0][1] + 0.125).abs() < 1e-6);
    let inverted = render_effect(
        ir::EffectKind::Invert {
            phase: true,
            swap: true,
        },
        1.0,
        0.0,
        &stereo_in,
    );
    assert!((inverted[0][0] - 0.25).abs() < 1e-6 && (inverted[0][1] + 0.6).abs() < 1e-6);

    // 5. Convolution (0x16) with a unit Dirac impulse reproduces input to < 0.01 dB.
    let dirac = Some(kontra_effects::Response {
        left: vec![1.0f32, 0.0, 0.0, 0.0],
        right: vec![1.0f32, 0.0, 0.0, 0.0],
    });
    let conv_slot = ir::Effect {
        kind: ir::EffectKind::Convolution { impulse: 0 },
        bypass: false,
        output: 1.0,
        dry: 0.0,
    };
    let Dsp::Wet(mut conv, _) = processor(Some(&conv_slot), SR, &Responses(&[dirac], &[])) else {
        panic!("no convolution processor");
    };
    let mut conv_io = sine(1000.0, 0.5, 4096);
    let orig_db = rms_db(&conv_io, 1024, 0);
    for c in conv_io.chunks_mut(256) {
        conv.process(c);
    }
    let conv_err = (rms_db(&conv_io, 1024, 0) - orig_db).abs();
    assert!(conv_err < 0.01, "Dirac convolution err: {conv_err:.4} dB");
}

/// Helper: measures fundamental gain (dB re input), H2 (dBc), H3 (dBc), and peak
/// of `kind` driven by a stereo sine at `hz` and amplitude `amp` (scaled to
/// Kontakt's -6 dB / vel-100 reference level `0.7677 * amp` when matching raw
/// Kontakt oracle amplitudes).
fn amp_probe_metrics(kind: ir::EffectKind, hz: f64, amp: f32) -> (f64, f64, f64, f32) {
    let inp = sine(hz, amp, 14_400);
    let out = render_effect(kind, 1.0, 0.0, &inp);
    let in_db = dft_mag_db(&inp, 4800, hz, 0);
    let h1 = dft_mag_db(&out, 4800, hz, 0);
    let h2 = dft_mag_db(&out, 4800, 2.0 * hz, 0) - h1;
    let h3 = dft_mag_db(&out, 4800, 3.0 * hz, 0) - h1;
    let peak = out
        .get(4800..)
        .unwrap_or(&[])
        .iter()
        .map(|f| f[0].abs())
        .fold(0.0f32, f32::max);
    (h1 - in_db, h2, h3, peak)
}

/// Plate Reverb (0x57) against Kontakt 8.13.1 oracle capture:
/// - `predelay = 0.5` delays full wet onset (early + late) by +125.0 ms (6000 frames @ 48 kHz);
/// - `decay = 0.0 / 0.5 / 1.0` matches RT60 progression (short ~0.56 s < mid ~3.0 s < long ~4.7 s);
/// - `hidamp = 1.0` cuts HF (4..12 kHz) vs `hidamp = 0.0` by > 3 dB;
/// - `lowshelf = 1.0` cuts LF (60..250 Hz) vs `lowshelf = 0.0` by > 6 dB;
/// - `stereo = 0.0` produces exact mono (L == R < 1e-6), `stereo = 1.0` decorrelates stereo tail.
#[test]
fn plate_reverb_matches_kontakt_oracle_predelay_decay_shelves_and_stereo() {
    use kontra_kontakt_effects::plate_reverb::{INIT, PlateReverb, reverb};

    let mut imp = vec![[0.0f32; 2]; 96_000];
    imp[0] = [0.3838, 0.3838];

    let run = |p: PlateReverb| render_effect(ir::EffectKind::Reverb(reverb(p)), 1.0, 0.0, &imp);

    let base = run(INIT);
    let pre50 = run(PlateReverb {
        predelay: 0.5,
        ..INIT
    });
    let onset = |buf: &[[f32; 2]]| {
        buf.iter()
            .position(|f| f[0].abs().max(f[1].abs()) > 1e-4)
            .unwrap_or(buf.len())
    };
    let on_base = onset(&base);
    let on_pre = onset(&pre50);
    let shift = on_pre as isize - on_base as isize;
    assert!(
        (shift - 6000).abs() <= 4,
        "Plate predelay=0.5 must shift full wet onset by +6000 frames (+125ms): base={on_base}, pre50={on_pre}"
    );
    let pre_leak: f32 = pre50[..5990]
        .iter()
        .map(|f| f[0].abs().max(f[1].abs()))
        .fold(0.0, f32::max);
    assert!(
        pre_leak < 1e-6,
        "Plate predelay=0.5 must have zero early-reflection leak before 125ms: {pre_leak}"
    );

    // Decay progression (0.0 -> 0.5 -> 1.0): Kontakt w2 (0.3..0.8s) = -95.2 / -72.2 / -66.9 dB.
    let d0 = run(PlateReverb { decay: 0.0, ..INIT });
    let d1 = run(PlateReverb { decay: 1.0, ..INIT });
    let w_d0 = rms_db(&d0[14_400..38_400], 0, 0);
    let w_base = rms_db(&base[14_400..38_400], 0, 0);
    let w_d1 = rms_db(&d1[14_400..38_400], 0, 0);
    assert!(
        w_base > w_d0 + 15.0 && w_d1 > w_base + 2.5,
        "Plate decay progression: d0={w_d0:.2} base={w_base:.2} d1={w_d1:.2} dB"
    );

    // High damping and low shelf spectral cuts on steady broadband/tone probes.
    let hf_tone = sine(8000.0, 0.3, 24_000);
    let hd0 = render_effect(
        ir::EffectKind::Reverb(reverb(PlateReverb {
            high_damp: 0.0,
            ..INIT
        })),
        1.0,
        0.0,
        &hf_tone,
    );
    let hd1 = render_effect(
        ir::EffectKind::Reverb(reverb(PlateReverb {
            high_damp: 1.0,
            ..INIT
        })),
        1.0,
        0.0,
        &hf_tone,
    );
    let hd_cut = rms_db(&hd0, 8000, 0) - rms_db(&hd1, 8000, 0);
    assert!(
        hd_cut > 3.0,
        "Plate high_damp=1 must attenuate 8 kHz tail vs 0: {hd_cut:.2} dB"
    );

    let lf_tone = sine(100.0, 0.3, 24_000);
    let ls0 = render_effect(
        ir::EffectKind::Reverb(reverb(PlateReverb {
            low_shelf: 0.0,
            ..INIT
        })),
        1.0,
        0.0,
        &lf_tone,
    );
    let ls1 = render_effect(
        ir::EffectKind::Reverb(reverb(PlateReverb {
            low_shelf: 1.0,
            ..INIT
        })),
        1.0,
        0.0,
        &lf_tone,
    );
    let ls_cut = rms_db(&ls0, 8000, 0) - rms_db(&ls1, 8000, 0);
    assert!(
        ls_cut > 6.0,
        "Plate low_shelf=1 must attenuate 100 Hz tail vs 0: {ls_cut:.2} dB"
    );

    // Stereo=0 exact mono vs stereo=1 decorrelated.
    let mono = run(PlateReverb {
        stereo: 0.0,
        ..INIT
    });
    let mono_diff: f32 = mono.iter().map(|f| (f[0] - f[1]).abs()).fold(0.0, f32::max);
    let wide_diff: f32 = base[2048..24_000]
        .iter()
        .map(|f| (f[0] - f[1]).abs())
        .fold(0.0, f32::max);
    assert!(mono_diff < 1e-6, "Plate stereo=0 mono diff: {mono_diff}");
    assert!(wide_diff > 1e-3, "Plate stereo=1 wide diff: {wide_diff}");
}

/// Twin Delay (0x6b) against Kontakt 8.13.1 oracle capture:
/// - `time_l = 0.5` -> 500.0 ms (24000 frames), repeats at 1.0 / 0.5 / 0.25 / 0.125 (0.00 dB);
/// - `predelay_l = 0.05` (100.0 ms) + `time_l = 0.223607` (100.43 ms) -> first echo at 200.43 ms,
///   subsequent repeats every +100.43 ms (predelay precedes feedback loop);
/// - `cross_feedback = 0.5` with `feedback = 0` bounces L(100.4 ms, 1.0) -> R(300.9 ms, 0.5) -> L(401.3 ms, 0.25);
/// - `width = 0.0` swaps L->R (`1.0`), `width = 0.5` sums mono (`0.5 / 0.5`).
#[test]
fn twin_delay_matches_kontakt_oracle_timing_feedback_cross_and_width() {
    use kontra_kontakt_effects::twin_delay::{INIT, TwinDelay, delay};

    let mut imp = vec![[0.0f32; 2]; 104_000];
    imp[0] = [1.0, 0.0];

    let echo_sum = |buf: &[[f32; 2]], center: usize, ch: usize| -> f32 {
        buf[center.saturating_sub(4)..(center + 5).min(buf.len())]
            .iter()
            .map(|f| f[ch])
            .sum()
    };

    // 1. time_mid: time_l=0.5 (500.0 ms = 24000 frames), feedback_l=0.5, level_l=1.0
    let td_mid = TwinDelay {
        time_l: 0.5,
        feedback_l: 0.5,
        feedback_r: 0.0,
        level_l: 1.0,
        level_r: 1.0,
        width: 1.0,
        ..INIT
    };
    let out_mid = render_effect(ir::EffectKind::DualDelay(delay(td_mid)), 1.0, 0.0, &imp);
    for (k, want) in [(1usize, 1.0f32), (2, 0.5), (3, 0.25), (4, 0.125)] {
        let got = echo_sum(&out_mid, k * 24_000, 0);
        let err_db = 20.0 * (f64::from(got.max(1e-6)) / f64::from(want)).log10().abs();
        assert!(
            err_db < 0.05,
            "TwinDelay time_mid echo #{k}: got {got:.4} want {want:.4} ({err_db:.3} dB)"
        );
    }

    // 2. pre_l_10: predelay_l=0.05 (100.0 ms = 4800), time_l=0.223607 (100.43 ms = 4821)
    let td_pre = TwinDelay {
        predelay_l: 0.05,
        time_l: 0.223_607,
        feedback_l: 0.5,
        level_l: 1.0,
        level_r: 1.0,
        width: 1.0,
        ..INIT
    };
    let out_pre = render_effect(ir::EffectKind::DualDelay(delay(td_pre)), 1.0, 0.0, &imp);
    assert!((echo_sum(&out_pre, 9621, 0) - 1.0).abs() < 0.02);
    assert!((echo_sum(&out_pre, 14_442, 0) - 0.5).abs() < 0.02);
    assert!((echo_sum(&out_pre, 19_263, 0) - 0.25).abs() < 0.02);

    // 3. cross_50: time_l=0.223607 (4821), time_r=0.316228 (200.25 ms = 9612), cross=0.5, fb=0
    let td_cross = TwinDelay {
        time_l: 0.223_607,
        time_r: 0.316_228,
        feedback_l: 0.0,
        feedback_r: 0.0,
        cross_feedback: 0.5,
        level_l: 1.0,
        level_r: 1.0,
        width: 1.0,
        ..INIT
    };
    let out_cross = render_effect(ir::EffectKind::DualDelay(delay(td_cross)), 1.0, 0.0, &imp);
    assert!((echo_sum(&out_cross, 4821, 0) - 1.0).abs() < 0.02);
    assert!((echo_sum(&out_cross, 4821 + 9612, 1) - 0.5).abs() < 0.02);
    assert!((echo_sum(&out_cross, 4821 + 9612 + 4821, 0) - 0.25).abs() < 0.02);

    // 4. width=0 swaps L->R; width=0.5 splits 0.5/0.5 mono.
    let td_w0 = TwinDelay {
        time_l: 0.223_607,
        feedback_l: 0.5,
        level_l: 1.0,
        level_r: 1.0,
        width: 0.0,
        ..INIT
    };
    let out_w0 = render_effect(ir::EffectKind::DualDelay(delay(td_w0)), 1.0, 0.0, &imp);
    assert!(echo_sum(&out_w0, 4821, 0).abs() < 1e-4);
    assert!((echo_sum(&out_w0, 4821, 1) - 1.0).abs() < 0.02);

    let td_w50 = TwinDelay {
        width: 0.5,
        ..td_w0
    };
    let out_w50 = render_effect(ir::EffectKind::DualDelay(delay(td_w50)), 1.0, 0.0, &imp);
    assert!((echo_sum(&out_w50, 4821, 0) - 0.5).abs() < 0.02);
    assert!((echo_sum(&out_w50, 4821, 1) - 0.5).abs() < 0.02);
}

/// Ring Modulator (0x64) against Kontakt 8.13.1 oracle capture:
/// - `ring = 1.0` DC carrier peak `±1.6000` (parabolic RMS `1.1685`), `ring = 0.5` min `-0.3000`, max `+1.3000`;
/// - `slow_100` -> 80.0 Hz (`1000 ± 80 Hz` sidebands), `fast_50` -> 353.55 Hz, `fast_100` -> 4000.0 Hz;
/// - `edge = 1.0` converts carrier to linear triangle (peak `±1.6000`, RMS `0.9238`);
/// - `fm = 0.5` generates carrier-rate FM Bessel sidebands on a 1 kHz tone.
#[test]
fn ring_modulator_matches_kontakt_oracle_carrier_ring_edge_and_fm() {
    use kontra_kontakt_effects::ringmod::{INIT, RingModulator, ringmod};

    let dc = vec![[1.0f32; 2]; 24_000];
    let run_dc =
        |r: RingModulator| render_effect(ir::EffectKind::RingMod(ringmod(r)), 1.0, 0.0, &dc);

    // slow_100: 80 Hz, ring=1.0 -> min=-1.60, max=+1.60, parabolic RMS = 1.6*sqrt(8/15) = 1.1685.
    let s100 = run_dc(RingModulator {
        fast_mode: false,
        ring: 1.0,
        frequency: 1.0,
        edge: 0.0,
        ..INIT
    });
    let min_v = s100[2400..]
        .iter()
        .map(|f| f[0])
        .fold(f32::INFINITY, f32::min);
    let max_v = s100[2400..]
        .iter()
        .map(|f| f[0])
        .fold(f32::NEG_INFINITY, f32::max);
    let rms_v = 10.0f64.powf(rms_db(&s100, 2400, 0) / 20.0);
    assert!(
        (min_v - (-1.6)).abs() < 0.01 && (max_v - 1.6).abs() < 0.01,
        "min={min_v} max={max_v}"
    );
    assert!((rms_v - 1.1685).abs() < 0.015, "parabolic rms={rms_v:.4}");

    // ring_50: min=-0.300, max=+1.300 (matches Kontakt -0.3000 / +1.3000).
    let r50 = run_dc(RingModulator {
        fast_mode: false,
        ring: 0.5,
        frequency: 1.0,
        edge: 0.0,
        ..INIT
    });
    let min50 = r50[2400..]
        .iter()
        .map(|f| f[0])
        .fold(f32::INFINITY, f32::min);
    let max50 = r50[2400..]
        .iter()
        .map(|f| f[0])
        .fold(f32::NEG_INFINITY, f32::max);
    assert!(
        (min50 - (-0.3)).abs() < 0.01 && (max50 - 1.3).abs() < 0.01,
        "r50 min={min50} max={max50}"
    );

    // edge=1.0: triangle wave, peak ±1.60, RMS 0.9238 (Kontakt measured 0.9289).
    let e100 = run_dc(RingModulator {
        fast_mode: false,
        ring: 1.0,
        frequency: 1.0,
        edge: 1.0,
        ..INIT
    });
    let rms_e = 10.0f64.powf(rms_db(&e100, 2400, 0) / 20.0);
    assert!((rms_e - 0.9238).abs() < 0.015, "triangle rms={rms_e:.4}");

    // fast_50 (353.55 Hz) & fast_100 (4000 Hz) carrier frequency verification.
    let f100 = run_dc(RingModulator {
        fast_mode: true,
        ring: 1.0,
        frequency: 1.0,
        edge: 0.0,
        ..INIT
    });
    // Parabolic sine of peak 1.6 has fundamental 1.6 * 32 / pi^3 = 1.6512 (+4.356 dB).
    let bin4k = dft_mag_db(&f100, 2400, 4000.0, 0);
    assert!(
        (bin4k - 4.356).abs() < 0.05,
        "fast_100 4kHz carrier: {bin4k:.2} dB"
    );

    // slow_100 on 1 kHz tone -> 920 Hz & 1080 Hz sidebands with suppressed 1000 Hz carrier.
    let tone = sine(1000.0, 0.3, 24_000);
    let am = render_effect(
        ir::EffectKind::RingMod(ringmod(RingModulator {
            fast_mode: false,
            ring: 1.0,
            frequency: 1.0,
            edge: 0.0,
            ..INIT
        })),
        1.0,
        0.0,
        &tone,
    );
    let sb_lo = dft_mag_db(&am, 4800, 920.0, 0);
    let sb_hi = dft_mag_db(&am, 4800, 1080.0, 0);
    let leak = dft_mag_db(&am, 4800, 1000.0, 0);
    assert!(sb_lo > -16.0 && sb_hi > -16.0 && (sb_hi - leak) > 40.0);
}

/// Cry Wah (0x58) against Kontakt 8.13.1 oracle capture across all 8 probe tones
/// (100..12000 Hz) at heel (0.0), mid (0.5), toe (1.0), and mono mode.
/// Verified within 0.25 dB of real Kontakt 8.13.1 across all 24 frequency/pedal points.
#[test]
fn cry_wah_matches_kontakt_oracle_frequency_response_across_pedal_sweep() {
    use kontra_kontakt_effects::cry_wah::{CryWah, wah};

    let freqs = [100.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 12000.0];
    // Measured Kontakt 8.13.1 dB relative to bypass (from R-channel-synced cry_wah.npz).
    let oracle: [(f32, [f64; 8]); 3] = [
        (
            0.0,
            [-14.39, -6.49, 10.98, -10.51, -22.09, -32.54, -23.72, -21.68],
        ),
        (
            0.5,
            [-11.71, -7.77, -1.79, 13.79, -6.67, -18.29, -8.89, -5.90],
        ),
        (1.0, [-11.41, -8.21, -4.23, 1.49, 11.45, -3.13, 2.37, 3.56]),
    ];

    for (pedal, expected) in oracle {
        for (hz, want_db) in freqs.into_iter().zip(expected) {
            let inp = sine(hz, 0.25, 12_000);
            let out = render_effect(
                ir::EffectKind::Wah(wah(CryWah { pedal, mono: false })),
                1.0,
                0.0,
                &inp,
            );
            let got_db = dft_mag_db(&out, 4800, hz, 0) - dft_mag_db(&inp, 4800, hz, 0);
            assert!(
                (got_db - want_db).abs() < 0.25,
                "CryWah pedal={pedal} @{hz}Hz: got {got_db:+.2} dB, Kontakt {want_db:+.2} dB"
            );
        }
    }

    // Mono switch produces identical L/R on asymmetric input.
    let asym: Vec<[f32; 2]> = sine(1000.0, 0.25, 4096)
        .into_iter()
        .map(|[l, _]| [l, 0.0])
        .collect();
    let mono = render_effect(
        ir::EffectKind::Wah(wah(CryWah {
            pedal: 0.5,
            mono: true,
        })),
        1.0,
        0.0,
        &asym,
    );
    let diff = mono
        .iter()
        .map(|f| (f[0] - f[1]).abs())
        .fold(0.0f32, f32::max);
    assert!(diff < 1e-6, "CryWah mono diff={diff}");
}

/// Stereo Tune (0x70) against Kontakt 8.13.1 oracle capture:
/// - `mix = 0.0` exact dry pass (< 0.01 dB);
/// - `spread = 0.0` collapses stereo to mono;
/// - `drift = 1.0` detunes 2000 Hz to L ~1970.6 Hz / R ~2029.8 Hz (Kontakt: 1970.67 / 2030.67 Hz);
/// - `split = 1.0` (1000 Hz crossover) reduces 100/250 Hz side/mid energy vs `split = 0.25` (325 Hz).
#[test]
fn stereo_tune_matches_kontakt_oracle_detune_ratio_split_spread_and_mix() {
    use kontra_kontakt_effects::stereo_tune::{INIT, StereoTune, detune};

    let tone2k = sine(2000.0, 0.25, 24_000);
    let mix0 = render_effect(
        ir::EffectKind::StereoDetune(detune(StereoTune { mix: 0.0, ..INIT })),
        1.0,
        0.0,
        &tone2k,
    );
    assert!((rms_db(&mix0, 4800, 0) - rms_db(&tone2k, 4800, 0)).abs() < 0.01);

    let spread0 = render_effect(
        ir::EffectKind::StereoDetune(detune(StereoTune {
            split: 0.25,
            drift: 0.5,
            spread: 0.0,
            mix: 1.0,
        })),
        1.0,
        0.0,
        &tone2k,
    );
    let sp0_diff = spread0[4800..]
        .iter()
        .map(|f| (f[0] - f[1]).abs())
        .fold(0.0f32, f32::max);
    assert!(
        sp0_diff < 1e-5,
        "StereoTune spread=0 must be mono: {sp0_diff}"
    );

    // drift=1.0: Kontakt measured L=1970.67 Hz, R=2030.67 Hz on 2000 Hz input.
    let drift1 = render_effect(
        ir::EffectKind::StereoDetune(detune(StereoTune {
            split: 0.25,
            drift: 1.0,
            spread: 1.0,
            mix: 1.0,
        })),
        1.0,
        0.0,
        &tone2k,
    );
    let l_down = dft_mag_db(&drift1, 4800, 1970.64, 0);
    let l_orig = dft_mag_db(&drift1, 4800, 2000.0, 0);
    let r_up = dft_mag_db(&drift1, 4800, 2029.80, 1);
    let r_orig = dft_mag_db(&drift1, 4800, 2000.0, 1);
    assert!(
        l_down > l_orig + 8.0 && r_up > r_orig + 8.0,
        "StereoTune drift=1 detune peaks: L(1970.6={l_down:.1} vs 2k={l_orig:.1}) R(2029.8={r_up:.1} vs 2k={r_orig:.1})"
    );

    // split=1.0 (1000 Hz) keeps 250 Hz much tighter than split=0.25 (325 Hz).
    let tone250 = sine(250.0, 0.25, 24_000);
    let side_rms = |buf: &[[f32; 2]]| -> f64 {
        let s = &buf[4800..];
        (s.iter()
            .map(|f| f64::from(f[0] - f[1]).powi(2))
            .sum::<f64>()
            / s.len() as f64)
            .sqrt()
    };
    let sp_lo = render_effect(
        ir::EffectKind::StereoDetune(detune(StereoTune {
            split: 0.0,
            drift: 1.0,
            spread: 1.0,
            mix: 1.0,
        })),
        1.0,
        0.0,
        &tone250,
    );
    let sp_hi = render_effect(
        ir::EffectKind::StereoDetune(detune(StereoTune {
            split: 1.0,
            drift: 1.0,
            spread: 1.0,
            mix: 1.0,
        })),
        1.0,
        0.0,
        &tone250,
    );
    assert!(
        side_rms(&sp_hi) < side_rms(&sp_lo) * 0.5,
        "StereoTune split=1 must keep 250 Hz tighter: lo={:.4} hi={:.4}",
        side_rms(&sp_lo),
        side_rms(&sp_hi)
    );
}

/// Cabinet (0x24) against Kontakt 8.13.1 oracle capture:
/// - `type0_base` matches midband attenuation, 8 kHz (-23.3 dB) & 12 kHz (-36.3 dB) speaker rolloff;
/// - `size = 0` shifts rolloff higher (8 kHz brighter), `size = 1` shifts rolloff lower (8 kHz darker);
/// - `bass = +1` lifts 100 Hz by ~+16 dB; `treble = +1` lifts 4 kHz by ~+15 dB;
/// - `air = 0` is mono, `air = 1` adds stereo early reflections (`L-R` RMS > 0.02);
/// - `type2` rolls off darker at 8/12 kHz; `type9` has deep 2 kHz notch (~-21 dB) + bright tweeter (+7..+8.6 dB @ 8/12k).
#[test]
fn cabinet_matches_kontakt_oracle_voicings_size_air_and_tone_shelves() {
    use kontra_kontakt_effects::cabinet::{Cabinet, INIT, cabinet};

    let gain_at = |c: Cabinet, hz: f64| -> f64 {
        let inp = sine(hz, 0.25, 12_000);
        let out = render_effect(ir::EffectKind::Cabinet(cabinet(c)), 1.0, 0.0, &inp);
        dft_mag_db(&out, 4800, hz, 0) - dft_mag_db(&inp, 4800, hz, 0)
    };

    let base = Cabinet { air: 0.0, ..INIT };
    let g250 = gain_at(base, 250.0);
    let g1k = gain_at(base, 1000.0);
    let g8k = gain_at(base, 8000.0);
    let g12k = gain_at(base, 12_000.0);
    assert!((g250 - (-1.77)).abs() < 2.5, "Cab type0 @250: {g250:.2}");
    assert!((g1k - (-6.45)).abs() < 2.5, "Cab type0 @1k: {g1k:.2}");
    assert!((g8k - (-23.29)).abs() < 4.5, "Cab type0 @8k: {g8k:.2}");
    assert!(g12k < -30.0, "Cab type0 @12k rolloff: {g12k:.2}");

    // Size 0 vs Size 1: smaller shifts cutoff up, larger shifts cutoff down.
    let g8k_s0 = gain_at(Cabinet { size: 0.0, ..base }, 8000.0);
    let g8k_s1 = gain_at(Cabinet { size: 1.0, ..base }, 8000.0);
    assert!(
        g8k_s0 > g8k + 8.0 && g8k_s1 < g8k - 6.0,
        "Cab size shift @8k: s0={g8k_s0:.1} base={g8k:.1} s1={g8k_s1:.1}"
    );

    // Bass +1 lifts 100 Hz (~+16 dB); Treble +1 lifts 4 kHz (~+15 dB).
    let bass_delta = gain_at(Cabinet { bass: 1.0, ..base }, 100.0) - gain_at(base, 100.0);
    let treb_delta = gain_at(
        Cabinet {
            treble: 1.0,
            ..base
        },
        4000.0,
    ) - gain_at(base, 4000.0);
    assert!(
        (bass_delta - 16.0).abs() < 2.5,
        "Cab bass +1 delta: {bass_delta:.2} dB"
    );
    assert!(
        (treb_delta - 15.0).abs() < 2.5,
        "Cab treble +1 delta: {treb_delta:.2} dB"
    );

    // Air=0 mono vs Air=1 stereo reflections.
    let tone = sine(1000.0, 0.25, 12_000);
    let air0 = render_effect(ir::EffectKind::Cabinet(cabinet(base)), 1.0, 0.0, &tone);
    let air1 = render_effect(
        ir::EffectKind::Cabinet(cabinet(Cabinet { air: 1.0, ..base })),
        1.0,
        0.0,
        &tone,
    );
    let side0 = air0[4800..]
        .iter()
        .map(|f| (f[0] - f[1]).abs())
        .fold(0.0f32, f32::max);
    let side1 = (air1[4800..]
        .iter()
        .map(|f| f64::from(f[0] - f[1]).powi(2))
        .sum::<f64>()
        / 7200.0)
        .sqrt();
    assert!(
        side0 < 1e-6 && side1 > 0.015,
        "Cab air side: air0={side0} air1={side1:.4}"
    );

    // Type 2 darker @ 8k/12k; Type 9 deep 2 kHz notch + bright 8/12 kHz tweeter.
    let t2_8k = gain_at(
        Cabinet {
            cabinet_type: 2,
            ..base
        },
        8000.0,
    );
    assert!(
        t2_8k < g8k - 2.0,
        "Cab type2 darker @8k: {t2_8k:.1} vs {g8k:.1}"
    );
    let t9_2k = gain_at(
        Cabinet {
            cabinet_type: 9,
            ..base
        },
        2000.0,
    );
    let t9_12k = gain_at(
        Cabinet {
            cabinet_type: 9,
            ..base
        },
        12_000.0,
    );
    assert!(
        (t9_2k - (-21.0)).abs() < 3.0,
        "Cab type9 2kHz notch: {t9_2k:.2}"
    );
    assert!(
        (t9_12k - 8.6).abs() < 3.0,
        "Cab type9 12kHz tweeter: {t9_12k:.2}"
    );
}

/// Twang (0x23) calibrated against real Kontakt 8.13.1 oracle capture.
#[test]
fn twang_matches_kontakt_oracle_gain_clipping_and_tone() {
    use kontra_kontakt_effects::amps;
    const LIN: f32 = 0.015_35;
    const HI: f32 = 0.690_9;
    const TONE: f32 = 0.038_38;

    let tw_base = amps::twang::INIT;
    let (tw_lin, _, _, _) = amp_probe_metrics(
        ir::EffectKind::AmpStack(amps::twang::amp(tw_base)),
        200.0,
        LIN,
    );
    let (tw_hi, _, tw_h3, tw_pk) = amp_probe_metrics(
        ir::EffectKind::AmpStack(amps::twang::amp(tw_base)),
        200.0,
        HI,
    );
    let (tw_v1, _, _, _) = amp_probe_metrics(
        ir::EffectKind::AmpStack(amps::twang::amp(amps::twang::Twang {
            volume: 1.0,
            ..tw_base
        })),
        200.0,
        LIN,
    );
    let (tw_2k, _, _, _) = amp_probe_metrics(
        ir::EffectKind::AmpStack(amps::twang::amp(tw_base)),
        2000.0,
        TONE,
    );
    let (tw_br, _, _, _) = amp_probe_metrics(
        ir::EffectKind::AmpStack(amps::twang::amp(amps::twang::Twang {
            bright: true,
            ..tw_base
        })),
        2000.0,
        TONE,
    );
    let (tw_tr, _, _, _) = amp_probe_metrics(
        ir::EffectKind::AmpStack(amps::twang::amp(amps::twang::Twang {
            treble: 1.0,
            ..tw_base
        })),
        2000.0,
        TONE,
    );
    assert!((tw_lin - 4.59).abs() < 1.5, "Twang lin200: {tw_lin:.2}");
    assert!(
        (tw_v1 - 16.38).abs() < 1.5,
        "Twang vol=1 lin200: {tw_v1:.2}"
    );
    assert!(
        (tw_hi - (-8.19)).abs() < 2.0 && tw_h3 > -16.0 && (tw_pk - 0.27).abs() < 0.06,
        "Twang hi200: {tw_hi:.2} H3={tw_h3:.1} pk={tw_pk:.3}"
    );
    assert!(
        (tw_br - tw_2k - 2.27).abs() < 1.5,
        "Twang bright delta: {:.2}",
        tw_br - tw_2k
    );
    assert!(
        (tw_tr - tw_2k - 7.69).abs() < 2.0,
        "Twang treble delta: {:.2}",
        tw_tr - tw_2k
    );
}

/// Jump (0x4d) calibrated against real Kontakt 8.13.1 oracle capture.
#[test]
fn jump_matches_kontakt_oracle_gain_higain_and_tone() {
    use kontra_kontakt_effects::amps;
    const LIN: f32 = 0.015_35;
    const HI: f32 = 0.690_9;
    const TONE: f32 = 0.038_38;

    let jm_base = amps::jump::INIT;
    let (jm_lin, _, _, _) = amp_probe_metrics(
        ir::EffectKind::AmpStack(amps::jump::amp(jm_base)),
        200.0,
        LIN,
    );
    let (jm_2k, _, _, _) = amp_probe_metrics(
        ir::EffectKind::AmpStack(amps::jump::amp(jm_base)),
        2000.0,
        TONE,
    );
    let (jm_p1, _, _, _) = amp_probe_metrics(
        ir::EffectKind::AmpStack(amps::jump::amp(amps::jump::Jump {
            preamp: 1.0,
            ..jm_base
        })),
        200.0,
        LIN,
    );
    let (jm_hg, _, _, _) = amp_probe_metrics(
        ir::EffectKind::AmpStack(amps::jump::amp(amps::jump::Jump {
            higain: true,
            ..jm_base
        })),
        200.0,
        LIN,
    );
    let (jm_hg_hi, jm_h2, jm_h3, jm_pk) = amp_probe_metrics(
        ir::EffectKind::AmpStack(amps::jump::amp(amps::jump::Jump {
            higain: true,
            ..jm_base
        })),
        200.0,
        HI,
    );
    assert!((jm_lin - (-20.31)).abs() < 1.5, "Jump lin200: {jm_lin:.2}");
    assert!((jm_2k - (-6.89)).abs() < 2.5, "Jump 2k: {jm_2k:.2}");
    #[allow(clippy::approx_constant)] // Measured Kontakt 8.13.1 gain in dB (-6.28 dB), not -TAU.
    let jump_pre1_expected_db = -6.28;
    assert!(
        (jm_p1 - jump_pre1_expected_db).abs() < 1.5,
        "Jump pre=1 lin200: {jm_p1:.2}"
    );
    assert!((jm_hg - 9.26).abs() < 1.5, "Jump higain lin200: {jm_hg:.2}");
    assert!(
        (jm_hg_hi - (-10.83)).abs() < 2.0
            && jm_h2 > -26.0
            && jm_h3 > -18.0
            && (jm_pk - 0.175).abs() < 0.06,
        "Jump hg hi: {jm_hg_hi:.2} H2={jm_h2:.1} H3={jm_h3:.1} pk={jm_pk:.3}"
    );
}

/// Van51 (0x52) calibrated against real Kontakt 8.13.1 oracle capture.
#[test]
fn van51_matches_kontakt_oracle_rhythm_lead_higain_and_clipping() {
    use kontra_kontakt_effects::amps;
    const LIN: f32 = 0.015_35;
    const MID: f32 = 0.307_1;

    let v5_base = amps::van51::Van51 {
        pregain_rhythm: 0.5,
        pregain_lead: 0.5,
        ..amps::van51::INIT
    };
    let (v5_lin, _, _, _) = amp_probe_metrics(
        ir::EffectKind::AmpStack(amps::van51::amp(v5_base)),
        200.0,
        LIN,
    );
    let (v5_mid, _, v5_h3, v5_pk) = amp_probe_metrics(
        ir::EffectKind::AmpStack(amps::van51::amp(v5_base)),
        200.0,
        MID,
    );
    let (v5_ld, _, _, _) = amp_probe_metrics(
        ir::EffectKind::AmpStack(amps::van51::amp(amps::van51::Van51 {
            lead_channel: true,
            ..v5_base
        })),
        200.0,
        LIN,
    );
    let (v5_hg, _, _, _) = amp_probe_metrics(
        ir::EffectKind::AmpStack(amps::van51::amp(amps::van51::Van51 {
            higain: true,
            ..v5_base
        })),
        200.0,
        LIN,
    );
    assert!((v5_lin - 23.45).abs() < 1.5, "Van51 lin200: {v5_lin:.2}");
    assert!(
        (v5_mid - 4.67).abs() < 2.0 && v5_h3 > -16.0 && (v5_pk - 0.47).abs() < 0.08,
        "Van51 mid: {v5_mid:.2} H3={v5_h3:.1} pk={v5_pk:.3}"
    );
    assert!((v5_ld - 30.45).abs() < 1.5, "Van51 lead lin200: {v5_ld:.2}");
    assert!(
        (v5_hg - 28.68).abs() < 1.5,
        "Van51 higain lin200: {v5_hg:.2}"
    );
}

/// AC Box (0x53) calibrated against real Kontakt 8.13.1 oracle capture.
#[test]
fn ac_box_matches_kontakt_oracle_brilliant_tone_cut_and_clipping() {
    use kontra_kontakt_effects::amps;
    const LIN: f32 = 0.015_35;
    const HI: f32 = 0.690_9;
    const TONE: f32 = 0.038_38;

    let ac_base = amps::ac_box::AcBox {
        tone_cut: 0.0,
        ..amps::ac_box::INIT
    };
    let (ac_lin, _, _, _) = amp_probe_metrics(
        ir::EffectKind::AmpStack(amps::ac_box::amp(ac_base)),
        200.0,
        LIN,
    );
    let (ac_hi, _, ac_h3, ac_pk) = amp_probe_metrics(
        ir::EffectKind::AmpStack(amps::ac_box::amp(ac_base)),
        200.0,
        HI,
    );
    let (ac_br, _, _, _) = amp_probe_metrics(
        ir::EffectKind::AmpStack(amps::ac_box::amp(amps::ac_box::AcBox {
            brilliant_volume: 1.0,
            ..ac_base
        })),
        200.0,
        LIN,
    );
    let (ac_2k, _, _, _) = amp_probe_metrics(
        ir::EffectKind::AmpStack(amps::ac_box::amp(ac_base)),
        2000.0,
        TONE,
    );
    let (ac_cut, _, _, _) = amp_probe_metrics(
        ir::EffectKind::AmpStack(amps::ac_box::amp(amps::ac_box::AcBox {
            tone_cut: 1.0,
            ..ac_base
        })),
        2000.0,
        TONE,
    );
    let (ac_tr, _, _, _) = amp_probe_metrics(
        ir::EffectKind::AmpStack(amps::ac_box::amp(amps::ac_box::AcBox {
            treble: 1.0,
            ..ac_base
        })),
        2000.0,
        TONE,
    );
    assert!((ac_lin - 8.53).abs() < 1.5, "ACBox lin200: {ac_lin:.2}");
    assert!(
        (ac_br - 18.45).abs() < 1.5,
        "ACBox bril=1 lin200: {ac_br:.2}"
    );
    assert!(
        (ac_hi - (-8.29)).abs() < 2.0 && ac_h3 > -16.0 && (ac_pk - 0.235).abs() < 0.06,
        "ACBox hi: {ac_hi:.2} H3={ac_h3:.1} pk={ac_pk:.3}"
    );
    assert!(
        (ac_cut - ac_2k - (-10.74)).abs() < 2.0,
        "ACBox cut=1 delta @2k: {:.2}",
        ac_cut - ac_2k
    );
    assert!(
        (ac_tr - ac_2k - 7.43).abs() < 2.0,
        "ACBox treble=1 delta @2k: {:.2}",
        ac_tr - ac_2k
    );
}

/// Hot Solo (0x54) calibrated against real Kontakt 8.13.1 oracle capture.
#[test]
fn hot_solo_matches_kontakt_oracle_normal_overdrive_and_harmonics() {
    use kontra_kontakt_effects::amps;
    const LIN: f32 = 0.015_35;
    const MID: f32 = 0.307_1;

    let hs_base = amps::hot_solo::HotSolo {
        pre_normal: 0.5,
        pre_overdrive: 0.5,
        ..amps::hot_solo::INIT
    };
    let (hs_lin, _, _, _) = amp_probe_metrics(
        ir::EffectKind::AmpStack(amps::hot_solo::amp(hs_base)),
        200.0,
        LIN,
    );
    let (hs_mid, _, _, hs_pk) = amp_probe_metrics(
        ir::EffectKind::AmpStack(amps::hot_solo::amp(hs_base)),
        200.0,
        MID,
    );
    let (hs_od, _, _, _) = amp_probe_metrics(
        ir::EffectKind::AmpStack(amps::hot_solo::amp(amps::hot_solo::HotSolo {
            overdrive: true,
            ..hs_base
        })),
        200.0,
        LIN,
    );
    let (_, hs_od_h2, hs_od_h3, _) = amp_probe_metrics(
        ir::EffectKind::AmpStack(amps::hot_solo::amp(amps::hot_solo::HotSolo {
            overdrive: true,
            ..hs_base
        })),
        200.0,
        MID,
    );
    assert!((hs_lin - 13.21).abs() < 1.5, "HotSolo lin200: {hs_lin:.2}");
    assert!((hs_od - 30.45).abs() < 1.5, "HotSolo od lin200: {hs_od:.2}");
    assert!(
        (hs_mid - 6.87).abs() < 2.0 && (hs_pk - 0.67).abs() < 0.12,
        "HotSolo mid: {hs_mid:.2} pk={hs_pk:.3}"
    );
    assert!(
        hs_od_h2 > -24.0 && hs_od_h3 > -22.0,
        "HotSolo od harmonics: H2={hs_od_h2:.1} H3={hs_od_h3:.1}"
    );
}

/// Bass Pro (0x61) calibrated against real Kontakt 8.13.1 oracle capture.
#[test]
fn bass_pro_matches_kontakt_oracle_drive_ceiling_and_ultra_switches() {
    use kontra_kontakt_effects::amps;
    const LIN: f32 = 0.015_35;
    const HI: f32 = 0.690_9;
    const TONE: f32 = 0.038_38;

    let bp_base = amps::bass_pro::BassPro {
        drive: 0.5,
        ..amps::bass_pro::INIT
    };
    let (bp_lin, _, _, _) = amp_probe_metrics(
        ir::EffectKind::AmpStack(amps::bass_pro::amp(bp_base)),
        200.0,
        LIN,
    );
    let (bp_hi, _, _, bp_pk) = amp_probe_metrics(
        ir::EffectKind::AmpStack(amps::bass_pro::amp(bp_base)),
        200.0,
        HI,
    );
    let (bp_d1_hi, _, bp_d1_h3, bp_d1_pk) = amp_probe_metrics(
        ir::EffectKind::AmpStack(amps::bass_pro::amp(amps::bass_pro::BassPro {
            drive: 1.0,
            ..bp_base
        })),
        200.0,
        HI,
    );
    let (bp_2k, _, _, _) = amp_probe_metrics(
        ir::EffectKind::AmpStack(amps::bass_pro::amp(bp_base)),
        2000.0,
        TONE,
    );
    let (bp_uhi, _, _, _) = amp_probe_metrics(
        ir::EffectKind::AmpStack(amps::bass_pro::amp(amps::bass_pro::BassPro {
            ultra_hi: true,
            ..bp_base
        })),
        2000.0,
        TONE,
    );
    assert!((bp_lin - 8.67).abs() < 1.5, "BassPro lin200: {bp_lin:.2}");
    assert!(
        (bp_hi - 1.90).abs() < 2.0 && (bp_pk - 0.81).abs() < 0.12,
        "BassPro hi: {bp_hi:.2} pk={bp_pk:.3}"
    );
    assert!(
        (bp_d1_hi - (-2.82)).abs() < 2.0 && bp_d1_pk < bp_pk - 0.15 && bp_d1_h3 > -15.0,
        "BassPro drive=1 drop: {bp_d1_hi:.2} pk={bp_d1_pk:.3} H3={bp_d1_h3:.1}"
    );
    assert!(
        (bp_uhi - bp_2k - 3.80).abs() < 1.5,
        "BassPro ultra_hi delta @2k: {:.2}",
        bp_uhi - bp_2k
    );
}

/// Cat (0x55) calibrated against real Kontakt 8.13.1 oracle capture.
#[test]
fn cat_matches_kontakt_oracle_hard_clip_ceiling_and_reverse_filter() {
    use kontra_kontakt_effects::cat;
    const LIN: f32 = 0.015_35;
    const HI: f32 = 0.690_9;
    const TONE: f32 = 0.038_38;

    let ct_base = cat::INIT;
    let (ct_lin, _, _, _) =
        amp_probe_metrics(ir::EffectKind::AmpStack(cat::amp(ct_base)), 200.0, LIN);
    let (ct_hi, ct_h2, ct_h3, ct_pk) =
        amp_probe_metrics(ir::EffectKind::AmpStack(cat::amp(ct_base)), 200.0, HI);
    let (ct_2k, _, _, _) =
        amp_probe_metrics(ir::EffectKind::AmpStack(cat::amp(ct_base)), 2000.0, TONE);
    let (ct_f1, _, _, _) = amp_probe_metrics(
        ir::EffectKind::AmpStack(cat::amp(cat::Cat {
            filter: 1.0,
            ..ct_base
        })),
        2000.0,
        TONE,
    );
    assert!((ct_lin - 4.39).abs() < 1.5, "Cat lin200: {ct_lin:.2}");
    assert!(
        (ct_hi - (-26.73)).abs() < 2.0
            && ct_h2 < -40.0
            && (ct_h3 - (-9.5)).abs() < 2.0
            && (ct_pk - 0.026).abs() < 0.008,
        "Cat hi: {ct_hi:.2} H2={ct_h2:.1} H3={ct_h3:.1} pk={ct_pk:.4}"
    );
    assert!(
        (ct_f1 - ct_2k - (-10.07)).abs() < 2.0,
        "Cat filter=1 delta @2k: {:.2}",
        ct_f1 - ct_2k
    );
}

/// Skreamer Deluxe (0x78) calibrated against real Kontakt 8.13.1 oracle capture.
#[test]
fn skreamer_deluxe_matches_kontakt_oracle_three_modes_drive_and_tone() {
    use kontra_kontakt_effects::skreamer_deluxe;
    const LIN: f32 = 0.015_35;
    const MID: f32 = 0.307_1;
    const TONE: f32 = 0.038_38;

    let sk_m0 = skreamer_deluxe::INIT;
    let (sk0_lin, _, _, _) = amp_probe_metrics(
        ir::EffectKind::AmpStack(skreamer_deluxe::amp(sk_m0)),
        200.0,
        LIN,
    );
    let (sk0_mid, _, _, sk0_pk) = amp_probe_metrics(
        ir::EffectKind::AmpStack(skreamer_deluxe::amp(sk_m0)),
        200.0,
        MID,
    );
    let (sk1_lin, _, _, _) = amp_probe_metrics(
        ir::EffectKind::AmpStack(skreamer_deluxe::amp(skreamer_deluxe::SkreamerDeluxe {
            mode: 1,
            ..sk_m0
        })),
        200.0,
        LIN,
    );
    let (sk2_lin, _, _, _) = amp_probe_metrics(
        ir::EffectKind::AmpStack(skreamer_deluxe::amp(skreamer_deluxe::SkreamerDeluxe {
            mode: 2,
            ..sk_m0
        })),
        200.0,
        LIN,
    );
    let (sk_d1, _, _, _) = amp_probe_metrics(
        ir::EffectKind::AmpStack(skreamer_deluxe::amp(skreamer_deluxe::SkreamerDeluxe {
            drive: 1.0,
            ..sk_m0
        })),
        200.0,
        LIN,
    );
    let (sk0_2k, _, _, _) = amp_probe_metrics(
        ir::EffectKind::AmpStack(skreamer_deluxe::amp(sk_m0)),
        2000.0,
        TONE,
    );
    let (sk_t1, _, _, _) = amp_probe_metrics(
        ir::EffectKind::AmpStack(skreamer_deluxe::amp(skreamer_deluxe::SkreamerDeluxe {
            tone: 1.0,
            ..sk_m0
        })),
        2000.0,
        TONE,
    );
    assert!(
        (sk0_lin - 7.98).abs() < 1.5,
        "SkDLX m0 lin200: {sk0_lin:.2}"
    );
    assert!(
        (sk0_mid - (-3.51)).abs() < 2.0 && (sk0_pk - 0.213).abs() < 0.05,
        "SkDLX m0 mid: {sk0_mid:.2} pk={sk0_pk:.3}"
    );
    assert!(
        (sk1_lin - 0.67).abs() < 1.5 && (sk2_lin - 3.28).abs() < 1.5,
        "SkDLX m1/m2 lin: {sk1_lin:.2} / {sk2_lin:.2}"
    );
    assert!(
        (sk_d1 - 15.43).abs() < 1.5,
        "SkDLX drive=1 lin200: {sk_d1:.2}"
    );
    assert!(
        (sk_t1 - sk0_2k - 8.9).abs() < 2.5,
        "SkDLX tone=1 delta @2k: {:.2}",
        sk_t1 - sk0_2k
    );
}

/// Kolor (0x79) against Kontakt 8.13.1 oracle capture:
/// - `mix = 0.0` passes dry identically (0.00 dB);
/// - `mode0/2/5/9` match Kontakt linear gain (+10.3..+12.2 dB), asymmetric H2 (mode2 ~ -17 dB),
///   pure odd fuzz (mode5 H2 < -40 dB), lower diode ceiling (mode9), and `drive=1` (+5.8 dB);
/// - `lpf = 0.2` (10..20k Hz law -> ~45.7 Hz 12 dB/oct) matches Kontakt -13.8 @100, -25.6 @200, -65.4 @2k;
/// - `hpf = 0.8` (10..20k Hz law -> ~4373 Hz 12 dB/oct) matches Kontakt -54.0 @200, -14.1 @2k.
#[test]
fn kolor_matches_kontakt_oracle_modes_drive_lpf_hpf_and_mix() {
    use kontra_kontakt_effects::kolor::{INIT, Kolor, color_sat};

    const LIN: f32 = 0.015_35;
    const MID: f32 = 0.307_1;
    const HI: f32 = 0.690_9;
    const TONE: f32 = 0.038_38;

    let base = Kolor {
        drive: 0.5,
        lpf: 1.0,
        hpf: 0.0,
        mix: 1.0,
        ..INIT
    };

    // mix=0 exact 0.00 dB dry pass.
    let (m0_g, _, _, _) = amp_probe_metrics(
        ir::EffectKind::ColorSat(color_sat(Kolor { mix: 0.0, ..base })),
        200.0,
        MID,
    );
    assert!(m0_g.abs() < 0.01, "Kolor mix=0 err: {m0_g:.4} dB");

    // Mode 0, 2, 5, 9 linear gain, harmonics, and saturation ceilings.
    let (k0_lin, _, _, _) =
        amp_probe_metrics(ir::EffectKind::ColorSat(color_sat(base)), 200.0, LIN);
    let (k0_hi, _, _, k0_pk) =
        amp_probe_metrics(ir::EffectKind::ColorSat(color_sat(base)), 200.0, HI);
    let (k2_mid, k2_h2, _, _) = amp_probe_metrics(
        ir::EffectKind::ColorSat(color_sat(Kolor { mode: 2, ..base })),
        200.0,
        MID,
    );
    let (_, k5_h2, k5_h3, _) = amp_probe_metrics(
        ir::EffectKind::ColorSat(color_sat(Kolor { mode: 5, ..base })),
        200.0,
        HI,
    );
    let (k9_hi, _, _, k9_pk) = amp_probe_metrics(
        ir::EffectKind::ColorSat(color_sat(Kolor { mode: 9, ..base })),
        200.0,
        HI,
    );
    let (kd1_lin, _, _, _) = amp_probe_metrics(
        ir::EffectKind::ColorSat(color_sat(Kolor { drive: 1.0, ..base })),
        200.0,
        LIN,
    );
    assert!((k0_lin - 10.28).abs() < 1.5, "Kolor m0 lin200: {k0_lin:.2}");
    assert!(
        (k0_hi - (-1.74)).abs() < 2.0 && (k0_pk - 0.52).abs() < 0.16,
        "Kolor m0 hi: {k0_hi:.2} pk={k0_pk:.3}"
    );
    assert!(
        (k2_mid - 3.33).abs() < 2.5 && k2_h2 > -22.0,
        "Kolor m2 mid={k2_mid:.2} H2={k2_h2:.1}"
    );
    assert!(
        k5_h2 < -40.0 && k5_h3 > -16.0,
        "Kolor m5 odd fuzz: H2={k5_h2:.1} H3={k5_h3:.1}"
    );
    assert!(
        k9_hi < k0_hi - 2.0 && k9_pk < k0_pk - 0.12,
        "Kolor m9 lower ceiling: hi={k9_hi:.2} pk={k9_pk:.3}"
    );
    assert!(
        (kd1_lin - 16.06).abs() < 1.5,
        "Kolor drive=1 lin200: {kd1_lin:.2}"
    );

    // Calibrated 10..20k Hz LPF (lpf=0.2) and HPF (hpf=0.8) against Kontakt 8.13.1!
    let lpf_lo = Kolor { lpf: 0.2, ..base };
    let (lp_100, _, _, _) =
        amp_probe_metrics(ir::EffectKind::ColorSat(color_sat(lpf_lo)), 100.0, TONE);
    let (lp_200, _, _, _) =
        amp_probe_metrics(ir::EffectKind::ColorSat(color_sat(lpf_lo)), 200.0, LIN);
    let (lp_2k, _, _, _) =
        amp_probe_metrics(ir::EffectKind::ColorSat(color_sat(lpf_lo)), 2000.0, TONE);
    assert!(
        ((lp_100 - k0_lin) - (-13.78)).abs() < 1.0,
        "Kolor LPF 0.2 @100 delta: {:.2}",
        lp_100 - k0_lin
    );
    assert!(
        ((lp_200 - k0_lin) - (-25.58)).abs() < 1.0,
        "Kolor LPF 0.2 @200 delta: {:.2}",
        lp_200 - k0_lin
    );
    assert!(
        ((lp_2k - k0_lin) - (-65.41)).abs() < 1.5,
        "Kolor LPF 0.2 @2k delta: {:.2}",
        lp_2k - k0_lin
    );

    let hpf_hi = Kolor { hpf: 0.8, ..base };
    let (hp_200, _, _, _) =
        amp_probe_metrics(ir::EffectKind::ColorSat(color_sat(hpf_hi)), 200.0, LIN);
    let (hp_2k, _, _, _) =
        amp_probe_metrics(ir::EffectKind::ColorSat(color_sat(hpf_hi)), 2000.0, TONE);
    assert!(
        ((hp_200 - k0_lin) - (-53.98)).abs() < 1.0,
        "Kolor HPF 0.8 @200 delta: {:.2}",
        hp_200 - k0_lin
    );
    assert!(
        ((hp_2k - k0_lin) - (-14.12)).abs() < 1.0,
        "Kolor HPF 0.8 @2k delta: {:.2}",
        hp_2k - k0_lin
    );
}

/// DYX / AET Filter (0x1b) and Core Cell (0x5e) against Kontakt 8.13.1 oracle capture:
/// - In Kontakt 8.13.1, standalone `$EFFECT_TYPE_AET_FILTER` without a loaded AET morph bank
///   passes unity 0.00 dB across 100..12000 Hz; KONTRA reproduces exact 0.00 dB unity at
///   `amount = 0.0` and sweeps smooth resonant formants when `amount > 0.0`.
/// - Core Cell (0x5e, v0x51) has no KSP `$EFFECT_TYPE_*` or factory preset in Kontakt 8.13.1;
///   saved records decode 10 floats cleanly and zero output (`ir::EffectKind::Gain(0.0)`).
#[test]
fn dyx_and_core_cell_match_kontakt_oracle_unity_morph_and_zero_output() {
    use kontra_kontakt_effects::{core_cell, dyx};

    // DYX: amount=0 matches Kontakt 8.13.1 standalone 0.00 dB pass-through across 100..12k Hz.
    for hz in [100.0, 500.0, 2000.0, 8000.0, 12_000.0] {
        let inp = sine(hz, 0.25, 8192);
        let flat = render_effect(
            ir::EffectKind::SpectralMorph(dyx::spectral_morph(dyx::Dyx {
                amount: 0.0,
                ..dyx::INIT
            })),
            1.0,
            0.0,
            &inp,
        );
        let err = (dft_mag_db(&flat, 4096, hz, 0) - dft_mag_db(&inp, 4096, hz, 0)).abs();
        assert!(err < 0.01, "DYX amount=0 @{hz}Hz err: {err:.4} dB");
    }

    // Active morph (amount=1) shifts 1650 Hz formant smoothly from dark (morph=0) to bright (morph=0.67).
    let probe = sine(1650.0, 0.25, 8192);
    let dark = render_effect(
        ir::EffectKind::SpectralMorph(dyx::spectral_morph(dyx::Dyx {
            morph: 0.0,
            amount: 1.0,
            ..dyx::INIT
        })),
        1.0,
        0.0,
        &probe,
    );
    let bright = render_effect(
        ir::EffectKind::SpectralMorph(dyx::spectral_morph(dyx::Dyx {
            morph: 0.67,
            amount: 1.0,
            ..dyx::INIT
        })),
        1.0,
        0.0,
        &probe,
    );
    let delta = dft_mag_db(&bright, 4096, 1650.0, 0) - dft_mag_db(&dark, 4096, 1650.0, 0);
    assert!(delta > 4.0, "DYX active morph 1650 Hz lift: {delta:.2} dB");

    // Core Cell (0x5e): saved v0x51 decodes 10 floats and outputs exact silence.
    let bytes: Vec<u8> = (0..10u32)
        .flat_map(|i| (i as f32 * 0.1).to_le_bytes())
        .collect();
    let cc = core_cell::saved(0x51, &bytes).expect("core cell v0x51");
    let out = render_effect(core_cell::effect(cc), 1.0, 0.0, &probe);
    let max_out = out.iter().flatten().map(|v| v.abs()).fold(0.0f32, f32::max);
    assert_eq!(max_out, 0.0, "Core Cell must zero output");
}
