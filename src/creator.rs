//! The library creator: a folder of samples in, playable instruments out,
//! saved twice: as a KONTRA library (`.kontra-instrument` presets and a
//! `kontra-library.json` manifest) and as a Kontakt library (`.nki`
//! presets, with an `.sfz` beside each).
//!
//! Samples become instruments by what their paths say ([`names`]): the
//! words that are not a note, velocity layer, round robin or microphone
//! name the instrument. A sample with no note in its name is pitched by ear
//! ([`pitch`]). Each round robin and microphone is a group; round robins
//! take turns by a small KSP script, which Kontakt and KONTRA both run.
//! Within a group, each velocity layer spreads its roots over the keyboard,
//! split halfway between neighbours. Loops come from WAV `smpl` chunks.
//! Every instrument gets a report of how it was mapped and what was unsure.
//!
//! Layout of what is written, under the output folder:
//! `<Name>/` (KONTRA) and `<Name> (Kontakt)/`, each with `Instruments/`,
//! `Samples/<instrument>/` and `Reports/`.

pub mod names;
mod nki;
pub mod pitch;

use crate::import::{Ahdsr, Group, Instrument, Loop, Zone};
use anyhow::{Context, Result, bail, ensure};
use names::{Parsed, Velocity};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// The extension of a KONTRA instrument preset.
pub const NATIVE: &str = "kontra-instrument";
/// The file that makes a folder a KONTRA library and names it.
pub const MANIFEST: &str = "kontra-library.json";

pub fn is_native(path: &Path) -> bool {
    path.extension().is_some_and(|x| x.eq_ignore_ascii_case(NATIVE))
}

/// A preset either player opens: an NKI or a KONTRA instrument.
pub fn is_instrument(path: &Path) -> bool {
    is_native(path) || path.extension().is_some_and(|x| x.eq_ignore_ascii_case("nki"))
}

/// What to create.
pub struct Options {
    pub source: PathBuf,
    /// The library's name; the source folder's when empty.
    pub name: String,
    pub vendor: String,
    pub out: PathBuf,
    pub kontra: bool,
    pub kontakt: bool,
}

/// What was created.
#[derive(Debug, Default)]
pub struct Created {
    pub kontra: Option<PathBuf>,
    pub kontakt: Option<PathBuf>,
    pub instruments: Vec<Summary>,
    /// Files left out: not audio KONTRA decodes.
    pub skipped: Vec<PathBuf>,
}

#[derive(Debug)]
pub struct Summary {
    pub name: String,
    pub samples: usize,
    pub groups: usize,
    pub layers: usize,
    pub zones: usize,
    /// Lines of the report that need a look.
    pub issues: Vec<String>,
}

/// A library's manifest.
#[derive(Serialize, Deserialize)]
struct Manifest {
    format: String,
    version: u32,
    name: String,
    #[serde(default)]
    vendor: String,
}

/// The name and vendor of the KONTRA library in `dir`, if it is one.
pub fn manifest(dir: &Path) -> Option<(String, String)> {
    let m: Manifest = serde_json::from_slice(&std::fs::read(dir.join(MANIFEST)).ok()?).ok()?;
    (m.format == "kontra-library" && !m.name.trim().is_empty()).then(|| (m.name, m.vendor))
}

/// A KONTRA instrument preset: what KONTRA plays, sample paths relative to it.
#[derive(Serialize, Deserialize)]
struct Native {
    format: String,
    version: u32,
    name: String,
    groups: Vec<Group>,
    zones: Vec<Zone>,
    #[serde(default)]
    scripts: Vec<String>,
}

/// Open a KONTRA instrument preset.
pub fn read_native(path: &Path) -> Result<Instrument> {
    let native: Native = serde_json::from_slice(&std::fs::read(path).with_context(|| format!("Reading {}", path.display()))?)
        .with_context(|| format!("{} is not a KONTRA instrument", path.display()))?;
    ensure!(native.format == "kontra-instrument", "Not a KONTRA instrument");
    ensure!(native.version <= 1, "Saved by a newer KONTRA");
    ensure!(native.groups.len() <= crate::engine::MAX_GROUPS, "Too many groups");
    ensure!(native.zones.iter().all(|z| z.group < native.groups.len()), "A zone refers to a missing group");
    let path = path.canonicalize()?;
    let dir = path.parent().context("Preset has no folder")?;
    let mut missing = BTreeSet::new();
    let zones = native
        .zones
        .into_iter()
        .map(|mut z| {
            z.sample = dir.join(&z.sample);
            z.sample = z.sample.canonicalize().unwrap_or(z.sample);
            // Tune has f32 precision, as in Kontakt; JSON's last digit may be a ulp off.
            z.tune = f64::from(z.tune as f32);
            z.available = z.sample.is_file();
            if !z.available {
                missing.insert(z.sample.to_string_lossy().into_owned());
            }
            z
        })
        .collect();
    Ok(Instrument {
        script_state: vec![Default::default(); native.scripts.len()],
        path,
        name: native.name,
        groups: native.groups,
        zones,
        scripts: native.scripts,
        missing_samples: missing.into_iter().collect(),
        ..Default::default()
    })
}

