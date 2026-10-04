# Player backend boundary

This document describes optional UVI loading, live audio and instrument UI integration in the isolated `codex/uvi-latest-integration` working tree following `510446d`/`0a56891`, reviewed on 2026-10-04. The original integration audit and its line references below describe that checkpoint. Clean `0679223` is now installed as a local Linux checkpoint; installation does not establish a completed real-time performance proof. The broader common backend contract below remains proposed. No complete Falcon parity or support for unimplemented SFZ, Sine, Koda or other backends is claimed.

The user's concurrent core/MIDI refactor remains separately owned and unmerged. Its precision, protocol and lifecycle requirements inform this boundary; its dirty source is not an accepted API snapshot.

## Current inspection and sample preview boundary

At `0679223`, UVI publishes an immutable initial sample mapping and source-owned
script key declarations, with activation/source fences. These support authored
key ranges, layer inspection and scoped PCM/voice readouts. They do not establish
live script-mutated mapping, round-robin roles, signal roles or sounding ranges.
Identical key/velocity extents and duplicated sample paths are insufficient to
infer any of those roles.

The Mapping design prototype has a separate synthetic sample preview. It is not
the installed plugin's sample audition implementation. The existing plugin
audition request sends musical events through the selected instrument: it can
advance its Lua state and round-robin selection. Reusing that operation for a
button labelled sample preview would therefore be misleading.

A production raw-sample preview needs its own prepared PCM owner and audition
voice, with selection/source-generation checks, off-thread resource preparation,
bounded audio handoff and acknowledged retirement. Both Kontakt and UVI should
use this shared operation while keeping their native sample resolution and
playback laws explicit. It must not invoke instrument scripts, advance musical
round-robin state, or borrow the mutable native renderer. Such a shared audition
operation is not yet implemented here.

### Control feedback and presentation follow-up

The follow-up source enables the existing Original/Vectorized selector for an
owned current UVI performance panel. Vectorized replaces supported interactive
skins with theme controls while retaining authored layout, decorative artwork,
visibility, ranges, units and native callbacks. Original and Kontra retain the
previous UVI renderer; this is not a separate rebuilt Kontra panel. Unsupported
XY, waveform and meter displays remain explicit.

GUI queue admission now returns a checked FIFO ticket. Only GUI callers lock
ticket assignment and queue insertion together; the audio callback carries a
scalar in its fixed packet. The worker publishes a processed ticket with the
exact processor snapshot. Optimistic values settle from that snapshot's ticket,
not from a newer audio frame or another processor's refresh. Receipt means the
setter/initial callback dispatch returned or was nonfatally rejected. It does
not mean a yielded callback finished, PCM became audible, or later worker work
cannot fail. Callback order and the native scheduling clock remain unchanged.

This corrects feedback ownership; it does not remove worker buffering latency
or establish faster Lua/DSP processing. UI snapshot polling still uses the
existing serialized loader cadence. Focused functional and visual verification
must accompany adoption of this source follow-up.

The 2026-10-04 read-only main audit found a concrete porting requirement: current
`In` carries `midi::Velocity`, newer release/controller forms and physical ports
through `feed_on_port`/`dispatch_to_on_port`. This isolated adapter still accepts
the earlier event vocabulary. Preserve main's precision, port routing and
decode-once behavior when transplanting the native callback/delay hooks; project
to UVI's narrower event format only at its owned packet boundary. Main's internal
`engine/player.rs::Player` is not an exported neutral backend contract, and its
rack lacks this branch's source/delay hooks. These findings are coordination
requirements, not merged changes or a stable external ABI.

Main's new Kontakt NativeUI and isolated UVI also request incompatible Lua VM
features. A bounded Linux proof shows that separately built modules with local
Lua symbols and a C ownership boundary can coexist. See
[Player Lua runtime boundary](PLAYER_LUA_RUNTIME_BOUNDARY.md) for the measured
scope, lifetime requirements and unproved production/platform integration.

## Implemented loading and audio ownership

