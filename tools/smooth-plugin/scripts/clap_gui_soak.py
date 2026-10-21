#!/usr/bin/env python3
"""A CLAP gui host for SMOOTH: it drives the gui extension the way a Linux DAW
does, and measures what the plug-in answers.

    xvfb-run -a -s "-screen 0 1600x1000x24" python3 tools/smooth-plugin/scripts/clap_gui_soak.py \\
        ~/.clap/SMOOTH.clap matrix
    ... order | float --out DIR | resize --out DIR | soak [--cycles N] [--audio] [--floating]

matrix  is_api_supported per API and floating flag, preferred API, sizes, hints.
order   call orders a DAW uses: embedded and floating, destroy without hide,
        set_size before show, set_parent twice, Wayland refused, scale units.
float   a floating window: shown, resized, hidden, shown again, photographed.
resize  embedded host-driven sizes, each checked against the child window and
        photographed with `magick import` into --out.
soak    open/close the editor N times on a real X11 window; RSS, socket fds,
        threads and leaked windows. --audio runs a 512-frame process loop on
        its own thread for the whole run; --floating uses the top-level mode.

The ABI comes from clap_smoke.py (CLAP 1.x headers). Nothing is installed.
Exit status is non-zero when any check fails.
"""
import argparse
import ctypes as C
import math
import os
import pathlib
import subprocess
import sys
import threading
import time

sys.path.insert(0, str(pathlib.Path(__file__).parent))
from clap_smoke import (  # noqa: E402
    Audio,
    Entry,
    Factory,
    Gui,
    Host,
    InEvents,
    OutEvents,
    Plugin,
    Process,
    Version,
)

U32, PTR, CF = C.c_uint32, C.c_void_p, C.CFUNCTYPE
IS_VIEWABLE = 2


class ClapWindow(C.Structure):
    # clap_window: the api name, then a union whose X11 member is an unsigned long.
    _fields_ = [("api", C.c_char_p), ("x11", C.c_ulong)]


class ResizeHints(C.Structure):
    _fields_ = [
        ("horizontal", C.c_bool), ("vertical", C.c_bool), ("preserve", C.c_bool),
        ("aspect_w", U32), ("aspect_h", U32),
    ]


class HostGui(C.Structure):
    _fields_ = [
        ("resize_hints_changed", CF(None, PTR)),
        ("request_resize", CF(C.c_bool, PTR, U32, U32)),
        ("request_show", CF(C.c_bool, PTR)),
        ("request_hide", CF(C.c_bool, PTR)),
        ("closed", CF(None, PTR, C.c_bool)),
    ]


class XWindowAttributes(C.Structure):
    _fields_ = [
        ("x", C.c_int), ("y", C.c_int), ("width", C.c_int), ("height", C.c_int),
        ("border_width", C.c_int), ("depth", C.c_int), ("visual", PTR), ("root", C.c_ulong),
        ("klass", C.c_int), ("bit_gravity", C.c_int), ("win_gravity", C.c_int),
        ("backing_store", C.c_int), ("backing_planes", C.c_ulong), ("backing_pixel", C.c_ulong),
        ("save_under", C.c_int), ("colormap", C.c_ulong), ("map_installed", C.c_int),
        ("map_state", C.c_int), ("all_event_masks", C.c_long), ("your_event_mask", C.c_long),
        ("do_not_propagate_mask", C.c_long), ("override_redirect", C.c_int), ("screen", PTR),
    ]


