# KONTRA local native-window preservation

Base: MUI `822b1922ad1872aaee15b371a31410b0f3580b88` vendored
`moose-baseview`, with its unchanged local `../xim-rs` IME dependencies.

Preserves KONTRA `c96ad3a` root-reparent visibility behavior while consuming
the newer MUI display/accessibility integration:

- `remove_after_window` retains the reparented window, removes its old parent.
- Reparenting to the X11 root refreshes mapping/visibility before returning.

The original native X11 regression is retained in `tests/x11_visibility.rs`.
Run against an isolated server:

```sh
xvfb-run -a cargo test --manifest-path vendor/moose-baseview/Cargo.toml \
  --test x11_visibility -- --ignored --test-threads=1
```

The application MUI source patch consumes this package. New MUI parent-before/
after-map pixel tests cover a different transition and must also pass before
deployment. This local preservation does not claim complete DAW validation.