Kontakt remains the immediate `Engine` player. UVI has a concrete optional per-part endpoint feeding the same Rack mixer; it is no longer only a staged CLI/controller. `Part.uvi` is an appended native bank/UUID/member identity, preserving older positional state fields (`src/plugin.rs:108`). Live diagnostics read installed generation/failure atomics (`src/plugin.rs:438`). Rack still stores Kontakt engines, with parallel optional native endpoints in Dsp; this is an implemented two-player adaptation, not yet a neutral backend enum/API.

| Boundary | Current implementation | Ownership invariant |
| --- | --- | --- |
| New native selection | `src/plugin.rs:2256` prepares the requested worker; `2359` commits it after Ready through `Registry::adopt_prepared` | Initialization failure leaves the installed source intact. Commit transfers the existing worker/mailbox rather than reopening resources or starting a second worker. |
| Restored native parts | `src/plugin/uvi_load.rs:80` services each persisted part independently; `22` checks source, epoch, part generation, actual rate and admitted host maximum | Multiple restored parts have distinct controllers/activations. The single new-selection staging request is not the live player registry. Unsupported configuration is explicit. |
| Control owner | `src/plugin/uvi_control.rs:107` stores worker, mailbox, source/destination/context and retirement receipt keyed by epoch/generation | Loading, Lua/renderer ownership, image decoding and worker stop/join stay off audio. `prepare` and `adopt_prepared` are distinct paths into the same owner. |
| Prepared audio endpoint | `src/plugin/uvi_control.rs:255` extracts one AudioPort and allocates Bridge/Slot; `44` wraps the endpoint | No worker, mutable UI assets or resource authority is placed in the audio payload. The prepared endpoint carries destination and generation checks. |
| Delay preparation/adoption | `src/plugin.rs:2335` prepares all-slot delay storage; `4052` accepts an epoch/ticket/layout and acknowledges it | Different callback queues have no ordering guarantee. The loader waits for delay-storage acknowledgment before publishing native endpoints. |
| Player adoption | `src/plugin.rs:4117` checks activation/destination/generation/delay layout; `4139` installs the native endpoint | Rejected endpoints move to bounded off-thread retirement. Replacement defers if the retiring pool/discard path has no room. |
| Shared source/mixer | `src/engine/rack.rs:299` and `311`; plugin call at `src/plugin.rs:4523` | Native replaces only the slot source. Gain, pan, mute/solo, sends, buses, meters and scope remain shared and apply once. Muted sources still advance. Native failure returns silence while retaining source override, preventing accidental Kontakt fallback. |

The configured worker lead is explicitly 16 packets (`src/plugin.rs:44`). Bridge admission computes `L = ceil(max_host_frames / 256) * 256 + 16 * 256`, rather than calling the queue depth its latency (`src/uvi/bridge.rs:97`). The context comes from AudioConfig's sample rate and maximum host block at reset (`src/plugin.rs:3999`), not a fixed 48 kHz assumption. Current native admission requires an integral 8–192 kHz rate and host maximum 1–65,536 frames (`src/plugin/uvi_load.rs:91`). Each Bridge is prepared for that maximum and accepts arbitrary smaller render pieces into fixed 256-frame worker packets. Plugin retains its existing event/MAX_BLOCK splitting.

Delay admission distinguishes a pending growth/callback acknowledgment from terminal layout, allocation and 256 MiB budget failures. It memoizes the outcome by epoch, rack-slot count and maximum host frames. Matching unpublished controllers are cancelled and their UI leaves Loading with an explicit unsupported-configuration status; already exported players retain receipt-ordered ownership. The authored regression checks budget rejection, repeated loader passes without restart, and successful acknowledgment after a changed configuration. This does not establish arbitrary multi-part memory/performance support.

