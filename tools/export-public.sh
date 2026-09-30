#!/usr/bin/env bash
# Export a clean tree of this repository with a fresh single-commit history.
#
#   tools/export-public.sh [--private|--public] [--ref REF] [--out DIR] [--no-verify]
#
#   --private   (default) the full code, including src/access.rs and the
#               `library-access` feature, for the private repository.
#   --public    additionally removes the access module and the feature,
#               internal notes (audits/, PRODUCT.md, DESIGN.md, tools/*.py)
#               and README/THIRD_PARTY sections between
#               <!-- private:start --> and <!-- private:end -->.
#   --ref REF   what to export (default HEAD). Only tracked files are
#               exported (git archive), so ignored files such as
#               /artifacts never leak.
#   --out DIR   output directory (default ../kontra-private or
#               ../kontra-public next to the repository). Must not exist.
#   --no-verify skip `cargo test --release` in the exported tree.
#
# Both modes drop agent tooling (.claude/, .impeccable/, .mcp.json, graft/)
# and scrub local paths from Markdown, then run a leak scan that fails on
# library/sample files, KSP source, key-like hex strings and personal data.
# Verification builds with $CARGO_TARGET_DIR if set (share a target to save
# disk), else in DIR/target, which is deleted afterwards.
set -euo pipefail

mode=private ref=HEAD out= verify=1
while [ $# -gt 0 ]; do
  case $1 in
    --private) mode=private ;;
    --public) mode=public ;;
    --ref) ref=$2; shift ;;
    --out) out=$2; shift ;;
    --no-verify) verify=0 ;;
    -h|--help) sed -n '2,25p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
  shift
done

repo=$(git rev-parse --show-toplevel)
out=${out:-$(dirname "$repo")/kontra-$mode}
[ -e "$out" ] && { echo "$out already exists; remove it or pass --out" >&2; exit 1; }
mkdir -p "$out"
out=$(cd "$out" && pwd)
echo "Exporting $ref ($mode) to $out"
git -C "$repo" archive --format=tar "$ref" | tar -x -C "$out"
cd "$out"

# --- Both modes: agent tooling and local paths --------------------------------
rm -rf .claude .impeccable .mcp.json .graft graft
# Markdown only; code keeps its own paths (the leak scan below checks them).
find . -name '*.md' -type f -print0 | xargs -0 sed -i \
  -e 's#/mnt/MAIN_STORAGE/Libraries/Kontakt#<library root>#g' \
  -e 's#/mnt/MAIN_STORAGE/Libraries#<libraries>#g' \
  -e 's#/mnt/MAIN_STORAGE#<storage>#g' \
  -e 's#/mnt/Windows11/DEV_PROJECTS/Repos/KONTAKTO#<repository>#g' \
  -e 's#/home/derpcat#~#g'

