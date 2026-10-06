//! What a load did, as plain data: shown in the load-report panel and
//! written to the log. Built from the translator's unsupported list
//! (`sampler_ir::Unsupported`) plus the core's runtime counters. Never
//! carries key material.

use serde::{Deserialize, Serialize};

/// Why something in the source does not play as authored.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MissingReason {
    /// The translator does not recognize the feature.
    Unknown,
    /// Recognized, but not implemented yet.
    NotModeled,
    /// Recognized, but this value cannot be represented.
    InvalidValue,
    /// Representable, but how the source maps it to sound is not established.
    UnknownLaw,
}

/// One feature that was not translated or not implemented.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Missing {
    /// In source terms: `"zone 3 (Piano_C4.wav)"`, `"script 1 line 40:7"`.
    pub location: String,
    /// The source feature: module, opcode, field or builtin name.
    pub feature: String,
    /// The authored value, verbatim.
    pub value: String,
    pub reason: MissingReason,
}

/// What was decoded, translated and is playing.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decoded {
    /// Source format and container version, e.g. `"Kontakt (v0x80)"`.
    pub format: String,
    pub zones: usize,
    pub groups: usize,
    pub samples: usize,
    pub buses: usize,
    pub scripts: usize,
    pub articulations: usize,
    pub controls: usize,
    /// Keys the zones map, one bit per MIDI key, for the keyboard.
    pub keys: u128,
}

impl Decoded {
    /// Whether some zone maps MIDI key `key`.
    pub fn maps(&self, key: u8) -> bool {
        key < 128 && self.keys & 1 << key != 0
    }
}

/// The keys `instrument`'s zones map, one bit per MIDI key.
pub fn key_bits(instrument: &sampler_ir::Instrument) -> u128 {
    instrument.zones.iter().fold(0, |bits, z| bits | range_bits(z.keys.low, z.keys.high))
}

/// Keys `low..=high` as bits; none when the range is empty.
pub fn range_bits(low: u8, high: u8) -> u128 {
    let (low, high) = (low.min(127), high.min(127));
    if low > high { 0 } else { (u128::MAX >> (127 - high)) & (u128::MAX << low) }
}

/// Problems while playing, cumulative since the part was installed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeProblems {
    /// Notes or voices refused for lack of preallocated capacity.
    pub capacity_drops: u64,
    /// Streamed audio not read in time and played silent.
    pub underruns: u64,
    /// Rendered frames replaced by silence for being non-finite.
    pub nonfinite: u64,
    /// Script callbacks that ran past their budget.
    pub script_overruns: u64,
    /// Input played at lower precision than sent (MIDI 2.0 values narrowed
    /// to the MIDI 1.0 zone) or not played at all (per-note controllers,
    /// program changes): counted, never silently dropped.
    pub narrowed_input: u64,
    pub ignored_input: u64,
}

/// A part's load report.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoadReport {
    pub name: String,
    pub path: String,
    pub decoded: Decoded,
    pub missing: Vec<Missing>,
    /// Refreshed by the shell from [`super::Core::problems`].
    pub runtime: RuntimeProblems,
}

impl From<&sampler_ir::Unsupported> for Missing {
    fn from(u: &sampler_ir::Unsupported) -> Self {
        Self {
            location: u.location.clone(),
            feature: u.feature.clone(),
            value: u.value.clone(),
            reason: match u.reason {
                sampler_ir::Reason::Unknown => MissingReason::Unknown,
                sampler_ir::Reason::NotModeled => MissingReason::NotModeled,
                sampler_ir::Reason::InvalidValue => MissingReason::InvalidValue,
                sampler_ir::Reason::UnknownLaw => MissingReason::UnknownLaw,
            },
        }
    }
}

impl LoadReport {
    /// The decoded counts and missing list of a translated instrument.
    pub fn of(instrument: &sampler_ir::Instrument, path: &std::path::Path, samples: usize) -> Self {
        Self {
            name: instrument.name.clone(),
            path: path.display().to_string(),
            decoded: Decoded {
                format: match &instrument.source {
                    sampler_ir::SourceFormat::Native => "Native".into(),
                    sampler_ir::SourceFormat::Kontakt { version } => format!("Kontakt (v{version:#x})"),
                    sampler_ir::SourceFormat::Sfz => "SFZ".into(),
                    sampler_ir::SourceFormat::Uvi => "UVI".into(),
                },
                zones: instrument.zones.len(),
                groups: instrument.groups.len(),
                samples,
                buses: instrument.buses.len(),
                scripts: instrument.behaviors.len(),
                articulations: instrument.articulations.len(),
                controls: instrument.controls.len(),
                keys: key_bits(instrument),
            },
            missing: instrument.unsupported.iter().map(Missing::from).collect(),
            runtime: RuntimeProblems::default(),
        }
    }

    /// One line per entry, for the log file.
    pub fn lines(&self) -> impl Iterator<Item = String> + '_ {
        let d = &self.decoded;
        std::iter::once(format!(
            "{} ({}): {} zones, {} groups, {} samples, {} buses, {} scripts, {} articulations, {} controls",
            self.name, d.format, d.zones, d.groups, d.samples, d.buses, d.scripts, d.articulations, d.controls
        ))
        .chain(self.missing.iter().map(|m| {
            format!("missing ({:?}): {} = {:?} at {}", m.reason, m.feature, m.value, m.location)
        }))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn key_bits_cover_each_zone_range() {
        let d = super::Decoded { keys: super::range_bits(60, 64), ..Default::default() };
        assert!(!d.maps(59) && d.maps(60) && d.maps(64) && !d.maps(65) && !d.maps(200));
        assert_eq!(super::range_bits(0, 255), u128::MAX);
        assert_eq!(super::range_bits(70, 60), 0);
    }
}
