---
status: draft
project: plugins/kontra
owner: claude-opus-5-5
created: 2026-11-24
updated: 2026-11-24
proof_scope: internal
evidence_freshness_days: 180
---

# Persistent library index with auto-relocation

A user reported that their libraries were scanned once and then gone after a
Bitwig restart. Every plugin open ran a full live rescan, and the catalog
started out EMPTY. If a drive was unreadable at that moment (an HDD automount
still asleep, or a missing Flatpak permission), the catalog came back empty.
Projects store absolute preset paths, so a library that moved also broke
every project that used it.

Stacked on `library-discovery.md` (#163). This spec reuses that change's
per-root diagnostics as the reasons it shows for offline libraries.

## Acceptance criteria

1. **Persistent index is the source of truth.**
   - The full catalog is written to `$CONFIG/kontra-v3/library-index.json`.
     It holds, for each library: identity, last-seen folder, name, vendor,
     format, presets relative to the library, art, offline reason, the
     folders it moved from, and folder stamps. The file lives in the config
     directory, not in a purgeable cache.
   - `Browser::open` loads the index into the catalog before any scan
     starts, so the catalog is never empty while a scan is still running.
   - The file is written whole to a temporary file and then renamed into
     place.
2. **Incremental scan.** Each library records the modification time of every
   folder its preset walk visited. If none of those times changed, the next
   scan reuses the library's presets and art without walking it.
   `Progress.folders` shows the folders that were skipped.
3. **Event-driven refresh.** A rescan starts in these cases:
   - when a root is added or removed (unchanged behaviour);
   - when the plugin opens, as an incremental revalidation;
   - when `Browser::poll` runs, at most every 30 s, and a background stat
     check finds that something changed. The check fires on any of:
     - an offline library's folder or root becoming readable (a mount
       appeared);
     - an online library's stamped folder changing or disappearing;
     - a root's own modification time changing.

   The stat check runs on a `kontra-watch` thread, so `poll` never touches
   the disk. The `notify` crate is not in `Cargo.lock`, so std polling is
   used and no dependency is added.
4. **Missing is not deleted.**
   - If a library from the index is not found by a scan but its folder is
     still under a current root, it stays in the catalog with
     `offline = Some(reason)`. The reason comes from the root's #163
     summary, or is "not found". Its presets stay listed.
   - The UI dims an offline library's preset list and shows an
     `Offline · {reason}` pill.
   - A library is dropped only when the player removes or hides the root
     it sits under.
5. **Stable identity.** `kontra_library::identity` gives each library an id
   that does not depend on its path:
   - Kontakt with a `.nicnt`: `nicnt:{product}|{company}`.
   - A lone `.ufs`/`.sf2`/`.sf3` bank: `bank:{ext}:{size}:{fp}`. `fp` is
     FNV-1a 64 over the first and last 64 KiB.
   - Any other folder library (Kontakt without `.nicnt`, UVI folder,
     SFZ/DecentSampler/native): `dir:{fnv(sorted relative preset
     paths)}:{size}:{fp of the first preset}`.

   None of these include the folder name, so renaming a library's folder
   keeps its id.
6. **Auto-relocate.**
   - When a scan finds a library id from the index at a different folder,
     and the old folder no longer exists, the index updates the library's
     folder and appends the old folder to `moved_from`.
   - `Browser` records `(old, new)` in `Prefs.moved`. Recent and starred
     paths under the old folder are rewritten to the new one.
   - The scan covers every root: the player's, auto-discovered, and
     machine-scan results.
7. **Load path resolves moved files.**
   - `Rack::know` takes the index's `(id, folder)` pairs and `Prefs.moved`.
   - `Rack::restore` rewrites each saved part's path, preset and multi
     before loading. If a saved path does not exist, it resolves through
     `(library id, relative path)` in the index, then through
     `kontra_format::moved`.
   - `Rack::load` and `Rack::load_saved` also map a missing instrument path
     through `kontra_format::moved`. Browser preset loads, saved-state
     restore, multis and snapshot presets therefore all load from the new
     place.
8. **Saved state keeps identity.**
   - `SavedPart` gains optional `library` (id) and `relative` (path inside
     the library). `Saved` gains `multi_library` and `multi_relative`.
   - These fields are filled on save when the file belongs to an indexed
     library.
   - The fields use serde defaults, so old states that hold only paths
     still load (`VERSION` stays 1), and the next save upgrades them.
9. **Locate.**
   - The header of an offline library's presets has a "Locate…" button. It
     opens the folder picker (`Wanted::Locate(id)`).
   - `Browser::locate(id, folder)` computes the identity of the chosen
     folder (scanned as a single root). If it matches, it re-links the
     library (index, `Prefs.moved`, recent/starred) and adds the folder as
     a root when no root covers it.
   - If the ids differ, it reports "that folder holds a different library"
     and changes nothing.

## Non-goals

- OS file-watch APIs (inotify, FSEvents, ReadDirectoryChangesW). std
  polling meets the latency we need without a new dependency.
- Incremental *detection*. The library search above libraries still lists
  folders; only the per-library preset walk and bank inspection are
  skipped. (`ponytail:` comment in `scan_with_inspector`.)
- NI registration ids (SNPID) beyond the `.nicnt` product and company.
  The existing `product()` parser yields name and company only.

## Tests

- `index_round_trips_and_keeps_relative_presets` (library)
- `an_unreadable_library_stays_offline_not_dropped` (library)
- `an_unchanged_library_is_not_walked_again` (library)
- `identity_survives_a_folder_rename` (library)
- `a_moved_library_is_relocated_by_identity` (library)
- `a_saved_state_loads_from_a_moved_library` (host): fixture library, save
  state, move the folder, reopen; it resolves and loads from the new path.
- `locate_relinks_by_identity_and_refuses_another_library` (host)
- `old_states_without_identity_still_parse` (host)

## Verification Matrix

| Check | Tier | Target / OS | Exact Command | Pass Criterion | Status / Observed Evidence |
|---|---|---|---|---|---|
| Spec proof gate | Doc | any | `python3 tools/check-spec-proof.py --changed origin/main` | Exit 0, zero proof errors | PASS (2026-11-24, linux-x86_64) |
| Fast unit/lib tests | Compile/ABI | linux-x86_64 | `matari fast kontra -p kontra-library -p kontra-host` (+ `--check-only -p kontra-ui -p kontra-plugin`) | Exit 0, zero failures | PASS (2026-11-24, linux-x86_64: 303 passed, 4 skipped; ui+plugin check clean) |
| Full PR verification gate | Validator/Runtime | linux-x86_64 | `matari check origin/main --project plugins/kontra` | fmt, dbg/todo guard, tests, clippy green | PENDING |
| Real automount wake | Hardware/OS-CI | linux | open KONTRA with an HDD automount asleep, then access it | libraries show offline, then online within 30 s, no reset | PENDING_OS_CI (linux) |

## Unverified Boundaries & Promotion Gate

- The real-hardware automount wake and Flatpak permission grant have not
  been run. Unit tests simulate them with a folder that is unreadable and
  then created. Before `approved`, verify on a real automount.
- Windows and macOS share the code path (std `fs::metadata`) but have not
  run.
