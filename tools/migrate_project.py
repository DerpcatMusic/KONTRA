#!/usr/bin/env python3
"""Read-only inventory of Kontakt instances in a REAPER .RPP project."""

from __future__ import annotations

import argparse
import base64
import json
import os
import re
import struct
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Iterable


OPEN = re.compile(r"<([A-Za-z0-9_]+)(?:\s|$)")
PLUGIN_BLOCKS = {"VST", "VST3", "AU", "CLAP"}
FX_CHAINS = {"FXCHAIN", "FXCHAIN_REC"}
FX_CONTAINERS = {"CONTAINER", "CONTAINERFX"}
AUTOMATION_BLOCKS = {"PARMENV", "PROGRAMENV"}
STATE_BLOCKS = {"CHUNK", "VSTCHUNK", "VST3CHUNK", "AUCHUNK", "CLAPCHUNK"}
KONTAKT = re.compile(r"\bkontakt\b", re.IGNORECASE)
INSTRUMENT_FILE = re.compile(r"\.(?:nki|nkm)$", re.IGNORECASE)


def quoted_values(line: str) -> list[str]:
    values = []
    i = 0
    while i < len(line):
        if line[i] != '"':
            i += 1
            continue
        i += 1
        value = []
        while i < len(line):
            if line[i] == "\\" and i + 1 < len(line) and line[i + 1] == '"':
                value.append('"')
                i += 2
            elif line[i] == '"':
                values.append("".join(value))
                i += 1
                break
            else:
                value.append(line[i])
                i += 1
        else:
            raise ValueError("unterminated quoted field")
    return values


def _first_quoted(line: str) -> str | None:
    values = quoted_values(line)
    return values[0] if values else None


def _active(stack: list[dict], key: str) -> dict | None:
    return next((node[key] for node in reversed(stack) if node.get(key) is not None), None)


def _visible_refs(line: str, line_number: int) -> list[dict]:
    return [
        {"path": value, "line": line_number, "confirmed_active_preset": False}
        for value in quoted_values(line)
        if INSTRUMENT_FILE.search(value)
    ]