class X:
    """The few Xlib calls a host makes: its parent window, the windows under it, sizes."""

    def __init__(self):
        x = C.CDLL("libX11.so.6")
        x.XInitThreads()
        x.XOpenDisplay.argtypes, x.XOpenDisplay.restype = [C.c_char_p], PTR
        x.XDefaultRootWindow.argtypes, x.XDefaultRootWindow.restype = [PTR], C.c_ulong
        x.XCreateSimpleWindow.argtypes = [PTR, C.c_ulong, C.c_int, C.c_int, U32, U32, U32, C.c_ulong, C.c_ulong]
        x.XCreateSimpleWindow.restype = C.c_ulong
        x.XMapWindow.argtypes = [PTR, C.c_ulong]
        x.XResizeWindow.argtypes = [PTR, C.c_ulong, U32, U32]
        x.XDestroyWindow.argtypes = [PTR, C.c_ulong]
        x.XSync.argtypes = [PTR, C.c_int]
        x.XQueryTree.argtypes = [PTR, C.c_ulong, C.POINTER(C.c_ulong), C.POINTER(C.c_ulong),
                                 C.POINTER(C.POINTER(C.c_ulong)), C.POINTER(U32)]
        x.XFree.argtypes = [PTR]
        x.XGetWindowAttributes.argtypes = [PTR, C.c_ulong, C.POINTER(XWindowAttributes)]
        self.x = x
        self.display = x.XOpenDisplay(None)
        assert self.display, "an X display is needed (use xvfb-run)"
        self.root = x.XDefaultRootWindow(self.display)
        self.parent = x.XCreateSimpleWindow(self.display, self.root, 0, 0, 800, 473, 0, 0, 0xFFFFFF)
        x.XMapWindow(self.display, self.parent)
        x.XSync(self.display, 0)

    def _children(self, window):
        root, parent = C.c_ulong(), C.c_ulong()
        kids, count = C.POINTER(C.c_ulong)(), U32()
        self.x.XQueryTree(self.display, window, C.byref(root), C.byref(parent), C.byref(kids), C.byref(count))
        found = [kids[i] for i in range(count.value)]
        if kids:
            self.x.XFree(kids)
        return found

    def embedded(self):
        """Windows the plug-in put under the host's parent window."""
        return self._children(self.parent)

    def top_level(self):
        """Windows the plug-in put on the root: floating windows."""
        return [w for w in self._children(self.root) if w != self.parent]

    def leftovers(self):
        return self.embedded() + self.top_level()

    def attributes(self, window):
        attrs = XWindowAttributes()
        self.x.XSync(self.display, 0)
        return attrs if self.x.XGetWindowAttributes(self.display, window, C.byref(attrs)) else None

    def resize_parent(self, width, height):
        self.x.XResizeWindow(self.display, self.parent, width, height)
        self.x.XSync(self.display, 0)

    def close(self):
        self.x.XDestroyWindow(self.display, self.parent)
        self.x.XSync(self.display, 0)


def rss_kb():
    with open("/proc/self/status") as status:
        return next(int(line.split()[1]) for line in status if line.startswith("VmRSS:"))


def socket_fds():
    return sum(
        1 for fd in os.listdir("/proc/self/fd")
        if os.path.islink(f"/proc/self/fd/{fd}") and os.readlink(f"/proc/self/fd/{fd}").startswith("socket:")
    )


def threads():
    return len(os.listdir("/proc/self/task"))


def shot(window, target):
    subprocess.run(["magick", "import", "-window", str(window), str(target)], check=True)
    print(f"  wrote {target}")


