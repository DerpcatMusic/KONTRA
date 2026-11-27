#!/usr/bin/env python3
"""Automatic Bitwig Studio Kontakt VST3 -> KONTRA v3 VST3 project migrator.

Decodes Kontakt 6/7/8 VST3 state chunks (including encrypted commercial
libraries via installed .nicnt SNPID keys) using `kontra-inspect`, resolves
each loaded instrument to its local .nki path, preserves slot mix and KSP
persistent script state, and rewrites Bitwig .bwproject files cleanly.

Examples:
  # Dry-run across all Bitwig projects
  migrate_bitwig_auto.py ~/Bitwig --dry-run

  # Migrate one project into a safe copy beside the original
  migrate_bitwig_auto.py "~/Bitwig/MySong/MySong.bwproject"

  # Batch-migrate all projects with migratable Kontakt instances into [KONTRA] copies
  migrate_bitwig_auto.py ~/Bitwig --apply

  # In-place batch migration (requires Bitwig closed; saves .pre-kontra-migrate backups)
  migrate_bitwig_auto.py ~/Bitwig --apply --in-place
"""

from __future__ import annotations

import argparse
import copy
import hashlib
import io
import json
import os
import re
import shutil
import signal
import struct
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from inspect_bitwig import Reader, validate_structure, vst_component
from migrate_bitwig import RewriteReader, apply_edits

DEFAULT_KONTRA_CID = b"34ABFE55BD9BFB28B2B7BBFFE3C16B7F"
KONTAKT_DISPLAY_NAMES = {
    b"Kontakt",
    b"Kontakt 6",
    b"Kontakt 6 Portable",
    b"Kontakt 7",
    b"Kontakt 7 Portable",
    b"Kontakt 8",
    b"Kontakt 8 Portable",
    b"Kontakt Portable",
}
VST3_ROOT_CANDIDATES = {
    b"C:\\Program Files\\Common Files\\VST3",
    b"C:/Program Files/Common Files/VST3",
    b"/usr/lib/vst3",
    b"/usr/local/lib/vst3",
    b"/Library/Audio/Plug-Ins/VST3",
}


def resolve_kontra_cid(plugin: Path) -> bytes:
    """Reads KONTRA's VST3 Class ID from moduleinfo.json when available."""
    modinfo = plugin / "Contents" / "Resources" / "moduleinfo.json"
    if modinfo.is_file():
        try:
            data = json.loads(modinfo.read_text(encoding="utf-8"))
            for cls in data.get("Classes", []):
                cid = cls.get("CID", "")
                if isinstance(cid, str) and len(cid) == 32:
                    bytes.fromhex(cid)
                    return cid.upper().encode("ascii")
        except (OSError, ValueError, json.JSONDecodeError):
            pass
    return DEFAULT_KONTRA_CID


def find_inspector(explicit: Path | None) -> Path:
    if explicit is not None:
        path = explicit.expanduser().resolve()
        if not path.is_file() or not os.access(path, os.X_OK):
            raise ValueError(f"kontra-inspect binary is not executable: {path}")
        return path

    env_path = os.environ.get("KONTRA_INSPECT")
    if env_path:
        path = Path(env_path).expanduser().resolve()
        if path.is_file() and os.access(path, os.X_OK):
            return path

    kontra_root = Path(__file__).resolve().parents[1]
    candidates: list[Path] = []
    target_roots: list[Path] = [kontra_root / "target", kontra_root.parent.parent / "target"]
    env_target = os.environ.get("CARGO_TARGET_DIR")
    if env_target:
        target_roots.insert(0, Path(env_target).expanduser())

    which = shutil.which("kontra-inspect")
    if which:
        candidates.append(Path(which))

    def collect_from(root: Path) -> None:
        if not root.is_dir():
            return
        for profile in ("release", "debug"):
            direct = root / profile / "kontra-inspect"
            if direct.is_file() and os.access(direct, os.X_OK):
                candidates.append(direct)

    for root in target_roots:
        collect_from(root)

    if not candidates and (kontra_root / "Cargo.toml").is_file():
        try:
            meta = subprocess.run(
                [
                    "cargo",
                    "metadata",
                    "--no-deps",
                    "--format-version",
                    "1",
                    "--manifest-path",
                    str(kontra_root / "Cargo.toml"),
                ],
                capture_output=True,
                text=True,
                check=False,
            )
            if meta.returncode == 0:
                td = json.loads(meta.stdout).get("target_directory")
                if isinstance(td, str):
                    collect_from(Path(td))
        except (OSError, json.JSONDecodeError):
            pass

    if not candidates:
        raise ValueError(
            "could not locate kontra-inspect; build it with "
            "`cargo build --manifest-path plugins/kontra/Cargo.toml -p kontra-inspect --release` "
            "or pass --inspector PATH"
        )
    # Prefer the most recently modified executable.
    candidates.sort(key=lambda p: p.stat().st_mtime, reverse=True)
    return candidates[0]


