# Runtime mixer and installed-test validation

The first complete 2026-10-12 test attempt exposed historical installed-test
assumptions after the library environment was supplied explicitly. This report
separates those test defects from confirmed runtime behavior. It does not claim
Kontakt/Falcon native sound parity or reinterpret a silent library as audible.

## Solo mask cost and semantics

`Part::mix_nodes` uses `tree::audible` to decide which runtime buses receive a
nonzero mixer gain. The existing contract keeps explicitly soloed nodes, their
ancestors and their descendants. Mute is a separate fader decision: muting a
soloed node does not cancel its solo request or admit an unrelated sibling.

The old algorithm searched all possible solo nodes separately for every tree
node, then walked their parents. A shallow tree with any solo therefore cost
quadratic work. The old installed AO test repeated that operation for every
translated group, compounding the cost toward cubic work. Scripted UVI groups
are split by original keygroup/oscillator identity, so the node count can be
thousands; they are not simply the few displayed source layers.

The new allocation-free algorithm marks each solo node's ancestor path once,
then checks each remaining node for an explicitly soloed ancestor. Its work is
bounded by node count times tree depth. A marked ancestor is not treated as a
solo request, preserving sibling exclusion. Parents may appear after children.
Short output slices and untouched output slots beyond the tree retain the old
behavior. Out-of-range parents and cycles were not supported by the old
algorithm; this optimization does not add a new malformed-tree contract.

An authored branching tree includes out-of-order parents, siblings and a
grandchild. All 4,096 combinations of open/solo/mute/muted-solo across six nodes
are compared with an independent ancestor/descendant oracle. Fader gain,
partial output slices and preserved trailing slots are checked in every case.

## Bounded installed mixer protocol

The old checker auditioned one held note, rendered 400 blocks with 2 ms sleeps
for each group, and asserted that each solo peak was no larger than the earlier
all-source peak. AO's thousands of translated groups made it effectively
unbounded. Sequential windows could outlive a one-shot sample; different source
phases, effects and sample positions also invalidate that magnitude inequality.
Failed key attempts were not released, and controller state was unspecified.

The replacement uses the explicit AO Bartok program rather than the first bank
member, sets CC1=100 and CC11=127, and tries at most four mapped keys at a fixed
velocity. It permits up to two audio seconds for actual onset, within a shared
20-second wall deadline after loading. Preparation has its own explicit
120-second cancellation budget and reports `load_ms`; the historical AO load
alone takes approximately 28 seconds on this machine. Every attempt receives a note-off, owner advancement
and NOTE_END service. Offline begin/render/end blocks service lazy sample heads
without consuming an unloaded attack as realtime silence; offline failures must
remain zero. Cases start with fresh notes and clear previous voice/FX
state. Configured AO roots must locate and load the fixture; load failures are
test failures instead of skipped passes.

The checker verifies finite rendered audio and post-fader node meters, tests
all-source mute once, and samples two currently active solo paths. Each solo
mask is checked exactly against the source tree's ancestor/descendant relation.
Sources outside the solo path must have zero post-fader audio; the selected
active node must pass audio and must become zero when explicitly muted. This
checks actual runtime routing without comparing unrelated aggregate peaks or
misclassifying downstream effect tails as a muted input leak. The small authored
routing fixtures remain the exhaustive semantic gate.

## Validation receipts

Focused command results are appended after serialized compilation and execution.
Library files remain read-only. No library PCM or protected script source is
persisted by these checks. The interrupted whole-suite attempt is retained as
`/tmp/kontra-parity-audit-20261012/final-cargo-tests.log`; interruption of the
unbounded root executable is not a passing suite result.

The first bounded-check attempt placed the 20-second deadline around preparation
as well as audio. It returned `Canceled` after 27.63 seconds, before any mixer
audition ran. That was a misconfigured test budget, not a library playback
failure. The separate preparation and audio budgets above correct its scope.

The tree tests passed **3/3**, including all 4,096 state combinations and the
10,001-node sparse solo case with zero measured allocation/free calls. The large
case reported 24 microseconds including its assertions on this run; this is an
informational smoke measurement, not a calibrated latency guarantee. Log:
`/tmp/kontra-parity-audit-20261012/runtime-tree.log`, exit 0.

