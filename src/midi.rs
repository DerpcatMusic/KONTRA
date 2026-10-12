//! Transport-independent controller policy shared by native playback and scripts.

/// Native MIDI2 uses unified bank/parameter messages instead of these MIDI1 CCs.
pub(crate) const fn ignored_midi2_cc(cc: u8) -> bool {
    matches!(cc, 0 | 6 | 32 | 38 | 98..=101)
}

/// CC121's reset value, or `None` when the controller retains its value.
/// Standard defaults follow MIDI RP-015; parameter settings are preserved.
/// Extra controllers restore authored device defaults. The internal bend and
/// pressure slots (128 and 129) use zero, like their MIDI message counterparts.
/// https://midi.org/response-to-reset-all-controllers
pub(crate) fn reset_controller_value(cc: u8, initial: Option<u8>) -> Option<u8> {
    match cc {
        0 | 7 | 10 | 32 | 39 | 42 | 70..=79 | 91..=95 | 120..=127 => None,
        11 | 98..=101 => Some(127),
        1 | 64..=67 | 128 | 129 => Some(0),
        2..=119 => Some(initial.unwrap_or(0)),
        _ => None,
    }
}

/// A received parameter operation; interpretation belongs to the parameter owner.
/// Validated musical velocity. MIDI byte projections are compatibility views;
/// the original normalized value remains available through playback.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Velocity {
    value: f64,
    projection: u8,
}

impl Velocity {
    pub const ZERO: Self = Self { value: 0., projection: 0 };
    pub const FULL: Self = Self { value: 1., projection: 127 };

    pub fn new(value: f64) -> Option<Self> {
        (value.is_finite() && (0.0..=1.0).contains(&value))
            .then_some(Self { value, projection: (value * 127.).round() as u8 })
    }

    pub fn from_midi1(value: u8) -> Self {
        let projection = value.min(127);
        Self { value: f64::from(projection) / 127., projection }
    }

    /// CA-031 keeps the original upper seven bits for scripts and zone lookup.
    pub fn from_midi14(value: u16) -> Self {
        let value = value.min(16383);
        Self { value: f64::from(value) / 16383., projection: (value >> 7) as u8 }
    }

    pub fn from_midi2(value: u16) -> Self {
        Self { value: f64::from(value) / f64::from(u16::MAX), projection: (value >> 9) as u8 }
    }

    pub fn midi1(self) -> u8 { self.projection }
    pub fn dsp(self) -> f32 { self.value as f32 }
    pub fn normalized(self) -> f64 { self.value }
}

/// Validated normalized continuous control, retaining the original precision.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct ControlValue {
    value: f64,
    projection: u8,
    resolution: ControlResolution,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
enum ControlResolution {
    #[default]
    Normalized,
    Midi1,
    PairedMidi1,
    Midi2,
}

impl ControlValue {
    pub const ZERO: Self = Self { value: 0., projection: 0, resolution: ControlResolution::Normalized };
    pub const FULL: Self = Self { value: 1., projection: 127, resolution: ControlResolution::Normalized };
    pub fn new(value: f64) -> Option<Self> {
        (value.is_finite() && (0.0..=1.0).contains(&value))
            .then_some(Self { value, projection: (value * 127.).round() as u8, resolution: ControlResolution::Normalized })
    }
    pub fn from_midi1(value: u8) -> Self {
        let projection = value.min(127);
        match projection {
            0 => Self::ZERO,
            127 => Self::FULL,
            _ => Self { value: f64::from(projection) / 127., projection, resolution: ControlResolution::Midi1 },
        }
    }
    pub fn from_midi14(value: u16) -> Self {
        let value = value.min(16383);
        match value {
            0 => Self::ZERO,
            16383 => Self::FULL,
            _ => Self { value: f64::from(value) / 16383., projection: (value >> 7) as u8, resolution: ControlResolution::PairedMidi1 },
        }
    }
    pub fn from_midi2(value: u32) -> Self {
        match value {
            0 => Self::ZERO,
            u32::MAX => Self::FULL,
            _ => Self { value: f64::from(value) / f64::from(u32::MAX), projection: (value >> 25) as u8, resolution: ControlResolution::Midi2 },
        }
    }
    pub fn midi1(self) -> u8 { self.projection }
    pub fn dsp(self) -> f32 { self.value as f32 }
    pub fn normalized(self) -> f64 { self.value }

    /// Channel pan: preserve MIDI1's existing mixer law. Higher resolutions
    /// retain their protocol center (M2-115 Min-Center-Max); this mixer maps
    /// each side linearly to its endpoint. Authored CC routes still use dsp().
    pub(crate) fn pan(self) -> f32 {
        let centered = |maximum: f64, center: f64| {
            let raw = (self.value * maximum).round();
            ((raw - center) / if raw < center { center } else { maximum - center }) as f32
        };
        match self.resolution {
            ControlResolution::Midi1 => ((self.dsp() * 127. - 64.) / 63.).clamp(-1., 1.),
            ControlResolution::PairedMidi1 => centered(16383., 8192.),
            ControlResolution::Midi2 => centered(4294967295., 2147483648.),
            ControlResolution::Normalized => (self.value * 2. - 1.) as f32,
        }
    }
}

/// Signed normalized bend. Wire centers and existing endpoint mapping remain exact.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bend {
    value: f64,
    projection: u16,
}

impl Bend {
    pub const ZERO: Self = Self { value: 0., projection: 8192 };
    pub fn new(value: f64) -> Option<Self> {
        (value.is_finite() && (-1.0..=1.0).contains(&value)).then_some(Self {
            value, projection: (value * 8192. + 8192.).round().clamp(0., 16383.) as u16,
        })
    }
    pub fn from_midi1(value: u16) -> Self {
        let projection = value.min(16383);
        Self { value: (f64::from(projection) - 8192.) / 8192., projection }
    }
    pub fn from_midi2(value: u32) -> Self {
        Self { value: (f64::from(value) - 2147483648.) / 2147483648., projection: (value >> 18) as u16 }
    }
    pub fn midi1(self) -> u16 { self.projection }
    pub fn dsp(self) -> f32 { self.value as f32 }
    pub fn normalized(self) -> f64 { self.value }
}

/// Accepted continuous control; legacy script slot numbers stay at its boundary.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Control {
    Cc(u8, ControlValue),
    /// Raw callback source and the captured paired-controller default action.
    Cc14 { cc: u8, raw: u8, coarse: u8, value: ControlValue },
    Bend(Bend),
    Pressure(ControlValue),
    /// CC121's synthetic pressure default retains neutral native MPE gain.
    /// An explicitly received or authored Pressure(ZERO) remains active.
    ResetPressure,
}

impl Control {
    pub fn valid(self) -> bool {
        match self {
            Self::Cc(cc, _) => cc < 128,
            Self::Cc14 { cc, raw, coarse, .. } => coarse < 32 && !matches!(coarse, 0 | 6)
                && raw < 128 && (cc == coarse || cc == coarse + 32),
            _ => true,
        }
    }

