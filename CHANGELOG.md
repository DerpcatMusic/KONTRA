# Changelog

Human-reviewed changes belong in the unreleased section before publication. Nightly
notes compare those entries with the previous published source, retain known limits,
and include the complete shipped public commit messages and merged PR descriptions.
Each published release manifest also retains its versioned changelog. Frozen entries
below record reviewed source checkpoints; they are not claims about pending work.

## Unreleased

### Experimental v2 development

- Add prepared low/high shelving EQ to the shared voice biquad processor, with
  explicit resonance semantics, stability checks and boost/cut response evidence.

- Reuse retained note performance/channel context for KSP CC reads and generated
  controller writes, including overlapping note/release callbacks after waits.

- Route controller callbacks through native input/downstream state and bounded
  continuations. KSP CC consumption/remapping now reaches ordinary MIDI and MPE
  manager ingress, preserving full-resolution forwarding and pedal ownership.

- Consume pending attacks when scripts stop their own event before forwarding,
  avoiding a spurious callback fault while preserving later host key-up pairing.

- Dispatch nested native callbacks through bounded, preallocated work storage. Deep
  release chains preserve callback fuel and wait ordering without growing the audio
  thread stack; gate completion retains its owner through callback side effects.

- Separate raw host key ownership from scripted note ends and callback faults.
  Late note-offs retain same-key FIFO pairing, source timers preserve host deadlines,
  and MPE member expression follows physical key ownership through script completion.

- Add release-event suppression and delayed forwarding through the native gate,
  pedal and release-layer owners. KSP release callbacks can wait and resume their
  original event without losing key-up context, group snapshots or reserved capacity.

- Isolate polyphonic cells by script instance in the prepared native layout. Programs
  share state only within their instance, retain old layouts across plan replacement,
  and account for the sum of namespace sizes before activation.

- Add stored-ID KSP note_off with explicit duration overrides through native key-up
  scheduling. Deadline replacement preserves ownership and capacity on failure;
  queued note ends retain silent-source owners until execution or cancellation.
  Control callbacks can stop stored notes; full vendor stage/selector parity remains open.

- Add generation-scoped source event IDs and KSP play_note return values through
  a preallocated native alias index. Retired IDs cannot target reused note slots;
  identity exhaustion fails before generated audio publication. Event-targeted
  commands and complete vendor lifetime semantics remain open.

- Lower KSP select/case, Boolean expressions, range checks and hexadecimal integers
  onto shared native operations. First-match dispatch survives waits; short-circuit
  guards skip invalid array reads while dead source still validates. Constant and
  runtime evaluation share numeric semantics; vendor parity remains unverified.

- Add prepared region groups with per-note editable and committed masks, inherited
  generated-note selections and off-audio generation retirement. KSP group commands
  now route attacks and release layers, including pedal-held release snapshots.
  Complete source group/import and vendor parity remain open.

- Add KSP integer constants and bounded arrays through native script-instance banks.
  Indexed reads/writes, inc/dec and num_elements preserve generation ownership and
  reject invalid accesses before adjacent state can change. Million-element source
  arrays are prepared off audio; full typed state and vendor fidelity remain open.

- Schedule typed control values on the native sample timeline, retaining their
  original plan and reserving revision space. Equal-time writes and script resumes
  preserve order; cancellation/panic release ownership without applying stale edits.
  Production host automation routing remains open.

- Connect shared controls to native voice gain processors with generation-owned,
  sample-clock smoothing. Script assignments, queued edits and recall drive the
  same DSP state; new voices join existing ramps. A waiting KSP UI callback now
  controls live native audio headlessly. Production UI/host integration remains open.

- Support evaluated KSP gate-linked (`-1`) and whole-source (`0`) note durations
  through explicit native policies. Whole-source notes retain layers/tails and
  return unused release reservations without fabricating note-off audio; loops
  remain owned until stopped. Source handles, offsets and vendor fidelity remain open.

- Implement KSP current-event `change_note`/`change_velo` through shared native
  note properties. Pre-forward edits affect mapping; late edits update script reads
  without changing running audio, release mapping or physical-key ownership.
  Full event targets, ordered stages and vendor fidelity remain incomplete.

- Forward unsuppressed KSP note callbacks at wait, exit or completion on the original
  native note identity. Conditional suppression no longer requires a leading command;
  selection retains high-resolution velocity, expression and original-plan ownership.
  Multi-slot, controller and release forwarding remain incomplete.

- Add native per-voice gain/biquad chains with explicit pre/post-envelope order,
  independent stereo state, control-prepared generation banks and bounded DSP tails.
  Musical release preserves post-envelope tails; finite choke bounds the whole chain.
  Vendor DSP profiles, automation and broader bus/effect graphs remain incomplete.

- Execute KSP integer expressions in note key, velocity, duration and wait arguments
  through native register services, preserving bounded child ownership and scheduler
  reservations. Add explicit seven-bit onset reads without narrowing core note state.
  Invalid evaluated arguments fault before publishing work; Kontakt parity remains open.

- Execute KSP integer expressions, precedence, parentheses, bitwise operators and scalar inc/dec through explicit signed-32 native instructions. Bound expression depth and register storage; the native audition command now sizes callback registers from the prepared plan.

- Add generation-owned script-instance integer banks and explicit program bindings. KSP globals now share values across note/release/UI waits while polyphonic cells stay per-note; schema validation, replacement, faults, cancellation and deferred destruction preserve ownership.

- Add native delayed/curved envelopes and one-shot AHD with exact duration boundaries, captured release/choke levels and physical-key-independent source completion. Source-clock curve anchoring preserves block partition identity; Kontakt/Falcon curve and clock mappings remain separate required profiles.

- Add prepared per-region constant, linear and power velocity response through shared attack/release admission, preserving raw velocity and layer predicates. Defer other format frontends in favor of full Kontakt/Falcon, DSP, MIDI, core and UI completion.

- Compile finite wrap/ping-pong pass counts into shared source traversal boundaries. Release can shorten but never restart or extend the limit; counted release layers can finish naturally without a duration command. Independent finite PCM references cover interpolation through the final tail and EOF.

- Execute KSP scalar UI handlers through native plan-owned callbacks, sharing the bounded scheduler without fabricated notes. Control interaction admission is atomic with callback capacity; waits, generation replacement, fault isolation and outcome backpressure retain explicit ownership.

- Add typed headless controls with stable identity, transactional edits/recall, coherent snapshots and bounded acknowledged UI transfers. KSP scalar knob/slider/button/switch declarations and callback access now use native generation-owned state without a window; UI rendering and the complete interaction callback family remain open.

- Add native ping-pong loop topology with shared traversal/interpolation boundaries, exact endpoint visits and explicit outward release exits. Independent unrolled PCM and analytic fractional checks cover reverse starts, short loops, high rates, muted phase and block partitioning.

- Execute KSP scalar while loops and nested continue through the existing bounded native scheduler. Repeating-note tests cover physical release under sustain, independent same-key owners, zero-time fuel exhaustion, cancellation and standalone WAV output.

- Add native physical-key queries and overflow-free signed comparisons; compile nested KSP scalar conditionals and callback exit with bounded code and registers. Regression cases cover same-key owners, waits under sustain, signed boundaries, terminal retry and exact standalone WAV output.

- Compile KSP polyphonic integer declarations, scalar assignments and note/release callbacks into the new owned native program table. Source/rate/storage budgets are validated before activation; overlapping callbacks share only their originating note state, with executable PCM and standalone WAV checks. Full KSP parity remains open.

- Reserve native physical-release callback capacity at triggered-input admission. Release handlers retain their original program and note state across EOF, pedals and plan replacement; faults remain observable without blocking physical release, and hard cleanup suppresses pending handlers.

- Add bounded note-owned integer state shared across native callbacks and retained through release, terminal retry and plan replacement. Generated children and reused note slots start with zero state; KSP variable syntax and typed semantics remain open.

- Compile full-resolution controller conjunctions into native attack/key/gate selection, with coherent onset/current policies, shared microphone predicates and transactional take decisions. Control-time interval projections bound controller release overlap without a Cartesian state table; correlated multi-controller cases can remain conservative.

- Retain coherent articulation and full-resolution controller snapshots in a bounded shared-version pool. MIDI/MPE effective controller updates preserve note onset state without callback allocation; pedal publication and scope admission are atomic.

- Add explicit musical performance domains, note-owned articulation snapshots and silent latched keyswitches. Sparse region filtering and onset/current release policies stay independent of MPE member channels; ordinary MIDI and MPE can route notes to a chosen domain.

- Select native key/gate release layers with note-owned voice, family, decision and command reservations. Independent phase sequences, explicit velocity policies and finite family gates survive pedals and plan replacement; hard/fault cleanup suppresses pending releases without spawning audio.

- Retain logical admission, key-up and gate-closure times with distinct musical/cleanup causes in note-owned release context. MIDI 1/2 and MPE preserve optional release velocity through pedals, source EOF, plan replacement and terminal retry; key/gate state derives from these records.

- Add explicit seeded random, no-repeat and shuffle policies to native scoped take selection. Draws and bag swaps commit only after successful admission; shuffle storage is budgeted and prepared on control, with bounded callback work and original-generation ownership.

- Add coordinated native sequential takes with global/key/channel/channel-key scopes, transactional multi-family admission and separately budgeted note-owned decision history. Mutable sequence state travels with prepared generations for off-audio retirement; old delayed children retain their original sequence after replacement.

- Add immediate and sample-scheduled family chokes using native envelope state. Fades capture each source's level without extending existing tails, preserve loop phase and sibling/physical-note ownership, and cancel delayed starts; naturally retired targets cannot affect reused family slots.

- Follow direct child/family/source ownership links during release and family stops, with a preallocated closure stack and no recursive traversal. Independent descendants, envelope tails, failed admission and terminal retries retain their lifetime rules; phase workloads measure admission, note-off and retirement separately.

- Allocate arena slots through control-prepared free bitmaps, preserving lowest-slot order, generation quarantine and exact transfer rollback. No callback allocation or free-list ordering change is introduced.

