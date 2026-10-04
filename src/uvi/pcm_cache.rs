//! Optional initial-static PCM cache. Cache failures are misses; owned PCM is
//! fully validated before Lua sees the same aliases as the original loader.
use super::{
    library::{self, Library, LoadedProgram},
    sample::{self, Sample, SampleLoop},
    storage::Storage,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

const MAGIC: &[u8; 8] = b"UVIPCM02";
const META_LIMIT: usize = 16 << 20;
const META_INPUT_LIMIT: usize = 2 << 20;
const SOURCE_LIMIT: usize = 256 << 20;
const BANK_LIMIT: u64 = 512 << 20;
// ponytail: one replaceable cache slot; a disk LRU is unnecessary for this experiment.
static WRITE_LOCK: Mutex<()> = Mutex::new(());
static SERIAL: AtomicU64 = AtomicU64::new(0);
type Samples = HashMap<String, Arc<Sample>>;
type Progress<'a> = dyn FnMut(usize, usize, usize, usize, Option<&str>) + 'a;
#[derive(Serialize, Deserialize, PartialEq, Eq)]
struct Alias {
    path: String,
    identity: Vec<u64>,
    slot: usize,
}
#[derive(Serialize, Deserialize)]
struct Entry {
    rate: u32,
    channels: usize,
    frames: usize,
    loops: Vec<SampleLoop>,
    unity_note: Option<u32>,
    wavetable_cycle_frames: Option<u32>,
    riff_metadata: Vec<Vec<u8>>,
    offset: usize,
    len: usize,
    tag: u8,
}
#[derive(Serialize, Deserialize)]
struct Meta {
    bank: [u8; 32],
    program: [u8; 32],
    contract: [u8; 32],
    aliases: Vec<Alias>,
    entries: Vec<Entry>,
}
fn hash(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}
fn contract() -> [u8; 32] {
    let mut h = Sha256::new();
    for bytes in [
        include_bytes!("pcm_cache.rs").as_slice(),
        include_bytes!("sample.rs").as_slice(),
        include_bytes!("storage.rs").as_slice(),
        include_bytes!("library.rs").as_slice(),
        include_bytes!("generator.rs").as_slice(),
        include_bytes!("ufs.rs").as_slice(),
        include_bytes!("crypto.rs").as_slice(),
        include_bytes!("../audio.rs").as_slice(),
        include_bytes!("../../Cargo.lock").as_slice(),
        include_bytes!("../../Cargo.toml").as_slice(),
    ] {
        h.update(bytes);
    }
    h.finalize().into()
}
fn bank_hash(path: &Path, stop: Option<&AtomicBool>) -> Result<[u8; 32]> {
    let mut file = open_regular(path)?;
    let before = file.metadata()?;
    ensure!(
        before.len() <= BANK_LIMIT,
        "Bank outside optional cache bound"
    );
    let mut hash = Sha256::new();
    let mut scratch = vec![0; 1 << 20];
    let mut bytes = 0u64;
    loop {
        sample::check_cancel(stop)?;
        let n = file.read(&mut scratch)?;
        sample::check_cancel(stop)?;
        if n == 0 {
            break;
        }
        bytes = bytes
            .checked_add(n as u64)
            .context("Cache bank length overflow")?;
        ensure!(bytes <= BANK_LIMIT, "Bank outside optional cache bound");
        hash.update(&scratch[..n]);
    }
    let after = file.metadata()?;
    ensure!(
        bytes == before.len()
            && before.len() == after.len()
            && before.modified()? == after.modified()?,
        "Bank changed while fingerprinting"
    );
    Ok(hash.finalize().into())
}
fn plan(lib: &Library, loaded: &LoadedProgram) -> Result<Vec<Alias>> {
    let mut seen = HashSet::new();
    let mut identities = HashMap::new();
    let mut aliases = Vec::new();
    let mut bytes = 0;
    let paths = library::initial_paths(loaded);
    ensure!(!paths.is_empty() && paths.len() <= 65536, "Optional cache path bound");
    for (path, image) in paths {
        ensure!(!image, "Image/wavetable import uses the original loader");
        if !seen.insert(path) {
            continue;
        }
        bytes = library::alias_bytes(bytes, path)?;
        let identity = lib.audio_identity(&loaded.path, path)?;
        for id in &identity {
            let member = lib
                .directory
                .files
                .iter()
                .find(|m| m.record_offset == *id)
                .context("Missing cache source record")?;
            ensure!(
                member.mode == 0 && member.size > 0 && member.size <= SOURCE_LIMIT as u64,
                "Only bounded unencrypted sample records are cached"
            );
        }
        let next = identities.len();
        let slot = *identities.entry(identity.clone()).or_insert(next);
        aliases.push(Alias {
            path: path.to_owned(),
            identity,
            slot,
        });
    }
    ensure!(
        bytes <= META_INPUT_LIMIT,
        "Optional cache metadata input bound"
    );
    Ok(aliases)
}
struct HashRead<R> {
    inner: R,
    hash: Sha256,
}
impl<R: Read> Read for HashRead<R> {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(bytes)?;
        self.hash.update(&bytes[..n]);
        Ok(n)
    }
}
struct HashWrite<W> {
    inner: W,
    hash: Sha256,
}
impl<W: Write> Write for HashWrite<W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let n = self.inner.write(bytes)?;
        self.hash.update(&bytes[..n]);
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}
fn validate_entry(e: &Entry, alias: &Alias, lib: &Library) -> Result<usize> {
    ensure!(
        e.rate > 0 && e.channels > 0 && e.channels <= 64 && e.frames > 0,
        "Invalid cache dimensions"
    );
    let count = e
        .frames
        .checked_mul(e.channels)
        .context("Cache scalar overflow")?;
    ensure!(count <= SOURCE_LIMIT / 4, "Cached decoded PCM bound");
    ensure!(
        e.riff_metadata.len() <= 4096
            && e.riff_metadata
                .iter()
                .try_fold(0usize, |n, v| n.checked_add(v.len()))
                .is_some_and(|n| n <= 2 << 20),
        "Cache RIFF metadata bound"
    );
    ensure!(e.loops.len() <= 4096, "Cache loop metadata bound");
    for id in &alias.identity {
        let member = lib
            .directory
            .files
            .iter()
            .find(|m| m.record_offset == *id)
            .context("Cache source record missing")?;
        let scalars = if alias.identity.len() == 1 {
            count
        } else {
            e.frames
        };
        ensure!(
            scalars <= (SOURCE_LIMIT - member.size as usize) / 4,
            "Cache encoded and decoded sample bound"
        );
    }
    if alias.identity.len() > 1 {
        ensure!(
            e.channels == alias.identity.len(),
            "Cache mono bundle geometry"
        );
    }
    Ok(count)
}
fn open_regular(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    ensure!(
        file.metadata()?.file_type().is_file(),
        "Optional cache input is not a regular file"
    );
    Ok(file)
}