    /// Legacy callback projection; aliases belong to this boundary, not state.
    pub fn legacy(self) -> (u8, i32) {
        match self {
            Self::Cc(cc, value) => (cc, i32::from(value.midi1())),
            Self::Cc14 { cc, raw, .. } => (cc, i32::from(raw)),
            Self::Bend(value) => (128, i32::from(value.midi1()) - 8192),
            Self::Pressure(value) => (129, i32::from(value.midi1())),
            Self::ResetPressure => (129, 0),
        }
    }

    /// Reset policy follows the combined controller, not its fine-byte callback.
    pub fn reset_controller_number(self) -> u8 {
        match self { Self::Cc14 { coarse, .. } => coarse, _ => self.legacy().0 }
    }

    pub fn from_legacy(cc: u8, value: i32) -> Option<Self> {
        Some(match cc {
            0..=127 => Self::Cc(cc, ControlValue::from_midi1(value.clamp(0, 127) as u8)),
            128 => Self::Bend(Bend::from_midi1((value.clamp(-8192, 8191) + 8192) as u16)),
            129 => Self::Pressure(ControlValue::from_midi1(value.clamp(0, 127) as u8)),
            _ => return None,
        })
    }
}

/// Native per-note data retains its wire precision through authored callbacks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NoteControl {
    Registered { index: u8, value: u32 },
    Assignable { index: u8, value: u32 },
    Pitch(Bend),
}

impl NoteControl {
    pub fn script_controller(self) -> i32 {
        match self {
            Self::Registered { index, .. } => i32::from(index),
            Self::Assignable { index, .. } => 256 + i32::from(index),
            Self::Pitch(_) => 512,
        }
    }

    pub fn script_value(self) -> i32 {
        match self {
            Self::Registered { value, .. } | Self::Assignable { value, .. } => (value >> 25) as i32,
            Self::Pitch(value) => i32::from(value.midi1()) - 8192,
        }
    }
}

/// Mutation scope is separate from a callback's physical cleanup provenance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlTarget {
    Physical(u8),
    Authored,
}

/// A received control retains its canonical value and decoded parameter until
/// routing delivers it. Native controls and synthetic resets have no parameter.
#[derive(Debug, PartialEq)]
pub(crate) struct ReceivedControl {
    pub control: Control,
    pub parameter: Option<ParameterEvent>,
}

impl ReceivedControl {
    pub(crate) fn without_parameter(control: Control) -> Self {
        Self { control, parameter: None }
    }
}

/// One-shot CA-031 note velocity state. Range acceptance copies only this
/// owner; it never projects controller values or the parameter decoder.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct VelocityPrefix(Option<u8>);

impl VelocityPrefix {
    pub(crate) fn receive(&mut self, raw: u8) { if raw < 128 { self.0 = Some(raw); } }
    pub(crate) fn reset(&mut self) { self.0 = None; }
    pub(crate) fn velocity(&mut self, raw: u8) -> Option<Velocity> {
        if raw >= 128 { return None; }
        let prefix = self.0.take()?;
        (raw != 0).then(|| Velocity::from_midi14((u16::from(raw) << 7) | u16::from(prefix)))
    }
    pub(crate) fn release_velocity(&mut self, raw: u8) -> Option<Velocity> {
        if raw >= 128 { return None; }
        Some(Velocity::from_midi14((u16::from(raw) << 7) | u16::from(self.0.take()?)))
    }
}

/// Physical wire state. Authored/synthetic writes do not enter this decoder.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Midi1Decoder {
    parameter: ParameterDecoder,
    coarse: [Option<u8>; 32],
    paired: u32,
    pub(crate) velocity_prefix: VelocityPrefix,
}

impl Midi1Decoder {
    pub fn cc(&mut self, cc: u8, raw: u8) -> Option<ReceivedControl> {
        if cc >= 128 || raw >= 128 { return None; }
        let parameter = self.parameter.cc(cc, raw);
        let mut control = Control::Cc(cc, ControlValue::from_midi1(raw));
        if cc == 88 { self.velocity_prefix.receive(raw); }
        let coarse = cc & 31;
        if cc < 64 && !matches!(coarse, 0 | 6) {
            if cc < 32 { self.coarse[coarse as usize] = Some(raw); }
            else if self.coarse[coarse as usize].is_some() { self.paired |= 1 << coarse; }
            if self.paired & (1 << coarse) != 0 && let Some(msb) = self.coarse[coarse as usize] {
                let word = (u16::from(msb) << 7) | if cc < 32 { 0 } else { u16::from(raw) };
                control = Control::Cc14 { cc, raw, coarse, value: ControlValue::from_midi14(word) };
            }
        }
        Some(ReceivedControl { control, parameter })
    }

    pub fn parameter(&mut self, cc: u8, raw: u8) -> Option<ParameterEvent> {
        self.parameter.cc(cc, raw)
    }

    /// CA-031 consumes the prefix even for the zero-velocity Note On shortcut,
    /// whose prefix has no effect. Native MIDI2 notes never call this method.
    pub fn velocity(&mut self, raw: u8) -> Option<Velocity> {
        self.velocity_prefix.velocity(raw)
    }

    /// Explicit Note Off velocity zero is valid and can receive the prefix.
    pub fn release_velocity(&mut self, raw: u8) -> Option<Velocity> {
        self.velocity_prefix.release_velocity(raw)
    }

    pub fn reset_controllers(&mut self, defaults: &[Option<u8>; 128]) {
        self.parameter.reset();
        self.velocity_prefix.reset();
        for cc in 0..32u8 {
            if let Some(value) = reset_controller_value(cc, defaults[cc as usize]) {
                self.coarse[cc as usize] = Some(value);
                self.paired &= !(1 << cc);
            }
        }
    }

    pub fn seed_registered_parameter(&mut self, address: u16, msb: u8) {
        self.parameter.seed_registered_parameter(address, msb);
    }

    pub fn reset(&mut self) { *self = Self::default(); }
}

/// Native MIDI2 channel parameter intent, retaining its wire precision and kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeParameter {
    pub registered: bool,
    pub address: u16,
    pub data: NativeParameterData,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeParameterData {
    Absolute(u32),
    Relative(i32),
}

impl NativeParameter {
    pub fn wire_value(self) -> u32 {
        match self.data { NativeParameterData::Absolute(value) => value, NativeParameterData::Relative(delta) => delta as u32 }
    }

    /// Local KSP projection, not a wire translator. Relative operations retain
    /// the signed original delta and never manufacture an absolute callback.
    pub fn script_value(self) -> i32 {
        match self.data { NativeParameterData::Absolute(value) => (value >> 18) as i32, NativeParameterData::Relative(delta) => delta }
    }

    pub fn delta(self) -> Option<i32> {
        match self.data { NativeParameterData::Relative(delta) => Some(delta), _ => None }
    }
}

