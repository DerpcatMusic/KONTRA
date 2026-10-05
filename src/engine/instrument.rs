//! Whole-instrument controls shared by script initialization and playback.

use super::params::GroupPar;

/// Native program-container note history, shared across its channels and groups.
/// Key-up, MIDI reset, and bank replacement retain this owner.
pub(super) struct NoteHistory {
    prior: i16,
    current: i16,
}

impl Default for NoteHistory {
    fn default() -> Self { Self { prior: -1, current: -1 } }
}

impl NoteHistory {
    pub(super) fn observe(&mut self, note: u8) {
        if note >= 128 { return; }
        let note = i16::from(note);
        self.prior = if self.prior < 0 { note } else { self.current };
        self.current = note;
    }

    pub(super) fn interval(&self) -> i16 { self.current - self.prior }
}

/// Runtime instrument controls, separate from MIDI channel and part controls.
/// Imported program values seed these same writable controls.
pub(super) use crate::import::ProgramControls as InstrumentControls;

/// The installed engine owns these mutable parameter layers; prepared banks
/// carry them only until handoff (and again when retired off the audio thread).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum ControllerPolicy {
    #[default]
    Compatibility,
    Native,
}

impl ControllerPolicy {
    pub(super) fn for_instrument(instrument: &crate::import::Instrument) -> Self {
        if instrument.native_program_private.as_ref().is_some_and(|record|
            matches!(record.controller_options(), Ok(Some(_)))) {
            Self::Native
        } else {
            Self::Compatibility
        }
    }

    pub(super) fn decode(self, address: super::params::Address, value: i32) -> f32 {
        match self.conversion_parameter(address) {
            Some(GroupPar::Volume) => super::params::native_volume(f64::from(value)),
            Some(GroupPar::Pan) => super::params::native_pan(f64::from(value)),
            _ => address.decode(value),
        }
    }

    pub(super) fn encode(self, address: super::params::Address, value: f32) -> i32 {
        match self.conversion_parameter(address) {
            Some(GroupPar::Volume) => super::params::native_volume_value(value),
            Some(GroupPar::Pan) => super::params::native_pan_value(value),
            _ => address.encode(value),
        }
    }

    pub(super) fn conversion_parameter(self, address: super::params::Address) -> Option<GroupPar> {
        match address {
            super::params::Address::Instrument(p @ (GroupPar::Volume | GroupPar::Pan))
            | super::params::Address::Group(_, p @ (GroupPar::Volume | GroupPar::Pan)) if self == Self::Native => Some(p),
            _ => None,
        }
    }

    pub(super) fn native_control(self, address: super::params::Address) -> Option<GroupPar> {
        match address {
            super::params::Address::Instrument(p @ (GroupPar::Volume | GroupPar::Pan)) if self == Self::Native => Some(p),
            _ => None,
        }
    }

    pub(super) fn control_value(param: GroupPar, value: crate::midi::ControlValue, maximum_db: i32) -> f32 {
        match param {
            GroupPar::Volume => super::params::native_cc7(value, maximum_db),
            GroupPar::Pan => super::params::native_cc10(value),
            _ => unreachable!("only native standard controls have physical scalar writes"),
        }
    }
}

/// Native program output stage, after program effects and returned sends.
/// Branch decisions survive progressive fragments of one native host span.
#[derive(Default)]
pub(super) struct ProgramAmplifier {
    current: [f32; 2], // linear gain, bipolar pan
    target: Option<[f32; 2]>,
    window: Option<(u64, u64)>,
    moving: [bool; 2],
}

impl ProgramAmplifier {
    pub(super) fn idle(&mut self) {
        self.target = None;
        self.window = None;
    }

    pub(super) fn is_fresh(&self) -> bool { self.target.is_none() }

    pub(super) fn process(&mut self, controls: InstrumentControls, window: Option<(u64, u64)>,
        rate: f32, fed: bool, left: &mut [f32], right: &mut [f32]) {
        if left.is_empty() || right.is_empty() { return; }
        if !fed {
            // An absent program buffer makes the next fed buffer seed directly.
            self.idle();
            return;
        }
        self.process_shared(controls, window, rate, fed, self.is_fresh(), left, right);
    }

