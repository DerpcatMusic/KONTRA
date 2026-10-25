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
use kontakto::creator::nki::StartSource;

mod audio;
mod ksp;
mod program;

/// Folder version: bump it when the generated instruments change. The folder
/// name also carries a digest of the written bytes, so stale output cannot survive.
const VERSION: u32 = 5;
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
                "0.25 s samples sit wholly in the head, so the disk-start path under a \
                 modulated start is not exercised"
            }
            Self::UiLoad => {
                "300 UI writes on one knob stand in for Areia's 16,700-line on note; \
                 one message per note instead of its 67"
            }
            Self::KeyPosition => "",
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

    /// UI writes each played key drives, as the authored KSP intends: the
    /// `set_control_par` count in [`ksp::UI_LOAD`]. Zero when there is no UI load.
    pub fn ui_writes_per_key(self) -> usize {
        match self {
            Self::UiLoad => 300,
            Self::CcStart | Self::KeyPosition | Self::Dense | Self::Streamed => 0,
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
        let root = output_root()?;
        // The folder is named by a digest of what the fixture is written from, so an
        // edit to the spec, the writer or the authoring code gives a new folder.
        let dir = root.join(format!("{}-v{VERSION}-{:016x}", self.name(), digest(&spec)));
        if !dir.join("ready").exists() {
            // Folders of this fixture with another digest or version are stale. Within a
            // run the digest is fixed, so no test can see its folder removed underneath it.
            let stale = format!("{}-", self.name());
            if let Ok(entries) = std::fs::read_dir(&root) {
                for entry in entries {
                    let entry = entry?;
                    if entry.file_name().to_string_lossy().starts_with(&stale) {
                        std::fs::remove_dir_all(entry.path())?;
                    }
                }
            }
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
                start_mod_frames: 3_600,
                start_route: Some(StartSource::MidiCc(113)),
            },
            Self::UiLoad => Spec {
                name: "ProbeUiLoad",
                mics: &[Mic::Close],
                rounds: 1,
                layers: 1,
                seconds: 0.25,
                groups: groups(self.name(), &[Mic::Close], 1),
                script: Some(ksp::UI_LOAD),
                start_mod_frames: 0,
                start_route: None,
            },
            Self::KeyPosition => Spec {
                name: "ProbeKeyPosition",
                mics: &[Mic::Close],
                rounds: 1,
                layers: 2,
                seconds: 8.0,
                groups: groups(self.name(), &[Mic::Close], 1),
                script: None,
                start_mod_frames: 3_600,
                start_route: Some(StartSource::KeyPosition),
            },
            Self::Dense => Spec {
                name: "ProbeDense",
                mics: &[Mic::Close],
                rounds: 4,
                layers: 1,
                seconds: 0.25,
                groups: groups(self.name(), &[Mic::Close], 4),
                script: Some(ksp::DENSE),
                start_mod_frames: 0,
                start_route: None,
            },
            Self::Streamed => Spec {
                name: "ProbeStreamed",
                mics: &[Mic::Close],
                rounds: 1,
                layers: 1,
                seconds: 2.0,
                groups: groups(self.name(), &[Mic::Close], 1),
                script: None,
                start_mod_frames: 0,
                start_route: None,
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
#[derive(Debug)]
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
    /// S.Mod range of every zone, source frames. Kontakt ignores a `play_note` offset past it
    /// in DFD mode, so a script that starts at an offset sets the range it needs, as a library does.
    start_mod_frames: u32,
    /// Sample-start route on every group, as a library carries it: its source
    /// sweeps each zone through the S.Mod range. Without one, Kontakt starts a
    /// zone with an S.Mod range at a random delay.
    start_route: Option<StartSource>,
}

/// One authored group and the sample set its zones take.
#[derive(Debug)]
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

/// A digest of a fixture's spec and of the sources that write it. The writer's
/// output is not byte-stable (random UUIDs and a timestamp), so the sources stand in.
fn digest(spec: &Spec) -> u64 {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    format!("{spec:?}").hash(&mut hasher);
    for source in [
        include_str!("../../../src/creator/nki.rs"),
        include_str!("fixtures.rs"),
        include_str!("fixtures/program.rs"),
        include_str!("fixtures/audio.rs"),
        include_str!("fixtures/ksp.rs"),
    ] {
        source.hash(&mut hasher);
    }
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_fixture_names_itself_and_has_a_script_except_streamed_and_key_position() {
        // Key position starts from a KeyPosition sample-start route, not a script.
        for fixture in Fixture::ALL {
            assert_eq!(fixture.name().parse::<Fixture>(), Ok(fixture));
            assert_eq!(
                fixture.spec().script.is_some(),
                !matches!(fixture, Fixture::Streamed | Fixture::KeyPosition)
            );
        }
    }

    #[test]
    fn key_position_routes_its_sample_start() {
        let spec = Fixture::KeyPosition.spec();
        assert_eq!(spec.start_mod_frames, 3_600);
        assert!(matches!(spec.start_route, Some(StartSource::KeyPosition)));
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
