//! Port from v1 0cb7a8a0:src/engine/overrides.rs. Sparse user layer; no source IR mutation.
use sampler_core::{EngineParameterAddress, EngineParameterOffset};
/// Overrides one engine holds; further ones are ignored.
pub const MAX_OVERRIDES: usize = 256;

/// A parameter the player can edit. Filter and EQ knobs name their group
/// insert slot; EQ bands count from 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Param {
    Attack,
    /// Attack shape, -1..=1.
    Curve,
    Hold,
    Decay,
    Sustain,
    Release,
    Cutoff(u8),
    Resonance(u8),
    Freq(u8, u8),
    Bandwidth(u8, u8),
    Gain(u8, u8),
}

impl Param {
    pub const ENVELOPE: [Self; 6] = [
        Self::Attack,
        Self::Curve,
        Self::Hold,
        Self::Decay,
        Self::Sustain,
        Self::Release,
    ];

    // The view uses the shared service's normalized values; native conversion
    // remains in Model and in sampler-core's admitted binding law.
    pub fn norm(self, value: f32) -> f32 {
        value
    }
    pub fn value(self, norm: f32) -> f32 {
        norm.clamp(0., 1.)
    }
}

/// One edit: `offset` on `param`'s normalized value, in one group or in all
/// of them (`None`). Offsets for all groups and for one group add up.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Override {
    pub group: Option<u16>,
    pub param: Param,
    pub offset: f32,
}

/// A part's overrides as saved with it.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Edits(pub Vec<Override>);

impl Edits {
    /// The offset stored for exactly `group` and `param`.
    pub fn get(&self, group: Option<u16>, param: Param) -> f32 {
        self.0
            .iter()
            .find(|o| o.group == group && o.param == param)
            .map_or(0.0, |o| o.offset)
    }

    /// What applies to `group`: its own offset plus the all-groups one.
    pub fn offset(&self, group: u16, param: Param) -> f32 {
        let one = |g| self.get(g, param);
        one(None) + one(Some(group))
    }

    /// Store `o`, replacing its group and parameter's; offset 0 removes it.
    /// False when it changed nothing or there is no room.
    pub fn set(&mut self, o: Override) -> bool {
        if !o.offset.is_finite() {
            return false;
        }
        let at = self
            .0
            .iter()
            .position(|e| e.group == o.group && e.param == o.param);
        match at {
            Some(i) if o.offset == 0.0 => {
                self.0.swap_remove(i);
            }
            Some(i) if self.0[i].offset != o.offset => self.0[i].offset = o.offset,
            None if o.offset != 0.0 && self.0.len() < MAX_OVERRIDES => self.0.push(o),
            _ => return false,
        }
        true
    }

    /// Remove every override of `param` for all groups and for each one.
    pub fn reset(&mut self, param: Param) {
        self.0.retain(|o| o.param != param);
    }
}

