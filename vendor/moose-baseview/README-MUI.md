# MUI native text-input patch

This directory is the `crates/moose-baseview` source from Matari-Audio/moose
revision **c1e0eed67159b61aea6e2ffeb905d5b4fbd452b0**, the revision in MUI's
previous lockfile. Its original MIT and Apache-2.0 licenses and README are retained.
Only this crate is vendored; it has no workspace-inherited manifest values.

The Linux `Window::close_bounded` API, bounded join, callback revocation and
regressions are ported from Matari-Audio/moose revision
**bffa4677d0b82119d38566ce7e932dc5c463d497**, under the same original licenses.
MUI also checks revocation while draining its added XIM callback queue. Detach
requires a pinned plug-in image and no registered host callbacks; callers must
revoke their handler's host state first. Ordinary close remains synchronous.

MUI additions expose `Event::Ime` and `WindowContext::set_ime_configuration`.
Configuration includes physical client-relative candidate geometry, surrounding
UTF-8 text, byte selection, and marked range. Platforms ignore identical values:
changing the caret cannot repeatedly disable/enable an active composition.

* Cocoa implements `NSTextInputClient` selectors and `interpretKeyEvents`, converts
  UTF-16 ranges, returns attributed surrounding text and screen candidate bounds.
  A synchronous text shadow covers multiple edits between UI ticks; focus loss
  immediately blocks late mutating callbacks.
* Windows uses IMM32 result/preedit strings and UTF-16 caret conversion,
  associates/disassociates the thread input context only on enable changes,
  positions composition/candidate windows, and serves `IMR_DOCUMENTFEED`.
* X11 uses the maintained MIT-licensed `zed-xim` fork of xim-rs, pinned to
  `16f35a2c881b815a2b6cdfd6687988e84f8447d8`, vendored at `../xim-rs` with an
  acknowledgement callback hook and required preedit-start reply. Its transport shares the original
  owned XCB connection, replaces the input context when field identity or focus
  changes, and processes incremental preedit/caret callbacks. Retired ICs remain
  rejected until their destruction reply; fresh creation waits for that reply.
  Negotiated event masks determine press/release forwarding and synchronous flags.
  Synchronous keys queue until the matching IC acknowledgement, without blocking
  the event loop; stale IC acknowledgements cannot release the current queue.
  Unrequested events retain local keyboard handling. It positions the candidate
  spot and reads `XMODIFIERS` and locale variables; it never mutates the host's environment or
  process-wide locale. An XIM server must be running before opening the window.

The platform mechanism was cross-checked with Apache-2.0 GPUI sources at
`a84689073d296dfd39987bc7dd478e43ef76d83a`, paths
`crates/gpui_macos/src/window.rs`, `crates/gpui_windows/src/events.rs`, and
`crates/gpui_linux/src/linux/x11/client.rs`. No GPUI source code is copied here;
platform glue is newly written against baseview's existing ownership model.

XIM has no standard surrounding-text selection protocol: X11 retains that
configuration for change detection, but only sends supported spot attributes.
This is not a native Wayland backend; XWayland does not provide Wayland
text-input-v3 support. Cocoa explicit replacement ranges are translated to ordered
UTF-8 selection events before their associated preedit/commit. Input identity
changes cancel the preceding native context composition. XIM forwarding follows
[X.Org XIM protocol event masks](https://xorg.freedesktop.org/archive/current/doc/libX11/XIM/xim.html).

## Maintaining this patch

When updating baseview, diff against the exact revision above first, retain the
plugin keyboard hooks, parent-window behavior, original display connection and
handler-before-native-window teardown. Keep native callbacks outside MUI's model
lock. Do not silently replace these APIs with no-ops on an OS.

The standalone `Cargo.lock` files here and in `../xim-rs` pin the regression
builds separately from MUI's workspace lockfiles. `tools/verify.sh root-test`
runs both library suites on Linux; CI also runs baseview's native library suite
on Windows and macOS. CI fetches each standalone lock before testing offline,
so XIM's test-only dependencies do not depend on a developer's Cargo cache.
Update and commit the corresponding standalone lock when changing these manifests.

Run the vendored unit tests and platform checks separately:

```
cargo fetch --manifest-path vendor/moose-baseview/Cargo.toml --locked
cargo fetch --manifest-path vendor/xim-rs/Cargo.toml --locked
cargo test --manifest-path vendor/moose-baseview/Cargo.toml --lib --locked --offline
cargo test --manifest-path vendor/xim-rs/Cargo.toml -p zed-xim --lib --features x11rb-client,x11rb-xcb --locked --offline
cargo check --manifest-path vendor/moose-baseview/Cargo.toml --target x86_64-pc-windows-gnu
cargo check --manifest-path vendor/moose-baseview/Cargo.toml --target x86_64-pc-windows-msvc
cargo check --manifest-path vendor/moose-baseview/Cargo.toml --target x86_64-apple-darwin
cargo check --manifest-path vendor/moose-baseview/Cargo.toml --target aarch64-apple-darwin
cargo test -p mui-baseview
```

## Native acceptance recipe

Run `cargo run -p mui-baseview --example ime` for two editable fields and a
native commit counter. Use the same view embedded under a real host's raw
parent, then repeat after closing/reopening the editor. Start Japanese/Chinese/
Korean system IMEs (IBus/Fcitx XIM on X11, `XMODIFIERS=@im=...` at process startup).
Check that preedit is visible without editing the value, candidate movement
changes the underline/cursor, Enter commits exactly once, Escape cancels, and
ordinary typing/dead keys still work. Try `a😀é`, non-Latin text and selection
replacement; candidates must follow the actual caret at 1x/1.5x/2x, after zoom,
resize, scrolling and parent movement. Switch field focus and native window focus
mid-composition; no stale preedit should reach the newly focused field. With
keyboard capture off, host shortcuts and held-key releases must keep working.
Open two windows, compose in one and disable text input in the other; IMM32 must
not cancel the first window's shared-thread composition. Confirm close/reopen
recreates independent native input contexts and never keeps a dead connection.

Cross-compilation proves API/type compatibility, not native IME usability; these
manual checks remain required on each operating system and real plugin hosts.
