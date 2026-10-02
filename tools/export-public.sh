#!/usr/bin/env bash
# Export a clean tree of this repository with a fresh single-commit history.
#
#   tools/export-public.sh [--private|--public] [--ref REF] [--out DIR]
#                         [--dry-run] [--verify]
#
#   --private   (default) the full code, including src/access.rs and the
#               `library-access` feature, for the private repository.
#   --public    retains all source, tools, features, notices and provenance;
#               omits agent/workstation settings, local corpus reports and
#               generated artifacts, then sanitizes workstation paths.
#   --ref REF   what to export (default HEAD). Only tracked files are
#               exported (git archive), so ignored files such as
#               /artifacts never leak.
#   --out DIR   output directory (default ../kontra-private or
#               ../kontra-public next to the repository). Must not exist.
#   --dry-run   export and check the public manifest, but create no commit.
#   --verify    opt in to `cargo test --release` (default: no tests).
#   --no-verify skip tests, even if --verify was also passed.
#
# Both modes drop agent tooling (.claude/, .impeccable/, .mcp.json, .graft,
# graft/). Public mode also omits PRODUCT.md, private corpus/workstation reports
# in audits/, and artifacts/. It keeps DESIGN.md, public diagnostics/research,
# the access feature, all code and all tracked legal notices.
# Both modes scrub local paths from Markdown and run a leak scan for
# library/sample files, KSP source, key-like hex strings and personal data.
# Verification builds with $CARGO_TARGET_DIR if set (share a target to save
# disk), else in DIR/target, which is deleted afterwards.
set -euo pipefail

mode=private ref=HEAD out= verify=0 dry_run=0
while [ $# -gt 0 ]; do
  case $1 in
    --private) mode=private ;;
    --public) mode=public ;;
    --ref) [ $# -ge 2 ] || { echo "--ref needs a value" >&2; exit 2; }; ref=$2; shift ;;
    --out) [ $# -ge 2 ] || { echo "--out needs a value" >&2; exit 2; }; out=$2; shift ;;
    --dry-run) dry_run=1 ;;
    --verify) verify=1 ;;
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

# --- Both modes: agent tooling and documentation paths -------------------------
rm -rf .claude .impeccable .mcp.json .graft graft
# Keep docs useful while removing this workstation's literal paths.
find . -name '*.md' -type f -print0 | xargs -0 sed -E -i \
  -e 's#/mnt/[^/[:space:]]+/DEV_PROJECTS/Repos/KONTAKTO#/path/to/repository#g' \
  -e 's#/mnt/[^/[:space:]]+/Libraries/Kontakt#/path/to/Kontakt-Libraries#g' \
  -e 's#/mnt/[^/[:space:]]+/Libraries#/path/to/Libraries#g' \
  -e 's#/mnt/[^/[:space:]]+#/path/to/storage#g' \
  -e 's#/home/[^/[:space:]]+#${HOME}#g'

# --- Public mode: preserve implementation and legal provenance ----------------
if [ $mode = public ]; then
  rm -rf PRODUCT.md artifacts
  rm -f audits/CORRELATIONS.md audits/EFFECTS.md audits/HOST-SERVICES.md \
        audits/LATENCY.md audits/LIBRARIES.md audits/MODULATION.md audits/NTFS.md \
        audits/PERF.md audits/README.md audits/RECOVERY.md \
        audits/REPLACEMENT_REVIEW.md audits/RESOURCES.md audits/WORKBENCH.md
  # These paths are per-machine defaults; keep the tools and source logic,
  # replacing only the embedded data-root strings in the exported copy.
  while IFS= read -r -d '' f; do
    sed -E -i \
      -e 's#/mnt/[^/[:space:]]+/DEV_PROJECTS/Repos/KONTAKTO#/path/to/repository#g' \
      -e 's#/mnt/[^/[:space:]]+/Libraries/Kontakt#/path/to/Kontakt-Libraries#g' \
      -e 's#/mnt/[^/[:space:]]+/Libraries#/path/to/Libraries#g' \
      -e 's#/mnt/[^/[:space:]]+#/path/to/storage#g' "$f"
  done < <(grep -rlIZ --exclude=export-public.sh '/mnt/' . || true)
  # Public UVI example checksums are metadata, not keys. Keep their links and
  # private provenance; omit only these two reviewed strings from the export.
  if [ -f docs/FALCON_FORMAT_GROUNDWORK.md ]; then
    sed -i \
      -e 's/, SHA-256 `98f403a58b92a4e04364094c08d4b114edc93549cf3937f4dd5bb745973bcb2d`//g' \
      -e 's/, SHA-256 `b17f3895d0e78b4c16d6ab5691f39ac080a2b70a983705c228e1fe086478cf0e`//g' \
      docs/FALCON_FORMAT_GROUNDWORK.md
  fi
