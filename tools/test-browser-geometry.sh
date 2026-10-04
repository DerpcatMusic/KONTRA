#!/usr/bin/env bash
set -euo pipefail

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repo_root"
test_binary=$(mktemp "${TMPDIR:-/tmp}/kontra-browser-geometry.XXXXXX")
trap 'rm -f "$test_binary"' EXIT

# Compile the actual geometry module without building the sampler/GPU stack.
rustc --edition 2024 --test -o "$test_binary" - <<'RS'
#[path = "src/ui/browser_geometry.rs"]
mod geometry;
RS
"$test_binary" "$@"
