//! Kontakt Surround Panner speaker law (BEffectSurroundPanner, version 0x70). Native: table init
//! `0x140a9cb40`, periodic sine lookup `0x140ac3230`, source position `0x140a985b0`, source-to-speaker
//! distance `0x140a94180`, angular weight `0x140a93cb0`, gain law `0x140a94580`.
//! Verified against Kontakt 8 kemu vectors (`surround_panner_native_vectors.json`,
//! `tools/re/surround_panner_kemu_vectors.py`) at 1e-5 of peak.
//!
//! Not modelled here: the per-block routing driver (`0x140ab0070`, `0x140ab4eb0`), the LFE
//! sub-object, the shared host mix (`0x140ac21f0`), and the output channel layouts. Speakers are
//! given as plane positions in [0, 1]^2; the native zero-index sentinel (-1) is not used.
use std::f64::consts::{FRAC_PI_2, PI, TAU};

/// Entries in each table (native index 0..=1024).
pub const TABLE_LEN: usize = 1025;
/// Native speaker-plane scale of the source radius (`+0x348` multiplies the centred position).
const SPEAKER_SCALE: f32 = 0.1;
/// Native distance scale when the flag is set (`0x144fc3e50`).
const DISTANCE_SCALE: f32 = 1.178_511_26;
/// Native minimum distance clamp in the gain law (`0x1447148b4`).
const MIN_DISTANCE: f32 = 0.01;
/// Native threshold below which the source is treated as centred (`0x1447148a0`).
const CENTRE_EPS: f32 = 1e-5;
/// Native threshold for the weight direction (`0x14471489c`).
const DIRECTION_EPS: f32 = 1e-6;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Speaker {
    pub pos: Point,
    /// Native byte at `+0x1c8 + 12 * i`: 1 excludes the speaker from the law.
    pub disabled: bool,
}

/// Native gain law selector (`+0x394`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GainMode {
    /// Normalised power law over the enabled speakers (`(w / d^2)^p / sum`).
    Power,
    /// Cosine taper of `d * p * 2pi`, zero past `pi/2`.
    Cosine,
    /// `2^(-8 d p)`.
    Exp2,
}

/// The two 1025-entry tables: `sin` is the quarter-wave sine at step `2pi/4096` (`0x14a611750`),
/// `asin` is `asin(-1 + j * 2/1024)` (`0x14a612760`). Both are filled by the native init
/// `FUN_140a9cb40` with a running double accumulator, reproduced here.
pub struct Tables {
    sin: Box<[f32; TABLE_LEN]>,
    asin: Box<[f32; TABLE_LEN]>,
}

impl Tables {
    pub fn new() -> Self {
        let mut sin = Box::new([0f32; TABLE_LEN]);
        let mut a = 0f64;
        for v in sin.iter_mut() {
            *v = a.sin() as f32;
            a += 0.001_533_980_787_885_641_2;
        }
        let mut asin = Box::new([0f32; TABLE_LEN]);
        let mut a = -1f64;
        for v in asin.iter_mut() {
            *v = a.asin() as f32;
            a += 0.001_953_125;
        }
        Self { sin, asin }
    }

    /// Periodic sine (`0x140ac3230`): reduces to [0, pi/2] and reads the quarter-wave table.
    pub fn sin(&self, x: f32) -> f32 {
        let mut x = x;
        let mut sign = 1.0f32;
        if x >= 1e-7 || x <= -1e-7 {
            while x as f64 > TAU {
                x = (x as f64 - TAU) as f32;
            }
            while x < 0.0 {
                x = (x as f64 + TAU) as f32;
            }
            if x as f64 >= PI {
                x = (x as f64 - PI) as f32;
                sign = -1.0;
            }
            if x as f64 >= FRAC_PI_2 {
                x = (PI - x as f64) as f32;
            }
        } else {
            x = 0.0;
        }
        let idx = ((x as f64 * (2.0 / PI)) as f32 * 1024.0 + 0.5) as i32;
        sign * self.sin[idx.clamp(0, 1024) as usize]
    }

