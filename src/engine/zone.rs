//! Off-thread zone mapping preparation. Audio installs one prepared snapshot;
//! source identities and sample indices never depend on which zones loaded.
use super::{
    Bank,
    bank::{Frames, SampleData, Span, ZonePlay},
    wavetable,
};
use crate::{
    import::{Group, Zone},
    ksp::engine::{ZoneEdit, ZonePar},
};
use anyhow::{Context as _, Result, ensure};
use std::{collections::VecDeque, path::PathBuf, sync::Arc};

pub(crate) const QUEUE: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Mapping {
    pub group: usize,
    pub low: u8,
    pub high: u8,
}
impl From<&Zone> for Mapping {
    fn from(z: &Zone) -> Self {
        Self {
            group: z.group,
            low: z.low_key,
            high: z.high_key,
        }
    }
}
impl Mapping {
    pub(crate) fn get(self, par: ZonePar) -> i32 {
        match par {
            ZonePar::Group => self.group as i32,
            ZonePar::LowKey => self.low as i32,
            ZonePar::HighKey => self.high as i32,
        }
    }
    pub(crate) fn set(
        &mut self,
        par: ZonePar,
        value: i32,
        groups: usize,
    ) -> Result<(), &'static str> {
        match par {
            ZonePar::Group if value >= 0 && (value as usize) < groups => {
                self.group = value as usize
            }
            ZonePar::LowKey if (0..=127).contains(&value) => self.low = value as u8,
            ZonePar::HighKey if (0..=127).contains(&value) => self.high = value as u8,
            ZonePar::Group => return Err("set_zone_par: group index out of range"),
            _ => return Err("set_zone_par: key must be in 0..127"),
        }
        // An intermediate inverted range selects no keys. Do not reorder it:
        // the following collected setter may finish the requested range.
        Ok(())
    }
}

pub(crate) struct Context {
    groups: Vec<Group>,
    paths: Vec<PathBuf>,
    source_identity: Option<usize>,
    loop_controls: Vec<bool>,
    sources: Vec<Option<crate::audio::Source>>,
    stream_bytes: usize,
    loaded: Vec<bool>,
}
pub(crate) struct State {
    pub maps: Vec<Mapping>,
    zones: Arc<[Zone]>,
    plays: Arc<[ZonePlay]>,
    samples: Vec<SampleData>,
    pub revision: u64,
    native_controls: bool,
    bytes: usize,
}

pub(crate) struct Job {
    pub context: Arc<Context>,
    pub base: Arc<State>,
    pub edit: ZoneEdit,
}
/// Returned whole to the worker after installation/completion or cancellation.
/// Swapping also places the previous buffers here for disposal off audio.
pub(crate) struct Prepared {
    pub context: Arc<Context>,
    pub expected: Arc<State>,
    state: Arc<State>,
    samples: Vec<SampleData>,
    key_start: [u32; 129],
    key_zones: Arc<[u32]>,
    pub edits: Vec<ZoneEdit>,
    pub completed: usize,
    pub installed: bool,
    pub success: bool,
    pub error: Option<String>,
}

