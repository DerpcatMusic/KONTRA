#!/usr/bin/env python3
"""Inspect local libraries and serve source/conversion reports from memory.

No plugin or audio device is opened. Source is opt-in; reports are not written
to disk. Initialization runs in a read-only Linux mount sandbox.
"""
from __future__ import annotations

import argparse
import copy
from functools import partial
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import secrets
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import webbrowser

ROOT = Path(__file__).resolve().parents[1]
VIEWER = Path(__file__).with_suffix(".html")
HEAVY = Path("/home/derpcat/.cache/kontakto-heavy")
PROGRESS_PHASES = frozenset(("snapshot_or_container", "snapshot_base", "container",
    "program_metadata", "source_frontend", "translate_and_initialize", "ui_projection",
    "ui_render", "conversion_report_and_placeholder_plan", "bank_directory",
    "container_program_and_scripts", "xml_parse_and_metadata", "translate", "ui_model_export", "production_ui_lifecycle"))


def default_binary() -> Path:
    target = Path("/mnt/Windows11/DEV_WORKSPACE/Toolchains/User/cargo-target")
    optimized = [target / profile / "corpus-health" for profile in ("dsp-dev", "ci", "release")]
    optimized.append(target / f"kontakto-{ROOT.name}" / "ci" / "corpus-health")
    available = [path for path in optimized if path.is_file()]
    return max(available, key=lambda path: path.stat().st_mtime_ns) if available else target / "ci" / "corpus-health"


def inspect_command(binary: Path, path: str, *, initialize: bool,
                    source: bool, program: str | None, ram: Path,
                    render: bool = False, base: str | None = None,
                    ui_only: bool = False, profile_init: bool = False,
                    settle_ui: bool = False) -> list[str]:
    initialize = initialize or render or ui_only or profile_init or settle_ui
    if "::" in path:
        path, member = path.split("::", 1)
        if program is not None and program != member:
            raise ValueError("The bank member in the path differs from --program.")
        program = member
    args = [str(binary), "inspect", path]
    if program is not None:
        args.extend(["--program", program])
    if base is not None:
        args.extend(["--base", base])
    if source:
        args.append("--ephemeral-source")
    if render:
        args.append("--ephemeral-render")
    if ui_only:
        args.append("--ui-only")
    if profile_init:
        args.append("--profile-init")
    if settle_ui:
        args.append("--settle-ui")
    if initialize:
        args.append("--initialize")
    sandbox = shutil.which("bwrap")
    if sandbox:
        # Only this owned RAM directory is writable. Library paths and the
        # remainder of the user's filesystem retain read-only mounts.
        args = [sandbox, "--die-with-parent", "--new-session", "--ro-bind", "/", "/",
                "--dev-bind", "/dev", "/dev", "--proc", "/proc",
                "--bind", str(ram), str(ram), "--", *args]
    elif initialize:
        raise RuntimeError("Initialization requires bubblewrap (bwrap) to keep library files read-only. Static inspection remains available.")
    if HEAVY.is_file():
        args.insert(0, str(HEAVY))
    return args


def inspect_environment(ram: Path, initialize: bool) -> dict[str, str]:
    env = os.environ.copy()
    env.update(KONTRA_SCAN_ACTIVE="1", KONTRA_DISABLE_NETWORK="1",
               KONTRA_INSPECT_PROGRESS="1",
               MUI_REPORTING_DISABLED="1", XDG_CACHE_HOME=str(ram / "cache"),
               KONTRA_LOG_DIR=str(ram / "logs"),
               KONTRA_REPORT_DIR=str(ram / "reports"))
    if initialize:
        env["KONTRA_INSPECT_READ_ONLY_INIT"] = "1"
    else:
        env.pop("KONTRA_INSPECT_READ_ONLY_INIT", None)
    for child in ("cache", "logs", "reports"):
        (ram / child).mkdir(exist_ok=True)
    return env


def last_progress(stderr: bytes) -> str | None:
    """Use only the inspector's bounded numeric markers, never raw diagnostics."""
    phase = None
    for line in stderr.splitlines():
        if not line.startswith(b"INSPECT_PROGRESS ") or len(line) > 512:
            continue
        try:
            record = json.loads(line[len(b"INSPECT_PROGRESS "):])
        except (ValueError, UnicodeError):
            continue
        name = record.get("phase") if isinstance(record, dict) else None
        if isinstance(name, str) and name in PROGRESS_PHASES:
            phase = name + (" (started)" if record.get("status") == "started" else " (completed)")
    return phase


