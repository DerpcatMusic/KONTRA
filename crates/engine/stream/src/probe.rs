//! Whether a library sits on fast storage, measured: a few reads that
//! bypass the page cache, spread over one of its sample files. The preload
//! law's 60 KB per channel is sized for spinning disks; on fast storage a
//! head of 12 KiB per channel still starts every page on time
//! (05-STREAM.md §1). The kernel's rotational flag decides where it can be
//! read; the timed probe decides elsewhere.

use std::path::Path;
use std::time::Duration;

/// Reads per probe.
const READS: u64 = 16;
/// The lower-quartile read at or below this is fast storage (NVMe and SATA
/// SSDs read in about 0.1 ms; a disk seek takes several ms). The quartile,
/// not the median: a busy machine delays some reads (the median read 0.07 ms
/// once and over 1 ms another time at load average 30), a disk never reads
/// a quarter of scattered blocks fast.
const FAST: Duration = Duration::from_millis(1);

/// Whether `file`'s device reads fast, measured on each load (16 small
/// reads: about 2 ms on fast storage). `false` when it can't be measured
/// (another platform, a file system without direct reads): the law's own
/// preload then holds.
#[must_use]
pub fn fast_storage(file: &Path) -> bool {
    match rotational(file) {
        Some(spinning) => !spinning,
        None => quartile_read(file).is_some_and(|t| t <= FAST),
    }
}

/// Whether the device holding `file` is a spinning disk, as the kernel
/// reports it: the same answer every time, where a timed read changes with
/// the machine's load (Dolce pinned 52 MB one run and 260 MB the next).
// ponytail: a USB flash stick reports non-rotational and is slower than the
// heads assume; add a floor from the timed probe if one shows up.
#[cfg(target_os = "linux")]
fn rotational(file: &Path) -> Option<bool> {
    let dev = rustix::fs::stat(file).ok()?.st_dev;
    let (major, minor) = (rustix::fs::major(dev), rustix::fs::minor(dev));
    let node = if major == 0 {
        // btrfs and friends number their mounts anonymously: the mount table
        // names the device behind.
        let table = std::fs::read_to_string("/proc/self/mountinfo").ok()?;
        let source = table.lines().find_map(|line| {
            let (head, tail) = line.split_once(" - ")?;
            (head.split_whitespace().nth(2)? == format!("{major}:{minor}"))
                .then(|| tail.split_whitespace().nth(1).map(str::to_owned))?
        })?;
        format!("/sys/class/block/{}", source.rsplit('/').next()?)
    } else {
        format!("/sys/dev/block/{major}:{minor}")
    };
    // A partition keeps the flag on its disk, one level up.
    let flag = ["queue/rotational", "../queue/rotational"]
        .iter()
        .find_map(|rel| std::fs::read_to_string(format!("{node}/{rel}")).ok())?;
    Some(flag.trim() == "1")
}

#[cfg(not(target_os = "linux"))]
fn rotational(_: &Path) -> Option<bool> {
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

#[cfg(test)]
mod tests {
    #[test]
    fn the_answer_for_a_file_does_not_change_between_calls() {
        let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let first = super::fast_storage(&file);
        assert!((0..50).all(|_| super::fast_storage(&file) == first));
    }
}
