# KONTRA native window patches

Based on MUI `cc4e61e6ebdf46ecf5a473dd05211c7e7abfe105`, including its
native IME, cross-platform accessibility, display ownership and shared scene
snapshot fixes. MUI dependencies, including `moose-baseview`, resolve to the
application's current MUI Git revision through `Cargo.lock`; MOOSE's older
host must not shadow these fixes.

Native provider exceptions: `accesskit_macos` and `accesskit_unix` come from
MUI's vendored Git packages at the application revision in `Cargo.lock`.
The Unix provider joins generation-owned workers on teardown. On macOS,
the native provider uses runtime class names that coexist with other Matari
plugins loaded by the same host. Its Git revision
matches the other MUI packages in `Cargo.lock`. The combined accessibility
adapter now uses upstream `NativeAccessibility` and `AccessibilityUi` through
the distinct `kontra-native-host` package. It does not override upstream
`mui-baseview`; provider/action/X11 bounds implementation is shared instead of
copied. The native endpoint stays on the window thread, portable actions/tree
preparation run under the model lock, and provider publication happens after
release. `WillClose` drops the native-first endpoint pair before model access;
the window adapter also drops Handler before its WindowContext.

Native IME event and geometry conversions use upstream public helpers. The
local code only converts the resulting physical geometry into baseview's
representation. `Driver::ui_scale()` is applied once, preserving Unicode ranges.

Linux picker parenting snapshots the live X11 window before the first frame
and clears it before native close, outside the UI model lock. The local
`on_x11_window` adapter hook forwards that lifetime to the portal picker;
closing an older window cannot clear a newly opened window's parent.

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