    /// Bank inner stages share the container's fresh receipt. An absent inner
    /// buffer retains its scalar state while another program can remain active.
    pub(super) fn process_shared(&mut self, controls: InstrumentControls, window: Option<(u64, u64)>,
        rate: f32, fed: bool, fresh: bool, left: &mut [f32], right: &mut [f32]) {
        if !fed || left.is_empty() || right.is_empty() { return; }
        let target = [controls.gain, controls.pan];
        if !target.iter().all(|x| x.is_finite()) || !rate.is_finite() || rate <= 0. { return; }
        if fresh {
            self.current = target;
            self.target = None;
        }
        let up = 10.0 / rate;
        let up = up + up;
        if self.window != window || self.target != Some(target) {
            let ratio = (self.current[0] + 1e-12).abs() / (target[0] + 1e-12).abs();
            self.moving = [ratio > 1.001 || ratio < 0.999, (self.current[1] - target[1]).abs() > up];
            self.target = Some(target);
            self.window = window;
        }
        // Native decisions are buffer-scoped. Sample-timed target changes above
        // begin a new decision within that span: the progressive KONTRA adapter,
        // without a claim that native callback scheduling splits the same buffer.
        let down = -20.0 / rate;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let gain = if self.moving[0] { self.current[0] } else { target[0] };
            let pan = if self.moving[1] { self.current[1] } else { target[1] };
            *l *= gain;
            *r *= gain;
            *l *= 1.0 - pan.clamp(0.0, 1.0);
            *r *= (pan + 1.0).clamp(0.0, 1.0);
            if self.moving[0] { self.current[0] += (target[0] - self.current[0]) * (1.0 / 1800.0); }
            if self.moving[1] {
                self.current[1] = if self.current[1] <= target[1] {
                    (self.current[1] + up).min(target[1])
                } else { (self.current[1] + down).max(target[1]) };
            }
        }
    }
}

#[derive(Default)]
pub(super) struct InstrumentState {
    pub note_modes: [super::NoteMode; 16],
    pub program_amplifier: ProgramAmplifier,
    pub shared_lfos: super::lfo::Shared,
    pub shared_ahdsr: Option<Box<super::ahdsr_shared::Range>>,
    pub lfo_timings: super::lfo::Timings,
    pub controls: InstrumentControls,
    /// Last authored native Volume units; imported linear gain has no such authority.
    pub volume_value: Option<i32>,
    pub pan_value: Option<i32>,
    pub controller_options: ni_file::kontakt::objects::ProgramControllerOptions,
    pub controller_policy: ControllerPolicy,
    pub sampling_profile: crate::import::SamplingProfile,
    pub base: Vec<super::GroupSettings>,
    pub settings: Vec<super::GroupSettings>,
}

impl InstrumentState {
    /// Preparation only: allocate both layers before entering the audio thread.
    pub(super) fn new(controls: InstrumentControls, settings: Vec<super::GroupSettings>) -> Self {
        Self { note_modes: [super::NoteMode::Poly; 16], program_amplifier: Default::default(), shared_ahdsr: super::ahdsr_shared::Range::new(&settings), shared_lfos: super::lfo::Shared::new(&settings), lfo_timings: super::lfo::Timings::new(&settings), controls, volume_value: None, pan_value: None, controller_options: Default::default(), controller_policy: ControllerPolicy::Compatibility, sampling_profile: Default::default(), base: settings.clone(), settings }
    }

    pub(super) fn control_value(&self, param: GroupPar) -> Option<i32> {
        match param { GroupPar::Volume => self.volume_value, GroupPar::Pan => self.pan_value, _ => None }
    }

    pub(super) fn set_control_value(&mut self, param: GroupPar, value: Option<i32>) {
        match param { GroupPar::Volume => self.volume_value = value, GroupPar::Pan => self.pan_value = value, _ => {} }
    }

    pub(super) fn uses_sustain(&self) -> bool {
        self.controller_options.sustain_mode != ni_file::kontakt::objects::SustainPedalMode::ControllerOnly
    }

    /// Only these two native note actions share the verified program switch.
    pub(super) fn native_note_action(&self, cc: u8) -> bool {
        !matches!(cc, 120 | 123) || self.controller_options.standard_note_modes
    }
}

impl InstrumentControls {
    pub(super) fn read(self, param: GroupPar) -> Option<f32> {
        match param {
            GroupPar::Volume => Some(self.gain),
            GroupPar::Pan => Some(self.pan),
            GroupPar::Tune => Some(self.tune),
            GroupPar::Output => None,
        }
    }

