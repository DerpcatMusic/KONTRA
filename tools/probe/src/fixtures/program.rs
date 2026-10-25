//! Writes one fixture as a Kontakt instrument: WAV samples beside an `.nki`
//! whose groups, zones and script are authored directly, then saved by the
//! library's NKI writer under `Samples/` and `Instruments/`.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use anyhow::{Context, Result};
use kontakto::creator::nki::{self, Program, SampleFile};
use sampler_ir::{AssetRef, Group, GroupRef, KeyRange, KeyTracking, Playback, VelocityRange, Zone};

use super::audio::{self, RATE};
use super::{GroupSpec, ROOTS, Spec};

/// The first key each root's zone covers.
pub(super) fn low_key(index: usize) -> u8 {
    match index.checked_sub(1) {
        Some(previous) => ROOTS[previous].0 + 1,
        None => 0,
    }
}

/// The last key each root's zone covers.
pub(super) fn high_key(index: usize) -> u8 {
    match ROOTS.get(index + 1) {
        Some(&(next, _)) => next - 1,
        None => 127,
    }
}

/// The velocity band of layer `layer` when a fixture has `count` layers.
pub(super) fn velocities(layer: usize, count: usize) -> VelocityRange {
    let scale = |n: usize| u8::try_from(n * 128 / count).unwrap_or(127);
    VelocityRange {
        low: scale(layer),
        high: scale(layer + 1).saturating_sub(1),
    }
}

fn modified(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |age| age.as_secs())
}

fn file_name(spec: &Spec, group: &GroupSpec, note: &str, layer: usize) -> String {
    let mut name = format!("{}_{note}_{}", spec.name, spec.mics[group.mic].tag());
    if spec.rounds > 1 {
        name.push_str(&format!("_rr{}", group.round + 1));
    }
    if spec.layers > 1 {
        name.push_str(&format!("_v{}", layer + 1));
    }
    name
}

/// Where `<out>/<name> (Kontakt)` keeps the fixture's preset.
pub(super) fn preset(spec: &Spec, out: &Path) -> PathBuf {
    out.join(format!("{} (Kontakt)", spec.name))
        .join("Instruments")
        .join(format!("{}.nki", spec.name))
}

/// Writes `spec` under `<out>/<name> (Kontakt)` and returns its `.nki`.
pub(super) fn write(spec: &Spec, out: &Path) -> Result<PathBuf> {
    let root = out.join(format!("{} (Kontakt)", spec.name));
    let instruments = root.join("Instruments");
    let samples = root.join("Samples").join(spec.name);
    std::fs::create_dir_all(&instruments)
        .with_context(|| format!("creating {}", instruments.display()))?;
    std::fs::create_dir_all(&samples).with_context(|| format!("creating {}", samples.display()))?;

    let groups: Vec<Group> = spec
        .groups
        .iter()
        .map(|group| Group {
            name: group.name.clone(),
            ..Group::default()
        })
        .collect();
    let mut files = Vec::new();
    let mut zones = Vec::new();
    for (index, group) in spec.groups.iter().enumerate() {
        let (level, noise) = spec.mics[group.mic].mix();
        for (n, (midi, note)) in ROOTS.iter().enumerate() {
            for layer in 0..spec.layers {
                let gain = if spec.layers > 1 {
                    0.2 * (layer + 1) as f32
                } else {
                    1.0
                };
                let seed = u32::from(*midi) * 31 + group.mic as u32;
                let pcm = audio::tone(*midi, spec.seconds, level * gain, noise, seed);
                let name = file_name(spec, group, note, layer);
                let path = samples.join(&name);
                audio::write_wav(&path, &pcm)?;
                files.push(SampleFile {
                    relative: format!("../Samples/{}/{name}", spec.name),
                    rate: RATE,
                    channels: 1,
                    bits: 16,
                    frames: pcm.len() as u64,
                    modified: modified(&path),
                });
                zones.push(Zone {
                    group: Some(GroupRef(index)),
                    keys: KeyRange {
                        low: low_key(n),
                        high: high_key(n),
                    },
                    velocities: velocities(layer, spec.layers),
                    pitch: KeyTracking::Tracked { root: *midi },
                    playback: Playback {
                        start_range: u64::from(spec.start_mod_frames),
                        ..Playback::default()
                    },
                    ..Zone::new(AssetRef(files.len() - 1))
                });
            }
        }
    }

    let preset = preset(spec, out);
    nki::write(
        &preset,
        &Program {
            name: spec.name,
            author: "KONTRA probe",
            groups: &groups,
            zones: &zones,
            samples: &files,
            script: spec.script,
        },
    )?;
    Ok(preset)
}
