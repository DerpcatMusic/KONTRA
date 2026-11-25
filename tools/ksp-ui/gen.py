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
    p.par(s[3], "HIDE", 16)  # $HIDE_WHOLE_CONTROL
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
    p.par("$f0", "HIDE", 1)  # $HIDE_PART_BG on a text-only label
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


def layers():
    """Who hears a click where two controls overlap, and whether a hidden one does: declaration
    order, Z_LAYER, widget type, a label on top, hidden controls and panels. Each case is two
    controls, the second 20, 10 further in, clicked where they overlap. Shoot at 100 % (DPI=96)."""
    p = Page("layers")
    # (first, second, setup for (first var, second var)) per case; each is (kind, name, args)
    btn = lambda n: ("ui_button", n, "")
    cases = [
        (btn("da"), btn("db"), []),                                   # declaration order
        (btn("fa"), btn("fb"), [("a", "Z_LAYER", 1)]),                # front layer, declared first
        (btn("ga"), btn("gb"), [("b", "Z_LAYER", -1)]),               # back layer, declared last
        (btn("ta"), ("ui_knob", "tb", "(0, 100, 1)"), []),            # a button under a later knob
        (("ui_switch", "la", ""), ("ui_label", "lb", "(1, 1)"), [("b", "Z_LAYER", 1)]),
        (btn("ha"), btn("hb"), [("b", "HIDE", 16)]),                  # the top one hidden whole ($HIDE_WHOLE_CONTROL)
        (btn("pa"), btn("pb"), [("b", "HIDE", 1)]),                   # the top one without its BG ($HIDE_PART_BG)
        (("ui_mouse_area", "ma", ""), btn("mb"), []),                 # a mouse area under a button
    ]
    for c, (first, second, pars) in enumerate(cases):
        x, y = 10 + 110 * c, 20
        a = p.control(*first)
        b = p.control(*second)
        p.at(a, x, y, 80, 30)
        p.at(b, x + 20, y + 10, 80, 30)
        for which, par, value in pars:
            p.par(a if which == "a" else b, par, value)
        p.act("click", x + 50, y + 25)
    # A button in a hidden panel, and one in a back-layer panel under a default button.
    hp = p.control("ui_panel", "hp")
    p.at(hp, 10, 120)
    p.par(hp, "HIDE", 16)  # $HIDE_WHOLE_CONTROL
    hpin = p.control("ui_button", "hpin")
    p.at(hpin, 0, 0, 80, 30)
    p.par(hpin, "PARENT_PANEL", "__ID_hp")
    p.act("click", 50, 135)
    pzo = p.control("ui_button", "pzo")
    p.at(pzo, 140, 130, 80, 30)
    pz = p.control("ui_panel", "pz")
    p.at(pz, 120, 120)
    p.par(pz, "Z_LAYER", -1)
    pzin = p.control("ui_button", "pzin")
    p.at(pzin, 0, 0, 80, 30)
    p.par(pzin, "PARENT_PANEL", "__ID_pz")
    p.act("click", 170, 145)
    # Row 3: a control's own Z_LAYER inside a panel, nested panels, and a mouse area on its own.
    def button(name, x, y, panel=None, layer=None):
        v = p.control("ui_button", name)
        p.at(v, x, y, 80, 30)
        if panel:
            p.par(v, "PARENT_PANEL", f"__ID_{panel}")
        if layer is not None:
            p.par(v, "Z_LAYER", layer)
        return v

    def panel(name, x, y, parent=None, layer=None):
        v = p.control("ui_panel", name)
        p.at(v, x, y)
        if parent:
            p.par(v, "PARENT_PANEL", f"__ID_{parent}")
        if layer is not None:
            p.par(v, "Z_LAYER", layer)
        return v

    y = 220
    # A back-layer button in a default panel, under a default button declared before it.
    button("cout", 20, y + 10)
    panel("ca", 0, y)
    button("cin", 0, 0, "ca", -1)
    p.act("click", 50, y + 25)
    # A front-layer button in a back-layer panel, under a default button declared before it.
    button("cbo", 130, y + 10)
    panel("cb", 110, y, layer=-1)
    button("cbin", 0, 0, "cb", 1)
    p.act("click", 160, y + 25)
    # Nested: a button in an inner panel against a later button in the outer one.
    panel("no", 220, y)
    panel("ni", 0, 0, "no")
    button("nin", 0, 0, "ni")
    button("nout", 20, 10, "no")
    p.act("click", 270, y + 25)
    # Nested under a back-layer outer panel, against a default button declared before.
    button("nbo", 350, y + 10)
    panel("nbout", 330, y, layer=-1)
    panel("nbmid", 0, 0, "nbout")
    button("nbin", 0, 0, "nbmid")
    p.act("click", 380, y + 25)
    # A mouse area alone (drag events on): a click, a drag across and out of it, a hover.
    ma = p.control("ui_mouse_area", "mal")
    p.at(ma, 450, y, 80, 30)
    p.par(ma, "RECEIVE_DRAG_EVENTS", 1)
    p.act("click", 490, y + 15)
    p.act("drag", 460, y + 15, 100, 0)
    p.act("drag", 600, y + 15, -110, 0)
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


