# Live load failures, 2026-10-04

Read from the user's retained local journals after installing clean source
`cc54c675706277a75de71b68029b08b5a473592a`, build `c01b223593a9516e`.
This investigation only read logs and source; it did not replay audio, decode
another bank, rebuild, restart the host or change settings.

## Augmented Orchestra: the selected program cannot execute yet

`Presets/04 Hybrid/Aesthetic Dune MW.uvip` failed in `uvi_preflight` at frame
zero. Bank opening and program decoding completed; sample loading, Lua
initialization, sound preparation and rack installation were not reached.
This particular error is KONTRA's playback admission check, not an activation
error or evidence of failed program decryption. Browser indexing is separate
from loading a playable rack instrument.

The retained graph reports 97,046 nodes, 76,310 connections, 6,984 sample zones
and one script processor. Its inventory identifies 787 distinct rejected nodes:

| Feature | Rejected nodes | Remaining execution boundary |
| --- | ---: | --- |
| CombFilter | 389 | Connected controls and graph/property lifecycle |
| MS20 | 388 | Hosted controls, smoothing, tracking and complete processor integration |
| Flanger | 4 | Independent controls and connected modulation |
| StepEnvelope | 1 | Unverified Smooth setting at node 10 |
| MultiLFO | 1 | Native source evaluation, connected clocks, events and random-state ordering |
| DiodeClipper | 1 | Gain, reset, hosted controls and full callback lifecycle |
| FeedbackMachine | 1 | Unsupported processor/control source |
| Drive | 1 | Hosted smoothing, modes, oversampling and voice lifecycle |
| Layer | 1 | Unsupported nondefault PlayMode |

These counts were reconstructed from the retained typed node chunks and agree
with `static_rejected_nodes`. Their scope is per-node checks plus the first
control-graph construction failure; they are not an exhaustive census of every
unsupported property. Node 13 is MultiLFO, but fixing that node alone cannot
admit this program. Initially bypassed nodes also remain checked because
instrument controls can enable them later.

The implementation boundary starts at `ProgramPreflight::validate` in
`src/uvi/playback.rs`; worker initialization retains the structured graph in
`src/uvi/worker.rs` before enforcing that check. Isolated native helper
comparisons do not establish connected whole-program execution. The playback
solution is to implement and verify the missing source, effects and lifecycle
paths, then admit their verified scope. Removing the checks would conceal
missing sound behavior rather than supply it.

