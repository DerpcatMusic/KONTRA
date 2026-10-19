# Actionable KSP support reports

The Halpern report identified `callback #6: InvalidInput`, which did not identify an operation or its input. The instrument is unavailable locally. This change improves evidence collection; it does not claim to fix or reproduce that instrument's failure.

Runtime faults retain the actual failing instruction PC (before the dispatcher advances it), physical script slot, callback role and ID, UI ID, source byte offset/line/character column, builtin, up to 32 numeric VM registers, eight caller source sites, sample time, and source SHA-256. Arrays and waits receive named operand/domain fields. Real registers are labeled as raw IEEE-754 bits. Omitted registers/callers are counted explicitly. Fuel exhaustion identifies the next instruction to execute.

The compiler shares a sparse immutable source map across entries into each bytecode module. Fault collection copies fixed-size data while the continuation still owns its registers; it performs no allocation, text formatting, locking, logging, I/O, or source lookup on audio. The existing 256-record inbox counts overflow. Source lookup, hashing, grouping, formatting, and file publication remain off audio. Source excerpts are only attached when the exact generation's source fingerprint matches. Missing context is labeled explicitly.

The clipboard report includes the bounded excerpts and execution evidence instead of discarding them. Structured exports retain the same evidence. Existing path redaction and prohibited-field removal still apply. The journal keeps the first evidence per fault site; repeated faults remain counted in the runtime summary/sidecar. A report is diagnostic evidence, not an executable reproducer or a guarantee that every bug can be resolved without the instrument.

Initializer bounds warnings identify the actual index, array length/name or menu count, valid MIDI-key domain, and fallback. They say `KSP warning` instead of claiming that a warning itself is an unimplemented feature. Native-zone identity warnings include bounded counts of unrecognized serialized layouts (version, private-data length, raw u16 at offset 56); they do not invent native IDs. Indexed cursor-picture warnings retain their property values.

## Before-change contracts

| Path / formats | Thread / owner | Bounds and recovery | Distinguishing proof |
|---|---|---|---|
| KSP lowering / all | Loader creates source maps; prepared programs share their Arc; generation retirement drops them through existing ownership | At most one source run per instruction; no bytecode or DSP-law changes | Synthetic functions and shared UI callbacks resolve the actual failing source site |
| Callback fault capture / CLAP, VST3, standalone | Audio reads retained continuation/registers before retirement and copies into the existing inbox | 32 registers, eight callers, 256 queued faults; omission/overflow explicit | Allocation-counted callback capture, transport overflow, completion-backpressure tests |
| Report resolution / all | Loader consumes copied evidence and reads the matching instrument source; existing diagnostics worker writes journal | Five source lines, existing per-line/excerpt caps, clipboard problem caps | Actual VM failure → structured evidence; mismatched source rejected; copied text includes source/operands with redacted paths |

No native-host playback or Halpern-specific compatibility result is claimed. Validation is recorded in the PR.

## Default-feature CI fixture corrections

The first corrected CI build reached the root unit suite and exposed four fixture failures. The `CI required` job followed the failed Linux test job; Windows and macOS checks passed.

| Test path | Thread / owner | Bounds and recovery | Distinguishing proof |
|---|---|---|---|
| Copied log context | Test-owned synthetic history; report worker formats warning/error events only | Existing report caps; Info events remain inspectable in the UI and excluded from copied problems | An Info-level source excerpt is excluded; the dedicated runtime-error copy test still requires source and operands |
| Shared-filter product fixture | Test thread owns a `TempDir` and ordinary V2Core instances | Temporary directory follows the runner's scratch policy and drops after cores | Preserve all PCM comparisons, controller changes, held-owner identity and release assertions on a hosted Linux runner |
| Authored UVI playback fixture | Loader owns shared keygroup envelope; UI snapshot owns button activation capability | Four real shared source gates; existing bounded activation queue; no scalar button pulses | Assert actual keygroup gates and member ownership, then keep finite/late audio, sustain, release and zero-audio-heap checks |
| Momentary-button UI fixture | Test host owns persistent renderer input state and consumes explicit activations | One action per click/keyboard event; no scalar edit/reset | Actual pointer/keyboard actions call the authored callback exactly once |

The shared-filter and UVI envelope/button test bodies predate this PR. Production DSP/ownership guards remain unchanged.
