//! Mode-1 chain of the Replika core (selector path k = 1, index 28 = 0.3): the three inline heads, the spread table
//! and the frame of `141aa9480` for path 1 (decompile `0141aa0000.c` 5277-5700, the loop body).
//!
//! Path 1 differs from path 0 (the mode-0 topology in `mod.rs`): the taps are not saturated, head 3 has no limiter,
//! and the heads are written and read in the native order: H1 write and read, H3 read, F1 on the mix, H2 write and
//! read, F2, H3 write, then the output term `F1 Ctl(0x60) + F2` (Ctl(0x60) = 1) and the shared wet, matrix and limiter
//! tail. Before the taps, the spread table glides and the caller `141abd290` (`delay::positions`) places the heads'
//! delay positions every frame while `C+0x938` is set.
//!
//! Start state: [`Mode1::start`] (`start.rs`) builds it from the rate and the reset controls. The native dump
//! `docs/re/engine/vectors/replika-native-mode1-index28-start.json` (`tools/re/replika_mode1_start_kemu.py`) is only the
//! oracle of `tests::start_matches_native_dump`, through [`Mode1::from_native`] (test only). The ring is zero at start.
//!
//! Setter writes: the native setter `141ac5980` runs its tail on every write while the core is active (`C+0x8`):
//! `141a95830` snaps each spread entry whose flag is set onto its target and clears the flag, and `141abd290` places
//! the head positions. `ReplikaCore::writes` counts the writes and [`Mode1::snap`] runs once per new write. The
//! vectors write index 28 every quantum, so without the snap the spread width stays on its glide and the output onset
//! is 131 frames early.
//!
//! The head read wrapper `141ab7a90` multiplies each head's read by its level glide (`+0x5828`, `H+0x70`) and gain
//! glide (`+0x5848`, `H+0x74`) and mixes the channels with `H+0x78` and `H+0x7c` ([`Wrap::apply`]). Heads 2 and 3 have
//! level 0 in this state, so they read zero, as the native wrapper does.
//!
//! Not ported, inert in the vectors: the LFO `141ab7480` and the rotator (every head depth `s` is 0, so the
//! modulation is zero), the bypass fade `141acc600`, the wrapper's fade and hold branches (`H+0x5824` and `H+0x6c`
//! are zero), the lowcut and highcut sections (identity at the defaults).
//! Not ported: the design chain for a selector that is not already applied (`Head::set_mode` does not change the
//! lane objects; the start dump has them), the mode-1 tap ring clear for the flag-1 path, and paths 2 to 4.

use super::delay::{Spread, positions};
use super::head::{Fade, Glide, Head, Heads, Lfo};
use super::mode1::{PostFilter, WriteFilter};
use super::read::Tap;
use super::{ReplikaCore, ReplikaState, index};
use crate::dsp::tpt_svf::{Section, State};

/// Words of one head object (`0x5900` bytes) and of one write lane object (`0x1480` bytes), for the dump loader.
#[cfg(test)]
const HEAD_WORDS: usize = 0x5900 / 4;
#[cfg(test)]
const LANE_WORDS: usize = 0x1480 / 4;
/// Lane objects inside a head: L at `+0x2dc0`, R at `+0x4240`.
#[cfg(test)]
const LANE_L: usize = 0x2dc0;
#[cfg(test)]
const LANE_R: usize = 0x4240;

/// One entry of the smoothed spread table (`C+0x14210 + 0x28 k`): `value` glides toward `target` by `coef` per frame
/// (`coef` is the previous entry's last slot). `flag` (`+0x18`) is set when the target moved and cleared when the
/// value snaps onto the target inside `thr`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Entry {
    pub value: f64,
    pub target: f64,
    pub thr: f64,
    pub flag: bool,
    pub coef: f64,
}