[UVI's Step Envelope explanation](https://support.uvi.net/hc/en-us/articles/360001201418-Learning-Falcon-206-Using-Step-Envelopes)
describes smoothing between steps and separately describes Spline interpolation.
This corroborates that the rejected Smooth control has audible meaning; it does
not specify the native smoothing kernel or clock arithmetic. Documentation alone
does not justify treating it as zero or opening its execution gate.

## Why the failure message and Logs were misleading

`prepare_uvi` in `src/plugin/uvi_load.rs` unconditionally appended "the current
instrument is still playing", even with an empty rack. The source correction
removes that claim from both loading and failure messages.

`ProgramPreflight::validate` serialized every rejection into one error string.
The retained staging event was 371,377 bytes before abbreviation. Its data
became only `diagnostic_truncated` and `original_bytes`, losing the member and
generation needed for reliable association. The same failure was reported by
the loader issue, loader completion and staging notification. They are three
reports of one rejected load, not three separately proven DSP crashes.

The integrated source corrections use a bounded count/kind summary, retain
the existing typed per-node causes, carry the exact worker load ID and stage
into staging notifications, and wrap Logs text within the available width.
Association must use genuine identity; older abbreviated staging events cannot
be joined to other attempts merely because their bank and reason look alike.

The single warning in the Augmented Orchestra session is
`support/previous_session_unconfirmed`: an earlier session ended without native
crash confirmation. It is not another program-decoder or DSP rejection, and
does not itself establish that KONTRA crashed. The evidence remains local.

The recorded attempt finished in 1,630.68 ms. Its recorded stages were bank
opening 432.51 ms, program decode 260.03 ms, diagnosis/preflight reporting
881.47 ms and preflight failure reporting 56.56 ms. These are that attempt's
retained wall-time measurements, not a new benchmark or general loading claim.

## Oboe: a separate failure after initialization

The next journal records `VWinds-Oboe_V2.ufs`, `Presets/Oboe.uvip`, reaching
playback before failing with `Bridge(RequestCapacity)` at the reported process
frame 95,360. This failure belongs to the host/worker audio bridge, not the
Augmented Orchestra feature checks.

The actual capacity rejection is the bounded ring's `push` in
`src/uvi/bridge.rs:84`, reached by `seal_partial` after its bounded service
retry. The displayed excerpt at installed `src/plugin/uvi.rs:859` is the
caller propagating that bridge error; it does not identify a native DSP
processor that crashed.

The audio-owned first-fault frontier retains 27 pending requests in a ring
with capacity 27 and no prefetched audio. Submitted and received frontiers
were 88,320 and 86,016 frames; the current playout packet began at 90,624.
Completed delivery was therefore 18 native packets (96 ms at 48 kHz) behind
that packet. This describes a delivery backlog, not its CPU or scheduler cause.

The later worker observation was Ready with zero recorded errors, one active
voice, 338 completed packets and 32 over-budget attempts. Recorded render wall
time averaged 4.20 ms and peaked at 44.56 ms against a 5.33 ms packet budget.
These later concurrent counters are not an atomic snapshot of the fault.
The timer excludes several service operations, so the mean does not establish
sustainable throughput. No specific processor or CPU contention is proven to
have caused this fault.

Static queue arithmetic is consistent with the retained frontier; no duplicate
packet or wrong-stamp cause is established by this evidence. The next playback
solution requires identifying and reducing worker service delays while
preserving control timing and audio behavior. Increasing queue capacity alone
does not establish a throughput fix. See
[the render-timer and convolution boundary](UVI_PACKET_COST_BOUNDARY_EVIDENCE.md).

## Verification boundary

Later static source work adds these narrowly scoped corrections:

- `6390be3` fences native panels against the full loader context. A restore of
  new state from the same bank/member no longer qualifies the retained old panel
  merely by source and captured generation. Failed/unadopted inspection remains
  available when its context still matches; downstream edit admission is unchanged.
- `2b108ff` releases displaced UI/state/runtime snapshot owners after unlocking
  the worker's phase/mailbox mutex. The same request IDs, stop gates, publication
  and genuine state-capture errors are preserved. Controller-side destruction
  can now overlap new worker snapshot work, so identical peak lifetime or cost
  is not claimed. Normal service already avoided unrequested full-graph scans.
- `31bb99e` iterates the immutable absolute-control declaration order by copied
  node ID instead of cloning its Vec whenever the per-frame loop is reached.
  Visit order, validation, arithmetic and publication are unchanged; other
  control allocations remain.
- `304c223` caches nine phase/table points for the existing admitted fixed,
  global Step Envelope projection within a logical 256-frame block. Complete
  projection inputs identify the cache; numeric writes invalidate it and the
  original scalar gates and queried phase-overflow check still run. Other block
  widths retain the previous path. This is reuse of the current projection,
  not proof of native host-generation mapping or smoothing support.
- `5970da8` validates final owned PanLaw scalars at the sampled constructor/save
  boundary, after the existing authored work and command application. Transient
  writes and saved prefixes remain admissible when repaired at that boundary.
  All original Program/Layer/Keygroup owners, including inactive ones, must have
  a supported scalar of zero or one; future and wait-suspended callbacks are not
  fully executed by this check. Errors identify the actual node, kind and
  parameter without echoing invalid retained text. Existing per-launch channel
  restrictions and native setter/clamping uncertainties remain unchanged.

Each correction passed independent static review. None establishes a measured
throughput improvement or the cause of the Oboe delivery backlog. No new feature
gate is opened, and none admits the rejected Augmented Orchestra program.
Prepared Step cases compare every-frame output and failure timing with the
explicit previous scalar path, including transport changes and overflow; PanLaw
cases exercise actual Session ordering, restore repair and error privacy. These
authored cases are uncompiled/unrun, not successful functional verification.

Installed binaries remain `cc54c67` during this investigation. Source commits
`d7384b9`, `e4a674a` and `6a13afb` correct the playback claim, bound the preflight
summary and retain exact failure identity, and wrap/group Logs respectively.
The summary and layout changes passed independent static review. Authored
tests, compilation and editor verification remain pending under the CPU
constraint; no new build, tests or playback runs are claimed here. Neither
failure has been shown resolved by the installed checkpoint, and complete
Falcon compatibility remains unimplemented.
