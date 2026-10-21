# M5: group residency and dormant definitions

Package `purge`, based on `gpt/topology` (`2a231791`), 2026-10-13.

## Contract and evidence

The official [KSP Group Commands](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/group-commands)
sections `purge_group()` and `get_purge_state()` define unload/reload modes 0/1,
loaded-state readback 0/1, and a returned asynchronous command identity.
[Callbacks: on async_complete](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/callbacks)
defines delivery after execution, `$NI_ASYNC_ID` as the initiating command's ID,
and `$NI_ASYNC_EXIT_STATUS` as success/failure 1/0. Sections fetched on 2026-10-13.
The documented allowed contexts are UI-control(s) and persistence-changed callbacks;
Multi scripts cannot use this instrument operation. This package does not change
KSP operand/context lowering; the separate operand-fix package must be integrated.

Binary: read-only `/mnt/Windows11/DEV_WORKSPACE/re-binaries/Kontakt8.exe`,
SHA-256 `0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`.
The retained Ghidra `kontakt-engine/vtable-methods.csv`, rows 50360–50361,
identifies `BSamplePoolItem` at preferred VA `0x144e97bd8`, pointing to
`0x14081e0c0` and `0x140993f40`. Direct `r2` `pxq 16` confirmed both pointers.
The [topology evidence B4](KONTAKT_TOPOLOGY_EVIDENCE.md) also identifies the named
`BSamplePool::AttachSample(BZone&, BSamplePoolItem*, SyncedThreadingJobManager*)`.
These are **pool/job identity evidence**, not a recovered native purge algorithm.
The recovered full pseudocode remains unavailable. Direct `r2` `izz` also found
purge-specific context diagnostics at `0x144e89e58` (Multi-script restriction) and
`0x144e89e90` (UI-control/persistence restriction), corroborating the public contract.

Reproduction:

```sh
r2 -q -e scr.color=false -e bin.relocs.apply=true \
  -c 'pxq 16 @ 0x144e97bd8; izz~purge_group; q' \
  /mnt/Windows11/DEV_WORKSPACE/re-binaries/Kontakt8.exe
```

`0cb7a8a0:src/engine/script.rs` was checked for the existing script/engine boundary;
no reusable group-residency mechanism was found in that bounded inspection. The
implementation reuses v2's counted-reader snapshots, existing async callback/ID
service, source recipes, parked workers and page cancellation protocol. No native
pseudocode, proprietary binary or library material is copied into this package.

## Implementation decisions

Residency is independent of attenuation. Group selection is gated before voice
allocation, including release selection and cold-start holding. A purge request
retires that group's existing voices in the addressed plan, cancels unowned page
requests, and releases that group's asset references. The worker removes full PCM,
preload ranges and mipmaps only when the last group/ungrouped-region/plan owner is
gone. The fixed reusable page pool keeps its capacity; its sample contents are
invalidated/recycled, rather than freeing audio-owned buffers on the render thread.

Full PCM now uses a borrowed snapshot guard, like streamed heads. Sample destruction
and decoder I/O occur off audio. A purge's successful completion waits until retired
sample generations are reclaimed, so a held reader cannot cause premature success.
Reload keeps groups unavailable until worker completion is admitted at a render
boundary, including a zero-frame render. Stale completions cannot reopen a newer
purge. Superseded operations report failure; failed multi-asset reloads roll back
partial residency before completion. Mode and revision share an atomic state so
failure rollback cannot consume a newer request's reference.

Each accepted request uses the existing instrument/plan async-ID domain and retains
its originating script instance. Completion invokes the existing `async_complete`
callback and async-wait resumption; it does not fabricate an early callback in the
request handler. Storage recipes retain reopenable source metadata, not a second
copy of sample buffers. Both streamed and eager Kontakt loaders install recipes.
Memory-only assets supplied by another frontend must install their own decoder if
they need reload after purge; missing decoders produce an explicit failed completion.

Saved mute retains groups, zones, source identities, limits, inserts and modulation
at translation. `Group::muted` lowers to a dormant selection gate; live
`Runtime::set_group_muted` restores eligibility and keeps existing attenuation
behavior for live mute. Purge does not globally enable script gain rendering.
The public muted-state setter takes a dense group index; the KSP purge operation
resolves physical source group indices first.

