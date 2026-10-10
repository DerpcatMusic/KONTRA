# Perf round 4: combined verdict and comparable CLAP admission

Base/previous package: `38322e9589d2c88da11eaff3e28d2a5a585d3cea`, including Python source `21bcdde3b545c20571ed3475b5285e6575119fce`. Own new checkout: `/home/derpcat/.t3/worktrees/KONTAKTO/pi-perf-admission-round4`, branch `pi/perf-admission-round4`. Previous delivery and frozen integration source are untouched. This document supersedes only the **PENDING 618 combined receipt** statements in `pi-perf-acceptance-next.md`; its workload/safety/onset/RAM limitations still apply.

## Exact executed evidence, not performance acceptance

Read and checked the integration receipts at `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/pi-integration-round2`:

- Source `429e7dff3ecb0f53aef32c43201ab796ccc8c28c`; tree `94ad5caaa5e5ec70e1595d34166c25110d294c67`.
- `FROZEN-SOURCE-429e7dff.json` SHA256 `69a158bfb771a8853c89ddd2a00fa5f39e326a147bb1deedb120c18cb338a29c`.
- `FINAL-VERDICT.json` SHA256 `f4f69ade8fb6d2963fa520d78e40adcf91824bf55947e32ba71f5db82d605e15`.
- `test-results.json` SHA256 `3cbe70e59d9334b3cb1b7c529252f121c3b9d6f446f04cdb3981f9afc9005c4a`.
- `compiled-artifacts.json` SHA256 `a47f7877f84520ab11adb9069d98f03e5db52cda9dbcc44798d47c9799413ac8`.

All five hashes were checked against the final verdict where applicable. Relevant compile/stream/UI20 JSON and log hashes were also checked. This is receipt verification, not a rerun of compiled binaries. The combined compilation exited 0 (255.6 seconds); 14 direct runs recorded 118 PASS invocations, 117 unique executed tests, zero FAIL, 43 ignored. The dedicated AR kernel test repeats one executed case. Ignored tests are not verified.

Perf-specific executed tests:

- `stream::tests`: 11 PASS, 1 ignored, binary SHA256 `fff70ad38ce315239a973acb81573281a6f1b276ad05934f63018c73d75e39c4`.
- All four new cases executed: `reload_accounts_for_packed_replacements_throughout_one_batch`, `reload_rescans_the_budget_after_trim_and_a_transient_read_failure`, `reload_serializes_publication_with_control_side_trim`, `concurrent_reloads_share_the_same_admission_budget`.
- Neighbor coverage executed: parallel header order, lazy heads/budget, full-decode random-access equivalence, trim, v1 RAM policy, both wavetable pin/range cases. `authored_polyphony_sizes_new_stream_pools_without_shrinking_the_policy_floor` remains ignored (rejected fixed-pool experiment), not a regression PASS.
- UI20 host 5 / activity 2 / idle 1 PASS; test binary SHA256 `54b9f1bea9eb3c9f800a6141bf544f9c7f9abff560136ac130f5a5b784122abc`.
- Actual UI Session/State warmed allocator checks PASS. This is a bounded owned fixture, not universal zero allocation or native lifecycle proof.

Synthetic product smoke: 384000 finite/nonzero stereo samples, zero runtime-problem fields; functional only. Its example begins the block after MIDI, so its timing does not establish production callback-boundary/onset parity. C++ presented host compiled with `-Werror`, not launched; it is not the CPU host artifact.

At exact `429e7dff`, `crates/sampler-kontakt/src/stream.rs:745–850`, `Streamer::trim`, `Streamer::reload`, and free `reload` serialize head mutation, retain the guard through IO/publication, scan head bytes once lazily, and update packed replacement deltas. For n assets/c admitted cold requests this removes repeated n-asset accounting scans (O(n+c), apart from range traversal/IO). Executed budget/concurrency tests now support correctness. **No measured speedup or RAM reduction follows from this complexity statement.** IO-held trim/retirement delay and native reload/budget concurrency remain UNKNOWN.

## Decisive actual-CLAP plan: BLOCKED until artifact admission

All 168 entries in the current `compiled-artifacts.json` are debug executables: `opt_level="0"`, `debug_assertions=true`, `overflow_checks=true`. There is no comparable release candidate CLAP in this manifest. Do not use the debug scanner, CPU example or smoke executable against a release frozen v1, and do not substitute the newly compiled presented host for `clap-cpu-host`.

**Measurement prerequisite BLOCKED:** integration must select an already-built exact-source **release** plugin + matching CLI + CPU host with trustworthy build/profile/source receipts. If none is available, add that release artifact/profile prerequisite to the next combined batch; no standalone compile cycle is requested. This round does not search or hash unrelated multi-GB artifact trees. Absence from this manifest is not proof that no release artifact exists elsewhere.

Admission order, owned by integration:

