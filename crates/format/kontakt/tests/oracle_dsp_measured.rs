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