    /// Apply a decoded engine parameter; output routing belongs to the racks.
    pub(super) fn write(&mut self, param: GroupPar, value: f32) -> bool {
        match param {
            GroupPar::Volume => self.gain = value.max(0.0),
            GroupPar::Pan => self.pan = value.clamp(-1.0, 1.0),
            GroupPar::Tune => self.tune = value,
            GroupPar::Output => return false,
        }
        true
    }
}

#[cfg(all(test, feature = "plugin"))]
#[path = "instrument_source_upgrade_tests.rs"]
mod source_upgrade_tests;

#[cfg(all(test, feature = "plugin"))]
#[path = "program_amp_tests.rs"]
mod program_amp_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        engine::{
            Engine, ScriptSetup, load_scripts,
            params::{Address, id},
        },
        import::Instrument,
        ksp::{EnginePar, KspEngine},
    };

    #[test]
    fn incompatible_group_upgrade_replaces_state_and_voice_lifetimes_without_heap() {
        use crate::{audio::Sample, engine::Bank, import::{Group, Zone}};
        let prepare = |groups: Vec<Group>, group| Box::new(Bank::from_samples(groups,
            vec![Zone { group, ..Default::default() }],
            vec![(Default::default(), Sample { rate: 48_000, frames: vec![[0.25; 2]; 4096] })],
        ).unwrap());
        let mut engine = Engine::default(); engine.attack = 0.;
        engine.set_bank(Some(prepare(vec![Group::default()], 0)));
        assert!(engine.write(Address::Instrument(GroupPar::Volume), 0.25));
        engine.note_on(0, 60, 127);
        engine.render(&mut [0.; 128], &mut [0.; 128]);
        assert_eq!(engine.active_voices(), 1);
        let base_storage = engine.state.base.as_ptr();
        // Zone/sample counts match, but the new zone needs a settings row that
        // did not exist in the live instrument. This is a replacement, not a fill.
        let mut incoming = prepare(vec![Group { gain: 0.125, ..Default::default() },
            Group { gain: 0.75, ..Default::default() }], 1);
        incoming.initial_state.as_mut().unwrap().controls.gain = 0.5;
        let (mut retired, mut carried) = (None, usize::MAX);
        let (mut left, mut right) = ([0.; 128], [0.; 128]);
        assert_eq!(crate::plugin::tests::allocations(|| {
            retired = engine.upgrade_bank(incoming);
            carried = engine.active_voices();
            engine.note_on(0, 60, 127);
            for _ in 0..3 { engine.render(&mut left, &mut right); }
        }), 0);
        assert_eq!(carried, 0, "incompatible fills must close old voice lifetimes");
        assert_eq!(engine.state.base.len(), 2);
        assert_eq!(engine.state.settings.len(), 2);
        assert_eq!(engine.state.controls.gain, 0.5);
        assert_eq!(engine.active_voices(), 1);
        assert!(left.iter().chain(&right).all(|&sample| (sample - 0.09375).abs() < 1e-7),
            "the new zone must render through its own group and program state");
        let previous = retired.as_ref().unwrap().initial_state.as_ref().unwrap();
        assert_eq!(previous.controls.gain, 0.25);
        assert_eq!(previous.base.as_ptr(), base_storage);
        drop(retired); // Dispose the replaced parameter storage away from audio.
    }

    #[test]
    fn prepared_voice_rows_recycle_and_retire_across_every_bank_lifetime_without_heap() {
        use crate::{audio::Sample, engine::{Bank, GroupSettings, MAX_VOICES},
            fx::{Chain, Effect, Kind, Params, params::Filter},
            import::{Group, ModAssignment, ModSource, ModTarget, Zone}};
        let group = |count: usize| Group {
            mods: (0..count).map(|i| ModAssignment { cc_default: None,
                name: format!("route{i}"), source: ModSource::Constant,
                target: if i % 2 == 0 { ModTarget::Volume }
                    else { ModTarget::Module { param: "filterCutoff".into(), slot: 0 } },
                intensity: if i % 2 == 0 { 0.03125 } else { 0.001953125 },
                invert: false, lag_ms: 7, shaper: None,
            }).collect(),
            fx: Chain { slots: vec![Effect { slot: 0, kind: Kind::Filter, version: 0,
                bypass: false, output_gain: 1., dry_level: 0., params: Params::Filter(Filter {
                    filter_type: 2, cutoff: 0.2, resonance: 0.15, extra: [0.;3], native_flag: None }) }] },
            ..Default::default()
        };
        let prepare = |count, frames| Box::new(Bank::from_samples(vec![group(count)], vec![Zone::default()],
            vec![(Default::default(), Sample { rate: 48000, frames: vec![[0.1;2];frames] })]).unwrap());
        // Consuming edits prepare replacement rosters, preserving the authored
        // base and returning a bank that is already coherent for installation.
        let edited = (*prepare(2, 8192)).with_prepared_settings(|settings| {
            settings[0] = GroupSettings::from(&group(32));
        }).unwrap();
        assert_eq!(edited.initial_state.as_ref().unwrap().base[0].mods.voiced_count(), 1);
        assert_eq!(edited.initial_state.as_ref().unwrap().settings[0].mods.voiced_count(), 16);
        assert!(edited.route_spares.iter().all(|row| row.values.len()==16 && row.external.len()==16));
        let (mut engine, mut oracle) = (Engine::default(), Engine::default());
        engine.attack=0.; oracle.attack=0.;
        let edited=Box::new(edited); // Worker-prepared handoff payload.
        let mut retired=None;
        assert_eq!(crate::plugin::tests::allocations(|| retired=engine.set_bank(Some(edited))),0);
        assert!(retired.is_none());
        oracle.set_bank(Some(prepare(32,8192)));
        assert_eq!(crate::plugin::tests::allocations(|| {
            engine.note_on(0,60,127); oracle.note_on(0,60,127);
        }),0);
        let (mut l,mut r,mut expected_l,mut expected_r)=([0.;128],[0.;128],[0.;128],[0.;128]);
        engine.render(&mut l,&mut r); oracle.render(&mut expected_l,&mut expected_r);
        assert_eq!(l,expected_l); assert_eq!(r,expected_r);
        // The wider effective seed above proves installation. Residency reuse
        // below starts from matching immutable source records on both banks.
        let (same_source, same_source_oracle)=(prepare(32,8192),prepare(32,8192));
        let mut old_oracle=None;
        assert_eq!(crate::plugin::tests::allocations(|| {
            retired=engine.set_bank(Some(same_source));
            old_oracle=oracle.set_bank(Some(same_source_oracle));
            engine.note_on(0,60,127); oracle.note_on(0,60,127);
        }),0);
        drop(retired.take()); drop(old_oracle);
        let live_values=engine.player.voices[0].mods.as_ptr();
        let live_external=engine.player.voices[0].filter.ext.as_ptr();
        let free_rows=engine.player.route_spares.as_ptr();
        engine.render(&mut l,&mut r); oracle.render(&mut expected_l,&mut expected_r);
        assert_eq!(l,expected_l); assert_eq!(r,expected_r);
        let incoming=prepare(32,8192);
        let unused_rows=incoming.route_spares.as_ptr();
        assert_eq!(crate::plugin::tests::allocations(|| retired=engine.upgrade_bank(incoming)),0);
        assert_eq!(engine.player.voices[0].mods.as_ptr(),live_values);
        assert_eq!(engine.player.voices[0].filter.ext.as_ptr(),live_external);
        assert_eq!(engine.player.route_spares.as_ptr(),free_rows);
        assert_eq!(retired.as_ref().unwrap().route_spares.as_ptr(),unused_rows);
        assert!(engine.bank().unwrap().route_spares.is_empty());
        assert_eq!(crate::plugin::tests::allocations(|| {
            engine.render(&mut l,&mut r); oracle.render(&mut expected_l,&mut expected_r);
            assert_eq!(l,expected_l); assert_eq!(r,expected_r);
        }),0);
        assert!(l.iter().any(|x|x.abs()>0.01));
        assert!(crate::plugin::tests::allocations(|| drop(retired.take()))>0,
            "unused incoming preparation is destroyed by the worker");

        // An incompatible wider roster replaces the live state and returns all
        // old rows on the retired bank rather than truncating or reusing them.
        let incoming=prepare(48,512);
        let mut fresh=Engine::default(); fresh.attack=0.; fresh.set_bank(Some(prepare(48,512)));
        assert_eq!(crate::plugin::tests::allocations(|| retired=engine.upgrade_bank(incoming)),0);
        assert_eq!(engine.active_voices(),0);
        assert_eq!(retired.as_ref().unwrap().route_spares.len(),MAX_VOICES);
        assert!(retired.as_ref().unwrap().route_spares.iter().all(|row|row.values.len()==16 && row.external.len()==16));
        drop(retired.take());
        assert_eq!(crate::plugin::tests::allocations(|| {
            engine.note_on(0,60,127); fresh.note_on(0,60,127);
            for _ in 0..8 {
                engine.render(&mut l,&mut r); fresh.render(&mut expected_l,&mut expected_r);
                assert_eq!(l,expected_l); assert_eq!(r,expected_r);
            }
        }),0);
        assert_eq!(engine.active_voices(),0,"EOF recycles each route box");
        assert_eq!(engine.player.route_spares.len(),MAX_VOICES);

        // Global capacity stealing consumes a recycled row; reset then returns
        // every live row without allocator activity before unloading the pool.
        assert_eq!(crate::plugin::tests::allocations(|| {
            for _ in 0..=MAX_VOICES { engine.note_on(0,60,127); }
            assert_eq!(engine.active_voices(),MAX_VOICES);
            assert!(engine.player.route_spares.is_empty());
            engine.reset(48000.);
            assert_eq!(engine.active_voices(),0);
            assert_eq!(engine.player.route_spares.len(),MAX_VOICES);
            retired=engine.set_bank(None);
        }),0);
        assert!(engine.player.route_spares.is_empty());
        assert_eq!(retired.as_ref().unwrap().route_spares.len(),MAX_VOICES);
        assert!(crate::plugin::tests::allocations(|| drop(retired.take()))>0,
            "unloaded route storage is destroyed off-thread");
    }

    #[test]
    fn bank_handoffs_retire_parameter_storage_and_upgrade_live_pcm_without_heap() {
        use crate::{audio::Sample, engine::{Bank, overrides::{Override, Param}, params::Stage}, import::{Group, Zone}};
        let prepare = |gain: f32, group_gain: f32| {
            let mut bank = Bank::from_samples(
                vec![Group { gain: group_gain, ..Default::default() }], vec![Zone::default()],
                vec![(Default::default(), Sample { rate: 48_000, frames: vec![[0.25; 2]; 4096] })],
            ).unwrap();
            bank.transpose = 12;
            let state = bank.initial_state.as_mut().unwrap();
            state.controls = InstrumentControls { gain, pan: -0.2, tune: 2.0 };
            state.base[0].envelope = Some(crate::engine::voice::Ahdsr::UNITY);
            state.settings[0].envelope = Some(crate::engine::voice::Ahdsr::UNITY);
            Box::new(bank)
        };
        let edit = |engine: &mut Engine| {
            for (param, value) in [(GroupPar::Volume, 0.5), (GroupPar::Pan, 0.35), (GroupPar::Tune, 3.0)] {
                assert!(engine.write(Address::Instrument(param), value));
            }
            assert!(engine.write(Address::Group(0, GroupPar::Volume), 0.625));
            assert!(engine.write(Address::Envelope(0, Stage::Sustain), 0.625));
            engine.set_override(Override { group: Some(0), param: Param::Sustain, offset: 0.1 });
        };
        let (mut actual, mut oracle) = (Engine::default(), Engine::default());
        actual.attack = 0.; oracle.attack = 0.;
        let initial = prepare(0.2, 0.3);
        let mut retired = None;
        assert_eq!(crate::plugin::tests::allocations(|| retired = actual.set_bank(Some(initial))), 0);
        assert!(retired.is_none());
        oracle.set_bank(Some(prepare(0.2, 0.3)));
        edit(&mut actual); edit(&mut oracle);
        let controls = actual.state.controls;
        let (base_gain, effective_gain) = (actual.state.base[0].gain, actual.state.settings[0].gain);
        let (base_sustain, effective_sustain) = (actual.state.base[0].envelope.unwrap().sustain, actual.state.settings[0].envelope.unwrap().sustain);
        assert_ne!(base_sustain, effective_sustain, "authored and editor layers must remain distinct");
        let (base_storage, effective_storage) = (actual.state.base.as_ptr(), actual.state.settings.as_ptr());
        actual.note_on(0, 60, 127); oracle.note_on(0, 60, 127);
        let (mut a, mut ar, mut b, mut br) = ([0.; 128], [0.; 128], [0.; 128], [0.; 128]);
        for _ in 0..3 { actual.render(&mut a, &mut ar); oracle.render(&mut b, &mut br); }
        let upgrade = prepare(0.9, 0.8);
        assert_eq!(crate::plugin::tests::allocations(|| retired = actual.upgrade_bank(upgrade)), 0);
        assert_eq!(actual.state.controls, controls);
        assert_eq!((actual.state.base[0].gain, actual.state.settings[0].gain), (base_gain, effective_gain));
        assert_eq!((actual.state.base[0].envelope.unwrap().sustain, actual.state.settings[0].envelope.unwrap().sustain), (base_sustain, effective_sustain));
        assert_eq!((actual.state.base.as_ptr(), actual.state.settings.as_ptr()), (base_storage, effective_storage));
        assert!(actual.bank().unwrap().initial_state.is_none(), "installed banks do not own live parameters");
        let unused_seed = retired.as_ref().unwrap().initial_state.as_ref().unwrap();
        assert_eq!(unused_seed.controls.gain, 0.9);
        assert_eq!((unused_seed.base[0].gain, unused_seed.settings[0].gain), (0.8, 0.8));
        assert_eq!(crate::plugin::tests::allocations(|| {
            for _ in 0..2 {
                actual.render(&mut a, &mut ar); oracle.render(&mut b, &mut br);
                assert_eq!(a, b, "upgrade keeps the active voice's audio and controls");
                assert_eq!(ar, br);
            }
        }), 0);
        assert!(a.iter().chain(&ar).any(|x| x.abs() > 0.01));
        drop(retired.take()); // Worker-side disposal, outside the callback count.

        let replacement = prepare(0.7, 0.4);
        assert_eq!(crate::plugin::tests::allocations(|| retired = actual.set_bank(Some(replacement))), 0);
        let previous = retired.as_ref().unwrap().initial_state.as_ref().unwrap();
        assert_eq!(previous.controls, controls);
        assert_eq!((previous.base[0].gain, previous.settings[0].gain), (base_gain, effective_gain));
        assert_eq!((previous.base[0].envelope.unwrap().sustain, previous.settings[0].envelope.unwrap().sustain), (base_sustain, effective_sustain));
        assert_eq!((previous.base.as_ptr(), previous.settings.as_ptr()), (base_storage, effective_storage));
        assert_eq!(actual.state.controls.gain, 0.7);
        assert_eq!(actual.state.base[0].gain, 0.4);
        assert_eq!(actual.active_voices(), 0);
        drop(retired.take());

        let controls = actual.state.controls;
        let (base_storage, effective_storage) = (actual.state.base.as_ptr(), actual.state.settings.as_ptr());
        assert_eq!(crate::plugin::tests::allocations(|| retired = actual.set_bank(None)), 0);
        let previous = retired.as_ref().unwrap().initial_state.as_ref().unwrap();
        assert_eq!(previous.controls, controls);
        assert_eq!((previous.base.as_ptr(), previous.settings.as_ptr()), (base_storage, effective_storage));
        assert!(actual.bank().is_none());
        assert!(actual.state.base.is_empty() && actual.state.settings.is_empty());
        assert_eq!(actual.state.controls, InstrumentControls::default());
        drop(retired);
    }

    #[test]
    fn script_setup_and_playback_share_instrument_controls() {
        let instrument = Instrument {
            scripts: vec![
                "on init
            set_engine_par($ENGINE_PAR_VOLUME,250000,-1,-1,-1)
            set_engine_par($ENGINE_PAR_PAN,750000,-1,-1,-1)
            set_engine_par($ENGINE_PAR_TUNE,625000,-1,-1,-1)
            end on"
                    .into(),
            ],
            ..Default::default()
        };
        let mut setup = ScriptSetup::new(&instrument, 48_000.);
        let par = |id| EnginePar {
            id,
            group: -1,
            slot: -1,
            generic: -1,
        };
        for (id, value) in [
            (id::VOLUME, 250_000),
            (id::PAN, 750_000),
            (id::TUNE, 625_000),
        ] {
            assert!(setup.set_engine_par(0, par(id), value));
            assert_eq!(setup.engine_par(par(id)), Some(value));
        }
        let (runtime, errors) = load_scripts(&instrument, vec![], 48_000.);
        assert!(errors.is_empty(), "{errors:?}");
        let mut engine = Engine::default();
        engine.tune = -2.0;
        engine.attack = 0.0;
        let expected = InstrumentControls {
            gain: Address::Instrument(GroupPar::Volume).decode(250_000),
            pan: 0.5,
            tune: 9.0,
        };
        let bank = crate::engine::Bank::from_samples(
            vec![crate::import::Group::default()],
            vec![crate::import::Zone::default()],
            vec![(
                std::path::PathBuf::new(),
                crate::audio::Sample {
                    rate: 48_000,
                    frames: vec![[0.25; 2]; 4096],
                },
            )],
        )
        .unwrap();
        engine.set_bank(Some(Box::new(bank)));
        engine.set_script(runtime);
        assert_eq!(engine.tune, -2.0);
        assert_eq!(engine.state.controls, expected);
        {
            let (_, host) = engine.scripted(0).unwrap();
            for id in [id::VOLUME, id::PAN, id::TUNE] {
                assert_eq!(host.engine_par(par(id)), setup.engine_par(par(id)));
            }
        }
        assert_eq!(engine.player.midi.cc[0][7].midi1(), 127);
        assert_eq!(engine.player.midi.cc[0][10].midi1(), 64);

        let (mut left, mut right) = ([0.0; 64], [0.0; 64]);
        assert_eq!(
            crate::plugin::tests::allocations(|| {
                engine.note_on(0, 60, 127);
                for _ in 0..4 { engine.render(&mut left, &mut right); }
            }),
            0
        );
        let target = crate::engine::voice::balance(0.25 * expected.gain, expected.pan);
        assert!(left.iter().all(|&sample| (sample - target[0]).abs() < 1e-7));
        assert!(right.iter().all(|&sample| (sample - target[1]).abs() < 1e-7));
        let voices = engine.voice_census();
        assert_eq!(voices.len(), 1);
        assert!(
            (voices[0].step - 2_f64.powf(7.0 / 12.0)).abs() < 1e-6,
            "instrument and part semitone tuning must combine during playback"
        );
    }

    #[test]
    fn authored_group_volume_pan_units_follow_metadata_in_setup_live_readback_and_pcm_without_heap() {
        use crate::{engine::{Bank, Disp}, import::{Group, Zone}};
        use ni_file::kontakt::objects::ProgramDataPrivateParams;
        const STAGE: usize = 768;
        const VALUES: [(i32, i32); 5] = [
            (500_000, 500_000), (1_000_000, 496_062), (500_000, 503_936),
            (0, 0), (500_000, 1_000_000),
        ];
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("group-parameter-units.wav");
        let mut wav = hound::WavWriter::create(&path, hound::WavSpec {
            channels: 2, sample_rate: 48_000, bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        }).unwrap();
        for _ in 0..16_384 { for _ in 0..2 { wav.write_sample(0.25_f32).unwrap(); } }
        wav.finalize().unwrap();
        let par = |id| EnginePar { id, group: 0, slot: -1, generic: -1 };
        // Independent replay of native float converters, including CC10's
        // byte-63/64 parameter values. Compatibility retains its existing law.
        for (policy, gains, pans) in [
            (ControllerPolicy::Native, [0.5, 4., 0.5, 0., 0.5],
                [0., -0.007_875_979, 0.007_871_985, -1., 1.]),
            (ControllerPolicy::Compatibility,
                [0.497_633_96, 3.9810717, 0.497_633_96, 0., 0.497_633_96],
                [0., -0.007876, 0.007872, -1., 1.]),
        ] {
            let record = (policy == ControllerPolicy::Native).then(|| ProgramDataPrivateParams {
                version: 0x80,
                data: [0u32.to_le_bytes().as_slice(), &[1, 1], 0i32.to_le_bytes().as_slice(), &[0xa5, 0, 0xff]].concat(),
            });
            let mut script = String::from("on init\nset_engine_par($ENGINE_PAR_VOLUME,500000,0,-1,-1)\nset_engine_par($ENGINE_PAR_PAN,500000,0,-1,-1)\nend on\non controller\n");
            for (stage, (volume, pan)) in VALUES.iter().enumerate().skip(1) {
                script.push_str(&format!("if ($CC_NUM={stage})\nset_engine_par($ENGINE_PAR_VOLUME,{volume},0,-1,-1)\nset_engine_par($ENGINE_PAR_PAN,{pan},0,-1,-1)\nend if\n"));
            }
            script.push_str("end on");
            let instrument = Instrument {
                groups: vec![Group::default()],
                zones: vec![Zone { sample: path.clone(), ..Default::default() }],
                native_program_private: record, scripts: vec![script], ..Default::default()
            };
            assert_eq!(ControllerPolicy::for_instrument(&instrument), policy);
            for parameter in [GroupPar::Volume, GroupPar::Pan] {
                assert_eq!(policy.native_control(Address::Group(0, parameter)), None,
                    "group scalar conversion must not claim physical CC7/10 ownership");
            }
            let mut setup = ScriptSetup::new(&instrument, 48_000.);
            for (stage, &(volume, pan)) in VALUES.iter().enumerate() {
                for (id, value) in [(id::VOLUME, volume), (id::PAN, pan)] {
                    assert!(setup.set_engine_par(0, par(id), value));
                    assert_eq!(setup.engine_par(par(id)), Some(value));
                }
                match setup.engine_par_display(par(id::VOLUME), volume) {
                    Some(Disp::Gain(value)) if policy == ControllerPolicy::Native => assert_eq!(value, gains[stage]),
                    None if policy == ControllerPolicy::Compatibility => {},
                    _ => panic!("setup Volume display must use the selected scalar policy"),
                }
                match setup.engine_par_display(par(id::PAN), pan) {
                    Some(Disp::Pan(value)) if policy == ControllerPolicy::Native => assert_eq!(value, pans[stage]),
                    None if policy == ControllerPolicy::Compatibility => {},
                    _ => panic!("setup Pan display must use the selected scalar policy"),
                }
            }
            let mut pcm = [[[0_f32; STAGE * VALUES.len()]; 2]; 4];
            for (mode, output) in pcm.iter_mut().enumerate() {
                let bank = Bank::load(&instrument).unwrap();
                assert_eq!(bank.initial_state.as_ref().unwrap().controller_policy, policy);
                let (runtime, errors) = load_scripts(&instrument, vec![], 48_000.);
                assert!(errors.is_empty(), "{errors:?}");
                let mut engine = Engine::default();
                engine.reset(48_000.); engine.attack = 0.; engine.blocking_streams = true;
                engine.set_bank(Some(Box::new(bank))); engine.set_script(runtime);
                let fragments: &[usize] = if mode % 2 == 0 { &[128] } else { &[1, 7, 31, 128] };
                assert_eq!(crate::plugin::tests::allocations(|| {
                    engine.note_on(0, 60, 127);
                    for (stage, &(volume, pan)) in VALUES.iter().enumerate() {
                        if stage != 0 {
                            if mode < 2 {
                                let (_, mut host) = engine.scripted(0).unwrap();
                                assert!(host.set_engine_par(0, par(id::VOLUME), volume));
                                assert!(host.set_engine_par(0, par(id::PAN), pan));
                                assert_eq!(host.engine_par(par(id::VOLUME)), Some(volume));
                                assert_eq!(host.engine_par(par(id::PAN)), Some(pan));
                            } else { engine.cc(0, stage as u8, 127); }
                        }
                        let (mut offset, mut block) = (stage * STAGE, 0);
                        while offset < (stage + 1) * STAGE {
                            let length = fragments[block % fragments.len()].min((stage + 1) * STAGE - offset);
                            let [left, right] = &mut *output;
                            engine.render_checked(&mut left[offset..offset + length], &mut right[offset..offset + length]).unwrap();
                            offset += length; block += 1;
                        }
                        let (_, host) = engine.scripted(0).unwrap();
                        assert_eq!(host.engine_par(par(id::VOLUME)), Some(volume));
                        assert_eq!(host.engine_par(par(id::PAN)), Some(pan));
                        match host.engine_par_display(par(id::VOLUME), volume) {
                            Some(Disp::Gain(value)) if policy == ControllerPolicy::Native => assert_eq!(value, gains[stage]),
                            None if policy == ControllerPolicy::Compatibility => {},
                            _ => panic!("live Volume display must use the selected scalar policy"),
                        }
                        match host.engine_par_display(par(id::PAN), pan) {
                            Some(Disp::Pan(value)) if policy == ControllerPolicy::Native => assert_eq!(value, pans[stage]),
                            None if policy == ControllerPolicy::Compatibility => {},
                            _ => panic!("live Pan display must use the selected scalar policy"),
                        }
                    }
                }), 0);
                assert_eq!(engine.underruns(), 0);
                for stage in 0..VALUES.len() {
                    let gain = 0.25 * gains[stage];
                    let expected = [gain * (1. - pans[stage].max(0.)), gain * (1. + pans[stage].min(0.))];
                    for channel in 0..2 {
                        assert!(output[channel][stage * STAGE + 512..(stage + 1) * STAGE].iter()
                            .all(|value| (*value - expected[channel]).abs() < 1e-6),
                            "{policy:?} mode{mode} stage{stage} channel{channel} group scalar PCM");
                    }
                }
            }
            for output in &pcm[1..] {
                assert!(pcm[0].iter().flatten().zip(output.iter().flatten()).all(|(a, b)| (a - b).abs() < 1e-6),
                    "direct and scripted group writes must retain PCM across callback partitions");
            }
        }
    }
}
