//! Sample voices, source windows and the per-block render kernel.

use super::{
    DECLICK, EventId, MAX_BLOCK,
    bank::{Bank, Span},
    filter::{FilterKey, LaneFilter, VoiceFilter},
    map::{FOREVER, LoopMap, PlayMap, Run},
    params::{Inputs, PITCH_ENVS},
    stream::Slot,
};
use crate::audio::Frame;

pub use super::envelope::{Ahdsr, Flex, FlexPoint, Phase};
pub(crate) use super::envelope::{Amplitude, Envelope, Fade};
use super::envelope::{Shape, affine};

/// Dense source scratch capacity. Wider trajectories gather only their taps;
/// this memory policy never limits the note's playback rate.
pub(crate) const WINDOW: usize = 4096 + 8;
/// 1.0 in the kernel's 32.32 fixed-point positions.
pub(super) const FIXED_ONE: f64 = (1u64 << 32) as f64;

/// Prepared counted-loop lookahead admits a one-ms ramp of up to six source
/// clock blocks, one rounding point, the current block and endpoint.
const COUNTED_FADE_POINTS: usize = 6 * MAX_BLOCK + 1;

/// Checked 32.32 source increment, without a pitch ceiling or saturating cast.
pub(super) fn sample_clock(step: f64) -> Option<u64> {
    let fixed = step * FIXED_ONE;
    (fixed.is_finite() && fixed >= 0. && fixed < u64::MAX as f64).then_some(fixed as u64)
}
/// Seconds a voice stays muted before its stream pauses.
const PAUSE_AFTER: f32 = 0.1;

/// A voice's streaming slot and how far its data is known to be published.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Stream {
    pub slot: u16,
    pub tag: u16,
    pub trusted: u64,
    /// Stopped while the voice is muted; the slot stays the voice's.
    pub paused: bool,
    /// Generation-scoped packed tap request for the current wide plan.
    pub sparse: Option<super::stream::SparseRequest>,
}

impl Stream {
    fn resume(&mut self, slots: &[Slot], sample: u32, map: &PlayMap, wraps: u64, limit: u64, pos: u64) {
        if !self.paused { return; }
        let first = pos.saturating_sub(1);
        let from = limit.max(first);
        self.tag = slots[self.slot as usize].configure(sample, map, wraps, from, first);
        (self.trusted, self.paused) = (from, false);
        self.sparse = None;
    }
}

/// Voices whose blocks resample alike: the same destination, step, fraction
/// of the position and envelope curve. Resampling is linear, so their source
/// frames sum at the source rate, weighted by their gains, and the lane
/// interpolates once for all of them.
/// Packed in two words, so it passes in registers: `route` holds the step
/// (32.32, under 2^38), the bus plus one (0 for the output) at bit 48 and a
/// set top bit; bit 61 selects native cubic interpolation. `shape` holds
/// the fraction of the position and the decay's bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Lane {
    route: std::num::NonZeroU64,
    shape: u64,
}

impl Lane {
    const STEP: u64 = (1 << 48) - 1;

    /// `step` and `base` in 32.32; `base` in `[1, 2)`, the window
    /// starting one frame before the position. `decay` is the bits of the
    /// per-frame decay factor, 0 for a held gain.
    fn new(bus: Option<u8>, step: u64, base: u64, decay: u32, native: bool) -> Self {
        let bus = bus.map_or(0, |b| u64::from(b) + 1);
        Self {
            route: Self::route(bus << 48 | (step & Self::STEP) | if native { Self::NATIVE } else { 0 }),
            shape: (base & 0xFFFF_FFFF) | u64::from(decay) << 32,
        }
    }

    const NATIVE: u64 = 1 << 61;
    const SOLO: u64 = 1 << 62;

    /// With the top bit set, never zero.
    fn route(bits: u64) -> std::num::NonZeroU64 {
        std::num::NonZeroU64::new(bits | 1 << 63).unwrap_or(std::num::NonZeroU64::MAX)
    }

    /// A voice alone in its lane, rendering itself: one per voice (see
    /// [`Lanes::add`]).
    fn solo(bus: Option<u8>) -> Self {
        let bus = bus.map_or(0, |b| u64::from(b) + 1);
        Self {
            route: Self::route(Self::SOLO | bus << 48),
            shape: 0,
        }
    }

    pub fn is_solo(&self) -> bool {
        self.route.get() & Self::SOLO != 0
    }

    pub fn bus(&self) -> Option<u8> {
        ((self.route.get() >> 48) as u8).checked_sub(1)
    }

    pub fn native(&self) -> bool {
        self.route.get() & Self::NATIVE != 0
    }

    pub fn step(&self) -> u64 {
        self.route.get() & Self::STEP
    }

    pub fn base(&self) -> u64 {
        (1 << 32) | (self.shape & 0xFFFF_FFFF)
    }

    fn decay(&self) -> u32 {
        (self.shape >> 32) as u32
    }

    fn hash(&self) -> u64 {
        (self.route.get() ^ self.shape.rotate_left(29)).wrapping_mul(0x9E37_79B9_7F4A_7C15)
    }

    /// Source frames one block of `n` frames reads.
    pub fn count(&self, n: usize) -> usize {
        ((self.base() + self.step() * (n as u64 - 1)) >> 32) as usize + 4
    }

    /// Resample the summed window `acc` into `left`/`right` with the
    /// lane's envelope curve (`amp` is scratch).
    pub fn mix(&self, acc: &[Frame], amp: &mut [f32], left: &mut [f32], right: &mut [f32]) {
        let n = left.len();
        self.curve(&mut amp[..n]);
        mix_selected(
            self.native(),
            acc,
            self.base(),
            self.step(),
            &amp[..n],
            [1.0; 2],
            [0.0; 2],
            left,
            right,
        );
    }

    /// The envelope's curve over the block, relative to the voices' weights.
    pub fn curve(&self, amp: &mut [f32]) {
        match self.decay() {
            0 => amp.fill(1.0),
            decay => _ = affine(amp, 1.0, (f32::from_bits(decay), 0.0)),
        }
    }
}

/// One block's voices by [`Lane`], in the order lanes first appear, each a
/// chain of its voices: an open-addressed table, so grouping is linear in
/// the voices, with no sort and no allocation.
pub(crate) struct Lanes {
    /// `(stamp, index into `lanes`)`; entries of older stamps are free.
    table: Box<[(u32, u16)]>,
    stamp: u32,
    /// Each lane, its filter class ([`Plan::class`]), its first and last
    /// voice and how many it has.
    pub lanes: Vec<(Lane, u64, u16, u16, u16)>,
    /// Each lane's filter, for classes other than 0.
    pub keys: Box<[FilterKey]>,
    /// The next voice of each voice's lane, and each voice's lane.
    next: Box<[u16]>,
    of: Box<[u16]>,
}

impl Lanes {
    pub fn new(voices: usize) -> Self {
        Self {
            table: vec![(0, 0); (2 * voices).next_power_of_two()].into_boxed_slice(),
            stamp: 0,
            lanes: Vec::with_capacity(voices),
            keys: vec![FilterKey::default(); voices].into_boxed_slice(),
            next: vec![0; voices].into_boxed_slice(),
            of: vec![0; voices].into_boxed_slice(),
        }
    }

    pub fn clear(&mut self) {
        self.stamp = self.stamp.wrapping_add(1);
        if self.stamp == 0 {
            self.table.fill((0, 0));
            self.stamp = 1;
        }
        self.lanes.clear();
    }

    #[inline(always)]
    pub fn add(&mut self, mut lane: Lane, class: u64, key: &FilterKey, voice: u16) {
        if lane.is_solo() {
            lane.shape = u64::from(voice);
        }
        let mask = self.table.len() - 1;
        let mut h =
            ((lane.hash() ^ class.wrapping_mul(0xD6E8_FEB8_6659_FD93)) >> 40) as usize & mask;
        loop {
            let (stamp, i) = self.table[h];
            if stamp != self.stamp {
                self.of[voice as usize] = self.lanes.len() as u16;
                self.table[h] = (self.stamp, self.lanes.len() as u16);
                if class != 0 {
                    self.keys[self.lanes.len()] = *key;
                }
                self.lanes.push((lane, class, voice, voice, 1));
                return;
            }
            let entry = &mut self.lanes[i as usize];
            if entry.0 == lane && entry.1 == class && (class == 0 || self.keys[i as usize] == *key)
            {
                self.of[voice as usize] = i;
                self.next[entry.3 as usize] = voice;
                (entry.3, entry.4) = (voice, entry.4 + 1);
                return;
            }
            h = (h + 1) & mask;
        }
    }

    /// `voice` has no lane this block.
    pub fn skip(&mut self, voice: u16) {
        self.of[voice as usize] = u16::MAX;
    }

    /// Whether `voice` renders in its lane this block: it shares it, or
    /// its filter.
    pub fn shared(&self, voice: u16) -> bool {
        self.lanes
            .get(self.of[voice as usize] as usize)
            .is_some_and(|l| l.4 > 1 || l.1 != 0)
    }

    /// The voices of the lane starting at `first`, `count` of them.
    pub fn members(&self, first: u16, count: u16) -> impl Iterator<Item = u16> + '_ {
        std::iter::successors(Some(first), |&v| Some(self.next[v as usize])).take(count as usize)
    }
}

/// A voice's next block as [`Voice::plan`] works it out.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Plan {
    pub counted_cursor: Option<super::counted_loop::Cursor>,
    pub n: usize,
    /// Source frames per output frame, 32.32 fixed point.
    pub step: u64,
    pub travel: u64,
    pub last: u64,
    pub curved_pitch: bool,
    /// The source span exceeds dense scratch; gather four taps per output.
    pub wide: bool,
    /// Ratio before the lagged pitch curve; stable routes stay in this ratio.
    pub base_step: f64,
    pub base_semitones: f32,
    pub lagging: bool,
    pub lag_gain: f32,
    /// First unplayable sample boundary, retained after advancing the voice.
    pub terminal: Option<usize>,
    pub target: [f32; 2],
    pub muted: bool,
    /// Ordinary output frames, or counted source distance, to the near EOF.
    pub declick: Option<f64>,
    /// Per-channel gain in its lane.
    pub weights: [f32; 2],
    /// The lane's group filter: 0 for none, else the hash of its key
    /// ([`VoiceFilter::held`]) and bus.
    pub class: u64,
}

pub(super) enum Start {
    Pending,
    Canceled,
    Failed(super::stream::StreamError),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Onset { New, Waiting, Started }

/// Actual DSP receipt; an underrun can still contain a rendered source prefix.
pub(super) struct Rendered {
    pub alive: bool,
    pub underrun: bool,
    pub work: bool,
}

/// One playing zone. Plain data; the engine owns the storage.
/// One voice's reusable route history and pitch-preview scratch, prepared by
/// the loader. The player recycles these boxes and retires the pool with Bank.
pub(super) struct VoiceRoutes {
    pub values: Box<[Option<f64>]>,
    pub targets: Box<[Option<f32>]>,
    pub preview: Box<[Option<f64>]>,
    pub external: Box<[super::filter::ExternalState]>,
}

impl VoiceRoutes {
    pub(super) fn prepare(settings: &[super::GroupSettings]) -> anyhow::Result<Vec<Self>> {
        for group in settings {
            anyhow::ensure!(group.envelope.as_ref().is_none_or(Ahdsr::prepared_valid)
                && group.pitch_envelopes.iter().all(|source| source.env.prepared_valid())
                && group.flex.as_ref().is_none_or(Flex::finite)
                && group.filter.as_deref().is_none_or(super::filter::GroupFilter::envelopes_prepared_valid),
                "Prepared envelope source contains a nonfinite value");
            anyhow::ensure!(group.filter.as_deref()
                .is_none_or(super::filter::GroupFilter::processed_fields_finite),
                "Prepared group FX contains a nonfinite processed field");
            anyhow::ensure!(!group.native_volume_env || group.envelope.as_ref()
                .is_none_or(|source| (-1.0..=1.0).contains(&source.curve)),
                "Native primary AHDSR curve is outside the saved source domain [-1, 1]");
            anyhow::ensure!(group.wavetable.is_none() || group.pitch_glides.is_empty(),
                "Glide on wavetable sources is outside the verified sampler source contract");
            super::lfo::validate_sources(&group.pitch_lfos, &group.volume_lfos)?;
            anyhow::ensure!(group.wavetable.is_none() || !group.pitch_lfos.iter()
                .chain(group.volume_lfos.iter().map(|l| &l.source)).any(|l| l.shared),
                "Shared Digital LFOs are outside the verified ordinary sampler source contract");
            super::glide::validate_sources(&group.pitch_glides, &group.pitch_lfos, &group.volume_lfos)?;
        }
        let voiced = settings.iter().map(|g| g.mods.voiced_count()).max().unwrap_or(0);
        let external = settings.iter().map(|g| g.filter.as_deref()
            .map_or(0, super::filter::GroupFilter::external_count)).max().unwrap_or(0);
        let bytes = voiced.checked_mul(2 * std::mem::size_of::<Option<f64>>() + std::mem::size_of::<Option<f32>>())
            .and_then(|n| external.checked_mul(std::mem::size_of::<super::filter::ExternalState>())
                .and_then(|m| n.checked_add(m)))
            .and_then(|n| n.checked_mul(super::MAX_VOICES));
        anyhow::ensure!(bytes.is_some_and(|n| n <= isize::MAX as usize),
            "voice modulation storage size exceeds the addressable allocation limit");
        fn filled<T: Clone + Default>(count: usize) -> anyhow::Result<Box<[T]>> {
            let mut values = Vec::new();
            values.try_reserve_exact(count)?;
            values.resize(count, T::default());
            Ok(values.into_boxed_slice())
        }
        let mut rows = Vec::new();
        rows.try_reserve_exact(super::MAX_VOICES)?;
        for _ in 0..super::MAX_VOICES {
            rows.push(Self { values: filled(voiced)?, targets: filled(voiced)?,
                preview: filled(voiced)?, external: filled(external)? });
        }
        Ok(rows)
    }
}

pub(crate) struct Voice {
    pub event: EventId,
    pub host_note: Option<super::HostRef>,
    pub per_note: Option<super::PerNoteInput>,
    pub zone_id: u32,
    pub group: u32,
    pub voice_group: Option<u16>,
    pub channel: u8,
    pub note: u8,
    /// Zone/KSP projection; DSP reads onset_velocity.
    pub velocity: u8,
    pub onset_velocity: crate::midi::Velocity,
    pub release_velocity: Option<crate::midi::Velocity>,
    /// Counter timing belongs to this event, independent of same-key retriggers.
    pub counter_start: u64,
    pub counter_stop: Option<u64>,
    pub owner: Option<(u8, u8)>,
    pub input_channel: Option<u8>,
    /// The key or event is still down.
    pub held: bool,
    /// This voice was held at the sostenuto pedal's down edge.
    pub sostenuto: bool,
    /// The envelope has been released.
    pub released: bool,
    /// Started by a note release; ignores later note-offs and pedal changes.
    pub release_trigger: bool,
    /// A former MPE note keeps its expression when its member channel is reused.
    pub frozen_expression: Option<super::Expression>,
    /// Primary note age, shared by all layers of that note.
    pub age: u64,
    /// Sample admission order retains oldest ties after voice storage compaction.
    pub admission: u64,
    pub sample: u32,
    pub wavetable: Option<super::wavetable::Table>,
    pub span: u32,
    pub map: PlayMap,
    pub counted: Option<super::counted_loop::Cursor>,
    /// Saved geometry; control offsets always apply to it, never accumulate.
    pub base_loop: Option<LoopMap>,
    pub loop_offsets: [f32; 2],
    pub loop_deferred: Option<[f32; 2]>,
    pub loop_deferred_edits: u32,
    pub wraps: u64,
    /// Virtual path length for the current wraps.
    pub length: u64,
    /// Virtual frames below this are resident; the rest stream.
    pub limit: u64,
    /// Virtual playback position.
    pub pos: f64,
    /// Source frames per output frame before group tune, modulation and
    /// scripted tuning.
    pub step: f64,
    /// Scripted tuning ratio.
    pub tune: f64,
    /// Cached `(semitones, ratio)` of group, instrument and modulated pitch.
    pub pitch: (f32, f64),
    /// Current value of each of the group's voiced modulation assignments.
    pub mods: Box<[Option<f64>]>,
    pub mod_targets: Box<[Option<f32>]>,
    pub mod_preview: Box<[Option<f64>]>,
    pub script_mods: [i32; super::SCRIPT_MOD_SOURCES],
    pub pitch_envs: [Envelope; PITCH_ENVS],
    pub pitch_lfo: super::lfo::Clock,
    pub pitch_glide: Option<super::glide::Clock>,
    /// The last modulation result `(gain, semitones)`, and the input stamp
    /// ([`Context::inputs`]) it holds for while the modulation is settled.
    pub modulated: (f32, f32),
    pub settled: Option<u32>,
    pub stream: Option<Stream>,
    /// AHDSR envelope (the engine defaults without one, unity with only a flex).
    pub env: Amplitude,
    pub primary_amp: Option<super::envelope::PrimaryAmplitude>,
    /// The group's flex envelope, multiplied with `env`.
    pub flex: Option<Envelope>,
    pub fade: Fade,
    /// Zone gain with velocity and key crossfades; group volume and
    /// modulation apply per block.
    pub base_level: f32,
    /// Scripted event volume.
    pub volume: f32,
    /// Zone pan.
    pub base_pan: f32,
    /// Scripted event pan, added to zone and group pan.
    pub pan: f32,
    /// Next sample's channel gains; live edits ramp over five milliseconds.
    pub gains: [f32; 2],
    pub gain_start: [f32; 2],
    pub gain_target: [f32; 2],
    pub gain_remaining: u32,
    pub gain_frames: u32,
    /// Frames the voice has been muted for, to pause its stream.
    pub muted: u32,
    /// Admission waits without aging the voice. Only an observed cold wait
    /// cancels its unheard attack after release; resident releases still play.
    pub(super) onset: Onset,
    /// Group insert filter state (untouched when the group has none).
    pub filter: VoiceFilter,
    pub plan: Plan,
}

/// Borrowed state shared by all voices of one render block.
pub(crate) struct Context<'a> {
    pub bank: &'a Bank,
    pub settings: &'a [super::GroupSettings],
    pub instrument: super::instrument::InstrumentControls,
    pub slots: &'a [Slot],
    /// One immutable owner supplies physical controls and delivered expression.
    pub(super) midi: &'a super::midi_state::MidiState,
    pub native_mpe_pressure: bool,
    /// Verified ordinary Sampler cubic policy; higher qualities retain their existing path.
    pub native_sampler: bool,
    pub host_notes: &'a super::host_notes::Owners,
    pub per_note: &'a super::per_note::InputControls,
    /// Instrument tune in semitones.
    pub tune: f32,
    pub rate: f32,
    pub tempo: f32,
    pub blocking: bool,
    pub stream_error: &'a std::cell::Cell<Option<super::stream::StreamError>>,
    /// Native sample-geometry controls update only at the 32-frame edge.
    pub native_control_tick: bool,
    /// Changes whenever anything modulation reads may have: controllers,
    /// bend, pressure or group parameters.
    pub inputs: u32,
    pub shared_lfos: &'a super::lfo::Shared,
    pub shared_ahdsr: Option<&'a super::ahdsr_shared::Range>,
    pub lfo_timings: &'a super::lfo::Timings,
    pub now: u64,
}

/// Source readiness without committing onset, amplifier/filter interpolation,
/// or waveform clocks. The temporary source arrays stay inside preparation.
pub(super) struct PreparedSource {
    pitch: (f32, f64), initial: bool, input: u8, x: super::Expression,
    curved_pitch: bool, terminal: Option<usize>, target: [f32; 2],
    lagging: bool, lag_gain: f32, base_step: f64, base_semitones: f32,
    counted_cursor: Option<super::counted_loop::Cursor>, fixed_step: u64,
    travel: u64, last: u64, wide: bool,
}

impl PreparedSource {
    pub(super) fn has_output(&self) -> bool { self.terminal != Some(0) }
}

impl<'a> Context<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(bank: &'a Bank, state: &'a super::instrument::InstrumentState,
        midi: &'a super::midi_state::MidiState, host_notes: &'a super::host_notes::Owners,
        per_note: &'a super::per_note::InputControls, instrument: super::instrument::InstrumentControls,
        native_mpe_pressure: bool, rate: f32, tempo: f32,
        stream_error: &'a std::cell::Cell<Option<super::stream::StreamError>>,
        now: u64, blocking: bool, tune: f32) -> Self {
        Self { bank, settings: &state.settings, instrument, slots: bank.slots(), midi,
            native_mpe_pressure, host_notes, per_note,
            tune: state.controls.tune + f32::from(bank.transpose) + tune,
            rate, tempo, blocking, stream_error, native_control_tick: now & 31 == 0,
            native_sampler: state.sampling_profile == crate::import::SamplingProfile::Kontakt,
            inputs: midi.revision, shared_lfos: &state.shared_lfos,
            shared_ahdsr: state.shared_ahdsr.as_deref(), lfo_timings: &state.lfo_timings, now }
    }


    /// Physical performance inputs; delivered KSP channels do not own these rows.
    pub fn inputs(
        &self,
        physical_input: u8,
        note: u8,
        velocity: crate::midi::Velocity,
        release_velocity: Option<crate::midi::Velocity>,
        expression: super::Expression,
    ) -> Inputs<'_> {
        self.midi.modulation_inputs(physical_input, note, velocity, release_velocity, expression)
    }
}

/// Preallocated per-engine render buffers.
pub(crate) struct Scratch {
    pub window: Box<[Frame]>,
    pub amp: [f32; MAX_BLOCK],
    pub flex: [f32; MAX_BLOCK],
    pub positions: [u64; MAX_BLOCK],
    pub gains: [Frame; MAX_BLOCK],
    pub mod_gain: [f32; MAX_BLOCK],
    pub mod_pitch: [f32; MAX_BLOCK],
    /// A filtered voice's own output before it joins the mix.
    pub out: [[f32; MAX_BLOCK]; 2],
    /// A lane's summed window.
    pub acc: Box<[Frame]>,
}

impl Default for Scratch {
    fn default() -> Self {
        Self {
            window: vec![[0.0; 2]; WINDOW].into_boxed_slice(),
            amp: [0.0; MAX_BLOCK],
            flex: [0.0; MAX_BLOCK],
            positions: [0; MAX_BLOCK],
            gains: [[0.; 2]; MAX_BLOCK],
            mod_gain: [1.; MAX_BLOCK],
            mod_pitch: [0.; MAX_BLOCK],
            out: [[0.0; MAX_BLOCK]; 2],
            acc: vec![[0.0; 2]; WINDOW].into_boxed_slice(),
        }
    }
}

/// Balance law shared with the rack: the far side attenuates linearly.
#[inline]
pub(crate) fn balance(gain: f32, pan: f32) -> [f32; 2] {
    // Scripts set gains and pans: NaN or infinity silences, never poisons, the mix.
    if !gain.is_finite() || pan.is_nan() {
        return [0.0; 2];
    }
    [gain * (1.0 - pan.max(0.0)), gain * (1.0 + pan.min(0.0))]
}

impl Voice {
    /// Main physical sample tap; a wavetable has no single source cursor.
    pub(super) fn physical_pos(&self) -> Option<f64> {
        if let Some(cursor) = self.counted {
            return (cursor.active() || cursor.position < self.map.end as f64).then_some(cursor.position);
        }
        self.wavetable.is_none().then(|| self.map.run(self.pos as u64, self.wraps)).flatten()
            .map(|run| run.frame as f64 + if run.reverse { -self.pos.fract() } else { self.pos.fract() })
    }

    pub(super) fn into_routes(self) -> VoiceRoutes {
        VoiceRoutes { values: self.mods, targets: self.mod_targets,
            preview: self.mod_preview, external: self.filter.ext }
    }
}

