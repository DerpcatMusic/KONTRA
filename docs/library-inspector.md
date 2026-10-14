# Offline library inspector

The inspector traces local library source to Kontra's static script bindings,
converted instrument structure and optional initialized UI model. It does not
open the plugin or audition audio. Use it before a runtime or native comparison
to identify where information is missing and which implementation owns it.

Native Kontakt and Falcon/Workstation behavior and their official scripting
contracts are the reference. Agreement between Kontra v1 and v2 is useful
regression evidence and can still preserve the same mistake. The
[Kontakt native contract audit](audit-2026-10-12/kontakt-native-contract.md) and
[Falcon native contract audit](audit-2026-10-12/falcon-native-contract.md) distinguish
documented rules, local findings and the native captures still needed.

## Start the workbench

Build through the repository's heavy-job wrapper:

```sh
/home/derpcat/.cache/kontakto-heavy cargo build --locked --profile ci -p corpus-health
python3 tools/library-inspector.py '/path/to/Piano.nki' --source
```

For a program inside a UFS bank:

```sh
python3 tools/library-inspector.py '/path/to/Bank.ufs::Presets/Piano.uvip' --source
```

For UI diagnosis without a plugin, DAW, window or audio device, build the optional
production renderer and use the UI-focused path:

```sh
/home/derpcat/.cache/kontakto-heavy cargo build --locked --profile ci -p corpus-health --features ui-render
python3 tools/library-inspector.py '/path/to/Piano.nki' --render --ui-only
python3 tools/library-inspector.py '/path/to/Preset.nksn' --render --ui-only
```

`--ui-only` skips the separate static source inventory, parity classification and
placeholder audio plan. It still runs the production parser and script initializer
once, then shares that initialized model with the renderer. It cannot be combined
with `--source`. Falcon uses the ordinary constructor without line-coverage
instructions; the complete initialization audit uses coverage. Omit it for the complete source/conversion audit, including stage
timings that distinguish audit overhead from product initialization.

For a UVI initialization profile, add `--profile-init`. The Load timings view
separates compilation, object-graph construction, module execution, restore,
`onInit`, parameter overlays, IR translation and UI export. It also shows selected
native API wall clocks and bounded Lua safepoint stacks. Nested clocks overlap;
sample counts distinguish inclusive visits and the observed top frame, rather
than exclusive CPU time. Stack sampling is limited to 500 Hz, four frames and
512 distinct frame records. It reports stack-collection time; aggregate profiling
overhead needs a same-executable on/off comparison. Production constructors do
not enable this collector.
No script text, argument values, stack-memory bytes or heap contents are captured.
Compare like modes and record OS-cache/workload conditions before claiming speed
improvements; an inspected constructor is not a plugin-startup measurement.

For deterministic UVI inspection, set `KONTRA_UVI_AUDIT_SEED` to an unsigned
32-bit integer. Initialized reports record the applied seed in `mode.audit_seed`.
Both profiled and ordinary inspection use that explicit scanner configuration;
omitting it preserves the previous RNG behavior. Plugin defaults are unchanged.

Snapshots use the same base-NKI resolver and identity check as playback. Saved
snapshot state is applied before initialization; UI resources belong to the base
NKI. If no unambiguous base is found, supply `--base ORIGINAL.nki`. A mismatched
base is rejected, rather than previewing another instrument's interface.

For Kontakt interfaces that change during a deferred `persistence_changed`
callback, use the explicit production lifecycle witness:

```sh
python3 tools/library-inspector.py '/path/to/Preset.nksn' --settle-ui
```

This mode initializes the production loader once, then compares the compiled
initial interface with its state after bounded Part/V2Core plan activation.
It creates the real streaming and DSP runtime, without a plugin, DAW or audio
device. It injects no note, controller, transport or gesture input. Authored
startup and listener callbacks may run; any audio output is measured and
discarded. The shared read-only UI effect handler applies position, visibility
and other supported writes; unavailable file/MIDI services remain observable.
This is a diagnostic runtime path. The ordinary `--ui-only --render` path still
creates no Runtime and explicitly labels deferred persistence work as pending.

