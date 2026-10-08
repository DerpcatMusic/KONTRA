//! Port from v1 0cb7a8a0:src/engine/bank.rs (Builder::plan and spans).
use super::{Pcm, Range, ir};

pub(super) const PRELOAD: usize = 4048;
const MIN_PRELOAD: usize = 1024;
const FLOOR_PRELOAD: usize = 256;

/// All offsets first, then initialized-controller reach, with v1's budget fit.
pub(super) fn plan(
    instrument: &ir::Instrument,
    assets: &[Pcm],
    reachable: &[(u32, u32)],
    frame_bytes: &[usize],
    budget: usize,
) -> (Vec<Vec<Range<usize>>>, usize) {
    assert_eq!(reachable.len(), instrument.zones.len());
    let all: Vec<_> = instrument
        .zones
        .iter()
        .map(|z| (0, z.playback.start_range))
        .collect();
    let reach: Vec<_> = reachable
        .iter()
        .map(|&(lo, hi)| (u64::from(lo), u64::from(hi)))
        .collect();
    let at = |reach: &[(u64, u64)], preload: usize, cover: u64| {
        spans(instrument, assets, reach, preload, cover)
    };
    assert_eq!(frame_bytes.len(), assets.len());
    // Port v1 source-width planning; predictive packing only reduces the result.
    let fits = |ranges: &[Vec<Range<usize>>]| {
        ranges
            .iter()
            .zip(frame_bytes)
            .try_fold(0usize, |n, (ranges, &width)| {
                ranges
                    .iter()
                    .try_fold(n, |n, r| n.checked_add(r.len().checked_mul(width)?))
            })
            .is_some_and(|n| n <= budget)
    };
    let largest = |lo: u64, hi: u64, at: &dyn Fn(u64) -> Vec<Vec<Range<usize>>>| {
        let (mut low, mut high, mut best) = (lo, hi.saturating_add(1), at(lo));
        while high - low > 64 {
            let mid = low + (high - low) / 2;
            let candidate = at(mid);
            if fits(&candidate) {
                low = mid;
                best = candidate;
            } else {
                high = mid;
            }
        }
        (best, low)
    };
    let full = at(&all, PRELOAD, u64::MAX);
    if fits(&full) {
        return (full, PRELOAD);
    }
    if fits(&at(&all, MIN_PRELOAD, u64::MAX)) {
        let (ranges, preload) = largest(MIN_PRELOAD as u64, PRELOAD as u64, &|p| {
            at(&all, p as usize, u64::MAX)
        });
        return (ranges, preload as usize);
    }
    if fits(&at(&reach, PRELOAD, u64::MAX)) {
        let width = all.iter().map(|&(_, hi)| hi).max().unwrap_or(0);
        let (ranges, _) = largest(0, width, &|margin| {
            let widened: Vec<_> = reach
                .iter()
                .zip(&all)
                .map(|(&(lo, hi), &(_, max))| {
                    (
                        lo.saturating_sub(margin),
                        hi.saturating_add(margin).min(max),
                    )
                })
                .collect();
            at(&widened, PRELOAD, u64::MAX)
        });
        return (ranges, PRELOAD);
    }
    if fits(&at(&reach, MIN_PRELOAD, u64::MAX)) {
        let (ranges, preload) = largest(MIN_PRELOAD as u64, PRELOAD as u64, &|p| {
            at(&reach, p as usize, u64::MAX)
        });
        return (ranges, preload as usize);
    }
    if fits(&at(&reach, MIN_PRELOAD, 0)) {
        let width = reach.iter().map(|&(lo, hi)| hi - lo).max().unwrap_or(0);
        return (
            largest(0, width, &|cover| at(&reach, MIN_PRELOAD, cover)).0,
            MIN_PRELOAD,
        );
    }
    let floor = at(&reach, FLOOR_PRELOAD, 0);
    if fits(&floor) {
        let (ranges, preload) = largest(FLOOR_PRELOAD as u64, MIN_PRELOAD as u64, &|p| {
            at(&reach, p as usize, 0)
        });
        return (ranges, preload as usize);
    }
    (floor, FLOOR_PRELOAD)
}

