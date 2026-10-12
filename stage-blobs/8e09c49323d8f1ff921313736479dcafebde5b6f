"""Presented-window pixel checks. Images remain in RAM; receipts contain bounds only.

Pure black rectangles are suspicious, not proof of a defect without an authored
reference. Flicker requires a caller-selected static region (exclude meters).
"""
import hashlib
import json
import math
from pathlib import Path
import struct
import argparse
import os
import subprocess
import io
import time

def black_regions(image):
    """Merge equal pure-black horizontal runs; ignore small keys/text/shadows."""
    import numpy as np
    black = np.all(image == 0, axis=2)
    active, rectangles = {}, []
    for y, row in enumerate(black):
        edges = np.flatnonzero(np.diff(np.r_[False, row, False]))
        runs = {(int(a), int(b)) for a, b in edges.reshape(-1, 2) if b - a >= 48}
        for run in set(active) - runs:
            start = active.pop(run)
            if y - start >= 48:
                rectangles.append([run[0], start, run[1] - run[0], y - start])
        for run in runs:
            active.setdefault(run, y)
    for (a, b), start in active.items():
        if len(black) - start >= 48:
            rectangles.append([a, start, b - a, len(black) - start])
    area = image.shape[0] * image.shape[1]
    return sorted([r for r in rectangles if r[2] * r[3] >= area * .01])


def chrome_copies(image, bounds, template=None):
    import numpy as np
    x, y, w, h = bounds
    template = image[y:y+h, x:x+w] if template is None else template
    assert template.shape == (h, w, 3) and image.shape[0] >= h and image.shape[1] >= w
    # Flat fills have no identity and must never certify duplicated chrome.
    pixels = template.reshape(-1, 3)
    colors, indices, counts = np.unique(pixels, axis=0, return_index=True, return_counts=True)
    if len(colors) < 8:
        return []
    probes = indices[np.argsort(counts)[:32]]
    py, px = np.divmod(probes, w)
    candidates = np.argwhere(np.all(image == pixels[probes[0]], axis=2))
    copies = []
    for cy, cx in candidates:
        left, top = int(cx - px[0]), int(cy - py[0])
        if left < 0 or top < 0 or left+w > image.shape[1] or top+h > image.shape[0]:
            continue
        if not np.array_equal(image[top+py, left+px], template[py, px]):
            continue
        if np.mean(np.all(image[top:top+h, left:left+w] == template, axis=2)) >= .98:
            copies.append([left, top, w, h])
    return sorted(copies)


def visibility_flicker(frames):
    """Repeated edge appearance/disappearance in static 16px tiles, >=3 flips."""
    import numpy as np
    if len(frames) < 5:
        return []
    assert all(frame.shape == frames[0].shape for frame in frames)
    h, w = frames[0].shape[:2]
    states = []
    for frame in frames:
        gray = frame.astype(np.int16).mean(axis=2)
        edges = np.abs(np.diff(gray, axis=1)) > 12
        states.append([[bool(edges[y:y+16, x:x+16].mean() > .08)
                        for x in range(0, w-16, 16)] for y in range(0, h-16, 16)])
    states = np.asarray(states)
    if not states.size:
        return []
    flips = np.count_nonzero(states[1:] != states[:-1], axis=0)
    return [[int(x*16), int(y*16), 16, 16, int(flips[y, x])]
            for y, x in np.argwhere(flips >= 3)]


def frame_times(times):
    values = sorted((b-a)*1000 for a, b in zip(times, times[1:]))
    assert all(math.isfinite(v) and v > 0 for v in values)
    if not values:
        return {'count': 0, 'p50_ms': None, 'p99_ms': None, 'max_ms': None}
    return {'count': len(values), 'p50_ms': values[math.ceil(len(values)*.5)-1],
            'p99_ms': values[math.ceil(len(values)*.99)-1], 'max_ms': values[-1]}


