# VWinds runtime queue investigation

Investigated 2026-10-04 against installed clean `c3d3094` (KONTRA 0.3.148).
This investigates native UVI playback delivery, not library decryption or
complete Falcon fidelity.

## Confirmed failure and reproduction

The retained user journals report RequestCapacity for Oboe, Flute/Flute2,
Bb Clarinet 2, Contrabass Clarinet and English Horn 2. Flute2 exhausted all 27
pending packets with no prefetched PCM and no recorded worker execution error.
Its later worker observation retained six renderer voices. Source line 859 in
the user's report propagates the Bridge error; it does not identify a failing
DSP instruction or establish an allocator leak.

A production-crate-linked Worker+Bridge replay at 48 kHz/512 host frames failed
with one held Flute2 note both with and without subsequent CC1 sweeps. Sampled
renderer membership stayed at five. These bounded runs exclude the DAW/editor;
wall timing includes scheduler effects and low-priority execution. Stable
renderer membership does not bound Lua metadata, suspended tasks or PCM usage.

`perf` recording was unavailable under existing permissions; no system settings
were changed. A bounded GDB symbol-only sample of the worker identified repeated
ModulationGraph value/cache/edge evaluation and Renderer bus/scope work. Debugger
stops perturb deadlines, so its replay timing is not a performance benchmark.
A proposed whole-table builtin-random scan optimization was discarded for this
failure: the exact Flute2 graph has no builtin-random target edges.

## Exact script and resource ownership

Targeted private inspection used the selected Flute2 program and its own
hornScript3 module, not a presumed equivalent module from another bank. CC1
spawns cancellable smoothing work; zero/nonzero crossings can start/stop
layered notes. Air changes a control scalar. Three initialization tasks continue
generating internal controls while a note is held. No direct sample-resource
load was found on Air/CC1 paths; advanced pitch remapping is a separate path.
No vendor source or access data is included here.

BankResources reuses exact sample aliases and decoded identities. New resource
observations retain the existing owned PCM byte counter, alias count and
revision; the core's generic resident-memory count alone does not measure UVI
resource memory. Start/release/kill-fade totals are emitted commands, not audible
DSP starts or successful completions. Lua reachability retirement, renderer
voice retention and authoritative host-note completion remain distinct.

## Post-failure execution defect

Ordinary controller cancellation must preserve an exported old endpoint while
its replacement prepares. Previously terminal failure used that same path and
could leave a ready worker running or parked on full output, with Player/Lua
resources retained until endpoint retirement.

The correction gives terminal failure a nonjoining execution-stop request after
capturing its first cause. Controller ownership still waits for the Audio
retirement receipt. Generic cancellation keeps its playback contract. Focused
functional tests passed for continued nonzero old-endpoint playback after generic
cancel, stopped live voices after terminal abort, and exact discarded queue
counts after a full-queue worker exits. This fixes post-fault continued execution;
it does not establish that initial throughput is sufficient.

## Diagnostics and current verification boundary

Reviewed source adds fixed scalar observations for Session and Renderer wall
cost, prune-owned GC steps/collections, retained Lua metadata/tasks, emitted
starts/releases/kill fades, resource aliases/PCM/revision and pending UI snapshot
service time. UI service lies outside the existing packet-render timer.
Observations are independent concurrent samples, not a transaction at the
fault frame. Automatic and script-authored GC are outside prune GC timing.
Counters publish before propagating a returned Player failure. Panics are not
phase observations.

Six focused cleanup/mailbox/counter tests and four PanLaw tests passed in the
combined test executable. A subsequent production build includes Linux thread
CPU observations and bound connection Ratio slots. The latter removes repeated
name lookup while retaining the existing recursive values, defaults, live writes
and edge order; it does not lower modulation clocks.

