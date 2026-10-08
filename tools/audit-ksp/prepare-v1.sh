#!/usr/bin/env bash
# Build an unchanged v1 engine with a small compiler-observation adapter.
set -eu
repo=$(git rev-parse --show-toplevel)
audit_cache="$HOME/.cache/kontakto-audit-ksp"
mkdir -p "$audit_cache/v1"
git archive 0cb7a8a0 | tar -x -C "$audit_cache/v1"
cat >> "$audit_cache/v1/src/ksp/mod.rs" <<'RUST'
/// Audit-only access to the unchanged compiler.
pub fn audit_compile(source: &str, groups: usize) -> Option<(usize, Vec<String>)> {
    compile::compile(source, &compile::Setup { groups, outputs: 8, zones: 0 })
        .ok().map(|p| (p.errors.len(), p.diagnostics.into_iter().collect()))
}
RUST
cp "$repo/tools/audit-ksp/v1_bridge.rs" "$audit_cache/v1/examples/ksp_audit_bridge.rs"
cd "$audit_cache/v1"
"$HOME/.cache/kontakto-heavy" cargo build --profile ci --locked --no-default-features --example ksp_audit_bridge
