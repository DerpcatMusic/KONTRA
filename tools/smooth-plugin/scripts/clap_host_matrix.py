#!/usr/bin/env python3
"""The host matrix for SMOOTH: the configurations a real DAW uses and the
focused smoke host does not.

clap_smoke.py renders one shape of call: 44.1 kHz, 256-sample blocks, separate
input and output buffers, no mid-block events. This script runs the grid a DAW
actually produces: sample rates 44.1 to 192 kHz, fixed and varying block sizes
from 1 to 4096, mono and stereo, in-place buffers (input and output are the
same memory), parameter changes at a sample inside a block and through a flush
with no audio, activate, deactivate and reactivate, zero-frame calls, silence,
input and output channel counts that differ, and an editor opened and closed
between calls.

For every configuration it asserts the same facts, each measured after
SETTLE_SECONDS, once the 30 ms gain ramps have converged:
  - mix 0 gives the dry signal, delayed by the reported latency;
  - out trim -6 dB gives -6.02 dB;
  - bypass passes the dry signal, delayed by the latency;
  - a trim change sent inside a block is heard at exactly that sample (or at the
    sub-block start, when it is closer than MIN_SUBBLOCK to it);
  - every edge case (channel counts, zero frames, no input bus) is finite and
    writes every output channel the host connected.
Per rate it also checks the controls move the audio by the expected amount, the
state round trip, process() outside start and stop, and activate/deactivate
cycles at changing rates and block sizes.

    python3 tools/smooth-plugin/scripts/clap_host_matrix.py ~/.clap/SMOOTH.clap [--quick]

Exit status is non-zero when any configuration fails. Each check is guarded:
a host call that fails is recorded as a finding, and the matrix carries on.
"""
import ctypes as C
import json
import math
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from clap_smoke import (  # noqa: E402 - the shared CLAP ABI structs
    PTR, U32, U64, F64, Audio, Entry, Factory, Gui, Header, Host, InEvents, Latency,
    OutEvents, ParamEvent, ParamInfo, Params, Plugin, Process, Version, plain,
)

# The reference's defaults (see clap_smoke.py), with the mix dry for the checks.
DEFAULTS = {
    "Depth": 5.0, "Detail": 5.0, "Attack": 5.0, "Release": 3.0, "Mode": 0.0,
    "Low latency": 0.0, "Quality": 0.0, "Mix": 100.0, "Out trim": 0.0,
    "Delta": 0.0, "Bypass": 0.0,
}

RATES = [44100.0, 48000.0, 88200.0, 96000.0, 176400.0, 192000.0]
# Fixed blocks, and one pattern that changes size on every call.
PATTERNS = {
    "1": [1], "16": [16], "64": [64], "333": [333], "512": [512], "4096": [4096],
    "varying": [64, 1, 333, 16, 512, 4096, 7],
}
QUICK_RATES = [44100.0, 48000.0, 192000.0]
QUICK_PATTERNS = ["1", "64", "4096", "varying"]

TRIM_MINUS_6_DB = 10 ** (-6 / 20)
# Dry, bypass, out and the trim are one-pole ramps of 30 ms (dsp.rs), so a
# window that starts at the latency sees their transients. Every level is
# measured after the settle; the windows that test a change start after it.
SETTLE_SECONDS = 0.4
MEASURE_SECONDS = 0.2
TOLERANCE = 1e-4        # sample error, steady state
# The reference's gain helper runs in float32 (dsp.rs SmoothedGain, pinned bit-exact by
# `the_mix_ramp_matches_the_reference`). Its step (target - gain) * c rounds to zero once
# the gain is within ulp/c of the target, so a dry or out gain settles short by about
# 1e-4 at 192 kHz (c ~ 7e-6). That is -80 dB and inaudible; it is inherited, not a bug.
REFERENCE_FLOOR = {192000.0: 2e-4}
RATIO_TOLERANCE = 1e-3  # gain ratio, steady state
# moose's chunker (AutomationConfig::DEFAULT.min_subblock_samples = 32) applies
# a parameter change that lands closer than this to its sub-block's start at
# that start. Changes at or past it split the block and land exactly.
MIN_SUBBLOCK = 32
# Depth 10 must take at least this much off Depth 0 on noise. Measured: 3.5 dB
# at 44.1 kHz, 1.1 dB at 96 kHz, 0.45 dB at 192 kHz, so this guards that the
# control is audible at every rate and is not a calibrated target.
DEPTH_MIN_DB = 0.25
CLAP_PROCESS_ERROR = 0
CLAP_PROCESS_TAIL = 3
CLAP_AUDIO_PORT_IS_MAIN = 1