/// Physical channel RPN1/2 configuration. Native fine tuning retains all 32
/// bits; MIDI1's fixed-point value is zero-extended (M2-115 §4.1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ChannelTuning {
    fine: u32,
    coarse: u8,
    native_fine: bool,
    ratio: f64,
}

impl Default for ChannelTuning {
    fn default() -> Self {
        // Neutral receiver policy, independent of authored instrument tuning.
        Self { fine: 0x80000000, coarse: 64, native_fine: false, ratio: 1. }
    }
}

impl ChannelTuning {
    pub fn ratio(self) -> f64 { self.ratio }

    fn update_ratio(&mut self) {
        let semitones = (f64::from(self.fine) - 2147483648.) / 2147483648.
            + f64::from(self.coarse) - 64.;
        self.ratio = 2f64.powf(semitones / 12.);
    }

    /// RP-018 ignores the value byte: fine steps by 1/8192 semitone, coarse
    /// by one semitone. MIDI1 saturation follows the most recent fine message's
    /// resolution; native relative edits also establish full-resolution state.
    pub fn parameter(&mut self, address: u16, data: ParameterData) -> bool {
        if !matches!(address, 1 | 2) { return false; }
        if let ParameterData::Entry { msb, lsb, .. } = data
            && (lsb >= 128 || msb.is_some_and(|value| value >= 128)) { return false; }
        match (address, data) {
            (1, ParameterData::Entry { msb, lsb, fine }) => {
                let coarse = if fine { self.msb(1) } else { msb.unwrap_or(self.msb(1)) };
                self.fine = ((u32::from(coarse) << 7) | u32::from(if fine { lsb } else { 0 })) << 18;
                self.native_fine = false;
            }
            (1, ParameterData::Increment) => {
                let maximum = if self.native_fine { u32::MAX } else { 16383 << 18 };
                self.fine = self.fine.saturating_add(1 << 18).min(maximum);
            }
            (1, ParameterData::Decrement) => self.fine = self.fine.saturating_sub(1 << 18),
            (2, ParameterData::Entry { msb, fine: false, .. }) => self.coarse = msb.unwrap_or(self.coarse),
            (2, ParameterData::Entry { fine: true, .. }) => {}, // RPN2 ignores the LSB.
            (2, ParameterData::Increment) => self.coarse = self.coarse.saturating_add(1).min(127),
            (2, ParameterData::Decrement) => self.coarse = self.coarse.saturating_sub(1),
            _ => unreachable!(),
        }
        self.update_ratio();
        true
    }

    pub fn native(&mut self, address: u16, value: u32) -> bool {
        match address {
            1 => { self.fine = value; self.native_fine = true; }
            2 => self.coarse = (value >> 25) as u8,
            _ => return false,
        }
        self.update_ratio();
        true
    }

    /// UMP §7.4.8 uses a signed delta in the property's native scale. Coarse
    /// tuning discards sub-semitone deltas toward zero without accumulating
    /// them (the low-resolution receiver policy allowed by M2-115 §2.4).
    pub fn native_relative(&mut self, address: u16, delta: i32) -> bool {
        match address {
            1 => { self.fine = self.fine.saturating_add_signed(delta); self.native_fine = true; }
            2 => self.coarse = (i32::from(self.coarse) + delta / 33554432).clamp(0, 127) as u8,
            _ => return false,
        }
        self.update_ratio();
        true
    }

    pub fn msb(self, address: u16) -> u8 { if address == 1 { (self.fine >> 25) as u8 } else { self.coarse } }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ParameterEvent {
    pub registered: bool,
    pub address: u16,
    pub data: ParameterData,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ParameterValue {
    pub registered: bool,
    pub address: u16,
    pub value: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ParameterData {
    /// `fine` identifies CC38. An unknown coarse value is not silently zeroed.
    Entry {
        msb: Option<u8>,
        lsb: u8,
        fine: bool,
    },
    Increment,
    Decrement,
}

impl ParameterData {
    /// Local endpoint policy when the updated cents field cannot fit seven bits.
    pub fn bend_step(self, current: f64, maximum: f64) -> Option<f64> {
        let step = match self { Self::Increment => 1., Self::Decrement => -1., _ => return None };
        Some(((current * 100.).round() + step).clamp(0., (maximum * 100.).round()) / 100.)
    }

    /// RP-018 changes the supported cents LSB, retaining the semitone MSB.
    /// Only an unrepresentable LSB keeps the existing numeric endpoint policy.
    pub fn bend_fields(self, coarse: u8, fine: u8, maximum: f64) -> Option<(u8, u8)> {
        let step = match self { Self::Increment => 1, Self::Decrement => -1, _ => return None };
        let next = i16::from(fine) + step;
        if (0..128).contains(&next) { return Some((coarse, next as u8)); }
        let range = self.bend_step(f64::from(coarse) + f64::from(fine) / 100., maximum)?;
        let cents = (range * 100.).round() as u16;
        let coarse = (cents / 100).min(127) as u8;
        Some((coarse, (cents - u16::from(coarse) * 100) as u8))
    }
}

impl ParameterEvent {
    pub fn absolute(self) -> Option<ParameterValue> {
        Some(ParameterValue {
            registered: self.registered,
            address: self.address,
            value: self.value()?,
        })
    }

    /// A complete data-entry value, suitable for KSP's 14-bit parameter value.
    pub fn value(self) -> Option<u16> {
        match self.data {
            ParameterData::Entry {
                msb: Some(msb),
                lsb,
                ..
            } => Some((u16::from(msb) << 7) | u16::from(lsb)),
            _ => None,
        }
    }
}

/// One physical input channel's bounded, allocation-free RPN/NRPN decoder.
/// It tracks the active transaction, not every parameter's effective setting.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ParameterDecoder {
    rpn: [Option<u8>; 2],
    nrpn: [Option<u8>; 2],
    registered: Option<bool>,
    msb: Option<u8>,
}

impl ParameterDecoder {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    fn seed_registered_parameter(&mut self, address: u16, msb: u8) {
        if msb < 128 && self.selection() == Some((true, address)) { self.msb = Some(msb); }
    }

    fn selection(&self) -> Option<(bool, u16)> {
        let registered = self.registered?;
        let [Some(msb), Some(lsb)] = (if registered { self.rpn } else { self.nrpn }) else {
            return None;
        };
        let address = (u16::from(msb) << 7) | u16::from(lsb);
        (address != 16383).then_some((registered, address))
    }

    /// Decode received CC only. Authored `set_rpn/nrpn` keeps its own path.
    /// CC6 clears its fine byte (MIDI 1.0 Detailed Specification, Control Change).
    /// Relative intents ignore the value byte; RP-018 defines parameter-specific
    /// step sizes, including manufacturer-defined NRPN steps.
    pub fn cc(&mut self, cc: u8, value: u8) -> Option<ParameterEvent> {
        if cc >= 128 || value >= 128 {
            return None;
        }
        match cc {
            98..=101 => {
                let before = self.selection();
                let registered = cc >= 100;
                self.registered = Some(registered);
                let address = if registered {
                    &mut self.rpn
                } else {
                    &mut self.nrpn
                };
                address[usize::from(cc & 1 == 0)] = Some(value);
                if self.selection() != before {
                    self.msb = None;
                }
                return None;
            }
            121 => {
                self.reset();
                return None;
            }
            _ => {}
        }
        let (registered, address) = self.selection()?;
        let data = match cc {
            6 => {
                self.msb = Some(value);
                ParameterData::Entry {
                    msb: self.msb,
                    lsb: 0,
                    fine: false,
                }
            }
            38 => ParameterData::Entry {
                msb: self.msb,
                lsb: value,
                fine: true,
            },
            96 => ParameterData::Increment,
            97 => ParameterData::Decrement,
            _ => return None,
        };
        Some(ParameterEvent {
            registered,
            address,
            data,
        })
    }
}

/// MPE's chosen receiver limit is 96 semitones; fine sensitivity is in cents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BendRange {
    pub coarse: u8,
    pub fine: u8,
}

impl BendRange {
    pub fn semitones(self) -> f64 {
        (f64::from(self.coarse) + f64::from(self.fine) / 100.).min(96.)
    }

