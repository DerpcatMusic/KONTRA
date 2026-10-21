//! Band-energy state, band filterbank and band-gain smoothing of soothe3's gain body.
//!
//! Each function is a translation of a callee of the gain body `0x8e04c0`:
//!
//! | function | address | used by |
//! |---|---|---|
//! | [`FilterBank::analyze`] | `0x8ef8b0` -> `0x8f7630` | both analyzers: band energies `E[c][b] = dot(D[c][s_b..], W_b[s_b..])` |
//! | [`FilterBank::synthesize`] | `0x8f03f0` -> `0x8f78e0` | both analyzers: per-bin gain `G[c][k] = sum_b L[c][b] * S_b[k]` |
//! | [`attack_release`] | `0x8ef9c0` | 4096-point analyzer only (`analyzer[+0x58] = 1`, mode-1 configuration) |
//! | [`smooth`] | `0x8f0210` | 4096-point analyzer only, on the band gains |
//! | [`attack_coefficient`] | inline `0x8e2eac..0x8e2f50` | 4096-point analyzer only, feeds the two calls above |
//! | [`max_over_channels`] | `0x89ad70` | quality-1 configuration |
//!
//! Views. As in `reference::gain`, a view is a slice of channel vectors. The filterbank object holds two
//! band tables: analysis at `+0x1a0` and synthesis at `+0x340`. Each table has 16 records of `start`
//! (i32 at `+8b`), `len` (i32 at `+0x80+8b`) and a weight pointer (u64 at `+0x100+8b`). The binary's
//! count is `+0x190`, or `+0x180` when that is zero (16 in every capture). Only `W_b[start..start+len]`
//! is read, so [`Band::weights`] holds that slice. The fields `+0x188` and `+0x198` only feed headers
//! that the arithmetic does not read.
//!
//! Evidence: `tools/smooth-core/research/re-tools/emulate/gain_fx_band_state.py` runs the callees in situ
//! (base, quality 1 and mode 1 at 48 kHz; the attack coefficient also at poked rate/hop pairs) and on
//! random views. The fixture is `fixtures/gain_band_state.json`. Element-wise steps are exact f32
//! operations in the binary's order. The dot products are IPP's `DotProd`, modelled by
//! [`ipp::dot_prod`]: not bit-exact (worst relative error 1.2e-7 on the captured analysis cases).

use super::ipp;

/// `.rdata` `0x209d0c8`: smoothing coefficient of `0x8f0210` (f32 `0x3dcccccd`).
pub const SMOOTH_ALPHA: f32 = 0.1;

/// `.rdata` `0x209da84`: start value of the channel maximum in `0x89ad70` (f32 `0xff7fffff`, -FLT_MAX).
pub const MAX_INIT: f32 = -f32::MAX;

/// One band of a filterbank table: bins `start..start + len`, with `weights = W_b[start..start + len]`.
#[derive(Clone, Debug, PartialEq)]
pub struct Band {
    pub start: usize,
    pub len: usize,
    pub weights: Vec<f32>,
}

/// The two band tables of one analyzer's filterbank (`analyzer + 0xf28`).
///
/// Analysis (`+0x1a0`) and synthesis (`+0x340`) share the band layout but not the weights: the
/// analysis weights sum to about 1 per band (energy averaging), the synthesis weights are per-bin
/// gains.
#[derive(Clone, Debug, PartialEq)]
pub struct FilterBank {
    pub analysis: Vec<Band>,
    pub synthesis: Vec<Band>,
}

impl FilterBank {
    /// `0x8ef8b0` -> `0x8f7630`: for every band `b` of the output views, `energies[c][b]` is the
    /// dot product of the spectrum `spectrum[c]` over the band's bins with the band's analysis weights.
    /// The binary overwrites each output element (no accumulation).
    pub fn analyze(&self, spectrum: &[&[f32]], energies: &mut [&mut [f32]]) {
        assert!(
            spectrum.len() >= energies.len(),
            "spectrum has fewer channels than the energies"
        );
        for b in 0..energies.first().map_or(0, |e| e.len()) {
            let band = &self.analysis[b];
            let bins = band.start..band.start + band.len;
            for (c, e) in energies.iter_mut().enumerate() {
                // both operands start `band.start` floats into 16-byte aligned buffers
                e[b] = ipp::dot_prod_phase(&spectrum[c][bins.clone()], &band.weights, band.start % 4);
            }
        }
    }

