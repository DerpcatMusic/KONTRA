//! Kontakt 8 Replika core (`NI::DSP::FXFDelayLight`, object 0x14980 B), piece 1: feedback, LOWCUT and
//! HIGHCUT, SATURATION, output path. Decompile: the per-frame loop `141aa9480` (`0141aa0000.c` 5277-5700),
//! the head write `141acd110`, head read `141ab7a90` with mode-0 tap `141ab82e0`, the head gain setter
//! `141abba60`, the feedback gain `141ac9420` (`1.2 x index 5`), the TPT pair `141aa3ec0`, the saturator
//! `141aa44a0` and the limiter `141ae3ba0`. Generic: no product names.
//!
//! Method and evidence: `docs/re/engine/k-replika-binding.md`, `k-replika-gaps.md` ("Piece 1").
//! Checked against the emulated native vectors `docs/re/engine/vectors/replika-native-*.json` (native
//! output of the original code under the kemu emulator; not a Kontakt render).
//!
//! Ducking (piece 2): the gate of `141aa9480` steps 10 and 14 (setters 17, 18, 19), see [`Gate`]. Its
//! detector is the instantaneous |mid| of the two taps. The head position update `141abd290` (named
//! "ducking" in the earlier notes) is head delay positions, not the ducker; it stays unported (GAP-M).
//!
//! Not ported (see the gaps doc): the lane banks (the lane products `m` are 1 in every
//! vector, taken as 1), the LFO and rotator (head read depth `s` is 0, so the rotator output does not
//! reach the read), head gain index 12 away from its reset value 0.5 (floor `f9` unbacked), the head
//! position update `141abd290` (GAP-M), the mode 1 reader (GAP-C), the glide 3 object `F+0x100` (its enable is clear).
use sampler_ir::fx::timemod::{Cut as CutSpec, ReplikaControls, linear_sweep_hz, log_sweep_hz};

use super::db_limiter::DbLimiter;
use super::tpt_svf::{Cut, CutState, Saturator, SaturatorState};

/// Setter indices of the controls this piece reads (`141ac5980`, labels in `k-replika-binding.md`).
pub mod index {
    pub const INPUT_A: usize = 0;
    pub const INPUT_B: usize = 1;
    pub const LOWCUT: usize = 2;
    pub const HIGHCUT: usize = 3;
    pub const DRY_WET: usize = 4;
    pub const FEEDBACK: usize = 5;
    pub const TIME: usize = 8;
    /// Path selector (`141aca670`): k = min(4, trunc(5 v)), see `head::path_for_index`.
    pub const SELECT: usize = 28;
    /// Head gain (index 12): not wired, its floor is unbacked (GAP-H); listed for the setter table.
    #[allow(dead_code)]
    pub const HEAD_GAIN: usize = 12;
    pub const PAN_DRY: usize = 13;
    pub const PAN_WET: usize = 14;
    pub const DELAY_OFFSET: usize = 15;
    pub const WIDTH: usize = 16;
    /// Ducking gate (`141ac5980` cases 0x11, 0x12, 0x13). The tie to the saved DUCKING_* fields is INFERRED.
    pub const DUCKING_SENSE: usize = 17;
    pub const DUCKING_AMOUNT: usize = 18;
    pub const DUCKING_RELEASE: usize = 19;
    pub const SATURATION: usize = 29;
    pub const TAP_GAIN: usize = 52;
    pub const COUNT: usize = 53;
}

