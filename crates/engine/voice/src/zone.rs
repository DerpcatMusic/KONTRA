//! What a voice plays: a sample region and how the cursor walks it.

use std::sync::Arc;

use kontra_stream::{Mix, Sample};

/// A region's loops, held inline: starting a voice copies its region, and
/// the audio thread never allocates. Eight is the most any format writes
/// (Kontakt's loop array); a ninth would spill to the heap.
pub type Loops = smallvec::SmallVec<[Loop; 8]>;

/// A loop over source frames `start..end`, inside the region.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Loop {
    pub start: u64,
    pub end: u64,
    /// Frames before `end` that fade linearly into the same number before
    /// `start`, so the wrap is seamless: frame `p` in `end - crossfade..end`
    /// plays `(1 − w)·s[p] + w·s[p − (end − start)]`, `w` rising from 0.
    /// Kontakt's loop X-Fade (linear complementary, measured against Kontakt
    /// to −70 dBFS residual, docs/re/engine/LOOP_XFADE_HOLD_PROBES.md §1).
    pub crossfade: u64,
    /// Total passes through the loop; 0 repeats forever (or until release),
    /// 1 plays once without wrapping.
    pub count: u32,
    /// Each repeat plays backward, then forward again. Endpoints are visited
    /// once per turn (docs/ALTERNATING_LOOPS.md). No crossfade.
    pub alternating: bool,
    /// Repeats until the note is released, then plays on past `end`.
    pub until_release: bool,
    /// Linear playback-rate multiplier applied on loop repeats (1.0 untuned).
    pub tune_ratio: f32,
}

impl Default for Loop {
    fn default() -> Self {
        Self {
            start: 0,
            end: 0,
            crossfade: 0,
            count: 0,
            alternating: false,
            until_release: false,
            tune_ratio: 1.0,
        }
    }
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