impl Voice {
    /// An unavailable start stays at its authored source/envelope phase until
    /// its actual interpolation window is ready. Realtime onset may be late;
    /// offline readiness failures are reported rather than counted as audio.
    fn admit(&self, cx: &Context, last: u64, wide: bool, positions: &[u64], travel: u64, curved: bool) -> Result<Option<super::stream::SparseRequest>, Start> {
        if self.wavetable.is_some() { return Ok(None); }
        if let Some(quality) = self.native_hq(cx) {
            let mut points = [super::hq::Point::default(); MAX_BLOCK];
            Self::hq_points(cx.bank, self.counted, self.map.start as f64 + self.pos, self.map.end,
                positions, travel, curved, &mut points[..positions.len()]).map_err(|kind| self.source_error(kind))?;
            let mut resident = true;
            for &point in &points[..positions.len()] {
                if point.ended { continue; }
                let (first, count) = point.window(quality).map_err(|kind| self.source_error(kind))?;
                resident &= super::hq::covered(point, first, count, |first, count|
                    cx.bank.samples[self.sample as usize].resident_window(first, count));
            }
            if resident { return Ok(None); }
            let stream = self.stream.ok_or_else(|| self.source_error(super::stream::StreamErrorKind::Unavailable))?;
            let slot = &cx.slots[stream.slot as usize];
            let request = slot.request_kernel(stream.tag, quality, &points[..positions.len()])
                .map_err(|kind| self.source_error(kind))?;
            match slot.ready_sparse(stream.tag, request, cx.blocking) {
                Ok(true) => {},
                Ok(false) if self.onset != Onset::Started => return Err(Start::Pending),
                Err(kind) => return Err(self.source_error(kind)),
                Ok(false) => {},
            }
            return Ok(Some(request));
        }
        if let Some(cursor) = self.counted {
            let mut taps = [u64::MAX; MAX_BLOCK * 8];
            let stride = if cx.bank.counted_loops[cursor.rows as usize].crossfaded() { 8 } else { 4 };
            self.counted_taps(cx.bank, cursor, positions, &mut taps[..positions.len() * stride])
                .ok_or_else(|| self.source_error(super::stream::StreamErrorKind::UnrepresentablePitch))?;
            let taps = &taps[..positions.len() * stride];
            let sample = &cx.bank.samples[self.sample as usize];
            if taps.iter().all(|&tap| tap == u64::MAX || sample.span_at(tap).is_some()) { return Ok(None); }
            let stream = self.stream.ok_or_else(|| self.source_error(super::stream::StreamErrorKind::Unavailable))?;
            let slot = &cx.slots[stream.slot as usize];
            let request = slot.request_sparse(stream.tag, taps).map_err(|kind| self.source_error(kind))?;
            match slot.ready_sparse(stream.tag, request, cx.blocking) {
                Ok(true) => {},
                Ok(false) if self.onset != Onset::Started => return Err(Start::Pending),
                Err(kind) if self.onset != Onset::Started => return Err(self.source_error(kind)),
                Err(kind) => { if cx.stream_error.get().is_none() { cx.stream_error.set(Some(super::stream::StreamError {
                    sample:self.sample, frame:cursor.position as u64, kind })); } },
                Ok(false) => {},
            }
            return Ok(Some(request));
        }
        let first = self.pos as i64 - 1;
        let base = ((self.pos - first as f64) * FIXED_ONE) as u64;
        let count = ((base + last) >> 32) + 4;
        let need = (first.saturating_add(count as i64).max(0) as u64).min(self.length);
        if wide && need > self.limit {
            let Some(stream) = self.stream else {
                return Err(self.source_error(super::stream::StreamErrorKind::Unavailable));
            };
            let slot = &cx.slots[stream.slot as usize];
            let mut taps = [u64::MAX; MAX_BLOCK * 4];
            for (i, offset) in positions.iter().enumerate() {
                let at = first + ((base + offset) >> 32) as i64 - 1;
                for tap in 0..4 {
                    let v = at + tap as i64;
                    if v >= 0 && (v as u64) < self.length { taps[i * 4 + tap] = v as u64; }
                }
            }
            let request = slot.request_sparse(stream.tag, &taps[..positions.len() * 4])
                .map_err(|kind| self.source_error(kind))?;
            match slot.ready_sparse(stream.tag, request, cx.blocking) {
                Ok(true) => {},
                Ok(false) if self.onset != Onset::Started => return Err(Start::Pending),
                Err(kind) if self.onset != Onset::Started => return Err(self.source_error(kind)),
                Err(kind) => {
                    if cx.stream_error.get().is_none() {
                        cx.stream_error.set(Some(super::stream::StreamError { sample:self.sample, frame:self.pos as u64, kind }));
                    }
                },
                Ok(false) => {}, // Established realtime underruns keep their clocks.
            }
            return Ok(Some(request));
        }
        if self.onset == Onset::Started { return Ok(None); }
        if need > self.limit {
            let Some(stream) = self.stream else {
                return Err(Start::Failed(super::stream::StreamError {
                    sample: self.sample, frame: self.limit, kind: super::stream::StreamErrorKind::Unavailable,
                }));
            };
            match cx.slots[stream.slot as usize].ready(stream.tag, need, cx.blocking) {
                Ok(true) => {},
                Ok(false) => return Err(Start::Pending),
                Err(kind) => return Err(Start::Failed(super::stream::StreamError {
                    sample: self.sample, frame: self.limit, kind,
                })),
            }
        }
        Ok(None)
    }

    fn counted_taps(&self, bank: &Bank, mut cursor: super::counted_loop::Cursor,
        positions: &[u64], out: &mut [u64]) -> Option<super::counted_loop::Cursor> {
        let data = &bank.counted_loops[cursor.rows as usize];
        let mut previous = 0;
        let stride = if data.crossfaded() { 8 } else { 4 };
        for (&offset, taps) in positions.iter().zip(out.chunks_exact_mut(stride)) {
            cursor.advance(data, offset.checked_sub(previous)? as f64 / FIXED_ONE)?;
            if stride == 8 { taps.copy_from_slice(&cursor.crossfade_taps(data)); }
            else { taps.copy_from_slice(&cursor.taps(data)); }
            previous = offset;
        }
        Some(cursor)
    }

    /// Follow actual future source points only far enough to locate the
    /// existing one-millisecond end ramp. Ordered overshoot can skip rows;
    /// summing their nominal repeat distances would invent a different tail.
    fn counted_end(&mut self, cx: &Context, n: usize, base_step: f64, lagging: bool) -> Option<f64> {
        let mut cursor = self.counted?;
        let data = &cx.bank.counted_loops[cursor.rows as usize];
        if data.linear() { return Some(data.end as f64 - cursor.position); }
        if cursor.unlimited(data) { return None; }
        let mut clock = self.pitch_lfo;
        let mut glide = self.pitch_glide?;
        let mut envelopes = self.pitch_envs;
        self.mod_preview.copy_from_slice(&self.mods);
        let horizon = n.checked_add((DECLICK * cx.rate).ceil() as usize)?.checked_add(1)?;
        let mut done = 0;
        let mut distance = 0.;
        while done < horizon {
            let count = (horizon - done).min(MAX_BLOCK);
            let mut positions = [0; MAX_BLOCK];
            let (travel, _) = self.preview_source(cx, &mut clock, &mut glide, &mut envelopes,
                base_step, lagging, done, &mut positions[..count])?;
            let mut previous = 0;
            for &offset in &positions[..count] {
                let step = offset.checked_sub(previous)? as f64 / FIXED_ONE;
                cursor.advance(data, step)?;
                if cursor.done(data) {
                    return Some(distance + offset as f64 / FIXED_ONE
                        - (cursor.position - data.end as f64));
                }
                previous = offset;
            }
            let step = travel.checked_sub(previous)? as f64 / FIXED_ONE;
            cursor.advance(data, step)?;
            if cursor.done(data) {
                return Some(distance + travel as f64 / FIXED_ONE
                    - (cursor.position - data.end as f64));
            }
            distance += travel as f64 / FIXED_ONE;
            done += count;
        }
        None
    }

    fn preview_source(&mut self, cx: &Context, clock: &mut super::lfo::Clock,
        glide: &mut super::glide::Clock,
        envelopes: &mut [Envelope; PITCH_ENVS], base_step: f64, lagging: bool, at: usize,
        positions: &mut [u64]) -> Option<(u64,u64)> {
        let group = &cx.settings[self.group as usize];
        let n = positions.len();
        let mut pitch = [0.; MAX_BLOCK];
        if lagging {
            let mut gain = [1.; MAX_BLOCK];
            group.mods.render(&mut self.mod_preview, &self.mod_targets, cx.rate, &mut gain[..n], &mut pitch[..n]);
        }
        for (params, state) in group.pitch_envelopes.iter().zip(envelopes) {
            params.render_add(state, &mut pitch[..n], cx.rate);
        }
        clock.positions_with_sources(&group.pitch_lfos, &[], cx.rate, cx.tempo, base_step,
            Some(&pitch[..n]), Some((glide, &group.pitch_glides)),
            Some(cx.shared_lfos.view(self.group as usize, cx.now.saturating_add(at as u64))),
            Some(cx.lfo_timings.group(self.group as usize)),
            self.wavetable.is_some(), positions, None)
    }

    fn source_error(&self, kind: super::stream::StreamErrorKind) -> Start {
        Start::Failed(super::stream::StreamError { sample:self.sample, frame:self.pos as u64, kind })
    }

    pub(super) fn audible_gains(&self) -> [f32; 2] {
        if self.pitch.0.is_nan() { self.gains }
        else { self.gains.map(|gain| (gain * self.plan.lag_gain).max(0.)) }
    }

    pub(super) fn terminal_offset(&self) -> Option<usize> { self.plan.terminal }

    fn capture_terminal(&mut self, boundary: Option<usize>) {
        if let Some(boundary) = boundary {
            self.plan.terminal = Some(self.plan.terminal.map_or(boundary, |previous| previous.min(boundary)));
        }
    }

    /// Work out one block (at most [`MAX_BLOCK`] frames) before rendering
    /// it: modulation, pitch and gain into [`Voice::plan`], and the [`Lane`]
    /// it can mix in. `bus` is the group's, with its gains when it only
    /// passes to the output through a fader at rest.
    pub(super) fn prepare_source(&mut self, cx: &Context, n: usize) -> Result<PreparedSource, Start> {
        if let Some(source) = self.env.shared() {
            let points = cx.shared_ahdsr.and_then(|range| range.points(self.group as usize,cx.now));
            if points.is_none_or(|points| !source.can_consume(points.len(),n)) {
                return Err(self.source_error(super::stream::StreamErrorKind::UnsupportedWindow));
            }
        }
        // An unheard attack does not arrive after its physical release. Pedals
        // defer release(), while a release-triggered voice has its own onset.
        let cancel_onset = (self.released && !self.release_trigger) || self.fade.dying();
        if self.onset == Onset::Waiting && cancel_onset {
            self.capture_terminal(Some(0));
            return Err(Start::Canceled);
        }
        if self.onset != Onset::Started { self.resume(cx); }
        if cx.native_control_tick {
            self.refresh_loop(cx);
        }
        let group = &cx.settings[self.group as usize];
        if self.native_hq(cx).is_some() && (self.map.looped.is_some() || self.map.reverse
            || self.counted.is_some_and(|cursor| {
                let rows = &cx.bank.counted_loops[cursor.rows as usize];
                cursor.crossfaded(rows) || rows.terminal() != self.map.end
            })) {
            // Wide HQ stencils require the current physical loop/direction domain,
            // not historical virtual runs. Never substitute a quartet kernel.
            return Err(self.source_error(super::stream::StreamErrorKind::UnsupportedWindow));
        }
        if self.native_cubic(cx) && !self.map.reverse
            && self.map.looped.is_some_and(|row| !row.alternating && row.xfade == 0
                && (row.until_release || cx.bank.groups()[self.group as usize].source_mode == Some(3))) {
            // These Native domains require retained physical Rows. Failed or
            // missing preparation cannot fall back to virtual modulo, which
            // loses the pending overshoot at release.
            return Err(self.source_error(super::stream::StreamErrorKind::UnsupportedWindow));
        }
        if self.native_cubic(cx) && !self.map.reverse && self.map.looped.is_none()
            && self.counted.is_none()
            && (self.map.start != 0 || self.map.end != cx.bank.samples[self.sample as usize].frames) {
            return Err(self.source_error(super::stream::StreamErrorKind::UnsupportedWindow));
        }
        if self.native_cubic(cx)
            && cx.bank.groups()[self.group as usize].source_mode == Some(3)
            && (self.map.reverse
                || self.map.looped.is_some_and(|row| row.alternating || row.xfade != 0)
                || self.counted.is_some_and(|cursor| cursor.crossfaded(&cx.bank.counted_loops[cursor.rows as usize]))) {
            return Err(self.source_error(super::stream::StreamErrorKind::UnsupportedWindow));
        }
        if let Some(primary) = &mut self.primary_amp {
            primary.bypassed = group.primary_bypass.is_some_and(|p| p.1);
        }
        if self.counted.is_some() && group.mods.loop_offsets() != [0.; 2] {
            return Err(self.source_error(super::stream::StreamErrorKind::UnsupportedWindow));
        }
        let key = self.owner.map_or(self.note, |(_, key)| key);
        let x = self
            .frozen_expression
            .unwrap_or(cx.midi.expression[self.channel as usize & 15][key as usize & 127]);
        let x = cx.per_note.overlay(self.per_note, x);
        let x = cx.host_notes.overlay(self.host_note, x);
        let input = self.input_channel.unwrap_or(self.channel);
        let member = cx.midi.mpe.member(input).filter(|_| self.frozen_expression.is_none());
        let member_tune = member.map_or(0., |zone|
            cx.midi.bend[input as usize].dsp() * cx.midi.channel_bend_range[input as usize].map(|range| range.semitones())
                .unwrap_or_else(|| zone.member_range.semitones()) as f32);
        let member_gain = if member.is_some() && cx.native_mpe_pressure
            && cx.midi.pressure_active[input as usize] {
            10f32.powf((cx.midi.pressure[input as usize].dsp() - 1.) * 12. / 20.)
        } else { 1. };
        let zone = cx.midi.mpe.zone(input);
        let master = zone.and_then(|zone| {
            cx.midi.channel_bend_range[zone.manager as usize].map(|range| range.semitones())
                .or_else(|| zone.manager_range.map(|range| range.semitones()))
                .map(|range| (zone.manager, cx.midi.bend[zone.manager as usize].dsp(), range as f32))
        });
        let channel_tune = if zone.is_none() {
            cx.midi.channel_bend_range[input as usize].map_or(0., |range|
                cx.midi.bend[input as usize].dsp() * range.semitones() as f32)
        } else { 0. };
        // Member sensitivity is already projected into the note expression.
        // Authored pitch routes retain only the manager bend unless an explicit
        // manager range owns that contribution independently.
        let bend_pitch = zone.map(|zone| {
            if zone.manager_range.is_some() || cx.midi.channel_bend_range[zone.manager as usize].is_some() { 0. }
            else { cx.midi.bend[zone.manager as usize].dsp() }
        }).or_else(|| cx.midi.channel_bend_range[input as usize].map(|_| 0.));
        if self.settled != Some(cx.inputs) {
            let mut inputs = cx.inputs(input, self.note, self.onset_velocity, self.release_velocity, x);
            inputs.script_mods = &self.script_mods;
            inputs.per_note_cc = cx.per_note.controllers(self.per_note);
            group.mods.targets(&mut self.mod_targets, &inputs, bend_pitch);
        }
        group.mods.prepare(&mut self.mods, &self.mod_targets);
        let lagging = group.mods.lagging(&self.mods, &self.mod_targets);
        let (modulation, fixed_pitch) = group.mods.fixed(&self.mods);
        let (lag_gain, lag_pitch) = group.mods.lagged(&self.mods);
        self.modulated = ((modulation * lag_gain).max(0.), fixed_pitch + lag_pitch);
        if !lagging { self.settled = Some(cx.inputs); }
        let initial = self.pitch.0.is_nan();
        let base_semitones = fixed_pitch + group.tune + cx.tune + x.tune + member_tune + channel_tune
            + master.map_or(0., |(_, bend, range)| bend * range);
        let semitones = base_semitones + lag_pitch;
        let pitch = if semitones == self.pitch.0 { self.pitch }
            else { (semitones, 2f64.powf(f64::from(semitones) / 12.0)) };
        // RPN1/2 belongs to the physical input. Apply its full-resolution
        // ratio after absolute note/attribute pitch overlays, once, to both
        // the steady step and the curved preview/render source clock.
        let channel_ratio = cx.midi.tuning[input as usize].ratio();
        let step = self.step * self.tune * pitch.1 * channel_ratio;
        let base_step = if lagging {
            self.step * self.tune * 2f64.powf(f64::from(base_semitones) / 12.) * channel_ratio
        } else { self.step * self.tune * pitch.1 * channel_ratio };
        let curved_pitch = lagging || group.pitch_lfos.iter().any(|l| !l.bypassed)
            || group.pitch_glides.iter().any(|source| !source.bypassed)
            || group.pitch_envelopes.iter().any(|e| !e.bypass);
        let failure = super::stream::StreamError { sample:self.sample, frame:self.pos as u64,
            kind:super::stream::StreamErrorKind::UnrepresentablePitch };
        let overflow = || Start::Failed(failure);
        if self.pitch_glide.is_none() { return Err(overflow()); }
        if !super::lfo::Clock::sources_representable(&group.pitch_lfos, &group.volume_lfos, cx.rate, cx.tempo, Some(cx.lfo_timings.group(self.group as usize))) {
            return Err(overflow());
        }
        if !step.is_finite() || step < 0. || !self.pos.is_finite() || self.pos < 0.
            || self.pos + 4. >= (1u64 << 53) as f64 {
            return Err(overflow());
        }
        // This fixed source-clock admission bound is independent of CPU load;
        // unsupported rates fail rather than shortening the authored fade.
        if self.counted.is_some_and(|cursor| !cx.bank.counted_loops[cursor.rows as usize].linear())
            && (!cx.rate.is_finite() || cx.rate <= 0.
            || (DECLICK * cx.rate).ceil() > COUNTED_FADE_POINTS as f32) {
            return Err(overflow());
        }
        let fixed_step = if self.wavetable.is_some() { super::wavetable::clock(step) }
            else { sample_clock(step).ok_or_else(overflow)? };
        let mut positions = [0; MAX_BLOCK];
        let cold = self.onset != Onset::Started && self.stream.is_some() && self.wavetable.is_none();
        let mut preview_n = if cold { MAX_BLOCK } else { n };
        let mut preview_travel;
        let (travel, last, mut terminal) = if curved_pitch {
            let mut preview = |frames: usize, positions: &mut [u64]| {
                let mut clock = self.pitch_lfo;
                let mut glide = self.pitch_glide?;
                let mut envelopes = self.pitch_envs;
                self.mod_preview.copy_from_slice(&self.mods);
                self.preview_source(cx, &mut clock, &mut glide, &mut envelopes, base_step, lagging, 0, &mut positions[..frames])
            };
            let mut clock = preview(preview_n, &mut positions);
            // A speculative horizon cannot reject a representable current
            // fragment. Its eventual actual plan validates its own clock.
            if clock.is_none() && preview_n != n {
                preview_n = n;
                clock = preview(n, &mut positions);
            }
            let (full_travel, _) = clock.ok_or_else(overflow)?;
            preview_travel = full_travel;
            let travel = if preview_n == n { full_travel } else { positions[n] };
            let last = positions[n - 1];
            let terminal = if self.wavetable.is_none() {
                positions[..n].iter().position(|position|
                    self.pos + *position as f64 / FIXED_ONE >= self.length as f64)
                    .or_else(|| (self.pos + travel as f64 / FIXED_ONE >= self.length as f64).then_some(n))
            } else { None };
            (travel, last, terminal)
        } else {
            let travel = fixed_step.checked_mul(n as u64).ok_or_else(overflow)?;
            if fixed_step.checked_mul(preview_n as u64).is_none() { preview_n = n; }
            preview_travel = fixed_step * preview_n as u64;
            for (i, position) in positions[..preview_n].iter_mut().enumerate() { *position = fixed_step * i as u64; }
            let terminal = (self.wavetable.is_none() && self.pos + travel as f64 / FIXED_ONE >= self.length as f64).then(|| {
                if fixed_step == 0 { 0 } else {
                    (((self.length as f64 - self.pos).max(0.) * FIXED_ONE / fixed_step as f64).ceil() as usize).min(n)
                }
            });
            (travel, fixed_step * (n as u64 - 1), terminal)
        };
        let mut counted_cursor = None;
        if let Some(mut cursor) = self.counted {
            let data = &cx.bank.counted_loops[cursor.rows as usize];
            let mut previous = 0;
            terminal = None;
            for (i, &offset) in positions[..preview_n].iter().enumerate() {
                cursor.advance(data, offset.checked_sub(previous).ok_or_else(overflow)? as f64 / FIXED_ONE)
                    .ok_or_else(overflow)?;
                if cursor.done(data) {
                    if i <= n { terminal = Some(i); }
                    break;
                }
                previous = offset;
            }
            // Validate and preview the exact committed block advance too;
            // an end exactly after its last output remains an exclusive edge.
            let mut end_cursor = self.counted.unwrap();
            let mut prior = 0;
            for &offset in &positions[..n] {
                end_cursor.advance(data, offset.checked_sub(prior).ok_or_else(overflow)? as f64 / FIXED_ONE)
                    .ok_or_else(overflow)?;
                if end_cursor.crossfaded(data) && end_cursor.position.fract() != 0. {
                    return Err(self.source_error(super::stream::StreamErrorKind::UnsupportedWindow));
                }
                prior = offset;
            }
            end_cursor.travel(travel.checked_sub(prior).ok_or_else(overflow)? as f64 / FIXED_ONE)
                .ok_or_else(overflow)?;
            // Fractional crossfade support depends on a native producer quantum
            // that is not yet mapped. Fail before PCM or retained clock mutation.
            if end_cursor.crossfaded(data) && end_cursor.position.fract() != 0. {
                return Err(self.source_error(super::stream::StreamErrorKind::UnsupportedWindow));
            }
            counted_cursor = Some(end_cursor);
            let mut normalized = end_cursor;
            normalized.advance(data, 0.).ok_or_else(overflow)?;
            if terminal.is_none() && normalized.done(data) { terminal = Some(n); }
        }
        // f64 must retain consecutive integer tap coordinates, and the
        // relative32.32 clock must remain representable. Stream publication
        // packs its virtual frame coordinate into48 bits.
        let first = self.pos as i64 - 1;
        let base = ((self.pos - first as f64) * FIXED_ONE) as u64;
        let count = base.checked_add(last).ok_or_else(overflow)? >> 32;
        let end = self.pos + travel as f64 / FIXED_ONE + 4.;
        if self.wavetable.is_none() && (end >= (1u64 << 53) as f64
            || self.stream.is_some() && end >= (1u64 << 48) as f64) {
            return Err(overflow());
        }
        let horizon = positions[preview_n - 1];
        if base.checked_add(horizon).is_none()
            || self.pos + horizon as f64 / FIXED_ONE + 4. >= (1u64 << 48) as f64 {
            preview_n = n;
            preview_travel = travel;
        }
        let predicted_count = (base + positions[preview_n - 1]) >> 32;
        // Keep the prepared sparse windows across shorter host fragments.
        // Returning to a dense ring here would discard their lookahead.
        let sparse_active = self.stream
            .is_some_and(|stream| cx.slots[stream.slot as usize].has_sparse());
        let wide = self.wavetable.is_none() && (self.native_hq(cx).is_some() || self.counted.is_some()
            || count.max(predicted_count) + 4 > WINDOW as u64 || sparse_active);
        let level =
            self.base_level * cx.instrument.gain * group.gain * modulation * self.volume * x.gain * member_gain;
        let target = balance(
            level,
            ((self.base_pan + cx.instrument.pan).clamp(-1.0, 1.0) + group.pan + self.pan + x.pan)
                .clamp(-1.0, 1.0),
        );
        // A paused, silent voice keeps moving without source data. When it
        // becomes audible, renew its stream tag before sparse admission.
        let silent_paused = self.stream.is_some_and(|stream| stream.paused)
            && !lagging && target.map(|gain| (gain * lag_gain).max(0.)) == [0.; 2]
            && self.audible_gains() == [0.; 2] && group.filter.is_none();
        let sparse = if silent_paused { None } else {
            if self.counted.is_some() { self.resume(cx); }
            else if let Some(stream) = &mut self.stream {
                stream.resume(cx.slots, self.sample, &self.map, self.wraps, self.limit, self.pos as u64);
            }
            match self.admit(cx, last, wide, &positions[..preview_n], preview_travel, curved_pitch) {
                Ok(request) => request,
                Err(start) => {
                    if matches!(start, Start::Pending) {
                        self.onset = Onset::Waiting;
                        if cancel_onset {
                            self.capture_terminal(Some(0));
                            return Err(Start::Canceled);
                        }
                    }
                    return Err(start);
                }
            }
        };
        if let Some(stream) = &mut self.stream { stream.sparse = sparse; }
        Ok(PreparedSource { pitch, initial, input, x, curved_pitch, terminal, target, lagging, lag_gain, base_step, base_semitones, counted_cursor, fixed_step, travel, last, wide })
    }

