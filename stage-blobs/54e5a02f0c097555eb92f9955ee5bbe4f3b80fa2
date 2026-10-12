//! Voice playback paths.
//!
//! A voice reads *virtual* frames 0, 1, 2… in playback order. [`PlayMap`]
//! maps them to sample frames through loops, loop crossfades and reverse
//! playback. The audio thread and the streamer share this mapping, so streamed
//! frames are bit-identical to resident ones.

use crate::audio::Frame;

/// Unbounded virtual length / never.
pub(crate) const FOREVER: u64 = u64::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LoopMap {
    pub start: u64,
    pub end: u64,
    /// Crossfade width; the bank guarantees `xfade <= start` and `xfade <= end - start`.
    pub xfade: u64,
    pub until_release: bool,
    pub alternating: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PlayMap {
    /// First sample frame (forward) or lowest frame played (reverse).
    pub start: u64,
    /// Exclusive end frame.
    pub end: u64,
    pub reverse: bool,
    pub looped: Option<LoopMap>,
}

/// Consecutive virtual frames that map to consecutive sample frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Run {
    pub frame: u64,
    pub len: u64,
    /// Frames descend from `frame` instead of ascending.
    pub reverse: bool,
    pub blend: Option<Blend>,
}

/// Loop crossfade: the run's frames fade toward the frames one loop length earlier,
/// which lead seamlessly into the loop start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Blend {
    pub partner: u64,
    /// Position of the run's first frame inside the crossfade.
    pub offset: u64,
    pub width: u64,
}

impl Blend {
    /// Weight of the partner frame `i` frames into the run.
    #[inline]
    pub fn weight(&self, i: u64) -> f32 {
        (self.offset + i) as f32 / self.width as f32
    }

    /// Crossfaded frame `i` frames into the run.
    #[inline]
    pub fn apply(&self, i: u64, own: Frame, partner: Frame) -> Frame {
        let t = self.weight(i);
        [
            own[0] + (partner[0] - own[0]) * t,
            own[1] + (partner[1] - own[1]) * t,
        ]
    }
}

impl PlayMap {
    /// Native forward loop controls add truncated fractions of total sample frames.
    /// Cropped/reverse/alternating paths have not been independently established.
    pub fn controlled_loop(mut self, offsets: [f32; 2], frames: u64) -> Option<Self> {
        let mut l = self.looped?;
        if self.reverse || l.alternating || self.start != 0 || self.end != frames || frames < 4 || frames > i32::MAX as u64
            || !offsets.iter().all(|v| v.is_finite()) { return None; }
        let shifts = offsets.map(|v| frames as f32 * v);
        if !shifts.iter().all(|&v| v.is_finite() && v >= i32::MIN as f32 && v < 2147483648.) { return None; }
        let [start_shift, length_shift] = shifts.map(|v| i64::from(v as i32));
        let n = frames as i64;
        let mut start = l.start as i64 + start_shift;
        let mut end = l.end as i64 + start_shift + length_shift;
        if start < 0 { end = (end - start).min(n); start = 0; }
        if end >= n { start = (start + n - end).max(0); end = n; }
        if end - start < 4 {
            if start < n - 4 { end = start + 4; } else if start >= 4 { start = end - 4; }
        }
        // Do not turn an out-of-range native signed bound into an unsigned path.
        if start < 0 || end > n || end - start < 4 { return None; }
        l.start = start as u64;
        l.end = end as u64;
        l.xfade = l.xfade.min(l.end - l.start - 1).min(l.start);
        self.looped = Some(l);
        Some(self)
    }

    /// Loop geometry `(loop, first crossing, loop length)` when the path reaches the loop.
    fn cycle(&self) -> Option<(LoopMap, u64, u64)> {
        let l = self.looped.filter(|l| {
            if self.reverse {
                l.alternating && self.end > l.start
            } else {
                self.start < l.end
            }
        })?;
        let first = if self.reverse {
            self.end - l.start
        } else {
            l.end - self.start
        };
        let len = l.end - l.start;
        Some((
            l,
            first,
            if l.alternating {
                (2 * (len - 1)).max(1)
            } else {
                len
            },
        ))
    }

