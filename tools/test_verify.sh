#!/usr/bin/env bash
# The gate's own parsers, tested against the shapes real output takes.
#
# A gate row that misreads its input fails in one of two ways, and both have
# happened on this machine: it passes what it should fail, or it fails what it
# should pass. `init unanswered` did the second -- an instrument that had
# answered every init action printed `[]`, the sum over an empty list came back
# empty, and the row reported "load printed no counts" on the one load that had
# nothing left to answer. The ceiling exists to reach zero, so the gate went red
# exactly when the work succeeded.
#
# The functions are lifted out of tools/verify.sh rather than copied: a copy
# would test the copy. If the extraction finds nothing, the calls below fail and
# so does this script -- a rename cannot make it pass vacuously.
set -uo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
WORK=$(mktemp -d "${TMPDIR:-/tmp}/verify-parsers.XXXXXX")
trap 'rm -rf "$WORK"' EXIT
FAILED=0

# shellcheck disable=SC1090
source <(sed -n '/^init_unanswered()/,/^}/p' "$HERE/verify.sh")
# The parity row's summary parser: a missing or corrupt `verify-parity.json` must
# fail the row, not print nothing and leave it reading "ok ... (no summary)".
# shellcheck disable=SC1090
source <(sed -n '/^parity_row()/,/^}/p' "$HERE/verify.sh")
# `pixel_row` and `panel_row_kind` are not tested because they are not in the
# gate: this branch has no pixel rows and no golden of v3's page (see verify.sh's
# header). A test over a helper nothing calls is the same defect as a test that
# cannot fail, so the helpers went rather than the tests staying.

check() {
    local name=$1 want=$2 file=$3 got
    got=$(init_unanswered "$file")
    if [ "$got" = "$want" ]; then
        printf '  ok   %-34s %s\n' "$name" "'$got'"
    else
        printf '  FAIL %-34s got %s, want %s\n' "$name" "'$got'" "'$want'"
        FAILED=1
    fi
}

# A load that answered everything: the target state, and it must pass.
printf 'load: 6232 zones\ninit unanswered: []\nscripts: 0 slots\n' > "$WORK/empty"
check "every action handled counts 0" 0 "$WORK/empty"

# The shape measured today on Electric Mint.
printf 'init unanswered: [(SetNksNavPar, 25), (SetNksNavName, 21), (MfSetExportArea, 2), (MfCopyExportArea, 2), (MfSetNumExportAreas, 1)]\n' > "$WORK/some"
check "the shape measured today sums to 51" 51 "$WORK/some"

# No line at all: the load failed or the summary changed shape. This must not
# read as zero -- that was the older bug, and it hid a failed load.
printf 'load: failed\n' > "$WORK/missing"
check "a missing line is not a measurement" "" "$WORK/missing"

# Present but unparseable: also not a measurement.
printf 'init unanswered: [garbage]\n' > "$WORK/malformed"
check "a malformed list is not a measurement" "" "$WORK/malformed"

# One pair among junk: a partial parse must not report a partial sum.
printf 'init unanswered: [(SetNksNavPar, 4), junk]\n' > "$WORK/partial"
check "a partly parsed list is not a measurement" "" "$WORK/partial"

# Whitespace inside the brackets must not read as an empty list.
printf 'init unanswered: [   ]\n' > "$WORK/spaces"
check "brackets holding only spaces count 0" 0 "$WORK/spaces"

# A row's summary is not optional: absence is a failure, not silence.
if parity_row "$WORK/no-such-summary.json" >/dev/null 2>&1; then
    printf '  FAIL %-34s a missing summary "passed"\n' "a missing parity summary fails"
    FAILED=1
else
    printf '  ok   %-34s\n' "a missing parity summary fails"
fi
printf 'not json\n' >"$WORK/bad-parity.json"
if parity_row "$WORK/bad-parity.json" >/dev/null 2>&1; then
    printf '  FAIL %-34s a corrupt summary "passed"\n' "a corrupt parity summary fails"
    FAILED=1
else
    printf '  ok   %-34s\n' "a corrupt parity summary fails"
fi
printf '%s' '{"cases":[{"id":"a","status":"matches","ok":true,"metrics":{"level_db":0.1,"band_rms_db":0.2}},{"id":"b","status":"DRIFT","ok":false,"metrics":{"level_db":1.0,"band_rms_db":2.0}}]}' >"$WORK/good-parity.json"
summary=$(parity_row "$WORK/good-parity.json")
case $summary in
    "2 cases, 1 finding"*"b +1.00/2.00 dB FINDING"*) printf '  ok   %-34s %s\n' "a readable summary is summarised" "'$summary'" ;;
    *) printf '  FAIL %-34s got %s\n' "a readable summary is summarised" "'$summary'"; FAILED=1 ;;
esac

# The parity runner is the gate's ruler for the corpus row: its alignment and
# its per-case containment decide what that row measures, so its self-tests run
# with the rest of the parsers. No library, no Kontakt, no binary.
if python3 "$HERE/parity-corpus/test_run.py" >"$WORK/parity-tests.log" 2>&1; then
    printf '  ok   %-34s %s\n' "parity runner self-tests" "$(tail -1 "$WORK/parity-tests.log")"
else
    printf '  FAIL %-34s see %s\n' "parity runner self-tests" "$WORK/parity-tests.log"
    FAILED=1
fi

echo
if [ "$FAILED" -ne 0 ]; then
    echo "FAILED: a gate parser misreads its input"
    exit 1
fi
echo "PASS: 6 parser cases, 3 parity-summary cases, parity runner self-tests"
