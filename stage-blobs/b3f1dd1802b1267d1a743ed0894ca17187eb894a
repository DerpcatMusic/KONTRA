# Core and MIDI refactor

Build a concise, idiomatic Rust instrument core with explicit ownership of MIDI,
library delivery, note lifetimes, authored parameters, prepared samples and DSP
clocks. Complete coherent production slices, using only quick functional checks with
observable playback value; full instrument/listening validation belongs to the user. This roadmap is open; compilation, serialization and native scalar
traces do not establish complete playback behavior or Kontakt equivalence.

## Contract and evidence

- Cover all CC messages, MIDI2, MPE, ADSR/AHDSR/Flex and per-note processing. Keep physical MIDI independent of library policy.
- Preserve sound and interaction under CPU optimization. CPU telemetry must not change admission, release duration, expressions, routing, quiet-note survival or NOTE_END timing.
- Preserve full-width native data and exact note identity. Quantize only where MIDI1 or KSP explicitly requires it. Unsupported inputs diagnose or reject before claiming executable behavior.
- Prepare storage off-thread. Audio callbacks must not allocate, destroy retained heap objects, perform file I/O, lock or perform unbounded work.
- Keep channel/velocity articulation migration for later. Consolidate distortion and EQ only after the core and its playback evidence are complete.

[Primary references](CORE_MIDI_REFERENCES.md) cover Kontakt, Falcon, Surge XT,
Bitwig and MIDI specifications. The pinned Moose fork and vendor patches define
actual Truce capabilities. The [MIDI contract](MIDI_CONTROL_CONTRACT.md) and
[native controller evidence](CORE_NATIVE_CONTROLLER_EVIDENCE.md) distinguish
verified behavior, local policies and unsupported cases.

## State ownership

| Owner | Responsibility |
| --- | --- |
| Format adapters | Wire/host units, transport precision, exact identities and unsupported-input diagnostics |
| Router | Physical-to-library delivery and Engine-owned MPE configuration |
| `midi.rs` / `MidiState` | Decoding, accepted physical controls, parameter transactions, reset policy and negotiated configuration |
| Library receiver / `group_start` | Raw delivered controls, immutable group predicates and instrument selector clocks |
| `Player` / `host_notes` / `per_note` | Pedals, starts/releases, sample voices, exact roots and persistent note generations |
| `SourceNote` / `Pedals` | Shared live key-up, capture and effective-Off policy without a second MIDI state or note registry |
| `rtc` | Native program-owned note-key source ages and balanced host spans |
| `InstrumentState` | Authored parameters, effective overrides, controller policy and group settings |
| Prepared `Bank` | Immutable mappings/source geometry, prepared route storage and worker handoff |
| `Voice` / envelope / LFO | Sample-time DSP, advancement, mute/bypass and stream-wait behavior |
| `Commands` | Timeline admission, coalescing, retained note snapshots and applied-row ownership |
| KSP | Authored callbacks, script-visible projections and retained work with original ownership |
| Rack | Ordered cross-part DSP/callback boundaries and scoped voice census |
| `ProgramBank` | Sparse saved-bank selection, one physical decoder/raw receiver, canonical note storage and original-program releases |

Physical channel, delivered channel, MPE membership and exact host identity are
separate. A logical note can retain several sample voices, generated children,
pending callbacks and release families. Member-expression freezing, detached
MIDI2 generations and exact host expressions also have separate lifetimes.

Prefer concrete private state, exhaustive enums, ordinary slices and existing
helpers. New types should enforce ownership or an invariant. Extract files for
coherent policy/state owners; do not scatter coupled code into public helpers.
Imported definitions stay in import/creator code. Mutable program/group settings
belong to `InstrumentState`; residency payloads must not become a second authority.
Preserve positional plugin-state serialization.

## Implemented production slices

These are source/integration statements. Selected quick checks are recorded in
the proof ledger; other new scenarios remain unrun.