def validate_reports(value: object) -> list[dict]:
    records = value if isinstance(value, list) else [value]
    if not records or any(not isinstance(r, dict) or
                          r.get("format") != "kontra-library-inspector" or
                          r.get("schema") != 1 or
                          not isinstance(r.get("scripts"), list) or
                          not isinstance(r.get("stages"), list)
                          for r in records):
        raise ValueError("Expected a KONTRA Library Inspector schema 1 report.")
    return records


def inspect_one(binary: Path, path: str, *, initialize: bool, source: bool,
                program: str | None, ram: Path, timeout: int,
                render: bool = False, base: str | None = None,
                ui_only: bool = False, profile_init: bool = False,
                settle_ui: bool = False) -> list[dict]:
    initialize = initialize or render or ui_only or profile_init or settle_ui
    started = time.monotonic()
    command = inspect_command(binary, path, initialize=initialize,
                              source=source, program=program, ram=ram, render=render, base=base, ui_only=ui_only, profile_init=profile_init, settle_ui=settle_ui)
    process = subprocess.Popen(command, cwd=ROOT,
                               env=inspect_environment(ram, initialize),
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                               start_new_session=True)
    try:
        stdout, _stderr = process.communicate(timeout=timeout)
    except (subprocess.TimeoutExpired, KeyboardInterrupt) as error:
        # The wrapper, sandbox and compiler/frontend belong to this job. Stop
        # its process group rather than leaving a library reader orphaned.
        os.killpg(process.pid, signal.SIGTERM)
        try:
            _stdout, stderr = process.communicate(timeout=2)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            _stdout, stderr = process.communicate()
        if isinstance(error, subprocess.TimeoutExpired):
            progress = last_progress(stderr or error.stderr or b"")
            raise RuntimeError(f"Inspection timed out after {timeout}s; last recorded phase: {progress or 'none (possibly waiting for the heavy-job queue)'}. Increase --timeout or use --ui-only to separate source-audit overhead from production UI loading.") from error
        raise
    try:
        # Failed-stage reports remain useful and may have a nonzero exit code.
        reports = validate_reports(json.loads(stdout))
        for report in reports:
            report.setdefault("performance", {})["launcher_ms"] = (time.monotonic() - started) * 1000
        return reports
    except (ValueError, UnicodeError) as error:
        # Never print captured diagnostics: vendor code or values may occur in
        # a compiler's error text. The source-aware report owns safe diagnostics.
        raise RuntimeError(f"Inspector did not produce a valid report (exit {process.returncode}); rebuild corpus-health with inspect support.") from error


class ReportHandler(BaseHTTPRequestHandler):
    def __init__(self, *args, token: str, html: bytes, report: bytes,
                 images: dict[str, bytes], **kwargs):
        self.token, self.html, self.report, self.images = token, html, report, images
        super().__init__(*args, **kwargs)

    def log_message(self, *args):
        pass

    def do_GET(self):
        host = self.headers.get("Host", "")
        allowed = {f"127.0.0.1:{self.server.server_port}",
                   f"localhost:{self.server.server_port}"}
        if host not in allowed:
            self.send_error(403, "Invalid local host")
            return
        prefix = f"/{self.token}/"
        if self.path == prefix:
            content, mime = self.html, "text/html; charset=utf-8"
        elif self.path == prefix + "report":
            content, mime = self.report, "application/json; charset=utf-8"
        elif self.path.startswith(prefix) and self.path[len(prefix):] in self.images:
            content, mime = self.images[self.path[len(prefix):]], "image/png"
        else:
            self.send_error(404)
            return
        self.send_response(200)
        self.send_header("Content-Type", mime)
        self.send_header("Content-Length", str(len(content)))
        self.send_header("Cache-Control", "no-store")
        self.send_header("X-Content-Type-Options", "nosniff")
        self.send_header("X-Frame-Options", "DENY")
        self.send_header("Referrer-Policy", "no-referrer")
        self.send_header("Content-Security-Policy", "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; connect-src 'self'; img-src 'self' blob:; base-uri 'none'; form-action 'none'")
        self.end_headers()
        self.wfile.write(content)


