#!/usr/bin/env bash
# Prove tools/build-slot behaves without building anything: no cargo, no
# workspace compile. Every case runs against KONTRA_SLOT_ROOT inside a throwaway
# /var/tmp directory, so a bug here cannot touch the real slots or /var/tmp/tgt-*.
#
#   tools/test_build_slot.sh
#
# /var/tmp, not /tmp: the point of the tool is that slots live on real disk, and
# a test that put them on the 16 GB tmpfs would be testing the wrong filesystem.
set -euo pipefail
export LC_ALL=C

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
slot="$repo_root/tools/build-slot"
tmp=$(mktemp -d /var/tmp/build-slot-test.XXXXXX)
trap 'rm -rf -- "$tmp"' EXIT
export KONTRA_SLOT_ROOT="$tmp/slots"
root=$KONTRA_SLOT_ROOT

pass=0
fail=0
ok()   { pass=$((pass + 1)); printf 'ok   %s\n' "$1"; }
bad()  { fail=$((fail + 1)); printf 'FAIL %s\n' "$1"; }
check() { # description actual expected
    if [ "$2" = "$3" ]; then ok "$1"; else
        bad "$1"
        printf '       expected: %s\n       actual:   %s\n' "$3" "$2"
    fi
}
contains() { # description haystack needle
    case $2 in *"$3"*) ok "$1" ;; *) bad "$1"; printf '       missing: %s\n       in: %s\n' "$3" "$2" ;; esac
}

# Run the slot and keep its status, without `set -e` swallowing it. A step whose
# only proof is "the test did not die" is not a step that was checked: `--gc` at
# the bottom of this file used to run as a bare assignment, so a non-zero exit
# killed the script before the assertion that reads the status could run, and a
# failure was reported as a missing summary rather than as a named failure.
out=
rc=0
attempt_in() { # dir command...
    local d=$1; shift
    out=$( cd "$d" && "$@" 2>&1 ) && rc=0 || rc=$?
    return 0
}

# The free-space floor is driven by a figure the test pins, never by this
# machine's disk. Two host dependencies used to hide here:
#
#   - the refusal was reached with `--min-free-gb 999999`, a floor no disk
#     meets, so the case proved "this always refuses" and could say nothing
#     about whether the comparison against the floor is right;
#   - every other case expected a build to run, which needs more real free
#     space than the 40 GB default floor -- true on this machine today, false on
#     a fuller one. A test that passes or fails with how full the disk is tells
#     you nothing about the code.
#
# 64 GiB is comfortably above the floor; the cases that want less say so
# themselves, per invocation.
export KONTRA_SLOT_FREE_BYTES=$(( 64 * 1024 * 1024 * 1024 ))

[ -x "$slot" ] || { echo "tools/build-slot is not executable" >&2; exit 1; }

# --- a fake worktree: a real repository, small, on real disk ----------------
mkrepo() { # path
    git init -q --initial-branch=main "$1"
    git -C "$1" config user.email t@t
    git -C "$1" config user.name t
    printf 'a\n' >"$1/a"
    git -C "$1" add a
    git -C "$1" commit -qm one
}

# A lane merged into main and left clean: HEAD on lane, lane == main's merge.
merged_repo() { # path
    mkrepo "$1"
    git -C "$1" checkout -q -b lane
    printf 'b\n' >>"$1/a"; git -C "$1" commit -qam two
    git -C "$1" checkout -q main
    git -C "$1" merge -q --no-ff lane -m merge
    git -C "$1" checkout -q lane
}

# --- 1. the command receives the slot it was told to use --------------------
merged_repo "$tmp/wt-alloc"
got=$(cd "$tmp/wt-alloc" && "$slot" --name alpha bash -c 'printf "%s" "$CARGO_TARGET_DIR"' 2>/dev/null)
check "--name alpha sets CARGO_TARGET_DIR" "$got" "$root/kontra-slot-alpha"

got=$(cd "$tmp/wt-alloc" && "$slot" bash -c 'printf "%s" "$CARGO_TARGET_DIR"' 2>/dev/null)
check "default name is the worktree basename" "$got" "$root/kontra-slot-wt-alloc"

[ -f "$root/kontra-slot-wt-alloc.slot" ] && ok "marker written on every run" || bad "marker written on every run"
contains "marker records worktree" "$(cat "$root/kontra-slot-wt-alloc.slot")" "$tmp/wt-alloc"
contains "marker records branch" "$(cat "$root/kontra-slot-wt-alloc.slot")" "branch='lane'"
contains "marker records base" "$(cat "$root/kontra-slot-wt-alloc.slot")" "base='main'"
contains "marker records last use" "$(cat "$root/kontra-slot-wt-alloc.slot")" "last_used='"