def bitwig_is_running() -> bool:
    try:
        out = subprocess.run(
            ["pgrep", "-f", "BitwigStudio"],
            capture_output=True,
            text=True,
            check=False,
        )
        return out.returncode == 0
    except OSError:
        return False


def vst3_preset(state: bytes, cid: bytes) -> bytes:
    return (
        b"VST3\x01\x00\x00\x00"
        + cid
        + struct.pack("<Q", 48 + len(state))
        + state
        + b"List"
        + struct.pack("<I4sQQ", 1, b"Comp", 48, len(state))
    )


def is_kontakt_vstpreset(preset_bytes: bytes) -> tuple[bool, bytes | None]:
    if len(preset_bytes) < 48 or preset_bytes[:8] != b"VST3\x01\x00\x00\x00":
        return False, None
    cid_ascii = preset_bytes[8:40]
    try:
        plugin_id = bytes.fromhex(cid_ascii.decode("ascii"))
    except (ValueError, UnicodeError):
        return False, None
    return (b"kontakt" in plugin_id.lower()), cid_ascii


def collect_projects(paths: list[Path]) -> list[Path]:
    found: list[Path] = []
    seen: set[Path] = set()
    for raw in paths:
        p = raw.expanduser().resolve()
        if p.is_file():
            if p.suffix.casefold() != ".bwproject" and ".bwproject." not in p.name.casefold():
                raise ValueError(f"not a .bwproject file: {p}")
            if p not in seen:
                seen.add(p)
                found.append(p)
        elif p.is_dir():
            for proj in sorted(p.rglob("*.bwproject")):
                if "auto-backups" in proj.parts:
                    continue
                if proj.stem.endswith(" [KONTRA]") or "[KONTRA" in proj.stem:
                    continue
                resolved = proj.resolve()
                if resolved not in seen:
                    seen.add(resolved)
                    found.append(resolved)
        else:
            raise ValueError(f"path does not exist: {p}")
    return found


def scan_project_kontakt_entries(project: Path) -> list[tuple[str, bytes, bytes]]:
    """Returns `[(zip_entry, old_cid_ascii, comp_bytes), ...]` for Kontakt VST3 states."""
    entries: list[tuple[str, bytes, bytes]] = []
    with zipfile.ZipFile(project) as archive:
        for info in archive.infolist():
            if not info.filename.startswith("plugin-states/") or not info.filename.endswith(".vstpreset"):
                continue
            if info.file_size > 128 * 1024 * 1024:
                continue
            preset = archive.read(info)
            is_k, old_cid = is_kontakt_vstpreset(preset)
            if not is_k or old_cid is None:
                continue
            comp = vst_component(preset)
            entries.append((info.filename, old_cid, comp))
    return entries


