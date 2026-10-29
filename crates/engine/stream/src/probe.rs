//! Whether a library sits on fast storage, measured: a few reads that
//! bypass the page cache, spread over one of its sample files. The preload
//! law's 60 KB per channel is sized for spinning disks; on fast storage a
//! head of 12 KiB per channel still starts every page on time
//! (05-STREAM.md §1).

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

/// Reads per probe.
const READS: u64 = 16;
/// The lower-quartile read at or below this is fast storage (NVMe and SATA
/// SSDs read in about 0.1 ms; a disk seek takes several ms). The quartile,
/// not the median: a busy machine delays some reads (the median read 0.07 ms
/// once and over 1 ms another time at load average 30), a disk never reads
/// a quarter of scattered blocks fast.
const FAST: Duration = Duration::from_millis(1);

/// Whether `file`'s device reads fast, measured once per device per
/// process. `false` when it can't be measured (another platform, a file
/// system without direct reads): the law's own preload then holds.
#[must_use]
pub fn fast_storage(file: &Path) -> bool {
    static SEEN: OnceLock<Mutex<HashMap<u64, bool>>> = OnceLock::new();
    let Some(device) = device(file) else {
        return false;
    };
    let seen = SEEN.get_or_init(Mutex::default);
    if let Some(&fast) = seen.lock().ok().as_ref().and_then(|s| s.get(&device)) {
        return fast;
    }
    let fast = quartile_read(file).is_some_and(|t| t <= FAST);
    if let Ok(mut s) = seen.lock() {
        s.insert(device, fast);
    }
    fast
}

#[cfg(target_os = "linux")]
fn device(file: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    Some(std::fs::metadata(file).ok()?.dev())
}

#[cfg(not(target_os = "linux"))]
fn device(_: &Path) -> Option<u64> {
    None
}

/// The lower quartile of [`READS`] 4 KiB direct reads spread over `file`.
#[cfg(target_os = "linux")]
fn quartile_read(file: &Path) -> Option<Duration> {
    use rustix::fs::{Mode, OFlags};
    const BLOCK: usize = 4096;
    let fd = rustix::fs::open(file, OFlags::RDONLY | OFlags::DIRECT, Mode::empty()).ok()?;
    let size = rustix::fs::fstat(&fd).ok()?.st_size;
    let blocks = u64::try_from(size).ok()? / BLOCK as u64;
    if blocks == 0 {
        return None;
    }
    // Direct reads need a block-aligned buffer.
    let mut buf = vec![0u8; 2 * BLOCK];
    let skip = buf.as_ptr().align_offset(BLOCK);
    let block = buf.get_mut(skip..skip + BLOCK)?;
    let mut times: Vec<Duration> = (0..READS)
        .map(|i| {
            let at = (blocks * i / READS) * BLOCK as u64;
            let start = std::time::Instant::now();
            rustix::io::pread(&fd, &mut *block, at).ok()?;
            Some(start.elapsed())
        })
        .collect::<Option<_>>()?;
    times.sort();
    times.get(times.len() / 4).copied()
}

#[cfg(not(target_os = "linux"))]
fn quartile_read(_: &Path) -> Option<Duration> {
    None
}
