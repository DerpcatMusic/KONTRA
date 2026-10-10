# W8 bounded host callback ordering — READY

Verified source: c591b435cf7b2919f5144c9bdd3686c1cfa5cbec on
v2/w8-load-attribution-resume. Apply c250646e → 01e33a7d → 4e949aca →
c591b435 after the preserved compact-persistence product chain ending 67b6bfc6.
The receipt commit adds only this document and a scheduler limit comment.
External evidence: /mnt/Windows11/DEV_WORKSPACE/kontra-runs/w8-callback-block-20261010.

The production plugin already calls begin_block before host MIDI. v2 previously
installed script fuel during render, after initial callbacks, and refilled it
after render fragments. A 64-frame callback shared only 8,192 instructions and
had no elapsed-time allowance. The frozen Areia/Dolce callbacks spanned six/three
blocks before voices started.

Ported reference: v1 0cb7a8a0 src/ksp/runtime.rs begin_audio_block and resume;
src/ksp/vm.rs deadline checks at Checkpoint, backwards Jump and Call. v2 now
installs frames*2048/loaded_parts and 40% of host-block time divided by loaded
parts before events. Offline blocks retain a larger deterministic instruction
allowance without clock checks. Each part consumes its remaining active callback
time across events and resumes. Host render fragments and empty renders cannot
refill it. Legacy render-driven callers retain their instruction-only policy.
Invalid host rates exhaust time safely. Straight runs check time between bounded
slices; general operations check periodically through the existing scheduler.

The load probe now begins the host block before first MIDI and each later render
block. The CPU adapters and offline PCM witnesses also begin before MIDI, as the
production plugin does. Historical frozen load probes omitted this boundary;
their numbers cannot establish production callback-budget parity.

Trusted corrected per-worktree wrapper verification:

- RED: the authored 10,000-increment callback stopped at 2,048 under the baseline
  loader cap; the intended assertion failed with exit 101. The driver restored
  exact candidate blobs before GREEN in its finally block.
- GREEN: 62 actual tests passed: host allowance 2, core behavior 29, host
  persistence 11, core revision 2, KSP state 5, stream readiness 1, offline PCM 1,
  widget callback 1, and corrected alignment selection 10.
- The fixtures cover shared fuel across callbacks/fragments, exhausted time,
  positive-time preemption of a 100,000-increment straight run with exact resume,
  allocation-free callback execution, rack/frame sizing and callback order.
- Three audit examples compile with the required shots feature. The authored
  v1 Rust adapter compiles with its existing dependency; no third-party host ran.
- kontakto, sampler-core and sampler-ksp library area no-run passed.

CHECKS.json preserves the initial missing-feature example failure and zero-test
alignment selection; neither is a valid pass. REMAINING-CHECKS.json records their
successful corrections. READY-CHECKS.json consolidates only valid results.
TRANSPORT.json pins the actual per-worktree target and wrapper digest. FREEZE.json
pins the default-feature ci probe binary and source; it has not been timed.
No target override, timestamp manipulation or foreign worktree mutation occurred.

Scope outside the load adapter: minimal sampler-core scheduler fields,
behavior/render changes and the behavior fixture, plus existing audit hooks.
W9 was notified. No KSP service, streaming storage, UI publication, dependency or
host-save schema changes.

Limits: one expensive general instruction can exceed the soft deadline. The
allowance bounds interpreter work between checks, not a hard realtime deadline.
Cold-page onset remains open. The preceding frozen 67b6 pair was QUIET:
Areia install-to-sound 45.953 ms and editor RSS 564.520 MiB; Dolce 18.401 ms and
468.922 MiB. It recorded 36/47 cold starts and 4/1 underruns after callbacks.
The stream reload all-asset head-byte sum inside each cold-asset iteration was
reported to W9 for its storage lane. No post-change onset, dense-chord CPU,
all-14 rerun or v1 parity is claimed.

The pair drained and DIRECT went to W0 for release priority. The untimed checks
also drained with MainPID 0 and exit 0. No W8 quiet request/grant/override remains;
latest user steering handed DIRECT to W9 for its frozen Horns A/B.

NEXT: W0 integrates the READY chain; W9 owns the measured cold-stream/CPU follow-up.