- Maintain arena capacity and occupancy during ownership changes instead of scanning reservations for every admission. Plan transfer rollback preserves counters and generations; a new burst workload measures the remaining insertion cost explicitly.

- Admit native absolute note pitch separately from tuning and live expression, and consume MIDI 2.0 Pitch 7.9 attributes without changing physical note pairing. Generated notes retain fractional pitch through transposition; source-rate failures remain transactional.

- Compile native per-key semitone tuning into prepared source rates. Plan replacement tunes new inputs while held notes and later generated children retain their original tuning; live expression stays independent and fixed-pitch regions remain exempt.

- Validate immutable PCM once and share its original buffer across prepared plans. Plan adoption and retirement perform no audio-thread reference-count or destruction work; independent plan edits avoid copying or rescanning resident samples.

- Skip PCM/filter evaluation for explicitly zero-gain voices while preserving exact source phase, envelope transitions, loop exits and retirement. Muted +7-semitone workloads measured 41.5–44.1× faster locally; reference PCM and audible-path checks guard the optimization.

- Make native pressure/timbre modulation audible through bounded prepared routes to gain, stereo balance and semitone pitch. Canonical inputs stay intact; event-time projection is cached per expression owner, retains original plans across adoption and participates in every source-rate preflight. Real UMP tests cover MPE playback and muted-source continuity.

- Apply MPE manager sustain/sostenuto through native channel scopes, with atomic pedal-down reservation and capacity-independent pedal-up. Physical input domains remain isolated; repeated sostenuto and ignored member pedals preserve capture semantics.

- Apply whole-semitone MPE RPN 0 sensitivity transactionally: independent manager range, shared member range, retained raw bends and frozen released-note pitch. Selector/null/NRPN isolation and failed-rate rollback preserve existing notes and future controller state.

- Project fixed-zone MPE channel pressure and CC74 into native expression, preserving released member snapshots and unrelated note dimensions. Manager combination is explicit; audible modulation destinations and dynamic receiver configuration remain pending.

- Add fixed-zone MPE note/pitch projection with generational tail bindings, initial controller state and atomic whole-zone gestures. Member expression freezes at physical key-up while manager pitch reaches retained owners; bounded heap-free batches validate occupied voices once. Dynamic configuration and remaining MPE controls are still unsupported.

- Admit initial native expression before source selection and bound note programs. Immediate snapshot children retain the supplied pitch/gain/pan; invalid expression and unsupported source rates fail without partial native layer ownership.

- Apply live native note-scoped pitch without resetting source phase. Immediate/queued expression and later source admissions validate one another's rate constraints, including delayed voices and linked children; detached/snapshot owners stay isolated and failed generated sources leave no partial ownership.

- Add optional equal-tempered root-key tracking to native regions. Preparation compiles and validates the rate for every mapped key; playback preserves exact root unity and keeps logical pitch separate from physical note addressing.

- Compile native source cursor templates during preparation, discard the redundant authoring playback fields, and share one bounded voice-admission boundary. Prepared note starts no longer repeat static pitch/rate calculations or source-view validation.

- Add fractional resident playback, source/output rate conversion and static semitone transposition to the new core. Ratio-dependent windowed-sinc filtering preserves independent forward/reverse loop traversal and release guards; analytic tones, independent FIR sequences, block-partition and heap checks cover the path. High-ratio performance and live pitch remain open.

- Skip unused voice reservations with a bounded occupancy bitmap while preserving exact slot-order mixing. The sparse resident workload measured 1.37–2.25× improvement locally; boundary, reuse, EOF and allocation checks cover ownership.

- Skip per-sample envelope-state work after AHDSR reaches a constant sustain level, preserving sample arithmetic and the existing unity fast path. A reproducible resident workload reports about 2.9× median improvement for 1,024 sustained voices in 256-frame blocks on the measured local machine; no production latency guarantee is implied.

- Exercise resident plan replacement in `sampler-native replace`: adopt a second WAV between overlapping notes, preserve each original source and tail, and retire the old plan outside rendering. Independent process-level audio checks pass at 44.1/48/96 kHz.

- Add bounded prepared-plan replacement with generational note ownership and control-side retirement. Existing notes, tails and callbacks keep original PCM/programs while new inputs use the active plan; saturated queues retain ownership, and audio-thread heap checks cover cross-thread transfers.

- Run authored KSP-subset scripts against resident WAV samples through the independent native executable. Compile at the source rate and render a fixed two-second audition; process-level checks compare exact expected audio at 44.1/48/96 kHz and preserve existing files.

- Apply channel-scoped All Sound Off to native tails, generated descendants, delayed sources and retained callbacks. Silenced physical inputs retain same-key pairing until key-up; queued releases cannot resurrect work or target retired owners.

- Apply MIDI 1.0/2.0 All Notes Off through bounded native gate cleanup. Sustain/sostenuto and input-domain isolation are preserved, with observable release counts and no dependence on spare channel or command capacity.

- Reclaim finished internal note/expression ownership before bounded behavior admission runs out of slots. Repeating scripts remain independent of block size and rejected host terminals; live voices, descendants and pins still retain their owners.

- Add a clean-sheet KSP note-callback source subset with bounded compilation and explicit unsupported diagnostics. Literal waits and fixed-duration generated notes execute through the new core and `sampler-native script`; authored timing/ownership fixtures pass without the old parser or VM. Kontakt fidelity remains unverified.

- Separate callback retention from generated-note duration in native behavior execution. Fixed-duration notes can outlive input release; gate-linked notes and retained callbacks keep distinct ownership. Panic cancels both kinds of waits, and tests cover late generation and fault-time retirement.

- Add budgeted callback-local integer state, logical-key readback and validated branches to native behavior programs. Locals survive waits and remain isolated across notes and reused slots; checked overflow and zero-time loops produce retained faults. The echo example now runs a counter loop.

- Add native bounded behavior programs with generated notes, sample-time waits, suppression, instruction fuel and retained completion/fault ownership. An independent MIDI-driven `sampler-native echo` command exercises the new path; no old parser or VM is linked.

- Make linked-release propagation linear even across reverse-order reused slots. Independent children retain their gate and ancestry until explicitly released; deep-chain heap checks and a before/after microbenchmark cover the change.

- Replace repeated descendant-retirement scans with counted child ownership and an iterative parent walk. Deep reused-slot trees, pinned descendants, failed admission and terminal backpressure preserve ownership without callback heap work; an independent benchmark records the scaling improvement.

- Share bounded sample-offset UMP block processing with the native executable. Validate whole-batch timing before mutation, preserve stable event order, and keep processing releases after per-event admission failures; partition and heap checks cover the callback.

- Add a clean-sheet UMP decoder preserving MIDI 1.0/2.0 precision and protocol identity, with bounded note/pedal ingress and native demo integration. Pin the wire specification, test framing and callback heap safety, and raise the authoritative new-core Rust Doctor score to 91. Full MIDI 2.0 musical/device support remains open.

- Add per-region PCM ranges, forward/reverse playback and continuous/until-release loops to the new core. Independent voice cursors preserve shared-asset ownership; exact loop-boundary release, invalid views and callback heap safety are tested.

- Enforce the selected new-core Rust Doctor threshold of 90 with an authoritative, complete scoped report and source-hash evidence, alongside the workspace gate against new errors. The current new-core scan meets 90 with no rules disabled.

- Add native sample-time linear AHDSR with per-region parameters and retained release-tail ownership. Pedals defer release; panic and explicit stops cut tails. Independent timing, retirement and callback allocation/free tests cover the new path.

- Pin Rust Doctor 0.7.0 with a fail-closed CI report check, PR baseline comparison and a documented Rust/realtime DSP review policy. Preserve full-workspace debt instead of hiding it behind a score.

- Add an independent `sampler-native` offline executable with owned prepared PCM, indexed native region selection and transactional layer admission. Its narrow WAV path and rendered output are checked; live audio, plugin hosts and full source/DSP behavior remain in development.

- Add physical/effective key separation, channel-scoped sustain/sostenuto and timestamped expression to the new core's shared sample-time queue. Queue-full cleanup and scheduled ownership are covered by independent realtime checks; advanced pedal and continuation behavior remain open.

- Add independent generational family/expression ownership, explicit child inheritance and bounded cleanup to the new sampler core. Render resident PCM in event-delimited segments and include a reproducible microbenchmark and core-only allocation/free checks. This remains experimental; it does not change production playback or establish MIDI 2.0 device support.

### Accepted 0.3.148 — primary envelope processing and visible crash receipts

Two reviewed logical fixes advance the accepted counter from 0.3.146.

### Fixed after 0.3.146

- Render strictly admitted saved primary AHDSR sources on their native finite 32-frame control clock, preserving native attack/release curves, captured release state, interpolation, pedal ownership, retrigger and sample EOF. Four focused mathematical/resident-engine gates pass, including fragmented blocks and no audio heap allocations. Unsupported transforms retain diagnosed fallback instead of being silently admitted.
- Keep crash-report delivery status, report ID and an allowed public GitHub issue link above the Logs filters. Default warning/error filters, search changes and replacement histories no longer hide the receipt; Copy issue link accepts only the validated public KONTRA issue URL.

### Known limits for 0.3.148

- Fresh isolated f119048 passes locked all-targets/all-features checking and optimized builds, 600 library regressions (30 ignored), 89 playback regressions (four ignored), 59 reporter checks, manual export, seven saved-volume/phase checks, actual Logs/receipt/native-parent checks, four primary-AHDSR gates, Linux exported CLAP/VST3 processing and strict C++ editor attachment. Final version metadata and hosted release checks are verified separately before publication.
- Primary AHDSR admission is deliberately bounded to the proved saved format and unity target. Held-voice live envelope changes, transformed targets and other unproved formats remain unsupported or diagnosed. These gates do not establish whole-preset sound equivalence or fix the reported AREIA double attack by themselves.
- Mac/Windows DAW runtime, the reported uncaptured Mac opening-crash cause, a genuine production crash-to-issue roundtrip and full native Lua UI rendering remain unverified. Performance counters are test instrumentation, not a GPU or display-FPS result.


