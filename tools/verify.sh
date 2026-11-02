#!/usr/bin/env bash
# Gate a build on the self-test loop before any binary is handed over.
#
#   tools/verify.sh            run every check, print a table, exit non-zero on failure
#   tools/verify.sh --quick    skip the audio render (fast; still runs the unit layers)
#   tools/verify.sh --portable run only the rows that need nothing from this
#                              machine: no sample library, no Kontakt, no GUI.
#                              That is tests, build, clap_entry, rustfmt and
#                              clippy -- the same checks CI can run. It prints
#                              its own PORTABLE PASSED line, because it is not a
#                              delivery gate and must never be read as one.
#
# What it refuses to let through:
#   - a failing test in the crates the change touched;
#   - a plugin build with no CLAP entry (a host would not see it);
#   - Electric Mint asking for more script calls than the agreed ceiling, or a
#     load that produced no answer at all;
#   - a render that does not complete, drops sample frames, or clips;
#   - the parity corpus row cannot produce a summary at all, or one of its
#     cases could not be compared. The row measures this engine against
#     references recorded from real Kontakt and prints where it disagrees; it
#     does not gate on the size of that disagreement (the baselines are another
#     branch's engine's), so the header must not promise a page check it does
#     not make.
#
# Rows this branch does not have, and why (ported from feat/integration, whose
# test layout and tooling this gate was written for):
#   - ui-drive (layer 2): the tool is not here. It needs `kontra-ui`'s
#     `Set::Axes` and `Gesture::owner`/`entry`, which v3 does not have; porting
#     the tool would mean porting the ui_xy pointer feature, not a tool.
#   - the pixel rows (layer 3): a golden of *our* page (feat/integration's) and
#     a panel cut from Kontakt's render. v3's page has never been captured, so
#     our golden would fail on any legitimate difference; the pixel-diff tool
#     and its self-test are here, and a golden can be captured with ui-shots.
#   - tests: parity regressions: that test file is feat/integration's.
set -uo pipefail

REPO=$(cd "$(dirname "$0")/.." && pwd)
# /tmp is a 16 GB tmpfs on this machine; a release build of the workspace does
# not fit beside three lanes. Keep the target on real disk.
TARGET=${CARGO_TARGET_DIR:-/var/tmp/tgt-v3}
# $HOME rather than an absolute path: the gate is run from a checkout, and a
# committed home directory names whoever ran it last.
EMINT=${EMINT:-$HOME/Armory/VSTs/Kontakt1/Native Instruments Session Guitarist Electric Mint 1.1.0/Session Guitarist - Electric Mint Library/Instruments/Electric Mint.nki}
# Ceilings, not goals: they must only ever come down.
#
# Measured on feat/integration (the branch this gate was written for), and NOT
# raised for v3. v3 fails both on Electric Mint today: its script host leaves
# the MIDI-file object unanswered, so the instrument's init makes 10,221,505
# unanswered calls (ceiling 51) and the render 60,680,810 (ceiling 55), which
# overflows the engine's trace ring and makes `kontra-inspect play` report an
# incomplete render. That is a defect in the engine, not in the row: the row
# prints the number and the reason it is above the ceiling.
MAX_UNHANDLED=${MAX_UNHANDLED:-55}
MAX_INIT_UNANSWERED=${MAX_INIT_UNANSWERED:-51}
# A build or a GUI step that hangs is a failed row, not a hung gate: every cargo
# and ui call below is wrapped. 30 minutes is generous for a cold release build
# of the crates the gate names and still finite.
STEP_TIMEOUT=${STEP_TIMEOUT:-1800}
QUICK=
MODE=full
for arg in "$@"; do
    case "$arg" in
        --quick) QUICK=--quick ;;
        --portable) MODE=portable ;;
        *) echo "verify: unknown argument '$arg'" >&2; exit 2 ;;
    esac
done

cd "$REPO"
# One private directory holds every log, JSON and PNG this gate writes, made
# before the first redirect. A fixed /tmp path is a symlink a local user can
# pre-create (CWE-377), and on a machine without that directory the redirect
# fails and the step "fails" without ever running. Kept after the run so a
# failure message can name a file that still exists.
LOGS=${KONTRA_VERIFY_LOGS:-$(mktemp -d "${TMPDIR:-/tmp}/kontra-verify.XXXXXX")} \
    || { echo "verify: cannot create a log directory (mktemp failed)" >&2; exit 2; }
