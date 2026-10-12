//! Immutable per-region playback metadata and independent per-voice cursors.
use crate::{
    Error, Frame,
    envelope::EnvelopeState,
    resample::{DECIMATION_REACH, Kernel, MAX_STEP, MIN_STEP},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Direction {
    #[default]
    Forward,
    Reverse,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopMode {
    Continuous,
    UntilRelease,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopShape {
    Wrap,
    /// Wrap with a linear complementary blend into the pre-loop guard (post-loop
    /// guard for reverse playback). Keeps the loop period; frames must be nonzero,
    /// fit the loop, and fit that guard inside the source view. Linear gains sum
    /// to one: right for correlated legs, a mid-fade dip for uncorrelated ones.
    Crossfade {
        frames: usize,
    },
    /// `Crossfade` with cos/sin gains: constant power for uncorrelated legs
    /// (noise, ensembles), up to +3 dB mid-fade for correlated ones.
    EqualPowerCrossfade {
        frames: usize,
    },
    /// Reflect between the first/last included frames; endpoints occur once per turn.
    PingPong,
}

/// Half-open sample-frame range. One-frame loops are valid.
#[derive(Clone, Copy, Debug)]
pub struct Loop {
    pub start: usize,
    pub end: usize,
    pub mode: LoopMode,
    pub shape: LoopShape,
    /// Total outward passes. One traverses the loop once without repeating;
    /// ping-pong adds a round trip per further pass. None repeats indefinitely.
    pub passes: Option<std::num::NonZeroU32>,
}

impl Loop {
    fn period(self) -> u64 {
        let length = (self.end - self.start) as u64;
        match self.shape {
            LoopShape::Wrap
            | LoopShape::Crossfade { .. }
            | LoopShape::EqualPowerCrossfade { .. } => length,
            LoopShape::PingPong => (2 * (length - 1)).max(1),
        }
    }

    // Relative index, contiguous count and whether traversal reverses the initial
    // direction. Used by both exact spans and every interpolation guard read.
    #[inline]
    fn span(self, distance: u64) -> (u64, u64, bool) {
        let length = (self.end - self.start) as u64;
        if self.shape != LoopShape::PingPong || length == 1 {
            let within = distance % length;
            return (within, length - within, false);
        }
        let period = self.period();
        // distance starts after the first pass has visited the far endpoint.
        let phase = (distance + 1) % period;
        if phase < length - 1 {
            (length - 1 - phase, length - phase, true)
        } else {
            (phase - (length - 1), period - phase + 1, false)
        }
    }
}

/// One physical native loop slot. Tuning applies to repeated traversals.
#[derive(Clone, Copy, Debug)]
pub struct LoopSlot {
    pub range: Loop,
    pub tuning: f64,
}

#[derive(Clone, Copy, Debug)]
struct LoopSlots {
    slots: [Option<LoopSlot>; 8],
    exits: [Option<u64>; 8],
}

/// Immutable source view. Transposition combines with the asset/output rate ratio.
#[derive(Clone, Copy, Debug, Default)]
pub struct Playback {
    pub start: usize,
    pub end: Option<usize>,
    pub direction: Direction,
    pub loop_range: Option<Loop>,
    pub loop_slots: [Option<LoopSlot>; 8],
    pub transpose_semitones: f64,
}

impl Playback {
    pub(super) fn step(self, source_rate: u32, output_rate: u32) -> f64 {
        f64::from(source_rate) / f64::from(output_rate) * (self.transpose_semitones / 12.0).exp2()
    }

    pub(super) fn cursor(
        self,
        frames: usize,
        source_rate: u32,
        output_rate: u32,
    ) -> Result<Cursor, Error> {
        let end = self.end.unwrap_or(frames);
        let step = self.step(source_rate, output_rate);
        if self.start >= end
            || end > frames
            || source_rate == 0
            || output_rate == 0
            || !self.transpose_semitones.is_finite()
            || !(MIN_STEP..=MAX_STEP).contains(&step)
            || self.loop_range.is_some_and(|r| {
                r.start < self.start
                    || r.start >= r.end
                    || r.end > end
                    || r.shape == LoopShape::PingPong && (r.end - r.start - 1) as u64 > u64::MAX / 2
                    || matches!(r.shape, LoopShape::Crossfade { frames } | LoopShape::EqualPowerCrossfade { frames } if frames == 0
                    || frames > r.end - r.start
                    || frames > match self.direction {
                        Direction::Forward => r.start - self.start,
                        Direction::Reverse => end - r.end,
                    })
            })
        {
            return Err(Error::InvalidInput);
        }
        let mut cursor = Cursor {
            start: self.start,
            end,
            direction: self.direction,
            loop_range: self.loop_range,
            loops: None,
            position: 0,
            fraction: 0.0,
            step,
            exit: None,
            last: [0.; 2],
            starvation: None,
            fade_in: 0,
            fade_frames: output_rate.div_ceil(1000),
            cold_hold: 0,
        };
        if let Some(range) = self.loop_range
            && let Some(passes) = range.passes
        {
            let repeated = range
                .period()
                .checked_mul(u64::from(passes.get() - 1))
                .ok_or(Error::InvalidInput)?;
            ((end - self.start) as u64)
                .checked_add(repeated)
                .ok_or(Error::InvalidInput)?;
            cursor.exit = Some(
                cursor
                    .first_boundary(range)
                    .checked_add(repeated)
                    .ok_or(Error::InvalidInput)?,
            );
        }
        if self.loop_slots.iter().any(Option::is_some) {
            if self.loop_range.is_some() {
                return Err(Error::InvalidInput);
            }
            let mut slots = self.loop_slots;
            // Preserve physical slots in Playback; the cursor visits source order.
            slots.sort_by_key(|slot| {
                slot.map_or(u64::MAX, |slot| cursor.first_boundary(slot.range))
            });
            let mut exits = [None; 8];
            let mut previous_end = 0;
            for (i, slot) in slots
                .iter()
                .enumerate()
                .filter_map(|(i, s)| s.map(|s| (i, s)))
            {
                let single = Playback {
                    loop_range: Some(slot.range),
                    loop_slots: [None; 8],
                    ..self
                }
                .cursor(frames, source_rate, output_rate)?;
                if !slot.tuning.is_finite()
                    || !(MIN_STEP..=MAX_STEP).contains(&(step * slot.tuning))
                {
                    return Err(Error::InvalidInput);
                }
                let first = cursor.first_boundary(slot.range);
                let begin = first - (slot.range.end - slot.range.start) as u64;
                if begin < previous_end {
                    return Err(Error::InvalidInput);
                }
                previous_end = first;
                exits[i] = single.exit;
            }
            if slots.iter().flatten().count() == 1 && slots[0].unwrap().tuning == 1.0 {
                // The existing fast cursor covers the overwhelmingly common one-slot case.
                cursor.loop_range = Some(slots[0].unwrap().range);
                cursor.exit = exits[0];
            } else {
                cursor.loops = Some(LoopSlots { slots, exits });
            }
        }
        Ok(cursor)
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Cursor {
    start: usize,
    end: usize,
    direction: Direction,
    loop_range: Option<Loop>,
    loops: Option<LoopSlots>,
    // Distance along the traversal, not an absolute floating-point PCM index.
    position: u64,
    fraction: f64,
    step: f64,
    // First loop boundary that will no longer wrap. Earlier traversal remains
    // available to the interpolation window after release.
    exit: Option<u64>,
    last: Frame,
    // Remaining fade-out frames after a miss; zero while waiting for the page.
    starvation: Option<u32>,
    // Remaining fade-in frames after a recovered miss.
    fade_in: u32,
    fade_frames: u32,
    /// Bounded onset hold; later streaming misses still advance in source time.
    cold_hold: u32,
}

struct ReadAddress {
    primary: usize,
    // Partner index and (primary, partner) gains.
    crossfade: Option<(usize, [f64; 2])>,
}

impl Cursor {
    /// Start within the original view, measured in source time, never pitch time.
    /// Offsets at/past the view end are silent. Starting past a loop's outward
    /// edge bypasses it; starting inside retains its original boundaries/count.
    pub(super) fn with_offset(mut self, micros: u32, source_rate: u32) -> Self {
        let ticks = u64::from(micros) * u64::from(source_rate);
        self.position = ticks / 1_000_000;
        self.fraction = (ticks % 1_000_000) as f64 / 1_000_000.;
        if self.position >= (self.end - self.start) as u64
            || self
                .loop_range
                .is_some_and(|r| self.position >= self.first_boundary(r))
        {
            self.loop_range = None;
            self.exit = None;
        }
        if let Some(mut loops) = self.loops {
            for (i, slot) in loops
                .slots
                .iter()
                .enumerate()
                .filter_map(|(i, s)| s.map(|s| (i, s)))
            {
                let first = self.first_boundary(slot.range);
                if self.position >= first {
                    loops.exits[i] = Some(first);
                }
            }
            self.loops = Some(loops);
        }
        self
    }

    /// Skip `frames` further source frames from the current start position.
    pub(super) fn skip(mut self, frames: u32) -> Self {
        self.position += u64::from(frames);
        if self.position >= (self.end - self.start) as u64
            || self
                .loop_range
                .is_some_and(|r| self.position >= self.first_boundary(r))
        {
            self.loop_range = None;
            self.exit = None;
        }
        if let Some(mut loops) = self.loops {
            for (i, slot) in loops
                .slots
                .iter()
                .enumerate()
                .filter_map(|(i, s)| s.map(|s| (i, s)))
            {
                let first = self.first_boundary(slot.range);
                if self.position >= first {
                    loops.exits[i] = Some(first);
                }
            }
            self.loops = Some(loops);
        }
        self
    }

    pub(super) fn unbounded_loop(&self) -> bool {
        self.loops.is_some_and(|loops| {
            loops
                .slots
                .iter()
                .enumerate()
                .any(|(i, slot)| slot.is_some() && loops.exits[i].is_none())
        }) || self.loop_range.is_some() && self.exit.is_none()
    }

    pub(super) fn trace_direction(&self) -> Direction { self.direction }

    pub(super) fn trace_loops(&self) -> [Option<crate::SelectedLoop>; 8] {
        let convert = |range: Loop, tuning: f64| crate::SelectedLoop {
            start: range.start, end: range.end, until_release: range.mode == LoopMode::UntilRelease,
            alternating: range.shape == LoopShape::PingPong,
            crossfade: match range.shape { LoopShape::Crossfade {frames} | LoopShape::EqualPowerCrossfade {frames} => frames, _ => 0 },
            count: range.passes.map_or(0, |n| n.get()), tuning_bits: tuning.to_bits(),
        };
        if let Some(loops) = self.loops { loops.slots.map(|s| s.map(|s| convert(s.range, s.tuning))) }
        else { let mut slots = [None; 8]; slots[0] = self.loop_range.map(|r| convert(r, 1.0)); slots }
    }

    pub(super) fn trace_start(&self) -> u64 { self.start as u64 }
    pub(super) fn trace_position(&self) -> u64 { self.index(self.position as i128).map_or(self.start as u64, |i| i as u64) }

    pub(super) fn step(&self) -> f64 {
        if let Some(loops) = self.loops {
            let (local, position) = self.segment(i128::from(self.position));
            if let Some(r) = local.loop_range
                && position >= i128::from(local.first_boundary(r))
                && local.exit.is_none_or(|exit| position < i128::from(exit))
            {
                let tuning = loops
                    .slots
                    .iter()
                    .flatten()
                    .find(|slot| slot.range.start == r.start && slot.range.end == r.end)
                    .unwrap()
                    .tuning;
                return self.step * tuning;
            }
        }
        self.step
    }

    // Reuse the established single-loop address law for each serial segment.
    // Each finite slot adds its repetitions to the traversal before the next.
    fn segment(&self, position: i128) -> (Self, i128) {
        let mut local = *self;
        let loops = self.loops.unwrap();
        local.loops = None;
        local.loop_range = None;
        local.exit = None;
        let mut offset = position;
        for (i, slot) in loops
            .slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.map(|s| (i, s)))
        {
            let first = self.first_boundary(slot.range);
            if loops.exits[i].is_none_or(|exit| offset < i128::from(exit)) {
                local.loop_range = Some(slot.range);
                local.exit = loops.exits[i];
                return (local, offset);
            }
            offset -= i128::from(loops.exits[i].unwrap() - first);
        }
        (local, offset)
    }

    // Prepared candidates and validated expression changes supply this rate.
    pub(super) fn with_step(mut self, step: f64) -> Self {
        debug_assert!((MIN_STEP..=MAX_STEP).contains(&step));
        self.step = step;
        self
    }

    fn first_boundary(&self, loop_range: Loop) -> u64 {
        match self.direction {
            Direction::Forward => (loop_range.end - self.start) as u64,
            Direction::Reverse => (self.end - loop_range.start) as u64,
        }
    }

    pub(super) fn release(&mut self) {
        if let Some(mut loops) = self.loops {
            let mut offset = self.position;
            for (i, slot) in loops
                .slots
                .iter()
                .enumerate()
                .filter_map(|(i, s)| s.map(|s| (i, s)))
            {
                let mut local = *self;
                local.loops = None;
                local.loop_range = Some(slot.range);
                local.exit = loops.exits[i];
                local.position = offset;
                local.release();
                loops.exits[i] = local.exit;
                if let Some(exit) = local.exit {
                    offset = offset.saturating_sub(exit - local.first_boundary(slot.range));
                }
            }
            self.loops = Some(loops);
            return;
        }

        if let Some(r) = self.loop_range
            && r.mode == LoopMode::UntilRelease
            && self.exit.is_none_or(|exit| self.position < exit)
        {
            let first = self.first_boundary(r);
            let length = r.period();
            let distance = self.position.saturating_sub(first);
            let cycles = distance / length
                + u64::from(
                    !distance.is_multiple_of(length)
                        || self.fraction != 0.0 && self.position >= first,
                );
            let mut exit = first.saturating_add(cycles.saturating_mul(length));
            // Once a crossfade has begun, complete that wrap before the final pass.
            // This also retains its past interpolation guards at the exact boundary.
            if let LoopShape::Crossfade { frames } | LoopShape::EqualPowerCrossfade { frames } =
                r.shape
                && exit.saturating_sub(self.position) <= frames as u64
            {
                exit = exit.saturating_add(length);
            }
            self.exit = Some(self.exit.map_or(exit, |finite| finite.min(exit)));
        }
    }

    fn limit(&self) -> Option<u64> {
        let length = (self.end - self.start) as u64;
        if let Some(loops) = self.loops {
            let mut limit = length;
            for (i, slot) in loops
                .slots
                .iter()
                .enumerate()
                .filter_map(|(i, s)| s.map(|s| (i, s)))
            {
                limit = limit
                    .checked_add(loops.exits[i]?.checked_sub(self.first_boundary(slot.range))?)?;
            }
            return Some(limit);
        }
        self.loop_range.map_or(Some(length), |r| {
            self.exit
                .map(|exit| exit.saturating_add(length - self.first_boundary(r)))
        })
    }

    /// A fading miss completes its fade first; a waiting one ends with its source.
    pub(super) fn done(&self) -> bool {
        self.starvation.is_none_or(|remaining| remaining == 0)
            && (self.position == u64::MAX || self.limit().is_some_and(|end| self.position >= end))
    }

    pub(super) fn starved(&self) -> bool {
        self.starvation.is_some()
    }

    /// Start silent, waiting for its stream window (a cold start).
    pub(super) fn cold(mut self) -> Self {
        self.starvation = Some(0);
        self.cold_hold = self.fade_frames.saturating_mul(50);
        self
    }

    /// Faded out and silently waiting for its stream window.
    pub(super) fn waiting(&self) -> bool {
        self.starvation == Some(0)
    }

    /// One millisecond of native fade from the last complete resampled frame.
    /// No incomplete resampler frame is published. The cursor keeps advancing in
    /// time, so the source stays aligned with its envelope and demand.
    fn render_starvation(
        &mut self,
        output: &mut [Frame],
        envelope: &mut EnvelopeState,
        gain: f32,
        gains: [f32; 2],
    ) -> usize {
        let count = output
            .len()
            .min(self.starvation.unwrap() as usize)
            .min(envelope.remaining());
        for frame in &mut output[..count] {
            let remaining = self.starvation.as_mut().unwrap();
            *remaining -= 1;
            let fade = *remaining as f32 / self.fade_frames as f32;
            let level = envelope.constant_level().unwrap_or_else(|| envelope.next());
            for channel in 0..2 {
                frame[channel] += self.last[channel] * gain * gains[channel] * level * fade;
            }
            self.advance();
        }
        count
    }

    /// Fade out, then wait silently (still advancing) until the window at the
    /// cursor is resident again, then fade back in. Probed once per call.
    fn render_starved(
        &mut self,
        pcm: &(impl ReadFrames + ?Sized),
        output: &mut [Frame],
        envelope: &mut EnvelopeState,
        gain: f32,
        gains: [f32; 2],
        kernel: &Kernel,
    ) -> usize {
        if self.cold_hold > 0 {
            if self.sample(pcm, kernel).is_some() {
                self.cold_hold = 0;
            } else {
                let count = output.len().min(self.cold_hold as usize);
                self.cold_hold -= count as u32;
                // Hold both source and envelope: never skip the first transient.
                if count == output.len() {
                    return count;
                }
                return count
                    + self.render_starved(
                        pcm,
                        &mut output[count..],
                        envelope,
                        gain,
                        gains,
                        kernel,
                    );
            }
        }
        let count = if self.starvation != Some(0) {
            self.render_starvation(output, envelope, gain, gains)
        } else if self.done() || self.sample(pcm, kernel).is_none() {
            self.last = [0.; 2];
            return self.advance_silent(output.len(), envelope);
        } else {
            self.starvation = None;
            self.fade_in = self.fade_frames;
            0
        };
        if count == output.len() || envelope.remaining() == 0 {
            return count;
        }
        count + self.render(pcm, &mut output[count..], envelope, gain, gains, kernel)
    }

    /// Resolve an integer position on the traversal. Out-of-view guards are zero;
    /// loop guards follow the same repeated path as the cursor, in either direction.
    fn index(&self, position: i128) -> Option<usize> {
        if self.loops.is_some() {
            let (local, position) = self.segment(position);
            return local.index(position);
        }
        let mut offset = u64::try_from(position).ok()?;
        if let Some(r) = self.loop_range {
            let first = self.first_boundary(r);
            let length = (r.end - r.start) as u64;
            if let Some(exit) = self.exit
                && offset >= exit
            {
                offset = first.checked_add(offset - exit)?;
            } else if offset >= first {
                offset = first - length + r.span(offset - first).0;
            }
        }
        if offset >= (self.end - self.start) as u64 {
            return None;
        }
        Some(match self.direction {
            Direction::Forward => self.start + offset as usize,
            Direction::Reverse => self.end - 1 - offset as usize,
        })
    }

    /// Resolve both source legs once for rendering and residency prediction.
    fn address(&self, position: i128) -> Option<ReadAddress> {
        if self.loops.is_some() {
            let (local, position) = self.segment(position);
            return local.address(position);
        }
        let index = self.index(position)?;
        let Some(r) = self.loop_range else {
            return Some(ReadAddress {
                primary: index,
                crossfade: None,
            });
        };
        let (LoopShape::Crossfade { frames } | LoopShape::EqualPowerCrossfade { frames }) = r.shape
        else {
            return Some(ReadAddress {
                primary: index,
                crossfade: None,
            });
        };
        let offset = position as u64;
        let first = self.first_boundary(r);
        let length = (r.end - r.start) as u64;
        let remaining = if offset < first {
            first - offset
        } else {
            length - (offset - first) % length
        };
        if remaining > frames as u64
            || self
                .exit
                .is_some_and(|exit| offset >= exit || remaining >= exit - offset)
        {
            return Some(ReadAddress {
                primary: index,
                crossfade: None,
            });
        }
        let partner = match self.direction {
            Direction::Forward => index - length as usize,
            Direction::Reverse => index + length as usize,
        };
        let blend = (frames as u64 - remaining) as f64 / frames as f64;
        let gains = if let LoopShape::EqualPowerCrossfade { .. } = r.shape {
            let (sin, cos) = (blend * std::f64::consts::FRAC_PI_2).sin_cos();
            [cos, sin]
        } else {
            [1. - blend, blend]
        };
        Some(ReadAddress {
            primary: index,
            crossfade: Some((partner, gains)),
        })
    }

    /// Read the virtual source before resampling: both legs use the same phase.
    fn read(&self, pcm: &(impl ReadFrames + ?Sized), position: i128) -> Option<Frame> {
        match self.address(position) {
            None => Some([0.; 2]),
            Some(ReadAddress {
                primary,
                crossfade: None,
            }) => pcm.frame(primary),
            Some(ReadAddress {
                primary,
                crossfade: Some((partner, [ga, gb])),
            }) => {
                let a = pcm.frame(primary)?;
                let b = pcm.frame(partner)?;
                Some(std::array::from_fn(|channel| {
                    (ga * f64::from(a[channel]) + gb * f64::from(b[channel])) as f32
                }))
            }
        }
    }

    fn crossfaded(&self) -> bool {
        self.loops.is_some()
            || self.loop_range.is_some_and(|r| {
                matches!(
                    r.shape,
                    LoopShape::Crossfade { .. } | LoopShape::EqualPowerCrossfade { .. }
                )
            })
    }

    fn advance(&mut self) {
        let phase = self.fraction + self.step();
        // Truncation is floor here (phase >= 0) and, unlike f64::floor on the
        // SSE2 baseline, needs no libm call.
        let whole = (phase as i64) as f64;
        self.fraction = phase - whole;
        self.position = self.position.saturating_add(whole as u64);
    }

    #[inline]
    fn span(&self) -> (usize, usize, Direction) {
        let length = (self.end - self.start) as u64;
        let mut direction = self.direction;
        let (offset, count) = if let Some(r) = self.loop_range {
            let first = self.first_boundary(r);
            if let Some(exit) = self.exit
                && self.position >= exit
            {
                let offset = first + (self.position - exit);
                (offset, length - offset)
            } else if self.position < first {
                (self.position, first - self.position)
            } else {
                let loop_length = (r.end - r.start) as u64;
                let (within, count, reverse) = r.span(self.position - first);
                // Only a reflected span can cross an outward exit inside its
                // contiguous run. Wrap spans already stop at that boundary.
                let count = if r.shape == LoopShape::PingPong {
                    self.exit
                        .map_or(count, |exit| count.min(exit - self.position))
                } else {
                    count
                };
                if reverse {
                    direction = match direction {
                        Direction::Forward => Direction::Reverse,
                        Direction::Reverse => Direction::Forward,
                    };
                }
                (first - loop_length + within, count)
            }
        } else {
            (self.position, length - self.position)
        };
        let index = match self.direction {
            Direction::Forward => self.start + offset as usize,
            Direction::Reverse => self.end - 1 - offset as usize,
        };
        (index, count as usize, direction)
    }

    #[inline]
    pub(super) fn render(
        &mut self,
        pcm: &(impl ReadFrames + ?Sized),
        output: &mut [Frame],
        envelope: &mut EnvelopeState,
        gain: f32,
        gains: [f32; 2],
        kernel: &Kernel,
    ) -> usize {
        if self.starved() {
            return self.render_starved(pcm, output, envelope, gain, gains, kernel);
        }
        if gain == 0.0 || gains == [0.0; 2] {
            self.last = [0.; 2];
            return self.advance_silent(output.len(), envelope);
        }
        if self.step() == 1.0 && self.fraction == 0.0 && self.fade_in == 0 && !self.crossfaded() {
            let mut offset = 0;
            while offset < output.len() && !self.done() && !envelope.done() {
                let (index, count, direction) = self.span();
                let count = count.min(output.len() - offset).min(envelope.remaining());
                let range = match direction {
                    Direction::Forward => index..index + count,
                    Direction::Reverse => index + 1 - count..index + 1,
                };
                let Some(span) = pcm.span(range) else {
                    // A page boundary or miss uses the same scalar path. Only
                    // fully available frames commit cursor/envelope/output state.
                    return offset
                        + self.render_filtered(
                            pcm,
                            &mut output[offset..],
                            envelope,
                            gain,
                            gains,
                            kernel,
                        );
                };
                let destination = &mut output[offset..offset + count];
                match direction {
                    Direction::Forward => mix(span.iter(), destination, envelope, gain, gains),
                    Direction::Reverse => {
                        mix(span.iter().rev(), destination, envelope, gain, gains)
                    }
                }
                self.last = match direction {
                    Direction::Forward => *span.last().unwrap(),
                    Direction::Reverse => span[0],
                };
                self.position = self.position.saturating_add(count as u64);
                offset += count;
            }
            return offset;
        }
        self.render_filtered(pcm, output, envelope, gain, gains, kernel)
    }

    fn advance_silent(&mut self, frames: usize, envelope: &mut EnvelopeState) -> usize {
        if self.loops.is_none() && self.fraction == 0.0 && self.step().fract() == 0.0 {
            let step = self.step() as u64;
            let remaining = self
                .limit()
                .unwrap_or(u64::MAX)
                .saturating_sub(self.position);
            let count = frames
                .min(envelope.remaining())
                .min(usize::try_from(remaining.div_ceil(step)).unwrap_or(usize::MAX));
            for _ in 0..count {
                if envelope.constant_level().is_some() {
                    break;
                }
                envelope.next();
            }
            self.position = self
                .position
                .saturating_add((count as u64).saturating_mul(step));
            return count;
        }
        // Preserve the exact fractional recurrence, including rounding, rather
        // than replacing repeated addition with a partition-dependent product.
        let mut rendered = 0;
        for _ in 0..frames {
            if self.done() || envelope.done() {
                break;
            }
            if envelope.constant_level().is_none() {
                envelope.next();
            }
            self.advance();
            rendered += 1;
        }
        rendered
    }

    /// Resample from octave level floor(log2(step)) when its whole support maps
    /// to one contiguous run of the view. Seams fall back to the original frames.
    // ponytail: loops shorter than twice the reach (~150 frames at 2x, ~1.6k at
    // 16x) never take this path; per-level loop traversal if that matters.
    fn sample_level(&self, pcm: &(impl ReadFrames + ?Sized), kernel: &Kernel) -> Option<Frame> {
        let levels = pcm.levels();
        if levels.is_empty() || self.step() < 2.0 || self.crossfaded() {
            return None;
        }
        let k = (self.step().log2().floor() as usize).min(levels.len());
        if k == 0 {
            return None;
        }
        let scale = 1_i64 << k;
        let step = self.step() / scale as f64;
        let radius = kernel.window(step);
        let reach = DECIMATION_REACH * (scale - 1) + (radius + 2) * scale;
        let position = i128::from(self.position);
        let left = self.index(position - i128::from(reach))?;
        let right = self.index(position + i128::from(reach))?;
        if left.abs_diff(right) != 2 * reach as usize {
            return None;
        }
        // Physical direction of this run (a reflected ping-pong span reverses).
        let index = self.index(position)? as f64;
        let x = if right > left {
            index + self.fraction
        } else {
            index - self.fraction
        } / scale as f64;
        let j = x.floor();
        let first = usize::try_from(j as i64 - radius).ok()?;
        let taps = levels[k - 1].get(first..=first + 2 * radius as usize)?;
        Some(kernel.sample_window(x - j, step, taps))
    }

    /// One complete output frame at the cursor, or None if any tap is missing.
    fn sample(&self, pcm: &(impl ReadFrames + ?Sized), kernel: &Kernel) -> Option<Frame> {
        let position = i128::from(self.position);
        if self.step() == 1.0 && self.fraction == 0.0 {
            self.read(pcm, position)
        } else if let Some(sample) = self.sample_level(pcm, kernel) {
            Some(sample)
        } else {
            let radius = kernel.window(self.step());
            let left = self.index(position - i128::from(radius));
            let right = self.index(position + i128::from(radius));
            let contiguous = if self.crossfaded() {
                None
            } else {
                match (left, right) {
                    (Some(left), Some(right))
                        if right.checked_sub(left) == Some(2 * radius as usize) =>
                    {
                        pcm.span(left..right + 1).map(|s| (s, false))
                    }
                    (Some(left), Some(right))
                        if left.checked_sub(right) == Some(2 * radius as usize) =>
                    {
                        pcm.span(right..left + 1).map(|s| (s, true))
                    }
                    _ => None,
                }
            };
            if let Some((span, false)) = contiguous {
                Some(kernel.sample_window(self.fraction, self.step(), span))
            } else if let Some((span, reverse)) = contiguous {
                Some(kernel.sample(self.fraction, self.step(), |offset| {
                    span[(if reverse {
                        radius - offset
                    } else {
                        offset + radius
                    }) as usize]
                }))
            } else {
                let mut ready = true;
                let sample = kernel.sample(self.fraction, self.step(), |offset| {
                    self.read(pcm, position + i128::from(offset))
                        .unwrap_or_else(|| {
                            ready = false;
                            [0.; 2]
                        })
                });
                ready.then_some(sample)
            }
        }
    }

    /// Frames whose kernel windows all lie in one forward contiguous span,
    /// resolved once; each frame then slices that span directly. Identical
    /// arithmetic to the per-frame path, which handles everything else.
    // ponytail: reverse spans and octave levels stay per-frame; add runs for
    // them if reverse or mipmapped pitched voices dominate a profile.
    fn render_run(
        &mut self,
        pcm: &(impl ReadFrames + ?Sized),
        output: &mut [Frame],
        envelope: &mut EnvelopeState,
        gain: f32,
        gains: [f32; 2],
        kernel: &Kernel,
    ) -> usize {
        if self.loops.is_some() {
            return 0;
        }
        if self.step() >= 2.0 && !pcm.levels().is_empty() {
            return 0;
        }
        let mut count = output.len().min(envelope.remaining());
        let radius = kernel.window(self.step());
        let bank = kernel.polyphase(self.step());
        let taps = 2 * radius as usize + 1;
        // Frames read per output frame: the bank reads whole chunks.
        let width = bank.map_or(taps, |bank| bank.width());
        // Stop the run short of the next boundary (loop end or crossfade)
        // instead of rendering every frame before it one at a time.
        let (_, contiguous, _) = self.span();
        let fade = match self.loop_range.map(|r| r.shape) {
            Some(LoopShape::Crossfade { frames } | LoopShape::EqualPowerCrossfade { frames }) => {
                frames
            }
            _ => 0,
        };
        let margin = radius as usize + width - taps + fade + 2;
        if let Some(room) = contiguous.checked_sub(margin) {
            count = count.min((room as f64 / self.step()) as usize);
        }
        // Upper bound on the frames advanced by `count` steps.
        let advance = (count as f64 * self.step()).ceil() as i64 + 1;
        let position = i128::from(self.position);
        let (Some(left), Some(right)) = (
            self.index(position - i128::from(radius)),
            self.index(position + i128::from(advance + radius) + (width - taps) as i128),
        ) else {
            return 0;
        };
        if count == 0 || right.checked_sub(left) != Some(advance as usize + width - 1) {
            return 0;
        }
        // A window without a wrap lies within one loop pass, whose crossfade
        // is at its end: clear at both ends means clear throughout.
        if self.crossfaded()
            && [
                position - i128::from(radius),
                position + i128::from(advance + radius) + (width - taps) as i128,
            ]
            .into_iter()
            .any(|p| self.address(p).is_some_and(|a| a.crossfade.is_some()))
        {
            return 0;
        }
        // A window across pages or resident ranges is gathered on the stack.
        let mut gathered = [[0.0f32; 2]; GATHER];
        let span = match pcm.span(left..right + 1) {
            Some(span) => span,
            None if right + 1 - left <= GATHER => {
                let out = &mut gathered[..right + 1 - left];
                if !pcm.copy(left..right + 1, out) {
                    return 0;
                }
                &*out
            }
            None => return 0,
        };
        let output = &mut output[..count];
        match bank {
            Some(bank) => sampler_simd::dispatch_fused(
                (self, output, envelope),
                #[inline(always)]
                |(this, output, envelope)| {
                    this.run(
                        span,
                        width,
                        output,
                        envelope,
                        gain,
                        gains,
                        #[inline(always)]
                        |f, w| bank.dot::<false>(f, w),
                    )
                },
                #[inline(always)]
                |(this, output, envelope)| {
                    this.run(
                        span,
                        width,
                        output,
                        envelope,
                        gain,
                        gains,
                        #[inline(always)]
                        |f, w| bank.dot::<true>(f, w),
                    )
                },
            ),
            None => {
                let step = self.step();
                self.run(span, width, output, envelope, gain, gains, |f, w| {
                    kernel.sample_window(f, step, w)
                })
            }
        }
        count
    }

    /// The [`Self::render_run`] frame loop over one contiguous span, with the
    /// cursor in locals; same arithmetic as [`Self::advance`].
    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    fn run(
        &mut self,
        span: &[Frame],
        width: usize,
        output: &mut [Frame],
        envelope: &mut EnvelopeState,
        gain: f32,
        gains: [f32; 2],
        sample: impl Fn(f64, &[Frame]) -> Frame,
    ) {
        let (mut fraction, step, mut offset, mut last) = (self.fraction, self.step(), 0, self.last);
        for frame in output {
            let source = sample(fraction, &span[offset..offset + width]);
            last = if source.iter().all(|value| value.is_finite()) {
                source
            } else {
                [0.; 2]
            };
            let level = envelope.constant_level().unwrap_or_else(|| envelope.next());
            for channel in 0..2 {
                frame[channel] += source[channel] * gain * gains[channel] * level;
            }
            let phase = fraction + step;
            let whole = phase as i64;
            fraction = phase - whole as f64;
            offset += whole as usize;
        }
        self.fraction = fraction;
        self.position += offset as u64;
        self.last = last;
    }

    fn render_filtered(
        &mut self,
        pcm: &(impl ReadFrames + ?Sized),
        output: &mut [Frame],
        envelope: &mut EnvelopeState,
        gain: f32,
        gains: [f32; 2],
        kernel: &Kernel,
    ) -> usize {
        let mut rendered = 0;
        while rendered < output.len() {
            if self.done() || envelope.done() {
                break;
            }
            if self.fade_in == 0 {
                let run =
                    self.render_run(pcm, &mut output[rendered..], envelope, gain, gains, kernel);
                if run > 0 {
                    rendered += run;
                    continue;
                }
            }
            let Some(source) = self.sample(pcm, kernel) else {
                self.starvation = Some(self.fade_frames);
                return rendered
                    + self.render_starved(
                        pcm,
                        &mut output[rendered..],
                        envelope,
                        gain,
                        gains,
                        kernel,
                    );
            };
            let source = if self.fade_in > 0 {
                self.fade_in -= 1;
                let ramp = 1. - self.fade_in as f32 / self.fade_frames as f32;
                source.map(|value| value * ramp)
            } else {
                source
            };
            // The existing mix fault guard still observes an overflowing kernel;
            // never retain that nonfinite value as a later starvation tail.
            self.last = if source.iter().all(|value| value.is_finite()) {
                source
            } else {
                [0.; 2]
            };
            let frame = &mut output[rendered];
            let level = envelope.constant_level().unwrap_or_else(|| envelope.next());
            for channel in 0..2 {
                frame[channel] += source[channel] * gain * gains[channel] * level;
            }
            self.advance();
            rendered += 1;
        }
        rendered
    }
}

fn mix<'a>(
    input: impl Iterator<Item = &'a Frame>,
    output: &mut [Frame],
    envelope: &mut EnvelopeState,
    gain: f32,
    gains: [f32; 2],
) {
    let constant = envelope.constant_level();
    // Preserve the existing unity fast path: a general constant level otherwise
    // adds an unnecessary multiply to every sample in the common default case.
    if constant == Some(1.0) {
        for (frame, source) in output.iter_mut().zip(input) {
            for channel in 0..2 {
                frame[channel] += source[channel] * gain * gains[channel];
            }
        }
        return;
    }
    for (frame, source) in output.iter_mut().zip(input) {
        let level = constant.unwrap_or_else(|| envelope.next());
        for channel in 0..2 {
            frame[channel] += source[channel] * gain * gains[channel] * level;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_preserve_source_time_and_bounds_without_advancing_loop_passes() {
        for direction in [Direction::Forward, Direction::Reverse] {
            let cursor = Playback {
                start: 8,
                end: Some(40),
                direction,
                loop_range: Some(Loop {
                    start: 16,
                    end: 32,
                    shape: LoopShape::Wrap,
                    mode: LoopMode::UntilRelease,
                    passes: None,
                }),
                ..Playback::default()
            }
            .cursor(48, 44100, 48000)
            .unwrap();
            let fractional = cursor.with_offset(125, 44100);
            assert_eq!(fractional.position, 5);
            assert_eq!(fractional.fraction, 0.5125);
            assert_eq!(fractional.step(), cursor.step());
            assert_eq!(
                fractional.index(5),
                Some(if direction == Direction::Forward {
                    13
                } else {
                    34
                })
            );
            let mut inside = cursor.with_offset(400, 48000);
            assert_eq!((inside.position, inside.fraction), (19, 0.2));
            assert!(inside.unbounded_loop());
            inside.release();
            assert_eq!(inside.exit, Some(24));
            let past = cursor.with_offset(500, 48000);
            assert!(!past.unbounded_loop());
            assert_eq!(
                past.index(24),
                Some(if direction == Direction::Forward {
                    32
                } else {
                    15
                })
            );
            for offset in [1_000, u32::MAX] {
                let end = cursor.with_offset(offset, u32::MAX);
                assert!(end.done());
                assert_eq!(end.index(i128::from(end.position)), None);
            }
        }
    }

    #[test]
    fn fractional_phase_survives_large_positions_and_guard_mapping() {
        let mut cursor = Playback {
            loop_range: Some(Loop {
                passes: None,
                start: 0,
                end: 1,
                shape: LoopShape::Wrap,
                mode: LoopMode::Continuous,
            }),
            ..Playback::default()
        }
        .cursor(1, 12000, 48000)
        .unwrap();
        cursor.position = (1_u64 << 54) + 3;
        let origin = cursor.position;
        for fraction in [0.25, 0.5, 0.75] {
            cursor.advance();
            assert_eq!((cursor.position, cursor.fraction), (origin, fraction));
            assert_eq!(cursor.index(i128::from(cursor.position) + 768), Some(0));
        }
        cursor.advance();
        assert_eq!((cursor.position, cursor.fraction), (origin + 1, 0.0));
        cursor.position = u64::MAX;
        assert!(cursor.done());
    }

    #[test]
    fn reflected_guards_keep_integer_identity_beyond_float_precision_and_reject_period_overflow() {
        for direction in [Direction::Forward, Direction::Reverse] {
            let mut cursor = Playback {
                direction,
                loop_range: Some(Loop {
                    passes: None,
                    start: 0,
                    end: 4,
                    mode: LoopMode::Continuous,
                    shape: LoopShape::PingPong,
                }),
                ..Playback::default()
            }
            .cursor(4, 12000, 48000)
            .unwrap();
            cursor.position = (1_u64 << 54) + 3;
            for _ in 0..8 {
                cursor.advance();
                for guard in -768..=768 {
                    let at = i128::from(cursor.position) + guard;
                    let expected = [2, 1, 0, 1, 2, 3][((at - 4) % 6) as usize];
                    assert_eq!(
                        cursor.index(at),
                        Some(if direction == Direction::Forward {
                            expected
                        } else {
                            3 - expected
                        })
                    );
                }
            }
            assert_eq!((cursor.position, cursor.fraction), ((1_u64 << 54) + 5, 0.));
        }
        if let Ok(length) = usize::try_from(u64::MAX / 2 + 2) {
            let playback = Playback {
                loop_range: Some(Loop {
                    passes: None,
                    start: 0,
                    end: length,
                    mode: LoopMode::Continuous,
                    shape: LoopShape::PingPong,
                }),
                ..Playback::default()
            };
            assert!(matches!(
                playback.cursor(length, 48000, 48000),
                Err(Error::InvalidInput)
            ));
        }
    }
    #[test]
    fn counted_exit_can_shorten_but_never_extend_or_restart_and_rejects_clock_overflow() {
        for shape in [LoopShape::Wrap, LoopShape::PingPong] {
            for direction in [Direction::Forward, Direction::Reverse] {
                let playback = Playback {
                    direction,
                    loop_range: Some(Loop {
                        start: 2,
                        end: 5,
                        mode: LoopMode::UntilRelease,
                        shape,
                        passes: std::num::NonZeroU32::new(3),
                    }),
                    ..Playback::default()
                };
                let original = playback.cursor(8, 48000, 48000).unwrap();
                let range = playback.loop_range.unwrap();
                let first = original.first_boundary(range);
                let exit = original.exit.unwrap();
                assert_eq!(exit, first + 2 * range.period());
                for (position, fraction, expected) in [
                    (0, 0., first),
                    (first, 0., first),
                    (first, 0.5, first + range.period()),
                    (exit - 1, 0.5, exit),
                    (exit, 0., exit),
                    (exit + 1, 0., exit),
                ] {
                    let mut cursor = original;
                    cursor.position = position;
                    cursor.fraction = fraction;
                    cursor.release();
                    assert_eq!(cursor.exit, Some(expected));
                    assert_eq!(cursor.limit(), Some(expected + 8 - first));
                    let saved = cursor.exit;
                    cursor.release();
                    assert_eq!(cursor.exit, saved);
                }
            }
        }
        if let Ok(end) = usize::try_from(u64::MAX / 2 + 1) {
            for count in [2, 3] {
                let playback = Playback {
                    loop_range: Some(Loop {
                        start: 0,
                        end,
                        shape: LoopShape::Wrap,
                        mode: LoopMode::Continuous,
                        passes: std::num::NonZeroU32::new(count),
                    }),
                    ..Playback::default()
                };
                assert!(matches!(
                    playback.cursor(end, 48000, 48000),
                    Err(Error::InvalidInput)
                ));
            }
        }
    }

    #[test]
    fn crossfade_laws_keep_amplitude_or_power_complementary() {
        for shape in [
            LoopShape::Crossfade { frames: 4 },
            LoopShape::EqualPowerCrossfade { frames: 4 },
        ] {
            let cursor = Playback {
                loop_range: Some(Loop {
                    start: 8,
                    end: 16,
                    mode: LoopMode::Continuous,
                    shape,
                    passes: None,
                }),
                ..Playback::default()
            }
            .cursor(24, 48000, 48000)
            .unwrap();
            for position in 12..16 {
                let (partner, [a, b]) = cursor.address(position).unwrap().crossfade.unwrap();
                assert_eq!(partner, position as usize - 8);
                let sum = match shape {
                    LoopShape::Crossfade { .. } => a + b,
                    _ => a * a + b * b,
                };
                assert!((sum - 1.).abs() < 1e-12, "{shape:?} {position}");
            }
            let [a, b] = cursor.address(14).unwrap().crossfade.unwrap().1;
            assert!((a - b).abs() < 1e-12);
        }
    }

    #[test]
    fn crossfade_release_finishes_entered_wraps_and_keeps_every_past_guard() {
        let pcm: Vec<Frame> = (0..24)
            .map(|i| [i as f32 / 24., -(i * 7 % 19) as f32 / 20.])
            .collect();
        for direction in [Direction::Forward, Direction::Reverse] {
            for passes in [
                None,
                std::num::NonZeroU32::new(2),
                std::num::NonZeroU32::new(5),
            ] {
                let initial = Playback {
                    direction,
                    loop_range: Some(Loop {
                        start: 8,
                        end: 16,
                        mode: LoopMode::UntilRelease,
                        shape: LoopShape::Crossfade { frames: 4 },
                        passes,
                    }),
                    ..Playback::default()
                }
                .cursor(24, 48000, 48000)
                .unwrap();
                for position in [0, 11, 12, 15, 16, 17, 19, 20, 23, 24, 25, 28, 32] {
                    for fraction in [0., 0.5] {
                        let mut cursor = Cursor {
                            position,
                            fraction,
                            ..initial
                        };
                        let before = cursor;
                        cursor.release();
                        let boundary = (16..).step_by(8).find(|&b| b > position + 4).unwrap();
                        let expected_exit = initial.exit.map_or(boundary, |end| end.min(boundary));
                        assert_eq!(cursor.exit, Some(expected_exit));
                        cursor.release();
                        assert_eq!(
                            cursor.exit,
                            Some(expected_exit),
                            "repeated release cannot extend"
                        );
                        for i in 0..=position {
                            assert_eq!(
                                cursor.read(pcm.as_slice(), i128::from(i)),
                                before.read(pcm.as_slice(), i128::from(i))
                            );
                        }
                        let ordered: Vec<_> = match direction {
                            Direction::Forward => pcm.clone(),
                            Direction::Reverse => pcm.iter().rev().copied().collect(),
                        };
                        let count = (expected_exit - 16) / 8 + 1;
                        let mut expected = ordered[..8].to_vec();
                        for pass in 0..count {
                            for i in 0..8 {
                                let mut value = ordered[8 + i];
                                if pass + 1 < count && i >= 4 {
                                    let blend = (i - 4) as f64 / 4.;
                                    for channel in 0..2 {
                                        value[channel] = ((1. - blend) * f64::from(value[channel])
                                            + blend * f64::from(ordered[i][channel]))
                                            as f32;
                                    }
                                }
                                expected.push(value);
                            }
                        }
                        expected.extend_from_slice(&ordered[16..]);
                        for i in -4..expected.len() as i128 + 4 {
                            let frame = usize::try_from(i)
                                .ok()
                                .and_then(|i| expected.get(i))
                                .copied()
                                .unwrap_or([0.; 2]);
                            assert_eq!(
                                cursor.read(pcm.as_slice(), i).unwrap(),
                                frame,
                                "{direction:?}, {position}, {fraction}, {passes:?}, read {i}"
                            );
                        }
                    }
                }
            }
        }
    }
}

mod demand;
pub use demand::SampleDemand;

/// Borrowed decoded data. Missing physical frames differ from out-of-view zeros.
/// Largest window `render_run` gathers across storage boundaries.
const GATHER: usize = 1024;

pub(super) trait ReadFrames {
    fn frame(&self, index: usize) -> Option<Frame>;
    fn span(&self, range: std::ops::Range<usize>) -> Option<&[Frame]>;
    /// Copy `range` into `out` (its length), across storage boundaries;
    /// `false` if any frame is missing.
    fn copy(&self, range: std::ops::Range<usize>, out: &mut [Frame]) -> bool {
        match self.span(range.clone()) {
            Some(span) => out.copy_from_slice(span),
            None => {
                for (index, frame) in range.zip(out) {
                    let Some(value) = self.frame(index) else {
                        return false;
                    };
                    *frame = value;
                }
            }
        }
        true
    }
    /// Pre-decimated octave levels, level 1 first.
    fn levels(&self) -> &[Box<[Frame]>] {
        &[]
    }
}
/// Resident frames with any octave levels.
pub(super) struct Resident<'a> {
    pub frames: &'a [Frame],
    pub levels: &'a [Box<[Frame]>],
}
impl ReadFrames for Resident<'_> {
    fn frame(&self, index: usize) -> Option<Frame> {
        self.frames.frame(index)
    }
    fn span(&self, range: std::ops::Range<usize>) -> Option<&[Frame]> {
        self.frames.span(range)
    }
    fn levels(&self) -> &[Box<[Frame]>] {
        self.levels
    }
}
impl ReadFrames for [Frame] {
    fn frame(&self, index: usize) -> Option<Frame> {
        self.get(index).copied()
    }
    fn span(&self, range: std::ops::Range<usize>) -> Option<&[Frame]> {
        self.get(range)
    }
}
/// The asset's resident pages, then cache pages.
pub(super) struct PagedFrames<'a> {
    pub cache: crate::stream::PageReader<'a>,
    pub asset: crate::AssetId,
    pub head: &'a [(usize, crate::Packed)],
}
impl PagedFrames<'_> {
    /// The resident range holding frame `index`, and its first frame.
    fn range(&self, index: usize) -> Option<(usize, &crate::Packed)> {
        let i = self.head.partition_point(|(start, _)| *start <= index);
        let (start, frames) = self.head.get(i.checked_sub(1)?)?;
        (index - start < frames.len()).then_some((*start, frames))
    }
}
impl ReadFrames for PagedFrames<'_> {
    fn frame(&self, index: usize) -> Option<Frame> {
        match self.range(index) {
            Some((start, frames)) => Some(frames.frame(index - start)),
            None => self.cache.frame(self.asset, index),
        }
    }
    fn span(&self, range: std::ops::Range<usize>) -> Option<&[Frame]> {
        if !range.is_empty()
            && let Some((start, frames)) = self.range(range.start)
        {
            // Packed ranges convert through `copy`.
            return frames.frames()?.get(range.start - start..range.end - start);
        }
        self.cache.span(self.asset, range)
    }
    fn copy(&self, range: std::ops::Range<usize>, out: &mut [Frame]) -> bool {
        let mut at = range.start;
        while at < range.end {
            let o = at - range.start;
            let (part, end) = match self.range(at) {
                Some((start, frames)) => {
                    let end = (start + frames.len()).min(range.end);
                    frames.copy(at - start, &mut out[o..o + end - at]);
                    at = end;
                    continue;
                }
                None => {
                    // Up to the page end or the next resident range.
                    let next = self.head.partition_point(|(start, _)| *start <= at);
                    let end = ((at / crate::PAGE_FRAMES + 1) * crate::PAGE_FRAMES)
                        .min(range.end)
                        .min(self.head.get(next).map_or(usize::MAX, |(start, _)| *start));
                    match self.cache.span(self.asset, at..end) {
                        Some(part) => (part, end),
                        None => return false,
                    }
                }
            };
            out[o..o + part.len()].copy_from_slice(part);
            at = end;
        }
        true
    }
}

