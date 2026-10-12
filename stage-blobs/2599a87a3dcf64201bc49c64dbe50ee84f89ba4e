# Cold-head reload: bounded source slice

Source commit: **618c7fdcf9d224fa6d62e8774d52b12a91912ee0**. Parent: `108a6ee9f29ce611e1d307d11e9ba65bb831e34f`. Only `crates/sampler-kontakt/src/stream.rs` changes in that commit. Status: **source-ready for the combined batch; uncompiled and unrun Rust, no performance acceptance**.

## Bottleneck and complete change

The preceding implementation scans every asset's `Pcm::head_bytes()` inside each cold-asset iteration. `head_bytes()` also collects retired snapshot generations. With N assets and C cold requests, the residency-accounting traversal is O(N × C), in addition to source IO, packing and per-range work.

The slice scans current head bytes once, lazily on the first eligible cold request. After each successful publication it subtracts the previous head's packed bytes and adds the newly loaded head's actual packed bytes. Admission still reserves the same raw stereo frame estimate. A replacement is not charged twice; successful compression does not consume the raw estimate forever. Error classification and cold retry behavior are retained. The estimate and snapshot now sum with saturation instead of wrapping on overflow.

A per-Streamer control-only mutation guard is shared by the background reloader, explicit reload, and trim. It spans source reads and head publication, so another reload cannot admit from a stale snapshot and trim cannot invalidate accounting mid-batch. The total is local to one pass, not a persistent cache that becomes stale after errors or trim. Audio only marks cold assets/unparks workers and reads immutable snapshots; it never takes this lock. Decode page workers retain their existing separate endpoint lock and priority behavior.

Residency-accounting traversal becomes O(N + C), plus per-range/packing/IO costs. No added per-asset table or audio-thread allocation. One Arc-backed mutex is added per Streamer; this is not a claim of lower measured RAM. A long storage read can delay a control-side trim until the batch releases the guard. Live lifecycle/load measurements must assess that trade-off. Direct external `Pcm::set_ranges` writers are not serialized by this guard; inspected product call sites publish through this Streamer after initialization. Asset slices supplied to the existing public APIs still define their accounting scope; no new global cross-part budget is claimed.

Exact source spans at the source SHA:

- `Streamer::head_mutation`: lines 500–501.
- `Streamer::start`, guard creation/sharing: lines 657–693.
- `Streamer::trim`, guard before accounting: lines 708–732.
- `Streamer::reload`, same guard: lines 737–746.
- Free `reload`, lazy snapshot and replacement deltas: lines 748–807.
- `load_ranges`, existing publication/packed-byte return: lines 478–488.
- New test fixture and four regressions: lines 966–1142.

## Native and v1 evidence: scope is limited

The [NI Kontakt manual, DFD Tab](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/classic-view#dfd-tab) was fetched and read in this round. **DFD Preload Buffer Size** defines per-sample RAM portions for immediate playback in DFD groups; NI advises increasing it when DFD drop-outs disappear in Sampler mode. **Background loading** explicitly warns of artifacts when notes play before loading completes. The new slice keeps KONTRA's existing preload/page fallback policy; it does not implement NI's entire per-group DFD setting or background-loading policy.

Read-only historical native evidence was also inspected: `t3code-80fe786b/artifacts/ni-file-records-2026-10-08/{check-results,sources}.json`. Original evidence is pinned in `docs/re/NI_FILE_BINARY_RECORDS.md`: Kontakt standalone 8.13.1 SHA256 `0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`, public reader VA `0x140d0d4b0`, DFD preload word at serializable receiver `+0x21b60` (document line 113). The original machine-code receipt passes its narrow serializer checks and explicitly says proprietary execution and full audio parity are false. It does not establish native reload locking, budget enforcement, scheduler behavior or runtime CPU/RAM. Its VST3 payload identity is distinct and its reader addresses cannot be substituted for standalone addresses.

REA `current_document` returned `target_unavailable` (no active app). Rather than initialize a new large analysis or execute an official host, this round reuses the existing immutable native receipt plus the current authoritative DFD documentation as corresponding static/documentary evidence. [The reference JSON](pi-head-reload-reference.json) retains exact paths/hashes, official section excerpts, source spans and limitations. No Wine, official-reader process, third-party library opener, sample/key/decrypted dumps, or reference-worktree modifications occurred.

Our frozen v1 `0cb7a8a0b4d43086596a64c77320caa1b26d6d98:src/engine/residency.rs:324–343` updates per-sample bytes using `new - previous` only after the audio-side head swap is accepted. Its file SHA256 is `ec607a65429ad2ec14340475f9dc0f7e3ae8b1385dc98924fea8333f610b129a`. This informed accounting inspection, not a port of its complete adaptive residency/audio-swap policy. v1 bank preload planning and RAM fallback remain distinct source behavior. No v1 timing was collected here.

## No-build checks completed

- `git diff --check`: PASS.
- Direct `rustfmt --edition 2024 --check crates/sampler-kontakt/src/stream.rs`: PASS (syntax/format only, not type checking).
- Source invariants: shared guard reaches the reloader, explicit reload and trim; lazy whole-asset snapshot exists once; packed replacement delta is applied after success: PASS.
- Independent Python arithmetic model: **638,976 steps**, including unique assets, duplicate aliases, nonzero previous heads, replacement growth/shrink and budget boundaries: PASS. Unique-asset admissions match repeated scans; duplicate aliases remain conservative against actual unique stored bytes. This model does not execute Rust or certify concurrent/runtime behavior.
- Existing `python3 tools/kontra-gate/check-editor-cycles.py`: **5/5 PASS**, including missing/invalid/reset UI20 rejection and named deltas. These are the existing numeric transport fixtures, not tests of the new reload code.

Four Rust regressions are authored but **not executed**: packed replacement accounting over one cold batch; transient read failure and post-trim refresh; controlled blocked read/publication versus trim; concurrent reloads sharing one admission budget. Test readers wait on bounded channels, not performance-sensitive sleeps. No stale binary is used as evidence for new source.

## Combined-batch integration request

`618c7fdc` has no semantic UI20 dependency; apply its narrow stream-file patch to the combined candidate. The current integration base already contains a8c's pinned wavetable residency changes. Preserve every pinned-wave field, parameter, preload condition, budget check and trim exclusion. The new test-only `reload_fixture` constructs a Streamer directly: on integration, initialize its existing `pinned_wavetables` field with `HashSet::new()`. This is the only required adaptation identified by the source comparison.

The integration owner performs one combined build/run cycle, freezes the candidate/source manifest and maps each test binary back to that candidate. Do not create a separate per-lane compilation cycle. After building the combined sampler-kontakt test binary, run these filters directly against that binary:

```text
stream::tests::reload_
stream::tests::concurrent_reloads_share_the_same_admission_budget
stream::tests::lazy_heads_are_not_read_until_requested_and_respect_the_budget
stream::tests::trimming_purges_idle_heads_until_within_budget
stream::tests::random_access_reads_match_a_full_decode
stream::tests::wavetable_
```

Also retain the combined root no-run gate and relevant existing streaming/cold-offset/paged-render coverage. Native real-instrument admission needs matched cold/warm dense-chord/onset runs, PCM/underrun checks, loaded/peak RSS and audio/control CPU on frozen v1 and this exact candidate; the comparable Kontakt measurements remain separately unavailable/UNKNOWN. Contended timings stay UNKNOWN.

**Acceptance target remains UNACHIEVED:** significantly lower CPU and RAM than BOTH frozen v1 and Kontakt has not been established. This slice is not a new percentage threshold, native-readiness verdict, version release, publication or install.