mkdir -p "$LOGS"
FAILED=0
declare -a ROWS

row() { ROWS+=("$(printf '%-42s %s' "$1" "$2")"); }
bad() { row "$1" "FAIL  $2"; FAILED=1; }
ok()  { row "$1" "ok    $2"; }

# Does a captured string contain a substring? A `case` rather than
# `printf ... | grep -q`, because with `pipefail` a grep that exits at its first
# match makes the writer take SIGPIPE, the pipeline returns 141, and
# `! printf ... | grep -q` then reads as "not found". That misfired on cargo
# fmt's 200 KB of diffs: the row reported "could not run" while holding the
# diffs in its hand. Under 64 KB (the pipe buffer) the same pattern works, which
# is why the older `test result:` checks below never showed it -- latent until a
# test log is big.
has() { case "$1" in *"$2"*) return 0 ;; *) return 1 ;; esac; }

# The load summary's total of unhandled init actions, or "" when the line is
# absent or malformed -- which the caller treats as a failure, because "no
# measurement" must never read as "nothing unanswered".
#
# A present and *empty* list is 0. That is the state the ceiling exists to
# reach, and the gate used to fail on it: `grep -o ', (n)'` finds nothing in
# `[]`, so the sum came back empty and the row said "load printed no counts" on
# the one load that had nothing left to answer.
init_unanswered() {
    python3 - "$1" <<'PY'
import re
import sys

text = open(sys.argv[1], errors="replace").read()
m = re.search(r"init unanswered: \[([^\]]*)\]", text)
if not m:
    raise SystemExit(0)                     # no line: print nothing
inner = m.group(1).strip()
if not inner:
    print(0)                                # present and empty: all handled
    raise SystemExit(0)
# Strict: every entry must be a pair. A partial match must not report a partial
# sum -- "4 of the 51" is a measurement of something else.
if not re.fullmatch(r"\([^(),]+, \d+\)(?:, \([^(),]+, \d+\))*", inner):
    raise SystemExit(0)
print(sum(int(count) for count in re.findall(r", (\d+)\)", inner)))
PY
}

# Print the table and exit, for both modes. A function rather than two copies of
# the tail, so the portable path can never drift into printing the delivery
# gate's success line.
finish() {
    echo
    printf '%s\n' "${ROWS[@]}"
    echo
    echo "logs: $LOGS"
    if [ "$FAILED" -ne 0 ]; then
        echo "VERIFY FAILED -- do not deploy"
        exit 1
    fi
    # The play dump is a few hundred MB of per-voice detail that nothing reads
    # after the rows above, and /tmp is a 16 GB tmpfs here. Keep it (and the
    # rest) only when something failed and it is worth reading.
    rm -f "$LOGS/verify-play.json"
    if [ "$MODE" = portable ]; then
        echo "PORTABLE PASSED -- not a delivery gate: the library, render, ui-drive"
        echo "and pixel rows did not run. tools/verify.sh (no flag) is what gates a deploy."
        exit 0
    fi
    echo "VERIFY PASSED -- this branch has no tools/deploy.sh: a release is a CI job"
}

# One line of parity text from the runner's JSON summary. A missing, unreadable
# or shapeless file fails: "no measurement" must never read as a pass, and the
# row this feeds used to print "(no summary)" and call itself ok.
parity_row() { # json
    python3 - "$1" 2>/dev/null <<'PY'
import json, sys
rows = json.load(open(sys.argv[1]))["cases"]
parts, findings = [], []
for r in rows:
    m = r.get("metrics") or {}
    # `ok` is the runner's own verdict: False is a case that disagrees with
    # Kontakt beyond its threshold (`DRIFT`), or could not be compared at all.
    # Counting only the runner's "finding" status missed exactly the cases the
    # corpus exists to show, and printed "0 findings" over two of them.
    mark = "" if r.get("ok") else " FINDING"
    if mark:
        findings.append(r["id"])
    if m.get("level_db") is None or m.get("band_rms_db") is None:
        parts.append(f"{r['id']} {r['status']}{mark}")
    else:
        parts.append(
            f"{r['id']} {m['level_db']:+.2f}/{m['band_rms_db']:.2f} dB{mark}"
        )
# A case that matches its baseline but still disagrees with Kontakt is a
# finding, not a pass. Printing the deltas under an unqualified "ok" is how a
# row teaches people to stop reading it -- and the whole point of the corpus is
# that these delta numbers are the to-do list, not a footnote.
lead = (f"{len(rows)} cases, {len(findings)} finding"
        f"{'' if len(findings) == 1 else 's'} (beyond its own threshold vs Kontakt): ")
print(lead + "; ".join(parts))
PY
}

