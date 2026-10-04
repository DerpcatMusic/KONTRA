//! The independently decoded saved retriggered Multi LFO subset.
//! Random, wider free-running sources and live frequency conversion remain
//! unsupported. See audits/MODULATION.md for the source-clock boundary.

#[cfg(all(test, feature = "plugin"))]
#[path = "lfo_timing_tests.rs"]
mod timing_tests;

#[cfg(all(test, feature = "plugin"))]
#[path = "lfo_shared_tests.rs"]
mod shared_tests;

use super::voice::sample_clock;
#[cfg(test)]
use super::voice::FIXED_ONE;
use crate::modulation::{PitchLfo, VolumeLfo};

/// Each native slot owns one source clock and at most one prepared volume target.
/// Validate before installation, including consuming edits to prepared settings.
pub(crate) fn validate_sources(pitch: &[PitchLfo], volume: &[VolumeLfo]) -> anyhow::Result<()> {
    let mut sources = [None::<&PitchLfo>; super::params::PITCH_ENVS];
    let (mut pitch_slots, mut volume_slots) = (0_u16, 0_u16);
    for (is_pitch, source) in pitch.iter().map(|s| (true, s))
        .chain(volume.iter().map(|l| (false, &l.source))) {
        let slot = usize::from(source.slot);
        anyhow::ensure!(slot < sources.len(),
            "LFO source slot {slot} is outside the sixteen native source slots");
        anyhow::ensure!([source.start_phase, source.count, source.note_value, source.sine, source.fade_ms]
            .iter().all(|v| v.is_finite()), "LFO source slot {slot} has nonfinite source geometry");
        if let Some(sync) = source.fade_sync {
            anyhow::ensure!(sync.note_value.is_finite() && sync.note_value > 1e-9 && sync.count.is_finite() && source.fade_ms == 0.,
                "LFO source slot {slot} has invalid synchronized fade geometry or ambiguous units");
        }
        if let Some(digital) = &source.digital {
            anyhow::ensure!((0. ..=1.).contains(&source.start_phase)
                && [digital.rectangle, digital.triangle, digital.saw, digital.pulse_width]
                    .iter().all(|v| v.is_finite()),
                "Digital LFO source slot {slot} has invalid phase or nonfinite waveform geometry");
        }
        anyhow::ensure!(!source.shared || (source.digital.is_some() && source.fade_ms == 0. && source.fade_sync.is_none()
            && (source.note_value > 1e-9 || source.count > 0.)),
            "Shared LFO source slot {slot} requires deterministic Digital, zero fade and known frequency timing");
        let mask = if is_pitch { &mut pitch_slots } else { &mut volume_slots };
        let bit = 1_u16 << slot;
        anyhow::ensure!(*mask & bit == 0,
            "LFO source slot {slot} has duplicate {} rows", if is_pitch { "pitch" } else { "volume" });
        *mask |= bit;
        if let Some(previous) = sources[slot] {
            // Depth/target identity and volume lag/polarity belong to consumers.
            anyhow::ensure!(previous.start_phase == source.start_phase
                && previous.count == source.count && previous.note_value == source.note_value
                && previous.sine == source.sine && previous.digital == source.digital
                && previous.fade_ms == source.fade_ms && previous.fade_sync == source.fade_sync
                && previous.extended_fade == source.extended_fade
                && previous.bypassed == source.bypassed
                && previous.shared == source.shared,
                "LFO source slot {slot} has inconsistent shared source geometry");
        } else { sources[slot] = Some(source); }
    }
    anyhow::ensure!(pitch.iter().all(|l| l.depth.is_finite()
        && l.targets.iter().all(|t| t.1.is_finite())), "LFO pitch target has nonfinite depth");
    anyhow::ensure!(volume.iter().all(|l| l.intensity.is_finite() && l.lag_ms >= 0),
        "LFO volume target has nonfinite intensity or negative lag");
    Ok(())
}

/// Worker-prepared sparse source owners. Mutable phase belongs to a group/slot;
/// target lag and audio interpolation remain with each voice. Rows are retired
/// with InstrumentState's existing off-thread Bank handoff.
#[derive(Default)]
pub(super) struct Shared {
    groups: Vec<std::ops::Range<usize>>,
    rows: Vec<SharedRow>,
}

struct SharedRow {
    slot: u8,
    state: std::cell::Cell<SharedState>,
}

#[derive(Clone, Copy, Default)]
struct SharedState {
    phase: f64,
    next_phase: f64,
    step: f64,
    generation: u64,
    base: u64,
    end: u64,
    points: [f32; 32],
    count: u8,
    committed: bool,
    previewed: bool,
    producer: Option<u64>,
}

impl Shared {
    /// Called only during bank preparation, after source validation.
    pub(super) fn new(settings: &[super::GroupSettings]) -> Self {
        let mut result = Self::default();
        if !settings.iter().any(|g| g.pitch_lfos.iter().chain(g.volume_lfos.iter().map(|l| &l.source))
            .any(|l| l.shared)) { return result; }
        for group in settings {
            let start = result.rows.len();
            for slot in 0..16 {
                if group
                    .pitch_lfos
                    .iter()
                    .chain(group.volume_lfos.iter().map(|l| &l.source))
                    .any(|l| l.shared && l.slot == slot)
                {
                    result.rows.push(SharedRow {
                        slot,
                        state: Default::default(),
                    });
                }
            }
            result.groups.push(start..result.rows.len());
        }
        result
    }

    pub(super) fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    fn group(&self, group: usize) -> &[SharedRow] {
        self.groups
            .get(group)
            .map_or(&[], |r| &self.rows[r.clone()])
    }

    /// All contenders in a progressive segment begin their admitted audio at
    /// `now`. The copied candidate never uses a cold voice's physical onset.
    /// Nomination below changes only identity, never these plan/render points.
    pub(super) fn stage(
        &self,
        group: usize,
        settings: &super::GroupSettings,
        now: u64,
        window: (u64, u64),
        rate: f32,
        tempo: f32,
        timings: &[Timing; 16],
    ) {
        self.stage_candidate(group, settings, now, window, rate, tempo, timings, false);
    }

    /// Admission may inspect copied points, but only rendering owns generation
    /// publication. It must refresh this preview after same-time source edits.
    pub(super) fn stage_preview(
        &self,
        group: usize,
        settings: &super::GroupSettings,
        now: u64,
        window: (u64, u64),
        rate: f32,
        tempo: f32,
        timings: &[Timing; 16],
    ) {
        self.stage_candidate(group, settings, now, window, rate, tempo, timings, true);
    }

    fn stage_candidate(
        &self,
        group: usize,
        settings: &super::GroupSettings,
        now: u64,
        window: (u64, u64),
        rate: f32,
        tempo: f32,
        timings: &[Timing; 16],
        preview: bool,
    ) {
        if now >= window.1 {
            return;
        }
        for row in self.group(group) {
            let mut state = row.state.get();
            if state.generation == window.0
                && state.end == window.1
                && (state.committed || (!preview && !state.previewed && state.base == now))
            {
                continue;
            }
            state.count = 0;
            state.committed = false;
            state.previewed = preview;
            state.producer = None;
            state.generation = window.0;
            state.base = now;
            state.end = window.1;
            let source = settings
                .pitch_lfos
                .iter()
                .chain(settings.volume_lfos.iter().map(|l| &l.source))
                .find(|l| l.shared && l.slot == row.slot);
            if let Some(source) = source.filter(|s| !s.bypassed)
                && let Some(step) = Clock::digital_step(source, rate, tempo, Some(timings))
            {
                let frames = window.1 - now;
                let count = frames.div_ceil(32).min(32) as usize;
                let mut phase = state.phase;
                for point in &mut state.points[..count] {
                    *point = Clock::waveform(source, phase);
                    phase += step;
                    if phase > 1. {
                        phase -= 1.;
                    }
                }
                // Native common zero-fade still corrects the final padded
                // point: source+5C * double(1/32) * doubleStep.
                phase -= ((count as u64 * 32 - frames) as f64 * 0.03125) * step;
                if phase < 0. {
                    phase += 1.;
                }
                state.next_phase = phase;
                state.step = step;
                state.count = count as u8;
            }
            row.state.set(state);
        }
    }

    /// A successfully planned Started voice, including muted/released tails,
    /// can nominate its stable admission serial. Vec compaction is irrelevant.
    pub(super) fn nominate(&self, group: usize, admission: u64) {
        for row in self.group(group) {
            let mut state = row.state.get();
            if !state.committed && state.count != 0 {
                state.producer = Some(state.producer.map_or(admission, |old| old.min(admission)));
                row.state.set(state);
            }
        }
    }

    pub(super) fn commit(&self) {
        for row in &self.rows {
            let mut state = row.state.get();
            if !state.committed && state.producer.is_some() {
                state.phase = state.next_phase;
                state.committed = true;
                row.state.set(state);
            }
        }
    }