def inspect_rpp(lines: Iterable[str], source: str) -> dict:
    stack: list[dict] = []
    tracks = []
    root_seen = False
    root_closed = False

    for line_number, raw_line in enumerate(lines, 1):
        line = raw_line.rstrip("\r\n")
        stripped = line.strip()
        if not stripped:
            continue
        if root_closed:
            raise ValueError(f"unexpected content after project close at line {line_number}")

        opening = OPEN.match(stripped)
        if opening:
            tag = opening.group(1).upper()
            if not root_seen:
                if tag != "REAPER_PROJECT":
                    raise ValueError("file does not start with a REAPER_PROJECT block")
                root_seen = True
            for node in stack:
                if node.get("plugin") is not None:
                    node["plugin"]["serialized_block_bytes"] += len(raw_line.encode("utf-8"))

            track = _active(stack, "track")
            chain = _active(stack, "chain")
            plugin = None
            if tag == "TRACK":
                track = {
                    "track_index": len(tracks),
                    "track_name": f"Track {len(tracks) + 1}",
                    "kontakt_instances": [],
                    "automation_blocks": [],
                }
                tracks.append(track)
            elif tag in FX_CHAINS and track is not None:
                chain = {"tag": tag, "plugin_count": 0}
            elif tag in PLUGIN_BLOCKS:
                descriptor = _first_quoted(stripped) or stripped
                if track is not None and KONTAKT.search(descriptor):
                    fx_index = chain["plugin_count"] if chain is not None else None
                    plugin = {
                        "track_index": track["track_index"],
                        "track_name": track["track_name"],
                        "fx_chain": chain["tag"] if chain is not None else None,
                        "fx_index": fx_index,
                        "inside_fx_container": any(node["tag"] in FX_CONTAINERS for node in stack),
                        "plugin_type": tag,
                        "descriptor": descriptor,
                        "line": line_number,
                        "visible_instrument_refs": [],
                        "automation_markers_inside_plugin_block": [],
                        "state_chunk_markers": [],
                        "serialized_block_bytes": len(raw_line.encode("utf-8")),
                    }
                    track["kontakt_instances"].append(plugin)
                if chain is not None:
                    chain["plugin_count"] += 1

            if tag in AUTOMATION_BLOCKS and track is not None:
                track["automation_blocks"].append({"kind": tag, "line": line_number})
                active_plugin = _active(stack, "plugin")
                if active_plugin is not None:
                    active_plugin["automation_markers_inside_plugin_block"].append(
                        {"kind": tag, "line": line_number}
                    )
            if tag in STATE_BLOCKS:
                active_plugin = _active(stack, "plugin")
                if active_plugin is not None:
                    active_plugin["state_chunk_markers"].append(tag)

            stack.append({"tag": tag, "track": track, "chain": chain, "plugin": plugin})
            continue

        if stripped == ">":
            if not stack:
                raise ValueError(f"unexpected block close at line {line_number}")
            for node in stack:
                if node.get("plugin") is not None:
                    node["plugin"]["serialized_block_bytes"] += len(raw_line.encode("utf-8"))
            stack.pop()
            if not stack:
                root_closed = True
            continue

        if stripped.startswith("<"):
            raise ValueError(f"malformed block opener at line {line_number}")

        for node in stack:
            if node.get("plugin") is not None:
                node["plugin"]["serialized_block_bytes"] += len(raw_line.encode("utf-8"))

        track = _active(stack, "track")
        if track is not None and stack[-1]["tag"] == "TRACK" and stripped.startswith("NAME "):
            name = _first_quoted(stripped)
            if name is not None:
                track["track_name"] = name
        active_plugin = _active(stack, "plugin")
        if active_plugin is not None:
            active_plugin["visible_instrument_refs"].extend(_visible_refs(stripped, line_number))

    if not root_seen:
        raise ValueError("file does not contain a REAPER_PROJECT block")
    if stack:
        raise ValueError(f"unclosed {stack[-1]['tag']} block")

    instances = []
    for track in tracks:
        for instance in track["kontakt_instances"]:
            instance["track_name"] = track["track_name"]
            instances.append(instance)
    for instance in instances:
        instance["opaque_state"] = {
            "status": "not_decoded",
            "plugin_block_bytes": instance.pop("serialized_block_bytes"),
            "chunk_markers": instance.pop("state_chunk_markers"),
            "note": "Kontakt state is preserved as host text but is not decoded or translated.",
        }
        instance["automation_risk"] = {
            "track_markers": next(
                track["automation_blocks"]
                for track in tracks
                if track["track_index"] == instance["track_index"]
            ),
            "inside_plugin_block_markers": instance.pop("automation_markers_inside_plugin_block"),
            "mapping": "unverified",
        }

    warnings = []
    if instances:
        warnings.extend(
            [
                "Visible .nki/.nkm strings are references only; verify the active instrument in Kontakt.",
                "Kontakt processor state, script persistence, and internal output routing remain opaque.",
                "Track or plugin automation is reported but no Kontakt-to-KONTRA parameter mapping is applied.",
                "No project or plugin identifiers were changed during inventory.",
            ]
        )

    return {
        "format": "REAPER .RPP text",
        "source": source,
        "read_only": True,
        "conversion_status": "inventory_only",
        "track_count": len(tracks),
        "kontakt_instance_count": len(instances),
        "kontakt_instances": instances,
        "warnings": warnings,
    }


def inventory(path: Path) -> dict:
    if path.suffix.casefold() != ".rpp":
        raise ValueError("first supported input format is REAPER .RPP")
    with path.open("r", encoding="utf-8-sig", newline="") as project:
        return inspect_rpp(project, str(path))


def parse_mapping(value: str) -> tuple[int, int, Path]:
    location, separator, mapping = value.partition("=")
    if not separator or not mapping:
        raise ValueError("mapping must be TRACK:FX=MULTI.kontra-multi")
    track, separator, fx = location.partition(":")
    if not separator or not track.isdecimal() or not fx.isdecimal():
        raise ValueError("mapping location must be zero-based TRACK:FX")
    path = Path(mapping).expanduser()
    if path.suffix.casefold() != ".kontra-multi":
        raise ValueError("mapping file must have the .kontra-multi extension")
    if not path.is_file():
        raise ValueError(f"SavedMulti mapping does not exist: {path}")
    return int(track), int(fx), path.resolve()