    /// `0x8f03f0` -> `0x8f78e0`: zeroes every output channel, then for every band `b` of the levels
    /// adds `levels[c][b] * W_b` to the bins of that band (`AddProductC`, bins in band order).
    pub fn synthesize(&self, levels: &[&[f32]], out: &mut [&mut [f32]]) {
        for g in out.iter_mut() {
            g.fill(0.0);
        }
        for b in 0..levels.first().map_or(0, |l| l.len()) {
            let band = &self.synthesis[b];
            let bins = band.start..band.start + band.len;
            for (c, g) in out.iter_mut().enumerate() {
                ipp::add_product_c(&band.weights, levels[c][b], &mut g[bins.clone()]);
            }
        }
    }
}

/// `0x8ef9c0`: attack/release update of the band-energy state, in place.
///
/// The band count is `energies[c].len()`, which equals the state view's `+0x84` field (16) in every
/// captured call. Channels of the state view are 32 floats apart; only the first `bands` entries of each
/// channel are read and written, the rest is padding that the binary leaves unchanged.
///
/// Per channel and band, with `d = E - S` (`0x8efc88`, IPP `Sub`):
/// `S += max(d, 0) * attack` (`0x8f0038`) then `S += min(d, 0) * release` (`0x8f0188`).
/// The body passes the same coefficient for both (`xmm2 = xmm3`, see [`attack_coefficient`]).
///
/// Channel pairing: equal counts pair channel by channel, a single channel on either side is
/// broadcast, any other combination does nothing (the binary's view rule).
pub fn attack_release(state: &mut [&mut [f32]], energies: &[&[f32]], attack: f32, release: f32) {
    let channels = match (state.len(), energies.len()) {
        (a, b) if a == b => a,
        (1, b) => b,
        (a, 1) => a,
        _ => 0,
    };
    for c in 0..channels {
        let s = &mut state[pick(c, state.len())];
        let e = energies[pick(c, energies.len())];
        assert!(
            s.len() >= e.len(),
            "state holds fewer bands than the energies"
        );
        for (sk, &ek) in s.iter_mut().zip(e) {
            let d = ek - *sk; // Sub: dst = s2 - s1 with s1 = S, s2 = E
            let up = if d < 0.0 { 0.0 } else { d }; // Threshold_I, relOp less, level 0
            let down = if d > 0.0 { 0.0 } else { d }; // Threshold, relOp greater, level 0
            *sk += up * attack; // AddProductC(src = up, val = attack, dst = S)
            *sk += down * release; // AddProductC(src = down, val = release, dst = S)
        }
    }
}

/// `0x8f0210`: smoothing of each channel in place. A forward one-pole pass (`y += (x - y) * a`,
/// starting from `y = x[0]`), then a backward pass over `x[n-2]..x[0]` that continues from the
/// forward result `y[n-1]` (the binary does not reset the state). `a` is [`SMOOTH_ALPHA`].
pub fn smooth(view: &mut [&mut [f32]]) {
    for x in view.iter_mut() {
        let n = x.len();
        if n == 0 {
            continue;
        }
        let mut y = x[0];
        for k in 0..n {
            y += (x[k] - y) * SMOOTH_ALPHA;
            x[k] = y;
        }
        for k in (0..n - 1).rev() {
            y += (x[k] - y) * SMOOTH_ALPHA;
            x[k] = y;
        }
    }
}

/// `0x89ad70`: `max` over the channels of the per-channel maximum (IPP `Max` per channel), starting
/// from [`MAX_INIT`]. Returns `MAX_INIT` for an empty view.
pub fn max_over_channels(view: &[&[f32]]) -> f32 {
    let mut m = MAX_INIT;
    for ch in view {
        let t = ipp::max(ch);
        m = if t > m { t } else { m };
    }
    m
}

