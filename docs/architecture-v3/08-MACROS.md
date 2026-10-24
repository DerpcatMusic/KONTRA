# 08 Macros: declare once

Every concept is declared once, and macros generate the rest: enums,
dispatch, metadata, (de)serialization, UI descriptors and tests. Proc
macros live in `kontra-macros` (L0). The others are `macro_rules!` in the
crate that owns the concept. Rule: a macro must delete more code than it
adds, so expanded code stays readable with `cargo expand`, and errors
point at the declaring line (`syn::Error::new_spanned`).

## 1. Inventory

| Macro | Kind | Declares | Generates | Replaces today |
|---|---|---|---|---|
| `#[derive(Params)]` | derive | a module's parameter struct with `#[param(range, unit, law, default, smooth)]` attributes | `Id` enum, `set(id, v)`, defaults, metadata table (name, unit, range), serde, automation ids | hand param structs + 5 parameter registries + `engine_par_ids.tsv` |
| `modules!` | proc | the DSP module list (03-DSP.md §5) | `AnyModule` enum, dispatch, `build(spec)`, param routing, UI descriptors | 2 `Processor` enums, `lower.rs` mapping, 824 match sites |
| `module_tests!` | macro_rules | per-module | block-size independence, reset = fresh, silence in/out, no NaN/denormal at param extremes | per-module hand tests |
| `laws!` | proc | a `laws/` folder | `pub const` ids + `include_str!` loaders + validation test | hand law tables in Rust |
| `vectors!` | proc | a `vectors/*.json` glob | one `#[test]` per vector file with tolerance from the file | handwritten vector loops |
| `#[derive(Stats)]` | derive | a counters struct | `snapshot()`, `reset()`, `Display`, publication field list | ~25 counter fields in `Runtime`, 19 `AtomicU64` in `Shared` |
| `#[derive(Config)]` | derive | settings structs | defaults from attributes, serde with version, UI settings descriptors | 93 handwritten `impl Default` (most) |
| `builtins!` | macro_rules (exists, extend) | KSP/Lua builtins | sema signatures, lowering to `HostApi`, docs, unsupported list | the existing table + hand lowering |
| `map_effects!` / `map_mods!` / `map_zone!` | macro_rules | vendor → catalog rows (06-FORMATS.md §2) | translator functions + reverse tables | `kontakt/effects/*`, `uvi/fx/*` mapping code |
| `formats!` | macro_rules | format list | registry, sniff order | hand registry in `src/formats.rs` |
| `probe!` | macro_rules | an instrumentation point | nothing unless `probe` feature; else a typed record into the probe ring | 76 `#[cfg(test)]` hooks inside production functions |
| `errors!` or `thiserror` | `thiserror` derive | per-crate `Error` enum | `Display`, `From` | 25 hand error enums, `Display` impls |

`thiserror` is already in `Cargo.lock` transitively (no crate uses it directly yet). Add it as a direct dependency instead of writing a macro.

## 2. Examples

```rust
// crates/model/params/src/dynamics.rs
#[derive(Params)]
pub struct Compressor {
    #[param(range = -60.0..=0.0, unit = Db, default = -12.0, smooth = 10.ms)] pub threshold: f32,
    #[param(range = 1.0..=20.0, unit = Ratio, default = 4.0, law = Exp)]      pub ratio: f32,
    #[param(range = 0.1..=200.0, unit = Ms, default = 10.0, law = Exp)]       pub attack: f32,
    #[param(range = 5.0..=2000.0, unit = Ms, default = 100.0, law = Exp)]     pub release: f32,
    #[param(range = 0.0..=24.0, unit = Db, default = 0.0, smooth = 10.ms)]    pub makeup: f32,
    #[param(range = 0.0..=1.0, unit = Percent, default = 1.0, smooth = 10.ms)] pub mix: f32,
}
```

```rust
// crates/engine/engine/src/stats.rs
#[derive(Stats, Default)]
pub struct Stats {
    pub voices_started: u64, pub voices_stolen: u64, pub dropout_frames: u64,
    pub queue_refused: u64, #[stat(max)] pub peak_voices: u32, #[stat(max)] pub max_callback_ns: u64,
}
```

```rust
// crates/dsp/dynamics/tests/vectors.rs
kontra_macros::vectors!("vectors/*.json", |v| dynamics::Compressor::run_vector(v));
```

## 3. Expected savings (estimates, to be checked per migration step)

| Area | Lines today (approx.) | After |
|---|---|---|
| Processor matches + `lower.rs` + `dsp.rs` routing | ~9k | ~1k (registry + generated) |
| Parameter registries (5) | ~4k | catalog ~2.5k, generated rest |
| Vendor effect mapping (kontakt + uvi) | ~8k | ~1.5k table rows |
| Counters/Stats plumbing | ~2k | ~0.3k |
| Defaults/config | ~2k | attributes |
| Test hooks + per-module hand tests | ~6k | ~1k macros + vectors |
