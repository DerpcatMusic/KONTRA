# UVI loading order and partial progress evidence

Reviewed 2026-10-04. Private source-direct probes used source snapshot
`2b13e77` and existing cached dependencies. The experimental loader remains
private and is not part of the application.

## Retained-session experiment

The existing worker prepares initial static resources before constructing Lua.
A private fresh-load experiment instead constructed one hosted `Session` with
real bank resource authority, observed an owned UI snapshot, prepared static
PCM, merged aliases into the same resource cache, and constructed `Renderer`.
It retained the same Lua VM; it did not run `onInit` twice. The cache merge
validated unique PCM residency and alias occupancy before committing, shared
PCM by exact archive record identity, and retained existing dynamic aliases.
No snapshot was published to the application and no controls became active.

For two actual Clarinet revisions, explicitly seeded local baseline/candidate
runs matched owned initialized UI, pending musical/native command order,
resource request/response order, first 512-frame audio bit patterns, command
counts, logs and host completions, saved-state bytes and post-save pending
commands. Final merged alias counts were **199 / 197**, with **218,923,526 /
317,220,848 resident PCM bytes**, respectively. The same retained VM continued
through rendering and saving. Restoring those states used the unchanged
original restoration path in both probes and matched its outputs and resource
order. This does not prove a reordered restoration path, later arbitrary UI
requests, endpoint adoption, realtime behavior or native engine fidelity.
The explicit seed controlled the local shared CRT RNG; it establishes no
cross-platform RNG equivalence. These successful cases do not establish a
safe general reorder.

## Executable counterexamples

Authored archives contained real decoded WAV resources. A private **128-byte**
aggregate budget made production's resource-admission arithmetic observable
without allocating hundreds of MiB. The original aggregate limit remains
**512 MiB**. Four independent contracts failed:

| Contract | Static-first baseline | Early Lua experiment |
| --- | --- | --- |
| Aggregate resource task outcome | Static PCM uses 80 bytes. A 64-byte dynamic load exceeds the budget; its authored failure is handled, and the Player becomes Ready. | The dynamic task succeeds with 64 bytes. Final static merge would need 144 bytes and fails; the Player never becomes Ready. The failed merge leaves its prior 64-byte cache intact. |
| Static failure before callbacks | Corrupt static audio fails before Lua; no dynamic resource call occurs. | A dynamic call and successful completion occur before the same static decoder failure. A read-only snapshot has already been observed. |
| Cancellation before authored work | A stop observed during initial static preparation cancels with zero dynamic calls and no Ready Player. | A stop after the early snapshot cancels with one completed dynamic call and no Ready Player. Both report cancellation, but their preceding work differs. |
| Original first failure | Corrupt static audio is the first failure. | With an additional authored `onInit` failure, that Lua failure wins before static decoding occurs. |

A separate corrupt dynamic resource case preserved handled task failure and
Ready status in both orders. It does not repair the four counterexamples.
Resource metadata alone cannot prove exact retained PCM size, decode integrity
or these task/failure ordering contracts.

## Current behavior and alternative

Production retains static preparation before authored initialization. Fresh
initialized UI observation follows successful Lua initialization; restoration
observation follows the prevalidated renderer prefix and authored restore
commands. Controls require successful audio endpoint adoption. Progress shown
before then is loading evidence, not an initialized interactive instrument.

Info reuses the bounded `WorkerLoadActivity` snapshot: current phase, completed
stages, parsed nodes, distinct statically rejected nodes, initial resource
paths completed/total, unique decodes and retained decoded bytes. Unknown
inventories remain pending. These initial counters exclude later script-loaded
resources. Logs retains grouped causes and locations; no synthetic preview
Session or duplicate diagnostic report is introduced for progress.

A versioned decoded-PCM disk cache is under private investigation as an
alternative that preserves static decode/admission before Lua. It requires
independent content identity, exact metadata/PCM/budget checks, corruption
fallback and resource/failure-order evidence before any production change.

Source contracts: `Library::samples_with_progress_cancel` and `BankResources`
in `src/uvi/library.rs`; `Player::new_inner` in `src/uvi/player.rs`;
`SavedState::prepare_audio`/`prepare_renderer` in `src/uvi/state.rs`;
`Session::new_hosted_program_chain_with_state` in `src/uvi/script.rs`;
`Worker::load_activity` and `run` in `src/uvi/worker.rs`.