`SourceDelay` is shared engine machinery (`src/engine/source_delay.rs:10`), with loader-allocated per-slot storage in `src/plugin/uvi_delay.rs`. When native latency is active, Rack delays Kontakt main stereo **and independent direct outputs** before common mixing (`src/engine/rack.rs:344`). Native already carries L and bypasses that extra delay. The current policy gives all native endpoints the same configured L; heterogeneous backend latency compensation remains future work. Host-reported latency adds active native L to existing auto-alignment latency (`src/plugin.rs:4701`). Changing/resetting the latency context clears obsolete prepared history. These are timing contracts, not evidence that a paid instrument meets its deadline.

Bridge's live `process` uses preallocated bounded request/output rings and caller-owned left/right spans (`src/uvi/bridge.rs:291`). Missing PCM silences the entire affected packet; late PCM is discarded while its event/completion timeline remains intact. Queue-full submission retains the pending request for retry. Fatal transport/admission errors abort the activation and silence it explicitly. `process_offline` may wait for worker output (`301`); waiting is not part of the live callback contract. Allocating Lua, graph rendering and effect/resource preparation remain worker-side. Construction, growth, endpoint destruction and worker joins remain loader/teardown operations.

## Canonical events, completion and retirement

Core retains host protocol selection/reset, original physical provenance, exact host note identity, routing and final host NoteEnd. The UVI Slot adapts accepted core `In` events before Kontakt articulation (`src/plugin.rs:3350`; `src/plugin/uvi.rs:328`). Its fixed-capacity ledger maps canonical `HostNote` ownership to activation-local `HostRoot { epoch, generation, token }`; this token is not another MIDI registry. Native Lua's opaque VoiceId remains a separate identity. Ordered `HostedInput::On`, `Off` and non-note `Event` share one bounded stream, preserving native traversal order.

Native event projection is narrower than the core's precision-preserving event vocabulary. Unsupported expression/MPE/choke/tuning cases diagnose or abort according to the Slot policy; this integration does not claim every host event has an equivalent native implementation. Protocol decoding and reset policy must not be reimplemented inside UVI merely to fill those gaps.

Worker completion transfer is durable: a successful bounded queue push precedes Session ledger acknowledgment; queue-full completion suffixes remain retained for retry (`src/uvi/worker.rs:1021`). Rendered/prefetched PCM is not audible consumption. Slot polls completions using the activation-relative **whole host callback start**, and Bridge subtracts L before advancing the exclusive consumption fence (`src/plugin/uvi.rs:560`; `src/uvi/bridge.rs:421`). PCM underrun/discard alone cannot open that fence.

`finish_host_notes` aggregates active and retiring native owners, Kontakt voices/jobs and alignment ownership (`src/plugin.rs:3418`). Kontakt PCM delayed by the shared SourceDelay has an additional fixed-capacity, conservative terminal-readiness fence (`src/plugin/uvi_delay.rs:168`). It retains the first eligible observation until its delay expires, invalidates readiness when ownership becomes pending again and retains rows under host End backpressure. Native completions already carry their latency fence and receive this extra fence only when a Kontakt owner also exists. Rows retire only after the core's aggregate End is accepted; abort is explicit lifecycle cancellation, not a fabricated normal completion.

Replacing/resetting native audio aborts its activation but retains its canonical owners in a preallocated retiring pool (`src/plugin.rs:4129`). Once End ownership finishes, the endpoint moves through the existing discard queue (`4582`). `Audio::drop` first destroys Slot/AudioPort, then publishes its atomic retirement receipt with Release (`src/plugin/uvi_control.rs:79`). Loader Acquire receipt checks permit controller stop/join/removal (`307`). Canceling an exported request suppresses future publication/UI without killing the controller ahead of the audio receipt.

Teardown also has structural ownership: Dsp's registry Arc lease is declared after every native endpoint/retiring pool (`src/plugin.rs:3560`); Shared's endpoint queues precede its registry lease (`524–542`). Thus queued/installed endpoints disappear before the final controller owner. This is not callback mutex use: Dsp retains the lifetime lease, while normal processing moves prepared ownership through bounded queues. Keep these ordering invariants when extracting the common player owner.

## Implemented instrument UI and state boundary

