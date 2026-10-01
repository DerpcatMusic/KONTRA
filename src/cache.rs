//! On-disk cache of imported instruments, so a reload skips NKI parsing,
//! sample resolution and impulse decoding.
//!
//! An entry is keyed by the preset's canonical path and program, and stamped
//! with its size, modification time and a hash of the importer's sources
//! (`build.rs`): anything else, a corrupt file included, reads as a miss and
//! the preset is parsed again. Only instruments with every sample found are
//! stored, so a library completed later is never masked. Entries are written
//! to a temporary file and renamed into place.
//!
//! Layout: the stamp, then a JSON head (everything but zones and impulses),
//! then zones and impulse audio in little-endian binary; zones are the bulk
//! (93k in Areia) and would take far longer to parse as JSON.

use crate::audio::{Header, Sample, Source, Sources};
use crate::fx::{Params, ProgramFx, params::Impulse};
use crate::import::{Group, Instrument, Loop, VoiceLimit, Zone};
use crate::ksp::Persisted;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Everything but zones and impulses.
#[derive(Serialize, Deserialize)]
struct Head<'a> {
    name: Cow<'a, str>,
    groups: Cow<'a, [Group]>,
    warnings: Cow<'a, [String]>,
    scripts: Cow<'a, [String]>,
    script_state: Cow<'a, [Persisted]>,
    fx: Cow<'a, ProgramFx>,
    voice_limit: Option<VoiceLimit>,
    voice_groups: Cow<'a, [Option<VoiceLimit>]>,
    kontakt_sample_bytes: f64,
    kontakt_preload: i32,
}

/// `~/.cache/kontra` (or the platform's cache folder).
pub fn dir() -> Option<PathBuf> {
    Some(dirs::cache_dir()?.join("kontra"))
}

/// Program `program` of the canonical preset `path`, if cached and current.
pub fn load(dir: &Path, path: &Path, program: u32) -> Option<Instrument> {
    let stamp = stamp(path, program)?;
    let bytes = std::fs::read(entry(dir, path, program)).ok()?;
    decode(bytes.strip_prefix(&stamp[..])?, path)
}