# The target directory must be a plain directory under the root, never a
# hand-named legacy dir.
contains "slot lives under KONTRA_SLOT_ROOT" "$got" "$root/kontra-slot-"
case $got in *"/tgt-"*) bad "slot must not use the legacy tgt-* names" ;; *) ok "slot must not use the legacy tgt-* names" ;; esac

# A second worktree with the same basename must not adopt that slot: sharing a
# target directory between worktrees is how this workspace got false passes.
mkdir -p "$tmp/other/wt-alloc"
merged_repo "$tmp/other/wt-alloc"
rm -f "$tmp/ran"
set +e
out=$(cd "$tmp/other/wt-alloc" && "$slot" bash -c "touch '$tmp/ran'" 2>&1)
rc=$?
set -e
[ "$rc" -ne 0 ] && ok "a slot is not shared between same-named worktrees ($rc)" || bad "a slot is not shared between same-named worktrees"
[ -e "$tmp/ran" ] && bad "collision must not run the command" || ok "collision must not run the command"
contains "collision names the owning worktree" "$out" "$tmp/wt-alloc"

# --- 2. exit code is the command's, not the wrapper's -----------------------
set +e
(cd "$repo_root" && "$slot" --name exit7 bash -c 'exit 7' >/dev/null 2>&1)
rc=$?
set -e
check "exit status propagates (7)" "$rc" "7"

# A command's own flags are not parsed as this script's.
got=$(cd "$repo_root" && "$slot" --name exit7 printf '%s' --gc 2>/dev/null)
check "flags after the command reach the command" "$got" "--gc"

# --- 3. the free-space floor refuses before running, and says how to reclaim -
# "Enough" and "one byte short" are both decided by a pinned figure, so these
# cases mean the same thing on an empty disk and on a full one. The floor is the
# default 40 GB unless the case names another.
floor=$(( 40 * 1024 * 1024 * 1024 ))

# Exactly the floor is not below it: the build must run. This is the half the
# old case could not test at all, so a comparison that always refused passed.
rm -f "$tmp/ran"
attempt_in "$repo_root" env KONTRA_SLOT_FREE_BYTES=$floor \
    "$slot" --name atfloor bash -c "touch '$tmp/ran'"
check "free space exactly at the floor runs the command" "$rc" "0"
[ -e "$tmp/ran" ] && ok "a build at the floor runs" || bad "a build at the floor runs"

# One byte short of the floor: refused, and nothing was started. The comparison
# itself is in KB (df's unit, and KONTRA_SLOT_FREE_BYTES is divided by 1024), so
# a one-byte shortfall is reported as 1 KB -- the refusal is exact, its wording
# is rounded up to the unit free space is measured in.
rm -f "$tmp/ran"
attempt_in "$repo_root" env KONTRA_SLOT_FREE_BYTES=$(( floor - 1 )) \
    "$slot" --name lowspace bash -c "touch '$tmp/ran'"
check "one byte below the floor exits 75" "$rc" "75"
[ -e "$tmp/ran" ] && bad "low space must not run the command" || ok "low space must not run the command"
contains "refusal names the floor" "$out" "floor"
contains "refusal names the shortfall" "$out" "short of"
contains "refusal reports the shortfall it was given" "$out" "1.0 KB short"
contains "refusal lists largest slots" "$out" "reclaim candidates"
contains "refusal prints the gc command" "$out" "--gc"
[ -d "$root/kontra-slot-lowspace" ] && bad "refusal leaves no slot directory" || ok "refusal leaves no slot directory"

# A named floor is honoured against the same figure: 2147483647 bytes (2 GiB)
# against --min-free-gb 3 refuses, against --min-free-gb 1 runs. Without this,
# a floor that was read but never compared would pass every case above.
rm -f "$tmp/ran"
attempt_in "$repo_root" env KONTRA_SLOT_FREE_BYTES=2147483647 \
    "$slot" --name tiny --min-free-gb 3 bash -c "touch '$tmp/ran'"
check "a named floor refuses below it" "$rc" "75"
[ -e "$tmp/ran" ] && bad "a named floor must not run the command" || ok "a named floor must not run the command"
rm -f "$tmp/ran"
attempt_in "$repo_root" env KONTRA_SLOT_FREE_BYTES=2147483647 \
    "$slot" --name tiny --min-free-gb 1 bash -c "touch '$tmp/ran'"
check "a named floor runs above it" "$rc" "0"
[ -e "$tmp/ran" ] && ok "a named floor runs the command" || bad "a named floor runs the command"