    #[inline(always)]
    pub fn plan(&mut self, cx: &Context, n: usize, bus: Option<u8>,
        through: Option<[f32; 2]>) -> Result<Option<Lane>, Start> {
        let PreparedSource { pitch, initial, input, x, curved_pitch, terminal, target, lagging, lag_gain, base_step, base_semitones, counted_cursor, fixed_step, travel, last, wide } = self.prepare_source(cx, n)?;
        let group = &cx.settings[self.group as usize];
        self.onset = Onset::Started;
        self.pitch = pitch;
        if !curved_pitch && group.volume_lfos.is_empty() {
            self.pitch_lfo.skip_bypassed(n);
        }
        // A dying fade has an exact saved frame count even when the host
        // fragment outlives it. The boundary is exclusive of its final step.
        let terminal = if self.fade.dying() && self.fade.left as usize <= n {
            Some(terminal.map_or(self.fade.left as usize, |source| source.min(self.fade.left as usize)))
        } else { terminal };
        // Same-frame host expression may arrive after an unscripted note was
        // spawned. Start at the final initial value; later edits still ramp.
        if initial {
            self.gains = target;
            self.gain_start = target;
            self.gain_target = target;
            self.gain_remaining = 0;
        } else if target != self.gain_target {
            self.gain_start = self.gains;
            self.gain_target = target;
            self.gain_frames = (cx.rate * 0.005).round().max(1.) as u32;
            self.gain_remaining = self.gain_frames;
        }
        // Zero amplifier gain cannot skip inserts: post-amp filters may ring,
        // and pre-amp filters must retain their state for a quick unmute.
        let muted = !lagging && target.map(|gain| (gain * lag_gain).max(0.)) == [0.; 2]
            && self.audible_gains() == [0.; 2] && group.filter.is_none();
        // A sample ending mid-waveform ramps out over its last millisecond.
        let counted_end = self.counted_end(cx, n, base_step, lagging);
        let end = ((self.length as f64 - self.pos) / (travel as f64 / FIXED_ONE / n as f64)) as f32;
        let declick = self.wavetable.is_none() && if self.counted.is_some() { counted_end.is_some() }
            else { end < n as f32 + DECLICK * cx.rate };
        let mut plan = Plan {
            counted_cursor,
            n,
            // 32.32 fixed point: exact, cheap to index.
            step: fixed_step,
            travel,
            last,
            curved_pitch,
            wide,
            base_step,
            base_semitones,
            lagging,
            lag_gain,
            terminal,
            target,
            muted,
            declick: if self.counted.is_some() { counted_end } else { declick.then_some(f64::from(end)) },
            weights: [0.0; 2],
            class: 0,
        };
        // A filter held all block is linear and time-invariant: voices of
        // any group with it at the same settings can share it.
        let class = match &group.filter {
            None => Some(0),
            Some(_) if muted => None,
            Some(f) => {
                let mut inputs = cx.inputs(input, self.note, self.onset_velocity, self.release_velocity, x);
                inputs.script_mods = &self.script_mods;
                inputs.per_note_cc = cx.per_note.controllers(self.per_note);
                self.filter.follow(f, &group.mods, &inputs, n, cx.rate);
                self.filter
                    .hold(f, &group.mods, cx.rate)
                    .map(|hash| hash ^ bus.map_or(0, |b| u64::from(b) + 1) << 48)
            }
        };
        let mut lane = None;
        // One gain all block, before any filter: the voice's frames can be
        // summed, weighted, with others resampled alike.
        if let Some(class) = class.filter(|_| {
            self.wavetable.is_none()
                && !curved_pitch
                && !wide
                && fixed_step <= Lane::STEP
                && !lagging
                && group.volume_lfos.is_empty()
                && !muted
                && !declick
                && self.gain_remaining == 0
                && self.fade.steady()
                && !self.primary_amp.is_some_and(|primary| primary.fading())
        }) {
            let bypassed = self.primary_amp.is_some_and(|primary| primary.bypassed);
            let flex = match &self.flex {
                _ if bypassed => Some(Shape::Flat(1.)),
                Some(env) => env.shape(n),
                None => Some(Shape::Flat(1.0)),
            };
            let envelope = if bypassed { Some(Shape::Flat(1.)) } else { self.env.shape(n) };
            if let Some(shape) = envelope.zip(flex).and_then(|(a, b)| a.times(b)) {
                let (level, decay) = match shape {
                    Shape::Flat(level) => (level, 0),
                    Shape::Decay(k, level) => (level, k.to_bits()),
                };
                let a = level * self.fade.value();
                let first = self.pos as i64 - 1;
                // Bus gains fold into the weights only ahead of no filter.
                let through = through.filter(|_| class == 0);
                let g = through.unwrap_or([1.0; 2]);
                plan.weights = [a * (target[0] * lag_gain).max(0.) * g[0], a * (target[1] * lag_gain).max(0.) * g[1]];
                plan.class = class;
                let base = ((self.pos - first as f64) * FIXED_ONE) as u64;
                lane = Some(Lane::new(
                    bus.filter(|_| through.is_none()),
                    plan.step,
                    base,
                    decay,
                    self.native_cubic(cx),
                ));
            }
        }
        // Resampled its own way, a voice still shares its filter.
        if let Some(class) = class.filter(|&c| c != 0 && lane.is_none()) {
            plan.class = class;
            lane = Some(Lane::solo(bus));
        }
        self.plan = plan;
        Ok(lane)
    }

    /// Live boundary edits preserve the fractional physical cursor. Once released,
    /// the existing until-release seam and tail retain their final geometry.
    fn refresh_loop(&mut self, cx: &Context) {
        let Some(base) = self.base_loop else {
            return;
        };
        let offsets = cx.settings[self.group as usize].mods.loop_offsets();
        if offsets == self.loop_offsets {
            return;
        }
        if self.released {
            self.defer_loop(offsets);
            return;
        }
        let sample = &cx.bank.samples[self.sample as usize];
        let Some(map) = (PlayMap {
            looped: Some(base),
            ..self.map
        })
        .controlled_loop(offsets, sample.frames) else {
            self.defer_loop(offsets);
            return;
        };
        if map == self.map {
            self.loop_offsets = offsets;
            self.loop_deferred = None;
            return;
        }
        let Some(run) = self.map.run(self.pos as u64, self.wraps) else {
            return;
        };
        let physical = run.frame as f64 + self.pos.fract();
        // The native behind-end seam transition is not established. Keep this
        // edit inert rather than invent a cursor reset; a later valid edit works.
        if physical > map.looped.unwrap().end as f64 + 4. {
            self.defer_loop(offsets);
            return;
        }
        self.loop_offsets = offsets;
        self.loop_deferred = None;
        self.pos = physical;
        self.map = map;
        self.wraps = map.wraps(FOREVER);
        self.length = map.len(self.wraps);
        let first = (self.pos as u64).saturating_sub(1);
        if let Some(run) = map.run(first, self.wraps)
            && let Some(span) = sample.span_at(run.frame)
        {
            self.span = span;
        }
        let span = self.span(cx.bank);
        self.limit = map.resident_limit(first, self.wraps, span.start, span.end());
        if let Some(stream) = &mut self.stream {
            stream.sparse = None;
            let slot = &cx.slots[stream.slot as usize];
            if self.limit == FOREVER {
                slot.stop();
                (stream.tag, stream.trusted, stream.paused) = (0, 0, false);
            } else {
                let from = self.limit.max(first);
                stream.tag = slot.configure(self.sample, &map, self.wraps, from, first);
                stream.trusted = from;
                stream.paused = false;
            }
        }
    }

    fn defer_loop(&mut self, offsets: [f32; 2]) {
        if self.loop_deferred != Some(offsets) {
            self.loop_deferred_edits = self.loop_deferred_edits.saturating_add(1);
            self.loop_deferred = Some(offsets);
        }
    }

    /// Fill only a moving voice's curve, using the same integer phase under
    /// every host partition. The first changed sample keeps the prior gain.
    fn render_gains(&mut self, curve: &mut [Frame]) -> bool {
        if self.gain_remaining == 0 {
            return false;
        }
        for frame in curve {
            *frame = self.gains;
            if self.gain_remaining != 0 {
                self.gain_remaining -= 1;
                self.gains = if self.gain_remaining == 0 {
                    self.gain_target
                } else {
                    let phase =
                        (self.gain_frames - self.gain_remaining) as f32 / self.gain_frames as f32;
                    std::array::from_fn(|c| {
                        self.gain_start[c] + (self.gain_target[c] - self.gain_start[c]) * phase
                    })
                };
            }
        }
        true
    }

    /// Mix the planned block into `left`/`right`, through the group's
    /// filter unless `bare` (its lane filters). Returns `(alive, underrun)`.
    pub(super) fn render(
        &mut self,
        cx: &Context,
        scratch: &mut Scratch,
        left: &mut [f32],
        right: &mut [f32],
        bare: bool,
        sends: Option<&mut crate::fx::SendInputs<'_>>,
    ) -> Rendered {
        let Plan {
            n,
            step,
            muted,
            declick,
            ..
        } = self.plan;
        let group = &cx.settings[self.group as usize];
        let own = group.filter.as_ref().filter(|_| !bare);
        let native = self.native_cubic(cx);
        if self.plan.lagging {
            let settled = group.mods.render(&mut self.mods, &self.mod_targets, cx.rate,
                &mut scratch.mod_gain[..n], &mut scratch.mod_pitch[..n]);
            self.settled = settled.then_some(cx.inputs);
            let (fixed_gain, fixed_pitch) = group.mods.fixed(&self.mods);
            let (lag_gain, lag_pitch) = group.mods.lagged(&self.mods);
            self.plan.lag_gain = lag_gain;
            self.modulated = ((fixed_gain * lag_gain).max(0.), fixed_pitch + lag_pitch);
            let semitones = self.plan.base_semitones + lag_pitch;
            self.pitch = (semitones, 2f64.powf(f64::from(semitones) / 12.));
        }
        if self.plan.curved_pitch || !group.volume_lfos.is_empty() {
            let base_step = self.plan.base_step;
            let mut pitch = [0.; MAX_BLOCK];
            if self.plan.lagging { pitch[..n].copy_from_slice(&scratch.mod_pitch[..n]); }
            for (params, state) in group.pitch_envelopes.iter().zip(&mut self.pitch_envs) {
                params.render_add(state, &mut pitch[..n], cx.rate);
            }
            let volume = (!group.volume_lfos.is_empty()).then_some(&mut scratch.out[0][..n]);
            self.pitch_lfo.positions_with_sources(
                &group.pitch_lfos,
                &group.volume_lfos,
                cx.rate,
                cx.tempo,
                base_step,
                Some(&pitch[..n]),
                Some((self.pitch_glide.as_mut().expect("admitted Glide clock"), &group.pitch_glides)),
                Some(cx.shared_lfos.view(self.group as usize, cx.now)),
                Some(cx.lfo_timings.group(self.group as usize)),
                self.wavetable.is_some(),
                &mut scratch.positions[..n],
                volume,
            ).expect("source clock checked by the identical planning preview");
        } else {
            for (params, env) in group.pitch_envelopes.iter().zip(&mut self.pitch_envs) {
                if !params.bypass { env.skip_pitch(n, cx.rate); }
            }
        }
        let gains = self.audible_gains();
        let base_moving = self.render_gains(&mut scratch.gains[..n]);
        let moving = base_moving || self.plan.lagging;
        if moving {
            for (i, frame) in scratch.gains[..n].iter_mut().enumerate() {
                let base = if base_moving { *frame } else { self.gains };
                let lag = if self.plan.lagging { scratch.mod_gain[i] } else { self.plan.lag_gain };
                *frame = base.map(|gain| (gain * lag).max(0.));
            }
        }
        let curve = moving.then_some(&scratch.gains[..n]);
        let amp = &mut scratch.amp[..n];
        let flex = &mut scratch.flex[..n];
        let bypassed = self.primary_amp.is_some_and(|primary| primary.bypassed);
        if muted {
            // Unheard: the envelopes and fade only move on, to where
            // rendering them would have left them.
            let terminal = if bypassed { None } else { if self.env.shared().is_some() {
                self.env.skip_cached(cx.shared_ahdsr.and_then(|range| range.points(self.group as usize,cx.now))
                    .expect("plan checked the sealed amplitude cache"),n)
            } else { self.env.skip_terminal(n, None, cx.rate) } };
            self.capture_terminal(terminal);
            if !bypassed && let Some(env) = &mut self.flex {
                let terminal = env.skip_terminal(n, group.flex.as_ref(), cx.rate);
                self.capture_terminal(terminal);
            }
            if let Some(primary) = &mut self.primary_amp {
                let terminal = primary.run(n, None);
                self.capture_terminal(terminal);
            }
            self.fade.skip(n);
        } else {
            let terminal = if bypassed { amp.fill(1.); None }
                else { if self.env.shared().is_some() {
                    self.env.render_cached(cx.shared_ahdsr.and_then(|range| range.points(self.group as usize,cx.now))
                        .expect("plan checked the sealed amplitude cache"),amp)
                } else { self.env.render_terminal(amp, None, cx.rate) } };
            self.capture_terminal(terminal);
            if !bypassed && let Some(env) = &mut self.flex {
                let terminal = env.render_terminal(flex, group.flex.as_ref(), cx.rate);
                self.capture_terminal(terminal);
                amp.iter_mut().zip(flex.iter()).for_each(|(a, f)| *a *= f);
            }
            if let Some(primary) = &mut self.primary_amp {
                let terminal = primary.run(n, Some(amp));
                self.capture_terminal(terminal);
            }
            self.fade.apply(amp);
            if !group.volume_lfos.is_empty() {
                amp.iter_mut()
                    .zip(&scratch.out[0][..n])
                    .for_each(|(a, volume)| *a *= volume);
            }
            if let Some(end) = declick {
                let declick = DECLICK * cx.rate;
                for (i, a) in amp.iter_mut().enumerate() {
                    let remaining = if self.counted.is_some() {
                        let (position, next) = if self.plan.curved_pitch {
                            (scratch.positions[i], if i + 1 < n { scratch.positions[i + 1] } else { self.plan.travel })
                        } else { (i as u64 * step, (i as u64 + 1) * step) };
                        ((end - position as f64 / FIXED_ONE) / ((next - position) as f64 / FIXED_ONE)) as f32
                    }
                    else if self.plan.curved_pitch {
                        let position = scratch.positions[i];
                        let next = scratch
                            .positions
                            .get(i + 1)
                            .filter(|_| i + 1 < n)
                            .copied()
                            .unwrap_or(self.plan.travel);
                        ((self.length as f64 - self.pos - position as f64 / FIXED_ONE)
                            / ((next - position) as f64 / FIXED_ONE)) as f32
                    } else {
                        end as f32 - i as f32
                    };
                    *a *= (remaining / declick).clamp(0.0, 1.0);
                }
            }
        }

        // Zero user gain still feeds pre-amplifier inserts, but an exclusive
        // source/envelope/fade terminal stops voice-local processing exactly.
        // Otherwise a long interval leaks pre-amp sends after NOTE_END while
        // fragmented rendering removes the voice at that same terminal frame.
        // Program return effects retain their independently owned tails.
        let n = self.plan.terminal.map_or(n, |stop| stop.min(n));
        let amp = &amp[..n];
        let curve = curve.map(|curve| &curve[..n]);
        let mut underrun = false;
        if muted {
            // Exact zero gain without inserts needs no source frames. Keep
            // the voice's position and envelopes moving, and pause streaming
            // after a while. Filtered voices always take the ordinary path.
            // No insert state exists in this branch; rest is an empty reset.
            if self.muted == 0 {
                self.filter.rest();
            }
            self.muted = self.muted.saturating_add(n as u32);
            // Still in the resident head, resuming is a voice start as usual.
            if self.muted as f32 >= PAUSE_AFTER * cx.rate || (self.pos as u64) < self.limit {
                self.pause(cx);
            }
        } else if n != 0 {
            let (window, base) = self.window(
                cx,
                &mut scratch.window,
                &scratch.positions[..self.plan.n],
                &mut underrun,
            );
            let [out_l, out_r] = &mut scratch.out;
            let (l, r) = match own {
                _ if own.is_some() || moving => {
                    out_l[..n].fill(0.0);
                    out_r[..n].fill(0.0);
                    (&mut out_l[..n], &mut out_r[..n])
                }
                _ => (&mut left[..n], &mut right[..n]),
            };
            if own.is_some() {
                // Inserts need the source before the native Amplifier. Reuse
                // the control scratch for unity interpolation, then for the
                // insert envelopes after interpolation has finished.
                scratch.flex[..n].fill(1.0);
                if self.plan.curved_pitch && self.wavetable.is_none() && !self.plan.wide {
                    mix_positions(
                        native,
                        window,
                        base,
                        &scratch.positions[..n],
                        &scratch.flex[..n],
                        [1.; 2],
                        [0.; 2],
                        l,
                        r,
                    );
                } else {
                    mix_selected(
                        native,
                        window,
                        base,
                        if self.wavetable.is_some() || self.plan.wide {
                            1 << 32
                        } else {
                            step
                        },
                        &scratch.flex[..n],
                        [1.0; 2],
                        [0.0; 2],
                        l,
                        r,
                    );
                }
            } else {
                if self.plan.curved_pitch && self.wavetable.is_none() && !self.plan.wide {
                    mix_positions(
                        native,
                        window,
                        base,
                        &scratch.positions[..n],
                        amp,
                        if moving { [1.; 2] } else { gains },
                        [0.; 2],
                        l,
                        r,
                    );
                } else {
                    mix_selected(
                        native,
                        window,
                        base,
                        if self.wavetable.is_some() || self.plan.wide {
                            1 << 32
                        } else {
                            step
                        },
                        amp,
                        if moving { [1.; 2] } else { gains },
                        [0.; 2],
                        l,
                        r,
                    );
                }
            }
            if let Some(filter) = own {
                let (l, r) = (&mut out_l[..n], &mut out_r[..n]);
                self.filter.process_amplified(
                    filter,
                    &group.mods,
                    &mut scratch.flex,
                    l,
                    r,
                    cx.rate,
                    amp,
                    gains,
                    [0.; 2],
                    curve,
                    sends,
                );
                left[..n]
                    .iter_mut()
                    .zip(l.iter())
                    .for_each(|(o, x)| *o += x);
                right[..n]
                    .iter_mut()
                    .zip(r.iter())
                    .for_each(|(o, x)| *o += x);
            }
            if own.is_none()
                && let Some(curve) = curve
            {
                for i in 0..n {
                    left[i] += out_l[i] * curve[i][0];
                    right[i] += out_r[i] * curve[i][1];
                }
            }
        }
        let alive = self.advance(cx);
        if alive && !muted && self.plan.wide { self.prefetch(cx); }
        Rendered { alive, underrun, work: n != 0 }
    }

    /// Add the planned block's source frames, times its lane weights, to
    /// `acc` (the lane's window): the lane resamples the sum once. Returns
    /// the actual DSP receipt.
    pub(super) fn accumulate(
        &mut self,
        cx: &Context,
        buf: &mut [Frame],
        acc: &mut [Frame],
        filter: Option<&mut LaneFilter>,
    ) -> Rendered {
        let Plan {
            n, target, weights, ..
        } = self.plan;
        let group = &cx.settings[self.group as usize];
        // The lane renders the envelope's curve; the voice's own only moves on.
        let bypassed = self.primary_amp.is_some_and(|primary| primary.bypassed);
        let terminal = if bypassed { None } else { if self.env.shared().is_some() {
                self.env.skip_cached(cx.shared_ahdsr.and_then(|range| range.points(self.group as usize,cx.now))
                    .expect("plan checked the sealed amplitude cache"),n)
            } else { self.env.skip_terminal(n, None, cx.rate) } };
        self.capture_terminal(terminal);
        if !bypassed && let Some(env) = &mut self.flex {
            let terminal = env.skip_terminal(n, group.flex.as_ref(), cx.rate);
            self.capture_terminal(terminal);
        }
        for (params, env) in group.pitch_envelopes.iter().zip(&mut self.pitch_envs) {
            if !params.bypass { env.skip_pitch(n, cx.rate); }
        }
        let (first, _, count) = self.reach();
        self.muted = 0;
        self.resume(cx);
        // Resident frames add in straight from storage, never decoded to memory.
        // Resident frames add in straight from storage, streamed ones from
        // the ring: never copied to memory first.
        let mut underrun = false;
        let resident = self.resident(cx.bank, first, count);
        if let Some(filter) = filter {
            // The frames also move the voice's filter state on: decoded once.
            filter.begin(&self.filter);
            let mut add = |src: &[Frame], at: usize| {
                accumulate(src, weights, &mut acc[at..]);
                filter.dots(src, at);
            };
            if let Some((span, at)) = resident {
                add(
                    span.data.window(at, &mut buf[..count]).unwrap_or_default(),
                    0,
                );
            } else if let Some([a, b]) = self.streamed(cx, first, count) {
                add(a, 0);
                add(b, a.len());
            } else {
                underrun = self.gather(cx, first, &mut buf[..count]);
                add(&buf[..count], 0);
            }
            filter.end(&mut self.filter, weights);
        } else if !resident.is_some_and(|(span, at)| span.data.accumulate(at, weights, acc)) {
            match self.streamed(cx, first, count) {
                Some([a, b]) => {
                    let (head, tail) = acc.split_at_mut(a.len());
                    accumulate(a, weights, head);
                    accumulate(b, weights, tail);
                }
                None => {
                    underrun = self.gather(cx, first, &mut buf[..count]);
                    accumulate(&buf[..count], weights, acc);
                }
            }
        }
        self.gains = target;
        Rendered { alive: self.advance(cx), underrun, work: n != 0 }
    }