/// Store `instrument` (program `program` of the canonical preset `path`),
/// unless a sample is missing. Failures only cost the next load a parse.
pub fn store(dir: &Path, path: &Path, program: u32, instrument: &Instrument) {
    if !instrument.missing_samples.is_empty() {
        return;
    }
    let Some(mut bytes) = stamp(path, program) else {
        return;
    };
    if encode(instrument, &mut bytes).is_err() {
        return;
    }
    let entry = entry(dir, path, program);
    let tmp = entry.with_extension(format!("tmp{}", std::process::id()));
    let written = std::fs::create_dir_all(dir)
        .and_then(|()| std::fs::write(&tmp, &bytes))
        .and_then(|()| std::fs::rename(&tmp, &entry));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

// ponytail: entries are never evicted; one per preset and program opened
// (Areia ≈ 8 MiB). Add an LRU sweep if the folder grows a problem.
fn entry(dir: &Path, path: &Path, program: u32) -> PathBuf {
    let mut h = std::hash::DefaultHasher::new();
    (path.as_os_str().as_encoded_bytes(), program).hash(&mut h);
    dir.join(format!("{:016x}.instrument", h.finish()))
}

/// What an entry must start with: importer version, program, and the
/// preset's path, size and modification time.
fn stamp(path: &Path, program: u32) -> Option<Vec<u8>> {
    let meta = path.metadata().ok()?;
    let mtime = meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?;
    let mut out = b"KONTRA instrument ".to_vec();
    out.extend_from_slice(env!("KONTRA_IMPORT_HASH").as_bytes());
    out.extend_from_slice(&program.to_le_bytes());
    out.extend_from_slice(&meta.len().to_le_bytes());
    out.extend_from_slice(&mtime.as_nanos().to_le_bytes());
    let path = path.as_os_str().as_encoded_bytes();
    out.extend_from_slice(&(path.len() as u64).to_le_bytes());
    out.extend_from_slice(path);
    Some(out)
}

fn encode(i: &Instrument, out: &mut Vec<u8>) -> serde_json::Result<()> {
    let head = serde_json::to_vec(&Head {
        name: Cow::Borrowed(&i.name),
        groups: Cow::Borrowed(&i.groups),
        warnings: Cow::Borrowed(&i.warnings),
        scripts: Cow::Borrowed(&i.scripts),
        script_state: Cow::Borrowed(&i.script_state),
        fx: Cow::Borrowed(&i.fx),
        voice_limit: i.voice_limit,
        voice_groups: Cow::Borrowed(&i.voice_groups),
        kontakt_sample_bytes: i.kontakt_sample_bytes,
        kontakt_preload: i.kontakt_preload,
    })?;
    let mut w = Writer(out);
    w.bytes(&head);

    // Sample paths once each; zones refer to them by index.
    let mut ids = std::collections::HashMap::new();
    let mut paths = Vec::new();
    let zone_paths: Vec<u32> = (i.zones.iter())
        .map(|z| {
            *ids.entry(z.sample.as_os_str()).or_insert_with(|| {
                paths.push(z.sample.as_os_str().as_encoded_bytes());
                paths.len() as u32 - 1
            })
        })
        .collect();
    w.u64(paths.len() as u64);
    for path in paths {
        w.bytes(path);
    }
    w.u64(i.zones.len() as u64);
    for (z, &path) in i.zones.iter().zip(&zone_paths) {
        w.u64(z.group as u64);
        w.u32(path);
        w.0.extend_from_slice(&[
            z.available as u8,
            z.low_key,
            z.high_key,
            z.root,
            z.low_velocity,
            z.high_velocity,
            z.fade_low_velocity,
            z.fade_high_velocity,
            z.fade_low_key,
            z.fade_high_key,
        ]);
        w.u64(z.start as u64);
        w.u32(z.end as u32);
        w.u64(z.start_mod.map_or(u64::MAX, u64::from));
        w.u32(z.gain.to_bits());
        w.u32(z.pan.to_bits());
        w.u64(z.tune.to_bits());
        match &z.loop_range {
            None => w.0.push(0),
            Some(l) => {
                w.0.push(1 + l.until_release as u8);
                w.u64(l.start as u64);
                w.u64(l.end as u64);
                w.u64(l.crossfade as u64);
            }
        }
    }

    // Impulses once each; convolution slots refer to them by index + 1.
    let mut distinct: Vec<&Arc<Sample>> = Vec::new();
    let slots: Vec<u32> = (i.fx.effects())
        .filter_map(|(_, fx)| match &fx.params {
            Params::Convolution(c) => Some(c.ir.as_ref().map_or(0, |Impulse(ir)| {
                let at = distinct.iter().position(|d| Arc::ptr_eq(d, ir));
                1 + at.unwrap_or_else(|| {
                    distinct.push(ir);
                    distinct.len() - 1
                }) as u32
            })),
            _ => None,
        })
        .collect();
    w.u64(distinct.len() as u64);
    for ir in distinct {
        w.u32(ir.rate);
        w.u64(ir.frames.len() as u64);
        for f in ir.frames.iter().flatten() {
            w.u32(f.to_bits());
        }
    }
    w.u64(slots.len() as u64);
    for slot in slots {
        w.u32(slot);
    }
    Ok(())
}

fn decode(bytes: &[u8], path: &Path) -> Option<Instrument> {
    let mut r = Reader(bytes);
    let head: Head = serde_json::from_slice(r.bytes()?).ok()?;

    let paths = (0..r.len(8)?)
        .map(|_| {
            // SAFETY: written from `as_encoded_bytes` on this platform.
            Some(PathBuf::from(unsafe { std::ffi::OsStr::from_encoded_bytes_unchecked(r.bytes()?) }))
        })
        .collect::<Option<Vec<_>>>()?;
    let zones = (0..r.len(59)?)
        .map(|_| {
            let group = r.u64()? as usize;
            let sample = paths.get(r.u32()? as usize)?.clone();
            let [available, low_key, high_key, root, low_velocity, high_velocity, fade_low_velocity, fade_high_velocity, fade_low_key, fade_high_key] =
                r.array()?;
            Some(Zone {
                group,
                sample,
                available: available != 0,
                low_key,
                high_key,
                root,
                low_velocity,
                high_velocity,
                fade_low_velocity,
                fade_high_velocity,
                fade_low_key,
                fade_high_key,
                start: r.u64()? as usize,
                end: r.u32()? as i32,
                start_mod: u32::try_from(r.u64()?).ok(),
                gain: f32::from_bits(r.u32()?),
                pan: f32::from_bits(r.u32()?),
                tune: f64::from_bits(r.u64()?),
                loop_range: match r.array::<1>()?[0] {
                    0 => None,
                    kind => Some(Loop {
                        until_release: kind == 2,
                        start: r.u64()? as usize,
                        end: r.u64()? as usize,
                        crossfade: r.u64()? as usize,
                    }),
                },
            })
        })
        .collect::<Option<Vec<_>>>()?;
    if zones.iter().any(|z| z.group >= head.groups.len()) {
        return None;
    }

    let impulses = (0..r.len(12)?)
        .map(|_| {
            let rate = r.u32()?;
            let frames = (0..r.len(8)?)
                .map(|_| Some([f32::from_bits(r.u32()?), f32::from_bits(r.u32()?)]))
                .collect::<Option<_>>()?;
            Some(Arc::new(Sample { rate, frames }))
        })
        .collect::<Option<Vec<_>>>()?;
    let mut fx = head.fx.into_owned();
    let mut slots = (fx.effects_mut()).filter_map(|fx| match &mut fx.params {
        Params::Convolution(c) => Some(&mut c.ir),
        _ => None,
    });
    for _ in 0..r.len(4)? {
        let ir = slots.next()?;
        *ir = match r.u32()? {
            0 => None,
            at => Some(Impulse(Arc::clone(impulses.get(at as usize - 1)?))),
        };
    }
    if slots.next().is_some() || !r.0.is_empty() {
        return None;
    }
    drop(slots);

    Some(Instrument {
        path: path.into(),
        name: head.name.into_owned(),
        groups: head.groups.into_owned(),
        zones,
        warnings: head.warnings.into_owned(),
        missing_samples: Vec::new(),
        scripts: head.scripts.into_owned(),
        fx,
        voice_limit: head.voice_limit,
        voice_groups: head.voice_groups.into_owned(),
        script_state: head.script_state.into_owned(),
        kontakt_sample_bytes: head.kontakt_sample_bytes,
        kontakt_preload: head.kontakt_preload,
    })
}

/// Each of `paths` (a preset's distinct samples, in order) with its source
/// and header as a load of `preset` stored them, if every file holding
/// them is unchanged: a load then opens no sample before it plays.
/// Entries are keyed by the preset and its sample paths, and stamped with
/// each holding file's size and modification time.
pub fn headers(dir: &Path, preset: &Path, paths: &[&Path]) -> Option<Vec<(Source, Header)>> {
    let bytes = std::fs::read(headers_entry(dir, preset, paths)).ok()?;
    let mut r = Reader(bytes.strip_prefix(HEADERS)?);
    let files = (0..r.len(8)?)
        .map(|_| {
            // SAFETY: written from `as_encoded_bytes` on this platform.
            let file = PathBuf::from(unsafe { std::ffi::OsStr::from_encoded_bytes_unchecked(r.bytes()?) });
            let stamp = r.array::<24>()?;
            (file_stamp(&file)? == stamp).then_some(file)
        })
        .collect::<Option<Vec<_>>>()?;
    if r.len(37)? != paths.len() {
        return None;
    }
    let mut sources = Sources::default();
    let headers = (paths.iter())
        .map(|&path| {
            let file = files.get(r.u32()? as usize)?.clone();
            let (offset, len) = (r.u64()?, r.u64()?);
            let len = (len != u64::MAX).then_some(len);
            let (rate, frames, bits) = (r.u32()?, r.u64()?, r.u32()?);
            let [keyed] = r.array()?;
            let header = Header {
                rate,
                frames,
                bits: u16::try_from(bits).ok(),
            };
            Some((sources.rebuild(path, file, offset, len, keyed != 0).ok()?, header))
        })
        .collect::<Option<Vec<_>>>()?;
    r.0.is_empty().then_some(headers)
}

/// Store what [`headers`] reads back. Failures only cost the next load its opens.
pub fn store_headers(dir: &Path, preset: &Path, paths: &[&Path], headers: &[(&Source, Header)]) {
    let mut bytes = HEADERS.to_vec();
    let mut w = Writer(&mut bytes);
    let mut ids = std::collections::HashMap::new();
    let mut files = Vec::new();
    let records: Vec<_> = (headers.iter())
        .map(|(source, header)| {
            let (file, offset, len, keyed) = source.parts();
            let id = *ids.entry(file).or_insert_with(|| {
                files.push(file);
                files.len() as u32 - 1
            });
            (id, offset, len, keyed, header)
        })
        .collect();
    w.u64(files.len() as u64);
    for file in files {
        let Some(stamp) = file_stamp(file) else { return };
        w.bytes(file.as_os_str().as_encoded_bytes());
        w.0.extend_from_slice(&stamp);
    }
    w.u64(records.len() as u64);
    for (id, offset, len, keyed, header) in records {
        w.u32(id);
        w.u64(offset);
        w.u64(len.unwrap_or(u64::MAX));
        w.u32(header.rate);
        w.u64(header.frames);
        w.u32(header.bits.map_or(u32::MAX, u32::from));
        w.0.push(keyed as u8);
    }
    let entry = headers_entry(dir, preset, paths);
    let tmp = entry.with_extension(format!("tmp{}", std::process::id()));
    let written = std::fs::create_dir_all(dir)
        .and_then(|()| std::fs::write(&tmp, &bytes))
        .and_then(|()| std::fs::rename(&tmp, &entry));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

const HEADERS: &[u8] = b"KONTRA headers 1\n";

// ponytail: never evicted, like instrument entries; ≈37 bytes a sample
// (860 KiB for ANALOG STRINGS).
fn headers_entry(dir: &Path, preset: &Path, paths: &[&Path]) -> PathBuf {
    let mut h = std::hash::DefaultHasher::new();
    preset.as_os_str().as_encoded_bytes().hash(&mut h);
    for path in paths {
        path.as_os_str().as_encoded_bytes().hash(&mut h);
    }
    dir.join(format!("{:016x}.headers", h.finish()))
}

/// A file's size and modification time (ns).
fn file_stamp(path: &Path) -> Option<[u8; 24]> {
    let meta = path.metadata().ok()?;
    let mtime = meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?;
    let mut out = [0; 24];
    out[..8].copy_from_slice(&meta.len().to_le_bytes());
    out[8..].copy_from_slice(&mtime.as_nanos().to_le_bytes());
    Some(out)
}

struct Writer<'a>(&'a mut Vec<u8>);

impl Writer<'_> {
    fn u32(&mut self, x: u32) {
        self.0.extend_from_slice(&x.to_le_bytes());
    }
    fn u64(&mut self, x: u64) {
        self.0.extend_from_slice(&x.to_le_bytes());
    }
    fn bytes(&mut self, x: &[u8]) {
        self.u64(x.len() as u64);
        self.0.extend_from_slice(x);
    }
}

