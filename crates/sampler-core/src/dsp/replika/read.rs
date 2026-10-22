//! Replika read tap of the mode-1 head (`141ab87d0`, with the section update `14199dcc0`): one of the two kernels
//! at `H+0x1a8` and `H+0x278`. It returns the ring sample at `pos - x` by linear interpolation. Before that it runs
//! the section over the window of taps the read has just passed (`n` taps ending at `I + 2`, I the integer part of
//! the distance), writing each lowpass back into the ring.
//!
//! Native (`docs/re/engine/disasm/replika-3b-listings.txt`, decompile `0141ab0000.c`): the section frequency is
//! `clamp(1 / |x_prev - x + offset|, 0.2, 1)`; `14199dcc0` sets the section to the tangent series of `f pi rate`
//! with the kernel's damping. The ring is the f32 array behind the handle at `K+0xc0`. Not ported: the driver
//! `141ab8c60` (it writes `pos` and `x`) and the deposit `141accfb0`.
//!
//! Vectors: `docs/re/engine/vectors/replika-native-tap-read.json` (kemu, 3 seeds, 240 calls each).

use super::design::tan_poly;
use crate::dsp::tpt_svf::{Section, State};

/// `1447149d4`: pi as f32.
const PI_F32: f32 = f32::from_bits(0x4049_0fdb);
/// `144f38948`: the lower clamp of the section frequency.
const FREQ_LO: f64 = 0.2;

/// `14199dcc0`: `sec` retuned to frequency `f` with the rate factor `rate`: g = tan(f pi rate), damping kept.
pub(super) fn retune(sec: Section, f: f32, rate: f32) -> Section {
    Section::new(tan_poly(f * PI_F32 * rate), sec.parts().3)
}

/// One read tap. Field comments give the offsets in the kernel object.
#[derive(Clone, Copy, Debug)]
pub struct Tap {
    /// `+0x00`: read position (written by the driver).
    pub pos: f64,
    /// `+0x18`: ring mask (length - 1).
    pub mask: i32,
    /// `+0x28`: phase, the last index plus the fraction.
    pub phase: f64,
    /// `+0x30`: the last index.
    pub idx: i32,
    /// `+0x38`: the last x.
    pub x_prev: f64,
    /// `+0x48`: offset.
    pub offset: f64,
    /// `+0x50..+0x60`: the section (`sec.parts()` gives c2, c3, c4 and the damping k at `+0x7c`).
    pub sec: Section,
    /// `+0x50` s0, `+0x54` s1.
    pub state: State,
    /// `+0x74`: section frequency. `+0x80`: rate factor (preset).
    pub freq: f32,
    pub rate: f32,
    /// `+0x08`: the driver's index (`trunc(pos + rate) & mask`), read by the deposit `141accfb0`.
    pub wr: i32,
    /// `+0x10`: the driver's fraction, read by the deposit as the sub-sample offset.
    pub frac: f64,
    /// `+0x1c`: taps written per frame (the index step), read by the deposit as its count.
    pub delta: i32,
    /// `+0x20`, `+0x40`: the driver's index and fraction of the previous frame.
    pub prev_wr: i32,
    pub prev_frac: f64,
    /// `+0x90`, `+0x98`, `+0xa0`: the last three rates (the deposit's `a` and `b`).
    pub rates: [f64; 3],
    /// `+0x64` bp, `+0x68` lp, `+0x6c` d, `+0x70` v of the last tap.
    pub bp: f32,
    pub lp: f32,
    pub d: f32,
    pub v: f32,
}

impl Tap {
    /// A tap with the preset fields (the section coefficients are set by the first call).
    pub fn new(
        pos: f64,
        mask: i32,
        phase: f64,
        idx: i32,
        x_prev: f64,
        offset: f64,
        damping: f32,
        rate: f32,
    ) -> Self {
        Self {
            pos,
            mask,
            phase,
            idx,
            x_prev,
            offset,
            sec: Section::new(0., damping),
            state: State::default(),
            freq: 0.,
            rate,
            bp: 0.,
            lp: 0.,
            d: 0.,
            v: 0.,
            wr: 0,
            frac: 0.,
            delta: 0,
            prev_wr: 0,
            prev_frac: 0.,
            rates: [0.; 3],
        }
    }

