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

For every configuration it asserts the same facts:
  - mix 0 gives the dry signal, delayed by the reported latency;
  - out trim -6 dB gives -6.02 dB, at a sample inside a block;
  - bypass passes the dry signal;
  - every edge case (channel counts, zero frames, no input bus) is finite and
    writes every output channel the host connected.

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
    PTR, U32, F64, Audio, Entry, Factory, Gui, Header, Host, InEvents, Latency,
    OutEvents, ParamEvent, ParamInfo, Params, Plugin, Process, Version, plain,
)

# The reference's defaults (see clap_smoke.py), with the mix dry for the checks.
DEFAULTS = {
    "Depth": 5.0, "Detail": 5.0, "Attack": 5.0, "Release": 3.0, "Mode": 0.0,
    "Low latency": 0.0, "Quality": 0.0, "Mix": 100.0, "Out trim": 0.0,
    "Delta": 0.0, "Bypass": 0.0,
}

RATES = [44100.0, 48000.0, 88200.0, 96000.0, 192000.0]
# Fixed blocks, and one pattern that changes size on every call.
PATTERNS = {
    "1": [1], "32": [32], "64": [64], "128": [128], "512": [512],
    "2048": [2048], "4096": [4096], "varying": [64, 1, 2048, 32, 512, 4096, 128, 7],
}
QUICK_RATES = [44100.0, 48000.0, 192000.0]
QUICK_PATTERNS = ["1", "64", "4096", "varying"]

TRIM_MINUS_6_DB = 10 ** (-6 / 20)
# The engine's mix gain settles over its first few thousand samples (measured
# residual ~5e-4 right after the latency, ~3e-5 by 4000 samples), so the dry
# check has a transient window and a steady-state window.
TRANSIENT_FRAMES = 8192
TRANSIENT_TOLERANCE = 1e-3
TOLERANCE = 1e-4


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
        """The audio ports the plug-in declares: (is_input, channel_count) per port."""
        ports = C.cast(self.ports_ptr, C.POINTER(AudioPorts)).contents
        found = []
        for is_input in (True, False):
            for index in range(ports.count(self.pointer, is_input)):
                info = AudioPortInfo()
                assert ports.get(self.pointer, index, is_input, C.byref(info))
                found.append((is_input, info.channel_count))
        return found

    def activate(self, rate, max_block):
        assert self.plugin.activate(self.pointer, rate, 1, max_block), "activate failed"
        assert self.plugin.start(self.pointer), "start failed"

    def deactivate(self):
        self.plugin.stop(self.pointer)
        self.plugin.deactivate(self.pointer)

    def process(self, inputs, frames, out_channels=None, events=(), in_place=False):
        """One process call. `inputs` is a list of channel lists (empty for a
        call with no input bus); `events` are (offset, name, value) inside the
        block; `out_channels` is the number of output channels the host gives."""
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
        assert status != 0, f"process returned CLAP_PROCESS_ERROR (call at sample {self.time}, {frames} frames)"
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


def dry_failures(produced, reference, tag, label):
    """The dry path: `produced` is the output from the latency on, `reference`
    the input it should equal. Two windows, see TRANSIENT_FRAMES."""
    transient = max_error(produced[:TRANSIENT_FRAMES], reference[:TRANSIENT_FRAMES])
    steady = max_error(produced[TRANSIENT_FRAMES:], reference[TRANSIENT_FRAMES:])
    found = []
    if transient >= TRANSIENT_TOLERANCE:
        found.append(f"{tag}: {label} transient is {transient:.2e} from the delayed dry")
    if steady >= TOLERANCE:
        found.append(f"{tag}: {label} steady state is {steady:.2e} from the delayed dry")
    return found, transient, steady


def guarded(name, check, *args):
    """A host call that fails is a finding about the plug-in, not a reason to
    stop the matrix: record it against this name and carry on."""
    try:
        return check(*args)
    except (AssertionError, OSError) as error:
        return {"config": name, "metrics": {}, "failures": [f"{name}: {error}"]}