Four bounded production-linked Flute2 replays then isolated the failure. Each
uses one hosted note, 48 kHz, 512 host frames and the same packet/latency limits.
Held-note and CC1-sweep runs completed eight seconds without an endpoint failure
or underrun; their average measured worker CPU was about 3.96 and 3.93 ms per
256-frame attempt. A held note with periodic full UI snapshots also completed.
An actual Air widget drag with periodic snapshots still failed RequestCapacity
around 1.34 seconds: measured CPU averaged 5.49 ms against a 5.33 ms packet
budget, with Session wall time averaging 1.21 ms and Renderer 4.29 ms. Its UI
snapshot service consumed another 53 ms across 13 captures outside packet timing.
These are functional observations under low-priority execution, not controlled
before/after benchmarks or proof of all-bank performance.

Across those tapes, decoded owned PCM remained 285,958,764 bytes, with 302 aliases
and revision 2 after initial publication. Air retained five renderer voices and
three suspended tasks in the captured interval. The CC1 tape emitted bounded
start/kill-fade commands. This rejects unlimited sample spawning as the observed
cause in these tapes; it does not prove every lifetime or controller safe.

Edit admission previously built a complete UI snapshot twice per gesture. The
reviewed correction reads the selected control and its ancestor chain, retaining
raw identity, scope, range, visibility, enabled-state and budget checks before
and after pending Lua work. Full published snapshots still validate all widgets.
An unrelated malformed sibling may now block publication without rejecting an
otherwise valid target edit; that is an intentional change in admission scope.
All eleven focused combined target/Session/Ratio/CPU-diagnostic/cleanup cases
passed. The corrected production-linked Air tape completed eight seconds with
finite nonzero PCM, no underruns and no endpoint failure. Average measured worker
CPU was 4.21 ms per packet; Session wall mean was 0.515 ms. Owned PCM stayed at
285,958,764 bytes. This verifies the scoped correction for this tape, not an
all-bank or native-fidelity claim. A second owned Oboe V2 Air-drag tape also
completed eight seconds with finite nonzero PCM and no underrun or endpoint
failure. Neither replay opens the DAW/editor or establishes sustained polyphonic
performance.

Installed binaries remain the clean `c3d3094` checkpoint at this writing. No
queue enlargement, silent DSP bypass, polyphony clamp or new graph admission is
part of this investigation.


## Native block preparation and shared kernels

Bounded radare2/Ghidra decompilation of retained native loaded text identifies a
parameter preparation path that constructs ceil(N/32)+1 float32 control points
for its ordinary case (nine for a 256-frame packet). Constant scalar and flagged
PCM-rate expansion paths are distinct. Connection/Ratio preparation and typed
mixing/conversion operate on the prepared point arrays. Generation and coverage
belong to the native context; a frame/32 or frame/256 cache key does not preserve
that contract by itself. Initialized protected dispatch data remains unproved.

Our current Renderer still reevaluates scope targets per PCM frame. Indexed
numeric block preparation is therefore an evidence-backed direction, with owner,
voice, write epoch, event segmentation, mapper/converter ordering and failure
checks retained. A blanket lower-rate shortcut would need additional proof.
Decompilation also traces a block-oriented common sample base and a concrete
LoopLabOscillator path containing a necessary scalar interpolation loop. That
concrete type is not evidence of the ordinary SamplePlayer backend; its exact
mode/factory binding remains unresolved.

The shared PCM engine already has runtime AVX2 decoding/accumulation, filters
have SSE2/AVX2 paths, and convolution has vector multiply/add. UVI currently uses
its own render scheduling and storage/interpolation adapter. The ownership target
is shared consumed numeric kernels with format-specific preparation and
lifecycle adapters, rather than identical Kontakt/Falcon callback clocks. No
unified DSP engine or stable external backend ABI is claimed here.

A small exact SIMD change replaces repeated 128-byte MIDI controller predicates
with eight bounded SSE2 loads and a sign-bit reduction on x86_64; other targets
keep the scalar path. It preserves the same validation gate and error order.
An extracted-helper exhaustive byte-position check passed; a bounded isolated
predicate comparison favors the vector path on valid input and favors scalar
short-circuiting for a first-byte rejection. That is not a whole-renderer speedup.
The integrated exhaustive parity and public/registered error-precedence tests
passed. Two authored production plugin scenarios also passed for delayed rack
adoption, keyboard/gain/pan/UI playback, and saving/reopening controls.