impl Param {
    pub fn address(self, group: i32) -> EngineParameterAddress {
        let (name, slot, generic) = match self {
            Self::Attack => ("ATTACK", -2, -1),
            Self::Curve => ("ATK_CURVE", -2, -1),
            Self::Hold => ("HOLD", -2, -1),
            Self::Decay => ("DECAY", -2, -1),
            Self::Sustain => ("SUSTAIN", -2, -1),
            Self::Release => ("RELEASE", -2, -1),
            Self::Cutoff(s) => ("CUTOFF", i32::from(s), -1),
            Self::Resonance(s) => ("RESONANCE", i32::from(s), -1),
            Self::Freq(s, b) => (
                ["FREQ1", "FREQ2", "FREQ3"][usize::from(b.min(2))],
                i32::from(s),
                -1,
            ),
            Self::Bandwidth(s, b) => (
                ["BW1", "BW2", "BW3"][usize::from(b.min(2))],
                i32::from(s),
                -1,
            ),
            Self::Gain(s, b) => (
                ["GAIN1", "GAIN2", "GAIN3"][usize::from(b.min(2))],
                i32::from(s),
                -1,
            ),
        };
        EngineParameterAddress {
            parameter: sampler_core::engine_parameter_id(&format!("ENGINE_PAR_{name}")).unwrap(),
            group,
            slot,
            generic,
        }
    }
    pub fn of(b: sampler_core::EngineParameterBinding) -> Option<Self> {
        let a = b.address;
        if b.control.0 >> 104 == 0x454e56 {
            return Self::ENVELOPE
                .into_iter()
                .find(|p| p.address(a.group).parameter == a.parameter);
        }
        let s = u8::try_from(a.slot).ok()?;
        let name = sampler_core::engine_parameter_name(a.parameter)?;
        Some(match name.trim_start_matches('$') {
            "ENGINE_PAR_CUTOFF" => Self::Cutoff(s),
            "ENGINE_PAR_RESONANCE" => Self::Resonance(s),
            "ENGINE_PAR_FREQ1" => Self::Freq(s, 0),
            "ENGINE_PAR_FREQ2" => Self::Freq(s, 1),
            "ENGINE_PAR_FREQ3" => Self::Freq(s, 2),
            "ENGINE_PAR_BW1" => Self::Bandwidth(s, 0),
            "ENGINE_PAR_BW2" => Self::Bandwidth(s, 1),
            "ENGINE_PAR_BW3" => Self::Bandwidth(s, 2),
            "ENGINE_PAR_GAIN1" => Self::Gain(s, 0),
            "ENGINE_PAR_GAIN2" => Self::Gain(s, 1),
            "ENGINE_PAR_GAIN3" => Self::Gain(s, 2),
            _ => return None,
        })
    }
}
impl Edits {
    pub fn native(&self) -> std::sync::Arc<[EngineParameterOffset]> {
        self.0
            .iter()
            .take(MAX_OVERRIDES)
            .filter(|o| o.offset.is_finite())
            .map(|o| EngineParameterOffset {
                address: o.param.address(o.group.map_or(-1, i32::from)),
                offset: o.offset,
            })
            .collect()
    }
}
impl moose::core::custom_state::StateField for Edits {
    fn write_field(&self, buf: &mut Vec<u8>) {
        serde_json::to_string(self)
            .unwrap_or_default()
            .write_field(buf);
    }
    fn read_field(c: &mut moose::core::custom_state::StateCursor) -> Option<Self> {
        let mut e: Self = serde_json::from_str(&String::read_field(c)?).unwrap_or_default();
        e.0.retain(|o| o.offset.is_finite());
        e.0.truncate(MAX_OVERRIDES);
        Some(e)
    }
}
/// A playing voice as the editor sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tap {
    pub group: u16,
    pub note: u8,
    pub velocity: u8,
    /// [`super::Phase`] as `u8`.
    pub phase: u8,
    /// Envelope level, 0..=1.
    pub level: f32,
}

impl Tap {
    pub fn pack(self) -> u64 {
        let level = (self.level.clamp(0.0, 1.0) * 65535.0).round() as u64;
        1 << 63
            | u64::from(self.group) << 32
            | u64::from(self.note & 127) << 25
            | u64::from(self.velocity & 127) << 18
            | u64::from(self.phase & 7) << 16
            | level
    }

    pub fn unpack(x: u64) -> Option<Self> {
        (x >> 63 == 1).then(|| Self {
            group: (x >> 32) as u16,
            note: (x >> 25) as u8 & 127,
            velocity: (x >> 18) as u8 & 127,
            phase: (x >> 16) as u8 & 7,
            level: (x & 0xffff) as f32 / 65535.0,
        })
    }
}

impl Tap {
    pub fn from_native(t: sampler_core::VoiceTap) -> Self {
        Self {
            group: t.group as u16,
            note: t.key,
            velocity: t.velocity,
            phase: t.phase,
            level: t.level,
        }
    }
}
