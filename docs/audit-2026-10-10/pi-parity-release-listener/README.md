# Listener context and range checks — source-only candidate

Base: `13c044d6f9f80576507521a2dc6521e9aad7d9c5`. Private branch:
`pi-parity-release-listener-20261010`. No merge is authorized or performed.

## Primary authority

`primary-authority.json` contains only the two raw sections extracted directly
from the archived NI KSP `ksp-time.html`. Verified document SHA-256:
`aafd0e71a266571cc5a6adb9dbfb15bb31f4c4819e9748427634d171b34fa2f9`.
Each section has its own raw-section SHA-256. Historical source3665 handoffs and
project compatibility claims were not used as evidence for CURRENT13 behavior.

- `set_listener`: init only; TIMER_MS >= 1000 microseconds; TIMER_BEAT 1..24;
  TRANSP_START and TRANSP_STOP parameter 1; no other documented signals.
- `change_listener_par`: any callback; only TIMER_MS and TIMER_BEAT; same positive
  ranges, with 0 permitted to disable either timer.

## Changes and exact current-source callers

- `crates/sampler-ksp/src/sema.rs`: `Sema::builtin` checks constant listener
  signals and parameters. `listener_range` supplies the documented bounds.
- `crates/sampler-ksp/src/eval.rs`: `Eval::builtin_inner` validates evaluated
  arguments before inserting into `Initial.model.listeners`. Its `callback_type`
  rejects non-init `set_listener`, including calls through user functions.
  `eval::run` is called by initialization in `src/lib.rs`.
- `crates/sampler-ksp/src/lower.rs`: `Gen::builtin` rejects every lowered
  `set_listener`; `src/lib.rs::compile_initialized_inner` skips init, so these
  are non-init callback contexts. `Unit::program` emits reachable functions in
  the caller's context. Live `change_listener_par` evaluates each argument once,
  accepts only valid timer values or zero, and writes the existing store key.
  Invalid live arguments perform no write and leave the old period unchanged.
  This rejection policy is a safety choice, NOT verified native error behavior.
- Existing `Unit::listener_driver` is byte-for-byte preserved: TIMER_MS waits in
  microseconds; TIMER_BEAT reads the current host quarter duration each period;
  zero periods poll until re-enabled. Driver SHA-256:
  `d5904a3c513abb9632990ccb4a7879dae76cf3330ebac4869982747de473a8f8`.
- Unchanged `src/lib.rs` timer wiring: lines 1255..1307 create per-signal bodies
  and call `listener_driver` for timer keys in the post-init model; lines
  1492 onward initialize the same store keys. Live updates do not invent a
  driver for a signal absent from that post-init model.

The exclusive shared spans are the listener builtin arms in eval/lower, the
listener check/helper in sema, and the listener fixture in `tempo_waits.rs`.
Unrelated formatter changes were removed. Root source, old ledgers, all
manifests, official-docs/sections.json, lib wiring, and transport dispatch were
not modified.

## Checks and tests

`checks.json` records the Python source assertions, an independent 144-case
boundary model, unchanged driver and manifests, and seven authored Rust tests
in `crates/sampler-ksp/tests/listener_contract.rs`. The Python model does NOT
execute the Rust implementation or prove the emitted VM instructions.

The Rust source tests cover registration bounds and supported signals,
nonconstant init arguments, direct/function caller contexts, zero disable,
change availability in every supported callback, live store preservation on
invalid values, and refusal to create unsupported signal keys.

`tests/tempo_waits.rs::beat_listener_reads_current_tempo_each_period` preserves
its disabled-then-enabled host-tempo scenario using a valid init registration,
init `change_listener_par(...,0)`, and persistence `change_listener_par(...,4)`.
Existing `tests/params.rs::timer_listener_plays_notes_on_its_period` and
`tests/listener_liveness.rs::preempted_listener_driver_keeps_ticking_after_one_second`
are unchanged. All these Rust tests are **NOT_RUN / NOT_COMPILED**.

Executed: Python `source_check.py`; installed rustfmt 1.99.0 syntax parsing via
`--edition 2024 --emit stdout --config skip_children=true`; `git diff --check`.
No cargo, rustc, clippy, build, test executable, native app, bank reader, host,
performance job, install, publication, nested agent, or schedule was run.

## Remaining gates

- TRANSP_START/STOP dispatch is a separate gate. This patch only validates their
  init registration; it neither changes nor claims transport dispatch.
- Native behavior, including invalid-input diagnostics, is **UNKNOWN**.
- CPU AND RAM against both references: **UNACHIEVED**.
- Direct `restore_initialized` trusts a caller-supplied cached model and does not
  re-evaluate dynamic init arguments. No cache API change is claimed. The normal
  Kontakt product-cache path hashes `../sampler-ksp/src` in
  `crates/sampler-kontakt/build.rs`, includes `KONTRA_IMPORT_HASH` in its stamp,
  and rejects a mismatching stamp in `src/cache.rs`. Thus source changes are
  represented in that invalidation contract, but rebuilding/loading was NOT_RUN.
- No native parity or executable validation is inferred from source completeness
  of the fresh compile / live-update checks. Do not automatically merge.