/// Reset values of the setters the wrapper writes at reset (`1408ddbc0`, constants in `constants.tsv`:
/// `144f38814` = 0.762405 for indices 0 and 1, `144f38764` = 0.3 for index 15). Index 52 has no reset
/// entry; its implicit value is 1.0 (observed in the emulator: 0.0 silences the output).
const RESET: [(usize, f32); 33] = [
    (0, 0.762_405),
    (1, 0.762_405),
    (4, 1.),
    (7, 0.),
    (10, 0.),
    (11, 0.5),
    (12, 0.5),
    (13, 0.5),
    (14, 0.5),
    (15, 0.3),
    (16, 0.5),
    (17, 0.),
    (18, 0.),
    (19, 0.),
    (25, 0.),
    (26, 1.),
    (30, 0.),
    (31, 0.),
    (32, 1.),
    (34, 0.),
    (35, 0.),
    (36, 0.),
    (37, 0.),
    (39, 0.),
    (40, 0.),
    (41, 0.),
    (42, 0.),
    (43, 0.),
    (44, 0.),
    (45, 0.),
    (46, 0.),
    (51, 1.),
    (52, 1.),
];

/// Input and output level law (setter `141a90910`, measured): `P = (T4(x) - C) D` with
/// `x = 499.980408 v + 819.609009` (constants `1479ab5dc`, `1479ab5ec`, `constants.tsv`), T4 the dB table
/// with linear interpolation. `C` and `D` are not in `constants.tsv` (`1479ab128`, `1479ab280` are not
/// referenced there); they are fitted to the native output of setter 0 (13 points, residual 1e-7).
const CONV_OFFSET: f32 = 0.012_345_674;
const CONV_SCALE: f32 = 1.003_110_8;

/// Head delay offsets (frames, 48 kHz) from the TIME base `T` (`96000 v8 - 600 v15`), measured from the
/// head read bases after the first update (`H+0xd8`, `H+0xe0`, bias `H+0xb4` = -2):
/// head 1 L = T, head 1 R = T + 360, head 2 L = T + 540, head 2 R = T - 180, head 3 L = T - 180, head 3 R = T + 540.
/// The offsets are measured at one TIME only (0.02); their TIME dependence is not decompiled.
const HEAD_OFFSET: [[f64; 2]; 3] = [[0., 360.], [540., -180.], [-180., 540.]];
/// Head bias (`H+0xb4` = -2): the read is 2 frames earlier than the base.
const HEAD_BIAS: f64 = 2.;
/// Feedback gain (`141ac9420`): head 2 and head 3 output gain = 1.2 x index 5 (`1479ab410` = 1.2).
const FEEDBACK_SCALE: f32 = 1.2;

/// Ducking constants (`144714948` = 0.05 as f64, `1479ab510` = 333.33331750085034, `14769abe8` = 0.001 as
/// f64, `144fc44fc` = 9.96578407 f32, `144fc4b50` = 1200 f32; `constants.tsv`). Release times in ms: 200 at
/// index 19 = 0 and 7500 at 1 (`0141ac0000.c` 3616-3630, setup `0141aa0000.c` 1101-1104).
const GATE_BASE: f64 = f64::from_bits(0x3fa9_9999_a000_0000);
const GATE_RETENTION_NUM: f64 = f64::from_bits(0x4074_d555_44bb_5563);
const GATE_THRESHOLD_FLOOR: f32 = 0.001;
const GATE_MS_SCALE: f64 = f64::from_bits(0x3f50_624d_e000_0000);
const GATE_DB_PER_INDEX: f32 = 9.965_784_f32;
const GATE_MIN_RELEASE_MS: f32 = 200.;
const GATE_RELEASE_SPAN_MS: f32 = 7_300.;

/// Ducker (the gate of `141aa9480`, steps 10 and 14). Detector: the instantaneous |mid| of the two taps,
/// mid = 0.5 (tapA + tapB). Closed branch (|mid| < threshold): env = env x release. Open branch: env moves
/// toward the target with the retention. The wet taps are scaled by 10^(env/20). All four are CONFIRMED by
/// the decompile arithmetic; the names (sense, amount, release) are INFERRED from the setter cases.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Gate {
    /// `+0x13e18`: max(1 - v17, 0.001). Sense: higher values trigger on quieter input.
    threshold: f64,
    /// `+0x13e10`: -60 x v18 dB (amount). 0 dB means no duck.
    target: f64,
    /// `+0x13e28`: 0.05^(1 / ((200 + 7300 v19) x 0.001 x rate)) per frame, the closed-branch decay.
    release: f64,
    /// `+0x13e20`: 0.05^(333.33331750085034 / rate) per frame, the open-branch retention (setup `141aa1580`).
    retention: f64,
}