    /// Table index of `c` in [-1, 1] on the asin grid (`(c/2 + 1/2) * 1024 + 1/2`, clamped).
    fn index(c: f32) -> usize {
        ((c * 0.5 + 0.5) * 1024.0 + 0.5) as i32 as usize
    }

    fn asin_at(&self, c: f32) -> f32 {
        self.asin[Self::index(c).min(1024)]
    }
}

impl Default for Tables {
    fn default() -> Self {
        Self::new()
    }
}

/// Panner state the law reads (the native object fields, in plain form).
pub struct Panner {
    /// `+0x280`: speakers in use.
    pub speakers: Vec<Speaker>,
    /// `+0x284 + 12 * i`: source positions.
    pub sources: Vec<Point>,
    /// `+0x348`.
    pub scale: f32,
    /// `+0x34c`: power-law exponent.
    pub exponent: f32,
    /// `+0x350`, `+0x354`: output offset.
    pub offset: Point,
    /// `+0x358`: radius offset.
    pub radius_offset: f32,
    /// `+0x35c`: angle offset.
    pub angle_offset: f32,
    /// `+0x394`.
    pub mode: GainMode,
    pub tables: Tables,
}

fn clamp01(v: f32) -> f32 {
    if v < 0.0 {
        0.0
    } else if v > 1.0 {
        1.0
    } else {
        v
    }
}

impl Panner {
    pub fn new(speakers: Vec<Speaker>, sources: Vec<Point>, mode: GainMode) -> Self {
        Self {
            speakers,
            sources,
            scale: 1.0,
            exponent: 1.0,
            offset: Point::default(),
            radius_offset: 0.0,
            angle_offset: 0.0,
            mode,
            tables: Tables::new(),
        }
    }

    /// Source position on the speaker plane (`0x140a985b0`): mirrored quadrant angle, radius plus
    /// offset, lifted by the angle offset, clamped to [0, 1].
    pub fn source_position(&self, src: usize) -> Point {
        let s = self.sources[src];
        let ax = (s.x - 0.5).abs();
        let ay = (s.y - 0.5).abs();
        let r = (ay * ay + ax * ax).sqrt();
        let c = if r >= CENTRE_EPS { ax / r } else { 1.0 };
        // Quadrant angle from the centre: acos(c), mirrored into the quadrant of the source.
        let mut theta = (FRAC_PI_2 - self.tables.asin_at(c) as f64) as f32;
        if s.x < 0.5 {
            theta = (PI - theta as f64) as f32;
        }
        if s.y < 0.5 {
            theta = (TAU - theta as f64) as f32;
        }
        let rad = r + self.radius_offset;
        let phi = ((theta as f64 * (1.0 / PI) + self.angle_offset as f64) * PI) as f32;
        let x = 0.5 + rad * self.tables.sin((phi as f64 + FRAC_PI_2) as f32);
        let y = 0.5 + rad * self.tables.sin(phi);
        Point {
            x: clamp01(x + self.offset.x),
            y: clamp01(y + self.offset.y),
        }
    }

    /// Scaled source point used by the distance (`0x140a94180`).
    fn scaled_source(&self, src: usize) -> Point {
        let p = self.source_position(src);
        Point {
            x: (p.x - 0.5) * self.scale + 0.5,
            y: (p.y - 0.5) * self.scale + 0.5,
        }
    }

    fn scaled_speaker(p: Point) -> Point {
        Point {
            x: (p.x - 0.5) * SPEAKER_SCALE + 0.5,
            y: (p.y - 0.5) * SPEAKER_SCALE + 0.5,
        }
    }

    /// Distance from the mapped source to the scaled speaker (`0x140a94180`), times the native
    /// 1.1785 when `scaled` is set.
    pub fn distance(&self, src: usize, spk: usize, scaled: bool) -> f32 {
        let a = self.scaled_source(src);
        let b = Self::scaled_speaker(self.speakers[spk].pos);
        let d = ((a.y - b.y) * (a.y - b.y) + (a.x - b.x) * (a.x - b.x)).sqrt();
        if scaled {
            d * DISTANCE_SCALE
        } else {
            d
        }
    }