fn spans(
    instrument: &ir::Instrument,
    assets: &[Pcm],
    reach: &[(u64, u64)],
    preload: usize,
    cover: u64,
) -> Vec<Vec<Range<usize>>> {
    let preload = preload as u64;
    let mut ranges = vec![Vec::new(); assets.len()];
    let mut ends = vec![0u64; assets.len()];
    for (zone, &(lo, hi)) in instrument.zones.iter().zip(reach) {
        let id = zone.asset.0;
        let play = zone.playback;
        let end = play
            .end
            .unwrap_or(assets[id].frame_count() as u64)
            .min(assets[id].frame_count() as u64);
        ends[id] = ends[id].max(end);
        let first = lo.saturating_sub(1);
        let head = hi.min(lo.saturating_add(cover)).saturating_add(preload);
        let mut range = if play.reverse {
            end.saturating_sub(head)..end.saturating_sub(first)
        } else {
            play.start.saturating_add(first)..play.start.saturating_add(head)
        };
        if !play.reverse {
            let mut short_loop = |r: ir::LoopRange, until_release: bool| {
                if r.end <= play.start.saturating_add(4 * preload) && play.start < r.end {
                    range.start =
                        range.start.min(r.start.saturating_sub(
                            r.crossfade.frames(f64::from(assets[id].sample_rate())),
                        ));
                    range.end = range.end.max(r.end.saturating_add(if until_release {
                        preload
                    } else {
                        0
                    }));
                }
            };
            match play.looping {
                ir::Looping::Continuous(r) => short_loop(r, false),
                ir::Looping::UntilRelease(r) => short_loop(r, true),
                ir::Looping::Slots(slots) => {
                    for slot in slots.into_iter().flatten() {
                        short_loop(slot.range, slot.until_release);
                    }
                }
                _ => {}
            }
        }
        ranges[id].push(range);
    }
    for (ranges, end) in ranges.iter_mut().zip(ends) {
        ranges.sort_unstable_by_key(|r| r.start);
        let mut merged: Vec<Range<u64>> = Vec::new();
        for range in ranges.drain(..) {
            match merged.last_mut() {
                Some(last) if range.start <= last.end.saturating_add(preload / 2) => {
                    last.end = last.end.max(range.end)
                }
                _ => merged.push(range),
            }
        }
        let covered: u64 = merged
            .iter()
            .map(|r| r.end.min(end).saturating_sub(r.start.min(end)))
            .sum();
        if end > 0 && (end <= 2 * preload || covered.saturating_add(preload) >= end) {
            *ranges = vec![0..end];
        } else {
            *ranges = merged
                .into_iter()
                .filter_map(|r| {
                    let r = r.start.min(end)..r.end.min(end);
                    (!r.is_empty()).then_some(r)
                })
                .collect();
        }
    }
    ranges
        .into_iter()
        .map(|rs| {
            rs.into_iter()
                .map(|r| r.start as usize..r.end as usize)
                .collect()
        })
        .collect()
}

