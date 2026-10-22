//! Kontakt Gater per-frame kernel, ported from the native code (`FUN_141ae2f60`, Kontakt 8). Field
//! names carry the native offsets from the voice struct (the block core's `+0x200` object), so
//! each line can be checked against the decompile.
//!
//! Not ported here: the clock and segment schedule (`FUN_141a9d870`, `FUN_141acb6f0`,
//! `FUN_141a98d20`), which supply the per-frame level [`GaterKernel::frame`] takes, and the
//! parameter setters. Those stay separate steps; see `docs/re/engine/k-mangling.md`.
//!
//! Operation order follows the decompile exactly, so the result is bit-identical to the native
//! kernel for the same state (see `gater_kemu_vectors.json`).

/// 0x14470bad4: unity level, ramp ceiling (1.0).
const ONE: f32 = f32::from_bits(0x3f80_0000);
/// 0x144714994: the 2.0 of the flag-8 window.
const TWO: f32 = f32::from_bits(0x4000_0000);
/// 0x144f385a4: denormal guard added to the filter recursion (1e-18).
const TINY: f32 = f32::from_bits(0x219a_92ef);
/// 0x144725080: filter tap factor (0.6).
const TAP: f32 = f32::from_bits(0x3f19_999a);
/// 0x147698870: added to the shaping sum before the division (0.36).
const SHAPE_OFFSET: f32 = f32::from_bits(0x3eb8_51ec);
/// 0x1479ab1bc: x^5 coefficient of the shaping curve (0.13333).
const SHAPE_5: f32 = f32::from_bits(0x3e08_8872);
/// 0x1479ab1d4: x^3 coefficient of the shaping curve (0.33333).
const SHAPE_3: f32 = f32::from_bits(0x3eaa_aa9f);
/// 0x1479af27c: int32 noise to [-1, 1) (2^-31, as the binary stores it).
const NOISE_SCALE: f32 = f32::from_bits(0x3000_009d);
/// Delay ring length (`+0x138`), reads and writes clamp to its last entry.
pub const RING: usize = 4000;

/// Gater kernel state. `ramp`, `slope` and `offset` index the five flag bits 1, 2, 4, 8, 16, at
/// native `+0xb8/+0xb4/+0xb0`, `+0xc8/+0xc4/+0xc0`, `+0xd8/+0xd4/+0xd0`, `+0xe8/+0xe4/+0xe0` and
/// `+0x100/+0xfc/+0xf8`.
#[derive(Clone, Debug)]
pub struct GaterKernel {
    /// `+0x6c`: frame counter. A control tick runs when it passes `ctrl` (`+0x70`).
    pub frame: i32,
    /// `+0x70`: control period minus one, in frames.
    pub ctrl: i32,
    /// `+0x68`: mode. `< 1` selects the first shaping of the flag-8 window, otherwise the second.
    pub mode: i32,
    /// `+0x74`: pending ramp bits (1, 2, 4, 8, 16). A bit clears when its ramp reaches unity.
    pub flags: u32,
    /// `+0x78`: ramp step added to each per-bit ramp at a control tick.
    pub step: f32,
    pub ramp: [f32; 5],
    pub slope: [f32; 5],
    pub offset: [f32; 5],
    /// `+0x110`: scale of the flag-1 level before the shaping curve.
    pub scale: f32,
    /// `+0x108`: 32-bit LCG state for the noise source.
    pub seed: i32,
    /// `+0x130` write position and `+0x134` read delay in the ring.
    pub write: u32,
    pub delay: i32,
    /// `+0x138`: delay ring.
    pub ring: Box<[f32; RING]>,
    /// `+0x10c`, `+0x11c`: filter recursion state.
    pub z1: f32,
    pub z2: f32,
    /// `+0x114`, `+0x118`, `+0x120`: shaping level, its offset sum and the normaliser.
    pub shape: f32,
    pub shape_sum: f32,
    pub normaliser: f32,
    /// `+0x124`, `+0x128`: flag-2 and flag-4 levels (the output gains of the shaped stage).
    pub gain2: f32,
    pub gain4: f32,
    /// `+0xf0` and `+0x12c`: dry and wet levels of the flag-8 window.
    pub dry: f32,
    pub wet: f32,
    /// `+0x104`: mix level of the flag-16 ramp (applied to both channels).
    pub mix: f32,
    /// Per-bit levels written at each control tick (`+0xac`, `+0xbc`, `+0xcc`, `+0xdc`, `+0xec`,
    /// `+0xf4`). Kept for the state image; the audio path reads only the fields above.
    pub level: [f32; 6],
}

