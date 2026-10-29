//! What a voice plays: a sample region and how the cursor walks it.

use std::sync::Arc;

use kontra_stream::{Mix, Sample};

/// A loop over source frames `start..end`, inside the region.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Loop {
    pub start: u64,
    pub end: u64,
    /// Frames before `end` that fade linearly into the same number before
    /// `start`, so the wrap is seamless: frame `p` in `end - crossfade..end`
    /// plays `(1 − w)·s[p] + w·s[p − (end − start)]`, `w` rising from 0.
    /// Kontakt's loop X-Fade (linear complementary, measured against Kontakt
    /// to −70 dBFS residual, docs/re/engine/LOOP_XFADE_HOLD_PROBES.md §1).
    pub crossfade: u64,
    /// Repeats after the first pass; 0 repeats forever (or until release).
    pub count: u32,
    /// Each repeat plays backward, then forward again. Endpoints are visited
    /// once per turn (docs/ALTERNATING_LOOPS.md). No crossfade.
    pub alternating: bool,
    /// Repeats until the note is released, then plays on past `end`.
    pub until_release: bool,
}

impl Loop {
    /// The fade length that fits: no longer than the loop, nor than the audio
    /// before its start.
    fn fade(&self) -> u64 {
        self.crossfade.min(self.end - self.start).min(self.start)
    }

    /// Frames in one turn, and the backward part at its start. A forward loop
    /// turns once per body; a ping-pong turn runs `end - 2` down to `start + 1`
    /// then `start` up to `end - 1`. Bodies shorter than 3 frames play forward.
    fn turn(&self) -> (u64, u64) {
        let body = self.end - self.start;
        if self.alternating && body >= 3 {
            (2 * body - 2, body - 2)
        } else {
            (body, 0)
        }
    }

    /// Repeats after the first pass that starts at logical `at`, counting until
    /// `released` if the loop ends on release. `None` repeats forever.
    fn repeats(&self, at: u64, released: Option<u64>) -> Option<u64> {
        let count = (self.count > 0).then_some(u64::from(self.count));
        let held = released
            .filter(|_| self.until_release)
            .map(|r| r.saturating_sub(at).div_ceil(self.turn().0));
        match (count, held) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }
}

/// The playable region of one zone, already resolved by the plan.
#[derive(Clone)]
pub struct Region {
    pub sample: Arc<Sample>,
    /// First source frame played, after start offsets.
    pub start: u64,
    /// One past the last source frame played.
    pub end: u64,
    /// Loops in play order. Each is entered when the playhead first reaches its
    /// end; a loop ending before the playhead or past the region is skipped.
    // ponytail: a release inside a crossfade drops the blend at once; fade the
    // blend out when a library's release clicks.
    pub loops: Vec<Loop>,
    /// Plays `end - 1` down to `start`; loops are ignored.
    pub reverse: bool,
    /// How the sample's channels reach the voice's left and right; `None`
    /// plays mono on both sides and two or more channels as left and right.
    pub mix: Option<Mix>,
    /// Read as a wavetable instead: `start`, `end` and the loops go unused.
    pub table: Option<crate::Table>,
}

/// Consecutive source frames the cursor reads under one mapping.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Run {
    /// The frame played first.
    pub first: u64,
    /// Frames in the run.
    pub count: usize,
    /// Plays `first`, `first - 1`, … down to `first + 1 - count`.
    pub descending: bool,
    /// Set when the pass wraps at `end`: see [`Run::crossfade`].
    fade: Option<Fade>,
}

#[derive(Clone, Copy, Debug)]
struct Fade {
    end: u64,
    len: u64,
    span: u64,
}

impl Run {
    /// The part of the run inside its wrap crossfade: `(offset in the run,
    /// partner first frame, frames, fade start, fade length)`.
    pub(crate) fn crossfade(&self) -> Option<(usize, u64, usize, u64, u64)> {
        let fade = self.fade?;
        let from = (fade.end - fade.len).max(self.first);
        let to = fade.end.min(self.first + self.count as u64);
        (fade.len > 0 && from < to).then(|| {
            (
                (from - self.first) as usize,
                from - fade.span,
                (to - from) as usize,
                fade.end - fade.len,
                fade.len,
            )
        })
    }
}

