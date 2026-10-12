# Recorded player behavior — 2026-10-03

These checks record actual stereo output from `Sampler::process`, with explicit
incoming note/controller/expression events and independently known signal changes.
The source is a continuous 240 Hz sine at amplitude 0.02 plus its 480 Hz harmonic
at 0.006. They run at 48 kHz, through direct playback and pass-through KSP scripts.
They measure complete transitions as well as settled output, and check that audio
processing allocates and frees no heap memory.

The recorded results below apply to the 2026-10-03 checkpoint. The subsequent
core refactor has staged new scenarios separately below. The user now permits
quick functional event-to-output checks; full instrument/listening validation
belongs to the user. Unrun scenarios are not measured passes.

## Musical results

The chord has five octave pitches with four phase-aligned exact owners each.
Releasing three owners per pitch must divide its RMS by four, independent of
the sampler's gain curve. Measured results agree in both script configurations.

| Input stage | Observed stereo RMS | Exact note ends so far |
| --- | ---: | ---: |
| Start 20 notes | 0.03981162 | 0 |
| Release 15, retain five | 0.00995290 | 15 |
| Sustain down | 0.00995290 | 15 |
| Release the last five under sustain | 0.00995290 | 15 |
| Sustain up, after tails | 0 | 20 |
| Start another owner | 0.00370876 | 20 |
| Choke that owner | 0 | 21 |
| Late expressions, CC and +6 dB automation | 0 | 21 |

Every original address ends exactly once. The later owner also ends once; late
events cannot resurrect it. Fixed 128-frame and irregular 1/7/31/127/13/64-frame
renders differ by at most **1.4901161e-8** across the complete chord recording.

The second recording starts two same-pitch owners, panned independently left and
right. A +12-semitone expression moves only the left fundamental from 240 to
480 Hz (measured amplitude 0.00502377); the right fundamental remains 240 Hz.
Left-only gain 0.25 changes RMS from 0.00370876 to 0.00092719. Unassigned
CC1/11/74 leave the dry amplifier unchanged. Master automation of -6.0206 dB
halves both channels. After releasing and replacing the left owner on the same
channel and key, expressions addressed to the old owner do not tune or mute the
replacement. Complete fixed/irregular PCM differs by at most **2.3283064e-10**.

## Failures that drove changes

Live instrument and voice gain/pan previously used each processing fragment's
length as their ramp duration. The expression recording reproduced a peak PCM
difference of approximately 0.00052. Both paths now retain a 5 ms sample clock
and retarget from the reached value. The voice curve stays at the native
Amplifier position, preserving nonlinear insert and pre/post-send ordering.

The first chord comparison also differed by 0.02822349 during release. A direct
engine probe reproduced that exact difference when forcing overload: this was
the existing real-time CPU policy applying a 5 ms fade to released voices.
Tiny debug callbacks exceeded its measured-load threshold. That CPU-driven
behavior has now been removed, including the lower-load quiet-layer cutoff.
The recordings run in real-time process mode. Cycling reported load through
0.69/0.7/0.85/0.9/1.0/2.0/0.0 produces **bit-identical complete PCM** to zero load,
with identical exact note ends and their sample-frame timestamps, in both direct
and scripted playback. This injects reported load; it does not claim the test
machine physically met a 200% real-time deadline.

Heuristic program/bus effect sleeping also discarded quiet signals. Regressions
failed before its removal: authored gain stages should amplify a 1e-7 input to
0.0032768, but the chain returned the original 1e-7; an IR reflection at sample
3000 should reach 0.0004096 after amplification, but program and bus paths returned
zero. All three audio checks now pass, including the complete quiet IR response.
Convolution keeps its full IR and exact-zero FFT/history shortcuts.

Zero-gain filtered voices previously cleared processor state immediately. A short
mute/unmute produced peak errors of 0.10054535 during mute and 0.18157868 after
unmute with the amplifier before the filter, or 0.15239868 after unmute with the
amplifier after it. Filtered voices now keep ordinary processing; the paired
reference checks verify audio and source-clock state without heap operations.

Ordinary amplitude envelopes also used a raw `1e-4` quiet floor for held
sustains and release completion. A sustain of `5e-5` with valid gain 10000
should produce the same 0.125 PCM as sustain 0.5 at gain 1. Before the change,
the quiet held voice retired and its release ended after one sample, with peak
errors of 0.125 and 0.12428252. Positive ordinary sustains now remain held;
release completion uses the captured starting level, independent of gain.
The exponential law remains -60 dB over the authored release time and completes
at a relative -80 dB endpoint. Duplicate note-offs retain that endpoint. Native
amplitude and pitch/filter modulation envelope laws are unchanged.

The modulation lag audit exposed another quiet-input shortcut: snapping a
positive-lag residual of `5e-7` directly to zero changed amplified engine PCM
by 0.00031292439. The independent reference uses the exponential lag law and
the same sample-time gain ramp. Exact equality may skip work; a small positive
residual cannot bypass its authored lag. After removing the epsilon snap, the
rendered comparison has zero PCM error.

The reverb LFO audit measured the callback-sized interpolation against a
per-sample reference over 8–192 kHz, multiple block sizes, driven/tail phases and
freeze. Maximum relative error was 0.00014732474 and final output/block-partition
error was 0.00011616945. These numbers alone do not prove total inaudibility. The callback-sized
interpolation has been removed; processing now advances and evaluates the LFO
per sample while retaining the SIMD delay/feedback kernel. The updated matrix
now matches the scalar reference and every callback partition exactly, including
the tail and freeze transition.

## Replay and artifacts

```sh
KONTRA_PERFORMANCE_ARTIFACT_DIR=artifacts/core-performance RUSTC_WRAPPER= \
  cargo test --lib plugin::performance_tests -- --nocapture
```

