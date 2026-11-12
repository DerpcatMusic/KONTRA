#!/usr/bin/env python3
"""Move every control of an instrument's face in the plugin's real editor.

The editor runs under Xvfb (as drive.sh does) and each control is moved with
real xdotool pointer input: knobs, sliders and value edits are dragged,
buttons and switches clicked, menus opened, tables and XY pads swept. A
control passes when the face changes after its move. A move that repaints
more than 15% of the face (a page switch) reopens the editor, so the next
control is tried on the face as it loads.

  tools/kontra-window/sweep.py <kontra-window> <instrument> <controls.jsonl> <out dir>

<controls.jsonl> is ui-shots' `<name>-<slot>-controls.jsonl`: the page size,
then each control's widget, name, kind and page rect in points. The face is found as the bounding box of what
differs from the part panel's background; its width over the page width is
the editor's zoom. Prints one JSON line per control, then a summary line.
"""
import json
import os
import subprocess
import sys
import time
from pathlib import Path

from PIL import Image, ImageChops

# The interface panel at the default 1180x780 window, device scale 1.5:
# right of the browser, under the part tabs, above the keyboard.
PANEL = (400, 192, 1760, 1000)
ORDER = {'knob': 0, 'slider': 0, 'value_edit': 0, 'table': 1, 'xy': 1, 'menu': 2,
         'switch': 3, 'button': 3}


def sh(*args, **kw):
    return subprocess.run(args, check=False, **kw)


def shot(path):
    sh('import', '-window', 'root', str(path))
    return Image.open(path).convert('RGB')


def face_box(image):
    panel = image.crop(PANEL)
    background = Image.new('RGB', panel.size, panel.getpixel((10, 4)))
    diff = ImageChops.difference(panel, background).convert('L').point(lambda v: 255 if v > 6 else 0)
    box = diff.getbbox()
    if not box:
        return None
    return (box[0] + PANEL[0], box[1] + PANEL[1], box[2] + PANEL[0], box[3] + PANEL[1])


def changed(a, b, box=None, level=12):
    return share(a, b, box, level) > 0


def share(a, b, box=None, level=12):
    """The share of pixels in `box` that differ by more than `level`."""
    d = ImageChops.difference(a, b)
    if box:
        d = d.crop(box)
    mask = d.convert('L').point(lambda v: 255 if v > level else 0)
    return mask.histogram()[255] / max(1, mask.width * mask.height)


def press_at(x, y):
    """The pointer arrives, then presses: a real pointer is seen over a
    control before it clicks, and a press in the same instant as the warp
    lands before the editor has seen where the pointer is."""
    sh('xdotool', 'mousemove', str(x), str(y))
    time.sleep(0.25)
    sh('xdotool', 'mousedown', '1')


def move(kind, x0, y0, x1, y1, sign=1):
    """Moves a control; `sign` -1 drags the other way (down and left)."""
    cx, cy = (x0 + x1) // 2, (y0 + y1) // 2
    if kind in ('knob', 'slider', 'value_edit'):
        press_at(cx, cy)
        for step in range(1, 9):
            sh('xdotool', 'mousemove', str(cx + sign * 6 * step), str(cy - sign * 8 * step))
            time.sleep(0.03)
        sh('xdotool', 'mouseup', '1')
    elif kind in ('table', 'xy'):
        press_at(x0 + 3, y1 - 3)
        for step in range(1, 9):
            sh('xdotool', 'mousemove', str(x0 + 3 + (x1 - x0 - 6) * step // 8),
               str(y1 - 3 - (y1 - y0 - 6) * step // 8))
            time.sleep(0.03)
        sh('xdotool', 'mouseup', '1')
    else:
        press_at(cx, cy)
        time.sleep(0.05)
        sh('xdotool', 'mouseup', '1')


def main():
    if len(sys.argv) != 5:
        sys.exit(__doc__)
    binary, instrument, listing, out = sys.argv[1], sys.argv[2], Path(sys.argv[3]), Path(sys.argv[4])
    out.mkdir(parents=True, exist_ok=True)
    lines = [json.loads(line) for line in listing.read_text().splitlines() if line.strip()]
    page_width = next(line['page'][0] for line in lines if 'page' in line)
    controls = sorted((c for c in lines if 'kind' in c), key=lambda c: ORDER.get(c['kind'], 9))
    display = os.environ.get('XDISPLAY', ':96')
    lock = Path(f'/tmp/.X{display[1:]}-lock')
    if lock.exists():
        sys.exit(f'FAIL display {display} is taken; set XDISPLAY')
    env = dict(os.environ, DISPLAY=display, WAYLAND_DISPLAY='', MUI_RENDERER='cpu')
    os.environ['DISPLAY'] = display
    sh(binary, 'state', instrument, str(out / 'part.state'))
    xvfb = subprocess.Popen(['Xvfb', display, '-screen', '0', '1920x1280x24', '-nolisten', 'tcp'],
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    editor = None

    def reopen(editor):
        """The editor on the untouched part: after a page switch, the rest
        of the face is tried as it loads."""
        if editor:
            editor.terminate()
            editor.wait(10)
        with open(out / 'run.log', 'a') as log:
            editor = subprocess.Popen(['nice', '-n', '19', binary, '--state', str(out / 'part.state'),
                                       '--output', 'Discard'], env=env, stdout=log, stderr=subprocess.STDOUT)
        time.sleep(int(os.environ.get('OPEN_S', '25')))
        return editor

    try:
        time.sleep(1)
        editor = reopen(None)
        before = shot(out / 'open.png')
        opened = before
        box = face_box(before)
        if not box:
            sys.exit('FAIL no face in the interface panel')
        zoom = (box[2] - box[0]) / page_width
        print(json.dumps({'face': box, 'zoom': round(zoom, 4)}), flush=True)
        passed = tried = 0
        for n, c in enumerate(controls):
            x, y, w, h = c['rect']
            r = (int(box[0] + x * zoom), int(box[1] + y * zoom),
                 int(box[0] + (x + w) * zoom), int(box[1] + (y + h) * zoom))
            # Inside the panel only: the part of the face scrolled away is not tried.
            if r[3] > PANEL[3] or r[2] > PANEL[2]:
                print(json.dumps(dict(c, result='offscreen')), flush=True)
                continue
            tried += 1
            after = None
            # A fader already at its top answers the other way.
            for sign in (1, -1) if c['kind'] in ('knob', 'slider', 'value_edit') else (1,):
                move(c['kind'], *r, sign=sign)
                # Away from the control, so its hover art is not taken for a response.
                sh('xdotool', 'mousemove', '1900', '1270')
                time.sleep(0.6)
                after = shot(out / f'{n:03}.png')
                if changed(before, after, box):
                    break
            own = changed(before, after, r)
            anywhere = changed(before, after, box)
            passed += anywhere
            print(json.dumps(dict(c, result='pass' if anywhere else 'dead', own=own,
                                  elsewhere=anywhere and not own)), flush=True)
            if c['kind'] == 'menu':
                sh('xdotool', 'key', 'Escape')
                time.sleep(0.3)
            time.sleep(0.3)
            before = shot(out / 'last.png')
            if share(opened, before, box) > 0.15:
                editor = reopen(editor)
                before = shot(out / 'last.png')
        print(json.dumps({'summary': f'{passed}/{tried}', 'passed': passed, 'controls': tried}), flush=True)
    finally:
        for p in (editor, xvfb):
            if p:
                p.terminate()
        for p in (editor, xvfb):
            if p:
                try:
                    p.wait(10)
                except subprocess.TimeoutExpired:
                    p.kill()


if __name__ == '__main__':
    main()
