//! Step-pattern arpeggiator: a 128-entry step table drives onsets, gates,
//! velocities, offsets and controller events over an ordered list of the held
//! keys. Configuration of the arpeggiator block for formats whose arpeggiator
//! is a pattern sequencer (UVI Workstation 4.0.9, `Arpeggiator`).
//!
//! Laws are the traced ones from UVI Workstation 4.0.9: scheduler 0x141110c90,
//! pending note-off drain 0x141112000, ordering 0x1411121e0, input 0x14110f880.
//! Fixed capacity, no allocation. Not provided because unproved: modes 24, 26
//! and 27 (sort key and weighted draw unrecovered), Song Position trigger,
//! recording, bypass pass-through, and the host's pseudo-random stream (the
//! random modes use [`crate::blocks::lfo::hash`], so only the distribution
//! matches).

use super::lfo::hash;

pub const PATTERN_STEPS: usize = 128;
/// Ordered-list capacity; longer lists are truncated.
const CAP: usize = 2048;
const PENDING: usize = 512;

/// One ordered entry: `source` is the held key whose velocity applies, `key`
/// the pitch (the native pair's two words).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pair {
    pub source: u8,
    pub key: i32,
}

/// One step (native record at 0x14 bytes: state, size, level, offset, CC).
/// State 0 rests, 1 and 3 play (3 rounds its gate up and drains in its own
/// pass), 2 extends the preceding note's gate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PatternStep {
    pub state: u8,
    pub size: f32,
    pub level: f32,
    pub offset: i32,
    pub cc: i32,
}

