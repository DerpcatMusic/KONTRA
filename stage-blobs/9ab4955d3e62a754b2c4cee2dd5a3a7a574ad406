# KONTRA changes to moose-baseview

Source: [Matari-Audio/moose](https://github.com/Matari-Audio/moose/tree/bffa4677d0b82119d38566ce7e932dc5c463d497/crates/moose-baseview),
revision `bffa4677d0b82119d38566ce7e932dc5c463d497`, crate
`moose-baseview 0.3.4-moose.1`. This copies only the already locked baseview
crate, not the surrounding MOOSE framework. Its upstream authors are retained
in `Cargo.toml`, and its MIT and Apache-2.0 licenses are retained unchanged.
`README.md` records the earlier RustAudio/baseview origin and MOOSE changes.

KONTRA's authored patch changes `src/platform/x11/visibility_tree.rs`:

- Reparenting retains the window itself and removes its former ancestors.
- Reparenting to the root refreshes mapping state and cached visibility before
  returning. The root remains excluded from the ancestry list.

The previous code retained an unmapped old parent and skipped regeneration on
a root reparent. An original Xvfb test reproduced a server-viewable child with
zero frame callbacks after that transition. The late-parent-map control received
callbacks normally. This proves that transition's bookkeeping defect; it does
not establish the cause of a separate reported host failure.

Original native regressions are in `tests/x11_visibility.rs`. They require an
isolated X11 server and are ignored by default. Run with:

```sh
xvfb-run -a cargo test --locked --profile ci -p moose-baseview --test x11_visibility -- --ignored --test-threads=1
```

No backend, surface alpha, rendering, constructor, or other platform behavior
is changed. No upstream executable or commercial instrument was used as a
fixture.