impl Gate {
    fn new(rate: f32, sense: f32, amount: f32, release: f32) -> Self {
        // `141ac5980` case 0x11: 1 - v, floored at 0.001 (f32).
        let threshold = (1. - sense).max(GATE_THRESHOLD_FLOOR);
        // Case 0x12: v x -60 (f32).
        let target = amount * -60.;
        // Case 0x13: the exponent is 1 / (lerp(200, 7500, v) ms x 0.001 x rate), in f64.
        let ms = GATE_RELEASE_SPAN_MS * release + GATE_MIN_RELEASE_MS;
        let release = GATE_BASE.powf(1. / (f64::from(ms) * GATE_MS_SCALE * f64::from(rate)));
        let retention = GATE_BASE.powf(GATE_RETENTION_NUM / f64::from(rate));
        Self {
            threshold: f64::from(threshold),
            target: f64::from(target),
            release,
            retention,
        }
    }
}

/// Ring capacity in frames: a power of two of 2.5 s. The native rings are 2^19 frames (`H+0x15c`); the
/// largest read is TIME 2000 ms + 540 + 2 frames, so 2.5 s covers it at 48 kHz. Equal output while the
/// longest read fits; the ring is per-head stereo, 3 heads.
pub fn ring_len(rate: f32) -> usize {
    ((2.5 * f64::from(rate)) as usize).next_power_of_two()
}

/// Storage the caller provides for the three heads (`[L, R]` per frame, head-major).
pub fn storage_len(rate: f32) -> usize {
    3 * ring_len(rate)
}

/// Slab entries after the rings for the packed [`ReplikaState`] (30 words in 15 `[f64; 2]` slots: the frame
/// counter, 28 f32 filter, saturator and limiter words, and the ducker dB envelope).
pub const STATE_SLOTS: usize = 15;

/// T4 dB table with linear interpolation (`1404d14d0`, entries `2^(k/60 - 20)`, deltas to the next entry).
/// The native index is `(short)(int)x`: below 0 (so x <= -1) the gain is 0, and in (-1, 0) it interpolates from
/// entry 0. Entries are f32 of the exact `2^(k/60 - 20)` (checked against the table under kemu, 2400 entries).
fn t4(x: f32) -> f32 {
    let k = x.trunc();
    if k < 0. {
        return 0.;
    }
    if k >= 2400. {
        return 1_048_576.;
    }
    let f = x - k;
    let v = |k: f32| (f64::from(k) / 60. - 20.).exp2() as f32;
    v(k) + f * (v(k + 1.) - v(k))
}

/// Input or output level from a setter value (`141a90910`, measured law above).
fn level(v: f32) -> f32 {
    (t4(v * 499.980_408 + 819.609_009) - CONV_OFFSET) * CONV_SCALE
}

/// Derived controls (`Ctl(x)` in `k-replika-core.md`: `P0 + x`), refreshed by each setter.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Ctl {
    /// `Ctl(0x0)`: input level, both channels (index 0).
    input: f32,
    /// `Ctl(0x4)`: output level (index 1, same law).
    output: f32,
    /// `Ctl(0x8)` and `Ctl(0xc)`: dry gain and wet gain (index 4).
    dry: f32,
    wet: f32,
    /// `Ctl(0x1c)` and `Ctl(0x20)`: dry pan pair (index 13).
    dry_l: f32,
    dry_r: f32,
    /// `Ctl(0x24)` and `Ctl(0x28)`: wet pan pair (index 14).
    wet_l: f32,
    wet_r: f32,
    /// `Ctl(0x64)`: side scale (index 16, WIDTH).
    side: f32,
    /// `Ctl(0x68)`: tap gain (index 52).
    tap: f32,
}

