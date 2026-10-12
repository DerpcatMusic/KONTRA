# Piccolo request-capacity failure

The user's 2026-10-04 screenshots show `Piccolo 2`, member
`Presets/Piccolo 2.uvip` in `VWinds-Piccolo.ufs`, failing during playback with
`Bridge(RequestCapacity)` at `process`, reported callback frame `78464`.
The screenshots do not identify the binary's exact source revision.

The displayed loader snapshot reports 27 configured pending packets, worker
status `ready`, zero recorded worker errors, mean recorded render wall time
3.95 ms, maximum 32.56 ms and a 5.33 ms budget per 256 frames. It also shows
37 over-budget render attempts, 382 blocked submission attempts and 278
completed packets. These are cumulative concurrent counters observed after
the endpoint fault, not an atomic census taken at that fault.

## What is established

- The audio bridge exhausted its bounded pending-request storage. This error
  does not report a resource-decryption or program-parsing failure.
- The displayed mean is below the packet budget. Neither that mean nor the
  maximum separates rendering CPU usage from worker scheduling delays.
- The endpoint frame is captured at the caller segment's start. It must not
  be presented as the bridge's exact packet-seal frontier or used with later
  worker counters to infer an exact backlog.
- Missing browser artwork and flat preset listings are separate source
  defects: both artwork scan paths exclude UVI, and the selected-bank preset
  path ignores member folder structure.

## Changes and verification scope

Independent captured endpoint causes now remain the primary report reason
when a later worker snapshot also contains an error. Worker failure text is
still used when the endpoint reports worker failure/stoppage, or when no
endpoint cause was captured. Full worker evidence remains in the report.
The prepared regression is uncompiled and unexecuted.

Grouped Logs follow the same precedence as the loader and Info. A different
worker failure is retained once per unique child as supplementary evidence;
volatile frame and statistics fields do not create duplicate children. Exact
`Bridge(Worker(Failed))` and `Bridge(Worker(Stopped))` symptoms still use the
original worker cause when available. These source changes do not rewrite raw
records or prove that a later worker observation caused the captured endpoint
fault. Prepared grouping/source-context regressions remain unrun.
When every child has the same primary cause, grouped details print it once above
the unique items. Mixed causes remain attached to their respective children.
Already-labelled Lua location fields are omitted from the display's JSON clone;
supplementary failures, real excerpts and original records are retained.

The bridge also makes one bounded service retry when its pending ring is full
at packet sealing. This permits room that became available after the earlier
service poll to be used, preserving FIFO order, the completed packet and the
original capacity/latency. Persistent pressure still aborts. Authored real-queue
fixtures are prepared but unrun; their Ready status is a fixture, not proof of
native readiness or sustainable throughput.

An optional failure snapshot retains the bridge's own frame, partial packet,
submitted/received/discard/consumed frontiers, latency and local queue counts
through the existing atomic first-cause publication. Those values are frozen
after the bridge abort and are separate from the reported caller frame and later
worker observations. Direct faults or panics without an owned bridge abort can
have no frontier snapshot. This is not a census of concurrent worker queues.
Info and Logs expose the retained evidence; neither infers CPU ownership.

No queue-size increase, request dropping, event reordering, new render
algorithm or CPU improvement is established by this evidence. Fixing this
Piccolo playback failure requires verification after the user lifts the CPU
restriction. No build, playback replay or timing run was performed for this
report; installed binaries remain unchanged.