/// `0x8e2eac..0x8e2f50`: attack coefficient `c = 1 - 0.1^(1 / t)` with
/// `t = (rate / hop) * 0.001f * 500.0f` (f32 operations, left to right), where `rate` is
/// `analyzer[+0x20]` and `hop` is `analyzer[+0x60]`.
///
/// The `pow` is evaluated in f64 (`pow(0.1, 1/t)`, base `0x209d348` = `(double)0.1f`), `1 - p` in
/// f64, and the result is rounded to f32 (`cvtpd2ps`). The constants are `0x209d03c` = 0.001f and
/// `0x209d878` = 500.0f. A negative `hop` takes the unsigned conversion path (`0x8e2ebf`).
pub fn attack_coefficient(rate: f32, hop: i64) -> f32 {
    let hop_f = if hop >= 0 {
        hop as f32
    } else {
        let u = hop as u64;
        let half = (((u >> 1) | (u & 1)) as i64) as f32;
        half + half
    };
    let t = rate / hop_f;
    let t = t * 0.001_f32;
    let t = t * 500.0_f32;
    let p = POW_BASE.powf(1.0_f64 / f64::from(t));
    (1.0_f64 - p) as f32
}

/// `.rdata` `0x209d348`: the `pow` base, stored as the f64 `0.10000000149011612`.
const POW_BASE: f64 = 0.1_f32 as f64;

