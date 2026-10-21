//! Band-solo filter of soothe3's process block, translated from `0x89a1a0` (the per-sample
//! filter and the Tilt copy), `0x89bf10` (the per-channel coefficient store) and `0x88e150`
//! (the TPT state-variable coefficients).
//!
//! What the binary does, as established by disassembly and by the emulated fixture
//! (`research/re-tools/emulate/solo_filter_fixture.py`, `fixtures/solo_filter.json`):
//!
//! - The solo band is `obj + 0x10c + 0x20 * idx` (`0x890a00`, the call at `0x89257d`). This module
//!   reads its fields the way the filter does: frequency `+0x04` (f32), half bandwidth `+0x0c`
//!   (f32, octaves) and shape `+0x1c` (u32). The setter writes shape to `+0x18` (index 26), so
//!   the filter reads the field the setter calls `selector` (index 27). See the notes in
//!   `research/stereo-ms-sidechain-output.md`, which the port has to resolve.
//! - One [`SoloChannel`] per channel, 0x78 bytes, laid out as the binary reads it (`repr(C)`).
//! - [`store_coefficients`] (`0x89bf10`): mode and targets. Shapes `{2, 3}` band-pass (mode 2),
//!   `{1, 7}` low-pass (0), `{0, 6}` high-pass (1), `{4}` notch (3), `{5}` copies the input
//!   (no state). For the filter modes, target R is the clamped band frequency
//!   `R = (t >= f) ? max(f, 1e-6) : t` with `t = fs * 0.499`, and target k is `max(hb, 0)`
//!   for band-pass and notch, `1.0` for low-pass and high-pass.
//! - [`svf_coefficients`] (`0x88e150`): from the smoothed R and k, the TPT coefficients
//!   `g = tan(pi f / fs)`, `k = 1 / Q` from the bandwidth in octaves, `a1 = 1/(1 + g(g+k))`,
//!   `a2 = a1 g`, `a3 = a2 g`.
//! - [`process`] (`0x89a1a0`): per sample, per channel, the smoothed R and k move toward their
//!   targets by a one-pole step with rate `+0x38` when that rate is below 1, and snap otherwise.
//!   The coefficients are then recomputed (`0x88e150`) and the TPT update runs. Output is the
//!   LP, HP, BP or notch of the mode. Samples are rounded to f32 on store.
//!
//! The constructor sets the state (sample rate at `+0`, rate `1 - exp(-1/(0.03 fs))` at `+0x38`,
//! R and k at 1000 Hz and 1.0). The fixture uses the constructor's values.

/// Clamp floor for the frequency (`[0x18209d178]`, 1e-6).
pub const FREQ_FLOOR: f64 = 1.0e-6;
/// Nyquist factor (`[0x18209d3c0]`, 0.499): R is capped at `0.499 * fs`.
pub const NYQUIST_FACTOR: f64 = 0.499;
/// `pi` as stored at `[0x18209d4c8]`.
pub const PI: f64 = std::f64::consts::PI;
/// `log2(e)` as stored at `[0x18209d480]`.
pub const LOG2_E: f64 = 1.442_695_040_888_963_4;
/// Lower bound of the `g` argument of the log (`[0x18209d028]`, 1e-30).
pub const G_FLOOR: f64 = 1.0e-30;
/// Fallback `k` when the bandwidth is not positive (`[0x18209d6f0]`, 1e9).
pub const K_FALLBACK: f64 = 1.0e9;
/// Floor on `k` before the reciprocal (`[0x18209d0e0]`, 1e-9).
pub const K_FLOOR: f64 = 1.0e-9;
/// The 1.0 that numerator, smoothing threshold and `a1` use (`[0x18209d450]`).
pub const ONE: f64 = 1.0;

/// Band fields the solo filter reads (`obj + 0x10c + 0x20 * idx`), as the binary reads them.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SoloBand {
    /// `+0x04`, Hz.
    pub freq: f32,
    /// `+0x0c`, half bandwidth in octaves.
    pub half_bw_oct: f32,
    /// `+0x1c`, shape index 0..=7 (LowCut, LowShelf, Bell, Bandpass, BandReject, Tilt, HighShelf, HighCut).
    pub shape: u32,
}

