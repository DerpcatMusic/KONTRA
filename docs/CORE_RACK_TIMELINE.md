# Rack sample timeline and KSP voice counts

Rack advances every instrument through the same DSP intervals. At each boundary,
all preceding instrument DSP is complete. Each instrument then runs due callbacks,
applies due parameter writes, and admits due note/control commands, in slot order.
The next boundary is the earliest instrument timer, command, write, original FX
block end, or retained native host-span end. Future callbacks run after their
preceding DSP; callbacks exactly at the outer render endpoint remain deferred.

An Engine outer render still owns one Runtime begin/finish, instruction-budget
lifetime, level-meter interval, and native host span. Its small private render
cursor retains command/write positions and original FX block offsets. Rack uses
existing scratch and bus buffers; it creates no callback vectors or note snapshots.
Group-send buffers reset only at their original per-instrument FX block boundary.
Main and direct-output intervals mix before a subsequent FX block reuses storage.
Muted instruments continue their DSP and lifecycle processing.

`$PLAYED_VOICES_INST` reads the current Player sample-voice roster. In a scoped
Rack invocation, `$PLAYED_VOICES_TOTAL` combines that live roster with the immutable
peer census for that invocation. Ordered phases update the census after each
instrument; queued notes and KSP event metadata do not substitute for sample voices.
The source is the actual Engine admission/retirement state at the common DSP frame.
The [NI KSP manual](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/built-in-variables-and-constants)
defines the current/all-instrument distinction; it does not establish native
same-frame callback ordering or layer-count parity. This implementation preserves
the existing core callback → writes → commands policy and does not claim those
unclosed native details or shared AHDSR batch ordering.

Rack, routed MIDI, delayed articulation, and plugin handoff/UI/async callbacks use
the crate-private `Rack::with_part`. Direct Engine and direct mutable `Rack.parts`
calls have no peer owner: TOTAL remains explicitly unavailable there, as in worker
initialization. Standalone `Engine::render` also clears any temporary census while
it independently advances DSP; only the aligned Rack phases retain peer counts.

Rack retains an absolute frame owner. Growth initializes only newly prepared
instruments at that frame; existing instruments, voices, waits, and source clocks
are never shifted. Existing reset and bank/script replacement preserve their
Engine frame. The plugin already converted each Engine's local NOTE_END to its
own host-block origin before this change; clock divergence alone was not proof of
an incorrect previously emitted plugin NOTE_END.

Nonfinite FX recovery still silences the failed interval and clears FX state.
Previously mixed direct-output intervals remain committed; a later failed interval
cannot retroactively erase them by clearing reused effect buffers.

The `src/engine/rack_timeline_tests.rs` PCM proof covers common boundaries,
peer voice counts, original FX offsets, and fresh-part timing against independently
chronological standalone-engine playback. It includes twenty notes reduced to five,
physical pedals, exact-owner expression, routed group sends/direct outputs, distinct
FX blocks, and growth at frame333. The focused twenty-to-five Rack test passed
in the central test runner (0.03s), including complete PCM, meters, exact owner
ends, and fixed/irregular partitions. The prepared 54-tap FIR becomes mathematically
silent at 1093; the measured FFT overlap residue peaked at 4.1909518e-10 at 1093
in both partitions, followed by exact zero from 1280 through 1408. This result
does not report execution of the separate growth test or a broader suite.