### Accepted 0.3.146 — saved phase, native parent validation and diagnostics

Four reviewed logical fixes advance the accepted counter from 0.3.142. Together
with saved sine-LFO volume, five outcomes follow the published 0.3.141 release.

### Fixed after 0.3.142

- Apply finite saved cycle phases from 0 through 1 to admitted retriggered sine pitch and volume LFOs on their existing shared clock. Native field identity, independent phase values, positive lag, fragmented playback, bypass, retrigger and zero audio heap are checked. Other waveforms, free-running ownership, live phase edits and complete Analog Strings preset parity remain unsupported or unproved.
- Validate CLAP parent API/null handles and VST3 platform/null parent/missing callback before interpreting native handles or attaching the editor. Actual adapter and C++ attachment regressions pass; the uncaptured Mac opening crash remains unconfirmed.
- Show readable script callback, argument, array/listener and source-availability context. Copy-all begins with a sanitized warning/error digest and retains structured events; unavailable-source inspection preserves navigation. Logs filter caches invalidate when journal identity or retained history shape changes, including equal-revision replacement.
- Include versioned Added headings and Known limits in generated nightly notes. The 18-outcome release fixture reproduces the prior omission, and all publication/signing scenarios pass. The published 0.3.141 body is corrected; its immutable original manifest omission is disclosed.

### Known limits for 0.3.146

- The exact pre-version candidate bdef149 passes locked all-targets/all-features checking and optimized builds, 595 library regressions (30 ignored), 89 playback regressions (four ignored), 59 reporter checks, seven saved-volume/phase checks, manual export, actual Logs and native-parent checks, and Linux exported CLAP/VST3 processing. Final versioned binaries and hosted release gates are verified separately before publication.
- Mac/Windows DAW playback, production crash-to-issue delivery, full native Lua UI rendering and whole-library DSP parity are unverified. Source-only envelope and drop implementations are excluded until their own acceptance gates pass.


### Accepted 0.3.142 — saved sine-LFO volume

One reviewed logical defect advances the accepted patch counter from 0.3.141.

### Fixed after 0.3.141

- Render admitted saved sine-LFO volume targets with their native bipolar depth and positive lag, shared 32-frame source clock, retained bypass state and decoded amplifier placement. Seven focused engine gates, including nonlinear ordering and zero audio heap, pass with 595 library and 89 playback regressions. Additional volume targets, unknown flags, separate invert/shaper/fade, live volume intensity edits and full preset sonic parity remain unsupported or unproved.

### Accepted 0.3.141 — reporter and native Ladder processing

Eighteen reviewed logical outcomes advance the accepted baseline from 0.3.123 to
0.3.141. Correction fixtures, receipt refinements and the already-counted worker
permit/zero-rate renderer fixes do not add duplicate counts.

### Added after 0.3.123

- Add single-rate native Ladder LP4 processing with the checked nonlinear four-pole kernel, signed Gain and correct insert ordering.
- Expose delivery status, report ID and an allowed public issue link in Logs, and restore only reviewed receipt fields on a later launch.
- Include exact locally archived crash originals and saved queue/delivery records in manual support exports, with missing, busy or changed-source coverage. Original copies are unredacted regardless of the structured-log toggle; OS-wide searches and binary dump copying are excluded.

### Fixed after 0.3.123

- Apply ordinary Ladder Gain, resonance and cutoff steps on the physical control clock, with consistent reset/retarget snapshots and fragmented-block behavior.
- Preserve the stored version 144–146 cutoff boundary-snapshot law independently of the Gain/resonance clock; unknown versions remain diagnosed.
- Keep enabled Ladder routes on persistent control ticks even when their depth is zero or target value is unchanged.
- Advertise configured CLAP MPE input while retaining preferred CLAP notes and existing output dialects. The Linux original-tone gate checks member/manager expression and pedals; automatic zone negotiation is not added.
- Retain filename mappings and raw timestamps when an optional calendar-date view cannot represent the saved value.
- Require the recorded host executable/application and documented fatal OS metadata before confirming a native crash. Missing, simulated, nonfatal, oversized and mismatched evidence stays unconfirmed; confirmation does not assign plug-in fault ownership.
- Redact authored cases of quoted Unix paths, Windows drive paths and UNC assignments while preserving useful exception/frame symbols. Unseen native fields are not certified safe by these fixtures.
- Bound recovered journal submissions to 128 startup and 2,048 recent records, with complete-original size/hash and explicit omitted-slot/drop accounting. Complete private originals survive acknowledgement and are not automatically pruned.
- Keep an active durable publisher’s lock and in-flight files out of stale-session recovery cleanup.
- Retain unconfirmed incidents privately without blocking later confirmed delivery; recheck up to 32 deferred records per registration and keep a discovery cursor across launches.
- Measure the complete serialized UTF-8 JSON body, including escaping, before network submission instead of estimating individual fields.
- Bound legacy/native buffered reads and stream complete oversized originals into a private archive before retiring local source slots. Prefix/suffix views do not confirm partial native JSON, and failed preservation leaves the source intact.
- Keep independent keyed incidents, durable legacy migration and exact-ID acknowledgement. After acknowledgement and permit release, bounded actor scans can drain later confirmed incidents in the same host; offline delivery or failed retirement stops continuation without losing pending evidence.
- Use recorded incident host/build/system metadata with explicit unknowns, and render/hash submitted evidence independently of a later reopening host or current build. Reopening context is a separate local diagnostics event, excluded from the submitted report.
- Preserve complete private evidence before recording a source-digest/recipe/limit-bound manual-export disposition for packets over the 16 MiB JSON limit. No acknowledgement, deletion or delivery is invented. Release the worker permit so other queued incidents can proceed once; changed evidence or policy is reevaluated. Logs and restored receipts truthfully say manual export is required and automatic retry is paused for unchanged evidence.

### Known limits for 0.3.141

- Fresh isolated source 4a4f5ca passes locked all-targets/all-features checking, test compilation and production build; all 477 tracked source hashes remain unchanged. Its frozen executables pass all 590 library regressions (30 ignored), all 89 playback regressions (four ignored), all 59 reporter checks and the manual-export gate. Parent 9c1f7d1 passes six Ladder, 10 BUFFR durable-file, 11 flight-recorder and one optional-date vendor regression plus Linux strict-MPE/native VST3 gates; the 4a correction changes only authored actor-lifecycle tests. Final versioned production identity and host checks are verified separately before export.
- Ladder High Quality oversampling, other Ladder modes and complete audible/native transition equivalence remain unproved. Known control/routing/PCM gates establish only their scoped laws; whole-library sonic parity is not claimed.
- The Linux strict-MPE CLAP and authored-tone VST3 gates do not establish Mac/Windows DAW behavior, automatic MPE negotiation, or the cause of the user’s uncaptured Mac crash. Complete native Lua UI support, unaccepted volume/UI work and new Rad/Keytar sonic claims are not included.
- Automatic reports submit bounded, sanitized evidence, not complete raw local journal/native history. Manual original copies are explicitly unredacted. The deployed service stores full submitted reports privately, applies server redaction and 30-day expiry, and publishes metadata summaries; a real production host-crash-to-private-store-to-public-issue roundtrip remains unverified.
- The already accepted upload-permit-release and zero-rate renderer fixes are not counted again. Queue continuation, receipt refinements and correction fixtures stay within their parent outcomes. Documentation and external service deployment add no plug-in fix count.


### Fixed after 0.3.115

Eight newly accepted logical fixes advance 0.3.115 to0.3.123; closing the already-counted VST3 optional-length outcome adds no duplicate count.

- Source-zone group/key edits now prepare on the worker in order. Init edits are installed before the bank becomes playable; runtime completions retain exact source IDs, budgets and restore freshness. Actual Conflux captures now select different physical wavetables rather than all playing the same fallback source.
- Held-note expression retains its original destination when MIDI channel modes or routing change, through pedals and release ownership.
- Modern version0x90–0x92 Ladder records preserve leading Gain, cutoff, resonance and raw native flags with byte-exact roundtrip. Ladder DSP was unsupported at the 0.3.123 checkpoint; the single-rate LP4 scope added above is separate.
- Legacy Delay saved sync units use host tempo across all rack scopes. Independent absolute/synchronized physical caches survive binary and JSON host state and stale paired restoration; unknown saved units retain explicit diagnostics. Native fixed-ring duration limits remain separate.
- Integer division/remainder by zero returns native zero without generating invented script faults. Bounds, unavailable MIDI context and real nonfinite-result errors remain visible.
- Automatic crash-report delivery releases its worker permit on completion, allowing later confirmed incidents in the same host. Invalid acknowledgements and offline failures retain pending evidence.
- Live and restored bypass reaches admitted saved pitch LFO sources, preserving paused source phase/fade and removing only the selected contribution. Wider waves, free-running clocks and live frequency remain unsupported.
- The native VST3 adapter now accepts negative optional note-length metadata. Real exported processing passes ten authored-tone cases covering anonymous/live note IDs, independent cents-frequency checks, sample offset 16 and explicit note-off. This closes the adapter gap disclosed in 0.3.115; the reported Mac keyboard/arranger payload has not been captured, so its cause remains unconfirmed.
- Frozen coherent core cfef7b0 passes the locked all-targets/all-features check, optimized production build, 555 library regressions (30 ignored), 89 playback regressions (four ignored), and the real exported VST3 gate. These checks establish the listed behaviors, not full sonic parity or Mac DAW crash resolution.

### Verified processing and diagnostics follow-ups