    /// Loop wraps taken when the key is released at virtual frame `release`
    /// (`FOREVER` while held). A wrap whose crossfade has begun is completed.
    /// Alternating until-release paths instead store the virtual release frame:
    /// playback continues in the normal direction from its current sample frame.
    pub fn wraps(&self, release: u64) -> u64 {
        match self.cycle() {
            None => 0,
            Some((l, _, _)) if !l.until_release || release == FOREVER => FOREVER,
            Some((l, _, _)) if l.alternating => release,
            Some((l, first, len)) => (release + l.xfade).saturating_sub(first).div_ceil(len),
        }
    }

    /// Virtual length of the whole path.
    pub fn len(&self, wraps: u64) -> u64 {
        match self.cycle() {
            Some(_) if wraps == FOREVER => FOREVER,
            Some((l, _, _)) if l.alternating => {
                let frame = self.alternating(wraps, FOREVER).frame;
                wraps
                    + if self.reverse {
                        (frame + 1).saturating_sub(self.start)
                    } else {
                        self.end - frame
                    }
            }
            Some((l, first, len)) if wraps > 0 => first + (wraps - 1) * len + (self.end - l.start),
            _ => self.end - self.start,
        }
    }

    /// First virtual frame whose mapping changes when a held path (`FOREVER`
    /// wraps) is released with `wraps` wraps.
    pub fn divergence(&self, wraps: u64) -> u64 {
        match self.cycle() {
            Some((l, _, _)) if l.alternating && wraps != FOREVER => wraps,
            Some((l, first, len)) if wraps != FOREVER => first + wraps * len - l.xfade,
            _ => FOREVER,
        }
    }

    /// The run containing virtual frame `v`, or `None` past the end.
    pub fn run(&self, v: u64, wraps: u64) -> Option<Run> {
        let total = self.len(wraps);
        if v >= total {
            return None;
        }
        if self.cycle().is_some_and(|(l, _, _)| l.alternating) {
            return Some(self.alternating(v, wraps));
        }
        if self.reverse {
            return Some(Run {
                frame: self.end - 1 - v,
                len: total - v,
                reverse: true,
                blend: None,
            });
        }
        let linear = |frame: u64| Run {
            frame,
            len: self.end - frame,
            reverse: false,
            blend: None,
        };
        let Some((l, first, len)) = self.cycle().filter(|_| wraps > 0) else {
            return Some(linear(self.start + v));
        };
        let frame = if v < first {
            self.start + v
        } else {
            // `k + 1` crossings passed; after the last wrap the tail plays through.
            let k = (v - first) / len;
            if wraps != FOREVER && k + 1 >= wraps {
                return Some(linear(l.start + (v - first - (wraps - 1) * len)));
            }
            l.start + (v - first) % len
        };
        let fade_start = l.end - l.xfade;
        Some(if frame < fade_start {
            Run {
                frame,
                len: fade_start - frame,
                reverse: false,
                blend: None,
            }
        } else {
            let blend = Blend {
                partner: frame - len,
                offset: frame - fade_start,
                width: l.xfade,
            };
            Run {
                frame,
                len: l.end - frame,
                reverse: false,
                blend: Some(blend),
            }
        })
    }

    /// Reflect at the loop's sample endpoints without duplicating them. The
    /// virtual path stays monotone, so the existing fractional resampler and
    /// streaming ring use the same interpolation stencil across both turns.
    fn alternating(&self, v: u64, release: u64) -> Run {
        let l = self.looped.unwrap();
        let (start, end, lo, hi) = if self.reverse {
            (
                0,
                self.end - self.start,
                self.end - l.end,
                self.end - l.start,
            )
        } else {
            (self.start, self.end, l.start, l.end)
        };
        let frame = |v: u64| {
            let first = hi - start;
            if v < first {
                return (start + v, first - v, false);
            }
            let leg = hi - lo - 1;
            if leg == 0 {
                return (lo, 1, false);
            }
            let phase = (v - first) % (2 * leg);
            if phase < leg {
                (hi - 2 - phase, leg - phase, true)
            } else {
                (lo + 1 + phase - leg, 2 * leg - phase, false)
            }
        };
        let (at, len, reverse) = if release != FOREVER && v >= release {
            let at = frame(release).0 + (v - release);
            (at, end - at, false)
        } else {
            let (at, len, reverse) = frame(v);
            (at, len.min(release.saturating_sub(v)), reverse)
        };
        Run {
            frame: if self.reverse { self.end - 1 - at } else { at },
            len,
            reverse: reverse ^ self.reverse,
            blend: None,
        }
    }

