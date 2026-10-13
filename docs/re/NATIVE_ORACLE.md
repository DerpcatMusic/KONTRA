# Native oracle and format catalog (2026-10-13)

The library inspector used to compare KONTRA's parser with itself; every report said
"native equivalence: unknown". This note describes the two references that replace that:
Kontakt's own post-load state, and Kontakt's own readers.

## 1. Native oracle: what real Kontakt loaded

`tools/kontakt-oracle/dump.py OUT.jsonl A.nki ...` runs Kontakt 8.13.1 standalone under
Wine on a private Xvfb (:78) with `dump.lua`, using the official Kontakt Lua API
(Kontakt 8 API Reference, en 260924; needs `DeveloperFeatures=dw:1` in
`Portapotty/UserData/Settings.cfg`, enabled 2026-10-13, backup next to it). Per instrument
it records groups (name, volume, pan, tune, playback mode, start options, voice group),
every zone (key/velocity ranges and fades, root, tune, volume, pan, sample, start/end,
frames, 8 loops), voice groups, instrument options and script slots (name, bypass,
protection, length + FNV hash only; source is never stored). Startup dialogs are clicked
away; one session at a time.

`ORACLE_LUA=probes/X.lua dump.py OUT.jsonl` runs a probe instead: set known values through
Kontakt's API, save the instrument, and read the saved file with KONTRA.

`tools/kontakt-oracle/diff.py NATIVE.jsonl` runs `ir_json` (sampler-kontakt example; one
small crate, ~1-2 s per instrument, no plugin) on each input and compares field by field.

### Results (one instrument per library, plus Afflatus 4 Horns KS)

| Instrument | values compared | mismatches |
|---|---:|---:|
| Afflatus 2 Horns KS / 4 Horns KS | 316,574 / 317,014 | 0 / 0 |
| ANALOG STRINGS | 1,851,839 | 0 |
| Areia 16 Violins Core | 923,736 | 0 |
| Chorus Women Traditional | 472,009 | 0 |
| Dolce 7 1st Violins Legato | 567,510 | 0 |
| Morphology Evolved | 86,742 | 0 |
| Pacific 10 Cellos Cluster Risers | 1,964 | 0 |
| Vista 3 Violins FFF Overlay | 80,500 | 6 (Cycle Round Robin, see below) |
| Solo Violin | 502,151 | 0 |
| Una Corda Pure / Cotton | 79,709 / 79,621 | 0 / 0 |
| Conflux | — | not registered in this Kontakt (needs activation entry) |

The zone/group/sample mapping layer is correct. The gaps are in what the Lua API does not
expose: effects, modulation, scripts, DSP.

### Laws measured with probes

- **Volume dB is 6 dB per doubling**: stored gain = 2^(dB/6) (−6 dB = 0.5, −60 dB = 1/1024).
  `probes/volume_law.lua`, test `kontakt_volume_db_is_six_db_per_doubling`.
- **Start options** are stored exactly as `set_group_start_options` receives them; the getter
  reports round-robin position stored + 1. Open: whether stored position 0 (Vista) is the
  first step or never matches needs the native evaluator.
- `get_voice_group` is a 1-based index into `get_voice_groups`.

## 2. Format catalog: what Kontakt's readers read

- `tools/re/ser_catalog.py` reads every serializable class straight from the executable
  (vtable slot 1 = SerType, slot 2 = current version, slot 11 / slot 8 for FX = reader):
  **118 classes** → `docs/re/kontakt-catalog/ser_classes.tsv`.
- `tools/re/query.sh` (+ `ghidra/DecompileByString.java`) runs read-only headless Ghidra
  queries on the analysed project (`~/kontakto-engine-analysis-20261007/ghidra-project`):
  by string, pointer slot, RIP-relative LEA, or `@address`. All 104 readers decompile in 10 s
  (output kept in `~/.cache/kontra-re`, not in the repo).
- `tools/re/reader_schemas.py` turns each reader into a per-version field layout:
  **409 layouts** → `reader_schemas.tsv` (agrees with 14/15 hand-derived layouts).
- `tools/re/gen_fx_layouts.py` generates `vendor/ni-file/.../native_fx_layouts.rs`
  (71 fully typed effect layouts) that now drive `EffectParameters::read`.

### Fixed from the catalog

- BParExternalMod 0x105 (every Kontakt 8.13-saved instrument failed to load).
- GaloisReverb room type is a u32 enum: every Hall reverb was translated as a Room.
- SSL G-EQ v0x11's four trailing fields.

## 3. Ranked gaps the tools now show (12 representative instruments)

1. Modulation into effect/modulator parameters: 8,271 dropped routes (Analog Strings 6,065:
   formant, lo-fi, distortion, shaper, LFO phase; Conflux 2,190: LFO rate/amount by envelope
   and LFO). Modulator-intensity scaling is wired (ID25 law) but Conflux's assignments are
   all outside the proved envelope (route intensity saved at 0 and set live by script,
   negative-depth flags, shaped/inverted). Next: Kontakt's modulation-law compiler
   FUN_140873040 (15 KB; templates `$out := $in * {} * $intensity`, `$in + {} * $intensity`,
   `table($c, %s($signal))`, `fcinv_neg`, ...) gives every target's native law.
2. ~~Conflux: 90 of 92 groups are Time Machine 2~~ — corrected 2026-10-13: Conflux stores
   source mode 3 = DFD on 90 groups (Kontakt's enum-name table at exe offset 0x7555700:
   Sampler, Tone, TimeMachine, DirectFromDisk, BeatMachine, TimeMachine2, S1200, MP60,
   TimeMachinePro). No corpus library uses Time Machine yet; the engine exists
   (sampler-core source/time_machine.rs; TM1 native, TM2/Pro labelled approximations).
3. Modulators never decoded: DBD envelope 0x41, Glide 0x0B, envelope follower 0x0A, Arp 0x09,
   group dynamics 0x4A, zone level envelope 0x2D.
4. Random Bipolar external source law (550 in Conflux).
