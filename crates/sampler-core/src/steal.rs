//! Voice stealing at capacity: admission fades out existing voices instead of
//! rejecting the new note. Off by default (strict capacity errors); hosts that
//! play instruments turn it on with [`Runtime::set_voice_stealing`].
use crate::{Error, Index, PlanId, Prepared, Runtime, VoiceId};

/// Polyphony of the instrument or of a voice group (Kontakt voice groups):
/// starting a voice past `voices` fades one other out over `fade` frames, a
/// released one first when `prefer_released`, else chosen by `kill`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoiceLimit {
    pub voices: u32,
    pub kill: Kill,
    pub prefer_released: bool,
    pub fade: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kill {
    /// The quietest.
    Any,
    Oldest,
    Newest,
    /// The highest note.
    Highest,
    Lowest,
}

impl Prepared {
    /// `instrument` limits every voice of the plan; `groups[g]` indexes
    /// `limits` for the voices of group `g` (see [`Prepared::with_groups`]).
    pub fn with_voice_limits(
        mut self,
        instrument: Option<VoiceLimit>,
        limits: Vec<VoiceLimit>,
        groups: Vec<Option<usize>>,
    ) -> Result<Self, Error> {
        if groups.len() != self.group_count as usize
            || groups.iter().flatten().any(|&l| l >= limits.len())
            || instrument.iter().chain(&limits).any(|l| l.voices == 0)
        {
            return Err(Error::InvalidInput);
        }
        self.voice_limit = instrument;
        self.voice_limits = limits.into_boxed_slice();
        self.group_voice_limits = groups.into_boxed_slice();
        Ok(self)
    }
}

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

    /// Voices stolen since the runtime started.
    pub fn steals(&self) -> u64 {
        self.steals
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
            self.steals += 1;
            self.choke_voice(victim, stealing.fade);
        }
        while self.voices.available() < required {
            let Some(oldest) = self.slots_where(|v| v.stolen).min_by_key(|&(_, v)| v.born) else {
                break;
            };
            self.end_voice(VoiceId(self.voices.id(oldest.0.get())));
        }
    }

    /// After `new` (of `plan`, in `group`) started: fade out voices past the
    /// instrument's and the group's voice limits.
    pub(crate) fn enforce_voice_limits(&mut self, plan: PlanId, group: Option<u32>, new: VoiceId) {
        let prepared = &self.plans.get(plan.0).unwrap().prepared;
        if prepared.voice_limit.is_none() && prepared.voice_limits.is_empty() {
            return;
        }
        let set_of = |g: Option<u32>| {
            g.and_then(|g| {
                prepared
                    .group_voice_limits
                    .get(g as usize)
                    .copied()
                    .flatten()
            })
        };
        let set = set_of(group);
        let limits = [
            prepared.voice_limit.map(|l| (l, None)),
            set.map(|s| (prepared.voice_limits[s], Some(s))),
        ];
        for (limit, set) in limits.into_iter().flatten() {
            loop {
                let prepared = &self.plans.get(plan.0).unwrap().prepared;
                // (index, born, key, level, releasing) of the other members.
                let members = || {
                    self.slots_where(|v| !v.stolen)
                        .filter(|&(i, _)| i.get() != new.0.index)
                        .filter_map(|(i, v)| {
                            let note = self.notes.get(self.families.get(v.family.0)?.note.0)?;
                            let member = note.plan == plan
                                && set.is_none_or(|s| {
                                    v.group
                                        .and_then(|g| prepared.group_voice_limits.get(g as usize))
                                        .copied()
                                        .flatten()
                                        == Some(s)
                                });
                            let level = if v.started {
                                v.envelope.current() * v.gain
                            } else {
                                -1.0
                            };
                            member.then_some((
                                i,
                                v.born,
                                note.pitch.key(),
                                level,
                                v.envelope.releasing(),
                            ))
                        })
                };
                // ponytail: O(voices) scan per admission (Kontakt instruments all
                // carry a limit); keep per-limit member counts if it shows up.
                if members().count() < limit.voices as usize {
                    break;
                }
                let released = limit.prefer_released && members().any(|m| m.4);
                let pool = members().filter(|m| !released || m.4);
                let victim = match limit.kill {
                    Kill::Oldest => pool.min_by_key(|m| m.1),
                    Kill::Newest => pool.max_by_key(|m| m.1),
                    Kill::Highest => pool.max_by_key(|m| (m.2, m.1)),
                    Kill::Lowest => pool.min_by_key(|m| (m.2, std::cmp::Reverse(m.1))),
                    Kill::Any => pool.min_by(|a, b| a.3.total_cmp(&b.3).then(a.1.cmp(&b.1))),
                };
                let Some((index, ..)) = victim else { break };
                self.voices.at_mut(index).stolen = true;
                self.stolen += 1;
                self.choke_voice(index, limit.fade);
            }
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
