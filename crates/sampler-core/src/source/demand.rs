use super::Cursor;
use crate::{Action, AssetId, EnvelopeState, Error, Runtime, VoiceId};
use std::ops::Range;

/// A borrowed demand description: it does not pin a plan or transfer sample data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SampleDemand {
    pub asset: AssetId,
    pub frames: Range<usize>,
    /// First output frame that requires this range, on the engine sample clock.
    pub deadline: u64,
}

impl Runtime {
    /// Predict reads from the current voice snapshot over a bounded output horizon.
    /// Includes muted sources, interpolation guards and both crossfade legs. Future
    /// control/release commands are not executed or guessed; requery after changes.
    /// The visitor can stop at queue capacity: `false` reports incomplete demand.
    /// This read-only query does not settle already-due events or advance playback.
    pub fn visit_voice_demand(
        &self,
        voice: VoiceId,
        frames: u32,
        mut accept: impl FnMut(SampleDemand) -> bool,
    ) -> Result<bool, Error> {
        let end = self
            .now
            .checked_add(u64::from(frames))
            .ok_or(Error::ClockOverflow)?;
        let v = self.voices.get(voice.0).ok_or(Error::StaleHandle)?;
        if v.cursor.starved() {
            return Ok(true);
        }
        let family = self.families.get(v.family.0).unwrap();
        let note = self.notes.get(family.note.0).unwrap();
        let at = if v.started {
            self.now
        } else {
            self.commands
                .iter()
                .find_map(|command| {
                    matches!(command.action, Action::Start(id) if id == voice).then_some(command.at)
                })
                .expect("a pending source owns its start command")
        };
        if at >= end {
            return Ok(true);
        }
        let expression = self.expressions.get(note.expression.0).unwrap();
        let cursor = v.cursor.with_step(v.base_step * expression.rendered.ratio);
        let frames = ((end - at) as u32).min(v.tail_remaining.unwrap_or(u32::MAX));
        let asset = self.plans.get(note.plan.0).unwrap().prepared.pcm[v.sample].asset_id();
        Ok(cursor.visit_demand(frames, v.envelope, |offset, frames| {
            accept(SampleDemand {
                asset,
                frames,
                deadline: at + u64::from(offset),
            })
        }))
    }
}

impl Cursor {
    pub(crate) fn visit_demand(
        mut self,
        frames: u32,
        mut envelope: EnvelopeState,
        mut accept: impl FnMut(u32, Range<usize>) -> bool,
    ) -> bool {
        let mut covered_end: Option<i128> = None;
        for offset in 0..frames {
            if self.done() || envelope.done() {
                break;
            }
            let radius = if self.step == 1. && self.fraction == 0. {
                0
            } else {
                crate::resample::Kernel::radius(self.step)
            };
            let mut primary = None;
            let mut partner = None;
            let left = i128::from(self.position) - i128::from(radius);
            let right = i128::from(self.position) + i128::from(radius);
            // Snapshot traversal is immutable and virtual position is monotonic,
            // including reverse/looped physical reads. Overlapping filter guards
            // already carry their earlier deadline; resolve only newly entered taps.
            let begin = covered_end.map_or(left, |end| left.max(end + 1));
            for position in begin..=right {
                if let Some(super::ReadAddress {
                    primary: index,
                    crossfade: second,
                }) = self.address(position)
                {
                    if !extend(&mut primary, index, |range| accept(offset, range)) {
                        return false;
                    }
                    if let Some((index, _)) = second
                        && !extend(&mut partner, index, |range| accept(offset, range))
                    {
                        return false;
                    }
                }
            }
            for range in [primary, partner].into_iter().flatten() {
                if !accept(offset, range) {
                    return false;
                }
            }
            covered_end = Some(covered_end.map_or(right, |end| end.max(right)));
            if envelope.constant_level().is_none() {
                envelope.next();
            }
            self.advance();
        }
        true
    }
}

fn extend(
    pending: &mut Option<Range<usize>>,
    index: usize,
    mut accept: impl FnMut(Range<usize>) -> bool,
) -> bool {
    if let Some(range) = pending {
        if index >= range.start.saturating_sub(1) && index <= range.end {
            range.start = range.start.min(index);
            range.end = range.end.max(index + 1);
            return true;
        }
        if !accept(range.clone()) {
            return false;
        }
    }
    *pending = Some(index..index + 1);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Direction, Envelope, Loop, LoopMode, LoopShape, Playback};

    #[test]
    fn overlapping_windows_keep_every_first_use_deadline_across_loop_and_release_topology() {
        for direction in [Direction::Forward, Direction::Reverse] {
            for shape in [
                None,
                Some(LoopShape::Wrap),
                Some(LoopShape::PingPong),
                Some(LoopShape::Crossfade { frames: 7 }),
            ] {
                for step in [1. / 256., 0.5, 1., 1.0001, 2., 16.] {
                    for released in [false, true] {
                        let mut cursor = Playback {
                            start: 2,
                            end: Some(122),
                            direction,
                            loop_range: shape.map(|shape| Loop {
                                start: 31,
                                end: 83,
                                shape,
                                mode: LoopMode::UntilRelease,
                                passes: std::num::NonZeroU32::new(8),
                            }),
                            ..Playback::default()
                        }
                        .cursor(128, 48000, 48000)
                        .unwrap()
                        .with_step(step);
                        for _ in 0..19 {
                            cursor.advance();
                        }
                        if released {
                            cursor.release();
                        }
                        let envelope = EnvelopeState::new(Envelope::one_shot(5, 600, 3));
                        let mut expected: [Option<u32>; 128] = [None; 128];
                        let mut naive = cursor;
                        let mut env = envelope;
                        for frame in 0..640 {
                            if naive.done() || env.done() {
                                break;
                            }
                            let radius = if naive.step == 1. && naive.fraction == 0. {
                                0
                            } else {
                                crate::resample::Kernel::radius(naive.step)
                            };
                            for tap in -radius..=radius {
                                if let Some(address) =
                                    naive.address(i128::from(naive.position) + i128::from(tap))
                                {
                                    expected[address.primary].get_or_insert(frame);
                                    if let Some((partner, _)) = address.crossfade {
                                        expected[partner].get_or_insert(frame);
                                    }
                                }
                            }
                            env.next();
                            naive.advance();
                        }
                        let mut actual: [Option<u32>; 128] = [None; 128];
                        assert!(cursor.visit_demand(640, envelope, |frame, range| {
                            for index in range {
                                actual[index].get_or_insert(frame);
                            }
                            true
                        }));
                        assert_eq!(
                            actual, expected,
                            "{direction:?} {shape:?} {step} released={released}"
                        );
                    }
                }
            }
        }
    }
}