    /// The `count` frames from `first` in the stream ring, as its two
    /// runs, when all of them are streamed and published.
    fn streamed<'b>(
        &mut self,
        cx: &Context<'b>,
        first: i64,
        count: usize,
    ) -> Option<[&'b [Frame]; 2]> {
        let (Ok(v), Some(stream)) = (u64::try_from(first), self.stream.as_mut()) else {
            return None;
        };
        let need = v + count as u64;
        if v < self.limit || need > self.length {
            return None;
        }
        let slot = &cx.slots[stream.slot as usize];
        if let Some(end) = slot.published(stream.tag) {
            stream.trusted = stream.trusted.max(end);
        }
        slot.runs(v, count).filter(|_| stream.trusted >= need)
    }

    /// The planned block's source frames: the first (one before the
    /// position, the cubic's left tap), the position's offset from it
    /// (32.32) and how many.
    fn reach(&self) -> (i64, u64, usize) {
        let first = self.pos as i64 - 1;
        let base = ((self.pos - first as f64) * FIXED_ONE) as u64;
        let count = ((base + self.plan.last) >> 32) as usize + 4;
        (first, base, count)
    }

    fn native_hq(&self, cx: &Context) -> Option<i32> {
        let quality = cx.settings[self.group as usize].interp_quality;
        (cx.native_sampler && self.wavetable.is_none()
            && matches!(cx.bank.groups()[self.group as usize].source_mode, Some(0 | 3))
            && matches!(quality, 1 | 2)).then_some(quality)
    }

    fn hq_points(bank: &Bank, mut cursor: Option<super::counted_loop::Cursor>, pos: f64, end: u64,
        positions: &[u64], travel: u64, curved: bool, points: &mut [super::hq::Point],
    ) -> Result<Option<super::counted_loop::Cursor>, super::stream::StreamErrorKind> {
        use super::stream::StreamErrorKind;
        let mut previous = 0;
        for (i, point) in points.iter_mut().enumerate() {
            let offset = positions[i];
            let next = positions.get(i + 1).copied().unwrap_or(travel);
            let mut domain = None;
            let ended;
            let position = if let Some(ref mut cursor) = cursor {
                let data = &bank.counted_loops[cursor.rows as usize];
                cursor.advance(data, offset.checked_sub(previous).ok_or(StreamErrorKind::UnrepresentablePitch)? as f64 / FIXED_ONE)
                    .ok_or(StreamErrorKind::UnrepresentablePitch)?;
                if let Some((start,end,left,right)) = cursor.domain(data) {
                    domain = Some(super::hq::Domain {
                        start:u32::try_from(start).map_err(|_| StreamErrorKind::UnsupportedWindow)?,
                        end:u32::try_from(end).map_err(|_| StreamErrorKind::UnsupportedWindow)?,left,right });
                }
                ended = cursor.done(data);
                cursor.position
            } else { let position = pos + offset as f64 / FIXED_ONE; ended = position.floor() >= end as f64; position };
            if !position.is_finite() || !(0. ..(1u64 << 48) as f64).contains(&position) {
                return Err(StreamErrorKind::UnrepresentablePitch);
            }
            let fraction = (((position.fract() * FIXED_ONE) as u32) >> 8) as f32 * (1. / (1 << 24) as f32);
            let step = next.checked_sub(offset).ok_or(StreamErrorKind::UnrepresentablePitch)? as f64 / FIXED_ONE;
            *point = super::hq::Point { floor:position as u64, fraction, step, domain, ended,
                resample:curved || ((step as f32) - 1.).abs() > 1e-4 };
            point.validate()?;
            previous = offset;
        }
        if let Some(ref mut cursor) = cursor {
            cursor.travel(travel.checked_sub(previous).ok_or(StreamErrorKind::UnrepresentablePitch)? as f64 / FIXED_ONE)
                .ok_or(StreamErrorKind::UnrepresentablePitch)?;
        }
        Ok(cursor)
    }

    fn native_cubic(&self, cx: &Context) -> bool {
        cx.native_sampler && self.wavetable.is_none()
            && matches!(cx.bank.groups()[self.group as usize].source_mode, Some(0 | 3))
            && cx.settings[self.group as usize].interp_quality == 0
    }

    /// The source frames the planned block reads (see [`Voice::reach`]) and
    /// the position's offset into them.
    fn window<'b>(
        &mut self,
        cx: &Context<'b>,
        buf: &'b mut [Frame],
        positions: &[u64],
        underrun: &mut bool,
    ) -> (&'b [Frame], u64) {
        self.muted = 0;
        if let Some(quality) = self.native_hq(cx) {
            let out = &mut buf[..self.plan.n + 3];
            out.fill([0.; 2]);
            if let Some((stream, request)) = self.stream.and_then(|stream|
                stream.sparse.map(|request| (stream, request))) {
                if !cx.slots[stream.slot as usize].copy_kernel(stream.tag, request, &mut out[1..self.plan.n + 1]) {
                    *underrun = true;
                }
            } else {
                let mut offsets = [0; MAX_BLOCK];
                if self.plan.curved_pitch { offsets[..self.plan.n].copy_from_slice(positions); }
                else { for (i, offset) in offsets[..self.plan.n].iter_mut().enumerate() { *offset = self.plan.step * i as u64; } }
                let mut points = [super::hq::Point::default(); MAX_BLOCK];
                if let Err(kind) = Self::hq_points(cx.bank, self.counted, self.map.start as f64 + self.pos,
                    self.map.end, &offsets[..self.plan.n], self.plan.travel, self.plan.curved_pitch, &mut points[..self.plan.n]) {
                    *underrun = true;
                    if cx.stream_error.get().is_none() { cx.stream_error.set(Some(super::stream::StreamError {
                        sample:self.sample, frame:self.pos as u64, kind })); }
                    return (out, 1 << 32);
                }
                for i in 0..self.plan.n {
                    if points[i].ended { continue; }
                    match super::hq::sample(quality, points[i], |first, frames| {
                        super::hq::read(points[i], first, frames, |first, frames| self.gather_hq(cx, first as i64, frames))
                    }) {
                        Ok(frame) => out[i + 1] = frame,
                        Err(kind) => {
                            *underrun = true;
                            if cx.stream_error.get().is_none() { cx.stream_error.set(Some(super::stream::StreamError {
                                sample:self.sample, frame:points[i].floor, kind })); }
                        }
                    }
                }
            }
            return (out, 1 << 32);
        }
        let interpolate: fn(&[Frame; 4], f32) -> Frame = if self.native_cubic(cx) { lagrange } else { hermite };
        if let Some(mut table) = self.wavetable {
            let span = self.span(cx.bank);
            table.first -= span.start as usize;
            let buf = &mut buf[..self.plan.n + 3];
            buf.fill([0.; 2]);
            let source = &cx.settings[self.group as usize].wavetable.unwrap();
            if self.plan.curved_pitch {
                table.render_positions(
                    &span.data,
                    source,
                    self.pos,
                    positions,
                    &mut buf[1..self.plan.n + 1],
                );
            } else {
                table.render(
                    &span.data,
                    source,
                    self.pos,
                    self.plan.step as f64 / FIXED_ONE,
                    &mut buf[1..self.plan.n + 1],
                );
            }
            return (buf, 1 << 32);
        }
        self.resume(cx);
        if let Some(mut cursor) = self.counted {
            let data = &cx.bank.counted_loops[cursor.rows as usize];
            let out = &mut buf[..self.plan.n + 3];
            out.fill([0.; 2]);
            let stride = if data.crossfaded() { 8 } else { 4 };
            let mut packet = [[0.; 2]; MAX_BLOCK * 8];
            let sparse = self.stream.and_then(|stream| stream.sparse.map(|request| (stream, request)));
            if let Some((stream, request)) = sparse
                && !cx.slots[stream.slot as usize].copy_sparse(stream.tag, request, &mut packet[..self.plan.n * stride]) {
                *underrun = true;
            }
            let sample = &cx.bank.samples[self.sample as usize];
            let mut fade_gains = [super::counted_loop::Gain::default(), super::counted_loop::Gain::default()];
            let mut previous = 0;
            for i in 0..self.plan.n {
                let offset = if self.plan.curved_pitch { positions[i] } else { self.plan.step * i as u64 };
                cursor.advance(data, (offset - previous) as f64 / FIXED_ONE)
                    .expect("counted cursor checked by planning preview");
                previous = offset;
                let physical = cursor.crossfade_taps(data);
                let mut values = [[0.; 2]; 8];
                for (j, &tap) in physical[..stride].iter().enumerate() {
                    if tap == u64::MAX { continue; }
                    if let Some(index) = sample.span_at(tap) {
                        let span = &sample.spans[index as usize];
                        let mut unpack = [[0.; 2]; 1];
                        values[j] = span.data.window((tap - span.start) as usize, &mut unpack)
                            .expect("resident physical tap")[0];
                    } else if sparse.is_some() { values[j] = packet[i * stride + j]; }
                    else { *underrun = true; }
                }
                if let Some(fade) = cursor.crossfade(data) {
                    // Integral phases use the two raw centers. Native gain is
                    // applied before interpolation, then resampled signals add.
                    for j in [1, 5] {
                        if physical[j] == u64::MAX { continue; }
                        let gain = fade_gains[usize::from(j == 5)].point(fade, physical[j], j == 5);
                        if gain <= 0. { values[j] = [0.; 2]; }
                        else if gain < 1. { values[j] = values[j].map(|value| value * gain); }
                    }
                    out[i + 1] = std::array::from_fn(|channel| values[1][channel] + values[5][channel]);
                } else {
                    let fraction = (((cursor.position.fract() * FIXED_ONE) as u32) >> 8) as f32 / (1u32 << 24) as f32;
                    out[i + 1] = if fraction == 0. { values[1] }
                        else { interpolate(values[..4].try_into().expect("four main taps"), fraction) };
                }
            }
            return (out, 1 << 32);
        }
        if self.plan.wide {
            // Keep one complete envelope, modulation and insert pass. Only
            // source gathering is sparse: skipped frames are never copied.
            let out = &mut buf[..self.plan.n + 3];
            out.fill([0.; 2]);
            let origin = self.pos as i64 - 1;
            let base = ((self.pos - origin as f64) * FIXED_ONE) as u64;
            let direct = !self.plan.curved_pitch && (base | self.plan.step) as u32 == 0;
            let mut packet = [[0.; 2]; MAX_BLOCK * 4];
            let sparse = self.stream.and_then(|stream| stream.sparse.map(|request| (stream, request)));
            if let Some((stream, request)) = sparse
                && !cx.slots[stream.slot as usize]
                    .copy_sparse(stream.tag, request, &mut packet[..self.plan.n * 4]) {
                packet[..self.plan.n * 4].fill([0.; 2]);
                *underrun = true;
            }
            for i in 0..self.plan.n {
                let offset = if self.plan.curved_pitch { positions[i] } else { self.plan.step * i as u64 };
                let position = base + offset;
                let first = origin + (position >> 32) as i64 - 1;
                let mut taps = [[0.; 2]; 4];
                if sparse.is_some() { taps.copy_from_slice(&packet[i * 4..i * 4 + 4]); }
                else { *underrun |= self.gather(cx, first, &mut taps); }
                let fraction = ((position as u32) >> 8) as f32 * (1. / (1 << 24) as f32);
                out[i + 1] = if direct { taps[1] } else { interpolate(&taps, fraction) };
            }
            return (out, 1 << 32);
        }
        let (first, base, count) = self.reach();
        let buf = &mut buf[..count];
        if let Some((span, at)) = self.resident(cx.bank, first, count) {
            // In range, so always `Some`.
            return (span.data.window(at, buf).unwrap_or_default(), base);
        }
        *underrun = self.gather(cx, first, buf);
        (buf, base)
    }

    /// Move past the planned block; whether the voice plays on.
    /// Prepare source data ahead without advancing any musical state. Two
    /// maximum fragments let a cached window survive a shorter host fragment
    /// followed by a full one. Changed input is checked against its actual taps.
    fn prefetch(&mut self, cx: &Context) {
        let Some(stream) = self.stream.filter(|stream| !stream.paused) else { return; };
        let mut lfo = self.pitch_lfo;
        let Some(mut glide) = self.pitch_glide else { return; };
        let mut envelopes = self.pitch_envs;
        self.mod_preview.copy_from_slice(&self.mods);
        if let Some(quality) = self.native_hq(cx) {
            let mut points = [super::hq::Point::default(); MAX_BLOCK * 2];
            let mut pos = self.map.start as f64 + self.pos;
            let mut counted = self.counted;
            for block in 0..2 {
                let mut positions = [0; MAX_BLOCK];
                let Some((travel, _)) = self.preview_source(cx, &mut lfo, &mut glide, &mut envelopes,
                    self.plan.base_step, self.plan.lagging, self.plan.n + block * MAX_BLOCK, &mut positions) else { return; };
                let Ok(next) = Self::hq_points(cx.bank, counted, pos, self.map.end, &positions, travel,
                    self.plan.curved_pitch, &mut points[block * MAX_BLOCK..(block + 1) * MAX_BLOCK]) else { return; };
                counted = next;
                pos += travel as f64 / FIXED_ONE;
            }
            let _ = cx.slots[stream.slot as usize].prefetch_kernel(stream.tag, quality, &points);
            return;
        }
        let mut taps = [u64::MAX; MAX_BLOCK * 8];
        let mut pos = self.pos;
        let mut counted = self.counted;
        let stride = if counted.is_some_and(|cursor| cx.bank.counted_loops[cursor.rows as usize].crossfaded()) { 8 } else { 4 };
        // A crossfade has two quartets per output; keep the same bounded packet.
        for block in 0..taps.len() / (MAX_BLOCK * stride) {
            let mut positions = [0; MAX_BLOCK];
            let Some((travel, last)) = self.preview_source(cx, &mut lfo, &mut glide, &mut envelopes,
                self.plan.base_step, self.plan.lagging, self.plan.n + block * MAX_BLOCK, &mut positions) else { return; };
            if let Some(cursor) = counted {
                let Some(mut next) = self.counted_taps(cx.bank, cursor, &positions,
                    &mut taps[block * MAX_BLOCK * stride..(block + 1) * MAX_BLOCK * stride]) else { return; };
                if next.advance(&cx.bank.counted_loops[next.rows as usize],
                    (travel - positions[MAX_BLOCK - 1]) as f64 / FIXED_ONE).is_none() { return; }
                counted = Some(next);
                continue;
            }
            let origin = pos as i64 - 1;
            let base = ((pos - origin as f64) * FIXED_ONE) as u64;
            if base.checked_add(last).is_none() { return; }
            for (i, offset) in positions.into_iter().enumerate() {
                let first = origin + ((base + offset) >> 32) as i64 - 1;
                for tap in 0..4 {
                    let at = first + tap as i64;
                    if at >= 0 && (at as u64) < self.length {
                        taps[(block * MAX_BLOCK + i) * 4 + tap] = at as u64;
                    }
                }
            }
            pos += travel as f64 / FIXED_ONE;
            if !pos.is_finite() || pos + 4. >= (1u64 << 48) as f64 { return; }
        }
        // A speculative future window cannot fail the current valid render.
        // Its eventual actual request carries readiness and failure outcomes.
        let _ = cx.slots[stream.slot as usize].prefetch_sparse(stream.tag, &taps);
    }

    fn advance(&mut self, cx: &Context) -> bool {
        self.pos += self.plan.travel as f64 / FIXED_ONE;
        if let Some(cursor) = self.plan.counted_cursor { self.counted = Some(cursor); }
        if self.wavetable.is_some() {
            self.pos = self.pos.rem_euclid(super::wavetable::CYCLE as f64);
        }
        if let Some(stream) = self.stream.filter(|s| !s.paused && self.counted.is_none()) {
            cx.slots[stream.slot as usize].release_below((self.pos as u64).saturating_sub(1));
        }
        !self.plan.terminal.is_some_and(|boundary| boundary <= self.plan.n)
            && !self.primary_amp.is_some_and(|primary| primary.done())
            && !self.env.done()
            && !self.flex.as_ref().is_some_and(Envelope::done)
            && !self.fade.finished()
            && self.counted.is_none_or(|cursor| !cursor.done(&cx.bank.counted_loops[cursor.rows as usize]))
            && self.pos < self.length as f64
    }

    /// Stop streaming while muted, keeping the slot.
    fn pause(&mut self, cx: &Context) {
        if let Some(stream) = self
            .stream
            .as_mut()
            .filter(|s| !s.paused && self.limit != FOREVER)
        {
            cx.slots[stream.slot as usize].stop();
            stream.paused = true;
        }
    }

    /// Stream again from the position (never from behind it, which would
    /// decode everything played while muted).
    fn resume(&mut self, cx: &Context) {
        if self.counted.is_some() {
            if let Some(stream) = self.stream.as_mut().filter(|s| s.paused) {
                let map = PlayMap { start:0, end:cx.bank.samples[self.sample as usize].frames,
                    reverse:false, looped:None };
                stream.tag = cx.slots[stream.slot as usize].configure(self.sample, &map, 0, 0, 0);
                stream.trusted = 0;
                stream.paused = false;
                stream.sparse = None;
            }
            return;
        }
        if let Some(stream) = &mut self.stream {
            stream.resume(cx.slots, self.sample, &self.map, self.wraps, self.limit, self.pos as u64);
        }
    }

    /// Fast path: the window of `count` frames from `first` is one
    /// contiguous, unblended resident run: its span and offset there.
    fn resident<'b>(&self, bank: &'b Bank, first: i64, count: usize) -> Option<(&'b Span, usize)> {
        let v = u64::try_from(first).ok()?;
        if v + count as u64 > self.limit {
            return None;
        }
        let run = self.map.run(v, self.wraps)?;
        if run.reverse || run.blend.is_some() || run.len < count as u64 {
            return None;
        }
        let span = self.span(bank);
        let at = run.frame.checked_sub(span.start)? as usize;
        (at + count <= span.data.len()).then_some((span, at))
    }

    fn span<'b>(&self, bank: &'b Bank) -> &'b Span {
        &bank.samples[self.sample as usize].spans[self.span as usize]
    }

    /// Assemble the window from resident runs and the stream ring; returns
    /// true if streamed frames were missing (they play as silence).
    fn gather(&mut self, cx: &Context, first: i64, out: &mut [Frame]) -> bool {
        let lead = usize::try_from(-first).unwrap_or(0).min(out.len());
        out[..lead].fill([0.0; 2]);
        let mut i = lead;
        let mut underrun = false;
        while i < out.len() {
            let v = (first + i as i64) as u64;
            let rest = &mut out[i..];
            let n = if v < self.limit {
                let Some(run) = self.map.run(v, self.wraps) else {
                    rest.fill([0.0; 2]);
                    break;
                };
                let n = run.len.min(self.limit - v).min(rest.len() as u64) as usize;
                copy_run(self.span(cx.bank), &run, &mut rest[..n]);
                n
            } else {
                let n = self.length.saturating_sub(v).min(rest.len() as u64) as usize;
                if n == 0 {
                    rest.fill([0.0; 2]);
                    break;
                }
                underrun |= self.copy_streamed(cx, v, &mut rest[..n]);
                n
            };
            i += n;
        }
        underrun
    }

    fn gather_hq(&self, cx: &Context, first: i64, out: &mut [Frame]) -> Result<(), super::stream::StreamErrorKind> {
        out.fill([0.; 2]);
        let sample = &cx.bank.samples[self.sample as usize];
        let lead = if first < 0 { first.unsigned_abs().min(out.len() as u64) as usize } else { 0 };
        let mut done = lead;
        while done < out.len() {
            let at = (first + done as i64) as u64;
            if at >= sample.frames { break; }
            let index = sample.span_at(at).ok_or(super::stream::StreamErrorKind::Unavailable)?;
            let span = &sample.spans[index as usize];
            let n = (span.end() - at).min((out.len() - done) as u64) as usize;
            if !span.data.decode((at - span.start) as usize, &mut out[done..done + n]) {
                return Err(super::stream::StreamErrorKind::Decode);
            }
            done += n;
        }
        Ok(())
    }

    fn copy_streamed(&mut self, cx: &Context, v: u64, out: &mut [Frame]) -> bool {
        let Some(stream) = &mut self.stream else {
            out.fill([0.0; 2]);
            return true;
        };
        let slot = &cx.slots[stream.slot as usize];
        let need = v + out.len() as u64;
        let refresh = |stream: &mut Stream| {
            if let Some(end) = slot.published(stream.tag) {
                stream.trusted = stream.trusted.max(end);
            }
        };
        refresh(stream);
        if stream.trusted < need {
            match slot.ready(stream.tag, need, cx.blocking) {
                Ok(_) => refresh(stream),
                Err(kind) => {
                    if cx.stream_error.get().is_none() {
                        cx.stream_error.set(Some(super::stream::StreamError { sample:self.sample, frame:v, kind }));
                    }
                }
            }
        }
        let ready = stream.trusted.saturating_sub(v).min(out.len() as u64) as usize;
        slot.copy(v, &mut out[..ready]);
        out[ready..].fill([0.0; 2]);
        ready < out.len()
    }

    /// Release the envelope and, for release-ended loops, re-plan the path.
    /// Slots come from and return to `free`.
    pub fn release(&mut self, bank: &Bank, settings: &[super::GroupSettings], free: &mut Vec<u16>) {
        if self.released {
            return;
        }
        self.released = true;
        self.held = false;
        if let Some(primary) = &mut self.primary_amp {
            primary.bypassed = settings[self.group as usize].primary_bypass.is_some_and(|p| p.1);
            primary.release();
        }
        self.env.release(None);
        self.filter.release();
        for env in &mut self.pitch_envs {
            env.release(None);
        }
        if let Some(env) = &mut self.flex {
            env.release(settings[self.group as usize].flex.as_ref());
        }
        if self.wavetable.is_some() {
            return;
        }
        if let Some(cursor) = &mut self.counted {
            cursor.release(&bank.counted_loops[cursor.rows as usize]);
            return;
        }
        let release = if !self.map.reverse && self.map.looped.is_some_and(|looped|
            looped.until_release && !looped.alternating && looped.xfade == 0) {
            self.pos.ceil() as u64
        } else { self.pos as u64 + 3 };
        let wraps = self.map.wraps(release);
        if wraps == self.wraps {
            return;
        }
        let diverge = self.map.divergence(wraps);
        self.wraps = wraps;
        self.length = self.map.len(wraps);
        let span = self.span(bank);
        let first = (self.pos as u64).saturating_sub(1);
        self.limit = self
            .map
            .resident_limit(first, wraps, span.start, span.end());
        let slots = bank.slots();
        if self.limit == FOREVER {
            if let Some(stream) = self.stream.take() {
                slots[stream.slot as usize].stop();
                free.push(stream.slot);
            }
            return;
        }
        // A paused stream restarts on the new path when the voice is heard.
        if self.stream.is_some_and(|s| s.paused) {
            return;
        }
        let (slot, trusted) = match self.stream {
            Some(stream) => (stream.slot, stream.trusted),
            None => match free.pop() {
                Some(slot) => (slot, self.limit),
                None => return,
            },
        };
        // Published frames before the divergence stay valid; restart after them.
        let from = self.limit.max(trusted.min(diverge));
        let tag = slots[slot as usize].configure(self.sample, &self.map, wraps, from, first);
            self.stream = Some(Stream {
            slot,
            tag,
            trusted: from,
                paused: false,
                sparse: None,
        });
    }
}

/// Copy a resident run into `out`, applying reverse order and loop crossfades.
fn copy_run(span: &Span, run: &Run, out: &mut [Frame]) {
    let n = out.len();
    let at = |frame: u64| frame.checked_sub(span.start).map(|f| f as usize);
    if run.reverse {
        let decoded = at(run.frame)
            .and_then(|top| (top + 1).checked_sub(n))
            .is_some_and(|low| span.data.decode(low, out));
        if decoded {
            out.reverse();
        } else {
            out.fill([0.0; 2]);
        }
        return;
    }
    if !at(run.frame).is_some_and(|s| span.data.decode(s, out)) {
        out.fill([0.0; 2]);
        return;
    }
    let (Some(blend), Some(partner)) = (run.blend, run.blend.and_then(|b| at(b.partner))) else {
        return;
    };
    // Partners decode in stack-sized chunks: no allocation on the audio thread.
    let mut partners = [[0.0; 2]; 64];
    for (c, chunk) in out.chunks_mut(partners.len()).enumerate() {
        let done = c * partners.len();
        let partners = &mut partners[..chunk.len()];
        if !span.data.decode(partner + done, partners) {
            return;
        }
        for (i, (o, p)) in chunk.iter_mut().zip(partners.iter()).enumerate() {
            *o = blend.apply((done + i) as u64, *o, *p);
        }
    }
}

/// 4-point, 3rd-order Hermite (Catmull-Rom) interpolation between `q[1]` and `q[2]`.
#[inline(always)]
pub(crate) fn hermite(q: &[Frame; 4], t: f32) -> Frame {
    std::array::from_fn(|c| {
        let (xm1, x0, x1, x2) = (q[0][c], q[1][c], q[2][c], q[3][c]);
        let c1 = 0.5 * (x1 - xm1);
        let c2 = xm1 - 2.5 * x0 + 2.0 * x1 - 0.5 * x2;
        let c3 = 0.5 * (x2 - xm1) + 1.5 * (x0 - x1);
        ((c3 * t + c2) * t + c1) * t + x0
    })
}

/// Ordinary native Sampler quality zero: the four-node Lagrange polynomial.
/// Preserve the existing source clock; this does not reproduce native fragment phase grouping.
#[inline(always)]
fn lagrange(q: &[Frame; 4], t: f32) -> Frame {
    let a = t + 1.;
    let b = a - 1.;
    let c = b - 1.;
    let d = c - 1.;
    std::array::from_fn(|channel| {
        let [xm1, x0, x1, x2] = q.map(|frame| frame[channel]);
        (x2 * a * b * c - xm1 * b * c * d) * (1f32 / 6.)
            + (x0 * a * c - x1 * a * b) * d * 0.5
    })
}

#[allow(clippy::too_many_arguments)]
fn mix_selected(
    native: bool,
    window: &[Frame], base: u64, step: u64, amp: &[f32],
    gains: [f32; 2], delta: [f32; 2], left: &mut [f32], right: &mut [f32],
) {
    if !native || (base | step) as u32 == 0 {
        return mix(window, base, step, amp, gains, delta, left, right);
    }
    for (i, ((l, r), a)) in left.iter_mut().zip(right).zip(amp).enumerate() {
        let p = base + step * i as u64;
        let j = (p >> 32) as usize;
        let t = ((p as u32) >> 8) as f32 * (1. / (1 << 24) as f32);
        let Some(q) = window.get(j - 1..).and_then(<[Frame]>::first_chunk::<4>) else { break; };
        let [yl, yr] = lagrange(q, t);
        *l += yl * a * (gains[0] + delta[0] * i as f32);
        *r += yr * a * (gains[1] + delta[1] * i as f32);
    }
}

