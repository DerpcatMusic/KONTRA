#!/usr/bin/env python3
"""Render the ranked table (markdown) from the same data build_tsv.py writes.

Usage: table_md.py  ->  stdout
"""
import json
import os
import sys

#: Where the survey's own files live: the census, the extracted scripts and every
#: table written by these scripts. Override with ``CENSUS_WORK``; the default is
#: the directory the 2026-11-01 pass used.
WORK = os.environ.get("CENSUS_WORK", "/var/tmp/gaps")


SHORT = {
 "GetEnginePar": "a probe to name the ids, then a read law per id",
 "GetEngineParDisp": "a display-formatting law per parameter",
 "UI asset not found": "nothing: the file is not in the library (sampled, §4)",
 "LASS `.nicnt` access data": "nothing we may do: Kontakt refuses it too",
 "$MARK_2 … $MARK_28 (27 constants)": "a ledger table to fill — the values already exist",
 "%NOTE_DURATION": "nothing: implemented, the ledger row is stale",
 "legacy XML instrument (pre-Kontakt 4)": "a format to read (Kontakt-2 XML preset)",
 "modulator category 3": "one reader case, decoded from the native reader",
 "unknown function num_slices_zone": "a zone-slice model",
 "SetEnginePar": "a write law per id, plus the async type swap",
 "no playable zone at all": "nothing: saved purge state (sampled)",
 "GetFolder": "one folder-kind enum case",
 "unknown command hide_inst_icon": "a no-op binding (one line)",
 "silent at every key with zones": "a symptom, not a gap — see §5",
 "LoadArray": "the numeric .nka array read",
 "AttachZone": "a runtime zone model",
 "SetUiWfProperty": "wire one property to the waveform view",
 "LoadIrSample": "fill the IR slot from the library's impulses",
 "LoadPerformanceView": "no format exists — cannot be done yet",
 "MfSetExportArea": "the export-area object",
 "unknown callback _pgs_changed": "a callback alias (one line, if Kontakt honours it)",
 "unknown function zone_slice_start": "the zone-slice model",
 "expected a constant": "an ordering fix: our parser refuses a `declare` Kontakt accepts",
 "SetNksNavPar": "an NKS2 navigation model (deliberately absent)",
 "SetNksNavName": "the same model",
 "$EVENT_VELOCITY is read-only": "an ordering fix: these scripts clamp velocity, Kontakt allows it",
 "GetVoiceLimit": "a real per-group voice counter",
 "SetVoiceLimit": "a per-group voice cap",
 "MfSetNumExportAreas": "the export-area object",
 "redirect_output takes 2 arguments": "an ordering fix: our signature is stricter than Kontakt's",
 "MfCopyExportArea": "the export-area object",
 "SetRpn": "an RPN port in the offline renderer",
 "not a UI control": "an ordering fix: accept a ui id held in a variable",
 "$NUM_PRESETS is a reserved name": "an ordering fix: it is not a Kontakt builtin",
 "ExposeControls": "a host-automation exposure list",
 "LoadNativeUi": "no format exists — cannot be done yet",
 "unknown function mf_get_note_length": "one accessor on data `mf_get_note` already reads",
 "unknown command dont_use_machine_mode": "a no-op binding (Kontakt's is a no-op)",
 "FindZone": "a zone name index",
 "unknown command set_num_user_zones": "the user-zone model",
 "?main_controls_inst_panels_1_icon_panel_stage is not declared": "an ordering fix: undeclared text variable",
 "GetSampleLength": "one sample-metadata field",
 "?home_vr_xypad is not declared": "an ordering fix: undeclared text variable",
 "LoadMidiFile": "a MIDI-file read into an array",
 "more values than array elements": "an ordering fix: KSP lets the surplus be ignored",
}


def main():
    tsv = sys.argv[1] if len(sys.argv) > 1 else f"{WORK}/gap-inventory.tsv"
    rows = [l.rstrip("\n").split("\t") for l in open(tsv)]
    head, body = rows[0], rows[1:]
    for r in body:
        gap, kind, ins, libs, refs, takes, ex = r[:7]
        short = SHORT.get(gap, takes[:60])
        ex0 = ex.split(";")[0]
        if " (" in ex0 and len(ex0) > 34:
            ex0 = ex0.split(" (")[0]
        print(f"| {gap} | {kind} | {int(ins):,} | {libs} | {short} | {ex0} |")


if __name__ == "__main__":
    main()