impl Region {
    /// Moves each loop by `start` of the sample's frames and its end by
    /// `length` more, each truncated to whole frames; a loop stays within the
    /// region and at least four frames long, or keeps its place.
    pub fn shift_loops(&mut self, start: f32, length: f32) {
        let n = self.sample.frames() as i64;
        let frames = |x: f32| (n as f32 * x) as i64;
        let (start, length) = (frames(start), frames(length));
        // Bounds are the region's, not the sample's: `locate` skips a loop
        // starting before it or ending past it.
        let hi = i64::try_from(self.end).unwrap_or(n).min(n);
        let lo = i64::try_from(self.start).unwrap_or(hi).min(hi);
        for l in &mut self.loops {
            let mut a = l.start as i64 + start;
            let mut b = l.end as i64 + start + length;
            if a < lo {
                (a, b) = (lo, (b - a + lo).min(hi));
            }
            if b > hi {
                (a, b) = ((a + hi - b).max(lo), hi);
            }
            if b - a < 4 {
                if a < hi - 4 {
                    b = a + 4;
                } else {
                    a = b - 4;
                }
            }
            if a >= lo && b <= hi && b - a >= 4 {
                (l.start, l.end) = (a as u64, b as u64);
            }
        }
    }

    /// The run at logical position `logical`: frames played from the region's
    /// start, so loops and reversal change only the mapping to source frames,
    /// never the cursor. `None` once the region has ended. `released` is the
    /// logical position of the note's release, if it came.
    fn locate(&self, logical: u64, released: Option<u64>) -> Option<Run> {
        if self.reverse {
            if logical >= self.end.saturating_sub(self.start) {
                return None;
            }
            let first = self.end - 1 - logical;
            return Some(Run {
                first,
                count: usize::try_from(first - self.start + 1).unwrap_or(usize::MAX),
                descending: true,
                fade: None,
            });
        }
        // `at` is the logical position at which the playhead reaches `src`.
        let (mut at, mut src) = (0u64, self.start);
        for l in &self.loops {
            if l.start < self.start || l.end <= l.start || l.end > self.end || l.end <= src {
                continue;
            }
            // Play forward to the loop's end; the arrival wraps unless it is the last pass.
            let arrive = l.end - src;
            let b0 = at + arrive;
            let repeats = l.repeats(b0, released);
            let fade = (!l.alternating && l.fade() > 0 && repeats != Some(0)).then_some(Fade {
                end: l.end,
                len: l.fade(),
                span: l.end - l.start,
            });
            if logical < b0 {
                return Some(Run {
                    first: src + (logical - at),
                    count: usize::try_from(b0 - logical).unwrap_or(usize::MAX),
                    descending: false,
                    fade,
                });
            }
            // Then the repeats, one turn each, until the count or the release.
            let (turn, back) = l.turn();
            let body_end = repeats.map(|n| b0.saturating_add(n.saturating_mul(turn)));
            at = b0;
            src = l.end;
            if let Some(end) = body_end
                && logical >= end
            {
                at = end;
                continue;
            }
            let o = logical - at;
            let (t, u) = (o / turn, o % turn);
            if u < back {
                return Some(Run {
                    first: l.end - 2 - u,
                    count: usize::try_from(back - u).unwrap_or(usize::MAX),
                    descending: true,
                    fade: None,
                });
            }
            let wraps = repeats.is_none_or(|n| t + 1 < n);
            let fade = (!l.alternating && l.fade() > 0 && wraps).then_some(Fade {
                end: l.end,
                len: l.fade(),
                span: l.end - l.start,
            });
            return Some(Run {
                first: l.start + (u - back),
                count: usize::try_from(turn - u).unwrap_or(usize::MAX),
                descending: false,
                fade,
            });
        }
        // The rest of the region plays forward.
        let rest = self.end - src;
        let into = logical.checked_sub(at)?;
        (into < rest).then(|| Run {
            first: src + into,
            count: usize::try_from(rest - into).unwrap_or(usize::MAX),
            descending: false,
            fade: None,
        })
    }