class AudioPortInfo(C.Structure):
    _fields_ = [
        ("id", U32), ("name", C.c_char * 256), ("flags", U32), ("channel_count", U32),
        ("port_type", C.c_char_p), ("in_place_pair", U32),
    ]


class Tail(C.Structure):
    _fields_ = [("get", C.CFUNCTYPE(U32, PTR))]


class AudioPorts(C.Structure):
    _fields_ = [
        ("count", C.CFUNCTYPE(U32, PTR, C.c_bool)),
        ("get", C.CFUNCTYPE(C.c_bool, PTR, U32, C.c_bool, C.POINTER(AudioPortInfo))),
    ]


class OStream(C.Structure):
    _fields_ = [("ctx", PTR), ("write", C.CFUNCTYPE(C.c_int64, PTR, PTR, U64))]


class IStream(C.Structure):
    _fields_ = [("ctx", PTR), ("read", C.CFUNCTYPE(C.c_int64, PTR, PTR, U64))]


class StateExt(C.Structure):
    _fields_ = [
        ("save", C.CFUNCTYPE(C.c_bool, PTR, C.POINTER(OStream))),
        ("load", C.CFUNCTYPE(C.c_bool, PTR, C.POINTER(IStream))),
    ]


class Instance:
    """One loaded SMOOTH instance, driven the way a host drives it."""

    def __init__(self, path):
        self.path = str(path)
        self.lib = C.CDLL(self.path)
        self.entry = Entry.in_dll(self.lib, "clap_entry")
        assert self.entry.init(self.path.encode())
        self.factory_ptr = self.entry.factory(b"clap.plugin-factory")
        factory = C.cast(self.factory_ptr, C.POINTER(Factory)).contents
        descriptor = factory.descriptor(self.factory_ptr, 0).contents
        self.callbacks = [C.CFUNCTYPE(PTR, PTR, C.c_char_p)(lambda *_: None)]
        self.callbacks += [C.CFUNCTYPE(None, PTR)(lambda *_: None) for _ in range(3)]
        self.host = Host(Version(1, 2, 0), None, b"SMOOTH host matrix", b"Derpcat", b"", b"0.1", *self.callbacks)
        self.pointer = factory.create(self.factory_ptr, C.byref(self.host), descriptor.id)
        assert self.pointer
        self.plugin = C.cast(self.pointer, C.POINTER(Plugin)).contents
        assert self.plugin.init(self.pointer)
        self.params = C.cast(self.plugin.extension(self.pointer, b"clap.params"), C.POINTER(Params)).contents
        self.latency_ext = C.cast(self.plugin.extension(self.pointer, b"clap.latency"), C.POINTER(Latency)).contents
        self.ports_ptr = self.plugin.extension(self.pointer, b"clap.audio-ports")
        self.gui_ptr = self.plugin.extension(self.pointer, b"clap.gui")
        self.tail_ptr = self.plugin.extension(self.pointer, b"clap.tail")
        self.infos = {}
        for index in range(self.params.count(self.pointer)):
            info = ParamInfo()
            assert self.params.info(self.pointer, index, C.byref(info))
            self.infos[info.name.decode()] = info
        self.pending = []
        self.time = 0
        self.last_status = None
        self.in_events = InEvents(
            None,
            C.CFUNCTYPE(U32, PTR)(lambda _: len(self.pending)),
            C.CFUNCTYPE(PTR, PTR, U32)(lambda _, index: C.addressof(self.pending[index])),
        )
        self.out_events = OutEvents(None, C.CFUNCTYPE(C.c_bool, PTR, PTR)(lambda *_: True))

    def param_event(self, name, value, time=0):
        return ParamEvent(
            Header(C.sizeof(ParamEvent), time, 0, 5, 0), self.infos[name].id, None, -1, -1, -1, -1,
            plain(name, value),
        )

    def flush(self, values):
        """Parameter changes with no audio, as a host does while stopped."""
        self.pending = [self.param_event(name, value) for name, value in values.items()]
        self.params.flush(self.pointer, C.byref(self.in_events), C.byref(self.out_events))
        self.pending = []

    def value(self, name):
        out = F64()
        assert self.params.value(self.pointer, self.infos[name].id, C.byref(out))
        return out.value

    def latency(self):
        return self.latency_ext.get(self.pointer)

    def tail(self):
        """The tail the host reads from clap.tail, in samples (0 if none)."""
        return C.cast(self.tail_ptr, C.POINTER(Tail)).contents.get(self.pointer) if self.tail_ptr else 0

    def ports(self):
        """The audio ports the plug-in declares, as the host reads them."""
        ports = C.cast(self.ports_ptr, C.POINTER(AudioPorts)).contents
        found = []
        for is_input in (True, False):
            for index in range(ports.count(self.pointer, is_input)):
                info = AudioPortInfo()
                assert ports.get(self.pointer, index, is_input, C.byref(info)), f"port {index} get failed"
                found.append({
                    "input": is_input, "main": bool(info.flags & CLAP_AUDIO_PORT_IS_MAIN),
                    "channels": info.channel_count, "name": info.name.decode(),
                    "type": info.port_type.decode() if info.port_type else None,
                })
        return found

    def save_state(self):
        """clap.state save into a memory stream; returns the bytes."""
        ext = C.cast(self.plugin.extension(self.pointer, b"clap.state"), C.POINTER(StateExt)).contents
        blob = bytearray()

        def write(_, buffer, size):
            blob.extend(C.string_at(buffer, size))
            return size

        stream = OStream(None, C.CFUNCTYPE(C.c_int64, PTR, PTR, U64)(write))
        assert ext.save(self.pointer, C.byref(stream)), "state save failed"
        return bytes(blob)

    def load_state(self, blob):
        """clap.state load from a memory stream holding `blob`."""
        ext = C.cast(self.plugin.extension(self.pointer, b"clap.state"), C.POINTER(StateExt)).contents
        offset = [0]

        def read(_, buffer, size):
            count = min(size, len(blob) - offset[0])
            C.memmove(buffer, blob[offset[0]: offset[0] + count], count)
            offset[0] += count
            return count

        stream = IStream(None, C.CFUNCTYPE(C.c_int64, PTR, PTR, U64)(read))
        assert ext.load(self.pointer, C.byref(stream)), "state load failed"

    def activate(self, rate, max_block):
        assert self.plugin.activate(self.pointer, rate, 1, max_block), "activate failed"
        assert self.plugin.start(self.pointer), "start failed"

    def deactivate(self):
        self.plugin.stop(self.pointer)
        self.plugin.deactivate(self.pointer)

    def process(self, inputs, frames, out_channels=None, events=(), in_place=False, strict=True):
        """One process call. `inputs` is a list of channel lists (empty for a
        call with no input bus); `events` are (offset, name, value) inside the
        block; `out_channels` is the number of output channels the host gives.
        `strict` asserts the status is not an error; `last_status` holds it."""
        out_channels = len(inputs) if out_channels is None else out_channels
        in_arrays = [(C.c_float * frames)(*channel) for channel in inputs]
        if in_place:
            assert out_channels == len(inputs)
            out_arrays = in_arrays
        else:
            out_arrays = [(C.c_float * frames)() for _ in range(out_channels)]
        in_ptrs = (C.POINTER(C.c_float) * max(1, len(in_arrays)))(*in_arrays)
        out_ptrs = (C.POINTER(C.c_float) * max(1, len(out_arrays)))(*out_arrays)
        in_audio = Audio(in_ptrs, None, len(in_arrays), 0, 0)
        out_audio = Audio(out_ptrs, None, len(out_arrays), 0, 0)
        self.pending = [self.param_event(name, value, time) for time, name, value in sorted(events)]
        process = Process(
            self.time, frames, None, C.pointer(in_audio), C.pointer(out_audio),
            1 if in_arrays else 0, 1, C.pointer(self.in_events), C.pointer(self.out_events),
        )
        status = self.plugin.process(self.pointer, C.byref(process))
        self.last_status = status
        self.pending = []
        if strict:
            assert status != CLAP_PROCESS_ERROR, f"process returned CLAP_PROCESS_ERROR (call at sample {self.time}, {frames} frames)"
        self.time += frames
        return [list(array) for array in out_arrays]

    def stream(self, signal, pattern, in_place=False, changes=None, out_channels=None):
        """Feed `signal` (channel lists) in blocks that follow `pattern`, with
        `changes` = {sample: {name: value}} delivered at those samples."""
        total = len(signal[0])
        outputs = [[] for _ in range(out_channels or len(signal))]
        index, call = 0, 0
        while index < total:
            size = min(pattern[call % len(pattern)], total - index)
            call += 1
            events = [
                (t - index, name, value)
                for t in range(index, index + size)
                for name, value in (changes or {}).get(t, {}).items()
            ]
            block = [channel[index:index + size] for channel in signal]
            produced = self.process(block, size, out_channels=len(outputs), events=events, in_place=in_place)
            for channel, samples in zip(outputs, produced):
                channel.extend(samples)
            index += size
        return outputs

    def gui(self):
        return C.cast(self.gui_ptr, C.POINTER(Gui)).contents

    def close(self):
        self.plugin.destroy(self.pointer)
        self.entry.deinit()