| Area | Current behavior | Important boundary |
| --- | --- | --- |
| MIDI precision | Canonical attack/release velocity, paired14 CCs, CC88, received RPN/NRPN, physical RPN0/1/2 and dual-zone MPE | Unsupported UMP groups, addresses and native relative RPN0/6 diagnose; relative cents normalization remains qualified in the contract |
| MIDI2 ingress | Full-width native controls use their own path; bank/data/selector CCs follow UMP receiver recommendations | No MIDI1 compound decode of native MIDI2 CC; Program Change currently diagnoses rather than inventing bank switching |
| Controller delivery | Raw library receiver precedes callbacks; accepted modulation receipts/defaults retain separate lifecycle | Ignore/wait must not rewind raw values; negative native defaults mean inactive until accepted input |
| Controller policy | Validated native CC7/10, CC120/123 enablement and ControllerOnly sustain; chronological authored/physical authority; traced default post-program gain/pan | SustainWithoutController modulation availability, other program modes, external sends and complete native tail/activity parity remain open |
| Expressions | Exact host expressions, generation-backed native per-note controls and MPE freeze/reset isolation | Detach/reuse must not transfer old values or terminal identity to a fresh owner |
| Note lifetime | Physical provenance, sostenuto captures, retained release snapshots, active-event queries, stable callback timestamps and actual voice counts | Native deferred-pedal source membership and complete native retirement parity remain open |
| Parameters | Named program controls, shared verified group/instrument units, queued chronological writes and immutable residency compatibility; nonfinite editor overrides reject before replacing prior values or refreshing live state | Unsupported writes must not report successful audible behavior |
| Sampling identity | Explicit Compatibility/Kontakt sampling profile owns preparation and kernel selection independently of private MIDI options; saved JSON migration and cache epoch preserve that distinction | Missing authored JSON profile retains historical Compatibility playback; changing profile closes old note owners |
| Local mono ownership | Explicit idle-time per-delivered-channel MonoRetrigger API uses canonical accepted roots, successful source/group receipts and original per-note generations; bounded shared Digital pitch/volume-LFO groups are admitted | Resident/script-free scope; shared pitch requires a declared current host range; no automatic CC126 mapping, generic Kontakt legato, voice reuse or shared AHDSR |
| Render order | Complete each DSP interval before later writes/callbacks; persistent bus/voice clocks; original send/direct ranges; actual input/tap work reaches the Native program amplifier; retained external send tags stay outside main audio/activity | External tag-to-host-output association remains unavailable and diagnoses; enabled unknown main FX conservatively retain tail activity; other native modes and math-library equivalence remain open |
| Rack | All parts finish preceding DSP before ordered boundary work; scoped TOTAL and fresh-part local clocks | Peer census is valid only inside Rack's boundary context; see [Rack ownership](CORE_RACK_TIMELINE.md) |
| Native group selection | Final admission, raw delivered control bank, saved predicates/boolean precedence, round-robin and release-mask ordering | Random sequence is explicit local policy; unsupported records reject rather than becoming Always; Slice remains unsupported |
| Primary envelopes | Ordinary AHDSR/Flex, current-level release, retained fragment clocks, strict live bypass and KSP completion policy | Saved initially bypassed primary sources and unsupported source rosters stay guarded |
| Other envelopes | Bounded AHD, nonvolume bypass, Flex v12 OneShot/Loop and vector-owned point indices | Retargeting, more saved modes and general shared AHDSR remain open |
| LFO/Glide | Verified timing, strict Digital waveform/sharing, saved synchronized count floor, live volume target intensity and retriggered/shared synchronized Digital frequency with native units/getter drift and retained phase, legacy source fade and bounded Time/Speed Glide | General waveforms/free-running geometry, unsynchronized shared frequency and wider native shared scheduling remain limited or unsupported |
| Native RTC | Program-owned shared note-key table, final ordinary admission/reset ordering and balanced spans <=1024 frames | Fresh unseeded cells are deterministic local policy; downstream shaping of raw factors above unity remains unproved |
| Loops | Bounded counted/ordered forward loops, negative infinite counts, physical release phase and verified integral crossfades | Fractional crossfade, alternating modes and unsupported tuning/geometry remain guarded |
| Samples/streams | Wide pitch, truthful cold waits/underruns, sparse packets, full-generation dense publication and source-geometry upgrade guards | These are not anti-aliasing or disk-deadline proofs |
| Resampling | Native ordinary Sampler/recognized DFD quality0 Lagrange, physical crop support and tiny assets; captured High/Ultra kernels, full physical support and per-point counted-loop packets; Compatibility/unity paths preserved | Positive HQ crossfades, fractional/alternating loops, alias rejection and general native equivalence remain open |
| Wavetable/NativeUI | Verified phase transforms, complete-head dependency guard and requested authored UI integration | See [Conflux audit](../audits/CONFLUX_NATIVE_UI.md); MenuFilter, remaining shapers/sync and macOS remain open |