    pub(super) fn view(&self, group: usize, now: u64) -> View<'_> {
        View {
            owners: self,
            group,
            now,
        }
    }
}

/// Planning/rendering share immutable candidate points. Lookahead uses a copied
/// phase forecast only; it cannot produce or commit a future host generation.
#[derive(Clone, Copy)]
pub(super) struct View<'a> {
    owners: &'a Shared,
    group: usize,
    now: u64,
}

impl View<'_> {
    fn epoch(self) -> Option<(u64, u64)> {
        let state = self.owners.group(self.group).first()?.state.get();
        Some((state.generation, state.end))
    }

    fn point(self, source: &PitchLfo, offset: usize, entry: u64, entry_offset: u8) -> Option<f32> {
        let state = self
            .owners
            .group(self.group)
            .iter()
            .find(|r| r.slot == source.slot)?
            .state
            .get();
        if state.count == 0 {
            return None;
        }
        let frame = self.now.checked_add(offset as u64)?;
        if frame < state.end {
            // Select the full-window suffix once, then consume its points at
            // this voice's retained modulo-32 boundaries. A late first producer
            // supplies a shorter cache without changing the receiver's grid.
            let effective_entry = entry.max(state.base);
            let remaining = state.end.checked_sub(effective_entry)?.div_ceil(32) as usize;
            let suffix = usize::from(state.count).saturating_sub(remaining);
            let elapsed = effective_entry.checked_sub(entry)?;
            let offset = (u64::from(entry_offset) + elapsed) & 31;
            let first_boundary = effective_entry.checked_add((32 - offset) & 31)?;
            // Initial endpoint seeding reads point zero without consuming it.
            let cursor = frame.saturating_sub(first_boundary) / 32;
            let index = suffix.checked_add(usize::try_from(cursor).ok()?)?;
            return (index < usize::from(state.count))
                .then(|| state.points[index]);
        }
        let steps = (frame - state.end) / 32;
        // Existing speculative packet horizons are bounded; a longer forecast
        // cannot reject the actual current fragment or advance this owner.
        if steps > 64 {
            return None;
        }
        let mut phase = state.next_phase;
        for _ in 0..steps {
            phase += state.step;
            if phase > 1. {
                phase -= 1.;
            }
        }
        Some(Clock::waveform(source, phase))
    }
}

/// One note's native source phases and retained pitch/volume interpolators.
/// The native source clock pauses when bypassed; its audio interpolation
/// offset advances separately, including short event fragments.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Clock {
    phase: [f64; 16],
    digital_started: u16,
    previous: f32,
    current: f32,
    offset: u8,
    initialized: bool,
    fades: [Fade; 16],
    fade_started: u16,
    volume: [Option<super::lfo_volume::Target>; 16],
    volume_previous: f32,
    volume_current: f32,
    volume_initialized: bool,
    shared_window: (u64, u64),
    shared_entry: u64,
    shared_entry_offset: u8,
}

/// Group source prototype; same-kind native refresh replaces timing, not voice state.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct FadeTiming {
    remaining: u32,
    factor: f32,
}

impl FadeTiming {
    fn new(ms: f32, rate: f32) -> Self {
        let remaining = (ms * (rate / 32.) * 0.001) as u32;
        let factor = if remaining == 0 { 1. } else {
            (1. + 1. / f64::from(0.3f32)).powf(1. / f64::from(remaining)) as f32
        };
        Self { remaining, factor }
    }

    fn synchronized(source: &PitchLfo, rate: f32, tempo: f32) -> Self {
        let tempo = if tempo < 0.1 { 120. } else { tempo };
        // Native delay getter: ordered f32 arithmetic, then its 0..5000ms bounds.
        let sync = source.fade_sync.expect("prepared synchronized source");
        let ms = (((sync.note_value * 60.) / tempo) * sync.count.max(1.)) * 1000.;
        Self::new(ms.clamp(0., 5000.), rate)
    }
}

/// Allocated with the bank's prepared settings; callback refresh only replaces numbers.
#[derive(Default)]
pub(super) struct Timings(Vec<[Timing; 16]>);

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Timing {
    frequency: f32,
    step: f64,
    fade: FadeTiming,
}

impl Timings {
    pub(super) fn new(settings: &[super::GroupSettings]) -> Self {
        Self(vec![[Timing::default(); 16]; settings.len()])
    }

    pub(super) fn group(&self, group: usize) -> &[Timing; 16] { &self.0[group] }