    fn parameter(self, data: ParameterData) -> Option<Self> {
        if let ParameterData::Entry { msb, lsb, fine } = data { return Some(self.entry(msb, lsb, fine)); }
        let (coarse, fine) = data.bend_fields(self.coarse, self.fine, 96.)?;
        Some(Self { coarse, fine })
    }

    fn entry(self, msb: Option<u8>, lsb: u8, fine: bool) -> Self {
        if fine {
            Self {
                coarse: msb.unwrap_or(self.coarse).min(96),
                fine: lsb,
            }
        } else {
            Self {
                coarse: msb.unwrap_or(self.coarse).min(96),
                fine: 0,
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MpeZone {
    pub manager: u8,
    pub members: u8,
    pub member_range: BendRange,
    /// None preserves the library's pitch sensitivity for manually configured MPE.
    pub manager_range: Option<BendRange>,
}

impl MpeZone {
    pub fn member_mask(self) -> u16 {
        let low = if self.manager == 0 {
            1
        } else {
            15 - self.members
        };
        (((1u32 << self.members) - 1) << low) as u16
    }

    pub fn channels(self) -> u16 {
        if self.members == 0 {
            0
        } else {
            self.member_mask() | (1 << self.manager)
        }
    }
}

/// Channels changing zone or manager/member role require lifecycle cleanup
/// before publishing this state. Range updates also refresh existing voices.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct MpeChange {
    pub channels: u16,
    pub ranges: u16,
}

/// Effective lower and upper zones, independent of authored routing settings.
/// MPE v1.1 §§2.2.1/2.2.3/2.2.5 define overlap, lifecycle and sensitivity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MpeZones {
    zones: [MpeZone; 2],
}

impl Default for MpeZones {
    fn default() -> Self {
        Self {
            zones: [0, 15].map(|manager| MpeZone {
                manager,
                members: 0,
                member_range: BendRange {
                    coarse: 48,
                    fine: 0,
                },
                manager_range: None,
            }),
        }
    }
}

impl MpeZones {
    pub fn zones(self) -> [MpeZone; 2] {
        self.zones
    }

    /// The active zone governing this channel, including its manager.
    pub fn zone(self, channel: u8) -> Option<MpeZone> {
        if channel >= 16 {
            return None;
        }
        self.zones
            .into_iter()
            .find(|zone| zone.channels() & (1 << channel) != 0)
    }

    pub fn member(self, channel: u8) -> Option<MpeZone> {
        self.zone(channel).filter(|zone| zone.manager != channel)
    }

    pub fn manager(self, channel: u8) -> Option<MpeZone> {
        self.zone(channel).filter(|zone| zone.manager == channel)
    }

    /// Apply an authored zone configuration. Wire negotiation uses `parameter`
    /// and resets the addressed zone's sensitivities to the mandated ±2/±48.
    pub fn configure(&mut self, manager: u8, members: u8, member_range: u8) -> Option<MpeChange> {
        let index = Self::index(manager)?;
        if members > 15 {
            return None;
        }
        let before = *self;
        self.set_members(index, members);
        self.zones[index].member_range = BendRange {
            coarse: member_range.min(96),
            fine: 0,
        };
        self.zones[index].manager_range = None;
        Some(self.change(before))
    }

    /// Consume the shared received decoder's event, never script-authored RPN.
    /// Fine-only RPN0 uses this parameter's effective coarse value; generic
    /// parameter callbacks still require their own complete absolute value.
    pub fn parameter(&mut self, channel: u8, event: ParameterEvent) -> Option<MpeChange> {
        if !event.registered {
            return None;
        }
        let before = *self;
        match event.address {
            6 => {
                let ParameterData::Entry { msb: Some(members), fine: false, .. } = event.data else { return None; };
                let index = Self::index(channel)?;
                if members > 15 {
                    return None;
                }
                self.set_members(index, members);
                self.zones[index].member_range = BendRange {
                    coarse: 48,
                    fine: 0,
                };
                self.zones[index].manager_range = Some(BendRange { coarse: 2, fine: 0 });
            }
            0 => {
                let zone = self.zone(channel)?;
                let index = Self::index(zone.manager)?;
                if zone.manager == channel {
                    let range = zone
                        .manager_range
                        .unwrap_or(BendRange { coarse: 2, fine: 0 });
                    self.zones[index].manager_range = Some(range.parameter(event.data)?);
                } else {
                    self.zones[index].member_range = zone.member_range.parameter(event.data)?;
                }
            }
            _ => return None,
        }
        Some(self.change(before))
    }

    fn index(manager: u8) -> Option<usize> {
        match manager {
            0 => Some(0),
            15 => Some(1),
            _ => None,
        }
    }

    fn set_members(&mut self, index: usize, members: u8) {
        self.zones[index].members = members;
        // Both managers reserve a channel when both zones are active. A new
        // 14/15-member zone therefore removes the other zone entirely.
        if members > 0 {
            let available = 14u8.saturating_sub(members);
            self.zones[1 - index].members = self.zones[1 - index].members.min(available);
        }
    }

    fn change(self, before: Self) -> MpeChange {
        let mut change = MpeChange::default();
        for channel in 0..16 {
            let old = before.zone(channel);
            let new = self.zone(channel);
            if old.map(|z| (z.manager, z.manager == channel))
                != new.map(|z| (z.manager, z.manager == channel))
            {
                change.channels |= 1 << channel;
            }
            if old != new {
                change.ranges |= 1 << channel;
            }
        }
        change
    }
}

#[cfg(test)]
mod performance_tests {
    use super::*;

    #[test]
    fn velocity_keeps_original_precision_and_validates_musical_endpoints() {
        for value in [0., 0.123456789012345, 0.5, 1.] {
            let velocity = Velocity::new(value).unwrap();
            assert_eq!(velocity.normalized(), value);
            assert_eq!(velocity.dsp(), value as f32);
        }
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.01, 1.01] {
            assert_eq!(Velocity::new(value), None);
        }
        for value in 0..=127 {
            assert_eq!(Velocity::from_midi1(value).midi1(), value);
        }
        assert_eq!(Velocity::from_midi1(255).normalized(), 1.);
        assert_eq!(Velocity::from_midi2(0).normalized(), 0.);
        assert_eq!(Velocity::from_midi2(u16::MAX).normalized(), 1.);
        let a = Velocity::from_midi2(32768);
        let b = Velocity::from_midi2(32769);
        assert_eq!(a.midi1(), b.midi1());
        assert_ne!(a.normalized(), b.normalized());
        assert_ne!(a.dsp(), b.dsp());
    }

    fn select(decoder: &mut ParameterDecoder, address: u8) {
        decoder.cc(101, 0);
        decoder.cc(100, address);
    }

    fn received(
        zones: &mut MpeZones,
        decoder: &mut ParameterDecoder,
        channel: u8,
        address: u8,
        value: u8,
    ) -> MpeChange {
        select(decoder, address);
        zones
            .parameter(channel, decoder.cc(6, value).unwrap())
            .unwrap()
    }

    #[test]
    fn mpe_dual_zones_trim_overlap_and_report_role_reassignment_immediately() {
        for upper_first in [false, true] {
            for first in 0..=15 {
                for last in 0..=15 {
                    let mut zones = MpeZones::default();
                    let mut decoder = ParameterDecoder::default();
                    let managers = if upper_first { [15, 0] } else { [0, 15] };
                    received(&mut zones, &mut decoder, managers[0], 6, first);
                    received(&mut zones, &mut decoder, managers[1], 6, last);
                    let [lower, upper] = zones.zones();
                    assert_eq!(lower.channels() & upper.channels(), 0);
                    let ordered = if upper_first {
                        [upper, lower]
                    } else {
                        [lower, upper]
                    };
                    assert_eq!(ordered[1].members, last);
                    assert_eq!(
                        ordered[0].members,
                        if last == 0 {
                            first
                        } else {
                            first.min(14u8.saturating_sub(last))
                        }
                    );
                }
            }
        }
        let mut zones = MpeZones::default();
        let mut decoder = ParameterDecoder::default();
        assert_eq!(
            received(&mut zones, &mut decoder, 0, 6, 15).channels,
            u16::MAX
        );
        let change = received(&mut zones, &mut decoder, 15, 6, 15);
        assert_eq!(
            change.channels,
            u16::MAX,
            "every channel changed zone or manager/member role"
        );
        assert_eq!(zones.member(0).unwrap().manager, 15);
        assert_eq!(zones.manager(15).unwrap().members, 15);
        assert_eq!(
            received(&mut zones, &mut decoder, 15, 6, 0).channels,
            u16::MAX
        );
        assert!(zones.zone(0).is_none() && zones.zone(15).is_none());
        assert_eq!(
            zones.zones()[1].manager_range,
            Some(BendRange { coarse: 2, fine: 0 })
        );
    }

    #[test]
    fn mpe_ranges_use_parameter_baselines_and_remain_independent_between_zones() {
        let mut zones = MpeZones::default();
        let mut decoder = ParameterDecoder::default();
        let mut another_member = ParameterDecoder::default();
        zones.configure(0, 4, 24).unwrap();
        zones.configure(15, 3, 12).unwrap();
        assert_eq!(zones.manager(0).unwrap().manager_range, None);
        received(&mut zones, &mut decoder, 2, 0, 7);
        zones.parameter(2, decoder.cc(38, 25).unwrap()).unwrap();
        assert_eq!(zones.member(4).unwrap().member_range.semitones(), 7.25);
        assert_eq!(zones.member(12).unwrap().member_range.semitones(), 12.);
        received(&mut zones, &mut decoder, 0, 0, 6);
        zones.parameter(0, decoder.cc(38, 50).unwrap()).unwrap();
        assert_eq!(
            zones.manager(0).unwrap().manager_range.unwrap().semitones(),
            6.5
        );
        assert_eq!(zones.manager(15).unwrap().manager_range, None);
        // Each complete transaction keeps its physical channel's coarse byte,
        // even though the resulting sensitivity is shared by all zone members.
        received(&mut zones, &mut another_member, 3, 0, 12);
        assert_eq!(zones.member(4).unwrap().member_range.semitones(), 12.);
        select(&mut decoder, 0);
        zones.parameter(2, decoder.cc(6, 7).unwrap()).unwrap();
        received(&mut zones, &mut another_member, 3, 0, 12);
        zones.parameter(2, decoder.cc(38, 25).unwrap()).unwrap();
        assert_eq!(zones.member(4).unwrap().member_range.semitones(), 7.25);
        select(&mut decoder, 1);
        select(&mut decoder, 0);
        let fine_only = decoder.cc(38, 75).unwrap();
        assert_eq!(fine_only.absolute(), None);
        zones.parameter(0, fine_only).unwrap();
        assert_eq!(
            zones.manager(0).unwrap().manager_range.unwrap().semitones(),
            6.75
        );
        zones.parameter(0, decoder.cc(6, 5).unwrap()).unwrap();
        assert_eq!(
            zones.manager(0).unwrap().manager_range.unwrap().semitones(),
            5.
        );
        let before = zones;
        assert_eq!(decoder.cc(121, 0), None);
        assert_eq!(decoder.cc(38, 99), None);
        assert_eq!(
            zones, before,
            "controller reset does not reset negotiated settings"
        );
        received(&mut zones, &mut decoder, 0, 6, 4);
        assert_eq!(zones.member(4).unwrap().member_range.semitones(), 48.);
        assert_eq!(
            zones.manager(0).unwrap().manager_range.unwrap().semitones(),
            2.
        );
        assert_eq!(zones.member(12).unwrap().member_range.semitones(), 12.);
    }

    #[test]
    fn mpe_ignores_invalid_managers_counts_nrpn_relative_and_mcm_fine() {
        let mut zones = MpeZones::default();
        let mut decoder = ParameterDecoder::default();
        select(&mut decoder, 6);
        let entry = decoder.cc(6, 4).unwrap();
        for channel in 1..15 {
            assert_eq!(zones.parameter(channel, entry), None);
        }
        assert_eq!(zones, MpeZones::default());
        for value in [16, 127] {
            assert_eq!(zones.parameter(0, decoder.cc(6, value).unwrap()), None);
        }
        assert_eq!(zones.parameter(0, decoder.cc(38, 3).unwrap()), None);
        for cc in [96, 97] {
            assert_eq!(zones.parameter(0, decoder.cc(cc, 0).unwrap()), None);
        }
        decoder.cc(99, 0);
        decoder.cc(98, 6);
        assert_eq!(zones.parameter(0, decoder.cc(6, 4).unwrap()), None);
        assert_eq!(zones, MpeZones::default());
        assert_eq!(zones.configure(0, 16, 48), None);
        assert_eq!(zones.configure(4, 2, 48), None);
        assert_eq!(zones.zone(16), None);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ChannelTuning, ParameterData, ParameterDecoder, ParameterEvent, ParameterValue, reset_controller_value,
    };

    #[test]
    fn native_relative_tuning_keeps_signed_intent_fraction_and_receiver_capacity() {
        use super::{NativeParameter, NativeParameterData};
        let relative = |address, delta| NativeParameter { registered: true, address, data: NativeParameterData::Relative(delta) };
        let absolute = NativeParameter { registered: true, address: 1, data: NativeParameterData::Absolute(0xa0012345) };
        assert_eq!((absolute.script_value(), absolute.wire_value(), absolute.delta()), ((0xa0012345u32 >> 18) as i32, 0xa0012345, None));
        for delta in [i32::MIN, -33554431, -1, 0, 1, i32::MAX] {
            let event = relative(1, delta);
            assert_eq!((event.script_value(), event.wire_value(), event.delta()), (delta, delta as u32, Some(delta)));
        }
        let mut tuning = ChannelTuning::default();
        tuning.native(1, 0xa0012345);
        let original = tuning.fine;
        tuning.native_relative(1, 1);
        assert_eq!(tuning.fine, original + 1);
        assert_ne!(tuning.ratio(), 2f64.powf(((f64::from(original) - 2147483648.) / 2147483648.) / 12.));
        tuning.native_relative(1, -1);
        assert_eq!(tuning.fine, original);
        tuning.native(1, 0);
        tuning.native_relative(1, i32::MIN);
        assert_eq!(tuning.fine, 0);
        tuning.native(1, u32::MAX);
        tuning.native_relative(1, i32::MAX);
        assert_eq!(tuning.fine, u32::MAX);
        tuning.parameter(1, ParameterData::Entry { msb: Some(127), lsb: 0, fine: false });
        tuning.parameter(1, ParameterData::Entry { msb: Some(127), lsb: 127, fine: true });
        tuning.native_relative(1, 1);
        assert_eq!(tuning.fine, (16383 << 18) + 1, "native relative message establishes its own higher resolution");

        tuning.native(2, 64 << 25);
        for delta in [-33554431, -1, 0, 1, 33554431] { tuning.native_relative(2, delta); }
        assert_eq!(tuning.coarse, 64, "both signed substep directions discard without aggregation");
        tuning.native_relative(2, 33554432 + 1);
        assert_eq!(tuning.coarse, 65);
        tuning.native_relative(2, -33554432 - 1);
        assert_eq!(tuning.coarse, 64);
        tuning.native(2, 127 << 25);tuning.native_relative(2, i32::MAX);
        assert_eq!(tuning.coarse, 127);
        tuning.native(2, 0);tuning.native_relative(2, i32::MIN);
        assert_eq!(tuning.coarse, 0);
        let retained = tuning;
        assert!(!tuning.native_relative(0, 33554432));
        assert!(!tuning.native_relative(6, 33554432));
        assert_eq!(tuning, retained);
    }

    #[test]
    fn channel_tuning_preserves_fixed_point_resolution_and_relative_endpoints() {
        let mut tuning = ChannelTuning::default();
        let entry = |msb, lsb, fine| ParameterData::Entry { msb, lsb, fine };
        assert_eq!(tuning.ratio(), 1.);
        assert!(tuning.parameter(1, entry(Some(127), 0, false)));
        assert!(tuning.parameter(1, entry(Some(127), 127, true)));
        assert_eq!(tuning.fine, 16383 << 18);
        assert_eq!(tuning.ratio(), 2f64.powf((8191. / 8192.) / 12.));
        assert!(tuning.parameter(1, ParameterData::Increment));
        assert_eq!(tuning.fine, 16383 << 18, "MIDI1 maximum cannot become +1 semitone");
        assert!(tuning.parameter(1, entry(Some(0), 0, false)));
        assert!(tuning.parameter(1, ParameterData::Decrement));
        assert_eq!(tuning.fine, 0);

        assert!(tuning.native(1, 0xa0012345));
        let original = tuning.fine;
        assert!(tuning.parameter(1, ParameterData::Increment));
        assert_eq!(tuning.fine, original + (1 << 18), "relative MIDI1 retains the native fraction");
        assert!(tuning.parameter(1, ParameterData::Decrement));
        assert_eq!(tuning.fine, original);
        assert_eq!(tuning.ratio(), 2f64.powf(((f64::from(original) - 2147483648.) / 2147483648.) / 12.));
        assert!(tuning.native(1, u32::MAX));
        assert!(tuning.parameter(1, ParameterData::Increment));
        assert_eq!(tuning.fine, u32::MAX);
        assert!(tuning.parameter(1, entry(None, 127, true)));
        assert_eq!(tuning.fine, 16383 << 18, "a MIDI1 absolute edit returns to its own resolution");
        assert!(tuning.parameter(1, ParameterData::Increment));
        assert_eq!(tuning.fine, 16383 << 18);

        assert!(tuning.native(2, (127 << 25) | 0x1ffffff));
        assert_eq!(tuning.coarse, 127);
        assert!(tuning.parameter(2, ParameterData::Increment));
        assert_eq!(tuning.coarse, 127);
        let retained = tuning;
        assert!(tuning.parameter(2, entry(Some(0), 0, true)));
        assert_eq!(tuning, retained, "coarse tuning ignores fine Data Entry, including a stale decoder baseline");
        assert!(tuning.parameter(2, entry(Some(0), 0, false)));
        assert!(tuning.parameter(2, ParameterData::Decrement));
        assert_eq!(tuning.coarse, 0);
        let retained = tuning;
        assert!(!tuning.native(3, 0));
        assert!(!tuning.parameter(1, entry(Some(128), 0, false)));
        assert_eq!(tuning, retained);
    }

    fn select(d: &mut ParameterDecoder, registered: bool, address: u16) {
        let msb = if registered { 101 } else { 99 };
        assert_eq!(d.cc(msb, (address >> 7) as u8), None);
        assert_eq!(d.cc(msb - 1, (address & 127) as u8), None);
    }

    #[test]
    fn parameters_remain_independent_on_interleaved_physical_channels() {
        let mut channels = [ParameterDecoder::default(); 16];
        assert_eq!(channels[0].cc(101, 0), None);
        assert_eq!(channels[7].cc(99, 2), None);
        assert_eq!(channels[0].cc(100, 1), None);
        assert_eq!(channels[7].cc(98, 3), None);
        let a = channels[0].cc(6, 64).unwrap();
        let b = channels[7].cc(6, 5).unwrap();
        assert_eq!(
            a.absolute(),
            Some(ParameterValue {
                registered: true,
                address: 1,
                value: 8192
            })
        );
        assert_eq!((a.registered, a.address, a.value()), (true, 1, Some(8192)));
        assert_eq!(
            (b.registered, b.address, b.value()),
            (false, 259, Some(640))
        );
        assert_eq!(channels[0].cc(38, 127).unwrap().value(), Some(8319));
        assert_eq!(channels[7].cc(38, 1).unwrap().value(), Some(641));
    }

    #[test]
    fn data_entry_msb_clears_fine_and_unknown_baselines_remain_explicit() {
        let mut d = ParameterDecoder::default();
        select(&mut d, true, 1);
        let fine = d.cc(38, 99).unwrap();
        assert_eq!(
            fine.data,
            ParameterData::Entry {
                msb: None,
                lsb: 99,
                fine: true
            }
        );
        assert_eq!(fine.value(), None);
        assert_eq!(fine.absolute(), None);
        assert_eq!(d.cc(6, 64).unwrap().value(), Some(8192));
        assert_eq!(d.cc(38, 5).unwrap().value(), Some(8197));
        assert_eq!(d.cc(6, 65).unwrap().value(), Some(8320));
        // A single selector can change an address after both bytes are known.
        assert_eq!(d.cc(100, 2), None);
        assert_eq!(d.cc(38, 7).unwrap().value(), None);
        assert_eq!(d.cc(6, 127).unwrap().value(), Some(16256));
        assert_eq!(d.cc(38, 127).unwrap().value(), Some(16383));
        assert_eq!(d.cc(6, 0).unwrap().value(), Some(0));
    }

    #[test]
    fn selectors_switch_families_and_null_disables_every_data_operation() {
        let mut d = ParameterDecoder::default();
        assert_eq!(d.cc(101, 0), None);
        assert_eq!(
            d.cc(6, 12),
            None,
            "both initial selector bytes are required"
        );
        assert_eq!(d.cc(100, 0), None);
        assert_eq!(d.cc(6, 12).unwrap().value(), Some(1536));
        assert_eq!(d.cc(99, 3), None);
        assert_eq!(d.cc(38, 1), None, "partial NRPN must not write prior RPN");
        assert_eq!(d.cc(98, 4), None);
        let n = d.cc(6, 5).unwrap();
        assert_eq!((n.registered, n.address), (false, 388));
        for registered in [true, false] {
            select(&mut d, registered, 16382);
            assert_eq!(d.cc(6, 127).unwrap().address, 16382);
            select(&mut d, registered, 16383);
            for cc in [6, 38, 96, 97] {
                assert_eq!(d.cc(cc, 127), None);
            }
        }
    }

    #[test]
    fn relative_operations_ignore_wire_magnitude_without_inventing_step_sizes() {
        let mut d = ParameterDecoder::default();
        for registered in [true, false] {
            for address in [0, 1, 2, 3, 4, 6, 16382] {
                select(&mut d, registered, address);
                for value in [0, 1, 127] {
                    for (cc, data) in [
                        (96, ParameterData::Increment),
                        (97, ParameterData::Decrement),
                    ] {
                        let event = d.cc(cc, value).unwrap();
                        assert_eq!(
                            event,
                            ParameterEvent {
                                registered,
                                address,
                                data
                            }
                        );
                        assert_eq!(event.value(), None);
                        assert_eq!(event.absolute(), None);
                    }
                }
            }
        }
    }

    #[test]
    fn reset_and_invalid_input_cannot_retarget_or_leak_transaction_data() {
        let mut d = ParameterDecoder::default();
        select(&mut d, true, 1);
        d.cc(6, 64);
        for (cc, value) in [(101, 128), (98, 255), (6, 128), (121, 255), (255, 0)] {
            assert_eq!(d.cc(cc, value), None);
        }
        assert_eq!(d.cc(38, 3).unwrap().value(), Some(8195));
        assert_eq!(d.cc(121, 0), None);
        for cc in [6, 38, 96, 97] {
            assert_eq!(d.cc(cc, 1), None);
        }
        d.cc(100, 1);
        assert_eq!(d.cc(6, 2), None);
        d.cc(101, 0);
        assert_eq!(d.cc(38, 4).unwrap().value(), None);
        assert_eq!(d.cc(6, 2).unwrap().value(), Some(256));
        d.reset();
        assert_eq!(d.cc(96, 0), None);
    }

    #[test]
    fn reset_policy_covers_every_controller_and_preserves_device_defaults() {
        let retained = [
            0, 7, 10, 32, 39, 42, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 91, 92, 93, 94, 95, 120, 121,
            122, 123, 124, 125, 126, 127,
        ];
        let standard = [
            (1, 0),
            (11, 127),
            (64, 0),
            (65, 0),
            (66, 0),
            (67, 0),
            (98, 127),
            (99, 127),
            (100, 127),
            (101, 127),
        ];
        for initial in [None, Some(37), Some(127)] {
            for cc in 0..128 {
                let expected = if retained.contains(&cc) {
                    None
                } else {
                    Some(
                        standard
                            .iter()
                            .find(|&&(c, _)| c == cc)
                            .map_or(initial.unwrap_or(0), |&(_, value)| value),
                    )
                };
                assert_eq!(
                    reset_controller_value(cc, initial),
                    expected,
                    "CC{cc}, authored default {initial:?}"
                );
            }
            for cc in [128, 129] {
                assert_eq!(reset_controller_value(cc, initial), Some(0));
            }
            for cc in 130..=255 {
                assert_eq!(reset_controller_value(cc, initial), None);
            }
        }
    }
}

#[cfg(test)]
mod continuous_value_tests {
    use super::{Bend, ControlValue, Velocity};