/// The inner loop: resample `window` from `base` by `step` (32.32 fixed
/// point), apply per-frame amplitude and ramped channel gains, and accumulate.
/// Dispatches to an AVX-512 kernel that works sixteen frames at a time, or
/// an AVX2 one that works eight (3.2x the scalar loop at 1000 voices), when
/// the CPU has them.
#[allow(clippy::too_many_arguments)]
fn mix(
    window: &[Frame],
    base: u64,
    step: u64,
    amp: &[f32],
    gains: [f32; 2],
    delta: [f32; 2],
    left: &mut [f32],
    right: &mut [f32],
) {
    // Whole steps from a whole position (a note on its root at the
    // sample's rate, or octaves up): the cubic is its centre tap, exactly.
    const FRACTION: u64 = (1 << 32) - 1;
    if step & FRACTION == 0 && base & FRACTION == 0 {
        #[cfg(target_arch = "x86_64")]
        if std::arch::is_x86_feature_detected!("avx2") {
            // SAFETY: the running CPU supports AVX2.
            return unsafe {
                taps_avx2(
                    window,
                    base >> 32,
                    step >> 32,
                    amp,
                    gains,
                    delta,
                    left,
                    right,
                )
            };
        }
        return taps(
            window,
            base >> 32,
            step >> 32,
            amp,
            gains,
            delta,
            left,
            right,
        );
    }
    #[cfg(target_arch = "x86_64")]
    if crate::audio::avx512() && std::arch::is_x86_feature_detected!("fma") {
        // SAFETY: the running CPU supports every feature `mix_avx512` is compiled for.
        return unsafe { mix_avx512(window, base, step, amp, gains, delta, left, right) };
    }
    #[cfg(target_arch = "x86_64")]
    if std::arch::is_x86_feature_detected!("avx2") && std::arch::is_x86_feature_detected!("fma") {
        // SAFETY: the running CPU supports every feature `mix_avx2` is compiled for.
        return unsafe { mix_avx2(window, base, step, amp, gains, delta, left, right, 0) };
    }
    mix_body(window, base, step, amp, gains, delta, left, right, 0);
}

/// [`mix_avx2`] sixteen frames a pass: each pair of rows (frames `k` and
/// `k + 8`) shares a register through the same in-lane transpose, and one
/// two-source permute per tap puts the sixteen frames back in order. The
/// same arithmetic, element for element; the rest goes eight at a time.
#[cfg(target_arch = "x86_64")]
#[allow(clippy::too_many_arguments)]
#[target_feature(enable = "avx2,fma,avx512f,avx512bw,avx512vl")]
fn mix_avx512(
    window: &[Frame],
    base: u64,
    step: u64,
    amp: &[f32],
    gains: [f32; 2],
    delta: [f32; 2],
    left: &mut [f32],
    right: &mut [f32],
) {
    use std::arch::x86_64::*;
    let n = left.len().min(right.len()).min(amp.len());
    let full = n / 16 * 16;
    let fits = full > 0
        && base >> 32 >= 1
        && ((base + step * (full as u64 - 1)) >> 32) as usize + 3 <= window.len();
    let done = if fits { full } else { 0 };
    let frames = window.as_ptr().cast::<f32>();
    let v = _mm512_set1_ps;
    let lanes = _mm512_setr_ps(
        0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0,
    );
    let lane_steps = _mm512_mullo_epi32(
        _mm512_set1_epi32(step as u32 as i32),
        _mm512_setr_epi32(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15),
    );
    // Quarter q of the transposed pair (a, b) holds frames 4q..4q+4 of rows
    // (a) 0..4, (b) 4..8 in its low and high halves, and 8..16 above.
    let low = _mm512_setr_epi32(0, 1, 2, 3, 16, 17, 18, 19, 8, 9, 10, 11, 24, 25, 26, 27);
    let high = _mm512_setr_epi32(4, 5, 6, 7, 20, 21, 22, 23, 12, 13, 14, 15, 28, 29, 30, 31);
    for i in (0..done).step_by(16) {
        let p = base + step * i as u64;
        // SAFETY: `fits` bounds every frame's taps j-1..=j+2 inside `window`,
        // and i+16 <= n bounds `amp`, `left` and `right`.
        unsafe {
            let row = |k: u64| {
                let j = |k: u64| ((p + step * k) >> 32) as usize;
                let lo = _mm256_loadu_ps(frames.add(2 * (j(k) - 1)));
                let hi = _mm256_loadu_ps(frames.add(2 * (j(k + 8) - 1)));
                _mm512_castpd_ps(_mm512_insertf64x4(
                    _mm512_castpd256_pd512(_mm256_castps_pd(lo)),
                    _mm256_castps_pd(hi),
                    1,
                ))
            };
            let (r0, r1, r2, r3) = (row(0), row(1), row(2), row(3));
            let (r4, r5, r6, r7) = (row(4), row(5), row(6), row(7));
            let (t0, t1) = (_mm512_unpacklo_ps(r0, r1), _mm512_unpackhi_ps(r0, r1));
            let (t2, t3) = (_mm512_unpacklo_ps(r2, r3), _mm512_unpackhi_ps(r2, r3));
            let (t4, t5) = (_mm512_unpacklo_ps(r4, r5), _mm512_unpackhi_ps(r4, r5));
            let (t6, t7) = (_mm512_unpacklo_ps(r6, r7), _mm512_unpackhi_ps(r6, r7));
            let (s0, s1) = (
                _mm512_shuffle_ps(t0, t2, 0x44),
                _mm512_shuffle_ps(t0, t2, 0xEE),
            );
            let (s2, s3) = (
                _mm512_shuffle_ps(t1, t3, 0x44),
                _mm512_shuffle_ps(t1, t3, 0xEE),
            );
            let (s4, s5) = (
                _mm512_shuffle_ps(t4, t6, 0x44),
                _mm512_shuffle_ps(t4, t6, 0xEE),
            );
            let (s6, s7) = (
                _mm512_shuffle_ps(t5, t7, 0x44),
                _mm512_shuffle_ps(t5, t7, 0xEE),
            );
            // Tap k of channel c for all sixteen frames: column 2k + c.
            let taps = [
                [
                    _mm512_permutex2var_ps(s0, low, s4),
                    _mm512_permutex2var_ps(s1, low, s5),
                ],
                [
                    _mm512_permutex2var_ps(s2, low, s6),
                    _mm512_permutex2var_ps(s3, low, s7),
                ],
                [
                    _mm512_permutex2var_ps(s0, high, s4),
                    _mm512_permutex2var_ps(s1, high, s5),
                ],
                [
                    _mm512_permutex2var_ps(s2, high, s6),
                    _mm512_permutex2var_ps(s3, high, s7),
                ],
            ];
            let frac = _mm512_add_epi32(_mm512_set1_epi32(p as u32 as i32), lane_steps);
            let t = _mm512_mul_ps(
                _mm512_cvtepi32_ps(_mm512_srli_epi32(frac, 8)),
                v(1.0 / (1 << 24) as f32),
            );
            let a = _mm512_loadu_ps(amp.as_ptr().add(i));
            let fi = _mm512_add_ps(v(i as f32), lanes);
            for (c, out) in [left.as_mut_ptr(), right.as_mut_ptr()]
                .into_iter()
                .enumerate()
            {
                let (xm1, x0, x1, x2) = (taps[0][c], taps[1][c], taps[2][c], taps[3][c]);
                let c1 = _mm512_mul_ps(v(0.5), _mm512_sub_ps(x1, xm1));
                let c2 = _mm512_sub_ps(
                    _mm512_add_ps(
                        _mm512_sub_ps(xm1, _mm512_mul_ps(v(2.5), x0)),
                        _mm512_mul_ps(v(2.0), x1),
                    ),
                    _mm512_mul_ps(v(0.5), x2),
                );
                let c3 = _mm512_add_ps(
                    _mm512_mul_ps(v(0.5), _mm512_sub_ps(x2, xm1)),
                    _mm512_mul_ps(v(1.5), _mm512_sub_ps(x0, x1)),
                );
                let y = _mm512_add_ps(_mm512_mul_ps(c3, t), c2);
                let y = _mm512_add_ps(_mm512_mul_ps(y, t), c1);
                let y = _mm512_add_ps(_mm512_mul_ps(y, t), x0);
                let gain = _mm512_add_ps(v(gains[c]), _mm512_mul_ps(v(delta[c]), fi));
                let o = out.add(i);
                let sum = _mm512_mul_ps(_mm512_mul_ps(y, a), gain);
                _mm512_storeu_ps(o, _mm512_add_ps(_mm512_loadu_ps(o), sum));
            }
        }
    }
    mix_avx2(window, base, step, amp, gains, delta, left, right, done);
}

#[cfg(target_arch = "x86_64")]
#[allow(clippy::too_many_arguments)]
#[target_feature(enable = "avx2")]
fn taps_avx2(
    window: &[Frame],
    first: u64,
    stride: u64,
    amp: &[f32],
    gains: [f32; 2],
    delta: [f32; 2],
    left: &mut [f32],
    right: &mut [f32],
) {
    taps(window, first, stride, amp, gains, delta, left, right);
}

/// [`mix`] at a whole step from a whole position: frame `i` is window
/// frame `first + i * stride`, the value the cubic takes there, and the
/// gains apply in `mix_body`'s order, so the output is the same bit for bit.
#[allow(clippy::too_many_arguments)]
#[inline(always)]
fn taps(
    window: &[Frame],
    first: u64,
    stride: u64,
    amp: &[f32],
    gains: [f32; 2],
    delta: [f32; 2],
    left: &mut [f32],
    right: &mut [f32],
) {
    let first = first as usize;
    let n = left.len().min(right.len()).min(amp.len());
    let frames = window.get(first..).unwrap_or_default();
    let (left, right, amp) = (&mut left[..n], &mut right[..n], &amp[..n]);
    if stride == 1 {
        for (i, (((l, r), a), x)) in left
            .iter_mut()
            .zip(right.iter_mut())
            .zip(amp)
            .zip(frames)
            .enumerate()
        {
            let fi = i as f32;
            *l += x[0] * a * (gains[0] + delta[0] * fi);
            *r += x[1] * a * (gains[1] + delta[1] * fi);
        }
        return;
    }
    // A rate rounded to zero holds the current source frame. Indexed reads
    // also keep this bounded by the output length instead of step_by(0).
    let frames = (0..n).map_while(|i| frames.get(i * stride as usize));
    for (i, (((l, r), a), x)) in left
        .iter_mut()
        .zip(right.iter_mut())
        .zip(amp)
        .zip(frames)
        .enumerate()
    {
        let fi = i as f32;
        *l += x[0] * a * (gains[0] + delta[0] * fi);
        *r += x[1] * a * (gains[1] + delta[1] * fi);
    }
}

