//! Auto-align: per-patch latency compensation (07-APP.md §6).
//!
//! Many libraries start their audible attack late on purpose: a breath, a
//! bow noise, a legato transition or a script that waits. Each patch is
//! measured once at load ([`measure`], off the audio thread). The slowest
//! loaded patch sets the plugin's reported latency ([`reported_ms`]), and
//! every other note is held back by the difference to its own delay
//! ([`Holds`], [`Holder`]), so layered patches start together and the host's
//! delay compensation puts every attack on the grid.
//!
//! - Holding is the same live and on playback.
//! - A hold never goes negative: a sample's lead-in is never trimmed.
//! - The note is held, not the audio; its release and expression are held
//!   by as much as its note-on.
//! - An articulation whose legato sounds later than its first note is played
//!   by a legato script ([`Delay::mono`]): its events never overtake an
//!   earlier one of the same part, so a legato stays a legato.

mod holder;
mod onset;

pub use holder::Holder;
pub use onset::{LEAD_MS, onset_ms, transition_ms};

/// The latest a patch may sound and still be aligned; later counts as this.
pub const MAX_MS: f32 = 500.0;
/// Where a note's attack is: its 5 ms loudness first within this of its loudest.
pub const ONSET_DB: f32 = -20.0;
/// Velocities measured, one per [`bucket`]: legato speeds follow velocity in
/// most libraries that have them (≤64 slow, 65–100 medium, 101+ fast).
pub const VELOCITIES: [u8; 3] = [48, 90, 120];

/// The velocity bucket `velocity` falls in.
#[must_use]
pub fn bucket(velocity: u8) -> usize {
    match velocity {
        0..=64 => 0,
        65..=100 => 1,
        _ => 2,
    }
}

/// How late one articulation sounds, in ms after the note-on, as a first
/// note and as a legato (another note held), per velocity bucket; `None`
/// where nothing could be measured.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Delay {
    pub first: [Option<f32>; 3],
    pub legato: [Option<f32>; 3],
}

impl Delay {
    /// The delay of a note: legato or first at its velocity's bucket, else
    /// the nearest bucket measured, else the other kind.
    #[must_use]
    pub fn ms(&self, legato: bool, velocity: u8) -> Option<f32> {
        let near = |row: &[Option<f32>; 3]| {
            let b = bucket(velocity);
            (0..3)
                .flat_map(|d| [b.checked_sub(d), Some(b + d)])
                .flatten()
                .find_map(|i| row.get(i).copied().flatten())
        };
        let (want, other) = if legato {
            (&self.legato, &self.first)
        } else {
            (&self.first, &self.legato)
        };
        near(want).or_else(|| near(other))
    }

    /// The latest it sounds.
    #[must_use]
    pub fn max(&self) -> Option<f32> {
        self.first
            .iter()
            .chain(&self.legato)
            .flatten()
            .copied()
            .reduce(f32::max)
    }

    /// A legato script plays it: at some velocity a legato sounds more than
    /// 15 ms later than a first note.
    #[must_use]
    pub fn mono(&self) -> bool {
        (self.first.iter().zip(&self.legato))
            .any(|(f, l)| matches!((f, l), (Some(f), Some(l)) if l - f > 15.0))
    }
}

/// A patch's timing, kept with it: as loaded, per articulation, and the
/// player's choices.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Timing {
    /// The patch as it loads, before an articulation is picked.
    pub loaded: Delay,
    /// Per articulation, in the patch's order.
    pub articulations: Vec<Delay>,
    /// Every note of the patch is this late (ms), whatever was measured.
    pub override_ms: Option<f32>,
    /// Not aligned: plays as late as the library does, and sets no latency.
    pub exclude: bool,
}

impl Timing {
    fn delay(&self, articulation: Option<usize>) -> &Delay {
        articulation
            .and_then(|a| self.articulations.get(a))
            .unwrap_or(&self.loaded)
    }

    /// How late a note sounds, in ms.
    #[must_use]
    pub fn own(&self, articulation: Option<usize>, legato: bool, velocity: u8) -> f32 {
        if self.exclude {
            return 0.0;
        }
        self.override_ms
            .or_else(|| self.delay(articulation).ms(legato, velocity))
            .unwrap_or(0.0)
            .clamp(0.0, MAX_MS)
    }