    /// First virtual frame from `from` on whose data (or crossfade partner)
    /// lies outside the resident span `[a, b)`; `FOREVER` if the rest of the
    /// path is resident.
    pub fn resident_limit(&self, from: u64, wraps: u64, a: u64, b: u64) -> u64 {
        let mut v = from;
        let mut skipped = false;
        while let Some(run) = self.run(v, wraps) {
            if let Some(i) = run.outside(a, b) {
                return v + i;
            }
            v += run.len;
            // Every wrapped cycle maps identically: after checking a whole
            // one, skip to the tail.
            if let Some((l, first, len)) = self.cycle()
                && !skipped
                && wraps > 0
                && v >= first.max(from) + len
            {
                if wraps == FOREVER {
                    return FOREVER;
                }
                skipped = true;
                v = v.max(if l.alternating {
                    wraps
                } else {
                    first + (wraps - 1) * len
                });
            }
        }
        FOREVER
    }
}

impl Run {
    /// Index of the first frame outside `[a, b)`, if any.
    fn outside(&self, a: u64, b: u64) -> Option<u64> {
        let ascending = |lo: u64| {
            if lo < a {
                Some(0)
            } else if lo + self.len > b {
                Some(b.saturating_sub(lo))
            } else {
                None
            }
        };
        let own = if !self.reverse {
            ascending(self.frame)
        } else if self.frame >= b || self.frame < a {
            Some(0)
        } else {
            (self.frame + 1 < a + self.len).then(|| self.frame + 1 - a)
        };
        let partner = self.blend.and_then(|blend| ascending(blend.partner));
        own.into_iter().chain(partner).min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reference: step one frame at a time like a naive sampler would.
    fn naive(map: &PlayMap, release: u64, count: u64) -> Vec<(u64, Option<(u64, f32)>)> {
        let mut out = Vec::new();
        if let Some(l) = map.looped.filter(|l| l.alternating)
            && (if map.reverse {
                map.end > l.start
            } else {
                map.start < l.end
            })
        {
            let mut s = if map.reverse {
                map.end as i64 - 1
            } else {
                map.start as i64
            };
            let normal = if map.reverse { -1 } else { 1 };
            let mut direction = normal;
            for v in 0..count {
                if s < 0
                    || (!map.reverse && s >= map.end as i64)
                    || (map.reverse && s < map.start as i64)
                {
                    break;
                }
                if l.until_release && v >= release {
                    direction = normal;
                }
                out.push((s as u64, None));
                let held = !l.until_release || v < release;
                if !held {
                    direction = normal;
                } else if l.end - l.start == 1 && s == l.start as i64 {
                    continue;
                } else if direction > 0 && s == l.end as i64 - 1 {
                    direction = -1;
                } else if direction < 0 && s == l.start as i64 {
                    direction = 1;
                }
                s += direction;
            }
            return out;
        }
        if map.reverse {
            return (map.start..map.end)
                .rev()
                .take(count as usize)
                .map(|f| (f, None))
                .collect();
        }
        let mut s = map.start;
        let mut v = 0;
        while s < map.end && v < count {
            let lp = map.looped.filter(|l| map.start < l.end);
            let wrapping = lp.is_some_and(|l| {
                // The crossing ahead wraps unless released before its crossfade began.
                s < l.end
                    && (!l.until_release
                        || release == FOREVER
                        || v + (l.end - s) < release + l.xfade)
            });
            let blend = lp
                .filter(|l| wrapping && s >= l.end - l.xfade && s < l.end)
                .map(|l| {
                    (
                        s - (l.end - l.start),
                        (s - (l.end - l.xfade)) as f32 / l.xfade as f32,
                    )
                });
            out.push((s, blend));
            s += 1;
            v += 1;
            if let Some(l) = lp
                && wrapping
                && s == l.end
            {
                s = l.start;
            }
        }
        out
    }

    fn expand(map: &PlayMap, wraps: u64, count: u64) -> Vec<(u64, Option<(u64, f32)>)> {
        let mut out = Vec::new();
        let mut v = 0;
        while let Some(run) = map.run(v, wraps) {
            for i in 0..run.len.min(count - v) {
                let frame = if run.reverse {
                    run.frame - i
                } else {
                    run.frame + i
                };
                out.push((frame, run.blend.map(|b| (b.partner + i, b.weight(i)))));
            }
            v += run.len;
            if v >= count {
                break;
            }
        }
        out
    }

    fn maps() -> Vec<PlayMap> {
        let lp = |start, end, xfade, until_release| {
            Some(LoopMap {
                start,
                end,
                xfade,
                until_release,
                alternating: false,
            })
        };
        let mut out = vec![
            PlayMap {
                start: 3,
                end: 40,
                reverse: false,
                looped: None,
            },
            PlayMap {
                start: 3,
                end: 40,
                reverse: true,
                looped: None,
            },
            PlayMap {
                start: 0,
                end: 40,
                reverse: false,
                looped: lp(10, 20, 0, false),
            },
            PlayMap {
                start: 2,
                end: 40,
                reverse: false,
                looped: lp(10, 20, 4, false),
            },
            PlayMap {
                start: 2,
                end: 40,
                reverse: false,
                looped: lp(10, 20, 4, true),
            },
            PlayMap {
                start: 17,
                end: 30,
                reverse: false,
                looped: lp(8, 20, 5, true),
            },
            PlayMap {
                start: 25,
                end: 30,
                reverse: false,
                looped: lp(8, 20, 5, true),
            },
        ];
        for reverse in [false, true] {
            for until_release in [false, true] {
                for (start, end) in [(10, 20), (10, 11), (10, 12)] {
                    out.push(PlayMap {
                        start: 3,
                        end: 40,
                        reverse,
                        looped: Some(LoopMap {
                            start,
                            end,
                            xfade: 0,
                            until_release,
                            alternating: true,
                        }),
                    });
                }
            }
        }
        out
    }

    #[test]
    fn runs_match_naive_stepping() {
        for map in maps() {
            for release in [FOREVER, 0, 5, 14, 18, 19, 31, 47, 60] {
                let wraps = map.wraps(release);
                let count = map.len(wraps).min(120);
                assert_eq!(
                    expand(&map, wraps, count),
                    naive(&map, release, count),
                    "{map:?} release {release}"
                );
                // Released paths agree with the held path up to the divergence point.
                let held = expand(&map, FOREVER, 120);
                let cut = map.divergence(wraps).min(count) as usize;
                assert_eq!(
                    expand(&map, wraps, count)[..cut],
                    held[..cut.min(held.len())],
                    "{map:?} {release}"
                );
            }
        }
    }

    #[test]
    fn resident_limit_matches_brute_force() {
        for map in maps() {
            for release in [FOREVER, 0, 14, 31] {
                let wraps = map.wraps(release);
                for (a, b) in [
                    (0, 40),
                    (3, 22),
                    (5, 15),
                    (0, 9),
                    (10, 40),
                    (12, 35),
                    (20, 40),
                ] {
                    let frames = expand(&map, wraps, 400);
                    let inside = |f: u64| f >= a && f < b;
                    for from in [0, 3, 7, 12, 25] {
                        let brute = frames
                            .iter()
                            .skip(from)
                            .position(|(f, blend)| {
                                !inside(*f) || blend.is_some_and(|(p, _)| !inside(p))
                            })
                            .map_or(FOREVER, |i| (from + i) as u64);
                        let limit = map.resident_limit(from as u64, wraps, a, b);
                        let limit = if frames.len() == 400 && limit >= 400 {
                            FOREVER
                        } else {
                            limit
                        };
                        assert_eq!(
                            limit, brute,
                            "{map:?} release {release} span {a}..{b} from {from}"
                        );
                    }
                }
            }
        }
    }
}