# --- Public mode: no library access, no internal notes ------------------------
if [ $mode = public ]; then
  rm -f src/access.rs
  mv src/no_access.rs src/access.rs
  sed -i '/^\/\/ Encrypted library content: the `library-access` feature/d;
          /^#\[cfg_attr(.*feature = "library-access".*path = /d' src/lib.rs
  sed -i '/^# Reads a library.s own access data/d; /^library-access = /d; /^aes = /d' Cargo.toml
  grep -q 'library-access\|path = "no_access.rs"' src/lib.rs Cargo.toml &&
    { echo "library-access still referenced in src/lib.rs or Cargo.toml" >&2; exit 1; }
  sed -i 's#^//! Stand-in for the `library-access` feature: this build#//! Library access: this build#' src/access.rs
  sed -i '/name: test (library-access)/,+1d; s/ --features library-access,standalone/ --features standalone/;
          s/ --features library-access$//' .github/workflows/ci.yml
  sed -i "s/^  FEATURES: library-access$/  FEATURES: ''/" .github/workflows/nightly.yml
  sed -i '/export-public.sh empties it/d' .github/workflows/nightly.yml
  grep -rq 'library-access' .github &&
    { echo "library-access still referenced in .github/workflows" >&2; exit 1; }
  find . -name '*.md' -type f -print0 |
    xargs -0 sed -i '/<!-- private:start -->/,/<!-- private:end -->/d'
  rm -rf audits PRODUCT.md DESIGN.md tools/*.py tools/export-public.sh
  # Drop Cargo.lock entries (aes and its dependencies) no longer used.
  cargo metadata --offline --format-version 1 >/dev/null
fi
# Markers left in either mode are just comments; drop the marker lines.
find . -name '*.md' -type f -print0 |
  xargs -0 sed -i '/^<!-- private:\(start\|end\) -->$/d'

# --- Leak scan -----------------------------------------------------------------
fail=0
# This script names what it looks for; it is not scanned (public mode drops it).
scan() { grep -r --exclude=export-public.sh "$@"; }
leak() { echo "LEAK: $*" >&2; fail=1; }

# Library, preset and sample files: only the MIT codec test data and assets/.
while IFS= read -r f; do leak "library/sample file $f"; done < <(
  find . -type f \( -iname '*.nki' -o -iname '*.nkx' -o -iname '*.nkm' -o -iname '*.nicnt' \
    -o -iname '*.nkr' -o -iname '*.nks' -o -iname '*.nkc' -o -iname '*.nkb' -o -iname '*.wav' \
    -o -iname '*.ncw' -o -iname '*.aif' -o -iname '*.aiff' -o -iname '*.flac' \) \
    ! -path './vendor/ncw/tests/data/*' ! -path './assets/*')
[ -e artifacts ] && leak "artifacts/ exported"
# Anything large outside assets/ is suspect (renders, captures, dumps).
while IFS= read -r f; do leak "file over 4 MiB: $f"; done < <(
  find . -type f -size +4M ! -path './assets/*')

# KSP source: synthetic test scripts are short; a library script has many
# callback/declaration lines. Non-Rust files may contain none.
ksp='^[[:space:]]*"?(declare (const |ui_|polyphonic |global |read |pers |[%$!@~?])|end on\b|on (init|note|release|ui_control|controller|persistence_changed|pgs_changed|listener)\b)'
while IFS=: read -r f n; do
  case $f in
    *.rs) case $f in ./tests/*|*/tests.rs) limit=40 ;; *) limit=3 ;; esac ;;
    *) limit=0 ;;
  esac
  [ "$n" -gt $limit ] && leak "KSP-like source ($n lines) in $f"
done < <(scan -cEI "$ksp" . | grep -v ':0$' || true)

# Key-like 64-digit hex strings. Lockfile checksums and the codec's test
# data hashes are the known exceptions.
while IFS= read -r f; do leak "64-digit hex string in $f"; done < <(
  scan -lEI '(^|[^0-9A-Fa-f])[0-9A-Fa-f]{64}([^0-9A-Fa-f]|$)' . |
    grep -vE '^\./(vendor/ni-file/)?Cargo\.lock$|^\./vendor/ncw/WRITER_VALIDATION\.md$' || true)

# Personal data, passwords and local paths.
while IFS= read -r f; do leak "personal data or local path in $f"; done < <(
  scan -lEI 'djderpcat|9240|/home/derpcat' . || true)
[ $mode = public ] && while IFS= read -r f; do leak "local /mnt path in docs: $f"; done < <(
  grep -rlE '/mnt/' --include='*.md' . || true)

if [ $mode = public ]; then
  while IFS= read -r f; do leak "decryption code in $f"; done < <(
    grep -rlEI --include='*.rs' --include='*.toml' 'JDX|<HU>|Aes256|0x608da0a2|^aes = ' . || true)
fi
[ $fail = 0 ] || { echo "Leak scan failed; $out left for inspection." >&2; exit 1; }
echo "Leak scan clean."

# --- Fresh history -------------------------------------------------------------
# Branch `initial`, not main: local hooks refuse commits on main. Publish
# with `git push <remote> initial:main`.
git init -q -b initial
git add -A
GIT_AUTHOR_NAME=DerpcatMusic GIT_AUTHOR_EMAIL=djderpcat@gmail.com \
GIT_COMMITTER_NAME=DerpcatMusic GIT_COMMITTER_EMAIL=djderpcat@gmail.com \
  git -c commit.gpgsign=false commit -q -m "KONTRA: initial commit"
echo "Committed $(git rev-parse --short HEAD) on branch initial in $out"
echo "Publish with: git -C $out push <remote> initial:main"

# --- Verify --------------------------------------------------------------------
if [ $verify = 1 ]; then
  own_target=0
  if [ -z "${CARGO_TARGET_DIR:-}" ]; then export CARGO_TARGET_DIR=$out/target; own_target=1; fi
  cargo test --release
  [ $mode = private ] && cargo test --release --features library-access
  [ $own_target = 1 ] && rm -rf "$out/target"
  git status --porcelain | grep -q . && { echo "Build changed tracked files:" >&2; git status --short >&2; exit 1; }
  echo "Exported tree builds and tests pass."
fi
