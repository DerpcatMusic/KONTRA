# Initial admission reuse and phase cancellation

Reviewed 2026-10-04. These changes follow installed `817b03a` and remain later
source evidence. They do not establish complete startup, live deadlines or native
instrument fidelity.

An immutable Program's exact static admission result is reused during its
initial worker/Player/Renderer construction. Sample, rate, resource, Lua and
saved-override validation still runs; public entry APIs keep their existing
checks. The retained source snapshot is `384ed21`; no cross-program result cache
or generic worker pool is added.

Actual V2 Clarinet baseline/candidate captures preserve exact PCM and metadata,
aliases/resident bytes, graph report, initialized UI, saved-state bytes,
fresh/saved note audio and phase/UI publication order. The source-direct suite
passes 370 checks with one ignored. Three baseline and three candidate runs,
ordered B C C B B C with OS cache state unchanged, reduce median **worker
initialization wall time from 2,674.840 to 2,591.648 ms (3.11%)**. Separate
whole-helper user/system CPU includes authority/config/report/stop work. Neither
measurement covers UI assets or host audio adoption, and neither is a general
loading-time or realtime guarantee.

A subsequent phase-cancellation change borrows the existing owner stop flag
only during initial preparation. Checks after successful operations can stop
before the next phase, including after Lua success and initialized UI observation
before Renderer construction. Genuine preflight, saved preparation, Lua and
Renderer errors propagate before the later flag check. Cancelled work retains
its graph report/phase, rejects stale generations and cannot publish Ready or
initialized UI after observing the flag.

The combined source-direct checks pass 372 with one ignored, including authored
Lua-error precedence and a stop during the initialized UI observer. Actual owner
stop cases confirm cancellation at the intended phase boundaries and default
uncancelled functional equality. Their concurrent stop/join timings are
illustrative functional observations, not a cancellation-speed claim.

Whole Program decode/static preflight, in-flight reads/decryption/image conversion,
already executing Lua/dynamic loads and Renderer/saved preparation are not
interrupted inside those units. The earlier [resource-packet cancellation
proof](UVI_INITIAL_RESOURCE_CANCELLATION.md) remains separate; no universal
retirement deadline follows from either proof.
