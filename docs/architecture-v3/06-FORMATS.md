# 06 Formats: thin translators

A format crate reads a vendor's files and emits `kontra-ir` plus law ids.
It has no DSP, no stream service, no script runtime and no engine
dependency (01-LAYOUT.md DAG: `format-* → format, ir, params, law` only).
Today `sampler-kontakt` has its own `effects/` DSP (~6.5k), the stream
service every format uses (`stream.rs` 5.2k) and head preparation, and
`sampler-uvi` has its own script thread and stream. Those all move out.

## 1. The contract

```rust
pub trait Format: Send + Sync + 'static {
    const ID: FormatId;
    fn sniff(&self, path: &Path, head: &[u8]) -> Confidence;
    fn scan(&self, root: &Path, out: &mut dyn ScanSink) -> Result<()>;          // library browser entries
    fn load(&self, entry: &EntryRef, cx: &mut LoadCx) -> Result<Instrument>;     // → kontra-ir
    fn source(&self, sample: &SampleRef) -> Result<Box<dyn Source>>;            // reader for kontra-stream
}
pub struct LoadCx<'a> { pub diag: &'a mut Diagnostics, pub assets: &'a dyn Assets, pub options: &'a LoadOptions }
```

- `kontra-format` holds the registry (`formats! { kontakt::Kontakt, uvi::Uvi, sfz::Sfz, native::Native, ample::Ample, ikmpak::Ikm }`),
  shared readers (WAV/FLAC/AIFF, decrypted container streams) and `Diagnostics`.
- `Instrument` is the IR: Articulation → Variation → Group → Zone,
  modulation routes, bus/insert chains as `ModuleSpec { kind, law, params }`,
  scripts as source plus dialect, UI as `ui-model` trees, and preload hints
  (start ranges).
- **Everything unsupported is a diagnostic** with an impact rating. Nothing
  is silently dropped, and the per-library gap log is built from these
  diagnostics (memory: signal trace + gap log).

## 2. Mapping effects: data, not code

Today each vendor effect is hand-mapped in `kontakt/effects/*` and
`uvi/fx/*`, then mapped again from IR to core in `lower.rs`. In v3 it's
one table per format:

```rust
kontra_macros::map_effects! { Kontakt;
    // vendor id           module                law                             params: vendor → catalog (+ law per param)
    0x2D "Solid Bus Comp"  => dynamics::Compressor, laws::KONTAKT_SOLID_BUS_COMP, { thresh => Threshold, ratio => Ratio, attack => Attack, release => Release, makeup => Makeup, mix => Mix };
    0x0B "Skreamer"        => drive::Shaper,       laws::KONTAKT_SKREAMER,       { drive => Drive, tone => Tone, bright => Bright, mix => Mix };
    0x20 "Replika"         => delay::DelayLine,    laws::KONTAKT_REPLIKA,        { time => Time, feedback => Feedback, ... };
    // ...
}
```

This generates `fn effect(id, raw: &RawParams, diag) -> Option<ModuleSpec>`
and the reverse table that KSP `$ENGINE_PAR_*` and the UI use. One line per
vendor effect. Adding an effect is that one line, plus a law file if the
taper is new.

The same macro maps modulators (`map_mods!`: LFO, envelopes, step,
follower) and source/zone fields (`map_zone!`: tune, pan, volume, S.Mod,
loop modes).

## 3. Size targets

| Crate | Today (production) | Target |
|---|---|---|
| `format/kontakt` | 23k (incl. 5.2k stream, ~6.5k effects DSP) | file readers (unchanged, mostly vendor/) + ~8k mapping |
| `format/uvi` | 25.6k (incl. 5.3k script, 1.8k thread, stream) | readers + ~6k mapping |
| `format/sfz` | (inside root) | ~1.5k, both directions (export for debugging) |
| `format/native` | `sampler-native` 3.2k | ~1.5k, serde of IR + sample pack |

## 4. Native format

The `native` format is the IR serialized as versioned, documented
MessagePack, plus a sample pack. It is what `create-library` writes,
what tests load (no vendor files needed in CI), and the export target for
debugging a translation (`kontra-cli inspect --to native`).
