//! Ordinary forward sampler loop state. See audits/COUNTED_LOOPS.md.

use crate::import::NativeLoop;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Row {
    start: u64,
    end: u64,
    count: i32,
    until_release: bool,
    xfade: u64,
}

/// Native physical fade spans. The two decoded streams are interpolated
/// separately before addition; this is not an output-frame blend weight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Crossfade {
    pub length: u64,
    pub begin: u64,
    pub end: u64,
}

impl Crossfade {
    fn block_gain(self, block: u64, partner: bool) -> (f32, f32) {
        let (begin, end) = if partner { (self.begin - self.length, self.end - self.length) }
            else { (self.begin, self.end) };
        if !partner && block >= end { return (0., 0.); }
        if !partner && block + 512 <= begin || partner && block >= end { return (1., 0.); }
        if partner && block + 512 < begin { return (0., 0.); }
        let step = if partner { 1. } else { -1. } / (end - begin) as f32;
        let gain = (block as i64 - begin as i64) as f32 * step
            + if partner { 0. } else { 1. };
        (gain, step)
    }
}

/// Reuse exact raw accumulation within a physical loader block. Clipping is
/// output-only: retaining a clipped value would change the subsequent fade.
#[derive(Default)]
pub(super) struct Gain {
    key: Option<(Crossfade, bool, u64)>,
    tap: u64,
    raw: f32,
    step: f32,
}

impl Gain {
    pub fn point(&mut self, fade: Crossfade, tap: u64, partner: bool) -> f32 {
        if tap == u64::MAX { return 0.; }
        let block = tap & !511;
        let key = (fade, partner, block);
        if self.key != Some(key) || tap < self.tap {
            self.key = Some(key);
            self.tap = block;
            (self.raw, self.step) = fade.block_gain(block, partner);
        }
        // Preserve each native f32 addition, including skipped source frames.
        // See 0x140829298..0x14082938c; never replace this with tap/width.
        for _ in self.tap..tap { self.raw += self.step; }
        self.tap = tap;
        self.raw.clamp(0., 1.)
    }
}

/// Prepared off-thread, retained by the bank across zone-map replacement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Rows {
    rows: [Row; 8],
    len: usize,
    pub end: u64,
    pub frames: u64,
}

impl Rows {
    pub fn prepare(saved: &[NativeLoop], frames: u64, end: u64, nonreflecting_single_pass: bool) -> Result<Self, &'static str> {
        if saved.len() > 8 || saved.iter().any(|l| l.slot >= 8)
            || saved.windows(2).any(|w| w[0].slot >= w[1].slot) {
            return Err("invalid saved loop slot order");
        }
        if frames == 0 || frames > i32::MAX as u64
            || frames < 4 && saved.iter().any(|l| l.mode != 0) {
            return Err("sample frame count outside native signed geometry");
        }
        let mut result = Self { rows: [Row::default(); 8], len: 0, end, frames };
        for l in saved.iter().filter(|l| l.mode != 0) {
            if !matches!(l.mode, 1 | 2) { return Err("One Shot or reserved loop mode"); }
            // Count one exhausts before the native direction-flip branch.
            if l.alternating && !(nonreflecting_single_pass && l.count == 1 && l.crossfade == 0) {
                return Err("counted or ordered alternating loop");
            }
            if l.tuning != 1. { return Err("nonunity loop tuning"); }
            let start = i64::from(l.start).clamp(0, frames as i64 - 4) as u64;
            let length = i64::from(l.length).clamp(0, (frames as i64 - i64::from(l.start)).max(0)).max(4) as u64;
            let loop_end = start.checked_add(length).ok_or("loop geometry overflow")?;
            if loop_end > end { return Err("loop end beyond cropped sample end"); }
            let xfade = i64::from(l.crossfade).clamp(0, (length - 1).min(start) as i64) as u64;
            result.rows[result.len] = Row { start, end: loop_end, count:l.count, until_release:l.mode == 2, xfade };
            result.len += 1;
        }
        Ok(result)
    }

    pub fn linear(&self) -> bool { self.len == 0 }

    pub fn crossfaded(&self) -> bool { self.rows[..self.len].iter().any(|row| row.xfade != 0) }

    pub fn terminal(&self) -> u64 { self.end }
}

/// Scalar voice state; the source clock remains the existing 32.32 clock.
#[derive(Clone, Copy, Debug)]
pub(super) struct Cursor {
    pub rows: u32,
    pub position: f64,
    index: usize,
    len: usize,
    remaining: i32,
    looped: bool,
    released: bool,
}