// --- scanning --------------------------------------------------------------

/// One sample file and what is known of it.
#[derive(Debug, Clone)]
struct Source {
    path: PathBuf,
    parsed: Parsed,
    rate: u32,
    channels: u16,
    bits: u16,
    frames: u64,
    /// Unity note from a `smpl` chunk.
    unity: Option<u8>,
    loop_range: Option<(u64, u64)>,
}

/// Format, length and loop of a WAV or AIFF file, from its headers.
fn probe(path: &Path) -> Result<(u32, u16, u16, u64, Option<u8>, Option<(u64, u64)>)> {
    use std::io::Read;
    let mut head = Vec::new();
    // Headers and smpl chunks sit in the first few kilobytes, or right after the data.
    std::fs::File::open(path)?.take(1 << 16).read_to_end(&mut head)?;
    let len = std::fs::metadata(path)?.len();
    ensure!(head.len() >= 12, "Too short for audio");
    let le32 = |b: &[u8], at: usize| u32::from_le_bytes(b[at..at + 4].try_into().unwrap());
    let be32 = |b: &[u8], at: usize| u32::from_be_bytes(b[at..at + 4].try_into().unwrap());
    match &head[..4] {
        b"RIFF" => {
            let (mut fmt, mut data_len, mut unity, mut loop_range) = (None, None, None, None);
            let mut at = 12u64;
            let mut file = std::fs::File::open(path)?;
            while at + 8 <= len {
                let mut header = [0; 8];
                use std::io::Seek;
                file.seek(std::io::SeekFrom::Start(at))?;
                file.read_exact(&mut header)?;
                let size = le32(&header, 4) as u64;
                match &header[..4] {
                    b"fmt " | b"smpl" => {
                        let mut body = vec![0; size.min(4096) as usize];
                        file.read_exact(&mut body)?;
                        if &header[..4] == b"fmt " && body.len() >= 16 {
                            fmt = Some((u16::from_le_bytes([body[2], body[3]]), le32(&body, 4), u16::from_le_bytes([body[14], body[15]])));
                        } else if body.len() >= 36 {
                            unity = u8::try_from(le32(&body, 12)).ok().filter(|n| *n < 128);
                            if le32(&body, 28) > 0 && body.len() >= 60 {
                                let (start, end) = (le32(&body, 44) as u64, le32(&body, 48) as u64);
                                loop_range = (end > start).then_some((start, end + 1));
                            }
                        }
                    }
                    b"data" => data_len = Some(size.min(len - at - 8)),
                    _ => {}
                }
                at += 8 + size + (size & 1);
            }
            let (channels, rate, bits) = fmt.context("WAV has no fmt chunk")?;
            ensure!(channels > 0 && bits > 0 && rate > 0, "WAV format is invalid");
            let frames = data_len.context("WAV has no data chunk")? / (u64::from(channels) * u64::from(bits.div_ceil(8)));
            Ok((rate, channels, bits, frames, unity, loop_range))
        }
        b"FORM" => {
            let mut at = 12;
            while at + 8 <= head.len() {
                let size = be32(&head, at + 4) as usize;
                if &head[at..at + 4] == b"COMM" && at + 26 <= head.len() {
                    let c = &head[at + 8..];
                    let channels = u16::from_be_bytes([c[0], c[1]]);
                    let frames = be32(c, 2) as u64;
                    let bits = u16::from_be_bytes([c[6], c[7]]);
                    // 80-bit extended rate: exponent then a 64-bit mantissa.
                    let exponent = i32::from(u16::from_be_bytes([c[8], c[9]]) & 0x7fff) - 16383 - 63;
                    let mantissa = u64::from_be_bytes(c[10..18].try_into().unwrap());
                    let rate = (mantissa as f64 * 2f64.powi(exponent)).round() as u32;
                    return Ok((rate, channels, bits, frames, None, None));
                }
                at += 8 + size + (size & 1);
            }
            bail!("AIFF has no COMM chunk")
        }
        _ => bail!("Not a WAV or AIFF file"),
    }
}

/// The first second of the sample's body, as mono.
fn mono(path: &Path, frames: u64, rate: u32) -> Result<Vec<f32>> {
    let mut reader = crate::audio::Sources::default().source(path)?.open()?;
    let mut buf = vec![[0.0f32; 2]; frames.min(u64::from(rate) * 3 / 2) as usize];
    reader.read(0, &mut buf)?;
    Ok(buf.iter().map(|[l, r]| 0.5 * (l + r)).collect())
}