/// v1 keeps short/whole resident loops raw; ordinary onset spans may be predictive.
pub(super) fn compression(
    instrument: &ir::Instrument,
    assets: &[Pcm],
    ranges: &[Vec<Range<usize>>],
    preload: usize,
) -> Vec<bool> {
    let mut compressed = vec![true; assets.len()];
    for zone in &instrument.zones {
        let id = zone.asset.0;
        let p = zone.playback;
        let whole = ranges[id].len() == 1 && ranges[id][0] == (0..assets[id].frame_count());
        let resident = |r: ir::LoopRange| {
            whole
                || (!p.reverse
                    && r.end <= p.start.saturating_add(4 * preload as u64)
                    && p.start < r.end)
        };
        let looping = match p.looping {
            ir::Looping::Continuous(r) | ir::Looping::UntilRelease(r) => resident(r),
            ir::Looping::Slots(slots) => slots.into_iter().flatten().any(|s| resident(s.range)),
            _ => false,
        };
        compressed[id] &= !looping;
    }
    compressed
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source(frames: usize, zones: Vec<ir::Zone>) -> (ir::Instrument, Vec<Pcm>) {
        (
            ir::Instrument {
                zones,
                ..Default::default()
            },
            vec![Pcm::streamed(48000, frames).unwrap()],
        )
    }
    #[test]
    fn forward_and_reverse_offsets_are_source_frames_and_nearby_heads_merge() {
        let mut a = ir::Zone::new(ir::AssetRef(0));
        let mut b = a.clone();
        b.playback.start = 6000;
        let (i, pcm) = source(20000, vec![a.clone(), b]);
        assert_eq!(
            spans(&i, &pcm, &[(0, 0), (0, 0)], PRELOAD, u64::MAX),
            vec![vec![0..10048]]
        );
        a.playback.reverse = true;
        let (i, pcm) = source(20000, vec![a]);
        assert_eq!(
            spans(&i, &pcm, &[(1000, 1000)], PRELOAD, u64::MAX),
            vec![vec![14952..19001]]
        );
    }
    #[test]
    fn native_width_budget_keeps_the_full_preload_and_wider_sources_shrink() {
        let (i, pcm) = source(20000, vec![ir::Zone::new(ir::AssetRef(0))]);
        assert_eq!(plan(&i, &pcm, &[(0,0)], &[4], 17000).1, PRELOAD);
        assert!(plan(&i, &pcm, &[(0,0)], &[8], 17000).1 < PRELOAD);
        assert_eq!(plan(&i, &pcm, &[(0,0)], &[4], 0).1, FLOOR_PRELOAD);
    }
    #[test]
    fn initialized_reach_fits_before_discarding_the_preload() {
        let mut zone = ir::Zone::new(ir::AssetRef(0));
        zone.playback.start_range = 12000;
        let (i, pcm) = source(100000, vec![zone]);
        let (ranges, preload) = plan(&i, &pcm, &[(8000, 8000)], &[8], 32768);
        assert_eq!(preload, PRELOAD);
        assert!(!ranges[0].iter().any(|r| r.contains(&0)));
        assert!(
            ranges[0]
                .iter()
                .any(|r| r.contains(&8000) && r.contains(&12047))
        );
        assert!(ranges[0].iter().map(|r| r.len() * 8).sum::<usize>() <= 32768);
    }
    #[test]
    fn all_short_native_loop_slots_are_resident_and_short_samples_are_whole() {
        let mut zone = ir::Zone::new(ir::AssetRef(0));
        let make = |start, end, until_release| {
            Some(ir::LoopSlot {
                range: ir::LoopRange {
                    start,
                    end,
                    crossfade: ir::Span::Frames(32),
                    alternating: false,
                },
                count: 0,
                tuning: 1.,
                until_release,
            })
        };
        zone.playback.looping = ir::Looping::Slots([
            make(4000, 6000, false),
            None,
            make(7000, 10000, true),
            None,
            None,
            None,
            None,
            None,
        ]);
        let (i, pcm) = source(30000, vec![zone]);
        assert_eq!(
            spans(&i, &pcm, &[(0, 0)], PRELOAD, u64::MAX),
            vec![vec![0..14048]]
        );
        let ranges = spans(&i, &pcm, &[(0, 0)], PRELOAD, u64::MAX);
        assert_eq!(compression(&i, &pcm, &ranges, PRELOAD), vec![false]);
        let (i, pcm) = source(8000, vec![ir::Zone::new(ir::AssetRef(0))]);
        assert_eq!(
            spans(&i, &pcm, &[(0, 0)], PRELOAD, u64::MAX),
            vec![vec![0..8000]]
        );
        let ranges = spans(&i, &pcm, &[(0, 0)], PRELOAD, u64::MAX);
        assert_eq!(compression(&i, &pcm, &ranges, PRELOAD), vec![true]);
    }
}