    /// Angular weight of one speaker (`0x140a93cb0`): zero when disabled, else the squared
    /// cosine of the direction taper.
    pub fn weight(&self, src: usize, spk: usize) -> f32 {
        let sp = self.speakers[spk];
        if sp.disabled {
            return 0.0;
        }
        let s = self.scale * 0.707_106_77;
        let q = Self::scaled_speaker(sp.pos);
        let radius =
            ((0.5 - q.y) * (0.5 - q.y) + (0.5 - q.x) * (0.5 - q.x)).sqrt() * DISTANCE_SCALE;
        let source = self.sources[src];
        let dx = source.x - sp.pos.x;
        let dy = source.y - sp.pos.y;
        let mut wf = (radius - s) / (radius + s);
        if wf <= 0.0 {
            wf = 0.0;
        }
        let len = (dy * dy + dx * dx).sqrt();
        let c = if len <= DIRECTION_EPS { 0.0 } else { dx / len };
        let theta = if dx >= 0.0 {
            let a = self.tables.asin_at(c);
            if dy >= 0.0 {
                (a as f64 + PI) as f32
            } else {
                (TAU - a as f64) as f32
            }
        } else {
            let a = self.tables.asin_at(-c);
            if dy < 0.0 {
                a
            } else {
                (PI - a as f64) as f32
            }
        };
        let w = self
            .tables
            .sin((theta as f64 * 0.5 * wf as f64 + FRAC_PI_2) as f32);
        w * w
    }

    /// Gain of one source into one speaker (`0x140a94580`).
    pub fn gain(&self, src: usize, spk: usize) -> f32 {
        match self.mode {
            GainMode::Power => {
                let p = self.exponent;
                let d = self.distance(src, spk, true).max(MIN_DISTANCE);
                let num = (self.weight(src, spk) / (d * d)).powf(p);
                let mut sum = 0f32;
                for (i, sp) in self.speakers.iter().enumerate() {
                    if sp.disabled {
                        continue;
                    }
                    let di = self.distance(src, i, true).max(MIN_DISTANCE);
                    sum += (self.weight(src, i) / (di * di)).powf(p);
                }
                num / sum
            }
            GainMode::Cosine => {
                let x =
                    (self.distance(src, spk, true) * self.exponent * 6.283_185_482_025_146) as f64;
                if x <= FRAC_PI_2 {
                    self.tables.sin((x + FRAC_PI_2) as f32)
                } else {
                    0.0
                }
            }
            GainMode::Exp2 => {
                let x = (self.distance(src, spk, true) * self.exponent * -8.0) as f64;
                2f64.powf(x) as f32
            }
        }
    }
}

/// One-pole coefficient of the panned path's gain smoother (native, kemu `dc_step`): every sample
/// the gain moves `1/1800` of the way to its block target. Only measured at the driver's default
/// rate; the driver never sets a rate, so the rate dependence is still open.
pub const GAIN_SMOOTH: f32 = 1.0 / 1800.0;

/// Smoothed gain per (source, output) pair. The first block starts each pair at its target.
pub struct GainSmoother {
    gain: Vec<Vec<Option<f32>>>,
}

impl GainSmoother {
    pub fn new(sources: usize, outputs: usize) -> Self {
        Self {
            gain: vec![vec![None; outputs]; sources],
        }
    }

    /// Renders one route: each sample emits `gain * input` (overwriting `out` when `overwrite`), then
    /// steps the gain toward `target`.
    pub fn render(
        &mut self,
        s: usize,
        k: usize,
        target: f32,
        overwrite: bool,
        input: &[f32],
        out: &mut [f32],
    ) {
        let gain = self.gain[s][k].get_or_insert(target);
        for (o, i) in out.iter_mut().zip(input) {
            let v = *gain * i;
            *o = if overwrite { v } else { *o + v };
            *gain += GAIN_SMOOTH * (target - *gain);
        }
    }
}

/// Route of one output in the block driver (`0x140ab0070`, per-output flag at record `+0x1c8 + 12k`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Route {
    /// Flag 1: scalar level (`+0xc00`) and no panning. The LFE sub-object (vtable at `+0x7a0`, not
    /// modelled) processes the buffer afterwards.
    Lfe { level: f32 },
    /// Flag 0: positional, rendered by the panned path (`0x140ab4eb0`, not ported yet).
    Panned,
}