/// The mode-1 chain: the three heads (`C+0xac0`, `C+0x6380`, `C+0xbc40`) with their tap rings, the spread table and
/// the caller's inputs. `heads.path` is the selector path (1 here).
#[derive(Clone, Debug)]
pub struct Mode1 {
    pub heads: Heads,
    /// `[head][channel]`: the tap rings (`K+0xc0`, the mode-0 ring of the channel).
    pub rings: [[Vec<f32>; 2]; 3],
    /// Table entries in native order: width (`+0x2c`), skew (`+0x34`), ratio (`+0x30`), depth (`+0x38`).
    pub table: [Entry; 4],
    /// The live targets of the entries (the P0 block), in the table order.
    pub targets: [f64; 4],
    /// The caller's bounds `[lo, hi]` (`C+0xad8`, `C+0xad0`) and flag (`C+0x600`).
    pub bounds: [f64; 2],
    pub flag: i32,
    /// `C+0x938`: the caller runs (the stored spreads are used).
    pub smooth: bool,
    /// The core's setter-write count at the last snap (`ReplikaCore::writes`).
    pub seen: u32,
    /// The head read wrappers (`141ab7a90`): the level and gain glides and the own/cross mix of each head.
    pub wrap: [Wrap; 3],
}

/// One glide of the head read wrapper (`141ab7a90`): `cur` steps by `step` toward `target` once per call, and
/// settles on the target inside `thr` (or when `count` runs out) if its flag is set.
#[derive(Clone, Copy, Debug)]
pub struct WrapGlide {
    pub cur: f32,
    pub target: f32,
    pub step: f32,
    pub thr: f32,
    pub count: i32,
    pub flag: bool,
}

impl WrapGlide {
    /// `o` is the glide's first word (`+0x5828` the level, `+0x5848` the gain); the fields follow at +4, +8, +0xc,
    /// +0x14 and +0x1c.
    #[cfg(test)]
    fn load(w: Words, o: usize) -> Self {
        WrapGlide {
            cur: w.f32(o),
            target: w.f32(o + 4),
            step: w.f32(o + 8),
            thr: w.f32(o + 0xc),
            count: w.i32(o + 0x14),
            flag: w.flag(o + 0x1c),
        }
    }

    /// One call's step (the first half of `141ab7a90` for this glide).
    fn step(&mut self) {
        let gap = (self.target - self.cur).abs();
        let settle = if gap <= self.thr {
            true
        } else {
            self.count -= 1;
            self.count < 1
        };
        if settle {
            if self.flag {
                self.cur = self.target;
                self.step = 0.;
                self.count = 0;
                self.flag = false;
            }
        } else {
            self.cur += self.step;
        }
    }
}

/// The head read wrapper of one head: the level (`H+0x70`, glide `+0x5828`), the gain (`H+0x74`, glide `+0x5848`),
/// and the own (`H+0x78`) and cross (`H+0x7c`) mix of the two channels.
#[derive(Clone, Copy, Debug)]
pub struct Wrap {
    pub level: WrapGlide,
    pub gain: WrapGlide,
    pub own: f32,
    pub cross: f32,
}

impl Wrap {
    #[cfg(test)]
    fn load(w: Words) -> Self {
        Wrap {
            level: WrapGlide::load(w, 0x5828),
            gain: WrapGlide::load(w, 0x5848),
            own: w.f32(0x78),
            cross: w.f32(0x7c),
        }
    }

    /// The wrapper's output for the driver's `[L, R]` (`141ab7a90`, the driver path): both glides step, then the
    /// channels take `H74 * H70` and the own/cross mix. The fade and hold branches are inactive in the vectors
    /// (`H+0x5824` and `H+0x6c` are zero) and are not ported.
    fn apply(&mut self, y: [f32; 2]) -> [f32; 2] {
        self.level.step();
        self.gain.step();
        let g = self.gain.cur * self.level.cur;
        let l = g * y[0];
        let r = g * y[1];
        [l * self.own + r * self.cross, r * self.own + l * self.cross]
    }
}

/// Reads a dense u32 word array by native byte offset (the dump loader of the tests).
#[cfg(test)]
#[derive(Clone, Copy)]
struct Words<'a>(&'a [u32]);