def build_device_replacements(node: bytes, old_id: bytes, plugin: Path, kontra_cid: bytes) -> dict[bytes, bytes]:
    plugin_str = str(plugin.resolve()).encode("utf-8")
    plugin_parent = str(plugin.resolve().parent).encode("utf-8")
    plugin_name = plugin.resolve().name.encode("utf-8")
    replacements: dict[bytes, bytes] = {old_id: kontra_cid}

    for match in re.finditer(rb"\x08([\x00-\x7f][\x00-\xff]{3})", node):
        size = int.from_bytes(match[1], "big")
        value = node[match.end() : match.end() + size]
        if len(value) != size or size > 4096:
            continue
        low = value.lower()
        if b"kontakt" in low and value.endswith(b".vst3\n" + old_id):
            replacements[value] = plugin_str + b"\n" + kontra_cid
        elif b"kontakt" in low and value.endswith(b".vst3"):
            if value.startswith((b"/", b"C:\\", b"C:/", b"D:\\", b"D:/")) or b":\\" in value:
                replacements[value] = plugin_str
            else:
                replacements[value] = plugin_name
        elif value in KONTAKT_DISPLAY_NAMES:
            replacements[value] = b"KONTRA"
        elif value == b"Native Instruments":
            replacements[value] = b"Matari Audio"
        elif value in VST3_ROOT_CANDIDATES:
            replacements[value] = plugin_parent
    return replacements


def rewrite_project(
    source: Path,
    destination: Path,
    converted: dict[str, tuple[bytes, bytes]],
    plugin: Path,
    kontra_cid: bytes,
    in_place: bool = False,
) -> dict:
    """Rewrites `converted` (`{zip_entry: (old_cid_ascii, oast_bytes)}`) in `source`."""
    source = source.resolve()
    destination = destination.resolve()
    if not in_place:
        if destination == source:
            raise ValueError("output copy must differ from source unless --in-place is set")
        if destination.parent != source.parent or destination.suffix.casefold() != ".bwproject":
            raise ValueError("output must be a .bwproject beside the source")
        if destination.exists():
            raise ValueError(f"output already exists: {destination}")

    data = source.read_bytes()
    validate_structure(data)
    boundary = int(data[24:40], 16)
    reader = RewriteReader(data, int(data[16:24], 16), boundary)
    reader.obj()
    if reader.pos != boundary:
        raise ValueError("expected exactly one Bitwig project object")

    edits: list[tuple[int, int, bytes]] = []
    new_states: dict[str, bytes] = {}
    skipped_shared: list[str] = []

    for entry, (old_cid, oast_bytes) in converted.items():
        preset_name = Path(entry).name
        needle = b"\x08" + struct.pack(">I", len(preset_name.encode())) + preset_name.encode()
        candidates = [(a, b) for a, b in reader.plugins if needle in data[a:b]]
        if not candidates:
            skipped_shared.append(f"{entry}: no VST3 device references state")
            continue
        # Verify every referencing device is this Kontakt VST3 plugin and non-overlapping.
        valid_candidates = True
        local_edits: list[tuple[int, int, bytes]] = []
        for start, end in candidates:
            if any(not (end <= a or start >= b) for a, b, _ in edits):
                valid_candidates = False
                break
            node = data[start:end]
            if old_cid not in node:
                valid_candidates = False
                break
            replacements = build_device_replacements(node, old_cid, plugin, kontra_cid)
            rewritten = RewriteReader(node, 0, len(node), replacements)
            rewritten.obj()
            if not rewritten.edits:
                valid_candidates = False
                break
            local_edits.append((start, end, apply_edits(node, rewritten.edits)))
        if not valid_candidates:
            skipped_shared.append(f"{entry}: could not safely rewrite all referencing devices")
            continue
        edits.extend(local_edits)
        new_states[entry] = vst3_preset(oast_bytes, kontra_cid)

    if not new_states:
        return {"rewritten_entries": 0, "skipped_rewrite": skipped_shared}

    main = apply_edits(data[:boundary], edits)
    main = main[:24] + f"{len(main):016x}".encode() + main[40:]
    validate_structure(main)

    with zipfile.ZipFile(source) as archive:
        buf = io.BytesIO()
        with zipfile.ZipFile(buf, "w", compression=zipfile.ZIP_DEFLATED) as out_zip:
            for info in archive.infolist():
                out_zip.writestr(copy.copy(info), new_states.get(info.filename, archive.read(info)))
        result_bytes = main + buf.getvalue()
        with zipfile.ZipFile(io.BytesIO(result_bytes)) as check_zip:
            if check_zip.namelist() != archive.namelist() or check_zip.testzip() is not None:
                raise ValueError("output archive validation failed")
            for info in archive.infolist():
                expected = new_states.get(info.filename, archive.read(info))
                if check_zip.read(info.filename) != expected:
                    raise ValueError(f"output entry {info.filename} differs from expected bytes")

    if hashlib.sha256(source.read_bytes()).digest() != hashlib.sha256(data).digest():
        raise ValueError("source project changed during migration; aborting write")

    if in_place:
        backup = source.with_name(source.name + ".pre-kontra-migrate")
        if not backup.exists():
            shutil.copy2(source, backup)
        tmp = source.with_name(f".{source.name}.kontra-tmp")
        tmp.write_bytes(result_bytes)
        os.replace(tmp, source)
    else:
        tmp = destination.with_name(f".{destination.name}.kontra-tmp")
        tmp.write_bytes(result_bytes)
        os.replace(tmp, destination)

    return {
        "rewritten_entries": len(new_states),
        "rewritten_devices": len(edits),
        "skipped_rewrite": skipped_shared,
        "output": str(destination),
    }


