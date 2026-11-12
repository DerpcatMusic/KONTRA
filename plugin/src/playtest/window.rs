//! Hardware counters around one segment, for `tools/perf/window.py`: when
//! `KONTRA_PERF_SEGMENT` names a segment, `perf stat` (started disabled, its
//! control FIFOs in `KONTRA_PERF_CTL` and `KONTRA_PERF_ACK`) counts only that
//! segment's blocks until its last voice ends (not the effects' ring-out),
//! and the count of blocks is printed
//! as `PERFWIN <program>|<segment>|<blocks>`. Without the variables nothing
//! happens.
//!
//! For `tools/perf/allocs.py`, with `KONTRA_ALLOC_MARKS` set, every
//! `process` call is bracketed by a lookup of `KX_ALLOC_ON` and then
//! `KX_ALLOC_OFF`: its preloaded counter takes those two `getenv` names as
//! the marks and counts the allocations between them.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::sync::OnceLock;

/// Either side of one `process` call.
pub(super) enum Mark {
    Enter,
    Leave,
}

/// Tells an allocation counter that `process` starts or ends, if one asked.
pub(super) fn process_mark(mark: Mark) {
    static ASKED: OnceLock<bool> = OnceLock::new();
    if *ASKED.get_or_init(|| std::env::var_os("KONTRA_ALLOC_MARKS").is_some()) {
        let name = match mark {
            Mark::Enter => "KX_ALLOC_ON",
            Mark::Leave => "KX_ALLOC_OFF",
        };
        let _ = std::env::var_os(name);
    }
}

/// Starts counting if `segment` is the one asked for; whether it is.
pub(super) fn open(segment: &str) -> bool {
    let asked = std::env::var("KONTRA_PERF_SEGMENT").is_ok_and(|s| s == segment);
    if asked {
        command("enable\n");
    }
    asked
}

/// Stops counting and reports the window's blocks.
pub(super) fn close(program: &str, segment: &str, blocks: usize) {
    command("disable\n");
    eprintln!("PERFWIN {program}|{segment}|{blocks}");
}

/// One command to perf, waiting for its acknowledgement so the window
/// starts and ends at the block boundary.
fn command(cmd: &str) {
    let (Some(ctl), Some(ack)) = (
        std::env::var_os("KONTRA_PERF_CTL"),
        std::env::var_os("KONTRA_PERF_ACK"),
    ) else {
        return;
    };
    let sent = OpenOptions::new()
        .write(true)
        .open(ctl)
        .and_then(|mut f| f.write_all(cmd.as_bytes()));
    if sent.is_ok() {
        let mut reply = [0u8; 5];
        let _ = File::open(ack).and_then(|mut f| f.read(&mut reply));
    }
}