def noise(count, seed, amplitude=0.3):
    """White noise from a fixed LCG, so every run sees the same signal."""
    state, out = seed, []
    for _ in range(count):
        state = (state * 1664525 + 1013904223) & 0xFFFFFFFF
        out.append(((state >> 8) / 8388608.0 - 1.0) * amplitude)
    return out


def rms(samples):
    return math.sqrt(sum(v * v for v in samples) / max(1, len(samples)))


def max_error(a, b):
    return max((abs(x - y) for x, y in zip(a, b)), default=0.0)


def settle_frames(rate):
    return round(SETTLE_SECONDS * rate)


def measure_frames(rate, pattern):
    """A measurement window: MEASURE_SECONDS, and at least eight of the largest blocks."""
    return max(round(MEASURE_SECONDS * rate), 8 * max(pattern))


def steady_error(produced, reference, latency, settle, total):
    """The largest sample error of `produced` from `reference` delayed by the
    latency, over the steady window: host samples [latency + settle, total)."""
    return max_error(produced[latency + settle: total], reference[settle: total - latency])


def steady_ratio(produced, reference, latency, settle, total):
    """The RMS ratio of `produced` to the delayed `reference`, steady window."""
    return rms(produced[latency + settle: total]) / rms(reference[settle: total - latency])