fi
# Strip marker comments only. The enclosed provenance text remains public.
find . -name '*.md' -type f -print0 |
  xargs -0 sed -i '/^<!-- private:\(start\|end\) -->$/d'

# Public exports must contain the access feature, complete code, and every
# tracked license/notice. This also catches a future destructive filter.
if [ $mode = public ]; then
  required=(LICENSE NOTICE THIRD_PARTY.md docs/LEGAL.md assets/OFL.txt \
            licenses/MOOSE/LICENSE licenses/MOOSE/LICENSE-APACHE licenses/MOOSE/LICENSE-MIT \
            licenses/MOOSE/NOTICE licenses/MUI/LICENSE about.toml tools/licenses.py \
            src/access.rs src/no_access.rs \
            vendor/ni-file/Cargo.toml vendor/ni-file/README.md vendor/ni-file/src/lib.rs \
            tools/export-public.sh)
  for f in "${required[@]}"; do
    [ -f "$f" ] || { echo "public manifest missing required file: $f" >&2; exit 1; }
  done
  grep -Eq '^[[:space:]]*library-access[[:space:]]*=' Cargo.toml ||
    { echo "public manifest lost the library-access feature" >&2; exit 1; }
  grep -Fq 'feature = "library-access"' src/lib.rs ||
    { echo "public manifest lost the access module selection" >&2; exit 1; }
  grep -Fq 'Ma5onic/ni-file' THIRD_PARTY.md &&
    grep -Fq 'No explicit redistribution license found' THIRD_PARTY.md &&
    grep -Fq 'The keystream in `src/access.rs`' THIRD_PARTY.md ||
    { echo "public manifest lost required third-party provenance" >&2; exit 1; }
  mapfile -t code_files < <(git -C "$repo" ls-tree -r --name-only "$ref" |
    grep -Ei '\.(rs|py|sh|swift|c|cc|cpp|h|hpp|js|ts|cjs|m|mm|java|kt|cs|go|rb|lua|pl|ps1|bat|cmd|sql)$|^\.github/scripts/macos_installer/postinstall$' |
    grep -v '^\.claude/' || true)
  for f in "${code_files[@]}"; do
    [ -f "$f" ] || { echo "public manifest omitted code file: $f" >&2; exit 1; }
  done
  mapfile -t legal_files < <(git -C "$repo" ls-tree -r --name-only "$ref" |
    grep -Ei '(^|/)(LICENSE|COPYING|NOTICE)([-._][^/]*)?$' || true)
  for f in "${legal_files[@]}"; do
    [ -f "$f" ] || { echo "public manifest omitted legal file: $f" >&2; exit 1; }
  done
  omitted=(.claude .impeccable .mcp.json .graft graft PRODUCT.md artifacts \
           audits/CORRELATIONS.md audits/EFFECTS.md audits/HOST-SERVICES.md \
           audits/LATENCY.md audits/LIBRARIES.md audits/MODULATION.md audits/NTFS.md \
           audits/PERF.md audits/README.md audits/RECOVERY.md \
           audits/REPLACEMENT_REVIEW.md audits/RESOURCES.md audits/WORKBENCH.md)
  for f in "${omitted[@]}"; do
    [ ! -e "$f" ] || { echo "public manifest retained private material: $f" >&2; exit 1; }
  done
  if grep -rlI --exclude=export-public.sh '/mnt/' . >/dev/null ||
     grep -rlI --include='*.md' -E '/home/[^/[:space:]]+' . >/dev/null; then
    echo "public manifest still contains a workstation path" >&2
    exit 1
  fi
  echo "Public manifest verified: ${#code_files[@]} source/tool files and ${#legal_files[@]} tracked legal files retained."
fi

# --- Leak scan -----------------------------------------------------------------
fail=0
# This script contains the scan patterns; its output is covered by the manifest.
scan() { grep -rI --exclude=export-public.sh "$@"; }
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