#[cfg(test)]
impl Words<'_> {
    fn u32(&self, o: usize) -> u32 {
        self.0[o / 4]
    }
    fn i32(&self, o: usize) -> i32 {
        self.u32(o) as i32
    }
    fn f32(&self, o: usize) -> f32 {
        f32::from_bits(self.u32(o))
    }
    fn f64(&self, o: usize) -> f64 {
        f64::from_bits(u64::from(self.u32(o + 4)) << 32 | u64::from(self.u32(o)))
    }
    fn byte(&self, o: usize) -> u8 {
        (self.u32(o & !3) >> (8 * (o & 3))) as u8
    }
    fn flag(&self, o: usize) -> bool {
        self.byte(o) != 0
    }
    /// A section object at `o`: state `+0`, `+4`, coefficients `+8..+0x10`, damping `+0x2c`.
    fn section(&self, o: usize) -> (State, Section) {
        let state = State {
            s0: self.f32(o),
            s1: self.f32(o + 4),
        };
        let sec = Section::from_native(
            self.f32(o + 8),
            self.f32(o + 0xc),
            self.f32(o + 0x10),
            self.f32(o + 0x2c),
        );
        (state, sec)
    }
    /// A lane object (`0x1480` bytes) at `o`, as the words `WriteFilter::from_object` takes.
    fn lane(&self, o: usize) -> &[u32] {
        &self.0[o / 4..o / 4 + LANE_WORDS]
    }
}

/// One tap object (`K` = `H+0x1a8` L, `H+0x278` R), native offsets in the comments of [`Tap`].
#[cfg(test)]
fn load_tap(w: Words, o: usize) -> Tap {
    Tap {
        pos: w.f64(o),
        mask: w.i32(o + 0x18),
        phase: w.f64(o + 0x28),
        idx: w.i32(o + 0x30),
        x_prev: w.f64(o + 0x38),
        offset: w.f64(o + 0x48),
        sec: Section::from_native(
            w.f32(o + 0x58),
            w.f32(o + 0x5c),
            w.f32(o + 0x60),
            w.f32(o + 0x7c),
        ),
        state: State {
            s0: w.f32(o + 0x50),
            s1: w.f32(o + 0x54),
        },
        freq: w.f32(o + 0x74),
        rate: w.f32(o + 0x80),
        wr: w.i32(o + 0x08),
        frac: w.f64(o + 0x10),
        delta: w.i32(o + 0x1c),
        prev_wr: w.i32(o + 0x20),
        prev_frac: w.f64(o + 0x40),
        rates: [w.f64(o + 0x90), w.f64(o + 0x98), w.f64(o + 0xa0)],
        hist: [w.f32(o + 0x84), w.f32(o + 0x88), w.f32(o + 0x8c)],
        bp: w.f32(o + 0x64),
        lp: w.f32(o + 0x68),
        d: w.f32(o + 0x6c),
        v: w.f32(o + 0x70),
    }
}

