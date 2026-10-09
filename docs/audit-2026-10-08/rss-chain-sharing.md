# Shared immutable voice chains

Base: `c39fd7b0fc3245b3087c5da7408028a9831ffbfd` (includes W15 pan routing).
Status: **HOLD, parked** at the item timebox. Seeded UVI indices11/21 and the
authorized quiet load receipt remain UNKNOWN. Pacific smoke trace is resolved.

## Change and v1 source

Port the exact-bit HashMap entry reuse in v1
`0cb7a8a0:src/engine/params.rs::share_curves` to the shared IR lowerer. V1
`src/engine/bank.rs` keeps processor settings per group. The v2 lowerer formerly
retained and compiled a separate immutable voice chain for every zone.

Reuse a chain only when authored group and chain identity, combined boost,
combined pan and pan law agree. Region attenuation remains per region. Different
authored owners are kept even when their processor values happen to match.
Each zone still undergoes its original construction and validation; duplicate
transient chains are discarded on the loader. This removes repeated compiled
lanes and filter coefficient caches, not per-voice filter histories, delays,
cursors or expression ownership. The existing voice-modulation cache also
benefits from stable shared chain indices. No streaming, compiler or VM change.

Source attribution: `dsp/svf.rs::FilterBank::new` allocates a coefficient cache
for every compiled filter; each cache contains 64 four-f64 coefficient rows.
This explains why retained RSS savings can exceed the immutable processor
struct size. This is source evidence, not a measured allocation-owner total.

## Targeted checks

Failing-first private unit: the original lowerer retained five chains where
four were expected. The final seven-zone fixture checks repeated-chain reuse,
different authored chains/groups, pan, boost, and per-region attenuation.

Candidate: unit plus controls, DSP, envelope batching, lower, signal trace and
voice modulation: **62 PASS, 1 ignored**. Existing DSP fixtures check independent
overlapping voice histories, tails, slot reuse and no audio-thread heap calls.
Root `cargo test --locked --lib --no-run` PASS.

Offline witness uses the production V2Loader/V2Core at 48 kHz, block64, four
seconds, note-plan key/program/keyswitch, velocity64, CC1=110, CC11=127 and
sustain127. It writes only BLAKE3 hashes and numeric runtime counters. This is
a paired audio correctness check on the gate items, not the scanner's realtime
protocol (which uses CC1=100). The full 22-item matrix has 23 program records: all load/render, and all 23
runtime-counter records match. Seventeen audible PCM records match (15 Kontakt
programs plus UVI indices 19/20). NKM index17/program0 is identically silent on
both builds and remains UNKNOWN for audible parity. UVI indices11/12/13/14/21
have different digests and remain UNKNOWN. There is no all-22 PASS claim.

The unchanged baseline also produces different PCM on repeat UVI12/13 runs,
with zero counters and nonzero audio. W10 confirmed the production threaded Lua
path lacks an offline script barrier and starts Luau math RNG without a fixed
audit seed. W10 owns the explicit audit-only seed/clock-barrier follow-up; the
normal product randomness and realtime path remain its responsibility. The
seeded follow-up is recorded below; the original unseeded receipts are retained.
Numeric receipts: `c39-chain-pcm-summary.json` and
`c39-chain-aa-pcm-{12,13}.json` in the same owned cache.

UVI12 graph prefixes show the baseline A/A first difference at frame8192: source
ratio 0.867491744330555 versus 0.8659148502993635. Before/after differs upstream
at frame0, ratio0.7895661428745404 versus0.788853833822799. Graphs agree. Existing
PCM witness exits before the trace writer flushes; these UVI exports lack valid
JSON footers. Prefix records support this diagnostic observation only; complete
coverage and dropped-record counts remain UNKNOWN. No trace was repaired.
See `c39-chain-uvi12-first-divergence.json` and `c39-chain-uvi12-trace-summary.json`.

## Seeded follow-up, parked

W10 supplied audit protocol `0be9aa7b8bc41257980c455f1ecb1e93a4ea00fb`.
Both sides include the same work/save dependencies `08dfa0e7`, `469013bc`,
`c41e9ef9`, `78d159df`, root `--features shots`, and
`KONTRA_UVI_AUDIT_SEED=42`. Source fingerprints match outside `lower.rs`.
The isolated reproduction branch is `audit/w8-chain-seeded-c39` at `bb36f954`;
scanner-only conflict hunks were excluded because this check uses the renderer.
Do not merge that audit branch as the chain production change.

| Held UVI index | Matched dev baseline/candidate | Verdict |
|---|---|---|
| 11 | both timeout at 180 seconds | UNKNOWN |
| 12 | identical audible PCM, zero counters | PASS |
| 13 | identical audible PCM, zero counters | PASS |
| 14 | identical audible PCM, zero counters | PASS |
| 21 | both timeout at 180 seconds | UNKNOWN |

Rows12/13 reproduce W10's exact committed optimized hashes `97ad41a1…` and
`e8e09e24…`. Full PCM records, including every runtime counter, agree on all
three passing rows. Combined with the original protocol's Kontakt and UVI19/20
checks, there are now 20 audible matching records, one identical silent record,
and two held records. This combines explicitly different audit protocols; it
is not an all-22 seeded PASS claim.