class Instance:
    """One plug-in instance, driven through the gui extension as a host would."""

    def __init__(self, path, x):
        self.lib = C.CDLL(str(path))
        self.x = x
        self.resize_requests = []
        self.on_request_resize = None  # (w, h) -> bool, set by the caller
        entry = Entry.in_dll(self.lib, "clap_entry")
        assert entry.init(str(path).encode())
        self.factory_ptr = entry.factory(b"clap.plugin-factory")
        factory = C.cast(self.factory_ptr, C.POINTER(Factory)).contents
        descriptor = factory.descriptor(self.factory_ptr, 0).contents
        self.host_gui = HostGui(
            CF(None, PTR)(lambda _: None),
            CF(C.c_bool, PTR, U32, U32)(self._request_resize),
            CF(C.c_bool, PTR)(lambda _: False),
            CF(C.c_bool, PTR)(lambda _: False),
            CF(None, PTR, C.c_bool)(lambda *_: None),
        )
        extension = CF(PTR, PTR, C.c_char_p)(
            lambda _, name: C.addressof(self.host_gui) if name == b"clap.gui" else None
        )
        noop = CF(None, PTR)(lambda *_: None)
        self.host = Host(Version(1, 2, 0), None, b"SMOOTH gui host", b"Derpcat", b"", b"0.1",
                         extension, noop, noop, noop)
        self.pointer = factory.create(self.factory_ptr, C.byref(self.host), descriptor.id)
        assert self.pointer
        self.plugin = C.cast(self.pointer, C.POINTER(Plugin)).contents
        assert self.plugin.init(self.pointer)
        self.gui = C.cast(self.plugin.extension(self.pointer, b"clap.gui"), C.POINTER(Gui)).contents
        # Extension slots that are plain pointers in clap_smoke, called with their real signatures.
        self.preferred = CF(C.c_bool, PTR, C.POINTER(C.c_char_p), C.POINTER(C.c_bool))(self.gui.preferred)
        self.hints = CF(C.c_bool, PTR, C.POINTER(ResizeHints))(self.gui.hints)
        self.adjust = CF(C.c_bool, PTR, C.POINTER(U32), C.POINTER(U32))(self.gui.adjust)
        self.set_parent_fn = CF(C.c_bool, PTR, C.POINTER(ClapWindow))(self.gui.parent)
        self.transient = CF(C.c_bool, PTR, C.POINTER(ClapWindow))(self.gui.transient)
        self.size_fn = CF(U32, PTR)(lambda _: 0)
        self.get_fn = CF(PTR, PTR, U32)(lambda *_: None)
        self.push_fn = CF(C.c_bool, PTR, PTR)(lambda *_: True)
        self.in_events = InEvents(None, self.size_fn, self.get_fn)
        self.out_events = OutEvents(None, self.push_fn)

    def _request_resize(self, _, width, height):
        self.resize_requests.append((width, height))
        return self.on_request_resize(width, height) if self.on_request_resize else False

    def set_parent(self, api=b"x11", window=None):
        win = ClapWindow(api, self.x.parent if window is None else window)
        return self.set_parent_fn(self.pointer, C.byref(win))

    def open_embedded(self):
        return self.gui.create(self.pointer, b"x11", False) and self.set_parent() and self.gui.show(self.pointer)

    def open_floating(self):
        self.before_show = set(self.x.top_level())
        return self.gui.create(self.pointer, b"x11", True) and self.gui.show(self.pointer)

    def size(self):
        width, height = U32(), U32()
        ok = self.gui.size(self.pointer, C.byref(width), C.byref(height))
        return (width.value, height.value) if ok else None

    def pump(self, seconds):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            self.plugin.main(self.pointer)
            time.sleep(0.02)

    def window(self, floating=False):
        """The plug-in's X window: under the host's parent, or top-level when floating.

        A floating editor also creates 1x1 helper windows on the root (its input
        method), so the window is the largest new top-level one.
        """
        if floating:
            fresh = [w for w in self.x.top_level() if w not in getattr(self, "before_show", set())]
            areas = [(a.width * a.height, w) for w in fresh if (a := self.x.attributes(w))]
            return max(areas)[1] if areas else None
        kids = self.x.embedded()
        return kids[-1] if kids else None

    def window_size(self, floating=False):
        kid = self.window(floating)
        attrs = self.x.attributes(kid) if kid else None
        return (attrs.width, attrs.height, attrs.map_state) if attrs else None

    def wait_window(self, size, floating=False, timeout=3.0):
        end = time.monotonic() + timeout
        seen = None
        while time.monotonic() < end:
            self.plugin.main(self.pointer)
            seen = self.window_size(floating)
            if seen and seen[:2] == size:
                return seen
            time.sleep(0.02)
        return seen

    def close(self):
        self.plugin.destroy(self.pointer)


class Checks:
    def __init__(self):
        self.failed = []

    def check(self, name, ok, detail=""):
        print(f"  {'PASS' if ok else 'FAIL'} {name}{(' - ' + str(detail)) if detail != '' else ''}")
        if not ok:
            self.failed.append(name)
        return ok