def block_starts(pattern, total):
    """(start, size) of every host block the stream() schedule makes."""
    index, call, blocks = 0, 0, []
    while index < total:
        size = min(pattern[call % len(pattern)], total - index)
        blocks.append((index, size))
        index += size
        call += 1
    return blocks


def change_point(pattern, latency):
    """A sample for an in-block change: the middle of the first host block that
    starts at or after the latency. Returns (block_start, change)."""
    start, size = next(b for b in block_starts(pattern, latency + 2 * max(pattern)) if b[0] >= latency)
    return start, start + size // 2


def first_difference(a, b):
    return next((n for n in range(min(len(a), len(b))) if a[n] != b[n]), None)


def relative_difference(a, b):
    """RMS of a - b relative to the RMS of b, over equal-length windows."""
    return rms([x - y for x, y in zip(a, b)]) / rms(b)


def guarded(name, check, *args):
    """A host call that fails is a finding about the plug-in, not a reason to
    stop the matrix: record it against this name and carry on."""
    try:
        return check(*args)
    except (AssertionError, OSError) as error:
        return {"config": name, "metrics": {}, "failures": [f"{name}: {error}"]}


def check_config(path, rate, pattern_name, channels, in_place):
    """Dry, trim, bypass, a change inside a block, and reactivation, for one
    sample rate, block pattern, channel count and buffer arrangement. Levels
    are measured after the settle; the change is checked against the same
    stream without it, sample for sample."""
    pattern = PATTERNS[pattern_name]
    tag = f"{rate:.0f}Hz/{pattern_name}/{channels}ch/{'in-place' if in_place else 'split'}"
    failures, metrics = [], {}
    settle, measure = settle_frames(rate), measure_frames(rate, pattern)

    # The change sits in the first block after the latency; the window after
    # it starts one settle later.
    instance = Instance(path)
    instance.flush({**DEFAULTS, "Mix": 0.0})
    instance.activate(rate, max(pattern))
    latency = instance.latency()
    start, change = change_point(pattern, latency)
    expected = change if change - start >= MIN_SUBBLOCK else start
    total = change + settle + measure
    signal = [noise(total, 1 + c) for c in range(channels)]
    metrics.update(latency=latency, change=change, change_block_start=start)

    # 1. Mix 0 gives the dry signal, delayed by the reported latency.
    dry = instance.stream(signal, pattern, in_place=in_place)
    for c in range(channels):
        error = steady_error(dry[c], signal[c], latency, settle, total)
        metrics[f"dry_steady_{c}"] = error
        if error >= TOLERANCE:
            failures.append(f"{tag}: mix 0 channel {c} steady state is {error:.2e} from the delayed dry")

    # 2. Out trim -6 dB is -6.02 dB on the signal, steady.
    instance.flush({"Out trim": -6.0})
    trimmed = instance.stream(signal, pattern, in_place=in_place)
    ratio = steady_ratio(trimmed[0], signal[0], latency, settle, total)
    metrics["trim_ratio"] = ratio
    if abs(ratio - TRIM_MINUS_6_DB) >= RATIO_TOLERANCE:
        failures.append(f"{tag}: out trim -6 dB gives steady ratio {ratio:.5f}")
    instance.deactivate()
    instance.close()

    # 3. Bypass passes the dry signal, delayed by the latency the host is told.
    instance = Instance(path)
    instance.flush({**DEFAULTS, "Bypass": 1.0})
    instance.activate(rate, max(pattern))
    bypassed = instance.stream(signal, pattern, in_place=in_place)[0]
    error = steady_error(bypassed, signal[0], latency, settle, total)
    metrics["bypass_steady"] = error
    if error >= TOLERANCE:
        failures.append(f"{tag}: bypass steady state is {error:.2e} from the delayed dry")
    instance.deactivate()
    instance.close()

    # 4. A trim change at `change` in host time lands there. `dry` above is the
    #    same stream without it, so the first differing sample is the change,
    #    or the sub-block start it coalesces to. Then trim is -6 dB, steady.
    instance = Instance(path)
    instance.flush({**DEFAULTS, "Mix": 0.0})
    instance.activate(rate, max(pattern))
    stepped = instance.stream(signal, pattern, in_place=in_place, changes={change: {"Out trim": -6.0}})
    first = first_difference(stepped[0], dry[0])
    metrics["change_first_sample"] = first
    if first != expected:
        failures.append(f"{tag}: a trim change at sample {change} is first heard at {first}, expected {expected}")
    lo = change + settle
    after = rms(stepped[0][lo:total]) / rms(signal[0][lo - latency: total - latency])
    metrics["change_after_ratio"] = after
    if abs(after - TRIM_MINUS_6_DB) >= RATIO_TOLERANCE:
        failures.append(f"{tag}: after the in-block change the steady ratio is {after:.5f}")

    # 5. Reactivate at the same rate: the dry path again, steady.
    instance.deactivate()
    instance.flush({**DEFAULTS, "Mix": 0.0})
    instance.activate(rate, max(pattern))
    again = instance.stream(signal, pattern, in_place=in_place)[0]
    error = steady_error(again, signal[0], latency, settle, total)
    metrics["reactivated_steady"] = error
    if error >= REFERENCE_FLOOR.get(rate, TOLERANCE):
        failures.append(f"{tag}: reactivated dry path steady state is {error:.2e} from the delayed dry")
    instance.deactivate()
    instance.close()

    return {"config": tag, "metrics": metrics, "failures": failures}