impl Default for GaterKernel {
    fn default() -> Self {
        Self {
            frame: 0,
            ctrl: 0,
            mode: 0,
            flags: 0,
            step: 0.,
            ramp: [0.; 5],
            slope: [0.; 5],
            offset: [0.; 5],
            scale: 0.,
            seed: 0,
            write: 0,
            delay: 0,
            ring: Box::new([0.; RING]),
            z1: 0.,
            z2: 0.,
            shape: 0.,
            shape_sum: 0.,
            normaliser: 0.,
            gain2: 0.,
            gain4: 0.,
            dry: 0.,
            wet: 0.,
            mix: 0.,
            level: [0.; 6],
        }
    }
}

impl GaterKernel {
    /// Runs one frame: `input` is (L, R) at `+0xc` and `+0x58`, `gain` is the clock level at
    /// `+0x50`. Returns (L, R) from `+0x54` and `+0x5c`.
    pub fn frame(&mut self, input: [f32; 2], gain: f32) -> [f32; 2] {
        let [in_l, in_r] = input;
        let write = self.write;
        // The read index is taken before the ring write, from the write position and delay.
        let mut read = write.wrapping_sub(self.delay as u32);
        if (read as i32) < 0 {
            read = read.wrapping_add(RING as u32);
        }
        let read = (read.min(RING as u32 - 1)) as usize;

        // LCG noise source, advanced every frame.
        self.seed = self.seed.wrapping_mul(0x41c6_4e6d).wrapping_add(0x3039);
        let noise = self.seed as f32 * NOISE_SCALE;

        let mut frame = self.frame + 1;
        if self.ctrl < frame {
            frame = 0;
            if self.flags != 0 {
                self.control_tick();
            }
        }
        self.frame = frame;

        // Recursion over noise: two state taps, shaped by the flag-1 normaliser.
        let a = self.z1 * self.shape_sum + self.z2;
        let b = (noise - (a + a)) * self.normaliser * self.shape + self.z1;
        let c = b + self.z1;
        let b = b + TINY;
        let d = c * TAP;
        let c = self.shape * c + self.z2;
        let e = c + TINY;
        let w = (((c + self.z2) - d) * self.gain2 + d) * self.gain4;

        let slot = write.min(RING as u32 - 1) as usize;
        self.ring[slot] = w;
        self.z2 = e;
        self.z1 = b;

        let out_l = (self.mix * in_l + w) * gain * self.wet + self.dry * in_l;
        let delayed = self.ring[read];
        let out_r = (self.mix * in_r + delayed) * gain * self.wet + self.dry * in_r;

        self.write = if write + 1 < RING as u32 {
            write + 1
        } else {
            write - (RING as u32 - 1)
        };
        [out_l, out_r]
    }

