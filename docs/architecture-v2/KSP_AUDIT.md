# KSP behavioral audit — checkpoint

Audit base: `699758ab` / branch `v2/gpt-ksp-audit`, 2026-10-08. Audit-only changes; runtime code is unchanged. This is an unfinished checkpoint requested by the coordinator after a server restart. Further ranked findings follow in subsequent commits.

## 1. Most engine-parameter writes update a script-local mirror without changing the engine

**Severity: critical. Corpus exposure: 2,741 / 2,741 indexed instruments**, including 2,741 with unapplied calls reachable from `on ui_control`, 377 from `on persistence_changed`, and 374 from `on note`. These are static exposure counts, not measurements that every instrument audibly fails. The census covers 52 unique cached scripts and counts each associated instrument once per finding.

**Manual:** [set_engine_par / get_engine_par](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameter-commands#set_engine_par--) specify engine mutation and engine readback with group, module slot and generic/bus addressing. Parameters must retain the same behavior when their identifiers or address components are held in variables. Effect type/subtype changes also have asynchronous completion requirements.

**Code evidence:** `crates/sampler-ksp/src/lower.rs:1916` returns zero for an absent local store key; `:1921` writes only the script-local four-component store and emits an effect. `:2146` recognizes volume/pan/tune only when the parameter and the slot/generic values are compile-time constants; `:2161` recognizes five envelope parameters; `:2176` recognizes five effect-slot controls. All other parameters, and dynamic versions of the specialized ones, go through the fallback. `crates/sampler-core/src/ops.rs:540` only drains the effect queue. The production consumer at `src/sound/v2.rs:759` routes effects to script views, `src/sound/mod.rs:232` calls `apply_ui_effect`, and `crates/sampler-ksp/src/lib.rs:273` applies keyboard/UI properties, with no engine-parameter handler. Thus a setter/getter round trip can pass while the DSP never changes. The compile-time evaluator has the same mirror at `crates/sampler-ksp/src/eval.rs:1202` and `:1214`.

**Failing reproduction:** `crates/sampler-ksp/tests/audit.rs:344`, `dynamic_engine_parameter_write_changes_audio`. A note callback calls `set_engine_par($p, 0, 0, -1, -1)` where `$p` holds `$ENGINE_PAR_VOLUME`; the constant-sample fixture remains audible instead of becoming silent. The prior audit run failed this probe. Run `/home/derpcat/.cache/kontakto-heavy cargo test -p sampler-ksp --test audit dynamic_engine_parameter_write_changes_audio -- --ignored`.

**Important boundary:** literal volume/pan/tune, five volume-envelope controls and five effect-slot controls already have specialized runtime paths (`lower.rs:1833`, `:1856`, `:1893`). The Kontakt loader also applies a subset of captured init writes to effect racks (`crates/sampler-kontakt/src/library.rs:249`, `crates/sampler-kontakt/src/effects.rs:171`). This finding is about the general dispatch gap, not a claim that those paths never work.

**Required fix:** resolve the parameter and address at runtime against engine/module state; use the same engine-backed readback for all scripts. Cover dynamic IDs, variables for slot/generic, bus addresses and cross-script reads. A mirror-only round trip is insufficient validation; assert engine state or audio changes.

## Evidence retained

- Opt-in contract probes: `crates/sampler-ksp/tests/audit.rs`; prior run: 37 failures and 3 passes across 40 probes.
- Full identifier inventory: [KSP_AUDIT_ENGINE_PAR.tsv](KSP_AUDIT_ENGINE_PAR.tsv), 676 unique ENGINE_PAR identifiers from the required Kontakt identifier catalog, with corpus exposure, dispatch status, implementation value law and manual citation for each.
- Census and test logs remain in `/home/derpcat/.cache/kontakto-gpt-ksp-audit/`. Library scripts, samples and decrypted data are not committed.