    /// `141aba040` with flag 0 (and `141abaab0`, which it calls): the driver and kernel fields to zero (`K+0x00`,
    /// `+0x08..+0x40`, `+0x48`, `+0x50`, `+0x54`) and the rates `+0x90..+0xa0` to 1. The section, the freq block and
    /// the outputs keep their values. Not here: the deposit's history `+0x84..+0x8c` (the `Deposit` object).
    pub fn reset(&mut self) {
        self.pos = 0.;
        self.wr = 0;
        self.frac = 0.;
        self.delta = 0;
        self.prev_wr = 0;
        self.phase = 0.;
        self.idx = 0;
        self.x_prev = 0.;
        self.prev_frac = 0.;
        self.offset = 0.;
        self.state = State::default();
        self.reset_rates();
    }

    /// `141abaab0`: the rates `+0x90..+0xa0` to 1 (its history `+0x84` and `+0x8c` is not modelled).
    pub fn reset_rates(&mut self) {
        self.rates = [1.; 3];
    }

    /// `141aba040(K, flag)`: `reset()`, and with `flag` the ring clear `1407e6d80` of the ring that `K+0xc0` names
    /// (`ring`: `**(H+0x150)` for L, `**(H+0x188)` for R, see `Head::set_mode`). The clear writes 0 to every element:
    /// `ring.fill(0.)`.
    pub fn reset_flagged(&mut self, flag: bool, ring: &mut [f32]) {
        self.reset();
        if flag {
            ring.fill(0.);
        }
    }

    /// `14199dcc0`: the section for frequency `f`, g = tan(f pi rate) with the kernel's damping.
    fn set_freq(&mut self, f: f32) {
        self.freq = f;
        self.sec = retune(self.sec, f, self.rate);
    }