fn read(
    path: &Path,
    lib: &Library,
    loaded: &LoadedProgram,
    bank: [u8; 32],
    aliases: &[Alias],
    stop: Option<&AtomicBool>,
    progress: &mut Progress<'_>,
) -> Result<Samples> {
    sample::check_cancel(stop)?;
    let mut file = open_regular(path)?;
    let extent = usize::try_from(file.metadata()?.len())?;
    ensure!(
        (80..=80 + META_LIMIT + library::PCM_LIMIT).contains(&extent),
        "Cache extent bound"
    );
    let mut prefix = [0; 80];
    file.read_exact(&mut prefix)?;
    sample::check_cancel(stop)?;
    ensure!(&prefix[..8] == MAGIC, "Cache version mismatch");
    let size = usize::try_from(u64::from_le_bytes(prefix[8..16].try_into()?))?;
    ensure!(
        size <= META_LIMIT && 80 + size <= extent,
        "Cache metadata extent"
    );
    let mut json = vec![0; size];
    file.read_exact(&mut json)?;
    sample::check_cancel(stop)?;
    ensure!(hash(&json) == prefix[16..48], "Cache metadata checksum");
    let meta: Meta = serde_json::from_slice(&json)?;
    ensure!(
        meta.bank == bank
            && meta.program == super::state::fingerprint(&loaded.program)?
            && meta.contract == contract()
            && meta.aliases == aliases,
        "Stale static cache"
    );
    let metadata = meta.entries.iter().try_fold(
        aliases
            .iter()
            .map(|a| a.path.len() + a.identity.len() * 8)
            .sum::<usize>(),
        |n, e| {
            e.riff_metadata
                .iter()
                .try_fold(n, |n, v| n.checked_add(v.len()))
                .and_then(|n| n.checked_add(e.loops.len().checked_mul(24)?))
        },
    );
    ensure!(
        metadata.is_some_and(|n| n <= META_INPUT_LIMIT),
        "Cache aggregate metadata input bound"
    );
    let slots = aliases.iter().map(|a| a.slot + 1).max().unwrap_or(0);
    ensure!(
        meta.entries.len() == slots,
        "Cache identity occupancy mismatch"
    );
    let mut reader = HashRead {
        inner: file,
        hash: Sha256::new(),
    };
    let mut pcm = Vec::new();
    let mut resident = 0;
    progress(aliases.len(), 0, 0, 0, None);
    for (slot, e) in meta.entries.into_iter().enumerate() {
        let alias = aliases
            .iter()
            .find(|a| a.slot == slot)
            .context("Cache slot not referenced")?;
        let count = validate_entry(&e, alias, lib)?;
        ensure!(e.offset == resident, "Noncontiguous cache PCM");
        resident = resident
            .checked_add(e.len)
            .context("Cache resident overflow")?;
        ensure!(
            resident <= library::PCM_LIMIT && resident <= extent - 80 - size,
            "Cache resident PCM bound"
        );
        progress(aliases.len(), 0, 0, 0, Some(&alias.path));
        let interleaved = Storage::cache_read(&mut reader, e.tag, count, e.len, stop)?;
        pcm.push(Arc::new(Sample {
            rate: e.rate,
            channels: e.channels,
            frames: e.frames,
            interleaved,
            loops: e.loops,
            unity_note: e.unity_note,
            wavetable_cycle_frames: e.wavetable_cycle_frames,
            wavetable_image: false,
            riff_metadata: e.riff_metadata,
        }));
    }
    ensure!(
        resident == extent - 80 - size,
        "Cache payload extent mismatch"
    );
    ensure!(
        reader.hash.finalize().as_slice() == &prefix[48..80],
        "Cache PCM checksum"
    );
    sample::check_cancel(stop)?;
    let mut result = HashMap::new();
    let mut admitted = HashSet::new();
    let mut bytes = 0;
    for alias in aliases {
        let sample = pcm
            .get(alias.slot)
            .context("Invalid cache alias slot")?
            .clone();
        if admitted.insert(alias.slot) {
            bytes += sample.interleaved.bytes();
        }
        result.insert(alias.path.clone(), sample);
        // A cache hit performs zero codec decodes. Resident/alias counters remain exact.
        progress(aliases.len(), result.len(), 0, bytes, Some(&alias.path));
    }
    progress(aliases.len(), result.len(), 0, bytes, None);
    Ok(result)
}
struct Temp<'a> {
    path: &'a Path,
}
impl Drop for Temp<'_> {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.path);
    }
}
fn write(
    path: &Path,
    lib: &Library,
    loaded: &LoadedProgram,
    bank: [u8; 32],
    aliases: Vec<Alias>,
    samples: &Samples,
    stop: Option<&AtomicBool>,
) -> Result<()> {
    let Ok(_lock) = WRITE_LOCK.try_lock() else {
        return Ok(());
    };
    let mut entries = Vec::new();
    let mut offset = 0;
    let mut metadata = aliases
        .iter()
        .map(|a| a.path.len() + a.identity.len() * 8)
        .sum::<usize>();
    for alias in &aliases {
        if alias.slot < entries.len() {
            continue;
        }
        ensure!(alias.slot == entries.len(), "Invalid cache alias ordering");
        let s = samples
            .get(&alias.path)
            .context("Missing successful static sample")?;
        ensure!(
            !s.wavetable_image,
            "Image resource cannot be cached as audio"
        );
        metadata = metadata
            .checked_add(s.riff_metadata.iter().map(Vec::len).sum::<usize>())
            .and_then(|n| n.checked_add(s.loops.len().checked_mul(24)?))
            .context("Cache metadata size overflow")?;
        ensure!(
            metadata <= META_INPUT_LIMIT,
            "Optional cache metadata input bound"
        );
        let e = Entry {
            rate: s.rate,
            channels: s.channels,
            frames: s.frames,
            loops: s.loops.clone(),
            unity_note: s.unity_note,
            wavetable_cycle_frames: s.wavetable_cycle_frames,
            riff_metadata: s.riff_metadata.clone(),
            offset,
            len: s.interleaved.bytes(),
            tag: s.interleaved.cache_tag()?,
        };
        validate_entry(&e, alias, lib)?;
        offset = offset
            .checked_add(e.len)
            .context("Cache resident overflow")?;
        ensure!(offset <= library::PCM_LIMIT, "Cache resident budget");
        entries.push(e);
    }
    let meta = Meta {
        bank,
        program: super::state::fingerprint(&loaded.program)?,
        contract: contract(),
        aliases,
        entries,
    };
    let json = serde_json::to_vec(&meta)?;
    ensure!(json.len() <= META_LIMIT, "Optional cache metadata bound");
    let parent = path.parent().context("Cache path has no parent")?;
    std::fs::create_dir_all(parent)?;
    let name = path
        .file_name()
        .context("Cache path has no filename")?
        .to_string_lossy();
    let temp = path.with_file_name(format!(
        ".{name}.{}.{}.tmp",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temp)?;
    let _cleanup = Temp { path: &temp };
    sample::check_cancel(stop)?;
    file.write_all(MAGIC)?;
    file.write_all(&(json.len() as u64).to_le_bytes())?;
    file.write_all(&hash(&json))?;
    file.write_all(&[0; 32])?;
    file.write_all(&json)?;
    let mut writer = HashWrite {
        inner: &mut file,
        hash: Sha256::new(),
    };
    for slot in 0..meta.entries.len() {
        let alias = meta
            .aliases
            .iter()
            .find(|a| a.slot == slot)
            .context("Cache slot missing")?;
        samples[&alias.path]
            .interleaved
            .write_cache(&mut writer, stop)?;
    }
    let payload: [u8; 32] = writer.hash.finalize().into();
    file.seek(SeekFrom::Start(48))?;
    file.write_all(&payload)?;
    // Cache is disposable: checksums recover partial storage after a crash.
    // Avoid a durability-only fsync on the initial loading path.
    sample::check_cancel(stop)?;
    std::fs::rename(&temp, path)?;
    Ok(())
}

/// Only the initial Worker path opts in. Public Library/CLI and dynamic callbacks
/// retain their original reader/decoder path, error ordering and revision checks.
pub(crate) fn load(
    lib: &Library,
    loaded: &LoadedProgram,
    bank_path: &Path,
    cache_path: Option<&Path>,
    progress: &mut Progress<'_>,
    stop: Option<&AtomicBool>,
) -> Result<(Samples, Option<bool>)> {
    sample::check_cancel(stop)?;
    let enabled = cache_path.is_some();
    let prepared = (|| -> Result<_> {
        let path = cache_path.context("Cache disabled")?;
        let aliases = plan(lib, loaded)?;
        let bank = bank_hash(bank_path, stop)?;
        Ok((path, aliases, bank))
    })();
    let prepared = match prepared {
        Ok(p) => Some(p),
        Err(e) if e.is::<sample::LoadCancelled>() => return Err(e),
        Err(_) => None,
    };
    if let Some((path, aliases, bank)) = &prepared {
        match read(path, lib, loaded, *bank, aliases, stop, progress) {
            Ok(samples) => match bank_hash(bank_path, stop) {
                Ok(current) if current == *bank => return Ok((samples, Some(true))),
                Err(e) if e.is::<sample::LoadCancelled>() => return Err(e),
                _ => {} // Drop all cache PCM if the bank changed during its read.
            },
            Err(e) if e.is::<sample::LoadCancelled>() => return Err(e),
            Err(_) => {} // All rejected cache PCM has dropped before original decoding.
        }
    }
    let samples = lib.samples_with_progress_cancel(loaded, progress, stop)?;
    if let Some((path, aliases, bank)) = prepared {
        let publish = (|| -> Result<()> {
            ensure!(
                bank_hash(bank_path, stop)? == bank,
                "Bank changed during static decode"
            );
            write(path, lib, loaded, bank, aliases, &samples, stop)
        })();
        if let Err(e) = publish {
            if e.is::<sample::LoadCancelled>() {
                return Err(e);
            }
        }
    }
    Ok((samples, enabled.then_some(false)))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn fifo_without_writer_is_rejected_without_opening_a_stream() {
        use std::os::unix::ffi::OsStrExt;
        let path = std::env::temp_dir().join(format!(
            "uvi-cache-fifo-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        let name = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        // SAFETY: valid NUL-terminated authored path; no vendor/source file used.
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        let result = open_regular(&path);
        std::fs::remove_file(path).unwrap();
        assert!(result.is_err());
    }
}