The immediate retirement of playing group voices is an explicit implementation
policy required for the requested zero voice-kernel work. Native in-flight voice
and purge timing has not been measured. No claim is made that every native tail or
same-frame callback ordering rule has been recovered. Instrument/bus tails and
scheduled callbacks can continue independently of purged group voices.

## Focused witnesses

`sampler-core --test purge_residency` covers:

- purged voice retirement and rejected new starts, with a no-heap request;
- full PCM plus mipmap release, reload audio and exact returned/completed ID;
- shared assets within a plan and across retained plans, including last-owner retirement;
- borrowed PCM delaying completion until reclamation;
- an in-flight reload superseded by purge;
- failure status and rollback of partially reloaded multi-asset groups;
- streamed preload release/reload, stale in-flight page completion and cold-start rejection;
- saved mute/unmute preserving sample offset, loop, physical group ID and pre-amp chain;
- physical source addressing and invalid-index rejection without integer truncation.

Independent synthetic byte counts: 128 stereo f32 frames occupy 1,024 bytes;
full PCM plus four mip levels occupy 1,984 bytes, then zero after purge and
1,024 after reload (mipmaps rebuild on demand). Shared ownership retains 1,024
bytes until the last owner leaves. A constant packed mono head takes 256 bytes,
then zero after purge. These are residency counters on synthetic assets, **not
real-library RSS or CPU measurements**.

Failing-first direction was checked by restoring the old attenuation-only
`Op::Purge`: the voice-retirement fixture observed 1 active voice instead of 0.
Restoring the destructive saved-mute branch made
`library::modulation::fx_decode_group_insert_errors_reach_the_ir_report` fail
its retained-content assertion. Removing failed-reload rollback made its focused
fixture retain 1,024 bytes instead of zero. These were behavior ablations in this
worktree, not a claim that new APIs compile on the untouched baseline.

`sampler-kontakt` also has a paused cold-reloader fixture proving a completed
purge cannot be undone by an older cold I/O read, and an actual `Streamer::start`
fixture checking installed decoder recipes restore authored disjoint head ranges. Existing malformed-FX diagnostics
remain present for muted groups because those definitions are now translated.

## Remaining work and integration

T14 is deliberately not bound to head sizing: the stored DFD field is identified
at native receiver offset `+0x21b60` (see `NI_FILE_BINARY_RECORDS.md`), but its
byte/channel units and effective override law are still unverified. Preserve it
until a measured setter/consumer establishes those units.

The final combined build, library render, RSS and CPU measurements belong to the
coordinator under the user's 2026-10-13 **NEVER BUILD** directive. This package runs
only focused crate tests/checks with `kontakto-heavy` and `-j 4`; no workspace,
`--no-run`, release, plugin or corpus run. CPU acceptance here is structural: no
active group voices or source/FX kernels after purge; numeric performance remains
unverified. Measure Afflatus mic and Conflux layer toggles once after integrating
the independent KSP operand fix, including purge/reload while notes are playing.

Shared-file integration hunks: `library.rs` removes the destructive early return,
sets `muted` on the retained IR group and updates its existing fixture; `effects.rs`
adds `muted: false` to one complete test initializer. `src/sound/waveform.rs` borrows
the new PCM guard in its repeated bin loop. `behavior.rs` and `dsp/lanes.rs` have no
net changes; the WIP formatting churn is removed by the finishing commit.

## Final validation receipt

All calls used `/home/derpcat/.cache/kontakto-heavy` and `-j 4`:

- `cargo test -j 4 -p sampler-core --test purge_residency`: 12 passed.
- `cargo test -j 4 -p sampler-kontakt --lib library::modulation::fx_decode_group_insert_errors_reach_the_ir_report`: 1 passed; the saved-mute ablation failed as described above.
- `cargo test -j 4 -p sampler-kontakt --lib stream::tests::ksp_residency`: 2 passed.
- `cargo check -j 4 -p sampler-kontakt`: passed, including core/IR/pool/KSP dependencies, with compiler warnings retained.
- `git diff --check`: passed.

No real-library playback, native reference recording, CPU/RSS benchmark, full suite,
plugin build, workspace build, `--no-run` or release build was run by this package.
