//! UVI interoperability entry points. Container/crypto, program graphs, audio,
//! scripting and DSP retain their own state and validation boundaries.

pub(crate) mod access;
pub(crate) mod cli;
pub use cli::run as cli;

pub mod comb;
pub mod crypto;
pub mod bridge;
pub mod diagnostics;
pub mod dsp;
pub mod effects;
pub mod exciter;
pub mod filter;
pub mod flanger;
pub mod generator;
pub mod host;
pub mod library;
pub mod maximizer;
pub mod modulation;
pub mod ms20;
pub mod phasor;
pub mod playback;
pub mod player;
pub mod program;
pub mod resampling;
pub mod sample;
pub mod script;
pub mod sparkverb;
pub mod state;
pub mod storage;
pub mod time_effects;
pub mod ufs;
#[cfg(feature = "plugin")]
pub(crate) mod ui_assets;
pub mod waveshaper;
pub mod worker;

use crate::{
    audio,
    engine::{Bank, Engine, EventChange, GroupMask, MAX_BLOCK, MAX_GROUPS, NoteEvent},
    import::{Group, Zone},
};
use anyhow::{Context, Result, ensure};
use roxmltree::{Document, Node, ParsingOptions};
use serde::Serialize;
use std::{
    collections::{BTreeMap, HashMap},
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

const XML_LIMIT: usize = 16 << 20;

pub fn read_text(path: &Path) -> Result<String> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take((XML_LIMIT + 1) as u64)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= XML_LIMIT, "UVI text exceeds 16 MiB limit");
    String::from_utf8(bytes).context("UVI text must be UTF-8")
}

fn xml(text: &str) -> Result<Document<'_>> {
    ensure!(text.len() <= XML_LIMIT, "UVI XML exceeds 16 MiB limit");
    let doc = Document::parse_with_options(
        text,
        ParsingOptions {
            allow_dtd: false,
            nodes_limit: 100_000,
            ..Default::default()
        },
    )?;
    ensure!(
        doc.descendants()
            .all(|n| n.ancestors().take(66).count() <= 65),
        "UVI XML exceeds depth limit"
    );
    Ok(doc)
}

fn attributes(node: Node<'_, '_>) -> BTreeMap<String, String> {
    node.attributes()
        .map(|a| (a.name().into(), a.value().into()))
        .collect()
}

pub fn inspect_preset(text: &str) -> Result<program::Program> {
    program::parse_program(text)
}

#[derive(Debug, Serialize)]
pub struct MappingZone {
    pub layer: usize,
    pub variant: u32,
    pub purged: bool,
    pub streaming: bool,
    pub max_sample_start_ms: f64,
    pub attributes: BTreeMap<String, String>,
    pub zone: Zone,
}

#[derive(Debug, Serialize)]
pub struct Mapping {
    pub path: PathBuf,
    pub layers: Vec<String>,
    pub attributes: BTreeMap<String, String>,
    pub zones: Vec<MappingZone>,
    pub warnings: Vec<String>,
}

fn number(node: Node<'_, '_>, name: &str, default: f64) -> Result<f64> {
    let value = node
        .attribute(name)
        .map(str::parse::<f64>)
        .transpose()?
        .unwrap_or(default);
    ensure!(value.is_finite(), "Nonfinite UVI attribute {name}");
    Ok(value)
}

fn integer(node: Node<'_, '_>, name: &str, default: i64, lo: i64, hi: i64) -> Result<i64> {
    Ok(node
        .attribute(name)
        .map(str::parse::<i64>)
        .transpose()?
        .unwrap_or(default)
        .clamp(lo, hi))
}

pub fn read_mapping(path: &Path) -> Result<Mapping> {
    parse_mapping(path, &read_text(path)?)
}

