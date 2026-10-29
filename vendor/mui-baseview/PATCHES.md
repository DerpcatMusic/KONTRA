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

Native rendering also uses MUI's retained CPU rasterizer and native presenter.
`MUI_RENDERER=cpu` bypasses GPU initialization. GPU initialization, terminal
resize/present, and surface-recovery failures detach the GPU and select CPU
rendering for that window, without periodically retrying the failed driver.
CPU presentation keeps model snapshots outside the native present call, uses
reference-resolved welding, and preserves input, pointer, IME, accessibility,
portal parenting and native timing. A successful CPU submission uses the existing
`Presented` timing outcome and records `mui-baseview/cpu_frame_presented` once.
CPU presenter failures stay pending and retry after 500 ms; they do not clear
the unpainted scene. CPU target-size limits remain MUI's existing limits.

The shared X11 CPU regression checks actual opaque green window pixels,
re-presentation after Expose, idle retention, close/reopen, and recovery after
a destroyed GPU device causes resize failure. Run on an isolated X11 server:

```sh
MUI_RENDERER=cpu xvfb-run -a cargo test --locked -p kontra-native-host \
  native_cpu_fallback_presents_and_reopens -- --ignored --test-threads=1
WGPU_BACKEND=metal xvfb-run -a cargo test --locked -p kontra-native-host \
  native_cpu_fallback_presents_and_reopens -- --ignored --test-threads=1
xvfb-run -a cargo test --locked -p kontra-native-host \
  native_resize_failure_falls_back_and_presents -- --ignored --test-threads=1
```

`native_destroyed_parent_and_drawable_stop_callbacks` deletes a real X11
host parent and a standalone drawable without calling editor close first. It
asserts server deletion, spontaneous handler release, one `WillClose`, and no
further frame/resize callbacks. It runs in the same ignored native-host suite.

Linux `open_threaded` shares `open`'s parent validation, headless offer, settings,
and handler construction, but returns baseview's typed no-host `ThreadedWindow`.
The MUI editor can move its control handle without moving native state. Each open
keeps a separate Requests object and worker channel; close/reopen cannot revive
old geometry or close work. The original `open` API remains available.

The MUI editor retains its X11 parent hook and installs it on each fresh Requests
generation, including the first open. Shared FnMut delivery is serialized before
checking that generation; an active old cancellation precedes the next creation,
and a delayed old cancellation cannot clear a reopened parent's identifier. The
registration mutex is released before user code. The existing serialized hook
contract does not support synchronously opening another window from that callback.

## Frame pacing and counts (`src/pace.rs`)

- **Capped retries:** a GPU present that returns `Frame::Skipped` (occluded, timeout, outdated) gets one immediate retry, then waits 100 ms unless a new scene arrives. Before this, an unpainted frame snapshotted the scene and acquired a surface every refresh for as long as a host hid the surface.
- **Stuck accessibility flag:** a tree request that `AccessibilityUi::prepare` cannot answer stops the a11y redraw until one is answered. Upstream `AccessibilityUi` keeps `asked` set when the reader deactivates before the next tick (`Gone` clears `active` only), so `wants_tree()` redraws every frame. **Upstream fix:** clear `asked` in the deactivation handler, or make `wants_tree` require `active` (mui `crates/mui-baseview/src/a11y.rs`).
- **Session log:** every 10 s with any activity, a `mui-baseview/frames` breadcrumb records presented / skipped-unpainted / a11y-redraws / requested counts.