`src/plugin/uvi_ui.rs:15` owns a loader-side mailbox. It discovers native ScriptProcessors, requests one snapshot at a time fairly, checks request/processor/activation stamps and publishes immutable snapshot/artwork Arcs (`34`). Native artwork authority/decoding stays in `uvi::ui_assets`; neither the editor nor callback runs Lua or decodes images.

Each PartView receives only matching epoch/generation UI state (`src/plugin/uvi_load.rs:48`). `src/ui/uvi_instrument.rs:205` projects available native widgets and artwork through the existing editor infrastructure; `138` sends stamped native edits. Shared ingress rejects superseded/failed activations, and audio forwards bounded edits at the native event clock (`src/plugin.rs:1505`, `4290`). This is a basic native instrument panel, not a promise of complete vendor UI rendering. Bounded native state captures persistent widgets, authored onSave data, original-node parameter deltas and approved resource paths; arbitrary VM stacks, voices and transport are excluded. Core owns rack/browser/host parameter identity; backend owns native widgets, script parameter semantics and native state versioning.

## Authoritative ownership and shared sampler facilities

Ownership means one authoritative state/policy owner, not duplicating sampler machinery behind format folders.

| Responsibility | Authoritative owner | Current coordination boundary |
| --- | --- | --- |
| Host ingress precision, protocol/reset, physical/delivered addresses | Core MIDI/protocol | Align with concurrent MIDI contract; do not add a native RPN/NRPN registry. |
| Exact host roots, routing, final End and activation lifecycle | Core player/routing | Native adapts host ownership and reports descendant completion. |
| Rack controls, mixing, alignment, latency and host outputs | Shared core audio | Existing Rack source hook and SourceDelay; native signal order remains backend-owned. |
| Catalog identity, preparation admission and endpoint retirement | Core loader with backend control owner | Typed source/currentness checks, bounded handoffs and receipt-ordered Registry lifetime. |
| Container/decryption, program interpretation, scripting and native parameter semantics | Backend | Kontakt import/KSP; UVI UFS/crypto/program/script/host. Private authority stays off audio. |
| Native DSP/UI adaptation | Backend using shared facilities | Preserve native units/clocks/order, while reusing equivalent kernels and editor infrastructure. |
| PCM, convolution, compatible delay/routing kernels and resource infrastructure | Shared resource/DSP owner | Backend supplies native decoding, channel/loop semantics and coefficient preparation. |

Already shared: `audio::Pcm` (`src/audio.rs:56`) and UVI's exact multichannel adapter (`src/uvi/storage.rs:26`); `fx::convolution::Convolver` (`src/uvi/effects.rs:23`); common Rack routing and the prepared SourceDelay above. UVI sample/IR lookup shares immutable resources, with native channel routing/reconstruction retained in its adapter. Kontakt streaming remains coupled to Bank/voice mappings; UVI currently holds resident packed assets. There is no implemented UFS streaming path to advertise.

Resampling, envelopes, filters and modulation can share numerically equivalent kernels, not conflicting native laws. UVI's measured impulse FIR/endpoint policy, OnePole law and delay units, native envelope clocks/lookahead, control smoothing, bypass/reset and nonlinear order stay explicit until equivalence is proven. Native controller forwarding, per-processor observations, audible applied state and routed sustain are backend semantics; receipt of the same CC is not proof that Kontakt's state/default/reset law can substitute. Numerical/native compatibility findings belong in the separately owned UVI compatibility report, not a universal controller contract.

## Proposed narrow common Rust contract

The concrete Kontakt-plus-UVI path above supplies evidence for a later common boundary. It does not yet implement a generic optional-backend registry or a universal player trait. Agree the extraction with the user's other core run before editing its MIDI/engine/player ownership. No full folder rename, worker requirement or generic DSP framework is prescribed.

