//! Universal time-stretch / pitch-shift voice source.
//!
//! One engine serves every Kontakt "machine" source mode (Time Machine, Time
//! Machine II, Time Machine Pro) and is the place Tone/Beat Machine plug in
//! later: a [`Mode`] only selects a [`ModeConfig`] (grain size, overlap,
//! alignment search, quality). Formats translate their stored controls into
//! [`Params`] and nothing else.
//!
//! Method: windowed overlap-add with optional waveform-similarity alignment
//! (WSOLA). Two independent rates drive it, which is what makes the source
//! "independent pitch and speed":
//!
//! * `advance` - source frames the analysis position moves per output frame
//!   (time axis: speed, with the asset/output rate ratio folded in);
//! * `step` - source frames read per output frame *inside* a grain (pitch: key
//!   transposition, tune and rate ratio).
//!
//! `advance == step == 1` reproduces the source exactly: the grains are
//! periodic-Hann windows at constant overlap-add, so identity holds from the
//! first frame (pre-roll grains start before the note).
//!
//! Real-time contract: every buffer is allocated in [`Engine::new`]; `render`
//! never allocates or locks. Source access is only `ReadFrames::copy`, so
//! resident and paged (DFD) assets both work; a missing page silences that
//! grain and sets [`Engine::starved`] (the runtime counts it as an underrun).
//! [`Engine::demand`] names the source range the next block will read, for
//! the stream cache to keep resident.
//!
//! Status of the constants in [`Mode::config`]: PROVISIONAL until the
//! extraction spec (`specs/time-machine.md`, binary reader 0x140d03aa0 and the
//! Time Machine DSP) fixes the native values. Formant preservation and the
//! transient handler are plumbed through [`Params`] but not yet implemented;
//! [`Engine::implemented`] says which controls change the sound, so callers
//! can report the rest instead of pretending.

use super::ReadFrames;
use crate::Frame;
use std::ops::Range;

/// Kontakt machine source modes served by this engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mode {
    TimeMachine,
    TimeMachine2,
    TimeMachinePro,
}

/// Mode-specific shape of the shared algorithm.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModeConfig {
    /// Grain length in milliseconds (rounded to a multiple of `overlap`).
    pub grain_ms: f32,
    /// Grains overlapping at any instant (hop = grain / overlap); 2 or 4.
    pub overlap: usize,
    /// Alignment search radius as a fraction of the hop; 0 disables WSOLA.
    pub search: f32,
    /// Alignment search decimation (compare every n-th frame).
    pub search_stride: usize,
}

impl Mode {
    /// PROVISIONAL constants, see the module docs.
    pub fn config(self, hq: bool) -> ModeConfig {
        match self {
            Mode::TimeMachine => ModeConfig {
                grain_ms: 40.,
                overlap: 2,
                search: 0.,
                search_stride: 4,
            },
            Mode::TimeMachine2 => ModeConfig {
                grain_ms: 46.,
                overlap: 4,
                search: 0.5,
                search_stride: 4,
            },
            Mode::TimeMachinePro => ModeConfig {
                grain_ms: if hq { 93. } else { 46. },
                overlap: 4,
                search: 0.5,
                search_stride: if hq { 2 } else { 4 },
            },
        }
    }
}

/// Controls that change per block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Params {
    /// Analysis-position advance per output frame, in source frames; 0 freezes.
    pub advance: f64,
    /// Source frames consumed per output frame inside a grain (pitch ratio).
    pub step: f64,
    /// Kontakt "smooth" / grain smoothness, 0..=1. Plumbed; see module docs.
    pub smooth: f32,
    /// Formant shift ratio (1 = untouched). Plumbed; see module docs.
    pub formant: f32,
    /// Time Machine Pro "keep formants". Plumbed; see module docs.
    pub keep_formants: bool,
    /// Transient preservation amount, 0..=1. Plumbed; see module docs.
    pub transient: f32,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            advance: 1.,
            step: 1.,
            smooth: 0.,
            formant: 1.,
            keep_formants: false,
            transient: 0.,
        }
    }
}