run_tests() {
    local crate=$1
    local out
    out=$(CARGO_TARGET_DIR="$TARGET" timeout "$STEP_TIMEOUT" cargo test -p "$crate" 2>&1)
    local line
    line=$(printf '%s\n' "$out" | grep -E "^test result:" | awk '{p+=$4; f+=$6} END {print p" passed, "f" failed"}')
    # A crate that fails to COMPILE prints no result line at all, so "no
    # result" is a failure, not a pass -- that is how a broken test file once
    # reported ok.
    if has "$out" "test result: FAILED"; then
        bad "tests: $crate" "${line:-some tests failed}"
    elif ! has "$out" "test result:"; then
        bad "tests: $crate" "did not compile or did not run"
    else
        ok "tests: $crate" "${line:-no result line}"
    fi
}

# rustfmt, first-party only. cargo fmt --all --check exits non-zero when
# vendor/ differs, which it does and always has, so the exit code is not the
# verdict: the verdict is whether any *first-party* file appears in the diff
# list. A run that produced no diff list at all (no rustfmt, broken manifest) is
# a failure too -- "nothing to report" must never mean "passed".
lint_fmt() {
    local out status diffs
    out=$(cargo fmt --all --check 2>&1); status=$?
    diffs=$(printf '%s\n' "$out" | grep '^Diff in ' | grep -v '/vendor/' |
        sed 's/^Diff in //;s/:[0-9]*:$//' | sort -u)
    if [ -n "$diffs" ]; then
        bad "rustfmt" "$(printf '%s\n' "$diffs" | wc -l) first-party file(s) not formatted"
        printf '%s\n' "$diffs" | head -5 | sed 's/^/      /'
    elif [ "$status" -ne 0 ] && ! has "$out" "Diff in "; then
        bad "rustfmt" "could not run: $(printf '%s\n' "$out" | head -1)"
    else
        ok "rustfmt" "first-party code is formatted (vendor/ excluded)"
    fi
}

