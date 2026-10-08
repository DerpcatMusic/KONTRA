//! Versioned v2-owned persistent numeric sample metadata.
//! Never stores PCM, scripts, properties or access keys. Cache errors are misses.
use crate::samples::{Samples, Source};
use sampler_core::Pcm;
use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    io::{Cursor, Read},
    path::{Path, PathBuf},
    sync::Arc,
};
const MAGIC: &[u8] = b"KONTRA v2 numeric headers 2\n";
const BUDGET: u64 = 64 << 20;

fn u64le(reader: &mut impl Read) -> Option<u64> {
    let mut bytes = [0; 8];
    reader.read_exact(&mut bytes).ok()?;
    Some(u64::from_le_bytes(bytes))
}
fn u32le(reader: &mut impl Read) -> Option<u32> {
    let mut bytes = [0; 4];
    reader.read_exact(&mut bytes).ok()?;
    Some(u32::from_le_bytes(bytes))
}
fn entry(dir: &Path, preset: &Path, paths: &[&Path]) -> PathBuf {
    let mut hash = std::hash::DefaultHasher::new();
    preset.as_os_str().as_encoded_bytes().hash(&mut hash);
    for path in paths {
        path.as_os_str().as_encoded_bytes().hash(&mut hash);
    }
    dir.join(format!("{:016x}.headers", hash.finish()))
}
fn stamp(path: &Path) -> Option<[u8; 24]> {
    let meta = path.metadata().ok()?;
    let time = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    let mut stamp = [0; 24];
    stamp[..8].copy_from_slice(&meta.len().to_le_bytes());
    stamp[8..].copy_from_slice(&time.as_nanos().to_le_bytes());
    Some(stamp)
}
fn decode(bytes: &[u8], paths: &[&Path], samples: &mut Samples) -> Option<Vec<Source>> {
    let mut reader = Cursor::new(bytes.strip_prefix(MAGIC)?);
    let files = usize::try_from(u64le(&mut reader)?).ok()?;
    if files > paths.len() {
        return None;
    }
    let mut holding = Vec::with_capacity(files);
    for _ in 0..files {
        let length = usize::try_from(u64le(&mut reader)?).ok()?;
        if length > 32768 {
            return None;
        }
        let mut name = vec![0; length];
        reader.read_exact(&mut name).ok()?;
        // UTF-8 cache names cover native library paths. Other names miss safely.
        let path = PathBuf::from(String::from_utf8(name).ok()?);
        let mut version = [0; 24];
        reader.read_exact(&mut version).ok()?;
        if stamp(&path)? != version {
            return None;
        }
        holding.push((path, u64::from_le_bytes(version[..8].try_into().ok()?)));
    }
    if usize::try_from(u64le(&mut reader)?).ok()? != paths.len() {
        return None;
    }
    let mut sources = Vec::with_capacity(paths.len());
    for &path in paths {
        let (file, length) = holding.get(u32le(&mut reader)? as usize)?;
        let offset = u64le(&mut reader)?;
        let size = u64le(&mut reader)?;
        let size = if size == u64::MAX { *length } else { size };
        if offset.checked_add(size)? > *length {
            return None;
        }
        let rate = u32le(&mut reader)?;
        let frames = usize::try_from(u64le(&mut reader)?).ok()?;
        let frame_bytes = u32le(&mut reader)? as usize;
        if !matches!(frame_bytes, 4 | 6 | 8) {
            return None;
        }
        let mut keyed = [0];
        reader.read_exact(&mut keyed).ok()?;
        if rate == 0 || rate > 768000 || keyed[0] > 1 {
            return None;
        }
        sources.push(samples.cached_source(
            path,
            file,
            offset,
            size,
            keyed[0] == 1,
            (rate, frames),
            frame_bytes,
        )?);
    }
    (reader.position() as usize == reader.get_ref().len()).then_some(sources)
}
pub(crate) fn load(preset: &Path, paths: &[&Path], samples: &mut Samples) -> Option<Vec<Source>> {
    let dir = dirs::cache_dir()?.join("kontra/v2-headers");
    let file = entry(&dir, preset, paths);
    if file.metadata().ok().is_some_and(|m| m.len() <= BUDGET) {
        if let Ok(bytes) = std::fs::read(file) {
            return decode(&bytes, paths, samples);
        }
    }
    None
}
fn encode(sources: &[Arc<Source>], pcm: &[Pcm], frame_bytes: &[usize]) -> Option<Vec<u8>> {
    if sources.len() != pcm.len() {
        return None;
    }
    let mut ids = HashMap::new();
    let mut files = Vec::new();
    for source in sources {
        let next = files.len() as u32;
        ids.entry(&source.path).or_insert_with(|| {
            files.push(&source.path);
            next
        });
    }
    let weight = (MAGIC.len() as u64)
        .checked_add(16)?
        .checked_add((sources.len() as u64).checked_mul(37)?)?
        .checked_add(files.iter().try_fold(0u64, |sum, file| {
            let len = file.to_str()?.len();
            (len <= 32768).then_some(())?;
            sum.checked_add(len as u64 + 32)
        })?)?;
    if weight > BUDGET {
        return None;
    }
    let mut bytes = MAGIC.to_vec();
    bytes.extend((files.len() as u64).to_le_bytes());
    for file in files {
        let name = file.to_str()?.as_bytes();
        bytes.extend((name.len() as u64).to_le_bytes());
        bytes.extend(name);
        bytes.extend(stamp(file)?);
    }
    bytes.extend((sources.len() as u64).to_le_bytes());
    if sources.len() != frame_bytes.len() {
        return None;
    }
    for ((source, pcm), &width) in sources.iter().zip(pcm).zip(frame_bytes) {
        if !matches!(width, 4 | 6 | 8) {
            return None;
        }
        bytes.extend(ids[&source.path].to_le_bytes());
        bytes.extend(source.offset.to_le_bytes());
        bytes.extend(source.size.to_le_bytes());
        bytes.extend(pcm.sample_rate().to_le_bytes());
        bytes.extend((pcm.frame_count() as u64).to_le_bytes());
        bytes.extend((width as u32).to_le_bytes());
        bytes.push(source.key.is_some() as u8);
    }
    (bytes.len() as u64 <= BUDGET).then_some(bytes)
}
pub(crate) fn store(
    preset: &Path,
    paths: &[&Path],
    sources: &[Arc<Source>],
    pcm: &[Pcm],
    frame_bytes: &[usize],
) {
    let Some(dir) = dirs::cache_dir().map(|d| d.join("kontra/v2-headers")) else {
        return;
    };
    let Some(bytes) = encode(sources, pcm, frame_bytes) else {
        return;
    };
    let file = entry(&dir, preset, paths);
    let tmp = file.with_extension(format!("tmp{}", std::process::id()));
    if std::fs::create_dir_all(&dir)
        .and_then(|_| std::fs::write(&tmp, bytes))
        .and_then(|_| std::fs::rename(&tmp, &file))
        .is_err()
    {
        let _ = std::fs::remove_file(tmp);
        return;
    }
    // Only the v2 numeric-cache namespace is evicted.
    let mut entries: Vec<_> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|s| s == "headers"))
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            Some((m.modified().ok()?, m.len(), e.path()))
        })
        .collect();
    let mut held: u64 = entries.iter().map(|e| e.1).sum();
    entries.sort_by_key(|e| e.0);
    for (_, size, path) in entries {
        if held <= BUDGET {
            break;
        }
        if std::fs::remove_file(path).is_ok() {
            held -= size;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn numeric_headers_round_trip_and_reject_changed_files_bad_ranges_and_truncation() {
        let root = std::env::temp_dir().join(format!("v2-header-cache-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("sample.ncw");
        std::fs::write(&path, [0; 128]).unwrap();
        let mut samples = Samples::new(&root);
        let sources = vec![Arc::new(samples.source(&path).unwrap())];
        let pcm = vec![Pcm::streamed(48000, 200).unwrap()];
        let bytes = encode(&sources, &pcm, &[6]).unwrap();
        let mut legacy = b"KONTRA headers 1\n".to_vec();
        legacy.extend(&bytes[MAGIC.len()..]);
        assert!(decode(&legacy, &[&path], &mut samples).is_none());
        let got = decode(&bytes, &[&path], &mut samples).unwrap();
        assert_eq!(got[0].header, Some((48000, 200)));
        assert_eq!(got[0].frame_bytes, Some(6));
        let opened = crate::stream::Streamer::open(vec![(Arc::new(got[0].clone()), path.as_path())], 48000, &crate::StreamPolicy::default(), 0, &||false).unwrap();
        assert_eq!(opened.frame_bytes, vec![6]); // Invalid file bytes prove this numeric hit did not open a codec.
        for end in 0..bytes.len() {
            assert!(decode(&bytes[..end], &[&path], &mut samples).is_none());
        }
        let mut invalid_width = bytes.clone();
        let width_at = bytes.len() - 5;
        invalid_width[width_at..width_at + 4].copy_from_slice(&12u32.to_le_bytes());
        assert!(decode(&invalid_width, &[&path], &mut samples).is_none());
        let mut previous_version = bytes.clone();
        previous_version[MAGIC.len()-2] = b'1';
        assert!(decode(&previous_version, &[&path], &mut samples).is_none());
        let mut invalid = bytes.clone();
        let size_at = bytes.len() - 37 + 12;
        invalid[size_at..size_at + 8].copy_from_slice(&1024u64.to_le_bytes());
        assert!(decode(&invalid, &[&path], &mut samples).is_none());
        std::fs::write(&path, [0; 129]).unwrap();
        assert!(decode(&bytes, &[&path], &mut samples).is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
}