    #[test]
    fn pan_preserves_resolution_centers_without_changing_continuous_cc_values() {
        assert_eq!(std::mem::size_of::<ControlValue>(),16);
        for raw in 0..=127 {
            let value=ControlValue::from_midi1(raw);
            let old=((value.dsp()*127.-64.)/63.).clamp(-1.,1.);
            assert_eq!(value.pan().to_bits(),old.to_bits(),"MIDI1 CC10 {raw}");
        }
        for value in [ControlValue::from_midi1(64),ControlValue::from_midi14(8192),
            ControlValue::from_midi2(0x80000000),ControlValue::new(0.5).unwrap()] {
            assert_eq!(value.pan(),0.);
        }
        for raw in [0,1,6145,8191,8192,8193,9217,16383] {
            let value=ControlValue::from_midi14(raw);
            assert_eq!(value.normalized(),f64::from(raw)/16383.);
            let expected=(f64::from(raw)-8192.)/if raw<8192 {8192.} else {8191.};
            assert_eq!(value.pan(),expected as f32);
        }
        for raw in [0,1,0x70000001,0x7fffffff,0x80000000,0x88000001,0x89000001,u32::MAX] {
            let value=ControlValue::from_midi2(raw);
            assert_eq!(value.normalized(),f64::from(raw)/4294967295.);
            let expected=(f64::from(raw)-2147483648.)/if raw<0x80000000 {2147483648.} else {2147483647.};
            assert_eq!(value.pan(),expected as f32);
        }
        let (a,b)=(ControlValue::from_midi2(0x88000001),ControlValue::from_midi2(0x89000001));
        assert_eq!(a.midi1(),b.midi1());
        assert_ne!(a.pan(),b.pan());
        assert_eq!(ControlValue::new(0.5001).unwrap().pan(),0.0002);
    }

