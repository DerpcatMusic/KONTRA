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
