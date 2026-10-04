# Worker terminal publication and journal ordering

Static review found that the worker thread finalized its diagnostic trace before
calling `finish` after `run` returned or panicked. Trace records use a bounded
journal sender whose waiting send can block. An exited worker could therefore
remain advertised Ready, with its original failure unpublished, while terminal
logging waited. Requests could continue accumulating and produce a secondary
capacity failure. This is a source-order finding, not measured attribution of
the user's Piccolo or Alto Flute failure.

The correction first finalizes initialization timing and copies the observed
phase/frame under the Details guard, then drops that guard. It calls the existing
`finish` before any terminal trace operation. The original failure, error counter,
terminal status, UI invalidation and bounded queue cancellation retain their
existing ownership. Terminal tracing uses the retained cause and phase scalars;
the final load report is installed afterward. No Details guard remains held
through these terminal journal sends.

The original failure is installed before the Release publication of Failed.
A finalized load report can arrive later than terminal status; it is separate
from the preserved first cause. Concurrent counters remain observations, not
an atomic fault census. The change does not enlarge queues, change event order,
establish render speed or provide a production realtime deadline guarantee.

Independent review checked the source order and existing `finish` behavior.
A private authored harness with a blocking LoadTrace facade is prepared but
unrun; that facade does not exercise the actual diagnostic journal. Compilation,
real-journal backpressure and plugin playback verification remain pending under
the CPU restriction. Blocking inside `run` or startup tracing, and waiting for
journal completion when joining the worker, are outside this correction.
Installed binaries remain unchanged.

## Diagnostic report ownership

The same Details mutex is used by worker phase and runtime-snapshot publication.
Previously, control-thread `diagnostic_report` serialized the complete parsed
graph and retained reports while holding it. Later source retains immutable
Arc owners and scalar observations under the guard, then serializes graphs,
Lua metadata and report JSON after releasing it. Failure text also has an Arc
owner; the public `private_failure` API still returns the complete owned String,
but makes that text copy outside the guard. Failure content is not truncated.

The initialization summary and fixed diagnostic-string references remain under
the guard; current production initialization has a fixed stage sequence. Report
field names, optional cache fields and null behavior are preserved. Status and
counters are concurrent observations sampled before serialization, not one
transactional audio-frame snapshot. This is a source ownership correction,
not a measured scheduling or playback improvement. Final combined compilation,
schema comparisons and runtime inspection remain pending.