/// The whole per-frame core: the immutable plan (controls, sections, delays). Mutable per-channel state
/// is in [`ReplikaState`], and the heads' samples are in the caller's storage (see [`storage_len`]).
#[derive(Clone, Debug)]
pub struct ReplikaCore {
    rate: f32,
    raw: [f32; index::COUNT],
    ctl: Ctl,
    saturation: f32,
    feedback_gain: f32,
    gate: Gate,
    delay: [[usize; 2]; 3],
    cut: Cut,
    saturator: Saturator,
    ring: usize,
    /// Setter writes so far. The native setter (`141ac5980`) runs its tail on every write, and the mode-1 spread snap
    /// is driven by this count (see `chain::Mode1::seen`).
    writes: u32,
}

/// Mutable state: write index, the TPT pair of each filter and channel, the saturator states and the
/// two limiters (matrix output and head 3 path).
#[derive(Clone, Debug)]
pub struct ReplikaState {
    frame: usize,
    filters: [[CutState; 2]; 2],
    saturator: [SaturatorState; 2],
    matrix_limiter: DbLimiter,
    head3_limiter: DbLimiter,
    /// Ducker envelope in dB (`+0x13e38`, starts at 0 dB).
    env: f64,
    /// The mode-1 chain (heads, rings, spread table), present once the caller has placed it (`chain.rs`).
    mode1: Option<Box<chain::Mode1>>,
}

impl ReplikaState {
    pub fn new(rate: f32) -> Self {
        Self {
            frame: 0,
            filters: [[[Default::default(); 2]; 2]; 2],
            saturator: [[Default::default(); 2]; 2],
            matrix_limiter: DbLimiter::new(rate),
            head3_limiter: DbLimiter::new(rate),
            env: 0.,
            mode1: None,
        }
    }

    /// The plain state (everything but the mode-1 chain), for [`ReplikaState::store`].
    fn plain(&self) -> Self {
        Self {
            frame: self.frame,
            filters: self.filters,
            saturator: self.saturator,
            matrix_limiter: self.matrix_limiter,
            head3_limiter: self.head3_limiter,
            env: self.env,
            mode1: None,
        }
    }

    /// Every f32 of the state in packing order: filter sections, saturator sections, limiter dB states.
    fn each(&mut self, mut f: impl FnMut(&mut f32)) {
        for sec in self
            .filters
            .iter_mut()
            .flatten()
            .flatten()
            .chain(self.saturator.iter_mut().flatten())
        {
            f(&mut sec.s0);
            f(&mut sec.s1);
        }
        for lim in [&mut self.matrix_limiter, &mut self.head3_limiter] {
            let mut d = lim.dynamics();
            d.iter_mut().for_each(&mut f);
            lim.set_dynamics(d);
        }
    }

    /// Read the state packed by [`ReplikaState::store`] from `STATE_SLOTS` slab entries. Zeros are the
    /// reset state (the slab is cleared at reset).
    pub fn load(rate: f32, slots: &[[f64; 2]]) -> Self {
        let mut st = Self::new(rate);
        let mut words = slots.iter().flatten().copied();
        st.frame = words.next().unwrap_or(0.) as usize;
        st.each(|x| *x = words.next().unwrap_or(0.) as f32);
        st.env = words.next().unwrap_or(0.);
        st
    }

    /// Pack the state into the slab entries (f32 values widen to f64 exactly, so `load` restores them).
    pub fn store(&self, slots: &mut [[f64; 2]]) {
        let mut words = slots.iter_mut().flatten();
        let mut put = |x: f64| {
            if let Some(w) = words.next() {
                *w = x;
            }
        };
        put(self.frame as f64);
        let mut st = self.plain();
        st.each(|x| put(f64::from(*x)));
        put(self.env);
    }
}