1. Pin the chosen current source SHA/tree and plugin/CLI hashes. Plugin-adjacent `BUILD.json` must bind actual path, full source SHA, `profile=release`, plugin/CLI/host hashes and the exact CPU-host source hash. `ci` inherits release but disables ThinLTO (`Cargo.toml:97–100,173–175` at this round's base); that label is not release/profile parity. Debug is not admissible. Do not manufacture a profile attestation from a filename.
2. Verify frozen v1's existing SHA256 manifest when used. The frozen CLAP is **0.3.152**, a distinct artifact lineage from the `0cb7a8a0` scanner/CPU adapters. `~/.cache/kontra-v1/README.md` does not attest the CLAP's compiler profile. Obtain the original release/build provenance from integration; otherwise profile parity is UNKNOWN and the pair stays blocked. Do not rebuild or mutate v1 to remedy missing evidence.
3. Use the **same exact CPU host binary** in both cells, and verify its source digest against `vendor/moose-clap/tests/live_performance.cpp`. Require production-equivalent MIDI/begin-block order and the same callback-thread clock (`CLOCK_THREAD_CPUTIME_ID`, source lines 303–307), peak scan, sample rate, block count, steady window and complete/audible/native readback. Historical W8 begin-after-MIDI examples are diagnostics only.
4. Pin the original `cpu-audit-native.py` workload: program 0/one part/48kHz/4s; Pure piano or Vista 5 Violins or ANALOG STRINGS, unchanged file identity across cells; blocks 32/64/256; exact scheduled CC1/CC11/pedal/notes and 12000–48000 steady window. A scenario label alone does not certify equivalent source/features/routing. Record library/source identity and engine feature coverage; silence or omitted voices cannot count as an improvement.
5. Grant a new quiet window only after all heavy jobs drain. No timing during C++ tests, compiler jobs or other heavy work. Use existing `Activity`, private settings/tmpfs and numeric receipts. Contended cells stay UNKNOWN. Profiled/diagnostic cells are attribution only, not scored CPU acceptance.
6. Run bounded alternating/reversed v1/v2 source-cold pairs using already-built artifacts and the existing driver, with separate output directories. Source-cold requires actual `pages_after=0`. Then unforced-source-cache pairs; do not call those warm without a checked priming witness. Repeat count/window are integration's authorization, not an invented significance threshold.
7. Keep thread CPU, callback wall/deadline/wake misses and underruns separate. Retain spread and exact workload/artifact receipts, not just favorable medians. This driver is not whole-process CPU/RAM evidence. For matched retained memory use existing legacy lifecycle observations, not v2-only UI20 or trimmed load-probe RSS. The fixed 100ms ready warmup is not immediate cold onset.

The prior package's `21bcdde3` identity/cold-page checks are source plus 6/6 no-host fixtures, **UNRUN on actual hosts**. Integration alone owns any expensive execution. No generic `gate.py`/`probes()` (official UVI fallback), Wine, native-reader process, installation, servers, schedules or new collectors.

## Corresponding Kontakt evidence and limits

Re-read the immutable NI DFD/static native reference package from source `3d4c9659a4bec3c6c97126bffb5404a9147a2f50` at `fix-w13-clap-editor-rss/docs/audit-2026-10-10/pi-head-reload-reference.json`; package SHA256 `ee85cad917a4f69afde8a112ad5a33cad0b2dad0dee608b18517bed7d07bd384`. Reverified small original native receipt hashes read-only:

- `t3code-80fe786b/artifacts/ni-file-records-2026-10-08/check-results.json`: `f56987ad0ee1cf1783c0774ea91d503e93852341ebf0d0d23855491f7ac598e2`.
- `sources.json`: `4cfbf652b7aca9e475ac9b2bb8386635dcd52cb1930ab76fdfb487fdf7f419cd`.

The authoritative [Kontakt manual DFD Tab](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/classic-view#dfd-tab), recorded full HTML SHA256 `846a09243ad7d17d8daa92bf5105418501ebb25cd358ba8d09108ffb5d4ae7c4`, documents small RAM sections, preload/drop-out tradeoffs and background-loading artifacts. Existing static native evidence pins Kontakt standalone 8.13.1 SHA256 `0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`, reader `0x140d0d4b0`, DFD serializable word `+0x21b60`. This is justified reuse of immutable evidence, not a new REA/runtime run. It does **not** expose native locking/profile/workload clocks or certify lower CPU/RAM. No new script builtin, importer or DSP behavior is implemented here.

**Lower CPU AND RAM than BOTH frozen v1 and Kontakt: UNACHIEVED.** Kontakt runtime measurement remains unavailable under current safety constraints: UNKNOWN, not PASS. Cold-onset, IO-overlap/trim timing, symmetric total process CPU/RAM and real native readiness/presentation also remain UNKNOWN.

**NEXT:** integration selects/attests the exact already-built release pair or puts its missing artifacts in the next combined batch; perf supplies only the smallest proven driver admission correction and no-host regression fixtures. No changes to the completed `429e7dff` freeze.
