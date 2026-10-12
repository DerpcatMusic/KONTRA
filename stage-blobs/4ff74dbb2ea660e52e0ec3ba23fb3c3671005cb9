# moose-baseview

MOOSE's vendored copy of [baseview](https://github.com/RustAudio/baseview), the
low-level windowing library for audio plugin UIs.

- Upstream: `baseview` **0.3.4** from crates.io, git commit
  `bc987b96c22cbdb36507196dbbcad00c768e135b` (the `.cargo_vcs_info.json` of the
  published crate). The first commit that adds this directory is that source
  unchanged except for the package name and the removal of upstream's
  `[workspace]` table (its examples are not vendored).
- License: unchanged, MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`).
- The public API stays source compatible with upstream 0.3.4. MOOSE additions
  are additive only. Depend on it under the upstream name so `use baseview::*`
  keeps working:

  ```toml
  baseview = { package = "moose-baseview", path = "crates/moose-baseview" }
  ```

## MOOSE additions (vs upstream 0.3.4)

All additive; nothing upstream exposes was renamed or removed.

API:

- `WindowSettings::scale_factor_override` / `with_scale_factor_override`,
  `Window::set_scale_factor_override`, `WindowContext::set_scale_factor_override`:
  a host-provided scale that wins over the OS scale on Windows and X11 (no-op
  on macOS). It does not resize the window; call `resize(logical)` after it.
- `WindowEvent::ScaleFactorChanged(f64)`: the platform scale changed.
- `Window::set_keyboard_capture` / `WindowContext::set_keyboard_capture`
  (KURV K23): Windows only; with capture off, keys stay in the host's pump.
- `pin_current_image_for_detached_work()` (KURV K24/K27): pins the plugin
  binary so a detached render thread can never outlive its code.
- `Window::close_bounded(timeout)` on Linux: opt-in X11 close for an editor
  whose host state was already revoked. Detaches a stalled window thread only
  after pinning its image; normal `close` still waits for the thread.

Behaviour:

- Windows: plugins never set process DPI awareness; the per-thread DPI guard
  restores the host thread's context; `ProcessDpiAwareness` mapping fixed.
  Child windows handle `WM_DPICHANGED_AFTERPARENT` (keep the logical size,
  emit `ScaleFactorChanged`); top-level windows emit it on `WM_DPICHANGED`.
  With an override, hit-testing and sizes use the override consistently.
- Windows: `OleInitialize` balanced with `OleUninitialize`; an MTA host
  thread disables drag and drop instead of failing window creation. System
  DLLs load with `LOAD_LIBRARY_SEARCH_SYSTEM32`.
- X11: core auto-repeat pairs become one repeated key-down (MOOSE X01);
  ignored key events are forwarded to the embed parent, with synthesized
  key-ups on focus loss and close (KURV K26); `Xft.dpi` clamped to 0.5-4
  (KURV K25); override and fallback scale honoured at creation. XDND limits
  property sizes, verifies selection replies, and parses URI lines before
  percent decoding.
- macOS: the backing scale is re-read when the view moves into a window, so a
  view created before the host attached it gets its real scale.

Not ported (upstream already covers it, or not needed): KURV's HWND parking on
close (upstream destroys synchronously), the 4 ms Windows frame timer
(upstream uses display refresh), X11 parent tracking and XDND (already upstream).

`rustfmt.toml` carries upstream's formatting settings so the workspace
`cargo fmt --check` leaves these sources in upstream style.
