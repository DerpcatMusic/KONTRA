# Bound relative-connection settings

The constructor binds the existing Ratio, Bypass and Inverted parameter records
before copying connection edges into numeric targets. Relative Factor and
additive evaluation read Bypass and Inverted directly through those stable
slots. They remain scalar settings: this does not turn them into recursive
modulation targets or cache their live values.

Registered and external overrides still precede static numbers and the original
attribute/default fallback. Missing public fields retain the named external
lookup. Target-law and eager-base checks precede Bypass; Bypass precedes Ratio
and source consumption; Inverted follows source consumption. Mode1 skips,
mapping, arithmetic, source clocks, cadence, validation and admission are
unchanged. Appending a later registered parameter preserves existing indices.

## Focused verification

All 20 selected authored functional checks passed. Two new checks compare the
frozen original scalar helper's bits and complete errors, and a specialized
one-edge Factor/additive reference's result/error and Constant clock state.
Cases include missing/malformed/nonfinite attributes, signed zero, both override
paths, defaults, appended slots, bypass skipping invalid later settings, and
cold/warm source failures in their original order. Malformed attributes are
mutations of valid authored programs, not new parser admission.

The remaining selected checks cover actual local Renderer writes/per-voice
application, nested Ratio, registered/external results, source memo/clock
guards, physical target conversions, absolute producers, Mode1 and failed
worker observations. They are synthetic local functional checks. The procedural
reference shares unchanged Ratio/source machinery; neither it nor these checks
is a native Workstation oracle or an independently compiled whole legacy player.

## Cost and measurement limits

On this x86_64 build, a Connection occupies 72 inline bytes versus 56 for its
legacy shape. Two persistent edge owners therefore add 32 bytes of payload per
serialized connection: 3,200,000 bytes at the existing 100,000-connection limit.
Vector spare capacity and allocator overhead are additional; this is not a
total allocated-memory bound. No new heap container or narrowed index exists.

One pinned, low-priority B-C-C-B probe performed 262,144 Boolean queries per
row. Named lookup took 1,937,196 and 1,693,421 ns; bound lookup took 600,551 and
598,702 ns. Each row produced the same checksum. This measures only the scalar
lookup helper on a small authored graph, without a statistical confidence
interval. It does not establish full-renderer throughput, larger-graph cache
effects, DAW deadlines, bank playback speedup or a universal RequestCapacity
fix. No actual-bank replay, native PCM comparison or new visual capture was
performed for this change. Full Falcon fidelity remains unfinished.