    #[test]
    fn wire_projections_discard_low_bits_without_rounding_dsp_values() {
        for raw in [0, 1, (1 << 25) - 1, 1 << 25, 127 << 25, u32::MAX] {
            let value = ControlValue::from_midi2(raw);
            assert_eq!(value.midi1(), (raw >> 25) as u8);
            assert_eq!(value.normalized(), f64::from(raw) / f64::from(u32::MAX));
        }
        for raw in [0, 1, (1 << 18) - 1, 1 << 18, 0x7fffffff, 0x80000000, u32::MAX] {
            let value = Bend::from_midi2(raw);
            assert_eq!(value.midi1(), (raw >> 18) as u16);
            assert_eq!(value.normalized(), (f64::from(raw) - 2147483648.) / 2147483648.);
        }
        for raw in [0, 1, 511, 512, 127 << 9, u16::MAX] {
            let value = Velocity::from_midi2(raw);
            assert_eq!(value.midi1(), (raw >> 9) as u8);
            assert_eq!(value.normalized(), f64::from(raw) / f64::from(u16::MAX));
        }
        let paired = ControlValue::from_midi14(255);
        let host = ControlValue::new(paired.normalized()).unwrap();
        assert_eq!((paired.midi1(), host.midi1()), (1, 2));
        assert_eq!(paired.dsp(), host.dsp());
        assert_ne!(paired, host, "source projection is observable callback metadata");
        let wire = Bend::from_midi2(0x7fffffff);
        let host = Bend::new(wire.normalized()).unwrap();
        assert_eq!((wire.midi1(), host.midi1()), (8191, 8192));
        assert_eq!(wire.normalized(), host.normalized());
        assert_ne!(wire, host);
        assert_eq!(Velocity::from_midi2(0), Velocity::ZERO);
    }

