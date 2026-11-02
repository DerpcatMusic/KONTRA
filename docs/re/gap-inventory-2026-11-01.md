# Gap inventory — 2026-11-01

The user asked whether it would be worth scanning every instrument and listing
what needs to be added. It is worth it, and this is the list. Nothing here
changes engine or UI behaviour: this is a survey, and every number was produced
by a command on this machine, quoted in §6 so it can be re-run rather than
believed.

**The headline is not the ranking, it is what the ranking is made of.** 1,929
instruments use a symbol the ledger marks unsupported or stub; 772 of those use
no such name other than two the engine already answers (`$MARK_2…28`,
`%NOTE_DURATION`), 741 use a name it genuinely lacks, and 416 use both. The
split is `symbols.json` with those two names removed from each instrument's
unsupported set — `$MARK_1` is `KSP-FULL` in the ledger and never was in it.
The real language gaps sit in the text scanner's blind spot — it counts only the
tokens it can find in its own table, so a command Kontakt has and we do not is
invisible to it. Compiling the extracted scripts instead of scanning them finds
them: 718 of the 3,674 script-bearing instruments have a slot that will not
compile, and the largest single cause is one missing command name.

---

## 1. The shape of the corpus

| | instruments | of the 6,037 `.nki` |
|---|---:|---|
| files swept | 6,037 | |
| load at all | 3,694 | 61 % |
| of those, with at least one script | 3,674 | |
| … whose every slot compiles | 2,956 | |
| … with a slot that does not compile | **718** | |
| … with no unanswered init action | 1,614 | 44 % |
| refused by the reader | 2,343 | 39 % |
| of the loaded: silent at every key tried (704 keyswept) | 180 | |
| of the loaded: no playable zone at all | 254 | |
| of the loaded: a UI asset that does not resolve | 1,571 | 21,570 references |

The three walls are different in kind and should not be added together:

- **39 % never open.** 2,343 instruments in three groups: the LASS bundle's
  key-locked `.nicnt` (1,151, which Kontakt refuses too), Garritan's Kontakt-2
  XML presets (624), and 544 instruments — mostly Heavyocity's — that hit one
  unimplemented modulator category. The last 24 are a missing program (18) or an
  unrecognised container (6).
- **20 % open with a dead script slot.** 718 instruments, listed by cause in §2.
- **The rest open, and "open" is not "play".** 3,694 instruments load; of those,
  254 have no playable zone at all — 242 of them Spitfire Glass & Steel patches
  whose every zone belongs to a group saved as purged (§4) — and 180 of the 704
  instruments re-rendered across nine keys are silent at every key tried. The
  180 counts the whole loaded set, not the 704: the 704 is the subset found
  silent at key 60, and key 60 is one of the nine, so an instrument that sounds
  anywhere in 36…84 was never a candidate for it. The number is exact for
  silence at these nine keys — every one of the 180 answered all nine renders —
  and it is an upper bound for instruments silent at every key, because one of
  them may sound at a key outside 36…84. No instrument that is silent
  everywhere can be missing from it. The remainder play, with wrong numbers in a
  script (the three `*EnginePar` rows, 1,599 / 1,430 / 321 instruments) or with
  pictures that are not on the disk at all (1,571). This row is therefore a
  statement about loading, not about sounding.

## 2. The ranked table

Every row is ranked by **instruments affected**, which is the only ordering the
data supports. It is *not* the same as instruments unblocked — the `kind` column
says which of the three walls a row sits behind:

- `reader` — the file does not open at all; nothing else about it can matter.
- `command` / `function` / `callback` — the instrument opens, and this call is
  answered with nothing (or the script that uses it does not compile).
- `ordering` — our front end refuses something Kontakt accepts. A rule to relax,
  not a feature to build.
- `bookkeeping` — the engine already answers; the ledger does not say so.
- `not ours` — the gap is in the library on disk, not in KONTRA.
- `out of scope` — Kontakt refuses it too.
- `symptom` — an observation, not a gap.
- `diagnostic` — a measurement the survey does not have: no count, so no
  ranking. Rows of this kind are kept out of the table below for that reason;
  the one there used to be is stated in prose underneath it.

`libraries` counts the product folders that contain at least one affected
instrument. `references` is call sites for a command, occurrences for a symbol.
For the silence row it is also the measurement's own denominator: how many keys
answered for each instrument it counts.

