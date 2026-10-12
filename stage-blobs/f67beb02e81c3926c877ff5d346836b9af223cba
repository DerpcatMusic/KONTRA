# MUI XIM transport patch

Vendored from the MIT-licensed Zed fork of Riey/xim-rs at exact revision
`16f35a2c881b815a2b6cdfd6687988e84f8447d8`. Original source, README and LICENSE
are retained. MUI adds a default `ClientHandler::handle_sync_reply` callback
and invokes it for `XIM_SYNC_REPLY`, which upstream otherwise discards. It also
sends the mandatory `XIM_PREEDIT_START_REPLY` after successful preedit start,
with -1 indicating the unlimited owned string buffer. Text-bearing `XLookupBoth`
commits use the same single text commit path as `XLookupChars`; synchronous
keysym-only commits receive an acknowledgement while retaining upstream
unsupported keysym interpretation. The
baseview client uses the new callback to serialize synchronous key requests
without blocking the GUI thread. No parser features have been added.

The callback is source-compatible with handlers that do not override it. The
baseview regression tests cover outstanding requests, deferred keys, stale IC
acknowledgements and negotiated masks.

The standalone `Cargo.lock` is committed for the Linux regression suite. CI runs
`cargo fetch --manifest-path vendor/xim-rs/Cargo.toml --locked`, then tests the
`zed-xim` library with `x11rb-client,x11rb-xcb` and `--locked --offline`. This
includes the MUI transport regressions and preserves the original crate warnings.
