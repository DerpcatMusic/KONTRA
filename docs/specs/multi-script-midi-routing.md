---
status: in-progress
project: kontra
owner: claude-opus-5-5
created: 2026-11-24
updated: 2026-11-24
proof_scope: internal
evidence_freshness_days: 180
---

# Multi slots: a MIDI player's notes reach the slot on its channel

## Problem & Goal

- **Problem:** A Kontakt multi whose slot 1 is a sample-less script part
  (Conflux's factory "Midi Player" arpeggiator) and whose slot 2 holds the
  synth opened, since #160, as one part with the player dropped: the
  patterns never played.
- **Goal:** Notes the player's scripts make (`play_note`) reach the sounding
  slot that listens on the player's channel, while the multi still opens as
  one part named after it (#160's fix).
- **Plan Reference:** `docs/architecture-v3/13-IR-VNEXT.md`, Kontakt parity lane.

## Primary-Source & Version Proof

Internal: a translator law over IR already produced (slots, channels,
scripts); no external API is relied on. Kontakt oracle evidence:

- measured: `mf_get_next_at` / `mf_get_prev_at` include an event at the
  position (`tools/kontakt-oracle/probes/ksp/mf_at_edges.kspp`, 8.13);
- **pending:** two players on one channel. Does each get the original
  note, does player B get A's generated notes, and how far does
  `ignore_event` reach? The fold assumes slot order, the way the engine
  passes `play_note` notes on; it stands only if the oracle agrees.

## Decision

A translator law in `kontra-format-kontakt` (`multi::fold_players`), not a
new engine concept: a slot with no zones whose channel a sounding slot
hears (`a == 0 || b == 0 || a == b`, 0 = omni) puts its scripts, in slot
order, **in front of** that slot's own script chain, and is no slot of its
own. The engine already passes a script's `play_note` notes to the script
slots after it as their own notes (`engine/src/script.rs`), which is what a
listening slot sees in Kontakt. So:

- no engine or IR change, nothing vendor-named in the engine;
- RT-safe by construction: it runs on the control side at translation;
- no feedback loops by construction: a chain only feeds the scripts after
  it, so a script never hears its own notes. There is no routing graph to
  reject cycles in.
- Script `slot` places are renumbered (players first) so presets and saved
  sessions keep each script's values; a reread of one slot (`multi/<index>`)
  folds the same way.

A player is a slot that **sounds nothing itself**: no zones, or zones that
play only digital silence (`multi::sounds`: a plain WAV of at most 1 MiB
whose data is all zeros). Kontakt's Chords tool has one zone on
`ghost sample.wav`, half a second of zeros.

A slot whose resource container (its program's
`resource_container_filename_ref`) the multi's file table names relative to
a Kontakt installation (`Content/Tools/Chords/Samples/Chords 1.0.nkr`) is
translated against that container, not the multi's library: its scripts'
`load_array(.., 2)` data comes from it. The host passes the installations
it knows (`kontra_library::kontakt::candidates()`); the format crate takes
plain paths and does not depend on kontra-library. Each of such a slot's
scripts carries that container's folder (`ir::Script::container`), and
keeps it when folded into the synth: the host sets the script's
`rc_folder` (`$GET_FOLDER_RC_DIR`) from it whatever its source names, and
a file the script reads relative to it (up to the folder above it, its
library) matches names without case.

The Chords player reads its patterns from
`get_folder(%get_folder_argument[0]) & !custom_path_modifiers[..]`; its
`.nkr` data holds `get_folder_argument = [4]` (resource container dir) and
the modifier `"../"`, so it asks for
`<Chords/Samples/>../Library Data/Midi Files/<set>/<pattern>.mid`, and
ships `MIDI Files/`. The factory folder (`$GET_FOLDER_FACTORY_DIR`) plays
no part and is left as it was (`factory_folder_of`).

Rejected: a voice-less "script part" that forwards its output events to
other parts' inputs. It needs an output queue per part, a part graph with
cycle checks at prepare, and a second hidden part in the UI, for the same
audible result.

## Invariants & Non-Goals

### Hard Invariants
- **Audio RT-Safety:** unchanged audio path; the fold runs off the audio thread.
- **Determinism & Compatibility:** the folded chain is a pure function of
  the multi; saved values address scripts by their new place.
- **Mirror Boundaries:** nothing outside `plugins/kontra`.

### Non-Goals
- Notes a *sounding* slot's scripts make reaching another sounding slot.
- One player feeding two sounding slots runs once per slot (a copy each);
  a randomising player may differ between them.

## API / Data / File Contract

- `kontra_format_kontakt::multi_load` / `multi` take `installs: &[PathBuf]`
  and return folded slots; with `only`, every slot is translated so its
  players can fold in. `Slot::sounds` says whether a slot sounds itself.
- `ir::Script::container`: the folder of the resource container the
  script came with, when the translator knows it.

## Verification Matrix

| Check | Tier | Target / OS | Exact Command | Pass Criterion | Status / Observed Evidence |
|---|---|---|---|---|---|
| Spec proof gate | Doc | any | `python3 tools/check-spec-proof.py --changed origin/main` | Exit 0 | PASS (2026-11-24, linux-x86_64) |
| Fold law unit tests | Compile/ABI | linux-x86_64 | `matari fast kontra -p kontra-format-kontakt` | `multi::tests::*` pass, incl. `players_never_feed_themselves` | PASS (2026-11-24, linux-x86_64) |
| Player note plays through the chain | Validator/Runtime | linux-x86_64 | `matari fast kontra -p kontra-host` | `a_players_note_plays_through_the_script_after_it` passes | PASS (2026-11-24, linux-x86_64) |
| Conflux patch audio from a held key | Validator/Runtime | linux-x86_64 with libraries | `build-slot cargo nextest run -p kontra-plugin -E 'test(a_conflux_patch_opens)'` | one part named "Airy Soul"; rms > 1e-4 from one held key | PASS (2026-11-24, linux-x86_64: all twelve pattern tracks reload at load, the `.mid` misses are the patch's own five empty slots and fail as in the original; the held key plays the chord) |
| Load-time wait chain done before the first note | Validator/Runtime | linux-x86_64 | `matari fast kontra -p kontra-host` | `a_load_time_wait_chain_is_done_before_the_first_note` passes | PASS (2026-11-24, linux-x86_64) |
| MIDI marker at a position includes an event there | Validator/Runtime | linux-x86_64 + Kontakt 8.13 oracle | `python3 tools/kontakt-oracle/ksp_probe.py <ANALOG STRINGS.nki> tools/kontakt-oracle/probes/ksp/mf_at_edges.kspp`; `matari fast kontra -p kontra-host` | oracle: next_at(0)=2, next_at(50)=3, prev_at(100)=3, prev_at(0)=2; `the_midi_marker_at_a_position_includes_an_event_there` passes | PASS (2026-11-24, linux-x86_64; oracle values as listed) |
| Conflux notes are the player's, not the key's | Validator/Runtime | linux-x86_64 with libraries | (not written yet) | the held key's own voice is ignored and the voices that sound carry the player's script source | PENDING (follow-up; the player ignores the key, so today's rms comes from its notes, but no test asserts it) |
| Two same-channel players route as in Kontakt | Validator/Runtime | linux-x86_64 + Kontakt oracle | (probe not written yet) | original note reaches each player; A's generated notes reach B; `ignore_event` scope matches | PENDING (review #177; the fold's slot-order assumption) |
| Folded player hears only its own channel | Validator/Runtime | linux-x86_64 | (not written yet) | a channel-1 player folded into an omni synth runs on channel-1 notes only | PENDING (needs a per-script channel gate in the engine) |
| Container-relative read ignores case | Compile/ABI | linux-x86_64 | `matari fast kontra -p kontra-host` | `a_read_relative_to_the_container_folder_ignores_case` passes | PASS (2026-11-24, linux-x86_64) |

## Why Conflux was silent (engine trace, 2026-11-24)

The empty `Midi Files/.mid`
reads were not the fault: the patch saves five of its twelve pattern
slots as "Empty" (`!pattern_paths` = ""), and those reads fail and
complete as they should. The player was silent for two engine reasons:
1. its load-time `on persistence_changed` reloads all twelve tracks,
   waiting on each file; once loading ended a resumed chain ran 12 loop
   turns a block (`BLOCK_SCRIPT_TURNS`), so `$opccb_done` stayed 0 (and
   `on note` ignored the key) for about 6 s of audio. The engine now runs
   the script work due within 0.25 s of script time at load
   (`Engine::run_load_waits`), where files read at once;
2. `mf_get_next_at` / `mf_get_prev_at` skipped an event exactly at the
   position, so a chord on tick 0 never played. Measured inclusive in
   Kontakt 8.13 (`probes/ksp/mf_at_edges.kspp`).

## Unverified Boundaries & Promotion Gate

- Two same-channel players: routing unmeasured in the oracle (see
  Primary-Source); the fold assumes slot order.
- A player folded into an omni slot hears every channel (no per-script
  channel gate yet); the original hears only its own.
- The real Conflux multi asserting script-generated notes from a held key
  (follow-up; the existing test only checks one part named after the patch
  that plays).
