# KONTRA local native-window preservation

Base: MUI `822b1922ad1872aaee15b371a31410b0f3580b88` vendored
`moose-baseview`, with its unchanged local `../xim-rs` IME dependencies.

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

macOS retains MUI's owned-loop shutdown and view-class lifetime fixes:

- Only a view owning `NSApplication.run()` stops that loop, posting an
  application-defined event so a display-link/timer close returns from `run()`.
  Embedded child views leave the host's application loop alone.
- One UUID-named Objective-C view class is registered per Rust implementation
  and loaded baseview image. Registered classes outlive AccessKit's cached
  subclasses; view instances and their Rust state still deallocate normally.
- Deallocation explicitly dispatches to NSView, including externally subclassed
  views, rather than disposing a superclass beneath a cached native subclass.

The native editor smoke fixture validates two child close/reopen cycles and
owned-loop termination. The independent X11 root-reparent patch is preserved.

Native Expose now emits `WindowEvent::RedrawRequested`, matching the shared MUI
host contract. This invalidates the retained CPU presenter without changing the
independent root-reparent visibility patch.

X11 server destruction invalidates the owned drawable, stops frame/resize
callbacks, and terminates the editor thread while delivering `WillClose` once.
An empty ancestry is never visible, including after the host deletes its parent;
floating windows also clear their visibility on destruction. The drawable's
destructor does not destroy an already deleted server resource. KONTRA's root
reparenting behavior remains intact.
