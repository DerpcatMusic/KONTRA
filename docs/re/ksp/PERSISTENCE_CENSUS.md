# Persistence census: make_persistent, read_persistent_var, on persistence_changed

Scope: 11 real instruments, extracted with `sampler-kontakt` `dump_scripts` (one instrument at a
time, no render). Not a corpus census: the library-wide survey is barred by the load rules, so the
library-wide count is still open. Counts are per distinct script (a `.nki` can hold several
script slots). Regex hits in the script source, not parsed calls.

| Library | Instrument (script slot) | make_persistent | make_instr_persistent | read_persistent_var | on persistence_changed |
|---|---|---:|---:|---:|---:|
| Conflux 1.1.0 | Conflux (slot A) | 189 | 16 | 0 | 1 |
| Conflux 1.1.0 | Conflux (slot B) | 34 | 0 | 0 | 1 |
| Conflux 1.1.0 | Conflux (slot C) | 0 | 0 | 0 | 0 |
| Afflatus Chapter II Brass | Barbarian Brass | 62 | 0 | 62 | 0 |
| Morphology Evolved | Morphology Evolved | 479 | 0 | 1 | 1 |
| Audio Imperia Dolce | Dolce 01 7 1st Violins Legato | 575 | 0 | 244 | 1 |
| Areia 1.2.0 | Areia 16 Violins Core Techniques | 667 | 0 | 268 | 1 |
| Pacific Ensemble Strings | 10 Cellos Legato Sustains | 42 | 0 | 42 | 0 |
| Una Corda Library | Una Corda Felt (slot A) | 79 | 9 | 43 | 1 |
| Una Corda Library | Una Corda Felt (slots B-D) | 0 | 0 | 0 | 0 |
| Performance Samples Vista | Vista 5 Violins | 35 | 0 | 35 | 0 |
| Shreddage 3.5 E-Upright | E-Upright (slot A) | 25 | 42 | 0 | 1 |
| Shreddage 3.5 E-Upright | E-Upright (slot B) | 11 | 21 | 0 | 1 |
| ANALOG STRINGS | ANALOG STRINGS (slot A) | 468 | 48 | 446 | 1 |
| ANALOG STRINGS | ANALOG STRINGS (slots B, C) | 46 | 0 | 19 | 0 |
| Audio Imperia CHORUS | Chorus Women Traditional Articulations | 466 | 0 | 174 | 1 |

UVI: no `.nki` in the installed UVI folders; no UVI instrument is covered yet.

## Patterns

- Every instrument that uses `make_persistent` also uses `read_persistent_var` (Afflatus, Pacific,
  Vista: all of them) or `on persistence_changed` (Conflux, Morphology, Dolce, Areia, Una Corda,
  Shreddage, ANALOG STRINGS, Chorus). Pacific, Vista and Afflatus never use `on persistence_changed`,
  so their restore relies on `read_persistent_var` in `on init`.
- `make_instr_persistent` appears in Conflux, Una Corda, Shreddage and ANALOG STRINGS. It is saved
  with the instrument and not by snapshots.
- `on persistence_changed` is one callback per script slot. Several slots in one instrument can each
  have one.

## Check against the interpreter docs (docs/architecture-v2/KSP_SEMANTICS.md, section 1.2)

| Pattern in corpus | Documented behaviour | Status |
|---|---|---|
| `make_persistent` in init (all instruments) | marks the variable, saved with instrument | matches (static marking) |
| `make_persistent` only inside an untaken `if` | the doc said marking happens only if the call executes | **native: Kontakt saved `$z = 4`, so it marks statically, as our code does.** The doc row was wrong (`persistence_native_golden.rs`, case `untaken_mp`) |
| `make_instr_persistent` (Conflux, Una Corda, Shreddage, ANALOG) | saved with instrument, not snapshots | mismatch: `snapshot.rs` overwrites any named entry regardless of kind. Snapshot-side only |
| `read_persistent_var` inside init (Afflatus, Pacific, Vista, Dolce, Areia, ANALOG, Chorus) | restores immediately, only in init | native: restores `$p` at the read (B=7). Our code also restores non-persistent variables; native not probed (`persistence_native_golden.rs`, `int_pers_readvar`) |
| restore at end of init, then `on persistence_changed` (Conflux, Morphology, Dolce, Areia, Una Corda, Shreddage, ANALOG, Chorus) | values restored after init | native: no value is restored before the end of init (C=0 for `int_pers_noread`, `real_pers`, `array_pers`). Verified for init. **`on persistence_changed` did not run on a Lua `load_instrument`** (its marker group stayed clear), so the order against it is not verified; needs a GUI preset load |
| arrays restored as prefix, cut to declared length (`%` arrays in Dolce, Areia, ANALOG, Chorus) | not documented | unknown: native check `array_pers` covers the same length only; shorter/longer saves not probed yet |

Native values: `tools/ksp-conformance/probes/persistence_roundtrip.lua`, decoded by
`tools/ksp-conformance/persistence_decode.py` into `tools/ksp-conformance/persistence_golden.json`, and
compared by `crates/sampler-ksp/tests/persistence_native_golden.rs`. The multi-slot probe is still to run.
