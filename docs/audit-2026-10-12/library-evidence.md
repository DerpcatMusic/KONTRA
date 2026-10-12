# Library evidence interpretation — 2026-10-12

This audit separates installed inventory, metadata translation, authored UI,
audition admission, source-family selection, and native Kontakt/Falcon audio.
The current run and its exact binary digests belong in
[library-results.md](library-results.md). Historical observations below are
preserved evidence, not a new run of the current source or a dirty v1 worktree.

## Coverage and trustworthy denominators

Fresh native directory enumeration found **1,495 preset/container IDs across
15 libraries**: 781 NKI, 54 NKM and 660 UVI programs. UVI contributes 620
Augmented Orchestra, 16 Clarinets, 15 Double Reeds and 9 Flutes programs.
The earlier 26 UFS physical-file count is not the UVI program denominator.
That first filesystem list could not establish bank-program coverage.

The full parse tier and per-library Original/audio smoke test answer different
questions. Parse checks metadata translation and binding without decoded PCM.
Kontakt uses shared placeholder silence while lowering. In `check_multi`,
parse checks **at most the first embedded program**; `multi.programs` and
`multi.checked` retain the denominator and actual number checked. A complete
manifest pass therefore does not translate every embedded NKM program.
`NoZone` can describe an empty/controller program; it is not automatically
corrupt format data. Parse coverage provides no authored UI, sample-decode,
audibility, calibrated performance or native full-patch parity verdict.

Representative Original/audio observations cover each installed library but
do not cover all of its presets. In a multi, `plays_note=yes` means at least
one observed program sounded; consult per-program admission and picks before
calling the entire multi playable. A short-window `silent` result means no
audible output in that window, not proof that a longer audition stays silent.

## Historical scanner observations

These unchanged owner-local TSV/cache files were read, not rerun:

| Evidence | Items | Load admission | Original UI | Short audition | Note plan |
|---|---:|---|---|---|---|
| `~/.cache/kontakto-survey/out-v1` | 6 | 5 yes / 1 no | 3 original-ok / 3 missing-images | 6 yes | 5 matched / 1 mismatch |
| `~/.cache/kontakto-survey/out` | 16 | 16 yes | 13 original-ok / 1 blank / 2 missing-images | 15 yes / 1 silent | 15 matched / 1 mismatch |
| `~/.cache/kontakto-survey/prev-e9d918e9/out` | 16 | 16 yes | 13 original-ok / 1 error / 2 missing-images | 13 yes / 3 silent | 15 matched / 1 fallback |

All three receipts **lack the current dropped-slot and native-family schema**.
Their omitted fields are `schema_unknown`, not zero dropped modules or a family
MATCH. An absence of script faults does not establish implemented DSP,
all callback behavior, valid widget gestures or native host parity.

The cache's `revision` is the exact scanner binary digest. The retained v1
and current historical v2 binaries independently hash to their cache revisions:

| Historic scanner | SHA-256 |
|---|---|
| Kontakt v1 scanner | `ac5aed734bb7fca40d6d000ff1f3e89128436b8f38d9d674fd70466f90dcdf08` |
| Survey v2 scanner | `840341e5809c7c73074171b42ef76aec3c10b2e09edbcc8e9bd5c4d734414783` |
| Previous v2 scanner, cache witness | `1cf74e962ab015eb785b96ecef8ebd7c51030e6f4d20a938aacc99f93dc804a7` |

The previous binary's digest above is retained cache provenance; its executable
was not independently rehashed in this lane. Source comparisons use committed
Kontakt v1 `0cb7a8a0` and later UVI v1 `4bffbb18`. Their findings are described
in [kontakt-parity.md](kontakt-parity.md) and
[falcon-parity.md](falcon-parity.md). Neither a source match nor a frozen scanner
run measures the extensive uncommitted v1 checkout.

## Findings that require careful interpretation

