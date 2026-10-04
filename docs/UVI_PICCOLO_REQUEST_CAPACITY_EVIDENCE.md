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

No queue-size increase, request dropping, event reordering, new render
algorithm or CPU improvement is established by this evidence. Fixing this
Piccolo playback failure requires verification after the user lifts the CPU
restriction. No build, playback replay or timing run was performed for this
report; installed binaries remain unchanged.
