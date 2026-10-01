# Output routing: Kontakt's batch operations and KONTRA's answers

In Kontakt, output routing lives in the Outputs section (F2), and host ports only
exist once the user has built them. KONTRA has a fixed set of 16 stereo host ports,
"st.1" to "st.16". It routes parts to those ports automatically and names each port
after whatever plays through it. The same bus model drives the plugin and the
standalone.

Code: `src/routing.rs` (policy and names), `src/plugin.rs` (`route`,
`Shared::reroute`, `outs_of`), `src/fx/processor.rs` (output channels), `src/ui/menu.rs`
(`Target::Routing`), `src/ui/mixer.rs` (the "Outputs" choice). Host port names come
from the vendored MOOSE patch (`vendor/MOOSE-PATCHES.md`).

## Kontakt's operations

| Kontakt | What it does there | KONTRA |
|---|---|---|
| Outputs > Batch configuration > "one individual channel for each loaded instrument" | Clears the output section and builds a stereo channel for each instrument, then the host has to rescan | **Automatic**: the mixer's Outputs mode "One per instrument". Each part keeps its port across reloads, and a new part takes the lowest free port. **One click**: Routing menu > "Give every instrument its own output" (also clears hand-picked routes). |
| Outputs > Batch configuration > reset to a single stereo channel | Returns everything to st.1 | **One click**: set the Outputs mode to "Stereo mix", or Routing menu > "Reset routing" (clears hand-picked routes and mic ports; in Stereo mix it also moves every part back to st.1). |
| Add / delete output channels, "Delete unused" | Manages the list of channels the host sees | **Not needed**: the 16 ports always exist, and unused ports stay "st.N". The mixer only shows buses that are in use or named. |
| Rename an output channel | Types a name into the channel's header | **Automatic**: a port is named after what plays through it ("Harp", "3 Cellos", "Harp Close"). A port shared by several parts gets their common leading words, otherwise "Violins +2". **One click**: "Name outputs after instruments" freezes those names. Renaming a bus strip by hand still wins. |
| Instrument header output dropdown | Moves one instrument to a channel | Same dropdown, plus an "Automatic" entry. Picking a port or dragging a part onto a bus strip marks the route as hand-picked, and automatic routing leaves it alone. |
| Mic mixer routed to "Out N" (library script, `$ENGINE_PAR_OUTPUT_CHANNEL`) | The user creates enough output channels and maps each mic to a separate host output | **Automatic**: in the "One per mic" mode, each output channel the library's script uses gets its own free port, named "<instrument> <mic>" after the library's bus or group. In the other modes those mics play through the part's own port. |
| Instrument header MIDI channel (and the loading option to assign channels automatically) | Set per instrument, or for new instruments as they load | **One click**: Routing menu > "Give every instrument its own MIDI channel" assigns A1, A2 and so on in rack order. "All Omni" undoes it. |
| Aux channels (4 aux sends per instrument) | Sends into aux channels that have their own output | Unchanged: each part has one send to any bus. Automatic routing never uses a send's target bus for another part. |
| Master (volume, tune, reference tone) | Global | Unchanged. Master processing applies to what reaches the host ports. |
| Save output section as default | Stores the layout for new sessions | **Not needed**: the Outputs mode and hand-picked routes are saved with the session and the multi. |

## Host behaviour

- **CLAP:** after a change settles (500 ms without further changes), the plugin
  calls `clap_host_audio_ports.rescan(CLAP_AUDIO_PORTS_RESCAN_NAMES)` from the main
  thread. The request is triggered from `process`.
- **VST3:** the plugin calls `restartComponent(kIoTitlesChanged)` on the shim's
  next main-thread flush. `getBusInfo` then returns the new titles.
- **Hosts that ignore these calls** keep showing the old names until they list
  the ports again (for example after a project reload), when they get the current
  names. Audio routing never depends on the port names.
- **Not verified:** how these calls behave in Bitwig, Reaper or Cubase. Nothing
  public documents it.

## Limits

- Only 16 ports exist. If more parts or mics need a port, a part without a free
  port keeps the port it already has, and a mic without one plays through its part.
- An output channel is a direct out. As in Kontakt, it skips the instrument's
  insert and main FX, the instrument volume and the part's send, but still follows
  the part's fader and pan.
- Setting `$ENGINE_PAR_OUTPUT_CHANNEL` for a whole instrument (group -1 with no
  bus) is not modelled. Groups and instrument buses are modelled.
- The library's own dropdowns still show "Out 1" to "Out 8".
- A mic cannot be routed by hand; it follows the Outputs mode.
