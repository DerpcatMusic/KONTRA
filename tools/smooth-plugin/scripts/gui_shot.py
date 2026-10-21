#!/usr/bin/env python3
"""Open SMOOTH's editor in a real host window and photograph it.

The plug-in is loaded through CLAP, given an X11 parent, shown, and captured.
The picture is what a musician sees, so it is the honest check that the
interface draws.

    xvfb-run -a python3 tools/smooth-plugin/scripts/gui_shot.py ~/.clap/SMOOTH.clap out.png
    xvfb-run -s "-screen 0 1600x1000x24" python3 tools/smooth-plugin/scripts/gui_shot.py \\
        ~/.clap/SMOOTH.clap out.png --drag --gesture-check

`--drag` moves the first band's handle and photographs the result.
`--wheel-check` wheels band 2 up, Alt-wheels its depth and Shift-wheels it
fine, drags it, double-clicks it, asserting the host hears one gesture for
each and photographing each stage. `--keys` adds the arrow and Delete checks,
which do not yet reach the editor under Xvfb.
`--gesture-check` then runs a block of audio and asserts the host heard the
drag: one begin, the values, one end, per parameter. A handle is not a
widget, so that is the only way the gesture can reach a host.

Give Xvfb a screen at least as large as the window (1200x710): the pointer
cannot leave the screen, and `magick import` only sees what is on it.
"""
import ctypes as C
import pathlib
import random
import subprocess
import sys
import threading
import time

U32, U64, I32, F64, PTR = C.c_uint32, C.c_uint64, C.c_int32, C.c_double, C.c_void_p

sys.path.insert(0, str(pathlib.Path(__file__).parent))
from clap_smoke import (  # noqa: E402
    Audio,
    Descriptor,
    Entry,
    Factory,
    Gui,
    Host,
    InEvents,
    OutEvents,
    ParamEvent,
    Header,
    Params,
    ParamInfo,
    Plugin,
    Process,
    Version,
    plain,
)


class Window(C.Structure):
    _fields_ = [("api", C.c_char_p), ("x11", C.c_ulong)]


