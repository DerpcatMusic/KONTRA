#!/bin/bash
# Native PGO: train the CLI, then build the plugins with the same codegen flags.
#   tools/pgo.sh build [moose args]
#   tools/pgo.sh install            explicitly install after training
# Inputs: KONTRA_PGO_PRESETS (one preset path per line), or KONTRA_PGO_LIBRARIES.
# Results and failed-load diagnostics remain in the isolated target/training.log.
set -euo pipefail
cd "$(dirname "$0")/.."
mode=${1:-build}; shift || true
case $mode in build|install) ;; *) echo "usage: tools/pgo.sh build|install [cargo moose args]" >&2; exit 2 ;; esac
host=$(rustc -vV | sed -n 's/^host: //p')
requested=$host
args=("$@")
for ((n=0; n<${#args[@]}; n++)); do
  case ${args[n]} in
    --target) requested=${args[n+1]:-}; ((n+=1)) ;;
    --target=*) requested=${args[n]#--target=} ;;
  esac
done
if [ "$requested" != "$host" ]; then
  echo "PGO must train on the target platform ($host here, requested $requested). Use a normal cross build or run this script on the target." >&2
  exit 2
fi
case $host in x86_64-*) baseline=x86-64 ;; *) baseline=generic ;; esac
cpu=${KONTRA_PGO_CPU:-$baseline}
profdata="$(rustc --print sysroot)/lib/rustlib/$host/bin/llvm-profdata"
[ -x "$profdata" ] || { echo "needs: rustup component add llvm-tools" >&2; exit 1; }
raw=$(mktemp -d)
trap 'rm -rf "$raw"' EXIT
export CARGO_TARGET_DIR=${KONTRA_PGO_TARGET_DIR:-${CARGO_TARGET_DIR:-$PWD/artifacts}/pgo}
mkdir -p "$CARGO_TARGET_DIR"
CARGO_TARGET_DIR=$(cd "$CARGO_TARGET_DIR" && pwd)
export CARGO_TARGET_DIR
merged=$CARGO_TARGET_DIR/merged.profdata
log=$CARGO_TARGET_DIR/training.log
flags="${RUSTFLAGS:-} -Ctarget-cpu=$cpu"
printf 'Target: %s\nFlags: %s\n' "$host" "$flags" > "$log"
RUSTC_WRAPPER= RUSTFLAGS="$flags -Cprofile-generate=$raw" cargo build --release --bin kontakto
bin=$CARGO_TARGET_DIR/release/kontakto
[ -x "$bin" ] || bin=$bin.exe
"$bin" bench 2000 24 4 >> "$log" 2>&1
"$bin" bench 1000 16 1 --root >> "$log" 2>&1
presets=$raw/presets.txt
if [ -n "${KONTRA_PGO_PRESETS:-}" ]; then
  cp "$KONTRA_PGO_PRESETS" "$presets"
elif [ -d "${KONTRA_PGO_LIBRARIES:-}" ]; then
  # Stable selection rather than whichever directory entry find returns first.
  for lib in "$KONTRA_PGO_LIBRARIES"/*/; do
    [ -d "$lib" ] || continue
    find "$lib" -iname '*.nki' -type f | LC_ALL=C sort | sed -n '1p'
  done > "$presets"
else
  : > "$presets"
  echo "Synthetic training only: set KONTRA_PGO_PRESETS for a representative shipping workload." >&2
fi
count=0
while IFS= read -r nki || [ -n "$nki" ]; do
  [ -n "$nki" ] || continue
  count=$((count + 1))
  printf '\nPreset: %s\n' "$nki" >> "$log"
  for frames in 64 512; do
    if ! "$bin" bench-host 5 16 "--frames=$frames" "$nki" >> "$log" 2>&1; then
      echo "PGO training failed: $nki ($frames frames). See $log" >&2
      exit 1
    fi
  done
done < "$presets"
printf '\nReal presets trained: %s\n' "$count" >> "$log"
"$profdata" merge -o "$merged" "$raw"/*.profraw
export RUSTC_WRAPPER= RUSTFLAGS="$flags -Cprofile-use=$merged"
cargo build --release --lib --bin kontakto
case $mode in
  install) cargo moose install --clap --vst3 --user "$@" ;;
  build) cargo moose build --clap --vst3 "$@" ;;
esac