A separately frozen matched `ci` pair retried index11. Its unchanged baseline
also timed out at 180 seconds, so optimization did not resolve the UNKNOWN.
The candidate attempt was canceled and index21's optimized attempts were not
measured when the item was parked for the P0 installed Falcon discovery hotfix.
A read-only candidate snapshot showed `uvi-script` at64.73 CPU seconds, the
main thread at0.13 and stream workers at0.00, with the main thread waiting.
That routes investigation to the UVI owner path; it proves neither exhaustion
nor the underlying cause. No compiler, VM, stream, or audit-protocol repair is
included in this chain change.

Numeric receipts in the owned cache:
`c39-chain-seeded-summary.{json,md}`,
`c39-chain-seeded-{before,after}-pcm-{11,12,13,14,21}.json`,
`c39-chain-seeded-ci-before-pcm-11.json`,
`c39-chain-seeded-ci-after-thread-snapshot.json`,
`c39-chain-seeded-ci-cancelled.json`.
All default-feature RSS/load frozen binaries remain untouched. These normal
wrapper renders establish correctness only; there is no CPU or load verdict.

| Additional frozen witness | SHA256 |
|---|---|
| c39-chain-seeded-before-pcm (dev) | 0bd1964150da39c05d88ac0a93b1bbde121d3aa2a60fa14e5e808263aef0f604 |
| c39-chain-seeded-after-pcm (dev) | 3280032dc59edc4744c354fba6aadc27fb5a4c49ed8df4da1ec198bc2b21d974 |
| c39-chain-seeded-ci-before-pcm | fd0964c0eb0941cae34c967ea5b09f1bcae7c1a6ef4ba16ab5e9752a4f9ba06c |
| c39-chain-seeded-ci-after-pcm | ed765dca34b829cdecbf5ab4247c22171e12424558a873ab2e2eebae299f2526 |

Each has a separate BUILD.json with features, source fingerprints and protocol.
No authored PCM was retained; the original manifest and note plans are reused.

Parked work: resolve the owner-clock/work behavior for UVI11/21 after W10's
hotfix, then complete strict parity. Run the prepared six-cell quiet collector
only after the actual W9→W8 handoff. No W8 quiet request was created or claimed.
NEXT: UVI11/21 owner evidence, then authorized quiet load measurements.

## RSS and load receipts

Numeric receipts: `~/.cache/kontakto-fix-load/rss-owners/c39-chain-{before,after}-rss.json`.
Same production base, original plugin Load/publication/editor path, unoptimized
root test/dev profile, disabled
product cache (`XDG_CACHE_HOME=/dev/null`). Normal kontakto-heavy wrapper;
load/onset timing is **UNKNOWN**, since no quiet window was held.

| Preset | Editor RSS before MiB | After MiB | Delta MiB | RSS before Load before/after MiB | Load before/after ms, UNKNOWN |
|---|---:|---:|---:|---:|---:|
| Conflux | 246.48 | 244.13 | −2.36 | 38.03 / 37.73 | 5290.75 / 7607.52 |
| Pacific Legato | 187.60 | 156.55 | −31.05 | 38.66 / 38.45 | 861.10 / 987.02 |
| Analog | 815.61 | 398.81 | −416.80 | 38.31 / 38.20 | 21529.33 / 15358.85 |

Pool bytes stay 25,165,824 and eager head bytes stay zero. No unaccepted W9
streaming trial is included. Pacific smoke peak differs (0.06596755 before,
0.05550770 after); gate Pacific FX PCM agrees but is a different item. Analog
smoke first-audio frames differ (192 before, 256 after). Both original observations are retained. Two baseline Pacific traces and the
candidate trace now have identical graphs and all 52,245 records, complete with
zero dropped records. Peak is 0.05550770089030266 and first audio192 in the
parsed baseline repeat/candidate. Sample source, amplifier, filters, buses and
master records all match. This clears the candidate-specific Pacific smoke
concern without attributing the original untraced variation to a cause.
`c39-chain-pacific-trace-comparison.json` records the equality; the trace JSON/SVG
files live beside the frozen binaries. Analog onset still awaits quiet evidence.

An additional four-second Pacific offline gate render completed (nonzero), but
its remaining unrequested piano/A/A repeats were canceled to prioritize the
exact smoke trace. The unpaired digest is UNKNOWN and retained in
`c39-chain-pacific-extra-cancelled.json`; it is not used as a parity result.

Frozen binaries and BUILD.json live under
`/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w8-rss-20261008/`:

| Binary | SHA256 |
|---|---|
| c39-chain-before-test | c7976f14ceceae3432c122541594115b1cd4c6eb5337a7b0e8f05e852a419bf4 |
| c39-chain-before-pcm | 943ac31e2ede0727f753af987b1b51a6cc2af75ee8cf5b00bf33ac5ccb1ef197 |
| c39-chain-after-test | 5f9f28c5af795241e12264bf8576cfd6c8094eab125ccbd9bc01bb81946a5fed |
| c39-chain-after-pcm | 3e259384bacb0b5f19620ea21cb546eb6aea6c83d85e57fc5097fe71725bb4fc |

The frozen baseline has diagnostic-only test/example edits; its production
source is c39. Candidate compiled source is recorded in BUILD.json. Subsequent
formatting/help text and this receipt do not change the measured production code.
