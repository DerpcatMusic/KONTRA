//! What a voice plays: a sample region and how the cursor walks it.

use std::sync::Arc;

use kontra_stream::Sample;

/// A forward loop over source frames `start..end`, inside the region.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Loop {
    pub start: u64,
    pub end: u64,
    /// Frames before `end` that fade linearly into the same number before
    /// `start`, so the wrap is seamless: frame `p` in `end - crossfade..end`
    /// plays `(1 − w)·s[p] + w·s[p − (end − start)]`, `w` rising from 0.
    /// Kontakt's loop X-Fade (linear complementary, measured against Kontakt
    /// to −70 dBFS residual, docs/re/engine/LOOP_XFADE_HOLD_PROBES.md §1).
    pub crossfade: u64,
}

impl Loop {
    /// The fade length that fits: no longer than the loop, nor than the audio
    /// before its start.
    fn fade(&self) -> u64 {
        self.crossfade.min(self.end - self.start).min(self.start)
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
    /// Played until the voice ends; ignored when reversed.
    // ponytail: forward loop until end only; alternating and until-release
    // loops when a library needs them.
    pub looped: Option<Loop>,
    /// Plays `end - 1` down to `start`.
    pub reverse: bool,
}

impl Region {
    fn active_loop(&self) -> Option<Loop> {
        self.looped
            .filter(|l| !self.reverse && l.end > l.start && l.start >= self.start)
    }

    /// Whether logical position `logical` is past everything the region plays.
    /// Logical positions count frames played from the region's start, so loops
    /// and reversal change only the mapping to source frames, never the cursor.
    pub(crate) fn finished(&self, logical: f64) -> bool {
        self.active_loop().is_none() && logical >= self.end.saturating_sub(self.start) as f64
    }

    /// The part of the run `first..first + count` inside the loop's crossfade:
    /// `(offset in the run, partner first frame, frames, fade start, fade length)`.
    pub(crate) fn crossfade(
        &self,
        first: u64,
        count: usize,
    ) -> Option<(usize, u64, usize, u64, u64)> {
        let l = self.active_loop()?;
        let fade = l.fade();
        let from = (l.end - fade).max(first);
        let to = l.end.min(first + count as u64);
        (fade > 0 && from < to).then(|| {
            (
                (from - first) as usize,
                from - (l.end - l.start),
                (to - from) as usize,
                l.end - fade,
                fade,
            )
        })
    }

    /// Calls `f(offset, first, count)` for each run of consecutive source frames
    /// covering logical `from..from + len`, in logical order; `offset` indexes the
    /// span. Positions before the region or past its end are skipped. Reversed runs
    /// descend: `first` is the highest frame.
    pub(crate) fn runs(&self, from: i64, len: usize, mut f: impl FnMut(usize, u64, usize)) {
        let skip = usize::try_from(-from).unwrap_or(0).min(len);
        let mut i = skip;
        let mut logical = u64::try_from(from + skip as i64).unwrap_or(0);
        let total = self.end.saturating_sub(self.start);
        while i < len {
            let (first, room) = if self.reverse {
                if logical >= total {
                    return;
                }
                let first = self.end - 1 - logical;
                (first, first - self.start + 1)
            } else if let Some(l) = self.active_loop() {
                let mut frame = self.start + logical;
                if frame >= l.end {
                    frame = l.start + (frame - l.start) % (l.end - l.start);
                }
                (frame, l.end - frame)
            } else {
                if logical >= total {
                    return;
                }
                let first = self.start + logical;
                (first, self.end - first)
            };
            let count = usize::try_from(room).unwrap_or(usize::MAX).min(len - i);
            f(i, first, count);
            i += count;
            logical += count as u64;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kontra_stream::{BlobKey, Encoding, MemSource, SampleFmt, Store, StoreConfig};

    fn region(start: u64, end: u64, looped: Option<Loop>, reverse: bool) -> Region {
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
            looped,
            reverse,
        }
    }

    fn collect(r: &Region, from: i64, len: usize) -> Vec<(usize, u64, usize)> {
        let mut v = Vec::new();
        r.runs(from, len, |o, f, c| v.push((o, f, c)));
        v
    }

    #[test]
    fn forward_runs_skip_before_start_and_stop_at_end() {
        let r = region(10, 20, None, false);
        assert_eq!(collect(&r, -2, 20), vec![(2, 10, 10)]);
    }

    #[test]
    fn a_loop_wraps_into_two_runs() {
        let r = region(
            0,
            50,
            Some(Loop {
                start: 10,
                end: 20,
                crossfade: 0,
            }),
            false,
        );
        assert_eq!(collect(&r, 15, 12), vec![(0, 15, 5), (5, 10, 7)]);
        assert!(!r.finished(1e9));
    }

    #[test]
    fn reverse_runs_descend_from_the_end() {
        let r = region(10, 20, None, true);
        assert_eq!(collect(&r, 0, 4), vec![(0, 19, 4)]);
        assert_eq!(collect(&r, 8, 5), vec![(0, 11, 2)]);
        assert!(r.finished(10.0));
    }
}
