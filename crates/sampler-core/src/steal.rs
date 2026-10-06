//! Voice stealing at capacity: admission fades out existing voices instead of
//! rejecting the new note. Off by default (strict capacity errors); hosts that
//! play instruments turn it on with [`Runtime::set_voice_stealing`].
use crate::{Error, Index, Runtime, VoiceId};

/// Polyphony is `Limits::voices − headroom`. A stolen voice fades over `fade`
/// frames in one of the `headroom` slots; when more voices are stolen at once
/// than there is headroom, the oldest stolen voices end without a fade.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stealing {
    pub fade: u32,
    pub headroom: usize,
}

impl Stealing {
    /// Kontakt's voice-group default fade (10 ms) and a quarter of the slots
    /// as fade headroom.
    pub fn for_limits(rate: u32, voices: usize) -> Self {
        Self {
            fade: rate / 100,
            headroom: voices / 4,
        }
    }
}

impl Runtime {
    /// Steal voices at capacity (`Some`) or reject admissions with
    /// [`Error::Capacity`] (`None`, the default).
    pub fn set_voice_stealing(&mut self, stealing: Option<Stealing>) -> Result<(), Error> {
        if stealing.is_some_and(|s| s.headroom >= self.voices.slots.len()) {
            return Err(Error::InvalidInput);
        }
        self.stealing = stealing;
        Ok(())
    }

    pub fn voice_stealing(&self) -> Option<Stealing> {
        self.stealing
    }

    /// Voices fading after being stolen.
    pub fn stolen_voices(&self) -> usize {
        self.stolen
    }

    /// Make room for `required` new voices within polyphony. Victims, in
    /// order: released voices, oldest first; then the quietest sounding voice.
    pub(crate) fn steal_voices(&mut self, required: usize) {
        let Some(stealing) = self.stealing else {
            return;
        };
        // Sounding (unstolen) voices + reservations + required ≤ slots − headroom.
        while self.voices.available() + self.stolen < required + stealing.headroom {
            let Some(victim) = self.victim() else {
                break;
            };
            self.voices.at_mut(victim).stolen = true;
            self.stolen += 1;
            self.choke_voice(victim, stealing.fade);
        }
        while self.voices.available() < required {
            let Some(oldest) = self.slots_where(|v| v.stolen).min_by_key(|&(_, v)| v.born) else {
                break;
            };
            self.end_voice(VoiceId(self.voices.id(oldest.0.get())));
        }
    }

    fn slots_where(
        &self,
        keep: impl Fn(&crate::Voice) -> bool,
    ) -> impl Iterator<Item = (Index, &crate::Voice)> {
        self.voices
            .slots
            .iter()
            .enumerate()
            .filter_map(move |(i, s)| Some((Index::new(i), s.value.as_ref().filter(|v| keep(v))?)))
    }

    // ponytail: O(voices) scan per stolen voice; keep an age/level heap if chords
    // of hundreds of layers at full polyphony show up in profiles.
    fn victim(&self) -> Option<Index> {
        let candidates = || self.slots_where(|v| !v.stolen);
        candidates()
            .filter(|(_, v)| v.envelope.releasing())
            .min_by_key(|&(_, v)| v.born)
            .or_else(|| {
                candidates().min_by(|(_, a), (_, b)| {
                    let level = |v: &crate::Voice| {
                        if v.started {
                            v.envelope.current() * v.gain
                        } else {
                            // Not yet sounding: the cheapest to take.
                            -1.0
                        }
                    };
                    level(a).total_cmp(&level(b)).then(a.born.cmp(&b.born))
                })
            })
            .map(|(i, _)| i)
    }
}