impl Bank {
    pub(super) fn prepare_zone_service(
        &mut self,
        source: &[Zone],
        source_identity: Option<usize>,
        sources: Vec<Option<crate::audio::Source>>,
    ) {
        let mut paths = vec![PathBuf::new(); self.samples.len()];
        for p in self.plays.iter() {
            paths[p.sample as usize] = source[p.zone_id as usize].sample.clone();
        }
        let mut loaded = vec![false; source.len()];
        for p in self.plays.iter() {
            loaded[p.zone_id as usize] = true;
        }
        self.zone_context = Some(Arc::new(Context {
            groups: self.groups().to_vec(),
            loaded,
            paths,
            source_identity,
            stream_bytes: self.bytes.saturating_sub(
                self.samples
                    .iter()
                    .flat_map(|s| &s.spans)
                    .map(|span| span.data.bytes())
                    .sum(),
            ),
            sources,
            loop_controls: self
                .base
                .iter()
                .map(|g| g.mods.has_loop_controls())
                .collect(),
        }));
        self.zone_state = Some(Arc::new(State {
            maps: source.iter().map(Mapping::from).collect(),
            zones: self.zones.clone(),
            plays: self.plays.clone(),
            samples: self.samples.clone(),
            revision: 0,
            native_controls: self.native_controls,
            bytes: self.bytes,
        }));
        self.zone_spares =
            VecDeque::with_capacity(source.len().saturating_mul(3).saturating_add(64).max(QUEUE));
    }
    pub(crate) fn zone_par(&self, zone: i32, par: ZonePar) -> Option<i32> {
        usize::try_from(zone)
            .ok()
            .and_then(|id| self.zone_state.as_ref()?.maps.get(id).copied())
            .map(|m| m.get(par))
    }
    pub(crate) fn zone_init_source_matches(&self, source: usize) -> bool {
        self.zone_context
            .as_ref()
            .is_some_and(|c| c.source_identity.is_none_or(|own| own == source))
    }
    pub(crate) fn zone_editable(&self, id: usize) -> bool {
        self.zone_context
            .as_ref()
            .and_then(|c| c.loaded.get(id))
            .copied()
            .unwrap_or(false)
    }
    pub(crate) fn zone_job(&self, edit: ZoneEdit) -> Option<Job> {
        Some(Job {
            context: self.zone_context.clone()?,
            base: self.zone_state.clone()?,
            edit,
        })
    }
    /// Source-only identity is the owning Instrument's stable zone allocation.
    /// A worker rebase is legal only for that same generation and source.
    pub(crate) fn zone_preload_matches(&self, context: &Context) -> bool {
        self.zone_context.as_ref().is_some_and(|own| {
            own.source_identity.is_some()
                && own.source_identity == context.source_identity
                && own.paths == context.paths
                && own
                    .sources
                    .iter()
                    .map(|s| s.as_ref().map(|s| s.version))
                    .eq(context
                        .sources
                        .iter()
                        .map(|s| s.as_ref().map(|s| s.version)))
        })
    }
    pub(crate) fn rebase_zone_preload(
        &mut self,
        context: &Context,
        parent: Arc<State>,
        canceled: &dyn Fn() -> bool,
    ) -> Result<()> {
        ensure!(
            self.zone_preload_matches(context),
            "Zone preload source changed"
        );
        if self
            .zone_parent
            .as_ref()
            .is_some_and(|s| Arc::ptr_eq(s, &parent))
        {
            return Ok(());
        }
        let own = self
            .zone_context
            .as_ref()
            .context("Zone preload context unavailable")?
            .clone();
        let baseline = self
            .zone_state
            .as_ref()
            .context("Zone preload state unavailable")?;
        ensure!(
            baseline.samples.len() == parent.samples.len()
                && baseline.plays.len() == parent.plays.len()
                && baseline
                    .plays
                    .iter()
                    .zip(parent.plays.iter())
                    .all(|(a, b)| a.zone_id == b.zone_id && a.sample == b.sample),
            "Zone preload source layout changed"
        );
        let mut samples = self.samples.clone();
        for (sample, old) in samples.iter_mut().zip(&parent.samples) {
            ensure!(
                sample.frames == old.frames && sample.rate == old.rate,
                "Zone preload sample changed"
            );
            // Existing voices captured a complete WT window. Keep it available
            // when the ordinary RAM head does not cover that window.
            for span in &old.spans {
                if span.start == 0
                    && span.end() == old.frames
                    && !sample
                        .spans
                        .iter()
                        .any(|s| s.start == 0 && s.end() == sample.frames)
                {
                    sample.spans.push(span.clone());
                }
            }
        }
        let baseline = State {
            maps: baseline.maps.clone(),
            zones: baseline.zones.clone(),
            plays: baseline.plays.clone(),
            samples,
            revision: parent.revision,
            native_controls: self.native_controls,
            bytes: self.bytes,
        };
        let (state, samples, key_start, key_zones) =
            Prepared::prepare_maps(&own, &baseline, parent.maps.clone(), canceled)?;
        self.zones = state.zones.clone();
        self.plays = state.plays.clone();
        self.samples = samples;
        self.key_start = key_start;
        self.key_zones = key_zones;
        self.native_controls = state.native_controls;
        self.bytes = state.bytes;
        self.zone_state = Some(state);
        self.zone_parent = Some(parent);
        Ok(())
    }
    /// Constant-size publication; new voices use new geometry, active voices
    /// keep their captured group, map, table and span indices.
    pub(crate) fn install_zone_map(&mut self, prepared: &mut Prepared) -> bool {
        if !prepared.success
            || self
                .zone_context
                .as_ref()
                .is_none_or(|c| !Arc::ptr_eq(c, &prepared.context))
            || self
                .zone_state
                .as_ref()
                .is_none_or(|s| !Arc::ptr_eq(s, &prepared.expected))
        {
            return false;
        }
        std::mem::swap(self.zone_state.as_mut().unwrap(), &mut prepared.state);
        let new = self.zone_state.as_ref().unwrap();
        // Old geometry is retained by prepared.state; replacing these Arc
        // handles cannot release their last reference on the audio thread.
        self.zones = new.zones.clone();
        self.plays = new.plays.clone();
        self.native_controls = new.native_controls;
        self.bytes = new.bytes;
        std::mem::swap(&mut self.samples, &mut prepared.samples);
        std::mem::swap(&mut self.key_start, &mut prepared.key_start);
        std::mem::swap(&mut self.key_zones, &mut prepared.key_zones);
        prepared.installed = true;
        true
    }
}