impl ReplikaCore {
    /// The core for the Kontakt 8 Replika controls (`sampler_ir::fx::timemod::ReplikaControls`); every
    /// other setter keeps its reset value.
    pub fn from_controls(rate: f32, c: &ReplikaControls) -> Self {
        let mut core = Self::new(rate);
        let values = [
            (index::INPUT_A, c.input_a),
            (index::INPUT_B, c.input_b),
            (index::LOWCUT, c.lowcut),
            (index::HIGHCUT, c.highcut),
            (index::DRY_WET, c.dry_wet),
            (index::FEEDBACK, c.feedback),
            (index::TIME, c.time),
            (index::DELAY_OFFSET, c.delay_offset),
            (index::PAN_DRY, c.pan_dry),
            (index::PAN_WET, c.pan_wet),
            (index::WIDTH, c.width),
            (index::SATURATION, c.saturation),
            (index::TAP_GAIN, c.tap_gain),
            (index::DUCKING_SENSE, c.ducking_sense),
            (index::DUCKING_AMOUNT, c.ducking_amount),
            (index::DUCKING_RELEASE, c.ducking_release),
        ];
        for (i, v) in values {
            core.set_param(i, v);
        }
        core
    }

    /// The core with the reset values (`1408ddbc0`); index 8 (TIME) and index 3 (HIGHCUT) start at the
    /// harness choices of `replika_kemu_vectors.py` and are expected to be set by the caller.
    pub fn new(rate: f32) -> Self {
        let mut raw = [0.; index::COUNT];
        for (i, v) in RESET {
            raw[i] = v;
        }
        raw[index::TIME] = 0.02;
        raw[index::HIGHCUT] = 1.;
        let mut core = Self {
            rate,
            raw,
            writes: 0,
            ctl: Ctl {
                input: 1.,
                output: 1.,
                dry: 1.,
                wet: 1.,
                dry_l: 1.,
                dry_r: 1.,
                wet_l: 1.,
                wet_r: 1.,
                side: 0.5,
                tap: 1.,
            },
            saturation: 0.,
            feedback_gain: 0.,
            gate: Gate::new(rate, 0., 0., 0.),
            delay: [[1; 2]; 3],
            cut: Cut::new(
                CutSpec {
                    highpass_hz: None,
                    lowpass_hz: None,
                },
                rate,
            ),
            saturator: Saturator::new(rate),
            ring: ring_len(rate),
        };
        core.refresh();
        core
    }

    /// Setter (index, value). The value is clamped to 0..=1 as the wrapper clamps it. Live: the next
    /// frame uses the new value (the native live path writes the control at once).
    pub fn set_param(&mut self, index: usize, value: f32) {
        if index >= index::COUNT {
            return;
        }
        self.writes = self.writes.wrapping_add(1);
        self.raw[index] = if value.is_nan() {
            0.
        } else {
            value.clamp(0., 1.)
        };
        self.refresh();
    }

    /// Recompute the derived controls from the raw values.
    fn refresh(&mut self) {
        let v = self.raw;
        let (dry, wet) = if v[index::DRY_WET] > 0.5 {
            (2. * (1. - v[index::DRY_WET]), 1.)
        } else {
            (1., 2. * v[index::DRY_WET])
        };
        self.ctl = Ctl {
            input: level(v[index::INPUT_A]),
            output: level(v[index::INPUT_B]),
            dry,
            wet,
            dry_l: 2. * (1. - v[index::PAN_DRY]),
            dry_r: 2. * v[index::PAN_DRY],
            wet_l: 2. * (1. - v[index::PAN_WET]),
            wet_r: 2. * v[index::PAN_WET],
            side: v[index::WIDTH],
            tap: v[index::TAP_GAIN],
        };
        self.saturation = v[index::SATURATION];
        self.feedback_gain = FEEDBACK_SCALE * v[index::FEEDBACK];
        self.gate = Gate::new(
            self.rate,
            v[index::DUCKING_SENSE],
            v[index::DUCKING_AMOUNT],
            v[index::DUCKING_RELEASE],
        );
        // TIME (mode 3): 2000 ms x v, 48 frames per ms at 48 kHz; the delay offset index 15 is 600 frames at 48 kHz.
        let scale = f64::from(self.rate) / 48_000.;
        let time = 2. * f64::from(self.rate) * f64::from(v[index::TIME])
            - 600. * scale * f64::from(v[index::DELAY_OFFSET]);
        for (h, offsets) in HEAD_OFFSET.iter().enumerate() {
            for (c, o) in offsets.iter().enumerate() {
                let frames = (time + o * scale + HEAD_BIAS).round().max(1.);
                self.delay[h][c] = frames as usize;
            }
        }
        // LOWCUT (highpass, enabled when != 0) and HIGHCUT (lowpass, enabled when != 1), as in `replika_cut`.
        let spec = CutSpec {
            highpass_hz: (v[index::LOWCUT] != 0.)
                .then(|| log_sweep_hz(v[index::LOWCUT], 10., 1000.)),
            lowpass_hz: (v[index::HIGHCUT] != 1.)
                .then(|| linear_sweep_hz(v[index::HIGHCUT], 200., 20_000.)),
        };
        self.cut = Cut::new(spec, self.rate);
    }

