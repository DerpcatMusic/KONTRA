//! Synthetic instruments that take the paths the audited libraries take, built at
//! run time so CI needs no vendor files and no audio is committed.
//!
//! Each fixture is authored as sampler IR (groups, zones, sample table) plus KSP
//! and written by the library's NKI writer — the same container the real
//! libraries use. What the writer still cannot carry is listed in
//! [`Fixture::gaps`].

use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{Mutex, PoisonError};

use anyhow::{Context, Result};

mod audio;
mod ksp;
mod program;

/// Folder version: bump it when the generated instruments change.
const VERSION: u32 = 3;
/// The natural notes every fixture covers, C2 to A3 (the keys the probe plays).
const ROOTS: [(u8, &str); 13] = [
    (48, "C2"),
    (50, "D2"),
    (52, "E2"),
    (53, "F2"),
    (55, "G2"),
    (57, "A2"),
    (59, "B2"),
    (60, "C3"),
    (62, "D3"),
    (64, "E3"),
    (65, "F3"),
    (67, "G3"),
    (69, "A3"),
];

/// A synthetic instrument, named as on the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fixture {
    /// Areia pattern: three mic groups with round robins, a played note replaced
    /// by three `play_note` children whose start comes from CC113.
    CcStart,
    /// Areia's effect traffic: 300 UI writes per note before the note may sound.
    UiLoad,
    /// Vista pattern: two velocity layers, the start offset derived from the key.
    KeyPosition,
    /// Chord pattern: one mic with round robins, so every chord key runs a script.
    Dense,
    /// Samples much longer than the head, so voices read from disk after the preload.
    Streamed,
}

impl Fixture {
    /// Every fixture, in run order.
    pub const ALL: [Self; 5] = [
        Self::CcStart,
        Self::UiLoad,
        Self::KeyPosition,
        Self::Dense,
        Self::Streamed,
    ];

    /// The name on the command line and in the folder name.
    pub fn name(self) -> &'static str {
        match self {
            Self::CcStart => "cc_start",
            Self::UiLoad => "ui_load",
            Self::KeyPosition => "key_position",
            Self::Dense => "dense",
            Self::Streamed => "streamed",
        }
    }

    /// Parts of the audited pattern the instrument does not carry. Empty when the
    /// fixture carries everything it stands for.
    pub fn gaps(self) -> &'static str {
        match self {
            Self::CcStart => {
                "no CC113 to S.Mod/playPos route and no preload head over the modulated \
                 start range: the offset goes through play_note, so the audited disk-start \
                 path is not exercised"
            }
            Self::UiLoad => {
                "300 UI writes on one knob stand in for Areia's 16,700-line on note; \
                 one message per note instead of its 67"
            }
            Self::KeyPosition => {
                "no KeyPosition/RTC modulation route: the script computes the offset from \
                 $EVENT_NOTE and passes it to play_note"
            }
            Self::Dense => {
                "one play_note per played key, not one per mic layer; no UI traffic \
                 (ui_load carries that)"
            }
            Self::Streamed => {
                "samples are short enough to stay in the page cache; no degraded-disk run (C2)"
            }
        }
    }

    /// Voices one played key is expected to start, as the authored KSP intends.
    pub fn expected_voices(self) -> usize {
        match self {
            Self::CcStart => 3,
            Self::UiLoad | Self::KeyPosition | Self::Dense | Self::Streamed => 1,
        }
    }

    /// Writes the instrument if it is not there yet and returns its `.nki` path.
    ///
    /// # Errors
    /// Fails when the samples cannot be written or the NKI writer rejects them.
    pub fn build(self) -> Result<PathBuf> {
        // Tests share one process: a second caller waits and then finds the folder ready.
        static BUILD: Mutex<()> = Mutex::new(());
        let _guard = BUILD.lock().unwrap_or_else(PoisonError::into_inner);

        let spec = self.spec();
        let dir = output_root()?.join(format!("{}-v{VERSION}", self.name()));
        if !dir.join("ready").exists() {
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
            program::write(&spec, &dir)?;
            std::fs::write(dir.join("ready"), "")?;
        }
        Ok(program::preset(&spec, &dir))
    }

    fn spec(self) -> Spec {
        match self {
            Self::CcStart => Spec {
                name: "ProbeCcStart",
                mics: &[Mic::Close, Mic::Room, Mic::Far],
                rounds: 4,
                layers: 1,
                seconds: 0.25,
                groups: groups(self.name(), &[Mic::Close, Mic::Room, Mic::Far], 4),
                script: Some(ksp::CC_START),
            },
            Self::UiLoad => Spec {
                name: "ProbeUiLoad",
                mics: &[Mic::Close],
                rounds: 1,
                layers: 1,
                seconds: 0.25,
                groups: groups(self.name(), &[Mic::Close], 1),
                script: Some(ksp::UI_LOAD),
            },
            Self::KeyPosition => Spec {
                name: "ProbeKeyPosition",
                mics: &[Mic::Close],
                rounds: 1,
                layers: 2,
                seconds: 0.25,
                groups: groups(self.name(), &[Mic::Close], 1),
                script: Some(ksp::KEY_POSITION),
            },
            Self::Dense => Spec {
                name: "ProbeDense",
                mics: &[Mic::Close],
                rounds: 4,
                layers: 1,
                seconds: 0.25,
                groups: groups(self.name(), &[Mic::Close], 4),
                script: Some(ksp::DENSE),
            },
            Self::Streamed => Spec {
                name: "ProbeStreamed",
                mics: &[Mic::Close],
                rounds: 1,
                layers: 1,
                seconds: 2.0,
                groups: groups(self.name(), &[Mic::Close], 1),
                script: None,
            },
        }
    }
}