impl Prepared {
    /// Serialized worker uses the last prepared state in this context so
    /// successive batches cannot overwrite earlier edits awaiting install.
    pub(crate) fn build(
        jobs: Vec<Job>,
        base: Arc<State>,
        canceled: &dyn Fn() -> bool,
    ) -> Box<Self> {
        let context = jobs[0].context.clone();
        let edits: Vec<_> = jobs.iter().map(|job| job.edit).collect();
        let result = Self::prepare(&context, &base, &edits, canceled);
        let (state, samples, key_start, key_zones, error) = match result {
            Ok((state, samples, starts, keys)) => (state, samples, starts, keys, None),
            Err(e) => (
                base.clone(),
                Vec::new(),
                [0; 129],
                Arc::default(),
                Some(format!("{e:#}")),
            ),
        };
        Box::new(Self {
            context,
            expected: base,
            state,
            samples,
            key_start,
            key_zones,
            edits,
            completed: 0,
            installed: false,
            success: error.is_none(),
            error,
        })
    }
    pub(crate) fn resulting_state(&self) -> Arc<State> {
        self.state.clone()
    }
    fn prepare(
        context: &Context,
        base: &State,
        edits: &[ZoneEdit],
        canceled: &dyn Fn() -> bool,
    ) -> Result<(Arc<State>, Vec<SampleData>, [u32; 129], Arc<[u32]>)> {
        let mut maps = base.maps.clone();
        for edit in edits {
            ensure!(!canceled(), "Zone mapping preparation canceled");
            let id = usize::try_from(edit.zone).context("Invalid source zone ID")?;
            ensure!(
                context.loaded.get(id).copied().unwrap_or(false),
                "Source zone has no playable sample; mapping unchanged"
            );
            maps.get_mut(id)
                .context("Invalid source zone ID")?
                .set(edit.par, edit.value, context.groups.len())
                .map_err(anyhow::Error::msg)?;
        }
        Self::prepare_maps(context, base, maps, canceled)
    }
    fn prepare_maps(
        context: &Context,
        base: &State,
        maps: Vec<Mapping>,
        canceled: &dyn Fn() -> bool,
    ) -> Result<(Arc<State>, Vec<SampleData>, [u32; 129], Arc<[u32]>)> {
        let mut zones = base.zones.to_vec();
        let mut plays = base.plays.to_vec();
        let mut samples = base.samples.clone();
        for (z, p) in zones.iter_mut().zip(&mut plays) {
            ensure!(!canceled(), "Zone mapping preparation canceled");
            let m = maps[p.zone_id as usize];
            if Mapping::from(&*z) == m {
                continue;
            }
            z.group = m.group;
            z.low_key = m.low;
            z.high_key = m.high;
            let group = &context.groups[z.group];
            let sample = &mut samples[p.sample as usize];
            let path = &context.paths[p.sample as usize];
            let current = if let Some(original) = context.sources[p.sample as usize].as_ref() {
                let fresh = crate::audio::Sources::default()
                    .sources(&[path.as_path()])
                    .pop()
                    .unwrap()?;
                ensure!(
                    fresh.version == original.version,
                    "Zone sample source changed while preparing mapping"
                );
                Some(fresh)
            } else {
                None
            };
            let (map, _) = super::bank::play_map(z, group, sample.frames)
                .map_err(|(_, e)| anyhow::anyhow!(e))?;
            p.map = map;
            p.start_mod = z
                .start_mod
                .map_or(0, u64::from)
                .min(map.end - map.start - 1);
            p.wavetable = match group.wavetable.as_ref() {
                None => {
                    ensure!(
                        group.source_mode == Some(0),
                        "Target source mode {:?} is not implemented for zone remapping",
                        group.source_mode
                    );
                    None
                }
                Some(source) => {
                    ensure!(
                        wavetable::supported(source, group.key_tracking),
                        "Unsupported wavetable source state for target group {}",
                        z.group
                    );
                    let table = wavetable::Table::new(map.start as usize, map.end as usize)
                        .context("Wavetable requires complete 2048-frame cycles")?;
                    if sample.wavetable_span(table).is_none() {
                        ensure!(
                            !path.as_os_str().is_empty(),
                            "Complete wavetable sample data unavailable"
                        );
                        let source = current
                            .as_ref()
                            .context("Verified wavetable sample source unavailable")?;
                        let mut reader = source.open()?;
                        let header = reader.header();
                        ensure!(
                            header.frames == sample.frames && header.rate == sample.rate,
                            "Zone sample changed while preparing mapping"
                        );
                        let bytes = sample
                            .frames
                            .checked_mul(crate::audio::Pcm::frame_bytes(header.bits) as u64)
                            .context("Wavetable resident size overflow")?;
                        ensure!(
                            bytes
                                <= super::bank::memory_budget()
                                    .saturating_sub(super::bank::resident_bytes())
                                    as u64,
                            "Complete wavetable exceeds resident memory budget"
                        );
                        let (mut ints, mut frames) = (Vec::new(), Vec::new());
                        let data = Frames::new(reader.read_pcm_cancelable(
                            0..sample.frames,
                            false,
                            &mut ints,
                            &mut frames,
                            canceled,
                        )?);
                        // Whole-source span ends at physical EOF. Existing span
                        // indices and their ordering stay valid for old voices.
                        let fresh = crate::audio::Sources::default()
                            .sources(&[path.as_path()])
                            .pop()
                            .unwrap()?;
                        ensure!(
                            fresh.version == source.version,
                            "Zone sample changed during wavetable read"
                        );
                        sample.spans.push(Span { start: 0, data });
                    }
                    Some(table)
                }
            };
        }
        let mut starts = [0; 129];
        let mut keys = Vec::new();
        for note in 0..128u8 {
            starts[note as usize] = u32::try_from(keys.len()).context("Zone key index overflow")?;
            keys.extend(
                zones
                    .iter()
                    .enumerate()
                    .filter(|(_, z)| (z.low_key..=z.high_key).contains(&note))
                    .map(|(i, _)| i as u32),
            );
        }
        starts[128] = u32::try_from(keys.len()).context("Zone key index overflow")?;
        let native_controls = plays.iter().zip(&zones).any(|(p, z)| {
            context.loop_controls[z.group]
                && z.loop_range.as_ref().is_some_and(|l| !l.alternating)
                && p.map
                    .controlled_loop([0.; 2], samples[p.sample as usize].frames)
                    .is_some()
        });
        let bytes = context.stream_bytes
            + samples
                .iter()
                .flat_map(|s| &s.spans)
                .map(|span| span.data.bytes())
                .sum();
        let state = Arc::new(State {
            maps,
            zones: zones.into(),
            plays: plays.into(),
            samples: samples.clone(),
            native_controls,
            bytes,
            revision: base
                .revision
                .checked_add(1)
                .context("Zone mapping revision exhausted")?,
        });
        Ok((state, samples, starts, keys.into()))
    }
}
