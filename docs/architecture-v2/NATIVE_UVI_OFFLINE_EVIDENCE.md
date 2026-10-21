# Native UVI offline source evidence

UNPACER 0.3.6 recovered one offline stage of the original UVI Workstation image
without launching its entry point or reading activation/account state. This
provides a source-bound DSP research reference; it is not an installable image,
runtime restoration or proof of native audio equivalence.

| Artifact | Bytes | SHA-256 |
|---|---:|---|
| Original Workstation | 64,649,552 | `78729e96b752aea746280275072ad24cb4399a053739c49a161ff1fcfbf85721` |
| Extracted guard | 13,645,824 | `1acacffc56785f0517c205e3e4e16c7fe3c197e73c99249797c761ee1d13bcbc` |
| Separate research image | 64,649,552 | `c997d9ce5ca6619a74d159a3424003cb5f76f7c51825f2234b2d057ee1ac9925` |

The guard's complete `.text` (10,113,024 bytes), `.rdata` (2,228,224 bytes)
and `.pdata` (284,672 bytes) exactly match the previously reviewed decoder
reference. Complete constructor/continuation, primitive and preparation spans
also match. This supports the source model rather than guessing another
product's transform from a section name or address alone.

A private caller-supplied profile binds both inputs, complete source spans and
four independently retained instruction anchors. UNPACER passed 894 complete
held-out type descriptors, separate from its 64 training equations, and all
four anchors. Its inferred model word stays in process memory and is not
included in profiles or receipts. A separate in-memory review compared the full
1,821-byte previously retained setter and all 41 historical UVI function spans:
every byte matched. Six complete function extents also match unchanged original
exception metadata and disassemble to their full lengths. Only after these
checks was the output digest registered and a new read-only research image
written. The original file remains unchanged.

The transformed records are `.text`, `IPPCODE`, `.data` and `IPPDATA`, totaling
30,743,040 file bytes. All other bytes remain identical. This is a byte-stage
boundary, not a percentage of algorithms or features recovered.

## LFO source admission

UNPACER packaged the complete original exception-owned generator range
`0x1410f8ca0..0x1410f9be6`, 3,910 bytes, SHA-256
`2fece96c113516ee91b07a73c31f199c7bd5b5f29afb62c96e9632d18165a429`.
It decoded 880 instructions. REA/Ghidra 12.1.4 admitted exactly those source
bytes with zero synthetic function-body bytes, evidence
`ev_869ce6edc5b14b7aabe80f17d608437c9a0c15ddb2510e5358ea00aa5406b161`.
Carrier headers and analysis scaffolding are separate from vendor instructions.

Historical pseudocode is now grounded for the independently compared spans;
this does not validate arbitrary historical project memory, decompiler semantics
or execution reachability. Factory tables in virtual data, constructor/reset
ownership, per-source state, clock partitioning, rate modulation, interpolation
and waveform output still need explicit original-kernel witnesses before
clearing importer diagnostics. A generic mathematical replacement is not a
native parity oracle.

Private reproduction and retained failures are under
`PluginArtifacts/kontra-uvi-offline-recovery-20261015`: `inspect.json`,
`source-review.json`, the derivation and validated profiles,
`derive-digest.json`, `independent-review.json`, `output-validation.json`, and
the verified `lfo-generator-frontier` bundle. The first function request used
the encoded input with a decoded-stage selector and was correctly refused;
the successful request uses the independently validated research image.
Earlier activation-window observations remain historical failures rather than
being rewritten as successful runtime capture.

## Bounded original-code execution

Six original setter calls match independently calculated state and exact write
masks at 44.1, 48 and 96 kHz. The rate setter updates both chaos denominators
and the phase increment; the examined frequency setter resets state only in
mode 3, and the depth setter writes its scalar field. All 4,816 caller-owned
bytes are checked at each snapshot.

The generator reads a fifth Windows x64 stack argument. A private additive
eight-slot bridge first passed authored ABI checks for 0..8 arguments, rejection
of 9, the existing four-slot convention, mixed integer/float/pointer arguments,
stack alignment/home space, return registers, nonvolatile registers and caller
sentinels. Neither the installed tool nor vendor instructions were modified.
Private profile registration remains explicitly caller-reviewed authority.

On that adapter, six original generator calls compare Lorenz type 7 and an
authored 257-cell User type 9 table at the three rates over 64 frames. All
caller state, output and canary bytes match the independent binary32 model.
Another 42 calls reuse the same source closure and 13,920-byte caller blocks
through actual partitions 0, 1, 31, 32, 33, 63 and 64. Each partition's state and
outputs match its own model; this does not assert partition-invariant Euler
integration. Lorenz commits actual cell steps and computes an additional
uncommitted boundary lookahead. User interpolation uses the binary32-derived
u32 phase increment, 24-bit fraction and wrapped table endpoint.

The cases explicitly set reset, sync mode, delay, rise and smoothing to zero,
and bipolar/depth to one. They do not establish constructor defaults, absent
User-table behavior, triggering, shared ownership or host PCM parity. Runtime
admission of these waveforms remains separate work. Private receipts under
`kontra-uvi-lfo-20261015` are `kernel-state-20261015/independent-results.json`,
`kernel-generator-draft-20261015/independent-results.json` and
`kernel-generator-partitions-20261015/independent-results.json`.
