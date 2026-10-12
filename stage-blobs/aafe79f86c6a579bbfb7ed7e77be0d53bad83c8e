# Authored renderer checkpoint — 2026-10-08

W3 owns authored resources, pictures, fonts, NativeUI and pure painters. W1 owns
selection/publication/typed transport; W2 owns input; W8 owns loading and header
artwork. This checkpoint does **not** certify all R0–R12 acceptance conditions.

## Shared scanner evidence

Driver source: `tools/kontra-scan@2355155b56a5ae26add8bdbc287d46d42e7825f2`.
Frozen v2 binary SHA-256:
`f1105599c13b14410ed891b29a014b8af43596e238850bb8e4439596e96e013a`.
W3 after adapter: `5a1793a3` (before the subsequent waveform/text/header changes).
Each item is a fresh whole worker, Original, using the common CLI and note plan.

| Fixture | Frozen v2 Original | W3 Original | Frozen v2 peak MiB | W3 peak MiB |
|---|---|---|---:|---:|
| Conflux | missing-images | original-ok | 229.18 | 264.71 |
| Analog Strings | original-ok | original-ok | 1257.89 | 895.82 |
| Clarinet A | missing-images | original-ok | 140.43 | 154.17 |

These are a three-item shard, not a corpus result. Logs/JSON/screenshots are in
`~/.cache/kontakto-w3/{final-baseline-v2,after-final-scanner/scan}`. There is no
same-final-driver combined W3+W8 load measurement yet. Analog's editor-open
memory gate against v1 remains open; lower picture residency alone does not pass
it. Clarinet's increase over the already-low frozen v2 peak also remains open.
Phase RSS and toggle/reopen/cancel measurements remain required.

Conflux consumes its real legacy `.nui` graph. The initial Native page resolves
167/167 requests (134 modules and 33 images), decodes 33/33 images and validates
8 supplied fonts. Its cream ground fraction changes from 93.8985% to 0%, hence
non-ground pixels change from 6.1015% to 100%. Native page pixel BLAKE3:
`4d49a3b17c15f0ba74735be00495b87389bb4485ed40bf15717c2006e92049a1`.
This is our authored renderer, not a retained native Kontakt golden capture.
Native Edit/FX pages and source callback gestures remain to be checked.
Clarinet uses the shared UVI resource route; visible requested frames resolve
8/8 and one supplied font resolves. The requested 21/21 full image inventory is
still a separate acceptance condition.

## Macro-name regression

The six complete published `@Footer__Macro__Name__1..6` strings reach Native
primitives and final text geometry unchanged. Only lengths/equality/metrics are
recorded. Generic editable payload insets `[8,6]` survived `.pad(0)`, reducing
44 px fields to 28 px viewports. Removing them restores the full viewport.
The Native graph declares no font for these fields; v1's bounded caption fit
is used with the existing fallback face while unfocused. Authored fonts and
focused editing retain their specified size. All six seven-character captions
now have a complete advance of 44 px inside a 44 px viewport.

`native_saved_text_reaches_authored_field_without_host_insets` fails before the
inset fix and passes after the full fit fix. It requires a locally owned fixture
and `RUST_MIN_STACK=67108864`; no saved names or source bytes are logged.
`native-text-{red,green,font,properties,fit}.log` retain the numeric receipts.

Supplied Native fonts can also resolve their embedded family/full/PostScript
names, rather than requiring the family to equal a filename. Only bounded,
validated package font bytes provide this authority; no system-font discovery
or substitution is performed. Font bytes participate in the existing 64 MiB
picture/font LRU. Native modules/fonts have a separate 16 MiB bound and Native
images a 48 MiB LRU; generation/cancel and phase-memory acceptance still need
broader lifecycle tests.

## Remaining gates

- Full strips/states, handles, hide masks, authored caption placement and source
  colours; corner/alpha and bitmap-font pixel fixtures at device scales 1 and 2.
- Native page/gesture/source-binding checks, including the four classic aliases.
- Structured shared resource failures and reload retry; arbitrary image-format
  frame/window/downscale fidelity and cancellation.
- Real waveform slices/wavetable semantics, source meter colour/range states,
  file captions and geometry/device snapping.
- Final-driver v1 comparison, owner-resolved W8 loader integration, phase RSS,
  quick15 and corpus-wide Original OK/no-regression acceptance.

W8's header/source-accessor checkpoint `93390aeb` is imported without rewriting
his loader or authored decoder. The only additional non-owned implementation
edit is W8's prerequisite allocation-accounting test block in `src/plugin.rs`.
The root shots no-run gate and the synthetic Native source-edit/context/font
checks pass. The real macro regression passes in 45.25 seconds.