pub fn parse_mapping(path: &Path, text: &str) -> Result<Mapping> {
    let doc = xml(text)?;
    let root = doc.root_element();
    ensure!(
        root.has_tag_name("layers"),
        "Expected UVI layers mapping root"
    );
    ensure!(
        integer(root, "channelMax", 2, 1, i64::MAX)? <= 2,
        "Multichannel UVI mapping playback is unavailable"
    );
    let mut mapping = Mapping {
        path: path.into(),
        layers: Vec::new(),
        attributes: attributes(root),
        zones: Vec::new(),
        warnings: Vec::new(),
    };
    let start = number(root, "maxSampleStart", 0.)?.max(0.);
    for layer in root.children().filter(|n| n.has_tag_name("layer")) {
        let layer_index = mapping.layers.len();
        mapping
            .layers
            .push(layer.attribute("name").unwrap_or_default().into());
        let start = number(layer, "maxSampleStart", start)?.max(0.);
        for node in layer.children().filter(|n| n.has_tag_name("zone")) {
            let Some(sample) = node.attribute("path").filter(|s| !s.is_empty()) else {
                mapping
                    .warnings
                    .push(format!("Layer {layer_index}: skipped zone without path"));
                continue;
            };
            let key = |name, default| integer(node, name, default, 0, 127).map(|v| v as u8);
            let vel = |name, default| integer(node, name, default, 1, 127).map(|v| v as u8);
            let (low_key, high_key, low_velocity, high_velocity) = (
                key("lowKey", 0)?,
                key("highKey", 127)?,
                vel("lowVel", 1)?,
                vel("highVel", 127)?,
            );
            if low_key > high_key || low_velocity > high_velocity {
                mapping
                    .warnings
                    .push(format!("Layer {layer_index}: skipped inverted zone range"));
                continue;
            }
            let sample = PathBuf::from(sample.replace('\\', "/"));
            // Windows drive paths and UFS volume aliases need an actual resolver.
            ensure!(
                !sample.to_string_lossy().contains(':')
                    && !sample.to_string_lossy().starts_with('$'),
                "Unresolved UVI sample path {}",
                sample.display()
            );
            let sample = if sample.is_absolute() {
                sample
            } else {
                path.parent().unwrap_or(Path::new(".")).join(sample)
            };
            let gain = 10f64.powf(number(node, "gain", 0.)? / 20.) as f32;
            let tune = 2f64.powf(number(node, "tune", 0.)? / 1200.);
            ensure!(
                gain.is_finite() && tune.is_finite() && tune > 0.,
                "UVI gain/tune exceeds playback range"
            );
            let index = mapping.zones.len();
            ensure!(
                index < MAX_GROUPS,
                "UVI mapping exceeds engine zone/group limit"
            );
            mapping.zones.push(MappingZone {
                layer: layer_index,
                variant: integer(node, "rr", 1, 1, u32::MAX as i64)? as u32 - 1,
                purged: integer(node, "purged", 0, 0, 1)? != 0,
                streaming: integer(node, "streaming", 1, 0, 1)? != 0,
                max_sample_start_ms: number(node, "maxSampleStart", start)?.max(0.),
                attributes: attributes(node),
                zone: Zone {
                    group: index,
                    available: sample.is_file(),
                    sample,
                    low_key,
                    high_key,
                    low_velocity,
                    high_velocity,
                    root: key("baseNote", 60)?,
                    gain,
                    tune,
                    ..Default::default()
                },
            });
        }
    }
    ensure!(
        !mapping.zones.is_empty(),
        "UVI mapping contains no valid zones"
    );
    Ok(mapping)
}

impl Mapping {
    /// All samples are resident for this offline renderer. Keep dimension
    /// selection in the UVI model; each selected zone targets one engine group.
    pub fn load_bank(&self) -> Result<Bank> {
        let mut samples = Vec::new();
        let mut frames_left = (256 << 20) / std::mem::size_of::<audio::Frame>();
        for mapped in self.zones.iter().filter(|z| !z.purged) {
            let path = &mapped.zone.sample;
            if samples.iter().any(|(p, _)| p == path) {
                continue;
            }
            let mut reader = audio::Sources::default().source(path)?.open()?;
            let header = reader.header();
            let frames = usize::try_from(header.frames)?;
            ensure!(
                frames <= frames_left,
                "UVI offline sample data exceeds 256 MiB limit"
            );
            let mut sample = audio::Sample {
                rate: header.rate,
                frames: vec![[0.; 2]; frames],
            };
            reader.read(0, &mut sample.frames)?;
            frames_left -= frames;
            samples.push((path.clone(), sample));
        }
        let groups = self
            .zones
            .iter()
            .map(|z| Group {
                name: self.layers[z.layer].clone(),
                muted: z.purged,
                ..Default::default()
            })
            .collect();
        // Purged zones retain their identity but are not submitted to the bank.
        let zones = self
            .zones
            .iter()
            .filter(|z| !z.purged)
            .map(|mapped| {
                let mut zone = mapped.zone.clone();
                let (_, sample) = samples
                    .iter()
                    .find(|(path, _)| path == &zone.sample)
                    .expect("Loaded UVI sample");
                zone.start_mod = Some(
                    (mapped.max_sample_start_ms * f64::from(sample.rate) / 1000.)
                        .min(sample.frames.len().saturating_sub(1) as f64)
                        .min(u32::MAX as f64) as u32,
                );
                zone
            })
            .collect();
        Bank::from_samples(groups, zones, samples)
    }

