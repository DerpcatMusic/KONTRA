# Falcon/UVI native contract and validation boundary

Reviewed 2026-10-12 on `feat/kontakt-falcon-parity` at `71fc3b40`. This is a
read-only contract audit of current v2, frozen v1 `4bffbb18`, UVI's public
manual and UVIScript reference. No Falcon or UVI Workstation instance was
launched and no new native audio, GUI, timing or script capture was made. The
installed UVI Workstation VST3 exists in the local Wine prefix; the available
static inventory identifies version 4.0.9, while it found no Falcon executable
([inventory](../re/DSP_SYSTEM_INVENTORY.md)). UVI's current online reference
does not publish one uniform software-version label for every page; versioned
native observations must therefore record the actual binary and preset identity.

## Source contract, with implementation implications

| UVI-documented behavior | Present implementation and required admission |
| --- | --- |
| Falcon's MIDI path is Multi → Part → Program → Layer → Keygroup; audio returns from each keygroup voice through Layer → Program → Part → Multi. Keygroup effects are per voice; Layer/Program/Part/Multi effects process their scoped sums. Event processors stop at Layer; modulators have a different downward scope. [Falcon structure](https://manual.uvi.net/falcon/en/structure/), [Falcon interface and aux buses](https://manual.uvi.net/falcon/en/interface/) | `lib.rs::translate_full` creates a shared `sampler_ir::Instrument` and maps scripted `(layer, oscillator)` pairs to groups. The known v2 frontier remains Multi/Part execution, full channel routing, several oscillator modes and most effects; `falcon-parity.md` correctly treats working leaf DSP separately from whole-program parity. Keep the XML physical tree and scoped event/audio/control graphs as typed import data before flattening any supported subset into core IR. An unknown module must retain its location, bypass and source identity and block a whole-program parity claim. |
| A ScriptProcessor runs UVIScript, a sandboxed **Lua 5.1** dialect. `require` loads Lua modules; `os`, `io` and C modules are unavailable. Most callbacks run under a preallocated real-time pool, while the main chunk and `onLoad`/`onSave` have relaxed allocation constraints. [Falcon scripting](https://manual.uvi.net/falcon/en/scripting/), [UVIScript language](https://lua.uvi.net/_lua_reference.html) | v2 runs Luau through `mlua` (`script.rs`, `Cargo.toml`). Its new `parity::check_source_syntax` is explicitly a **compiler-only Luau check**, not native Lua 5.1 acceptance or host-API behavior. The older `UVI_LUA.md` statement that the historical 660 scripts compiled means only that their source passed this frontend. A pinned dialect corpus must cover 5.1 constructs, forbidden later syntax, library differences, error behavior and source maps; any Luau compatibility shim must be admitted by outcomes in the native version under test. |
| Missing event callbacks forward unchanged. Defining a callback, including an empty one, makes that callback responsible for forwarding with `postEvent`; `onEvent` supersedes specialized handlers. `onInit` runs after successful initialization; widget construction is confined to the main chunk. On restore, widget values are restored before `onLoad`, before processing starts. [Callbacks](https://lua.uvi.net/group___event_callbacks.html), [Falcon pitch-invert example](https://manual.uvi.net/falcon/en/scripting/), [UI guide](https://lua.uvi.net/_u_i_page.html) | v2 has authored callback and restoration paths; its own `uvi_audit.rs` fixtures exercise forwarding, note identity, state and bypass. **Concrete order gap:** `script.rs::load_scripts` calls `restore_ui_custom` (`onLoad`) before `restore_ui_values` for initial state, although explicit `restore_ui_state` in `script/ui.rs` uses the documented values-first order. These are generated XML/synthetic host checks. Capture native callback order and exact forwarded event fields with an authored script at Program and Layer scope, including a swallowed NoteOn and unhandled NoteOff, then compare the same MIDI trace through v2. |
| Every event callback is an independent cooperative coroutine. `spawn` starts after the current instant; `run` starts immediately. `wait` uses milliseconds and `waitBeat` musical beats with sample-accurate scheduling. `waitForRelease`/`isNoteHeld` belong to the creating note callback and do not inherit into spawned threads. A widget `changed` callback cannot yield. [Threading guide](https://lua.uvi.net/_time_intro.html), [UI guide](https://lua.uvi.net/_u_i_page.html) | `ScriptHost` has coroutine/deferred scheduling. Live `ScriptThread` owns Luau off the audio thread and applies returned commands at a later block (`scripted/thread.rs` says within a block or two); its 1 ms clock poll is a different latency contract from native sample-accurate callbacks. Retain timestamped scheduling in shared core, measure onset error across host block sizes and tempo changes, and report live/inline paths separately. Do not infer a speed or timing parity claim from script compilation or offline stepping. |
| `Element` exposes typed `setParameter`, `getParameter`, `hasParameter`, `getParameterConnections`, `parameterDefinitions` (id, type, default, readOnly, optional bounds), parent/path, name-keyed `children` and ordered `synthChildren`. `getParameterConnections` returns nil when absent. **Unknown parameter or wrong Lua value type passed to `setParameter` is silently ignored by the current reference**, whereas some frozen-v1 synthetic tests expect an error. `output` assignment is asynchronous. Effect insert/move/remove is asynchronous; moving preserves effect identity, settings, modulation and audio tail, while removal expires its reference. [Element](https://lua.uvi.net/class_element.html), [element catalog](https://lua.uvi.net/_elements.html) | v2's `parameters.tsv` and host object proxies supply many names and typed values, but `script.rs` still stores some writes as overlays and `UVI_PARITY_LEDGER.md` marks unbound Part writes/stubbed effects. Admit each `(element type, parameter id/name)` only with correct type/default/readback, actual audio/control binding, update cadence and lifecycle. Reconcile the v1 error-expectation conflict against a version-pinned native script receipt before changing behavior. Preserve stable element handles across reorder, but invalidate removed nodes; plain positional indices cannot serve as permanent identity. |
| `loadSample(oscillator, path, callback?)` loads into a sample oscillator asynchronously and returns an `AsyncTask`; `task.finished` may be polled where yielding is legal. Completion callback runs on the script thread. `setPlaybackOptions` takes sample-frame positions and is also asynchronous. A `SampleMappingOscillator` instead uses `osc:loadMapping(path)` to replace a zone set asynchronously; `loadSample` on it raises. Its loading status and live zone count are distinct. [Async API](https://lua.uvi.net/group___async.html), [async guide](https://lua.uvi.net/_async_intro.html), [MultiSample guide](https://lua.uvi.net/_multi_sample_intro.html) | Current v2 `script.rs` records `loadSample` as unavailable and schedules a nil callback; `parity/ledger.rs` marks it Unsupported. Frozen v1 `src/uvi/host.rs::install_resources` had a stronger resource-task simulation, but its isolated test expects `loadSample(...).success` and callback completion inline; that is not proof of native asynchronous timing or replacement semantics. Add a bounded background resource preparation transaction with task ID, target oscillator identity, generation, success/failure and callback delivery; publish the new immutable sample/mapping state only when prepared. Test overlapping requests, previous-sample retention on failure, loop options, cancellation on program unload and callback/poll equivalence against the native host. |
| UI controls are authored in the main chunk, with parent-relative panels, explicit or grid geometry, typed widget values and `changed` callbacks. Parameter-bound widgets follow the target parameter; scripts can use background images, custom strips, SVG and fonts. One program-level performance UI is selected by `makePerformanceView`. [UI guide](https://lua.uvi.net/_u_i_page.html), [UI functions](https://lua.uvi.net/group___u_i.html) | v2 executes embedded modules and projects widgets into `sampler_ui_ir`; `bank.rs`/`resources.rs` resolve bounded bank or loose-file assets, and `UVI_LUA.md` records unresolved fractional geometry. Module and asset lookup must carry the requesting script's bank member and program origin; fallback and ambiguity policies are current implementation choices, not vendor guarantees. Test a native authored preset whose module sits in a nested folder and whose UI uses images/fonts, then compare dependency resolution, widget tree/state, image pixels and edit roundtrip. A rendered inspector UI is not by itself a native visual comparison. |

### Native source versus inference

The linked UVI pages establish host behavior, but they do **not** specify the
binary `.uvip`/`.ufs` layout, an atomic loader protocol, a lock-free queue, or
the DSP coefficient laws of every named module. `UVI4` XML, UFS namespaces,
script members and current sample paths are observed/implemented in this repo;
the official [MappingArticulations](https://lua.uvi.net/MappingArticulations.uvip)
and [FXControls](https://lua.uvi.net/FXControls.uvip) examples are useful public
XML fixtures ([prior inspection](../FALCON_RUNTIME_UI_GROUNDWORK.md)), but do
not prove protected-bank format coverage. The proposed immutable IR and atomic
commit below are engineering inferences from the documented asynchronous edits
and real-time ownership, not a claim that Falcon uses the same internal design.

## Minimum independent implementation path

1. **Build one version-pinned native contract corpus.** Use authored, clear
   programs and Lua scripts first. Record UVI product/version, plugin hash,
   host/sample rate/block size, XML fixture hash, script callback/event trace,
   task completion trace, state readback, screenshot or PCM hash as appropriate.
   Existing frozen-v1 tests and Workstation static original-byte checks remain
   hypotheses or narrower machine-code evidence until the corresponding native
   runtime event is captured. Do not print or persist protected source, samples,
   access values or bank keys.
2. **Keep the physical program graph.** Parse a bounded XML tree with source
   spans and ordered Program/Layer/Keygroup/Oscillator, processor, modulation,
   rack/chain, aux, output and unknown-node records. Resolve script/module/UI
   and sample dependencies under one bank-origin authority. Produce an immutable
   graph with stable internal node IDs and typed parameter descriptors, then
   lower only admitted nodes to shared `sampler_ir`. A missing audio-critical
   node must remain visible in load diagnostics, not silently disappear.
3. **Separate preparation from commit.** Decode/prepare samples and effect
   state on workers; queue bounded typed edits from script owner/UI/MIDI; at an
   audio-block boundary swap a validated graph or node-state generation.
   References and pending async tasks carry generation IDs so stale completions
   cannot bind a new preset or a replaced oscillator. Script callbacks receive
   completion on their owner thread. Old voices/tails need explicit lifetime
   policy and native measurements; immediate destruction is not assumed.
4. **Conform the script adapter independently.** Keep Luau only where native
   5.1 source/result fixtures agree. Verify callback dispatch, note identity,
   same-instant order, beat scheduling, typed parameters, persistence, module
   resolution and UI changes before calling an API supported. Publish precise
   Unsupported/Approximate findings for unavailable async loading, oscillator
   modes and DSP, with a direct path back to the source node.
5. **Admit sound one scoped component at a time.** A parameter catalog row or
   v1 leaf kernel does not establish audible parity. Require same-version native
   PCM/stage traces for sample selection, voice envelope and routing first, then
   effects/modulation by scope, sample rate, block size, bypass, live parameter
   change, reload, tails and channel count. Whole-program parity requires no
   unbound audio-critical node in its active path.

### Receipt matrix for forward work

| Gate | Native receipt | Current local baseline to compare |
| --- | --- | --- |
| Language/API | Accepted/rejected source, callback/event trace, typed readback, callback error trace | `parity::check_source_syntax` (Luau only), `tests/lua_compatibility.rs`, `tests/uvi_audit.rs`, `tests/host_parameters.rs` |
| Scheduling | Note-on/release and generated-event sample indices over 32/64/256-frame blocks and tempo change; `spawn` versus `run` ordering | `ScriptHost` inline timestamped commands; live `ScriptThread` delayed queue and 1 ms poll |
| Async resources | `task.finished/success`, callback thread/order, old/new oscillator state and mapping zones for successful, failed and superseded loads | `script.rs` loadSample stub, `script/async_data.rs` data callback tests, `tests/load_data.rs` |
| Object/edit lifetime | IDs/readback before/after insert, move, remove, output change and preset reload; effect tail continuity | `script.rs` element proxies, `parameters.tsv`, `engine_parameters.rs`, `tests/host_parameters.rs` |
| Hierarchy/audio | MIDI event order and stage PCM taps at keygroup, layer, program, part and master, including aux sends and >2 input channels | `lib.rs` XML translation, `inserts.rs` nested rack regression, `scripted.rs` group routing; v1 admitted kernels are separate evidence |
| UI/dependencies | Native widget tree, image/font resolution, save/restore and edits for a public or self-authored script | `script/ui.rs`, `script/modules.rs`, `bank.rs`, `resources.rs`, `tests/ui_assets.rs` |

The existing 31 promoted `uvi_audit.rs` cases are useful authored host
regressions ([audit receipt](falcon-parity.md)); they do not replace any native
cell above. The recent installed-library scanner and headless checks can
establish access, command emission and audible KONTRA output, but neither
native Falcon PCM equality nor a speed advantage. Their results belong in the
separate library and runtime ledgers.