#[cfg(test)]
mod native_slot_tests {
    use super::*;
    #[test]
    fn tuning_applies_on_repetitions_and_finite_exit_restores_the_base_rate() {
        let mut slots = [None; 8];
        slots[7] = Some(LoopSlot {
            range: Loop {
                start: 2,
                end: 4,
                mode: LoopMode::Continuous,
                shape: LoopShape::Wrap,
                passes: std::num::NonZeroU32::new(3),
            },
            tuning: 2.,
        });
        let mut c = Playback {
            loop_slots: slots,
            ..Default::default()
        }
        .cursor(8, 48000, 48000)
        .unwrap();
        let mut got = Vec::new();
        while !c.done() {
            got.push((c.index(i128::from(c.position)).unwrap(), c.step()));
            c.advance();
        }
        assert_eq!(
            got,
            vec![
                (0, 1.),
                (1, 1.),
                (2, 1.),
                (3, 1.),
                (2, 2.),
                (2, 2.),
                (4, 1.),
                (5, 1.),
                (6, 1.),
                (7, 1.)
            ]
        );
    }
    #[test]
    fn release_exits_the_current_slot_and_skips_future_until_release_slots() {
        let mut slots = [None; 8];
        for (slot, start) in [(1, 1), (7, 4)] {
            slots[slot] = Some(LoopSlot {
                range: Loop {
                    start,
                    end: start + 2,
                    mode: LoopMode::UntilRelease,
                    shape: LoopShape::Wrap,
                    passes: None,
                },
                tuning: 1.,
            });
        }
        let mut c = Playback {
            loop_slots: slots,
            ..Default::default()
        }
        .cursor(8, 48000, 48000)
        .unwrap();
        for _ in 0..4 {
            c.advance();
        }
        c.release();
        let mut got = Vec::new();
        while !c.done() {
            got.push(c.index(i128::from(c.position)).unwrap());
            c.advance();
        }
        assert_eq!(got, [2, 3, 4, 5, 6, 7]);
    }
}