    /// `141ab87d0`: the interpolated ring sample at `pos - x`. `ring` is the data of length `mask + 1`.
    pub fn read(&mut self, ring: &mut [f32], x: f64) -> f32 {
        let mask = self.mask;
        let mut dist = self.pos - x;
        if dist < 0.0 {
            dist += f64::from(mask + 1);
        }
        let i = dist as i32;
        let frac = (dist - f64::from(i)) as f32;

        let a = (self.x_prev - x) + self.offset;
        let freq = (1.0 / a.abs()).max(FREQ_LO).min(1.0) as f32;
        let n = ((a + self.phase) as i32).wrapping_sub(self.idx).min(15);
        self.set_freq(freq);

        if n >= 1 {
            let start = i + (3 - n);
            for j in 0..n {
                let slot = ((start + j) & mask) as usize;
                let v = ring[slot];
                self.v = v;
                self.d = v - self.state.s1;
                let (bp, lp) = self.sec.tick(&mut self.state, v);
                self.bp = bp;
                self.lp = lp;
                ring[slot] = lp;
            }
        }

        let idx = i & mask;
        self.idx = idx;
        self.x_prev = x;
        self.phase = f64::from(idx) + f64::from(frac);
        let v0 = ring[idx as usize];
        let v1 = ring[((idx + 1) & mask) as usize];
        (v1 - v0) * frac + v0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    const TOLERANCE: f32 = 1e-5;

    fn vector() -> Value {
        let path = format!(
            "{}/../../docs/re/engine/vectors/replika-native-tap-read.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(path).expect("vector file"))
            .expect("vector json")
    }

    fn words(v: &Value) -> Vec<u32> {
        v.as_array()
            .expect("words")
            .iter()
            .map(|w| w.as_u64().expect("u32") as u32)
            .collect()
    }

    fn f64_at(w: &[u32], off: usize) -> f64 {
        f64::from_bits(u64::from(w[off / 4 + 1]) << 32 | u64::from(w[off / 4]))
    }

    fn close(label: &str, got: f32, want: f32) {
        assert!(
            (got - want).abs() <= TOLERANCE,
            "{label}: got {got} want {want}"
        );
    }

    #[test]
    fn tap_read_matches_native_vectors() {
        let doc = vector();
        for case in doc["cases"].as_array().expect("cases") {
            let init = words(&case["init_words"]);
            let mask = case["mask"].as_i64().expect("mask") as i32;
            let mut tap = Tap::new(
                f64_at(&init, 0x00),
                mask,
                f64_at(&init, 0x28),
                init[0x30 / 4] as i32,
                f64_at(&init, 0x38),
                f64_at(&init, 0x48),
                f32::from_bits(init[0x7c / 4]),
                f32::from_bits(init[0x80 / 4]),
            );
            let mut ring: Vec<f32> = words(&case["ring0"])
                .into_iter()
                .map(f32::from_bits)
                .collect();
            let xs = case["xs"].as_array().expect("xs");
            let rets = case["rets"].as_array().expect("rets");
            for (k, (x, want)) in xs.iter().zip(rets).enumerate() {
                let got = tap.read(&mut ring, x.as_f64().expect("x"));
                close(
                    &format!("seed {} call {k} return", case["seed"]),
                    got,
                    want.as_f64().unwrap() as f32,
                );
            }

            let end = &case["end"];
            let end_words = words(&case["end_words"]);
            let (c2, c3, c4, k) = tap.sec.parts();
            close("c2", c2, end["c2"].as_f64().unwrap() as f32);
            close("c3", c3, end["c3"].as_f64().unwrap() as f32);
            close("c4", c4, end["c4"].as_f64().unwrap() as f32);
            close("damping", k, f32::from_bits(end_words[0x7c / 4]));
            close("s0", tap.state.s0, end["s0"].as_f64().unwrap() as f32);
            close("s1", tap.state.s1, end["s1"].as_f64().unwrap() as f32);
            close("bp", tap.bp, end["bp"].as_f64().unwrap() as f32);
            close("lp", tap.lp, end["lp"].as_f64().unwrap() as f32);
            close("d", tap.d, end["d"].as_f64().unwrap() as f32);
            close("v", tap.v, end["v"].as_f64().unwrap() as f32);
            close("freq", tap.freq, end["f"].as_f64().unwrap() as f32);
            assert_eq!(tap.idx as i64, end["idx"].as_i64().unwrap(), "idx");
            for (i, want) in words(&case["ring1"]).into_iter().enumerate() {
                close(&format!("ring[{i}]"), ring[i], f32::from_bits(want));
            }
        }
    }

    #[test]
    fn kernel_reset_matches_native_vectors() {
        let path = format!(
            "{}/../../docs/re/engine/vectors/replika-native-kernel-reset.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let doc: Value = serde_json::from_str(&std::fs::read_to_string(path).expect("vector file"))
            .expect("vector json");
        let num = |v: &Value, k: &str| v[k].as_f64().unwrap_or_else(|| panic!("field {k}"));
        for case in doc["cases"].as_array().expect("cases") {
            let init = &case["init"];
            let end = &case["end"];
            let mut t = Tap::new(
                num(init, "pos"),
                num(init, "wr") as i32,
                num(init, "phase"),
                num(init, "idx") as i32,
                num(init, "xprev"),
                num(init, "offset"),
                num(init, "damp") as f32,
                num(init, "rate") as f32,
            );
            t.wr = num(init, "wr") as i32;
            t.frac = num(init, "frac");
            t.delta = num(init, "delta") as i32;
            t.prev_wr = num(init, "pwr") as i32;
            t.prev_frac = num(init, "pfrac");
            t.rates = [num(init, "rate0"), num(init, "rate1"), num(init, "rate2")];
            t.state = State {
                s0: num(init, "s0") as f32,
                s1: num(init, "s1") as f32,
            };
            t.reset();
            let seed = &case["seed"];
            assert_eq!(t.pos, num(end, "pos"), "seed {seed} pos");
            assert_eq!(t.wr as i64, num(end, "wr") as i64, "seed {seed} wr");
            assert_eq!(t.frac, num(end, "frac"), "seed {seed} frac");
            assert_eq!(
                t.delta as i64,
                num(end, "delta") as i64,
                "seed {seed} delta"
            );
            assert_eq!(t.prev_wr as i64, num(end, "pwr") as i64, "seed {seed} pwr");
            assert_eq!(t.phase, num(end, "phase"), "seed {seed} phase");
            assert_eq!(t.idx as i64, num(end, "idx") as i64, "seed {seed} idx");
            assert_eq!(t.x_prev, num(end, "xprev"), "seed {seed} xprev");
            assert_eq!(t.prev_frac, num(end, "pfrac"), "seed {seed} pfrac");
            assert_eq!(t.offset, num(end, "offset"), "seed {seed} offset");
            assert_eq!(f64::from(t.state.s0), num(end, "s0"), "seed {seed} s0");
            assert_eq!(f64::from(t.state.s1), num(end, "s1"), "seed {seed} s1");
            assert_eq!(t.rates, [1.0; 3], "seed {seed} rates");
            // Fields the reset does not touch.
            assert_eq!(
                f64::from(t.sec.parts().3),
                num(end, "damp"),
                "seed {seed} damp"
            );
            assert_eq!(f64::from(t.rate), num(end, "rate"), "seed {seed} rate");
        }
    }
}