    fn select(&self, note: &script::Note, cycle: &mut [u64]) -> Option<usize> {
        let candidates = || {
            self.zones.iter().enumerate().filter(|(_, z)| {
                !z.purged
                    && z.layer == note.dim1
                    && (z.zone.low_key..=z.zone.high_key).contains(&note.note)
                    && (z.zone.low_velocity..=z.zone.high_velocity).contains(&note.velocity)
            })
        };
        let mut variants: Vec<_> = candidates().map(|(_, z)| z.variant).collect();
        variants.sort_unstable();
        variants.dedup();
        if variants.is_empty() {
            return None;
        }
        let variant = note.dim2.unwrap_or_else(|| {
            let count = &mut cycle[note.dim1];
            let variant = variants[(*count % variants.len() as u64) as usize];
            *count = count.wrapping_add(1);
            variant
        });
        candidates()
            .find(|(_, z)| z.variant == variant)
            .map(|(i, _)| i)
    }
}

pub fn render(
    mapping: &Mapping,
    commands: &[script::Command],
    frames: u64,
    mut write: impl FnMut([f32; 2]) -> Result<()>,
) -> Result<()> {
    ensure!(
        frames <= 48000 * 60,
        "UVI offline render exceeds 60 seconds"
    );
    ensure!(
        commands.windows(2).all(|w| w[0].frame <= w[1].frame),
        "Unsorted UVI command stream"
    );
    let mut engine = Engine::default();
    engine.set_bank(Some(Box::new(mapping.load_bank()?)));
    struct PostedVoice {
        // Key release consumes a gate, but the engine ID still addresses its
        // draining voice for subsequent gain, tune, pan and fade commands.
        event: crate::engine::EventId,
        note: u8,
        released: bool,
        volume: f32,
        tune: f64,
        pan: f32,
    }
    let mut voices: HashMap<u32, Vec<PostedVoice>> = HashMap::new();
    let mut cycle = vec![0; mapping.layers.len()];
    let (mut at, mut next) = (0, 0);
    let (mut left, mut right) = ([0.; MAX_BLOCK], [0.; MAX_BLOCK]);
    while at < frames {
        while let Some(command) = commands.get(next).filter(|c| c.frame <= at) {
            use script::Action;
            match &command.action {
                Action::ChokeRoot => {
                    anyhow::bail!("Hosted root choking requires the native Program renderer")
                }
                Action::Start(note) => {
                    if let Some(index) = mapping.select(note, &mut cycle) {
                        let mut groups = GroupMask::none();
                        groups.set(index, true);
                        let mut event = NoteEvent::new(note.channel, note.note, note.velocity);
                        event.groups = Some(&groups);
                        event.volume = note.volume;
                        event.tune = note.tune;
                        event.pan = note.pan;
                        event.offset_us = note.offset_us;
                        if let Some(id) = engine.start_event(&event) {
                            voices.entry(note.id).or_default().push(PostedVoice {
                                event: id,
                                note: note.note,
                                released: false,
                                volume: note.volume,
                                tune: note.tune,
                                pan: note.pan,
                            });
                        }
                    }
                }
                Action::Release(id) => {
                    if let Some(posted) = voices
                        .get_mut(id)
                        .and_then(|v| v.last_mut())
                        .filter(|v| !v.released)
                    {
                        posted.released = true;
                        engine.release_event(posted.event);
                    }
                }
                Action::ReleaseNote {
                    id,
                    note,
                    channel: _,
                    layer,
                } => {
                    ensure!(
                        layer.is_none(),
                        "Scoped native layers are not part of a standalone DMAP"
                    );
                    if let Some(events) = voices.get_mut(id) {
                        if let Some(posted) = events
                            .iter_mut()
                            .find(|v| v.note == *note && !v.released)
                        {
                            posted.released = true;
                            engine.release_event(posted.event);
                        }
                    }
                }
                Action::Controller {
                    channel,
                    controller,
                    value,
                } => engine.cc(*channel, *controller, *value),
                Action::ControllerAll { controller, value } => {
                    for channel in 0..16 {
                        engine.cc(channel, *controller, *value);
                    }
                }
                Action::PitchBend { channel, bend } => {
                    ensure!(
                        channel < &16 && bend.is_finite() && (-1. ..=1.).contains(bend),
                        "Invalid UVI pitch bend"
                    );
                    let value = if *bend < 0. {
                        8192. + bend * 8192.
                    } else {
                        8192. + bend * 8191.
                    };
                    engine.pitch_bend(*channel, value.round() as u16);
                }
                Action::AfterTouch { channel, value } => engine.channel_pressure(*channel, *value),
                Action::PolyAfterTouch {
                    channel,
                    note,
                    value,
                } => {
                    engine.poly_pressure(*channel, *note, *value);
                }
                Action::PolyAfterTouchAll { note, value } => {
                    for channel in 0..16 {
                        engine.poly_pressure(channel, *note, *value);
                    }
                }
                Action::Transport {
                    playing,
                    beat,
                    tempo,
                } => engine.set_transport(*playing, *tempo, *beat, (4, 4)),
                Action::Fade {
                    id,
                    start,
                    target,
                    duration_frames,
                    kill,
                    layer,
                } => {
                    ensure!(
                        layer.is_none(),
                        "Scoped native layers are not part of a standalone DMAP"
                    );
                    ensure!(
                        !kill || *target == 0.,
                        "Nonzero-target fade termination requires native Program playback"
                    );
                    if let Some(events) = voices.get(id) {
                        for event in events.iter().map(|v| v.event) {
                            if let Some(start) = start {
                                engine.fade_event(event, 0., *start, false);
                            }
                            engine.fade_event(
                                event,
                                *duration_frames as f32 / 48000.,
                                *target,
                                *kill,
                            );
                        }
                    }
                }
                Action::Change {
                    id,
                    gain,
                    tune,
                    pan,
                    layer,
                    relative,
                } => {
                    ensure!(
                        layer.is_none(),
                        "Scoped native layers are not part of a standalone DMAP"
                    );
                    if let Some(events) = voices.get_mut(id) {
                        for voice in events {
                            if let Some(value) = gain {
                                voice.volume = if *relative { voice.volume * value } else { *value };
                                ensure!(voice.volume.is_finite() && voice.volume >= 0., "Invalid UVI note gain");
                                engine.change_event(voice.event, EventChange::Volume(voice.volume));
                            }
                            if let Some(value) = tune {
                                voice.tune = if *relative { voice.tune + value } else { *value };
                                ensure!(voice.tune.is_finite() && voice.tune.abs() <= 120., "Unsupported UVI note tuning");
                                engine.change_event(voice.event, EventChange::Tune(voice.tune));
                            }
                            if let Some(value) = pan {
                                voice.pan = if *relative { voice.pan + value } else { *value };
                                ensure!((-1. ..=1.).contains(&voice.pan), "Unsupported UVI note pan");
                                engine.change_event(voice.event, EventChange::Pan(voice.pan));
                            }
                        }
                    }
                }
            }
            next += 1;
        }
        let end = commands
            .get(next)
            .map_or(frames, |c| c.frame)
            .min(frames)
            .min(at + MAX_BLOCK as u64);
        let count = (end - at) as usize;
        engine.render(&mut left[..count], &mut right[..count]);
        for (&l, &r) in left[..count].iter().zip(&right[..count]) {
            ensure!(l.is_finite() && r.is_finite(), "Nonfinite UVI audio");
            write([l, r])?;
        }
        at = end;
    }
    Ok(())
}

