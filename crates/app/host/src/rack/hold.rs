//! Auto-align on the audio side: each note held by its own frames.

use super::{Event, HostPattern};

/// The most velocity ranges a [`Hold`] tells apart.
pub const HOLD_STEPS: usize = 8;

/// Frames a part holds each note so it sounds with the latest part
/// (auto-align): by velocity and by whether another key is held (a legato).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Hold {
    /// By velocity range, rising: its top velocity, then the frames a first
    /// note and a legato wait. A velocity past the last takes the last.
    pub steps: [(u8, u32, u32); HOLD_STEPS],
    /// A legato script plays the part: no note event overtakes an earlier
    /// one, so a legato stays a legato.
    pub mono: bool,
}

impl Hold {
    /// Every note held `frames`.
    #[must_use]
    pub const fn flat(frames: u32) -> Self {
        Self {
            steps: [(127, frames, frames); HOLD_STEPS],
            mono: false,
        }
    }

    fn frames(&self, legato: bool, velocity: u8) -> u32 {
        (self.steps.iter().find(|s| velocity <= s.0))
            .or(self.steps.last())
            .map_or(0, |&(_, first, later)| if legato { later } else { first })
    }

    /// The longest any note waits.
    #[must_use]
    pub fn max(&self) -> u32 {
        (self.steps.iter())
            .map(|&(_, first, legato)| first.max(legato))
            .max()
            .unwrap_or(0)
    }
}

/// A part's notes under its hold: what each key waits, which are down.
pub(super) struct Held {
    pub hold: Hold,
    /// By channel and key, the frames its note-on waited.
    keys: [[u32; 128]; 16],
    /// By channel, the keys down.
    down: [u128; 16],
    /// The latest due time of a note event while `mono`.
    last: u64,
}

impl Held {
    pub fn new(hold: Hold) -> Self {
        Self {
            hold,
            keys: [[0; 128]; 16],
            down: [0; 16],
            last: 0,
        }
    }

    /// Every note let go: nothing is down.
    pub fn clear(&mut self) {
        self.down = [0; 16];
    }

    /// When `body`, arriving at sample `at`, is due. A note's release and
    /// expression wait as long as its note-on did.
    pub fn due(&mut self, at: u64, body: &Event) -> u64 {
        let frames = match *body {
            Event::NoteOn { note, velocity, .. } => {
                let legato = self.down.iter().any(|&d| d != 0);
                let velocity = (velocity * 127.0).round().clamp(0.0, 127.0) as u8;
                let frames = self.hold.frames(legato, velocity);
                let (c, k) = (usize::from(note.channel & 15), note.key & 127);
                if let (Some(slot), Some(down)) = (
                    self.keys.get_mut(c).and_then(|c| c.get_mut(usize::from(k))),
                    self.down.get_mut(c),
                ) {
                    *slot = frames;
                    *down |= 1u128 << k;
                }
                frames
            }
            Event::NoteOff(p) | Event::NoteOffVelocity(p, _) | Event::Choke(p) => {
                let frames = self.waited(p);
                for (c, down) in self.down.iter_mut().enumerate() {
                    if p.channel == -1 || usize::try_from(p.channel).is_ok_and(|p| p == c) {
                        *down &= match u8::try_from(p.key) {
                            Ok(k @ 0..=127) => !(1u128 << k),
                            _ => 0,
                        };
                    }
                }
                frames
            }
            Event::Expression(p, _) => self.waited(p),
            Event::Ump(_) => self.hold.max(),
        };
        let mut due = at + u64::from(frames);
        // Only notes keep their order: a controller stream would hold every
        // legato back by the longest hold.
        let note = !matches!(body, Event::Expression(..) | Event::Ump(_));
        if self.hold.mono && note {
            due = due.max(self.last);
            self.last = due;
        }
        due
    }

    /// What the note `p` names waited; the longest for a wildcard.
    fn waited(&self, p: HostPattern) -> u32 {
        let (Ok(c), Ok(k)) = (usize::try_from(p.channel), usize::try_from(p.key)) else {
            return self.hold.max();
        };
        (self.keys.get(c).and_then(|c| c.get(k)).copied()).unwrap_or_else(|| self.hold.max())
    }
}
