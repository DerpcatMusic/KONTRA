//! Immutable per-region playback metadata and independent per-voice cursors.
use crate::{
    Error, Frame,
    envelope::EnvelopeState,
    resample::{Kernel, MAX_STEP, MIN_STEP},
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

/// Half-open sample-frame range. One-frame loops are valid.
#[derive(Clone, Copy, Debug)]
pub struct Loop {
    pub start: usize,
    pub end: usize,
    pub mode: LoopMode,
}

/// Immutable source view. Transposition combines with the asset/output rate ratio.
#[derive(Clone, Copy, Debug, Default)]
pub struct Playback {
    pub start: usize,
    pub end: Option<usize>,
    pub direction: Direction,
    pub loop_range: Option<Loop>,
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
            || self
                .loop_range
                .is_some_and(|r| r.start < self.start || r.start >= r.end || r.end > end)
        {
            return Err(Error::InvalidInput);
        }
        Ok(Cursor {
            start: self.start,
            end,
            direction: self.direction,
            loop_range: self.loop_range,
            position: 0,
            fraction: 0.0,
            step,
            exit: None,
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Cursor {
    start: usize,
    end: usize,
    direction: Direction,
    loop_range: Option<Loop>,
    // Distance along the traversal, not an absolute floating-point PCM index.
    position: u64,
    fraction: f64,
    step: f64,
    // First loop boundary that will no longer wrap. Earlier traversal remains
    // available to the interpolation window after release.
    exit: Option<u64>,
}

impl Cursor {
    pub(super) fn step(&self) -> f64 {
        self.step
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
        if let Some(r) = self.loop_range
            && r.mode == LoopMode::UntilRelease
            && self.exit.is_none()
        {
            let first = self.first_boundary(r);
            let length = (r.end - r.start) as u64;
            let distance = self.position.saturating_sub(first);
            let cycles = distance / length
                + u64::from(
                    !distance.is_multiple_of(length)
                        || self.fraction != 0.0 && self.position >= first,
                );
            self.exit = Some(first.saturating_add(cycles.saturating_mul(length)));
        }
    }

    fn limit(&self) -> Option<u64> {
        let length = (self.end - self.start) as u64;
        self.loop_range.map_or(Some(length), |r| {
            self.exit
                .map(|exit| exit.saturating_add(length - self.first_boundary(r)))
        })
    }

    pub(super) fn done(&self) -> bool {
        self.position == u64::MAX || self.limit().is_some_and(|end| self.position >= end)
    }

    /// Resolve an integer position on the traversal. Out-of-view guards are zero;
    /// loop guards follow the same repeated path as the cursor, in either direction.
    fn index(&self, position: i128) -> Option<usize> {
        let mut offset = u64::try_from(position).ok()?;
        if let Some(r) = self.loop_range {
            let first = self.first_boundary(r);
            let length = (r.end - r.start) as u64;
            if let Some(exit) = self.exit
                && offset >= exit
            {
                offset = first.checked_add(offset - exit)?;
            } else if offset >= first {
                offset = first - length + (offset - first) % length;
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

    fn advance(&mut self) {
        let phase = self.fraction + self.step;
        let whole = phase.floor();
        self.fraction = phase - whole;
        self.position = self.position.saturating_add(whole as u64);
    }

    fn span(&self) -> (usize, usize) {
        let length = (self.end - self.start) as u64;
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
                let within = (self.position - first) % loop_length;
                (first - loop_length + within, loop_length - within)
            }
        } else {
            (self.position, length - self.position)
        };
        let index = match self.direction {
            Direction::Forward => self.start + offset as usize,
            Direction::Reverse => self.end - 1 - offset as usize,
        };
        (index, count as usize)
    }

    #[inline]
    pub(super) fn render(
        &mut self,
        pcm: &[Frame],
        output: &mut [Frame],
        envelope: &mut EnvelopeState,
        gain: f32,
        gains: [f32; 2],
        kernel: &Kernel,
    ) {
        if self.step == 1.0 && self.fraction == 0.0 {
            let mut offset = 0;
            while offset < output.len() && !self.done() && !envelope.done() {
                let (index, count) = self.span();
                let count = count.min(output.len() - offset).min(envelope.remaining());
                let destination = &mut output[offset..offset + count];
                match self.direction {
                    Direction::Forward => mix(
                        pcm[index..index + count].iter(),
                        destination,
                        envelope,
                        gain,
                        gains,
                    ),
                    Direction::Reverse => mix(
                        pcm[index + 1 - count..=index].iter().rev(),
                        destination,
                        envelope,
                        gain,
                        gains,
                    ),
                }
                self.position = self.position.saturating_add(count as u64);
                offset += count;
            }
            return;
        }
        self.render_filtered(pcm, output, envelope, gain, gains, kernel);
    }

    fn render_filtered(
        &mut self,
        pcm: &[Frame],
        output: &mut [Frame],
        envelope: &mut EnvelopeState,
        gain: f32,
        gains: [f32; 2],
        kernel: &Kernel,
    ) {
        for frame in output {
            if self.done() || envelope.done() {
                break;
            }
            let radius = Kernel::radius(self.step);
            let position = i128::from(self.position);
            let left = self.index(position - i128::from(radius));
            let right = self.index(position + i128::from(radius));
            let source = match (left, right, self.direction) {
                (Some(left), Some(right), Direction::Forward)
                    if right.checked_sub(left) == Some(2 * radius as usize) =>
                {
                    let span = &pcm[left..=right];
                    kernel.sample(self.fraction, self.step, |offset| {
                        span[(offset + radius) as usize]
                    })
                }
                (Some(left), Some(right), Direction::Reverse)
                    if left.checked_sub(right) == Some(2 * radius as usize) =>
                {
                    let span = &pcm[right..=left];
                    kernel.sample(self.fraction, self.step, |offset| {
                        span[(radius - offset) as usize]
                    })
                }
                _ => kernel.sample(self.fraction, self.step, |offset| {
                    self.index(position + i128::from(offset))
                        .map_or([0.0; 2], |i| pcm[i])
                }),
            };
            let level = envelope.constant_level().unwrap_or_else(|| envelope.next());
            for channel in 0..2 {
                frame[channel] += source[channel] * gain * gains[channel] * level;
            }
            self.advance();
        }
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
    fn fractional_phase_survives_large_positions_and_guard_mapping() {
        let mut cursor = Playback {
            loop_range: Some(Loop {
                start: 0,
                end: 1,
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
}