def matrix(path, x, checks):
    inst = Instance(path, x)
    print("is_api_supported (embedded / floating):")
    for api in [b"x11", b"wayland", b"cocoa", b"win32"]:
        print(f"  {api.decode():8} embedded={inst.gui.supported(inst.pointer, api, False)}"
              f" floating={inst.gui.supported(inst.pointer, api, True)}")
    api_ptr, floating = C.c_char_p(), C.c_bool()
    ok = inst.preferred(inst.pointer, C.byref(api_ptr), C.byref(floating))
    print("get_preferred_api:", ok, api_ptr.value, "floating" if floating.value else "embedded")
    checks.check("x11 offered embedded", inst.gui.supported(inst.pointer, b"x11", False))
    checks.check("x11 offered floating on Linux", inst.gui.supported(inst.pointer, b"x11", True))
    checks.check("wayland refused, embedded and floating",
                 not inst.gui.supported(inst.pointer, b"wayland", False)
                 and not inst.gui.supported(inst.pointer, b"wayland", True))
    checks.check("no size before create", inst.size() is None, inst.size())
    checks.check("create(wayland) refused", not inst.gui.create(inst.pointer, b"wayland", False))
    checks.check("create(wayland, floating) refused", not inst.gui.create(inst.pointer, b"wayland", True))
    checks.check("create(x11, floating) accepted", inst.gui.create(inst.pointer, b"x11", True))
    print("floating after create: size", inst.size(), "can_resize", inst.gui.can_resize(inst.pointer))
    inst.gui.destroy(inst.pointer)
    checks.check("destroy after floating create leaves nothing", not x.leftovers())
    inst.close()

    inst = Instance(path, x)
    checks.check("create(x11) embedded", inst.gui.create(inst.pointer, b"x11", False))
    print("embedded after create: size", inst.size(), "can_resize", inst.gui.can_resize(inst.pointer))
    checks.check("size is 800x473 with no host scale and no window", inst.size() == (800, 473), inst.size())
    hints = ResizeHints()
    checks.check("resize hints reported", inst.hints(inst.pointer, C.byref(hints)))
    print(f"  hints horizontal={hints.horizontal} vertical={hints.vertical} preserve={hints.preserve}")
    clamp_w, clamp_h = U32(1000), U32(300)
    print("adjust_size(1000,300):", inst.adjust(inst.pointer, C.byref(clamp_w), C.byref(clamp_h)),
          clamp_w.value, clamp_h.value)
    checks.check("resize hints match can_resize", bool(hints.horizontal) == inst.gui.can_resize(inst.pointer))
    inst.gui.destroy(inst.pointer)
    inst.close()