/// Channel index for a view with `n` channels: a single-channel view is broadcast.
fn pick(c: usize, n: usize) -> usize {
    if n == 1 { 0 } else { c }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    const FIXTURE: &str = include_str!("../fixtures/gain_band_state.json");

    /// Relative tolerance for IPP `DotProd` (the only non-element-wise step).
    const DOT_TOL: f32 = 1e-6;

    fn bits_vec(v: &Value) -> Vec<f32> {
        v.as_array()
            .expect("array of f32 bit patterns")
            .iter()
            .map(|b| f32::from_bits(b.as_u64().expect("u32") as u32))
            .collect()
    }

    fn channels(v: &Value) -> Vec<Vec<f32>> {
        v.as_array()
            .expect("channel array")
            .iter()
            .map(bits_vec)
            .collect()
    }

    fn bands(v: &Value) -> Vec<Band> {
        v.as_array()
            .expect("band array")
            .iter()
            .map(|b| Band {
                start: b["start"].as_u64().unwrap() as usize,
                len: b["len"].as_u64().unwrap() as usize,
                weights: bits_vec(&b["weights"]),
            })
            .collect()
    }

    fn filterbank(doc: &Value, analyzer: &str) -> FilterBank {
        let t = &doc["tables"][analyzer];
        FilterBank {
            analysis: bands(&t["analysis"]),
            synthesis: bands(&t["synthesis"]),
        }
    }

    fn assert_bits(got: &[f32], want: &[f32], what: &str) {
        assert_eq!(got.len(), want.len(), "{what}: length");
        for (k, (g, w)) in got.iter().zip(want).enumerate() {
            assert_eq!(
                g.to_bits(),
                w.to_bits(),
                "{what}[{k}]: got {g:e}, emulated {w:e}"
            );
        }
    }

    fn assert_close(got: &[f32], want: &[f32], what: &str) -> f32 {
        assert_eq!(got.len(), want.len(), "{what}: length");
        let mut worst = 0.0f32;
        for (k, (g, w)) in got.iter().zip(want).enumerate() {
            let tol = DOT_TOL * w.abs().max(1.0);
            assert!(
                (g - w).abs() <= tol,
                "{what}[{k}]: got {g:e}, emulated {w:e}"
            );
            worst = worst.max((g - w).abs() / w.abs().max(1.0));
        }
        worst
    }

    #[test]
    fn analysis_matches_emulated_callee() {
        let doc: Value = serde_json::from_str(FIXTURE).unwrap();
        let mut worst = 0.0f32;
        let mut n = 0;
        for case in doc["cases"]["analysis"].as_array().unwrap() {
            let fb = filterbank(&doc, case["analyzer"].as_str().unwrap());
            let spectrum = channels(&case["spectrum"]);
            let want = channels(&case["energies"]);
            let spec: Vec<&[f32]> = spectrum.iter().map(|v| v.as_slice()).collect();
            let mut got: Vec<Vec<f32>> = want.iter().map(|v| vec![0.0; v.len()]).collect();
            let mut out: Vec<&mut [f32]> = got.iter_mut().map(|v| v.as_mut_slice()).collect();
            fb.analyze(&spec, &mut out);
            for (g, w) in got.iter().zip(&want) {
                worst = worst.max(assert_close(g, w, "analysis energies"));
                assert_bits(g, w, "analysis energies");
            }
            n += 1;
        }
        assert!(n >= 4, "fixture should carry analysis cases");
        eprintln!("analysis: {n} cases, worst relative error {worst:e}");
    }

    #[test]
    fn synthesis_matches_emulated_callee() {
        let doc: Value = serde_json::from_str(FIXTURE).unwrap();
        let mut n = 0;
        for case in doc["cases"]["synthesis"].as_array().unwrap() {
            let fb = filterbank(&doc, case["analyzer"].as_str().unwrap());
            let levels = channels(&case["levels"]);
            let want = channels(&case["out"]);
            let lv: Vec<&[f32]> = levels.iter().map(|v| v.as_slice()).collect();
            let mut got: Vec<Vec<f32>> = want.iter().map(|v| vec![0.0; v.len()]).collect();
            let mut out: Vec<&mut [f32]> = got.iter_mut().map(|v| v.as_mut_slice()).collect();
            fb.synthesize(&lv, &mut out);
            for (g, w) in got.iter().zip(&want) {
                assert_bits(g, w, "synthesis gains");
            }
            n += 1;
        }
        assert!(n >= 4, "fixture should carry synthesis cases");
    }

    #[test]
    fn attack_release_matches_emulated_callee() {
        let doc: Value = serde_json::from_str(FIXTURE).unwrap();
        let mut n = 0;
        for case in doc["cases"]["attack"].as_array().unwrap() {
            let a = f32::from_bits(case["attack"].as_u64().unwrap() as u32);
            let r = f32::from_bits(case["release"].as_u64().unwrap() as u32);
            let mut state = channels(&case["state_in"]);
            let energies = channels(&case["energies"]);
            let want = channels(&case["state_out"]);
            let en: Vec<&[f32]> = energies.iter().map(|v| v.as_slice()).collect();
            // The binary's state channels are 32 floats apart; the band loop covers the first 16
            // (the energies' length) and must leave the padding unchanged.
            let bands = en[0].len();
            let mut st: Vec<&mut [f32]> = state.iter_mut().map(|v| &mut v[..bands]).collect();
            attack_release(&mut st, &en, a, r);
            for (g, w) in state.iter().zip(&want) {
                assert_bits(g, w, "attack/release state (bands and padding)");
            }
            n += 1;
        }
        assert!(n >= 4, "fixture should carry attack/release cases");
    }

    #[test]
    fn smooth_matches_emulated_callee() {
        let doc: Value = serde_json::from_str(FIXTURE).unwrap();
        let mut n = 0;
        for case in doc["cases"]["smooth"].as_array().unwrap() {
            let mut data = channels(&case["input"]);
            let want = channels(&case["output"]);
            let mut view: Vec<&mut [f32]> = data.iter_mut().map(|v| v.as_mut_slice()).collect();
            smooth(&mut view);
            for (g, w) in data.iter().zip(&want) {
                assert_bits(g, w, "smoothed gains");
            }
            n += 1;
        }
        assert!(n >= 4, "fixture should carry smoother cases");
    }

    #[test]
    fn channel_max_matches_emulated_callee() {
        let doc: Value = serde_json::from_str(FIXTURE).unwrap();
        let mut n = 0;
        for case in doc["cases"]["max"].as_array().unwrap() {
            let data = channels(&case["input"]);
            let view: Vec<&[f32]> = data.iter().map(|v| v.as_slice()).collect();
            let want = f32::from_bits(case["output"].as_u64().unwrap() as u32);
            assert_eq!(max_over_channels(&view).to_bits(), want.to_bits());
            n += 1;
        }
        assert!(n >= 5);
    }

    #[test]
    fn attack_coefficient_matches_in_situ_body() {
        let doc: Value = serde_json::from_str(FIXTURE).unwrap();
        let mut n = 0;
        for case in doc["cases"]["coef"].as_array().unwrap() {
            let rate = f32::from_bits(case["rate"].as_u64().unwrap() as u32);
            let hop = case["hop"].as_i64().unwrap();
            let want = f32::from_bits(case["coef"].as_u64().unwrap() as u32);
            assert_eq!(
                attack_coefficient(rate, hop).to_bits(),
                want.to_bits(),
                "rate {rate} hop {hop}"
            );
            n += 1;
        }
        assert!(
            n >= 3,
            "fixture should carry coefficient cases at several rates"
        );
    }
}