    /// Head read delays in frames, `[head][L, R]`: the sample written `delay` frames ago.
    pub fn delays(&self) -> [[usize; 2]; 3] {
        self.delay
    }

    /// Process `input` (L, R) into `output` (L, R). `storage` is `storage_len(rate)` `[L, R]` entries.
    pub fn process(
        &self,
        state: &mut ReplikaState,
        storage: &mut [[f64; 2]],
        input: [&[f64]; 2],
        output: [&mut [f64]; 2],
    ) {
        let [input_l, input_r] = input;
        let [output_l, output_r] = output;
        let n = input_l
            .len()
            .min(input_r.len())
            .min(output_l.len())
            .min(output_r.len());
        self.sync_selector(state);
        let mode1 = state.mode1.as_ref().is_some_and(|m| m.heads.path == 1);
        for i in 0..n {
            let x = [input_l[i] as f32, input_r[i] as f32];
            let [l, r] = if mode1 {
                self.frame_mode1(state, x)
            } else {
                self.frame(state, storage, x)
            };
            output_l[i] = f64::from(l);
            output_r[i] = f64::from(r);
        }
    }

    /// One frame, in the order of `141aa9480`. Lane products `m` are 1 and the gate is open (no gain).
    fn frame(&self, st: &mut ReplikaState, storage: &mut [[f64; 2]], x: [f32; 2]) -> [f32; 2] {
        let ring = self.ring;
        let t = st.frame;
        st.frame = st.frame.wrapping_add(1);
        let c = self.ctl;
        let dry = [c.input * x[0], c.input * x[1]];
        // Path 0 taps: input x tap gain, saturated when the amount is set, crossfaded with the dry tap.
        let a = self.saturation;
        let mut tap = [dry[0] * c.tap, dry[1] * c.tap];
        if a != 0. {
            tap[0] = self.saturator.tick(&mut st.saturator[0], tap[0], a);
            tap[1] = self.saturator.tick(&mut st.saturator[1], tap[1], a);
        }
        tap = [
            dry[0] * c.tap * (1. - a) + a * tap[0],
            dry[1] * c.tap * (1. - a) + a * tap[1],
        ];

        let duck = self.duck(&mut st.env, tap);

        // Head 1: write the taps, read at its delays. Head 3: feedback read x gain, path-0 limiter at half scale.
        write(storage, ring, 0, t, tap);
        let head1 = [
            read(storage, ring, 0, t, self.delay[0][0], 0),
            read(storage, ring, 0, t, self.delay[0][1], 1),
        ];
        let g = self.feedback_gain;
        let head3 = [
            g * read(storage, ring, 2, t, self.delay[2][0], 0),
            g * read(storage, ring, 2, t, self.delay[2][1], 1),
        ];
        let head3 = half_scale(&mut st.head3_limiter, head3);

        // Mix, F1 on the mix, head 2 (F1 out), F2 on the head 2 read x gain, head 3 (F2 out).
        let mix = [head3[0] + head1[0], head3[1] + head1[1]];
        let f1 = [
            self.cut.tick(&mut st.filters[0][0], mix[0]),
            self.cut.tick(&mut st.filters[0][1], mix[1]),
        ];
        write(storage, ring, 1, t, f1);
        let h2 = [
            g * read(storage, ring, 1, t, self.delay[1][0], 0),
            g * read(storage, ring, 1, t, self.delay[1][1], 1),
        ];
        let f2 = [
            self.cut.tick(&mut st.filters[1][0], h2[0]),
            self.cut.tick(&mut st.filters[1][1], h2[1]),
        ];
        write(storage, ring, 2, t, f2);

        // OUT = F1 Ctl(0x60) + F2 (Ctl(0x60) = 1), wet taps (Ctl(0xc), pan pair, ducker gain), then the matrix.
        let out = [f1[0] + f2[0], f1[1] + f2[1]];
        self.tail(&mut st.matrix_limiter, dry, out, duck)
    }

