#!/usr/bin/env python3
"""Source assertions and independent boundary model; does NOT execute Rust."""
import hashlib
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[3]
BASE = "13c044d6f9f80576507521a2dc6521e9aad7d9c5"
KSP = "crates/sampler-ksp/"


def base(path):
    return subprocess.check_output(["git", "-C", str(ROOT), "show", f"{BASE}:{path}"], text=True)


def source(name):
    return (ROOT / KSP / "src" / name).read_text()


def driver(text):
    return text[text.index("    /// A timer listener's driver:"):text.index("    fn service(&mut self, builtin:")]


sema, evaluator, lower = [source(p) for p in ("sema.rs", "eval.rs", "lower.rs")]
assert driver(lower) == driver(base(KSP + "src/lower.rs")), "timer driver changed"
helper = sema.split("pub(crate) fn listener_range", 1)[1].split("fn is_literal", 1)[0]
assert "TIMER_MS => Ok((1000, i32::MAX))" in helper
assert "TIMER_BEAT => Ok((1, 24))" in helper
assert "TRANSP_START | TRANSP_STOP if b == Builtin::SetListener => Ok((1, 1))" in helper
assert "_ => fault" in helper
assert "fold(&self.hir, signal)" in sema and "fold(&self.hir, value)" in sema
init = evaluator.split("            SetListener | ChangeListenerPar => {", 1)[1].split("            builtin @", 1)[0]
assert "self.callback_type != b::cb::INIT" in init
assert init.index("listener_range") < init.index("self.st.model.listeners.insert")
assert "builtin == ChangeListenerPar && value == 0" in init
live = lower.split("            SetListener => {", 1)[1].split("            LoadArrayStr | SaveArrayStr =>", 1)[0]
assert 'return fault(span, "set_listener can only be used in on init")' in live
assert live.count("self.arg(args, 0, dst)?") == 1
assert live.count("self.arg(args, 1, t)?") == 1
assert "Key::Arg" not in live, "signal must not be evaluated twice"
assert "for signal in [b::signal::TIMER_MS, b::signal::TIMER_BEAT]" in live
assert live.index("jump_if_zero(t)") < live.index("self.in_range") < live.index("let invalid") < live.index("self.store(")
assert "self.land(zero)" in live and "self.land(invalid)" in live
assert "self.set(dst, 0)?" in live

# Boundary model of the documented admission contract, NOT the Rust VM.
cases = 0
for change in (False, True):
    for signal in (-1, 0, 1, 2, 3, 4, 5, 2147483647):
        for value in (-2147483648, -1, 0, 1, 24, 25, 999, 1000, 2147483647):
            if signal == 1:
                admitted = value >= 1000 or (change and value == 0)
            elif signal == 2:
                admitted = 1 <= value <= 24 or (change and value == 0)
            else:
                admitted = not change and signal in (3, 4) and value == 1
            ranges = {1: (1000, 2147483647), 2: (1, 24)}
            if not change:
                ranges.update({3: (1, 1), 4: (1, 1)})
            guarded = signal in ranges and ((change and value == 0) or ranges[signal][0] <= value <= ranges[signal][1])
            assert admitted == guarded, (change, signal, value)
            cases += 1

cache_hash = (ROOT / "crates/sampler-kontakt/build.rs").read_text()
cache_loader = (ROOT / "crates/sampler-kontakt/src/cache.rs").read_text()
assert '"../sampler-ksp/src"' in cache_hash
assert 'env!("KONTRA_IMPORT_HASH")' in cache_loader
assert "if actual != stamp" in cache_loader
untouched = [KSP + "src/lib.rs", KSP + "src/builtins.rs", KSP + "src/init_cache.rs",
             "crates/sampler-kontakt/build.rs", "crates/sampler-kontakt/src/cache.rs"]
# No tracked manifests or old source evidence may change in this worktree.
tracked = subprocess.check_output(["git", "-C", str(ROOT), "ls-files"], text=True).splitlines()
untouched += [p for p in tracked if Path(p).name in ("Cargo.toml", "Cargo.lock", "rust-toolchain.toml")]
for p in untouched:
    assert (ROOT / p).read_text() == base(p), p

tests = (ROOT / KSP / "tests/listener_contract.rs").read_text()
names = re.findall(r"#\[test\]\s*fn (\w+)", tests)
assert len(names) == 7
assert "set_listener($NI_SIGNAL_TIMER_BEAT,0)" not in (ROOT / KSP / "tests/tempo_waits.rs").read_text()
authority = json.loads((Path(__file__).parent / "primary-authority.json").read_text())
assert authority["document_sha256"] == "aafd0e71a266571cc5a6adb9dbfb15bb31f4c4819e9748427634d171b34fa2f9"
for section in authority["sections"]:
    assert hashlib.sha256(section["raw_html"].encode()).hexdigest() == section["raw_section_sha256"]

print(json.dumps({
    "base": BASE,
    "source_assertions": "PASS",
    "independent_boundary_model_cases": cases,
    "driver_sha256": hashlib.sha256(driver(lower).encode()).hexdigest(),
    "unchanged_files": untouched,
    "authored_tests": names,
    "rust_tests": "NOT_RUN",
    "rust_compilation": "NOT_COMPILED",
    "native": "UNKNOWN",
    "CPU_AND_RAM_vs_both_references": "UNACHIEVED",
    "scope_limit": "direct restore_initialized trusts supplied cached model; product cache stamp includes KSP sources and rejects old importer hashes",
}, indent=2))