# KSP source: authored test scripts are expected in parser/runtime and plugin tests; a
# library script outside those fixtures has many callback/declaration lines.
# These budgets cover reviewed, synthetic regressions; review provenance before raising them.
ksp='^[[:space:]]*"?(declare (const |ui_|polyphonic |global |read |pers |[%$!@~?])|end on\b|on (init|note|release|ui_control|controller|persistence_changed|pgs_changed|listener)\b)'
while IFS=: read -r f n; do
  case $f in
    *.rs) case $f in
      ./src/ksp/tests.rs) limit=231 ;; # Adds 8 authored optional_note_off fixture lines after wait_async's 223.
      ./src/import.rs) limit=5 ;; # Authored wavetable control-constant fixture.
      ./src/engine/params.rs) limit=5 ;; # Authored live saved pitch-LFO callback/control fixture.
      ./src/articulate.rs) limit=31 ;;
      ./tests/playback.rs) limit=79 ;;
      ./src/plugin.rs) limit=99 ;; # Authored native-send, source-context, script-page and live IR-switch fixtures.
      ./src/ui/vector.rs) limit=15 ;; # Authored graph/fader projection fixture.
      ./src/ksp/vm.rs) limit=7 ;; # Authored shared revision-owner fixture.
      ./tests/*|*/tests.rs) limit=40 ;;
      *) limit=3 ;;
    esac ;;
    *) limit=0 ;;
  esac
  [ "$n" -gt $limit ] && leak "KSP-like source ($n lines) in $f"
done < <(scan -cEI "$ksp" . | grep -v ':0$' || true)

# Key-like 64-digit hex strings. Lockfile checksums and the codec's test
# data hashes and the reviewed official actionlint archive checksum are the
# known exceptions; the latter is a public download-integrity check, not a key.
while IFS= read -r f; do
  if [ "$f" = ./release-fixes.json ]; then
    # Reviewed compiled-executable digest in five authored validation notes,
    # not a key. Preserve the ledger; every other hex string still fails.
    sed 's/Frozen executable SHA d1080da925e7118b46f3f0d5fa7e752404b00b7aef52777bb1c7ee118efadfb5;//g' "$f" |
      grep -qEI '(^|[^0-9A-Fa-f])[0-9A-Fa-f]{64}([^0-9A-Fa-f]|$)' || continue
  fi
  leak "64-digit hex string in $f"
done < <(
  scan -lEI '(^|[^0-9A-Fa-f])[0-9A-Fa-f]{64}([^0-9A-Fa-f]|$)' . |
    grep -vE '^\./(vendor/ni-file/)?Cargo\.lock$|^\./vendor/ncw/WRITER_VALIDATION\.md$|^\./\.github/scripts/check_workflows\.sh$' || true)

# Personal data, passwords and local paths.
while IFS= read -r f; do leak "personal data or local path in $f"; done < <(
  scan -lEI 'djderpcat|9240' . || true)
[ $mode = public ] && while IFS= read -r f; do leak "workstation path in $f"; done < <(
  scan -lEI '/mnt/' . || true)
[ $fail = 0 ] || { echo "Leak scan failed; $out left for inspection." >&2; exit 1; }
echo "Leak scan clean."

if [ $dry_run = 1 ]; then
  echo "Dry run complete; no commit was created and nothing was published."
  exit 0
fi

# --- Fresh history -------------------------------------------------------------
# Branch `initial`, not main: local hooks refuse commits on main. Publish
# with `git push <remote> initial:main`.
git init -q -b initial
git add -A
commit_name=${EXPORT_COMMIT_NAME:-DerpcatMusic}
commit_email=${EXPORT_COMMIT_EMAIL:-derpcatmusic@users.noreply.github.com}
GIT_AUTHOR_NAME=$commit_name GIT_AUTHOR_EMAIL=$commit_email \
GIT_COMMITTER_NAME=$commit_name GIT_COMMITTER_EMAIL=$commit_email \
  git -c commit.gpgsign=false commit -q -m "KONTRA: initial commit"
echo "Committed $(git rev-parse --short HEAD) on branch initial in $out"
echo "Publish with: git -C $out push <remote> initial:main"

# --- Verify --------------------------------------------------------------------
if [ $verify = 1 ]; then
  own_target=0
  if [ -z "${CARGO_TARGET_DIR:-}" ]; then export CARGO_TARGET_DIR=$out/target; own_target=1; fi
  cargo test --release
  cargo test --release --features library-access
  [ $own_target = 1 ] && rm -rf "$out/target"
  git status --porcelain | grep -q . && { echo "Build changed tracked files:" >&2; git status --short >&2; exit 1; }
  echo "Exported tree builds and tests pass."
fi
