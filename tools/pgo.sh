#!/bin/bash
# Profile-guided plugin build: train the CLI on real playback, then build the
# plugins with that profile (5-12% less CPU per block on the library benches).
#   tools/pgo.sh install            train, then cargo moose install --clap --vst3 --user
#   tools/pgo.sh build [moose args] train, then cargo moose build (e.g. --target ...)
# Training libraries: $KONTRA_PGO_LIBRARIES (a folder of Kontakt libraries).
set -euo pipefail
cd "$(dirname "$0")/.."
mode=${1:-install}; shift || true
cpu=${KONTRA_PGO_CPU:-x86-64-v3}
profdata=$(find "$(rustc --print sysroot)" -name llvm-profdata -type f | head -1)
[ -x "$profdata" ] || { echo "needs: rustup component add llvm-tools" >&2; exit 1; }
raw=$(mktemp -d)
# Its own target dir: the profile flags would otherwise rebuild everything the
# plain builds share.
export CARGO_TARGET_DIR=$PWD/target/pgo
merged=$CARGO_TARGET_DIR/merged.profdata
mkdir -p "$CARGO_TARGET_DIR"
# Training and the final build must share every codegen flag, or LLVM drops
# the profile for mismatched functions. cargo moose's own target-cpu flag is
# replaced by RUSTFLAGS, so it is passed here explicitly.
flags="-Ctarget-cpu=$cpu"
RUSTC_WRAPPER= RUSTFLAGS="$flags -Cprofile-generate=$raw" \
  cargo build --release --bin kontakto
bin=$CARGO_TARGET_DIR/release/kontakto
"$bin" bench 2000 24 4 >/dev/null
"$bin" bench 1000 16 1 --root >/dev/null
libs=${KONTRA_PGO_LIBRARIES:-}
if [ -d "$libs" ]; then
  # One instrument per library, 16 notes for 5 s each.
  for lib in "$libs"/*/; do
    nki=$(find "$lib" -iname '*.nki' -print -quit)
    [ -n "$nki" ] && "$bin" bench-host 5 16 "$nki" >/dev/null 2>&1 || true
  done
else
  echo "KONTRA_PGO_LIBRARIES unset: training on the synthetic bench only" >&2
fi
"$profdata" merge -o "$merged" "$raw"/*.profraw
rm -rf "$raw"
export RUSTC_WRAPPER= RUSTFLAGS="$flags -Cprofile-use=$merged"
case $mode in
  install) cargo moose install --clap --vst3 --user "$@" ;;
  build) cargo moose build --clap --vst3 "$@" ;;
  *) echo "usage: tools/pgo.sh install|build [cargo moose args]" >&2; exit 2 ;;
esac
