# 11 Migration: PR by PR, always shippable

This is a strangler migration, not a rewrite branch. Every step is one PR
that keeps `main` building, playing and passing CI. Moves use `git mv` and
leave a re-export behind (`pub use kontra_x::*;` in the old place) until the
last user is gone, so history and blame survive. A step is done when its
gate passes. A deletion step is done when the code is gone.

Playback fixes come first (ONE_ENGINE.md, FUTURE_SAMPLER_PLAN.md): users
hear phases 1–3. The structural work after them is easier and safer once
the contracts exist.

Step numbers in the tables below are the original outline. The task ids in
[tasks.toml](tasks.toml) / [TASKS.md](TASKS.md) are authoritative (for example
"1.3 one start batch" is task P1.4).

## Phase 0: guard rails (1–2 PRs)

| PR | Does | Gate |
|---|---|---|
| 0.1 | `tools/xtask` with `size`, `debt`, `deps-check` (current DAG as allow-list), `timings` | numbers match 00-BASELINE.md |
| 0.2 | `kontra-probe` crate from the v2.rs ignored probe; contracts C1–C10 as code, `#[ignore]` where failing today | runs on Areia/Vista locally, reports today's failures |
| 0.3 | `[profile.fast]`, CI matrix by crate | CI green, `fast` rebuild timed |

## Phase 1–3: playback (users hear it)

| PR | Does | Gate |
|---|---|---|
| 1.1 | Preload law: base + start range (S.Mod, script offsets, CC/RTC → playPos); no head shrinking below law | C9 on Areia/Vista |
| 1.2 | Remove cold/hold: late data = silence + `dropout_frames`, cursor advances | C1 |
| 1.3 | One start batch per callback instant (replace per-family onset cohorts) | C3, C6 (Areia "double attack, mono then wide") |
| 2.1 | UI writes → `LatestCell` control store; delete effect outbox + `BehaviorOverload::Abort` release | C7, C8 (Areia dense 64/64) |
| 2.2 | Delete fuel; KSP `while` limit only | C7 |
| 3.1 | Lua inline on audio (arena allocator), delete `ScriptThread` + offline host | C5 on UVI fixture |
| 3.2 | Auto-align as latency compensation (slowest patch reported, the rest held to it, live and on playback) | layered patches start together; C1 with transport on |

After phase 3, the original complaints are covered by green contracts:
double attacks (C3, C6), DAW lag (C1, C5), Areia width (C3) and Vista
position (C9).

## Phase 4: base and model crates (mechanical)

| PR | Does |
|---|---|
| 4.1 | Create `crates/{base,model,dsp,engine,script,format,app}/` folders; `git mv` `sampler-pool` → `base/pool`, `sampler-simd` → `base/simd`; rename packages, re-export old names |
| 4.2 | `kontra-time`, `kontra-law` (extract from core/ir), `kontra-macros` with `Stats`, `Config` derives |
| 4.3 | `kontra-params` catalog: all module params with `#[derive(Params)]`; KSP `$ENGINE_PAR_*` table and UVI parameter ids point at it; delete 4 of 5 registries |
| 4.4 | `kontra-events` (from `src/sound/event.rs` and the plain-data receiver wire types; the `sampler-midi` adapter moves to `kontra-engine` in P6.4c and held keys become the `NoteLedger` in P6.4b), `kontra-ui-model` (from `sampler-ui-ir` + core widget/script_params), `kontra-protocol` |

Gate: `xtask debt` shows registries 5 → 1 and `#[path]` count falling. All contracts unchanged.

## Phase 5: DSP families (one PR per family, parallelizable)

For each family, in the order dynamics → eq → filter → delay → reverb → drive → spatial → modulators:

1. Create `crates/dsp/<family>` with `dsp-core` (first PR also creates `dsp-core` and the `modules!` registry with the old `Processor` enum wrapping `AnyModule` as one variant).
2. Port the family's classes (03-DSP.md §3 table). Vendor variants become laws, and the existing vectors move into `vectors/`.
3. Point `lower.rs` and `dsp.rs` at the registry for that family. Delete the old files.
4. Gate: the family's vectors pass at stated tolerance, and C4 plus A/B
   renders on the reference libraries null below the family's tolerance.
   Every second class that survives has its measurement recorded.

Once the last family is done, delete both `Processor` enums, `lower.rs`
mapping and the routing match in `dsp.rs`. Gate: `xtask debt` shows
`Processor::` at 0.

## Phase 6: engine crates

| PR | Does |
|---|---|
| 6.1 | `kontra-stream` from core stream/residency/head + kontakt `stream.rs`/head_* + uvi stream; `Source` trait; content-keyed store; drop uvi → kontakt dependency |
| 6.2 | `kontra-voice` from `source*`, resample, release, pitch; one chain runner (delete ~54 `render*` variants) |
| 6.3 | `kontra-vm` from behavior/ops/schedule; `HostApi` trait; `on init` on the VM; delete `eval.rs`, `luau/` |
| 6.4 | `kontra-engine`: `Engine` struct (02-ENGINE.md), `EventQueue`, `NoteLedger`; `Runtime` 109 fields → the 10 owners; plugin shell stops splitting blocks |

Gate: all contracts green, CPU (C10) no worse than before 6.1, and the
`fast` rebuild after a DSP edit ≤ 25 s.

## Phase 7: script and format crates

| PR | Does |
|---|---|
| 7.1 | `kontra-ksp` (sampler-ksp minus deleted parts), `builtins!` drives lowering |
| 7.2 | `kontra-lua` (UVI script surface from sampler-uvi/sampler-script) |
| 7.3 | `kontra-format` + `formats!`; `format/kontakt` with `map_effects!` tables; delete `kontakt/effects` DSP |
| 7.4 | `format/uvi` same; `format/sfz`, `format/native` |

Gate: library surveys (memory: periodic library survey) show no new
regressions, and every unsupported feature appears as a diagnostic.

## Phase 8: app and plugin

v3 replaces v2 rather than moving it (fresh-engine decision, 2026-10-27).
The `sampler-*` crates, `src/sound`, `src/ui` and v2's `Shared` are not
moved into the new crates; they keep shipping as the root `kontakto`
package until the release gate, then they are deleted.

| PR | Does |
|---|---|
| 8.1 | `kontra-protocol`: commands in, publications and retirements out (07-APP.md §2) |
| 8.2 | `kontra-host` rack: N parts over `kontra-engine`, loads off the audio thread, MIDI channel routing, outputs, master, panic |
| 8.3 | `app/library` and `app/align` written for v3 (browser data, presets, favourites, latency compensation) |
| 8.4 | `app/ui` depends on protocol only; `xtask deps-check` enforces no engine edge |
| 8.5 | `plugin/` (`kontra-plugin`, bundle "KONTRA v3"): moose shell over host + ui, parameters, latency, editor, state recall tested headless on real instruments; ships beside v2 |
| 8.6 | Release gate; then v2 is deleted and `plugin/` becomes the KONTRA bundle |

Gate: v1 UX parity checks (memory: v1 UX parity), every README target, and
`xtask deps-check` with the final DAG.

## Rules during migration

- One concept per PR. Moves and behaviour changes go in separate PRs, so a
  move is reviewable as a pure rename.
- Never two implementations live at once for more than one phase. The old
  one is deleted in the PR that switches the last caller.
- Each phase ends with one coordinator build plus a survey (memory: merge
  then test once).
- Update this folder in the same PR when reality differs from the plan.
