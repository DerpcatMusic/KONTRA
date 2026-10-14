# Raw bus meter observations

`Runtime::enable_bus_input_peak` enables observation of an exact runtime bus's
summed stereo input, before tone, inserts and host mix gain. Unobserved inputs
skip the sample scan. `bus_peak` exposes that input or the existing post-insert,
post-mix output peak. Taking input observations does not drain output observations.
Configuration and observation belong to the active immutable plan generation;
retired voices and tails cannot be mistaken for the replacement generation.
All configuration, reads, accumulation and drains use preallocated storage.

The bus integration suite checks independent input/output values through gain,
fan-out and phase cancellation, block partitions, inactive buses, disabling and
reenabling interest, plan replacement and callback heap guards. Its 17 tests
passed on 2026-10-14 (`work/raw-bus-input-peaks-tests.log`). This is correctness
evidence, not a performance benchmark of enabled metering.

These are raw linear stereo sample peaks, not a native AudioMeter emulation.
UVI's element ordinal and XML node identity are different address spaces. A
native element/bus request needs an explicit validated mapping to a runtime bus;
using the instrument output for an unmapped layer or oscillator is incorrect.
The current translator can collapse scopes without inserts into their parent,
so even a valid native request need not have an independently observable bus.
Keygroup/oscillator taps and native mono summation, normalization, ballistics and
display laws remain unresolved. Taking the larger voice/channel peak does not
recover a summed signal: cancellation is a counterexample covered by the test.

The [conservative UVI bridge](UVI_METER_REQUESTS.md) now publishes raw observations
for uniquely owned Program/Layer default buses before the mixer drains them.
Typed availability stays separate from the scalar painter. Missing ownership,
collapsed/per-voice scopes, stale plans and native display laws remain diagnosed;
there is no Program fallback. Kontakt's existing output meter addresses and
readback behavior remain separate.