    /// Ramp update of the five flag bits, run once per control period (`+0x70`).
    fn control_tick(&mut self) {
        let step = self.step;
        // Bit 1: ramp to unity, then the shaping curve (levels +0xac, +0x114, +0x118, +0x120).
        if self.flags & 1 != 0 {
            let mut v = step + self.ramp[0];
            if ONE <= v {
                self.flags &= !1;
                v = ONE;
            }
            self.ramp[0] = v;
            v = v * self.slope[0] + self.offset[0];
            self.level[0] = v;
            if v <= 0. {
                v = 0.;
            }
            v *= self.scale;
            let cube = v * v * v;
            let g = cube * v * v * SHAPE_5 + cube * SHAPE_3 + v;
            self.shape = g;
            self.shape_sum = g + SHAPE_OFFSET;
            self.normaliser = ONE / (g * self.shape_sum + ONE);
        }
        // Bit 2: level +0xbc, gain2 (+0x124).
        if self.flags & 2 != 0 {
            let mut v = step + self.ramp[1];
            if ONE <= v {
                self.flags &= !2;
                v = ONE;
            }
            self.ramp[1] = v;
            v = v * self.slope[1] + self.offset[1];
            self.gain2 = v;
            self.level[1] = v;
        }
        // Bit 4: level +0xcc, gain4 (+0x128).
        if self.flags & 4 != 0 {
            let mut v = step + self.ramp[2];
            if ONE <= v {
                self.flags &= !4;
                v = ONE;
            }
            self.ramp[2] = v;
            v = v * self.slope[2] + self.offset[2];
            self.gain4 = v;
            self.level[2] = v;
        }
        // Bit 8: level +0xdc, dry (+0xf0) and wet (+0x12c) from the window shape.
        if self.flags & 8 != 0 {
            let mut v = step + self.ramp[3];
            if ONE <= v {
                self.flags &= !8;
                v = ONE;
            }
            self.ramp[3] = v;
            v = v * self.slope[3] + self.offset[3];
            self.level[3] = v;
            self.level[4] = v;
            let (dry, wet) = if self.mode < 1 {
                let wet = (TWO - v) * v;
                (ONE - wet, wet)
            } else {
                let mut dry = (ONE - v) + (ONE - v);
                let mut wet = v + v;
                if ONE <= v + v {
                    wet = ONE;
                }
                if ONE <= dry {
                    dry = ONE;
                }
                (dry, wet)
            };
            self.dry = dry;
            self.wet = wet;
        }
        // Bit 16: mix level +0x104 (and +0xf4).
        if self.flags & 16 != 0 {
            let mut v = step + self.ramp[4];
            if ONE <= v {
                self.flags &= !16;
                v = ONE;
            }
            self.ramp[4] = v;
            v = v * self.slope[4] + self.offset[4];
            self.mix = v;
            self.level[5] = v;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Native vectors (`tools/re/gater_kernel_kemu.py`, kemu on Kontakt8.exe): the kernel run on
    /// the same voice state, stimulus and clock level, frame by frame.
    #[test]
    fn native_kernel_vectors() {
        let cases: Vec<serde_json::Value> =
            serde_json::from_str(include_str!("gater_kemu_vectors.json")).unwrap();
        let mut failures = Vec::new();
        for case in &cases {
            let name = case["name"].as_str().unwrap();
            let f = |k: &str| case[k].as_f64().unwrap() as f32;
            let arr = |v: &serde_json::Value| -> Vec<f32> {
                v.as_array()
                    .unwrap()
                    .iter()
                    .map(|x| x.as_f64().unwrap() as f32)
                    .collect()
            };
            let mut k = GaterKernel {
                ctrl: case["ctrl"].as_i64().unwrap() as i32,
                mode: case["mode"].as_i64().unwrap() as i32,
                flags: case["flags"].as_u64().unwrap() as u32,
                step: f("step"),
                scale: f("scale"),
                delay: case["delay"].as_i64().unwrap() as i32,
                seed: case["seed"].as_i64().unwrap() as i32,
                ..GaterKernel::default()
            };
            for (i, r) in case["ramp"].as_array().unwrap().iter().enumerate() {
                let r = arr(r);
                k.ramp[i] = r[0];
                k.slope[i] = r[1];
                k.offset[i] = r[2];
            }
            let (stim_l, stim_r) = (arr(&case["stim"][0]), arr(&case["stim"][1]));
            let (gain, out_l, out_r) = (arr(&case["gain"]), arr(&case["outL"]), arr(&case["outR"]));
            let peak = out_l
                .iter()
                .chain(&out_r)
                .fold(0f32, |m, v| m.max(v.abs()))
                .max(1e-9);
            let mut worst = 0f32;
            for n in 0..gain.len() {
                let [l, r] = k.frame([stim_l[n], stim_r[n]], gain[n]);
                worst = worst.max((l - out_l[n]).abs()).max((r - out_r[n]).abs());
            }
            if worst > 1e-5 * peak {
                failures.push(format!("{name}: worst {worst:e} vs peak {peak:e}"));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}