impl FromStr for Fixture {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|f| f.name() == s)
            .ok_or_else(|| {
                let names: Vec<&str> = Self::ALL.iter().map(|f| f.name()).collect();
                format!("unknown fixture `{s}` ({})", names.join(", "))
            })
    }
}

impl fmt::Display for Fixture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// What one fixture writes.
struct Spec {
    /// Instrument name; every sample file name starts with it.
    name: &'static str,
    mics: &'static [Mic],
    /// Round robins per mic; 1 for none.
    rounds: u32,
    /// Velocity layers per key.
    layers: usize,
    seconds: f32,
    /// One group per round robin and mic, in the order the KSP addresses them.
    groups: Vec<GroupSpec>,
    /// KSP for the instrument's first script slot.
    script: Option<&'static str>,
}

/// One authored group and the sample set its zones take.
struct GroupSpec {
    name: String,
    /// Index into [`Spec::mics`].
    mic: usize,
    /// Round robin, zero-based.
    round: u32,
}

/// One group per `(round, mic)`, round-major so the KSP can index
/// `round * mics.len() + mic`.
fn groups(instrument: &str, mics: &[Mic], rounds: u32) -> Vec<GroupSpec> {
    let mut groups = Vec::with_capacity(mics.len() * rounds as usize);
    for round in 0..rounds {
        for (mic, m) in mics.iter().enumerate() {
            let mut label = Vec::new();
            if rounds > 1 {
                label.push(format!("RR{}", round + 1));
            }
            if mics.len() > 1 {
                label.push(m.tag().to_owned());
            }
            groups.push(GroupSpec {
                name: if label.is_empty() {
                    instrument.to_owned()
                } else {
                    label.join(" ")
                },
                mic,
                round,
            });
        }
    }
    groups
}

/// A microphone group: how loud its tone is and how much noise it carries.
#[derive(Debug, Clone, Copy)]
enum Mic {
    Close,
    Room,
    Far,
}

impl Mic {
    fn tag(self) -> &'static str {
        match self {
            Self::Close => "close",
            Self::Room => "room",
            Self::Far => "far",
        }
    }

    /// Tone level and noise level, linear.
    fn mix(self) -> (f32, f32) {
        match self {
            Self::Close => (0.5, 0.0),
            Self::Room => (0.3, 0.02),
            Self::Far => (0.15, 0.05),
        }
    }
}

/// `target/<profile>/kontra-probe-fixtures`, next to the probe binary.
fn output_root() -> Result<PathBuf> {
    let exe = std::env::current_exe().context("locating the probe binary")?;
    let dir = exe.parent().context("the probe binary has no folder")?;
    Ok(dir.join("kontra-probe-fixtures"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_fixture_names_itself_and_has_a_script_except_streamed() {
        for fixture in Fixture::ALL {
            assert_eq!(fixture.name().parse::<Fixture>(), Ok(fixture));
            assert_eq!(
                fixture.spec().script.is_some(),
                fixture != Fixture::Streamed
            );
        }
    }

    #[test]
    fn groups_are_round_major_so_the_script_can_index_round_times_mics() {
        let spec = Fixture::CcStart.spec();
        assert_eq!(spec.groups.len(), 12);
        for (index, group) in spec.groups.iter().enumerate() {
            assert_eq!(group.round as usize, index / 3);
            assert_eq!(group.mic, index % 3);
        }
        assert_eq!(Fixture::Dense.spec().groups.len(), 4);
        assert_eq!(Fixture::Streamed.spec().groups.len(), 1);
    }

    #[test]
    fn every_root_zone_has_a_key_band_and_the_ends_cover_the_keyboard() {
        assert_eq!(program::low_key(0), 0);
        assert_eq!(program::high_key(ROOTS.len() - 1), 127);
        for n in 1..ROOTS.len() {
            assert_eq!(program::low_key(n), ROOTS[n - 1].0 + 1);
            assert_eq!(program::high_key(n - 1), ROOTS[n].0 - 1);
        }
    }

    #[test]
    fn layers_split_the_velocity_range_without_gaps_or_overlap() {
        assert_eq!(
            program::velocities(0, 1),
            sampler_ir::VelocityRange { low: 0, high: 127 }
        );
        assert_eq!(
            program::velocities(0, 2),
            sampler_ir::VelocityRange { low: 0, high: 63 }
        );
        assert_eq!(
            program::velocities(1, 2),
            sampler_ir::VelocityRange { low: 64, high: 127 }
        );
        assert_eq!(
            program::velocities(3, 4),
            sampler_ir::VelocityRange { low: 96, high: 127 }
        );
    }
}
