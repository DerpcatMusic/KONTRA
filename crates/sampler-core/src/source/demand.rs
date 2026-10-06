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
    fn visit_demand(
        mut self,
        frames: u32,
        mut envelope: EnvelopeState,
        mut accept: impl FnMut(u32, Range<usize>) -> bool,
    ) -> bool {
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
            for tap in -radius..=radius {
                if let Some(super::ReadAddress {
                    primary: index,
                    crossfade: second,
                }) = self.address(i128::from(self.position) + i128::from(tap))
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
