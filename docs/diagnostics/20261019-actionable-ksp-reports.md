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
