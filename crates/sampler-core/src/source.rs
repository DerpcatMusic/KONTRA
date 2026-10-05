//! Immutable per-region playback metadata and independent per-voice cursors.
use crate::{Error, Frame, envelope::EnvelopeState};

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

/// A region's view into shared PCM. Offsets and loop bounds are absolute sample
/// frame indices. `None` end means source EOF. Validation occurs before admission.
#[derive(Clone, Copy, Debug, Default)]
pub struct Playback {
    pub start: usize,
    pub end: Option<usize>,
    pub direction: Direction,
    pub loop_range: Option<Loop>,
}

impl Playback {
    pub(super) fn cursor(self, frames: usize) -> Result<Cursor, Error> {
        let end = self.end.unwrap_or(frames);
        if self.start >= end
            || end > frames
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
            position: if self.direction == Direction::Forward {
                self.start
            } else {
                end
            },
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Cursor {
    start: usize,
    end: usize,
    direction: Direction,
    loop_range: Option<Loop>,
    // Forward: next frame; reverse: exclusive upper end of the next span.
    position: usize,
}

impl Cursor {
    pub(super) fn release(&mut self) {
        if self
            .loop_range
            .is_some_and(|r| r.mode == LoopMode::UntilRelease)
        {
            self.loop_range = None;
        }
    }

    pub(super) fn done(&self) -> bool {
        self.loop_range.is_none()
            && self.position
                == match self.direction {
                    Direction::Forward => self.end,
                    Direction::Reverse => self.start,
                }
    }

    pub(super) fn render(
        &mut self,
        pcm: &[Frame],
        output: &mut [Frame],
        envelope: &mut EnvelopeState,
        gain: f32,
        gains: [f32; 2],
    ) {
        let mut offset = 0;
        // Each pass consumes at least one output frame. One-frame loops are bounded
        // by the supplied block length, never by a user-controlled repeat count.
        while offset < output.len() && !self.done() && !envelope.done() {
            let (begin, end) = self
                .loop_range
                .map_or((self.start, self.end), |r| (r.start, r.end));
            // Defer wrap until the next read: release at the exact loop boundary
            // must proceed into the source tail rather than repeat an extra cycle.
            let count = match self.direction {
                Direction::Forward => {
                    if self.position == end {
                        self.position = begin;
                    }
                    end - self.position
                }
                Direction::Reverse => {
                    if self.position == begin {
                        self.position = end;
                    }
                    self.position - begin
                }
            }
            .min(output.len() - offset)
            .min(envelope.remaining());
            let destination = &mut output[offset..offset + count];
            match self.direction {
                Direction::Forward => {
                    mix(
                        pcm[self.position..self.position + count].iter(),
                        destination,
                        envelope,
                        gain,
                        gains,
                    );
                    self.position += count;
                }
                Direction::Reverse => {
                    mix(
                        pcm[self.position - count..self.position].iter().rev(),
                        destination,
                        envelope,
                        gain,
                        gains,
                    );
                    self.position -= count;
                }
            }
            offset += count;
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
