# Authored native UI frontend and connection gate

This developer prototype lowers a small `@component` syntax subset into Lua and
executes an authored scene in the installed `lua5.4` process. It is not called by
the plugin, executes no library assets, and renders no pixels. It does not claim
to execute Conflux or implement the complete legacy Native UI language.

The fixture uses a root-relative module, component properties and children,
named modifiers, a gesture closure, state, computed text and two scalar Kontakt
connections. `native_ui_frontend.py` preserves quoted strings/comments while
lowering component applications. The scene host supports only ZStack, Text,
Rectangle, frame and tap gestures. Unknown APIs/modifiers/identifiers, duplicate
properties, array connections and module cycles fail explicitly. Computed values
use pull evaluation; dependency tracking and automatic UI invalidation remain
unimplemented.

The Rust gate builds two **real KSP script slots** and exports exact slot/control
addresses. It implements NI's documented exposed-identifier rule: all script
slots, no variable sigil, first duplicate wins. Ignored duplicates are reported.
This is the prototype bridge's registry policy, not a claim that production
`expose_controls` or a native UI runtime is implemented. [NI command reference](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/user-interface-commands#expose_controls).

The Lua scene emits two gesture requests using those addresses. The same running
Rust host dispatches actual `Runtime::ui_control`, then returns the callback's
value to Lua. Expected output changes Amount17→22→44 and Remote9→14→24, while
the later duplicate Amount90 remains90. Scene text becomes Amount:44:1 and
Remote:24:1 without reconstructing scene-local state. This is a synchronous
developer protocol; production would use existing bounded UI/audio queues and
live revisions instead of subprocess blocking. No Lua execution belongs on audio.

To prepare the modules:

```sh
python3 tools/native_ui_frontend.py tests/fixtures/native-ui-scene /tmp/native-ui-lowered
```

Compile `tools/native_ui_bridge_probe.rs` with `rustc +1.98.1 --edition=2024 -O`,
`--extern kontakto=<frozen matching rlib>` and `-L dependency=<matching deps>`.
Run the resulting executable with the exact clean source revision, this
worktree's absolute path and an ignored output folder. Do not link an old rlib
against dependencies overwritten by another build. The probe checks build identity
and writes its registry/transcript. Parent coordinates the shared build/link slot.

Module limits are1MiB each,256 modules/8MiB total,4096 component applications;
Lua has a100,000-instruction budget, module-only imports and no file/process APIs
in the module environment. The POSIX developer launcher imposes256MiB address
space,2CPU seconds and disables core dumps. Rust retains at most64KiB protocol
output and accepts exactly two fixture requests. These are probe ceilings, not
production resource budgets or a portable embedded runtime.

Next gate: a maintained embedded Lua runtime/frontend, source mappings, typed
array/property/touch connections, reactive dependency lifetimes, epoch/cancel
handling, real MUI scene/image adapters and unchanged library-module acceptance.
Adding PNG/WebP/SVG assets alone cannot substitute for that executable scene.
