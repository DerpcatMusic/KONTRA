#!/usr/bin/env python3
"""The KSP UI probe suite: a few instruments that use every UI control type and a spread of
$CONTROL_PAR values, each logging its callbacks, so the oracle Kontakt and KONTRA can be diffed
(screenshots per page, and the callback trace while xdotool drives every control).

    python3 tools/ksp-ui/gen.py            # writes tools/ksp-ui/probes/*.ksp and lib/Resources/pictures

The pictures are drawn here (no library art). Each page's controls log to !log:
  uc,<control>,<value>,<shift>,<alt>,<ctrl>     on ui_control
  ucs,<ui id>                                   on ui_controls
  pc                                            on persistence_changed
  init,<what>,<value>                           reads taken in init (get_control_par answers)
and every log line saves the array to TRACE (see `trace_path`), through on pgs_changed.
"""
import struct
import zlib
from pathlib import Path

HERE = Path(__file__).resolve().parent
PROBES = HERE / "probes"
PICTURES = HERE / "lib" / "Resources" / "pictures"
# Where a probe saves its trace: Kontakt writes it there (Z: is /), KONTRA under its save folder.
TRACE_DIR = "Z:/data/dev/workspace/scratch/ksp-ui-trace"


def trace_path(page):
    return f"{TRACE_DIR}/{page}.nka"


# --- pictures -----------------------------------------------------------------------------

def png(path, w, h, pixel):
    """A plain RGBA PNG from pixel(x, y) -> (r, g, b, a)."""
    raw = b"".join(b"\0" + b"".join(bytes(pixel(x, y)) for x in range(w)) for y in range(h))

    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))
    path.write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
                     + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b""))


def descriptor(path, frames, *, vres=False, hres=False, fixed=(0, 0, 0, 0), horizontal=False):
    t, b, l, r = fixed
    path.write_text(
        "Has Alpha Channel: yes\n"
        f"Number of Animations: {frames}\n"
        f"Horizontal Animation: {'yes' if horizontal else 'no'}\n"
        f"Vertical Resizable: {'yes' if vres else 'no'}\n"
        f"Horizontal Resizable: {'yes' if hres else 'no'}\n"
        f"Fixed Top: {t}\nFixed Bottom: {b}\nFixed Left: {l}\nFixed Right: {r}\n")


DIGITS = {  # 3x5 digits, so a frame's index shows in a shot
    "0": "111101101101111", "1": "010110010010111", "2": "111001111100111", "3": "111001111001111",
    "4": "101101111001001", "5": "111100111001111", "6": "111100111101111", "7": "111001001001001",
    "8": "111101111101111", "9": "111101111001111"}


def glyph_at(text, x0, y0, x, y, scale=2):
    """Whether (x, y) lies on `text` drawn at (x0, y0)."""
    for i, ch in enumerate(text):
        gx, gy = (x - x0 - i * 4 * scale) // scale, (y - y0) // scale
        if 0 <= gx < 3 and 0 <= gy < 5 and x - x0 - i * 4 * scale >= 0 and DIGITS[ch][gy * 3 + gx] == "1":
            return True
    return False