const AUDIO: [&str; 4] = ["wav", "wave", "aif", "aiff"];

fn scan(source: &Path, skipped: &mut Vec<PathBuf>) -> Result<Vec<Source>> {
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(source).follow_links(true).sort_by_file_name() {
        let entry = entry?;
        let path = entry.path();
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        let hidden = entry.file_name().to_string_lossy().starts_with('.');
        if !entry.file_type().is_file() || hidden {
            continue;
        }
        if !AUDIO.contains(&ext.as_str()) {
            if matches!(ext.as_str(), "flac" | "ogg" | "mp3" | "ncw") {
                skipped.push(path.into());
            }
            continue;
        }
        let relative = path.strip_prefix(source).unwrap_or(path);
        let mut components: Vec<String> =
            relative.parent().into_iter().flat_map(|p| p.components()).map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
        components.push(path.file_stem().unwrap_or_default().to_string_lossy().into_owned());
        let parsed = names::parse(&components.iter().map(String::as_str).collect::<Vec<_>>());
        match probe(path) {
            Ok((rate, channels, bits, frames, unity, loop_range)) if frames > 0 => {
                out.push(Source { path: path.into(), parsed, rate, channels, bits, frames, unity, loop_range })
            }
            _ => skipped.push(path.into()),
        }
    }
    Ok(out)
}

// --- mapping ---------------------------------------------------------------

/// Note name in KONTRA's naming (C3 = 60).
pub fn note_text(note: i32) -> String {
    const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    format!("{}{}", NAMES[note.rem_euclid(12) as usize], note.div_euclid(12) - 2)
}

/// Key ranges for `roots` (sorted, distinct): each reaches halfway to its
/// neighbours, the ends to the ends of the keyboard.
pub fn split_keys(roots: &[u8]) -> Vec<(u8, u8)> {
    (0..roots.len())
        .map(|n| {
            let low = if n == 0 { 0 } else { (u16::from(roots[n - 1]) + u16::from(roots[n])) as u8 / 2 + 1 };
            let high = if n + 1 == roots.len() { 127 } else { ((u16::from(roots[n]) + u16::from(roots[n + 1])) / 2) as u8 };
            (low, high)
        })
        .collect()
}

/// Velocity ranges for the layers, softest first: from the top velocities
/// they name when all do, else evenly spread.
pub fn split_velocities(layers: &[Option<Velocity>]) -> Vec<(u8, u8)> {
    let uppers: Option<Vec<u8>> = layers.iter().map(|l| if let Some(Velocity::Upper(v)) = l { Some(*v) } else { None }).collect();
    if let Some(uppers) = uppers {
        return (0..uppers.len())
            .map(|n| (if n == 0 { 0 } else { uppers[n - 1].saturating_add(1) }, if n + 1 == uppers.len() { 127 } else { uppers[n] }))
            .collect();
    }
    let count = layers.len().max(1);
    (0..count).map(|n| ((n * 128 / count) as u8, ((n + 1) * 128 / count - 1) as u8)).collect()
}

/// An instrument made of some sources.
struct Plan {
    name: String,
    /// Each source's root and how it was found.
    roots: Vec<(i32, f64, String)>,
    groups: Vec<Group>,
    /// Zones; `sample` is the source index as a path, replaced on writing.
    zones: Vec<(Zone, usize)>,
    layers: usize,
    round_robins: usize,
    script: Option<String>,
    issues: Vec<String>,
}