impl Cursor {
    pub fn new(rows: u32, data: &Rows, position: f64, released: bool) -> Self {
        let mut cursor = Self { rows, position, index:0, len:data.len, remaining:0, looped:false, released:false };
        cursor.select(data);
        if released { cursor.release(data); }
        cursor
    }

    fn select(&mut self, data: &Rows) {
        self.looped = false;
        while self.index < data.len {
            let row = data.rows[self.index];
            if row.start < data.end && self.position.floor() < row.end as f64
                && !(self.released && row.until_release) {
                self.remaining = row.count;
                return;
            }
            self.index += 1;
        }
        self.remaining = 0;
    }

    /// End crossings consume the first partial pass too. Batch wraps so work
    /// is bounded by eight row transitions, even at extreme source steps.
    pub fn advance(&mut self, data: &Rows, travel: f64) -> Option<()> {
        let mut next = *self;
        next.travel(travel)?;
        while next.index < data.len {
            let row = data.rows[next.index];
            if next.position.floor() < row.end as f64 { break; }
            let length = row.end - row.start;
            let crossings = (next.position.floor() as u64 - row.end) / length + 1;
            next.looped = true;
            if next.remaining <= 0 || crossings < next.remaining as u64 {
                next.position -= (crossings * length) as f64;
                if next.remaining > 0 { next.remaining -= crossings as i32; }
                break;
            }
            // Exhaustion selects from the overshot physical position, without
            // rewinding it for the final crossing.
            next.position -= ((next.remaining - 1) as u64 * length) as f64;
            next.index += 1;
            next.select(data);
        }
        *self = next;
        Some(())
    }

    /// The producer returns at a host-fragment end before consuming its last
    /// crossing. A same-boundary release therefore precedes normalization.
    pub fn travel(&mut self, frames: f64) -> Option<()> {
        let position = self.position + frames;
        if !frames.is_finite() || frames < 0. || !position.is_finite() || position >= (1u64 << 53) as f64 { return None; }
        self.position = position;
        Some(())
    }

    pub fn release(&mut self, data: &Rows) {
        self.released = true;
        if self.index < data.len && data.rows[self.index].until_release { self.remaining = 1; }
    }

    pub fn done(&self, data: &Rows) -> bool { self.index == data.len && self.position >= data.end as f64 }

    pub fn unlimited(&self, data: &Rows) -> bool { self.index < data.len && self.remaining <= 0 }

    pub fn active(&self) -> bool { self.index < self.len }

    /// Current physical row, with native left/right support permissions.
    pub fn domain(&self, data: &Rows) -> Option<(u64, u64, bool, bool)> {
        (self.index < data.len).then(|| {
            let row = data.rows[self.index];
            (row.start, row.end, self.looped, self.remaining >= 0 && self.remaining != 1)
        })
    }

    pub fn crossfade(&self, data: &Rows) -> Option<Crossfade> {
        let row = data.rows.get(self.index).filter(|_| self.index < data.len)?;
        (row.xfade != 0).then_some(Crossfade { length: row.end - row.start, begin: row.end - row.xfade, end: row.end })
    }

    /// Eight raw physical taps for the admitted integral-phase crossfade.
    /// The renderer validates every phase and endpoint before using these.
    pub fn crossfade_taps(&self, data: &Rows) -> [u64; 8] {
        let Some(fade) = self.crossfade(data) else {
            let own = self.taps(data);
            return std::array::from_fn(|i| if i < 4 { own[i] } else { u64::MAX });
        };
        let first = self.position.floor() as i64 - 1;
        std::array::from_fn(|i| {
            let tap = first + (i % 4) as i64 - if i >= 4 { fade.length as i64 } else { 0 };
            if i >= 4 && self.position < fade.length as f64 || i < 4 && tap >= fade.end as i64
                || tap < 0 || tap as u64 >= data.frames { u64::MAX } else { tap as u64 }
        })
    }

    pub fn taps(&self, data: &Rows) -> [u64; 4] {
        if self.done(data) { return [u64::MAX; 4]; }
        let first = self.position.floor() as i64 - 1;
        std::array::from_fn(|i| {
            let mut tap = first + i as i64;
            if self.index < data.len {
                let row = data.rows[self.index];
                if (self.looped && tap < row.start as i64)
                    || (self.remaining >= 0 && self.remaining != 1 && tap >= row.end as i64) {
                    tap = row.start as i64 + (tap - row.start as i64).rem_euclid((row.end - row.start) as i64);
                }
            }
            if tap < 0 || tap as u64 >= data.frames { u64::MAX } else { tap as u64 }
        })
    }
}