/// One channel's solo state, `0x78` bytes, at `obj + 0xb7a8 + 0x78 * channel`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct SoloChannel {
    /// `+0x00`: sample rate.
    pub fs: f64,
    /// `+0x08`: filter mode, 0 low-pass, 1 high-pass, 2 band-pass, 3 notch.
    pub mode: i32,
    /// `+0x10`: target R (frequency, Hz), from [`store_coefficients`].
    pub target_r: f64,
    /// `+0x18`: target k (1/Q), from [`store_coefficients`].
    pub target_k: f64,
    /// `+0x20`: smoothed R.
    pub cur_r: f64,
    /// `+0x28`: smoothed k.
    pub cur_k: f64,
    /// `+0x30`: not read by the filter. The constructor writes 30.0 (the time constant in ms).
    pub tau_ms: f64,
    /// `+0x38`: smoothing rate, `1 - exp(-1 / (0.03 fs))` from the constructor.
    pub rate: f64,
    /// `+0x40`: g = tan(pi f / fs), from [`svf_coefficients`].
    pub g: f64,
    /// `+0x48`: k, from [`svf_coefficients`].
    pub k: f64,
    /// `+0x50`: a1.
    pub a1: f64,
    /// `+0x58`: a2.
    pub a2: f64,
    /// `+0x60`: a3.
    pub a3: f64,
    /// `+0x68`: TPT integrator 1.
    pub ic1: f64,
    /// `+0x70`: TPT integrator 2.
    pub ic2: f64,
}

const _: () = assert!(std::mem::size_of::<SoloChannel>() == 0x78);

/// The clamp the binary builds from its three-slot select (`0x88e150`, `0x89bf10`):
/// `p = (lo <= x) ? x : lo`, then `(x <= hi) ? p : hi`. NaN takes `lo` then `hi`.
#[inline]
fn clamp3(x: f64, lo: f64, hi: f64) -> f64 {
    let p = if lo <= x { x } else { lo };
    if x <= hi {
        p
    } else {
        hi
    }
}

/// `0x89bf10`: writes mode and targets into every channel record (`states.len()` channels).
///
/// Shape 5 (Tilt) writes nothing: the solo stream is a copy of the input.
pub fn store_coefficients(states: &mut [SoloChannel], band: &SoloBand) {
    let shape = band.shape;
    let (mode, band_pass) = match shape {
        2 | 3 => (2, true),
        1 | 7 => (0, false),
        0 | 6 => (1, false),
        4 => (3, true),
        _ => return,
    };
    let freq = band.freq as f64;
    let hb = band.half_bw_oct as f64;
    for s in states.iter_mut() {
        s.mode = mode;
        let t = s.fs * NYQUIST_FACTOR;
        // comisd/cmovae in 0x89bf10: A = freq if freq >= floor else floor; R = t >= freq ? A : t.
        let a = if freq >= FREQ_FLOOR { freq } else { FREQ_FLOOR };
        s.target_r = if t >= freq { a } else { t };
        s.target_k = if band_pass {
            // maxsd(hb, 0.0): NaN gives 0.0
            if hb > 0.0 {
                hb
            } else {
                0.0
            }
        } else {
            ONE
        };
    }
}

/// `0x88e150`: from the smoothed R (`cur_r`) and k (`cur_k`), writes g, k and the TPT coefficients.
pub fn svf_coefficients(s: &mut SoloChannel) {
    let fs = s.fs;
    let nyquist = fs * NYQUIST_FACTOR;
    let f = clamp3(s.cur_r, FREQ_FLOOR, nyquist);
    let g = (f * PI / fs).tan();

    // Bandwidth edges in the g domain: g * 2^(-hb) and g * 2^(+hb), hb = max(cur_k, 0) / 2.
    let hb = (if s.cur_k <= 0.0 { 0.0 } else { s.cur_k }) * 0.5;
    let log_g = (if g <= G_FLOOR { G_FLOOR } else { g }).ln() * LOG2_E;
    let lo = (log_g - hb).exp2();
    let hi = (log_g + hb).exp2();
    let span = hi - lo;
    let mut kk = if G_FLOOR < span { g / span } else { K_FALLBACK };
    if kk <= K_FLOOR {
        kk = K_FLOOR;
    }
    let k = ONE / kk;

    s.k = k;
    s.g = g;
    s.a1 = ONE / ((k + g) * g + ONE);
    s.a2 = s.a1 * g;
    s.a3 = s.a2 * g;
}