/// The part of the asset the engine plays: `0..len`, optionally sustaining in
/// a loop. Past the end without a loop the engine drains and finishes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Region {
    pub len: usize,
    pub looped: Option<Range<usize>>,
}

/// Largest `step` the buffers are sized for (two octaves up).
pub const MAX_STEP: f64 = 4.;

/// The engine's own analysis clock: constant `advance` per output frame, held
/// inside an optional loop, ending at `len` without one.
#[derive(Clone, Debug, Default)]
pub struct LinearClock {
    pos: f64,
    len: usize,
    looped: Option<Range<usize>>,
}

impl LinearClock {
    fn next(&mut self, hop: usize, advance: f64) -> Option<f64> {
        if let Some(l) = &self.looped {
            let span = (l.end - l.start).max(1) as f64;
            if self.pos >= l.end as f64 {
                self.pos = l.start as f64 + (self.pos - l.start as f64) % span;
            }
        } else if self.pos >= self.len as f64 {
            return None;
        }
        let at = self.pos;
        self.pos += hop as f64 * advance;
        Some(at)
    }
}

pub struct Engine {
    cfg: ModeConfig,
    grain: usize,
    hop: usize,
    window: Box<[f32]>,
    /// Overlap-add ring, `grain` frames; consumed frames are zeroed.
    acc: Box<[Frame]>,
    scratch: Box<[Frame]>,
    head: usize,
    until_grain: usize,
    clock: LinearClock,
    /// Where the last grain's content continues in the source (WSOLA target).
    follow: Option<f64>,
    len: usize,
    params: Params,
    /// Output frames left to drain once the source ended.
    drain: Option<usize>,
    starved: bool,
}

impl Engine {
    /// Allocates every buffer the engine will ever use.
    pub fn new(mode: Mode, hq: bool, rate: u32) -> Self {
        let cfg = mode.config(hq);
        let hop = ((cfg.grain_ms * rate as f32 / 1000.) as usize / cfg.overlap).max(8);
        let grain = hop * cfg.overlap;
        let search = (cfg.search * hop as f32) as usize;
        // Window reach + interpolation guard + both search directions.
        let reach = (grain as f64 * MAX_STEP) as usize + 2 * search + 8;
        // Periodic Hann at constant overlap-add, normalised to unity gain.
        let norm = 2. / cfg.overlap as f32;
        let window = (0..grain)
            .map(|n| norm * (0.5 - 0.5 * (std::f32::consts::TAU * n as f32 / grain as f32).cos()))
            .collect();
        Self {
            cfg,
            grain,
            hop,
            window,
            acc: vec![[0.; 2]; grain].into(),
            scratch: vec![[0.; 2]; reach].into(),
            head: 0,
            until_grain: 0,
            clock: LinearClock::default(),
            follow: None,
            len: 0,
            params: Params::default(),
            drain: None,
            starved: false,
        }
    }