Activation stops after 96,000 audio frames at 48 kHz, a five-second stepping
threshold, or 65,536 applied effects. Wall/effect thresholds are checked between
bounded blocks and effect drains; the launcher's process timeout supplies the
outer bound. Reports distinguish completion from pending callbacks, service
failures, retained callback faults and runtime problems, including faults already
removed from the active scheduler. Later user interactions and native pixel
equivalence remain unverified. `--settle-ui` requires the `ui-render` build and
implies `--ui-only --render`; it cannot be combined with `--source`.

Exact retained compiler roles distinguish a periodic listener driver from finite
activation work. Readiness and background observation are separate. Early stop
also requires one completed authored callback from each active driver's exact
retained mapping, followed by its UI effect drain. This captures timer-delayed
initial UI writes and reports `background_active` while retaining driver counts.
Later ticks can still change the UI; one completed cycle does not prove visual
stability. Unclassified or omitted pending work is conservatively retained; a
queued fault prevents readiness even when only a listener remains active.

`--render` implies `--initialize`. UI projection gains original-bitmap and Kontra
vector CPU witnesses, page/presentation selectors, an optional bounds overlay,
widget visibility and binding filters, asset/font request outcomes, and timing
records. Hidden-parent visibility comes from the shared UI IR query; resolved
coordinates come from the production layout. An asset that was never requested is
shown separately from a missing, ambiguous, failed-read or failed-decode request.
Recorded requests span all rendered pages and both presentations; bounded traces
retain aggregate counts when detailed records overflow.

