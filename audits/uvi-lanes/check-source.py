"""CURRENT13 static/semantic checks only. Does not execute Rust, Lua or audio."""
import hashlib
import json
import re
import subprocess
from html.parser import HTMLParser
from pathlib import Path

BASE = "13c044d6f9f80576507521a2dc6521e9aad7d9c5"
ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
SCRATCH = Path("/home/derpcat/.t3/scratch/2026-10-10-stop-this-thread-2336e1f0-2f34-7f6415ff")


def git(*args):
    return subprocess.check_output(["git", "-C", str(ROOT), *args], text=True).strip()


def sha(data):
    return hashlib.sha256(data).hexdigest()


# Exact source evidence, not a re-use of historical handoff line numbers.
SPANS = [
    ("crates/sampler-uvi/src/engine_parameters.rs", "field/binding/bindings/register/initialize", 10, 154),
    ("crates/sampler-uvi/src/inserts.rs", "insert identity, bypass and Gain", 13, 97),
    ("crates/sampler-uvi/src/inserts.rs", "OnePole static key-tracked approximation", 207, 230),
    ("crates/sampler-uvi/src/inserts.rs", "placed count", 300, 309),
    ("crates/sampler-uvi/src/lib.rs", "translate_full register and validation", 301, 312),
    ("crates/sampler-uvi/src/lib.rs", "attach_script initial defaults and Driver", 1086, 1131),
    ("crates/sampler-uvi/src/lib.rs", "streamed final bindings", 1402, 1447),
    ("crates/sampler-uvi/src/lib.rs", "resident script init overrides before lower", 1509, 1542),
    ("crates/sampler-uvi/src/lib.rs", "streamed script init overrides before lower", 1603, 1620),
    ("crates/sampler-uvi/src/lib.rs", "resident final bindings", 1680, 1712),
    ("crates/sampler-uvi/src/script.rs", "bounded local command admission", 656, 666),
    ("crates/sampler-uvi/src/script.rs", "initial install/build/load ordering", 1002, 1007),
    ("crates/sampler-uvi/src/script.rs", "type-only binding to queue", 1395, 1478),
    ("crates/sampler-uvi/src/script.rs", "initial overlay exports", 2354, 2381),
    ("crates/sampler-uvi/src/script_prelude.lua", "overlay read/write", 345, 403),
    ("crates/sampler-uvi/src/scripted.rs", "driver direct setter without ACK", 349, 382),
    ("crates/sampler-uvi/src/scripted/thread.rs", "ready report and command forwarding", 188, 280),
    ("crates/sampler-uvi/src/scripted/thread.rs", "audio command drain and clock tick", 439, 453),
    ("crates/sampler-kontakt/src/load.rs", "both preparation lower callers", 915, 1018),
    ("crates/sampler-core/src/lower.rs", "zone-consumed chains and all-controls schema", 371, 439),
    ("crates/sampler-core/src/lower.rs", "descriptor inventory from IR", 1077, 1151),
    ("crates/sampler-core/src/lower.rs", "bind_processor actual fields", 1588, 1635),
    ("crates/sampler-core/src/lower.rs", "lower_list requires consumed binding", 1685, 1755),
    ("crates/sampler-core/src/prepare.rs", "actual voice consumer construction", 1489, 1522),
    ("crates/sampler-core/src/lower.rs", "install consumed voice chains", 548, 556),
    ("src/sound/v2.rs", "product attach, nest, streamed assembly and returned driver", 3123, 3184),
    ("src/sound/v2.rs", "product render drives actual script wake", 1753, 1754),
    ("crates/sampler-core/src/bus.rs", "prepared bus consumer schema", 61, 77),
    ("crates/sampler-core/src/engine_parameters.rs", "binding validation not consumer validation", 392, 410),
    ("crates/sampler-core/src/engine_parameters.rs", "runtime setter and readback", 676, 803),
    ("crates/sampler-core/src/control.rs", "immediate accepted control write", 546, 567),
    ("crates/sampler-core/src/dsp/control.rs", "only consumed voice/bus ranges are edited", 251, 292),
    ("crates/sampler-core/src/ops.rs", "behavior outcome queue can drop and has no UVI token", 853, 886),
    ("crates/sampler-ir/src/lib.rs", "retain_zones does not remove controls/chains", 708, 751),
]