## Merged transport and binding check

W1 `4f1abcc7` is merged, retaining source-addressed typed snapshots and the
generation-bound waveform provider. The merged macro fixture passes again in
41.07 seconds: all six complete strings have 44 px advances/viewports and zero
editable insets. An explicitly unavailable supplied family reports the Native
font-service category; it does not silently use a different font.

The initial Native Edit graph reads values from `Edit__Synth__Src__WTSelect`,
`SPLSelect` and `WTShaper` parameters, all from KSP slot 2 (UI IDs 32809, 32820,
32815). Their three aliases are not read through `Parameter.value()` or
`ksp_control_property` in this active graph (extended probe: 127.33 seconds).
Picker writes and other states remain untested, so this does not classify
their complete interaction intent. The
`Edit__Synth__Shp__SelectAlias` state remains unclassified. This RAM-only probe
invokes the authored tab callback and records primitive kinds plus binding
metadata; it does not certify pointer gestures or native-host stacking.

Native parameter and meter lookup now uses the lowest KSP script slot for
duplicate identifiers, independent of publication order. The synthetic
unordered slots 4/2/3 regression fails with last-wins lookup and passes with
slot 2, matching the official [expose_controls contract](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/user-interface-commands).
The shared scanner adapter observes successful Native consumption instead of
retaining its old hardcoded false value; the common CLI remains unchanged.

PNG, JPEG, WebP and SVG now select the same requested strip frame/window and
device-size preparation. Whole-image non-streamable views share their decoded
Arc; selected views retain only their bounded pixels. The synthetic check
covers both strip axes, frame clamping, window overflow, downscale, cancellation
and alpha preservation. The merged UI suite passes 94 tests with 11 ignored.
Whole-worker memory and corpus acceptance remain open as listed above.

The fresh common-driver **debug** capture at `d0354672` loads Conflux but aborts
the Native graph at its unchanged 250 ms time guard, before image decoding.
There is no updated Native screenshot from that run. Its fixed diagnostic is
`NativeUI graph, NativeUI time budget exceeded`, digest `669a58cce2375280`.
The optimized three-item results above apply only to their recorded earlier
adapter, not to the latest branch. No guard was relaxed for the fixture.

Native readback now applies only to the addressed source and gives its typed
snapshot precedence over scalar fallback. A conflicting scalar value formerly
replaced the synthetic typed text and rewrote other script slots; that check
fails before the fix and passes after it. Native meter maps use the existing
per-source `PartShared.widget_meters` provider. The only new non-owned seam
edit is the meter-map argument at the Native loop in `src/ui/part.rs`.

## Pacific shared witness and current validation hold

The render auditor's `audit/ui-render-20261008@9791421e` consumes the shared
49-ID Pacific witness at product `9993db69`. Pinned v1 digest is
`870cea2140b5c9db5361831664966848302a2f57e82fcb6c7ed1b3534545ce5e`;
v2 digest is
`19e2f2c76771ceeb1f5db47956d404a290e16ef61dabb6e34909176b362b6751`.
These supersede the installed scanner versions for future comparisons; the
earlier three-item results above keep their original provenance.

All 49 v2 views are legacy-authored: 45 missing-images and four original-ok.
253 requested lookups resolve 196 images, and all 196 decodes succeed. The
57 failed lookups comprise 33 instruments with one miss and 12 with two.
The four successful instruments are the Cluster Risers for 10 Cellos,
12 Violas, 16 Violins and 8 Basses. Per-ID audio/note outcomes match; no sound
regression follows from the four silent pairs.

W3 inspected the existing example cache records, not the library resources.
Their `assets` field is a count; missing references are hashes. This cannot
establish physical absence or an own-index namespace mismatch. Shared frontend
`Source::read` currently discards backend resource errors through the `Option`
API, so the fixed `lookup-not-found` counter also cannot distinguish invalid,
ambiguous, inaccessible or corrupt resource lookup. No replacement or alias
artwork is justified. Structured failure propagation and own-index attribution
remain open; the shared collector is unchanged.

Exact available disk space fell to 19.83 GiB, below the required 25 GiB floor.
No W3 heavy job is active and its incremental directories have been pruned.
`f8621061` remains the latest pushed, gated checkpoint. Local `bf4b139a` bounds
rejected Native image-request metadata and cancels package member reads; its
failing-first queue-growth check is recorded, but the green check/build gate
must run after disk headroom returns. Do not integrate that local WIP.