def main():
    path = pathlib.Path(sys.argv[1]).resolve()
    out = pathlib.Path(sys.argv[2] if len(sys.argv) > 2 else "smooth-gui.png")
    lib = C.CDLL(str(path))
    entry = Entry.in_dll(lib, "clap_entry")
    assert entry.init(str(path).encode())
    factory_ptr = entry.factory(b"clap.plugin-factory")
    factory = C.cast(factory_ptr, C.POINTER(Factory)).contents
    descriptor = factory.descriptor(factory_ptr, 0).contents
    # The host's GUI extension (CLAP names the host side `clap.gui` too): a plug-in that changes its size asks the
    # host to resize the window it gave it, as a DAW does. The request is
    # answered by resizing the X parent, which the screenshot then shows.
    hosted = {"parent": None, "display": None, "resizes": []}

    def request_resize(_, width, height):
        if not hosted["parent"]:
            return False
        hosted["resizes"].append((width, height))
        hosted["x11"].XResizeWindow(hosted["display"], hosted["parent"], width, height)
        hosted["x11"].XFlush(hosted["display"])
        return True

    class HostGui(C.Structure):
        _fields_ = [
            ("resize_hints_changed", C.CFUNCTYPE(None, PTR)),
            ("request_resize", C.CFUNCTYPE(C.c_bool, PTR, U32, U32)),
            ("request_show", C.CFUNCTYPE(C.c_bool, PTR)),
            ("request_hide", C.CFUNCTYPE(C.c_bool, PTR)),
            ("closed", C.CFUNCTYPE(None, PTR, C.c_bool)),
        ]

    host_gui = HostGui(
        C.CFUNCTYPE(None, PTR)(lambda _: None),
        C.CFUNCTYPE(C.c_bool, PTR, U32, U32)(request_resize),
        C.CFUNCTYPE(C.c_bool, PTR)(lambda _: False),
        C.CFUNCTYPE(C.c_bool, PTR)(lambda _: False),
        C.CFUNCTYPE(None, PTR, C.c_bool)(lambda *_: None),
    )
    extension = C.CFUNCTYPE(PTR, PTR, C.c_char_p)(
        lambda _, name: C.addressof(host_gui) if name == b"clap.gui" else None
    )
    callbacks = [extension]
    callbacks += [C.CFUNCTYPE(None, PTR)(lambda *_: None) for _ in range(3)]
    host = Host(
        Version(1, 2, 0), None, b"SMOOTH screenshot host", b"Derpcat", b"", b"0.1", *callbacks
    )
    pointer = factory.create(factory_ptr, C.byref(host), descriptor.id)
    assert pointer
    plugin = C.cast(pointer, C.POINTER(Plugin)).contents
    assert plugin.init(pointer)
    params = C.cast(plugin.extension(pointer, b"clap.params"), C.POINTER(Params)).contents

    infos = {}
    for index in range(params.count(pointer)):
        info = ParamInfo()
        assert params.info(pointer, index, C.byref(info))
        infos[info.name.decode()] = info
    pending = []
    size_fn = C.CFUNCTYPE(U32, PTR)(lambda _: len(pending))
    get_fn = C.CFUNCTYPE(PTR, PTR, U32)(lambda _, index: C.addressof(pending[index]))
    push_fn = C.CFUNCTYPE(C.c_bool, PTR, PTR)(lambda *_: True)
    in_events = InEvents(None, size_fn, get_fn)
    out_events = OutEvents(None, push_fn)

    # What the host hears back. `process` flushes the editor's queued param
    # changes onto its output events, so a drag reaches the host only through
    # a block of audio: the recorder is the check that it does.
    heard = []

    def record(_, event):
        header = C.cast(event, C.POINTER(Header)).contents
        if header.type in (5, 7, 8):
            param = C.cast(event, C.POINTER(ParamEvent)).contents
            value = param.value if header.type == 5 else None
            heard.append((header.type, param.id, value))
        return True

    record_fn = C.CFUNCTYPE(C.c_bool, PTR, PTR)(record)
    record_out = OutEvents(None, record_fn)

    def set_values(values):
        pending[:] = [
            ParamEvent(
                Header(C.sizeof(ParamEvent), 0, 0, 5, 0), infos[name].id, None, -1, -1, -1, -1,
                plain(name, value),
            )
            for name, value in values.items()
        ]
        params.flush(pointer, C.byref(in_events), C.byref(out_events))
        pending.clear()

    # The reference screenshot's state, so the picture can be compared with it.
    set_values({"Depth": 6.0, "Detail": 6.3, "Attack": 5.0, "Release": 3.0, "Mix": 100.0})

    gui_ptr = plugin.extension(pointer, b"clap.gui")
    assert gui_ptr
    gui = C.cast(gui_ptr, C.POINTER(Gui)).contents
    assert gui.supported(pointer, b"x11", False)

    x11 = C.CDLL("libX11.so.6")
    x11.XInitThreads()
    x11.XOpenDisplay.argtypes = [C.c_char_p]
    x11.XOpenDisplay.restype = PTR
    x11.XDefaultRootWindow.argtypes = [PTR]
    x11.XDefaultRootWindow.restype = C.c_ulong
    x11.XCreateSimpleWindow.argtypes = [PTR, C.c_ulong, C.c_int, C.c_int, U32, U32, U32, C.c_ulong, C.c_ulong]
    x11.XCreateSimpleWindow.restype = C.c_ulong
    for name in ["XMapWindow", "XDestroyWindow", "XFlush"]:
        getattr(x11, name).argtypes = [PTR] if name == "XFlush" else [PTR, C.c_ulong]
    x11.XResizeWindow.argtypes = [PTR, C.c_ulong, U32, U32]
    x11.XCloseDisplay.argtypes = [PTR]

    display = x11.XOpenDisplay(None)
    assert display, "an X display is needed"
    parent = x11.XCreateSimpleWindow(display, x11.XDefaultRootWindow(display), 0, 0, 1200, 710, 0, 0, 0xFFFFFF)
    x11.XMapWindow(display, parent)
    x11.XFlush(display)
    hosted.update(parent=parent, display=display, x11=x11)
    set_parent = C.CFUNCTYPE(C.c_bool, PTR, C.POINTER(Window))(gui.parent)
    assert gui.create(pointer, b"x11", False)
    assert set_parent(pointer, C.byref(Window(b"x11", parent)))
    assert gui.show(pointer)

    def settle():
        for _ in range(60):
            plugin.main(pointer)
            time.sleep(0.05)

    def play(frames=88200, frequency=1000.0, events=None, realtime=False, shoot=None):
        """Run the engine over a tone, so the plot has something to show.

        `events` is the output-event sink: give it the recorder and the block
        reports what the editor told the host.
        """
        import math

        print("activate:", plugin.activate(pointer, 44100.0, 1, 256))
        print("start:", plugin.start(pointer))
        arrays = [(C.c_float * 256)() for _ in range(4)]
        inputs = (C.POINTER(C.c_float) * 2)(arrays[0], arrays[1])
        outputs = (C.POINTER(C.c_float) * 2)(arrays[2], arrays[3])
        audio_in = Audio(inputs, None, 2, 0, 0)
        audio_out = Audio(outputs, None, 2, 0, 0)
        for start in range(0, frames, 256):
            for i in range(256):
                index = start + i
                # A voice-like signal: a 220 Hz source with a strong formant
                # near 3 kHz and a ring at 7 kHz, over a little noise, so the
                # plot has resonances to find.
                t = index / 44100.0
                sample = 0.0
                for k in range(1, 60):
                    f = 220.0 * k
                    if f > 18000.0:
                        break
                    gain = 1.0 / k + 0.9 * math.exp(-((f - 3000.0) / 500.0) ** 2) + 0.5 * math.exp(-((f - 7000.0) / 250.0) ** 2)
                    sample += gain * math.sin(2 * math.pi * f * t)
                sample = sample * 0.12 + (random.random() - 0.5) * 0.01
                arrays[0][i] = arrays[1][i] = sample
            process = Process(
                start, 256, None, C.pointer(audio_in), C.pointer(audio_out), 1, 1,
                C.pointer(in_events), C.pointer(events or out_events),
            )
            status = plugin.process(pointer, C.byref(process))
            if status == 0:
                print("process refused")
                break
            if realtime:
                # A host calls at the buffer rate; the editor's loop runs between.
                time.sleep(256 / 44100.0)
                if (start // 256) % 4 == 0:
                    plugin.main(pointer)
                if shoot and start >= frames - 256 * 40:
                    shoot()
                    shoot = None
        plugin.stop(pointer)

    settle()
    subprocess.run(["magick", "import", "-window", str(parent), str(out)], check=True)
    print(f"wrote {out}")
    print(f"host resizes: {hosted['resizes']}")

    def scale():
        """The display's scale for the editor's own 800x473 space.

        The window is opened larger than the editor's logical size, so the
        geometry the server actually gave us -- not the size we asked for --
        sets the pointer's pixels.
        """
        import re

        geometry = subprocess.run(
            ["xdotool", "getwindowgeometry", "--shell", str(parent)],
            capture_output=True,
            text=True,
            check=True,
        ).stdout
        width = int(re.search(r"WIDTH=(\d+)", geometry).group(1))
        height = int(re.search(r"HEIGHT=(\d+)", geometry).group(1))
        print(f"window {width}x{height}, scale {width / 800.0:.2f}")
        return width / 800.0, height / 473.0

    if "--live" in sys.argv:
        # Play a tone through the engine, then photograph the plot again: the
        # reduction curve is the engine's own.
        live = out.with_name(out.stem + "-live" + out.suffix)

        def shoot():
            subprocess.run(["magick", "import", "-window", str(parent), str(live)], check=True)
            print(f"wrote {live}")

        play(realtime=True, shoot=shoot)

    if "--click" in sys.argv:
        # Click logical points in the editor's own 800x473 space: `--click
        # x,y x,y`. Each click is photographed, so what the click did is
        # visible rather than asserted.
        import re

        sx, sy = scale()
        rest = sys.argv[sys.argv.index("--click") + 1 :]
        for index, spec in enumerate(a for a in rest if re.fullmatch(r"\d+,\d+", a)):
            x, y = (int(v) for v in spec.split(","))
            point = (int(x * sx), int(y * sy))
            subprocess.run(["xdotool", "mousemove", *map(str, point)], check=True)
            settle()
            subprocess.run(["xdotool", "click", "1"], check=True)
            settle()
            shot = out.with_name(f"{out.stem}-click{index + 1}{out.suffix}")
            subprocess.run(["magick", "import", "-window", str(parent), str(shot)], check=True)
            print(f"wrote {shot}")

    if "--drag" in sys.argv:
        # Take a handle and move it: the first band sits at 100 Hz, on the
        # zero line, a quarter of the way across the plot. The pointer works
        # in the display's pixels, so the editor's points are scaled by the
        # window the server gave us.
        sx, sy = scale()
        start = (int(230 * sx), int(225 * sy))
        end = (int(330 * sx), int(300 * sy))
        subprocess.run(["xdotool", "mousemove", str(start[0]), str(start[1])], check=True)
        settle()
        subprocess.run(["xdotool", "mousedown", "1"], check=True)
        for step in range(1, 11):
            x = start[0] + (end[0] - start[0]) * step // 10
            y = start[1] + (end[1] - start[1]) * step // 10
            subprocess.run(["xdotool", "mousemove", str(x), str(y)], check=True)
            settle()
        subprocess.run(["xdotool", "mouseup", "1"], check=True)
        settle()
        dragged = out.with_name(out.stem + "-dragged" + out.suffix)
        subprocess.run(["magick", "import", "-window", str(parent), str(dragged)], check=True)
        print(f"wrote {dragged}")

        if "--gesture-check" in sys.argv:
            # A handle is not a widget, so its drag never reaches the host as
            # a widget edit: the editor queues the gesture itself, and only a
            # block of audio can flush it. That is the claim under test.
            heard.clear()
            play(events=record_out)
            lines = []
            for name in ("Band 1 frequency", "Band 1 depth"):
                ident = infos[name].id
                kinds = [kind for kind, seen, _ in heard if seen == ident]
                values = [value for kind, seen, value in heard if seen == ident and kind == 5]
                # One begin, the values, one end: a drag is one gesture, not
                # a stream of edits a host would each have to bracket.
                assert kinds == [7, *[5] * len(values), 8], (name, kinds)
                assert len(set(values)) > 1, (name, values)
                lines.append(
                    f"{name}: one gesture, {len(values)} values, "
                    f"{min(values):.6g} .. {max(values):.6g}"
                )
            print("gestures:\n  " + "\n  ".join(lines))
    if "--wheel-check" in sys.argv:
        # The wheel over band 2 (the 650 Hz bell, three and a half units up),
        # in the editor's own 800x473 space: a notch up widens it (Q falls),
        # Alt turns the wheel to its depth, and Shift makes a notch fine. Each
        # step is a host gesture, so the recorder must hear it open, move and close.
        sx, sy = scale()
        band_two = (int(382 * sx), int(175 * sy))

        def frames(count):
            for _ in range(count):
                plugin.main(pointer)
                time.sleep(0.02)

        def wheel_at(point, direction, count, modifiers=()):
            subprocess.run(["xdotool", "mousemove", str(point[0]), str(point[1])], check=True)
            settle()
            for key in modifiers:
                subprocess.run(["xdotool", "keydown", key], check=True)
            button = "4" if direction == "up" else "5"
            subprocess.run(["xdotool", "click", "--repeat", str(count), "--delay", "60", button], check=True)
            for key in modifiers:
                subprocess.run(["xdotool", "keyup", key], check=True)
            settle()

        def heard_run():
            heard.clear()
            play(events=record_out)

        def heard_gesture(name):
            heard_run()
            return heard_of(name)

        def heard_of(name):
            ident = infos[name].id
            kinds = [kind for kind, seen, _ in heard if seen == ident]
            values = [value for kind, seen, value in heard if seen == ident and kind == 5]
            return kinds, values

        def snap(tag):
            path = out.with_name(f"{out.stem}-wheel-{tag}{out.suffix}")
            subprocess.run(["magick", "import", "-window", str(parent), str(path)], check=True)
            print(f"wrote {path}")
            return path

        def differs(a, b):
            """Pixels that differ between two shots: the visible change."""
            result = subprocess.run(
                ["magick", "compare", "-metric", "AE", str(a), str(b), "null:"],
                capture_output=True,
                text=True,
            )
            # `magick compare -metric AE` prints the count, then its fraction.
            return int(float((result.stderr.split() or ["0"])[0]))

        # Band 2 is put on the bell the aim is taken from: 650 Hz, 3.6 units up. The
        # handle sits at logical (382, 175): the plot starts at y 41, the zero line at 222.
        set_values({"Band 2 frequency": 650.0, "Band 2 depth": 3.6})
        settle()
        rest = snap("rest")
        wheel_at(band_two, "up", 5)
        kinds, widened = heard_gesture("Band 2 q")
        assert kinds[:1] == [7] and kinds[-1:] == [8] and len(widened) > 1, kinds
        print(f"wheel up gestures: {kinds.count(7)} begin, {kinds.count(8)} end")
        assert all(b <= a for a, b in zip(widened, widened[1:])), widened
        assert widened[-1] < widened[0] or widened[0] < 0.707, widened
        print(f"wheel up: Q {widened[0]:.4f} -> {widened[-1]:.4f} ({len(widened)} values)")
        up = snap("up")
        print(f"pixels changed by the wheel: {differs(rest, up)}")

        wheel_at(band_two, "down", 3, ["alt"])
        kinds, depth = heard_gesture("Band 2 depth")
        assert kinds[:1] == [7] and kinds[-1:] == [8] and len(depth) > 1, kinds
        assert depth[-1] < depth[0], depth
        print(f"alt + wheel down: depth {depth[0]:.3f} -> {depth[-1]:.3f}")
        alt = snap("alt-depth")

        wheel_at(band_two, "up", 5, ["shift"])
        kinds, fine = heard_gesture("Band 2 q")
        assert kinds[:1] == [7] and kinds[-1:] == [8], kinds
        print(f"shift + wheel up: Q {fine[0]:.4f} -> {fine[-1]:.4f} (fine)")
        snap("shift-fine")
        print(f"pixels changed by the fine steps: {differs(alt, out.with_name(out.stem + '-wheel-shift-fine' + out.suffix))}")

        def click(point, count=1):
            """Presses and releases with editor frames between them, as a hand does: the
            plug-in sees each press, and the pair is inside the double-click time."""
            subprocess.run(["xdotool", "mousemove", str(point[0]), str(point[1])], check=True)
            settle()
            for _ in range(count):
                subprocess.run(["xdotool", "mousedown", "1"], check=True)
                frames(3)
                subprocess.run(["xdotool", "mouseup", "1"], check=True)
                frames(3)

        # The selected band's own drag: a press and a move on band 2 is one
        # host gesture for each of its parameters, and it ends on release. The band
        # bar's knobs name the selected band every frame, so this is the case that
        # used to stay open.
        set_values({"Band 2 frequency": 650.0, "Band 2 depth": 3.6, "Band 2 q": 0.707})
        settle()
        subprocess.run(["xdotool", "mousemove", str(band_two[0]), str(band_two[1])], check=True)
        settle()
        subprocess.run(["xdotool", "mousedown", "1"], check=True)
        for step in range(1, 9):
            subprocess.run(
                ["xdotool", "mousemove", str(band_two[0] + 30 * step // 8 * sx), str(band_two[1] + 20 * step // 8 * sy)],
                check=True,
            )
            settle()
        subprocess.run(["xdotool", "mouseup", "1"], check=True)
        settle()
        kinds, moved = heard_gesture("Band 2 frequency")
        assert kinds[:1] == [7] and kinds[-1:] == [8] and len(moved) > 1, kinds
        print(f"selected band drag: one gesture, {len(moved)} values, {moved[0]:.1f} -> {moved[-1]:.1f} Hz")
        snap("drag-selected")

        # A double click on a band resets its depth to flat and its width to the default.
        set_values({"Band 2 frequency": 650.0, "Band 2 depth": 3.6, "Band 2 q": 0.2})
        settle()
        click(band_two, count=2)
        heard_run()
        kinds, reset_depth = heard_of("Band 2 depth")
        _, reset_q = heard_of("Band 2 q")
        assert kinds[:1] == [7] and kinds[-1:] == [8] and reset_depth[-1] == 0.0, (kinds, reset_depth)
        assert abs(reset_q[-1] - 0.707) < 1e-6, reset_q
        print(f"double click: depth 3.6 -> {reset_depth[-1]:.1f}, width 0.20 -> {reset_q[-1]:.3f}")
        snap("double-click-reset")

        # Keys are opt-in (`--keys`): under Xvfb with no window manager the arrow
        # keys never reached the editor (the plot is focused, but the window
        # handler saw no key events), so this step is not a passing check yet.
        if "--keys" in sys.argv:
            # Keys, once the plot holds the focus from a click: Up is a boost, and
            # Delete takes the band out of the plot.
            set_values({"Band 2 frequency": 650.0, "Band 2 depth": 0.0, "Band 2 q": 0.707})
            settle()
            click(band_two)
            # With no window manager the keys go where X focus is, not where the pointer
            # is: give the window under the pointer (the plug-in's child) the focus, as a click would.
            import re

            # The editor draws into the child the plug-in opened, named MUI under the host's parent.
            tree = subprocess.run(["xwininfo", "-root", "-tree"], capture_output=True, text=True).stdout
            editor = re.search(r'(0x[0-9a-f]+) "MUI"', tree).group(1)
            subprocess.run(["xdotool", "windowfocus", "--sync", editor], check=True)
            settle()
            for key in ["Up", "Up"]:
                subprocess.run(["xdotool", "key", key], check=True)
                settle()
            kinds, up = heard_gesture("Band 2 depth")
            assert len(up) >= 1 and up[-1] > 0.0, (kinds, up)
            print(f"arrow up x2: depth 0.0 -> {up[-1]:.2f}")
            before_delete = snap("before-delete")
            subprocess.run(["xdotool", "key", "Delete"], check=True)
            settle()
            after_delete = snap("delete-key")
            print(f"pixels changed by Delete: {differs(before_delete, after_delete)}")
    assert gui.hide(pointer)
    gui.destroy(pointer)
    plugin.destroy(pointer)
    entry.deinit()


if __name__ == "__main__":
    main()