| Operation | Thread/context | Required contract |
| --- | --- | --- |
| Prepare typed source | Loader/control; actual rate, admitted block/output limits, activation identity and currentness | Fallible owned endpoint plus controller/resources/capabilities. Decode, script init, buffers and coefficient plans finish off callback; failure preserves installed audio. |
| Apply native control / publish state | Native execution/control owner; validated address/value and revision | Bounded command/snapshot exchange, explicit native clock, versioned state and resource references off audio. Kontakt may remain immediate; UVI remains worker-owned. |
| Process borrowed events into caller-owned buses | Audio; canonical core identity, timestamps/transport, valid frame spans and preallocated completion sink | Fixed capacities, exact output span/latency and explicit overflow/underrun status; no allocation, locks, I/O or heavy destruction. |
| Install / retire | Audio swaps prepared current endpoint; loader destroys acknowledged retired owner | Durable bounded handoff; full storage defers adoption. Controller outlives port until receipt, and core retains note owners through accepted aggregate End. |

These are required operations, not new names/types to impose. Preserve numeric precision and provenance through the core until the native projection boundary. Clock context must include sample rate, activation/frame origin, span and transport validity without replacing native control clocks. Capabilities describe only actual supported events, output layout, state operations, capacities and scheduling. Algorithmic/buffering latency is explicit; Kontakt is not forced through UVI's 256-frame worker transport. Shared DSP/resource code stays enabled wherever an enabled backend uses it.

The concurrent `CORE_MIDI_REFACTOR.md` and `MIDI_CONTROL_CONTRACT.md` assign protocol selection/reset and bounded per-physical-channel RPN/NRPN decoding to `midi.rs`, applied MIDI state to `engine/midi_state.rs`, player scratch/lifetime to `engine/player.rs` and canonical identities to `host_notes.rs`. Reuse those ownership decisions after coordination; the dirty original checkout is not merged into this isolated integration.

## Compile-time isolation and shared-file coordination

The UVI modules/helper wiring are feature-gated (`src/lib.rs:17`; `src/plugin.rs` module declarations). `Cargo.toml:26` makes native XML/Lua/PNG and FLAC capability optional. The central catalog scan is feature-gated (`src/library.rs:1270`); disabled UVI is neither scanned nor advertised, while generic persisted source identities remain representable for project round trips. The authored disabled-scanner check is at `src/library.rs:1342`.

The current candidate passes `cargo check --locked --all-targets` with UVI disabled. Its locked/offline normal dependency tree excludes mlua/mlua-sys/roxmltree/symphonia-codec-flac. Kontakt's ni-file, ncw and fastlz remain unconditional (`Cargo.toml:33–43`): complete Kontakt disablement is still a gap. Shared sha2/crc32fast/base64 users are not automatically backend-only dependencies.

Complete isolation belongs at the manifest and central format registry/catalog/player dispatch boundary: make Kontakt-only dependencies optional with their entry points, omit disabled scanners/registrations there, preserve shared facilities, and check actual Kontakt-only/UVI-only library/plugin dependency trees. Do not scatter backend-format branches across DSP loops or claim `--no-default-features` already disables Kontakt.

High-collision files are Cargo.toml/Cargo.lock, lib.rs, plugin.rs, library.rs, articulate.rs, routing.rs, timing.rs and engine ownership files. The new plugin native helpers and `src/uvi` have explicit leaf owners; `engine/source_delay.rs` is shared, not Falcon-exclusive. A future Falcon directory should own native adaptation while reusing shared PCM/convolution/MIDI/rack facilities. Changes to the concurrent core run require coordination rather than overwriting its dirty work.

## Rust API versus external ABI and validation

This is internal Rust dispatch: crates, borrows, slices, enums and error types evolve together. CLAP/VST3 host ABIs do not make a backend Rust API binary-stable. No stable external backend ABI is implemented. If independently distributed backend libraries become a requirement, design a versioned C-facing table with version/struct-size, capabilities, admitted limits, fixed-width scalars, pointer/length spans, opaque handles, numeric statuses and explicit allocation/free/thread/real-time ownership. Do not export Vec, String, Arc, trait objects, Result, Lua values or unwinding through it.