def fields():
    """Where a menu's, a button's and a switch's text starts, and how heavy it is: each kind in
    the stock font and in fonts 0, 1 and 13, with each alignment, and a value edit and a label
    in the stock font."""
    p = Page("fields")
    fonts = [None, 0, 1, 13]
    for r, kind in enumerate(["ui_menu", "ui_button", "ui_switch"]):
        for c, font in enumerate(fonts):
            for a in range(3):
                name = f"{kind[3:]}{c}{a}"
                var = p.control(kind, name, log=False)
                p.at(var, 10 + (c * 3 + a) * 76, 10 + r * 40, 72, 20)
                if kind == "ui_menu":
                    p.setup.append(f'add_menu_item({var}, "Lowpass 4", 0)')
                else:
                    p.par(var, "TEXT", "Lowpass 4")
                if font is not None:
                    p.par(var, "FONT_TYPE", font)
                p.par(var, "TEXT_ALIGNMENT", a)
    # The same line in a label, for each font: how its size compares with a field's.
    for c, font in enumerate(fonts):
        lab = p.control("ui_label", f"label{c}", "(1, 1)", log=False)
        p.at(lab, 10 + c * 3 * 76, 170, 72, 20)
        p.setup.append(f'set_text({lab}, "Lowpass 4")')
        if font is not None:
            p.par(lab, "FONT_TYPE", font)
    # A lit switch's words, and TEXTPOS_Y on a field: unset, 0 and 6, off and on.
    for c, (font, ty, on) in enumerate([(None, None, 1), (0, None, 1), (13, None, 1), (0, 0, 0),
                                        (0, 0, 1), (0, 6, 0), (0, 6, 1), (None, 0, 1)]):
        sw = p.control("ui_switch", f"lit{c}", log=False)
        p.at(sw, 10 + c * 76, 210, 72, 20)
        p.par(sw, "TEXT", "Lowpass 4")
        p.setup.append(f"{sw} := {on}")
        if font is not None:
            p.par(sw, "FONT_TYPE", font)
        if ty is not None:
            p.par(sw, "TEXTPOS_Y", ty)
    # A button with no TEXT_ALIGNMENT, off and on: where its words sit by default.
    for c in range(2):
        b = p.control("ui_button", f"plain{c}", log=False)
        p.at(b, 10 + (8 + c) * 76, 210, 72, 20)
        p.par(b, "TEXT", "Lowpass 4")
        p.setup.append(f"{b} := {c}")
    ve = p.control("ui_value_edit", "ve", "(0, 127, 1)", log=False)
    p.at(ve, 10, 130, 150, 20)
    p.setup.append("$ve := 61")
    lab = p.control("ui_label", "lab", "(1, 1)", log=False)
    p.at(lab, 170, 130, 150, 20)
    p.setup.append('set_text($lab, "Lowpass 4")')
    return p