    /// Repeats after the first pass that ends at logical `at`, counting until
    /// `released` if the loop ends on release. `None` repeats forever.
    ///
    /// `count` is total passes (`0` = infinite, `1` = 1 pass / 0 repeats).
    /// If `released` lands inside an active wrap crossfade, that wrap completes
    /// smoothly and the loop exits on the following pass without crossfading.
    fn repeats(&self, at: u64, released: Option<u64>) -> Option<u64> {
        let count = self.count.checked_sub(1).map(u64::from);
        let fade = if self.alternating { 0 } else { self.fade() };
        let held = released.filter(|_| self.until_release).map(|r| {
            r.saturating_add(fade)
                .saturating_sub(at)
                .div_ceil(self.turn().0)
        });
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
    /// Loops in slot order. Each is entered when the playhead reaches its end
    /// (in mirrored coordinates when reversed); a loop ending before the
    /// playhead or past the region is skipped.
    pub loops: Loops,
    /// Plays `end - 1` down to `start` (loops run in mirrored coordinates).
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
    /// Playback-rate multiplier for this run (1.0 outside tuned loop repeats).
    pub tune_ratio: f32,
    /// Set when the pass wraps at `end`: see [`Run::crossfade`].
    fade: Option<Fade>,
}

#[derive(Clone, Copy, Debug)]
struct Fade {
    /// Start of this run in forward (or mirrored) coordinates.
    run_origin: u64,
    end: u64,
    len: u64,
    span: u64,
    /// `Some(total_frames)` when the run is mirrored for reverse playback.
    mirror: Option<u64>,
}

/// The crossfade slice overlapping a [`Run`].
#[derive(Clone, Copy, Debug)]
pub(crate) struct CrossfadeRun {
    /// Offset into the run's decoded slice where blending begins.
    pub offset: usize,
    /// Lowest physical sample frame of the partner audio to read.
    pub partner_low: u64,
    /// Number of frames to blend.
    pub count: usize,
    /// Frames already elapsed in the crossfade before `offset`.
    pub progress: u64,
    /// Total crossfade length in frames.
    pub len: u64,
    /// Whether the partner audio plays descending (reverse loop).
    pub descending: bool,
}

impl Run {
    /// The part of the run inside its wrap crossfade.
    pub(crate) fn crossfade(&self) -> Option<CrossfadeRun> {
        let fade = self.fade?;
        let from = (fade.end - fade.len).max(fade.run_origin);
        let to = fade.end.min(fade.run_origin + self.count as u64);
        (fade.len > 0 && from < to).then(|| {
            let count = (to - from) as usize;
            let partner_origin = from - fade.span;
            let (partner_low, descending) = match fade.mirror {
                Some(total) => (total - (partner_origin + count as u64), true),
                None => (partner_origin, false),
            };
            CrossfadeRun {
                offset: (from - fade.run_origin) as usize,
                partner_low,
                count,
                progress: from - (fade.end - fade.len),
                len: fade.len,
                descending,
            }
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
    /// The sample frame the cursor plays `logical` frames into the region,
    /// through its loops and reverse; `None` past its end.
    #[must_use]
    pub fn frame_at(&self, logical: u64, released: Option<u64>) -> Option<u64> {
        self.locate(logical, released).map(|run| run.first)
    }

    /// Whether any loop in this region changes playback rate on repeats.
    pub(crate) fn has_tuned_loop(&self) -> bool {
        self.loops.iter().any(|l| {
            l.tune_ratio > 0.0 && l.tune_ratio.is_finite() && (l.tune_ratio - 1.0).abs() > 1e-6
        })
    }

    /// Playback-rate multiplier at `logical`, and how many logical source
    /// frames remain in the current run before it can change.
    pub(crate) fn tune_span(&self, logical: u64, released: Option<u64>) -> Option<(f64, u64)> {
        self.locate(logical, released)
            .map(|run| (f64::from(run.tune_ratio), run.count as u64))
    }

    fn locate(&self, logical: u64, released: Option<u64>) -> Option<Run> {
        let mirror = self.reverse.then(|| self.sample.frames().max(self.end));
        let (r_start, r_end) = match mirror {
            Some(total) => (total - self.end, total - self.start),
            None => (self.start, self.end),
        };
        let map_run =
            |m_first: u64, count: usize, m_desc: bool, tune_ratio: f32, fade: Option<Fade>| {
                let (first, descending) = match mirror {
                    Some(total) => (total - 1 - m_first, !m_desc),
                    None => (m_first, m_desc),
                };
                Run {
                    first,
                    count,
                    descending,
                    tune_ratio,
                    fade,
                }
            };
        // `at` is the logical position at which the playhead reaches `src`.
        let (mut at, mut src) = (0u64, r_start);
        for raw in &self.loops {
            let (l_start, l_end) = match mirror {
                Some(total) => {
                    if raw.end <= raw.start || raw.end > total {
                        continue;
                    }
                    (total - raw.end, total - raw.start)
                }
                None => (raw.start, raw.end),
            };
            // Allow starting inside a loop (`l_start < r_start < l_end`): plays
            // from `src` to `l_end`, then wraps to `l_start` (0140960000.c:661-701).
            if l_end <= l_start || l_end > r_end || l_end <= src {
                continue;
            }
            let l = Loop {
                start: l_start,
                end: l_end,
                ..*raw
            };
            // Play forward (or descending in mirrored coords) to the loop's end;
            // the arrival wraps unless it is the last pass.
            let arrive = l.end - src;
            let b0 = at + arrive;
            let repeats = l.repeats(b0, released);
            if logical < b0 {
                let run_origin = src + (logical - at);
                let fade = (!l.alternating && l.fade() > 0 && repeats != Some(0)).then_some(Fade {
                    run_origin,
                    end: l.end,
                    len: l.fade(),
                    span: l.end - l.start,
                    mirror,
                });
                return Some(map_run(
                    run_origin,
                    usize::try_from(b0 - logical).unwrap_or(usize::MAX),
                    false,
                    1.0,
                    fade,
                ));
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
            let tune = if l.tune_ratio > 0.0 && l.tune_ratio.is_finite() {
                l.tune_ratio
            } else {
                1.0
            };
            let o = logical - at;
            let (t, u) = (o / turn, o % turn);
            if u < back {
                return Some(map_run(
                    l.end - 2 - u,
                    usize::try_from(back - u).unwrap_or(usize::MAX),
                    true,
                    tune,
                    None,
                ));
            }
            let wraps = repeats.is_none_or(|n| t + 1 < n);
            let run_origin = l.start + (u - back);
            let fade = (!l.alternating && l.fade() > 0 && wraps).then_some(Fade {
                run_origin,
                end: l.end,
                len: l.fade(),
                span: l.end - l.start,
                mirror,
            });
            return Some(map_run(
                run_origin,
                usize::try_from(turn - u).unwrap_or(usize::MAX),
                false,
                tune,
                fade,
            ));
        }
        // The rest of the region plays through to its end.
        let rest = r_end - src;
        let into = logical.checked_sub(at)?;
        (into < rest).then(|| {
            map_run(
                src + into,
                usize::try_from(rest - into).unwrap_or(usize::MAX),
                false,
                1.0,
                None,
            )
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
            loops: loops.into(),
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
        // The play position of a looped voice is in the sample, not elapsed.
        assert_eq!(r.frame_at(25, None), Some(15));
        assert!(!r.finished(1e9, None));
    }

    #[test]
    fn reverse_runs_descend_from_the_end() {
        let r = region(10, 20, vec![], true);
        assert_eq!(collect(&r, 0, 4), vec![(0, 19, 4)]);
        assert_eq!(r.frame_at(3, None), Some(16));
        assert_eq!(collect(&r, 8, 5), vec![(0, 11, 2)]);
        assert!(r.finished(10.0, None));
    }
}
