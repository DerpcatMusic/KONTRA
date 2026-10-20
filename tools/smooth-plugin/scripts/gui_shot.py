#!/usr/bin/env python3
"""Open SMOOTH's editor in a real host window and photograph it.

The plug-in is loaded through CLAP, given an X11 parent, shown, and captured.
The picture is what a musician sees, so it is the honest check that the
interface draws.

    xvfb-run -a python3 tools/smooth-plugin/scripts/gui_shot.py ~/.clap/SMOOTH.clap out.png
"""
import ctypes as C
import pathlib
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
    callbacks = [C.CFUNCTYPE(PTR, PTR, C.c_char_p)(lambda *_: None)]
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
    set_parent = C.CFUNCTYPE(C.c_bool, PTR, C.POINTER(Window))(gui.parent)
    assert gui.create(pointer, b"x11", False)
    assert set_parent(pointer, C.byref(Window(b"x11", parent)))
    assert gui.show(pointer)

    def settle():
        for _ in range(60):
            plugin.main(pointer)
            time.sleep(0.05)

    def play(frames=8192, frequency=1000.0):
        """Run the engine over a tone, so the plot has something to show."""
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
                sample = math.sin(2 * math.pi * frequency * index / 44100.0) * 0.5
                arrays[0][i] = arrays[1][i] = sample
            process = Process(
                start, 256, None, C.pointer(audio_in), C.pointer(audio_out), 1, 1,
                C.pointer(in_events), C.pointer(out_events),
            )
            status = plugin.process(pointer, C.byref(process))
            if status == 0:
                print("process refused")
                break
        plugin.stop(pointer)

    settle()
    subprocess.run(["magick", "import", "-window", str(parent), str(out)], check=True)
    print(f"wrote {out}")

    if "--live" in sys.argv:
        # Play a tone through the engine, then photograph the plot again: the
        # reduction curve is the engine's own.
        play()
        settle()
        live = out.with_name(out.stem + "-live" + out.suffix)
        subprocess.run(["magick", "import", "-window", str(parent), str(live)], check=True)
        print(f"wrote {live}")

    if "--click" in sys.argv:
        # Click logical points in the editor's own 800x473 space: `--click
        # x,y x,y`. The window is larger than the editor's logical size, so
        # the geometry the display actually gave us sets the scale. Each click
        # is photographed, so what the click did is visible rather than
        # asserted.
        import re

        geometry = subprocess.run(
            ["xdotool", "getwindowgeometry", "--shell", str(parent)],
            capture_output=True,
            text=True,
            check=True,
        ).stdout
        width = int(re.search(r"WIDTH=(\d+)", geometry).group(1))
        height = int(re.search(r"HEIGHT=(\d+)", geometry).group(1))
        sx, sy = width / 800.0, height / 473.0
        print(f"window {width}x{height}, scale {sx:.2f}")
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
        # zero line, a quarter of the way across the plot.
        # The window is 800 by 473 logical; the display scales it, so the
        # pointer works in physical pixels.
        scale = 1200 / 800
        start = (int(230 * scale), int(225 * scale))
        end = (int(330 * scale), int(300 * scale))
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
    assert gui.hide(pointer)
    gui.destroy(pointer)
    plugin.destroy(pointer)
    entry.deinit()


if __name__ == "__main__":
    main()
