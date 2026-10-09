# W9: shipped 0.3.344 slow callbacks in Dolce and Areia

Status: source fix prepared; targeted RED/GREEN, root no-run and the non-quiet
Harmonics pathology pair are pending the explicit W8 machine handoff. Streaming
remains HOLD. This is not CPU parity evidence.

## Attribution

Both exact shipped-344 cells put `Runtime::capture_script_state` on the audio
thread. The caller is `src/sound/v2/persistence.rs:273`, invoked for every loaded
part after every render at `src/sound/v2.rs:1093`. Capture preflights every saved
value and then reads every value again (`crates/sampler-core/src/control/script_state.rs:148–156`
in source 7cd326ee). The full buffer includes large persistent script arrays.
This work continues with zero voices and a closed editor.

The shipped CLAP is SHA-256
`264017ceb04bfdea5e133dd9ed427970134c54c245d2eb48b8a4c06919b9539e`;
its receipt names source `7cd326ee5b67cb78f22fe235bd4b26aaebb0c291`, dirty=true.
W0 has no matching unstripped symbol companion. We did not substitute its
73fb9a7e artifact.

User-space sampling launched the frozen audit host as perf's child, without
changing ptrace/perf permissions. The shipped hot ELF virtual address is
`0x2922037`, within FDE `0x2921f90..0x29221f0`. The function's exact 31-byte
prologue matched an existing unstripped W9 audit witness at ELF VA `0x446160`;
addr2line names `Runtime::capture_script_state`. This is binary correspondence
plus source inspection, not a claimed matching symbol companion.

- Areia 16 Violins Core, 256 frames: 1,844 of 6,165 audio-thread samples hit that
  exact shipped instruction; libc memcpy was also prominent.
- Dolce Harmonics, 64 frames: raw primary-IP parsing recovered the same
  instruction. Its sampled period was 7.828 seconds out of 27.808 seconds on the
  audio thread. Memcpy leaf return addresses `0x2922153` and `0x2922136`, both
  inside capture, each accounted for 5.515 sampled seconds. These copies were
  predominantly 325 bytes (1,096 samples), despite integer cells.

Sampling is attribution only. These periods and sample counts are not block
latencies or quiet measurements. The shorter raw-IP reproduction was deliberately
interrupted after 30 seconds; an optional perf-script flag then failed, while
its numeric primary-IP/leaf-return summary had already been written. The two
full cell profiles completed audibly with matching native readback, finite
output, all expected events, and zero reported stream underruns. Their timing
status is CONTENDED/UNKNOWN.

Raw perf stacks, library state and journals stayed in tmpfs and were destroyed.
Sanitized stack addresses/functions and numeric receipts are under
`/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w9-dolce-p0-20261009/`.
The exact inputs and host digests come from W8's
`w8-sweep-344/W9-DOLCE-HANDOFF.json`; Areia's verified cell ID is
`86504c5269f3a683007175ae0b92df56400bc6eae460bfd37f4b5d8adbe9f1a9`.

## Fix and correctness boundary

The host's value snapshot now skips capture and publication when its
`(PlanId, control revision, script-write revision)` token is unchanged. Script
cell and text mutations invalidate it, including the straight interpreter and
general interpreter. Immutable bank/cell access does not invalidate it. Widget
edits, restores and DSP `ControlOperation::Edit` changes already advance the
control revision. Callback outcomes remain independent of this value-only
publication token.

Changed snapshots retain whole-buffer validation and coherent triple-buffer
publication. Capture's value getter is inlined and atom writes borrow the value,
removing by-value tagged scalar copies. The audio writer adds no allocation,
lock or readback request.

The v1 path was inspected: `0cb7a8a0:src/plugin.rs:3861` uses
`src/ksp/runtime.rs:1399` to refresh on demand in 16,384-value chunks. We retain
v2's coherent typed snapshot and skip unchanged work rather than expose a
partially refreshed host save. Conservative remaining limit: a write anywhere
in script storage invalidates the full snapshot, including nonpersistent cells;
a frequently changing large table can still require an expensive publication.
The short before/after reproduction must establish whether the idle gate closes
this reported pathology. A quiet slot is still required for CPU parity.

Regression commit: `7fa595e8` (unchanged 32K-cell block must neither visit staging
storage nor change the published slot). Fix: `034a4e4f`; no-heap restore/read
coverage: `c6db4941`. Tests also require a DSP-only queued edit to appear in the
next host save, script fast/general/array/text mutations to advance the token,
and reads to leave it unchanged. Existing KSP capture/restore and concurrent
host-save tests are included in targeted validation.

NEXT: W8 release → targeted RED/GREEN, root no-run, one non-quiet Harmonics pair
→ direct W6 release.