def strip(name, w, h, frames, colour, *, horizontal=False):
    """`frames` frames of w x h, each a filled box whose fill grows with the frame and which
    carries its index: frame selection is visible in a screenshot."""
    W, H = (w * frames, h) if horizontal else (w, h * frames)

    def pixel(x, y):
        f, fx, fy = ((x // w, x % w, y) if horizontal else (y // h, x, y % h))
        if fx in (0, w - 1) or fy in (0, h - 1):
            return (20, 20, 20, 255)
        if glyph_at(str(f), 3, 3, fx, fy):
            return (255, 255, 255, 255)
        fill = (fx * frames) < (f + 1) * w
        r, g, b = colour
        return (r, g, b, 255) if fill else (r // 3, g // 3, b // 3, 200)
    png(PICTURES / f"{name}.png", W, H, pixel)
    descriptor(PICTURES / f"{name}.txt", frames, horizontal=horizontal)


def pictures():
    PICTURES.mkdir(parents=True, exist_ok=True)
    strip("kp_knob", 40, 40, 31, (60, 140, 220))
    strip("kp_switch", 60, 20, 6, (220, 120, 40))
    strip("kp_button", 60, 20, 6, (60, 200, 90))
    strip("kp_menu", 90, 20, 6, (180, 80, 200))
    strip("kp_vedit", 60, 20, 6, (200, 200, 60))
    strip("kp_hstrip", 20, 20, 8, (200, 60, 60), horizontal=True)
    # A slider: a track picture and a handle.
    png(PICTURES / "kp_slider.png", 120, 12, lambda x, y: (90, 90, 90, 255) if 4 <= y < 8 else (0, 0, 0, 0))
    descriptor(PICTURES / "kp_slider.txt", 1)
    png(PICTURES / "kp_handle.png", 10, 20, lambda x, y: (230, 230, 230, 255))
    descriptor(PICTURES / "kp_handle.txt", 1)
    # A nine-slice label background: 6 px fixed borders, a centre that stretches.
    png(PICTURES / "kp_frame.png", 30, 30, lambda x, y: (240, 240, 240, 255) if min(x, y, 29 - x, 29 - y) < 6
        else (40, 70, 110, 255))
    descriptor(PICTURES / "kp_frame.txt", 1, vres=True, hres=True, fixed=(6, 6, 6, 6))
    # A wallpaper with a 50 px grid, so offsets show.
    png(PICTURES / "kp_wall.png", 900, 380, lambda x, y: (30, 30, 36, 255) if (x + 1) % 50 and (y + 1) % 50
        else (70, 70, 90, 255))
    descriptor(PICTURES / "kp_wall.txt", 1)
    # An XY cursor and a level-meter-free table background.
    png(PICTURES / "kp_cursor.png", 12, 12, lambda x, y: (255, 220, 0, 255) if abs(x - y) < 2 or abs(11 - x - y) < 2
        else (0, 0, 0, 0))
    descriptor(PICTURES / "kp_cursor.txt", 1)
    png(PICTURES / "kp_icon.png", 32, 21, lambda x, y: (255, 0, 128, 255) if (x + y) % 4 else (0, 0, 0, 255))
    descriptor(PICTURES / "kp_icon.txt", 1)


# --- pages ----------------------------------------------------------------------------------

WIDTH, HEIGHT = 900, 380


class Page:
    """One probe instrument: controls, their settings, and the logging around them."""

    def __init__(self, name):
        # One size for every page, small enough that Kontakt shows it whole at 150 %.
        self.name, self.width, self.height = name, WIDTH, HEIGHT
        self.decls, self.setup, self.callbacks = [], [], []
        self.acts = []  # drive.py lines, in page pixels
        self.controls = []  # (name, kind) in declaration order

    def control(self, kind, name, args="", log=True, persistent=True):
        sigil = {"ui_table": "%", "ui_text_edit": "@", "ui_xy": "?"}.get(kind, "$")
        var = f"{sigil}{name}"
        self.decls.append(f"declare {kind} {var}{args}")
        if persistent and kind not in ("ui_label", "ui_mouse_area", "ui_level_meter", "ui_waveform",
                                       "ui_wavetable", "ui_panel"):
            self.decls.append(f"make_persistent({var})")
        self.controls.append((name, kind))
        # Kontakt refuses to compile an `on ui_control` for these (probed in the oracle).
        if log and kind not in ("ui_label", "ui_level_meter"):
            value = {"ui_table": f"{var}[0]", "ui_xy": f"{var}[0]", "ui_text_edit": var}.get(kind, var)
            if kind == "ui_mouse_area":
                value = '$NI_MOUSE_EVENT_TYPE & "," & $NI_MOUSE_OVER_CONTROL'
            if kind == "ui_xy":
                value = f'{var}[0] & "," & {var}[1]'
            self.callbacks.append(
                f"on ui_control ({var})\n"
                f'  !log[$n] := "uc,{name}," & {value} & "," ...\n'
                f"    & get_control_par(get_ui_id({var}), $CONTROL_PAR_KEY_SHIFT) & \",\" ...\n"
                f"    & get_control_par(get_ui_id({var}), $CONTROL_PAR_KEY_ALT) & \",\" ...\n"
                f"    & get_control_par(get_ui_id({var}), $CONTROL_PAR_KEY_CONTROL)\n"
                "  call log_line\nend on")
        return var

    def par(self, var, par, value):
        fn = "set_control_par_str" if isinstance(value, str) else "set_control_par"
        v = f'"{value}"' if isinstance(value, str) else value
        self.setup.append(f"{fn}(get_ui_id({var}), $CONTROL_PAR_{par}, {v})")

    def act(self, *words):
        """One interaction for drive.py: click X Y | dclick X Y | drag X Y DX DY [MOD] |
        key NAME | type TEXT."""
        self.acts.append(" ".join(str(w) for w in words))

    def at(self, var, x, y, w=None, h=None):
        self.par(var, "POS_X", x)
        self.par(var, "POS_Y", y)
        if w is not None:
            self.par(var, "WIDTH", w)
        if h is not None:
            self.par(var, "HEIGHT", h)

    def read(self, what, expr):
        """An init-time read, logged: what Kontakt answers is compared, not drawn."""
        self.setup.append(f'!log[$n] := "init,{what}," & {expr}')
        self.setup.append("inc($n)")

    def ksp(self):
        out = ["{ KSP UI probe suite, page " + self.name + ": written by tools/ksp-ui/gen.py }",
               "on init", "  make_perfview", f"  set_ui_width_px({self.width})",
               f"  set_ui_height_px({self.height})", "  declare !log[1024]", "  declare $n := 0",
               "  pgs_create_key(KLOG, 1)"]
        out += ["  " + d for d in self.decls]
        out += ["  " + s for s in self.setup]
        out += ["  pgs_set_key_val(KLOG, 0, $n)", "end on", "",
                "function log_line", "  if ($n < 1023)", "    inc($n)", "  end if",
                "  pgs_set_key_val(KLOG, 0, $n)", "end function", "",
                "on ui_controls", '  !log[$n] := "ucs," & $NI_UI_ID', "  call log_line", "end on", "",
                "on persistence_changed", '  !log[$n] := "pc"', "  call log_line", "end on", "",
                "on pgs_changed", f'  save_array_str(!log, "{trace_path(self.name)}")', "end on", ""]
        out += [c + "\n" for c in self.callbacks]
        return "\n".join(out)


def widgets():
    """One of every control type, defaults only, on a grid: what each looks like bare."""
    p = Page("widgets")
    grid = [("ui_knob", "knob", "(0, 1000, 10)"), ("ui_slider", "slider", "(0, 127)"),
            ("ui_button", "button", ""), ("ui_switch", "switch", ""), ("ui_menu", "menu", ""),
            ("ui_label", "label", "(2, 1)"), ("ui_value_edit", "vedit", "(0, 100, 1)"),
            ("ui_table", "table", "[16](2, 2, 100)"), ("ui_xy", "xy", "[2]"),
            ("ui_level_meter", "meter", ""), ("ui_waveform", "wave", "(4, 2)"),
            ("ui_wavetable", "wtable", ""), ("ui_file_selector", "files", ""),
            ("ui_mouse_area", "mouse", ""), ("ui_text_edit", "tedit", ""), ("ui_panel", "panel", "")]
    for i, (kind, name, args) in enumerate(grid):
        var = p.control(kind, name, args)
        p.at(var, 10 + (i % 4) * 220, 10 + (i // 4) * 90)
        p.read(f"{name}.type", f"get_control_par(get_ui_id({var}), $CONTROL_PAR_TYPE)")
        p.read(f"{name}.w", f"get_control_par(get_ui_id({var}), $CONTROL_PAR_WIDTH)")
        p.read(f"{name}.h", f"get_control_par(get_ui_id({var}), $CONTROL_PAR_HEIGHT)")
    p.setup.append('add_menu_item($menu, "one", 1)')
    p.setup.append('add_menu_item($menu, "two", 2)')
    p.setup.append('set_text($label, "label text")')
    p.setup.append('set_text($button, "button")')
    p.setup.append('set_text($switch, "switch")')
    for a in ["drag 30 30 0 -40", "drag 30 30 0 -40 shift", "click 30 30 ctrl", "dclick 30 30",
              "click 290 19", "drag 240 19 40 0", "click 490 19", "click 710 19", "click 710 19",
              "click 50 109", "key Down", "key Return", "drag 520 109 0 -30", "drag 680 105 60 0",
              "click 40 205", "drag 40 205 20 5", "click 490 199", "click 710 199", "click 50 289",
              "click 270 289", "drag 270 289 20 0", "click 490 289", "type ab", "key Return"]:
        p.act(a)
    return p


def geometry():
    """Sizes, positions, grid moves, hide states, z-layers and nested panels."""
    p = Page("geometry")
    k = [p.control("ui_knob", f"k{i}", "(0, 100, 1)") for i in range(4)]
    p.at(k[0], 20, 20)
    p.setup.append(f"move_control({k[1]}, 3, 2)")
    p.setup.append(f"move_control_px({k[2]}, 300, 20)")
    p.setup.append(f"move_control({k[3]}, 0, 0)")  # hidden by a zero grid position
    s = [p.control("ui_switch", f"s{i}") for i in range(4)]
    for i, v in enumerate(s):
        p.at(v, 20 + 110 * i, 200, 100 - 20 * i, 18 + 6 * i)
        p.setup.append(f'set_text({v}, "s{i}")')
    p.setup.append(f"hide_part({s[1]}, $HIDE_PART_BG)")
    p.setup.append(f"hide_part({s[2]}, $HIDE_PART_TITLE)")
    p.par(s[3], "HIDE", 16 * 8)  # $HIDE_WHOLE_CONTROL
    sl = [p.control("ui_slider", f"sl{i}", "(0, 100)") for i in range(3)]
    for i, v in enumerate(sl):
        p.at(v, 500, 200 + 30 * i, 80 + 40 * i, 16)
    pa = p.control("ui_panel", "pa")
    pb = p.control("ui_panel", "pb")
    p.at(pa, 600, 20)
    p.at(pb, 30, 30)
    p.par(pb, "PARENT_PANEL", "__ID_pa")
    inner = p.control("ui_button", "inner")
    p.at(inner, 10, 10, 80, 20)
    p.par(inner, "PARENT_PANEL", "__ID_pb")
    p.setup.append(f'set_text({inner}, "in panel")')
    over = [p.control("ui_label", f"z{i}", "(1, 1)") for i in range(2)]
    p.at(over[0], 20, 300, 120, 40)
    p.at(over[1], 60, 315, 120, 40)
    p.par(over[0], "Z_LAYER", 1)
    p.par(over[1], "Z_LAYER", 0)
    for i, v in enumerate(over):
        p.setup.append(f'set_text({v}, "z{i}")')
    for v in k + s + sl + [pa, pb, inner]:
        n = v[1:]
        p.read(f"{n}.x", f"get_control_par(get_ui_id({v}), $CONTROL_PAR_POS_X)")
        p.read(f"{n}.y", f"get_control_par(get_ui_id({v}), $CONTROL_PAR_POS_Y)")
    for a in ["click 60 209", "click 170 209", "click 280 209", "click 390 209", "drag 520 208 40 0",
              "drag 520 238 40 0 alt", "drag 520 268 40 0 ctrl", "click 680 70", "drag 40 40 0 -30"]:
        p.act(a)
    return p


def pictures_page():
    """Skins, animation frames, skin offsets, wallpaper and mouse behaviours."""
    p = Page("pictures")
    p.setup.append('set_control_par_str($INST_WALLPAPER_ID, $CONTROL_PAR_PICTURE, "kp_wall")')
    p.setup.append('set_control_par_str($INST_ICON_ID, $CONTROL_PAR_PICTURE, "kp_icon")')
    p.setup.append("set_skin_offset(10)")
    knobs = []
    for i, (lo, hi, beh) in enumerate([(0, 100, 0), (-50, 50, -500), (0, 1000, 1000), (0, 10, 50)]):
        v = p.control("ui_slider", f"pk{i}", f"({lo}, {hi})")
        p.at(v, 20 + 60 * i, 20, 40, 40)
        p.par(v, "PICTURE", "kp_knob")
        p.par(v, "MOUSE_BEHAVIOUR", beh)
        p.par(v, "DEFAULT_VALUE", (lo + hi) // 2)
        p.setup.append(f"{v} := {lo + (hi - lo) * i // 4}")
        knobs.append(v)
    sw = p.control("ui_switch", "psw")
    p.at(sw, 20, 90, 60, 20)
    p.par(sw, "PICTURE", "kp_switch")
    bt = p.control("ui_button", "pbt")
    p.at(bt, 100, 90, 60, 20)
    p.par(bt, "PICTURE", "kp_button")
    for i, state in enumerate([0, 1, 2]):
        lab = p.control("ui_label", f"pstate{i}", "(1, 1)")
        p.at(lab, 180 + 70 * i, 90, 60, 20)
        p.par(lab, "PICTURE", "kp_switch")
        p.par(lab, "PICTURE_STATE", state)
    me = p.control("ui_menu", "pmenu")
    p.at(me, 20, 130, 90, 20)
    p.par(me, "PICTURE", "kp_menu")
    for i in range(4):
        p.setup.append(f'add_menu_item({me}, "item {i}", {i})')
    ve = p.control("ui_value_edit", "pve", "(0, 100, 1)")
    p.at(ve, 130, 130, 60, 20)
    p.par(ve, "PICTURE", "kp_vedit")
    sl = p.control("ui_slider", "pslider", "(0, 100)")
    p.at(sl, 220, 130, 120, 20)
    p.par(sl, "PICTURE", "kp_handle")
    p.par(sl, "PICTURE_STATE", 0)
    fr = p.control("ui_label", "pframe", "(1, 1)")
    p.at(fr, 20, 170, 250, 80)
    p.par(fr, "PICTURE", "kp_frame")
    p.setup.append(f'set_text({fr}, "nine-slice")')
    h = p.control("ui_slider", "phstrip", "(0, 7)")
    p.at(h, 300, 170, 20, 20)
    p.par(h, "PICTURE", "kp_hstrip")
    kn = p.control("ui_knob", "pknob", "(0, 1000, 10)")
    p.at(kn, 360, 170)
    p.setup.append(f"set_knob_unit({kn}, $KNOB_UNIT_DB)")
    p.setup.append(f'set_knob_label({kn}, "-3.0")')
    p.setup.append(f"set_knob_defval({kn}, 500)")
    p.par(kn, "AUTOMATION_NAME", "probe knob")
    for v in knobs + [sw, bt, me, ve, sl, h, kn]:
        p.read(f"{v[1:]}.value", f"get_control_par(get_ui_id({v}), $CONTROL_PAR_VALUE)")
    for i in range(4):
        p.act(f"drag {40 + 60 * i} 40 0 -30")
        p.act(f"drag {40 + 60 * i} 40 30 0")
    p.act("click 40 40 ctrl")
    for a in ["click 50 100", "click 50 100", "click 130 100", "click 65 140", "key Down", "key Down",
              "key Return", "drag 160 140 0 -20", "drag 230 140 60 0", "click 300 140",
              "drag 310 180 0 -40", "drag 380 190 0 -40", "drag 380 190 0 -40 shift",
              "click 380 190 ctrl"]:
        p.act(a)
    return p


def text():
    """Fonts 0-25, alignments, multi-line labels, text edits and colours."""
    p = Page("text")
    for f in range(26):
        lab = p.control("ui_label", f"f{f}", "(1, 1)", log=False)
        p.at(lab, 10 + (f % 5) * 170, 10 + (f // 5) * 34, 160, 30)
        p.par(lab, "FONT_TYPE", f)
        p.par(lab, "TEXT_ALIGNMENT", f % 3)
        p.setup.append(f'set_text({lab}, "Font {f} AaBb 019")')
    p.par("$f0", "HIDE", 2 ** 3)  # $HIDE_PART_BG on a text-only label
    ml = p.control("ui_label", "lines", "(2, 3)", log=False)
    p.at(ml, 10, 220, 200, 70)
    for i in range(5):
        p.setup.append(f'add_text_line({ml}, "line {i}")')
    p.par(ml, "TEXTPOS_Y", 8)
    te = p.control("ui_text_edit", "edit")
    p.at(te, 230, 220, 160, 20)
    p.par(te, "TEXT", "edit me")
    p.par(te, "FONT_TYPE", 25)
    for i, mode in enumerate(["$VALUE_EDIT_MODE_NOTE_NAMES", "1", "10", "1000"]):
        ve = p.control("ui_value_edit", f"ve{i}", f"(0, 127, {mode})")
        p.at(ve, 420 + 110 * i, 220, 100, 20)
        p.setup.append(f"$ve{i} := 61")
        p.par(f"$ve{i}", "SHOW_ARROWS", i % 2)
    for i, col in enumerate([0xFF0000, 0x00FF00, 0x0000FF]):
        b = p.control("ui_label", f"bg{i}", "(1, 1)", log=False)
        p.at(b, 10 + 90 * i, 310, 80, 30)
        p.setup.append(f"set_control_par(get_ui_id($bg{i}), $CONTROL_PAR_BG_COLOR, {col})")
        p.setup.append(f'set_text($bg{i}, "bg {i}")')
    p.setup.append("set_ui_color(9ff3322h)")
    p.read("font.25", "get_font_id(25)")
    for a in ["dclick 300 230", "type xyz", "key Return"] + [f"drag {450 + 110 * i} 230 0 -25"
                                                              for i in range(4)]:
        p.act(a)
    return p


def data():
    """Menus, tables, xy pads, level meters, waveforms, file selectors, mouse areas."""
    p = Page("data")
    me = p.control("ui_menu", "dmenu")
    p.at(me, 10, 10, 120, 20)
    for i in range(6):
        p.setup.append(f'add_menu_item({me}, "entry {i}", {i * 10})')
    p.setup.append(f'set_menu_item_str(get_ui_id({me}), 1, "renamed")')
    p.setup.append(f"set_menu_item_value(get_ui_id({me}), 2, 99)")
    p.setup.append(f"set_menu_item_visibility(get_ui_id({me}), 3, 0)")
    p.setup.append(f"set_control_par(get_ui_id({me}), $CONTROL_PAR_SELECTED_ITEM_IDX, 4)")
    p.read("menu.items", f"get_control_par(get_ui_id({me}), $CONTROL_PAR_NUM_ITEMS)")
    p.read("menu.value2", f"get_menu_item_value(get_ui_id({me}), 2)")
    p.read("menu.vis3", f"get_menu_item_visibility(get_ui_id({me}), 3)")
    p.read("menu.sel", f"get_control_par(get_ui_id({me}), $CONTROL_PAR_SELECTED_ITEM_IDX)")
    p.read("menu.value", me)
    t1 = p.control("ui_table", "tuni", "[16](3, 4, 100)")
    p.at(t1, 150, 10, 200, 80)
    t2 = p.control("ui_table", "tbip", "[12](2, 6, -100)")
    p.at(t2, 370, 10, 200, 80)
    p.setup.append(f"set_table_steps_shown({t1}, 8)")
    for i in range(16):
        p.setup.append(f"%tuni[{i}] := {i * 6}")
    for i in range(12):
        p.setup.append(f"%tbip[{i}] := {(i * 17) % 200 - 100}")
    p.par(t1, "BAR_COLOR", 0xFF8800)
    p.par(t2, "ZERO_LINE_COLOR", 0x00FFFF)
    xy = p.control("ui_xy", "pad", "[4]")
    p.at(xy, 590, 10, 150, 150)
    p.setup.append("?pad[0] := 0.25")
    p.setup.append("?pad[1] := 0.75")
    p.setup.append(f"set_control_par_str_arr(get_ui_id({xy}), $CONTROL_PAR_CURSOR_PICTURE, \"kp_cursor\", 0)")
    p.setup.append(f"set_control_par(get_ui_id({xy}), $CONTROL_PAR_MOUSE_MODE, 0)")
    lm = p.control("ui_level_meter", "lm")
    p.at(lm, 10, 120, 20, 120)
    p.par(lm, "VERTICAL", 1)
    p.par(lm, "OFF_COLOR", 0x202020)
    p.par(lm, "ON_COLOR", 0x00C000)
    p.par(lm, "OVERLOAD_COLOR", 0xFF0000)
    p.setup.append(f"attach_level_meter(get_ui_id({lm}), -1, -1, 0, -1)")
    wf = p.control("ui_waveform", "wf", "(6, 3)")
    p.at(wf, 50, 120, 300, 100)
    p.setup.append(f"set_ui_wf_property({wf}, $UI_WF_PROP_PLAY_CURSOR, 0, 2000)")
    p.par(wf, "WAVE_COLOR", 0x88CCFF)
    p.par(wf, "BG_COLOR", 0x101820)
    wt = p.control("ui_wavetable", "wt")
    p.at(wt, 370, 120, 200, 100)
    p.par(wt, "WT_VIS_MODE", 1)
    fs = p.control("ui_file_selector", "fsel")
    p.at(fs, 10, 260, 300, 110)
    # A small folder: a large one stalls Kontakt's editor while it lists it.
    p.par(fs, "BASEPATH", TRACE_DIR)
    p.par(fs, "FILE_TYPE", 1)
    ma = p.control("ui_mouse_area", "area")
    p.at(ma, 330, 260, 200, 100)
    p.par(ma, "DND_ACCEPT_AUDIO", 1)
    p.par(ma, "DND_ACCEPT_MIDI", 1)
    p.par(ma, "DND_ACCEPT_ARRAY", 1)
    p.par(ma, "RECEIVE_DRAG_EVENTS", 1)
    tbl = p.control("ui_label", "under", "(1, 1)", log=False)
    p.at(tbl, 330, 260, 200, 100)
    p.setup.append(f'set_text({tbl}, "mouse area")')
    sk = p.control("ui_slider", "keymods", "(0, 1000)")
    p.at(sk, 560, 260, 200, 20)
    for a in ["click 70 20", "key Down", "key Down", "key Return", "drag 160 50 150 0",
              "drag 380 20 0 60", "click 660 80", "drag 660 80 40 40", "drag 660 80 -30 0 shift",
              "click 430 300", "dclick 430 300", "drag 430 300 50 30", "drag 600 270 60 0",
              "drag 600 270 60 0 shift", "drag 600 270 60 0 alt", "drag 600 270 60 0 ctrl",
              "click 100 300"]:
        p.act(a)
    return p


def resolve_panels(text):
    """`__ID_x` in a PARENT_PANEL value means get_ui_id($x) (a number, not a string)."""
    import re
    return re.sub(r'set_control_par_str\((get_ui_id\(\$\w+\)), \$CONTROL_PAR_PARENT_PANEL, "__ID_(\w+)"\)',
                  r"set_control_par(\1, $CONTROL_PAR_PARENT_PANEL, get_ui_id($\2))", text)


def behaviour():
    """How far a drag and a fine (Shift) drag move each kind of control: one
    gesture per control, each starting mid-range, so each control's last callback is one law.
    Shoot at 100 % (DPI=96), so page pixels are screen pixels."""
    p = Page("behaviour")
    # (kind, name, args, start, gesture relative to the click point)
    rows = [
        [("ui_knob", f"k{d}", "(0, 1000, 1)", 500, f"drag 0 0 0 -{d}") for d in (10, 50, 100, 200)]
        + [("ui_knob", "kdown", "(0, 1000, 1)", 500, "drag 0 0 0 100"),
           ("ui_knob", "kright", "(0, 1000, 1)", 500, "drag 0 0 100 0"),
           ("ui_knob", "kfine", "(0, 1000, 1)", 500, "drag 0 0 0 -100 shift"),
           ("ui_knob", "k100r", "(0, 100, 1)", 50, "drag 0 0 0 -100")],
        [("ui_slider", f"s{d}", "(0, 1000)", 500, f"drag 0 0 {d} 0") for d in (10, 20, 40)]
        + [("ui_slider", "sup", "(0, 1000)", 500, "drag 0 0 0 -50"),
           ("ui_slider", "sfine", "(0, 1000)", 500, "drag 0 0 40 0 shift"),
           ("ui_slider", "smb1000", "(0, 1000)", 500, "drag 0 0 20 0"),
           ("ui_slider", "smbneg", "(0, 1000)", 500, "drag 0 0 0 -10"),
           ("ui_slider", "s100r", "(0, 100)", 50, "drag 0 0 20 0")],
        # Value edits first the ranges: in this rig a page loses its value-edit drags after the
        # first two to four (Kontakt hides and recentres the pointer; Xvfb seems to lose it).
        [("ui_value_edit", "v100r40", "(0, 100, 1)", 50, "drag 0 0 0 -40"),
         ("ui_value_edit", "v10kr", "(0, 10000, 1)", 5000, "drag 0 0 0 -10"),
         ("ui_value_edit", "v10", "(0, 1000, 1)", 500, "drag 0 0 0 -10"),
         ("ui_value_edit", "v40", "(0, 1000, 1)", 500, "drag 0 0 0 -40"),
         ("ui_value_edit", "vdown", "(0, 1000, 1)", 500, "drag 0 0 0 20"),
         ("ui_value_edit", "vright", "(0, 1000, 1)", 500, "drag 0 0 40 0"),
         ("ui_value_edit", "vfine", "(0, 1000, 1)", 500, "drag 0 0 0 -20 shift")],
        [("ui_slider", "smb500", "(0, 1000)", 500, "drag 0 0 20 0"),
         ("ui_slider", "smbn500", "(0, 1000)", 500, "drag 0 0 0 -10"),
         ("ui_knob", "kreset", "(0, 1000, 1)", 500, "click 0 0 ctrl")],
    ]
    for r, row in enumerate(rows):
        for c, (kind, name, args, start, gesture) in enumerate(row):
            var = p.control(kind, name, args)
            x, y = 10 + 110 * c, 20 + 90 * r
            p.at(var, x, y)
            p.setup.append(f"{var} := {start}")
            if name == "smb1000":
                p.par(var, "MOUSE_BEHAVIOUR", 1000)
            if name == "smbneg":
                p.par(var, "MOUSE_BEHAVIOUR", -1000)
            if name == "smb500":
                p.par(var, "MOUSE_BEHAVIOUR", 500)
            if name == "smbn500":
                p.par(var, "MOUSE_BEHAVIOUR", -500)
            p.read(f"{name}.start", var)
            for step in gesture.split(";"):
                op, dx, dy, *rest = step.split()
                # Grab each control where the widgets page does: 30, 10 in from its corner.
                p.act(op, x + 30 + int(dx), y + 10 + int(dy), *rest)
    return p


PAGES = [widgets, geometry, pictures_page, text, data, behaviour]


def main():
    pictures()
    PROBES.mkdir(parents=True, exist_ok=True)
    for make in PAGES:
        page = make()
        (PROBES / f"{page.name}.ksp").write_text(resolve_panels(page.ksp()))
        (PROBES / f"{page.name}.drive").write_text("".join(a + "\n" for a in page.acts))
        print(page.name, len(page.controls), "controls")


if __name__ == "__main__":
    main()