def edge_flush(path, rate):
    """Changes with no audio are read back and reach the next block."""
    tag = f"{rate:.0f}Hz/flush"
    instance = Instance(path)
    instance.flush(DEFAULTS)
    instance.activate(rate, 512)
    try:
        instance.flush({"Out trim": -6.0, "Mix": 0.0})
        assert abs(instance.value("Out trim") + 6.0) < 1e-9, "flushed out trim reads back wrong"
        assert abs(instance.value("Mix")) < 1e-9, "flushed mix reads back wrong"
        instance.process([[], []], 0, out_channels=2, events=[(0, "Out trim", 0.0)])
        assert abs(instance.value("Out trim")) < 1e-9, "a zero-frame call dropped its event"
        return {"config": tag, "metrics": {}, "failures": []}
    finally:
        instance.deactivate()
        instance.close()


def edge_silence(path, rate):
    tag = f"{rate:.0f}Hz/silence"
    instance = Instance(path)
    instance.flush(DEFAULTS)
    instance.activate(rate, 512)
    try:
        silent = instance.process([[0.0] * 512, [0.0] * 512], 512)
        ok = all(v == 0.0 for channel in silent for v in channel)
        return {"config": tag, "metrics": {}, "failures": [] if ok else [f"{tag}: silence in is not silence out"]}
    finally:
        instance.deactivate()
        instance.close()


def edge_channels(path, rate, inputs, outputs):
    """A channel count the host connects that differs from the plug-in's two
    lanes: mono in to stereo out (both outputs carry the mono), or stereo in to
    mono out (the plug-in must not index a channel the host did not connect).
    Steady, dry, as the lane each output maps to."""
    tag = f"{rate:.0f}Hz/{inputs}in-{outputs}out"
    settle, measure = settle_frames(rate), measure_frames(rate, [512])
    instance = Instance(path)
    instance.flush({**DEFAULTS, "Mix": 0.0})
    instance.activate(rate, 512)
    try:
        latency = instance.latency()
        total = latency + settle + measure
        signal = [noise(total, 3 + c) for c in range(inputs)]
        produced = instance.stream(signal, [512], out_channels=outputs)
        metrics, failures = {}, []
        for c in range(outputs):
            error = steady_error(produced[c], signal[min(c, inputs - 1)], latency, settle, total)
            metrics[f"output_{c}_error"] = error
            if error >= TOLERANCE:
                failures.append(f"{tag}: output channel {c} is {error:.2e} from its input lane")
        return {"config": tag, "metrics": metrics, "failures": failures}
    finally:
        instance.deactivate()
        instance.close()