The exact installed AO mixer check then passed in **16.43 seconds** including
preparation. Numeric load instrumentation reported **14,882 ms** of loading;
the audio/routing protocol took approximately **1.55 seconds**, within its
20-second bound. The program had **6,984 translated Group nodes**. Active nodes
877 and 2622 passed solo source audio with post-fader peaks **0.02767658** and
**0.020690354**, respectively. Both exact solo masks, outside-path zero meters,
all-source mute, muted-solo zero meters and finite PCM checks passed; offline
failures were zero. These are observed mixer gates, not a native Falcon audio
comparison. The command was `cargo test -p kontakto --profile ci --lib
sound::v2::tests::real_uvi_layer_nodes_pass_audio -- --exact --nocapture
--test-threads=1`, through the heavy wrapper with explicit Kontakt/UVI roots and
`KONTRA_AUDIT_LOAD=1`. Receipt:
`/tmp/kontra-parity-audit-20261012/runtime-uvi-mixer-green.log`, exit 0.

## Kontakt bus and streamed trait checks

The original `real_instrument_buses_become_mixer_nodes` failed in an
isolated installed-library run: its expected compact send names `send 0`
and `send 1` differed from the translated physical slots `send 2` and
`send 5`. Newly retained source instrument buses also expanded the tree;
some routed buses are classified as microphone nodes. The repaired check
retains exact insert/send expectations and verifies all sixteen physical
instrument-bus addresses against node names and authored output parents.
It passed in 3.10 s. This verifies topology and identity, not native audio.

The original `real_kontakt_instrument_plays_through_the_trait` failed after
40 tight render calls with no time for the loader's lazy sample heads to
arrive. Its adjacent offline storage fixture already proves how delayed
decoding should synchronize with the host seam. The installed test now
uses the real begin/render/end block contract in offline mode. Its
original 0.01 audio threshold, sparse residency, purge and subsequent
audible reload assertions remain.

This exposed a second stale assertion: residency after purging was
compared with the first-onset snapshot. Lazy heads can arrive while the
held and release voices render, so the immediate pre-purge snapshot is
the relevant baseline. With that corrected, the complete test passed in
0.12 s: resident bytes **100,675,984 → 100,669,640**, packed bytes freed
**4,758**, audible reload verified, offline failures **0**. This checks
purge behavior; it is not a general peak-memory or performance result.

Both focused commands ran through `kontakto-heavy`, `--profile ci`, with
`KONTRA_KONTAKT_LIBRARIES=/mnt/MAIN_STORAGE/Libraries/Kontakt` and
`KONTRA_SCAN_ACTIVE=1`. Original failures were reproduced before repair
using the retained root test executable, also through the heavy wrapper.
No Kontakt DSP kernel or parameter law changed in these repairs.

The installed `streamed_real_instrument_installs_physical_engine_bindings_and_lookups`
test likewise assumed every binding belonged to an internal modulator.
Current plans also bind effects and modulation-route controls. The
repaired test checks envelope stages against physical internal-modulator
owners, semantic aliases against their control identities and effect/route
consumers, and direct native effect lanes against their physical processor
slots. It requires both envelope and other owners and retains the native
attack setter/getter round trip. The original assertion failed in the
whole-suite attempt; the focused repaired test passed in **0.10 s**, with
**1,253 controls** and **272 physical lookups**. The receipt is
`/tmp/kontra-parity-audit-20261012/kontakt-source-binding-green.log`.

## Mixed-format rack host and generated ledger

The first explicit installed-rack run admitted a nonexistent relative virtual UFS
path because its resolver only checked for the substring `.ufs`. Discovery now
requires the bank file to exist; an authored fixture rejects missing banks and
directories while accepting existing virtual banks and ordinary files.

With both installed roots configured, the next run exposed the same headless UI
outbox omission: Conflux Hybrid Walk and the combined Horns case were silent.
The rack now retains each part's script models and consumes UI effects outside
the counted audio callback. Unsupported MIDI/array file services remain queued
and explicitly fail the witness. No gain, musical key, channel, duration or
audibility threshold changed. The final run passed both tests in 12.52 seconds,
including channel/output isolation, layered parts, MPE, articulation migration,
and zero heap calls during counted rendering. Informational block timings were
p50 218.954 microseconds and p99 438.138 microseconds against the 128-frame
48-kHz deadline of 2666.667 microseconds. This was not a quiet benchmark or native
PCM comparison. Its remaining Horns per-node-meter gap is printed separately.
Logs: `/tmp/kontra-parity-audit-20261012/one-rack-fixed.log` (unserviced failure)
and `one-rack-serviced.log` (pass).