def tables():
    """Where a stock table's columns and bars sit when its width is not a multiple of its
    columns, how its empty cells and background draw, and whether a label with no size
    draws a face (the tempo captions of a library's step sequencer)."""
    p = Page("tables")
    # (columns, range, x, y, width, height, steps shown, bar colour, hidden background)
    specs = [(16, 127, 10, 10, 247, 75, None, 0xF6B05C, False),
             (16, 1, 280, 10, 247, 16, None, 0xF6B05C, False),
             (16, 127, 550, 10, 240, 75, None, None, False),
             (16, 127, 10, 110, 100, 60, None, None, False),
             (13, 127, 130, 110, 200, 60, None, 0xF6B05C, False),
             (16, 127, 350, 110, 247, 60, 12, 0xF6B05C, False),
             (12, -100, 620, 110, 201, 60, None, None, False),
             (16, 127, 10, 200, 247, 60, None, 0xF6B05C, True)]
    for n, (cols, rng, x, y, w, h, shown, bar, bare) in enumerate(specs):
        t = p.control("ui_table", f"t{n}", f"[{cols}](4, 4, {rng})", log=False)
        p.at(t, x, y, w, h)
        for i in range(cols):
            v = 0 if i % 4 == 3 else (rng if i % 4 == 0 else rng * (i + 1) // (cols + 1))
            p.setup.append(f"%t{n}[{i}] := {v}")
        if shown is not None:
            p.setup.append(f"set_table_steps_shown({t}, {shown})")
        if bar is not None:
            p.par(t, "BAR_COLOR", bar)
        if bare:
            p.par(t, "HIDE", 1)  # $HIDE_PART_BG
    # Labels with words and no size (they read 87 x 18), in the stock font and in font 13,
    # beside the same labels sized 87 x 18.
    for c, (font, sized) in enumerate([(None, False), (13, False), (15, False), (None, True),
                                       (13, True)]):
        lab = p.control("ui_label", f"cap{c}", "(1, 1)", log=False)
        p.at(lab, 290 + c * 100, 210, *((87, 18) if sized else ()))
        p.setup.append(f'set_text({lab}, "/2T")')
        if font is not None:
            p.par(lab, "FONT_TYPE", font)
    # How far a tab and a space move a label's words: a library's list rows start with three tabs
    # and three spaces.
    for c, lead in enumerate(["", "\t", "\t\t\t", "   ", "\t\t\t   "]):
        for r, font in enumerate([13, None]):
            lab = p.control("ui_label", f"lead{c}{r}", "(1, 1)", log=False)
            p.at(lab, 10 + c * 150, 290 + r * 40, 140, 26)
            p.par(lab, "TEXT", lead + "Legato")
            p.par(lab, "TEXT_ALIGNMENT", 0)
            if font is not None:
                p.par(lab, "FONT_TYPE", font)
    return p


def hiding():
    """Whether a label's face and a table's frame come back when a control hidden whole is shown
    again (a library's step sequencer hides its page with $HIDE_WHOLE_CONTROL, then sets
    $HIDE_PART_NOTHING). Rows: untouched; whole then nothing; nothing alone; part bg then nothing."""
    p = Page("hiding")
    for r, seq in enumerate([[], [16, 0], [0], [1, 0]]):
        lab = p.control("ui_label", f"cap{r}", "(1, 1)", log=False)
        p.at(lab, 10, 10 + r * 60)
        p.setup.append(f'set_text({lab}, "/2T")')
        sized = p.control("ui_label", f"box{r}", "(1, 1)", log=False)
        p.at(sized, 110, 10 + r * 60, 60, 18)
        p.setup.append(f'set_text({sized}, "x1")')
        t = p.control("ui_table", f"t{r}", "[16](4, 4, 127)", log=False)
        p.at(t, 190, 10 + r * 60, 247, 40)
        p.setup.append(f"set_table_steps_shown({t}, 16)")
        p.par(t, "BAR_COLOR", 0xF6B05C)
        for i in range(16):
            p.setup.append(f"%t{r}[{i}] := {127 if i % 4 == 0 else 40}")
        for v in seq:
            for var in (lab, sized, t):
                p.par(var, "HIDE", v)  # 16 $HIDE_WHOLE_CONTROL, 1 $HIDE_PART_BG, 0 nothing
    return p


def textpos():
    """Where $CONTROL_PAR_TEXTPOS_Y puts a menu's, a value edit's and a button's words, stock and
    pictured, 16, 20 and 30 px tall: unset, 0, -4, 3, 6, 10 and 500 (libraries park words out of
    sight with 500)."""
    p = Page("textpos")
    rows = [(kind, None, None, None, h) for kind in ("ui_menu", "ui_value_edit", "ui_button")
            for h in (16, 20, 30)]
    rows += [("ui_menu", "kp_menu", None, None, 20), ("ui_menu", "kp_menu", 13, 2, 20)]
    for r, (kind, pic, font, align, h) in enumerate(rows):
        for c, y in enumerate([None, 0, -4, 3, 6, 10, 500]):
            args = "(0, 100, 1)" if kind == "ui_value_edit" else ""
            v = p.control(kind, f"r{r}c{c}", args, log=False)
            p.at(v, 10 + c * 100, 8 + r * 33, *((None, None) if pic else (90, h)))
            if kind == "ui_menu":
                p.setup.append(f'add_menu_item({v}, "Item AaBb", 0)')
                p.setup.append(f'add_menu_item({v}, "Other", 1)')
            elif kind == "ui_value_edit":
                p.setup.append(f"{v} := 61")
                p.par(v, "TEXT", "Val")
            else:
                p.setup.append(f'set_text({v}, "Button")')
            if pic:
                p.par(v, "PICTURE", pic)
            if font is not None:
                p.par(v, "FONT_TYPE", font)
            if align is not None:
                p.par(v, "TEXT_ALIGNMENT", align)
            if y is not None:
                p.par(v, "TEXTPOS_Y", y)
    return p


PAGES = [widgets, geometry, pictures_page, text, data, behaviour, layers, fields, tables, hiding, textpos]


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
