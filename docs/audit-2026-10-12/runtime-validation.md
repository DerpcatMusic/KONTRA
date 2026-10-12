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