    #[test]
    fn canonical_controls_validate_and_keep_precision_and_wire_centers() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.01, 1.01] {
            assert_eq!(ControlValue::new(value), None);
        }
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.01, 1.01] {
            assert_eq!(Bend::new(value), None);
        }
        for byte in 0..=127 { assert_eq!(ControlValue::from_midi1(byte).midi1(), byte); }
        for value in 0..=16383 { assert_eq!(Bend::from_midi1(value).midi1(), value); }
        assert_eq!(ControlValue::from_midi2(0), ControlValue::ZERO);
        assert_eq!(ControlValue::from_midi2(u32::MAX), ControlValue::FULL);
        assert_eq!(Bend::from_midi2(0x80000000), Bend::ZERO);
        assert_eq!(Bend::from_midi2(0).normalized(), -1.);
        assert_eq!(Bend::from_midi2(u32::MAX).normalized(), 1. - 1. / 2147483648.);
        let (a, b) = (ControlValue::new(0.5001).unwrap(), ControlValue::new(0.5030).unwrap());
        assert_eq!(a.midi1(), b.midi1());
        assert_ne!(a.normalized(), b.normalized());
        assert_ne!(a.dsp(), b.dsp());
    }
}

#[cfg(test)]
mod midi1_input_tests {
    use super::*;

