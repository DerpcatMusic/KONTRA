# UVI Linux test checkpoint

This is a private experimental checkpoint, not complete Falcon compatibility.
The optional UVI player shares KONTRA's browser, rack mixer, routing and keyboard.
Unsupported native graphs remain explicit failures. Starter coverage is still
1/50; the paid-library audit and native DSP comparisons have separate limits in
[uvi-compatibility.md](uvi-compatibility.md).

## Start and load

Build the CLI, plugin library and standalone with the `uvi,standalone` features.
The test package identifies its exact source and enabled features in its manifest.
It contains no vendor executable, bank, sample, script or access state.

Configure the local official Workstation reader in library settings or with
`KONTRA_UVI_READER`. Bounded installed-reader discovery is a fallback. The
decoder reads the executable; it does not run or activate Falcon/Workstation.
For protected banks, the normal desktop/DAW access store is
`$XDG_CONFIG_HOME/kontra/uvi-access` (usually `~/.config/kontra/uvi-access`).
`KONTRA_UVI_AUTHORITY_DIR` overrides that directory. Private bank-bound JSON
files are named by the bank UUID (32 lowercase hexadecimal characters plus `.json`).
Selecting a supported encrypted bank automatically prepares this private record
after verifying a complete bank PNG. Records bind the bank UUID, name and size;
changed banks are verified again, and legacy records are verified and upgraded.
Unsupported protection, missing suitable resources and corrupt/insecure private
records remain explicit errors. No manual key command is needed in the desktop
or plugins. These local records are separate from saved instrument controls.
The reader and bank files must remain present. The CLI setup commands are in the
[README](../README.md#experimental-uvi-interoperability).

Add the folder containing the banks in KONTRA's library settings and rescan.
Later launches publish the saved catalog immediately and verify it in the
background. This caches library/preset metadata, snapshot bindings and source
identities; samples are still loaded for the selected instrument. Rescan forces
rediscovery. Cached publication cannot restart an already staged instrument.
The browser saves its **Unified / By player** choice. Unified keeps one catalog;
By player gives Kontakt and UVI / Falcon separate headings with both available.
Library and preset rows identify their player; favorites, recent items and search
retain native bank identities. Existing settings default to Unified.
Select a supported UVI
program and wait for its controls before playing. Actual sample rate, host block
size and preparation limits can affect admission.

## Check the behavior

1. Play from the keyboard, then from the DAW. Check release, overlap and sustain.
2. Resize the editor; artwork and hit targets should follow one uniform scale.
3. Change a native control and verify the sound follows it.
4. Change rack gain, pan, mute and solo alongside another part.
5. Save a KONTRA multi, reopen it, and compare controls and sound. Test DAW save
   and reopen separately; the host pre-save API cannot report a failed capture.
6. Remove the part or reset the audio configuration and check that a replacement
   loads without old controls acting on the new instrument.

Save requires already applied control edits and a processed audio boundary.
If rack Save says it is waiting, resume audio processing and save again. A failed
native onSave retains the previous successful payload. DAW capture failures are
reported in Logs and retain prior state; they must not be read as fresh saves.
Ordinary playback/UI polling never runs onSave. Voice state and transport are
not part of instrument persistence.

Report the exact build identity, bank/program, sample rate and host block size
with any failure. Private logs or screenshots can contain purchased-library
names and artwork. Render-deadline reliability, complete native fonts/units,
advanced displays and full Falcon sound parity remain unproven.

## Read a diagnosis

`kontakto uvi-diagnose bank.ufs member.uvip --reader UVIWorkstationx64.exe
--content-key-file private-state.json` emits JSON after attempting initialization.
Use `--sample-rate` for the actual rate. Failed initialization still emits the
available report on stdout and exits with an error. This command sends no notes:
Ready means initialized, not that the musical behavior has been exercised.

The report separates decoded node identity and preflight rejection reasons from
worker evidence. Preflight admission is conditional on later resource/DSP/script
validation; even a bypassed unknown processor is rejected because a script could
enable it. Packet counters and retained voice instances describe this renderer,
not Falcon equivalence or audible voices. A zero error count with zero rendered
packets proves no playback. Explicit diagnoses request a stamped worker snapshot
at a processed boundary. Instrumented oscillators and inserts report processed block counts, current
bypass state and retained oscillator voice instances. Silent processing still
counts; containers, control-source nodes and scripts without probes remain unknown. A cached snapshot keeps its original frame when playback advances.
These counters do not prove audibility, script callback coverage or vendor fidelity.
Support exports keep a bounded UVI context and label omitted node rows with exact
included/omitted coverage. CLI diagnoses and each worker cache retain full rows.
No full-graph snapshot runs during ordinary packet or UI polling.

The editor's existing Logs and diagnostic export include UVI initialization
stages and failures. Rack worker snapshots retain current status/counters; worker
initialization durations remain available while a load is running. The rack
shows the current loading activity and elapsed seconds, and terminal failures
remain failures. Host latency admission survives engine resets while the native
source remains selected, preventing a zero/loaded-latency restart cycle; physical
compensation is retained only with matching already allocated delay storage.
Actual Bitwig behavior still requires testing with the newly installed plugin.
Worker load journals retain decoded graph details even when admission fails. Lua errors
retain processor/chunk/line and resumed frame when available; DSP errors identify
processor/node/frame at the instrumented boundaries. Missing source context is not
invented. Authored print counts and drops are measured separately from errors.
The JSON/journal contain names and failure messages, not full commercial scripts,
program XML, samples, reader constants or access keys. Review private names before
sharing. Journals and exports have their existing retention and truncation limits.

## What is actually established

| Area | Established evidence | Remaining work |
| --- | --- | --- |
| UFS/program/sample access | Owned-corpus directory and decode checks; native identities retained | Unsupported protection/layouts remain failures; no universal-bank claim |
| Lua and MIDI | Authored initialization, measured original-node Gain/Pan defaults, timed commands, control edits, transport and rooted note tests | Complete host API, MPE/tuning/expression and every library interaction |
| Native DSP | Scoped native comparisons and explicitly bounded admitted settings | Unsupported oscillators/routes/modes and broader live/rate fidelity |
| VWinds controls | Original-source panel with uniform intrinsic 720×480 scaling; 273 widgets/38 images retained | Native fonts/units, advanced displays and full interaction/audio comparison |
| Rack persistence | Real two-part save/reopen control and exact PCM regression | Host save failures cannot be returned through the framework hook |
| Playback scheduling | Allocation-free callback transport and bounded packet ownership tests | Sustainable deadlines on user hardware; prior paid-worker misses remain |
| Compatibility accounting | Parsed/admitted/rejected graph, worker lifecycle and explicitly requested instrumented node counters | Uninstrumented nodes, script callback coverage and native end-to-end musical parity |

VWinds is a hybrid sample/modelling product. Its publisher describes harmonic
alignment, airflow/vibrato and recorded or modelled transitions in the
[VWinds manual](https://www.acousticsamples.net/index.php?product_id=112&route=product/productmanual).
The installed program must be examined individually: implementing a generic
physical model does not establish its compatibility, and decoding its full graph
does not prove its transition or timbre behavior.

## Checkpoint validation

The combined `uvi,standalone` optimized CI test run passed 959 library checks,
89 CLI/playback checks and four additional binary/integration checks. Thirty-five
external-fixture checks were ignored and one screenshot check was excluded.
The fresh native tests include root Choke ownership, state construction and
resource precedence, no periodic onSave, real two-part rack save/reopen exact PCM,
UI scaling bounds, graph admission reporting and DSP/Lua error attribution.
These checks are scoped regressions, not a count of compatible Falcon features.

The loading checkpoint also covers automatic private access preparation, catalog
cache integrity/invalidation, stable host latency across resets and current load
activity. The 512-byte cipher block loop measured 2.14–2.40 times faster in paired
optimized synthetic checks without new dependencies, nightly Rust or threads.
Indexed graph lookup and removal of repeated finite checks on immutable validated
PCM retain exact paid/audio/event/state hashes in focused comparisons. A real
saved-root scan found 11 Kontakt libraries, 835 preset files, 1003 snapshot
bindings and 40 programs across 25 UVI banks. The full scan took 31.1 seconds;
a fresh instance published its saved index in 11.6 ms and completed background
verification/artwork in 188 ms. Settings remained byte-identical. This measures
metadata publication, not complete UI startup or sample loading time.
