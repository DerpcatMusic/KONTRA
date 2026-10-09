# W8 compact persistence preparation

Source: 67b6bfc6cc093ebb33842c13c8501859521e4983 on
v2/w8-load-attribution-resume, after preserved 7436689b and 323c5ccd.
Receipt directory: /mnt/Windows11/DEV_WORKSPACE/kontra-runs/w8-preparation-20261010.

Fresh preparation previously created a ScriptStateEntry for every numeric value;
that enum includes fixed-capacity text. Retained audio snapshots were already
compact, but this temporary buffer still inflated load peak RSS. Preparation now
uses compact ordered addresses, reads each live value individually, and seeds all
three atomic slots. The sparse callback path, publication protocol and host JSON
schema stay unchanged. The per-value schema formatter reuses one String.

The numeric regression measures peak allocations for 32,768 authored persistent
cells, including snapshot slots and masks, and limits them to 128 bytes per cell.
The shared-target results are untrusted and excluded. The corrected per-worktree
wrapper RED reproduced the intended 20,058,576-byte peak failure. The exact
committed candidate passed the bound of 4,194,304 bytes (at least 79% less peak
allocation), all 11 host persistence tests, 2 core revision tests, KSP state
tests, stream readiness, offline PCM, widget callback and area lib no-run.
No mtime workaround remains in the driver. TRANSPORT.json records the wrapper
digest and actual target. Source was committed before temporary baseline writes.

The compatibility fixture compares the prior schema algorithm and exact initial
host JSON, recalls typed numeric/text values, verifies one persistence callback
after the whole batch, checks all three seeded slots, and rejects a wrong-type
recall without replacing the existing published host state. Existing concurrent
save, exact scalar-bit/UTF-8, rejected capture, sparse slot rotation, restore and
zero-allocation callback witnesses remain required.

v1 reference: 0cb7a8a0 src/ksp/runtime.rs read_value and persistence. Its numeric
arrays do not reserve inline text per element. This adapts that storage property
to v2's existing address/atomic-slot representation; no v1 file format is added.
Cross-scope changes are sampler-ksp's persistence layout helper and sampler-core's
capture-domain registration plus its revision test. No KSP service, W0 tree,
streaming storage, UI publication or dependency change.

Known limit: saved-state recalls retain the core's existing wide validated batch
buffer and callbacks. This slice removes fresh capture staging, not that restore
API. Bulk dirty refreshes remain proportional to changed values. The previous
QUIET sparse onset receipt is 11.648 ms Areia / 6.574 ms Dolce versus frozen v1
1.739 / 1.721 ms; these are historical, not measurements of this source.

READY: product 67b6bfc6cc093ebb33842c13c8501859521e4983. Apply after the
preserved 6885c290 → 7436689b → 323c5ccd source chain; this is not standalone.
FREEZE.json records the corrected per-worktree transport and frozen probe.
CHECKS.json records every successful command. No all-14 rerun or new timing is
claimed. The observer remains f9d69ce6 and retains polling CPU as contention.

NEXT: remaining callback preemption and first-page ordering; frozen Areia/Dolce
pair only after W9 DIRECT, then fully drained DIRECT to W10.