The generated UVI ledger test also exposed stale source line locations. The
ledger was regenerated from `parity_summary --ledger`; normalizing only source
line numbers proved all statuses, reasons and translated-operation text unchanged.
Its large diff reflects repeated source citations, not thousands of new APIs.


## Remaining host-contract and quiet-output diagnostics

The next all-14-package run supplied the complete installed Kontakt and UVI
roots. It exposed four remaining root-library tests and two `real_banks` cases.
The receipts below preserve failures; repairs to a test's host lifecycle are
not evidence that its original audio contract passed.

The headless mixer and three UVI root checks now retain `loaded.scripts` and
service each part's owned KSP UI outbox outside the counted audio callback.
Effects must address the active plan and an existing owner/service table.
External MIDI and array-file jobs are refused and remain queued; the witness
fails explicitly instead of discarding them. This is the host contract already
proved by the mixed-format rack and native renderer checks. Audio blocks use
`begin_block(offline=true)`, finite rendering, and `end_block`/NOTE_END service.
Onsets permit up to two audio seconds (750 blocks of 128 frames at 48 kHz) with
a ten-second wall guard, preserving the existing **strict peak > 0.01** gate.
Warm-note heap counting excludes host model work, while retaining every audio
NoteOn, block and NoteOff operation and the zero-allocation assertion.

Source inspection also corrected the reset assumption. `Runtime::panic`
cancels all pending behavior contexts, including unfinished plan/on-init
callbacks and controller callbacks; it does **not** reset the retained CC1 or
CC11 values. Mixer auditions instead send channel all-sound-off (CC120), whose
source contract cancels that domain's note/controller callbacks while preserving
plan initialization. They reapply the declared controllers, service the owner
before each fresh note, and verify the admitted controller values. This is a
source-grounded host repair; it did not resolve the Horns failure below.

The UVI stream test now separates sample-head residency from the fixed 96 MiB
page pool. Purging compares the immediate pre-trim snapshot after release with
the post-trim snapshot, retaining positive freed bytes, reduced residency and
fixed-pool lower bounds. Those checks remain beyond the failing onset gate and
have **not** been validated for this Contrabass program.

Two serialized focused runs retained the four failures. The first host-lifecycle
run is `runtime-host-contract.log`, exit 101. The single numeric diagnostic run
is `runtime-host-diagnostic.log`, exit 101, with four tests completing in 13.53 s
(after compilation). Full paths are under
`/tmp/kontra-parity-audit-20261012/`. No PCM, protected script source or key
material was written by these witnesses.

The numeric diagnostic distinguishes the current Contrabass result from zero
output. With CC1=100 and CC11=127, admitted and effective controller values
matched (3374617161 and 4294967295). The pitch fixture at key 40 produced finite
output peak **0.0016416366** and node peak **0.0040436434**. The heap fixture at
key 36 produced **0.0017324268** / **0.0037928848**; the trait fixture at key 36
produced **0.0016875573** / **0.003782188**. Each had one active/audible voice,
level 1, no silent notes and no offline, streaming or Lua faults. They fail the
unchanged 0.01 witness threshold. There is no native-host amplitude reference
that establishes whether this level is wrong, so no gain law or threshold was
changed. The later pitch response, audio-thread heap and purge assertions remain
unverified for this program in these runs.

The installed Horns microphone case loaded in 467 ms and tested keys 60, 44,
46 and 48 at velocity 100 with CC1=100/CC11=127. All four notes were suppressed
without onset; `silent_notes=4`, packed silent receipt `[304,0,0]`, zero capacity,
streaming, offline and script faults. The source/UI host repair and the bounded
protocol did not resolve it. Its microphone routing contract remains unresolved;
passing the combined mixed-format Horns witness is not proof of this isolated
case.

The narrowed VWinds scripted bank run exposed a separate loader panic: key-range
selection renumbered zone sample assets, while a reverb impulse still named its
original asset index. The loader now decodes impulse assets before narrowing
zones in both scripted and plain bank paths. A focused index regression passed
**1/1**, and the installed Bb Clarinet scripted test passed **1/1** with a finite
peak of **0.026274467** and four reported script findings. The installed receipt
is `/tmp/kontra-parity-audit-20261012/uvi-vwinds-impulse-fix.log`, exit 0.
This proves the loader no longer panics for that program and that this audition
produces audio; it is not a native UVI PCM comparison. The separate Augmented
Orchestra `City Walker` case still produced peak zero in the earlier full
`real_banks` run, so that suite is not green.
