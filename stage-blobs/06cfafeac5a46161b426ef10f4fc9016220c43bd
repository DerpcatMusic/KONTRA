//! Port from v1 0cb7a8a0:src/creator.rs; new Kontakt/SFZ export uses v2 IR.
//! No legacy KONTRA preset or manifest reader/writer.
pub mod names;
mod nki;
pub mod pitch;

use sampler_ir::{self as ir, Group, Zone};
use anyhow::{Context, Result, bail, ensure};
use names::{Parsed, Velocity};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// What to create.
pub struct Options {
    pub source: PathBuf,
    /// The library's name; the source folder's when empty.
    pub name: String,
    pub vendor: String,
    pub out: PathBuf,
}

/// What was created.
#[derive(Debug, Default)]
pub struct Created {
    pub library: PathBuf,
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
            ensure!((1..=2).contains(&channels) && bits > 0 && rate > 0, "WAV format is invalid or not mono/stereo");
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
                    ensure!((1..=2).contains(&channels) && bits > 0 && rate > 0, "AIFF format is invalid or not mono/stereo");
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
    use symphonia::core::{audio::SampleBuffer, codecs::DecoderOptions, formats::FormatOptions,
        io::MediaSourceStream, meta::MetadataOptions, probe::Hint, errors::Error};
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|s| s.to_str()) { hint.with_extension(extension); }
    let stream = MediaSourceStream::new(Box::new(std::fs::File::open(path)?), Default::default());
    let mut format = symphonia::default::get_probe().format(&hint, stream, &FormatOptions::default(), &MetadataOptions::default())?.format;
    let track = format.default_track().context("No audio track")?;
    let mut decoder = symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default())?;
    let track = track.id;
    let wanted = frames.min(u64::from(rate) * 3 / 2) as usize;
    let mut buf = Vec::with_capacity(wanted);
    while buf.len() < wanted {
        let packet = match format.next_packet() {
            Ok(p) if p.track_id() == track => p,
            Ok(_) => continue,
            Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.into()),
        };
        let decoded = decoder.decode(&packet)?;
        let spec = *decoded.spec();
        let channels = spec.channels.count();
        ensure!((1..=2).contains(&channels), "Only mono/stereo samples are supported");
        let mut pcm = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        pcm.copy_interleaved_ref(decoded);
        buf.extend(pcm.samples().chunks_exact(channels).take(wanted - buf.len()).map(|s| 0.5 * (s[0] + s[channels - 1])));
    }
    Ok(buf)
}