fn plan(name: &str, sources: &[Source], progress: &dyn Fn(&str)) -> Plan {
    let mut issues = Vec::new();
    // Octave naming: compare named notes with what is heard, on a few.
    let mut shifts = BTreeMap::<i32, usize>::new();
    for s in sources.iter().filter(|s| s.parsed.note_name.is_some()).take(6) {
        if let Some((heard, confidence)) = mono(&s.path, s.frames, s.rate).ok().and_then(|m| pitch::detect(&m, s.rate))
            && confidence > 0.8
        {
            let shift = ((heard - s.parsed.note_name.unwrap() as f32) / 12.0).round() as i32 * 12;
            *shifts.entry(shift).or_default() += 1;
        }
    }
    let shift = shifts.iter().max_by_key(|(_, n)| **n).map_or(0, |(s, _)| *s).clamp(-24, 24);
    if shift != 0 {
        issues.push(format!(
            "Note names read with C{} = MIDI 60 (heard pitch disagreed with C3 = 60 by {shift} semitones)",
            3 - shift / 12
        ));
    }
    let roots: Vec<(i32, f64, String)> = sources
        .iter()
        .map(|s| {
            let file = s.path.file_name().unwrap_or_default().to_string_lossy();
            if let Some(n) = s.parsed.note_name {
                return ((n + shift).clamp(0, 127), 1.0, "name".into());
            }
            if let Some(n) = s.parsed.note_number {
                return (n, 1.0, "MIDI number".into());
            }
            progress(&format!("Listening to {file}"));
            let heard = mono(&s.path, s.frames, s.rate).ok().and_then(|m| pitch::detect(&m, s.rate));
            match (heard, s.unity) {
                (Some((midi, confidence)), _) if confidence >= 0.8 => {
                    let root = midi.round() as i32;
                    let cents = (midi - root as f32) * 100.0;
                    // Retune so the root key plays the note in tune.
                    // Kontakt stores tune as f32; round here so both formats agree exactly.
                    let tune = f64::from(2f32.powf(-cents / 1200.0));
                    (root.clamp(0, 127), tune, format!("pitch {:.0}% sure, {cents:+.0} cents", confidence * 100.0))
                }
                (heard, Some(unity)) => {
                    if let Some((_, c)) = heard {
                        issues.push(format!("{file}: pitch unclear ({:.0}% sure); used the smpl unity note", c * 100.0));
                    }
                    (i32::from(unity), 1.0, "smpl unity note".into())
                }
                (heard, None) => {
                    issues.push(format!(
                        "{file}: no note in the name and the pitch is unclear ({}); mapped at C3",
                        heard.map_or("no pitch".into(), |(_, c)| format!("{:.0}% sure", c * 100.0))
                    ));
                    (60, 1.0, "default C3".into())
                }
            }
        })
        .collect();
    for s in sources.iter().filter(|s| !s.parsed.ambiguous.is_empty()) {
        issues.push(format!(
            "{}: numbers {} were not read as anything",
            s.path.file_name().unwrap_or_default().to_string_lossy(),
            s.parsed.ambiguous.join(", ")
        ));
    }

    // Groups: one per round robin and microphone.
    let rrs: BTreeSet<Option<u32>> = sources.iter().map(|s| s.parsed.round_robin).collect();
    let mics: BTreeSet<Option<String>> = sources.iter().map(|s| s.parsed.mic.as_ref().map(|m| m.to_lowercase())).collect();
    let layers: Vec<Option<Velocity>> = sources.iter().map(|s| s.parsed.velocity).collect::<BTreeSet<_>>().into_iter().collect();
    if rrs.len() > 1 && rrs.contains(&None) {
        issues.push("Some samples name a round robin and others none; those count as a round robin of their own".into());
    }
    if layers.len() > 1 && layers.contains(&None) {
        issues.push("Some samples name a velocity layer and others none; those are the softest layer".into());
    }
    let velocities = split_velocities(&layers);
    let mut groups = Vec::new();
    let mut group_of = BTreeMap::new();
    for (r, rr) in rrs.iter().enumerate() {
        for mic in &mics {
            let mut label = Vec::new();
            if rrs.len() > 1 {
                label.push(format!("RR{}", r + 1));
            }
            if let Some(m) = mic {
                label.push(sources.iter().find_map(|s| s.parsed.mic.clone().filter(|x| x.to_lowercase() == *m)).unwrap_or_default());
            }
            group_of.insert((*rr, mic.clone()), groups.len());
            groups.push(Group {
                name: if label.is_empty() { name.to_string() } else { label.join(" ") },
                volume_env: Some(Ahdsr { attack_curve: 0.0, attack_ms: 0.0, decay_ms: 0.0, hold_ms: 0.0, release_ms: 250.0, sustain: 1.0, unknown_flag: 0, unknown_tail: Vec::new() }),
                ..Default::default()
            });
        }
    }

    // Zones: per group and layer, the roots split the keyboard.
    let mut cells: BTreeMap<(usize, usize), Vec<(u8, usize)>> = BTreeMap::new();
    for (n, s) in sources.iter().enumerate() {
        let group = group_of[&(s.parsed.round_robin, s.parsed.mic.as_ref().map(|m| m.to_lowercase()))];
        let layer = layers.iter().position(|l| *l == s.parsed.velocity).unwrap();
        cells.entry((group, layer)).or_default().push((roots[n].0 as u8, n));
    }
    let mut zones = Vec::new();
    for ((group, layer), mut members) in cells {
        members.sort();
        let mut kept: Vec<(u8, usize)> = Vec::new();
        for (root, n) in members {
            if let Some(&(_, first)) = kept.iter().find(|(r, _)| *r == root) {
                issues.push(format!(
                    "{} and {} share root {} in group {} layer {}; the second was left out",
                    sources[first].path.file_name().unwrap_or_default().to_string_lossy(),
                    sources[n].path.file_name().unwrap_or_default().to_string_lossy(),
                    note_text(i32::from(root)),
                    groups[group].name,
                    layer + 1
                ));
            } else {
                kept.push((root, n));
            }
        }
        let keys = split_keys(&kept.iter().map(|(r, _)| *r).collect::<Vec<_>>());
        for ((root, n), (low_key, high_key)) in kept.into_iter().zip(keys) {
            let s = &sources[n];
            let (low_velocity, high_velocity) = velocities[layer];
            let loop_range = s
                .loop_range
                .filter(|(_, end)| *end <= s.frames)
                .map(|(start, end)| Loop { start: start as usize, end: end as usize, alternating: false, until_release: false, crossfade: 0 });
            zones.push((
                Zone { group, low_key, high_key, root, low_velocity, high_velocity, tune: roots[n].1, loop_range, ..Default::default() },
                n,
            ));
        }
    }
    // Round robins take turns: a script lets one round robin's groups play per note.
    let script = (rrs.len() > 1).then(|| {
        let of: Vec<String> = (0..groups.len()).map(|g| (g / mics.len()).to_string()).collect();
        format!(
            "on init\n  set_script_title(\"Round Robin\")\n  declare $rr := 0\n  declare $g\n  declare %rr_of_group[{}] := ({})\nend on\n\non note\n  disallow_group($ALL_GROUPS)\n  $g := 0\n  while ($g < {})\n    if (%rr_of_group[$g] = $rr)\n      allow_group($g)\n    end if\n    inc($g)\n  end while\n  $rr := ($rr + 1) mod {}\nend on\n",
            groups.len(),
            of.join(", "),
            groups.len(),
            rrs.len()
        )
    });
    Plan { name: name.into(), roots, groups, zones, layers: layers.len(), round_robins: rrs.len(), script, issues }
}

