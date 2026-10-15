# Falcon UI parameter lookup evidence

The measured AO startup does **not** justify a catalog hash index. Its 575,045
native getter calls perform only 422 immutable-schema comparisons. Almost every
read returns retained text from a short list. No getter optimization or value
cache was added.

## Frozen executable and oracle

The [numeric receipt](receipts/ao-ui-lookup-pairs-20261015.json) uses the production
initializer and shared CPU painter in the standalone inspector, audit seed 611,
and `ci` opt-level-3/no-LTO executable SHA-256
`59ba975a09619eac03a33625eeb5a54b474d896575d31a694fd5f6b805aa8bd1`.
All 682 tracked source hashes in the recorded build manifest stayed unchanged
during compilation. The build started at `de38bbe06` with recorded existing
uncommitted report/UI changes; that commit alone is not the complete binary
source description.

The four runs are off/on/on/off. Complete interface, resolved geometry, Bitmap
PNG and Vector PNG hashes match across all four and against the preceding
seeded baseline. Each retains 10,145 widgets, 101 visible widgets, 53 valid
hierarchical menus and the same 28 diagnosed properties. All 89 requested assets
succeed; no pending asset, dropped probe, script fault or budget exhaustion is
observed. Work remains 14,053,215/33,554,432 checkpoints, with 19,501,213 left.

The AO path does not construct Runtime, decode PCM, host audio, run gestures or
publish a plugin UI. PNGs and full reports stay in process memory. The committed
receipt contains hashes, numeric counters and bounded filename/function/line
metadata, with no vendor script body or artwork.

## Exact search distribution

Both profiled runs produce identical distributions and account for every native
getter call: 575,013 ordinary reads plus 32 specialized Step Envelope level reads.

| Ordinary getter observation | Count |
| --- | ---: |
| Schema comparisons / hits | 422 / 0 |
| Largest searched schema | 14 fields |
| Retained-field comparisons / hits | 1,934,409 / 575,013 |
| Largest retained list | 16 fields |
| Saved-text results | 575,013 |
| Cloned retained string bytes | 6,881,699 |
| Dropped calls / allocation failures | 0 / 0 |

The fixed `<uncatalogued>` bucket contains 574,962 reads. Every one searches an
eight-field list and finds its first match at position 3 or 4, averaging 3.364
comparisons. The remaining 51 ordinary reads belong to DAHDSR (20),
ScriptEventModulation (24), LFO (3), StepEnvelope (3) and ConstantModulation (1).
None of these calls hits a catalog declaration; all find retained text.

The scrubbed bucket intentionally does not reveal arbitrary XML kind names. It
cannot identify a specific element type or property, and the sampled Lua frames
are not an exact host-call/caller join. The public prelude's connection traversal
and display-name fallback are possible text-read paths, but their presence in
code does not prove which path owns this bucket.

The catalog itself has 167 kinds and 2,877 fields: 94 kinds have at most eight
fields, only 12 exceed 32, and the largest has 657. No duplicate schema names are
present. None of the large schemas is a hot getter in this receipt. A blanket
hash index would target work that is absent here. A numeric/boolean clone-elision
change would also miss this workload: every ordinary result is text. Caching
returned text or mutable field lists would require a separate, proved invalidation
contract and is not licensed by these counts.

## Instrumentation cost and stage attribution

No compiler or named external CPU workload appears in 115 monitor observations;
other desktop activity was not excluded. Median process wall is 5,757.95 ms off
and 5,801.38 ms on, an observed difference of 43.43 ms (0.75%). Total child user
plus system CPU differs by 28.20 ms. These two observations per mode, their fixed
order, resource/cache variation and runtime/GC variation do not establish a
causal total-overhead bound.

The probe directly clocks record updates at 20.45–21.04 ms, and stack gathering
at 2.19–2.25 ms. Those clocks exclude complete branch overhead and other profiler
work. Native getter clocks include the new diagnostic work (261.6–270.5 ms);
they cannot be compared with an older uninstrumented getter clock as a speedup.
All lookups still read the current fields, clone and parse exactly as the ordinary
path, including allocation failure. Ordinary constructors keep profiling disabled.

On-run production translate/initialize is 4,486–4,504 ms. Its inclusive clocks
include 2,427–2,434 ms host construction, 1,837–1,850 ms script body/require and
1,953–1,966 ms post-init overlay/IR translation. Overlay export itself is only
30–32 ms. These clocks nest and must not be summed as exclusive costs. Inspector
model export is 157–176 ms; complete resource/CPU paint/PNG stage is 316–318 ms.
The next investigation should preserve that stage separation rather than treat
all remaining time as parameter search or asset I/O.

Trusted compile-time names now separate the library frames: top-frame visits
identify `scripts/_ui/modulatedknob.lua::__init` (82–85 self visits) and
`scripts/_main/globalfunctions.lua::getParameterDefinitionWithName` (23–27).
Prelude `getParameter` (200–232), element `__index` (133–147) and `type`
(118–121), plus the private catalog copier (103–112), remain prominent. These
are VM safepoint visits, not function self CPU. All 899/905 observations are
accounted for; 22 trusted names fit the bound without drops. No property-name or
value cache follows from these samples.