/// `acc += window · weights` per channel, over `acc`: one voice's share
/// of its [`Lane`].
fn accumulate(window: &[Frame], weights: [f32; 2], acc: &mut [Frame]) {
    #[cfg(target_arch = "x86_64")]
    if std::arch::is_x86_feature_detected!("avx2") {
        // SAFETY: the running CPU supports AVX2.
        return unsafe { accumulate_avx2(window, weights, acc) };
    }
    accumulate_body(window, weights, acc);
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
fn accumulate_avx2(window: &[Frame], weights: [f32; 2], acc: &mut [Frame]) {
    accumulate_body(window, weights, acc);
}

#[inline(always)]
fn accumulate_body(window: &[Frame], weights: [f32; 2], acc: &mut [Frame]) {
    let n = acc.len().min(window.len());
    let (acc, window) = (acc[..n].as_flattened_mut(), window[..n].as_flattened());
    let w: [f32; 16] = std::array::from_fn(|k| weights[k & 1]);
    let (chunks, tail) = acc.as_chunks_mut::<16>();
    let (xs, xtail) = window.as_chunks::<16>();
    for (a, x) in chunks.iter_mut().zip(xs) {
        for k in 0..16 {
            a[k] += w[k] * x[k];
        }
    }
    for (k, (a, x)) in tail.iter_mut().zip(xtail).enumerate() {
        *a += w[k] * x;
    }
}

#[cfg(target_arch = "x86_64")]
#[allow(clippy::too_many_arguments)]
#[target_feature(enable = "avx2,fma")]
fn mix_avx2(
    window: &[Frame],
    base: u64,
    step: u64,
    amp: &[f32],
    gains: [f32; 2],
    delta: [f32; 2],
    left: &mut [f32],
    right: &mut [f32],
    from: usize,
) {
    use std::arch::x86_64::*;
    let n = left.len().min(right.len()).min(amp.len());
    // Eight frames per pass: each loads its four stereo taps with one
    // 256-bit load, an 8x8 transpose turns them into one vector per tap and
    // channel, and the Hermite runs across the eight frames at once. The
    // arithmetic matches `mix_body` operation for operation, so the output
    // is bit-identical whichever path a frame takes.
    // From frame `from` on, eight at a time.
    let full = from + n.saturating_sub(from) / 8 * 8;
    let fits = full > from
        && base >> 32 >= 1
        && ((base + step * (full as u64 - 1)) >> 32) as usize + 3 <= window.len();
    let done = if fits { full } else { from };
    let frames = window.as_ptr().cast::<f32>();
    let v = _mm256_set1_ps;
    let lanes = _mm256_setr_ps(0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0);
    let lane_steps = _mm256_mullo_epi32(
        _mm256_set1_epi32(step as u32 as i32),
        _mm256_setr_epi32(0, 1, 2, 3, 4, 5, 6, 7),
    );
    for i in (from..done).step_by(8) {
        let p = base + step * i as u64;
        // SAFETY: `fits` bounds every frame's taps j-1..=j+2 inside `window`,
        // and i+8 <= n bounds `amp`, `left` and `right`.
        unsafe {
            let row = |k: u64| {
                let j = ((p + step * k) >> 32) as usize;
                _mm256_loadu_ps(frames.add(2 * (j - 1)))
            };
            let (r0, r1, r2, r3) = (row(0), row(1), row(2), row(3));
            let (r4, r5, r6, r7) = (row(4), row(5), row(6), row(7));
            let (t0, t1) = (_mm256_unpacklo_ps(r0, r1), _mm256_unpackhi_ps(r0, r1));
            let (t2, t3) = (_mm256_unpacklo_ps(r2, r3), _mm256_unpackhi_ps(r2, r3));
            let (t4, t5) = (_mm256_unpacklo_ps(r4, r5), _mm256_unpackhi_ps(r4, r5));
            let (t6, t7) = (_mm256_unpacklo_ps(r6, r7), _mm256_unpackhi_ps(r6, r7));
            let (s0, s1) = (
                _mm256_shuffle_ps(t0, t2, 0x44),
                _mm256_shuffle_ps(t0, t2, 0xEE),
            );
            let (s2, s3) = (
                _mm256_shuffle_ps(t1, t3, 0x44),
                _mm256_shuffle_ps(t1, t3, 0xEE),
            );
            let (s4, s5) = (
                _mm256_shuffle_ps(t4, t6, 0x44),
                _mm256_shuffle_ps(t4, t6, 0xEE),
            );
            let (s6, s7) = (
                _mm256_shuffle_ps(t5, t7, 0x44),
                _mm256_shuffle_ps(t5, t7, 0xEE),
            );
            // Tap k of channel c for all eight frames: column 2k + c.
            let taps = [
                [
                    _mm256_permute2f128_ps(s0, s4, 0x20),
                    _mm256_permute2f128_ps(s1, s5, 0x20),
                ],
                [
                    _mm256_permute2f128_ps(s2, s6, 0x20),
                    _mm256_permute2f128_ps(s3, s7, 0x20),
                ],
                [
                    _mm256_permute2f128_ps(s0, s4, 0x31),
                    _mm256_permute2f128_ps(s1, s5, 0x31),
                ],
                [
                    _mm256_permute2f128_ps(s2, s6, 0x31),
                    _mm256_permute2f128_ps(s3, s7, 0x31),
                ],
            ];
            let frac = _mm256_add_epi32(_mm256_set1_epi32(p as u32 as i32), lane_steps);
            let t = _mm256_mul_ps(
                _mm256_cvtepi32_ps(_mm256_srli_epi32(frac, 8)),
                v(1.0 / (1 << 24) as f32),
            );
            let a = _mm256_loadu_ps(amp.as_ptr().add(i));
            let fi = _mm256_add_ps(v(i as f32), lanes);
            for (c, out) in [left.as_mut_ptr(), right.as_mut_ptr()]
                .into_iter()
                .enumerate()
            {
                let (xm1, x0, x1, x2) = (taps[0][c], taps[1][c], taps[2][c], taps[3][c]);
                let c1 = _mm256_mul_ps(v(0.5), _mm256_sub_ps(x1, xm1));
                let c2 = _mm256_sub_ps(
                    _mm256_add_ps(
                        _mm256_sub_ps(xm1, _mm256_mul_ps(v(2.5), x0)),
                        _mm256_mul_ps(v(2.0), x1),
                    ),
                    _mm256_mul_ps(v(0.5), x2),
                );
                let c3 = _mm256_add_ps(
                    _mm256_mul_ps(v(0.5), _mm256_sub_ps(x2, xm1)),
                    _mm256_mul_ps(v(1.5), _mm256_sub_ps(x0, x1)),
                );
                let y = _mm256_add_ps(_mm256_mul_ps(c3, t), c2);
                let y = _mm256_add_ps(_mm256_mul_ps(y, t), c1);
                let y = _mm256_add_ps(_mm256_mul_ps(y, t), x0);
                let gain = _mm256_add_ps(v(gains[c]), _mm256_mul_ps(v(delta[c]), fi));
                let o = out.add(i);
                let sum = _mm256_mul_ps(_mm256_mul_ps(y, a), gain);
                _mm256_storeu_ps(o, _mm256_add_ps(_mm256_loadu_ps(o), sum));
            }
        }
    }
    mix_body(window, base, step, amp, gains, delta, left, right, done);
}

#[allow(clippy::too_many_arguments)]
#[inline(always)]
fn mix_body(
    window: &[Frame],
    base: u64,
    step: u64,
    amp: &[f32],
    gains: [f32; 2],
    delta: [f32; 2],
    left: &mut [f32],
    right: &mut [f32],
    from: usize,
) {
    let frames = left.iter_mut().zip(right.iter_mut()).zip(amp);
    for (i, ((l, r), a)) in frames.enumerate().skip(from) {
        let p = base + step * i as u64;
        let j = (p >> 32) as usize;
        let t = ((p as u32) >> 8) as f32 * (1.0 / (1 << 24) as f32);
        let Some(q) = window.get(j - 1..).and_then(<[Frame]>::first_chunk::<4>) else {
            break;
        };
        let [yl, yr] = hermite(q, t);
        let fi = i as f32;
        *l += yl * a * (gains[0] + delta[0] * fi);
        *r += yr * a * (gains[1] + delta[1] * fi);
    }
}

/// A native pitch buffer supplies one position per audio frame. Keep the
/// existing vector/lane paths for constant pitch and this scalar path scoped
/// to voices with an active decoded LFO.
#[allow(clippy::too_many_arguments)]
fn mix_positions(
    native: bool,
    window: &[Frame],
    base: u64,
    positions: &[u64],
    amp: &[f32],
    gains: [f32; 2],
    delta: [f32; 2],
    left: &mut [f32],
    right: &mut [f32],
) {
    for (i, (((l, r), a), &offset)) in left
        .iter_mut()
        .zip(right)
        .zip(amp)
        .zip(positions)
        .enumerate()
    {
        let p = base + offset;
        let j = (p >> 32) as usize;
        let t = ((p as u32) >> 8) as f32 * (1. / (1 << 24) as f32);
        let Some(q) = window.get(j - 1..).and_then(<[Frame]>::first_chunk::<4>) else {
            break;
        };
        let [yl, yr] = if native { lagrange(q, t) } else { hermite(q, t) };
        *l += yl * a * (gains[0] + delta[0] * i as f32);
        *r += yr * a * (gains[1] + delta[1] * i as f32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_normal_lagrange_matches_independent_polynomial_pcm_across_source_and_filter_lanes() {
        use crate::{audio::Sample, engine::{Engine, instrument::ControllerPolicy},
            import::{Group, Instrument, Loop, Zone}, fx::{Chain, Effect, Kind, Params, params::Filter}};
        let basis = [[0.;2],[0.;2],[0.;2],[1.;2]];
        assert_eq!(lagrange(&basis,0.25),[-5./128.;2]);
        assert_eq!(hermite(&basis,0.25),[-3./128.;2]);
        let cubic = [[-1.;2],[0.;2],[1.;2],[8.;2]];
        for t in [0.125f32,0.25,0.5,0.75,0.875] {
            assert!((lagrange(&cubic,t)[0]-t*t*t).abs()<2e-7,"cubic reproduction at {t}");
        }
        assert_ne!(Lane::new(None,5<<30,1<<32,0,true),Lane::new(None,5<<30,1<<32,0,false));
        assert_eq!(Lane::new(Some(7),5<<30,1<<32,0,true).bus(),Some(7));
        let dir = tempfile::tempdir().unwrap();
        let source: Vec<_> = (0..524288).map(|i| {
            let x=(i%23) as f32-11.;
            [x*x*x/8192., (x*x-17.)/1024.]
        }).collect();
        // Distinct settings exercise Native Normal, unchanged Compatibility,
        // and Compatibility with saved higher-quality metadata.
        for (policy,quality,rate,looped,filtered) in [
            (ControllerPolicy::Native,0,60000,false,false),
            (ControllerPolicy::Native,0,60000,true,false),
            (ControllerPolicy::Native,0,3084000,false,false),
            (ControllerPolicy::Native,0,60000,false,true),
            (ControllerPolicy::Compatibility,0,60000,false,false),
            (ControllerPolicy::Compatibility,1,60000,false,false),
            (ControllerPolicy::Compatibility,2,60000,false,false),
        ] {
            const N:usize=6144;
            let step=rate as f64/48000.;
            let native=policy==ControllerPolicy::Native && quality==0;
            let sampling_profile = if policy == ControllerPolicy::Native {
                crate::import::SamplingProfile::Kontakt
            } else { crate::import::SamplingProfile::Compatibility };
            let path=dir.path().join(format!("source-{policy:?}-{quality}-{rate}-{looped}-{filtered}.wav"));
            let mut wav=hound::WavWriter::create(&path,hound::WavSpec { channels:2,
                sample_rate:rate,bits_per_sample:32,sample_format:hound::SampleFormat::Float }).unwrap();
            for frame in &source { for channel in frame { wav.write_sample(*channel).unwrap(); } }
            wav.finalize().unwrap();
            let mut group=Group { interp_quality:quality,..Default::default() };
            if filtered {
                group.fx=Chain { slots:vec![Effect { slot:0,kind:Kind::Filter,version:0,bypass:false,
                    output_gain:1.,dry_level:0.,params:Params::Filter(Filter { filter_type:2,
                        cutoff:0.2,resonance:0.1,extra:[0.;3],native_flag:None }) }] };
            }
            let zone=Zone { sample:path.clone(),loop_range:looped.then_some(Loop {
                start:256,end:768,crossfade:0,until_release:false,alternating:false }),..Default::default() };
            // Evaluate node basis products independently in double precision.
            // This reference does not call either production cubic or PlayMap.
            let expected:Vec<_>=(0..N+128).map(|i| {
                let travel=i as f64*step;
                let p=if looped && travel>=768. { 256.+(travel-768.)%512. } else { travel };
                let floor=p.floor() as i64; let t=p-floor as f64;
                let q:[[f64;2];4]=std::array::from_fn(|j| {
                    let mut at=floor-1+j as i64;
                    if looped {
                        if travel>=768. && at<256 { at+=512; }
                        if at>=768 { at-=512; }
                    }
                    if at<0 || at>=source.len() as i64 { [0.;2] }
                    else { source[at as usize].map(f64::from) }
                });
                std::array::from_fn(|c| {
                    if native {
                        let nodes=[-1.,0.,1.,2.];
                        (0..4).map(|j| {
                            let weight=(0..4).filter(|&k|k!=j)
                                .map(|k|(t-nodes[k])/(nodes[j]-nodes[k])).product::<f64>();
                            q[j][c]*weight
                        }).sum::<f64>() as f32
                    } else {
                        // Independent Catmull-Rom basis for preserved settings.
                        let weights=[(-t+2.*t*t-t*t*t)/2.,(2.-5.*t*t+3.*t*t*t)/2.,
                            (t+4.*t*t-3.*t*t*t)/2.,(-t*t+t*t*t)/2.];
                        q.iter().zip(weights).map(|(q,w)|q[c]*w).sum::<f64>() as f32
                    }
                })
            }).collect();
            let make=|kind| {
                let mut bank=if kind==4 {
                    Bank::from_samples(vec![group.clone()],vec![Zone { sample:path.clone(),..Default::default() }],
                        vec![(path.clone(),Sample { rate:48000,frames:expected.clone() })]).unwrap()
                } else if kind>=2 {
                    Bank::load_bare(&Instrument { groups:vec![group.clone()],zones:vec![zone.clone()],
                        sampling_profile,
                        ..Default::default() }).unwrap()
                } else {
                    Bank::from_samples_with_profile(vec![group.clone()],vec![zone.clone()],
                        vec![(path.clone(),Sample { rate,frames:source.clone() })],sampling_profile).unwrap()
                }.with_prepared_settings(|rows| rows[0].envelope=Some(Ahdsr::UNITY)).unwrap();
                bank.initial_state.as_mut().unwrap().controller_policy=policy;
                let mut engine=Engine::default(); engine.reset(48000.); engine.attack=0.;
                engine.blocking_streams=true; engine.player.shared=kind!=1 && kind!=3;
                engine.set_bank(Some(Box::new(bank)));
                // Four equal voices force the filtered lane's adjoint kernel,
                // while the other renderer takes every voice through its own filter.
                for _ in 0..4 { engine.note_on(0,60,127); }
                engine
            };
            let mut engines=[make(0),make(1),make(2),make(3),make(4)];
            let mut pcm=vec![vec![vec![0.;N];2];5];
            assert_eq!(crate::plugin::tests::allocations(|| {
                for (index,engine) in engines.iter_mut().enumerate() {
                    let mut at=0; let mut block=0;
                    while at<N {
                        let n=if index%2==0 {128} else {[1,7,31,127,13,64][block%6]}.min(N-at);
                        let (left,right)=pcm[index].split_at_mut(1);
                        engine.render_checked(&mut left[0][at..at+n],&mut right[0][at..at+n]).unwrap();
                        at+=n;block+=1;
                    }
                    assert_eq!(engine.underruns(),0,"source readiness {policy:?} quality{quality} rate{rate} loop{looped} filter{filtered} engine{index}");
                }
            }),0);
            assert!(pcm[4].iter().flatten().any(|x|x.abs()>0.001));
            for index in 0..4 {
                let error=pcm[index].iter().flatten().zip(pcm[4].iter().flatten())
                    .map(|(a,b)|(a-b).abs()).fold(0.,f32::max);
                assert!(error<6e-6,"kernel PCM {policy:?} quality{quality} rate{rate} loop{looped} filter{filtered} engine{index}: {error}");
            }
        }
    }

    #[test]
    fn native_interpolation_residency_fill_preserves_pcm_but_new_source_identity_closes_old_owners() {
        use crate::{audio::Sample, engine::{Engine,instrument::ControllerPolicy},import::{Group,Zone}};
        let source:Vec<_>=(0..32768).map(|i|[(((i*17)%71) as f32-35.)/128.;2]).collect();
        let make=|quality,mode,profile| {
            let path=std::path::PathBuf::from("native-kernel-fill");
            let mut bank=Bank::from_samples_with_profile(vec![Group { interp_quality:quality,source_mode:mode,..Default::default() }],
                vec![Zone { sample:path.clone(),..Default::default() }],
                vec![(path,Sample { rate:60000,frames:source.clone() })],profile).unwrap();
            bank.initial_state.as_mut().unwrap().controller_policy=ControllerPolicy::Native;
            Box::new(bank)
        };
        let native=crate::import::SamplingProfile::Kontakt;
        for changed in [(1,Some(0),native),(0,Some(3),native),
            (0,Some(0),crate::import::SamplingProfile::Compatibility)] {
            let mut actual=Engine::default(); actual.reset(48000.);actual.attack=0.;actual.set_bank(Some(make(0,Some(0),native)));
            let mut reference=Engine::default(); reference.reset(48000.);reference.attack=0.;reference.set_bank(Some(make(0,Some(0),native)));
            let owner=super::super::HostNote {port:0,channel:0,key:60,id:214,clap:true};
            assert!(actual.host_note_on(owner,0,60,127,0.));
            assert!(reference.host_note_on(owner,0,60,127,0.));
            let (mut a,mut b)=([[0.;256];2],[[0.;256];2]);
            let [left,right]=&mut a; actual.render_checked(&mut left[..128],&mut right[..128]).unwrap();
            let same=make(0,Some(0),native); let replacement=make(changed.0,changed.1,changed.2);
            let mut next_reference=Engine::default();next_reference.reset(48000.);next_reference.attack=0.;
            next_reference.set_bank(Some(make(changed.0,changed.1,changed.2)));
            let (mut next,mut expected)=([[0.;128];2],[[0.;128];2]);
            let (mut unused,mut retired)=(None,None);
            assert_eq!(crate::plugin::tests::allocations(|| {
                unused=actual.upgrade_bank(same);
                assert_eq!(actual.active_voices(),1,"same source/quality residency keeps its owner");
                let [left,right]=&mut a; actual.render_checked(&mut left[128..],&mut right[128..]).unwrap();
                let [left,right]=&mut b; reference.render_checked(left,right).unwrap();
                retired=actual.upgrade_bank(replacement);
                assert_eq!(actual.active_voices(),0,"changed source/quality/profile is a new instrument lifetime");
                actual.mark_host_notes();
                assert_eq!(actual.host_note_end_at(owner),Some(256),"old exact root closes before a new kernel starts");
                let new_owner=super::super::HostNote {id:215,..owner};
                assert!(actual.host_note_on(new_owner,0,60,127,0.));
                assert!(next_reference.host_note_on(new_owner,0,60,127,0.));
                let [left,right]=&mut next;actual.render_checked(left,right).unwrap();
                let [left,right]=&mut expected;next_reference.render_checked(left,right).unwrap();
            }),0);
            let error=a.iter().flatten().zip(b.iter().flatten()).map(|(x,y)|(x-y).abs()).fold(0.,f32::max);
            assert!(error<2e-6,"same-source fill must retain selected kernel and phase: {error}");
            assert_eq!(next,expected,"new source profile must start at its own fresh source phase");
            drop((unused,retired));
        }
    }

    #[test]
    fn wide_curved_source_preserves_ram_stream_filter_and_fragment_clocks_without_heap() {
        use crate::{audio::Sample, engine::Engine, import::{Group, Instrument, Zone, ModAssignment, ModSource, ModTarget},
            fx::{Chain, Effect, Kind, Params, params::Filter}, modulation::ModEnvelope};
        const FRAMES: usize = 1536;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wide-curved.wav");
        let frames: Vec<_> = (0..131072).map(|i|
            [0.25 * (i as f64 * std::f64::consts::TAU * 2. / 48000.).sin() as f32; 2]).collect();
        let mut wav = hound::WavWriter::create(&path, hound::WavSpec {
            channels:2, sample_rate:48000, bits_per_sample:32, sample_format:hound::SampleFormat::Float,
        }).unwrap();
        for frame in &frames { for channel in frame { wav.write_sample(*channel).unwrap(); } }
        wav.finalize().unwrap();
        let assignment = |source, target, intensity, lag_ms| ModAssignment { cc_default: None,
            name:"wide clock".into(), source, target, intensity, invert:false, lag_ms, shaper:None,
        };
        let group = Group {
            mods:vec![assignment(ModSource::MidiCc(1), ModTarget::Pitch, 0.25, 13)],
            pitch_lfos:vec![crate::import::PitchLfo { extended_fade: false, fade_sync: None, shared: false, digital: None, start_phase:0.23, slot:0, count:1.,
                note_value:1./24., sine:0.5, fade_ms:0., depth:0.25, targets:vec![], bypassed:false }],
            envelopes:vec![ModEnvelope { native_ahd_only:false, env:crate::import::Ahdsr { attack_curve:0.7, attack_ms:7.,
                hold_ms:3., decay_ms:9., sustain:0.25, release_ms:12., unknown_flag:0, unknown_tail:vec![] },
                targets:vec![assignment(ModSource::Unassigned, ModTarget::Pitch, 0.125, 0)] }],
            fx:Chain { slots:vec![Effect { slot:0, kind:Kind::Filter, version:0, bypass:false,
                output_gain:1., dry_level:0., params:Params::Filter(Filter { filter_type:2,
                    cutoff:0.2, resonance:0.1, extra:[0.;3], native_flag:None }) }] },
            amp_split_slot:Some(1), ..Default::default()
        };
        let zone = Zone { sample:path.clone(), ..Default::default() };
        let create = |disk| {
            let bank = if disk { Bank::load_bare(&Instrument { groups:vec![group.clone()],
                zones:vec![zone.clone()], ..Default::default() }).unwrap() }
            else { Bank::from_samples(vec![group.clone()],vec![zone.clone()],
                vec![(path.clone(),Sample { rate:48000, frames:frames.clone() })]).unwrap() };
            let mut engine = Engine { attack:0., release:0.02, blocking_streams:true, ..Engine::default() };
            engine.player.shared=false;
            engine.set_bank(Some(Box::new(bank))); engine.note_on(0,127,127); engine
        };
        let mut engines = [create(false),create(false),create(true),create(true)];
        let mut pcm = [[[0.;FRAMES];2];4];
        assert_eq!(crate::plugin::tests::allocations(|| {
            for (stage, range) in [0..385,385..911,911..1000,1000..FRAMES].into_iter().enumerate() {
                for (index, engine) in engines.iter_mut().enumerate() {
                    if stage == 1 { engine.cc(0,1,127); }
                    if stage == 2 { engine.player.midi.tuning[0].native(1,0xc0000001); }
                    if stage == 3 { engine.note_off(0,127); }
                    let sizes:&[usize] = if index % 2 == 0 { &[128] } else { &[1,7,31,128] };
                    let (mut at, mut block) = (range.start,0);
                    while at < range.end {
                        let n = sizes[block % sizes.len()].min(range.end-at);
                        let [left,right] = &mut pcm[index];
                        engine.render_checked(&mut left[at..at+n],&mut right[at..at+n]).unwrap();
                        at += n; block += 1;
                    }
                }
            }
        }),0);
        for index in 1..4 {
            let error = pcm[0].iter().flatten().zip(pcm[index].iter().flatten())
                .map(|(a,b)|(a-b).abs()).fold(0.,f32::max);
            assert!(error < 1e-6,"wide curved/filter PCM engine{index}: {error}");
            assert_eq!(engines[index].underruns(),0);
        }
        assert!(pcm[0].iter().flatten().any(|x|x.abs()>0.01));
    }

    #[test]
    fn wide_stream_mute_keeps_started_clock_and_renews_unmute_packet_without_heap() {
        use crate::{audio::Sample, engine::Engine, import::{Group, Instrument, Zone}};
        // The 20 ms coefficient reaches the -80 dB release endpoint after ~1280 frames.
        const FRAMES: usize = 8832;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wide-mute.wav");
        let frames: Vec<_> = (0..589824).map(|i|
            [0.25 * (i as f64 * std::f64::consts::TAU * 2. / 48000.).sin() as f32; 2]).collect();
        let mut wav = hound::WavWriter::create(&path, hound::WavSpec {
            channels:2, sample_rate:48000, bits_per_sample:32, sample_format:hound::SampleFormat::Float,
        }).unwrap();
        for frame in &frames { for channel in frame { wav.write_sample(*channel).unwrap(); } }
        wav.finalize().unwrap();
        let zone = Zone { sample:path.clone(), tune:64., ..Default::default() };
        let create = |disk| {
            let bank = if disk { Bank::load_bare(&Instrument { groups:vec![Group::default()],
                zones:vec![zone.clone()], ..Default::default() }).unwrap() }
            else { Bank::from_samples(vec![Group::default()],vec![zone.clone()],
                vec![(path.clone(),Sample { rate:48000, frames:frames.clone() })]).unwrap() };
            let mut engine = Engine::default(); engine.reset(48000.); engine.attack=0.; engine.release=0.02;
            engine.blocking_streams=true; engine.player.shared=false;
            engine.set_bank(Some(Box::new(bank))); engine.note_on(0,60,127); engine
        };
        let mut engines = [create(false),create(false),create(true),create(true)];
        let mut pcm = [[[0.;FRAMES];2];4];
        let mut stopped_tags = [0;4];
        assert_eq!(crate::plugin::tests::allocations(|| {
            for (stage, range) in [0..512,512..6656,6656..7424,7424..FRAMES].into_iter().enumerate() {
                for (index, engine) in engines.iter_mut().enumerate() {
                    if stage == 1 { engine.player.voices[0].volume=0.; }
                    if stage == 2 { engine.player.voices[0].volume=1.; }
                    if stage == 3 { engine.note_off(0,60); }
                    let sizes:&[usize] = if index % 2 == 0 { &[128] } else { &[1,7,31,128] };
                    let (mut at, mut block) = (range.start,0);
                    while at < range.end {
                        let n = sizes[block % sizes.len()].min(range.end-at);
                        let [left,right] = &mut pcm[index];
                        engine.render_checked(&mut left[at..at+n],&mut right[at..at+n]).unwrap();
                        if stage < 3 {
                            assert_eq!(engine.active_voices(),1,"stage{stage} engine{index}: held wide voice survives");
                            let voice = &engine.player.voices[0];
                            assert!(voice.onset == Onset::Started);
                            assert_eq!(voice.pos,((at+n)*64) as f64,"muting advances the exact authored source clock");
                            assert_eq!(voice.terminal_offset(),None,"unmute cannot publish a canceled terminal0");
                            if stage == 2 && index >= 2 {
                                let stream = voice.stream.unwrap();
                                assert!(!stream.paused);
                                assert_ne!(stream.tag,stopped_tags[index],"unmute renews the stopped generation before admission");
                            }
                        }
                        at += n; block += 1;
                    }
                    if stage == 1 && index >= 2 {
                        let stream = engine.player.voices[0].stream.unwrap();
                        assert!(stream.paused,"long exact-zero mute retains its paused stream");
                        stopped_tags[index]=stream.tag;
                    }
                }
            }
        }),0);
        for index in 0..4 {
            let error = pcm[0].iter().flatten().zip(pcm[index].iter().flatten())
                .map(|(a,b)|(a-b).abs()).fold(0.,f32::max);
            assert!(error < 1e-6,"wide mute/unmute PCM engine{index}: {error}");
            assert_eq!(engines[index].underruns(),0);
            assert_eq!(engines[index].active_voices(),0,"ordinary release completes after the resumed note");
            for channel in &pcm[index] {
                assert!(channel[6144..6656].iter().all(|value| *value == 0.));
                assert!(channel[6912..7424].iter().any(|value| value.abs() > 0.01),"resumed PCM contains the musical tone");
            }
        }
    }

    #[test]
    fn unrepresentable_live_pitch_fails_before_stale_plan_pcm_without_heap() {
        use crate::{audio::Sample, engine::{Engine, HostNote, HostPattern, StreamErrorKind}, import::{Group, Zone}};
        let mut engine = Engine { attack:0., release:0.02, ..Engine::default() };
        engine.set_bank(Some(Box::new(Bank::from_samples(vec![Group::default()],vec![Zone::default()],
            vec![(Default::default(),Sample { rate:48000,frames:vec![[0.25;2];4096] })]).unwrap())));
        let note = HostNote { port:0, channel:0, key:60, id:91, clap:true };
        assert!(engine.host_note_on(note,0,60,127,0.));
        let (mut left,mut right) = ([0.;128],[0.;128]);
        engine.render_checked(&mut left,&mut right).unwrap();
        assert!(left.iter().any(|x|*x>0.1));
        engine.state.controls.tune=10000.;
        let failed_at = engine.player.now;
        let mut error = None;
        assert_eq!(crate::plugin::tests::allocations(|| {
            engine.host_note_off(HostPattern { port:0, channel:0, key:60, id:91, clap:true });
            error=engine.render_checked(&mut left,&mut right).err();
            engine.mark_host_notes();
            assert_eq!(engine.host_note_end_at(note),Some(failed_at),"failed source plan ends at fragment offset zero");
        }),0);
        assert_eq!(error.unwrap().kind,StreamErrorKind::UnrepresentablePitch);
        assert_eq!((left,right),([0.;128],[0.;128]));
        assert_eq!(engine.active_voices(),0);
    }

    #[test]
    fn physical_channel_tuning_composes_once_with_absolute_pitch_and_curved_clocks_without_heap() {
        use crate::{audio::Sample, engine::{Engine, InputAddress}, import::{Group, Zone, ModAssignment, ModSource, ModTarget}};
        let address = InputAddress { port:0, channel:4, key:60 };
        let samples: Vec<_> = (0..32768).map(|i|
            [0.2 * (i as f32 * std::f32::consts::TAU * 713. / 48000.).sin();2]).collect();
        for curved in [false,true] {
            let create = || {
                let group = Group { tune:1., mods: if curved { vec![ModAssignment { cc_default: None,
                    name:"lagged pitch".into(), source:ModSource::MidiCc(1), target:ModTarget::Pitch,
                    intensity:0.25, invert:false, lag_ms:13, shaper:None,
                }] } else { vec![] }, pitch_lfos: if curved { vec![crate::import::PitchLfo { extended_fade: false, fade_sync: None, shared: false, digital: None,
                    start_phase:0.23, slot:0, count:1., note_value:1./24., sine:0.5,
                    fade_ms:0., depth:0.25, targets:vec![], bypassed:false,
                }] } else { vec![] }, ..Group::default() };
                let mut engine = Engine { attack:0., release:0.02, ..Engine::default() };
                engine.set_bank(Some(Box::new(Bank::from_samples(vec![group],vec![Zone::default()],
                    vec![(Default::default(),Sample { rate:48000,frames:samples.clone() })]).unwrap())));
                engine.state.controls.tune=2.;
                engine.set_mpe_zone(Some((0,1<<4)));
                assert!(engine.note_on_input_velocity_pitch(address,7,60,crate::midi::Velocity::FULL,Some(63.5)));
                engine
            };
            let (mut whole,mut split,mut oracle)=(create(),create(),create());
            // These distractor rows must never tune a root delivered to7 from4.
            for engine in [&mut whole,&mut split] {
                engine.player.midi.tuning[0].native(2,76<<25);
                engine.player.midi.tuning[7].native(2,52<<25);
            }
            let (mut actual,mut fragmented,mut expected)=([[0.;4096];2],[[0.;4096];2],[[0.;4096];2]);
            assert_eq!(crate::plugin::tests::allocations(|| {
                for (stage,(fine,coarse)) in [(0xa0000001,67u32),(0x60000017,63)].into_iter().enumerate() {
                    // An independently calculated f64 event ratio is the PCM
                    // reference; no channel-tuning implementation reads it.
                    let semitones=(f64::from(fine)-2147483648.)/2147483648.+f64::from(coarse)-64.;
                    let ratio=2f64.powf(semitones/12.);
                    for engine in [&mut whole,&mut split] {
                        engine.player.midi.tuning[4].native(1,fine);
                        engine.player.midi.tuning[4].native(2,coarse<<25);
                    }
                    oracle.player.voices[0].tune=ratio;
                    for engine in [&mut whole,&mut split,&mut oracle] {
                        engine.cc(4,1,if stage==0 {127} else {37});
                        if stage==1 {
                            assert!(engine.note_controller_from(7,60,address,crate::midi::NoteControl::Registered {
                                index:3,value:(62.25*33554432.) as u32,
                            }));
                            engine.note_off_from(7,4,60);
                        }
                    }
                    for (engine,pcm,sizes) in [(&mut whole,&mut actual,&[128][..]),
                        (&mut split,&mut fragmented,&[1,7,31,128][..]),(&mut oracle,&mut expected,&[128][..])] {
                        let (mut at,mut block)=(stage*2048,0);
                        while at < (stage+1)*2048 {
                            let n=sizes[block%sizes.len()].min((stage+1)*2048-at);
                            let [l,r]=pcm; engine.render(&mut l[at..at+n],&mut r[at..at+n]);
                            at+=n; block+=1;
                        }
                    }
                    if stage==0 {
                        let voice=&whole.player.voices[0];
                        let base=2f64.powf(f64::from(voice.plan.base_semitones)/12.)*ratio;
                        assert_eq!(voice.plan.base_step,base,"physical fine/coarse ratio must stay f64 after absolute pitch");
                    }
                }
            }),0);
            assert!(expected.iter().flatten().any(|x|x.abs()>0.1));
            for (name,pcm) in [("whole",actual),("fragmented",fragmented)] {
                let error=pcm.iter().flatten().zip(expected.iter().flatten()).map(|(a,b)|(a-b).abs()).fold(0_f32,f32::max);
                assert!(error<2e-5,"{name} physical tuning PCM error={error}, curved={curved}");
            }
        }
    }

    #[test]
    fn positive_quiet_sustain_with_gain_boost_keeps_held_pcm_and_release_without_heap() {
        use crate::{
            audio::Sample,
            engine::{Bank, Engine},
            import::{Group, Zone},
            modulation::Ahdsr as ImportedAhdsr,
        };
        let create = |sustain, gain| {
            let group = Group {
                gain,
                native_volume_env: false,
                volume_env: Some(ImportedAhdsr {
                    attack_curve: 0.,
                    attack_ms: 0.,
                    hold_ms: 0.,
                    decay_ms: 0.,
                    sustain,
                    release_ms: 50.,
                    unknown_flag: 0,
                    unknown_tail: Vec::new(),
                }),
                ..Default::default()
            };
            let bank = Bank::from_samples(
                vec![group],
                vec![Zone::default()],
                vec![(
                    Default::default(),
                    Sample {
                        rate: 48_000,
                        frames: vec![[0.25; 2]; 8192],
                    },
                )],
            )
            .unwrap();
            let mut e = Engine::default();
            e.reset(48_000.);
            e.set_bank(Some(Box::new(bank)));
            e.note_on(0, 60, 127);
            e
        };
        // Same audible sustain: .25 * .5 *1 == .25 * .00005 *10000.
        // The normal-gain envelope is an independent musical PCM reference.
        let (mut held, mut normal) = (create(5e-5, 10000.), create(0.5, 1.));
        let (mut a, mut ar, mut b, mut br) = ([0.; 128], [0.; 128], [0.; 128], [0.; 128]);
        assert_eq!(
            crate::plugin::tests::allocations(|| {
                for _ in 0..4 {
                    held.render(&mut a, &mut ar);
                    normal.render(&mut b, &mut br);
                }
            }),
            0
        );
        let held_error = a
            .iter()
            .chain(&ar)
            .zip(b.iter().chain(&br))
            .map(|(a, b)| (a - b).abs())
            .fold(0., f32::max);
        let held_voices = held.active_voices();
        assert!(
            b.iter().all(|&x| (x - 0.125).abs() < 1e-6),
            "normal oracle must be audible sustain"
        );
        // Release exactly when the zero-time decay has landed on sustain;
        // the next sustain frame has not had an opportunity to retire it.
        let (mut quiet_release, mut normal_release) = (create(5e-5, 10000.), create(0.5, 1.));
        let mut release_error = 0_f32;
        let mut release_end = 0_f32;
        assert_eq!(
            crate::plugin::tests::allocations(|| {
                for e in [&mut quiet_release, &mut normal_release] {
                    e.render(&mut [0.; 2], &mut [0.; 2]);
                    e.note_off(0, 60);
                }
                for _ in 0..8 {
                    quiet_release.render(&mut a, &mut ar);
                    normal_release.render(&mut b, &mut br);
                    release_error = release_error.max(
                        a.iter()
                            .chain(&ar)
                            .zip(b.iter().chain(&br))
                            .map(|(a, b)| (a - b).abs())
                            .fold(0., f32::max),
                    );
                    release_end = b[127];
                }
                for _ in 0..20 {
                    quiet_release.render(&mut a, &mut ar);
                    normal_release.render(&mut b, &mut br);
                }
                assert_eq!(
                    quiet_release.active_voices(),
                    0,
                    "boosted release must complete"
                );
                assert_eq!(normal_release.active_voices(), 0);
            }),
            0
        );
        eprintln!(
            "quiet sustain: held voices={held_voices}, held PCM error={held_error}, release PCM error={release_error}, normal release frame1023={release_end}"
        );
        assert!(
            release_end > 0.001,
            "50ms oracle must still be audible before halfway through release"
        );
        assert!(
            held_voices == 1 && held_error < 1e-6 && release_error < 1e-6,
            "positive sustain is audible after gain: held voices={held_voices}, held error={held_error}, release error={release_error}"
        );
    }

    #[test]
    fn muted_native_filters_keep_tails_and_quick_unmute_state_without_heap() {
        use crate::{
            audio::Sample,
            engine::{Bank, Engine, Expression},
            fx::{Chain, Effect, Kind, Params, params::Filter},
            import::{Group, Zone},
        };
        let effect = Effect {
            slot: 0,
            kind: Kind::Filter,
            version: 0,
            bypass: false,
            output_gain: 1.,
            dry_level: 0.,
            params: Params::Filter(Filter {
                filter_type: 2,
                cutoff: 0.05,
                resonance: 0.6,
                extra: [0.; 3],
                native_flag: None,
            }),
        };
        let mut errors = [[0_f32; 3]; 2];
        for (case, split) in [0, 1].into_iter().enumerate() {
            let create = |keep_processing| {
                let group = Group {
                    fx: Chain {
                        slots: vec![effect.clone()],
                    },
                    amp_split_slot: Some(split),
                    ..Default::default()
                };
                let mut bank = Bank::from_samples(
                    vec![group],
                    vec![Zone::default()],
                    vec![(
                        Default::default(),
                        Sample {
                            rate: 48_000,
                            frames: vec![[0.25; 2]; 8192],
                        },
                    )],
                )
                .unwrap();
                // This admission flag only prevents the zero-gain shortcut;
                // no send tap or DSP slot is added to the reference chain.
                bank = bank.with_prepared_settings(|settings| {
                    settings[0].filter.as_mut().unwrap().pre_sends = keep_processing;
                }).unwrap();
                bank.initial_state.as_mut().unwrap().base[0].filter.as_mut().unwrap().pre_sends = keep_processing;
                let mut e = Engine::default();
                e.reset(48_000.);
                e.attack = 0.0001;
                e.player.shared = false;
                e.set_bank(Some(Box::new(bank)));
                e.note_on(0, 60, 127);
                e
            };
            let (mut actual, mut reference) = (create(false), create(true));
            let (mut l, mut r, mut el, mut er) = ([0.; 128], [0.; 128], [0.; 128], [0.; 128]);
            assert_eq!(
                crate::plugin::tests::allocations(|| {
                    for _ in 0..16 {
                        actual.render(&mut l, &mut r);
                        reference.render(&mut el, &mut er);
                        errors[case][0] = errors[case][0].max(
                            l.iter()
                                .chain(&r)
                                .zip(el.iter().chain(&er))
                                .map(|(a, b)| (a - b).abs())
                                .fold(0., f32::max),
                        );
                    }
                    for e in [&mut actual, &mut reference] {
                        e.player.midi.set_expression(
                            0,
                            60,
                            Expression {
                                gain: 0.,
                                ..Default::default()
                            },
                        );
                    }
                    // The five-ms ramp finishes before the wholly muted fragment.
                    for _ in 0..3 {
                        actual.render(&mut l, &mut r);
                        reference.render(&mut el, &mut er);
                        errors[case][1] = errors[case][1].max(
                            l.iter()
                                .chain(&r)
                                .zip(el.iter().chain(&er))
                                .map(|(a, b)| (a - b).abs())
                                .fold(0., f32::max),
                        );
                    }
                    assert_eq!(actual.player.voices[0].pos, reference.player.voices[0].pos);
                    for e in [&mut actual, &mut reference] {
                        e.player.midi.set_expression(0, 60, Expression::default());
                    }
                    for _ in 0..3 {
                        actual.render(&mut l, &mut r);
                        reference.render(&mut el, &mut er);
                        errors[case][2] = errors[case][2].max(
                            l.iter()
                                .chain(&r)
                                .zip(el.iter().chain(&er))
                                .map(|(a, b)| (a - b).abs())
                                .fold(0., f32::max),
                        );
                    }
                }),
                0
            );
            eprintln!(
                "amplifier split {split}: baseline/mute/unmute PCM errors {:?}",
                errors[case]
            );
        }
        assert!(
            errors.iter().flatten().all(|&error| error < 1e-6),
            "zero gain must keep native filter tails and pre-amplifier state: {errors:?}"
        );
    }

    #[test]
    fn voice_gain_pan_follow_sample_time_and_retarget_without_heap() {
        use crate::{
            audio::Sample,
            engine::{Bank, Engine},
            import::{Group, Zone},
        };
        let setup = |rate: u32| {
            let bank = Bank::from_samples(
                vec![Group::default()],
                vec![Zone::default()],
                vec![(
                    std::path::PathBuf::new(),
                    Sample {
                        rate,
                        frames: vec![[0.25; 2]; rate as usize],
                    },
                )],
            )
            .unwrap();
            let mut e = Engine::default();
            e.reset(rate as f64);
            e.attack = 0.0001;
            e.set_bank(Some(Box::new(bank)));
            e.note_on(0, 60, 100);
            e.render(&mut [0.; 128], &mut [0.; 128]);
            e
        };
        for rate in [48000, 96000] {
            let mut e = setup(rate);
            let initial = e.player.voices[0].gains;
            e.player.midi.set_expression(
                0,
                60,
                super::super::Expression {
                    gain: 0.,
                    ..Default::default()
                },
            );
            let frames = rate as usize / 200;
            let quarter = frames / 4;
            let (mut l, mut r) = ([0.; 128], [0.; 128]);
            e.render(&mut [], &mut []);
            assert_eq!(e.player.voices[0].gains, initial);
            assert_eq!(
                crate::plugin::tests::allocations(|| e.render(&mut l[..quarter], &mut r[..quarter])),
                0
            );
            assert_eq!(e.player.voices[0].gains, initial.map(|g| g * 0.75));
            assert_eq!(e.player.voices[0].gain_remaining as usize, frames - quarter);
            // Retarget from the next sample's current gain, preserving continuity.
            e.player.midi.set_expression(
                0,
                60,
                super::super::Expression {
                    gain: 1.,
                    pan: 1.,
                    ..Default::default()
                },
            );
            e.render(&mut l[..quarter], &mut r[..quarter]);
            assert_eq!(
                e.player.voices[0].gains,
                [initial[0] * 0.5625, initial[1] * 0.8125]
            );
        }
        let (mut whole, mut split) = (setup(48000), setup(48000));
        let mut a = [[0.; 2]; 1024];
        let mut b = a;
        let stages = [(0, 0., 0.), (75, 0.8, 1.), (319, 0., -1.), (601, 1., -0.5)];
        for (i, &(start, gain, pan)) in stages.iter().enumerate() {
            let end = stages.get(i + 1).map_or(a.len(), |s| s.0);
            for (e, pcm, sizes) in [
                (&mut whole, &mut a, &[128][..]),
                (&mut split, &mut b, &[1, 7, 31, 127, 13, 64][..]),
            ] {
                e.player.midi.set_expression(
                    0,
                    60,
                    super::super::Expression {
                        gain,
                        pan,
                        ..Default::default()
                    },
                );
                let (mut at, mut block) = (start, 0);
                assert_eq!(
                    crate::plugin::tests::allocations(|| {
                        while at < end {
                            let n = sizes[block % sizes.len()].min(end - at);
                            let (mut l, mut r) = ([0.; 128], [0.; 128]);
                            e.render(&mut l[..n], &mut r[..n]);
                            for j in 0..n {
                                pcm[at + j] = [l[j], r[j]];
                            }
                            at += n;
                            block += 1;
                        }
                    }),
                    0
                );
            }
            assert_eq!(whole.player.voices[0].gains, split.player.voices[0].gains);
            assert_eq!(
                whole.player.voices[0].gain_remaining,
                split.player.voices[0].gain_remaining
            );
        }
        let max = a
            .iter()
            .flatten()
            .zip(b.iter().flatten())
            .map(|(a, b)| (a - b).abs())
            .fold(0., f32::max);
        assert!(max < 1e-6, "gain/pan automation partition error {max}");
        assert_eq!(whole.player.voices[0].gain_remaining, 0);
    }

    #[test]
    fn zero_stride_taps_hold_source_and_keep_envelope_and_gain_ramps() {
        let window = [[10., 20.], [0.25, -0.5], [30., 40.]];
        let amp = [0.2, 0.4, 0.6, 0.8];
        let (mut left, mut right) = ([0.1; 4], [-0.1; 4]);
        taps(
            &window,
            1,
            0,
            &amp,
            [0.7, 0.3],
            [0.1, -0.05],
            &mut left,
            &mut right,
        );
        for (actual, expected) in left.into_iter().zip([0.135, 0.18, 0.235, 0.3]) {
            assert!((actual - expected).abs() < 1e-7);
        }
        for (actual, expected) in right.into_iter().zip([-0.13, -0.15, -0.16, -0.16]) {
            assert!((actual - expected).abs() < 1e-7);
        }
    }

    /// The AVX2 and centre-tap kernels match the scalar one bit for bit, at
    /// any step, length and window slack (short windows fall back to scalar).
    #[test]
    fn simd_mix_matches_scalar_exactly() {
        let window: Vec<Frame> = (0..700)
            .map(|i| [(i as f32 * 0.37).sin(), (i as f32 * 0.11).cos()])
            .collect();
        let amp: Vec<f32> = (0..MAX_BLOCK).map(|i| 1.0 - i as f32 / 300.0).collect();
        // Whole steps from a whole position take the centre-tap path.
        for (step, n, len, base) in [
            (1.0, 128, 700, 1.3),
            (0.2718, 128, 700, 1.3),
            (1.3717, 77, 700, 1.3),
            (4.0, 128, 700, 1.3),
            (1.0, 128, 130, 1.3),
            (1.0, 7, 12, 1.3),
            (1.0, 128, 700, 1.0),
            (0.0, 128, 700, 1.0),
            (0.0, 128, 4, 1.0),
            (2.0, 128, 700, 1.0),
            (3.0, 77, 700, 1.0),
            (1.0, 128, 132, 1.0),
            // Sixteen at a time, then eight, then single frames.
            (0.8123, 31, 700, 1.7),
            (1.9, 16, 40, 1.2),
            (0.5, 128, 67, 1.9),
        ] {
            let step = (step * FIXED_ONE) as u64;
            let base = (base * FIXED_ONE) as u64;
            type Kernel =
                fn(&[Frame], u64, u64, &[f32], [f32; 2], [f32; 2], &mut [f32], &mut [f32]);
            // Every kernel this CPU runs, not only the one dispatch picks.
            let mut kernels: Vec<Kernel> = vec![mix];
            #[cfg(target_arch = "x86_64")]
            {
                use std::arch::is_x86_feature_detected as has;
                if has!("avx2") && has!("fma") {
                    // SAFETY: the running CPU supports AVX2 and FMA.
                    kernels.push(|w, b, s, a, g, d, l, r| unsafe {
                        mix_avx2(w, b, s, a, g, d, l, r, 0)
                    });
                }
                if crate::audio::avx512() && has!("fma") {
                    // SAFETY: the running CPU supports AVX-512 F, BW and VL, AVX2 and FMA.
                    kernels.push(|w, b, s, a, g, d, l, r| unsafe {
                        mix_avx512(w, b, s, a, g, d, l, r)
                    });
                }
            }
            let run = |kernel: Option<Kernel>| {
                let (mut l, mut r) = (vec![0.25; n], vec![-0.5; n]);
                let args = (
                    &window[..len],
                    base,
                    step,
                    &amp[..n],
                    [0.7, 0.3],
                    [0.001, -0.002],
                );
                match kernel {
                    Some(k) => k(
                        args.0, args.1, args.2, args.3, args.4, args.5, &mut l, &mut r,
                    ),
                    None => mix_body(
                        args.0, args.1, args.2, args.3, args.4, args.5, &mut l, &mut r, 0,
                    ),
                }
                (l, r)
            };
            for kernel in kernels {
                assert_eq!(run(Some(kernel)), run(None), "step {step:#x}, {n} frames");
            }
        }
    }

    /// Nanoseconds per 128-frame block of each mix kernel, best of many:
    /// `cargo test --release --lib mix_speed -- --ignored --nocapture`
    #[test]
    #[ignore]
    #[cfg(target_arch = "x86_64")]
    fn mix_speed() {
        use std::arch::is_x86_feature_detected as has;
        let window: Vec<Frame> = (0..WINDOW)
            .map(|i| [(i as f32 * 0.37).sin(), (i as f32 * 0.11).cos()])
            .collect();
        let amp = [0.9f32; MAX_BLOCK];
        let (mut l, mut r) = ([0f32; MAX_BLOCK], [0f32; MAX_BLOCK]);
        let base = (1.3 * FIXED_ONE) as u64;
        type Kernel = fn(&[Frame], u64, u64, &[f32], [f32; 2], [f32; 2], &mut [f32], &mut [f32]);
        let mut kernels: Vec<(&str, Kernel)> = vec![("scalar", |w, b, s, a, g, d, l, r| {
            mix_body(w, b, s, a, g, d, l, r, 0)
        })];
        if has!("avx2") && has!("fma") {
            // SAFETY: the running CPU supports AVX2 and FMA.
            kernels.push(("avx2", |w, b, s, a, g, d, l, r| unsafe {
                mix_avx2(w, b, s, a, g, d, l, r, 0)
            }));
        }
        if crate::audio::avx512() && has!("fma") {
            // SAFETY: the running CPU supports AVX-512 F, BW and VL, AVX2 and FMA.
            kernels.push(("avx512", |w, b, s, a, g, d, l, r| unsafe {
                mix_avx512(w, b, s, a, g, d, l, r)
            }));
        }
        for step in [0.53, 1.06, 1.87] {
            let step = (step * FIXED_ONE) as u64;
            for (name, kernel) in &kernels {
                let mut best = f64::MAX;
                for _ in 0..200 {
                    let t = std::time::Instant::now();
                    for _ in 0..100 {
                        kernel(
                            &window,
                            base,
                            step,
                            &amp,
                            [0.7, 0.3],
                            [0.0; 2],
                            &mut l,
                            &mut r,
                        );
                    }
                    best = best.min(t.elapsed().as_secs_f64() / 100.0);
                }
                println!(
                    "step {:.2} {name}: {:.0} ns per block",
                    step as f64 / FIXED_ONE,
                    best * 1e9
                );
            }
        }
        assert!(l[0].is_finite());
    }

    #[test]
    fn flex_quiet_and_zero_sustains_keep_boosted_pcm_release_and_muted_clock_without_heap() {
        use crate::{
            audio::Sample,
            engine::{Bank, Engine, Expression},
            import::{FlexEnvelope, FlexPoint as ImportedPoint, Group, Zone},
        };
        const HELD: usize = 1536;
        const END: usize = HELD + 240 + 480;
        const FRAMES: usize = END + 128;
        // NI's Modulation manual, Flexible Envelopes: a non-looping sustain
        // freezes at its point's level while held; release enters the following
        // section. Zero is a valid held level, not a terminal point.
        // https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/modulation
        for zero_sustain in [false, true] {
            for mute in [false, true] {
                let create = |boosted: bool, shared: bool| {
                    let level = if boosted { 5e-5 } else { 0.5 };
                    let point = |time_ms, level| ImportedPoint { time_ms, level, curve: 0.5 };
                    let group = Group {
                        gain: if boosted { 10000. } else { 1. },
                        flex_env: Some(FlexEnvelope {
                            points: vec![
                                point(2., if zero_sustain { 0. } else { level }),
                                point(5., if zero_sustain { level } else { level * 2. }),
                                point(10., 0.),
                            ],
                            sustain: 0,
                            unknown_index: 0,
                            unknown_tail: Vec::new(),
                        }),
                        ..Default::default()
                    };
                    let bank = Bank::from_samples(vec![group], vec![Zone::default()], vec![(
                        Default::default(), Sample { rate: 48_000, frames: vec![[0.25; 2]; 8192] },
                    )]).unwrap();
                    let mut engine = Engine::default();
                    engine.reset(48_000.);
                    engine.player.shared = shared;
                    engine.set_bank(Some(Box::new(bank)));
                    engine
                };
                // Gain-equivalent ordinary-scale points are an independent
                // audible oracle. Fixed/shared and fragmented/unshared routes
                // must preserve the complete attack, mute and release recording.
                let mut engines = [create(true, true), create(true, false),
                    create(false, true), create(false, false)];
                let mut pcm = [[[0.; FRAMES]; 2]; 4];
                assert_eq!(crate::plugin::tests::allocations(|| {
                    for engine in &mut engines { engine.note_on(0, 60, 127); }
                    for (stage, range) in [0..256, 256..896, 896..HELD, HELD..END, END..FRAMES]
                        .into_iter().enumerate() {
                        for (index, engine) in engines.iter_mut().enumerate() {
                            if mute && stage == 1 {
                                engine.player.midi.set_expression(0, 60, Expression {
                                    gain: 0., ..Default::default()
                                });
                            }
                            if mute && stage == 2 {
                                engine.player.midi.set_expression(0, 60, Expression::default());
                            }
                            if stage == 3 { engine.note_off(0, 60); }
                            let sizes: &[usize] = if index % 2 == 0 { &[128] } else { &[1, 7, 31, 127, 13, 64] };
                            let (mut at, mut fragment) = (range.start, 0);
                            while at < range.end {
                                let n = sizes[fragment % sizes.len()].min(range.end - at);
                                let [left, right] = &mut pcm[index];
                                engine.render_checked(&mut left[at..at + n], &mut right[at..at + n]).unwrap();
                                at += n;
                                fragment += 1;
                            }
                            if stage < 3 {
                                assert_eq!(engine.active_voices(), 1,
                                    "held Flex owner disappeared: zero={zero_sustain}, mute={mute}, engine={index}, stage={stage}");
                            } else {
                                assert_eq!(engine.active_voices(), 0,
                                    "finite release must retire at its authored endpoint");
                            }
                        }
                    }
                }), 0);
                for index in [0, 1, 3] {
                    let error = pcm[index].iter().flatten().zip(pcm[2].iter().flatten())
                        .map(|(actual, expected)| (actual - expected).abs()).fold(0., f32::max);
                    assert!(error < 2e-6, "Flex PCM error={error}, zero={zero_sustain}, mute={mute}, engine={index}");
                }
                let held_level = if zero_sustain { 0. } else { 0.125 };
                for output in &pcm {
                    assert!(output.iter().all(|channel| channel[1200..HELD].iter()
                        .all(|sample| (sample - held_level).abs() < 1e-6)));
                    assert!((output[0][HELD + 239] - if zero_sustain { 0.125 } else { 0.25 }).abs() < 1e-6,
                        "post-sustain release segment must rise to its authored level");
                    assert!(output.iter().all(|channel| channel[END..].iter().all(|sample| *sample == 0.)));
                    if mute { assert!(output.iter().all(|channel| channel[512..896].iter().all(|sample| *sample == 0.))); }
                }
            }
        }
    }

    #[test]
    fn nearly_linear_long_envelopes_keep_boosted_pcm_trajectory_without_heap() {
        use crate::{audio::Sample, engine::{Bank, Engine, Expression, HostNote, HostPattern},
            import::{FlexEnvelope, FlexPoint as ImportedPoint, Group, Loop, Zone},
            modulation::Ahdsr as ImportedAhdsr};
        const ATTACK: usize = 48_000;
        const OFF: usize = ATTACK + 256;
        const END: usize = OFF + 1536;
        const MUTE: usize = 6001;
        const UNMUTE: usize = 14_897;
        let note = |channel| HostNote { port: 0, channel, key: 60, id: i32::from(channel) + 91, clap: true };
        let pattern = |channel| HostPattern { port: 0, channel: i32::from(channel), key: 60,
            id: i32::from(channel) + 91, clap: true };
        for flex in [false, true] {
            for curve in [-0.00021_f32, 0.00021] {
                let create = |shared| {
                    let group = Group {
                        gain: 10000.,
                        volume_env: (!flex).then_some(ImportedAhdsr {
                            attack_curve: curve, attack_ms: 1000., hold_ms: 0.,
                            decay_ms: 0., sustain: 1., release_ms: 20.,
                            unknown_flag: 0, unknown_tail: Vec::new(),
                        }),
                        flex_env: flex.then_some(FlexEnvelope {
                            points: vec![
                                ImportedPoint { time_ms: 1000., level: 5e-5, curve: (curve + 1.) * 0.5 },
                                ImportedPoint { time_ms: 20., level: 0., curve: 0.5 },
                            ],
                            sustain: 0, unknown_index: 0, unknown_tail: Vec::new(),
                        }),
                        ..Default::default()
                    };
                    let zone = Zone { loop_range: Some(Loop { start: 0, end: 512,
                        crossfade: 0, alternating: false, until_release: false }), ..Default::default() };
                    let bank = Bank::from_samples(vec![group], vec![zone], vec![(Default::default(),
                        Sample { rate: 48_000, frames: vec![[if flex { 0.25 } else { 2.5e-5 }; 2]; 512] })]).unwrap();
                    let mut engine = Engine::default(); engine.reset(48_000.);
                    engine.player.shared = shared; engine.set_bank(Some(Box::new(bank))); engine
                };
                let mut engines = [create(true), create(true), create(false), create(false),
                    create(true), create(false)];
                let mut pcm: [[Vec<f32>; 2]; 6] = std::array::from_fn(|_| std::array::from_fn(|_| vec![0.; END]));
                let mut ends = [[None; 2]; 6];
                assert_eq!(crate::plugin::tests::allocations(|| {
                    for (index, engine) in engines.iter_mut().enumerate() {
                        for channel in [0, 1] { assert!(engine.host_note_on(note(channel), channel, 60, 127, 0.)); }
                        if index >= 4 {
                            // The old fixed128 Engine path, including normalized
                            // shared release lanes, supplies a complete PCM
                            // compatibility reference independent of the new clock.
                            for voice in &mut engine.player.voices {
                                if let Amplitude::Ordinary(env) = &mut voice.env { env.use_scalar_clock_for_test(); }
                                if let Some(env) = &mut voice.flex { env.use_scalar_clock_for_test(); }
                            }
                        }
                    }
                    for (stage, range) in [0..MUTE, MUTE..UNMUTE, UNMUTE..OFF, OFF..END].into_iter().enumerate() {
                        for (index, engine) in engines.iter_mut().enumerate() {
                            for channel in [0, 1] {
                                match stage {
                                    1 => engine.player.midi.set_expression(channel, 60, Expression { gain: 0., ..Default::default() }),
                                    2 => engine.player.midi.set_expression(channel, 60, Expression::default()),
                                    3 => engine.host_note_off(pattern(channel)),
                                    _ => {},
                                }
                            }
                            engine.render_checked(&mut [], &mut []).unwrap();
                            let sizes: &[usize] = match index { 1 => &[1, 7, 31, 128], 3 => &[1], _ => &[128] };
                            let (mut at, mut chunk) = (range.start, 0);
                            while at < range.end {
                                let n = sizes[chunk % sizes.len()].min(range.end - at);
                                let [l, r] = &mut pcm[index];
                                engine.render_checked(&mut l[at..at + n], &mut r[at..at + n]).unwrap();
                                at += n; chunk += 1;
                            }
                            assert_eq!(engine.active_voices(), if stage < 3 { 2 } else { 0 });
                            if stage == 3 {
                                engine.mark_host_notes();
                                for channel in [0, 1] { ends[index][channel as usize] = engine.host_note_end_at(note(channel)); }
                            }
                        }
                    }
                }), 0);
                // Independent closed-form authored core curve, not glide's
                // prepared recurrence. Native AHDSR has a separate proven law.
                let saved_curve = if flex { 2. * ((curve + 1.) * 0.5) - 1. } else { curve };
                let k = 5. * f64::from(saved_curve);
                let scale = if flex { 0.25 } else { 0.5 };
                let release = f64::from(0.001_f32.powf(1. / 960.));
                // Gamma bounds the old fixed128 path and retained group's f32
                // rounding; |k| covers the nearly-linear rounded-multiplier limit.
                // Stage boundaries previously restarted partial groups at host
                // fragments. Compatibility is bounded, not universally bit-identical.
                let u = f64::from(f32::EPSILON) * 0.5;
                let bound = scale * (ATTACK as f64 * u / (1. - ATTACK as f64 * u) + k.abs());
                let (mut maximum, mut partition, mut previous) = (0_f64, 0_f32, 0_f64);
                for at in 0..END {
                    let level = if at < ATTACK {
                        (-k * (at + 1) as f64 / ATTACK as f64).exp_m1() / (-k).exp_m1()
                    } else if at < OFF { 1. }
                    else if flex { (1. - (at - OFF + 1) as f64 / 960.).max(0.) }
                    else { let value = release.powi((at - OFF + 1) as i32); if value < 1e-4 { 0. } else { value } };
                    let gain = if at < MUTE { 1. }
                        else if at < MUTE + 240 { 1. - (at - MUTE + 1) as f64 / 240. }
                        else if at < UNMUTE { 0. }
                        else { ((at - UNMUTE + 1) as f64 / 240.).min(1.) };
                    let expected = scale * level * gain;
                    for channels in &pcm {
                        for channel in channels {
                            maximum = maximum.max((f64::from(channel[at]) - expected).abs());
                            assert!(channel[at].is_finite());
                        }
                    }
                    for (channel, reference) in pcm[0].iter().enumerate() {
                        for index in 1..4 {
                            partition = partition.max((reference[at] - pcm[index][channel][at]).abs());
                        }
                        for index in 4..6 {
                            previous = previous.max(f64::from((reference[at] - pcm[index][channel][at]).abs()));
                        }
                    }
                }
                assert!(maximum <= bound, "flex={flex}, curve={curve}: PCM error {maximum}, f32 bound {bound}");
                assert!(partition < 1e-6, "flex={flex}, curve={curve}: complete partition/shared PCM error {partition}");
                assert!(previous <= 2. * bound, "flex={flex}, curve={curve}: old fixed128 PCM error {previous}");
                assert!(ends[0].iter().all(Option::is_some));
                for end in &ends[1..4] { assert_eq!(end, &ends[0], "partition changed the exclusive terminal sample"); }
                eprintln!("retained8 PCM flex={flex}, curve={curve}: oracle={maximum}, partition={partition}, previous128={previous}, bound={bound}, end={:?}", ends[0]);
                for channels in &pcm {
                    assert!(channels[0][ATTACK / 2] > scale as f32 * 0.49,
                        "a rounded multiplier must not freeze the authored attack");
                    assert!(channels.iter().all(|c| c[MUTE + 240..UNMUTE].iter().all(|&v| v == 0.)));
                    assert!(channels.iter().all(|c| c[END - 128..].iter().all(|&v| v == 0.)));
                }
            }
        }
    }

    #[test]
    fn saved_flex_oneshot_preserves_physical_release_and_complete_pcm_without_heap() {
        use crate::{audio::Sample, engine::{Bank, Engine, Expression},
            import::{FlexEnvelope, FlexPoint as ImportedPoint, Group, Loop, Zone}};
        use ni_file::kontakt::{Chunk, StructuredObject, objects::Group as RawGroup};
        fn object(id: u16, version: u16, private: &[u8], public: &[u8], children: &[u8]) -> Chunk {
            let mut data = vec![1]; data.extend(version.to_le_bytes());
            for part in [private, public, children] {
                data.extend((part.len() as u32).to_le_bytes()); data.extend(part);
            }
            Chunk { id, data }
        }
        fn name(out: &mut Vec<u8>, text: &str) {
            out.extend((text.len() as u32).to_le_bytes()); out.extend(text.as_bytes());
        }
        fn raw(env: &FlexEnvelope, source: [u8; 4], depth: f32, kind: u32) -> RawGroup {
            let mut concrete = Vec::new(); env.write(&mut concrete).unwrap();
            let mut wrapper = Vec::new();
            object(7, 0x90, &[], &kind.to_le_bytes(), &concrete).write(&mut wrapper).unwrap();
            let mut private = 1u32.to_le_bytes().to_vec(); name(&mut private, "volume");
            private.extend(depth.to_le_bytes()); private.extend((-1i16).to_le_bytes());
            private.push(0x10); private.extend(0u16.to_le_bytes()); name(&mut private, "ENV_FLEX_VOLUME");
            private.extend([0, 0]); private.extend(source); private.extend(0u32.to_le_bytes());
            name(&mut private, "ENV_FLEX_VOLUME"); private.extend(2u32.to_le_bytes());
            let mut slots = vec![1];
            object(0x0d, 0x80, &private, &[], &wrapper).write(&mut slots).unwrap();
            slots.extend([0; 15]);
            RawGroup(StructuredObject { version: 0x95, private_data: vec![], public_data: vec![],
                children: vec![object(0x3b, 0x10, &[], &slots, &[])] })
        }
        let mut tail = vec![0, 1]; // Native v12 Loop disabled, Oneshot enabled.
        tail.extend([0, 0, 128, 191, 0, 0, 0, 0, 0, 0, 128, 63, 0]);
        let envelope = FlexEnvelope { points: vec![
            ImportedPoint { time_ms: 1., level: 1., curve: 0.5 },
            ImportedPoint { time_ms: 3., level: 0.6, curve: 0.5 },
            ImportedPoint { time_ms: 5., level: 0., curve: 0.5 },
        ], sustain: 1, unknown_index: 0, unknown_tail: tail };
        let decoded = crate::modulation::read_group(&raw(&envelope, [0, 0, 1, 0], 1., 1)).unwrap();
        assert!(decoded.native_flex_oneshot);
        let group = Group { flex_env: decoded.flex_env, native_flex_oneshot: decoded.native_flex_oneshot,
            modulators: decoded.modulators, ..Default::default() };
        let json = serde_json::to_value(&group).unwrap();
        let restored: Group = serde_json::from_value(json.clone()).unwrap();
        assert!(restored.native_flex_oneshot);
        assert_eq!(restored.flex_env, Some(envelope.clone()), "cache keeps actual saved switches/indices");
        assert_eq!(FlexEnvelope::try_from(&restored.flex_env.as_ref().unwrap().to_chunk().unwrap()).unwrap(), envelope);
        let mut legacy = json;
        legacy.as_object_mut().unwrap().remove("native_flex_oneshot");
        legacy["flex_env"].as_object_mut().unwrap().remove("unknown_index");
        legacy["flex_env"].as_object_mut().unwrap().remove("unknown_tail");
        let legacy: Group = serde_json::from_value(legacy).unwrap();
        assert!(!legacy.native_flex_oneshot && legacy.flex_env.unwrap().unknown_tail.is_empty());
        for (source, depth, kind) in [([0, 1, 1, 0], 1., 1), ([0, 0, 0, 0], 1., 1),
            ([0, 0, 1, 1], 1., 1), ([0, 0, 1, 0], 0.5, 1), ([0, 0, 1, 0], 1., 0)] {
            let decoded = crate::modulation::read_group(&raw(&envelope, source, depth, kind)).unwrap();
            assert!(!decoded.native_flex_oneshot);
            assert_eq!(decoded.flex_env, Some(envelope.clone()));
            assert!(decoded.warnings.iter().any(|w| w.contains("native Oneshot/Loop mode not applied")));
        }
        for switch in 0..7 {
            let mut unsupported = envelope.clone();
            match switch {
                0 => unsupported.unknown_tail[0] = 1, // Loop+Oneshot is not a finite mode.
                1 => unsupported.unknown_tail[1] = 0,
                2 => unsupported.unknown_tail[1] = 2,
                3 => { unsupported.unknown_tail.drain(..2); }, // No inferred v11 constructor flags.
                4 => unsupported.unknown_tail[14] = 1, // Timing-record flag is not Oneshot.
                5 => unsupported.points[2].level = 0.25,
                _ => unsupported.unknown_index = 2,
            }
            let decoded = crate::modulation::read_group(&raw(&unsupported, [0, 0, 1, 0], 1., 1)).unwrap();
            assert!(!decoded.native_flex_oneshot && decoded.flex_env == Some(unsupported));
        }
        let mut flex = Flex::from(&envelope); flex.one_shot = true;
        let (mut rendered,mut skipped) = (Envelope::flex(),Envelope::flex());
        let mut source = [0.;433]; let (mut at,mut render_end,mut skip_end) = (0,None,None);
        assert_eq!(crate::plugin::tests::allocations(|| {
            for count in [17,1,7,31,127,13,64,172,1] {
                if at >= 17 { rendered.release(Some(&flex)); skipped.release(Some(&flex)); }
                if let Some(end) = rendered.render_terminal(&mut source[at..at+count],Some(&flex),48_000.) {
                    render_end.get_or_insert(at+end);
                }
                if let Some(end) = skipped.skip_terminal(count,Some(&flex),48_000.) {
                    skip_end.get_or_insert(at+end);
                }
                assert_eq!((rendered.phase(),rendered.level()),(skipped.phase(),skipped.level()));
                at += count;
            }
        }),0);
        assert_eq!((render_end,skip_end),(Some(432),Some(432)));
        assert!(rendered.done() && skipped.done());
        fn create(group: &Group, shared: bool, frames: usize, looped: bool) -> Engine {
            let zone = Zone { loop_range: looped.then_some(Loop { start: 0, end: frames,
                crossfade: 0, alternating: false, until_release: false }), ..Default::default() };
            let bank = Bank::from_samples(vec![group.clone()], vec![zone], vec![(Default::default(),
                Sample { rate: 48_000, frames: vec![[0.25; 2]; frames] })]).unwrap()
                .with_prepared_settings(|rows| {
                    if group.volume_env.is_none() { rows[0].envelope = Some(Ahdsr::UNITY); }
                }).unwrap();
            let mut engine = Engine::default(); engine.reset(48_000.); engine.player.shared = shared;
            engine.set_bank(Some(Box::new(bank))); engine
        }
        // An independent piecewise-linear source reference uses authored frame
        // durations. This mode gate does not assert native Flex curve/startup parity.
        let level = |age: usize| -> f32 {
            match age {
                0..48 => (age + 1) as f32 / 48.,
                48..192 => 1. - 0.4 * (age - 47) as f32 / 144.,
                192..432 => 0.6 * (1. - (age - 191) as f32 / 240.),
                _ => 0.,
            }
        };
        const FRAMES: usize = 768;
        for shared in [false, true] { for mute in [false, true] {
            let mut engines = [create(&group, shared, 256, true), create(&group, shared, 256, true),
                create(&group, shared, 256, true)];
            let dry_group = Group { gain:group.gain, ..Group::default() };
            let mut dry = [create(&dry_group, false, 256, true), create(&dry_group, false, 256, true)];
            let mut pcm = [[[0.; FRAMES]; 2]; 3]; let mut unmodulated = [[[0.; FRAMES]; 2]; 2];
            assert_eq!(crate::plugin::tests::allocations(|| {
                for engine in &mut engines { engine.note_on(0, 60, 127); engine.cc(0, 64, 127); }
                dry[0].note_on(0, 60, 127);
                for (start, end) in [(0,17), (17,43), (43,66), (66,91), (91,110), (110,237),
                    (237,269), (269,290), (290,432), (432,669), (669,700), (700,FRAMES)] {
                    if matches!(start, 17 | 91 | 269 | 290) {
                        for engine in &mut engines[..2] { engine.note_off(0, 60); }
                    }
                    if start == 43 { for engine in &mut engines { engine.cc(0, 64, 0); } }
                    if mute && matches!(start, 66 | 110) {
                        for engine in &mut engines { engine.player.midi.set_expression(0, 60,
                            Expression { gain: if start == 66 { 0. } else { 1. }, ..Default::default() }); }
                        for engine in &mut dry { engine.player.midi.set_expression(0, 60,
                            Expression { gain: if start == 66 { 0. } else { 1. }, ..Default::default() }); }
                    }
                    if start == 237 {
                        for engine in &mut engines { engine.note_on(0, 60, 127); }
                        dry[1].note_on(0, 60, 127);
                    }
                    if start == 700 { engines[2].note_off(0, 60); engines[2].note_off(0, 60); }
                    for (index, engine) in engines.iter_mut().enumerate() {
                        let sizes: &[usize] = if index == 1 { &[1,7,31,127,13,64] } else { &[128] };
                        let (mut at, mut fragment) = (start, 0);
                        while at < end {
                            let n = sizes[fragment % sizes.len()].min(end-at);
                            let [left,right] = &mut pcm[index];
                            engine.render_checked(&mut left[at..at+n], &mut right[at..at+n]).unwrap();
                            let (mut empty_left,mut empty_right) = ([],[]);
                            engine.render_checked(&mut empty_left,&mut empty_right).unwrap();
                            at += n; fragment += 1;
                        }
                        // The second attack is delivered at the next interval's start237.
                        let expected = usize::from(end < 432) + usize::from(end > 237 && end < 669);
                        assert_eq!(engine.active_voices(), expected, "exact finite point boundaries");
                        if index < 2 && end == 66 {
                            assert!(engine.player.voices.iter().all(|voice| voice.released && !voice.held),
                                "pedal-up records the physical release while Oneshot keeps sounding");
                        }
                    }
                    for (index,engine) in dry.iter_mut().enumerate() {
                        let [left,right] = &mut unmodulated[index];
                        engine.render_checked(&mut left[start..end],&mut right[start..end]).unwrap();
                    }
                }
            }), 0);
            for index in 0..3 { for channel in 0..2 { for frame in 0..FRAMES {
                let expected = unmodulated[0][channel][frame] * level(frame)
                    + if frame >= 237 { unmodulated[1][channel][frame] * level(frame-237) } else { 0. };
                assert!((pcm[index][channel][frame]-expected).abs() < 2e-6,
                    "Oneshot complete PCM: shared={shared}, mute={mute}, engine={index}, frame={frame}");
                assert!((pcm[index][channel][frame]-pcm[2][channel][frame]).abs() < 2e-6,
                    "early or duplicate physical release changes Oneshot PCM");
            } } }
        } }
        // EOF and a separate authored AHDSR still own independent termination.
        let mut eof = create(&group, false, 96, false); eof.note_on(0,60,127);
        let (mut l,mut r) = ([0.;96],[0.;96]); eof.render_checked(&mut l,&mut r).unwrap();
        assert_eq!(eof.active_voices(),0);
        let mut paired = group.clone(); paired.volume_env = Some(crate::import::Ahdsr {
            attack_curve: 0., attack_ms: 0., hold_ms: 0., decay_ms: 0., sustain: 1.,
            release_ms: 1., unknown_flag: 0, unknown_tail: Vec::new() });
        let mut paired = create(&paired,false,256,true); paired.note_on(0,60,127);
        paired.render_checked(&mut [0.;17],&mut [0.;17]).unwrap(); paired.note_off(0,60);
        paired.render_checked(&mut [0.;128],&mut [0.;128]).unwrap(); assert_eq!(paired.active_voices(),0);
        let mut ordinary = group.clone(); ordinary.native_flex_oneshot = false;
        let mut ordinary = create(&ordinary,false,256,true); ordinary.note_on(0,60,127);
        ordinary.render_checked(&mut [0.;17],&mut [0.;17]).unwrap(); ordinary.note_off(0,60);
        ordinary.render_checked(&mut [0.;240],&mut [0.;240]).unwrap();
        assert_eq!(ordinary.active_voices(),0,"ordinary Flex retains its full release segment");
        let mut stopped = create(&group,false,256,true); stopped.note_on(0,60,127);
        stopped.render_checked(&mut [0.;17],&mut [0.;17]).unwrap(); stopped.cc(0,120,0);
        stopped.render_checked(&mut [0.;256],&mut [0.;256]).unwrap();
        assert_eq!(stopped.active_voices(),0,"explicit SoundOff owns its stop fade before Flex completion");
    }

    #[test]
    fn saved_flex_loop_preserves_return_release_and_complete_pcm_without_heap() {
        use crate::{audio::Sample, engine::{Bank, Engine, Expression, HostNote, HostPattern},
            import::{FlexEnvelope, FlexPoint as ImportedPoint, Group, Loop, Zone}};
        use ni_file::kontakt::{Chunk, StructuredObject, objects::Group as RawGroup};
        fn object(id:u16,version:u16,private:&[u8],public:&[u8],children:&[u8])->Chunk {
            let mut data=vec![1]; data.extend(version.to_le_bytes());
            for part in [private,public,children] { data.extend((part.len() as u32).to_le_bytes()); data.extend(part); }
            Chunk { id,data }
        }
        fn name(out:&mut Vec<u8>,text:&str) { out.extend((text.len() as u32).to_le_bytes()); out.extend(text.as_bytes()); }
        fn raw(env:&FlexEnvelope,source:[u8;4],depth:f32)->RawGroup {
            let mut concrete=Vec::new(); env.write(&mut concrete).unwrap();
            let mut wrapper=Vec::new(); object(7,0x90,&[],&1u32.to_le_bytes(),&concrete).write(&mut wrapper).unwrap();
            let mut private=1u32.to_le_bytes().to_vec(); name(&mut private,"volume");
            private.extend(depth.to_le_bytes()); private.extend((-1i16).to_le_bytes());
            private.push(0x10); private.extend(0u16.to_le_bytes()); name(&mut private,"ENV_FLEX_VOLUME");
            private.extend([0,0]); private.extend(source); private.extend(0u32.to_le_bytes());
            name(&mut private,"ENV_FLEX_VOLUME"); private.extend(2u32.to_le_bytes());
            let mut slots=vec![1]; object(0x0d,0x80,&private,&[],&wrapper).write(&mut slots).unwrap(); slots.extend([0;15]);
            RawGroup(StructuredObject { version:0x95,private_data:vec![],public_data:vec![],
                children:vec![object(0x3b,0x10,&[],&slots,&[])] })
        }
        let mut tail=vec![1,0]; tail.extend([0,0,128,191,0,0,0,0,0,0,128,63,0]);
        let envelope=FlexEnvelope { points:vec![
            ImportedPoint { time_ms:1.,level:1.,curve:0.5 },
            ImportedPoint { time_ms:2.,level:0.25,curve:0.5 },
            ImportedPoint { time_ms:3.,level:0.75,curve:0.5 },
            ImportedPoint { time_ms:4.,level:0.,curve:0.5 },
        ],sustain:2,unknown_index:0,unknown_tail:tail };
        let decoded=crate::modulation::read_group(&raw(&envelope,[0,0,1,0],1.)).unwrap();
        assert!(decoded.native_flex_loop && !decoded.native_flex_oneshot);
        let group=Group { flex_env:decoded.flex_env,native_flex_loop:decoded.native_flex_loop,
            modulators:decoded.modulators,..Default::default() };
        let json=serde_json::to_value(&group).unwrap();
        let restored:Group=serde_json::from_value(json.clone()).unwrap();
        assert!(restored.native_flex_loop);
        assert_eq!(restored.flex_env,Some(envelope.clone()));
        assert_eq!(FlexEnvelope::try_from(&envelope.to_chunk().unwrap()).unwrap(),envelope);
        let mut legacy=json; legacy.as_object_mut().unwrap().remove("native_flex_loop");
        assert!(!serde_json::from_value::<Group>(legacy).unwrap().native_flex_loop);
        for (source,depth) in [([0,1,1,0],1.),([0,0,0,0],1.),([0,0,1,1],1.),([0,0,1,0],0.5)] {
            let decoded=crate::modulation::read_group(&raw(&envelope,source,depth)).unwrap();
            assert!(!decoded.native_flex_loop && decoded.flex_env==Some(envelope.clone()));
            assert!(decoded.warnings.iter().any(|w| w.contains("native Oneshot/Loop mode not applied")));
        }
        for mode in 0..6 {
            let mut unsupported=envelope.clone();
            match mode {
                0=>unsupported.unknown_tail[1]=1,
                1=>unsupported.unknown_index=1, // Single-segment hold is a separate native law.
                2=>unsupported.unknown_tail[0]=2,
                3=>unsupported.unknown_tail[14]=1,
                4=>unsupported.points[3].level=0.25,
                _=>{ unsupported.unknown_tail.drain(..2); },
            }
            let decoded=crate::modulation::read_group(&raw(&unsupported,[0,0,1,0],1.)).unwrap();
            assert!(!decoded.native_flex_loop && decoded.flex_env==Some(unsupported));
        }
        fn create(group:&Group,shared:bool,frames:usize,looped:bool)->Engine {
            let zone=Zone { loop_range:looped.then_some(Loop { start:0,end:frames,
                crossfade:0,alternating:false,until_release:false }),..Default::default() };
            let bank=Bank::from_samples(vec![group.clone()],vec![zone],vec![(Default::default(),
                Sample { rate:48_000,frames:vec![[0.25;2];frames] })]).unwrap()
                .with_prepared_settings(|rows| {
                    assert_eq!(rows[0].flex.as_ref().and_then(|f| f.loop_start),group.native_flex_loop.then_some(0));
                    rows[0].envelope=Some(Ahdsr::UNITY);
                }).unwrap();
            let mut engine=Engine::default(); engine.reset(48_000.); engine.player.shared=shared;
            engine.set_bank(Some(Box::new(bank))); engine
        }
        // Independent frame-domain oracle for the existing linear Flex kernel.
        // First loop return keeps .75, rather than resetting to start-point 1.
        let held=|age:usize|->f32 {
            if age<48 { (age+1) as f32/48. }
            else if age<144 { 1.-0.75*(age-47) as f32/96. }
            else if age<288 { 0.25+0.5*(age-143) as f32/144. }
            else { let phase=(age-288)%240;
                if phase<96 { 0.75-0.5*(phase+1) as f32/96. }
                else { 0.25+0.5*(phase-95) as f32/144. }
            }
        };
        let level=|age:usize,off:usize|->f32 {
            if age<off { held(age) }
            else if age<off+192 { held(off-1)*(1.-(age-off+1) as f32/192.) }
            else { 0. }
        };
        const FRAMES:usize=1280;
        let first=HostNote { port:0,channel:0,key:60,id:71,clap:true };
        let second=HostNote { id:72,..first };
        let pattern=|note:HostNote| HostPattern { port:0,channel:0,key:60,id:note.id,clap:true };
        for shared in [false,true] { for mute in [false,true] {
            let mut ordinary=restored.clone(); ordinary.native_flex_loop=false;
            let mut engines=[create(&restored,shared,256,true),create(&restored,shared,256,true),
                create(&restored,shared,256,true),create(&ordinary,shared,256,true)];
            let mut dry=[create(&Group::default(),false,256,true),create(&Group::default(),false,256,true)];
            let mut pcm=[[[0.;FRAMES];2];4]; let mut unmodulated=[[[0.;FRAMES];2];2];
            assert_eq!(crate::plugin::tests::allocations(|| {
                for engine in &mut engines { assert!(engine.host_note_on(first,0,60,127,0.)); engine.cc(0,64,127); }
                dry[0].note_on(0,60,127);
                let mut start=0;
                for end in [103,151,288,317,350,528,713,853,905,906,1045,1046,FRAMES] {
                    for (index,engine) in engines.iter_mut().enumerate() {
                        let mut at=start; let mut part=0;
                        while at<end {
                            let n=if index==1 { [1,7,31,127,13,64][part%6].min(end-at) } else { (end-at).min(128) };
                            let [l,r]=&mut pcm[index]; engine.render_checked(&mut l[at..at+n],&mut r[at..at+n]).unwrap();
                            engine.render_checked(&mut [],&mut []).unwrap(); at+=n; part+=1;
                        }
                        if index<3 {
                            let expected=usize::from(end<905)+usize::from(end>317 && end<1045);
                            assert_eq!(engine.active_voices(),expected,"exact post-loop endpoint {end}");
                        }
                    }
                    for (index,engine) in dry.iter_mut().enumerate() {
                        let [l,r]=&mut unmodulated[index]; engine.render_checked(&mut l[start..end],&mut r[start..end]).unwrap();
                    }
                    if mute && matches!(end,103|528) {
                        for engine in engines.iter_mut().chain(dry.iter_mut()) { engine.set_expression_on(0,60,
                            Expression { gain:if end==103 {0.} else {1.},..Default::default() }); }
                    }
                    if end==317 {
                        for engine in &mut engines { assert!(engine.host_note_on(second,0,60,127,0.)); }
                        dry[1].note_on(0,60,127);
                    }
                    if end==350 {
                        for engine in &mut engines[..2] { engine.host_note_off(pattern(first)); engine.host_note_off(pattern(first)); }
                        engines[3].host_note_off(pattern(first));
                        assert!(engines[0].host_key_held(0,60),"overlap owner remains held");
                        assert!(engines[0].player.voices.iter().any(|v| !v.held && !v.released),"pedal retains released physical owner");
                    }
                    if end==713 {
                        engines[2].host_note_off(pattern(first));
                        for engine in &mut engines { engine.cc(0,64,0); }
                        assert!(engines[0].player.voices.iter().any(|v| v.released && !v.held));
                    }
                    if end==853 { for engine in &mut engines { engine.host_note_off(pattern(second)); } }
                    start=end;
                }
                for engine in &mut engines[..3] {
                    engine.mark_host_notes();
                    assert_eq!(engine.host_note_end_at(first),Some(905));
                    assert_eq!(engine.host_note_end_at(second),Some(1045));
                }
            }),0);
            for index in 0..3 { for channel in 0..2 { for frame in 0..FRAMES {
                let expected=unmodulated[0][channel][frame]*level(frame,713)
                    + if frame>=317 { unmodulated[1][channel][frame]*level(frame-317,536) } else {0.};
                assert!((pcm[index][channel][frame]-expected).abs()<2e-6,"loop complete PCM shared={shared} mute={mute} engine={index} frame={frame}");
                assert!((pcm[index][channel][frame]-pcm[2][channel][frame]).abs()<2e-6,"independently deferred physical release");
            } } }
            assert!(pcm[0].iter().flatten().zip(pcm[3].iter().flatten()).any(|(a,b)| (a-b).abs()>1e-5),
                "ordinary held Flex must differ after a wider loop returns");
            if mute {
                // The240-frame gain ramp completes at343; both overlapping
                // Flex clocks then advance fully unheard until resume528.
                assert!(pcm.iter().all(|note| note.iter().all(|channel|
                    channel[343..528].iter().all(|&value| value == 0.))));
            }
        } }
        let mut eof=create(&restored,false,96,false); eof.note_on(0,60,127);
        eof.render_checked(&mut [0.;128],&mut [0.;128]).unwrap(); assert_eq!(eof.active_voices(),0);
        let mut stopped=create(&restored,false,256,true); stopped.note_on(0,60,127);
        stopped.render_checked(&mut [0.;400],&mut [0.;400]).unwrap(); stopped.cc(0,120,0);
        stopped.render_checked(&mut [0.;256],&mut [0.;256]).unwrap(); assert_eq!(stopped.active_voices(),0);
    }
}
