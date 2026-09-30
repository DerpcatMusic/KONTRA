//! Smart memory: sample heads sized by use, not fixed at load.
//!
//! A bank loads every streamed sample's head, its start preload, as the
//! budget allows. The audio thread counts each sample's note starts; this
//! manager, on the loader thread, turns the counts into heat that halves
//! every [`HALF_LIFE`] and resizes heads around it:
//! - where the budget cut the bank's preload short of [`PRELOAD_FRAMES`]
//!   and its streams then ran late, samples played and those sharing their
//!   keys (velocity layers, round robins, the other articulations a
//!   keyswitch reaches) grow back to it while spare RAM allows: the notes
//!   likely next start with the preload measured never to underrun. Banks
//!   that keep up keep their memory;
//! - under memory pressure the grown heads shrink back first, then heads of
//!   samples cold since the manager began watching drop to [`MIN_PRELOAD`],
//!   the planner's own floor; when RAM frees they grow back.
//!
//! Idle stream rings hand their pages back on their own (`stream.rs`).
//!
//! Samples the manager knows nothing about yet keep their start preload,
//! and a head is only swapped while no voice plays its sample
//! ([`super::Engine::swap_heads`]): a played note never loses data.

use super::bank::{
    Bank, Frames, MIN_PRELOAD, PRELOAD_FRAMES, RAM_HEADROOM, Span, ZonePlay, parallel, ram_free, resident,
    spans,
};
use super::stream::{RingUse, Streamer};
use crate::{audio::Source, import::Zone};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    time::{Duration, Instant},
};

/// Heat halves this often without plays.
const HALF_LIFE: f32 = 60.0;
/// Heat from which a sample counts as likely to play: one play, two half-lives ago.
const LIKELY: f32 = 0.25;
/// Heat under which a sample counts as cold: one play, over four half-lives ago.
const COLD: f32 = 0.05;
/// Watched this long, never-played samples count as cold rather than unknown.
const OBSERVE: Duration = Duration::from_secs(120);
/// Grown heads take at most a quarter of the spare RAM, and this per part.
const HOT_LIMIT: usize = 64 << 20;
/// Pressure ends once this much RAM is spare again, so it does not flap.
const RELIEF: isize = 256 << 20;
/// Heads read per round, so a round stays short.
const READS: usize = 256;

/// A head resized off the audio thread. Once swapped, `spans` holds the
/// head it replaced, to be freed off the audio thread too.
pub struct Head {
    sample: u32,
    tier: Tier,
    /// Bytes of the head as read.
    bytes: usize,
    pub(crate) spans: Vec<Span>,
    applied: bool,
}

impl Head {
    pub(crate) fn sample(&self) -> u32 {
        self.sample
    }

    pub(crate) fn applied(&mut self) {
        self.applied = true;
    }
}

pub type Heads = Box<Vec<Head>>;

/// Zones playing a sample, with the start offsets they keep resident.
pub(crate) type Plays = Vec<(ZonePlay, (u64, u64))>;

/// Per sample, the zones playing it and the keys they cover.
pub(crate) fn by_sample(
    plays: &[ZonePlay],
    zones: &[Zone],
    reach: &[(u64, u64)],
    samples: usize,
) -> Vec<(Plays, (u8, u8))> {
    let mut out = vec![(Vec::new(), (127, 0)); samples];
    for ((play, zone), &reach) in plays.iter().zip(zones).zip(reach) {
        let (plays, keys) = &mut out[play.sample as usize];
        plays.push((*play, reach));
        *keys = (keys.0.min(zone.low_key), keys.1.max(zone.high_key.min(127)));
    }
    out
}

/// How large a head is kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tier {
    Cold,
    Base,
    Hot,
}

struct Tracked {
    sample: u32,
    plays: Plays,
    keys: (u8, u8),
    /// Head preload per tier.
    preload: [u64; 3],
    /// Expected head bytes per tier.
    size: [usize; 3],
    tier: Tier,
    /// Resident bytes of the current head, and of the head it loaded with.
    bytes: usize,
    loaded: usize,
    /// Play count last read.
    seen: u32,
    heat: f32,
}

impl Tracked {
    /// Heat, and a share of its keys': a sample on a key just played is
    /// likely next.
    fn score(&self, keys: &[f32; 128]) -> f32 {
        let (lo, hi) = self.keys;
        let key = keys[lo as usize..=hi as usize].iter().fold(0f32, |m, &k| m.max(k));
        self.heat + 0.5 * key
    }