    /// Which [`Params`] currently change the sound.
    pub fn implemented(&self) -> &'static [&'static str] {
        &["advance", "step"]
    }

    pub fn grain(&self) -> usize {
        self.grain
    }

    /// Whether a needed source range was missing in the last `render`.
    pub fn starved(&self) -> bool {
        self.starved
    }

    pub fn done(&self) -> bool {
        self.drain == Some(0)
    }

    /// Begin playing `region` from source frame `from` on the engine's own
    /// linear clock. Allocation-free.
    pub fn start(
        &mut self,
        src: &(impl ReadFrames + ?Sized),
        region: Region,
        from: f64,
        params: Params,
    ) {
        self.clock = LinearClock { pos: from, len: region.len, looped: region.looped.clone() };
        self.start_clocked(src, region.len, from, params);
    }

    /// Begin playing an asset of `len` frames whose analysis position is
    /// supplied per grain by the caller's clock (see [`Engine::render_with`]),
    /// currently at `from`.
    pub fn start_clocked(
        &mut self,
        src: &(impl ReadFrames + ?Sized),
        len: usize,
        from: f64,
        params: Params,
    ) {
        self.acc.fill([0.; 2]);
        self.head = 0;
        self.until_grain = 0;
        self.follow = None;
        self.len = len;
        self.params = params;
        self.drain = None;
        self.starved = false;
        // Pre-roll: grains that began before the note, so the first frame is
        // already at full overlap.
        for j in (1..self.cfg.overlap).rev() {
            let skip = j * self.hop;
            self.grain_at(src, from - skip as f64 * params.advance, skip);
        }
    }

    /// Source range the next `frames` output frames will read when the
    /// analysis position is `at`.
    pub fn demand(&self, at: f64, frames: usize) -> Range<usize> {
        let grains = frames / self.hop + 2;
        let lo = at - self.search_radius() as f64 - 2.;
        let hi = at
            + (grains * self.hop) as f64 * self.params.advance
            + self.grain as f64 * self.params.step.max(0.)
            + self.search_radius() as f64
            + 4.;
        lo.max(0.) as usize..(hi.max(0.) as usize).min(self.len)
    }

    /// Fill `out` from the engine's own clock. Returns the number of frames
    /// produced, short of `out.len()` once the source ended.
    pub fn render(
        &mut self,
        src: &(impl ReadFrames + ?Sized),
        params: Params,
        out: &mut [Frame],
    ) -> usize {
        let mut clock = self.clock.clone();
        let advance = params.advance.max(0.);
        let n = self.render_with(src, params, out, |hop| clock.next(hop, advance));
        self.clock = clock;
        n
    }

    /// Fill `out`, updating the controls first. Each time a grain is due,
    /// `clock(hop)` returns the source position the grain is analysed at and
    /// moves the caller's clock `hop` output frames on; `None` ends the
    /// source. A voice cursor serves as this clock, which is how loops,
    /// direction and release stay the cursor's business.
    pub fn render_with(
        &mut self,
        src: &(impl ReadFrames + ?Sized),
        params: Params,
        out: &mut [Frame],
        mut clock: impl FnMut(usize) -> Option<f64>,
    ) -> usize {
        self.params = Params {
            step: params.step.clamp(1. / MAX_STEP, MAX_STEP),
            advance: params.advance.max(0.),
            ..params
        };
        self.starved = false;
        for (n, frame) in out.iter_mut().enumerate() {
            if self.until_grain == 0 && self.drain.is_none() {
                match clock(self.hop) {
                    Some(at) => {
                        self.grain_at(src, at, 0);
                        self.until_grain = self.hop;
                    }
                    None => self.drain = Some(self.grain - self.hop),
                }
            }
            if self.drain == Some(0) {
                return n;
            }
            *frame = std::mem::take(&mut self.acc[self.head]);
            self.head += 1;
            if self.head == self.grain {
                self.head = 0;
            }
            self.until_grain = self.until_grain.saturating_sub(1);
            if let Some(left) = &mut self.drain {
                *left -= 1;
            }
        }
        out.len()
    }

    fn search_radius(&self) -> usize {
        (self.cfg.search * self.hop as f32) as usize
    }

    /// Synthesize one grain whose nominal analysis start is `at`, writing its
    /// frames from `skip` on (earlier ones lie before the note).
    fn grain_at(&mut self, src: &(impl ReadFrames + ?Sized), at: f64, skip: usize) {
        let radius = self.search_radius();
        let step = self.params.step;
        let mut start = at;
        if radius > 0 && self.follow.is_some() && skip == 0 {
            start = at + self.best_offset(src, at, radius);
        }
        // Read what the grain spans, with the interpolation guard.
        let first = start.floor() as i64 - 1;
        let need = ((self.grain as f64 * step) as usize + 5).min(self.scratch.len());
        let ok = self.fetch(src, first, need);
        self.starved |= !ok;
        let frac = start - start.floor();
        if ok {
            let base = 1. + frac;
            for n in skip..self.grain {
                let x = base + n as f64 * step;
                let i = x as usize;
                let t = (x - i as f64) as f32;
                let (a, b, c, d) = (
                    self.scratch[i - 1],
                    self.scratch[i],
                    self.scratch[(i + 1).min(need - 1)],
                    self.scratch[(i + 2).min(need - 1)],
                );
                let w = self.window[n];
                let slot = &mut self.acc[(self.head + n - skip) % self.grain];
                for ch in 0..2 {
                    slot[ch] += w * catmull_rom(a[ch], b[ch], c[ch], d[ch], t);
                }
            }
        }
        if skip == 0 {
            self.follow = Some(start + self.hop as f64 * step);
        }
    }

    /// WSOLA: the offset in `-radius..=radius` around the nominal position
    /// `at` whose grain head best continues the previous grain (decimated
    /// normalised correlation against `follow`). Ties keep the earliest
    /// candidate, visited nearest-first, so unchanged playback stays exact.
    fn best_offset(&mut self, src: &(impl ReadFrames + ?Sized), at: f64, radius: usize) -> f64 {
        let Some(target) = self.follow else {
            return 0.;
        };
        let stride = self.cfg.search_stride;
        let len = self.hop;
        let window = len + 2 * radius;
        // scratch layout: previous grain's continuation, then the candidates.
        if len + window > self.scratch.len() {
            return 0.;
        }
        let centre = at.round() as i64;
        let ok = self.fetch_to(src, target.round() as i64, 0, len)
            & self.fetch_to(src, centre - radius as i64, len, window);
        if !ok {
            self.starved = true;
            return 0.;
        }
        let mut best = (f32::MIN, 0i64);
        for k in 0..=2 * radius as i64 {
            let delta = if k == 0 { 0 } else if k % 2 == 1 { (k + 1) / 2 } else { -(k / 2) };
            let base = len + (delta + radius as i64) as usize;
            let (mut dot, mut energy) = (0f32, 1e-9f32);
            for i in (0..len).step_by(stride) {
                let (t, c) = (self.scratch[i], self.scratch[base + i]);
                dot += t[0] * c[0] + t[1] * c[1];
                energy += c[0] * c[0] + c[1] * c[1];
            }
            let score = dot / energy.sqrt();
            if score > best.0 {
                best = (score, delta);
            }
        }
        (centre - at.round() as i64 + best.1) as f64
    }

    fn fetch(&mut self, src: &(impl ReadFrames + ?Sized), first: i64, len: usize) -> bool {
        self.fetch_to(src, first, 0, len)
    }

    /// Copy source frames `first..first+len` into `scratch[at..]`: zeros before
    /// the start or past the end, loop-wrapped when the region loops. `false`
    /// if a needed frame is not resident.
    fn fetch_to(
        &mut self,
        src: &(impl ReadFrames + ?Sized),
        first: i64,
        at: usize,
        len: usize,
    ) -> bool {
        let mut done = 0;
        let mut ok = true;
        while done < len {
            let index = first + done as i64;
            let out = &mut self.scratch[at + done..at + len];
            if index < 0 {
                let n = ((-index) as usize).min(out.len());
                out[..n].fill([0.; 2]);
                done += n;
                continue;
            }
            let index = index as usize;
            // Reads past the asset end are zeros (loops are the clock's business).
            let limit = self.len;
            if index >= limit {
                out.fill([0.; 2]);
                done = len;
                continue;
            }
            let n = (limit - index).min(out.len());
            ok &= src.copy(index..index + n, &mut out[..n]);
            if !ok {
                out[..n].fill([0.; 2]);
            }
            done += n;
        }
        ok
    }
}