The auditor's follow-up `661bd9d2` confirms no retained own-index/namespace
observations exist in the supplemental historical 7e82 cache either. Its
different path hashes are not equated with current 9993 request identities.
The 57 current failures remain **reported lookup-not-found**, not clean absence.

W10 supplies typed UVI resource errors at `4f033c79`, with origin prerequisites
bundled in `6cfcad4b`. Import through the owner-resolved integration ancestry
`ab818e2d`, not a standalone cherry-pick or a second Bank resolver. W3's local
Source-side preparation calls both providers' Result APIs, preserves absence
separately from invalid/ambiguous/corrupt/limit/read/unavailable errors, and emits
those fixed categories through the existing scanner adapter. Native module
lookup errors retain only fixed category messages. The common collector is
unchanged. A synthetic two-provider found/absent/invalid/failed-authority test
is prepared; integration, compilation and test execution remain pending below
the disk floor. This does not reclassify any frozen Pacific observation.

## Cache/resource checkpoint rebased onto 73e6089b

The pending changes from `822812aa` were rebased onto integration
`73e6089b16ca4d396965ecc305b2ca18ebd166ba`. The scanner conflict preserves
integration's Native font counts, unknown values, source presentation and paint
observations, and adds the fixed typed lookup categories. This supersedes the
validation hold above. The current disk rule permits builds down to 18 GiB;
25 GiB is a prune trigger.

Focused checks pass: root library `cargo test --features shots --no-run`, the
Native queue/cancellation/readback fixture, the two-provider resource Result
fixture, and the one-frame worker fixture. The frozen failing-first queue check
retained 5,001 names after 5,000 rejected requests; the green check retains one.
The real Conflux field-fit test passes with `RUST_MIN_STACK=33554432`, matching
the scanner thread stack: all six complete seven-character names have a 44 px
viewport, 44 px advance and zero host insets. Its initial run with the default
test-thread stack aborted during layout; no Native runtime budget was changed.

Paired focused scans use the frozen integration scanner at `73e6089b` and the
rebased candidate, both with the unchanged shared driver/collector. Baseline
binary SHA-256 is `adc880df63739f623cde612f4ca5275a3ca24c2a118a295ab7581e45f9ca02a5`;
candidate binary is `0af5624f9fe0a86bc7292ddf36710358643b2d13dc8c3c9b1bb80a424e2f8494`.
Driver SHA-256 remains `a4157d712f47cb41f3af4ef008ef5b634cbe21aaba1865ab471452a0f8a9aeb7`.
The `ci` scanner build and shared Python scanner checks pass.

| Candidate item | Condition | Original | Load ms | Peak RSS MiB |
| --- | --- | --- | ---: | ---: |
| Conflux | cold | original-ok | 413.7 | 343.88 |
| Conflux | os-warm | original-ok | 464.2 | 344.34 |
| Big Screen | cold | error, unchanged from baseline | 685.9 | 409.66 |
| Big Screen | os-warm | error, unchanged from baseline | 729.3 | 404.09 |

All eight baseline/candidate item-condition cells load and audition audibly.
Both Native Conflux cold/os-warm paints have zero missing images and the same
0.00041924 white fraction; no 250 ms Native budget hit recurred. Every retained
PNG hash matches baseline in its corresponding condition: three Conflux and
seven Big Screen images per cell. Big Screen's first Native program still fails
with `NativeUI meter unavailable`, diagnostic hash `b488581add938231`, on both
binaries in both conditions. Its second Native program paints successfully.
This is a pre-existing blocker, not complete Original UI acceptance.

Evidence and frozen binary/driver receipts live under
`~/.cache/kontakto-w3/cache-ready-73e6/{manifest,evidence}.json`; focused test logs
are in the parent cache directory. The private product cache was empty and
writable before each cell and has been removed. OS page cache is uncontrolled;
these timings do not establish a performance improvement. Screenshots total
7.02 MiB. No full gate, release build or installation was run.

Expected direct repair coverage of the coordinator's frozen 389 missing-image
items is **0/389**: 337 Augmented Orchestra, 45 Pacific, seven Vista. The pending
patch changes request bookkeeping, cancellation and error classification, not
resource search paths. It must not be credited with resolving those resources.
Their root-cause investigation is a separate follow-up: UVI resolver findings
go to W10; Kontakt resolver defects remain W3's scope.


## Native lowering stack safety