The artifact directory is ignored by Git. It contains stereo float WAV recordings
and JSON input/stage timelines for both block partitions and script configurations.
The current recording is [chord-script-128.wav](../artifacts/core-performance/chord-script-128.wav),
with its [input and measurements](../artifacts/core-performance/chord-script-128.json).
The load comparison is [zero load](../artifacts/core-performance/chord-script-128-load-zero.wav)
and [varying reported load](../artifacts/core-performance/chord-script-128-load-varies.wav):
the WAV files are byte-identical, with identical owner-end timestamps in their JSON.
The expression recording is [expressions-script-128.wav](../artifacts/core-performance/expressions-script-128.wav).
The measured result is also shown in [performance-proof.png](../artifacts/core-performance/performance-proof.png).
The plot is an inspection artifact; the Rust tests produce the WAV/JSON evidence.

The harness uses the framework's **real-time process mode**, a resident sample,
and the DSP entry point.
Parameter events pass through the actual `Params::set_plain` API before DSP,
matching the CLAP wrapper contract. It does not instantiate a native CLAP/VST3
wrapper or prove hardware deadline performance, device negotiation or Kontakt parity.
The filter/amplifier placement checks and integrated playback checks also pass.

## Attachment-driven compatibility fixes

The attached session reproduced initialization stopping at
`set_listener($NI_SIGNAL_TIMER_MS,-1)`. A focused check failed before the fix;
the shared validator now interprets exactly this sentinel as a disabled MS timer.
Activation, cancellation, beat phase, transport subscriptions and neighboring
invalid values are checked. This is a narrow legacy compatibility policy:
[NI's time-command documentation](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/time-related-commands)
documents zero disabling and the minimum active interval, not the -1 sentinel.

The missing instrument GUI was traced to ordinary versus Windows verbatim NKI
paths. Shared `Instrument::matches_path` accepts equivalent drive/UNC forms in
the UI and auto-alignment. A rendered control regression failed before the fix
and passes afterward, including live interaction and stale program/replacement
guards. It performs no filesystem work or case folding.

Incorrect release-sample selection in the reported library is **unverified**:
the session confirms imported zones and UI assets, but contains no sufficient
note/group execution trace and that instrument is unavailable locally. Rejected
native AHDSR behavior and deferred callbacks remain possible contributors, not
established causes. Synthetic correct note endings do not prove its release-group
selection. Full physical-channel modulation isolation, velocity precision and
MPE lifecycle work remain in [the working plan](CORE_MIDI_REFACTOR.md).

## Quick functional checkpoint — 2026-10-04

The user permits small checks with observable playback value and reserves full
instrument/listening validation. The following run against the centrally linked
`ci` library harness after decoded receipt, pedal, MIDI projection, mapped reader
and AHDSR ownership changes. They do not validate later High/Ultra or other ongoing
source work, native sound equivalence, external hosts or the whole instrument.

| Behavior | Observed result | Scope/time |
| --- | --- | --- |
| RPN0 coarse/fine chronology | Complete stereo PCM matches independent literal tuning, including reset/reselection, native replacement, physical4/7 and exact owner reuse | Passed,0.05s |
| Curved wide-pitch streaming | Actual disk/RAM PCM, pitch/filter state, fragmentation and callback heap guards agree | Passed,0.07s;1536 frames |
| Scoped scripted sostenuto | Exact and anonymous roots follow queued MPE-manager pedal capture, including generated-child distinction, complete PCM and callback heap guards | Passed,0.03s |
| Stale dense publication | A real stale worker after65536 configuration changes cannot start or corrupt the fresh owner; subsequent output agrees with RAM | Passed,0.07s;4096 frames |
| Rejected NOTE_END then physical stop | Actual plugin processing preserves the earlier terminal timestamp through a rejected host publication and later stop; retry publishes it exactly once | Passed,0.12s |
| Saved v73 LFO fade | Actual NKI/cache and complete PCM preserve the saved clamp mode through fragment, bypass and live changes | Passed,0.05s |
| Native High/Ultra landmarks | Fourteen captured native boundary/wide-pitch outputs agree with the convolution kernel | Passed,0.00s; scalar landmarks, not complete native instrument equivalence |
| Rack20-to5 lifecycle | Full stereo PCM, routed buses, meters and exact note endings match the independent schedule across fixed/irregular partitions; forced-stop sends end with the voice | Passed,0.03s; measured FIR overlap residual4.1909518e-10 at1093 in both partitions, exact silence1280..1408; no broader Rack execution claim |
| Offline failed-source ownership | A disappeared WAV fails actual plugin processing, retains its original part/generation through later blocks and bank replacement, and recovers only after reset; genuine silent PCM succeeds and the recovered known source sounds | Passed,0.08s; actual ProcessStatus/diagnostic/output and zero callback heap checks; external CLAP/VST3 execution remains separate |
| Native High/Ultra complete PCM | Four independently captured native source trajectories, RAM/disk, filtered/unfiltered and fragmented complete output agree | Passed,1.82s; fixture now includes exact1/32 DC passband excitation and freshly captured native arrays; original tolerance retained |
| Selected kernel prefetch | Pending future publication survives selection; cold/late silence and subsequent full disk/RAM PCM recover at the independently expected source cursor | Passed,2.68s; authored64x ratio corrected, independent clock/EOF expectations retained |
| Synchronized LFO timing | Actual NKI/cache and complete stereo PCM preserve prepared phase/fade countdown across tempo thresholds, paused/fresh notes and authored group refresh | Passed,0.10s; existing Engine32-frame source adapter, not native wrapper-span equivalence |
| Image-free renderer transition | Actual image pixels render, then vector pixels replace them and the obsolete premultiplied pixmap Weak owner expires | Passed,0.00s; CPU renderer payload, not GPU atlas capacity |
| Cropped HQ NKI/cache path | Genuine saved Sampler source identity survives NKI/cache; cropped High/Ultra complete PCM matches native captures through RAM/streaming and retains full physical filter support; incomplete prepared upgrades reject | Passed,0.13s; fixed writer source-mode loss and fixture directory ordering, cache and residency guards unchanged |
| Saved source identity | Authored mode0/3 and legacy None preserve the complete private tail; unsupported modes reject before replacing an existing file | Passed,0.00s; NKI serialization/identity check, not an audio equivalence claim |
| Synchronized frequency count floor | Saved negative, zero and fractional counts retain authored bits and match independently timed effective-one complete PCM across cache, tempo refresh, bypass, depth, mute, release and tail | Passed,0.17s; strict admitted Digital source layouts, two saved fade versions; no version/waveform broadening |
| Native tuning callback ownership | Native absolute/relative RPN1/2 update later fine-only MIDI1 script callback values consistently with actual tuning; null/other selectors, physical owners and exact note reuse remain separate | Passed,0.02s; callback-controlled gain matches literal tuning/gain stereo references with exact ends and zero callback heap work |
| Legacy disabled listener playback | Native signed nonpositive MS listener parameters disable both commands; actual initialization, callback timing, complete PCM and settled gain agree | Passed,0.01s; fixture observes the full gain ramp and public omission of disabled listeners |
| Shared synchronized LFO | Saved/cache source points and whole PCM follow independently prepared frequency steps across overlap, cached generations, ignored small BPM changes, idle authored refresh, bypass/mute and exact ends | Passed,0.02s; strict v71 zero-fade layout and local receiver-window contract |
| VST3 brightness consumption | Normalized SDK brightness reaches precise exact-note processing, preserving independent same-pitch owners through script and ignore modes | Passed,0.23s; Rust actual process output; advertisement remains bounded by truthful default metadata |
| Native program gain tolerance | Near-unity targets retain hidden current and branch decisions through declared host windows; complete PCM follows independent scalar output across internal fragments | Passed,0.03s; Native host-window branch law, distinct from Compatibility clocks |
| Native program placement/routing | Complete dry/nonlinear post-program PCM matches the independent gain/pan reference; direct group output stays outside program racks/amplifier | Passed,0.03s/0.02s; real FX installed and resident paired sources; direct output rows may disappear only after exact note completion |
| Prepared envelope/FX and supplied PCM validation | Zero sample rate, corrupt PCM and consumed authored/prepared envelope/FX values reject; repaired banks produce the complete expected stereo output and exact retirement | Passed,0.02s family rerun; every positive rate and the exact internal UNITY sentinel retained, raw authored infinity rejected |
| Live nonfinite expression rejection | Invalid gain/pan/tune updates leave active stereo PCM and endings unchanged, then valid recovery audibly changes the same note | Passed,0.02s family run; solo/lanes, tiny fragments and callback heap guards |
| Atomic filter retarget | Rejected type change preserves complete old-type PCM and lifetime, with meaningful DC/110Hz passband excitation | Passed,0.02s family rerun; bit-exact complete output, unchanged0.001 witness, exact retirement and callback heap guard |
| HQ zero-crossfade loops | Actual saved/cache counted/ordered High/Ultra complete PCM matched the earlier capture arrays in RAM/streaming, with offset and exact source endings | Historical pass,5.04s. Later investigation found the native capture wrote DFD loop rows into unbound storage; that DFD loop-domain evidence is withdrawn pending the repaired full-output rerun. Plain kernel evidence is separate; no NI async-I/O or deadline claim |
| Saved counted/curved lifetime | Handwritten complete physical PCM and source EOF timestamps agree across RAM/stream engines; held roots retain host lifetime until actual late key-up | Passed,1.70s; source-end observation uses cfg-only authoritative pending-end accessor, not predicted clocks |
| Shared AHDSR plugin path | Real sealed direct plugin ranges preserve overwrite-order Off, pedals, restart, consumer history, complete stereo PCM and exact host endings | Passed,0.79s; malformed/unsealed roster rejection passed separately in0.20s; general KSP/MPE/looped/streamed sources remain unsupported |
| Native program activity receipts | Gainer/bypass/tap-only racks follow actual input work, idle restart reseeds correctly, and quiet direct-route group sends remain audible after return gain | Passed,0.06s; complete PCM/ends across tiny fragments and zero callback heap work; enabled unknown DSP tails remain conservative |
| Exported SDK brightness/pressure | Actual exported VST3 consumes SDK Brightness5 and PolyPressure through same-pitch owners, generated children, released families and late retired expressions | Passed; each20480 stereo frames, maximum error0, peak0.115506046, terminal tail0. Real NKI base export plus checked reversible equal-length authored Native path replacement and actual state-parser acceptance; no CLI Native-export or advertisement claim |
| Asset demand and Original restoration | Real decoded/compressed ownership expires, stale completion is rejected, other-part assets remain, bound slider value/geometry restore, and a live Lua closure survives pixel pruning/restoration | Passed,0.60s with settled three-view captures inspected; CPU ownership, not OS RSS/GPU atlas capacity |

The earlier Rack failure exposed a separate rejected-NOTE_END backpressure bug.
The terminal owner now preserves completed timestamps through later physical
stops, and the actual plugin check above passed. The focused Rack lifecycle also
passed after the forced-stop send correction and independently justified FFT drain
boundary. Other Rack scenarios remain unrun. No broad suite ran.

## Current integration checks — 2026-10-04

The latest library/playback harness linked in2m31s. The following narrow checks
ran against that artifact, or the immediately preceding harness as stated. These
record actual behavior; they do not claim full instrument or host validation.

### Follow-up checkpoint — 2026-10-05

The newer harness linked in2m46s and, after a MIDI1 velocity fallback and vector
fixture/validation repair, in2m57s. Selected results below belong to those
artifacts. The sparse-bank ownership case remains under correction: its RPN2
fine-only stimulus incorrectly assumed an audible coarse-tuning change. The
fixture now uses supported CC6 under the retained selector; its literal end
times and PCM tolerance are unchanged, and this latest revision is unrun here.
Native bank amplifier equivalence remains separate from root ownership.

| Behavior | Actual observation | Scope/time |
| --- | --- | --- |
| Supported relative RPN0 cents fields | Literal complete PCM preserves the original semitone byte through representable cents increments/decrements, reset, MPE fanout, frozen release and exact reuse | Passed,0.03s;4096 frames, fixed/irregular fragments and no callback heap work |
| Ordinary plugin RPN0 chronology | Raw/scripted physical sensitivity, supported relative cents, fine follow-ups, reset and null/NRPN isolation match independent full plugin PCM and exact ends | Passed,1.06s; affected existing plugin oracle rerun |
| Shared unsynchronized Digital frequency | Actual saved/cache/KSP edits retain committed generation points and use captured native coefficients in later generations | Passed,0.06s;3072 stereo frames, identical257-frame host windows, exact ends, idle/mute/bypass and no callback heap work; minimum endpoint preparation/readback is not a minimum-Hz audibility claim |
| Shared synchronized frequency regression | The unchanged synchronized path retains committed points and next-generation output | Passed,0.03s; complete PCM and callback heap guards |
| Canonical loan and stale retirement | Foreign/stale root retirement cannot affect the live reused owner; unwind restores the raw receiver loan, then actual playback and END256 complete | Passed,0.04s; deliberate caught panic is part of the restoration check |
| Twenty-note performance regression | Twenty-to-five notes, pedals, stop and distinct routed FX retain chronological full PCM and exact late silence | Passed,0.07s; fixed/irregular fragments, residual4.1909518e-10 and no callback heap work |
| Authored NativeUI vector recipe | Original/vector pixels and measured gesture geometry agree; a real pointer edit reaches KSP as75 with one callback, while decoded/compressed artwork expires and Original restoration works | Passed,0.39s; malformed/version/dimension recipes restore Original demand; unannotated library artwork remains Original |

The ordinary exact-root sequence also passed in0.02s and hidden-image Lua/UI
handling in0.00s. None of these checks establish OS RSS, GPU atlas capacity,
external host execution or a user's full library/listening validation.

| Behavior | Observed result | Scope/time |
| --- | --- | --- |
| Independent sampling profile | Actual legacy authored JSON, explicit Kontakt profile, opaque NI MIDI options and cold NI cache choose independently materialized Hermite/Lagrange PCM | Passed,0.02s latest harness; bounded library/cache dependencies, old cache epoch rejects |
| Profile-only residency replacement | Same-source fill preserves PCM; changed profile closes the old exact root before fresh source/kernel onset | Passed,0.03s preceding harness; original callback heap guard retained |
| Shared synchronized raw count floor | Saved/cache counts1, negative, signed zero and sub-one fractions retain authored bits while complete stereo output follows literal effective-one source points | Passed,0.10s latest harness; strict v71 shared admission, overlap, prepared generations, tempo, bypass/depth/mute, idle and exact ends |
| Shared synchronized live frequency | Actual queued KSP writes preserve committed points/step/phase while later generations use literal native frequency steps; same-callback pending getters and applied settings agree at their respective boundaries | Passed,0.04s latest harness; complete3072-frame stereo output, overlap, idle, mute/bypass, exact ends and no callback heap work |
| Live volume-LFO intensity | Complete4096-frame stereo output matches independent triangle/lag/amplifier points through signed/unsigned writes, bypass, mute, release and ordered restoration | Passed,0.03s preceding harness; original source slots, sparse rejection, fixed/irregular partitions and no callback heap work |
| Plain Stereo work receipt | Actual nonzero Stereo matrix output retains native program-gain idle/reseed behavior | Passed,0.06s preceding harness; five concrete rack variants, exact ends and no callback heap work; Stereo arithmetic unchanged |
| Program Change diagnostics | Typed, paired and bare MIDI1/MIDI2 inputs diagnose once while preserving complete plugin PCM and exact-owner lifetimes | Passed,0.33s preceding harness; unsupported selection remains explicit, no invented bank/CC companions |
| Mono retained roots | EOF/wrapped identity fallback, rejected admission under native release pressure, pedal key reuse and batched MPE cleanup follow independent complete PCM and exact endings | Passed,0.04s current module run; resident/script-free local API only |
| Mono main sequence | Mono matches independently scheduled Poly output bit-for-bit under fixed and irregular partitions, in solo and shared lanes | Passed,0.04s current module run. Cross-partition compaction changes only floating accumulation order; its separate comparison uses the derived two-sum rounding bound, with unchanged exact per-partition PCM and terminal assertions |
| Mono residency ownership | Actual stale-parent rejection and compatible resident fill preserve accepted roots/mode/fallback; correctly rebased streamed replacement closes old source and clears mode | Passed,0.04s current module run; whole stereo PCM bit-exact against independently scheduled Poly, original exact ends and no callback heap work |
| Mono shared volume source | Resident MonoRetrigger fallback, rejected onset, live frequency/depth/bypass, mute and idle reuse match an independently scheduled Poly engine | Passed,0.04s latest harness; complete stereo PCM bit-exact in both fragment schedules, identical256-frame host generations, exact END321/1089 and no callback heap work |
| Mono shared pitch source | Admission preview and real rendering preserve same-frame frequency edits, reject a mapped unsafe source without cutting the selected note, and retain fallback/idle source clocks | Passed,0.03s latest harness; paired shared pitch/volume complete stereo PCM bit-exact against manual Poly, exact END321/1089 and no callback heap work; declared host range required |
| Static MPE shared primary | Complete plugin PCM follows literal shared envelope points through member/manager bends, pressure, CC74, pedals, member reuse and exact identities | Passed,0.22s latest harness; fixed and subdivided host ranges, seven roots, exact endings and no callback heap work |
| Live MPE sensitivity with shared primary | Actual raw MIDI1 RPN0 edits retain member fanout, independent manager sensitivity, frozen release pitch, reset and member reuse against literal full-output pitch/envelope references | Passed,0.26s latest harness; fixed/subdivided host ranges, exact endings and no callback heap work; unchanged roles only |
| Shared-source sensitivity refusal | Role/tuning/ordinary sensitivity changes reject before source/root publication; an earlier large range cannot hide behind a later reduction | Passed,0.20s latest harness; complete zero-PCM/no-root refusals plus the same short-source positive low-range companion; no global pitch cap |
| First MPE setup ordering | Initial host onset, MIDI1 onset and CC88 input preserve original prefix/configuration ordering and clear prior role pedals | Passed,0.17s latest harness; three complete-PCM startup capsules and admission/end checks; velocity bits without an authored velocity route are not an audibility claim |
| Scoped exact held-key ownership | CC120 and MPE role cleanup preserve an unrelated exact physical owner on the same delivered channel; public key state drives a later audible host accent | Passed,0.03s latest harness; complete stereo PCM against independently targeted choke/known accent, exact physical/host end distinctions, expression, sustain, fixed/irregular partitions and no callback heap work |
| Supported source/profile replacement | Same-source fill preserves playback while changed quality, supported Sampler→DFD identity, or profile closes the original exact root before fresh onset | Passed,0.04s latest harness; complete PCM and host END256, no callback heap work |
| Native source identity/quality admission | Real NKI/cache retain unsupported mode5, unresolved/missing mode metadata and malformed quality while supported siblings alone provide complete PCM; invalid target remaps fail atomically | Passed,0.03s latest harness; RAM/bare/supplied playback, Compatibility companions, prepared-quality rejection, accepted snapshot-mode3 init edits and unchanged mapping after rejection |
| Saved primary bypass isolation | Real NKI/cache retain bypassed primary metadata and exclude its unsupported source while supported sibling PCM survives accepted-but-rejected init remapping | Passed,0.03s latest harness; explicit saved sampling profile in RAM/bare/supplied references, accepted snapshot-mode3 edit and unchanged PCM tolerance |
| Native physical sampling domains | Actual saved/cache Normal/High/Ultra Sampler/DFD trajectories match repaired literal native output through RAM and streaming, including counted/release loops, crops and tiny assets | Passed,3.92s latest harness;80 source/quality/storage cases, unchanged native endpoints and callback heap checks; no native async-I/O or deadline claim |
| Required Normal loop-domain admission | Actual saved/cache malformed tiny and crop-crossing UntilRelease domains fail before source advancement while an existing sibling retains complete current/next-callback PCM; valid prepared and Compatibility companions remain audible | Passed,0.10s latest harness; Sampler/DFD, RAM/bare, exact surviving voice count and no callback heap work |
| Nonreflecting alternating row | A saved Sampler count-one, zero-crossfade alternating marker preserves later ordered forward rows and complete PCM against the independently marker-cleared source | Passed,0.41s latest harness; Normal/High/Ultra, actual NKI/cache RAM/stream, offsets, releases, retune, fixed/irregular fragments, endings and no callback heap work; DFD/reflecting rows remain excluded |
| Canonical exact-root storage split | Physical attacks/pedals, exact reuse, late expressions and waited generated/release children retain their original PCM/lifetimes | Passed,0.03s latest harness; no callback heap work; this validates ordinary Engine behavior, not unfinished bank coordination |
| Native image conversion | Actual lossless RGB/RGBA WebP and opaque/transparent SVG decode retain every expected pixel after in-place conversion | Passed,0.00s latest harness; one final conversion allocation replaces the extra full-image copy; decoder internals/OS RSS are not measured |
| Hidden NativeUI image ownership | Real Lua UI preserves visible/hidden geometry and tap callbacks while hidden images take no pixel lease and hidden missing resources do not fail visible rendering | Passed,0.00s latest harness; visible missing images still diagnose, real scene/pointer and Arc lifetime assertions |
| Compatibility source boundaries | Complete disk/RAM PCM, tiny loops/assets and onset/end ramps remain correct | Passed,0.32s/0.01s/0.01s preceding harness; targeted existing integration checks |
| External send exclusion | Actual raw native wrapper selector173 survives real instrument cache and stays outside main PCM; sentinel−1 companion remains audible; delayed external-only return does not hold main gain activity through idle/restart | Passed,0.05s preceding harness; full PCM against independent no-wet/reference amplifier, exact ends and no callback heap work; external host transfer remains unavailable |
| Editor override admission | NaN/±Inf updates preserve prior valid override, effective values, MIDI revision and complete playback; later valid retarget and clear remain audible | Passed,0.02s preceding harness; six independently authored sustain-level references, fixed/irregular output, natural EOF and no callback heap work |
| Retained program slot identity | Actual serialized sparse signed rows retain order/payload; single inner119 inside outer5 still selects outer5; multi-program bank remains explicitly rejected | Passed,0.00s preceding harness; parser/file ownership prerequisite only, no switching/audio claim |
| Live Digital LFO frequency | Actual KSP legal-unit writes preserve paired pitch/volume phase through mute/bypass/release, native getter drift and restored requests; invalid writes cannot shadow a same-callback getter | Passed,0.06s latest harness; complete6144-frame stereo output against independent source/amplifier points and no callback heap work; retriggered Digital only |
| Shared primary/rack/validation reruns | Actual plugin shared-envelope sequence, rack20-to5 pedal/stop/FX sequence and consumed-data/recovery family passed | Preceding harness:0.78s/0.04s/0.05s respectively; rack residual4.1909518e-10 and exact late silence unchanged |

The repaired native capture now binds loop rows to the actual Voice-selected Zone
storage. Native Normal11 pairs and High/Ultra14 pairs agree between source modes0/3
for the captured trajectories. The old unbound inputs are retained and labelled
in [the loop audit](../audits/COUNTED_LOOPS.md). The expanded actual saved/cache
PCM check exposed a real Normal single UntilRelease defect: virtual modulo
rewrapped a pending wide-step overshoot before release, extending the source
tail by one output. The physical-cursor fix passes that unchanged261 endpoint.
The check then reached a crop fixture input mismatch: public microsecond offset
quantizes to whole source frames, whereas the capture began at an unadmitted
quarter-frame origin. Corrected actual input passed the expanded complete-output
check. The original quarter-origin capture remains kernel evidence, rather than
a claim about the public offset receipt.

The Mono rounding diagnosis was independently reviewed. A voice ends at289;
fragment-end `swap_remove` compacts it at295 under the irregular partition and384
under the fixed partition. This reorders identical same-sign contributions. At
most seven terms justify the separate bound `2*gamma6/(1-gamma6)*peak`, where
`gamma6=6u/(1-6u)` and `u=f32::EPSILON/2`. Before correction, the maximum measured
difference was3.7252903e-9 and two bit steps. Production DSP arithmetic is unchanged;
both independently scheduled per-partition comparisons remain bit-exact.

The temporary cache failures were fixture dependency errors: without a library
`Samples/` boundary they recorded unrelated ancestor-directory mtimes. Corrected
fixtures retain dependency validation and use separate cache roots. No cache
invalidation policy was weakened.

The shared synchronized live-frequency first attempt inspected settings before
the queued callback write applied. The corrected check retains immediate pending
getter assertions, inspects effective settings after rendering applies the write,
and passes the unchanged complete PCM oracle. The source-identity scenario's
first scripted remap setup lacked required snapshot mode2or3. Its correction
requires an accepted init edit before testing rejection; the whole scenario now
passes. The older bypass fixture reached a distinct profile mismatch: supplied
banks used Compatibility against a Kontakt NI reference. The corrected fixture
passes its complete output comparison. The latest no-default production library compiled in34.06s,
including local mono shared-volume and direct static-MPE shared AHDSR admission.
That default library/CLI build passed in1m02s with those extensions. The newer
2m31s harness additionally includes live MPE sensitivity, shared-pitch mono,
nonreflecting Sampler rows, importer/metadata prerequisites, the canonical
exact-root storage split and image fixes. Bank coordination remains unfinished;
future source changes require their own compiler checkpoint.

The new mono shared-volume and MPE checks passed their corrected rerun. Mono's
first attempt changed actual host generation windows along with fragment sizes;
the correction declares identical256-frame host windows in both schedules.
The MPE reference sent literal envelope points through event Gain, accidentally
adding that independent5ms smoother. It now renders unity source PCM and applies
the unchanged literal gain externally. Production arithmetic and comparison
tolerances were not changed by either fixture correction. These checks subdivide
the same host generations; they do not establish invariance to arbitrary changes
in host generation size or general scripted/role-changing shared-source admission.

## Staged musical verification — 2026-10-04, unrun

These scenarios compare the complete Engine/plugin output against independent
signal or event references. Compilation checks that the harness is wired into
the code; it does not establish PCM results. Except for the explicitly recorded
quick checks above, these scenarios remain unrun; corrected variants also require
their own rerun. Each includes fixed/irregular
blocks, exact note-end checks and callback heap guards where applicable.

| Scenario | Concrete input and expected comparison | Source |
| --- | --- | --- |
| Aligned Rack lifecycle | Twenty actual voices, fifteen Off64 releases ending128, a waited callback144 inspecting literal INST/TOTAL5, exact tuning, sustain release640/end704 and stop800 compare every main/direct/aux bus against independently scheduled part output. Distinct64/127 FX blocks expose buffer-offset mistakes. Growth333 retains local listener/wait timing, fresh RTC age and generated-child Host completion4477. | [rack_timeline_tests.rs](../src/engine/rack_timeline_tests.rs), [timeline contract](CORE_RACK_TIMELINE.md) |
| Flex vector indices | A cached260-point Compatibility envelope crosses index255 through sustain, early release, loops and OneShot. Independent two-frame point arithmetic defines full1024-frame stereo PCM; source endings520/837 and held Host endings600/837 remain distinct. Impossible public successors are checked for safe containment rather than native geometry admission. | [flex_index_tests.rs](../src/engine/flex_index_tests.rs) |
| RPN0 coarse ownership | Legal cents127, CC121/reselection/fine-only writes and native replacement retain the actual coarse field. Independent literal tuning controls define stereo PCM across physical4/7, selected/null parameter state, waited On and exact identity reuse; callback values and absolute ends are separately checked. | [rpn_coarse_tests.rs](../src/engine/rpn_coarse_tests.rs) |
| Dense stream generation | A real old disk decode pauses after its last configuration check;65536 actual configure writes reuse the slot/tag before the old producer publishes. Stale data must remain unavailable. The same worker then fills the fresh WAV, producing complete4096-frame PCM against independent RAM, with waiting silence, owner ends and no callback heap work. | [dense_generation_tests.rs](../src/engine/stream/tests/dense_generation_tests.rs) |
| Event lifetime and callback timestamps | Finite source EOF, terminal note-off, waited callbacks, private release children and more than4096 genuine admissions distinguish metadata retention from actual voices. Getter/mark validity and actual instrument voice counts drive audible gain/pan choices; literal absolute ends distinguish the stable host-block origin from double-counted callback offsets. | [event_lifetime_tests.rs](../src/engine/event_lifetime_tests.rs), [native evidence](../audits/KSP_EVENT_COMPLETION.md) |
| Shared Digital receiver and cold admission | Overlapping off-tick pitch/volume voices compare full PCM with independently recorded balanced-window points and a retained32-frame consumer grid. A real paused disk worker leaves the older note cold while a ready younger note nominates the shared producer; the observed successful I/O onset alone schedules an independent RAM reference. Bypass, idle phase, fills and reset remain observable. | [lfo_shared_tests.rs](../src/engine/lfo_shared_tests.rs) |
| Native quality0 interpolation | Independent double-precision polynomial basis weights distinguish Lagrange from Hermite across quartet phases, ordinary loop stencils, wide steps, resident/stream sources and shared versus solo filter lanes. Compatible residency fills preserve the kernel; changed source identity/quality closes old owners. Compatibility, qualities1/2 and wavetable boundaries stay explicit. | [voice.rs](../src/engine/voice.rs), [native evidence](../audits/COUNTED_LOOPS.md) |
| Wavetable residency dependencies | Real disk-backed ordinary-to-wavetable remapping rejects stale or partial heads that omit either prepared table window. A complete RAM reference defines4096 frames, release/tail and exact ends; complete wavetable and unrelated ordinary heads remain eligible. Rejected packets preserve worker disposal ownership. | [residency_wavetable_tests.rs](../src/engine/residency/tests/residency_wavetable_tests.rs) |
| Program Change protocol boundary | Typed, companion-UMP and bare MIDI2 Program Changes each diagnose once without invented bank selection or CC0/32. Full8192-frame direct/scripted plugin PCM, exact same-key overlays and late old identities compare against the unchanged independent note schedule. | [program_change.rs](../src/plugin/performance_tests/program_change.rs) |
| Native group admission | Ten note admissions traverse default/overlapping/gapped keyswitch ranges, mixed Boolean CC conditions, ignored/waited controls, channel retargeting, RR, generated notes and release masks. Distinct sample tones must equal a script-free Engine with explicit independent group masks; selector bypass keeps its separate release-mask behavior. | [group_start_tests.rs](../src/engine/group_start_tests.rs) |
| Saved controller defaults | Serialized defaults −1/0/127 drive volume, pitch, sample start and filter routes. Unseen routes must equal route-free processing; accepted zero/full/paired values match explicit gain/tune references. Physical owners, ignored callbacks, authored broadcast, bank replacement and frozen MPE tails are distinguished. | [controller_defaults_tests.rs](../src/engine/controller_defaults_tests.rs) |
| Input replacement | Sounding and pending roots on the old port are canceled; the new port reuses the same note ID. Rejected NOTE_END output must retry the original tuples/timestamps. Old tuning, bend/MPE setup and late traffic must not affect new PCM. Native CC criteria check old-cleanup/new-home initialization order. | [port_replacement.rs](../src/plugin/performance_tests/port_replacement.rs) |
| Mixed MIDI protocols | Native CC bank/data/selector messages are ignored, native96/97 remain ordinary callbacks, and real MIDI1 transactions keep their address and steps. Waited gain changes, physical channel separation and native parameters are compared through full plugin PCM. | [mixed_protocol.rs](../src/plugin/performance_tests/mixed_protocol.rs) |
| Timer and DSP ordering | Equal-time timers/listeners, exhausted callback fuel, wait0 and zero-length renders observe completed DSP before scheduling changes. Gain/insert output equals a separately scheduled script-free reference at the actual sample boundaries. | [engine/mod.rs](../src/engine/mod.rs) |
| Actual voice getters | Reverse/fractional source positions, nonzero starts, loops, multiple zones, transposition, waits and EOF feed PLAY_POS/ZONE_ID queries. Getter-controlled gain must match independently scheduled source-position references. | [host_notes.rs](../src/engine/host_notes.rs) |
| Shared LFO fade | Pitch and volume share one saved source across bypass, mute, release and retrigger. Complete PCM and source positions must match a closed-form fade/lag reference and an independently controlled amplifier. | [lfo.rs](../src/engine/lfo.rs) |
| Digital Multi LFO | Saved waveform6 crosses exact phase-1 and pulse-width boundaries with normalization and fade. Shared pitch/volume and volume-only output compare against independently recorded source points; unsupported low-rate geometry must fail before PCM or source advancement. | [lfo.rs](../src/engine/lfo.rs) |
| Flex Oneshot | Serialized v12 switches and markers control early/duplicate note-off, overlap, pedals and source completion. Full PCM compares with an independent linear-point amplifier; physical release and exact owner retirement remain separately observable. | [voice.rs](../src/engine/voice.rs) |
| Stable voice victims | A short voice expires and reorders storage; equal-pitch and same-note layered voices then overflow the saved limit. Highest/Lowest/Newest output compares with independently selected sample voices and an explicit 10 ms fade, including exact stolen-sample and surviving-layer lifetimes. | [victim_tests.rs](../src/engine/victim_tests.rs) |
| Bank source compatibility | Same-source residency fills retain sounding notes, authored timing/depth/bypass/gain and received CC state. Changed Digital/Flex geometry, amplifier mode, ordinary/internal envelopes or external seed values must match an independently prepared full bank replacement, closing old exact owners before the fresh note. | [instrument_source_upgrade_tests.rs](../src/engine/instrument_source_upgrade_tests.rs) |
| Detached per-note pressure | Detached notes keep their final pressure while the same physical address is reused. CC121 resets current pressure, preserves detached/other-input/exact-note values and registered controls/bend, and cancels the current waited pressure work. Full PCM compares with independent exact gain/tuning controls and original owner endings. | [per_note_reset_tests.rs](../src/engine/per_note_reset_tests.rs) |
| Saved non-volume AHD | Serialized retriggered pitch/cutoff AHD sources pass through cache and preparation. Early note-off must preserve the complete finite source, with an independent AHD amplifier keeping it observable; ordinary-mode controls, explicit parameter references, mute/bypass/retrigger/pedals and EOF check the mode boundary. | [parameter scenarios](../src/engine/params/tests.rs) |
| Counted and ordered forward loops | Authored NKI/cache rows retain inactive holes and saved order. Counts, partial first passes, release exactly at a boundary, large source steps and first/final interpolation stencils compare against handwritten physical phases. Curved terminal fades compare complete RAM/stream PCM with an independently unrolled ordinary source, including fragmented renders and exact owner endings. Legacy zero-crossfade UntilRelease checks fractional phases before, at and after the boundary. | [nki.rs](../src/creator/nki.rs), [native evidence](../audits/COUNTED_LOOPS.md) |

| Negative counted loops | Counts −1 and i32::MIN keep infinite replay until physical release. Fractional left/right stencils and ordered following rows compare against handwritten phases and deliberately wrong-stencil contrasts; independent source EOF fixes the exact final voice frame. | [nki.rs](../src/creator/nki.rs), [native evidence](../audits/COUNTED_LOOPS.md) |
| Flex looping | Saved Loop switch and wider markers produce a repeated independently defined piecewise-linear amplifier. Pedal release enters post-loop points; fully muted intervals advance the same source before unmute. Complete PCM and exact owner ends compare fixed and irregular partitions. | [voice.rs](../src/engine/voice.rs) |
| Scripted root sostenuto | Exact and anonymous release-only/exhausted roots retain capture at accepted same-frame CC66 edges. Post-edge roots and children remain separate. Scoped CC121 clears live and queued captures despite ignored synthetic pedal-up, with immediate/waited release clocks unchanged. Complete PCM equals independently timed ordinary sources, including MPE, late expressions and exact ends. | [sostenuto_root_tests.rs](../src/engine/sostenuto_root_tests.rs) |
| Stream decode failure | A real corrupt speculative read follows a valid prefix. A held slow-pitch note consumes that prefix for more than five seconds; reclamation must preserve active PCM and reject stale generations. RAM reference, checked failure, stop/restart and exact ends expose zeroed-ring races. | [stream.rs](../src/engine/stream.rs) |
| Release-trigger NoteMono | Saved release-tail monophony interacts with physical/delivered channels, sustain, sibling group masks and manual releases. Independently synthesized voices and explicit fades define the complete output and exact owner endings. Native keyboard Mono/Legato is a separate unsupported policy. | [release_mono_tests.rs](../src/engine/release_mono_tests.rs) |
| Polyphonic Glide | Time/Speed, signed/clamped intervals, cross-channel original history, changed/ignored/waited events, sustain and mute compare with a route-free Engine driven by analytic tuning knots. Off-tick bypass of one of two sources must retain combined pitch endpoints, with exact ends and callback heap guards. | [glide_tests.rs](../src/engine/glide_tests.rs) |

| Integral native crossfade | Saved widths/counts/releases and ordered rows compare complete resident/stream PCM against independently materialized physical512-frame gain buffers and handwritten source centers. Hardcoded nonbinary float landmarks distinguish native rounding; fractional points or endpoints must fail before PCM or clock commit. | [nki.rs](../src/creator/nki.rs), [native evidence](../audits/COUNTED_LOOPS.md) |
| Saved primary amplitude bypass | Authored NKI import/cache preserves excluded primary AHDSR/Flex records and native slots. Valid siblings, including bypassed non-volume sources, must produce the same complete PCM as an independently prepared sibling-only instrument. All-excluded loading and remapping into an excluded source must fail explicitly. | [nki_envelope_bypass_tests.rs](../src/creator/nki_envelope_bypass_tests.rs) |
| Direct and scripted admission | A queued exact root precedes a public direct start, then waited generated notes use the actual later live history. Complete output equals independently tuned route-free voices; future commands, mute, owner ends and callback heap remain observable. | [glide_direct_admission_tests.rs](../src/engine/glide_direct_admission_tests.rs) |
| Compact command ownership | Full6144-release admission retains applied snapshots until removal; cancellation, reuse and clear keep snapshot identity and capacity without heap work. Complete direct/scripted PCM also observes cancellation of an unrendered root and reuse of its payload slot. | [commands.rs](../src/engine/commands.rs), [glide_direct_admission_tests.rs](../src/engine/glide_direct_admission_tests.rs) |
| MIDI channel modes | Received CC123–127 match independently scoped physical note-offs across MPE managers, remapped homes, unrelated inputs and pedals. Waited/ignored CC122 callbacks match independently timed exact gain changes. Complete PCM and exact owner endings compare block partitions; mode reconfiguration is a separate gap. | [channel_mode_tests.rs](../src/engine/channel_mode_tests.rs), [native evidence](../audits/CHANNEL_MODES.md) |
| MPE role expression isolation | Physical0–4 enter MPE while unrelated physical6–10 deliver to logical4. Exact, anonymous and waited survivors, partial gain/pan edits, Router onset defaults and direct authored expression state compare with the same physical sequence independently delivered to logical11. Changed owners still cancel; complete4096-frame PCM, exact ends, partitioning and callback heap remain observable. | [articulate_mpe_expression_tests.rs](../src/articulate_mpe_expression_tests.rs) |

| Saved LFO timing | Actual v71 Digital triangle import/cache distinguishes unsynchronized Hz, enabled note-value/count timing and rejected Hz300. Complete PCM follows independently authored tuning points across tempo120→83, bypass, depth and mute; source EOF fixes exact ends. This distinguishes admission/conversion, while tiny phase-rounding differences rely on the separate native trace. | [lfo_timing_tests.rs](../src/engine/lfo_timing_tests.rs) |
| Live primary amplitude bypass | Patterned stereo samples keep advancing while the primary AHDSR/Flex source and retained interpolator pause151–317. Bypass529 and physical release713/853 follow independently defined480-frame fades and exact Engine ends1193/1333. Pedals, silence343–528, looped-zone remap/provenance guards and ordinary negative controls remain observable. Native wrapper-span retirement is a separate boundary. | [primary_amp_tests.rs](../src/engine/primary_amp_tests.rs) |
| Factory mono/legato policy | Reduced actual factory On/Legato/Offset and Next/Last/Low/High policy compares changing sample cursors, tuning, held-key fallback and exact owners against independently scheduled script-free voices across physical/library channel changes. This proves the staged core mechanism, not full factory script support or a generic native keyboard mode. | [mono_legato_tests.rs](../src/engine/mono_legato_tests.rs), [source evidence](../audits/MONO_LEGATO.md) |

| Sample geometry replacement | A sounding8192-frame source reaches cursor4101, then a shorter2048-frame replacement or a96kHz source arrives. Upgrade must behave like independently explicit replacement, close the old exact owner at4101 and use the fresh source step. New On4105 has absolute EOF6153/8201; ramp landmarks, complete9000-frame stereo PCM, receipts and callback heap checks expose stale cursor/rate reuse. | [instrument_sample_upgrade_tests.rs](../src/engine/instrument_sample_upgrade_tests.rs) |

| Native shared release counters | Independent constant sample-start references hand-replay balanced1025/3073-frame host spans. Same-key cross-channel notes, no-zone reset, pedal admission2800, waited release3424 and generated final-key reset3548 distinguish current native key age from captured physical duration. Full6146-frame PCM and source EOFs compare subdivisions within the same host windows; separate44.1k arithmetic admits real host-partition rounding differences. Initialzero and raw>1/current-shaper boundaries are explicit. | [rtc_tests.rs](../src/engine/rtc_tests.rs), [native ownership](../src/engine/rtc.rs) |
| KSP finite AHDSR completion | Actual Engine PCM follows an independently authored10/3/6/30 control-point envelope, skipping Sustain only for the admitted primary source. Physical-channel4 pedals, waits, mute and exact HostNote EOF1600/1648 distinguish source completion from metadata retention. Unsupported source rosters and immediate release fallbacks have audible unmarked controls. | [wnt_tests.rs](../src/engine/wnt_tests.rs), [native source/lifetime evidence](../audits/KSP_EVENT_COMPLETION.md) |

The roadmap also retains previously staged streaming, envelope, bus, insert,
native parameter and sample-geometry scenarios. This table highlights recent
coherent slices; it is not a test-count claim or a declaration of completion.