`Commands` owns quota checks, stop coalescing, snapshot reservation and insertion
atomically. Opaque note handles cannot enter scalar admission. Ordered pending
note observations apply through the queue owner. Retained release snapshots are
required after KSP event recycling; compact rows do not promise a smaller
worst-case snapshot reserve.

RPN0 owns its numeric bend sensitivity and original coarse byte together. Legal
cents100..127 cannot silently change the baseline of later fine-only writes.
Received controller delivery now carries a non-copyable decoded receipt through
routing, rather than separately reconstructing a parameter at final delivery.

Live pedal transitions and effective-Off hold checks now share a private
`Pedals` owner; queued scripted root capture uses the same transition policy.
Onset and live modulation project from the same borrowed `MidiState`, retaining
caller-owned script/per-note/RTC inputs. Dense and sparse workers share one
bounded mapped decoder with borrowed scratch.

The native AHDSR consumer separates interpolation history from Source state.
The bounded shared-primary provider now seals accepted direct plugin ranges and
binds exact live start/Off receipts before voice publication. It preserves native
selective state and independent consumer histories. Direct static MPE routing now
projects the same canonical role-reset and pedal policy, preserves first-input
prefix/configuration ordering, and bounds prepared reads for negotiated member
and manager bend ranges. Unchanged-role MIDI1 RPN0 edits project through copied
canonical decoder state; source sealing retains every sensitivity maximum across
the full span before publication. Role-changing setup, general KSP, looped and streamed shared
sources remain unsupported. Actual plugin PCM, pedal/restart
timing, exact endings and callback heap checks passed within that narrow scope.
The [shared-source design](../audits/SHARED_AHDSR_DESIGN.md)
records those admission limits.