def presented_cell(cells, item, condition, source_sha):
    rows = [r for r in cells if r.get('item_sha256') == item and r.get('condition') == condition
            and r.get('source_sha') == source_sha]
    if not rows:
        return {'status': 'UNKNOWN', 'reason': 'presented-surface-receipt-absent'}
    for row in rows:
        if row.get('duplicate_chrome') or row.get('visibility_flicker') or row.get('status') == 'FAIL':
            return dict(row, status='FAIL')
    # ponytail: captures cover selected static ROIs; full interactive coverage
    # must be linked before a clean capture can certify the release UI axis.
    return dict(rows[0], status='UNKNOWN', reason=rows[0].get('reason', 'presented-coverage-incomplete'))


def read_frame(stream):
    """Native host protocol: width/height/monotonic ns, then RGB pixels."""
    import numpy as np
    def exact(size):
        result = bytearray()
        while len(result) < size:
            chunk = stream.read(size-len(result))
            if not chunk:
                raise EOFError('incomplete presented frame')
            result.extend(chunk)
        return result
    w, h, ns = struct.unpack('<IIQ', exact(16))
    assert 0 < w <= 8192 and 0 < h <= 8192 and w*h <= 16777216
    return ns / 1e9, np.frombuffer(exact(w*h*3), np.uint8).reshape(h, w, 3)


def summarize(frames, times, instrument, chrome, static, reference=None):
    import numpy as np
    assert frames and len(frames) == len(times)
    h, w = frames[0].shape[:2]
    assert all(f.shape == (h, w, 3) for f in frames)
    for x, y, bw, bh in [instrument, chrome, static]:
        assert x >= 0 and y >= 0 and bw > 0 and bh > 0 and x+bw <= w and y+bh <= h
    x, y, bw, bh = instrument
    black = []
    for index, frame in enumerate(frames):
        for a, b, rw, rh in black_regions(frame[y:y+bh, x:x+bw]):
            black.append({'frame': index, 'bounds': [x+a, y+b, rw, rh],
                          'reference_nonblack_fraction': None if reference is None else
                          float(np.mean(np.any(reference[y+b:y+b+rh, x+a:x+a+rw] != 0, axis=2)))})
    cx, cy, cw, ch = chrome
    template = (frames[0] if reference is None else reference)[cy:cy+ch, cx:cx+cw]
    duplicates = [{'frame': i, 'bounds': copies} for i, f in enumerate(frames)
                  if len(copies := chrome_copies(f, chrome, template)) > 1]
    sx, sy, sw, sh = static
    flicker = [[sx+a, sy+b, rw, rh, flips] for a, b, rw, rh, flips in
               visibility_flicker([f[sy:sy+sh, sx:sx+sw] for f in frames])]
    black_fault = any(r['reference_nonblack_fraction'] is not None and r['reference_nonblack_fraction'] >= .95 for r in black)
    return {'schema': 1, 'path': 'CLAP-GUI-X11-presented-window-XGetImage', 'frames': len(frames),
            'size': [w, h], 'frame_sha256': [hashlib.sha256(f.tobytes()).hexdigest() for f in frames],
            'black_regions': black, 'duplicate_chrome': duplicates, 'visibility_flicker': flicker,
            'capture_intervals': frame_times(times),
            'status': 'FAIL' if black_fault or duplicates or flicker else 'UNKNOWN',
            'limitations': 'Black authored regions need a fresh reference; static ROI must exclude animations. Capture intervals are not presentation times. Native timing and GPU completion require separate evidence. No absent observation certifies PASS.'}


def gpu_counters():
    result = []
    for card in sorted(Path('/sys/class/drm').glob('card[0-9]')):
        values = {'card': int(card.name[4:])}
        for field in ['gpu_busy_percent', 'mem_info_vram_used', 'mem_info_gtt_used']:
            try: values[field] = int((card / 'device' / field).read_text())
            except (OSError, ValueError): values[field] = None
        result.append(values)
    return result


