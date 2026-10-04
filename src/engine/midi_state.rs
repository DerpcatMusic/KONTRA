//! Accepted playback controls and physical received parameter transactions.
//! Scripts can filter/delay controls before acceptance; received decoding stays
//! on the physical input channel. Pedals and voice lifetimes belong to Player.

use super::Expression;
use crate::midi::{Midi1Decoder, ReceivedControl, ParameterEvent, ParameterValue, MpeZones, Control, ControlTarget, ControlValue, Bend, ChannelTuning, reset_controller_value};

#[cfg(all(test, feature = "plugin"))]
#[path = "controller_defaults_tests.rs"]
mod controller_defaults_tests;

#[cfg(all(test, feature = "plugin"))]
#[path = "rpn_coarse_tests.rs"]
mod rpn_coarse_tests;

#[cfg(all(test, feature = "plugin"))]
#[path = "rpn_tuning_callback_tests.rs"]
mod rpn_tuning_callback_tests;

/// RPN0's compound wire fields are not interchangeable with their numeric sum.
/// Legal cents100..127 must not change the retained coarse field on a fine edit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BendSensitivity {
    semitones: f64,
    coarse: u8,
}

impl BendSensitivity {
    pub(crate) fn semitones(self) -> f64 { self.semitones }
}

pub(super) struct MidiState {
    pub(super) cc: [[ControlValue; 128]; 16],
    /// Accepted source availability is independent of numeric/cache defaults.
    pub(super) cc_received: [[u64; 2]; 16],
    pub(super) bend: [Bend; 16],
    /// Per-note bend configuration, captured at physical Note On. Performance
    /// resets preserve it just as they preserve the negotiated MPE setup.
    pub(super) per_note_range: [f64; 16],
    /// Physical channel sensitivity replaces authored Pitch Bend → Pitch only
    /// after negotiation. Active MIDI1 MPE uses zone sensitivity instead.
    pub(super) channel_bend_range: [Option<BendSensitivity>; 16],
    /// RPN1/2 affect only their physical channel, including active MPE members.
    /// Like negotiated bend ranges, tuning survives performance/session resets.
    pub(super) tuning: [ChannelTuning; 16],
    /// Ordinary MIDI1 sensitivities yield to MPE defaults when joining a zone.
    midi1_bend_range: u16,
    pub(super) pressure: [ControlValue; 16],
    pub(super) pressure_active: [bool; 16],
    /// Delivered channel/key expression, independent of physical controller rows.
    pub(super) expression: Box<[[Expression; 128]; 16]>,
    pub(super) controller_defaults: [Option<u8>; 128],
    pub(super) mpe: MpeZones,
    /// Shared invalidation stamp for everything modulation reads, including
    /// group writes and exact host-note expression outside this state.
    pub(super) revision: u32,
    received: [Midi1Decoder; 16],
}

impl Default for MidiState {
    fn default() -> Self {
        Self {
            cc: [[ControlValue::ZERO; 128]; 16],
            cc_received: [[0; 2]; 16],
            bend: [Bend::ZERO; 16],
            // Local compatibility default; the UMP specification defines no default.
            per_note_range: [48.; 16],
            channel_bend_range: [None; 16],
            tuning: [ChannelTuning::default(); 16],
            midi1_bend_range: 0,
            pressure: [ControlValue::ZERO; 16],
            pressure_active: [false; 16],
            expression: Box::new([[Expression::default(); 128]; 16]),
            controller_defaults: [None; 128],
            mpe: MpeZones::default(),
            revision: 0,
            received: [Midi1Decoder::default(); 16],
        }
    }
}

