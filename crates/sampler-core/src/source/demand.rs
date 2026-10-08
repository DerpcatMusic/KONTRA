use super::Cursor;
use crate::{Action, AssetId, Direction, EnvelopeState, Error, Runtime, VoiceId};
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
        let Some(VoiceDemand {
            cursor,
            envelope,
            at,
            frames,
            asset,
        }) = self.voice_demand(voice, frames)?
        else {
            return Ok(true);
        };
        Ok(cursor.visit_demand(frames, envelope, |offset, frames| {
            accept(SampleDemand {
                asset,
                frames,
                deadline: at + u64::from(offset),
            })
        }))
    }

    /// A voice's cursor at its rendered pitch over the horizon, from its
    /// first output frame `at`; `None` when it starts beyond the horizon.
    pub(crate) fn voice_demand(
        &self,
        voice: VoiceId,
        frames: u32,
    ) -> Result<Option<VoiceDemand>, Error> {
        let end = self
            .now
            .checked_add(u64::from(frames))
            .ok_or(Error::ClockOverflow)?;
        let v = self.voices.get(voice.0).ok_or(Error::StaleHandle)?;
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
            return Ok(None);
        }
        let expression = self.expressions.get(note.expression.0).unwrap();
        Ok(Some(VoiceDemand {
            cursor: v.cursor.with_step(v.base_step * expression.rendered.ratio),
            envelope: v.envelope,
            at,
            frames: ((end - at) as u32).min(v.tail_remaining.unwrap_or(u32::MAX)),
            asset: self.plans.get(note.plan.0).unwrap().prepared.pcm[v.sample].asset_id(),
        }))
    }
}

pub(crate) struct VoiceDemand {
    pub cursor: Cursor,
    pub envelope: EnvelopeState,
    pub at: u64,
    pub frames: u32,
    pub asset: AssetId,
}

/// Up to four physical ranges, each with the output offset first needing it.
pub(crate) type LoopReach = [Option<(Range<usize>, u32)>; 4];

impl Cursor {
    /// Physical frames a stretch of `frames` output frames may read, when that
    /// stretch maps linearly (before any loop boundary or crossfade): a
    /// superset of `visit_demand`'s ranges, ignoring envelope ends. With the
    /// traversal direction, and the virtual offset of the cursor's window
    /// start, to recover first-use deadlines.
    pub(crate) fn linear_reach(&self, frames: u32) -> Option<(Range<usize>, Direction, f64)> {
        if self.loops.is_some() {
            return None;
        }
        let radius = crate::resample::Kernel::radius(self.step()) as f64;
        let length = (self.end - self.start) as f64;
        let low = self.position as f64 - radius;
        let high =
            self.position as f64 + self.fraction + f64::from(frames) * self.step() + radius + 1.;
        if let Some(r) = self.loop_range {
            let fade = match r.shape {
                crate::LoopShape::Crossfade { frames }
                | crate::LoopShape::EqualPowerCrossfade { frames } => frames as f64,
                _ => 0.,
            };
            if self.exit.is_some() || high + fade >= self.first_boundary(r) as f64 {
                return None;
            }
        }
        let (low, high) = (low.max(0.) as usize, high.min(length).max(0.) as usize);
        let range = match self.direction {
            Direction::Forward => self.start + low..self.start + high.max(low),
            Direction::Reverse => self.end - high.max(low)..self.end - low,
        };
        Some((
            range,
            self.direction,
            self.position as f64 + self.fraction + radius,
        ))
    }

    /// Physical ranges a looping stretch of `frames` output frames may read,
    /// each with the output offset it may first be needed: up to the loop
    /// boundary, within the loop (wrapping), and a crossfade's other leg. A
    /// superset of `visit_demand`'s ranges, ignoring envelope ends; `None` for
    /// ping-pong loops and a loop exit within reach.
    pub(crate) fn loop_reach(&self, frames: u32) -> Option<LoopReach> {
        if self.loops.is_some() {
            return None;
        }
        let r = self.loop_range?;
        let fade = match r.shape {
            crate::LoopShape::PingPong => return None,
            crate::LoopShape::Wrap => 0,
            crate::LoopShape::Crossfade { frames }
            | crate::LoopShape::EqualPowerCrossfade { frames } => frames as u64,
        };
        let radius = crate::resample::Kernel::radius(self.step()) as f64;
        let lead = self.position as f64 + self.fraction + radius;
        let low = (self.position as f64 - radius).max(0.) as u64;
        let high = (lead + f64::from(frames) * self.step() + 1.).ceil() as u64;
        if self.exit.is_some_and(|exit| high > exit) {
            return None;
        }
        // Virtual offsets: past the first boundary, offsets wrap into the loop.
        let first = self.first_boundary(r);
        let length = (r.end - r.start) as u64;
        let base = first - length;
        let when = |v: u64| (((v as f64 - lead) / self.step()).floor() - 1.).max(0.) as u32;
        let mut out: [Option<(Range<u64>, u32)>; 4] = Default::default();
        out[0] = Some((low..high.min(first), when(low)));
        if high > first {
            let start = low.max(first);
            let extent = high - start;
            let at = when(start);
            if extent >= length {
                out[1] = Some((base..first, at));
            } else {
                let from = base + (start - first) % length;
                out[1] = Some((from..(from + extent).min(first), at));
                out[2] = Some((base..(from + extent).saturating_sub(length).max(base), at));
            }
        }
        if fade > 0 && high + fade > first {
            out[3] = Some((base.saturating_sub(fade)..base, when(first - fade)));
        }
        let view = (self.end - self.start) as u64;
        Some(out.map(|part| {
            let (range, at) = part?;
            let (from, to) = (range.start as usize, range.end.min(view) as usize);
            (from < to).then(|| match self.direction {
                Direction::Forward => (self.start + from..self.start + to, at),
                Direction::Reverse => (self.end - to..self.end - from, at),
            })
        }))
    }