def edge_no_input_bus(path, rate):
    """No input bus at all: the output must be written, and finite."""
    tag = f"{rate:.0f}Hz/no-input-bus"
    instance = Instance(path)
    instance.flush(DEFAULTS)
    instance.activate(rate, 512)
    try:
        produced = instance.process([], 512, out_channels=2)
        finite = all(math.isfinite(v) for channel in produced for v in channel)
        return {"config": tag, "metrics": {"finite": finite}, "failures": [] if finite else [f"{tag}: non-finite output"]}
    finally:
        instance.deactivate()
        instance.close()


def check_lifecycle(path, rate, cycles=3):
    """The editor opened and closed between calls. A control instance makes the
    same audio calls with no editor: the two must agree sample for sample."""
    tag = f"{rate:.0f}Hz/editor-cycle"
    signal = [noise(2048, 5)]

    def run(with_editor):
        instance = Instance(path)
        instance.flush({**DEFAULTS, "Mix": 0.0})
        instance.activate(rate, 256)
        try:
            before = instance.stream(signal, [256])[0]
            gui = instance.gui() if with_editor else None
            for _ in range(cycles):
                if gui is not None:
                    assert gui.supported(instance.pointer, b"x11", False), "x11 editor not supported"
                    assert gui.create(instance.pointer, b"x11", False), "editor create failed"
                # A call while the editor is open must still be the same audio.
                instance.process(signal, 2048, out_channels=1)
                if gui is not None:
                    gui.destroy(instance.pointer)
            after = instance.stream(signal, [256])[0]
            return before, after
        finally:
            instance.deactivate()
            instance.close()

    with_editor = run(True)
    control = run(False)
    drift = max(max_error(with_editor[0], control[0]), max_error(with_editor[1], control[1]))
    failures = [] if drift == 0.0 else [f"{tag}: the editor changed the audio by {drift:.2e}"]
    return {"config": tag, "metrics": {"editor_drift": drift}, "failures": failures}


def check_ports(path):
    """The buses the plug-in declares: a stereo main pair in and out, and any
    further input a non-main stereo bus (the sidechain). The full list is reported."""
    instance = Instance(path)
    try:
        ports = instance.ports()
        inputs = [p for p in ports if p["input"]]
        outputs = [p for p in ports if not p["input"]]
        failures = []
        if not inputs or not inputs[0]["main"] or inputs[0]["channels"] != 2:
            failures.append(f"main input is not a stereo main bus: {inputs[:1]}")
        if len(outputs) != 1 or not outputs[0]["main"] or outputs[0]["channels"] != 2:
            failures.append(f"outputs are not one stereo main bus: {outputs}")
        for port in inputs[1:]:
            if port["main"] or port["channels"] != 2:
                failures.append(f"extra input is not a stereo sidechain: {port}")
        return {"config": "ports", "metrics": {"ports": ports}, "failures": failures}
    finally:
        instance.close()


def check_state(path, rate):
    """clap.state: save, load into an instance with other values, and the audio
    is the original's, sample for sample. The restored params read back as saved."""
    tag = f"{rate:.0f}Hz/state"
    values = {"Depth": 7.0, "Detail": 6.3, "Mix": 60.0, "Out trim": -3.0, "Attack": 4.0, "Release": 6.0}
    settle, measure = settle_frames(rate), measure_frames(rate, [512])
    original = Instance(path)
    original.flush({**DEFAULTS, **values})
    original.activate(rate, 512)
    try:
        blob = original.save_state()
        total = original.latency() + settle + measure
        signal = [noise(total, 21), noise(total, 22)]
        expected = original.stream(signal, [512])
    finally:
        original.deactivate()
        original.close()
    restored = Instance(path)
    try:
        restored.flush({**DEFAULTS, "Depth": 2.0, "Mix": 10.0, "Out trim": 5.0, "Attack": 9.0, "Release": 1.0})
        restored.activate(rate, 512)
        restored.load_state(blob)
        read_back = {name: restored.value(name) for name in values}
        produced = restored.stream(signal, [512])
    finally:
        restored.deactivate()
        restored.close()
    drift = max(max_error(produced[c], expected[c]) for c in range(2))
    failures = [
        f"{tag}: {name} reads {read_back[name]} after load, saved {value}"
        for name, value in values.items() if abs(read_back[name] - value) > 1e-6
    ]
    if drift >= 1e-6:
        failures.append(f"{tag}: audio after the load is {drift:.2e} from the original")
    return {"config": tag, "metrics": {"blob_bytes": len(blob), "audio_drift": drift}, "failures": failures}


