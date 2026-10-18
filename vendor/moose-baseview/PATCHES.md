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

## Windows first expose (0.3.344 follow-up)

The Windows message dispatcher previously omitted `WM_PAINT` and
`WM_SHOWWINDOW`. It now consumes the update region with `BeginPaint`/`EndPaint`
and sends `RedrawRequested`; showing a window also sends that event. The
KONTRA native host invalidates its GPU cache for that event and for explicit
full-redraw/resize requests. This gives a freshly shown/restored embedded editor
a complete frame even when its model has not changed.

## macOS open-state query

`WindowHandle::is_open` returns the inverse of the `closed` flag. That flag starts
false and becomes true when the view closes or deallocates, so the query now
matches the public `Window::is_open` contract. This one-line correction was
verified against those lifecycle writes; it has not been exercised on AppKit in
this Linux checkout.

## Linux editor controller ownership

The additive `ThreadedWindow::create` path accepts no host callbacks and returns
an auto-`Send` X11 worker controller. Native `WindowContext`, handler, and drawable
ownership remain on the existing worker. The original `Window` and its optional
owner-thread `HostCallbacks` remain `!Send`; both paths share request/join logic.
KONTRA's Linux MUI editor uses the no-host controller without an unsafe `Send`
assertion. Windows/macOS still use their legacy editor handle and are separate
ownership work.

Control-thread close joins even if the request channel has disconnected. Dropping
the controller inside its own native callback requests close and defers teardown
until that callback returns, avoiding self-join. Embedded close never adopts the
floating-window timeout/detach policy. Each controller has its own worker/channel
and atomic metadata, so an old close cannot address a reopened worker.

## X11 temporary grabs (issue #77)

The editor retains logical focus through `NotifyGrab`/`NotifyUngrab`. These
keyboard events redirect input temporarily without changing `XGetInputFocus`;
forwarding them as deactivation cleared MUI's held pointer and cancelled the
click or drag. Real `NotifyNormal` and `NotifyWhileGrabbed` focus changes still
clear held state and cancel gestures. Pointer-grab activation's pseudo crossing
is ignored; normal and ungrab crossings still reconcile the pointer.

`kontra-native-host`'s ignored
`native_temporary_grabs_preserve_gestures_but_real_focus_loss_cancels` test uses
actual keyboard/pointer grabs on an isolated X server and the production MUI
input adapter, without a GPU. It fails at the held-gesture assertion on the
original loop and passes with this patch. It also checks genuine deactivation
(normal and while grabbed) and physical pointer exit. The existing native host
CI step already runs it under Xvfb. This proves the X11 protocol correction;
Fedora/GNOME/Wayland with the reporting Bitwig host still needs confirmation.

Protocol: [Xlib sections 10.6.2 and 10.7](https://www.x.org/releases/X11R7.5/doc/libX11/libX11.html).
