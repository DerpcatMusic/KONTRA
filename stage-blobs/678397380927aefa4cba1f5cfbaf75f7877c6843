# W9: shipped 0.3.344 slow callbacks in Dolce and Areia

Status: targeted RED→GREEN, root no-run and the requested non-quiet Harmonics
pair complete. Code READY: `5592510e64630e433f4607c2f43d1ff49cf489d3`, pushed
and remote-verified. The machine was released directly to W12 after all owned
jobs drained. Streaming remains HOLD; this is not CPU parity evidence.

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
The requested short reproduction no longer shows the severe continuous
slow-callback class. A quiet slot is still required for CPU parity.

Regression commit: `7fa595e8` (unchanged 32K-cell block must neither visit staging
storage nor change the published slot). Fix: `034a4e4f`; no-heap restore/read
coverage: `c6db4941`. Tests also require a DSP-only queued edit to appear in the
next host save, script fast/general/array/text mutations to advance the token,
and reads to leave it unchanged. Existing KSP capture/restore and concurrent
host-save tests are included in targeted validation.

## Validation and non-quiet before/after

The failing-first test at `7fa595e8` failed on the intended assertion: an idle
block changed poisoned staging `Cell(-123)` back to live `Cell(17)`. With the fix:

- One core revision/fast/general/array/text/no-heap test passed.
- All five KSP state tests passed, including exact values, restore rejection,
  callback outcomes and allocation-free transport.
- All four host persistence tests passed: DSP-only `ControlOperation::Edit` →
  next save, unchanged 32K-cell skip, allocation-free restore/publication,
  exact scalar/text preservation and coherent concurrent saves.
- Root `cargo test --no-run` passed. No full-suite/gate run was added.

The diagnostic CLAP (no release/install) was built in release with
`clap,library-access,plugin`, source `5592510e`, SHA-256
`774bb65d2b384c51f4f9948124877a300e8f16fac62604fc92553a9aae96e8c1`.
The pair used the same frozen host and exporter, identical native-state hash
`2f93521bb0d843f7b6eb4a81b125088196c2f64b026a2270ea8281a79c9eefed`, and identical
audition-event hash `5d1a5e68a01801748ea48bb00e066441d7e3db8b625dca0e118dd3252645a2a2`.
Harmonics program0, key73/velocity64, 48kHz/64 frames, two audio seconds:

| Observation | Shipped 344 | Candidate |
|---|---:|---:|
| Callback wall p50 µs | 73,806.225 | 12.410 |
| Callback wall p99 µs | 88,011.783 | 161.663 |
| Audio-thread CPU p50 µs | 73,337.189 | 13.150 |
| Audio-thread CPU p99 µs | 86,847.782 | 160.391 |
| Observer wall seconds | 118.973 | 5.729 |
| Observer wall / audio seconds | 59.487 | 2.864 |
| Callback deadline misses / 1,500 | 1,500 | 14 |
| Wake deadline misses | 1,499 | 736 |
| Reported stream underruns | 0 | 0 |

Both completed all 1,500 blocks and four expected events, with finite audible
output and verified native readback. The wall observation includes load, audition,
host save and collector completion; it is not pure render time. Peaks differed
(0.0465093 versus 0.0383650); this was not a seeded PCM-equivalence test.

There was no quiet request/grant. The observer labelled before CONTENDED and
after QUIET, so this asymmetric one-shot pair remains UNKNOWN for CPU parity.
Product cache was disabled; OS cache was uncontrolled, with no separate cold/warm
claim. No candidate Areia/CHORUS or other Dolce preset was retested in this short
slot. The shared hot frame was confirmed on Areia before the fix.

Receipts: `validation/{SUMMARY.json,factor-pair.json,BUILD.json}` and numeric
host diagnostics under the run folder. Raw library state and journals remained
in tmpfs and were destroyed. Validation unit ended not-found/inactive/dead,
MainPID0, with no owned job/waiter/request/grant. W12 received the explicit direct
release with `validation/direct-handoff-W12.json`; W12 releases W6 after its GREEN.

NEXT: W12 format-fix GREEN → W6 → W13 → W11; W9 CPU ports resume afterward.