/// Block driver routing (`0x140ab0070`, scalar fast path). `inputs[s]` is source `s` for one block,
/// `outs[k]` is output `k`. For each output the first source to reach it overwrites it and the later
/// sources sum into it, as the native per-output bit does. `panned(s, k, overwrite, input, out)`
/// renders a positional route; the native call passes `overwrite` as its mode argument.
pub fn drive_block(
    inputs: &[&[f32]],
    routes: &[Route],
    outs: &mut [&mut [f32]],
    mut panned: impl FnMut(usize, usize, bool, &[f32], &mut [f32]),
) {
    let mut written = vec![false; routes.len()];
    for (s, input) in inputs.iter().enumerate() {
        for (k, route) in routes.iter().enumerate() {
            let overwrite = !written[k];
            written[k] = true;
            let out = &mut *outs[k];
            match *route {
                Route::Lfe { level } => {
                    for (o, i) in out.iter_mut().zip(input.iter()) {
                        *o = if overwrite { i * level } else { *o + i * level };
                    }
                }
                Route::Panned => panned(s, k, overwrite, input, out),
            }
        }
    }
}

#[cfg(test)]
mod driver_tests {
    use super::*;

    #[test]
    fn first_source_overwrites_later_sources_sum() {
        let a = [1.0f32, 2.0, 3.0, 4.0];
        let b = [0.5f32, 0.5, 0.5, 0.5];
        let mut out = [9.0f32; 4];
        let mut panned_calls = Vec::new();
        drive_block(
            &[&a, &b],
            &[Route::Lfe { level: 2.0 }, Route::Panned],
            &mut [&mut out[..], &mut [0.0f32; 4][..]],
            |s, k, overwrite, _, _| panned_calls.push((s, k, overwrite)),
        );
        assert_eq!(
            out,
            [3.0, 5.0, 7.0, 9.0],
            "LFE: a*2 overwrites, then b*2 sums"
        );
        assert_eq!(
            panned_calls,
            [(0, 1, true), (1, 1, false)],
            "panned route: overwrite on the first source only"
        );
    }

    #[test]
    fn lfe_output_never_reaches_the_panned_path() {
        let a = [1.0f32; 3];
        let mut out = [0.0f32; 3];
        let mut called = false;
        drive_block(
            &[&a],
            &[Route::Lfe { level: 0.25 }],
            &mut [&mut out[..]],
            |_, _, _, _, _| called = true,
        );
        assert!(!called);
        assert_eq!(out, [0.25; 3]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Deserialize)]
    struct Layout {
        name: String,
        source: [f32; 2],
        speakers: Vec<(f32, f32, bool)>,
        off: [f32; 2],
        radius_off: f32,
        angle_off: f32,
    }

    #[derive(serde::Deserialize)]
    struct Case {
        layout: String,
        scale: f32,
        p: f32,
        mode: u32,
        gain: Vec<f32>,
        dist: Vec<f32>,
        weight: Vec<f32>,
        pos: [f32; 2],
    }

    #[derive(serde::Deserialize)]
    struct Vectors {
        sin_table: Vec<f32>,
        asin_table: Vec<f32>,
        lookup: Vec<[f32; 2]>,
        cases: Vec<Case>,
        layouts: Vec<Layout>,
    }

    fn assert_close(name: &str, got: &[f32], want: &[f32]) {
        let peak = want.iter().fold(1e-30f32, |m, v| m.max(v.abs()));
        let worst = got
            .iter()
            .zip(want)
            .fold(0f32, |m, (g, w)| m.max((g - w).abs()));
        assert!(
            worst <= 1e-5 * peak.max(1.0),
            "{name}: worst {worst} vs peak {peak}"
        );
    }

    #[test]
    fn tables_match_native_init() {
        let v: Vectors =
            serde_json::from_str(include_str!("surround_panner_native_vectors.json")).unwrap();
        let t = Tables::new();
        assert_close("sin table", &t.sin[..], &v.sin_table);
        assert_close("asin table", &t.asin[..], &v.asin_table);
    }