def check_outside_processing(path, rate):
    """process() before start_processing and after stop_processing. A host that
    makes the call must not crash, and the call must leave the plug-in as it was:
    the steady audio matches an instance that never had the stray calls. The
    statuses are recorded; moose processes the block either way."""
    tag = f"{rate:.0f}Hz/outside-processing"
    settle, measure = settle_frames(rate), measure_frames(rate, [512])
    stray = [noise(512, 31)]
    instance = Instance(path)
    instance.flush({**DEFAULTS, "Mix": 0.0})
    try:
        assert instance.plugin.activate(instance.pointer, rate, 1, 512), "activate failed"
        instance.process(stray, 512, strict=False)
        before = instance.last_status
        assert instance.plugin.start(instance.pointer), "start failed"
        instance.process(stray, 512)
        instance.plugin.stop(instance.pointer)
        instance.process(stray, 512, strict=False)
        after = instance.last_status
        instance.plugin.deactivate(instance.pointer)
        instance.activate(rate, 512)
        instance.plugin.reset(instance.pointer)
        latency = instance.latency()
        total = latency + settle + measure
        signal = [noise(total, 33)]
        produced = instance.stream(signal, [512])[0]
    finally:
        instance.deactivate()
        instance.close()
    control = Instance(path)
    control.flush({**DEFAULTS, "Mix": 0.0})
    control.activate(rate, 512)
    try:
        expected = control.stream(signal, [512])[0]
    finally:
        control.deactivate()
        control.close()
    drift = max_error(produced[latency + settle:], expected[latency + settle:])
    metrics = {"status_before_start": before, "status_after_stop": after, "audio_drift": drift}
    failures = [f"{tag}: process returned CLAP_PROCESS_ERROR outside processing"] if CLAP_PROCESS_ERROR in (before, after) else []
    if drift >= TOLERANCE:
        failures.append(f"{tag}: the stray calls changed the audio by {drift:.2e}")
    return {"config": tag, "metrics": metrics, "failures": failures}


def check_cycles(path):
    """Activate, process and deactivate again, each cycle at a new rate and block
    size. The dry path comes back steady every time."""
    cycles = [(44100.0, 512), (192000.0, 4096), (48000.0, 1), (96000.0, 333), (176400.0, 64), (88200.0, 2048), (44100.0, 16)]
    instance = Instance(path)
    instance.flush({**DEFAULTS, "Mix": 0.0})
    failures, metrics = [], {}
    try:
        for index, (rate, block) in enumerate(cycles):
            instance.activate(rate, block)
            settle, measure = settle_frames(rate), measure_frames(rate, [block])
            latency = instance.latency()
            total = latency + settle + measure
            signal = [noise(total, 41 + index)]
            produced = instance.stream(signal, [block])[0]
            error = steady_error(produced, signal[0], latency, settle, total)
            metrics[f"cycle_{index}"] = {"rate": rate, "block": block, "steady_error": error}
            if error >= TOLERANCE:
                failures.append(f"cycles: cycle {index} ({rate:.0f} Hz, {block}-frame blocks) steady error {error:.2e}")
            instance.deactivate()
    finally:
        instance.close()
    return {"config": "cycles", "metrics": metrics, "failures": failures}


def check_controls(path, rate, channels):
    """Each control moves the audio by the amount it should, steady. One instance,
    one flush per setting: Mix is linear (Mix 50 is the mean of Mix 0 and Mix 100),
    Mix 100 differs from Mix 0, Depth 0 is the dry signal, Depth 10 takes some
    level off it (at least DEPTH_MIN_DB: the amount varies with the rate, see the
    report), and Bypass is the dry signal and not the wet one."""
    tag = f"{rate:.0f}Hz/{channels}ch/controls"
    pattern = PATTERNS["varying"]
    settle, measure = settle_frames(rate), measure_frames(rate, pattern)
    instance = Instance(path)
    instance.flush(DEFAULTS)
    instance.activate(rate, max(pattern))
    try:
        latency = instance.latency()
        total = latency + settle + measure
        signal = [noise(total, 1 + c) for c in range(channels)]

        def steady(values):
            # The wet path carries its reduction state from the stream before,
            # so each setting starts from a reset and the comparison is fair.
            instance.flush({**DEFAULTS, **values})
            instance.plugin.reset(instance.pointer)
            return instance.stream(signal, pattern)[0][latency + settle: total]

        dry, wet, half = steady({"Mix": 0.0}), steady({"Mix": 100.0}), steady({"Mix": 50.0})
        depth_zero, depth_ten = steady({"Mix": 100.0, "Depth": 0.0}), steady({"Mix": 100.0, "Depth": 10.0})
        bypassed = steady({"Bypass": 1.0})
    finally:
        instance.deactivate()
        instance.close()
    metrics = {
        "mix_50_error_from_mean": max_error(half, [(a + b) / 2 for a, b in zip(dry, wet)]),
        "mix_100_vs_mix_0": relative_difference(wet, dry),
        "depth_0_error_from_dry": max_error(depth_zero, dry),
        "depth_10_change_db": 20 * math.log10(rms(depth_ten) / rms(depth_zero)),
        "bypass_error_from_dry": max_error(bypassed, dry),
        "bypass_vs_mix_100": relative_difference(bypassed, wet),
    }
    failures = []
    if metrics["mix_50_error_from_mean"] >= TOLERANCE:
        failures.append(f"{tag}: Mix 50 is {metrics['mix_50_error_from_mean']:.2e} from the mean of Mix 0 and Mix 100")
    if metrics["mix_100_vs_mix_0"] < 0.01:
        failures.append(f"{tag}: Mix 100 is {metrics['mix_100_vs_mix_0']:.4f} (relative RMS) from Mix 0")
    if metrics["depth_0_error_from_dry"] >= TOLERANCE:
        failures.append(f"{tag}: Depth 0 is {metrics['depth_0_error_from_dry']:.2e} from the dry signal")
    if metrics["depth_10_change_db"] > -DEPTH_MIN_DB:
        failures.append(f"{tag}: Depth 10 changes Depth 0 by {metrics['depth_10_change_db']:.2f} dB, expected at least -{DEPTH_MIN_DB} dB")
    if metrics["bypass_error_from_dry"] >= TOLERANCE:
        failures.append(f"{tag}: Bypass is {metrics['bypass_error_from_dry']:.2e} from the dry signal")
    if metrics["bypass_vs_mix_100"] < 0.01:
        failures.append(f"{tag}: Bypass is {metrics['bypass_vs_mix_100']:.4f} (relative RMS) from Mix 100")
    return {"config": tag, "metrics": metrics, "failures": failures}