pub(super) fn write_wav(
    path: &Path,
    rate: u32,
    run: impl FnOnce(&mut dyn FnMut(audio::Frame) -> Result<()>) -> Result<()>,
) -> Result<f32> {
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    let result = (|| {
        let mut writer = hound::WavWriter::new(
            std::io::BufWriter::new(file),
            hound::WavSpec {
                channels: 2,
                sample_rate: rate,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )?;
        let mut peak = 0f32;
        run(&mut |frame| {
            for value in frame {
                peak = peak.max(value.abs());
                writer.write_sample(value)?;
            }
            Ok(())
        })?;
        writer.finalize()?;
        Ok(peak)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(path);
    }
    result
}

/// Decode a proven byte span with the existing bounded audio source reader.
/// The caller must recover its boundaries; this does not invent a UFS index.
pub fn open_member(path: &Path, offset: u64, length: u64) -> Result<audio::SampleReader> {
    ensure!(
        length >= 42
            && offset
                .checked_add(length)
                .is_some_and(|end| end <= path.metadata().map_or(0, |m| m.len())),
        "UVI member range is outside the file"
    );
    audio::Sources::default()
        .rebuild(
            Path::new("uvi-member.flac"),
            path.into(),
            offset,
            Some(length),
            false,
        )?
        .open()
}