The requested [NativeUI commit](https://github.com/DerpcatMusic/KONTRA/commit/4da7064)
is integrated into the dirty working tree while retaining newer core work. This
is not merged Git ancestry. Authored rendering, KSP bridging, MIDI learn, meters,
Lua sandboxing, conditional/sparse layouts and verified transforms are included;
prior UI captures do not establish current audio equivalence.

## Remaining implementation

### Active integration — 2026-10-05

Normal-quality Sampler/DFD crop support passed the repaired native complete-PCM
comparison through actual saved/cache RAM and streaming paths. The expanded
check exposed and corrected a short-loop physical-overshoot defect. A crop
fixture was corrected to the integer source offset actually admitted by the
public microsecond API; quarter-origin kernel captures remain separate evidence.
Playback retains physical taps outside the authored
crop, zero padding outside the physical asset, tiny assets and existing sparse
packet ownership. Unsupported DFD directions/crossfades are checked against
retained authored geometry before normalization can erase those flags.
Initial preparation and later zone remapping now share source-identity
admission: native ordinary modes0/3 require a supported quality, and mode9
requires its decoded wavetable owner. Unknown/mismatched modes retain diagnostics
instead of silently reaching the Compatibility kernel. Invalid prepared native
quality edits are rejected as well. The complete saved/cache playback and atomic
remapping check passed; it does not implement Time Machine or other modes.

An explicit sampling profile now separates source preparation/kernel selection
from private MIDI controller options. Actual saved JSON migration, NI import,
cold cache and complete PCM checks passed; authored JSON without the field keeps
Compatibility playback. A local, initially resident/script-free MonoRetrigger
API selects the last accepted held root per delivered channel. Its fallback
reuses canonical note owners and retains exact expressions and terminal identities.
Review fixes cover scoped cleanup, successful-group receipts, dying tails, batch
MPE cleanup and rejected same-key prospective inputs. Source EOF/wrapped identity,
pedal reuse and admission-pressure PCM checks passed. The main scenario matches
its independent Poly schedule exactly in each tested partition and both lane
configurations; the separate cross-partition comparison uses a derived bound for
existing fragment-end voice-compaction summation order. Rejected/compatible bank
updates were then found to clear the mode prematurely; the repair passed its
own complete PCM check through real stale-parent rejection, compatible resident
fill, fallback and actual streamed replacement. This remains a
bounded local API, not generic Kontakt mono/legato equivalence. Pure shared Digital
volume-LFO mono now also matches a manually scheduled Poly reference bit-for-bit
through fallback, controls and idle reuse. Its fragment schedules retain identical
host generation boundaries. Shared pitch is now admitted through a noncommitting
preview of that declared range; rendering regenerates uncommitted preview points
after same-frame edits. Its independent Poly PCM and failed-candidate isolation
checks pass. Shared AHDSR remains excluded from local mono.

Scoped CC120 and MPE role cleanup now include exact host roots when reconciling
the logical held-key latch. An unrelated physical root delivered to the same
channel retains its public key state and later audible behavior. The complete
PCM check also preserves the distinct audio-ending and physical-root-ending laws.

Live volume-LFO intensity and ordered signed/unsigned restoration passed complete
PCM checks. Retriggered Digital frequency passed actual KSP automation, native
getter drift, paired-source phase, bypass/mute, restoration and invalid same-callback
readback checks. Strict synchronized shared frequency also passed complete PCM:
queued writes preserve committed source points, while future generations consume
the updated count and tempo. Its pending getters and applied settings are checked
at their actual boundaries.
Plain Stereo Modeller carries actual stateless input work into the native program
amplifier; its expanded idle/reseed reference passed without changing Stereo DSP
arithmetic. External send tags now survive native parsing/cache and stay
outside main PCM/activity; unavailable host associations diagnose explicitly.
Invalid editor overrides preserve valid settings and playback. Native inner
program-slot IDs are retained losslessly; this prerequisite does not implement
switching. Root runs narrow gates after production owners freeze; earlier
production builds do not cover every later change.

### MIDI and note delivery

1. Complete live ownership boundaries for effective Off, physical key-up, pedal release, pending scripted roots and terminal host generations. Reuse the actual policy for future accepted-range projection; do not mirror it.
2. Preserve all CCs as authored data while closing verified controller availability/reset semantics. Generic sound-controller numbers must not silently rewrite authored envelopes.
3. Close Local/Omni/Mono/Poly policy and program/bank switching only with concrete instrument/rack behavior. Channel-mode release effects are separate from implementing those modes.
4. Keep manager, member and exact expression combination rules explicit. Preserve freeze through pedal tails, member reuse, transposition, generated children, wait/ignore and same-ID reuse.
5. Finish exact pressure/brightness and supported native parameter paths only when retained data actually reaches DSP. Extend advertised transport capabilities only after complete consumption.

### Modulation and envelope processing

1. Extend the implemented bounded shared AHDSR provider only with real sealed accepted ranges and live receipt binding. Direct MPE and unchanged-role RPN0 sensitivity edits are admitted; role-changing MPE, general scripted, looped and streamed combinations remain open. Source waveform, prepared geometry, interpolation history, pending countdown and clone-local completion are distinct owners.
2. Do not forecast arbitrary KSP branches, move callbacks early, expose future physical controls or emit PCM before required native source-order decisions are known. A narrow direct batch is not complete plugin support.
3. Close remaining retrigger/free-running LFOs, phase/delay/fade/sync, live timing, retargeting and envelope combinations from verified evidence. Keep unsupported geometry explicit.
4. Preserve sample-clock smoothing and advancement under fragmentation, mute/bypass, stream waits and effect processing. Getters must distinguish baseline, queued/effective state and unsupported metadata.
5. Extend mono/legato, held-key priority, voice reuse and retrigger only according to actual authored/native policy. Factory Unisono-Portamento evidence does not justify a guessed generic polyphony-one mode.

### Source quality and instrument behavior

1. Extend only independently verified source geometry. Normal crop/DFD and High/Ultra Sampler/DFD kernels now have focused complete-output evidence; remaining combinations require their own actual geometry and receipts. No four-tap substitution for HQ, fixed footprint cap or pitch clamp.
2. Complete fractional/alternating loop and remaining crossfade/tuning behavior before extending live controls. Preserve actual cursor phase through RAM/streaming and source-end fades.
3. Close cold readiness, offline errors and deadline behavior without erasing attacks or concealing failures. Offline failures must propagate through Engine, CLI and patched CLAP/VST3 adapters.
4. Close full native signal placement, output routing, inserts/sends, smoothing, panner and module-tail activity. Stable voice victim ties are implemented; custom priorities and native DSP-age equivalence remain open.
5. Add independent pitch/time or tempo engines when needed; ordinary speed playback is not a substitute. Add stable host parameter identities and useful editing over the same authoritative core state.
6. Diagnose the supplied Symphony GUI/release-selection case with its actual library and logs. Current unavailable attachments are not a reproduction; the screenshot remains unverified.

### Visual asset ownership

KONTRA view now releases decoded library artwork it no longer draws. Vector view
retains compact authored geometry/control metadata and replaces additional known
faces; arbitrary NativeUI artwork remains explicit visible demand. One worker per
part coalesces demand, reuses retained images and guards publication by generation,
script epoch and demand revision. Lua sessions own source modules independently
of images/fonts. Hidden/removed/closed editor views retire demand.

Actual PNG/filmstrip and live Lua checks passed through release and restoration,
including bound slider values, geometry, stale completion and unaffected sibling
assets. The vendored renderer sweeps obsolete CPU/GPU upload copies even on
image-free or partial frames; its actual CPU pixel/lifetime check passed. These
prove payload ownership, not instantaneous OS RSS reduction or GPU atlas capacity
shrink. In-flight decode may retain payloads until its guarded completion.

### Deferred articulation and DSP work

After core completion, migrate keyswitch policy to stable channel/velocity
selectors independent of physical MIDI. Bind selectors to instrument/program and
preserve original key passthrough, musical velocity and note-owned release
selection. Replayed keyswitch pulses are not a universal migration guarantee when
libraries share globals or use MPE member ownership.

Then consolidate repeated distortion/EQ DSP: shared waveshaping kernels and
configurable EQ/filter primitives, composed into each model's actual topology.
Preserve signal order, gain staging, state, oversampling and units. Sharing a
kernel does not prove model equivalence.

## Verification and compiler checkpoints

The user now permits quick functional checks based on real behavior, while
reserving full instrument/listening validation for themselves. Root runs compilation
and selected checks centrally after source owners freeze. Prefer actual event
input to PCM/lifetime output and independent references; do not run the broad
suite or add assertions that merely mirror implementation arithmetic. Unrun
fixtures remain staged and must not be described as passes.

The latest compile-only library/playback harness passed on2026-10-04 in2m31s.
Selected actual output checks now include Normal/High/Ultra Sampler/DFD domains,
failed loop-domain isolation, mono fallback and residency ownership, external
send exclusion, finite overrides, retriggered/shared synchronized live LFO
frequency and scoped exact-note held-key preservation. The corrected source
admission and saved-bypass scenarios passed through real accepted init edits.
Mono shared pitch/volume and direct shared-primary MPE sensitivity checks now
also pass complete PCM, original exact endings and callback heap guards. Saved
Sampler nonreflecting rows, ordinary exact-root storage ownership and image/Lua
fixes pass their targeted checks. The preceding no-default library passed
in34.82s and default library/CLI in1m02s; those production checkpoints predate
some newer harness changes. Bank coordinator and further parameter work remain
in progress and are not covered by these results. An older default
artifact passed the exported VST3 brightness/PolyPressure host comparison;
that result does not validate the newer source changes. Selected results and
their limits are in the proof ledger; this is not a broad-suite or full
instrument/listening-validation result.

[The performance proof ledger](CORE_PERFORMANCE_PROOF.md) separates older measured
results from current unrun scenarios. Required meaningful proof uses whole
input-to-output PCM and independent references: twenty notes, release fifteen and
hold five, pedals, expression/automation, stop, complete tails, exact identities
and NOTE_END. Cover fixed/irregular blocks, direct/scripted delivery, wait/ignore,
mute/unmute, bank replacement and callback heap guards as applicable. Do not use
a numeric test total as evidence of a proper instrument.

Earlier measured CPU-load equality through200% and fragmentation differences
below1.5e-8 apply only to their recorded checkpoint. Current fixtures, compilation
and native serialization/scalar traces do not inherit those playback claims.
External CLAP/VST3/macOS execution, hardware deadlines and controlled native audio
comparison remain separate requirements.

## Storage retention

Keep the current successful build, active outputs, reference ledger and musical
evidence. Remove obsolete known KONTRA debug artifacts and superseded own logs
after checking active processes; preserve shared targets and unrelated work.
Use bounded compiler logs and check free space before costly builds.

On2026-10-04, 922 stale import-cache files from superseded importer epochs were
removed after checking magic, age and filesystem identity, reclaiming9.8GB of
logical cache data. Recent caches, library indexes/headers, source files, active
targets and current diagnostic evidence were retained.