def order(path, x, checks):
    print("embedded: create, set_parent, show, hide, destroy")
    inst = Instance(path, x)
    inst.gui.scale(inst.pointer, 1.0)
    checks.check("create", inst.gui.create(inst.pointer, b"x11", False))
    size = inst.size()
    x.resize_parent(*size)
    checks.check("set_parent", inst.set_parent())
    checks.check("show", inst.gui.show(inst.pointer))
    seen = inst.wait_window(size)
    checks.check(f"child window {size[0]}x{size[1]} viewable", seen and seen[:2] == size and seen[2] == IS_VIEWABLE, seen)
    checks.check("hide", inst.gui.hide(inst.pointer))
    inst.gui.destroy(inst.pointer)
    checks.check("destroy leaves no window", not x.leftovers(), x.leftovers())
    inst.close()

    print("destroy without hide")
    inst = Instance(path, x)
    inst.gui.scale(inst.pointer, 1.0)
    inst.gui.create(inst.pointer, b"x11", False)
    inst.set_parent()
    inst.gui.show(inst.pointer)
    inst.pump(0.3)
    inst.gui.destroy(inst.pointer)
    checks.check("no window left after destroy without hide", not x.leftovers())
    inst.close()

    print("set_size between set_parent and show")
    inst = Instance(path, x)
    inst.gui.scale(inst.pointer, 1.0)
    inst.gui.create(inst.pointer, b"x11", False)
    inst.set_parent()
    x.resize_parent(640, 400)
    checks.check("set_size 640x400 accepted", inst.gui.set_size(inst.pointer, 640, 400))
    checks.check("size reports 640x400", inst.size() == (640, 400), inst.size())
    inst.gui.show(inst.pointer)
    seen = inst.wait_window((640, 400))
    checks.check("child follows set_size to 640x400", seen and seen[:2] == (640, 400), seen)
    inst.gui.destroy(inst.pointer)
    checks.check("no window left after resized destroy", not x.leftovers())
    inst.close()

    print("set_size below the minimum is clamped, and the host is asked to adopt it")
    inst = Instance(path, x)
    inst.gui.scale(inst.pointer, 1.0)
    inst.gui.create(inst.pointer, b"x11", False)
    inst.set_parent()
    inst.on_request_resize = lambda w, h: (x.resize_parent(w, h), inst.gui.set_size(inst.pointer, w, h))[1]
    x.resize_parent(800, 473)
    inst.gui.show(inst.pointer)
    # The first frame asks the host for the plug-in's own preferred size; let it land first.
    inst.pump(0.5)
    checks.check("set_size 300x200 answered true", inst.gui.set_size(inst.pointer, 300, 200))
    checks.check("plug-in asked the host for its fitted size", (396, 234) in inst.resize_requests,
                 inst.resize_requests)
    checks.check("size is the minimum 396x234", inst.size() == (396, 234), inst.size())
    seen = inst.wait_window((396, 234))
    checks.check("child window is 396x234", seen and seen[:2] == (396, 234), seen)
    inst.gui.destroy(inst.pointer)
    inst.close()

    print("set_parent called twice without destroy")
    inst = Instance(path, x)
    inst.gui.scale(inst.pointer, 1.0)
    inst.gui.create(inst.pointer, b"x11", False)
    checks.check("first set_parent", inst.set_parent())
    inst.gui.show(inst.pointer)
    inst.pump(0.2)
    checks.check("second set_parent", inst.set_parent())
    inst.pump(0.2)
    checks.check("one embedded window after second set_parent", len(x.embedded()) == 1, len(x.embedded()))
    inst.gui.destroy(inst.pointer)
    checks.check("no window left after destroy", not x.leftovers())
    inst.close()

    print("Wayland refused; floating works, and set_parent is refused for it")
    inst = Instance(path, x)
    inst.before_show = set(x.top_level())
    checks.check("create(wayland) refused", not inst.gui.create(inst.pointer, b"wayland", False))
    checks.check("set_parent(wayland) refused", not inst.set_parent(api=b"wayland"))
    checks.check("create(x11, floating) accepted", inst.gui.create(inst.pointer, b"x11", True))
    checks.check("set_parent refused for a floating GUI", not inst.set_parent())
    checks.check("set_parent with a null window refused", not inst.set_parent(window=0))
    checks.check("show opens a top-level window", inst.gui.show(inst.pointer))
    seen = inst.wait_window(inst.size(), floating=True)
    checks.check("top-level window is viewable", seen and seen[2] == IS_VIEWABLE, seen)
    checks.check("hide closes it", inst.gui.hide(inst.pointer) and not x.top_level(), x.top_level())
    checks.check("show reopens it", inst.gui.show(inst.pointer))
    seen = inst.wait_window(inst.size(), floating=True)
    checks.check("reopened window is viewable", seen and seen[2] == IS_VIEWABLE, seen)
    inst.gui.destroy(inst.pointer)
    checks.check("destroy leaves no window", not x.leftovers(), x.leftovers())
    inst.close()

    print("embedded into a parent that is not a window")
    inst = Instance(path, x)
    inst.gui.scale(inst.pointer, 1.0)
    inst.gui.create(inst.pointer, b"x11", False)
    gone = x.parent + 0x400000  # an X id that names no window on this server
    checks.check("set_parent to a window that does not exist refused", not inst.set_parent(window=gone))
    checks.check("no window opened under a missing parent", not x.leftovers(), x.leftovers())
    inst.gui.destroy(inst.pointer)
    inst.close()

    print("scale: host scale 1.5 before create gives physical pixels")
    inst = Instance(path, x)
    checks.check("set_scale(1.5)", inst.gui.scale(inst.pointer, 1.5))
    inst.gui.create(inst.pointer, b"x11", False)
    checks.check("size is physical 1200x710", inst.size() == (1200, 710), inst.size())
    x.resize_parent(1200, 710)
    inst.set_parent()
    inst.gui.show(inst.pointer)
    seen = inst.wait_window((1200, 710))
    checks.check("child is 1200x710 physical", seen and seen[:2] == (1200, 710), seen)
    inst.gui.destroy(inst.pointer)
    inst.close()


def floating(path, x, out, checks):
    """A floating window the way a host with no embedding sees it."""
    out.mkdir(parents=True, exist_ok=True)
    inst = Instance(path, x)
    inst.gui.scale(inst.pointer, 1.0)
    inst.before_show = set(x.top_level())
    checks.check("create(x11, floating)", inst.gui.create(inst.pointer, b"x11", True))
    checks.check("show", inst.gui.show(inst.pointer))
    seen = inst.wait_window((800, 473), floating=True)
    checks.check("top-level window 800x473 viewable", seen and seen[:2] == (800, 473) and seen[2] == IS_VIEWABLE, seen)
    inst.pump(0.5)
    shot(inst.window(floating=True), out / "floating-800x473.png")
    for width, height in [(640, 400), (1280, 800), (800, 473)]:
        ok = inst.gui.set_size(inst.pointer, width, height)
        seen = inst.wait_window((width, height), floating=True)
        print(f"resize floating to {width}x{height}: set_size={ok} plugin size={inst.size()} window={seen}")
        checks.check(f"floating set_size {width}x{height} honoured", ok and seen and seen[:2] == (width, height), seen)
        inst.pump(0.5)
        shot(inst.window(floating=True), out / f"floating-{width}x{height}.png")
    checks.check("hide", inst.gui.hide(inst.pointer) and not x.top_level())
    checks.check("show again", inst.gui.show(inst.pointer) and inst.wait_window((800, 473), floating=True))
    inst.gui.destroy(inst.pointer)
    checks.check("destroy leaves no window", not x.leftovers(), x.leftovers())
    inst.close()