# clippy, every target, first-party only, warnings are errors. The exclusion
# list is derived from the manifests' paths, not hand-kept, so vendoring another
# crate cannot silently put it inside our lint.
lint_clippy() {
    local out status errs excludes
    excludes=$(cargo metadata --no-deps --format-version 1 2>/dev/null | python3 -c '
import json, sys
m = json.load(sys.stdin)
print(" ".join("--exclude " + p["name"] for p in m["packages"]
               if "/vendor/" in p["manifest_path"].replace("\\\\", "/")))
' 2>/dev/null)
    # Word-splitting is intended: $excludes is a list of --exclude flags.
    # shellcheck disable=SC2086
    out=$(CARGO_TARGET_DIR="$TARGET" timeout "$STEP_TIMEOUT" cargo clippy \
        --workspace $excludes --all-targets --no-deps \
        --message-format=short -- -D warnings 2>&1); status=$?
    errs=$(printf '%s\n' "$out" | grep -E '^[^ :]+:[0-9]+:[0-9]+: error' | sort -u)
    if [ -n "$errs" ]; then
        bad "clippy" "$(printf '%s\n' "$errs" | wc -l) finding(s) under -D warnings"
        printf '%s\n' "$errs" | head -5 | sed 's/^/      /'
    elif [ "$status" -ne 0 ]; then
        bad "clippy" "could not run: $(printf '%s\n' "$out" | grep -m1 '^error' || printf '%s\n' "$out" | head -1)"
    else
        ok "clippy" "first-party crates clean under -D warnings (all targets)"
    fi
}

echo "== layer 0: the gate reads its own output correctly =="
# The gate is the authority, so its parsers are checked like any other code. This
# row caught a real one on the day it was written: `init unanswered: []` -- an
# instrument that answered everything, which is the state the ceiling exists to
# reach -- read as "no measurement" and failed the gate. It runs here so a change
# to a parser cannot be the thing that makes the gate quiet.
if bash "$REPO/tools/test_verify.sh" >"$LOGS/verify-parsers.log" 2>&1; then
    ok "gate parsers" "$(tail -1 "$LOGS/verify-parsers.log")"
else
    bad "gate parsers" "see $LOGS/verify-parsers.log"
fi
# The instruments' own tests, for the same reason: every number below is produced
# by one of them, so a tool that answers the wrong question makes correct code
# look wrong and wrong code look fine. A test file nothing runs cannot tell anyone
# anything, which is how three checks in this repository were written and then
# never failed. Both of these need nothing from this machine -- no library, no
# Kontakt, no display -- and both are fast.
if bash "$REPO/tools/test_build_slot.sh" >"$LOGS/verify-build-slot.log" 2>&1; then
    ok "build-slot self-tests" "$(tail -1 "$LOGS/verify-build-slot.log")"
else
    bad "build-slot self-tests" "see $LOGS/verify-build-slot.log"
fi
# The survey harness reads the corpus and writes the gap inventory, so a path
# assumption in it does not fail loudly -- it renames a library or silently
# drops an instrument. Its self-test needs none of the survey's inputs.
if python3 "$REPO/tools/census/test_census.py" >"$LOGS/verify-census-tests.log" 2>&1; then
    ok "census harness self-tests" "$(tail -1 "$LOGS/verify-census-tests.log")"
else
    bad "census harness self-tests" "see $LOGS/verify-census-tests.log"
fi
# pixel-diff needs numpy and Pillow. Without them nothing compares pixels at all,
# so the pixel rows below fail too; this row says which is missing rather than
# reporting a test that could not run as one that passed.
if python3 -c 'import numpy, PIL' 2>/dev/null; then
    if python3 "$REPO/tools/test_pixel_diff.py" >"$LOGS/verify-pixel-diff-tests.log" 2>&1; then
        ok "pixel-diff self-tests" "$(tail -1 "$LOGS/verify-pixel-diff-tests.log")"
    else
        bad "pixel-diff self-tests" "see $LOGS/verify-pixel-diff-tests.log"
    fi
else
    row "pixel-diff self-tests" "not run: no numpy/Pillow (the pixel rows cannot run either)"
fi

echo "== layer 0/1: tests =="
# kontra-plugin and kontra-ui-pictures are here because the plugin is what
# ships: a page that renders in the harness and not in the host is exactly how
# the wallpaper went missing (a 7952-row picture became 15904 at the live 2x
# device scale, past the renderer's 8192-pixel atlas, and was dropped silently).
#
# kontra-racks, kontra-ledger and kontra-inspect are here because their tests are
# not run by another crate's `cargo test` (`-p kontra-engine` compiles racks
# without running it). kontra-inspect is the gate's own instrument -- it measures
# every audio row below -- so a silent break there would make the gate report a
# pass it could not justify.
#
# `ui-drive` is not in this list: it is not on this branch (see the header).
for c in kontra-engine kontra-host kontra-ui mui kontra-native-host kontra-plugin kontra-ui-pictures kontra-racks kontra-ledger kontra-inspect; do run_tests "$c"; done
if [ -d crates/dsp/delay ]; then run_tests kontra-dsp-delay; fi

# The gate and CI both test in debug. A test that only fails under optimization
# is invisible to both, and `kontra-law` had exactly one until 2026-11-01: two
# spellings of `2^(x/12)` agreeing in debug and splitting by one ulp in release.
# The law crate is pure arithmetic and its suite runs in hundredths of a second,
# so it is held to both profiles here.
if CARGO_TARGET_DIR="$TARGET" timeout "$STEP_TIMEOUT" cargo test --release -q -p kontra-law \
        >"$LOGS/verify-law-release.log" 2>&1; then
    ok "tests: kontra-law (release)" \
        "$(grep -E '^test result' "$LOGS/verify-law-release.log" | awk '{p+=$4; f+=$6} END {print p" passed, "f" failed"}')"
else
    bad "tests: kontra-law (release)" "see $LOGS/verify-law-release.log"
fi

# The two parity regressions are feat/integration's test file
# (`crates/app/host/tests/parity_regressions.rs`, the XPERIMENTA Harpsichord and
# Cherry Electric Bass findings); v3 has no such file, so the row is not here.
# The parity corpus below is what holds audio to a file on this branch.

echo "== build =="
if CARGO_TARGET_DIR="$TARGET" timeout "$STEP_TIMEOUT" cargo build --release -p kontra-plugin >"$LOGS/verify-build.log" 2>&1; then
    ok "build kontra-plugin" "$(stat -c%s "$TARGET/release/libkontra_plugin.so" 2>/dev/null || echo '?') bytes"
else
    bad "build kontra-plugin" "see $LOGS/verify-build.log"
fi
if nm -D --defined-only "$TARGET/release/libkontra_plugin.so" 2>/dev/null | grep -q " clap_entry$"; then
    ok "exports clap_entry" "a CLAP host will see it"
else
    bad "exports clap_entry" "the .so has no CLAP entry"
fi

echo "== layer 0/1: the style the review checklist claims =="
# Neither this gate nor CI ran rustfmt or clippy before 2026-10-31, so the
# review checklist's "fmt and clippy clean on the touched crates" was a
# convention with nothing behind it: a warning could be merged and nobody would
# know for a week. Both are rows now, and both are strict.
#
# vendor/ is excluded from both. It is third-party source we consume, its
# formatting is not ours to churn, and it carries two clippy errors we did not
# write (vendor/mui's #[allow] without a reason). Excluding by manifest path
# rather than by a hand-kept list, so vendoring another crate cannot silently
# put it inside our lint.
lint_fmt
lint_clippy

# Portable mode stops here. Everything below needs this machine's sample
# library, and on a runner that has none the rows would print "skipped" -- a
# word this project has learned to read as "passed". Named as not-run instead.
if [ "$MODE" = portable ]; then
    row "load + render (Electric Mint)" "local-only, not run (--portable)"
    row "library free of probe copies" "local-only, not run (--portable)"
    row "parity corpus (3 cases)" "local-only, not run (--portable)"
    finish
fi

echo "== layer 0: Electric Mint =="
if CARGO_TARGET_DIR="$TARGET" cargo build --release -p kontra-inspect >"$LOGS/verify-inspect.log" 2>&1; then
    INSPECT="$TARGET/release/kontra-inspect"
    LOADOUT=$("$INSPECT" load "$EMINT" 2>&1)
    LOADSTATUS=$?
    printf '%s\n' "$LOADOUT" >"$LOGS/verify-load.txt"
    LOAD=$(printf '%s\n' "$LOADOUT" | grep -o "init unanswered: \[[^]]*\]" | head -1)
    # A sum over nothing is 0, which used to read as "nothing unanswered" when
    # the load had failed, the instrument was missing, or the output changed
    # shape. An empty sum is now distinguishable from a measured zero.
    INIT=$(init_unanswered "$LOGS/verify-load.txt")
    if [ "$LOADSTATUS" -ne 0 ]; then
        bad "init unanswered" "kontra-inspect load exited $LOADSTATUS; see $LOGS/verify-load.txt"
    elif [ -z "$LOAD" ] || [ -z "$INIT" ]; then
        bad "init unanswered" "load printed no 'init unanswered:' counts; see $LOGS/verify-load.txt"
    elif [ "$INIT" -le "$MAX_INIT_UNANSWERED" ]; then
        ok "init unanswered" "$INIT (ceiling $MAX_INIT_UNANSWERED)"
    else
        bad "init unanswered" "$INIT above ceiling $MAX_INIT_UNANSWERED"
    fi

    if [ "$QUICK" != "--quick" ]; then
        "$INSPECT" play "$EMINT" --json "$LOGS/verify-play.json" >"$LOGS/verify-play.txt" 2>&1
        PLAYSTATUS=$?
        if [ "$PLAYSTATUS" -ne 0 ] || [ ! -f "$LOGS/verify-play.json" ]; then
            bad "render completes" "kontra-inspect play exited $PLAYSTATUS; see $LOGS/verify-play.txt"
        else
            read -r DONE UNH FNV FRAMES CLIPPED < <(python3 - "$LOGS/verify-play.json" "$LOGS/verify-play.txt" <<'PY'
import json,re,sys
d=json.load(open(sys.argv[1]))['result']
m=re.search(r'actions_unhandled: (\d+)', d['engine_stats'])
txt=open(sys.argv[2]).read()
fr=re.search(r'missing sample frames: (\d+)', txt)
cl=re.search(r'clipped samples (\d+)', txt)
print(d['render_complete'], m.group(1) if m else '?', d['audio_fnv1a64'],
      fr.group(1) if fr else '?', cl.group(1) if cl else '?')
PY
)
            [ "$DONE" = "True" ] && ok "render completes" "yes" || bad "render completes" "no"
            [ "${UNH:-999999}" -le "$MAX_UNHANDLED" ] 2>/dev/null && ok "actions_unhandled" "$UNH (ceiling $MAX_UNHANDLED)" || bad "actions_unhandled" "$UNH above ceiling"
            [ "${FRAMES:-1}" = "0" ] && ok "missing sample frames" "0" || bad "missing sample frames" "$FRAMES"
            [ "${CLIPPED:-1}" = "0" ] && ok "clipped samples" "0" || bad "clipped samples" "$CLIPPED"
            ok "audio fingerprint" "$FNV"
        fi
    fi
else
    bad "build kontra-inspect" "see $LOGS/verify-inspect.log"
fi

echo "== layer 0: the sample library is not our scratch space =="
LIB=$(dirname "$(dirname "$EMINT")")
LEFTOVER=$(ls -d "$LIB"/Instruments/__* 2>/dev/null | head -1)
if [ -n "$LEFTOVER" ]; then
    bad "library free of probe copies" "left behind: $(basename "$LEFTOVER")"
else
    ok "library free of probe copies" "no __* directory under Instruments/"
fi

echo "== layer 0: the parity corpus still sounds like Kontakt =="
# Twenty cases hold the parity numbers to files instead of to an afternoon's memory: each is a
# note real Kontakt played, committed as a WAV with the MIDI that made it, captured twice so the
# instrument's own take-to-take spread sets the threshold. This row runs the gate's three
# (Glory Days Trumpet 1, Fredonia Grand Organ, Electric Mint) -- no Kontakt, no display, only the
# committed references and a KONTRA render.
#
# The cases' recorded baselines are feat/integration's engine, so on this branch "moved" is a
# comparison between two engines, not a regression, and the row measures rather than gates:
# every case's delta against Kontakt is printed, and the row fails when the corpus cannot run at
# all (exit 2), a case could not be compared, or the runner wrote no usable summary -- a missing
# summary used to print nothing and leave the row reading "ok ... (no summary)".
# Re-recording a baseline needs no Kontakt: `run.py --record` renders with this branch's binary.
# Only `run.py --capture` (re-capturing the reference WAVs) needs a Kontakt session.
if CARGO_TARGET_DIR="$TARGET" cargo build --release -p kontra-host --bin kontra-render \
        >"$LOGS/verify-render.log" 2>&1; then
    parity_status=0
    KONTRA_RENDER="$TARGET/release/kontra-render" python3 "$REPO/tools/parity-corpus/run.py" \
        --subset --json "$LOGS/verify-parity.json" >"$LOGS/verify-parity.txt" 2>&1 || parity_status=$?
    if [ "$parity_status" -le 1 ]; then
        if ! PARITYROW=$(parity_row "$LOGS/verify-parity.json" 2>>"$LOGS/verify-parity.txt"); then
            # A missing, corrupt or shapeless summary is a failed row. It used to
            # print nothing and leave the row reading "ok ... (no summary)", which
            # is the one way this row could vanish instead of failing.
            bad "parity corpus (3 cases)" "no usable summary in $LOGS/verify-parity.json; see $LOGS/verify-parity.txt"
        elif [ -z "$PARITYROW" ]; then
            bad "parity corpus (3 cases)" "the summary in $LOGS/verify-parity.json is empty; see $LOGS/verify-parity.txt"
        else
            ok "parity corpus (3 cases, measured)" "$PARITYROW"
        fi
    else
        bad "parity corpus (3 cases)" "the corpus could not run (exit $parity_status); see $LOGS/verify-parity.txt"
        grep -E '^\[(FAIL|ok )\]|^       ' "$LOGS/verify-parity.txt" | sed 's/^/      /' | head -12
    fi
else
    bad "parity corpus (3 cases)" "kontra-render did not build; see $LOGS/verify-render.log"
fi

echo "== layer 2/3: not on this branch =="
# Both rows measure the editor's page: ui-drive presses every visible control at
# its drawn centre, and pixel-diff compares the page to a golden captured from
# this tree. Neither tool's inputs exist here -- ui-drive needs `Set::Axes` and
# `Gesture::owner`, and no golden of v3's page has ever been captured -- so the
# rows are named as absent rather than reported as passes. Capturing a golden is
# `cargo run --release -p ui-shots -- <dir> <instrument>`, which is on this
# branch; porting ui-drive is a UI feature, not a tool.
row "ui-drive (both pages)" "not on this branch (no ui-drive, no Set::Axes)"
row "pixel diff (golden + Kontakt panel)" "not on this branch (no golden of v3's page)"

finish
