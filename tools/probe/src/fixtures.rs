//! Synthetic instruments that take the paths the audited libraries take, built at
//! run time so CI needs no vendor files and no audio is committed.
//!
//! Each fixture is written as WAV samples under the target directory and compiled
//! by the library creator into a Kontakt instrument (`.nki`), the format the real
//! libraries use. What the creator cannot author is listed in [`Fixture::gaps`].

use std::f32::consts::TAU;
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Mutex, PoisonError};

use anyhow::{Context, Result, ensure};
use kontakto::creator::{self, Options};

/// Sample rate of every fixture's WAV files.
const RATE: u32 = 48_000;
/// Folder version: bump it when the generated instruments change.
const VERSION: u32 = 1;
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
    /// Areia pattern: three mic groups with round robins and a script on every note.
    CcStart,
    /// Vista pattern: two layers, standing in for KeyPosition-driven sample start.
    KeyPosition,
    /// Chord pattern: one mic with round robins, so every chord voice runs a script.
    Dense,
    /// Samples much longer than the head, so voices read from disk after the preload.
    Streamed,
}

impl Fixture {
    /// Every fixture, in run order.
    pub const ALL: [Self; 4] = [
        Self::CcStart,
        Self::KeyPosition,
        Self::Dense,
        Self::Streamed,
    ];