The default 2 MiB Conflux field-fit test aborted before painting. GDB found
recursive `native_ui::draw` frames reserving 103,480 bytes each in debug,
plus iterator/collect frames; the old 32 MiB diagnostic thread measured
3,058,256 bytes during draw/layout. Resource resolution and image decoding
were outside that overflowing call chain.

Lowering now uses a heap work stack, heap completed-child results and boxed
modifier continuations. It preserves child order, inherited style, primitive
construction before decorator callbacks, and background/overlay/popover order.
The VM's node/depth limits are retained. No real UI thread stack was enlarged.
Changed owner files: `src/ui/native_ui.rs`; numeric audit observations only in
`src/ui/scan.rs`. No sampler, plugin shim or other owner's file changed.

Linux plugin and standalone frames run through baseview's plain `thread::spawn`
(`vendor/moose-baseview/src/platform/x11/window_thread.rs:136`), with Rust's
2 MiB default unless the environment overrides it. Its event loop calls
`handler.on_frame` at `x11/event_loop.rs:226`. CLAP parent setup calls
`editor.open` at `vendor/moose-clap/src/lib.rs:4845`; VST3 does so at
`vendor/moose-vst3/src/lib.rs:3402`. Windows creates its window on the caller
thread (`win/window.rs:960`) and paints in its message handler (`:387`). macOS
requires the main thread (`macos/window.rs:38`). Their host/main-thread stacks
are outside our control; these are source findings, not host-stack guarantees.

The frozen shared census identified 51 Native item IDs, all in Conflux.
The unchanged shared driver measured every ID with the numeric depth extension:
51 rendered program graphs tie at **24 primitive/decorator edges**. Another
50 first programs fail at the existing meter service before producing a graph;
their depth and stack remain unknown. Conflux itself and Big Screen program 1
are tied deepest measurable representatives, not proof about unavailable graphs.

Linux watermark measurements include Native Session initialization, Lua graph
generation and four draw/layout passes on the normal test stack:

| Program | Debug peak bytes | Optimized CI peak bytes | Depth |
| --- | ---: | ---: | ---: |
| Conflux, program 0 | 836,960 | 204,071 | 24 |
| Big Screen, program 1 | 836,960 | 204,071 | 24 |

No watermark saturated. The marked range leaves guard pages and 64 KiB below
the live measuring frame untouched; both peaks exceed that unmarked interval.
Debug uses about 40% of 2 MiB, leaving about 1.20 MiB. Optimized shared-scanner
UI/CPU-paint paths additionally measured 204,119 bytes for every successful
Native graph. The scanner's debug Conflux view hit its unchanged 250 ms VM
budget, so that failed paint is not presented as a complete debug stack probe.
These observations do not measure live host/GPU callbacks or arbitrary popup
states; they remove the identified depth-multiplied Rust lowering frames.

Acceptance: the synthetic 64-level graph aborted on an explicit 2 MiB thread
before the fix, and passes on that same explicit stack in debug and optimized
CI. The real six-field Conflux fit test now passes on the default test stack,
with complete saved values and 44 px frame/viewport/advance. Focused real stack
probes and root shots `cargo test --lib --features shots --no-run` pass.

Cold and OS-warm optimized Conflux and Big Screen scanner probes use the frozen
shared driver and baseline READY cache binary. All **20 PNG SHA-256 hashes**
match byte-for-byte: ten in each condition. Conflux remains Original OK with
zero missing images and white fraction 0.00041924398625429554. Big Screen
program 1 paints; program 0 retains meter diagnosis `b488581add938231` on both
builds. OS cache was uncontrolled; these are product-cache condition labels,
not flushed-cache performance claims. No big gate, release or install ran.

Evidence: `~/.cache/kontakto-w3/stack-safety/{manifest,evidence}.json`,
`depth-ranking-final.json`, `png-parity-{cold,os-warm}.json` and focused
`stack-*-watermark-final.log` / `stack-synthetic-*-green.log` in the W3 cache.
The shared collector CLI/metrics and driver stayed unchanged. The scanner was
built at 74718ad7; later commits add tests/reporting only, with unchanged
production lowering and scanner code.

## Native binding and telemetry checkpoint (W3)

The shared Native bridge preserves declared text and arrays when only scalar
telemetry is available. The focused test fails before the guard and passes for
text, integer arrays and real arrays afterward. Valid bindings retain the live
0.625 meter readback and typed edit routing.