/// One head object from the dump (offsets of `Head`'s fields in the comments of `head.rs`).
#[cfg(test)]
fn load_head(w: Words) -> Head {
    let post = |o: usize| PostFilter {
        env: w.f32(o + 0x20),
        coef: w.f32(o + 0x24),
        s0: w.f32(o + 0x28),
        s1: w.f32(o + 0x2c),
        c2: w.f32(o + 0x30),
        c3: w.f32(o + 0x34),
        c4: w.f32(o + 0x38),
        gain: w.f32(o + 0x54),
    };
    let (st0, sec0) = w.section(0x350);
    let (st1, sec1) = w.section(0x384);
    let kern1 = load_tap(w, 0x1a8);
    let rate = kern1.rate;
    Head {
        mode: w.i32(0xc0),
        factor: w.f32(0x80),
        s: w.f64(0x98),
        base_src: w.f64(0x50),
        delay: super::delay::DelayPos {
            lo: w.i32(0x8),
            hi: w.i32(0xc),
            mode: w.i32(0x28),
            base: w.f64(0x48),
            r58: w.f64(0x58),
            r60: w.f64(0x60),
            a: w.f64(0xc8),
            b: w.f64(0xd0),
            flag: w.flag(0x90),
        },
        taps: [kern1, load_tap(w, 0x278)],
        fade: Fade {
            coef: w.f64(0x57c0),
            level: w.f64(0x57c8),
            target: w.f64(0x57d0),
            thr: w.f64(0x57d8),
            pending: w.flag(0x57e0),
            trigger: w.flag(0x57e1),
        },
        glide: Glide {
            cur: w.f32(0x57e8),
            target: w.f32(0x57ec),
            step: w.f32(0x57f0),
            thr: w.f32(0x57f4),
            count: w.i32(0x57fc),
            pending: w.flag(0x5804),
            fresh: w.flag(0x5805),
            div: w.f32(0x57f8),
            count_init: w.i32(0x5800),
        },
        post: [post(0x56c0), post(0x5724)],
        sec: [sec0, sec1],
        rate,
        time: [w.f32(0xa0), w.f32(0xa4), w.f32(0xa8), w.f32(0xac)],
        anchor: [w.f64(0x0), w.f64(0x38), w.f64(0x40)],
        lane: w.i32(0x8c),
        mode_flags: [w.flag(0x68), w.flag(0x69), w.flag(0x6a)],
        kernel: [w.i32(0x270), w.i32(0x340)],
        ring_pos: [w.i32(0x158), w.i32(0x194)],
        ring_wrap: [w.i32(0x15c), w.i32(0x190)],
        hold: w.flag(0x6c),
        fade_on: w.flag(0x5824),
        fade_amount: w.f32(0x5808),
        stored: [w.f32(0x348), w.f32(0x34c)],
        write_filter: [
            WriteFilter::from_object(w.lane(LANE_L)),
            WriteFilter::from_object(w.lane(LANE_R)),
        ],
        sec_state: [st0, st1],
        sec_out: [w.f32(0x368), w.f32(0x39c)],
    }
}

impl Mode1 {
    /// The start state from the native dump (test oracle only: production builds the start with `start.rs`): `heads` are the three head objects as u32 words (`0x5900` bytes each),
    /// `core` the core words (byte offsets from `C`, dense), `p0` the P0 block words (`C+0x80` block). `rate` is the
    /// sample rate. The rings start at zero.
    #[cfg(test)]
    pub fn from_native(heads: [&[u32]; 3], core: &[u32], p0: &[u32], rate: f32) -> Self {
        assert!(
            heads.iter().all(|h| h.len() == HEAD_WORDS),
            "head object words"
        );
        let c = Words(core);
        let table = [0x14210usize, 0x14238, 0x14260, 0x14288].map(|b| Entry {
            value: c.f64(b),
            target: c.f64(b + 8),
            thr: c.f64(b + 0x10),
            flag: c.flag(b + 0x18),
            coef: c.f64(b - 8),
        });
        let p = Words(p0);
        let targets = [0x2c, 0x34, 0x30, 0x38].map(|o| f64::from(p.f32(o)));
        Self {
            heads: Heads {
                path: c.i32(0x604),
                heads: std::array::from_fn(|h| load_head(Words(heads[h]))),
            },
            rings: std::array::from_fn(|_| {
                let n = super::start::ring_len(rate);
                [vec![0.; n], vec![0.; n]]
            }),
            table,
            targets,
            bounds: [c.f64(0xad8), c.f64(0xad0)],
            flag: c.i32(0x600),
            smooth: c.flag(0x938),
            seen: 0,
            wrap: std::array::from_fn(|h| Wrap::load(Words(heads[h]))),
        }
    }

    /// `141aa9480`, the spread table (targets, then one glide step per entry) and the caller `141abd290` while
    /// `C+0x938` is set: the caller reads the stored spreads and sends `(t, s)` to each head's delay position.
    /// `141a95830` (the spread part): an entry whose flag is set takes its target at once.
    fn snap(&mut self) {
        for e in &mut self.table {
            if e.flag {
                e.value = e.target;
                e.flag = false;
            }
        }
    }