| gap | kind | instruments | libraries | what it would take | example |
|---|---|---:|---:|---|---|
| GetEnginePar | command | 1,599 | 22 | a probe to name the ids, then a read law per id | Concert Accordion.nki |
| UI asset not found | not ours | 1,571 | 49 | nothing: the file is not in the library (sampled, §4) | Bicycle - Sick Range.nki |
| GetEngineParDisp | command | 1,430 | 51 | a display-formatting law per parameter | Electro-Acoustic.nki |
| $MARK_2 … $MARK_28 (27 constants) | bookkeeping | 1,339 | 47 | a ledger table to fill — the values already exist | 40s Very Own - Drums |
| LASS `.nicnt` access data | out of scope | 1,151 | 1 | nothing we may do: Kontakt refuses it too | Aleatoric Non Vib Cellos.nki |
| %NOTE_DURATION | bookkeeping | 767 | 13 | nothing: implemented, the ledger row is stale | 02. Trumpet 1 Bucket Multi |
| legacy XML instrument (pre-Kontakt 4) | reader | 624 | 1 | a format to read (Kontakt-2 XML preset) | Bb Clarinet 1.nki |
| modulator category 3 | reader | 544 | 6 | one reader case, decoded from the native reader | Dramatic Techtronic 02.nki |
| unknown function num_slices_zone | function | 362 | 4 | a zone-slice model | a - Horn Solo - Core techniques.nki |
| SetEnginePar | command | 321 | 24 | a write law per id, plus the async type swap | Electro-Acoustic.nki |
| no playable zone at all | not ours | 254 | 7 | nothing: saved purge state (sampled) | Whistler Ensemble |
| GetFolder | command | 243 | 2 | one folder-kind enum case | Awesome Tension Arp IN C 2 MW Sweeping Filter.nki |
| unknown command hide_inst_icon | command | 188 | 2 | a no-op binding (one line) | AR Dataflow.nki |
| silent at every key with zones | symptom | 180 | 32 | a symptom, not a gap — see §5 | Accordions 2 Library |
| LoadArray | command | 75 | 8 | the numeric .nka array read | Ens Basses - KS.nki |
| AttachZone | command | 71 | 10 | a runtime zone model | Synth Loop Designer.nki |
| SetUiWfProperty | command | 61 | 1 | wire one property to the waveform view | Soprano Phrases Vowels.nki |
| LoadIrSample | command | 59 | 2 | fill the IR slot from the library's impulses | Baseball.nki |
| LoadPerformanceView | command | 58 | 2 | no format exists — cannot be done yet | 40s Very Own - Drums.nki |
| MfSetExportArea | command | 42 | 8 | the export-area object | Electro-Acoustic.nki |
| unknown callback _pgs_changed | callback | 36 | 3 | a callback alias (one line, if Kontakt honours it) | Distortion Lead.nki |
| unknown function zone_slice_start | function | 34 | 1 | the zone-slice model | Straight Pedal Combo 05.nki |
| expected a constant | ordering | 29 | 9 | an ordering fix: our parser refuses a `declare` Kontakt accepts | VI Full Ensemble.nki |
| SetNksNavPar | command | 26 | 11 | an NKS2 navigation model (deliberately absent) | Electric Mint.nki |
| SetNksNavName | command | 25 | 10 | the same model | Electric Mint.nki |
| $EVENT_VELOCITY is read-only | ordering | 22 | 1 | an ordering fix: these scripts clamp velocity, Kontakt allows it | Snare 1 Off.nki |
| GetVoiceLimit | command | 15 | 12 | a real per-group voice counter | Analog Hybrid Ensemble Designer.nki |
| SetVoiceLimit | command | 15 | 12 | a per-group voice cap | Analog Hybrid Ensemble Designer.nki |
| MfSetNumExportAreas | command | 13 | 5 | the export-area object | 40s Very Own - Drums.nki |
| redirect_output takes 2 arguments | ordering | 11 | 9 | an ordering fix: our signature is stricter than Kontakt's | Electric Ruby Deluxe.nki |
| MfCopyExportArea | command | 10 | 3 | the export-area object | Electric Mint.nki |
| SetRpn | command | 10 | 1 | an RPN port in the offline renderer | Ensemble - Traditional.nki |
| not a UI control | ordering | 10 | 1 | an ordering fix: accept a ui id held in a variable | Mute Trumpet.nki |
| $NUM_PRESETS is a reserved name | ordering | 9 | 8 | an ordering fix: it is not a Kontakt builtin | Action Strings 2.nki |
| ExposeControls | command | 9 | 2 | a host-automation exposure list | Omnia Altos - Lite.nki |
| LoadNativeUi | command | 9 | 2 | no format exists — cannot be done yet | Omnia Altos - Lite.nki |
| unknown function mf_get_note_length | function | 7 | 2 | one accessor on data `mf_get_note` already reads | Garage Kit - Lite.nki |
| unknown command dont_use_machine_mode | command | 5 | 3 | a no-op binding (Kontakt's is a no-op) | Analog Hybrid Loop Designer.nki |
| FindZone | command | 4 | 1 | a zone name index | NG Strum.nki |
| unknown command set_num_user_zones | command | 4 | 3 | the user-zone model | FOLDS.nki |
| ?main_controls_inst_panels_1_icon_panel_stage is not declared | function | 2 | 2 | an ordering fix: undeclared text variable | Lores.nki |
| GetSampleLength | command | 2 | 2 | one sample-metadata field | Analog Hybrid Kit Designer.nki |
| ?home_vr_xypad is not declared | function | 1 | 1 | an ordering fix: undeclared text variable | World Strings - Guzheng.nki |
| LoadMidiFile | command | 1 | 1 | a MIDI-file read into an array | Sonica KOTO 20.nki |
| more values than array elements | function | 1 | 1 | an ordering fix: KSP lets the surplus be ignored | Mizwed - Improv.nki |

One row is not in that table. **`silence with no explanation`** (kind
`diagnostic`) is a report the survey cannot produce yet — which zones named a
file that is not there, and why a zone produced nothing — so it has no
instrument count and cannot be ranked beside the others. It was in the table
when this file was written, with an `unmeasured` cell in an instruments-ordered
column, which made the ranking look as if it had covered it. It is §3's "gap
this survey found about itself": a defect in the measurement, not in the engine.

Three things about that table are worth saying out loud, because the ranking
alone would mislead:

1. **The top row cannot be worked yet.** `GetEnginePar` is answered for modelled
   parameters and declines for the rest; the census counts *calls*, not *ids*.
   342,005 declined calls cannot be turned into a work list until something
   records which parameter was asked for. That is a probe, not a patch (§5).
2. **The two largest rows are not ours.** The unresolved UI assets (1,571
   instruments, 49 libraries) are files the publishers never shipped — sampled
   and confirmed in §4 — and LASS is refused by Kontakt as well. A ranking by
   instrument count without that distinction would put two impossible rows at
   the top of a to-do list.
3. **`hide_inst_icon` is the cheapest real win in the file.** 188 instruments —
   every VSTBuzz Power Move patch — and the *only* compile error they have is
   that one command name. Kontakt's `hide_inst_icon()` hides the icon in its own
   header; KONTRA draws no header. A no-op binding compiles all 188 slots.

## 3. The per-library view

The user's own libraries matter more than the long tail, so here they are by
`.nki` count. "unanswered" is instruments with at least one unanswered init
action; "script fails" is instruments whose slot does not compile.

| library | .nki | loads | refused | no zone | silent | unanswered | script fails | UI refs | the one thing it needs |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---|
| Evolve R2 Library | 1,267 | 922 | 345 | 0 | 0 | 922 | 0 | 0 | modulator category 3 (327 of the refusals), then the two `*EnginePar` reads |
| AudioBro LASS Bundles | 1,151 | 0 | 1,151 | 0 | 0 | 0 | 0 | 0 | nothing we can do: key-locked `.nicnt`, Kontakt refuses it too |
| Glory Days – Big Band Horns | 643 | 643 | 0 | 0 | 0 | 0 | 0 | 2,076 | nothing: every instrument loads, no unanswered action, no dead script; only pictures, which the library does not ship |
| Garritan – Jazz & Big Band | 624 | 0 | 624 | 0 | 0 | 0 | 0 | 0 | the legacy XML reader |
| Spitfire Glass & Steel | 242 | 242 | 0 | 242 | 0 | 242 | 0 | 490 | saved purge state, not a gap; `GetFolder`/`GetEnginePar` after that |
| Kinetic Metal Library | 211 | 130 | 81 | 0 | 17 | 0 | 0 | 0 | modulator category 3 (81), then the silence |
| Spitfire Studio Brass | 204 | 204 | 0 | 0 | 0 | 204 | **204** | 1,020 | the zone-slice model — all 204 fail on it |
| Heavyocity Vocalise 3 | 190 | 190 | 0 | 0 | 0 | 0 | **190** | 0 | the zone-slice model |
| VSTBuzz Power Move | 160 | 160 | 0 | 0 | 0 | 0 | **160** | 0 | `hide_inst_icon`, one line |
| Sauceware Audio Trifecta | 109 | 0 | 109 | 0 | 0 | 0 | 0 | 0 | modulator category 3 |
| Vintage Organs Library | 100 | 100 | 0 | 0 | 0 | 100 | 0 | 0 | `GetEnginePar` / `GetEngineParDisp` |
| ~~Arabian Ethnic Orchestra~~ | — | — | — | — | — | — | — | — | **void: library deleted 2026-11-01** (the user removed it; its licence was never applied, so nothing it did was evidence about KONTRA) |
| Noon Instruments Toska | 84 | 84 | 0 | 0 | 0 | 84 | 0 | 84 | `GetEnginePar` / `GetEngineParDisp` |

Read down that table and the plan picks itself:

- **Glory Days is already script-clean** — 643 of 643 load, 0 script
  failures, 0 unanswered init actions; its only residue is 2,076 UI references
  the library does not ship (pictures). Nothing to do.
- **Vocalise 3, Power Move and Spitfire Brass** are all one *gap* from being
  script-clean, and only one of the three is one line: Power Move fails on
  `hide_inst_icon` alone, a no-op binding; Vocalise 3 (190) and Spitfire Brass
  (204) fail on the zone-slice model, which §5 calls the one row here that needs
  a *model* rather than a binding. Reading "one line each" off this table is how
  the largest real language gap gets estimated at an afternoon.
- **Evolve R2 + Trifecta + Kinetic Metal** are one reader case from opening 517
  of the 544 category-3 refusals (327 / 109 / 81). The other 27 are Kinetic Toys
  (19), Studio Series - Fire Saxophone (7) and Porphyra (1) — the same reader
  case, in libraries too small for §3's rows. Note that the per-library table's
  `refused` column is the wrong number to add up here: Evolve's 345 is 327
  modulator refusals plus 18 in the `program: missing` cause, which is a
  different reader case.

- ~~**Arabian Ethnic Orchestra** is the only library where instruments exist,
  have zones, load without complaint and make no sound — 83 of 84.~~
  **Withdrawn 2026-11-01.** Those 83 silent instruments were never evidence about
  KONTRA: the user reports the library's licence was never applied, and the
  library has since been deleted from the machine. Nothing it did should be used
  to rank a gap, and this table's `LoadArray` row was ranked partly on it.

### The gap this survey found about itself

Measured 2026-11-01 on a library that could not sound: the render reported

```
peak 0.0000  frames 168448  zones 255  load_s 0.03
stream: StreamStats { pages_read: 0, read_errors: 0, requests_refused: 0, bytes_read: 0 }
```

— silence, with **no error, no refused request and not one page read**. Two
consequences, and the second is worse than the first:

1. A user cannot tell "this library is not installed / not licensed" from "KONTRA
   has a bug". The instrument that produced the line above was reported to us as
   broken; it was a licence, and it sent a day of work in the wrong direction.
2. **Every silence-based statistic in this file is confounded by it.** The
   keysweep's 180 "silent with zones", the `LoadArray` correlation, and the
   `GetEnginePar` silence check all count instruments that may simply have no
   usable content. The survey cannot currently separate "we are silent" from
   "there is nothing to play".

The fix is a reporting one — count and name zones whose file is absent, at load
and in the render summary — and it was in flight when the session stopped. Until
it lands, treat any silence count here as an upper bound.

## 4. What is out of scope, and why

- **LASS, 1,151 instruments.** The four `.nicnt` files are present; the content
  is key-locked and the reader reports "no access data". Kontakt refuses these
  too on a machine without the library activated. This is recorded as out of
  scope by the orchestrator and was **not re-verified in this pass** — the claim
  to check, if anyone wants to spend the time, is that Kontakt 8 refuses these
  files as well.
- **The 254 zero-playable-zone instruments are not a gap.** 242 of them are
  Spitfire Glass & Steel patches whose every zone belongs to a group saved as
  purged; that is stored state, and `docs/re/KONTAKT_PURGE_RESIDENCY.md` already
  says so. It is not a load gap — but it is not settled either: `STATUS.md`
  records Kontakt playing a zero-zone Glass & Steel patch at −9.8 dBFS, so the
  open question is *which* saved purge states Kontakt restores, not whether we
  can read the file.
- **The 1,571 instruments with unresolved UI assets are (in every sampled case)
  the library's fault.** 44 instruments across 23 libraries were re-loaded on
  today's build: 1,503 unresolved references, 1,490 distinct paths, **all**
  absent from the library on disk, and **none** of the 25 sampled library
  folders ships a `pictures/` directory at all. The census counts reproduced
  exactly — the picture-sidecar fix of this morning moved none of them. So the
  right response to a picture that "does not resolve" is to check the publisher's
  install, not the reader. (Caveat: a picture inside an `.nkr` would also read as
  absent here; the reader itself searches `.nkr`/`.nicnt`, so this only fails to
  exonerate us if our archive index is wrong.)
- **Kontakt's private performance views (58 instruments) and native UI pages
  (9)** have no format in this repo. They are not schedulable work.

## 5. Do these three first

**Rewritten 2026-11-02, because two of the three no longer stand.**

The first version recommended, in order: `hide_inst_icon` plus five `ordering`
rules, `LoadArray`, and the zone-slice model. Two of those are gone:

- **The bindings were done the same evening** (`docs/re/ksp-bindings-2026-11-01.md`):
  728 failing slots in 718 instruments → **478 in 470**, 248 instruments
  unblocked, no regressions. What is left of that family is small:
  `dont_use_machine_mode` (25 slots), `_pgs_changed` (42),
  `mf_get_note_length` (7), `set_num_user_zones` (4).
- **`LoadArray` is not a silence cause, and the measurement says so.** 504
  keyswept instruments call it; **112 are silent, 22 % — *below* the 26 % base
  rate** of the 704 keyswept. The 57-of-61 figure this section was built on
  counts only instruments whose `load_array` was *unanswered at init* (75, of
  which 61 keyswept) — a narrower denominator that made a non-effect look like a
  93 % correlation. It remains worth doing for the 75 instruments' script
  capability; it is not the path to sound.

Judged by instruments per week of work, with no audio risk:

### 1. The browser hides 563 instruments — one scan rule

Measured over the library root: 18 libraries hold presets at one level and more
in a subfolder, and the scan stops descending as soon as a folder looks like a
library. Kinetic Metal 100 visible / **111 hidden**, Evolve R2 96/**92**, Tom
Factory 6/**66**, Voices of Rapture 4/**57**, Royal Albert Hall 1/**56**, VSTBuzz
Power Move 160/**27** — **563 instruments the user cannot reach from the browser
at all**, 15 % of everything that loads. The instruments are not broken; they are
invisible. This is a rule in `crates/app/library`, no parser and no DSP, and it is
the highest value per hour in the file — a user looking for a library that is
half-hidden has no way to tell that from a broken instrument.

### 2. The zone-slice model — 396 instruments

`num_slices_zone` (362) and `zone_slice_start` (34), all in loop designers: all
204 of Spitfire Studio Brass, all 190 of Heavyocity Vocalise 3, plus one
instrument each in two Heavyocity Damage libraries. Slices are an edit-time
division of a sample that neither the plan nor the playback has, so this is the
largest row that needs a *model* rather than a binding.

### 3. Garritan's legacy XML — 624 instruments Kontakt loads and we refuse

The single largest reader gap where the two sides disagree: Kontakt reads these
and we answer "legacy XML instruments are not read". 624 instruments in one
library. It was listed under "out of scope" in the first version; it should not
be — out of scope is LASS, where Kontakt refuses too. This is a format someone
has to decide to read or to declare unsupported on purpose.

**What not to do first.** The **180 silent-with-zones** instruments have no
proven cause any more: the two candidates this file offered (`LoadArray`,
`GetEnginePar`) both measure *below* the base rate — 22 % and 6 % against 26 %.
The next step is a diagnosis, not a patch: take the 180, group by library and by
what their scripts do at `on note`, and find one whose silence is reproducible
and explicable. `GetEnginePar` itself (1,599 instruments, 342,005 declined calls)
still cannot be estimated: nothing in the data says *which* ids are declined, so
its first step is one probe — a generated script calling `get_engine_par` for
every `$ENGINE_PAR_*` in every slot, against the oracle — and the answer is a
work list of N laws with N known.

## 6. Method — what was measured, how to re-run it

Everything below was run from `/var/tmp/wt-gaps` (worktree of `feat/integration`
at `8c64fccd0`) unless a path says otherwise. Working files are under
`/var/tmp/gaps/` (not `/tmp`, which is a 16 GB tmpfs here).

1. **The corpus census was not re-run.** `/var/tmp/census/partA/shard-*.jsonl`
   (6 shards, today's load sweep) is the source for every load, refusal,
   diagnostic and UI-asset count. `python3 analyze_census.py` aggregates it into
   `census_summary.json`. The pre-aggregated table in the brief and this pass
   differ by **at most 0.33 %** of each row's own count. The formula is the
   relative change `|after − before| / after` against this pass — a ratio of
   counts, not percentage points — and the two runs counted different corpora:
   the brief's 3,681 loaded instruments, this pass's 3,694 of 6,037 `.nki`
   records (13 more loaded; the brief's record total is not recorded). Row by
   row, brief → this pass: `GetEnginePar` 1,597 → 1,599 instruments (0.13 %) and
   341,995 → 342,005 calls (0.003 %); `GetEngineParDisp` 1,428 → 1,430 (0.14 %)
   and 17,075 → 17,131 (0.33 %); UI assets 1,566 of 3,681 → 1,571 of 3,694
   (0.32 % of the instrument count). The largest delta is smaller than the 13-file
   difference between the two corpora (0.35 %), so the two passes disagree about
   which records count as loaded, not about what the shards say.
2. **Scripts were extracted, not re-swept.**
   `python3 extract.py census.json /var/tmp/gaps` runs the repository's own
   `examples/dump_scripts` binary (`cargo build --example dump_scripts -p
   kontra-format-kontakt`) once per loaded instrument: 3,694 instruments,
   **8,016 script slots, 3.8 GB of KSP**. All 3,694 succeeded; a refused
   instrument has no script text to extract, so the symbol surface of the 2,343
   refused instruments is *not* in this survey.
3. **The symbol pass used the project's own scanner**, not a reimplementation:
   `analyze_symbols.py` imports `tools/re/ksp_symbols.py` and calls its
   `Resolver`, `scan_script` and `symbol_report`, so a status here is the status
   `ksp_symbols.py doctor` would print. 1,324 distinct names were seen.
4. **The compile pass is the new instrument.** `compile.tsv` is
   `kspcheck` — a 40-line binary in `tools/census/kspcheck` that links
   `crates/script/ksp` by path and calls `kontra_ksp::compile(src, &[])` on each
   of the 8,016 files — run as
   `ls scripts/*/slot-*.ksp | kspcheck > compile.tsv`: 7,288 compile, 728 fail,
   in 718 instruments. **This is where the symbol table's blind spot shows:** an
   unknown *command* is invisible to the text scanner, because the scanner only
   counts names it can resolve through the ledger. `hide_inst_icon` — 188
   instruments — does not appear anywhere in `docs/re/ksp/SYMBOLS.tsv`.
5. **The UI re-check** is `ui_sample.py 44`: 44 instruments sampled by library
   from the 1,571 with unresolved assets, each re-loaded with today's
   `kontra-inspect` (release, built after the `picture.rs` fix), every unresolved
   asset path tested against a lowercased index of the library's files
   (`Samples/` pruned).
6. **Silence** is `/var/tmp/keysweep/results.jsonl`: 704 instruments, 9 keys each
   (36…84), `sounds_at` empty = silent at every key tried. A record counts as
   silent only when all nine keys answered — a render that failed or timed out
   leaves no peak, and this pass's 180 all have nine; the seven records that lost
   a render are counted as errors, not as silence (`keysweep_errors` in
   `census_summary.json`).
7. **The table and this document's numbers** are generated:
   `build_tsv.py docs/re/gap-inventory-2026-11-01.tsv` rewrites the `.tsv` beside
   this file, `table_md.py docs/re/gap-inventory-2026-11-01.tsv` renders §2.

The measurement scripts are **committed**, under `tools/census/`:
`census_sweep.py` (step 1), `keysweep.py` (6), `merge_census.py`,
`analyze_census.py` (3), `extract.py` (2), `analyze_symbols.py` and
`merge_symbols.py` (3), `compile_fail.py` and `kspcheck/` (4), `ui_sample.py`
(5), `full_check.py`, `build_tsv.py`, `table_md.py`, `where.py` and
`instruments.py` (7). `tools/census/README.md` gives the command order, the
binaries they need (`kontra-inspect`, `kontra-render` and the `dump_scripts`
example from this workspace, plus `kspcheck`, built from `kspcheck/` itself) and
the runtime of each step; the scripts take their work directory from
`CENSUS_WORK` (default `/var/tmp/gaps`) and their binaries from
`KONTRA_INSPECT`, `KONTRA_RENDER` and `DUMP_SCRIPTS`. Nothing generated is
committed: `census.json`, `scripts/`, the `.tsv`s and `compile-*.json` are 4.2 GB
of output, which is why the document and the `.tsv` are the only records.

**Steps 1, 2, 5 and 6 need the sample library on disk** — the `.nki` files are
where the script text and the audio are — so those four are reproducible only on
a machine with the same libraries installed. Steps 3, 4 and 7 need only what the
earlier steps wrote; that was checked by re-running them against this pass's own
artifacts on 2026-11-01: `analyze_census.py` reproduced every count in
`census_summary.json` (its silence rows now also record `keys_answered`, and its
totals `silent_unmeasured` and `keysweep_errors`, which this pass did not
write), `compile_fail.py` reproduced `compile_fail.json` identically from
`compile.tsv`, and `build_tsv.py` reproduced the committed `gap-inventory.tsv`
byte-for-byte.

### What this survey cannot see

- **The 2,343 refused instruments' scripts.** No text, no compile, no symbol
  count. Their language needs are unknown, not zero — Garritan's 624 in
  particular may need KSP that nothing else in the corpus uses.
- **Which engine parameter ids are declined** (`GetEnginePar` and
  `*Disp`), which is the largest row in the table and the least actionable.
- **Whether a symbol that fails to compile is the *only* thing wrong with that
  instrument.** A compiled slot can still be answered wrongly; the counter only
  sees the front end.
- **Runtime behaviour after `init`.** The census records init; a script that
  only calls an unimplemented action from `on ui_control` is invisible here.
- **The oracle.** Nothing in this pass was compared against Kontakt. Where the
  numbers say "Kontakt refuses it too" or "Kontakt allows it", the evidence is
  either the orchestrator's earlier measurement (LASS) or the scripts themselves
  (velocity clamping) — not a fresh Kontakt run.
- **The 6 files that are neither XML, modulator nor `.nicnt`** (18 "program:
  missing" in Evolve, 6 unparsed) are counted, not diagnosed.
- **Sampling.** §3's per-library table is complete; §4's UI claim is a 44-
  instrument sample of 23 libraries, and §5's `LoadArray` claim is a correlation
  over 61 keyswept instruments. Both are labelled as such where they are used.

## 7. What the numbers say about the project, not just the corpus

- The ledger is not a measurement. Two of its "unsupported" rows are answered by
  the engine (`%NOTE_DURATION` by `Action::NoteDuration`, `$MARK_2…28` by
  `constants.tsv`), one of its owners points at a crate that has not existed for
  some time (`crates/sampler-ksp/src/sema.rs`), and it has no row at all for a
  command that blocks 188 instruments. A symbol table maintained by hand beside
  the code will drift; the compile pass is cheap enough (53 s for the whole
  corpus) to be the measurement instead.
- `tools/verify.sh` still prints `VERIFY PASSED` and Electric Mint's fingerprint
  is still `f18acaab5b897588`: nothing executable changed. That is the point —
  the first three items in §5 are each smaller than most changes this tree
  already carries.