- Tube and Transistor Distortion now use their checked native scalar curves without the previous invented drive compensation. Independent signed boundaries, live Drive readback, group/rack routing and zero-heap gates pass. The separately reviewed Damping/DC corrections are recorded below; whole-effect parity and corrected actual preset replay are not established.
- Optional KSP `note_off` microsecond offsets now replace existing duration timers and retain the exact scheduled event under pool pressure. Concurrent-note, channel, timing, invalid-offset and zero-heap checks pass.
- Stale script restores now cancel before obtaining a fresh epoch or publishing old scripts/effects. Controlled races cover source/state replacement, generation changes and removal; unrelated gain edits preserve valid restores.
- Saved legacy unsynchronized fade-in now reaches eligible retriggered sine Multi pitch sources on the same 32-frame clock. Independent physical cursor/PCM, fragmentation, retrigger, lifetime and zero-heap checks pass. Synchronized fades, wider waveforms, free-running clocks and live frequency remain unsupported.
- The combined follow-up source passes 510 library tests and all 89 playback regressions, with 30 and four deliberately ignored cases respectively.
- Classic Saturation now uses the checked native piecewise polynomial Shape law and linear Output instead of the previous proxy. Independent scalar/ordered-processing/readback/zero-heap gates pass, alongside all 89 playback regressions. Matched Contradiction replay gains 17.78 dB RMS with unchanged onset, events, controls, cursor travel and loop bounds; the bypassed Catastrophic control remains byte-identical. These observations do not certify sonic parity. Enhanced/Drums modes retain explicitly diagnosed approximations.
- Recognized group-effect targets no longer disable an unrelated internal pitch LFO at the same numeric slot. Unknown source-parameter assignments remain guarded; legitimate effect assignments are retained.
- Signed and unsigned intensity writes now reach admitted internal pitch LFO targets using original source/target indices. Callback readback, physical-depth PCM, unchanged phase, alias persistence, seeded restore and zero audio heap pass focused gates.
- Native Constant loop start/length assignments now control eligible forward full-sample sampler loops on 32-frame ticks, preserve fractional cursor/crossfade/streaming, and retain the final release seam. Unsupported geometry and behind-cursor edits remain diagnosed and deferred. Voice diagnostics expose applied bounds separately from virtual position. Six matched real-preset captures confirm applied windows and held wraps; Prelude changes substantially, while Contradiction remains quiet. These observations do not certify sonic parity.
- Saved retriggered, zero-delay sine-only Multi pitch assignments now run through a note-owned clock and audio-rate interpolation. Planner fragmentation no longer changes that clock. Focused semantic/PCM/zero-heap gates and all 89 playback regressions pass; wider waveforms, delayed/free-running ownership and live frequency remain unsupported. Six actual reported-preset captures exposed a separately corrected slot collision; three matched actual captures confirm pitch-source travel in Rad Prelude and Beta Decay, with unchanged controls and a byte-identical Catastrophic negative control. These checks do not establish full sonic parity.

- Legacy and modern signed modulation aliases now share the verified pitch/cutoff laws and exact target identity. Other unsupported legacy target laws remain guarded.
- Saved negative pitch and cutoff modulation now applies the native target sign bit independently of invert, with raw records preserved and both directions checked against physical-depth PCM references. Loop modulation remains incomplete.

- Native convolution early/late Size ratios now survive Reverse, Auto Gain, predelay, callbacks and restore independently; editing one Size leaves the other intact. Older host states retain their prior uniform behavior. Native nonunit time stretch remains incomplete.

- Exact host note IDs now retain their routed ownership through sustain, script children and waits, delayed expressions, release tails, and aggregate NOTE_END delivery. Generated articulation keyswitches cannot take the musical note ID; mixed raw MIDI releases and scoped cleanup preserve unrelated held notes. Unverified host expression types remain diagnosed.

- `wait_async` now retains the original callback until its admitted NKA/IR operation has finished installation or failed. Unknown and completed IDs continue immediately; the bounded wait state retains event/channel context without polling or audio-thread I/O.

- Explicit positive convolution crossover now prepares separate early/late filters at unit Size and matching sample rates, with the checked 50 ms blend and original source duration. Nonunit Size, automatic crossover and resampling remain approximate.

- macOS delivery adds one universal Intel/Apple Silicon installer for CLAP, VST3 and the standalone app, with standard plug-in folders and `/Applications/KONTRA.app`. Publication requires timestamped Developer ID Application product signatures, a Developer ID Installer package signature, Apple acceptance and validated stapled tickets. Separate architecture ZIPs retain notarized DMGs for compatibility. Existing owned signing credentials are reused; there is no unsigned fallback. Real submission IDs and package/product hashes are recorded in the release receipts.
- Modern signed cutoff intensity now uses the independently corroborated cubic law and inverse readback, with normalized bounds retained. The actual Conflux saved value showed the old linear conversion overstating its depth by about 4,877×; other modulation laws are unchanged.
- Native LFO waveform-specific v0x71–v0x73 records now decode and write losslessly with strict size/flag checks. Forty-two authored combinations and 60 actual selected records pass. LFO playback/freewheel behavior is still incomplete.
- Nonunit convolution Size now reports its resampling approximation and affected Auto Gain energy explicitly. Native pitch-preserving IR time stretch remains unavailable.
- Convolution IR high/low-pass bypass follows the checked native frequency/rate boundaries. Digital-pole decay padding fixes the reproduced near-Nyquist response truncation; independent numeric and shared routing/lifetime checks pass. Finite padding does not establish native prepared-length identity.
- VST3 editor attachment now opens the editor before processor activation or state restoration. The actual host-shim regression reproduces the old blank-view path and passes attach/resize/restore/reopen after the fix; the specific Void report remains unconfirmed.
- Native v0x103 external modulation now decodes with exact opaque-footer preservation. Six wavetable group parameters omit the module slot byte, fixing 21 misaligned target records. All 3,752 actual Conflux assignments now decode and roundtrip byte-exact, including 366 pitch-bend sources. Shaper decoding remains strict; footer semantics and target DSP support remain separate.
- Live convolution Reverse and Auto Gain now rebuild on the worker and survive saved-state restoration; stale completions retain the current kernel.
- Enabled eight-knot IR volume envelopes use checked amplitude interpolation before Auto Gain, with predelay kept separate. Malformed active curves retain diagnostics.
- Delayed script callbacks can update prepared short and automation control names without allocating on the audio thread. Three actual Conflux preset probes each pass 750 listener blocks without the property fault and confirm their authored −12/+12 semitone tuning; authored arithmetic warnings remain visible.
- Resident wavetable taps dispatch PCM once per block. Matched-output thread-CPU measurements show 1.55x/2.00x/1.73x speedups for F32/I16/I24 in this kernel; packed decoding and total-plugin performance are not included in that claim.
- Nightly packages build alongside the unchanged shipping checks; publication still requires both to pass for the same source commit. Duplicate ordinary main-push CI is removed. Hosted timing improvement is not yet measured.
- Uncaptured native GPU failures now reach persistent diagnostics after device rebuilds; original stderr and device-loss recovery remain intact. Void blank-editor repair is not yet confirmed.
- Registered per-note MIDI2 brightness now follows its retained note, including start-only modulation. Ambiguous host brightness is explicitly diagnosed instead of changing channel CC74 or a newer same-pitch note.
- Saved convolution Auto Gain now uses the checked prepared stereo-energy rule, threshold and cap while preserving dry output. Unequal early/late shaping remains approximate.
- Saved pitch and filter/EQ envelope bypass now uses the verified source flag, advances its clock and resumes without restarting. Amplitude/Flex bypass lifetime is still under investigation.
- Wavetable source records are read and edited without losing opaque bytes. The engine prepares resident 2048-frame cycles with note-based pitch, live position, and verified Linear/ASYM2MP phase forms. Nine actual Conflux captures across three patches confirm octave pitch ratios and zero underruns. Preset tuning and scripted pedal behavior remain under investigation. Native bandwidth-table preparation is missing for all quality settings, including High/Best anti-aliasing. Unsupported states are counted instead of played as ordinary samples.

### Fixed after 0.3.64

- Structured nightly notes now retain reviewed processing and version-specific fix headings instead of falsely reporting no reviewed changes.
- Modern signed pitch intensity now follows the checked cubic conversion and retains 24/36-semitone values; non-pitch modulation laws are unchanged.
- Convolution Reverse now reverses the source impulse response before rate conversion and predelay. Auto Gain uses the verified prepared-energy law, and live switches and eight-knot IR Volume Envelope passed focused processing checks. Unequal early/late shaping remains approximate.
- Large load dependency lists now use bounded typed journal chunks that can be reassembled completely in support reports. Source excerpts and path redaction remain intact; oversized individual values still report truncation.

### Added

- Bounded Falcon/UVI metadata inspection and format/runtime research document
  the next compatibility requirements. This does not add Falcon playback.
- A Rust dependency audit records reuse opportunities and verified runtime limits.

- Headless `audit-patch` accepts explicit snapshot and program selection while
  using the existing import, script, bank, effect and paced note/chord path.
  It retains structured load stages and diagnostics without opening an editor.

- Experimental Bitwig VST3 project inspection and explicit SavedMulti-to-KONTRA
  migration create a new project copy and report; source bytes are rechecked and
  retained. Shared plug-in-state entries and overlapping device mappings are
  rejected instead of changing multiple devices through one cached state.
- Independent authored performance pages expose each script slot's prepared UI
  buffer. Footer selection keeps callbacks on the selected owning slot; late
  publications and replacement epochs retain that ownership.

- Cached instrument snapshots appear in a compact header preset row with owned
  category labels, alongside the existing explicit snapshot picker/drop workflow.
- Independent user UI zoom preferences and persistent global editor window size.
- Native file-picker callbacks for supported KSP file selectors, with prepared paths
  and retained asynchronous callback routing.
- Imported Creator Tools performance-view controls for supported exported records,
  including null lists for empty menus and the initialization-only constraint.
- Declared JPEG resource names share the image decoder instead of being discarded.
- Prepared rack storage and viewport rows remove the previous fixed part cap; browser
  drops can append parts in the empty canvas. This is a feature, not a fix-count unit.
- Persistent library display aliases use the detached catalog without renaming
  library files. Native Reveal validates its target path before dispatch.
- Bounded source-identity diagnostics identify unsupported Kontakt 8 wavetable
  playback instead of representing it as supported sample playback.