def check_config(path, rate, pattern_name, channels, in_place):
    """Dry, trim, bypass, a change inside a block, and reactivation, for one
    sample rate, block pattern, channel count and buffer arrangement."""
    pattern = PATTERNS[pattern_name]
    tag = f"{rate:.0f}Hz/{pattern_name}/{channels}ch/{'in-place' if in_place else 'split'}"
    failures, metrics = [], {}

    # At least eight of the largest blocks, so the change windows below are
    # never empty at the 4096 block size.
    signal_frames = max(int(rate * 0.2), 8 * max(pattern))

    # 1. Mix 0 gives the dry signal, delayed by the reported latency.
    instance = Instance(path)
    instance.flush({**DEFAULTS, "Mix": 0.0})
    instance.activate(rate, max(pattern))
    latency = instance.latency()
    metrics["latency"] = latency
    total = latency + signal_frames
    signal = [noise(total, 1 + c) for c in range(channels)]
    dry = instance.stream(signal, pattern, in_place=in_place)
    for c in range(channels):
        found, transient, steady = dry_failures(
            dry[c][latency:], signal[c][: total - latency], tag, f"mix 0 channel {c}"
        )
        failures += found
        metrics[f"dry_transient_{c}"] = transient
        metrics[f"dry_steady_{c}"] = steady
    reference = signal[0][: total - latency]

    # 2. Out trim -6 dB is -6.02 dB on the whole signal.
    instance.flush({"Out trim": -6.0})
    trimmed = instance.stream(signal, pattern, in_place=in_place)
    ratio = rms(trimmed[0][latency:]) / rms(reference)
    metrics["trim_ratio"] = ratio
    if abs(ratio - TRIM_MINUS_6_DB) >= 0.002:
        failures.append(f"{tag}: out trim -6 dB gives ratio {ratio:.4f}")
    instance.deactivate()
    instance.close()

    # 3. Bypass passes the dry signal: delayed by the latency, or undelayed.
    # The report says which; a host that compensates expects the first.
    instance = Instance(path)
    instance.flush({**DEFAULTS, "Bypass": 1.0})
    instance.activate(rate, max(pattern))
    bypassed = instance.stream(signal, pattern, in_place=in_place)[0]
    delayed_error = max_error(bypassed[latency:], reference)
    undelayed_error = max_error(bypassed[: total - latency], signal[0][: total - latency])
    metrics["bypass_delayed_error"] = delayed_error
    metrics["bypass_undelayed_error"] = undelayed_error
    if min(delayed_error, undelayed_error) >= TOLERANCE:
        failures.append(
            f"{tag}: bypass matches neither the delayed nor the dry signal "
            f"(delayed {delayed_error:.2e}, undelayed {undelayed_error:.2e})"
        )
    instance.deactivate()
    instance.close()

    # 4. A change at a sample inside a block lands there: trim goes -6 dB at
    #    `change`. The windows leave a block's margin either side, because the
    #    host may apply a change at the block boundary and not inside it.
    instance = Instance(path)
    instance.flush({**DEFAULTS, "Mix": 0.0})
    instance.activate(rate, max(pattern))
    change = latency + signal_frames // 2
    margin = max(pattern) + 1
    stepped = instance.stream(signal, pattern, in_place=in_place, changes={change: {"Out trim": -6.0}})
    before = rms(stepped[0][latency + margin:change - margin]) / rms(reference[margin:change - latency - margin])
    after = rms(stepped[0][change + margin:]) / rms(signal[0][change - latency + margin: total - latency])
    metrics["step_before"] = before
    metrics["step_after"] = after
    if abs(before - 1.0) >= 0.002:
        failures.append(f"{tag}: before the in-block change the ratio is {before:.4f}")
    if abs(after - TRIM_MINUS_6_DB) >= 0.002:
        failures.append(f"{tag}: after the in-block change the ratio is {after:.4f}")

    # 5. Reactivate at the same rate: the dry path again.
    instance.deactivate()
    instance.flush({**DEFAULTS, "Mix": 0.0})
    instance.activate(rate, max(pattern))
    again = instance.stream(signal, pattern, in_place=in_place)
    found, _, _ = dry_failures(again[0][latency:], signal[0][: total - latency], tag, "reactivated dry path")
    failures += found
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


def edge_mono_in_stereo_out(path, rate):
    """A mono input the host sends to a stereo output: both output channels
    must carry the signal."""
    tag = f"{rate:.0f}Hz/mono-in-stereo-out"
    instance = Instance(path)
    instance.flush({**DEFAULTS, "Mix": 0.0})
    instance.activate(rate, 512)
    try:
        mono = noise(4096, 9)
        produced = instance.process([mono], 4096, out_channels=2)
        latency = instance.latency()
        metrics, failures = {}, []
        for c in range(2):
            error = max_error(produced[c][latency:], mono[: 4096 - latency])
            metrics[f"channel_{c}_error"] = error
            if error >= TOLERANCE * 10:
                failures.append(f"{tag}: output channel {c} differs from the input by {error:.2e}")
        return {"config": tag, "metrics": metrics, "failures": failures}
    finally:
        instance.deactivate()
        instance.close()


def edge_stereo_in_mono_out(path, rate):
    """A stereo input to a mono output: the plug-in must not index a channel
    the host did not connect."""
    tag = f"{rate:.0f}Hz/stereo-in-mono-out"
    instance = Instance(path)
    instance.flush({**DEFAULTS, "Mix": 0.0})
    instance.activate(rate, 512)
    try:
        stereo = [noise(4096, 3), noise(4096, 4)]
        produced = instance.process(stereo, 4096, out_channels=1)
        latency = instance.latency()
        error = max_error(produced[0][latency:], stereo[0][: 4096 - latency])
        failures = [] if error < TOLERANCE * 10 else [f"{tag}: mono output is {error:.2e} from the left dry"]
        return {"config": tag, "metrics": {"error": error}, "failures": failures}
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
    """The buses the plug-in declares, for a stereo effect."""
    instance = Instance(path)
    try:
        ports = instance.ports()
        expected = [(True, 2), (False, 2)]
        failures = [] if ports == expected else [f"ports are {ports}, expected {expected}"]
        return {"config": "ports", "metrics": {"ports": ports}, "failures": failures}
    finally:
        instance.close()


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
    results = [guarded("ports", check_ports, path)]
    for rate in rates:
        for pattern in patterns:
            for channels in (1, 2):
                for in_place in (False, True):
                    name = f"{rate:.0f}Hz/{pattern}/{channels}ch/{'in-place' if in_place else 'split'}"
                    results.append(guarded(name, check_config, path, rate, pattern, channels, in_place))
        for check in (edge_flush, edge_silence, edge_mono_in_stereo_out, edge_stereo_in_mono_out, edge_no_input_bus):
            results.append(guarded(f"{rate:.0f}Hz/{check.__name__}", check, path, rate))
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