// --- writing ---------------------------------------------------------------

/// A name safe as a file or folder name everywhere.
fn file_safe(name: &str) -> String {
    let cleaned: String = name.chars().map(|c| if c.is_control() || r#"/\:*?"<>|"#.contains(c) { '_' } else { c }).collect();
    let cleaned = cleaned.trim().trim_matches('.').to_string();
    if cleaned.is_empty() { "Untitled".into() } else { cleaned }
}

/// Copy (or hard-link) `from` to `to`, replacing what is there.
fn place(from: &Path, to: &Path) -> Result<()> {
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let _ = std::fs::remove_file(to);
    if std::fs::hard_link(from, to).is_err() {
        std::fs::copy(from, to).with_context(|| format!("Copying {}", from.display()))?;
    }
    Ok(())
}

fn sfz(plan: &Plan, relative: &[String]) -> String {
    let mut out = format!("// {} — made by KONTRA's library creator\n<global> ampeg_release=0.25\n", plan.name);
    // Groups run round robin by round robin, each with every microphone.
    let rrs = plan.round_robins;
    let mics = plan.groups.len() / rrs;
    for (g, group) in plan.groups.iter().enumerate() {
        let _ = write!(out, "\n<group> group_label={}", group.name.replace(' ', "_"));
        if rrs > 1 {
            let _ = write!(out, " seq_length={rrs} seq_position={}", g / mics + 1);
        }
        out.push('\n');
        for (z, n) in plan.zones.iter().filter(|(z, _)| z.group == g) {
            let _ = write!(
                out,
                "<region> sample={} lokey={} hikey={} pitch_keycenter={} lovel={} hivel={}",
                relative[*n], z.low_key, z.high_key, z.root, z.low_velocity.max(1), z.high_velocity
            );
            if (z.tune - 1.0).abs() > 1e-6 {
                let _ = write!(out, " tune={:.0}", 1200.0 * z.tune.log2());
            }
            if let Some(l) = &z.loop_range {
                let _ = write!(out, " loop_mode=loop_continuous loop_start={} loop_end={}", l.start, l.end - 1);
            }
            out.push('\n');
        }
    }
    out
}

fn report(plan: &Plan, sources: &[Source], relative: &[String], source_root: &Path) -> String {
    let mut out = format!(
        "Instrument: {}\nFrom: {}\nSamples: {} · groups: {} · velocity layers: {} · zones: {}\n",
        plan.name,
        source_root.display(),
        sources.len(),
        plan.groups.len(),
        plan.layers,
        plan.zones.len()
    );
    if plan.script.is_some() {
        out.push_str("Round robins: groups take turns by the \"Round Robin\" script.\n");
    }
    let _ = writeln!(out, "\n{:<40} {:<5} {:<36} {:<8} {:<8} {:<12} Loop", "Sample", "Root", "Root from", "Keys", "Velocity", "Group");
    for (z, n) in &plan.zones {
        let (root, _, how) = &plan.roots[*n];
        let _ = writeln!(
            out,
            "{:<40} {:<5} {:<36} {:<8} {:<8} {:<12} {}",
            relative[*n].rsplit('/').next().unwrap_or_default(),
            note_text(*root),
            how,
            format!("{}-{}", note_text(i32::from(z.low_key)), note_text(i32::from(z.high_key))),
            format!("{}-{}", z.low_velocity, z.high_velocity),
            plan.groups[z.group].name,
            z.loop_range.as_ref().map_or("-".into(), |l| format!("{}-{}", l.start, l.end)),
        );
    }
    out.push_str(if plan.issues.is_empty() { "\nNothing ambiguous.\n" } else { "\nCheck:\n" });
    for issue in &plan.issues {
        let _ = writeln!(out, "- {issue}");
    }
    out
}

/// Build the libraries `options` asks for; `progress` hears each step.
pub fn create(options: &Options, progress: &dyn Fn(&str)) -> Result<Created> {
    ensure!(options.source.is_dir(), "{} is not a folder", options.source.display());
    ensure!(options.kontra || options.kontakt, "Nothing to create: ask for KONTRA, Kontakt or both");
    let library = if options.name.trim().is_empty() {
        options.source.canonicalize()?.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Library".into())
    } else {
        options.name.trim().to_string()
    };
    let mut created = Created::default();
    progress(&format!("Scanning {}", options.source.display()));
    let sources = scan(&options.source, &mut created.skipped)?;
    ensure!(!sources.is_empty(), "No WAV or AIFF samples in {}", options.source.display());

    // Instruments by the words left in their paths.
    let mut by_name: BTreeMap<String, (String, Vec<Source>)> = BTreeMap::new();
    for s in sources {
        let name = if s.parsed.rest.is_empty() { library.clone() } else { s.parsed.rest.join(" ") };
        by_name.entry(name.to_lowercase()).or_insert_with(|| (name, Vec::new())).1.push(s);
    }

    let kontra = options.kontra.then(|| options.out.join(file_safe(&library)));
    let kontakt = options.kontakt.then(|| options.out.join(format!("{} (Kontakt)", file_safe(&library))));
    for (_, (name, sources)) in by_name {
        progress(&format!("Mapping {name} ({} samples)", sources.len()));
        let plan = plan(&name, &sources, progress);
        let folder = file_safe(&name);
        // Sample files keep their names unless two share one.
        let mut taken = BTreeSet::new();
        let relative: Vec<String> = sources
            .iter()
            .map(|s| {
                let mut file = file_safe(&s.path.file_name().unwrap_or_default().to_string_lossy());
                if !taken.insert(file.to_lowercase()) {
                    let rel = s.path.strip_prefix(&options.source).unwrap_or(&s.path);
                    file = file_safe(&rel.to_string_lossy().replace(['/', '\\'], "_"));
                    taken.insert(file.to_lowercase());
                }
                format!("../Samples/{folder}/{file}")
            })
            .collect();
        let zones: Vec<Zone> = plan.zones.iter().map(|(z, n)| Zone { sample: relative[*n].clone().into(), ..z.clone() }).collect();
        let text = report(&plan, &sources, &relative, &options.source);
        for root in kontra.iter().chain(&kontakt) {
            progress(&format!("Copying {name}'s samples"));
            for (s, rel) in sources.iter().zip(&relative) {
                place(&s.path, &root.join("Instruments").join(rel))?;
            }
            std::fs::create_dir_all(root.join("Reports"))?;
            std::fs::write(root.join("Reports").join(format!("{folder}.txt")), &text)?;
        }
        if let Some(root) = &kontra {
            let native = Native {
                format: "kontra-instrument".into(),
                version: 1,
                name: name.clone(),
                groups: plan.groups.clone(),
                zones: zones.clone(),
                scripts: plan.script.iter().cloned().collect(),
            };
            std::fs::write(root.join("Instruments").join(format!("{folder}.{NATIVE}")), serde_json::to_string_pretty(&native)?)?;
        }
        if let Some(root) = &kontakt {
            let files: Vec<nki::SampleFile> = sources
                .iter()
                .zip(&relative)
                .map(|(s, rel)| nki::SampleFile {
                    relative: rel.clone(),
                    rate: s.rate,
                    channels: s.channels,
                    bits: s.bits,
                    frames: s.frames,
                    modified: std::fs::metadata(&s.path)
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map_or(0, |d| d.as_secs()),
                })
                .collect();
            let preset = root.join("Instruments").join(format!("{folder}.nki"));
            nki::write(
                &preset,
                &nki::Program { name: &name, author: &options.vendor, groups: &plan.groups, zones: &zones, samples: &files, script: plan.script.as_deref() },
            )?;
            std::fs::write(root.join("Instruments").join(format!("{folder}.sfz")), sfz(&plan, &relative))?;
        }
        created.instruments.push(Summary {
            name,
            samples: sources.len(),
            groups: plan.groups.len(),
            layers: plan.layers,
            zones: plan.zones.len(),
            issues: plan.issues,
        });
    }
    if let Some(root) = &kontra {
        let manifest = Manifest { format: "kontra-library".into(), version: 1, name: library.clone(), vendor: options.vendor.clone() };
        std::fs::write(root.join(MANIFEST), serde_json::to_string_pretty(&manifest)?)?;
    }
    (created.kontra, created.kontakt) = (kontra, kontakt);
    Ok(created)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 16-bit sine WAV, optionally with a `smpl` loop.
    fn tone(path: &Path, hz: f32, channels: u16, loop_range: Option<(u32, u32)>) {
        let rate = 44100;
        let spec = hound::WavSpec { channels, sample_rate: rate, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for n in 0..rate * 3 / 10 {
            let v = (0.4 * (std::f32::consts::TAU * hz * n as f32 / rate as f32).sin() * 32767.0) as i16;
            for _ in 0..channels {
                w.write_sample(v).unwrap();
            }
        }
        w.finalize().unwrap();
        if let Some((start, end)) = loop_range {
            let mut bytes = std::fs::read(path).unwrap();
            let mut smpl = Vec::new();
            for v in [0, 0, 22675, 60, 0, 0, 0, 1, 0, 0, 0, start, end, 0, 0] {
                smpl.extend_from_slice(&u32::to_le_bytes(v));
            }
            bytes.extend_from_slice(b"smpl");
            bytes.extend_from_slice(&(smpl.len() as u32).to_le_bytes());
            bytes.extend_from_slice(&smpl);
            let riff = (bytes.len() - 8) as u32;
            bytes[4..8].copy_from_slice(&riff.to_le_bytes());
            std::fs::write(path, bytes).unwrap();
        }
    }

    #[test]
    fn keys_split_halfway() {
        assert_eq!(split_keys(&[60]), [(0, 127)]);
        assert_eq!(split_keys(&[48, 60, 67]), [(0, 54), (55, 63), (64, 127)]);
        assert_eq!(split_keys(&[60, 61]), [(0, 60), (61, 127)]);
        assert_eq!(split_keys(&[0, 127]), [(0, 63), (64, 127)]);
        use Velocity::*;
        assert_eq!(split_velocities(&[None]), [(0, 127)]);
        assert_eq!(split_velocities(&[Some(Ordinal(1)), Some(Ordinal(2)), Some(Ordinal(3))]), [(0, 41), (42, 84), (85, 127)]);
        assert_eq!(split_velocities(&[Some(Upper(40)), Some(Upper(90)), Some(Upper(127))]), [(0, 40), (41, 90), (91, 127)]);
        assert_eq!(note_text(60), "C3");
        assert_eq!(note_text(0), "C-2");
    }

    /// Synthetic samples in, both libraries out; each preset read back by
    /// KONTRA's own importers must give the zones that were planned.
    #[test]
    fn creates_libraries_that_read_back() {
        let root = std::env::temp_dir().join(format!("kontra-creator-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let source = root.join("Synth Set");
        std::fs::create_dir_all(source.join("Keys")).unwrap();
        std::fs::create_dir_all(source.join("Drone")).unwrap();
        for (note, hz) in [("C3", 261.63), ("G3", 392.0)] {
            for v in 1..=2 {
                for rr in 1..=2 {
                    let looped = (note == "G3" && v == 1 && rr == 1).then_some((1000, 5000));
                    tone(&source.join(format!("Keys/Keys_{note}_v{v}_rr{rr}.wav")), hz, 2, looped);
                }
            }
        }
        // No note in the name: A2 by ear, a little sharp.
        tone(&source.join("Drone/Drone.wav"), 221.0, 1, None);
        std::fs::write(source.join("Drone/Drone.flac"), b"not decoded").unwrap();

        let options = Options { source: source.clone(), name: String::new(), vendor: "Test".into(), out: root.join("out"), kontra: true, kontakt: true };
        let created = create(&options, &|_| {}).unwrap();
        assert_eq!(created.skipped, [source.join("Drone/Drone.flac")]);
        let names: Vec<_> = created.instruments.iter().map(|i| (i.name.as_str(), i.samples, i.groups, i.layers, i.zones)).collect();
        assert_eq!(names, [("Drone", 1, 1, 1, 1), ("Keys", 8, 2, 2, 8)]);
        let kontra = created.kontra.unwrap();
        let kontakt = created.kontakt.unwrap();
        assert_eq!(manifest(&kontra), Some(("Synth Set".into(), "Test".into())));

        for (instrument, expected) in [
            ("Keys", vec![
                // (group, keys, root, velocities, loop)
                (0, (0, 63), 60, (0, 63), None),
                (0, (64, 127), 67, (0, 63), Some((1000, 5001))),
                (0, (0, 63), 60, (64, 127), None),
                (0, (64, 127), 67, (64, 127), None),
                (1, (0, 63), 60, (0, 63), None),
                (1, (64, 127), 67, (0, 63), None),
                (1, (0, 63), 60, (64, 127), None),
                (1, (64, 127), 67, (64, 127), None),
            ]),
            ("Drone", vec![(0, (0, 127), 57, (0, 127), None)]),
        ] {
            let native = crate::import::read(&kontra.join(format!("Instruments/{instrument}.{NATIVE}"))).unwrap();
            let nki = crate::import::read(&kontakt.join(format!("Instruments/{instrument}.nki"))).unwrap();
            assert!(nki.missing_samples.is_empty() && native.missing_samples.is_empty());
            assert_eq!(nki.name, instrument);
            let got: Vec<_> = native
                .zones
                .iter()
                .map(|z| (z.group, (z.low_key, z.high_key), z.root, (z.low_velocity, z.high_velocity), z.loop_range.as_ref().map(|l| (l.start, l.end))))
                .collect();
            assert_eq!(got, expected, "{instrument}");
            // The NKI reads back zone for zone as the KONTRA preset, sample paths aside.
            assert_eq!(nki.zones.len(), native.zones.len());
            for (a, b) in nki.zones.iter().zip(&native.zones) {
                assert_eq!(a.sample.file_name(), b.sample.file_name());
                assert!(a.available && b.available);
                assert_eq!(Zone { sample: b.sample.clone(), ..a.clone() }, *b);
            }
            let group_names = |i: &Instrument| i.groups.iter().map(|g| g.name.clone()).collect::<Vec<_>>();
            assert_eq!(group_names(&nki), group_names(&native));
            assert_eq!(nki.groups[0].volume_env.as_ref().map(|e| e.release_ms), Some(250.0));
            assert_eq!(nki.scripts, native.scripts);
            for script in &native.scripts {
                crate::ksp::initialize(script, native.groups.len(), 1).unwrap();
            }
        }
        // The drone is tuned to play A2 in tune: 221 Hz is ~7.85 cents sharp.
        let drone = crate::import::read(&kontra.join(format!("Instruments/Drone.{NATIVE}"))).unwrap();
        let cents = 1200.0 * drone.zones[0].tune.log2();
        assert!((cents + 7.85).abs() < 1.0, "{cents}");
        let keys = crate::import::read(&kontra.join(format!("Instruments/Keys.{NATIVE}"))).unwrap();
        assert_eq!(keys.scripts.len(), 1, "round robins take turns by script");
        let sfz = std::fs::read_to_string(kontakt.join("Instruments/Keys.sfz")).unwrap();
        assert_eq!(sfz.matches("<region>").count(), 8);
        assert!(sfz.contains("seq_length=2 seq_position=2"));
        let report = std::fs::read_to_string(kontra.join("Reports/Drone.txt")).unwrap();
        assert!(report.contains("pitch") && report.contains("A2"), "{report}");

        // Both presets play: each key sounds at its own pitch, from the
        // nearest root transposed, in KONTRA's engine.
        let heard = |preset: &Path, note: u8, velocity: u8| {
            let instrument = crate::import::read(preset).unwrap();
            let mut e = crate::engine::Engine::default();
            e.blocking_streams = true;
            e.reset(44100.0);
            e.set_bank(Some(Box::new(crate::engine::Bank::load(&instrument).unwrap())));
            e.note_on(0, note, velocity);
            let (mut left, mut right) = (vec![0.0; 128], vec![0.0; 128]);
            let mut mono = Vec::new();
            for _ in 0..80 {
                e.render(&mut left, &mut right);
                mono.extend(left.iter().zip(&right).map(|(l, r)| 0.5 * (l + r)));
            }
            pitch::detect(&mono, 44100).map(|(midi, _)| midi)
        };
        let keys_nki = kontakt.join("Instruments/Keys.nki");
        let keys_native = kontra.join(format!("Instruments/Keys.{NATIVE}"));
        for (note, velocity) in [(60, 30), (62, 100), (67, 30), (70, 127)] {
            for preset in [&keys_nki, &keys_native] {
                let midi = heard(preset, note, velocity).unwrap_or_else(|| panic!("{} note {note} is silent", preset.display()));
                assert!((midi - f32::from(note)).abs() < 0.15, "{} note {note} sounds as {midi}", preset.display());
            }
        }
        let drone = heard(&kontakt.join("Instruments/Drone.nki"), 57, 100).unwrap();
        assert!((drone - 57.0).abs() < 0.05, "the drone's root plays in tune: {drone}");
        std::fs::remove_dir_all(&root).unwrap();
    }
}