    /// Whether logical position `logical` is past everything the region plays.
    pub(crate) fn finished(&self, logical: f64, released: Option<u64>) -> bool {
        self.locate(logical as u64, released).is_none()
    }

    /// Calls `f(offset, run)` for each run covering logical `from..from + len`,
    /// in logical order; `offset` indexes the span. Positions before the region
    /// are skipped; the walk stops at the region's end.
    pub(crate) fn runs(
        &self,
        from: i64,
        len: usize,
        released: Option<u64>,
        mut f: impl FnMut(usize, Run),
    ) {
        let skip = usize::try_from(-from).unwrap_or(0).min(len);
        let mut i = skip;
        let mut logical = u64::try_from(from + skip as i64).unwrap_or(0);
        while i < len {
            let Some(run) = self.locate(logical, released) else {
                return;
            };
            let count = run.count.min(len - i);
            if count == 0 {
                return;
            }
            f(i, Run { count, ..run });
            i += count;
            logical += count as u64;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kontra_stream::{BlobKey, Encoding, MemSource, SampleFmt, Store, StoreConfig};

    fn region(start: u64, end: u64, loops: Vec<Loop>, reverse: bool) -> Region {
        let fmt = SampleFmt {
            encoding: Encoding::I16,
            channels: 1,
        };
        let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
        let key = BlobKey {
            file: "r".into(),
            offset: 0,
            fmt,
        };
        let sample = store
            .load(
                &key,
                std::sync::Arc::new(MemSource::new(fmt, vec![0; 200])),
                100,
                &[],
            )
            .unwrap();
        Region {
            sample,
            start,
            end,
            loops,
            reverse,
            mix: None,
            table: None,
        }
    }

    #[test]
    fn a_loop_shift_stays_in_the_region_and_four_frames_long() {
        let shifted_from = |from, end, start, length| {
            let l = Loop {
                start: 10.max(from),
                end: 70,
                ..Loop::default()
            };
            let mut r = region(from, end, vec![l], false);
            r.shift_loops(start, length);
            r.loops.first().map(|l| (l.start, l.end))
        };
        let shifted = |end, start, length| shifted_from(0, end, start, length);
        // v1's native-checked writes, at a tenth of its 1000-frame sample.
        assert_eq!(shifted(100, 0.126_636_07, -0.917_009_95), Some((22, 26)));
        assert_eq!(shifted(100, -0.2, 0.0), Some((0, 60)));
        assert_eq!(shifted(100, 0.5, 0.0), Some((40, 100)));
        assert_eq!(shifted(100, 0.0, 0.0), Some((10, 70)));
        // A region ending before the sample does keeps the loop inside it.
        assert_eq!(shifted(80, 0.2, 0.0), Some((20, 80)));
        // And one starting after the sample does: `locate` skips a loop
        // starting before the region.
        assert_eq!(shifted_from(30, 100, -0.2, 0.0), Some((30, 70)));
    }

    fn collect(r: &Region, from: i64, len: usize) -> Vec<(usize, u64, usize)> {
        let mut v = Vec::new();
        r.runs(from, len, None, |o, run| v.push((o, run.first, run.count)));
        v
    }

    #[test]
    fn forward_runs_skip_before_start_and_stop_at_end() {
        let r = region(10, 20, vec![], false);
        assert_eq!(collect(&r, -2, 20), vec![(2, 10, 10)]);
    }

    #[test]
    fn a_loop_wraps_into_two_runs() {
        let r = region(
            0,
            50,
            vec![Loop {
                start: 10,
                end: 20,
                ..Loop::default()
            }],
            false,
        );
        assert_eq!(collect(&r, 15, 12), vec![(0, 15, 5), (5, 10, 7)]);
        assert!(!r.finished(1e9, None));
    }

    #[test]
    fn reverse_runs_descend_from_the_end() {
        let r = region(10, 20, vec![], true);
        assert_eq!(collect(&r, 0, 4), vec![(0, 19, 4)]);
        assert_eq!(collect(&r, 8, 5), vec![(0, 11, 2)]);
        assert!(r.finished(10.0, None));
    }
}