impl MidiState {
    /// Physical performance inputs; delivered KSP channels do not own these rows.
    pub(super) fn modulation_inputs(
        &self,
        physical_input: u8,
        note: u8,
        velocity: crate::midi::Velocity,
        release_velocity: Option<crate::midi::Velocity>,
        expression: Expression,
    ) -> super::params::Inputs<'_> {
        let c = physical_input as usize & 15;
        let master = self.mpe.member(physical_input).map(|zone| zone.manager as usize);
        super::params::Inputs { script_mods: &[],
            cc: &self.cc[c],
            cc_received: Some(&self.cc_received[c]),
            per_note_cc: None,
            cc74: expression.note_cc74.map(crate::midi::ControlValue::dsp).or_else(|| {
                master.map(|m| (expression.member_cc74.map(crate::midi::ControlValue::dsp)
                    .unwrap_or(self.cc[c][74].dsp()) + self.cc[m][74].dsp()).min(1.))
            }),
            cc74_received: expression.note_cc74.map(|_| true).or_else(|| master.map(|m| {
                expression.member_cc74_received.unwrap_or_else(|| super::params::controller_received(&self.cc_received[c], 74))
                    || super::params::controller_received(&self.cc_received[m], 74)
            })),
            bend: expression.member_bend.unwrap_or(self.bend[c]).dsp() + master.map_or(0., |m| self.bend[m].dsp()),
            pressure: expression.note_pressure.map(crate::midi::ControlValue::dsp).unwrap_or_else(|| {
                master.map_or(self.pressure[c].dsp(), |m| {
                    expression.member_pressure.map(crate::midi::ControlValue::dsp)
                        .unwrap_or(self.pressure[c].dsp()).max(self.pressure[m].dsp())
                })
            }),
            note,
            velocity,
            release_velocity,
            // Fixed at the note start, never read live.
            counter: 0.0,
        }
    }

    /// Reset accepted controls in place. Authored defaults and MPE setup stay
    /// configured; the engine resets physical transactions before callbacks.
    pub(super) fn reset(&mut self) {
        self.bend.fill(Bend::ZERO);
        self.pressure.fill(ControlValue::ZERO);
        self.pressure_active.fill(false);
        self.expression.fill([Expression::default(); 128]);
        self.cc.fill([ControlValue::ZERO; 128]);
        self.cc_received.fill([0; 2]);
        for cc in &mut self.cc {
            (cc[7], cc[10], cc[11]) = (ControlValue::FULL, ControlValue::from_midi1(64), ControlValue::FULL);
        }
        self.touch();
    }

    /// A different input port starts a new physical configuration namespace.
    /// Session resets retain these values; authored defaults and route-owned
    /// MPE configuration are restored by their existing owners.
    pub(super) fn reset_input_configuration(&mut self) {
        self.tuning.fill(ChannelTuning::default());
        self.channel_bend_range.fill(None);
        self.per_note_range.fill(48.);
        self.midi1_bend_range = 0;
        self.mpe = MpeZones::default();
        self.reset_received(u16::MAX);
        self.touch();
    }

    pub(super) fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// New instruments start their native CC sources at authored defaults.
    /// Numeric observers and physical protocol transactions retain their state.
    pub(super) fn clear_modulation_receipts(&mut self) {
        self.cc_received.fill([0; 2]);
        self.touch();
    }

    pub(super) fn set_defaults(&mut self, controllers: &[(u8, u8)]) {
        self.controller_defaults.fill(None);
        for &(cc, value) in controllers {
            if cc < 128 {
                self.controller_defaults[cc as usize] = Some(value);
            }
        }
    }

    /// Accept a performance value only on its physical row. Authored controls
    /// affect the part, including rows whose notes share a delivered channel.
    pub(super) fn accept(&mut self, target: ControlTarget, control: Control) -> bool {
        if !control.valid() || matches!(target, ControlTarget::Physical(channel) if channel >= 16) { return false; }
        let channels = match target {
            ControlTarget::Physical(channel) => (1u16 << channel),
            ControlTarget::Authored => u16::MAX,
        };
        for channel in (0..16).filter(|channel| channels & (1 << channel) != 0) {
            match control {
                Control::Cc(cc, value) => {
                    self.cc[channel][cc as usize] = value;
                    self.cc_received[channel][cc as usize / 64] |= 1 << (cc & 63);
                }
                Control::Cc14 { cc, raw, coarse, value } => {
                    if cc != coarse {
                        self.cc[channel][cc as usize] = ControlValue::from_midi1(raw);
                        self.cc_received[channel][cc as usize / 64] |= 1 << (cc & 63);
                    }
                    self.cc[channel][coarse as usize] = value;
                    self.cc_received[channel][coarse as usize / 64] |= 1 << (coarse & 63);
                }
                Control::Bend(value) => self.bend[channel] = value,
                Control::Pressure(value) => { self.pressure[channel] = value; self.pressure_active[channel] = true; },
                Control::ResetPressure => { self.pressure[channel] = ControlValue::ZERO; self.pressure_active[channel] = false; },
            }
        }
        self.touch();
        true
    }

    /// Byte projection for compatibility observers; playback reads canonical
    /// values directly, so this is not a second state authority.
    pub(super) fn midi1_cc(&self) -> [[u8; 128]; 16] {
        self.cc.map(|row| row.map(ControlValue::midi1))
    }

    pub(super) fn reset_controllers(&mut self, channel: u8) {
        let c = channel as usize;
        self.bend[c] = Bend::ZERO;
        self.pressure[c] = ControlValue::ZERO;
        self.pressure_active[c] = false;
        for cc in 0..128 {
            if let Some(value) = reset_controller_value(cc as u8, self.controller_defaults[cc]) {
                self.cc[c][cc] = ControlValue::from_midi1(value);
                // Reset supplies a real accepted performance value; retained
                // controllers keep both their value and availability.
                self.cc_received[c][cc / 64] |= 1 << (cc & 63);
            }
        }
        // Player has already accepted CC121 and its pedal releases, bumping
        // the revision before applying this reset.
    }

    pub(super) fn set_expression(&mut self, channel: u8, note: u8, expression: Expression) {
        if channel >= 16 || note >= 128
            || ![expression.gain, expression.tune, expression.pan].into_iter().all(f32::is_finite) {
            return;
        }
        let previous = std::mem::replace(
            &mut self.expression[channel as usize][note as usize],
            expression,
        );
        if previous.note_cc74 != expression.note_cc74
            || previous.member_cc74 != expression.member_cc74
            || previous.member_cc74_received != expression.member_cc74_received
            || previous.member_pressure != expression.member_pressure
            || previous.member_bend != expression.member_bend
            || previous.note_pressure != expression.note_pressure
        {
            self.touch();
        }
    }

    pub(super) fn mpe_role_changes(&self, next: MpeZones) -> u16 {
        (0..16u8).filter(|&channel| {
            let role = |zones: MpeZones| zones.zone(channel).map(|zone| (zone.manager, zone.manager == channel));
            role(self.mpe) != role(next)
        }).fold(0, |mask, channel| mask | (1 << channel))
    }

    pub(super) fn set_mpe(&mut self, mpe: MpeZones) {
        if self.mpe != mpe {
            for channel in 0..16 {
                if self.midi1_bend_range & (1 << channel) != 0
                    && self.mpe.zone(channel).is_none() && mpe.zone(channel).is_some() {
                    self.channel_bend_range[channel as usize] = None;
                    self.midi1_bend_range &= !(1 << channel);
                }
            }
            self.mpe = mpe;
            self.touch();
        }
    }

    /// A channel assigned a new MPE role starts without previous expression
    /// or pedals. Instrument-wide volume/pan policy remains Player-owned.
    pub(super) fn reset_mpe_channels(&mut self, channels: u16) {
        for channel in (0..16).filter(|channel| channels & (1 << channel) != 0) {
            self.reset_controllers(channel as u8);
            self.cc[channel][74] = ControlValue::from_midi1(64);
            self.cc_received[channel][1] |= 1 << (74 & 63);
            // These rows are physical inputs; expression rows use delivered
            // channels and may still belong to an unrelated surviving input.
        }
        self.touch();
    }

    pub(super) fn receive_cc(&mut self, input: u8, cc: u8, value: u8) -> Option<ParameterValue> {
        self.receive_parameter(input, cc, value)?.absolute()
    }

    /// Decode once before routing; negotiation and callbacks consume the same
    /// event, with their own parameter-specific interpretation.
    pub(super) fn receive_parameter(
        &mut self,
        input: u8,
        cc: u8,
        value: u8,
    ) -> Option<ParameterEvent> {
        self.received.get_mut(input as usize)?.parameter(cc, value)
    }

    /// RPN1/2 tune one physical channel. RPN0 supplies ordinary-channel
    /// semitones/cents; active MPE keeps its manager/member zone policy.
    pub(super) fn apply_midi1_parameter(&mut self, input: u8, parameter: ParameterEvent) -> bool {
        if input >= 16 || !parameter.registered { return false; }
        let channel = input as usize;
        if matches!(parameter.address, 1 | 2) {
            if !self.tuning[channel].parameter(parameter.address, parameter.data) { return false; }
            if matches!(parameter.data, crate::midi::ParameterData::Increment | crate::midi::ParameterData::Decrement) {
                self.received[channel].seed_registered_parameter(parameter.address, self.tuning[channel].msb(parameter.address));
            }
            self.touch();
            return true;
        }
        if self.mpe.zone(input).is_some() || parameter.address != 0 { return false; }
        let range = if let crate::midi::ParameterData::Entry { msb, lsb: cents, .. } = parameter.data {
            let Some(coarse) = msb.or_else(|| self.channel_bend_range[channel].map(|range| range.coarse)) else {
                return false;
            };
            if coarse >= 128 || cents >= 128 { return false; }
            BendSensitivity { semitones: f64::from(coarse) + f64::from(cents) / 100., coarse }
        } else {
            let Some(current) = self.channel_bend_range[channel] else { return false; };
            let fine = ((current.semitones() - f64::from(current.coarse)) * 100.).round() as u8;
            let Some((coarse, fine)) = parameter.data.bend_fields(current.coarse, fine, 128.27) else { return false; };
            let semitones = f64::from(coarse) + f64::from(fine) / 100.;
            self.received[channel].seed_registered_parameter(0, coarse);
            BendSensitivity { semitones, coarse }
        };
        self.channel_bend_range[channel] = Some(range);
        self.midi1_bend_range |= 1 << input;
        self.touch();
        true
    }

    /// A relative RPN0 change updates the recognized zone baseline, including
    /// member fanout. Only currently selected RPN0 transactions are seeded.
    pub(super) fn sync_mpe_parameter_baselines(&mut self, input: u8) {
        Self::seed_mpe_parameter_baselines(&mut self.received, self.mpe, input);
    }

    pub(super) fn seed_mpe_parameter_baselines(received: &mut [Midi1Decoder; 16], mpe: MpeZones, input: u8) {
        let Some(zone) = mpe.zone(input) else { return; };
        let (channels, range) = if input == zone.manager {
            (1 << input, zone.manager_range)
        } else { (zone.member_mask(), Some(zone.member_range)) };
        if let Some(range) = range {
            for channel in (0..16).filter(|channel| channels & (1 << channel) != 0) {
                received[channel].seed_registered_parameter(0, range.coarse);
            }
        }
    }

    /// An outer receiver may borrow an accepted ordinary parameter's coarse
    /// field without selecting it or changing another selected transaction.
    pub(super) fn seed_parameter_baseline(&self, input: u8, address: u16, decoder: &mut Midi1Decoder) {
        if input >= 16 { return; }
        let input = input as usize;
        let coarse = match address {
            0 => self.channel_bend_range[input].map(|range| range.coarse),
            1 | 2 => Some(self.tuning[input].msb(address)),
            _ => None,
        };
        if let Some(coarse) = coarse { decoder.seed_registered_parameter(address, coarse); }
    }

    /// UMP v1.1.2 §7.4.7.1: channel range uses HCU and cents, not Q7.25.
    /// §7.4.13: per-note sensitivity uses Q7.25 and is captured by subsequent
    /// physical Note On bindings. Configuration survives performance resets.
    pub(super) fn apply_native_parameter(&mut self, input: u8, parameter: crate::midi::NativeParameter) -> bool {
        use crate::midi::NativeParameterData::{Absolute, Relative};
        if input >= 16 || !parameter.registered { return false; }
        let channel = input as usize;
        match (parameter.address, parameter.data) {
            (0, Absolute(value)) => {
                let cents = (value >> 18) & 127;
                let coarse = (value >> 25) as u8;
                self.channel_bend_range[channel] = Some(BendSensitivity {
                    semitones: f64::from(coarse) + f64::from(cents) / 100., coarse,
                });
                // Recognized configuration refreshes only an already selected
                // RPN0 baseline; it never arms a null/other wire transaction.
                self.received[channel].seed_registered_parameter(0, coarse);
                self.midi1_bend_range &= !(1 << input);
            }
            (7, Absolute(value)) => self.per_note_range[channel] = f64::from(value) / 33554432.,
            (7, Relative(delta)) => {
                let current = (self.per_note_range[channel] * 33554432.) as u32;
                self.per_note_range[channel] = f64::from(current.saturating_add_signed(delta)) / 33554432.;
            }
            (1 | 2, Absolute(value)) => { self.tuning[channel].native(parameter.address, value); }
            (1 | 2, Relative(delta)) => { self.tuning[channel].native_relative(parameter.address, delta); }
            // Compound HCU/cents and discrete MCM have no guessed relative law.
            _ => return false,
        }
        // The selected MIDI1 transaction and playback read one recognized
        // tuning owner. A native replacement/relative edit must not leave a
        // later fine-only callback reporting its older coarse baseline.
        if matches!(parameter.address, 1 | 2) {
            self.received[channel].seed_registered_parameter(parameter.address,
                self.tuning[channel].msb(parameter.address));
        }
        self.touch();
        true
    }

    pub(super) fn receive_midi1_cc(&mut self, input: u8, cc: u8, raw: u8) -> Option<ReceivedControl> {
        self.received.get_mut(input as usize)?.cc(cc, raw)
    }

    pub(super) fn received_decoders(&self) -> [Midi1Decoder; 16] { self.received }

    pub(super) fn midi1_velocity(&mut self, input: u8, raw: u8) -> Option<crate::midi::Velocity> {
        self.received.get_mut(input as usize)?.velocity(raw)
    }

    pub(super) fn midi1_release_velocity(&mut self, input: u8, raw: u8) -> Option<crate::midi::Velocity> {
        self.received.get_mut(input as usize)?.release_velocity(raw)
    }

    /// RP-015 preserves volume/pan/bank transactions while resetting ordinary
    /// performance controllers and the pending wire-only velocity prefix.
    pub(super) fn reset_received_controllers(&mut self, inputs: u16) {
        for input in (0..16).filter(|input| inputs & (1 << input) != 0) {
            self.received[input].reset_controllers(&self.controller_defaults);
        }
    }

    pub(super) fn reset_received(&mut self, inputs: u16) {
        for input in (0..16).filter(|input| inputs & (1 << input) != 0) {
            self.received[input].reset();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ranges(midi: &MidiState) -> [Option<f64>; 16] {
        midi.channel_bend_range.map(|range| range.map(BendSensitivity::semitones))
    }

    #[test]
    fn native_relative_parameters_keep_physical_scope_and_next_note_range() {
        use crate::midi::{NativeParameter, NativeParameterData::{Absolute, Relative}};
        let parameter = |address, data| NativeParameter { registered: true, address, data };
        let mut midi = MidiState::default();
        let mut mpe = midi.mpe;mpe.configure(0, 7, 48).unwrap();midi.set_mpe(mpe);
        assert!(midi.apply_native_parameter(4, parameter(1, Absolute(0xa0012345))));
        let original = midi.tuning[4].ratio();
        assert!(midi.apply_native_parameter(4, parameter(1, Relative(1))));
        assert_ne!(midi.tuning[4].ratio(), original);
        assert!(midi.apply_native_parameter(4, parameter(1, Relative(-1))));
        assert_eq!(midi.tuning[4].ratio(), original);
        assert_eq!(midi.tuning[7].ratio(), 1.);
        assert!(midi.apply_native_parameter(0, parameter(2, Relative(12 << 25))));
        assert_eq!(midi.tuning[0].ratio(), 2.);
        assert_eq!(midi.tuning[4].ratio(), original, "optional tuning never fans out a manager edit");
        assert!(midi.apply_native_parameter(4, parameter(7, Absolute((12 << 25) | 17))));
        let captured = midi.per_note_range[4];
        assert!(midi.apply_native_parameter(4, parameter(7, Relative(-1))));
        assert_eq!(midi.per_note_range[4], captured - 1. / 33554432.);
        assert_eq!(midi.per_note_range[7], 48.);
        assert!(midi.apply_native_parameter(4, parameter(7, Absolute(0))));
        assert!(midi.apply_native_parameter(4, parameter(7, Relative(i32::MIN))));
        assert_eq!(midi.per_note_range[4], 0.);
        assert!(midi.apply_native_parameter(4, parameter(7, Absolute(u32::MAX))));
        assert!(midi.apply_native_parameter(4, parameter(7, Relative(i32::MAX))));
        assert_eq!(midi.per_note_range[4], 128. - 1. / 33554432.);
        let (tuning, ranges) = (midi.tuning, midi.per_note_range);
        let revision = midi.revision;
        for event in [parameter(0, Relative(1)), parameter(6, Relative(1)), parameter(99, Relative(-1)),
            NativeParameter { registered: false, address: 1, data: Relative(-1) }] {
            assert!(!midi.apply_native_parameter(4, event));
        }
        assert!(!midi.apply_native_parameter(16, parameter(1, Relative(1))));
        assert_eq!(midi.revision, revision);
        midi.reset_controllers(4);midi.reset_received_controllers(1 << 4);midi.reset();
        assert_eq!((midi.tuning, midi.per_note_range), (tuning, ranges));
    }

    #[test]
    fn channel_tuning_uses_retained_configuration_and_keeps_physical_mpe_scope() {
        use crate::midi::NativeParameter;
        let mut midi = MidiState::default();
        let apply = |midi: &mut MidiState, channel, cc, value| {
            midi.receive_midi1_cc(channel, cc, value).unwrap().parameter
                .is_some_and(|event| midi.apply_midi1_parameter(channel, event))
        };
        let select = |midi: &mut MidiState, channel, address| {
            apply(midi, channel, 101, 0);apply(midi, channel, 100, address);
        };
        select(&mut midi, 4, 1);
        assert!(apply(&mut midi, 4, 38, 1), "fine-only tuning has an explicit neutral configuration baseline");
        assert_eq!(midi.tuning[4].ratio(), 2f64.powf((1. / 8192.) / 12.));
        apply(&mut midi, 4, 6, 80);apply(&mut midi, 4, 38, 127);
        let relative = midi.receive_midi1_cc(4, 96, 127).unwrap();
        assert_eq!(relative.control.legacy(), (96, 127));
        assert_eq!(relative.parameter.unwrap().absolute(), None, "relative edits invent no absolute callback");
        assert!(midi.apply_midi1_parameter(4, relative.parameter.unwrap()));
        let fine = midi.receive_midi1_cc(4, 38, 0).unwrap().parameter.unwrap();
        assert_eq!(fine.value(), Some(81 << 7), "relative carry refreshes only the selected parameter baseline");
        assert!(midi.apply_midi1_parameter(4, fine));
        assert_eq!(midi.tuning[4].ratio(), 2f64.powf((17. / 64.) / 12.));
        assert_eq!(midi.tuning[7].ratio(), 1.);

        select(&mut midi, 7, 2);apply(&mut midi, 7, 6, 52);
        let retained = midi.tuning;
        apply(&mut midi, 7, 38, 127);
        assert_eq!(midi.tuning, retained, "RPN2 fine byte has no tuning effect");
        midi.reset_received_controllers(1 << 4);midi.reset_controllers(4);
        select(&mut midi, 4, 1);apply(&mut midi, 4, 38, 50);
        assert_eq!(midi.tuning[4].ratio(), 2f64.powf(((17. * 128. + 50.) / 8192.) / 12.));
        let retained = midi.tuning;
        midi.reset();midi.reset_received(u16::MAX);
        assert_eq!(midi.tuning, retained, "configuration follows the established sensitivity reset policy");

        let mut mpe = MpeZones::default();mpe.configure(0, 7, 48).unwrap();midi.set_mpe(mpe);
        assert_eq!(midi.tuning, retained);
        assert!(midi.apply_native_parameter(4, NativeParameter { registered: true, address: 1, data: crate::midi::NativeParameterData::Absolute(0xa0001234) }));
        let member = midi.tuning[4];
        select(&mut midi, 0, 2);apply(&mut midi, 0, 6, 76);
        assert_eq!(midi.tuning[4], member, "optional manager tuning has no invented member fanout");
        assert_eq!(midi.tuning[0].ratio(), 2.);
        assert_eq!(midi.tuning[7], retained[7]);
        select(&mut midi, 4, 1);apply(&mut midi, 4, 96, 0);
        let selected = midi.receive_midi1_cc(4, 38, 0).unwrap().parameter.unwrap();
        assert_eq!(selected.value(), Some(80 << 7));
        apply(&mut midi, 4, 101, 127);apply(&mut midi, 4, 100, 127);
        let retained = midi.tuning;
        assert!(!apply(&mut midi, 4, 97, 127));
        apply(&mut midi, 4, 99, 0);apply(&mut midi, 4, 98, 1);
        assert!(!apply(&mut midi, 4, 6, 0));
        assert_eq!(midi.tuning, retained, "null and NRPN selections have no invented tuning meaning");
    }

    #[test]
    fn wire_pairing_changes_only_accepted_canonical_rows_and_keeps_raw_projection() {
        let mut midi = MidiState::default();midi.reset();
        midi.receive_midi1_cc(4, 1, 0);
        let fine = midi.receive_midi1_cc(4, 33, 127).unwrap().control;
        assert_eq!(fine.legacy(), (33, 127));
        assert_eq!(midi.cc[4][1], ControlValue::ZERO, "decoding must not bypass callback ignore/wait");
        assert!(midi.accept(ControlTarget::Physical(4), fine));
        assert_eq!(midi.cc[4][1].normalized(), 127. / 16383.);
        assert_eq!((midi.midi1_cc()[4][1], midi.midi1_cc()[4][33]), (0, 127));
        assert_eq!(midi.cc[7][1], ControlValue::ZERO);
        let coarse = midi.receive_midi1_cc(4, 1, 64).unwrap().control;
        assert!(midi.accept(ControlTarget::Physical(4), coarse));
        assert_eq!(midi.cc[4][1].normalized(), 8192. / 16383.);
        assert!(midi.accept(ControlTarget::Authored, Control::Cc(1, ControlValue::from_midi1(63))));
        assert_eq!(midi.midi1_cc()[4][1], 63);
        let invalid = Control::Cc14 { cc: 127, raw: 127, coarse: 31, value: ControlValue::FULL };
        assert!(!midi.accept(ControlTarget::Physical(4), invalid));
    }

    #[test]
    fn relative_rpn0_steps_cents_and_reseeds_only_its_recognized_transaction() {
        let mut midi = MidiState::default();
        let apply = |midi: &mut MidiState, channel, cc, value| {
            midi.receive_midi1_cc(channel, cc, value).unwrap().parameter
                .is_some_and(|event| midi.apply_midi1_parameter(channel, event))
        };
        for channel in [4, 7] { apply(&mut midi, channel, 101, 0);apply(&mut midi, channel, 100, 0); }
        assert!(!apply(&mut midi, 7, 96, 127), "unknown ordinary sensitivity cannot invent a default");
        apply(&mut midi, 4, 6, 12);apply(&mut midi, 4, 38, 99);
        let received = midi.receive_midi1_cc(4, 96, 127).unwrap();
        assert_eq!(received.control.legacy(), (96, 127));
        let event = received.parameter.unwrap();assert_eq!(event.absolute(), None);
        assert!(midi.apply_midi1_parameter(4, event));
        assert_eq!(ranges(&midi)[4], Some(13.));
        let fine = midi.receive_midi1_cc(4, 38, 25).unwrap().parameter.unwrap();
        assert_eq!(fine.value(), Some((12 << 7) | 25), "supported cents100 retain the original coarse field");
        assert!(midi.apply_midi1_parameter(4, fine));
        assert_eq!(ranges(&midi)[4], Some(12.25));
        assert_eq!(ranges(&midi)[7], None);
        for magnitude in [0, 1, 127] { assert!(apply(&mut midi, 4, 97, magnitude)); }
        assert_eq!(ranges(&midi)[4], Some(12.22));
        apply(&mut midi, 4, 6, 13);
        assert!(apply(&mut midi, 4, 97, 127));
        assert_eq!(ranges(&midi)[4], Some(12.99));
        apply(&mut midi, 4, 101, 127);apply(&mut midi, 4, 100, 127);
        assert!(!apply(&mut midi, 4, 96, 0));
        apply(&mut midi, 4, 99, 0);apply(&mut midi, 4, 98, 0);
        assert!(!apply(&mut midi, 4, 96, 0), "NRPN0 has no invented pitch mapping");
        let selected = midi.receive_midi1_cc(4, 6, 7).unwrap().parameter.unwrap();
        assert!(!midi.apply_midi1_parameter(4, selected));
        midi.received[4].seed_registered_parameter(0, 12);
        assert_eq!(midi.receive_midi1_cc(4, 38, 50).unwrap().parameter.unwrap().value(), Some((7 << 7) | 50));
        midi.reset_received_controllers(1 << 4);
        apply(&mut midi, 4, 101, 0);apply(&mut midi, 4, 100, 0);
        assert!(apply(&mut midi, 4, 96, 0));
        assert_eq!(ranges(&midi)[4], Some(13.));
        apply(&mut midi, 4, 6, 127);apply(&mut midi, 4, 38, 99);apply(&mut midi, 4, 96, 0);
        assert_eq!(ranges(&midi)[4], Some(128.));
        let fine = midi.receive_midi1_cc(4, 38, 127).unwrap().parameter.unwrap();
        assert_eq!(fine.value(), Some((127 << 7) | 127), "extended carry keeps a legal seven-bit coarse baseline");
        assert!(midi.apply_midi1_parameter(4, fine));
        assert_eq!(ranges(&midi)[4], Some(128.27));
        assert!(apply(&mut midi, 4, 96, 127));
        assert_eq!(ranges(&midi)[4], Some(128.27), "increment at the legal maximum cannot move backwards");
        assert!(apply(&mut midi, 4, 97, 0));
        assert_eq!(ranges(&midi)[4], Some(128.26));
        midi.reset_received_controllers(1 << 4);
        apply(&mut midi, 4, 101, 0);apply(&mut midi, 4, 100, 0);
        assert!(apply(&mut midi, 4, 38, 127), "retained extended sensitivity supplies legal coarse127 after reset");
        assert_eq!(ranges(&midi)[4], Some(128.27));
        apply(&mut midi, 4, 6, 0);apply(&mut midi, 4, 97, 0);
        assert_eq!(ranges(&midi)[4], Some(0.));
    }

    #[test]
    fn relative_mpe_ranges_fan_out_members_and_preserve_other_parameter_baselines() {
        let mut midi = MidiState::default();
        let mut zones = midi.mpe;zones.configure(0, 4, 48).unwrap();zones.configure(15, 4, 24).unwrap();midi.set_mpe(zones);
        let apply = |midi: &mut MidiState, channel, cc, value| {
            let Some(event) = midi.receive_midi1_cc(channel, cc, value).unwrap().parameter else { return; };
            let mut zones = midi.mpe;
            if zones.parameter(channel, event).is_some() {
                midi.set_mpe(zones);
                if event.data.bend_step(0., 96.).is_some() { midi.sync_mpe_parameter_baselines(channel); }
            }
        };
        for channel in [0, 1, 2, 3, 14, 15] { apply(&mut midi, channel, 101, 0);apply(&mut midi, channel, 100, 0); }
        apply(&mut midi, 3, 100, 1);apply(&mut midi, 3, 6, 7);
        apply(&mut midi, 1, 6, 12);apply(&mut midi, 1, 38, 99);apply(&mut midi, 2, 96, 127);
        assert_eq!(midi.mpe.member(1).unwrap().member_range.semitones(), 13.);
        assert_eq!(midi.mpe.member(2).unwrap().member_range.semitones(), 13.);
        assert_eq!(midi.mpe.member(14).unwrap().member_range.semitones(), 24.);
        assert_eq!(midi.mpe.manager(0).unwrap().manager_range, None);
        assert_eq!(midi.receive_midi1_cc(14, 38, 25).unwrap().parameter.unwrap().value(), None,
            "another zone's unknown wire baseline must not be seeded");
        assert_eq!(midi.receive_midi1_cc(1, 38, 25).unwrap().parameter.unwrap().value(), Some((12 << 7) | 25));
        assert_eq!(midi.receive_midi1_cc(3, 38, 25).unwrap().parameter.unwrap().value(), Some((7 << 7) | 25));
        apply(&mut midi, 0, 96, 0);
        assert_eq!(midi.mpe.manager(0).unwrap().manager_range.unwrap().semitones(), 2.01);
        assert_eq!(midi.mpe.member(2).unwrap().member_range.semitones(), 13.);
        apply(&mut midi, 14, 6, 96);apply(&mut midi, 14, 96, 1);
        assert_eq!(midi.mpe.member(14).unwrap().member_range.semitones(), 96.);
        apply(&mut midi, 14, 97, 127);
        assert_eq!(midi.mpe.member(14).unwrap().member_range.semitones(), 96., "relative LSB1 returns to0 without borrowing");
        apply(&mut midi, 14, 97, 127);
        assert_eq!(midi.mpe.member(14).unwrap().member_range.semitones(), 95.99);
        assert_eq!(midi.mpe.member(2).unwrap().member_range.semitones(), 13.);
    }

    #[test]
    fn ordinary_midi1_ranges_use_one_physical_decoder_and_preserve_configuration() {
        let mut midi = MidiState::default();
        let apply = |midi: &mut MidiState, channel, cc, value| {
            midi.receive_midi1_cc(channel, cc, value).unwrap().parameter
                .is_some_and(|event| midi.apply_midi1_parameter(channel, event))
        };
        for channel in [4, 7] {
            assert!(!apply(&mut midi, channel, 101, 0));
            assert!(!apply(&mut midi, channel, 100, 0));
            assert!(!apply(&mut midi, channel, 38, 25), "unknown coarse value must retain authored fallback");
        }
        assert_eq!(ranges(&midi), [None; 16]);
        assert!(apply(&mut midi, 4, 6, 12));
        assert!(apply(&mut midi, 4, 38, 50));
        assert!(apply(&mut midi, 7, 6, 6));
        assert!(apply(&mut midi, 7, 38, 25));
        assert_eq!((ranges(&midi)[4], ranges(&midi)[7]), (Some(12.5), Some(6.25)));
        assert!(apply(&mut midi, 4, 38, 100));
        assert_eq!(ranges(&midi)[4], Some(13.));
        assert!(apply(&mut midi, 4, 38, 127));
        assert_eq!(ranges(&midi)[4], Some(13.27));
        let revision = midi.revision;
        assert!(midi.receive_midi1_cc(4, 38, 128).is_none());
        assert_eq!(midi.revision, revision, "only out-of-domain data bytes are rejected");
        assert!(apply(&mut midi, 4, 6, 127));
        assert!(apply(&mut midi, 4, 38, 127));
        assert_eq!(ranges(&midi)[4], Some(128.27));
        midi.reset_received_controllers(1 << 4);
        midi.reset_controllers(4);
        assert_eq!(ranges(&midi)[4], Some(128.27));
        assert!(!apply(&mut midi, 4, 38, 25), "CC121 clears selectors rather than inventing a new transaction");
        assert!(!apply(&mut midi, 4, 101, 0));
        assert!(!apply(&mut midi, 4, 100, 0));
        assert!(apply(&mut midi, 4, 38, 50), "known configuration supplies coarse data after transaction reset");
        assert_eq!(ranges(&midi)[4], Some(127.5));
        assert_eq!(ranges(&midi)[7], Some(6.25));
        let mut mpe = midi.mpe;
        mpe.configure(0, 7, 48).unwrap();midi.set_mpe(mpe);
        let range = ranges(&midi)[7];
        assert!(!apply(&mut midi, 7, 6, 2), "active MPE members retain zone negotiation semantics");
        assert_eq!(ranges(&midi)[7], range);
    }

    #[test]
    fn entering_mpe_replaces_only_ordinary_midi1_ranges_with_zone_sensitivity() {
        use crate::midi::{NativeParameter, ParameterData};
        let mut midi = MidiState::default();
        let ordinary = ParameterEvent { registered: true, address: 0,
            data: ParameterData::Entry { msb: Some(12), lsb: 25, fine: true } };
        for channel in [4, 7, 10] { assert!(midi.apply_midi1_parameter(channel, ordinary)); }
        assert!(midi.apply_native_parameter(7, NativeParameter { registered: true, address: 0,
            data: crate::midi::NativeParameterData::Absolute((24 << 25) | (50 << 18)) }));
        let mut mpe = midi.mpe;mpe.configure(0, 7, 48).unwrap();midi.set_mpe(mpe);
        assert_eq!(ranges(&midi)[4], None, "MIDI1 member starts with mandated zone range");
        assert_eq!(ranges(&midi)[7], Some(24.5), "native physical override is independent of zones");
        assert_eq!(ranges(&midi)[10], Some(12.25), "unrelated ordinary channel stays configured");
        mpe.configure(0, 3, 48).unwrap();midi.set_mpe(mpe);
        assert_eq!(ranges(&midi)[7], Some(24.5), "leaving a zone does not reset native configuration");
        assert!(midi.apply_midi1_parameter(4, ordinary));
        midi.reset_controllers(4);midi.reset();
        assert_eq!(ranges(&midi)[4], Some(12.25));
        mpe.configure(0, 7, 48).unwrap();midi.set_mpe(mpe);
        assert_eq!(ranges(&midi)[4], None, "re-entry replaces the retained ordinary MIDI1 range");
        assert_eq!(ranges(&midi)[7], Some(24.5));
    }

    #[test]
    fn native_ranges_preserve_wire_formats_channel_scope_and_reset_configuration() {
        use crate::midi::NativeParameter;
        let mut midi = MidiState::default();
        let parameter = |address, value| NativeParameter { registered: true, address, data: crate::midi::NativeParameterData::Absolute(value) };
        assert_eq!(ranges(&midi), [None; 16]);
        assert!(midi.apply_native_parameter(4, parameter(0, (12 << 25) | (100 << 18))));
        assert_eq!(ranges(&midi)[4], Some(13.));
        assert!(midi.apply_native_parameter(4, parameter(0, (127 << 25) | (127 << 18) | 0x3ffff)));
        assert_eq!(ranges(&midi)[4], Some(128.27));
        assert_eq!(ranges(&midi)[7], None);
        assert!(midi.apply_native_parameter(7, parameter(0, (12 << 25) | (25 << 18))));
        assert_eq!(ranges(&midi)[7], Some(12.25));
        let revision = midi.revision;
        for (channel, event) in [
            (16, parameter(7, 1)),
            (4, parameter(6, 1)),
            (4, NativeParameter { registered: false, address: 7, data: crate::midi::NativeParameterData::Absolute(1) }),
        ] {
            assert!(!midi.apply_native_parameter(channel, event));
        }
        assert_eq!(midi.revision, revision);
        assert_eq!(ranges(&midi)[4], Some(128.27));
        assert!(midi.apply_native_parameter(4, parameter(7, (12 << 25) | 1)));
        assert_eq!(midi.per_note_range[4], 12. + 1. / 33554432.);
        assert_eq!(midi.per_note_range[7], 48.);
        assert!(midi.apply_native_parameter(7, parameter(7, u32::MAX)));
        assert_eq!(midi.per_note_range[7], 128. - 1. / 33554432.);
        assert!(midi.apply_native_parameter(0, parameter(0, 0)));
        assert!(midi.apply_native_parameter(0, parameter(7, 0)));
        assert_eq!((ranges(&midi)[0], midi.per_note_range[0]), (Some(0.), 0.));
        let (channel, per_note) = (ranges(&midi), midi.per_note_range);
        midi.reset_controllers(4);
        midi.reset();
        assert_eq!((ranges(&midi), midi.per_note_range), (channel, per_note));
    }

    #[test]
    fn canonical_rows_isolate_physical_controls_and_broadcast_authored_values() {
        let mut midi = MidiState::default();
        let (a, b) = (ControlValue::new(0.5001).unwrap(), ControlValue::new(0.5030).unwrap());
        assert_eq!(a.midi1(), b.midi1());
        assert_eq!(crate::plugin::tests::allocations(|| {
            assert!(midi.accept(ControlTarget::Physical(2), Control::Cc(1, a)));
            assert!(midi.accept(ControlTarget::Physical(5), Control::Cc(1, b)));
        }), 0);
        assert_eq!((midi.cc[2][1], midi.cc[5][1], midi.cc[0][1]), (a, b, ControlValue::ZERO));
        let value = ControlValue::new(0.751).unwrap();
        assert!(midi.accept(ControlTarget::Authored, Control::Cc(1, value)));
        assert!(midi.cc.iter().all(|row| row[1] == value));
        assert!(midi.accept(ControlTarget::Physical(2), Control::Cc(1, a)));
        assert_eq!(midi.cc[5][1], value);
        assert!(midi.accept(ControlTarget::Physical(2), Control::Bend(Bend::new(-0.3333).unwrap())));
        assert!(midi.accept(ControlTarget::Authored, Control::Pressure(b)));
        assert_eq!(midi.bend[5], Bend::ZERO);
        assert!(midi.pressure.iter().all(|pressure| *pressure == b));
        let before = (midi.cc, midi.bend, midi.pressure, midi.revision);
        assert!(!midi.accept(ControlTarget::Physical(16), Control::Pressure(a)));
        assert!(!midi.accept(ControlTarget::Physical(255), Control::Bend(Bend::ZERO)));
        assert!(!midi.accept(ControlTarget::Authored, Control::Cc(128, a)));
        assert_eq!((midi.cc, midi.bend, midi.pressure, midi.revision), before);
        let projected = midi.midi1_cc();
        assert_eq!((projected[2][1], projected[5][1]), (a.midi1(), value.midi1()));
    }

    #[test]
    fn applied_reset_retains_setup_and_physical_transactions_reset_only_selected_inputs() {
        let mut midi = MidiState::default();
        midi.set_defaults(&[(3, 37), (7, 22)]);
        midi.mpe.configure(0, 1, 48);
        let mpe = midi.mpe;
        midi.reset();
        midi.accept(ControlTarget::Physical(0), Control::Cc(7, ControlValue::from_midi1(11)));
        midi.accept(ControlTarget::Physical(0), Control::Cc(3, ControlValue::from_midi1(99)));
        midi.accept(ControlTarget::Physical(0), Control::Bend(Bend::new(0.5).unwrap()));
        midi.accept(ControlTarget::Physical(0), Control::Pressure(ControlValue::from_midi1(63)));
        midi.set_expression(
            0,
            60,
            Expression {
                note_cc74: Some(crate::midi::ControlValue::from_midi1(45)),
                ..Expression::default()
            },
        );
        for input in [2, 3] {
            midi.receive_cc(input, 101, 0);
            midi.receive_cc(input, 100, input);
        }
        let revision = midi.revision;
        assert_eq!(
            crate::plugin::tests::allocations(|| {
                midi.reset_controllers(0);
                midi.reset_received(1 << 2);
            }),
            0
        );
        assert_eq!((midi.cc[0][3].midi1(), midi.cc[0][7].midi1()), (37, 11));
        assert_eq!((midi.bend[0], midi.pressure[0]), (Bend::ZERO, ControlValue::ZERO));
        assert_eq!(midi.expression[0][60].note_cc74, Some(crate::midi::ControlValue::from_midi1(45)));
        assert_eq!(midi.receive_cc(2, 6, 7), None);
        assert_eq!(
            midi.receive_cc(3, 6, 7),
            Some(ParameterValue {
                registered: true,
                address: 3,
                value: 7 << 7
            })
        );
        assert_eq!(
            midi.cc[0][6], ControlValue::ZERO,
            "received input does not bypass script acceptance"
        );
        assert_eq!(crate::plugin::tests::allocations(|| midi.reset()), 0);
        assert_eq!(midi.revision, revision.wrapping_add(1));
        assert_eq!((midi.cc[0][3].midi1(), midi.cc[0][7].midi1()), (0, 127));
        assert_eq!(midi.expression[0][60].note_cc74, None);
        assert_eq!(midi.controller_defaults[3], Some(37));
        assert_eq!(midi.mpe, mpe);
    }
}