def mapped_instances(report: dict, mappings: list[str]) -> tuple[list[dict], list[dict]]:
    plans = []
    skipped = []
    seen = set()
    for spec in mappings:
        track_index, fx_index, multi = parse_mapping(spec)
        key = (track_index, fx_index)
        if key in seen:
            raise ValueError(f"duplicate mapping for track {track_index}, FX {fx_index}")
        seen.add(key)
        instance = next(
            (item for item in report["kontakt_instances"]
             if (item["track_index"], item["fx_index"]) == key),
            None,
        )
        if instance is None:
            raise ValueError(f"track {track_index}, FX {fx_index} is not a detected Kontakt instance")
        reason = None
        if instance["fx_chain"] != "FXCHAIN":
            reason = "only the normal track FXCHAIN is supported"
        elif instance["inside_fx_container"]:
            reason = "nested FX containers are ambiguous"
        elif instance["automation_risk"]["track_markers"] or instance["automation_risk"]["inside_plugin_block_markers"]:
            reason = "track or plugin automation is present and unmapped"
        entry = {
            "track_index": track_index,
            "fx_index": fx_index,
            "track_name": instance["track_name"],
            "descriptor": instance["descriptor"],
            "multi": str(multi),
        }
        (skipped if reason else plans).append({**entry, **({"reason": reason} if reason else {})})
    return plans, skipped


def lua_quote(value: str) -> str:
    escaped = []
    for char in value:
        code = ord(char)
        if char == "\\":
            escaped.append("\\\\")
        elif char == '"':
            escaped.append('\\"')
        elif char == "\n":
            escaped.append("\\n")
        elif char == "\r":
            escaped.append("\\r")
        elif code < 32 or code == 127:
            escaped.append(f"\\{code:03d}")
        else:
            escaped.append(char)
    return '"' + "".join(escaped) + '"'