# --- 4. --list reports size and owner --------------------------------------
head -c 2097152 /dev/zero >"$root/kontra-slot-alpha/blob"
out=$(cd "$repo_root" && "$slot" --list 2>/dev/null)
contains "--list names the slot" "$out" "wt-alloc"
contains "--list reports the owning worktree" "$out" "$tmp/wt-alloc"
contains "--list reports a size" "$out" "MB"
contains "--list reports the branch" "$out" "lane"
contains "--list totals the slots" "$out" "total"

# --- 5. --gc removes merged+clean, skips unmerged and current --------------
# A legacy hand-named directory sits in the same root to prove --gc leaves it
# alone: it is not a slot, and removing it is not this tool's decision.
mkdir -p "$root/tgt-legacy" && printf 'x\n' >"$root/tgt-legacy/keep"

merged_repo "$tmp/wt-merged"                       # merged and clean -> removed
merged_repo "$tmp/wt-current"                      # merged and clean, but gc runs here -> kept
mkrepo "$tmp/wt-ahead"                             # a commit main does not have -> kept
git -C "$tmp/wt-ahead" checkout -q -b lane
printf 'b\n' >>"$tmp/wt-ahead/a"; git -C "$tmp/wt-ahead" commit -qam two

for r in wt-merged wt-current wt-ahead; do
    (cd "$tmp/$r" && "$slot" --base main true) 2>/dev/null
done

# Give the merged slot something to reclaim, so the byte count is meaningful.
head -c 1048576 /dev/zero >"$root/kontra-slot-wt-merged/blob"

# The status is asserted, not assumed: under `set -e` a non-zero `--gc` here
# killed the script before this line, so the check could only ever read 0 and a
# failed gc was reported as a truncated run with no summary.
attempt_in "$tmp/wt-current" "$slot" --gc
check "--gc exits 0" "$rc" "0"
if [ "$rc" -ne 0 ]; then printf '%s\n' "$out" | sed 's/^/       /'; fi
contains "--gc removes the merged+clean slot" "$out" "remove wt-merged"
[ -d "$root/kontra-slot-wt-merged" ] && bad "merged slot directory removed" || ok "merged slot directory removed"
[ -f "$root/kontra-slot-wt-merged.slot" ] && bad "merged slot marker removed" || ok "merged slot marker removed"
contains "--gc keeps the current slot" "$out" "current worktree"
[ -d "$root/kontra-slot-wt-current" ] && ok "current slot directory kept" || bad "current slot directory kept"
contains "--gc skips the unmerged lane" "$out" "unmerged"
[ -d "$root/kontra-slot-wt-ahead" ] && ok "unmerged slot directory kept" || bad "unmerged slot directory kept"
contains "--gc reports bytes reclaimed" "$out" "reclaimed"
contains "--gc never touches legacy tgt-*" "$( [ -f "$root/tgt-legacy/keep" ] && echo kept || echo gone )" "kept"

# --- 6. a dirty lane is skipped, not deleted --------------------------------
merged_repo "$tmp/wt-dirty2"
(cd "$tmp/wt-dirty2" && "$slot" --base main true) 2>/dev/null
printf 'scratch\n' >"$tmp/wt-dirty2/untracked-file"
out=$(cd "$repo_root" && "$slot" --gc 2>&1)
contains "--gc skips a dirty worktree" "$out" "dirty"
[ -d "$root/kontra-slot-wt-dirty2" ] && ok "dirty slot directory kept" || bad "dirty slot directory kept"

# --- 7. a slot whose build is still running is never reclaimed --------------
# The bug: every question --gc asked was about the *branch* (merged? clean?),
# and all of them stay true while a build links. A lane merged mid-build had its
# target directory deleted underneath it. The slot now records that a build is
# running, so "busy" is a fact --gc reads rather than a state it infers from how
# merged and clean the lane looks.
merged_repo "$tmp/wt-building"
rm -f "$root/kontra-slot-wt-building.slot"
( cd "$tmp/wt-building" && "$slot" --base main sleep 3 ) >/dev/null 2>&1 &
building=$!
# Wait for the marker to say so, rather than guessing with a sleep: the whole
# point is that this fact exists before the command starts.
for _ in $(seq 100); do
    grep -q "in_use='1'" "$root/kontra-slot-wt-building.slot" 2>/dev/null && break
    sleep 0.1
done
contains "a running build's marker says so before it starts" \
    "$(cat "$root/kontra-slot-wt-building.slot" 2>/dev/null)" "in_use='1'"

attempt_in "$repo_root" "$slot" --gc
check "--gc exits 0 while a build runs" "$rc" "0"
contains "--gc keeps a slot whose build is running" "$out" "in use by pid"
[ -d "$root/kontra-slot-wt-building" ] && ok "the running build's slot is kept" || bad "the running build's slot is kept"
[ -f "$root/kontra-slot-wt-building.slot" ] && ok "the running build's marker is kept" || bad "the running build's marker is kept"

