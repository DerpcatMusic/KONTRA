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
| `make_persistent` only inside an untaken `if` | marking happens only if the call executes | mismatch in our code (static). Native check in `persistence_roundtrip.lua` variant `untaken_mp` |
| `make_instr_persistent` (Conflux, Una Corda, Shreddage, ANALOG) | saved with instrument, not snapshots | mismatch: `snapshot.rs` overwrites any named entry regardless of kind. Snapshot-side only |
| `read_persistent_var` inside init (Afflatus, Pacific, Vista, Dolce, Areia, ANALOG, Chorus) | restores immediately, only in init | partial: no check that the variable is persistent. Native check: variant `int_pers_readvar` |
| restore at end of init, then `on persistence_changed` (Conflux, Morphology, Dolce, Areia, Una Corda, Shreddage, ANALOG, Chorus) | values restored after init, before `on persistence_changed` | matches the doc. Native check: variants `int_pers_noread`, `real_pers`, `array_pers` |
| arrays restored as prefix, cut to declared length (`%` arrays in Dolce, Areia, ANALOG, Chorus) | not documented | unknown: native check `array_pers` covers the same length only; shorter/longer saves not probed yet |

Native values for the rows marked "native check" come from `tools/ksp-conformance/probes/persistence_roundtrip.lua`
and `multi_slot_roundtrip.lua`, decoded by `tools/ksp-conformance/persistence_decode.py`. They are
pending (the probe is queued behind Kontakt sessions).