    pub(super) fn refresh(&mut self, settings: &[super::GroupSettings], group: Option<usize>, rate: f32, tempo: f32) {
        let groups = group.map_or(0..settings.len(), |g| g..(g + 1).min(settings.len()));
        for g in groups {
            for source in settings[g].pitch_lfos.iter().chain(settings[g].volume_lfos.iter().map(|l| &l.source)) {
                let frequency = source.frequency(tempo);
                let step = f64::from(frequency * (f64::from(rate / 32.).recip() as f32));
                let fade = if source.fade_sync.is_some() { FadeTiming::synchronized(source, rate, tempo) }
                    else { FadeTiming::default() };
                self.0[g][usize::from(source.slot)] = Timing { frequency, step, fade };
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Fade {
    remaining: u32,
    value: f32,
    factor: f32,
    extended: bool,
}

impl Fade {
    fn new(ms: f32, rate: f32, extended: bool) -> Self {
        // Native time getter returns milliseconds; DSP runs at rate / 32.
        let FadeTiming { remaining, factor } = FadeTiming::new(ms, rate);
        Self {
            remaining,
            value: 0.3,
            factor,
            extended,
        }
    }

    fn next(&mut self) -> f32 {
        if self.remaining == 0 {
            return 1.;
        }
        let gain = self.value - 0.3;
        // Native v71/v72 seed false; v73 carries the appended clamp switch.
        // Both modes emit state-.3 before advancing, for exactly N points.
        self.value = if self.extended {
            (self.value * self.factor).clamp(0.3, 1.3)
        } else {
            (self.value * self.factor).clamp(0., 1.)
        };
        self.remaining -= 1;
        gain
    }
}

impl Clock {
    pub(super) fn new(settings: &super::GroupSettings, timings: &[Timing; 16]) -> Self {
        let mut clock = Self::default();
        clock.sync_fades(&settings.pitch_lfos, &settings.volume_lfos, Some(timings), 0., 120.);
        clock
    }

    fn sync_fades(&mut self, pitch: &[PitchLfo], volume: &[VolumeLfo], timings: Option<&[Timing; 16]>, rate: f32, tempo: f32) {
        for source in pitch.iter().chain(volume.iter().map(|l| &l.source)) {
            if source.fade_sync.is_none() { continue; }
            let slot = usize::from(source.slot);
            let timing = timings.map_or_else(|| FadeTiming::synchronized(source, rate, tempo), |rows| rows[slot].fade);
            let bit = 1 << slot;
            if self.fade_started & bit == 0 {
                self.fades[slot] = Fade { remaining: timing.remaining, factor: timing.factor,
                    value: 0.3, extended: source.extended_fade };
                self.fade_started |= bit;
            } else {
                // Native source vtable+18 copies F4/F8, retaining mutable F0/FC.
                self.fades[slot].factor = timing.factor;
            }
        }
    }

    pub(super) fn sources_representable(lfos: &[PitchLfo], volume_lfos: &[VolumeLfo], rate: f32, tempo: f32, timings: Option<&[Timing; 16]>) -> bool {
        lfos.iter().chain(volume_lfos.iter().map(|l| &l.source)).all(|source| {
            let fade_valid = source.fade_sync.is_none() || timings.map_or_else(
                || FadeTiming::synchronized(source, rate, tempo), |rows| rows[usize::from(source.slot)].fade)
                .remaining <= i32::MAX as u32;
            source.bypassed || (fade_valid && (source.digital.is_none()
                || Self::digital_step(source, rate, tempo, timings).is_some()))
        })
    }

    fn source_frequency(source: &PitchLfo, tempo: f32, timings: Option<&[Timing; 16]>) -> f32 {
        if source.note_value > 1e-9 {
            timings.map_or_else(|| source.frequency(tempo), |rows| rows[usize::from(source.slot)].frequency)
        } else { source.frequency(tempo) }
    }

    fn digital_step(source: &PitchLfo, rate: f32, tempo: f32, timings: Option<&[Timing; 16]>) -> Option<f64> {
        let step = if source.note_value > 1e-9 && let Some(rows) = timings {
            rows[usize::from(source.slot)].step
        } else {
            let reciprocal = f64::from(rate / 32.).recip() as f32;
            f64::from(source.frequency(tempo) * reciprocal)
        };
        (step.is_finite() && (0. ..=1.).contains(&step)).then_some(step)
    }

    fn digital_point(&mut self, source: &PitchLfo, rate: f32, tempo: f32,
        sampled: &mut [Option<f32>; 16], fades: &mut [Option<f32>; 16],
        shared: Option<View<'_>>, timings: Option<&[Timing; 16]>, frame: usize) -> Option<f32> {
        // The proved strict single wrap requires at most one cycle per point.
        // Lower public engine rates need a separately proved native rate law.
        let step = Self::digital_step(source, rate, tempo, timings)?;
        let slot = usize::from(source.slot);
        if let Some(point) = sampled[slot] { return Some(point); }
        if source.shared {
            let point = shared?.point(source, frame, self.shared_entry, self.shared_entry_offset)?;
            sampled[slot] = Some(point);
            return Some(point);
        }
        let bit = 1 << slot;
        if self.digital_started & bit == 0 {
            self.phase[slot] = f64::from(source.start_phase);
            self.digital_started |= bit;
        }
        let mut point = Self::waveform(source, self.phase[slot]);
        point *= self.source_fade(source, rate, fades);
        sampled[slot] = Some(point);
        // One source advances after its point, once for every shared target.
        // Strict comparison preserves the authored and subsequent endpoint1.
        self.phase[slot] += step;
        if self.phase[slot] > 1. { self.phase[slot] -= 1.; }
        Some(point)
    }

    fn waveform(source: &PitchLfo, phase: f64) -> f32 {
        let digital = source.digital.as_ref().expect("prepared Digital source");
        // Native arithmetic shapes accumulate in this order. Sine retains the
        // existing standard-sine approximation; no native table parity is claimed.
        let mut point = -(phase * std::f64::consts::TAU).sin() as f32 * source.sine;
        let mut saw = 2. * phase;
        if saw > 1. { saw -= 1.; }
        point += (2. * saw - 1.) as f32 * digital.saw;
        let triangle = if phase < 0.5 { 4. * (phase - 0.25) } else { 4. * (0.75 - phase) };
        point += triangle as f32 * digital.triangle;
        point += if phase < f64::from(digital.pulse_width.clamp(0.05, 0.95)) {
            -digital.rectangle
        } else { digital.rectangle };
        if digital.normalize {
            let total = ((0. + digital.rectangle.abs()) + digital.triangle.abs())
                + (digital.saw.abs() + source.sine.abs());
            point *= total.max(1.).recip();
        }
        point
    }

    fn source_fade(&mut self, source: &PitchLfo, rate: f32, sampled: &mut [Option<f32>; 16]) -> f32 {
        if source.fade_ms <= 0. && source.fade_sync.is_none() { return 1.; }
        let slot = usize::from(source.slot);
        if let Some(gain) = sampled[slot] { return gain; }
        let bit = 1 << slot;
        if self.fade_started & bit == 0 {
            self.fades[slot] = Fade::new(source.fade_ms, rate, source.extended_fade);
            self.fade_started |= bit;
        }
        let gain = self.fades[slot].next();
        sampled[slot] = Some(gain);
        gain
    }

    pub fn skip_bypassed(&mut self, n: usize) {
        self.offset = ((usize::from(self.offset) + n) & 31) as u8;
    }

    /// Relative source positions for one ordinary sampler block. Previewing a
    /// copied clock during planning gives the same reach as rendering it.
    #[cfg(test)]
    pub fn positions(
        &mut self,
        lfos: &[PitchLfo],
        volume_lfos: &[VolumeLfo],
        rate: f32,
        tempo: f32,
        step: f64,
        out: &mut [u64],
        volume_out: Option<&mut [f32]>,
    ) -> Option<(u64, u64)> {
        self.positions_with_pitch(
            lfos,
            volume_lfos,
            rate,
            tempo,
            step,
            None,
            false,
            out,
            volume_out,
        )
    }

    /// The same retained native clock, with sample-frame AHDSR pitch added
    /// before checked source-clock conversion (or the wavetable's alias clock).
    #[cfg(test)]
    pub fn positions_with_pitch(
        &mut self,
        lfos: &[PitchLfo],
        volume_lfos: &[VolumeLfo],
        rate: f32,
        tempo: f32,
        step: f64,
        envelope_pitch: Option<&[f32]>,
        wavetable: bool,
        out: &mut [u64],
        volume_out: Option<&mut [f32]>,
    ) -> Option<(u64, u64)> {
        self.positions_with_sources(lfos, volume_lfos, rate, tempo, step,
            envelope_pitch, None, None, None, wavetable, out, volume_out)
    }

    /// All native pitch sources share one destination and interpolator.
    /// Envelopes retain their existing sample-frame approximation separately.
    pub(super) fn positions_with_sources(
        &mut self,
        lfos: &[PitchLfo],
        volume_lfos: &[VolumeLfo],
        rate: f32,
        tempo: f32,
        step: f64,
        envelope_pitch: Option<&[f32]>,
        mut glide: Option<(&mut super::glide::Clock, &[crate::modulation::PitchGlide])>,
        shared: Option<View<'_>>,
        timings: Option<&[Timing; 16]>,
        wavetable: bool,
        out: &mut [u64],
        mut volume_out: Option<&mut [f32]>,
    ) -> Option<(u64, u64)> {
        let n = out.len();
        if n == 0 {
            return Some((0, 0));
        }
        self.sync_fades(lfos, volume_lfos, timings, rate, tempo);
        if let Some(view) = shared {
            if let Some(epoch) = view.epoch() {
                if self.shared_window != epoch {
                    self.shared_window = epoch;
                    self.shared_entry = view.now;
                    self.shared_entry_offset = self.offset;
                }
            }
        }
        // Source geometry is immutable for the note. Keep the existing
        // LFO-only arithmetic; Glide groups use native normalized octaves.
        let normalized = glide.as_ref().is_some_and(|(_, sources)| !sources.is_empty());
        let unit = if normalized { 1. } else { 12. };
        let active = lfos.iter().any(|l| !l.bypassed)
            || glide.as_ref().is_some_and(|(_, sources)| sources.iter().any(|s| !s.bypassed));
        let volume_active = volume_lfos.iter().any(|l| !l.source.bypassed);
        // A coefficient is prepared once per note/target, outside the retained
        // control-point loop. Bypassing never discards this target state.
        for lfo in volume_lfos.iter().filter(|l| !l.source.bypassed) {
            self.volume[usize::from(lfo.source.slot)].get_or_insert_with(|| {
                super::lfo_volume::Target::new(lfo.lag_ms, rate)
                    .expect("prepared nonnegative volume target lag")
            });
        }
        let mut position = 0u64;
        for (i, frame) in out.iter_mut().enumerate() {
            // Pitch and volume consume one native source waveform. Its fade
            // advances once at a control point, regardless of target count.
            let mut sampled_fades = [None; 16];
            let mut sampled_digital = [None; 16];
            let pitch = if active {
                // Voice planning fragments are not native control intervals.
                // Sample the source at the retained note-clock boundary, even
                // when a command or another loop voice split this render call.
                if self.offset == 0 || !self.initialized {
                    let mut point = 0.;
                    let mut source_points = [0.; 16];
                    for lfo in lfos.iter().filter(|l| !l.bypassed) {
                        if lfo.digital.is_some() {
                            let value = self.digital_point(lfo, rate, tempo, &mut sampled_digital, &mut sampled_fades, shared, timings, i)?
                                * (unit * lfo.depth);
                            if normalized { source_points[usize::from(lfo.slot)] = value; }
                            else { point += value; }
                            continue;
                        }
                        let hz = f64::from(Self::source_frequency(lfo, tempo, timings));
                        let phase = (f64::from(lfo.start_phase)
                            + self.phase[lfo.slot as usize]
                            + i as f64 * hz / f64::from(rate))
                        .rem_euclid(1.);
                        let mut gain = unit * lfo.depth * lfo.sine / lfo.sine.abs().max(1.);
                        gain *= self.source_fade(lfo, rate, &mut sampled_fades);
                        // Multi negates sine; ordinary sine is not admitted.
                        let value = -(phase * std::f64::consts::TAU).sin() as f32 * gain;
                        if normalized { source_points[usize::from(lfo.slot)] = value; }
                        else { point += value; }
                    }
                    if normalized {
                        if let Some((clock, sources)) = glide.as_mut() {
                            clock.points(sources, &mut source_points);
                        }
                        // Stable slot order; native prepared dispatch summation order is unproved.
                        point = source_points.into_iter().fold(0., |sum, value| sum + value);
                    }
                    self.previous = self.current;
                    self.current = point;
                    if !self.initialized {
                        self.previous = point;
                        self.initialized = true;
                    }
                }
                self.previous + (self.current - self.previous) * (f32::from(self.offset) / 32.)
            } else {
                0.
            };
            let volume = if volume_active {
                if self.offset == 0 || !self.volume_initialized {
                    let mut point = 1.;
                    for lfo in volume_lfos.iter().filter(|l| !l.source.bypassed) {
                        let source = &lfo.source;
                        let slot = usize::from(source.slot);
                        // Native Multi fades its final bipolar waveform before
                        // target range conversion and lag; no intermediate clamp.
                        let signal = if source.digital.is_some() {
                            self.digital_point(source, rate, tempo, &mut sampled_digital, &mut sampled_fades, shared, timings, i)?
                        } else {
                            let phase = (f64::from(source.start_phase)
                                + self.phase[slot]
                                + i as f64 * f64::from(Self::source_frequency(source, tempo, timings)) / f64::from(rate))
                            .rem_euclid(1.);
                            -(phase * std::f64::consts::TAU).sin() as f32 * source.sine
                                / source.sine.abs().max(1.)
                                * self.source_fade(source, rate, &mut sampled_fades)
                        };
                        let target = self.volume[slot].as_mut().expect("prepared volume target");
                        point = target.point(signal, point, lfo.intensity, lfo.negative);
                    }
                    self.volume_previous = self.volume_current;
                    self.volume_current = point.max(0.);
                    if !self.volume_initialized {
                        self.volume_previous = self.volume_current;
                        self.volume_initialized = true;
                    }
                }
                self.volume_previous
                    + (self.volume_current - self.volume_previous) * (f32::from(self.offset) / 32.)
            } else {
                1.
            };
            if let Some(output) = volume_out.as_deref_mut() {
                output[i] = volume;
            }
            *frame = position;
            let envelope = envelope_pitch.map_or(0., |p| p[i]);
            let octaves = if normalized { f64::from(pitch) + f64::from(envelope) / 12. }
                else { f64::from(pitch + envelope) / 12. };
            let ratio = if active || envelope_pitch.is_some() {
                2f64.powf(octaves)
            } else {
                1.
            };
            let increment = if wavetable {
                let step = step * ratio;
                if !step.is_finite() || step < 0. { return None; }
                super::wavetable::clock(step)
            } else {
                sample_clock(step * ratio)?
            };
            position = position.checked_add(increment)?;
            self.offset = (self.offset + 1) & 31;
        }
        for lfo in lfos.iter().filter(|l| !l.bypassed && l.digital.is_none()) {
            let phase = &mut self.phase[lfo.slot as usize];
            *phase = (*phase + f64::from(Self::source_frequency(lfo, tempo, timings)) * n as f64 / f64::from(rate))
                .rem_euclid(1.);
        }
        for lfo in volume_lfos.iter().filter(|l| !l.source.bypassed && l.source.digital.is_none()) {
            let source = &lfo.source;
            // A single native source can feed pitch and volume. Neither the
            // second target nor a copied planning clock advances it twice.
            if !lfos.iter().any(|l| l.slot == source.slot && !l.bypassed) {
                let phase = &mut self.phase[usize::from(source.slot)];
                *phase = (*phase + f64::from(Self::source_frequency(source, tempo, timings)) * n as f64 / f64::from(rate))
                    .rem_euclid(1.);
            }
        }
        Some((position, out.last().copied().unwrap_or(0)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsafe_lfo_ownership_is_rejected_by_both_bank_preparation_paths() {
        use crate::{engine::{Bank, GroupSettings}, import::{DigitalLfo, Group}};
        let valid = || {
            let source = PitchLfo { extended_fade: false, fade_sync: None, shared: false, digital: None, start_phase: 0.25, slot: 7,
                count: 1., note_value: 1. / 24., sine: 1., fade_ms: 40.,
                depth: 0.25, targets: vec![(2, 0.25)], bypassed: false };
            Group { pitch_lfos: vec![source.clone()], volume_lfos: vec![VolumeLfo {
                source, target: 4, intensity: 0.7, negative: false, lag_ms: 15 }],
                ..Default::default() }
        };
        let corrupt = |group: &mut Group, case| {
            if case >= 16 {
                let digital = Some(DigitalLfo { rectangle: 0.5, triangle: -0.25,
                    saw: 0.25, normalize: true, pulse_width: 0.5 });
                group.pitch_lfos[0].digital = digital.clone();
                group.volume_lfos[0].source.digital = digital;
            }
            match case {
                0 => group.pitch_lfos[0].slot = 16,
                1 => group.volume_lfos[0].source.slot = 16,
                2 => group.pitch_lfos.push(group.pitch_lfos[0].clone()),
                3 => group.volume_lfos.push(group.volume_lfos[0].clone()),
                4 => group.volume_lfos[0].source.start_phase = 0.75,
                5 => group.volume_lfos[0].source.count = 2.,
                6 => group.volume_lfos[0].source.note_value = 1. / 12.,
                7 => group.volume_lfos[0].source.sine = 0.5,
                8 => group.volume_lfos[0].source.fade_ms = 30.,
                9 => group.volume_lfos[0].source.bypassed = true,
                10 => group.volume_lfos[0].source.digital = Some(DigitalLfo {
                    rectangle: 0.5, triangle: 0., saw: 0., normalize: false, pulse_width: 0.5 }),
                11 => group.pitch_lfos[0].count = f32::NAN,
                12 => group.pitch_lfos[0].depth = f32::NAN,
                13 => group.pitch_lfos[0].targets[0].1 = f32::INFINITY,
                14 => group.volume_lfos[0].intensity = f32::NAN,
                15 => group.volume_lfos[0].lag_ms = -1,
                16 => group.pitch_lfos[0].start_phase = 1.25,
                17 => group.volume_lfos[0].source.digital.as_mut().unwrap().normalize = false,
                18 => group.pitch_lfos[0].digital.as_mut().unwrap().pulse_width = f32::NAN,
                19 => group.volume_lfos[0].source.digital.as_mut().unwrap().pulse_width = 0.75,
                20 => group.volume_lfos[0].source.digital.as_mut().unwrap().rectangle = -0.5,
                21 => group.volume_lfos[0].source.digital.as_mut().unwrap().triangle = 0.25,
                22 => group.volume_lfos[0].source.digital.as_mut().unwrap().saw = -0.25,
                23 => group.volume_lfos[0].source.extended_fade = true,
                24 => group.volume_lfos[0].source.fade_sync = Some(crate::modulation::LfoFadeSync { note_value: 0.125, count: 4. }),
                25 => group.pitch_lfos[0].fade_sync = Some(crate::modulation::LfoFadeSync { note_value: f32::NAN, count: 4. }),
                _ => unreachable!(),
            }
        };
        for case in 0..26 {
            let mut bad = valid(); corrupt(&mut bad, case);
            let imported = Bank::from_samples(vec![bad.clone()], Vec::new(), Vec::new())
                .err().expect("unsafe authored source must fail preparation");
            let prepared = Bank::from_samples(vec![valid()], Vec::new(), Vec::new()).unwrap()
                .with_prepared_settings(|rows| rows[0] = GroupSettings::from(&bad))
                .err().expect("unsafe consuming settings edit must fail before installation");
            let reason = match case {
                0 | 1 => "outside", 2 | 3 => "duplicate", 11 => "nonfinite source",
                12 | 13 => "nonfinite depth", 14 | 15 => "volume target",
                16 | 18 => "invalid phase or nonfinite waveform", _ => "inconsistent shared",
            };
            assert!(format!("{imported:#}").contains(reason), "case{case}: {imported:#}");
            assert!(format!("{prepared:#}").contains(reason), "case{case}: {prepared:#}");
        }
    }

    #[test]
    fn saved_volume_fade_shares_source_clock_in_complete_pcm_without_heap() {
        use crate::{audio::Sample, engine::{Bank, Engine, Expression, HostNote, HostPattern, load_scripts},
            import::{Group, Instrument, Modulator, Zone}};
        const OFF: usize = 2301;
        const END: usize = OFF + 2304;
        let owner = HostNote { port: 0, channel: 0, key: 60, id: 91, clap: true };
        let pattern = HostPattern { port: 0, channel: 0, key: 60, id: 91, clap: true };
        let source = PitchLfo { extended_fade: false, fade_sync: None, shared: false, digital: None, start_phase: 0.25, slot: 7, count: 1., note_value: 1. / 24.,
            sine: 1., fade_ms: 40., depth: 0.25, targets: vec![(2, 0.25)], bypassed: false };
        let create = |index| {
            let mut group = Group::default();
            if index < 5 {
                group.volume_lfos.push(VolumeLfo { source: source.clone(), target: 4,
                    intensity: 0.7, negative: false, lag_ms: 15 });
                if index < 3 { group.pitch_lfos.push(source.clone()); }
                group.modulators.resize_with(8, || Modulator { name: String::new(), targets: vec![],
                    assignments: None, volume_env: false, bypassed: false, flex: false, primary_flex: false,
                    envelope: None, kind: "lfo".into() });
                group.modulators[7].targets = vec!["Other0".into(), "Other1".into(), "Pitch".into(),
                    "Other3".into(), "Volume".into()];
            }
            let script = "on init\ndeclare ui_button $pause\nend on\non ui_control($pause)\nset_engine_par($ENGINE_PAR_INTMOD_BYPASS,$pause,0,7,-1)\nend on";
            let instrument = Instrument { groups: vec![group.clone()], scripts: vec![script.into()], ..Default::default() };
            // A stereo ramp/constant exposes pitch travel and volume separately.
            // The independent plain engine supplies only the amplifier trajectory.
            let frames = (0..8192).map(|at| [0.125 + if index < 5 { at as f32 / 16384. } else { 0. }, 0.125]).collect();
            let mut bank = Bank::from_samples(vec![group], vec![Zone::default()],
                vec![(Default::default(), Sample { rate: 48_000, frames })]).unwrap();
            if index == 1 {
                // A consuming worker edit reaches the first complete PCM render.
                // Equivalent legacy phase/frequency/normalized sine remain legal;
                // the unused source-copy depth/targets do not bind volume to pitch.
                bank = bank.with_prepared_settings(|rows| {
                    let group = &mut rows[0];
                    for source in [&mut group.pitch_lfos[0], &mut group.volume_lfos[0].source] {
                        source.start_phase = 1.25; source.count = 1.;
                        source.note_value = 1. / 24.; source.sine = 3.;
                    }
                    group.volume_lfos[0].source.depth = -7.5;
                    group.volume_lfos[0].source.targets = vec![(99, 0.6)];
                }).unwrap();
            }
            let mut engine = Engine::default(); engine.reset(48_000.); engine.attack = 0.; engine.release = 0.005;
            engine.player.shared = index != 2; engine.set_bank(Some(Box::new(bank)));
            if index < 5 {
                let (runtime, errors) = load_scripts(&instrument, vec![], 48_000.);
                assert!(errors.is_empty(), "{errors:?}"); engine.set_script(runtime);
                assert_eq!(engine.settings().unwrap()[0].volume_lfos[0].source.fade_ms, 40.);
            }
            if index == 1 {
                let group = &engine.settings().unwrap()[0];
                assert_eq!(group.pitch_lfos[0].start_phase, 1.25);
                assert_eq!(group.pitch_lfos[0].sine, 3.);
                assert_eq!(group.pitch_lfos[0].depth, 0.25);
                assert_eq!(group.volume_lfos[0].source.depth, -7.5);
                assert_eq!(group.volume_lfos[0].lag_ms, 15);
            }
            engine
        };
        let mut engines: [Engine; 6] = std::array::from_fn(create);
        let mut pcm: [[Vec<f32>; 2]; 6] = std::array::from_fn(|_| std::array::from_fn(|_| vec![0.; END]));
        let mut ends = [None; 6];
        let mut replay = [[[0.; 128]; 2]; 5];
        let mut exercise = || {
            for engine in &mut engines { assert!(engine.host_note_on(owner, 0, 60, 127, 0.)); }
            for (stage, range) in [0..141, 141..205, 205..229, 229..617, 617..OFF, OFF..END].into_iter().enumerate() {
                for (index, engine) in engines.iter_mut().enumerate() {
                    if index < 5 && matches!(stage, 1 | 2) { engine.ui_control(0, 0, i32::from(stage == 1)); }
                    match stage {
                        3 => engine.player.midi.set_expression(0, 60, Expression { gain: 0., ..Default::default() }),
                        4 => engine.player.midi.set_expression(0, 60, Expression::default()),
                        5 => engine.host_note_off(pattern),
                        _ => {},
                    }
                    engine.render_checked(&mut [], &mut []).unwrap();
                    let sizes: &[usize] = if matches!(index, 1 | 2 | 4) { &[1, 7, 31, 128] } else { &[128] };
                    let (mut at, mut fragment) = (range.start, 0);
                    while at < range.end {
                        let n = sizes[fragment % sizes.len()].min(range.end - at);
                        let [left, right] = &mut pcm[index];
                        engine.render_checked(&mut left[at..at + n], &mut right[at..at + n]).unwrap();
                        at += n; fragment += 1;
                    }
                    if stage < 5 { assert_eq!(engine.active_voices(), 1); }
                    else {
                        assert_eq!(engine.active_voices(), 0); engine.mark_host_notes();
                        ends[index] = engine.host_note_end_at(owner);
                    }
                }
            }
            for (index, engine) in engines[..5].iter_mut().enumerate() {
                assert!(engine.host_note_on(HostNote { id: 92, ..owner }, 0, 60, 127, 0.));
                let [left, right] = &mut replay[index];
                engine.render_checked(left, right).unwrap();
                engine.host_note_off(HostPattern { id: 92, ..pattern });
                for _ in 0..20 {
                    let (mut left, mut right) = ([0.; 128], [0.; 128]);
                    engine.render_checked(&mut left, &mut right).unwrap();
                }
                assert_eq!(engine.active_voices(), 0);
            }
        };
        #[cfg(feature = "plugin")]
        assert_eq!(crate::plugin::tests::allocations(|| exercise()), 0);
        #[cfg(not(feature = "plugin"))]
        exercise();
        // Independent closed form, never calling Fade/Clock/Target. The native
        // legacy law has exactly60 scaled control points, then passes through.
        let start = f64::from(0.3_f32);
        let alpha = 1. - (-(2. * std::f64::consts::LN_10) / (15. * 48_000. / 32000.)).exp();
        let (mut active_frames, mut ticks, mut position) = (0usize, 0usize, 0u64);
        let (mut previous, mut current, mut target) = ((0., 1.), (0., 1.), 0.5_f64);
        let (mut oracle_error, mut partition, mut shared_error) = (0_f64, 0_f32, 0_f32);
        for at in 0..END {
            let bypass = (141..205).contains(&at);
            if !bypass && at % 32 == 0 {
                let fade = if ticks < 60 { (start * (1. + 1. / start).powf(ticks as f64 / 60.)).min(1.) - start } else { 1. };
                let signal = -(std::f64::consts::TAU * (0.25 + active_frames as f64 * 48. / 48_000.)).sin() * fade;
                let desired = (signal + 1.) * 0.5;
                if ticks == 0 { target = desired; }
                else if (desired - target).abs() >= 1e-5 { target += alpha * (desired - target); }
                previous = current; current = (signal * 3., 1. - (1. - target) * f64::from(0.7_f32));
                if ticks == 0 { previous = current; }
                ticks += 1;
            }
            let fraction = (at % 32) as f64 / 32.;
            let pitch = if bypass { 0. } else { previous.0 + (current.0 - previous.0) * fraction };
            let gain = if bypass { 1. } else { previous.1 + (current.1 - previous.1) * fraction };
            let amplitude = f64::from(pcm[5][1][at]) / 0.125;
            let expected_left = amplitude * (0.125 + position as f64 / FIXED_ONE / 16384.) * gain;
            let expected_right = f64::from(pcm[5][1][at]) * gain;
            for index in 0..3 {
                oracle_error = oracle_error.max((f64::from(pcm[index][0][at]) - expected_left).abs());
                oracle_error = oracle_error.max((f64::from(pcm[index][1][at]) - expected_right).abs());
                for channel in 0..2 { partition = partition.max((pcm[index][channel][at] - pcm[0][channel][at]).abs()); }
            }
            for index in 3..5 {
                oracle_error = oracle_error.max((f64::from(pcm[index][1][at]) - expected_right).abs());
                shared_error = shared_error.max((pcm[index][1][at] - pcm[0][1][at]).abs());
            }
            for channel in 0..2 { partition = partition.max((pcm[3][channel][at] - pcm[4][channel][at]).abs()); }
            position += (2f64.powf(pitch / 12.) * FIXED_ONE) as u64;
            if !bypass { active_frames += 1; }
        }
        assert!(oracle_error < 2e-5, "faded final PCM vs independent source/target law: {oracle_error}");
        assert!(partition < 1e-6, "fixed/irregular/shared PCM: {partition}");
        assert!(shared_error < 1e-6, "the second target must not advance source fade twice: {shared_error}");
        assert!(ends[0].is_some()); for end in &ends { assert_eq!(end, &ends[0]); }
        assert!((pcm[0][1][64] / pcm[5][1][64] - 0.65).abs() < 0.02,
            "fade starts the bipolar waveform at zero before unipolar volume conversion");
        assert!(pcm[0][1][64] > 0.01, "the initial faded waveform reaches audible final PCM");
        for (index, engine) in engines.iter().enumerate() {
            assert_eq!(engine.dropped_commands(), 0);
            for channel in 0..2 {
                assert!(pcm[index][channel][469..617].iter().all(|&value| value == 0.));
                assert!(pcm[index][channel][END - 128..].iter().all(|&value| value == 0.));
                if index < 5 { assert!(replay[index][channel].iter().zip(&pcm[index][channel])
                    .all(|(a, b)| (a - b).abs() < 1e-6), "retrigger resets phase/fade/lag"); }
            }
        }
        eprintln!("volume source fade PCM: oracle={oracle_error}, partition={partition}, shared={shared_error}, end={:?}", ends[0]);
    }

    #[test]
    fn saved_digital_source_reaches_complete_pcm_with_independent_endpoints_without_heap() {
        for normalize in [false, true] { for fade_ms in [0., 40.] {
        use crate::{audio::Sample, engine::{Bank, Engine, Expression, HostNote, HostPattern, load_scripts},
            import::{DigitalLfo, Group, Instrument, Modulator, Zone}};
        const OFF: usize = 2301;
        const END: usize = OFF + 2304;
        let owner = HostNote { port: 0, channel: 0, key: 60, id: 91, clap: true };
        let pattern = HostPattern { port: 0, channel: 0, key: 60, id: 91, clap: true };
        let source = PitchLfo { extended_fade: false, fade_sync: None, shared: false, digital: Some(DigitalLfo { rectangle: 0.75, triangle: -0.5, saw: 0.5,
            normalize, pulse_width: 0.5 }), start_phase: 1., slot: 7, count: 1., note_value: 1. / 64.,
            sine: 0.25, fade_ms, depth: 0.125, targets: vec![(2, 0.125)], bypassed: false };
        let create = |index| {
            let mut group = Group::default();
            if index < 5 {
                group.volume_lfos.push(VolumeLfo { source: source.clone(), target: 4,
                    intensity: 0.7, negative: false, lag_ms: 15 });
                if index < 3 { group.pitch_lfos.push(source.clone()); }
                group.modulators.resize_with(8, || Modulator { name: String::new(), targets: vec![],
                    assignments: None, volume_env: false, bypassed: false, flex: false, primary_flex: false,
                    envelope: None, kind: "lfo".into() });
                group.modulators[7].targets = vec!["Other0".into(), "Other1".into(), "Pitch".into(),
                    "Other3".into(), "Volume".into()];
            }
            let script = "on init\ndeclare ui_button $pause\nend on\non ui_control($pause)\nset_engine_par($ENGINE_PAR_INTMOD_BYPASS,$pause,0,7,-1)\nend on\non controller\nif ($CC_NUM = 17)\nset_engine_par($ENGINE_PAR_INTMOD_INTENSITY,500000,0,7,2)\nend if\nif ($CC_NUM = 18)\nset_engine_par($ENGINE_PAR_INTMOD_INTENSITY,750000,0,7,2)\nend if\nend on";
            let instrument = Instrument { groups: vec![group.clone()], scripts: vec![script.into()], ..Default::default() };
            // A stereo ramp/constant exposes pitch travel and volume separately.
            // The independent plain engine supplies only the amplifier trajectory.
            let frames = (0..8192).map(|at| [0.125 + if index < 5 { at as f32 / 16384. } else { 0. }, 0.125]).collect();
            let bank = Bank::from_samples(vec![group], vec![Zone::default()],
                vec![(Default::default(), Sample { rate: 32_768, frames })]).unwrap();
            let mut engine = Engine::default(); engine.reset(32_768.); engine.attack = 0.; engine.release = 0.005;
            engine.player.shared = index != 2; engine.set_bank(Some(Box::new(bank)));
            if index < 5 {
                let (runtime, errors) = load_scripts(&instrument, vec![], 32_768.);
                assert!(errors.is_empty(), "{errors:?}"); engine.set_script(runtime);
                assert_eq!(engine.settings().unwrap()[0].volume_lfos[0].source, source);
            }
            engine
        };
        let mut engines: [Engine; 6] = std::array::from_fn(create);
        let mut pcm: [[Vec<f32>; 2]; 6] = std::array::from_fn(|_| std::array::from_fn(|_| vec![0.; END]));
        let mut ends = [None; 6];
        let mut replay = [[[0.; 128]; 2]; 5];
        let mut exercise = || {
            for engine in &mut engines { assert!(engine.host_note_on(owner, 0, 60, 127, 0.)); }
            for (stage, range) in [0..141, 141..205, 205..229, 229..617, 617..OFF, OFF..END].into_iter().enumerate() {
                for (index, engine) in engines.iter_mut().enumerate() {
                    if index < 5 && matches!(stage, 1 | 2) { engine.ui_control(0, 0, i32::from(stage == 1)); }
                    match stage {
                        3 => engine.player.midi.set_expression(0, 60, Expression { gain: 0., ..Default::default() }),
                        4 => { engine.player.midi.set_expression(0, 60, Expression::default());
                            if index < 3 { engine.cc(0, 17, 127); } },
                        5 => engine.host_note_off(pattern),
                        _ => {},
                    }
                    engine.render_checked(&mut [], &mut []).unwrap();
                    let sizes: &[usize] = if matches!(index, 1 | 2 | 4) { &[1, 7, 31, 127] } else { &[128] };
                    let (mut at, mut fragment) = (range.start, 0);
                    while at < range.end {
                        let n = sizes[fragment % sizes.len()].min(range.end - at);
                        let [left, right] = &mut pcm[index];
                        engine.render_checked(&mut left[at..at + n], &mut right[at..at + n]).unwrap();
                        at += n; fragment += 1;
                    }
                    if stage < 5 { assert_eq!(engine.active_voices(), 1); }
                    else {
                        assert_eq!(engine.active_voices(), 0); engine.mark_host_notes();
                        ends[index] = engine.host_note_end_at(owner);
                    }
                }
            }
            for (index, engine) in engines[..5].iter_mut().enumerate() {
                if index < 3 { engine.cc(0, 18, 127); }
                assert!(engine.host_note_on(HostNote { id: 92, ..owner }, 0, 60, 127, 0.));
                let [left, right] = &mut replay[index];
                engine.render_checked(left, right).unwrap();
                engine.host_note_off(HostPattern { id: 92, ..pattern });
                for _ in 0..20 {
                    let (mut left, mut right) = ([0.; 128], [0.; 128]);
                    engine.render_checked(&mut left, &mut right).unwrap();
                }
                assert_eq!(engine.active_voices(), 0);
            }
        };
        #[cfg(feature = "plugin")]
        assert_eq!(crate::plugin::tests::allocations(|| exercise()), 0);
        #[cfg(not(feature = "plugin"))]
        exercise();
        // Independent closed form, never calling Fade/Clock/Target. The native
        // Digital arithmetic uses exact eighth-cycle points at32768Hz/128Hz.
        // The oracle never calls the production source, fade, or target helpers.
        let start = f64::from(0.3_f32);
        let alpha = 1. - (-(2. * std::f64::consts::LN_10) / (15. * 32_768. / 32000.)).exp();
        let scaled_points = (fade_ms * 1.024) as usize;
        let mut phase = 1_f64;
        let (mut ticks, mut position) = (0usize, 0u64);
        let (mut previous, mut current, mut target) = ((0., 1.), (0., 1.), 0.5_f64);
        let (mut oracle_error, mut partition, mut shared_error) = (0_f64, 0_f32, 0_f32);
        for at in 0..END {
            let bypass = (141..205).contains(&at);
            if !bypass && at % 32 == 0 {
                let fade = if ticks < scaled_points { (start * (1. + 1. / start).powf(ticks as f64 / scaled_points as f64)).min(1.) - start } else { 1. };
                // Recorded eighth-cycle arithmetic shapes, including endpoint1.
                // Saw/triangle use a table of independently evaluated points;
                // the pulse's equality case is the positive branch.
                let point = (phase * 8.).round() as usize;
                let saw = [-1., -0.5, 0., 0.5, 1., -0.5, 0., 0.5, 1.][point];
                let triangle = [-1., -0.5, 0., 0.5, 1., 0.5, 0., -0.5, -1.][point];
                let rectangle = if point < 4 { -0.75 } else { 0.75 };
                let waveform = -(phase * std::f64::consts::TAU).sin() * 0.25
                    + saw * 0.5 + triangle * -0.5 + rectangle;
                let signal = waveform * if normalize { 0.5 } else { 1. } * fade;
                phase += 0.125; if phase > 1. { phase -= 1.; }
                let desired = (signal + 1.) * 0.5;
                if ticks == 0 { target = desired; }
                else if (desired - target).abs() >= 1e-5 { target += alpha * (desired - target); }
                previous = current; current = (signal * if at >= 617 { 0. } else { 1.5 },
                    (1. - (1. - target) * f64::from(0.7_f32)).max(0.));
                if ticks == 0 { previous = current; }
                ticks += 1;
            }
            let fraction = (at % 32) as f64 / 32.;
            let pitch = if bypass { 0. } else { previous.0 + (current.0 - previous.0) * fraction };
            let gain = if bypass { 1. } else { previous.1 + (current.1 - previous.1) * fraction };
            let amplitude = f64::from(pcm[5][1][at]) / 0.125;
            let expected_left = amplitude * (0.125 + position as f64 / FIXED_ONE / 16384.) * gain;
            let expected_right = f64::from(pcm[5][1][at]) * gain;
            for index in 0..3 {
                oracle_error = oracle_error.max((f64::from(pcm[index][0][at]) - expected_left).abs());
                oracle_error = oracle_error.max((f64::from(pcm[index][1][at]) - expected_right).abs());
                for channel in 0..2 { partition = partition.max((pcm[index][channel][at] - pcm[0][channel][at]).abs()); }
            }
            for index in 3..5 {
                oracle_error = oracle_error.max((f64::from(pcm[index][1][at]) - expected_right).abs());
                shared_error = shared_error.max((pcm[index][1][at] - pcm[0][1][at]).abs());
            }
            for channel in 0..2 { partition = partition.max((pcm[3][channel][at] - pcm[4][channel][at]).abs()); }
            position += (2f64.powf(pitch / 12.) * FIXED_ONE) as u64;
        }
        assert!(oracle_error < 2e-5, "faded final PCM vs independent source/target law: {oracle_error}");
        assert!(partition < 1e-6, "fixed/irregular/shared PCM: {partition}");
        assert!(shared_error < 1e-6, "the second target must not advance source fade twice: {shared_error}");
        // The ordinary amplifier reaches -60 dB in the authored 5 ms;
        // its relative -80 dB terminal consumes ceil(163.84 * 80/60)
        // = 219 frames. The 8192-frame source cannot reach EOF here.
        for end in &ends { assert_eq!(*end, Some((OFF + 219) as u64)); }
        let expected_initial = if fade_ms > 0. { 0.65 } else {
            1. - (1. - (1. + 1.75 * if normalize { 0.5 } else { 1. }) * 0.5) * f64::from(0.7_f32)
        };
        assert!((f64::from(pcm[0][1][16] / pcm[5][1][16]) - expected_initial).abs() < 2e-6,
            "saved endpoint1 reaches audible PCM before the next source point");
        assert!(pcm[0][1][64] > 0.01, "the initial faded waveform reaches audible final PCM");
        for (index, engine) in engines.iter().enumerate() {
            assert_eq!(engine.dropped_commands(), 0);
            for channel in 0..2 {
                assert!(pcm[index][channel][469..617].iter().all(|&value| value == 0.));
                assert!(pcm[index][channel][END - 128..].iter().all(|&value| value == 0.));
                if index < 5 { assert!(replay[index][channel].iter().zip(&pcm[index][channel])
                    .all(|(a, b)| (a - b).abs() < 1e-6), "retrigger resets phase/fade/lag"); }
            }
        }
        if normalize && fade_ms == 0. {
            // Both planning topologies must report unsupported source geometry
            // before any PCM; low host rates are public, not globally clamped.
            for pitch_only in [true, false] {
                let mut group = Group::default();
                if pitch_only { group.pitch_lfos.push(source.clone()); }
                else { group.volume_lfos.push(VolumeLfo { source: source.clone(), target: 4,
                    intensity: 0.7, negative: false, lag_ms: 15 }); }
                let bank = Bank::from_samples(vec![group], vec![Zone::default()],
                    vec![(Default::default(), Sample { rate: 1024, frames: vec![[0.25; 2]; 4096] })]).unwrap();
                let mut engine = Engine::default(); engine.reset(1024.);
                engine.set_bank(Some(Box::new(bank)));
                let failed_at = engine.player.now;
                let (mut left, mut right) = ([1.; 128], [1.; 128]);
                let mut error = None;
                let mut fail = || {
                    assert!(engine.host_note_on(owner, 0, 60, 127, 0.));
                    engine.render_checked(&mut [], &mut []).unwrap();
                    error = engine.render_checked(&mut left, &mut right).err();
                    engine.mark_host_notes();
                    assert_eq!(engine.host_note_end_at(owner), Some(failed_at));
                };
                #[cfg(feature = "plugin")]
                assert_eq!(crate::plugin::tests::allocations(|| fail()), 0);
                #[cfg(not(feature = "plugin"))]
                fail();
                assert_eq!(error.unwrap().kind, crate::engine::StreamErrorKind::UnrepresentablePitch);
                assert_eq!((left, right), ([0.; 128], [0.; 128]));
                assert_eq!(engine.active_voices(), 0);
            }
        }
        eprintln!("Digital PCM normalize={normalize} fade={fade_ms}: oracle={oracle_error}, partition={partition}, shared={shared_error}, end={:?}", ends[0]);
        } }
    }

    #[test]
    fn synced_lfo_pcm_keeps_each_note_clock_through_mid_tick_tempo_changes() {
        use crate::{
            audio::Sample,
            engine::{Bank, Engine, NoteEvent},
            import::{Group, Zone},
        };
        const FRAMES: usize = 3501;
        const STAGES: [(usize, f64); 5] =
            [(0, 120.), (229, 60.), (617, 180.), (941, 90.), (1753, 120.)];
        const ONSETS: [usize; 2] = [0, 617];

        // A linear resident sample makes pitch travel audible and independently
        // calculable: cubic interpolation reproduces its straight line. The
        // constant-source engine supplies only the ordinary amplifier law.
        let create = |rate, modulated| {
            let source = PitchLfo { extended_fade: false, fade_sync: None, shared: false, digital: None,
                start_phase: 0.875,
                slot: 7,
                count: 1.,
                note_value: 1. / 24.,
                sine: 1.,
                fade_ms: 0.,
                depth: 0.25,
                targets: vec![(2, 0.25)],
                bypassed: false,
            };
            let group = if modulated {
                Group {
                    pitch_lfos: vec![source.clone()],
                    volume_lfos: vec![VolumeLfo {
                        source,
                        target: 4,
                        intensity: 0.7,
                        negative: false,
                        lag_ms: 15,
                    }],
                    ..Default::default()
                }
            } else {
                Group::default()
            };
            let frames = (0..8192)
                .map(|i| [0.125 + if modulated { i as f32 / 16384. } else { 0. }; 2])
                .collect();
            let bank = Bank::from_samples(
                vec![group],
                vec![Zone::default()],
                vec![(Default::default(), Sample { rate, frames })],
            )
            .unwrap();
            let mut engine = Engine::default();
            engine.reset(f64::from(rate));
            engine.attack = 0.;
            engine.set_bank(Some(Box::new(bank)));
            engine
        };

        // Source phase is the analytic integral of accepted tempo segments
        // since this note's onset. No production Clock/Target helpers are used.
        let oracle = |onset: usize, rate: u32, synced: bool| {
            let phase = |at: usize| {
                let mut cycles = 0.875;
                for (i, &(start, bpm)) in STAGES.iter().enumerate() {
                    let end = STAGES.get(i + 1).map_or(FRAMES, |s| s.0).min(at);
                    let start = start.max(onset);
                    if end > start {
                        // count1 times note_value1/24 gives a 2.5 denominator.
                        cycles += (end - start) as f64 * if synced { bpm / 2.5 } else { 48. }
                            / f64::from(rate);
                    }
                }
                cycles
            };
            let signal = |at| -(phase(at) * std::f64::consts::TAU).sin();
            let alpha =
                1. - (-(2. * std::f64::consts::LN_10) / (15. * f64::from(rate) / 32000.)).exp();
            let points = (FRAMES - onset).div_ceil(32);
            let mut volume = (signal(onset) + 1.) * 0.5;
            let mut controls = Vec::with_capacity(points);
            for tick in 0..points {
                let source = signal(onset + tick * 32);
                if tick > 0 {
                    let delta = (source + 1.) * 0.5 - volume;
                    // This is the authored native target's finite deadband,
                    // independently covered by lfo_volume's scalar fixtures.
                    if delta.abs() >= 1e-5 {
                        volume += alpha * delta;
                    }
                }
                controls.push((source * 3., 1. - (1. - volume) * f64::from(0.7_f32)));
            }
            let mut position = 0u64;
            (0..FRAMES - onset)
                .map(|frame| {
                    let tick = frame / 32;
                    let previous = controls[tick.saturating_sub(1)];
                    let current = controls[tick];
                    let fraction = (frame % 32) as f64 / 32.;
                    let pitch = previous.0 + (current.0 - previous.0) * fraction;
                    let gain = previous.1 + (current.1 - previous.1) * fraction;
                    let pcm = (0.125 + position as f64 / FIXED_ONE / 16384.) * gain;
                    position += (2f64.powf(pitch / 12.) * FIXED_ONE) as u64;
                    pcm
                })
                .collect::<Vec<_>>()
        };

        for rate in [48_000, 96_000] {
            let (mut whole, mut split, mut plain) =
                (create(rate, true), create(rate, true), create(rate, false));
            let (mut a, mut b, mut baseline) =
                ([[0.; 2]; FRAMES], [[0.; 2]; FRAMES], [[0.; 2]; FRAMES]);
            let mut exercise = || {
                for (stage, &(start, bpm)) in STAGES.iter().enumerate() {
                    let end = STAGES.get(stage + 1).map_or(FRAMES, |s| s.0);
                    for engine in [&mut whole, &mut split, &mut plain] {
                        engine.set_transport(
                            stage % 2 == 0,
                            bpm,
                            start as f64 / f64::from(rate),
                            (4, 4),
                        );
                        if let Some(channel) = ONSETS.iter().position(|onset| *onset == start) {
                            engine.start_event(&NoteEvent {
                                pan: if channel == 0 { -1. } else { 1. },
                                ..NoteEvent::new(channel as u8, 60, 127)
                            });
                        }
                        // Empty fragments must not consume phase or control ticks.
                        engine.render(&mut [], &mut []);
                    }
                    for (engine, pcm, sizes) in [
                        (&mut whole, &mut a, &[128][..]),
                        (&mut split, &mut b, &[1, 7, 31, 128][..]),
                        (&mut plain, &mut baseline, &[128][..]),
                    ] {
                        let mut at = start;
                        let mut block = 0;
                        while at < end {
                            let n = sizes[block % sizes.len()].min(end - at);
                            let (mut left, mut right) = ([0.; 128], [0.; 128]);
                            engine.render(&mut left[..n], &mut right[..n]);
                            for i in 0..n {
                                pcm[at + i] = [left[i], right[i]];
                            }
                            at += n;
                            block += 1;
                        }
                    }
                }
            };
            #[cfg(feature = "plugin")]
            assert_eq!(crate::plugin::tests::allocations(|| exercise()), 0);
            #[cfg(not(feature = "plugin"))]
            exercise();
            let partition_error = a
                .iter()
                .flatten()
                .zip(b.iter().flatten())
                .map(|(a, b)| (a - b).abs())
                .fold(0_f32, f32::max);
            let (mut oracle_error, mut unsynced_error) = (0_f64, 0_f64);
            for (channel, onset) in ONSETS.into_iter().enumerate() {
                let expected = oracle(onset, rate, true);
                let unsynced = oracle(onset, rate, false);
                for (frame, (expected, unsynced)) in expected.into_iter().zip(unsynced).enumerate()
                {
                    let at = onset + frame;
                    let amplitude = f64::from(baseline[at][channel]) / 0.125;
                    oracle_error =
                        oracle_error.max((f64::from(a[at][channel]) - amplitude * expected).abs());
                    unsynced_error = unsynced_error
                        .max((f64::from(a[at][channel]) - amplitude * unsynced).abs());
                }
            }
            eprintln!(
                "synced shared LFO PCM rate={rate}: partition={partition_error}, oracle={oracle_error}, unsynced={unsynced_error}"
            );
            assert!(
                partition_error < 1e-7 && oracle_error < 2e-7,
                "each note must retain phase, pitch travel and native target lag through tempo changes"
            );
            assert!(
                unsynced_error > 0.01,
                "the PCM oracle must distinguish a source ignoring tempo changes"
            );
        }
    }

    #[test]
    fn saved_sine_volume_shares_clock_lag_and_fragmented_interpolation() {
        for start_phase in [0., 0.25, 0.4990234375, 0.5, 0.5009765625, 0.75, 1.] {
            saved_sine_volume_shares_clock_lag_and_fragmented_interpolation_at_phase(start_phase);
        }
    }

    fn saved_sine_volume_shares_clock_lag_and_fragmented_interpolation_at_phase(start_phase: f32) {
        let source = PitchLfo { extended_fade: false, fade_sync: None, shared: false, digital: None,
            start_phase,
            slot: 7,
            count: 16.,
            note_value: 1. / 24.,
            sine: 1.,
            fade_ms: 0.,
            depth: 0.25,
            targets: vec![(2, 0.25)],
            bypassed: false,
        };
        let volume = VolumeLfo {
            source: source.clone(),
            target: 4,
            intensity: 1.,
            negative: false,
            lag_ms: 15,
        };
        let mut only = Clock::default();
        let mut shared = Clock::default();
        let mut pitch = Clock::default();
        let mut split = Clock::default();
        let (mut a, mut b, mut c) = ([0.; 512], [0.; 512], [0.; 512]);
        for at in (0..512).step_by(128) {
            let (mut positions, mut shared_positions, mut pitch_positions) =
                ([0; 128], [0; 128], [0; 128]);
            only.positions(
                &[],
                std::slice::from_ref(&volume),
                48_000.,
                120.,
                1.,
                &mut positions,
                Some(&mut a[at..at + 128]),
            ).unwrap();
            shared.positions(
                std::slice::from_ref(&source),
                std::slice::from_ref(&volume),
                48_000.,
                120.,
                1.,
                &mut shared_positions,
                Some(&mut b[at..at + 128]),
            ).unwrap();
            pitch.positions(
                std::slice::from_ref(&source),
                &[],
                48_000.,
                120.,
                1.,
                &mut pitch_positions,
                None,
            ).unwrap();
            if at == 0 {
                let semitones = -(std::f64::consts::TAU * f64::from(start_phase)).sin() as f32 * 3.;
                assert_eq!(
                    pitch_positions[1],
                    (2f64.powf(f64::from(semitones) / 12.) * FIXED_ONE) as u64,
                    "the saved cycle position reaches the physical pitch step"
                );
            }
            assert_eq!(
                shared_positions, pitch_positions,
                "volume cannot advance pitch a second time"
            );
            for (from, to) in [(at, at + 17), (at + 17, at + 128)] {
                split.positions(
                    std::slice::from_ref(&source),
                    std::slice::from_ref(&volume),
                    48_000.,
                    120.,
                    1.,
                    &mut positions[..to - from],
                    Some(&mut c[from..to]),
                ).unwrap();
            }
        }
        assert_eq!(
            a, b,
            "adding a pitch destination cannot advance the volume source twice"
        );
        assert!(a.iter().zip(c).all(|(a, c)| (a - c).abs() < 1e-7));
        assert!((shared.phase[7] - 3. * 512. / 48_000.).abs() < 1e-8);
        // Independent native control-point reference, including the saved cycle
        // position and the positive15ms coefficient at the next32-frame point.
        let initial = (1. - (std::f64::consts::TAU * f64::from(start_phase)).sin() as f32) * 0.5;
        let signal =
            -(std::f64::consts::TAU * (f64::from(start_phase) + 3. * 32. / 48_000.)).sin() as f32;
        let next = initial + ((signal + 1.) * 0.5 - initial) * 0.18508725;
        assert!((a[0] - initial).abs() < 1e-7);
        assert!((a[32] - initial).abs() < 1e-7);
        assert!((a[64] - next).abs() < 1e-7);
        assert!((a[48] - (initial + (next - initial) * 0.5)).abs() < 1e-7);

        let mut paused = only;
        let mut reference = only;
        let mut bypass = volume.clone();
        bypass.source.bypassed = true;
        let phase = paused.phase;
        let (mut positions, mut unity) = ([0; 64], [0.; 64]);
        paused.positions(
            &[],
            &[bypass],
            48_000.,
            120.,
            1.,
            &mut positions,
            Some(&mut unity),
        ).unwrap();
        assert_eq!(unity, [1.; 64]);
        assert_eq!(
            paused.phase, phase,
            "bypass pauses the source, retaining lag state"
        );
        reference.skip_bypassed(64);
        let (mut resumed, mut expected) = ([0.; 64], [0.; 64]);
        paused.positions(
            &[],
            std::slice::from_ref(&volume),
            48_000.,
            120.,
            1.,
            &mut positions,
            Some(&mut resumed),
        ).unwrap();
        reference.positions(
            &[],
            std::slice::from_ref(&volume),
            48_000.,
            120.,
            1.,
            &mut positions,
            Some(&mut expected),
        ).unwrap();
        assert_eq!(resumed, expected, "resume cannot restart the target lag");
        let mut zero = volume;
        zero.intensity = 0.;
        let mut clock = Clock::default();
        clock.positions(
            &[],
            std::slice::from_ref(&zero),
            48_000.,
            120.,
            1.,
            &mut positions,
            Some(&mut unity),
        ).unwrap();
        assert_eq!(unity, [1.; 64]);

        // Partial source bypass keeps the other source advancing while the
        // paused source retains its own phase/lag and aggregate interpolation.
        let mut first = zero.clone();
        first.intensity = 1.;
        let mut second = first.clone();
        second.source.slot = 3;
        second.source.count = 24.;
        second.intensity = 0.4;
        second.negative = true;
        let mut whole = Clock::default();
        let mut fragmented = Clock::default();
        let mut sources = [first, second];
        let (mut x, mut y) = ([0.; 128], [0.; 128]);
        let mut positions = [0; 128];
        for (n, bypass) in [(13, false), (73, true), (111, false)] {
            sources[0].source.bypassed = bypass;
            whole.positions(
                &[],
                &sources,
                48_000.,
                120.,
                1.,
                &mut positions[..n],
                Some(&mut x[..n]),
            ).unwrap();
            let mut at = 0;
            while at < n {
                let end = (at + 17).min(n);
                fragmented.positions(
                    &[],
                    &sources,
                    48_000.,
                    120.,
                    1.,
                    &mut positions[..end - at],
                    Some(&mut y[at..end]),
                ).unwrap();
                at = end;
            }
            assert!(
                x[..n]
                    .iter()
                    .zip(&y[..n])
                    .all(|(x, y)| (x - y).abs() < 1e-7)
            );
        }
        assert!((whole.phase[7] - 3. * 124. / 48_000.).abs() < 1e-8);
        assert!((whole.phase[3] - 2. * 197. / 48_000.).abs() < 1e-8);
    }
}
