# Native visibility preservation and browser reveal

This change builds on UI integration `1a1931d` and keeps the MUI lock at
`822b1922ad1872aaee15b371a31410b0f3580b88`.

The newer framework integration had lost KONTRA's `c96ad3a` X11 visibility
behavior. The vendored native baseview retains the child while discarding its
former parent ancestry and refreshes cached visibility when reparented to root.
The application's MUI source patch consumes this baseview through its existing
local mui-baseview host. Its sibling xim-rs path dependencies are included with
upstream licenses and provenance. The generated baseview library lock is ignored;
the application Cargo.lock remains authoritative for the plugin.

The browser's custom reveal path now follows MUI ListState's nearest-item rule:
when a row exceeds the viewport height, show its header. Previously a 60-point
row starting at 100 in a 24-point viewport scrolled to 136, hiding its header.
The new offset is 100. Normal rows retain their existing minimal reveal behavior.
Wheel acceleration, scroll animation and product keyboard actions are unchanged.

## Focused verification

The same original X11 tests ran under separate Xvfb servers against immutable
MUI822 and the candidate. Baseline: late-parent mapping passed, root reparent
failed. Candidate: both passed. They verify native frame callback delivery,
not rendered pixels or actual DAW/editor lifecycle.

```sh
xvfb-run -a cargo test --manifest-path vendor/moose-baseview/Cargo.toml \
  --test x11_visibility -- --ignored --test-threads=1
tools/test-browser-geometry.sh
```

The browser geometry tests compile the actual pure source module. All three
pass; restoring the old reveal rule makes the oversized-row case fail while
the ordinary/boundary cases pass. Rustfmt parses the updated browser using the
2024 edition, and the staged diff passes whitespace checks. A full plugin
build, GPU pixel test, real host interaction and Mac/Windows testing were not
performed during this focused change.

## Further framework reuse

Upstream NativeAccessibility/AccessibilityUi and native IME helpers can replace
local bridge duplication after giving the custom host a distinct package name:
the current local `mui-baseview` patch shadows the upstream package supplying
those APIs. Preserve native wheel/pointer hooks, parent-dialog cancellation and
bounded capture, and validate lock release before provider publication and
window-thread teardown. A rename alone does not complete that migration.

ListState/variable_list can replace browser height indexing and spacers, but
requires permanent unique row IDs, retained model state and a deliberate
scroll/focus handoff. The browser accepts duplicate favorites and has custom
Left/Right and Shift+Enter actions; index keys or enabling framework keyboard
handling blindly would change behavior. This focused fix does not claim that
larger migration, an application speedup or full framework runtime parity.