class Text(HTMLParser):
    def __init__(self):
        super().__init__()
        self.parts = []

    def handle_data(self, data):
        self.parts.append(data)


def plain(html):
    parser = Text()
    parser.feed(html)
    return " ".join(" ".join(parser.parts).split())


def primary_excerpts():
    element = SCRATCH / "handoff/official-docs/uvi-element.html"
    elements = Path("/home/derpcat/.t3/worktrees/KONTAKTO/t3code-80fe786b/artifacts/engine-analysis-2026-10-07/uvi-elements-current-20261009.html")
    expected = [
        (element, "b4516f54b9fd2061c5ba134afc30930b4c7922a4ed78b3badd022a395e5b5200"),
        (elements, "ddca396e24ff7a05dbd3be9b2b073cb5a4a7adfb67fbd00d75e597f6c0a08d0f"),
    ]
    for path, digest in expected:
        assert sha(path.read_bytes()) == digest, path
    out = []
    raw = element.read_text()
    for anchor, assertion in [
        ("a876941566822f111c98750e3c954a94c", "is silently ignored"),
        ("a249fb1fa90b51d0631486cb422b05841", "parameterValue (number|bool|string)"),
    ]:
        start = raw.index(f'<a id="{anchor}"')
        end = raw.find('<a id="', start + 1)
        section = raw[start:end if end >= 0 else len(raw)]
        excerpt = plain(section)
        assert assertion in excerpt
        out.append(dict(url=f"https://lua.uvi.net/class_element.html#{anchor}", document_sha256=expected[0][1], section_sha256=sha(section.encode()), excerpt=excerpt))
    raw = elements.read_text()
    for kind, field, bounds in [("OnePole", "Freq", ["20.000", "20000.000", "1000.000"]), ("Gain", "Volume", ["0.000", "3.981", "1.000"])]:
        start = raw.index(f'id="{kind}"')
        table_start = raw.index('<table', start)
        table_end = raw.index('</table>', table_start) + len('</table>')
        table = raw[table_start:table_end]
        row = next(row for row in re.findall(r'<tr\b.*?</tr>', table, re.S) if re.search(rf'<td>\s*{field}\s*</td>', row))
        excerpt = plain(row)
        assert all(value in excerpt for value in bounds), excerpt
        out.append(dict(url=f"https://lua.uvi.net/_elements.html#{kind}", document_sha256=expected[1][1], section_sha256=sha(row.encode()), excerpt=excerpt))
    return out