/// `0x89a1a0`: the solo filter over one block of `n` samples.
///
/// `states` holds one record per output channel and `band` is the solo band. `input` is the
/// main signal (one slice per channel, or one shared mono slice). `out` receives the solo stream.
/// With shape 5 the stream is a copy of `input` (same channel count), or of `input[0]` to every
/// output channel when the input is mono. Mismatched counts other than that write nothing.
pub fn process(states: &mut [SoloChannel], band: &SoloBand, input: &[&[f32]], out: &mut [&mut [f32]], n: usize) {
    let ch = out.len();
    if band.shape == 5 {
        if ch == 0 || n == 0 {
            return;
        }
        if input.len() == ch {
            for (o, i) in out.iter_mut().zip(input) {
                o[..n].copy_from_slice(&i[..n]);
            }
        } else if input.len() == 1 {
            for o in out.iter_mut() {
                o[..n].copy_from_slice(&input[0][..n]);
            }
        }
        return;
    }

    store_coefficients(&mut states[..ch], band);
    if n == 0 || ch == 0 {
        return;
    }
    for i in 0..n {
        for (k, s) in states[..ch].iter_mut().enumerate() {
            let x = input[k][i] as f64;

            // One-pole approach to the targets while rate < 1 (`comisd`/`ja`), snap otherwise.
            if s.rate < ONE {
                s.cur_r = (s.target_r - s.cur_r) * s.rate + s.cur_r;
                s.cur_k = (s.target_k - s.cur_k) * s.rate + s.cur_k;
            } else {
                s.cur_r = s.target_r;
                s.cur_k = s.target_k;
            }
            svf_coefficients(s);

            // TPT state-variable update: v3 = x - ic2, v1 = a1 ic1 + a2 v3, v2 = a2 ic1 + ic2 + a3 v3.
            let v3 = x - s.ic2;
            let v1 = s.ic1 * s.a1 + v3 * s.a2;
            let v2 = s.ic1 * s.a2 + s.ic2 + v3 * s.a3;
            s.ic1 = 2.0 * v1 - s.ic1;
            s.ic2 = 2.0 * v2 - s.ic2;
            let hp = (x - v1 * s.k) - v2;
            let y = match s.mode {
                1 => hp,
                2 => v1 * s.k,
                3 => hp + v2,
                _ => v2,
            };
            out[k][i] = y as f32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("fixtures/solo_filter.json");

    fn f64_of(bits: u64) -> f64 {
        f64::from_bits(bits)
    }

    /// Replays every fixture case from the state the emulator started it with and compares the
    /// output samples and the state fields the call writes, bit for bit (1e-6 relative allowed).
    #[test]
    fn solo_filter_matches_the_emulated_entry_point() {
        let fixture: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        assert!(cases.len() >= 300, "fixture covers the shapes, copy branch and smoothing");

        let mut states = [SoloChannel::default(); 2];
        let mut nonzero = 0usize;
        for (idx, case) in cases.iter().enumerate() {
            let before = case["state_before"].as_array().unwrap();
            for (c, rec) in before.iter().enumerate() {
                states[c] = record_from_bits(rec);
            }
            let band = SoloBand {
                freq: f32::from_bits(case["freq"].as_u64().unwrap() as u32),
                half_bw_oct: f32::from_bits(case["half_bw"].as_u64().unwrap() as u32),
                shape: case["shape"].as_u64().unwrap() as u32,
            };
            let n = case["n"].as_u64().unwrap() as usize;
            let ch = case["channels"].as_u64().unwrap() as usize;
            let in_ch = case["in_channels"].as_u64().unwrap() as usize;

            let inputs: Vec<Vec<f32>> = case["in"].as_array().unwrap().iter().map(f32s).collect();
            let in_refs: Vec<&[f32]> = inputs.iter().map(|v| v.as_slice()).collect();
            assert_eq!(in_refs.len(), in_ch, "case {idx}: input channels");
            let mut outs: Vec<Vec<f32>> = vec![vec![0.0; n]; ch];
            let mut out_refs: Vec<&mut [f32]> = outs.iter_mut().map(|v| v.as_mut_slice()).collect();
            process(&mut states[..ch], &band, &in_refs, &mut out_refs, n);

            nonzero += outs.iter().flatten().filter(|v| **v != 0.0).count();
            let want_out = case["out"].as_array().unwrap();
            for (c, expected) in want_out.iter().enumerate() {
                for (i, (g, e)) in outs[c].iter().zip(f32s(expected)).enumerate() {
                    let err = (g - e).abs() / e.abs().max(1e-12);
                    assert!(
                        g.to_bits() == e.to_bits() || err <= 1e-6,
                        "case {idx} ch {c} sample {i}: got {g}, emulated {e}"
                    );
                }
            }
            let after = case["state_after"].as_array().unwrap();
            for (c, rec) in after.iter().enumerate().take(ch) {
                assert_state_matches(idx, c, &states[c], rec);
            }
        }
        assert!(nonzero > 50_000, "outputs must be non-trivial, got {nonzero} nonzero samples");
    }

    fn f32s(v: &serde_json::Value) -> Vec<f32> {
        v.as_array()
            .expect("array of f32 bit patterns")
            .iter()
            .map(|b| f32::from_bits(b.as_u64().expect("u32") as u32))
            .collect()
    }

    /// Record from the fixture's `[fs, mode, +0x10, ..., +0x70]` bit patterns.
    fn record_from_bits(rec: &serde_json::Value) -> SoloChannel {
        let v: Vec<u64> = rec
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_u64().unwrap())
            .collect();
        let d = |k: usize| f64_of(v[k]);
        SoloChannel {
            fs: d(0),
            mode: v[1] as u32 as i32,
            target_r: d(2),
            target_k: d(3),
            cur_r: d(4),
            cur_k: d(5),
            tau_ms: d(6),
            rate: d(7),
            g: d(8),
            k: d(9),
            a1: d(10),
            a2: d(11),
            a3: d(12),
            ic1: d(13),
            ic2: d(14),
        }
    }

    fn assert_state_matches(idx: usize, c: usize, got: &SoloChannel, rec: &serde_json::Value) {
        let want = record_from_bits(rec);
        let fields = [
            ("fs", got.fs, want.fs),
            ("target_r", got.target_r, want.target_r),
            ("target_k", got.target_k, want.target_k),
            ("cur_r", got.cur_r, want.cur_r),
            ("cur_k", got.cur_k, want.cur_k),
            ("g", got.g, want.g),
            ("k", got.k, want.k),
            ("a1", got.a1, want.a1),
            ("a2", got.a2, want.a2),
            ("a3", got.a3, want.a3),
            ("ic1", got.ic1, want.ic1),
            ("ic2", got.ic2, want.ic2),
        ];
        for (name, g, w) in fields {
            let err = (g - w).abs() / w.abs().max(1e-12);
            assert!(
                g.to_bits() == w.to_bits() || err <= 1e-6,
                "case {idx} ch {c} {name}: got {g:e}, emulated {w:e}"
            );
        }
        assert_eq!(got.mode, want.mode, "case {idx} ch {c} mode");
    }

    #[test]
    fn copy_branch_and_mode_table_match_the_binary() {
        // Tilt copies the input; no state is written.
        let mut states = [SoloChannel::default(); 2];
        let before = states;
        let input = [1.0f32, -2.0, 0.5];
        let mut out = [0.0f32; 3];
        let band = SoloBand { freq: 1000.0, half_bw_oct: 1.0, shape: 5 };
        process(&mut states, &band, &[&input], &mut [&mut out], 3);
        assert_eq!(out, input);
        assert_eq!(states, before);

        // Mode table: {2,3} -> 2, {1,7} -> 0, {0,6} -> 1, {4} -> 3.
        for (shape, mode) in [(2, 2), (3, 2), (1, 0), (7, 0), (0, 1), (6, 1), (4, 3)] {
            let mut s = [SoloChannel { fs: 48000.0, ..Default::default() }];
            store_coefficients(&mut s, &SoloBand { freq: 1000.0, half_bw_oct: 1.0, shape });
            assert_eq!(s[0].mode, mode, "shape {shape}");
        }
        // k target: 1.0 for low-pass and high-pass, max(hb, 0) for band-pass and notch.
        let mut s = [SoloChannel { fs: 48000.0, ..Default::default() }];
        store_coefficients(&mut s, &SoloBand { freq: 1000.0, half_bw_oct: -0.5, shape: 1 });
        assert_eq!(s[0].target_k, 1.0);
        store_coefficients(&mut s, &SoloBand { freq: 1000.0, half_bw_oct: -0.5, shape: 2 });
        assert_eq!(s[0].target_k, 0.0);
    }
}