**PROVISIONAL (no Kontakt reference capture):** retain the requested face;
unmatched parameters report `connected=false` and ignore edits/touch; unmatched
meters report `connected=false` and no level. Program ownership is unchanged.
NI documents [identifier-only KSP exposure](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/user-interface-commands#expose_controls)
and [unconnected meters](https://developer.native-instruments.com/komplete-ui/docs/Packages/kontakt/Classes/KSPLevelMeter/).
All 12 installed NKRs were inspected; Conflux has 134 readable Native modules.
The two quoted program-0 candidate identifiers reached no binding call during
requested-face initialization/rendering, including tagged indirect flow. This
does not prove inactive branches or Kontakt-observed legacy Native behavior.

This checkpoint does not claim full-editor Native painting or a final N/50
Original-OK count. Those remain a separate, timeboxed investigation.


### Deferred Canvas callback budget

Canvas paint executes after graph construction and layout. Give each deferred
paint callback the same bounded 100,000-instruction/250 ms budget as an input
callback; the graph deadline may have expired while layout loaded fonts.

Failing-first regression waits 300 ms after lowering a Canvas, then paints it:
old code reports an expired NativeUI budget; the fixed callback paints. An
explicit 2 MiB full-editor fixture also paints a nested native graph. Real
Conflux and BigScreen program 1 painted with no native failure categories on
2 MiB threads: debug peak 1,120,160 bytes, optimized CI peak 269,079 bytes.
Receipts: `~/.cache/kontakto-w3/ui-audit-stack/`. The evidence build contained
additional test-only diagnostics; this commit excludes those probes.
BigScreen program 0 and the 50-multi count remain separate pending checks.


### Unconnected native string properties

Keep property return types when a requested face has an unmatched declaration:
caption, tooltip, value text and menu item text return empty strings; the menu
has zero items and no visible item. Other unavailable properties remain nil.
Bindings remain disconnected/inert and meters have no level; no other program
supplies their values. This extends the PROVISIONAL unmatched-binding policy
above; no Kontakt reference capture exists.

Failing-first legacy binding regression rejects nil captions, then passes with
typed empty strings. BigScreen program 0 now paints the requested face without
native failures on an explicit 2 MiB full-editor thread: debug peak 1,120,160
bytes; optimized CI peak 269,207 bytes. The CI area no-run and both explicit
2 MiB lowering/full-editor regressions pass. Receipts are under
`~/.cache/kontakto-w3/ui-audit-stack/`; the evidence build includes test-only
diagnostics omitted from this commit. The corpus count and scanner receipts
are recorded separately under `~/.cache/kontakto-w3/native-caption/`.

### Native evaluation uses work bounds (2026-10-09)

A synthetic finite graph with an injected 300 ms scheduler pause fails the old
250 ms elapsed guard and passes with that fatal guard removed. Its node and VM
checkpoint counts and output text match the unpaused graph. Existing bounds are
unchanged: initialization 500,000 checkpoints, graph 1,000,000, callback/Canvas
100,000, 16,384 component nodes, depth 192 and 128 MiB Lua memory. Checkpoints
mean Luau function-entry/loop-backedge interrupts, not exact instructions. The
pathological callback loop still stops at zero remaining checkpoints; deferred
Canvas starts a fresh callback allowance. Default and shots regressions pass,
as does the full-editor regression on a plain 2 MiB thread. No CPU claim follows
from these contended checks.

Eight frames each from Conflux and Big Screen programs 0/1 returned successfully
with numeric counters only. Initialization uses 3,762 checkpoints for each.
Conflux uses 1,724 nodes and 243,351 checkpoints on its first frame, then 243,343.
Big Screen program 0 uses 1,668 nodes and 239,813 then 239,805 checkpoints;
program 1 uses 1,724 nodes and 243,069 then 243,061. Conflux therefore reaches
10.52% of the node allowance and 24.34% of the graph checkpoint allowance.
The exact first failing W11 gesture was not recorded; its candidate rerun must
capture that frame/index before claiming full gesture acceptance. Shots now
exports phase-local `native_graph_work` counts, including partial graph failures.

W10 and W3 agree on one policy: deterministic work/node/depth/memory bounds
protect evaluation; elapsed asset/process watchdogs are incomplete operational
observations with preserved stage/counters, not product admission or proof of
missing resources. UVI retains its separately measured numeric allowances.
Receipts are under `~/.cache/kontakto-w3/native-budget/`. The immutable W10
Winds witness is reused, not rerun: both rows are Original-OK with wanted peaks
below 3 MiB and zero cache rejection/eviction/requeue counts. This does not
attribute their historical deadlines to the cache guard.
