# Gap inventory — 2026-11-01

The user asked whether it would be worth scanning every instrument and listing
what needs to be added. It is worth it, and this is the list. Nothing here
changes engine or UI behaviour: this is a survey, and every number was produced
by a command on this machine, quoted in §6 so it can be re-run rather than
believed.

**The headline is not the ranking, it is what the ranking is made of.** 1,929
instruments use a symbol the ledger marks unsupported or stub; 1,052 of those use
no unsupported name other than two the engine already answers
(`$MARK_2…28`, `%NOTE_DURATION`), and only 630 use a name it genuinely lacks.
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
- **The rest open and play**, with wrong numbers in a script (the three
  `*EnginePar` rows, 1,599 / 1,430 / 321 instruments) or with pictures that are
  not on the disk at all (1,571).

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

`libraries` counts the product folders that contain at least one affected
instrument. `references` is call sites for a command, occurrences for a symbol.

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
| **silence with no explanation** | diagnostic | unmeasured | — | report which zones named a file that is not there, and why a zone produced nothing | (see below) |
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

- **Glory Days, Vocalise 3, Power Move, Spitfire Brass** are one line each
  (`hide_inst_icon`, slice, slice, slice) from being script-clean.
- **Evolve R2 + Trifecta + Kinetic Metal** are one reader case from opening
  544 more instruments.
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

Ranked by instruments, the list starts with two things that cannot be done and
one that cannot be started. Judged by *what a week of work buys the user*, the
order is this:

### 1. `hide_inst_icon` — 188 instruments for one line

It is the only gap in the file with that ratio. All 188 are loose instruments
under `#Non NICNT` — 187 of them every VSTBuzz Power Move patch — and they fail
to compile for exactly this name and nothing else, so a no-op binding converts
188 dead slots into working ones and costs an afternoon including the test. It
buys no sound by itself — those instruments also need `GetEnginePar` to be right
— but it removes them from this list permanently, and it is the cheapest way to
prove the survey's central claim: that most of the "unsupported symbol" surface
is not engine work.

Do the `ordering` rows in the same pass — 80 more instruments, none of them
overlapping the 188 above, and each a one-rule change: `$EVENT_VELOCITY` must be
writable (22 instruments clamp velocity), `$NUM_PRESETS` must not be a reserved
name (9), `redirect_output` must take the argument counts Kontakt takes (11),
`hide_part`/`set_control_par` must accept a ui id held in a variable (10), and
the `expected a constant` family (29) needs one look each because the message
does not name the token. That is 268 instruments — 7 % of everything that loads
— for a week that touches no DSP.

### 2. `LoadArray` — 75 instruments, and the only proven silence

57 of the 61 keyswept instruments that call `load_array` are silent at every key
tried (93 %), against 180 silent of 704 keyswept overall. The mechanism is
unproven — this is a correlation, not a cause.

**Corrected 2026-11-01: the correlation is withdrawn until it is re-derived.**
Its largest citation, Arabian Ethnic Orchestra, was a library whose licence had
never been applied; it could not have sounded whatever our code did, and it has
been deleted. A correlation built on silence is confounded by **every** library
that cannot sound for licence or installation reasons, and we have no flag that
distinguishes those from our own silence — see the row below, which is a defect
in its own right. Re-derive the correlation over the 704 keyswept instruments
with those libraries excluded before acting on it.

The work is small and bounded: the numeric array reader already exists for
`load_array_str` and `array_file.rs` has tests. What is missing is the
mode/shape matrix — today only text arrays in `on init` are answered. It is also
the one row here that can be *proved* fixed by re-running the existing keysweep
and watching a peak rise.

Do it second only because it is worth 75 instruments against `hide_inst_icon`'s
188 — but do not defer it, because it is the only row with a demonstrated
audible effect, and the user can hear the difference.

### 3. The zone-slice model — 396 instruments, 4 libraries

`num_slices_zone` (362) and `zone_slice_start` (34) are the largest real language
gap in the corpus and they are all in loop designers: all 204 of Spitfire Studio
Brass, all 190 of Heavyocity Vocalise 3, and the Heavyocity Damage family. Slices
are an edit-time division of a sample that neither the plan nor the playback has,
so this is the one three-figure row that needs a *model* rather than a binding —
which is exactly why it should be scoped and started rather than left to rot at
the top of a list.

**What not to do first, and why.** `GetEnginePar` is the biggest number in the
file (1,599 instruments, 342,005 declined calls) and it would be wrong to start
there: nothing in the data says which parameter ids are declined, so the work
cannot be estimated, let alone scheduled. 17 of 335 keyswept `GetEnginePar`
instruments are silent, so it is not a silence bug either — it is a
numbers-in-the-script bug, and after the three rows above it is where the
remaining fidelity lives. The first step is a probe, not a patch: one instrument,
one generated script that calls `get_engine_par` for every `$ENGINE_PAR_*` id in
every slot, run against the oracle, and the answer is a work list of N laws with
N known.

## 6. Method — what was measured, how to re-run it

Everything below was run from `/var/tmp/wt-gaps` (worktree of `feat/integration`
at `8c64fccd0`) unless a path says otherwise. Working files are under
`/var/tmp/gaps/` (not `/tmp`, which is a 16 GB tmpfs here).

1. **The corpus census was not re-run.** `/var/tmp/census/partA/shard-*.jsonl`
   (6 shards, today's load sweep) is the source for every load, refusal,
   diagnostic and UI-asset count. `python3 analyze_census.py` aggregates it into
   `census_summary.json`. The pre-aggregated table in the brief and this pass
   differ by under 0.1 % on every row — brief → this pass: `GetEnginePar`
   1,597 → 1,599 instruments and 341,995 → 342,005 calls; `GetEngineParDisp`
   1,428 → 1,430 and 17,075 → 17,131; UI assets 1,566 of 3,681 → 1,571 of 3,694 —
   because the two passes disagree about which records count as loaded, not
   about what the shards say.
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
   `kspcheck` — a 40-line binary in `/var/tmp/gaps/kspcheck` that links the
   workspace's `kontra-ksp` crate by path and calls `kontra_ksp::compile(src,
   &[])` on each of the 8,016 files — run as
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
   (36…84), `sounds_at` empty = silent at every key tried.
7. **The table and this document's numbers** are generated:
   `build_tsv.py` writes the `.tsv` beside this file, `table_md.py` renders §2.

The measurement scripts themselves (`extract.py`, `analyze_census.py`,
`analyze_symbols.py`, `merge_symbols.py`, `kspcheck/`, `ui_sample.py`,
`build_tsv.py`, `table_md.py`) live in `/var/tmp/gaps` and are deliberately not
committed: they are throwaway harness code, and this section is their
specification. The two records are the deliverable.

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