def semantic_model():
    # ponytail: small independent model, not a Rust/Lua execution substitute.
    cutoff = (3, -1, -1, 20)
    gain = (0, -1, -1, 21)
    orphan = (3, -1, -1, 22)
    candidates = [(cutoff, "cutoff"), (gain, "gain"), (orphan, "orphan")]
    voice = [("cutoff", 0)]
    bus = [("gain", 0)]
    ranges = {"voice": ["cutoff"], "bus": ["gain"]}
    consumers = set()
    for scope, entries in [("voice", voice), ("bus", bus)]:
        for control, lane in entries:
            assert 0 <= lane < len(ranges[scope]) and ranges[scope][lane] == control
            consumers.add(control)
    inventory = {address for address, control in candidates if control in consumers}
    assert inventory == {cutoff, gain} and orphan not in inventory
    assert (3, 0, -1, 20) not in inventory  # preserve all four fields

    epoch, next_sequence, capacity = 7, 1, 2
    pending, committed, requests, acknowledgements = [], {cutoff: 1000}, [], []

    def admit(address, value, request_epoch):
        nonlocal next_sequence
        if request_epoch != epoch or address not in inventory or len(pending) >= capacity:
            return None
        token = (request_epoch, next_sequence)
        next_sequence += 1
        pending.append((token, address, value))
        requests.append((token, address, value))
        return token

    def apply(success, active_epoch):
        if len(acknowledgements) == 1 or not requests:
            return False  # reserve reverse space before application
        token, address, value = requests.pop(0)
        success = success and token[0] == active_epoch
        acknowledgements.append((token, address, success, value))
        return True

    def consume(record):
        if not pending or record[:2] != pending[0][:2] or record[0][0] != epoch:
            return False
        token, address, success, value = record
        pending.pop(0)
        if success:
            committed[address] = value
        return True

    def read(address):
        return next((value for _, a, value in reversed(pending) if a == address), committed[address])

    assert admit(orphan, 600, epoch) is None
    assert admit(cutoff, 600, epoch - 1) is None
    first = admit(cutoff, 600, epoch)
    second = admit(cutoff, 800, epoch)
    assert first != second and read(cutoff) == 800 and committed[cutoff] == 1000
    assert admit(cutoff, 900, epoch) is None
    assert apply(True, epoch)
    assert not apply(False, epoch) and len(requests) == 1
    assert not consume((second, cutoff, True, 800))  # out-of-order ACK
    record = acknowledgements.pop(0)
    assert consume(record) and committed[cutoff] == 600 and read(cutoff) == 800
    assert not consume(record)  # duplicate must not remove newer pending write
    assert apply(False, epoch)
    assert consume(acknowledgements.pop(0)) and read(cutoff) == 600
    assert not pending
    assert admit(cutoff, 900, epoch)
    assert apply(True, epoch + 1)  # reload causes rejection, not success on new plan
    assert consume(acknowledgements.pop(0)) and committed[cutoff] == 600
    assert not pending and not requests and not acknowledgements
    return "PASS_INDEPENDENT_PYTHON_MODEL_ONLY"


def main():
    assert git("rev-parse", f"{BASE}^{{tree}}") == "4af7c9babd93c49cdb0a026a63d25e6c433498b0"
    changed = git("diff", BASE, "--name-only").splitlines()
    untracked = git("ls-files", "--others", "--exclude-standard").splitlines()
    assert all(p.startswith("audits/uvi-lanes/") for p in changed + untracked)
    spans = []
    for path, symbol, start, end in SPANS:
        data = (ROOT / path).read_bytes()
        original = subprocess.check_output(["git", "-C", str(ROOT), "show", f"{BASE}:{path}"])
        assert data == original, path
        lines = data.splitlines(keepends=True)
        assert 0 < start <= end <= len(lines), (path, start, end)
        spans.append(dict(commit=BASE, path=path, symbol=symbol, lines=[start, end], git_blob=git("rev-parse", f"{BASE}:{path}"), span_sha256=sha(b"".join(lines[start - 1:end]))))
    source = (HERE / "characterization.rs").read_text()
    tests = re.findall(r'#\[test\]\s*fn (\w+)', source)
    assert len(tests) == 4
    assert not any(s in source for s in ["#[ignore]", "std::fs", "render(", "Bank::", "load_program("])
    result = dict(
        status="DESIGN_AND_DETACHED_CHARACTERIZATION_SOURCE_ONLY",
        source_commit=BASE,
        source_tree=git("rev-parse", f"{BASE}^{{tree}}"),
        product_files_unchanged=True,
        source_spans=spans,
        primary_spec_authority="Archived UVI primary specifications only; no native timing/ACK or numeric parity claim",
        primary_excerpts=primary_excerpts(),
        python_semantic_check=semantic_model(),
        rust_tests={"status": "NOT_RUN/NOT_COMPILED", "names": tests, "target_included": False},
        native="UNKNOWN",
        comparative_cpu_and_ram_both_references="UNACHIEVED",
        authored_files={p.name: sha(p.read_bytes()) for p in [HERE / "README.md", HERE / "characterization.rs", Path(__file__)]},
        limits=["No product fix", "Initialization reconciliation unresolved", "Independent Python model is not Rust execution", "rustfmt is syntax only", "No native/PCM/performance evidence"],
    )
    (HERE / "evidence.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps({"static": "PASS", "python_model": result["python_semantic_check"], "spans": len(spans), "tests": tests, "rust": "NOT_RUN/NOT_COMPILED"}, indent=2))


if __name__ == "__main__":
    main()