#[inline]
fn catmull_rom(a: f32, b: f32, c: f32, d: f32, t: f32) -> f32 {
    b + 0.5 * t * (c - a + t * (2. * a - 5. * b + 4. * c - d + t * (3. * (b - c) + d - a)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 44100;

    fn sine(len: usize, hz: f32) -> Vec<Frame> {
        (0..len)
            .map(|n| {
                let v = (std::f32::consts::TAU * hz * n as f32 / RATE as f32).sin() * 0.5;
                [v, v]
            })
            .collect()
    }

    fn play(mode: Mode, src: &[Frame], advance: f64, step: f64, max: usize) -> Vec<Frame> {
        let mut e = Engine::new(mode, false, RATE);
        let params = Params { advance, step, ..Params::default() };
        e.start(src, Region { len: src.len(), looped: None }, 0., params);
        let mut out = Vec::new();
        let mut block = [[0.; 2]; 256];
        while out.len() < max && !e.done() {
            let n = e.render(src, params, &mut block);
            out.extend_from_slice(&block[..n]);
            if n < block.len() {
                break;
            }
        }
        out
    }

    fn crossings(x: &[Frame]) -> usize {
        x.windows(2).filter(|w| (w[0][0] < 0.) != (w[1][0] < 0.)).count()
    }

    #[test]
    fn unity_speed_and_pitch_reproduce_the_source() {
        let src = sine(20000, 441.);
        for mode in [Mode::TimeMachine, Mode::TimeMachine2, Mode::TimeMachinePro] {
            let out = play(mode, &src, 1., 1., 16000);
            assert!(out.len() >= 16000, "{mode:?} ended early");
            let worst = out[..16000]
                .iter()
                .zip(&src)
                .map(|(a, b)| (a[0] - b[0]).abs().max((a[1] - b[1]).abs()))
                .fold(0., f32::max);
            assert!(worst < 1e-3, "{mode:?} deviates by {worst}");
        }
    }

    #[test]
    fn half_speed_doubles_duration_at_the_same_pitch() {
        let src = sine(22050, 441.);
        let out = play(Mode::TimeMachine2, &src, 0.5, 1., usize::MAX);
        let grain = Engine::new(Mode::TimeMachine2, false, RATE).grain();
        assert!(
            (out.len() as i64 - (2 * src.len() + grain) as i64).abs() < 2 * grain as i64,
            "length {}",
            out.len()
        );
        // Frequency from the steady middle of the output.
        let mid = &out[8000..out.len() - 8000];
        let hz = crossings(mid) as f32 / 2. / (mid.len() as f32 / RATE as f32);
        assert!((hz - 441.).abs() < 6., "pitch {hz}");
    }

    #[test]
    fn octave_up_doubles_pitch_at_the_same_duration() {
        let src = sine(22050, 441.);
        let out = play(Mode::TimeMachine2, &src, 1., 2., usize::MAX);
        let grain = Engine::new(Mode::TimeMachine2, false, RATE).grain();
        assert!(
            (out.len() as i64 - (src.len() + grain) as i64).abs() < 2 * grain as i64,
            "length {}",
            out.len()
        );
        let mid = &out[4000..out.len() - 4000];
        let hz = crossings(mid) as f32 / 2. / (mid.len() as f32 / RATE as f32);
        assert!((hz - 882.).abs() < 12., "pitch {hz}");
    }

    #[test]
    fn a_missing_page_silences_the_grain_and_reports_starvation() {
        struct Gone;
        impl ReadFrames for Gone {
            fn frame(&self, _: usize) -> Option<Frame> {
                None
            }
            fn span(&self, _: Range<usize>) -> Option<&[Frame]> {
                None
            }
        }
        let mut e = Engine::new(Mode::TimeMachine, false, RATE);
        e.start(&Gone, Region { len: 10000, looped: None }, 0., Params::default());
        let mut block = [[1.; 2]; 64];
        assert_eq!(e.render(&Gone, Params::default(), &mut block), 64);
        assert!(e.starved());
        assert!(block.iter().flatten().all(|v| *v == 0.));
    }

    #[test]
    fn a_looped_region_sustains_past_its_length_and_frozen_time_holds() {
        let src = sine(8000, 441.);
        let mut e = Engine::new(Mode::TimeMachine2, false, RATE);
        let params = Params { advance: 0., ..Params::default() };
        e.start(&src[..], Region { len: 8000, looped: Some(2000..6000) }, 3000., params);
        let mut block = [[0.; 2]; 512];
        for _ in 0..200 {
            assert_eq!(e.render(&src[..], params, &mut block), 512);
        }
        assert!(block.iter().flatten().any(|v| v.abs() > 0.05));
        assert!(!e.starved());
    }
}
