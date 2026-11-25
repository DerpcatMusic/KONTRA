#!/bin/sh
# Kontakt translator size, the 06-FORMATS.md §3 measure: production lines
# (each file up to its `mod tests`, the cfg(test) rigs audit.rs and fit.rs
# left out), split into file readers and mapping.
# Usage: tools/migrations/kontakt-sizes.sh
cd "$(cd "$(dirname "$0")/../../crates/format" && pwd)" || exit 1
lines() {
  for f in "$@"; do awk '/^mod tests/{exit} {n++} END{print n+0}' "$f"; done | awk '{s+=$1} END{print s}'
}
k=kontakt/src
readers=$(lines $k/access.rs $k/container.rs $k/resources.rs $k/samples.rs $k/snapshot.rs $k/multi.rs $k/lib.rs $k/provenance.rs)
all=$(lines $(ls $k/*.rs $k/laws/*.rs | grep -v -e /audit.rs -e /fit.rs))
effects=$(lines kontakt-effects/src/*.rs)
echo "readers $readers, mapping $((all - readers)) + kontakt-effects $effects = $((all - readers + effects))"
