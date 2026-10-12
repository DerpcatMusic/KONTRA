# KONTRA local native-window preservation

Base: MUI `1f1daadb0e77c16f30f394255121e7c2fb14b1f2` vendored
`moose-baseview`. Its unmodified `zed-xim` IME dependency comes from the same
MUI Git source pinned by the application lock, including its parser and text
packages; no local IME source copy is needed.

Preserves KONTRA `c96ad3a` root-reparent visibility behavior while consuming
the newer MUI display/accessibility integration:

- `remove_after_window` retains the reparented window, removes its old parent.
- Reparenting to the X11 root refreshes mapping/visibility before returning.

The original native X11 regression is retained in `tests/x11_visibility.rs` and
mirrored into the custom host integration tests so CI uses the application lock.
Run against an isolated server:

```sh
xvfb-run -a cargo test --locked -p kontra-native-host \
  --test x11_visibility -- --ignored --test-threads=1
```

The application MUI source patch consumes this package. New MUI parent-before/
after-map pixel tests cover a different transition and must also pass before
deployment. This local preservation does not claim complete DAW validation.

## Nested ancestor map discovery race

On Linux, the cached ancestor map state gates native `on_frame` and the first
GPU initialization. In a nested root → outer → parent → editor tree, mapping
an ancestor between its state snapshot and subscription on its own parent
could lose `MapNotify` and suppress a server-viewable editor indefinitely.

Foreign ancestors now select both `StructureNotify` and `SubstructureNotify`
before their state/tree snapshot. The editor's own event mask and the root
subscription are preserved. Existing root-reparent repair, subscription-error
fallback, and native late-map/reparent tests remain in place.

The ignored library regression
`map_between_ancestor_snapshot_and_parent_subscription_is_not_lost` uses a
second X11 connection and a test-only seam in the actual discovery function
to force that map ordering. It checks server viewability, cached viewability,
and preservation of the editor's input mask. Run it on a private X11 server,
alongside the existing native integration tests. A controlled original/fixed
run reproduced `VIEWABLE` with cached false before and cached true afterward;
the dependency lock and stimulus were identical. This proves the discovery
race and its fix, but does not establish the cause of a particular DAW session.
