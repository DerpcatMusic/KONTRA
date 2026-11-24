---
status: draft
project: plugins/kontra
owner: claude-opus-5-5
created: 2026-11-24
updated: 2026-11-24
proof_scope: external-api
evidence_freshness_days: 180
---

# Library discovery and visible scan diagnostics

Users report that KONTRA "won't scan anything". New users have no roots,
because roots come only from prefs or v2's `settings.json`. Scan failures
are stored in `Catalog.issues`, but no UI reads them. The fixed depth of 4
also misses deep layouts, and a Flatpak DAW cannot read `/mnt` or `/media`
unless it has been given filesystem permission.

## Acceptance criteria

1. **Per-folder status.** Every scanned folder has a status in Settings →
   Library folders, and the browser's empty state shows the same statuses.
   A folder reports one of:
   - `N libraries found`
   - `can't read: permission denied`
   - `not found`
   - `can't read: <error>`
   - `0 libraries — nothing matched`

   Permission errors (EACCES) and missing paths (ENOENT) are told apart from
   `io::ErrorKind`.
2. **Flatpak hint.** If KONTRA runs inside a Flatpak (`FLATPAK_ID`, or
   `/.flatpak-info` `[Application] name=`) and a folder is unreadable or
   missing, its status includes the exact
   `flatpak override --user --filesystem=<path> <app-id>` command.
3. **Report.** Copy diagnostics / Export… include a "Library scan" section
   that lists each folder's status and hint, followed by the first scan
   issues.
4. **Automatic roots.** Discovered roots are merged with the player's own
   and shown with an "auto" tag. Duplicates and roots the player has hidden
   are removed. Removing an auto root hides it, and the hidden list is
   stored in prefs. Discovery checks:
   - **NI registrations.**
     - Windows: `ContentDir` values from `reg query` under
       `HKLM`/`HKCU\SOFTWARE\Native Instruments`, and Service Center XMLs
       under Common Files.
     - macOS: Service Center XMLs (HFS `Vol:a:b:` paths) and
       `com.native-instruments.*.plist` `ContentDir` values, read through
       `plutil`.
     - Linux: the same data inside Wine prefixes (`~/.wine`, `$WINEPREFIX`,
       Bottles, Flatpak Bottles, Lutris `~/Games/*`). `system.reg` and
       `user.reg` are parsed as text. `Z:\` maps to `/`, `C:\` maps to
       `drive_c`, and any other drive maps through `dosdevices`.
   - **Default content folders.** These are the NI/Kontakt and UVI default
     folders for each OS. UVI Workstation/Falcon preference files are also
     searched for paths to soundbank folders.
   - **Shallow search.** The machine walk runs with limits on every
     discovery, searching home and mounted volumes up to 3 levels deep:
     - Linux: `/proc/self/mounts` on disk filesystems, `/mnt/*`, `/media/*`,
       `/media/$USER/*`, and `/run/media/$USER/*`
     - macOS: `/Volumes/*`
     - Windows: drive letters

     A folder qualifies by the same extension markers as the machine scan.
     Its container becomes a root. The search stops after 1.5 s or 40 000
     folders.
   - **Threading.** Discovery runs on the `kontra-scan` thread, never on the
     audio or UI thread. Tests never run it.
5. **No depth cap.** The catalog scan of every root (a folder the player
   adds, even a drive's root) searches the whole tree on all cores through
   `discover::par_walk`, a std-thread walk with no new dependency.
   `DEPTH = 4` is gone. The scan still prunes sample folders and folders of
   many audio files with no presets. It still stops at a library and
   follows symlinks with canonical cycle guards. The library-internal
   skips (`Documents`, `Data`, `Resources`…) apply only inside a library,
   so a drive's `Documents` folder is searched. A library 7 levels deep is
   found.
6. **Scan my machine.** The browser has a button that walks home and every
   mounted volume in parallel. It matches folders by extension only and
   never opens a file:
   - Kontakt `.nicnt`/`.nki`/`.nkm`/`.nksn`, an `Instruments` folder, or a
     monolith
   - UVI `.ufs`/`.uvip`
   - `.sf2`/`.sf3`, `.sfz`, `.dspreset` and `.kontra`

   The walk does not descend into a strongly marked library, and it skips
   system folders, junk folders (`node_modules`, `Caches`, `AppData`…),
   `Program Files` folders other than NI's and UVI's, hidden folders and
   symlinked folders. Each folder is listed once. Live progress shows the
   folders listed and libraries found, and the walk can be canceled.

   The libraries found are grouped into auto roots and saved in
   `Prefs.found`, and then the normal scan runs. A synthetic test walks
   20 000 folders and prints the walk's speed.
7. **Tests.** Fixture-text tests cover:
   - the `reg query`, `.reg`, Service Center XML, and plist parsers
   - Wine path mapping and HFS path mapping
   - the Flatpak id and hint
   - per-root status with EACCES and ENOENT
   - the 6-deep layout

   No real NI files are needed.

## Non-goals

- Reading binary plists without `plutil`, and reading the registry through
  a Rust registry crate. No dependency is added.
- Exposing more paths to the Flatpak sandbox automatically. KONTRA only
  prints the command.
- Mounting drives, fixing NTFS permissions, or reading UVI's encrypted
  databases.
- Catching every vendor layout. The shallow search is a heuristic with a
  hard budget, and the player can still add folders.

## Primary-Source & Version Proof

| Standard / Vendor API | Pinned Version / Spec Rev | Primary-Source URL | Verified Date | Key Invariants / Deprecations Checked |
|---|---|---|---|---|
| Windows `reg query` | Windows 10/11 | https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/reg-query | 2026-11-24 | output format `NAME    REG_SZ    value`, HKLM/HKCU, `/s` recursion |
| Flatpak sandbox (`/.flatpak-info`, `flatpak override --filesystem`) | Flatpak 1.14+ | https://docs.flatpak.org/en/latest/sandbox-permissions.html | 2026-11-24 | host paths outside the sandbox need a filesystem permission |
| `/proc/self/mounts` | Linux | https://man7.org/linux/man-pages/man5/proc_pid_mounts.5.html | 2026-11-24 | one mount per line, octal-escaped spaces |

## Verification Matrix

| Check | Tier | Target / OS | Exact Command | Pass Criterion | Status / Observed Evidence |
|---|---|---|---|---|---|
| Spec proof gate | Doc | any | `python3 tools/check-spec-proof.py plugins/kontra/docs/specs/library-discovery.md` | Exit 0, zero proof errors | PASS (2026-11-24, linux-x86_64) |
| Fast unit/lib tests | Compile/ABI | linux-x86_64 | `matari fast kontra -p kontra-library -p kontra-host -p kontra-ui -p kontra-plugin` | Exit 0, zero failures | PASS (2026-11-24, linux-x86_64) |
| Full PR verification gate | Validator/Runtime | linux-x86_64 | `matari check origin/main --project plugins/kontra` | fmt, dbg/todo guard, tests, clippy/ratchet green | PENDING |
| Windows registry / Service Center discovery | Hardware/OS-CI | windows | real Native Access install, `Scan my machine` | registered libraries appear as `auto` roots | PENDING_OS_CI (windows) |
| macOS plist / Service Center discovery | Hardware/OS-CI | macos | real Native Access install, `Scan my machine` | registered libraries appear as `auto` roots | PENDING_OS_CI (macos) |

## Unverified Boundaries & Promotion Gate

- Windows and macOS discovery compile but have not run on those OSes.
- macOS `plutil -convert json` (plist reading) has no live primary source yet: Apple's man page archive did not serve; verify before `approved`.