fn root(z: &Zone) -> u8 { match z.pitch { ir::KeyTracking::Tracked { root } | ir::KeyTracking::Scaled { root, .. } => root, _ => 60 } }
fn ratio(z: &Zone) -> f64 { 2f64.powf(z.tune.semitones() / 12.) }
fn loop_range(z: &Zone) -> Option<(ir::LoopRange, bool)> {
    match z.playback.looping { ir::Looping::Continuous(l) => Some((l, false)), ir::Looping::UntilRelease(l) => Some((l, true)), _ => None }
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
    /// Zones reference source indexes through v2 asset references.
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
                .map(|(start, end)| ir::LoopRange { start, end, alternating: false, crossfade: ir::Span::ZERO });
            zones.push((
                Zone { group: Some(ir::GroupRef(group)), keys: ir::KeyRange { low: low_key, high: high_key },
                    pitch: ir::KeyTracking::Tracked { root }, velocities: ir::VelocityRange { low: low_velocity, high: high_velocity },
                    tune: ir::Pitch::Ratio(roots[n].1), playback: ir::Playback { looping: loop_range.map_or(ir::Looping::None, ir::Looping::Continuous), ..Default::default() }, ..Zone::new(ir::AssetRef(n)) },
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

/// Port v1 copy/link, refusing collisions instead of replacing an output.
fn place(from: &Path, to: &Path) -> Result<()> {
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if std::fs::hard_link(from, to).is_err() {
        let mut target = std::fs::OpenOptions::new().write(true).create_new(true).open(to)?;
        std::io::copy(&mut std::fs::File::open(from)?, &mut target).with_context(|| format!("Copying {}", from.display()))?;
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
        for (z, n) in plan.zones.iter().filter(|(z, _)| z.group == Some(ir::GroupRef(g))) {
            let _ = write!(
                out,
                "<region> sample={} lokey={} hikey={} pitch_keycenter={} lovel={} hivel={}",
                relative[*n], z.keys.low, z.keys.high, root(z), z.velocities.low.max(1), z.velocities.high
            );
            if (ratio(z) - 1.0).abs() > 1e-6 {
                let _ = write!(out, " tune={:.0}", 1200.0 * ratio(z).log2());
            }
            if let Some((l, _)) = loop_range(z) {
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
            format!("{}-{}", note_text(i32::from(z.keys.low)), note_text(i32::from(z.keys.high))),
            format!("{}-{}", z.velocities.low, z.velocities.high),
            plan.groups[z.group.unwrap().0].name,
            loop_range(z).map_or("-".into(), |(l, _)| format!("{}-{}", l.start, l.end)),
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

    let target = options.out.join(format!("{} (Kontakt)", file_safe(&library)));
    ensure!(!target.exists(), "Output already exists: {}", target.display());
    std::fs::create_dir_all(&options.out)?;
    let staging = tempfile::Builder::new().prefix(".kontra-creator-").tempdir_in(&options.out)?;
    let kontakt = staging.path().join("library");
    let mut preset_names = BTreeSet::new();
    for (_, (name, sources)) in by_name {
        progress(&format!("Mapping {name} ({} samples)", sources.len()));
        let plan = plan(&name, &sources, progress);
        let folder = file_safe(&name);
        ensure!(preset_names.insert(folder.to_lowercase()), "Instrument names share the output name {folder}");
        // Sample files keep their names unless two share one.
        let mut taken = BTreeSet::new();
        let relative: Vec<String> = sources
            .iter()
            .map(|s| {
                let mut file = file_safe(&s.path.file_name().unwrap_or_default().to_string_lossy());
                if !taken.insert(file.to_lowercase()) {
                    let rel = s.path.strip_prefix(&options.source).unwrap_or(&s.path);
                    file = file_safe(&rel.to_string_lossy().replace(['/', '\\'], "_"));
                    ensure!(taken.insert(file.to_lowercase()), "Sample names share the output name {file}");
                }
                Ok(format!("../Samples/{folder}/{file}"))
            })
            .collect::<Result<_>>()?;
        let zones: Vec<Zone> = plan.zones.iter().map(|(z, _)| z.clone()).collect();
        let text = report(&plan, &sources, &relative, &options.source);
        for root in [&kontakt] {
            progress(&format!("Copying {name}'s samples"));
            for (s, rel) in sources.iter().zip(&relative) {
                place(&s.path, &root.join("Instruments").join(rel))?;
            }
            std::fs::create_dir_all(root.join("Reports"))?;
            std::fs::write(root.join("Reports").join(format!("{folder}.txt")), &text)?;
        }
        {
            let root = &kontakt;
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
    std::fs::rename(&kontakt, &target)?;
    created.library = target;
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

    /// Port v1's creator round-trip/render witness through v2's readers and core.
    #[test]
    fn creates_native_kontakt_and_sfz_that_read_back_and_play() {
        use crate::sound::{Core, CoreLoader, LoadRequest, BlockInfo, event::{Event, HostNote}};
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("Synth Set");
        std::fs::create_dir_all(source.join("Keys")).unwrap();
        std::fs::create_dir_all(source.join("Drone")).unwrap();
        for (note, hz) in [("C3", 261.63), ("G3", 392.0)] {
            for v in 1..=2 { for rr in 1..=2 {
                tone(&source.join(format!("Keys/Keys_{note}_v{v}_rr{rr}.wav")), hz, 2,
                    (note == "G3" && v == 1 && rr == 1).then_some((1000, 5000)));
            }}
        }
        tone(&source.join("Drone/Drone.wav"), 221.0, 1, None);
        std::fs::write(source.join("Drone/Drone.flac"), b"not decoded").unwrap();
        let options = Options { source: source.clone(), name: String::new(), vendor: "Test".into(), out: root.path().join("out") };
        let created = create(&options, &|_| {}).unwrap();
        assert_eq!(created.skipped, [source.join("Drone/Drone.flac")]);
        let names: Vec<_> = created.instruments.iter().map(|i| (i.name.as_str(), i.samples, i.groups, i.layers, i.zones)).collect();
        assert_eq!(names, [("Drone", 1, 1, 1, 1), ("Keys", 8, 2, 2, 8)]);
        let kontakt = created.library;
        let keys_nki = kontakt.join("Instruments/Keys.nki");
        let keys = sampler_kontakt::read(&keys_nki).unwrap();
        assert_eq!(keys.instrument.name, "Keys");
        assert_eq!(keys.instrument.zones.len(), 8);
        assert_eq!(keys.instrument.groups.len(), 2);
        let slots: Vec<_> = keys.instrument.zones.iter().filter_map(|z| match z.playback.looping {
            ir::Looping::Slots(slots) if slots.iter().any(Option::is_some) => Some(slots),
            _ => None,
        }).collect();
        assert_eq!(slots.len(), 1, "exactly one created zone has an authored loop");
        assert!(slots[0][1..].iter().all(Option::is_none), "creator keeps physical loop holes");
        let authored = slots[0][0].expect("creator writes physical loop slot zero");
        assert!(!authored.until_release);
        let looped = authored.range;
        assert_eq!((looped.start, looped.end), (1000, 5001));
        let sfz = std::fs::read_to_string(kontakt.join("Instruments/Keys.sfz")).unwrap();
        assert_eq!(sfz.matches("<region>").count(), 8);
        assert!(sfz.contains("seq_length=2 seq_position=2"));
        assert!(std::fs::read_to_string(kontakt.join("Reports/Drone.txt")).unwrap().contains("A2"));
        assert!(!kontakt.join("kontra-library.json").exists());
        assert!(create(&options, &|_| {}).unwrap_err().to_string().contains("Output already exists"));
        let heard = |preset: &Path, note: u8, velocity: u8| {
            let request = LoadRequest { path: preset.into(), sample_rate: 44100., ..Default::default() };
            let loaded = crate::sound::v2::V2Loader.prepare(&request, &mut |_| {}, &|| false).unwrap();
            let mut core = crate::sound::v2::V2Core::with_parts(1, 44100.);
            core.install(0, loaded.part);
            core.begin_block(&BlockInfo { frames: 128, offline: true, ..Default::default() });
            core.event(0, Event::NoteOn { note: HostNote { port: 0, channel: 0, key: note, id: 1, clap: true }, velocity: f64::from(velocity) / 127., tune: 0. });
            let mut mono = Vec::new();
            for _ in 0..80 {
                let audio = core.render(128);
                mono.extend((0..128).map(|n| 0.5 * (audio.buses[0][0][n] + audio.buses[0][1][n])));
            }
            pitch::detect(&mono, 44100).unwrap().0
        };
        for (note, velocity) in [(60, 30), (62, 100), (67, 30), (70, 127)] {
            let midi = heard(&keys_nki, note, velocity);
            assert!((midi - f32::from(note)).abs() < 0.15, "note {note} sounds as {midi}");
        }
        let drone = heard(&kontakt.join("Instruments/Drone.nki"), 57, 100);
        assert!((drone - 57.).abs() < 0.05, "the drone's root plays in tune: {drone}");
    }

    #[test]
    fn v1_aiff_creator_exports_a_playable_native_sample() {
        use crate::sound::{CoreLoader, LoadRequest};
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("AIFF"); std::fs::create_dir(&source).unwrap();
        let frames = 4410u32;
        let mut comm = Vec::new();
        comm.extend(1u16.to_be_bytes()); comm.extend(frames.to_be_bytes()); comm.extend(16u16.to_be_bytes());
        comm.extend([0x40, 0x0e, 0xac, 0x44, 0, 0, 0, 0, 0, 0]); // 44100 in IEEE 80-bit
        let mut sound = vec![0; 8];
        for n in 0..frames { sound.extend(((0.4 * (std::f32::consts::TAU * 261.63 * n as f32 / 44100.).sin() * 32767.) as i16).to_be_bytes()); }
        let mut bytes = b"FORM".to_vec(); bytes.extend((4u32 + 8 + comm.len() as u32 + 8 + sound.len() as u32).to_be_bytes()); bytes.extend(b"AIFF");
        bytes.extend(b"COMM"); bytes.extend((comm.len() as u32).to_be_bytes()); bytes.extend(comm);
        bytes.extend(b"SSND"); bytes.extend((sound.len() as u32).to_be_bytes()); bytes.extend(sound);
        let sample = source.join("Piano_C3.aiff"); std::fs::write(&sample, &bytes).unwrap();
        let created = create(&Options { source, name: "AIFF".into(), vendor: String::new(), out: root.path().join("out") }, &|_| {}).unwrap();
        assert_eq!(created.instruments[0].samples, 1);
        let request = LoadRequest { path: created.library.join("Instruments/Piano.nki"), sample_rate: 44100., ..Default::default() };
        let loaded = crate::sound::v2::V2Loader.prepare(&request, &mut |_| {}, &|| false).unwrap();
        assert!(loaded.part.is_some());
        assert_eq!(std::fs::read(created.library.join("Samples/Piano/Piano_C3.aiff")).unwrap(), bytes);
        let full = sampler_kontakt::decode(&bytes).unwrap();
        let asset = sampler_kontakt::Samples::new(root.path()).source(&sample).unwrap();
        let mut reader = sampler_kontakt::SampleReader::open(&asset).unwrap();
        assert_eq!(reader.frames(), frames as usize);
        for start in [1000, 0, 4000, 127] {
            let mut out = [[0.; 2]; 32]; reader.read(start, &mut out).unwrap();
            assert_eq!(&out, &full.frames[start..start + 32], "AIFF random access {start}");
        }
    }

}

pub fn create_library(args: &[String]) -> Result<()> {
    ensure!(!args.iter().any(|a| a == "--kontra-only"), "Legacy KONTRA file export is not supported; this creator writes Kontakt NKI and SFZ");
    for (n, arg) in args.iter().enumerate().filter(|(_, a)| a.starts_with("--")) {
        ensure!(matches!(arg.as_str(), "--name" | "--vendor" | "--out" | "--kontakt-only"), "Unknown creator option: {arg}");
        if arg != "--kontakt-only" { ensure!(args.get(n + 1).is_some_and(|v| !v.starts_with("--")), "{arg} needs a value"); }
    }
    let value = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let source = args
        .iter()
        .enumerate()
        .find(|(i, a)| !a.starts_with("--") && (*i == 0 || !matches!(args[i - 1].as_str(), "--name" | "--vendor" | "--out")))
        .map(|(_, a)| a)
        .context("create-library requires a folder of samples")?;
    let options = Options {
        source: source.into(),
        name: value("--name").unwrap_or_default(),
        vendor: value("--vendor").unwrap_or_default(),
        out: value("--out").map_or_else(|| std::path::PathBuf::from("."), Into::into),
    };
    let created = create(&options, &|step| eprintln!("{step}"))?;
    for i in &created.instruments {
        println!("{}: {} samples, {} groups, {} velocity layers, {} zones", i.name, i.samples, i.groups, i.layers, i.zones);
        for issue in &i.issues {
            println!("  check: {issue}");
        }
    }
    for path in &created.skipped {
        println!("skipped (not WAV/AIFF): {}", path.display());
    }
    println!("wrote {}", created.library.display());
    Ok(())
}
