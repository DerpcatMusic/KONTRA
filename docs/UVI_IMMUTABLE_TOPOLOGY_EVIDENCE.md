# Immutable relative-edge presence

This follow-up to `8bae38c` prepares the existing `any(ConnectionMode == 0)`
decision once for each compiled numeric parameter. The original evaluator
repeated that structural scan on every parameter memo miss. Connection topology
is immutable for a graph owner; numeric live writes change values or append an
unconnected parameter, and do not change the compiled connection modes.

The bit includes bypassed relative edges. It does not fold Bypass, Ratio,
Inverted, source values, target bases or source clocks. Depth and memo checks,
unsupported-target rejection, eager base evaluation, recursive edge order and
atomic target publication retain their original order. All three parameter
constructors and the single final edge-binding assignment initialize the bit.
No modulation cadence, admission rule, queue capacity or player API changes.

## Verification boundary

All 18 centrally selected functional checks passed. The authored regression refreshes a
reference graph's bit from the legacy structural scan before each query and
compares result bits, errors and semantic Constant clock state. It exercises
relative-only, absolute-only and mixed routes, nested Ratio dependencies,
two voice owners, bypass/inversion, retargeting, late numeric registration,
signed zero and failure precedence. Both evaluations share the current
evaluator body: this is a structural regression check, not an independent
old-player or native-engine comparison. Existing physical endpoint, input,
source memo, absolute clock and actual Renderer fixtures complement that check.

The candidate and legacy numeric records both occupy 104 inline bytes on this
Linux x86_64 target; this is a measured layout observation, not a portable ABI.
No new heap container, source index or cache invalidation policy is added.
There is no measured throughput improvement, actual-bank replay, DAW exercise,
new visual verification or complete Falcon fidelity proof in this change.
The installed checkpoint's build information and verification receipt identify
the actual clean source revision and executed checks.

The worker statistics comments also match their existing timer boundaries:
recorded render wall time includes packet validation and Player rendering;
measured worker CPU includes optional hosted-packet validation and Player
rendering after request/receipt validation. Waiting, service snapshots and output
publication are excluded. Validation failures returning before recording are
excluded. The timing implementation and report schema are unchanged.
