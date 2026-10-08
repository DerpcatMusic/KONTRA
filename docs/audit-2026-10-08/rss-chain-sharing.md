# Shared immutable voice chains

Base: `c39fd7b0fc3245b3087c5da7408028a9831ffbfd` (includes W15 pan routing).
Status: **HOLD** until the complete 22-item PCM comparison and Pacific Legato
smoke discrepancy are resolved. No load-time acceptance claim yet.

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
protocol (which uses CC1=100). First eleven gate items: identical nonzero PCM
and complete runtime counters. Remaining items are pending.

## RSS and load receipts

Numeric receipts: `~/.cache/kontakto-fix-load/rss-owners/c39-chain-{before,after}-rss.json`.
Same production base, original plugin Load/publication/editor path, disabled
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
smoke first-audio frames differ (192 before, 256 after). Both observations are
retained, pending trace/quiet checks; neither is being dismissed as contention.

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
