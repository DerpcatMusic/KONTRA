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
    /// Loop geometry `(loop, first crossing, loop length)` when the path reaches the loop.
    fn cycle(&self) -> Option<(LoopMap, u64, u64)> {
        let l = self
            .looped
            .filter(|l| !self.reverse && self.start < l.end)?;
        Some((l, l.end - self.start, l.end - l.start))
    }

    /// Loop wraps taken when the key is released at virtual frame `release`
    /// (`FOREVER` while held). A wrap whose crossfade has begun is completed.
    pub fn wraps(&self, release: u64) -> u64 {
        match self.cycle() {
            None => 0,
            Some((l, _, _)) if !l.until_release || release == FOREVER => FOREVER,
            Some((l, first, len)) => (release + l.xfade).saturating_sub(first).div_ceil(len),
        }
    }

    /// Virtual length of the whole path.
    pub fn len(&self, wraps: u64) -> u64 {
        match self.cycle() {
            Some(_) if wraps == FOREVER => FOREVER,
            Some((l, first, len)) if wraps > 0 => first + (wraps - 1) * len + (self.end - l.start),
            _ => self.end - self.start,
        }
    }

    /// First virtual frame whose mapping changes when a held path (`FOREVER`
    /// wraps) is released with `wraps` wraps.
    pub fn divergence(&self, wraps: u64) -> u64 {
        match self.cycle() {
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

    /// First virtual frame whose data (or crossfade partner) lies outside the
    /// resident span `[a, b)`; `FOREVER` if the whole path is resident.
    pub fn resident_limit(&self, wraps: u64, a: u64, b: u64) -> u64 {
        let mut v = 0;
        let mut skipped = false;
        while let Some(run) = self.run(v, wraps) {
            if let Some(i) = run.outside(a, b) {
                return v + i;
            }
            v += run.len;
            // Every wrapped cycle maps identically: after checking one, skip to the tail.
            if let Some((_, first, len)) = self.cycle()
                && !skipped
                && wraps > 0
                && v >= first + len
            {
                if wraps == FOREVER {
                    return FOREVER;
                }
                skipped = true;
                v = v.max(first + (wraps - 1) * len);
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
            })
        };
        vec![
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
        ]
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
                    let brute = frames
                        .iter()
                        .position(|(f, blend)| {
                            !inside(*f) || blend.is_some_and(|(p, _)| !inside(p))
                        })
                        .map_or(FOREVER, |i| i as u64);
                    let limit = map.resident_limit(wraps, a, b);
                    let limit = if frames.len() == 400 && limit >= 400 {
                        FOREVER
                    } else {
                        limit
                    };
                    assert_eq!(limit, brute, "{map:?} release {release} span {a}..{b}");
                }
            }
        }
    }
}
