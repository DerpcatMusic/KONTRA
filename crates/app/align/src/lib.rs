//! Auto-align: per-patch latency compensation (07-APP.md §6).
//!
//! Many libraries start their audible attack late on purpose: a breath, a
//! bow noise, a legato transition or a script that waits. Each patch is
//! measured once at load ([`measure`], off the audio thread). The slowest
//! loaded patch sets the plugin's reported latency ([`reported_ms`]), and
//! every other note is held back by the difference to its own delay
//! ([`Holds`]), so layered patches start together and the host's
//! delay compensation puts every attack on the grid.
//!
//! - Holding is the same live and on playback.
//! - A hold never goes negative: a sample's lead-in is never trimmed.
//! - The note is held, not the audio; its release and expression are held
//!   by as much as its note-on.
//! - An articulation whose legato sounds later than its first note is played
//!   by a legato script ([`Delay::mono`]): its events never overtake an
//!   earlier one of the same part, so a legato stays a legato.

mod onset;

pub use onset::{LEAD_MS, onset_ms, transition_ms};

/// The latest a patch may sound and still be aligned; later counts as this.
pub const MAX_MS: f32 = 500.0;
/// Where a note's attack is: its 5 ms loudness first within this of its loudest.
pub const ONSET_DB: f32 = -20.0;
/// Velocities every measurement plays. Where two neighbours sound more than
/// [`STEP_MS`] apart (a softer layer with a slower attack, a script that
/// picks a slower legato by velocity), more are played between them until
/// the velocity where it changes is found.
pub const VELOCITIES: [u8; 5] = [1, 32, 64, 96, 127];
/// Two velocities sounding this far apart (ms) get a range each.
pub const STEP_MS: f32 = 10.0;
/// The most velocity ranges a [`Delay`] keeps.
pub const MAX_STEPS: usize = 8;

/// How late notes up to velocity `top` sound, in ms after the note-on, as a
/// first note and as a legato (another note held); `None` where nothing
/// could be measured.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Step {
    pub top: u8,
    pub first: Option<f32>,
    pub legato: Option<f32>,
}

impl Step {
    fn ms(&self, legato: bool) -> Option<f32> {
        if legato {
            self.legato.or(self.first)
        } else {
            self.first.or(self.legato)
        }
    }

    /// Sounds audibly apart from `other`.
    fn differs(&self, other: &Self) -> bool {
        let apart = |a: Option<f32>, b: Option<f32>| match (a, b) {
            (Some(a), Some(b)) => (a - b).abs() > STEP_MS,
            (a, b) => a.is_some() != b.is_some(),
        };
        apart(self.first, other.first) || apart(self.legato, other.legato)
    }
}

/// How late one articulation sounds, by velocity range: steps in rising
/// `top` order, the last up to 127.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Delay {
    pub steps: Vec<Step>,
}

impl Delay {
    /// The delay of a note: legato or first at its velocity, else the other
    /// kind.
    #[must_use]
    pub fn ms(&self, legato: bool, velocity: u8) -> Option<f32> {
        (self.steps.iter().find(|s| velocity <= s.top))
            .or(self.steps.last())?
            .ms(legato)
    }

    /// The latest it sounds.
    #[must_use]
    pub fn max(&self) -> Option<f32> {
        (self.steps.iter())
            .flat_map(|s| [s.first, s.legato])
            .flatten()
            .reduce(f32::max)
    }

    /// A legato script plays it: at some velocity a legato sounds more than
    /// 15 ms later than a first note.
    #[must_use]
    pub fn mono(&self) -> bool {
        (self.steps.iter())
            .any(|s| matches!((s.first, s.legato), (Some(f), Some(l)) if l - f > 15.0))
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
/// (the last is "as loaded"), by velocity range (top velocity, first note,
/// legato); and which rows a legato script plays.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Holds {
    rows: Vec<(Vec<(u8, u32, u32)>, bool)>,
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
                let mut tops: Vec<u8> = timing.delay(art).steps.iter().map(|s| s.top).collect();
                if tops.last() != Some(&127) {
                    tops.push(127);
                }
                let steps = (tops.into_iter())
                    .map(|top| (top, frames(false, top), frames(true, top)))
                    .collect();
                (steps, !timing.exclude && timing.delay(art).mono())
            })
            .collect();
        Self { rows }
    }

    fn row(&self, articulation: Option<usize>) -> Option<&(Vec<(u8, u32, u32)>, bool)> {
        articulation
            .and_then(|a| self.rows.get(a))
            .or_else(|| self.rows.last())
    }

    /// By velocity range: its top velocity, then how long a first note and
    /// a legato wait, in frames.
    #[must_use]
    pub fn steps(&self, articulation: Option<usize>) -> &[(u8, u32, u32)] {
        self.row(articulation).map_or(&[], |(steps, _)| steps)
    }

    /// How long a note waits, in frames.
    #[must_use]
    pub fn frames(&self, articulation: Option<usize>, legato: bool, velocity: u8) -> u32 {
        let steps = self.steps(articulation);
        (steps.iter().find(|s| velocity <= s.0))
            .or(steps.last())
            .map_or(
                0,
                |&(_, first, legato_frames)| if legato { legato_frames } else { first },
            )
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
/// note and then a legato a third up, at [`VELOCITIES`] and between them
/// where they sound apart.
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

/// The velocities one articulation plays at most, bisecting included.
const MAX_TAKES: usize = 24;

fn delay(player: &mut impl Player, articulation: Option<usize>, key: u8) -> Delay {
    let mut points: Vec<Step> = (VELOCITIES.iter())
        .map(|&v| take(player, articulation, key, v))
        .collect();
    // Bisects every neighbouring pair that sounds apart down to adjacent
    // velocities: the upper one is where the next range starts.
    let mut i = 0;
    while let (Some(&a), Some(&b)) = (points.get(i), points.get(i + 1)) {
        if points.len() < MAX_TAKES && b.top - a.top > 1 && a.differs(&b) {
            let mid = a.top + (b.top - a.top) / 2;
            points.insert(i + 1, take(player, articulation, key, mid));
        } else {
            i += 1;
        }
    }
    let mut steps: Vec<Step> = Vec::new();
    for p in points {
        let full = steps.len() == MAX_STEPS;
        match steps.last_mut() {
            Some(last) if full || !last.differs(&p) => last.top = p.top,
            Some(last) => {
                last.top = p.top - 1;
                steps.push(p);
            }
            None => steps.push(p),
        }
    }
    Delay { steps }
}

/// One velocity's first note and legato: a [`Step`] whose `top` is it.
fn take(player: &mut impl Player, articulation: Option<usize>, key: u8, velocity: u8) -> Step {
    let rate = player.rate();
    let frames = |ms: f64| (ms * rate / 1000.0) as usize;
    let second = key.saturating_add(4).min(127);
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
    Step {
        top: velocity,
        first: onset_ms(&first, rate, ONSET_DB),
        legato: transition_ms(&legato, rate, second, ONSET_DB),
    }
}