- Every nightly records reviewed Added/Changed/Fixed/Known limits deltas, complete
  shipped public commit messages and merged PR descriptions in its release body and
  versioned manifest. The previous release source is the comparison baseline;
  source checkpoints remain traceable even when exports squash private history.
- Load supported snapshots from the instrument header picker or an explicit
  header drop. Parsing and base validation run on the loader before changing the
  active source; base and snapshot paths survive host state and KONTRA multis.
- Save successfully applied native script parameter edits alongside persistent
  script variables, and seed authored initialization getters during restoration.
  Prepared storage avoids audio-thread growth; refresh visits edited slots rather
  than every default parameter in a large library.
- One package version and build identity across CLI, standalone, plugin metadata,
  About, diagnostics and package manifests. Identity includes the actual full Git
  revision, optional export source revision, UTC timestamp, target, profile and features.
- Deliberate SemVer release preparation and reproducible nightly prereleases, with
  focused local checks and documented contributor workflow.
- Searchable structured Logs with failed-load context, source locations, bounded
  recent history, and private support reports that include retained crash journals.
  Reports identify omitted events, write errors and partial journal coverage.
- Script condition inheritance between successfully initialized slots, native
  pedal/release conditions, and additional supported script syntax and zone queries.
- Native NKSN snapshots applied to an explicit base instrument, including supported
  saved controls, instrument/group FX, known envelopes and modulation assignments.
  Three Analog Strings snapshots have production state checks and two decoded IRs each.
- Bounded asynchronous `load_array_str` reads and explicit-path `save_array_str`
  writes for typed NKA files, with retained array/UI revisions, completion callbacks
  and header, value, resource, capacity and write diagnostics. Three real Analog
  factory loads and the retained rhythm table are verified; read callback/install/
  refresh paths record zero heap operations over 7,800 blocks of 128 samples. Two
  unchanged-script browser-star callbacks write a copied favorites file with fresh
  byte readback, restore its original bytes and record zero audio heap operations
  over 6,300 blocks. All 11 original metadata files remain unchanged. Mode-based
  saves and external file dialogs remain unavailable, returning status 0.
- AHDSR/Flex envelope record writers that preserve opaque metadata. The selected
  782-file / 788-program corpus verifies 230,627 byte-exact record roundtrips and
  edited-value readbacks with zero errors.
- Typed native LFO parsing and writing for known fields, with 9,600 actual-library
  chunks round-tripping byte-for-byte. Eligible retriggered sine Multi pitch
  sources now play with proven legacy unsynchronized fade-in; broader waveforms,
  free-running clocks, synchronized fades and live frequency remain unsupported.
  Some tables remain raw, and typed metadata alone
  does not establish DSP behavior.
- Named bitmap font loading for 256-glyph Windows-1252 RGBA strips declared during
  initialization. Actual Areia Advanced resources verify two 256-glyph, 14-pixel
  fonts with variable advances and the native gray/orange switch-state change;
  broader font compatibility remains unverified.
- Internal slot-to-slot MIDI2 note-controller callbacks, registered/assignable/bend
  values, forwarding, waits and startup persistence delivery. External MIDI2 input
  and multi-script MIDI-input callbacks remain absent; the selected 13-script
  census contains no uses, so this does not establish an actual-library benefit.
- Lossless native v2 filename-table records preserve segment kinds, UTF-16 units,
  full timestamps, uninterpreted sample records and trailing metadata. Three actual
  tables covering 102,026 sample references pass byte-exact and edited readback checks.
  Existing raw chunk writing was already lossless; this adds typed editing access.
- Imported zero-crossfade alternating sample loops share reflected playback mapping
  between resident and streaming readers. Cache and NKI export retain their direction.
  Crossfaded alternating loops retain metadata and warn about forward-crossfade fallback;
  endpoint/interpolation equivalence with Kontakt remains unverified.
- Opt-in native UI timing capture records one bounded ten-second drag window and
  summarizes it on the diagnostics worker. It measures application callback and
  presentation-submission time, without forced GPU synchronization or display-FPS claims.

### Changed

- Paced headless audit JSON records first stream underruns, voice consumption and
  delivery status, and labels the uncapped voice pitch ratio precisely. The checked
  JSON helper and source/onset metrics are observability features, not additional
  fix-count units or throughput improvements.

- Rack headers give instrument titles more room beside
  compact MIDI and output routing controls. Combined 900/1180/1920 viewport
  bounds and routing/navigation/mute/remove callback checks pass.

- Logs search is simpler and copying includes complete retained diagnostic details;
  event text is owned before the query changes. Rack header artwork is more visible.
- Encrypted preset access failures explain the lookup boundary; XML fields tolerate
  surrounding whitespace. These diagnostics do not add keys or bypass encryption.
- Group processing follows decoded pre/post Amplifier insert order for supported
  filter and drive stages. Interleaved filter states remain out of shared lanes;
  effect indicators disclose partial processing instead of implying every FX runs.

### Fixed

- Authored performance-view background colors survive live publications; both
  focused background gates passed. This does not establish every layout or Lua UI.
- Native AR identities select the intended LP/HP/BP response and pole count. Native
  Daft LP/HP identities use IDs 70/71; unproved IDs 106/107 remain unsupported.
  These are two separately reviewed family-identity corrections.
- Native Daft cutoff/resonance decode after the retained leading parameter.
  Coefficient/PCM changes, live readback and zero-heap checks passed; the leading
  parameter remains preserved without an invented gain law.
- External modulation v0x104 records preserve opaque footer bytes through checked
  vendor parsing/roundtrips. This does not implement every route or opaque field.
- Physical note-off ownership survives channel mode changes. CC120 stop fades
  clear held ownership so late key-up cannot start post-stop release tails.
  These are separate routing and All Sound Off corrections; the physical-sibling
  follow-up is part of the same CC120 fix, not another count.
- Higher native LoFi frequency values sample more often without resetting held
  samples or clock phase. Authored direction/endpoints/retuning and zero-heap
  checks passed at 44.1/48/96 kHz. Six matched actual checks changed four outputs
  toward effect-off baselines and left two unchanged; the law is not calibrated
  against Kontakt and sound parity remains unverified.
- The first maximum-consumption stream block remains resident; the 4,096-frame
  RAM-coverage and zero-heap gate passed. This does not claim that all realtime
  underruns are eliminated.
- Documented `ui_menu` `$CONTROL_PAR_VALUE` queries return the selected entry
  index. Native persisted menus restore entry positions with retained origin/
  host state. Getter and persistence are two distinct corrections; both gates passed.
- Template-named snapshots bind to the checked instrument while rejecting foreign
  names/truncated metadata. Explicit modulation removals retain sibling identities;
  additions, replacements, malformed records and incompatible FX topology remain
  rejected. Binding and removal are two independently reviewed fixes.
- GPU startup diagnostics retain their cause and identify Linux embedding failures.
  This improves diagnosis; repair of Void blank windows or every host embedding
  failure remains unverified.
- Headless runtime faults retain callback/event actions, array variable/index/
  length and bounded readable source context. The zero-heap gate passed; repeated
  events update the latest action without growing or duplicating retained faults.
- Ownerless UI/listener CC and MPE actions use the configured part home; the
  routing/zero-heap gate passed. MIDI-owned callbacks retain their ownership.
- Legacy AHDSR cutoff intensity reaches the filter through the checked cubic depth
  law. Authored PCM/readback/zero-heap gates passed; original preset processing and
  calibrated Kontakt sound parity remain unverified.

- Persistent load journals retain warnings beyond the bounded report examples;
  an actual Areia load delivered all 1,125 warnings. Off-thread burst capture and
  export also retain 4,098 warning records with explicit delivery status. These
  are two distinct corrections: capture coverage and delivery under bursts.
- Explicit scripted AHD Only uses the shared envelope kernel; its focused gate
  passed. This does not establish every imported envelope or Kontakt's sound.
- Script-only Kontakt v3 snapshots apply saved script state without replacing
  native instrument state. Vendor checks and independent application of three
  actual Conflux presets with audio outputs passed. Initial realtime underruns
  were observed; later four preloaded Conflux runs at `1f62da4` had zero underruns
  and complete journal delivery, without a general realtime-performance claim.
- Library-root archive samples resolve from nested instrument directories.
  Authored checks, actual archive members and 1,985 playable Conflux zones passed;
  loading does not establish full-library playback or sound parity.
- Counted native modulation arrays preserve 64 external slots, high slot identities
  and unknown bytes. Vendor and actual byte-preservation checks passed; preserving
  records does not demonstrate processing every modulation source or target.
- Bounded Kontakt v0x103/v0x104 source headers retain known identities without
  interpreting opaque source state. Modern v3/v4 compact snapshot records preserve
  mode-dependent source fields and counted slots through checked roundtrips.
  These are parsing/preservation fixes; the later three checked Morphology
  applications below do not establish complete native processing.
- Release-trigger Note Mono cuts only matching sounding release tails and remains
  preserved when writing native start-condition records. Both production checks
  passed; arbitrary imported start conditions remain outside this validation.
- Native group Send Levels taps feed the existing instrument returns in decoded
  order. Analytical dry gain, return tails and zero-heap checks passed; this does
  not establish Kontakt bus assignment or sound equivalence.
- Fractional image resizing preserves coverage and transparent edge colors while
  retaining already enlarged pictures instead of resizing them repeatedly. Both
  focused image/UI checks passed; no GPU throughput or universal resource claim.

- Ordinary import preserves readable modulation slots when one bounded sibling
  record is undecodable. Unknown slots keep their positions and precise warnings;
  valid envelopes and later target identities survive. Snapshot decoding remains
  strict, and malformed container boundaries still fail.

- Process all eight native filter/EQ inserts and up to 32 sections instead of
  truncating supported chains. Eight four-band GEQs match the existing rack
  reference with live slot-7 edits and no audio heap operations; fixed state
  increases by 1,248 bytes per voice.
- Format, clone and retire large persistent script/native values outside the
  editor mutex. Revalidate the script epoch before committing a snapshot;
  unchanged host JSON and prepared audio buffers are retained.