The official [Rust ABI reference](https://doc.rust-lang.org/reference/items/external-blocks.html#abi) gives Rust ABI no stability guarantee. [Representation rules](https://doc.rust-lang.org/reference/type-layout.html#representations) do not turn nested Rust fields into C fields merely through `repr(C)`. [Unwinding rules](https://doc.rust-lang.org/reference/items/functions.html#unwinding) require an explicit boundary policy. A future ABI needs those guarantees by design, independently of today's Rust ownership.

The authored two-restored-part adoption/keyboard/mixer/UI scenario is at `src/plugin/uvi_integration_tests.rs:122`. It passes through the production loader/callback, checks exact native PCM after shared gain/pan, native Lua control changes and snapshots, keyboard release, reset to a new rate/block context, stale UI rejection, removal and latency retirement. Missing host timing retains the adapter's last valid timing instead of falsely failing playback. The callback allocation counter remains zero throughout this scenario. A separate authored test captures the complete editor and checks native controls, popup menus, keyboard, mixer edits and terminal failure presentation.

Validation on 2026-10-03 at `510446d`: `cargo test --locked --profile ci --features uvi -- --skip ui::tests::screenshot --test-threads=4` passes, including 888 library tests (31 ignored and one unrelated screenshot filtered), 89 CLI tests (four ignored), and both two-test integration targets. The subsequent delay-admission change passes both authored plugin integration tests and UVI-enabled/disabled all-target checks. Focused tests also cover packet stitching/offline equivalence, exact rooted event/End ownership, receipt-ordered controllers and prepared delay/direct outputs. Passing allocation tests does not establish a callback-wide deadline proof. Paid renderer throughput and real host playback/UI validation remain required; this document neither declares complete Falcon behavior nor a shipping runtime.

## Native state follow-up

`Part.uvi_state` retains opaque versioned native bytes even when the backend is disabled. `NativeState` shares its allocation across Selection/View clones and delegates to the existing byte-vector JSON/host codec; Debug exposes its length only. Backend state contains no container access keys. `uvi::state` validates the exact Program fingerprint, complete processor set, typed source-node overrides and approved resource targets before constructing a replacement. Measured absent-XML defaults are limited to Program/Layer/Keygroup Gain1/Pan0 and SamplePlayer Gain1; a missing SamplePlayer Pan is not invented, and a complete property inventory remains unfinished.

The loader/controller owns capture requests and replies. Audio publishes only an atomic minimum processed-packet frame. Commit checks source, activation epoch, worker and core generations, plus the currently saved baseline; captured bytes and baseline change together so a regular capture cannot cause a reload loop. Ordinary loader polling only drains explicit save replies and never invokes authored onSave. Native widget edits remain on the audio timeline.

Explicit rack Save waits at most 500ms for coherent native state, then reports success or retains the prior bytes with an error. The host Params pre-save hook cannot propagate an error to CLAP/VST3 serialization; it retains prior bytes and logs a failed capture. This is a documented limitation, especially for an unsealed partial audio packet in a stopped host. Source changes clear state. Restoration preloads saved resources/parameters before native constructor/onLoad/changed/onInit precedence; unsaved programs preserve Session initialization before Renderer construction for dynamic onInit loads.

### Truthful native diagnostics

UVI graph reports belong to `src/uvi/diagnostics.rs`; the worker owns lifecycle,
frame and renderer statistics. Root rack code only exports controller reports
and records configuration/adoption failures through the existing shared journal.
No graph/Lua inspection, serialization, formatting or diagnostic mutex is added
to the host audio callback. Graph admission and completed packets are different
evidence; individual node execution and vendor fidelity are not inferred.
`active_voices` keeps its cleanup contract. `last_completed_voices` is historical
and survives failure/stop, explicitly separate from current audibility.

Explicit runtime evidence uses a coalescing worker request/reply lane. The renderer
owns preallocated processing counters; the player maps them to parsed identities
only on request. Cached immutable reports carry their original activation and
processed frame. Containers and scripts without probes remain unknown. This lane
does not invoke onSave or change the render horizon, and does not infer audibility
or numerical Falcon fidelity. Browser grouping is shared catalog presentation;
UVI access defaults to the private app-config store with an environment override.