    /// The name on the command line and in the folder name.
    pub fn name(self) -> &'static str {
        match self {
            Self::CcStart => "cc_start",
            Self::KeyPosition => "key_position",
            Self::Dense => "dense",
            Self::Streamed => "streamed",
        }
    }

    /// Parts of the audited pattern the native creator cannot author. Empty when the
    /// fixture carries everything it stands for.
    pub fn gaps(self) -> &'static str {
        match self {
            Self::CcStart => {
                "no CC113 to sample-start route (playPos from CC) and no 300 set_control_par UI writes per note: \
                 the creator writes neither, its only script is the round-robin one"
            }
            Self::KeyPosition => {
                "no KeyPosition/RTC to sample-start route: the creator writes no modulation routes"
            }
            Self::Dense => {
                "no play_note per voice and no ignore_event on the original attack: the creator's only script \
                 is the round-robin one"
            }
            Self::Streamed => "",
        }
    }

    /// Writes the instrument if it is not there yet and returns its `.nki` path.
    ///
    /// # Errors
    /// Fails when the samples cannot be written or the creator rejects them.
    pub fn build(self) -> Result<PathBuf> {
        // Tests share one process: a second caller waits and then finds the folder ready.
        static BUILD: Mutex<()> = Mutex::new(());
        let _guard = BUILD.lock().unwrap_or_else(PoisonError::into_inner);

        let spec = self.spec();
        let dir = output_root()?.join(format!("{}-v{VERSION}", self.name()));
        if !dir.join("ready").exists() {
            let _ = std::fs::remove_dir_all(&dir);
            let samples = dir.join("samples");
            std::fs::create_dir_all(&samples)
                .with_context(|| format!("creating {}", samples.display()))?;
            write_samples(&spec, &samples)?;
            let options = Options {
                source: samples,
                name: spec.name.into(),
                vendor: "KONTRA probe".into(),
                out: dir.join("out"),
            };
            let created = creator::create(&options, &|_| {})
                .with_context(|| format!("creating {}", self.name()))?;
            ensure!(
                created.instruments.len() == 1,
                "{} made {} instruments, expected 1",
                self.name(),
                created.instruments.len()
            );
            std::fs::write(dir.join("ready"), "")?;
        }
        Ok(dir
            .join("out")
            .join(format!("{} (Kontakt)", spec.name))
            .join("Instruments")
            .join(format!("{}.nki", spec.name)))
    }

    fn spec(self) -> Spec {
        match self {
            Self::CcStart => Spec {
                name: "ProbeCcStart",
                mics: &[Mic::Close, Mic::Room, Mic::Far],
                rounds: 4,
                layers: 0,
                seconds: 0.25,
            },
            Self::KeyPosition => Spec {
                name: "ProbeKeyPosition",
                mics: &[Mic::Close],
                rounds: 0,
                layers: 2,
                seconds: 0.25,
            },
            Self::Dense => Spec {
                name: "ProbeDense",
                mics: &[Mic::Close],
                rounds: 4,
                layers: 0,
                seconds: 0.25,
            },
            Self::Streamed => Spec {
                name: "ProbeStreamed",
                mics: &[Mic::Close],
                rounds: 0,
                layers: 0,
                seconds: 2.0,
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
    /// Round robins per mic and note; 0 for none.
    rounds: u32,
    /// Velocity layers per note; 0 for none.
    layers: u32,
    seconds: f32,
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

/// Writes every WAV of `spec` into `dir`, named so the creator reads note, mic,
/// round robin and layer from the name.
fn write_samples(spec: &Spec, dir: &Path) -> Result<()> {
    let rounds: Vec<Option<u32>> = if spec.rounds == 0 {
        vec![None]
    } else {
        (1..=spec.rounds).map(Some).collect()
    };
    let layers: Vec<Option<u32>> = if spec.layers == 0 {
        vec![None]
    } else {
        (1..=spec.layers).map(Some).collect()
    };
    for (midi, note) in ROOTS {
        for (m, &mic) in spec.mics.iter().enumerate() {
            for round in &rounds {
                for layer in &layers {
                    let mut name = format!("{}_{note}_{}", spec.name, mic.tag());
                    if let Some(r) = round {
                        name.push_str(&format!("_rr{r}"));
                    }
                    if let Some(l) = layer {
                        name.push_str(&format!("_v{l}"));
                    }
                    // Layers are told apart by level, so a higher layer plays louder.
                    let gain = layer.map_or(1.0, |l| 0.2 * l as f32);
                    let (level, noise) = mic.mix();
                    let seed = u32::from(midi) * 31 + m as u32;
                    let samples = tone(midi, spec.seconds, level * gain, noise, seed);
                    let path = dir.join(format!("{name}.wav"));
                    std::fs::write(&path, wav(&samples))
                        .with_context(|| format!("writing {}", path.display()))?;
                }
            }
        }
    }
    Ok(())
}

/// A cosine at the pitch of `midi` (A3 = 69 is 440 Hz; the creator checks names
/// against what it hears), with `noise` of hiss and a 10 ms release. It starts at
/// full level on its first frame, so the onset is the first sample.
fn tone(midi: u8, seconds: f32, amp: f32, noise: f32, seed: u32) -> Vec<f32> {
    let hz = 440.0 * 2f32.powf((f32::from(midi) - 69.0) / 12.0);
    let frames = (seconds * RATE as f32) as usize;
    let release = (RATE / 100) as usize;
    let mut state = seed;
    (0..frames)
        .map(|i| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let hiss = (state >> 8) as f32 / (1 << 23) as f32 - 1.0;
            let t = i as f32 / RATE as f32;
            let fade = ((frames - i) as f32 / release as f32).min(1.0);
            (amp * (TAU * hz * t).cos() + noise * hiss) * fade
        })
        .collect()
}

/// A mono 16-bit PCM WAV file.
fn wav(samples: &[f32]) -> Vec<u8> {
    let data = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data as usize);
    out.extend(b"RIFF");
    out.extend((36 + data).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(1u16.to_le_bytes()); // PCM
    out.extend(1u16.to_le_bytes()); // mono
    out.extend(RATE.to_le_bytes());
    out.extend((RATE * 2).to_le_bytes());
    out.extend(2u16.to_le_bytes());
    out.extend(16u16.to_le_bytes());
    out.extend(b"data");
    out.extend(data.to_le_bytes());
    for s in samples {
        out.extend(((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    out
}