- Clicking either diagnostic row text line selects the event, as does its blank
  area. Native Copy all verification retained 148 events and eight source excerpts.
- Rejected zone mappings identify the offending field/value, original ranges,
  source/version, zone, group and sample. Validity checks remain strict.
- Preserve native GPU surface errors and flush startup stages before driver calls.
  This improves blank-editor diagnosis; a Windows driver crash is not reproduced.
- Read generated dependency license JSON explicitly as UTF-8, fixing Windows
  packaging on a CP1252 default locale. The non-ASCII generation regression passes.

- Native group feedback compressor, limiter, Solid Bus
  Compressor and Transient Master stages reuse bounded rack processing at the
  decoded Amplifier split. Rack-reference PCM, native edit/readback and zero-heap
  checks pass; this does not establish native Kontakt parameter or sound equivalence.
- Import and NKI writing retain native group start records.
  Record preservation does not implement every start condition or establish
  arbitrary imported-preset editing.
- Periodic audio snapshots wake a separate managed worker
  during instrument loads. Cumulative playback counters retain their baseline
  across generation changes, preventing repeated totals from appearing as new
  drops or underruns; independent wakeup, bounded handoff and exact delta
  regression checks pass.

- Parse/runtime diagnostics now show readable bounded source context with slot,
  line/column markers and the relevant command arguments. Serialized event data
  keeps the excerpts visible in journal/export and Logs Copy all; both exact
  diagnostic regressions passed on the corrected compiled binary.

- Decode complete extended PCM WAVE format descriptors, including the checked
  20-byte fmt records; two actual supplied samples decode fully. All 15 authored
  descriptor cases and the combined audio regression passed.
- Reveal actual library/log directories after validating the path, including
  extended Windows drive and UNC spellings. Explorer launch remains unverified.
- Resolve relative encrypted-preset paths and whitespace-delimited library access
  fields with actionable missing-data errors; right-key and wrong-key checks pass.
  The unavailable Emotional Piano payload has not been independently validated.
- Accept documented symbolic MAIN/GROUP/INSERT level-meter chain selectors while
  retaining rejection of invalid selectors. This is not a claim that every meter
  source works. The actual checked preset initializes three script slots with
  378, 22 and 1 controls without errors; Lua, unsupported taps and full playback
  remain separate limits.
- Accept Creator Tools null menu lists for exported empty menus. The actual checked
  Conflux resource loads 378 controls in 11 families; 100 callback operations show
  no measured heap operations. Lua UI and unsupported level taps remain explicit.
- Preserve independent script-slot pages and their callback ownership through
  footer changes, delayed publications and replacement epochs. Authored UI/backend
  checks pass; the Circle Bells payload was not available for validation.

- Forward initialization RPN messages after receiving script slots initialize.
- Persist global editor window size instead of losing the saved size between editors.
- Decode native Kontakt 8 flat filename tables and explicit effect-slot identities;
  retain supported records instead of rejecting their valid layout or misreading slots.
- Archive errors retain directory signatures and exact read boundaries. Rejected
  samples and loop bounds retain their actual failure cause and typed skip counters.
- Count KSP faults omitted by the retained source-location cap, and retain command,
  argument, signal and runtime-value context for invalid note/listener operations.
- Dispatch the documented legacy PGS callback spelling.
- Keep the rack welcome drop area and scrollbar gutter stable while accepting
  append drops on the empty canvas.
- Resolve declared JPEG resources and route decoded module envelope bypass and
  supported modern target depths through the existing processing paths.
- Schedule millisecond and beat listeners independently; registration, disabling
  or retuning one clock no longer overwrites the other clock's phase or generation.
- Preserve release tails during offline overload rendering.
- Preserve native group insert order around the Amplifier, including separate
  state for interleaved filters and honest partial-effect indicators.
- Resolve the authored compressor native ID through its documented KSP names.

- Group drive processing retains all eight native insert slots. A third drive
  stage was previously discarded, leaving Analog Strings' Saturation control
  editable without reaching its DSP.
- Original views retain unchanged control subtrees across live publications,
  while changed table rows, active gestures and replacement epochs rebuild.
- Windows editors default to Direct3D 12 instead of implicitly initializing
  Vulkan. Explicit `WGPU_BACKEND` selections remain authoritative; renderer
  startup, adapter details and recoverable failures enter persistent diagnostics.
  This avoids the reported Intel Vulkan path by default, but has not yet been
  verified against that FL Studio crash on the affected machine.
- Live UI refresh skips unchanged menu rows, and repeated identical indexed
  integer writes no longer dirty entire table snapshots. Listener behavior and
  audio work budgets remain unchanged.
- Import reuses one decoded filename table for samples, resources and impulses,
  avoiding three repeated full-table decodes in large instruments.
- Failed or canceled load reports retain their status and cause when diagnostics
  or artwork from the active instrument arrive later.
- Native SV Notch 4 filter type 58 uses the existing four-pole processing path;
  an actual Accordia resident-sample render now changes its PCM with zero render
  heap operations. This does not establish Kontakt sonic equivalence.
- Vectorized views retain broad pictured value graphs and their authored
  callbacks while continuing to replace ordinary knobs and faders.
- Compiled script programs share immutable UI revision-owner maps between
  runtimes. Revisions and mutable values remain local; matched Areia callback
  measurements show unchanged cost, without a runtime-speedup claim.
- Leaving or unfocusing an editor cancels delayed pointer restoration, including
  release events queued before the next frame. Popup menus capture hover and outside
  dismissal clicks so tooltips and underlying controls cannot cover or activate them.
- Solid G-EQ gain captions use the existing DSP's signed decibel conversion instead
  of raw normalized integers; this changes display text without changing its gain law.

- Shared controls recover movement before the drag threshold, so closed physical
  mouse paths return to their starting values at 100%, 150% and 200% display scale.

- Host position, tempo, play/stop and time signature reach script callbacks before
  MIDI input. Song position advances within the block at callback sample offsets;
  bar duration follows the host meter. Start/stop listener subscriptions are independent.
  Beat listeners still use elapsed-clock phase; missing host timeline validity is
  an upstream limitation, so unavailable beat position cannot be distinguished from zero.
- Repeated same-sample group parameter restores use a preallocated address lookup.
  The checked Areia Core F3 channel-overlap burst drops zero writes instead of 854,
  without increasing queue capacity; measured event-plus-render time stays near
  baseline at 3.56 versus 3.53 ms. Distinct-sample timing and latest-value reads remain.
- Instrument replacement clears previous script state and convolution settings;
  source epochs on both live-request and snapshot queues reject stale updates.
  A native Areia-to-CHORUS transition verifies the new logo, controls, header and
  playable range, resolving the observed cross-instrument state contamination.
- Physical note ownership, delayed callback cancellation, MIDI stop ordering and
  selective sound-off across articulation channels sharing one engine channel.
- Sustain release bookkeeping, generated-note lifecycle and MPE expression/tuning
  routing in the covered playback paths.
- Script-selected native release groups now start for all note durations when
  automatic release triggering is bypassed. Vista Harp and two Pacific Solo Harp
  presets restore exactly four damper voices on ordinary and pedal releases;
  their envelope/swell behavior still lacks Kontakt reference validation.
- Bounded physical-input CC120 cleanup and Panic termination for previously lingering
  Areia, Dolce and CHORUS script lifetimes. Reset restores recorded library device
  defaults alongside standard controllers in native playback and scripts, fixing
  all 12 previously silent legato fresh-note cases in the focused rerun.
  The six-patch, 24-case run covers 670,704 blocks with zero measured render-thread
  heap operations, nonfinite samples, drops, underruns or offline duration overruns;
  every fresh post-Panic voice has positive gain/envelope levels and final held,
  voice and pending state is clear. Three renders with effects disabled confirm
  audible CC121 recovery and baseline-matching CC120/Panic recovery.
- Zone ID mapping after import filtering and source-parser allocations in validated
  preset/container paths.
- Browser scaling/layout and selected DSP effect processing paths.
- RV2 Reverb Time captions use the existing DSP time conversion; the checked
  Areia Advanced state displays 1099.5 ms. This does not establish Kontakt's law.
- Original interface wallpaper page offsets and viewport rendering, authored fader
  travel, factory font color/state inheritance and explicit caption text colors.
  Factory glyphs still use the bundled font approximation.
- Editor publication of live script views through the existing bounded buffers,
  without formatting diagnostic reports or writing journals in editor frames.
- Delayed script callbacks now publish font, caption alignment and text-offset
  changes, including Analog Strings' centered Original-mode volume/FX captions.
  Native Original captures cover 27 playable cases across nine libraries at
  device scale 1.5; selected callbacks are checked, not every control action.
- Warm Vectorized CPU planning drops 78% in one bounded paired Analog trial
  (2.567 to 0.566 ms). This does not establish overall GPU/frame/input latency.
- Scalar edits avoid copying the imported interface under the view lock. A matched
  Analog trial measures mean edit submission at 1.152 to 0.000240 ms; whole observed
  frame means are 4.223 and 4.395 ms, providing no frame-rate improvement evidence.
- Changed publications reuse unchanged control storage; a separate matched Analog
  trial measures publication mean at 0.122 to 0.047 ms. Its final macro update copies
  3 of 934 controls; startup updates copy more. Drawing uses published revisions
  instead of scanning all control properties every frame. Native FPS remains unverified.
- Internal pitch AHDSR routing to voice modulation, selected filter coefficients
  and worker-built convolution cutoff processing. These changes do not establish
  Kontakt parameter-law or sonic equivalence.

### Compatibility

Kontakt preset import, scripts and playback remain partial. Successful import or a
passing synthetic test does not establish sonic parity for every library. Existing
compatibility notes and unsupported-operation diagnostics remain applicable; these
changes do not announce complete format, script or sound equivalence.

The focused offline results do not certify live-host deadlines or every library.
Opaque snapshot source state and unknown saved scalars are warned and remain unapplied.

