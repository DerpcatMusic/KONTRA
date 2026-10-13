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

impl Prepared {
    /// Optional source-resource domain per group, independent of voice-group
    /// membership. Indices name limits supplied to with_voice_limits.
    pub fn with_source_voice_limits(mut self, groups: Vec<Option<usize>>) -> Result<Self, Error> {
        if groups.len() != self.group_count as usize
            || groups
                .iter()
                .flatten()
                .any(|&l| l >= self.voice_limits.len())
        {
            return Err(Error::InvalidInput);
        }
        self.group_source_voice_limits = groups.into_boxed_slice();
        Ok(self)
    }

    /// Allocation domains assigned to the same exclusive set cut other domains
    /// in that set. Self-exclusion is never implied by a domain's membership.
    pub fn with_exclusive_sets(mut self, sets: Vec<Option<u32>>) -> Result<Self, Error> {
        if sets.len() != self.voice_limits.len() {
            return Err(Error::InvalidInput);
        }
        self.exclusive_peers = sets
            .iter()
            .enumerate()
            .map(|(domain, set)| {
                sets.iter()
                    .enumerate()
                    .filter_map(|(peer, other)| {
                        (peer != domain && set.is_some() && set == other).then_some(peer + 1)
                    })
                    .collect()
            })
            .collect();
        Ok(self)
    }
}

impl Prepared {
    /// Groups where a new voice cuts the other notes' voices of the same key
    /// in that group (monophonic release triggers).
    pub fn with_monophonic_release(mut self, groups: Vec<bool>) -> Result<Self, Error> {
        if groups.len() != self.group_count as usize {
            return Err(Error::InvalidInput);
        }
        self.monophonic_release = groups.into_boxed_slice();
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

    /// Voice slots the runtime was built with.
    pub fn voice_slots(&self) -> usize {
        self.voices.slots.len()
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
            self.steals += 1;
            self.steal_voice(victim, stealing.fade);
        }
        while self.voices.available() < required {
            let Some(oldest) = self.slots_where(|v| v.stolen).min_by_key(|&(_, v)| v.born) else {
                break;
            };
            self.end_voice(VoiceId(self.voices.id(oldest.0.get())));
        }
    }

    /// After a selected voice starts, enforce generation-qualified allocation
    /// domains. Logical counts exclude steals immediately; lists retain fades.
    pub(crate) fn enforce_voice_limits(&mut self, plan: PlanId, group: Option<u32>, new: VoiceId) {
        let prepared = &self.plans.get(plan.0).unwrap().prepared;
        let voice_set = group.and_then(|g| {
            prepared
                .group_voice_limits
                .get(g as usize)
                .copied()
                .flatten()
        });
        let source_set = group.and_then(|g| {
            prepared
                .group_source_voice_limits
                .get(g as usize)
                .copied()
                .flatten()
        });
        let selection = group.map(|g| 1 + prepared.voice_limits.len() + g as usize);
        let limits = [
            prepared.voice_limit.map(|l| (l, 0)),
            voice_set.map(|s| (prepared.voice_limits[s], s + 1)),
            source_set
                .filter(|s| Some(*s) != voice_set)
                .map(|s| (prepared.voice_limits[s], s + 1)),
        ];
        if let Some(set) = voice_set {
            self.link_allocation(new, 1, set + 1);
        }
        if let Some(domain) = selection {
            self.link_allocation(new, 2, domain);
        }
        if let Some(set) = source_set {
            self.link_allocation(new, 3, set + 1);
        }
        self.cut_monophonic_release(plan, group, new);
        if let Some(set) = voice_set {
            self.cut_exclusive_domains(plan, set);
        }
        for (limit, domain) in limits.into_iter().flatten() {
            // No allocated-slot or member traversal below the limit.
            while self.domain_live(plan, domain) > limit.voices as usize {
                let members = || {
                    self.domain_members(plan, domain)
                        .filter(|(i, v)| i.get() != new.0.index && !v.stolen)
                        .filter_map(|(i, v)| {
                            let note = self.notes.get(self.families.get(v.family.0)?.note.0)?;
                            let level = if v.started {
                                v.envelope.current() * v.gain
                            } else {
                                -1.0
                            };
                            Some((i, v.born, note.pitch.key(), level, v.envelope.releasing()))
                        })
                };
                // Preserve the pre-M1 Any and Pref.Rel laws until a native
                // labeled-victim probe resolves them (KONTAKT_TOPOLOGY.md T19).
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
                self.steal_voice(index, limit.fade);
            }
        }
    }

    /// Kontakt 8 manual, Group Editor / Voice Groups / Excl.Grp: cut samples
    /// in other voice domains sharing the set. BVoiceLimit reader 0x140d049b0
    /// confirms the saved exclusion field; it does not establish a victim law.
    /// Retain v1 0cb7a8a0 src/engine/mod.rs make_room's victim-domain fade.
    /// See docs/re/KONTAKT_ALLOCATION_M1.md for evidence and native probe limits.
    fn cut_exclusive_domains(&mut self, plan: PlanId, set: usize) {
        let peers = self
            .plans
            .get(plan.0)
            .unwrap()
            .prepared
            .exclusive_peers
            .get(set)
            .map_or(0, |peers| peers.len());
        for p in 0..peers {
            let prepared = &self.plans.get(plan.0).unwrap().prepared;
            let domain = prepared.exclusive_peers[set][p];
            let fade = prepared.voice_limits[domain - 1].fade;
            let mut next = self.domain_head(plan, domain);
            while let Some(index) = next {
                next = self.domain_next(index, domain);
                self.steal_voice(index, fade);
            }
        }
    }

    /// Kontakt 8 manual, Source Module / Sampler / Release Trigger / Monophonic:
    /// repeated notes cut previous release samples. Selection membership is
    /// qualified by PlanId (including generation), not a bare dense group.
    /// Retain the existing 10 ms fade; no native duration is claimed (see
    /// docs/re/KONTAKT_ALLOCATION_M1.md, BVoiceLimit reader 0x140d049b0).
    fn cut_monophonic_release(&mut self, plan: PlanId, group: Option<u32>, new: VoiceId) {
        let prepared = &self.plans.get(plan.0).unwrap().prepared;
        let Some(g) = group.filter(|&g| prepared.monophonic_release.get(g as usize) == Some(&true))
        else {
            return;
        };
        let fade = prepared.rate / 100;
        let domain = 1 + prepared.voice_limits.len() + g as usize;
        let note_of = |rt: &Self, v: &crate::Voice| rt.families.get(v.family.0).map(|f| f.note);
        let Some(new_note) = self.voices.get(new.0).and_then(|v| note_of(self, v)) else {
            return;
        };
        let key = self.notes.get(new_note.0).map(|n| n.pitch.key());
        let mut next = self.domain_head(plan, domain);
        while let Some(index) = next {
            next = self.domain_next(index, domain);
            let voice = self.voices.slots[index.get()].value.as_ref().unwrap();
            if voice.stolen {
                continue;
            }
            let Some(note) = note_of(self, voice) else {
                continue;
            };
            if note != new_note && self.notes.get(note.0).map(|n| n.pitch.key()) == key {
                self.steal_voice(index, fade);
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
