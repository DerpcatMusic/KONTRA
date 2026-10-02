# Legacy native UI execution subset

The optional plug-in adapter loads a requested `.nui` resource with embedded
Lua 5.4. It lowers the supported `@component` form, resolves dotted resource
module names, and retains the real resource bytes and module identity.
Entry resolution retains the optional `general.kontaktTargetVersion` metadata;
an explicit `.kscript` entry is rejected before legacy Lua evaluation. Bare
`load_native_ui` requests retain their legacy `.nui` intent.
Lua errors identify `.nui.lowered.lua`; line counts after a component are preserved,
while locations inside the rewritten property/child tables refer to lowered Lua.

Supported scene primitives are `ZStack`, `Text`, `Rectangle` and `Image`, with explicit
frames, RGB/RGBA and packed ARGB colors, computed text, local scalar state, inherited contexts and tap callbacks. Component functions
expand lazily within their parent context; local state hooks persist by
component source identity and scene path across refreshes. The
editor uses the existing MUI painter. Connected integer KSP UI controls retain
their exact script slot/control ID; duplicate identifiers use the first slot,
as specified for `expose_controls`. Tap edits use the existing bounded plug-in
queue and real `ui_control` callbacks. Published values include connected controls
from all successful slots, independently of the selected classic performance page.
Color opacity returns a new value with the requested alpha, preserving RGB.
This value contract follows the [NI Color reference](https://developer.native-instruments.com/komplete-ui/docs/StandardLibrary/Color/)
and matches the constructor/opacity forms observed in legacy resources; it does
not make the modern typed source language executable by this Lua adapter.

Scene preparation runs in the loader. Lua scene evaluation and gestures run in
editor/worker services; Lua is never stored in or executed by the audio engine.
Scalar snapshot copying charges the existing refresh budget. Resource payloads
are bounded before reading: 1 MiB per module, 8 MiB total, 256 modules. The VM
has a 64 MiB memory limit and 100,000 instructions per scene evaluation/gesture.
Scenes allow 4,096 unique nodes, depth 64, 8 KiB text, frames up to 4,096 pixels,
and 64 edits per gesture. Module loading is sealed after preparation; later
missing `require` calls fail without editor file I/O. OS, I/O, package/native
modules and dynamic source loaders are not exposed.
Protected calls preserve ordinary Lua results/errors, but rethrow sticky
instruction exhaustion even across nested catches. Coroutines and `xpcall`
are unavailable. Native temporary MIDI export remains explicitly unsupported
when invoked; looking up its API does not fabricate a file or enable mocks.

Image filenames retain their evaluated resource identity. Initial images prepare
on the loader; changed references queue through the same worker and publish only
to the matching scene and source epoch. SVGs retain paths, gradients and layers
through existing MUI vector paint. Supported PNG/JPEG/WebP images retain source
pixels. The cache retains both successes and failures, with at most 256 names
and a 128 MiB retained asset budget;
missing or unsupported assets are diagnosed and never replaced by invented artwork.
Unframed, nonresizable images use their intrinsic size; explicit frames and
resizable images use the authored frame.
Filtered SVG device-size misses also queue through that loader. Painting reads
only prepared cache entries; a cold or queued image remains pending. A retained
scene request bit permits queue-full retries on the existing worker, and stale
scene/epoch jobs are discarded before preparation. No filter rendering runs in
the editor snapshot or paint path.

Additional primitives/modifiers, indexed array connections, generalized
reactive subscriptions and parameter writes outside gestures are explicitly
unsupported. Unsupported scenes keep the classic KSP projection available and
record a loader warning; this subset does not establish full library native UI
layout or artwork parity. Modern `.kscript` UI is separate and remains unsupported.

The authored regression
`native_ui_scene_uses_exact_slot_plugin_edits_and_published_callbacks_without_audio_heap`
uses two actual KSP slots and the production scene preparation, edit queue,
callback execution, publication and MUI primitive adapter. It checks duplicate
identity, stale epochs, nested modules, bounded resources, source line diagnostics
retained vector artwork, changed-asset worker requests, stale asset rejection
and explicit unsupported-primitive rejection. No instrument-library code or artwork
is included in its fixtures.