def run_migration(
    projects: list[Path],
    inspector: Path,
    plugin: Path,
    apply_changes: bool,
    in_place: bool,
    output_copy: Path | None,
    include_empty: bool,
    strict: bool,
    roots: list[Path],
    index_path: Path | None,
    write_report: bool,
) -> dict:
    if output_copy is not None and len(projects) != 1:
        raise ValueError("--output-copy can only be used with a single project")
    if apply_changes and not plugin.is_dir():
        raise ValueError(f"KONTRA VST3 bundle not found: {plugin}")
    kontra_cid = resolve_kontra_cid(plugin)

    with tempfile.TemporaryDirectory(prefix="kontra-bw-migrate-") as work_str:
        work = Path(work_str)
        manifest: list[dict] = []
        # Map project -> list of (job_id, zip_entry, old_cid_ascii, state_path)
        project_jobs: dict[Path, list[tuple[str, str, bytes, Path]]] = {}
        project_errors: dict[Path, str] = {}

        job_counter = 0
        for proj in projects:
            try:
                entries = scan_project_kontakt_entries(proj)
            except (OSError, ValueError, zipfile.BadZipFile) as err:
                project_errors[proj] = str(err)
                continue
            if not entries:
                continue
            jobs_for_proj = []
            for zip_entry, old_cid, comp in entries:
                job_id = f"j{job_counter}"
                job_counter += 1
                chunk_file = work / f"{job_id}.nkm"
                state_file = work / f"{job_id}.state"
                chunk_file.write_bytes(comp)
                manifest.append({
                    "id": job_id,
                    "chunk": str(chunk_file),
                    "out_state": str(state_file),
                })
                jobs_for_proj.append((job_id, zip_entry, old_cid, state_file))
            project_jobs[proj] = jobs_for_proj

        reports_by_id: dict[str, dict] = {}
        if manifest:
            manifest_path = work / "manifest.json"
            manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
            cmd = [str(inspector), "migrate-batch", str(manifest_path)]
            for r in roots:
                cmd.extend(["--root", str(r.expanduser().resolve())])
            if index_path is not None:
                cmd.extend(["--index", str(index_path.expanduser().resolve())])
            proc = subprocess.run(cmd, capture_output=True, text=True, check=False)
            if proc.returncode != 0:
                raise ValueError(
                    f"kontra-inspect migrate-batch failed (rc={proc.returncode}): "
                    f"{proc.stderr.strip() or proc.stdout.strip()}"
                )
            for item in json.loads(proc.stdout):
                reports_by_id[item["id"]] = item

        summary_projects = []
        totals = {
            "projects_scanned": len(projects),
            "projects_with_kontakt": len(project_jobs),
            "projects_migrated": 0,
            "instances_total": 0,
            "instances_ok": 0,
            "instances_empty": 0,
            "instances_partial": 0,
            "instances_missing_key": 0,
            "instances_unresolved": 0,
            "instances_error": 0,
            "instances_rewritten": 0,
        }

        for proj, jobs_for_proj in project_jobs.items():
            converted: dict[str, tuple[bytes, bytes]] = {}
            inst_rows = []
            all_clean = True
            for job_id, zip_entry, old_cid, state_file in jobs_for_proj:
                rep = reports_by_id[job_id]
                st = rep["status"]
                totals["instances_total"] += 1
                if st == "ok":
                    totals["instances_ok"] += 1
                elif st == "empty":
                    totals["instances_empty"] += 1
                elif st == "partial":
                    totals["instances_partial"] += 1
                    all_clean = False
                elif st == "missing_library_key":
                    totals["instances_missing_key"] += 1
                    all_clean = False
                elif st == "unresolved_instrument":
                    totals["instances_unresolved"] += 1
                    all_clean = False
                else:
                    totals["instances_error"] += 1
                    all_clean = False

                will_migrate = (st == "ok") or (st == "empty" and include_empty)
                if will_migrate and state_file.is_file():
                    converted[zip_entry] = (old_cid, state_file.read_bytes())

                inst_rows.append({
                    "preset": zip_entry,
                    "status": st,
                    "will_migrate": will_migrate,
                    "encrypted": rep.get("encrypted", False),
                    "snpids": rep.get("snpids", []),
                    "matched_library": rep.get("matched_library"),
                    "slots": [
                        {
                            "slot": s["slot"],
                            "name": s["name"],
                            "matched_path": s["matched_path"],
                            "match_reason": s["match_reason"],
                            "channel": s["channel"],
                            "output": s["output"],
                            "script_slots": s["script_slots"],
                            "persistent_vars": s["persistent_vars"],
                        }
                        for s in rep.get("slots", [])
                    ],
                    "error": rep.get("error"),
                })

            proj_row: dict = {
                "project": str(proj),
                "kontakt_instances": len(jobs_for_proj),
                "migratable_instances": len(converted),
                "all_clean": all_clean,
                "status": "dry_run",
                "instances": inst_rows,
            }

            should_write = apply_changes and bool(converted) and (not strict or all_clean)
            if should_write:
                dest = (
                    proj
                    if in_place
                    else (
                        output_copy.expanduser().resolve()
                        if output_copy is not None
                        else proj.with_name(proj.stem + " [KONTRA].bwproject")
                    )
                )
                try:
                    rw = rewrite_project(proj, dest, converted, plugin, kontra_cid, in_place=in_place)
                    proj_row["status"] = "migrated" if rw["rewritten_entries"] else "unchanged"
                    proj_row.update(rw)
                    if rw["rewritten_entries"]:
                        totals["projects_migrated"] += 1
                        totals["instances_rewritten"] += rw["rewritten_entries"]
                        if write_report:
                            rep_path = dest.with_suffix(".kontra-migration.json")
                            rep_path.write_text(json.dumps(proj_row, indent=2) + "\n", encoding="utf-8")
                            proj_row["report_path"] = str(rep_path)
                except (OSError, ValueError, zipfile.BadZipFile) as err:
                    proj_row["status"] = "write_error"
                    proj_row["error"] = str(err)
            elif apply_changes:
                proj_row["status"] = "skipped_strict" if (strict and not all_clean) else "nothing_to_migrate"

            summary_projects.append(proj_row)

        return {
            "kontra_vst3": str(plugin.resolve()),
            "kontra_cid": kontra_cid.decode("ascii"),
            "mode": ("apply_in_place" if in_place else "apply_copy") if apply_changes else "dry_run",
            "totals": totals,
            "project_errors": {str(k): v for k, v in project_errors.items()},
            "projects": summary_projects,
        }


