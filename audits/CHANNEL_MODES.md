# Channel modes: physical receiver and library delivery

This audit closes the CC122 callback omission and records the existing CC123–127
release contract. It does not claim keyboard Mono/Omni support or native audio
parity for channel-mode execution. See [MONO_LEGATO.md](MONO_LEGATO.md) for the
separate Kontakt release Note Mono and Source Legato boundaries.

## Primary references

The [MIDI Association CC table](https://midi.org/midi-1-0-control-change-messages)
assigns CC122 Local Control, CC123 All Notes Off, CC124/125 Omni Off/On,
CC126 Mono On and CC127 Poly On. CC124–127 also carry All Notes Off.
CC126's data byte selects the channel count; zero uses the receiver's voices.

The [MIDI 1.0 Detailed Specification](https://midi.org/midi-1-0-detailed-specification),
version 4.2.1, printed pp20–26 and A4–A5, separates Basic Channel, voice
assignment and keyboard Local Control. Basic Channel is separately configured;
channel-mode messages cannot change it. Recognized All Notes Off enters release
and respects sustain. Local Off disconnects an internal keyboard from its sound
generator while MIDI input continues. Receiver implementation charts identify
which modes are supported; the specification does not define overlapping-note
priority in Mono mode. Downloaded official PDF SHA256:
`93cbd078d9f57cd5b20349bbd08fd877f057a74e61cf923a7cec5f1b65bc80c4`.

The [Kontakt Lua instrument options](https://docs.native-instruments.com/ni-tech-manuals/kontakt-api-reference-manual/en/instrument#get_instrument_options)
list `use_cc_120_123` separately from `use_cc_7_10` and `cc_64_mode`.
The [KSP controller callback](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/callbacks#on-controller)
documents receipt of MIDI CC, pitch bend and channel pressure, with CC0–127
identified as controllers. CC122 therefore needs ordinary controller delivery.
Falcon's distinct Layer Mono/Glide and Script Processor Legato policies are
documented in [MONO_LEGATO.md](MONO_LEGATO.md); they provide no Kontakt saved
keyboard-mode identity.

## Pinned Kontakt evidence

The read-only evidence uses Kontakt8.13.1 at image base `0x140000000`, SHA256
`0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`.
Bounded `af; pdg` at established controller entry `0x14086bf00` shows:

| Address | Observation |
| --- | --- |
| `0x14086c08e` | Raw controller index masked to seven bits; value saved into engine `+0x17314` before script delivery. |
| `0x14086c327..0x14086c35d` | Common script-slot loop invokes `0x140798190` with callback kind3. No CC122 exclusion occurs in this ingress function. |
| `0x14086c3f1` | Surviving event reaches controller update `0x140cee5b0` after callbacks. |

This establishes generic ingress order, not an exhaustive upstream dispatch or
native PCM oracle. Existing [controller evidence](../docs/CORE_NATIVE_CONTROLLER_EVIDENCE.md)
pins `use_cc_120_123` at `+0x1fb26` and its system-generator test at
`0x1408783fe`. There is no evidence here extending that saved switch to
CC124–127 or identifying a saved Omni/Mono keyboard policy.

## Rust ownership and change

`MidiState` stores accepted physical controller values, expression and MPE
configuration. The library receiver separately captures delivered channel/CC
receipt before script acceptance. `HostNote` retains original physical input
and exact identity independently of delivered library channel and key.

`Engine::control_from_received_scoped` already handles CC123–127 at original
ingress using the physical input plus MPE manager members. It releases exact
roots, and the anonymous path releases only captured input keys. Sustain and
sostenuto defer audible release. `InstrumentState::native_note_action` gates
only CC120/123 with the saved option. Existing CC124–127 release behavior stays
independent of that option.

The shared physical-stop helper also rebuilds delivered key indicators after
CC120 or MPE role reconciliation. Unscripted exact roots live in `HostNotes`,
not the anonymous `InputKey` table. The old reconstruction omitted a surviving
exact root from an unrelated physical input on the same delivered channel.
It could clear `Engine::key_down` despite that root remaining held and audible.
The helper now includes existing held, unsilenced exact owners alongside its
anonymous/Runtime lookup. It does not change selected-root cleanup, fade times,
controller acceptance, or the note-mode fallback roster.

The scripted ingress condition previously excluded CC122 along with handled
stop/reset controllers, losing its callback. Including `cc == 122` in ordinary
controller delivery fixes that omission. Ignored callbacks keep raw library
receipt distinct from accepted physical state; waiting callbacks retain their
original event and delivery context.

No new `MidiState` mode flag is added. KONTRA currently has no protocol Basic
Channel mode state, Omni admission switching, CC126 voice-per-channel keyboard
assignment, or internal keyboard/MIDI-Out path for Local Control to disconnect.
Keeping a numeric CC value is not implementation of those configurations.
The existing CC123–127 release subset is a local receiver contract, not a claim
that all four MIDI receiver modes or Kontakt keyboard priorities are supported.

## Staged verification

`src/engine/channel_mode_tests.rs` records every output sample and exact note
completion time. CC123–127 uses independently scheduled Note Off as its oracle,
with CC64/66, physical MPE manager0/member1/member2, delivered homes7/9, an
unrelated input5 sharing home7, exact/anonymous notes, script waits, saved option
enabled/disabled, shared lanes, fragmented renders and empty renders.

CC122 changes the captured left note by −6dB after a 1 ms callback wait. Its
external oracle schedules an exact host gain expression at the known frame;
the right note remains unchanged. A home7→9 change during the wait checks
captured authority, and forward/ignore cases check raw versus accepted caches.
Both cases assert complete stereo PCM, finite samples, final silence, exact
completion, no dropped commands and no callback heap allocation.

These tests are staged only. No Cargo, build, test or performance scenario was
run by this agent; central validation owns execution after integration.

The subsequent exact-root held-key regression in
`src/engine/held_key_tests.rs` passed central execution (0.03s). It compares
complete PCM with an independently targeted choke, then uses the surviving
public key latch for an audible host accent. It covers physical4/5 sharing
home7, CC120 and MPE role reconciliation, exact overlays, sustain, fragmented
rendering, callback heap checks and distinct physical/attached-audio end times.