def resize(path, x, out, checks):
    out.mkdir(parents=True, exist_ok=True)
    inst = Instance(path, x)
    inst.gui.scale(inst.pointer, 1.0)
    inst.gui.create(inst.pointer, b"x11", False)
    inst.set_parent()

    def host_resize(width, height):
        # Honour the plug-in's request: resize the parent, then tell the plug-in.
        x.resize_parent(width, height)
        return inst.gui.set_size(inst.pointer, width, height)

    inst.on_request_resize = host_resize
    x.resize_parent(800, 473)
    inst.gui.show(inst.pointer)
    inst.pump(0.5)
    checks.check("plug-in request_resize at first frame", bool(inst.resize_requests), inst.resize_requests)
    for width, height in [(800, 473), (640, 400), (1280, 800), (800, 473)]:
        ok = host_resize(width, height)
        seen = inst.wait_window((width, height))
        size = inst.size()
        print(f"resize to {width}x{height}: set_size={ok} plugin size={size} child={seen}")
        checks.check(f"set_size {width}x{height} honoured", ok and size == (width, height))
        checks.check(f"child window {width}x{height}", seen and seen[:2] == (width, height), seen)
        inst.pump(0.5)
        shot(x.parent, out / f"resize-{width}x{height}.png")
    checks.check("set_size below minimum clamped to 396x234", inst.gui.set_size(inst.pointer, 300, 200)
                 and inst.size() == (396, 234), inst.size())
    inst.gui.destroy(inst.pointer)
    checks.check("no window left after resize run", not x.leftovers())
    inst.close()


def soak(path, x, cycles, audio, floating_mode, checks, phase="full", trim=False):
    libc = C.CDLL("libc.so.6")
    inst = Instance(path, x)
    inst.gui.scale(inst.pointer, 1.0)
    if audio:
        inst.plugin.activate(inst.pointer, 48000.0, 1, 512)
        inst.plugin.start(inst.pointer)
    per_cycle_ms = []
    rss, fds, threads_seen, marks = [], [], [], []

    def sample(done):
        marks.append(done)
        rss.append(rss_kb())
        fds.append(socket_fds())
        threads_seen.append(threads())

    def cycle():
        t0 = time.perf_counter()
        if phase == "build":
            # The editor built and dropped with no window: isolates the editor's own state.
            ok = inst.gui.create(inst.pointer, b"x11", False)
            inst.gui.destroy(inst.pointer)
            per_cycle_ms.append((time.perf_counter() - t0) * 1000.0)
            return ok
        if floating_mode:
            ok = inst.open_floating()
        else:
            ok = inst.open_embedded()
        inst.pump(0.1)
        ok = ok and inst.gui.hide(inst.pointer)
        inst.gui.destroy(inst.pointer)
        if trim:
            libc.malloc_trim(0)
        per_cycle_ms.append((time.perf_counter() - t0) * 1000.0)
        return ok and not x.leftovers()

    context = AudioThread(inst) if audio else None
    if context:
        context.__enter__()
    sample(0)
    baseline_fds, baseline_threads = fds[-1], threads_seen[-1]
    clean = True
    for i in range(cycles):
        if not cycle():
            clean = False
            print(f"  cycle {i + 1}: a call failed or a window was left behind")
            break
        sample(i + 1)
    done = len(per_cycle_ms)
    sample(done)
    if context:
        context.__exit__()
        inst.plugin.stop(inst.pointer)
        inst.plugin.deactivate(inst.pointer)
    inst.close()
    slope = 0.0
    if done > 1:
        mean_x = sum(marks) / len(marks)
        mean_y = sum(rss) / len(rss)
        var = sum((a - mean_x) ** 2 for a in marks) or 1.0
        slope = sum((a - mean_x) * (b - mean_y) for a, b in zip(marks, rss)) / var
    after20 = rss[marks.index(20)] if 20 in marks else "n/a"
    mode = "floating" if floating_mode else "embedded"
    if phase == "build":
        mode = "build-only"
    print(f"soak: {mode} cycles={done} audio={audio} clean={clean}")
    print("  series cycle:rss_kB:fds:threads " + " ".join(f"{m}:{r}:{f}:{t}" for m, r, f, t in zip(marks, rss, fds, threads_seen)))
    print(f"  cycle ms: first={per_cycle_ms[0]:.0f} last={per_cycle_ms[-1]:.0f} mean={sum(per_cycle_ms)/done:.0f}")
    print(f"  RSS kB: start={rss[0]} after20={after20} end={rss[-1]} slope={slope:.1f} kB/cycle")
    print(f"  socket fds: baseline={baseline_fds} end={fds[-1]}  threads: baseline={baseline_threads} end={threads_seen[-1]}")
    if context:
        print(f"  audio process calls={context.calls} refused={context.refused}")
        checks.check("audio thread never refused", context.refused == 0, context.refused)
    checks.check("every cycle clean (no failed call, no leftover window)", clean and done == cycles)
    # The first open creates a process-lifetime connection; after it, fds must not grow per cycle.
    after_first = fds[marks.index(1)] if 1 in marks else fds[-1]
    checks.check("socket fds constant after the first open", fds[-1] <= after_first,
                 f"baseline {baseline_fds}, after first open {after_first}, end {fds[-1]}")
    checks.check("threads back to baseline", threads_seen[-1] <= baseline_threads, f"{baseline_threads}->{threads_seen[-1]}")