    /// Output frames until a linear stretch (see `linear_reach`) first reads
    /// physical frame `index`; `lead` is the reach's third value.
    pub(crate) fn first_use(&self, index: usize, direction: Direction, lead: f64) -> u32 {
        let offset = match direction {
            Direction::Forward => index - self.start,
            Direction::Reverse => self.end - 1 - index,
        } as f64;
        // One frame early: a deadline may only err towards urgency.
        (((offset - lead) / self.step()).floor() - 1.).max(0.) as u32
    }

    pub(crate) fn visit_demand(
        mut self,
        frames: u32,
        mut envelope: EnvelopeState,
        mut accept: impl FnMut(u32, Range<usize>) -> bool,
    ) -> bool {
        let mut covered_end: Option<i128> = None;
        let mut previous_step = self.step();
        for offset in 0..frames {
            if self.done() || envelope.done() {
                break;
            }
            if self.step() != previous_step {
                covered_end = None;
                previous_step = self.step();
            }
            let radius = if self.step() == 1. && self.fraction == 0. {
                0
            } else {
                crate::resample::Kernel::radius(self.step())
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
    fn a_linear_reach_covers_every_read_no_later_than_its_first_use() {
        for direction in [Direction::Forward, Direction::Reverse] {
            for looped in [false, true] {
                for step in [0.5, 1., 1.37, 2., 3.9] {
                    let mut cursor = Playback {
                        start: 3,
                        end: Some(4000),
                        direction,
                        loop_range: looped.then_some(Loop {
                            start: 2000,
                            end: 3900,
                            shape: LoopShape::Crossfade { frames: 64 },
                            mode: LoopMode::UntilRelease,
                            passes: None,
                        }),
                        ..Playback::default()
                    }
                    .cursor(4096, 48000, 48000)
                    .unwrap()
                    .with_step(step);
                    for _ in 0..7 {
                        cursor.advance();
                    }
                    let (reach, dir, lead) = cursor.linear_reach(300).expect("before the loop");
                    assert_eq!(dir, direction);
                    let envelope = EnvelopeState::new(Envelope::one_shot(0, 10_000, 0));
                    assert!(cursor.visit_demand(300, envelope, |frame, range| {
                        assert!(reach.start <= range.start && range.end <= reach.end);
                        for index in range {
                            assert!(cursor.first_use(index, dir, lead) <= frame);
                        }
                        true
                    }));
                    assert!(!looped || cursor.linear_reach(20_000).is_none());
                }
            }
        }
    }

    #[test]
    fn a_loop_reach_covers_every_read_no_later_than_its_first_use() {
        for direction in [Direction::Forward, Direction::Reverse] {
            for shape in [LoopShape::Wrap, LoopShape::Crossfade { frames: 40 }] {
                for (step, advance, frames) in
                    [(1., 900, 300), (1.37, 1000, 2000), (0.5, 1500, 300)]
                {
                    for passes in [None, std::num::NonZeroU32::new(9)] {
                        let mut cursor = Playback {
                            start: 3,
                            end: Some(4000),
                            direction,
                            loop_range: Some(Loop {
                                start: 600,
                                end: 1100,
                                shape,
                                mode: LoopMode::UntilRelease,
                                passes,
                            }),
                            ..Playback::default()
                        }
                        .cursor(4096, 48000, 48000)
                        .unwrap()
                        .with_step(step);
                        for _ in 0..advance {
                            cursor.advance();
                        }
                        let Some(reach) = cursor.loop_reach(frames) else {
                            assert!(passes.is_some());
                            continue;
                        };
                        let envelope = EnvelopeState::new(Envelope::one_shot(0, 10_000, 0));
                        let mut seen = 0;
                        assert!(cursor.visit_demand(frames, envelope, |frame, range| {
                            for index in range {
                                seen += 1;
                                assert!(
                                    reach
                                        .iter()
                                        .flatten()
                                        .any(|(r, at)| r.contains(&index) && *at <= frame),
                                    "{direction:?} {shape:?} {step} {index} at {frame}: {reach:?}"
                                );
                            }
                            true
                        }));
                        assert!(seen > 0);
                    }
                }
            }
        }
    }

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
