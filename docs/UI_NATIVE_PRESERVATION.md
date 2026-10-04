# Native visibility preservation and browser reveal

This change builds on UI integration `1a1931d` and keeps the MUI lock at
`822b1922ad1872aaee15b371a31410b0f3580b88`.

The newer framework integration had lost KONTRA's `c96ad3a` X11 visibility
behavior. The vendored native baseview retains the child while discarding its
former parent ancestry and refreshes cached visibility when reparented to root.
The application's MUI source patch consumes this baseview through its existing
local kontra-native-host. Its sibling xim-rs path dependencies are included with
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
performed during the initial visibility/reveal stage. The subsequent shared
bridge migration passed the full plugin all-targets/standalone cargo check,
21 headless custom-host tests, and both ignored native tests under Xvfb with
software GL. Those prove presentation/replacement and lifecycle, not pixel
readback or real DAW/macOS/Windows runtime behavior.

## Shared accessibility and IME bridge

The custom host is now named `kontra-native-host`; the upstream mui-baseview
patch is removed. moose-mui keeps its stable dependency alias and `window`
re-export, while the custom host consumes upstream NativeAccessibility,
AccessibilityUi and IME helpers at the existing pinned MUI revision.
Duplicated provider actions, semantic translation and X11 bounds code are
removed. Apply/prepare run under the model lock; native bounds, focus and
publication run outside it. The native-first pair is dropped on WillClose
before model access and before the adapter's WindowContext is destroyed.
Upstream's thread-confined native endpoint and post-drop invalidation apply.
Compile assertions verify native !Send/!Sync and portable Send. Native
wheel/pointer hooks, parent-dialog cancellation, diagnostics and reentrant-event
queue remain local. IME helpers scale geometry exactly once; the local code
only converts physical values to baseview's representation.

Run the custom host's headless tests with:

```sh
cargo test --locked -p kontra-native-host --lib
WGPU_BACKEND=gl LIBGL_ALWAYS_SOFTWARE=1 xvfb-run -a \
  cargo test --locked -p kontra-native-host --lib -- --ignored --test-threads=1
```

The ignored lifecycle test uses the production adapter builder and closes it
twice before show, checking provider attachment, model release and native
parent callbacks. The separate ignored surface test covers software-GL present,
surface replacement and reopening, including parent mapping order.

## Further framework reuse

ListState/variable_list can replace browser height indexing and spacers, but
requires permanent unique row IDs, retained model state and a deliberate
scroll/focus handoff. The browser accepts duplicate favorites and has custom
Left/Right and Shift+Enter actions; index keys or enabling framework keyboard
handling blindly would change behavior. This focused fix does not claim that
larger migration, an application speedup or full framework runtime parity.