/// Every read is `None` past the end.
struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let (head, rest) = self.0.split_at_checked(n)?;
        self.0 = rest;
        Some(head)
    }
    fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.take(N)?.try_into().ok()
    }
    fn u32(&mut self) -> Option<u32> {
        self.array().map(u32::from_le_bytes)
    }
    fn u64(&mut self) -> Option<u64> {
        self.array().map(u64::from_le_bytes)
    }
    fn bytes(&mut self) -> Option<&'a [u8]> {
        let n = usize::try_from(self.u64()?).ok()?;
        self.take(n)
    }
    /// A count of items at least `size` bytes each, checked against what is
    /// left so a corrupt count cannot reserve memory.
    fn len(&mut self, size: usize) -> Option<usize> {
        let n = usize::try_from(self.u64()?).ok()?;
        (n <= self.0.len() / size).then_some(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instruments_round_trip_and_stale_entries_miss() {
        let dir = std::env::temp_dir().join(format!("kontra-cache-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.nki");
        std::fs::write(&path, b"preset").unwrap();
        let ir = Arc::new(Sample { rate: 48_000, frames: vec![[0.5, -0.25]; 3] });
        let mut fx = ProgramFx::default();
        let band = crate::fx::params::IrBand { length_ratio: 1.0, low_cut_hz: 20.0, high_cut_hz: 20_000.0 };
        let conv = Params::Convolution(Box::new(crate::fx::params::Convolution {
            unknown: [0.0; 2],
            predelay_ms: 0.0,
            early: band,
            late: band,
            unknown_9: 0.0,
            flags: [false; 5],
            curve_x: vec![0.5],
            curve_db: vec![-6.0],
            ir_index: 0,
            ir_file: Some("ir.wav".into()),
            ir_error: None,
            ir: Some(Impulse(ir)),
        }));
        for _ in 0..2 {
            fx.insert.slots.push(crate::fx::Effect {
                slot: 0,
                kind: crate::fx::Kind::Convolution,
                version: 0,
                bypass: false,
                output_gain: 1.0,
                dry_level: 0.0,
                params: conv.clone(),
            });
        }
        let instrument = Instrument {
            path: path.clone(),
            name: "A".into(),
            groups: vec![Group::default()],
            zones: vec![
                Zone { sample: "/x/1.wav".into(), start_mod: Some(7), end: -3, tune: 1.5, ..Default::default() },
                Zone {
                    sample: "/x/1.wav".into(),
                    loop_range: Some(Loop { start: 1, end: 9, until_release: true, crossfade: 2 }),
                    ..Default::default()
                },
            ],
            scripts: vec!["on init\nend on".into()],
            script_state: vec![Persisted::from([("$x".into(), crate::ksp::Value::Int(3))])],
            fx,
            ..Default::default()
        };
        store(&dir, &path, 0, &instrument);
        let back = load(&dir, &path, 0).expect("a fresh entry hits");
        assert_eq!(format!("{back:?}"), format!("{instrument:?}"));
        let irs: Vec<_> = (back.fx.effects())
            .filter_map(|(_, fx)| match &fx.params {
                Params::Convolution(c) => c.ir.as_ref().map(|Impulse(ir)| Arc::clone(ir)),
                _ => None,
            })
            .collect();
        assert!(Arc::ptr_eq(&irs[0], &irs[1]), "a shared impulse stays shared");
        assert!(load(&dir, &path, 1).is_none(), "another program misses");

        // Truncated: a miss, not a panic.
        let entry = entry(&dir, &path, 0);
        let bytes = std::fs::read(&entry).unwrap();
        for cut in [bytes.len() - 1, bytes.len() / 2, 10] {
            std::fs::write(&entry, &bytes[..cut]).unwrap();
            assert!(load(&dir, &path, 0).is_none());
        }
        // The preset changed: stale.
        std::fs::write(&entry, &bytes).unwrap();
        std::fs::write(&path, b"preset, edited").unwrap();
        assert!(load(&dir, &path, 0).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn sample_headers_round_trip_and_changed_files_miss() {
        let dir = std::env::temp_dir().join(format!("kontra-headers-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (preset, sample) = (dir.join("a.nki"), dir.join("1.wav"));
        let spec = hound::WavSpec { channels: 1, sample_rate: 44_100, bits_per_sample: 24, sample_format: hound::SampleFormat::Int };
        let mut w = hound::WavWriter::create(&sample, spec).unwrap();
        (0..100).for_each(|i| w.write_sample(i).unwrap());
        w.finalize().unwrap();
        let source = Sources::default().source(&sample).unwrap();
        let header = source.open().unwrap().header();
        assert_eq!(header, Header { rate: 44_100, frames: 100, bits: Some(24) });
        let paths = [sample.as_path()];
        store_headers(&dir, &preset, &paths, &[(&source, header)]);
        let back = headers(&dir, &preset, &paths).expect("a fresh entry hits");
        assert_eq!((back[0].0.parts(), back[0].1), (source.parts(), header));
        assert!(headers(&dir, &dir.join("b.nki"), &paths).is_none(), "another preset misses");
        // The sample changed: stale.
        std::io::Write::write_all(&mut std::fs::OpenOptions::new().append(true).open(&sample).unwrap(), b"x").unwrap();
        assert!(headers(&dir, &preset, &paths).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
