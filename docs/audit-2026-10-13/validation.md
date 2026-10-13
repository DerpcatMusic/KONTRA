# Parity implementation and validation, 2026-10-13

The accepted production corrections cover native scalar Kontakt one-band EQ,
KSP table display columns, private instrument replacement and recalled-state
ownership, and UVI coroutine release ownership. The UI sweep also refuses
missing tests, zero-test successes, and incomplete measurement evidence.
The coordinator owns compilation and native execution; Sol reviewers performed
source work and synthetic checks without building or running native artifacts.

## Library inspection

A fresh release inspector reconciled all **1,495 manifest items across 15
libraries**: 781 NKI instruments, 54 NKM multis, and 660 UVI bank programs.
All 60 shards completed with zero integrity faults, missing items, or unexpected
items. All expected UVI programs reached production XML-to-IR conversion.
Every source report remains partial. The 94,659,900 construct occurrences
include repeated library code; they are not unique APIs or parity proofs.

All **17 representative initialized selections** reached IR. Nine selected
UI models have complete structural projection and eight remain incomplete.
The aggregate's `initialized_failed: 8` describes these incomplete selections;
it must not be presented as eight initializer crashes. Static inspection,
initialization, UI projection, render/gesture evidence, audio, and native
equivalence remain separate stages.

The source-free [aggregate](library-source-summary.json) and
[per-library table](library-source-summary.md) retain denominators and input
digests. Raw reports, frozen inspector identity, and execution receipts are
under `/mnt/MAIN_STORAGE/kontra-runs/native-parity-20261013/` outside Git.
Library files remained read-only and musical samples were not auditioned in
this new catalog run. Prior audibility receipts are historical evidence.

## Software and native evidence

All 14 first-party packages and 220 Cargo test targets were exercised. The
first combined run had three newly authored loader-fixture capacity failures
and one existing editor timing failure. The fixtures were corrected; the
affected root target passed serially with **666 tests, zero failures, 29
ignored**. Combined distinct Cargo-target results are **2,325 passed, 153
ignored**. Nested child-process result summaries are excluded from this count.
The final audit-only readiness regression and ignored installed Conflux callback gate also passed after correcting outbox servicing and classifying recurring listeners.

The UI sweep's **18 synthetic Python tests** passed. Against the actual
compiled current executable, the sweep rejected its absent historical
benchmark with exit 1 before creating result artifacts. That is an expected
negative admission check, not measured UI performance.

Fresh original-instruction validation passed **1,222 Kontakt records** and
**3,937 Workstation cases**. The [native evidence](native-reference-validation.md)
explains hashes, helper substitutions, saved snapshot recovery, exact case
boundaries, and the corrected previously vacuous EQ sample check. No Falcon
executable was located. Workstation establishes the demonstrated shared DSP
paths, not all Falcon modules, complete host behavior, or library parity.

The coordinator's rebuilt plugin and standalone passed before the final
versioned clean-source build. Release identity and installation require the
later exact commit/version build and factory checks; provisional dirty builds
are not shipping artifacts.

## Explicit UI limits

Current real-library Conflux, AO Bartok, and A Clarinet full-editor CPU paint
gates passed. They validate their declared editor/resource contracts, not
native pixels. Conflux Original visually renders its authored composition;
Vector visibly overlaps raw KSP controls because native authored layout is
not projected into that route. The [layout diagnosis](conflux-vector-layout.md)
records this unresolved capability; successful painting cannot admit visual
correctness.

The old Conflux callback gate observed unfinished callbacks behind a full
256-effect outbox. The corrected gate passed with the same sampled controls, its unchanged exact-three-effects assertion, and target label readback. Its servicing/readiness correction has a separate
[audit note](conflux-callback-gate.md) and coordinator receipt. The original
failure does not prove a production callback regression. Native UI/event,
complete resource lifetime, multiband/SIMD EQ, UVI OnePole production port,
asynchronous `loadSample`, and calibrated host audio parity remain open.
