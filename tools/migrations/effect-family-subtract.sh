#!/usr/bin/env bash
# Phase 3 of 13-IR-VNEXT: delete the unread parallel effect taxonomy.
# Nothing outside its own unit test read `EffectFamily`, `EffectKind::family`
# or `EffectSpec`; the null report (#155) keeps every audible recurrence as a
# typed `EffectKind`, so the families stay documentation, not a second type.
# Re-runnable: a second run changes nothing. Run from plugins/kontra.
set -euo pipefail
cd "$(dirname "$0")/../.."

before=$( { git grep -c -E '\b(EffectFamily|EffectSpec)\b|\.family\(\)' -- crates || true; } | awk -F: '{s+=$2} END {print s+0}')
bunx @ast-grep/cli scan -r tools/migrations/effect-family-subtract.yml -U crates/model/ir/src 2>/dev/null | tail -1 || true

# ast-grep's expandStart takes the attributes but not the `///` lines above
# them; a doc run left with no item would attach to the next one, so drop
# every doc run that is now followed by a blank line, and squeeze blanks.
python3 - crates/model/ir/src/generator.rs <<'PY'
import re, sys
p = sys.argv[1]
s = open(p).read()
s = re.sub(r"(?m)^(?:[ \t]*///[^\n]*\n)+(?=[ \t]*\n)", "", s)
s = re.sub(r"\n{3,}", "\n\n", s).rstrip("\n") + "\n"
open(p, "w").write(s)
PY

# The generator module's header no longer describes a taxonomy it lacks.
sed -i '1,2c //! Unified voice generator descriptor (IR vNext Phase 3).' crates/model/ir/src/generator.rs
sed -i 's/^use crate::{Effect, EffectId, EffectKind, Wave, Wavetable};$/use crate::{Wave, Wavetable};/' crates/model/ir/src/generator.rs
# Prose that named the type now names the plan's families.
sed -i '1s/inside one `EffectFamily`/inside one family/' crates/engine/effects/tests/effect_family_null.rs

after=$( { git grep -c -E '\b(EffectFamily|EffectSpec)\b|\.family\(\)' -- crates || true; } | awk -F: '{s+=$2} END {print s+0}')
echo "effect-family-subtract: references in crates/ $before -> $after"