def print_summary(result: dict) -> None:
    t = result["totals"]
    print(
        f"Mode: {result['mode']} | KONTRA CID: {result['kontra_cid']}\n"
        f"Projects scanned: {t['projects_scanned']} | with Kontakt: {t['projects_with_kontakt']} | "
        f"migrated: {t['projects_migrated']}\n"
        f"Instances: {t['instances_total']} total -> "
        f"{t['instances_ok']} resolved, {t['instances_empty']} empty, "
        f"{t['instances_partial']} partial, {t['instances_missing_key']} missing-key, "
        f"{t['instances_unresolved']} unresolved, {t['instances_error']} error | "
        f"rewritten: {t['instances_rewritten']}"
    )
    for proj in result["projects"]:
        rel = Path(proj["project"]).name
        print(
            f"\n  [{proj['status']}] {rel} "
            f"({proj['migratable_instances']}/{proj['kontakt_instances']} migratable)"
        )
        if proj.get("output"):
            print(f"    -> {proj['output']}")
        if proj.get("error"):
            print(f"    ERROR: {proj['error']}")
        for inst in proj["instances"]:
            flag = "MIGRATE" if inst["will_migrate"] else "KEEP   "
            slots_desc = ", ".join(
                f"{s['name']} -> {Path(s['matched_path']).name if s['matched_path'] else 'UNRESOLVED'} "
                f"(ch={s['channel']}, vars={s['persistent_vars']})"
                for s in inst["slots"]
            )
            if not slots_desc:
                if inst["status"] == "empty":
                    slots_desc = "(empty Kontakt multi)"
                elif inst["status"] == "missing_library_key":
                    slots_desc = f"(missing .nicnt key for SNPID {inst['snpids']})"
                elif inst.get("error"):
                    slots_desc = f"(error: {inst['error']})"
            print(f"    {flag} [{inst['status']:19s}] {Path(inst['preset']).name}: {slots_desc}")