The shared Falcon model and painter consume button off/on caption colours,
menu arrow visibility/colour and outlines, and the explicit knob/slider value
popup policy. The policy uses Kontra's passive pinned readout during adjustment;
Falcon's precise popup timing and pixel layout still need native comparison.
Authored callbacks refresh these properties through the existing UI snapshot
path. Malformed colour values and properties attached to an unsupported widget
kind stay visible as findings. These contracts follow the official
[Button](https://lua.uvi.net/class_button.html),
[Menu](https://lua.uvi.net/class_menu.html),
[Knob](https://lua.uvi.net/class_knob.html) and
[Slider](https://lua.uvi.net/class_slider.html) references.
Unmodeled meter ownership/channel behavior is retained as a diagnostic;
an offline witness never synthesizes live meter activity.

Legacy NativeUI requests use the actual production package, Session and view with
the already-initialized control seeds. They retain the evaluated composition in
both presentations. Evaluated NativeUI element geometry is reported separately
from classic IR binding coordinates. A loading/error caption is never returned as
an authored UI witness. This executes NativeUI's own module entry point, without
rerunning KSP `on init` or creating an audio Runtime.

CPU witnesses are Kontra renderer evidence, not native Kontakt/Falcon pixel
equivalence. They do not execute gestures or supply live waveform/meter services.
Pages are limited to 16 per interface and 2048 pixels per side; larger pages are
downscaled with the scale recorded. Resource preparation has a 30-second bound;
pending requests and failures keep a witness partial. Original artwork and PNG
pixels remain in process memory and private loopback routes with `no-store`.

Add `--initialize` to inspect actual product conversion and the initialized UI
model. For an NKM, `--program 2` selects the third embedded program for conversion;
the source inspection still includes all physical programs and script slots.
The default selected program is zero. Multiple input paths open together in the
workbench. `--binary` selects another corpus-health build; the default selects the
newest available optimized `dsp-dev`, `ci` or `release` binary on the configured
shared cargo-target volume. Rebuild this checkout before measuring; the launcher
does not silently fall back to a slow debug binary.

The Problems view lists recorded unsupported features, stubs, approximations,
faults and conversion gaps. Selecting a script problem opens its coordinates
alongside the frontend's lowering records and implementation/spec references.
Original code appears when `--source` was requested or the opened report includes it.
Converted structure exposes groups, zones, processors, modulators, routes,
controls and physical ownership. UI projection exposes model widget coordinates,
bindings and omissions. Optional CPU witnesses retain authored bitmap/native
composition; the bounds overlay draws measured geometry, not invented controls.

## Evidence boundaries

| Path | What is inspected | What remains unverified |
| --- | --- | --- |
| Kontakt default | Decoded chunk structure, every physical script slot, frontend reports and NCKP resource/field gaps | Product initialization, IR admission, sample binding, audio and live callbacks |
| Falcon default | Program XML hierarchy, script bindings, actual compiler-only Luau syntax checks and production XML-to-IR translation | Native Lua 5.1 semantics, script initialization, sample binding, audio and live callbacks |
| Optional initialization | Product initializer and UI model; Falcon IR includes `onLoad` parameter overlays through the shared scripted-loader path; Kontakt also attempts binding against zero-payload assets with deliberately fictitious sample metadata | Real sample admission, musical callbacks, rendered pixels, gestures and native equivalence |
| Optional `--render` | Production layout, selected-program asset worker, requested fonts/frames and CPU-rendered initial pixels; NativeUI uses the evaluated production composition | Native Kontakt/Falcon fidelity, GPU output, live waveform/meter services, user gestures and musical callbacks |
| Kontakt `--settle-ui` | Single real loader initialization, initial/activated models, production streaming/DSP scheduler and read-only UI effects, discarded output and explicit lifecycle/fault bounds | Later gestures and musical input, unsupported services, complete instrument parity and native pixel equivalence |

An available binding is not evidence that it executed. A successful stage means
that particular inspection completed, not that the whole library is compatible.
Unclassified fields remain unverified. Summary reports omit detailed coordinates
and lowering records; open a full per-file report for line inspection. Partial
program conversion does not establish complete NKM compatibility.

Kontakt UI projection reuses the translated initializer once. Positioned KSP
faults retain physical program/script-slot ownership and fail initialization;
compiler rejection fails the frontend stage. A rejected KONTRA frontend does not
establish that the native script is malformed. Omitted NCKP fields and controls
make UI projection partial. Default frontends retain categorized resource/field
gaps; optional source inspection adds categorized skipped counts in resource
metadata, the original NCKP JSON and its path.

Classic KSP painting follows the documented global z-layer, widget-type priority
and declaration order. Explicit native paint order remains authoritative. Parent
visibility and relative geometry are queried independently; sorting overlapping
widgets does not settle deferred callbacks or repair authored layout.

Kontakt product conversion can read sample headers and decode convolution impulse
resources. It does not audition samples or produce PCM output. The source-only
path avoids that initializer entirely. Runtime and audio evidence are always
separate, and native equivalence remains unknown in these reports.

Initialized Falcon inspection promotes the decoded source, initializes one host,
applies its parameter overlays and translates that resulting XML through the same
helper as the scripted bank loaders. The returned IR therefore reflects `onLoad`
changes; samples are not bound or decoded. Live/asynchronous changes require
separate runtime evidence.

## Source and file handling

The launcher captures reports in memory and serves them only on loopback, with
an unguessable URL and `no-store` responses. It has no analytics or uploads.
`--source` explicitly includes original code/XML; source is absent by default.
Clear memory removes loaded reports from the view; closing the launcher releases
its report data and removes its owned RAM workspace.

On this Linux environment, initialization requires bubblewrap. The launcher mounts
the filesystem read-only, permitting writes only in its owned `/dev/shm` directory.
Authored external writes consequently fail and remain part of the inspection;
library files are preserved. Scan mode also bypasses Kontakt product-cache lookup
and storage. Direct CLI initialization requires the launcher's read-only inspection
marker and scan mode. Use the launcher to establish the actual sandbox.

## Scriptable reports

The `corpus-health` binary also exposes:

```text
corpus-health inspect INPUT [--program INDEX|BANK_MEMBER]
    [--base ORIGINAL.nki] [--initialize] [--ephemeral-source] [--summary]
    [--ui-only] [--ephemeral-render] [--profile-init] [--settle-ui]
corpus-health inspect-list MANIFEST
```

The manifest is the existing `corpus-health list` format: `kind<TAB>id`, with
bank program IDs written as `BANK.ufs::MEMBER.uvip`. `inspect-list` emits compact,
source-free JSONL and reuses open banks within that process. Its source inspection
does not run initialization. Invalid manifests and failed stages return a nonzero
exit status while valid records remain individually inspectable.

For production load-stage counts without materializing a full inspector report,
use `corpus-health load-audit INPUT [--program INDEX] [--stream]`. Multis default
to every embedded program. The output contains source/admitted object counts,
unsupported feature counts, enabled and disabled DSP-slot/target outcomes,
unclassified tail byte totals, load time and process RSS. It never emits source
code, raw object bytes, samples or access data. DSP dispositions distinguish
an authored bypass from an enabled dropped slot; an approximated slot remains
unverified against the native law.

`--stream` binds all keys and scripts with a lazy 8 MiB head budget, 256 streaming
voices, two decoders and 64-frame blocks at 48 kHz. It measures readiness for
runtime publication, without creating a runtime, editor or auditioning audio.
These numbers therefore cannot be compared directly with editor RSS or first
sound. Run one input per fresh process for RSS attribution and use the existing
`tools/audit-load.py` `loader` / `loader-stream` modes for bounded read-only
shards. Only the private numeric header-cache directory is writable for stream
measurements. Empty-cache "cold" does not mean OS page-cache eviction.

Full reports use `format: kontra-library-inspector`, `schema: 1`. They contain
separate `stages`, `scripts`, `converted`, `ui`, `diagnostics` and `evidence`
sections, plus additive `performance` records. Script reports reuse `sampler-parity` coordinates, construct status,
owners and lowering records. A source-free report can be opened through the
workbench's Open reports action or `--report-json FILE`.

Validated Falcon hierarchical menus include their grouped tree and original
zero-based item indices in each widget's `menu_hierarchy` metadata. The shared
production popup writes the original item value when a leaf is selected;
ambiguous paths and renderer limits remain explicit UI omissions. See the
[hierarchical menu evidence](architecture-v2/UVI_HIERARCHICAL_MENUS.md).

Load timings include process wall time and launcher time including the heavy-job
queue, sandbox and JSON transfer. Asset preparation includes worker wait,
discovery, reads, decoding and cache admission. CPU paint/PNG time includes image
encoding. Nested phases overlap; do not sum them as exclusive costs. OS cache
state, native-host first-frame timing and DAW/audio latency are separate evidence.
Falcon initializer reports also identify unavailable script sample-loading services
separately from missing UI assets. UI image dimensions share production resource
authority; audio/impulse decoders and sample binding are absent in this preview.
A failed resource task consequently does not establish that its bank member is
missing, or that every resource-dependent `onLoad` branch matches plugin startup.
`--ephemeral-render` puts protected PNG pixels in the returned JSON: use the
memory-only launcher, rather than redirecting that output to a report file.

## Validation

Authored backend fixtures cover physical empty/bypassed multi slots, source
opt-in, malformed typed input, initialization refusal, compact evidence and
bank-member identities. Eight launcher tests cover read-only mounts, unsafe
initialization/render refusal, report schema, memory-only report/PNG serving,
snapshot base arguments, manifest IDs and source-free timeout progress.
The shared browser exercised problem navigation, lowering, source escaping,
conversion records, UI omissions and clearing. Final desktop and narrow previews
had no page overflow or console errors.

The related [Kontakt audit](audit-2026-10-12/kontakt-parity.md) records exact v1
NCKP fields that v2 omits, with source references and retained diagnostics.