def compositor_frame(owner_pid):
    import numpy as np
    from PIL import Image
    def owned(pid):
        seen = set()
        while pid and pid not in seen:
            if pid == owner_pid:
                return True
            seen.add(pid)
            try: pid = int(Path(f'/proc/{pid}/stat').read_text().rsplit(') ', 1)[1].split()[1])
            except (OSError, ValueError, IndexError): return False
        return False
    clients = json.loads(subprocess.check_output(['hyprctl', 'clients', '-j'], stderr=subprocess.DEVNULL, timeout=3))
    windows = [c for c in clients if c.get('title') == 'KONTRA presented gate'
               and c.get('mapped') and not c.get('hidden') and owned(c['pid'])]
    assert len(windows) == 1, 'one owned compositor window required'
    window = windows[0]
    x, y = window['at']; w, h = window['size']
    assert w > 0 and h > 0 and w*h <= 16777216
    raw = subprocess.check_output(['grim', '-g', f'{x},{y} {w}x{h}', '-t', 'ppm', '-'], stderr=subprocess.DEVNULL, timeout=5)
    frame = np.asarray(Image.open(io.BytesIO(raw)).convert('RGB'))
    return time.monotonic(), frame, [x, y, w, h]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('plugin', type=Path)
    parser.add_argument('cli', type=Path)
    parser.add_argument('host', type=Path)
    parser.add_argument('item', type=Path)
    parser.add_argument('out', type=Path)
    parser.add_argument('--source-sha', required=True)
    parser.add_argument('--frames', type=int, default=24)
    parser.add_argument('--ram-dir', type=Path, required=True, help='existing private /dev/shm directory; retained for renderer owner')
    parser.add_argument('--state', type=Path, help='reuse a RAM native state; state-author CLI hash remains separately bound')
    args = parser.parse_args()
    assert len(args.source_sha) == 40 and all(c in '0123456789abcdef' for c in args.source_sha)
    assert 5 <= args.frames <= 120
    assert args.ram_dir.resolve().is_relative_to(Path('/dev/shm')) and args.ram_dir.is_dir()
    heavy = Path.home() / '.cache/kontakto-heavy'
    assert not (Path.home() / '.cache/kontra-quiet-request').exists(), 'quiet request blocks submission'
    from evidence import Capture
    from contention import Activity
    from PIL import Image
    args.out.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, KONTRA_NATIVE_UI_TIMING='1')
    capture = Capture(args.out, env)
    env['XDG_CONFIG_HOME'] = str(capture.root / 'config')
    settings = capture.root / 'config/kontra/settings.json'
    settings.parent.mkdir(parents=True)
    settings.write_text(json.dumps({'version': 2, 'imported': True, 'roots': ['/mnt/MAIN_STORAGE/Libraries/Kontakt'], 'view_mode': 'Original'}))
    state = args.state or args.ram_dir / 'session.state'
    assert state.resolve().is_relative_to(Path('/dev/shm'))
    multi = capture.root / 'input.kontra-multi'
    multi.write_text(json.dumps({'format': 'kontra-multi', 'version': 2, 'name': '', 'parts': [{'path': str(args.item), 'program': 0, 'view': 1}]}))
    job = None
    activity = Activity(args.out)
    frames, times, gpu = [], [], []
    receipt = {'schema': 1, 'status': 'UNKNOWN', 'source_sha': args.source_sha,
               'plugin_sha256': hashlib.sha256(args.plugin.read_bytes()).hexdigest(),
               'cli_sha256': hashlib.sha256(args.cli.read_bytes()).hexdigest(),
               'cli_source_provenance': 'hash-bound state author; source distinct from measured plugin, full tree not established here',
               'host_sha256': hashlib.sha256(args.host.read_bytes()).hexdigest(),
               'item_sha256': hashlib.sha256(str(args.item).encode()).hexdigest(),
               'program': 0, 'condition': 'os-warm', 'view': 'Original', 'gpu': gpu,
               'native_timing': [], 'timing_status': 'UNKNOWN', 'gpu_completion': 'UNKNOWN'}
    try:
        activity.start()
        if args.state is None:
            result = subprocess.run([str(heavy), 'timeout', '180', str(args.cli), 'export-multi-state', str(multi), str(state)],
                                    env=env, stdout=subprocess.PIPE, stderr=capture.stderr)
            assert result.returncode == 0 and state.exists(), 'state author failed'
        receipt['state_sha256'] = hashlib.sha256(state.read_bytes()).hexdigest()
        job = subprocess.Popen([str(heavy), 'timeout', '150', str(args.host), str(args.plugin), str(state), str(args.frames)],
                               env=env, stdout=subprocess.PIPE, stderr=capture.stderr)
        bounds = []
        for _ in range(args.frames):
            read_frame(job.stdout)  # bounded host readiness/cadence; X11 pixels are not the compositor witness
            at, frame, window = compositor_frame(job.pid)
            times.append(at); frames.append(frame); bounds.append(window); gpu.append(gpu_counters())
        result = job.wait(timeout=30)
        assert result == 0, 'presented host failed'
        h, w = frames[0].shape[:2]
        # Protected chrome is static; animated authored instrument tiles remain a finding.
        instrument = (0, min(180, h//4), w, max(1, h-min(180, h//4)-min(120, h//4)))
        chrome = (0, 0, min(500, w), min(48, h))
        receipt.update(summarize(frames, times, instrument, chrome, chrome))
        receipt.update(path='CLAP-GUI-Hyprland-grim-presented-window', compositor_bounds=bounds)
        if all((frame[8:-8, 8:-8] == 0).all() for frame in frames):
            receipt.update(status='UNKNOWN', reason='presented-editor-unpainted')
        Image.fromarray(frames[0]).save(args.ram_dir / 'before.png')
        Image.fromarray(frames[-1]).save(args.ram_dir / 'last.png')
        receipt['instrument_visibility_findings'] = visibility_flicker([f[instrument[1]:instrument[1]+instrument[3]] for f in frames])
        for log in capture.root.rglob('*.jsonl'):
            for line in log.read_text(errors='replace').splitlines():
                try: row = json.loads(line)
                except ValueError: continue
                if row.get('event') == 'native_frame_timing':
                    # Fixed diagnostic schema: metadata only, no authored values.
                    receipt['native_timing'].append(row.get('data', {}))
                if row.get('event') == 'native_window':
                    reason = row.get('data', {}).get('reason', '')
                    stage = next((label for text, label in [('GPU ready', 'gpu-ready'), ('GPU unavailable', 'gpu-unavailable'),
                        ('GPU init creating native surface', 'gpu-surface'), ('GPU init creating adapter', 'gpu-device'),
                        ('waiting for first frame', 'window-ready'), ('window failed', 'window-failed')] if text in reason), 'window-other')
                    receipt.setdefault('renderer_stages', []).append(stage)
    except (AssertionError, EOFError, OSError, ValueError, subprocess.SubprocessError) as error:
        receipt.update(status='UNKNOWN', reason=type(error).__name__, frames=len(frames))
    finally:
        if job is not None and job.poll() is None:
            job.kill(); job.wait()
        activity.finish()
        observed = json.loads((args.out / 'activity.json').read_text())
        receipt['contention'] = observed['status']
        receipt['timing_status'] = observed['status'] if receipt['native_timing'] else 'UNKNOWN'
        capture.stderr.flush(); capture.stderr.seek(0)
        errors = capture.stderr.read()
        receipt['host_failures'] = [stage for stage in ['module', 'init', 'state load', 'load deadline', 'GUI create',
            'GUI attach/show', 'child query', 'one embedded editor child', 'visible editor child', 'presented readback', 'RGB visual masks']
            if ('presented-host failure: '+stage).encode() in errors]
        capture.finish()
        (args.out / 'metrics.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(json.dumps({key: receipt.get(key) for key in ['status', 'frames', 'reason', 'contention', 'black_regions', 'duplicate_chrome', 'visibility_flicker', 'native_timing', 'capture_intervals']}))


if __name__ == '__main__':
    main()