Analog Strings' live factory-preset and rhythm menus use bounded NKA reads; selected
menu checks do not establish that every action or preset works. The installed
header-favorite preset ID is absent from its registry, preventing that lookup from
updating favorites; no supplied IDs were repaired or compared with Kontakt.


### Known limits

- The `1bbd31c` actual six-Analog replay returned empty runtime-fault lists;
  Prelude3's earlier out-of-bounds fault was absent. Three native convolution IR
  ordinal/caption selections and host roundtrip ran through 750 blocks. These
  validate the checked processing/state paths, not Kontakt IR sound equivalence.
- Three actual Morphology snapshots (Default, PlipPlop and Whistling) applied to
  the checked bank with 933 groups, 3,495 zones and 458 samples, with zero init/
  runtime faults, nonfinite samples or observed underruns. Partial source warnings
  remain. Each run recorded `truncated_events = 1`: its roughly 91.8 KB
  `load_finished` information event exceeded the event bound. Writer-drop,
  write-error and retention-loss counters were zero; truncation was not zero.

#### Semantic coverage and remaining gaps

| Area | Checked scope | Remaining gap |
| --- | --- | --- |
| Snapshots/source records | Known bounded headers, counted slots, template binding/removal; three actual Morphology applications | Opaque source state, arbitrary topology edits and complete-library compatibility remain unsupported/unverified. |
| Native FX | Authored AR/Daft identities/layout, cubic legacy depth, live PCM/readback; six matched actual LoFi checks | Original Kontakt frequency/drive/depth calibration and full processing parity remain unverified. |
| Controllers/menus | Authored configured-home routing, value indices, persisted positions and physical ownership; actual six-Analog replay | Every library callback, Lua UI and host workflow remain outside this evidence. |
| Diagnostics/editor | Bounded actions/excerpts and startup causes; zero writer-drop/write-error/retention-loss in the checked Morphology runs | One truncated information event per Morphology run; Void window/embedding repair remains unverified. |
| Streaming/convolution | First-block RAM/zero-heap gate; checked zero-underrun preloaded runs and native three-IR selection/host roundtrip | Uninterrupted realtime performance and Kontakt convolution/sound parity are not established. |
| Wavetable DSP | Resident 2048-frame cycles, note-based pitch, live position and Linear/ASYM2MP phase forms; nine actual Conflux captures | Native bandwidth-table preparation and High/Best anti-aliasing remain missing; no complete wavetable or preset playback parity claim. |

- The accepted parsing/preservation gates cover bounded known source headers,
  modern compact snapshots and counted modulation slots. Three later checked
  Morphology snapshots apply, but successful decoding/application does not
  implement unsupported DSP or establish complete-library compatibility.
- Three actual Conflux snapshots apply independently, and library-root resolution
  loads 1,985 playable zones. Initial runs had realtime underruns; four later
  preloaded runs at `1f62da4` had none. These limited checks do not certify
  uninterrupted performance, complete-library playback or Kontakt sound parity.

- Root's shipping-profile focused checks through `e68d28c` passed 20 library checks
  and all 83 playback checks (4 ignored). NI's 13 focused checks passed. These results
  do not certify every actual library or a Windows/macOS DAW.
- Actual Conflux import/initialization at `0c0019a` reports no initialization
  errors and exposes 378 controls. This does not validate full playback, Lua UI,
  every control or actual meter signals; unsupported level taps remain explicit.
  Circle Bells' multi-page interface was not available; authored page tests do
  not certify that library.
- Source excerpts retain up to five source lines, each clipped at 512 UTF-8 bytes
  without splitting characters, plus labels/column markers. They intentionally
  retain readable source text; full raw scripts are not included in copied reports
  or support bundles. Missing source/locations cannot produce an excerpt.
- Bitwig migration is an experimental partial copy, not a verified replacement
  project: Kontakt automation/static host values are not translated, routing comes
  from an explicit SavedMulti, and original Kontakt bus assignments remain undecoded.
  VST2/CLAP instances are not classified; Bitwig reopening and sonic parity remain
  unverified. Unmapped devices stay unchanged, and shared-state migration is refused.
- Actual module-envelope callback checks retain 483 groups, 480 bypass sources and
  959 of 960 known filter/formant depth targets across three Analog Strings states;
  one opaque target remains unsupported. Finite PCM differences use an authored
  stimulus and replacement sample map with flattened buses and omitted program FX.
  This does not validate stock legacy depth controls or original-map/Kontakt audio.
- Native group insert-order checks cover rack split references, threshold changes,
  subtype retuning and 83 playback cases. Three factory-state probes perform 4,500
  finite resident renders without heap operations; their reference is the same
  engine with authored routing changes. Added inline fields measure 116 bytes per
  voice on the checked Rust 1.98.1 build, not a throughput or parity improvement.
- Wavetable record decoding alone does not establish DSP behavior. Partial group FX,
  decoded envelopes and selected compressor IDs do not establish Kontakt sonic
  equivalence; unsupported processing remains visible.
- Larger racks remain bounded by memory, voice budgets and host/editor capabilities.
  User zoom, cached snapshots and native file-picker tests do not certify every
  gesture, resource, preset or file-dialog workflow.

### Fixed

The 14 newly accepted logical defects are source-reviewed and recorded individually
in `release-fixes.json`; follow-up and safety fixtures do not add counts. The previous
published 0.3.96 binaries are unchanged. These entries describe the next source cut.

- The direct Rust VST3 event-admission path accepts unused optional note-length
  metadata without shortening retained ownership, including negative hints.
  Sources: `a357e1e`, `401d59e`. The native VST3 adapter in this 0.3.115 cut still
  drops negative optional lengths before this path; end-to-end native delivery
  and the reported Mac keyboard/clip silence are not established by these tests.
- Legacy unsynchronized Delay Time uses the checked conversion across its admitted
  absolute range. Sources: `f191cc2`, fractional fixture follow-up `aaf2959`.
- Delay writes the current input before advancing its ring clock, preserving the
  intended sample position. Source: `4ec2958`.
- Native Distortion Damping uses the checked native law. Source: `b3e453e`.
- Distortion's native DC filter runs after Damping with the checked scalar/SIMD
  summation order. Sources: `74e3c0e`, `cd940e2`; one DC-processing outcome.
- macOS plug-in packages use the signed standalone app’s imported CLAP/VST3
  package-type declarations and user-context Launch Services registration. No
  FinderInfo attributes are added to signed code. ZIP extraction and installed-
  payload gates check Foundation/Workspace package recognition
  and both plug-in factories. Sources: `6001126`. Actual hosted Mac execution is
  still required; this source correction is not a Bitwig scan or playback result.
- VST3 editor views retain their component while alive. Source: `3e76d60`.
- VST3 resize frames retain their owner through attachment and replacement.
  Source: `3784c67`.
- VST3 run-loop registration follows the currently attached view.
  Source: `e3822b2`.
- Attached GUI view ownership is independent of creation order. Source: `be80724`.
- Native editor startup and thread information is persisted before entering the
  platform window path, preserving context if startup fails. Source: `c3763d1`.
- VST3 initial cents tuning is applied atomically with note onset, including bounded
  extreme finite values. Sources: `53ab06f`, `2d5822e`.
- Zero-rate integer sample taps avoid the reproduced zero-stride iterator panic.
  This shared renderer correction is distinct from atomic initial tuning.
  Source: `4529cbf`.

### Added

- Confirmed previous-session host crashes recover accessible native evidence and
  complete sanitized diagnostic activity through the reused BUFFR reporter.
  This is the fourteenth accepted defect outcome: reliable crash-evidence recovery
  and acknowledged delivery, grouped with its retry/cancellation follow-ups.
  The private report includes the full sanitized evidence; the public issue is
  limited to safe platform/host/format/build/failure metadata. Automatic reports
  exclude library source excerpts, samples, access data and credentials. Full-hash
  acknowledgements govern delivery; failed or delayed reports remain retryable.
  No host-wide crash hook is installed, and host crashes do not establish KONTRA
  fault ownership. Sources: `665ffee`, `72d3b9b`, `0c7b622`.

### Known limits

- Frozen optimized source `d7b9e11ad096283aab06c79c15732d9bd5a48a03` passes
  all 547 library tests (30 ignored), including 30 support tests, and 89 playback
  tests (4 ignored). Dedicated extreme-onset, zero-stride PCM and exact scalar/SIMD
  checks pass in executable prefix `eebeb2eb1efd`. The clean CDLL prefix
  `d504c6786bb0` embeds that exact revision; its actual Linux VST3 factory exposes
  one KONTRA class with unchanged class ID and releases all factory references.
  Strict C++ editor attachment/lifetime gates pass on unchanged shim sources.
  These are optimized Linux and driver-free checks, not native Mac or every-host
  embedding validation. The tested executable carries its original 0.3.101
  metadata; the accepted ledger derives the next package version as 0.3.115.
- Native macOS Swift/Finder/factory checks, Developer ID signatures, Installer
  execution and Apple acceptance remain required hosted shipping gates for this
  cut. Existing 0.3.96 notarization does not verify these changed bundles.
- Crash recovery/delivery fixtures do not establish a real-world host-crash-to-server
  end-to-end result. Native evidence depends on accessible host/OS artifacts;
  public/private rendering is a support-service contract. Manual local exports may
  retain bounded, reviewed source excerpts; automatic reports exclude them.
- These corrections do not implement Conflux's native Lua interface or establish
  complete Kontakt processing parity, every host workflow or complete-library
  compatibility. No production crash report or synthetic public issue was sent
  as part of the test evidence.

### Candidates — not shipped

- Audit/source-identity metrics, the checked CLI JSON helper and delivery
  instrumentation are observability features, not additional logical defects.

### Reviewed source changes

> Retain authored performance-view background colors through live publication

> Correct native AR and Daft filter identities and stored parameter order

> Decode Kontakt external modulation v0x104 with opaque footer retention

> Preserve physical note-off ownership after channel mode changes

> Correct native LoFi frequency direction without claiming calibrated parity

> Keep the first maximum-consumption stream block resident

