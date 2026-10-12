# Player ABI playback evidence

A completed private Linux x86_64 proof runs a Lua 5.4 host and the existing UVI Lua 5.1 Worker in one process across a small C function table. This advances the earlier VM-lifetime experiment to actual audio and host-root completion delivery. The production neutral backend boundary remains unimplemented.

The proof uses original authored programs, the unmodified UVI source snapshot at `3c8e5ae`, and cached root dependencies built at `bb60218`. It is separate from the shared host checkout. No library content, activation material, or vendor source is included in this note.

## Completed measurements

Both `RTLD_LOCAL` and `RTLD_GLOBAL` passed. Across the two modes, twelve playable instance cycles produced 9,216 stereo PCM frames and twenty-four tagged root completions. Every saved sample is finite; peak amplitude is 0.125. The saved PCM is identical between loader modes. Each mode completed three physical module unload/reload rounds while the same host Lua 5.4 VM remained live.

The checks cover stamped note/choke/release playback, invalid event and activation identities, frame sequencing, completion eligibility, queue-full close, initialization and runtime callback errors, missing-bank failure, and healthy reopening. Module-owned PCM remains readable after instance close and must be released through its producing module. Outstanding instances or buffers prevent explicit resource quiescence. Repeated close/release through zeroed slots is rejected.

The private table has revision/size negotiation, scalar IDs/statuses, opaque instance handles, fixed wire structs, and producer-released byte buffers. Rust structs use `repr(C)`; C layout checks pass for this x86_64 build. Its sole dynamic export is the table entry point. Lua symbols remain local to their respective runtime. No Lua value, Rust enum/error/Vec, or VM-internal object crosses the table.

These measurements establish this build's audio, ownership, error, and lifetime boundary. They do not establish universal ABI stability, other operating systems, arbitrary third-party plugin compatibility, or integration with the shared host's Rust backend facade. See [Lua runtime separation](PLAYER_LUA_RUNTIME_BOUNDARY.md).

## Module lifetime and support toggle

Process-lifetime pinning is the smallest integration policy: retain one loaded backend module and its function table, close its instances when support is disabled, and reuse the module for fresh preparations when support is enabled again. Instance retirement still requires finishing host-root ownership, destroying its exclusive endpoint off audio, and then stopping/joining the controller.

The unload experiment also found a caller-thread lifetime requirement. A long-lived thread that called Rust kept the module loaded after `dlclose`, even after instances, buffers, and diagnostics were closed. Confining those calls to a dedicated host control thread and joining it before `dlclose` allowed all six measured physical unloads. The explicit resource-ready query alone is therefore insufficient for physical unload on this build. Pinning avoids this additional caller-thread retirement requirement.

Worker loading uses the existing global diagnostics Session. Final teardown must stop loaders, release diagnostic owners, and join diagnostics before physical unload. Ordinary support-toggle off should retire instances while retaining module diagnostics until final shutdown. Static inspection confirms diagnostics can recreate its Session after shutdown; reopening after quiesce also occurred within each completed proof round. Concurrent diagnostics restart/shutdown is outside the proof. See [diagnostics lifetime](../src/diagnostics.rs).

## Preserve the shared host's ownership

Existing hosted root tokens and activation stamps must pass unchanged through the boundary. The shared host retains root allocation, routing, MIDI ownership, destination lifetime, and consumed-time/latency mapping. The Worker retains UVI child voices, Lua state, program resources, and completion generation. A second MIDI ledger or router is unnecessary. See [hosted input identity](../src/uvi/script.rs) and [Worker and AudioPort ownership](../src/uvi/worker.rs).

Preparation must preserve the complete existing load gates: requested source and destination, catalog revision, backend epoch, sample rate, maximum host block, target identity/part generation, and worker lead configuration. Recheck the complete key after authority resolution, after preparation, and before adoption. Disable/re-enable must invalidate pending preparation through the existing activation/currentness mechanism. A stale result is retired off audio and cannot replace the live instrument. Adopt the prepared controller once; do not reopen content or start another Worker during endpoint export. See [load preparation and currentness](../src/plugin/uvi_load.rs) and [ready handoff and retirement receipts](../src/plugin/uvi_control.rs).

Production preparation needs a bounded, immutable neutral representation of the already-validated selection and its scoped authority. Borrowed control fields must be copied during preparation; saved state cannot grant content authority. The proof's fixed authored-fixture authority is not that production descriptor.

## Production work still pending

- Separate the loader-owned controller handle from the exclusive AudioPort endpoint. The proof combines them because all calls are serialized on a control thread. Keep the controller alive until endpoint destruction is acknowledged.
- Replace allocating proof marshaling with fixed inline inputs and caller-owned 256-frame stereo PCM storage. Retain producer-owned bytes for control-only errors/state/UI. The existing Worker packet API is fixed and nonblocking; the proof's input conversion and PCM receive allocate and are not audio-callback operations.
- Adapt the existing packet/Bridge scheduling once, preserving frame validation, bounded backpressure, consumed-boundary completion delivery, and mixer-delay acknowledgement. No additional queue, dispatcher, or latency mapper is established by the proof.
- Wire the real shared-host backend selection, scoped preparation, adoption, and retirement. UI/state/artwork and additional event types need explicit neutral calls when integrated.
- Establish real Part/Synth context owners before transporting parent writes. Worker Output currently has no context-command payload. A destination/part-generation binding belongs to the adopted host endpoint; epoch/generation/frame alone cannot select an arbitrary rack owner. Routing changes need ordering before subsequent attacks, and parent gain/pan need verified laws and a single consumer. MPE and host note-expression support remain separate, unverified boundaries.

The pending fixed marshaling and shared-host integration require compilation and focused validation when CPU work is authorized. No additional builds, tests, native probes, or playback were run for this static documentation follow-up.