class AudioThread:
    """A 512-frame process loop on its own thread, as a DAW's audio thread would run it."""

    def __init__(self, inst, frames=512):
        self.inst, self.frames = inst, frames
        self.stop = threading.Event()
        self.calls = 0
        self.refused = 0
        self.arrays = [(C.c_float * frames)() for _ in range(4)]
        for i in range(frames):
            self.arrays[0][i] = self.arrays[1][i] = 0.1 * math.sin(2 * math.pi * 440.0 * i / 48000.0)
        inputs = (C.POINTER(C.c_float) * 2)(self.arrays[0], self.arrays[1])
        outputs = (C.POINTER(C.c_float) * 2)(self.arrays[2], self.arrays[3])
        self.audio_in = Audio(inputs, None, 2, 0, 0)
        self.audio_out = Audio(outputs, None, 2, 0, 0)
        self.thread = threading.Thread(target=self.run, name="audio")

    def run(self):
        inst = self.inst
        while not self.stop.is_set():
            process = Process(
                self.calls * self.frames, self.frames, None, C.pointer(self.audio_in),
                C.pointer(self.audio_out), 1, 1, C.pointer(inst.in_events), C.pointer(inst.out_events),
            )
            if inst.plugin.process(inst.pointer, C.byref(process)) == 0:
                self.refused += 1
            self.calls += 1

    def __enter__(self):
        self.thread.start()
        return self

    def __exit__(self, *_):
        self.stop.set()
        self.thread.join()


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("bundle")
    parser.add_argument("mode", choices=["matrix", "order", "float", "resize", "soak"])
    parser.add_argument("--cycles", type=int, default=200)
    parser.add_argument("--audio", action="store_true")
    parser.add_argument("--floating", action="store_true")
    parser.add_argument("--phase", choices=["full", "build"], default="full")
    parser.add_argument("--trim", action="store_true", help="malloc_trim(0) after each cycle (experiment)")
    parser.add_argument("--out", type=pathlib.Path, default=None)
    args = parser.parse_args()
    path = pathlib.Path(args.bundle).resolve()
    x = X()
    checks = Checks()
    try:
        if args.mode == "matrix":
            matrix(path, x, checks)
        elif args.mode == "order":
            order(path, x, checks)
        elif args.mode == "float":
            assert args.out, "float needs --out"
            floating(path, x, args.out, checks)
        elif args.mode == "resize":
            assert args.out, "resize needs --out"
            resize(path, x, args.out, checks)
        else:
            soak(path, x, args.cycles, args.audio, args.floating, checks, args.phase, args.trim)
    finally:
        x.close()
    print("FAILED: " + ", ".join(checks.failed) if checks.failed else "all checks passed")
    return 1 if checks.failed else 0


if __name__ == "__main__":
    sys.exit(main())
