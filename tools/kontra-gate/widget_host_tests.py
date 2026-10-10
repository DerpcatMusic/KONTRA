#!/usr/bin/env python3
"""Synthetic end-to-end XTest/state fixtures; run through kontakto-heavy."""
import argparse
import ctypes
import hashlib
import json
import os
from pathlib import Path
import signal
import struct
import subprocess
import tempfile
import threading
import time


def check_released():
    x = ctypes.CDLL('libX11.so.6')
    x.XOpenDisplay.argtypes = [ctypes.c_char_p]; x.XOpenDisplay.restype = ctypes.c_void_p
    d = x.XOpenDisplay(None); assert d
    try:
        x.XKeysymToKeycode.argtypes = [ctypes.c_void_p, ctypes.c_ulong]; x.XKeysymToKeycode.restype = ctypes.c_ubyte
        code = x.XKeysymToKeycode(d, 65507)
        state = (ctypes.c_char * 32)()
        x.XQueryKeymap.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_char)]
        assert x.XQueryKeymap(d, state)
        assert not (state.raw[code // 8] & (1 << (code % 8))), 'Ctrl held after cleanup'
        x.XDefaultRootWindow.argtypes = [ctypes.c_void_p]; x.XDefaultRootWindow.restype = ctypes.c_ulong
        root, child = ctypes.c_ulong(), ctypes.c_ulong()
        rx, ry, wx, wy = [ctypes.c_int() for _ in range(4)]; mask = ctypes.c_uint()
        x.XQueryPointer.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.POINTER(ctypes.c_ulong), ctypes.POINTER(ctypes.c_ulong), *([ctypes.POINTER(ctypes.c_int)] * 4), ctypes.POINTER(ctypes.c_uint)]
        assert x.XQueryPointer(d, x.XDefaultRootWindow(d), ctypes.byref(root), ctypes.byref(child), ctypes.byref(rx), ctypes.byref(ry), ctypes.byref(wx), ctypes.byref(wy), ctypes.byref(mask))
        assert not (mask.value & (1 << 8)), 'primary button held after cleanup'
    finally:
        x.XCloseDisplay.argtypes = [ctypes.c_void_p]; x.XCloseDisplay(d)


def run(host, plugin):
    evidence = []
    with tempfile.TemporaryDirectory(prefix='kontra-widget-fixture-', dir='/dev/shm') as folder:
        root = Path(folder); template = root / 'template'
        result = subprocess.run([str(host), str(plugin), '--template', str(template)], input=b'', stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=15)
        assert result.returncode == 0, hashlib.sha256(result.stderr).hexdigest()
        plan = b'0 move 10 10\n1 wait 20\n2 button 1 1\n3 wait 50\n4 button 1 0\n5 wait 50\n6 checkpoint\n7 save\n8 reload\n9 wait 100\n10 checkpoint\n11 quit\n'
        read_fd, write_fd = os.pipe(); env = dict(os.environ, KONTRA_WIDGET_RGB_FD=str(write_fd))
        rgb = []; errors = []
        def pixels():
            try:
                with os.fdopen(read_fd, 'rb') as stream:
                    while header := stream.read(16):
                        assert len(header) == 16
                        sequence, generation, width, height = struct.unpack('<IIII', header)
                        assert 0 < width <= 4096 and 0 < height <= 2160
                        payload = stream.read(width * height * 3); assert len(payload) == width * height * 3
                        rgb.append({'sequence': sequence, 'generation': generation, 'width': width, 'height': height, 'rgb_sha256': hashlib.sha256(payload).hexdigest()})
            except BaseException as error: errors.append(type(error).__name__)
        job = subprocess.Popen([str(host), str(plugin), str(template), '--gestures'], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env, pass_fds=(write_fd,))
        os.close(write_fd); reader = threading.Thread(target=pixels); reader.start()
        try:
            stdout, stderr = job.communicate(plan, timeout=30)
            assert job.returncode == 0, hashlib.sha256(stderr).hexdigest()
        finally:
            if job.poll() is None: job.terminate(); job.wait(timeout=10)
            reader.join(timeout=5)
        assert not reader.is_alive() and not errors
        rows = [json.loads(line) for line in stdout.splitlines()]
        points = [row for row in rows if row.get('kind') == 'checkpoint']
        assert len(points) == 2 and len(rgb) == 2
        before, after = [dict(row['parameters']) for row in points]
        assert before[77] == after[77] == 0.75, 'OS pointer value did not survive fresh plugin'
        assert before[78] == 1 and after[78] == 2, 'fresh factory instance not observed'
        assert [row['generation'] for row in points] == [1, 2]
        assert all(row['nonfinite'] == 0 and abs(row['peak'] - 0.75) < 1e-9 for row in points), 'constant-mask audio checkpoint mismatch'
        check_released()
        evidence.append({'test': 'real-OS-pointer-CLAP-owner-audio-fresh-state', 'passed': True, 'factory_instances': 2, 'rgb_frames': rgb, 'stdout_sha256': hashlib.sha256(stdout).hexdigest(), 'stderr_sha256': hashlib.sha256(stderr).hexdigest()})
        key_plan = b'0 move 10 10\n1 wait 20\n2 key 32 1\n3 wait 50\n4 key 32 0\n5 wait 50\n6 checkpoint\n7 quit\n'
        result = subprocess.run([str(host), str(plugin), str(template), '--gestures'], input=key_plan, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=20)
        assert result.returncode == 0
        row = next(json.loads(line) for line in result.stdout.splitlines() if b'"checkpoint"' in line)
        assert dict(row['parameters'])[77] == 0.5, 'real OS key did not reach owner'
        evidence.append({'test': 'real-OS-key-owner', 'passed': True})
        malformed = b'0 move 10 10\n1 key 65507 1\n2 button 1 1\n3 wait 50\n4 invalid\n'
        result = subprocess.run([str(host), str(plugin), str(template), '--gestures'], input=malformed, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=20)
        assert result.returncode == 1 and b'plan operation' in result.stderr
        check_released(); evidence.append({'test': 'held-input-cleanup-after-rejected-plan', 'passed': True})
        job = subprocess.Popen([str(host), str(plugin), str(template), '--gestures'], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        try:
            # Wait for mapped editor before sending held input and interrupting a wait.
            ready = json.loads(job.stdout.readline()); assert ready['kind'] == 'ready'
            job.stdin.write(b'0 move 10 10\n1 key 65507 1\n2 button 1 1\n3 wait 10000\n'); job.stdin.flush()
            time.sleep(0.15); job.send_signal(signal.SIGTERM)
            stdout, stderr = job.communicate(timeout=10)
            assert job.returncode == 1 and b'interrupted' in stderr
        finally:
            if job.poll() is None: job.kill(); job.wait()
        check_released(); evidence.append({'test': 'held-input-cleanup-after-SIGTERM', 'passed': True})
        return {'schema': 1, 'passed': len(evidence), 'total': len(evidence), 'fixture_host_sha256': hashlib.sha256(host.read_bytes()).hexdigest(), 'fixture_plugin_sha256': hashlib.sha256(plugin.read_bytes()).hexdigest(), 'tests': evidence, 'boundary': 'synthetic CLAP fixture; instrument/widget acceptance remains UNKNOWN'}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(); parser.add_argument('host', type=Path); parser.add_argument('plugin', type=Path); parser.add_argument('receipt', type=Path)
    args = parser.parse_args(); result = run(args.host, args.plugin)
    args.receipt.write_text(json.dumps(result, indent=2) + '\n'); print(json.dumps({'passed': result['passed'], 'total': result['total']}))