    #[test]
    fn matches_native_kemu_vectors() {
        let v: Vectors =
            serde_json::from_str(include_str!("surround_panner_native_vectors.json")).unwrap();
        let t = Tables::new();
        let (mut got, mut want) = (Vec::new(), Vec::new());
        for [x, y] in &v.lookup {
            got.push(t.sin(*x));
            want.push(*y);
        }
        assert_close("lookup", &got, &want);
        for case in &v.cases {
            let lay = v.layouts.iter().find(|l| l.name == case.layout).unwrap();
            let speakers = lay
                .speakers
                .iter()
                .map(|&(x, y, disabled)| Speaker {
                    pos: Point { x, y },
                    disabled,
                })
                .collect();
            let mode = match case.mode {
                0 => GainMode::Power,
                1 => GainMode::Cosine,
                _ => GainMode::Exp2,
            };
            let mut p = Panner::new(
                speakers,
                vec![Point {
                    x: lay.source[0],
                    y: lay.source[1],
                }],
                mode,
            );
            p.scale = case.scale;
            p.exponent = case.p;
            p.offset = Point {
                x: lay.off[0],
                y: lay.off[1],
            };
            p.radius_offset = lay.radius_off;
            p.angle_offset = lay.angle_off;
            let tag = format!(
                "{} scale {} p {} mode {}",
                case.layout, case.scale, case.p, case.mode
            );
            let pos = p.source_position(0);
            assert_close(&format!("{tag} pos"), &[pos.x, pos.y], &case.pos);
            let dist: Vec<f32> = (0..p.speakers.len())
                .map(|i| p.distance(0, i, true))
                .collect();
            assert_close(&format!("{tag} dist"), &dist, &case.dist);
            let weight: Vec<f32> = (0..p.speakers.len()).map(|i| p.weight(0, i)).collect();
            assert_close(&format!("{tag} weight"), &weight, &case.weight);
            let gain: Vec<f32> = (0..p.speakers.len()).map(|i| p.gain(0, i)).collect();
            assert_close(&format!("{tag} gain"), &gain, &case.gain);
        }
    }
}

#[cfg(test)]
mod native_driver_tests {
    //! Block driver against Kontakt 8 kemu output (`tools/re/surround_panner_driver_kemu.py`): fixed
    //! positions, moving source, distance (scale) steps, a mono source, and a 40-block DC scale step.
    use super::*;

    #[derive(serde::Deserialize)]
    struct Block {
        sources: Vec<[f32; 2]>,
        scale: f32,
        input: Vec<Vec<f32>>,
        out: Vec<Vec<f32>>,
    }

    #[derive(serde::Deserialize)]
    struct Case {
        name: String,
        n: usize,
        speakers: Vec<[f32; 2]>,
        blocks: Vec<Block>,
    }

    #[derive(serde::Deserialize)]
    struct Fixture {
        cases: Vec<Case>,
    }