impl Default for PatternStep {
    fn default() -> Self {
        Self { state: 0, size: 1.0, level: 1.0, offset: 0, cc: 0 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatternTrigger {
    /// A new note restarts the pattern at step 0.
    Note,
    /// New notes join the running pattern.
    Legato,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PatternArp {
    /// Ordering mode 0..=23 or 25.
    pub mode: u8,
    pub octave: i32,
    /// 0 repeats the ordered pattern per octave, 1 expands the keys first.
    pub octave_mode: u8,
    pub num_strike: u32,
    pub repeat_bottom: bool,
    pub repeat_top: bool,
    /// Step length in quarter notes.
    pub resolution: f64,
    /// Multiplies every gate, not the onset interval.
    pub step_length: f32,
    /// Swing amount 0..=1.
    pub groove: f64,
    /// 0 plays the step levels, 1 the source velocities.
    pub velocity_blend: f32,
    pub num_steps: u32,
    /// Controller number for the step CC values; negative disables.
    pub step_cc_id: i32,
    pub trigger: PatternTrigger,
    /// Released keys stay in the pattern: pass to [`PatternArpState::note_off`].
    pub hold: bool,
    pub bypass: bool,
    pub steps: [PatternStep; PATTERN_STEPS],
}

impl PatternArp {
    pub fn valid(&self) -> bool {
        (self.mode <= 23 || self.mode == 25)
            && self.octave_mode <= 1
            && self.num_strike >= 1
            && (1..=PATTERN_STEPS as u32).contains(&self.num_steps)
            && self.resolution.is_finite()
            && self.resolution > 0.0
            && (0.0..=1.0).contains(&self.groove)
            && self.steps.iter().all(|s| s.state <= 3)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatternEvent {
    On { key: i32, velocity: i32, at: u32 },
    Off { key: i32, at: u32 },
    Cc { controller: i32, value: i32, at: u32 },
}

#[derive(Clone, Copy, Debug, Default)]
struct Pending {
    key: i32,
    remaining: u32,
    /// Gate came from a state-3 step: drained in the second pass.
    late: bool,
}

struct Buf {
    items: [Pair; CAP],
    len: usize,
}

impl Buf {
    fn new() -> Self {
        Self { items: [Pair::default(); CAP], len: 0 }
    }
    fn clear(&mut self) {
        self.len = 0;
    }
    fn push(&mut self, p: Pair) {
        if self.len < CAP {
            self.items[self.len] = p;
            self.len += 1;
        }
    }
    fn head_dup(&mut self) {
        if self.len > 0 && self.len < CAP {
            let first = self.items[0];
            self.items.copy_within(0..self.len, 1);
            self.items[0] = first;
            self.len += 1;
        }
    }
    fn tail_dup(&mut self) {
        if let Some(&last) = self.slice().last() {
            self.push(last);
        }
    }
    fn append_head(&mut self) {
        if self.len > 0 {
            self.push(self.items[0]);
        }
    }
    fn slice(&self) -> &[Pair] {
        &self.items[..self.len]
    }
    fn slice_mut(&mut self) -> &mut [Pair] {
        &mut self.items[..self.len]
    }
}

fn draw(rng: &mut u64, seed: u64) -> u64 {
    *rng += 1;
    hash(seed, 0x5eed, *rng)
}

fn shuffle(q: &mut [Pair], rng: &mut u64, seed: u64) {
    for i in (1..q.len()).rev() {
        q.swap(i, (draw(rng, seed) % (i as u64 + 1)) as usize);
    }
}

/// Fold of the pair list: low, high, next low, next high, ... (0x141113221).
fn fold(p: &[Pair], q: &mut Buf) {
    let (mut lo, mut hi) = (0usize, p.len());
    for i in 0..p.len() {
        if i % 2 == 0 {
            q.push(p[lo]);
            lo += 1;
        } else {
            hi -= 1;
            q.push(p[hi]);
        }
    }
}

fn shifted(key: i32, octaves: i32) -> i32 {
    (key + 12 * octaves).clamp(0, 127)
}

/// Builds the ordered list `q` from the held pairs `held` (ascending key).
/// Order of passes follows the native dispatcher: key expansion (octave mode
/// 1, 0x1411128e6), mode switch, per-pair octave blocks (octave mode 0,
/// 0x1411144b7), shuffles, mode 25 grouping (0x1411146b8), strike (0x141114803).
fn build(cfg: &PatternArp, held: &[Pair], prior: Option<i32>, rng: &mut u64, seed: u64, p: &mut Buf, q: &mut Buf) {
    p.clear();
    q.clear();
    for &h in held {
        p.push(h);
    }
    let sign = cfg.octave.signum();
    let blocks = cfg.octave.unsigned_abs() as i32;
    if cfg.octave_mode == 1 {
        let original = p.len;
        for i in 0..original {
            let h = p.items[i];
            for j in 1..=blocks {
                let key = shifted(i32::from(h.source), sign * j);
                if !p.slice().iter().any(|x| x.key == key) {
                    p.push(Pair { source: h.source, key });
                }
            }
        }
        p.slice_mut().sort_unstable_by_key(|x| x.key);
    }
    let n = p.len;
    if n == 0 {
        return;
    }
    let (b, t) = (cfg.repeat_bottom, cfg.repeat_top);
    let at = |i: usize| p.items[i];
    // 0x141e514f0: index permutation (0, 2, 1, 3) inside groups of four.
    let a = |i: usize| ([0usize, 2, 1, 3][i % 4] + 4 * (i / 4)).min(n - 1);
    let flags = |q: &mut Buf, head: bool, tail: bool| {
        if head {
            q.head_dup();
        }
        if tail {
            q.tail_dup();
        }
    };
    // Tail-paired passes of modes 12..=15 and head-paired passes of 16..=19.
    let tail_pass = |q: &mut Buf, descending: bool| {
        for i in 0..n - 1 {
            q.push(at(n - 1));
            q.push(at(if descending { n - 2 - i } else { i }));
        }
    };
    let head_pass = |q: &mut Buf, reverse: bool, offset: usize| {
        for i in 1..n {
            q.push(at(0));
            q.push(at(if reverse { n - i - offset } else { i }));
        }
    };
    match cfg.mode {
        0 => {
            (0..n).for_each(|i| q.push(at(i)));
            flags(q, b, t);
        }
        1 => {
            (0..n).rev().for_each(|i| q.push(at(i)));
            flags(q, t, b);
        }
        2 => {
            (0..n).for_each(|i| q.push(at(i)));
            if t {
                q.tail_dup();
            }
            (1..n - 1).rev().for_each(|i| q.push(at(i)));
            if b {
                q.append_head();
            }
        }
        3 => {
            (0..n).rev().for_each(|i| q.push(at(i)));
            if b {
                q.tail_dup();
            }
            (1..n - 1).for_each(|i| q.push(at(i)));
            if t {
                q.append_head();
            }
        }
        4 => {
            (0..n).for_each(|i| q.push(at(a(i))));
            flags(q, b, t);
        }
        5 => {
            (0..n).for_each(|i| q.push(at(n - 1 - a(i))));
            flags(q, t, b);
        }
        6 => {
            (0..n).for_each(|i| q.push(at(a(i))));
            if t {
                q.tail_dup();
            }
            (1..n - 1).for_each(|i| q.push(at(n - 1 - a(i))));
            if b {
                q.append_head();
            }
        }
        7 => {
            (0..n).for_each(|i| q.push(at(n - 1 - a(i))));
            if b {
                q.tail_dup();
            }
            (1..n - 1).for_each(|i| q.push(at(a(i))));
            if t {
                q.append_head();
            }
        }
        8 => {
            fold(p.slice(), q);
            flags(q, b, t);
        }
        9 => {
            fold(p.slice(), q);
            q.slice_mut().reverse();
            flags(q, t, b);
        }
        10 => {
            fold(p.slice(), q);
            if t {
                q.tail_dup();
            }
            fold(p.slice(), q);
            q.slice_mut()[n..].reverse();
            flags(q, b, false);
        }
        11 => {
            fold(p.slice(), q);
            if b {
                q.tail_dup();
            }
            fold(p.slice(), q);
            q.slice_mut()[..n].reverse();
            flags(q, t, false);
        }
        12 | 13 if n == 1 => q.push(at(0)),
        12 => {
            tail_pass(q, false);
            flags(q, t, b);
        }
        13 => {
            tail_pass(q, true);
            flags(q, t, b);
        }
        14 | 15 if n == 1 => q.push(at(0)),
        14 | 15 => {
            let first = cfg.mode == 15;
            tail_pass(q, first);
            flags(q, t, b);
            tail_pass(q, !first);
        }
        16..=19 if n == 1 => q.push(at(0)),
        16 => {
            head_pass(q, false, 0);
            flags(q, b, t);
        }
        17 => {
            head_pass(q, true, 0);
            flags(q, b, t);
        }
        18 => {
            head_pass(q, false, 0);
            flags(q, b, t);
            head_pass(q, true, 0);
        }
        19 => {
            head_pass(q, true, 1);
            flags(q, b, t);
            head_pass(q, false, 0);
        }
        20 | 21 => {
            for _ in 0..n {
                q.push(at((draw(rng, seed) % n as u64) as usize));
            }
        }
        22 | 23 => {
            (0..n).for_each(|i| q.push(at(i)));
            let items = q.slice_mut();
            for i in 0..n - 1 {
                let mut j = i + (draw(rng, seed) % (n - i) as u64) as usize;
                // The first pick rejects the previous key while a choice remains.
                if i == 0 && n >= 2 && prior.is_some_and(|k| items[j].key == k) {
                    j = (0..n).find(|&c| Some(items[c].key) != prior).unwrap_or(j);
                }
                items.swap(i, j);
            }
        }
        25 => (0..n).for_each(|i| q.push(at(i))),
        _ => {}
    }
    if cfg.octave_mode == 0 && blocks > 0 {
        let base = q.len;
        for j in 1..=blocks {
            for i in 0..base {
                let pair = q.items[i];
                q.push(Pair { source: pair.source, key: shifted(pair.key, sign * j) });
            }
        }
    }
    if matches!(cfg.mode, 21 | 23) {
        shuffle(q.slice_mut(), rng, seed);
        if cfg.mode == 23 && q.len >= 2 {
            let mut tries = 0;
            while prior.is_some_and(|k| q.items[0].key == k) && tries < 64 {
                shuffle(q.slice_mut(), rng, seed);
                tries += 1;
            }
        }
    }
    if cfg.mode == 25 {
        if cfg.octave_mode != 1 && cfg.num_strike > 1 {
            let len = q.len;
            let block = len / (blocks as usize + 1);
            if block > 0 {
                p.clear();
                (0..len).for_each(|i| p.push(q.items[i]));
                q.clear();
                for j in 0..len / block {
                    for _ in 0..cfg.num_strike {
                        (0..block).for_each(|k| q.push(p.items[j * block + k]));
                    }
                }
            }
        }
    } else if cfg.num_strike > 1 {
        p.clear();
        (0..q.len).for_each(|i| p.push(q.items[i]));
        q.clear();
        for i in 0..p.len {
            for _ in 0..cfg.num_strike {
                q.push(p.items[i]);
            }
        }
    }
}

/// Swing warp of a beat position (0x14154c3d0, constructor 0x14154afd0): the
/// sixteenth-note grid gets a 16-entry circular offset, even entries 0 and odd
/// entries 0.5, interpolated linearly; `b + (4/16) amount g(4 b)`.
pub fn groove_warp(beat: f64, amount: f64) -> f64 {
    let x = beat * 4.0;
    let i = x.floor();
    let f = x - i;
    let odd = |k: f64| if k.rem_euclid(2.0) == 1.0 { 0.5 } else { 0.0 };
    let (g0, g1) = (odd(i), odd(i + 1.0));
    beat + 0.25 * amount * (g0 + (g1 - g0) * f)
}

pub struct PatternArpState {
    down: [bool; 128],
    held: [bool; 128],
    velocity: [u8; 128],
    list: Buf,
    scratch: Buf,
    dirty: bool,
    new_note: bool,
    reindex: bool,
    old_len: usize,
    signature: Option<(u8, i32, u8, u32, bool, bool)>,
    running: bool,
    countdown: f64,
    beat: f64,
    step: usize,
    cursor: usize,
    prior: Option<i32>,
    rng: u64,
    seed: u64,
    pending: [Pending; PENDING],
    pending_len: usize,
}

impl PatternArpState {
    pub fn new(seed: u64) -> Self {
        Self {
            down: [false; 128],
            held: [false; 128],
            velocity: [0; 128],
            list: Buf::new(),
            scratch: Buf::new(),
            dirty: false,
            new_note: false,
            reindex: false,
            old_len: 0,
            signature: None,
            running: false,
            countdown: 0.0,
            beat: 0.0,
            step: 0,
            cursor: 0,
            prior: None,
            rng: 0,
            seed,
            pending: [Pending::default(); PENDING],
            pending_len: 0,
        }
    }

    pub fn note_on(&mut self, key: u8, velocity: u8) {
        let k = usize::from(key & 127);
        (self.down[k], self.held[k], self.velocity[k]) = (true, true, velocity);
        self.dirty = true;
        self.new_note = true;
    }

    /// With `hold` the key stays in the pattern (0x14110f880 leaves the held
    /// list and the ordering untouched on note-off).
    pub fn note_off(&mut self, key: u8, hold: bool) {
        let k = usize::from(key & 127);
        self.down[k] = false;
        if !hold {
            self.held[k] = false;
            self.dirty = true;
        }
    }

    /// Hold switched off: only the keys still down remain.
    pub fn release_hold(&mut self) {
        self.held = self.down;
        self.dirty = true;
    }

    /// Frames until the next step onset, while running.
    pub fn countdown(&self) -> Option<f64> {
        self.running.then_some(self.countdown)
    }

    pub fn ordered(&self) -> &[Pair] {
        self.list.slice()
    }

    fn rebuild(&mut self, cfg: &PatternArp) {
        let mut held = [Pair::default(); 128];
        let mut count = 0;
        for (k, &on) in self.held.iter().enumerate() {
            if on {
                held[count] = Pair { source: k as u8, key: k as i32 };
                count += 1;
            }
        }
        let before = self.list.len;
        build(cfg, &held[..count], self.prior, &mut self.rng, self.seed, &mut self.scratch, &mut self.list);
        self.dirty = false;
        if self.list.len == 0 {
            // 0x1411121e0: an empty input resets indices, beat phase and running.
            (self.cursor, self.beat, self.running) = (0, 0.0, false);
            (self.prior, self.reindex, self.old_len) = (None, false, 0);
            self.new_note = false;
            return;
        }
        if !self.reindex {
            self.old_len = before;
        }
        self.reindex = true;
        if self.new_note && cfg.trigger == PatternTrigger::Note {
            self.step = 0;
            self.running = false;
        }
        self.new_note = false;
        if !self.running {
            (self.countdown, self.running, self.beat) = (0.0, true, 0.0);
        }
    }

    /// Reorders for a fresh random pass without touching the transport.
    fn reorder(&mut self, cfg: &PatternArp) {
        let mut held = [Pair::default(); 128];
        let mut count = 0;
        for (k, &on) in self.held.iter().enumerate() {
            if on {
                held[count] = Pair { source: k as u8, key: k as i32 };
                count += 1;
            }
        }
        build(cfg, &held[..count], self.prior, &mut self.rng, self.seed, &mut self.scratch, &mut self.list);
    }

    /// 0x141112000: each pending note-off with `late` equal to `late` either
    /// counts `chunk` frames down or, when it would not survive the chunk
    /// (including a count already at zero), ends at `base + remaining`. A count
    /// equal to the chunk reaches zero and ends in the next drain.
    fn drain(&mut self, late: bool, chunk: u32, base: u32, emit: &mut impl FnMut(PatternEvent)) {
        let mut i = 0;
        while i < self.pending_len {
            let p = self.pending[i];
            if p.late != late {
                i += 1;
            } else if chunk <= p.remaining && p.remaining != 0 {
                self.pending[i].remaining -= chunk;
                i += 1;
            } else {
                emit(PatternEvent::Off { key: p.key, at: base + p.remaining });
                self.pending.copy_within(i + 1..self.pending_len, i);
                self.pending_len -= 1;
            }
        }
    }

    /// Advance `frames` at `tempo` BPM and `rate` Hz, reporting events in order
    /// with offsets inside the span.
    pub fn advance(&mut self, cfg: &PatternArp, frames: u32, tempo: f64, rate: f64, mut emit: impl FnMut(PatternEvent)) {
        if !cfg.valid() || !(tempo > 0.0) || !(rate > 0.0) {
            return;
        }
        let signature = (cfg.mode, cfg.octave, cfg.octave_mode, cfg.num_strike, cfg.repeat_bottom, cfg.repeat_top);
        if self.dirty || self.signature != Some(signature) {
            self.signature = Some(signature);
            self.rebuild(cfg);
        }
        let seconds_per_beat = 60.0 / tempo;
        let mut done = 0u32;
        let mut fires = 0u32;
        while done < frames {
            let mut chunk = frames - done;
            let mut fire = false;
            if self.running {
                let due = self.countdown.ceil().max(0.0).min(f64::from(u32::MAX)) as u32;
                fire = due == 0;
                chunk = chunk.min(due);
            }
            self.drain(false, chunk, done, &mut emit);
            if self.running {
                if fire {
                    // A step shorter than a frame fires repeatedly; bound the work.
                    fires += 1;
                    if fires > 4096 {
                        return;
                    }
                    self.fire(cfg, done, rate, seconds_per_beat, &mut emit);
                } else {
                    self.countdown -= f64::from(chunk);
                }
            }
            self.drain(true, chunk, done, &mut emit);
            done += chunk;
        }
    }

    /// One step boundary (0x141110c90).
    fn fire(&mut self, cfg: &PatternArp, at: u32, rate: f64, seconds_per_beat: f64, emit: &mut impl FnMut(PatternEvent)) {
        let len = self.list.len;
        if self.reindex {
            if len == 0 {
                self.cursor = 0;
            } else if self.old_len != len && self.prior.is_some() {
                self.cursor %= len;
                if !matches!(cfg.mode, 20..=23 | 25) {
                    if let Some(i) = self.list.slice().iter().position(|p| Some(p.key) == self.prior) {
                        self.cursor = (i + 1) % len;
                    }
                }
            }
            self.reindex = false;
        }
        let step = cfg.steps[self.step];
        let steps = cfg.num_steps as usize;
        if matches!(step.state, 1 | 3) && len > 0 {
            let first = self.list.items[self.cursor % len];
            self.prior = Some(first.key);
            let mut ties = 0;
            let mut probe = self.step;
            while ties < steps {
                let next = (probe + 1) % steps;
                if cfg.steps[next].state != 2 {
                    break;
                }
                probe = next;
                ties += 1;
            }
            let span = f64::from(1 + ties as u32) * cfg.resolution;
            let gate = (groove_warp(self.beat + span, cfg.groove) - groove_warp(self.beat, cfg.groove))
                * rate
                * seconds_per_beat
                * f64::from(step.size)
                * f64::from(cfg.step_length);
            let gate = if step.state == 3 { gate.ceil() } else { gate.trunc() };
            let gate = gate.max(0.0).min(f64::from(u32::MAX)) as u32;
            if !cfg.bypass {
                // 0x141110c90: (level 126 + 1)(1 - a) + velocity a, truncated.
                let blend = cfg.velocity_blend;
                let (group, start) = if cfg.mode == 25 {
                    let d = cfg.octave.unsigned_abs() as usize + 1;
                    if cfg.octave_mode == 1 {
                        (len, 0)
                    } else {
                        let size = (len / d) / cfg.num_strike as usize;
                        (size, size * self.cursor)
                    }
                } else {
                    (1, self.cursor % len)
                };
                for k in 0..group {
                    let pair = self.list.items[(start + k) % len];
                    let v = f32::from(self.velocity[usize::from(pair.source & 127)]);
                    let velocity = ((step.level * 126.0 + 1.0) * (1.0 - blend) + v * blend) as i32;
                    let key = pair.key + step.offset;
                    emit(PatternEvent::On { key, velocity, at });
                    if self.pending_len < PENDING {
                        self.pending[self.pending_len] = Pending { key, remaining: gate, late: step.state == 3 };
                        self.pending_len += 1;
                    }
                }
            }
            self.cursor = if cfg.mode == 25 && cfg.octave_mode == 0 {
                let d = cfg.octave.unsigned_abs() as usize + 1;
                let size = ((len / d) / cfg.num_strike as usize).max(1);
                (self.cursor + 1) % (len / size).max(1)
            } else {
                (self.cursor + 1) % len
            };
            if matches!(cfg.mode, 20..=23) && self.cursor == 0 {
                self.reorder(cfg);
            }
        }
        if step.state != 0 && !cfg.bypass && cfg.step_cc_id >= 0 {
            emit(PatternEvent::Cc { controller: cfg.step_cc_id, value: step.cc, at });
        }
        let next = groove_warp(self.beat + cfg.resolution, cfg.groove) - groove_warp(self.beat, cfg.groove);
        self.step = (self.step + 1) % steps;
        self.countdown += next * rate * seconds_per_beat;
        self.beat += cfg.resolution;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(mode: u8) -> PatternArp {
        let mut steps = [PatternStep::default(); PATTERN_STEPS];
        steps[0].state = 1;
        PatternArp {
            mode,
            octave: 0,
            octave_mode: 0,
            num_strike: 1,
            repeat_bottom: false,
            repeat_top: false,
            resolution: 1.0,
            step_length: 1.0,
            groove: 0.0,
            velocity_blend: 0.0,
            num_steps: 1,
            step_cc_id: -1,
            trigger: PatternTrigger::Note,
            hold: false,
            bypass: false,
            steps,
        }
    }

    fn all_play(mut c: PatternArp) -> PatternArp {
        c.steps.iter_mut().for_each(|s| s.state = 1);
        c
    }

    fn order(c: &PatternArp, keys: &[u8]) -> Vec<i32> {
        let mut s = PatternArpState::new(1);
        for &k in keys {
            s.note_on(k, 100);
        }
        s.rebuild(c);
        s.ordered().iter().map(|p| p.key).collect()
    }

    const K: [u8; 3] = [60, 64, 67];

    /// Spec "Note-sequence examples", 0x1411121e0 modes 0..3.
    #[test]
    fn modes_0_to_3_order_up_down_and_bounce() {
        let o = |m| order(&cfg(m), &K);
        assert_eq!(o(0), [60, 64, 67]);
        assert_eq!(o(1), [67, 64, 60]);
        assert_eq!(o(2), [60, 64, 67, 64]);
        assert_eq!(o(3), [67, 64, 60, 64]);
    }

    /// Endpoint flags pick HeadDupFront, TailDup or AppendHead per mode.
    #[test]
    fn repeat_flags_duplicate_the_endpoint_the_mode_names() {
        let with = |m, b, t| order(&PatternArp { repeat_bottom: b, repeat_top: t, ..cfg(m) }, &K);
        assert_eq!(with(0, true, true), [60, 60, 64, 67, 67]);
        assert_eq!(with(1, true, true), [67, 67, 64, 60, 60]);
        assert_eq!(with(2, false, true), [60, 64, 67, 67, 64]);
        assert_eq!(with(2, true, false), [60, 64, 67, 64, 60]);
        assert_eq!(with(3, true, false), [67, 64, 60, 60, 64]);
        assert_eq!(with(3, false, true), [67, 64, 60, 64, 67]);
    }

    /// 0x141e514f0: A(i) = min(n-1, (0,2,1,3)[i%4] + 4 floor(i/4)).
    #[test]
    fn modes_4_and_5_use_the_0_2_1_3_permutation() {
        let keys: Vec<u8> = (60..66).collect();
        assert_eq!(order(&cfg(4), &keys), [60, 62, 61, 63, 64, 65]);
        assert_eq!(order(&cfg(5), &keys), [65, 63, 64, 62, 61, 60]);
        assert_eq!(order(&cfg(4), &keys[..5]), [60, 62, 61, 63, 64]);
        assert_eq!(order(&cfg(6), &[60, 61, 62, 63]), [60, 62, 61, 63, 61, 62]);
    }

    #[test]
    fn modes_8_to_11_fold_low_high() {
        let keys = [60, 62, 64, 65, 67];
        assert_eq!(order(&cfg(8), &keys), [60, 67, 62, 65, 64]);
        assert_eq!(order(&cfg(9), &keys), [64, 65, 62, 67, 60]);
        assert_eq!(order(&cfg(10), &K), [60, 67, 64, 64, 67, 60]);
        assert_eq!(order(&PatternArp { repeat_top: true, ..cfg(10) }, &K), [60, 67, 64, 64, 67, 60, 64]);
        assert_eq!(order(&cfg(11), &K), [64, 67, 60, 60, 67, 64]);
    }

    /// Spec examples for the paired modes 12..=19 (OctaveMode 0, no flags).
    #[test]
    fn paired_modes_match_the_native_loop_indices() {
        let o = |m| order(&cfg(m), &K);
        assert_eq!(o(12), [67, 60, 67, 64]);
        assert_eq!(o(13), [67, 64, 67, 60]);
        assert_eq!(o(14), [67, 60, 67, 64, 67, 64, 67, 60]);
        assert_eq!(o(15), [67, 64, 67, 60, 67, 60, 67, 64]);
        assert_eq!(o(16), [60, 64, 60, 67]);
        assert_eq!(o(17), [60, 67, 60, 64]);
        assert_eq!(o(18), [60, 64, 60, 67, 60, 67, 60, 64]);
        assert_eq!(o(19), [60, 64, 60, 60, 60, 64, 60, 67]);
        assert_eq!(order(&cfg(12), &[60]), [60]);
    }

    /// OctaveMode 1 expands, clamps, drops duplicates and sorts before the mode;
    /// OctaveMode 0 appends shifted copies of the mode output (0x1411144b7).
    #[test]
    fn octave_modes_expand_before_or_after_ordering() {
        let pre = PatternArp { octave: 2, octave_mode: 1, ..cfg(1) };
        assert_eq!(order(&pre, &[60, 72]), [96, 84, 72, 60]);
        let post = PatternArp { octave: -1, octave_mode: 0, ..cfg(0) };
        assert_eq!(order(&post, &[60, 64]), [60, 64, 48, 52]);
        let up = PatternArp { octave: 1, octave_mode: 1, ..cfg(0) };
        assert_eq!(order(&up, &[60, 72]), [60, 72, 84]);
    }

    /// 0x141114803: every mode-emitted pair repeats NumStrike times in place.
    #[test]
    fn num_strike_repeats_each_pair() {
        let c = PatternArp { num_strike: 3, ..cfg(0) };
        assert_eq!(order(&c, &[60, 64]), [60, 60, 60, 64, 64, 64]);
    }

    /// 0x1411146b8: mode 25 with octave mode 0 repeats whole octave blocks.
    #[test]
    fn mode_25_repeats_octave_blocks() {
        let c = PatternArp { octave: 1, octave_mode: 0, num_strike: 2, ..cfg(25) };
        assert_eq!(order(&c, &[60, 64]), [60, 64, 60, 64, 72, 76, 72, 76]);
    }

    fn run(c: &PatternArp, keys: &[(u8, u8)], frames: u32) -> Vec<PatternEvent> {
        let mut s = PatternArpState::new(1);
        for &(k, v) in keys {
            s.note_on(k, v);
        }
        let mut out = Vec::new();
        // 120 BPM at 1000 Hz: a quarter note is 500 frames.
        s.advance(c, frames, 120.0, 1000.0, |e| out.push(e));
        out
    }

    /// 0x141110c90: (level 126 + 1)(1 - a) + velocity a, truncated toward zero.
    #[test]
    fn velocity_blends_step_level_and_source_velocity() {
        let on = |a: f32, level: f32| {
            let mut c = cfg(0);
            c.velocity_blend = a;
            c.steps[0].level = level;
            match run(&c, &[(60, 90)], 1)[0] {
                PatternEvent::On { velocity, .. } => velocity,
                e => panic!("{e:?}"),
            }
        };
        assert_eq!(on(0.0, 1.0), 127);
        assert_eq!(on(0.0, 0.0), 1);
        assert_eq!(on(1.0, 0.3), 90);
        assert_eq!(on(0.5, 0.5), ((0.5 * 126.0 + 1.0) * 0.5 + 45.0) as i32);
    }

    /// Swing: the odd sixteenth boundary is delayed by amount / 8 quarter notes.
    #[test]
    fn groove_delays_odd_sixteenths() {
        assert_eq!(groove_warp(0.0, 1.0), 0.0);
        assert!((groove_warp(0.25, 1.0) - 0.375).abs() < 1e-12);
        assert!((groove_warp(0.5, 1.0) - 0.5).abs() < 1e-12);
        assert!((groove_warp(0.125, 1.0) - (0.125 + 0.25 * 0.25)).abs() < 1e-12);
        let mut c = all_play(cfg(0));
        c.num_steps = 4;
        c.resolution = 0.25;
        c.groove = 1.0;
        let on: Vec<u32> = run(&c, &[(60, 100)], 400)
            .iter()
            .filter_map(|e| match e {
                PatternEvent::On { at, .. } => Some(*at),
                _ => None,
            })
            .collect();
        // 125 frames per sixteenth; the odd boundary moves to 187.5 and back.
        assert_eq!(on, [0, 188, 250]);
    }

    /// State 1 truncates its gate, state 3 rounds it up; a following state-2 step
    /// extends the gate by its step length.
    #[test]
    fn gates_truncate_ceil_and_extend_over_ties() {
        let off = |c: &PatternArp| {
            run(c, &[(60, 100)], 600).into_iter().find_map(|e| match e {
                PatternEvent::Off { at, .. } => Some(at),
                _ => None,
            })
        };
        let mut c = cfg(0);
        c.resolution = 0.3; // 150 frames
        c.steps[0].size = 0.5014; // 75.2 frames
        assert_eq!(off(&c), Some(75));
        c.steps[0].state = 3;
        assert_eq!(off(&c), Some(76));
        let mut tie = cfg(0);
        tie.num_steps = 2;
        tie.steps[1].state = 2;
        tie.steps[0].size = 0.5;
        // Two quarter notes of gate at size 0.5.
        assert_eq!(off(&tie), Some(500));
    }

    /// Rest steps do not advance the note cursor (only note steps do).
    #[test]
    fn rests_keep_the_note_cursor() {
        let mut c = cfg(0);
        c.num_steps = 2;
        c.steps[0].state = 1;
        c.steps[1].state = 0;
        let keys: Vec<i32> = run(&c, &[(60, 100), (64, 100)], 2000)
            .iter()
            .filter_map(|e| match e {
                PatternEvent::On { key, .. } => Some(*key),
                _ => None,
            })
            .collect();
        assert_eq!(keys, [60, 64]);
    }

    /// Step offset adds to the key; a nonzero step emits CC `StepCCId` = Step CC.
    #[test]
    fn offset_and_step_cc_events() {
        let mut c = cfg(0);
        c.steps[0].offset = -5;
        c.steps[0].cc = 99;
        c.step_cc_id = 74;
        let e = run(&c, &[(60, 100)], 1);
        assert!(matches!(e[0], PatternEvent::On { key: 55, at: 0, .. }));
        assert_eq!(e[1], PatternEvent::Cc { controller: 74, value: 99, at: 0 });
        c.step_cc_id = -1;
        assert!(run(&c, &[(60, 100)], 1).iter().all(|e| !matches!(e, PatternEvent::Cc { .. })));
        c.bypass = true;
        assert!(run(&c, &[(60, 100)], 1).is_empty());
    }

    /// 0x141112000: a count equal to the chunk reaches zero and ends in a later
    /// drain, never at the chunk end of the same call.
    #[test]
    fn note_off_at_exactly_the_chunk_end_waits_for_the_next_drain() {
        let mut c = cfg(0);
        c.steps[0].size = 0.2; // 100 frames
        let mut s = PatternArpState::new(1);
        s.note_on(60, 100);
        let mut ev = Vec::new();
        s.advance(&c, 100, 120.0, 1000.0, |e| ev.push(e));
        assert_eq!(ev.len(), 1, "{ev:?}");
        s.advance(&c, 10, 120.0, 1000.0, |e| ev.push(e));
        assert_eq!(ev[1], PatternEvent::Off { key: 60, at: 0 });
    }

    /// Hold keeps released keys in the pattern; releasing hold keeps only keys down.
    #[test]
    fn hold_latches_released_keys() {
        let c = cfg(0);
        let mut s = PatternArpState::new(1);
        s.note_on(60, 100);
        s.note_on(64, 100);
        s.note_off(64, true);
        s.advance(&c, 1, 120.0, 1000.0, |_| {});
        assert_eq!(s.ordered().len(), 2);
        s.release_hold();
        s.advance(&c, 1, 120.0, 1000.0, |_| {});
        assert_eq!(s.ordered().len(), 1);
    }

    /// Note trigger mode restarts at step 0 on a new note; legato continues.
    #[test]
    fn trigger_mode_restarts_or_continues() {
        let mut c = all_play(cfg(0));
        c.num_steps = 4;
        for (trigger, step_after) in [(PatternTrigger::Note, 1), (PatternTrigger::Legato, 2)] {
            c.trigger = trigger;
            let mut s = PatternArpState::new(1);
            s.note_on(60, 100);
            s.advance(&c, 600, 120.0, 1000.0, |_| {});
            assert_eq!(s.step, 2);
            s.note_on(64, 100);
            s.advance(&c, 1, 120.0, 1000.0, |_| {});
            assert_eq!(s.step, step_after, "{trigger:?}");
        }
    }
}