    fn spreads(&mut self) {
        for (e, &t) in self.table.iter_mut().zip(&self.targets) {
            if t != e.value {
                e.flag = true;
            }
            e.target = t;
        }
        for e in &mut self.table {
            if (e.target - e.value).abs() <= e.thr {
                if e.flag {
                    e.value = e.target;
                    e.flag = false;
                }
            } else {
                e.value = (1.0 - e.coef) * e.target + e.value * e.coef;
            }
        }
        if self.smooth {
            let v = |i: usize| self.table[i].value;
            let sp = Spread {
                width: v(0),
                skew: v(1),
                depth: v(3),
                ratio: v(2),
            };
            if let Some(p) = positions(sp, self.bounds, self.flag, self.heads.path) {
                for (head, (t, s)) in self.heads.heads.iter_mut().zip(p.heads) {
                    head.delay.set(t, s);
                }
            }
        }
    }
}

/// The two tap rings of head `h` (L, R), as the driver and the write stage take them.
fn ring_pair(rings: &mut [[Vec<f32>; 2]; 3], h: usize) -> [&mut [f32]; 2] {
    let [a, b] = &mut rings[h];
    [a.as_mut_slice(), b.as_mut_slice()]
}

impl ReplikaCore {
    /// The selector side effects (`Heads::select`) when index 28 moved. Without a mode-1 state (no start dump, part 5
    /// packs it) nothing runs and the mode-0 topology stays in use, as before.
    pub(super) fn sync_selector(&self, st: &mut ReplikaState) {
        let Some(mut m) = st.mode1.take() else {
            return;
        };
        // Path 3 is blocked (`Err`): the previous path stays.
        let _ = m.heads.select(st, self.raw[index::SELECT]);
        m.smooth = m.heads.path == 1;
        // The setter tail (`141a95830`) runs once per write: each write snaps the flagged spread entries.
        if m.seen != self.writes {
            m.snap();
            m.seen = self.writes;
        }
        st.mode1 = Some(m);
    }