def check_tail(path, rate):
    """A ringing engine says so twice: clap.tail reports its tail, and process
    returns CLAP_PROCESS_TAIL (3). A host that reads 0 stops early."""
    tag = f"{rate:.0f}Hz/tail"
    instance = Instance(path)
    instance.flush(DEFAULTS)
    instance.activate(rate, 512)
    try:
        tail = instance.tail()
        instance.process([[0.1] * 512, [0.1] * 512], 512)
        status = instance.last_status
        failures = []
        if tail <= 0:
            failures.append(f"{tag}: clap.tail reports {tail}; the engine rings after its input stops")
        if status != 3:
            failures.append(f"{tag}: process returned status {status}, expected CLAP_PROCESS_TAIL (3)")
        return {"config": tag, "metrics": {"tail": tail, "status": status}, "failures": failures}
    finally:
        instance.deactivate()
        instance.close()


def main():
    arguments = sys.argv[1:]
    quick = "--quick" in arguments
    path = pathlib.Path([a for a in arguments if not a.startswith("--")][0]).resolve()
    rates = QUICK_RATES if quick else RATES
    patterns = QUICK_PATTERNS if quick else list(PATTERNS)
    results = [guarded("ports", check_ports, path), guarded("cycles", check_cycles, path)]
    edges = {
        "flush": edge_flush, "silence": edge_silence, "no-input-bus": edge_no_input_bus,
        "mono-in-stereo-out": lambda p, r: edge_channels(p, r, 1, 2),
        "stereo-in-mono-out": lambda p, r: edge_channels(p, r, 2, 1),
    }
    for rate in rates:
        for pattern in patterns:
            for channels in (1, 2):
                for in_place in (False, True):
                    name = f"{rate:.0f}Hz/{pattern}/{channels}ch/{'in-place' if in_place else 'split'}"
                    results.append(guarded(name, check_config, path, rate, pattern, channels, in_place))
        for channels in (1, 2):
            results.append(guarded(f"{rate:.0f}Hz/{channels}ch/controls", check_controls, path, rate, channels))
        for name, check in edges.items():
            results.append(guarded(f"{rate:.0f}Hz/{name}", check, path, rate))
        results.append(guarded(f"{rate:.0f}Hz/state", check_state, path, rate))
        results.append(guarded(f"{rate:.0f}Hz/outside-processing", check_outside_processing, path, rate))
        results.append(guarded(f"{rate:.0f}Hz/editor-cycle", check_lifecycle, path, rate))
        results.append(guarded(f"{rate:.0f}Hz/tail", check_tail, path, rate))
    failing = [r for r in results if r["failures"]]
    report = {
        "artifact": str(path),
        "configurations": len(results),
        "failing": len(failing),
        "results": results,
    }
    print(json.dumps(report, indent=2, default=str))
    for result in failing:
        for message in result["failures"]:
            print(f"FAIL {message}", file=sys.stderr)
    return 1 if failing else 0


if __name__ == "__main__":
    sys.exit(main())