**Alto Flute has delayed audition admission evidence.** The historical survey
v2's 480-ms probe reports silence with all 22 controls bound, 18 successful
asset decodes, no Lua init/runtime faults and no underruns. The retained
[onset investigation](../re/uvi-survey-vwinds-level-investigation.md#alto-flute-onset-admission-not-convolution-attenuation)
places the first audition C4 source record and first master output above
1e-5 in the same block: old frame 8,832 (184 ms), newer frame 65,536
(1,365.3 ms). New output and audition-source admission were absent during the
first second. A longer final untraced host run did emit audio, peak
0.149784595. Instrumentation, generated velocities and zone selection differ;
these are admission-delay observations, not calibrated native latency.
Lua scheduling versus script-requested delay remains unresolved.

**Reverb peak differences do not justify gain compensation.** The same
investigation isolates the changed stage to shaped convolution rather than
GainMatrix. Older v2 raw-response peaks are not a native target. The frozen
UVI v1 reference independently uses the shaped/normalized response contracts
and produces lower, nonzero peaks. Retained scanner and live-host schedules,
resident/streamed PCM, script timing and measurement windows differ, so an
exact v1/v2 loudness ratio is not established. Native absolute-level parity
still needs a calibrated native recording and stage/voice evidence.

**Conflux's multi does not have a valid whole-program audio comparison.**
Historical v1 failed program 0's admission while program 1 sounded. Newer
historical v2 admits program 0 with zero sample zones and a null audition pick;
program 1 supplies the audible result. The older v2 receipt used a fallback,
while the newer one is an audition mismatch. The initialized performance-view
flags and physical slot ownership in
[survey-conflux-gaps.md](../re/survey-conflux-gaps.md) require actual native
activation/selection evidence. Selecting the largest helper widget list does
not establish the correct authored panel. Blank/error/missing-image categories
must remain independent of the one audible program.

**Pacific and Vista image gaps are not a v2-only regression.** Both frozen
Kontakt v1 and historical v2 report missing images while sounding on the
selected note. Current lookup diagnostics distinguish unavailable versus absent
members. The worktree review found the older resource-index fix already
superseded; supplied `pic`/`admin_bg` requests were absent in historical data.
Successful audio does not satisfy those UI requests, and inventing an image
would not restore authored fidelity.

## Reproducible aggregation

`tools/library_audit_summary.py` reads scanner TSVs with matching numeric JSON
cache evidence, and optional corpus-health JSONL. It never launches an engine.
It records input/artifact SHA-256s, per-library expected/observed counts,
missing IDs, per-program admission, UI/audio/note-plan statuses, enabled and
bypassed drop counts, native-family status, and partial embedded-program checks.
Missing fields remain distinct from explicit `UNKNOWN` verdicts and known zeros.
Conflicting cache revisions are classified as ambiguous rather than choosing a
digest. Duplicate scanner IDs fail instead of inflating completeness.

For pairwise same-note telemetry it excludes mismatched/fallback/null plans,
unknown pick origins, failed embedded programs and different keyswitch/held-note
plans. Eligibility establishes only a common audition, not native sound identity
or a performance comparison. A static family MATCH covers native-reader source
candidates, cursor, direction, loops and repeated cycle support. Script-driven
selection, multi-note plans and unverified native conditions require capture.
That oracle does not compare full-patch audio or prove native GUI behavior.

## Conservative native-family marker correction

The independent Python oracle previously interpreted StartCriteria mode 0
(`Always`) as boolean true, including interior AND_NOT combinations. The
[NI Group Start Options manual](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/classic-view),
verified on 2026-10-12, calls Always a "no-operation value": alone it permits
ordinary admission; after another condition it marks the end of the list and
does not enter the expression. The incoming join to that terminal row therefore
does not change the preceding real predicate.

The oracle now preserves no-criteria and lone-Always admission, excludes the
terminal marker and its incoming join, and returns typed
`UNKNOWN / native-start-interior-always-requires-capture` for an interior marker.
That conservative interior verdict is an inference from the documented marker
semantics, not a measured native evaluation law. It does not change runtime DSP
or invent a predicate to satisfy a fixture. Thirty-two-take regressions cover
interior AND, AND_NOT and OR, an interior marker between real conditions,
ordinary key/controller/cycle predicates with a terminal marker, lone Always,
and no criteria. `check-native-family.py` and the shared `kontra-scan/check.py`
both pass. The exporter recalculates these verdicts from raw receipts, so
historical family results can be corrected without repeating PCM work.

Interpreter: CPython **3.14.7**, `/usr/bin/python3.14`, SHA-256
`e9db67955243120e49352dc87977cdfb96a1dc130b58cfac7a0ab5f4f74731af`.
Tool and interpreter digests are retained in the owner-local
`/tmp/kontra-parity-audit-20261012/library-evidence-tools.json` receipt.

```sh
python3 tools/library_audit_summary.py \
  --scan fresh=/path/to/library-scan/results.tsv \
  --corpus parse=/path/to/corpus-parse.jsonl \
  --inventory /path/to/all-items-current.tsv \
  --inventory-status complete-installed-program-manifest \
  --artifact scanner=/path/to/kontra_scan \
  --out /path/to/library-evidence
python3 -m unittest discover -s tools -p test_library_audit_summary.py
```

Validation in this lane: **7 tests passed**. They cover omitted versus explicit
unknown metrics, invalid drop payloads, incomplete manifests/multis, ambiguous
cache provenance, duplicate IDs and unsafe comparison plans. Historical
aggregation was exercised on all 6/16/16 retained records. Fresh execution and
its final interpretation remain in the coordinator's library-results receipt.