def make_server(reports: list[dict], port: int = 0) -> tuple[ThreadingHTTPServer, str]:
    token = secrets.token_urlsafe(24)
    reports = copy.deepcopy(reports)
    images = {}
    for report_index, report in enumerate(reports):
        for face_index, face in enumerate(report.get("ui", {}).get("interfaces", [])):
            for shot_index, shot in enumerate(face.get("render", {}).get("shots", [])):
                encoded = shot.pop("png_hex", None)
                if encoded:
                    if not isinstance(encoded, str) or len(encoded) > (64 << 20):
                        raise ValueError("Invalid or oversized ephemeral UI image")
                    pixels = bytes.fromhex(encoded)
                    if not pixels.startswith(b"\x89PNG\r\n\x1a\n"):
                        raise ValueError("Expected an ephemeral PNG witness")
                    key = f"image/{report_index}/{face_index}/{shot_index}"
                    images[key] = pixels
                    shot["image_url"] = key
    handler = partial(ReportHandler, token=token, html=VIEWER.read_bytes(),
                      report=json.dumps(reports, ensure_ascii=False).encode(), images=images)
    server = ThreadingHTTPServer(("127.0.0.1", port), handler)
    return server, f"http://127.0.0.1:{server.server_port}/{token}/"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("paths", nargs="*", help="NKI/NKM/NKSN/UVIP, or bank.ufs::program.uvip")
    parser.add_argument("--binary", type=Path, default=default_binary())
    parser.add_argument("--initialize", action="store_true", help="Run script initialization and conversion in a read-only sandbox; no musical callbacks or audio rendering")
    parser.add_argument("--render", action="store_true", help="Initialize once and render original UI through Kontra's production CPU painter; requires corpus-health --features ui-render. Pixels stay in memory")
    parser.add_argument("--ui-only", action="store_true", help="Initialize the production UI without the separate static source/lowering audit; use with --render for faster UI diagnosis")
    parser.add_argument("--settle-ui", action="store_true", help="Kontakt only: prepare real streaming/runtime once and step bounded plan activation before rendering initial/settled UI; no plugin/DAW/device. Authored startup/listener callbacks may run, output is discarded")
    parser.add_argument("--profile-init", action="store_true", help="Opt in to initialization phase clocks, native API timings and bounded Lua safepoint attribution; no source/arguments/heap capture")
    parser.add_argument("--source", action="store_true", help="Include original library code in this memory-only view")
    parser.add_argument("--program", help="Kontakt multi index or UVI bank program member")
    parser.add_argument("--base", help="Explicit original base NKI for an NKSN snapshot; must pass the shared production identity check")
    parser.add_argument("--report-json", action="append", type=Path, default=[], help="Open an existing source-free inspection report")
    parser.add_argument("--port", type=int, default=0)
    parser.add_argument("--timeout", type=int, default=240, help="Per-file bound, including the shared heavy-job queue")
    parser.add_argument("--no-open", action="store_true")
    args = parser.parse_args()
    if args.paths and not args.binary.is_file():
        parser.error("Build the inspector first: kontakto-heavy cargo build --locked --profile ci -p corpus-health --features ui-render")
    if args.timeout < 1:
        parser.error("--timeout must be positive")
    if (args.ui_only or args.settle_ui) and args.source:
        parser.error("--ui-only skips source classification; omit --source or use a full inspection")
    reports = []
    try:
        for path in args.report_json:
            reports.extend(validate_reports(json.loads(path.read_bytes())))
        # Protected code and initialized cache data may only use a RAM mount.
        if args.paths and not Path("/dev/shm").is_dir():
            raise RuntimeError("This launcher requires /dev/shm for ephemeral inspection data. Use corpus-health inspect for a source-free static report.")
        with tempfile.TemporaryDirectory(prefix="kontra-library-inspector-", dir="/dev/shm") as temp:
            ram = Path(temp)
            for i, path in enumerate(args.paths):
                print(f"Inspecting {i+1}/{len(args.paths)}: {path}", flush=True)
                reports.extend(inspect_one(args.binary.resolve(), path,
                                           initialize=args.initialize, source=args.source,
                                           program=args.program, ram=ram, timeout=args.timeout,
                                           render=args.render, base=args.base, ui_only=args.ui_only, profile_init=args.profile_init, settle_ui=args.settle_ui))
            server, url = make_server(reports, args.port)
            print(f"{url}\nReports are held in memory. Ctrl+C closes the inspector.", flush=True)
            if not args.no_open:
                webbrowser.open(url)
            def terminate(_signum, _frame):
                raise KeyboardInterrupt
            signal.signal(signal.SIGTERM, terminate)
            try:
                server.serve_forever()
            except KeyboardInterrupt:
                pass
            finally:
                server.server_close()
    except (RuntimeError, ValueError, OSError, subprocess.TimeoutExpired) as error:
        print(f"library-inspector: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