    #[test]
    fn paired_controllers_keep_raw_callbacks_and_clear_fine_on_new_coarse() {
        let mut decoder = Midi1Decoder::default();
        assert_eq!(decoder.cc(33, 127).unwrap().control, Control::Cc(33, ControlValue::FULL));
        assert_eq!(decoder.cc(1, 64).unwrap().control, Control::Cc(1, ControlValue::from_midi1(64)));
        let paired = decoder.cc(33, 127).unwrap().control;
        assert_eq!(paired.legacy(), (33, 127));
        assert_eq!(paired, Control::Cc14 { cc: 33, raw: 127, coarse: 1,
            value: ControlValue::from_midi14(8319) });
        assert_eq!(decoder.cc(1, 0).unwrap().control, Control::Cc14 {
            cc: 1, raw: 0, coarse: 1, value: ControlValue::ZERO });
        assert_eq!(decoder.cc(33, 127).unwrap().control, Control::Cc14 {
            cc: 33, raw: 127, coarse: 1, value: ControlValue::from_midi14(127) });
        for (coarse, fine) in [(0, 32), (6, 38)] {
            assert!(matches!(decoder.cc(coarse, 64).unwrap().control, Control::Cc(..)));
            assert!(matches!(decoder.cc(fine, 127).unwrap().control, Control::Cc(..)));
        }
        assert_eq!(decoder.cc(128, 0), None);
        assert_eq!(decoder.cc(1, 128), None);
        decoder.cc(7, 100);decoder.cc(39, 127);
        decoder.reset_controllers(&[None; 128]);
        assert!(matches!(decoder.cc(39, 1).unwrap().control, Control::Cc14 { coarse: 7, .. }));
        assert!(matches!(decoder.cc(33, 1).unwrap().control, Control::Cc14 { coarse: 1, value, .. } if value.normalized()==1./16383.));
        decoder.reset();
        assert!(matches!(decoder.cc(39, 1).unwrap().control, Control::Cc(..)));
    }

    #[test]
    fn velocity_prefix_is_one_shot_and_zero_on_alias_has_no_effect() {
        let mut channels = [Midi1Decoder::default(); 16];
        channels[4].cc(88, 127);
        channels[4].cc(74, 35);
        assert_eq!(channels[4].velocity(128), None);
        assert_eq!(channels[4].release_velocity(255), None);
        assert_eq!(channels[7].velocity(100), None);
        assert_eq!(channels[4].velocity(127), Some(Velocity::FULL));
        assert_eq!(channels[4].velocity(127), None);
        channels[4].cc(88, 127);
        assert_eq!(channels[4].velocity(0), None);
        assert_eq!(channels[4].velocity(1), None);
        channels[4].cc(88, 1);
        assert_eq!(channels[4].release_velocity(0), Some(Velocity::from_midi14(1)));
        assert_eq!(channels[4].release_velocity(0), None);
        channels[4].cc(88, 127);
        channels[4].reset_controllers(&[None; 128]);
        assert_eq!(channels[4].velocity(1), None);
        for coarse in 0..=127u16 {
            for fine in [0, 127] {
                let value = Velocity::from_midi14((coarse << 7) | fine);
                assert_eq!(value.midi1(), coarse as u8, "prefix must not retarget the authored velocity zone");
            }
        }
        assert_eq!(Velocity::from_midi14(0), Velocity::ZERO);
        assert_eq!(Velocity::from_midi14(65535), Velocity::FULL);
    }
}