> Decode each RAM reference in the stream-start fixture

> Cancel held voice ownership immediately on All Sound Off

> Return selected menu index from KSP VALUE getter

> Bind template-named snapshots and apply explicit modulation removals

> Retain GPU startup causes and diagnose Linux embedding

> Retain runtime fault actions and source context in headless diagnostics

> Route ownerless script controllers through the configured part home

> Route legacy AHDSR cutoff intensity through measured cubic depth

> Restore native persisted menus by entry position

> Require init callback context in keyboard fault regression

> Journal load warnings beyond bounded report examples

> Implement explicit scripted AHD Only with the shared envelope kernel

> Support script-only Kontakt v3 snapshots without replacing native state

> Resolve library-root archive samples from nested instrument folders

> Decode counted native modulation arrays with 64 external slots

> Read bounded native source identities in Kontakt v0x103 and v0x104

> Preserve load diagnostic bursts and report audit delivery status

> Preserve and apply release-trigger Note Mono

> Decode and preserve modern v3 snapshot and v4 compact source records

> Route native group Send Levels taps into existing instrument returns

> Preserve fractional image coverage and transparent sprite edges

> Forward init RPN messages after receiving script slots initialize

> Simplify Logs search and copy complete retained diagnostics

> Make rack header artwork slightly more visible

> Persist global editor size and add independent UI zoom preferences

> Expose cached instrument snapshots in a compact preset row

> Decode Kontakt 8 flat filename tables and explicit effect slots

> fix(nkx): report directory signatures and read boundaries

> fix(samples): report rejected zone and loop bounds

> fix(diagnostics): count KSP faults omitted by the location cap

> Dispatch the documented legacy PGS callback spelling

> Append browser drops anywhere in the rack empty canvas

> Resolve declared JPEG resource names with shared image decoding

> Preserve native group Amplifier insert split metadata

> fix(load): classify skipped zones by their actual failure cause

> Route decoded module envelope bypass and modern target depths

> feat(ksp): route file selectors through native picker callbacks

> Own event detail text before updating the Logs query

> fix(diagnostics): serialize typed zone skip counters

> Load exported performance-view controls before KSP compilation

> Enforce the documented performance-view initialization constraint

> Keep snapshot categories in owned menu labels

> Check performance-view slot results using the runtime result type

> Keep rack welcome drop area and scrollbar gutter stable

> Inspect bounded source identities and report unsupported Kontakt 8 wavetable playback

> test(ksp): verify selector callbacks and prepared paths

> Accept Creator Tools null lists for empty exported menus

> docs: track generic compatibility gaps and validation boundaries

> fix(ksp): retain command and arguments in note validation faults

> Schedule KSP millisecond and beat listeners independently

> Check independent listener delivery on the allocation-free audio path

> Retain listener command and argument details in bounded faults

> fix(render): retain release tails during offline overload

> Preserve native group insert order around the amplifier

> Report fixed group voice state size in the pipeline proof

> Keep group effect indicators honest about partial processing

> Keep interleaved group filter states out of shared lanes

> Remove the rack part cap with prepared storage and viewport rows

> Resolve the authored compressor native ID through KSP names

> Add persistent library display names and validate native Reveal paths

> Explain encrypted preset access lookup and accept XML field whitespace

> Use the detached catalog for library display aliases

> Record complete nightly notes and count reviewed logical fixes

> fix(audio): decode extended PCM WAVE format descriptors

> Show bounded script source context with parse and runtime diagnostics

> Expose authored performance pages with independent script-slot buffers

> Advance to 0.3.18 for eighteen reviewed fixes

> Use imported tab helper for performance page selectors

> Fix source excerpt regression lease and report serialization

> Accept documented symbolic level-meter chain selectors

> feat(migration): inspect and migrate Bitwig copies with shared-state guard

> Record validated native group pipeline and actual state budget

> docs: record actual module envelope callback validation boundaries

> Describe reviewed source batch and omit unshipped candidates from notes

> Advance to 0.3.22 after four further compatibility fixes pass

> Read source excerpts from serialized diagnostic event data

> Advance to 0.3.23 with verified readable script diagnostics

## 0.3.0-nightly.20261002.g4399f700e590 — 2026-10-02

Public release source: `4399f700e5904114ebfdf84ef50091716f5867c7`.
Reviewed export checkpoint: `e53150559f37407197be3d6182aed9a1c3619e89`.
Previous release source: `521e6954749840ab15c3d9365d7cd3e95a4867ea`.

### Added

- Load supported NKSN snapshots from the instrument header picker or a header
  drop, after loading their base NKI. The loader validates the base and applies
  supported snapshot state before changing the active source. Both source paths
  survive DAW state and KONTRA multis; rejection preserves the existing source
  generation, installed epoch and source services.
- Persist successfully applied native script parameter edits alongside persistent
  variables. Restore authored initialization getters, retain decoded values actually
  applied by the engine, and replay current effect edits after processor rebuilds.
- Process all eight native group drive insert slots. Previously a third drive was
  discarded; actual Analog Strings Tube SHAPE/BYPASS edits now reach both groups'
  DSP. Matched sample renders are finite and change with the edit, with zero render
  heap operations over 2,250 calls per comparison side.

### Changed

- Save active native edits with prepared address storage and a reusable numeric-key
  hasher; refresh edited parameter slots rather than every default parameter.
- Share immutable compiled UI revision-owner maps between runtimes while keeping
  revisions and mutable values local. This reduces duplicate retained metadata;
  native-state persistence separately added about 0.08 ms in the measured Areia case.
- Avoid unchanged menu-row copies and redundant bounded live-refresh passes. Equal
  indexed integer writes leave table revisions current; script callbacks are retained.
- Decode large filename tables once for samples, resources and impulses. All three
  actual-library fingerprint comparisons match the prior implementation.
- Retain unchanged Original control subtrees across live publications. Changed rows,
  active gestures and replacement epochs still rebuild their affected controls.
- Keep compact articulation-mode help readable and preserve broad pictured value
  graphs with authored callbacks in vectorized views.

### Fixed

- Default Windows editors to Direct3D 12 rather than implicit Vulkan. Explicit
  `WGPU_BACKEND` choices remain authoritative. Persist renderer initialization,
  adapter details and recoverable startup failures in diagnostic journals.
- Map native SV Notch 4 filter type 58 to the existing four-pole DSP path. An actual
  Accordia resident-sample render changes its PCM without render heap operations.
- Preserve failed and canceled load status when later active-script diagnostics or
  pending artwork report warnings. A rejected snapshot retains its failure status.
- Replace a cached loading placeholder with the completed failure or empty-source
  state; include loading/status changes in the performance-view cache dependency.
- Preserve NIS/NKS decoder family, cursor/offset, declared lengths, available bytes,
  version and chunk context, including decompression and structured-object errors.
  Recognized malformed NIS files retain the original decoder cause; invalid metadata
  returns an error instead of panicking. Valid synthetic container roundtrips remain
  byte-exact, with truncated-container coverage preserving the underlying EOF cause.
- Make the Logs export regression wait for a newly started request and a re-enabled
  UI scene, rather than accepting a previous completion. It still proves an existing
  destination fails without overwriting the earlier report.
- Extend native-edit, snapshot rejection and save/reload fixtures to use resident
  banks, settle persistence, retain source generations and avoid dumping values.

### Known limits

- The Windows backend policy has not been tested against the reported FL Studio
  crash on the affected machine. Hosted Windows/macOS builds do not certify DAW use.
- Snapshots require their base NKI and cannot open independently or target programs
  inside an NKM. Opaque source state and unknown saved scalars warn and remain unapplied.
- Native drive/filter support and finite sample renders do not establish Kontakt
  parameter-law or sonic equivalence. Compatibility remains partial.
- Control reuse, owner-map sharing and bounded refresh proofs do not establish native
  display FPS, GPU completion latency or a whole-runtime speedup.
- Exact-source hosted CI passed 410 library and 78 playback tests. The downloaded
  Linux package passed 38 CLAP checks (6 skips, no warnings/failures) and strict-level-5
  VST3 validation at 48 kHz/128 samples with GUI checks skipped. These scoped checks
  do not validate every library, host deadline or Windows/macOS runtime.

### Reviewed source changes

- Load factory snapshots through the plugin worker and persist their source
- Keep old snapshot state regression independent of later appended fields
- Map native SV Notch 4 filter id to existing DSP
- Share immutable UI revision ownership across script runtimes
- Journal snapshot validation and preserve active source services on rejection
- Keep snapshot rejection commits in source request view lock order
- Finish validation trace before locking the visible snapshot report
- Correct snapshot trace value and worker proof return types
- Measure notch pass bands by normalized signal power
- Preserve applied native script edits across host-state restoration
- Replay current native effect edits after processor rebuilds
- Validate saved native addresses without scanning the edit list
- Report exact snapshot rejection fixture state changes without dumping values
- Use existing standard storage and share the native-state test instrument
- Install a resident bank in the native edit callback fixture
- Preserve failed load status when publishing the active script diagnostics
- Settle active snapshot persistence before testing rejection and retain failed merge status
- Keep rejected snapshot status when pending artwork reports warnings
- Preserve broad pictured value displays in vectorized views
- Keep articulation mode help readable in compact editors
- Snapshot only active native edits and reuse the locked numeric-key hasher
- Document snapshot loading and native control state restoration
- Assert rejected snapshot retains source generation and installed epoch
- Capture the decoded native value already applied by the engine
- Skip unchanged KSP menu rows during bounded live refresh
- Decode large sample file tables once per import
- Leave KSP table revisions current when indexed integers are unchanged
- Default Windows renderer to DX12 and persist GPU startup diagnostics
- Document renderer policy and large-library refresh/import fixes
- Retain unchanged Original controls across live row publications
- Retain all eight native group drive insert slots
- Record retained UI controls and complete group drive slots
- Wait for fresh completed exports and an enabled UI scene
- Replace cached loading placeholders after instrument failure
- fix(import): preserve container decoder boundaries in load errors