def main(argv: list[str] | None = None) -> int:
    if hasattr(signal, "SIGPIPE"):
        signal.signal(signal.SIGPIPE, signal.SIG_DFL)
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("paths", nargs="+", type=Path, help=".bwproject file(s) or directory(ies) to scan")
    parser.add_argument("--apply", action="store_true", help="write migrated project files (default for a single .bwproject unless --dry-run)")
    parser.add_argument("--dry-run", action="store_true", help="inspect and report only; write nothing")
    parser.add_argument("--in-place", action="store_true", help="replace projects in-place after creating .pre-kontra-migrate backups")
    parser.add_argument("--force", action="store_true", help="allow --in-place even when Bitwig Studio is running")
    parser.add_argument("--output-copy", type=Path, help="explicit destination path (single project only)")
    parser.add_argument("--skip-empty", action="store_true", help="leave empty Kontakt instances untouched")
    parser.add_argument("--strict", action="store_true", help="only migrate a project if 100%% of its Kontakt instances resolve")
    parser.add_argument("--root", action="append", default=[], type=Path, help="extra Kontakt library root directory (repeatable)")
    parser.add_argument("--index", type=Path, help="override KONTRA library-index.json path")
    parser.add_argument("--inspector", type=Path, help="explicit path to kontra-inspect binary")
    parser.add_argument("--plugin", type=Path, default=Path.home() / ".vst3/KONTRA.vst3", help="path to KONTRA.vst3 bundle")
    parser.add_argument("--json", type=Path, help="write full JSON report to FILE")
    parser.add_argument("--no-per-project-report", action="store_true", help="do not write .kontra-migration.json beside outputs")
    args = parser.parse_args(argv)

    try:
        projects = collect_projects(args.paths)
        if not projects:
            raise ValueError("no .bwproject files found")
        single_file_mode = len(args.paths) == 1 and args.paths[0].expanduser().is_file()
        apply_changes = (args.apply or single_file_mode) and not args.dry_run
        if apply_changes and args.in_place and bitwig_is_running() and not args.force:
            raise ValueError(
                "Bitwig Studio is currently running; close Bitwig before --in-place migration "
                "(or omit --in-place to write safe [KONTRA] copies, or pass --force)"
            )
        inspector = find_inspector(args.inspector)
        result = run_migration(
            projects=projects,
            inspector=inspector,
            plugin=args.plugin,
            apply_changes=apply_changes,
            in_place=args.in_place,
            output_copy=args.output_copy,
            include_empty=not args.skip_empty,
            strict=args.strict,
            roots=args.root,
            index_path=args.index,
            write_report=not args.no_per_project_report,
        )
    except (OSError, ValueError, zipfile.BadZipFile) as err:
        print(f"migrate_bitwig_auto: {err}", file=sys.stderr)
        return 2

    if args.json:
        args.json.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print_summary(result)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