    /// The ducker (steps 10 and 14 of `141aa9480`): the instantaneous |mid| of the taps against the threshold moves the
    /// dB envelope `env`; returns the wet gain of the frame.
    pub(super) fn duck(&self, env: &mut f64, tap: [f32; 2]) -> f32 {
        let detect = 0.5 * (tap[0] + tap[1]);
        let gate = self.gate;
        *env = if f64::from(detect.abs()) < gate.threshold {
            *env * gate.release
        } else {
            (*env - gate.target) * gate.retention + gate.target
        };
        t4(*env as f32 * GATE_DB_PER_INDEX + 1200.)
    }

    /// The wet taps, the matrix and the output limiter (the end of `141aa9480`), shared by both paths. `out` is the
    /// head sum of the frame, `dry` the input taps.
    pub(super) fn tail(
        &self,
        matrix_limiter: &mut DbLimiter,
        dry: [f32; 2],
        out: [f32; 2],
        duck: f32,
    ) -> [f32; 2] {
        let c = self.ctl;
        let wet = [
            out[0] * c.wet * c.wet_l * duck,
            out[1] * c.wet * c.wet_r * duck,
        ];
        let mid = 0.5 * (wet[0] + wet[1]);
        let side2 = (wet[0] - wet[1]) * c.side;
        let a_new = dry[0] * c.dry * c.dry_l + side2 + mid;
        let b_new = mid - side2 + dry[1] * c.dry * c.dry_r;
        let y = matrix_limiter.process([a_new * c.output * 0.5, b_new * c.output * 0.5], [0., 0.]);
        [y[0] * 2., y[1] * 2.]
    }
}

/// Head write (`141acd110`, mode 0): slot `t` of head `head`'s ring, both channels.
fn write(storage: &mut [[f64; 2]], ring: usize, head: usize, t: usize, value: [f32; 2]) {
    storage[head * ring + (t & (ring - 1))] = [f64::from(value[0]), f64::from(value[1])];
}

/// Head read (`141ab82e0`, mode 0): the sample `delay` frames before write index `t` (zero before the start).
fn read(storage: &[[f64; 2]], ring: usize, head: usize, t: usize, delay: usize, ch: usize) -> f32 {
    storage[head * ring + (t.wrapping_sub(delay) & (ring - 1))][ch] as f32
}

/// Path 0 limiter at half scale (`0141aa0000.c` 5561-5578): x 0.5, limiter, x 2.
fn half_scale(lim: &mut DbLimiter, x: [f32; 2]) -> [f32; 2] {
    let y = lim.process([x[0] * 0.5, x[1] * 0.5], [0., 0.]);
    [y[0] * 2., y[1] * 2.]
}

mod chain;
mod delay;
mod deposit;
mod design;
mod head;
mod mode1;
mod read;
mod write;
#[cfg(test)]
mod tests;