def export_state(exporter: Path, multi: Path, output: Path) -> tuple[bytes, dict]:
    result = subprocess.run(
        [str(exporter), "export-multi-state", str(multi), str(output)],
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode:
        raise ValueError(f"state export failed for {multi}: {result.stderr.strip() or result.stdout.strip()}")
    try:
        metadata = json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise ValueError(f"state exporter returned invalid JSON for {multi}: {error}") from error
    blob = output.read_bytes()
    if len(blob) != metadata.get("state_bytes") or not metadata.get("envelope_round_trip_verified"):
        raise ValueError(f"state exporter verification failed for {multi}")
    return blob, metadata


def reaper_script(source: Path, output: Path, plans: list[dict], prepared_mappings: list[dict]) -> str:
    temporary = output.with_name(f".{output.name}.migration-tmp.rpp")
    report_path = output.with_suffix(output.suffix + ".migration.json")
    rows = []
    report = {
        "status": "experimental_copy_saved",
        "source": str(source),
        "output": str(output),
        "reaper_reopen_verified": True,
        "state_install_readback_verified": True,
        "state_install_readback_format": "exact framed OAST plus the observed 8-byte REAPER VST3 readback trailer",
        "temporary_reopen_state_chunk_present": True,
        "post_reopen_state": None,
        "migrated": [
            {"track_index": plan["track_index"], "fx_index": plan["fx_index"], "track_name": plan["track_name"]}
            for plan in plans
        ],
        "prepared_mappings": prepared_mappings,
        "limitations": [
            "Kontakt plug-in values and opaque plugin state were not translated; the explicit SavedMulti supplied KONTRA parts.",
            "SavedMulti script persistence is kept as supplied; opaque Kontakt state and unmapped script controls cannot be recovered.",
            "SavedMulti has no rack-level output bus settings; KONTRA rack defaults are used.",
            "REAPER track routing and compatible FX pin mappings stay with the copied project; Kontakt-specific routing outside the mapping is not recovered.",
            "Audio decode and Kontakt sonic parity remain unverified.",
        ],
    }
    for plan in plans:
        rows.append(
            "  { track = %d, fx = %d, name = %s, state = %s, expected = %s },"
            % (
                plan["track_index"],
                plan["fx_index"],
                lua_quote(plan["track_name"]),
                lua_quote(plan["state_base64"]),
                lua_quote(plan["expected_readback_base64"]),
            )
        )
    lines = [
        "-- Generated by tools/migrate_project.py. Experimental REAPER VST3 state migration.",
        "-- Replaces only mapped, preflighted Kontakt instances in an output copy.",
        f"local SOURCE = {lua_quote(str(source))}",
        f"local OUTPUT = {lua_quote(str(output))}",
        f"local TEMP = {lua_quote(str(temporary))}",
        f"local REPORT = {lua_quote(str(report_path))}",
        f"local REPORT_CONTENT = {lua_quote(json.dumps(report, ensure_ascii=False))}",
        "local plans = {",
        *rows,
        "}",
        "local function exists(path)",
        '  local file = io.open(path, "rb")',
        "  if file then file:close(); return true end",
        "  return false",
        "end",
        "local function same_path(a, b)",
        '  return string.gsub(a, "\\\\", "/") == string.gsub(b, "\\\\", "/")',
        "end",
        "local function project_path()",
        '  local _, path = reaper.EnumProjects(-1, "")',
        "  return path",
        "end",
        "local function open_project(path)",
        '  reaper.Main_openProject("noprompt:" .. path)',
        "end",
        "local function kontakt(track, fx)",
        '  local ok, name = reaper.TrackFX_GetFXName(track, fx, "")',
        '  return ok and string.find(string.lower(name), "kontakt", 1, true) ~= nil, name',
        "end",
        "local function abort(message)",
        '  reaper.ShowConsoleMsg("KONTRA migration stopped: " .. message .. "\\n")',
        "  error(message)",
        "end",
        'if not same_path(project_path(), SOURCE) then abort("active project does not match the inventoried source") end',
        'if exists(OUTPUT) or exists(TEMP) or exists(REPORT) then abort("output, temporary copy, or migration report already exists; choose fresh paths") end',
        "local proj = reaper.EnumProjects(-1, \"\")",
        "for _, plan in ipairs(plans) do",
        "  local track = reaper.GetTrack(proj, plan.track)",
        '  if not track then abort("mapped track no longer exists: " .. plan.track) end',
        '  local ok, name = kontakt(track, plan.fx)',
        '  if not ok then abort("mapped FX is no longer Kontakt: " .. tostring(name)) end',
        '  if reaper.CountTrackEnvelopes(track) > 0 then abort("track automation is present on track " .. plan.track) end',
        "end",
        "reaper.Main_SaveProjectEx(proj, TEMP, 0)",
        'if not exists(TEMP) then abort("REAPER did not create the temporary project copy") end',
        "open_project(TEMP)",
        'if not same_path(project_path(), TEMP) then abort("REAPER did not open the temporary copy") end',
        "proj = reaper.EnumProjects(-1, \"\")",
        "table.sort(plans, function(a, b) if a.track == b.track then return a.fx > b.fx end return a.track > b.track end)",
        "for _, plan in ipairs(plans) do",
        "  local track = reaper.GetTrack(proj, plan.track)",
        '  local ok, old_name = kontakt(track, plan.fx)',
        '  if not ok then abort("Kontakt FX changed before replacement: " .. tostring(old_name)) end',
        "  local enabled = reaper.TrackFX_GetEnabled(track, plan.fx)",
        "  local old_type, old_inputs, old_outputs = reaper.TrackFX_GetIOSize(track, plan.fx)",
        '  if old_type < 0 then abort("REAPER cannot read Kontakt pin layout on track " .. plan.track) end',
        '  local new_fx = reaper.TrackFX_AddByName(track, "VST3: KONTRA", false, -1000 - plan.fx)',
        '  if new_fx < 0 then abort("REAPER could not insert KONTRA on track " .. plan.track) end',
        '  local _, new_name = reaper.TrackFX_GetFXName(track, new_fx, "")',
        '  if not string.find(string.upper(new_name), "KONTRA", 1, true) then abort("inserted FX is not KONTRA: " .. tostring(new_name)) end',
        "  local new_type, new_inputs, new_outputs = reaper.TrackFX_GetIOSize(track, new_fx)",
        '  if new_type < 0 then abort("REAPER cannot read KONTRA pin layout on track " .. plan.track) end',
        "  for _, direction in ipairs({0, 1}) do",
        "    local old_count = direction == 0 and old_inputs or old_outputs",
        "    local new_count = direction == 0 and new_inputs or new_outputs",
        "    for pin = 0, old_count - 1 do",
        "      local low, high = reaper.TrackFX_GetPinMappings(track, plan.fx + 1, direction, pin)",
        '      if pin < new_count then',
        '        if not reaper.TrackFX_SetPinMappings(track, new_fx, direction, pin, low, high) then abort("REAPER rejected pin mapping on track " .. plan.track) end',
        '      elseif low ~= 0 or high ~= 0 then abort("KONTRA has no matching pin " .. pin .. " for active Kontakt routing on track " .. plan.track) end',
        "    end",
        "  end",
        '  if not reaper.TrackFX_SetNamedConfigParm(track, new_fx, "vst_chunk", plan.state) then abort("REAPER rejected KONTRA state on track " .. plan.track) end',
        '  local state_ok, state_back = reaper.TrackFX_GetNamedConfigParm(track, new_fx, "vst_chunk")',
        '  if not state_ok or state_back ~= plan.expected then abort("REAPER KONTRA state readback differed from the verified VST3 chunk on track " .. plan.track) end',
        '  reaper.TrackFX_SetEnabled(track, new_fx, enabled)',
        '  if reaper.TrackFX_GetEnabled(track, new_fx) ~= enabled then abort("REAPER could not preserve FX enabled state on track " .. plan.track) end',
        '  if not reaper.TrackFX_Delete(track, plan.fx + 1) then abort("REAPER could not remove Kontakt on track " .. plan.track) end',
        "end",
        "local settle_at = reaper.time_precise() + 2.0",
        "local function finish_migration()",
        "  if reaper.time_precise() < settle_at then reaper.defer(finish_migration); return end",
        "  for _, plan in ipairs(plans) do",
        "    local track = reaper.GetTrack(proj, plan.track)",
        '    local state_ok, state_back = reaper.TrackFX_GetNamedConfigParm(track, plan.fx, "vst_chunk")',
        '    if not state_ok or state_back ~= plan.expected then abort("KONTRA state changed after the worker became ready on track " .. plan.track) end',
        "  end",
        "  reaper.MarkProjectDirty(proj)",
        "  reaper.Main_SaveProjectEx(proj, TEMP, 0)",
        "  open_project(TEMP)",
        '  if not same_path(project_path(), TEMP) then abort("REAPER could not reopen the converted temporary project") end',
        "  proj = reaper.EnumProjects(-1, \"\")",
        "  for _, plan in ipairs(plans) do",
        "    local track = reaper.GetTrack(proj, plan.track)",
        '    local ok, name = reaper.TrackFX_GetFXName(track, plan.fx, "")',
        '    if not ok or not string.find(string.upper(name), "KONTRA", 1, true) then abort("KONTRA FX did not survive project reload") end',
        '    local state_ok, state_back = reaper.TrackFX_GetNamedConfigParm(track, plan.fx, "vst_chunk")',
        '    if not state_ok or state_back ~= plan.expected then abort("converted project KONTRA state differs from the verified REAPER VST3 chunk") end',
        "  end",
        '  if not os.rename(TEMP, OUTPUT) then abort("could not publish the converted project copy") end',
        "  open_project(OUTPUT)",
        '  if not same_path(project_path(), OUTPUT) then abort("REAPER could not open the final copy") end',
        '  proj = reaper.EnumProjects(-1, "")',
        "  local final_state_checks = {}",
        "  for _, plan in ipairs(plans) do",
        "    local track = reaper.GetTrack(proj, plan.track)",
        '    local ok, name = reaper.TrackFX_GetFXName(track, plan.fx, "")',
        '    if not ok or not string.find(string.upper(name), "KONTRA", 1, true) then abort("final copy did not retain KONTRA") end',
        '    local state_ok, state_back = reaper.TrackFX_GetNamedConfigParm(track, plan.fx, "vst_chunk")',
        '    if not state_ok or state_back ~= plan.expected then abort("final copy KONTRA state differs from the verified REAPER VST3 chunk") end',
        '    table.insert(final_state_checks, string.format("{\\"track_index\\":%d,\\"fx_index\\":%d,\\"expected_chunk_matches\\":true}", plan.track, plan.fx))',
        "  end",
        '  local report_body, replaced = string.gsub(REPORT_CONTENT, [["post_reopen_state": null]], [["post_reopen_state":]] .. "[" .. table.concat(final_state_checks, ",") .. "]")',
        '  if replaced ~= 1 then abort("could not build migration verification report") end',
        '  local file = assert(io.open(REPORT, "w"))',
        "  file:write(report_body, \"\\n\")",
        "  file:close()",
        '  reaper.ShowConsoleMsg("KONTRA migration saved copy: " .. OUTPUT .. "\\n")',
        "end",
        "reaper.defer(finish_migration)",
    ]
    return "\n".join(lines) + "\n"


def prepare_migration(source: Path, output: Path, script: Path, exporter: Path, plans: list[dict]) -> dict:
    source = source.resolve()
    output = output.resolve()
    script = script.resolve()
    exporter = exporter.resolve()
    if output == source:
        raise ValueError("output copy must not be the source project")
    if output.parent != source.parent:
        raise ValueError("output copy must be beside the source project to preserve relative media paths")
    if output.suffix.casefold() != ".rpp":
        raise ValueError("output copy must use the .RPP extension")
    report_path = output.with_suffix(output.suffix + ".migration.json")
    if output.exists() or script.exists() or report_path.exists():
        raise ValueError("output project, script, and report paths must not already exist")
    if not exporter.is_file() or not os.access(exporter, os.X_OK):
        raise ValueError(f"state exporter must be an executable file: {exporter}")

    with tempfile.TemporaryDirectory(prefix="kontra-migration-") as work:
        metadata = []
        for index, plan in enumerate(plans):
            state_path = Path(work) / f"{index}.state"
            state, state_report = export_state(exporter, Path(plan["multi"]), state_path)
            framed = struct.pack("<II", len(state), 1) + state
            plan["state_base64"] = base64.b64encode(framed).decode("ascii")
            plan["expected_readback_base64"] = base64.b64encode(framed + bytes(8)).decode("ascii")
            state_report.pop("state", None)
            metadata.append({
                "track_index": plan["track_index"],
                "fx_index": plan["fx_index"],
                "reaper_chunk_frame": "little-endian OAST byte length, flags=1",
                "expected_readback_trailer_bytes": 8,
                **state_report,
                "state_embedded_in_reaper_script": True,
            })
        with script.open("x", encoding="utf-8") as output_script:
            output_script.write(reaper_script(source, output, plans, metadata))
    return {
        "status": "prepared_for_reaper",
        "source": str(source),
        "output_copy": str(output),
        "reaper_script": str(script),
        "prepared_mappings": metadata,
        "unsupported_state": [
            "Kontakt opaque plugin state, static parameter values, and KSP persistence are not translated.",
            "SavedMulti lacks rack-level bus settings; KONTRA rack defaults are used.",
            "Track FX automation and nested FX containers are skipped; compatible REAPER track and pin routing remains in the project copy.",
            "KONTRA audio decode and Kontakt sonic parity are unverified.",
        ],
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("project", type=Path, help="REAPER .RPP project to inspect")
    parser.add_argument("--map", action="append", default=[], metavar="TRACK:FX=MULTI", help="map a zero-based Kontakt instance to a .kontra-multi file; repeatable")
    parser.add_argument("--output-copy", type=Path, help="new .RPP path beside the source project")
    parser.add_argument("--reaper-script", type=Path, help="write an experimental ReaScript for preflighted mappings")
    parser.add_argument("--state-exporter", type=Path, help="executable kontakto CLI built with the plugin feature")
    args = parser.parse_args(argv)
    try:
        report = inventory(args.project)
        if not args.map:
            if args.output_copy or args.reaper_script or args.state_exporter:
                raise ValueError("--output-copy, --reaper-script, and --state-exporter require at least one --map")
        else:
            plans, skipped = mapped_instances(report, args.map)
            report["migration"] = {
                "status": "dry_run_preflight",
                "mappings": plans,
                "skipped": skipped,
                "read_only": True,
                "limitations": [
                    "Kontakt state and exposed parameters are opaque unless represented in the explicit SavedMulti.",
                    "Track routing remains with REAPER; Kontakt-specific plugin pin routing is not assumed equivalent.",
                ],
            }
            if args.output_copy or args.reaper_script or args.state_exporter:
                if not (args.output_copy and args.reaper_script and args.state_exporter):
                    raise ValueError("preparing a script requires --output-copy, --reaper-script, and --state-exporter")
                if not plans:
                    raise ValueError("no mapped Kontakt instances passed safety preflight")
                report["migration"] = prepare_migration(
                    args.project, args.output_copy, args.reaper_script, args.state_exporter, plans
                ) | {"skipped": skipped, "read_only_source": True}
    except (OSError, UnicodeError, ValueError) as error:
        print(f"migrate_project: {error}", file=sys.stderr)
        return 2
    json.dump(report, sys.stdout, ensure_ascii=False, indent=2)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