    fn read(&self, source: &Source, tier: Tier, cover: u64) -> anyhow::Result<Head> {
        let preload = self.preload[tier as usize];
        let plays: Vec<_> = self.plays.iter().map(|(p, r)| (p, *r)).collect();
        let (ranges, _, looping) = spans(&plays, preload, cover);
        // Heads the same size other parts or instances hold are shared: a
        // longer one is not, or shrinking would free nothing.
        let key = (source.path().to_path_buf(), !looping);
        let mut reader = None;
        let (mut ints, mut frames) = (Vec::new(), Vec::new());
        let mut end = 0;
        let spans = ranges
            .into_iter()
            .map(|range| {
                let from = std::mem::replace(&mut end, range.end);
                if let Some(span) = resident::find(&key, &range, from, range.end) {
                    return Ok(span);
                }
                let reader = match &mut reader {
                    Some(r) => r,
                    None => reader.insert(source.open()?),
                };
                let span = Span {
                    start: range.start,
                    data: Frames::new(reader.read_pcm(range, !looping, &mut ints, &mut frames)?),
                };
                resident::insert(key.clone(), &span);
                Ok(span)
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(Head {
            sample: self.sample,
            tier,
            bytes: spans.iter().map(|s: &Span| s.data.bytes()).sum(),
            spans,
            applied: false,
        })
    }
}

/// One part's smart memory (see the module docs).
pub struct Residency {
    usage: Arc<[AtomicU32]>,
    /// The streamer's: every tracked sample has one.
    sources: Arc<[Option<Arc<Source>>]>,
    rings: Arc<RingUse>,
    samples: Vec<Tracked>,
    cover: u64,
    /// Heat per key, from the plays of every sample on it.
    keys: [f32; 128],
    started: Instant,
    polled: Instant,
    pressure: bool,
    /// The part's underrun count when first polled, and whether it rose since.
    underruns: Option<u64>,
    late: bool,
    /// Heads read that the audio thread has not taken yet.
    waiting: Vec<Head>,
    in_flight: bool,
    /// Resident sample bytes.
    bytes: usize,
    /// Bytes heads shrank by below their loaded size.
    freed: usize,
}

impl Residency {
    /// Track `bank`'s streamed samples, if it streams; `plays[sample]`
    /// from [`by_sample`].
    pub(crate) fn new(
        bank: &Bank,
        plays: Vec<(Plays, (u8, u8))>,
        frame_bytes: &[usize],
        cover: u64,
    ) -> Option<Self> {
        let streamer = bank.streamer.as_ref()?;
        let base = bank.preload;
        let preload = [MIN_PRELOAD.min(base), base, PRELOAD_FRAMES.max(base)];
        // Heads that could change size at all.
        let sizes = preload != [base; 3];
        let samples = (plays.into_iter().enumerate())
            .filter(|(i, (plays, _))| sizes && bank.samples[*i].streamed && !plays.is_empty())
            .map(|(i, (plays, keys))| {
                let data = &bank.samples[i];
                let size = preload.map(|p| {
                    let refs: Vec<_> = plays.iter().map(|(p, r)| (p, *r)).collect();
                    let frames: u64 = spans(&refs, p, cover).0.iter().map(|r| r.end - r.start).sum();
                    frames as usize * frame_bytes[i]
                });
                Tracked {
                    sample: i as u32,
                    plays,
                    keys,
                    preload,
                    size,
                    tier: Tier::Base,
                    bytes: data.spans.iter().map(|s| s.data.bytes()).sum(),
                    loaded: data.spans.iter().map(|s| s.data.bytes()).sum(),
                    seen: 0,
                    heat: 0.0,
                }
            })
            .collect();
        Some(Self {
            usage: bank.usage.clone(),
            sources: streamer.sources(),
            rings: streamer.rings(),
            samples,
            cover,
            keys: [0.0; 128],
            started: Instant::now(),
            polled: Instant::now(),
            pressure: false,
            underruns: None,
            late: false,
            waiting: Vec::new(),
            in_flight: false,
            bytes: bank.bytes - Streamer::BYTES,
            freed: 0,
        })
    }

    /// Resident bytes: sample heads and the stream rings in use.
    pub fn resident(&self) -> usize {
        self.bytes + self.rings.bytes()
    }

    /// Bytes handed back and not taken again: shrunk heads and idle rings.
    pub fn freed(&self) -> usize {
        self.freed + self.rings.freed()
    }

    /// One round: read the plays and the part's `underruns` count, then
    /// the heads to resize for the audio thread to swap in, unless it still
    /// holds the last batch.
    pub fn poll(&mut self, underruns: u64) -> Option<Heads> {
        self.late |= *self.underruns.get_or_insert(underruns) < underruns;
        let now = Instant::now();
        let decay = 0.5f32.powf(now.duration_since(self.polled).as_secs_f32() / HALF_LIFE);
        self.polled = now;
        self.keys.iter_mut().for_each(|k| *k *= decay);
        for t in &mut self.samples {
            let count = self.usage[t.sample as usize].load(Ordering::Relaxed);
            let plays = count.wrapping_sub(t.seen) as f32;
            t.seen = count;
            t.heat = t.heat * decay + plays;
            if plays > 0.0 {
                let (lo, hi) = t.keys;
                self.keys[lo as usize..=hi as usize].iter_mut().for_each(|k| *k += plays);
            }
        }
        if self.in_flight {
            return None;
        }
        let spare = ram_free().map(|(free, total)| free as isize - (RAM_HEADROOM + total / 8) as isize);
        self.pressure = under_pressure(self.pressure, spare);
        let scores: Vec<_> = self.samples.iter().map(|t| t.score(&self.keys)).collect();
        let extra = |i: usize| {
            let t = &self.samples[i];
            t.size[Tier::Hot as usize].saturating_sub(t.size[Tier::Base as usize])
        };
        let observed = self.started.elapsed() >= OBSERVE;
        let budget = if self.late { budget(spare, self.pressure) } else { 0 };
        let tiers = tiers(&scores, extra, budget, self.pressure, observed);
        // Heads read for a tier since given up wait no longer; the rest are
        // not read again.
        let mut waiting = vec![false; self.samples.len()];
        let samples = &self.samples;
        self.waiting.retain(|h| {
            let i = samples.binary_search_by_key(&h.sample, |t| t.sample);
            i.is_ok_and(|i| tiers[i] == h.tier && !std::mem::replace(&mut waiting[i], true))
        });
        let mut resize: Vec<_> = (0..samples.len())
            .filter(|&i| tiers[i] != samples[i].tier && !waiting[i])
            .collect();
        // Shrinks first, as pressure asks; then the likeliest.
        resize.sort_by(|&a, &b| (tiers[a] as u8).cmp(&(tiers[b] as u8)).then(scores[b].total_cmp(&scores[a])));
        resize.truncate(READS);
        let cover = self.cover;
        let jobs: Vec<_> = resize.iter().map(|&i| (&self.samples[i], tiers[i])).collect();
        let sources = &self.sources;
        let read = parallel(jobs, |_: &mut (), (t, tier)| {
            let source = sources[t.sample as usize].as_ref()?;
            t.read(source, tier, cover).ok()
        });
        self.waiting.extend(read.into_iter().flatten());
        if !resize.is_empty() {
            // The readers' scratch stays in their threads' heaps otherwise.
            crate::audio::trim_heap();
        }
        if self.waiting.is_empty() {
            return None;
        }
        self.in_flight = true;
        Some(Box::new(std::mem::take(&mut self.waiting)))
    }

    /// The audio thread's answer to [`Residency::poll`]'s batch: swapped
    /// heads are counted and the ones they replaced freed; the others,
    /// whose samples were playing, wait for the next round.
    pub fn returned(&mut self, heads: Heads) {
        self.in_flight = false;
        let mut swapped = false;
        for head in *heads {
            if !head.applied {
                self.waiting.push(head);
                continue;
            }
            swapped = true;
            let Ok(i) = self.samples.binary_search_by_key(&head.sample, |t| t.sample) else {
                continue;
            };
            let t = &mut self.samples[i];
            self.bytes = self.bytes + head.bytes - t.bytes;
            self.freed = self.freed + t.loaded.saturating_sub(head.bytes) - t.loaded.saturating_sub(t.bytes);
            (t.bytes, t.tier) = (head.bytes, head.tier);
        }
        if swapped {
            // Replaced heads leave holes a size apart from what comes next.
            crate::audio::trim_heap();
        }
    }
}

/// Whether RAM is short, given `spare` bytes past the headroom the system
/// keeps, and whether it `was`.
fn under_pressure(was: bool, spare: Option<isize>) -> bool {
    spare.is_some_and(|spare| spare < if was { RELIEF } else { 0 })
}

/// Bytes grown heads may take: a quarter of the spare RAM, none under
/// pressure, and a fixed share where free RAM is not known.
fn budget(spare: Option<isize>, pressure: bool) -> usize {
    match spare {
        _ if pressure => 0,
        Some(spare) => (spare.max(0) as usize / 4).min(HOT_LIMIT),
        None => HOT_LIMIT / 4,
    }
}

/// Each sample's tier from its `score`: the likeliest grow while their
/// `extra` bytes fit `budget`; under `pressure`, once `observed` long
/// enough, the cold shrink.
fn tiers(scores: &[f32], extra: impl Fn(usize) -> usize, budget: usize, pressure: bool, observed: bool) -> Vec<Tier> {
    let mut tiers = vec![Tier::Base; scores.len()];
    if pressure {
        for (tier, &score) in tiers.iter_mut().zip(scores) {
            if observed && score < COLD {
                *tier = Tier::Cold;
            }
        }
        return tiers;
    }
    let mut likely: Vec<_> = (0..scores.len()).filter(|&i| scores[i] >= LIKELY).collect();
    likely.sort_by(|&a, &b| scores[b].total_cmp(&scores[a]));
    let mut left = budget;
    for i in likely {
        let Some(rest) = left.checked_sub(extra(i)) else {
            break;
        };
        (left, tiers[i]) = (rest, Tier::Hot);
    }
    tiers
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heads_never_swap_under_a_playing_or_just_started_voice() {
        use crate::{
            audio::{Pcm, Sample},
            engine::Engine,
            import::{Group, Zone},
        };
        use std::path::PathBuf;
        let zone = Zone {
            high_key: 127,
            sample: PathBuf::from("a"),
            ..Zone::default()
        };
        let frames = vec![[0.5; 2]; 48000];
        let sample = (PathBuf::from("a"), Sample { rate: 48000, frames });
        let mut bank = Bank::from_samples(vec![Group::default()], vec![zone], vec![sample]).unwrap();
        bank.samples[0].streamed = true;
        let mut e = Engine::default();
        e.release = 0.001;
        e.set_bank(Some(Box::new(bank)));
        let render = |e: &mut Engine| e.render(&mut [0.0; 4800], &mut [0.0; 4800]);
        let data = Pcm::pack(&[[0.0; 2]; 16], false);
        let mut heads = [Head {
            sample: 0,
            tier: Tier::Hot,
            bytes: data.bytes(),
            spans: vec![Span { start: 0, data: Frames::new(data) }],
            applied: false,
        }];
        e.note_on(0, 60, 100);
        e.swap_heads(&mut heads);
        assert!(!heads[0].applied, "a note just started keeps its head");
        render(&mut e);
        e.note_off(0, 60);
        e.swap_heads(&mut heads);
        assert!(!heads[0].applied, "a releasing note keeps its head");
        render(&mut e);
        assert_eq!(e.active_voices(), 0);
        e.swap_heads(&mut heads);
        assert!(heads[0].applied, "swapped once silent");
        assert_eq!(e.bank().unwrap().samples[0].spans[0].end(), 16);
        assert_eq!(heads[0].spans[0].end(), 48000, "the old head comes back to be freed");
    }

    #[test]
    fn pressure_scales_the_budget_down_and_back_up() {
        const MIB: isize = 1 << 20;
        assert!(!under_pressure(false, None), "unknown RAM is no pressure");
        assert!(under_pressure(false, Some(-MIB)));
        assert!(under_pressure(true, Some(100 * MIB)), "relief needs a margin");
        assert!(!under_pressure(true, Some(300 * MIB)));
        assert_eq!(budget(Some(-MIB), true), 0);
        assert_eq!(budget(Some(200 * MIB), false), 50 << 20);
        assert_eq!(budget(Some(1 << 40), false), HOT_LIMIT);
    }

    #[test]
    fn likely_samples_grow_within_budget_and_cold_ones_shrink_only_under_pressure() {
        // Played often, played once, a neighbour, never played.
        let scores = [5.0, 1.0, 0.5, 0.0];
        let tiers_at = |budget, pressure, observed| tiers(&scores, |_| 10, budget, pressure, observed);
        use Tier::*;
        assert_eq!(tiers_at(25, false, true), [Hot, Hot, Base, Base], "hottest first");
        assert_eq!(tiers_at(1000, false, true), [Hot, Hot, Hot, Base]);
        assert_eq!(tiers_at(1000, true, true), [Base, Base, Base, Cold]);
        assert_eq!(tiers_at(1000, true, false), [Base; 4], "unknown samples keep their preload");
    }
}