    #[test]
    fn panned_route_matches_native_kemu_blocks() {
        let v: Fixture =
            serde_json::from_str(include_str!("surround_panner_driver_native_vectors.json"))
                .unwrap();
        for case in &v.cases {
            let speakers: Vec<Speaker> = case
                .speakers
                .iter()
                .map(|&[x, y]| Speaker {
                    pos: Point { x, y },
                    disabled: false,
                })
                .collect();
            let outputs = speakers.len();
            let mut smoother = GainSmoother::new(case.blocks[0].sources.len(), outputs);
            for (b, block) in case.blocks.iter().enumerate() {
                let sources: Vec<Point> =
                    block.sources.iter().map(|&[x, y]| Point { x, y }).collect();
                let mut panner = Panner::new(speakers.clone(), sources, GainMode::Power);
                panner.scale = block.scale;
                let gains: Vec<Vec<f32>> = (0..block.sources.len())
                    .map(|s| (0..outputs).map(|k| panner.gain(s, k)).collect())
                    .collect();
                let inputs: Vec<&[f32]> = block.input.iter().map(|x| x.as_slice()).collect();
                let mut outs = vec![vec![0.0f32; case.n]; outputs];
                let mut out_refs: Vec<&mut [f32]> =
                    outs.iter_mut().map(|o| o.as_mut_slice()).collect();
                let routes = vec![Route::Panned; outputs];
                drive_block(
                    &inputs,
                    &routes,
                    &mut out_refs,
                    |s, k, overwrite, input, out| {
                        smoother.render(s, k, gains[s][k], overwrite, input, out);
                    },
                );
                for (k, got) in outs.iter().enumerate() {
                    let worst = got
                        .iter()
                        .zip(&block.out[k])
                        .fold(0f32, |m, (g, w)| m.max((g - w).abs()));
                    assert!(
                        worst <= 1e-5,
                        "{} block {b} output {k}: worst {worst}",
                        case.name
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod native_record_tests {
    //! Record-driven native run (`tools/re/surround_record_kemu.py`): reader, then the case-6 apply,
    //! then the block driver. Parameters are the values the apply wrote to the object; each block
    //! carries them so a per-block SIZE step is covered too.
    use super::*;

    #[derive(serde::Deserialize)]
    struct Block {
        sources: Vec<[f32; 2]>,
        input: Vec<Vec<f32>>,
        out: Vec<Vec<f32>>,
        scale: f32,
        exponent: f32,
        offset: [f32; 2],
        radius_offset: f32,
        angle_offset: f32,
        mode: u32,
    }

    #[derive(serde::Deserialize)]
    struct Case {
        name: String,
        n: usize,
        speakers: Vec<[f32; 2]>,
        blocks: Vec<Block>,
    }

    #[derive(serde::Deserialize)]
    struct Fixture {
        cases: Vec<Case>,
    }

    fn native_mode(m: u32) -> GainMode {
        match m {
            0 => GainMode::Power,
            1 => GainMode::Cosine,
            _ => GainMode::Exp2,
        }
    }

    /// Known gap: the record apply (setter `0x1409685e0`) leaves main-output gain state that the
    /// Rust model does not carry (base case native = Rust * pi/2; rate and LFE dB dependent). Run
    /// with `--ignored` to see the mismatch list; remove the ignore when the state is modelled.
    #[test]
    fn record_apply_matches_native_blocks() {
        let v: Fixture =
            serde_json::from_str(include_str!("surround_record_native_vectors.json")).unwrap();
        let mut failed = Vec::new();
        for case in &v.cases {
            let speakers: Vec<Speaker> = case
                .speakers
                .iter()
                .map(|&[x, y]| Speaker {
                    pos: Point { x, y },
                    disabled: false,
                })
                .collect();
            let outputs = speakers.len();
            let mut smoother = GainSmoother::new(case.blocks[0].sources.len(), outputs);
            for (b, block) in case.blocks.iter().enumerate() {
                let sources: Vec<Point> =
                    block.sources.iter().map(|&[x, y]| Point { x, y }).collect();
                let mut panner = Panner::new(speakers.clone(), sources, native_mode(block.mode));
                panner.scale = block.scale;
                panner.exponent = block.exponent;
                panner.offset = Point {
                    x: block.offset[0],
                    y: block.offset[1],
                };
                panner.radius_offset = block.radius_offset;
                panner.angle_offset = block.angle_offset;
                let gains: Vec<Vec<f32>> = (0..block.sources.len())
                    .map(|s| (0..outputs).map(|k| panner.gain(s, k)).collect())
                    .collect();
                let inputs: Vec<&[f32]> = block.input.iter().map(|x| x.as_slice()).collect();
                let mut outs = vec![vec![0.0f32; case.n]; outputs];
                let mut out_refs: Vec<&mut [f32]> =
                    outs.iter_mut().map(|o| o.as_mut_slice()).collect();
                let routes = vec![Route::Panned; outputs];
                drive_block(
                    &inputs,
                    &routes,
                    &mut out_refs,
                    |s, k, overwrite, input, out| {
                        smoother.render(s, k, gains[s][k], overwrite, input, out);
                    },
                );
                for (k, got) in outs.iter().enumerate() {
                    let worst = got
                        .iter()
                        .zip(&block.out[k])
                        .fold(0f32, |m, (g, w)| m.max((g - w).abs()));
                    if worst > 1e-5 {
                        failed.push(format!("{} block {b} output {k}: worst {worst}", case.name));
                    }
                }
            }
        }
        assert!(failed.is_empty(), "{}", failed.join("\n"));
    }
}
