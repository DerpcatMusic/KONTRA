# KONTRA native window patches

Based on MUI `cc4e61e6ebdf46ecf5a473dd05211c7e7abfe105`, including its
native IME, cross-platform accessibility, display ownership and shared scene
snapshot fixes. MUI dependencies, including `moose-baseview`, resolve to the
application's current MUI Git revision through `Cargo.lock`; MOOSE's older
host must not shadow these fixes.

macOS exception: `accesskit_macos` comes from MUI's vendored Git package
at `7d140a387be220fb28002cd6e3c35a79715f2193` instead of crates.io, so
the native provider uses runtime class names that coexist with other Matari
plugins loaded by the same host. Its Git revision
matches the other MUI packages in `Cargo.lock`. The combined accessibility
adapter remains the existing KONTRA implementation.

Retained KONTRA behavior:

- Five list rows per wheel notch, with acceleration across a fling.
- Pointer hide and restore during knob drags, and diagonal resize cursor.
- Bounded native drag timing capture for the diagnostics report.
- GPU initialization, uncaptured-error and panic diagnostics, with Windows
  backend selection that avoids implicitly initializing Vulkan in DAW hosts.

Raw key-down/up interception is now provided by upstream MUI's `KeyHook` and
`Requests::on_key`; KONTRA's adapter continues to use that contract. Local
native timing capture coexists with upstream MUI's profiling phases.

GPU errors use `Host::observe_gpu_error` once per observer and device
generation. The upstream callback keeps recording errors and losses; the
window forwards them to KONTRA's sink before/after present, outside the model
lock. Errors between ticks coalesce according to MUI's observation contract.