    /// One frame of path 1 (`141aa9480` with `C+0x604 = 1`), in the native order (see the module doc).
    pub(super) fn frame_mode1(&self, st: &mut ReplikaState, x: [f32; 2]) -> [f32; 2] {
        // The stream's frame counter (the quantum phase of `Stage::process`), as the mode-0 frame keeps it.
        st.frame = st.frame.wrapping_add(1);
        let c = self.ctl;
        let dry = [c.input * x[0], c.input * x[1]];
        let tap = [dry[0] * c.tap, dry[1] * c.tap];
        let ReplikaState {
            mode1,
            filters,
            env,
            matrix_limiter,
            ..
        } = st;
        let m = mode1.as_mut().expect("path 1 runs with the mode-1 state");
        m.spreads();
        let duck = self.duck(env, tap);
        // The LFO is inert here (head depth s = 0 in every vector), so the drive sees zero modulation.
        let lfo = Lfo::default();
        let heads = &mut m.heads.heads;
        let rings = &mut m.rings;
        let wrap = &mut m.wrap;
        heads[0]
            .write(tap, ring_pair(rings, 0))
            .expect("mode 1 writes");
        let h1 = wrap[0].apply(heads[0].drive(lfo, ring_pair(rings, 0)));
        let h3 = wrap[2].apply(heads[2].drive(lfo, ring_pair(rings, 2)));
        let mix = [h3[0] + h1[0], h3[1] + h1[1]];
        let f1 = [
            self.cut.tick(&mut filters[0][0], mix[0]),
            self.cut.tick(&mut filters[0][1], mix[1]),
        ];
        heads[1]
            .write(f1, ring_pair(rings, 1))
            .expect("mode 1 writes");
        let h2 = wrap[1].apply(heads[1].drive(lfo, ring_pair(rings, 1)));
        let f2 = [
            self.cut.tick(&mut filters[1][0], h2[0]),
            self.cut.tick(&mut filters[1][1], h2[1]),
        ];
        heads[2]
            .write(f2, ring_pair(rings, 2))
            .expect("mode 1 writes");
        let out = [f1[0] + f2[0], f1[1] + f2[1]];
        self.tail(matrix_limiter, dry, out, duck)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    const START: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/re/engine/vectors/replika-native-mode1-index28-start.json"
    ));
    const IMPULSE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/re/engine/vectors/replika-native-mode1-index28-v0p3.json"
    ));
    const NOISE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/re/engine/vectors/replika-native-mode1-index28-noise-v0p3.json"
    ));
    const N: usize = 4096;
    const QUANTUM: usize = 32;
    const CORE_WORDS: usize = 0x14980 / 4;
    const TOLERANCE: f32 = 1e-5;
    const RATE: f32 = 48_000.;

    /// Pairs `[offset, u32]` into a dense word array of `len` words.
    fn dense(list: &Value, len: usize) -> Vec<u32> {
        let mut out = vec![0u32; len];
        for p in list.as_array().expect("pair list") {
            let k = p[0].as_u64().expect("offset") as usize;
            out[k / 4] = p[1].as_u64().expect("word") as u32;
        }
        out
    }

    fn floats(v: &Value) -> Vec<f32> {
        v.as_array()
            .expect("samples")
            .iter()
            .map(|x| x.as_f64().expect("sample") as f32)
            .collect()
    }

    /// The start state of the dump (the oracle of `start_matches_native_dump`).
    fn native_dump_start() -> Mode1 {
        let start: Value = serde_json::from_str(START).expect("start json");
        let heads = ["H1", "H2", "H3"].map(|h| dense(&start["heads"][h], HEAD_WORDS));
        let core = dense(&start["core"], CORE_WORDS);
        let p0 = dense(&start["p0"], 0x70 / 4);
        Mode1::from_native([&heads[0], &heads[1], &heads[2]], &core, &p0, RATE)
    }

    /// Every field the chain reads, against the dump: the Debug text of each part, line by line. The rings are not
    /// in the compared parts (they start at zero in both).
    #[test]
    fn start_matches_native_dump() {
        let want = native_dump_start();
        let got = Mode1::start(RATE);
        let parts = [
            (
                "heads",
                format!("{:#?}", want.heads),
                format!("{:#?}", got.heads),
            ),
            (
                "wrap",
                format!("{:#?}", want.wrap),
                format!("{:#?}", got.wrap),
            ),
            (
                "table",
                format!("{:#?}", want.table),
                format!("{:#?}", got.table),
            ),
            (
                "targets",
                format!("{:?}", want.targets),
                format!("{:?}", got.targets),
            ),
            (
                "bounds",
                format!("{:?}", want.bounds),
                format!("{:?}", got.bounds),
            ),
            (
                "flag",
                format!("{:?}", want.flag),
                format!("{:?}", got.flag),
            ),
            (
                "smooth",
                format!("{:?}", want.smooth),
                format!("{:?}", got.smooth),
            ),
        ];
        let mut report = Vec::new();
        for (name, a, b) in &parts {
            if a == b {
                continue;
            }
            let (la, lb): (Vec<_>, Vec<_>) = (a.lines().collect(), b.lines().collect());
            if la.len() != lb.len() {
                report.push(format!("{name}: {} lines against {}", la.len(), lb.len()));
            }
            for (i, (x, y)) in la
                .iter()
                .zip(&lb)
                .enumerate()
                .filter(|(_, (x, y))| x != y)
                .take(16)
            {
                report.push(format!(
                    "{name} line {i}: native `{}` rust `{}`",
                    x.trim(),
                    y.trim()
                ));
            }
        }
        assert!(
            report.is_empty(),
            "start state differs:\n{}",
            report.join("\n")
        );
    }

    /// Runs the vector from the core with its reset defaults written through `set_param`.
    fn check(vector: &str) -> (f32, Option<usize>) {
        let v: Value = serde_json::from_str(vector).expect("vector json");
        let mut core_obj = ReplikaCore::new(RATE);
        for p in v["defaults"].as_array().expect("defaults") {
            core_obj.set_param(
                p[0].as_u64().unwrap() as usize,
                p[1].as_f64().unwrap() as f32,
            );
        }
        run(&v, &mut core_obj)
    }

    /// Runs the vector's stimulus and per-quantum writes through `core_obj` from the Rust start state (the test places
    /// the chain; the stage does not, see k-replika-gaps.md piece 3c part 5b). Returns the max abs error and the first
    /// frame over tolerance.
    fn run(v: &Value, core_obj: &mut ReplikaCore) -> (f32, Option<usize>) {
        let mut state = ReplikaState::new(RATE);
        state.mode1 = Some(Box::new(Mode1::start(RATE)));
        let stim = v["stim"].as_array().expect("stim");
        let (stim_l, stim_r) = (floats(&stim[0]), floats(&stim[1]));
        let mut storage = vec![[0.; 2]; crate::dsp::replika::storage_len(RATE)];
        let (mut out_l, mut out_r) = (vec![0.; N], vec![0.; N]);
        for q in 0..N / QUANTUM {
            for w in v["writes"][q].as_array().expect("writes") {
                core_obj.set_param(
                    w[0].as_u64().unwrap() as usize,
                    w[1].as_f64().unwrap() as f32,
                );
            }
            let span = q * QUANTUM..(q + 1) * QUANTUM;
            let input_l: Vec<f64> = stim_l[span.clone()].iter().map(|&x| f64::from(x)).collect();
            let input_r: Vec<f64> = stim_r[span.clone()].iter().map(|&x| f64::from(x)).collect();
            let mut o_l = vec![0.; QUANTUM];
            let mut o_r = vec![0.; QUANTUM];
            core_obj.process(
                &mut state,
                &mut storage,
                [&input_l[..], &input_r[..]],
                [&mut o_l[..], &mut o_r[..]],
            );
            out_l[span.clone()].copy_from_slice(&o_l);
            out_r[span].copy_from_slice(&o_r);
        }
        let (want_l, want_r) = (floats(&v["outL"]), floats(&v["outR"]));
        let mut max = 0f32;
        let mut first = None;
        for i in 0..N {
            let e = (out_l[i] as f32 - want_l[i])
                .abs()
                .max((out_r[i] as f32 - want_r[i]).abs());
            if e > TOLERANCE && first.is_none() {
                first = Some(i);
            }
            max = max.max(e);
        }
        (max, first)
    }

    #[test]
    fn impulse_index28_matches_native_vector() {
        let (max, first) = check(IMPULSE);
        eprintln!(
            "replika mode-1 impulse: max abs error {max:.3e}, first frame over 1e-5: {first:?}"
        );
        assert!(
            max <= TOLERANCE,
            "max abs error {max:.3e}, first frame over tolerance {first:?}"
        );
    }

    #[test]
    fn noise_index28_matches_native_vector() {
        let (max, first) = check(NOISE);
        eprintln!(
            "replika mode-1 noise: max abs error {max:.3e}, first frame over 1e-5: {first:?}"
        );
        assert!(
            max <= TOLERANCE,
            "max abs error {max:.3e}, first frame over tolerance {first:?}"
        );
    }

    /// Index 28 = 0.3 set through `ReplikaControls::path` (the sampler-ir field the Kontakt unit maps into), the other
    /// controls at their `ReplikaControls` defaults, and the vector's per-quantum writes. The chain is still placed by
    /// the test (`Mode1::start`, no dump): the stage does not carry it across blocks yet (gaps doc, piece 3c part 5b).
    #[test]
    fn controls_index28_mode1_matches_native_impulse_vector() {
        use sampler_ir::fx::timemod::ReplikaControls;
        let v: Value = serde_json::from_str(IMPULSE).expect("vector json");
        let controls = ReplikaControls {
            path: 0.3,
            ..ReplikaControls::default()
        };
        let mut core_obj = ReplikaCore::from_controls(RATE, &controls);
        let (max, first) = run(&v, &mut core_obj);
        eprintln!(
            "replika mode-1 controls impulse: max abs error {max:.3e}, first frame over 1e-5: {first:?}"
        );
        assert!(
            max <= TOLERANCE,
            "max abs error {max:.3e}, first frame over tolerance {first:?}"
        );
    }
}
