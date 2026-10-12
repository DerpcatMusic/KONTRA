# moose-clap

CLAP format wrapper for the moose audio plugin framework.

## Overview

Bridges a moose `PluginExport` implementation to the
[CLAP](https://cleveraudio.org/) plugin API. The `export_clap!` macro generates
the CLAP entry point, plugin descriptor, and all required extension callbacks so
the plugin appears as a native CLAP plugin to any compatible host.

User plugins typically take a direct optional dep on this crate
(`moose-clap = { workspace = true, optional = true }`) gated behind a
`clap` Cargo feature; the `moose::plugin!` macro emits a
`::moose_clap::export_clap!(...)` call when that feature is on. `cargo
moose build --clap` / `install --clap` selects it at the CLI.

## What it handles

- CLAP entry point and plugin factory
- Plugin descriptor (name, ID, vendor, features)
- Parameter mapping (clap-params extension)
- Audio processing bridge
- State save/restore (clap-state extension)
- GUI embedding (clap-gui extension)
- Note port configuration (clap-note-ports extension)

`ProcessContext::bus_routing` preserves the selected CLAP port configuration's
flattened bus ranges. Activation is `Unknown`: process-buffer presence and
pointer validity describe audio storage, not whether the host routed a bus.

## Key macro

- **`export_clap!`** -- generates the CLAP entry point for a `PluginExport` type

Part of [moose](https://github.com/Matari-Audio/moose). [Docs](https://truce.audio/docs/).