    /// The latest any note of the patch sounds (0 when excluded).
    #[must_use]
    pub fn latest(&self) -> f32 {
        if self.exclude {
            return 0.0;
        }
        self.override_ms
            .or_else(|| {
                self.articulations
                    .iter()
                    .chain([&self.loaded])
                    .filter_map(Delay::max)
                    .reduce(f32::max)
            })
            .unwrap_or(0.0)
            .clamp(0.0, MAX_MS)
    }
}

/// The latency to report, in ms: the latest patch.
#[must_use]
pub fn reported_ms(latest: impl IntoIterator<Item = f32>) -> f32 {
    latest.into_iter().fold(0.0, f32::max).clamp(0.0, MAX_MS)
}

/// A patch's holds for the audio thread, in frames: per articulation row
/// (the last is "as loaded"), first note or legato, velocity bucket; and
/// which rows a legato script plays.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Holds {
    rows: Vec<([[u32; 3]; 2], bool)>,
}

impl Holds {
    /// Holds for a patch with `timing` against the `reported` latency (ms).
    #[must_use]
    pub fn new(timing: &Timing, reported: f32, rate: f64) -> Self {
        let rows = (0..timing.articulations.len())
            .map(Some)
            .chain([None])
            .map(|art| {
                let frames = |legato: bool, velocity: u8| {
                    let ms = (reported - timing.own(art, legato, velocity)).max(0.0);
                    (f64::from(ms) * rate / 1000.0).round() as u32
                };
                let row = [false, true].map(|legato| VELOCITIES.map(|v| frames(legato, v)));
                (row, !timing.exclude && timing.delay(art).mono())
            })
            .collect();
        Self { rows }
    }

    fn row(&self, articulation: Option<usize>) -> Option<&([[u32; 3]; 2], bool)> {
        articulation
            .and_then(|a| self.rows.get(a))
            .or_else(|| self.rows.last())
    }

    /// How long a note waits, in frames.
    #[must_use]
    pub fn frames(&self, articulation: Option<usize>, legato: bool, velocity: u8) -> u32 {
        self.row(articulation)
            .and_then(|(row, _)| row.get(usize::from(legato))?.get(bucket(velocity)).copied())
            .unwrap_or(0)
    }

    /// A legato script plays the articulation.
    #[must_use]
    pub fn mono(&self, articulation: Option<usize>) -> bool {
        self.row(articulation).is_some_and(|(_, mono)| *mono)
    }
}

/// A patch the measurement plays: the host wraps a fresh engine (its
/// scripts' state changes as notes run through them).
pub trait Player {
    fn rate(&self) -> f64;
    /// Stops everything and lets it ring out; the patch as loaded.
    fn reset(&mut self);
    /// Picks articulation `row` (a keyswitch or the script's control).
    fn articulation(&mut self, row: usize);
    fn note(&mut self, key: u8, velocity: u8, on: bool);
    /// Renders `frames` more and appends them, summed to mono, to `out`.
    fn render(&mut self, frames: usize, out: &mut Vec<f32>);
}

/// Measures a patch with `articulations` rows by playing `key` as a first
/// note and then a legato a third up, at each of [`VELOCITIES`].
pub fn measure(player: &mut impl Player, articulations: usize, key: u8) -> Timing {
    let loaded = delay(player, None, key);
    let articulations = (0..articulations)
        .map(|a| delay(player, Some(a), key))
        .collect();
    Timing {
        loaded,
        articulations,
        ..Timing::default()
    }
}

fn delay(player: &mut impl Player, articulation: Option<usize>, key: u8) -> Delay {
    let rate = player.rate();
    let frames = |ms: f64| (ms * rate / 1000.0) as usize;
    let second = key.saturating_add(4).min(127);
    let mut d = Delay::default();
    for (b, &velocity) in VELOCITIES.iter().enumerate() {
        player.reset();
        if let Some(a) = articulation {
            player.articulation(a);
        }
        let mut sink = Vec::new();
        player.render(frames(50.0), &mut sink);
        let mut first = Vec::new();
        player.note(key, velocity, true);
        player.render(frames(1000.0), &mut first);
        let mut legato = first
            .get(first.len().saturating_sub(frames(LEAD_MS))..)
            .unwrap_or_default()
            .to_vec();
        player.note(second, velocity, true);
        player.render(frames(50.0), &mut legato);
        player.note(key, 0, false);
        player.render(frames(950.0), &mut legato);
        player.note(second, 0, false);
        if let (Some(f), Some(l)) = (d.first.get_mut(b), d.legato.get_mut(b)) {
            *f = onset_ms(&first, rate, ONSET_DB);
            *l = transition_ms(&legato, rate, second, ONSET_DB);
        }
    }
    d
}