wait "$building" 2>/dev/null || true
contains "a finished build clears the flag" \
    "$(cat "$root/kontra-slot-wt-building.slot" 2>/dev/null)" "in_use='0'"

# And once it has ended, the same merged+clean slot is reclaimed: the guard must
# not turn --gc into a no-op for every lane that built anything this hour.
attempt_in "$repo_root" "$slot" --gc
contains "--gc reclaims the slot once the build has ended" "$out" "remove wt-building"
[ -d "$root/kontra-slot-wt-building" ] && bad "a finished build's slot is reclaimable" || ok "a finished build's slot is reclaimable"

# --- 8. a marker that cannot say whether it is busy is given the window -----
# Markers written by the build-slot that predates `in_use` carry only a
# timestamp, and a wrapper killed by SIGKILL leaves `in_use=1` with a dead pid.
# Both are held for the build window rather than reclaimed on a guess; the
# window is a knob so this is decided by the test, not by the clock.
merged_repo "$tmp/wt-legacy"
(cd "$tmp/wt-legacy" && "$slot" --base main true) 2>/dev/null
printf "name='wt-legacy'\nworktree='%s'\nbranch='lane'\nbase='main'\nlast_used='%s'\n" \
    "$tmp/wt-legacy" "$(date +%s)" >"$root/kontra-slot-wt-legacy.slot"
attempt_in "$repo_root" "$slot" --gc
contains "a marker with no in_use is presumed live" "$out" "build window"
[ -d "$root/kontra-slot-wt-legacy" ] && ok "a legacy marker's slot is kept for the window" || bad "a legacy marker's slot is kept for the window"

attempt_in "$repo_root" env KONTRA_SLOT_BUILD_WINDOW=0 "$slot" --gc
contains "the build window is what held it" "$out" "remove wt-legacy"

# A flag whose build died without clearing it is not busy for ever: the pid is
# gone, so the same window applies and then the slot is reclaimed.
merged_repo "$tmp/wt-killed"
(cd "$tmp/wt-killed" && "$slot" --base main true) 2>/dev/null
printf "name='wt-killed'\nworktree='%s'\nbranch='lane'\nbase='main'\nin_use='1'\npid='999999'\nlast_used='1'\n" \
    "$tmp/wt-killed" >"$root/kontra-slot-wt-killed.slot"
attempt_in "$repo_root" "$slot" --gc
contains "a flag whose build died is reclaimable" "$out" "remove wt-killed"

# --- 9. the integration worktree's own slot is never reclaimed --------------
mkrepo "$tmp/wt-integration"
git -C "$tmp/wt-integration" checkout -q -b feat/integration
(cd "$tmp/wt-integration" && "$slot" true) 2>/dev/null
out=$(cd "$repo_root" && "$slot" --gc 2>&1)
contains "--gc keeps the integration worktree's slot" "$out" "branch is the base"
[ -d "$root/kontra-slot-wt-integration" ] && ok "integration slot directory kept" || bad "integration slot directory kept"

# --- 10. --gc honours KONTRA_SLOT_ROOT: the real root is untouched ----------
out=$(cd "$repo_root" && "$slot" --list 2>&1)
contains "--list reads the same root" "$out" "$root"

# --- 11. a failing --gc is reported, not swallowed by `set -e` --------------
# A slot directory that is a symlink is a --gc failure by design: remove_slot
# refuses to `rm -rf` a path it did not create, and exits 2. The step's status
# has to be read. As a bare `out=$(...); rc=$?` under `set -e`, a non-zero exit
# killed this script *before* the assertion, so the failure arrived as a
# truncated run with no summary -- and the assertion, when reached at all, could
# only ever read 0.
mkdir -p "$tmp/elsewhere"
printf "name='poison'\nworktree='%s'\nbranch='lane'\nbase='main'\nin_use='0'\npid=''\nlast_used='1'\n" \
    "$tmp/gone" >"$root/kontra-slot-poison.slot"
ln -sfn "$tmp/elsewhere" "$root/kontra-slot-poison"
attempt_in "$repo_root" "$slot" --gc
check "a failing --gc is reported, not fatal to the test" "$rc" "2"
contains "the failure is the one that was planted" "$out" "refusing to remove"
[ -L "$root/kontra-slot-poison" ] && ok "the refused slot was left alone" || bad "the refused slot was left alone"
rm -f "$root/kontra-slot-poison" "$root/kontra-slot-poison.slot"

printf '\n%d passed, %d failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
